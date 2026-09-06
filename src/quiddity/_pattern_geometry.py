# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Record-agnostic deterministic 2-D pattern geometry."""

import math
from collections.abc import Callable, Sequence
from typing import TypeVar

from quiddity._geometry import _unit, length_tol, plane_axes

#: The record type a caller's ``make`` builds. This module owns the collinearity, pitch and
#: lattice geometry and nothing about what a pattern record *is* — holes, pockets and slots each
#: pass their own constructor — so the return type follows that constructor rather than being
#: widened to a common base the module would then have to know about.
_R = TypeVar("_R")

_PATTERN_REL_TOL = 0.02
_PATTERN_ABS_TOL = 0.1


def _pattern_tol(nominal: float) -> float:
    return length_tol(nominal, rel=_PATTERN_REL_TOL, floor=_PATTERN_ABS_TOL)


def _project_out(w, *directions) -> tuple[float, ...] | None:
    """Remove every orthogonal unit *direction* from *w*, then renormalise."""
    for d in directions:
        k = sum(p * q for p, q in zip(w, d, strict=True))
        w = tuple(p - k * q for p, q in zip(w, d, strict=True))
    n = math.hypot(*w)
    return tuple(c / n for c in w)


def _plane_uv(axis) -> tuple[tuple[float, ...], tuple[float, ...]]:
    """Two orthonormal vectors spanning the plane perpendicular to *axis*.

    Seeded from the shared :func:`quiddity._geometry.plane_axes` basis for *axis*'s
    dominant component — the public frame in which :attr:`RectGrid.angle` is defined — then
    Gram-Schmidt'd against
    the *actual* axis so the result is exactly perpendicular to it whatever the axis is.

    Seed-and-orthogonalise rather than a near-axis special case: a threshold wide enough to
    absorb real STEP noise is also wide enough to return a basis that is NOT perpendicular to
    the normal it was given, which silently distorts every projected pitch. A 0.999 cosine
    cutoff, for example, lets a 2.5°-off normal through. An exactly axis-aligned axis
    reproduces the canonical basis unchanged, and a noisy one lands imperceptibly beside it,
    with no cutoff for real geometry to sit near.

    Deliberately NOT made right-handed about *axis*: ``(0, 0, -1)`` and ``(0, 0, 1)`` return the
    same pair, because the IR records a pattern's axis as a LETTER. A frame that flipped with a
    sign declaration cannot express would mirror the angle back through the waist.

    **Scope.** The seed follows *axis*'s dominant component, so the frame does jump a quarter
    turn across a dominant-component tie (``(1, 1-ε, 0)`` → ``(1-ε, 1, 0)``). That is not a gap
    between incompatible rules: :func:`quiddity._geometry.plane_axes` and the
    dominant axis letter use the same tie-break, so the frame and letter turn together.
    Recognition remains geometrically honest for an oblique pattern and reports its true,
    unforeshortened pitches. A consumer whose model supports only principal-axis patterns must
    reject that record rather than flatten it into the wrong plane; that policy remains outside
    the geometry package (ADR 0002).
    """
    n = math.hypot(*axis)
    a = tuple(c / n for c in axis)
    u0, v0 = plane_axes(a)
    # `u0`/`v0` span the plane of the DOMINANT component, so neither is parallel to `a` and
    # neither projection can collapse: `a`'s component along the third axis is the largest of
    # the three, hence at least 1/√3.
    u = _project_out(u0, a)
    assert u is not None  # noqa: S101 - the collapse argument above, made executable
    v = _project_out(v0, a, u)
    assert v is not None  # noqa: S101
    return u, v


