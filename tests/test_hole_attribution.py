"""F5: Hole occurrences own their complete original cylindrical roles."""

from __future__ import annotations

import ast
import copy
import math
from dataclasses import dataclass, replace
from pathlib import Path
from typing import cast

import pytest
from build123d import (
    Box,
    Circle,
    Compound,
    Cone,
    Cylinder,
    GeomType,
    Part,
    Plane,
    Pos,
    Rectangle,
    Rot,
    Shell,
    Sphere,
    Vector,
    chamfer,
    export_step,
    extrude,
    fillet,
    import_step,
)
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepBuilderAPI import BRepBuilderAPI_NurbsConvert
from OCP.GeomAbs import GeomAbs_Cone, GeomAbs_Cylinder, GeomAbs_Plane

from quiddity import recognise_holes
from quiddity._adjacency import (
    FaceGraph,
    FaceNode,
    SolidRef,
    edge_face_map,
    frame_points_outward,
)
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._cylinder_substrate import (
    _STACK_GAP_FRAC,
    _cyl_group_key,
    _line_key,
    _merge_runs,
    analyse_cylinders,
    full_cylinders,
)
from quiddity._effective_surfaces import SurfaceKind, SurfaceProvenance
from quiddity._geometry import length_tol, quantise
from quiddity._hole_features import (
    CounterBore,
    HoleRecord,
    SegmentEvidence,
    _bore_depth,
    _classify_end,
    _discover_holes,
    _drilled_from,
    _end_partners,
    _near_side_steps,
    _same_diameter,
    _segments,
)
from quiddity._registry import PHYSICAL_DEFINITIONS
from quiddity.countersinks import (
    CounterSink,
    _discover_countersinks,
    countersink_matches_hole,
    recognise_countersinks,
)
from quiddity.result import _take_inventory

ROOT = Path(__file__).parents[1]


def test_recovered_hole_candidate_retains_original_cylinder_dependency() -> None:
    native = Box(12, 12, 10) - Cylinder(2, 10)
    converted = Part(BRepBuilderAPI_NurbsConvert(native.wrapped, True).Shape())
    ledger = ClaimLedger(FaceGraph(converted))

    records = _discover_holes(converted, writer=ledger.writer)

    expected = recognise_holes(native)
    assert records == recognise_holes(converted) == expected
    assert _take_inventory(converted).result.holes == tuple(expected)
    (candidate,) = ledger.candidate_set(FamilyId.HOLES).candidates
    (surface_use,) = candidate.evidence.surfaces
    assert surface_use.node in candidate.evidence.defining
    assert surface_use.surface.kind is SurfaceKind.CYLINDER
    assert surface_use.surface.provenance is SurfaceProvenance.RECOVERED
    assert surface_use.material_side is not None
    assert surface_use.material_side.candidate_outward_sign == -1


def test_nonprincipal_converted_hole_keeps_standalone_and_aggregate_parity() -> None:
    native = Rot(31, 17, 43) * (Box(12, 12, 10) - Cylinder(2, 10))
    converted = Part(BRepBuilderAPI_NurbsConvert(native.wrapped, True).Shape())
    expected = recognise_holes(native)

    assert recognise_holes(converted) == expected
    assert _take_inventory(converted).result.holes == tuple(expected)


def test_recovered_hole_end_plane_refusal_cannot_claim_through(monkeypatch) -> None:
    import quiddity._effective_surfaces as surfaces

    native = Box(12, 12, 10) - Cylinder(2, 10)
    converted = Part(BRepBuilderAPI_NurbsConvert(native.wrapped, True).Shape())
    monkeypatch.setattr(
        surfaces._EffectiveFaceSurfaces,
        "_certify_plane",
        lambda _self, _node, _surface: surfaces.MaterialSideRefusalReason.SAMPLES_DISAGREE,
    )

    (refused_end,) = recognise_holes(converted)

    assert refused_end.bottom == "unknown"


def _through():
    return Box(60, 60, 20) - Cylinder(5, 20)


def _blind():
    return Box(60, 60, 20) - Pos(0, 0, 4) * Cylinder(5, 12)


def _counterbore():
    return Box(60, 60, 20) - Cylinder(5, 20) - Pos(0, 0, 7) * Cylinder(9, 6)


def _spotface_stack():
    return (
        Box(100, 100, 40)
        - Pos(0, 0, 17.5) * Cylinder(30, 5)
        - Pos(0, 0, 12) * Cylinder(9, 6)
        - Pos(0, 0, 1.5) * Cylinder(5.05, 15)
    )


def _countersunk():
    return Box(60, 60, 20) - Cylinder(2.5, 20) - Pos(0, 0, 7.5) * Cone(2.5, 5, 5)


def _drill_tool(radius, depth, top_z):
    tip = radius / math.tan(math.radians(59))
    bottom = top_z - depth
    return Pos(0, 0, top_z - depth / 2) * Cylinder(radius, depth) + Pos(
        0, 0, bottom - tip / 2
    ) * Cone(0, radius, tip)


def _line_distance(point, line, direction) -> float:
    offset = tuple(point[i] - line[i] for i in range(3))
    along = sum(offset[i] * direction[i] for i in range(3))
    return math.sqrt(sum((offset[i] - along * direction[i]) ** 2 for i in range(3)))


@dataclass(frozen=True)
class _CylinderFact:
    node: FaceNode
    direction: tuple[float, float, float]
    line: tuple[float, float, float]
    diameter: float
    lo: float
    hi: float


@dataclass(frozen=True)
class _ExpectedHole:
    record: HoleRecord
    nodes: frozenset
    solid: object


@dataclass(frozen=True)
class _ExpectedCounterSink:
    record: CounterSink
    node: FaceNode
    solid: SolidRef


def _canonical_direction(direction) -> tuple[float, float, float]:
    unit = (direction.X(), direction.Y(), direction.Z())
    for value in unit:
        if abs(value) > 1e-10:
            return tuple(-item for item in unit) if value < 0 else unit
    raise AssertionError("cylinder direction is zero")


def _face_interval(face, direction) -> tuple[float, float]:
    points = [vertex.center() for edge in face.edges() for vertex in edge.vertices()]
    points.extend(edge.center() for edge in face.edges())
    assert points
    values = [
        point.X * direction[0] + point.Y * direction[1] + point.Z * direction[2] for point in points
    ]
    return min(values), max(values)


def _raw_internal_cylinders(graph: FaceGraph, solid) -> list[_CylinderFact]:
    facts = []
    for face in solid.faces():
        node = graph.require_node(face)
        surface = BRepAdaptor_Surface(face.wrapped)
        if surface.GetType() != GeomAbs_Cylinder or frame_points_outward(face):
            continue
        cylinder = surface.Cylinder()
        direction = _canonical_direction(cylinder.Axis().Direction())
        location = cylinder.Axis().Location()
        raw_line = (location.X(), location.Y(), location.Z())
        along = sum(raw_line[i] * direction[i] for i in range(3))
        line = tuple(raw_line[i] - along * direction[i] for i in range(3))
        lo, hi = _face_interval(face, direction)
        facts.append(_CylinderFact(node, direction, line, 2 * cylinder.Radius(), lo, hi))
    return facts


