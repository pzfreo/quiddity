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

**The key is the live wrapper plus its orientation**, and both halves are load-bearing.

*The wrapper*, because ``dict`` lookup on a build123d ``Shape`` is ``hash(TopoDS_Shape)`` narrowed
by ``__eq__``, which is ``IsSame`` -- the same identity :class:`quiddity._adjacency.FaceEdges`
relies on, and the same one that makes two wrappers from two ``part.solids()`` calls hit one entry.
Holding the wrapper is also what makes the memo *safe*: a key that was only ``hash(shape.wrapped)``
returned a wrong census in the measurement that produced this module, because a freed temporary
probe solid reused a released address. The key pins the shape, so the address cannot be reused
while the entry lives -- and scoping the cache to one run is what stops that pin from becoming a
leak.

*The orientation*, because ``IsSame`` deliberately ignores it: a solid and
``Solid(solid.wrapped.Reversed())`` hash equal and compare equal, and one has volume ``+V`` while
the other has ``-V``. Bounding box, area and validity are orientation-blind, so ``volume`` is the
only signed value here, and no caller reverses a solid today -- but "the values do not move" is an
absolute claim and it should not rest on an unstated invariant. ``TopAbs_Orientation`` is a cheap
attribute read that costs nothing measurable beside the queries it guards, and a normal
``part.solids()`` walk yields the same orientation every time, so the hit rate is unaffected.

**Extension point.** :meth:`SolidProperties.derived` memoises one caller-owned value per solid
under a caller-chosen name, so a per-solid cache hangs off the run's existing instance rather
than inventing another lifetime to get wrong. The name is the owning module's business; this
module holds no policy about what is derived, only about how long the answer lives and what it
is keyed on. It has no caller in this change; two later PRs in this series are written against
it -- the solids a volumetric probe is measured against
(:mod:`quiddity._volume_probe`) and the point classifier the bevel corner probes use
(:mod:`quiddity._bevel`) -- and it goes if neither lands. A per-*node* cache is not one of its
users and should not become one: the wire-edge index that opening-wire incidence needs lives on
:class:`quiddity._adjacency.FaceGraph`, beside the other per-face caches.

Scope it to one run over one part, as :class:`quiddity._adjacency.FaceEdges` is scoped.
"""

from __future__ import annotations

from collections.abc import Callable
from typing import Protocol, TypeVar, cast

from quiddity._typing import Bounds, Part

_T = TypeVar("_T")

#: One cache entry's identity: the live shape wrapper (``IsSame``, which pins it) plus the
#: orientation ``IsSame`` throws away. See the module docstring for why both halves are needed.
_Key = tuple[Part, object]


def _key(solid: Part) -> _Key:
    """The wrapper, plus the orientation ``IsSame`` throws away.

    A shape with no live ``TopoDS_Shape`` behind it -- an empty compound, or one of the
    duck-typed scopes the provenance tests hand the recess cores -- has no orientation to read,
    and no ``IsSame`` identity either, so it keys on the wrapper alone. That is not a weaker
    guarantee: ``is_same`` refuses a null shape, so such a key never shares an entry anyway.
    """

    try:
        orientation: object = solid.wrapped.Orientation()
    except (AssertionError, AttributeError):
        orientation = None
    return (solid, orientation)


class SolidProperties:
    """The whole-solid queries of one run, computed on first ask and reused thereafter."""

    __slots__ = ("_area", "_bounds", "_derived", "_valid", "_volume")

    def __init__(self) -> None:
        self._bounds: dict[_Key, Bounds] = {}
        self._valid: dict[_Key, bool] = {}
        self._volume: dict[_Key, float] = {}
        self._area: dict[_Key, float] = {}
        self._derived: dict[tuple[str, Part, object], object] = {}

    def bounding_box(self, solid: Part) -> Bounds:
        """The solid's optimal bounding box -- ``solid.bounding_box()``, asked once."""

        key = _key(solid)
        bounds = self._bounds.get(key)
        if bounds is None:
            self._bounds[key] = bounds = solid.bounding_box()
        return bounds

    def is_valid(self, solid: Part) -> bool:
        """Whether the kernel calls the solid valid -- ``solid.is_valid``, asked once."""

        key = _key(solid)
        valid = self._valid.get(key)
        if valid is None:
            self._valid[key] = valid = bool(solid.is_valid)
        return valid

    def volume(self, solid: Part) -> float:
        """The solid's volume -- ``solid.volume``, asked once.

        The one signed value here, which is why orientation is part of the key.
        """

        key = _key(solid)
        volume = self._volume.get(key)
        if volume is None:
            self._volume[key] = volume = float(solid.volume)
        return volume

    def area(self, solid: Part) -> float:
        """The solid's area -- ``solid.area``, asked once."""

        key = _key(solid)
        area = self._area.get(key)
        if area is None:
            self._area[key] = area = float(solid.area)
        return area

    def derived(self, name: str, solid: Part, compute: Callable[[Part], _T]) -> _T:
        """One caller-owned per-solid value, computed on first ask under *name*.

        The caller owns both the name and the meaning; this only owns the lifetime and the
        shape identity. Two callers sharing a name are sharing a value on purpose.
        """

        key = (name, *_key(solid))
        if key not in self._derived:
            self._derived[key] = compute(solid)
        return cast(_T, self._derived[key])


class SolidPropertyOwner(Protocol):
    """A run object that owns the shared cache -- in production, :class:`FaceGraph`."""

    @property
    def solid_properties(self) -> SolidProperties: ...


class RunCapability(Protocol):
    """A run-bound capability that carries the graph -- a ``ClaimLedger`` or ``EvidenceWriter``."""

    @property
    def graph(self) -> SolidPropertyOwner: ...


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


def run_solid_properties(capability: RunCapability | None) -> SolidProperties:
    """The same, for the recognisers that hold a ledger or writer rather than a graph.

    Most public entry points receive their run as ``ledger=``/``writer=``, so the graph is one
    attribute away and ``None`` means standalone. Spelling that out at each of them produced the
    same ternary eight times, which is eight chances to get the ninth one backwards.
    """

    return SolidProperties() if capability is None else capability.graph.solid_properties
