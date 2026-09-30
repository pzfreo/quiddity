# ADR 0026 — Versioned correspondence receipts resolve faces and accepted features

- **Status:** Accepted
- **Date:** 2026-09-30
- **Decider:** Paul Fremantle

## Context

PMI authoring stores durable intent against accepted manufacturing features and against ordinary
source faces such as datum and flatness surfaces. A later revision produces a new B-rep and a new
recognition run. Run-local `FeatureRef` and `FaceRef` values correctly cannot cross that boundary,
while public-record equality, traversal indices and nearest geometry can silently attach a
tolerance to the wrong symmetric occurrence.

FreeCAD and OCAF obtain their strongest persistent names from modeling-operation history. A plain
STEP revision usually contains only the resulting B-rep, so Quiddity needs a correspondence
protocol that can use source history when present and geometry/topology when it is absent. The
previous F6 experiment implemented complete body descriptors and edited-model relations for one
uncounted family before a consumer existed; it was removed. Its order-neutral and fail-closed
principles remain valid, but its implementation is not restored.

## Decision

Publish `quiddity.correspondence` as an optional sidecar over completed recognition evidence. A
versioned strict-JSON `CorrespondenceReceipt` addresses one accepted feature, one source face, or
one explicit non-empty source-face set. The caller supplies a stable model-lineage string at issue
and resolution time. Lineage prevents accidental comparison of unrelated documents; it is an
assertion by the caller, not proof of geometric correspondence.

Consumers persist and return the complete receipt but do not interpret its provider-owned payload.
The envelope declares its format, subject kind and strategy. Face indices, STEP entity numbers,
geometric descriptors, checksums and thresholds are never public identity. A future strategy may
use modeling history, a stronger deterministic graph matcher, or learned candidate generation
without changing receipt storage or resolution outcomes. Old strategies may coexist or refuse as
incompatible; they never silently reinterpret an old payload.

Resolve receipts in a batch against one supplied current `RecognitionEvidence` or
`FramedRecognitionEvidence`. Resolution does not start recognition. A global one-to-one assignment
prevents two prior subjects claiming one current subject. Each input receipt returns in input order
with one closed status:

- `resolved`: exactly one current subject survives every maximum assignment;
- `missing`: no compatible current subject exists;
- `ambiguous`: alternatives or competition prevent one unique assignment;
- `incompatible`: lineage, format, strategy or supported-domain requirements do not agree.

Only `resolved` exposes current-run references. Face subjects return `FaceRef`s; feature subjects
return one `FeatureRef`. An unresolved result exposes no guessed reference or confidence score.

### `analytic-v1`

The first strategy is deliberately bounded:

- planar faces whose body-owned outer wire contains finite lines/circular arcs;
- explicit sets of those faces;
- accepted `holes` features;
- revisions compared in the same evidence coordinate/frame contract.

A planar descriptor retains its canonical outward plane and complete ordered outer boundary but
excludes inner wires. A datum face therefore survives an unrelated hole added through it, while
two equal/coincident candidates remain ambiguous. Source coordinates use the existing outer-profile
proof boundary: model-length coordinates are rounded to six decimal places and unit directions to
eight. These are representation-noise bounds, not manufacturing tolerances.

Hole matching uses ordered proof levels. Exact complete records win first. If none exists, one
candidate at the same axis and opening with the same bottom and nested treatments may change bore
diameter/depth. If none exists, one candidate may move while retaining the complete machining
specification. No distance chooses among equal candidates. Batch assignment decides uniqueness.

V1 does not promise arbitrary rigid re-registration, freeform faces, non-hole families, or
split/merge correspondence. Issuance outside its domain fails explicitly. A supported receipt with
no current candidate is `missing`; a receipt from another strategy or lineage is `incompatible`.

Receipt payloads carry a truncated SHA-256 integrity checksum to reject accidental or unsophisticated
payload modification. This is not authentication and does not make caller-controlled data trusted.
All decoded fields are validated as untrusted input before matching.

## Enforced by

- `tests/test_correspondence.py`, the authored revised-geometry corpus described in
  `docs/benchmarks/485-correspondence-corpus.md`: datum faces with changed inner loops, face sets,
  unchanged/resized/moved/added/removed holes, coincident and symmetric ambiguity, global
  competition, non-enumerative larger batches, strategy and lineage mismatch, and malformed
  receipts.
- `correspondence_api.json` and its installed-wheel/package/version tests.
- Architecture guards keep correspondence out of discovery, reconciliation and result records;
  the resolver consumes a completed evidence view and cannot construct a run.

## Consequences

Draftwright and pmi-assist depend on one durable protocol rather than `analytic-v1`. Improving
matching changes strategy payloads and corpus evidence, not their storage model. Datum faces and
accepted features share the same lifecycle and refusal vocabulary. Weak matches ask for a user
re-pick rather than moving PMI silently. V1 provides useful revised-model correspondence while
leaving rigid registration, broader feature adapters and split/merge semantics to independently
reviewed strategies.
