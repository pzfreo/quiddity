# Epic 0004 — Geometry foundation generalisation

**Status:** proposed
**Owner:** @pzfreo
**Opened:** 2026-08-23
**Baseline:** `44e74df` (`0.3.2.dev0`, after epic 0003 and the `0.3.1` release)

## Purpose

Epic 0003 made recognition execution coherent: every physical result now passes through one
candidate, evidence, freeze, disposition and projection lifecycle. The next limitation is not
orchestration. It is the geometry substrate exposed to family predicates and the public shapes
available to describe what they find.

This epic strengthens that substrate before feature-family expansion resumes. It addresses the
foundational gaps identified by the [3D geometry scorecard](../scorecard.md) (review feedback
incorporated here is recorded in [`epic-0004-feedback.md`](../epic-0004-feedback.md)):

1. exact analytic geometry exported as B-splines currently fails closed;
2. the AAG cannot distinguish smooth joins by material side;
3. recognisers can cross smooth subdivisions but cannot inspect a feature through a removable
   blend chain;
4. several mature records and predicates encode world-axis spans rather than a free local frame;
5. accepted records have run-local Candidate identity but no stable correspondence across runs;
6. defining evidence remains absent for some physical families, limiting measured ownership and
   future interaction rules;
7. the neutral substrate is private, so a third party can build on this package's geometry
   reasoning only by forking it.

The work remains deterministic and rule-based. It does not add a learned recogniser, a plugin
system, machining policy, defeaturing mutations, or a new public feature family.

## Outcome

At completion, a recogniser should be able to consume an imported B-Rep through one documented
geometry pipeline:

```text
imported shape
    -> bounded canonical analytic view
    -> immutable attributed face graph
    -> optional immutable blend-collapsed view
    -> family-owned predicate in a local frame
    -> Candidate with defining evidence
    -> existing freeze / disposition / projection lifecycle
    -> optional cross-run correspondence
```

The public aggregate must remain byte-identical for geometry already inside the current supported
domain, except where a separately reviewed schema migration explicitly authorises a richer record.

## Design principles

### Preserve the epic 0003 phase boundary

Canonicalisation and graph-view construction are neutral context derivation. They happen before
physical discovery and cannot issue Candidates. Family discovery remains write-only. Reconciliation
continues to receive only completed CandidateSets and frozen evidence; it cannot inspect a Part,
canonicalise geometry, collapse a graph or invoke discovery.

### Prefer views to destructive rewriting

The imported Part remains the source of truth. Canonical analytic recovery and blend collapse must
retain provenance back to the original faces. A recogniser may reason over an effective face or
collapsed region, but defining evidence must name the original `FaceNode` objects that established
the record. Nothing in this epic edits or defeatures the caller's model.

Shape-level canonical replacement may be used internally when OCCT requires it, but it must produce
an immutable run-owned analysis shape and an explicit original-to-analysis provenance map. It must
not silently replace the public Part or make record coordinates depend on replacement order.

### New geometry facts are total; acceptance stays family-owned

The shared AAG reports analytic candidates, smooth-sided arc kinds, collapsed regions and local
frames as neutral facts. It does not label machining features or discard awkward topology.
Recognisers decide whether those facts are sufficient for their own contract, following ADR 0009.

### Fail closed with a bounded residual

Canonical recovery is accepted only when the fitted primitive stays within a declared local
residual and preserves the topology required by downstream graph construction. Blend collapse is
accepted only when a complete, unambiguous chain satisfies its neutral contract. Unsupported or
ambiguous geometry remains visible as the original graph and produces no expanded recognition.

No corpus-derived numerical threshold may enter these predicates. Length comparisons use ADR 0008
and the smallest controlling local nominal.

### Public schemas change by supersession

An axis-aligned record cannot be made oblique by broadening the meaning of `axis="x"`. Free-axis
support therefore uses a local orthonormal frame and section-based geometry in a new schema or a
new versioned record. Existing record meanings remain stable through their documented compatibility
window. Reconciliation may prefer the richer complete record over a legacy fragment only through a
named identity/evidence rule.

## Work packages and sequence

Each package is a separate PR-sized issue. Later packages may depend on earlier neutral APIs, but
no PR may combine a substrate change with a new feature predicate merely to demonstrate it.

### F0 — Baseline, external evidence and invariants

Freeze the evidence before changing geometry semantics.

Deliverables:

- record the exact current golden, NIST, MFCAD++ and performance results;
- add an external-corpus scanner for MFTRCAD instance and relationship annotations without
  vendoring the complete dataset;
- document MFTRCAD licence, file/annotation identity, taxonomy mapping and known invalid-topology
  handling before any sample enters the repository;
- select deterministic development and sealed holdout manifests by rule, not by observed success;
- inventory and freeze the existing native-analytic/B-spline, blend-chain, oblique-frame,
  traversal, mirror and scale adversaries, adding a fixture only where that inventory finds a gap;
- pin current empty output on unsupported variants so later gains are attributable.

MFTRCAD is evidence, not an oracle. Its feature-instance and relationship labels can identify
interactions and false negatives, but its taxonomy does not define this package's record contracts
or reconciliation policy. Synthetic-corpus measurements remain separate from real-part evidence.

The checked-in rigid-motion sweep at baseline `b03ba00` closes a separate synthetic-corpus blind
spot. It validates face-by-face correspondence before matching accepted occurrences by defining
evidence, so absence and reconciliation-driven family changes are measured rather than inferred
from counts. Of 75 golden-corpus census records, Z30 retains 34 in-family, reclassifies 14 and loses
27; X30 retains 29, reclassifies seven and loses 39; the X90 axis-permutation control retains all
75. See `docs/benchmarks/rigid-motion-sweep.md` and its canonical JSON companion. These numbers
order later F4b family work but do not themselves decide whether the supplied world frame is
semantically meaningful or amend ADR 0001.

A follow-on part-relative normalization prototype now tests that decision rather than stopping at
measurement. Independently normalizing every original and rotated golden fixture retains all 75
occurrences in-family at Z30, X30 and X90. On a deterministic 100-model MFCAD++ development sample,
the 98 comparable X30 models improve from 217 same-family / 341 absent occurrences to 561
same-family / one absent, at about 2.4% normalization overhead on a 20-model timing sample. This is
strong evidence for continuing, but not a shipping decision: normalization changes 19 accepted
fragments on the unrotated 100-model sample, four normalized rotation comparisons still differ,
and two models trip legacy Passage compatibility. The public local-frame/free-axis contract and
those reconciliation failures remain the exit gate. See
`docs/benchmarks/frame-handling-prototype.md`.

Exit gate: the scanner reproduces a documented baseline without changing source recognition code;
the holdout stays sealed.

### F1 — Bounded canonical analytic recovery

Introduce a private, lazy, run-owned `EffectiveSurfaceIndex` keyed only by the original
`FaceGraph`'s issuer-owned `FaceNode` values. F1 rejects an analysis-shape pre-pass: a second
topology universe would require a face bijection before Candidate evidence could remain truthful.
The index reads `graph.face(node)`, caches one immutable result per node, never substitutes the
caller Part or graph, and retains that exact original node as the only evidence provenance.

The closed result is native analytic, uniquely recovered analytic, or refused original. Native and
recovered plane, cylinder, cone and sphere facts contain canonical finite numeric parameters,
original node, a closed orientation capability (`NATIVE_ORIENTED` or `RECOVERED_UNORIENTED`),
requested tolerance, kernel-reported deviation and the separate
verified acceptance bound. Refusals retain the original surface behind a private query and use a
closed reason: unsupported kind, unavailable fit, invalid/nonfinite result, bound exceeded,
or ambiguous primitive. An oriented-view request against `RECOVERED_UNORIENTED` separately refuses
as `ORIENTATION_UNPROVEN`; this is not a geometry-recovery failure. Native analytic faces use a
zero-recovery fast path. Only B-spline/Bezier faces enter recovery; torus recovery remains an
explicit unsupported refusal until a separately reviewed fitter, residual proof and performance
gate exist.

`ShapeAnalysis_CanonicalRecognition.GetGap()` is recorded only as `kernel_reported_gap`; F1 does
not independently rename that scalar a maximum. The acceptance certificate is the documented OCCT
`ShapeAnalysis_CanonicalRecognition` face operation itself: OCCT's official shape-healing contract
defines recognition by the maximum-distance criterion over the input face. Each primitive attempt
uses a fresh/reset recogniser, requires success and status zero, records `GetGap()`, and requires it
not exceed the requested tolerance. The supported OCCT version and source/documentation contract
are pinned by test and ADR. A finite UV sample is only an adversary, never the certificate. If that
upstream maximum-distance contract changes or cannot be established for the installed version,
recovery refuses globally rather than substituting a sampled bound.

All four eligible primitive fits are evaluated independently. Canonical parameters use deterministic
run/sign, closest-axis-point and frame conventions. Multiple materially non-equivalent passing
facts produce `AMBIGUOUS_PRIMITIVE`; call order is never precedence. Degenerate trims, huge-radius
near-planes, cylinder/cone ambiguity, sphere poles/seams and split faces fail closed unless the
same uniqueness and bound are proven.

Topology, boundaries, adjacency, `TopAbs_Orientation`, material-side probes and Candidate evidence
always use the original face/solid/graph. F1 deliberately recovers **unoriented primitive
geometry**: canonical axis/frame signs are serialization conventions, not material-side facts.
Recovered geometry may not answer normals, concavity, outwardness or any orientation-dependent
family rule. Those readers remain classified raw/deferred until a separately reviewed
recovered-orientation capability and consumer slice supplies
material-side semantics; a request for oriented recovered geometry returns
`ORIENTATION_UNPROVEN`. This removes the unsafe one-anchor/global-parity claim while preserving
current original-face behaviour. Effective facts cannot replace topology or decide a family
predicate.

Recovery tolerance is an ADR 0008 same-geometry policy fixed before corpus measurement:
`fit_tol = relative * local_nominal + coordinate_floor`. For trimmed-face area `A > 0` and physical
trim-boundary length `P >= 0`, the rotation/translation-invariant nominal is
`min(sqrt(A), 2*A/P)` when `P > 0`, otherwise `sqrt(A)`. Area and boundary length are measured on
the original face in model units. `P` counts each physical boundary component once and excludes
periodic seam pairs and degenerate representation edges; a topologically closed face therefore has
`P == 0` regardless of seam parameterisation. Nonfinite/nonpositive area or a nonfinite/negative
perimeter refuses. This makes long-thin patches width-controlled without using a world AABB, while
closed sphere-like faces use their area scale. Same geometry with different STEP seam
parameterisation must produce the same nominal. The exact relative coefficient and coordinate floor are
named and justified in ADR 0008 before corpus inspection. Requested tolerance, kernel-reported gap
and the OCCT maximum-distance certificate remain distinct full-precision values.

F1 is staged rather than migrating every distributed surface read in one PR:

1. land the neutral four-primitive index, refusals, caching, provenance and architecture guards
   with zero recognition-output change;
2. freeze a machine-checked roster of every `BRepAdaptor_Surface.GetType`, `Face.geom_type`,
   `graph.is_planar` and equivalent decision, classifying each as migrated, deliberately raw
   topology, orientation-deferred, or torus-deferred. Every non-migrated entry requires a named
   rationale; raw surface classification may not remain a family-acceptance escape hatch;
