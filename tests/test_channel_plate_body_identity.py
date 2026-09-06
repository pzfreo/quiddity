"""Public Channel/Plate ownership for compound-safe downstream selection (#390)."""

from __future__ import annotations

from collections import defaultdict
from pathlib import Path

from build123d import Box, Compound, Pos, Rot, export_step, import_step

from quiddity import (
    FramedRecognitionResult,
    build_framed_recognition_result,
    recognise_plates,
)
from tools._legacy_recognition import (
    build_raw_recognition_result,
    recognise_channels,
)


def _u_channel():
    return (
        Box(50, 50, 12)
        + Pos(0, -18.75, 15) * Box(50, 12.5, 18)
        + Pos(0, 18.75, 15) * Box(50, 12.5, 18)
    )


def _monolithic_rebate():
    return Box(80, 60, 30) - Pos(0, 0, 7.5) * Box(80, 20, 15)


def _supported_body_keys(plates):
    axes = defaultdict(set)
    for plate in plates:
        if plate.body_key is not None:
            axes[plate.body_key].add(plate.axis)
    return {body_key for body_key, values in axes.items() if len(values) >= 2}


def test_compound_channel_support_is_body_local_and_aggregate_visible() -> None:
    valid = Pos(-80, 0, 0) * _u_channel()
    rebate = Pos(80, 0, 0) * _monolithic_rebate()
    part = Compound([valid, rebate])

    result = build_raw_recognition_result(part)
    assert len(result.channels) == 2
    assert all(channel.body_key not in ((), None) for channel in result.channels)

    supported = _supported_body_keys(result.plates)
    assert len(supported) == 1
    assert [channel.width for channel in result.channels if channel.body_key in supported] == [25.0]
    assert [channel.width for channel in result.channels if channel.body_key not in supported] == [
        20.0
    ]


def test_body_join_survives_compound_order_and_public_serialisation() -> None:
    valid = Pos(-80, 0, 0) * _u_channel()
    rebate = Pos(80, 0, 0) * _monolithic_rebate()

    observations = []
    for children in ([valid, rebate], [rebate, valid]):
        part = Compound(children)
        channels = recognise_channels(part)
        plates = recognise_plates(part)
        supported = _supported_body_keys(plates)
        observations.append(
            (
                [(channel.width, channel.body_key in supported) for channel in channels],
                [channel.to_dict()["body_key"] for channel in channels],
                [plate.to_dict()["body_key"] for plate in plates],
            )
        )
    assert observations[0] == observations[1]


def test_coincident_body_signatures_refuse_join_instead_of_using_order() -> None:
    first = _u_channel()
    second = _u_channel()
    part = Compound([first, second])

    assert recognise_channels(part)
    assert recognise_plates(part)
    assert all(channel.body_key is None for channel in recognise_channels(part))
    assert all(plate.body_key is None for plate in recognise_plates(part))


def test_translated_equal_bodies_have_distinct_joinable_keys() -> None:
    part = Compound([Pos(-60, 0, 0) * _u_channel(), Pos(60, 0, 0) * _u_channel()])
    channels = recognise_channels(part)
    plates = recognise_plates(part)

    assert len(channels) == 2
    assert len({channel.body_key for channel in channels}) == 2
    assert all(
        len({plate.axis for plate in plates if plate.body_key == channel.body_key}) >= 2
        for channel in channels
    )


def test_framed_rigid_motion_keeps_each_channel_plate_join_coherent() -> None:
    part = Rot(17, 23, 31) * Compound(
        [Pos(-80, 0, 0) * _u_channel(), Pos(80, 0, 0) * _monolithic_rebate()]
    )
    framed = build_framed_recognition_result(part)
    assert isinstance(framed, FramedRecognitionResult)

    channels = [
        r for r in framed.result.section_recesses if r.classification.feature_kind == "channel"
    ]
    assert len(channels) == 2
    assert {r.body for r in channels} == {0, 1}
    # Unified records join by result-local body index, not the old geometric body signature.
    assert all(r.evidence.defining_faces for r in channels)
    assert framed.result.section_recess_refusals == ()


def test_nested_single_solid_wrapper_uses_the_same_physical_scope() -> None:
    part = Compound([Compound([_u_channel()])])
    (channel,) = recognise_channels(part)
    plates = recognise_plates(part)

    assert channel.body_key not in ((), None)
    assert {plate.body_key for plate in plates} == {channel.body_key}


def test_body_join_is_exact_after_step_round_trip(tmp_path: Path) -> None:
    source = Rot(17, 23, 31) * _u_channel()
    path = tmp_path / "rotated-channel.step"
    assert export_step(source, path)

    imported = import_step(path)

    assert recognise_channels(imported) == recognise_channels(source)
    assert recognise_plates(imported) == recognise_plates(source)
