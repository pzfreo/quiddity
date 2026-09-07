# Scorecard: b123d-recognisers as a 3D geometry utility

An assessment of this project as a utility for B-Rep feature recognition, scored against
[Analysis Situs](https://analysissitus.org) and the wider state of the art — classical
graph/hint/volumetric methods, the 2021–2026 learned branch (UV-Net, BRepNet, Hierarchical
CADNet, AAGNet, BrepMFR, BRepFormer), and commercial recognisers (CADfix, Spatial CGM, HOOPS,
Siemens NX, CAMWorks/FeatureWorks, Autodesk Fusion).

**Assessed:** 23 August 2026, at `main` (`d73f612`) **plus [PR #174](https://github.com/pzfreo/b123d-recognisers/pull/174)**
(`a387c44`, "Close framework follow-up gaps from #173"), which completes the epic-0003
recogniser framework — fail-closed recess reconciliation, validated disposition reasons,
registry/projection binding checks, and `docs/adding-a-recogniser.md`. Quality figures quoted
below (875 tests, 96.36% coverage, the interleaved benchmark pairs) are that PR's verified
numbers. This page extends, and does not replace,
[`prior-art-feature-recognition.md`](prior-art-feature-recognition.md), which records the
architectural reading behind ADR 0004; external facts below that are not in that survey were
checked August 2026 and will date.

---

## What is being scored

`b123d-recognisers` is a deterministic, rule-based feature recogniser for imported STEP/B-Rep
geometry: ~13,300 lines of typed Python on build123d/OCP, exposing 25 `recognise_*` families
that return frozen, JSON-serialisable parametric records (a hole with axis, diameter, depth and
bottom classification — not a face label). Since epic 0003 it discovers over an attributed face
graph (`FaceGraph`, four-valued arc classification), writes per-record face evidence into a
write-only claim ledger, and resolves inter-family conflicts in an explicit reconciler with a
closed table of 13 reasoned dispositions. Scope is deliberately narrow: geometry-only (no
machining or drawing policy), single-library (no app, no viewer), analytic surfaces only.

## Executive summary

**As a library for recovering dimensioned engineering intent from clean, axis-aligned,
analytic-surface prismatic and turned parts, in Python, with an honest and machine-readable
statement of what it can and cannot do, this project is genuinely good — and its
evidence-and-measurement discipline exceeds every open-source peer, including Analysis Situs.**
As a general 3D geometry recognition utility it is not yet competitive with Analysis Situs or
commercial recognisers: it has no canonical (B-spline→analytic) recovery, a pervasive
principal-axis assumption, no blend suppression, no defeaturing, and a feature vocabulary
several times smaller. It does not compete with the learned branch on per-face recall over
arbitrary feature variety, and by design (ADR 0002) it should not try; it competes on the thing
the learned branch does not produce — deterministic parametric records.

| Dimension | Grade | One-line justification |
| --- | :---: | --- |
| Parametric record quality | **A** | Frozen, typed, JSON-serialisable, unit-annotated records with a versioned manifest; peer of Analysis Situs, ahead of all learned methods |
| Determinism & explainability | **A** | Byte-identical semantic goldens, traversal-order invariance, cross-platform tie-breaks, reasoned dispositions |
| Validation honesty | **A** | Sealed 33-model holdout, per-face claim scoring, pinned corpora, capability manifest that fails closed |
| Engineering quality | **A−** | 875 tests, 96% branch coverage, 3 OS × 3 Python CI, mypy/ruff, py.typed, OIDC publishing; alpha-stage API |
| Architecture | **B+** | AAG + write-only claims + explicit reconciliation is the field's converged shape; registry deliberately closed, no plugin path |
| Feature-interaction handling | **B−** | Real reconciliation across 6 claiming families and measured intersection wins; but interaction coverage is thin vs hint-based/volumetric methods |
| Feature coverage | **C+** | 25 families incl. patterns and turned features; no threads, through steps, internal grooves, ribs, vertex blends, sheet metal |
| Performance | **C+** | Fine for interactive single-part use (~0.2 s/part composite); ~0.9 s/part census on real NIST parts, on a faster host and after two rounds of performance work totalling about 6.2×; no parallelism; C++ peers are orders faster |
| Geometric generality | **D+** | Analytic surfaces only, no B-spline recovery (fails closed to *nothing*); principal-axis bias measured at 78% zero-recall on oblique MFCAD classes |
| Ecosystem reach | **C** | pip-installable Python with one dependency is unique in this field; but single-consumer provenance, no non-Python story, alpha status |

**Overall: a B+ special-purpose utility with A-grade engineering, C-grade generality.**

---

## Detailed scorecard

### 1. Parametric record quality — A

Every family returns records carrying the values a downstream CAD/CAM/drawing consumer
actually needs: `HoleRecord(axis, location, diameter, depth, bottom ∈ through/flat/drill_point/unknown, cbore, spotface, csink)`,
`CounterSink(major_diameter, drill_diameter, included_angle)`, `Slot`/`Pocket` with axes, spans
and material side, pattern records (`BoltCircle`, `RectGrid`, `LinearArray`) with pitch and
basis conventions, `TurnedStep`/`Groove`/`Chamfer`/`Fillet` for turned parts. Records are
frozen dataclasses of JSON-serialisable values only — no kernel handles — with per-field units
(`mm`/`deg`/`unit-vector`) in the shipped `capabilities.json` manifest (25 families, 53 KB,
regenerated deterministically, unknown format versions fail closed).

This is the strongest axis of the project and the correct one to be strong on: no published
learned system emits complete parametric feature records end-to-end (they emit face labels;
parameters must still be fitted afterwards), and among rule-based peers only Analysis Situs's
hole recogniser and the commercial CAM recognisers are comparable. What is missing versus those
peers: **persistent feature identity across model edits** (Analysis Situs attaches persistent
IDs; records here are per-run values with no cross-run correspondence) and machining semantics
(deliberately out of scope per ADR 0001).

### 2. Determinism and explainability — A

ADR 0002's contract is unusually strong and unusually *enforced*: equivalent geometry must give
byte-identical canonical records, pinned by 20 semantic goldens including a `traversal_order`
fixture that builds the same topology through different operation orders; a documented
dominant-axis tie-break removes a real Windows/Unix OCCT divergence; scale sweeps at 0.05×–100×
pin the tolerance-only families. Every reconciliation outcome carries a `ReasonCode` from a
closed, validated table (PR #174 rejects unregistered reasons with `ValueError`), so a dropped
record is attributable — "this chamfer was superseded by an angled step" — rather than silent.
No competitor in any branch documents this level of output discipline; it is the property that
lets a CAM toolpath or drawing dimension consume the output without human review, which is
exactly where 94%-accurate classifiers cannot go.

### 3. Validation honesty — A

The measurement machinery is the project's most distinctive asset:

- **Per-face attribution where it matters.** The claim ledger records which faces established
  each record, so the six claiming families are scored against MFCAD++'s per-face labels by
  *observed ownership*, not fitted counts — including the pinned finding that a genuine
  6-sided passage's walls carry five different corpus labels, i.e. the corpus is wrong at
  intersections and the score knows it.
- **A sealed holdout.** 33 MFCAD++ val-split models, disjoint from the 40 design models by
  construction, scored once after the last predicate change: 8 angled-step records at 100%
  precision with 70 confusable faces available to go wrong on; zero stock-face claims over
  1,037 labelled faces; a defect found and sealed. The stated policy — re-scoring is fine,
  tuning against it costs a fresh draw — is train/test hygiene applied to a rule-based system,
  which neither Analysis Situs nor any commercial vendor publishes.
- **Real-part regression.** 10 NIST CTC/FTC parts (550–1170 mm, real PMI models) and 3 real
  turned parts pinned per-family, with the change-detector tier explicitly labelled as not a
  correctness baseline.
- **Capability contract.** `capabilities.md` states per-family proven scope *and exclusions*
  with primary evidence, mirrored in a machine-readable manifest CI derives from the installed
  package, so an undocumented recogniser fails closed.

The honest caveat, which the project itself states: both MFCAD corpora are synthetic, and the
design-set figures are regression evidence, not generalisation estimates. Recall on real-world
variety is unknown beyond 13 real parts.

### 4. Engineering quality — A−

875 tests passing at 96.36% branch coverage (enforced ≥91%), CI on ubuntu/macos/windows ×
Python 3.10/3.12/3.14 with SHA-pinned actions, mypy (near-strict) + ruff, `py.typed` with a
wheel-level typed-consumer fixture, architecture tests that enforce module seams and the
one-way discovery→reconciliation boundary by AST inspection, OIDC trusted publishing with
TestPyPI dev snapshots, 9 accepted ADRs, benchmark budgets in a JSON policy file. The minus:
`Development Status :: 3 - Alpha`, a 0.x API with deprecation shims already accumulating, and a
bus factor of one.

### 5. Architecture — B+

The epic-0003 framework (completed by PR #174) lands on the representation the field converged
on: an attributed adjacency graph (Joshi & Chang 1988; Analysis Situs's `asiAlgo_AAG`; the gAAG
under AAGNet and the 2024–26 transformers). The local variant is sound and in places
better-reasoned than the precedent — write-only evidence sinks make output order-independence
structural rather than disciplinary; defining-vs-consulted evidence prevents a fillet
manufacturing conflicts with the faces it bridges; identity-based (not value-based) candidate
matching fixed a real bug the golden corpus missed. Differences from Analysis Situs's AAG that
currently cost capability: arc classification is four-valued (`convex/concave/smooth/unknown`)
versus seven (no `SmoothConcave`/`SmoothConvex` — the tangential-join-with-a-material-side
distinction the prior-art page itself flags as "the thing a recogniser needs"); there is no
`Collapse()` blend-suppression primitive (`smooth_region` sees *through* a blend for coherence
queries but recognisers cannot analyse the joined faces as if the blend were absent — issue
#60's chamfered groove is this gap); no subgraph stack. The execution registry is deliberately
closed, and that is really two decisions fused together. **Closed adjudication** — the finite
`FamilyId` enum, the 13-reason disposition table bound to family pairs, the sealed evidence
index, registry-declared execution order — is load-bearing for every A-grade row above and
should stay closed: a runtime plugin would break determinism, the capability manifest and the
census contract at once. **The private substrate** is the part that actually costs points:
`FaceGraph`, the arc queries and `smooth_region` are underscore-private, so the only way to
build on this project's geometry reasoning is to fork it — where Analysis Situs's standing as
"the only open-source feature-recognition *framework*" comes precisely from its substrate
being the product. Publishing the neutral substrate as a versioned API — third-party
recognisers building alongside, returning their own records, simply not part of the closed
aggregate — captures the framework value without touching the moat; see recommendation 7
below.

### 6. Feature-interaction handling — B−

Interaction is where all rule-based recognition breaks, and the project's answer — let families
over-claim independently, then adjudicate from recorded evidence with 13 reasoned rules
(precedence and compatibility kinds) — is structurally right and measurably working: the
MFCAD++ suite pins that reconciliation drops exactly the 8 chamfer proposals that are really
angled-step slants, that obround pockets defeat competing wall-pair fragments, and that no
claim lands on stock across both corpora. PR #174's fail-closed rule (empty defining evidence
proves nothing) closes a vacuous-suppression hole. But the reconciliation vocabulary spans one
cluster (recesses, bevels, turned steps/grooves); hint-based systems (OOFF/IF²) and volumetric
decomposition handle arbitrary interactions by construction, MFCAD++ packs 3–10 interacting
features per model precisely because this is the hard axis, and features that *destroy* each
other's defining faces (a bolt hole through a step's closing flat is handled; general
subdivision mostly is not — one residual diagnostic code exists) remain the open frontier here
as everywhere.

### 7. Feature coverage — C+

25 families is respectable and includes things peers lack (double-D bores, hexagonal
stock/bosses, repeating radial profiles, plates/levels/risers for drawing support, hole *and*
pocket *and* slot patterns with completeness semantics). But against the FeatureNet/MFCAD++
24-class machining taxonomy the project itself adopts, the whole through-step group is
unrecognised (epic 0002); against Analysis Situs: no vertex blends, no blend chains, no
sheet-metal bends/flanges, no isolated-feature suppression/defeaturing at all (recognition
only, by scope); against CAM recognisers: no threads/tapped holes (largely a STEP limitation,
but NX/CAMWorks infer them), no machining-volume output. Oblique and curved variants of
recognised classes are excluded (measured: three mostly-oblique MFCAD classes return nothing in
78% of their models).

### 8. Performance — C+

Recorded honestly and budgeted: composite workload 0.85 s for four golden parts plus a census;
census over 13 real parts 12.0 s (~0.9 s/part on 550–1170 mm NIST models, from ~5.8 s/part
before the performance work began); peak RSS 500–592 MB, up 2.5% on the census arm where
round 2's caches live; the 661× pattern-allocation fix guarded by an operation-count sentinel
rather than CI wall-clock. PR #174's interleaved
pre/post-epic pairs show the framework cost nothing. The census figures were re-measured on an
Apple M5 Max rather than the epic development container, so the per-part numbers combine a
faster host with a ~6.2× code speedup — 4.7× from the run-scoped caching series and a further
1.33× from the round-2 adjacency, support-cut and volume-probe work — and only the code part is
portable; see [`benchmarks/recognition-budget.md`](benchmarks/recognition-budget.md) for the
same-machine pairs. Round 2 left the composite arm at x1.01, inside its own spread — read as
unchanged rather than as a 1% gain. This is adequate for interactive single-part import and
small batches, and the honesty of the
budget machinery is exemplary — but it is Python orchestrating OCP per face: C++ recognisers
(Analysis Situs, CGM, commercial CAM) run equivalent queries interactively on much larger
models, and learned inference is milliseconds per part after training. No parallelism, no
incremental re-recognition (the graph is per-run, as ADR 0004 itself notes).

### 9. Geometric generality — D+

The two hard walls, both documented and both measured:

- **Analytic surfaces only, failing closed to nothing.** A NURBS export whose faces are exact
  cylinders typed as `GeomAbs_BSplineSurface` recognises as *empty*. Analysis Situs treats
  canonical recognition (curvature-probing B-splines back to plane/cylinder/cone/sphere/torus)
  as a *precondition* of feature recognition, and the independent 2025 JCDE result (0 of 12
  features recognised before analytic replacement, 12 of 12 after) shows why. This is the
  single largest capability gap versus every serious peer, and it gates real-world usefulness
  on which CAD system wrote the STEP file.
- **Principal-axis bias.** Polygonal bosses/stock and pads are Z-only; recess wall filtering,
  passages, double-D bores, angled steps and the turned treatments require principal axes;
  only bosses accept free axes. The MFCAD oblique-class measurement (above) quantifies the
  cost. Peers are axis-agnostic.

Scale behaviour, by contrast, is handled better than peers document: a uniform
`rel*nominal + floor` tolerance policy (ADR 0008) with the 0.2.4 lesson that minimum-evidence
gates must *not* scale (19 records lost on real parts when they did) recorded as a test.

Single solids and per-solid multi-solid input; no assembly semantics (mates, instances) —
common to all recognisers in this survey except HOOPS's assembly-level features.

### 10. Ecosystem reach — C

Unique niche: the only pip-installable, permissively-licensed, typed-Python deterministic
feature recogniser (`build123d>=0.9` its sole runtime dependency). For the growing
build123d/CadQuery code-CAD community there is simply no alternative. Against that: Analysis
Situs ships an interactive workbench, Tcl console, viewer and imperative API and is "the only
open-source industrially proven FR framework" with ~10 years of history and named industrial
use; this project is months old on PyPI, alpha, extracted from and still shaped by a single
consumer (Draftwright), with no GUI, no visual debugger for *why* a face wasn't claimed
(observations/diagnostics are the seed of one), and no non-Python consumption story beyond
JSON.

---

## Head-to-head: Analysis Situs

The closest comparable — the other open-source, deterministic, AAG-based, OCCT-backed
recogniser — and the architectural precedent ADR 0004 cites.

| | b123d-recognisers | Analysis Situs |
| --- | --- | --- |
| Language / kernel | Python on build123d/OCP (OCCT) | C++ on OCCT; Tcl scripting; Active Data model |
| License | Apache-2.0, all of it | BSD-3 core; commercial tier (CNC milling/turning recognisers, sheet-metal production tooling) |
| Graph | `FaceGraph`, 4-valued arcs, per-run, write-only evidence + sealed index | `asiAlgo_AAG`, 7-valued arcs incl. smooth-sided pair, subgraph stack, results accrete as attributes |
| Blend handling | Recognises external fillets/chamfers; `smooth_region` sees through blends for coherence only | Recognises **and suppresses** blend chains (EBF/VBF, spring/cross edges); `Collapse()` lets recognisers see through blends — the capability issue #60 shows is missing here |
| B-spline input | Excluded, fails closed to empty | Canonical recognition recovers analytic surfaces first |
| Holes | Coaxial stacks, cbore/spotface/csink, bottom classification | Symmetry extraction + pattern dictionary: plain/countersunk/counterbored/counterdrilled, blind/through, full parameters |
| Beyond both | Slots/pockets/passages/channels with reconciliation, hole/slot/pocket patterns, plates/levels/risers, double-D, hex stock/boss, radial repetition | Sheet-metal bends + unfolding, defeaturing, DFM checks, CNC part typing (lathe/mill/mill-turn), persistent feature IDs, interactive workbench |
| Determinism evidence | Byte-identical goldens, traversal-order test, cross-platform tie-breaks, sealed holdout, per-face claim scoring | Deterministic by design; no published golden/holdout/measurement discipline |
| Extensibility | Closed registry; fork to extend (guide: `adding-a-recogniser.md`) | Tcl plugins, C++ framework explicitly for building custom recognisers |
| Maturity | 0.3.x alpha, one maintainer, one downstream | ~10 years, industrial deployments, active releases (2025.2) |

**Read:** Analysis Situs is the more capable *geometry* system — canonical recognition, blend
suppression, richer arc taxonomy, defeaturing, sheet metal — and the more mature product. This
project is the more rigorous *measurement* system, the stronger typed-data contract, and the
only one of the two consumable as a Python library. They are complementary more than
competitive; the three Analysis Situs capabilities most worth borrowing (in the pattern-not-code
sense ADR 0004 already adopts) are, in order: canonical recognition, blend collapse, and the
smooth-sided arc values.

## Against the learned state of the art

Per-face segmentation on synthetic benchmarks is a solved-looking problem the field is already
moving past: MFCAD saturated (~99.9%), MFCAD++ at 97.4% (Hierarchical CADNet 2022) → ~99%
(AAGNet-class, 2023–24), instance segmentation on MFInstSeg (AAGNet), transformers with global
attention (BrepMFR, BRepFormer — 93.2% on the harder MFTRCAD, which is the more honest number),
self-supervised B-rep pretraining, and synthetic-to-real domain adaptation as an explicitly
open problem. None of it outputs a diameter. The gap between "99% of faces correctly labelled
*through hole*" and "a `HoleRecord` a CAM system can drill from" is instance grouping plus
parameter fitting plus determinism, and that gap is this project's entire reason to exist —
ADR 0002 closes the learned branch deliberately, and `prior-art-feature-recognition.md` states
the honest position: not on the academic frontier, and correctly not trying to be.

Where the learned work still matters here: MFCAD++/MFInstSeg as false-negative detectors (already
adopted, done properly with claims-based scoring); and the convergence signal that even
commercial vendors (HOOPS AI, 2025: transformer+GNN per-face classifier) are bolting learned
labelling onto parametric pipelines. A hybrid — learned proposal, deterministic verification —
is the one future in which the ADR 0002 boundary might be renegotiated without giving up
pinned goldens, and the prior-art page already names its precondition (a decidable, explainable
output).

## Against commercial recognisers

NX, CAMWorks/FeatureWorks, CADfix, Spatial CGM and Fusion all emit parametric features, several
reconstruct editable feature trees or drive toolpaths directly, and field reports put standard-
feature automation at 85–95%. They are richer (threads, machining semantics, defeaturing,
tolerances/PMI) and faster. None is open, embeddable in Python, or transparent about failure
modes; none publishes anything like a capability manifest or a holdout score. For its actual
target — an open recognition layer under code-CAD tooling — the commercial tier is not a
substitute; for a manufacturing workflow, this project is not yet a substitute for them.

## What would move the grades most

Ranked by grade-impact per unit of work, consistent with the project's own epics and the
prior-art page's conclusions:

1. **Canonical recognition** (B-spline→analytic with bounded residual) — turns the D+ in
   geometric generality into a B and multiplies the value of everything else, because it
   removes the dependence on which CAD system exported the file. The declared fail-closed
   behaviour makes this safe to add incrementally.
2. **Oblique-axis relaxation** in the recess/wall filters — the 78% zero-recall measurement
   shows the principal-axis assumption, not the vocabulary, is the dominant recall ceiling on
   third-party geometry.
3. **Blend collapse** — issue #60's chamfered groove documents features hidden behind
   lead-in blends; Analysis Situs's `Collapse()` is the proven pattern, and the two smooth-sided
   arc values are its prerequisite.
4. **Through-step family** (epic 0002) — the one whole MFCAD++ group with zero coverage.
5. **Claims in the remaining families** — converts the estimated (fitted) corpus figures for
   holes/fillets/bosses into measured per-face precision, extending the project's strongest
   property to its whole surface.
6. **Persistent feature identity** across re-recognition of edited models — the Analysis Situs
   capability a STEP *editor* (the stated consumer) will want first.
7. **A published framework API** — promote the neutral substrate (`FaceGraph`, arc and
   smooth-region queries, and once epic 0004 lands, the canonical-surface view, collapsed views
   and `LocalFrame`/`PlanarSection`) to a public, versioned contract under ADR 0005 discipline.
   Adjudication stays closed; extension moves out-of-tree. This is the one lever that moves
   ecosystem reach, and it doubles as the bus-factor mitigation: external recognisers become a
   nursery, proving themselves out-of-tree and graduating into the registry with the evidence
   bar already met. Timing matters — publish only after the epic's F1–F4 stop churning exactly
   these APIs, or the freeze taxes every package after it.

## Can the foundation scale to these, or is it boxed in?

Mostly the former. Five of the six changes above land on foundations the epic-0003 framework
visibly built for; the one genuine corner-in-progress is oblique-axis generality, and it lives
in the public record schemas rather than the architecture — which means it gets more expensive
with every release and is cheapest to fix now, before 1.0.

**Canonical recognition — a clean slot, not a wall.** The whole-package B-spline exclusion is
the right corner *not* to have cut: because recognition fails closed to *nothing*, canonical
recognition can be added as a pre-pass that rewrites B-spline faces to analytic surfaces before
`FaceGraph` construction, and nothing downstream changes. That routing matters because surface
typing is scattered — 42 `GeomAbs_` call sites across 12 modules — so a per-face
"effective surface" adapter would touch everything, while a shape-level rewrite touches
nothing. OCCT ships `ShapeAnalysis_CanonicalRecognition` (7.7+, reachable through OCP), the
fit is deterministic given a tolerance so ADR 0002 holds, and ADR 0008 already supplies the
vocabulary for bounding the residual. The D+ in geometric generality is a missing pre-pass,
not a rearchitecture.

**Claims in remaining families, and through steps — green by construction.** This is what
epic 0003 and PR #174 paid for up front: the write-only sink, sealed index and registry make
migrating a family to claims mechanical, `adding-a-recogniser.md` is the recipe for a new
family, and the framework's cost was measured at zero performance regression.

**Blend collapse — amber, and the local version can improve on the precedent.** The immutable
per-run `FaceGraph` has no `Collapse()`, but `smooth_region` already demonstrates the right
idiom: derived *views* over an immutable graph rather than Analysis Situs's mutating collapse,
whose own header warns that collapsed attributes are not cleaned up by `PopSubgraph`. A
collapsed view is additive, and the evidence model survives it — defining evidence names real
faces, which a recogniser working through a view can still claim. The prerequisite is
enriching `ArcKind` from four to six values (adding the smooth-sided pair; Analysis Situs's
seven-member enum needs no further mirroring here, since `unknown` already covers `Undefined`
and non-manifold input is out of scope), and the `"smooth"` literal does not leak outside
`_adjacency.py`, so that is a contained internal refactor rather than a contract change.

**Persistent feature identity — green, as a layer.** Records-are-values means identity cannot
live inside records, but ADR 0004 already concluded face indices are not persistent identity
anyway. Cross-run correspondence — fingerprint-matching quantised records between runs — is a
sidecar consistent with the recognition-versus-policy split. Not blocked; just not free.

**Oblique axes — the real one, in two layers of different health.** The *machinery* is
axis-agnostic: `FaceGraph` arcs are classified from surface normals, not world axes, and
ADR 0009 already moved the axis-aligned filtering out of shared reductions and into the
families that own it, so relaxing predicates is normal work. The *contract* is not: roughly
twenty `axis: str` (`"x"`/`"y"`/`"z"`) fields across the public records, and
`Slot`/`Pocket`/`Channel` parameterised as axis-aligned spans (`lo`/`hi`, `d_lo`/`d_hi`,
`w_center`). An oblique pocket is inexpressible in that schema at any tolerance — no predicate
change fixes it. The escape route is already demonstrated in the newer families:
`PrismaticPocket`/`Passage` carry a free axis plus a `section` polygon, and `BossRecord` takes
a free vector. The path is therefore **supersession, not extension** — section-based records
grow to cover what span-based ones cannot, and the reconciliation precedence machinery for
"richer record defeats fragment" already exists and is tested. The cost is real (the
`_recess_*` machinery is the largest code mass, ~2,500 lines built around axis-aligned spans),
but it is a planned migration, not a rewrite. The trap is shipping 1.0 with the span-based
schemas as the frozen contract.

Two cross-cutting notes. **Performance is a tax, not a wall**: per-solid and per-family
parallelism composes with determinism (discover concurrently, sort canonically), though
canonical recovery and oblique axes will both multiply candidate spaces and deserve a budget
line. And **the actual scaling ceiling is not architectural**: the closed registry means the
vocabulary grows only as fast as one maintainer can produce goldens, corpus figures and
capability rows per family. That evidence bar plus a bus factor of one bounds the roadmap, not
the code — and it is also the moat, because the honesty machinery is the one thing no
competitor has. The structural mitigation is recommendation 7's published substrate API:
adjudication stays closed, but external recognisers can then prove themselves out-of-tree and
graduate into the registry with the evidence bar already met, so the ceiling scales with the
ecosystem rather than with one maintainer.

**Verdict:** the epic-0003 foundation reads as if designed against exactly this scorecard's
gaps — the fail-closed boundaries, evidence model and reconciliation are extension points, not
walls. The single most corner-avoiding move available is migrating the recess families to
section-based records before 1.0 freezes the axis-aligned worldview into the compatibility
contract.

## Epic 0004 projections and current correction

[Epic 0004 — Geometry foundation generalisation](epics/0004-geometry-foundation-generalisation.md)
(carried on this branch with review amendments applied; the review record is
[`epic-0004-feedback.md`](epic-0004-feedback.md)) operationalises gaps 1–3 and 5–7 above into
work packages F0–F7. The table is the current post-work assessment, not the projection made when
the epic was drafted:

| Dimension | Pre-epic | Current | Driver |
| --- | :---: | :---: | --- |
| Geometric generality | D+ | **D+** | F1 recovery has 0 of 64 recogniser reader sites migrated; F4b fixes whole-part presentation, not internally oblique features; F3b adds one bounded blend consumer with no matching external holdout population |
| Architecture | B+ | **B+** | The private substrate is stronger, but F7 is not published and F6 has no external consumer; a consumer-proven narrow facade remains the exit gate |
| Feature coverage | C+ | **C+** | Existing families gained stronger evidence and selected bounded variants, but the large B-spline and internal-obliquity recall gaps remain |
| Feature-interaction handling | B− | **B** | F5 closes 20 families as `FullyAttributed`; Step Levels and Risers remain structurally `IncompleteAttribution`, and interaction rules still need observed overlaps and separate review |
| Validation honesty | A | **A** | Already at ceiling; F0's MFTRCAD scanning and sealed draws extend the lead |
| Determinism & explainability | A | **A** | Maintained by construction — byte-identical goldens gate every neutral change |
| Parametric record quality | A | **A** | Section records are richer; F6 correspondence stays a private diagnostic, so the persistent-ID gap only half-closes |
| Engineering quality | A− | **A−** | Still alpha, still bus factor one; the two-independent-reviews process tightens dependence on scarce reviewer attention |
| Performance | C+ | **C+** | The composite is gated, and the census overrun recorded here (134.165s against a 109.651s ceiling) has since been closed by two rounds of performance work and a re-baseline to 11.990s; parallelism is still out of scope |
| Ecosystem reach | C | **C** | No recogniser consumes recovered surfaces and F7 is not public; Draftwright must first prove the smallest useful installed-wheel facade |

Overall: this remains a carefully engineered special-purpose utility. The epic improved internal
authority, evidence and bounded interaction handling, but it did not improve the measured D+
geometric generality, C+ feature coverage, C ecosystem reach or C+ performance grades. It is not
near parity with Analysis Situs as a recognition platform; this project remains strongest in
measurement discipline and typed, fail-closed contracts, while Analysis Situs has much broader
recognition and product capability.

Two review amendments shaped the original plan (the reasoning is recorded in
[`epic-0004-feedback.md`](epic-0004-feedback.md)), but their projected gains were not established:

1. **F4 is split, with its schema half landing early.** F4 carries the oblique half of the
   generality jump plus the whole coverage move, and it was the one package with a clock on
   it — every release pins the axis-span schemas deeper. F4a (the versioned
   `LocalFrame`/`PlanarSection` schema with byte-identical principal-axis projection) now
   lands right after F0, defusing the 1.0 corner even if the epic stalls and letting F1/F5
   write their fixtures against the final schema once instead of twice; F4b delivers the
   oblique predicates family-by-family.
2. **A framework API was proposed as the terminal package (F7).** The implementation work showed
   that publishing the broad substrate carries substantial compatibility and assurance cost, while
   no external consumer yet proves its utility. ADR 0010 therefore replaces that projection with a
   consumer spike followed, if justified, by the smallest useful `GeometryGraph` facade.

Future re-grades require observed consumer or corpus value, not completion of internal packages.

## Sources

In-repo: [`capabilities.md`](capabilities.md), [`prior-art-feature-recognition.md`](prior-art-feature-recognition.md),
ADRs [0001](adr/0001-standalone-geometry-only-apache-library.md)–[0010](adr/0010-narrow-public-geometry-facade.md),
[`benchmarks/recognition-budget.md`](benchmarks/recognition-budget.md),
[`benchmarks/pattern-grid-scaling.md`](benchmarks/pattern-grid-scaling.md), `migration/PARITY.md`,
the corpus suites under `tests/`, and [PR #174](https://github.com/pzfreo/b123d-recognisers/pull/174).

External (checked August 2026): Analysis Situs — [feature-recognition framework](https://analysissitus.org/features/features_feature-recognition-framework.html),
[recognition principles](https://analysissitus.org/features/features_recognition-principles.html),
[drilled holes](https://analysissitus.org/features/features_recognize-drill-holes.html),
[blends](https://analysissitus.org/features/features_recognize-fillets.html),
[canonical recognition](https://analysissitus.org/features/features_recognize-analytical.html),
[license](https://analysissitus.org/license.html), [source](https://gitlab.com/ssv/AnalysisSitus);
Joshi & Chang 1988 (AAG); Vandenbrande & Requicha (hint-based OOFF);
[UV-Net](https://openaccess.thecvf.com/content/CVPR2021/html/Jayaraman_UV-Net_Learning_From_Boundary_Representations_CVPR_2021_paper.html),
[BRepNet](https://openaccess.thecvf.com/content/CVPR2021/html/Lambourne_BRepNet_A_Topological_Message_Passing_System_for_Solid_Models_CVPR_2021_paper.html),
[Hierarchical CADNet / MFCAD++](https://dl.acm.org/doi/10.1016/j.cad.2022.103226),
[AAGNet](https://www.sciencedirect.com/science/article/abs/pii/S0736584523001369),
[BrepMFR](https://www.sciencedirect.com/science/article/abs/pii/S0167839624000529),
[BRepFormer](https://arxiv.org/abs/2504.07378),
[BRepGAT](https://academic.oup.com/jcde/article/10/6/2384/7453688),
[NURBS→analytic ablation, JCDE 2025](https://academic.oup.com/jcde/article/12/8/78/8197883);
commercial: [CADfix DX](https://www.iti-global.com/interoperability-products/cadfix/cadfix-dx/),
[Spatial CGM](https://www.spatial.com/solutions/3d-modeling/cgm-modeler),
[HOOPS AI feature recognition](https://docs.techsoft3d.com/hoops/ai/programming_guide/feature-rec.html),
[CAMWorks](https://camworks.com/why-camworks/),
[Fusion feature recognition](https://help.autodesk.com/cloudhelp/ENU/Fusion-360-API/files/FeatureRecognition_UM.htm).
Some primary sites were read via search excerpts behind an egress proxy; figures flagged as
approximate in the text should be treated accordingly.
