"""#234 neutral Slot/Pocket occurrence and cylindrical-cap provenance."""

from __future__ import annotations

import ast
import inspect
from copy import deepcopy
from dataclasses import replace
from functools import partial
from itertools import permutations
from pathlib import Path
from typing import Any, cast

import pytest
from build123d import Box, Compound, Cylinder, Edge, Pos, Rot, export_step, import_step
from OCP.BRepFeat import BRepFeat_SplitShape

from quiddity import recognise_slots
from quiddity._adjacency import FaceEdges, FaceGraph
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._geometry import length_tol
from quiddity._recess_core import (
    _corner_notch_proposals,
    _pocket_proposals_one,
    _recognise_corner_notches,
    _recognise_pockets_one,
    _recognise_slots_one,
    _slot_proposals_one,
)
from quiddity._recess_faces import (
    _cylinder_faces,
    _planar_faces,
)
from quiddity._recess_features import _discover_pockets, _discover_slots
from quiddity._recess_obround import (
    _CAP_CLUSTER_FRAC,
    _compatible_end_groups,
    _extend_obround_ends,
    _extend_obround_proposals,
    _obround_end,
    _obround_ends,
    _recognise_obround_from_ends,
)
from quiddity._recess_reduce import (
    _body_scoped_proposals,
    _Claims,
    _collapse_collinear,
    _collapse_collinear_proposals,
    _merge,
    _merge_proposals,
    _prism_material_fraction,
    _RecessProposal,
    _same_channel_line,
)
from quiddity._registry import PHYSICAL_DEFINITIONS, FullyAttributed
from quiddity._volume_probe import PRISM_PROBE_FLOOR
from tools._legacy_recognition import (
    recognise_pockets,
)

ROOT = Path(__file__).parents[1]


def _obround(length: float, width: float, depth: float):
    end = Cylinder(width / 2, depth)
    return Box(length, width, depth) + Pos(-length / 2, 0, 0) * end + Pos(length / 2, 0, 0) * end


@pytest.mark.parametrize("length", [3, 30])
def test_slot_dual_read_retains_exact_cap_groups(length: float) -> None:
    part = Box(100, 60, 20) - _obround(length, 12, 20)
    graph = FaceGraph(part)
    proposals = _body_scoped_proposals([part], partial(_slot_proposals_one, graph=graph))
    assert [proposal.record.to_dict() for proposal in proposals] == [
        record.to_dict() for record in recognise_slots(part)
    ]
    (proposal,) = proposals
    assert len(proposal.caps) == 2
    assert all(group for group in proposal.caps)
    assert proposal.caps[0].isdisjoint(proposal.caps[1])
    assert all(not graph.is_planar(node) for group in proposal.caps for node in group)
    assert bool(proposal.planar) is (length == 30)


def test_stubby_pocket_dual_read_retains_caps_without_publishing_them() -> None:
    tool = _obround(6, 10, 8)
    part = Box(60, 40, 12) - Pos(0, 0, 4) * tool
    graph = FaceGraph(part)
    proposals = _body_scoped_proposals([part], partial(_pocket_proposals_one, graph=graph))
    assert [proposal.record.to_dict() for proposal in proposals] == [
        record.to_dict() for record in recognise_pockets(part)
    ]
    (proposal,) = proposals
    assert proposal.planar == frozenset()
    assert len(proposal.caps) == 2


@pytest.mark.parametrize(("angle", "expected"), [(2.1, 1), (2.2, 0)])
def test_near_principal_cap_centerlines_use_scaled_pairing_boundary(
    angle: float, expected: int
) -> None:
    tool = Rot(0, 0, angle) * _obround(1, 1, 8)
    part = Box(60, 40, 12) - Pos(0, 0, 4) * tool
    graph = FaceGraph(part)
    ends = [end for end in _obround_ends(part, graph) if end[2] == "z"]

    assert len(ends) == 2
    delta = abs(ends[0][4] - ends[1][4])
    tolerance = length_tol(ends[0][3], rel=_CAP_CLUSTER_FRAC)
    assert (delta <= tolerance) is bool(expected)
    assert len(_pocket_proposals_one(part, graph=graph)) == expected


