# Rust API evolution

Before 1.0, a breaking Rust API change requires a minor release. The next release
is **0.5.0**: adding `#[non_exhaustive]` itself breaks struct literals, exhaustive
struct patterns and exhaustive enum matches. Versions are updated by the release
process. Patch releases preserve source compatibility, including default features.

## Extensible types

The following public records, documents, reports, errors and defaultable options
are non-exhaustive. Read their public fields normally, destructure with `..`, and
include a fallback in matches on extensible enums. New fields must preserve the
existing constructors; new enum variants must preserve existing variant shapes.
Renaming/removing fields or variants and changing field types still requires a
minor release. A non-exhaustive annotation is not a general semver exemption.

- `haecceity::anchor`: `AnchorRefusal`, `FaceAnchor`.
- `haecceity::brep_write`: `WriteOptions`, `PmiCarried`, `PmiRefusal`, `WriteError`, `WriteReport`.
- `haecceity::express`: `Violation`.
- `haecceity::express_rules`: `RuleViolation`.
- `haecceity::gvp`: `ValidationProperty`.
- `haecceity::json::presentation`: `Graphic`, `Presentation`, `TextPlacement`, `PolylineRole`.
- `haecceity::presentation`: `Callout`.
- `haecceity::mesh`: `FaceMesh`, `MeshRefusal`.
- `haecceity::overlap`: `OverlapRefusal`.
- `haecceity::p21`: `P21Error`.
- `haecceity::pmi`: `Accounting`, `CalloutLink`, `Finding`, `FindingKind`, `GraphicalCallout`, `Identifiers`, `PartIdentifiers`, `PartProvenance`, `PmiRead`, `Provenance`, `ReadError`, `SavedView`.
- `haecceity::pmi::verify`: `Problem`, `Snapshot`, `Verification`.
- `haecceity::pmi::write`: `EditionRewrite`, `GeometryCarrierExemption`, `LongStringWarning`, `Refusal`, `UpgradeRepair`, `UpgradeRepairKind`, `WriteError`, `WriteReport`.
- `haecceity::removal`: `Blocker`, `Emptied`, `RemovalPlan`, `RemovalRefusal`, `RemovalReport`.
- `haecceity::step`: `Assembly`, `Component`, `PartDefinition`, `PartPlacement`, `StepError`, `StepFile`.
- `haecceity::step::manifest`: `Common`, `Instance`, `Manifest`, `Product`, `Ranges`, `ReaderVersion`, `Refusal`, `ScanOptions`, `Source`, `Unowned`.
- `quiddity::correspondence`: `Change`, `Entry`, `FaceFingerprint`, `FeatureFingerprint`, `Recognition`, `RecognitionWithEvidence`, `Thresholds`.
- `quiddity::evidence_view`: `EvidenceCandidate`, `EvidenceDocument`, `EvidenceFeature`, `EvidenceRecord`.
- `quiddity::features::angled_steps`: `AngledStep`.
- `quiddity::features::annular_grooves`: `AnnularGroove`.
- `quiddity::features::bevel`: `BevelReject`.
- `quiddity::features::blends`: `Blend`, `BlendPath`, `CircularBlendPath`, `StraightBlendPath`.
- `quiddity::features::bosses`: `BossRecord`.
- `quiddity::features::chamfers`: `Chamfer`, `ChamferOptions`.
- `quiddity::features::circular_blind_steps`: `CircularBlindStep`.
- `quiddity::features::circular_face_patterns`: `CircularFacePattern`.
- `quiddity::features::countersinks`: `CounterSink`.
- `quiddity::features::cylindrical_end_surface`: `CylindricalEndSurface`.
- `quiddity::features::cylindrical_profiles`: `CylindricalProfile`.
- `quiddity::features::discovery`: `DiscoveryRefusal`.
- `quiddity::features::edge_open_circular`: `EdgeOpenCircularPocket`, `OpenCircularSection`, `OpenCircularSectionSegment`.
- `quiddity::features::edge_open_prismatic`: `EdgeOpenPrismaticRecess`, `OpenPolygonalSection`, `OpenSectionOpening`.
- `quiddity::features::effective_surfaces`: `MaterialSideRefusalReason`, `OrientedRefusal`, `SurfaceRefusalReason`.
- `quiddity::features::evidence`: `EvidenceError`, `EvidenceStatus`, `Occurrence`, `RunMode`.
- `quiddity::features::face_grooves`: `FaceGroove`.
- `quiddity::features::fillets`: `Fillet`, `FilletOptions`.
- `quiddity::features::flats`: `Flat`.
- `quiddity::features::freeform_surfaces`: `BSplineSurfaceSupport`, `FreeformSurface`, `SurfaceContinuityLink`.
- `quiddity::features::grooves`: `Groove`.
- `quiddity::features::gussets`: `GussetRib`, `GussetRibPattern`.
- `quiddity::features::helical_springs`: `HelicalSpring`.
- `quiddity::features::hole_patterns`: `HolePattern`.
- `quiddity::features::holes`: `CounterBore`, `ExitObstruction`, `HoleOptions`, `HoleRecord`, `MouthChamfer`.
- `quiddity::features::interior_voids`: `InteriorVoid`, `InteriorVoidRefusal`.
- `quiddity::features::internal_bevels`: `InternalBevel`.
- `quiddity::features::levels`: `FaceLevel`, `FaceLevelOptions`, `RiserEvidence`, `RiserOptions`.
- `quiddity::features::local_steps`: `LocalStep`, `LocalStepEnd`.
- `quiddity::features`: `FamilyFailure`, `Features`, `Inventory`.
- `quiddity::features::modelled_threads`: `ModelledThread`, `ModelledThreadRefusal`, `ThreadAxis`.
- `quiddity::features::oblique_through_steps`: `ObliqueThroughStep`.
- `quiddity::features::oriented_chamfers`: `OrientedChamfer`, `OrientedChamferOptions`.
- `quiddity::features::oriented_slots`: `OrientedSlot`, `OrientedSlotPattern`.
- `quiddity::features::outer_profile`: `OuterProfileRefusalReason`, `PlanarOuterProfile`, `ProfileArc`, `ProfileLine`, `ProfileSupport`.
- `quiddity::features::pads`: `PadError`, `PadOptions`, `RaisedPad`.
- `quiddity::features::paired_ramp_steps`: `PairedRampStep`.
- `quiddity::features::passages`: `Passage`, `PassageEnds`, `PassageFrame`, `PassageSection`, `PassageSectionVertex`, `SectionPassage`.
- `quiddity::features::plates`: `Plate`, `PlateOptions`.
- `quiddity::features::polygonal_bosses`: `PolygonalOptions`, `PolygonalPrism`.
- `quiddity::features::polygonal_oblique_through_steps`: `PolygonalObliqueThroughStep`.
- `quiddity::features::prismatic_pockets`: `PrismaticPocket`.
- `quiddity::features::prismatic_through_cuts`: `PrismaticThroughCut`.
- `quiddity::features::profiled_bores`: `DoubleDBore`, `DoubleDBoreOptions`.
- `quiddity::features::recess_patterns`: `PocketPattern`, `SlotPattern`.
- `quiddity::features::recess_records`: `Channel`, `FloorFillet`, `Pocket`, `Slot`.
- `quiddity::features::reconcile`: `Candidate`, `Disposition`, `Family`, `ReasonCode`.
- `quiddity::features::rectangular_blind_slots`: `RectangularBlindSlot`.
- `quiddity::features::repeating_profiles`: `RepeatingRadialProfile`, `RepeatingRadialProfileOptions`.
- `quiddity::features::round_bottom_slots`: `RoundBottomBlindSlot`.
- `quiddity::features::section_recess`: `ClosedSectionProfile`, `EndSurface`, `OpenSectionProfile`, `PlanarEndSurface`, `PlanarEndTerm`, `PlanarEnvelopeEndSurface`, `SectionEnd`, `SectionProfile`, `SectionRecess`, `SectionRecessArray`, `SectionRecessBodyRef`, `SectionRecessClassification`, `SectionRecessDocument`, `SectionRecessEnds`, `SectionRecessEvidence`, `SectionRecessFaceRef`, `SectionRecessGeometry`, `SectionRecessGrid`, `SectionRecessPattern`, `SectionRecessRefusal`.
- `quiddity::features::section_recess_family`: `SectionRecessProjection`.
- `quiddity::features::sections`: `LocalFrame`, `OccurrenceGeometry`, `SectionEnds`, `SectionValue`, `VertexValue`.
- `quiddity::features::semicircular_blind_slots`: `SemicircularBlindSlot`.
- `quiddity::features::sheet_metal`: `FlatOverlapWitness`, `FlatPatternPlan`, `FormedSheetFeature`, `SheetBend`, `SheetEdgeTreatment`, `SheetFlange`, `SheetMetalBody`, `SheetMetalOptions`, `UnfoldedBendStrip`, `UnfoldedFlangeFace`.
- `quiddity::features::thin_walls`: `ShellHistoryHint`, `ThinWallBody`, `UnpairedWallFace`, `WallFacePair`.
- `quiddity::features::through_notches`: `RectangularThroughNotch`.
- `quiddity::features::through_steps`: `ThroughStep`.
- `quiddity::features::turned`: `TurnedProfileKey`.
- `quiddity::features::turned_bodies`: `Bore`, `ProfileSegment`, `TurnedBody`.
- `quiddity::features::turned_steps`: `TurnedStep`.
- `quiddity::features::v_channels`: `VChannel`.
- `quiddity::frames`: `FrameRefusal`, `FramedError`, `FramedRecognition`, `PartFrame`.
- `quiddity::published_records`: `PublishedFeatures`.
- `quiddity::recognition`: `BodyReason`, `FaceAccounting`, `FaceRefusal`, `RecognitionDocument`, `Record`.
- `quiddity::serve`: `Failure`, `RecognitionOutput`.

