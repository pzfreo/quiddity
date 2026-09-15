# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Run-local candidate identity and defining evidence for epic 0003.

Public recognition records compare by value.  A candidate is different: it is one proposal made
during one run, and two equal records may have been established by different faces.  Candidates
therefore compare by identity and can only be issued through :class:`EvidenceSink`, which validates
every face node against the run's graph before candidate and evidence become visible together.

A failed predicate has no Candidate, so its bounded diagnostic evidence is a separate sink-issued
Observation rather than a fabricated proposal. The sink intentionally has no lookup API;
reconciliation receives an immutable index only after aggregate discovery has issued every
physical proposal and the issuer has been terminally sealed. Standalone compatibility paths may
still take non-closing point-in-time snapshots.
"""

from __future__ import annotations

from collections.abc import Callable, Iterable, Mapping, Sequence
from dataclasses import dataclass, field
from enum import Enum
from types import MappingProxyType
from typing import Generic, TypeVar, cast

from quiddity._adjacency import FaceGraph, FaceNode, SolidRef
from quiddity._effective_surfaces import (
    SurfaceKind,
    SurfaceProvenance,
    SurfaceUse,
    _validate_surface_use,
)
from quiddity._passage_compat import CompatibilitySnapshot, PassageCompatibilityView

RecordT = TypeVar("RecordT")


class FamilyId(Enum):
    """Closed identifiers for every physical aggregate family.

    ``LEGACY`` remains only for standalone compatibility through ``ClaimLedger.add_defining``.
    It is not a physical aggregate family and aggregate completeness rejects it.
    Adding a real member is an explicit integration change rather than an arbitrary string.
    """

    LEGACY = "legacy"
    ANGLED_STEPS = "angled_steps"
    BLENDS = "blends"
    BOSSES = "bosses"
    CHAMFERS = "chamfers"
    CHANNELS = "channels"
    CIRCULAR_BLIND_STEPS = "circular_blind_steps"
    COUNTERSINKS = "countersinks"
    DOUBLE_D_BORES = "double_d_bores"
    EDGE_OPEN_CIRCULAR_POCKETS = "edge_open_circular_pockets"
    EDGE_OPEN_PRISMATIC_RECESSES = "edge_open_prismatic_recesses"
    FILLETS = "fillets"
    FLATS = "flats"
    GROOVES = "grooves"
    GUSSET_RIBS = "gusset_ribs"
    HOLES = "holes"
    ORIENTED_SLOTS = "oriented_slots"
    PADS = "pads"
    PAIRED_RAMP_STEPS = "paired_ramp_steps"
    PASSAGES = "passages"
    PLATES = "plates"
    POCKETS = "pockets"
    POLYGONAL_BOSSES = "polygonal_bosses"
    POLYGONAL_STOCK = "polygonal_stock"
    PRISMATIC_POCKETS = "prismatic_pockets"
    REPEATING_RADIAL_PROFILES = "repeating_radial_profiles"
    RECTANGULAR_BLIND_SLOTS = "rectangular_blind_slots"
    ROUND_BOTTOM_BLIND_SLOTS = "round_bottom_blind_slots"
    SECTION_RECESSES = "section_recesses"
    RISERS = "risers"
    SLOTS = "slots"
    STEP_LEVELS = "step_levels"
    THROUGH_STEPS = "through_steps"
    TURNED_STEPS = "turned_steps"


class DerivedId(Enum):
    """Closed identifiers for post-reconciliation, non-physical projections."""

    HOLE_PATTERNS = "hole_patterns"
    SLOT_PATTERNS = "slot_patterns"
    ORIENTED_SLOT_PATTERNS = "oriented_slot_patterns"
    POCKET_PATTERNS = "pocket_patterns"
    GUSSET_RIB_PATTERNS = "gusset_rib_patterns"
    PASSAGES_COMPAT = "passages_compat"


@dataclass(frozen=True, slots=True, init=False)
class Evidence:
    """Original defining/constituent nodes and effective-surface acceptance dependencies."""

    defining: frozenset[FaceNode]
    constituent: frozenset[FaceNode]
    surfaces: tuple[SurfaceUse, ...] = ()

    def __init__(
        self,
        defining: frozenset[FaceNode],
        surfaces: tuple[SurfaceUse, ...] = (),
        *,
        constituent: frozenset[FaceNode] | None = None,
    ) -> None:
        # Omission is the fail-closed migration state: a family publishes no wider membership
        # than the ownership evidence it already proves. Frozen snapshots carry no three-state
        # contract, and the default preserves the defining set's exact identity.
        object.__setattr__(self, "defining", defining)
        object.__setattr__(self, "constituent", defining if constituent is None else constituent)
        object.__setattr__(self, "surfaces", surfaces)


class PredicateId(Enum):
    """Closed failed-predicate observations with demonstrated diagnostic consumers."""

    ANGLED_STEP_TERMINAL = "angled_step_terminal"


def _record_candidate(
    by_record: Mapping[int, Sequence[Candidate[object]]], record: object
) -> Candidate[object] | None:
    """Resolve the private record adapter only when proposal identity is unambiguous."""

    candidates = by_record.get(id(record), ())
    if len(candidates) > 1:
        raise ValueError(f"{record!r} has multiple candidates; use candidate identity")
    return candidates[0] if candidates else None


@dataclass(frozen=True, slots=True)
class SplitTriangularTerminalFact:
    """A linear outer boundary split topologically but still having three geometric sides."""

    raw_outer_edges: int
    effective_outer_sides: int = 3

    def __post_init__(self) -> None:
        if self.raw_outer_edges <= 3 or self.effective_outer_sides != 3:
            raise ValueError("a split triangular terminal requires raw > 3 and effective == 3")


@dataclass(frozen=True, eq=False, init=False, slots=True)
class Observation:
    """One sink-issued failed predicate attempt; never a physical proposal."""

    family: FamilyId
    predicate: PredicateId
    subject: FaceNode
    consulted: frozenset[FaceNode]
    fact: SplitTriangularTerminalFact
    _issuer: object = field(repr=False)


@dataclass(frozen=True, eq=False, init=False, slots=True)
class Candidate(Generic[RecordT]):
    """One sink-issued proposal, compared by run-local identity."""

    family: FamilyId
    record: RecordT
    evidence: Evidence
    compatibility: PassageCompatibilityView | None
    _issuer: object = field(repr=False)


@dataclass(frozen=True, init=False, slots=True)
class CandidateSet(Generic[RecordT]):
    """The source-ordered candidates issued for one family."""

    family: FamilyId
    candidates: tuple[Candidate[RecordT], ...]
    _issuer: object = field(repr=False)


class CompletedOccurrence:
    """Opaque, issuer-validated provenance for one completed physical occurrence."""

    __slots__ = ("__issuer",)

    def __init__(self) -> None:
        raise TypeError("completed occurrences are issuer-created")

    def record(self, record_type: type[RecordT]) -> RecordT:
        """Return the exact completed record occurrence after issuer validation."""

        return _completed_record(self, record_type)

    def defining(self) -> tuple[FaceNode, ...]:
        """Return graph-ordered defining nodes after issuer validation."""

        return _completed_defining(self)

    def solid(self) -> SolidRef | None:
        """Return recomputed common-solid authority, or ``None`` for empty evidence."""

        return _completed_solid(self)


class CompletedInputs:
    """Opaque provenance restricted to one definition's declared predecessors."""

    __slots__ = ("__issuer",)

    def __init__(self) -> None:
        raise TypeError("completed inputs are issuer-created")

    def records(self, family: FamilyId, record_type: type[RecordT]) -> tuple[RecordT, ...]:
        """Return exact predecessor records for one declared completed family."""

        return tuple(
            occurrence.record(record_type) for occurrence in _restricted_occurrences(self, family)
        )

    def occurrences(
        self, family: FamilyId, record_type: type[RecordT]
    ) -> tuple[CompletedOccurrence, ...]:
        """Return validated occurrence handles for one declared predecessor."""

        occurrences = _restricted_occurrences(self, family)
        for occurrence in occurrences:
            occurrence.record(record_type)
        return occurrences


