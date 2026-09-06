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
from dataclasses import dataclass
from enum import Enum
from types import MappingProxyType
from typing import Protocol, TypeAlias, TypeVar, cast

from quiddity._candidates import (
    Candidate,
    CandidateSet,
    CompletedInputs,
    EvidenceIndex,
    FamilyId,
)
from quiddity._claims import EvidenceWriter
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
from quiddity._recess_features import (
    _discover_channels,
    _discover_pockets,
    _discover_slots,
)
from quiddity._run import RecognitionContext
from quiddity._section_recess import SectionRecess
from quiddity._section_recess_discovery import discover_section_recesses
from quiddity._typing import CylinderInventory
from quiddity.angled_steps import AngledStep, recognise_angled_steps
from quiddity.blends import Blend, _discover_blends
from quiddity.chamfers import Chamfer, recognise_chamfers
from quiddity.circular_blind_steps import (
    CircularBlindStep,
    _discover_circular_blind_steps,
)
from quiddity.countersinks import CounterSink, _discover_countersinks
from quiddity.edge_open_circular_recesses import (
    EdgeOpenCircularPocket,
    recognise_edge_open_circular_pockets,
)
from quiddity.edge_open_prismatic_recesses import (
    EdgeOpenPrismaticRecess,
    recognise_edge_open_prismatic_recesses,
)
from quiddity.fillets import Fillet, _discover_fillets
from quiddity.flats import Flat, _discover_flats
from quiddity.grooves import Groove, recognise_grooves
from quiddity.levels import (
    FaceLevel,
    RiserEvidence,
    _discover_risers,
    _discover_step_levels,
)
from quiddity.oriented_slots import (
    OrientedSlot,
    OrientedSlotArray,
    OrientedSlotGrid,
    recognise_oriented_slot_patterns,
)
from quiddity.oriented_slots import (
    _body_keys as _oriented_slot_body_keys,
)
from quiddity.oriented_slots import (
    _project as _project_oriented_slot,
)
from quiddity.pads import RaisedPad, _discover_rectangular_pads
from quiddity.paired_ramp_steps import PairedRampStep, recognise_paired_ramp_steps
from quiddity.passages import (
    Passage,
    SectionPassage,
    recognise_section_passages,
)
from quiddity.plates import Plate, _discover_plates
from quiddity.polygonal_bosses import (
    PolygonalBoss,
    PolygonalStock,
    _discover_polygonal_bosses,
    _discover_polygonal_stock,
)
from quiddity.prismatic_pockets import PrismaticPocket, recognise_prismatic_pockets
from quiddity.profiled_bores import DoubleDBore, _discover_double_d_bores
from quiddity.rectangular_blind_slots import (
    RectangularBlindSlot,
    recognise_rectangular_blind_slots,
)
from quiddity.repeating_profiles import (
    RepeatingRadialProfile,
    _discover_repeating_radial_profiles,
)
from quiddity.round_bottom_slots import (
    RoundBottomBlindSlot,
    recognise_round_bottom_blind_slots,
)
from quiddity.slots import (
    Channel,
    Pocket,
    PocketArray,
    PocketGrid,
    Slot,
    SlotArray,
    SlotGrid,
    recognise_pocket_patterns,
    recognise_slot_patterns,
)
from quiddity.through_steps import ThroughStep, recognise_through_steps
from quiddity.turned import TurnedStep, recognise_turned_steps

# Internal detector identities survive the public SectionRecess schema replacement so that
# discovery, reconciliation and effectiveness scoring remain comparable across the cutover.
RECESS_SOURCE_FAMILIES = frozenset({
    FamilyId.POCKETS, FamilyId.CHANNELS, FamilyId.PRISMATIC_POCKETS,
    FamilyId.PASSAGES, FamilyId.EDGE_OPEN_PRISMATIC_RECESSES,
    FamilyId.EDGE_OPEN_CIRCULAR_POCKETS, FamilyId.RECTANGULAR_BLIND_SLOTS,
    FamilyId.ROUND_BOTTOM_BLIND_SLOTS,
})


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


class DerivedId(Enum):
    """Closed identifiers for post-reconciliation, non-physical projections."""

    HOLE_PATTERNS = "hole_patterns"
    SLOT_PATTERNS = "slot_patterns"
    ORIENTED_SLOT_PATTERNS = "oriented_slot_patterns"
    POCKET_PATTERNS = "pocket_patterns"
    PASSAGES_COMPAT = "passages_compat"


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