Types re-exported at the crate root retain those names too: for example,
`quiddity::Fingerprints` is `quiddity::correspondence::Fingerprints`.

## Construction and migration

Use the reader/recogniser for output-only records. For authored values, use:

- `Record::new(id, family, record_type, faces, parameters)`: starts unproved.
- `RecognitionOutput::new(recognition, document)` for an owned service result.
- `Fingerprints::new(scale)`: current fingerprint version, empty features/faces.
- `Occurrence::new(record, defining, context)` for caller-supplied evidence.
- `FamilyFailure::panicked(family, stage, message)` for a reported panic.
- `Finding::new(kind, part, ids, entity, detail)` for a diagnostic.
- `GraphicalCallout::new(id, name, links)` and
  `CalloutLink::new(association, definition, targets, entity)` for callout evidence.
- `Snapshot::new(doc, defs, read)` for PMI verification inputs.
- `WriteOptions::new(name).with_pmi(pmi)` for B-rep writes.
- `Assembly::new(product_definition, name, components)` and
  `Component::new(occurrence, definition)` for assembly descriptions.
- `EditionRewrite::new(id, from, to)`, `Blocker::new(id, entity, family, references)`
  and `Emptied::new(id, entity, attribute, lower)` for expected report values.
- Manifest `Refusal::new(products, entity, reason)` for an explicit refusal.
- `ScanOptions::default()` also offers `with_chunk_size`, `with_index_dir`,
  `with_no_product_hashes` and `with_instance_limit` builders.
