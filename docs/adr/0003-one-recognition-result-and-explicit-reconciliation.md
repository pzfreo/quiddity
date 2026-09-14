# ADR 0003 — One recognition result and explicit reconciliation

- **Status:** Accepted
- **Date:** 2026-08-15
- **Decider:** Paul Fremantle

## Context

Independent recognisers can describe overlapping physical regions: a groove floor may resemble a
turned step, a pocket floor a global level, and a pattern both a group and several member
features. First-match ownership scattered among consumers produces different answers from the same
solid. A plain empty result also cannot distinguish absence, ambiguity, rejection and unsupported
topology.

Draftwright ADR 0017 identified the problem, but mixed recognition decisions with downstream
requirement and annotation identity. This project owns only the geometry-side portion.

## Decision

One call to `recognise(part)` produces one immutable `RecognitionResult` containing:

- reusable geometry inventories;
- proposed candidates and their evidence claims;
- accepted feature records and physical measurables;
- explicit rejected, ambiguous and unsupported outcomes;
- deterministic feature, region and measurable identities.

Candidate discovery and reconciliation are separate stages. A recogniser proposes a plausible
interpretation; a named reconciler accepts, combines or rejects conflicting claims and records the
reason. No universal constraint solver is required: family-specific rules may migrate behind the
one reconciliation protocol incrementally.

