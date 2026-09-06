"""#235: Slots own every selected planar wall and cylindrical cap patch."""

from __future__ import annotations

import ast
import inspect
import math
from copy import deepcopy
from pathlib import Path

import pytest
from build123d import Box, Compound, Cylinder, Edge, Plane, Pos, Rot, export_step, import_step
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepFeat import BRepFeat_SplitShape
from OCP.GeomAbs import GeomAbs_Cylinder

from quiddity import _recess_features as recess_features
from quiddity import recognise_slots
from quiddity._adjacency import FaceEdges, FaceGraph
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._recess_features import _discover_slots, _SlotAttributionError
from quiddity._recess_records import Slot
from quiddity._recess_reduce import _RecessProposal
from quiddity._registry import PHYSICAL_DEFINITIONS, FullyAttributed
from quiddity._run import start
from quiddity.result import _discover_all, _take_inventory

ROOT = Path(__file__).parents[1]
_AXIS = {"x": 0, "y": 1, "z": 2}


def _obround(length: float, width: float, depth: float):
    end = Cylinder(width / 2, depth)
    return Box(length, width, depth) + Pos(-length / 2, 0, 0) * end + Pos(length / 2, 0, 0) * end


def _body_key(solid) -> tuple[float, ...]:
    # Independently project the documented #517 key policy, not raw kernel last bits.
    box = solid.bounding_box()
    return (
        *(round(float(value), 6) or 0.0 for value in (*tuple(box.min), *tuple(box.max))),
        float(f"{solid.volume:.12g}"),
        float(f"{solid.area:.12g}"),
    )


def _empty_prism(part, spans: dict[int, tuple[float, float]]) -> bool:
    sizes = [spans[i][1] - spans[i][0] - 2e-7 for i in range(3)]
    if min(sizes) <= 0:
        return False
    centre = [(spans[i][0] + spans[i][1]) / 2 for i in range(3)]
    probe = Pos(*centre) * Box(*sizes)
    intersection = part.intersect(probe)
    if intersection is None:
        return True
    volume = (
        intersection.volume
        if hasattr(intersection, "volume")
        else sum(shape.volume for shape in intersection)
    )
    return volume == pytest.approx(0.0, abs=1e-10)


