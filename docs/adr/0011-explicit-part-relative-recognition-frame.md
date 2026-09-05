# ADR 0011 — Pair local recognition with an explicit part frame

- **Status:** Accepted
- **Date:** 2026-08-27
- **Decider:** Paul Fremantle
- **Evidence:** [frame-handling evaluation](../benchmarks/frame-handling-prototype.md), issue #272,
  spike #274, shipped framed route 0.4.3, working-shape contract #282, Epic 0005 issue #317

## Context

Recognition currently interprets several geometric predicates and reconciliation choices in world
XYZ. A rigid X30-plus-translation presentation removes 1,571 of 2,784 occurrences in the first
500 MFCAD++ development models. The same physical part can therefore produce a materially different
inventory solely because of its STEP placement.

Making every recogniser free-axis would spread frame policy and new record schemas across the
package. Hiding normalization inside the existing entry point would silently change the meaning
of legacy axis letters and let callers mistake local coordinates for caller coordinates.

## Decision

Adopt the explicit framed boundary as an opt-in public route:

1. A geometry-established, right-handed `PartFrame(origin, x, y, z)` maps caller-space points to a
   local recognition frame.
2. A successful framed recognition result owns that frame, the exact topology-preserving local
   working `Shape` passed to recognition, and the existing `RecognitionResult`. All evaluated shape
   coordinates, record coordinates, and axis letters are local to the paired frame. Consumers must
   retain the successful result while relying on the working shape's identity relationship to
   topology-bearing evidence.
3. Existing `build_recognition_result(part)` behavior remains unchanged. Framed recognition stays
   a separate opt-in route; making it the aggregate default requires its own compatibility
   decision and release plan.
4. Frame inference is closed. Geometry with no analytic direction returns a typed refusal. A
   single established axis returns an explicit `AXIAL` gauge: its chosen roll is a deterministic
   representative and must not be treated as a semantic material axis. The prototype's remaining
   representative is explicitly a gauge choice, not production material-axis semantics.
5. Recognition still executes once through the existing registry and reconciliation stack. The
   boundary does not expose `GeometryGraph`, correspondence, candidates, or recogniser internals.

The selected implementation uses rigid `TopLoc` placement. It changes evaluated coordinates
without rebuilding topology and exposes the exact placed object so consumers do not create a
second normalization authority.

## Evidence and gate

The complete 20-fixture golden inventory is invariant occurrence-by-occurrence under Z30, X30,
X90 and translation after independent frame inference: 75/75 same-family, with no refusal,
reclassification, absence or introduction.

On the deterministic first 500 lexical MFCAD++ test-split files used as open development data,
all 500 infer a full frame. Framed X30-plus-translation retains all 2,750 baseline occurrences,
with zero reclassifications and absences; one model introduces one extra Slot fragment. Replacing
copied BRep transformation with rigid TopLoc placement eliminated the other 12 transform-induced
occurrence differences and the macOS body-ancestry failure. A degenerate 7.1e-15 recess probe that
previously raised an OCCT domain error now fails the candidate closed.

Frame inference plus normalization consumes 16.30 seconds against 436.12 seconds of framed
recognition (3.74%). The paired framed route including that work is about 8.29% slower than the raw
paired recognition run.

Acceptance is supported by the following evidence:

- directed axes use geometry-established orientation where observable; `ORTHOGONAL` and `AXIAL`
  explicitly publish the remaining gauge instead of claiming a material direction;
- the legacy route and its recess semantics remain unchanged, while the framed route's one known
  Slot fragment is recorded as a bounded opt-in numerical limitation;
- the supported Linux, macOS and Windows matrix exercises the deterministic framed contract;
- the named 500-model MFCAD++ development evaluation is checked in as a machine report; and
- legacy public tests and goldens remain unchanged.

Acceptance does not make framing the default aggregate behavior. That is a separate compatibility
decision and release plan under Epic 0005.

The sealed MFTRCAD holdout is not required for this architecture decision and remains unused.

## Consequences

- Callers can distinguish part placement from recognition semantics without a broad free-axis
  record migration.
- Unconstrained roll is explicit gauge rather than a hidden semantic axis; geometry with no
  analytic direction produces an explicit non-result.
- The legacy public API and coordinate meaning remain compatible. The framed result's required
  working-shape field shipped in 0.4.6 as an explicitly approved pre-1.0 patch compatibility
  event; callers constructing `FramedRecognitionResult` directly must add the working shape.
- The topology-preserving placement route is release quality as an explicit opt-in API.
- Making it the default recogniser path remains deferred to a separate compatibility decision.

## Amendment (ordinary aggregate route, issue #317)

