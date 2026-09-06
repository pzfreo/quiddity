# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Private, geometry-free residual diagnostics over frozen run evidence."""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum

from quiddity._candidates import EvidenceIndex, FamilyId, Observation, PredicateId
from quiddity._dispositions import Outcome, ReconciliationResult
from quiddity.chamfers import Chamfer


class DiagnosticStatus(Enum):
    UNSUPPORTED = "unsupported"


class DiagnosticCode(Enum):
    UNSUPPORTED_SUBDIVIDED_ANGLED_STEP_TERMINAL = "unsupported.subdivided_angled_step_terminal"


@dataclass(frozen=True, slots=True)
class ResidualDiagnostic:
    """Stable private summary; no graph or kernel identity escapes the run."""

    code: DiagnosticCode
    status: DiagnosticStatus
    family: str
    axis: str
    at: tuple[float, float, float]
    raw_outer_edges: int
    effective_outer_sides: int

    def to_dict(self) -> dict[str, str | int | list[float]]:
        return {
            "code": self.code.value,
            "status": self.status.value,
            "family": self.family,
            "axis": self.axis,
            "at": list(self.at),
            "raw_outer_edges": self.raw_outer_edges,
            "effective_outer_sides": self.effective_outer_sides,
        }


def diagnose_residuals(
    reconciliation: ReconciliationResult,
    evidence: EvidenceIndex,
) -> tuple[ResidualDiagnostic, ...]:
    """Join bounded failed-terminal observations to accepted planar Chamfers."""

    accepted = tuple(
        disposition.candidate
        for disposition in reconciliation.dispositions
        if disposition.outcome is Outcome.ACCEPTED
    )
    grouped: dict[int, list[Observation]] = {}
    for observation in evidence.observations(
        FamilyId.ANGLED_STEPS, PredicateId.ANGLED_STEP_TERMINAL
    ):
        grouped.setdefault(id(observation.subject), []).append(observation)

    diagnostics: list[ResidualDiagnostic] = []
    for observations in grouped.values():
        observation = min(
            observations,
            key=lambda item: (item.fact.raw_outer_edges, item.fact.effective_outer_sides),
        )
        subject = frozenset({observation.subject})
        owners = tuple(
            candidate
            for candidate in accepted
            if observation.subject in evidence.defining_of(candidate)
            # FaceLevel and Riser candidates are neutral profile substrate, not accepted
            # manufacturing-feature explanations. Their newly complete attribution must not
            # erase an independently observed unsupported AngledStep terminal.
            and candidate.family not in {FamilyId.STEP_LEVELS, FamilyId.RISERS}
        )
        if any(candidate.family is not FamilyId.CHAMFERS for candidate in owners):
            continue
        chamfers = tuple(
            candidate.record
            for candidate in owners
            if candidate.family is FamilyId.CHAMFERS
            and evidence.defining_of(candidate) == subject
            and isinstance(candidate.record, Chamfer)
            and not candidate.record.turned
        )
        if chamfers:
            chamfer = chamfers[0]
            diagnostics.append(
                ResidualDiagnostic(
                    code=DiagnosticCode.UNSUPPORTED_SUBDIVIDED_ANGLED_STEP_TERMINAL,
                    status=DiagnosticStatus.UNSUPPORTED,
                    family="angled_steps",
                    axis=chamfer.axis,
                    at=chamfer.at,
                    raw_outer_edges=observation.fact.raw_outer_edges,
                    effective_outer_sides=observation.fact.effective_outer_sides,
                )
            )
    return tuple(
        sorted(
            diagnostics,
            key=lambda item: (
                item.code.value,
                item.axis,
                item.at,
                item.raw_outer_edges,
            ),
        )
    )
