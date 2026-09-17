"""F5e: every CounterSink occurrence owns only its original conical seat face."""

from __future__ import annotations

import ast
import copy
import math
from pathlib import Path

import pytest
from build123d import (
    Align,
    Box,
    Compound,
    Cone,
    Cylinder,
    GeomType,
    Pos,
    Rot,
    Shell,
    export_step,
    import_step,
)
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.GeomAbs import GeomAbs_Cone, GeomAbs_Cylinder

from quiddity import recognise_countersinks
from quiddity._adjacency import FaceGraph
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity.countersinks import _discover_countersinks
from quiddity.result import _take_inventory
from tests.route_pins import assert_core_route_is_closed


def _qualified_calls(tree: ast.AST) -> list[tuple[str, ast.Call]]:
    aliases: dict[str, str] = {}
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                local = alias.asname or alias.name.split(".")[0]
                aliases[local] = alias.name if alias.asname else alias.name.split(".")[0]
        elif isinstance(node, ast.ImportFrom) and node.module:
            for alias in node.names:
                aliases[alias.asname or alias.name] = f"{node.module}.{alias.name}"

    def name(node: ast.expr) -> str:
        if isinstance(node, ast.Name):
            return aliases.get(node.id, node.id)
        if isinstance(node, ast.Attribute):
            base = name(node.value)
            return f"{base}.{node.attr}" if base else node.attr
        return ""

    return [(name(node.func), node) for node in ast.walk(tree) if isinstance(node, ast.Call)]


def _plate(points=((0, 0),)):
    part = Box(90, 60, 12)
    for x, y in points:
        part -= Pos(x, y, 0) * Cylinder(3, 12)
        part -= Pos(x, y, 4) * Cone(3, 7, 4)
    return part


def _angle_plate(included_angle: float):
    depth = 4.0
    minor = 3.0
    major = minor + depth * math.tan(math.radians(included_angle / 2))
    return Box(100, 100, 16) - Cylinder(minor, 16) - Pos(0, 0, depth) * Cone(minor, major, depth)


def _assert_role(ledger, candidate, record) -> None:
    defining = ledger.defining_of(candidate)
    assert len(defining) == 1
    assert ledger.graph.common_valid_solid(defining) is not None
    (node,) = defining
    face = ledger.graph.face(node)
    surface = BRepAdaptor_Surface(face.wrapped)
    assert surface.GetType() == GeomAbs_Cone
    circles = sorted(face.edges().filter_by(GeomType.CIRCLE), key=lambda edge: edge.radius)
    assert len(circles) >= 2
    minor, major = circles[0], circles[-1]
    opening = major.arc_center
    inner = minor.arc_center
    delta = (inner.X - opening.X, inner.Y - opening.Y, inner.Z - opening.Z)
    depth = math.sqrt(sum(value * value for value in delta))
    axis = tuple(round(value / depth, 4) for value in delta)
    assert record.axis == axis
    assert record.location == tuple(round(value, 4) for value in (opening.X, opening.Y, opening.Z))
    assert record.major_diameter == round(2 * major.radius, 4)
    assert record.drill_diameter == round(2 * minor.radius, 4)
    assert record.depth == round(depth, 4)
    assert record.included_angle == round(2 * abs(math.degrees(surface.Cone().SemiAngle())), 2)
    matching_cylinders = []
    for other in ledger.graph.nodes:
        other_surface = BRepAdaptor_Surface(ledger.graph.face(other).wrapped)
        if other_surface.GetType() != GeomAbs_Cylinder:
            continue
        cylinder = other_surface.Cylinder()
        axis_line = cylinder.Axis()
        direction = axis_line.Direction()
        line_direction = (direction.X(), direction.Y(), direction.Z())
        if abs(sum(a * b for a, b in zip(record.axis, line_direction, strict=True))) <= 0.999:
            continue
        line_point = axis_line.Location()
        offset = tuple(
            record.location[index] - (line_point.X(), line_point.Y(), line_point.Z())[index]
            for index in range(3)
        )
        along = sum(offset[index] * line_direction[index] for index in range(3))
        distance = math.sqrt(
            sum((offset[index] - along * line_direction[index]) ** 2 for index in range(3))
        )
        radius_tolerance = 0.0167 * (record.drill_diameter / 2)
        line_tolerance = 0.0167 * record.drill_diameter
        if (
            abs(2 * cylinder.Radius() - record.drill_diameter) <= 2 * radius_tolerance + 1e-9
            and distance <= line_tolerance + 1e-9
        ):
            matching_cylinders.append(other)
    assert matching_cylinders
    assert all(context not in defining for context in matching_cylinders)


