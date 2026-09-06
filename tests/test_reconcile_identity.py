# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""A rule finds a record's evidence by identity, so the order records arrive in cannot matter.

Every rule in `_reconcile` used to pair its records against the ledger's claims **by position**,
with `zip(..., strict=True)`. That works only while a recogniser writes one claim per record in
exactly the order it returns them — a coupling between two files that nothing checked and no
reader could see.

`strict=True` catches a *count* that drifts. It cannot catch a **permutation**, which keeps the
count and hands every record another record's faces. That is not a hypothesis: a mutation in the
chamfer/angled-step work that claimed before sorting rather than after did exactly this, kept the
slant, dropped the real chamfer, and survived the entire golden corpus. Only a part built so the
kernel's traversal order and the record sort order disagree caught it, and every rule added since
carried the same coupling on trust.

These tests permute deliberately. If a rule ever goes back to reading position, they fail — which
is what the old design could not arrange for itself.
"""

from __future__ import annotations

import random

from build123d import Box, BuildPart, BuildSketch, Plane, Polygon, Pos, Rot, extrude

from quiddity._adjacency import FaceGraph
from quiddity._claims import ClaimLedger
from quiddity._reconcile import (
    chamfers_that_are_not_angled_steps,
    prismatic_pockets_that_are_not_pockets,
)
from tools._legacy_recognition import namespace

_WEDGE = 5.657


r = namespace()


def _busy_part():
    """One solid carrying every family the bevel and pocket rules decide between."""

    with BuildPart() as prism:
        with BuildSketch(Plane.XY):
            Polygon((-12, -9), (12, -9), (0, 12))
        extrude(amount=14)
    return (
        (Box(140, 90, 20) - Pos(-40, 0, 2) * prism.part)
        - Pos(20, 0, 8) * Box(20, 12, 14)
        - Pos(0, 45, 10) * Rot(45, 0, 0) * Box(150, _WEDGE, _WEDGE)
        - Pos(0, -45, 10) * Rot(45, 0, 0) * Box(150, _WEDGE, _WEDGE)
        - Pos(55, 20, 10) * Rot(45, 0, 0) * Box(30, _WEDGE, _WEDGE)
    )


def _claimed(part):
    ledger = ClaimLedger(FaceGraph(part))
    return ledger, {
        "pockets": r.recognise_pockets(part, ledger=ledger),
        "prismatic": r.recognise_prismatic_pockets(part, ledger=ledger),
        "chamfers": r.recognise_chamfers(part, ledger=ledger),
        "steps": r.recognise_angled_steps(part, ledger=ledger),
    }


def test_the_fixture_actually_exercises_both_rules():
    """Otherwise the permutation tests below are shuffling empty lists.

    The trap this file exists to catch needs at least two records per family: with one, every
    permutation is the identity and a positional join looks correct.
    """

    _ledger, found = _claimed(_busy_part())
    assert len(found["chamfers"]) >= 2, "the bevel rule needs something to get wrong"
    assert len(found["prismatic"]) >= 2, "and so does the pocket rule"


def test_permuting_recogniser_output_does_not_change_what_survives():
    """The acceptance criterion: reconciliation is correct under deliberately permuted output.

    Shuffled twenty ways with a fixed seed rather than reversed once, because a reversal is a
    single permutation and the failure being guarded against is *any* of them. Compared as sets:
    a rule returns records in the order it was given them, so the order of the answer follows the
    order of the question and only membership is the rule's business.
    """

    part = _busy_part()
    ledger, found = _claimed(part)
    evidence = ledger.snapshot_index()
    baseline_bevels = set(
        chamfers_that_are_not_angled_steps(found["chamfers"], found["steps"], evidence)
    )
    baseline_pockets = set(
        prismatic_pockets_that_are_not_pockets(found["prismatic"], found["pockets"], evidence)
    )

    shuffler = random.Random(20260820)
    for _ in range(20):
        bevels = list(found["chamfers"])
        pockets = list(found["prismatic"])
        shuffler.shuffle(bevels)
        shuffler.shuffle(pockets)

        assert (
            set(chamfers_that_are_not_angled_steps(bevels, found["steps"], evidence))
            == baseline_bevels
        )
        assert (
            set(prismatic_pockets_that_are_not_pockets(pockets, found["pockets"], evidence))
            == baseline_pockets
        )


def test_two_records_of_equal_value_keep_separate_evidence():
    """Identity, not value — the property that made the ledger's claims identity-compared.

    A dictionary keyed on the record would have merged these into one entry covering both their
    faces, and a rule asking which to keep would find a single claim naming everything. That is
    why `Claim` compares by identity, and `defining_of` has to preserve it.
    """

    part = Box(120, 60, 20) - Pos(-30, 0, 8) * Box(16, 12, 14) - Pos(30, 0, 8) * Box(16, 12, 14)
    ledger, found = _claimed(part)
    pockets = found["prismatic"]

    assert len(pockets) == 2, "two identical recesses, differing only in where they are"
    first, second = pockets
    assert ledger.defining_of(first) != ledger.defining_of(second)
    assert not ledger.defining_of(first) & ledger.defining_of(second)


def test_a_record_that_claimed_nothing_reports_no_evidence_rather_than_failing():
    """Empty is an answer a rule can act on; raising would make the caller handle a normal case.

    A stubby obround recess is recovered from its cylindrical end caps, which `_planar_faces`
    never yields, so it claims nothing and is absent from the ledger entirely.
    """

    ledger = ClaimLedger(FaceGraph(Box(20, 20, 20)))
    stranger = object()
    assert ledger.defining_of(stranger) == frozenset()