def _line_groups(facts: list[_CylinderFact]) -> list[list[_CylinderFact]]:
    groups: list[list[_CylinderFact]] = []
    for fact in facts:
        for group in groups:
            first = group[0]
            if (
                abs(sum(a * b for a, b in zip(first.direction, fact.direction, strict=True)))
                > 0.999999
                and _line_distance(fact.line, first.line, first.direction) <= 1e-5
            ):
                group.append(fact)
                break
        else:
            groups.append([fact])
    return groups


def _connected_stacks(solid, facts: list[_CylinderFact]) -> list[list[_CylinderFact]]:
    """Split coaxial facts when original material separates their axial voids."""

    ordered = sorted(facts, key=lambda fact: (fact.lo, fact.hi, fact.diameter))
    stacks: list[list[_CylinderFact]] = []
    for fact in ordered:
        if not stacks:
            stacks.append([fact])
            continue
        previous_hi = max(item.hi for item in stacks[-1])
        if fact.lo <= previous_hi + 1e-5:
            stacks[-1].append(fact)
            continue
        midpoint = (previous_hi + fact.lo) / 2
        point = _point_on_line(fact.line, fact.direction, midpoint)
        if solid.is_inside(Vector(*point)):
            stacks.append([fact])
        else:
            # A transverse interruption, relief, or transition leaves the centreline
            # void and therefore belongs to this same logical axial occurrence.
            stacks[-1].append(fact)
    return stacks


def _point_on_line(
    line: tuple[float, float, float],
    direction: tuple[float, float, float],
    coordinate: float,
) -> tuple[float, float, float]:
    return (
        line[0] + coordinate * direction[0],
        line[1] + coordinate * direction[1],
        line[2] + coordinate * direction[2],
    )


def _deep_end_is_conical(solid, line, direction, deep) -> bool:
    for face in solid.faces():
        surface = BRepAdaptor_Surface(face.wrapped)
        if surface.GetType() != GeomAbs_Cone:
            continue
        cone = surface.Cone()
        cone_direction = _canonical_direction(cone.Axis().Direction())
        if abs(sum(a * b for a, b in zip(direction, cone_direction, strict=True))) < 0.999999:
            continue
        location = cone.Axis().Location()
        cone_line = (location.X(), location.Y(), location.Z())
        if _line_distance(cone_line, line, direction) > 1e-5:
            continue
        lo, hi = _face_interval(face, direction)
        if lo - 1e-5 <= deep <= hi + 1e-5:
            return True
    return False


def _fresh_expected_countersinks(part, graph: FaceGraph) -> list[_ExpectedCounterSink]:
    """Reconstruct conical predecessor occurrences from original faces alone."""

    expected = []
    for solid in part.solids() or [part]:
        for face in solid.faces():
            surface = BRepAdaptor_Surface(face.wrapped)
            if surface.GetType() != GeomAbs_Cone:
                continue
            circles = sorted(face.edges().filter_by(GeomType.CIRCLE), key=lambda edge: edge.radius)
            if len(circles) < 2:
                continue
            minor, major = circles[0], circles[-1]
            if major.radius < 1.5 * minor.radius:
                continue
            included = round(2 * abs(math.degrees(surface.Cone().SemiAngle())), 2)
            if included > 160:
                continue
            opening, inner = major.arc_center, minor.arc_center
            delta = (inner.X - opening.X, inner.Y - opening.Y, inner.Z - opening.Z)
            depth = math.sqrt(sum(value * value for value in delta))
            assert depth > 0
            record = CounterSink(
                axis=tuple(round(value / depth, 4) for value in delta),
                location=tuple(round(value, 4) for value in (opening.X, opening.Y, opening.Z)),
                major_diameter=round(2 * major.radius, 4),
                drill_diameter=round(2 * minor.radius, 4),
                included_angle=included,
                depth=round(depth, 4),
            )
            node = graph.require_node(face)
            solid_ref = graph.common_valid_solid((node,))
            assert solid_ref is not None
            expected.append(_ExpectedCounterSink(record, node, solid_ref))
    return expected


def _record_matches(expected: HoleRecord, actual: HoleRecord) -> bool:
    return (
        expected.bottom == actual.bottom
        and expected.cbore == actual.cbore
        and expected.spotface == actual.spotface
        and expected.csink == actual.csink
        and math.isclose(expected.diameter, actual.diameter, rel_tol=1e-6, abs_tol=1e-7)
        and math.isclose(expected.depth, actual.depth, rel_tol=1e-6, abs_tol=1e-7)
        and all(
            math.isclose(left, right, rel_tol=1e-6, abs_tol=1e-7)
            for left, right in zip(expected.axis, actual.axis, strict=True)
        )
        and all(
            math.isclose(left, right, rel_tol=1e-6, abs_tol=0.01)
            for left, right in zip(expected.location, actual.location, strict=True)
        )
    )


def _fresh_expected_holes(part, graph: FaceGraph) -> list[_ExpectedHole]:
    """Reconstruct bounded test-domain Hole occurrences before any recogniser output read."""

    solids = list(part.solids())
    sources = solids if len(solids) > 1 else [part]
    expected: list[_ExpectedHole] = []
    for solid in sources:
        solid_ref = graph.common_valid_solid(graph.require_node(face) for face in solid.faces())
        assert solid_ref is not None
        line_groups = _line_groups(_raw_internal_cylinders(graph, solid))
        for group in (
            stack for line_group in line_groups for stack in _connected_stacks(solid, line_group)
        ):
            direction = group[0].direction
            line = group[0].line
            lo = min(fact.lo for fact in group)
            hi = max(fact.hi for fact in group)
            # A valid opening is an interval end on the body's projected envelope. This is
            # derived from original topology and remains rotation independent.
            body_points = [vertex.center() for face in solid.faces() for vertex in face.vertices()]
            body_s = [
                point.X * direction[0] + point.Y * direction[1] + point.Z * direction[2]
                for point in body_points
            ]
            opening_margin = max(1e-5, 0.51 * min(fact.diameter for fact in group))
            open_lo = abs(lo - min(body_s)) <= opening_margin
            open_hi = abs(hi - max(body_s)) <= opening_margin
            if not (open_lo or open_hi):
                continue
            at_lo = [fact for fact in group if abs(fact.lo - lo) <= 1e-5]
            at_hi = [fact for fact in group if abs(fact.hi - hi) <= 1e-5]
            if open_hi and not open_lo:
                from_hi = True
            elif open_lo and not open_hi:
                from_hi = False
            else:
                from_hi = max(fact.diameter for fact in at_hi) >= max(
                    fact.diameter for fact in at_lo
                )
            ordered = sorted(group, key=lambda fact: fact.hi, reverse=from_hi)
            bore_diameter = min(fact.diameter for fact in ordered)
            bore = [
                fact for fact in group if math.isclose(fact.diameter, bore_diameter, rel_tol=1e-4)
            ]
            bore_top = max(fact.hi for fact in bore) if from_hi else min(fact.lo for fact in bore)
            deep = min(fact.lo for fact in bore) if from_hi else max(fact.hi for fact in bore)
            bottom = "through" if open_lo and open_hi else "flat"
            if bottom != "through":
                deep = min(fact.lo for fact in group) if from_hi else max(fact.hi for fact in group)
                if _deep_end_is_conical(solid, line, direction, deep):
                    bottom = "drill_point"
            near = [
                fact
                for fact in ordered
                if (fact.lo >= bore_top - 1e-5 if from_hi else fact.hi <= bore_top + 1e-5)
                and not math.isclose(fact.diameter, bore_diameter, rel_tol=1e-4)
            ]
            selected: list[_CylinderFact] = []
            specs = []
            minimum = math.inf
            for fact in near:
                if fact.diameter > minimum and not math.isclose(
                    fact.diameter, minimum, rel_tol=1e-4
                ):
                    continue
                minimum = fact.diameter
                if not any(
                    math.isclose(fact.diameter, item.diameter, rel_tol=1e-4) for item in selected
                ):
                    selected.append(fact)
            cbore = spotface = None
            for fact in selected:
                lands = [
                    item
                    for item in near
                    if math.isclose(item.diameter, fact.diameter, rel_tol=1e-4)
                ]
                span = max(item.hi for item in lands) - min(item.lo for item in lands)
                spec = CounterBore(round(fact.diameter, 4), round(span, 2))
                specs.append((spec, lands))
                if spec.depth < 0.2 * spec.diameter:
                    spotface = spotface or spec
                else:
                    cbore = cbore or spec
            selected_specs = {id(spec): lands for spec, lands in specs}
            owner = set(bore)
            for chosen_spec in (cbore, spotface):
                if chosen_spec is not None:
                    owner.update(selected_specs[id(chosen_spec)])
            if bottom != "through":
                owner.update(fact for fact in group if fact.lo - 1e-5 <= deep <= fact.hi + 1e-5)
            opening = hi if from_hi else lo
            axis = (-direction[0], -direction[1], -direction[2]) if from_hi else direction
            record = HoleRecord(
                axis=axis,
                location=_point_on_line(line, direction, opening),
                diameter=bore_diameter,
                depth=round(abs(bore_top - deep), 2),
                bottom=bottom,
                cbore=cbore,
                spotface=spotface,
            )
            expected.append(
                _ExpectedHole(record, frozenset(fact.node for fact in owner), solid_ref)
            )
    return expected


