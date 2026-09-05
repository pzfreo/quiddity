# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Deterministic geometry-only feature recognition for build123d solids.

This Apache-2.0 package is the single recognition implementation shared by consumers. Import the
public surface from here, not the implementation submodules.

Recogniser contract (ADR 0002)
------------------------------
A *feature* recogniser takes one of two shapes:

- **Part-based** — ``recognise_<feature>(part, *, <tuning / injected deps>) -> list[record]``
  (``recognise_holes(part, *, cyls=None)``, ``recognise_chamfers(part, *, tol=...)``,
  ``recognise_risers(part, *, tol=...)``). Everything after ``part`` is
  **keyword-only** — both tuning and any injected inventory. A recogniser **never
  re-recognises a dependency internally**; the orchestration caller owns the single inventory
  and threads it.
- **Derived** — ``recognise_<feature>(inventory) -> list[record]``
  (``recognise_hole_patterns(holes)``):
  operates purely on another recogniser's records, no ``part`` and no tuning, so the
  single inventory arg is unambiguous and stays positional.

Common to both: a **British** ``recognise_`` verb (not ``find_``/``analyse_``); a
**deterministic ``list`` of frozen-dataclass records** (empty when absent — never
``Optional``-singular, never a bare ``list`` of primitives); **geometry-only records** (no
build123d types leak out; consumers adapt these values into their own domain models).

The contract holds for **every** recogniser, including the two that once strained it —
their records were simply the wrong shape:

- ``recognise_face_levels -> list[FaceLevel]`` (was ``list[float]``) — a level is now a
  ``FaceLevel(z)`` record.
- ``recognise_turned_steps -> list[TurnedStep]`` (was ``TurnedProfile | None``) — each
  ``TurnedStep`` now carries its ``axis``, so it is a self-contained record and the old
  ``TurnedProfile`` wrapper is no longer the return. ``TurnedProfile`` survives only as a
  **pipeline aggregate** (``TurnedProfile.from_steps``) for consumers that want axis +
  shoulders as a unit — it is not a recogniser return.

Record class names avoid consumer-domain ``Feature`` types: for example, the public records are
``HoleRecord`` and ``BossRecord`` rather than drawing or manufacturing IR types.

``analyse_cylinders`` / ``full_cylinders`` / ``feature_diameters`` are **not** recognisers
under this contract — they are cylinder-analysis *substrate* (a tuple of dicts / a diameter
query), and deliberately keep their names. Likewise the **shared single-face reads**
(``classify_bevel``/``BevelReject``, ``fillet_anchor``, ``cone_rims``,
``floor_face_anchor``, ``step_level_zs``): helpers shared with the declared
front-end, not recognisers — they traffic in build123d/OCP objects and are exposed only where
existing downstream compatibility requires them. New declared-feature consumers should use the
reviewed five-operation roster in :mod:`quiddity.inspection`; the broader helper set is
not implied to be supported inspection API.

``FaceEdges`` is neither a recogniser nor substrate but a **shared memo**: pass one instance to
several recognisers running over the same part and they stop re-deriving each face's edges,
which is about a fifth of a full census. It is exported only because it appears in those
recognisers' signatures — a ``face_edges=`` parameter no public API could construct would be
useless. Omit it and every recogniser behaves exactly as before.

