"""F5f: each Boss occurrence owns its complete original external segment."""

from __future__ import annotations

import ast
import copy
import math
from pathlib import Path

import pytest
from build123d import (
    Axis,
    Box,
    Compound,
    Cylinder,
    GeomType,
    Part,
    Pos,
    Rot,
    Shell,
    Sphere,
    chamfer,
    export_step,
    fillet,
    import_step,
)
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepBuilderAPI import BRepBuilderAPI_NurbsConvert
from OCP.GeomAbs import GeomAbs_Cone, GeomAbs_Cylinder, GeomAbs_Plane, GeomAbs_Sphere, GeomAbs_Torus

from quiddity import recognise_bosses
from quiddity._adjacency import (
    FaceEdges,
    FaceGraph,
    edge_face_map,
    frame_points_outward,
)
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._cylinder_substrate import analyse_cylinders, full_cylinders
from quiddity._effective_surfaces import SurfaceKind, SurfaceProvenance
from quiddity._hole_features import _classify_end, _discover_bosses, _segments
from quiddity.result import _take_inventory

ROOT = Path(__file__).parents[1]


def test_recovered_boss_candidate_retains_original_cylinder_dependency() -> None:
    converted = Part(BRepBuilderAPI_NurbsConvert(Cylinder(4, 10).wrapped, True).Shape())
    ledger = ClaimLedger(FaceGraph(converted))

    assert _discover_bosses(converted, writer=ledger.writer) == recognise_bosses(converted)
    (candidate,) = ledger.candidate_set(FamilyId.BOSSES).candidates
    (surface_use,) = candidate.evidence.surfaces
    assert surface_use.node in candidate.evidence.defining
    assert surface_use.surface.kind is SurfaceKind.CYLINDER
    assert surface_use.surface.provenance is SurfaceProvenance.RECOVERED
    assert surface_use.material_side is not None
    assert surface_use.material_side.candidate_outward_sign == 1


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


def _fresh_external_segments(part):
    """Independently rebuild full external runs from fresh native adaptors."""

    by_axis = {"z": [], "cross": []}
    solids = part.solids()
    faces = (
        [(solid_idx, face) for solid_idx, solid in enumerate(solids) for face in solid.faces()]
        if solids
        else [(0, face) for face in part.faces()]
    )
    for solid_idx, face in faces:
        surface = BRepAdaptor_Surface(face.wrapped)
        if surface.GetType() != GeomAbs_Cylinder or not frame_points_outward(face):
            continue
        cylinder = surface.Cylinder()
        direction = cylinder.Axis().Direction()
        raw = (direction.X(), direction.Y(), direction.Z())
        dominant = max(range(3), key=lambda index: abs(raw[index]))
        sign = 1.0 if raw[dominant] > 0 else -1.0
        unit = tuple(sign * value for value in raw)
        point = cylinder.Axis().Location()
        axis_point = (point.X(), point.Y(), point.Z())
        projected = sum(axis_point[index] * unit[index] for index in range(3))
        radial = tuple(axis_point[index] - projected * unit[index] for index in range(3))
        v0, v1 = surface.FirstVParameter(), surface.LastVParameter()
        s0, s1 = projected + sign * v0, projected + sign * v1
        diameter = float(f"{2 * cylinder.Radius():.6g}")
        axis = "xyz"[dominant]
        by_axis["z" if axis == "z" else "cross"].append(
            {
                "key": (
                    solid_idx,
                    axis,
                    *(round(value, 3) for value in radial),
                    float(f"{diameter:.4g}"),
                ),
                "diameter": diameter,
                "dir_xyz": unit,
                "axis_xyz": axis_point,
                "s_lo": min(s0, s1),
                "s_hi": max(s0, s1),
                "u_extent": surface.LastUParameter() - surface.FirstUParameter(),
                "face": face,
            }
        )

    segments = []
    for partition in (by_axis["z"], by_axis["cross"]):
        groups = {}
        for fact in partition:
            groups.setdefault(fact["key"], []).append(fact)
        for group in groups.values():
            group.sort(key=lambda fact: fact["s_lo"])
            run = [group[0]]
            hi = group[0]["s_hi"]
            for fact in group[1:]:
                gap = abs(fact["diameter"]) * 0.0125 + 1e-6
                if fact["s_lo"] <= hi + gap:
                    run.append(fact)
                    hi = max(hi, fact["s_hi"])
                else:
                    if sum(item["u_extent"] for item in run) >= math.pi * 1.05:
                        segments.append(run)
                    run, hi = [fact], fact["s_hi"]
            if sum(item["u_extent"] for item in run) >= math.pi * 1.05:
                segments.append(run)
    return [
        {
            **run[0],
            "s_lo": min(item["s_lo"] for item in run),
            "s_hi": max(item["s_hi"] for item in run),
            "faces": tuple(item["face"] for item in run),
        }
        for run in segments
    ]


