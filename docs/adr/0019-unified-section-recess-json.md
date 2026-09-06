# ADR 0019 — Unify constant-section recesses in one JSON geometry

- **Status:** Accepted
- **Date:** 2026-09-04
- **Issues:** #290, #495
- **Prototype:** #496
- **Extension:** [ADR 0020](0020-native-cylindrical-section-ends.md) proposes explicit
  planar/cylindrical end surfaces in document schema 3; the schema-2 example below
  records this ADR's original contract.
- **Consumer review:**
  [`docs/draftwright-section-recess-schema-review.md`](../draftwright-section-recess-schema-review.md)

## Context

The package describes closely related subtractive geometry with separate public records:
`Pocket`, `PrismaticPocket`, `RectangularBlindSlot`, `RoundBottomBlindSlot`,
`EdgeOpenPrismaticRecess`, `EdgeOpenCircularPocket` and `SectionPassage`. A proposed oriented
circular-end pocket would require another record because the axis-letter `Pocket` cannot express
its free in-plane directions.

These divisions partly reflect which recogniser found the geometry rather than a difference in
the underlying geometric value. Each occurrence has a section placed in 3-D, an extent along a run
direction, physical section boundaries, and conditions at its two run ends. Shape-specific records
duplicate that vocabulary and require every consumer to add an adapter and IR type for each new
section shape.

Draftwright reviewed the proposed unification against its declaration, planner and renderer paths.
It confirmed that a free-frame line/arc section is sufficient, that convenience dimensions are
derivable, and that replacing the specialised records is preferable to carrying two encodings of
one fact. Draftwright also identified two requirements that apply to every consumer: shape
classification must be issued by the recogniser, and serialized face references must make their
one-document lifetime explicit.

The package is pre-1.0 and Draftwright is the primary consumer. The two projects will coordinate
the breaking change. Preserving the old record schemas is not a design goal.

## Decision

### One structural geometry

Replace the specialised constant-section pocket, edge-open recess and passage output schemas with
one domain-neutral `SectionRecess` JSON geometry. Specialised recognition algorithms remain
separate and fail closed under their own contracts; accepted occurrences converge only at the
public value.

A `SectionRecess` contains:

- a right-handed free 3-D `frame` with `origin`, `run`, `u` and `v`;
- one strictly increasing `run_interval` measured along `run`;
- one canonical 2-D `profile` in `(u, v)` coordinates; and
- explicit `low` and `high` end conditions.

The frame and closed line/arc boundary use the existing `PassageFrame` and `PassageSection`
placement, canonicalisation, precision and reconstruction rules. The final wire encoding may use
the existing vertex-and-bulge representation or an equivalent canonical line/arc segment list; it
must have exactly one meaning and must not duplicate derived radius or dimension values.

The profile is one of two tagged variants:

- `closed`: a continuous, simple, closed physical boundary; or
- `open`: one continuous chain of physical line/arc segments plus the explicit gap between its
  loose endpoints.

An open-profile gap records missing boundary, not a wall or curve. A consumer may construct a
temporary closure against stock or body geometry for its own operation, but neither provider nor
consumer may report that construction as observed source geometry.

Each run end carries a condition of `open` or `capped`. Existing planar termination gradients are
geometry in the section frame and remain available where proved. `capped` identifies a physical
termination; it does not encode the detailed bottom shape. Opening direction, cap condition and
bottom treatment are separate facts.

