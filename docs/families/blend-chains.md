# Publish complete blend chains separately from dimension-worthy Fillets

- **Formerly:** ADR 0013, moved here 2026-09-15 as a family record. The number stays citeable;
  [its stub](../adr/0013-public-blend-chain-recognition.md) points here.
- **Modules:** `blends`

- **Status:** Accepted
- **Date:** 2026-09-01
- **Decider:** Paul Fremantle
- **Issue:** #414

## Context

Public `Fillet` deliberately means a dimension-worthy external edge treatment. It excludes small
edge breaks, internal rounds and non-principal cylinders, and returns one record per qualifying
curved face. Those are legitimate product semantics, but they cannot also describe the broader
rolling-ball geometry downstream consumers need to locate and group.

The existing private `BlendCollapseIndex` already establishes a stronger neutral occurrence. It
groups native cylindrical patches only when exact original topology proves one connected,
nonbranching, same-solid chain with one analytic cylinder, one convex or concave material side,
two unambiguous support regions and complete spring/internal/terminal boundary provenance. Its
complete MFCAD++-2,500 label-blind audit found five newly reached untouched Round faces, all on
convex chains. The one concave pure-Round chain was already constituent evidence of a Pocket, while
1,827 other concave chains carried no Round label. Labels do not decide geometry, but this result
shows no independent downstream ownership benefit for public concave occurrences. MFInstSeg remains
sealed pseudo-blind transfer evidence and did not shape this decision.

## Decision

Add public `recognise_blends(part) -> list[Blend]` and aggregate
`RecognitionResult.blends`. The initial public family includes complete **convex** cylindrical
chains only; concave chains remain available inside the private `BlendCollapseIndex` until a
concrete consumer can define their ownership relative to pockets, slots and steps. One complete
public chain is one occurrence, regardless of how many original patches subdivide it. `Blend`
serializes:

- the analytic radius;
- the proved `side`, fixed to `"convex"` in schema version 1;
- the dominant local `axis` plus its canonical full `axis_direction`; and
- a leader point obtained by projecting the chain's aggregate area centroid to its common
  analytic cylinder.

Every original cylindrical patch is defining and constituent evidence. Public face identity is
projected through the existing run-local `RecognitionEvidence` API; records do not serialize face
indices, graph handles or kernel objects.

`Blend` is geometry, while `Fillet` is a more specific dimensioning-ready interpretation.
Discovery remains independent. During aggregate reconciliation, accepted Fillet occurrences
supersede a Blend only when their exact defining-face union covers the complete chain. Small,
oblique or otherwise unmatched convex chains remain accepted. Current CircularBlindStep and
annular Boss/Hole geometry do not satisfy the complete-chain predicate, so no speculative
precedence rule is added for them.

The recogniser adds no radius threshold, maximum-size policy or new numerical tolerance. It
inherits only the split-invariant analytic-equivalence and smooth-side tolerances already fixed by
ADR 0008. Native oriented cylinders are the supported first contract; recovered/unoriented
cylinders, tori, spheres, surfaces of revolution, vertex blends and refused components remain
explicitly outside it.

## Architectural conformance

- ADR 0002: the public facade and private writer-enabled core share records, order and
  serialization; records are frozen and serializable.
- ADR 0003: Blend, Fillet and other families discover independently; exact terminal evidence is
  the sole precedence authority.
- ADR 0004: `BlendCollapseIndex` remains immutable and complete provenance is expanded to original
  graph nodes before Candidate issuance.
- ADR 0007: `blends._discover_blends` is the only writer-enabled family core and the registry is
  its sole production writer caller.
- ADR 0008: no corpus-derived or record-rounded admission threshold is introduced.
- ADR 0009: the neutral index retains its closed total/refusal contract; publication and
  reconciliation decisions live in the Blend consumer and aggregate policy respectively.
- ADR 0011: raw coordinates keep caller-space meaning; framed results express axis, direction and
  anchor in the exact paired local working shape without changing the frame representative.

## Required evidence

- convex, small-radius and non-principal authored positives plus public concave exclusion;
- sharp/full-cylinder negatives, index refusal and tolerance-boundary coverage;
- rigid translation/rotation, scale, traversal-order and compound ownership controls;
- exact defining-face evidence and one-occurrence-per-chain behavior;
- aggregate coexistence/precedence with Fillet, CircularBlindStep and annular Boss/Hole geometry;
- an open MFCAD++ before/after report separating Blend/Fillet records, defining recall and face
  coverage; and
- one bounded independent contract review, followed by final-diff ADR conformance.

After reviewed implementation merges to main, the independent Analysis Situs comparison may be
rerun against that exact commit and pinned settings. MFInstSeg is evaluated later only as an
aggregate pseudo-blind transfer milestone; individual transfer models do not feed development.

## Consequences

Consumers can locate complete external rounds and their exact faces without weakening Fillet
semantics or opening the private graph API. Existing Fillet records stay source/value compatible.
The aggregate and census gain an additive `blend` family, and the capability manifest advances
with `Blend` schema version 1. Concave chains remain private evidence rather than duplicate
top-level occurrences. The initial family is intentionally cylindrical rather than a claim of
complete vertex- or turned-blend coverage.

