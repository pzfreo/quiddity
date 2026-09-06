"""F5c: Flat occurrences own only their planar truncation face."""

from __future__ import annotations

import ast
import copy
import math
from pathlib import Path

import pytest
from build123d import (
    Align,
    Box,
    Cylinder,
    GeomType,
    Plane,
    Pos,
    RegularPolygon,
    Rot,
    Shell,
    export_step,
    extrude,
    import_step,
)

from quiddity import recognise_flats
from quiddity._adjacency import FaceGraph, edge_face_map, neighbours
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._cylinder_substrate import analyse_cylinders
from quiddity._geometry import _axis_line_coordinates, _canonical_axis_direction
from quiddity.flats import _discover_flats

_CENTRE = (Align.CENTER, Align.CENTER, Align.CENTER)


def _lone_d():
    return Cylinder(20, 40) - Pos(50, 0, 0) * Box(80, 80, 60, align=_CENTRE)


def _double_d():
    return Cylinder(20, 40) & Box(25, 60, 60, align=_CENTRE)


def _xyz(value) -> tuple[float, float, float]:
    return (value.X, value.Y, value.Z)


def _assert_flat_role(part, ledger: ClaimLedger, candidate, record) -> None:
    """Reconstruct the Flat from fresh topology/cylinder facts, not issued evidence."""

    defining = ledger.defining_of(candidate)
    assert len(defining) == 1
    (node,) = defining
    owner_solid = ledger.graph.common_valid_solid((node,))
    assert owner_solid is not None
    owner = ledger.graph.face(node)
    assert owner.geom_type == GeomType.PLANE
    center = _xyz(owner.center())
    normal = _xyz(owner.normal_at(owner.center()))
    edges = edge_face_map(part.faces())
    adjacent = set(neighbours(owner, edges))
    cylinders = [
        fact
        for inventory in analyse_cylinders(part)
        for fact in inventory
        if fact.get("external")
        and fact["face"] in adjacent
        and ledger.graph.common_valid_solid((node, ledger.graph.require_node(fact["face"])))
        is owner_solid
    ]
    assert cylinders
    stock = cylinders[0]
    assert all(
        fact["diameter"] == pytest.approx(stock["diameter"])
        and fact["axis_xyz"] == pytest.approx(stock["axis_xyz"])
        and fact["dir_xyz"] == pytest.approx(stock["dir_xyz"])
        for fact in cylinders
    )
    axis = stock["axis"]
    direction = _canonical_axis_direction(axis, stock["dir_xyz"])
    assert record.axis == axis
    assert record.axis_direction == pytest.approx(direction, abs=1e-9)
    assert record.axis_line == pytest.approx(
        _axis_line_coordinates(axis, stock["axis_xyz"], direction), abs=1e-9
    )
    assert record.stock_span == pytest.approx(
        (round(stock["s_lo"], 3), round(stock["s_hi"], 3)), abs=1e-9
    )
    assert record.at == pytest.approx(tuple(round(v, 3) for v in center), abs=1e-9)

    axis_point = stock["axis_xyz"]
    own_offset = sum((center[i] - axis_point[i]) * normal[i] for i in range(3))
    radius = stock["diameter"] / 2
    stock_faces = {
        fact["face"]
        for inventory in analyse_cylinders(part)
        for fact in inventory
        if fact.get("external")
        and ledger.graph.common_valid_solid((node, ledger.graph.require_node(fact["face"])))
        is owner_solid
        and fact["diameter"] == pytest.approx(stock["diameter"])
        and fact["axis_xyz"] == pytest.approx(stock["axis_xyz"])
        and fact["dir_xyz"] == pytest.approx(stock["dir_xyz"])
        and fact["s_lo"] == pytest.approx(stock["s_lo"])
        and fact["s_hi"] == pytest.approx(stock["s_hi"])
    }
    opposed = []
    for face in part.faces():
        if face is owner or face.geom_type != GeomType.PLANE:
            continue
        if not stock_faces.intersection(neighbours(face, edges)):
            continue
        other_node = ledger.graph.require_node(face)
        if ledger.graph.common_valid_solid((node, other_node)) is not owner_solid:
            continue
        other_normal = _xyz(face.normal_at(face.center()))
        if sum(normal[i] * other_normal[i] for i in range(3)) > -0.95:
            continue
        other_center = _xyz(face.center())
        other_offset = sum((other_center[i] - axis_point[i]) * other_normal[i] for i in range(3))
        if 0.05 < other_offset < radius - 0.05:
            opposed.append((face, other_offset))
    assert len(opposed) <= 1
    expected_across = own_offset + opposed[0][1] if opposed else own_offset + radius
    assert record.across == pytest.approx(round(expected_across, 3), abs=1e-9)
    assert stock["face"] is not owner
    if opposed:
        assert ledger.graph.require_node(opposed[0][0]) not in defining
    assert math.isfinite(expected_across)