def test_fuzzy_centerline_pairing_is_limited_to_stubby_recesses() -> None:
    """End recovery must not duplicate an elongated recess already found from its walls."""

    def end(*, center: float, flat: float, direction: int) -> tuple:
        return ("y", "x", "z", 1.0, center, flat, direction, 4.0, 12.0, frozenset())

    # Both pairs straddle the legacy two-decimal centre bucket while remaining within the
    # radius-scaled cap tolerance. Only the pair whose straight run is no longer than its width
    # belongs to the end-only recovery path.
    stubby = [
        end(center=0.004, flat=-0.5, direction=-1),
        end(center=0.006, flat=0.5, direction=1),
    ]
    elongated = [
        end(center=25.529956, flat=12.0169, direction=-1),
        end(center=25.541459, flat=17.2992, direction=1),
    ]

    assert tuple(map(len, _compatible_end_groups(stubby))) == (2,)
    assert tuple(map(len, _compatible_end_groups(elongated))) == (1, 1)


def test_fuzzy_centerline_pairing_refuses_a_shared_partner() -> None:
    """Uniqueness belongs to both ends; traversal order cannot select one of two low caps."""

    ends = [
        ("y", "x", "z", 1.0, 0.004, -0.5, -1, 4.0, 12.0, frozenset()),
        ("y", "x", "z", 1.0, 0.014, 0.5, 1, 4.0, 12.0, frozenset()),
        ("y", "x", "z", 1.0, 0.024, -0.4, -1, 4.0, 12.0, frozenset()),
    ]

    for ordered in permutations(ends):
        assert tuple(map(len, _compatible_end_groups(list(ordered)))) == (1, 1, 1)


@pytest.mark.parametrize(
    ("family", "part", "expected_planar"),
    [
        ("slot", Box(80, 50, 16) - Box(28, 10, 16), 2),
        ("slot", Box(100, 100, 16) - Box(54, 12, 16) - Box(12, 54, 16), 4),
        ("pocket", Box(60, 40, 12) - Pos(0, 0, 4) * Box(20, 12, 8), 2),
        ("pocket", Box(60, 40, 12) - Pos(25, 15, 4) * Box(20, 20, 8), 3),
        ("pocket", Box(80, 50, 14) - Pos(0, 0, 4) * _obround(30, 10, 10), 2),
    ],
)
def test_occurrence_matrix_preserves_public_parity_and_exact_planar_roles(
    family: str, part, expected_planar: int
) -> None:
    memo = FaceEdges()
    graph = FaceGraph(part, face_edges=memo)
    one = _slot_proposals_one if family == "slot" else _pocket_proposals_one
    public = recognise_slots if family == "slot" else recognise_pockets
    core = _discover_slots if family == "slot" else _discover_pockets
    proposals = _body_scoped_proposals(
        list(part.solids()) or [part], partial(one, face_edges=memo, graph=graph)
    )
    ledger = ClaimLedger(graph)

    assert [proposal.record.to_dict() for proposal in proposals] == [
        record.to_dict() for record in public(part, face_edges=memo)
    ]
    # The public entry points are writer-free per ADR 0002, so the parity is between the core
    # the registry calls and the public function, which is the seam it always mattered across.
    assert core(part, face_edges=memo, writer=ledger.writer) == public(part, face_edges=memo)
    assert all(len(proposal.planar) == expected_planar for proposal in proposals)
    assert all(node in graph.nodes for proposal in proposals for node in proposal.planar)
    assert {claim.claimant for claim in ledger.claims} == {
        proposal.record for proposal in proposals if proposal.planar
    }
    cap_nodes = frozenset(
        node for proposal in proposals for group in proposal.caps for node in group
    )
    if family == "slot":
        assert all(claim.defining.isdisjoint(cap_nodes) for claim in ledger.claims)
    else:
        assert cap_nodes.issubset(frozenset().union(*(claim.defining for claim in ledger.claims)))


