"""#594: construction-defined circular troughs use physical open arc profiles."""

import math

import pytest
from build123d import Box, Cylinder, GeomType, Pos, Rot, export_step, fillet
from OCP.BRep import BRep_Tool
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeEdge
from OCP.BRepFeat import BRepFeat_SplitShape

from quiddity import (
    build_framed_recognition_evidence,
    build_section_recess_document,
    import_step_geometry,
)
from quiddity._adjacency import FaceGraph
from quiddity._cylindrical_seats import cylindrical_seat_proofs
from quiddity.evidence import build_recognition_evidence

SEAT_SWEEP = math.radians(62.888787672)


def seat(sweep=SEAT_SWEEP):
    height = 5 + 7.15 * math.cos(sweep / 2)
    return Box(20, 6, 10) - Pos(0, 0, height) * Cylinder(7.15, 8, rotation=(90, 0, 0))


def seats(records):
    return [
        r
        for r in records
        if r.classification.feature_kind == "channel"
        and r.classification.section_shape == "circular"
    ]


def assert_arc(record, radius, sweep, length):
    geometry = record.geometry
    assert geometry.profile.closure == "open"
    first, last = geometry.profile.boundary
    assert last.bulge == 0
    assert 4 * math.atan(abs(first.bulge)) == pytest.approx(sweep)
    chord = math.dist(first.point, last.point)
    recovered_radius = chord * (1 + first.bulge**2) / (4 * abs(first.bulge))
    assert recovered_radius == pytest.approx(radius, abs=0.0008)
    assert geometry.run_interval[1] - geometry.run_interval[0] == pytest.approx(length, abs=0.001)
    assert geometry.ends.low.condition == geometry.ends.high.condition == "open"


@pytest.mark.parametrize("sweep", [math.pi / 6, math.radians(62.888787672), math.pi / 2, math.pi])
@pytest.mark.parametrize("scale", [0.1, 1, 10])
@pytest.mark.parametrize("placed", [False, True])
def test_arc_geometry_survives_scale_and_rigid_placement(sweep, scale, placed):
    part = seat(sweep).scale(scale)
    if placed:
        part = Pos(13, -7, 29) * Rot(21, 34, 17) * part
    assert part.is_valid and len(part.solids()) == 1
    document = build_section_recess_document(part)
    (record,) = seats(document.occurrences)
    assert_arc(record, 7.15 * scale, sweep, 6 * scale)
    view = build_framed_recognition_evidence(part)
    (framed,) = seats(view.result.section_recesses)
    assert_arc(framed, 7.15 * scale, sweep, 6 * scale)


def test_three_distinct_seats_keep_exact_original_faces_and_step_geometry(tmp_path):
    part = Pos(9, 0, 0) * Box(2, 46, 10)
    for y in (-20, 0, 20):
        part += Pos(0, y, 0) * seat()
    assert part.is_valid and len(part.solids()) == 1
    path = tmp_path / "seat-frame.step"
    export_step(part, path)
    for body in (part, import_step_geometry(path)):
        view = build_recognition_evidence(body)
        records = seats(view.result.section_recesses)
        assert len(records) == 3
        seen = set()
        for r in records:
            assert_arc(r, 7.15, math.radians(62.888787672), 6)
            assert len(r.evidence.defining_faces) == 1
            assert not seen.intersection(r.evidence.defining_faces)
            seen.update(r.evidence.defining_faces)
            face = body.faces()[r.evidence.defining_faces[0]]
            assert face.geom_type == GeomType.CYLINDER
            assert BRepAdaptor_Surface(face.wrapped).Cylinder().Radius() == pytest.approx(7.15)