The explicit framed boundary is the ordinary aggregate route for new integrations from 0.4.8.
This changes guidance, not the return type of an existing function. Successful calls still own one
`PartFrame`, the exact locally placed working shape, and the result of one aggregate run. Typed
frame refusal remains explicit and never triggers raw recognition.

Caller-coordinate operation is now named directly by `build_raw_recognition_result`. The
historical `build_recognition_result` remains a raw compatibility alias throughout 0.4.x and is
removed in 0.5.0; it will not silently change to a framed union return. This gives callers a staged
pre-1.0 migration while avoiding two implicit defaults indefinitely. Examples, capability guidance
and the named Draftwright integration use the framed lifecycle; code that intentionally needs
caller/world coordinates uses the explicit raw name.

The paired bounded-report route follows the same authority: `FramedRecognitionReport` owns the
same frame and exact working shape beside one local `RecognitionReport`. Preparation exposes
`recognise_report()` for consumers whose classification depends on the normalized shape and
precomputed cylinder substrate. Neither result nor report path repeats aggregate recognition.

## Amendment (principal-axis Polygonal Stock, issue #311)

An `ORTHOGONAL` frame may map a regular prism's physical extrusion direction to any local principal
axis. Frame inference must not reorder that valid representative for one family: doing so would
silently change every other local record. Instead, whole-part `PolygonalStock` recognition accepts
X, Y or Z in the supplied recognition frame and uses its existing `axis` field to disambiguate
`base` and `top`; its centre, flat directions and flat centres remain full local 3-D values.

This is an additive pre-1.0 record-value expansion, not a schema replacement. Existing direct
Z-axis records remain byte-identical. Attached `PolygonalBoss` stays Z-only because its support and
material-side semantics are a separate contract without corresponding evidence. The stock family
still issues exactly one Candidate owning the complete eight-face boundary of one valid solid.
Generic caller-space axes remain unsupported by the raw route and become principal only through the
existing explicit framed boundary. The framed result continues to pair the exact working shape,
local records and frame; no consumer may infer or substitute a different representative.

## Amendment (principal-axis Polygonal Boss, issue #332)

Attached `PolygonalBoss` discovery now accepts X, Y or Z in the supplied recognition frame. Its
existing orientation-bearing schema is sufficient: `axis` identifies the axial coordinate,
`center`, flat directions and flat centres remain full local 3-D evidence, and `base < top` retains
an ordered coordinate interval. Attachment direction is not inferred from that ordering. Instead,
both terminal boundaries must have normals agreeing on one signed principal direction; trying the
positive and negative interpretations independently keeps ambiguous or inconsistent caps closed.

The provider checks all three principal axes against the same run-owned graph and issues one
candidate owning exactly the six original side faces. Complete convex corner-blend cycles use the
same selected axial coordinate rather than a hidden Z assumption. Whole stock, inward recesses,
other side counts, detached bodies and cross-solid assemblies remain separate or refused classes.
The explicit framed result continues to pair these local values and anchors with the exact working
shape. This is an additive pre-1.0 value expansion, not a schema or framing-default change.

## Amendment (prepared consumer classification, issue #328)

Some consumers own a rotational/prismatic classification that is itself defined from the local
working shape and cylinder substrate. Requiring that boolean before normalization makes the
framed call placement-dependent; defaulting it and filtering later cannot repair family
applicability or reconciliation decisions already made inside the aggregate.

`prepare_framed_part()` therefore publishes one additional lifecycle boundary. It performs frame
inference, exact topology-preserving normalization, and the already-public cylinder analysis once,
then returns a `PreparedFramedPart` pairing those three facts. The consumer derives its policy from
that exact local value and calls `prepared.recognise(rotational=...)`. That method injects the
prepared cylinder inventory into the existing aggregate, which still executes once through the
registry and reconciliation stack, and returns a `FramedRecognitionResult` with the same frame and
working shape.

This does not publish `RecognitionContext`, graph identity, Candidates, evidence, registry, or
reconciliation. It does not transfer classification, view, or drawing policy upstream. The
existing explicit-boolean framed function delegates through the prepared lifecycle and remains
source-compatible; typed frame refusal still occurs before any aggregate and permits an explicit
legacy fallback. FULL, ORTHOGONAL and AXIAL gauges all preserve the caller's local classification.

## Amendment (paired framed recognition evidence, issue #463)

The prepared lifecycle also supports one evidence-bearing aggregate result. The successful value
pairs its frame and exact local working shape with the original caller part and the public
accepted-occurrence evidence projected from that same run. Records and `face(ref)` remain local to
the working shape. `caller_face(ref)` is a separate exact projection to the caller part, authorized
only when applying the exact retained rigid placement to each caller face produces a complete bijection
under OCCT `IsSame` identity. This retains topology-partner provenance while distinguishing
separate located occurrences of one shared TShape.