def _claimed(part):
    plain = recognise_countersinks(part)
    ledger = ClaimLedger(FaceGraph(part))
    measured = _discover_countersinks(part, writer=ledger.writer)
    assert measured == plain
    assert [record.to_dict() for record in measured] == [record.to_dict() for record in plain]
    candidates = ledger.candidate_set(FamilyId.COUNTERSINKS).candidates
    assert len(candidates) == len(measured)
    for candidate, record in zip(candidates, measured, strict=True):
        assert candidate.record is record
        _assert_role(ledger, candidate, record)
    return ledger, measured


def _boundary_claimed(part, accepted: bool) -> None:
    if accepted:
        _ledger, records = _claimed(part)
        assert records
        return
    assert recognise_countersinks(part) == []
    ledger = ClaimLedger(FaceGraph(part))
    assert _discover_countersinks(part, writer=ledger.writer) == []
    assert ledger.candidate_set(FamilyId.COUNTERSINKS).candidates == ()


@pytest.mark.parametrize(
    "transform",
    [None, Rot(90, 0, 0), Rot(0, 90, 0), Rot(31, 17, 43), Rot(0, 0, 180)],
)
def test_countersink_owner_is_rotation_invariant(transform) -> None:
    part = _plate()
    if transform is not None:
        part = transform * part
    _ledger, records = _claimed(part)
    assert len(records) == 1


def test_multiple_countersinks_keep_sorted_occurrence_identity() -> None:
    ledger, records = _claimed(_plate(((-30, -15), (5, 12), (30, -8))))
    assert len(records) == 3
    assert records == sorted(records, key=lambda record: (record.location, record.major_diameter))
    assert (
        len(
            {
                next(iter(ledger.defining_of(candidate)))
                for candidate in ledger.candidate_set(FamilyId.COUNTERSINKS).candidates
            }
        )
        == 3
    )


@pytest.mark.parametrize("bore_depth", [8.0, 12.0])
def test_blind_and_through_bores_keep_conical_owner(bore_depth: float) -> None:
    part = Box(40, 40, 12) - Cylinder(3, bore_depth) - Pos(0, 0, 4) * Cone(3, 7, 4)
    _ledger, records = _claimed(part)
    assert len(records) == 1


def test_opposite_mouth_orientation_keeps_conical_owner() -> None:
    part = Box(40, 40, 12) - Cylinder(3, 12) - Pos(0, 0, -4) * Rot(180, 0, 0) * Cone(3, 7, 4)
    _ledger, records = _claimed(part)
    assert len(records) == 1
    assert records[0].axis[2] > 0


def test_both_face_through_hole_issues_two_distinct_cone_owners() -> None:
    part = (
        Box(40, 40, 12)
        - Cylinder(3, 12)
        - Pos(0, 0, 4) * Cone(3, 7, 4)
        - Pos(0, 0, -4) * Rot(180, 0, 0) * Cone(3, 7, 4)
    )
    ledger, records = _claimed(part)
    assert len(records) == 2
    candidates = ledger.candidate_set(FamilyId.COUNTERSINKS).candidates
    assert len({next(iter(ledger.defining_of(candidate))) for candidate in candidates}) == 2
    assert {record.axis[2] for record in records} == {-1.0, 1.0}


def test_unequal_countersinks_keep_deterministic_occurrence_order() -> None:
    part = Box(80, 40, 12)
    part -= Pos(-20, 0, 0) * Cylinder(3, 12)
    part -= Pos(-20, 0, 4) * Cone(3, 7, 4)
    part -= Pos(20, 0, 0) * Cylinder(2, 12)
    part -= Pos(20, 0, 3) * Cone(2, 5, 3)
    _ledger, records = _claimed(part)
    assert len(records) == 2
    assert records == sorted(records, key=lambda record: (record.location, record.major_diameter))
    assert {record.major_diameter for record in records} == {10.0, 14.0}


