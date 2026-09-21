from __future__ import annotations

import json
from dataclasses import fields, replace

import pytest
from build123d import (
    Axis,
    Box,
    Pos,
    RectangleRounded,
    Rot,
    export_step,
    extrude,
    fillet,
    import_step,
)

from quiddity._adjacency import FaceGraph
from quiddity._recess_core import _pocket_proposals_one, _slot_proposals_one
from quiddity.result import build_recognition_result
from quiddity.slots import (
    Pocket,
    Slot,
    recognise_pocket_patterns,
    recognise_pockets,
    recognise_slot_patterns,
    recognise_slots,
)
from tests.golden._common import obround_tool


def _rounded_rectangle(radius: float, *, blind: bool, scale: float = 1.0):
    stock = Box(40 * scale, 30 * scale, 10 * scale)
    if blind:
        tool = extrude(RectangleRounded(20 * scale, 10 * scale, radius * scale), 5 * scale)
    else:
        tool = Pos(0, 0, -10 * scale) * extrude(
            RectangleRounded(20 * scale, 10 * scale, radius * scale), 20 * scale
        )
    return stock - tool


def _record(part, *, blind: bool) -> Slot | Pocket:
    graph = FaceGraph(part)
    proposals = (
        _pocket_proposals_one(part, graph=graph)
        if blind
        else _slot_proposals_one(part, graph=graph)
    )
    assert len(proposals) == 1
    return proposals[0].record


@pytest.mark.parametrize("blind", [False, True])
@pytest.mark.parametrize("radius", [1.0, 2.0, 3.0])
@pytest.mark.parametrize("scale", [0.1, 1.0, 10.0])
def test_uniform_four_corner_radius_is_scale_stable(blind, radius, scale) -> None:
    record = _record(_rounded_rectangle(radius, blind=blind, scale=scale), blind=blind)
    assert record.corner_radius == pytest.approx(radius * scale, abs=0.01)
    assert record.end_radius is None
    assert record.length == pytest.approx(20 * scale, abs=0.01)
    assert (record.lo, record.hi) == pytest.approx((-10 * scale, 10 * scale), abs=0.01)


@pytest.mark.parametrize("blind", [False, True])
def test_uniform_four_corner_radius_survives_step_round_trip(tmp_path, blind) -> None:
    path = tmp_path / ("rounded-pocket.step" if blind else "rounded-slot.step")
    export_step(_rounded_rectangle(2.0, blind=blind), path)
    record = _record(import_step(path), blind=blind)
    assert (record.corner_radius, record.length, record.lo, record.hi) == (2.0, 20.0, -10.0, 10.0)


@pytest.mark.parametrize("rotation", [Rot(90, 0, 0), Rot(0, 90, 0)])
def test_uniform_four_corner_radius_is_principal_axis_invariant(rotation) -> None:
    assert (
        _record(rotation * _rounded_rectangle(2.0, blind=False), blind=False).corner_radius == 2.0
    )


def test_square_and_obround_footprints_are_not_four_corner_proofs() -> None:
    square = Box(40, 30, 10) - Box(20, 10, 20)
    obround = Box(40, 30, 10) - obround_tool(20, 10, 20)
    square_record = _record(square, blind=False)
    obround_record = _record(obround, blind=False)
    assert (square_record.end_radius, square_record.corner_radius) == (None, None)
    assert (obround_record.end_radius, obround_record.corner_radius) == (5.0, None)
    assert (obround_record.length, obround_record.lo, obround_record.hi) == (20.0, -10.0, 10.0)


def test_blind_obround_publishes_end_radius_without_corner_radius() -> None:
    part = Box(80, 50, 14) - Pos(0, 0, 4) * obround_tool(30, 10, 10)
    record = recognise_pockets(part)[0]

    assert (record.end_radius, record.corner_radius) == (5.0, None)
    assert (record.length, record.lo, record.hi) == (30.0, -15.0, 15.0)


