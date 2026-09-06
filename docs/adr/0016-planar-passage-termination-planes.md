# ADR 0016 — Represent planar Passage terminations in the section frame

- **Status:** Accepted
- **Date:** 2026-09-02
- **Issue:** #453

## Context

A straight constant-section passage can terminate at two exterior stock faces that are neither
parallel to each other nor perpendicular to the passage run. The wall planes still prove one
unique run and one constant intrinsic section. The accepted `SectionPassage` v1 record cannot,
however, describe the physical ends: its two `run_interval` values reconstruct complete planes
perpendicular to `frame.run`.

For the authored wedge fixture, the six wall spans range from `[-11.212, 5.681]` to
`[-8.788, 9.319]`. Using their envelope includes exterior air near each sloped end; using their
intersection truncates real passage walls. Neither is the physical occurrence, and a corpus-tuned
angle or interval tolerance cannot make it one.

## Decision

Schema v2 retains the canonical run frame and intrinsic section. `run_interval = (low, high)` is
the pair of end intersections on the section-centroid run line. `PassageEnds` additionally carries
two local planar gradients. At section coordinate `(x, y)`, the physical open interval is

```text
low  + low_gradient.u  * x + low_gradient.v  * y <= t
t <= high + high_gradient.u * x + high_gradient.v * y
```

The existing flat case has both gradients `(0, 0)` and therefore keeps its exact v1 geometry.
Schema v2 permits four decimal places for intrinsic section points: three-decimal input remains
valid and existing exactly representable values do not change, while the extra digit prevents a
steep termination gradient from amplifying section rounding beyond the established `0.002 mm`
whole-occurrence displacement bound. The gradients are dimensionless, serialized to six decimal
places, and are part of the immutable public record. The producer must prove that the low plane remains strictly below the
high plane over the whole section and that the clipped prism is empty and open through both
termination planes. Recognition refuses a termination plane parallel to the run, a curved or
nonplanar termination, crossing end planes, and any wall set that does not independently prove one
unique straight run and constant section.

Opening stock faces remain consulted termination context. Their identity is not published and
they are neither defining nor constituent evidence. Mouth geometry does not choose the run.

This also applies when both stock planes are parallel to each other but oblique to the
wall-proved run. Parallel stock normals do not establish a perpendicular passage. Such
occurrences use the same planar-end proof, with equal nonzero local end gradients, rather
than the perpendicular-end fast path. No extra record variant or schema version is required.

## Consequences

The record describes the complete clipped passage rather than an envelope or a common-core
approximation. Local gradients rotate and translate with the canonical frame without depending on
world axes. Existing callers that only encounter flat passages observe unchanged interval
semantics; capability negotiation exposes the `PassageEnds` and `SectionPassage` schema-v2 change
before a consumer accepts sloped records.

Draftwright currently treats every `SectionPassage` as an explicit unsupported requirement and
has no IR converter. Its flat mouth-correlation path remains unchanged. To correlate a sloped
principal mouth, it need only evaluate the corresponding local end equation; until then it may
fail closed to its existing generic unsupported-profile warning without emitting a false
dimension.

This decision extends the existing Passage record rather than creating a second physical Passage
authority. ADRs 0002, 0003, 0007, 0008, 0009, 0010 and 0011 otherwise remain in force.