def _occurrence_issuer(occurrence: CompletedOccurrence) -> _CandidateIssuer:
    issuer = getattr(occurrence, "_CompletedOccurrence__issuer", None)
    if issuer is None:
        raise ValueError("completed occurrence issuer was mutated")
    return cast("_CandidateIssuer", issuer)


def _inputs_issuer(inputs: CompletedInputs) -> _CandidateIssuer:
    issuer = getattr(inputs, "_CompletedInputs__issuer", None)
    if issuer is None:
        raise ValueError("completed-input issuer was mutated")
    return cast("_CandidateIssuer", issuer)


def _completed_record(occurrence: CompletedOccurrence, record_type: type[RecordT]) -> RecordT:
    return _occurrence_issuer(occurrence).completed_record(occurrence, record_type)


def _completed_defining(occurrence: CompletedOccurrence) -> tuple[FaceNode, ...]:
    return _occurrence_issuer(occurrence).completed_defining(occurrence)


def _completed_solid(occurrence: CompletedOccurrence) -> SolidRef | None:
    return _occurrence_issuer(occurrence).completed_solid(occurrence)


def _restricted_occurrences(
    inputs: CompletedInputs, family: FamilyId
) -> tuple[CompletedOccurrence, ...]:
    return _inputs_issuer(inputs).restricted_occurrences(inputs, family)