def test_equal_occurrences_on_separate_solids_remain_distinct() -> None:
    first = Box(100, 60, 20) - _obround(3, 12, 20)
    second = Pos(200, 0, 0) * (Box(100, 60, 20) - _obround(3, 12, 20))
    part = Compound([first, second])
    graph = FaceGraph(part)
    proposals = _body_scoped_proposals(
        list(part.solids()), partial(_slot_proposals_one, graph=graph)
    )
    assert len(proposals) == 2
    assert proposals[0] is not proposals[1]
    assert proposals[0].record is not proposals[1].record
    assert proposals[0].caps[0].isdisjoint(proposals[1].caps[0])


def test_coincident_equal_occurrences_do_not_collapse_by_record_value() -> None:
    first = Box(100, 60, 20) - _obround(3, 12, 20)
    part = Compound([first, deepcopy(first)])
    graph = FaceGraph(part)
    proposals = _body_scoped_proposals(
        list(part.solids()), partial(_slot_proposals_one, graph=graph)
    )

    assert len(proposals) == 2
    assert proposals[0].record == proposals[1].record
    assert proposals[0].record is not proposals[1].record
    assert {node for group in proposals[0].caps for node in group}.isdisjoint(
        node for group in proposals[1].caps for node in group
    )


def test_merge_and_collapse_union_occurrence_provenance() -> None:
    part = Box(120, 120, 20) - Box(60, 14, 20) - Box(14, 60, 20)
    graph = FaceGraph(part)
    raw = _slot_proposals_one(part, graph=graph)
    # The production one-solid path has already reduced these; direct synthetic proposal
    # adversaries pin the neutral reducers' identity union without rematching record values.
    left, right = graph.nodes[0], graph.nodes[1]
    record = raw[0].record
    merged = _merge_proposals(
        [_RecessProposal(record, frozenset({left})), _RecessProposal(record, frozenset({right}))]
    )
    assert len(merged) == 1 and merged[0].planar == frozenset({left, right})
    assert _collapse_collinear_proposals(raw, part) == raw


def test_real_crossing_collapse_absorbs_every_arm_in_geometric_order(monkeypatch) -> None:
    import quiddity._recess_core as module

    part = Box(120, 120, 20) - Box(60, 14, 20) - Box(14, 60, 20)
    graph = FaceGraph(part)
    raw = []
    real = module._collapse_collinear_proposals

    def capture(proposals, owner):
        raw.extend(proposals)
        return real(proposals, owner)

    monkeypatch.setattr(module, "_collapse_collinear_proposals", capture)
    reduced = module._slot_proposals_one(part, graph=graph)

    assert [(p.record.long_axis, p.record.lo, p.record.hi) for p in raw] == [
        ("x", -30.0, -7.0),
        ("y", -30.0, -7.0),
        ("y", 7.0, 30.0),
        ("x", 7.0, 30.0),
    ]
    assert [(p.record.long_axis, p.record.lo, p.record.hi) for p in reduced] == [
        ("x", -30.0, 30.0),
        ("y", -30.0, 30.0),
    ]
    for proposal in reduced:
        absorbed = frozenset(
            node
            for arm in raw
            if arm.record.long_axis == proposal.record.long_axis
            for node in arm.planar
        )
        assert proposal.planar == absorbed and len(absorbed) == 4


def test_legacy_crossing_and_merge_projections_absorb_all_source_claims(monkeypatch) -> None:
    """The retained record-only reducers mirror the occurrence-safe provenance union."""
    import quiddity._recess_core as module

    part = Box(120, 120, 20) - Box(60, 14, 20) - Box(14, 60, 20)
    graph = FaceGraph(part)
    raw = []

    def capture(proposals, _part):
        raw.extend(proposals)
        return proposals

    monkeypatch.setattr(module, "_collapse_collinear_proposals", capture)
    module._slot_proposals_one(part, graph=graph)
    claims: _Claims = {proposal.record: set(proposal.planar) for proposal in raw}
    collapsed = _collapse_collinear([proposal.record for proposal in raw], part, claims)
    assert len(collapsed) == 2
    for record in collapsed:
        expected = set().union(
            *(
                set(proposal.planar)
                for proposal in raw
                if proposal.record.long_axis == record.long_axis
            )
        )
        assert claims[record] == expected

    first = raw[0]
    duplicate = replace(first.record, width=first.record.width + 1)
    merge_claims: _Claims = {
        first.record: set(first.planar),
        duplicate: {next(node for node in graph.nodes if node not in first.planar)},
    }
    assert _merge([duplicate, first.record], merge_claims) == [first.record]
    assert merge_claims[first.record] == set().union(*merge_claims.values())
    # Compatibility callers without claims remain valid and take the same geometry path.
    assert len(_collapse_collinear([proposal.record for proposal in raw], part)) == 2


