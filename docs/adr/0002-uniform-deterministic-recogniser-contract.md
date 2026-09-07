# ADR 0002 — Uniform deterministic recogniser contract

- **Status:** Accepted
- **Date:** 2026-08-15
- **Decider:** Paul Fremantle

Step Level and Riser determinism was originally value-level rather than occurrence-preserving. The
body-local amendments for issues #335 and #336 below supersede that structural exclusion: both
families now retain equal-valued occurrences across solids and bind their exact same-solid faces.

## Context

Recognition functions historically differed in naming, signatures, return shapes, dependency
handling and serialization. Draftwright ADR 0013 established and mechanically tested a uniform
contract before extraction.

## Decision

A base recogniser has this shape:

```python
recognise_<feature>(part, *, <tuning>, <injected evidence>) -> list[RecordType]
```

A derived recogniser is a pure function of records already produced:

```python
recognise_hole_patterns(holes) -> list[PatternRecord]
```

Every public recognition record is a typed frozen dataclass, contains only serialisable geometry
values, and provides a stable dictionary projection. Empty means confidently absent within the
recogniser's documented supported domain; ambiguity or unsupported topology is diagnostic output,
not an empty-list alias.

Recognisers are deterministic with respect to equivalent input geometry and configured tolerance.
They do not call sibling recognisers. The orchestration layer computes reusable evidence once and
injects it, preventing duplicate work and divergent feature universes.

Public spelling uses British `recognise_`. Low-level substrates may use precise non-recogniser
verbs such as `analyse_cylinders` because they return evidence rather than accepted features.

## Required guards

- Signature and return-annotation tests enumerate every public `recognise_*` function.
- Returned records are frozen and JSON-serialisable without build123d/OCP objects.
- Permuting kernel traversal order does not alter deterministic record ordering.
- A mutation test proves each injected dependency is used rather than recomputed.

## F4b Passage compatibility transition (0.4)

`recognise_section_passages` is the attributed Passage entry point and returns the sole physical
`SectionPassage` record. `recognise_passages` remains a writer-free legacy projection; any
non-`None` ledger raises `PassageCompatibilityError` before discovery. This intentional pre-1.0
break removes the otherwise unavoidable second Passage evidence authority.

## Consequences

The contract favours predictable composition over one-off convenience. An aggregate that appears
too small for a list must receive a self-contained record, not a special return shape.

## Amendment (body-local Riser evidence and projection, issue #336)

`recognise_risers` reduces equal face-derived values only within one valid solid, then globally
sorts the occurrence list without set-deduplicating across solids. Each recogniser-produced
`RiserEvidence` carries the eligible `body_levels` derived from that same solid. The pure
`project_step_shoulders` operation intersects caller-selected FaceLevel occurrences with this
authority before testing the riser endpoints and preserves equal projected occurrences across
bodies. Numeric levels remain compatibility value selectors. Full FaceLevel records distinguish
equal-Z occurrences with different body-local supports; value-identical occurrences use the
explicit `levels_by_riser` input aligned with the deterministic riser occurrence roster. This
avoids publishing a topology-derived body identifier that could not remain stable across runs.

`body_levels=None` is compatibility for manually constructed pre-0.4.9 evidence only; it retains
the historical value-only projection. Production discovery never emits `None`. The serialized
field addition advances `RiserEvidence` schema version to 2 without publishing `SolidRef`, graph
handles or a second recognition site.

## Amendment (0.2.6, epic 0002)

**The declared shape has no slot for a write-only sidecar, and seven recognisers now take one.**

`recognise_slots`, `recognise_pockets`, `recognise_passages`, `recognise_grooves`,
`recognise_turned_steps`, `recognise_chamfers` and `recognise_angled_steps` accept
``ledger: ClaimLedger | None = None``. It is neither tuning nor injected evidence: nothing is read
from it, and the recogniser writes into it. It is the only mutable parameter in the contract, and
a parameter kind used seven times should be named rather than left to resemble the two it is not.

The shape is therefore:

```python
recognise_<feature>(part, *, <tuning>, <injected evidence>, <claim sidecar>) -> list[RecordType]
```