def _fresh_occurrences_one(solid, body_key):
    """Independent OCP/AAG reconstruction; no recogniser or reducer calls."""
    graph = FaceGraph(solid)
    box = solid.bounding_box()
    extent = (box.size.X, box.size.Y, box.size.Z)
    planes = []
    cylinders = []
    for node in graph.nodes:
        bounds = graph.bounds(node)
        if graph.is_planar(node):
            normal = graph.normal(node)
            if normal is None:
                continue
            axis = max(range(3), key=lambda index: abs(normal[index]))
            if abs(normal[axis]) >= 0.999999:
                planes.append((node, axis, normal[axis], bounds))
            continue
        adaptor = BRepAdaptor_Surface(graph.face(node).wrapped)
        if adaptor.GetType() != GeomAbs_Cylinder:
            continue
        cylinder = adaptor.Cylinder()
        direction = cylinder.Axis().Direction()
        vector = (direction.X(), direction.Y(), direction.Z())
        axis = max(range(3), key=lambda index: abs(vector[index]))
        if abs(vector[axis]) < 0.999999:
            continue
        location = cylinder.Location()
        cylinders.append(
            (node, axis, cylinder.Radius(), (location.X(), location.Y(), location.Z()), bounds)
        )

    raw = []
    for left_index, left in enumerate(planes):
        for right in planes[left_index + 1 :]:
            ln, width_axis, lsign, lb = left
            rn, raxis, rsign, rb = right
            if width_axis != raxis or lsign * rsign >= 0:
                continue
            la = sum(lb[width_axis]) / 2
            ra = sum(rb[width_axis]) / 2
            if (ra - la) * lsign <= 0:
                ln, rn, lsign, rsign, lb, rb, la, ra = rn, ln, rsign, lsign, rb, lb, ra, la
            if (ra - la) * lsign <= 0:
                continue
            others = [axis for axis in range(3) if axis != width_axis]
            overlaps = [
                (axis, min(lb[axis][1], rb[axis][1]) - max(lb[axis][0], rb[axis][0]))
                for axis in others
            ]
            if min(value for _axis, value in overlaps) <= 0:
                continue
            overlaps.sort(key=lambda item: item[1], reverse=True)
            (long_axis, length), (depth_axis, _depth) = overlaps
            if (
                overlaps[0][1] - overlaps[1][1] <= 0.05 * overlaps[0][1]
                and extent[depth_axis] > extent[long_axis]
            ):
                long_axis, depth_axis = depth_axis, long_axis
                length = overlaps[1][1]
            width = abs(ra - la)
            if width > length or length >= 0.9 * extent[long_axis]:
                continue
            spans = {
                width_axis: tuple(sorted((la, ra))),
                long_axis: (
                    max(lb[long_axis][0], rb[long_axis][0]),
                    min(lb[long_axis][1], rb[long_axis][1]),
                ),
                depth_axis: (
                    max(lb[depth_axis][0], rb[depth_axis][0]),
                    min(lb[depth_axis][1], rb[depth_axis][1]),
                ),
            }
            common = set(graph.neighbours(ln)) & set(graph.neighbours(rn))
            if not common or not all(
                graph.arc(ln, node) == graph.arc(rn, node)
                for node in common
                if graph.is_planar(node)
            ):
                continue
            if not _empty_prism(solid, spans):
                continue
            record = Slot(
                "xyz"[width_axis],
                "xyz"[long_axis],
                round(width, 2),
                round(spans[long_axis][1] - spans[long_axis][0], 2),
                round((la + ra) / 2, 2),
                round(spans[long_axis][0], 2),
                round(spans[long_axis][1], 2),
                round(spans[depth_axis][0], 2),
                round(spans[depth_axis][1], 2),
                body_key,
            )
            raw.append([record, {ln, rn}, []])

    # Same void through an orthogonal wall pair: retain the narrower public record and union roles.
    merged = []
    for item in sorted(raw, key=lambda item: (item[0].width, item[0].location)):
        keeper = next(
            (old for old in merged if math.dist(old[0].location, item[0].location) <= 0.1), None
        )
        if keeper is None:
            merged.append(item)
        else:
            keeper[1].update(item[1])

    # A crossing void splits each run into collinear arms. Rejoin only when the entire
    # intervening prism is empty; solid-separated equal channels remain distinct.
    changed = True
    while changed:
        changed = False
        for left_index, left in enumerate(merged):
            a = left[0]
            for right_index in range(left_index + 1, len(merged)):
                right = merged[right_index]
                b = right[0]
                if (
                    a.width_axis != b.width_axis
                    or a.long_axis != b.long_axis
                    or abs(a.width - b.width) > 0.1
                    or abs(a.w_center - b.w_center) > 0.1
                    or abs(a.d_lo - b.d_lo) > 0.1
                    or abs(a.d_hi - b.d_hi) > 0.1
                ):
                    continue
                if a.hi <= b.lo:
                    gap = (a.hi, b.lo)
                elif b.hi <= a.lo:
                    gap = (b.hi, a.lo)
                else:
                    continue
                axis = _AXIS[a.long_axis]
                width_axis = _AXIS[a.width_axis]
                depth_axis = _AXIS[a.depth_axis]
                spans = {
                    axis: gap,
                    width_axis: (a.w_center - a.width / 2, a.w_center + a.width / 2),
                    depth_axis: (a.d_lo, a.d_hi),
                }
                if not _empty_prism(solid, spans):
                    continue
                lo, hi = min(a.lo, b.lo), max(a.hi, b.hi)
                left[0] = Slot(
                    a.width_axis,
                    a.long_axis,
                    a.width,
                    round(hi - lo, 2),
                    a.w_center,
                    round(lo, 2),
                    round(hi, 2),
                    a.d_lo,
                    a.d_hi,
                    body_key,
                )
                left[1].update(right[1])
                left[2].extend(group for group in right[2] if group not in left[2])
                merged.pop(right_index)
                changed = True
                break
            if changed:
                break

    # Cylindrical endpoint regions independently establish obround records and cap patch groups.
    cap_regions = {}
    for node, depth_axis, radius, centre, bounds in cylinders:
        key = (
            depth_axis,
            round(radius, 7),
            tuple(round(value, 7) for value in centre),
            tuple(round(v, 7) for v in bounds[depth_axis]),
        )
        cap_regions.setdefault(key, set()).add(node)
    regions = [(key, nodes) for key, nodes in cap_regions.items()]
    for index, (left_key, left_nodes) in enumerate(regions):
        for right_key, right_nodes in regions[index + 1 :]:
            depth_axis, radius, lc, depth = left_key
            r_depth_axis, r_radius, rc, r_depth = right_key
            if depth_axis != r_depth_axis or radius != r_radius or depth != r_depth:
                continue
            deltas = [abs(rc[axis] - lc[axis]) for axis in range(3)]
            long_axis = max(range(3), key=deltas.__getitem__)
            if long_axis == depth_axis or deltas[long_axis] <= 0:
                continue
            width_axis = next(axis for axis in range(3) if axis not in (long_axis, depth_axis))
            if abs(lc[width_axis] - rc[width_axis]) > 1e-6:
                continue
            lo = min(lc[long_axis], rc[long_axis]) - radius
            hi = max(lc[long_axis], rc[long_axis]) + radius
            record = Slot(
                "xyz"[width_axis],
                "xyz"[long_axis],
                round(2 * radius, 2),
                round(hi - lo, 2),
                round((lc[width_axis] + rc[width_axis]) / 2, 2),
                round(lo, 2),
                round(hi, 2),
                round(depth[0], 2),
                round(depth[1], 2),
                body_key,
            )
            # Slot recovery is the zero-floor route: its cap patches traverse the
            # entire owner depth. A blind obround has the same plan view but fails here.
            owner_depth = (getattr(box.min, "XYZ"[depth_axis]), getattr(box.max, "XYZ"[depth_axis]))
            if depth[0] != pytest.approx(owner_depth[0]) or depth[1] != pytest.approx(
                owner_depth[1]
            ):
                continue
            cap_nodes = left_nodes | right_nodes
            # Each endpoint patch belongs to the same smooth cylindrical boundary and
            # joins the planar side continuation tangentially. Stock cylinders without
            # that endpoint connectivity are not obround cap evidence.
            if not all(
                any(graph.arc(node, neighbour) == "smooth" for neighbour in graph.neighbours(node))
                for node in cap_nodes
            ):
                continue
            keeper = next(
                (
                    old
                    for old in merged
                    if old[0].width_axis == record.width_axis
                    and old[0].long_axis == record.long_axis
                    and old[0].width == record.width
                    and old[0].w_center == record.w_center
                    and old[0].d_lo == record.d_lo
                    and old[0].d_hi == record.d_hi
                    and math.dist(old[0].location, record.location) <= 0.1
                ),
                None,
            )
            groups = [frozenset(left_nodes), frozenset(right_nodes)]
            if keeper is None:
                merged.append([record, set(), groups])
            else:
                keeper[0] = record
                keeper[2] = groups
    return graph, merged


