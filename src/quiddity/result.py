# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Immutable aggregate of one complete recognition pass.

This is the orchestration boundary above the package ADR 0002 recognisers. It owns every public
recognition family and the shared evidence consumers reuse. It owns candidate reconciliation,
bounded geometric diagnostics and public-result projection. Requirement identity, drawing
policy and manufacturing decisions belong to consumers and require their own evidence.
"""

from __future__ import annotations

import math
import warnings
from collections.abc import Callable, Iterable, Mapping
from dataclasses import dataclass, field, fields, replace
from enum import Enum
from types import MappingProxyType
from typing import TypeVar, cast

from quiddity._candidates import CandidateSet, EvidenceIndex, FamilyId
from quiddity._claims import ClaimLedger
from quiddity._corner_section import prove_corner_section
from quiddity._correspondence import _CorrespondenceSnapshotAuthority
from quiddity._diagnostics import ResidualDiagnostic, diagnose_residuals
from quiddity._dispositions import (
    Outcome,
    ReasonCode,
)
from quiddity._dispositions import (
    ReconciliationResult as CandidateReconciliation,
)
from quiddity._features import (
    BoltCircle,
    BossRecord,
    HoleRecord,
    LinearArray,
    RectGrid,
)
from quiddity._geometry import plane_axes
from quiddity._open_channel_section import prove_open_channel
from quiddity._reconcile import (
    reconcile_bevel_candidates,
    reconcile_blend_candidates,
    reconcile_circular_step_fillets,
    reconcile_oriented_slot_passages,
    reconcile_profiled_bore_candidates,
    reconcile_recess_candidates,
    reconcile_step_groove_candidates,
)
from quiddity._registry import (
    DERIVED_DEFINITIONS,
    PHYSICAL_DEFINITIONS,
    RECESS_SOURCE_FAMILIES,
    AcceptedInputs,
    AcceptedProjectionInputs,
    DerivedId,
    DiscoveryServices,
    FullyAttributed,
    ProjectionDiscoverer,
    ProjectionInputs,
    _issue_projection_inputs,
    validate_output,
    validate_result_fields,
)
from quiddity._run import RecognitionContext, start
from quiddity._section_adapters import legacy_section_geometry
from quiddity._section_recess import (
    ClosedSectionProfile,
    OpenSectionProfile,
    PlanarEndSurface,
    SectionEnd,
    SectionRecess,
    SectionRecessArray,
    SectionRecessClassification,
    SectionRecessEnds,
    SectionRecessEvidence,
    SectionRecessGeometry,
    SectionRecessGrid,
    SectionRecessRefusal,
)
from quiddity._section_recess_geometry import _polygonal_shape
from quiddity._sections import LocalFrame
from quiddity._typing import Bounds, CylinderInventory, FrozenCylinderInventory, Part
from quiddity.angled_steps import AngledStep
from quiddity.blends import Blend
from quiddity.chamfers import Chamfer
from quiddity.circular_blind_steps import CircularBlindStep
from quiddity.countersinks import CounterSink
from quiddity.edge_open_circular_recesses import EdgeOpenCircularPocket
from quiddity.edge_open_prismatic_recesses import EdgeOpenPrismaticRecess
from quiddity.fillets import Fillet
from quiddity.flats import Flat
from quiddity.grooves import Groove
from quiddity.levels import (
    FaceLevel,
    RiserEvidence,
    bounded_end_margin,
)
from quiddity.oriented_slots import OrientedSlot, OrientedSlotArray, OrientedSlotGrid
from quiddity.pads import RaisedPad
from quiddity.paired_ramp_steps import PairedRampStep
from quiddity.passages import (
    Passage,
    PassageFrame,
    PassageSectionVertex,
    SectionPassage,
)
from quiddity.plates import Plate
from quiddity.polygonal_bosses import (
    PolygonalBoss,
    PolygonalStock,
)
from quiddity.prismatic_pockets import PrismaticPocket
from quiddity.profiled_bores import DoubleDBore
from quiddity.rectangular_blind_slots import RectangularBlindSlot
from quiddity.repeating_profiles import RepeatingRadialProfile
from quiddity.round_bottom_slots import RoundBottomBlindSlot
from quiddity.slots import (
    Channel,
    Pocket,
    PocketArray,
    PocketGrid,
    Slot,
    SlotArray,
    SlotGrid,
)
from quiddity.through_steps import ThroughStep
from quiddity.turned import TurnedProfile, TurnedStep

#: The families this aggregate runs, exactly once, per orchestration.
MIGRATED: frozenset[str] = frozenset(
    definition.public_entrypoint
    for definition in PHYSICAL_DEFINITIONS
    if definition.family not in RECESS_SOURCE_FAMILIES
) | frozenset(
    definition.public_entrypoint
    for definition in DERIVED_DEFINITIONS
    if definition.public_entrypoint is not None
    and definition.identifier not in {DerivedId.POCKET_PATTERNS, DerivedId.PASSAGES_COMPAT}
)


class Deferral(Enum):
    """Why a family is not in :data:`MIGRATED` — a code, not a paragraph.

    Deferrals are explicit compatibility states, not backlog markers. A family may remain
    outside the aggregate only while one of these concrete constraints is true.

    ``CLASSIFICATION_GATED`` — an automatic-model consumer runs it only for one part class, so
    hoisting it unconditionally scans the other class for a result that is discarded.  Note
    this constraint is about applicability, not ownership. It ends when the orchestration
    carries the classification and can make the decision once.

    ``BUILD_MODEL_ONLY`` — only an automatic-model consumer needs the result. Hoisting it
    unconditionally would remove no scan from that path and add one for callers that already
    supply declared geometry.

    ``CALLER_SPECIFIC_INPUT`` — an input other than the part decides the answer and the
    callers pass different ones, so there is no single per-build value for a frozen
    aggregate to hold. Experience showed the reason is usually a
    *shape* problem rather than a fact about the feature: the scan did not depend on the
    caller's input, only the filter did, so separating the two gave the aggregate something
    single-valued to own.  Prefer that split before reaching for this member again.

    ``NO_INDEPENDENT_CONSUMER`` — reached only through one shared helper, so there is
    nothing to cache for.  Unlike the others, not scheduled to change.
    """

    CLASSIFICATION_GATED = "classification-gated"
    BUILD_MODEL_ONLY = "build-part-model-only"
    CALLER_SPECIFIC_INPUT = "caller-specific-input"
    NO_INDEPENDENT_CONSUMER = "no-independent-consumer"


@dataclass(frozen=True)
class Deferred:
    """A family the aggregate does not own, and the constraint that stops it.

    ``blocker`` is the issue that removes the constraint, or ``None`` when the deferral is
    not scheduled to end.  A deferral without either is "not got to it yet", which is not a
    reason.
    """

    reason: Deferral
    blocker: int | None = None


#: The families the aggregate does NOT own, each with its constraint.
#:
#: ``BUILD_MODEL_ONLY`` is gone as a live reason: its three families cost the
#: declared path nothing because that path does not run automatic recognition, so aggregate
#: completeness was reason enough on its own.  The enum member survives because a future
#: family can be deferred for that reason again; what does not survive is a *deferral*
#: justified by a cost that no longer exists.
#:
#: ``CALLER_SPECIFIC_INPUT`` is gone too: ``recognise_step_shoulders`` split into a
#: level-free scan the aggregate owns (``recognise_risers``) and a pure
#: ``project_step_shoulders`` each consumer applies with its own level set.
#:
#: ``CLASSIFICATION_GATED`` is gone last. Its three families are gated INSIDE the
#: orchestration now — one place decides, once, from the classification the result carries —
#: rather than each call site deciding for itself. Migration and applicability turned out to
#: be different questions: the aggregate can own a family it does not always run.
#:
#: The map is empty: every public
#: ``recognise_*`` family is owned by the one orchestration. The mechanism stays — a new
#: family still has to be classified — and every enum member survives for a future one.
DEFERRED: dict[str, Deferred] = {}


# Internal execution order is derived from the explicit closed registry, never filesystem order.
PHYSICAL_FAMILIES: tuple[FamilyId, ...] = tuple(
    definition.family for definition in PHYSICAL_DEFINITIONS
)


@dataclass(frozen=True, slots=True)
class CandidateInventory:
    """One source-ordered candidate set for every physical family."""

    _sets: Mapping[FamilyId, CandidateSet[object]]

    @classmethod
    def complete(cls, sets: Iterable[CandidateSet[object]]) -> CandidateInventory:
        by_family = {candidate_set.family: candidate_set for candidate_set in sets}
        if tuple(by_family) != PHYSICAL_FAMILIES or len(by_family) != len(PHYSICAL_FAMILIES):
            raise ValueError("physical candidate inventory is incomplete or out of order")
        return cls(MappingProxyType(by_family))

    def candidate_set(self, family: FamilyId) -> CandidateSet[object]:
        return self._sets[family]

    def records(self, family: FamilyId) -> tuple[object, ...]:
        return tuple(candidate.record for candidate in self.candidate_set(family).candidates)

    def replacing(self, replacements: Iterable[CandidateSet[object]]) -> CandidateInventory:
        changed = dict(self._sets)
        for candidate_set in replacements:
            changed[candidate_set.family] = candidate_set
        return CandidateInventory.complete(changed[family] for family in PHYSICAL_FAMILIES)


@dataclass(frozen=True, slots=True)
class DerivedInventory:
    """Post-reconciliation projections; these are not physical candidates."""

    hole_patterns: tuple[BoltCircle | LinearArray | RectGrid, ...]
    slot_patterns: tuple[SlotArray | SlotGrid, ...]
    oriented_slot_patterns: tuple[OrientedSlotArray | OrientedSlotGrid, ...]
    pocket_patterns: tuple[PocketArray | PocketGrid, ...]
    passages: tuple[Passage, ...]


@dataclass(frozen=True, slots=True)
class InventoryProduct:
    """The one internal inventory consumed by result, census and attribution views."""

    context: RecognitionContext
    evidence: EvidenceIndex
    physical: CandidateInventory
    reconciliation: CandidateReconciliation
    diagnostics: tuple[ResidualDiagnostic, ...]
    derived: DerivedInventory
    result: RecognitionResult
    _legacy_result: _LegacyRecognitionResult
    _correspondence_authority: object | None = field(default=None, repr=False, compare=False)

    @property
    def accepted(self) -> CandidateInventory:
        return CandidateInventory.complete(
            self.reconciliation.accepted_set(self.physical.candidate_set(family))
            for family in PHYSICAL_FAMILIES
        )

    @property
    def distinct_steps(self) -> CandidateSet[object]:
        source = self.physical.candidate_set(FamilyId.TURNED_STEPS)
        related = {
            id(item.candidate)
            for item in self.reconciliation.for_family(FamilyId.TURNED_STEPS)
            if item.reason is ReasonCode.TURNED_STEP_GROOVE_COMPATIBLE
        }
        accepted = self.reconciliation.accepted_set(source)
        result = object.__new__(CandidateSet)
        object.__setattr__(result, "family", FamilyId.TURNED_STEPS)
        object.__setattr__(
            result,
            "candidates",
            tuple(candidate for candidate in accepted.candidates if id(candidate) not in related),
        )
        object.__setattr__(result, "_issuer", source._issuer)
        return result


@dataclass(frozen=True)
class RecognitionResult:
    """The immutable feature inventory produced by one recognition orchestration run.

    Every public ``recognise_*`` family is owned here, although classification gates mean an
    inapplicable family need not run. This is a recognition inventory, not drafting state and
    not a promise that the evidence-gated correspondence extensions have landed.
    """

    cylinders: FrozenCylinderInventory
    countersinks: tuple[CounterSink, ...]
    holes: tuple[HoleRecord, ...]
    double_d_bores: tuple[DoubleDBore, ...]
    hole_patterns: tuple[BoltCircle | LinearArray | RectGrid, ...]
    bosses: tuple[BossRecord, ...]
    polygonal_bosses: tuple[PolygonalBoss, ...]
    polygonal_stock: tuple[PolygonalStock, ...]

    slots: tuple[Slot, ...]
    #: Rectangular through slots whose in-plane axes are not principal in the supplied frame.
    oriented_slots: tuple[OrientedSlot, ...]
    slot_patterns: tuple[SlotArray | SlotGrid, ...]
    oriented_slot_patterns: tuple[OrientedSlotArray | OrientedSlotGrid, ...]
    grooves: tuple[Groove, ...]
    flats: tuple[Flat, ...]
    #: Constant-section recesses with result-local body and face references (ADR 0019).
    section_recesses: tuple[SectionRecess, ...]
    section_recess_refusals: tuple[SectionRecessRefusal, ...]
    section_recess_patterns: tuple[SectionRecessArray | SectionRecessGrid, ...]

    pads: tuple[RaisedPad, ...]
    #: Complete outer-wire cyclic correspondence.  Geometry-only: consumers may compare a
    #: declared axis/count, but this inventory never manufactures gear semantics.
    repeating_radial_profiles: tuple[RepeatingRadialProfile, ...]
    turned_steps: tuple[TurnedStep, ...]
    #: Area-filtered interior prismatic levels. The support spans remain on each record so IR
    #: assembly can preserve level-to-face correspondence; sizing and critique project the Z
    #: values through :meth:`step_ladder_for_z_span`.
    step_levels: tuple[FaceLevel, ...]
    #: Whether the part classified as ROTATIONAL, carried so consumers can distinguish the
    #: context used by classification-gated families. Plate discovery remains absent for a
    #: rotational-classified shape with no established turned profile. In a mixed compound,
    #: completed TurnedStep ownership suppresses only the same solid, leaving independent
    #: prismatic bodies eligible.
    rotational: bool
    #: Candidate step risers, scanned once and projected per consumer. NOT shoulders:
    #: which risers count depends on the level set the asker holds, and that is the whole
    #: reason this family could not be hoisted until the scan and the filter were separated.
    risers: tuple[RiserEvidence, ...]
    #: Chamfers and fillets are recognised on every part: planar/cylindrical on a prismatic
    #: part and conical/toroidal on a turned part. Plate discovery consumes completed turned-step
    #: occurrences and excludes only their owning solids. The dependency lives in the one
    #: orchestration rather than at each call site.
    chamfers: tuple[Chamfer, ...]
    #: Prismatic-only: an angled blind step is the same planar oblique-bevel read as a
    #: chamfer, while the conical bevel on a rotational part cannot establish one.
    angled_steps: tuple[AngledStep, ...]
    #: Prismatic-only conservative two-ramp through-side cuts. The supported domain is the
    #: unfragmented mirror-symmetric terminal topology documented by its recogniser.
    paired_ramp_steps: tuple[PairedRampStep, ...]
    #: Prismatic-only rectangular open-profile steps spanning a source solid.
    through_steps: tuple[ThroughStep, ...]
    #: Prismatic-only quarter-cylindrical corner cuts with one interior blind terminal.
    circular_blind_steps: tuple[CircularBlindStep, ...]
    #: Complete straight or circular rolling-ball paths not superseded by a specific family.
    blends: tuple[Blend, ...]
    fillets: tuple[Fillet, ...]
    plates: tuple[Plate, ...]

    @property
    def turned_profiles(self) -> tuple[TurnedProfile, ...]:
        """Body-local physical profile groups derived from the turned-step occurrence roster."""

        return TurnedProfile.grouped_from_steps(self.turned_steps)

    def step_ladder_for_z_span(
        self,
        z_min: float,
        z_max: float,
        *,
        boundary_margin: float | None = None,
    ) -> list[float]:
        """Return the effective step ladder within an explicit Z envelope.

        ``z_min``, ``z_max``, and an explicit ``boundary_margin`` use model length units
        (millimetres in conventional build123d/STEP workflows). For a Z-turned profile, only
        shoulders strictly inside ``z_min + boundary_margin`` and ``z_max - boundary_margin`` are
        rungs; equality is excluded. A span narrower than twice the margin therefore has no
        turned rungs.

        ``boundary_margin=None`` uses :data:`STEP_LADDER_BOUNDARY_MARGIN`, capped so it can
        never exceed a quarter of the span. The inset excludes an *end treatment* — a chamfer or
        edge break just inside the face — which is a manufacturing constant that does not grow
        with the shaft, so ADR 0008 keeps it absolute and bounds it instead. It stays the same
        rule ``step_level_records`` applies, which ADR 0006 requires the two to share.

        Prismatic levels are already envelope-filtered by :func:`step_level_records` during the
        recognition pass, so this projection returns them unchanged. The span is still validated
        on every path so invalid geometry input cannot be hidden by part classification.

        One rule serves every consumer. Model construction and completeness checks need the
        same set, but deriving it separately could silently project over different ladders.

        Geometry-only, so it is a legitimate source for critique under the independent-evidence
        rule: it reads the aggregate's own recognition, never the model.
        """
        if not math.isfinite(z_min) or not math.isfinite(z_max):
            raise ValueError("z_min and z_max must be finite")
        if z_min > z_max:
            raise ValueError("z_min must not exceed z_max")
        if boundary_margin is None:
            boundary_margin = bounded_end_margin(z_max - z_min)
        if not math.isfinite(boundary_margin) or boundary_margin < 0.0:
            raise ValueError("boundary_margin must be finite and non-negative")
        profiles = self.turned_profiles
        prof = profiles[0] if len(profiles) == 1 else None
        if prof is not None and prof.axis == "z":
            return [
                float(z)
                for z in prof.shoulders
                if z_min + boundary_margin < z < z_max - boundary_margin
            ]
        return [level.z for level in self.step_levels]

    def step_ladder(self, bb: Bounds) -> list[float]:
        """Compatibility shim for a build123d bounding box.

        Deprecated since 0.2.1. Use :meth:`step_ladder_for_z_span` with the two scalar Z limits.
        This shim remains for the 0.2.x compatibility line and is removed no earlier than 1.0.0.
        """
        warnings.warn(
            "RecognitionResult.step_ladder(BoundBox) is deprecated since 0.2.1; use "
            "step_ladder_for_z_span(z_min, z_max). It will be removed no earlier than 1.0.0.",
            DeprecationWarning,
            stacklevel=2,
        )
        return self.step_ladder_for_z_span(float(bb.min.Z), float(bb.max.Z))


@dataclass(frozen=True)
class _LegacyRecognitionResult(RecognitionResult):
    """Internal detector inventory for scoring and frozen migration measurements only."""

    channels: tuple[Channel, ...]
    rectangular_blind_slots: tuple[RectangularBlindSlot, ...]
    round_bottom_blind_slots: tuple[RoundBottomBlindSlot, ...]
    pockets: tuple[Pocket, ...]
    prismatic_pockets: tuple[PrismaticPocket, ...]
    edge_open_circular_pockets: tuple[EdgeOpenCircularPocket, ...]
    edge_open_prismatic_recesses: tuple[EdgeOpenPrismaticRecess, ...]
    pocket_patterns: tuple[PocketArray | PocketGrid, ...]
    section_passages: tuple[SectionPassage, ...]
    passages: tuple[Passage, ...]


validate_result_fields(
    frozenset(_LegacyRecognitionResult.__dataclass_fields__)
    - {"cylinders", "rotational", "section_recess_refusals", "section_recess_patterns"}
)


def build_raw_recognition_result(
    part: Part,
    *,
    cylinders: CylinderInventory | None = None,
    rotational: bool = False,
) -> RecognitionResult:
    """Run the shared recognition inventory once in the caller's coordinate frame.

    Dependencies are computed by this orchestration layer and injected downstream: holes
    reuse both the cylinder substrate and countersinks, while patterns reuse their accepted
    member records.  No recogniser rediscovers one of those dependencies internally.

    *rotational* is the caller's geometric classification.
    It gates the three families that only one part class consumes, so migrating them did not
    mean scanning every turned build for a discarded result.

    It is a scoping constraint, not an architectural one.
    The classification itself is geometry-only: it can be derived from bounding-box
    proportions, the largest external cylinder, and concentricity. It remains caller-supplied
    for compatibility; consumers may also use the same fact for view or drawing policy without
    transferring ownership of that policy into this package.

    The default is ``False`` — prismatic, so nothing is gated away.  A caller who has no
    classification (the lazy critique aggregate on a declared build) gets the COMPLETE
    inventory, which is the right default for a completeness check: over-recognising costs
    time, under-recognising reports a real feature as absent.
    """

    return _take_inventory(part, cylinders=cylinders, rotational=rotational).result


def build_recognition_result(
    part: Part,
    *,
    cylinders: CylinderInventory | None = None,
    rotational: bool = False,
) -> RecognitionResult:
    """Compatibility name for caller-coordinate recognition.

    New integrations should ordinarily use ``build_framed_recognition_result`` and retain its
    frame and working part.  Use ``build_raw_recognition_result`` when caller/world coordinates
    are deliberately required. This compatibility name retains caller-coordinate semantics;
    it will not silently change return type. The former pre-Quiddity 0.4/0.5 migration schedule
    is historical, not a removal promise for the reset Quiddity version series.
    """

    return build_raw_recognition_result(
        part,
        cylinders=cylinders,
        rotational=rotational,
    )


def _take_inventory(
    part: Part,
    *,
    cylinders: CylinderInventory | None = None,
    rotational: bool = False,
) -> InventoryProduct:
    """Run the explicit physical, reconciliation, derived and projection phases once."""

    context = start(part, cylinders, rotational=rotational)
    ledger = ClaimLedger(context.graph, definitions=PHYSICAL_DEFINITIONS)
    physical = CandidateInventory.complete(_discover_all(context, ledger))
    evidence = ledger.freeze_index()
    evidence.validate_complete_inventory(
        tuple(physical.candidate_set(family) for family in PHYSICAL_FAMILIES)
    )
    _validate_attribution(context, physical, evidence)
    reconciliation = _reconcile_existing(physical, evidence)
    diagnostics = diagnose_residuals(reconciliation, evidence)
    accepted = CandidateInventory.complete(
        reconciliation.accepted_set(physical.candidate_set(family)) for family in PHYSICAL_FAMILIES
    )
    derived = _derive_patterns(accepted)
    passage_definition = next(
        item for item in PHYSICAL_DEFINITIONS if item.family is FamilyId.PASSAGES
    )
    passage_projection = _issue_projection_inputs(
        accepted.candidate_set(FamilyId.PASSAGES),
        evidence,
    )
    derived = replace(
        derived,
        passages=_derive_passage_compat(
            passage_projection,
            ProjectionInputs(passage_definition.projected(context)),
        ),
    )
    result = _project_result(context, accepted, derived, evidence)
    correspondence = _CorrespondenceSnapshotAuthority()
    product = InventoryProduct(
        context=context,
        evidence=evidence,
        physical=physical,
        reconciliation=reconciliation,
        diagnostics=diagnostics,
        derived=derived,
        result=RecognitionResult(
            **{item.name: getattr(result, item.name) for item in fields(RecognitionResult)}
        ),
        _legacy_result=result,
        _correspondence_authority=correspondence,
    )
    correspondence.bind(product)
    return product


def _discover_all(
    context: RecognitionContext, ledger: ClaimLedger
) -> tuple[CandidateSet[object], ...]:
    """Discover and atomically complete each physical family in registry order."""

    services = DiscoveryServices(
        context=context,
        writer=ledger.writer,
        cylinders=(list(context.cylinders[0]), list(context.cylinders[1])),
    )
    completed: list[CandidateSet[object]] = []
    for definition in PHYSICAL_DEFINITIONS:
        inputs = ledger.restricted_inputs(definition)
        applicable = definition.applicable(context)
        records = definition.discover(services, inputs) if applicable else []
        validate_output(definition, records)
        completed.append(ledger.candidate_set_for(definition.family, records))
    return tuple(completed)


def _validate_attribution(
    context: RecognitionContext, physical: CandidateInventory, evidence: EvidenceIndex
) -> None:
    """Revalidate registry completeness and common-solid provenance on terminal evidence."""

    for definition in PHYSICAL_DEFINITIONS:
        candidates = physical.candidate_set(definition.family).candidates
        for candidate in candidates:
            defining = evidence.defining_of(candidate)
            if isinstance(definition.attribution, FullyAttributed) and not defining:
                raise ValueError(
                    f"{definition.family.value} promises complete defining attribution"
                )
            if defining and context.graph.common_valid_solid(defining) is None:
                raise ValueError("physical defining evidence lost its common valid solid")


RecordT = TypeVar("RecordT")


def _records(
    inventory: CandidateInventory,
    family: FamilyId,
    record_type: type[RecordT],
) -> list[RecordT]:
    records = list(inventory.records(family))
    if not all(isinstance(record, record_type) for record in records):
        raise TypeError(f"{family.value} inventory has the wrong record type")
    return cast(list[RecordT], records)


def _reconcile_existing(
    physical: CandidateInventory, evidence: EvidenceIndex
) -> CandidateReconciliation:
    """Apply existing policies to completed candidates and terminal evidence only."""

    decisions = reconcile_recess_candidates(
        physical.candidate_set(FamilyId.SLOTS),
        physical.candidate_set(FamilyId.POCKETS),
        physical.candidate_set(FamilyId.PRISMATIC_POCKETS),
        physical.candidate_set(FamilyId.PASSAGES),
        evidence,
        rectangular_blind_slots=physical.candidate_set(FamilyId.RECTANGULAR_BLIND_SLOTS),
        edge_open_circular_pockets=physical.candidate_set(FamilyId.EDGE_OPEN_CIRCULAR_POCKETS),
    )
    decisions += reconcile_bevel_candidates(
        physical.candidate_set(FamilyId.CHAMFERS),
        physical.candidate_set(FamilyId.ANGLED_STEPS),
        evidence,
    )
    circular_fillet_decisions = reconcile_circular_step_fillets(
        physical.candidate_set(FamilyId.FILLETS),
        physical.candidate_set(FamilyId.CIRCULAR_BLIND_STEPS),
        evidence,
    )
    decisions += circular_fillet_decisions
    decisions += reconcile_blend_candidates(
        physical.candidate_set(FamilyId.BLENDS),
        physical.candidate_set(FamilyId.FILLETS),
        evidence,
        rejected_fillets=frozenset(
            decision.candidate
            for decision in circular_fillet_decisions
            if decision.outcome is Outcome.REJECTED
        ),
    )
    decisions += reconcile_profiled_bore_candidates(
        physical.candidate_set(FamilyId.HOLES),
        physical.candidate_set(FamilyId.DOUBLE_D_BORES),
        evidence,
    )
    decisions += reconcile_step_groove_candidates(
        physical.candidate_set(FamilyId.TURNED_STEPS),
        physical.candidate_set(FamilyId.GROOVES),
        evidence,
    )
    decisions += reconcile_oriented_slot_passages(
        physical.candidate_set(FamilyId.PASSAGES),
        physical.candidate_set(FamilyId.ORIENTED_SLOTS),
        evidence,
    )
    return CandidateReconciliation.complete(
        tuple(physical.candidate_set(family) for family in PHYSICAL_FAMILIES),
        decisions,
        evidence,
    )


def _derive_patterns(accepted: CandidateInventory) -> DerivedInventory:
    """Derive pattern projections only from accepted member records."""

    accepted_records = {family: tuple(accepted.records(family)) for family in PHYSICAL_FAMILIES}
    derived: dict[DerivedId, tuple[object, ...]] = {}
    for definition in DERIVED_DEFINITIONS:
        if definition.role == "projection":
            continue
        inputs = AcceptedInputs.restricted(definition.sources, accepted_records)
        standard_derive = cast(Callable[[AcceptedInputs], list[object]], definition.derive)
        records = standard_derive(inputs)
        validate_output(definition, records)
        derived[definition.identifier] = tuple(records)
    return DerivedInventory(
        hole_patterns=cast(
            tuple[BoltCircle | LinearArray | RectGrid, ...],
            derived[DerivedId.HOLE_PATTERNS],
        ),
        slot_patterns=cast(tuple[SlotArray | SlotGrid, ...], derived[DerivedId.SLOT_PATTERNS]),
        oriented_slot_patterns=cast(
            tuple[OrientedSlotArray | OrientedSlotGrid, ...],
            derived[DerivedId.ORIENTED_SLOT_PATTERNS],
        ),
        pocket_patterns=cast(
            tuple[PocketArray | PocketGrid, ...], derived[DerivedId.POCKET_PATTERNS]
        ),
        passages=(),
    )


def _derive_passage_compat(
    inputs: AcceptedProjectionInputs, projection: ProjectionInputs
) -> tuple[Passage, ...]:
    definition = next(
        item for item in DERIVED_DEFINITIONS if item.identifier is DerivedId.PASSAGES_COMPAT
    )
    derive = cast(ProjectionDiscoverer, definition.derive)
    records = derive(inputs, projection)
    validate_output(definition, records)
    return cast(tuple[Passage, ...], tuple(records))


def _section_passage_recess(
    record: SectionPassage,
    *,
    context: RecognitionContext,
    evidence: EvidenceIndex,
    index: int,
) -> SectionRecess:
    """Project one accepted passage without rediscovering or weakening its physical proof."""

    defining = evidence.defining_of(record)
    constituent = evidence.constituent_of(record)
    owner = context.graph.common_valid_solid(defining)
    if owner is None:
        raise ValueError("accepted section passage lost its body authority")
    line_only = all(vertex.bulge == 0.0 for vertex in record.section.boundary)
    section_shape = (
        _polygonal_shape(tuple(vertex.point for vertex in record.section.boundary))
        if line_only
        else "general"
    )
    geometry = SectionRecessGeometry(
        "section_recess",
        record.frame,
        record.run_interval,
        ClosedSectionProfile("closed", record.section.boundary),
        SectionRecessEnds(
            SectionEnd("open", PlanarEndSurface(gradient=record.ends.low_gradient)),
            SectionEnd("open", PlanarEndSurface(gradient=record.ends.high_gradient)),
        ),
    )
    return SectionRecess(
        index,
        owner.ordinal,
        geometry,
        SectionRecessClassification("passage", section_shape),
        SectionRecessEvidence(
            tuple(sorted(node.index for node in defining)),
            tuple(sorted(node.index for node in constituent)),
        ),
    )


def _prismatic_pocket_recess(
    record: PrismaticPocket,
    *,
    context: RecognitionContext,
    evidence: EvidenceIndex,
    index: int,
) -> SectionRecess:
    """Project one accepted pocket at the legacy publication grid."""

    defining = evidence.defining_of(record)
    constituent = evidence.constituent_of(record)
    owner = context.graph.common_valid_solid(defining)
    if owner is None:
        raise ValueError("accepted prismatic pocket lost its body authority")
    geometry = legacy_section_geometry(record)
    section_shape = _polygonal_shape(tuple(vertex.point for vertex in geometry.profile.boundary))
    if len(geometry.profile.boundary) != len(record.section):
        section_shape = "polygonal"
    return SectionRecess(
        index,
        owner.ordinal,
        geometry,
        SectionRecessClassification("pocket", section_shape),
        SectionRecessEvidence(
            tuple(sorted(node.index for node in defining)),
            tuple(sorted(node.index for node in constituent)),
        ),
    )


def _canonical_open_profile(
    boundary: tuple[PassageSectionVertex, ...],
) -> OpenSectionProfile:
    reversed_boundary = tuple(
        PassageSectionVertex(
            boundary[-1 - index].point,
            -boundary[-2 - index].bulge if index < len(boundary) - 1 else 0.0,
        )
        for index in range(len(boundary))
    )
    canonical = min(boundary, reversed_boundary)
    return OpenSectionProfile(
        "open",
        canonical,
        (canonical[-1].point, canonical[0].point),
    )


def _principal_open_geometry(
    *,
    axis: str,
    run_interval: tuple[float, float],
    open_sign: int,
    boundary: tuple[PassageSectionVertex, ...],
    placement: tuple[float, float, float] | None = None,
) -> SectionRecessGeometry:
    axis_index = "xyz".index(axis)
    transverse = tuple(index for index in range(3) if index != axis_index)
    points = tuple(vertex.point for vertex in boundary)
    center2 = cast(
        tuple[float, float],
        tuple(
            0.5 * (min(point[i] for point in points) + max(point[i] for point in points))
            for i in range(2)
        ),
    )
    center3 = [0.0, 0.0, 0.0]
    center3[transverse[0]], center3[transverse[1]] = center2
    if placement is not None:
        center3 = list(placement)
    frame_value = LocalFrame.principal(axis, cast(tuple[float, float, float], tuple(center3)))
    coordinate_order = (1, 0) if axis == "y" else (0, 1)
    local = tuple(
        PassageSectionVertex(
            vertex.point
            if placement is not None
            else cast(
                tuple[float, float],
                tuple(round(vertex.point[index] - center2[index], 4) for index in coordinate_order),
            ),
            -vertex.bulge if placement is None and axis == "y" else vertex.bulge,
        )
        for vertex in boundary
    )
    frame = PassageFrame(
        cast(tuple[float, float, float], tuple(round(value, 3) for value in frame_value.origin)),
        frame_value.run,
        frame_value.u,
        frame_value.v,
    )
    return SectionRecessGeometry(
        "section_recess",
        frame,
        run_interval,
        _canonical_open_profile(local),
        SectionRecessEnds(
            SectionEnd("open" if open_sign == -1 else "capped"),
            SectionEnd("capped" if open_sign == -1 else "open"),
        ),
    )


def _principal_local_point(axis: str, offsets: Mapping[str, float]) -> tuple[float, float]:
    """Express principal-axis transverse offsets in ``LocalFrame.principal`` coordinates."""

    transverse = {"x": ("y", "z"), "y": ("z", "x"), "z": ("x", "y")}[axis]
    return (round(offsets.get(transverse[0], 0.0), 4), round(offsets.get(transverse[1], 0.0), 4))


def _legacy_section_recess(
    record: object,
    *,
    context: RecognitionContext,
    evidence: EvidenceIndex,
    index: int,
    geometry: SectionRecessGeometry,
    feature_kind: str,
    section_shape: str,
) -> SectionRecess:
    defining = evidence.defining_of(record)
    constituent = evidence.constituent_of(record)
    owner = context.graph.common_valid_solid(defining)
    if owner is None:
        raise ValueError("accepted legacy section recess lost its body authority")
    return SectionRecess(
        index,
        owner.ordinal,
        geometry,
        SectionRecessClassification(feature_kind, section_shape),
        SectionRecessEvidence(
            tuple(sorted(node.index for node in defining)),
            tuple(sorted(node.index for node in constituent)),
        ),
    )


def _corner_pocket_recess(
    record: Pocket | Channel,
    *,
    context: RecognitionContext,
    evidence: EvidenceIndex,
    index: int,
) -> SectionRecess | None:
    if isinstance(record, Channel) or not record.edge_anchored:
        channel = prove_open_channel(
            context.graph, evidence.defining_of(record), evidence.constituent_of(record), record
        )
        if channel is None:
            return None
        geometry = _principal_open_geometry(
            axis=channel.axis,
            run_interval=channel.run_interval,
            open_sign=1,
            boundary=tuple(
                PassageSectionVertex((round(p[0], 4), round(p[1], 4)), 0.0)
                for p in channel.boundary
            ),
        )
        geometry = replace(geometry, ends=SectionRecessEnds(SectionEnd("open"), SectionEnd("open")))
        return _legacy_section_recess(
            record,
            context=context,
            evidence=evidence,
            index=index,
            geometry=geometry,
            feature_kind="channel",
            section_shape="rectangular",
        )
    proof = prove_corner_section(context.graph, evidence.defining_of(record), record.depth_axis)
    if proof is None:
        return None
    geometry = _principal_open_geometry(
        axis=record.depth_axis,
        run_interval=proof.run_interval,
        open_sign=proof.open_sign,
        boundary=tuple(
            PassageSectionVertex((round(point[0], 4), round(point[1], 4)), 0.0)
            for point in proof.boundary
        ),
    )
    return _legacy_section_recess(
        record,
        context=context,
        evidence=evidence,
        index=index,
        geometry=geometry,
        feature_kind="edge_open_recess",
        section_shape="polygonal",
    )


def _rectangular_blind_slot_recess(
    record: RectangularBlindSlot,
    *,
    context: RecognitionContext,
    evidence: EvidenceIndex,
    index: int,
) -> SectionRecess:
    half_width, half_depth = record.width / 2, record.depth / 2
    opening = record.depth_sign * half_depth
    floor = -opening
    chain = tuple(
        PassageSectionVertex(
            _principal_local_point(
                record.axis,
                {record.width_axis: width, record.depth_axis: depth},
            ),
            0.0,
        )
        for width, depth in (
            (-half_width, opening),
            (-half_width, floor),
            (half_width, floor),
            (half_width, opening),
        )
    )
    geometry = _principal_open_geometry(
        axis=record.axis,
        run_interval=(
            round(record.at["xyz".index(record.axis)] - record.length / 2, 3),
            round(record.at["xyz".index(record.axis)] + record.length / 2, 3),
        ),
        open_sign=record.open_sign,
        boundary=chain,
        placement=record.at,
    )
    return _legacy_section_recess(
        record,
        context=context,
        evidence=evidence,
        index=index,
        geometry=geometry,
        feature_kind="edge_open_recess",
        section_shape="rectangular",
    )


def _round_bottom_blind_slot_recess(
    record: RoundBottomBlindSlot,
    *,
    context: RecognitionContext,
    evidence: EvidenceIndex,
    index: int,
) -> SectionRecess:
    half_width = record.width / 2
    half_flat = record.flat_width / 2
    half_depth = record.radius / 2
    opening = record.depth_sign * half_depth
    floor = -opening

    def point(width: float, depth: float) -> tuple[float, float]:
        return _principal_local_point(
            record.axis,
            {record.width_axis: width, record.depth_axis: depth},
        )

    width_vector = point(1.0, 0.0)
    depth_vector = point(0.0, 1.0)
    determinant = width_vector[0] * depth_vector[1] - width_vector[1] * depth_vector[0]
    orientation = 1 if determinant > 0 else -1
    arc_bulge = round(math.tan(record.depth_sign * orientation * math.pi / 8), 12)
    chain = (
        PassageSectionVertex(point(-half_width, opening), arc_bulge),
        PassageSectionVertex(point(-half_flat, floor), 0.0),
        PassageSectionVertex(point(half_flat, floor), arc_bulge),
        PassageSectionVertex(point(half_width, opening), 0.0),
    )
    geometry = _principal_open_geometry(
        axis=record.axis,
        run_interval=(
            round(record.at["xyz".index(record.axis)] - record.length / 2, 3),
            round(record.at["xyz".index(record.axis)] + record.length / 2, 3),
        ),
        open_sign=record.open_sign,
        boundary=chain,
        placement=record.at,
    )
    return _legacy_section_recess(
        record,
        context=context,
        evidence=evidence,
        index=index,
        geometry=geometry,
        feature_kind="edge_open_recess",
        section_shape="general",
    )


def _edge_open_prismatic_recess(
    record: EdgeOpenPrismaticRecess,
    *,
    context: RecognitionContext,
    evidence: EvidenceIndex,
    index: int,
) -> SectionRecess:
    defining = evidence.defining_of(record)
    constituent = evidence.constituent_of(record)
    owner = context.graph.common_valid_solid(defining)
    if owner is None:
        raise ValueError("accepted edge-open prismatic recess lost its body authority")
    boundary = tuple(PassageSectionVertex(point, 0.0) for point in record.section.wall_chain)
    return SectionRecess(
        index,
        owner.ordinal,
        _principal_open_geometry(
            axis=record.axis,
            run_interval=record.run_interval,
            open_sign=record.open_sign,
            boundary=boundary,
        ),
        SectionRecessClassification(
            "edge_open_recess",
            _polygonal_shape(tuple(vertex.point for vertex in boundary))
            if len(boundary) == 4
            else "polygonal",
        ),
        SectionRecessEvidence(
            tuple(sorted(node.index for node in defining)),
            tuple(sorted(node.index for node in constituent)),
        ),
    )


def _edge_open_circular_recess(
    record: EdgeOpenCircularPocket,
    *,
    context: RecognitionContext,
    evidence: EvidenceIndex,
    index: int,
) -> SectionRecess:
    defining = evidence.defining_of(record)
    constituent = evidence.constituent_of(record)
    owner = context.graph.common_valid_solid(defining)
    if owner is None:
        raise ValueError("accepted edge-open circular recess lost its body authority")
    vertices = tuple(
        PassageSectionVertex(
            segment.start,
            0.0 if segment.kind == "line" else round(math.tan(cast(float, segment.sweep) / 4), 12),
        )
        for segment in record.section.segments
    ) + (PassageSectionVertex(record.section.segments[-1].end, 0.0),)
    return SectionRecess(
        index,
        owner.ordinal,
        _principal_open_geometry(
            axis=record.axis,
            run_interval=record.run_interval,
            open_sign=record.open_sign,
            boundary=vertices,
        ),
        SectionRecessClassification("edge_open_recess", "obround"),
        SectionRecessEvidence(
            tuple(sorted(node.index for node in defining)),
            tuple(sorted(node.index for node in constituent)),
        ),
    )


def _unique_section_recesses(records: Iterable[SectionRecess]) -> tuple[SectionRecess, ...]:
    """Prefer the first truthful projection of one body-local physical face region."""

    unique: list[SectionRecess] = []
    seen: set[tuple[int, tuple[int, ...]]] = set()
    for record in records:
        key = (record.body, record.evidence.constituent_faces)
        if key not in seen:
            seen.add(key)
            unique.append(replace(record, index=len(unique)))
    return tuple(unique)


def _accepted_region_key(
    record: object, *, context: RecognitionContext, evidence: EvidenceIndex
) -> tuple[int, tuple[int, ...]]:
    defining = evidence.defining_of(record)
    owner = context.graph.common_valid_solid(defining)
    if owner is None:
        raise ValueError("accepted section feature lost its body authority")
    return (
        owner.ordinal,
        tuple(sorted(node.index for node in evidence.constituent_of(record))),
    )


def _matching_recesses(record, recesses, context, evidence):
    defining = evidence.defining_of(record)
    constituent = evidence.constituent_of(record)
    owner = context.graph.common_valid_solid(defining)
    if owner is None:
        return ()
    indices = {node.index for node in (*defining, *constituent)}
    return tuple(
        item
        for item in recesses
        if item.body == owner.ordinal
        and indices
        and indices <= set(item.evidence.constituent_faces)
    )


def _recess_refusals(accepted, recesses, *, context, evidence):
    refusals = []
    for definition in PHYSICAL_DEFINITIONS:
        if definition.family not in RECESS_SOURCE_FAMILIES:
            continue
        for candidate in accepted.candidate_set(definition.family).candidates:
            if _matching_recesses(candidate, recesses, context, evidence):
                continue
            defining = evidence.defining_of(candidate)
            owner = context.graph.common_valid_solid(defining)
            if owner is None:
                raise ValueError("accepted recess candidate lost body authority")
            refusal = SectionRecessRefusal(
                owner.ordinal,
                "unsupported_support_geometry",
                SectionRecessEvidence(
                    tuple(sorted(node.index for node in defining)),
                    tuple(sorted(node.index for node in evidence.constituent_of(candidate))),
                ),
            )
            if refusal not in refusals:
                refusals.append(refusal)
    return tuple(refusals)


def _section_patterns(patterns, recesses, context, evidence):
    result: list[SectionRecessArray | SectionRecessGrid] = []
    for pattern in patterns:
        matches = [
            _matching_recesses(record, recesses, context, evidence) for record in pattern.pockets
        ]
        if any(len(match) != 1 for match in matches):
            continue  # a pattern cannot refer to unproved or ambiguously projected geometry
        members = tuple(match[0].index for match in matches)
        if len(set(members)) != len(members):
            continue
        if isinstance(pattern, PocketArray):
            result.append(SectionRecessArray(members, pattern.pitch, pattern.direction))
        else:
            u, v = plane_axes(pattern.pockets[0].depth_axis)
            cosine, sine = (
                math.cos(math.radians(pattern.angle)),
                math.sin(math.radians(pattern.angle)),
            )
            col_direction = cast(
                tuple[float, float, float],
                tuple(cosine * a + sine * b for a, b in zip(u, v, strict=True)),
            )
            row_direction = cast(
                tuple[float, float, float],
                tuple(-sine * a + cosine * b for a, b in zip(u, v, strict=True)),
            )
            result.append(
                SectionRecessGrid(
                    members,
                    pattern.rows,
                    pattern.cols,
                    pattern.row_pitch,
                    pattern.col_pitch,
                    row_direction,
                    col_direction,
                    pattern.center,
                )
            )
    return tuple(result)


def _project_result(
    context: RecognitionContext,
    accepted: CandidateInventory,
    derived: DerivedInventory,
    evidence: EvidenceIndex,
) -> _LegacyRecognitionResult:
    """Project accepted and derived inventories without discovery or reconciliation."""

    z_cyls, cross_cyls = context.cylinders
    passage_definition = next(
        definition for definition in PHYSICAL_DEFINITIONS if definition.family is FamilyId.PASSAGES
    )
    native_section_recesses = tuple(_records(accepted, FamilyId.SECTION_RECESSES, SectionRecess))
    native_regions = {
        (record.body, record.evidence.constituent_faces) for record in native_section_recesses
    }
    uncovered_prismatic = tuple(
        record
        for record in _records(accepted, FamilyId.PRISMATIC_POCKETS, PrismaticPocket)
        if _accepted_region_key(record, context=context, evidence=evidence) not in native_regions
    )
    prismatic_recesses = tuple(
        _prismatic_pocket_recess(
            record,
            context=context,
            evidence=evidence,
            index=index,
        )
        for index, record in enumerate(uncovered_prismatic)
    )
    passage_recesses = tuple(
        _section_passage_recess(
            record,
            context=context,
            evidence=evidence,
            index=index,
        )
        for index, record in enumerate(_records(accepted, FamilyId.PASSAGES, SectionPassage))
    )
    open_prismatic_recesses = tuple(
        _edge_open_prismatic_recess(
            record,
            context=context,
            evidence=evidence,
            index=index,
        )
        for index, record in enumerate(
            _records(
                accepted,
                FamilyId.EDGE_OPEN_PRISMATIC_RECESSES,
                EdgeOpenPrismaticRecess,
            )
        )
    )
    open_circular_recesses = tuple(
        _edge_open_circular_recess(
            record,
            context=context,
            evidence=evidence,
            index=index,
        )
        for index, record in enumerate(
            _records(
                accepted,
                FamilyId.EDGE_OPEN_CIRCULAR_POCKETS,
                EdgeOpenCircularPocket,
            )
        )
    )
    rectangular_blind_recesses = tuple(
        _rectangular_blind_slot_recess(record, context=context, evidence=evidence, index=index)
        for index, record in enumerate(
            _records(accepted, FamilyId.RECTANGULAR_BLIND_SLOTS, RectangularBlindSlot)
        )
    )
    round_bottom_recesses = tuple(
        _round_bottom_blind_slot_recess(record, context=context, evidence=evidence, index=index)
        for index, record in enumerate(
            _records(accepted, FamilyId.ROUND_BOTTOM_BLIND_SLOTS, RoundBottomBlindSlot)
        )
    )
    legacy_candidates: tuple[Pocket | Channel, ...] = (
        *_records(accepted, FamilyId.POCKETS, Pocket),
        *_records(accepted, FamilyId.CHANNELS, Channel),
    )
    corner_recesses = tuple(
        projected
        for index, record in enumerate(legacy_candidates)
        if (
            projected := _corner_pocket_recess(
                record, context=context, evidence=evidence, index=index
            )
        )
        is not None
    )
    section_recesses = _unique_section_recesses(
        (
            *native_section_recesses,
            *prismatic_recesses,
            *passage_recesses,
            *open_prismatic_recesses,
            *open_circular_recesses,
            *rectangular_blind_recesses,
            *round_bottom_recesses,
            *corner_recesses,
        )
    )
    refusals = _recess_refusals(accepted, section_recesses, context=context, evidence=evidence)
    patterns = _section_patterns(derived.pocket_patterns, section_recesses, context, evidence)
    return _LegacyRecognitionResult(
        cylinders=(tuple(z_cyls), tuple(cross_cyls)),
        countersinks=tuple(_records(accepted, FamilyId.COUNTERSINKS, CounterSink)),
        holes=tuple(_records(accepted, FamilyId.HOLES, HoleRecord)),
        double_d_bores=tuple(_records(accepted, FamilyId.DOUBLE_D_BORES, DoubleDBore)),
        hole_patterns=derived.hole_patterns,
        bosses=tuple(_records(accepted, FamilyId.BOSSES, BossRecord)),
        polygonal_bosses=tuple(_records(accepted, FamilyId.POLYGONAL_BOSSES, PolygonalBoss)),
        polygonal_stock=tuple(_records(accepted, FamilyId.POLYGONAL_STOCK, PolygonalStock)),
        channels=tuple(_records(accepted, FamilyId.CHANNELS, Channel)),
        slots=tuple(_records(accepted, FamilyId.SLOTS, Slot)),
        oriented_slots=tuple(_records(accepted, FamilyId.ORIENTED_SLOTS, OrientedSlot)),
        slot_patterns=derived.slot_patterns,
        oriented_slot_patterns=derived.oriented_slot_patterns,
        rectangular_blind_slots=tuple(
            _records(
                accepted,
                FamilyId.RECTANGULAR_BLIND_SLOTS,
                RectangularBlindSlot,
            )
        ),
        round_bottom_blind_slots=tuple(
            _records(
                accepted,
                FamilyId.ROUND_BOTTOM_BLIND_SLOTS,
                RoundBottomBlindSlot,
            )
        ),
        grooves=tuple(_records(accepted, FamilyId.GROOVES, Groove)),
        flats=tuple(_records(accepted, FamilyId.FLATS, Flat)),
        section_recesses=section_recesses,
        section_recess_refusals=refusals,
        section_recess_patterns=patterns,
        pockets=tuple(_records(accepted, FamilyId.POCKETS, Pocket)),
        prismatic_pockets=tuple(_records(accepted, FamilyId.PRISMATIC_POCKETS, PrismaticPocket)),
        edge_open_circular_pockets=tuple(
            _records(
                accepted,
                FamilyId.EDGE_OPEN_CIRCULAR_POCKETS,
                EdgeOpenCircularPocket,
            )
        ),
        edge_open_prismatic_recesses=tuple(
            _records(
                accepted,
                FamilyId.EDGE_OPEN_PRISMATIC_RECESSES,
                EdgeOpenPrismaticRecess,
            )
        ),
        pocket_patterns=derived.pocket_patterns,
        pads=tuple(_records(accepted, FamilyId.PADS, RaisedPad)),
        repeating_radial_profiles=tuple(
            _records(
                accepted,
                FamilyId.REPEATING_RADIAL_PROFILES,
                RepeatingRadialProfile,
            )
        ),
        turned_steps=tuple(_records(accepted, FamilyId.TURNED_STEPS, TurnedStep)),
        rotational=context.rotational,
        step_levels=tuple(_records(accepted, FamilyId.STEP_LEVELS, FaceLevel)),
        risers=tuple(_records(accepted, FamilyId.RISERS, RiserEvidence)),
        chamfers=tuple(_records(accepted, FamilyId.CHAMFERS, Chamfer)),
        angled_steps=tuple(_records(accepted, FamilyId.ANGLED_STEPS, AngledStep)),
        paired_ramp_steps=tuple(_records(accepted, FamilyId.PAIRED_RAMP_STEPS, PairedRampStep)),
        through_steps=tuple(_records(accepted, FamilyId.THROUGH_STEPS, ThroughStep)),
        circular_blind_steps=tuple(
            _records(accepted, FamilyId.CIRCULAR_BLIND_STEPS, CircularBlindStep)
        ),
        section_passages=(
            tuple(_records(accepted, FamilyId.PASSAGES, SectionPassage))
            if passage_definition.projected(context)
            else ()
        ),
        passages=derived.passages if passage_definition.projected(context) else (),
        blends=tuple(_records(accepted, FamilyId.BLENDS, Blend)),
        fillets=tuple(_records(accepted, FamilyId.FILLETS, Fillet)),
        plates=tuple(_records(accepted, FamilyId.PLATES, Plate)),
    )
