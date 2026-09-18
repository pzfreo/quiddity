# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

from dataclasses import dataclass

import pytest
from build123d import Box

import quiddity.result as result_module
from quiddity._adjacency import FaceGraph
from quiddity._candidates import Candidate, CandidateSet, Evidence, FamilyId
from quiddity._claims import ClaimLedger
from quiddity._dispositions import (
    Disposition,
    Outcome,
    ReasonCode,
    ReconciliationResult,
)
from quiddity._reconcile import reconcile_recess_candidates
from quiddity.passages import SectionPassage, _discover_section_passages
from quiddity.prismatic_pockets import PrismaticPocket


@dataclass(frozen=True)
class Record:
    value: int


def test_defaults_and_rejections_preserve_candidate_identity_and_source_order() -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    first_record = Record(1)
    second_record = Record(1)
    first = ledger.propose(FamilyId.SLOTS, first_record)
    second = ledger.propose(FamilyId.SLOTS, second_record)
    winner_record = Record(2)
    winner = ledger.propose(FamilyId.POCKETS, winner_record)
    slots = ledger.candidate_set_for(FamilyId.SLOTS, [first_record, second_record])
    pockets = ledger.candidate_set_for(FamilyId.POCKETS, [winner_record])
    result = ReconciliationResult.complete(
        (slots, pockets),
        (
            Disposition(
                second,
                Outcome.REJECTED,
                ReasonCode.SLOT_SUPERSEDED_BY_POCKET,
                (winner,),
            ),
        ),
        ledger.snapshot_index(),
    )

    assert tuple(item.candidate for item in result.dispositions) == (first, second, winner)
    assert result.dispositions[0].reason is ReasonCode.DEFAULT_ACCEPTED
    assert result.accepted_set(slots).candidates == (first,)
    with pytest.raises(ValueError, match="candidate family"):
        ReconciliationResult.complete(
            (slots, pockets),
            (
                Disposition(
                    second,
                    Outcome.REJECTED,
                    ReasonCode.CHAMFER_SUPERSEDED_BY_ANGLED_STEP,
                    (winner,),
                ),
            ),
            ledger.snapshot_index(),
        )


def test_duplicate_foreign_and_invalid_related_dispositions_fail_closed() -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    record = Record(1)
    candidate = ledger.propose(FamilyId.SLOTS, record)
    slots = ledger.candidate_set_for(FamilyId.SLOTS, [record])
    decision = Disposition(candidate, Outcome.ACCEPTED, ReasonCode.DEFAULT_ACCEPTED)

    with pytest.raises(TypeError):
        ReconciliationResult(())  # type: ignore[call-arg]

    with pytest.raises(ValueError, match="more than one"):
        ReconciliationResult.complete(
            (slots,),
            (
                decision,
                Disposition(candidate, Outcome.ACCEPTED, ReasonCode.DEFAULT_ACCEPTED),
            ),
            ledger.snapshot_index(),
        )
    with pytest.raises(ValueError, match="not self-related"):
        ReconciliationResult.complete(
            (slots,),
            (
                Disposition(
                    candidate,
                    Outcome.REJECTED,
                    ReasonCode.SLOT_SUPERSEDED_BY_POCKET,
                    (candidate,),
                ),
            ),
            ledger.snapshot_index(),
        )
    with pytest.raises(ValueError, match="closed enums"):
        ReconciliationResult.complete(
            (slots,),
            (Disposition(candidate, Outcome.REJECTED, "free form"),),  # type: ignore[arg-type]
            ledger.snapshot_index(),
        )
    other = ClaimLedger(FaceGraph(Box(4, 4, 4)))
    foreign_record = Record(2)
    foreign = other.propose(FamilyId.POCKETS, foreign_record)
    foreign_set = other.candidate_set_for(FamilyId.POCKETS, [foreign_record])
    with pytest.raises(ValueError, match="another evidence issuer"):
        ReconciliationResult.complete((slots, foreign_set), (), ledger.snapshot_index())
    with pytest.raises(ValueError, match="not in the physical inventory"):
        ReconciliationResult.complete(
            (slots,),
            (Disposition(foreign, Outcome.ACCEPTED, ReasonCode.DEFAULT_ACCEPTED),),
            ledger.snapshot_index(),
        )
    with pytest.raises(ValueError, match="related candidate is not"):
        ReconciliationResult.complete(
            (slots,),
            (
                Disposition(
                    candidate,
                    Outcome.REJECTED,
                    ReasonCode.SLOT_SUPERSEDED_BY_POCKET,
                    (foreign,),
                ),
            ),
            ledger.snapshot_index(),
        )
    complete = ReconciliationResult.complete((slots,), (), ledger.snapshot_index())
    with pytest.raises(ValueError, match="another evidence issuer"):
        complete.accepted_set(foreign_set)


