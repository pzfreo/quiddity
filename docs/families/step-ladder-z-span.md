# Explicit step-ladder Z-span boundary

- **Formerly:** ADR 0006, moved here 2026-09-15 as a family record. The number stays citeable;
  [its stub](../adr/0006-explicit-step-ladder-z-span.md) points here.
- **Modules:** `levels`

- **Status:** Accepted
- **Date:** 2026-08-16
- **Package review:** `b123d-recognisers` issue #19
- **Consumer review:** Draftwright integration following the 0.2.1 package release

## Context

`RecognitionResult.step_ladder(BoundBox)` was the aggregate's only operation whose public input
required a build123d object. The operation does not inspect topology or a complete bounding box. It
uses only the minimum and maximum Z coordinates, and only when excluding the two end faces of a
Z-turned profile. Prismatic levels were already envelope-filtered when the aggregate was built.

Current production callers are Draftwright's analysis/page-sizing path and independent prismatic-
coverage lint. Its package typing fixture and aggregate regression tests also exercise the public
contract. Both Draftwright callers already hold the part bounding box, so they can adapt its two Z
values at their consumer boundary without moving drawing or completeness policy into this package.

The literal `0.6` was an unnamed end exclusion. Its effective unit is the model length unit used by
the STEP/build123d geometry (conventionally millimetres). A shoulder exactly at that distance from
an end is excluded by the established strict inequality.

## Decision

Keep ladder projection on `RecognitionResult`. It chooses between two package-owned evidence sets,
`turned_steps` and `step_levels`; moving that choice into Draftwright would duplicate recognition
semantics across consumers.

Add `step_ladder_for_z_span(z_min, z_max, *, boundary_margin=0.6)`. Its public input is three typed
scalars, not a kernel object. `STEP_LADDER_BOUNDARY_MARGIN` names the shared default used by both
the earlier prismatic-level capture and this turned projection. The operation rejects
non-finite limits, reversed limits, and negative/non-finite margins. Equality at either inset end is
excluded, and a Z span no wider than twice the margin produces no turned rungs. Prismatic and non-Z
turned results preserve the already-filtered `step_levels` projection exactly.

Retain `step_ladder(BoundBox)` as a compatibility shim. It extracts the two scalar values, emits a
`DeprecationWarning` naming the replacement, and delegates to the new operation. It is deprecated
since 0.2.1, remains for the full 0.2.x line, and is removed no earlier than 1.0.0.

## Consequences

- New consumers need no build123d type to project an existing aggregate.
- Draftwright continues to own bounding-box acquisition, sizing, and completeness policy; this
  package owns only the geometry-evidence selection rule.
- The result remains a deterministic `list[float]`, preserving caller behavior. Tests prove repeat
  equality and JSON serialization; `RecognitionResult` and every contained record remain frozen.
- No recogniser, record schema, capability family, canonical golden, or recognition policy changes.

## Amendment (0.2.3, ADR 0008)

`boundary_margin` defaults to `None`, which resolves to `STEP_LADDER_BOUNDARY_MARGIN` capped at a
quarter of the span. An explicit float is still honoured literally, so every caller inventory above
is unaffected unless it relied on the default for a span shorter than 2.4 mm.

ADR 0008 asked whether this inset should scale with the part. It should not: it excludes an end
treatment, and a chamfer or edge break does not grow with the shaft. Deriving it proportionally
broke this ADR's own regression, which pins a 0.6 mm end step on a 10 mm part as something the
inset must drop. The cap is what an absolute constant needs to stay safe on a part modelled small,
and it preserves the requirement above that the prismatic capture and the turned projection share
one rule.
