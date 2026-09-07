"""F5: repeating radial profiles own their exact opposed source faces."""

from __future__ import annotations

import ast
import copy
import inspect
import math
from copy import deepcopy
from dataclasses import replace
from pathlib import Path

import pytest
from build123d import (
    Box,
    Compound,
    Cylinder,
    GeomType,
    Plane,
    Pos,
    RegularPolygon,
    Rot,
    export_step,
    extrude,
    import_step,
)

import quiddity.repeating_profiles as module
from quiddity import recognise_repeating_radial_profiles
from quiddity._adjacency import FaceGraph
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._effective_surfaces import SURFACE_READER_SITES
from quiddity._geometry import part_scale
from quiddity._registry import PHYSICAL_DEFINITIONS, FullyAttributed, NotCounted
from quiddity._run import start
from quiddity.result import _discover_all, _take_inventory
from tests.golden._common import toothed_prism

ROOT = Path(__file__).parents[1]


def _notched_round(repeats: int = 5):
    """Existing-predicate line/arc profile with an exact common-circle centre."""

    part = Cylinder(20, 10)
    for index in range(repeats):
        part -= Rot(0, 0, 360 * index / repeats) * Pos(18, 0, 0) * Box(8, 3, 10)
    return part


def _distance(left, right) -> float:
    return math.hypot(left[0] - right[0], left[1] - right[1])


def _polar(points, centre):
    def one(candidate):
        raw = [
            (_distance(point, centre), math.atan2(point[1] - centre[1], point[0] - centre[0]))
            for point in candidate
        ]
        angles = [raw[0][1]]
        for _radius, angle in raw[1:]:
            while angle - angles[-1] > math.pi:
                angle -= 2 * math.pi
            while angle - angles[-1] < -math.pi:
                angle += 2 * math.pi
            angles.append(angle)
        phase = angles[0]
        direct = tuple(
            (round(radius, 6), round(angle - phase, 6))
            for (radius, _), angle in zip(raw, angles, strict=True)
        )
        return min(direct, tuple((radius, -angle) for radius, angle in direct))

    return min(one(points), one(tuple(reversed(points))))


