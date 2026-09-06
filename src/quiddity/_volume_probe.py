# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Policy-neutral volumetric evidence for axis-aligned candidate regions."""

from __future__ import annotations

from collections.abc import Iterable
from typing import Protocol, cast

from build123d import Box, Pos, Solid

from quiddity._typing import Part

#: OCCT cannot construct a volumetric probe at or below this coordinate extent.
PRISM_PROBE_FLOOR = 1e-6


class _VolumeValue(Protocol):
    @property
    def volume(self) -> float: ...


def intersection_volume(result: object) -> float:
    """Normalize empty, single-shape and fragmented boolean volumes without policy.

    Unexpected result types and kernel errors propagate to the caller's proof boundary.
    No tolerance, absolute value, clamping or fragment selection is applied here.
    """
    if result is None:
        return 0.0
    if hasattr(result, "volume"):
        return float(cast(_VolumeValue, result).volume)
    return sum(float(shape.volume) for shape in cast(Iterable[_VolumeValue], result))


def material_fraction(part: Part, probe: Solid) -> float:
    """Measure occupied fraction of the supplied probe; callers own admission policy."""
    return intersection_volume(part.intersect(probe)) / float(probe.volume)


def prism_material_fraction(
    spans: dict[str, tuple[float, float]], part: Part, *, inset: float
) -> float:
    """Return the fraction of an inset axis-aligned prism occupied by ``part``.

    This measures geometry only. Consumers separately own whether they require exact emptiness
    or permit a named material fraction, and they supply their own inset policy.
    """

    size: dict[str, float] = {}
    centre: dict[str, float] = {}
    for axis, (low, high) in spans.items():
        axis_inset = min(inset, (high - low) / 4)
        size[axis] = (high - low) - 2 * axis_inset
        centre[axis] = (low + high) / 2
    if min(size.values()) <= 0:
        raise ValueError("prism spans must have positive extent")
    if min(size.values()) <= PRISM_PROBE_FLOOR:
        # Nominally disjoint face bounds can overlap by a final bit while remaining below the
        # kernel's constructible-solid floor. Such a sliver cannot prove an empty region.
        return 1.0
    probe = Pos(centre["x"], centre["y"], centre["z"]) * Box(size["x"], size["y"], size["z"])
    intersection = part.intersect(probe)
    occupied = intersection_volume(intersection)
    return float(occupied / (size["x"] * size["y"] * size["z"]))


def prism_is_empty(spans: dict[str, tuple[float, float]], part: Part, *, inset: float) -> bool:
    """Whether the inset prism has exactly zero volumetric intersection with ``part``."""

    return prism_material_fraction(spans, part, inset=inset) == 0.0
