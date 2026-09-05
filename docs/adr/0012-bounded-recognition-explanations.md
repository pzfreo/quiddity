# ADR 0012 — Publish bounded explanations beside recognition results

- **Status:** Accepted
- **Date:** 2026-08-28
- **Decider:** Paul Fremantle
- **Evidence:** Epic 0005 E0 baseline, issue #295, ADRs 0003/0004/0007/0011

## Context

The public aggregate returns an immutable `RecognitionResult`, but an empty tuple cannot tell a
consumer whether a family ran and found nothing, was classification-gated, proposed candidates
that reconciliation rejected, or encountered one of the few failed predicates that currently
emits bounded evidence. The private `InventoryProduct` already distinguishes these states, but it
cannot be exported without exposing candidate identity, graph-local evidence and reconciliation
authority.

An explanation is necessarily incomplete. Most predicates deliberately fail closed without
issuing an Observation, and the lazy effective-surface index contains only facts requested by
actual consumers. Scanning faces or rerunning recognisers after the terminal inventory would make
the diagnostic path a second recognition authority.

## Decision

Publish an additive `build_recognition_report(part, *, cylinders=None, rotational=False)` route.
It executes the existing aggregate orchestration exactly once and returns an immutable report
containing:

1. the unchanged `RecognitionResult` from that run;
2. one entry for every closed physical family, ordered by the registry's execution roster;
3. whether that family was `evaluated` or `not-applicable` for the exact run;
4. proposed, accepted and rejected candidate counts plus closed disposition reason summaries; and
5. public value projections of the residual diagnostics already produced from frozen evidence.

The report publishes `ExplanationCoverage.BOUNDED`. This is a semantic warning, not a confidence
score: a missing diagnostic means only that no supported bounded diagnostic was established.
Evaluated with zero proposals means the family completed without candidates; it does not prove
that the part contains no related unsupported geometry.

Public explanation types contain only strings, enums, numbers, tuples and the existing result.
They never expose `InventoryProduct`, `Candidate`, `Observation`, `FaceNode`, graph identity,
evidence handles or related-candidate identity. A disposition summary includes its closed reason,
outcome, occurrence count and total related-occurrence count. It does not manufacture a causal
tree from overlap.

`RecognitionResult` gains no field, preserving construction, equality, typing and positional
compatibility. The new route is raw-frame like `build_recognition_result`: diagnostic coordinates
have the same caller/world coordinate meaning as its records. ADR 0011's opt-in framed result is
unchanged. A framed explanation requires a separately paired frame, working shape and report; it
is deferred rather than returning local coordinates through a raw-looking value.

Surface provenance/refusal summaries are also deferred. Reading the private lazy cache would
publish an accidental implementation subset; filling it would force geometry queries not made by
recognition. A future summary needs a reviewed consulted-evidence contract, not cache inspection.

No JSON schema is published in this increment. Immutable Python value types are the compatibility
surface. A future serialized form requires an explicit format version and canonical contract.

## Authority and module boundary

The projection runs only after evidence freeze and completed reconciliation. Per-family
applicability is evaluated from the closed registry predicate against the exact existing run
context; it cannot create, accept or reject a candidate. Counts are derived from issuer-validated
candidate sets and final dispositions.

Private `_diagnostics` remains geometry-free and retains its ADR 0007 signature. A separate
projection module may translate its stable primitive values into public equivalents, but cannot
import geometry, call a recogniser or receive mutable evidence. Registry declarations remain
internal execution metadata; the public roster is manually tested against them rather than
generated into exports.

## Consequences

- Consumers can distinguish not-run, clean evaluated absence and reconciliation loss without
  depending on private state.
- Existing callers and `RecognitionResult` remain source and value compatible.
- Explanations are honest but not exhaustive; documentation and type names retain that boundary.
- No recognition predicate, tolerance, candidate identity or reconciliation precedence changes.
- MFCAD++ accepted records and score-vector counts must remain identical; only report projection
  cost is new and is paid only by callers of the new route.
- Framed explanations, surface-read summaries and serialized schemas remain explicit future work.

## Amendment (paired framed report, issue #317)

`build_framed_recognition_report` now supplies the previously deferred paired lifecycle. A
successful `FramedRecognitionReport` owns one `PartFrame`, the exact topology-preserving local
working shape, and the `RecognitionReport` produced by one aggregate run on that shape. Its records,
diagnostics and evaluated topology therefore share local coordinates and identity authority.

`PreparedFramedPart.recognise_report()` reuses the prepared cylinder substrate after a consumer's
local classification decision. A `RefusedPartFrame` remains the complete refusal result; there is
no automatic raw fallback. `build_raw_recognition_report` names intentional caller-coordinate
reporting, while `build_recognition_report` remains its 0.4.x compatibility alias.

## Amendment (evidence and explanations from one run, issue #494)

`RecognitionEvidence.report` now retains the existing immutable `RecognitionReport`, projected
from the same completed private inventory as the accepted face evidence. Its `report.result`
is the identical object as `view.result`. `FramedRecognitionEvidence.report` delegates to that
same report, with local coordinates paired to the view's exact working part and frame.
Preparation and classification remain unchanged; no second inventory or cylinder scan occurs.

The report is projected eagerly while the inventory is available. Evidence views retain only
the public report, never the private product, rejected candidates or mutable execution context.
Result-only and report-only builders remain unchanged. Evidence consumers pay the bounded report
projection cost; simply accessing `report` does no work and returns the same immutable value.

Explanations describe detector-family proposals and dispositions. Unified SectionRecess public
occurrences may combine those detectors, so report counts need not equal the public feature
roster. `BOUNDED` remains mandatory: neither empty diagnostics nor unassociated faces prove
absence of missed features. This addition does not introduce a JSON report format, new manifest
symbols, or a result schema change.

### Rejected alternatives for the shared lifecycle

- Reconstruct explanations from an accepted-only result: rejected candidates and observations
  are no longer available there.
- Retain the private inventory lazily: unnecessarily extends private execution-state lifetime.
- Add another recognition builder or an `explain=True` mode: existing evidence consumers can
  obtain the report directly without another lifecycle or union of return types.

## Previously rejected alternatives

- **Add diagnostics to `RecognitionResult`:** changes its central construction/equality contract
  and makes every existing result look causally complete.
- **Expose `InventoryProduct`:** leaks run-local authority and makes private lifecycle objects a
  public compatibility surface.
- **Scan residual faces after recognition:** creates a second geometry consumer and turns absence
  into an unreviewed classifier.
- **Report the effective-surface cache:** its contents depend on which lazy consumers happened to
  query it and therefore are not a stable statement about the part.
- **Attach raw explanations to `FramedRecognitionResult`:** risks confusing local diagnostic
  coordinates with caller coordinates and broadens ADR 0011 without a paired working-shape API.
