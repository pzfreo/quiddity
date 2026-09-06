# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

from __future__ import annotations

import json
import math
from dataclasses import dataclass, replace
from types import SimpleNamespace

import pytest
from build123d import (
    Box,
    BuildPart,
    BuildSketch,
    Compound,
    Cone,
    Edge,
    Face,
    Plane,
    Polygon,
    Pos,
    RegularPolygon,
    Rot,
    Shell,
    Wedge,
    export_step,
    extrude,
    import_step,
)

from quiddity import (
    PassageFrame,
    PassageSection,
    PassageSectionVertex,
    build_section_recess_document,
)
from quiddity._adjacency import FaceEdges, FaceGraph
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._registry import PHYSICAL_DEFINITIONS
from quiddity._run import start
from quiddity._section_passages import (
    _DIRECTION_TOL,
    _INTERVAL_TOL,
    _pair_line,
    _parallel_pair_candidates,
)
from quiddity._sections import LocalFrame, PlanarSection, SectionVertex
from quiddity.passages import _section_passage_record
from quiddity.result import _discover_all
from tools._legacy_recognition import (
    PassageCompatibilityError,
    PassageEnds,
    SectionPassage,
    build_recognition_result,
    recognise_passages,
    recognise_section_passages,
)


def _square():
    return Box(60, 40, 20) - Box(10, 10, 60)


class _LineVector:
    def __init__(self, xyz: tuple[float, float, float]) -> None:
        self.X, self.Y, self.Z = xyz

    def normalized(self) -> _LineVector:
        return self


class _LineEdge:
    geom_type = type("GeometryType", (), {"name": "LINE"})()

    def __init__(self, low: float, high: float) -> None:
        self._ends = (_LineVector((0.0, 0.0, low)), _LineVector((0.0, 0.0, high)))

    def tangent_at(self) -> _LineVector:
        return _LineVector((0.0, 0.0, 1.0))

    def position_at(self, at: float) -> _LineVector:
        return self._ends[0 if at == 0.0 else 1]


class _SharedEdges:
    def __init__(self, intervals: tuple[tuple[float, float], ...]) -> None:
        self._edges = tuple(_LineEdge(*interval) for interval in intervals)

    def shared_edges(self, left: object, right: object) -> tuple[_LineEdge, ...]:
        del left, right
        return self._edges


class _ReversedFacesPart:
    """A shallow part view that changes only face/solid presentation order."""

    def __init__(self, part) -> None:
        self._part = part

    def faces(self):
        return list(reversed(self._part.faces()))

    def solids(self):
        return list(reversed(self._part.solids()))

    def __getattr__(self, name):
        return getattr(self._part, name)


class _SplitJunctionEdges(FaceEdges):
    def __init__(self, target: Edge) -> None:
        super().__init__()
        self._target = target
        start = target.position_at(0.0)
        middle = target.position_at(0.5)
        end = target.position_at(1.0)
        self._replacement = (Edge.make_line(start, middle), Edge.make_line(middle, end))

    def of(self, face):
        edges = super().of(face)
        if not any(edge.wrapped.IsSame(self._target.wrapped) for edge in edges):
            return edges
        return [
            replacement
            for edge in edges
            for replacement in (
                self._replacement if edge.wrapped.IsSame(self._target.wrapped) else (edge,)
            )
        ]


@pytest.mark.parametrize(
    ("intervals", "accepted"),
    (
        (((-10.0, 0.0), (0.0, 10.0)), True),
        (((-10.0, 0.0), (_INTERVAL_TOL, 10.0)), True),
        (((-10.0, 0.0), (math.nextafter(_INTERVAL_TOL, math.inf), 10.0)), False),
        (((-10.0, 0.0), (-_INTERVAL_TOL, 10.0)), True),
        (((-10.0, 0.0), (math.nextafter(-_INTERVAL_TOL, -math.inf), 10.0)), False),
        (((-10.0, 10.0), (-10.0, 10.0)), False),
    ),
)
def test_segmented_junction_union_has_closed_gap_and_overlap_boundaries(
    intervals: tuple[tuple[float, float], ...], accepted: bool
) -> None:
    frame = LocalFrame.canonical((0.0, 0.0, 1.0), (0.0, 0.0, 0.0))
    result = _pair_line(_SharedEdges(intervals), object(), object(), frame)  # type: ignore[arg-type]
    assert (result is not None) is accepted
    if result is not None:
        assert result[2:] == (-10.0, 10.0)


def test_direction_buckets_do_not_narrow_the_parallel_tolerance() -> None:
    seed = (1.0, 0.49e-9, 0.0)
    adjacent_bucket = (1.0, 0.51e-9, 0.0)
    boundary_x = 1.0 - _DIRECTION_TOL
    boundary = (boundary_x, math.sqrt(1.0 - boundary_x * boundary_x), 0.0)
    outside_x = math.nextafter(boundary_x, -math.inf)
    outside = (outside_x, math.sqrt(1.0 - outside_x * outside_x), 0.0)
    assert tuple(round(value, 9) for value in seed) != tuple(
        round(value, 9) for value in adjacent_bucket
    )
    candidates = (
        (object(), object(), seed),
        (object(), object(), adjacent_bucket),
        (object(), object(), boundary),
        (object(), object(), outside),
    )
    selected = _parallel_pair_candidates(candidates, (1.0, 0.0, 0.0))  # type: ignore[arg-type]
    assert selected == candidates[:3]


