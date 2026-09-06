from __future__ import annotations

import math

import pytest
from build123d import (
    Box,
    Compound,
    Cylinder,
    Pos,
    RegularPolygon,
    Rot,
    chamfer,
    export_step,
    extrude,
    import_step,
)

from quiddity import build_section_recess_document


def _base(sides=6, scale=1.0):
    return Pos(0, 0, 10 * scale) * Box(40 * scale, 40 * scale, 20 * scale) - Pos(
        0, 0, -5 * scale
    ) * extrude(RegularPolygon(8 * scale, sides), 30 * scale)


def _treated(sides=6, scale=1.0):
    base = _base(sides, scale)
    edge = next(
        edge
        for edge in base.edges()
        if abs(edge.center().Z - 20 * scale) < 1e-6 and edge.length < 20 * scale
    )
    return chamfer([edge], 0.1 * scale)


@pytest.mark.parametrize("sides", [3, 4, 6])
@pytest.mark.parametrize("scale", [0.1, 1, 10])
@pytest.mark.parametrize("rotation", [Rot(), Rot(17, 31, 43), Rot(180, 0, 0)])
def test_entry_treatment_preserves_base_section_and_original_evidence(sides, scale, rotation):
    part = Pos(3, 7, 11) * rotation * _treated(sides, scale)
    document = build_section_recess_document(part)
    passages = [r for r in document.occurrences if r.classification.feature_kind == "passage"]
    assert len(passages) == 1
    record = passages[0]
    assert len(record.evidence.defining_faces) == sides
    assert len(record.evidence.constituent_faces) == sides + 1
    assert set(record.evidence.defining_faces) < set(record.evidence.constituent_faces)
    assert len(record.geometry.profile.boundary) == sides
    assert record.geometry.run_interval[1] - record.geometry.run_interval[0] == pytest.approx(
        20 * scale, abs=0.002
    )
    points = [v.point for v in record.geometry.profile.boundary]
    area = (
        abs(
            sum(
                a[0] * b[1] - a[1] * b[0]
                for a, b in zip(points, points[1:] + points[:1], strict=True)
            )
        )
        / 2
    )
    assert area == pytest.approx(
        sides * 32 * scale**2 * math.sin(2 * math.pi / sides), abs=0.02 * scale
    )


@pytest.mark.parametrize("kind", ["step", "chamfer_step", "cross_hole", "bridge", "mouth_bridge"])
def test_treatment_does_not_excuse_unrelated_missing_support_or_material(kind):
    part = _treated()
    if kind == "step":
        part = _base() - Pos(10, 0, 19.95) * Box(20, 40, 0.1)
    elif kind == "chamfer_step":
        part -= Pos(10, 0, 17.5) * Box(20, 40, 5)
    elif kind == "cross_hole":
        part -= Pos(0, 0, 10) * Rot(90, 0, 0) * Cylinder(1, 50)
    elif kind == "bridge":
        part += Pos(0, 0, 10) * Box(1, 30, 1)
    else:
        part += Pos(0, 0, 20.5) * Box(1, 30, 1)
    assert not any(
        r.classification.feature_kind == "passage"
        for r in build_section_recess_document(part).occurrences
    )


def test_treated_passage_compound_ownership_and_step_round_trip(tmp_path):
    part = Compound([_treated(), Pos(60, 0, 0) * _treated()])
    before = build_section_recess_document(part)
    assert len(before.occurrences) == 2
    first, second = before.occurrences
    assert first.body != second.body
    assert set(first.evidence.constituent_faces).isdisjoint(second.evidence.constituent_faces)
    path = tmp_path / "treated.step"
    export_step(part, path)
    after = build_section_recess_document(import_step(path))
    assert [r.geometry for r in after.occurrences] == [r.geometry for r in before.occurrences]


def test_treatment_kernel_failure_refuses(monkeypatch):
    import quiddity._entry_treatments as implementation

    def fail(*args):
        raise RuntimeError("kernel treatment cell failed")

    monkeypatch.setattr(implementation, "_cell_supports", fail)
    assert not any(
        r.classification.feature_kind == "passage"
        for r in build_section_recess_document(_treated()).occurrences
    )