with three properties that keep it inside this record's determinism guarantee rather than beside
it:

- **Write-only during discovery.** A recogniser appends and never reads back, so no family's
  output can depend on which families ran first. This is what keeps the sidecar compatible with
  ADR 0003's separation of discovery from reconciliation, and it is why the parameter is not
  "injected evidence" -- evidence flows in, claims flow out.
- **Passing it changes nothing about the return value.** Each family's claim tests assert this
  directly, calling with and without a ledger and comparing.
- **A mispaired ledger is refused, not silently ignored.** `FaceGraph.require_node` raises rather
  than resolving nothing, because an empty ledger reads downstream as "this family claims nothing"
  rather than as "you paired the wrong graph".

## Required guards, added

- A recogniser offering the sidecar returns the same records with and without it.
- Its claims name the faces the record was **established by** -- asserted against the geometry
  those faces have, not against a captured count.
- A ledger built from a different part is refused.

## F5 defining-attribution migration

Newly migrated attribution uses one private writer-enabled core while the supported public wrapper
invokes the same core without a writer. Across those independent calls parity means record type,
value, order and `to_dict()`, not Python identity. Within the writer-enabled run each Candidate must
retain the exact returned record occurrence; equal-valued occurrences remain identity-distinct.

`recognise_flats` is the first family using this shape without adding a public sidecar parameter.
Its private core completes discovery, sizing, sorting and every original-face/common-solid binding
before publishing any Candidate. The public wrapper calls that core without a sink and retains its
exact signature and byte/value/order behaviour. The owning planar face is defining; the matched
cylinder and an optional opposed flat contribute sizing as consulted context.

`recognise_fillets` follows the same private-core shape without adding a public sidecar. Its
writer-enabled aggregate call and writer-free public call preserve every existing keyword and
record value/order. The core completes both cylinder and torus discovery, final sorting and all
owner-face/common-solid validation before publishing. Each Candidate retains the exact returned
record occurrence and only its original curved blend face as defining evidence.

`recognise_countersinks` uses the same shape. On valid closed-solid inputs, its geometry-only
public facade and private writer-enabled core preserve record type, value, order and `to_dict()`;
each same-run Candidate retains exact returned-record identity. Open or ambiguous topology may
remain publicly recognised for compatibility while aggregate evidence refuses before publication.

`recognise_bosses` follows without introducing a canonical sort: its private core preserves the
existing `_segments(external)` emission order in both writer-free and writer-enabled calls. Each
same-run Candidate is identity-bound to that returned occurrence and contains the complete
graph-deduplicated set of original external cylinder faces in its producing segment. Every pending
set binds to one valid solid before the first publication; end-classification faces remain transient
consulted context. Equal-valued Boss records from distinct valid bodies remain distinct occurrences.

`recognise_double_d_bores` likewise delegates to one private optional-writer core without changing
its public signature or geometry-only behavior. On valid issuable solids, writer-free and aggregate
runs preserve record type, value and current sorted order; each aggregate Candidate is identity-bound
to the exact returned occurrence. The defining set contains every original planar/cylindrical
lateral wall patch in the proven constant Double-D extrusion. Extremal stock planes, opening wires,
per-solid extrema and the empty-prism boolean remain consulted throughness/serialization facts.
## Amendment (Polygonal Boss attribution, issue #218)

`recognise_polygonal_bosses` remains the public writer-free facade over one private discovery
core.  Writer-free and aggregate calls share the same records, stable ordering and serialization;
aggregate Candidates retain the exact returned record occurrences rather than rematching equal
values.  A Polygonal Boss owns exactly the six original vertical side faces in its accepted ring.
Terminal, support and transition caps remain consulted geometry and are never defining evidence.
`PolygonalStock` remains a separate public record and attribution family.

## Amendment (Polygonal Boss blend-view consumer, issue #192)