def _axis_point(segment, coordinate):
    axis_point = segment["axis_xyz"]
    direction = segment["dir_xyz"]
    projected = sum(axis_point[index] * direction[index] for index in range(3))
    return tuple(
        axis_point[index] + (coordinate - projected) * direction[index] for index in range(3)
    )


def _fresh_end_state(part, segment, coordinate, high):
    direction = segment["dir_xyz"]
    sign = 1.0 if high else -1.0
    span = segment["s_hi"] - segment["s_lo"]
    margin = max(0.0125 * segment["diameter"], min(0.45 * span, 0.5 * segment["diameter"]))
    mapping = edge_face_map(part.faces())
    owners = segment["faces"]
    partners = []
    for owner in owners:
        for edge in owner.edges():
            points = [edge.center(), *(vertex.center() for vertex in edge.vertices())]
            if not all(
                abs(
                    point.X * direction[0]
                    + point.Y * direction[1]
                    + point.Z * direction[2]
                    - coordinate
                )
                <= margin
                for point in points
            ):
                continue
            partners.extend(
                face
                for face in mapping.get(edge, ())
                if not any(face.is_same(candidate) for candidate in owners)
            )
    weak = None
    for partner in partners:
        surface = BRepAdaptor_Surface(partner.wrapped)
        kind = surface.GetType()
        if kind == GeomAbs_Cone:
            apex = surface.Cone().Apex()
            apex_coordinate = (
                apex.X() * direction[0] + apex.Y() * direction[1] + apex.Z() * direction[2]
            )
            return "open" if (apex_coordinate - coordinate) * sign > 0 else "flat"
        if kind == GeomAbs_Torus:
            curls_in = surface.Torus().MajorRadius() < segment["diameter"] / 2
            return "open" if curls_in else "flat"
        if kind == GeomAbs_Plane:
            normal = partner.normal_at(partner.center())
            dot = (
                normal.X * direction[0] + normal.Y * direction[1] + normal.Z * direction[2]
            ) * sign
            if dot < -0.5:
                return "flat"
            if dot > 0.5:
                return "open"
        elif kind == GeomAbs_Sphere:
            weak = "flat" if frame_points_outward(partner) else "open"
        elif kind == GeomAbs_Cylinder:
            weak = "flat"
    return weak or "unknown"


