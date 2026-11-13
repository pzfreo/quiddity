# Reducing work without changing face-integral decisions

This change retains the integration densities, quadrature nodes, operation order,
initial cuts, error criteria, depth limit and kink budget. It does not infer failure
from elapsed time, face size, spline degree or a large parent-panel error.

## Reusing work

`face_moments` stores its complete result in the immutable part's face cache, including
`None`; all moment components are retained. A transformed part starts with fresh
geometry caches, and sense repairs also replace the face caches. The mass
cache introduced by the existing shared moment/mass integration retains its own rule.

Within one face integral, the NURBS evaluator borrows one immutable surface and
retains each axis's last basis and derivative by the exact bits of its clamped
parameter. A changed span pair rebuilds the homogeneous control block; non-finite
parameters use the uncached path. Reuse preserves u/v/component accumulation order,
quadrature nodes, convergence criteria and refusal conditions. These operands are
discarded with the integral.

An adaptive panel requests kink locations only if there is a refinement floor, its
depth is below `MAX_DEPTH`, and its interval still has kink budget. Otherwise the
location cannot affect the next step. All quadrature nodes and the walking state are
still evaluated. The discarded corner search has local state, cannot change the walk,
and does not propagate its projection failures. The initial boundary cuts are untouched.

## A certificate for an inevitable refusal

This is a proof about the existing finite algorithm, not a proof that the geometric
area or moments do not exist, or that a different integrator could not compute them.
Gauss--Kronrod differences are numerical error estimates, not rigorous bounds on the
true integration error.

For one requested density, the coarse pass fixes
`F = 1e-11 * sum(coarse term magnitudes)`. It is not recomputed during refinement.
For an accepted leaf with estimated error `e` and magnitude `s`, the original
adaptive walk updates its unresolved excess as follows:

```
if !(e <= max(1e-9 * s, 0)):
    E = max(E, e)
```

`E` starts at zero. Boundary aggregation also takes a maximum, and the caller rejects
the refined loop when `E > F`. The face does not proceed to loop orientation or publish
an integral in that case. The mass and moment refinements each use their own fixed `F`.

Suppose an accepted leaf makes `E > F`. Every subsequent maximum is at least this `E`.
Thus either a later evaluation fails (the original returns `None`), or refinement
finishes with excess still greater than `F` (the original also returns `None`).
No continuation can produce a successful face result. The optimized walk may return
that rejected prefix; the boundary propagates it immediately, and the unchanged
caller applies the same rejection. In particular, the canonical mass-refinement
caller records the canonical legacy refusal before considering an explicitly
selected master trim. No rejected prefix is published.

This does not reject a parent that can still be subdivided. With ordinary finite
values, an accepted leaf that fails both the relative criterion and `F` was accepted
only because it reached `MAX_DEPTH`. A large error accepted by the relative criterion
does not enter `E`. Equality with `F` is not refusal. With an infinite or NaN floor the
ordered `E > F` guard cannot trigger. Once it does trigger, Rust's `f64::max` cannot
reduce the non-NaN `E`, even if a later estimate is NaN. Evaluation failures remain
failures; no new numerical answer is invented.

## Recovery from an explicit master trim

A refused 3D projection may retry one complete, explicit STEP master-pcurve loop
on a native, nonperiodic, noncollapsed B-spline or Bézier surface. Successful legacy
integrals retain their operation order and values. Near parallel surface tangents
can make Gauss–Newton oscillate even with small position residuals; using the file's
selected trim supplies positions and derivatives without that ill-conditioned solve.
Recovered faces measure the region bounded by the **file's master pcurves**. This
can differ from a 3D-edge answer by the file's own representation disagreement.

Each edge needs exactly one master pcurve, finite controls within the surface
domain, and matching endpoints within the file's stated precision plus the kernel
coordinate floor. Recovery additionally samples `|S(pc(t)) - C3d(t)|` at 33 equally
spaced parameters (including endpoints) on **every span in the union of the 2D and
3D curves' knot breaks**. Nonfinite distances or a sampled maximum greater than
`0.001 * L` refuse recovery. Here `L` is the diagonal of this trimmed face's
`Part::face_bounds` axis-aligned box, in millimetres, with no unit-size floor. This
is the bounding-box length normalization used for GVP centroid evidence; tensor
comparisons separately normalize by the reference inertia trace. The interior
check bounds sampled disagreement, not an unsampled mathematical supremum. It
uses the existing GVP band, not STEP's usually much tighter declared precision.
Endpoint and small-gap connectivity checks retain their original precision band.

A known recall limit is that a NURBS pcurve's **whole knot domain must equal the
edge interval**: a valid subinterval trim is still ineligible. Unsupported,
missing, ambiguous or unresolved master trims remain unavailable.

The outer rule retains its coarse `F = 1e-11 * magnitude`, `1e-9` local criterion,
`MAX_DEPTH = 12`, and kink budget. Inner adaptive primitives sum their error
estimates, use the same depth bound, and receive half of F through a conservative
pcurve variation bound. Their explicitly node-paired weighted errors enter each
outer panel's estimate. Unresolved integrals still refuse. These are numerical
error estimates, as in the legacy walk, rather than rigorous true-error bounds.

Canonical mass and boundary choice are published under one once-cell before any
moments escape, including concurrent first queries. A known canonical legacy
refusal retries only the eligible master boundary, without repeating legacy mass.
A failure evaluating additional moment fields before canonical mass was determined
still needs an independent canonical query; it cannot poison a valid mass cache.
Export copies the retained source master flags directly, without integrating or
consulting query-populated caches. Thus writing retains the authority recovery
needs and produces identical bytes with and without prior mass queries.

The synthetic regression uses polynomial normal-offset edges on
`S(u,v)=(w*u+v, epsilon*v, v²)`, with independent one-dimensional references for
area, flux, centroid and central moments. The original narrow case is now rejected
by the interior band; a wider case retains the ill-conditioned projection inside
the band. Endpoint-matching interior bulges also refuse. Neither reference bands
nor quadrature tolerances are enlarged (ADR 0008).

## Settled volume queries

A volume pass refuses as soon as a sampled line is unresolved. Later samples
cannot restore an answer; placeholder zeros only finish discarded integration
work and must never be published as material or air. Successful passes retain
their sampling and accumulation order.

For `fills_at_most`, a material prefix can stop the inset pass when `limit < 1`,
even before air is found; otherwise both material and air are needed. The exact
pass can reject once `material / whole > limit`: lengths are nonnegative and
quadrature weights positive, so its running total cannot decrease. Equality does
not reject. Acceptance requires finishing any shortened inset pass, preserving
the empty/full conventions and refusing unanswered lines. An unknown or
nonpositive whole volume also refuses the at-most query.

## Verification obligations

The tests compare exhaustive and early walks, including accepted sums and end states,
ascending/descending intervals, a bad parent resolved by subdivision, acceptance by
the relative criterion, adjacent floating-point values around the floor, non-finite
floors and exhaustion of the depth budget. A rejected prefix is explicitly shown to
have an incomplete sum, so callers must check its excess before using that sum.
The existing quadrature and kink-budget tests retain exhaustive mode.

Independent face-area/moment references, placements, existing recognition snapshots,
inventory floors and tolerances remain unchanged. Repeated-moment speedups measure
cache reuse; they are not claims about cold integrations or whole-part recognition.