`recognise_polygonal_bosses` is the sole F3b consumer of the private blend-collapsed view. Direct
and aggregate calls continue through the same private discovery core and preserve the public
signature, record type, value, order and serialization. Only an explicitly selected, complete,
issuer-owned six-chain convex singleton cycle may add adjacency between the six original planar
supports; all profile, cap and height geometry continues to query the base `FaceGraph`. Every
synthetic arc is expanded and its complete original node and occurrence multiset revalidated before
the proposal can be staged. The six planar supports remain the complete defining evidence; hidden
blend nodes, spring/internal/terminal occurrences and cap faces are consulted only. Logical values
never enter records or evidence. `recognise_polygonal_stock` and every other recogniser remain
base-graph-only, and no public type or reconciliation rule changes.

## Amendment (Polygonal Stock attribution, issue #232)

`recognise_polygonal_stock` remains the writer-free facade over a private optional-writer core.
Each aggregate occurrence owns the complete eight-face boundary of its one valid solid: six
geometrically ordered outward side faces and the uniquely selected lower and upper terminal caps.
Cap identity is retained when recognition selects the cap; it is never inferred later from a
leftover face or rematched from rounded base/top values. Polygonal Stock remains `NotCounted`
because it is stock context rather than a machined feature, independently of complete attribution.

`recognise_rectangular_pads` follows the same private-core contract. Its public facade remains
geometry-only and preserves the existing per-solid value deduplication and global record sort. Each
issuable occurrence owns exactly five pairwise-distinct original faces: the accepted +Z top and the
unique maximal-base x0, x1, y0 and y1 wall-role faces. Tier regions, stock extrema and unselected
wall/top candidates remain consulted. Equal values with conflicting ordered role identities refuse
aggregate publication rather than being rematched by value.

## Amendment (Hole attribution, issue #220)

`recognise_holes` follows the private optional-writer contract. A completed Hole occurrence owns
the complete graph-deduplicated set of original internal cylindrical patches that establish its
bore, selected near-side counterbore/spotface lands, and any blind deep-extension span. End planes,
drill-point cones, shoulder transitions, skipped grooves, crossing faces and far-side steps remain
consulted. A nested countersink is an identity-linked completed COUNTERSINKS predecessor: its cone
remains defining only for that predecessor and is never copied into HOLES evidence.

## Amendment (Channel attribution, issue #225)

An aggregate Channel owns exactly the two original planar side walls that establish its width:
the geometrically low inward-facing wall followed by the geometrically high inward-facing wall.
Floors, caps, envelope ends, curved interruptions and other wall pairs remain consulted. Discovery
carries these wall identities through the existing per-solid exact-value reduction and global
geometry sort; competing wall pairs for one value refuse aggregate publication rather than being
rematched from the rounded record.

## Amendment (Plate attribution, issue #228)

An aggregate Plate owns the complete original low-negative and high-positive planar face clusters
whose adjacent events establish its record. Every cluster member remains defining because its area
and centroid contribute to admission or the weighted `u`/`v` projection. Other groups, bbox facts
and TURNED_STEPS records remain consulted.

Issue #334 supersedes the earlier bounded compound refusal. Face grouping, bounding-box area and
thickness denominators, adjacent events and geometry-key deduplication are all scoped to one valid
source solid before global ordering. Equal-valued records on separate solids retain multiplicity;
their `u`/`v` witnesses remain body-local and no face pair or area sum crosses a solid boundary.
The aggregate resolves every staged low/high face through the one run graph before deciding
ambiguity: wrapper- or traversal-duplicate role pairs collapse by graph-issued node identity,
while two distinct bound role pairs competing for one key on the same `SolidRef` refuse atomically.
An open-shell writer-free call retains its legacy geometry projection, but aggregate publication
still requires one valid solid.

Issue #329 replaces only the coordinate-aligned cross-area denominator. For each principal Plate
normal, the recogniser uses the smallest rectangular cross-envelope obtained by aligning the
body's own eligible transverse planar directions with the two transverse coordinates. The
orientation is intrinsic to that body and therefore rotates with it; frame roll cannot inflate the
denominator. `min_area_frac`, the low/high groups, adjacency, thickness gate, record values,
defining faces and per-solid issuance remain unchanged. Where no transverse planar direction
exists, the record-only compatibility path retains its existing coordinate envelope; curved and
non-prismatic bodies remain outside the documented Plate domain.