def _assert_role(part, ledger: ClaimLedger, candidate, record, segment) -> None:
    expected = frozenset(ledger.graph.require_node(face) for face in segment["faces"])
    defining = ledger.defining_of(candidate)
    assert defining == expected
    assert defining
    assert ledger.graph.common_valid_solid(defining) is not None
    assert record.diameter == pytest.approx(segment["diameter"])
    assert record.height == pytest.approx(round(segment["s_hi"] - segment["s_lo"], 2))

    direction = segment["dir_xyz"]
    low = tuple(round(value, 10) for value in _axis_point(segment, segment["s_lo"]))
    high = tuple(round(value, 10) for value in _axis_point(segment, segment["s_hi"]))
    low_state = _fresh_end_state(part, segment, segment["s_lo"], False)
    high_state = _fresh_end_state(part, segment, segment["s_hi"], True)
    from_high = not (low_state == "open" and high_state != "open")
    expected_location = high if from_high else low
    expected_axis = direction if from_high else tuple(-value for value in direction)
    assert tuple(round(value, 10) for value in record.location) == expected_location
    assert record.axis == pytest.approx(expected_axis)


def _claimed(part, **kwargs):
    plain = recognise_bosses(part, **kwargs)
    ledger = ClaimLedger(FaceGraph(part))
    measured = _discover_bosses(part, writer=ledger.writer, **kwargs)
    assert measured == plain
    assert [record.to_dict() for record in measured] == [record.to_dict() for record in plain]
    candidates = ledger.candidate_set(FamilyId.BOSSES).candidates
    segments = _fresh_external_segments(part)
    assert len(candidates) == len(measured) == len(segments)
    for candidate, record, segment in zip(candidates, measured, segments, strict=True):
        assert candidate.record is record
        _assert_role(part, ledger, candidate, record, segment)
    return ledger, measured


def _bossed_plate(x: float = 0.0):
    return Box(60, 60, 10) + Pos(x, 0, 9) * Cylinder(12, 8)


def test_boss_writer_preserves_public_output_and_complete_segment_role() -> None:
    ledger, records = _claimed(_bossed_plate())
    assert len(records) == 1
    candidate = ledger.candidate_set(FamilyId.BOSSES).candidates[0]
    defining = ledger.defining_of(candidate)
    constituent = ledger.snapshot_index().constituent_of(candidate)
    terminal = constituent - defining
    assert len(defining) == 1 and defining < constituent
    assert len(terminal) == 1
    assert (
        BRepAdaptor_Surface(ledger.graph.face(next(iter(terminal))).wrapped).GetType()
        == GeomAbs_Plane
    )


@pytest.mark.parametrize(
    ("part", "axis", "location"),
    [
        (_bossed_plate(), (0.0, 0.0, 1.0), (0.0, 0.0, 13.0)),
        (
            Box(60, 60, 10) + Pos(0, 0, -9) * Cylinder(12, 8),
            (0.0, 0.0, -1.0),
            (0.0, 0.0, -13.0),
        ),
        (Rot(0, 90, 0) * _bossed_plate(), (1.0, 0.0, 0.0), (13.0, 0.0, 0.0)),
        (Rot(90, 0, 0) * _bossed_plate(), (0.0, -1.0, 0.0), (0.0, -13.0, 0.0)),
    ],
)
def test_plate_boss_orientation_is_rederived_for_both_sides_and_xyz(part, axis, location) -> None:
    ledger, (record,) = _claimed(part)
    assert record.axis == pytest.approx(axis)
    assert record.location == pytest.approx(location)
    (candidate,) = ledger.candidate_set(FamilyId.BOSSES).candidates
    defining = ledger.defining_of(candidate)
    terminal = ledger.snapshot_index().constituent_of(candidate) - defining
    assert len(terminal) == 1
    normal = ledger.graph.normal(next(iter(terminal)))
    assert normal is not None and sum(a * b for a, b in zip(normal, axis, strict=True)) > 0.99