def _oracle_boundary(face, graph: FaceGraph, node, bbox, *, tol: float):
    """Independently prove one extremal outer-wire orbit without production helpers."""

    normal = graph.normal(node)
    if normal is None:
        return None
    axis_index = max(range(3), key=lambda index: abs(normal[index]))
    if abs(normal[axis_index]) < 0.999:
        return None
    axis = "xyz"[axis_index]
    plane_axes = tuple(candidate for candidate in "xyz" if candidate != axis)
    bounds = graph.bounds(node)[axis_index]
    if abs(bounds[1] - bounds[0]) > tol:
        return None
    at = sum(bounds) / 2
    edges = []
    wire = face.outer_wire()
    for edge in wire.edges():
        points = tuple(
            tuple(
                float(getattr(edge.position_at(i / 8), candidate.upper()))
                for candidate in plane_axes
            )
            for i in range(9)
        )
        edges.append(
            (getattr(edge.geom_type, "name", str(edge.geom_type)), float(edge.length), points)
        )
    if not edges or all(kind == GeomType.CIRCLE.name for kind, _length, _points in edges):
        return None

    # Build the endpoint incidence graph independently and require one degree-two cycle.
    vertices = []
    incidence = []
    edge_vertices = []
    for edge_index, (_kind, _length, points) in enumerate(edges):
        pair = []
        for point in (points[0], points[-1]):
            matches = [i for i, vertex in enumerate(vertices) if _distance(point, vertex) <= tol]
            if len(matches) > 1:
                return None
            if matches:
                vertex_index = matches[0]
            else:
                vertex_index = len(vertices)
                vertices.append(point)
                incidence.append([])
            incidence[vertex_index].append(edge_index)
            pair.append(vertex_index)
        if pair[0] == pair[1]:
            return None
        edge_vertices.append(tuple(pair))
    if any(len(items) != 2 for items in incidence):
        return None
    reached = set()
    frontier = [0]
    while frontier:
        edge_index = frontier.pop()
        if edge_index in reached:
            continue
        reached.add(edge_index)
        frontier.extend(
            neighbour
            for vertex_index in edge_vertices[edge_index]
            for neighbour in incidence[vertex_index]
            if neighbour not in reached
        )
    if len(reached) != len(edges):
        return None

    circles = []
    for edge in wire.edges():
        if edge.geom_type == GeomType.CIRCLE:
            circles.append(
                tuple(
                    float(getattr(edge.arc_center, candidate.upper())) for candidate in plane_axes
                )
            )
    centre = None
    if len(circles) >= 2:
        mean = tuple(sum(point[i] for point in circles) / len(circles) for i in range(2))
        if all(_distance(point, mean) <= tol for point in circles):
            centre = mean
    if centre is None:
        centre = tuple(float(getattr(wire.bounding_box().center(), a.upper())) for a in plane_axes)

    for repeats in range(len(edges) - 1, 4, -1):
        if len(edges) <= repeats or len(edges) % repeats:
            continue
        angle = 2 * math.pi / repeats
        mapping = []
        for kind, length, points in edges:
            rotated = tuple(
                (
                    centre[0]
                    + (point[0] - centre[0]) * math.cos(angle)
                    - (point[1] - centre[1]) * math.sin(angle),
                    centre[1]
                    + (point[0] - centre[0]) * math.sin(angle)
                    + (point[1] - centre[1]) * math.cos(angle),
                )
                for point in points
            )
            matches = []
            for candidate_index, (other_kind, other_length, other_points) in enumerate(edges):
                if kind != other_kind or abs(length - other_length) > tol:
                    continue
                if any(
                    max(_distance(a, b) for a, b in zip(rotated, candidate, strict=True)) <= tol
                    for candidate in (other_points, tuple(reversed(other_points)))
                ):
                    matches.append(candidate_index)
            if len(matches) != 1:
                break
            mapping.append(matches[0])
        if len(mapping) != len(edges) or len(set(mapping)) != len(edges):
            continue
        unseen = set(range(len(edges)))
        orbits = []
        while unseen:
            start = min(unseen)
            orbit = []
            current = start
            while current not in orbit:
                orbit.append(current)
                current = mapping[current]
            if current != start or len(orbit) != repeats:
                break
            unseen -= set(orbit)
            orbits.append(orbit)
        if unseen:
            continue
        signature = tuple(
            sorted(
                (
                    edges[orbit[0]][0],
                    round(edges[orbit[0]][1], 6),
                    _polar(edges[orbit[0]][2], centre),
                )
                for orbit in orbits
            )
        )
        return axis, plane_axes, at, centre, repeats, len(edges), signature
    return None


def _oracle(part, graph: FaceGraph, *, tol: float = 1e-5):
    """Topology-first expected occurrence and exact source roles, before Candidate reads."""

    bbox = part.bounding_box()
    metric_tol = max(tol, max(bbox.size.X, bbox.size.Y, bbox.size.Z) * 1e-5)
    proved = []
    for node in graph.nodes:
        if graph.is_planar(node):
            evidence = _oracle_boundary(graph.face(node), graph, node, bbox, tol=metric_tol)
            if evidence is not None:
                proved.append((node, evidence))
    expected = []
    for axis in "xyz":
        index = "xyz".index(axis)
        lo, hi = float(getattr(bbox.min, axis.upper())), float(getattr(bbox.max, axis.upper()))
        lower = [
            (node, fact)
            for node, fact in proved
            if fact[0] == axis and abs(fact[2] - lo) <= metric_tol
        ]
        upper = [
            (node, fact)
            for node, fact in proved
            if fact[0] == axis and abs(fact[2] - hi) <= metric_tol
        ]
        for low_node, low in lower:
            matches = [
                (node, fact)
                for node, fact in upper
                if low[:2] == fact[:2]
                and _distance(low[3], fact[3]) <= metric_tol
                and low[4:] == fact[4:]
            ]
            if len(matches) != 1:
                continue
            high_node, _high = matches[0]
            coords = [0.0, 0.0, 0.0]
            coords["xyz".index(low[1][0])] = low[3][0]
            coords["xyz".index(low[1][1])] = low[3][1]
            coords[index] = (lo + hi) / 2
            expected.append(
                (
                    (axis, tuple(coords), (lo, hi), low[4], low[5], low[6]),
                    frozenset({low_node, high_node}),
                )
            )
    return sorted(expected, key=lambda item: item[0])