def _claimed(part, **kwargs):
    ledger = ClaimLedger(FaceGraph(part))
    expected = _fresh_expected_holes(part, ledger.graph)
    public = recognise_holes(part, **kwargs)
    records = _discover_holes(part, writer=ledger.writer, **kwargs)
    assert [type(record) for record in records] == [type(record) for record in public]
    assert [record.to_dict() for record in records] == [record.to_dict() for record in public]
    candidates = ledger.candidate_set(FamilyId.HOLES).candidates
    assert len(candidates) == len(records)
    unmatched = list(expected)
    paired = []
    for record in records:
        matches = [item for item in unmatched if _record_matches(item.record, record)]
        assert matches, (record, [item.record for item in unmatched])
        item = matches[0]  # ordinal is used only among already-equal independently derived facts
        unmatched.remove(item)
        paired.append(item)
    assert not unmatched
    for item, record, candidate in zip(paired, records, candidates, strict=True):
        assert candidate.record is record
        assert item.nodes
        assert ledger.defining_of(candidate) == item.nodes
        assert ledger.graph.common_valid_solid(item.nodes) == item.solid
    return records, candidates, ledger


@pytest.mark.parametrize("part", [_through(), _blind(), _counterbore(), _spotface_stack()])
def test_basic_hole_routes_have_exact_cylindrical_owners(part) -> None:
    _claimed(part)


@pytest.mark.parametrize(
    "part",
    [_blind(), Rot(0, 90, 0) * _blind(), Rot(90, 0, 0) * _blind()],
)
def test_blind_hole_constituent_retains_its_exact_terminal_plane(part) -> None:
    (record,), (candidate,), ledger = _claimed(part)
    defining = ledger.defining_of(candidate)
    constituent = ledger.snapshot_index().constituent_of(candidate)
    terminal = constituent - defining

    assert record.bottom == "flat"
    assert defining < constituent and len(terminal) == 1
    assert (
        BRepAdaptor_Surface(ledger.graph.face(next(iter(terminal))).wrapped).GetType()
        == GeomAbs_Plane
    )
    assert ledger.graph.common_valid_solid(constituent) is not None


def test_cached_end_classification_retains_terminal_identity_for_later_projection() -> None:
    part = _blind()
    z_cyls, cross_cyls = analyse_cylinders(part)
    internal = [
        cylinder
        for cylinder in full_cylinders(z_cyls) + full_cylinders(cross_cyls)
        if not cylinder["external"]
    ]
    (segment,) = _segments(internal)
    adjacency = edge_face_map(part.faces())
    cache = {}

    classified = []
    for coordinate, high in ((segment["s_lo"], False), (segment["s_hi"], True)):
        state = _classify_end(segment, coordinate, high, adjacency, cache)
        retained = []
        assert (
            _classify_end(
                segment,
                coordinate,
                high,
                adjacency,
                cache,
                terminal_faces=retained,
            )
            == state
        )
        classified.append((state, retained))

    (terminal,) = [faces for state, faces in classified if state == "flat"]
    assert len(terminal) == 1
    assert BRepAdaptor_Surface(terminal[0].wrapped).GetType() == GeomAbs_Plane

    # A later classification-only cache consumer must remain valid and must not require a
    # destination list merely because identity was retained in the cached proof.
    assert _classify_end(
        segment,
        segment["s_hi"],
        True,
        adjacency,
        cache,
    ) in {"flat", "open"}


def test_uncached_end_classification_optionally_returns_terminal_identity() -> None:
    part = _blind()
    z_cyls, cross_cyls = analyse_cylinders(part)
    internal = [
        cylinder
        for cylinder in full_cylinders(z_cyls) + full_cylinders(cross_cyls)
        if not cylinder["external"]
    ]
    (segment,) = _segments(internal)
    adjacency = edge_face_map(part.faces())
    retained = []

    state = _classify_end(
        segment,
        segment["s_lo"],
        False,
        adjacency,
        terminal_faces=retained,
    )
    assert state in {"flat", "open"}
    assert _classify_end(segment, segment["s_lo"], False, adjacency) == state


def test_through_hole_has_no_terminal_constituent_to_infer() -> None:
    (record,), (candidate,), ledger = _claimed(_through())
    assert record.bottom == "through"
    assert ledger.snapshot_index().constituent_of(candidate) == ledger.defining_of(candidate)


def test_split_and_interrupted_bore_retains_every_original_patch() -> None:
    keyed = (
        Box(60, 40, 10)
        - Cylinder(5, 12)
        - Pos(0, 5, 0) * Box(2, 4, 12)
        - Pos(0, -5, 0) * Box(2, 4, 12)
    )
    records, candidates, ledger = _claimed(keyed)
    assert len(records) == 1
    assert len(ledger.defining_of(candidates[0])) > 1

    crossed = Box(60, 60, 40) - Cylinder(5, 40) - Cylinder(3, 60, rotation=(0, 90, 0))
    records, candidates, ledger = _claimed(crossed)
    assert len(records) == len(candidates) == 2
    for _record, candidate in zip(records, candidates, strict=True):
        assert ledger.defining_of(candidate)