def _as_linear_array(
    members, pts: Sequence[tuple[float, float]], make: Callable[..., _R]
) -> _R | None:
    """A linear-array record when *pts* (2D) are collinear at constant pitch.

    Record-generic: *make* ``(ordered_members, pitch, direction) -> Record`` builds the
    concrete record (a :class:`LinearArray` for holes, a ``PocketArray`` for pockets) so the
    collinearity/pitch geometry is shared. *members* need a ``.location`` (world centre)."""
    n = len(pts)
    # endpoints are the farthest-apart pair: robust for any orientation. A
    # lexicographic (x, y) sort would pick the wrong ends for a near-axis row
    # whose coordinates carry the sub-micron noise real STEP geometry always
    # has — an interior point sorts first, halving the span and pitch and
    # rejecting a perfectly good array.
    i0, i1 = max(
        ((i, j) for i in range(n) for j in range(i + 1, n)),
        key=lambda ij: math.dist(pts[ij[0]], pts[ij[1]]),
    )
    first, last = pts[i0], pts[i1]
    dx, dy = last[0] - first[0], last[1] - first[1]
    span = math.hypot(dx, dy)
    if span < _PATTERN_ABS_TOL:
        return None
    ux, uy = dx / span, dy / span
    # collinearity: every point within tolerance of the first→last line —
    # scaled to the pitch, not the span (a long row must not absorb holes
    # millimetres off-line)
    line_tol = _pattern_tol(span / (n - 1))
    if any(abs((p[0] - first[0]) * -uy + (p[1] - first[1]) * ux) > line_tol for p in pts):
        return None
    # project each point onto the first→last axis once (used both for the pitch
    # check, sorted, and to order the members below)
    proj = [(p[0] - first[0]) * ux + (p[1] - first[1]) * uy for p in pts]
    ts = sorted(proj)
    pitches = [ts[i + 1] - ts[i] for i in range(n - 1)]
    pitch = span / (n - 1)
    if max(abs(p - pitch) for p in pitches) > _pattern_tol(pitch):
        return None
    # order members along the array, in world coordinates
    ordered = sorted(zip(proj, members, strict=True), key=lambda t: t[0])
    w0 = ordered[0][1].location
    w1 = ordered[-1][1].location
    d = tuple(b - a for a, b in zip(w0, w1, strict=True))
    norm = math.hypot(d[0], d[1], d[2])
    return make(
        tuple(h for _, h in ordered),
        round(pitch, 2),
        _unit(tuple(c / norm for c in d)),
    )


def _linear_array_candidates(
    members, pts: Sequence[tuple[float, float]], make: Callable[..., _R]
) -> list[tuple[_R, frozenset[int]]]:
    """All linear arrays within a spec group: every pair seeds a line, the
    group's collinear points are gathered and sorted, and each maximal
    constant-pitch run of ≥3 becomes a candidate. Returns
    ``(record, frozenset(member indices))`` candidates; *make* builds the record."""
    n = len(pts)
    out, seen = [], set()
    for i in range(n):
        for j in range(i + 1, n):
            dx, dy = pts[j][0] - pts[i][0], pts[j][1] - pts[i][1]
            span = math.hypot(dx, dy)
            if span < _PATTERN_ABS_TOL:
                continue
            ux, uy = dx / span, dy / span
            tol = _pattern_tol(span)
            online = [
                m
                for m in range(n)
                if abs(-(pts[m][1] - pts[i][1]) * ux + (pts[m][0] - pts[i][0]) * uy) <= tol
            ]
            order = sorted(
                online,
                key=lambda m: (pts[m][0] - pts[i][0]) * ux + (pts[m][1] - pts[i][1]) * uy,
            )
            ts = [(pts[m][0] - pts[i][0]) * ux + (pts[m][1] - pts[i][1]) * uy for m in order]
            # split the sorted collinear points into maximal constant-pitch runs
            a = 0
            while a < len(order) - 2:
                pitch = ts[a + 1] - ts[a]
                b = a + 1
                while b + 1 < len(order) and abs((ts[b + 1] - ts[b]) - pitch) <= _pattern_tol(
                    pitch
                ):
                    b += 1
                run = order[a : b + 1]
                run_key = frozenset(run)
                if len(run) >= 3 and run_key not in seen:
                    seen.add(run_key)
                    pat = _as_linear_array([members[m] for m in run], [pts[m] for m in run], make)
                    if pat is not None:
                        out.append((pat, run_key))
                a = b  # a broken pitch starts the next run at the break point
    return out