def _polygonal_tool(sides_or_points):
    with BuildPart() as tool:
        with BuildSketch(Plane.XY):
            if isinstance(sides_or_points, int):
                RegularPolygon(7, sides_or_points)
            else:
                Polygon(*sides_or_points)
        extrude(amount=60, both=True)
    return tool.part


def test_six_sided_passage_uses_wall_run_and_exact_sloped_termination_planes() -> None:
    stock = Wedge(60, 50, 20, 0, -10, 60, 5)
    part = stock - _polygonal_tool(6)

    (record,) = recognise_section_passages(part)

    assert record.frame.run == (0.0, 0.0, 1.0)
    assert len(record.section.boundary) == 6
    assert record.run_interval == pytest.approx((-10.0, 7.5), abs=0.001)
    assert record.ends.low_gradient == pytest.approx((0.0, -0.2), abs=1e-6)
    assert record.ends.high_gradient == pytest.approx((0.0, -0.3), abs=1e-6)
    assert recognise_passages(part) == []


@pytest.mark.parametrize("sides", (3, 4, 6))
@pytest.mark.parametrize("angle", (1.0, 20.0, 35.0))
def test_parallel_stock_ends_do_not_choose_oblique_passage_run(sides, angle):
    part = Box(60, 50, 20) - Rot(0, angle, 0) * _polygonal_tool(sides)
    (record,) = recognise_section_passages(part)

    radians = math.radians(angle)
    assert record.frame.run == pytest.approx((math.sin(radians), 0, math.cos(radians)), abs=1e-6)
    assert record.run_interval == pytest.approx(
        (-10 / math.cos(radians), 10 / math.cos(radians)), abs=0.002
    )
    assert record.ends.low_gradient == record.ends.high_gradient
    assert math.hypot(*record.ends.low_gradient) == pytest.approx(math.tan(radians), abs=1e-6)
    # Every reconstructed termination vertex lies on actual stock, not a wall envelope.
    for at, gradient, expected_z in (
        (record.run_interval[0], record.ends.low_gradient, -10),
        (record.run_interval[1], record.ends.high_gradient, 10),
    ):
        for vertex in record.section.boundary:
            u, v = vertex.point
            t = at + gradient[0] * u + gradient[1] * v
            z = (
                record.frame.origin[2]
                + t * record.frame.run[2]
                + u * record.frame.u[2]
                + v * record.frame.v[2]
            )
            assert z == pytest.approx(expected_z, abs=0.002)


@pytest.mark.parametrize("scale", (0.1, 1.0, 10.0))
def test_parallel_oblique_ends_survive_placement_and_scale(scale):
    part = Box(60, 50, 20) - Rot(0, 20, 0) * _polygonal_tool(6)
    moved = Pos(17, -11, 9) * Rot(31, 17, 23) * part.scale(scale)
    (record,) = recognise_section_passages(moved)
    assert len(record.section.boundary) == 6
    assert record.ends.low_gradient == record.ends.high_gradient
    assert math.hypot(*record.ends.low_gradient) == pytest.approx(
        math.tan(math.radians(20)), abs=1e-6
    )
    assert record.run_interval[1] - record.run_interval[0] == pytest.approx(
        scale * 20 / math.cos(math.radians(20)), abs=0.002
    )


def test_oblique_parallel_end_proof_does_not_accept_blind_void():
    with BuildSketch() as sketch:
        RegularPolygon(7, 6)
    part = Box(60, 50, 20) - Rot(0, 20, 0) * extrude(sketch.sketch, amount=30)
    assert recognise_section_passages(part) == []


def test_oblique_parallel_ends_survive_step_round_trip(tmp_path):
    part = Box(60, 50, 20) - Rot(0, 20, 0) * _polygonal_tool(6)
    path = tmp_path / "oblique-parallel-ends.step"
    export_step(part, path)
    assert recognise_section_passages(import_step(path)) == recognise_section_passages(part)


def test_oblique_parallel_ends_publish_owned_wall_evidence_in_public_document():
    part = Box(60, 50, 20) - Rot(0, 20, 0) * _polygonal_tool(6)
    compound = Compound([part, Pos(100, 0, 0) * part])
    document = build_section_recess_document(compound)
    records = [
        record for record in document.occurrences if record.classification.feature_kind == "passage"
    ]
    assert len(records) == 2
    assert records[0].body != records[1].body
    for record in records:
        assert record.classification.section_shape == "hexagonal"
        assert len(record.evidence.defining_faces) == len(record.evidence.constituent_faces) == 6
        assert record.geometry.ends.low.gradient == record.geometry.ends.high.gradient
        assert math.hypot(*record.geometry.ends.low.gradient) == pytest.approx(
            math.tan(math.radians(20)), abs=1e-6
        )
    assert set(records[0].evidence.constituent_faces).isdisjoint(
        records[1].evidence.constituent_faces
    )


