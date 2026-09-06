# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Contract tests for the opt-in part-relative recognition frame spike."""

from __future__ import annotations

from pathlib import Path
from typing import cast

import pytest
from build123d import (
    Axis,
    Box,
    Compound,
    Cylinder,
    Pos,
    RegularPolygon,
    Shape,
    Sphere,
    Vector,
    extrude,
)

import quiddity._run as run_module
import quiddity.frames as frames
from quiddity._typing import CylinderInventory, Part
from quiddity.frames import (
    FramedPreparation,
    FramedRecognitionReport,
    FramedRecognitionResult,
    FrameGauge,
    FrameRefusalReason,
    PartFrame,
    PreparedFramedPart,
    RefusedPartFrame,
    build_framed_recognition_report,
    build_framed_recognition_result,
    infer_part_frame,
    prepare_framed_part,
)
from quiddity.result import (
    RecognitionResult,
    build_raw_recognition_result,
    build_recognition_result,
)
from tests.golden._common import load_fixture
from tools.frame_handling_prototype import evaluate_goldens, evaluate_translated_goldens


def test_frame_point_transforms_are_inverse() -> None:
    frame = PartFrame(
        (10.0, 20.0, 30.0),
        (0.0, 1.0, 0.0),
        (0.0, 0.0, 1.0),
        (1.0, 0.0, 0.0),
        FrameGauge.FULL,
    )

    assert frame.to_local((14.0, 25.0, 36.0)) == (5.0, 6.0, 4.0)
    assert frame.to_world((5.0, 6.0, 4.0)) == (14.0, 25.0, 36.0)


def test_frame_origin_and_axes_follow_a_rigid_motion() -> None:
    source = Box(10, 20, 30)
    source_frame = infer_part_frame(source)
    assert isinstance(source_frame, PartFrame)
    part = Pos(13, -7, 5) * source.rotate(Axis.X, 30)
    frame = infer_part_frame(part)

    assert isinstance(frame, PartFrame)
    assert frame.gauge is FrameGauge.ORTHOGONAL
    expected = Vector(*source_frame.origin).rotate(Axis.X, 30) + Vector(13, -7, 5)
    assert frame.origin == pytest.approx(tuple(expected), abs=1e-9)


def test_asymmetric_geometry_establishes_axes_that_follow_a_rigid_motion() -> None:
    source = Box(10, 20, 30) + Pos(9, 18, 28) * Box(2, 3, 4)
    source_frame = infer_part_frame(source)
    assert isinstance(source_frame, PartFrame)
    assert source_frame.gauge is FrameGauge.FULL

    frame = infer_part_frame(Pos(13, -7, 5) * source.rotate(Axis.X, 30))

    assert isinstance(frame, PartFrame)
    assert frame.gauge is FrameGauge.FULL
    for name in ("x", "y", "z"):
        expected = Vector(*getattr(source_frame, name)).rotate(Axis.X, 30)
        assert getattr(frame, name) == pytest.approx(tuple(expected), abs=1e-9)


def test_surface_of_revolution_reports_its_unobservable_roll_gauge() -> None:
    frame = infer_part_frame(Cylinder(10, 30).rotate(Axis.X, 37))

    assert isinstance(frame, PartFrame)
    assert frame.gauge is FrameGauge.AXIAL


def test_frame_inference_refuses_material_without_an_analytic_direction() -> None:
    refusal = infer_part_frame(Sphere(10))

    assert refusal == RefusedPartFrame(FrameRefusalReason.NO_ANALYTIC_DIRECTION)


def test_empty_shape_returns_the_typed_no_material_refusal() -> None:
    part = Compound(children=[])
    expected = RefusedPartFrame(FrameRefusalReason.NO_MATERIAL)

    assert infer_part_frame(part) == expected
    assert build_framed_recognition_result(part) == expected
    assert build_framed_recognition_report(part) == expected


@pytest.mark.parametrize(
    "build",
    [build_framed_recognition_result, build_framed_recognition_report],
)
def test_framed_routes_preserve_nonfinite_inference_refusal(monkeypatch, build) -> None:
    expected = RefusedPartFrame(FrameRefusalReason.NONFINITE_GEOMETRY)
    monkeypatch.setattr(frames, "infer_part_frame", lambda _part: expected)

    assert build(Box(10, 10, 10)) == expected


def test_framed_recognition_is_opt_in_and_does_not_mutate_legacy_behavior() -> None:
    fixture = load_fixture(Path("tests/golden/straight_and_obround_slots/fixture.py"))
    part = fixture.build_fixture()
    legacy_before = build_recognition_result(part)

    framed = build_framed_recognition_result(Pos(13, -7, 5) * part.rotate(Axis.X, 30))

    assert isinstance(framed, FramedRecognitionResult)
    assert len(framed.result.slots) == len(legacy_before.slots) == 5
    assert not any(
        r.classification.feature_kind == "passage" for r in framed.result.section_recesses
    )
    assert build_recognition_result(part) == legacy_before