**Claims are written during discovery and read only afterwards** (#92). A recogniser records the
faces a candidate was established by, into a run-local append-only claim ledger; nothing consults
those claims while recognisers run. The ledger is deliberately *not* the face graph: the graph
holds geometric fact, a claim is an interpretation of that fact, and separating them keeps the
graph immutable and reusable while each run gets its own ledger. A claim also names a **role** —
`defining` today — because a feature does not relate to every face it touched the same way, and
treating consultation as consumption would manufacture conflicts. This is what makes "every
claimed region is traceable" achievable at all —
without it, whether two records describe one feature can only be answered by comparing coordinates
each family derived by its own procedure, which is how passage/slot reconciliation was first
written and is the defect that prompted this amendment.

Writing claims during discovery does **not** merge the two stages, because the separation this ADR
protects is against *order dependence*: a recogniser that declined a face because another family
had already claimed it would make the census depend on which family ran first. That remains
forbidden. Recording what a recogniser used cannot change what any recogniser returns.

Overlapping claims are evidence, not a verdict. The reconciler applies an explicit compatibility
or precedence rule; two candidates sharing a defining face may both survive, as a pattern and its
members do.

This is close to what the prior art does. Analysis Situs marks feature candidate faces on its
attributed adjacency graph and writes recognition results back as further attributes, so
recognition accretes rather than returning in one shot; see
[`docs/prior-art-feature-recognition.md`](../prior-art-feature-recognition.md). It corroborates
the decision rather than settling it — the local evidence is what settles it, and the sidecar
ledger is a deliberate divergence taken to keep the graph immutable. An earlier reading of this
ADR treated it as forbidding claims outright, which it never did.

Identity is derived from geometry under documented tolerance. It must not use Python identity,
kernel traversal order, display labels, page coordinates or a bare solid enumeration index.

**That rule governs persistent semantic identity — what a record is, and what a consumer may
store, serialise or compare across runs.** It does not govern a *run-local handle*, which exists
only to let two stages of one run refer to the same face, is never serialised, and stops meaning
anything the moment the part changes. `FaceNode` is such a handle: it carries a kernel traversal
position and is compared by Python identity, both of which are forbidden to a record and both of
which are correct here, because identity by construction is exactly what makes a foreign node
impossible to mistake for a local one. The two must not be conflated in either direction — a
handle must never leak into a record, and a record's identity must never be derived from one.

Consumer lifecycle caches are outside the result. A consumer may cache a result, but cannot make
its cache semantics part of this package's aggregate value.

## Required evidence

- One aggregate run performs each expensive substrate analysis once.
- Equivalent re-imports produce identical serialized results and identities.
- Conflicting groove/step, pocket/level and pattern/member fixtures have explicit outcomes.
- An ambiguous or unsupported fixture cannot return clean absence.
- Every accepted candidate has one reconciliation outcome and every claimed region is traceable.

## Consequences

Consumers receive one explainable physical feature universe. Migration can be feature-family by
feature-family, but temporary partial results must say which families were not evaluated.

For F4b Passage migration, `RecognitionResult.section_passages` is the physical Candidate field.
Reconciliation decisions attach to those rich identities. The legacy `passages` tuple is derived
only from accepted rich occurrences and never feeds reconciliation, census, or evidence backward.

## Amendment (0.2.6, epic 0002)

**A reconciler corrects double-counting. It does not correct recall, and the failure it leaves
behind is quieter than the one it fixes.**

This record gives a reconciler three verbs -- accept, combine, reject -- and its consequences do
not record what happens when the families being reconciled disagree by *omission* rather than by
overlap. Measured per face over 2,000 MFCAD++ models: the rule that drops a chamfer whose face an
angled step claimed leaves 28 chamfer records still landing on faces the corpus labels
*Triangular blind step* -- the exact faces that rule exists to remove. It did not remove them
because `recognise_angled_steps` never claimed them: its blind-end test requires a neighbour of
exactly three edges, and a neighbouring feature subdividing that triangle makes it four or five.

So when family A misses a feature family B also proposes, reconciliation converts **A's false
negative into B's false positive**. The census then reports one record for the face, under the
wrong family, and every count in it looks correct. That is harder to notice than a double count,
and it is the mirror of the failure the shared-predicate arrangement had before a rule replaced
it, where two call sites disagreeing could make a feature vanish claimed by neither.

Two consequences follow, and neither changes the decision above.

- **A rule's ceiling is the recall of the weakest family it reconciles.** Precedence between two
  families is worth having exactly as far as both can see the feature. Evidence offered for a
  reconciliation rule should therefore include what the *losing* family recognises, not only that
  the winner's records survive.
- **There is no fourth verb.** A reconciler cannot say "this face is contested and I am not
  deciding", and nothing in the result can carry that. Whether one is needed is not settled here:
  it is the residual-evidence half of ADR 0004, which is decided in principle and not yet built,
  and this is the first argument for it that comes from measurement rather than from
  architecture.

## Amendment (0.2.6, issue #127)

**There is one inventory. Any other view of a part is a projection of it, never a second
orchestration that is expected to agree.**

This record says one call produces one result, and says nothing about what a *second* entry point
into the same recognition may do. `feature_census` was such an entry point: it called the same
recognisers in its own order, with its own choices about what to inject into each, and reported
counts. Nothing forced the two to stay aligned, and they did not — measured over 73 parts, a real
turned screw was a plate to one and not to the other, because the two had written the same gate
differently. Each difference between them (a memo injected here and not there, countersinks fed
to the hole recogniser on one side only) was a divergence nobody had decided.

Two rules follow.

- **A view counts or filters the one inventory; it does not re-run recognition.** The census is
  now a projection of what `build_recognition_result` returns, so the two cannot answer
  differently about a part. Where a view *deliberately* differs, the difference is a named
  reconciliation rule applied to the shared inventory rather than a different sequence of calls —
  `steps_that_are_not_grooves` is the only such rule today, and it is a compatibility rule under
  the decision above: both records survive, only the count is corrected.
- **The state a run shares is owned as one object.** The graph, the claim ledger, the face-edge
  memo and the cylinder scan are facts about a run over a part. Derived individually they are
  also individually forgettable, and a recogniser with no parameter to receive one derives its
  own — silently, correctly, and twice. One object owns them, and orchestration is what holds
  it.

  **Ownership, not yet the call interface.** A recogniser is still handed `face_edges=`,
  `ledger=`, `cyls=` and `graph=` separately, because a public standalone recogniser has to keep
  a signature a caller with only a part can use. So this decides where that state comes from and
  how many times it is derived; it does not decide how a recogniser receives it. That second half
  is a migration of recogniser internals, and calling it done here would be the same kind of
  claim this amendment exists to correct.

Neither is a new architecture. Both are what "one aggregate run performs each expensive substrate
analysis once", in the evidence list above, has to mean if it is to be checkable rather than
aspirational — and it is now checked by counting derivations rather than by inspection.

## Amendment (0.2.6, issues #112 and #119)

**Paired faces describe one recess only when the AAG says that they bound one void; overlap alone
does not establish ownership.** Opposed planar walls can be drawn from different arms of an
interrupted polygonal boundary and still satisfy every metric test used by a slot or rectangular
pocket recogniser. The shared-neighbour arcs provide the missing local fact: for a valid pair,
where both walls meet a shared boundary neighbour, they must do so with the same convexity. A
fragmented boundary may provide no shared neighbour; then the walls must be connected through
smooth AAG arcs, the same query a gAAG answers by merging those face fragments. This is discovery evidence,
not a claim from another recogniser, so consuming it does not violate the order-independence rule.

Aggregate reconciliation then compares complete boundary claims. A four-wall ring yields to the
paired record that dimensions the same rectangular void; a non-rectangular ring defeats paired
fragments contained within it; mere partial overlap leaves both records intact. On the 40-model
MFCAD++ design corpus this changes 35 proposed slots to 19 accepted slots, removes every 0.08,
0.19 and 0.31 mm grazing-wall artifact without a size threshold, and reduces cross-family recess
overlap from 32 record pairs to two compatible pairs sharing a single face each (Pocket/Slot and
Pocket/Passage). Both are deliberately retained because neither claim contains the other.

## Amendment (0.3.1, issue #142)

The AAG condition above is necessary, not sufficient. Two independent recesses can contribute an
outer wall pair whose shared stock faces have matching arcs, and a narrow H- or U-shaped connector
can make their boundary graph connected while most of the proposed rectangle remains solid. For a
paired-wall `Slot`, `Pocket`, or `Channel`, the remaining unrounded axis-aligned prism must therefore
be materially empty after any curved end interruption has been proved from opposite-turn AAG arcs
and trimmed. Public record rounding is applied only after that admission decision.

Candidate existence has no material-volume allowance. The Boolean probe uses only the numerical
coordinate floor documented by ADR 0008; the separate 1% policy for recombining already-recognised
collinear slot arms is not reused. A complete outer boundary and an arbitrary internal island are
not enough for a simple rectangular record, because that record cannot represent the island. This
keeps AAG/gAAG in its proper role—coherent topology and normalized boundary evidence—without asking
connectivity to prove geometry it cannot prove. Emptiness is evaluated within the source solid:
material belonging to that same solid, including a connected island or bridge, is not representable
by the simple record; disconnected compound members are recognised independently and do not
suppress it.

## Amendment (framework consolidation, issue #157)

Discovery and reconciliation now receive structurally different evidence capabilities on the
first migrated paths. `EvidenceSink` can append a candidate and its defining evidence atomically
but cannot look anything up. `EvidenceIndex` can look up only a copied, immutable issuance prefix
and has no append route. Passage discovery therefore completes before recess reconciliation;
the rule receives completed records and the point-in-time index, not a `Part`, mutable ledger, or
recogniser it could invoke.

This is deliberately not yet the one aggregate-wide freeze. Several unmigrated physical families
still discover after recess reconciliation, so the private legacy ledger may append after an index
snapshot; those later writes cannot change the earlier index. The sole terminal freeze follows
only when all physical discovery moves ahead of reconciliation under epic 0003. Public standalone
recognisers keep their existing ledger-compatible facades while their migrated discovery cores
receive only neutral graph facts and the write-only sink.

## Amendment (framework consolidation, issue #159)

Aggregate recognition now has one explicit lifecycle. It derives neutral run context, completes
all applicable physical discovery, binds every returned occurrence to exactly one family-scoped
candidate, and terminally seals evidence once. Existing reconciliation rules then consume only
the complete candidate inventory and frozen index; pattern records are derived afterward from
accepted members, and projection merely unwraps those inventories into `RecognitionResult`.

The terminal seal rejects later proposals and a second seal. Standalone compatibility may still
take non-closing point-in-time snapshots, but aggregate census and attribution consume the same
`InventoryProduct` as public result construction rather than running discovery or policy again.
The temporary physical-family roster is deliberately closed and explicit until issue #160 owns
registry metadata; cylinders, graph facts, rotational classification and turned-profile
applicability remain context or value dependencies, not physical candidates.

## Amendment (geometry foundation, issue #219)

Physical dependency reads now cross one issuer-owned completion boundary rather than a record-only
side table. Immediately after each registry discoverer returns and output validation succeeds, the
issuer atomically binds the complete returned occurrence sequence, creates deliberate empty
Candidates where required, closes later issuance for that family and retains that exact
CandidateSet for terminal inventory. Failed completion publishes no empty prefix, occurrence handle
or completed-family state.

Downstream physical adapters receive opaque occurrence handles only for their exact declared,
already-completed predecessors. Every handle read revalidates exact Candidate/record/evidence
identity, original graph nodes and the common valid SolidRef. This is completed physical provenance,
not an EvidenceIndex, disposition or accepted-state view: discovery still cannot enumerate global
Candidates, inspect reconciliation or infer whether an occurrence will survive policy. Opaque
restricted-input capabilities bind the consumer definition and declared predecessor roster, so a
family cannot broaden its own dependency authority.

## Amendment (framework consolidation, issue #158)

Every emitted physical aggregate candidate now receives exactly one identity-preserving
`Disposition`. The implemented closed outcomes distinguish acceptance from rejection; closed
namespaced reasons state the named policy, and `related` holds the actual
same-run winning or compatible candidates in source order. The reconciliation result stores only
the ordered dispositions. Accepted candidate sets, pattern inputs and the distinct-step census
projection are derived views rather than synchronized inventories.

The existing policies retain their order and meaning. Recess and bevel precedence reject the
less expressive proposal and name its winner. A TurnedStep and Groove that describe the same band
both remain accepted and name each other as compatible; only the census view excludes that step
from a second physical-feature count. Empty defining evidence proves neither containment nor
compatibility. Candidates untouched by a named conflict receive default acceptance.

This protocol can disposition only proposals that discovery emitted. In particular, the #111
subdivided-terminal miss has no AngledStep candidate and cannot honestly be labelled ambiguous or
unsupported by reconciliation without rediscovering geometry. Bounded missing-candidate residual
diagnosis remains assigned to issue #161 under ADR 0004.

## Amendment (framework consolidation, issue #160)

The aggregate's physical and derived execution rosters are now one closed internal registry rather
than duplicated call lists. Definitions declare only value flow: Holes consume completed
Countersinks, Plates consume completed TurnedSteps, and the three pattern projections consume
accepted Hole, Slot or Pocket records after reconciliation. Restricted input views reject an
undeclared sibling read. Reconciliation relationships and their order are not registry metadata;
they remain named policy in `_reconcile` over the completed candidate inventory and terminal
evidence.

Every physical and derived definition also states whether it contributes to an existing census
key or is deliberately not counted. This is completeness evidence, not authority to create or
reorder public census keys. Public records, exports, schemas and result construction remain manual
contracts so adding registry metadata cannot publish a capability accidentally.

## Amendment (framework consolidation, issue #161)

Reconciliation remains strictly candidate-only. A failed predicate emits no Candidate and cannot
honestly receive a disposition. The bounded residual phase therefore consumes separately issued
failed-predicate observations only after reconciliation; it neither changes accepted candidates
nor searches geometry. The first consumer diagnoses a subdivided AngledStep terminal only when
the same slant survives as an accepted planar Chamfer. Diagnostics remain private and do not alter
`RecognitionResult`, census, capabilities or public family precedence.

## Amendment (geometry foundation, issue #182)

The neutral blend-collapsed view cannot issue Candidates and is not part of aggregate discovery.
Logical handles are never accepted by `EvidenceSink`. A future named consumer must first expand a
logical occurrence to same-run original `FaceNode`s, then explicitly classify the complete
provenance into defining and consulted evidence. Existing candidate identity, terminal freeze and
reconciliation remain unchanged; complete geometric provenance alone does not establish ownership.

F5 adds closed private registry attribution dispositions and a terminal validator. Registry metadata
states expected family completeness but cannot create evidence, accept a Candidate, or authorise a
reconciliation rule. After the sole evidence freeze, every fully attributed family occurrence must
have non-empty issuer-snapshotted defining evidence; incomplete families may mix measured and empty
occurrences. `candidate_set_for` remains the only record-occurrence binding path.

## Amendment (F6 accepted correspondence snapshots, issue #185)

Cross-run correspondence begins from a private optional snapshot issued only after terminal
evidence validation and completed reconciliation. `_take_inventory` binds one lazy opaque authority
to the exact `InventoryProduct`; copied, reconstructed, mutated or mixed products cannot reuse it.
The snapshot selects exact accepted Candidate identities from reconciliation and never treats
`RecognitionResult`, completed discovery order or record equality as authority. F6a records one
run's accepted RRP facts only: it performs no matching, changes no disposition and publishes no
result/schema/census surface. F6b may compare two such snapshots only after this substrate is
independently accepted.

F6b1 remains a private optional consumer. Its sole authority-bearing entry accepts two exact
`InventoryProduct` objects and materializes their issuer-validated schema-three snapshots; a
snapshot-only leaf has no public or production caller. Matching starts with snapshot-local body
groups, requires one shared proper-similarity witness for every occurrence assigned between a
group pair, and enumerates the complete maximum-weight group/occurrence solution. Equal or
coincident descriptors retain multiplicity. A nonunique body assignment, occurrence assignment,
or witness becomes one `AMBIGUOUS` component; no tuple position, object identity, hash, or nearest
distance establishes correspondence. F6b1 emits no split/merge claim and changes no recognition,
disposition, result, registry, census, capability, or manifest state.

## Amendment (F6 correspondence withdrawn, 2026-09-14)

The two amendments above are withdrawn as implementation. `_correspondence`,
`_correspondence_match`, `_correspondence_partition` and the schema-three `_body_geometry`
boundary grammar are removed, along with `FaceGraph.body_geometry` and
`FaceGraph.matching_boundary`, whose only callers they were. Nothing in the package or its
public surface consumed them: the matcher had no `src` importer, and `_take_inventory`
constructed and bound a snapshot authority on every run whose only reader was the matcher.

The requirement they addressed is unchanged and still unmet. Accepted records carry run-local
Candidate identity only, so an editing consumer cannot yet say that an occurrence in one run is
the occurrence it edited in another. The reasoning above — that a persistent identity must be
re-proved geometrically rather than minted from traversal order, that ambiguity must fail
closed, and that no tuple position, object identity, hash or nearest distance may establish
correspondence — remains the accepted design for whatever meets it next.

What removal records is a scope judgement, not a reversal. The delivered matcher covered one
family, `REPEATING_RADIAL_PROFILES`, itself `NotCounted("correspondence evidence is not a
distinct feature")`, and the per-occurrence snapshot was written against that family's shape.
Whether the approach generalises to holes or pockets was never established, so the cost was
being carried without the evidence that would justify it. Reintroduction needs a named
downstream consumer, a compatibility declaration, and this ADR amended again.

## Amendment (Slot curved-depth closure, issue #353)

Slot discovery must prove that both ends of its selected depth axis remain open. A deep obround
blind pocket can otherwise invert the length/depth interpretation: its machining depth is the
longer wall extent, its real curved footprint ends appear on the proposed through axis, and the
planar floor is queried on the wrong axis. The resulting paired-wall record describes empty volume
but is not a through Slot.

The existing immutable AAG is the authority for this distinction. If a connected non-planar region
joins both proposed walls smoothly and its aggregate boundary closes either selected depth end, the
Slot predicate refuses before issuance. The region is expanded across smooth patch subdivisions,
but separate curved ends connected only through planar walls remain distinct. Correct obround Slot
caps lie on the long-axis ends; nonsmooth added-material interruptions remain eligible under the
existing rule. This adds no sibling read or reconciliation policy, changes no public schema, and
uses `COORD_FLOOR` only to compare coordinates belonging to the same topological boundary.

## Amendment (public within-run evidence projection, issue #375)

The one completed `InventoryProduct` may issue a narrow public, read-only evidence view. Public
`FeatureRef` values retain exact accepted Candidate occurrence identity and public `FaceRef` values
retain exact original `FaceNode` identity only inside that view. Neither handle is a record,
serializable identity, reconciliation input or cross-run correspondence key. The view exposes no
proposal or rejected Candidate and cannot discover, accept, reject or mutate evidence.

Defining evidence remains the sole ownership relation and the sole face relation reconciliation
may read. Issue #368 may add constituent membership to the same terminal view only as a second
non-owning projection with `defining` as a required subset. A consumer cannot construct either
relationship from adjacency, labels or record values.

## Amendment (AngledStep subdivided terminal recovery, issue #111)

The issue #158 and #161 statements that the subdivided terminal supplies no `AngledStep` Candidate
are superseded only for the closed geometry proof accepted by ADR 0004: an axis-aligned planar
terminal with exactly three straight cyclic outer-boundary runs. Discovery now emits the missing
Candidate with its original slant as defining evidence and its exact original terminal face as
constituent evidence. The existing Chamfer candidate remains independently proposed; the unchanged
named bevel precedence then rejects it using exact shared defining evidence.

This does not add a fourth reconciliation outcome, let reconciliation rediscover geometry, or make
the result order-dependent. Curved, unreadable and genuinely four-run terminals still emit no
`AngledStep`. The former failed-predicate diagnostic remains a compatible closed explanation value
but is no longer issued for geometry that passes the terminal proof.

## Amendment (Double-D bore precedence, issue #304)

An accepted `DoubleDBore` supersedes an ordinary `HoleRecord` only when the Hole's non-empty exact
defining-node set is a proper subset of the Double-D occurrence's complete defining wall set. The
Double-D record describes the same constant through void and additionally retains both planar
flats; the Hole is the partial-cylinder interpretation of that boundary. The rule runs after all
discovery against terminal evidence, rejects only the Hole Candidate, and relates it to the exact
winning Double-D Candidate. Direct family discovery remains independent.

Coordinates, diameter, axis and record equality are not occurrence authority. Coincident voids
on separate solids, disjoint ordinary holes and incomplete/empty evidence therefore cannot be
collapsed. Hole-pattern derivation consumes the accepted Hole inventory and cannot recreate the
rejected partial interpretation.

The bounded public explanation projection adds the matching closed
`ReconciliationReason.HOLE_SUPERSEDED_BY_DOUBLE_D_BORE` value. This is an additive public enum
member, not a record-schema or capability-manifest change; aggregate clients that display or
persist reconciliation reasons can distinguish the corrected physical decision.

## Amendment (free-axis rectangular Slot successor, issue #310)

`SectionPassage` remains the sole physical discovery authority for a closed constant-section
through ring. A completed occurrence may issue `OrientedSlot` only when its serialized boundary
proves a non-square four-line rectangle not representable by legacy principal-axis `Slot`. The
successor reissues the exact source walls; explicit reconciliation rejects only that generic
Passage occurrence. Pattern recognition consumes accepted successor records only.
