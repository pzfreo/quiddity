# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

from __future__ import annotations

import json
import math
from dataclasses import replace
from pathlib import Path

import pytest
from build123d import (
    Align,
    Axis,
    Box,
    Compound,
    Cone,
    Cylinder,
    Pos,
    Rot,
    Solid,
    export_step,
    import_step,
)

from quiddity import (
    feature_census,
    recognise_oriented_slot_patterns,
    recognise_oriented_slots,
)
from quiddity._candidates import FamilyId
from quiddity._dispositions import Outcome, ReasonCode
from quiddity.frames import FramedRecognitionResult, build_framed_recognition_result
from quiddity.result import _take_inventory
from tools._legacy_recognition import (
    build_recognition_result,
    recognise_section_passages,
)


def _rectangular_through_slot(angle: float = 30.0):
    tool = Rot(0, 0, angle) * Box(
        30,
        8,
        20,
        align=(Align.CENTER, Align.CENTER, Align.CENTER),
    )
    return Box(100, 70, 10) - tool


def _oriented_slot_pattern(points, *, angle: float = 30.0):
    part = Box(120, 90, 10)
    for x, y in points:
        part -= (
            Pos(x, y, 0)
            * Rot(0, 0, angle)
            * Box(24, 6, 20, align=(Align.CENTER, Align.CENTER, Align.CENTER))
        )
    return part


@pytest.mark.parametrize("angle", [17.0, 30.0, 45.0])
def test_free_axis_rectangle_is_an_oriented_slot(angle: float) -> None:
    part = _rectangular_through_slot(angle)

    (record,) = recognise_oriented_slots(part)

    assert record.width == pytest.approx(8.0, abs=0.002)
    assert record.length == pytest.approx(30.0, abs=0.002)
    assert record.depth == pytest.approx(10.0)
    assert record.center == (0.0, 0.0, 0.0)
    # Public section vertices are serialized to 0.001 model units before this projection.
    assert (
        abs(sum(a * b for a, b in zip(record.width_direction, record.long_direction, strict=True)))
        < 2e-5
    )
    assert record.source == recognise_section_passages(part)[0]


def test_aggregate_reconciles_the_generic_source_passage() -> None:
    part = _rectangular_through_slot()
    product = _take_inventory(part)

    assert product.result.oriented_slots == tuple(recognise_oriented_slots(part))
    assert product._legacy_result.section_passages == ()
    (decision,) = tuple(
        item
        for item in product.reconciliation.for_family(FamilyId.PASSAGES)
        if item.reason is ReasonCode.PASSAGE_SUPERSEDED_BY_ORIENTED_SLOT
    )
    assert decision.outcome is Outcome.REJECTED
    assert decision.related[0].family is FamilyId.ORIENTED_SLOTS
    assert product.evidence.defining_of(decision.candidate) == product.evidence.defining_of(
        decision.related[0]
    )


@pytest.mark.parametrize("presentation", [Rot(), Rot(90, 0, 0), Rot(0, 90, 0)])
def test_principal_rectangle_stays_in_legacy_slot_family(presentation) -> None:
    result = build_recognition_result(
        presentation * _rectangular_through_slot(0.0), rotational=False
    )

    assert len(result.slots) == 1
    assert result.oriented_slots == ()
    assert result.section_passages == ()


def test_square_and_curved_passages_are_not_oriented_slots() -> None:
    square = Box(100, 70, 10) - Rot(0, 0, 30) * Box(
        12,
        12,
        20,
        align=(Align.CENTER, Align.CENTER, Align.CENTER),
    )
    round_hole = Box(100, 70, 10) - Cylinder(5, 20)

    assert recognise_oriented_slots(square) == []
    assert recognise_oriented_slots(round_hole) == []


def test_tapered_nonplanar_passage_is_not_an_oriented_slot() -> None:
    tapered = Box(100, 70, 10) - Rot(0, 0, 30) * Cone(
        3, 8, 20, align=(Align.CENTER, Align.CENTER, Align.CENTER)
    )

    assert recognise_oriented_slots(tapered) == []