3. migrate consumers in explicitly ordered family slices. Private cores receive a restricted
   read-only surface query; public wrappers construct one graph/index for standalone use and
   registry adapters inject `RecognitionContext.surfaces`. Families cannot construct or invoke
   the fitter. Cylinder analysis and every public/aggregate caller must share one surface universe.

The neutral slice changes no public signature, capability manifest, record, Candidate, disposition
or reconciliation policy. ADR 0004 owns original-node provenance and the immutable view; ADR 0007
owns the module/core-wrapper and reader-roster seams; ADR 0008 owns the residual policy; ADR 0002
is amended when standalone/aggregate injection first changes. ADR 0005 applies only if a later
slice changes a public signature or capability contract.

Exit gate: native and supported equivalent encodings return byte-identical records, ordering and
defining `FaceNode` identities for every migrated consumer; unsupported, ambiguous and unbounded
B-splines refuse; direct and aggregate entry points agree; one recovery occurs per original node
per run; semantic goldens remain byte-identical. Measure native-only index overhead separately from
B-spline recovery runtime/peak RSS without rebasing existing ceilings. The two-review and holdout
chronology applies separately to any slice that changes recovery acceptance. Torus remains named
unsupported unless independently authorised.

F1 neutral-slice delivery chronology: after the recovery contract was frozen, two independent
exact-head reviews accepted it, the full non-holdout suite passed, and the composite budget passed,
MFTRCAD buckets 10–19 were authorised for reveal. The run reached a pre-existing Slot-recognition
`Standard_DomainError` while constructing a probe box and aborted before producing a complete
report. The lazy F1 index has no production consumer in this slice and was not queried by that
failure, so the attempt supplies no F1 score, pass, regression conclusion, or recovery-quality
evidence. Those buckets are nevertheless revealed and permanently consumed. The only post-reveal
implementation delta is generic refusal-branch test coverage; no recovery predicate, tolerance,
certificate, or production source changed.

### F2 — Complete smooth-sided AAG semantics

Preserve the current closed `ArcKind = convex | concave | smooth | unknown` and `None`-for-
non-adjacency contract byte-for-byte. Add a separate private closed `SmoothSide = neutral | convex |
concave | unproven`, queried only when the legacy pair arc is `smooth`. A named `is_any_smooth`
reads only the legacy first-order fact. `smooth_region` and every existing direct smooth caller
migrate through that helper, while exact nonsmooth callers keep their present comparison. No family
consumes `SmoothSide` in F2, so unavailable enrichment cannot tighten or relax recognition.

Sidedness is certified only from original closed-solid topology; F1 recovered facts remain
`RECOVERED_UNORIENTED`, and `oriented_fact` continues to refuse them. At each shared edge the graph
requires exactly two distinct incident faces owned by exactly one same original closed manifold
solid. Open faces/shells, cross-solid pairs, seams/self-adjacency, duplicate or ambiguous face
ownership, non-manifold incidence and ownership lookup failure make only `SmoothSide` unproven;
they never rewrite the legacy pair arc.

The legacy pair result is computed and cached first by today's exact midpoint/all-shared-edge
algorithm. Closed-solid eligibility and every second-order check gate only `SmoothSide`; they can
never veto or rewrite that legacy result. If the legacy result is `smooth`, then for each regular
nondegenerate shared edge sample deterministic arc-length fractions 1/4, 1/2 and 3/4. At each
sample, obtain each original face's outward normal and its inward boundary co-normal from the
face-oriented edge walk. Project that co-normal into the surface's first derivatives and evaluate
signed normal curvature from the second fundamental form:

`k = dot(n, a*a*duu + 2*a*b*duv + b*b*dvv) / |a*du + b*dv|^2`.

The sign convention is frozen by constructed geometry: negative outward-normal curvature is
smooth-convex and positive is smooth-concave. Curvature is made dimensionless with local length
`L = min(edge_length, sqrt(face_a_area), sqrt(face_b_area))`. `L` must be finite and positive.
With ADR-0008 constant `SMOOTH_CURVATURE_GAP = 1e-6`, `neutral` requires a stronger continuation
certificate: the two original surfaces must both be native analytic and have equivalent canonical
plane/cylinder/cone/sphere parameters. A new topology-free private `_analytic_surfaces` leaf owns
canonicalisation, finite/domain validation and equivalence; both `_effective_surfaces` and
`_adjacency` depend on it, and it imports only OCP plus `_geometry`. It owns no graph/node,
recovery, orientation, evidence or cache. This refactors F1 authority without changing recovery.

For local `L`, equivalence length tolerance is `1e-9 * L + COORD_FLOOR`; axis equivalence requires
`1 - abs(dot) <= 1e-9`; cone semi-angle difference is at most `1e-9` radians. Plane offsets use
the length tolerance. Cylinders require axis, closest-axis-line distance and radius agreement;
cones require axis, apex and semi-angle agreement; spheres require centre and radius agreement.
No kernel-handle identity shortcut is allowed because one surface may be instanced with different
placements. Curvature equality alone never proves neutral; a plane joined to a quartic tangent
surface is therefore `unproven` even when both boundary curvatures are zero.

Without that continuation certificate, each sample can prove a side only when the normalized
curvatures are materially unequal. A zero is omitted from sign unanimity only when its source is a
proven plane. Any other `abs(k*L) <= gap`, `abs((k_a-k_b)*L) <= gap`, empty remaining sign set, or
opposite strict signs is `unproven`. All remaining values strictly negative prove `convex`; all
strictly positive prove `concave`. Unavailable/degenerate D2 data, projection failure,
contradictory samples, seam/pole instability or unreliable orientation are also `unproven`.

Each immutable per-edge sided observation is cached against the exact original unordered node pair
and shared-edge identity, including `unproven`. The authoritative legacy pair arc remains in its
existing unordered-pair cache; the `SmoothSide` reduction has its own unordered-pair cache. All
three sided observations on every shared edge must agree, and multiple shared edges must agree;
otherwise the side is `unproven`. Swapping nodes, reversing edge traversal, kernel face order and
shared-edge order cannot change either fact.

Some fail-closed reducer states need not be representable by a valid development solid. When an
exhaustive checked-in-fixture scan finds no smooth multi-edge pair (as at F2), the acceptance
evidence may use real kernel observations up to the unavailable boundary and a narrowly injected
frozen observation for only the reducer state. The test must name the scan result and the exact
boundary it substitutes; it may not replace an available end-to-end convex, concave, neutral,
unproven, imported, ownership, seam/pole or non-manifold fixture.
The frozen F2 scan also found 36 regular plane/curve samples and no curved/curved same-sign or
opposite-sign sample in the semantic fixtures. Those two algebraic sign reductions therefore use
direct curvature observations; the available plane/round signs remain end-to-end fixtures.

Freeze an AST caller roster before migration. Current production dispositions are exact
nonsmooth comparisons in `_recess_core` and any-smooth traversal in `FaceGraph.smooth_region`;
tests/tools are classified too. Compatibility traversal uses `is_any_smooth(arc)`; sided reads use
only `smooth_side`. Truthiness and negative inference from `unknown`/`None` are forbidden after F2.
`_adjacency` retains ownership of both facts, observations and caches and may not import F1,
families, orchestration, claims or reconciliation.

Required evidence includes coplanar and same-cylinder/sphere/cone neutral splits; a plane-to-quartic
tangent false-neutral refusal; external boss and internal pocket rounds; unequal same-sign
curvature; an inflection/opposite-sign refusal;
mirror, rigid transform, scale, node/edge permutation and reversed orientation; periodic seams,
poles, degenerate edges/D2, open Face, two-solid Compound and non-manifold refusal; agreeing and
disagreeing multi-edge pairs; STEP round-trip; and mutation tests for all four `SmoothSide`
branches. Exact public records/order/to_dict, Candidate defining-node identities, full goldens and
performance remain unchanged. Close #129 as satisfied/superseded, retaining only this richer
smooth residual in #181.

Because F2 has no sided consumer and must preserve recognition output, it does not spend another
recognition holdout. Freeze algorithm and ADR-0008 constants before development-arc inspection,
then require synthetic/imported-development arc matrices, full/static/package/performance evidence
and two exact-head accepts. A future F3 or first sided consumer owns a separately predeclared
untouched holdout; consumed MFTRCAD buckets 10–19 may never be reused.

### F3 — Immutable blend-collapsed graph views

F3a adds a private derived topology without changing recognition. `_blend_view` sits above
`_adjacency` and `_effective_surfaces`, receives restricted graph/surface queries, and may not import
families, run orchestration, claims, Candidates, reconciliation or records. The base `FaceGraph`
remains immutable and complete. F3a promotes F2's existing private same-solid/two-face proof into
one immutable graph-owned `EdgeOwnershipFact` rather
than re-walking `Part.solids()` in the view. The fact contains an opaque run-issued `SolidRef`, the
exact two graph-issued incident `FaceNode`s and a graph-issued `SharedEdgeOccurrenceRef`. That
adjacency occurrence pairs exactly two `EdgeOccurrenceRef` half-edges, one per incident node; each
half-edge contains its owning node, face-wire occurrence ordinal/orientation, underlying topological
edge identity and issuer snapshot. The pair is unordered by endpoint and issued once, so traversal
cannot duplicate it while periodic seams and repeated oriented occurrences stay distinct. The fact is available
only for one valid closed manifold solid with exactly two incident faces; open, ambiguous,
cross-solid, duplicate and nonmanifold ownership are closed refusals. Issuer snapshots are
revalidated on every read.

The first closed grammar supports only original **native constant-radius cylindrical** blend
patches. Sphere, cone, torus, B-spline/Bezier and recovered-unoriented surfaces refuse. Radius is
finite/positive and compared through `_analytic_surfaces`. Discovery starts from maximal connected
native-cylinder blend components and maximal native-neutral support regions; it never proposes an
eligible subset of a refused maximal component. The split-invariant local nominal is
`L = min(radius, total physical length of each complete spring/terminal group,
sqrt(aggregate area) of the blend component and each support region)`; seam and degenerate
representation edges are excluded and every term must be finite and positive. Radius and
closest-axis-line distance use
`1e-9 * L + COORD_FLOOR`, while cylinder-axis equality uses
`1 - abs(dot(left, right)) <= 1e-9`. Edge roles and coverage use exact original shared-edge
topology, not coordinate proximity. Part/world bounds, rounded record values and fitted or
recovered radii never set this policy. ADR-0008 owns these constants before development inspection.
Candidate blend nodes form one nonbranching strip/path in one exact closed manifold solid. Every
blend patch has exactly one nonempty (possibly split-edge) spring group to each of the same two
distinct support regions and no other smooth-sided neighbour. A singleton component has internal
blend degree zero; a multi-patch path has exactly two degree-one ends and every other patch degree
two. Their spring
boundaries are legacy-smooth blend-to-support arcs with one uniform proved F2 side, convex or
concave. Equivalent split supports are regions joined only by native-analytic neutral continuation.
Internal blend-to-blend cross arcs require legacy smoothness, neutral continuation and equal
cylinder parameters/radius. Remaining terminal cross arcs are non-spring boundaries from a blend
end to retained nonblend faces. A terminal group is a maximal connected component of those exact
edge occurrences plus their incident retained faces; exactly two nonbranching groups must exist and
are retained as provenance only. Vertex-only contact is never an edge. Every original boundary edge
must be exactly spring, internal cross, terminal cross or an identified periodic seam. Unaccounted
edges, partial support, periodic closed chains without two terminal groups, cycles, branch degree
greater than two, mixed side/radius/body, ambiguous
ownership, nonmanifold incidence or overlap refuse the complete component. Convex and concave strips
are both eligible, but never mixed.

