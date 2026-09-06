# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Shared public body-correlation key boundaries."""

import math
from pathlib import Path

import pytest
from build123d import (
    Axis,
    Box,
    Compound,
    Cylinder,
    GeomType,
    Pos,
    Rot,
    SlotOverall,
    export_step,
    extrude,
    fillet,
    import_step,
)

from quiddity._body_identity import body_signature, unambiguous_body_keys
from quiddity._geometry import body_signature as geometry_body_signature
from quiddity.evidence import FramedRecognitionEvidence
from quiddity.frames import build_framed_recognition_evidence
from quiddity.levels import recognise_face_levels
from quiddity.oriented_slots import recognise_oriented_slots
from quiddity.result import build_raw_recognition_result


def test_oblique_curved_body_key_is_stable_across_step(tmp_path: Path) -> None:
    shaft = Pos(0, 0, 20) * Cylinder(15, 40) + Pos(0, 0, 55) * Cylinder(8, 30)
    curved = fillet([edge for edge in shaft.edges() if edge.geom_type == GeomType.CIRCLE], 0.2)
    source = curved.rotate(Axis.Y, 23)
    path = tmp_path / "oblique-curved.step"
    assert export_step(source, path)

    assert unambiguous_body_keys([import_step(path)], require_valid_solid=True) == (
        unambiguous_body_keys([source], require_valid_solid=True)[0],
    )


def _slotted_body():
    return Box(100.123456789, 70, 10) - Rot(0, 0, 17) * Box(30, 8, 20)


def test_all_signature_entrypoints_and_validity_modes_share_one_policy():
    body = _slotted_body()
    expected = body_signature(body)
    assert geometry_body_signature is body_signature
    assert unambiguous_body_keys([body]) == (expected,)
    assert unambiguous_body_keys([body], require_valid_solid=True) == (expected,)


@pytest.mark.parametrize("placement", [Pos(), Pos(12.3456789, 23, 4), Rot(0, 0, 11)])
def test_slot_and_levels_share_body_key_direct_and_aggregate(placement):
    body = placement * _slotted_body()
    expected = body_signature(body)
    slots = recognise_oriented_slots(body)
    levels = recognise_face_levels(body)
    assert len(slots) == 1 and levels
    assert {record.body_key for record in [*slots, *levels]} == {expected}
    result = build_raw_recognition_result(body)
    # Aggregate reconciliation removes exterior FaceLevels on this through-slotted plate.
    assert len(result.oriented_slots) == 1
    assert {record.body_key for record in [*result.oriented_slots, *levels]} == {expected}


def test_duplicate_and_distinct_bodies_keep_occurrence_aligned_keys():
    first = _slotted_body()
    duplicate = _slotted_body()
    other = Pos(200, 0, 0) * first
    for strict in (False, True):
        assert unambiguous_body_keys([first, duplicate, other], require_valid_solid=strict) == (
            None,
            None,
            body_signature(other),
        )
    for solids in ([first, other], [other, first]):
        part = Compound(children=solids)
        slots = recognise_oriented_slots(part)
        levels = recognise_face_levels(part)
        assert len(slots) == 2 and levels
        assert {slot.body_key for slot in slots} == {level.body_key for level in levels}
        assert {slot.body_key for slot in slots} == {body_signature(first), body_signature(other)}


def test_framed_keys_use_the_shared_policy_in_working_coordinates():
    body = Pos(13, 29, 41) * Rot(17, 31, 43) * _slotted_body()
    view = build_framed_recognition_evidence(body)
    assert isinstance(view, FramedRecognitionEvidence)
    expected = body_signature(view.part)
    assert len(view.result.oriented_slots) == 1
    levels = recognise_face_levels(view.part)
    assert levels
    assert {record.body_key for record in [*view.result.oriented_slots, *levels]} == {expected}


@pytest.mark.parametrize("reverse", [False, True])
def test_non_slot_body_with_equal_signature_refuses_slot_key(reverse):
    # Same removed area (240) and perimeter (76), but different feature anatomy.
    width = (38 - math.sqrt(38**2 - math.pi * 240)) / (math.pi / 2)
    straight = (76 - math.pi * width) / 2
    rectangle = Box(100, 70, 10) - Rot(0, 0, 17) * Box(30, 8, 20)
    obround = Box(100, 70, 10) - extrude(SlotOverall(straight + width, width), amount=20, both=True)
    assert body_signature(rectangle) == body_signature(obround)
    bodies = [rectangle, obround]
    part = Compound(children=bodies[::-1] if reverse else bodies)
    slots = recognise_oriented_slots(part)
    assert len(slots) == 1 and slots[0].body_key is None
    levels = recognise_face_levels(part)
    assert levels and all(level.body_key is None for level in levels)
    result = build_raw_recognition_result(part)
    assert len(result.oriented_slots) == 1
    assert result.oriented_slots[0].body_key is None
