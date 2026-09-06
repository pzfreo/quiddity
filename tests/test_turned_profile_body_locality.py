# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Body-local occurrence and grouping contracts for turned profiles."""

from pathlib import Path

import pytest
from build123d import Axis, Box, Compound, Cylinder, Pos, Rotation, export_step, import_step

from quiddity import (
    FramedRecognitionResult,
    TurnedProfile,
    TurnedProfileKey,
    TurnedStep,
    build_framed_recognition_result,
    build_recognition_result,
    recognise_turned_steps,
)
from quiddity._adjacency import FaceGraph
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity.result import _take_inventory


def _shaft(*, x: float = 0.0, y: float = 0.0, scale: float = 1.0):
    large = Pos(0, 0, 20 * scale) * Cylinder(15 * scale, 40 * scale)
    small = Pos(0, 0, 55 * scale) * Cylinder(8 * scale, 30 * scale)
    return Pos(x, y, 0) * Rotation(0, 90, 0) * (large + small)


def _y_shaft(*, x: float = 0.0):
    large = Pos(0, 0, 20) * Cylinder(15, 40)
    small = Pos(0, 0, 55) * Cylinder(8, 30)
    return Pos(x, 0, 0) * Rotation(90, 0, 0) * (large + small)


def test_parallel_equal_shafts_retain_two_physical_profile_groups() -> None:
    part = Compound(children=[_shaft(y=-30), _shaft(y=30)])

    steps = recognise_turned_steps(part)
    profiles = TurnedProfile.grouped_from_steps(steps)

    assert len(steps) == 4
    assert len(profiles) == 2
    assert [profile.axis for profile in profiles] == ["x", "x"]
    assert [profile.profile.axis_origin for profile in profiles if profile.profile] == [
        (0.0, -30.0, 0.0),
        (0.0, 30.0, 0.0),
    ]
    dimensions = [
        [(step.lo, step.hi, step.diameter) for step in profile.steps] for profile in profiles
    ]
    assert dimensions == [
        [(0.0, 40.0, 30.0), (40.0, 70.0, 16.0)],
        [(0.0, 40.0, 30.0), (40.0, 70.0, 16.0)],
    ]


def test_aggregate_matches_body_local_profile_roster() -> None:
    part = Compound(children=[_shaft(y=-30), _shaft(y=30, scale=0.8)])

    public = tuple(recognise_turned_steps(part))
    result = build_recognition_result(part, rotational=True)

    assert result.turned_steps == public
    assert len(TurnedProfile.grouped_from_steps(result.turned_steps)) == 2


