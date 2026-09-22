# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""The named rules that decide between families describing one physical region (ADR 0003).

Candidate discovery and reconciliation are separate stages. A recogniser proposes; the rules
here accept, combine or reject. They live outside every recogniser on purpose: one that declined
a face because another family had already claimed it would make the census depend on which
family ran first, which ADR 0003 forbids and ADR 0002 forbids again by ruling out sibling calls.

The rules are of two kinds, because ADR 0003 says a reconciler "accepts, combines or rejects":

- **precedence** -- the recess families are reconciled from their complete boundary claims, with
  a rectangular paired-wall record winning only where it describes the same four-wall void and
  a non-rectangular ring winning over paired-wall fragments assembled inside it; a chamfer that
  is an angled step's slant is dropped because the step says strictly more;
- **compatibility** -- a turned step whose band is a groove keeps its record, because both
  descriptions are needed, and only the *count* is corrected.

Not a constraint solver. ADR 0003 allows family-specific rules to migrate behind this protocol
one at a time, and these are the rules for which there is measured evidence.

**A rule finds a record's evidence by identity**, through the terminal frozen `EvidenceIndex`.
Every rule here once paired its records against the ledger's claims *by position*, which held only
while a recogniser wrote one claim per record in the order it returned them -- a coupling across
two files that nothing checked. `strict=True` catches a count that drifts and cannot catch a
permutation, and a permutation hands every record another record's faces while the counts stay
right. A mutation in the chamfer/angled-step work did exactly that and survived the whole golden
corpus.
"""

from __future__ import annotations

from collections import defaultdict
from typing import cast

from quiddity._candidates import Candidate, CandidateSet, EvidenceIndex, FamilyId
from quiddity._dispositions import (
    Disposition,
    Outcome,
    ReasonCode,
    ReconciliationResult,
)
from quiddity._passage_compat import grouping_from_view
from quiddity._recess_records import Pocket, Slot
from quiddity.angled_steps import AngledStep
from quiddity.chamfers import Chamfer
from quiddity.grooves import Groove
from quiddity.passages import Passage
from quiddity.prismatic_pockets import PrismaticPocket
from quiddity.turned import TurnedStep


def reconcile_recess_candidates(
    slots: CandidateSet[object],
    pockets: CandidateSet[object],
    prismatic: CandidateSet[object],
    passages: CandidateSet[object],
    evidence: EvidenceIndex,
    *,
    rectangular_blind_slots: CandidateSet[object] | None = None,
    edge_open_circular_pockets: CandidateSet[object] | None = None,
) -> tuple[Disposition, ...]:
    """Disposition form of the existing cascading recess precedence policy."""

    decisions: list[Disposition] = []
    pocket_candidates = list(pockets.candidates)
    rectangular_blind_candidates = list(
        (
            evidence.candidate_set_for(FamilyId.RECTANGULAR_BLIND_SLOTS, ())
            if rectangular_blind_slots is None
            else rectangular_blind_slots
        ).candidates
    )
    edge_open_circular_candidates = list(
        (
            evidence.candidate_set_for(FamilyId.EDGE_OPEN_CIRCULAR_POCKETS, ())
            if edge_open_circular_pockets is None
            else edge_open_circular_pockets
        ).candidates
    )
    passage_candidates = list(passages.candidates)

    accepted_prismatic: list[Candidate[object]] = []
    for ring in prismatic.candidates:
        winners = tuple(
            pocket
            for pocket in pocket_candidates
            if isinstance(ring.record, PrismaticPocket)
            and ring.record.sides == 4
            and (walls := evidence.defining_of(pocket))
            and walls <= evidence.defining_of(ring)
        )
        if winners:
            decisions.append(
                Disposition(
                    ring,
                    Outcome.REJECTED,
                    ReasonCode.PRISMATIC_SUPERSEDED_BY_POCKET,
                    winners,
                )
            )
        else:
            accepted_prismatic.append(ring)

    nonrect_prismatic = tuple(
        ring
        for ring in accepted_prismatic
        if isinstance(ring.record, PrismaticPocket) and ring.record.sides != 4
    )
    accepted_pockets: list[Candidate[object]] = []
    for pocket in pocket_candidates:
        pocket_evidence = evidence.defining_of(pocket)
        if not pocket_evidence:
            accepted_pockets.append(pocket)
            continue
        pocket_members = evidence.constituent_of(pocket)
        winners = tuple(
            open_pocket
            for open_pocket in edge_open_circular_candidates
            if pocket_evidence <= evidence.defining_of(open_pocket)
        )
        reason = ReasonCode.POCKET_SUPERSEDED_BY_EDGE_OPEN_CIRCULAR_POCKET
        if not winners:
            winners = tuple(
                blind_slot
                for blind_slot in rectangular_blind_candidates
                if pocket_members and pocket_members <= evidence.defining_of(blind_slot)
            )
            reason = ReasonCode.POCKET_SUPERSEDED_BY_RECTANGULAR_BLIND_SLOT
        if not winners:
            winners = tuple(
                passage
                for passage in passage_candidates
                if pocket_evidence <= evidence.defining_of(passage)
            )
            reason = ReasonCode.POCKET_SUPERSEDED_BY_PASSAGE
        if not winners:
            winners = tuple(
                ring for ring in nonrect_prismatic if pocket_evidence <= evidence.defining_of(ring)
            )
            reason = ReasonCode.POCKET_SUPERSEDED_BY_PRISMATIC
        if winners:
            decisions.append(Disposition(pocket, Outcome.REJECTED, reason, winners))
        else:
            accepted_pockets.append(pocket)

    grouped_passages: dict[tuple, list[Candidate[object]]] = defaultdict(list)
    grouped_evidence: dict[tuple, set] = defaultdict(set)
    for passage in passage_candidates:
        compatibility = evidence.passage_compatibility(passage)
        grouping = grouping_from_view(compatibility)
        if grouping is not None and grouping[2] != 4:
            key = grouping[:2]
            grouped_passages[key].append(passage)
            grouped_evidence[key].update(evidence.defining_of(passage))

    accepted_slots: list[Candidate[object]] = []
    for slot in slots.candidates:
        walls = evidence.defining_of(slot)
        if not walls:
            accepted_slots.append(slot)
            continue
        winners = tuple(
            pocket for pocket in accepted_pockets if walls <= evidence.defining_of(pocket)
        )
        reason = ReasonCode.SLOT_SUPERSEDED_BY_POCKET
        if not winners:
            winners = tuple(
                ring for ring in accepted_prismatic if walls <= evidence.defining_of(ring)
            )
            reason = ReasonCode.SLOT_SUPERSEDED_BY_PRISMATIC
        if not winners:
            matching = [key for key, nodes in grouped_evidence.items() if walls <= nodes]
            winners = tuple(passage for key in matching for passage in grouped_passages[key])
            reason = ReasonCode.SLOT_SUPERSEDED_BY_PASSAGE
        if winners:
            decisions.append(Disposition(slot, Outcome.REJECTED, reason, winners))
        else:
            accepted_slots.append(slot)

    for passage in passage_candidates:
        compatibility = evidence.passage_compatibility(passage)
        grouping = grouping_from_view(compatibility)
        if grouping is None or grouping[2] != 4:
            continue
        winners = tuple(
            slot
            for slot in accepted_slots
            if (walls := evidence.defining_of(slot)) and walls <= evidence.defining_of(passage)
        )
        if winners:
            decisions.append(
                Disposition(
                    passage,
                    Outcome.REJECTED,
                    ReasonCode.PASSAGE_SUPERSEDED_BY_SLOT,
                    winners,
                )
            )
    return tuple(decisions)


def reconcile_bevel_candidates(
    chamfers: CandidateSet[object],
    angled_steps: CandidateSet[object],
    evidence: EvidenceIndex,
) -> tuple[Disposition, ...]:
    """Reject only chamfers whose defining face is an accepted angled-step slant."""

    decisions = []
    for chamfer in chamfers.candidates:
        winners = tuple(
            step
            for step in angled_steps.candidates
            if not evidence.defining_of(chamfer).isdisjoint(evidence.defining_of(step))
        )
        if winners:
            decisions.append(
                Disposition(
                    chamfer,
                    Outcome.REJECTED,
                    ReasonCode.CHAMFER_SUPERSEDED_BY_ANGLED_STEP,
                    winners,
                )
            )
    return tuple(decisions)


def reconcile_oriented_slot_passages(
    passages: CandidateSet[object],
    oriented_slots: CandidateSet[object],
    evidence: EvidenceIndex,
) -> tuple[Disposition, ...]:
    """Prefer the specific free-axis rectangular Slot interpretation to its source passage."""

    decisions = []
    for passage in passages.candidates:
        walls = evidence.defining_of(passage)
        winners = tuple(
            slot
            for slot in oriented_slots.candidates
            if walls and walls == evidence.defining_of(slot)
        )
        if winners:
            decisions.append(
                Disposition(
                    passage,
                    Outcome.REJECTED,
                    ReasonCode.PASSAGE_SUPERSEDED_BY_ORIENTED_SLOT,
                    winners,
                )
            )
    return tuple(decisions)


def reconcile_circular_step_fillets(
    fillets: CandidateSet[object],
    circular_steps: CandidateSet[object],
    evidence: EvidenceIndex,
) -> tuple[Disposition, ...]:
    """Reject a fillet only when a surviving circular step claims its curved wall."""

    decisions = []
    for fillet in fillets.candidates:
        fillet_faces = evidence.defining_of(fillet)
        winners = tuple(
            step
            for step in circular_steps.candidates
            if fillet_faces and fillet_faces <= evidence.defining_of(step)
        )
        if winners:
            decisions.append(
                Disposition(
                    fillet,
                    Outcome.REJECTED,
                    ReasonCode.FILLET_SUPERSEDED_BY_CIRCULAR_BLIND_STEP,
                    winners,
                )
            )
    return tuple(decisions)


def reconcile_blend_candidates(
    blends: CandidateSet[object],
    fillets: CandidateSet[object],
    evidence: EvidenceIndex,
    *,
    rejected_fillets: frozenset[Candidate[object]] = frozenset(),
) -> tuple[Disposition, ...]:
    """Prefer complete feature semantics over the neutral cylindrical chain carrier."""

    decisions = []
    for blend in blends.candidates:
        blend_faces = evidence.defining_of(blend)
        fillet_winners = tuple(
            fillet
            for fillet in fillets.candidates
            if fillet not in rejected_fillets
            if not blend_faces.isdisjoint(evidence.defining_of(fillet))
        )
        covered = frozenset(
            node for fillet in fillet_winners for node in evidence.defining_of(fillet)
        )
        if blend_faces and blend_faces <= covered:
            decisions.append(
                Disposition(
                    blend,
                    Outcome.REJECTED,
                    ReasonCode.BLEND_SUPERSEDED_BY_FILLET,
                    fillet_winners,
                )
            )
    return tuple(decisions)


def reconcile_profiled_bore_candidates(
    holes: CandidateSet[object],
    double_d_bores: CandidateSet[object],
    evidence: EvidenceIndex,
) -> tuple[Disposition, ...]:
    """Reject a partial circular-hole reading of one accepted Double-D void.

    A Double-D bore retains its complete constant-extrusion wall set. A coincident ordinary
    Hole sees only the cylindrical subset of that same boundary. Exact graph containment is
    therefore the occurrence-safe precedence proof; equal coordinates, diameters and axes are
    deliberately insufficient.
    """

    decisions = []
    for hole in holes.candidates:
        hole_faces = evidence.defining_of(hole)
        winners = tuple(
            bore
            for bore in double_d_bores.candidates
            if hole_faces and hole_faces < evidence.defining_of(bore)
        )
        if winners:
            decisions.append(
                Disposition(
                    hole,
                    Outcome.REJECTED,
                    ReasonCode.HOLE_SUPERSEDED_BY_DOUBLE_D_BORE,
                    winners,
                )
            )
    return tuple(decisions)


def reconcile_boss_turned_step_candidates(
    bosses: CandidateSet[object],
    steps: CandidateSet[object],
    evidence: EvidenceIndex,
) -> tuple[Disposition, ...]:
    """Prefer a turned-profile rung over a duplicate cylindrical boss reading.

    Exact defining-face equality proves occurrence identity without comparing
    rounded dimensions or joining separate bodies. The turned step owns the
    axial shoulder semantics; a standalone boss scan may still report the raw
    cylinder, but the aggregate publishes the more complete interpretation.
    """

    decisions = []
    for boss in bosses.candidates:
        defining = evidence.defining_of(boss)
        winners = tuple(
            step for step in steps.candidates if defining and defining == evidence.defining_of(step)
        )
        if winners:
            decisions.append(
                Disposition(
                    boss,
                    Outcome.REJECTED,
                    ReasonCode.BOSS_SUPERSEDED_BY_TURNED_STEP,
                    winners,
                )
            )
    return tuple(decisions)


def reconcile_step_groove_candidates(
    steps: CandidateSet[object],
    grooves: CandidateSet[object],
    evidence: EvidenceIndex,
) -> tuple[Disposition, ...]:
    """Relate compatible records while accepting both physical descriptions."""

    step_relations: dict[int, list[Candidate[object]]] = defaultdict(list)
    groove_relations: dict[int, list[Candidate[object]]] = defaultdict(list)
    for groove in grooves.candidates:
        floor = evidence.defining_of(groove)
        if not floor:
            continue
        for step in steps.candidates:
            if floor <= evidence.defining_of(step):
                step_relations[id(step)].append(groove)
                groove_relations[id(groove)].append(step)
    decisions = [
        Disposition(
            step,
            Outcome.ACCEPTED,
            ReasonCode.TURNED_STEP_GROOVE_COMPATIBLE,
            tuple(step_relations[id(step)]),
        )
        for step in steps.candidates
        if id(step) in step_relations
    ]
    decisions.extend(
        Disposition(
            groove,
            Outcome.ACCEPTED,
            ReasonCode.GROOVE_TURNED_STEP_COMPATIBLE,
            tuple(groove_relations[id(groove)]),
        )
        for groove in grooves.candidates
        if id(groove) in groove_relations
    )
    return tuple(decisions)


def reconcile_recesses(
    slots: list[Slot],
    pockets: list[Pocket],
    prismatic: list[PrismaticPocket],
    passages: list[Passage],
    evidence: EvidenceIndex,
) -> tuple[list[Slot], list[Pocket], list[PrismaticPocket], list[Passage]]:
    """Apply the recess-family precedence rules after every family has proposed.

    A shared face is only evidence.  The verdicts here require containment by a more complete
    description of the same boundary:

    - a verified through ring defeats paired-wall pocket fragments inside it;
    - a verified floored pocket defeats a slot built from the same walls;
    - a non-rectangular passage or prismatic-pocket ring defeats a slot assembled from a
      subset of its walls;
    - a four-wall passage still yields to the Slot that dimensions that rectangular void.

    Interrupted rings may be returned as several records with the same section. Their claims
    are pooled only within that exact ``(axis, section)`` identity before testing containment,
    matching the way slot reduction pools collinear wall arms into one record.
    """

    groups = (
        evidence.candidate_set_for(FamilyId.SLOTS, slots),
        evidence.candidate_set_for(FamilyId.POCKETS, pockets),
        evidence.candidate_set_for(FamilyId.PRISMATIC_POCKETS, prismatic),
        evidence.candidate_set_for(FamilyId.PASSAGES, passages),
    )
    result = ReconciliationResult.complete(
        groups,
        reconcile_recess_candidates(*groups, evidence),
        evidence,
    )
    return cast(
        tuple[list[Slot], list[Pocket], list[PrismaticPocket], list[Passage]],
        tuple(
            [candidate.record for candidate in result.accepted_set(group).candidates]
            for group in groups
        ),
    )


def steps_that_are_not_grooves(
    steps: list[TurnedStep], grooves: list[Groove], evidence: EvidenceIndex
) -> list[TurnedStep]:
    """The turned steps that are a distinct machined feature, for counting purposes only.

    A groove *is* a rung of the step ladder: an external band whose OD is a local minimum is
    both "the band between these two shoulders" and "the annular channel cut into the shaft".
    Measured on the pinned `turned_steps_and_grooves` golden, the O24 band from 15.5 to 20.5 is
    reported by both families, and `feature_census` counted it once under each -- one machined
    feature, two features in the metric.

    **Both records survive**, unlike the passage a slot already describes. That is not
    inconsistency, it is the difference ADR 0003 draws between rejecting and combining:

    - The two are not competing descriptions of one void, one of which says more. They are a
      *feature* and a *profile*, and a consumer dimensioning the shaft needs both -- the groove
      to call out a width that excludes the lead-in chamfers, the ladder to place every
      shoulder.
    - Deleting the rung would not simplify the ladder, it would falsify it.
      `TurnedProfile.from_steps` takes contiguity as a caller precondition, and `shoulders`
      treats an interior `hi` with no following `lo` as "a real end face, not a shared
      shoulder". A ladder with the groove removed therefore describes a shaft with two end
      faces where the groove is -- a different shaft.

    So `build_recognition_result` carries both, and this belongs to `feature_census` alone --
    the one place that claims to count *distinct machined features* rather than to describe
    them. That the two deliberately disagree is the point, and is why this is a named function
    rather than a subtraction written inline at the call site.

    It waited on a defect upstream of the count. While it was unwired, `recognise_turned_steps`
    reported the groove's rung at the shaft's OD on a part modelled small, so there was no
    groove rung to reconcile and the count stopped being scale-free -- one wrong count being
    better than a scale-dependent one. That is fixed, and `test_scale_invariance` now runs the
    census over the turned-step golden at a twentieth and a hundred times size. Not over the
    vendored corpus, which cannot check it: all 50 NIST and MFCAD++ parts are milled prismatic
    and report no turned steps at all.
    """

    step_set = evidence.candidate_set_for(FamilyId.TURNED_STEPS, steps)
    groove_set = evidence.candidate_set_for(FamilyId.GROOVES, grooves)
    related = {
        id(item.candidate)
        for item in reconcile_step_groove_candidates(step_set, groove_set, evidence)
        if item.reason is ReasonCode.TURNED_STEP_GROOVE_COMPATIBLE
    }
    return cast(
        list[TurnedStep],
        [candidate.record for candidate in step_set.candidates if id(candidate) not in related],
    )


def chamfers_that_are_not_angled_steps(
    chamfers: list[Chamfer], angled_steps: list[AngledStep], evidence: EvidenceIndex
) -> list[Chamfer]:
    """The chamfers that are not the slant of an angled blind step, dropped where they are.

    A blind step's slant is an oblique planar bevel bridging two perpendicular axis-aligned
    walls at a convex corner, which is a chamfer's entire signature. `recognise_chamfers`
    therefore proposes it, correctly on its own evidence, and `recognise_angled_steps` claims
    it as well. **Both families claim exactly one face -- the slant itself -- so a contested
    face is not two features overlapping but one face described twice**, and the step record
    wins: it says everything the chamfer says and adds `length`, the distance the slant runs
    before the triangular flat closes it. That flat is what the step found and the chamfer
    could not see.

    Precedence, like `passages_that_are_not_slots`, and for the same reason -- one description
    subsumes the other. Not the containment test that rule uses, though: there a slot's two
    walls sit *inside* a passage's whole ring, so the direction of the subset carries the
    verdict. Here the two claims are the same single face, so overlap and containment are the
    same question and the honest way to write it is that they name the same face at all.

    **This is what `_adjacency.has_triangular_companion` used to do, and did not generalise.**
    Both recognisers consulted it, one requiring the flat and one refusing it, so the split was
    kept by two call sites agreeing about a helper rather than by anyone deciding. The comment
    on it recorded the risk that made it fragile: two copies that drifted would not double-count
    but would make the feature *vanish*, claimed by neither. The discriminator is now private to
    `angled_steps`, which is the family it defines, and the ownership question is answered where
    ADR 0003 puts it -- from the claims, after discovery, by a named rule that a third family
    reading bevels could join without either recogniser learning about it.

    A consequence a caller sees: `recognise_chamfers` called on its own now reports a blind
    step's slant, as it did before `recognise_angled_steps` existed. `feature_census` and
    `build_recognition_result` both apply this rule, so the reconciled answer is unchanged.
    """

    chamfer_set = evidence.candidate_set_for(FamilyId.CHAMFERS, chamfers)
    step_set = evidence.candidate_set_for(FamilyId.ANGLED_STEPS, angled_steps)
    result = ReconciliationResult.complete(
        (chamfer_set, step_set),
        reconcile_bevel_candidates(chamfer_set, step_set, evidence),
        evidence,
    )
    return cast(
        list[Chamfer],
        [candidate.record for candidate in result.accepted_set(chamfer_set).candidates],
    )


def prismatic_pockets_that_are_not_pockets(
    prismatic: list[PrismaticPocket],
    pockets: list[Pocket],
    evidence: EvidenceIndex,
) -> list[PrismaticPocket]:
    """The prismatic pockets no rectangular `Pocket` already describes, dropped where they are.

    Two families reach a rectangular recess and neither is wrong. `recognise_pockets` pairs two
    facing walls; `recognise_prismatic_pockets` walks the closed ring those walls sit in. On the
    geometry alone both are true, and which record a caller wants is not a question either
    recogniser can answer about itself.

    **For a four-sided ring, the rectangular record wins**, and the direction is not arbitrary.
    `Pocket` measures
    `width` and `length` on named axes -- the numbers a drawing calls out -- where the prismatic
    record carries a four-corner section that says the same thing less directly. For a shape the
    older family can express, it expresses it better. For every shape it cannot, nothing here
    fires and the prismatic record is the only one there is.

    Containment, like `passages_that_are_not_slots`, and for the same reason: a `Pocket` claims
    the two walls it was paired from, and those walls are members of the ring, so the subset runs
    one way and never the other. Mere overlap would be too weak -- two recesses sharing a wall
    are two recesses, which is what ADR 0003 means by overlapping claims being evidence rather
    than a verdict.
    """

    empty_slots = evidence.candidate_set_for(FamilyId.SLOTS, ())
    pocket_set = evidence.candidate_set_for(FamilyId.POCKETS, pockets)
    prismatic_set = evidence.candidate_set_for(FamilyId.PRISMATIC_POCKETS, prismatic)
    empty_passages = evidence.candidate_set_for(FamilyId.PASSAGES, ())
    groups = (empty_slots, pocket_set, prismatic_set, empty_passages)
    result = ReconciliationResult.complete(
        groups,
        reconcile_recess_candidates(*groups, evidence),
        evidence,
    )
    return cast(
        list[PrismaticPocket],
        [candidate.record for candidate in result.accepted_set(prismatic_set).candidates],
    )