Frame inference refusal remains `RefusedPartFrame`. A missing or non-bijective caller-face relation
returns a separate typed evidence-mapping refusal; it never falls
back to raw recognition or approximate matching. Preparation still scans cylinders once, and the
consumer still selects `rotational` from the prepared local substrate before the one aggregate
call. Existing result/report operations, raw evidence, frame gauges and recognition outcomes are
unchanged.

### Amendment (completed result on late mapping refusal, issue #493)

Mapping checks occur both before inventory and against the completed evidence face census.
`RefusedFramedEvidence[FramedRecognitionResult].result` is `None` for a pre-inventory refusal.
If the post-inventory pairing or final census fails, it instead retains the exact completed
`FramedRecognitionResult`: the prepared frame and working part, and the same aggregate and
cylinder objects from that run. No partial evidence view, face references or private inventory
product is exposed. A caller may reuse that result without another inventory; the provider never
silently falls back to raw recognition. Successful evidence results are unchanged.

The optional field is additive and defaults to `None`, preserving the one-argument refusal
constructor. The existing evidence manifest enumerates public symbols, not dataclass fields;
its symbol inventory and format stay unchanged. Consumer typing and runtime contract tests
cover the new field. A generic result carrier avoids an evidence-to-frames import cycle while
the framed entry points specialize it to `FramedRecognitionResult`.

## Amendment (principal-axis rectangular recesses, issue #320)

An `ORTHOGONAL` frame may assign any equivalent physical direction to local X, Y or Z. Existing
principal-axis `Pocket` and `Slot` semantics therefore cannot use world Z or XYZ iteration order to
choose a floor, opening or depth. Corner interruptions use the uniquely shallowest complete
physical leg as depth. Opposed-wall floored recesses evaluate both perpendicular interpretations
and accept exactly one. A tied corner leg or multiple valid floored interpretation remains an
explicit refusal under the existing coordinate floor.

This changes record values where the legacy result encoded presentation rather than physical
geometry, but does not change the record schema, tolerance policy, defining-face ownership or
reconciliation authority. Internally oblique features in an otherwise established part frame
remain outside the principal-axis contract. The exact authored and MFCAD++ evidence is recorded in
the [rectangular recess frame-axis audit](../benchmarks/e2-rectangular-recess-axis-audit.md).

## Amendment (principal-axis rectangular pads, issue #331)

The framed aggregate may map a physical rectangular island's attachment direction to any signed
local principal axis. Pad discovery therefore evaluates those directions on the exact normalized
working shape and returns the orientation in `RaisedPad.axis` and `RaisedPad.direction`; the
framing policy does not reorder the representative for this family and consumers must not probe
alternate rotations. The record's XYZ bounds, orientation, aggregate evidence and paired
`FramedRecognitionResult.part` share one local coordinate system. Internally oblique islands remain
outside this principal-axis contract. The exact authored and MFCAD++ evidence is recorded in the
[rectangular-pad axis validation](../benchmarks/e2-rectangular-pad-axis-validation.md).

## Amendment (body-local Plate occurrences, issue #334)

A framed working shape may contain multiple valid solids. Plate recognition therefore derives its
area denominator, thickness envelope, coplanar groups, adjacent events and value deduplication per
solid in that exact local shape. It then orders all physical occurrences geometrically. Equal
records on separate bodies remain multiplicity-distinct; their `u`/`v` witnesses, defining faces
and `SolidRef` ownership never cross a body boundary. This changes compound output only: the Plate
schema, principal-normal scope, thresholds and framing representative remain unchanged.

## Amendment (Plate in-plane roll covariance, issue #329)

An `AXIAL` or otherwise geometry-established frame may choose a different gauge about a physical
Plate normal. Plate large-area eligibility therefore uses a body-intrinsic oriented transverse
envelope rather than the local coordinate-aligned bounding rectangle. The authority is derived in
the exact framed working solid and rotates with that solid; the frame representative is not
reordered or probed again. Axis, bounds, witnesses and original defining faces remain expressed in
the one supplied local frame, and internally oblique Plate normals remain unsupported.

## Amendment (internally oblique Slot directions, issue #310)

An internally slanted slot does not redefine the part frame. `OrientedSlot` expresses width,
long, run and centre in the one coordinate system supplied to recognition: raw for raw calls and
the selected local frame for framed calls. Whole-part covariance transforms them together; no
family-specific reframing or world-axis fallback is permitted.

## Amendment (framed body correlation, issue #390)

Body keys are derived after framing, from the exact working solids that produce the records. A key
therefore belongs to the returned local coordinate system and is shared coherently by all
body-owned records; it is not promised equal to the corresponding raw/world key. Consumers compare
keys only within one result and do not use them as cross-run persistent identity.