def test_chamfered_and_filleted_free_ends_keep_owner_and_orientation() -> None:
    negative = Box(60, 60, 10) + Pos(0, 0, -9) * Cylinder(12, 8)
    chamfered = chamfer(
        negative.edges().filter_by(GeomType.CIRCLE).sort_by(Axis.Z)[0],
        1.0,
    )
    chamfered_ledger, (chamfered_record,) = _claimed(chamfered)
    assert chamfered_record.axis == pytest.approx((0.0, 0.0, -1.0))
    assert chamfered_record.location[2] == pytest.approx(-12.0)

    positive = _bossed_plate()
    free = [edge for edge in positive.edges().filter_by(GeomType.CIRCLE) if edge.center().Z > 12.9]
    filleted_ledger, (filleted_record,) = _claimed(fillet(free, 1.0))
    assert filleted_record.axis == pytest.approx((0.0, 0.0, 1.0))
    assert filleted_record.location[2] == pytest.approx(12.0)

    for ledger in (chamfered_ledger, filleted_ledger):
        (candidate,) = ledger.candidate_set(FamilyId.BOSSES).candidates
        constituent = ledger.snapshot_index().constituent_of(candidate)
        assert all(
            BRepAdaptor_Surface(ledger.graph.face(node).wrapped).GetType()
            not in (GeomAbs_Cone, GeomAbs_Torus)
            for node in constituent
        )


def test_external_spherical_end_retains_exact_classification_face() -> None:
    part = Box(60, 60, 10) + Pos(0, 0, 9) * Cylinder(5, 8) + Pos(0, 0, 13) * Sphere(5)
    z_cylinders, cross_cylinders = analyse_cylinders(part)
    external = [
        cylinder
        for cylinder in full_cylinders(z_cylinders) + full_cylinders(cross_cylinders)
        if cylinder["external"]
    ]
    (segment,) = _segments(external)
    adjacency = edge_face_map(part.faces())
    retained = []

    assert (
        _classify_end(
            segment,
            segment["s_hi"],
            True,
            adjacency,
            terminal_faces=retained,
        )
        == "flat"
    )
    assert len(retained) == 1
    assert BRepAdaptor_Surface(retained[0].wrapped).GetType() == GeomAbs_Sphere


def test_radial_pipe_boss_and_turned_od_keep_current_roles() -> None:
    pipe = Cylinder(20, 60) - Cylinder(15, 60)
    radial = pipe + Pos(-24, 0, 0) * Cylinder(5, 12, rotation=(0, 90, 0))
    ledger, records = _claimed(radial)
    candidate, record = next(
        (candidate, record)
        for candidate, record in zip(
            ledger.candidate_set(FamilyId.BOSSES).candidates, records, strict=True
        )
        if record.diameter == pytest.approx(10.0)
    )
    assert record.axis == pytest.approx((-1.0, 0.0, 0.0))
    assert record.location[0] == pytest.approx(-30.0)
    assert ledger.defining_of(candidate)

    _ledger, (turned,) = _claimed(Cylinder(30, 40) - Cylinder(10, 40))
    assert turned.diameter == pytest.approx(60.0)
    assert turned.height == pytest.approx(40.0)


def test_opposite_radial_pipe_boss_points_outward() -> None:
    pipe = Cylinder(20, 60) - Cylinder(15, 60)
    radial = pipe + Pos(24, 0, 0) * Rot(0, 90, 0) * Cylinder(5, 12)
    _ledger, records = _claimed(radial)
    (record,) = [record for record in records if record.diameter == pytest.approx(10.0)]
    assert record.axis == pytest.approx((1.0, 0.0, 0.0))
    assert record.location[0] == pytest.approx(30.0)


def test_stepped_external_shaft_and_distinct_equal_radius_occurrences() -> None:
    stepped = Cylinder(8, 10) + Pos(0, 0, 10) * Cylinder(5, 8)
    _ledger, records = _claimed(stepped)
    assert {record.diameter for record in records} == {10.0, 16.0}

    plate = _bossed_plate(-18) + Pos(36, 0, 0) * Cylinder(12, 8)
    ledger, records = _claimed(plate)
    equal = [record for record in records if record.diameter == pytest.approx(24.0)]
    assert len(equal) == 2 and equal[0].location != equal[1].location
    candidates = ledger.candidate_set(FamilyId.BOSSES).candidates
    assert len({ledger.defining_of(candidate) for candidate in candidates}) == len(candidates)


