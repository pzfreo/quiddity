# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""The dimensionable diameters of one part, across every cylindrical family.

Not a recogniser and not owned by either family it reads: a caller checking that every
dimensionable diameter is called out needs holes and bosses together, so this sits above both.
"""

from collections.abc import Sequence

from quiddity._cylinder_substrate import (
    analyse_cylinders,
)
from quiddity._typing import CylinderInventory, Part
from quiddity.bosses import BossRecord, recognise_bosses
from quiddity.holes import HoleRecord, recognise_holes


def feature_diameters(
    part: Part,
    cyls: CylinderInventory | None = None,
    holes: Sequence[HoleRecord] | None = None,
    bosses: Sequence[BossRecord] | None = None,
) -> list[float]:
    """Sorted unique diameters of the *recognised* dimensionable cylindrical
    features on *part*: every hole bore, each hole's counterbore/spotface step,
    and every boss.

    This is the inventory to use for coverage checks ("is each dimensionable
    diameter called out?"). It is deliberately built from
    :func:`recognise_holes` / :func:`recognise_bosses`, not the raw :func:`full_cylinders`
    patch list, so partial cylinders that never become a real feature — slot ends and
    interrupted recesses (an exact half-cylinder pair sums to a full turn and
    fools an angle-only test, but is not a bore) — are excluded, while genuine
    counterbore/spotface steps are kept.

    Pass *cyls* — a precomputed ``analyse_cylinders(part)`` result — to share one
    scan between ``recognise_holes`` and ``recognise_bosses``. Pass *holes* — a precomputed
    ``recognise_holes`` result — to reuse the single feature inventory instead of
    re-detecting (the single-inventory rule).
    """
    cyls = analyse_cylinders(part) if cyls is None else cyls
    if holes is None:
        holes = recognise_holes(part, cyls=cyls)
    diams: list[float] = []
    for h in holes:
        diams.append(h.diameter)
        if h.cbore is not None:
            diams.append(h.cbore.diameter)
        if h.spotface is not None:
            diams.append(h.spotface.diameter)
    for b in recognise_bosses(part, cyls=cyls) if bosses is None else bosses:
        diams.append(b.diameter)
    return sorted(set(diams))