PhysicalDiscoverer: TypeAlias = Callable[[DiscoveryServices, CompletedInputs], list[object]]
Applicability: TypeAlias = Callable[[RecognitionContext], bool]
DerivedDiscoverer: TypeAlias = Callable[[AcceptedInputs], list[object]]
ProjectionDiscoverer: TypeAlias = Callable[
    [AcceptedProjectionInputs, ProjectionInputs], list[object]
]


def always(context: RecognitionContext) -> bool:
    del context
    return True


def prismatic(context: RecognitionContext) -> bool:
    return not context.rotational


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


@dataclass(frozen=True, slots=True)
class DerivedDefinition:
    identifier: DerivedId
    record_types: tuple[type[object], ...]
    result_field: str
    public_entrypoint: str | None
    sources: tuple[FamilyId, ...]
    derive: DerivedDiscoverer | ProjectionDiscoverer
    census: CensusSpec
    role: str = "discoverer"


def _simple(call: Callable[[DiscoveryServices], list[object]]) -> PhysicalDiscoverer:
    def discover(services: DiscoveryServices, inputs: CompletedInputs) -> list[object]:
        del inputs
        return call(services)

    return discover


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


def _plates(services: DiscoveryServices, inputs: CompletedInputs) -> list[object]:
    turned_solids = frozenset(
        solid
        for occurrence in inputs.occurrences(FamilyId.TURNED_STEPS, TurnedStep)
        if (solid := occurrence.solid()) is not None
    )
    if services.context.rotational and not turned_solids:
        return []
    return list(
        _discover_plates(
            services.context.part,
            writer=services.writer,
            excluded_solids=turned_solids,
        )
    )


def _risers(services: DiscoveryServices, inputs: CompletedInputs) -> list[object]:
    body_levels: dict[object, list[FaceLevel]] = {}
    for occurrence in inputs.occurrences(FamilyId.STEP_LEVELS, FaceLevel):
        solid = occurrence.solid()
        if solid is None:  # pragma: no cover - completed occurrences revalidate this invariant
            raise ValueError("completed FaceLevel occurrence has no valid solid")
        body_levels.setdefault(solid, []).append(occurrence.record(FaceLevel))
    return list(
        _discover_risers(
            services.context.part,
            writer=services.writer,
            body_levels=body_levels,
        )
    )


def _oriented_slots(services: DiscoveryServices, inputs: CompletedInputs) -> list[object]:
    occurrences = inputs.occurrences(FamilyId.PASSAGES, SectionPassage)
    solids = []
    for occurrence in occurrences:
        solid = occurrence.solid()
        if solid is None:  # pragma: no cover - completed passage evidence is nonempty/same-solid
            raise ValueError("completed SectionPassage occurrence has no valid solid")
        solids.append(solid)
    keys = _oriented_slot_body_keys(services.context.graph, tuple(solids))
    found: list[OrientedSlot] = []
    for occurrence, solid in zip(occurrences, solids, strict=True):
        record = _project_oriented_slot(
            occurrence.record(SectionPassage),
            keys[solid],
        )
        if record is None:
            continue
        defining = occurrence.defining()
        services.writer.sink.propose(FamilyId.ORIENTED_SLOTS, record, defining=defining)
        found.append(record)
    found.sort()
    return list(found)


def _hole_patterns(inputs: AcceptedInputs) -> list[object]:
    return list(recognise_hole_patterns(inputs.records(FamilyId.HOLES, HoleRecord)))


def _slot_patterns(inputs: AcceptedInputs) -> list[object]:
    return list(recognise_slot_patterns(inputs.records(FamilyId.SLOTS, Slot)))


def _oriented_slot_patterns(inputs: AcceptedInputs) -> list[object]:
    return list(
        recognise_oriented_slot_patterns(inputs.records(FamilyId.ORIENTED_SLOTS, OrientedSlot))
    )


