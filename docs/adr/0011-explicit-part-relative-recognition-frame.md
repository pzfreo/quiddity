# ADR 0011 — Pair local recognition with an explicit part frame

- **Status:** Accepted
- **Date:** 2026-08-27
- **Decider:** Paul Fremantle
- **Evidence:** [frame-handling evaluation](../benchmarks/frame-handling-prototype.md),
  [rectangular recess axis audit](../benchmarks/e2-rectangular-recess-axis-audit.md),
  [rectangular pad axis validation](../benchmarks/e2-rectangular-pad-axis-validation.md)
- **Record form:** current state, rewritten 2026-09-15. The per-issue amendments this file
  carried until then are in `git log -- docs/adr/0011-explicit-part-relative-recognition-frame.md`.

## Context

Several predicates and reconciliation choices are interpreted in the recognition frame's XYZ. A
rigid X30-plus-translation presentation removed 1,571 of 2,784 occurrences in the first 500
MFCAD++ development models: the same part gave a different inventory because of its STEP placement.
Making every recogniser free-axis would spread frame policy and new schemas across the package;
hiding normalisation inside the existing entry point would silently change what axis letters mean.

## Decision

**An explicit, geometry-established frame.** `infer_part_frame` returns a right-handed
`PartFrame(origin, x, y, z)` or a typed `RefusedPartFrame`. Inference is closed: `FrameGauge`
records what geometry established (`FULL`, `ORTHOGONAL`, `AXIAL`); a remaining roll or axis
assignment is a deterministic representative, never a semantic material axis. Refusal never
falls back to raw recognition.

**The framed route is the ordinary aggregate route.** `prepare_framed_part` infers the frame,
places the part by rigid `TopLoc` (changing evaluated coordinates without rebuilding topology) and
scans cylinders once, returning a `PreparedFramedPart`. The consumer derives any local
classification from that exact value and calls `recognise(rotational=...)`, `recognise_report()`
or the evidence variant; each runs the one aggregate. The successful value owns the frame, the
exact working shape and the `RecognitionResult`; every coordinate, record and axis letter in it is
local to that frame. `build_framed_recognition_result` delegates through the prepared lifecycle.

**The raw route is named.** `build_raw_recognition_result` operates in caller coordinates.
`build_recognition_result` is its compatibility alias and will never silently become framed.

**Families are covariant with the supplied frame; none reframes.** An `ORTHOGONAL` frame may map
a physical direction to any local principal axis, so a family must not assume Z or resolve ties by
XYZ iteration order: Polygonal Stock and Boss accept X, Y or Z and carry `axis`; Raised Pads
evaluate all six signed directions and carry `axis` and `direction`; rectangular recesses evaluate
both perpendicular interpretations and accept exactly one, refusing a tie; Plate eligibility uses
a body-intrinsic transverse envelope that rotates with the solid. Internally oblique geometry stays
outside the principal-axis contract, except `OrientedSlot`, which expresses its directions in the
one supplied frame. Body keys are derived after framing and are local to that result.

**Compounds.** A working shape may hold several valid solids; body-owned families scope
denominators, grouping and deduplication per solid and keep equal records on separate bodies.

**Not exposed.** The boundary publishes no `GeometryGraph`, Candidates, correspondence or
recogniser internals. Recognition still executes once through the registry and reconciliation.

## Evidence

The 20-fixture golden inventory is invariant occurrence by occurrence under Z30, X30, X90 and
translation after independent inference (75/75). On the first 500 MFCAD++ test-split models all
infer a full frame; framed X30-plus-translation retains all 2,750 baseline occurrences with one
extra Slot fragment on one model, recorded as a bounded limitation. Inference and normalisation
cost 3.7% of framed recognition time.

## Enforced by

- Frame contract tests over the gauges and refusal, on the Linux, macOS and Windows matrix.
- Per-family axis tests for the covariance rules above, with the checked-in MFCAD++ reports.
- Legacy goldens and public tests unchanged by the framed route.

## Consequences

Callers separate placement from recognition semantics without a free-axis record migration.
Unconstrained roll is explicit gauge rather than a hidden axis. The cost is that every family
migrating onto the framed route must be audited for a hidden Z or XYZ-order assumption, and that
the raw alias must live until its removal is a release decision of its own.
