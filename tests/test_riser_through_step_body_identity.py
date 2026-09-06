# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Public same-solid ownership between RiserEvidence and ThroughStep (#388)."""

from copy import deepcopy
from pathlib import Path

from build123d import Align, Axis, Box, Compound, Pos, export_step, import_step

from quiddity import (
    FramedRecognitionResult,
    build_framed_recognition_result,
    recognise_risers,
    recognise_through_steps,
)

_MINIMUM_Z = (Align.CENTER, Align.CENTER, Align.MIN)


def _stair():
    """A Y-through open step whose two X walls are also structural risers."""

    return Box(20, 30, 10, align=_MINIMUM_Z) + Pos(0, 0, 10) * Box(10, 30, 10, align=_MINIMUM_Z)


def _remote_through_step(*, gap: float):
    body = Box(40, 30, 20) - Pos(15, 10, 0) * Box(20, 20, 30)
    return Pos(0, 30 + gap, 0) * body


def _assert_same_body_join(part) -> None:
    risers = recognise_risers(part)
    through_steps = recognise_through_steps(part)
    assert risers and through_steps
    through_keys = {step.body_key for step in through_steps}
    assert all(riser.body_key not in ((), None) for riser in risers)
    assert all(riser.body_key in through_keys for riser in risers)


def test_same_body_risers_and_through_steps_publish_one_join() -> None:
    risers = recognise_risers(_stair())
    through_steps = recognise_through_steps(_stair())

    _assert_same_body_join(_stair())
    assert {riser.body_key for riser in risers} == {step.body_key for step in through_steps}
    assert [riser.to_dict()["body_key"] for riser in risers]
    assert [step.to_dict()["body_key"] for step in through_steps]


def test_quarter_millimetre_air_gap_keeps_remote_through_step_distinct() -> None:
    part = Compound(children=[_stair(), _remote_through_step(gap=0.25)])
    riser_keys = {riser.body_key for riser in recognise_risers(part)}
    through_keys = {step.body_key for step in recognise_through_steps(part)}

    assert len(riser_keys) == 1
    assert len(through_keys) == 2
    assert riser_keys < through_keys


def test_exact_touch_without_shared_solid_does_not_create_shared_identity() -> None:
    part = Compound(children=[_stair(), _remote_through_step(gap=0.0)])

    assert len(part.solids()) == 2
    assert len({step.body_key for step in recognise_through_steps(part)}) == 2
    _assert_same_body_join(part)


def test_framed_result_keeps_each_riser_joined_to_one_through_step_body() -> None:
    source = Compound(children=[_stair(), _remote_through_step(gap=0.25)])
    framed = build_framed_recognition_result(Pos(13, -7, 5) * source.rotate(Axis.X, 30))

    assert isinstance(framed, FramedRecognitionResult)
    through_keys = {step.body_key for step in framed.result.through_steps}
    assert len(through_keys) == 2
    assert framed.result.risers
    assert all(riser.body_key in through_keys for riser in framed.result.risers)


def test_coincident_equal_signatures_refuse_both_family_joins() -> None:
    first = _stair()
    part = Compound(children=[first, deepcopy(first)])

    risers = recognise_risers(part)
    through_steps = recognise_through_steps(part)
    assert risers and through_steps
    assert all(riser.body_key is None for riser in risers)
    assert all(step.body_key is None for step in through_steps)


def test_compound_body_joins_survive_step_round_trip(tmp_path: Path) -> None:
    source = Compound(children=[_stair(), _remote_through_step(gap=0.25)])
    path = tmp_path / "riser-through-step-compound.step"
    assert export_step(source, path)
    imported = import_step(path)

    assert recognise_risers(imported) == recognise_risers(source)
    assert recognise_through_steps(imported) == recognise_through_steps(source)