def _oracle_inventory(part, *, tol: float = 1e-5):
    """Independent per-solid occurrence roster, source faces, owner and global order."""

    solids = list(part.solids()) or [part]
    expected = []
    for solid in solids:
        graph = FaceGraph(solid)
        for facts, nodes in _oracle(solid, graph, tol=tol):
            faces = tuple(graph.face(node) for node in nodes)
            owner = graph.common_valid_solid(nodes)
            assert owner is not None
            expected.append((facts, faces, owner))
    return sorted(expected, key=lambda item: item[0])


def _assert_attributed(part, *, repeats: int):
    # Freeze the complete expected topology before aggregate orchestration creates any Candidate
    # or exposes any run evidence. Only the final comparison crosses into the run graph.
    (((axis, centre, span, oracle_repeats, edge_count, signature), expected_faces, owner),) = (
        _oracle_inventory(part)
    )
    assert owner is not None
    public = recognise_repeating_radial_profiles(part)
    product = _take_inventory(part)
    records = product.result.repeating_radial_profiles
    candidates = product.physical.candidate_set(FamilyId.REPEATING_RADIAL_PROFILES).candidates
    assert [record.to_dict() for record in records] == [record.to_dict() for record in public]
    assert len(records) == len(candidates) == 1
    candidate = candidates[0]
    record = records[0]
    assert candidate.record is record
    assert record.repeat_count == repeats
    assert (
        record.axis,
        record.centre,
        record.span,
        record.repeat_count,
        record.edge_count,
        record.sector_signature,
    ) == (axis, centre, span, oracle_repeats, edge_count, signature)
    actual = product.evidence.defining_of(candidate)
    actual_faces = [product.context.graph.face(node) for node in actual]
    assert all(
        sum(candidate_face.is_same(expected_face) for candidate_face in actual_faces) == 1
        for expected_face in expected_faces
    )
    assert all(
        sum(candidate_face.is_same(expected_face) for expected_face in expected_faces) == 1
        for candidate_face in actual_faces
    )
    assert product.context.graph.common_valid_solid(actual) is not None
    return record


@pytest.mark.parametrize("repeats", [6, 8, 10])
def test_repeat_counts_own_exact_opposed_faces(repeats: int) -> None:
    _assert_attributed(toothed_prism(repeats=repeats), repeats=repeats)


@pytest.mark.parametrize("repeats", [5, 7, 11])
def test_supported_minimum_and_prime_line_arc_profiles(repeats: int) -> None:
    _assert_attributed(_notched_round(repeats), repeats=repeats)


def test_straight_edge_odd_fixture_behavior_is_unchanged() -> None:
    # Its odd-tip wire bbox is not the rotation centre and it has no circular centre evidence.
    assert recognise_repeating_radial_profiles(toothed_prism(repeats=5)) == []


def test_empty_all_circle_polygon_and_inner_wire_controls() -> None:
    assert recognise_repeating_radial_profiles(Box(10, 10, 10)) == []
    assert recognise_repeating_radial_profiles(Cylinder(20, 10)) == []
    assert recognise_repeating_radial_profiles(extrude(RegularPolygon(20, 8), 10)) == []
    inner_only = Cylinder(30, 10) - _notched_round(5)
    assert recognise_repeating_radial_profiles(inner_only) == []


def test_extremal_tolerance_and_one_to_one_opposed_matching(monkeypatch) -> None:
    part = toothed_prism()
    bbox = part.bounding_box()
    metric = part_scale(bbox) * 1e-5
    faces = list(part.faces())
    real = [module._prove_boundary(face, bbox, tol=metric) for face in faces]
    boundaries = [item for item in real if item is not None]
    assert len(boundaries) == 2
    lower = min(boundaries, key=lambda item: item.at)
    upper = max(boundaries, key=lambda item: item.at)
    spare_faces = [face for face, item in zip(faces, real, strict=True) if item is None]

    def run(evidence):
        monkeypatch.setattr(
            module,
            "_prove_boundary",
            lambda face, _bbox, *, tol: next(
                (item for expected_face, item in evidence if face.is_same(expected_face)), None
            ),
        )
        return module._recognise_solid(part, tol=1e-5)

    assert (
        len(run([(lower.face, replace(lower, at=bbox.min.Z + metric)), (upper.face, upper)])) == 1
    )
    assert (
        run([(lower.face, replace(lower, at=bbox.min.Z + 1.01 * metric)), (upper.face, upper)])
        == []
    )
    duplicate_lower = replace(lower, face=spare_faces[0])
    assert run([(lower.face, lower), (spare_faces[0], duplicate_lower), (upper.face, upper)]) == []
    duplicate_upper = replace(upper, face=spare_faces[1])
    assert run([(lower.face, lower), (upper.face, upper), (spare_faces[1], duplicate_upper)]) == []


