# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Pure derived pattern recognition for slot and pocket records."""

from __future__ import annotations

from collections.abc import Sequence

from quiddity._pattern_geometry import (
    _linear_array_candidates,
    _plane_uv,
    _rect_grid,
)
from quiddity._recess_records import (
    Pocket,
    PocketArray,
    PocketGrid,
    Slot,
    SlotArray,
    SlotGrid,
)


def _pocket_spec_key(pk: Pocket) -> tuple:
    """The grouping key shared by pockets of the *same milled recess* — same orientation, size,
    AND opening plane. Only identical, same-orientation, COPLANAR pockets can form one array (a
    90°-rotated pocket swaps width_axis/long_axis and reads as a different feature). The
    depth-axis extent (d_lo, d_hi) is part of the key: pattern detection projects the depth
    coordinate away, so without it pockets on different-height stepped faces — or opening
    opposite directions — whose in-plane centres happen to line up would merge into one planar
    array that does not exist. Coordinates snap to 3 dp so boolean-op float noise
    does not split an array (mirrors ``HoleSpec``'s axis snap)."""
    return (
        pk.width_axis,
        pk.long_axis,
        round(pk.width, 3),
        round(pk.length, 3),
        round(pk.depth, 3),
        round(pk.d_lo, 3),
        round(pk.d_hi, 3),
        pk.open_sign,  # opposite-facing pockets sharing a depth range are on different faces
        pk.edge_anchored,  # implicit-location corner recesses are a distinct feature class
        None if pk.end_radius is None else round(pk.end_radius, 3),
        None if pk.corner_radius is None else round(pk.corner_radius, 3),
        pk.body_key,
    )


def _mk_pocket_linear(
    members: Sequence[Pocket], pitch: float, direction: tuple[float, float, float]
) -> PocketArray:
    return PocketArray(pockets=tuple(members), pitch=pitch, direction=direction)


def _mk_pocket_grid(
    members: Sequence[Pocket],
    rows: int,
    cols: int,
    row_pitch: float,
    col_pitch: float,
    angle: float,
    center: tuple[float, float, float],
) -> PocketGrid:
    return PocketGrid(
        pockets=tuple(members),
        rows=rows,
        cols=cols,
        row_pitch=row_pitch,
        col_pitch=col_pitch,
        angle=angle,
        center=center,
    )


def recognise_pocket_patterns(pockets: Sequence[Pocket]) -> list[PocketArray | PocketGrid]:
    """Recognise :class:`PocketArray` (linear) and :class:`PocketGrid` (rectangular) arrays
    among *pockets* (``Pocket`` records, e.g. from :func:`recognise_pockets`) — the recess
    analog of :func:`quiddity.recognise_hole_patterns`.

    A DERIVED recogniser (single positional inventory, package ADR 0002): pockets are grouped by
    orientation + size (:func:`_pocket_spec_key`), each group's centres are projected into the
    opening plane (perpendicular to the shared depth axis), and the same collinear / lattice
    geometry the hole patterns use (shared via *make* factories) is enumerated and
    allocated greedily largest-first so each pocket belongs to at most one array. Pockets have
    no bolt-circle form, so only grid + linear candidates are considered. Un-arrayed pockets
    are simply absent from the result."""
    axis_unit = {"x": (1.0, 0.0, 0.0), "y": (0.0, 1.0, 0.0), "z": (0.0, 0.0, 1.0)}
    groups: dict = {}
    for pk in pockets:
        if pk.body_key is None:
            continue  # ambiguous physical ownership cannot authorise a pattern
        groups.setdefault(_pocket_spec_key(pk), []).append(pk)

    patterns: list[PocketArray | PocketGrid] = []
    for members in groups.values():
        if len(members) < 3:
            continue
        u, v = _plane_uv(axis_unit[members[0].depth_axis])
        pts = [
            (
                sum(a * b for a, b in zip(pk.location, u, strict=True)),
                sum(a * b for a, b in zip(pk.location, v, strict=True)),
            )
            for pk in members
        ]
        candidates: list = []
        grid = _rect_grid(members, pts, _mk_pocket_grid)
        if grid is not None:
            candidates.append((grid, frozenset(range(len(members)))))
        candidates += _linear_array_candidates(members, pts, _mk_pocket_linear)
        candidates.sort(key=lambda c: -len(c[1]))
        used: set = set()
        for pattern, idx in candidates:
            if idx & used:
                continue
            patterns.append(pattern)
            used |= idx
    return patterns


