# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

from __future__ import annotations

import math
from pathlib import Path

import pytest

from quiddity import (
    BoltCircle,
    HoleRecord,
    LinearArray,
    RectGrid,
    import_step_geometry,
    recognise_hole_patterns,
    recognise_holes,
)
from quiddity._pattern_geometry import _plane_uv

ROOT = Path(__file__).parents[1]
SPOOL = ROOT / "tests/corpus/cadgenbench/flanged_spool_132.step"


def _hole(location, *, axis=(0.0, 0.0, -1.0), diameter=8.0, depth=5.0) -> HoleRecord:
    return HoleRecord(
        axis=axis,
        location=location,
        diameter=diameter,
        depth=depth,
        bottom="through",
    )


def _circle(*, z: float, scale: float = 1.0) -> list[HoleRecord]:
    return [
        _hole(
            (
                55.0 * scale * math.cos(index * math.tau / 12),
                55.0 * scale * math.sin(index * math.tau / 12),
                z,
            ),
            diameter=8.0 * scale,
            depth=5.0 * scale,
        )
        for index in range(12)
    ]


@pytest.mark.parametrize("scale", [0.05, 1.0, 20.0])
def test_equal_bolt_circles_on_separate_opening_planes_remain_distinct(scale: float) -> None:
    holes = _circle(z=0.0, scale=scale) + _circle(z=108.0 * scale, scale=scale)

    patterns = recognise_hole_patterns(holes)

    assert len(patterns) == 2
    assert all(isinstance(pattern, BoltCircle) for pattern in patterns)
    assert sorted(len(pattern.holes) for pattern in patterns) == [12, 12]
    assert {hole for pattern in patterns for hole in pattern.holes} == set(holes)
    assert sorted(round(pattern.center[2], 8) for pattern in patterns) == [0.0, 108.0 * scale]


def test_equal_grids_on_separate_opening_planes_remain_distinct() -> None:
    holes = [
        _hole((float(column * 10), float(row * 12), z))
        for z in (0.0, 40.0)
        for row in range(2)
        for column in range(3)
    ]

    patterns = recognise_hole_patterns(holes)

    assert len(patterns) == 2
    assert all(isinstance(pattern, RectGrid) for pattern in patterns)
    assert sorted(len(pattern.holes) for pattern in patterns) == [6, 6]
    assert {hole for pattern in patterns for hole in pattern.holes} == set(holes)


def test_oblique_translated_opening_planes_are_traversal_invariant() -> None:
    length = math.sqrt(14.0)
    axis = (1.0 / length, 2.0 / length, 3.0 / length)
    u, v = _plane_uv(axis)
    translation = (1_000_000.0, -2_000_000.0, 3_000_000.0)
    holes = []
    for plane in (0.0, 75.0):
        for index in range(6):
            angle = index * math.tau / 6
            axial_noise = (1.0 if index % 2 == 0 else -1.0) * 1e-5
            holes.append(
                _hole(
                    tuple(
                        translation[coordinate]
                        + (plane + axial_noise) * axis[coordinate]
                        + 30.0 * math.cos(angle) * u[coordinate]
                        + 30.0 * math.sin(angle) * v[coordinate]
                        for coordinate in range(3)
                    ),
                    axis=axis,
                )
            )

    def signature(records):
        return sorted(
            (
                len(pattern.holes),
                round(pattern.diameter, 6),
                round(
                    sum(
                        (pattern.center[index] - translation[index]) * axis[index]
                        for index in range(3)
                    ),
                    6,
                ),
            )
            for pattern in records
            if isinstance(pattern, BoltCircle)
        )

    assert (
        signature(recognise_hole_patterns(holes))
        == signature(recognise_hole_patterns(list(reversed(holes))))
        == [(6, 60.0, 0.0), (6, 60.0, 75.0)]
    )


def test_linear_array_may_cross_opening_planes() -> None:
    holes = [_hole((5.0, -3.0, z)) for z in (0.0, 10.0, 20.0, 30.0)]

    assert recognise_hole_patterns(holes) == [
        LinearArray(
            holes=tuple(holes),
            pitch=10.0,
            direction=(0.0, 0.0, 1.0),
        )
    ]


def test_flanged_spool_retains_both_bolt_circles() -> None:
    holes = recognise_holes(import_step_geometry(SPOOL))

    circles = [
        pattern for pattern in recognise_hole_patterns(holes) if isinstance(pattern, BoltCircle)
    ]

    assert sorted(
        (round(circle.diameter), len(circle.holes), round(circle.center[2])) for circle in circles
    ) == [(90, 6, 139), (110, 12, 5), (110, 12, 113)]
