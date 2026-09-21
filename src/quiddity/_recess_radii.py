# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Fail-closed radius proofs for legacy principal-axis recess records.

An obround end radius and a rounded-rectangle corner radius are different geometry.  The
obround proof remains in :mod:`quiddity._recess_obround`; this module proves only the latter:
four equal concave quarter cylinders at the four flat-wall junctions, spanning the record's
complete depth.  Anything partial, mixed-radius, misplaced or ambiguous remains unknown.
"""

from __future__ import annotations

from dataclasses import replace
from itertools import product
from types import SimpleNamespace
from typing import TypeVar

from quiddity._recess_faces import _AXES, _union_bb
from quiddity._recess_records import Pocket, Slot
from quiddity._typing import Bounds

# Legacy recess coordinates and dimensions are published at two decimal places.  This bound
# admits their maximum independent rounding displacement without inheriting the much wider
# topology merge tolerance used to combine interrupted wall candidates.
_PUBLISHED_COORD_TOL = 0.011

# A quarter cylinder spans one radius on both footprint axes.  Keep the ratio bound aligned
# with the established half-cylinder shape proof in `_recess_obround`.
_QUARTER_RATIO_TOL = 0.1

_RadiusRecord = TypeVar("_RadiusRecord", Slot, Pocket)


def _close(left: float, right: float) -> bool:
    return abs(left - right) <= _PUBLISHED_COORD_TOL


def _axis_bounds(bounds: Bounds | SimpleNamespace, axis: str) -> tuple[float, float]:
    coordinate = "XYZ"[_AXES[axis]]
    return (
        float(getattr(bounds.min, coordinate)),
        float(getattr(bounds.max, coordinate)),
    )


def _same_span(actual: tuple[float, float], expected: tuple[float, float]) -> bool:
    return _close(actual[0], expected[0]) and _close(actual[1], expected[1])


def _candidate_radii(cylinders: list[tuple], record: Slot | Pocket) -> tuple[float, ...]:
    radii: list[float] = []
    for radius, axis, _location, bounds, concave, _node in cylinders:
        if (
            not concave
            or axis != record.depth_axis
            or radius <= 0
            or not _same_span(_axis_bounds(bounds, axis), (record.d_lo, record.d_hi))
        ):
            continue
        if not any(_close(radius, existing) for existing in radii):
            radii.append(float(radius))
    return tuple(radii)


def _site_bounds(
    record: Slot | Pocket,
    radius: float,
    long_side: int,
    width_side: int,
) -> dict[str, tuple[float, float]]:
    long_bounds = (
        (record.lo - radius, record.lo) if long_side < 0 else (record.hi, record.hi + radius)
    )
    width_low = record.w_center - record.width / 2
    width_high = record.w_center + record.width / 2
    width_bounds = (
        (width_low, width_low + radius) if width_side < 0 else (width_high - radius, width_high)
    )
    return {
        record.long_axis: long_bounds,
        record.width_axis: width_bounds,
        record.depth_axis: (record.d_lo, record.d_hi),
    }


def _site_matches(
    cylinders: list[tuple],
    record: Slot | Pocket,
    radius: float,
    long_side: int,
    width_side: int,
) -> bool:
    expected = _site_bounds(record, radius, long_side, width_side)
    long_center = record.lo if long_side < 0 else record.hi
    width_center = record.w_center + width_side * (record.width / 2 - radius)
    li = _AXES[record.long_axis]
    wi = _AXES[record.width_axis]
    matches = [
        item
        for item in cylinders
        if item[4]
        and item[1] == record.depth_axis
        and _close(float(item[0]), radius)
        and _close(float(item[2][li]), long_center)
        and _close(float(item[2][wi]), width_center)
        and _same_span(_axis_bounds(item[3], record.depth_axis), expected[record.depth_axis])
    ]
    if not matches:
        return False

    combined = matches[0][3]
    for item in matches[1:]:
        combined = _union_bb(combined, item[3])
    if not all(_same_span(_axis_bounds(combined, axis), span) for axis, span in expected.items()):
        return False
    footprint_extents = [
        _axis_bounds(combined, axis)[1] - _axis_bounds(combined, axis)[0]
        for axis in (record.long_axis, record.width_axis)
    ]
    return all(abs(extent / radius - 1.0) <= _QUARTER_RATIO_TOL for extent in footprint_extents)


def _proved_corner_radius(record: Slot | Pocket, cylinders: list[tuple]) -> float | None:
    """Return a uniform four-corner radius proved by original cylindrical source patches.

    ``None`` means only that the uniform proof did not succeed.  It does not assert sharp
    corners: partial rounds, mixed radii and unsupported topology deliberately share that
    fail-closed result.
    """

    if isinstance(record, Pocket) and record.edge_anchored:
        return None
    proved: list[float] = []
    for radius in _candidate_radii(cylinders, record):
        # At half-width the footprint is an obround/stadium, whose two semicircular ends are
        # intentionally represented by the separate end-radius proof.
        if radius >= record.width / 2 - _PUBLISHED_COORD_TOL:
            continue
        if all(
            _site_matches(cylinders, record, radius, long_side, width_side)
            for long_side, width_side in product((-1, 1), repeat=2)
        ):
            published = round(radius, 2)
            if published > 0 and published not in proved:
                proved.append(published)
    return proved[0] if len(proved) == 1 else None


def _with_proved_corner_radius(record: _RadiusRecord, cylinders: list[tuple]) -> _RadiusRecord:
    """Publish a proved uniform radius and restore the rounded rectangle's overall span.

    Wall pairing locates ``lo``/``hi`` at the four arc tangencies, so a rounded rectangle's
    raw length is the flat run.  The same proof that establishes all four corners authorises
    extending each end by one radius.  Unsupported, partial and mixed-radius topology remains
    byte-for-byte unchanged with ``corner_radius=None``.
    """

    radius = _proved_corner_radius(record, cylinders)
    if radius is None:
        return record
    return replace(
        record,
        lo=round(record.lo - radius, 2),
        hi=round(record.hi + radius, 2),
        length=round(record.hi - record.lo + 2 * radius, 2),
        corner_radius=radius,
    )