@pytest.mark.parametrize("sides", (3, 4, 6))
@pytest.mark.parametrize(
    ("wedge", "expected"),
    (
        ((60, 50, 20, 0, -8, 60, 7), ((0.0, -0.16), (0.0, -0.26))),
        ((60, 50, 24, 0, -10, 60, 8), ((0.0, -0.2), (0.0, -0.32))),
    ),
)
def test_authored_polygonal_passages_cover_distinct_planar_terminations(
    sides: int,
    wedge: tuple[float, float, float, float, float, float, float],
    expected: tuple[tuple[float, float], tuple[float, float]],
) -> None:
    (record,) = recognise_section_passages(Wedge(*wedge) - _polygonal_tool(sides))

    assert len(record.section.boundary) == sides
    assert record.ends.low_gradient == pytest.approx(expected[0], abs=1e-6)
    assert record.ends.high_gradient == pytest.approx(expected[1], abs=1e-6)


def test_sloped_passage_is_covariant_and_scale_preserves_end_gradients() -> None:
    source = Wedge(60, 50, 20, 0, -10, 60, 5) - _polygonal_tool(6)
    original = recognise_section_passages(source)[0]
    rotated = Rot(17, 23, 31) * source
    moved = Pos(17, -9, 4) * rotated
    scaled = rotated.scale(2.5)

    rotated_record = recognise_section_passages(rotated)[0]
    moved_record = recognise_section_passages(moved)[0]
    scaled_record = recognise_section_passages(scaled)[0]

    assert moved_record.ends == rotated_record.ends
    assert scaled_record.ends == rotated_record.ends
    assert sorted(
        math.hypot(*value)
        for value in (moved_record.ends.low_gradient, moved_record.ends.high_gradient)
    ) == pytest.approx(
        sorted(
            math.hypot(*value)
            for value in (original.ends.low_gradient, original.ends.high_gradient)
        ),
        abs=1e-6,
    )
    assert scaled_record.run_interval == pytest.approx(
        tuple(value * 2.5 for value in rotated_record.run_interval),
        abs=0.002,
    )


def test_sloped_passage_publishes_walls_only_as_defining_and_constituent_evidence() -> None:
    part = Wedge(60, 50, 20, 0, -10, 60, 5) - _polygonal_tool(6)
    ledger = ClaimLedger(FaceGraph(part))

    records = recognise_section_passages(part, ledger=ledger)
    (candidate,) = ledger.candidate_set(FamilyId.PASSAGES).candidates
    evidence = ledger.snapshot_index()

    assert candidate.record is records[0]
    assert len(evidence.defining_of(candidate)) == 6
    assert evidence.constituent_of(candidate) == evidence.defining_of(candidate)


def test_sloped_passages_preserve_compound_multiplicity_and_presentation_order() -> None:
    first = Wedge(60, 50, 20, 0, -10, 60, 5) - _polygonal_tool(6)
    compound = Compound([first, Pos(100, 0, 0) * first])

    expected = recognise_section_passages(compound)
    reordered = recognise_section_passages(_ReversedFacesPart(compound))  # type: ignore[arg-type]

    assert len(expected) == len(reordered) == 2
    assert reordered == expected


def test_sloped_passage_survives_step_round_trip(tmp_path) -> None:
    source = Rot(17, 23, 31) * (Wedge(60, 50, 20, 0, -10, 60, 5) - _polygonal_tool(6))
    path = tmp_path / "sloped-end-passage.step"

    assert export_step(source, path)
    imported = import_step(path)

    assert recognise_section_passages(imported) == recognise_section_passages(source)


@dataclass(frozen=True)
class _RawPassageOracle:
    walls: tuple[Face, ...]
    run: tuple[float, float, float]
    interval: tuple[float, float]


def _xyz(value) -> tuple[float, float, float]:
    return (float(value.X), float(value.Y), float(value.Z))


def _canonical_direction(value) -> tuple[float, float, float]:
    direction = _xyz(value.normalized())
    dominant = max(range(3), key=lambda index: (round(abs(direction[index]), 6), -index))
    if direction[dominant] < 0.0:
        direction = tuple(-coordinate for coordinate in direction)
    return direction


def _same_shape(left, right) -> bool:
    return bool(left.wrapped.IsSame(right.wrapped))


def _raw_square_oracle(part) -> _RawPassageOracle:
    """Reconstruct the fixture occurrence without production graph/section helpers."""

    walls = tuple(face for face in part.faces() if math.isclose(face.area, 200.0, abs_tol=1e-6))
    assert len(walls) == 4
    long_edges = tuple(
        edge
        for face in walls
        for edge in face.edges()
        if edge.geom_type.name == "LINE" and math.isclose(edge.length, 20.0, abs_tol=1e-6)
    )
    assert len(long_edges) == 8
    directions = tuple(_canonical_direction(edge.tangent_at()) for edge in long_edges)
    run = directions[0]
    assert all(
        sum(a * b for a, b in zip(run, item, strict=True)) > 1.0 - 1e-9 for item in directions
    )
    adjacency = {
        at: {
            other
            for other in range(len(walls))
            if at != other
            and any(
                _same_shape(left, right)
                for left in walls[at].edges()
                for right in walls[other].edges()
            )
        }
        for at in range(len(walls))
    }
    assert all(len(neighbours) == 2 for neighbours in adjacency.values())
    coordinates = tuple(
        sum(a * b for a, b in zip(_xyz(vertex), run, strict=True))
        for face in walls
        for vertex in face.vertices()
    )
    return _RawPassageOracle(walls, run, (min(coordinates), max(coordinates)))


