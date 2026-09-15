# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Feature-recognition census.

A **measurement tool**, not a recogniser: it counts how completely the recognition suite
(:mod:`quiddity`) captures a part's machined features. It produces no consumer-domain
feature or drawing policy.

The metric is a **census**: the number of features each recogniser finds, per kind. Across a
representative corpus, changes in kinds and counts expose recognition changes directly; a single
part's census states exactly what was recognised.

*Why census-only, no completeness ratio.* A ratio needs an independent denominator, and there
isn't a good one. The obvious substrate — :func:`quiddity.feature_diameters` —
is itself built from ``recognise_holes`` / ``recognise_bosses``, so diffing recognised diameters
against it is **tautological**: both sides move together, the ratio is 1.0 for every real part,
and a genuine recogniser regression drops the denominator too (so it never signals). The only
independent substrate, the raw ``analyse_cylinders`` patch list, is **noisy** — radiused slot
ends and other non-feature partial cylinders are never features, so legitimate parts would score
below 1.0 permanently. That is why :func:`feature_diameters` avoids the raw substrate. A
census is the one honest signal, so that is all this reports.

Above the aggregate rather than beside it: this counts what
:func:`quiddity.build_recognition_result` returns, over the same run, so a count and a
result cannot describe one part differently. It was a parallel orchestration once, and the two
drifted -- see ADR 0003 for what that cost.
"""

from __future__ import annotations

from collections.abc import Sequence
from typing import cast

from quiddity._record import Record
from quiddity._registry import validate_census_contract
from quiddity._typing import Part
from quiddity.result import _take_inventory

# Publicly stable order remains an explicit census contract; registry metadata is checked against
# it but does not generate, publish, or reorder keys.
_LEGACY_CENSUS_BINDINGS: tuple[tuple[str, str], ...] = (
    ("hole", "holes"),
    ("hole_pattern", "hole_patterns"),
    ("boss", "bosses"),
    ("step", "turned_steps"),
    ("groove", "grooves"),
    ("flat", "flats"),
    ("slot", "slots"),
    ("oriented_slot", "oriented_slots"),
    ("rectangular_blind_slot", "rectangular_blind_slots"),
    ("round_bottom_blind_slot", "round_bottom_blind_slots"),
    ("channel", "channels"),
    ("pocket", "pockets"),
    ("prismatic_pocket", "prismatic_pockets"),
    ("edge_open_circular_pocket", "edge_open_circular_pockets"),
    ("edge_open_prismatic_recess", "edge_open_prismatic_recesses"),
    ("passage", "section_passages"),
    ("chamfer", "chamfers"),
    ("angled_step", "angled_steps"),
    ("gusset_rib", "gusset_ribs"),
    ("paired_ramp_step", "paired_ramp_steps"),
    ("through_step", "through_steps"),
    ("circular_blind_step", "circular_blind_steps"),
    ("blend", "blends"),
    ("fillet", "fillets"),
    ("countersink", "countersinks"),
    ("plate", "plates"),
)
CENSUS_BINDINGS = tuple(
    (key, source)
    for key, source in _LEGACY_CENSUS_BINDINGS
    if source
    not in [
        "channels",
        "rectangular_blind_slots",
        "round_bottom_blind_slots",
        "pockets",
        "prismatic_pockets",
        "edge_open_circular_pockets",
        "edge_open_prismatic_recesses",
        "pocket_patterns",
        "section_passages",
        "passages",
    ]
) + (("section_recess", "section_recesses"),)
CENSUS_KEYS: tuple[str, ...] = tuple(key for key, _source in CENSUS_BINDINGS)
validate_census_contract({source: key for key, source in CENSUS_BINDINGS})


def feature_census(part: Part) -> dict[str, int]:
    """The count of recognised features per kind for *part* (see module docs).

    Counts the one shared inventory -- the same sequence of recogniser calls, over the same
    run-local state, that `build_recognition_result` returns. It is not a second orchestration
    that happens to agree; agreeing is not something a second one can be made to keep doing.

    Returns ``{kind: count}`` with a stable, complete set of keys: a kind absent from the part
    reports ``0``, not a missing key. Face levels and step risers are computed by the inventory
    and not counted here: they feed other recognisers rather than being distinct machined
    features, and their level derivation belongs to the model layer, not to a metric.

    Taken as prismatic, because that is the classification under which nothing is gated away
    and a census is a completeness check. `build_recognition_result` takes *rotational* from
    its caller; this has no caller to take it from, and under-counting a real feature is the
    worse error for a metric.
    """

    product = _take_inventory(part)
    found = product.result
    records: dict[str, Sequence[Record]] = {}
    for key, source in CENSUS_BINDINGS:
        # TurnedStep is the registered source, but the census deliberately projects its accepted
        # step/groove compatibility relation so one machined band is not counted twice.
        records[key] = (
            cast(
                Sequence[Record],
                tuple(candidate.record for candidate in product.distinct_steps.candidates),
            )
            if key == "step"
            else cast(Sequence[Record], getattr(found, source))
        )
    # The one place the census deliberately differs from the result it counts. A groove is
    # both an annular channel and a rung of the step ladder; both records are true and both
    # survive into the inventory, but it is one machined feature and this counts features.
    # See `steps_that_are_not_grooves` for why the ladder keeps its rung.
    return {kind: len(recs) for kind, recs in records.items()}
