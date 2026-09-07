# ADR 0010 — Publish a narrow geometry facade; keep correspondence optional

- **Status:** Accepted
- **Date:** 2026-08-26
- **Accepted:** 2026-08-27
- **Decider:** Paul Fremantle
- **Evidence:** [epic 0004 retrospective](../epics/0004-architecture-retrospective.md),
  [F7 geometry facade spike](../f7-geometry-facade-spike.md)

## Context

Epic 0004 established graph-owned identity, analytic-surface facts, blend-collapsed views,
canonical sections, defining evidence and private cross-run correspondence. The private substrate
is intentionally rigorous, but its full implementation surface is too broad to freeze merely
because Draftwright needs some neutral geometry queries.

## Decision

F7 will publish one facade in `b123d_recognisers.geometry`, selected from concrete installed-wheel
Draftwright use cases. It may expose graph-owned opaque face/boundary/blend references, neutral
surface and blend facts, explicit collapsed views with complete provenance, and intrinsic section
values.

It will not expose or require correspondence snapshots, body-boundary descriptors, rigid or
partition matching, Candidate/evidence, registry, reconciliation, issuers, run tokens or private
cache classes. Correspondence remains an optional private upper layer until a separately reviewed
consumer demonstrates a need.

Remove superseded private schema construction paths only after a closed caller/artifact audit.
Other internal refactoring is deferred until a concrete consumer change is blocked by the current
structure. Infrastructure tidiness alone is not an F7 deliverable.

## Consequences

- Draftwright receives a smaller API and compatibility burden.
- Private implementation modules remain replaceable behind facade projections.
- F6 retains its proven capability without becoming a mandatory dependency or public framework.
- F7 starts with a consumer fitness spike rather than an export inventory.
- A future correspondence API requires a new ADR/API-major decision; it is not an additive detail
  of the initial geometry facade.

The detailed evidence and simplification programme are recorded in
`docs/epics/0004-architecture-retrospective.md`.

## Amendment (F7 consumer spike, issue #262)

**The permitted surface in the decision above is wider than the evidence supports, and the
roster is narrower than a facade.** The spike recorded in
[`docs/f7-geometry-facade-spike.md`](../f7-geometry-facade-spike.md) ran two consumers through a
provisional `GeometryGraph`. Polygonal Boss — in-package — exercised the whole surface, which
proves the facade is *sufficient* and is not evidence that any consumer *needs* it. Draftwright's
real workflow, declaring a fillet from its cylindrical face, needed one analytic fact and one
on-surface anchor. Constructing a graph around a single face is conceptually larger than the
problem, so the spike returned no-go on publishing `GeometryGraph` and go on the graph-independent
`inspect_face(face)` contract.

Two consequences for the decision above.

**Blend facts, collapsed views and intrinsic section values leave the initial roster.** They are
named as permitted, and no installed-wheel Draftwright operation consumes them. They remain
private until a reviewed consumer demonstrates a need, on the same rule this ADR already applies
to correspondence.

**The initial roster is the declared-feature inspection family, not a geometry facade.** Every
declared feature in Draftwright needs the same shape of answer — one closed analytic fact off one
face, so that a declared feature and a detected one agree. Read from
`src/draftwright/model/declare.py` at Draftwright 0.4.16.dev0 (`a81c418`), five already exist,
spelled five different ways:

| declared feature | current entry point | status |
| --- | --- | --- |
| fillet | `experimental_geometry.inspect_face` | consumed in production, Draftwright #1347 |
| countersink | `cone_rims` | root export, countersink-family module |
| chamfer | `classify_bevel` / `BevelReject` | root export, chamfer-family module |
| double-D | `profiled_bores.read_double_d_tool` | public module, not root-exported |
| pocket floor | `floor_face_anchor` | root export |

Four of the five reach into a recogniser family's own module, which is why they read as ad hoc.
Unifying them behind one `inspect_*` contract is a smaller and more stable surface than anything
graph-shaped, and it serves the requirement they were each written for: declared/detected parity.

`experimental_geometry` stays absent from the package root exports and the capability manifest
until that roster is reviewed and accepted under #262.