def test_public_nested_schema_and_json_shape() -> None:
    (record,) = recognise_section_passages(_square())
    assert record == SectionPassage(
        PassageFrame(
            (0.0, 0.0, 0.0),
            (0.0, 0.0, 1.0),
            (1.0, 0.0, 0.0),
            (0.0, 1.0, 0.0),
        ),
        (-10.0, 10.0),
        PassageSection(
            (
                PassageSectionVertex((-5.0, -5.0), 0.0),
                PassageSectionVertex((5.0, -5.0), 0.0),
                PassageSectionVertex((5.0, 5.0), 0.0),
                PassageSectionVertex((-5.0, 5.0), 0.0),
            )
        ),
        PassageEnds(False, False),
    )
    payload = json.loads(json.dumps(record.to_dict()))
    assert list(payload) == ["frame", "run_interval", "section", "ends"]
    assert payload["ends"] == {
        "low_capped": False,
        "high_capped": False,
        "low_gradient": [0.0, 0.0],
        "high_gradient": [0.0, 0.0],
    }


def test_rich_api_is_the_exact_passages_candidate_authority() -> None:
    part = _square()
    ledger = ClaimLedger(FaceGraph(part))
    records = recognise_section_passages(part, ledger=ledger)
    (candidate,) = ledger.candidate_set(FamilyId.PASSAGES).candidates
    assert candidate.record is records[0]
    assert len(ledger.defining_of(candidate)) == len(records[0].section.boundary) == 4
    compatibility = ledger.snapshot_index().passage_compatibility(candidate)
    assert compatibility.eligible is True
    assert compatibility.axis == "z"
    assert compatibility.legacy_ordinal == 0
    assert compatibility.at == (0.0, 0.0, 0.0)


def test_legacy_ledger_refuses_before_any_geometry_work(monkeypatch) -> None:
    import quiddity.passages as module

    ledger = ClaimLedger(FaceGraph(_square()))
    monkeypatch.setattr(module, "FaceGraph", lambda *args, **kwargs: pytest.fail("geometry ran"))
    with pytest.raises(
        PassageCompatibilityError,
        match=r"recognise_passages\(\.\.\., ledger=\.\.\.\) is unavailable from 0\.4\.0",
    ):
        recognise_passages(object(), ledger=ledger)  # type: ignore[arg-type]
    assert ledger.candidate_set(FamilyId.PASSAGES).candidates == ()


def test_aggregate_has_one_rich_authority_and_legacy_projection() -> None:
    result = build_recognition_result(_square())
    assert len(result.section_passages) == len(result.passages) == 1
    assert result.passages == tuple(recognise_passages(_square()))


def test_duplicate_legacy_defining_roster_refuses_before_publication(monkeypatch) -> None:
    import quiddity.passages as passages_module

    part = _square()
    graph = FaceGraph(part)
    roster = passages_module._legacy_roster(part, graph)
    assert len(roster) == 1
    monkeypatch.setattr(passages_module, "_legacy_roster", lambda *_args: roster + roster)
    ledger = ClaimLedger(graph)
    with pytest.raises(ValueError, match="competing defining-node matches"):
        passages_module._discover_section_passages(part, graph, ledger.writer)
    assert ledger.candidate_set(FamilyId.PASSAGES).candidates == ()


def test_graph_identical_duplicate_rich_proposal_collapses_to_one_candidate(monkeypatch) -> None:
    import quiddity.passages as passages_module
    from quiddity._section_passages import section_ring_proposals

    part = _square()
    graph = FaceGraph(part)
    (proposal,) = section_ring_proposals(part, graph)
    monkeypatch.setattr(
        passages_module,
        "section_ring_proposals",
        lambda *_args: [proposal, proposal],
    )
    ledger = ClaimLedger(graph)
    records = passages_module._discover_section_passages(part, graph, ledger.sink)
    assert len(records) == 1
    (candidate,) = ledger.candidate_set(FamilyId.PASSAGES).candidates
    assert candidate.record is records[0]


def test_legacy_roster_assigns_ordinals_after_the_stable_public_sort(monkeypatch) -> None:
    import quiddity.passages as passages_module
    from quiddity._rings import Ring

    part = _square()
    graph = FaceGraph(part)
    nodes = graph.nodes
    square = Ring(
        nodes[:4],
        ((-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)),
        2,
        -5.0,
        5.0,
        (frozenset(), frozenset()),
    )
    triangle = Ring(
        nodes[4:7],
        ((-1.0, -1.0), (1.0, -1.0), (0.0, 2.0)),
        2,
        -5.0,
        5.0,
        (frozenset(), frozenset()),
    )
    monkeypatch.setattr(passages_module, "rings", lambda *_args: iter((square, triangle)))
    roster = passages_module._legacy_roster(part, graph)
    assert [record.sides for record, _nodes in roster] == [4, 3]
    assert [record for record, _nodes in roster] == passages_module._discover_passages(
        part, graph, None
    )


