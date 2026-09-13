"""#593: exterior air does not join bore occurrences in one connected body."""

from __future__ import annotations

import pytest
from build123d import Box, Cylinder, GeomType, Pos, Rot, chamfer, export_step
from OCP.BRep import BRep_Tool
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeEdge, BRepBuilderAPI_NurbsConvert
from OCP.BRepFeat import BRepFeat_SplitShape

from quiddity import (
    FramedRecognitionEvidence,
    analyse_cylinders,
    build_framed_recognition_evidence,
    import_step_geometry,
    recognise_holes,
)
from quiddity.evidence import build_recognition_evidence


def _lug_frame(count=6, gap=5.1, *, chamfered=False):
    """Construction authority: equal bored lugs joined by an off-axis spine."""
    thickness = 3.2 if chamfered else 2.7
    pitch = thickness + gap
    span = (count - 1) * pitch + thickness
    part = Pos(4, 0, (count - 1) * pitch / 2) * Box(2, 6, span)
    for i in range(count):
        part += Pos(0, 0, i * pitch) * Box(10, 6, thickness)
    for i in range(count):
        part -= Pos(0, 0, i * pitch) * Cylinder(0.55, thickness + 1)
    if chamfered:
        part = chamfer(part.edges().filter_by(GeomType.CIRCLE), length=0.25)
    assert len(part.solids()) == 1 and part.is_valid
    return part


@pytest.mark.parametrize("count,gap", [(2, 0.1), (3, 1), (6, 5.1), (6, 30)])
@pytest.mark.parametrize("chamfered", [False, True])
def test_lugs_are_separate_despite_one_owner_and_a_void_centreline(count, gap, chamfered):
    part = _lug_frame(count, gap, chamfered=chamfered)
    holes = recognise_holes(part)
    assert len(holes) == count
    assert [h.diameter for h in holes] == pytest.approx([1.1] * count)
    assert [h.depth for h in holes] == pytest.approx([2.7] * count)
    assert {h.bottom for h in holes} == {"through"}
    assert {h.axis for h in holes} == {(0.0, 0.0, -1.0)}
    assert len({h.location for h in holes}) == count


@pytest.mark.parametrize("scale", [0.05, 1, 100])
def test_separate_occurrences_survive_placement_and_framed_evidence(scale):
    part = Pos(13, -7, 29) * Rot(21, 34, 17) * _lug_frame(chamfered=True).scale(scale)
    framed = build_framed_recognition_evidence(part)
    assert isinstance(framed, FramedRecognitionEvidence)
    holes = framed.result.holes
    assert len(holes) == 6
    assert [h.diameter / scale for h in holes] == pytest.approx([1.1] * 6)
    # Hole depths retain their existing two-decimal publication contract; at
    # 0.05x the exact 0.135 depth lies on a rounding tie after rigid placement.
    assert [h.depth for h in holes] == pytest.approx([2.7 * scale] * 6, abs=0.005 + 1e-9)
    assert {h.bottom for h in holes} == {"through"}


def test_step_round_trip_and_exact_disjoint_occurrence_evidence(tmp_path):
    path = tmp_path / "lug-frame.step"
    export_step(_lug_frame(chamfered=True), path)
    part = import_step_geometry(path)
    view = build_recognition_evidence(part)
    refs = [ref for ref in view.features if view.family(ref) == "holes"]
    assert len(refs) == 6
    owned = []
    for ref in refs:
        faces = [view.face(face) for face in view.defining_faces(ref)]
        assert len(faces) == 1
        assert faces[0].geom_type == GeomType.CYLINDER
        assert all(not faces[0].is_same(previous) for previous in owned)
        assert any(faces[0].is_same(original) for original in part.faces())
        owned.extend(faces)
        record = view.record(ref)
        assert record.depth == pytest.approx(2.7)
        assert record.diameter == pytest.approx(1.1)


@pytest.mark.parametrize("crossings", [1, 2])
def test_real_cross_drillings_still_join_the_interrupted_bore(crossings):
    part = Box(60, 60, 40) - Cylinder(3, 60, rotation=(0, 90, 0))
    for x in [0] if crossings == 1 else [-12, 12]:
        part -= Pos(x, 0, 0) * Cylinder(5, 40)
    view = build_recognition_evidence(part)
    holes = view.result.holes
    assert len(holes) == crossings + 1
    (cross,) = [h for h in holes if h.diameter == pytest.approx(6)]
    assert cross.depth == pytest.approx(60)
    assert cross.bottom == "through"