Record validation enforces positive separation of the two termination planes over the physical
profile, including analytic interior extrema of arcs (#516). An open physical chain must be
simple and cannot overlap or backtrack; its absent closing boundary is not an edge to validate.
These checks preserve the truthful geometry contract independently of the originating detector.

The initially admitted interpretations are:

| Feature kind | Profile | Run ends |
| --- | --- | --- |
| `pocket` | closed | exactly one capped |
| `edge_open_recess` | open | exactly one capped |
| `passage` | closed | both open |
| `channel` | open | both open |

An enclosed cavity or any other combination is refused until a
recogniser proves it and the schema explicitly admits its `feature_kind`.

### Authoritative classification

Every occurrence carries two required, provider-issued classifications:

- `feature_kind`: the engineering interpretation of profile closure and end topology; and
- `section_shape`: the proved geometric class of the 2-D profile.

The initial `feature_kind` vocabulary is the admitted table above. The initial `section_shape`
vocabulary is:

- `rectangular`;
- `circular`;
- `obround`;
- `triangular`;
- `hexagonal`;
- `polygonal`; and
- `general`.

`general` means that the recogniser proved a valid section but no more specific class. It is not an
invitation to guess. Both vocabularies are closed and versioned with the enclosing schema.

The recogniser has source B-rep geometry and owns tangency, parallelism, curve identity and
tolerance policy, so it must issue these classifications. Consumers may perform cheap consistency
validation and must reject a mismatch, but they must not independently reclassify rounded JSON.
Geometry remains sufficient for reconstruction without switching on classification. Classification
selects downstream domain convention; it is not a substitute for geometry.

No stored `width`, `length`, `depth`, `radius`, side count or named world axis accompanies the
profile. Such values are derived views selected by the authoritative classification. A public API
may provide one implementation of those derivations as convenience functions, but they are not
second serialized facts.

### Constant-section boundary

`SectionRecess` admits only a physical support profile that is constant over its proved run
interval, apart from separately proved end or edge treatments. A drafted, tapered, twisted or
otherwise varying wall set does not become a `SectionRecess` by recording a nominal section.

A polygonal pocket's planar mouth may be partitioned into multiple coplanar stock faces.
These patches collectively provide termination context only when every wall has observed
convex or smooth adjacency to a qualifying patch on the same mouth plane and all consulted
faces belong to the same valid solid. This does not relax the floor, constant-section,
empty-run, open-mouth or floor-backing proofs. Stock patches remain consulted context,
not defining or constituent evidence; their partition does not change the public geometry.

An intact mixed line/circular-arc floor may likewise establish a `general` pocket when
its exact extrusion has complete observed wall support, an empty interior, an open planar
mouth and backed floor, all within one owning solid. The proof reads the physical wire:
short straight segments and distinct cylinder axes are not merged into a nominal obround.
Unsupported floor islands, missing wall patches and obstructed runs or mouths remain refused.
This admits, for example, rounded rectangles without adding a shape-specific schema.
Existing more specific proved classifications take precedence for the same floor.

Publication keeps the existing displacement limit. Arc-rounding displacement is bounded
over the whole sweep analytically: for equal sweeps the pointwise difference is a constant
vector plus a rotating vector, whose maximum lies at an endpoint or vector alignment.
A radius-times-sweep-change term bounds the serialized bulge perturbation. This avoids
rejecting accurately representable transformed sections solely through an unnecessarily
loose sum of centre, radius and angular errors; it does not relax the geometric limit.

Chamfers, blends, wall draft and bottom radii do not add optional shape-specific fields to this
record. Independently proved chamfer and blend occurrences remain separate and may be related to a
recess occurrence in a future result relationship table. The base recess and related treatment
must not publish two authoritative values for the same measurement. Wall draft or another variation
that prevents proof of a constant base support causes refusal; a future lofted or station-based
geometry requires its own decision.

An entry bevel may separately explain missing planar passage support without a new public
termination schema (#540). One intact polygonal mouth supplies the base section. Observed
opposite stock, wall and bevel planes bound a finite removed cell; its predicted bevel footprint
must equal the complete observed face, and the owning solid must contain no material in that
cell. Original walls plus those proved treatment supports must cover every base-wall patch.
The existing full-section void and open-end proofs remain mandatory. An unrelated outlet step,
wall hole or obstruction is not explained merely because some bevel exists nearby.

Only original base walls are defining evidence; original proved bevels join them as constituent
evidence. Stock faces are consulted context. Reconstructed cell faces are private verification
geometry and never enter the face roster as observed support. The base passage profile and
planar run ends retain their existing meaning, with the explicitly proved treatment exception.
This does not admit arbitrary nonplanar ends, varying sections or a general relationship API.

ADR 0021 separately permits proved circular apertures strictly inside channel wall
or floor supports. Each original inner wire must match a native inward cylinder
with two observed finite limits, complete original side support and an empty cell
on the same body. Original supports plus these exact aperture explanations must
cover each complete proposed patch; touching ends, outer-contour breaks and
unexplained gaps remain refused. Opposite-wall bore segments stay separate. The
public profile describes base channel support with these proved interruptions,
not an unperforated final boundary. Original cylinders join constituent evidence;
independent hole occurrences remain separate. This does not admit pierced capped
ends or imply full treated-boundary reconstruction from the base record alone.

Islands are excluded from schema version 1 because no current family proves them. A later version
may add a canonical list of closed inner profiles after recognition and consumer evidence exists.

### JSON occurrence and evidence envelope

Serialized recognition output separates:

1. `geometry`: reconstructible section, placement and end topology;
2. `classification`: provider-issued feature and section meanings;
3. `evidence`: source faces that define and physically constitute the occurrence; and
4. `relationships`: optional links to independently proved occurrences when that facility is
   introduced.

The contract contains no build123d, OpenCascade, Draftwright IR, CAM strategy or other
implementation-specific value.

Face evidence uses a document-local indexed table, not random names and not durable identifiers.
The result document contains a `faces` roster in the exact source-face enumeration used by that
recognition run. Its zero-based array position is the face index. `defining_faces` and
`constituent_faces` contain only indices into that roster. Occurrence and body references use the
same document-local principle.

For example:

```json
{
  "schema_version": 2,
  "reference_scope": "result",
  "bodies": [
    {"index": 0}
  ],
  "faces": [
    {"index": 0},
    {"index": 1}
  ],
  "occurrences": [
    {
      "index": 0,
      "body": 0,
      "geometry": {
        "type": "section_recess",
        "frame": {
          "origin": [10.0, 20.0, 30.0],
          "run": [0.0, 0.0, -1.0],
          "u": [1.0, 0.0, 0.0],
          "v": [0.0, -1.0, 0.0]
        },
        "run_interval": [0.0, 8.0],
        "profile": {
          "closure": "closed",
          "boundary": [
            {"point": [-10.0, -5.0], "bulge": 0.0},
            {"point": [10.0, -5.0], "bulge": 1.0},
            {"point": [10.0, 5.0], "bulge": 0.0},
            {"point": [-10.0, 5.0], "bulge": 1.0}
          ]
        },
        "ends": {
          "low": {"condition": "capped", "gradient": [0.0, 0.0]},
          "high": {"condition": "open", "gradient": [0.0, 0.0]}
        }
      },
      "classification": {
        "feature_kind": "pocket",
        "section_shape": "obround"
      },
      "evidence": {
        "defining_faces": [1],
        "constituent_faces": [0, 1]
      }
    }
  ]
}
```

Every index must be a non-negative integer within its referenced roster. Rosters are canonical and
contain no duplicate entry. A consumer can derive reverse face-to-occurrence indices; the format
does not serialize a second redundant association map.

The face index is neither random nor globally meaningful. It is valid only within its containing
result document and can be resolved to a source face only while the exact recognition input and its
face roster are retained. It must never be used as a database key, compared across recognition
runs, or assumed stable after re-import, editing, healing, framing or tessellation. Applications
that retain several results must scope every reference through its containing result rather than
copying a bare integer out of it.

Surface kind, area, centroid, bounds or other geometric descriptors may later accompany a face-table
entry for inspection or best-effort correspondence. Such descriptors are signatures, can collide
or change, and do not establish cross-run identity.

The existing Python `RecognitionEvidence` API continues to issue opaque run-local `FaceRef` and
`FeatureRef` objects until the JSON migration replaces or supplements that surface. Those objects
remain non-serializable and resolve through their issuing evidence view. Their internal graph index
is not the JSON contract.

## Consumer contract

A consumer reconstructs geometry from `geometry` and uses `classification` only for domain policy.
It validates the closed enum vocabulary and fails closed when it has no convention for a
`feature_kind` × `section_shape` combination. It must not coerce an unsupported general or oriented
section into the nearest axis-aligned convention.

CAM tool choice, setup, accessibility, feeds, speeds and toolpath strategy are consumer policy and
do not enter recognition JSON. The neutral record supplies the geometry needed to make those
decisions without claiming that finished B-rep geometry uniquely determines a machining process.

Draftwright owns its IR, declaration grammar, planning convention and drawing representation. It
will validate one free-frame oriented obround occurrence end to end in the same development window
as the provider migration.

## Migration and evidence

The migration endpoint is a coordinated breaking replacement, not a permanent additive
compatibility layer. Delivery is staged so each production slice can be validated independently.
During that transition, the neutral records coexist temporarily with specialised records; this
does not commit the project to preserving both contracts. PR #496 is the first additive production
slice, covering closed constant-section pockets, while the remaining steps complete the replacement:

The public `build_section_recess_document(part)` entry point runs the ordinary raw/caller-coordinate
aggregate once and serializes its accepted `RecognitionResult.section_recesses` inventory. It is
not a second detector and must not bypass aggregate reconciliation. Its body and face tables cover
the complete input rosters, while occurrence indices are dense within the emitted document.

1. implement the neutral nested frame, profile, end and classification values (completed in
   #496 for closed constant-section pockets);
2. project every representable accepted specialised occurrence into `SectionRecess` without
   changing discovery or reconciliation behavior;
3. update the JSON manifest and Draftwright consumer together;
4. verify existing supported occurrences reconstruct the same removal geometry within published
   serialization bounds;
5. verify every migrated occurrence receives the expected authoritative classifications;
6. carry the oriented circular-end recogniser through the same record and through Draftwright; and
7. remove the superseded public records, result fields and consumer paths.

Draftwright may provide a one-time authored-script migration and must report stranded parameter
identifiers rather than silently suppressing them. This does not require the provider to preserve
the old JSON records or maintain a deprecation window.

Development and authored fixtures establish the schema. MFInstSeg remains a pseudo-blind aggregate
transfer corpus and must not be inspected or used to tune the representation.

### Production evidence

PR #496 promoted the validated vertical slice into the public ``SectionRecess`` record,
``recognise_section_recesses`` entry point, normal aggregate inventory and capability manifest.
Export one STEP file with:

```console
uv run python tools/export_section_recesses.py part.step --output recesses.json
```

The companion Draftwright work consumes that JSON without importing provider record classes and
derives drawing length, width and depth from the profile and run interval. Its fail-closed consumer
tests complement the provider's covariance, reconstruction, ownership and rejection tests.

The contract proof is not obround-specific. The same projector round-trips free-axis triangular,
rectangular and hexagonal closed profiles using straight boundary segments, and the pre-existing
adapter suite round-trips all 25 golden polygonal recess records. Polygonal recall remains a
separate detector-covariance question: adopting this value removes the schema obstacle but does not
turn principal-axis discoveries into free-axis discoveries.

The subsequent complete MFCAD++ six-sided-pocket overlay answers that question narrowly: an intact
free-frame floor proof is perfectly pure but adds only 49 net faces, raising coverage from 0.9320 to
0.9406. Orientation covariance is therefore a valid substrate, not the main polygonal residual.
Treatment-interrupted cavity propagation is separate recognition work and remains the next gate.

The migration also projects accepted passages, prismatic pockets, edge-open polygonal/circular
recesses, rectangular blind slots and round-bottom blind slots from their existing exact proofs.
The legacy `Pocket` value is intentionally not projected by itself: despite its historic name and
rectangular dimensions, some occurrences summarize a non-rectangular boundary by its extents.
Claiming a rectangular section from that value would fabricate geometry. Such an occurrence enters
the unified inventory only when a geometry-bearing native or prismatic proof establishes its real
section.

Legacy polygon vertices can coincide after their three-decimal serialization (for example,
a sub-micron corner chamfer). A shared publication adapter for `Passage` and `PrismaticPocket`
normalises the recorded boundary once using integer publication-grid coordinates. It removes
collapsed edges and exact collinear backtracking, validates simplicity, and canonicalises the
remaining loop. Removed excursions must stay within the existing 0.002 mm displacement bound.
Non-adjacent repetitions that still imply ambiguous topology, zero-area loops and intersections
are refused with a bounded `LegacySectionProjectionError` naming the failed condition. Source
records and face evidence are retained; simplified profiles remain classified as `polygonal`.

Issue #547 enforces this refusal boundary during aggregate publication. Expected `ValueError`
from public geometry construction is converted locally to `LegacySectionProjectionError`.
Projection skips only that occurrence, then the existing `SectionRecessRefusal` roster publishes
its accepted source evidence unless another truthful occurrence already covers that region.
Other occurrences survive and their public indices and pattern references are rebuilt normally.
Discovery, source-proof, ownership and evidence-invariant failures are outside this conversion;
there is no blanket exception handler around recognition. Historical compatibility checks during
passage issuance are also outside this publication boundary.

The adapter chooses a frame origin on the same grid near the analytic centroid, then subtracts
integer grid coordinates before constructing the public section. Its analytic local centroid
remains within the published 0.0008 mm allowance. Distinct grid vertices cannot collapse through
a second rounding after translation. In particular, production projection does not pass through
the stricter private `SectionOccurrence`, whose exactly-zero centroid requirement can conflict
with representability on the publication grid. `PlanarSection` and private exact-occurrence
invariants are unchanged. The adapter depends on the internal public-record storage solely to
construct the final JSON geometry; it does not invoke recognition.

The normal effectiveness runner now scores ``SectionRecess`` from the production inventory. There
is no overlay path or second implementation.

The provider cutover audit and consumer instructions are recorded in
[`docs/section-recess-migration.md`](../section-recess-migration.md). Exact accepted regions are
checked independently of benchmark scores. The two authored corner notches now project from an
independent source-face proof to an L-shaped physical open chain with one capped run end. Their
gap is not an observed diagonal wall or a floor outline. No new topology combination or vocabulary
is needed: they are `edge_open_recess` / `polygonal`.

The source-face proof also excludes owning-body material throughout the entire proved rectangular
floor support and a thin exterior mouth slab. These closed volumes are internal verification probes,
not published geometry. Checking only the wall/floor incidence would miss suspended obstructions
that leave those faces intact. A failed material check refuses the unified projection.

### Completed provider cutover in #498

The remaining two authored summaries now have an independent three-source-support proof. The
bounded wall overlap defines the run, full source-face patches establish the U chain, and owning-body
material probes verify the section and all three openings. Schema 2 explicitly admits these
open-profile, two-open-end occurrences as `channel`; it does not weaken the old envelope-spanning
Channel detector or invent a fourth wall.

At the maintainer's explicit request this breaking cutover ships as 0.4.15 rather than the planned
0.5.0. It overrides the earlier 0.4.x preservation promise for the specialised recess APIs;
consumers must migrate or pin 0.4.14. The public API removes the specialised records, result fields
and root entrypoints. Arrays
and grids reference unified occurrence indices rather than embedded Pocket values. Patterns are
published only when every member has one unambiguous unified geometric occurrence.

An accepted internal extent summary without a truthful section produces `SectionRecessRefusal`
with body and face evidence, not geometry or substitute dimensions. This is an explicit retirement
of weak geometry claims, not silent loss of evidence. Refusals are included in the JSON envelope
and Python evidence view but excluded from the geometric occurrence census. Source association
coverage includes their evidence and must not be described as reconstructible geometry coverage.

Public census keys now count unified `section_recess` occurrences once. The private physical
candidate inventory and benchmark taxonomy retain historical identities; benchmark baselines do
not change just because output records change. The internal test/scoring adapter is not a supported
consumer compatibility layer. The migration guide specifies the complete new public boundary.

The developer may use MFCAD++ repeatedly. A person outside the implementation loop runs MFInstSeg
once after the branch and test command are frozen and returns only the aggregate JSON. A favourable
result permits production integration; an unfavourable result rejects or narrows the recognition
hypothesis but does not authorize inspecting holdout models or tuning against them.

### Still-used oriented-slot source contract (#533)

`OrientedSlot` remains a public output and still embeds `SectionPassage` as its
source geometry. The cutover must not leave this live reference unpublished:
`SectionPassage` and its `PassageEnds` are root-exported, versioned nested records
under the `oriented-slots` capability. This repairs the manifest/export omission;
it does not change the existing slot geometry, serialized fields or schema versions.
Shared `PassageFrame` and `PassageSection` retain their primary declaration under
`section-recesses`.

This does not restore standalone passage entrypoints, result fields or census
families. Unified recess consumers continue to use `SectionRecess`. A future
replacement of `OrientedSlot.source` would require an explicit migration and
occurrence correspondence, not a silent type substitution in this repair.
ADR 0005's manifest validator checks every record reference, including nested
containers and unions, against the complete published record inventory.

## Consequences

- New section shapes and free orientations do not require new foundational JSON records.
- Rectangular, polygonal, principal obround and oriented obround pockets share one representation.
- Edge-open records preserve only their real wall chain and explicit absence; the truthful-open
  principle of ADR 0018 remains in force.
- Passages share the same structural vocabulary while retaining an authoritative semantic
  classification and potentially separate recognisers.
- Consumers receive one reconstructible geometry and one authoritative classification, with no
  duplicated dimensional source of truth.
- General CAM and 3-D applications can consume the JSON without importing the provider's geometry
  kernel or Draftwright concepts.
- The breaking migration is larger now but removes continuing adapter, IR and schema proliferation.
- Non-constant sections, islands and durable cross-run topology identity remain deliberately out of
  scope.

ADR 0018 is superseded where it requires separate public edge-open record families and unchanged
existing output schemas. Its physical open-chain, explicit-gap and no-fabricated-geometry decisions
remain normative. The migration matrix in `docs/planar-section-schema-proposal.md` is superseded
where it retains legacy pocket records as authoritative. ADR 0005's versioned manifest and
fail-closed consumer declaration remain in force for the coordinated replacement.

ADRs 0001, 0002, 0003, 0004, 0007, 0008, 0009, 0010, 0011, 0012, 0013, 0014 and 0016 otherwise
remain in force.