def test_record_directions_follow_a_rotated_whole_part() -> None:
    base = recognise_oriented_slots(_rectangular_through_slot(30.0))[0]
    rotated = recognise_oriented_slots(Rot(90, 0, 0) * _rectangular_through_slot(30.0))[0]

    assert rotated.width == base.width
    assert rotated.length == base.length
    assert rotated.depth == base.depth
    assert math.isclose(abs(rotated.width_direction[0]), abs(base.width_direction[0]), abs_tol=2e-6)
    assert math.isclose(abs(rotated.width_direction[2]), abs(base.width_direction[1]), abs_tol=2e-6)
    assert abs(rotated.width_direction[1]) < 2e-6


@pytest.mark.parametrize("x_angle,z_angle", [(17, 41), (49, 23), (73, 67)])
def test_public_record_accepts_serialized_frame_after_arbitrary_rigid_transform(
    x_angle: float, z_angle: float
) -> None:
    base = Box(120, 90, 10) - Rot(0, 0, 30) * Box(24, 6, 20, align=(Align.CENTER,) * 3)
    moved = base.rotate(Axis.X, x_angle).rotate(Axis.Z, z_angle)

    (record,) = recognise_oriented_slots(moved)
    framed = build_framed_recognition_result(moved)

    assert record.width == pytest.approx(6.0, abs=0.002)
    assert record.length == pytest.approx(24.0, abs=0.002)
    assert record.source.frame == recognise_section_passages(moved)[0].frame
    assert isinstance(framed, FramedRecognitionResult)
    assert len(framed.result.oriented_slots) == 1


def test_framed_result_preserves_oriented_array_under_arbitrary_presentation() -> None:
    part = _oriented_slot_pattern(((-30, 0), (0, 0), (30, 0)))

    baseline = build_framed_recognition_result(part)
    presented = build_framed_recognition_result(Pos(13, -7, 5) * Rot(17, 29, 11) * part)

    assert isinstance(baseline, FramedRecognitionResult)
    assert isinstance(presented, FramedRecognitionResult)
    assert presented.result.oriented_slots == baseline.result.oriented_slots
    assert presented.result.oriented_slot_patterns == baseline.result.oriented_slot_patterns


def test_mirror_and_reversed_face_traversal_preserve_records(monkeypatch) -> None:
    part = Pos(11, 0, 0) * _rectangular_through_slot(23)
    expected = recognise_oriented_slots(part)
    mirrored = recognise_oriented_slots(part.mirror())
    solid_faces = Solid.faces

    monkeypatch.setattr(Solid, "faces", lambda self: list(reversed(solid_faces(self))))

    assert len(mirrored) == 1
    assert mirrored[0].width == expected[0].width
    assert mirrored[0].length == expected[0].length
    assert recognise_oriented_slots(part) == expected


def test_step_round_trip_preserves_oriented_slot(tmp_path) -> None:
    source = _rectangular_through_slot(37.0)
    path = tmp_path / "oriented-slot.step"
    export_step(source, path)

    before = recognise_oriented_slots(source)
    after = recognise_oriented_slots(import_step(path))

    assert after == before


@pytest.mark.parametrize("scale", [0.1, 1.0, 10.0])
def test_scale_preserves_one_oriented_slot(scale: float) -> None:
    tool = Rot(0, 0, 23) * Box(
        30 * scale,
        8 * scale,
        20 * scale,
        align=(Align.CENTER, Align.CENTER, Align.CENTER),
    )
    part = Box(100 * scale, 70 * scale, 10 * scale) - tool

    (record,) = recognise_oriented_slots(part)

    assert record.width == pytest.approx(8 * scale, abs=0.002)
    assert record.length == pytest.approx(30 * scale, abs=0.002)


def test_translation_changes_only_public_location_values() -> None:
    base = recognise_oriented_slots(_rectangular_through_slot())[0]
    moved = recognise_oriented_slots(Pos(12, -7, 4) * _rectangular_through_slot())[0]

    assert moved.center == (12.0, -7.0, 4.0)
    assert moved.width == base.width
    assert moved.length == base.length
    assert moved.width_direction == base.width_direction
    assert moved.long_direction == base.long_direction


def test_blind_and_edge_open_rectangles_are_not_oriented_through_slots() -> None:
    blind = Box(100, 70, 10) - Pos(0, 0, 4) * Rot(0, 0, 30) * Box(
        30,
        8,
        6,
        align=(Align.CENTER, Align.CENTER, Align.CENTER),
    )
    edge_open = Box(100, 70, 10) - Pos(45, 0, 0) * Rot(0, 0, 30) * Box(
        30,
        8,
        20,
        align=(Align.CENTER, Align.CENTER, Align.CENTER),
    )

    assert recognise_oriented_slots(blind) == []
    assert recognise_oriented_slots(edge_open) == []


