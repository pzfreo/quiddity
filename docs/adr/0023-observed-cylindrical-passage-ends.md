# ADR 0023 — Polygonal passages ending on an observed bore

- **Status:** Accepted; independent architecture and implementation review clear
- **Date:** 2026-09-06
- **Issues:** #540, #369, #290

## Decision

Extend the existing SectionRecess end combinations to a closed, convex,
line-only polygonal passage with one open planar end perpendicular to the run
(zero local gradient) and one open native
cylindrical end. The cylindrical end is the negative branch when high, or the
positive branch when low. Both ends are open; the profile is closed. This is
distinct from the capped, external-cylinder pocket of ADR 0020 and the laterally
open U-channel of ADR 0022. No new public surface fields are proposed.

The record describes the connected void bounded laterally by the complete
original polygonal wall ring, from its observed planar mouth to its observed
intersection with a cross-bore. It does not assert a machining operation, fill
the cross-bore, combine opposite passages through the bore, or publish the bore
surface as a polygonal wall. The independent bore remains a separate feature.

Admit exactly the new combination: closed convex polygon, both ends open,
zero-gradient planar end, and high-negative/low-positive cylinder branch.
Do not weaken the existing external-cylinder pocket restrictions. A sloped
planar mouth requires an additional varying-plane separation proof and is not
included in this initial extension.

## Evidence motivating the bounded scope

The open-development investigation of #540 found two six-wall groups in
MFCAD++ model 12354, each sharing a planar mouth and a single native cylindrical
termination. That identifies a candidate anatomy, not a proven detection or
an estimate for the full corpus. No MFInstSeg geometry or diagnostic feedback
was used.

An independently authored construction is a 40 mm cube, a radius-8 Y-axis
through-bore, and a radius-3 triangular, rectangular or hexagonal X-axis
through-cut. Each side of the bore has a distinct complete polygonal wall ring.
The initial exact-cell experiment proves both regions for all three profiles,
at scales 0.1 and 1, with translation and an arbitrary (17, 31, 43) degree
rotation: twelve constructions, twenty-four regions. At unit scale their
per-region volumes are approximately 141.127130, 217.712177 and 283.374482 mm³.
The experiment checks bidirectional wall-patch equality, empty cell volume and
empty openings. It is not yet a production recogniser or a complete adversarial
validation suite.

## Required proof

Discovery uses the same-run original face graph and effective-surface query.
Labels and expected polygon side counts must not enter production predicates.

- Identify one connected, unbranched concave wall cycle. Every wall is an
  original plane parallel to the run, and every adjacent wall pair supplies its
  original straight junction. A degree-two count alone does not exclude several
  disconnected cycles. Recover the boundary by graph traversal, not angular
  sorting of an assumed profile. Prove a simple convex section.
- Every wall has observed convex adjacency to the same original planar mouth
  and native inward-facing cylinder. Their contexts and walls belong to one
  valid solid. The cylinder axis is perpendicular to the run within the
  existing source/projection tolerances; a nearby fitted cylinder is not authority.
- Build the exact half-prism from the observed planar mouth towards the bore
  axis plane, then subtract the native bore. Require one valid positive cell,
  the complete planar mouth patch, exactly the supported cylindrical branch,
  and no surviving artificial axis-plane cap.
- Require bidirectional equality between every original wall and generated
  lateral patches. A common core or envelope with missing lateral support is
  not an occurrence. Require a strict cylinder domain over the whole convex
  profile and strict separation from the planar end, including interior
  extrema rather than only the wall junction endpoints.
- Prove the cell and both outward opening probes empty in the owning solid.
  Refuse obstructed, blind, disconnected, partially capped or cross-body cases.
  Expected geometric/projection failures refuse this candidate, not the run.
- Publish only original wall evidence and original owner identity. Terminal
  context authorises the end geometry but does not become constituent wall
  evidence. Reconcile with existing passage candidates without duplicate public
  occurrences or weaker generic evidence matching.

## Public contract and validation

Keep the existing canonical local frame, origin-centred closed profile,
centroid run interval, and tagged cylindrical surface. Preserve the distinctions
between low/high order, open/capped topology and positive/negative branch.
Use the shared whole-occurrence 0.002 mm publication displacement bound,
including axis tilt, profile/frame rounding and curvature amplification.
Consumers must be able to reconstruct the region from JSON alone.

Before production, add several positive profiles, scaled and arbitrary rigid
transforms, STEP round trips, two-owner duplicates, unequal opposite passages,
strict-domain/touching-end boundaries, residual planar caps, obstructions,
missing/split support and ambiguous terminal authority. Test public round trips,
ownership, coexistence with the independent bore, duplicate reconciliation and
per-candidate failure isolation. Obtain independent ADR and implementation
review, full MFCAD++ before/after coverage and defining evidence, and green CI.

## Implementation evidence

The private source proof and shared bounded public projection are implemented.
Independent review cleared both. The retained authored tests include STEP round
trip, arbitrary rigid transforms, two-owner duplicates, a physical bridge that
refuses only the obstructed side, a partial-bore refusal, unequal opposite mouth
positions, and independent empty-volume gates. JSON-only reconstruction passes
for triangular, rectangular and hexagonal profiles at two scales and arbitrary
rotation. Existing cylindrical pocket/channel regressions remain green.

Native discovery now carries explicit passage classification; the scorer maps
that classification to the existing passage taxonomy instead of the historical
native-recess pocket default. No dataset labels influence discovery. The motivating
MFCAD++ model 12354 publishes both original six-wall groups; a full before/after
comparison and required CI are still needed before merge. This architecture
acceptance is not a claim that those delivery gates are complete.

## Explicit remaining scope

This does not settle the larger stepped/grooved-stock residual in #540.
An authored hidden-groove counterexample produces identical final solids with
different prior-stock removal volumes: final STEP cannot uniquely establish
that machining history. An observed wall description or explicitly justified
closure convention remains a separate architectural question. Neither the
counterexample nor this cylindrical increment closes #540 or #369.