def test_same_legacy_defining_set_cannot_issue_competing_rich_records(monkeypatch) -> None:
    import quiddity.passages as passages_module
    from quiddity._section_passages import section_ring_proposals

    part = Rot(17, 23, 31) * (
        Box(80, 40, 20) - Pos(-20, 0, 0) * Box(8, 8, 60) - Pos(20, 0, 0) * Box(12, 6, 60)
    )
    graph = FaceGraph(part)
    first, second = section_ring_proposals(part, graph)
    competing = replace(second, nodes=first.nodes)
    monkeypatch.setattr(
        passages_module,
        "section_ring_proposals",
        lambda *_args: [first, competing],
    )
    ledger = ClaimLedger(graph)
    with pytest.raises(ValueError, match="one passage defining set produced competing records"):
        passages_module._discover_section_passages(part, graph, ledger.sink)
    assert ledger.candidate_set(FamilyId.PASSAGES).candidates == ()


def test_oblique_passage_is_rich_only_and_keeps_exact_wall_ownership() -> None:
    part = Rot(17, 23, 31) * _square()
    oracle = _raw_square_oracle(part)
    assert recognise_passages(part) == []
    graph = FaceGraph(part)
    ledger = ClaimLedger(graph)
    (record,) = recognise_section_passages(part, ledger=ledger)
    assert record.frame.run == pytest.approx(oracle.run, abs=5e-7)
    assert record.run_interval == pytest.approx(oracle.interval, abs=5e-4)
    assert record.frame.run == (0.390731, -0.26913, 0.880283)
    assert len(record.section.boundary) == 4
    (candidate,) = ledger.candidate_set(FamilyId.PASSAGES).candidates
    assert candidate.record is record
    compatibility = ledger.snapshot_index().passage_compatibility(candidate)
    assert compatibility.eligible is False
    assert compatibility.axis is None
    defining = ledger.defining_of(candidate)
    assert len(defining) == 4
    assert all(
        any(_same_shape(graph.face(node), wall) for wall in oracle.walls) for node in defining
    )
    assert all(
        any(_same_shape(graph.face(node), wall) for node in defining) for wall in oracle.walls
    )
    result = build_recognition_result(part)
    assert result.section_passages == (record,)
    assert result.passages == ()


@pytest.mark.parametrize(
    ("section", "wall_count"),
    [
        (3, 3),
        (6, 6),
        (((-8, -8), (8, -8), (8, 8), (3, 8), (3, -3), (-3, -3), (-3, 8), (-8, 8)), 8),
    ],
    ids=("triangle", "hexagon", "concave-u"),
)
def test_free_axis_line_sections_preserve_complete_wall_cycles(section, wall_count) -> None:
    part = Rot(17, 23, 31) * (Box(60, 40, 20) - _polygonal_tool(section))
    ledger = ClaimLedger(FaceGraph(part))
    (record,) = recognise_section_passages(part, ledger=ledger)
    assert len(record.section.boundary) == wall_count
    (candidate,) = ledger.candidate_set(FamilyId.PASSAGES).candidates
    assert candidate.record is record
    assert len(ledger.defining_of(candidate)) == wall_count
    assert recognise_passages(part) == []


def test_multiple_unequal_free_axis_occurrences_on_one_solid_are_distinct() -> None:
    part = Box(80, 40, 20)
    part = part - Pos(-20, 0, 0) * Box(8, 8, 60) - Pos(20, 0, 0) * Box(12, 6, 60)
    part = Rot(17, 23, 31) * part
    ledger = ClaimLedger(FaceGraph(part))
    records = recognise_section_passages(part, ledger=ledger)
    candidates = ledger.candidate_set(FamilyId.PASSAGES).candidates
    assert len(records) == len(candidates) == 2
    assert all(
        candidate.record is record for candidate, record in zip(candidates, records, strict=True)
    )
    assert records[0] != records[1]


def test_equal_coincident_solids_keep_two_occurrence_identities() -> None:
    first = Rot(17, 23, 31) * _square()
    second = Rot(17, 23, 31) * _square()
    part = Compound([first, second])
    ledger = ClaimLedger(FaceGraph(part))
    records = recognise_section_passages(part, ledger=ledger)  # type: ignore[arg-type]
    candidates = ledger.candidate_set(FamilyId.PASSAGES).candidates
    assert len(records) == len(candidates) == 2
    assert records[0] == records[1]
    assert records[0] is not records[1]
    assert candidates[0] is not candidates[1]


def test_material_classification_reads_each_graph_authorized_solid_not_the_compound(
    monkeypatch,
) -> None:
    import quiddity._section_passages as section_module

    first = Rot(17, 23, 31) * _square()
    second = Pos(140, 0, 0) * first
    part = Compound([first, second])
    original = section_module._material_fraction
    classified = []

    def same_solid_only(solid, probe):
        assert len(solid.solids()) == 1
        classified.append(solid)
        return original(solid, probe)

    monkeypatch.setattr(section_module, "_material_fraction", same_solid_only)
    assert len(recognise_section_passages(part)) == 2  # type: ignore[arg-type]
    assert classified


@pytest.mark.parametrize(
    "part",
    (
        Pos(17, -9, 4) * Rot(17, 23, 31) * _square(),
        (Rot(17, 23, 31) * _square()).mirror(Plane.YZ),
        (Rot(17, 23, 31) * _square()).scale(2.5),
    ),
)
def test_free_axis_passage_survives_translation_mirror_and_uniform_scale(part) -> None:
    graph = FaceGraph(part)
    ledger = ClaimLedger(graph)
    records = recognise_section_passages(part, ledger=ledger)  # type: ignore[arg-type]
    assert len(records) == 1
    (candidate,) = ledger.candidate_set(FamilyId.PASSAGES).candidates
    assert candidate.record is records[0]
    assert len(ledger.defining_of(candidate)) == 4


