# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Exact principal-axis rectangular blind-slot recognition."""

from __future__ import annotations

import pytest
from build123d import (
    Align,
    Box,
    Compound,
    Keep,
    Plane,
    Pos,
    Rot,
    Shell,
    Solid,
    export_step,
    import_step,
)

import quiddity.rectangular_blind_slots as rectangular_blind_slots
from quiddity._adjacency import FaceGraph
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._dispositions import Outcome, ReasonCode
from quiddity.evidence import build_recognition_evidence
from quiddity.frames import build_framed_recognition_result
from quiddity.rectangular_blind_slots import (
    RectangularBlindSlot,
    _contains_span,
    _has_unambiguous_slot_roles,
    _length_tolerance,
    recognise_rectangular_blind_slots,
)
from quiddity.result import _take_inventory
from tools._legacy_recognition import (
    build_raw_recognition_result,
)


def _slot(scale: float = 1.0):
    stock = Box(
        30 * scale,
        20 * scale,
        40 * scale,
        align=(Align.CENTER, Align.CENTER, Align.MIN),
    )
    tool = Pos(0, 5 * scale, 0) * Box(
        10 * scale,
        5 * scale,
        20 * scale,
        align=(Align.CENTER, Align.MIN, Align.MIN),
    )
    return stock - tool


@pytest.mark.parametrize("rotation", [Rot(0, 0, 0), Rot(90, 0, 0), Rot(0, 90, 0), Rot(180, 0, 0)])
def test_unified_slot_preserves_world_placement_and_ends(rotation):
    result = build_raw_recognition_result(Pos(123, -57, 91) * rotation * _slot())
    (source,) = result.rectangular_blind_slots
    (unified,) = result.section_recesses
    geometry = unified.geometry
    frame = geometry.frame
    midpoint = sum(geometry.run_interval) / 2
    center = tuple(frame.origin[index] + midpoint * frame.run[index] for index in range(3))
    assert center == pytest.approx(source.at, abs=0.002)
    assert geometry.ends.low.condition == ("open" if source.open_sign == -1 else "capped")
    assert geometry.ends.high.condition == ("open" if source.open_sign == 1 else "capped")


def test_rectangular_blind_slot_has_truthful_dimensions_and_complete_evidence():
    part = _slot()
    ledger = ClaimLedger(FaceGraph(part))

    actual = recognise_rectangular_blind_slots(part, ledger=ledger)

    assert actual == [
        RectangularBlindSlot(
            axis="z",
            open_sign=-1,
            length=20.0,
            width_axis="x",
            depth_axis="y",
            depth_sign=1,
            width=10.0,
            depth=5.0,
            at=(0.0, 7.5, 10.0),
        )
    ]
    assert len(ledger.claims) == 1
    assert len(ledger.claims[0].defining) == 4

    evidence = build_recognition_evidence(part)
    (feature,) = tuple(
        ref for ref in evidence.features if evidence.family(ref) == "section_recesses"
    )
    assert evidence.constituent_faces(feature) == evidence.defining_faces(feature)
    assert len(evidence.defining_faces(feature)) == 4

    (unified,) = build_raw_recognition_result(part).section_recesses
    assert unified.classification.feature_kind == "edge_open_recess"
    assert unified.classification.section_shape == "rectangular"
    assert unified.geometry.profile.closure == "open"
    assert unified.evidence.defining_faces == unified.evidence.constituent_faces


def test_axis_sign_scale_translation_frame_and_step_roundtrip_are_stable(tmp_path):
    base = _slot()
    path = tmp_path / "rectangular-blind-slot.step"
    export_step(base, path)
    assert recognise_rectangular_blind_slots(import_step(path)) == (
        recognise_rectangular_blind_slots(base)
    )

    for part, axis in (
        (base, "z"),
        (Rot(180, 0, 0) * base, "z"),
        (Rot(90, 0, 0) * base, "y"),
        (Rot(0, 90, 0) * base, "x"),
    ):
        (record,) = recognise_rectangular_blind_slots(part)
        assert record.axis == axis
        assert record.length == 20.0
        assert record.width == 10.0
        assert record.depth == 5.0

    shifted = Pos(123, -57, 91) * base
    assert len(recognise_rectangular_blind_slots(shifted)) == 1
    framed = build_framed_recognition_result(Rot(17, 29, 11) * shifted)
    assert len(framed.result.section_recesses) == 1
    assert framed.result.section_recesses[0].geometry.profile.closure == "open"

    for scale in (0.001, 1000.0):
        (record,) = recognise_rectangular_blind_slots(_slot(scale))
        assert record.length == pytest.approx(20 * scale, abs=max(0.001, scale * 1e-6))
        assert record.width == pytest.approx(10 * scale, abs=max(0.001, scale * 1e-6))
        assert record.depth == pytest.approx(5 * scale, abs=max(0.001, scale * 1e-6))


def test_reconciliation_prefers_complete_edge_open_contract_over_pocket_fragment():
    product = _take_inventory(_slot())

    (blind_slot,) = product.reconciliation.for_family(FamilyId.RECTANGULAR_BLIND_SLOTS)
    (pocket,) = product.reconciliation.for_family(FamilyId.POCKETS)
    assert blind_slot.outcome is Outcome.ACCEPTED
    assert pocket.outcome is Outcome.REJECTED
    assert pocket.reason is ReasonCode.POCKET_SUPERSEDED_BY_RECTANGULAR_BLIND_SLOT
    assert pocket.related == (blind_slot.candidate,)
    assert len(product._legacy_result.rectangular_blind_slots) == 1
    assert product._legacy_result.pockets == ()