`BlendCollapseIndex(base, surfaces)` atomically binds both restricted capabilities to the same
graph-issued run token, then discovers and caches issuer-owned frozen `BlendChain` occurrences once;
construction itself changes nothing. Empty graphs still validate the binding. Every returned
surface fact is revalidated against its requested original node. A foreign or mixed capability is a
construction error, not a geometric refusal. A caller must explicitly pass selected
same-index chains to `index.view(selected)`. There is no collapse-all default. Selection is validated
atomically: foreign/copied/mutated/stale chains or overlap in any blend face/spring/cross occurrence
refuse the whole selection and create no half-view. Otherwise disjoint parallel chains may share the
same two support regions and remain distinct arc occurrences. Incompatible overlapping support
partitions refuse the whole deterministic conflict component before any chain is issued; discovery
order never selects a winner.

`CollapsedGraphView` is explicitly a bounded support-bridge abstraction, not replacement topology,
a `FaceGraph` subclass or a duck type. Frozen
issuer-owned `LogicalNode` and `LogicalArc` occurrences validate on every cold/warm read. A logical
node is one selected maximal support region or a singleton retained face, and expands to a nonempty
immutable set of exact original graph-issued `FaceNode`s. Selected blend faces have no logical node
and exist only in provenance. Every base edge occurrence whose retained endpoints map to distinct
logical nodes is projected once; occurrences internal to an aggregated support are stored in that
node's provenance, while every occurrence incident to a hidden blend node is deliberately absent.
No synthetic support-terminal incidence is invented. `view(())` therefore has singleton nodes and
every original base occurrence. Each selected chain adds one synthetic sharp `LogicalArc` between
its two distinct support regions, with `convex`/`concave` derived from the uniform spring side. It
claims no kernel curve. `arcs_between` returns a tuple of occurrences, preserving parallel chains
and any existing support adjacency rather than collapsing them to one pair value.

`OriginalArcRef` is an issuer-owned unordered original-node pair plus exact graph-owned
`SharedEdgeOccurrenceRef`. `FrozenProvenance` for a synthetic arc contains both support regions, every hidden blend
node, and every original spring/internal/terminal arc. Expansion is deterministic and complete.
Occurrence tuples use the original graph's run-local node/wire/edge order solely as a stable
presentation order; tuple position and node index carry no geometry, ownership or cross-run
identity. A
future consumer must expand the selected logical occurrence, then #192 must explicitly classify the
original nodes as defining or consulted evidence under the existing evidence semantics before
Candidate issuance. Complete provenance does not itself imply ownership. Logical handles themselves
are never sink-compatible, and `_blend_view` cannot issue evidence.

F3b selects Polygonal Bosses as that sole consumer. Only a complete six-chain convex singleton
cycle between six retained planar vertical supports is selected, and every synthetic arc is
expanded and revalidated before use. The six original planar supports remain the complete defining
Candidate evidence. Hidden cylindrical blend nodes, spring/internal/terminal occurrences and cap
context are consulted only. Direct and aggregate discovery share the existing Polygonal Boss core;
other families and Polygonal Stock remain base-graph-only. The development fixture is a regular
hexagonal attached prism with its six vertical corners filleted at constant radius, and its output
is required to equal the sharp control exactly. Partial or competing cycles refuse rather than
selecting a subset. Fillet Candidates, when independently present, define the cylindrical faces and
therefore do not overlap the Polygonal Boss planar defining set; no new reconciliation rule exists.

Refused discovery attempts are closed values with named reasons; selecting one is impossible.
`view(())` is the exact base projection, and a refused component cannot alter retained node identity,
neighbours, base arc occurrences or caches. Cache surface/radius reads, per-edge roles, component
classification (including refusal), selection validation and expansion once. Invariance compares
logical incidence and exact original provenance under an explicit face/edge correspondence—not node
indices, traversal order or aggregate counts.

F3a lands only this neutral private index/view, ADRs and synthetic/imported-development matrices,
with zero registry/context/public/output change and no holdout. After two exact-head ACCEPTs, a
separate F3b issue #192/PR names one existing family, one development fixture, its exact chain-selection
rule, direct/aggregate injection and defining-versus-consulted classification. F3b alone owns output/reconciliation/corpus
changes and a predeclared untouched holdout; consumed buckets 10–19 remain forbidden.

Required evidence includes convex/concave single and split strips; ordinary cylinders that must not
collapse; mixed radius/side, partial, branch, cycle, seam, vertex-only, duplicate support, open,
cross-solid and nonmanifold refusal; overlap and parallel/existing-arc multiplicity; foreign/copied/
mutated/stale handles; exact expansion; mirror/rotation/scale/traversal/STEP; refusal identity and
cache counts. Measure view-not-constructed, constructed-unqueried and queried costs separately.

ADR-0004 owns logical-to-original node/arc provenance and whole-component conflict refusal;
ADR-0007 owns the one-way module/capability seam and forbids `FaceGraph` substitution; ADR-0008 owns
the numerical policy above; and ADR-0003 owns expansion and explicit classification as original
defining/consulted evidence before sink issuance. ADR-0002 and ADR-0009 change only in the separately
authorised F3b consumer slice.

### F4 — Free-axis local frames and section records

Provide one shared immutable local-frame representation for geometry that is not aligned with world
X/Y/Z, then migrate the recess contracts that cannot truthfully express oblique geometry.

Minimum internal values:

```python
@dataclass(frozen=True)
class LocalFrame:
    origin: tuple[float, float, float]
    run: tuple[float, float, float]
    u: tuple[float, float, float]
    v: tuple[float, float, float]


@dataclass(frozen=True)
class SectionVertex:
    point: tuple[float, float]
    bulge: float  # tan(signed circular sweep / 4); zero is a line


@dataclass(frozen=True)
class PlanarSection:
    boundary: tuple[SectionVertex, ...]


@dataclass(frozen=True)
class SectionEnds:
    low_capped: bool
    high_capped: bool


@dataclass(frozen=True, eq=False)
class SectionOccurrence:
    body: BodyRef
    frame: LocalFrame
    run_interval: tuple[float, float]
    section: PlanarSection
    ends: SectionEnds
```

The bulge form is the closed line/arc union: each vertex starts the segment ending at the next
vertex; zero is a line and a finite non-zero value is the circular arc whose signed sweep is
`4*atan(bulge)`. It represents the existing polygonal sections without loss and does not force
obround slots/pockets into a false polygon. A full circle uses at least two arcs.

`PlanarSection` is intrinsic 2-D geometry. Placement, run extent, end topology and run-owned body
identity belong only to `SectionOccurrence`. `SectionEnds(False, False)` represents a through
section; the current blind adapter requires exactly one capped end, preserving which end is open.
An orchestration-owned issuer creates and validates `BodyRef`; records and callers cannot construct
or copy one into another run.

Canonical winding, area and centroid are analytic over the complete line-and-circular-arc loop,
not over its chord polygon or vertex mean. For bulge `b`, sweep is `4*atan(b)` and the circular
segment's signed area and Green-theorem first moments are included. Equivalent subdivision of an
arc therefore leaves area, centroid, frame origin and reconstructed geometry unchanged within the
named local geometry tolerance. Reversal maps each reversed edge to the negated bulge of the
oppositely directed original edge before choosing the canonical cyclic start. Non-adjacent
line/line, line/arc and arc/arc crossings, overlaps and tangencies are rejected; adjacent segments
may meet only at their shared endpoint. Bulges use a separate dimensionless serialization
precision, and serialization fails closed if a non-zero arc becomes zero or reconstruction moves
beyond the local tolerance.

Exact public names are deferred, but the invariants are not:

- `run`, `u` and `v` form a canonical right-handed orthonormal frame;
- intrinsic sections are origin-centred and frame origins are perpendicular to `run`, so inverse
  section/frame or origin/interval translations cannot create a second encoding of one geometry;
- sign and basis tie-breaks are deterministic under equivalent topology;
- sections have canonical winding and start vertex;
- run-local occurrence identity includes an orchestration-owned body reference, frame, run
  interval, section geometry and end topology; the pure frame/section values do not contain
  kernel objects;
- principal-axis inputs continue to project byte-identical legacy records during migration;
- oblique geometry is represented by a section record, never squeezed into `axis: str` spans;
- reconciliation names when a complete section record supersedes an axis-span fragment;
- schema/version/capability changes follow ADR 0005 and downstream golden migration.

The version-1 proposal also owns a normative consumer contract: world reconstruction uses the
rounded serialized basis directly (`origin + t*run + x*u + y*v`), never an unspecified
re-orthonormalization; serialized frame residuals have explicit validation bounds; vector lengths,
finite non-boolean numerics, end booleans, interval order and positive simple boundary winding are
validated. Length values are millimetres under the current capability contract. The nested value
inherits the future enclosing family record's capability-manifest `schema_version`; it does not
start a second version-negotiation protocol.

The discrete canonical-frame gauge is chosen from the six-decimal serialized run (positive
dominant component, ties Z→Y→X), while analytic vectors remain full precision. A consumer derives
the same expected basis for validation but reconstructs with the serialized vectors unchanged.
Serialized intrinsic centring and origin/run perpendicularity have explicit projection-derived
bounds. Every private occurrence read/projection revalidates the canonical frame, section, interval,
end topology and run-owned body provenance so reflection or foreign-state mutation fails closed.

This package is explicitly split into two halves with different risk and different clocks:

- **F4a — the schema**: private frame/section primitives, canonical tie-breaks, concrete
  legacy→section→legacy parity adapters for records that already carry truthful sections, and an
  independently reviewed versioned public proposal. It requires no recogniser changes. F4a lands
  early (see the recommended order) so F1 fixtures, F5 evidence and later F4b records are written
  once against the final geometry shape. The primitives remain private until F7; the first F4b
  family that emits a richer feature record owns the ADR 0005 public-schema transition.
- **F4b — the oblique predicates**: the hard geometry work in the `_recess_*` subsystem,
  delivered family-by-family whenever ready, with no shared cliff.

Sequence within the halves: neutral private frame primitives; versioned public proposal; exact
private compatibility adapters (F4a); then family-private oblique predicates and an authorised
public record transition; only then deprecation (F4b). Do not rewrite the whole `_recess_*`
subsystem in one PR. The package has no public deserializer, so "dual read" means consumer-owned
reading of the proposal; this repository proves only pure legacy→section→legacy projection.

Exit gate: all rotations, mirrors and traversal permutations give canonical frames; principal-axis
goldens remain stable; a separately authorised oblique corpus set gains records with zero off-target
defining claims; Draftwright explicitly reviews the schema transition before production pin movement.

