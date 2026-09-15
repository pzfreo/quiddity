# ADR 0010 — Publish a narrow geometry facade; keep correspondence optional

- **Status:** Accepted
- **Date:** 2026-08-26
- **Decider:** Paul Fremantle
- **Evidence:** [epic 0004 retrospective](../epics/0004-architecture-retrospective.md),
  [F7 geometry facade spike](../f7-geometry-facade-spike.md)
- **Record form:** current state, rewritten 2026-09-15. The per-issue amendments this file
  carried until then are in `git log -- docs/adr/0010-narrow-public-geometry-facade.md`.

## Context

Epic 0004 built a rigorous private substrate: graph-owned identity, analytic-surface facts,
blend-collapsed views, canonical sections and defining evidence. Its full surface is too broad to
freeze because a consumer needs a few neutral geometry answers. The original wording permitted a
`geometry` facade; a consumer spike showed the real need was narrower, and this record now states
what that evidence selected.

## Decision

**Publish for a named consumer use, not from an export inventory.** A surface leaves the private
substrate only when an installed-wheel consumer operation needs it. Sufficiency is not need: the
in-package spike proved a `GeometryGraph` facade could serve every family and was still a no-go,
because the consumer's workflow needed one analytic fact off one face.

**Three published surfaces, each with its own closed manifest.**

- `quiddity.inspection`: the declared-feature inspection roster. One closed analytic fact off one
  face so that a declared feature and a detected one agree. Its contract is `inspection_api.json`
  (format 1); the earlier root and family-module entry points are identity-preserving aliases.
- `quiddity.evidence`: a run-local, read-only view over one completed aggregate run. It issues
  opaque `FeatureRef` and `FaceRef` values, maps a feature to its exact accepted record, its
  **defining** faces and its **constituent** faces (`defining` a required subset), resolves a face
  reference to the borrowed build123d face, and reports association coverage with explicit
  numerators and denominators. Framed evidence maps working-shape faces back to caller faces by
  exact OCCT topology identity under the retained rigid placement, requiring a bijection, and
  returns a typed refusal otherwise. Its contract is `evidence_api.json`.
- `quiddity.experimental_geometry`: `GeometryGraph`, opaque identities, adjacency, blend facts and
  `inspect_face`. It stays out of the package root and the capability manifest until a consumer
  need graduates part of it, as `inspect_face` graduated into `inspection`.

**References are run-local, never persistent names.** `FeatureRef` and `FaceRef` compare only by
same-view identity, cannot be serialised, carry no index or hash, and fail closed when forged,
copied or supplied to another view. Equal-valued occurrences get different references. Symmetric
faces may be indistinguishable; a persistent identity is a separately reviewed layer that must
represent ambiguity rather than resolve it by traversal order.

**Membership is retained, never reconstructed.** Constituent faces are the identities the
recogniser's own accepted proof selected. Wider membership is not rebuilt later by coordinate
matching, adjacency flood-fill, corpus labels or a second pass. Where a family needs a bounded
region search, it runs inside that family's pre-Candidate proof and lives in its module.

**Not published:** graph construction, adjacency, blend collapse, sections, Candidates,
`EvidenceIndex`, the registry, reconciliation, issuers, run tokens and cross-run correspondence.
The F6 correspondence layer was withdrawn on 2026-09-14 for want of a consumer; a future
correspondence API is a new ADR and an API-major decision, not an additive detail here.

## Enforced by

- The three installed-wheel manifests and their validators, with consumer typing checks in
  `tests/typing/`.
- `tests/test_architecture.py`: `experimental_geometry` absent from root exports; private
  section adapters used only by the unified projection; correspondence absent.
- Evidence tests: forged, copied and cross-view references refuse; `defining ⊆ constituent`;
  framed caller-face mapping refuses a non-bijection.

## Consequences

Consumers get a small API and compatibility burden; private modules stay replaceable behind
projections. The cost is that a consumer who wants graph-shaped answers has to make the case with
a workflow, and the package answers with the narrowest surface that serves it.
