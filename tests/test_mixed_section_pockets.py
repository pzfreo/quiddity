from __future__ import annotations

import math

import pytest
from build123d import (
    Box,
    Compound,
    Cylinder,
    Pos,
    RectangleRounded,
    Rot,
    export_step,
    extrude,
    import_step,
)

from quiddity import build_section_recess_document
from quiddity._sections import PlanarSection, SectionVertex


def _pocket(scale=1.0):
    stock = Pos(0, 0, 7 * scale) * Box(40 * scale, 30 * scale, 24 * scale)
    tool = extrude(RectangleRounded(13.6 * scale, 7.9 * scale, 3.94 * scale), 19 * scale)
    return stock - tool


@pytest.mark.parametrize("scale", [0.1, 1, 10])
@pytest.mark.parametrize("rotation", [Rot(), Rot(180, 0, 0), Rot(17, 31, 43)])
def test_mixed_pocket_keeps_short_end_segments_and_exact_area(scale, rotation):
    document = build_section_recess_document(Pos(3, 7, 11) * rotation * _pocket(scale))
    assert len(document.occurrences) == 1
    record = document.occurrences[0]
    assert record.classification.section_shape == "general"
    assert record.classification.feature_kind == "pocket"
    boundary = record.geometry.profile.boundary
    assert len(boundary) == 8
    assert sum(v.bulge != 0 for v in boundary) == 4
    area = PlanarSection(tuple(SectionVertex(v.point, v.bulge) for v in boundary)).area
    expected = (13.6 * 7.9 - (4 - math.pi) * 3.94**2) * scale**2
    assert area == pytest.approx(expected, abs=0.01 * scale)
    lengths = sorted(
        math.dist(v.point, boundary[(i + 1) % 8].point)
        for i, v in enumerate(boundary)
        if v.bulge == 0
    )
    assert lengths == pytest.approx([0.02 * scale] * 2 + [5.72 * scale] * 2, abs=0.002)
    assert record.geometry.run_interval[1] - record.geometry.run_interval[0] == pytest.approx(
        19 * scale, abs=0.002
    )
    assert len(record.evidence.defining_faces) == 8
    assert len(record.evidence.constituent_faces) == 9
    assert not document.refusals


@pytest.mark.parametrize("change", ["floor_hole", "wall_hole", "bridge", "mouth_bridge"])
def test_mixed_pocket_refuses_unexplained_missing_support_and_obstructions(change):
    part = _pocket()
    if change == "floor_hole":
        part -= Pos(0, 0, -3) * Cylinder(1, 10)
    elif change == "wall_hole":
        part -= Pos(0, 0, 8) * Rot(90, 0, 0) * Cylinder(1, 40)
    elif change == "bridge":
        part += Pos(0, 0, 8) * Box(1, 12, 1)
    else:
        part += Pos(0, 0, 19.5) * Box(1, 12, 1)
    document = build_section_recess_document(part)
    assert not any(r.classification.section_shape == "general" for r in document.occurrences)


def test_equal_mixed_pockets_on_separate_bodies_keep_distinct_evidence():
    document = build_section_recess_document(Compound([_pocket(), Pos(60, 0, 0) * _pocket()]))
    assert len(document.occurrences) == 2
    first, second = document.occurrences
    assert first.body != second.body
    assert set(first.evidence.constituent_faces).isdisjoint(second.evidence.constituent_faces)


def test_mixed_pocket_step_round_trip_preserves_general_profile(tmp_path):
    path = tmp_path / "rounded-pocket.step"
    part = Pos(3, 7, 11) * Rot(17, 31, 43) * _pocket()
    before = build_section_recess_document(part)
    export_step(part, path)
    after = build_section_recess_document(import_step(path))
    assert len(after.occurrences) == 1
    assert after.occurrences[0].geometry == before.occurrences[0].geometry
    assert after.occurrences[0].classification == before.occurrences[0].classification


def test_mixed_pocket_kernel_failure_refuses(monkeypatch):
    import quiddity._section_recess_geometry as implementation

    def fail(*args):
        raise RuntimeError("kernel probe failed")

    monkeypatch.setattr(implementation, "_covered_patch", fail)
    assert not build_section_recess_document(_pocket()).occurrences
