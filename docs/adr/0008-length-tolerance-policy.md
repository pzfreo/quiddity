# ADR 0008 — Length tolerance policy

- **Status:** Accepted
- **Date:** 2026-08-16
- **Package review:** [epic 0001](../epics/0001-review-remediation.md), finding 2
- **Record form:** current state, rewritten 2026-09-15. The per-site classification tables and
  per-issue amendments this file carried until then are in
  `git log -- docs/adr/0008-length-tolerance-policy.md`. The current site inventory is the
  code, and its audit is issue #611.

## Context

Recognition gates were written in fixed millimetres, so a 2 mm part and a 2 m weldment were
judged by the same band and an inch-authored model by a band 25× tighter than intended. Two
measurements ruled out a uniform scaling pass: the fixtures span a factor of six with no single
reference scale, and about half the constants counted were not lengths at all. Scaling six
minimum-size gates to the part in 0.2.3 lost records on every affected real part and gained none;
0.2.4 reverted them. The policy below is what survived.

## Decision

**One tolerance form.** `_geometry.length_tol(nominal, *, rel, floor) = rel * nominal + floor`.
`rel` grows with the thing measured; `floor` is the noise band below which two coordinates are
one coordinate. The terms add rather than taking a maximum, so a small feature's own size keeps
mattering. A negative nominal raises; a NaN nominal propagates and the candidate fails closed.
`_geometry.part_scale(bbox)` is the reference length for gates with no smaller feature to hand.

**Nominal is the smallest geometry that decides the comparison, not the part.** A diameter match
scales with that diameter; an axial gap between coaxial bands with their diameter; a coordinate
merge with no local feature scales with `part_scale`. A 0.5 mm merge band is right for a 3 mm
hole in a 500 mm plate; scaled to the plate it merges the hole into its neighbour.

**A tolerance is not a threshold.** A tolerance asks *are these two things the same?* and scales
with what it compares. A minimum-evidence threshold asks *is this big enough to be a feature?*
and is absolute: scaling it makes a feature's existence depend on its surroundings, and decides
significance inside recognition, which [ADR 0001](0001-standalone-geometry-only-apache-library.md)
assigns to consumers. A threshold calibrated to what one corpus thought worth dimensioning is not
a geometric boundary at all; the chamfer minimum leg was removed on that ground and the caller's
explicit `tol=` remains the only such floor.

**Dimensionless quantities never scale.** Ratios, fractions, unit-vector dot products, angles,
counts and float epsilons are already scale-free. A name ending in `_TOL` does not make a value a
length; only the comparison it sits in does.

**An absolute constant is legal only when justified and bounded.** It must model a physical
process, with a comment naming it (an edge break, a deburr, an end treatment), and be bounded so
it cannot swamp a small feature: paired with a proportional term, or capped against the feature it
applies to (`min(pad, band_width / 2)`, `levels.bounded_end_margin`). An unbounded absolute
constant is a defect whatever its justification.

**Exact ties refuse.** A strict bound uses `_geometry.clears_threshold`, so a span must clear a
maximum on its meaningful side; reconstruction noise on either side of a tie is not acceptance.

**Kernel floors are not allowances.** `_geometry.COORD_FLOOR` (1e-6 model units) is the smallest
separation at which the package asks OCCT to distinguish coincident geometry, used to inset probes
and compare coordinates of one topological boundary. A material test still requires exactly zero
volume. Numerical conditioning (factoring a known shared root out of an intersection test) is
never a reason to widen a recognition or publication allowance.

**Same-geometry certificates are tolerances at the local scale.** Analytic recovery requests
`1e-6 * local + COORD_FLOOR` with `local = min(sqrt(A), 2A/P)` over the original face; native
analytic equivalence and blend-chain equality use `1e-9 * local + COORD_FLOOR` and
`1 - abs(dot) <= 1e-9` for axes; smooth-side curvature is normalised by a local length with a
`1e-6` gap. World bounds, rounded records and fitted radii are forbidden authorities for these.

**Publication is separate from recognition.** Record rounding (`round(x, 3)`, `FLOAT_DIGITS`) is
the public record contract, not a tolerance, and never admits a candidate. Section publication
reconstructs the occurrence from its serialised basis and refuses when any boundary point moves
by more than 0.002 mm; that bound is a publication check, not a discovery allowance.

**Scope, not thresholds, is body-local.** Area and span denominators are taken from the candidate
face's own valid solid, so a foreign body in a compound can neither suppress a feature nor bridge
air. The dimensionless fractions themselves are unchanged by that scoping.

## Enforced by

- `tests/test_scale_invariance.py`: families gated only by proportional tolerances are invariant
  across 0.05×–100×; the exclusion list names the families gated by an absolute minimum, which
  are *correctly* not invariant.
- Golden fixtures at reference scale, reviewed cell by cell when a gate moves.
- Review: a new absolute constant without a named physical justification and a bound is a defect.

## Consequences

Public `tol=` keywords default to `None` and resolve to the derived value; a float keeps its
meaning. No record schema changed. Parts far from the fixture corpus's scale classify
differently, which is the point. Grep cannot tell a threshold from a tolerance; classifying a site
means reading its comparison, which is why the site audit lives in an issue rather than here.
