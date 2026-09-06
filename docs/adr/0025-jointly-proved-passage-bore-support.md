# ADR 0025 — Jointly proved passage and bore support

- **Status:** Deferred; geometrically feasible, insufficient measured priority for production implementation
- **Date:** 2026-09-06
- **Issues:** #540, #369, #290

## Problem and evidence

A transverse bore can split a polygonal passage wall into disconnected coplanar
pieces. Neither the complete unperforated passage-wall proof nor ADR0021's
complete interior circular-aperture proof describes that geometry. Requiring
each interrupted feature to prove complete support independently is circular:
each lacks precisely the boundary removed by the other.

MFCAD++ development model 12121 has a complete planar mouth, six polygonal wall
planes represented by eight original faces, a two-plane convex roof and one
native transverse bore. Its original wall pairs 19/33 and 21/32 are coplanar
fragments split by bore face 12. The bore is internal to the passage span, not
its terminal surface. This is distinct from ADR0023's whole-bore termination.

An authored radius-3 polygonal passage through a 40 mm cube clipped by a roof
plane of normal `(0.2,0,1)` intersects a radius-2 X-axis bore at `(y,z)=(2,0)`.
Triangle, rectangle and hexagon examples, including arbitrary placement, have
source-only joint-cell feasibility: original boundary equality, empty material,
observed mouth/rim matching and outward opening probes pass. Expected combined
removed volumes agree with the authored construction. Attached bridges refuse.
The same prototype reaches the motivating development model without labels or
construction history as recogniser inputs. This is not a production acceptance
claim or a corpus-wide gain estimate.

## Proposed decision

The full 2,500-input development opportunity scan found one candidate (model
12121) and eight previously uncovered six-sided passage wall faces. Seven
baseline-invalid models were explicitly excluded from the opportunity count.
The first-500 scan found the same candidate. These are prototype opportunities,
not a production scorer result. Prototype SHA256:
`368ca7ae62968690f690bd25fdff934163674f97b03fff2e10427df9a3ef1391`.

Independent architecture review supports the bounded approach with internal-only
end interruption and local fragment association; both conditions are exercised
by authored regressions. Twenty-two authored tests plus a separate disconnected
coaxial-bore negative pass. This slice is deferred for low measured reach, **not**
declared impossible, and it does not close #540 or #369. Revisit for concrete
consumer demand or evidence of a materially larger recurring opportunity.

Extend ADR0021's **base-support interpretation**, not its independent-aperture
predicate, with a separate bounded joint proof. Initially support one closed
convex line-only passage with a complete planar mouth, one convex two-plane
roof as in ADR0024, and one native inward transverse cylinder with two complete
original terminal rims. Do not weaken existing independently proved apertures.

Recover the passage section from original straight junctions in the mouth-side
wall cycle. Collect only same-owner, same-oriented-plane original fragments for
each wall support. Both observed roof terms remain active and share an original
convex ridge. The bore's axis, radius and finite ends come from its native
surface and complete original circular rims, with observed planar end context.
No model labels, expected side counts, global bounds or prior-stock history may
supply production dimensions or missing geometry.

Plane coincidence is not occurrence membership: associate fragments through
the seed's original concave-connected wall component, not every coplanar face
on the body. An unrelated coplanar recess must neither contribute support nor
poison an otherwise valid candidate.

This is an internal support interruption. Require positive separation between
the complete finite bore cell and every nominal passage opening cap before
forming their union. A bore touching or crossing a mouth or roof boundary is
outside this decision, even if positive-area roof patches survive.

Reconstruct both candidate cells privately. Their union must be one valid,
positive, empty cell whose entire non-opening boundary and selected original
wall/cylindrical evidence cover one another exactly. This is a direct joint
boundary proof, not mutual trust in two previously accepted occurrences.
Every generated opening cap must have unique original outward terminal-surface
authority. Match the complete original mouth wire and both bore rim disks;
require positive-area roof terms and empty outward probes at every opening.
All consulted original faces must belong to one valid solid. Do not join
disconnected bore segments across air or borrow support from another body.

## Public interpretation and evidence

Use the existing SectionRecess base profile, frame and end surfaces. As in
ADR0021, it describes base recess geometry with proved support interruptions,
not a promise that every nominal wall point exists in the final part. Neither
the union volume nor the bore-only volume is a passage-only volume. Do not
publish either as such or invent complete physical wall faces.

Original polygonal wall fragments are defining evidence. Those fragments and
the original cylinder that explains their interruption are constituents;
mouth/roof/bore-end stock faces remain context. No generated union face, cap or
disk becomes an original face reference. Existing independent hole recognition
is preserved, but this proof must not invent a Hole occurrence or depend on
one being separately accepted. Exact reconstruction of the fully interacting
boundary from the base recess record alone is not promised. A public feature
relationship API remains separate work.

Projection must preserve existing whole-occurrence displacement bounds and
all applicable public end/profile invariants. Joint proof failure or projection
refusal must not discard unrelated accepted recognition. Same-owner overlapping
candidate reconciliation must remain deterministic.

## Delivery gates and exclusions

Before production: authored scale and tolerance boundaries, STEP transport,
transforms, split-wall membership, same-looking separate bodies, missing rims,
blind/obstructed bores, lateral breakouts, unsupported roof/extra context and
unexplained support gaps; JSON-only base-geometry reconstruction; independent
architecture and implementation review; measured open-development reach and a
full before/after regression comparison; required green CI.

This does not admit arbitrary interacting void graphs, fitted cylinders,
partial/missing terminal rims, multiple bores, hidden-groove prior-stock
ambiguity or arbitrary piecewise roofs. Broader terminal combinations require
their own evidence. No MFInstSeg anatomy or iterative feedback is used.