## Amendment (neutral recess provenance, issue #234)

Slot and Pocket reduction carries immutable occurrence objects rather than using serialized record
values as provenance identity. Intentional merge and collinear-collapse operations union the exact
source nodes of the occurrences they absorb; body scoping replaces only the public record field and
retains the occurrence's topology. Corner-notch discovery likewise issues its floor-and-wall
identity set directly, with no record-value claim map between discovery and reduction. Obround
endpoint clusters retain every original cylindrical
patch, separately from planar evidence. This substrate is private and neutral: public values/order,
legacy claims, family attribution status and aggregate evidence publication do not change here.

## Amendment (Repeating Radial Profile attribution, issue #239)

`recognise_repeating_radial_profiles` remains a writer-free geometry facade. In an aggregate run,
each retained occurrence owns exactly the two original opposed extremal planar faces whose complete
outer boundaries establish its neutral rotational correspondence record. Boundary edges, sampled
points, side regions, rejected faces, tolerance facts and correspondence alternatives are consulted
geometry, not additional defining face evidence. Attribution does not classify gears, splines or
manufacturing intent, and the family's existing non-census disposition remains unchanged.
All discoverable geometry, source-identity and one-body failures are exhausted across the complete
proposal roster before the first issue. Publication then relies on the issuer's validated proposal
operation; this recogniser does not claim a separate transaction or rollback for arbitrary injected
issuer failures.

## Amendment (Slot attribution, issue #235)

Each Slot occurrence now owns the complete route-selected original topology carried by #234:
every planar wall retained by intentional merge/collinear collapse and every patch in the selected
low/high cylindrical cap groups. The public writer-free values and order remain unchanged. Equal
values on separate solids remain separate occurrences. One original wall may truthfully define
distinct exact Slot records on the same `SolidRef` (nested or adjacent reductions); each Candidate
retains that node rather than imposing exclusive ownership. Graph-identical proposal duplicates
collapse, while same-record competing role sets, cross-solid reuse, foreign identity, cap ambiguity
or missing one-body proof refuse before any Slot issuance.

## Amendment (Pocket attribution, issue #236)

Each retained Pocket occurrence owns the complete original topology selected by its discovery
route. Opposed-wall pockets own the intentionally merged planar walls but not their consulted
floor; corner notches own both walls and the floor that establishes their footprint; obround
pockets additionally own every patch in each selected endpoint cluster. Publication follows
complete graph identity and one-solid validation and does not alter writer-free values or order.
Corner compatibility remains exactly the existing world-Z-floor/world-X/Y-wall grammar, including
Z-based `open_sign`; this attribution amendment does not generalize it to arbitrary local frames.
An original face may define multiple unequal Pocket records on the same graph-issued solid (nested
depth readings in established geometry). Equal records collapse only when their complete bound role
sets are identical; competing assignments refuse before publication.

## Amendment (Pocket complete constituent boundary, issue #420)

For the existing opposed-wall rectangular and circular-end Pocket routes, the final neutral
proposal proof may retain one complete bounded interior region as constituent membership. It joins
an exact opening inner-wire region to the exact wall, cap and floor nodes already retained by the
proposal, before Candidate issuance. The region and proposal must select each other uniquely and
the region plus its opening must belong to one valid solid. Whole-body, cross-solid, intersecting,
merged, fragmented or competing cases retain the historical defining-and-floor constituent set.

This changes constituent membership only. Pocket records, ordering, serialization, standalone
recognition, defining evidence, claims and reconciliation remain unchanged. The neutral reduction
does not read Candidates, public evidence, sibling recognisers, corpus labels or policy outcomes.

## Amendment (Rectangular Pad blend-view consumer, issue #277)

