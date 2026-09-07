"""A wall's blend tangent is not a physical planar pocket floor (#556)."""

import pytest
from build123d import Box, Compound, Pos, Rot, export_step, fillet

from quiddity import build_raw_recognition_result, import_step_geometry
from quiddity.evidence import build_recognition_evidence


def _pocket(scale=1.0, *, treated=False):
    part = Box(80 * scale, 60 * scale, 20 * scale) - Pos(0, 0, 10 * scale) * Box(
        30 * scale, 20 * scale, 20 * scale
    )
    if treated:
        edges = [
            edge
            for edge in part.edges()
            if abs(edge.bounding_box().min.Z) < 1e-6
            and abs(edge.bounding_box().max.Z) < 1e-6
            and abs(edge.center().X) < 20 * scale
            and abs(edge.center().Y) < 15 * scale
        ]
        assert len(edges) == 4
        part = fillet(edges, 4 * scale)
    return part


@pytest.mark.parametrize("scale", [0.1, 1, 10])
@pytest.mark.parametrize("rotation", [Rot(), Rot(90, 0, 0), Rot(0, 90, 0), Rot(180, 0, 0)])
def test_floor_blend_refuses_phantom_cap_and_preserves_blends(scale, rotation):
    placement = Pos(123.0004, -57.0004, 91.0004) * rotation
    plain = build_raw_recognition_result(placement * _pocket(scale))
    (pocket,) = plain.section_recesses
    assert pocket.geometry.run_interval[1] - pocket.geometry.run_interval[0] == pytest.approx(
        10 * scale, abs=0.002
    )
    assert not plain.section_recess_refusals

    part = placement * _pocket(scale, treated=True)
    result = build_raw_recognition_result(part)
    assert not result.section_recesses
    assert result.section_recess_refusals
    assert all(r.reason == "unsupported_support_geometry" for r in result.section_recess_refusals)
    assert len(result.blends) == 4
    assert all(r.evidence.defining_faces for r in result.section_recess_refusals)


def test_refused_floor_preserves_other_body_and_run_local_evidence():
    part = Compound([_pocket(treated=True), Pos(100, 0, 0) * _pocket()])
    result = build_raw_recognition_result(part)
    (pocket,) = result.section_recesses
    assert pocket.index == 0
    assert all(r.body != pocket.body for r in result.section_recess_refusals)
    assert len(result.blends) == 4
    view = build_recognition_evidence(part)
    for ref in view.features:
        assert all(view.face(face) is not None for face in view.defining_faces(ref))


def test_floor_refusal_survives_step_round_trip(tmp_path):
    path = tmp_path / "floor-blend.step"
    assert export_step(_pocket(treated=True), path)
    result = build_raw_recognition_result(import_step_geometry(path))
    assert not result.section_recesses
    assert result.section_recess_refusals
    assert len(result.blends) == 4


def test_mouth_blend_does_not_hide_a_physical_planar_floor():
    from tests.test_prismatic_pockets import _hexagonal, _with_one_treated_mouth_edge

    result = build_raw_recognition_result(_with_one_treated_mouth_edge(_hexagonal(), fillet))
    (pocket,) = result.section_recesses
    assert pocket.geometry.run_interval == (2, 10)
    assert pocket.geometry.ends.low.condition == "capped"