def test_enclosed_pocket_doubly_open_channel_and_non_slot_role_are_refused():
    stock = Box(30, 20, 40, align=(Align.CENTER, Align.CENTER, Align.MIN))
    enclosed = stock - (Pos(0, 5, 10) * Box(10, 5, 20, align=(Align.CENTER, Align.MIN, Align.MIN)))
    channel = stock - (Pos(0, 5, 0) * Box(10, 5, 40, align=(Align.CENTER, Align.MIN, Align.MIN)))
    short_notch = stock - (Pos(0, 5, 0) * Box(10, 5, 4, align=(Align.CENTER, Align.MIN, Align.MIN)))

    assert recognise_rectangular_blind_slots(enclosed) == []
    assert recognise_rectangular_blind_slots(channel) == []
    assert recognise_rectangular_blind_slots(short_notch) == []


def test_compound_members_cannot_supply_one_cross_solid_slot():
    left = Pos(-40, 0, 0) * Box(10, 10, 10)
    right = Pos(40, 0, 0) * Box(10, 10, 10)
    assert recognise_rectangular_blind_slots(Compound([left, right])) == []


def test_split_cap_sides_floor_and_compound_order_preserve_complete_occurrences():
    part = _slot()
    rebuilt = []
    for face in part.faces():
        bounds = face.bounding_box()
        if (
            abs(bounds.min.Z - 20.0) < 1e-7
            and abs(bounds.max.Z - 20.0) < 1e-7
            and bounds.min.X < 0 < bounds.max.X
            and bounds.min.Y > 4.0
        ):
            rebuilt.extend(face.split(Plane.YZ, Keep.BOTH))
        elif (
            abs(abs(bounds.min.X) - 5.0) < 1e-7
            and abs(bounds.max.X - bounds.min.X) < 1e-7
            and bounds.min.Y > 4.0
            and bounds.min.Z < 10 < bounds.max.Z
        ):
            rebuilt.extend(face.split(Plane.XY.offset(10), Keep.BOTH))
        elif (
            abs(bounds.min.Y - 5.0) < 1e-7
            and abs(bounds.max.Y - bounds.min.Y) < 1e-7
            and bounds.min.X < 0 < bounds.max.X
            and bounds.min.Z < 10 < bounds.max.Z
        ):
            rebuilt.extend(face.split(Plane.YZ, Keep.BOTH))
        else:
            rebuilt.append(face)
    split = Solid(Shell(rebuilt))
    assert split.is_valid
    (record,) = recognise_rectangular_blind_slots(split)
    assert record == recognise_rectangular_blind_slots(part)[0]
    split_ledger = ClaimLedger(FaceGraph(split))
    assert recognise_rectangular_blind_slots(split, ledger=split_ledger) == [record]
    assert len(split_ledger.claims[0].defining) == 8

    left = Pos(-50, 0, 0) * part
    right = Pos(50, 0, 0) * part
    forward = recognise_rectangular_blind_slots(Compound([left, right]))
    reverse = recognise_rectangular_blind_slots(Compound([right, left]))
    assert forward == reverse
    assert len(forward) == 2


@pytest.mark.parametrize(
    "interruption",
    (
        Pos(4, 6, 9) * Box(4, 3, 2, align=(Align.MIN, Align.MIN, Align.MIN)),
        Pos(-1, 3, 9) * Box(2, 4, 2, align=(Align.MIN, Align.MIN, Align.MIN)),
    ),
)
def test_perforated_side_or_floor_is_not_a_complete_rectangular_section(interruption):
    interrupted = _slot() - interruption
    assert interrupted.is_valid
    assert recognise_rectangular_blind_slots(interrupted) == []


def test_material_in_sweep_and_open_invalid_body_are_refused():
    part = _slot()
    connected_bridge = Pos(-1, 5, 9) * Box(2, 2, 2)
    assert recognise_rectangular_blind_slots(part + connected_bridge) == []

    open_body = Solid(Shell(list(part.faces())[:-1]))
    assert not open_body.is_valid
    assert recognise_rectangular_blind_slots(open_body) == []


@pytest.mark.parametrize("proof", ("_common_convex_context", "_empty_sweep"))
def test_failed_material_proof_refuses_the_candidate(monkeypatch, proof):
    monkeypatch.setattr(rectangular_blind_slots, proof, lambda *_args: False)
    assert recognise_rectangular_blind_slots(_slot()) == []


def test_local_span_tolerance_accepts_and_rejects_both_sides():
    tolerance = _length_tolerance(20.0)
    expected = (0.0, 20.0)
    assert _contains_span((0.5 * tolerance, 20.0 - 0.5 * tolerance), expected, 20.0)
    assert not _contains_span((2 * tolerance, 20.0), expected, 20.0)
    assert not _contains_span((0.0, 20.0 - 2 * tolerance), expected, 20.0)


def test_role_tie_refuses_without_an_axis_or_traversal_tiebreak():
    tolerance = _length_tolerance(20.0)
    assert _has_unambiguous_slot_roles(20.0, 20.0, 20.0 - 2 * tolerance)
    assert not _has_unambiguous_slot_roles(20.0, 20.0, 20.0 - 0.5 * tolerance)
    assert not _has_unambiguous_slot_roles(20.0, 20.0 + 2 * tolerance, 5.0)


def test_raw_aggregate_projects_the_new_family():
    result = build_raw_recognition_result(_slot())
    assert list(result.rectangular_blind_slots) == recognise_rectangular_blind_slots(_slot())