@pytest.mark.parametrize("angle,offset", [(30, 0), (45, 0), (45, 2), (60, -2)])
@pytest.mark.parametrize("placed", [False, True])
@pytest.mark.parametrize("scale", [0.05, 1, 100])
def test_oblique_cross_drilling_preserves_the_complete_bore(angle, offset, placed, scale):
    part = (
        Box(60, 60, 60)
        - Cylinder(3, 100, rotation=(0, 90, 0))
        - Pos(0, offset, 0) * Cylinder(4, 160, rotation=(0, angle, 0))
    ).scale(scale)
    if placed:
        part = Pos(13, -7, 29) * Rot(21, 34, 17) * part
    assert len(part.solids()) == 1 and part.is_valid
    framed = build_framed_recognition_evidence(part)
    assert isinstance(framed, FramedRecognitionEvidence)
    for holes in (recognise_holes(part), framed.result.holes):
        (bore,) = [h for h in holes if h.diameter == pytest.approx(6 * scale)]
        assert bore.depth == pytest.approx(60 * scale)
        assert bore.bottom == "through"


@pytest.mark.parametrize("placed", [False, True])
def test_interruption_containment_allows_inventory_rounding(placed):
    part = Box(200, 200, 200) - Cylinder(0.05, 220, rotation=(0, 90, 0)) - Cylinder(50.00004, 220)
    if placed:
        part = Pos(13, -7, 29) * Rot(21, 34, 17) * part
    assert len(part.solids()) == 1 and part.is_valid
    framed = build_framed_recognition_evidence(part)
    assert isinstance(framed, FramedRecognitionEvidence)
    for holes in (recognise_holes(part), framed.result.holes):
        (bore,) = [h for h in holes if h.diameter == pytest.approx(0.1)]
        assert bore.depth == pytest.approx(200)
        assert bore.bottom == "through"


@pytest.mark.parametrize("placed", [False, True])
def test_lateral_cavity_contact_does_not_bridge_exterior_air(placed):
    tube = Pos(10, 0, 0) * Cylinder(9, 40)
    lugs = Pos(0, 0, -18) * Box(10, 12, 4) + Pos(0, 0, 18) * Box(10, 12, 4)
    part = (tube + lugs) - Pos(10, 0, 0) * Cylinder(8, 50) - Cylinder(3, 50)
    # The middle slab opens the gap completely. Both bore lands still share
    # side edges with the long cavity, whose interior excludes their axis.
    part -= Pos(-7, 0, 0) * Box(20, 40, 32)
    if placed:
        part = Pos(13, -7, 29) * Rot(21, 34, 17) * part
    assert len(part.solids()) == 1 and part.is_valid
    framed = build_framed_recognition_evidence(part)
    assert isinstance(framed, FramedRecognitionEvidence)
    for holes in (recognise_holes(part), framed.result.holes):
        bores = [h for h in holes if h.diameter == pytest.approx(6)]
        assert len(bores) == 2
        assert [h.depth for h in bores] == pytest.approx([4, 4])
        assert {h.bottom for h in bores} == {"through"}


def test_internal_cross_drilling_does_not_bridge_a_later_exterior_gap():
    left = Box(24, 24, 20) - Cylinder(2, 20) - Cylinder(4, 24, rotation=(0, 90, 0))
    right = Pos(0, 0, 30) * (Box(24, 24, 6) - Cylinder(2, 6))
    spine = Pos(11, 0, 11.5) * Box(2, 24, 43)
    part = left + spine + right
    assert len(part.solids()) == 1
    axial = [h for h in recognise_holes(part) if h.diameter == pytest.approx(4)]
    assert len(axial) == 2
    assert sorted(h.depth for h in axial) == pytest.approx([6, 20])


@pytest.mark.parametrize("recovered", [False, True])
def test_crossing_cylinder_split_into_patches_preserves_the_interruption(recovered):
    part = Box(60, 60, 40) - Cylinder(5, 40) - Cylinder(3, 60, rotation=(0, 90, 0))
    cylinder = next(
        face
        for face in part.faces()
        if face.geom_type == GeomType.CYLINDER
        and BRepAdaptor_Surface(face.wrapped).Cylinder().Radius() == pytest.approx(5)
    )
    surface = BRep_Tool.Surface_s(cylinder.wrapped)
    adaptor = BRepAdaptor_Surface(cylinder.wrapped)
    # Split between the two cross-bore mouths. Each side then meets a different
    # original patch of the same crossing cylinder; raw face equality is too strict.
    edge = BRepBuilderAPI_MakeEdge(
        surface.UIso(1.0), adaptor.FirstVParameter(), adaptor.LastVParameter()
    ).Edge()
    splitter = BRepFeat_SplitShape(part.wrapped)
    splitter.Add(edge, cylinder.wrapped)
    splitter.Build()
    assert splitter.IsDone()
    split = type(part).cast(splitter.Shape())
    assert len(split.faces()) > len(part.faces())
    if recovered:
        split = type(part).cast(BRepBuilderAPI_NurbsConvert(split.wrapped, True).Shape())
    expected = recognise_holes(part)
    assert recognise_holes(split) == expected
    inventory = analyse_cylinders(split)
    reversed_inventory = tuple(list(reversed(group)) for group in inventory)
    assert recognise_holes(split, cyls=reversed_inventory) == expected
