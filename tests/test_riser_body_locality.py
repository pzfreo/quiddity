# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Body-local discovery, attribution and level projection for RiserEvidence."""

from copy import deepcopy

import pytest
from build123d import Align, Axis, Box, Compound, Pos, export_step, import_step

from quiddity import (
    FaceLevel,
    FramedRecognitionResult,
    RiserEvidence,
    StepShoulder,
    build_framed_recognition_result,
    build_recognition_result,
    project_step_shoulders,
    recognise_risers,
    step_level_records,
)
from quiddity._candidates import FamilyId
from quiddity.result import _take_inventory
from tests.route_pins import assert_core_route_is_closed

_MINIMUM_Z = (Align.CENTER, Align.CENTER, Align.MIN)


def _stepped(*, dy: float, dx: float = 0.0, level: float = 10.0):
    base = Box(80, 40, level, align=_MINIMUM_Z)
    upper = Pos(20, 0, level) * Box(40, 40, 10, align=_MINIMUM_Z)
    return Pos(dx, dy, 0) * (base + upper)


def _signature(part):
    return [
        (riser.axis, riser.positions, tuple(level.z for level in riser.body_levels or ()))
        for riser in recognise_risers(part)
    ]


def test_separated_equal_steps_retain_two_real_risers_and_no_compound_envelope_faces() -> None:
    part = Compound(children=[_stepped(dy=-50), _stepped(dy=50)])

    assert _signature(part) == [
        ("x", (0.0,), (10.0,)),
        ("x", (0.0,), (10.0,)),
    ]
    shoulders = project_step_shoulders(recognise_risers(part), levels=[10.0])
    assert [(item.axis, item.position) for item in shoulders] == [
        ("x", 0.0),
        ("x", 0.0),
    ]


def test_unequal_body_level_projection_cannot_borrow_the_other_solids_level() -> None:
    low = _stepped(dy=-50, dx=-30, level=10)
    high = _stepped(dy=50, dx=30, level=15)
    risers = recognise_risers(Compound(children=[low, high]))

    assert [(r.positions, tuple(level.z for level in r.body_levels or ())) for r in risers] == [
        ((-30.0,), (10.0,)),
        ((30.0,), (15.0,)),
    ]
    assert [item.position for item in project_step_shoulders(risers, levels=[10.0])] == [-30.0]
    assert [item.position for item in project_step_shoulders(risers, levels=[15.0])] == [30.0]


def test_equal_z_projection_can_select_one_body_local_level_occurrence() -> None:
    part = Compound(children=[_stepped(dy=-50), _stepped(dy=50)])
    risers = recognise_risers(part)
    levels = step_level_records(part)

    assert len(levels) == 2
    assert levels[0].z == levels[1].z == 10.0
    assert project_step_shoulders(risers, levels=[levels[0]]) == [StepShoulder("x", 0.0)]
    assert len(project_step_shoulders(risers, levels=[10.0])) == 2


def test_value_identical_bodies_can_be_selected_by_riser_occurrence() -> None:
    first = _stepped(dy=0)
    part = Compound(children=[first, deepcopy(first)])
    risers = recognise_risers(part)

    assert len(risers) == 2
    assert risers[0] == risers[1]
    assert risers[0].body_levels == risers[1].body_levels
    assert project_step_shoulders(
        risers,
        levels_by_riser=((risers[0].body_levels or ()), ()),
    ) == [StepShoulder("x", 0.0)]


def test_occurrence_aligned_projection_rejects_misaligned_or_ambiguous_inputs() -> None:
    risers = recognise_risers(_stepped(dy=0))

    with pytest.raises(ValueError, match="one-for-one"):
        project_step_shoulders(risers, levels_by_riser=())
    with pytest.raises(ValueError, match="not both"):
        project_step_shoulders(risers, levels=[10.0], levels_by_riser=((10.0,),))


def test_non_default_tolerance_is_shared_with_body_level_authority() -> None:
    part = _stepped(dy=0, level=0.55)
    levels = step_level_records(part, tol=0.1)
    risers = recognise_risers(part, tol=0.1)

    assert [level.z for level in levels] == [0.55]
    assert tuple(level.z for level in risers[0].body_levels or ()) == (0.55,)
    assert project_step_shoulders(risers, levels=levels) == [StepShoulder("x", 0.0)]