def _expected(part):
    """Derive occurrences, values, dedup, order and roles before Candidate inspection."""
    aggregate = FaceGraph(part)
    solids = list(part.solids())
    sources = solids if len(solids) > 1 else [part]
    keys = [_body_key(solid) for solid in sources]
    expected = []
    for solid, key in zip(sources, keys, strict=True):
        local, occurrences = _fresh_occurrences_one(solid, key if keys.count(key) == 1 else None)
        for record, walls, groups in occurrences:
            mapped_walls = frozenset(aggregate.require_node(local.face(node)) for node in walls)
            mapped_groups = tuple(
                frozenset(aggregate.require_node(local.face(node)) for node in group)
                for group in groups
            )
            nodes = frozenset((*mapped_walls, *(node for group in mapped_groups for node in group)))
            expected.append((record, nodes, mapped_walls, mapped_groups))
    expected.sort(key=lambda item: (item[0].width, item[0].location))
    return aggregate, expected


@pytest.mark.parametrize(
    ("part", "planar", "caps"),
    [
        (Box(80, 50, 16) - Box(28, 10, 16), 2, 0),
        (Box(120, 60, 20) - Box(20, 20, 20), 4, 0),
        (Box(120, 120, 20) - Box(60, 14, 20) - Box(14, 60, 20), 4, 0),
        (Box(100, 60, 20) - _obround(30, 12, 20), 2, 2),
        (Box(100, 60, 20) - _obround(3, 12, 20), 0, 2),
    ],
)
def test_route_matrix_matches_fresh_complete_role_inventory(part, planar: int, caps: int) -> None:
    fresh, expected = _expected(part)
    product = _take_inventory(part)
    candidates = product.physical.candidate_set(FamilyId.SLOTS).candidates
    records = tuple(candidate.record for candidate in candidates)
    assert [record.to_dict() for record in recognise_slots(part)] == [
        item[0].to_dict() for item in expected
    ]
    assert len(records) == len(candidates) == len(expected)
    for candidate, record, (_want, nodes, walls, groups) in zip(
        candidates, records, expected, strict=True
    ):
        assert candidate.record is record
        actual = product.evidence.defining_of(candidate)
        assert len(walls) == planar and len(groups) == caps
        assert len(actual) == len(nodes)
        expected_faces = [fresh.face(node) for node in nodes]
        actual_faces = [product.context.graph.face(node) for node in actual]
        assert all(any(face.is_same(want) for face in actual_faces) for want in expected_faces)
        assert product.context.graph.common_valid_solid(actual) is not None
        for node in actual:
            if product.context.graph.is_planar(node):
                normal = product.context.graph.normal(node)
                assert normal is not None
                axis = max(range(3), key=lambda index: abs(normal[index]))
                assert axis in {_AXIS[record.width_axis], _AXIS[record.long_axis]}
                assert abs(normal[axis]) == pytest.approx(1.0)
                lo, hi = product.context.graph.bounds(node)[axis]
                assert lo == pytest.approx(hi)
                if axis == _AXIS[record.width_axis]:
                    assert abs(lo - record.w_center) == pytest.approx(record.width / 2)
                else:
                    assert abs(lo - (record.lo + record.hi) / 2) == pytest.approx(record.length / 2)
            else:
                surface = BRepAdaptor_Surface(product.context.graph.face(node).wrapped)
                assert surface.GetType() == GeomAbs_Cylinder
                cylinder = surface.Cylinder()
                direction = cylinder.Axis().Direction()
                components = (abs(direction.X()), abs(direction.Y()), abs(direction.Z()))
                assert components[_AXIS[record.depth_axis]] == pytest.approx(1.0)
                assert cylinder.Radius() == pytest.approx(record.width / 2)