def test_opposed_blind_holes_remain_two_independent_axial_stacks() -> None:
    part = Box(60, 60, 40) - Pos(0, 0, 15) * Cylinder(5, 10) - Pos(0, 0, -15) * Cylinder(5, 10)
    records, candidates, ledger = _claimed(part)
    assert len(records) == 2
    assert {record.bottom for record in records} == {"flat"}
    assert {record.axis for record in records} == {(0.0, 0.0, -1.0), (0.0, 0.0, 1.0)}
    first, second = (ledger.defining_of(candidate) for candidate in candidates)
    assert first.isdisjoint(second)
    assert ledger.graph.common_valid_solid(first) == ledger.graph.common_valid_solid(second)


def test_near_step_and_bottom_relief_roles_exclude_transition_context() -> None:
    grooved = (
        Box(60, 60, 20)
        - Cylinder(5, 20)
        - Pos(0, 0, 7) * Cylinder(9, 6)
        - Pos(0, 0, 7) * Cylinder(10, 2)
    )
    records, candidates, ledger = _claimed(grooved)
    (record,) = records
    assert record.cbore is not None and record.spotface is None
    defining = ledger.defining_of(candidates[0])
    cbore_nodes = [
        node
        for node in defining
        if math.isclose(
            BRepAdaptor_Surface(ledger.graph.face(node).wrapped).Cylinder().Radius() * 2,
            record.cbore.diameter,
        )
    ]
    assert len(cbore_nodes) == 2
    assert all(
        BRepAdaptor_Surface(ledger.graph.face(node).wrapped).GetType() == GeomAbs_Cylinder
        for node in defining
    )

    relief = Box(60, 60, 40) - Pos(0, 0, 12.5) * Cylinder(4.25, 15) - Pos(0, 0, 6) * Cylinder(5, 2)
    records, candidates, ledger = _claimed(relief)
    assert records[0].bottom == "flat"
    assert {
        round(BRepAdaptor_Surface(ledger.graph.face(node).wrapped).Cylinder().Radius() * 2, 2)
        for node in ledger.defining_of(candidates[0])
    } == {8.5, 10.0}


def test_spotface_and_same_diameter_boundaries_are_strict() -> None:
    def segment(diameter, depth):
        return {"diameter": diameter, "s_lo": 0.0, "s_hi": depth, "faces": [object()]}

    below = _near_side_steps([segment(10.0, 1.994)])
    at = _near_side_steps([segment(10.0, 1.995)])
    assert below.spotface is not None and below.cbore is None
    assert at.cbore is not None and at.spotface is None
    boundary = 10.0 / (1.0 - 1e-4)
    assert _same_diameter(10.0, math.nextafter(boundary, 0.0))
    assert not _same_diameter(10.0, math.nextafter(boundary, math.inf))


def test_bore_depth_boundary_excludes_far_step_but_owns_blind_relief() -> None:
    bore_face, far_face, relief_face = object(), object(), object()
    bore = cast(
        SegmentEvidence,
        {
            "diameter": 10.0,
            "s_lo": 0.0,
            "s_hi": 10.0,
            "faces": [bore_face],
        },
    )
    far_step = cast(
        SegmentEvidence,
        {
            "diameter": 18.0,
            "s_lo": -3.0,
            "s_hi": 0.0,
            "faces": [far_face],
        },
    )
    through = _bore_depth([far_step, bore], bore, bottom="through", from_hi=True)
    assert through.depth == 10.0 and through.faces == (bore_face,)

    relief = cast(
        SegmentEvidence,
        {
            "diameter": 12.0,
            "s_lo": -2.0,
            "s_hi": 0.0,
            "faces": [relief_face],
        },
    )
    blind = _bore_depth([relief, bore], bore, bottom="flat", from_hi=True)
    assert blind.depth == 12.0
    assert blind.faces == (bore_face, relief_face)


def _segment(
    diameter: float,
    lo: float,
    hi: float,
    *,
    line=(0.0, 0.0, 0.0),
    direction=(0.0, 0.0, 1.0),
    face=None,
) -> SegmentEvidence:
    return cast(
        SegmentEvidence,
        {
            "diameter": diameter,
            "axis": "z",
            "solid_idx": 0,
            "u_extent": 2 * math.pi,
            "axis_xyz": line,
            "dir_xyz": direction,
            "s_lo": lo,
            "s_hi": hi,
            "face": face or object(),
            "faces": [face or object()],
            "external": False,
        },
    )


def test_cylinder_quantisation_line_projection_and_gap_boundaries() -> None:
    diameter = 2.4691357
    (measured,) = analyse_cylinders(Cylinder(diameter / 2, 5))[0]
    assert measured["diameter"] == quantise(diameter)
    assert _cyl_group_key(dict(measured, diameter=1.2344)) != _cyl_group_key(
        dict(measured, diameter=1.2346)
    )

    base = _segment(10, 0, 1)
    inside_line = dict(base, axis_xyz=(0.00049, 0.0, 9.0))
    outside_line = dict(base, axis_xyz=(0.00051, 0.0, 9.0))
    assert _line_key(base) == _line_key(inside_line)
    assert _line_key(base) != _line_key(outside_line)

    gap = length_tol(10, rel=_STACK_GAP_FRAC)
    touching = _segment(10, 1 + gap, 2)
    separated = _segment(10, math.nextafter(1 + gap, math.inf), 2)
    assert len(_merge_runs([base, touching], _cyl_group_key)) == 1
    assert len(_merge_runs([base, separated], _cyl_group_key)) == 2


def test_opening_tie_break_and_monotonic_step_rejection(monkeypatch) -> None:
    import quiddity._hole_features as module

    lo = _segment(10, 0, 4)
    hi = _segment(10, 4, 10)
    monkeypatch.setattr(module, "_classify_end", lambda *_args, **_kwargs: "open")
    from_hi, opening, coordinate, bottom = _drilled_from([lo, hi], {}, {})
    assert from_hi and opening is hi and coordinate == 10 and bottom == "through"

    near = _segment(18, 6, 10)
    narrower = _segment(14, 4, 6)
    rejected_wider = _segment(16, 2, 4)
    selection = _near_side_steps([near, narrower, rejected_wider])
    assert selection.cbore == CounterBore(18, 4)
    assert selection.spotface == CounterBore(14, 2)
    assert rejected_wider["faces"][0] not in selection.faces


def test_countersink_matcher_tolerances_are_closed() -> None:
    hole = HoleRecord(
        axis=(0.0, 0.0, 1.0),
        location=(0.0, 0.0, 0.0),
        diameter=10.0,
        depth=20.0,
        bottom="through",
    )

    def sink(*, radial=0.0, axial=0.0, diameter=10.0):
        return CounterSink(
            axis=(0.0, 0.0, 1.0),
            location=(radial, 0.0, axial - 2.0),
            major_diameter=20.0,
            drill_diameter=diameter,
            included_angle=90.0,
            depth=2.0,
        )

    axis_tol = length_tol(hole.diameter, rel=0.0333)
    diameter_tol = length_tol(hole.diameter, rel=0.0333)
    mouth_tol = length_tol(hole.diameter, rel=0.0833)
    assert countersink_matches_hole(sink(radial=axis_tol), hole)
    assert not countersink_matches_hole(sink(radial=math.nextafter(axis_tol, math.inf)), hole)
    assert countersink_matches_hole(sink(diameter=hole.diameter + diameter_tol), hole)
    assert not countersink_matches_hole(
        sink(diameter=math.nextafter(hole.diameter + diameter_tol, math.inf)), hole
    )
    assert countersink_matches_hole(sink(axial=mouth_tol), hole)
    assert countersink_matches_hole(sink(axial=hole.depth + mouth_tol), hole)
    blind = replace(hole, bottom="flat")
    assert not countersink_matches_hole(sink(axial=hole.depth), blind)


