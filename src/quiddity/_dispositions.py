# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Identity-safe final outcomes for completed physical candidates (ADR 0003)."""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum

from quiddity._candidates import Candidate, CandidateSet, EvidenceIndex, FamilyId


class Outcome(Enum):
    ACCEPTED = "accepted"
    REJECTED = "rejected"


class ReasonCode(Enum):
    DEFAULT_ACCEPTED = "default.accepted"
    PRISMATIC_SUPERSEDED_BY_POCKET = "recess.prismatic_superseded_by_pocket"
    POCKET_SUPERSEDED_BY_RECTANGULAR_BLIND_SLOT = (
        "recess.pocket_superseded_by_rectangular_blind_slot"
    )
    POCKET_SUPERSEDED_BY_EDGE_OPEN_CIRCULAR_POCKET = (
        "recess.pocket_superseded_by_edge_open_circular_pocket"
    )
    POCKET_SUPERSEDED_BY_PASSAGE = "recess.pocket_superseded_by_passage"
    POCKET_SUPERSEDED_BY_PRISMATIC = "recess.pocket_superseded_by_prismatic"
    SLOT_SUPERSEDED_BY_POCKET = "recess.slot_superseded_by_pocket"
    SLOT_SUPERSEDED_BY_PRISMATIC = "recess.slot_superseded_by_prismatic"
    SLOT_SUPERSEDED_BY_PASSAGE = "recess.slot_superseded_by_passage"
    PASSAGE_SUPERSEDED_BY_SLOT = "recess.passage_superseded_by_slot"
    PASSAGE_SUPERSEDED_BY_ORIENTED_SLOT = "recess.passage_superseded_by_oriented_slot"
    CHAMFER_SUPERSEDED_BY_ANGLED_STEP = "bevel.chamfer_superseded_by_angled_step"
    FILLET_SUPERSEDED_BY_CIRCULAR_BLIND_STEP = "blend.fillet_superseded_by_circular_blind_step"
    BLEND_SUPERSEDED_BY_FILLET = "blend.chain_superseded_by_fillet"
    HOLE_SUPERSEDED_BY_DOUBLE_D_BORE = "bore.hole_superseded_by_double_d_bore"
    TURNED_STEP_GROOVE_COMPATIBLE = "turned.step_groove_compatible"
    GROOVE_TURNED_STEP_COMPATIBLE = "turned.groove_step_compatible"


@dataclass(frozen=True, eq=False, slots=True)
class Disposition:
    """One final identity outcome, optionally related to its actual winners or peers."""

    candidate: Candidate[object]
    outcome: Outcome
    reason: ReasonCode
    related: tuple[Candidate[object], ...] = ()


_REASON_SPEC: dict[ReasonCode, tuple[Outcome, FamilyId | None, FamilyId | None, bool]] = {
    ReasonCode.DEFAULT_ACCEPTED: (Outcome.ACCEPTED, None, None, False),
    ReasonCode.PRISMATIC_SUPERSEDED_BY_POCKET: (
        Outcome.REJECTED,
        FamilyId.PRISMATIC_POCKETS,
        FamilyId.POCKETS,
        True,
    ),
    ReasonCode.POCKET_SUPERSEDED_BY_RECTANGULAR_BLIND_SLOT: (
        Outcome.REJECTED,
        FamilyId.POCKETS,
        FamilyId.RECTANGULAR_BLIND_SLOTS,
        True,
    ),
    ReasonCode.POCKET_SUPERSEDED_BY_EDGE_OPEN_CIRCULAR_POCKET: (
        Outcome.REJECTED,
        FamilyId.POCKETS,
        FamilyId.EDGE_OPEN_CIRCULAR_POCKETS,
        True,
    ),
    ReasonCode.POCKET_SUPERSEDED_BY_PASSAGE: (
        Outcome.REJECTED,
        FamilyId.POCKETS,
        FamilyId.PASSAGES,
        True,
    ),
    ReasonCode.POCKET_SUPERSEDED_BY_PRISMATIC: (
        Outcome.REJECTED,
        FamilyId.POCKETS,
        FamilyId.PRISMATIC_POCKETS,
        True,
    ),
    ReasonCode.SLOT_SUPERSEDED_BY_POCKET: (
        Outcome.REJECTED,
        FamilyId.SLOTS,
        FamilyId.POCKETS,
        True,
    ),
    ReasonCode.SLOT_SUPERSEDED_BY_PRISMATIC: (
        Outcome.REJECTED,
        FamilyId.SLOTS,
        FamilyId.PRISMATIC_POCKETS,
        True,
    ),
    ReasonCode.SLOT_SUPERSEDED_BY_PASSAGE: (
        Outcome.REJECTED,
        FamilyId.SLOTS,
        FamilyId.PASSAGES,
        True,
    ),
    ReasonCode.PASSAGE_SUPERSEDED_BY_SLOT: (
        Outcome.REJECTED,
        FamilyId.PASSAGES,
        FamilyId.SLOTS,
        True,
    ),
    ReasonCode.PASSAGE_SUPERSEDED_BY_ORIENTED_SLOT: (
        Outcome.REJECTED,
        FamilyId.PASSAGES,
        FamilyId.ORIENTED_SLOTS,
        True,
    ),
    ReasonCode.CHAMFER_SUPERSEDED_BY_ANGLED_STEP: (
        Outcome.REJECTED,
        FamilyId.CHAMFERS,
        FamilyId.ANGLED_STEPS,
        True,
    ),
    ReasonCode.FILLET_SUPERSEDED_BY_CIRCULAR_BLIND_STEP: (
        Outcome.REJECTED,
        FamilyId.FILLETS,
        FamilyId.CIRCULAR_BLIND_STEPS,
        True,
    ),
    ReasonCode.BLEND_SUPERSEDED_BY_FILLET: (
        Outcome.REJECTED,
        FamilyId.BLENDS,
        FamilyId.FILLETS,
        True,
    ),
    ReasonCode.HOLE_SUPERSEDED_BY_DOUBLE_D_BORE: (
        Outcome.REJECTED,
        FamilyId.HOLES,
        FamilyId.DOUBLE_D_BORES,
        True,
    ),
    ReasonCode.TURNED_STEP_GROOVE_COMPATIBLE: (
        Outcome.ACCEPTED,
        FamilyId.TURNED_STEPS,
        FamilyId.GROOVES,
        True,
    ),
    ReasonCode.GROOVE_TURNED_STEP_COMPATIBLE: (
        Outcome.ACCEPTED,
        FamilyId.GROOVES,
        FamilyId.TURNED_STEPS,
        True,
    ),
}
if set(_REASON_SPEC) != set(ReasonCode):
    raise ValueError("every closed reason needs one semantic specification")


