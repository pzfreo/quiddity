# Release notes

## Next release

- Raw, framed and prepared evidence views now expose `report`: bounded explanations from
  the same recognition run, sharing the exact `result`. No extra inventory is needed (#494).

- Framed evidence mapping refusals now retain an optional completed framed `result` when
  inventory already ran. Early refusals carry `None`; callers can reuse late results without
  rerunning recognition. No unproved face mapping or partial evidence is exposed (#493).

- Preserve all remaining support fragments when build123d boolean subtraction returns a
  ShapeList. This fixes the public SectionRecess crash on build123d 0.10 without weakening
  the complete-support proof. Focused compatibility CI pins build123d 0.9, 0.10 and 0.11.

- Add `quiddity part.step [-o recognition.json]`, `--capabilities` and `--version`.
  The shared `quiddity.document.build_recognition_document` projects one framed evidence run
  into JSON with unchanged feature records, explicit coordinate placement, document-local face
  references, derived records and association accounting. Recognition algorithms are unchanged.

## 0.2.0 — Quiddity second alpha

Quiddity starts its own version history at 0.2.0. The earlier `b123d-recognisers`
release line below records the first alpha; those versions and artifacts are unchanged.

- Rename the distribution to `quiddity`, the Python namespace to `quiddity`, and the
  capability CLI to `quiddity-capabilities`. There is no old-import compatibility alias.
  Rename public manifest identifiers to `quiddity-capabilities`, `quiddity-evidence-api`
  and `quiddity-inspection-api`; recognition behavior and geometry schemas are unchanged. See
  [the Quiddity migration guide](docs/quiddity-migration.md).

## 0.4.15

**Breaking API change despite the patch version.** Published as a patch at the maintainer's
request, this release makes an explicit exception to the earlier promise to preserve the
specialised recess APIs throughout 0.4.x. Consumers must follow the
[migration guide](docs/section-recess-migration.md), or pin 0.4.14 until migrated.

- Reject obround pocket claims obstructed by same-body material, including suspended bridges
  and obstructions inside curved ends. Exact semicircular probes now verify the empty run,
  open mouth and complete floor backing before unified geometry is published.

- **Unified recess provider cutover:** specialised pocket, channel, recess and passage root
  exports/result fields are replaced by `SectionRecess`; JSON schema 2 adds occurrence-indexed
  patterns and explicit face-referenced geometry refusals. The public census counts unified recesses
  once. Private detector benchmark identities remain unchanged. See the migration guide below.

- Prove both remaining authored partial-support channels from actual source patches, empty runs,
  lateral openings and two open ends. Unsupported extent summaries retain evidence as refusals,
  never fabricated geometry.

- Project proved corner notches as two physical wall segments with an explicit gap and one capped
  run end in `SectionRecess`. The independent source-face proof retains body/face ownership and
  refuses non-rectangular, perforated or doubly capped anatomy. No bounding-box closure is invented.
  Material probes additionally refuse suspended and mouth-only obstructions within the owning body.

- Preserve legacy polygon profiles on their publication grid, including collapsed micro-chamfer
  edges, without changing strict section validation or discarding source face evidence. Explicit
  bounded refusals distinguish genuinely unrepresentable geometry from normalization.

- Add full JSON geometry goldens for all seven unified recess proof paths and migration checks
  across the authored goldens and vendored MFCAD++ development corpus. See
  [the migration guide](docs/section-recess-migration.md) for the completed public cutover and
  explicit unsupported-geometry refusal contract.

- Generalise the unified `SectionRecess` geometry to closed and truthfully open line/arc profiles,
  pocket/edge-open/passage topology, and project accepted passages, prismatic pockets and exact
  edge-open recess families while retaining their body and face evidence.

- Add the public `build_section_recess_document(part)` JSON envelope builder. It projects accepted
  aggregate `SectionRecess` records using result-local body and face rosters; the export CLI uses
  this same path rather than running a separate detector.

- **Promoted unified constant-section recess geometry.**
  `recognise_section_recesses()` and `RecognitionResult.section_recesses` now publish the
  ADR-0019 `SectionRecess` contract through the normal evidence, inventory and capability
  surfaces. The production recogniser covers validated closed obround and intact polygonal
  pockets in arbitrary rigid presentation, with reconstructible line/arc profiles and
  result-local face/body indices. The effectiveness runner scores these physical records through
  dataset taxonomy without an experimental overlay.

## 0.4.14

- **Paired framed recognition with exact caller-face evidence.**
  `build_framed_recognition_evidence()` and
  `PreparedFramedPart.recognise_evidence()` return accepted occurrence, constituent and geometry
  association evidence from the same local aggregate run. Each working-face `FaceRef` can resolve
  separately to its exact caller-part topology partner under the published rigid placement.
  Missing or non-bijective mapping refuses explicitly; there is no raw rerun, coordinate-proximity
  matching, face-order fallback or persistent ID.

- **Reported associated and unassociated geometry explicitly.**
  `RecognitionEvidence.association` provides overlap-safe face-count and surface-area partitions,
  per-family contributions and exact run-local references for unassociated faces. These values
  describe evidence coverage, not recognition accuracy or residual-geometry classification.

- **Release-level exception.**
  ADR 0005 ordinarily reserves additive public API for a minor release. The project owner
  explicitly directed these evidence additions to ship as patch release 0.4.14 so Draftwright can
  consume them. Existing result/report and raw-evidence behavior remains compatible; consumers
  must still adopt the new API deliberately.

## 0.4.13

- **Recovered interrupted passages and treated-mouth prismatic pockets.**
  Passage recognition gained two-ended enclosure traversal, edge-incidence mouth recovery and
  nonparallel planar termination while preserving exact constituent evidence and fail-closed
  ownership. Prismatic pockets can recover a bounded polygonal cavity through a partial chamfer or
  blend treatment when the direct wall cycle, unique floor and material/void proof are complete.

- **Expanded truthful blend and chamfer coverage.**
  Concave edge-blend chains and native toroidal blend paths are now public alongside the existing
  convex cylindrical chains. Geometric chamfers below the drawing-callout floor remain recognised
  as physical evidence without becoming dimension callouts.

- **Added safer STEP ingestion and hardened Passage serialization.**
  The geometry-only STEP loader avoids metadata-dependent assembly failure modes. Passage frames
  accept valid serialized values and section canonicalization now occurs after numeric rounding,
  keeping reports reproducible.

## 0.4.12

- **Added free-axis rectangular through-slot recognition.**
  `recognise_oriented_slots()` projects exact four-wall rectangular `SectionPassage`
  occurrences whose width and long directions are oblique in the supplied recognition frame.
  The immutable `OrientedSlot` record retains free direction vectors, dimensions, centre,
  source passage, body authority and original-face evidence. Linear and complete rectangular
  same-body arrays are available through `recognise_oriented_slot_patterns()` and the aggregate
  result. Principal-axis slots remain unchanged `Slot` values; square, curved, capped, tapered,
  incomplete and ownership-ambiguous interpretations remain excluded.

- **Extended paired-ramp steps to shallow nonzero ramp angles.**
  `PairedRampStep` recognition no longer inherits the unrelated Chamfer draft-angle threshold.
  Shallow mirror-symmetric ramp pairs now use the existing geometric direction tolerance while
  exact principal planes and the existing asymmetry, completeness, opening, terminal and
  ownership refusals remain unchanged.

- **Recorded the additive contract transition explicitly.**
  The capability manifest adds schema-version-1 `OrientedSlot`, `OrientedSlotArray` and
  `OrientedSlotGrid` records plus their entry points and aggregate fields. Existing principal
  `Slot` records and legacy raw/framed recognition behavior retain their prior meanings;
  consumers must explicitly adopt the new family before treating it as supported.

- **Release-level exception.**
  ADR 0005 ordinarily classifies an additive public family as a minor release. The project owner
  explicitly directed this additive contract to ship as patch release 0.4.12 instead. Consumers
  must still opt in to the new family and aggregate fields; the lower release number does not make
  unknown records safe to consume implicitly.

## 0.4.11

- **Published conservative convex rolling-ball Blend chains.**
  `recognise_blends()` and aggregate `RecognitionResult.blends` report one immutable occurrence
  for each complete, same-solid, same-radius native cylindrical convex chain, including small and
  non-principal rounds. Records retain exact defining/constituent face evidence, canonical full
  axis direction and a subdivision-invariant cylindrical anchor. Accepted dimension-worthy
  `Fillet` occurrences retain precedence only when their exact defining faces cover the complete
  chain; concave, branched, incomplete, recovered-surface and non-cylindrical blends remain outside
  the public contract.

- **Improved reproducible Round coverage without changing unrelated recognition.** On the complete
  2,500-model MFCAD++ development split, five public Blend occurrences raise class-23 defining-face
  recall from 0/13 to 5/13 and face coverage from 5/13 to 10/13. The same seven pre-existing invalid
  Hole rows remain explicit, and every per-model result outside the new Blend counter and Round
  mapping is unchanged. A rejected broad-scope audit records why 1,827 concave chains without Round
  labels were not published as duplicate top-level features.

## 0.4.10

- **Published exact original-face membership for downstream consumers.** Accepted occurrences now
  expose opaque, run-local defining and constituent face references without leaking mutable kernel
  identity. Hole/Boss terminals, Channel/Pocket floors, complete Pocket interior regions, and
  PrismaticPocket floors are available through the versioned evidence contract. Corpus reports
  score face coverage separately from defining-face recall.

- **Closed several deterministic recognition gaps.** Slot recognition now proves depth closure;
  paired-ramp steps accept geometrically unchanged subdivided terminals and ramp boundaries;
  stubby circular-end pockets tolerate imported analytic noise under the existing local tolerance;
  and AngledStep accepts a subdivided blind terminal only when its linear boundary still proves
  exactly three cyclic straight runs.

- **Corrected downstream ownership and covariance defects.** Double-D bores suppress the ordinary
  Hole candidate built from the same defining faces, external stepped-shaft cones no longer appear
  as CounterSinks, Plate thickness ties are frame-covariant, and turned-step shoulder filtering is
  measured relative to the owning shaft axis line rather than the world origin.

- **Added conservative edge-open rectangular blind-slot recognition.**
  `recognise_rectangular_blind_slots()` reports principal-axis recesses with one rectangular blind
  cap, two opposed planar sides, one planar floor, a source-envelope mouth and an empty rectangular
  removal sweep. The immutable `RectangularBlindSlot` record and aggregate output retain complete
  original-face evidence and supersede only Pocket candidates whose complete constituent evidence
  belongs to the accepted slot. Enclosed pockets, through or doubly open channels, short notches,
  non-principal, cross-solid, materially obstructed and ambiguous interpretations remain excluded.

- **Added conservative round-bottom blind-slot recognition.**
  `recognise_round_bottom_blind_slots()` reports principal-axis, edge-open recesses with one
  planar blind cap and a constant U section formed by a flat floor tangent to two equal quarter
  cylinders. The immutable `RoundBottomBlindSlot` record, aggregate result, census, capability
  manifest and run-local defining/constituent face evidence preserve occurrence and body identity.
  Through, doubly capped, rectangular, obround, non-principal, perforated, interrupted,
  cross-solid and materially obstructed sections remain excluded. On the fixed MFCAD++-500
  development set, class-19 defining recall moves from 0/90 to 72/90 at 72/72 defining precision,
  while exact face coverage moves from 9/90 to 75/90 and every pre-existing class metric and
  family count remains unchanged.

## 0.4.9

- **Kept FaceLevel occurrences body-local in compounds.** Equal-height faces on separate solids
  remain separate records with their own transverse support bounds; recognition no longer creates
  a synthetic support span across empty space.

- **Recognised and projected risers per solid.** Compound-global bounds no longer discard real
  body-local risers or introduce transitions belonging to another component. Face-level and
  shoulder projections consume the same per-solid evidence.

- **Kept turned profiles body-local.** Parallel, coaxial, disjoint, and mixed-axis shafts retain
  distinct axis lines, profile membership, and solid ownership. Aggregate publication is atomic:
  ambiguous or inconsistent evidence cannot leak a valid-looking prefix. Plate projection excludes
  only the solid owned by a completed turned profile, preserving prismatic siblings.

Together these changes close the remaining body-local provider defects blocking Draftwright's
framed-coordinate adapter. They are deterministic geometry fixes; no learned policy or
corpus-specific feedback enters recognition. MFCAD++-500 development evidence is recorded for all
three changes, while MFInstSeg remains a downstream transfer baseline.

## 0.4.8

- **Made framed recognition the ordinary route for new integrations.** Successful framed result
  and bounded-report calls pair one inferred frame, the exact local working shape, and one
  aggregate result/report. `build_raw_recognition_result()` and `build_raw_recognition_report()`
  now name deliberate caller-coordinate operation. The historical ambiguous names remain raw
  compatibility aliases throughout 0.4.x and are scheduled for removal in 0.5.0; typed frame
  refusal never falls back automatically.

## 0.4.7

- **Let consumers classify the exact framed working shape before recognition.**
  `prepare_framed_part()` returns the inferred `LocalFrame`, topology-preserving local `part`, and
  one reusable cylinder inventory. Consumers can inspect that same local geometry, choose their
  own deterministic policy, and then call `recognise()` without a second cylinder scan or a second
  aggregate pass. Typed frame refusals support an explicit legacy fallback; the existing direct
  aggregate and framed-recognition APIs remain compatible.

- **Removed sign-dependent Fillet attribution at ambiguous support-plane ties.** Fillet recognition
  now refuses a candidate when equally valid neighbouring support planes disagree only because of
  axis sign. This removes eight false Fillet records and all observed Fillet frame transitions in
  the fixed 500-part MFCAD++ audit while preserving the other family counts.

- **Proved principal-axis covariance for angled steps and prismatic pockets.** Fixed corpus slices
  and rigid-motion controls now exercise X-, Y-, and Z-primary forms and distinguish recogniser
  defects from axis-sensitive golden serialization. The evidence introduces no learned policy and
  leaves existing result schemas unchanged.

## 0.4.6

- **Connected canonical cylinder recovery to Hole and Boss recognition.** Exact cylinders
  represented by eligible B-spline or Bezier faces can now participate through the same aggregate
  records and original-face evidence as native cylinders, after a version-pinned OCCT recovery
  certificate and an independent material-side proof. Unsupported OCCT versions, marginal trim
  coverage, ambiguous ownership and unproved orientation continue to fail closed; native-cylinder
  behavior remains on its compatibility path.

- **Recognised rounded rectangular pads through complete corner-blend evidence.** A rectangular
  pad whose four vertical corners are replaced by one complete convex blend cycle now produces the
  same `RaisedPad` record as its sharp control. The route requires four unique wall roles, an
  outward planar top, exact missing-area explanation and complete original-face provenance;
  partial, competing, concave, perforated or ambiguous cycles remain excluded.

- **Added circular blind-step recognition.** `recognise_circular_blind_steps()` reports inward
  quarter-cylindrical principal-axis corner cuts with one concave planar terminal, an opening at
  the opposite stock-envelope end, two convex transverse joins and an empty terminal-sector sweep.
  Full bores, through or doubly capped grooves, external/oblique/tapered walls, obstructed sectors
  and cross-solid evidence are rejected. The aggregate and capability manifest expose the new
  immutable `CircularBlindStep` family.

- **Preserved principal-axis polygonal stock through framed recognition.** `PolygonalStock` now
  accepts a valid regular-prism extrusion along local X, Y or Z and records that choice in its
  existing `axis` field. Existing direct Z records remain unchanged; this does not generalise
  attached `PolygonalBoss` recognition or arbitrary caller-space axes.

- **Added conservative rectangular through-step recognition.**
  `recognise_through_steps()` returns immutable `ThroughStep` records for exactly two
  principal-plane wall regions joined by one complete concave seam and open across both ends of a
  valid source solid. Independent holes, notches and curved boundary interruptions may cut a wall
  only while the complete seam, envelope, terminals and exactly empty removed prism remain proved.
  The additive public schema includes oriented `section`, run `axis`/`length`, and removed-prism
  midpoint `at`; the aggregate adds required
  `RecognitionResult.through_steps`, and the census adds `through_step`. Channels, pockets, slots,
  capped or partial runs, seam interruptions, tapered/non-principal walls and ambiguous sections
  remain excluded.

- **Added conservative paired-ramp step recognition.** `recognise_paired_ramp_steps()` returns
  immutable `PairedRampStep` records for principal-axis, mirror-symmetric two-ramp cuts with one
  proved exterior opening and closing terminal in a valid solid. The aggregate exposes them as
  `RecognitionResult.paired_ramp_steps`, and the census key is `paired_ramp_step`. The first
  supported domain deliberately excludes asymmetric, non-principal and fragmented occurrences.
  Each record carries the common ramp angle, run axis/length and shared-ridge callout anchor.

- **Added bounded aggregate recognition explanations.** `build_recognition_report()` returns the
  unchanged `RecognitionResult` together with per-family evaluated/not-applicable state,
  proposed/accepted/rejected counts, closed reconciliation reasons and the existing supported
  residual diagnostics from exactly one run. The report is explicitly non-exhaustive and exposes
  no graph, candidate or evidence identity. Existing result and framed-recognition contracts are
  unchanged.

- **Exposed the exact framed-recognition working shape.** Successful
  `FramedRecognitionResult` values now include `part`, the topology-preserving local shape passed
  to recognition. Consumers can therefore lower geometry and interpret records in one coordinate
  system without reconstructing private normalization. This adds a required public dataclass field;
  callers that manually construct `FramedRecognitionResult` must supply `(frame, part, result)`.
  Existing frame refusals and the legacy caller-space recognition route are unchanged.

## 0.4.5

- **Completed the supported inspection contract consumed by Draftwright.** The installed
  inspection manifest now freezes the closed `BevelReject.reason` values and the ordered names,
  types, and units of all six `read_double_d_tool()` return members. Runtime geometry behavior,
  aliases, recogniser output, and the recogniser capability manifest are unchanged. This closes
  the provider-contract gap in 0.4.4 that prevented a fail-closed downstream adoption.

## 0.4.4

- **Promoted the consumer-proven declared-feature inspection API.** New consumers can import
  `inspect_face`, its closed analytic result/refusal values, `classify_bevel` / `BevelReject`,
  `cone_rims`, `read_double_d_tool`, and `floor_face_anchor` from the supported
  `b123d_recognisers.inspection` namespace. A separately versioned, fail-closed
  `inspection_api.json` manifest freezes signatures, value schemas, and compatibility paths
  without changing the recognition capability manifest. Existing root, family-module, and
  `experimental_geometry.inspect_face` paths retain exact object identity. `GeometryGraph`,
  adjacency, blend collapse, correspondence, and evidence remain private or experimental.

- **Added exact NURBS-plane support for Raised Pads with a fail-closed ownership contract.**
  Exact analytic planes converted to B-splines are recovered under a versioned OCCT certificate,
  while material side is proved separately from the unchanged original face. Pad recognition now
  requires every participating face to have exactly one owner in a valid closed solid; open
  shells, invalid bodies, and ambiguous or missing ownership intentionally return no Pad records.
  A pinned 20-fixture conversion sweep validates face correspondence, topology, complete records,
  defining evidence, reviewed geometry bounds, and a separate performance ceiling.

- **Measured NURBS recovery on an external human-authored STEP corpus.** A reproducible scanner
  keeps the licensed Fusion 360 Gallery data outside the repository, fixes its sample before OCCT
  import or fitting, and records every refusal and import failure. Across 1,000 B-spline-bearing
  models, recovery accepts 48/12,729 spline faces (0.3771%) across 21 models and changes no Pad
  result. A topology-preserving counterfactual also exposes each recovered primitive to every
  recogniser under both classification modes: cylinders add 29 patch records, four Flat candidates
  and one Hole candidate across 11/20 completed affected models, while recovered planes and cones
  add nothing. The results support a focused cylinder-orientation investigation, not blanket
  family migration; changed candidates remain unvalidated until curved orientation is proved.

## 0.4.3

- **Added opt-in part-relative recognition.** `build_framed_recognition_result(part)` pairs the
  unchanged local `RecognitionResult` with an explicit `PartFrame`, allowing callers to separate
  physical-part coordinates from arbitrary STEP placement without changing the legacy entry point.
  Frame gauges state when discrete orientation (`ORTHOGONAL`) or continuous roll (`AXIAL`) is not
  observable from geometry; `FULL` is reserved for a geometry-directed ordered basis.

- **Corrected prior-art dataset guidance.** MFCAD's in-repository STEP models and labels are MIT
  licensed, while MFCAD++ and MFInstSeg require separate terms review. The documentation also
  records that `.face_truth` labels are Python pickles and should be inspected with
  `pickletools.dis` rather than loaded blindly.

## 0.4.2

- **Fixed aggregate evidence for two-sided countersinks and multiple edge pads.** A through bore
  with countersink seats on both faces now retains the same deterministic first seat carried by
  the public `HoleRecord`; the other seat remains an independent `CounterSink` instead of making
  the aggregate fail. Distinct rectangular pads may now share an original stock-wall face when
  OCCT merges their coplanar boundary, while unique top-face ownership and same-solid proof remain
  required. These cases are frozen from Draftwright consumer regressions.

## 0.4.1

- **Fixed plate evidence when separate solids have coincident planes.** Plate discovery still
  groups coplanar faces for its public value result, but attributed recognition now partitions
  each low/high role by solid ownership and retains the unique solid proved on both sides. This
  prevents an unrelated face on the same plane from contaminating an otherwise valid plate
  occurrence. Multiple possible common solids remain ambiguous and fail closed.

## 0.4.0

- **Added an experimental, graph-independent face-inspection contract.**
  `b123d_recognisers.experimental_geometry.inspect_face(face)` returns a closed analytic surface
  fact and optional trimmed-surface anchor without exposing graph identity to the caller.
  Draftwright's declared-fillet workflow is the first external consumer. The module is deliberately
  absent from package-root exports and the capability manifest while its naming and refusal model
  are reviewed. The larger `GeometryGraph` surface remains experimental and is consumed internally
  by Polygonal Boss/Stock only; no recognition record, manifest schema or existing public result
  changes in this release.

- **Passage attribution now has one rich physical authority.** `SectionPassage` records canonical
  constant-section geometry on principal and free axes, while writer-free `recognise_passages`
  retains its historical schema-v1 values and order. Aggregate `.passages` is the accepted-rich
  projectable subsequence rather than a second discovery authority. This intentionally omits a
  historical partial-span false positive on checked-in `10060.step`: direct legacy output remains
  `(X, Z)`, rich and aggregate output truthfully retain only Z, and the Passage census changes
  from 2 to 1. Exact Slot, Pocket and Prismatic Pocket dispositions and every other census key on
  that fixture remain unchanged. Passing `ledger=` to the legacy API now fails loudly; attributed
  callers migrate to `recognise_section_passages`.

## 0.3.1

- **Clarified empty-evidence reconciliation and restored rotational Passage evidence.** Empty
  defining evidence now explicitly proves no containment: an unrelated Passage cannot erase an
  evidence-free obround Pocket, and an evidence-free obround Slot cannot erase an unrelated
  four-wall Passage. This blesses the fail-closed semantics introduced by the framework
  consolidation; reverting them would restore vacuous empty-set precedence. Passage discovery
  again runs for rotational aggregate inputs so its evidence participates in reconciliation,
  while the public rotational projection remains empty as before the consolidation.

  The private framework was narrowed to what has a production consumer: Candidate evidence owns
  defining faces only, failed-predicate terminal context remains on Observations, and unused
  ambiguous/unsupported Candidate disposition arms were removed. No public record or schema
  changes.

- **Fixed: unrelated recess walls no longer manufacture a rectangular feature across solid
  material.** AAG/gAAG coherence remains the first proof that two opposed walls participate in
  one boundary, but connectivity alone cannot establish that the rectangle between them was
  removed: parallel recesses, and H- or U-shaped connections between them, supplied concrete
  counterexamples. Paired-wall `Slot`, `Pocket`, and `Channel` candidates now additionally require
  their uninterrupted, **unrounded** rectangular prism to be materially empty within the source
  solid being recognised. Disconnected compound members remain independent. Only a curved end
  interruption proved from opposite-turn AAG arcs is trimmed before that test. Candidate
  admission has no material-volume allowance; the existing 1% allowance remains confined to
  recombining already-recognised collinear slot arms.

  This is a visible recognition correction: callers that relied on permissive opposed-wall
  candidates will receive fewer records. Across the ten vendored NIST complex parts it removes
  old rectangles whose proposed volumes still contain material—including repeated pockets about
  24–31% solid and slots about 3–54% solid—while the frozen 33-model MFCAD++ holdout remains
  unchanged. The historical NIST counts remain recorded in the tests, with the reviewed removals
  applied separately rather than silently rewriting that baseline (#142).

## 0.3.0

- **Added explicit turned semantics to chamfer and fillet records.** `Chamfer` and `Fillet`
  now carry `turned: bool = False`. Conical chamfers and toroidal fillets report `True`;
  planar bevels and cylindrical blends report `False`. The default preserves legacy positional
  construction. Recognition acceptance, counts, measurements and claims are unchanged. This lets
  drafting consumers choose a shaft-profile view without guessing from axis equality or rescanning
  the solid (#150).
## 0.2.9

- **Fixed: dimensioned 0.3 mm chamfers are recognised.** The absolute chamfer evidence floor is
  recalibrated from 0.5 mm to 0.3 mm against the two C0.3 flange edge treatments on the real
  GRM-03 turned screw. The threshold remains one family-level manufacturing policy for planar
  and conical chamfers: there is no turned-only exception. A 0.2 mm synthetic edge break and the
  existing countersink, taper, drill-point and corpus negatives remain excluded. Equality at the
  floor now tolerates only the shared 1 µm coordinate-noise band before record quantisation
  (#146).

## 0.2.8

- **Fixed: turned chamfers are now recognised.** A lathe produces a short external conical face
  where a prismatic part has an oblique planar bevel; the chamfer reader now recognises both,
  recovers the axial and radial legs, and proves the cone is adjacent to an external OD so an
  internal countersink is not misreported. `RecognitionResult.chamfers` is no longer gated away
  when `rotational=True`.

- **Added: turned fillets and radiused groove lead-ins.** A lathe sweeps an edge fillet into a
  torus; external, principal-axis tori now produce `Fillet` records in rotational inventories.
  Groove recognition also follows a bounded coaxial torus/annular-wall chain between two OD
  bands, so a rounded lead-in joins the bands without mistaking an unrelated torus for a groove.

## 0.2.7

- **Fixed: a slot with a coaxial post at either end is recognised again.** The AAG coherence
  gate added in 0.2.6 treated every face adjacent to both slot walls as boundary evidence. A
  convex cylindrical post meets those walls with opposite arc directions, so it vetoed the
  planar boundary and made the slot silently disappear. Planar shared neighbours now carry the
  coherence decision when present, with curved neighbours retained for boundaries that have no
  planar member. One- and two-post fixtures both recover the original 30 × 8 slot through the
  direct recogniser and the aggregate, without weakening the grazing-wall rejection that
  prompted the gate (#140).

  This is a recognition-behaviour patch with no public record, capability-manifest, typing or
  archive-boundary change. Roll back to 0.2.6 to restore the stricter 0.2.6 boundary policy.

## 0.2.6

- **Recess candidates now have AAG-coherent boundaries and one aggregate ownership policy.**
  Opposed planar walls must have agreeing AAG arcs into shared boundary neighbours; when STEP
  fragments that boundary, a smooth-arc walk supplies the equivalent gAAG region. Complete
  polygonal passage and pocket rings then take precedence over paired-wall fragments contained
  inside them; four-wall rings still yield to the more directly dimensioned `Slot` or `Pocket`.
  On the 40-model MFCAD++ design corpus this reduces 35 proposed slots to 19 accepted slots,
  removes all sub-0.5 mm grazing-wall artifacts without a fitted size threshold, and leaves only
  two intentional one-face partial overlaps where neither claim contains the other (#112, #119).

- **Fixed: a wall of a triangular pocket was reported as an angled step.** A step is a wedge cut
  into an *edge of the part*; the gate meant to exclude recess walls asked what the slant bridges,
  and a pocket whose plan is a right triangle answers correctly — its hypotenuse bridges the two
  axis-aligned walls beside it and its floor is a triangle. Four of the five gates passed and it
  was still a pocket. Now separated by what lies *beyond* the virtual corner: free space for a
  corner of the stock, material for two walls of a recess meeting.

  **No recorded value changes on any vendored part.** The geometry occurs in none of the 73, which
  is why it survived until a corpus was held out — it was found on MFCAD++ models drawn from
  classes no predicate here was shaped by.

- **Fixed: a bolt hole through a step's blind end deleted the step.** The flat closing an angled
  step is recognised by being a triangle, and the edge count included the hole's circle, so the
  face read as four edges and the whole record vanished — every field of it correct. The count is
  now taken on the flat's **outer wire**: a triangle with a hole through it still has three edges
  there, a rectangle still has four however it is drilled, so no chamfer end cap is admitted. A
  triangle whose *side* is split by a neighbouring feature is still missed, and is still listed as
  an exclusion.

  Additive where it applies, and again **no recorded value changes on any vendored part**.

- **A held-out corpus, and the first accuracy figure not measured on geometry this project was
  shaped by.** `tests/corpus/mfcadpp_holdout` is thirty-three MFCAD++ models from the *val* split,
  covering the twenty classes the vendored design set does not target, scored once after the last
  predicate change: angled steps 100% precision over eight records, and none of 226 stock faces
  claimed by anything. The rule that selected it is in its manifest so the next draw can be made
  the same way.

- **A runtime budget with two workloads.** `tools/benchmark_recognition.py --workload census`
  measures `feature_census` over the vendored NIST and real parts, `--workload composite` keeps
  the four-fixture release arm, and `--check` measures either against
  `docs/benchmarks/recognition-budget.json`. Reported as minimums rather than medians, with peak
  RSS. Not wired into CI: a wall-clock assertion on a shared runner fails for reasons unrelated to
  the code.

- **Internal: the recess hotspot is four modules.** `_recess_core` carried face reading, candidate
  reduction, obround recovery and the three families' predicates in 1,200 lines; those are now
  `_recess_faces`, `_recess_reduce`, `_recess_obround` and a smaller `_recess_core`, layered
  strictly downward and asserted edge by edge. No public symbol, signature, record value or
  `__module__` changed.

- **`feature_census` no longer counts a plate on a shaft whose steps form a turned profile.**
  `build_recognition_result` has always suppressed one there -- a stepped shaft is not a plate --
  and the census did not, so the two entry points reported different answers about the same
  solid. Measured across the 73 corpus parts it happened on exactly one, a real turned screw, and
  **a public count changes for it**: `plate` goes from 1 to 0. No semantic golden moves, because
  no pinned fixture carries the shape.

  The census applies the half of the aggregate's gate it can evaluate. The other half is the
  caller's `rotational` classification, which `feature_census(part)` does not take.

- **New family: `recognise_passages` / `Passage`.** A through void bounded by a closed ring of
  three or more planar walls is recognised without requiring those walls to share principal
  axes. The record carries the run `axis`, `length`, side count and section polygon, so a
  triangular, rectangular or hexagonal passage keeps the geometry needed for dimensioning.

  A through slot is also an uncapped wall ring, so the base recogniser may propose both records.
  The aggregate and census reconcile them from their claimed faces: a four-wall passage yields
  to the more directly dimensioned `Slot`, while a non-rectangular ring defeats paired-wall
  fragments contained inside it. Calling `recognise_passages` directly returns candidates before
  that aggregate policy, consistently with the other reconciled families.

  `Passage` and `recognise_passages` are new public names. The capability manifest records the
  family as introduced in 0.2.6, with independent functional and golden evidence.

- **New family: `recognise_prismatic_pockets` / `PrismaticPocket`.** A floored recess of any
  planar cross-section, found by walking the closed ring of walls rather than pairing walls that
  share a normal axis. `recognise_pockets` buckets walls by axis and pairs within a bucket, so a
  triangular recess -- whose walls share no axis -- forms no candidate and reaches no gate:
  measured over 600 MFCAD++ models, **94% of triangular-pocket faces never reach a test**, which
  is why that family scored 0% on them and 4% on hexagonal ones.

  The record carries `sides` and the `section` polygon, as `Passage` does, because a triangular
  and a hexagonal pocket of equal depth are otherwise the same record. It is **not** a `Pocket`:
  folding it in would have made `Pocket.width` sometimes a wall-to-wall measurement and sometimes
  a bounding-box extent, changing what an existing field means for every consumer.

  **Neither family subsumes the other.** An obround recess has cylindrical ends and forms no
  closed planar ring, so `recognise_pockets` remains the only path to it -- measured at zero
  rings across the whole *Circular end pocket* class. Where both see the same rectangular recess
  both report it, and `_reconcile.prismatic_pockets_that_are_not_pockets` keeps the `Pocket`.

  Additive: **no existing recorded value changed**. Every golden gained the new family's output
  and nothing was removed or altered.

- **Fixed: a family that walks the graph accepted a ledger built from a different part.**
  `recognise_passages` resolved no face against its graph, so a mispaired one was never refused
  and it reported records describing the *other* solid. Now checked in the shared ring walk, so
  both ring-walking families are covered.

- **The chamfer/angled-step split moved from the recognisers to the reconciler.** Both families
  read the same oblique bevel, and until now each carried a gate phrased in terms of the *other*
  one: `recognise_chamfers` declined a bevel edge-adjacent to a triangular flat, and
  `recognise_angled_steps` required one, through a predicate they shared in
  `_adjacency`. That is an ownership decision, and ADR 0003 puts ownership after
  discovery. Both families now write a claim naming the one face they were established by — the
  bevel, and the slant — and `_reconcile.chamfers_that_are_not_angled_steps` drops a chamfer
  whose face a step already has.

  **Reconciled output is unchanged.** `build_recognition_result` and `feature_census` apply the
  rule and were verified byte-identical over all 72 corpus parts: 19 synthetic goldens, 40
  labelled MFCAD++ models, 10 NIST CTC models and 3 real turned parts.

  **What changes is `recognise_chamfers` called on its own**, which now reports a blind step's
  slant, as it did before `recognise_angled_steps` existed. Over the 40 MFCAD++ models that is 8
  extra records on 8 models — 8 of the 11 angled steps there; the other 3 are turned away as
  spanning wedges, which is the chamfer family's own gate. Every one of the 8 lands on a face
  the corpus labels *Triangular blind step*, and the rule takes back all 8. A caller who
  wants the reconciled answer should use the
  aggregate or the census, which is the same posture `recognise_passages` takes towards a slot's
  void and `recognise_turned_steps` towards a groove's rung. One pinned golden moved: the
  `recognise_chamfers` entry of `angled_blind_step` gained the ramp's slant.

  `recognise_chamfers` and `recognise_angled_steps` gain an optional `ledger=` parameter, in the
  shape `recognise_slots` and `recognise_grooves` already have. No record gains, loses or alters
  a field.

## 0.2.5

Adds a recogniser family, and with it corrects a defect that family's absence was causing.
`recognise_chamfers` was reporting the slanted walls of steps and passages as chamfers: measured
per face over 120 MFCAD++ models its precision was 44%, and on nine of the ten models carrying an
angled step, the step's slant was the **only** chamfer reported while the genuine chamfers on the
same part were rejected. Anyone consuming chamfer records on prismatic parts should take this.

The distinction between the two is deliberately **not** size. Every part-relative and
neighbour-relative ratio measured — leg against part extent, truncation of each neighbour, strip
aspect, area against neighbour — overlaps between the two populations, so any threshold would have
been fitted to one corpus. A chamfer runs the full length of the edge it breaks; an angled step
stops, and a triangular flat closes the blind end. That test is topological and mentions nothing
outside the feature, so it holds at any scale — `tests/test_scale_invariance.py` proves it across
0.05x-100x.

- **New family: `recognise_angled_steps` / `AngledStep`.** A wedge taken out of an edge — one
  oblique planar wall stopping part-way along it, closed by an axis-aligned triangular flat.
  100% precision and 70% instance recall over 120 MFCAD++ models. `length` is the field a
  `Chamfer` has no analogue for: a chamfer spans its whole edge, so its extent is not a chosen
  dimension. Ends whose triangle is subdivided into four or more edges by a neighbouring feature
  are not recognised, which accounts for about half the recall gap and is documented rather than
  worked around.
- **`recognise_chamfers` declines a bevel with a triangular blind end.** Precision 44% to 78% with
  every real chamfer kept. This is a recognition-behaviour change: a part with an angled step
  loses a chamfer record and gains an angled-step record. No pinned golden moved.
- **Chamfered grooves are read as one groove.** A conical lead-in between two cylindrical bands is
  matched by its rims rather than split into separate features (#60).
- **Countersink radii are read from the surface adaptor**, not `Face.radius`, correcting sizes on
  interrupted and cross-bored geometry (#74).
- **A census is about 14% faster.** `Face.edges()` is the suite's most expensive derived query and
  every recogniser was asking it of the same faces; one `FaceEdges` memo is now shared across a
  run. The new family costs roughly 11% of a census back, so the net gain is smaller than 14% —
  both figures are measured against their own baselines rather than combined into one.

`FaceEdges`, `AngledStep` and `recognise_angled_steps` are new public names.
Strict semver would make that a minor version; this project ships 0.2.x patches and says so here
instead. No existing record gains, loses or alters a field.


## 0.2.4

Corrects a regression in 0.2.3. Every pinned golden is byte-identical to the original Draftwright
capture again, and every count 0.2.3 changed on real parts is restored — verified against the NIST
MBE PMI complex test cases, not only the synthetic corpus. Anyone on 0.2.3 should take this.

Output is **not** identical to 0.2.2 in every case, and the exception is deliberate. On
`nist_ftc_09` the level recogniser reports fifteen levels where 0.2.2 reported sixteen, because
0.2.2 split a pair of faces **0.475 mm apart under a 0.5 mm tolerance** into separate levels — a
consequence of grouping by grid cell rather than by distance, fixed independently of the tolerance
work. Two faces closer together than the tolerance are one level. A 0.635 mm gap on the same part
is still correctly two.

- **Minimum-evidence thresholds are absolute again.** 0.2.3 scaled them to the part, which made a
  feature's existence depend on what surrounds it: the same 1 mm chamfer was recognised on an
  80 mm plate and absent on a 200 mm one. Six NIST MBE PMI parts lost records in nineteen places
  and gained in none. Affects the `chamfers` minimum leg, `fillets.min_radius`, `plates` slab
  thickness, `pads` footprint, `polygonal_bosses` height and `flats` minimum depth.
- **The recess merge band is absolute again.** Scaling it to the whole solid merged pockets and
  slots that a smaller plate kept distinct, and simultaneously raised the minimum separation of
  two slot ends — losing records in both directions at once.
- **`RiserEvidence.tol` reports `0.5` again**, so the goldens match the capture.
- [ADR 0008](docs/adr/0008-length-tolerance-policy.md) now distinguishes a **tolerance** ("are
  these two things the same?", which scales with what it compares) from a **minimum-evidence
  threshold** ("is this big enough to be a feature?", which must not). Whether a small feature on
  a large part is worth dimensioning is consumer policy under ADR 0001, and recognition should not
  answer it silently.
- Genuinely feature-relative tolerances from 0.2.3 are kept: diameter matching in `countersinks`,
  `grooves` and the cylinder stack, and the recess cap radii. Those compare two measurements of
  one feature and do scale with it.
- The `grooves` step-depth and width margins are absolute again. Found by auditing every
  remaining proportional gate rather than from a report: the NIST parts are prismatic and have no
  grooves, so nothing downstream would have surfaced it. A 2 mm groove was recognised on 15 mm bar
  and lost on 100 mm.
- The `flats` chord gates are absolute again too. They were missed on the first pass and caught
  by the real parts: `ctc_05` reported four flats on 0.2.2, none on 0.2.3, and two on the partial
  fix. Verified against all five NIST complex test cases, every reported count restored.
- `tests/test_large_part_small_features.py` pins the property with parts larger than the fixture
  corpus carrying features smaller than it implies — the combination the 30–180 mm fixtures never
  covered. `tests/test_nist_ctc_corpus.py` pins the reported baseline against the real parts, and
  skips unless `B123D_NIST_STEP_DIR` points at them, since `migration/PARITY.md` commits the
  project to comparing record projections rather than committing STEP bytes.

## 0.2.3

Recognition-behaviour release. Every gate that compares a length now scales with the geometry it
judges, so the same feature is recognised the same way whatever size it is modelled at. Records,
signatures and record schemas are unchanged; the capability manifest differs from 0.2.2 only by the
package version it embeds.

- **Length tolerances are proportional** ([ADR 0008](docs/adr/0008-length-tolerance-policy.md)).
  Every golden fixture now recognises identically from 0.05x to 100x, across every recogniser
  family; previously six of seventeen changed. Of the roughly 39 constants the review counted, 18
  were not lengths at all — ratios, direction cosines, angles, counts, epsilons — and three model a
  physical constant (an edge break does not grow with the shaft) and stay absolute, bounded so they
  cannot swallow a small feature.
- **`tol=` accepts `None`** on `recognise_plates`, `recognise_chamfers`,
  `recognise_rectangular_pads`, `recognise_polygonal_bosses`, `recognise_polygonal_stock`,
  `recognise_face_levels` and `recognise_risers`, as does `min_radius=` on `recognise_fillets` and
  `boundary_margin=` on `step_ladder_for_z_span`. `None` derives the value from the part. **An
  explicit float keeps its literal millimetre meaning**, so a caller who has calibrated against
  their own geometry is unaffected.
- **`RiserEvidence.tol` reports the tolerance its scan resolved** rather than a fixed `0.5`. This is
  the only record value that moves — 33 values across 5 fixtures, and no geometry field moves at
  all. The field keeps its default, so direct construction is unchanged. Recorded as the first
  intentional divergence from the Draftwright capture in `migration/PARITY.md`.
- **Coplanar faces group by distance, not by grid cell.** `round(coord / tol) * tol` merged faces
  0.24 mm apart while splitting faces 0.02 mm apart, and put a multiple of `tol` into `Plate.lo`
  and `.hi` — so a 3.7 mm slab reported a thickness of 3.5 mm. Both fixed.
- **An area gate no longer turns on a floating-point tie.** A face whose area sat exactly on the
  40% threshold was admitted or refused according to rounding, which made one fixture gain a plate
  at 5x and 10x and nowhere else.
- Proven across STEP export and re-import: all seventeen fixtures reproduce their pinned records
  exactly. Geometry arriving as B-splines remains an explicit whole-package exclusion; see
  [`docs/capabilities.md`](docs/capabilities.md).

## 0.2.2

- Decomposes the cylinder, hole/boss, pattern, and recess implementations along private,
  architecture-tested seams. Existing public imports, object identities, record module paths,
  deterministic ordering, shared-inventory behavior, capability declarations, and canonical
  semantic goldens are unchanged.

## 0.2.1

Compatibility-safe boundary and delivery-workflow patch release. Recognition output and canonical
semantic goldens are unchanged.

- Adds `RecognitionResult.step_ladder_for_z_span(z_min, z_max, *, boundary_margin=0.6)` as the
  build123d-free aggregate projection boundary. The margin is in model length units and its strict
  end behavior, validation, determinism, and JSON-safe output are tested. The old
  `step_ladder(BoundBox)` call is deprecated since 0.2.1 but remains throughout 0.2.x and will be
  removed no earlier than 1.0.0. Existing recognition semantics and goldens are unchanged.
- Adds a single-job Draftwright downstream canary for package pull requests and weekly consumer-
  drift checks. It records the resolved consumer commit, package commit/version, capability digest,
  and wall time while reusing the candidate-wheel contract harness rather than duplicating either
  repository's platform matrix. Package branches now launch that platform matrix only through the
  pull request instead of duplicating it for both branch-push and PR events, and superseded PR runs
  are cancelled. Recognition behavior and canonical goldens are unchanged.

## 0.2.0

Additive production-hardening release with no recognition-policy changes.

- Adds a deterministic, versioned capability manifest covering every public recogniser and
  record, with independent runtime/schema/evidence validation and installed-wheel parity.
- Exposes supported Python and command-line manifest queries so consumers can fail closed on
  unknown capability families without reading package internals.
- Makes the shipped `py.typed` contract enforceable, aligns public capability prose with proven
  behavior, and makes package rationale self-contained for standalone readers.
- Bounds complete hole-grid candidate work, strengthens branch-sensitive coverage to an enforced
  91.4% floor, and publishes Linux coverage through Codecov.

All canonical semantic goldens remain unchanged. Draftwright consumes this release through its
separately owned downstream capability declaration.

## 0.1.0

First stable release of the standalone Apache-2.0 recognition package.

- Promotes `0.1.0a1` after the packaged cutover merged in Draftwright PR #1168
  (`d659e7a6`), with the duplicate embedded recogniser implementation removed.
- Retains the 17 pinned semantic golden fixtures, public-inventory/serialization contracts, and
  cross-platform Python 3.10/3.12/3.14 matrix; this release contains no new recognition behaviour.
- Uses the reviewed TestPyPI-first Trusted Publishing path to promote one exact wheel and sdist
  to PyPI without rebuilding between indexes.

The complete migration, provenance, and performance evidence remains in
[`migration/PARITY.md`](migration/PARITY.md).

## 0.1.0a1

First prerelease of the standalone Apache-2.0 recognition package extracted from Draftwright.

- Includes every recogniser, shared geometry substrate, aggregate result, and feature census from
  Draftwright commit `3fe20b0f71a71deced06b310943dd44cc66e355e`.
- Matches all 17 pinned semantic golden fixtures and preserves the ADR 0002 public contract.
- Normalizes an exact dominant-axis numerical tie to the pinned result across Windows, macOS, and
  Linux; no feature-policy changes are included.
- Ships typed Python sources plus `LICENSE`, `NOTICE`, and `THIRD_PARTY_NOTICES.md`.

The complete migration and performance evidence is in [`migration/PARITY.md`](migration/PARITY.md).