def test_recognised_and_legacy_riser_records_remain_totally_ordered() -> None:
    (recognised,) = recognise_risers(_stepped(dy=0))
    legacy = RiserEvidence(
        vertical=recognised.vertical,
        axis=recognised.axis,
        positions=recognised.positions,
        other_axis=recognised.other_axis,
        other_positions=recognised.other_positions,
        z_lo=recognised.z_lo,
        z_hi=recognised.z_hi,
        lo_at_envelope=recognised.lo_at_envelope,
        hi_at_envelope=recognised.hi_at_envelope,
        tol=recognised.tol,
    )

    assert legacy != recognised
    assert sorted((recognised, legacy)) == [legacy, recognised]
    assert project_step_shoulders([legacy], levels=[10.0])
    assert RiserEvidence.__lt__(legacy, object()) is NotImplemented


def test_projection_empty_and_untied_boundaries_fail_closed() -> None:
    assert project_step_shoulders([], levels=[1.0]) == []
    untied = RiserEvidence(
        vertical=False,
        axis="x",
        positions=(4.0,),
        other_axis="y",
        other_positions=(),
        z_lo=2.0,
        z_hi=3.0,
        lo_at_envelope=False,
        hi_at_envelope=False,
        body_levels=(FaceLevel(1.0),),
    )

    assert project_step_shoulders([untied], levels=[]) == []
    assert project_step_shoulders([untied], levels=[1.0]) == []


def test_aggregate_riser_occurrences_have_distinct_defining_solid_authority() -> None:
    part = Compound(children=[_stepped(dy=-50), _stepped(dy=50)])
    product = _take_inventory(part)
    candidates = product.physical.candidate_set(FamilyId.RISERS).candidates

    assert len(candidates) == 2
    defining = [product.evidence.defining_of(candidate) for candidate in candidates]
    assert all(nodes for nodes in defining)
    owners = [product.context.graph.common_valid_solid(nodes) for nodes in defining]
    assert all(owner is not None for owner in owners)
    assert owners[0] != owners[1]
    assert product.result.risers == tuple(recognise_risers(part))


def test_translating_one_child_changes_only_its_body_local_riser() -> None:
    fixed = _stepped(dy=-50, dx=-30)
    baseline = _signature(Compound(children=[fixed, _stepped(dy=50, dx=30)]))
    moved = _signature(Compound(children=[fixed, _stepped(dy=50, dx=45)]))

    assert baseline == [("x", (-30.0,), (10.0,)), ("x", (30.0,), (10.0,))]
    assert moved == [("x", (-30.0,), (10.0,)), ("x", (45.0,), (10.0,))]


def test_nested_compound_and_child_order_preserve_riser_occurrences() -> None:
    left = _stepped(dy=-50, dx=-30)
    right = _stepped(dy=50, dx=30)
    flat = Compound(children=[left, right])
    baseline = _signature(flat)
    reversed_nested = Compound(children=[Compound(children=[right]), Compound(children=[left])])

    assert _signature(reversed_nested) == baseline


def test_framed_rigid_motion_preserves_body_local_riser_occurrences() -> None:
    part = Compound(children=[_stepped(dy=-50, dx=-30), _stepped(dy=50, dx=30)])
    baseline_result = build_framed_recognition_result(part)
    moved_result = build_framed_recognition_result(Pos(13, -7, 5) * part.rotate(Axis.X, 30))
    assert isinstance(baseline_result, FramedRecognitionResult)
    assert isinstance(moved_result, FramedRecognitionResult)
    baseline = baseline_result.result.risers
    moved = moved_result.result.risers

    assert len(moved) == len(baseline)
    for actual, expected in zip(moved, baseline, strict=True):
        assert actual.axis == expected.axis
        assert actual.positions == pytest.approx(expected.positions, abs=1e-9)
        assert actual.body_levels is not None
        assert expected.body_levels is not None
        assert [level.z for level in actual.body_levels] == pytest.approx(
            [level.z for level in expected.body_levels], abs=1e-9
        )


def test_step_round_trip_preserves_body_local_risers(tmp_path) -> None:
    part = Compound(children=[_stepped(dy=-50, dx=-30), _stepped(dy=50, dx=30)])
    path = tmp_path / "separate-stepped-risers.step"
    export_step(part, path)

    imported = import_step(path)

    assert _signature(imported) == _signature(part)
    assert build_recognition_result(imported).risers == tuple(recognise_risers(imported))


def test_riser_private_core_and_registry_writer_route_are_closed() -> None:
    # As with step levels, the public entry point is not this core under another name and so is
    # not a sanctioned caller. `recognise_risers` rebuilds its own body levels from the part,
    # because standalone there is no run to read completed FaceLevel authority from; the core
    # takes that authority as `body_levels` and the declaration is what supplies it.
    assert_core_route_is_closed(
        module="levels",
        core="_discover_risers",
        declaration="_discover_riser_family",
        handed_over={"writer": "services.writer", "body_levels": "body_levels"},
        also_reached_from={},
    )
