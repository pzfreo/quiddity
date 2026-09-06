# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Internal shared axis and length conventions used by recognition records and patterns.

Two things live here because every recogniser needs them and none owns them: the stable
dominant-axis convention, and the length-tolerance form of ADR 0008.
"""

from __future__ import annotations

import math
from collections.abc import Sequence

from quiddity._body_identity import body_signature as body_signature
from quiddity._typing import Bounds, Span2, Vector3

_PLANE_AXES = {
    "x": ((0.0, 1.0, 0.0), (0.0, 0.0, 1.0)),
    "y": ((0.0, 0.0, 1.0), (1.0, 0.0, 0.0)),
    "z": ((1.0, 0.0, 0.0), (0.0, 1.0, 0.0)),
}

_DOMINANT_TIE_TOL = 1e-12

# Direction gates. A recogniser classifies a face by where its *unit* normal points, so these
# are direction cosines: dimensionless, and never scaled with the part (ADR 0008). They were
# spelled as bare 0.99 and 0.01 at a dozen sites across six modules, where the same number
# appearing twice could not be told from the same *decision* appearing twice.
#
# Naming convention for numeric constants in this package:
#
#   ``*_TOL`` / ``*_MARGIN``   a length, in model units — must scale, per ADR 0008
#   ``*_FRAC`` / ``*_RATIO``   dimensionless proportion of something named
#   ``*_COS``                  a direction cosine — a unit-vector component
#   ``*_ANGLE``                an angle, in the unit the name or comment states
#   ``*_EPS``                  a float-comparison epsilon, not a physical allowance
#
# Record rounding (``round(x, 3)``, ``round(x, 4)``, ``FLOAT_DIGITS``) is none of these: it is
# the public record contract of ADR 0002 and the schema of ADR 0005, and is left alone.

#: A unit-vector component at or above this means the direction *is* that axis.
AXIS_ALIGNED_COS = 0.99

#: A unit-vector component at or below this means the direction has no part along that axis.
#: Deliberately not ``1 - AXIS_ALIGNED_COS``: "points along" and "has no component along" are
#: separate decisions, and a face may satisfy neither.
AXIS_ZERO_COS = 0.01

#: How far in from a corner to probe when asking which side of a face holds material. A
#: fraction of the distance to the face centre, so it follows the face rather than the part.
#: Shared by the chamfer and fillet recognisers, which ask the same question the same way.
INTERIOR_PROBE_FRAC = 0.05

#: Two faces count as *smoothly* joined when their outward normals agree to within this much of
#: parallel, measured as ``1 - dot`` where they meet. A face split by a neighbouring feature is
#: the exact case, its halves coplanar; a tangential blend is the other.
#:
#: **Measured, not chosen.** Over the checked-in STEP corpus the distribution is bimodal with a
#: four-order-of-magnitude empty band between the modes: 114 arcs sit at ``<= 1e-12`` -- tangency
#: is *exact*, on imported geometry as much as constructed -- and the nearest non-tangent arc is
#: at ``1e-3``, about 2.6 degrees. Nothing lies between. This sits in that band, three orders
#: above float noise and six below the nearest real corner.
#:
#: An earlier value left a gap of ``5e-4`` -- a cosine of 0.9995, about 1.81 degrees -- picked
#: on the assumption that a kernel
#: boolean leaves noise at these joins. It does not -- the tangencies are exact -- and at 1.8
#: degrees a genuinely shallow step would have read as smooth, which is the one false positive
#: this attribute must not produce.
#:
#: Dimensionless, so it never scales with the part (ADR 0008).
SMOOTH_ARC_GAP = 1e-9


def _unit(v: Sequence[float]) -> tuple[float, float, float]:
    """Normalise negative zeros out of a direction 3-vector.

    Unpacked rather than built by comprehension so the return type is the three floats a record
    axis actually requires. Every caller passes a direction, so a different length is a
    programming error and the unpacking says so.
    """

    x, y, z = (0.0 if c == 0 else c for c in v)
    return (x, y, z)


def _axis_letter_of(axis: Sequence[float]) -> str:
    """Return a platform-stable dominant axis, preferring Z then Y for numerical ties."""

    components = tuple(abs(float(component)) for component in axis)
    if len(components) != 3:
        raise ValueError("axis must be a 3-vector")
    peak = max(components)
    # OCCT can perturb an exact diagonal by a final bit in opposite directions on Windows and
    # Unix. The routing letter is discrete, so resolve numerical ties explicitly instead of
    # allowing that insignificant noise to select a different feature universe.
    return next(
        letter
        for letter, component in reversed(tuple(zip("xyz", components, strict=True)))
        if peak - component <= _DOMINANT_TIE_TOL
    )


def plane_axes(axis: str | Sequence[float]) -> tuple[Vector3, Vector3]:
    """Return the stable right-handed in-plane basis for an axis letter or vector."""

    letter = axis if isinstance(axis, str) else _axis_letter_of(axis)
    return _PLANE_AXES[letter]


def _axis_direction_components(
    axis: str, direction: Sequence[float] | None = None
) -> tuple[tuple[float, ...], float, int]:
    if axis not in "xyz" or len(axis) != 1:
        raise ValueError("axis must be 'x', 'y', or 'z'")
    if direction is None:
        direction = tuple(1.0 if letter == axis else 0.0 for letter in "xyz")
    try:
        raw = tuple(float(component) for component in direction)
    except (TypeError, ValueError) as exc:
        raise ValueError("axis_direction must be a 3-vector") from exc
    if len(raw) != 3:
        raise ValueError("axis_direction must be a 3-vector")
    if not all(math.isfinite(component) for component in raw):
        raise ValueError("axis_direction must contain only finite values")
    norm = math.hypot(*raw)
    if norm <= 1e-12:
        raise ValueError("axis_direction must be non-zero")
    index = "xyz".index(axis)
    if abs(raw[index]) + norm * 1e-9 < max(abs(component) for component in raw):
        raise ValueError(f"axis_direction's dominant component must match axis={axis!r}")
    return raw, norm, index


def _normalised_axis_direction(axis: str, direction: Sequence[float] | None = None) -> Vector3:
    raw, norm, index = _axis_direction_components(axis, direction)
    sign = -1.0 if raw[index] < 0 else 1.0
    return (sign * raw[0] / norm, sign * raw[1] / norm, sign * raw[2] / norm)


def _canonical_axis_direction(axis: str, direction: Sequence[float] | None = None) -> Vector3:
    raw, norm, index = _axis_direction_components(axis, direction)
    sign = -1.0 if raw[index] < 0 else 1.0
    unit = (
        tuple(sign * component for component in raw)
        if abs(norm - 1.0) <= 2e-6
        else _normalised_axis_direction(axis, raw)
    )
    rounded = tuple(0.0 if abs(component) < 0.5e-6 else round(component, 6) for component in unit)
    return (rounded[0], rounded[1], rounded[2])


def _canonical_axis_span(
    axis: str, direction: Sequence[float] | None, span: Sequence[float]
) -> Span2:
    raw, _norm, index = _axis_direction_components(axis, direction)
    lo, hi = (float(value) for value in span)
    if raw[index] < 0:
        lo, hi = -hi, -lo
    return (round(lo, 3), round(hi, 3))


def _axis_line_coordinates(
    axis: str, point: Sequence[float], direction: Sequence[float] | None = None
) -> Span2:
    px, py, pz = (float(component) for component in point)
    vector = _normalised_axis_direction(axis, direction)
    along = px * vector[0] + py * vector[1] + pz * vector[2]
    foot = tuple(
        component - along * delta for component, delta in zip((px, py, pz), vector, strict=True)
    )
    keep = [index for index, letter in enumerate("xyz") if letter != axis]
    coordinates = tuple(round(foot[index], 3) for index in keep)
    return (
        0.0 if coordinates[0] == 0 else coordinates[0],
        0.0 if coordinates[1] == 0 else coordinates[1],
    )


def _coaxial_axis_lines(
    point_a: Sequence[float],
    direction_a: Sequence[float],
    point_b: Sequence[float],
    direction_b: Sequence[float],
    *,
    tol: float,
) -> bool:
    """Whether two unit-direction axis lines are parallel and within *tol*.

    A cone or torus adjacent to an external cylinder is not thereby a *turned* transition:
    intersecting round features can share an edge while their axes differ. Turned evidence
    requires the analytic surface and the stock cylinder to revolve about the same line.
    """

    da = tuple(float(component) for component in direction_a)
    db = tuple(float(component) for component in direction_b)
    if len(da) != 3 or len(db) != 3:
        raise ValueError("directions must be 3-vectors")
    if abs(sum(da[i] * db[i] for i in range(3))) < 1 - 1e-6:
        return False
    offset = tuple(float(point_a[i]) - float(point_b[i]) for i in range(3))
    along = sum(offset[i] * db[i] for i in range(3))
    distance_sq = sum((offset[i] - along * db[i]) ** 2 for i in range(3))
    return bool(distance_sq <= tol**2)


def _axis_direction_is_aligned(
    axis: str, direction: Sequence[float] | None, *, tol: float = 1e-3
) -> bool:
    vector = _canonical_axis_direction(axis, direction)
    index = "xyz".index(axis)
    return abs(vector[index] - 1.0) <= tol and all(
        abs(vector[other]) <= tol for other in range(3) if other != index
    )


#: Relative band within which two independently-derived magnitudes are the same magnitude.
#: An area is a product of coordinates, so the same nominal quantity computed two ways differs
#: in its last bits — and by a *different* amount for the same part modelled at another scale.
#: A discrete gate compared at exact equality then answers differently for the same geometry.
_MAGNITUDE_TIE_EPS = 1e-9


def clears_threshold(magnitude: float, threshold: float) -> bool:
    """True when *magnitude* is above *threshold*, with an exact tie resolved as below it.

    ``area >= min_area_frac * cross`` looks like a decision about the part and is really a
    decision about the last few bits of two independently-computed products. In the chamfer
    golden a face area is exactly 40% of the cross-section: the difference is ``-1.7e-13`` at
    1x, exactly zero at 5x and 10x, and ``-9.3e-10`` at 100x, so a plate appeared at two scales
    and nowhere else.

    A magnitude sitting exactly on its threshold is not evidence of a feature, so the tie
    resolves against admitting one. What matters more than the direction is that it resolves
    the *same* way at every scale.

    This is the area-gate counterpart of the dominant-axis tie break in :func:`_axis_letter_of`:
    a discrete classification must not be selected by insignificant noise.
    """

    return magnitude >= threshold * (1.0 + _MAGNITUDE_TIE_EPS)


def cluster_coordinates(coordinates: Sequence[float], *, tol: float) -> list[list[int]]:
    """Group *coordinates* into clusters no wider than *tol*, returned as index lists.

    The obvious alternative — ``round(x / tol) * tol`` as a dict key — groups by which grid
    cell a coordinate lands in rather than by how far apart two coordinates are. That merges a
    pair ``tol`` apart when they share a cell and splits a pair a thousandth apart when they
    straddle a boundary, and shifts arbitrarily when the model is scaled.

    Clusters are bounded rather than single-linkage: a coordinate opens a new cluster once it
    is more than *tol* from the cluster's lowest member, so every member is within *tol* of
    every other. Single linkage would chain — a fine staircase of 0.4 mm risers judged at
    ``tol=0.5`` would collapse into one level spanning the whole part.

    Index lists come back in ascending coordinate order, and ties keep their input order, so
    the grouping does not depend on the order faces were traversed in.
    """

    order = sorted(range(len(coordinates)), key=lambda index: (coordinates[index], index))
    clusters: list[list[int]] = []
    for index in order:
        if clusters and coordinates[index] - coordinates[clusters[-1][0]] <= tol:
            clusters[-1].append(index)
        else:
            clusters.append([index])
    return clusters


def part_scale(bbox: Bounds) -> float:
    """Return a solid's characteristic length: the largest extent of its bounding box.

    The reference length for tolerances that compare two coordinates with no smaller feature
    to measure against — a level bucket, a floor-plane coincidence, a merge radius. Takes the
    bounding box rather than the part because every caller already holds one, and asking for a
    second is both a wasted traversal and a chance for the two to disagree.
    """

    return max(float(bbox.size.X), float(bbox.size.Y), float(bbox.size.Z))


#: The absolute band below which two coordinates are the same coordinate, and the default
#: ``floor`` of a feature-relative gate. It models float and kernel noise only — OCCT settles
#: boolean-cut coordinates to around 1e-7 — not any manufacturing allowance, so it is the same
#: number everywhere and does not vary by recogniser. A gate whose floor needs to be larger than
#: this is expressing something physical and must say what, per ADR 0008.
COORD_FLOOR = 1e-6


def quantise(value: float, *, figures: int = 6) -> float:
    """*value* rounded to *figures* significant figures.

    The scale-free form of ``round(value, n)``. A decimal quantum is a length, so it is 0.16% of
    a 6.25 mm band and 0.8% of the same band on a part modelled at a twentieth — which is how
    ``analyse_cylinders`` came to report a Ø6.25 band as Ø0.31. Significant figures keep the
    quantum proportional to what is being measured, and unlike a relative *tolerance* they still
    form a grid, so a value quantised this way can be used as a grouping key.

    Not a tolerance and not a minimum-evidence threshold, so ADR 0008 does not classify it: it is
    a precision floor on a measured value, whose job is to make two derivations of one length
    compare equal despite kernel noise.

    **The guarantee is a relative error bound, not commutativity with scaling.** An earlier
    version of this docstring claimed the latter — ``quantise(quantise(v) * k) == quantise(v * k)``
    — and it is false for about a fifth of values: rounding to a grid before multiplying can land
    either side of the next grid line, so 57.9998… × 5 gives 290.0 one way and 289.999 the other.
    Both are within the bound; neither is more correct. The property that holds, and the one the
    substrate needs, is that the error never exceeds ``10**-figures`` of the value at any scale.
    """

    return float(f"{value:.{figures}g}")


def length_tol(nominal: float, *, rel: float, floor: float = COORD_FLOOR) -> float:
    """Return a length tolerance proportional to *nominal* but never below *floor*.

    The package's one tolerance form, per ADR 0008. *rel* carries the part of the allowance
    that grows with the thing being measured — machining and modelling error both scale with
    size — and *floor* carries the part that does not: the absolute noise band below which two
    coordinates are the same coordinate.

    The terms add rather than taking a maximum. A maximum discards one term below
    ``floor / rel``, so every feature smaller than that gets exactly the floor and its own size
    stops mattering — the same scale-blindness, moved to the small end.

    *nominal* is a diameter, radius, width or envelope extent, and is therefore non-negative.
    A negative value would silently *tighten* the gate below its floor, which no caller can
    mean, so it is a programming error and raises.

    A NaN nominal is deliberately not rejected. It means the geometry that produced it is
    malformed, and this package fails closed on malformed evidence rather than raising: the NaN
    propagates, every comparison against it is false, and the candidate is refused.
    """

    if nominal < 0.0:
        raise ValueError("nominal must be a non-negative length")
    return rel * nominal + floor