def test_mixed_axis_emission_keeps_z_before_cross_and_occurrence_binding() -> None:
    z_boss = Pos(0, 80, 0) * _bossed_plate()
    x_boss = Pos(100, -80, 0) * Rot(0, 90, 0) * _bossed_plate()
    part = Compound([x_boss, z_boss])
    ledger, records = _claimed(part)
    assert len(records) == 2
    assert records[0].axis == pytest.approx((0.0, 0.0, 1.0))
    assert records[1].axis == pytest.approx((1.0, 0.0, 0.0))
    candidates = ledger.candidate_set(FamilyId.BOSSES).candidates
    assert all(
        candidate.record is record for candidate, record in zip(candidates, records, strict=True)
    )


def test_translation_scale_mirror_and_nonprincipal_rotation_keep_writer_parity() -> None:
    original = _bossed_plate()
    for part in (
        Pos(17, -9, 4) * original,
        original.scale(3),
        original.scale(0.05),
        original.scale(20),
        original.mirror(),
        Rot(31, 17, 43) * original,
    ):
        _ledger, records = _claimed(part)
        assert records


def test_step_round_trip_preserves_segment_role_correspondence(tmp_path) -> None:
    source_ledger, source_records = _claimed(_bossed_plate())
    target = tmp_path / "boss.step"
    assert export_step(_bossed_plate(), target)
    imported = import_step(target)
    imported_ledger, imported_records = _claimed(imported)
    assert [record.to_dict() for record in source_records] == [
        record.to_dict() for record in imported_records
    ]
    source_faces = sum(
        len(source_ledger.defining_of(candidate))
        for candidate in source_ledger.candidate_set(FamilyId.BOSSES).candidates
    )
    imported_faces = sum(
        len(imported_ledger.defining_of(candidate))
        for candidate in imported_ledger.candidate_set(FamilyId.BOSSES).candidates
    )
    assert source_faces == imported_faces == 1


def test_keyway_split_segment_claims_every_original_face() -> None:
    part = Cylinder(20, 20) - Pos(15, 0, 0) * Box(20, 4, 30) - Pos(-15, 0, 0) * Box(20, 4, 30)
    ledger, records = _claimed(part)
    candidates = ledger.candidate_set(FamilyId.BOSSES).candidates
    assert len(records) == len(candidates) == 1
    assert len(ledger.defining_of(candidates[0])) == 2


def test_equal_full_records_remain_distinct_per_body_occurrences() -> None:
    solid = _bossed_plate()
    part = Compound([solid, copy.deepcopy(solid)])
    ledger, records = _claimed(part)
    candidates = ledger.candidate_set(FamilyId.BOSSES).candidates
    assert len(records) == len(candidates) == 2
    assert records[0] == records[1] and records[0] is not records[1]
    assert candidates[0].record is records[0]
    assert candidates[1].record is records[1]
    assert ledger.defining_of(candidates[0]).isdisjoint(ledger.defining_of(candidates[1]))
    solids = [ledger.graph.common_valid_solid(ledger.defining_of(item)) for item in candidates]
    assert None not in solids and solids[0] is not solids[1]


def test_aggregate_inventory_publishes_terminal_boss_evidence() -> None:
    product = _take_inventory(_bossed_plate())
    candidates = product.physical.candidate_set(FamilyId.BOSSES).candidates
    assert candidates
    assert tuple(candidate.record for candidate in candidates) == product.result.bosses
    assert product.accepted.candidate_set(FamilyId.BOSSES).candidates == candidates
    assert all(product.evidence.defining_of(candidate) for candidate in candidates)


