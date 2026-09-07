# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Policy-neutral volumetric evidence for axis-aligned candidate regions.

**A probe is asked about material, so it is asked of solids.** An imported STEP part is often a
:class:`~build123d.Compound` holding the machined solid *beside* loose construction geometry --
``nist_ctc_01`` is one solid plus a compound of 78 datum edges, ``nist_ctc_03`` has 87 of them,
``nist_ctc_02`` adds an open shell. build123d's ``Compound.intersect`` distributes the
intersection over every child, so a probe of ``ctc_01`` built **79** boolean operations to answer
a question only one of them could contribute to: measured, one recognition of that part ran 2154
``BRepAlgoAPI`` builds, 1772 of them inside volume probes and all but 56 of those on loose edges.

Those could not change an answer, because they could not produce volume:

- :func:`intersection_volume` sums ``.volume`` over the fragments ``Shape.intersect`` returns,
  and that list has been through ``ShapeList.expand()``, which dissolves compounds, wires and
  *shells* -- so a fragment is only ever a ``Vertex``, ``Edge``, ``Face`` or ``Solid``. The
  ``volume`` of the first three is literally ``return 0.0`` in build123d; a manifold ``Shell``
  is the one non-solid shape with a volume, and expansion means one never reaches the sum.
- A fragment can only be a ``Solid`` if it came from a ``Solid`` child. Every child intersection
  is one ``BRepAlgoAPI_Common``, whose result has the lesser dimension of its arguments, so a
  child of dimension two or less yields geometry of dimension two or less.

So the sum over *all* children equals the sum over the solid children, and
:func:`probe_solids` narrows every probe in this module to those. The sum is over all of them:
a multi-solid part keeps contributing every body it has, exactly as the distribution did.
"""

from __future__ import annotations

from collections.abc import Iterable
from typing import Protocol, cast

from build123d import Box, Compound, Pos, Solid

from quiddity._solid_properties import SolidProperties, SolidPropertyOwner, solid_properties
from quiddity._typing import Part

#: OCCT cannot construct a volumetric probe at or below this coordinate extent.
PRISM_PROBE_FLOOR = 1e-6

#: Name under which a run's :class:`SolidProperties` holds one part's probe solids.
#:
#: ``derived`` documents that two callers sharing a name are sharing a value on purpose, so the
#: name is namespaced by the module that owns the meaning rather than left in a flat space.
_PROBE_SOLIDS = "_volume_probe.solids"


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


def _solids_of(part: Part) -> tuple[Part, ...]:
    return tuple(part.solids())


def probe_solids(
    part: Part, *, properties: SolidProperties | SolidPropertyOwner | None = None
) -> tuple[Part, ...]:
    """The bodies of *part* a volumetric probe is measured against.

    A part that is already one solid -- what most call sites hold, having come through
    ``graph.solid_shape(owner)`` -- is its own answer, and so is anything that is not a
    build123d ``Compound`` at all: the standalone and test callers that pass a bare object
    with an ``intersect`` method keep measuring exactly what they measured before.

    Only a compound fans out, and only a compound is asked to name its solids. *properties* is
    the run's cache (see :mod:`quiddity._solid_properties`), reached from the ``graph`` the
    caller already holds; without one the traversal is simply repeated, as every other value in
    that module is when a recogniser is called standalone.
    """

    if not isinstance(part, Compound):
        return (part,)
    return solid_properties(properties).derived(_PROBE_SOLIDS, part, _solids_of)


def probe_volume(
    part: Part,
    probe: Solid,
    *,
    properties: SolidProperties | SolidPropertyOwner | None = None,
) -> float:
    """The volume *probe* shares with the material of *part* -- its solids, and all of them."""

    return float(
        sum(
            intersection_volume(body.intersect(probe))
            for body in probe_solids(part, properties=properties)
        )
    )


def material_fraction(
    part: Part,
    probe: Solid,
    *,
    properties: SolidProperties | SolidPropertyOwner | None = None,
) -> float:
    """Measure occupied fraction of the supplied probe; callers own admission policy."""
    return probe_volume(part, probe, properties=properties) / float(probe.volume)


def prism_material_fraction(
    spans: dict[str, tuple[float, float]],
    part: Part,
    *,
    inset: float,
    properties: SolidProperties | SolidPropertyOwner | None = None,
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
    occupied = probe_volume(part, probe, properties=properties)
    return float(occupied / (size["x"] * size["y"] * size["z"]))


def prism_is_empty(
    spans: dict[str, tuple[float, float]],
    part: Part,
    *,
    inset: float,
    properties: SolidProperties | SolidPropertyOwner | None = None,
) -> bool:
    """Whether the inset prism has exactly zero volumetric intersection with ``part``."""

    return prism_material_fraction(spans, part, inset=inset, properties=properties) == 0.0