def test_public_claim_ledger_and_writer_use_the_same_complete_product() -> None:
    part = Box(100, 60, 20) - _obround(30, 12, 20)
    public_ledger = ClaimLedger(FaceGraph(part))
    via_ledger = recognise_slots(part, ledger=public_ledger)
    writer_ledger = ClaimLedger(FaceGraph(part))
    via_writer = recognise_slots(part, ledger=writer_ledger.writer)
    plain = recognise_slots(part)
    assert [item.to_dict() for item in via_ledger] == [item.to_dict() for item in plain]
    assert [item.to_dict() for item in via_writer] == [item.to_dict() for item in plain]
    assert [len(claim.defining) for claim in public_ledger.claims] == [4]
    assert [len(claim.defining) for claim in writer_ledger.claims] == [4]


def test_step_split_cap_publishes_every_original_patch(tmp_path: Path) -> None:
    part = Box(100, 60, 20) - _obround(30, 12, 20)
    graph, expected = _expected(part)
    _record, _nodes, _walls, cap_groups = expected[0]
    cap_node = max(
        (node for group in cap_groups for node in group), key=lambda node: graph.bounds(node)[0][1]
    )
    face = graph.face(cap_node)
    bounds = face.bounding_box()
    seam = Edge.make_line((21, 0, bounds.min.Z), (21, 0, bounds.max.Z))
    splitter = BRepFeat_SplitShape(part.wrapped)
    splitter.Add(seam.wrapped, face.wrapped)
    splitter.Build()
    assert splitter.IsDone()
    split = type(part).cast(splitter.Shape())
    path = tmp_path / "slot-split-cap.step"
    assert export_step(split, path)
    imported = import_step(path)
    ledger = ClaimLedger(FaceGraph(imported))
    (record,) = _discover_slots(imported, writer=ledger.writer)
    (candidate,) = ledger.candidate_set(FamilyId.SLOTS).candidates
    defining = ledger.defining_of(candidate)
    assert candidate.record is record
    assert sum(not ledger.graph.is_planar(node) for node in defining) == 3
    assert sum(ledger.graph.is_planar(node) for node in defining) == 2


def test_equal_coincident_and_separate_occurrences_keep_identity_and_body_scope() -> None:
    first = Box(80, 50, 16) - Box(28, 10, 16)
    for part in (
        Compound([first, deepcopy(first)]),
        Compound([first, Pos(150, 0, 0) * deepcopy(first)]),
    ):
        ledger = ClaimLedger(FaceGraph(part))
        records = _discover_slots(part, writer=ledger.writer)
        candidates = ledger.candidate_set(FamilyId.SLOTS).candidates
        assert len(records) == len(candidates) == 2
        assert all(
            candidate.record is record
            for candidate, record in zip(candidates, records, strict=True)
        )
        roles = [ledger.defining_of(candidate) for candidate in candidates]
        assert roles[0].isdisjoint(roles[1])
        assert all(ledger.graph.common_valid_solid(nodes) is not None for nodes in roles)


def test_same_body_equal_size_occurrences_and_traversal_keep_exact_identity(monkeypatch) -> None:
    stock = Box(120, 70, 20)
    part = stock - Pos(-32, 0, 0) * Box(24, 10, 20) - Pos(32, 0, 0) * Box(24, 10, 20)
    fresh, expected = _expected(part)
    assert len(expected) == 2
    ledger = ClaimLedger(FaceGraph(part))
    records = _discover_slots(part, writer=ledger.writer)
    candidates = ledger.candidate_set(FamilyId.SLOTS).candidates
    assert [record.to_dict() for record in records] == [item[0].to_dict() for item in expected]
    assert records[0].width == records[1].width
    assert records[0].length == records[1].length
    roles = [ledger.defining_of(candidate) for candidate in candidates]
    assert roles[0].isdisjoint(roles[1])
    assert all(
        {fresh.require_node(ledger.graph.face(node)) for node in actual} == set(want[1])
        for actual, want in zip(roles, expected, strict=True)
    )

    original = type(part).faces
    monkeypatch.setattr(type(part), "faces", lambda self: list(reversed(original(self))))
    reversed_ledger = ClaimLedger(FaceGraph(part))
    reversed_records = _discover_slots(part, writer=reversed_ledger.writer)
    assert [record.to_dict() for record in reversed_records] == [
        record.to_dict() for record in records
    ]
    assert [
        frozenset(reversed_ledger.graph.bounds(node) for node in reversed_ledger.defining_of(item))
        for item in reversed_ledger.candidate_set(FamilyId.SLOTS).candidates
    ] == [frozenset(ledger.graph.bounds(node) for node in nodes) for nodes in roles]