def test_legacy_reducer_geometric_measurement_boundaries() -> None:
    """Compatibility reducers retain their documented closed prism and interval semantics."""
    base = _slot_proposals_one(Box(80, 50, 16) - Box(28, 10, 16))[0].record
    before = replace(base, lo=base.lo - 20, hi=base.lo - 10)
    after = replace(base, lo=base.hi + 10, hi=base.hi + 20)
    overlap = replace(base, lo=base.lo + 1, hi=base.hi - 1)
    assert _same_channel_line(before, base) == (before.hi, base.lo)
    assert _same_channel_line(after, base) == (base.hi, after.lo)
    assert _same_channel_line(overlap, base) is None
    assert _collapse_collinear([base], Box(80, 50, 16)) == [base]

    spans = {"x": (-1.0, 1.0), "y": (-1.0, 1.0), "z": (-1.0, 1.0)}

    class IntersectionPart:
        def __init__(self, result):
            self.result = result

        def intersect(self, _probe):
            return self.result

    class Volume:
        def __init__(self, volume):
            self.volume = volume

    assert _prism_material_fraction(spans, IntersectionPart(None), inset=0) == 0.0
    assert _prism_material_fraction(spans, IntersectionPart(Volume(4.0)), inset=0) == 0.5
    assert (
        _prism_material_fraction(spans, IntersectionPart([Volume(1.0), Volume(3.0)]), inset=0)
        == 0.5
    )
    with pytest.raises(ValueError, match="positive extent"):
        _prism_material_fraction({**spans, "x": (1.0, 1.0)}, IntersectionPart(None))
    assert (
        _prism_material_fraction(
            {**spans, "x": (1.0, 1.0 + PRISM_PROBE_FLOOR / 2)},
            IntersectionPart(None),
            inset=0,
        )
        == 1.0
    )


def test_merge_preserves_distinct_split_cap_patch_groups() -> None:
    part = Box(120, 60, 20) - _obround(30, 12, 20)
    graph = FaceGraph(part)
    nodes = graph.nodes[:4]
    record = _slot_proposals_one(part, graph=graph)[0].record
    merged = _merge_proposals(
        [
            _RecessProposal(record, caps=(frozenset(nodes[:2]),)),
            _RecessProposal(record, caps=(frozenset(nodes[2:]),)),
        ]
    )

    assert len(merged) == 1
    assert merged[0].caps == (frozenset(nodes[:2]), frozenset(nodes[2:]))


def test_imported_split_cylindrical_cap_retains_every_original_patch(tmp_path: Path) -> None:
    part = Box(100, 60, 20) - _obround(30, 12, 20)
    graph = FaceGraph(part)
    cap = max(_obround_ends(part, graph), key=lambda end: end[5])
    (cap_node,) = cap[9]
    face = graph.face(cap_node)
    seam = Edge.make_line((21, 0, face.bounding_box().min.Z), (21, 0, face.bounding_box().max.Z))
    splitter = BRepFeat_SplitShape(part.wrapped)
    splitter.Add(seam.wrapped, face.wrapped)
    splitter.Build()
    assert splitter.IsDone()
    split = type(part).cast(splitter.Shape())
    path = tmp_path / "split-cap.step"
    assert export_step(split, path)

    imported = import_step(path)
    imported_graph = FaceGraph(imported)
    ends = _obround_ends(imported, imported_graph)
    assert sorted(len(end[9]) for end in ends) == [1, 2]
    proposals = _slot_proposals_one(imported, graph=imported_graph)
    assert len(proposals) == 1
    assert sorted(len(group) for group in proposals[0].caps) == [1, 2]
    assert frozenset(node for group in proposals[0].caps for node in group) == frozenset(
        node for end in ends for node in end[9]
    )