F4b Section Passage holdout chronology: after Draftwright schema acceptance, two independent
exact-head implementation accepts and the full static/test/package/corpus/performance/CI gates,
`F4B-SECTION-PASSAGES-H1` designates only bucket 36 with token
`f4b_section_passages_h1`. The designation is neutral policy/tests/docs work and accesses no
archive membership, annotation, STEP, geometry, recognition or outcome. Bucket 36 begins
`sealed_unrevealed`; buckets 34/35 remain independently sealed and unauthorized; the ordinary
complement becomes 37..999. A later exact authorization consumes bucket 36 permanently even on
zero, invalid input or abort, with no retry, alternate allocation, replacement or post-reveal
fitting.

The sole authorized F4b attempt at rebased PR #252 head
`438088ae1b14b3c9b6883d84f343bb6abfa392a5` used exact acknowledgement
`F4B-SECTION-PASSAGES-H1`. Its available sealed root contained no models matching bucket 36, so
discovery stopped before annotation, STEP or recognition and no report was created. No alternate
root/allocation, retry, replacement or fitting followed. Bucket 36 is permanently consumed and
inconclusive, not regression evidence; buckets 34 and 35 remain independently sealed.

F3b Polygonal Boss holdout chronology: before semantic implementation or holdout inspection,
`F3B-POLYGONAL-BOSSES-H1` neutrally designates only bucket 37 with token
`f3b_polygonal_bosses_h1`. The policy/tests/docs designation accesses no archive membership,
annotation, STEP geometry, recognition or outcome. Bucket 37 begins `sealed_unrevealed`, buckets
34/35 remain independently sealed and unauthorized, and the ordinary complement becomes 38..999.
Only exact acknowledgement `F3B-POLYGONAL-BOSSES-H1` after frozen gates and two independent
exact-head accepts may authorize one attempt; it permanently consumes bucket 37 even on zero,
invalid input or abort, with no retry, alternate allocation, replacement or post-reveal fitting.

The sole authorized F3b attempt at merged-main head
`d7efa41c92ff57d873e618eb8318df1a36a3d76b` used exact acknowledgement
`F3B-POLYGONAL-BOSSES-H1`. Its available sealed root contained no models matching bucket 37, so
discovery stopped before annotation, STEP import or recognition and no report was created. No
alternate root/allocation, retry, replacement or fitting followed. Bucket 37 is permanently
consumed and inconclusive, not regression evidence; buckets 34 and 35 remain independently sealed.

That allocation chronology is now historical. The current schema-2 corpus policy uses one stable
hash split: MFTRCAD buckets 0..499 are open development/validation data and buckets 500..999 are the
sealed final holdout. Feature-specific selectors, acknowledgements and one-shot reveals are retired;
their status remains only as provenance. Malformed development inputs are recorded and skipped.
The complete MFCAD++ corpus is open validation data. This simpler policy measures aggregate utility
throughout development while retaining one substantial MFTRCAD half for final evaluation.
Old holdout buckets 10..19 and unrevealed allocations 34/35 move into development; previously
accessed buckets are not newly independent evidence. Repository chronology records no earlier
`unselected` scan of buckets 500..999, an attestation about recorded runs rather than proof that
external access was impossible.

### F5 — Complete defining-evidence migration

Move every physical registry definition from deliberate empty evidence to truthful defining
evidence where the record has a geometric ownership proof. This is a staged programme, not one
22-family implementation PR.

Required contract:

- every physical registry definition carries one closed private disposition:
  `FullyAttributed(proof_contract)` or
  `IncompleteAttribution(reason, follow_up_or_exclusion)`, with every string non-empty;
- `FullyAttributed` is a family-completeness promise: every returned aggregate occurrence on every
  output path has non-empty defining evidence. `IncompleteAttribution` may contain useful measured
  occurrences as well as empty ones and tooling must report those separately;
- the frozen baseline has exactly six complete families: PrismaticPockets, Passages, Grooves,
  TurnedSteps, Chamfers and AngledSteps. Slots/Pockets are partial, Channels deliberately writes
  nothing, and the other thirteen physical families begin incomplete;
- after the sole terminal evidence freeze and before reconciliation, orchestration validates every
  complete-family Candidate against the registry declaration and its issuer-owned frozen evidence;
- evidence names only original graph nodes that establish the record, not stock/context faces;
- `FaceGraph.common_valid_solid(nodes)` is the sole graph-owned membership proof. Every non-empty
  non-LEGACY aggregate physical defining set, complete or partial, must prove unambiguous membership
  in exactly one valid closed `SolidRef` atomically before Candidate publication and again from the
  terminal frozen evidence. `LEGACY` standalone compatibility retains its existing graph-membership
  boundary. Body provenance is generic, while defining-versus-context roles remain family-owned
  geometry contracts;
- direct recognition remains unchanged with or without the writer;
- equal-valued occurrences stay identity-distinct;
- empty evidence never proves containment, precedence or compatibility;
- corpus reports distinguish measured ownership precision from fitted record counts;
- capability manifest format 1 remains unchanged. Attribution appears in reviewed private metadata
  and capability prose unless ADR 0005 separately authorises a new public format.

F5a lands the closed 22-family metadata, common-solid capability, post-freeze validator, ADRs and
registry-driven tooling with no status promotion or output change. F5b adversarially audits the six
already-complete families. Later families migrate one private core per independently reviewed PR;
public signatures remain unchanged, no discovery reader receives frozen evidence, and a family does
not gain a reconciliation rule merely because it now has claims. A rule requires an observed
overlap, a named geometry relation and separate review.

F5b is a frozen tests-and-documentation audit with this exact path matrix:

- prismatic pockets: triangle, rectangle and hexagon rings, either cap/open sign and multi-ring
  ordering; every ring wall defines the record while the sole cap/floor does
  not;
- passages: triangle, rectangle, hexagon and concave-U rings across supported principal axes and
  multi-passage ordering; the uncapped ring defines the record while
  mouth/stock faces do not;
- grooves: sharp-band, conical/chamfered-lead-in and toroidal/radiused-lead-in joins, plus multiple
  grooves; only each floor band defines its record, never shaft bands, walls, cones or tori;
- turned steps: ordinary and groove rungs, supported principal axes and a split/multiple-widest-band
  case; all and only the widest external bands containing the rung midpoint define it. Only when no
  band contains that point may the frozen per-band bounded edge-break pad establish eligibility;
  every widest eligible tie defines the rung, not shoulder planes. Those planes establish the
  serialized interval as consulted context, so an edge-break-imported band may be narrower than the
  shoulder-delimited rung;
- chamfers: planar bevel and turned conical constructors, including rotational projection; only the
  bevel/cone defines the record, not bridged planes or the external cylinder;
- angled steps: ordinary and drilled triangular terminals, multiple/reversed ordering, and the
  split-terminal near miss; only the slant defines a successful Candidate, while a permitted
  diagnostic Observation must never become an orphan Candidate.

Every positive fixture compares independent writer-off/on record type, value, order and `to_dict`,
then proves same-run Candidate count, family, `candidate.record is returned_record`, nonempty
issuer-valid common-solid evidence and no surplus Candidate. Every negative fixture proves no family
Candidate, with the bounded AngledStep Observation stated separately. Correspondence is sampled at
the representation-sensitive seams rather than multiplied across every topology row: mirrored
pocket cap orientation, scaled groove floor binding, reversed bevel/step issuance order, cross-axis
turned bands and a real STEP turned Chamfer. These compare independently derived defining roles,
never FaceNode indices or byte-identical scaled records. The frozen development fixtures contain no
two valid same-body output occurrences that serialize to equal values; manufacturing one requires
coincident duplicate solids or direct sink injection and bypasses the family contract. F5b records
that bounded construction result and relies on the existing generic issuer equal-value identity
adversary. A constructor-count guard plus the reviewed case table makes a future output constructor
require review before `FullyAttributed` can remain truthful; semantic helper branches remain
review-owned because a test-name or AST-body assertion would be tautological.

Coplanar subdivision of one logical wall into several face nodes is not a current output path: the
ring grammar requires one degree-two graph node per polygon side and refuses that topology rather
than merging it. A bounded fixture search also found no recognised ring with a multi-segment shared
wall junction, so F5b makes no representation claim for that topology. Adding either form of wall
merging is a future semantic recogniser change with its own attribution contract, not retrospective
evidence for the current family.

F5b changes no `src`, registry disposition, predicate, defining role, output, reconciliation,
capability manifest or golden. If the audit finds a production defect, it stops and opens a semantic
family child instead of fixing it under this neutral scope.

Across independent writer-off/writer-on calls, parity means record type, value, ordering and
`to_dict()`, never cross-run Python identity. Within one writer-enabled run each Candidate record is
the exact returned object occurrence; value rematching is forbidden and equal-valued occurrences
remain distinct.

The generic per-face report consumes only the completed frozen inventory/evidence and enumerates all
22 families: returned records, physical/final accepted Candidates, attributed occurrences, defining
face occurrences/distinct faces and complete/incomplete reason. Corpus taxonomy adapters remain
diagnostic comparison layers and cannot define attribution or rerun recognisers.

F5a and the retrospective tests/docs-only F5b audit consume no new holdout. Every semantic family
child freezes its exact ownership rule, development
evidence and untouched attribution holdout allocation before implementation, obtains two exact-head
accepts before one authorised reveal, and may not reuse consumed MFTRCAD buckets 10–19. A post-reveal
defining-role/status change invalidates that family result.

F5c/#196 applies that gate first to Flats. The planar truncation face is the sole defining node;
the matched external cylinder and optional same-stock antiparallel flat remain consulted. The
public wrapper is unchanged, all pending bindings validate before issuance, and registry promotion
lands only with the complete evidence matrix. `F5-FLATS-H1`, MFTRCAD v1 bucket 20, was technically
sealed by #197 before implementation and remained unrevealed until the two pre-reveal accepts.
The authorised one-shot reveal then consumed bucket 20 at exact PR #199 head `8796a86`: 23 complete
models/69 files, zero invalid models, and a non-vacuous exact 10 Flat proposals = 10 accepted = 10
attributed. The selected-artifact SHA-256 is
`a2e045e3d6eb2b1ecd454fcbd12c04aaf5a4fb1ad85519891d8bcc48cd86356b`. No predicate, defining role,
status rule, or output was tuned from the result; the allocation is regression evidence only.
Its 10 claimed Flat faces plus 10 nonempty attributed Candidates prove one defining face per
Candidate arithmetically, but the scanner did not reconstruct planar-owner/stock/opposition
geometry. Exact role correctness therefore remains development-matrix evidence; H1 proves only
non-vacuous attribution completeness, one-face cardinality, and output retention.

The F5c development matrix found no valid closed-manifold occurrence with more than one distinct
same-stock antiparallel Flat: a connected planar truncation is one face, while splitting it creates
separate axial stock spans or unsupported/ambiguous topology. The audit therefore proves order
invariance with a six-flat face-traversal reversal and treats open, duplicated, or ambiguous
ownership as a pre-publication refusal; it does not fabricate a second opposition by bypassing the
recogniser. If a future valid fixture contains multiple eligible oppositions, F5c stops and opens a
semantic prerequisite rather than choosing by traversal order under this attribution-only slice.
An open Shell carrying the real Flat faces is the executable family-level body-refusal case.
OCCT cannot construct a valid closed nonmanifold Solid with one unambiguous owning Flat face:
three-face edge incidence is either an open/invalid shell or ambiguous ownership, both rejected by
the same graph-owned `common_valid_solid` authority before publication. F5c records that bounded
kernel construction result instead of treating a vacuous no-output shape as a separate positive
proof; a deep-copied geometric clone and a translated-stale clone exercise the remaining identity
refusals directly. A shallow wrapper around the same OCCT topology may correctly resolve through
the graph's `IsSame` identity and is not described as invalid evidence.