@pytest.mark.parametrize(
    "part",
    [
        extrude(RegularPolygon(20, 6), 30),
        Pos(9, -13, 7) * extrude(RegularPolygon(20, 6), 30).rotate(Axis.X, 30),
        Pos(-4, 8, 11) * extrude(RegularPolygon(20, 6), 30).rotate(Axis((0, 0, 0), (1, 1, 0)), 37),
    ],
)
def test_framed_polygonal_stock_survives_rigid_presentation(part) -> None:
    framed = build_framed_recognition_result(part)

    assert isinstance(framed, FramedRecognitionResult)
    assert framed.frame.gauge is FrameGauge.ORTHOGONAL
    assert len(framed.result.polygonal_stock) == 1
    record = framed.result.polygonal_stock[0]
    assert record.side_count == 6
    assert record.axis in {"x", "y", "z"}
    assert record.length == pytest.approx(30.0)


def test_framed_polygonal_stock_record_is_exactly_rigid_motion_invariant() -> None:
    source = extrude(RegularPolygon(20, 6), 30)
    presentations = (
        source,
        source.rotate(Axis.Z, 17),
        Pos(9, -13, 7) * source.rotate(Axis.X, 30),
        Pos(-4, 8, 11) * source.rotate(Axis((0, 0, 0), (1, 1, 0)), 37),
    )

    framed = [build_framed_recognition_result(part) for part in presentations]

    assert all(isinstance(result, FramedRecognitionResult) for result in framed)
    records = [
        cast(FramedRecognitionResult, result).result.polygonal_stock[0].to_dict()
        for result in framed
    ]
    assert records == [records[0]] * len(records)


def test_framed_result_exposes_the_exact_shape_recognised(monkeypatch) -> None:
    part = Pos(13, -7, 5) * Box(10, 20, 30).rotate(Axis.X, 30)
    recognised_parts: list[Shape] = []
    original = frames.build_raw_recognition_result

    def capture(
        working_part: Shape,
        *,
        cylinders: CylinderInventory | None = None,
        rotational: bool = False,
    ) -> RecognitionResult:
        recognised_parts.append(working_part)
        return original(
            cast(Part, working_part),
            cylinders=cylinders,
            rotational=rotational,
        )

    monkeypatch.setattr(frames, "build_raw_recognition_result", capture)

    framed = build_framed_recognition_result(part)

    assert isinstance(framed, FramedRecognitionResult)
    assert framed.part is recognised_parts[0]


def _stepped_shaft() -> Shape:
    return Cylinder(10, 30) + Pos(0, 0, 30) * Cylinder(7, 10)


@pytest.mark.parametrize(
    ("source", "gauge"),
    [
        (
            Box(10, 20, 30) + Pos(9, 18, 28) * Box(2, 3, 4) - Pos(3, 4, 0) * Cylinder(1, 30),
            FrameGauge.FULL,
        ),
        (Box(10, 20, 30), FrameGauge.ORTHOGONAL),
        (_stepped_shaft(), FrameGauge.AXIAL),
    ],
)
@pytest.mark.parametrize("rotational", [False, True])
def test_prepared_full_orthogonal_and_axial_parts_preserve_local_classification(
    source: Shape,
    gauge: FrameGauge,
    rotational: bool,
) -> None:
    moved = Pos(13, -7, 5) * source.rotate(Axis((0, 0, 0), (1, 1, 0)), 37)
    prepared = prepare_framed_part(cast(Part, moved))

    assert isinstance(prepared, PreparedFramedPart)
    assert prepared.frame.gauge is gauge
    framed = prepared.recognise(rotational=rotational)
    direct = build_recognition_result(
        cast(Part, prepared.part),
        cylinders=(list(prepared.cylinders[0]), list(prepared.cylinders[1])),
        rotational=rotational,
    )

    assert framed.frame is prepared.frame
    assert framed.part is prepared.part
    assert framed.result == direct
    assert framed.result.rotational is rotational