@pytest.mark.parametrize(
    ("split_axis", "end_offset"),
    [("angular", None), ("axial", None), ("axial", 5e-6), ("axial", -5e-6), ("axial", 1e-5)],
)
@pytest.mark.parametrize("placed", [False, True])
def test_native_surface_seams_do_not_split_the_occurrence(split_axis, end_offset, placed):
    part = seat()
    face = next(f for f in part.faces() if f.geom_type == GeomType.CYLINDER)
    a = BRepAdaptor_Surface(face.wrapped)
    s = BRep_Tool.Surface_s(face.wrapped)
    if split_axis == "angular":
        edge = BRepBuilderAPI_MakeEdge(
            s.UIso((a.FirstUParameter() + a.LastUParameter()) / 2),
            a.FirstVParameter(),
            a.LastVParameter(),
        ).Edge()
    else:
        at = (a.FirstVParameter() + a.LastVParameter()) / 2
        if end_offset is not None:
            # The 5e-6 mm fragments are shorter than the seat's length tolerance.
            at = (a.FirstVParameter() if end_offset > 0 else a.LastVParameter()) + end_offset
        edge = BRepBuilderAPI_MakeEdge(
            s.VIso(at),
            a.FirstUParameter(),
            a.LastUParameter(),
        ).Edge()
    splitter = BRepFeat_SplitShape(part.wrapped)
    splitter.Add(edge, face.wrapped)
    splitter.Build()
    assert splitter.IsDone()
    split = type(part).cast(splitter.Shape())
    if placed:
        split = Pos(13, -7, 29) * Rot(21, 34, 17) * split
    assert split.is_valid and len(split.solids()) == 1
    (record,) = seats(build_section_recess_document(split).occurrences)
    walls = {i for i, f in enumerate(split.faces()) if f.geom_type == GeomType.CYLINDER}
    assert len(walls) == 2
    assert set(record.evidence.defining_faces) == walls
    assert_arc(record, 7.15, math.radians(62.888787672), 6)


@pytest.mark.parametrize(
    "part",
    [
        Box(20, 6, 20) - Cylinder(3, 30, rotation=(90, 0, 0)),
        Cylinder(7.15, 6, rotation=(90, 0, 0)),
        Box(20, 10, 10) - Pos(0, 0, 11.1) * Cylinder(7.15, 6, rotation=(90, 0, 0)),
        seat() + Pos(0, 0, 4.8) * Box(10, 1, 0.4),
        seat() - Pos(0, 0, 0) * Cylinder(1, 20),
        fillet((Box(20, 6, 10)).edges().filter_by(GeomType.LINE), radius=1),
    ],
)
def test_bores_bosses_caps_obstructions_and_broken_support_refuse(part):
    assert cylindrical_seat_proofs(FaceGraph(part)) == ()


def test_concave_corner_fillet_is_not_a_circular_channel():
    from build123d import Axis

    part = Box(20, 6, 10) - Pos(0, 0, 5) * Box(10, 8, 10)
    corners = [
        edge
        for edge in part.edges().filter_by(Axis.Y)
        if abs(edge.center().Z) < 1e-6 and abs(abs(edge.center().X) - 5) < 1e-6
    ]
    assert len(corners) == 2
    rounded = fillet(corners, radius=1)
    assert cylindrical_seat_proofs(FaceGraph(rounded)) == ()


def test_suspended_material_refuses_even_with_complete_cylindrical_support():
    part = seat()
    original_area = sum(f.area for f in part.faces() if f.geom_type == GeomType.CYLINDER)
    part += Pos(0, 0, 4.7) * Cylinder(0.1, 12, rotation=(90, 0, 0))
    part += Pos(4, 5, 4.7) * Box(8, 1, 0.2)
    part += Pos(8, 4, 0) * Box(1, 3, 9.6)
    assert part.is_valid and len(part.solids()) == 1
    wall_area = sum(
        f.area
        for f in part.faces()
        if f.geom_type == GeomType.CYLINDER
        and abs(BRepAdaptor_Surface(f.wrapped).Cylinder().Radius() - 7.15) < 1e-6
    )
    assert wall_area == pytest.approx(original_area)
    assert cylindrical_seat_proofs(FaceGraph(part)) == ()


def test_separate_bodies_keep_separate_occurrences():
    from build123d import Compound

    doc = build_section_recess_document(Compound([seat(), Pos(30, 0, 0) * seat()]))
    records = seats(doc.occurrences)
    assert len(records) == 2
    assert {r.body for r in records} == {0, 1}


def test_undercut_arc_and_spline_trims_remain_outside_the_native_contract():
    from OCP.BRepBuilderAPI import BRepBuilderAPI_NurbsConvert

    assert cylindrical_seat_proofs(FaceGraph(seat(math.radians(200)))) == ()
    part = seat()
    recovered = type(part).cast(BRepBuilderAPI_NurbsConvert(part.wrapped, True).Shape())
    assert cylindrical_seat_proofs(FaceGraph(recovered)) == ()
