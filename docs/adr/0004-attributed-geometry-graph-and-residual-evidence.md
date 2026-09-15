# ADR 0004 — Attributed geometry graph and residual evidence

- **Status:** Accepted
- **Date:** 2026-08-15
- **Decider:** Paul Fremantle
- **Record form:** current state, rewritten 2026-09-15. The per-issue amendments this file
  carried until then are in `git log -- docs/adr/0004-attributed-geometry-graph-and-residual-evidence.md`.

## Context

Recognisers that each traverse faces and encode adjacency locally cannot explain why a feature
was accepted, cannot report geometry no recogniser claimed, and cannot stop two recognisers from
silently owning one region. Analysis Situs demonstrates the substrate: an attributed adjacency
graph over B-rep faces. This record adopts the pattern, not the runtime or its algorithms; see
[prior art](../prior-art-feature-recognition.md).

## Decision

**One immutable per-run face graph** (`FaceGraph` in `_adjacency`). Nodes are original faces,
identified by run-local `FaceNode` handles. Arcs are shared boundaries carrying `ArcKind`
(concave, convex, smooth) and, where proven, a `SmoothSide` material-side enrichment. Closed-solid
ownership is issued as `SolidRef`; `common_valid_solid(nodes)` is the one body-provenance proof,
and every non-empty defining set must resolve to one valid solid before publication. Consumers
query the graph; nothing mutates or substitutes it.

**Named layers above the graph, all opt-in.**

- `EffectiveSurfaceIndex` (`_effective_surfaces`) derives at most one closed native, recovered or
  refused analytic fact per original node. Recovered geometry is unoriented until a
  `MaterialSideCertificate` is proved by bounded solid probes on the exact original face; a
  canonical axis sign is never material-side evidence. Consumers receive a `SurfaceUse` that
  retains the original node. Recovery is enabled only for the reviewed OCP/OCCT binding.
- The blend-collapsed view (`_blend_view`) hides selected cylindrical blend faces from logical
  incidence. Every logical node expands to complete original provenance; the view cannot issue
  Candidates, and its consumers are a reviewed roster.
- Seeing through a blend or across a split face is a **named query a recogniser asks for**, never a
  widened default adjacency. Making blended neighbours simply neighbours would move every family's
  answers at once, which the characterisation corpus forbids.

**Identity.** Raw face indices and graph handles are run-local. Public records contain
geometry-derived values and serialisable evidence summaries, never live OCP objects or handles.

**Evidence roles.** A Candidate's defining evidence is the set of original faces that establish
it; constituent evidence, where a family publishes it, is wider physical membership with
`defining` as a required subset, retained at the recogniser's own decision site and never
reconstructed later by coordinate matching or adjacency flood-fill.

**Residual evidence.** A failed predicate may issue an `Observation` carrying closed primitive
facts. The private residual reducer (`_diagnostics`) is a bounded identity join over observations
and accepted candidates: it does not rescan topology and is not a second recogniser. Broader
residual classification and a public diagnostic schema are not built; the requirement stands.

The graph is recognition infrastructure. It does not decide whether residual geometry should be
dimensioned or shown; consumers translate neutral diagnostics into their own policy
([ADR 0001](0001-standalone-geometry-only-apache-library.md)).

## Enforced by

- `tests/test_architecture.py`: every arc reader has one reviewed disposition; the surface-reader
  roster covers every raw classification; the blend index and view have only reviewed production
  call sites; the residual reducer cannot rediscover or mutate geometry.
- `tests/test_arcs.py` and the effective-surface tests for the graph and index contracts.
- Golden fixtures: existing recogniser outputs stay stable while a family migrates onto the graph.

## Consequences

The graph gives every family the same neutral facts and lets
[ADR 0003](0003-one-recognition-result-and-explicit-reconciliation.md) reconcile by exact face
identity instead of by comparing coordinates each family derived its own way. The cost is identity
and tolerance discipline: a recogniser that wants to see through a blend must say so, and a family
that wants wider membership must retain it when it proves the feature.
