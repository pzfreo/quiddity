"""Native freeform support, continuity and sampled shell offsets for #759."""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from build123d import Cylinder, Face, Solid
from OCP.BRep import BRep_Tool
from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeFace, BRepBuilderAPI_NurbsConvert
from OCP.Geom import Geom_BSplineSurface
from OCP.gp import gp_Pnt
from OCP.TColgp import TColgp_Array2OfPnt
from OCP.TColStd import (
    TColStd_Array1OfInteger,
    TColStd_Array1OfReal,
    TColStd_Array2OfReal,
)

from quiddity import (
    build_recognition_result,
    import_step_geometry,
    recognise_freeform_surfaces,
    recognise_thin_wall_bodies,
)
from quiddity.freeform_surfaces import _construction, _support


def _array1(values, cls):
    result = cls(1, len(values))
    for index, value in enumerate(values, 1):
        result.SetValue(index, value)
    return result


def _rebuild(support):
    nu, nv = len(support.poles), len(support.poles[0])
    poles = TColgp_Array2OfPnt(1, nu, 1, nv)
    weights = TColStd_Array2OfReal(1, nu, 1, nv)
    for u, row in enumerate(support.poles, 1):
        for v, point in enumerate(row, 1):
            poles.SetValue(u, v, gp_Pnt(*point))
            weights.SetValue(u, v, support.weights[u - 1][v - 1])
    return Geom_BSplineSurface(
        poles,
        weights,
        _array1(support.u_knots, TColStd_Array1OfReal),
        _array1(support.v_knots, TColStd_Array1OfReal),
        _array1(support.u_multiplicities, TColStd_Array1OfInteger),
        _array1(support.v_multiplicities, TColStd_Array1OfInteger),
        support.u_degree,
        support.v_degree,
        support.u_periodic,
        support.v_periodic,
    )


def test_unclaimed_native_bspline_solid_has_full_support_and_entrypoint_parity():
    part = Solid(BRepBuilderAPI_NurbsConvert(Cylinder(8, 12).wrapped, True).Shape())
    records = recognise_freeform_surfaces(part)
    assert tuple(records) == build_recognition_result(part).freeform_surfaces
    record = next(item for item in records if item.construction_vector == (0, 0, 12))
    assert record.continuity_group == (record.face,)
    assert record.offset_partner is None
    assert record.construction_kind == "linear_extrusion"
    assert record.construction_axis == "v"
    assert record.construction_vector == pytest.approx((0, 0, 12))
    assert record.support_kind == "bspline"
    assert (
        _rebuild(record.support)
        .Value(0.3, 2)
        .Distance(BRep_Tool.Surface_s(part.faces()[record.face].wrapped).Value(0.3, 2))
        < 1e-6
    )


def test_two_nontranslated_sections_report_ruled_construction():
    poles = TColgp_Array2OfPnt(1, 2, 1, 4)
    for v, z in enumerate((0, 2, 1, 0), 1):
        poles.SetValue(1, v, gp_Pnt(0, v * 5, 0))
        poles.SetValue(2, v, gp_Pnt(10, v * 5, z))
    native = Geom_BSplineSurface(
        poles,
        _array1((0.0, 1.0), TColStd_Array1OfReal),
        _array1((0.0, 1.0), TColStd_Array1OfReal),
        _array1((2, 2), TColStd_Array1OfInteger),
        _array1((4, 4), TColStd_Array1OfInteger),
        1,
        3,
    )
    face = Face(BRepBuilderAPI_MakeFace(native, 0, 1, 0, 1, 1e-7).Face())
    support = _support(BRep_Tool.Surface_s(face.wrapped))
    assert _construction(support) == ("ruled", "u", None)
    assert recognise_freeform_surfaces(face) == []
    assert build_recognition_result(face).freeform_surfaces == ()
    assert _rebuild(support).Value(0.4, 0.6).Distance(native.Value(0.4, 0.6)) < 1e-6


@pytest.mark.slow
def test_cgb241_outer_skins_have_rebuildable_support_and_inner_offset_links():
    expected = json.loads(Path(__file__).with_name("freeform_surfaces_expected.json").read_text())
    part = import_step_geometry(
        Path(__file__).parent / "corpus" / "cadgenbench_inputs" / "cgb241.step"
    )
    faces = tuple(part.faces())
    records = {record.face: record for record in recognise_freeform_surfaces(part)}
    assert len(records) == expected["native_bspline_faces"]
    aggregate = build_recognition_result(part)
    assert {record.face: record for record in aggregate.freeform_surfaces} == records
    (wall,) = recognise_thin_wall_bodies(part)
    outer = [index for index in wall.history_hint.outer_faces if index in records]
    large_outer = sorted(outer, key=lambda index: -faces[index].area)[
        : expected["large_outer_patches"]
    ]
    assert len(large_outer) == expected["large_outer_patches"]
    assert (
        sorted(len(group) for group in {records[index].continuity_group for index in outer})
        == (expected["outer_bspline_group_sizes"])
    )
    mates = {
        face: other
        for pair in wall.face_pairs
        for face, other in (
            (pair.first_face, pair.second_face),
            (pair.second_face, pair.first_face),
        )
    }
    assert all(mates[index] in records for index in large_outer)
    for index in large_outer:
        outer = records[index]
        inner = records[mates[index]]
        assert outer.support_kind == inner.support_kind == "bspline"
        assert outer.construction_kind is None
        assert outer.offset_partner == inner.face
        assert inner.offset_partner == outer.face
        assert outer.offset_distance == pytest.approx(expected["measured_wall_offset_mm"], abs=1e-3)
        assert inner.offset_distance == pytest.approx(outer.offset_distance)
        assert outer.offset_basis == inner.offset_basis == "reciprocal_material_rays"
        native = BRep_Tool.Surface_s(faces[index].wrapped)
        rebuilt = _rebuild(outer.support)
        for u, v in ((0.2, 0.3), (0.5, 0.5), (0.8, 0.7)):
            assert rebuilt.Value(u, v).Distance(native.Value(u, v)) < 1e-6
        assert index in outer.continuity_group
        assert all(link.other_face in outer.continuity_group for link in outer.continuity_links)
    json.dumps([record.to_dict() for record in records.values()], allow_nan=False)