def test_completion_rejects_invalid_inventory_and_reason_shapes() -> None:
    """The coordinator fail-closes every semantic boundary, not only provenance."""

    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    slot_record = Record(1)
    other_slot_record = Record(3)
    pocket_record = Record(2)
    slot = ledger.propose(FamilyId.SLOTS, slot_record)
    other_slot = ledger.propose(FamilyId.SLOTS, other_slot_record)
    pocket = ledger.propose(FamilyId.POCKETS, pocket_record)
    slots = ledger.candidate_set_for(FamilyId.SLOTS, [slot_record, other_slot_record])
    pockets = ledger.candidate_set_for(FamilyId.POCKETS, [pocket_record])
    evidence = ledger.snapshot_index()

    with pytest.raises(ValueError, match="more than once"):
        ReconciliationResult.complete((slots, slots), (), evidence)
    with pytest.raises(ValueError, match="does not match its outcome"):
        ReconciliationResult.complete(
            (slots, pockets),
            (
                Disposition(
                    slot,
                    Outcome.ACCEPTED,
                    ReasonCode.SLOT_SUPERSEDED_BY_POCKET,
                    (pocket,),
                ),
            ),
            evidence,
        )
    with pytest.raises(ValueError, match="invalid related"):
        ReconciliationResult.complete(
            (slots,),
            (
                Disposition(
                    slot,
                    Outcome.REJECTED,
                    ReasonCode.SLOT_SUPERSEDED_BY_POCKET,
                ),
            ),
            evidence,
        )
    with pytest.raises(ValueError, match="related family"):
        ReconciliationResult.complete(
            (slots,),
            (
                Disposition(
                    slot,
                    Outcome.REJECTED,
                    ReasonCode.SLOT_SUPERSEDED_BY_POCKET,
                    (other_slot,),
                ),
            ),
            evidence,
        )


def test_legacy_and_uncovered_sets_cannot_enter_or_escape_reconciliation() -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    legacy_record = Record(1)
    slot_record = Record(2)
    pocket_record = Record(3)
    ledger.propose(FamilyId.LEGACY, legacy_record)
    ledger.propose(FamilyId.SLOTS, slot_record)
    ledger.propose(FamilyId.POCKETS, pocket_record)
    legacy = ledger.candidate_set_for(FamilyId.LEGACY, [legacy_record])
    slots = ledger.candidate_set_for(FamilyId.SLOTS, [slot_record])
    pockets = ledger.candidate_set_for(FamilyId.POCKETS, [pocket_record])
    evidence = ledger.snapshot_index()

    with pytest.raises(ValueError, match="legacy candidates"):
        ReconciliationResult.complete((legacy,), (), evidence)

    result = ReconciliationResult.complete((slots,), (), evidence)
    with pytest.raises(ValueError, match="not covered"):
        result.accepted_set(pockets)


def test_policy_execution_order_cannot_change_final_disposition_order() -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    records = [Record(1), Record(2)]
    candidates = [ledger.propose(FamilyId.CHAMFERS, record) for record in records]
    chamfers = ledger.candidate_set_for(FamilyId.CHAMFERS, records)
    decisions = tuple(
        Disposition(candidate, Outcome.ACCEPTED, ReasonCode.DEFAULT_ACCEPTED)
        for candidate in reversed(candidates)
    )

    result = ReconciliationResult.complete((chamfers,), decisions, ledger.snapshot_index())

    assert tuple(item.candidate for item in result.dispositions) == tuple(candidates)


def test_copied_issuer_token_cannot_forge_a_disposition_candidate() -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    record = Record(1)
    valid = ledger.propose(FamilyId.SLOTS, record)
    forged = object.__new__(Candidate)
    object.__setattr__(forged, "family", FamilyId.SLOTS)
    object.__setattr__(forged, "record", Record(2))
    object.__setattr__(forged, "evidence", Evidence(frozenset()))
    object.__setattr__(forged, "_issuer", valid._issuer)
    forged_set = object.__new__(CandidateSet)
    object.__setattr__(forged_set, "family", FamilyId.SLOTS)
    object.__setattr__(forged_set, "candidates", (forged,))
    object.__setattr__(forged_set, "_issuer", valid._issuer)

    with pytest.raises(ValueError, match="not present"):
        ReconciliationResult.complete((forged_set,), (), ledger.snapshot_index())


