# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Kernel-free cylindrical end values and complete polygon-domain bounds.

The cylinder axis lies in the section plane. Heights are measured along frame.run.
This value is not yet registered in the public SectionEnd contract.
"""

from __future__ import annotations

import math
from dataclasses import dataclass

from quiddity._record import Record


def _finite(values: tuple[float, ...]) -> bool:
    return all(
        not isinstance(v, bool) and isinstance(v, int | float) and math.isfinite(v) for v in values
    )


@dataclass(frozen=True, order=True, slots=True)
class CylindricalEndSurface(Record):
    """One explicit branch of a cylinder perpendicular to the section run.

    The axis point is the point on the axis closest to the section-frame origin.
    Direction is normalized on interpretation after six-decimal serialization.
    """

    type: str
    axis_point: tuple[float, float, float]
    axis_direction: tuple[float, float]
    radius: float
    branch: str

    def __post_init__(self) -> None:
        if self.type != "cylinder" or self.branch not in {"positive", "negative"}:
            raise ValueError("cylindrical end needs an explicit cylinder type and branch")
        if (
            not isinstance(self.axis_point, tuple)
            or len(self.axis_point) != 3
            or not isinstance(self.axis_direction, tuple)
            or len(self.axis_direction) != 2
            or not _finite((*self.axis_point, *self.axis_direction, self.radius))
            or self.radius <= 0
        ):
            raise ValueError("cylindrical end needs finite axis values and positive radius")
        values = (*self.axis_point, *self.axis_direction, self.radius)
        if any(round(v, 6) != v for v in values):
            raise ValueError("cylindrical end values serialize at six decimal places")
        norm = math.hypot(*self.axis_direction)
        if abs(norm - 1) > 2e-6:
            raise ValueError("cylinder axis direction must be unit length")
        dominant = max(range(2), key=lambda i: (abs(self.axis_direction[i]), i))
        if self.axis_direction[dominant] <= 0:
            raise ValueError("cylinder axis direction must have canonical sign")
        if (
            abs(sum(a * b for a, b in zip(self.axis_point[:2], self.axis_direction, strict=True)))
            > 2e-6
        ):
            raise ValueError("cylinder axis point must be closest to the frame origin")

    def _offset(self, point: tuple[float, float]) -> float:
        if len(point) != 2 or not _finite(point):
            raise ValueError("section point must contain two finite values")
        x, y = self.axis_direction
        return (
            -y * (point[0] - self.axis_point[0]) + x * (point[1] - self.axis_point[1])
        ) / math.hypot(x, y)

    def _height(self, offset: float) -> float:
        # Factored discriminant avoids cancellation near the branch boundary.
        discriminant = (self.radius - abs(offset)) * (self.radius + abs(offset))
        if discriminant <= 0:
            raise ValueError("cylinder branch must exist strictly over the complete profile")
        sign = 1 if self.branch == "positive" else -1
        return self.axis_point[2] + sign * math.sqrt(discriminant)

    def height(self, point: tuple[float, float]) -> float:
        return self._height(self._offset(point))

    def polygon_height_bounds(self, points: tuple[tuple[float, float], ...]) -> tuple[float, float]:
        """Exact height bounds for an already validated closed line-only polygon.

        The transverse coordinate is affine, so its extrema occur at vertices.
        The interior may cross the cylinder crest even when no vertex lies there.
        """
        if len(points) < 3:
            raise ValueError("height bounds need a complete polygon")
        offsets = tuple(self._offset(point) for point in points)
        low, high = min(offsets), max(offsets)
        farthest = max(abs(low), abs(high))
        nearest = 0.0 if low <= 0 <= high else min(abs(low), abs(high))
        heights = (self._height(farthest), self._height(nearest))
        return min(heights), max(heights)