## Graduation outcome (issue #186)

The five-operation roster was reviewed and promoted in 0.4.4 as
`b123d_recognisers.inspection`, with its own closed format-1 `inspection_api.json` contract. The
recognition capability manifest remains byte- and schema-independent. Existing root,
family-module, and `experimental_geometry.inspect_face` paths are identity-preserving aliases so
the publication does not fork behavior or require a flag-day consumer migration.

Only the graph-independent surface values and operation roster graduated. `GeometryGraph`, its
opaque identities, adjacency, blend collapse, sections, correspondence, evidence, registry, and
reconciliation did not. This is the final interpretation of the original `geometry` namespace
wording above: consumer evidence selected a narrower inspection namespace, while the broader
facade remains an experiment.

## Amendment (within-run recognition evidence, issue #375)

Issue #368 supplies the consumer evidence the earlier graph-facade spike lacked: a downstream
consumer must associate one accepted, occurrence-preserving feature with the exact original faces
that define it and, later, the wider faces that constitute it. Reconstructing that association
from record coordinates would create a second attribution authority; importing private Candidate,
EvidenceIndex or FaceNode values would freeze the framework rather than a consumer contract.

Publish a separate narrow `b123d_recognisers.evidence` view over one completed raw recognition
inventory. It issues opaque `FeatureRef` and `FaceRef` values, exposes the existing immutable
`RecognitionResult`, maps a feature reference to its exact accepted record and defining face
references, and resolves a face reference to the borrowed build123d face from the exact caller
part. Equal-valued feature occurrences retain different references. References are issuer-created,
compare only by same-view object identity, cannot be serialized, and fail closed when forged,
copied or supplied to another view.

This is **run-local reference**, not persistent naming. A face reference contains no public index,
kernel hash, geometry-derived pseudo-identity or cross-run equality. The caller must not mutate the
part while using the view. Equivalent re-imports and rigid transforms may produce equivalent
records and face geometry, but never interchangeable references. Symmetric faces may be genuinely
indistinguishable; any future persistent API must use the separately reviewed correspondence layer
and represent ambiguity rather than resolve it through traversal order.

The view does not publish adjacency, blend collapse, graph construction, Candidate/evidence
objects, reconciliation, issuers, run tokens or correspondence. It runs the existing aggregate
once and projects its accepted inventory; it neither calls recognisers again nor changes
recognition. Constituent membership is added by #368 only after its defining-subset invariant is
reviewed. Framed recognition is excluded until it can map evidence back to faces of the caller's
part; a working-shape face must not silently escape as caller identity.

The new namespace has an independent closed installed API manifest. It does not change inspection
format 1 or the recognition capability manifest, because neither document describes run-local
reference operations.

## Amendment (constituent face projection, issue #368)

The same run-local view may project an accepted occurrence's **constituent** faces alongside its
existing **defining** faces. Defining evidence keeps its exact meaning: it is the evidence that
establishes the record and remains the only face set consulted by claims, reconciliation,
dispositions, diagnostics and correspondence. Constituent evidence is physical membership for
downstream selection and coverage; it is not ownership or precedence.

The terminal evidence issuer enforces `defining` as a subset of `constituent`, exact graph-issued
identity and one valid solid. Omission means constituent is the exact defining set, so an
unmigrated family fails closed rather than guessing from adjacency. A face may be constituent to
more than one accepted occurrence, and membership queries preserve occurrence identity and
proposal order without turning overlap into a contest.

Wider membership must retain identities already selected by the recogniser's accepted geometry
proof. It must not be reconstructed later by coordinate matching, adjacency flood-fill, corpus
labels or a second recognition pass. In particular, interrupted or blend-owned faces do not
become constituents merely because they touch a feature. The public
`RecognitionEvidence.constituent_faces()` method is therefore an additive projection through the
existing `FeatureRef` and `FaceRef` carrier; it adds no durable face name and does not alter the
format-1 manifest boundary.

## Amendment (Prismatic Pocket cap membership, issue #403)