def _slot_spec_key(sl: Slot) -> tuple:
    """The grouping key shared by slots of the *same milled feature* — same orientation, size,
    AND through plane. Only identical, same-orientation, coplanar slots form one array. The
    through-axis extent (d_lo, d_hi) is part of the key so slots on different-height stepped
    faces whose in-plane centres line up don't merge into a planar array that does not exist
    (mirrors ``_pocket_spec_key``). A slot is THROUGH — no floor, no opening direction — so
    unlike the pocket key there is no ``depth`` and no ``open_sign``. Coordinates snap to 3 dp so
    boolean-op float noise does not split an array."""
    return (
        sl.width_axis,
        sl.long_axis,
        round(sl.width, 3),
        round(sl.length, 3),
        round(sl.d_lo, 3),
        round(sl.d_hi, 3),
        None if sl.end_radius is None else round(sl.end_radius, 3),
        None if sl.corner_radius is None else round(sl.corner_radius, 3),
        sl.body_key,
    )


def _mk_slot_linear(
    members: Sequence[Slot], pitch: float, direction: tuple[float, float, float]
) -> SlotArray:
    return SlotArray(slots=tuple(members), pitch=pitch, direction=direction)


def _mk_slot_grid(
    members: Sequence[Slot],
    rows: int,
    cols: int,
    row_pitch: float,
    col_pitch: float,
    angle: float,
    center: tuple[float, float, float],
) -> SlotGrid:
    return SlotGrid(
        slots=tuple(members),
        rows=rows,
        cols=cols,
        row_pitch=row_pitch,
        col_pitch=col_pitch,
        angle=angle,
        center=center,
    )


def recognise_slot_patterns(slots: Sequence[Slot]) -> list[SlotArray | SlotGrid]:
    """Recognise :class:`SlotArray` (linear) and :class:`SlotGrid` (rectangular) arrays among
    *slots* (``Slot`` records, e.g. from :func:`recognise_slots`) — the through-slot analog of
    :func:`recognise_pocket_patterns`.

    A DERIVED recogniser (single positional inventory, package ADR 0002): slots are grouped by
    orientation + size + through plane (:func:`_slot_spec_key`), each group's centres are
    projected into the face plane (perpendicular to the shared through axis), and the same
    collinear / lattice geometry the hole/pocket patterns use (shared via *make* factories,
    shared record-generic factories) is enumerated and allocated greedily largest-first. Slots
    have no bolt-circle form, so
    only grid + linear candidates are considered. Un-arrayed slots are absent from the result."""
    axis_unit = {"x": (1.0, 0.0, 0.0), "y": (0.0, 1.0, 0.0), "z": (0.0, 0.0, 1.0)}
    groups: dict = {}
    for sl in slots:
        if sl.body_key is None:
            continue  # ambiguous physical ownership cannot authorise a pattern
        groups.setdefault(_slot_spec_key(sl), []).append(sl)

    patterns: list[SlotArray | SlotGrid] = []
    for members in groups.values():
        if len(members) < 3:
            continue
        u, v = _plane_uv(axis_unit[members[0].depth_axis])
        pts = [
            (
                sum(a * b for a, b in zip(sl.location, u, strict=True)),
                sum(a * b for a, b in zip(sl.location, v, strict=True)),
            )
            for sl in members
        ]
        candidates: list = []
        grid = _rect_grid(members, pts, _mk_slot_grid)
        if grid is not None:
            candidates.append((grid, frozenset(range(len(members)))))
        candidates += _linear_array_candidates(members, pts, _mk_slot_linear)
        candidates.sort(key=lambda c: -len(c[1]))
        used: set = set()
        for pattern, idx in candidates:
            if idx & used:
                continue
            patterns.append(pattern)
            used |= idx
    return patterns