def test_full_cylinder_and_lone_d_end_do_not_supply_obround_cap_pairs() -> None:
    round_hole = Box(80, 60, 20) - Cylinder(6, 20)
    round_graph = FaceGraph(round_hole)
    cylinder = next(item for item in _cylinder_faces(round_hole, round_graph) if item[4])
    assert _obround_end(cylinder, frozenset({cylinder[5]})) is None
    assert _obround_ends(round_hole, round_graph) == []

    lone_d = Box(80, 60, 20) - (Box(40, 12, 20) + Pos(-20, 0, 0) * Cylinder(6, 20))
    (proposal,) = _slot_proposals_one(lone_d, graph=FaceGraph(lone_d))
    assert proposal.caps == () and proposal.record.length == 40.0


@pytest.mark.parametrize("mutation", ["missing", "one", "axis", "radius", "depth"])
def test_cap_matching_refusals_leave_the_legacy_record_unextended(monkeypatch, mutation) -> None:
    import quiddity._recess_obround as module

    part = Box(100, 60, 20) - _obround(30, 12, 20)
    graph = FaceGraph(part)
    extended = _slot_proposals_one(part, graph=graph)[0]
    radius = extended.record.width / 2
    raw = _RecessProposal(
        replace(
            extended.record,
            lo=extended.record.lo + radius,
            hi=extended.record.hi - radius,
            length=extended.record.length - 2 * radius,
        ),
        extended.planar,
    )
    ends = _obround_ends(part, graph)
    if mutation == "missing":
        changed = []
    elif mutation == "one":
        changed = ends[:1]
    else:
        target = ends[0]
        index, value = {
            "axis": (2, "x" if target[2] != "x" else "y"),
            "radius": (3, target[3] * 1.1),
            "depth": (8, target[8] + 1.0),
        }[mutation]
        changed_end = (*target[:index], value, *target[index + 1 :])
        changed = [changed_end, *ends[1:]]
    monkeypatch.setattr(module, "_obround_ends", lambda _part, _graph: changed)

    assert _extend_obround_proposals([raw], part, graph) == [raw]


@pytest.mark.parametrize(
    ("part", "one"),
    [
        (Box(100, 60, 20) - _obround(30, 12, 20), _slot_proposals_one),
        (Box(60, 40, 12) - Pos(25, 15, 4) * Box(20, 20, 8), _pocket_proposals_one),
    ],
)
def test_occurrence_and_roles_are_independent_of_face_traversal(part, one) -> None:
    baseline_graph = FaceGraph(part)
    baseline = one(part, graph=baseline_graph)

    class ReorderedPart:
        def faces(self):
            return list(reversed(part.faces()))

        def solids(self):
            return part.solids()

        def bounding_box(self):
            return part.bounding_box()

        def intersect(self, other):
            return part.intersect(other)

    reordered_part = ReorderedPart()
    memo = FaceEdges()
    reordered_graph = FaceGraph(reordered_part, face_edges=memo)
    reordered = one(reordered_part, face_edges=memo, graph=reordered_graph)

    def presented(proposals, graph):
        return [
            (
                proposal.record.to_dict(),
                sorted(graph.bounds(node) for node in proposal.planar),
                sorted(sorted(graph.bounds(node) for node in group) for group in proposal.caps),
            )
            for proposal in proposals
        ]

    assert presented(reordered, reordered_graph) == presented(baseline, baseline_graph)


def test_corner_notch_provenance_has_no_value_keyed_intermediate() -> None:
    part = Box(60, 40, 12) - Pos(25, 15, 4) * Box(20, 20, 8)
    graph = FaceGraph(part)
    proposals = _corner_notch_proposals(
        _planar_faces(part, None, graph),
        part.bounding_box(),
    )

    assert len(proposals) == 1 and len(proposals[0].planar) == 3
    source = inspect.getsource(_pocket_proposals_one)
    assert "_Claims" not in source
    assert "_corner_notch_proposals" in source


