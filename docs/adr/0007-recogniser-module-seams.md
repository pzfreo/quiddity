# ADR 0007 — Internal recogniser module seams

- **Status:** Accepted
- **Date:** 2026-08-16
- **Record form:** current state, rewritten 2026-09-15. The per-issue amendments this file
  carried until then, one per module added, are in
  `git log -- docs/adr/0007-recogniser-module-seams.md`.

## Context

Two oversized modules once combined topology scans, feature interpretation and pure pattern
geometry, so a change to one family appeared coupled to the others and the flow of shared
evidence was invisible. The fix had to be mechanical: no added scan, no changed record, no
reordered result, no new public import path.

## Decision

**Private implementation modules behind thin public facades.** Every module added since is
private (`_name.py`) unless a reviewed public contract says otherwise. The public module roster is
closed and listed as `PUBLIC_MODULES` in `tests/test_architecture.py`. A public family module
re-exports from its private core with object identity and `__module__` preserved; it is never a
second implementation.

**The seam table is the record.** The allowed dependency edges between modules are the
dictionary `MODULE_SEAM_EDGES` in `tests/test_architecture.py`, one entry per module, with a
comment where the reason is not obvious. This file no longer restates that table. Adding a module
or an edge means adding it there in the same PR, and the review of that edge is the architecture
review. Every module except the root re-export has an entry (`test_every_module_has_a_seam_entry`),
and an edge the table does not list fails the suite.

**Layers, bottom up.** The table is acyclic (`test_module_graph_is_acyclic`). The layers below
are the intended reading of it, not a theorem: a few reviewed exceptions (a family facade
importing `result` inside a function, `_run` reaching `experimental_geometry`) and the helper
modules that sit between layers are recorded as comments in the table:

1. Leaves and near-leaves: `_typing`, `_record`, `_manifest`, `_solid_properties`,
   `_body_identity`, `_geometry`.
2. The graph: `_analytic_surfaces`, `_adjacency`.
3. Shared substrates and evidence primitives that scan or probe once and publish no record:
   `_effective_surfaces`, `_surface_facts`, `_blend_view`, `_cylinder_substrate`, `_volume_probe`, `_wire_seed`,
   `_support_patches`, and the run-local evidence types `_candidates`, `_claims`, `_dispositions`.
   `experimental_geometry` sits here too: a public wrapper over the graph and surface index with
   a reviewed consumer roster (`_run`, `_geometry_evidence`, `pads`, `polygonal_bosses`).
4. Family cores and shared proofs (`_cylinder_stacks`, the `_recess_*`, `_section_*` and
   `_cylindrical_*` modules) and the public family modules over them.
5. Orchestration: `_reconcile`, `_run`, `_registry`, `result`.
6. Projections and facades: `census`, `explanations`, `evidence`, `inspection`, `capabilities`,
   `frames`, `document`, `step_io`, `cli`.

Interpretation depends on geometric fact; the reverse edge, a graph module importing a
recogniser, is what would make the graph mutable, so it stays absent.

**Rules the seams protect.**

- One scan per run: shared inventories are computed in a substrate and injected downward, never
  duplicated in a family module. Recess families share one face inventory; pattern modules are
  record-agnostic and perform no topology scan.
- Family modules interpret injected evidence and never call a sibling recogniser; importing a
  sibling's record type or helper is allowed.
- The registry imports family modules, never their internals. It reads the `PhysicalDefinition`
  each module declares; a family's private discovery core is reached only from that declaration
  and, where the family has one, its own public entry point. The layer 5 -> layer 4 edge is
  therefore one import per family module rather than one per private name, and each core's
  callers are pinned by `tests/route_pins.py`.
- The reconciler imports no discovery module and calls no recogniser.
- Migrated discovery cores receive a write-only evidence sink, never an index they could read.
- Some modules have a reviewed consumer roster rather than an open edge: `_blend_view`,
  `_manifest`, `_section_adapters`, `experimental_geometry`. A new consumer is a reviewed change
  to that roster.
- Step Levels and Risers issue occurrences through the same writer seam as every other family.
  Their public values are not injective occurrence keys, so the writer binds each occurrence to
  its own body-local faces rather than rematching by value.

## Enforced by

`tests/test_architecture.py`: `test_module_graph_is_acyclic`,
`test_internal_module_seams_match_adr_0007`, `test_every_module_has_a_seam_entry`,
`test_no_accidental_public_modules`,
`test_compatibility_facades_preserve_export_identity_and_module_paths`,
`test_reconciler_never_imports_or_calls_discovery`,
`test_migrated_discovery_cores_receive_write_only_evidence`,
`test_recess_families_keep_one_shared_face_inventory_and_patterns_are_pure`, the per-roster
consumer tests, and `test_runtime_package_does_not_import_draftwright`. Golden fixtures and
determinism tests protect behaviour across any move.

## Consequences

A hole or boss change no longer shares a file with pattern allocation; a recess change no longer
shares a file with its pattern interpretation. The installed wheel gains private files but no
public symbol, so a module move is a patch-level change. The price is that every new module is a
table edit and a review; that is the intended friction.