@pytest.mark.parametrize(
    "part",
    [
        Pos(17, -13, 29) * toothed_prism(),
        Rot(0, 0, 37) * toothed_prism(),
        toothed_prism().mirror(Plane.YZ),
        toothed_prism().mirror(Plane.XZ),
        toothed_prism().scale(0.2),
        toothed_prism().scale(5),
    ],
)
def test_axis_preserving_transforms_keep_source_roles(part) -> None:
    _assert_attributed(part, repeats=8)


@pytest.mark.parametrize(
    ("part", "axis"),
    [
        (Rot(0, 90, 0) * toothed_prism(), "x"),
        (Rot(90, 0, 0) * toothed_prism(), "y"),
    ],
)
def test_all_principal_axes_are_oracle_derived(part, axis: str) -> None:
    record = _assert_attributed(part, repeats=8)
    assert record.axis == axis


def test_real_step_round_trip_retains_roles(tmp_path: Path) -> None:
    source = _notched_round(7)
    path = tmp_path / "radial.step"
    assert export_step(source, path)
    imported = import_step(path)
    _assert_attributed(imported, repeats=7)


def test_custom_tolerance_path_preserves_value_and_roles() -> None:
    part = _notched_round(5)
    expected = recognise_repeating_radial_profiles(part, tol=2e-5)
    ledger = ClaimLedger(FaceGraph(part))
    actual = module._discover_repeating_radial_profiles(part, tol=2e-5, writer=ledger.writer)
    assert [item.to_dict() for item in actual] == [item.to_dict() for item in expected]
    assert len(ledger.candidate_set(FamilyId.REPEATING_RADIAL_PROFILES).candidates) == 1


def test_metric_tolerance_floor_crossover_is_exact(monkeypatch) -> None:
    part = toothed_prism().scale(2)
    metric = part_scale(part.bounding_box()) * 1e-5
    seen = []

    def decline(_face, _bbox, *, tol):
        seen.append(tol)
        return None

    monkeypatch.setattr(module, "_prove_boundary", decline)
    assert module._recognise_solid(part, tol=metric) == []
    assert set(seen) == {metric}
    seen.clear()
    explicit = metric * 1.01
    assert module._recognise_solid(part, tol=explicit) == []
    assert set(seen) == {explicit}


@pytest.mark.parametrize("factory", [_notched_round, toothed_prism])
def test_common_circle_and_bbox_centre_routes_are_independently_reconstructed(factory) -> None:
    part = factory(8)
    graph = FaceGraph(part)
    expected = _oracle(part, graph)
    assert len(expected) == 1
    centre = expected[0][0][1]
    assert centre[:2] == pytest.approx((0.0, 0.0), abs=1e-8)


def test_equal_coincident_solids_remain_distinct_occurrences() -> None:
    first = toothed_prism()
    part = Compound([first, deepcopy(first)])
    expected = _oracle_inventory(part)
    ledger = ClaimLedger(FaceGraph(part))
    records = module._discover_repeating_radial_profiles(part, writer=ledger.writer)
    candidates = ledger.candidate_set(FamilyId.REPEATING_RADIAL_PROFILES).candidates
    assert len(records) == len(candidates) == 2
    assert len(expected) == 2 and expected[0][0] == expected[1][0]
    assert records[0] == records[1] and records[0] is not records[1]
    assert candidates[0].record is records[0] and candidates[1].record is records[1]
    defining = [ledger.defining_of(candidate) for candidate in candidates]
    assert all(len(nodes) == 2 for nodes in defining)
    assert defining[0].isdisjoint(defining[1])
    assert ledger.graph.common_valid_solid(defining[0]) != ledger.graph.common_valid_solid(
        defining[1]
    )
    for candidate, (_facts, expected_faces, _owner) in zip(candidates, expected, strict=True):
        actual_faces = [ledger.graph.face(node) for node in ledger.defining_of(candidate)]
        assert all(any(face.is_same(want) for face in actual_faces) for want in expected_faces)


