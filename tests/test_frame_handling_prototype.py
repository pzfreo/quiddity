# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Decision tests for the part-relative frame-handling prototype."""

from __future__ import annotations

from build123d import Axis, Box, Sphere

from quiddity.frames import (
    FrameRefusalReason,
    PartFrame,
    RefusedPartFrame,
    infer_part_frame,
)


def test_frame_inference_tracks_a_rigidly_rotated_prism() -> None:
    frame = infer_part_frame(Box(10, 20, 30).rotate(Axis.X, 30))

    assert isinstance(frame, PartFrame)
    axes = (frame.x, frame.y, frame.z)
    assert all(
        abs(sum(left * right for left, right in zip(axes[i], axes[j], strict=True))) < 1e-12
        for i, j in ((0, 1), (0, 2), (1, 2))
    )


def test_frame_inference_refuses_geometry_with_no_direction_evidence() -> None:
    assert infer_part_frame(Sphere(10)) == RefusedPartFrame(
        FrameRefusalReason.NO_ANALYTIC_DIRECTION
    )