def _external_cone_and_cylinder(
    *, major: float = 6.0, bore: float = 3.0, offset: float = 0.0, tilt: float = 0.0
):
    """The documented external/centre-drill false-positive geometry, in separate solids."""

    cone = Cone(3, major, max(major - 3, 1.0))
    cylinder = Pos(offset, 0, 0) * Rot(tilt, 0, 0) * Cylinder(bore, 4)
    return Compound([cone, cylinder])


def _internal_cone_and_cylinder(
    *, major: float = 6.0, bore: float = 3.0, offset: float = 0.0, tilt: float = 0.0
):
    """An inward-facing cone plus an independently adjustable cylindrical substrate."""

    depth = max(major - 3, 1.0)
    cone_owner = Box(30, 30, 10) - Cone(3, major, depth)
    cylinder = Pos(offset, 0, 0) * Rot(tilt, 0, 0) * Cylinder(bore, 4)
    return Compound([cone_owner, cylinder])


@pytest.mark.parametrize("angle", [60.0, 82.0, 90.0, 100.0, 120.0, 160.0])
def test_standard_and_inclusive_maximum_angles_keep_exact_owner(angle: float) -> None:
    _ledger, records = _claimed(_angle_plate(angle))
    assert len(records) == 1
    assert records[0].included_angle == pytest.approx(angle, abs=0.02)


@pytest.mark.parametrize(("ratio", "accepted"), [(1.499, False), (1.5, True), (1.501, True)])
def test_flare_ratio_boundary_is_inclusive(ratio: float, accepted: bool) -> None:
    _boundary_claimed(_internal_cone_and_cylinder(major=3 * ratio), accepted)


@pytest.mark.parametrize(("delta", "accepted"), [(0.049, True), (0.0501, True), (0.051, False)])
def test_minor_radius_match_boundary_is_inclusive(delta: float, accepted: bool) -> None:
    _boundary_claimed(_internal_cone_and_cylinder(bore=3 + delta), accepted)


@pytest.mark.parametrize(("offset", "accepted"), [(0.099, True), (0.1002, True), (0.101, False)])
def test_axis_line_offset_boundary_is_inclusive(offset: float, accepted: bool) -> None:
    _boundary_claimed(_internal_cone_and_cylinder(offset=offset), accepted)


@pytest.mark.parametrize(("tilt", "accepted"), [(2.5, True), (2.56, True), (2.57, False)])
def test_parallel_angular_boundary_is_strict(tilt: float, accepted: bool) -> None:
    _boundary_claimed(_internal_cone_and_cylinder(tilt=tilt), accepted)


def test_external_cone_cannot_borrow_a_cylinder_from_another_solid() -> None:
    _boundary_claimed(_external_cone_and_cylinder(), False)


@pytest.mark.parametrize(
    "transform",
    [None, Rot(90, 0, 0), Rot(0, 90, 0), Rot(31, 17, 43), Rot(0, 0, 180)],
)
def test_connected_external_stepped_shaft_is_rejected_covariantly(transform) -> None:
    xyz_min = (Align.CENTER, Align.CENTER, Align.MIN)
    shaft = (
        Cylinder(3, 5, align=xyz_min)
        + Pos(0, 0, 5) * Cone(3, 7, 4, align=xyz_min)
        + Pos(0, 0, 9) * Cylinder(7, 5, align=xyz_min)
    )
    if transform is not None:
        shaft = transform * shaft
    _boundary_claimed(shaft, False)
    assert _take_inventory(shaft).result.countersinks == ()


def test_centre_drill_geometry_false_positive_remains_attributed() -> None:
    depth = 4.0
    major = 2 + depth * math.tan(math.radians(30))
    centre_drilled = (
        Cylinder(20, 30) - Pos(0, 0, 10) * Cylinder(2, 10) - Pos(0, 0, 13) * Cone(2, major, depth)
    )
    assert centre_drilled.bounding_box().max.Z == 15.0
    _ledger, records = _claimed(centre_drilled)
    assert len(records) == 1
    assert records[0].included_angle == 60.0
    assert records[0].location[2] == 15.0