def test_patterns_require_matching_geometry_plane_orientation_and_body() -> None:
    source = recognise_oriented_slots(_rectangular_through_slot())[0]
    members = [replace(source, center=(float(x), 0.0, 0.0)) for x in (-20, 0, 20)]

    (pattern,) = recognise_oriented_slot_patterns(members)

    assert pattern.slots == tuple(members)
    assert pattern.pitch == pytest.approx(20.0)
    assert (
        recognise_oriented_slot_patterns([members[0], members[1], replace(members[2], width=9.0)])
        == []
    )
    assert (
        recognise_oriented_slot_patterns([members[0], members[1], replace(members[2], length=31.0)])
        == []
    )
    assert (
        recognise_oriented_slot_patterns(
            [members[0], members[1], replace(members[2], center=(20.0, 0.0, 2.0))]
        )
        == []
    )
    assert (
        recognise_oriented_slot_patterns(
            [members[0], members[1], replace(members[2], body_key=None)]
        )
        == []
    )


def test_real_array_and_grid_are_derived_from_aggregate_occurrences() -> None:
    array = build_recognition_result(
        _oriented_slot_pattern(((-30, 0), (0, 0), (30, 0))), rotational=False
    )
    grid = build_recognition_result(
        _oriented_slot_pattern(((-30, -20), (0, -20), (30, -20), (-30, 20), (0, 20), (30, 20))),
        rotational=False,
    )

    assert len(array.oriented_slots) == 3
    assert type(array.oriented_slot_patterns[0]).__name__ == "OrientedSlotArray"
    assert len(grid.oriented_slots) == 6
    assert type(grid.oriented_slot_patterns[0]).__name__ == "OrientedSlotGrid"


def test_compound_ownership_prevents_cross_body_patterns() -> None:
    body = _rectangular_through_slot(30)
    part = Compound(children=[Pos(-80, 0, 0) * body, Pos(80, 0, 0) * body])

    result = build_recognition_result(part, rotational=False)

    assert len(result.oriented_slots) == 2
    assert len({slot.body_key for slot in result.oriented_slots}) == 2
    assert result.oriented_slot_patterns == ()


def test_competing_orientation_and_material_obstruction_refuse_false_patterns() -> None:
    mixed = Box(120, 90, 10)
    for x, angle in ((-30, 30), (0, 30), (30, 45)):
        mixed -= (
            Pos(x, 0, 0)
            * Rot(0, 0, angle)
            * Box(24, 6, 20, align=(Align.CENTER, Align.CENTER, Align.CENTER))
        )
    obstructed = _rectangular_through_slot(30) + Box(
        2, 20, 10, align=(Align.CENTER, Align.CENTER, Align.CENTER)
    )

    result = build_recognition_result(mixed, rotational=False)
    assert len(result.oriented_slots) == 3
    assert result.oriented_slot_patterns == ()
    assert recognise_oriented_slots(obstructed) == []


def test_oriented_slot_semantic_golden() -> None:
    from tests.golden.oriented_slots.fixture import build_fixture

    part = build_fixture()
    product = _take_inventory(part)
    pattern = product.result.oriented_slot_patterns[0]
    summary = {
        "fixture": "oriented_slots",
        "physical": [
            {
                "center": list(slot.center),
                "width": slot.width,
                "length": slot.length,
                "depth": slot.depth,
                "has_source": slot.source in recognise_section_passages(part),
            }
            for slot in product.result.oriented_slots
        ],
        "aggregate": {
            "pattern_type": type(pattern).__name__,
            "member_centers": [list(slot.center) for slot in pattern.slots],
            "pitch": pattern.pitch,
            "section_passages": len(product._legacy_result.section_passages),
        },
        "reconciliation": sorted(
            decision.reason.value
            for decision in product.reconciliation.for_family(FamilyId.PASSAGES)
            if decision.reason is ReasonCode.PASSAGE_SUPERSEDED_BY_ORIENTED_SLOT
        ),
        "census": {"oriented_slot": feature_census(part)["oriented_slot"]},
    }
    expected = json.loads(
        Path("tests/golden/oriented_slots/contract.json").read_text(encoding="utf-8")
    )

    assert summary == expected
