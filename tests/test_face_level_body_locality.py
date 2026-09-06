# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Body-local occurrence and support contracts for FaceLevel."""

from copy import deepcopy
from typing import cast

import pytest
from build123d import Align, Axis, Box, Compound, Cylinder, Pos, export_step, import_step

from quiddity import (
    FaceLevel,
    FramedRecognitionResult,
    build_framed_recognition_result,
    build_recognition_result,
    recognise_turned_steps,
    step_level_records,
)
from quiddity._adjacency import FaceGraph
from quiddity._candidates import FamilyId
from quiddity._claims import EvidenceWriter
from quiddity.levels import _discover_step_levels, recognise_face_levels
from quiddity.result import _take_inventory

_MINIMUM_Z = (Align.CENTER, Align.CENTER, Align.MIN)


def _stepped(dx: float):
    base = Box(80, 50, 10, align=_MINIMUM_Z)
    upper = Pos(20, 0, 10) * Box(40, 50, 10, align=_MINIMUM_Z)
    return Pos(dx, 0, 0) * (base + upper)


def _z_shaft():
    return Cylinder(20, 30, align=_MINIMUM_Z) + Pos(0, 0, 30) * Cylinder(12, 20, align=_MINIMUM_Z)


def test_equal_levels_on_separate_bodies_retain_two_body_local_supports() -> None:
    left = _stepped(-70)
    right = _stepped(70)
    part = Compound(children=[left, right])

    levels = list(build_recognition_result(part).step_levels)

    assert levels == step_level_records(part)
    assert [(level.z, level.x_span, level.y_span) for level in levels] == [
        (10.0, (-110.0, -70.0), (-25.0, 25.0)),
        (10.0, (30.0, 70.0), (-25.0, 25.0)),
    ]
    assert len({level.body_key for level in levels}) == 2
    assert all(level.body_key not in ((), None) for level in levels)


def test_aggregate_occurrences_retain_distinct_defining_solid_authority() -> None:
    part = Compound(children=[_stepped(-70), _stepped(70)])
    product = _take_inventory(part)
    candidates = product.physical.candidate_set(FamilyId.STEP_LEVELS).candidates

    assert len(candidates) == 2
    defining = [product.evidence.defining_of(candidate) for candidate in candidates]
    assert all(nodes for nodes in defining)
    solids = [product.context.graph.common_valid_solid(nodes) for nodes in defining]
    assert all(solid is not None for solid in solids)
    assert solids[0] != solids[1]


def test_child_order_does_not_change_body_local_level_order() -> None:
    left = _stepped(-70)
    right = _stepped(70)

    forward = step_level_records(Compound(children=[left, right]))
    reverse = step_level_records(Compound(children=[right, left]))

    assert reverse == forward
    assert [record.to_dict() for record in reverse] == [record.to_dict() for record in forward]


def test_nested_disconnected_stair_does_not_borrow_turned_profile_membership() -> None:
    shaft = _z_shaft()
    # Wholly inside the shaft AABB, but outside its cylindrical material.
    stair = Pos(17, 17, 0) * Box(2, 2, 8, align=_MINIMUM_Z) + Pos(17.5, 17, 8) * Box(
        1, 2, 5, align=_MINIMUM_Z
    )
    part = Compound(children=[shaft, stair])

    levels = recognise_face_levels(part)
    profiles = {step.profile.body_key for step in recognise_turned_steps(part) if step.profile}
    stair_levels = [level for level in levels if level.x_span and level.x_span[0] >= 16]
    shaft_levels = [level for level in levels if level not in stair_levels]

    assert {level.body_key for level in shaft_levels} == profiles
    assert len({level.body_key for level in stair_levels}) == 1
    assert stair_levels[0].body_key not in profiles


def test_blind_bore_floor_joins_its_turned_body_not_a_remote_solid() -> None:
    bored = _z_shaft() - Pos(0, 0, 42) * Cylinder(4, 8, align=_MINIMUM_Z)
    part = Compound(children=[bored, Pos(60, 0, 0) * Box(10, 10, 10, align=_MINIMUM_Z)])

    (floor,) = [level for level in recognise_face_levels(part) if level.z == 42.0]
    turned_keys = {step.profile.body_key for step in recognise_turned_steps(part) if step.profile}

    assert floor.body_key in turned_keys
    assert floor.body_key not in {
        level.body_key for level in recognise_face_levels(part) if level.x_span == (55.0, 65.0)
    }


