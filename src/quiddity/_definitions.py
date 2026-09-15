# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""What a family declares about itself, beside its code, for the registry to sequence.

A leaf below every family module. It names the shapes a declaration takes and nothing about
execution: the registry (`_registry`) is still the one ordered literal that says which families
run and in what order, and it validates every declaration it lists.
"""

from __future__ import annotations

from collections.abc import Callable, Mapping
from dataclasses import dataclass, field
from types import MappingProxyType
from typing import TypeAlias, TypeVar, cast

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
DerivedDiscoverer: TypeAlias = Callable[[AcceptedInputs], list[object]]


def always(context: RecognitionContext) -> bool:
    del context
    return True


def prismatic(context: RecognitionContext) -> bool:
    return not context.rotational


def simple(call: Callable[[DiscoveryServices], list[object]]) -> PhysicalDiscoverer:
    """Adapt a discoverer that needs no completed predecessor inputs."""

    def discover(services: DiscoveryServices, inputs: CompletedInputs) -> list[object]:
        del inputs
        return call(services)

    return discover


@dataclass(frozen=True, slots=True)
class Evidence:
    """What the capability manifest publishes for a family (ADR 0005)."""

    goldens: tuple[str, ...] = ()
    golden_paths: tuple[str, ...] = ()
    tests: tuple[str, ...] = ()
    introduced: str = "0.2.0"


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
    evidence: Evidence | None = field(default=None, kw_only=True)


@dataclass(frozen=True, slots=True)
class DerivedDefinition:
    identifier: DerivedId
    record_types: tuple[type[object], ...]
    result_field: str
    public_entrypoint: str | None
    sources: tuple[FamilyId, ...]
    derive: Callable[..., list[object]]
    census: CensusSpec
    role: str = "discoverer"
    evidence: Evidence | None = field(default=None, kw_only=True)