def _single_internal_end_states(part) -> tuple[str, str]:
    inventory = analyse_cylinders(part)
    cylinders = [item for group in inventory for item in group if not item["external"]]
    segments = _segments(full_cylinders(cylinders))
    assert len(segments) == 1
    segment = segments[0]
    adjacency = edge_face_map(part.faces())
    return (
        _classify_end(segment, segment["s_lo"], False, adjacency),
        _classify_end(segment, segment["s_hi"], True, adjacency),
    )


def test_real_end_partner_plane_cone_torus_sphere_and_cylinder_routes() -> None:
    assert set(_single_internal_end_states(_blind())) == {"flat", "open"}
    assert set(_single_internal_end_states(Box(60, 60, 20) - _drill_tool(5, 12, 10))) == {
        "drill_point",
        "open",
    }
    assert _single_internal_end_states(Sphere(20) - Cylinder(4, 50)) == ("open", "open")

    blind = Box(60, 60, 20) - Pos(0, 0, 4) * Cylinder(5, 12)
    bottom_edge = [
        edge for edge in blind.edges().filter_by(GeomType.CIRCLE) if abs(edge.center().Z + 2) < 0.01
    ]
    assert set(_single_internal_end_states(fillet(bottom_edge, 1.5))) == {"flat", "open"}

    through = _through()
    opening_edge = [
        edge
        for edge in through.edges().filter_by(GeomType.CIRCLE)
        if abs(edge.center().Z - 10) < 0.01
    ]
    assert _single_internal_end_states(fillet(opening_edge, 1.0)) == ("open", "open")

    crossed = (
        Box(60, 60, 40) - Pos(0, 0, 9) * Cylinder(5, 22) - Cylinder(2, 60, rotation=(0, 90, 0))
    )
    assert any(
        record.bottom == "flat" and math.isclose(record.diameter, 10)
        for record in _claimed(crossed)[0]
    )


def test_end_partner_margin_and_narrowest_bore_tie_are_explicit() -> None:
    part = _blind()
    inventory = analyse_cylinders(part)
    cylinders = [item for group in inventory for item in group if not item["external"]]
    (segment,) = _segments(full_cylinders(cylinders))
    adjacency = edge_face_map(part.faces())
    end = segment["s_hi"]
    margin = max(
        length_tol(segment["diameter"], rel=_STACK_GAP_FRAC),
        min(0.45 * (segment["s_hi"] - segment["s_lo"]), 0.5 * segment["diameter"]),
    )
    assert _end_partners(segment, end + margin, adjacency)
    assert not _end_partners(segment, math.nextafter(end + margin, math.inf), adjacency)

    (counterbore,), _, _ = _claimed(_counterbore())
    assert counterbore.diameter == 10 and counterbore.cbore == CounterBore(18, 6)
    split = (
        Box(60, 40, 10)
        - Cylinder(5, 12)
        - Pos(0, 5, 0) * Box(2, 4, 12)
        - Pos(0, -5, 0) * Box(2, 4, 12)
    )
    (tied,), (candidate,), ledger = _claimed(split)
    assert tied.diameter == 10 and len(ledger.defining_of(candidate)) > 1


def test_double_counterbore_excludes_equal_diameter_far_side_land() -> None:
    part = (
        Box(60, 60, 20)
        - Cylinder(5, 20)
        - Pos(0, 0, 7) * Cylinder(9, 6)
        - Pos(0, 0, -7) * Cylinder(9, 6)
    )
    (record,), (candidate,), ledger = _claimed(part)
    assert record.bottom == "through" and record.cbore is not None
    defining = ledger.defining_of(candidate)
    step_nodes = [
        node
        for node in ledger.graph.nodes
        if BRepAdaptor_Surface(ledger.graph.face(node).wrapped).GetType() == GeomAbs_Cylinder
        and math.isclose(
            BRepAdaptor_Surface(ledger.graph.face(node).wrapped).Cylinder().Radius() * 2,
            record.cbore.diameter,
        )
    ]
    assert len(step_nodes) == 2 and sum(node in defining for node in step_nodes) == 1


def test_drill_point_cone_is_context_not_hole_evidence() -> None:
    part = Box(60, 60, 20) - _drill_tool(5, 12, 10)
    (record,), (candidate,), ledger = _claimed(part)
    assert record.bottom == "drill_point"
    assert all(
        BRepAdaptor_Surface(ledger.graph.face(node).wrapped).GetType() == GeomAbs_Cylinder
        for node in ledger.defining_of(candidate)
    )
    constituent = ledger.snapshot_index().constituent_of(candidate)
    terminal = constituent - ledger.defining_of(candidate)
    assert len(terminal) == 1
    assert (
        BRepAdaptor_Surface(ledger.graph.face(next(iter(terminal))).wrapped).GetType()
        == GeomAbs_Cone
    )


@pytest.mark.parametrize(
    ("part", "end"),
    [(_countersunk(), "near"), (Rot(180, 0, 0) * _countersunk(), "far")],
)
def test_nested_countersink_stays_predecessor_owned_and_hole_consulted(part, end) -> None:
    product = _take_inventory(part)
    holes = product.physical.candidate_set(FamilyId.HOLES).candidates
    countersinks = product.physical.candidate_set(FamilyId.COUNTERSINKS).candidates
    assert len(holes) == len(countersinks) == 1
    hole, countersink = holes[0], countersinks[0]
    assert isinstance(hole.record, HoleRecord)
    assert isinstance(countersink.record, CounterSink)
    (expected_cone,) = _fresh_expected_countersinks(part, product.context.graph)
    assert countersink.record == expected_cone.record
    assert product.evidence.defining_of(countersink) == frozenset((expected_cone.node,))
    assert product.context.graph.common_valid_solid((expected_cone.node,)) == expected_cone.solid
    assert expected_cone.node not in product.evidence.constituent_of(hole)

    (solid,) = part.solids()
    (bore,) = _raw_internal_cylinders(product.context.graph, solid)
    assert product.evidence.defining_of(hole) == frozenset((bore.node,))
    assert product.context.graph.common_valid_solid((bore.node,)) == expected_cone.solid
    expected_hole = HoleRecord(
        axis=(-bore.direction[0], -bore.direction[1], -bore.direction[2]),
        location=_point_on_line(bore.line, bore.direction, bore.hi),
        diameter=bore.diameter,
        depth=round(bore.hi - bore.lo, 2),
        bottom="through",
        csink=None,
    )
    assert _record_matches(expected_hole, replace(hole.record, csink=None))
    inner = tuple(
        expected_cone.record.location[index]
        + expected_cone.record.axis[index] * expected_cone.record.depth
        for index in range(3)
    )
    projected = sum(
        (inner[index] - expected_hole.location[index]) * expected_hole.axis[index]
        for index in range(3)
    )
    if end == "near":
        assert math.isclose(projected, 0, abs_tol=0.01)
    else:
        assert math.isclose(projected, expected_hole.depth, abs_tol=0.01)
    assert hole.record.csink is countersink.record
    hole_nodes = product.evidence.defining_of(hole)
    cone_nodes = product.evidence.defining_of(countersink)
    assert hole_nodes and cone_nodes and hole_nodes.isdisjoint(cone_nodes)
    assert all(
        BRepAdaptor_Surface(product.context.graph.face(node).wrapped).GetType() == GeomAbs_Cylinder
        for node in hole_nodes
    )
    assert all(
        BRepAdaptor_Surface(product.context.graph.face(node).wrapped).GetType() == GeomAbs_Cone
        for node in cone_nodes
    )