- `Graphic::new(source, name)` for an initially empty presentation graphic.
- `TextPlacement::new(origin, x_axis, normal)` for a caller-supplied text frame;
  set optional public metadata afterward. `presentation::Callout::default()`
  creates empty annotation geometry.
- `Default::default()` followed by assignment for defaultable options and
  collections, including `Features`, `PublishedFeatures`, `Presentation`,
  `PartIdentifiers` and report types with defaults.

For example, an options literal becomes:

```rust
let mut options = quiddity::HoleOptions::default();
options.with_countersinks = true;
```

An exhaustive error match should preserve its existing arms and add a meaningful
fallback (usually reporting the error's `Display` text). Applications that refuse
unknown evidence can return their normal refusal; tests that require a particular
closed set should fail explicitly on an unknown variant. `UpgradeRepairKind::code()`
provides its stable writer-report code without a cross-crate exhaustive match.

`TextPlacement`, `presentation::Callout`, `PolylineRole`, `EvidenceStatus` and
`FaceAccounting` have grown before and are extensible too. Unknown proof/accounting
variants must fail closed in consumers; only `EvidenceStatus::Verified` grants
verification. Rust extensibility does not loosen JSON schema validation.

## Deliberately closed values

The survey also covers haecceity's geometry and semantic PMI APIs and quiddity's
planning/input values. Fixed mathematical representations (`V3`, `Bounds`, `Frame`,
`Curve`, `Surface`, `Arc`, `Pcurve`, HLR `Exact`), topology construction values
(`Edge`, `Loop`, `Face`, `Solid`, `Source`), identifier/quantity wrappers, EXPRESS
syntax values, PMI model values and standard-defined enums remain constructible
and exhaustive. Their fields and variants are a source compatibility commitment:
adding one requires a minor release. This also covers correspondence `Class`, the
stored and compared values `Fingerprints`, `Correspondence` and `Alignment` (Specify
builds and persists them as plain values), and
coordinate representations (`RecordFrame`, `FrameGauge`, `FilletAxis`, `ProfileAxis`, `Grid`).
Input tolerances without defaults remain explicit closed values; no tolerance or
recognition policy changed in this migration. Opaque objects with private state
already require their existing factory functions.

Rust source compatibility is separate from JSON/STEP compatibility. These
annotations do not change serialized data or grant permission to change a wire
schema. Keep the existing schema and fingerprint version policies.

## Automated checks

`.github/check_semver.py` uses pinned cargo-semver-checks 0.51.0, the latest stable
crates.io version of each published crate, and the default/JSON API
configuration. The lint job separately compiles supported feature-off builds.
Every run checks patch compatibility. Only a non-empty Unreleased
`### Breaking (Rust API)` section with an explicit `Pending version: X.Y.Z`
(at least the next minor before 1.0) sanctions detected breaks. Historical or
empty sections do not qualify; tool failures always fail. See CONTRIBUTING.
The `f3d` conversion dependency is
excluded from this check to keep its cost bounded; its API changes remain subject
to the same release rule. CI separately audits Cargo.lock with cargo-audit 0.22.2.