def test_late_body_validation_refuses_before_publication(monkeypatch: pytest.MonkeyPatch) -> None:
    part = Compound([_bossed_plate(-80), _bossed_plate(80)])
    ledger = ClaimLedger(FaceGraph(part))
    original = ledger.graph.common_valid_solid
    calls = 0

    def fail_second(nodes):
        nonlocal calls
        calls += 1
        return original(nodes) if calls == 1 else None

    monkeypatch.setattr(ledger.graph, "common_valid_solid", fail_second)
    with pytest.raises(ValueError, match="Boss defining faces"):
        _discover_bosses(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.BOSSES).candidates == ()


def test_late_binding_refuses_before_publication(monkeypatch: pytest.MonkeyPatch) -> None:
    part = Compound([_bossed_plate(-80), _bossed_plate(80)])
    ledger = ClaimLedger(FaceGraph(part))
    original = ledger.graph.require_node
    calls = 0

    def fail_later(face):
        nonlocal calls
        calls += 1
        if calls > 1:
            raise ValueError("later Boss binding failed")
        return original(face)

    monkeypatch.setattr(ledger.graph, "require_node", fail_later)
    with pytest.raises(ValueError, match="later Boss binding failed"):
        _discover_bosses(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.BOSSES).candidates == ()


def test_missing_or_aliased_boss_source_roles_refuse_before_publication(monkeypatch) -> None:
    import quiddity._hole_features as module

    part = _bossed_plate()
    ledger = ClaimLedger(FaceGraph(part))
    original_segments = module._segments

    def without_segment_faces(cylinders):
        return [dict(segment, faces=[]) for segment in original_segments(cylinders)]

    monkeypatch.setattr(module, "_segments", without_segment_faces)
    with pytest.raises(ValueError, match="defining faces"):
        _discover_bosses(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.BOSSES).candidates == ()

    monkeypatch.setattr(module, "_segments", original_segments)
    original_classify = module._classify_end

    def alias_terminal(segment, *args, terminal_faces=None, **kwargs):
        state = original_classify(
            segment,
            *args,
            terminal_faces=terminal_faces,
            **kwargs,
        )
        if state == "open" and terminal_faces is not None:
            terminal_faces.append(segment["faces"][0])
        return state

    monkeypatch.setattr(module, "_classify_end", alias_terminal)
    with pytest.raises(ValueError, match="terminal identity aliases"):
        _discover_bosses(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.BOSSES).candidates == ()


def test_repeated_supplied_cylinder_face_collapses_to_one_defining_node() -> None:
    part = _bossed_plate()
    z_cyls, cross_cyls = analyse_cylinders(part)
    external = next(fact for fact in z_cyls if fact["external"])
    supplied = ([*z_cyls, external], cross_cyls)
    ledger = ClaimLedger(FaceGraph(part))
    records = _discover_bosses(part, cyls=supplied, writer=ledger.writer)
    assert records == recognise_bosses(part, cyls=supplied)
    (candidate,) = ledger.candidate_set(FamilyId.BOSSES).candidates
    assert len(ledger.defining_of(candidate)) == 1


def test_supplied_cylinders_and_face_edges_preserve_full_lifecycle() -> None:
    part = _bossed_plate()
    supplied = analyse_cylinders(part)
    _ledger, supplied_records = _claimed(part, cyls=supplied, face_edges=FaceEdges())
    assert supplied_records == recognise_bosses(part)


def test_supplied_face_edge_memo_reads_each_face_once() -> None:
    class CountingFaceEdges(FaceEdges):
        def __init__(self):
            super().__init__()
            self.counts = {}

        def of(self, face):
            self.counts[id(face.wrapped)] = self.counts.get(id(face.wrapped), 0) + 1
            return super().of(face)

    memo = CountingFaceEdges()
    _ledger, records = _claimed(_bossed_plate(), face_edges=memo)
    assert records
    assert memo.counts and max(memo.counts.values()) == 1


