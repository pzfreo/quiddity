# Shared geometry for ray queries

Each `RayCaster::for_solid` previously allocated face boxes, edge boxes, polyline
run boxes and lazy crack slots for the entire part. A context retaining classifiers
for many solids therefore retained many identical copies of those arrays. Building
another context for evidence repeated those allocations while the inventory's
context was still alive.

The part now owns these immutable derived values. Ordinary casters borrow its
default face boxes and edge query arrays. The exact same loops compute every bound
and every polyline run in the same order, without a tolerance or sample change.
Crack queries use all adjacent faces in the part, independent of a caster's selected
faces or its tolerance, so their existing lazy results can also be shared.

The selected face list, membership mask, hierarchy, root bounds, edge tolerance and
intersectability flag remain local to each caster. In particular, a classifier with
explicit conservative face boxes retains those supplied boxes and does not initialize
the default face-box cache. This preserves the orientation proof's lazy query path.

This changes storage ownership, not ray crossings or material classification. Boxes
remain only a pruning mechanism; the original surface intersections, trimmed-domain
tests, edge contact handling and parity votes determine the answer.

A transformed part is constructed with fresh derived caches. The existing import
inverted-shell and mixed-normal repairs invalidate the shared arrays along with the affected face
caches, so answers from the old domain orientation cannot survive it. The public
`Part` contract remains immutable after querying; callers changing topology must
construct a new part as before.

Memory for these arrays changes from one whole-part copy per live caster to one copy
per live part. The per-caster hierarchy and membership storage remain proportional
to their previous sizes. Runtime and peak-memory gains must be measured separately;
this allocation argument alone is not a complete large-part acceptance result.
