# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Geometry-only records shared by recess recognition and patterns."""

from __future__ import annotations

from dataclasses import dataclass

from quiddity._record import Record
from quiddity._typing import Vector3


@dataclass(frozen=True)
class Slot(Record):
    """A milled slot / reduced across-flats section.

    A slot is bounded by two opposed parallel walls facing each other.  The
    wall separation is the *width* (the defining size); the extent the walls
    share along the part's longer in-plane axis is the *length*.

    width_axis: axis across the slot (the walls' normal axis), 'x'/'y'/'z'.
    long_axis:  axis the slot runs along (largest shared wall extent).
    width:      wall-to-wall separation — the defining size dim.
    length:     slot extent along ``long_axis``.
    w_center:   ``width_axis`` coordinate of the slot centreline.
    lo, hi:     ``long_axis`` coordinates of the slot ends (lo < hi).
    d_lo, d_hi: extent on the third (depth) axis.
    end_radius: radius of both proved semicircular obround ends, or ``None`` when unproved.
    corner_radius: radius of all four proved rounded corners, or ``None`` when unproved.
    """

    width_axis: str
    long_axis: str
    width: float
    length: float
    w_center: float
    lo: float
    hi: float
    d_lo: float
    d_hi: float
    # Geometry-derived identity of the physical solid that supplied this record.  A compound
    # must not turn equal features on separate bodies into one manufacturing pattern.
    # The empty tuple preserves hand-built record compatibility. ``None`` is reserved for
    # unavailable/ambiguous recognised provenance and is ineligible for pattern grouping.
    body_key: tuple[float, ...] | None = ()
    # Appended after the historical positional fields. ``None`` means unknown/unproved,
    # never that the corresponding ends or corners are square.
    end_radius: float | None = None
    corner_radius: float | None = None

    @property
    def depth_axis(self) -> str:
        return next(a for a in "xyz" if a not in (self.width_axis, self.long_axis))

    @property
    def location(self) -> tuple[float, float, float]:
        """The slot centroid in world XYZ — mid-span along ``long_axis``, the centreline on
        ``width_axis``, mid-through on ``depth_axis``. The ``location`` is the shared pattern
        geometry key, matching pocket and hole records."""
        coord = {
            self.long_axis: (self.lo + self.hi) / 2,
            self.width_axis: self.w_center,
            self.depth_axis: (self.d_lo + self.d_hi) / 2,
        }
        return (coord["x"], coord["y"], coord["z"])


@dataclass(frozen=True)
class SlotArray(Record):
    """N identical milled slots in a straight, constant-pitch line — the through-slot
    analog of :class:`PocketArray`. ``slots`` are the member :class:`Slot` records (ordered along
    the array); ``pitch`` is the centre-to-centre spacing and ``direction`` the (unit) array
    axis."""

    slots: tuple[Slot, ...]
    pitch: float
    direction: Vector3


@dataclass(frozen=True)
class SlotGrid(Record):
    """N×M identical milled slots on a rectangular lattice — the through-slot analog of
    :class:`PocketGrid`. ``slots`` are the member :class:`Slot` records; the lattice is
    ``rows``×``cols`` at ``row_pitch``/``col_pitch``, rotated ``angle`` degrees about
    ``center`` — the same convention :class:`PocketGrid` and `RectGrid` document."""

    slots: tuple[Slot, ...]
    rows: int
    cols: int
    row_pitch: float
    col_pitch: float
    angle: float
    center: Vector3