@pytest.mark.parametrize(
    "part",
    [
        Box(60, 60, 20) - Cylinder(5, 20),
        Box(40, 40, 10) - (Box(16, 8, 20) + Cylinder(4, 20)),
        Box(30, 30, 10),
    ],
)
def test_internal_partial_and_non_cylindrical_shapes_issue_no_boss(part) -> None:
    assert recognise_bosses(part) == []
    ledger = ClaimLedger(FaceGraph(part))
    assert _discover_bosses(part, writer=ledger.writer) == []
    assert ledger.candidate_set(FamilyId.BOSSES).candidates == ()


def test_foreign_supplied_cylinder_refuses_before_publication() -> None:
    part = _bossed_plate()
    foreign_part = Pos(200, 0, 0) * Cylinder(5, 10)
    supplied = analyse_cylinders(foreign_part)
    ledger = ClaimLedger(FaceGraph(part))
    assert recognise_bosses(part, cyls=supplied)
    with pytest.raises(ValueError):
        _discover_bosses(part, cyls=supplied, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.BOSSES).candidates == ()


@pytest.mark.parametrize("translated", [False, True])
def test_cloned_supplied_segment_face_refuses_without_publication(translated) -> None:
    part = _bossed_plate()
    z_cyls, cross_cyls = analyse_cylinders(part)
    supplied_z = [dict(fact) for fact in z_cyls]
    owner = next(fact for fact in supplied_z if fact["external"])
    clone = copy.deepcopy(owner["face"])
    if translated:
        clone = Pos(1, 0, 0) * clone
    owner["face"] = clone
    ledger = ClaimLedger(FaceGraph(part))
    assert recognise_bosses(part, cyls=(supplied_z, cross_cyls))
    with pytest.raises(ValueError):
        _discover_bosses(part, cyls=(supplied_z, cross_cyls), writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.BOSSES).candidates == ()


def test_mixed_body_supplied_segment_refuses_before_publication() -> None:
    solid = _bossed_plate()
    part = Compound([solid, Pos(150, 0, 0) * copy.deepcopy(solid)])
    z_cyls, _cross_cyls = analyse_cylinders(part)
    owners = [fact for fact in z_cyls if fact["external"] and fact["diameter"] == 24]
    assert len(owners) == 2
    mixed = [dict(owners[0]), dict(owners[1])]
    mixed[1]["solid_idx"] = mixed[0]["solid_idx"]
    mixed[1]["axis_xyz"] = mixed[0]["axis_xyz"]
    ledger = ClaimLedger(FaceGraph(part))
    assert recognise_bosses(part, cyls=(mixed, []))
    with pytest.raises(ValueError, match="Boss defining faces"):
        _discover_bosses(part, cyls=(mixed, []), writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.BOSSES).candidates == ()


def test_real_face_axial_union_substitution_measures_span_then_refuses_mixed_body() -> None:
    part = Compound([Cylinder(5, 10), Pos(0, 0, 10) * Cylinder(5, 10)])
    z_cyls, _cross_cyls = analyse_cylinders(part)
    owners = [fact for fact in z_cyls if fact["external"]]
    assert len(owners) == 2
    supplied = [dict(fact) for fact in owners]
    supplied[1]["solid_idx"] = supplied[0]["solid_idx"]
    (record,) = recognise_bosses(part, cyls=(supplied, []))
    assert record.height == pytest.approx(20.0)
    ledger = ClaimLedger(FaceGraph(part))
    with pytest.raises(ValueError, match="Boss defining faces"):
        _discover_bosses(part, cyls=(supplied, []), writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.BOSSES).candidates == ()


def test_open_shell_keeps_geometry_result_but_refuses_aggregate_publication() -> None:
    shell = Shell(_bossed_plate().faces())
    assert recognise_bosses(shell)
    ledger = ClaimLedger(FaceGraph(shell))
    with pytest.raises(ValueError, match="Boss defining faces"):
        _discover_bosses(shell, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.BOSSES).candidates == ()