def test_multiple_unequal_solids_follow_record_order() -> None:
    part = Compound([Pos(-60, 0, 0) * _notched_round(5), Pos(60, 0, 0) * _notched_round(7)])
    expected = _oracle_inventory(part)
    ledger = ClaimLedger(FaceGraph(part))
    records = module._discover_repeating_radial_profiles(part, writer=ledger.writer)
    candidates = ledger.candidate_set(FamilyId.REPEATING_RADIAL_PROFILES).candidates
    assert records == sorted(records)
    assert [candidate.record for candidate in candidates] == records
    assert [record.repeat_count for record in records] == [5, 7]
    assert [item[0][3] for item in expected] == [5, 7]
    assert all(len(ledger.defining_of(candidate)) == 2 for candidate in candidates)


def test_reversed_solid_face_traversal_preserves_records_and_roles(monkeypatch) -> None:
    part = _notched_round(7)
    ledger = ClaimLedger(FaceGraph(part))
    expected = _oracle(part, ledger.graph)
    original = type(part).faces
    monkeypatch.setattr(type(part), "faces", lambda self: list(reversed(original(self))))
    records = module._discover_repeating_radial_profiles(part, writer=ledger.writer)
    candidate = ledger.candidate_set(FamilyId.REPEATING_RADIAL_PROFILES).candidates[0]
    assert records[0].repeat_count == 7
    assert ledger.defining_of(candidate) == expected[0][1]


def test_public_path_is_writer_free_and_value_stable(monkeypatch) -> None:
    part = toothed_prism()
    expected = [record.to_dict() for record in recognise_repeating_radial_profiles(part)]
    original = module._discover_repeating_radial_profiles
    seen = []

    def observed(*args, **kwargs):
        seen.append(kwargs.get("writer"))
        return original(*args, **kwargs)

    monkeypatch.setattr(module, "_discover_repeating_radial_profiles", observed)
    actual = [record.to_dict() for record in module.recognise_repeating_radial_profiles(part)]
    assert actual == expected
    assert seen == [None]


@pytest.mark.parametrize("failure", ["same", "stale", "body", "late", "reuse"])
def test_identity_and_body_failures_are_named_and_atomic(monkeypatch, failure: str) -> None:
    part = toothed_prism()
    ledger = ClaimLedger(FaceGraph(part))
    original = module._recognise_solid

    def corrupted(*args, **kwargs):
        proposal = original(*args, **kwargs)[0]
        if failure == "same":
            return [replace(proposal, upper_face=proposal.lower_face)]
        if failure == "stale":
            return [replace(proposal, upper_face=Pos(0, 0, 100) * proposal.upper_face)]
        if failure == "late":
            broken = replace(proposal, upper_face=Pos(0, 0, 100) * proposal.upper_face)
            return [proposal, broken]
        if failure == "reuse":
            other_record = replace(
                proposal.record,
                centre=(proposal.record.centre[0] + 1, *proposal.record.centre[1:]),
            )
            return [proposal, replace(proposal, record=other_record)]
        return [proposal]

    monkeypatch.setattr(module, "_recognise_solid", corrupted)
    if failure == "body":
        monkeypatch.setattr(ledger.graph, "common_valid_solid", lambda _nodes: None)
    with pytest.raises(module._RepeatingRadialAttributionError):
        module._discover_repeating_radial_profiles(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.REPEATING_RADIAL_PROFILES).candidates == ()


def test_foreign_writer_refusal_leaves_no_prefix() -> None:
    part = _notched_round(5)
    foreign = ClaimLedger(FaceGraph(Box(4, 4, 4)))
    with pytest.raises(module._RepeatingRadialAttributionError):
        module._discover_repeating_radial_profiles(part, writer=foreign.writer)
    assert foreign.candidate_set(FamilyId.REPEATING_RADIAL_PROFILES).candidates == ()


def test_shallow_wrappers_resolve_but_deep_geometric_clones_refuse(monkeypatch) -> None:
    part = _notched_round(5)
    original = module._recognise_solid
    proposals = original(part, tol=1e-5)

    shallow = replace(
        proposals[0],
        lower_face=copy.copy(proposals[0].lower_face),
        upper_face=copy.copy(proposals[0].upper_face),
    )
    monkeypatch.setattr(module, "_recognise_solid", lambda *_args, **_kwargs: [shallow])
    ledger = ClaimLedger(FaceGraph(part))
    assert len(module._discover_repeating_radial_profiles(part, writer=ledger.writer)) == 1

    clone = replace(proposals[0], upper_face=deepcopy(proposals[0].upper_face))
    monkeypatch.setattr(module, "_recognise_solid", lambda *_args, **_kwargs: [clone])
    refused = ClaimLedger(FaceGraph(part))
    with pytest.raises(module._RepeatingRadialAttributionError):
        module._discover_repeating_radial_profiles(part, writer=refused.writer)
    assert refused.candidate_set(FamilyId.REPEATING_RADIAL_PROFILES).candidates == ()


