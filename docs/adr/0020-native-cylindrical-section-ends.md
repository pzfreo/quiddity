# ADR 0020 — Native cylindrical SectionRecess ends

- **Status:** Proposed; private proof reviewed, public implementation pending
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

Those results establish feasibility, not production completion. Deliver authored
public-path tests (including the breakout), tolerance-boundary and serialization
tests, exact reconstruction, MFCAD++ before/after evidence, independent substantive
review and required green CI before accepting this ADR as implemented.

## Deliberate exclusions

This does not solve #538: holes piercing walls require independent interaction
proofs. Nor does it introduce stepped or piecewise-planar end surfaces for #540.
Those need their own physical boundaries; do not substitute a common-core interval,
envelope, arbitrary internal closing surface or general-purpose surface framework.

Consumers may dimension the footprint and report maximum, centroid or local depth
with explicit semantics. They must not call a varying physical depth uniform.