def test_coincident_body_signatures_refuse_public_membership() -> None:
    body = _stepped(0)
    levels = recognise_face_levels(Compound(children=[body, deepcopy(body)]))

    assert levels
    assert all(level.body_key is None for level in levels)


def test_recognised_and_legacy_face_levels_remain_totally_ordered() -> None:
    (recognised,) = step_level_records(_stepped(0))
    legacy = FaceLevel(recognised.z, recognised.x_span, recognised.y_span)

    assert legacy != recognised
    assert sorted((recognised, legacy)) == [legacy, recognised]
    assert FaceLevel.__lt__(legacy, object()) is NotImplemented


def test_framed_rigid_motion_preserves_body_local_level_occurrences() -> None:
    part = Compound(children=[_stepped(-70), _stepped(70)])
    baseline = build_framed_recognition_result(part)
    moved = build_framed_recognition_result(Pos(13, -7, 5) * part.rotate(Axis.X, 30))

    assert isinstance(baseline, FramedRecognitionResult)
    assert isinstance(moved, FramedRecognitionResult)
    assert len(moved.result.step_levels) == len(baseline.result.step_levels)
    assert len({level.body_key for level in moved.result.step_levels}) == 2
    assert all(level.body_key not in ((), None) for level in moved.result.step_levels)
    for actual, expected in zip(moved.result.step_levels, baseline.result.step_levels, strict=True):
        assert actual.z == pytest.approx(expected.z, abs=1e-9)
        assert actual.x_span == pytest.approx(expected.x_span, abs=1e-9)
        assert actual.y_span == pytest.approx(expected.y_span, abs=1e-9)


def test_connected_faces_at_one_level_still_coalesce_within_their_body() -> None:
    part = _stepped(0) + Pos(-20, 0, 10) * Box(40, 50, 10, align=_MINIMUM_Z)

    assert step_level_records(part) == []


def test_nested_compounds_retain_the_flat_body_occurrence_roster() -> None:
    left = _stepped(-70)
    right = _stepped(70)
    nested = Compound(children=[Compound(children=[left]), Compound(children=[right])])

    assert step_level_records(nested) == step_level_records(Compound(children=[left, right]))


def test_large_foreign_body_does_not_raise_a_small_bodys_area_threshold() -> None:
    small = _stepped(-70)
    large = Pos(500, 0, 0) * Box(1000, 1000, 20, align=_MINIMUM_Z)

    assert step_level_records(Compound(children=[small, large])) == step_level_records(small)


def test_step_round_trip_preserves_body_local_occurrences(tmp_path) -> None:
    part = Compound(children=[_stepped(-70), _stepped(70)])
    path = tmp_path / "separate-stepped-bodies.step"
    export_step(part, path)

    imported = import_step(path)

    assert step_level_records(imported) == step_level_records(part)


def test_writer_refuses_a_level_when_same_solid_authority_is_missing() -> None:
    face = Box(20, 10, 2).faces().sort_by(Axis.Z)[-1]
    part = Compound(children=[Pos(0, 0, z) * face for z in (-1, 4, 9)])
    graph = FaceGraph(part)

    class _UnusedSink:
        @staticmethod
        def propose(*args, **kwargs):
            raise AssertionError("validation must finish before publication")

    writer = cast(EvidenceWriter, type("Writer", (), {"graph": graph, "sink": _UnusedSink()})())

    assert step_level_records(part) == [FaceLevel(5.0, (-10.0, 10.0), (-5.0, 5.0), None)]
    with pytest.raises(ValueError, match="no unambiguous valid solid"):
        _discover_step_levels(part, writer=writer)


def test_writer_free_open_face_uses_the_non_solid_compatibility_scope() -> None:
    face = Box(20, 10, 2).faces().sort_by(Axis.Z)[-1]

    levels = recognise_face_levels(face)

    assert len(levels) == 1
    assert levels[0].z == pytest.approx(1.0)