The neutral ring proof retains the exact graph nodes whose end-bounded adjacency establishes each
cap instead of reducing them immediately to two booleans. `PrismaticPocket` publishes the cap at
its accepted closed end as constituent evidence together with its defining wall ring. Defining
evidence, claims, reconciliation and the public record remain wall-only and unchanged.

This is retention at the existing decision site, not a later floor search. All cap patches that
the unchanged proof accepts at the closed end are retained; none is chosen by corpus label,
coordinate rematching or adjacency expansion after recognition. Passages and rejected enclosed
cavities publish no Prismatic Pocket evidence. Non-exclusive membership remains valid when an
accepted cap patch is also constituent to another occurrence.

## Amendment (within-run geometry association summary, issue #462)

The same run-local evidence view may expose one immutable accounting projection over the
constituent evidence already retained by the completed run. The overall associated face set is
the union of every accepted physical occurrence's constituent faces, so overlapping occurrences
never count an original face twice. Per-family contributions are separate unions in closed
registry order and may overlap one another. Total, associated and unassociated face counts and
surface areas retain explicit numerators and denominators; a ratio with a zero denominator is
undefined rather than reported as an invented zero or complete score.

This is **association coverage**, not recognition accuracy, feature recall or proof that any
accepted classification is correct. The denominator is every original caller-part face, including
intentional stock/background geometry, and partial constituent publication produces partial
association. Exact unassociated faces remain opaque within-run `FaceRef` values. The projection
does not rescan topology, traverse residual geometry, classify leftovers, call a recogniser,
serialize references or create a second attribution authority. Like the rest of the evidence
view, it remains raw/caller-coordinate only until framed evidence can map explicitly back to the
caller's faces.

## Amendment (paired framed recognition evidence, issue #463)

The raw-only exclusion above is superseded by a paired framed evidence lifecycle. A successful
framed evidence product owns the inferred `PartFrame`, the exact topology-preserving local working
shape, the original caller part and one `RecognitionEvidence` projected from the same completed
aggregate run. Its `FaceRef` values name working-shape faces in local coordinates; an explicitly
named caller-face resolver maps the same references back to borrowed faces of the caller part.

The mapping authority is exact OCCT topology identity after applying the same retained rigid
`TopLoc` placement used by ADR 0011's normalization to each caller face, never face order,
coordinate proximity, a kernel hash or
geometric correspondence. This distinguishes separate placed occurrences that deliberately share
one underlying TShape while retaining topology-partner provenance. The
relation must be a bijection over every original face in both directions before recognition runs;
absence, duplication, concurrent mutation or a manually constructed prepared value without its
caller authority returns a typed mapping refusal. Raw evidence behavior and reference semantics
remain unchanged.

`PreparedFramedPart` retains the caller part privately and offers the evidence operation after the
consumer has selected its local classification. This preserves the prepared lifecycle's one
cylinder scan and one aggregate run. The framed evidence projection does not call the raw evidence
entry point, build a second graph, expose graph nodes, or publish durable cross-run identity.

## Amendment (Pocket bounded-region membership, issue #420)

The prohibition on reconstructing wider membership later by adjacency flood-fill remains. A narrow
exception is admitted inside the final neutral Pocket proposal proof: an exact inner-wire seed may
traverse concave or smooth original graph arcs and associate the resulting bounded region with the
exact retained wall, cap and floor nodes. This occurs before Candidate issuance; the public evidence
view performs no search or rematch. Both association directions must be one-to-one, and invalid or
ambiguous cases fall back to the existing constituent set. The opening face is proof context, not a
constituent, and defining evidence is unchanged.

This is physical run-local membership, not ownership or persistent identity. It publishes no
adjacency, cavity API, graph type or durable face name.

## Amendment (two-ended Passage enclosure, issue #446)

The same restriction permits one Passage-specific discovery proof before Candidate issuance.
Exactly two complete convex inner-wire mouths may seed the same concave-or-smooth region when both
mouths have matching straight-edged polygonal sections, at least three original planar wall seeds,
one valid solid, and the established empty-prism/open-end proof. The mouth-adjacent planar walls
are defining evidence; the exact traversed region is constituent membership. The opening stock
faces remain consulted context.