def test_projection_revalidates_candidate_state_and_set_family() -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    record = Record(1)
    candidate = ledger.propose(FamilyId.SLOTS, record)
    slots = ledger.candidate_set_for(FamilyId.SLOTS, [record])
    result = ReconciliationResult.complete((slots,), (), ledger.snapshot_index())
    wrong_family = object.__new__(CandidateSet)
    object.__setattr__(wrong_family, "family", FamilyId.HOLES)
    object.__setattr__(wrong_family, "candidates", (candidate,))
    object.__setattr__(wrong_family, "_issuer", candidate._issuer)

    with pytest.raises(ValueError, match="family does not match"):
        result.accepted_set(wrong_family)

    object.__setattr__(candidate, "record", Record(2))
    with pytest.raises(ValueError, match="issued state"):
        result.accepted_set(slots)
    with pytest.raises(ValueError, match="issued state"):
        result.for_family(FamilyId.SLOTS)


def test_related_candidates_are_canonicalized_to_physical_source_order() -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    slot_record = Record(1)
    pocket_records = [Record(2), Record(3)]
    slot = ledger.propose(FamilyId.SLOTS, slot_record)
    pockets = [ledger.propose(FamilyId.POCKETS, record) for record in pocket_records]
    slot_set = ledger.candidate_set_for(FamilyId.SLOTS, [slot_record])
    pocket_set = ledger.candidate_set_for(FamilyId.POCKETS, pocket_records)

    result = ReconciliationResult.complete(
        (slot_set, pocket_set),
        (
            Disposition(
                slot,
                Outcome.REJECTED,
                ReasonCode.SLOT_SUPERSEDED_BY_POCKET,
                tuple(reversed(pockets)),
            ),
        ),
        ledger.snapshot_index(),
    )

    assert result.dispositions[0].related == tuple(pockets)


def test_rectangular_slot_passage_precedence_is_a_real_rejected_trace() -> None:
    part = Box(60, 40, 20) - Box(30, 10, 30)

    product = result_module._take_inventory(part)
    rejected = [
        item
        for item in product.reconciliation.for_family(FamilyId.PASSAGES)
        if item.reason is ReasonCode.PASSAGE_SUPERSEDED_BY_SLOT
    ]

    assert len(rejected) == 1
    (rich_candidate,) = product.physical.candidate_set(FamilyId.PASSAGES).candidates
    assert rejected[0].candidate is rich_candidate
    assert isinstance(rich_candidate.record, SectionPassage)
    assert rejected[0].outcome is Outcome.REJECTED
    assert len(rejected[0].related) == 1
    assert rejected[0].related[0].family is FamilyId.SLOTS
    assert product.result.slots
    assert product._legacy_result.section_passages == ()


def test_empty_pocket_evidence_proves_neither_passage_nor_ring_containment() -> None:
    part = Box(60, 40, 20) - Box(10, 10, 60)
    ledger = ClaimLedger(FaceGraph(part))
    pocket = Record(1)
    passages_found = _discover_section_passages(part, ledger.graph, ledger.sink)
    ring = PrismaticPocket("z", 3, 5.0, 1, (0.0, 0.0, 0.0), ((0.0, 0.0),) * 3)
    ledger.propose(FamilyId.POCKETS, pocket)
    ledger.propose(FamilyId.PRISMATIC_POCKETS, ring, [ledger.graph.nodes[1]])
    slots = ledger.candidate_set_for(FamilyId.SLOTS, ())
    pockets = ledger.candidate_set_for(FamilyId.POCKETS, (pocket,))
    rings = ledger.candidate_set_for(FamilyId.PRISMATIC_POCKETS, (ring,))
    passages = ledger.candidate_set_for(FamilyId.PASSAGES, passages_found)
    evidence = ledger.snapshot_index()

    decisions = reconcile_recess_candidates(slots, pockets, rings, passages, evidence)

    assert all(item.candidate.record is not pocket for item in decisions)
