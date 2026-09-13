# Native cylindrical seats as open-arc section channels

Issue [#594](https://github.com/pzfreo/quiddity/issues/594).
Baseline: merged #593 repair, `e122302b864b46412014a4199d70ab7e956a8b4e`
(Quiddity 0.2.8).

## Geometry contract

A seat is represented by the existing `SectionRecess` record: a `channel` with
`section_shape="circular"`, a two-vertex `OpenSectionProfile`, and two planar open
ends. The physical boundary is one arc; the straight chord is explicitly absent.
The section frame and axial interval locate the occurrence. For chord length `c`
and nonzero bulge `b`, radius is `c * (1 + b*b) / (4 * abs(b))` and angular sweep
is `4 * atan(abs(b))`. No public fields or top-level family are added.

The bounded native proof requires:

- connected, concave native patches of one located cylindrical surface, owned by
  one valid solid; unrelated coaxial occurrences remain separate;
- convex junctions to original planar stock at both axial ends, and parallel
  longitudinal lips on one planar opening with a consistent exterior normal;
- an arc no larger than a semicircle, reconstructed from the original rim and
  the cylinder's radius, axis and opening direction;
- exact bidirectional area-union coverage between the original cylindrical
  patches and the reconstructed swept support;
- empty material volume throughout the swept circular segment and beyond both
  axial openings and the lateral mouth, using complete curved probes.

Only original cylindrical wall nodes become defining/constituent evidence.
Stock planes are context; generated probes never become evidence. Native angular
and axial seam splits keep all original patches in one occurrence. Tangent fillets,
convex cylinders, complete bores, capped recesses, broken support and obstructed
troughs fail this proof. Recovered spline trims, undercuts beyond 180°, curved or
slanted end conditions and mixed-support channels are outside this first slice.
This is an explicit native trim contract, recorded in the reader and module rosters.

## Publication

Open-profile points use their existing four-decimal allowance; bulges retain
12 decimals, frame directions six, and origin/axial coordinates three. A complete
arc displacement bound includes centre/radial-vector differences, sweep rounding,
frame displacement and end displacement, and must stay within the existing
0.002 mm whole-occurrence allowance. It does not sample only arc endpoints.

The closed-section projector's three-decimal endpoint rounding can noticeably
change a shallow arc's reconstructed radius. This open-profile projection uses
the already supported precision without changing the closed-section contract.
Consumers retain responsibility for drafting style and radius/diameter annotation.

## Supplied part

Fixture: `tests/fixtures/issue_1595_whistle_key_frame.step` from Draftwright branch
[`issue-1595-whistle-key-frame-fixture`](https://github.com/pzfreo/draftwright/tree/issue-1595-whistle-key-frame-fixture),
author-supplied from `pzfreo/whistle-key` at `7fb4d14`.
SHA-256: `8f060dfd4eeb4ff9589243f85eea8ca027784ac51ae98dc2d29a2371a2d115d0`.
The file is not copied into this repository.

The one-solid, 222-face part has three relevant original faces (indices 85, 107
and 114 in the supplied STEP). Their radius is 7.15 mm, sweep is
1.0976164074654 rad / 62.888787672°, and axial length is 6 mm. Their Y intervals
are `[-35.2975, -29.2975]`, `[29.2975, 35.2975]` and `[-10.9225, -4.9225]`.
All three satisfy exact support and all four empty-volume checks. The initial
reported semicircular span was inaccurate; the source trims determine the result.

## Authored validation

`tests/test_cylindrical_seats.py` has 37 authored cases covering 30°, 62.89°, 90°
and 180° arcs; 0.1×, 1× and 10× scale; rigid placement; raw and framed routes;
three distinct seats and exact original evidence; STEP round-trip; angular/axial
native seam subdivision; separate owning solids; full bores, convex cylinders,
blind cuts, broken support and convex/concave fillets; and the explicit undercut
and spline-trim refusal boundary.

A suspended rod attached outside the seat leaves its complete cylindrical support
unchanged but places material inside the trough. It is refused, establishing why
support equality alone is insufficient.

## Vendored corpus comparison

All 89 vendored STEP files complete without errors; 82 documents are identical.
The seven changed documents add these circular-channel occurrences:

| Part | Section occurrences before → after | New channel geometry |
| --- | --- | --- |
| MFCAD++ 1000 | 8 → 9 | One R1 semicircle, length 12.593 |
| MFCAD++ 10007 | 1 → 3 | Two R≈3.744 arcs, 12.30° and 27.88°, length 18.961 |
| Vendored MFCAD++ 408 | 3 → 4 | One R2 semicircle, length 10.457 |
| Vendored MFCAD++ 498 | 3 → 4 | One R≈1.258 semicircle, length 37.071 |
| NIST CTC-02 | 6 → 8 | Two R60 / 108.63° channels, length 40 |
| NIST FTC-08 | 4 → 6 | Two R5.08 semicircles, length 3.429 |
| NIST FTC-09 | 7 → 11 | Four R6.35 semicircles, length 3.038 |

Other physical-family records are unchanged. Associated-face counts rise by
1, 2, 1, 1, 0, 2 and 4 respectively. The CTC-02 faces were already associated;
new occurrence evidence does not imply new face coverage. Native section indices
may move when new occurrences are inserted. These are observed geometry changes,
not an independently labelled seat-accuracy score.

The unchanged v13 taxonomy on the 40-model MFCAD++ development subset reports
three additional section records (56 → 59), all unmapped by its current vocabulary.
No mapped-class record counts or defining-face recalls improve. Face coverage
rises from 20/22 to 22/22 for class 1 and from 0/1 to 1/1 for class 7. Broad pocket
precision denominators increase for classes 14/16/18/22, so their reported precision
falls; taxonomy-mismatch defining faces rise from 265 to 267. The taxonomy was
not edited to improve these results. No full-2,500-model or MFInstSeg result is
claimed; no MFInstSeg models were inspected.

## Query-cost sentinel

New discovery adds 74 cached `shared_occurrences` queries on NIST CTC-02:
3,138 → 3,212. Independently measured shape comparisons under that method remain
exactly 1,494 on baseline and repair; comparisons per call fall from 0.4761 to
0.4651. The sentinel denominator is updated to the measured workload, while its
comparison count and no-rescanning requirement remain intact.

## Independent review

No actionable correctness findings remain in the bounded implementation.
The reviewer independently reconstructed published arcs in 3-D under additional
rotations/scales and measured a maximum 0.00089 mm displacement from original
cylindrical faces, within the 0.002 mm bound. Arbitrarily rotated 100× cases can
reach conservative publication refusal; scale support is bounded by that existing
absolute publication contract, not an unrestricted promise.

## Final checks

- Full local suite: **7,875 passed**, **95.17%** line/branch coverage; the unchanged
  91% gate passes. Ruff lint/formatting and mypy (96 source files) pass.
- Raw and framed supplied-fixture checks both return three seats and preserve the
  six separate Ø1.1 × 2.7 bores from #593.
- Runtime minimum of three: composite **0.966 s** against **1.191 s**, census
  **12.492 s** against **13.189 s**. Both fixed budgets pass. Baseline composite
  in the paired window was 0.935 s; no budget was raised.
- Final 89-part documents and 40-model scorer outputs match the compatibility
  comparison above (scorer runtime excluded).

Environment: Python 3.14.7, build123d 0.11.1, OCP 7.9.3.1, macOS arm64.
Source SHA-256 values:

- `_cylindrical_seats.py`: `74caa78f24c49a678a786cb2543e8b9ca33e626de14aeb3034674741daaef9ff`

- `_section_recess_geometry.py`: `3b42fa5ea049cf29d9ed732162d4ed0f4402596c2f07af36e9cade8791b606c0`