def test_checked_nested_slots_share_truthful_wall_evidence_on_one_solid() -> None:
    part = import_step(ROOT / "tests/corpus/mfcadpp/10190.step")
    product = _take_inventory(part)
    candidates = product.physical.candidate_set(FamilyId.SLOTS).candidates
    assert candidates
    shared = []
    for index, left in enumerate(candidates):
        left_nodes = product.evidence.defining_of(left)
        for right in candidates[index + 1 :]:
            right_nodes = product.evidence.defining_of(right)
            overlap = left_nodes & right_nodes
            if overlap:
                shared.append((left, right, overlap))
                assert left.record != right.record
                assert product.context.graph.common_valid_solid(left_nodes) == (
                    product.context.graph.common_valid_solid(right_nodes)
                )
    assert shared


def test_same_record_competing_role_sets_still_refuse_atomically(monkeypatch) -> None:
    part = Box(80, 50, 16) - Box(28, 10, 16)
    context = start(part)
    ledger = ClaimLedger(context.graph, definitions=PHYSICAL_DEFINITIONS)
    (proposal,) = recess_features._body_scoped_proposals(
        [part], lambda solid: recess_features._slot_proposals_one(solid, graph=ledger.graph)
    )
    disjoint = frozenset(node for node in ledger.graph.nodes if node not in proposal.planar)
    assert disjoint and disjoint.isdisjoint(proposal.planar)
    assert ledger.graph.common_valid_solid(disjoint) is not None
    competing = _RecessProposal(proposal.record, disjoint, ())
    monkeypatch.setattr(
        recess_features,
        "_body_scoped_proposals",
        lambda _sources, _recognise_one: [proposal, competing],
    )
    with pytest.raises(_SlotAttributionError, match="competing source roles"):
        _discover_all(context, ledger)
    assert ledger.candidate_set(FamilyId.SLOTS).candidates == ()
    assert FamilyId.SLOTS not in ledger._issuer._completed
    assert FamilyId.SLOTS not in ledger._issuer._completed_occurrences


def test_graph_identical_proposal_duplicate_collapses_before_issue(monkeypatch) -> None:
    part = Box(80, 50, 16) - Box(28, 10, 16)
    ledger = ClaimLedger(FaceGraph(part))
    (proposal,) = recess_features._body_scoped_proposals(
        [part], lambda solid: recess_features._slot_proposals_one(solid, graph=ledger.graph)
    )
    monkeypatch.setattr(
        recess_features,
        "_body_scoped_proposals",
        lambda _sources, _recognise_one: [proposal, proposal],
    )
    (record,) = _discover_slots(part, writer=ledger.writer)
    (candidate,) = ledger.candidate_set(FamilyId.SLOTS).candidates
    assert candidate.record is record
    assert ledger.defining_of(candidate) == frozenset(proposal.planar)


def test_empty_complete_role_set_refuses_before_issue(monkeypatch) -> None:
    part = Box(80, 50, 16) - Box(28, 10, 16)
    ledger = ClaimLedger(FaceGraph(part))
    (proposal,) = recess_features._body_scoped_proposals(
        [part], lambda solid: recess_features._slot_proposals_one(solid, graph=ledger.graph)
    )
    monkeypatch.setattr(
        recess_features,
        "_body_scoped_proposals",
        lambda _sources, _recognise_one: [_RecessProposal(proposal.record, frozenset(), ())],
    )
    with pytest.raises(_SlotAttributionError, match="no defining source faces"):
        _discover_slots(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.SLOTS).candidates == ()


def test_competing_cap_clusters_are_a_closed_atomic_attribution_failure(monkeypatch) -> None:
    part = Box(100, 60, 20) - _obround(30, 12, 20)
    ledger = ClaimLedger(FaceGraph(part))

    def compete(_sources, _recognise_one):
        raise ValueError("obround cap clusters compete at one endpoint")

    monkeypatch.setattr(recess_features, "_body_scoped_proposals", compete)
    with pytest.raises(_SlotAttributionError, match="cap ownership is ambiguous"):
        _discover_slots(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.SLOTS).candidates == ()


def test_unrelated_geometry_value_error_is_not_relabelled(monkeypatch) -> None:
    part = Box(100, 60, 20) - _obround(30, 12, 20)
    ledger = ClaimLedger(FaceGraph(part))

    def geometry_failure(_sources, _recognise_one):
        raise ValueError("kernel classification failed")

    monkeypatch.setattr(recess_features, "_body_scoped_proposals", geometry_failure)
    with pytest.raises(ValueError, match="kernel classification failed"):
        _discover_slots(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.SLOTS).candidates == ()