def test_translation_and_uniform_scale_keep_owner_correspondence() -> None:
    original = _plate()
    for part in (Pos(17, -9, 4) * original, original.scale(3), original.mirror()):
        _ledger, records = _claimed(part)
        assert len(records) == 1


@pytest.mark.parametrize(
    "part",
    [
        Cylinder(3, 12),
        Box(40, 40, 12) - Cylinder(3, 12),
        Box(40, 40, 12) - Cylinder(3, 12) - Pos(0, 0, 2) * Cone(3, 4, 2),
        _angle_plate(165.0),
    ],
)
def test_rejected_shapes_issue_no_countersink_candidate(part) -> None:
    ledger = ClaimLedger(FaceGraph(part))
    assert _discover_countersinks(part, writer=ledger.writer) == []
    assert ledger.candidate_set(FamilyId.COUNTERSINKS).candidates == ()


@pytest.mark.parametrize(
    "part",
    [
        Compound([Cone(0, 6, 3), Cylinder(3, 4)]),
        Cone(3, 6, 3),
        Compound([Cone(3, 8, 10) & Rot(20, 0, 0) * Box(30, 30, 2), Cylinder(3, 4)]),
        Box(40, 40, 12) - Cylinder(5, 4) - Cylinder(3, 12),
    ],
)
def test_distinct_rim_and_context_refusals_issue_no_candidate(part) -> None:
    _boundary_claimed(part, False)


def test_side_clipped_external_cone_is_rejected() -> None:
    clipped = Cone(3, 6, 3) - Pos(3, 0, 0) * Box(4, 20, 20)
    _boundary_claimed(Compound([clipped, Cylinder(3, 4)]), False)


def test_geometry_only_open_shell_result_has_no_aggregate_publication() -> None:
    shell = Shell(_plate().faces())
    assert recognise_countersinks(shell)
    ledger = ClaimLedger(FaceGraph(shell))
    with pytest.raises(ValueError, match="no unambiguous valid solid"):
        _discover_countersinks(shell, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.COUNTERSINKS).candidates == ()


def test_step_round_trip_preserves_conical_owner_role(tmp_path: Path) -> None:
    path = tmp_path / "countersink.step"
    source_ledger, source_records = _claimed(_plate())
    export_step(_plate(), path)
    imported = import_step(path)
    imported_ledger, records = _claimed(imported)
    assert len(records) == 1
    source_candidate = source_ledger.candidate_set(FamilyId.COUNTERSINKS).candidates[0]
    imported_candidate = imported_ledger.candidate_set(FamilyId.COUNTERSINKS).candidates[0]
    assert source_records[0].to_dict() == records[0].to_dict()
    assert BRepAdaptor_Surface(
        source_ledger.graph.face(next(iter(source_ledger.defining_of(source_candidate)))).wrapped
    ).Cone().SemiAngle() == pytest.approx(
        BRepAdaptor_Surface(
            imported_ledger.graph.face(
                next(iter(imported_ledger.defining_of(imported_candidate)))
            ).wrapped
        )
        .Cone()
        .SemiAngle()
    )


def test_aggregate_inventory_publishes_terminal_countersink_evidence() -> None:
    product = _take_inventory(_plate(((-20, 0), (20, 0))))
    candidates = product.physical.candidate_set(FamilyId.COUNTERSINKS).candidates
    assert candidates
    assert tuple(candidate.record for candidate in candidates) == product.result.countersinks
    assert product.accepted.candidate_set(FamilyId.COUNTERSINKS).candidates == candidates
    assert all(len(product.evidence.defining_of(candidate)) == 1 for candidate in candidates)


def test_multiple_valid_solids_publish_independent_owner_bodies() -> None:
    part = _plate() + Pos(150, 0, 0) * _plate()
    ledger, records = _claimed(part)
    candidates = ledger.candidate_set(FamilyId.COUNTERSINKS).candidates
    assert len(records) == len(candidates) == 2
    solids = {ledger.graph.common_valid_solid(ledger.defining_of(item)) for item in candidates}
    assert None not in solids and len(solids) == 2


