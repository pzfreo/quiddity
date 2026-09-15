# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""`feature_census` and `build_recognition_result` must not answer differently about one part.

The two used to build similar run-local state and apply related reconciliation through separate
code paths. That was a silent-divergence point by construction: nothing forced a rule added to
one to be added to the other, and the symptom was not a crash but a *number* — the same solid
reported as having a feature by one entry point and not by the other.

It was not hypothetical. Measured across all 73 corpus parts, the two disagreed about `plate` on
one of them: `build_recognition_result` suppresses a plate when the shaft's steps form a turned
profile, and the census counted one anyway. A real turned screw, one part in seventy-three, and
nothing in the suite was looking.

They now share one inventory — `_take_inventory` — so a disagreement of that kind can no longer
be written. What remains for this file to guard is the *mapping*: the census names a kind, the
inventory returns a field, and nothing but this checks that the census counts the field it means
to. A key wired to the wrong family, or a family added to one side and not the other, still
produces a wrong number silently. So the property is kept rather than retired as impossible.

**It is no longer checked over the whole vendored corpus.** The mapping is a property of the two
inventories, not of any part: a key wired to the wrong field is wrong on *every* part that
carries the family, so the evidence only has to make each family appear once. Measured, the 32
golden fixtures already do — every one of the seventeen `SHARED` families is populated by at least
one of them (`section_recess` by eight, `hole` by seven, `boss`, `blend` and `plate` by four
each, `slot`, `chamfer` and `through_step` by two, the rest by one). Reading all 87 vendored
parts added 188 seconds serially at the series' branch point, and 65 s once run-scoped caching
landed — either way the whole test suite's critical path, longer than the next four slowest tests
together, and unsplittable by `xdist` because it is one test — and put no family through the map
that the goldens had not already.

What is kept from the corpus is the one part the goldens cannot make: the real turned screw
below. Goldens are built to exercise one family at a time, and the historical disagreement was an
*interaction* — a shaft whose steps form a ladder and a slab the plate recogniser would claim,
on the same part. That is a fifth of a second, so it stays, and the test checks the part still
carries the shape rather than trusting that it does.

**Two differences are by design and are named rather than asserted away.** They are the reason
this cannot simply compare every key:

- `step` — `steps_that_are_not_grooves` is a *compatibility* rule under ADR 0003. Both records
  survive into the inventory, and only the census's count is corrected, so the census reporting
  fewer steps than the result carries is the rule working.
- pattern families — the census counts hole patterns but has no key for slot or pocket patterns.
  That is a scope decision about what a "distinct machined feature" is, not a divergence, but it
  is worth knowing it is deliberate.