def test_two_sided_countersink_keeps_one_hole_predecessor_and_one_unmatched_seat() -> None:
    part = (
        Box(50, 50, 12)
        - Cylinder(3, 12)
        - Pos(0, 0, 4) * Cone(3, 7, 4)
        - Pos(0, 0, -4) * Cone(7, 3, 4)
    )

    product = _take_inventory(part)
    holes = product.physical.candidate_set(FamilyId.HOLES).candidates
    countersinks = product.physical.candidate_set(FamilyId.COUNTERSINKS).candidates

    assert len(holes) == 1
    assert len(countersinks) == 2
    assert isinstance(holes[0].record, HoleRecord)
    assert holes[0].record.csink is countersinks[0].record
    assert product.evidence.defining_of(holes[0])
    assert all(product.evidence.defining_of(candidate) for candidate in countersinks)


def _completed_countersinks(part):
    ledger = ClaimLedger(FaceGraph(part), definitions=PHYSICAL_DEFINITIONS)
    records = _discover_countersinks(part, writer=ledger.writer)
    ledger.candidate_set_for(FamilyId.COUNTERSINKS, records)
    holes = next(item for item in PHYSICAL_DEFINITIONS if item.family is FamilyId.HOLES)
    occurrences = ledger.restricted_inputs(holes).occurrences(
        FamilyId.COUNTERSINKS, type(records[0])
    )
    return ledger, records, occurrences


def test_matched_countersink_requires_exact_completed_predecessor() -> None:
    part = _countersunk()
    records = recognise_countersinks(part)
    ledger = ClaimLedger(FaceGraph(part))
    with pytest.raises(ValueError, match="predecessor identity"):
        _discover_holes(part, csinks=records, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.HOLES).candidates == ()

    # An injected sibling record which does not match this Hole remains irrelevant context.
    unrelated = recognise_countersinks(Pos(100, 0, 0) * _countersunk())
    hole_records = _discover_holes(part, csinks=unrelated, writer=ledger.writer)
    assert hole_records and hole_records[0].csink is None
    assert ledger.candidate_set(FamilyId.HOLES).candidates


def test_completed_countersink_from_another_run_refuses_body_link_atomically() -> None:
    part = _countersunk()
    foreign, records, occurrences = _completed_countersinks(copy.deepcopy(part))
    assert foreign is not None
    ledger = ClaimLedger(FaceGraph(part))
    with pytest.raises(ValueError, match="different solids"):
        _discover_holes(
            part,
            csinks=records,
            predecessor_occurrences=occurrences,
            writer=ledger.writer,
        )
    assert ledger.candidate_set(FamilyId.HOLES).candidates == ()


def test_ambiguous_or_empty_countersink_predecessor_refuses_atomically() -> None:
    part = _countersunk()
    ledger, records, occurrences = _completed_countersinks(part)
    with pytest.raises(ValueError, match="ambiguous matching"):
        _discover_holes(
            part,
            csinks=[records[0], records[0]],
            predecessor_occurrences=occurrences,
            writer=ledger.writer,
        )
    assert ledger.candidate_set(FamilyId.HOLES).candidates == ()

    empty = ClaimLedger(FaceGraph(part), definitions=PHYSICAL_DEFINITIONS)
    public_record = recognise_countersinks(part)[0]
    empty.candidate_set_for(FamilyId.COUNTERSINKS, (public_record,))
    holes_definition = next(item for item in PHYSICAL_DEFINITIONS if item.family is FamilyId.HOLES)
    empty_occurrences = empty.restricted_inputs(holes_definition).occurrences(
        FamilyId.COUNTERSINKS, type(public_record)
    )
    with pytest.raises(ValueError, match="different solids"):
        _discover_holes(
            part,
            csinks=(public_record,),
            predecessor_occurrences=empty_occurrences,
            writer=empty.writer,
        )
    assert empty.candidate_set(FamilyId.HOLES).candidates == ()


def test_cross_solid_or_reused_countersink_predecessor_refuses_prefix_free(monkeypatch) -> None:
    import quiddity._hole_features as module

    left, right = Pos(-50, 0, 0) * _countersunk(), Pos(50, 0, 0) * _countersunk()
    part = Compound([left, right])
    ledger, countersinks, occurrences = _completed_countersinks(part)
    first = min(countersinks, key=lambda item: item.location[0])

    monkeypatch.setattr(
        module,
        "countersink_matches_hole",
        lambda csink, hole: csink is first and hole.location[0] > 0,
    )
    with pytest.raises(ValueError, match="different solids"):
        _discover_holes(
            part,
            csinks=(first,),
            predecessor_occurrences=occurrences,
            writer=ledger.writer,
        )
    assert ledger.candidate_set(FamilyId.HOLES).candidates == ()

    monkeypatch.setattr(module, "countersink_matches_hole", lambda _csink, _hole: True)
    with pytest.raises(ValueError, match="shared by Hole"):
        _discover_holes(
            part,
            csinks=(first,),
            predecessor_occurrences=occurrences,
            writer=ledger.writer,
        )
    assert ledger.candidate_set(FamilyId.HOLES).candidates == ()


def test_duplicate_hole_face_ownership_refuses_without_prefix(monkeypatch) -> None:
    import quiddity._hole_features as module

    part = _through()
    ledger = ClaimLedger(FaceGraph(part))
    original = module._merge_stacks

    def duplicate(stacks, edge_faces, cache=None, face_surfaces=None):
        merged = original(stacks, edge_faces, cache, face_surfaces)
        return [*merged, merged[0]]

    monkeypatch.setattr(module, "_merge_stacks", duplicate)
    with pytest.raises(ValueError, match="share defining"):
        _discover_holes(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.HOLES).candidates == ()


def test_equal_holes_keep_occurrence_and_body_identity() -> None:
    original = _through()
    part = Compound([Pos(-80, 0, 0) * original, Pos(80, 0, 0) * copy.deepcopy(original)])
    records, candidates, ledger = _claimed(part)
    assert len(records) == 2
    assert records[0].diameter == records[1].diameter
    first, second = (ledger.defining_of(candidate) for candidate in candidates)
    assert first.isdisjoint(second)
    assert ledger.graph.common_valid_solid(first) != ledger.graph.common_valid_solid(second)