def test_shared_node_across_solidrefs_refuses_before_issue(monkeypatch) -> None:
    first = Box(80, 50, 16) - Box(28, 10, 16)
    part = Compound([first, Pos(150, 0, 0) * deepcopy(first)])
    ledger = ClaimLedger(FaceGraph(part))
    sources = list(part.solids())
    proposals = recess_features._body_scoped_proposals(
        sources,
        lambda solid: recess_features._slot_proposals_one(solid, graph=ledger.graph),
    )
    assert len(proposals) == 2
    shared = next(iter(proposals[0].planar))
    mixed = _RecessProposal(
        proposals[1].record,
        proposals[1].planar | {shared},
        proposals[1].caps,
    )
    monkeypatch.setattr(
        recess_features,
        "_body_scoped_proposals",
        lambda _sources, _recognise_one: [proposals[0], mixed],
    )
    with pytest.raises(_SlotAttributionError, match="one valid solid"):
        _discover_slots(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.SLOTS).candidates == ()


def test_shared_node_with_conflicting_issuer_solidrefs_refuses_atomically(monkeypatch) -> None:
    first = Box(80, 50, 16) - Box(28, 10, 16)
    part = Compound([first, Pos(150, 0, 0) * deepcopy(first)])
    ledger = ClaimLedger(FaceGraph(part))
    proposals = recess_features._body_scoped_proposals(
        list(part.solids()),
        lambda solid: recess_features._slot_proposals_one(solid, graph=ledger.graph),
    )
    assert len(proposals) == 2
    owners = [ledger.graph.common_valid_solid(proposal.planar) for proposal in proposals]
    assert owners[0] is not None and owners[1] is not None and owners[0] != owners[1]
    shared = next(iter(proposals[0].planar))
    mixed = _RecessProposal(
        proposals[1].record,
        proposals[1].planar | {shared},
        proposals[1].caps,
    )
    monkeypatch.setattr(
        recess_features,
        "_body_scoped_proposals",
        lambda _sources, _recognise_one: [proposals[0], mixed],
    )
    monkeypatch.setattr(
        ledger.graph,
        "common_valid_solid",
        lambda nodes: owners[0] if frozenset(nodes) == proposals[0].planar else owners[1],
    )
    with pytest.raises(_SlotAttributionError, match="ambiguously reused"):
        _discover_slots(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.SLOTS).candidates == ()


@pytest.mark.parametrize("wrap", [True, False])
def test_candidate_node_resolution_preserves_the_public_error_boundary(monkeypatch, wrap) -> None:
    part = Box(80, 50, 16) - Box(28, 10, 16)
    ledger = ClaimLedger(FaceGraph(part))

    def stale(_node):
        raise KeyError("stale issued node")

    monkeypatch.setattr(ledger.graph, "face", stale)
    error = _SlotAttributionError if wrap else KeyError
    with pytest.raises(error):
        _discover_slots(part, writer=ledger.writer, _wrap_identity_errors=wrap)
    assert ledger.candidate_set(FamilyId.SLOTS).candidates == ()


def test_public_compatibility_path_does_not_relabel_source_identity(monkeypatch) -> None:
    part = Box(80, 50, 16) - Box(28, 10, 16)
    ledger = ClaimLedger(FaceGraph(part))

    def stale(_face):
        raise ValueError("foreign source face")

    monkeypatch.setattr(ledger.graph, "require_node", stale)
    with pytest.raises(ValueError, match="foreign source face"):
        recognise_slots(part, ledger=ledger)
    assert ledger.candidate_set(FamilyId.SLOTS).candidates == ()


@pytest.mark.parametrize(
    "foreign_part",
    [
        deepcopy(Box(80, 50, 16) - Box(28, 10, 16)),
        Pos(0.25, 0, 0) * (Box(80, 50, 16) - Box(28, 10, 16)),
    ],
)
def test_deep_and_translated_stale_graphs_refuse_without_prefix(foreign_part) -> None:
    part = Box(80, 50, 16) - Box(28, 10, 16)
    ledger = ClaimLedger(FaceGraph(foreign_part))
    with pytest.raises((_SlotAttributionError, ValueError)):
        _discover_slots(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.SLOTS).candidates == ()


@pytest.mark.parametrize(("length", "accepted"), [(89.99, True), (90.0, False), (90.01, False)])
def test_max_span_boundary_is_strict_and_writer_matches_public(
    length: float, accepted: bool
) -> None:
    part = Box(100, 60, 20) - Box(length, 10, 20)
    ledger = ClaimLedger(FaceGraph(part))
    records = _discover_slots(part, writer=ledger.writer)
    assert bool(records) is accepted
    assert records == recognise_slots(part)
    assert bool(ledger.candidate_set(FamilyId.SLOTS).candidates) is accepted


