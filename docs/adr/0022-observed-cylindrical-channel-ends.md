# ADR 0022 — Observed cylindrical channel terminations

- **Status:** Proposed; architecture reviewed, authored feasibility established
- **Date:** 2026-09-06
- **Issues:** #538 (remaining end-intersecting bore), #369, #290

## Anatomy and decision

The reduced Draftwright #1166 fixture has three original channel supports at
`x=12.61` and `y=±1`. They all terminate on observed convex edges of the same
native bore cylinder, whose axis runs along X through `(y,z)=(0,66)`, radius 4.
The actual upper interface is `z=66-sqrt(16-y²)`: its centroid is Z=62, its wall
tips are Z=62.12701665, and there is no Z=62.13 planar cap left.

Represent this as the existing SectionRecess channel: a physical U-profile in XY,
open lower planar end at Z=0, open upper **negative-branch** cylindrical end,
and explicitly absent lateral support at X=13.55. It is not a flat repaired cap,
an arbitrary common-core interval, or an invented arc wall across two voids.

No new surface type is needed. Extend ADR 0020's admissible combinations narrowly:
the existing cylinder value may describe this open-channel termination, and its
reversed-run equivalent. Keep three concepts independent:

- `open` / `capped` is the termination topology;
- `positive` / `negative` chooses the cylinder's mathematical branch;
- `low` / `high` orders the occurrence along its canonical run.

Keep the existing closed-pocket restrictions intact. Initially the new channel
combination has one open planar end with zero gradient, one open cylindrical end,
and an origin-centred three-line U-profile whose absent closure defines one simple
rectangular probe domain. A high cylindrical end uses the negative branch; the
reversed-run low end uses the positive branch. This does not admit every open
chain or every cylinder/end-condition combination.

## Proof and publication gates

- The complete original three-support trim agrees bidirectionally with the exact
  clipped reconstruction. Every terminal support has observed adjacency to the
  same native cylinder; nearby or coaxial substitutes are not authority.
- All original support and terminal context belongs to one valid solid.
- The branch exists over the whole rectangular probe domain. Prove strict
  low/high separation including interior extrema; wall endpoints alone miss the
  central Z=62 minimum in this example.
- The finite opposite planar end comes from original termination context, not a
  fitted interval. Base volume, both end openings and the lateral opening are empty.
- Temporary probe closure remains absent physical support. Original channel
  supports are constituent evidence; bore/stock terminal context is not. Existing
  independent bore occurrences remain separate.
- Publish the centroid intersection in `run_interval`, not maximum wall extent.
  Keep the existing whole-occurrence serialization displacement limit; include
  source-axis tilt and cylindrical curvature amplification over the full domain.

## Evidence and exclusions

An authored exact-volume prototype covers scales 0.1, 1 and 10 under identity,
arbitrary rigid rotation and run reversal. The base removal volume is
116.63908461722733 mm³ at unit scale. Complete support, same-owner termination and
all three openings pass. Reducing the bore radius to 3.99 leaves planar cap
remnants; that case fails support, empty-volume and common-cylinder termination
proofs and must not be forced into a single cylindrical end.

The bounded public validator and private original-source proof are implemented
and independently reviewed, but are not yet connected to public discovery.
Eighteen authored proof checks cover scaled/principal-transformed geometry, STEP
round-trip, partial cap refusal, each material/opening probe and an oblique bore
at, inside and outside corner tangency. The latter exposed an area-only proof
gap: strict cylinder-domain validity is now checked explicitly at all footprint
corners using the exact coordinate perpendicular to both cylinder axis and run.
Legacy constituent hints can contain exterior stock at small scales because of
rounded bounds. The proof selects the actual concavely adjacent backing face
and requires complete support equality; only its proved supports may be published.

Before production: integrate discovery and bounded publication, consumer JSON
reconstruction, additional transformed/tolerance/ownership fixtures,
and negatives for touching ends, split terminal authority, partial cap remnants,
material/lateral obstructions and unmatched wall extents. Then independent review,
open-development comparison and required green CI. No MFInstSeg input is needed.

This does not generalise the support-aperture exception of ADR 0021 to capped run
ends, infer machining history, or claim full treated-boundary reconstruction from
a base feature alone.