def test_late_owner_validation_refuses_before_publication(monkeypatch: pytest.MonkeyPatch) -> None:
    part = _plate(((-20, 0), (20, 0)))
    ledger = ClaimLedger(FaceGraph(part))
    original = ledger.graph.common_valid_solid
    calls = 0

    def fail_second(nodes):
        nonlocal calls
        calls += 1
        return original(nodes) if calls == 1 else None

    monkeypatch.setattr(ledger.graph, "common_valid_solid", fail_second)
    with pytest.raises(ValueError, match="no unambiguous valid solid"):
        _discover_countersinks(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.COUNTERSINKS).candidates == ()


def test_late_owner_binding_refuses_before_publication(monkeypatch: pytest.MonkeyPatch) -> None:
    part = _plate(((-20, 0), (20, 0)))
    ledger = ClaimLedger(FaceGraph(part))
    original = ledger.graph.require_node
    calls = 0

    def fail_second(face):
        nonlocal calls
        calls += 1
        if calls == 2:
            raise ValueError("later countersink binding failed")
        return original(face)

    monkeypatch.setattr(ledger.graph, "require_node", fail_second)
    with pytest.raises(ValueError, match="later countersink binding failed"):
        _discover_countersinks(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.COUNTERSINKS).candidates == ()


def test_foreign_writer_refuses_without_publication() -> None:
    part = _plate()
    foreign = ClaimLedger(FaceGraph(Box(20, 20, 20)))
    with pytest.raises(ValueError):
        _discover_countersinks(part, writer=foreign.writer)
    assert foreign.candidate_set(FamilyId.COUNTERSINKS).candidates == ()


@pytest.mark.parametrize("translated", [False, True])
def test_deep_cloned_owner_refuses_without_publication(
    monkeypatch: pytest.MonkeyPatch, translated: bool
) -> None:
    part = _plate()
    ledger = ClaimLedger(FaceGraph(part))
    original = ledger.graph.require_node

    def cloned(face):
        changed = copy.deepcopy(face)
        if translated:
            changed = changed.translate((1, 0, 0))
        return original(changed)

    monkeypatch.setattr(ledger.graph, "require_node", cloned)
    with pytest.raises(ValueError):
        _discover_countersinks(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.COUNTERSINKS).candidates == ()


def test_reversed_face_traversal_preserves_occurrence_roles(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    part = _plate(((-20, 0), (20, 0)))
    baseline = [record.to_dict() for record in recognise_countersinks(part)]
    part_type = type(part)
    original = part_type.faces

    def reversed_faces(self):
        faces = original(self)
        return type(faces)(reversed(faces))

    monkeypatch.setattr(part_type, "faces", reversed_faces)
    ledger, records = _claimed(part)
    assert [record.to_dict() for record in records] == baseline
    assert len(ledger.candidate_set(FamilyId.COUNTERSINKS).candidates) == 2


def test_only_the_declaration_may_call_writer_enabled_core() -> None:
    assert_core_route_is_closed(
        module="countersinks",
        core="_discover_countersinks",
        handed_over={"writer": "services.writer"},
        also_reached_from={"recognise_countersinks": ("writer",)},
    )


def test_counter_sink_constructor_roster_is_closed() -> None:
    root = Path(__file__).parents[1]
    sites: list[tuple[str, str]] = []
    for path in (root / "src" / "quiddity").glob("*.py"):
        tree = ast.parse(path.read_text(encoding="utf-8"))
        for qualified, node in _qualified_calls(tree):
            if (path.name == "countersinks.py" and qualified == "CounterSink") or qualified in {
                "quiddity.countersinks.CounterSink",
                "quiddity.CounterSink",
            }:
                function = next(
                    (
                        parent.name
                        for parent in ast.walk(tree)
                        if isinstance(parent, ast.FunctionDef) and node in ast.walk(parent)
                    ),
                    "",
                )
                sites.append((path.name, function))
    assert sites == [("countersinks.py", "_discover_countersinks")]