def test_all_turned_evidence_validates_before_any_candidate_is_published(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    part = Compound(children=[_shaft(y=-30), _shaft(y=30)])
    ledger = ClaimLedger(FaceGraph(part))
    original = FaceGraph.common_valid_solid
    calls = 0

    def fail_later_proposal(graph: FaceGraph, nodes):
        nonlocal calls
        calls += 1
        return None if calls == 2 else original(graph, nodes)

    monkeypatch.setattr(FaceGraph, "common_valid_solid", fail_later_proposal)

    with pytest.raises(ValueError, match="no common valid solid"):
        recognise_turned_steps(part, ledger=ledger)
    assert calls == 2
    assert ledger.claims == ()


def test_coaxial_disjoint_profiles_and_child_order_retain_membership() -> None:
    left = _shaft(x=-100)
    right = _shaft(x=100)

    forward = recognise_turned_steps(Compound(children=[left, right]))
    reverse = recognise_turned_steps(Compound(children=[right, left]))

    assert forward == reverse
    profiles = TurnedProfile.grouped_from_steps(forward)
    assert len(profiles) == 2
    assert [profile.profile.body_bounds[:2] for profile in profiles if profile.profile] == [
        (-100.0, -30.0),
        (100.0, 170.0),
    ]


def test_mixed_principal_axes_form_independent_profiles() -> None:
    profiles = TurnedProfile.grouped_from_steps(
        recognise_turned_steps(Compound(children=[_shaft(y=-50), _y_shaft(x=50)]))
    )

    assert len(profiles) == 2
    assert [profile.axis for profile in profiles] == ["x", "y"]
    assert all(len(profile.steps) == 2 for profile in profiles)


def test_equal_value_records_with_distinct_keys_are_not_merged() -> None:
    first = TurnedProfileKey("x", (0.0, -10.0, 0.0), (0.0, 20.0, -20.0, 0.0, -10.0, 10.0))
    second = TurnedProfileKey("x", (0.0, 10.0, 0.0), (0.0, 20.0, 0.0, 20.0, -10.0, 10.0))
    steps = [
        TurnedStep("x", 0, 10, 20, first),
        TurnedStep("x", 10, 20, 10, first),
        TurnedStep("x", 0, 10, 20, second),
        TurnedStep("x", 10, 20, 10, second),
    ]

    assert len(TurnedProfile.grouped_from_steps(steps)) == 2
    with pytest.raises(ValueError, match="multiple physical profiles"):
        TurnedProfile.from_steps(steps)


def test_refused_and_legacy_profile_keys_remain_sortable_and_groupable() -> None:
    legacy = TurnedProfileKey("x", (0.0, 0.0, 0.0), (0.0, 20.0, -10.0, 10.0, -10.0, 10.0))
    refused = TurnedProfileKey("x", (0.0, 0.0, 0.0), (0.0, 20.0, -10.0, 10.0, -10.0, 10.0), None)
    steps = [
        TurnedStep("x", 0, 10, 20, legacy),
        TurnedStep("x", 10, 20, 10, refused),
    ]

    assert sorted((legacy, refused)) == [refused, legacy]
    assert len(TurnedProfile.grouped_from_steps(steps)) == 2
    assert TurnedProfileKey.__lt__(legacy, object()) is NotImplemented


def test_step_round_trip_preserves_parallel_profile_grouping(tmp_path: Path) -> None:
    source = Compound(children=[_shaft(y=-30), _shaft(y=30)])
    source_steps = recognise_turned_steps(source)
    path = tmp_path / "parallel-shafts.step"
    assert export_step(source, path)

    imported_steps = recognise_turned_steps(import_step(path))
    profiles = TurnedProfile.grouped_from_steps(imported_steps)

    assert imported_steps == source_steps
    assert len(profiles) == 2
    assert [profile.profile.axis_origin for profile in profiles if profile.profile] == [
        (0.0, -30.0, 0.0),
        (0.0, 30.0, 0.0),
    ]


def test_framed_rigid_motion_preserves_profile_inventory() -> None:
    part = Compound(children=[_shaft(y=-30), _shaft(y=30, scale=0.8)])
    baseline = build_framed_recognition_result(part, rotational=True)
    moved = build_framed_recognition_result(
        Pos(13, -7, 5) * part.rotate(Axis.X, 30), rotational=True
    )

    assert isinstance(baseline, FramedRecognitionResult)
    assert isinstance(moved, FramedRecognitionResult)
    # This is an AXIAL frame: roll about X is an explicitly non-semantic gauge, so transverse
    # descriptor coordinates may rotate while physical grouping and axial dimensions must hold.
    assert len(moved.result.turned_profiles) == len(baseline.result.turned_profiles) == 2
    assert [(step.axis, step.lo, step.hi, step.diameter) for step in moved.result.turned_steps] == [
        (step.axis, step.lo, step.hi, step.diameter) for step in baseline.result.turned_steps
    ]


def test_turned_profile_suppresses_plate_only_on_its_own_solid() -> None:
    bracket = (Pos(0, 120, 5) * Box(80, 60, 10)) + (Pos(0, 120, 35) * Box(80, 10, 50))
    part = Compound(children=[_shaft(), bracket])

    product = _take_inventory(part, rotational=True)

    assert len(product.result.turned_steps) == 2
    assert len(product.result.plates) == 2
    step_solids = {
        product.context.graph.common_valid_solid(product.evidence.defining_of(candidate))
        for candidate in product.physical.candidate_set(FamilyId.TURNED_STEPS).candidates
    }
    plate_solids = {
        product.context.graph.common_valid_solid(product.evidence.defining_of(candidate))
        for candidate in product.physical.candidate_set(FamilyId.PLATES).candidates
    }
    assert None not in step_solids | plate_solids
    assert step_solids.isdisjoint(plate_solids)


def test_rotational_classification_without_a_turned_body_does_not_admit_plates() -> None:
    assert build_recognition_result(Box(80, 60, 10), rotational=True).plates == ()
