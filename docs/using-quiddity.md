# Using Quiddity

Three normal ways in, in the order most callers want them. Every example below runs as written
against the installed package, except the last, which names a STEP file you would supply.

If you are reading the capability manifest or the individual `recognise_*` functions first, you
have probably started one level too low — see [Expert operations](#expert-operations) for when
that is the right call.

## 1. Recognise a complete part

One call, one immutable result. This is the way to interpret a whole part, because the aggregate
runs every family in dependency order and *reconciles* them: where two families describe the same
void, a named rule decides which record survives.

Use `build_framed_recognition_result`. It recognises in an inferred local frame and hands back
the frame and the working part alongside the records, which is what a CAD application needs to
map a record onto the shape it is holding:

```python
from build123d import Box, Pos

import quiddity

part = Box(80, 50, 20) - Pos(0, 0, 6) * Box(30, 12, 12)
framed = quiddity.build_framed_recognition_result(part)

result = framed.result
print(len(result.section_recesses))  # constant-section recesses, unified
print(len(result.step_levels))  # horizontal face levels
print(len(result.plates))
```

Retain `framed.frame` and `framed.part` while you consume `framed.result`; the records are
expressed in that frame.

`build_recognition_result(part)` returns the same records in caller coordinates without the
frame. It is a compatibility name -- fine when you deliberately want world coordinates, and used
in the rest of this guide's examples to keep them short, but not the route to reach for first.

`RecognitionResult` is a frozen dataclass with one tuple field per family. Fields are empty when
the part has none — empty means *confidently absent* within the family's documented domain, not
"unsupported", which is reported as diagnostics instead.

The field you want is not always the one named after the feature you have in mind. Constant-
section voids — passages, prismatic pockets, edge-open recesses — are published together as
`section_recesses`, so a caller looking for a `passages` field will not find one. See
[`migration-0.4.md`](migration-0.4.md) for why, and what reconciliation does to a passage that
coincides with a slot.

## 2. Recognise with source-face evidence

When you need to know *which faces* established a feature — to highlight them, to drive a
selection, or to prove provenance — use the evidence lifecycle. It gives you the result, the
accepted occurrences, the report, and the face association from one run.

```python
from build123d import Box, Pos

from quiddity.evidence import build_recognition_evidence

part = Box(80, 50, 20) - Pos(0, 0, 6) * Box(30, 12, 12)
evidence = build_recognition_evidence(part)

for feature in evidence.features:
    record = evidence.record(feature)
    print(evidence.family(feature), type(record).__name__, len(evidence.defining_faces(feature)))
```

`defining_faces` returns only the faces that *establish* the feature — a slot's two walls, a
hole's bore patches. Faces the recogniser merely consulted (floors, caps, neighbours, stock) are
context and are deliberately not claimed; `constituent_faces` is the wider membership where a
family has one.

`quiddity.build_framed_recognition_evidence` is the ordinary part-relative lifecycle and the one
to prefer, matching level 1's framed route. Note the import: it is exported from the package
root, not from `quiddity.evidence`, which holds only the raw caller-coordinate
`build_recognition_evidence` shown above.

The write side of evidence — `ClaimLedger`, `EvidenceWriter`, `EvidenceSink` — is private and
carries no compatibility promise. No public recogniser accepts one, by
[ADR 0002](adr/0002-uniform-deterministic-recogniser-contract.md). Reading evidence is what a
consumer needs, and that is this API.

## 3. Inspect local geometry

For a single face, without interpreting a whole part, the narrow `quiddity.inspection` surface
publishes the consumer-proven geometry reads.

```python
from build123d import Box, Pos

from quiddity.inspection import inspect_face

part = Box(80, 50, 20) - Pos(0, 0, 6) * Box(30, 12, 12)
face = part.faces().sort_by()[0]

inspected = inspect_face(face)
print(inspected.surface.kind)  # SurfaceKind.PLANE
```

This is deliberately small — five published operations, per
[ADR 0005](adr/0005-versioned-cross-repository-capability-contract.md), which supersedes ADR 0010.
Graph identity, adjacency and blend collapse stay private.

## Expert operations

The individual `recognise_*` functions remain supported, and they are the right tool when you are
deliberately examining **one family**, or a shape the aggregate would interpret differently.

```python
from build123d import Box, Pos

from quiddity.passages import recognise_section_passages

part = Box(80, 50, 20) - Pos(-25, 0, 0) * Box(200, 10, 10)
rings = recognise_section_passages(part)  # every uncapped ring, unreconciled
```

They are *not* the way to assemble a complete interpretation of one part. Calling several and
combining the results yourself skips reconciliation, so you will get the same void reported twice
by two families that each describe it correctly. That is what
`build_recognition_result` exists to resolve.

Two further differences worth knowing before you compare a standalone call to a run:

- **Predecessors are not injected.** `recognise_holes(part)` reports no countersinks; a run
  passes it the completed countersink family, and passing `csinks=` yourself reproduces that.
- **An entry point is not always the standalone equivalent of its family.** It may be broader
  than the family's discovery (`recognise_face_levels`), or a view over a whole completed run
  (`recognise_section_recesses`).

## Starting from a STEP file

The examples above build parts inline for brevity. From a file, read it with the package's
geometry-only reader and pass the shape to any of the three levels:

```python
import quiddity

part = quiddity.import_step_geometry("bracket.step")
```

## Determinism

Every recogniser is deterministic with respect to equivalent input geometry and its configured
tolerance, and no recogniser's *discovery* calls a sibling. (A view over a completed run, such as
`recognise_section_recesses`, necessarily runs them all -- that is what makes it a view.) The same part gives the same records in the same
order, run to run and machine to machine.