def test_open_shell_and_malformed_sampling_fail_closed(monkeypatch) -> None:
    shell = _notched_round(5).shells()[0]
    # Historical geometry-only recognition accepts the complete shell boundary; aggregate
    # attribution refuses because no valid SolidRef owns it.
    assert len(recognise_repeating_radial_profiles(shell)) == 1
    ledger = ClaimLedger(FaceGraph(shell))
    with pytest.raises(module._RepeatingRadialAttributionError, match="valid solid"):
        module._discover_repeating_radial_profiles(shell, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.REPEATING_RADIAL_PROFILES).candidates == ()
    monkeypatch.setattr(
        type(shell.faces()[0]),
        "outer_wire",
        lambda _self: (_ for _ in ()).throw(RuntimeError("bad wire")),
    )
    assert recognise_repeating_radial_profiles(shell) == []


def test_second_occurrence_identity_failure_is_detected_before_any_issue(monkeypatch) -> None:
    first = Pos(-60, 0, 0) * _notched_round(5)
    second = Pos(60, 0, 0) * _notched_round(7)
    part = Compound([first, second])
    ledger = ClaimLedger(FaceGraph(part))
    original = module._recognise_solid
    calls = 0

    def corrupt_second(solid, *, tol, properties=None):
        nonlocal calls
        proposals = original(solid, tol=tol, properties=properties)
        calls += 1
        if calls == 2:
            proposal = proposals[0]
            proposals[0] = replace(
                proposal,
                upper_face=Pos(0, 0, 100) * proposal.upper_face,
            )
        return proposals

    monkeypatch.setattr(module, "_recognise_solid", corrupt_second)
    with pytest.raises(module._RepeatingRadialAttributionError):
        module._discover_repeating_radial_profiles(part, writer=ledger.writer)
    assert calls == 2
    assert ledger.candidate_set(FamilyId.REPEATING_RADIAL_PROFILES).candidates == ()


def test_aggregate_late_refusal_has_no_empty_candidate_or_completed_capability(monkeypatch) -> None:
    part = Compound([Pos(-60, 0, 0) * _notched_round(5), Pos(60, 0, 0) * _notched_round(7)])
    original = module._recognise_solid
    calls = 0

    def corrupt_second(solid, *, tol, properties=None):
        nonlocal calls
        proposals = original(solid, tol=tol, properties=properties)
        calls += 1
        if calls == 2:
            proposals[0] = replace(
                proposals[0],
                upper_face=Pos(0, 0, 100) * proposals[0].upper_face,
            )
        return proposals

    monkeypatch.setattr(module, "_recognise_solid", corrupt_second)
    context = start(part)
    ledger = ClaimLedger(context.graph, definitions=PHYSICAL_DEFINITIONS)
    with pytest.raises(module._RepeatingRadialAttributionError):
        _discover_all(context, ledger)
    assert ledger.candidate_set(FamilyId.REPEATING_RADIAL_PROFILES).candidates == ()
    assert FamilyId.REPEATING_RADIAL_PROFILES not in ledger._issuer._completed
    assert FamilyId.REPEATING_RADIAL_PROFILES not in ledger._issuer._completed_occurrences


def test_empty_and_below_minimum_emit_no_candidate() -> None:
    for part in (Box(10, 10, 10), _notched_round(4)):
        ledger = ClaimLedger(FaceGraph(part))
        assert module._discover_repeating_radial_profiles(part, writer=ledger.writer) == []
        assert ledger.candidate_set(FamilyId.REPEATING_RADIAL_PROFILES).candidates == ()


def test_registry_status_and_not_counted_disposition_are_exact() -> None:
    definition = next(
        item for item in PHYSICAL_DEFINITIONS if item.family is FamilyId.REPEATING_RADIAL_PROFILES
    )
    assert isinstance(definition.attribution, FullyAttributed)
    assert isinstance(definition.census, NotCounted)
    assert definition.census.reason == "correspondence evidence is not a distinct feature"