F5d/#201 applies the same gate to Fillets after #200 seals `F5-FILLETS-H1`, MFTRCAD v1
bucket 21, without archive access. Each Fillet is defined solely by its original partial-cylinder
or torus blend face. That adaptor supplies analytic kind, principal axis, radius and the serialized
anchor evaluated at the midpoint of its trimmed parameter bounds; the anchor is not claimed to lie
inside an arbitrarily holed trim. Neighbour planes, same-solid external stock cylinders,
second-diameter bands, spherical continuation and convexity/material probes are consulted only.
The pre-existing whole-part bounding-box extent remains consulted by maximum-radius eligibility and
is not made body-local in this attribution slice.

F5d preserves the principal-axis domain: translations, mirrors, uniform scales and axis-preserving
rotations/permutations are positive representation checks, while an arbitrary non-principal
rotation remains negative. The matrix separately exercises every public keyword route and the
three alternative torus eligibility paths (transverse plane, two distinct external band diameters,
or spherical continuation). A fresh-adaptor role oracle reconstructs kind, axis, radius and
parameter-bound anchor without calling the production anchor helper, binds toroidal stock context
to the owner's `SolidRef`, and proves consulted faces absent from defining evidence. Unavailable real
open/nonmanifold/ambiguous or equal-valued family fixtures receive a recorded bounded construction
disposition plus the nearest non-vacuous graph/issuer adversary; ordinary no-output geometry and
fabricated sink input do not count as proof.

The pre-implementation route scan covered all 40 checked-in MFCAD++ development STEP models,
three Gramel STEP fixtures and constructed build123d turned rounds. No recognised turned Fillet
appeared in the imported development sets, and constructed valid rounds exposed only the
transverse-plane torus route. The two-distinct-band and spherical-continuation alternatives have no
isolated real fixture in that bounded corpus/construction surface, so F5d pins them with narrow
production-boundary mutations using real torus, owner and same-solid context faces. These
substitutions may not replace an available returned path or authorize a predicate/role change; the
first future real occurrence becomes a mandatory regression fixture.

Writer-off/on parity applies to valid closed-solid inputs whose owner/context can be issued by the
run graph. The public facade remains a geometry-only compatibility reader and is not changed to
construct a graph or reject an otherwise recognised open/ambiguous shape; the aggregate writer
fails closed before publication when owner or exact turned eligibility context lacks one common
valid `SolidRef`. As with Flats, OCCT offers no valid closed nonmanifold Solid with one unambiguous
owner and same-solid Fillet context: attempted three-face incidence becomes an open/invalid shell
or ambiguous ownership. The real open-Shell refusal plus late body/context failures are the bounded
family-level evidence; a vacuous nonmanifold no-output shape is not counted separately.

The same bounded construction audit found no valid distinct Fillet occurrences with an equal full
record value. Equal radii are common, but distinct faces have distinct parameter-bound anchors;
forcing equal `axis/radius/at/turned` requires coincident duplicate topology, which is invalid or
ambiguous under the common-solid issuer contract. F5d therefore pins equal-radius occurrence
identity in the family loop and relies on the generic issuer's equal-value identity adversary for
the unreachable full-record collision. Any future constructible collision becomes a mandatory
family regression.

The authorised one-shot F5d reveal then consumed bucket 21 at exact pre-reveal PR #203 head
`71be0b0`: 21 complete models/63 files, zero invalid models, and 6 Fillet proposals = 6 accepted =
6 attributed across four models. The selected-artifact SHA-256 is
`6323bd2af053ada35952e8e7af4172a7da14bc0ec04ec4b3ec5b7b1275206f5a`. The completed
generic report contains six claimed Fillet face occurrences, so nonempty-evidence arithmetic proves
one defining face per Candidate and no reconciliation loss. It does not reconstruct analytic owner,
trim bounds or turned context geometry; those exact role claims remain frozen development-matrix
evidence. No recogniser predicate, tolerance, defining role, registry rule, ordering or
reconciliation was changed from the result, and bucket 21 is permanently consumed.

F5e/#205 migrates Countersinks after #204 technically seals `F5-COUNTERSINKS-H1`, MFTRCAD v1
bucket 22, without archive access. Each standalone record is defined solely by its exact original
conical seat face; the matching coaxial bore cylinder and line/radius facts remain consulted
eligibility context. A fresh adaptor/rim oracle must reconstruct every serialized field from that
face, while the private writer core retains the geometry-only public facade and validates all
pending owners before publication. Parity applies to valid closed-solid issuable owners; open,
nonmanifold, or ambiguous geometry may remain publicly recognised while aggregate evidence refuses.
The matrix includes arbitrary compound rotations because Countersinks have no principal-axis gate,
and separately pins radius, axis-line, and fixed angular tolerances. Bucket 22 may be revealed once
only after full gates and two independent exact-head accepts; zero occurrences still consume it.

The private `_discover_countersinks` core carries each sorted `(CounterSink, original cone face)`
occurrence through final output ordering. With the registry's graph-bound writer it resolves and
validates every final owner before the first proposal, then issues only that cone as defining
evidence; the public facade calls the same geometry path without a writer. A fresh adaptor/rim
oracle reconstructs axis, location, both diameters, included angle and depth independently. The
matrix covers X/Y/Z and arbitrary compound orientations, translation/mirror/scale, standard and
inclusive maximum angles, multiple occurrences, STEP, geometry-only open-shell compatibility,
negative/no-leak shapes and late validation refusal. The raw cylinder adaptor reader roster moves
to the private core without changing its orientation-deferred disposition; `cone_rims` remains a
separate reader authority. `COUNTERSINKS` becomes `FullyAttributed` on that same reviewed head.

Development construction also corrected one inherited prose assumption: a side-clipped cone can
retain circular-arc boundary geometry and remains accepted. F5e pins that as a regression positive;
only non-circular or insufficient rim geometry is negative. Tightening trim/full-circle semantics is
a separate behavior-changing prerequisite, not an attribution fix.

The negative matrix separately constructs a cone trimmed between two oblique non-circular
boundaries and proves empty public/writer output, while an apex drill-point proves the insufficient
rim branch. A real turned workpiece with pilot bore and 60-degree end cone pins the documented
centre-drill false positive. As in F5c/F5d, OCCT does not provide a valid closed nonmanifold Solid
with one unambiguous owner cone: attempted extra edge incidence becomes open/invalid or ambiguous.
The real open Shell plus late graph/body refusal is the bounded family evidence; a vacuous
nonmanifold no-output shape is not counted as a distinct execution path.

The authorised one-shot F5e reveal at exact pre-reveal PR #207 head `8c0462a` selected 36 model
triples/108 files, then aborted when `20240116_231044_6899` imported as an invalid B-rep. It produced
no complete scanner report and therefore no Countersinks attribution or output-retention claim.
The failure did not authorize an alternate scanner mode, rerun, replacement, predicate change, or
role change. Bucket 22 is permanently consumed and the holdout result is explicitly inconclusive;
the exact ownership claim remains supported by the frozen development matrix and mechanical gates.

F5f/#209 migrates cylindrical Bosses only after #208 technically seals `F5-BOSSES-H1`, MFTRCAD v1
bucket 23, without archive access. The sealed allocation is excluded permanently from `unselected`,
requires its exact non-transferable acknowledgement, and begins `sealed_unrevealed`; the normalized
unselected complement is 24..999. The defining evidence contract is the complete identity set of
original external cylinder faces in the producing segment. End partners remain transient consulted
orientation context. The migration preserves current unsorted emission order and the geometry-only
public facade; no recognition, output, reconciliation, or capability-format change is part of the
neutral seal. Bucket 23 may be revealed once only after the semantic child passes full gates and two
independent exact-head accepts; a zero or aborted result still consumes it and remains inconclusive.

The F5f implementation seam is one private `_discover_bosses` core. It preserves the current
unsorted segment emission order, carries each `BossRecord` with an immutable original segment-face
snapshot, resolves/deduplicates nodes in graph-owned run order, and validates every complete set
against one `SolidRef` before the first Candidate is issued. The public facade remains writer-free.
End-classification partners are transient context and the shared Hole/Boss classifier and its raw
surface-reader roster stay in place. A mandatory coincident-valid-solids adversary proves two
equal-valued Boss records retain distinct record identities, Candidate occurrences, defining sets
and body ownership rather than being value-rematched.

The F5f construction audit found that fusing two touching equal-radius native cylinders is healed by
OCCT into one cylindrical face, with or without glue, so it cannot prove an axial original-face
split. The executable matrix instead uses a valid keyway-interrupted solid whose one segment has
two distinct original circumferential cylinder faces and proves both are defining. A supplied
real-face aggregation adversary exercises axial union-span grouping but is refused at the body gate
when its faces come from different solids; it is not misreported as a positive Candidate. A future
imported valid one-solid axial subdivision must add an exact all-patch regression. Native zero-span
cylinders and a valid closed nonmanifold Solid likewise could not be constructed: degenerate shapes
are rejected before Boss discovery, while open Shell and injected ambiguous/body failures exercise
the nearest non-vacuous aggregate refusal boundaries.

The authorised one-shot F5f reveal at exact pre-reveal PR #211 head `f3e7dd9` selected 32 model
triples/96 files, then aborted when `20240124_001736_209` imported as an invalid B-rep. It produced
no complete scanner report and therefore no Bosses attribution or output-retention claim. The
failure did not authorize an alternate scanner mode, rerun, replacement, predicate change, or role
change. Bucket 23 is permanently consumed and the holdout result is explicitly inconclusive; the
exact ownership claim remains supported by the frozen development matrix and mechanical gates.

F5g/#213 may begin only after #212 technically seals `F5-DOUBLE-D-BORES-H1`, MFTRCAD v1 bucket 24,
without archive access. The allocation starts `sealed_unrevealed`, is permanently excluded from
`unselected`, accepts only its exact non-transferable acknowledgement, and moves the normalized
unselected complement to 25..999. Its sole purpose is the later one-shot Double-D attribution
chronology; the neutral seal creates no recognition, evidence, result, or corpus-outcome claim.

The F5g implementation uses one private `profiled_bores._discover_double_d_bores` core. Each exact
low/high four-edge opening pair seeds four role-labelled chains of original planar chord-wall and
cylindrical arc-wall faces. Same-support inward continuations must pair one low seed to one high
seed, cover the serialized interval without overlap/gap, remain nonbranching and stay disjoint
across roles and occurrences. Every complete union is resolved in graph order and proved to have
one valid SolidRef before publication; only those lateral faces are defining. End planes, opening
wires, per-solid extrema and the empty-prism boolean remain consulted facts establishing eligibility,
throughness, high-end location and depth. The public facade remains writer-free and output-identical.

OCCT healing of native and ordinary STEP Double-D tools retains one full-span face per logical wall;
the current development fixture therefore cannot supply a positive middle axial patch. The
production chain grammar and interval checks admit consecutive disjoint/meeting axial patches, while
the imported ordinary fixture proves role correspondence. A future valid one-solid axial subdivision
must add an exact all-patch regression before it can be claimed as observed positive evidence.