def test_solid_membrane_d_cutout_and_blind_obround_do_not_leak() -> None:
    membrane = Box(120, 60, 20) - Pos(-25, 0, 0) * Box(30, 10, 20) - Pos(25, 0, 0) * Box(30, 10, 20)
    d_cutout = Box(100, 60, 20) - (Box(18, 12, 20) + Pos(9, 0, 0) * Cylinder(6, 20))
    blind = Box(100, 60, 20) - Pos(0, 0, 5) * _obround(30, 12, 10)
    membrane_ledger = ClaimLedger(FaceGraph(membrane))
    membrane_records = _discover_slots(membrane, writer=membrane_ledger.writer)
    assert len(membrane_records) == 2
    assert len(membrane_ledger.candidate_set(FamilyId.SLOTS).candidates) == 2
    for part in (d_cutout, blind):
        ledger = ClaimLedger(FaceGraph(part))
        assert _discover_slots(part, writer=ledger.writer) == recognise_slots(part) == []
        assert ledger.candidate_set(FamilyId.SLOTS).candidates == ()


@pytest.mark.parametrize(
    "part",
    [
        Pos(37, -19, 11) * (Box(80, 50, 16) - Box(28, 10, 16)),
        Rot(90, 0, 0) * (Box(80, 50, 16) - Box(28, 10, 16)),
        Rot(0, 90, 0) * (Box(80, 50, 16) - Box(28, 10, 16)),
        (Box(80, 50, 16) - Box(28, 10, 16)).mirror(Plane.YZ),
        (Box(80, 50, 16) - Box(28, 10, 16)).scale(0.2),
        (Box(80, 50, 16) - Box(28, 10, 16)).scale(5),
        Rot(90, 0, 0) * (Box(100, 60, 20) - _obround(30, 12, 20)),
    ],
)
def test_principal_axes_translation_mirror_and_scale_preserve_complete_roles(part) -> None:
    ledger = ClaimLedger(FaceGraph(part))
    records = _discover_slots(part, writer=ledger.writer)
    assert [record.to_dict() for record in records] == [
        record.to_dict() for record in recognise_slots(part)
    ]
    candidates = ledger.candidate_set(FamilyId.SLOTS).candidates
    assert len(records) == len(candidates) == 1
    assert candidates[0].record is records[0]
    assert ledger.defining_of(candidates[0])
    assert ledger.graph.common_valid_solid(ledger.defining_of(candidates[0])) is not None


@pytest.mark.parametrize(
    "part",
    [
        Box(80, 60, 20) - Pos(0, 0, 5) * Box(30, 10, 10),  # floored Pocket
        Box(80, 60, 20) - Cylinder(6, 20),  # full cylindrical hole
        Box(80, 60, 20),
    ],
)
def test_non_slot_controls_publish_nothing(part) -> None:
    ledger = ClaimLedger(FaceGraph(part))
    assert _discover_slots(part, writer=ledger.writer) == []
    assert ledger.candidate_set(FamilyId.SLOTS).candidates == ()


def test_shared_graph_face_edges_and_foreign_authority_boundaries() -> None:
    part = Box(80, 50, 16) - Box(28, 10, 16)
    memo = FaceEdges()
    ledger = ClaimLedger(FaceGraph(part, face_edges=memo))
    assert _discover_slots(part, face_edges=memo, graph=ledger.graph) == recognise_slots(
        part, face_edges=memo
    )
    assert _discover_slots(part, face_edges=memo, writer=ledger.writer)
    foreign = ClaimLedger(FaceGraph(Pos(100, 0, 0) * part))
    with pytest.raises(_SlotAttributionError):
        _discover_slots(part, writer=foreign.writer)
    assert foreign.candidate_set(FamilyId.SLOTS).candidates == ()
    with pytest.raises(_SlotAttributionError, match="one authority"):
        _discover_slots(part, graph=ledger.graph, writer=foreign.writer)


def test_late_second_body_failure_is_atomic_and_uncompleted(monkeypatch) -> None:
    first = Box(80, 50, 16) - Box(28, 10, 16)
    part = Compound([first, Pos(150, 0, 0) * deepcopy(first)])
    context = start(part)
    ledger = ClaimLedger(context.graph, definitions=PHYSICAL_DEFINITIONS)
    real = ledger.graph.common_valid_solid
    owners = []

    def fail_second(nodes):
        owner = real(nodes)
        if owner is not None and owner not in owners:
            owners.append(owner)
        return None if len(owners) > 1 else owner

    monkeypatch.setattr(ledger.graph, "common_valid_solid", fail_second)
    with pytest.raises(_SlotAttributionError, match="one valid solid"):
        _discover_all(context, ledger)
    assert ledger.candidate_set(FamilyId.SLOTS).candidates == ()
    assert FamilyId.SLOTS not in ledger._issuer._completed
    assert FamilyId.SLOTS not in ledger._issuer._completed_occurrences