def test_private_core_and_constructor_rosters_are_closed() -> None:
    package = ROOT / "src/quiddity"
    sites = []
    constructors = []
    proposal_constructors = []
    for path in package.glob("*.py"):
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        aliases = set()
        module_aliases = set()
        for statement in tree.body:
            if isinstance(statement, ast.ImportFrom) and statement.module in {
                "quiddity.repeating_profiles",
                "repeating_profiles",
            }:
                aliases.update(
                    alias.asname or alias.name
                    for alias in statement.names
                    if alias.name == "_discover_repeating_radial_profiles"
                )
            if isinstance(statement, ast.ImportFrom) and statement.module == "quiddity":
                module_aliases.update(
                    alias.asname or alias.name
                    for alias in statement.names
                    if alias.name == "repeating_profiles"
                )
            if isinstance(statement, ast.Import):
                module_aliases.update(
                    alias.asname or alias.name
                    for alias in statement.names
                    if alias.name == "quiddity.repeating_profiles"
                )
        if path.name == "repeating_profiles.py":
            aliases.add("_discover_repeating_radial_profiles")
        for node in ast.walk(tree):
            if not isinstance(node, ast.Call):
                continue
            if isinstance(node.func, ast.Name) and node.func.id in aliases:
                sites.append((path.name, node))
            if (
                isinstance(node.func, ast.Attribute)
                and node.func.attr == "_discover_repeating_radial_profiles"
                and isinstance(node.func.value, ast.Name)
                and node.func.value.id in module_aliases
            ):
                sites.append((path.name, node))
            name = node.func.id if isinstance(node.func, ast.Name) else ""
            if name == "RepeatingRadialProfile":
                constructors.append(path.name)
            if name == "_RepeatingRadialProposal":
                proposal_constructors.append(path.name)
    assert {path for path, _call in sites} == {"repeating_profiles.py", "_registry.py"}
    registry_call = next(call for path, call in sites if path == "_registry.py")
    keywords = {keyword.arg: keyword.value for keyword in registry_call.keywords}
    assert isinstance(keywords["writer"], ast.Attribute)
    assert keywords["writer"].attr == "writer"
    public_call = next(call for path, call in sites if path == "repeating_profiles.py")
    assert all(keyword.arg != "writer" for keyword in public_call.keywords)
    assert constructors == ["repeating_profiles.py"]
    assert proposal_constructors == ["repeating_profiles.py"]
    assert tuple(inspect.signature(recognise_repeating_radial_profiles).parameters) == (
        "part",
        "tol",
    )
    registry_keywords = {keyword.arg: keyword.value for keyword in registry_call.keywords}
    writer = registry_keywords["writer"]
    assert isinstance(writer, ast.Attribute) and writer.attr == "writer"
    assert isinstance(writer.value, ast.Name) and writer.value.id == "s"
    assert {key for key in SURFACE_READER_SITES if key.startswith("repeating_profiles:")} == {
        "repeating_profiles:_sample_wire:geom_type:1",
        "repeating_profiles:_sample_wire:geom_type:2",
        "repeating_profiles:_common_circle_centre:geom_type:1",
    }
    imported_modules = {
        node.module
        for node in ast.walk(
            ast.parse((package / "repeating_profiles.py").read_text(encoding="utf-8"))
        )
        if isinstance(node, ast.ImportFrom) and node.module is not None
    }
    assert imported_modules == {
        "__future__",
        "dataclasses",
        "math",
        "build123d",
        "quiddity._adjacency",
        "quiddity._candidates",
        "quiddity._claims",
        "quiddity._geometry",
        "quiddity._record",
        "quiddity._solid_properties",
        "quiddity._typing",
        "quiddity.profiled_bores",
    }
    source = (package / "repeating_profiles.py").read_text(encoding="utf-8")
    source_tree = ast.parse(source)
    assert "evidence publication was refused" not in source
    tree = ast.parse(source)
    publication_calls = [
        node
        for node in ast.walk(source_tree)
        if isinstance(node, ast.Call)
        and isinstance(node.func, ast.Attribute)
        and node.func.attr == "add_defining"
    ]
    assert len(publication_calls) == 1
    assert not any(
        publication_calls[0] in tuple(ast.walk(node))
        for node in ast.walk(tree)
        if isinstance(node, ast.Try)
    )
    for prohibited in (
        "CandidateSet",
        "EvidenceIndex",
        "InventoryProduct",
        "ReconciliationResult",
        "CompletedInputs",
        "candidate_set(",
        "snapshot_index(",
        "freeze_index(",
    ):
        assert prohibited not in source
