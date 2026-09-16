# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Closed internal execution registry for recognition orchestration.

The registry is deliberately not a plugin system and does not publish API or schema.  It owns
only physical discovery order, declared physical dependencies, neutral applicability, derived
pattern order, and explicit census coverage.  Public exports, capability metadata, result
projection, reconciliation policy, and census key order remain independently reviewed surfaces.
"""

from __future__ import annotations

from collections.abc import Callable, Mapping
from dataclasses import dataclass, field
from typing import Protocol, TypeAlias

from quiddity import (
    angled_steps,
    blends,
    chamfers,
    circular_blind_steps,
    countersinks,
    edge_open_circular_recesses,
    edge_open_prismatic_recesses,
    fillets,
    flats,
    grooves,
    gussets,
    levels,
    oriented_slots,
    pads,
    paired_ramp_steps,
    plates,
    polygonal_bosses,
    prismatic_pockets,
    profiled_bores,
    rectangular_blind_slots,
    repeating_profiles,
    round_bottom_slots,
    section_recesses,
    slots,
    through_steps,
    turned,
)
from quiddity._candidates import (
    Candidate,
    CandidateSet,
    CompletedInputs,
    DerivedId,
    EvidenceIndex,
    FamilyId,
)
from quiddity._definitions import (
    AcceptedInputs,
    CensusSpec,
    Counted,
    DerivedDefinition,
    DiscoveryServices,
    FullyAttributed,
    IncompleteAttribution,
    ManifestEvidence,
    NotCounted,
    PhysicalDefinition,
    always,
    prismatic,
    simple,
)
from quiddity._features import (
    BoltCircle,
    BossRecord,
    HoleRecord,
    LinearArray,
    RectGrid,
    recognise_hole_patterns,
)
from quiddity._hole_features import _discover_bosses, _discover_holes
from quiddity._passage_compat import PassageCompatibilityView, passage_from_view
from quiddity.countersinks import CounterSink
from quiddity.passages import (
    Passage,
    SectionPassage,
    recognise_section_passages,
)

# Internal detector identities survive the public SectionRecess schema replacement so that
# discovery, reconciliation and effectiveness scoring remain comparable across the cutover.
RECESS_SOURCE_FAMILIES = frozenset(
    {
        FamilyId.POCKETS,
        FamilyId.CHANNELS,
        FamilyId.PRISMATIC_POCKETS,
        FamilyId.PASSAGES,
        FamilyId.EDGE_OPEN_PRISMATIC_RECESSES,
        FamilyId.EDGE_OPEN_CIRCULAR_POCKETS,
        FamilyId.RECTANGULAR_BLIND_SLOTS,
        FamilyId.ROUND_BOTTOM_BLIND_SLOTS,
    }
)


@dataclass(frozen=True, slots=True)
class ProjectionInputs:
    """The sole already-decided aggregate applicability fact available to projections."""

    projected: bool


@dataclass(frozen=True, slots=True)
class _ProjectionInputSnapshot:
    inputs: AcceptedProjectionInputs
    candidate_set: CandidateSet[object]
    candidates: tuple[Candidate[object], ...]
    evidence: EvidenceIndex


class _ProjectionInputAuthority(Protocol):
    def validate(self, inputs: AcceptedProjectionInputs) -> _ProjectionInputSnapshot: ...


@dataclass(frozen=True, slots=True, init=False)
class AcceptedProjectionInputs:
    """Exact accepted occurrence identities and their issuer-validated compatibility facts."""

    _allowed: frozenset[FamilyId]
    _candidate_set: CandidateSet[object]
    _candidates: tuple[Candidate[object], ...]
    _evidence: EvidenceIndex
    _issuer: _ProjectionInputAuthority

    def passage_views(
        self,
    ) -> tuple[tuple[SectionPassage, PassageCompatibilityView], ...]:
        family = FamilyId.PASSAGES
        if family not in self._allowed:
            raise ValueError("passages is not a declared accepted projection source")
        snapshot = self._issuer.validate(self)
        if snapshot.candidate_set.family is not family:
            raise ValueError("accepted passages projection source family changed")
        result: list[tuple[SectionPassage, PassageCompatibilityView]] = []
        seen: set[int] = set()
        for candidate in self._candidates:
            if id(candidate) in seen:
                raise ValueError("accepted passages projection roster contains a duplicate")
            seen.add(id(candidate))
            if not isinstance(candidate.record, SectionPassage):
                raise TypeError("passages projection source has the wrong record type")
            result.append((candidate.record, self._evidence.passage_compatibility(candidate)))
        return tuple(result)


def _projection_authority_factory():
    """Close the mint token inside one function closure, never a module attribute."""

    authority = object()

    def mint(accepted: CandidateSet[object], evidence: EvidenceIndex) -> AcceptedProjectionInputs:
        if accepted.family is not FamilyId.PASSAGES:
            raise ValueError("projection inputs require the accepted passages candidate set")
        evidence.validate_candidate_set(accepted)
        original: _ProjectionInputSnapshot | None = None

        class Issuer:
            def __init__(self, supplied: object) -> None:
                if supplied is not authority:
                    raise ValueError("projection input issuer lacks orchestration authority")

            def validate(self, inputs: AcceptedProjectionInputs) -> _ProjectionInputSnapshot:
                snapshot = original
                if snapshot is None or snapshot.inputs is not inputs:
                    raise ValueError("accepted projection inputs were not issued by orchestration")
                if (
                    inputs._issuer is not self
                    or inputs._candidate_set is not snapshot.candidate_set
                    or inputs._evidence is not snapshot.evidence
                    or snapshot.candidate_set.candidates is not snapshot.candidates
                    or len(inputs._candidates) != len(snapshot.candidates)
                    or any(
                        current is not original_candidate
                        for current, original_candidate in zip(
                            inputs._candidates, snapshot.candidates, strict=True
                        )
                    )
                ):
                    raise ValueError("accepted passages projection roster changed after issuance")
                snapshot.evidence.validate_candidate_set(snapshot.candidate_set)
                return snapshot

        result = object.__new__(AcceptedProjectionInputs)
        object.__setattr__(result, "_allowed", frozenset((FamilyId.PASSAGES,)))
        object.__setattr__(result, "_candidate_set", accepted)
        object.__setattr__(result, "_candidates", accepted.candidates)
        object.__setattr__(result, "_evidence", evidence)
        issuer = Issuer(authority)
        object.__setattr__(result, "_issuer", issuer)
        original = _ProjectionInputSnapshot(result, accepted, accepted.candidates, evidence)
        return result

    return mint


_issue_projection_inputs = _projection_authority_factory()
del _projection_authority_factory


ProjectionDiscoverer: TypeAlias = Callable[
    [AcceptedProjectionInputs, ProjectionInputs], list[object]
]


@dataclass(frozen=True, slots=True)
class ProjectionDefinition:
    """A derived family projected from accepted occurrences rather than discovered.

    Separate from `DerivedDefinition` because its `derive` takes the projection input types
    declared just above, which live here rather than in the `_definitions` leaf: they reach
    `SectionPassage` and `PassageCompatibilityView`, which sit above it. Keeping the two apart
    types each `derive` exactly, and removes the `role` string that used to tell them apart.

    A projection publishes no entry point. Its records reach a caller through the aggregate.
    """

    identifier: DerivedId
    record_types: tuple[type[object], ...]
    result_field: str
    sources: tuple[FamilyId, ...]
    derive: ProjectionDiscoverer
    census: CensusSpec
    evidence: ManifestEvidence | None = field(default=None, kw_only=True)


def _holes(services: DiscoveryServices, inputs: CompletedInputs) -> list[object]:
    countersinks = list(inputs.records(FamilyId.COUNTERSINKS, CounterSink))
    occurrences = inputs.occurrences(FamilyId.COUNTERSINKS, CounterSink)
    return list(
        _discover_holes(
            services.context.part,
            cyls=services.cylinders,
            csinks=countersinks,
            face_edges=services.context.face_edges,
            writer=services.writer,
            predecessor_occurrences=occurrences,
            face_surfaces=services.context.face_surfaces,
        )
    )


def _hole_patterns(inputs: AcceptedInputs) -> list[object]:
    return list(recognise_hole_patterns(inputs.records(FamilyId.HOLES, HoleRecord)))


def _passages_compat(
    inputs: AcceptedProjectionInputs, projection: ProjectionInputs
) -> list[object]:
    if not projection.projected:
        return []
    found: list[tuple[Passage, int]] = []
    for _, fact in inputs.passage_views():
        if not fact.eligible:
            continue
        assert fact.legacy_ordinal is not None
        found.append((passage_from_view(fact, Passage), fact.legacy_ordinal))
    found.sort(key=lambda item: item[1])
    return [record for record, _ in found]


PHYSICAL_DEFINITIONS: tuple[PhysicalDefinition, ...] = (
    countersinks.DEFINITION,
    PhysicalDefinition(
        FamilyId.HOLES,
        (HoleRecord,),
        "holes",
        "recognise_holes",
        (FamilyId.COUNTERSINKS,),
        always,
        _holes,
        Counted("hole"),
        FullyAttributed(
            "every returned Hole claims its complete original cylindrical occurrence faces"
        ),
    ),
    profiled_bores.DEFINITION,
    PhysicalDefinition(
        FamilyId.BOSSES,
        (BossRecord,),
        "bosses",
        "recognise_bosses",
        (),
        always,
        simple(
            lambda s: list(
                _discover_bosses(
                    s.context.part,
                    cyls=s.cylinders,
                    face_edges=s.context.face_edges,
                    writer=s.writer,
                    face_surfaces=s.context.face_surfaces,
                )
            )
        ),
        Counted("boss"),
        FullyAttributed("every returned boss claims its original external segment faces"),
    ),
    polygonal_bosses.BOSSES,
    polygonal_bosses.STOCK,
    slots.CHANNELS,
    slots.SLOTS,
    rectangular_blind_slots.DEFINITION,
    round_bottom_slots.DEFINITION,
    grooves.DEFINITION,
    flats.DEFINITION,
    slots.POCKETS,
    prismatic_pockets.DEFINITION,
    edge_open_circular_recesses.DEFINITION,
    edge_open_prismatic_recesses.DEFINITION,
    section_recesses.DEFINITION,
    pads.DEFINITION,
    repeating_profiles.DEFINITION,
    turned.DEFINITION,
    levels.STEP_LEVELS,
    levels.RISERS,
    chamfers.DEFINITION,
    angled_steps.DEFINITION,
    paired_ramp_steps.DEFINITION,
    gussets.DEFINITION,
    through_steps.DEFINITION,
    circular_blind_steps.DEFINITION,
    PhysicalDefinition(
        FamilyId.PASSAGES,
        (SectionPassage,),
        "section_passages",
        "recognise_section_passages",
        (),
        always,
        simple(
            lambda s: list(
                recognise_section_passages(
                    s.context.part, ledger=s.writer, face_edges=s.context.face_edges
                )
            )
        ),
        NotCounted("Counted once through the unified section_recess projection"),
        FullyAttributed("every returned passage claims its defining passage faces"),
        projected=prismatic,
    ),
    oriented_slots.DEFINITION,
    blends.DEFINITION,
    fillets.DEFINITION,
    plates.DEFINITION,
)


DERIVED_DEFINITIONS: tuple[DerivedDefinition, ...] = (
    DerivedDefinition(
        DerivedId.HOLE_PATTERNS,
        (BoltCircle, LinearArray, RectGrid),
        "hole_patterns",
        "recognise_hole_patterns",
        (FamilyId.HOLES,),
        _hole_patterns,
        Counted("hole_pattern"),
    ),
    slots.SLOT_PATTERNS,
    oriented_slots.PATTERNS,
    slots.POCKET_PATTERNS,
    gussets.PATTERNS,
)

PROJECTION_DEFINITIONS: tuple[ProjectionDefinition, ...] = (
    ProjectionDefinition(
        DerivedId.PASSAGES_COMPAT,
        (Passage,),
        "passages",
        (FamilyId.PASSAGES,),
        _passages_compat,
        NotCounted("compatibility projection of accepted section passages"),
    ),
)


def validate_definitions(
    physical: tuple[PhysicalDefinition, ...],
    derived: tuple[DerivedDefinition, ...],
    projections: tuple[ProjectionDefinition, ...],
) -> None:
    """Fail closed when the closed internal registry is incomplete or incoherent."""

    families = tuple(definition.family for definition in physical)
    expected = tuple(family for family in FamilyId if family is not FamilyId.LEGACY)
    if len(set(families)) != len(families) or set(families) != set(expected):
        raise ValueError("physical definitions must cover every non-legacy family exactly once")
    positions = {family: index for index, family in enumerate(families)}
    fields = [definition.result_field for definition in physical]
    if len(set(fields)) != len(fields):
        raise ValueError("physical result fields must be unique")
    every_derived: tuple[DerivedDefinition | ProjectionDefinition, ...] = (*derived, *projections)
    counted_keys = [
        definition.census.key for definition in physical if isinstance(definition.census, Counted)
    ] + [
        definition.census.key
        for definition in every_derived
        if isinstance(definition.census, Counted)
    ]
    if len(set(counted_keys)) != len(counted_keys) or any(not key for key in counted_keys):
        raise ValueError("counted census keys must be non-empty and unique")
    for index, definition in enumerate(physical):
        if not definition.record_types or not definition.public_entrypoint:
            raise ValueError("physical definitions require record and public contracts")
        if not isinstance(definition.census, Counted | NotCounted):
            raise ValueError("physical definitions require an explicit census disposition")
        if isinstance(definition.census, NotCounted) and not definition.census.reason:
            raise ValueError("not-counted census reasons must be non-empty")
        if not isinstance(definition.attribution, FullyAttributed | IncompleteAttribution):
            raise ValueError("physical definitions require an attribution disposition")
        if (
            isinstance(definition.attribution, FullyAttributed)
            and not definition.attribution.proof_contract.strip()
        ):
            raise ValueError("fully-attributed proof contracts must be non-empty")
        if isinstance(definition.attribution, IncompleteAttribution) and (
            not definition.attribution.reason.strip()
            or not definition.attribution.follow_up_or_exclusion.strip()
        ):
            raise ValueError("incomplete-attribution reasons and dispositions must be non-empty")
        if definition.applicable not in {always, prismatic}:
            raise ValueError("physical applicability must use a reviewed neutral predicate")
        if definition.projected not in {always, prismatic}:
            raise ValueError("physical projection must use a reviewed neutral predicate")
        if any(
            dependency not in positions or positions[dependency] >= index
            for dependency in definition.dependencies
        ):
            raise ValueError("physical dependencies must exist before their consumer")
    derived_ids = tuple(definition.identifier for definition in every_derived)
    if len(set(derived_ids)) != len(derived_ids) or set(derived_ids) != set(DerivedId):
        raise ValueError("derived definitions must cover every derived id exactly once")
    derived_fields = [definition.result_field for definition in every_derived]
    if len(set(derived_fields)) != len(derived_fields) or set(fields) & set(derived_fields):
        raise ValueError("registry result fields must be unique")
    for discoverer in derived:
        if not discoverer.public_entrypoint:
            raise ValueError("derived definitions require a public entrypoint")
    for derived_definition in every_derived:
        if not derived_definition.record_types:
            raise ValueError("derived definitions require record contracts")
        if not isinstance(derived_definition.census, Counted | NotCounted):
            raise ValueError("derived definitions require an explicit census disposition")
        if (
            isinstance(derived_definition.census, NotCounted)
            and not derived_definition.census.reason
        ):
            raise ValueError("not-counted census reasons must be non-empty")
        if any(source not in positions for source in derived_definition.sources):
            raise ValueError("derived sources must be registered physical families")


def validate_result_fields(result_fields: frozenset[str]) -> None:
    """Validate registry coverage against independently declared internal detector fields."""

    every_derived: tuple[DerivedDefinition | ProjectionDefinition, ...] = (
        *DERIVED_DEFINITIONS,
        *PROJECTION_DEFINITIONS,
    )
    registered = {definition.result_field for definition in PHYSICAL_DEFINITIONS} | {
        definition.result_field for definition in every_derived
    }
    if registered != result_fields:
        raise ValueError("registry fields do not exactly cover physical and derived results")


def validate_output(
    definition: PhysicalDefinition | DerivedDefinition | ProjectionDefinition,
    records: list[object],
) -> None:
    """Reject an adapter output that violates its declared record contract."""

    if not all(isinstance(record, definition.record_types) for record in records):
        raise TypeError(f"{definition.result_field} discovery returned an undeclared record type")


def validate_census_contract(
    expected: Mapping[str, str],
    physical: tuple[PhysicalDefinition, ...] = PHYSICAL_DEFINITIONS,
    derived: tuple[DerivedDefinition, ...] = DERIVED_DEFINITIONS,
    projections: tuple[ProjectionDefinition, ...] = PROJECTION_DEFINITIONS,
) -> None:
    """Compare census key-to-source bindings with the independent manual census contract."""

    every_derived: tuple[DerivedDefinition | ProjectionDefinition, ...] = (*derived, *projections)
    actual = {
        definition.result_field: definition.census.key
        for definition in physical
        if isinstance(definition.census, Counted)
    } | {
        definition.result_field: definition.census.key
        for definition in every_derived
        if isinstance(definition.census, Counted)
    }
    if actual != dict(expected):
        raise ValueError("registry census bindings do not match the manual census contract")


validate_definitions(PHYSICAL_DEFINITIONS, DERIVED_DEFINITIONS, PROJECTION_DEFINITIONS)
