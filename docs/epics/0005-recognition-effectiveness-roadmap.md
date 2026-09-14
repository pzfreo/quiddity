# Epic 0005 — Recognition effectiveness and transfer evidence

**Status:** active
**Owner:** @pzfreo
**Opened:** 2026-08-28
**Baseline:** `16c5e9d` (0.4.5.dev0; 2,388 tests collected)
**Tracker:** [#290](https://github.com/pzfreo/quiddity/issues/290)

## Outcome

Improve the usefulness and geometric reach of the aggregate recogniser by connecting the
foundation already built, filling the most consequential vocabulary gaps, and measuring whether
those changes transfer beyond project-authored fixtures.

This is an effectiveness epic, not a new foundation programme. Architecture is work in service of
a named recogniser or downstream capability. A private abstraction, proof or package is not an
outcome by itself.

The epic balances three concerns:

1. preserve the core model's identity, provenance, deterministic output and fail-closed contracts;
2. turn existing surface, blend, frame and diagnostic machinery into accepted feature records and
   useful explanations;
3. improve results on independently authored datasets without fitting production geometry rules to
   a benchmark taxonomy.

## Why now

The August 2026 project scorecard and the Epic 0004 retrospective agree on the main constraint:
the foundation has been built faster than it has been connected. Canonical surface recovery,
blend-collapsed graph views and correspondence machinery are individually substantial, but their
recogniser and public consumers remain narrow. (Correspondence was the sharpest case: it never
reached a consumer and was withdrawn on 2026-09-14 — see the F6 withdrawal amendment in ADR 0003.)
At the same time, known MFCAD++ classes have little or no coverage, and unsupported input commonly
produces an empty result without a useful account of why.

The next material improvement therefore comes from integration and measured feature coverage, not
from another general substrate designed in advance of a consumer.

## Evidence policy

### Dataset roles

| Evidence source | Role | How it may be used |
| --- | --- | --- |
| Project fixtures and goldens | Exact contracts, determinism, compatibility and adversaries | Direct development and regression evidence |
| Real third-party parts | Practical geometry and downstream-use evidence | Direct development evidence, with provenance |
| MFCAD++ | Open development corpus and false-negative detector | Inspect models and labels, diagnose failures, choose work, tune topology rules, and add justified regressions |
| MFInstSeg | Independent transfer baseline | Run at baseline and milestones; use aggregate and per-class results for direction, but do not copy individual models into tests or encode unexplained dataset-specific exceptions |

This policy deliberately replaces the previous one-shot bucket sealing and unsealing process. This
is deterministic geometry software, not a trained statistical model. The cost and latency of the
old protocol exceeded the confidence it added. Consistency, provenance and honest disclosure are
the controls here.

MFInstSeg is an evaluation baseline, not a secret oracle. Its results may show that a family or
class needs attention. The resulting implementation must still be justified by a general geometric
contract and demonstrated with MFCAD++, an authored adversary or a real part. If an individual
MFInstSeg model is inspected, record that fact in the benchmark report; the run remains useful but
is no longer described as independent for that class.

### What dataset labels may decide

Dataset labels may locate likely omissions and quantify agreement. They do not define public record
semantics, feature ownership at intersections, reconciliation policy or numerical tolerances.
MFCAD++ and MFInstSeg taxonomy mappings must be explicit, versioned and allowed to say
`incomparable`.

In particular, a single face label is not authoritative evidence that only one physical feature
owns the face. Corpus-label agreement and the package's defining-face attribution are reported as
separate views.

### Required score vector

Do not collapse the epic to one accuracy grade. Every corpus report records, where its annotations
permit:

- models loaded, invalid and evaluated;
- emitted occurrences by package family and mapped dataset class;
- supported-class instance recall;
- defining-face precision and recall;
- cross-family or taxonomy-mismatch counts;
- models returning no physical records;
- unsupported, rejected and reconciliation-dropped outcomes when diagnostics expose them;
- runtime distribution and corpus total;
- exact commit, package version, dataset version/hash, selection rule and mapping version.

Every percentage carries its numerator and denominator. Unsupported geometry, absent recogniser
families, predicate rejection, reconciliation loss, taxonomy mismatch and unreadable input are not
merged into a generic miss when they can be distinguished.

### Baseline and comparison

Before the first behaviour-changing child lands:

1. capture the existing golden, real-part, MFCAD++ and performance results;
2. add or freeze the MFInstSeg adapter and taxonomy mapping;
3. run and publish the MFInstSeg baseline using the score vector above;
4. state any models or classes already inspected by the development team.

Rerun MFCAD++ after each capability package. Rerun MFInstSeg at meaningful milestones and at epic
close, not on every predicate edit. Keep historical reports immutable; scoring-code corrections
produce a new report and explain the changed denominator rather than rewriting the baseline.

## Delivery rules

1. **Geometry first, corpus second.** Every new acceptance rule states the topology and geometry
   that make it valid independently of the dataset example that exposed the gap.
2. **Consumer with substrate.** A foundation change lands only with a named downstream consumer or
   as the smallest separately reviewable prerequisite to an already scoped consumer issue.
3. **Prefer two consumers before generalising.** One family may justify a private helper; a shared
   abstraction normally needs two materially different consumers.
4. **No unexplained thresholds.** New numerical gates require units, local scale authority,
   boundary tests on both sides and a reason not derived solely from corpus optimisation.
5. **Preserve evidence.** Original-face provenance, candidate identity, complete dispositions and
   deterministic projection remain non-negotiable.
6. **Measure end to end.** A package is not complete because a surface or graph query works. It must
   measure accepted records, false positives, empty results and runtime through the aggregate path.
7. **Report neutral or negative results.** No corpus improvement is a valid outcome when the work
   establishes that a proposed migration or family is not valuable.
8. **Do not redesign while migrating.** Mechanical adoption of an existing neutral API and a
   feature-semantic change should be separate commits or issues whenever they can be reviewed
   independently.

## Work packages

The order below is a priority order, not permission to make one large pull request. Each package is
split into PR-sized tracker issues with its own baseline and acceptance evidence.

### E0 — Reproducible effectiveness baseline

Build one corpus-report schema and adapters for MFCAD++ and MFInstSeg. Freeze the baseline, taxonomy
mapping, denominators, runtime environment and known limitations. Reuse the existing candidate and
defining-face evidence rather than inferring correctness from record counts where possible.

**Exit gate:** another agent can reproduce the reports from documented commands and obtain the same
counts; no production predicate changes are included.

### E1 — Explain empty and partial results

Turn the existing refusal, observation and disposition facts into a bounded aggregate diagnostic:
what geometry was seen, which relevant surface kinds were unsupported or recovered, which families
were inapplicable, and which candidates were rejected or reconciled away. Do not promise a complete
causal proof where the current evidence cannot supply one.

**Exit gate:** representative raw-spline, oblique, blended and unsupported-family cases no longer
return an unexplained empty result; public compatibility is separately reviewed if diagnostics are
exported.

### E2 — Make framed recognition the ordinary safe route

Close the decision and compatibility gaps around the released framed path. Resolve the exposed
working-shape need in [#282](https://github.com/pzfreo/b123d-recognisers/issues/282), determine
whether framing can become the default aggregate behaviour, and preserve an explicit raw/world
frame route where required.

This package fixes whole-part presentation bias. It must not claim support for a feature oblique
inside an otherwise aligned part.

**Exit gate:** golden and corpus rigid-motion comparisons retain equivalent occurrences and record
the coordinate-frame contract; default behaviour changes only through an accepted ADR and release
plan.

### E3 — Connect canonical surface recovery

Deliver [#276](https://github.com/pzfreo/b123d-recognisers/issues/276) family by family, beginning
with the eligible consumers that produce the largest practical recognition gain. Keep raw topology,
orientation-deferred and torus-deferred sites explicit rather than forcing a uniform migration.

Each family migration records native-versus-recovered parity, spline-converted fixture results,
MFCAD++ effect, false positives, area/threshold sensitivity and runtime.

**Exit gate:** the migrated families recognise their supported geometry through the aggregate path;
the effective-surface index is no longer counted as value merely because it fits faces in isolation.

### E4 — Connect blend-collapsed views where recall warrants it

Execute [#277](https://github.com/pzfreo/b123d-recognisers/issues/277) as measurement followed by
targeted adoption. Prefer recognisers whose geometric contract survives provenance-preserving
collapse without weakening material or completeness proofs. Retire family-local workarounds only
after parity evidence.

**Exit gate:** every new consumer has blended positive cases, non-blend controls, ambiguous-chain
refusals, expanded original-face evidence and end-to-end corpus results. A measured absence of value
closes a proposed consumer without migration.

### E5 — Fill high-value vocabulary gaps

Use open MFCAD++ results, downstream need and reconciliation leverage to order missing families.
Begin by resolving the remaining step geometries in
[#89](https://github.com/pzfreo/b123d-recognisers/issues/89) and
[#111](https://github.com/pzfreo/b123d-recognisers/issues/111), subject to fresh baseline counts.
Reassess [#249](https://github.com/pzfreo/b123d-recognisers/issues/249) after the shared local-frame
contract is settled rather than extending axis-letter records.

A new family includes its immutable record, defining evidence, positive and near-miss fixtures,
aggregate reconciliation, manifest entry, MFCAD++ before/after measurement, runtime and downstream
capability decision.

**Exit gate:** the family improves supported occurrence coverage without an unexplained precision
loss in existing families, and any contested-face improvement is reported separately from raw
family recall.

### E6 — Downstream effectiveness and final transfer report

Exercise the changed outputs in concrete inspection, drawing or manufacturing-data tasks rather
than stopping at recognition records. Select only workflows with an identified consumer; do not
invent a general intermediate representation inside this epic.

At the frozen epic head, rerun the complete MFCAD++ and MFInstSeg reports and compare them with E0.
Report gains, regressions, unchanged gaps, taxonomy disagreements, runtime and any MFInstSeg models
inspected during development.

**Exit gate:** at least one real downstream workflow benefits from the recognition changes, and the
final report is reproducible without claiming that synthetic-corpus agreement equals real-world
accuracy.

## Child issue contract

Every child issue must state:

- the user-visible or recogniser outcome;
- the exact baseline commit and relevant existing behaviour;
- supported geometry, exclusions and ambiguous cases;
- dataset classes and downstream consumers expected to move;
- tests to add or update: positive, negative, tolerance boundary, transformed/order variant,
  aggregate reconciliation and installed/public contract as applicable;
- before/after MFCAD++ commands and score-vector fields;
- whether an MFInstSeg milestone run is required;
- performance budget and rollback/compatibility implications;
- documentation, manifest and ADR effects;
- what evidence would justify closing with no implementation.

An agent starting a child reads this epic, the child issue, linked ADRs and the latest relevant
benchmark report. The agent records commands, commit, corpus identity and denominators in the PR or
report. It must not rely on conversation history or unstated observations from a previous agent.

## Epic completion criteria

- [ ] E0 baseline and MFInstSeg evaluation approach are reproducible and documented.
- [ ] Empty or materially partial aggregate results have useful bounded diagnostics.
- [ ] The framed path has an accepted default/opt-in decision and compatibility story.
- [ ] Canonical recovery has multiple end-to-end recogniser consumers with measured value.
- [ ] Blend views are adopted only in families with demonstrated benefit.
- [ ] The highest-value missing step families are implemented or closed with recorded evidence.
- [ ] At least one downstream workflow demonstrates the value of the changed recognition output.
- [ ] MFCAD++, MFInstSeg, authored fixtures and real-part evidence are reported separately.
- [ ] Final corpus reports show exact numerators, denominators, versions and known contamination.
- [ ] Documentation and the capability manifest describe shipped behaviour rather than planned
      foundation work.

## Explicit non-goals

- a machine-learning training or model-evaluation protocol;
- one-shot sealed buckets, acknowledgement tokens or per-family unsealing ceremonies;
- maximising agreement with a dataset's single-label taxonomy;
- publishing private geometry machinery without a concrete consumer;
- completing correspondence or persistent identity work without a downstream use case;
- a single composite grade or accuracy percentage;
- threads, sheet metal or other new domains unless baseline evidence and a consumer reorder them
  through a separately reviewed child issue.

## Risks and containment

| Risk | Containment |
| --- | --- |
| Benchmark-specific predicates | require a corpus-independent geometry explanation, adversaries and boundary tests |
| Architecture again outruns value | consumer-with-substrate rule and end-to-end exit gates |
| Dataset taxonomy distorts public semantics | explicit mappings, `incomparable`, and separate attribution reporting |
| MFInstSeg gradually becomes development data | disclose inspected examples and justify fixes outside MFInstSeg |
| Aggregate score hides family regressions | score vector with per-family numerators and denominators |
| Runtime grows with every shared query | lazy run-owned facts and package-level performance budgets |
| One maintainer or changing agents lose context | child issue contract plus immutable benchmark reports |

## Source assessments

- Project scorecard assessed at `16c5e9d`, 2026-08-28.
- [`0004-architecture-retrospective.md`](0004-architecture-retrospective.md).
- [`../scorecard.md`](../scorecard.md), retained as project-internal historical assessment.