The authorised F5g reveal ran exactly once at accepted pre-reveal PR #215 head `80def7f`, after two
exact-head ACCEPTs, green focused/static/package/CI/Codecov gates, a passing composite budget, and
crossed same-host census neutrality evidence. Selection `f5_double_d_bores_h1` contained no models;
the scanner raised `no models match selection 'f5_double_d_bores_h1'` before importing STEP,
reading annotations, running recognition or producing a complete JSON report. No retry,
replacement, alternate mode, fitting or treatment change followed. Bucket 24 is permanently
`consumed`, the result is inconclusive and not regression evidence, and exact Double-D ownership
remains supported by the frozen development matrix and mechanical gates.

F5 Wave 1 sealing/#216 predeclares three independent semantic allocations without archive access:
`F5-POLYGONAL-BOSSES-H1` bucket 25, `F5-PADS-H1` bucket 26, and `F5-HOLES-H1`
bucket 27. The immutable allocation roster and checked policy manifest retain one exact token,
acknowledgement and `sealed_unrevealed` state per allocation; the exact unselected complement moves
to 28..999. Authority is non-transferable between the three selections, and `all` remains closed.
This neutral tools/tests/docs slice supplies no membership, geometry, recognition, attribution or
outcome evidence. Each later semantic child owns its own two-review/mechanical gates, one-shot
authorization and independent consumed chronology; zero, abort, invalid or completed outcomes may
not be retried, replaced or fitted.

The Channels semantic child #225 is independently protected by `F5-CHANNELS-H1`, bucket 28. Its
neutral seal adds the exact `f5_channels_h1` selection and acknowledgement while leaving the archive,
membership and outcomes untouched; it moves the ordinary unselected complement to buckets 29..999.
At accepted pre-reveal PR #230 head `bdbe3cc`, after two exact-head accepts and all mechanical,
performance and CI gates, its one authorised selection contained 28 model triples (84 files).
Annotation validation stopped on repeated instance membership in
`20240125_003844_2492_result_rel.json` `relation[0]`, before that model's STEP import or
recognition. The overall audit produced no complete report, so no aggregate Channel counts or
attribution outcomes are available or claimable. Selected-artifact SHA-256:
`b9995ccd4acb273b2e1a2d81942bb848c838d3b65d8a51b75461bf4288b73319`. The temporary
selection was deleted and no retry, alternate mode, replacement or fitting followed. Bucket 28 is
permanently consumed and inconclusive, not regression evidence.

The Plates semantic child #228 is independently protected by `F5-PLATES-H1`, bucket 29. Its neutral
seal adds the exact `f5_plates_h1` selection and acknowledgement without archive access, membership
inspection, recognition, or outcome evidence; the ordinary unselected complement moved to buckets
30..999. At accepted pre-reveal PR #231 head `b1bdcf3`, after two exact-head accepts and all
mechanical, performance and CI gates, its one authorised selection contained 39 model triples (117
files). Annotation validation stopped on repeated instance membership in
`20240124_001736_5206_result_rel.json` `relation[0]`, before that model's STEP import or
recognition. The overall audit produced no complete report, so no aggregate Plate counts or
attribution outcomes are available or claimable. Selected-artifact SHA-256:
`e5b57ca085664bba044379d8f2aca8c7f7807f201f22a8bbdab507d26735fa89`. The temporary
selection was deleted and no retry, alternate mode, replacement or fitting followed. Bucket 29 is
permanently consumed and inconclusive, not regression evidence.

F5 Plate attribution/#228 preserves the whole-part public recogniser and its record/order semantics.
Each supported aggregate Plate owns every original planar face in its low-negative and high-positive
coordinate clusters. TURNED_STEPS remains a restricted record-only global veto and contributes no
Plate evidence. Whole-part grouping can mix provenance across compounds; that bounded unsupported
aggregate path raises `_PlateAttributionError` before issuance or family completion while the public
geometry-only facade remains unchanged. Aggregate ambiguity is decided only after graph binding:
identical low/high node-role pairs collapse independent of face wrappers and traversal order, and
more than one distinct role pair for a public deduplication key refuses before any issue. Exact
Plate ownership therefore remains established by the frozen development matrix rather than the
inconclusive consumed allocation.

The Polygonal Stock semantic child #232 is independently protected by
`F5-POLYGONAL-STOCK-H1`, bucket 30. Its neutral seal adds the exact
`f5_polygonal_stock_h1` selection and acknowledgement without archive access, membership
inspection, recognition, annotation reading, or outcome evidence; the ordinary unselected
complement moved to buckets 31..999. After #232 cleared two independent exact-head reviews and all
mechanical gates at accepted pre-reveal head `e884768`, its one authorized selection contained 37
model triples (111 files; selected-artifact SHA-256
`9b987707e0307f8dcdd9cce2daffa2113c482e1c1069479ba58b3b31ed7f725e`). Annotation
validation stopped because `20240116_231044_1243_result_rel.json` was not a JSON object, before that
model's STEP import or recognition. The overall audit produced no complete report, so no aggregate
Polygonal Stock counts or attribution outcomes are available or claimable. The temporary selection
was deleted without retry, alternate mode, replacement or fitting. The allocation is permanently
consumed and inconclusive, not regression evidence, and may not be reused.

The Slot and Pocket semantic children #235 and #236 share neutral occurrence/cap-provenance
prerequisite #234, but their outcome evidence remains independent. `F5-SLOTS-H1`/bucket 31 and
`F5-POCKETS-H1`/bucket 32 add the exact `f5_slots_h1` and `f5_pockets_h1` selections and
acknowledgements without archive access, membership inspection, recognition, annotation reading or
outcome evidence. The combined neutral designation moves the ordinary unselected complement to
buckets 33..999, but neither acknowledgement authorizes the sibling selection. Each allocation
remains `sealed_unrevealed` until its own semantic child clears two independent exact-head reviews
and all mechanical gates. Any authorized zero, abort, invalid or completed reveal consumes only
that allocation without replacement, retry or fitting; only a completed valid report can become
regression evidence, while zero/abort/invalid outcomes remain inconclusive.

At accepted pre-reveal PR #246 head `d4343ad`, after two independent exact-head accepts and all
current-head CI, coverage and focused gates, the single authorized
`f5_slots_h1`/`F5-SLOTS-H1` attempt found zero models. It stopped before annotation reading, STEP
import or recognition and produced no report, digest, counts or outcomes. Temporary output was
deleted and no retry, alternate root or allocation, replacement or fitting followed. Bucket 31 is
permanently consumed and inconclusive, not regression evidence.

At accepted pre-reveal PR #248 head `e20400c59b0381e03ca30eb9e6ab400689eea4dc`, after two independent exact-head accepts and every
quality, package, performance, CI and coverage gate, the sole authorized Pocket attempt created
temporary root `/tmp/mftrcad-pocket32.UdUMnL`. Archive extraction did not start because `unzip` was
not installed. The scanner was then invoked exactly once with `f5_pockets_h1` and acknowledgement
`F5-POCKETS-H1`; it rejected the empty root because `steps/` and `labels/` were absent. No archive
content, allocation membership, annotation, STEP, recognition, report, digest, count or outcome was
accessed or produced. The temporary root was deleted without retry, alternate extraction tool/root,
allocation, replacement or fitting. Bucket 32 is permanently consumed and inconclusive, not
regression evidence. Buckets 34 and 35 remain sealed and the complement remains 36..999.

F5 Slot attribution/#235 promotes only `SLOTS`. Each occurrence owns every exact planar wall
retained by intentional merge/collinear collapse plus every patch in the selected low/high
cylindrical cap groups. Rectangular, crossing, elongated-obround and cap-recovered routes therefore
share one occurrence-safe proposal product while keeping their distinct defining roles. The public
signature, values, order and ledger compatibility remain unchanged; the registry alone uses the
private writer core with shared `FaceEdges`. All occurrence binding and one-SolidRef checks complete
before issuance. Distinct exact records on that same solid may share a truthful wall node; identical
bound proposals collapse, while same-record competing roles and cross-solid reuse refuse atomically.
Bucket 31 remained sealed through independent reviews and gates; its later zero-population attempt
is the consumed, inconclusive lifecycle result recorded above.

The Repeating Radial Profile, Step Level and Riser semantic children #239, #240 and #241 have
independent neutral allocations despite being designated together: `F5-REPEATING-RADIAL-PROFILES-H1`
at bucket 33, `F5-STEP-LEVELS-H1` at bucket 34 and `F5-RISERS-H1` at bucket 35. Their exact
selection tokens and acknowledgements are non-transferable, including between the three siblings.
This designation changes only scanner policy, tests and documentation and moves the ordinary
unselected complement to buckets 36..999; no archive membership, annotation, geometry,
recognition result or outcome was inspected. Before an authorized attempt, each remains
`sealed_unrevealed` until its own
semantic child has two independent exact-head accepts and all mechanical gates. Any authorized
zero, abort, invalid or completed reveal consumes only that allocation without replacement, retry
or fitting.

Step Levels and Risers have now reached reviewed structural exclusions rather than semantic
promotions. Whole-part Step Level clustering can combine equal-Z source faces from different
`SolidRef`s into one value. Riser `sorted(set(...))` likewise collapses distinct source faces,
including equal records on different solids. Choosing one face is incomplete and traversal-based;
unioning cross-solid faces violates the one-body evidence invariant. Both registry paths therefore
remain writer-free, `IncompleteAttribution` and `NotCounted`, with their public values and aggregate
behaviour unchanged. Occurrence-preserving/body-scoped records or an explicit multi-source ownership
ADR are prerequisites to revisit them. Buckets 34 and 35 remain sealed and were not revealed.

At accepted pre-reveal PR #245 head `6ccbedc`, after two independent exact-head accepts, all
mechanical/package/performance gates, nine OS/Python CI jobs and Codecov patch/project success, the
single authorized `f5_repeating_radial_profiles_h1`/`F5-REPEATING-RADIAL-PROFILES-H1` attempt found
zero models. It stopped before annotation reading, STEP import or recognition and produced no
report, digest, counts or outcomes. Temporary output was deleted and no retry, alternate root or
allocation, replacement or fitting followed. Bucket 33 is permanently consumed and inconclusive,
not regression evidence; buckets 34 and 35 remain sealed.

F5 Repeating Radial Profile attribution/#239 uses one private writer-capable discovery core while
preserving the public writer-free values, order and schema. Each retained occurrence carries its
exact lower and upper extremal planar source faces through correspondence and sorting; the aggregate
binds both to one valid solid and publishes the exact two defining nodes only after every occurrence
has validated. Boundary curves, sampling, side regions and rejected alternatives remain consulted.
This is exhaustive prevalidation atomicity: once staging succeeds, publication uses the issuer's
validated no-fail proposal contract rather than inventing a family-local transaction or rollback.
The family becomes `FullyAttributed` but retains
`NotCounted("correspondence evidence is not a distinct feature")`; it remains neutral correspondence
evidence rather than a manufacturing classifier. Bucket 33 remained sealed until the independent
exact-head reviews and all frozen mechanical gates completed; its later zero-population attempt is
the consumed, inconclusive lifecycle result recorded above.