## Amendment — proved concave chains (issue #440)

Public `Blend` includes complete concave chains satisfying the same cylindrical rolling-ball
contract as convex chains. `side` now admits both values already established by the neutral index:
`"convex"` for an external round and `"concave"` for an internal round. This changes the admitted
values and documented meaning of an existing field. Under ADR 0005 that is `Blend` schema version
2 even though the field names and serialized types are unchanged. It requires a future minor
package release and explicit consumer acceptance; this epic does not publish beyond v0.4.12.

The earlier decision treated overlap with Pocket constituent evidence as a reason to keep concave
chains private. That conflated two compatible descriptions. A Pocket describes a manufacturing
recess and may include its rounded interior transition as constituent evidence. A Blend describes
the rolling-ball geometry itself and owns the curved patches that establish it. Constituent overlap
does not create a defining-claim conflict under ADR 0003, so both occurrences remain accepted.
`Fillet` precedence remains limited to its existing external convex interpretation.

The neutral discovery predicate does not change. Public concave projection adds one intrinsic
edge-blend condition: when both complete supports prove parallel planes, refuse the chain because
those surfaces have no intersection edge to round. This distinguishes a semicircular obround-slot
end from an internal corner treatment without consulting feature labels. `BlendCollapseIndex`
still requires one same-solid, nonbranching, same-cylinder/radius chain, a consistently proved
material side, exactly two unambiguous support regions and complete spring, internal and terminal
boundaries. Ordinary sharp concave transitions, parallel-wall round ends, incomplete or ambiguous
chains, mixed radii/sides, toroidal and other unsupported surfaces remain outside the public result.

The change is motivated by downstream access to internal-round radius and face identity and by the
aggregate transfer gap recorded on issue #440. Individual MFInstSeg geometry remains uninspected;
MFCAD++ labels measure the result but do not decide whether proved rolling-ball geometry exists.

## Amendment — explicit straight and circular rolling paths (issue #442)

`Blend` schema version 3 replaces the cylindrical surface-axis fields with one discriminated path
record before Draftwright or another known consumer adopts the family. A rolling-ball occurrence
now contains `radius`, proved `side`, and exactly one of:

- `StraightBlendPath(at, direction)`, where `at` is the subdivision-invariant area centre projected
  to the common cylinder axis and `direction` is canonical; or
- `CircularBlendPath(center, normal, radius)`, describing the complete centre-line circle of a
  native torus by its centre, canonical plane normal, and major radius.

The nested record type is the discriminator. No nullable collection of flat fields can represent
an invalid mixture of line and circle parameters. `Blend.radius` always means rolling-ball radius,
which is the cylinder radius for a straight path and torus minor radius for a circular path.
`CircularBlendPath.radius` is always the path's major radius. The old dominant `axis`, surface
leader `at`, and `axis_direction` fields are removed rather than assigned a second meaning for
tori. A straight path intentionally remains an unbounded analytic line plus an occurrence anchor,
matching the information content of schema 2; a circular path is initially a complete closed
circle. Partial circular paths require a later explicit bounded-arc schema and are not inferred.

The first circular-path recogniser accepts one complete, same-solid component of native toroidal
patches only when all patches share one torus, their original boundaries are fully accounted for,
and they meet exactly two unambiguous support regions through tangent springs. The initial supported
support geometry is the ordinary turned edge: one plane transverse to, and one cylinder coaxial
with, the torus axis. Material side is derived from the oriented native torus minor-radius normal
and must be consistent across patches and spring evidence. A full torus has no supports; a rolled
bead has the wrong support topology; incomplete, partial-path, mixed-torus, ambiguous-side and
cross-solid components refuse.

`Fillet(turned=True)` remains the dimension-worthy external interpretation. Discovery stays
independent, and aggregate reconciliation may supersede a circular-path Blend only when accepted
Fillet defining faces cover its complete toroidal component. Internal circular-path Blends remain
available because the Fillet contract intentionally excludes them.

This is a deliberate pre-adoption schema correction, not compatibility padding. Schema versions 1
and 2 remain historical facts, but no release after v0.4.12 is made by this epic. The implementation
requires several checked-in authored goldens: external and internal circular-path occurrences plus
multi-body ownership, as well as full-torus, bead, turned-Fillet coexistence, incomplete-support,
non-tangent, transform, mirror, scale, STEP and traversal-order controls. Sparse MFCAD++ evidence
measures the result but does not define it; MFInstSeg remains aggregate-only pseudo-blind evidence.

The circular consumer reads the graph's exact legacy `"smooth"` arc only as the native tangency
certificate for torus-to-torus grouping and torus-to-support springs. It does not interpret the
legacy convex/concave arc value: torus/cylinder springs cannot provide that fact. Side instead comes
from the oriented native torus differential relative to its analytic minor-radius direction, sampled
at multiple interior parameters and accepted only when every patch agrees. Architecture tests roster
all four exact-smooth reads so no truthiness or negative inference can enter unnoticed.