def test_face_and_solid_presentation_reversal_preserves_record_and_wall_shapes() -> None:
    first = Rot(17, 23, 31) * _square()
    second = Pos(140, 0, 0) * Rot(17, 23, 31) * (Box(70, 50, 24) - Box(12, 8, 60))
    part = Compound([first, second])

    def observed(supplied):
        graph = FaceGraph(supplied)
        ledger = ClaimLedger(graph)
        records = recognise_section_passages(supplied, ledger=ledger)
        candidates = ledger.candidate_set(FamilyId.PASSAGES).candidates
        assert len(records) == len(candidates) == 2
        assert all(
            candidate.record is record
            for candidate, record in zip(candidates, records, strict=True)
        )
        walls = tuple(
            tuple(graph.face(node) for node in ledger.defining_of(candidate))
            for candidate in candidates
        )
        return records, walls

    ordinary, ordinary_walls = observed(part)
    reversed_records, reversed_walls = observed(_ReversedFacesPart(part))
    assert reversed_records == ordinary
    for ordinary_group, reversed_group in zip(ordinary_walls, reversed_walls, strict=True):
        assert all(
            any(_same_shape(left, right) for right in reversed_group) for left in ordinary_group
        )
        assert all(
            any(_same_shape(left, right) for right in ordinary_group) for left in reversed_group
        )


def test_full_discovery_accepts_one_junction_split_into_collinear_occurrences() -> None:
    part = Rot(17, 23, 31) * _square()
    oracle = _raw_square_oracle(part)
    target = next(
        left
        for left in oracle.walls[0].edges()
        for right in oracle.walls[1].edges()
        if _same_shape(left, right)
    )
    memo = _SplitJunctionEdges(target)
    graph = FaceGraph(part, face_edges=memo)
    ledger = ClaimLedger(graph)
    (record,) = recognise_section_passages(part, face_edges=memo, ledger=ledger)
    (candidate,) = ledger.candidate_set(FamilyId.PASSAGES).candidates
    assert candidate.record is record
    defining = ledger.defining_of(candidate)
    assert len(defining) == 4
    assert all(
        any(_same_shape(graph.face(node), wall) for wall in oracle.walls) for node in defining
    )
    assert all(
        any(_same_shape(graph.face(node), wall) for node in defining) for wall in oracle.walls
    )
    assert record == recognise_section_passages(part)[0]


def test_late_foreign_occurrence_refuses_before_any_candidate_prefix(monkeypatch) -> None:
    import quiddity.passages as passages_module
    from quiddity._section_passages import section_ring_proposals

    part = Rot(17, 23, 31) * _square()
    graph = FaceGraph(part)
    valid = section_ring_proposals(part, graph)
    foreign_part = Pos(100, 0, 0) * part
    foreign = section_ring_proposals(foreign_part, FaceGraph(foreign_part))
    assert len(valid) == len(foreign) == 1
    monkeypatch.setattr(
        passages_module,
        "section_ring_proposals",
        lambda supplied_part, supplied_graph: [valid[0], foreign[0]],
    )
    ledger = ClaimLedger(graph)
    with pytest.raises(ValueError, match="not issued by this graph|body authority changed"):
        passages_module._discover_section_passages(part, graph, ledger.writer)
    assert ledger.candidate_set(FamilyId.PASSAGES).candidates == ()


def test_aggregate_late_passage_failure_has_no_completion_or_occurrence_capability(
    monkeypatch,
) -> None:
    import quiddity.passages as passages_module
    from quiddity._section_passages import section_ring_proposals

    first = Rot(17, 23, 31) * _square()
    part = Compound([first, Pos(150, 0, 0) * first])
    context = start(part)
    proposals = section_ring_proposals(part, context.graph)
    assert len(proposals) == 2
    foreign_part = Pos(400, 0, 0) * first
    (foreign,) = section_ring_proposals(foreign_part, FaceGraph(foreign_part))
    malformed = replace(proposals[1], solid=foreign.solid)
    monkeypatch.setattr(
        passages_module,
        "section_ring_proposals",
        lambda supplied_part, supplied_graph: [proposals[0], malformed],
    )
    ledger = ClaimLedger(context.graph, definitions=PHYSICAL_DEFINITIONS)
    with pytest.raises(ValueError, match="body authority changed"):
        _discover_all(context, ledger)
    assert ledger.candidate_set(FamilyId.PASSAGES).candidates == ()
    assert FamilyId.PASSAGES not in ledger._issuer._completed
    assert FamilyId.PASSAGES not in ledger._issuer._completed_occurrences
    assert all(
        FamilyId.PASSAGES not in snapshot.occurrences
        for snapshot in ledger._issuer._restricted_snapshots.values()
    )


@pytest.mark.parametrize(
    ("fractions", "accepted"),
    (
        ((1e-9, 1e-9, 1e-9), True),
        ((math.nextafter(1e-9, math.inf), 0.0, 0.0), False),
        ((0.0, math.nextafter(1e-9, math.inf), 0.0), False),
        ((0.0, 0.0, math.nextafter(1e-9, math.inf)), False),
    ),
)
def test_full_prism_and_both_end_slabs_share_the_closed_material_boundary(
    monkeypatch, fractions: tuple[float, float, float], accepted: bool
) -> None:
    import quiddity._section_passages as module

    part = Rot(17, 23, 31) * _square()
    (proposal,) = module.section_ring_proposals(part, FaceGraph(part))
    pending = iter(fractions)
    monkeypatch.setattr(module, "_material_fraction", lambda part, probe: next(pending))
    assert (
        module._void_and_open(part, proposal.frame, proposal.run_interval, proposal.section)
        is accepted
    )