The frozen development matrix has real LINE-only bbox-centred and mixed LINE/CIRCLE
common-circle-centred occurrences, including minimum five and higher prime counts. A bounded
build123d/OCCT construction search did not produce a stable valid closed spline/freeform radial
profile whose imported opposed wires preserve exact sampled bijection. This migration therefore
makes no spline-specific acceptance claim and does not alter curve predicates to manufacture one;
curve-kind mismatch and sampled-shape refusal remain pinned synthetically until a genuine supported
fixture exists as a separately reviewed semantic prerequisite.

F5 Channel attribution/#225 preserves the public graph-only `ledger=` compatibility path while the
aggregate registry uses a private writer seam. Each issuable Channel owns exactly its original
geometrically ordered low/high inward side walls; floors, caps, envelope facts, interruptions and
alternative pairs remain consulted. One original wall may truthfully define distinct Channels on
its two sides when each occurrence retains a distinct ordered wall pair, exact record and one-body
proof; only competing wall pairs for the same serialized record are ambiguous. All per-solid value
ambiguity, graph identity and common-body checks complete before the first proposal is issued. Exact
Channel ownership therefore remains established by the frozen development matrix rather than the
inconclusive consumed allocation.

F5 Polygonal Stock attribution/#232 preserves the exact-prism public predicate and its `NotCounted`
census treatment while replacing the former structural exclusion. Each aggregate record owns its
complete original eight-face solid boundary: the six ordered outward side faces and the unique lower
and upper caps. Cap nodes are retained at selection time rather than inferred from the remaining
inventory or rounded coordinates. Exact graph/writer authority, eight-node inventory equality and a
single SolidRef are proved before issuance. Bucket 30 stayed sealed until the implementation gates;
its later input-validation abort permanently consumed it inconclusively without regression evidence.

F5 prerequisite/#219 closes the record-only physical-dependency gap before Holes attribution. Each
registry family now completes atomically immediately after discovery: returned occurrences bind to
one exact CandidateSet, deliberate empty Candidates are staged without prefix leakage, later writes
under that family close, and the same set feeds terminal inventory. The issuer exposes opaque,
read-time-validated occurrence handles only through an equally opaque input capability bound to the
consumer's exact declared predecessor roster. Handles carry exact record identity, original defining
nodes and recomputed common-SolidRef provenance, but no global inventory, EvidenceIndex, disposition,
acceptance or reconciliation state. This neutral seam changes no family output/status and consumes
no holdout; it permits the later COUNTERSINKS-to-HOLES adapter to prove nested occurrence/body identity
without cone re-recognition or double ownership.

F5 recess prerequisite/#234 replaces the value-keyed Slot/Pocket reduction authority with private
immutable occurrences. Exact planar source nodes and complete cylindrical endpoint patch clusters
survive merge, collinear collapse, obround extension/recovery and body scoping, while record-only
compatibility remains value/order identical. The compatibility ledger is projected from those
occurrences and publishes no new cap evidence. At #234 landing, SLOT and POCKET remained
`IncompleteAttribution`; Slot was later promoted by #235 and Pocket by #236. No allocation,
schema, census, reconciliation or public-output change belonged to #234.

F5 Pocket attribution/#236 promotes only `POCKETS`. Opposed-wall routes define the intentionally
merged wall union while their floor remains consulted; corner notches define their two walls and
footprint floor; elongated and recovered obrounds define every selected endpoint patch in addition
to any retained walls. Discovery validates every occurrence, identity and SolidRef before the first
issuance. Public records/order, Pocket reconciliation and derived patterns remain unchanged, and
the independently authorized bucket 32 remained sealed through all review and delivery gates; its
later extraction-precondition failure is the consumed, inconclusive lifecycle result above.
The retained corner compatibility route is deliberately world-frame-specific: it considers only a
world-Z floor with world-X/Y walls and derives `open_sign` on Z. Rotating a corner may therefore
produce the established world-Z reinterpretation rather than a covariant rotation of its old
record. General all-axis corner semantics require a separately reviewed F4b geometry change and
are not introduced by this attribution-only child.
Graph-bound source sharing is valid only between unequal records whose complete role sets resolve
to the same SolidRef. Equal-record identical sets collapse; competing assignments, cross-solid or
stale provenance still refuse atomically. Sources are never subtracted or merged to force uniqueness.
Likewise, a physically split corner floor remains the legacy no-output case: joining its patches
would expand the corner recogniser's semantic domain and is deferred rather than fitted in F5.
The executable invalid-topology boundary uses an open Shell. A bounded OCCT construction search did
not produce a valid closed non-manifold Solid with one unambiguous Pocket occurrence: added
incidence either remains open/invalid or makes the wall/floor assignment ambiguous, which the same
common-SolidRef and complete-role checks already refuse. This is not claimed as an extra supported
geometry route.

F5 cumulative exit ledger at the F5 closure point, before the later F4b allocation lifecycle:

- the closed registry contains exactly 22 non-legacy physical definitions: 20 are
  `FullyAttributed`; only Step Levels and Risers remain `IncompleteAttribution`, each with the
  reviewed occurrence/multi-body representation prerequisite from #240/#241 and merged #247;
- all 22 definitions have one explicit census disposition (15 `Counted`, seven `NotCounted`). The
  only physical predecessor dependencies are Holes on Countersinks and Plates on Turned Steps;
  derived Hole, Slot and Pocket patterns consume only their accepted declared source family;
- terminal validation and per-face tooling consume the one issuer-owned frozen inventory. Public
  compatibility ledgers project the same family cores; they are not a parallel aggregate claim
  authority and no recogniser is rerun to reconstruct attribution;
- named MFTRCAD buckets 20 through 33 are permanently consumed. Buckets 34 and 35 remain
  independently `sealed_unrevealed` as part of the accepted Step Level/Riser structural
  disposition, and the ordinary complement remains exactly 36 through 999. A consumed allocation
  supplies regression evidence only when its reveal completed validly; zero, aborted and invalid
  attempts remain inconclusive and are never retried or fitted;
- capability manifest format 1 remains unchanged. The private registry plus the reviewed human
  capability table carry attribution completeness, so no public schema or Draftwright pin moved.

The later F4b lifecycle recorded above consumes bucket 36 inconclusively, moves the current ordinary
complement to 37..999, and advances the capability manifest to format 2 for the separately reviewed
physical/projection/API role contract. That later state does not rewrite this historical F5 ledger.

Exit gate: every physical definition has an explicit attribution disposition; capability evidence
truthfully distinguishes attributed and unattributed families; per-face tools consume the same
frozen inventory and no parallel claim path remains. A family may remain incomplete only with a
reviewed structural exclusion or blocker, not a placeholder.

### F6 — Persistent cross-run feature correspondence

Add an optional sidecar that matches accepted records between recognition runs without changing
record equality or Candidate identity.

Required contract:

- F6a first adds an issuer-owned private optional snapshot of accepted occurrences from one
  completed immutable inventory product; it is neither discovery, reconciliation nor matching;
- F6a's first closed roster is Repeating Radial Profiles. A graph-authorized complete bounded-
  analytic body descriptor separates scalar intrinsic, translation-normalized world-oriented
  boundary and placement facts; complete defining-face and record summaries preserve multiplicity;
- descriptors are collision-prone compatibility evidence, never stable body identity. Equal and
  coincident bodies remain indistinguishable alternatives rather than receiving traversal IDs;
- F6b later consumes exactly two accepted snapshots and constructs the cross-run compatibility
  graph;
- matching distinguishes unchanged, moved, resized, split, merged, added and removed occurrences;
- ambiguity is explicit and never resolved by traversal index or nearest-neighbour guess alone;
- run-local Candidate IDs and kernel face indices never become public persistent IDs;
- public records remain plain values; persistence metadata is a separate versioned projection;
- equivalent unchanged geometry produces stable correspondence across STEP round-trips and platform
  traversal differences.

F6a is behavior-neutral: no recogniser/result/record/registry/census/manifest/allocation change and
no holdout. This package begins as a private diagnostic consumed by tests and tooling. A public
identity schema requires its own ADR and downstream consumer before publication.

Exit gate: edit-sequence fixtures pin identity through harmless re-export, translation and dimension
changes, while split/merge ambiguity fails closed; recognition results remain unchanged.

### F7 — Published substrate API