@dataclass(frozen=True)
class Pocket(Record):
    """A blind rectangular recess — a milled slot/pocket capped by a floor.

    The through/blind split (:func:`_has_floor`) is the only difference from a
    :class:`Slot`: a pocket is floored, so it carries a third defining size — the
    ``depth`` from the open face down to the floor (``d_hi - d_lo``, the walls' extent
    on the depth axis). ``width``/``length`` are the in-plane footprint, exactly as for
    a slot; elongated and near-square *bounded* recesses are the same feature here. A recess
    whose longitudinal end reaches a source-solid envelope is instead the explicit edge-open
    ``RectangularBlindSlot`` family.

    width_axis: axis across the recess (the walls' normal axis), 'x'/'y'/'z'.
    long_axis:  axis the recess runs along (largest shared wall extent).
    width:      wall-to-wall separation — the shorter in-plane size.
    length:     recess extent along ``long_axis`` — the longer in-plane size.
    depth:      open-face-to-floor depth (along ``depth_axis``).
    w_center:   ``width_axis`` coordinate of the recess centreline.
    lo, hi:     ``long_axis`` coordinates of the recess ends (lo < hi).
    d_lo, d_hi: extent on the depth axis (``depth == d_hi - d_lo``).
    end_radius: radius of both proved semicircular obround ends, or ``None`` when unproved.
    corner_radius: radius of all four proved rounded corners, or ``None`` when unproved.
    """

    width_axis: str
    long_axis: str
    width: float
    length: float
    depth: float
    w_center: float
    lo: float
    hi: float
    d_lo: float
    d_hi: float
    # Which depth end is the OPENING: +1 opens toward +depth (floor at d_lo), -1 toward -depth
    # (floor at d_hi). d_lo/d_hi alone are the unordered extent, so without this two pockets on
    # opposite faces sharing an absolute depth range are indistinguishable — and would merge into
    # one impossible array. Defaults to +1 for hand-built records.
    open_sign: int = 1
    # A corner interruption touches two adjacent envelope edges, so its in-plane
    # location is implicit and no centre-location dimensions are required.
    edge_anchored: bool = False
    # See Slot.body_key.  Appended after the existing defaults to preserve positional callers.
    body_key: tuple[float, ...] | None = ()
    # Appended after every historical positional field. ``None`` is an epistemic value:
    # the shape was not proved, rather than proved square.
    end_radius: float | None = None
    corner_radius: float | None = None

    @property
    def depth_axis(self) -> str:
        return next(a for a in "xyz" if a not in (self.width_axis, self.long_axis))

    @property
    def location(self) -> tuple[float, float, float]:
        """The recess centroid in world XYZ — mid-span along ``long_axis``, the centreline on
        ``width_axis``, mid-depth on ``depth_axis``. The ``location`` is the shared pattern
        geometry key, matching the corresponding hole and slot records."""
        coord = {
            self.long_axis: (self.lo + self.hi) / 2,
            self.width_axis: self.w_center,
            self.depth_axis: (self.d_lo + self.d_hi) / 2,
        }
        return (coord["x"], coord["y"], coord["z"])


@dataclass(frozen=True)
class Channel(Record):
    """A full-span floored rectangular channel, open at both longitudinal ends.

    Unlike :class:`Pocket`, a channel's longitudinal extent and depth are already
    conveyed by the part envelope and its plate/level scheme.  The record retains
    that geometry for identity and correspondence, but its sole defining measurement
    is the wall-to-wall ``width``.
    """

    width_axis: str
    long_axis: str
    width: float
    w_center: float
    lo: float
    hi: float
    d_lo: float
    d_hi: float
    open_sign: int = 1
    # Source-body correlation only. Empty is retained for hand-built legacy records; ``None``
    # means recognised ownership collided and must not be used for a cross-family join.
    body_key: tuple[float, ...] | None = ()

    @property
    def depth_axis(self) -> str:
        return next(a for a in "xyz" if a not in (self.width_axis, self.long_axis))

    @property
    def length(self) -> float:
        return self.hi - self.lo

    @property
    def depth(self) -> float:
        return self.d_hi - self.d_lo

    @property
    def location(self) -> tuple[float, float, float]:
        coord = {
            self.long_axis: (self.lo + self.hi) / 2,
            self.width_axis: self.w_center,
            self.depth_axis: (self.d_lo + self.d_hi) / 2,
        }
        return (coord["x"], coord["y"], coord["z"])


@dataclass(frozen=True)
class PocketArray(Record):
    """N identical blind pockets in a straight, constant-pitch line — the recess analog
    of :class:`quiddity.LinearArray`. ``pockets`` are the member
    :class:`Pocket` records (ordered along the array); ``pitch`` is the centre-to-centre spacing
    and ``direction`` the unit array axis."""

    pockets: tuple[Pocket, ...]
    pitch: float
    direction: Vector3


@dataclass(frozen=True)
class PocketGrid(Record):
    """N×M identical blind pockets on a rectangular lattice — the recess analog of
    :class:`quiddity.RectGrid`. ``pockets`` are the member
    :class:`Pocket` records; the lattice is ``rows``×``cols`` at ``row_pitch``/``col_pitch``,
    rotated ``angle`` degrees about ``center`` — the same convention `RectGrid` documents
    (columns along the first basis, ``angle`` naming that direction in ``[0, 180)``)."""

    pockets: tuple[Pocket, ...]
    rows: int
    cols: int
    row_pitch: float
    col_pitch: float
    angle: float
    center: Vector3


# Keep the supported historical module path stable for repr and pickle consumers.
for _record_type in (Slot, SlotArray, SlotGrid, Pocket, Channel, PocketArray, PocketGrid):
    _record_type.__module__ = "quiddity.slots"
