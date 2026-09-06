# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""One bevel face, two families that read it, and a rule that decides which owns it.

An angled blind step's slant is an oblique planar face bridging two perpendicular axis-aligned
walls at a convex corner. That is a chamfer's entire signature, so `recognise_chamfers` proposes
it — correctly, on the evidence a chamfer recogniser has. The thing that says otherwise is the
triangular flat closing the blind end, and that is the *step* family's evidence.

Until now both families consulted one shared predicate, `_adjacency.has_triangular_companion`:
the chamfer family declined a bevel that had a triangular companion, the step family required
one, and the split held because two call sites agreed about a helper. The comment on it recorded
why that was fragile — two copies that drifted would not double-count, they would make the
feature *vanish*, claimed by neither, which is far harder to notice than a duplicate.

These tests pin the replacement. The discriminator is private to `angled_steps`, the family it
defines. Both families write a claim naming the one face they were established by. And
`_reconcile.chamfers_that_are_not_angled_steps` reads those claims and drops a chamfer whose face
a step already has.

Every part here is the same 45° wedge in the same block, once stopping inside and once running
through — the pairing `tests/test_angled_steps.py` is built on, reused so that what these tests
add is the *ownership*, not a second opinion about the geometry.
"""

from __future__ import annotations

from attribution_audit import attributed_run
from build123d import Box, Pos, Rot

import quiddity as r
from quiddity._adjacency import FaceEdges, FaceGraph
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._reconcile import (
    chamfers_that_are_not_angled_steps as _reconcile_chamfers,
)

#: A 45° wedge cutting 4 mm into each of the two faces meeting at the edge.
_WEDGE = 5.657


def _block():
    return Box(60, 40, 12)


def _blind():
    """The wedge stopped inside the part: a triangular flat closes its +X end."""

    return _block() - Pos(-20, 20, 6) * Rot(45, 0, 0) * Box(30, _WEDGE, _WEDGE)


def _through():
    """The same wedge run edge to edge, so neither end is closed."""

    return _block() - Pos(0, 20, 6) * Rot(45, 0, 0) * Box(70, _WEDGE, _WEDGE)


def _both():
    """One blind wedge and one through wedge on one block, so the rule has to separate them."""

    return _blind() - Pos(0, -20, 6) * Rot(45, 0, 0) * Box(70, _WEDGE, _WEDGE)


def _both_reversed():
    """The same two wedges, cut in the other order — and the kernel yields them in that order.

    This is the fixture that makes the rule's positional pairing testable. ``_both()`` happens
    to yield its two bevels in the order the records sort into, so pairing the wrong claim with
    the wrong record is invisible on it; here the kernel yields the *through* bevel first while
    ``(axis, at)`` sorts the *blind* one first, so a claim written before the sort pairs each
    record with the other's face and the rule keeps the slant and drops the chamfer.
    """

    return (_block() - Pos(0, -20, 6) * Rot(45, 0, 0) * Box(70, _WEDGE, _WEDGE)) - Pos(
        -20, 20, 6
    ) * Rot(45, 0, 0) * Box(30, _WEDGE, _WEDGE)


def _claimed(part):
    """Both families against one ledger, proved to return what they return without it."""

    ledger, chamfers = attributed_run(part, FamilyId.CHAMFERS, r.recognise_chamfers)
    steps = r.recognise_angled_steps(part, ledger=ledger)

    plain_steps = r.recognise_angled_steps(part)
    assert steps == plain_steps, "claiming changed what was recognised"
    assert [step.to_dict() for step in steps] == [step.to_dict() for step in plain_steps]
    candidates = ledger.candidate_set(FamilyId.ANGLED_STEPS).candidates
    assert len(candidates) == len(steps)
    assert all(candidate.record is step for candidate, step in zip(candidates, steps, strict=True))
    return ledger, chamfers, steps


def _face_of(ledger, record):
    (node,) = next(claim for claim in ledger.claims if claim.claimant is record).defining
    return node


def test_a_chamfer_claims_the_bevel_and_not_the_two_walls_it_bridges():
    """The bevel is what established the record; the walls only locate the virtual corner.

    Every number on a `Chamfer` is read off the bevel face — both legs from its in-plane bbox
    extents, `at` from its centroid. The two axis-aligned planes it bridges are consulted by the
    convexity probe and belong to whatever feature owns them, which is the line
    `_claims` draws between a pocket's floor and a fillet's bridged faces.
    """

    ledger, chamfers, _ = _claimed(_through())
    (chamfer,) = chamfers

    assert len(ledger.claims) == 1, "one claim, so nothing but the bevel was claimed"
    node = _face_of(ledger, chamfer)
    (x_lo, x_hi), _y, _z = ledger.graph.bounds(node)
    assert ledger.graph.is_planar(node)
    # The through wedge spans the block in X, so its bevel is the full 60 mm strip -- not one of
    # the 40 x 12 walls it bridges, whose X extent is the same but whose normal is axis-aligned.
    assert round(x_hi - x_lo, 3) == 60.0
    assert ledger.graph.normal(node) is not None
    assert max(abs(component) for component in ledger.graph.normal(node)) < 0.99


def test_an_angled_step_claims_the_slant_and_not_the_flat_that_closes_it():
    """The triangular flat is consulted, exactly as a groove's two walls are.

    It is what makes the slant blind, and it is not what the record measures: both legs,
    `length` and `at` all come off the slant. Claiming it would have every step contest its own
    end cap with whatever else owns that face.
    """

    ledger, _, steps = _claimed(_blind())
    (step,) = steps

    node = _face_of(ledger, step)
    claimed_faces = {node for claim in ledger.claims for node in claim.defining}
    assert claimed_faces == {node}, "the flat and the walls stay unclaimed"

    (x_lo, x_hi), _y, _z = ledger.graph.bounds(node)
    assert round(x_hi - x_lo, 3) == step.length, "the claimed face spans the step's run"

    # And the flat really is there, adjacent and triangular -- otherwise the assertion above
    # would be about a face that nothing had to decline to claim.
    flats = [
        other
        for other in ledger.graph.neighbours(node)
        if ledger.graph.is_planar(other) and len(ledger.graph.edges(other)) == 3
    ]
    assert len(flats) == 1


def test_the_chamfer_family_and_the_step_family_claim_the_same_face():
    """The overlap the rule exists to resolve, stated directly rather than through a count.

    Both claims name one face, and it is the *same* face. That is what makes this precedence
    rather than the containment `passages_that_are_not_slots` uses: there a slot's walls sit
    inside a passage's whole ring and the direction of the subset carries the verdict, here the
    two claims are indistinguishable and the verdict is which record says more.
    """

    ledger, chamfers, steps = _claimed(_blind())
    (chamfer,), (step,) = chamfers, steps

    assert _face_of(ledger, chamfer) is _face_of(ledger, step)
    assert chamfer.leg1 == step.leg1 and chamfer.leg2 == step.leg2 and chamfer.at == step.at
    assert ledger.claims_of(_face_of(ledger, step)) == ledger.claims


def test_the_rule_drops_the_chamfer_a_step_already_has_and_nothing_else():
    """On the pair, and on the part carrying one of each so it has to separate them."""

    ledger, chamfers, steps = _claimed(_blind())
    assert len(chamfers) == 1 and len(steps) == 1
    assert chamfers_that_are_not_angled_steps(chamfers, ledger) == []

    through_ledger, through_chamfers, through_steps = _claimed(_through())
    assert len(through_chamfers) == 1 and through_steps == []
    assert chamfers_that_are_not_angled_steps(through_chamfers, through_ledger) == through_chamfers

    both_ledger, both_chamfers, both_steps = _claimed(_both())
    assert len(both_chamfers) == 2, "both wedges are bevels on the chamfer family's evidence"
    (kept,) = chamfers_that_are_not_angled_steps(both_chamfers, both_ledger)
    assert kept.at != both_steps[0].at


def test_the_rule_pairs_each_chamfer_with_its_own_claim_and_not_the_next_ones():
    """The pairing is positional, so it is only correct while claim order is *return* order.

    On a part where the kernel yields the two bevels in one order and ``(axis, at)`` sorts them
    into the other, a claim written before the sort pairs each record with the other's face —
    and the rule then keeps the slant and drops the real chamfer, silently, with the right
    number of records either way. That is the one failure a count cannot see, and this fixture
    is the reason ``recognise_chamfers`` claims *after* it sorts.
    """

    part = _both_reversed()
    ledger, chamfers, steps = _claimed(part)
    (step,) = steps

    assert [claim.claimant for claim in ledger.claims][: len(chamfers)] == chamfers, (
        "claims must be written in the order the records are returned"
    )
    (kept,) = chamfers_that_are_not_angled_steps(chamfers, ledger)
    assert kept.at != step.at, "the rule kept the slant and dropped the chamfer"
    assert kept.at == (0.0, -18.0, 4.0), "the surviving record is the through wedge"


def test_the_rule_does_not_care_what_order_or_how_many_records_it_is_given():
    """Each record carries its own evidence, so the list is a list and not a parallel array.

    This test used to assert the opposite -- that a short list *raises* -- because the rule
    paired records against claims by position and `strict=True` caught a count that drifted.
    That refusal was never protecting a caller from anything real; it was the coupling
    announcing itself. It also could not see a *permutation*, which keeps the count and attaches
    every record to another record's faces, and which a mutation in this very family once
    produced.

    With evidence looked up by identity there is nothing left to refuse: reversing the list, or
    passing part of it, gives the same answers for the records passed.
    """

    ledger, chamfers, _ = _claimed(_both())
    kept = chamfers_that_are_not_angled_steps(chamfers, ledger)
    assert len(kept) == 1

    assert chamfers_that_are_not_angled_steps(list(reversed(chamfers)), ledger) == list(
        reversed(kept)
    )
    for one in chamfers:
        assert chamfers_that_are_not_angled_steps([one], ledger) == ([one] if one in kept else [])


def test_a_ledger_built_from_another_block_is_refused_rather_than_left_empty():
    """The provenance check, on both families, with a twin that is equal by value.

    A graph built from a different part resolves nothing, so the ledger comes back empty — and
    empty reads downstream as "these families claim nothing" rather than as "you paired the
    wrong graph". Here that reading is the exact failure the rule exists to prevent: no step
    claims, so no chamfer is dropped, and the slant is reported twice.
    """

    part, twin = _blind(), _blind()
    assert r.recognise_chamfers(twin) == r.recognise_chamfers(part), "the twin is this block"

    for recognise in (r.recognise_chamfers, r.recognise_angled_steps):
        foreign = ClaimLedger(FaceGraph(twin))
        try:
            recognise(part, ledger=foreign)
        except ValueError as refusal:
            assert "built from a different part" in str(refusal)
        else:
            raise AssertionError(f"{recognise.__name__} accepted another part's graph")


def test_a_shared_face_edge_memo_does_not_change_what_is_claimed():
    """`face_edges=` is an optimisation and `ledger=` is a sidecar; neither is a behaviour
    switch, and `feature_census` passes both to both families at once."""

    part = _both()
    memo = FaceEdges()
    ledger = ClaimLedger(FaceGraph(part, face_edges=memo))
    chamfers = r.recognise_chamfers(part, face_edges=memo, ledger=ledger)
    steps = r.recognise_angled_steps(part, face_edges=memo, ledger=ledger)

    assert chamfers == r.recognise_chamfers(part)
    assert steps == r.recognise_angled_steps(part)
    assert len(chamfers_that_are_not_angled_steps(chamfers, ledger)) == 1


def test_the_census_counts_the_slant_once_and_under_the_family_that_owns_it():
    """The metric, on the pair and on the part carrying one of each.

    `feature_census` claims to count distinct machined features. Two families describing one
    face is the case that would inflate it, and the blind wedge is where it would happen: with
    the rule unwired, the same slant appears under `chamfer` and under `angled_step`.
    """

    blind, through, both = (
        r.feature_census(_blind()),
        r.feature_census(_through()),
        (r.feature_census(_both())),
    )

    assert (blind["chamfer"], blind["angled_step"]) == (0, 1)
    assert (through["chamfer"], through["angled_step"]) == (1, 0)
    assert (both["chamfer"], both["angled_step"]) == (1, 1)


def test_the_three_rules_share_one_ledger_without_reading_each_others_claims():
    """A part carrying a slot, a chamfer and a blind step, through the one aggregate.

    `build_recognition_result` writes slots, passages and both bevel families into a single
    ledger, so each rule has to select the claims it decides between rather than everything
    written before it. The through wedge survives, the blind wedge's slant does not reach
    `chamfers`, and the slot still suppresses the passage that is that slot.
    """

    part = _both() - Box(30, 8, 20)
    result = r.build_recognition_result(part)

    assert len(result.slots) == 1
    from tools._legacy_recognition import recognise_passages

    assert recognise_passages(part), "the slot must be a passage too, or that rule is vacuous"
    assert not any(
        item.classification.feature_kind == "passage" for item in result.section_recesses
    ), "the slot still wins against the passage"
    assert len(result.angled_steps) == 1
    assert len(result.chamfers) == 1
    assert result.chamfers[0].at != result.angled_steps[0].at


def chamfers_that_are_not_angled_steps(chamfers, ledger):
    steps = [claim.claimant for claim in ledger.claims if isinstance(claim.claimant, r.AngledStep)]
    return _reconcile_chamfers(chamfers, steps, ledger.snapshot_index())
