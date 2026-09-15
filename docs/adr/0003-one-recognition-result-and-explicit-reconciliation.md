# ADR 0003 — One recognition result and explicit reconciliation

- **Status:** Accepted
- **Date:** 2026-08-15
- **Decider:** Paul Fremantle
- **Record form:** current state, rewritten 2026-09-15. The per-issue amendments this file
  carried until then are in `git log -- docs/adr/0003-one-recognition-result-and-explicit-reconciliation.md`.

## Context

Independent recognisers can describe overlapping physical regions: a groove floor may resemble a
turned step, a pocket floor a level, a pattern both a group and its members. First-match ownership
scattered among consumers produces different answers from the same solid, and a plain empty result
cannot distinguish absence, ambiguity, rejection and unsupported topology.

## Decision

**One inventory.** One aggregate run produces one immutable `RecognitionResult`. Census,
explanations, the evidence view, the section-recess document and the framed routes are
projections of that one inventory. A view counts or filters; it never re-runs recognition. Where
a view deliberately differs, the difference is a named rule over the shared inventory, such as
`steps_that_are_not_grooves` in `_reconcile`, which the census consumes as `distinct_steps`.

**One lifecycle.** The run derives its neutral shared state once and owns it as one
`RecognitionContext` (`_run`): face graph, cylinder scan, face-edge memo, surface index. Evidence
is deliberately outside it; the ledger and candidates belong to the inventory phase in `result`.
The run then completes every applicable physical
family through the closed registry (`_registry`); binds each returned occurrence to exactly one
family-scoped Candidate through a write-only `EvidenceSink`, atomically per family, so a family
whose validation fails publishes no partial prefix; seals evidence once into a read-only
`EvidenceIndex`; reconciles; derives pattern records from accepted members; and projects. The seal
rejects later proposals and a second seal.

**Discovery and reconciliation are separate stages.** Claims are written during discovery and read
only afterwards. A recogniser never declines a face because another family claimed it, so the
census cannot depend on family order. Recording what a recogniser used cannot change what it
returns. The ledger is deliberately not the face graph: the graph holds geometric fact, a claim is
an interpretation of it, and separating them keeps the graph immutable and reusable.

**Reconciliation is named policy.** The rules are the functions in `_reconcile.py`; they receive
completed candidates and the frozen index, never a `Part`, a mutable ledger or a recogniser. Every
physical candidate receives exactly one identity-preserving `Disposition`: accepted, or rejected
with a closed private `ReasonCode`, projected publicly as `ReconciliationReason`, and the winning
or compatible candidates in `related`. Which family wins each named conflict is stated in the
rule's docstring in `_reconcile.py`. Overlapping claims are evidence, not a verdict: a pattern and its members both survive;
a TurnedStep and a Groove describing one band both survive and only the census count corrects.
Empty defining evidence proves neither containment nor compatibility. Rules compare defining
evidence; one rule, prismatic pockets against blind slots, also reads constituent membership, and
any further such read is a reviewed change. The legacy `passages` projection is derived from
accepted occurrences and never feeds reconciliation, census or evidence backward.

**Identity.** Record identity is derived from geometry under documented tolerance, never from
Python identity, kernel traversal order, labels or a solid enumeration index. Run-local handles
(`FaceNode`, `FeatureRef`, `FaceRef`) are the opposite by design: compared by identity, never
serialised, meaningless once the part changes, and never leaked into a record.

**What reconciliation cannot do.** It corrects double-counting, not recall: when family A misses a
feature family B also proposes, precedence converts A's false negative into B's false positive,
and the census still looks right. A rule's ceiling is the recall of the weaker family, so evidence
for a rule includes what the losing family recognises. There is no fourth verb. A failed predicate
emits no Candidate; it may emit an `Observation`, which the private residual reducer joins to
accepted candidates after reconciliation without searching geometry.

**Cross-run correspondence is not provided.** The F6 matcher was withdrawn on 2026-09-14: it
covered one uncounted family and had no consumer. Accepted records carry run-local identity only.
A successor must re-prove identity geometrically, fail closed on ambiguity, never use tuple
position, object identity, hash or nearest distance, and arrive with a named consumer.

Consumer lifecycle caches are outside the result.

## Enforced by

- `tests/test_architecture.py`: the reconciler never imports or calls discovery; phase functions
  have one-way capability boundaries; only orchestration creates restricted completed inputs;
  every result field is registry-owned or a reviewed exception; correspondence stays absent.
- `tests/test_run_context.py`: one aggregate run derives each shared substrate once.
- Fixture tests for each named rule, and for the fact that ambiguous or unsupported geometry
  cannot return clean absence.

## Consequences

Consumers receive one explainable feature universe. Adding a family means one registry entry and,
where it overlaps an existing family, one named rule with evidence from both sides. Which
families were evaluated for a run is published by the explanation report
([ADR 0005](0005-versioned-cross-repository-capability-contract.md)); `RecognitionResult` itself
carries no such field.
