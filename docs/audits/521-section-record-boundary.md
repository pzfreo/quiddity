# SectionRecess module boundary — issue #521

Baseline: `5532ae6`, epic #514.

## Dependency finding

The former `_section_recess` mixed immutable record validation with FaceGraph traversal,
kernel construction, material probing and public geometry projection. Record consumers such as
the public facade, legacy section adapters and evidence projection consequently imported a module
that also owned native discovery. This is a useful responsibility boundary, not a file-size target.

## Bounded extraction

- `_section_recess` retains every existing public record class and its validation.
- `_section_recess_geometry` owns the moved kernel-backed candidates and geometry projection.
- `_section_recess_discovery` assembles candidate evidence for the aggregate registry.
- `section_recesses` remains the public facade and existing document builder.
- `result` retains reconciliation, legacy-to-unified assembly, deduplication and pattern remapping.

The move changes no predicate or method body. Public type identity, defining module, constructor
fields and JSON remain unchanged. Private geometry consumers and tests import the new owning
module; there is no reverse import from records to the new geometry implementation.

The record module still uses existing PassageFrame/PassageSection value types and the analytic
section validator. This is a separation of direct responsibilities, not a claim that importing
the package becomes kernel-free: the historical `passages` module still contains implementation.
Splitting that shared type hierarchy is not necessary to make this change useful.

## Why not split result.py again here?

The current orchestration owns exact accepted identity, reconciliation order, legacy projection,
deduplication and pattern-index publication together. Moving a few functions without moving their
authority would add another import boundary but would not simplify that lifecycle. Leave that
module intact in this bounded issue; its stale description is documentation work in #522.

## Proof

Architecture tests constrain all three internal layers and preserve the exact arc-policy callsite
roster under the renamed module. Public class-identity tests, geometry goldens, rounding regressions,
cutover/migration tests and the existing section tests verify the unchanged public contract.
No new schema, second document builder, discovery pass, corpus access or performance work.
