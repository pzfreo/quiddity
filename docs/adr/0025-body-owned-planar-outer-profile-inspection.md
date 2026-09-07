# ADR 0025 — Body-owned planar outer-profile inspection

- **Status:** Proposed
- **Date:** 2026-09-07
- **Consumer:** Quiddity #579 / Draftwright #1504

## Decision proposed for review

Extend the existing `quiddity.evidence` lifecycle with one narrow, lazy inspection:
`planar_outer_profile(face_ref)`. It returns an issued `PlanarOuterProfileEvidence` or a
`RefusedPlanarOuterProfile` with a closed reason. It uses the existing run's face graph and
body ownership, never a second aggregate or a consumer-side topology reconstruction.

This is a specific extension of ADR0010's no-public-adjacency boundary: an ordered outer-wire
value and exact source-edge resolution are permitted for this consumer. General graph adjacency,
blend collapse, cross-run correspondence and persistent references remain private. The new
inspection does read the requested original wire after recognition, unlike the existing pure
accepted-occurrence projections. It neither widens accepted feature membership nor changes
claims, reconciliation, census, association or the immutable recognition result. A lazy cache
avoids all wire-reading cost when consumers do not request profiles and repeats no face proof.

The evidence carrier binds the supporting `FaceRef`, geometry value and the **complete exact
face roster of its one valid source solid**. This roster supplies body identity within the view
without another public reference type or a geometry-derived key. Equal-valued placed or copied
bodies remain separate; any ambiguous face ownership in the complete solid refuses. Consumers
may join accepted physical evidence by exact face membership. A profile is not a new physical
feature and does not need a Plate, Blend or PolygonalStock occurrence to exist.

`profile_edge(issued_profile, index)` resolves the corresponding original edge through that
same view. A foreign, copied or forged carrier fails. Geometry values are frozen and serializable;
source bindings and references are not. The input part must remain unchanged for the evidence
lifetime, as already required by ADR0010. No source identity survives serialization or reimport.

## Supported geometry and schema

Schema 1 is a valid native planar face's complete **convex outer wire** containing at least two
finite native lines and otherwise only finite circular arcs. Inner loops are counted and excluded
from this outer-wire projection; they never connect to its support roster. Concave outer wires,
freeform or other analytic curve kinds, circle-only profiles, degenerate/incomplete boundaries,
unowned faces and ambiguous body membership return named refusals. This does not infer an outer
silhouette of an assembly, merge coplanar patches or connect separate bodies.

`PlanarOuterProfile` contains `origin`, outward face `normal`, ordered `supports`,
`inner_loop_count`, `schema_version=1`, and `boundary_kind="outer"`. `ProfileLine` stores finite
`start` and `end`, exposes their normalized `direction`, and serializes `kind="line"`.
`ProfileArc` stores finite `start`, `end`, `center`, `radius`, signed radian `sweep`, and
`kind="arc"`. Sweep is measured about the profile normal. The supported convex output uses
positive sweeps. Coordinates retain source floating-point precision without display rounding. Value constructors
validate closed connectivity, the supporting plane, arc radius and directed sweep reconstruction,
and convex winding; hand-built inconsistent geometry is not a valid schema-1 value.

The complete wire is oriented counterclockwise about the outward face normal, with material to
its left, and starts at its lexicographically least start point. That starting index is a local
ordering convention, not a persistent name; rigid transforms may cyclically rotate the roster.
Consecutive supports, including last/first, share exact original topological vertices. An arc
between two lines retains the actual finite tangencies. Extending those oriented lines gives
an explicit virtual intersection where one exists; the provider chooses no measurement sector.

Raw evidence uses caller coordinates. Framed evidence uses its exact working coordinates and
retained `PartFrame.to_world` mapping; `caller_face` preserves the exact caller face binding.
The same inspection is available on both lifecycles. No caller-space edge rematching is exposed.

## Bounds and module seams

Native source plane coincidence uses the existing 1e-6 model-length bound; angular winding and
convexity use 2e-8 radians. These are numerical consistency bounds, not feature-size thresholds,
fit tolerances or permission to round endpoints. No minimum useful edge length or angle is chosen.

`_outer_profile` is a kernel-free value leaf over `_record`. `_outer_profile_geometry` reads
original topology and graph ownership, depending only on that leaf, `_adjacency` and `_typing`.
`evidence` projects those facts into opaque source bindings. Neither leaf may import recognisers,
Candidates, reconciliation, results, frames or the public evidence facade. The closed module and
raw-reader rosters record this exception explicitly.

The evidence API manifest adds the new symbols without changing its format-1 document shape or
API major. The independently versioned recognition and five-operation inspection manifests stay
unchanged. This is an additive schema-1 capability, not a change to SectionRecess or Plate.

## Evidence and consumer policy

Construction-authored tests cover sharp and rounded polygons, directed supports, virtual
intersections, holes excluded from adjacency, concavity and unsupported curves, equal/touching
and shared-topology bodies, reversed winding, opposite normals, scale, rigid transforms and framed
mapping. Source correspondence tests resolve actual edges and deliberately remove or corrupt
supports. A counted inventory test and lazy-cache test enforce one aggregate for all consumers.

Draftwright owns angle selection, real/virtual measurement intent, sector, datums, tolerances,
views, annotation placement and labels. This proposal supplies geometry and source evidence only.
The original STC618 file is not needed for CI and has not been claimed as a verified canary.
