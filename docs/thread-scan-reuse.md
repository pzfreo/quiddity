# Reuse thread scan indices and exact axis projections

Thread discovery repeatedly scans unrelated owners' edges and rebuilds each axis's transverse coordinates. I index eligible edges and axes by owner in the original order, reuse exact coordinate operands, and evaluate the existing short-angle rejection early only when rounded coordinate bounds prove that every sample stays in one strict quadrant. The helix fit, winding refusal, sample grid and thresholds remain unchanged.

The directed regressions compare the affected computations with their original
paths, including refusal and boundary cases. They do not replace full workspace
regression or strict snapshot reproduction on the standard ARM macOS runner.
No expected snapshots, reference bands or inventory floors are changed.

## Evaluating the existing angular rejection earlier

The next candidate precomputes bounds of the existing edge samples. It never uses
the box as a thread measurement or substitutes sampled geometry for source curves.
For a candidate axis, the same rounded subtraction and `geom::dot` operations bound
the transverse coordinates: select each endpoint according to the fixed coefficient's
sign. Rounded addition and multiplication are monotone. Non-finite bounds or ranges
crossing zero retain the complete path.

If both transverse coordinates keep a strict sign, every sample's `atan2` lies in
one quadrant. Successive angles differ by at most a quarter turn, so the quotient
in `nearest_turn` lies between 1/4 and 3/4 and its floor is zero. The last unwrapped
angle is therefore its ordinary endpoint angle. The candidate evaluates the same
endpoint difference and skips the pair only when it satisfies the original
`abs(da) < PI * 0.5` rejection. At equality it retains the full calculation. Such a
sequence also cannot satisfy the existing `TAU * 1.99` winding test. Both routes
therefore produce exactly the same absence of a helix or winding refusal.

The argument uses the documented quadrant ranges of Rust's
[`f64::atan2`](https://doc.rust-lang.org/std/primitive.f64.html#method.atan2), not a
claimed cross-platform precision for that function. Runtime comparisons remain
necessary on each supported build. Regression tests check coordinate enclosure,
the shortcut against both complete predicates, reversed traversal, full helices,
the rounded quarter-turn boundary, signed zero, overflow and non-finite samples.
Full strict snapshot and workspace regression remain separate acceptance gates.
