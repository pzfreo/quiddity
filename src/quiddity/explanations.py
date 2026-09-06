# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Bounded public explanations projected from one completed recognition run.

These values distinguish evaluated absence, classification gating and reconciliation loss. They
are deliberately not an exhaustive account of geometry that recognition did not accept.
"""

from __future__ import annotations

from collections import Counter
from dataclasses import dataclass
from enum import Enum

from quiddity._diagnostics import ResidualDiagnostic
from quiddity._dispositions import Outcome, ReasonCode
from quiddity._registry import PHYSICAL_DEFINITIONS
from quiddity._typing import CylinderInventory, Part
from quiddity.result import InventoryProduct, RecognitionResult, _take_inventory


class ExplanationCoverage(Enum):
    """How completely a report can explain non-recognition."""

    BOUNDED = "bounded"


class FamilyEvaluation(Enum):
    """Whether one family executed for the aggregate's classification."""

    EVALUATED = "evaluated"
    NOT_APPLICABLE = "not-applicable"


class RecognitionOutcome(Enum):
    """Public value projection of a final candidate outcome."""

    ACCEPTED = "accepted"
    REJECTED = "rejected"


class ReconciliationReason(Enum):
    """Closed public projection of aggregate reconciliation reasons."""

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
    FILLET_SUPERSEDED_BY_CIRCULAR_BLIND_STEP = (
        "blend.fillet_superseded_by_circular_blind_step"
    )
    BLEND_SUPERSEDED_BY_FILLET = "blend.chain_superseded_by_fillet"
    HOLE_SUPERSEDED_BY_DOUBLE_D_BORE = "bore.hole_superseded_by_double_d_bore"
    TURNED_STEP_GROOVE_COMPATIBLE = "turned.step_groove_compatible"
    GROOVE_TURNED_STEP_COMPATIBLE = "turned.groove_step_compatible"


class RecognitionDiagnosticStatus(Enum):
    """Closed status of a supported bounded recognition diagnostic."""

    UNSUPPORTED = "unsupported"


class RecognitionDiagnosticCode(Enum):
    """Closed codes currently established by frozen run evidence."""

    UNSUPPORTED_SUBDIVIDED_ANGLED_STEP_TERMINAL = (
        "unsupported.subdivided_angled_step_terminal"
    )


@dataclass(frozen=True, slots=True)
class DispositionExplanation:
    """Candidate outcomes sharing one reason, not published physical occurrences.

    ``occurrences`` counts candidate dispositions; ``related_occurrences`` sums their
    related-candidate links and may count the same related candidate more than once.
    """

    reason: ReconciliationReason
    outcome: RecognitionOutcome
    occurrences: int
    related_occurrences: int


@dataclass(frozen=True, slots=True)
class FamilyExplanation:
    """Bounded lifecycle counts for one detector family.

    Accepted candidates precede public projection and deduplication. Several detectors
    can contribute to one published occurrence; these counts are not a feature census.
    """

    family: str
    evaluation: FamilyEvaluation
    proposed: int
    accepted: int
    rejected: int
    dispositions: tuple[DispositionExplanation, ...]


@dataclass(frozen=True, slots=True)
class RecognitionDiagnostic:
    """Kernel- and identity-free projection of one established residual diagnostic."""

    code: RecognitionDiagnosticCode
    status: RecognitionDiagnosticStatus
    family: str
    axis: str
    at: tuple[float, float, float]
    raw_outer_edges: int
    effective_outer_sides: int


@dataclass(frozen=True, slots=True)
class RecognitionReport:
    """One public result and detector explanations from exactly the same run.

    Use ``detector_families`` for candidate lifecycle counts and the collections in
    ``result`` for published records. ``families`` retains the original detector-count
    field; neither name counts deduplicated public occurrences.
    """

    coverage: ExplanationCoverage
    result: RecognitionResult
    families: tuple[FamilyExplanation, ...]
    diagnostics: tuple[RecognitionDiagnostic, ...]

    @property
    def detector_families(self) -> tuple[FamilyExplanation, ...]:
        """Explicitly named view of detector counts, preserving their provenance.

        For example, accepted ``pockets`` and ``section_recesses`` candidates may
        publish one record in ``result.section_recesses``. Summing accepted counts
        would count detector decisions, not distinct recognised features.
        """

        return self.families