def test_same_body_equal_bores_keep_distinct_occurrence_roles() -> None:
    part = Box(70, 40, 20) - Pos(-18, 0, 0) * Cylinder(5, 20) - Pos(18, 0, 0) * Cylinder(5, 20)
    records, candidates, ledger = _claimed(part)
    assert len(records) == 2
    assert records[0].diameter == records[1].diameter
    assert records[0].depth == records[1].depth
    defining = [ledger.defining_of(candidate) for candidate in candidates]
    assert defining[0].isdisjoint(defining[1])
    assert ledger.graph.common_valid_solid(defining[0]) == ledger.graph.common_valid_solid(
        defining[1]
    )


def test_coincident_equal_full_records_keep_distinct_occurrence_bodies() -> None:
    original = _through()
    part = Compound([original, copy.deepcopy(original)])
    records, candidates, ledger = _claimed(part)
    assert len(records) == 2 and records[0] == records[1] and records[0] is not records[1]
    defining = [ledger.defining_of(candidate) for candidate in candidates]
    assert defining[0].isdisjoint(defining[1])
    assert ledger.graph.common_valid_solid(defining[0]) != ledger.graph.common_valid_solid(
        defining[1]
    )
    assert all(
        candidate.record is record for candidate, record in zip(candidates, records, strict=True)
    )


@pytest.mark.parametrize("transform", [Rot(0, 90, 0), Rot(31, 17, 43)])
def test_cross_and_nonprincipal_axes_keep_exact_roles(transform) -> None:
    _claimed(transform * _through())


def test_mirror_preserves_counterbore_occurrence_and_roles() -> None:
    _claimed(_counterbore().mirror(Plane.YZ))


def test_scale_traversal_step_and_supplied_dependencies_preserve_roles(
    monkeypatch, tmp_path: Path
) -> None:
    for part in (_through().scale(0.2), _through().scale(5)):
        _claimed(part)

    part = _counterbore()
    solid_type = type(part)
    original_faces = solid_type.faces

    def reversed_faces(self):
        faces = original_faces(self)
        return type(faces)(reversed(faces))

    monkeypatch.setattr(solid_type, "faces", reversed_faces)
    _claimed(part)
    monkeypatch.undo()

    target = tmp_path / "hole.step"
    assert export_step(_counterbore(), target)
    _claimed(import_step(target))

    supplied_part = _spotface_stack()
    cylinders = analyse_cylinders(supplied_part)
    from quiddity._adjacency import FaceEdges

    _claimed(supplied_part, cyls=cylinders, face_edges=FaceEdges())


def test_shoulder_transitions_and_opening_chamfer_remain_context_only() -> None:
    base = _counterbore()
    shoulder = [
        edge
        for edge in base.edges().filter_by(GeomType.CIRCLE)
        if abs(edge.center().Z - 4) < 0.01 and abs(edge.radius - 5) < 0.01
    ]
    for part in (chamfer(shoulder, 1.0), fillet(shoulder, 1.0)):
        records, candidates, ledger = _claimed(part)
        assert records[0].cbore is not None
        assert all(
            BRepAdaptor_Surface(ledger.graph.face(node).wrapped).GetType() == GeomAbs_Cylinder
            for node in ledger.defining_of(candidates[0])
        )

    opened = _through()
    lip = max(opened.edges().filter_by(GeomType.CIRCLE), key=lambda edge: edge.center().Z)
    records, candidates, ledger = _claimed(chamfer(lip, 1.0))
    assert records[0].bottom == "through"
    assert all(
        BRepAdaptor_Surface(ledger.graph.face(node).wrapped).GetType() == GeomAbs_Cylinder
        for node in ledger.defining_of(candidates[0])
    )


def test_radial_turned_and_separate_coaxial_bodies_keep_body_roles() -> None:
    _claimed(Cylinder(15, 60, rotation=(0, 90, 0)) - Cylinder(3, 40))
    _claimed(Cylinder(30, 40) - Cylinder(10, 40))

    plate = Pos(-1, 0, 0) * (Box(2, 40, 12) - Cylinder(4.9, 40, rotation=(0, 90, 0)))
    block = Pos(60.5, 0, 0) * (
        Box(119, 40, 12) - Pos(-59.5, 0, 0) * Cylinder(4.9, 24, rotation=(0, 90, 0))
    )
    records, candidates, ledger = _claimed(Compound([plate, block]))
    assert len(records) == len(candidates) == 2
    assert ledger.defining_of(candidates[0]).isdisjoint(ledger.defining_of(candidates[1]))


def test_rounded_slot_and_unrelated_wider_groove_issue_no_surplus_roles() -> None:
    profile = Rectangle(8, 30) + Circle(4)
    slot = extrude(profile, 10)
    part = Box(60, 60, 20) - slot
    assert recognise_holes(part) == []
    ledger = ClaimLedger(FaceGraph(part))
    assert _discover_holes(part, writer=ledger.writer) == []
    assert ledger.candidate_set(FamilyId.HOLES).candidates == ()

    partial_cylinder = Cylinder(5, 20) & Pos(2.5, 0, 0) * Box(5, 20, 20)
    partial = Box(60, 60, 20) - partial_cylinder
    assert recognise_holes(partial) == []
    ledger = ClaimLedger(FaceGraph(partial))
    assert _discover_holes(partial, writer=ledger.writer) == []
    assert ledger.candidate_set(FamilyId.HOLES).candidates == ()

    grooved = Box(60, 60, 20) - Cylinder(5, 20) - Cylinder(6, 3)
    (record,), (candidate,), ledger = _claimed(grooved)
    assert record.diameter == 10.0
    assert {
        round(BRepAdaptor_Surface(ledger.graph.face(node).wrapped).Cylinder().Radius() * 2, 2)
        for node in ledger.defining_of(candidate)
    } == {10.0}


@pytest.mark.parametrize("stale", [False, True])
def test_deep_or_translated_cylindrical_snapshot_refuses_atomically(monkeypatch, stale) -> None:
    import quiddity._hole_features as module

    part = _through()
    ledger = ClaimLedger(FaceGraph(part))
    original = module._bore_depth

    def changed(*args, **kwargs):
        selection = original(*args, **kwargs)
        face = copy.deepcopy(selection.faces[0])
        if stale:
            face = Pos(1, 0, 0) * face
        return replace(selection, faces=(face, *selection.faces[1:]))

    monkeypatch.setattr(module, "_bore_depth", changed)
    with pytest.raises(ValueError):
        _discover_holes(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.HOLES).candidates == ()


def test_missing_or_aliased_hole_source_roles_refuse_atomically(monkeypatch) -> None:
    import quiddity._hole_features as module

    part = _blind()
    ledger = ClaimLedger(FaceGraph(part))
    original_depth = module._bore_depth

    def without_cylinder(*args, **kwargs):
        return replace(original_depth(*args, **kwargs), faces=())

    monkeypatch.setattr(module, "_bore_depth", without_cylinder)
    with pytest.raises(ValueError, match="cylindrical evidence"):
        _discover_holes(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.HOLES).candidates == ()

    monkeypatch.setattr(module, "_bore_depth", original_depth)
    original_drilled_from = module._drilled_from

    def alias_terminal(stack, *args, bottom_faces=None, **kwargs):
        result = original_drilled_from(
            stack,
            *args,
            bottom_faces=bottom_faces,
            **kwargs,
        )
        assert bottom_faces is not None
        bottom_faces.append(stack[0]["faces"][0])
        return result

    monkeypatch.setattr(module, "_drilled_from", alias_terminal)
    with pytest.raises(ValueError, match="terminal identity aliases"):
        _discover_holes(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.HOLES).candidates == ()


