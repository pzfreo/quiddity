# ADR 0014 — Load recognition inputs without STEP assembly metadata

- **Status:** Accepted
- **Date:** 2026-09-02
- **Issue:** #421

## Context

Recognition accepts build123d shapes and reads B-Rep topology and geometry. It does not consume
STEP product names, colours, layers, or assembly hierarchy. The usual build123d STEP importer
nevertheless constructs an XCAF document and reads that metadata. With the currently pinned OCP
binding, asking an assembly-component label for an absent `TDataStd_Name` attribute can terminate
the interpreter with `SIGSEGV`; no caller exception boundary can contain it.

The public NIST AP242 CTC-02 conformance model reproduces the fault. OCCT's plain
`STEPControl_Reader` loads the same file as one compound with 637 faces and one solid because it
does not traverse XCAF labels.

## Decision

Publish `import_step_geometry(path) -> Shape` as the package's supported loader for recognition
inputs. It uses `STEPControl_Reader`, transfers every root, obtains `OneShape()`, downcasts that
topology, and wraps it with build123d's topology map. It fails explicitly when reading or transfer
does not succeed, the result is null, or build123d has no wrapper for the returned topology.

Assembly structure is intentionally flattened. Separate solids inside the transferred compound
remain separate ownership units; metadata is not reconstructed or represented as empty values.
Consumers requiring names, colours, layers, or assembly hierarchy retain responsibility for a
metadata-aware importer and may pass its resulting shape into recognition.

Repository corpus and audit tools use this loader so an uncatchable metadata defect cannot abort a
measurement run. STEP round-trip tests that intentionally exercise build123d's importer continue to
use it; this decision does not redefine that third-party API.

## Evidence and compatibility

- a subprocess reproduction records exit 139 for the metadata importer on NIST AP242 CTC-02 while
  `import_step_geometry` loads 637 faces and one solid;
- files accepted by both readers retain type, solid count, face count, and ordered geometric face
  signatures;
- a two-solid compound retains both body-local solids;
- read, empty-transfer, null and unknown-topology failures are tested;
- root-transfer completeness is checked against OCCT's candidate roots, not solid count;
  a partially successful transfer is rejected before accessing the combined shape (#515);
- an architecture/source guard keeps corpus tools off the metadata-aware importer.

This adds a public operation without changing a recognition record, family, aggregate result, or
capability-manifest format. ADR 0005 still makes an additive public API a future minor-release
event. Epic #290 records the implementation on `main` but publishes no version beyond v0.4.12.

## Consequences

Recognition and its corpus runners survive valid geometry whose optional assembly metadata is
incomplete. Callers choosing the safe path knowingly lose metadata the package never used.
Flattening does not merge solids and therefore does not weaken compound ownership. A future fixed
metadata-aware importer may remain useful to consumers, but it does not make this smaller
geometry-only contract incorrect.