`recognise_rectangular_pads` remains the public writer-free facade over the same optional-writer
core and retains its signature, `RaisedPad` schema, ordering and sharp/native behavior. A second
proposal route may restore a rectangular Pad interrupted by one complete selected four-corner
convex blend cycle. It reconstructs the existing x/y wall planes and highest-perimeter-wall base;
the four authored radii explain the rounded top's exact missing area and are never recognition
tolerances. Direct and aggregate calls use the same route and return the exact sharp-control
record. Aggregate defining evidence remains exactly the original rounded top and four planar wall
roles. Logical nodes and hidden blend faces never enter records or evidence.

## Amendment (principal-axis Rectangular Pads, issue #331)

`recognise_rectangular_pads` evaluates all six signed principal directions in the supplied
recognition frame using one run-owned effective-surface query and geometry graph. `RaisedPad`
schema version 2 adds `axis` and `direction`; its sorted XYZ bounds still locate the exact local
island, while those fields identify the attachment-to-terminal coordinate and outward sign.
Defaults of `axis="z"` and `direction=1` preserve positional construction and values for legacy
+Z records.

A rectilinear union can expose overlapping five-face readings on more than one axis. Boundary-
disjoint occurrences remain independent; overlapping readings select the unique orientation with
the shortest attachment span. A minimum tied within the existing absolute recognition tolerance
is refused rather than resolved by XYZ iteration order. Competing values or roles within the
selected orientation still reach the normal identity and one-solid validation and cannot be
silently arbitrated away. Sharp and complete four-corner convex-blend routes retain exactly one
top and four original wall roles as defining evidence.

## Amendment (Groove turned-profile membership, issue #354)

`Groove` schema version 2 appends an optional `profile: TurnedProfileKey`. Recogniser-produced
records always derive it from the same body-partitioned cylinder inventory and physical solid
bounds as `TurnedStep`; the `None` default preserves hand-built schema-v1 records. This lets a
consumer join a groove to exactly one public turned profile without rescanning topology or using
coordinate proximity. Before attributed publication, all Groove floor evidence must resolve to one
valid solid and one profile key must not identify two solids; ambiguity refuses the family before
any partial prefix is written.

## Amendment (cross-family body correlation, issue #390)

`Channel` and `Plate` schema version 2 append the same opaque, serializable `body_key`. The key is
derived from the source solid's frame-local bounds, volume and area and exists only to compare
records produced by one recognition coordinate system. Separate solids with an equal signature
receive `None`; traversal order, kernel handles and nearest-coordinate inference never break that
ambiguity. Hand-built legacy records retain the empty-tuple default. This changes no geometric
candidate, evidence role, count or reconciliation decision.

Single-solid STEP imports obey the same source-solid scope (#559). A compound wrapper may
also carry loose construction edges or faces; its mass properties and bounds are not that
solid's correlation key. Families scan each actual solid even when there is only one, retaining
the original input as a compatibility fallback only when no solids exist. The complete solid
roster still determines signature ambiguity, and source signatures are not normalized into
persistent identifiers. This corrects key values without changing their schema or precision.

## Amendment (FaceLevel/turned-profile body correlation, issue #385)

`FaceLevel` and `TurnedProfileKey` schema version 2 append the same optional, opaque `body_key`
used by other body-owned records. Recogniser-produced records derive it once from the complete
valid-solid roster, so levels embedded in `RiserEvidence`, standalone level results, `TurnedStep`
profiles and `Groove` profiles agree by equality. Equal signatures on separate solids publish
`None`; legacy hand-built records retain the empty-tuple default. This lets a consumer distinguish
a disconnected stair enclosed by a shaft's AABB from a blind-bore floor physically owned by that
shaft without rescanning topology or treating containment as identity.

## Amendment (Riser/ThroughStep body correlation, issue #388)

`RiserEvidence` schema version 3 and `ThroughStep` schema version 2 append the shared optional
`body_key`. The complete source-solid roster resolves keys before either family publishes records;
equal signatures refuse with `None`, and legacy constructors retain the empty tuple. A consumer can
therefore reconcile a structural riser with a ThroughStep only when their non-null keys agree,
without allowing a nearby or touching-but-disconnected solid to steal the riser. The field changes
ownership correlation only, not either family's geometric acceptance or defining evidence.