def test_full_prism_coordinate_floor_is_fail_closed_at_equality(monkeypatch) -> None:
    import quiddity._section_passages as module

    part = Rot(17, 23, 31) * _square()
    (proposal,) = module.section_ring_proposals(part, FaceGraph(part))
    with pytest.raises(ValueError, match="too short"):
        module._probe_prism(
            proposal.frame,
            (0.0, 2 * module._COORD_FLOOR),
            proposal.section,
        )
    sentinel = object()
    captured = []
    monkeypatch.setattr(
        module.Solid,
        "extrude",
        lambda face, vector: captured.append(vector.length) or sentinel,
    )
    assert (
        module._probe_prism(
            proposal.frame,
            (0.0, math.nextafter(2 * module._COORD_FLOOR, math.inf)),
            proposal.section,
        )
        is sentinel
    )
    assert captured[0] > 0.0


def test_candidate_compatibility_fact_is_issuer_revalidated() -> None:
    part = _square()
    ledger = ClaimLedger(FaceGraph(part))
    recognise_section_passages(part, ledger=ledger)
    (candidate,) = ledger.candidate_set(FamilyId.PASSAGES).candidates
    index = ledger.snapshot_index()
    original = candidate.compatibility
    object.__setattr__(candidate, "compatibility", None)
    with pytest.raises(ValueError, match="issued state"):
        index.passage_compatibility(candidate)
    object.__setattr__(candidate, "compatibility", original)
    assert original is not None
    object.__setattr__(original, "axis", "bad")
    with pytest.raises(ValueError, match="compatibility axis is invalid"):
        index.passage_compatibility(candidate)
    object.__setattr__(original, "axis", "z")
    assert index.passage_compatibility(candidate) is original


def test_oblique_passage_step_round_trip_preserves_schema_and_wall_count(tmp_path) -> None:
    source = Rot(17, 23, 31) * _square()
    source_oracle = _raw_square_oracle(source)
    path = tmp_path / "oblique-section-passage.step"
    assert export_step(source, path)
    imported = import_step(path)
    imported_oracle = _raw_square_oracle(imported)
    assert imported_oracle.run == pytest.approx(source_oracle.run, abs=1e-9)
    assert imported_oracle.interval == pytest.approx(source_oracle.interval, abs=1e-8)
    assert recognise_section_passages(imported) == recognise_section_passages(source)
    assert recognise_passages(imported) == []


def test_whole_occurrence_serialization_displacement_refuses_before_evidence() -> None:
    accepted = Rot(17, 23, 31) * (Box(60, 40, 5000) - Box(10, 10, 15000))
    assert len(recognise_section_passages(accepted)) == 1

    refused = Rot(17, 23, 31) * (Box(60, 40, 10000) - Box(10, 10, 30000))
    ledger = ClaimLedger(FaceGraph(refused))
    with pytest.raises(ValueError, match="serialization exceeds the displacement bound"):
        recognise_section_passages(refused, ledger=ledger)
    assert ledger.candidate_set(FamilyId.PASSAGES).candidates == ()


@pytest.mark.parametrize(
    "part",
    [
        Box(60, 40, 20) - Pos(0, 0, 5) * Box(10, 10, 20),
        (Box(60, 40, 20) - Box(10, 10, 60)) + Box(10, 10, 2),
        (Box(60, 40, 20) - Box(10, 10, 60)) + Pos(0, 4, 0) * Box(10, 0.1, 5),
        Box(60, 40, 20) - Cone(5, 7, 60),
    ],
    ids=("one-cap", "membrane", "partial-rib", "taper"),
)
def test_caps_obstructions_and_taper_refuse_without_evidence(part) -> None:
    part = Rot(17, 23, 31) * part
    ledger = ClaimLedger(FaceGraph(part))
    assert recognise_section_passages(part, ledger=ledger) == []
    assert ledger.candidate_set(FamilyId.PASSAGES).candidates == ()


def test_open_shell_cannot_supply_body_authority() -> None:
    solid = Rot(17, 23, 31) * _square()
    shell = Shell(solid.faces())
    ledger = ClaimLedger(FaceGraph(shell))
    assert recognise_section_passages(shell, ledger=ledger) == []  # type: ignore[arg-type]
    assert ledger.candidate_set(FamilyId.PASSAGES).candidates == ()


