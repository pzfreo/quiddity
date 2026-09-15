# ADR 0005 — Public surfaces and their versioned contracts

- **Status:** Accepted
- **Date:** 2026-08-15
- **Decider:** Paul Fremantle
- **Record form:** current state, rewritten 2026-09-15; absorbs ADR 0010 (narrow geometry facade)
  and ADR 0012 (bounded explanations). The full format-1 examples, the consumer declaration
  schema and the per-issue schema events are in `git log` on this file.

## Context

The package owns geometry-only recognition; a consumer owns its IR, declarations, drawing policy
and completeness semantics. A new family can be valid package capability with no consumer
representation, and a consumer declaration can outlive the adapter that justified it. Code and
prose do not make that boundary safe; it has to be machine-checkable from the installed package.
Separately, the private substrate (graph identity, analytic facts, candidates, reconciliation) is
too broad to freeze because a consumer needs a few neutral answers.

## Decision

**Publish for a named consumer use, not from an export inventory.** A surface leaves the private
substrate only when an installed-wheel consumer operation needs it. Sufficiency is not need: the
spike that could serve every family through a `GeometryGraph` facade was still a no-go because the
consumer's workflow needed one analytic fact off one face.

**Four published surfaces, each with an independent closed manifest or contract.**

1. **Recognition.** The `recognise_*` entry points, public records, `RecognitionResult`, census
   and the framed routes. Contract: `capabilities.json` (`quiddity-capabilities`, format 2),
   returned by `capability_manifest()` and validated by `validate_capability_manifest`.
2. **Explanations.** `build_raw_recognition_report` and its framed variant run the one
   aggregate and return the unchanged result beside one entry per closed physical family in
   registry order: evaluated or not-applicable, proposed/accepted/rejected counts, closed
   `ReconciliationReason` summaries, and residual diagnostics projected from frozen evidence.
   Coverage is always `BOUNDED`: a missing diagnostic means no bounded diagnostic was established,
   not that nothing was missed. Counts are detector candidates, not public occurrences, since
   accepted candidates can converge on one `SectionRecess`. `RecognitionResult` gains no field.
   The compatibility surface is the immutable Python types; there is no JSON form.
   `build_recognition_report` is the raw compatibility alias, as for results.
3. **Inspection.** The declared-feature roster: `inspect_face` plus the four declared-feature
   reads (countersink rims, bevel classification, double-D tool, pocket floor anchor), each one
   closed fact off one face so a declared feature and a detected one agree. Contract: `inspection_api.json` (`quiddity-inspection-api`, format 1).
4. **Evidence.** A run-local read-only view over one completed run: opaque `FeatureRef` and
   `FaceRef`, each feature's record, defining faces and constituent faces (`defining` a required
   subset), and association coverage with explicit denominators. References compare by same-view
   identity, never serialise, and fail closed when forged or crossed. Framed evidence maps
   working-shape faces to caller faces only by exact OCCT identity under the retained placement,
   requiring a bijection. Contract: `evidence_api.json` (`quiddity-evidence-api`, format 1).

`experimental_geometry` stays out of the root and every manifest until a consumer need graduates
part of it, as `inspect_face` did. Not published: graph construction, adjacency, blend collapse,
Candidates, `EvidenceIndex`, the registry, reconciliation, run tokens, and cross-run
correspondence, withdrawn on 2026-09-14 for want of a consumer.

**The capability manifest.** Families are named by permanent lower-case identifiers
(`holes`, `hole-patterns`) that survive any rename of module, function or class; a rename is an
alias with deprecation and removal versions, kept for at least one major-version cycle. Every exported `recognise_*` and every exported
record appears exactly once under one owning family with: `status` (`supported`, `deferred`,
`unsupported`), `introduced_in`, `recognisers` each with `kind` (`part`, `derived`) and `role`
(`physical`, `compatibility`, `derived`), `records` each with a positive `schema_version`
describing the serialised `to_dict()` contract and a record `role`, `census_name` and
`census_output` or a `census_rationale`, and `golden_evidence`, `test_evidence` and
`documentation` paths that must exist in the source archive. Package CI derives the real
inventory independently and fails closed on anything unlisted, duplicated, stale or misordered;
canonical expected data is input to that check, never rewritten by it. A wheel's manifest data
is identical to the sdist's. A later format may grow only through a namespaced `extensions`
object whose entries declare whether a reader may ignore them; unknown fields otherwise fail.

**The consumer side is the consumer's.** A consumer keeps its own declaration pinning a package
version range, manifest format and every record schema it reads, and validates it only against
the installed package's public surface. Unknown families are never silently treated as
geometry-only or mapped to a generic feature. The consumer-side states and their transition
rules are the consumer's own record; this one governs package releases only.

**Compatibility events.**

| Change | Release |
| --- | --- |
| Add a supported family, an optional record field, or a record `schema_version` | Package minor; consumer accepts explicitly before use |
| Fix prose or an evidence path | Patch |
| Required field, changed meaning/unit/type, removal, or identifier reuse | Next minor before 1.0, major after; alias and deprecation first where representable |
| `ReconciliationReason` value added | Public enum addition with a private/public parity guard; not a schema or manifest event. Removing or changing a value is a compatibility event |
| Any `format_version` increase | New schema major; readers reject until upgraded |

Pre-1.0 is not permission for silent drift. One recorded exception: the free-axis Slot successor
shipped in a patch release by the owner's explicit override; its additive requirements stood.

## Enforced by

- `validate_capability_manifest`, `validate_inspection_api_manifest` and the evidence manifest's
  internal check, with their manifest tests; `tests/test_capability_claims.py`; the consumer
  typing check in `tests/typing/`.
- `tests/test_architecture.py`: every defined public recogniser is exported; correspondence
  absent; the published `ReconciliationReason` values and `RecognitionResult` fields may
  only grow without a deliberate edit to the pinned roster. `tests/test_experimental_geometry.py` and `tests/test_inspection_api.py` keep
  `experimental_geometry` out of the root.
- Evidence and explanation tests: forged, copied and cross-view references refuse;
  `defining ⊆ constituent`; the report's result is the same object as the plain run's.

## Consequences

A recogniser cannot become public without a stable identifier, record contract, evidence and
documentation, and a consumer cannot silently ignore a family or keep a dead declaration.
Geometry-only evidence is a first-class state, not a hole in a checklist. Adding a family is
therefore a registration tax; it is paid deliberately, once, and checked by machine.