@pytest.mark.parametrize(
    "part",
    [
        _lone_d(),
        _double_d(),
        Cylinder(20, 40) & Box(30, 30, 60, align=_CENTRE),
        Cylinder(20, 40) & extrude(RegularPolygon(22, 6), 40),
        Rot(0, 90, 0) * _lone_d(),
        Rot(90, 0, 0) * _lone_d(),
        Rot(0, 25, 0) * _lone_d(),
        _lone_d().mirror(Plane.YZ),
        _lone_d().scale(10),
        Cylinder(20, 40) - Pos(59.49, 0, 0) * Box(80, 80, 60, align=_CENTRE),
    ],
)
def test_flat_writer_preserves_records_and_binds_only_each_owner_face(part) -> None:
    plain = recognise_flats(part)
    ledger = ClaimLedger(FaceGraph(part))
    measured = _discover_flats(
        part,
        cyls=analyse_cylinders(part),
        face_edges=None,
        writer=ledger.writer,
    )

    assert measured == plain
    assert [item.to_dict() for item in measured] == [item.to_dict() for item in plain]
    candidates = ledger.candidate_set(FamilyId.FLATS).candidates
    assert len(candidates) == len(measured)
    defining_sets = []
    for candidate, record in zip(candidates, measured, strict=True):
        assert candidate.record is record
        defining = ledger.defining_of(candidate)
        assert ledger.graph.common_valid_solid(defining) is not None
        _assert_flat_role(part, ledger, candidate, record)
        defining_sets.append(defining)
    assert len(set(defining_sets)) == len(defining_sets)


def test_later_flat_binding_failure_publishes_no_prefix(monkeypatch) -> None:
    part = _double_d()
    ledger = ClaimLedger(FaceGraph(part))
    real_require = ledger.graph.require_node
    calls = 0

    def fail_second(face):
        nonlocal calls
        calls += 1
        if calls == 2:
            raise ValueError("later face binding failed")
        return real_require(face)

    monkeypatch.setattr(ledger.graph, "require_node", fail_second)
    with pytest.raises(ValueError, match="later face binding failed"):
        _discover_flats(
            part,
            cyls=analyse_cylinders(part),
            face_edges=None,
            writer=ledger.writer,
        )
    assert ledger.candidate_set(FamilyId.FLATS).candidates == ()


def test_later_flat_body_validation_failure_publishes_no_prefix(monkeypatch) -> None:
    part = _double_d()
    ledger = ClaimLedger(FaceGraph(part))
    real_common = ledger.graph.common_valid_solid
    calls = 0

    def fail_second(nodes):
        nonlocal calls
        calls += 1
        if calls == 2:
            return None
        return real_common(nodes)

    monkeypatch.setattr(ledger.graph, "common_valid_solid", fail_second)
    with pytest.raises(ValueError, match="no unambiguous valid solid"):
        _discover_flats(
            part,
            cyls=analyse_cylinders(part),
            face_edges=None,
            writer=ledger.writer,
        )
    assert ledger.candidate_set(FamilyId.FLATS).candidates == ()


def test_flat_writer_from_another_graph_refuses_without_publication() -> None:
    part = _lone_d()
    foreign = ClaimLedger(FaceGraph(_double_d()))
    with pytest.raises(ValueError):
        _discover_flats(
            part,
            cyls=analyse_cylinders(part),
            face_edges=None,
            writer=foreign.writer,
        )
    assert foreign.candidate_set(FamilyId.FLATS).candidates == ()


def test_registry_is_the_only_production_writer_enabled_flat_caller() -> None:
    package = Path(__file__).parents[1] / "src" / "quiddity"
    importers = set()
    for path in package.glob("*.py"):
        if path.name == "flats.py":
            continue
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        direct = any(
            isinstance(node, ast.ImportFrom)
            and node.module == "quiddity.flats"
            and any(alias.name == "_discover_flats" for alias in node.names)
            for node in ast.walk(tree)
        )
        qualified = any(
            isinstance(node, ast.Attribute) and node.attr == "_discover_flats"
            for node in ast.walk(tree)
        )
        if direct or qualified:
            importers.add(path.name)
    assert importers == {"_registry.py"}


@pytest.mark.parametrize(
    "part",
    [
        Cylinder(20, 40),
        Cylinder(20, 40) - Cylinder(5, 40),
        Cylinder(20, 40) - Pos(59.8, 0, 0) * Box(80, 80, 60, align=_CENTRE),
        Box(60, 60, 20) - Pos(0, 0, 5) * Box(15, 20, 10, align=_CENTRE),
        Cylinder(20, 40) - Box(50, 8, 20, align=_CENTRE),
        Cylinder(20, 40) - Pos(14, 0, 0) * Box(20, 8, 20, align=_CENTRE),
    ],
)
def test_rejected_context_geometry_issues_no_flat_candidate(part) -> None:
    ledger = ClaimLedger(FaceGraph(part))
    assert (
        _discover_flats(
            part,
            cyls=analyse_cylinders(part),
            face_edges=None,
            writer=ledger.writer,
        )
        == []
    )
    assert ledger.candidate_set(FamilyId.FLATS).candidates == ()