@pytest.mark.parametrize("rounded_corners", [1, 2, 3])
def test_partial_corner_sets_remain_unknown(rounded_corners) -> None:
    tool = Box(20, 10, 20)
    tool = fillet(list(tool.edges().filter_by(Axis.Z))[:rounded_corners], 2.0)
    record = _record(Box(40, 30, 10) - tool, blind=False)
    assert (record.end_radius, record.corner_radius) == (None, None)


@pytest.mark.parametrize("first_radius_corners", [1, 2])
def test_mixed_corner_radii_remain_unknown(first_radius_corners) -> None:
    tool = Box(20, 10, 20)
    tool = fillet(list(tool.edges().filter_by(Axis.Z))[:first_radius_corners], 1.0)
    tool = fillet(list(tool.edges().filter_by(Axis.Z)), 2.0)
    record = _record(Box(40, 30, 10) - tool, blind=False)
    assert (record.end_radius, record.corner_radius) == (None, None)


@pytest.mark.parametrize("blind", [False, True])
def test_direct_records_are_json_serializable_and_keep_unknown_distinct_from_square(blind) -> None:
    rounded = _rounded_rectangle(2.0, blind=blind)
    record = (recognise_pockets(rounded) if blind else recognise_slots(rounded))[0]
    payload = json.loads(json.dumps(record.to_dict(), allow_nan=False))

    assert payload["end_radius"] is None
    assert payload["corner_radius"] == 2.0
    assert payload["length"] == 20.0
    assert tuple(field.name for field in fields(record))[-2:] == ("end_radius", "corner_radius")


def test_aggregate_slot_retains_the_direct_radius_contract() -> None:
    part = _rounded_rectangle(2.0, blind=False)
    direct = recognise_slots(part)
    aggregate = build_recognition_result(part, rotational=False).slots

    assert [record.to_dict() for record in aggregate] == [record.to_dict() for record in direct]
    assert aggregate[0].corner_radius == 2.0


def test_historical_positional_construction_keeps_body_key_position() -> None:
    body_key = (1.0, 2.0)
    slot = Slot("y", "x", 10.0, 20.0, 0.0, -10.0, 10.0, -5.0, 5.0, body_key)
    pocket = Pocket("y", "x", 10.0, 20.0, 5.0, 0.0, -10.0, 10.0, 0.0, 5.0, 1, False, body_key)

    assert slot.body_key == pocket.body_key == body_key
    assert (slot.end_radius, slot.corner_radius) == (None, None)
    assert (pocket.end_radius, pocket.corner_radius) == (None, None)


def _pattern_slots(radius: float | None) -> list[Slot]:
    return [
        Slot("y", "x", 10.0, 20.0, center, -10.0, 10.0, -5.0, 5.0, (), None, radius)
        for center in (0.0, 20.0, 40.0)
    ]


def _pattern_pockets(radius: float | None) -> list[Pocket]:
    return [
        Pocket(
            "y",
            "x",
            10.0,
            20.0,
            5.0,
            center,
            -10.0,
            10.0,
            0.0,
            5.0,
            1,
            False,
            (),
            None,
            radius,
        )
        for center in (0.0, 20.0, 40.0)
    ]


@pytest.mark.parametrize(
    ("records", "recognise"),
    [(_pattern_slots, recognise_slot_patterns), (_pattern_pockets, recognise_pocket_patterns)],
)
def test_pattern_identity_includes_corner_radius(records, recognise) -> None:
    uniform = records(2.0)
    assert len(recognise(uniform)) == 1

    mixed = [*uniform[:2], records(3.0)[2]]
    assert recognise(mixed) == []

    end_uniform = [replace(record, corner_radius=None, end_radius=5.0) for record in records(None)]
    assert len(recognise(end_uniform)) == 1

    end_mixed = [*end_uniform[:2], replace(end_uniform[2], end_radius=4.0)]
    assert recognise(end_mixed) == []
