# Epic 0003 — Recogniser framework consolidation

**Status:** complete (2026-08-23)
**Owner:** @pzfreo
**Opened:** 2026-08-22
**Baseline:** `ccf3b8c` (0.3.1.dev0, after #149) — 800 tests collected; Ruff and mypy clean

This epic pauses feature-family expansion and gives every aggregate recogniser one minimum
candidate/evidence lifecycle. It follows #127's one-run/one-inventory work and the design review
on [#162](https://github.com/pzfreo/b123d-recognisers/issues/162).

The framework standardises lifecycle, identity and evidence. It does **not** standardise family
geometry. A slot, passage, angled step and analytic curved profile may continue to use different
AAG/gAAG queries and completeness proofs.

| Issue | Deliverable | Behaviour change |
| --- | --- | --- |
| [#156](https://github.com/pzfreo/b123d-recognisers/issues/156) | identity-safe candidates and defining evidence | no |
| [#157](https://github.com/pzfreo/b123d-recognisers/issues/157) | write-only discovery evidence; frozen reconciliation index | no |
| [#158](https://github.com/pzfreo/b123d-recognisers/issues/158) | dispositions across existing conflict rules | internal diagnostics only |
| [#159](https://github.com/pzfreo/b123d-recognisers/issues/159) | explicit discovery, reconciliation, derived and projection phases | no |
| [#160](https://github.com/pzfreo/b123d-recognisers/issues/160) | ordered internal recogniser registry | no |
| [#161](https://github.com/pzfreo/b123d-recognisers/issues/161) | role-based evidence and one bounded residual consumer | internal diagnostics only |

## Baseline correction

The proposal discussed on #162 was originally written against an unpushed development worktree.
That state did not land with #149. At this baseline:

- `RecognitionRun` owns `FaceEdges`, `FaceGraph`, `ClaimLedger` and the cylinder inventory, but the
  append-only ledger sits inside the otherwise immutable run facts;
- public record objects are still the claimant keys, with `ClaimLedger` repairing value-equality
  ambiguity through identity lookup;
- `reconcile_recesses` returns four filtered record lists, has no dispositions, takes `Part`, and
  calls `recognise_passages` itself;
- chamfer/angled-step, prismatic-pocket/Pocket and step/groove rules also return filtered lists;
- `_take_inventory` interleaves discovery, reconciliation, derived patterns and result projection;
- census still reads the ledger to apply `steps_that_are_not_grooves` and is therefore not a pure
  projection of `RecognitionResult`.

Consequently #158 must **implement**, not extract, the first complete disposition protocol. #157
moves Passage discovery and introduces narrow evidence capabilities, but #159 owns the first
aggregate-wide ordering of all physical discovery before one evidence freeze. No task may cite
the unpushed prototype as evidence.

## Relationship to earlier work

- #127 supplied the migration base: one run context, shared immutable graph facts, identity-safe
  claim lookup and one aggregate inventory.
- #142 is closed by #149. The accepted rule is the baseline recess behaviour and is not to be
  redesigned here.
- #129 remains open, but its original typed-result, all-shared-edge and symmetric-cache requests
  are present on this baseline. Audit and close or rewrite that tracker separately; do not fold an
  arc-semantics change into this behaviour-neutral epic.
- Epic 0002 items 3 and 5 concern recognition scope and smooth/oblique geometry. This epic neither
  closes nor implements them. It only supplies a consistent lifecycle for existing families.
- No unmerged feature-expansion branch is a prerequisite. In particular, the curved step/slot
  families discussed in early #162 drafts are absent from this baseline. If any later lands, it
  must migrate like every other existing family rather than changing this epic's baseline.

## Minimum internal API

Names may be refined during implementation, but weakening these boundaries requires a new design
review.

```python
RecordT = TypeVar("RecordT")  # invariant


@dataclass(frozen=True, slots=True, eq=False, init=False)
class Candidate(Generic[RecordT]):  # constructed only by this module's sink
    family: FamilyId
    record: RecordT
    evidence: Evidence


@dataclass(frozen=True, slots=True)
class Evidence:
    defining: frozenset[FaceNode]


@dataclass(frozen=True, slots=True)
class CandidateSet(Generic[RecordT]):
    family: FamilyId
    candidates: tuple[Candidate[RecordT], ...]


class EvidenceSink(Protocol):
    def propose(
        self,
        family: FamilyId,
        record: RecordT,
        *,
        defining: Iterable[FaceNode],
    ) -> Candidate[RecordT]: ...

    def observe(
        self,
        family: FamilyId,
        predicate: PredicateId,
        *,
        subject: FaceNode,
        consulted: Iterable[FaceNode],
        fact: PredicateFact,
    ) -> Observation: ...


class CandidateIndex(Protocol):
    def by_family(self, family: FamilyId) -> tuple[Candidate[Any], ...]: ...


class EvidenceIndex(Protocol):
    def defining_of(self, candidate: Candidate[Any]) -> frozenset[FaceNode]: ...
    def claims_of(self, node: FaceNode) -> tuple[Candidate[Any], ...]: ...
    def observations(self, family: FamilyId, predicate: PredicateId) -> tuple[Observation, ...]: ...
```

`EvidenceSink.propose` is the atomic identity boundary: it validates graph provenance and creates
the candidate and its evidence together. `Candidate` construction is module-private, and the
sink/index retain a run-private issuance token so a manually forged or foreign-run candidate is
rejected. `CandidateSet` validates that its family and every candidate's family agree. Empty
defining evidence is represented deliberately but must never prove subset containment.

Candidate evidence records defining ownership only. A failed predicate has no Candidate, so #161
records its one demonstrated context-bearing case as a separate sink-issued `Observation` with an
issuer-snapshotted subject, consulted nodes and one closed primitive `PredicateFact`.
Observations are not proposals: they never enter CandidateSet, inventory completeness or
dispositions. Public records remain ordinary immutable value objects; candidate and observation
identity are run-local and never become persistent feature identity.

The disposition layer is private:

```python
Outcome = Literal["accepted", "rejected"]


@dataclass(frozen=True, slots=True, eq=False)
class Disposition:
    candidate: Candidate[Any]
    outcome: Outcome
    reason: ReasonCode
    related: tuple[Candidate[Any], ...] = ()


@dataclass(frozen=True, slots=True)
class ReconciliationResult:
    dispositions: tuple[Disposition, ...]

    @property
    def accepted(self) -> tuple[Candidate[Any], ...]:
        return tuple(d.candidate for d in self.dispositions if d.outcome == "accepted")
```

Reasons are closed, namespaced codes. Nothing in this epic exposes them publicly before #161 has
multiple proven consumers and a separately reviewed compatibility shape. Dispositions are the
single source of truth; accepted candidates are an identity- and source-order-preserving view,
never a second stored roster.

## Lifecycle

The aggregate lifecycle has five visible internal phases:

1. **Physical discovery** receives immutable neutral context plus a write-only sink. Every
   applicable physical family runs at most once and cannot inspect sibling proposals.
2. **Reconciliation** receives completed candidate sets and a frozen evidence index. Named
   reconcilers run in explicit source order and give every proposal exactly one final disposition.
3. **Residual diagnosis** joins frozen failed-predicate observations to accepted candidates by
   exact graph identity. It performs no geometry discovery and emits only private serialisable
   summaries. The bounded #161 consumer joins one subdivided-terminal AngledStep observation to
   the accepted planar Chamfer defining the same slant; it neither creates an AngledStep nor
   changes the Chamfer disposition.
4. **Derived projection** builds patterns from accepted members. Pattern functions are registry
   definitions for orchestration completeness, but their returned records are not aggregate
   proposals or candidates until a real conflict consumer requires candidate semantics.
5. **Projection** constructs the public `RecognitionResult` and the private inventory product.
   It performs no discovery and hides no compatibility policy.

Interdependent cascading rules stay inside one named reconciler. A coordinator rejects a second
disposition for the same candidate. Reconcilers cannot receive `Part`, build graph facts, mutate
evidence or invoke discoverers.

The inventory product consists of accepted result records, reconciliation trace and frozen
evidence index. Snapshots are result projections. Census and per-face attribution may also consume
the trace/index, but must not rerun discovery or apply unnamed policy. In particular,
step/groove compatibility moves from hidden census filtering to a named compatibility disposition
that accepts both records while identifying their relationship for distinct-feature counting.

## Recognition context and dependencies

`RecognitionContext` evolves from `RecognitionRun` and contains immutable facts only: the part or
solid scope needed by discoverers, `FaceEdges`, `FaceGraph`, analytic inventories and documented
tolerance facts. Evidence is not a context field. Every substrate the context owns is derived at
most once per aggregate run; migrating every family-private scan is separate measured work.

The registry introduced by #160 distinguishes three categories rather than pretending every call
is independent:

- **physical candidate definitions** have neutral discovery applicability and public-projection
  applicability that read context only;
- **dependent physical definitions** receive declared completed upstream values where current
  semantics require them, such as countersinks feeding hole discovery;
- **derived projection definitions** consume accepted records after reconciliation, such as
  patterns, and do not enter `CandidateIndex` or receive dispositions.

The registry is explicit and source-ordered. It drives internal orchestration and completeness
tests, not public exports or schema publication.

The implementation uses two discriminated definition types rather than one optional-field base:

- `PhysicalDefinition` carries a `FamilyId`, accepted record types, result field, public entry
  point, explicit upstream physical dependencies, context-only neutral applicability, aggregate
  discovery adapter and an explicit counted/not-counted census disposition.
- `DerivedDefinition` carries a closed derived id, accepted source families, result field, record
  types, public entry point, derive adapter and census disposition. It never issues candidates.

Dependency views expose only declared, already-completed values. Holes declare Countersinks;
Plates declare TurnedSteps while retaining separate prismatic context applicability. Hole, Slot
and Pocket patterns declare their corresponding accepted physical sources. Cylinder/graph facts
remain context substrates, while chamfer and fillet include flags remain explicit discovery
configuration rather than pretending the whole family is inapplicable.

## Standalone compatibility

Existing public `recognise_*` functions remain facades that construct a standalone context and
return that family's direct proposal records. They do not run cross-family reconciliation. Their
current parameters may be adapted privately, but removal or deprecation is outside this epic.

At no point may a geometry predicate be loosened or tightened merely to fit the framework. Any
recognition-semantic change needs a separate issue, adversarial evidence and explicit approval.

## Architecture guards

Tests must make these violations visible:

- a family imports or calls a sibling recogniser;
- a discoverer reads evidence or aliases a sibling discovery entry point;
- a reconciler imports discovery, receives `Part`, builds graph facts or mutates evidence;
- projection or census reruns discovery;
- residual diagnosis scans the graph, receives `Part`/a sink, or changes a disposition;
- registry order is inferred from filesystem/module discovery;
- a registered family lacks result, census, record-contract or capability coverage;
- a candidate receives zero or two final dispositions;
- a candidate or candidate set was not issued by this run's sink, or mixes family identifiers;
- equal-valued distinct candidates collapse by value equality;
- foreign-graph nodes enter evidence.

## Migration sequence

1. #156 introduces candidate/evidence identity behind one reference family with byte-identical
   public and aggregate outputs.
2. #157 introduces write-only sink and frozen-index capabilities and moves Passage discovery out
   of recess reconciliation. Aggregate orchestration may retain a private legacy-ledger adapter
   temporarily; #157 does not claim the global freeze while later physical discovery still exists.
3. #159 extracts all physical discovery before reconciliation, wraps every physical family output
   as sink-issued candidates, performs the sole global freeze, applies the existing filtered rules
   to produce accepted candidate sets, moves patterns to the derived phase, and makes the inventory
   product authoritative. Its transitional reconciled inventory has no complete disposition trace.
4. #158 replaces those filtered rules with complete dispositions for recess and the other existing
   named conflicts and gives non-conflicting candidates default accepted dispositions. At this
   step the universal one-disposition invariant applies to every physical aggregate proposal.
   #111's measured miss has no emitted candidate and therefore cannot truthfully receive a
   candidate disposition; its residual hypothesis remains explicitly owned by #161.
5. #160 introduces the registry only after discoverers share a stable internal call shape.
6. #161 records only the failed subdivided-triangular terminal predicate as a sink-issued
   observation, and joins that observation to an accepted same-slant Chamfer as one private
   unsupported residual diagnostic.
   It does not fix #111 recognition, scan generic residual graph faces or publish a diagnostic API.
   After implementation and two independent accepts, the frozen 33-model holdout was revealed
   once and pinned at zero diagnostics; no predicate changed after reveal.

Each step is independently reviewable and may retain compatibility adapters. Do not combine the
whole sequence into one framework rewrite.

## Gate for every PR

Before merge, every child issue must have:

1. the exact invariant and migration seam stated before implementation;
2. architecture, equal-value identity, ordering and adversarial phase-boundary tests;
3. an independent logic/clean-code review;
4. a separate architecture review against ADR 0003, 0004, 0007, 0008 and 0009;
5. both reviewers accepting the exact final revision after all counterexamples are addressed;
6. focused and full tests, coverage, Ruff, mypy, diff-check, manifest/package contracts,
   semantic goldens and downstream compatibility;
7. the recorded composite benchmark remaining inside budget;
8. development-corpus results used only as blast-radius evidence;
9. existing revealed holdouts remain unchanged where relevant. A new sealed holdout is revealed
   only when a child changes recognition semantics or adds a genuinely new predicate, and only
   after implementation and two accepts;
10. ADR, capability, epic and tracker prose matching the merged implementation;
11. all required checks green.

## Global acceptance criteria

- [x] No recogniser family or supported geometry is added during the epic.
- [x] Public standalone outputs, aggregate fields, schemas and semantic goldens remain stable
      unless a separately reviewed correctness change authorises movement.
- [x] Every physical aggregate proposal has run-local identity-safe evidence and exactly one
      disposition; derived projection records are explicitly outside that set.
- [x] Discovery cannot read sibling evidence through its provided API.
- [x] Reconcilers consume only completed candidates/frozen evidence and never run discovery.
- [x] Patterns are explicitly post-reconciliation projections.
- [x] Result, trace and evidence index form one authoritative inventory product.
- [x] Context-owned substrates are derived at most once per aggregate run.
- [x] The registry distinguishes neutral applicability, discovery dependencies and derived
      families and makes incomplete integration fail visibly.
- [x] At least recess, chamfer/angled-step, prismatic-pocket/Pocket and step/groove rules use the
      common disposition protocol; any conflict family added after this baseline must join it too.
- [x] Accepted, rejected and compatibility dispositions have real internal consumers; #161 adds
      a bounded residual diagnostic for missing-candidate evidence without a public commitment.
- [x] Full quality, package, current delivery-ownership and performance gates pass.
- [x] ADR 0003/0004 and capability documentation describe the final architecture.
- [x] #156–#161 close with evidence and #162 can close.

## Completion evidence

The bounded stack merged in the reviewed migration order:

| Issue | PR | Merge commit |
| --- | --- | --- |
| framework proposal | [#165](https://github.com/pzfreo/b123d-recognisers/pull/165) | `4570a4d` |
| #156 | [#166](https://github.com/pzfreo/b123d-recognisers/pull/166) | `933177b` |
| #157 | [#167](https://github.com/pzfreo/b123d-recognisers/pull/167) | `22b445d` |
| #159 | [#168](https://github.com/pzfreo/b123d-recognisers/pull/168) | `d185aee` |
| #158 | [#169](https://github.com/pzfreo/b123d-recognisers/pull/169) | `7f92791` |
| #160 | [#170](https://github.com/pzfreo/b123d-recognisers/pull/170) | `b58a8cb` |
| #161 | [#171](https://github.com/pzfreo/b123d-recognisers/pull/171) | `1c3b2d8` |

Every implementation PR received independent exact-revision logic and architecture ACCEPTs after
its concrete review findings were resolved. The final cumulative revision passed Ruff, mypy, both
Codecov gates and the full test suite on Python 3.10, 3.12 and 3.14 across Linux, macOS and Windows.
The package contract passed before a candidate wheel was built and installed; manifest, semantic
golden and corpus contracts remained in the full suite. The composite five-sample minimum was
2.646 seconds against the recorded 2.698-second ceiling. The #161 predicate was revealed only
after two accepts and pinned at zero diagnostics over all 33 holdout models, with no post-reveal
predicate change.

The obsolete cross-repository development-wheel Draftwright canary was removed separately by
reviewed [#164](https://github.com/pzfreo/b123d-recognisers/pull/164). The accepted delivery
protocol is stable-package-first: this repository proves its package contract before publication;
Draftwright remains on its previous exact artifact until its own PR pins and validates a new stable
artifact. This epic published no release and moved no downstream production pin.

## Non-goals

- new feature-family recognition or corpus-driven threshold fitting;
- ML classification, generic subgraph isomorphism or a universal constraint solver;
- one universal geometric recogniser abstraction or public base record;
- public run-local candidate identity;
- public diagnostics before their compatibility semantics are reviewed;
- speculative per-family substrate declarations;
- unrelated release, packaging or CI-workflow work.