def test_open_flat_topology_refuses_before_publication() -> None:
    part = _lone_d()
    shell = Shell(part.faces()[:-1])
    ledger = ClaimLedger(FaceGraph(shell))
    with pytest.raises(ValueError, match="no unambiguous valid solid"):
        _discover_flats(
            shell,
            cyls=analyse_cylinders(shell),
            face_edges=None,
            writer=ledger.writer,
        )
    assert ledger.candidate_set(FamilyId.FLATS).candidates == ()


def test_deep_copied_face_binding_refuses_before_publication(monkeypatch) -> None:
    part = _double_d()
    ledger = ClaimLedger(FaceGraph(part))
    real_require = ledger.graph.require_node

    def copied(face):
        return real_require(copy.deepcopy(face))

    monkeypatch.setattr(ledger.graph, "require_node", copied)
    with pytest.raises(ValueError):
        _discover_flats(
            part,
            cyls=analyse_cylinders(part),
            face_edges=None,
            writer=ledger.writer,
        )
    assert ledger.candidate_set(FamilyId.FLATS).candidates == ()


def test_mutated_face_binding_refuses_before_publication(monkeypatch) -> None:
    part = _double_d()
    ledger = ClaimLedger(FaceGraph(part))
    real_require = ledger.graph.require_node

    def mutated(face):
        changed = copy.deepcopy(face).translate((1, 0, 0))
        return real_require(changed)

    monkeypatch.setattr(ledger.graph, "require_node", mutated)
    with pytest.raises(ValueError):
        _discover_flats(
            part,
            cyls=analyse_cylinders(part),
            face_edges=None,
            writer=ledger.writer,
        )
    assert ledger.candidate_set(FamilyId.FLATS).candidates == ()


def test_reversed_face_traversal_preserves_flat_roles(monkeypatch) -> None:
    part = Cylinder(20, 40) & extrude(RegularPolygon(22, 6), 40)
    baseline = [record.to_dict() for record in recognise_flats(part)]
    part_type = type(part)
    real_faces = part_type.faces

    def reversed_faces(self):
        faces = real_faces(self)
        return type(faces)(reversed(faces))

    monkeypatch.setattr(part_type, "faces", reversed_faces)
    ledger = ClaimLedger(FaceGraph(part))
    measured = _discover_flats(
        part,
        cyls=analyse_cylinders(part),
        face_edges=None,
        writer=ledger.writer,
    )
    assert [record.to_dict() for record in measured] == baseline
    for candidate, record in zip(
        ledger.candidate_set(FamilyId.FLATS).candidates, measured, strict=True
    ):
        _assert_flat_role(part, ledger, candidate, record)


def test_real_step_round_trip_preserves_flat_occurrence_roles(tmp_path) -> None:
    target = tmp_path / "double-d.step"
    assert export_step(_double_d(), target)
    part = import_step(target)
    plain = recognise_flats(part)
    ledger = ClaimLedger(FaceGraph(part))
    measured = _discover_flats(
        part,
        cyls=analyse_cylinders(part),
        face_edges=None,
        writer=ledger.writer,
    )
    assert [record.to_dict() for record in measured] == [record.to_dict() for record in plain]
    for candidate, record in zip(
        ledger.candidate_set(FamilyId.FLATS).candidates, measured, strict=True
    ):
        assert candidate.record is record
        _assert_flat_role(part, ledger, candidate, record)


def test_disjoint_coaxial_stock_regions_keep_occurrence_specific_context() -> None:
    def lone(radius: float, offset: float, z: float):
        beyond = 40 if offset >= 0 else -40
        cutter = Pos(offset + beyond, 0, 0) * Box(80, 80, 60, align=_CENTRE)
        return Pos(0, 0, z) * (Cylinder(radius, 40) - cutter)

    part = lone(20, 10, -30) + Cylinder(5, 60) + lone(12, -8, 60)
    ledger = ClaimLedger(FaceGraph(part))
    measured = _discover_flats(
        part,
        cyls=analyse_cylinders(part),
        face_edges=None,
        writer=ledger.writer,
    )
    assert sorted(record.across for record in measured) == [20.0, 30.0]
    for candidate, record in zip(
        ledger.candidate_set(FamilyId.FLATS).candidates, measured, strict=True
    ):
        _assert_flat_role(part, ledger, candidate, record)


def test_multiple_valid_solids_emit_independent_flat_occurrences() -> None:
    part = Pos(0, -60, 0) * _lone_d() + Pos(0, 60, 0) * _lone_d()
    ledger = ClaimLedger(FaceGraph(part))
    measured = _discover_flats(
        part,
        cyls=analyse_cylinders(part),
        face_edges=None,
        writer=ledger.writer,
    )
    candidates = ledger.candidate_set(FamilyId.FLATS).candidates
    assert len(measured) == len(candidates) == 2
    solids = []
    for candidate, record in zip(candidates, measured, strict=True):
        _assert_flat_role(part, ledger, candidate, record)
        solids.append(ledger.graph.common_valid_solid(ledger.defining_of(candidate)))
    assert solids[0] is not solids[1]