def _rect_grid(members, pts: Sequence[tuple[float, float]], make: Callable[..., _R]) -> _R | None:
    """A rectangular-grid record when the whole spec group fills a regular N×M
    lattice, else ``None``. The two shortest near-orthogonal pairwise vectors
    define the lattice basis; every point must land on an integer cell and every
    cell must be occupied (no holes, no extras). 2×2 is excluded — four lattice
    corners are a rectangle, not a grid. *make* ``(members, rows, cols, row_pitch,
    col_pitch, angle, center) -> Record`` builds the concrete record."""
    n = len(pts)
    if n < 6:
        return None
    diffs = []
    for i in range(n):
        for j in range(n):
            if i == j:
                continue
            dx, dy = pts[j][0] - pts[i][0], pts[j][1] - pts[i][1]
            length = math.hypot(dx, dy)
            if length > _PATTERN_ABS_TOL:
                diffs.append((length, dx, dy))
    if not diffs:
        return None
    diffs.sort()
    l1, b1x, b1y = diffs[0]
    u1 = (b1x / l1, b1y / l1)
    basis2 = next(
        (
            (length, dx, dy)
            for length, dx, dy in diffs
            if abs((dx * u1[0] + dy * u1[1]) / length) < 0.2
        ),
        None,
    )
    if basis2 is None:
        return None
    l2, b2x, b2y = basis2
    u2 = (b2x / l2, b2y / l2)
    a0 = min(p[0] * u1[0] + p[1] * u1[1] for p in pts)
    b0 = min(p[0] * u2[0] + p[1] * u2[1] for p in pts)
    cells = []
    for p in pts:
        da = p[0] * u1[0] + p[1] * u1[1] - a0
        db = p[0] * u2[0] + p[1] * u2[1] - b0
        ci, cj = round(da / l1), round(db / l2)
        if abs(da - ci * l1) > _pattern_tol(l1) or abs(db - cj * l2) > _pattern_tol(l2):
            return None
        cells.append((ci, cj))
    if len(set(cells)) != n:
        return None
    # `u1` is the FIRST lattice basis and `u2` the orthogonal one; the cell index along each
    # is `ci`/`cj`. Which of those is a "row" is a naming choice, and this recogniser made the
    # serialized convention defines COLUMNS along the first local direction and ROWS along
    # the second. Swapping those names transposes counts and pitches when a consumer rebuilds
    # member positions.
    #
    # The serialized convention is explicit: columns vary along the first
    # basis, rows along the second, and the pitch tuple is `(row_pitch, column_pitch)`. Note
    # `u1` is the SHORTEST pairwise vector, not "X" — so this is a consistent local-lattice
    # convention, not a world-axis one, and holds for a rotated grid.
    #
    # Hence the angle is reduced modulo 180°, not 90°. `angle` names the COLUMN direction, and
    # a grid is invariant under a half-turn (the cell set is symmetric about its centre) but
    # NOT under a quarter-turn — a quarter-turn swaps the roles of rows and columns. Folding to
    # [0, 90) discarded exactly that distinction, so whenever the shortest basis was the
    # original ROW direction the rebuilt lattice came back transposed in place: same angle,
    # swapped counts and pitches, and therefore a different point set.
    cols = max(c[0] for c in cells) + 1
    rows = max(c[1] for c in cells) + 1
    if rows < 2 or cols < 2 or max(rows, cols) < 3 or rows * cols != n:
        return None
    center = tuple(sum(c) / n for c in zip(*(h.location for h in members), strict=True))
    return make(
        tuple(members),
        rows,
        cols,
        round(l2, 2),
        round(l1, 2),
        round(math.degrees(math.atan2(u1[1], u1[0])) % 180.0, 2),
        center,
    )