This exception recovers a physical Passage whose wall spans no longer form the historical equal-
span cycle. It does not publish a cavity traversal API, infer ownership after recognition, admit a
one-ended pocket or circular bore, or let constituent membership affect reconciliation.

Issue #450 clarifies that exact mouth topology may be used inside this pre-Candidate proof. The
section corners are the shared vertices of consecutive ordered wire edges, rather than the
unordered unique-vertex enumeration returned by `Wire.vertices()`. Public records, original-face
evidence, ownership authority, and the facade boundary are unchanged.

Issue #453 permits the same producer to derive a unique run from the planar wall junctions when
the two opening stock faces are not parallel. Those faces supply only the two local termination
plane equations added by ADR 0016; their identity remains consulted context and never crosses the
facade. The defining and constituent evidence remains the exact original wall region.

## Amendment (interrupted Prismatic Pocket discovery, issue #460)

The one-ended exclusion above is superseded only for `PrismaticPocket` discovery inside its
existing family module. One original principal-plane inner wire may seed a same-solid
concave-or-smooth region when the region contains a unique direct cycle of at least three original
planar wall supports and exactly one distinct principal-plane floor. The support cycle, rather than
the treated mouth wire, defines the constant polygonal section. The complete section must be void
from floor to mouth and immediately outside the mouth, and completely material immediately behind
the floor. Multiple mouths, intersecting candidate regions, a broken or branching support cycle,
multiple floors, a floor breach, a deeper non-mouth interruption, through/enclosed topology and
invalid ownership refuse.

The original planar wall supports are defining evidence. The exact traversed wall, treatment and
floor region is constituent evidence; the opening stock face remains consulted context. This proof
is completed before Candidate issuance and changes neither reconciliation nor the public evidence
facade. In particular, it does not authorize a post-acceptance flood fill, durable face identity,
public adjacency/cavity API, corpus-driven membership, or reading another family's Candidates.
Smooth mouth incidence is accepted only alongside at least one convex incidence and the complete
physical proof, allowing a partial rolling treatment without turning arbitrary tangent openings
into seeds.

Issue #460 additionally permits a floor-seeded proof when a deeper side opening has broken that
mouth-side support cycle. One straight-edged principal-plane floor wire must meet exactly one
concave original planar wall support per edge, all supports must share one principal run direction
and one valid solid, and the floor wire itself defines the non-four-sided polygonal section. A
broken-mouth four-sided floor is ambiguous with the more specific Pocket and rectangular blind-slot
contracts and remains with those producers rather than being widened into a generic record. The
complete section must be void from the floor to one exterior termination and completely material
immediately behind the floor. The exact floor-adjacent wall supports remain defining evidence and
those supports plus the floor are constituent evidence; faces belonging to the intersecting side
opening are neither claimed nor absorbed merely because they touch the cavity.

This narrowly supersedes the deeper-interruption refusal only where the intact floor independently
proves the original prismatic section. Curved or multiply wired floors, missing/duplicate supports,
non-principal or tapered walls, inconsistent run directions, zero depth, a breached floor,
enclosed topology, invalid ownership and competing wall assignments still refuse. The proof stays
before Candidate issuance inside `prismatic_pockets`; it publishes no floor-seeded traversal API
and reads no labels, sibling Candidates, reconciliation or completed evidence.

## Amendment (public oriented Slot projection, issue #310)

`OrientedSlot.source` does not expose the private section graph. It nests the already supported
immutable `SectionPassage` value so callers receive the exact frame, span, serialized rectangle
and open-end proof that authored the projection. Original-face identity remains available only
through the run-scoped evidence facade.

## Amendment (body-owned outer-profile inspection, issue #579)

[ADR0025](0025-body-owned-planar-outer-profile-inspection.md) accepts a bounded same-run
outer-wire inspection for Draftwright's demonstrated angular-support need. Its explicit
exception covers ordered line/arc values and exact source-edge resolution, including lazy
original-wire reads after recognition. It does not widen accepted constituent membership or
publish general adjacency, cross-run correspondence or drawing policy. This contract extends
the accepted pure evidence projections above.
