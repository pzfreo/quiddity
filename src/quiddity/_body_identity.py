# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Serializable, geometry-derived source-body correlation.

The value is deliberately useful only for equality.  It is not a persistent topology handle:
separate solids with the same signature are ambiguous and callers must publish ``None`` rather
than assigning either occurrence by traversal order.
"""

from __future__ import annotations

from collections import Counter
from collections.abc import Sequence

from quiddity._solid_properties import SolidProperties, solid_properties
from quiddity._typing import Part

BodyKey = tuple[float, ...]


def body_signature(solid: Part, *, properties: SolidProperties | None = None) -> BodyKey:
    """Return the shared public geometric correlation signature.

    Bounds use six decimals; area and volume use twelve significant figures. All families
    use this policy, independent of whether their source-validity check is required here.
    The signature is not occurrence identity: duplicate signatures must still be refused.

    *properties* is the run's whole-solid cache. Nine calls of :func:`unambiguous_body_keys` a
    run recomputed the same box, volume and area of the same solids; passing the run's cache is
    what stops that. The values are unchanged either way -- an absent cache means a fresh one.
    """

    memo = solid_properties(properties)
    bb = memo.bounding_box(solid)
    values = (
        float(bb.min.X),
        float(bb.min.Y),
        float(bb.min.Z),
        float(bb.max.X),
        float(bb.max.Y),
        float(bb.max.Z),
        memo.volume(solid),
        memo.area(solid),
    )
    coordinates = tuple(round(value, 6) or 0.0 for value in values[:6])
    mass_properties = tuple(float(f"{value:.12g}") for value in values[6:])
    return (*coordinates, *mass_properties)


def unambiguous_body_keys(
    sources: Sequence[Part],
    *,
    require_valid_solid: bool = False,
    properties: SolidProperties | None = None,
) -> tuple[BodyKey | None, ...]:
    """Return occurrence-aligned keys, refusing duplicate geometric signatures.

    Existing recess projections also operate on record-only open-shell compatibility inputs.
    New physical-ownership fields opt into ``require_valid_solid`` so those inputs publish no
    misleading body identity without changing the older recess value contract.
    """

    memo = solid_properties(properties)
    signatures = tuple(
        (
            body_signature(source, properties=memo)
            if not require_valid_solid or (source.solids() and memo.is_valid(source))
            else None
        )
        for source in sources
    )
    counts = Counter(signature for signature in signatures if signature is not None)
    return tuple(
        signature if signature is not None and counts[signature] == 1 else None
        for signature in signatures
    )