@pytest.mark.parametrize(
    "record",
    [
        lambda: PassageFrame(
            (0.0, 0.0, 0.0),
            (0.0, 0.0, -1.0),
            (1.0, 0.0, 0.0),
            (0.0, -1.0, 0.0),
        ),
        lambda: PassageFrame(
            (0.0, 0.0, 0.0),
            (0.0, 0.0, 1.0),
            (0.707107, 0.707107, 0.0),
            (-0.707107, 0.707107, 0.0),
        ),
        lambda: PassageFrame(
            (0.0, 0.0, 0.0),
            (0.1234567, 0.0, 0.992349952),
            (0.0, 1.0, 0.0),
            (-0.992349952, 0.0, 0.1234567),
        ),
        lambda: PassageEnds(0, False),
        lambda: SectionPassage(
            PassageFrame(
                (0.0, 0.0, 0.0),
                (0.0, 0.0, 1.0),
                (1.0, 0.0, 0.0),
                (0.0, 1.0, 0.0),
            ),
            (1.0, 1.0),
            PassageSection(
                (
                    PassageSectionVertex((-1.0, -1.0), 0.0),
                    PassageSectionVertex((1.0, -1.0), 0.0),
                    PassageSectionVertex((0.0, 2.0), 0.0),
                )
            ),
            PassageEnds(False, False),
        ),
    ],
)
def test_public_schema_refuses_noncanonical_values(record) -> None:
    with pytest.raises(ValueError):
        record()


def test_public_frame_accepts_six_decimal_unit_rounding_bound() -> None:
    frame = PassageFrame(
        (0.0, 0.0, 0.0),
        (0.0, 0.0, 1.0),
        (0.999999, 0.0, 0.0),
        (0.0, 1.0, 0.0),
    )

    assert frame.u == (0.999999, 0.0, 0.0)


def test_public_frame_refuses_direction_beyond_six_decimal_rounding_bound() -> None:
    with pytest.raises(ValueError, match="frame directions must be unit length"):
        PassageFrame(
            (0.0, 0.0, 0.0),
            (0.0, 0.0, 1.0),
            (1.000002, 0.0, 0.0),
            (0.0, 1.0, 0.0),
        )


def test_public_passage_ends_accept_planar_local_gradients() -> None:
    ends = PassageEnds(False, False, (-0.25, 0.125), (0.5, -0.375))

    assert ends.low_gradient == (-0.25, 0.125)
    assert ends.high_gradient == (0.5, -0.375)


def test_public_passage_ends_refuse_unserialized_gradient() -> None:
    with pytest.raises(ValueError, match="low_gradient must use at most 6 decimal places"):
        PassageEnds(False, False, (0.0000001, 0.0), (0.0, 0.0))


def test_schema_v2_section_point_precision_is_bounded_at_four_decimals() -> None:
    section = PassageSection(
        (
            PassageSectionVertex((-1.0001, -1.0), 0.0),
            PassageSectionVertex((1.0001, -1.0), 0.0),
            PassageSectionVertex((0.0, 2.0), 0.0),
        )
    )
    assert section.boundary[0].point == (-1.0001, -1.0)

    with pytest.raises(ValueError, match="point must use at most 4 decimal places"):
        PassageSectionVertex((0.00001, 0.0), 0.0)


def test_section_is_recanonicalized_after_public_coordinate_rounding() -> None:
    section = PlanarSection(
        tuple(
            SectionVertex(point)
            for point in (
                (-0.9998444103843976, 0.9999182634638213),
                (-0.9994589537203384, -0.9999207651031005),
                (1.000720579557841, -1.000535647743874),
                (1.0000275433263752, 1.0009049347765366),
            )
        )
    )

    serialized = _section_passage_record(
        SimpleNamespace(
            frame=LocalFrame.canonical((0.0, 0.0, 1.0), (0.0, 0.0, 0.0)),
            run_interval=(-1.0, 1.0),
            section=section,
            low_gradient=(0.0, 0.0),
            high_gradient=(0.0, 0.0),
        )
    ).section

    assert serialized.boundary[0].point == (-0.9995, -0.9999)
    assert tuple(vertex.point for vertex in serialized.boundary) == (
        (-0.9995, -0.9999),
        (1.0007, -1.0005),
        (1.0, 1.0009),
        (-0.9998, 0.9999),
    )


def test_public_section_passage_refuses_crossing_termination_planes() -> None:
    section = PassageSection(
        (
            PassageSectionVertex((-2.0, -1.0), 0.0),
            PassageSectionVertex((2.0, -1.0), 0.0),
            PassageSectionVertex((2.0, 1.0), 0.0),
            PassageSectionVertex((-2.0, 1.0), 0.0),
        )
    )
    frame = PassageFrame(
        (0.0, 0.0, 0.0),
        (0.0, 0.0, 1.0),
        (1.0, 0.0, 0.0),
        (0.0, 1.0, 0.0),
    )

    with pytest.raises(ValueError, match="termination planes must not cross"):
        SectionPassage(
            frame,
            (-1.0, 1.0),
            section,
            PassageEnds(False, False, (1.0, 0.0), (-1.0, 0.0)),
        )


def test_public_section_passage_refuses_curved_section_with_sloped_ends() -> None:
    section = PassageSection(
        (
            PassageSectionVertex((-1.0, 0.0), 1.0),
            PassageSectionVertex((1.0, 0.0), 1.0),
        )
    )
    frame = PassageFrame(
        (0.0, 0.0, 0.0),
        (0.0, 0.0, 1.0),
        (1.0, 0.0, 0.0),
        (0.0, 1.0, 0.0),
    )

    with pytest.raises(ValueError, match="sloped passage terminations require a line-only"):
        SectionPassage(
            frame,
            (-1.0, 1.0),
            section,
            PassageEnds(False, False, (0.1, 0.0), (0.0, 0.0)),
        )