def _project_diagnostic(item: ResidualDiagnostic) -> RecognitionDiagnostic:
    return RecognitionDiagnostic(
        code=RecognitionDiagnosticCode(item.code.value),
        status=RecognitionDiagnosticStatus(item.status.value),
        family=item.family,
        axis=item.axis,
        at=item.at,
        raw_outer_edges=item.raw_outer_edges,
        effective_outer_sides=item.effective_outer_sides,
    )


def _project_report(product: InventoryProduct) -> RecognitionReport:
    families: list[FamilyExplanation] = []
    for definition in PHYSICAL_DEFINITIONS:
        candidates = product.physical.candidate_set(definition.family).candidates
        dispositions = product.reconciliation.for_family(definition.family)
        grouped: Counter[tuple[ReasonCode, Outcome]] = Counter(
            (item.reason, item.outcome) for item in dispositions
        )
        related: Counter[tuple[ReasonCode, Outcome]] = Counter()
        for item in dispositions:
            related[(item.reason, item.outcome)] += len(item.related)
        summaries = tuple(
            DispositionExplanation(
                reason=ReconciliationReason(reason.value),
                outcome=RecognitionOutcome(outcome.value),
                occurrences=count,
                related_occurrences=related[(reason, outcome)],
            )
            for (reason, outcome), count in sorted(
                grouped.items(), key=lambda item: (item[0][0].value, item[0][1].value)
            )
        )
        accepted = sum(item.outcome is Outcome.ACCEPTED for item in dispositions)
        rejected = sum(item.outcome is Outcome.REJECTED for item in dispositions)
        families.append(
            FamilyExplanation(
                family=definition.family.value,
                evaluation=(
                    FamilyEvaluation.EVALUATED
                    if definition.applicable(product.context)
                    else FamilyEvaluation.NOT_APPLICABLE
                ),
                proposed=len(candidates),
                accepted=accepted,
                rejected=rejected,
                dispositions=summaries,
            )
        )
    return RecognitionReport(
        coverage=ExplanationCoverage.BOUNDED,
        result=product.result,
        families=tuple(families),
        diagnostics=tuple(_project_diagnostic(item) for item in product.diagnostics),
    )


def build_raw_recognition_report(
    part: Part,
    *,
    cylinders: CylinderInventory | None = None,
    rotational: bool = False,
) -> RecognitionReport:
    """Recognise once in caller coordinates and return bounded lifecycle explanations.

    An absent diagnostic is not proof that the part contains no unsupported geometry. Families
    marked ``evaluated`` completed their current recogniser and may still exclude geometry outside
    its documented supported domain.
    """

    return _project_report(_take_inventory(part, cylinders=cylinders, rotational=rotational))


def build_recognition_report(
    part: Part,
    *,
    cylinders: CylinderInventory | None = None,
    rotational: bool = False,
) -> RecognitionReport:
    """Compatibility name for a caller-coordinate bounded report.

    New integrations should ordinarily use ``build_framed_recognition_report``.  Use
    ``build_raw_recognition_report`` when caller/world coordinates are deliberate.
    """

    return build_raw_recognition_report(
        part,
        cylinders=cylinders,
        rotational=rotational,
    )


__all__ = [
    "DispositionExplanation",
    "ExplanationCoverage",
    "FamilyEvaluation",
    "FamilyExplanation",
    "RecognitionDiagnostic",
    "RecognitionDiagnosticCode",
    "RecognitionDiagnosticStatus",
    "RecognitionOutcome",
    "RecognitionReport",
    "ReconciliationReason",
    "build_raw_recognition_report",
    "build_recognition_report",
]
