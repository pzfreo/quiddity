# ADR 0024 — Observed convex two-plane passage ends

- **Status:** Proposed; independent architecture review clear, production proof pending
- **Date:** 2026-09-06
- **Issues:** #540, #369, #290

## Problem and evidence

A polygonal passage can open through a continuous convex roof made of two
nonparallel planes. Its wall junctions do not terminate in one plane, even
though every physical wall and both openings are unambiguous. It is neither
a discontinuous stock step nor an arbitrary maximum-wall envelope.

MFCAD++ model 10545, wall group 11–16, has one complete planar mouth and two
roof faces (34/35) sharing an original convex ridge. The latter touch three
and five walls respectively, with a collective complete ring, and belong to
the same valid solid. This motivates a hypothesis, not a dataset-derived rule.

An independent construction clips a 40 mm cube below the plane through
`(0,0,20)` with normal `(0.2,0,1)`, retaining the original flat roof on the
other side. A radius-3 regular triangular, rectangular or hexagonal Z-through
cut crosses the roof ridge. The exact clipped-cell experiment passes for
all three profiles at scales 0.1 and 1, translated and arbitrarily rotated:
twelve constructions. Unit-scale removed-cell volumes are 466.614487559,
718.2 and 932.579456065 mm³. Original wall equality, both roof terms, the
opposite mouth and empty cell/opening probes pass. Current recognition misses
all three authored profiles.

## Proposed public geometry

Add a kernel-free `PlanarEnvelopeEndSurface` with JSON discriminator
`type="plane_envelope"`, `operator="min" | "max"`, and exactly two plane terms.
Each term contains a six-decimal `height` at section origin and a six-decimal
two-component `gradient`. Coordinates are the existing SectionRecess local
frame. The terms are canonically ordered by `(height, gradient)`.

For a point `(u,v)`, the end height is the selected minimum/maximum of
`height + gradient[0]*u + gradient[1]*v`. The centroid entry in `run_interval`
is that value at `(0,0)`, rounded to the existing interval precision. Plane
heights are absolute local run coordinates, like the placement of cylindrical
ends, not offsets silently anchored to a different plane.

Initially admit only a closed convex line-only polygonal passage with both
ends open, one zero-gradient planar end and one two-plane envelope. A high
envelope uses `min`; its run-reversed low equivalent uses `max`. This records
a continuous convex stock ridge, not a concave valley, disconnected step,
generic CSG expression or arbitrary piecewise surface. Existing single-plane,
cylindrical pocket and cylindrical channel/passage contracts remain unchanged.

Both terms must contribute a positive-area patch inside the profile. Refuse
duplicate, parallel, globally dominant, collapsed or boundary-only terms.
Require strict separation from the opposite plane over the complete domain;
for this bounded combination the critical separation is attained at profile
vertices. Check centroid consistency separately. The interval is not a claim
of uniform depth or a maximum height.

## Original-source proof

- Recover one connected original concave wall cycle and a simple convex
  section using original straight junctions. No expected side count, angular
  template or label enters discovery.
- Identify the observed complete planar opposite mouth. The two other
  original planes collectively touch every wall through convex edges; they
  share an original convex ridge and have outward normals with the correct
  nonzero run component. All walls and contexts have one valid owner.
- Reconstruct the exact cell by clipping a private bounded prism against
  both observed plane half-spaces. The prism ceiling is only a construction
  aid: no artificial cap may remain. Both selected roof terms must contribute
  positive-area end patches in the final cell. Every generated terminal patch
  must match exactly one selected observed plane (or the opposite mouth).
- Require one valid positive cell, complete opposite-mouth equality and
  bidirectional equality of every original wall with reconstructed lateral
  patches. Every original terminal wall boundary must agree with the selected
  mouth/roof support; extra unmodelled terminal context is not ignored.
- Prove the cell and both outward opening probes empty in the owning solid.
  Material bridges, blind cavities, lateral breakouts, incomplete support,
  cross-body borrowing and unrelated nearby roof planes must be refused.
- Publish original walls as defining/constituent evidence. Roof planes and
  ridge remain contextual authority, not newly claimed feature faces. Generated
  clipping geometry is never evidence. Preserve independent interacting
  features and normal same-owner reconciliation.

## Projection and consumer checks

Preserve the existing 0.002 mm whole-occurrence displacement limit. For each
plane, bound height/gradient rounding over the entire profile, along with
profile and frame projection. Evaluate the bound across corresponding raw and
public profile points, not merely identical coordinates before and after
rounding. Minimum and maximum are 1-Lipschitz in their
term values: a uniform bound for both terms bounds the resulting end graph,
even if rounding moves the crease. Do not substitute a fitted single plane.
Revalidate active patches and all public invariants after rounding; candidate
projection refusal must not discard unrelated recognition.

Tests must reconstruct the volume from JSON without private recognisers.
Include several ridge positions and slopes, run reversal, arbitrary transforms,
scales, STEP round trips, compound ownership, physical obstruction and missing
support, wrong/concave ridge, inactive/tangent terms and rounding boundaries.
In particular, a concave valley with the same overall bounds must not pass
as the convex roof.
Independent architecture and implementation review, before/after MFCAD++
coverage and defining evidence, and required CI remain delivery gates.

## Scope not decided here

This does not resolve the hidden-groove prior-stock ambiguity, partial transverse
bores, missing wall cycles, general treated boundaries, two envelope ends, or
arbitrary numbers of roof planes. It does not claim the remaining #540 residual
is unsupported or that one development example predicts full-corpus reach.
No MFInstSeg anatomy or feedback is needed to define or test this contract.
