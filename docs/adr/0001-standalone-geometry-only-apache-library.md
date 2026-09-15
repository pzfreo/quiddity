# ADR 0001 — Standalone geometry-only Apache library

- **Status:** Accepted
- **Date:** 2026-08-15
- **Decider:** Paul Fremantle
- **Record form:** current state, rewritten 2026-09-15; absorbs ADR 0014 (geometry-only STEP
  loading). History is `git log` on this file.

## Context

Draftwright and build123d-mcp both need feature measurements from imported or otherwise
unattributed B-rep solids. Recognition is not drawing-specific, and a copy in each consumer drifts.
Draftwright's ADRs 0007 and 0013 selected a standalone package; this record establishes it.

## Decision

**One distribution, one direction.** The package is `quiddity` (published first as
`b123d-recognisers`), Apache-2.0. The dependency direction is always
`consumer → quiddity → build123d/OCP`; the package never imports a consumer.

**What it accepts.** build123d shapes, using OCP internally where necessary. The supported loader
for recognition inputs is `import_step_geometry(path) -> Shape`: it uses OCCT's plain
`STEPControl_Reader`, transfers every root, checks transfer completeness against OCCT's candidate
roots, and wraps the result with build123d's topology map, failing explicitly on a read, transfer,
null or unknown-topology failure. Assembly structure is flattened and product names, colours,
layers and hierarchy are not read: recognition never used them, and the metadata-aware importer
can terminate the interpreter on a valid file with an absent name attribute, which no exception
boundary can contain. Separate solids inside the transferred compound stay separate ownership
units. A consumer that needs metadata uses its own importer and passes the shape in. Corpus and
audit tools use this loader so a metadata defect cannot abort a measurement run.

**What it returns.** Geometric records, measurables, evidence and recognition diagnostics.

**What it does not own.**

- Drawing requirements, dimensions, callouts, views, placement or lint severity.
- Whether a feature is *significant*: a 1 mm fillet on a 200 mm part is reported, and whether
  it is worth dimensioning is the consumer's policy
  ([ADR 0008](0008-length-tolerance-policy.md) applies this to thresholds).
- Editing-session state, reconstruction commands or CAM operation selection.
- Consumer caches tied to a drawing, document or server lifecycle; only immutable recognition
  values cross the boundary.

Consumers adapt records into their own domain IR.

## Enforced by

- `tests/test_architecture.py::test_runtime_package_does_not_import_draftwright` and the
  published-prose tests, which keep consumer paths and consumer ADR numbers out of runtime prose.
- STEP loader tests: read, empty-transfer, null, partial-transfer and unknown-topology failures;
  parity of type, solid count, face count and face signatures with the metadata-aware importer on
  files both accept; a source guard keeping corpus tools off the metadata-aware importer.

## Consequences

Recognition is reusable by Apache, proprietary and copyleft consumers. Cross-repository releases
become necessary, so the public surface must stay small, versioned and explicit
([ADR 0005](0005-versioned-cross-repository-capability-contract.md)). Callers of the supported
loader knowingly lose metadata the package never used.