@dataclass(frozen=True, init=False, slots=True)
class ReconciliationResult:
    """The sole ordered truth from which accepted physical candidates are projected."""

    dispositions: tuple[Disposition, ...]
    _issuer: object
    _membership: frozenset[int]
    _evidence: EvidenceIndex

    @classmethod
    def complete(
        cls,
        physical: tuple[CandidateSet[object], ...],
        decisions: tuple[Disposition, ...],
        evidence: EvidenceIndex,
    ) -> ReconciliationResult:
        issuer = physical[0]._issuer if physical else None
        ordered_list: list[Candidate[object]] = []
        physical_seen: set[int] = set()
        for group in physical:
            evidence.validate_candidate_set(group)
            if group._issuer is not issuer:
                raise ValueError("physical candidate sets belong to different issuers")
            if group.family is FamilyId.LEGACY:
                raise ValueError("legacy candidates cannot receive aggregate dispositions")
            for candidate in group.candidates:
                if candidate._issuer is not issuer or candidate.family is not group.family:
                    raise ValueError("candidate does not match its physical candidate set")
                if id(candidate) in physical_seen:
                    raise ValueError("physical candidate occurs more than once")
                physical_seen.add(id(candidate))
                ordered_list.append(candidate)
        ordered = tuple(ordered_list)
        membership = {id(candidate): candidate for candidate in ordered}
        source_position = {id(candidate): index for index, candidate in enumerate(ordered)}
        decided: dict[int, Disposition] = {}
        for disposition in decisions:
            if not isinstance(disposition.outcome, Outcome) or not isinstance(
                disposition.reason, ReasonCode
            ):
                raise ValueError("disposition outcome and reason must use closed enums")
            candidate_member = membership.get(id(disposition.candidate))
            if candidate_member is not disposition.candidate:
                raise ValueError("disposition candidate is not in the physical inventory")
            if id(candidate_member) in decided:
                raise ValueError("physical candidate received more than one disposition")
            expected_outcome, subject_family, related_family, requires_related = _REASON_SPEC[
                disposition.reason
            ]
            if disposition.outcome is not expected_outcome:
                raise ValueError("disposition reason does not match its outcome")
            if subject_family is not None and disposition.candidate.family is not subject_family:
                raise ValueError("disposition reason does not match its candidate family")
            if requires_related != bool(disposition.related):
                raise ValueError("disposition reason has invalid related candidates")
            related_seen: set[int] = set()
            canonical_related = tuple(
                sorted(disposition.related, key=lambda item: source_position.get(id(item), -1))
            )
            for related in canonical_related:
                related_member = membership.get(id(related))
                if related_member is not related:
                    raise ValueError("related candidate is not in the physical inventory")
                if related is candidate_member or id(related) in related_seen:
                    raise ValueError("related candidates must be distinct and not self-related")
                related_seen.add(id(related))
                if related_family is not None and related.family is not related_family:
                    raise ValueError("disposition reason does not match related family")
            decided[id(candidate_member)] = (
                disposition
                if canonical_related == disposition.related
                else Disposition(
                    disposition.candidate,
                    disposition.outcome,
                    disposition.reason,
                    canonical_related,
                )
            )

        result = []
        for candidate in ordered:
            result.append(
                decided.get(
                    id(candidate),
                    Disposition(candidate, Outcome.ACCEPTED, ReasonCode.DEFAULT_ACCEPTED),
                )
            )
        completed = object.__new__(cls)
        object.__setattr__(completed, "dispositions", tuple(result))
        object.__setattr__(completed, "_issuer", issuer)
        object.__setattr__(completed, "_membership", frozenset(membership))
        object.__setattr__(completed, "_evidence", evidence)
        return completed

    def accepted_set(self, source: CandidateSet[object]) -> CandidateSet[object]:
        """Return accepted members of *source* without storing a second roster."""

        self._evidence.validate_candidate_set(source)
        if source._issuer is not self._issuer or any(
            id(candidate) not in self._membership for candidate in source.candidates
        ):
            raise ValueError("candidate set is not covered by this reconciliation")

        accepted = {
            id(item.candidate) for item in self.dispositions if item.outcome is Outcome.ACCEPTED
        }
        result = object.__new__(CandidateSet)
        object.__setattr__(result, "family", source.family)
        object.__setattr__(
            result,
            "candidates",
            tuple(candidate for candidate in source.candidates if id(candidate) in accepted),
        )
        object.__setattr__(result, "_issuer", source._issuer)
        return result

    def for_family(self, family: FamilyId) -> tuple[Disposition, ...]:
        for item in self.dispositions:
            self._evidence.defining_of(item.candidate)
        return tuple(item for item in self.dispositions if item.candidate.family is family)