**Delivered scope correction (#262, #186):** the installed-wheel spike returned no-go on
publishing `GeometryGraph`. The supported surface is the graph-independent
`b123d_recognisers.inspection` roster used by five declared-feature workflows. Graph identity,
adjacency, blends, collapsed views, sections, correspondence, and evidence remain private or
experimental. ADR 0010 records why this narrower outcome supersedes the graph-shaped requirement
below.

The [architecture retrospective](0004-architecture-retrospective.md) and proposed ADR 0010 narrow
this sequencing further: before production exports, an installed-wheel Draftwright fitness spike
must identify two or three concrete consumer operations and the exact neutral symbols they need.
F7 publishes that bounded facade, not an inventory of private implementation. F6 snapshots, body
descriptors, rigid matching and partition matching remain an optional private upper layer unless a
separately reviewed consumer use case justifies publication.

Promote a consumer-proven facade over the neutral geometry substrate to a public, versioned
contract so that out-of-tree geometry consumers do not fork this repository. Adjudication remains
closed: an external consumer uses geometry queries and returns its own values; it
does not enter `build_recognition_result`, reconciliation, the census, or the capability
manifest.

Required contract:

- the published surface is one graph-independent inspection namespace, never the concrete private
  `FaceGraph` or provisional `GeometryGraph`; its exact surface/result and four family-reader
  roster is limited to operations exercised by installed-wheel Draftwright;
- the registry, disposition table, `FamilyId`, evidence sink/index and reconciliation remain
  private; no dynamic registration, filesystem discovery or plugin import path is introduced;
- the substrate API is versioned and manifest-declared under ADR 0005 discipline, with a
  documented compatibility window, and its exports are enumerated by a completeness test the
  same way recogniser exports are;
- determinism guarantees are stated per query (same part, same facts, any platform) and pinned
  by golden evidence, so external consumers inherit the contract internal families rely on;
- a documented graduation path states what an out-of-tree family must present to enter the
  closed registry: fixtures, semantic goldens, capability row, corpus evidence — the same bar
  `adding-a-recogniser.md` sets internally.

Sequencing: only after a real external consumer proves the smallest useful facade. Publishing even
at epic exit carries compatibility, documentation and assurance cost, so the broad private
substrate is not exported speculatively. A successful consumer spike may justify a narrow package
and graduation path; it does not by itself establish an ecosystem or remove the maintainer's
evidence-throughput ceiling.

Exit gate: a demonstration out-of-tree geometry consumer performs the named Draftwright operations
against only the published inspection namespace and documented contracts. A separate inspection
API manifest and versioned compatibility test cover that roster; the recogniser
capability manifest remains family-only and no internal adjudication symbol is reachable.

## Review and delivery process

Every child follows the evidence gate used for recent recogniser work:

1. State the exact neutral or family contract and adversaries before implementation.
2. Freeze development evidence and, when recognition semantics may change, a disjoint sealed
   holdout selected without inspecting outcomes.
3. Implement the smallest coherent slice with no unrelated family expansion.
4. Pass focused tests, the full suite, Ruff, mypy, diff-check, manifest regeneration, package-wheel
   contract and the composite performance/memory budget.
5. Obtain two independent reviews: one geometry/correctness review and one architecture/ADR review.
6. Resolve every blocking counterexample and re-review the exact final head.
7. Reveal a sealed holdout only after both accepts when a new predicate or recovery decision can
   change recognition. A neutral refactor proves unchanged existing holdouts and does not invent a
   reveal ceremony.
8. Record exact commit, commands, counts and benchmark convention in the child issue/PR.
9. Merge children in dependency order; do not stack an unreviewed semantic consumer on a neutral
   substrate PR.

MFTRCAD development and holdout partitions must be disjoint by published dataset identity or a
deterministic manifest rule. Only a completed valid reveal may become regression evidence. A zero,
aborted or invalid reveal is permanently consumed but remains inconclusive. Further fitting requires
a fresh draw.

## Architecture guards

The epic is not complete until tests make these properties executable:

- canonical recovery and graph views live below family policy and cannot import recognisers,
  reconciliation, registry execution or projection;
- discovery cores receive neutral context plus `EvidenceSink`, never an evidence reader;
- reconciliation receives no Part, canonicaliser, graph builder, collapsed-view builder or
  discoverer;
- collapsed/effective nodes always expand to same-run original `FaceNode` evidence;
- physical Candidate inventory and exactly-one-disposition coverage remain complete;
- registry definitions declare canonical, collapsed-view and local-frame dependencies explicitly;
- projection remains typed and manual; registry metadata cannot silently publish a schema;
- public result fields, capability records, census bindings and downstream goldens have independent
  completeness checks;
- no filesystem discovery, dynamic recogniser import, learned classifier or implicit plugin path is
  introduced;
- tolerance and canonical residual policies have named tests and no corpus-fitted constants.

## Global acceptance criteria

- [ ] Native analytic geometry returns byte-identical records and evidence to the baseline.
- [ ] Supported B-spline encodings return the same records as their native analytic equivalents.
- [ ] Unsupported/non-analytic B-splines fail closed with bounded private diagnostics.
- [ ] The AAG exposes reviewed smooth-sided semantics without changing family output by itself.
- [ ] Blend-collapsed views are immutable, provenance-complete and opt-in.
- [ ] At least one existing blend-obscured case is recovered in a separately reviewed consumer PR.
- [ ] Free-axis frames and sections are deterministic under rotation, mirror, traversal and STEP
      round-trip.
- [ ] No oblique feature is represented by misusing an axis-aligned public record.
- [ ] Every physical family declares truthful defining-evidence support or an explicit exclusion.
- [ ] Cross-run correspondence is a sidecar and does not alter records or Candidate identity.
- [ ] MFCAD++, MFTRCAD and real-part evidence is reported separately with provenance and limitations.
- [ ] Public API, capability schema, census and Draftwright contracts follow ADR 0005 transitions.
- [ ] The neutral substrate is published as a versioned public API with closed adjudication, a
      completeness test, and a demonstrated out-of-tree consumer.
- [ ] Full quality, package, cross-platform and performance gates pass at every semantic landing.
- [ ] All child issues close with exact-head logic and architecture accepts.

## Explicit non-goals

- adding through steps, threads, sheet-metal features, ribs or another recogniser family;
- training or embedding MFTReNet/BRepFormer or any learned model;
- treating a corpus label as geometric truth at an interaction;
- mutating or defeaturing the caller's Part;
- general free-form surface recognition beyond bounded recovery of analytic primitives;
- assembly mates, machining operations, tool selection, tolerances or drawing policy;
- public plugin discovery or third-party registry mutation;
- publishing residual diagnostics or persistent IDs before a real consumer and separate ADR.

## Principal risks

| Risk | Containment |
| --- | --- |
| Canonicalisation changes topology or face identity | analysis-only shape, explicit provenance, analytic-equivalence fixtures, fail closed |
| Torus recovery exceeds the documented OCCT canonical seam | torus is a separately gated F1 increment; the four documented primitives exit independently |
| Publishing the substrate freezes APIs still in motion | F7 runs strictly last; no public substrate export before F1–F4a settle |
| Richer arc kinds silently alter existing predicates | neutral-only F2 PR; explicit any-smooth compatibility helper |
| Collapse becomes hidden defeaturing policy | immutable opt-in view; no global automatic consumer |
| Oblique migration creates two competing truths | versioned section record, named precedence, ADR 0005 migration |
| Candidate search space harms performance | cheap applicability gates, derive-once context, per-package budget |
| Synthetic datasets encourage taxonomy fitting | geometry contracts first, real-part controls, sealed draws |
| Persistent matching invents certainty | explicit ambiguity, no traversal/face-index identity |
| Epic becomes another feature-expansion programme | non-goals enforced; each PR changes substrate or one existing consumer only |

## Recommended issue order

1. F0 baseline and MFTRCAD ingestion/audit.
2. F4a versioned frame/section schema with byte-identical principal-axis projection — first
   because it is the only package whose cost grows with every release that pins the axis-span
   schemas deeper, and because later fixtures and evidence should be written against the final
   schema once rather than twice.
3. F1 canonical analytic recovery design and neutral implementation (torus as a separately
   gated increment).
4. F2 smooth-sided AAG taxonomy.
5. F3 immutable collapsed views.
6. F5 defining-evidence migration, parallelised by independent family only after the neutral APIs
   settle.
7. F4b family-by-family oblique predicates, and the axis-span deprecation window.
8. F6 persistent correspondence after canonical frames and attribution are stable.
9. F7 published substrate API, strictly last: it freezes the neutral APIs the earlier packages
   are still shaping.

The first implementation goal should stop after F0 and the design review for F1. Canonicalisation
has the largest leverage, but it also sits beneath every recogniser; evidence and a reviewed seam
must precede code.
### F5h Polygonal Boss original-side attribution (#218)

The Polygonal Boss migration is independently sealed by `F5-POLYGONAL-BOSSES-H1` (bucket 25).
Its neutral implementation preserves the public Z-axis regular-hexagon recognition path and
promotes only `POLYGONAL_BOSSES`.  Every returned occurrence carries exactly its six original
vertical ring faces through stable sorting and binds them to one run-owned `SolidRef` before the
first Candidate is issued.  Support, terminal and transition caps are independently rederived
consulted facts for attachment and Z extent and are excluded from defining evidence.  Equal-valued
occurrences remain identity-distinct across valid solids; `POLYGONAL_STOCK` remains unchanged.

The one-shot reveal ran only after the frozen `ff3ce83` implementation had two independent accepts,
full/static/package checks, the composite performance gate and 12/12 CI/Codecov. It selected 22
complete models (66 files, 599 faces), found zero invalid models and returned zero physical,
accepted or attributed Polygonal Boss Candidates. Bucket 25 is permanently consumed; this completed
negative draw is regression evidence for zero output only and supplies no positive side-versus-cap
ownership evidence. Selected-artifact SHA-256:
`caf1f57ccc142697d10a8d74527ad08c4bdfa3e7a34dfab71518dda32f739eb4`; report SHA-256:
`43aa6950f0f8781972ad1ba8c1c3a36ec4d6c89c65e227826915d96b137e2a28`. No retry, replacement,
alternate selection or fitting occurred, and the temporary extracted selection/report were deleted.

### F5i rectangular Pad attribution (#217)

Bucket 26 (`F5-PADS-H1`) was independently sealed and remained unaccessed throughout development.
The implementation uses one private optional-writer core while preserving the public geometry path.
Each attributed Pad owns the exact accepted +Z top and one unique maximal-base original wall for
each ordered x0/x1/y0/y1 role. These five nodes must be pairwise distinct, belong to one SolidRef
and remain disjoint across occurrences; all proposals validate before first issuance. Stock bounds,
tier/ledge regions and other scanned faces are consulted only. The existing unsigned wall-normal,
three-decimal top rounding, absolute `_TOL`, area-fill tolerance, z0 highest-role rule, per-solid
value dedup and final ordering are unchanged. A later one-shot reveal requires two exact-head accepts
and every mechanical/performance gate. At accepted head `f8322bd`, 1,505 full tests passed with
96.22% coverage, the composite benchmark minimum was 2.388 seconds under the 2.698-second ceiling,
all 12 CI/Codecov checks passed, and two independent reviews accepted the exact head. The one-shot
selection then completed over 24 models (72 files, 572 faces, zero invalid), producing four
physical, four accepted and four attributed Pads with 20 claimed face occurrences. This is positive
regression evidence for occurrence retention, zero reconciliation loss and 20 total defining
occurrences (average five per Candidate), but it contains no per-Candidate cardinality vector and
the generic scanner did not independently prove which occurrence is the top or each ordered wall role.
Selected-artifact SHA-256: `20a25c12b9da60142c60526cf20fcd1e1435b7a2b0c2f9dae4d665e38561c611`;
report SHA-256: `3d63840c836ae9925f2903c8f338c8b8a2d17a4e44f7c7692f3e048741396e1f`.
Bucket 26 is permanently consumed; the temporary selection/report were deleted and no rerun after
corpus access, alternate selection or fitting occurred.

### F5j Hole attribution (#220)

Bucket 27 (`F5-HOLES-H1`) was kept independently sealed throughout development. At accepted
pre-reveal PR #227 head `2ddf17e`, after 1,568 full tests, static/package/performance/CI gates and
two independent exact-head accepts, its one authorised selection contained 35 model triples (105
files). Annotation validation stopped on repeated instance membership in
`20240124_001736_4786_result_rel.json` `relation[0]`. The failing model did not reach STEP import or
recognition; because the overall audit produced no complete report, no aggregate Hole counts or
attribution outcomes are available or claimable. The temporary selection/report were deleted and no retry, alternate mode,
replacement or fitting followed. Bucket 27 is permanently consumed and inconclusive, not
regression evidence; exact Hole ownership remains established by the frozen development matrix.
Hole discovery uses one private optional-writer core while preserving the public signature and
geometry-only path. Each issuable Hole owns all and only the original internal cylindrical patches
that establish its bore/deep span and selected near-side counterbore or spotface lands. End and
bottom faces, transition cones/tori, crossing faces, skipped grooves and through-hole far-side steps
remain consulted. A composed countersink is linked through the restricted completed-predecessor
capability introduced by #219; its cone stays defining solely for COUNTERSINKS. Every face,
common-solid and optional predecessor identity/body check completes before first Hole issuance.
Promotion to `FullyAttributed` and any one-shot bucket reveal require the same exact head to prove
the complete lifecycle, independent topology-first role oracle, static/package/full/performance/CI
gates and two independent accepts. Zero, invalid input, abort or completion consumes the allocation
without retry or fitting and without implying geometry the scanner does not reconstruct.

The frozen development evidence also pins the recognition-owned helper boundaries rather than
inferring them from final records: six-significant cylinder measurement and four-significant
grouping, projected line identity, scale-relative stack gaps, high-coordinate opening ties,
narrowest-bore/monotonic near-step selection, through-versus-blind depth ownership, and all closed
CounterSink association tolerances. Original-topology fixtures exercise plane, cone, inward/outward
torus, sphere and crossing-cylinder end partners; these are role and lifecycle evidence only, not
new recognition claims or holdout-derived tuning.