class EvidenceSink:
    """Write-only candidate/evidence capability handed to discovery."""

    __slots__ = ("__issuer",)

    def __init__(self, issuer: _CandidateIssuer) -> None:
        self.__issuer = issuer

    def propose(
        self,
        family: FamilyId,
        record: RecordT,
        *,
        defining: Iterable[FaceNode] = (),
        constituent: Iterable[FaceNode] | None = None,
        surfaces: Iterable[SurfaceUse] = (),
        compatibility: PassageCompatibilityView | None = None,
    ) -> Candidate[RecordT]:
        """Atomically validate evidence and issue one identity-safe candidate."""

        return self.__issuer.propose(
            family,
            record,
            defining=defining,
            constituent=constituent,
            surfaces=surfaces,
            compatibility=compatibility,
        )

    def observe(
        self,
        family: FamilyId,
        predicate: PredicateId,
        *,
        subject: FaceNode,
        consulted: Iterable[FaceNode],
        fact: SplitTriangularTerminalFact,
    ) -> Observation:
        """Atomically issue one failed-predicate observation."""

        return self.__issuer.observe(
            family,
            predicate,
            subject=subject,
            consulted=consulted,
            fact=fact,
        )


@dataclass(frozen=True, init=False, slots=True)
class EvidenceIndex:
    """Immutable point-in-time evidence issued by one recognition run.

    This is a copy, not a live read-only facade. A standalone compatibility caller may snapshot
    an issued prefix and continue writing; those later proposals cannot appear here. Aggregate
    orchestration instead obtains the same read type from its one terminal freeze, after every
    physical proposal has been bound, then validates exact inventory coverage before any rule
    reads it. The index exposes neither creation mode nor a route back to the issuer.
    """

    _graph: FaceGraph = field(repr=False)
    _token: object = field(repr=False)
    _candidates: tuple[Candidate[object], ...] = field(repr=False)
    _issued: Mapping[int, _IssuedCandidate] = field(repr=False)
    _by_record: Mapping[int, tuple[Candidate[object], ...]] = field(repr=False)
    _by_node: Mapping[FaceNode, tuple[Candidate[object], ...]] = field(repr=False)
    _by_constituent: Mapping[FaceNode, tuple[Candidate[object], ...]] = field(repr=False)
    _observations: tuple[Observation, ...] = field(repr=False)
    _issued_observations: Mapping[int, _IssuedObservation] = field(repr=False)

    def _validate_graph(self, graph: FaceGraph) -> None:
        """Prove this terminal index belongs to exactly *graph* without exposing its token."""

        if self._graph is not graph or self._graph.run_token is not graph.run_token:
            raise ValueError("evidence index belongs to another graph run")

    def candidate_set(self, family: FamilyId) -> CandidateSet[object]:
        """Return the source-ordered candidates for *family* in this snapshot."""

        candidates = tuple(
            issued.candidate
            for candidate in self._candidates
            if (issued := self._validate(candidate)).family is family
        )
        result = object.__new__(CandidateSet)
        object.__setattr__(result, "family", family)
        object.__setattr__(result, "candidates", candidates)
        object.__setattr__(result, "_issuer", self._token)
        return result

    def candidate_set_for(
        self, family: FamilyId, records: Iterable[object]
    ) -> CandidateSet[object]:
        """Select accepted occurrences from this family's frozen candidates by identity."""

        available = list(self.candidate_set(family).candidates)
        selected: list[Candidate[object]] = []
        used: set[int] = set()
        for record in records:
            candidate = next(
                (
                    item
                    for item in available
                    if id(item) not in used and self._validate(item).record is record
                ),
                None,
            )
            if candidate is None:
                raise ValueError("accepted record has no candidate in this evidence snapshot")
            used.add(id(candidate))
            selected.append(candidate)
        result = object.__new__(CandidateSet)
        object.__setattr__(result, "family", family)
        object.__setattr__(result, "candidates", tuple(selected))
        object.__setattr__(result, "_issuer", self._token)
        return result

    def validate_complete_inventory(self, candidate_sets: tuple[CandidateSet[object], ...]) -> None:
        """Prove one same-run, non-legacy inventory covers every frozen candidate once."""

        seen: set[int] = set()
        for candidate_set in candidate_sets:
            if candidate_set._issuer is not self._token:
                raise ValueError("candidate set belongs to another evidence issuer")
            if candidate_set.family is FamilyId.LEGACY:
                raise ValueError("legacy candidates are not aggregate physical proposals")
            for candidate in candidate_set.candidates:
                issued = self._validate(candidate)
                if issued.family is not candidate_set.family:
                    raise ValueError("candidate family does not match its candidate set")
                if id(candidate) in seen:
                    raise ValueError("candidate occurs more than once in physical inventory")
                seen.add(id(candidate))
        expected = {id(candidate) for candidate in self._candidates}
        if seen != expected:
            raise ValueError("physical inventory does not exactly cover frozen candidates")

    def validate_candidate_set(self, candidate_set: CandidateSet[object]) -> None:
        """Validate one same-run subset against issuer-owned frozen snapshots."""

        if candidate_set._issuer is not self._token:
            raise ValueError("candidate set belongs to another evidence issuer")
        for candidate in candidate_set.candidates:
            issued = self._validate(candidate)
            if issued.family is not candidate_set.family:
                raise ValueError("candidate family does not match its candidate set")

    def defining_of(self, subject: object) -> frozenset[FaceNode]:
        """Return defining evidence by candidate identity.

        Record-object lookup is a private migration adapter. It fails closed when one record
        object backs multiple proposals; only Candidate identity is unambiguous in that case.
        """

        if isinstance(subject, Candidate):
            return self._validate(subject).defining
        candidate = _record_candidate(self._by_record, subject)
        return self._validate(candidate).defining if candidate is not None else frozenset()

    def constituent_of(self, subject: object) -> frozenset[FaceNode]:
        """Return physical membership without changing defining ownership or reconciliation."""

        if isinstance(subject, Candidate):
            return self._validate(subject).constituent
        candidate = _record_candidate(self._by_record, subject)
        return self._validate(candidate).constituent if candidate is not None else frozenset()

    def memberships_of(self, node: FaceNode) -> tuple[Candidate[object], ...]:
        """Return every candidate naming *node* as constituent, in proposal order."""

        if not self._graph.owns(node):
            raise ValueError(f"{node!r} is not this graph's node")
        candidates = self._by_constituent.get(node, ())
        for candidate in candidates:
            self._validate(candidate)
        return candidates

    def observations(self, family: FamilyId, predicate: PredicateId) -> tuple[Observation, ...]:
        """Return validated failed attempts in issuance order."""

        issued = tuple(self._validate_observation(item) for item in self._observations)
        return tuple(
            item.observation
            for item in issued
            if item.family is family and item.predicate is predicate
        )

    def claims_of(self, node: FaceNode) -> tuple[Candidate[object], ...]:
        """Return candidates naming *node*, in proposal order."""

        if not self._graph.owns(node):
            raise ValueError(f"{node!r} is not this graph's node")
        candidates = self._by_node.get(node, ())
        for candidate in candidates:
            self._validate(candidate)
        return candidates

    def passage_compatibility(self, candidate: Candidate[object]) -> PassageCompatibilityView:
        """Return one validated issuer-frozen Passage compatibility fact."""

        issued = self._validate(candidate)
        if issued.family is not FamilyId.PASSAGES or not isinstance(
            issued.compatibility, PassageCompatibilityView
        ):
            raise ValueError("candidate has no Passage compatibility authority")
        return issued.compatibility

    def _validate(self, candidate: Candidate[object]) -> _IssuedCandidate:
        issued = self._issued.get(id(candidate))
        if issued is None or issued.candidate is not candidate:
            raise ValueError("candidate is not present in this evidence snapshot")
        if (
            candidate._issuer is not self._token
            or candidate.family is not issued.family
            or candidate.record is not issued.record
            or candidate.evidence is not issued.evidence
            or candidate.evidence.defining is not issued.defining
            or candidate.evidence.constituent is not issued.constituent
            or candidate.evidence.surfaces is not issued.surfaces
            or candidate.compatibility is not issued.compatibility
            or (
                issued.compatibility is not None
                and issued.compatibility.issued_snapshot() != issued.compatibility_snapshot
            )
        ):
            raise ValueError("candidate no longer matches its issued state")
        for surface_use in issued.surfaces:
            _validate_surface_use(surface_use, self._graph)
        return issued

    def _validate_observation(self, observation: Observation) -> _IssuedObservation:
        issued = self._issued_observations.get(id(observation))
        if issued is None or issued.observation is not observation:
            raise ValueError("observation is not present in this evidence snapshot")
        if (
            observation._issuer is not self._token
            or observation.family is not issued.family
            or observation.predicate is not issued.predicate
            or observation.subject is not issued.subject
            or observation.consulted is not issued.consulted
            or observation.fact is not issued.fact
            or observation.fact.raw_outer_edges != issued.raw_outer_edges
            or observation.fact.effective_outer_sides != issued.effective_outer_sides
        ):
            raise ValueError("observation no longer matches its issued state")
        return issued


