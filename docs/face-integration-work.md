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

## Faces with one very large B-spline net

A face whose B-spline net has many knot spans in u and few in v (a threaded part's
helical flank, a sweep along a spline: 300 to 800 spans against one to four), or a
cylinder carrying a helical thread edge, integrates along u for its inner integral at
every boundary node, so the work grew with the spans (or radians) times the nodes (#302).
These changes remove that, none touching the integrands, nodes or the boundary quadrature:

- An open, unpinched face with at least 32 u spans and eight times v's takes the other
  form of Green's theorem, the inner integral along v from the loop's first point, whose
  few spans each cost one 16-point panel. Each panel's rule is compared with the same
  rule on its halves, to 1e-9 of its terms' magnitude in each component (and 1e-12 of the
  largest component's); a face that fails the comparison anywhere, or that this form
  refuses or finds invalid, is integrated again by the stock form, so only a completed,
  checked result is used. A loop's direction still reads the swept parameter area.
  xu9299 independently found the same approach (the other Green's-theorem form) and the
  narrow-dip counterexample in #302.
  xu9299 also found that the stationary-node test must judge motions by the active
  parameter ranges (`domain()`), not the knot vector's end values, which a writer may pad.
- A cylinder or cone whose loops (closed in the lifted parameters, not winding round u) run
  through at least 32 radians of u, as a thread's helical edge does, takes the same form:
  along u the inner integral costs a panel per radian at every boundary node, along v one
  panel. The same check and fallback apply.
- On a face whose inner integral's parameter has many spans (the same threshold: a face
  closed in v, whose loops wind round it, must keep integrating along u), a node that
  moves along its surface's own parameter line, within 1e-12 of its motion across it
  (each motion taken as a share of the surface's active parameter range, `domain()`, so u
  and v of different scales are judged alike), weighs nothing and is not evaluated. This applies to the existing form as
  well as the new one: a closed B-spline face whose loops wind round u and whose v has many
  spans skips such nodes too. Locating the polynomial piece of a node for its
  velocity now searches the knots instead of rebuilding their distinct list (the same
  piece), which had cost one pass over the knots per node.

Results agree with the stock form to round-off (about 1e-10 mm in a recognised location).
`tests/face_moments.rs` and the face-area checks against OpenCascade are unchanged.

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

Recovery also requires `maximum_deviation * P / A <= 0.001`, using that **same
sampled maximum**, the physical length `P` of the master trim on the surface
(including the short UV closing segments), and its integrated area `A`. This
rejects long thin strips whose disagreement is small against the diagonal but
large against their width. Length uses bounded adaptive Gauss–Kronrod with a
face-relative error budget; its summed estimated length error is added to `P`.
The area already passes the recovery integrator's numerical criteria before
this guard runs, and neither mass nor moments are published before it passes.

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
moments escape, including concurrent first queries. An unresolved canonical legacy
quadrature retries only the eligible master boundary, without repeating legacy mass.
A completed but invalid canonical sum stays refused, as in a standalone mass query.
A failure evaluating additional moment fields before canonical mass was determined
still needs an independent canonical query; it cannot poison a valid mass cache.
### Export of source master flags

STEP export preserves the source `PCURVE_S1`/`PCURVE_S2` flag on raw master slots
of native B-spline and Bézier faces, even when no mass query or recovery has taken
place. A source `CURVE_3D` designation stays `CURVE_3D`. This deliberately changes
1,126 faces across the 11 flagged corpus files relative to the previous writer
on main, which always emitted `CURVE_3D` for them. Export copies retained source
metadata without integrating or consulting query-populated caches, preserving
identical bytes with and without prior mass queries.

The synthetic regression uses polynomial normal-offset edges on
`S(u,v)=(w*u+v, epsilon*v, v²)`, with independent one-dimensional references for
area, flux, centroid and central moments. The original narrow cases are rejected by the representation guards; a broad
patch retains a localized ill-conditioned projection near its boundary while
satisfying both bands. Endpoint-matching interior bulges also refuse. Neither reference bands
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