def test_status_registry_writer_and_private_module_seams_are_closed() -> None:
    definition = next(item for item in PHYSICAL_DEFINITIONS if item.family is FamilyId.SLOTS)
    assert isinstance(definition.attribution, FullyAttributed)
    package = ROOT / "src/quiddity"
    callers = []
    importers = []

    def bindings(tree):
        names = {}
        modules = {}
        for node in ast.walk(tree):
            if isinstance(node, ast.ImportFrom):
                for alias in node.names:
                    names[alias.asname or alias.name] = alias.name
            elif isinstance(node, ast.Import):
                for alias in node.names:
                    modules[alias.asname or alias.name.split(".")[0]] = alias.name
        return names, modules

    def called_leaf(node, names, modules):
        if isinstance(node, ast.Name):
            return names.get(node.id, node.id)
        if isinstance(node, ast.Attribute):
            if isinstance(node.value, ast.Name) and node.value.id in modules:
                return node.attr
            return node.attr
        return ""

    for path in package.glob("*.py"):
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        names, modules = bindings(tree)
        if any(
            isinstance(node, ast.ImportFrom)
            and any(alias.name == "_discover_slots" for alias in node.names)
            for node in ast.walk(tree)
        ):
            importers.append(path.name)
        for node in ast.walk(tree):
            if (
                isinstance(node, ast.Call)
                and called_leaf(node.func, names, modules) == "_discover_slots"
            ):
                callers.append((path.name, node))
    assert importers == ["_registry.py"]
    assert {path for path, _call in callers} == {"_registry.py", "_recess_features.py"}
    registry_call = next(call for path, call in callers if path == "_registry.py")
    writer = {item.arg: item.value for item in registry_call.keywords}["writer"]
    assert isinstance(writer, ast.Attribute) and writer.attr == "writer"
    assert isinstance(writer.value, ast.Name) and writer.value.id == "s"
    assert tuple(inspect.signature(recognise_slots).parameters) == (
        "part",
        "face_edges",
        "ledger",
    )

    watched = {
        "_slot_proposals_one",
        "_body_scoped_proposals",
        "_RecessProposal",
        "_merge_proposals",
        "_collapse_collinear_proposals",
        "_extend_obround_proposals",
    }
    sites = {name: [] for name in watched}
    for path in package.glob("*.py"):
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        names, modules = bindings(tree)

        class Roster(ast.NodeVisitor):
            def __init__(self, source_name, imported_names, imported_modules):
                self.source_name = source_name
                self.functions = []
                self.imported_names = imported_names
                self.imported_modules = imported_modules

            def visit_FunctionDef(self, node):
                self.functions.append(node.name)
                self.generic_visit(node)
                self.functions.pop()

            def visit_Call(self, node):
                leaf = called_leaf(node.func, self.imported_names, self.imported_modules)
                if leaf in sites:
                    sites[leaf].append((self.source_name, self.functions[-1]))
                self.generic_visit(node)

        Roster(path.name, names, modules).visit(tree)
    assert sites["_slot_proposals_one"] == [
        ("_recess_core.py", "_recognise_slots_one"),
    ]
    assert sites["_body_scoped_proposals"] == [
        ("_recess_features.py", "_discover_slots"),
        ("_recess_features.py", "_discover_slots"),
        ("_recess_features.py", "_discover_pockets"),
    ]
    assert {path for path, _function in sites["_RecessProposal"]} == {
        "_recess_core.py",
        "_recess_obround.py",
        "_recess_reduce.py",
    }
    assert {
        function for path, function in sites["_merge_proposals"] if path == "_recess_core.py"
    } == {
        "_slot_proposals_one",
        "_pocket_proposals_one",
    }
    assert sites["_collapse_collinear_proposals"] == [("_recess_core.py", "_slot_proposals_one")]
    assert sites["_extend_obround_proposals"] == [
        ("_recess_core.py", "_slot_proposals_one"),
        ("_recess_core.py", "_pocket_proposals_one"),
    ]

    prohibited = {
        "CandidateSet",
        "EvidenceIndex",
        "InventoryProduct",
        "ReconciliationResult",
        "CompletedInputs",
        "candidate_set",
        "accepted_set",
        "disposition",
    }
    for module_name in (
        "_recess_core.py",
        "_recess_faces.py",
        "_recess_obround.py",
        "_recess_reduce.py",
        "_recess_features.py",
    ):
        tree = ast.parse((package / module_name).read_text(encoding="utf-8"))
        names, _modules = bindings(tree)
        referenced = {
            names.get(node.id, node.id) for node in ast.walk(tree) if isinstance(node, ast.Name)
        } | {node.attr for node in ast.walk(tree) if isinstance(node, ast.Attribute)}
        assert prohibited.isdisjoint(referenced), (module_name, prohibited & referenced)