@dataclass(frozen=True, slots=True)
class _IssuedCandidate:
    """Issuer-owned snapshot against which the outward Candidate is verified."""

    candidate: Candidate[object]
    family: FamilyId
    record: object
    evidence: Evidence
    defining: frozenset[FaceNode]
    constituent: frozenset[FaceNode]
    surfaces: tuple[SurfaceUse, ...]
    compatibility: PassageCompatibilityView | None
    compatibility_snapshot: CompatibilitySnapshot | None


@dataclass(frozen=True, slots=True)
class _CompletedOccurrenceSnapshot:
    handle: CompletedOccurrence
    candidate: Candidate[object]
    family: FamilyId
    record: object
    defining: tuple[FaceNode, ...]
    solid: SolidRef | None


@dataclass(frozen=True, slots=True)
class _RestrictedInputsSnapshot:
    inputs: CompletedInputs
    definition: object
    consumer: FamilyId
    allowed: tuple[FamilyId, ...]
    occurrences: Mapping[FamilyId, tuple[CompletedOccurrence, ...]]


@dataclass(frozen=True, slots=True)
class _IssuedObservation:
    observation: Observation
    family: FamilyId
    predicate: PredicateId
    subject: FaceNode
    consulted: frozenset[FaceNode]
    fact: SplitTriangularTerminalFact
    raw_outer_edges: int
    effective_outer_sides: int