def test_legacy_claim_projections_are_derived_from_occurrence_proposals() -> None:
    """Compatibility maps remain projections, never an alternate provenance authority."""
    slot_part = Box(80, 50, 16) - Box(28, 10, 16)
    pocket_part = Box(60, 40, 12) - Pos(0, 0, 4) * Box(20, 12, 8)
    corner_part = Box(60, 40, 12) - Pos(25, 15, 4) * Box(20, 20, 8)

    for part, recognise_one in (
        (slot_part, _recognise_slots_one),
        (pocket_part, _recognise_pockets_one),
    ):
        graph = FaceGraph(part)
        claims: _Claims = {}
        records = recognise_one(part, graph=graph, claims=claims)
        assert records
        assert list(claims) == records
        assert all(claims[record] for record in records)
        assert all(node in graph.nodes for nodes in claims.values() for node in nodes)

    graph = FaceGraph(corner_part)
    faces = _planar_faces(corner_part, None, graph)
    claims = {}
    records = _recognise_corner_notches(faces, corner_part.bounding_box(), claims)
    assert len(records) == 1
    expected = _corner_notch_proposals(faces, corner_part.bounding_box())[0]
    assert claims[records[0]] == set(expected.planar)

    # Empty compatibility projections must neither invent keys nor leak stale authority.
    plain = Box(20, 20, 20)
    plain_graph = FaceGraph(plain)
    for recognise_one in (_recognise_slots_one, _recognise_pockets_one):
        empty_claims: _Claims = {}
        assert recognise_one(plain, graph=plain_graph, claims=empty_claims) == []
        assert empty_claims == {}
    empty_claims = {}
    assert (
        _recognise_corner_notches(
            _planar_faces(plain, None, plain_graph), plain.bounding_box(), empty_claims
        )
        == []
    )
    assert empty_claims == {}


def test_legacy_obround_projection_preserves_claims_and_record_only_route() -> None:
    """The retained compatibility helpers still project their historical values and claims."""
    part = Box(100, 60, 20) - _obround(30, 12, 20)
    graph = FaceGraph(part)
    proposal = _slot_proposals_one(part, graph=graph)[0]
    radius = proposal.record.width / 2
    raw = replace(
        proposal.record,
        lo=proposal.record.lo + radius,
        hi=proposal.record.hi - radius,
        length=proposal.record.length - 2 * radius,
    )
    claims: _Claims = {raw: {graph.nodes[0]}}
    assert _extend_obround_ends([raw], part, claims) == [proposal.record]
    assert claims[proposal.record] == claims[raw]

    faces = _planar_faces(part, None, graph)
    projected = cast(list[Any], _recognise_obround_from_ends(part, faces, graph=graph))
    proposed = cast(
        list[_RecessProposal],
        _recognise_obround_from_ends(part, faces, graph=graph, proposals=True),
    )
    assert [record.to_dict() for record in projected] == [
        item.record.to_dict() for item in proposed
    ]


def test_obround_end_rejects_nonconcave_surface_before_shape_classification() -> None:
    part = Box(80, 60, 20) - Cylinder(6, 20)
    graph = FaceGraph(part)
    cap = next(item for item in _cylinder_faces(part, graph) if item[4])
    assert _obround_end((*cap[:4], False, cap[5])) is None