def _pocket_patterns(inputs: AcceptedInputs) -> list[object]:
    return list(recognise_pocket_patterns(inputs.records(FamilyId.POCKETS, Pocket)))


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
    PhysicalDefinition(
        FamilyId.COUNTERSINKS,
        (CounterSink,),
        "countersinks",
        "recognise_countersinks",
        (),
        always,
        _simple(lambda s: list(_discover_countersinks(s.context.part, writer=s.writer))),
        Counted("countersink"),
        FullyAttributed("every returned countersink claims its original conical seat face"),
    ),
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
    PhysicalDefinition(
        FamilyId.DOUBLE_D_BORES,
        (DoubleDBore,),
        "double_d_bores",
        "recognise_double_d_bores",
        (),
        always,
        _simple(
            lambda s: list(
                _discover_double_d_bores(
                    s.context.part,
                    face_edges=s.context.face_edges,
                    writer=s.writer,
                )
            )
        ),
        NotCounted("not a distinct census key"),
        FullyAttributed(
            "every returned Double-D bore claims its complete original lateral wall faces"
        ),
    ),
    PhysicalDefinition(
        FamilyId.BOSSES,
        (BossRecord,),
        "bosses",
        "recognise_bosses",
        (),
        always,
        _simple(
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
    PhysicalDefinition(
        FamilyId.POLYGONAL_BOSSES,
        (PolygonalBoss,),
        "polygonal_bosses",
        "recognise_polygonal_bosses",
        (),
        always,
        _simple(
            lambda s: list(
                _discover_polygonal_bosses(
                    s.context.part,
                    graph=s.context.geometry,
                    writer=s.writer,
                )
            )
        ),
        NotCounted("not a distinct census key"),
        FullyAttributed("every returned Polygonal Boss claims its six original side faces"),
    ),
    PhysicalDefinition(
        FamilyId.POLYGONAL_STOCK,
        (PolygonalStock,),
        "polygonal_stock",
        "recognise_polygonal_stock",
        (),
        always,
        _simple(
            lambda s: list(
                _discover_polygonal_stock(
                    s.context.part,
                    graph=s.context.geometry,
                    writer=s.writer,
                )
            )
        ),
        NotCounted("stock context is not a machined feature"),
        FullyAttributed("every returned Polygonal Stock owns its complete eight-face boundary"),
    ),
    PhysicalDefinition(
        FamilyId.CHANNELS,
        (Channel,),
        "channels",
        "recognise_channels",
        (),
        always,
        _simple(
            lambda s: list(
                _discover_channels(
                    s.context.part,
                    face_edges=s.context.face_edges,
                    writer=s.writer,
                )
            )
        ),
        NotCounted("Counted once through the unified section_recess projection"),
        FullyAttributed("every returned Channel owns its exact two opposed side-wall faces"),
    ),
    PhysicalDefinition(
        FamilyId.SLOTS,
        (Slot,),
        "slots",
        "recognise_slots",
        (),
        always,
        _simple(
            lambda s: list(
                _discover_slots(s.context.part, writer=s.writer, face_edges=s.context.face_edges)
            )
        ),
        Counted("slot"),
        FullyAttributed("every returned Slot owns its complete selected wall and cap faces"),
    ),
    PhysicalDefinition(
        FamilyId.RECTANGULAR_BLIND_SLOTS,
        (RectangularBlindSlot,),
        "rectangular_blind_slots",
        "recognise_rectangular_blind_slots",
        (),
        prismatic,
        _simple(lambda s: list(recognise_rectangular_blind_slots(s.context.part, ledger=s.writer))),
        NotCounted("Counted once through the unified section_recess projection"),
        FullyAttributed("every returned rectangular blind slot owns its two sides, floor, and cap"),
    ),
    PhysicalDefinition(
        FamilyId.ROUND_BOTTOM_BLIND_SLOTS,
        (RoundBottomBlindSlot,),
        "round_bottom_blind_slots",
        "recognise_round_bottom_blind_slots",
        (),
        prismatic,
        _simple(
            lambda s: list(recognise_round_bottom_blind_slots(s.context.part, ledger=s.writer))
        ),
        NotCounted("Counted once through the unified section_recess projection"),
        FullyAttributed(
            "every returned round-bottom blind slot owns its two curved sides, floor, and cap"
        ),
    ),
    PhysicalDefinition(
        FamilyId.GROOVES,
        (Groove,),
        "grooves",
        "recognise_grooves",
        (),
        always,
        _simple(
            lambda s: list(
                recognise_grooves(
                    s.context.part,
                    cyls=s.cylinders,
                    ledger=s.writer,
                    face_edges=s.context.face_edges,
                )
            )
        ),
        Counted("groove"),
        FullyAttributed("every returned groove claims its defining groove faces"),
    ),
    PhysicalDefinition(
        FamilyId.FLATS,
        (Flat,),
        "flats",
        "recognise_flats",
        (),
        always,
        _simple(
            lambda s: list(
                _discover_flats(
                    s.context.part,
                    cyls=s.cylinders,
                    face_edges=s.context.face_edges,
                    writer=s.writer,
                )
            )
        ),
        Counted("flat"),
        FullyAttributed("every returned flat claims its defining planar truncation face"),
    ),
    PhysicalDefinition(
        FamilyId.POCKETS,
        (Pocket,),
        "pockets",
        "recognise_pockets",
        (),
        always,
        _simple(
            lambda s: list(
                _discover_pockets(s.context.part, writer=s.writer, face_edges=s.context.face_edges)
            )
        ),
        NotCounted("Counted once through the unified section_recess projection"),
        FullyAttributed("every returned Pocket owns its selected walls, corner floor, or caps"),
    ),
    PhysicalDefinition(
        FamilyId.PRISMATIC_POCKETS,
        (PrismaticPocket,),
        "prismatic_pockets",
        "recognise_prismatic_pockets",
        (),
        always,
        _simple(
            lambda s: list(
                recognise_prismatic_pockets(
                    s.context.part, ledger=s.writer, face_edges=s.context.face_edges
                )
            )
        ),
        NotCounted("Counted once through the unified section_recess projection"),
        FullyAttributed("every returned prismatic pocket claims its defining boundary faces"),
    ),
    PhysicalDefinition(
        FamilyId.EDGE_OPEN_CIRCULAR_POCKETS,
        (EdgeOpenCircularPocket,),
        "edge_open_circular_pockets",
        "recognise_edge_open_circular_pockets",
        (),
        always,
        _simple(
            lambda s: list(
                recognise_edge_open_circular_pockets(
                    s.context.part, ledger=s.writer, face_edges=s.context.face_edges
                )
            )
        ),
        NotCounted("Counted once through the unified section_recess projection"),
        FullyAttributed("every returned open circular pocket claims its physical wall chain"),
    ),
    PhysicalDefinition(
        FamilyId.EDGE_OPEN_PRISMATIC_RECESSES,
        (EdgeOpenPrismaticRecess,),
        "edge_open_prismatic_recesses",
        "recognise_edge_open_prismatic_recesses",
        (),
        always,
        _simple(
            lambda s: list(
                recognise_edge_open_prismatic_recesses(
                    s.context.part, ledger=s.writer, face_edges=s.context.face_edges
                )
            )
        ),
        NotCounted("Counted once through the unified section_recess projection"),
        FullyAttributed("every returned edge-open recess claims its physical wall supports"),
    ),
    PhysicalDefinition(
        FamilyId.SECTION_RECESSES,
        (SectionRecess,),
        "section_recesses",
        "recognise_section_recesses",
        (),
        always,
        _simple(lambda s: list(discover_section_recesses(s.context.part, writer=s.writer))),
        Counted("section_recess"),
        FullyAttributed(
            "every SectionRecess publishes its original wall faces and complete constituent set"
        ),
    ),
    PhysicalDefinition(
        FamilyId.PADS,
        (RaisedPad,),
        "pads",
        "recognise_rectangular_pads",
        (),
        always,
        _simple(
            lambda s: list(
                _discover_rectangular_pads(
                    s.context.part,
                    writer=s.writer,
                    face_surfaces=s.context.face_surfaces,
                    geometry=s.context.geometry,
                )
            )
        ),
        NotCounted("not a distinct census key"),
        FullyAttributed("every returned Pad owns its exact top and four perimeter-wall faces"),
    ),
    PhysicalDefinition(
        FamilyId.REPEATING_RADIAL_PROFILES,
        (RepeatingRadialProfile,),
        "repeating_radial_profiles",
        "recognise_repeating_radial_profiles",
        (),
        always,
        _simple(
            lambda s: list(_discover_repeating_radial_profiles(s.context.part, writer=s.writer))
        ),
        NotCounted("correspondence evidence is not a distinct feature"),
        FullyAttributed(
            "every returned repeating radial profile owns its exact opposed source faces"
        ),
    ),
    PhysicalDefinition(
        FamilyId.TURNED_STEPS,
        (TurnedStep,),
        "turned_steps",
        "recognise_turned_steps",
        (),
        always,
        _simple(
            lambda s: list(
                recognise_turned_steps(s.context.part, cyls=s.cylinders, ledger=s.writer)
            )
        ),
        Counted("step"),
        FullyAttributed("every returned turned step claims its defining profile faces"),
    ),
    PhysicalDefinition(
        FamilyId.STEP_LEVELS,
        (FaceLevel,),
        "step_levels",
        "recognise_face_levels",
        (),
        always,
        _simple(lambda s: list(_discover_step_levels(s.context.part, writer=s.writer))),
        NotCounted("level substrate is not a distinct feature"),
        FullyAttributed(
            "every returned FaceLevel owns the exact body-local horizontal face cluster"
        ),
    ),
    PhysicalDefinition(
        FamilyId.RISERS,
        (RiserEvidence,),
        "risers",
        "recognise_risers",
        (FamilyId.STEP_LEVELS,),
        always,
        _risers,
        NotCounted("riser evidence is not a distinct feature"),
        FullyAttributed("every returned RiserEvidence owns all producing faces on one valid solid"),
    ),
    PhysicalDefinition(
        FamilyId.CHAMFERS,
        (Chamfer,),
        "chamfers",
        "recognise_chamfers",
        (),
        always,
        _simple(
            lambda s: list(
                recognise_chamfers(
                    s.context.part,
                    cyls=s.cylinders,
                    ledger=s.writer,
                    face_edges=s.context.face_edges,
                    include_planar=not s.context.rotational,
                )
            )
        ),
        Counted("chamfer"),
        FullyAttributed("every returned chamfer claims its defining bevel face"),
    ),
    PhysicalDefinition(
        FamilyId.ANGLED_STEPS,
        (AngledStep,),
        "angled_steps",
        "recognise_angled_steps",
        (),
        prismatic,
        _simple(
            lambda s: list(
                recognise_angled_steps(
                    s.context.part, ledger=s.writer, face_edges=s.context.face_edges
                )
            )
        ),
        Counted("angled_step"),
        FullyAttributed("every returned angled step claims its defining slant face"),
    ),
    PhysicalDefinition(
        FamilyId.PAIRED_RAMP_STEPS,
        (PairedRampStep,),
        "paired_ramp_steps",
        "recognise_paired_ramp_steps",
        (),
        prismatic,
        _simple(lambda s: list(recognise_paired_ramp_steps(s.context.part, ledger=s.writer))),
        Counted("paired_ramp_step"),
        FullyAttributed(
            "every returned paired-ramp step claims both original ramps and its closing terminal"
        ),
    ),
    PhysicalDefinition(
        FamilyId.THROUGH_STEPS,
        (ThroughStep,),
        "through_steps",
        "recognise_through_steps",
        (),
        prismatic,
        _simple(lambda s: list(recognise_through_steps(s.context.part, ledger=s.writer))),
        Counted("through_step"),
        FullyAttributed("every returned through step claims both rectangular wall regions"),
    ),
    PhysicalDefinition(
        FamilyId.CIRCULAR_BLIND_STEPS,
        (CircularBlindStep,),
        "circular_blind_steps",
        "recognise_circular_blind_steps",
        (),
        prismatic,
        _simple(
            lambda s: list(
                _discover_circular_blind_steps(
                    s.context.part,
                    graph=s.context.graph,
                    cylinders=s.cylinders,
                    effective=s.context.face_surfaces,
                    sink=s.writer.sink,
                )
            )
        ),
        Counted("circular_blind_step"),
        FullyAttributed(
            "every returned circular blind step claims its cylindrical wall and terminal"
        ),
    ),
    PhysicalDefinition(
        FamilyId.PASSAGES,
        (SectionPassage,),
        "section_passages",
        "recognise_section_passages",
        (),
        always,
        _simple(
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
    PhysicalDefinition(
        FamilyId.ORIENTED_SLOTS,
        (OrientedSlot,),
        "oriented_slots",
        "recognise_oriented_slots",
        (FamilyId.PASSAGES,),
        prismatic,
        _oriented_slots,
        Counted("oriented_slot"),
        FullyAttributed(
            "every oriented slot reissues the exact accepted rectangular passage wall set"
        ),
    ),
    PhysicalDefinition(
        FamilyId.BLENDS,
        (Blend,),
        "blends",
        "recognise_blends",
        (),
        always,
        _simple(
            lambda s: list(
                _discover_blends(
                    s.context.part,
                    graph=s.context.graph,
                    surfaces=s.context.surfaces,
                    writer=s.writer,
                )
            )
        ),
        Counted("blend"),
        FullyAttributed("every returned Blend owns every original cylindrical chain patch"),
    ),
    PhysicalDefinition(
        FamilyId.FILLETS,
        (Fillet,),
        "fillets",
        "recognise_fillets",
        (),
        always,
        _simple(
            lambda s: list(
                _discover_fillets(
                    s.context.part,
                    min_radius=None,
                    max_radius_frac=0.45,
                    cyls=s.cylinders,
                    face_edges=s.context.face_edges,
                    include_cylindrical=not s.context.rotational,
                    writer=s.writer,
                )
            )
        ),
        Counted("fillet"),
        FullyAttributed("every returned fillet claims its original curved blend face"),
    ),
    PhysicalDefinition(
        FamilyId.PLATES,
        (Plate,),
        "plates",
        "recognise_plates",
        (FamilyId.TURNED_STEPS,),
        always,
        _plates,
        Counted("plate"),
        FullyAttributed("every returned Plate claims its complete low/high planar face groups"),
    ),
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
    DerivedDefinition(
        DerivedId.SLOT_PATTERNS,
        (SlotArray, SlotGrid),
        "slot_patterns",
        "recognise_slot_patterns",
        (FamilyId.SLOTS,),
        _slot_patterns,
        NotCounted("not a distinct census key"),
    ),
    DerivedDefinition(
        DerivedId.ORIENTED_SLOT_PATTERNS,
        (OrientedSlotArray, OrientedSlotGrid),
        "oriented_slot_patterns",
        "recognise_oriented_slot_patterns",
        (FamilyId.ORIENTED_SLOTS,),
        _oriented_slot_patterns,
        NotCounted("not a distinct census key"),
    ),
    DerivedDefinition(
        DerivedId.POCKET_PATTERNS,
        (PocketArray, PocketGrid),
        "pocket_patterns",
        "recognise_pocket_patterns",
        (FamilyId.POCKETS,),
        _pocket_patterns,
        NotCounted("not a distinct census key"),
    ),
    DerivedDefinition(
        DerivedId.PASSAGES_COMPAT,
        (Passage,),
        "passages",
        None,
        (FamilyId.PASSAGES,),
        _passages_compat,
        NotCounted("compatibility projection of accepted section passages"),
        "projection",
    ),
)


def validate_definitions(
    physical: tuple[PhysicalDefinition, ...],
    derived: tuple[DerivedDefinition, ...],
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
    counted_keys = [
        definition.census.key for definition in physical if isinstance(definition.census, Counted)
    ] + [definition.census.key for definition in derived if isinstance(definition.census, Counted)]
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
    derived_ids = tuple(definition.identifier for definition in derived)
    if len(set(derived_ids)) != len(derived_ids) or set(derived_ids) != set(DerivedId):
        raise ValueError("derived definitions must cover every derived id exactly once")
    derived_fields = [definition.result_field for definition in derived]
    if len(set(derived_fields)) != len(derived_fields) or set(fields) & set(derived_fields):
        raise ValueError("registry result fields must be unique")
    for derived_definition in derived:
        if not derived_definition.record_types:
            raise ValueError("derived definitions require record contracts")
        if derived_definition.role == "projection":
            if derived_definition.public_entrypoint is not None:
                raise ValueError("projection definitions cannot declare a public entrypoint")
        elif derived_definition.role == "discoverer":
            if not derived_definition.public_entrypoint:
                raise ValueError("discoverer definitions require a public entrypoint")
        else:
            raise ValueError("derived definition role is not recognized")
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

    registered = {definition.result_field for definition in PHYSICAL_DEFINITIONS} | {
        definition.result_field for definition in DERIVED_DEFINITIONS
    }
    if registered != result_fields:
        raise ValueError("registry fields do not exactly cover physical and derived results")


def validate_output(
    definition: PhysicalDefinition | DerivedDefinition,
    records: list[object],
) -> None:
    """Reject an adapter output that violates its declared record contract."""

    if not all(isinstance(record, definition.record_types) for record in records):
        raise TypeError(f"{definition.result_field} discovery returned an undeclared record type")


def validate_census_contract(
    expected: Mapping[str, str],
    physical: tuple[PhysicalDefinition, ...] = PHYSICAL_DEFINITIONS,
    derived: tuple[DerivedDefinition, ...] = DERIVED_DEFINITIONS,
) -> None:
    """Compare census key-to-source bindings with the independent manual census contract."""

    actual = {
        definition.result_field: definition.census.key
        for definition in physical
        if isinstance(definition.census, Counted)
    } | {
        definition.result_field: definition.census.key
        for definition in derived
        if isinstance(definition.census, Counted)
    }
    if actual != dict(expected):
        raise ValueError("registry census bindings do not match the manual census contract")


validate_definitions(PHYSICAL_DEFINITIONS, DERIVED_DEFINITIONS)