def test_reversed_edge_face_traversal_preserves_occurrence_roles(monkeypatch) -> None:
    part = Compound([_bossed_plate(-80), _bossed_plate(80)])
    baseline = [record.to_dict() for record in recognise_bosses(part)]
    part_type = type(part)
    original = part_type.faces

    def reversed_faces(self):
        faces = original(self)
        return type(faces)(reversed(faces))

    monkeypatch.setattr(part_type, "faces", reversed_faces)
    _ledger, records = _claimed(part)
    assert [record.to_dict() for record in records] == baseline


def test_supplied_cylinder_permutation_preserves_occurrence_correspondence() -> None:
    part = Compound(
        [
            Pos(-80, 0, 0) * Cylinder(5, 8),
            Cylinder(7, 12),
            Pos(80, 0, 0) * Cylinder(9, 16),
        ]
    )
    z_cyls, cross_cyls = analyse_cylinders(part)

    def run(supplied):
        ledger = ClaimLedger(FaceGraph(part))
        records = _discover_bosses(part, cyls=supplied, writer=ledger.writer)
        assert records == recognise_bosses(part, cyls=supplied)
        candidates = ledger.candidate_set(FamilyId.BOSSES).candidates
        assert all(
            candidate.record is record
            for candidate, record in zip(candidates, records, strict=True)
        )
        signatures = {
            (
                tuple(sorted(record.to_dict().items())),
                tuple(
                    sorted(
                        tuple(round(value, 6) for value in ledger.graph.face(node).center())
                        for node in ledger.defining_of(candidate)
                    )
                ),
            )
            for candidate, record in zip(candidates, records, strict=True)
        }
        return records, signatures

    baseline_records, baseline_signatures = run((z_cyls, cross_cyls))
    reversed_records, reversed_signatures = run((list(reversed(z_cyls)), cross_cyls))
    assert [record.to_dict() for record in reversed_records] == list(
        reversed([record.to_dict() for record in baseline_records])
    )
    assert reversed_signatures == baseline_signatures


def test_foreign_writer_refuses_without_publication() -> None:
    part = _bossed_plate()
    foreign = ClaimLedger(FaceGraph(Box(20, 20, 20)))
    with pytest.raises(ValueError):
        _discover_bosses(part, writer=foreign.writer)
    assert foreign.candidate_set(FamilyId.BOSSES).candidates == ()


def test_private_core_has_one_production_writer_caller_and_one_constructor() -> None:
    package = ROOT / "src/quiddity"
    core_sites = []
    constructor_sites = []
    for path in package.glob("*.py"):
        tree = ast.parse(path.read_text(encoding="utf-8"))
        for qualified, call in _qualified_calls(tree):
            if qualified.endswith("._discover_bosses") or qualified == "_discover_bosses":
                core_sites.append((path.name, call))
            if qualified.endswith(".BossRecord") or qualified == "BossRecord":
                constructor_sites.append((path.name, call))
    assert len(core_sites) == 2
    assert {path for path, _call in core_sites} == {"_hole_features.py", "_registry.py"}
    registry_call = next(call for path, call in core_sites if path == "_registry.py")
    writer = next(keyword.value for keyword in registry_call.keywords if keyword.arg == "writer")
    assert isinstance(writer, ast.Attribute)
    assert isinstance(writer.value, ast.Name) and writer.value.id == "s" and writer.attr == "writer"
    assert [(path, len(call.args)) for path, call in constructor_sites] == [
        ("_hole_features.py", 0)
    ]


def test_boss_extraction_does_not_move_shared_surface_reader_authority() -> None:
    from quiddity._effective_surfaces import SURFACE_READER_SITES

    assert "_hole_features:_classify_end_uncached:adaptor:1" in SURFACE_READER_SITES
    assert "_hole_features:_classify_end_uncached:adaptor:2" in SURFACE_READER_SITES
    assert not any("_discover_bosses" in key for key in SURFACE_READER_SITES)