"""

from __future__ import annotations

import dataclasses
from pathlib import Path

import pytest
from build123d import import_step

import quiddity as r
from quiddity._registry import DERIVED_DEFINITIONS, PHYSICAL_DEFINITIONS, Counted, NotCounted
from quiddity.census import feature_census
from tests.golden._common import load_fixture

GOLDEN = Path(__file__).parent / "golden"
CORPUS = Path(__file__).parent / "corpus"

#: Census key -> result field, for every family both inventories report: every registry family
#: that is `Counted`, less the two documented exceptions. `step` is a compatibility rule under
#: ADR 0003 (both records survive, only the count is corrected) and `flat` is a substrate.
SHARED = {
    definition.census.key: definition.result_field
    for definition in (*PHYSICAL_DEFINITIONS, *DERIVED_DEFINITIONS)
    if isinstance(definition.census, Counted) and definition.census.key not in {"step", "flat"}
}


def _disagreements(part):
    counts = feature_census(part)
    result = r.build_recognition_result(part)
    return {
        key: (counts[key], len(getattr(result, field)))
        for key, field in SHARED.items()
        if counts[key] != len(getattr(result, field))
    }


@pytest.mark.parametrize("fixture", sorted(p.parent.name for p in GOLDEN.glob("*/fixture.py")))
def test_the_two_inventories_agree_on_every_golden(fixture):
    """Every synthetic part, so a failure names its fixture and mismatched family."""

    part = load_fixture(GOLDEN / fixture / "fixture.py").build_fixture()
    assert _disagreements(part) == {}


#: The one imported part that ever disagreed: a real turned screw whose four steps form a ladder,
#: and from which `recognise_plates`, asked on its own, still claims a slab. `plate` went 1 to 0
#: for this file when the census learned the aggregate's turned-profile gate.
DIAGNOSTIC_SCREW = CORPUS / "gramel" / "GRM-03_thumbwheel_drive_screw.step"


@pytest.mark.skipif(
    not DIAGNOSTIC_SCREW.is_file(),
    reason="the vendored corpora are excluded from the sdist",
)
def test_the_two_inventories_agree_on_the_screw_that_once_disagreed():
    """Where the one real disagreement was, and the only part in 73 that had it.

    Goldens are built to exercise one family at a time; the divergence that motivated this file
    needed a real turned screw whose steps form a ladder *and* a slab a plate would be claimed
    from. Imported geometry is where an inventory that has quietly drifted shows up.
    """

    # No skipping. This is a checked-in input, and swallowing an import failure would quietly
    # remove the one diagnostic screw this file exists for from the evidence. A file that stops
    # importing is a finding, so the exception is left to fail the test.
    part = import_step(str(DIAGNOSTIC_SCREW))

    # The part still has to carry the shape that made it diagnostic, or the agreement below is
    # agreement about nothing: a step ladder, and a plate that only the ladder suppresses.
    result = r.build_recognition_result(part)
    assert len(result.turned_steps) >= 2, "the screw's step ladder"
    assert r.recognise_plates(part) and not result.plates, "the plate the ladder suppresses"

    assert _disagreements(part) == {}


#: `RecognitionResult` fields the census deliberately does not count, and why. The decision is
#: forced where the family is declared: every registry definition states `Counted` or
#: `NotCounted(reason)`, and `validate_census_contract` refuses a registry that disagrees with
#: the census. The fields below are the ones no registry definition owns.
RESULT_ONLY = {
    definition.result_field: definition.census.reason
    for definition in (*PHYSICAL_DEFINITIONS, *DERIVED_DEFINITIONS)
    if isinstance(definition.census, NotCounted)
    # The converged recess detectors are registry families without a public result field.
    and definition.result_field in r.RecognitionResult.__dataclass_fields__
} | {
    "cylinders": "the shared cylinder scan",
    "flats": "substrate for turned features",
    "rotational": "a classification, not a record list",
    "turned_steps": "counted as `step` after `steps_that_are_not_grooves`",
    "section_recess_patterns": "census counts hole patterns only",
    "section_recess_refusals": "evidence without reconstructible geometry is not an occurrence",
}


def test_the_shared_map_still_covers_every_family_the_census_counts():
    """A census key added without updating `SHARED` would be compared against nothing.

    That is the failure mode this file exists to prevent, one level up.
    """

    counted = set(
        feature_census(load_fixture(GOLDEN / "simple_through_hole" / "fixture.py").build_fixture())
    )
    # `step` and `flat` are the documented exceptions: a compatibility rule and a substrate.
    assert counted - set(SHARED) == {"step", "flat"}


def test_the_shared_map_still_covers_every_family_the_aggregate_reports():
    """And the same in the other direction, which the first test cannot see.

    A new `RecognitionResult` family omitted from the census would leave the two inventories
    covering different sets while every comparison here still passed -- exactly the drift this
    file claims to catch. The exclusions are the useful part: they make the current asymmetry
    deliberate, and force a decision each time an aggregate family is added.
    """

    fields = {
        field.name
        for field in dataclasses.fields(
            r.build_recognition_result(
                load_fixture(GOLDEN / "simple_through_hole" / "fixture.py").build_fixture()
            )
        )
    }
    assert fields - set(SHARED.values()) == set(RESULT_ONLY)
