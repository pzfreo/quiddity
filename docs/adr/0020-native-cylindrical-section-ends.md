# ADR 0020 — Native cylindrical SectionRecess ends

- **Status:** Accepted; implementation merge remains gated on required CI
- **Date:** 2026-09-06
- **Issues:** #541, #369, #290; related #540 and #538

## Problem

A constant rectangular pocket in cylindrical stock has constant wall-support planes
but a curved mouth. In #541 its footprint is 6 × 24 mm, floor is Z=8 and mouth is
`Z=sqrt(400-Y²)`. Depth is 8–12 mm. Publishing a uniform 12 mm physical prism would
include 183.59733889611766 mm³ of exterior air. Refusal preserves honesty but does
not preserve the downstream drawing capability.

## Proposed decision

Extend the single SectionRecess representation with an explicitly discriminated
analytic end surface. Keep end condition (`open` or `capped`) separate from surface
kind (`plane` or `cylinder`). Existing planar geometry keeps its physical meaning.

A cylindrical end carries its axis placement and direction in the section frame,
radius, and explicit positive/negative run branch. Initially admit only a native
cylinder with axis perpendicular to the run, a planar floor, and a closed polygonal
profile. The existing profile supplies the complete end domain; do not duplicate
it or embed build123d/OCP objects. Retained run-interval values remain intersections
on the section-centroid run line and must agree with the end surfaces.

The public discriminator, canonicalization, schema versions, manifest entries,
whole-occurrence serialization error bound and exporter/consumer reconstruction
tests must be delivered together. Do not silently add cylinder parameters to a
record that existing readers interpret as a plane.

The implementation uses document version 3 and explicit nested surface records.
Cylindrical profile coordinates use the existing four-decimal allowance; axis,
radius and cylinder-placement values use six decimals. Publication retains the
0.002 mm whole-occurrence displacement limit. Its bound includes the source-axis
perpendicularisation error over the complete axial footprint, cylinder-parameter
and profile rounding, and frame rounding. A small angular tolerance alone cannot
justify flattening a long, slightly tilted cylinder. A tighter paired-coordinate
bound is used only for convex original and serialized profiles; other profiles
retain a conservative bound.

## Physical proof obligations

- An original planar floor supplies one complete line-bounded profile.
- Every floor edge has original corresponding wall support. Area equality alone
  is insufficient: a sideways breakout can collapse a missing wall to zero area.
- All original wall planes are constant along the proved run. Their finite source
  areas and reconstructed clipped wall areas cover one another, without gaps.
- Original termination context proves one native cylinder and one valid owner;
  every wall has observed convex termination on that cylinder.
- The selected branch exists and stays strictly above the floor over the complete
  profile, including its boundary. For a planar polygonal floor and perpendicular
  cylinder axis, the maximum absolute transverse offset occurs at a vertex.
- Clipping must not reduce the floor domain. Compare complete floor support in
  both directions as well as checking strictly positive separation.
- The complete clipped interior is empty, the floor is backed by material, and
  an outward-run slab beyond the complete cylindrical mouth is empty.
- Stock faces are consulted context, not pocket constituent evidence. Generated
  clipping faces never become original face references.

Production must consume the existing internal analytic substrate with native
provenance enforced, not depend on the public inspection facade used in the scratch
experiment. Recovery, sphere/cone ends, interrupted support and ambiguous branches
are not implicitly admitted.

## Evidence and delivery gate

The private JSON-value reconstruction passed centred and offset cases at three
scales and three rigid placements. The source-face proof passed twelve positive
cases, six negatives, distinct compound ownership and STEP round-trip. Independent
review found a side-breakout false positive; complete edge support and strict
boundary separation fixed it, and focused re-review was clear.

The production implementation is in PR #544. Authored public-path tests include
the breakout, a source-axis tilt exceeding the publication bound, and JSON-only
consumer reconstruction across eighteen scaled, offset and transformed cases.
The reconstructed volumes and sampled physical boundaries (including curved
interiors) agree within the unchanged publication allowance. Sampling supplements,
not replaces, the analytic whole-occurrence bound. Four additional authored
triangular/hexagonal cases retain complete original wall evidence.

Independent whole-branch and focused closure reviews have no remaining blocking
findings. The full same-selection MFCAD++ comparison at `88cd359` versus
main-equivalent `154182c` evaluated 2,493 of 2,500 models with the same seven invalid
inputs: zero changed model results, coverage/defining losses or summary changes.
This is a proved consumer capability repair, not a claimed corpus recall increase.
See [the validation record](../benchmarks/e5-cylindrical-pocket-541.md).
Required green CI remains the implementation merge gate.

## Deliberate exclusions

This does not solve #538: holes piercing walls require independent interaction
proofs. Nor does it introduce stepped or piecewise-planar end surfaces for #540.
Those need their own physical boundaries; do not substitute a common-core interval,
envelope, arbitrary internal closing surface or general-purpose surface framework.

Consumers may dimension the footprint and report maximum, centroid or local depth
with explicit semantics. They must not call a varying physical depth uniform.
