# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Kernel-free values for the bounded within-run outer-profile inspection contract."""

from __future__ import annotations

import math
from dataclasses import dataclass, field
from enum import Enum
from typing import Literal

from quiddity._record import Record

Point3 = tuple[float, float, float]


def _point(value: Point3) -> None:
    if not isinstance(value, tuple) or len(value) != 3 or not all(math.isfinite(v) for v in value):
        raise ValueError("profile coordinates must be finite three-tuples")


@dataclass(frozen=True, slots=True)
class ProfileLine(Record):
    """An oriented finite straight support, traversed from start to end."""

    start: Point3
    end: Point3
    kind: Literal["line"] = field(default="line", init=False)

    def __post_init__(self) -> None:
        _point(self.start)
        _point(self.end)
        if math.dist(self.start, self.end) == 0:
            raise ValueError("profile line must have nonzero length")

    @property
    def direction(self) -> Point3:
        length = math.dist(self.start, self.end)
        return tuple((b - a) / length for a, b in zip(self.start, self.end, strict=True))  # type: ignore[return-value]


@dataclass(frozen=True, slots=True)
class ProfileArc(Record):
    """A finite circular arc; signed sweep is radians around the profile normal."""

    start: Point3
    end: Point3
    center: Point3
    radius: float
    sweep: float
    kind: Literal["arc"] = field(default="arc", init=False)

    def __post_init__(self) -> None:
        for point in (self.start, self.end, self.center):
            _point(point)
        if not math.isfinite(self.radius) or self.radius <= 0:
            raise ValueError("profile arc radius must be positive and finite")
        if not math.isfinite(self.sweep) or not 0 < abs(self.sweep) < 2 * math.pi:
            raise ValueError("profile arc must have a finite nonzero partial-circle sweep")


@dataclass(frozen=True, slots=True)
class PlanarOuterProfile(Record):
    """Schema 1: one convex outer wire, oriented with material left of traversal.

    ``normal`` is the supporting face's outward normal. Coordinates are unrounded,
    in the evidence view's space. Consecutive supports (including last/first) meet
    at their finite endpoints. Circular transitions remain explicit; their adjacent
    lines can be extended to derive a virtual intersection without inventing a vertex.
    A serialized value carries geometry only, never source or body identity.
    """

    origin: Point3
    normal: Point3
    supports: tuple[ProfileLine | ProfileArc, ...]
    inner_loop_count: int = 0
    schema_version: Literal[1] = field(default=1, init=False)
    boundary_kind: Literal["outer"] = field(default="outer", init=False)

    def __post_init__(self) -> None:
        _point(self.origin)
        _point(self.normal)
        if type(self.inner_loop_count) is not int or self.inner_loop_count < 0:
            raise ValueError("inner loop count must be a nonnegative integer")
        if abs(math.hypot(*self.normal) - 1) > 1e-8:
            raise ValueError("profile normal must have unit length")
        if (
            not isinstance(self.supports, tuple)
            or sum(isinstance(s, ProfileLine) for s in self.supports) < 2
        ):
            raise ValueError("profile requires at least two finite line supports")
        for at, support in enumerate(self.supports):
            if not isinstance(support, ProfileLine | ProfileArc):
                raise TypeError("profile supports must be lines or circular arcs")
            if support.end != self.supports[(at + 1) % len(self.supports)].start:
                raise ValueError("profile supports must form one exactly connected closed wire")


class OuterProfileRefusalReason(Enum):
    """Closed unsupported outcomes for planar outer-profile inspection."""

    NOT_PLANAR = "not_planar"
    AMBIGUOUS_BODY = "ambiguous_body"
    UNSUPPORTED_CURVE = "unsupported_curve"
    INSUFFICIENT_LINE_SUPPORTS = "insufficient_line_supports"
    CONCAVE_PROFILE = "concave_profile"
    INVALID_BOUNDARY = "invalid_boundary"


@dataclass(frozen=True, slots=True)
class RefusedPlanarOuterProfile:
    reason: OuterProfileRefusalReason