``project_step_shoulders`` is likewise not a recogniser but its mirror image: a **pure
projection** over :func:`recognise_risers`' records, with no ``part`` and no geometry access
at all. It carries the ``project_`` verb rather than ``recognise_`` because the aggregate owns
the evidence and each consumer projects. A function that cannot look at a solid cannot become a
second recognition site.
"""

from __future__ import annotations

from importlib.metadata import PackageNotFoundError, version

from quiddity._adjacency import FaceEdges
from quiddity._bevel import BevelReject, classify_bevel
from quiddity._features import (
    BoltCircle,
    BossRecord,
    CounterBore,
    HoleRecord,
    HoleSpec,
    LinearArray,
    RectGrid,
    analyse_cylinders,
    feature_diameters,
    full_cylinders,
    recognise_bosses,
    recognise_hole_patterns,
    recognise_holes,
)
from quiddity.angled_steps import (
    AngledStep,
    recognise_angled_steps,
)
from quiddity.blends import (
    Blend,
    CircularBlendPath,
    StraightBlendPath,
    recognise_blends,
)
from quiddity.chamfers import Chamfer, recognise_chamfers
from quiddity.circular_blind_steps import (
    CircularBlindStep,
    recognise_circular_blind_steps,
)
from quiddity.countersinks import (
    CounterSink,
    cone_rims,
    countersink_matches_hole,
    recognise_countersinks,
)
from quiddity.explanations import (
    DispositionExplanation,
    ExplanationCoverage,
    FamilyEvaluation,
    FamilyExplanation,
    RecognitionDiagnostic,
    RecognitionDiagnosticCode,
    RecognitionDiagnosticStatus,
    RecognitionOutcome,
    RecognitionReport,
    ReconciliationReason,
    build_raw_recognition_report,
    build_recognition_report,
)
from quiddity.fillets import Fillet, fillet_anchor, recognise_fillets
from quiddity.flats import Flat, recognise_flats
from quiddity.frames import (
    FramedEvidence,
    FramedEvidenceRefusalReason,
    FramedPreparation,
    FramedRecognition,
    FramedRecognitionEvidence,
    FramedRecognitionReport,
    FramedRecognitionResult,
    FramedReport,
    FrameGauge,
    FrameInference,
    FrameRefusalReason,
    PartFrame,
    PreparedFramedPart,
    RefusedFramedEvidence,
    RefusedPartFrame,
    build_framed_recognition_evidence,
    build_framed_recognition_report,
    build_framed_recognition_result,
    infer_part_frame,
    prepare_framed_part,
)
from quiddity.grooves import Groove, floor_face_anchor, recognise_grooves
from quiddity.levels import (
    STEP_LADDER_BOUNDARY_MARGIN,
    FaceLevel,
    RiserEvidence,
    StepShoulder,
    project_step_shoulders,
    recognise_face_levels,
    recognise_risers,
    step_level_records,
    step_level_zs,
)
from quiddity.oriented_slots import (
    OrientedSlot,
    OrientedSlotArray,
    OrientedSlotGrid,
    recognise_oriented_slot_patterns,
    recognise_oriented_slots,
)
from quiddity.pads import RaisedPad, recognise_rectangular_pads
from quiddity.paired_ramp_steps import PairedRampStep, recognise_paired_ramp_steps
from quiddity.passages import (
    PassageFrame,
    PassageSection,
    PassageSectionVertex,
)
from quiddity.plates import Plate, has_multi_axis_plates, recognise_plates
from quiddity.polygonal_bosses import (
    PolygonalBoss,
    PolygonalStock,
    recognise_polygonal_bosses,
    recognise_polygonal_stock,
)
from quiddity.profiled_bores import DoubleDBore, recognise_double_d_bores
from quiddity.repeating_profiles import (
    RepeatingRadialProfile,
    recognise_repeating_radial_profiles,
)
from quiddity.result import (
    RecognitionResult,
    build_raw_recognition_result,
    build_recognition_result,
)
from quiddity.section_recesses import (
    ClosedSectionProfile,
    OpenSectionProfile,
    SectionEnd,
    SectionRecess,
    SectionRecessArray,
    SectionRecessBodyRef,
    SectionRecessClassification,
    SectionRecessDocument,
    SectionRecessEnds,
    SectionRecessEvidence,
    SectionRecessFaceRef,
    SectionRecessGeometry,
    SectionRecessGrid,
    SectionRecessRefusal,
    build_section_recess_document,
    recognise_section_recesses,
)
from quiddity.slots import (
    Slot,
    SlotArray,
    SlotGrid,
    recognise_slot_patterns,
    recognise_slots,
)
from quiddity.step_io import import_step_geometry
from quiddity.through_steps import ThroughStep, recognise_through_steps
from quiddity.turned import (
    TurnedProfile,
    TurnedProfileKey,
    TurnedStep,
    recognise_turned_steps,
)

try:
    __version__ = version("quiddity")
except PackageNotFoundError:  # pragma: no cover - only a bare, uninstalled source tree
    __version__ = "0.2.1"

# Imported after the recognition surface because census consumes that public orchestration.
from quiddity.capabilities import (  # noqa: E402
    CAPABILITY_FORMAT,
    CAPABILITY_FORMAT_VERSION,
    CapabilityManifestError,
    capability_manifest,
    capability_manifest_json,
    validate_capability_manifest,
)
from quiddity.census import feature_census  # noqa: E402

__all__ = [
    "__version__",
    "CAPABILITY_FORMAT",
    "CAPABILITY_FORMAT_VERSION",
    "CapabilityManifestError",
    "STEP_LADDER_BOUNDARY_MARGIN",
    "AngledStep",
    "PassageFrame",
    "PassageSection",
    "PassageSectionVertex",
    "PairedRampStep",
    "SectionRecess",
    "SectionRecessClassification",
    "SectionRecessEnds",
    "SectionRecessEvidence",
    "SectionRecessGeometry",
    "SectionRecessRefusal",
    "SectionRecessArray",
    "SectionRecessGrid",
    "SectionEnd",
    "ClosedSectionProfile",
    "OrientedSlot",
    "OrientedSlotArray",
    "OrientedSlotGrid",
    "BoltCircle",
    "Blend",
    "CircularBlendPath",
    "Chamfer",
    "Fillet",
    "Flat",
    "FrameGauge",
    "FrameInference",
    "FrameRefusalReason",
    "FramedRecognition",
    "FramedEvidence",
    "FramedEvidenceRefusalReason",
    "FramedRecognitionEvidence",
    "FramedRecognitionResult",
    "FramedRecognitionReport",
    "FramedReport",
    "FramedPreparation",
    "RefusedFramedEvidence",
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
    "Groove",
    "BossRecord",
    "CounterBore",
    "CounterSink",
    "countersink_matches_hole",
    "DoubleDBore",
    "FaceLevel",
    "HoleRecord",
    "HoleSpec",
    "LinearArray",
    "Plate",
    "PartFrame",
    "PreparedFramedPart",
    "PolygonalBoss",
    "PolygonalStock",
    "RaisedPad",
    "RepeatingRadialProfile",
    "RectGrid",
    "Slot",
    "SlotArray",
    "SlotGrid",
    "RiserEvidence",
    "StepShoulder",
    "StraightBlendPath",
    "TurnedProfile",
    "TurnedProfileKey",
    "TurnedStep",
    "RecognitionResult",
    "RefusedPartFrame",
    "BevelReject",
    "FaceEdges",
    "analyse_cylinders",
    "classify_bevel",
    "cone_rims",
    "fillet_anchor",
    "floor_face_anchor",
    "has_multi_axis_plates",
    "project_step_shoulders",
    "recognise_face_levels",
    "recognise_risers",
    "step_level_records",
    "step_level_zs",
    "feature_diameters",
    "feature_census",
    "import_step_geometry",
    "capability_manifest",
    "capability_manifest_json",
    "validate_capability_manifest",
    "recognise_angled_steps",
    "CircularBlindStep",
    "recognise_circular_blind_steps",
    "recognise_paired_ramp_steps",
    "ThroughStep",
    "recognise_through_steps",
    "recognise_section_recesses",
    "OpenSectionProfile",
    "build_section_recess_document",
    "SectionRecessBodyRef",
    "SectionRecessDocument",
    "SectionRecessFaceRef",
    "recognise_oriented_slots",
    "recognise_oriented_slot_patterns",
    "recognise_bosses",
    "recognise_blends",
    "recognise_chamfers",
    "recognise_fillets",
    "recognise_flats",
    "recognise_grooves",
    "recognise_countersinks",
    "recognise_double_d_bores",
    "recognise_hole_patterns",
    "recognise_holes",
    "recognise_plates",
    "recognise_polygonal_bosses",
    "recognise_polygonal_stock",
    "recognise_rectangular_pads",
    "recognise_repeating_radial_profiles",
    "recognise_slot_patterns",
    "recognise_slots",
    "recognise_turned_steps",
    "build_recognition_result",
    "build_raw_recognition_result",
    "build_framed_recognition_result",
    "build_framed_recognition_evidence",
    "build_framed_recognition_report",
    "build_recognition_report",
    "build_raw_recognition_report",
    "full_cylinders",
    "infer_part_frame",
    "prepare_framed_part",
]