def test_preparation_scans_cylinders_once_before_the_single_aggregate(monkeypatch) -> None:
    moved = Pos(13, -7, 5) * _stepped_shaft().rotate(Axis.X, 37)
    original = frames.analyse_cylinders
    calls: list[Part] = []

    def counted(part: Part):
        calls.append(part)
        return original(part)

    monkeypatch.setattr(frames, "analyse_cylinders", counted)

    prepared: FramedPreparation = prepare_framed_part(cast(Part, moved))
    assert isinstance(prepared, PreparedFramedPart)

    def forbidden_rescan(*_args, **_kwargs):
        raise AssertionError("prepared aggregate must not rescan cylinders")

    aggregate = frames.build_raw_recognition_result
    aggregate_calls: list[tuple[Part, CylinderInventory, bool]] = []

    def counted_aggregate(
        part: Part,
        *,
        cylinders: CylinderInventory | None = None,
        rotational: bool = False,
    ) -> RecognitionResult:
        assert cylinders is not None
        aggregate_calls.append((part, cylinders, rotational))
        return aggregate(part, cylinders=cylinders, rotational=rotational)

    monkeypatch.setattr(run_module, "analyse_cylinders", forbidden_rescan)
    monkeypatch.setattr(frames, "build_raw_recognition_result", counted_aggregate)
    # A consumer can inspect the exact local substrate before choosing its own policy.
    rotational = any(evidence["external"] for group in prepared.cylinders for evidence in group)
    framed = prepared.recognise(rotational=rotational)

    assert calls == [prepared.part]
    assert len(aggregate_calls) == 1
    aggregate_part, aggregate_cylinders, aggregate_rotational = aggregate_calls[0]
    assert aggregate_part is prepared.part
    assert all(
        actual is expected
        for actual, expected in zip(aggregate_cylinders[0], prepared.cylinders[0], strict=True)
    )
    assert all(
        actual is expected
        for actual, expected in zip(aggregate_cylinders[1], prepared.cylinders[1], strict=True)
    )
    assert aggregate_rotational is True
    assert framed.result.rotational is True
    assert framed.result.cylinders == prepared.cylinders


def test_preparation_refusal_allows_an_explicit_legacy_fallback() -> None:
    part = Sphere(10)

    prepared = prepare_framed_part(part)

    assert prepared == RefusedPartFrame(FrameRefusalReason.NO_ANALYTIC_DIRECTION)
    assert isinstance(build_recognition_result(part), RecognitionResult)
    assert build_raw_recognition_result(part) == build_recognition_result(part)


def test_framed_report_pairs_one_local_run_with_the_exact_working_shape() -> None:
    part = Pos(13, -7, 5) * _stepped_shaft().rotate(Axis.X, 37)

    framed = build_framed_recognition_report(cast(Part, part), rotational=True)

    assert isinstance(framed, FramedRecognitionReport)
    assert framed.report.result.rotational is True
    assert framed.report.result.cylinders
    assert all(
        evidence["face"] in framed.part.faces()
        for group in framed.report.result.cylinders
        for evidence in group
    )


def test_framed_report_preserves_typed_frame_refusal() -> None:
    assert build_framed_recognition_report(Sphere(10)) == RefusedPartFrame(
        FrameRefusalReason.NO_ANALYTIC_DIRECTION
    )


@pytest.mark.parametrize(
    ("source", "gauge", "topology_expected"),
    [
        (
            Box(10, 20, 30) + Pos(9, 18, 28) * Box(2, 3, 4) - Pos(3, 4, 0) * Cylinder(1, 30),
            FrameGauge.FULL,
            True,
        ),
        (Box(10, 20, 30), FrameGauge.ORTHOGONAL, False),
        (Cylinder(10, 30), FrameGauge.AXIAL, True),
    ],
)
def test_framed_working_shape_uses_the_published_local_coordinates(
    source: Shape, gauge: FrameGauge, topology_expected: bool
) -> None:
    part = Pos(13, -7, 5) * source.rotate(Axis.X, 30)

    framed = build_framed_recognition_result(part)

    assert isinstance(framed, FramedRecognitionResult)
    assert framed.frame.gauge is gauge
    expected = sorted(
        tuple(round(value, 8) for value in framed.frame.to_local((vertex.X, vertex.Y, vertex.Z)))
        for vertex in part.vertices()
    )
    actual = sorted(
        tuple(round(value, 8) for value in (vertex.X, vertex.Y, vertex.Z))
        for vertex in framed.part.vertices()
    )
    assert actual == expected
    cylinder_evidence = framed.result.cylinders[0] + framed.result.cylinders[1]
    assert bool(cylinder_evidence) is topology_expected
    assert all(evidence["face"] in framed.part.faces() for evidence in cylinder_evidence)


def test_normalization_makes_the_complete_golden_inventory_rotation_invariant() -> None:
    report = evaluate_goldens()

    assert report["refused"] == {}
    assert report["totals"] == {
        name: {
            "baseline_records": 102,
            "same_family": 102,
            "reclassified": 0,
            "absent": 0,
            "introduced": 0,
        }
        for name in ("Z30", "X30", "X90")
    }


def test_normalization_makes_the_complete_golden_inventory_translation_invariant() -> None:
    report = evaluate_translated_goldens()

    assert report["refused"] == {}
    assert report["totals"] == {
        name: {
            "baseline_records": 102,
            "same_family": 102,
            "reclassified": 0,
            "absent": 0,
            "introduced": 0,
        }
        for name in ("T", "X30+T")
    }
