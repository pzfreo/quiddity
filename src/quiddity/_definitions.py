# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""What a family declares about itself, beside its code, for the registry to sequence.

A leaf below every family module. It names the shapes a declaration takes and nothing about
execution: the registry (`_registry`) is still the one ordered literal that says which families
run and in what order, and it validates the sequence and census of what it lists; the
manifest tool validates the evidence.
"""

from __future__ import annotations

from collections.abc import Callable, Mapping
from dataclasses import dataclass, field
from types import MappingProxyType
from typing import Literal, TypeAlias, TypeVar, cast

from quiddity._candidates import CompletedInputs, DerivedId, FamilyId
from quiddity._claims import EvidenceWriter
from quiddity._run import RecognitionContext
from quiddity._typing import CylinderInventory


@dataclass(frozen=True, slots=True)
class Counted:
    """A definition contributes to one existing stable census key."""

    key: str


@dataclass(frozen=True, slots=True)
class NotCounted:
    """A definition is deliberately absent from the feature census."""

    reason: str


CensusSpec: TypeAlias = Counted | NotCounted


@dataclass(frozen=True, slots=True)
class FullyAttributed:
    """Every aggregate output path has non-empty original-face defining evidence."""

    proof_contract: str


@dataclass(frozen=True, slots=True)
class IncompleteAttribution:
    """At least one output path lacks a reviewed complete ownership proof."""

    reason: str
    follow_up_or_exclusion: str


AttributionSpec: TypeAlias = FullyAttributed | IncompleteAttribution


@dataclass(frozen=True, slots=True)
class DiscoveryServices:
    """Run facts and the sole write capability available to registry adapters."""

    context: RecognitionContext
    writer: EvidenceWriter
    cylinders: CylinderInventory


RecordT = TypeVar("RecordT")


@dataclass(frozen=True, slots=True)
class AcceptedInputs:
    """Read-only accepted records for exactly one derived definition's sources."""

    _allowed: frozenset[FamilyId]
    _records: Mapping[FamilyId, tuple[object, ...]]

    @classmethod
    def restricted(
        cls,
        allowed: tuple[FamilyId, ...],
        accepted: Mapping[FamilyId, tuple[object, ...]],
    ) -> AcceptedInputs:
        return cls(
            frozenset(allowed),
            MappingProxyType({family: accepted[family] for family in allowed}),
        )

    def records(self, family: FamilyId, record_type: type[RecordT]) -> tuple[RecordT, ...]:
        if family not in self._allowed:
            raise ValueError(f"{family.value} is not a declared accepted source")
        records = self._records[family]
        if not all(isinstance(record, record_type) for record in records):
            raise TypeError(f"{family.value} source has the wrong record type")
        return cast(tuple[RecordT, ...], records)


PhysicalDiscoverer: TypeAlias = Callable[[DiscoveryServices, CompletedInputs], list[object]]
Applicability: TypeAlias = Callable[[RecognitionContext], bool]


def always(context: RecognitionContext) -> bool:
    del context
    return True


def prismatic(context: RecognitionContext) -> bool:
    return not context.rotational


#: The release the manifest attributes a family to when the declaration names none.
FIRST_RELEASE = "0.2.0"


#: The roles `capabilities.validate_capability_manifest` accepts for a published record.
ExtraRecordRole = Literal["aggregate", "evidence", "nested", "output", "projection"]


@dataclass(frozen=True, slots=True)
class ManifestEvidence:
    """What the capability manifest publishes for a family (ADR 0005).

    Not the run-local `_candidates.Evidence`: this is the golden and test paths a consumer can
    open, not the faces a record was proven from.
    """

    goldens: tuple[str, ...] = ()
    golden_paths: tuple[str, ...] = ()
    tests: tuple[str, ...] = ()
    introduced: str = FIRST_RELEASE
    #: Records the family publishes that are not its `record_types` output: nested parts of a
    #: record, aggregate documents, and projection outputs that reach a different result field.
    #: `(name, role, membership)`, where an empty membership means the record is reached through
    #: another rather than through a field of its own.
    extra_records: tuple[tuple[str, ExtraRecordRole, tuple[str, ...]], ...] = ()


@dataclass(frozen=True, slots=True)
class PhysicalDefinition:
    family: FamilyId
    record_types: tuple[type[object], ...]
    result_field: str
    public_entrypoint: str
    dependencies: tuple[FamilyId, ...]
    applicable: Applicability
    discover: PhysicalDiscoverer
    census: CensusSpec
    attribution: AttributionSpec
    projected: Applicability = always
    evidence: ManifestEvidence | None = field(default=None, kw_only=True)


@dataclass(frozen=True, slots=True)
class DerivedDefinition:
    identifier: DerivedId
    record_types: tuple[type[object], ...]
    result_field: str
    public_entrypoint: str
    sources: tuple[FamilyId, ...]
    derive: Callable[[AcceptedInputs], list[object]]
    census: CensusSpec
    evidence: ManifestEvidence | None = field(default=None, kw_only=True)