def test_later_common_solid_failure_is_prefix_free(monkeypatch) -> None:
    part = Compound([Pos(-50, 0, 0) * _through(), Pos(50, 0, 0) * _through()])
    ledger = ClaimLedger(FaceGraph(part))
    original = ledger.graph.common_valid_solid
    calls = 0

    def fail_second(nodes):
        nonlocal calls
        calls += 1
        return None if calls == 2 else original(nodes)

    monkeypatch.setattr(ledger.graph, "common_valid_solid", fail_second)
    with pytest.raises(ValueError, match="one valid solid"):
        _discover_holes(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.HOLES).candidates == ()


def test_open_shell_public_compatibility_refuses_aggregate_without_prefix() -> None:
    shell = Shell(_through().faces())
    assert recognise_holes(shell)
    ledger = ClaimLedger(FaceGraph(shell))
    with pytest.raises(ValueError, match="one valid solid"):
        _discover_holes(shell, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.HOLES).candidates == ()


def test_foreign_writer_and_late_binding_fail_atomically(monkeypatch) -> None:
    foreign = ClaimLedger(FaceGraph(Pos(100, 0, 0) * _through()))
    with pytest.raises(ValueError):
        _discover_holes(_through(), writer=foreign.writer)
    assert foreign.candidate_set(FamilyId.HOLES).candidates == ()

    part = Compound([Pos(-50, 0, 0) * _through(), Pos(50, 0, 0) * _through()])
    ledger = ClaimLedger(FaceGraph(part))
    original = ledger.graph.require_node
    calls = 0

    def fail_late(face):
        nonlocal calls
        calls += 1
        if calls > 1:
            raise ValueError("late Hole binding")
        return original(face)

    monkeypatch.setattr(ledger.graph, "require_node", fail_late)
    with pytest.raises(ValueError, match="late Hole binding"):
        _discover_holes(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.HOLES).candidates == ()


def test_box_boss_and_slot_issue_no_hole_evidence() -> None:
    for part in (
        Box(20, 20, 20),
        Box(40, 40, 10) + Pos(0, 0, 10) * Cylinder(5, 10),
    ):
        assert recognise_holes(part) == []
        ledger = ClaimLedger(FaceGraph(part))
        assert _discover_holes(part, writer=ledger.writer) == []
        assert ledger.candidate_set(FamilyId.HOLES).candidates == ()


def _qualified_calls(tree: ast.AST) -> list[tuple[str, ast.Call]]:
    aliases: dict[str, str] = {}
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                aliases[alias.asname or alias.name.split(".")[0]] = alias.name
        elif isinstance(node, ast.ImportFrom) and node.module:
            for alias in node.names:
                aliases[alias.asname or alias.name] = f"{node.module}.{alias.name}"

    def qualified(node: ast.expr) -> str:
        if isinstance(node, ast.Name):
            return aliases.get(node.id, node.id)
        if isinstance(node, ast.Attribute):
            return f"{qualified(node.value)}.{node.attr}"
        return ""

    return [(qualified(node.func), node) for node in ast.walk(tree) if isinstance(node, ast.Call)]


def test_private_hole_core_has_one_writer_caller_and_declared_predecessor() -> None:
    sites: list[tuple[str, ast.Call]] = []
    for path in (ROOT / "src/quiddity").glob("*.py"):
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        sites.extend(
            (path.name, call)
            for qualified, call in _qualified_calls(tree)
            if qualified.endswith("._discover_holes") or qualified == "_discover_holes"
        )
    assert {name for name, _call in sites} == {"_hole_features.py", "_registry.py"}
    registry = next(call for name, call in sites if name == "_registry.py")
    keywords = {keyword.arg: keyword.value for keyword in registry.keywords}
    writer = keywords["writer"]
    assert (
        isinstance(writer, ast.Attribute)
        and writer.attr == "writer"
        and isinstance(writer.value, ast.Name)
        and writer.value.id == "services"
    )
    predecessor = keywords["predecessor_occurrences"]
    assert isinstance(predecessor, ast.Name) and predecessor.id == "occurrences"

    source = ast.parse((ROOT / "src/quiddity/_hole_features.py").read_text(encoding="utf-8"))
    functions = {node.name: node for node in source.body if isinstance(node, ast.FunctionDef)}
    public_calls = [
        call
        for qualified, call in _qualified_calls(functions["recognise_holes"])
        if qualified == "_discover_holes"
    ]
    assert len(public_calls) == 1
    assert {keyword.arg for keyword in public_calls[0].keywords} == {
        "cyls",
        "csinks",
        "face_edges",
    }

    registry_tree = ast.parse((ROOT / "src/quiddity/_registry.py").read_text(encoding="utf-8"))
    registry_functions = {
        node.name: node for node in registry_tree.body if isinstance(node, ast.FunctionDef)
    }
    holes_body = registry_functions["_holes"]
    occurrence_calls = [
        call
        for qualified, call in _qualified_calls(holes_body)
        if qualified.endswith(".occurrences")
    ]
    assert len(occurrence_calls) == 1
    call = occurrence_calls[0]
    assert isinstance(call.args[0], ast.Attribute) and call.args[0].attr == "COUNTERSINKS"
    assert isinstance(call.args[1], ast.Name) and call.args[1].id == "CounterSink"

    forbidden = {
        "EvidenceIndex",
        "CandidateInventory",
        "CandidateReconciliation",
        "ReconciliationResult",
        "reconcile_recess_candidates",
    }
    imported = {
        alias.name
        for node in source.body
        if isinstance(node, ast.ImportFrom)
        for alias in node.names
    }
    assert imported.isdisjoint(forbidden)


def test_hole_record_and_step_constructor_roster_is_closed() -> None:
    sites: list[tuple[str, str]] = []
    for path in (ROOT / "src/quiddity").glob("*.py"):
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        for qualified, _call in _qualified_calls(tree):
            if qualified.endswith(".HoleRecord") or qualified == "HoleRecord":
                sites.append((path.name, "HoleRecord"))
            if qualified.endswith(".CounterBore") or qualified == "CounterBore":
                sites.append((path.name, "CounterBore"))
    assert sites == [
        ("_hole_features.py", "CounterBore"),
        ("_hole_features.py", "HoleRecord"),
    ]

    from quiddity._effective_surfaces import SURFACE_READER_SITES

    assert "_hole_features:_classify_end_uncached:adaptor:1" in SURFACE_READER_SITES
    assert "_hole_features:_classify_end_uncached:adaptor:2" in SURFACE_READER_SITES
    assert not any("_discover_holes" in key for key in SURFACE_READER_SITES)


def test_terminal_inventory_retains_complete_hole_identity() -> None:
    product = _take_inventory(_counterbore())
    candidates = product.physical.candidate_set(FamilyId.HOLES).candidates
    assert len(candidates) == len(product.result.holes) == 1
    assert candidates[0].record is product.result.holes[0]
    assert product.evidence.defining_of(candidates[0])