class _CandidateIssuer:
    """Mutable run store owned by the legacy ledger until its read/write views split."""

    def __init__(
        self,
        graph: FaceGraph,
        *,
        on_issued: Callable[[Candidate[object]], None] | None = None,
        definitions: Sequence[object] = (),
    ) -> None:
        self._graph = graph
        self._token = object()
        self._candidates: list[Candidate[object]] = []
        self._issued: dict[int, _IssuedCandidate] = {}
        self._by_record: dict[int, list[Candidate[object]]] = {}
        self._by_node: dict[FaceNode, list[Candidate[object]]] = {}
        self._by_constituent: dict[FaceNode, list[Candidate[object]]] = {}
        self._observations: list[Observation] = []
        self._issued_observations: dict[int, _IssuedObservation] = {}
        self._completed: dict[FamilyId, CandidateSet[object]] = {}
        self._completed_occurrences: dict[FamilyId, tuple[CompletedOccurrence, ...]] = {}
        self._occurrence_snapshots: dict[int, _CompletedOccurrenceSnapshot] = {}
        self._restricted_snapshots: dict[int, _RestrictedInputsSnapshot] = {}
        self._definitions = tuple(definitions)
        self._on_issued = on_issued
        self._sealed = False
        self.sink = EvidenceSink(self)

    def propose(
        self,
        family: FamilyId,
        record: RecordT,
        *,
        defining: Iterable[FaceNode],
        constituent: Iterable[FaceNode] | None = None,
        surfaces: Iterable[SurfaceUse] = (),
        compatibility: PassageCompatibilityView | None = None,
    ) -> Candidate[RecordT]:
        if self._sealed:
            raise RuntimeError("evidence issuance is sealed")
        if family in self._completed:
            raise RuntimeError(f"{family.value} candidate issuance is already completed")
        nodes = frozenset(defining)
        members = nodes if constituent is None else frozenset(constituent)
        surface_uses = tuple(surfaces)
        foreign = [node for node in members | nodes if not self._graph.owns(node)]
        if foreign:
            raise ValueError(f"{sorted(node.index for node in foreign)} are not this graph's nodes")
        if not nodes <= members:
            raise ValueError("defining evidence must be a subset of constituent evidence")
        if (
            family is not FamilyId.LEGACY
            and members
            and self._graph.common_valid_solid(members) is None
        ):
            raise ValueError("physical evidence must belong to one valid closed solid")
        if surface_uses:
            migrated_surface_families = (
                FamilyId.PADS,
                FamilyId.HOLES,
                FamilyId.BOSSES,
                FamilyId.CIRCULAR_BLIND_STEPS,
            )
            if family not in migrated_surface_families:
                raise ValueError("only explicitly migrated families may carry surface evidence")
            snapshots = tuple(_validate_surface_use(use, self._graph) for use in surface_uses)
            surface_nodes = tuple(snapshot.node for snapshot in snapshots)
            if len(surface_nodes) != len(set(surface_nodes)):
                raise ValueError("surface evidence repeats an original face")
            if frozenset(surface_nodes) != nodes:
                raise ValueError("surface evidence must cover every defining face exactly once")
            material = tuple(
                snapshot.material_side
                for snapshot in snapshots
                if snapshot.material_side is not None
            )
            if family is FamilyId.PADS and len(material) != 1:
                raise ValueError("Pad evidence requires exactly one material-side certificate")
            if family in (FamilyId.HOLES, FamilyId.BOSSES):
                expected_sign = -1 if family is FamilyId.HOLES else 1
                recovered = tuple(
                    snapshot
                    for snapshot in snapshots
                    if snapshot.surface.provenance is SurfaceProvenance.RECOVERED
                )
                if (
                    any(snapshot.surface.kind is not SurfaceKind.CYLINDER for snapshot in snapshots)
                    or any(snapshot.material_side is None for snapshot in recovered)
                    or any(
                        certificate.candidate_outward_sign != expected_sign
                        for certificate in material
                    )
                ):
                    raise ValueError(
                        f"{family.value} cylinder evidence has incompatible material side"
                    )
        elif family in (FamilyId.PADS, FamilyId.HOLES, FamilyId.BOSSES) and nodes:
            raise ValueError(f"{family.value} candidates require effective-surface evidence")
        if family is FamilyId.PASSAGES:
            if not isinstance(compatibility, PassageCompatibilityView):
                raise ValueError("passage candidates require a compatibility fact")
        elif compatibility is not None:
            raise ValueError("only passage candidates may carry compatibility facts")
        candidate = object.__new__(Candidate)
        object.__setattr__(candidate, "family", family)
        object.__setattr__(candidate, "record", record)
        object.__setattr__(
            candidate,
            "evidence",
            Evidence(nodes, constituent=members, surfaces=surface_uses),
        )
        object.__setattr__(candidate, "compatibility", compatibility)
        object.__setattr__(candidate, "_issuer", self._token)
        self._candidates.append(candidate)
        self._issued[id(candidate)] = _IssuedCandidate(
            candidate,
            family,
            record,
            candidate.evidence,
            candidate.evidence.defining,
            candidate.evidence.constituent,
            candidate.evidence.surfaces,
            compatibility,
            compatibility.issued_snapshot() if compatibility is not None else None,
        )
        self._by_record.setdefault(id(record), []).append(candidate)
        for node in nodes:
            self._by_node.setdefault(node, []).append(candidate)
        for node in members:
            self._by_constituent.setdefault(node, []).append(candidate)
        if self._on_issued is not None:
            self._on_issued(candidate)
        return candidate

    def observe(
        self,
        family: FamilyId,
        predicate: PredicateId,
        *,
        subject: FaceNode,
        consulted: Iterable[FaceNode],
        fact: SplitTriangularTerminalFact,
    ) -> Observation:
        if self._sealed:
            raise RuntimeError("evidence issuance is sealed")
        if family in self._completed:
            raise RuntimeError(f"{family.value} observation issuance is already completed")
        if not isinstance(family, FamilyId) or not isinstance(predicate, PredicateId):
            raise ValueError("observation family and predicate must use closed enums")
        if not isinstance(fact, SplitTriangularTerminalFact):
            raise ValueError("observation fact must use the closed predicate fact type")
        context = frozenset(consulted)
        if family is not FamilyId.ANGLED_STEPS or predicate is not PredicateId.ANGLED_STEP_TERMINAL:
            raise ValueError("unsupported observation family/predicate combination")
        if len(context) != 1:
            raise ValueError("an angled-step terminal observation requires exactly one terminal")
        if subject in context:
            raise ValueError("observation subject and consulted context must be disjoint")
        foreign = [node for node in context | {subject} if not self._graph.owns(node)]
        if foreign:
            raise ValueError(f"{sorted(node.index for node in foreign)} are not this graph's nodes")
        observation = object.__new__(Observation)
        object.__setattr__(observation, "family", family)
        object.__setattr__(observation, "predicate", predicate)
        object.__setattr__(observation, "subject", subject)
        object.__setattr__(observation, "consulted", context)
        object.__setattr__(observation, "fact", fact)
        object.__setattr__(observation, "_issuer", self._token)
        issued = _IssuedObservation(
            observation,
            family,
            predicate,
            subject,
            context,
            fact,
            fact.raw_outer_edges,
            fact.effective_outer_sides,
        )
        self._observations.append(observation)
        self._issued_observations[id(observation)] = issued
        return observation

    @property
    def candidates(self) -> tuple[Candidate[object], ...]:
        for candidate in self._candidates:
            self._validate(candidate)
        for observation in self._observations:
            self._validate_observation(observation)
        return tuple(self._candidates)

    def candidate_set(self, family: FamilyId) -> CandidateSet[object]:
        candidates = tuple(
            issued.candidate
            for candidate in self._candidates
            if (issued := self._validate(candidate)).family is family
        )
        return self._make_candidate_set(family, candidates)

    def candidate_set_for(
        self, family: FamilyId, records: Iterable[object]
    ) -> CandidateSet[object]:
        """Atomically complete *family* against its exact returned occurrences.

        Completion validates the whole family before publishing deliberate empty Candidates,
        occurrence handles, or the completed-family marker.  The resulting CandidateSet is the
        one terminal inventory source and completion permanently closes later family issuance.
        """

        if family in self._completed:
            raise RuntimeError(f"{family.value} is already completed")
        ordered_records = tuple(records)
        available: dict[int, list[Candidate[object]]] = {
            key: list(value) for key, value in self._by_record.items()
        }
        used: set[int] = set()
        ordered: list[Candidate[object]] = []
        planned: list[tuple[Candidate[object], _IssuedCandidate]] = []
        for record in ordered_records:
            candidates = available.get(id(record), [])
            wrong = [
                candidate
                for candidate in candidates
                if id(candidate) not in used and self._validate(candidate).family is not family
            ]
            if wrong:
                raise ValueError(f"record was issued under {self._validate(wrong[0]).family.value}")
            candidate = next(
                (
                    candidate
                    for candidate in candidates
                    if id(candidate) not in used and self._validate(candidate).family is family
                ),
                None,
            )
            if candidate is None:
                candidate = object.__new__(Candidate)
                evidence = Evidence(frozenset())
                object.__setattr__(candidate, "family", family)
                object.__setattr__(candidate, "record", record)
                object.__setattr__(candidate, "evidence", evidence)
                object.__setattr__(candidate, "compatibility", None)
                object.__setattr__(candidate, "_issuer", self._token)
                planned.append(
                    (
                        candidate,
                        _IssuedCandidate(
                            candidate,
                            family,
                            record,
                            evidence,
                            evidence.defining,
                            evidence.constituent,
                            evidence.surfaces,
                            None,
                            None,
                        ),
                    )
                )
                available.setdefault(id(record), []).append(candidate)
            used.add(id(candidate))
            ordered.append(candidate)

        issued_for_family = {
            id(candidate)
            for candidate in self._candidates
            if self._validate(candidate).family is family
        }
        planned_ids = {id(candidate) for candidate, _issued in planned}
        if issued_for_family != used - planned_ids:
            raise ValueError("family issued candidates absent from its returned inventory")

        planned_by_id = {id(candidate): issued for candidate, issued in planned}
        candidate_set = self._make_candidate_set(family, tuple(ordered))
        handles: list[CompletedOccurrence] = []
        snapshots: list[_CompletedOccurrenceSnapshot] = []
        for candidate in ordered:
            issued = planned_by_id.get(id(candidate)) or self._validate(candidate)
            defining = tuple(node for node in self._graph.nodes if node in issued.defining)
            solid = self._graph.common_valid_solid(defining) if defining else None
            handle = object.__new__(CompletedOccurrence)
            object.__setattr__(handle, "_CompletedOccurrence__issuer", self)
            handles.append(handle)
            snapshots.append(
                _CompletedOccurrenceSnapshot(
                    handle, candidate, family, issued.record, defining, solid
                )
            )

        # Phase two contains only installation of the fully validated staged snapshot.
        for candidate, issued in planned:
            self._candidates.append(candidate)
            self._issued[id(candidate)] = issued
            self._by_record.setdefault(id(issued.record), []).append(candidate)
        self._completed[family] = candidate_set
        self._completed_occurrences[family] = tuple(handles)
        self._occurrence_snapshots.update({id(snapshot.handle): snapshot for snapshot in snapshots})
        return candidate_set

    def restricted_inputs(self, definition: object) -> CompletedInputs:
        """Create one opaque input view for an exact declared predecessor roster."""

        if not any(definition is registered for registered in self._definitions):
            raise ValueError("completed inputs require an exact registered definition authority")
        consumer = getattr(definition, "family", None)
        allowed = getattr(definition, "dependencies", None)
        if (
            not isinstance(consumer, FamilyId)
            or not isinstance(allowed, tuple)
            or not all(isinstance(family, FamilyId) for family in allowed)
        ):
            raise TypeError("completed inputs require one physical definition authority")
        if consumer in allowed or len(allowed) != len(set(allowed)):
            raise ValueError("completed-input dependency roster is invalid")
        missing = tuple(family for family in allowed if family not in self._completed)
        if missing:
            raise ValueError(
                "declared physical dependency has not completed: "
                + ", ".join(family.value for family in missing)
            )
        inputs = object.__new__(CompletedInputs)
        object.__setattr__(inputs, "_CompletedInputs__issuer", self)
        occurrences = MappingProxyType(
            {family: self._completed_occurrences[family] for family in allowed}
        )
        self._restricted_snapshots[id(inputs)] = _RestrictedInputsSnapshot(
            inputs, definition, consumer, allowed, occurrences
        )
        return inputs

    def restricted_occurrences(
        self, inputs: CompletedInputs, family: FamilyId
    ) -> tuple[CompletedOccurrence, ...]:
        snapshot = self._validate_restricted(inputs)
        if family not in snapshot.allowed:
            raise ValueError(f"{family.value} is not a declared physical dependency")
        occurrences = snapshot.occurrences[family]
        if occurrences is not self._completed_occurrences.get(family):
            raise ValueError("completed predecessor occurrence snapshot is stale")
        for occurrence in occurrences:
            self._validate_completed(occurrence)
        return occurrences

    def completed_record(
        self, occurrence: CompletedOccurrence, record_type: type[RecordT]
    ) -> RecordT:
        snapshot = self._validate_completed(occurrence)
        if not isinstance(snapshot.record, record_type):
            raise TypeError(f"{snapshot.family.value} dependency has the wrong record type")
        return snapshot.record

    def completed_defining(self, occurrence: CompletedOccurrence) -> tuple[FaceNode, ...]:
        return self._validate_completed(occurrence).defining

    def completed_solid(self, occurrence: CompletedOccurrence) -> SolidRef | None:
        return self._validate_completed(occurrence).solid

    def _validate_completed(self, occurrence: CompletedOccurrence) -> _CompletedOccurrenceSnapshot:
        snapshot = self._occurrence_snapshots.get(id(occurrence))
        if snapshot is None or snapshot.handle is not occurrence:
            raise ValueError("completed occurrence was not issued by this run")
        if getattr(occurrence, "_CompletedOccurrence__issuer", None) is not self:
            raise ValueError("completed occurrence issuer was mutated")
        issued = self._validate(snapshot.candidate)
        defining = tuple(node for node in self._graph.nodes if node in issued.defining)
        solid = self._graph.common_valid_solid(defining) if defining else None
        if (
            issued.family is not snapshot.family
            or issued.record is not snapshot.record
            or defining != snapshot.defining
            or solid != snapshot.solid
        ):
            raise ValueError("completed occurrence provenance was mutated")
        return snapshot

    def _validate_restricted(self, inputs: CompletedInputs) -> _RestrictedInputsSnapshot:
        snapshot = self._restricted_snapshots.get(id(inputs))
        if snapshot is None or snapshot.inputs is not inputs:
            raise ValueError("completed inputs were not issued by this run")
        if getattr(inputs, "_CompletedInputs__issuer", None) is not self:
            raise ValueError("completed-input issuer was mutated")
        if (
            getattr(snapshot.definition, "family", None) is not snapshot.consumer
            or getattr(snapshot.definition, "dependencies", None) != snapshot.allowed
        ):
            raise ValueError("completed-input definition authority was mutated")
        if any(family not in self._completed for family in snapshot.allowed):
            raise ValueError("completed-input predecessor state is stale")
        return snapshot

    def snapshot_index(self) -> EvidenceIndex:
        """Copy the currently issued prefix into an immutable read capability."""

        for candidate in self._candidates:
            self._validate(candidate)
        for observation in self._observations:
            self._validate_observation(observation)
        result = object.__new__(EvidenceIndex)
        object.__setattr__(result, "_graph", self._graph)
        object.__setattr__(result, "_token", self._token)
        object.__setattr__(result, "_candidates", tuple(self._candidates))
        object.__setattr__(result, "_issued", MappingProxyType(dict(self._issued)))
        object.__setattr__(
            result,
            "_by_record",
            MappingProxyType({key: tuple(value) for key, value in self._by_record.items()}),
        )
        object.__setattr__(
            result,
            "_by_node",
            MappingProxyType({key: tuple(value) for key, value in self._by_node.items()}),
        )
        object.__setattr__(
            result,
            "_by_constituent",
            MappingProxyType({key: tuple(value) for key, value in self._by_constituent.items()}),
        )
        object.__setattr__(result, "_observations", tuple(self._observations))
        object.__setattr__(
            result,
            "_issued_observations",
            MappingProxyType(dict(self._issued_observations)),
        )
        return result

    def freeze_index(self) -> EvidenceIndex:
        """Seal aggregate issuance once and return its complete immutable evidence index."""

        if self._sealed:
            raise RuntimeError("candidate issuance is already sealed")
        index = self.snapshot_index()
        self._sealed = True
        return index

    def _make_candidate_set(
        self, family: FamilyId, candidates: tuple[Candidate[object], ...]
    ) -> CandidateSet[object]:
        result = object.__new__(CandidateSet)
        object.__setattr__(result, "family", family)
        object.__setattr__(result, "candidates", candidates)
        object.__setattr__(result, "_issuer", self._token)
        return result

    def defining_of(self, subject: object) -> frozenset[FaceNode]:
        if isinstance(subject, Candidate):
            return self._validate(subject).evidence.defining
        candidate = _record_candidate(self._by_record, subject)
        return self._validate(candidate).evidence.defining if candidate is not None else frozenset()

    def candidates_of(self, node: FaceNode) -> tuple[Candidate[object], ...]:
        if not self._graph.owns(node):
            raise ValueError(f"{node!r} is not this graph's node")
        candidates = tuple(self._by_node.get(node, ()))
        for candidate in candidates:
            self._validate(candidate)
        return candidates

    def _validate(self, candidate: Candidate[object]) -> _IssuedCandidate:
        issued = self._issued.get(id(candidate))
        if issued is None or issued.candidate is not candidate:
            raise ValueError("candidate was not issued by this run")
        if (
            candidate._issuer is not self._token
            or candidate.family is not issued.family
            or candidate.record is not issued.record
            or candidate.evidence is not issued.evidence
            or candidate.evidence.defining is not issued.defining
            or candidate.evidence.constituent is not issued.constituent
        ):
            raise ValueError("candidate no longer matches its issued state")
        return issued

    def _validate_observation(self, observation: Observation) -> _IssuedObservation:
        issued = self._issued_observations.get(id(observation))
        if issued is None or issued.observation is not observation:
            raise ValueError("observation was not issued by this run")
        if (
            observation._issuer is not self._token
            or observation.family is not issued.family
            or observation.predicate is not issued.predicate
            or observation.subject is not issued.subject
            or observation.consulted is not issued.consulted
            or observation.fact is not issued.fact
            or observation.fact.raw_outer_edges != issued.raw_outer_edges
            or observation.fact.effective_outer_sides != issued.effective_outer_sides
        ):
            raise ValueError("observation no longer matches its issued state")
        return issued