def test_occurrence_proposal_and_cap_helper_seams_are_closed() -> None:
    package = ROOT / "src/quiddity"
    watched = {
        "_slot_proposals_one",
        "_pocket_proposals_one",
        "_corner_notch_proposals",
        "_merge_proposals",
        "_collapse_collinear_proposals",
        "_extend_obround_proposals",
        "_obround_ends",
        "_cylinder_faces",
    }
    calls = {name: [] for name in watched}
    imports = {name: set() for name in watched}
    for path in sorted(package.glob("*.py")):
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        direct = {}
        for node in ast.walk(tree):
            if isinstance(node, ast.ImportFrom):
                for alias in node.names:
                    if alias.name in watched:
                        direct[alias.asname or alias.name] = alias.name
                        imports[alias.name].add(path.name)

        class Visitor(ast.NodeVisitor):
            def __init__(self, source_path, aliases):
                self.source_path = source_path
                self.aliases = aliases
                self.functions = []

            def visit_FunctionDef(self, node):
                self.functions.append(node.name)
                self.generic_visit(node)
                self.functions.pop()

            def visit_Call(self, node):
                leaf = ""
                if isinstance(node.func, ast.Name):
                    leaf = self.aliases.get(node.func.id, node.func.id)
                elif isinstance(node.func, ast.Attribute):
                    leaf = node.func.attr
                if leaf in watched:
                    calls[leaf].append(
                        (
                            self.source_path.name,
                            self.functions[-1] if self.functions else "<module>",
                        )
                    )
                self.generic_visit(node)

        Visitor(path, direct).visit(tree)

    assert calls == {
        "_slot_proposals_one": [("_recess_core.py", "_recognise_slots_one")],
        "_pocket_proposals_one": [("_recess_core.py", "_recognise_pockets_one")],
        "_corner_notch_proposals": [
            ("_recess_core.py", "_pocket_proposals_one"),
            ("_recess_core.py", "_recognise_corner_notches"),
        ],
        "_merge_proposals": [
            ("_recess_core.py", "_slot_proposals_one"),
            ("_recess_core.py", "_pocket_proposals_one"),
        ],
        "_collapse_collinear_proposals": [("_recess_core.py", "_slot_proposals_one")],
        "_extend_obround_proposals": [
            ("_recess_core.py", "_slot_proposals_one"),
            ("_recess_core.py", "_pocket_proposals_one"),
        ],
        "_obround_ends": [
            ("_recess_obround.py", "_extend_obround_proposals"),
            ("_recess_obround.py", "_recognise_obround_from_ends"),
        ],
        "_cylinder_faces": [
            ("_recess_obround.py", "_extend_obround_ends"),
            ("_recess_obround.py", "_obround_ends"),
        ],
    }
    assert imports["_slot_proposals_one"] == {"_recess_features.py"}
    assert imports["_pocket_proposals_one"] == {"_recess_features.py"}
    assert imports["_cylinder_faces"] == {"_recess_obround.py"}

    for name in ("_recess_faces.py", "_recess_obround.py", "_recess_reduce.py"):
        source = (package / name).read_text(encoding="utf-8")
        assert not any(
            prohibited in source
            for prohibited in (
                "CandidateSet",
                "EvidenceIndex",
                "InventoryProduct",
                "ReconciliationResult",
                "FullyAttributed",
                "IncompleteAttribution",
            )
        )


def test_competing_endpoint_cap_clusters_fail_closed(monkeypatch) -> None:
    import quiddity._recess_obround as module

    part = Box(100, 60, 20) - _obround(30, 12, 20)
    graph = FaceGraph(part)
    extended = _slot_proposals_one(part, graph=graph)[0]
    radius = extended.record.width / 2
    proposal = _RecessProposal(
        replace(
            extended.record,
            lo=extended.record.lo + radius,
            hi=extended.record.hi - radius,
            length=extended.record.length - 2 * radius,
        ),
        extended.planar,
    )
    ends = _obround_ends(part, graph)
    public_before = [record.to_dict() for record in recognise_slots(part)]
    low = min(ends, key=lambda end: end[5])
    spare = next(node for node in graph.nodes if node not in low[9])
    competing = (*low[:9], frozenset({spare}))
    monkeypatch.setattr(module, "_obround_ends", lambda _part, _graph: [*ends, competing])
    with pytest.raises(ValueError, match="compete for one endpoint"):
        _extend_obround_proposals([proposal], part, graph)
    # #234 is neutral: record-only callers retain the historical deterministic
    # first matching cap, while only the occurrence/provenance path refuses.
    assert [record.to_dict() for record in recognise_slots(part)] == public_before


def test_slot_and_pocket_children_promote_only_their_families() -> None:
    by_family = {definition.family: definition for definition in PHYSICAL_DEFINITIONS}
    assert isinstance(by_family[FamilyId.SLOTS].attribution, FullyAttributed)
    assert isinstance(by_family[FamilyId.POCKETS].attribution, FullyAttributed)
