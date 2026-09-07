# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Whole-solid geometric queries, asked once per recognition run instead of once per family.

Four kernel queries about a *whole* solid -- its optimal bounding box, its validity, its volume
and its area -- are read by nearly every family, and each read is expensive in proportion to the
solid, not to the question. On the 664-face NIST part ``nist_ctc_02`` they were **48% of a
recognition run**: 2379 ``BRepBndLib::AddOptimal`` calls, of which the 57 whole-solid boxes cost
10.2 s on their own. They were not 2379 *different* questions. ``turned.profile_key_from_bands``
asked for the same solid's box once per shaft, ``_body_identity.unambiguous_body_keys`` recomputed
box + validity + volume + area for every solid on each of the nine calls a run makes of it, and
about twenty recognisers each asked the part for its box once, independently, having no way to
learn that the family before them had just done the same.

This is the one place a run answers them. :class:`FaceGraph` owns one instance for the length of
the run -- it already owns the part, its faces and its solids, and it is already reachable at
every discovery entry point through the ledger or writer the recogniser is handed -- so a family
that has one asks the run, and a family called standalone gets a fresh call-scoped instance from
:func:`solid_properties` and computes exactly what it computed before.

**The values do not move.** Every accessor calls the same kernel query with the same default
arguments it replaced -- ``bounding_box()`` keeps ``optimal=True`` and the default tolerance --
so this is the same answer asked once, never a cheaper answer asked often.

**Keyed on the live wrapper, not on an address.** ``dict`` lookup on a build123d ``Shape`` is
``hash(TopoDS_Shape)`` narrowed by ``__eq__``, which is ``IsSame`` -- the same identity
:class:`quiddity._adjacency.FaceEdges` relies on, and the same one that makes two wrappers from
two ``part.solids()`` calls hit one entry. Holding the wrapper as the key is also what makes the
memo *safe*: a key that was only ``hash(shape.wrapped)`` returned a wrong census in the
measurement that produced this module, because a freed temporary probe solid reused a released
address. The key pins the shape, so the address cannot be reused while the entry lives -- and
scoping the cache to one run is what stops that pin from becoming a leak.

**Extension point.** :meth:`SolidProperties.derived` memoises one caller-owned value per solid
under a caller-chosen name, so a later per-solid cache -- a solid classifier, a volume-probe
scope, a wire-edge index -- hangs off the run's existing instance rather than inventing another
lifetime to get wrong. The name is the owning module's business; this module holds no policy
about what is derived, only about how long the answer lives and what it is keyed on.

Scope it to one run over one part, as :class:`quiddity._adjacency.FaceEdges` is scoped.
"""

from __future__ import annotations

from collections.abc import Callable
from typing import Protocol, TypeVar, cast

from quiddity._typing import Bounds, Part

_T = TypeVar("_T")


class SolidProperties:
    """The whole-solid queries of one run, computed on first ask and reused thereafter."""

    __slots__ = ("_area", "_bounds", "_derived", "_valid", "_volume")

    def __init__(self) -> None:
        self._bounds: dict[Part, Bounds] = {}
        self._valid: dict[Part, bool] = {}
        self._volume: dict[Part, float] = {}
        self._area: dict[Part, float] = {}
        self._derived: dict[tuple[str, Part], object] = {}

    def bounding_box(self, solid: Part) -> Bounds:
        """The solid's optimal bounding box -- ``solid.bounding_box()``, asked once."""

        bounds = self._bounds.get(solid)
        if bounds is None:
            self._bounds[solid] = bounds = solid.bounding_box()
        return bounds

    def is_valid(self, solid: Part) -> bool:
        """Whether the kernel calls the solid valid -- ``solid.is_valid``, asked once."""

        valid = self._valid.get(solid)
        if valid is None:
            self._valid[solid] = valid = bool(solid.is_valid)
        return valid

    def volume(self, solid: Part) -> float:
        """The solid's volume -- ``solid.volume``, asked once."""

        volume = self._volume.get(solid)
        if volume is None:
            self._volume[solid] = volume = float(solid.volume)
        return volume

    def area(self, solid: Part) -> float:
        """The solid's area -- ``solid.area``, asked once."""

        area = self._area.get(solid)
        if area is None:
            self._area[solid] = area = float(solid.area)
        return area

    def derived(self, name: str, solid: Part, compute: Callable[[Part], _T]) -> _T:
        """One caller-owned per-solid value, computed on first ask under *name*.

        The caller owns both the name and the meaning; this only owns the lifetime and the
        shape identity. Two callers sharing a name are sharing a value on purpose.
        """

        key = (name, solid)
        if key not in self._derived:
            self._derived[key] = compute(solid)
        return cast(_T, self._derived[key])


class SolidPropertyOwner(Protocol):
    """A run object that owns the shared cache -- in production, :class:`FaceGraph`."""

    @property
    def solid_properties(self) -> SolidProperties: ...


def solid_properties(
    owner: SolidProperties | SolidPropertyOwner | None,
) -> SolidProperties:
    """The run's shared cache, or a fresh call-scoped one when there is no run.

    *owner* is whatever the caller happens to hold: the run's graph, a cache already resolved by
    the function above it, or ``None``. A public recogniser called on its own has no graph to
    share with and must still answer exactly what it answered before -- so it gets its own
    instance. The values are identical, and the sharing that pays across families still pays
    within one call: ``recognise_grooves`` asked one solid for its bounding box once per shaft.
    """

    if owner is None:
        return SolidProperties()
    if isinstance(owner, SolidProperties):
        return owner
    return owner.solid_properties
