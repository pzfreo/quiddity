# Reducing work without changing face-integral decisions

The original work-reuse optimizations retained the integration densities, quadrature nodes, operation order,
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

## Choosing the STEP boundary for measurement

A STEP surface curve can provide several descriptions of one edge. Its master
representation selects the spatial curve or a numbered pcurve. Public haecceity mass
properties use those selections for face areas, fluxes and moments and their shell and
solid sums. Recognition uses the master boundary only when a selected pcurve deviates
from its 3D edge beyond the face's STEP uncertainty. Otherwise it integrates the
projected 3D boundary in caller coordinates, including under an integration budget.

A pcurve is evaluated in the units and parameter interval of its named carrier.
If that carrier differs from the face being measured, the point and tangent
are transferred through 3D onto the receiving surface. Endpoint agreement,
finite carrier domains, intersection and integration checks govern acceptance.
If the assembled loop fails, measurement retries the whole face using the 3D
edges. Mass and moments share the accepted choice across query orders.

The selected boundary goes through the same integration as the projected one: the
cheaper along-v form and its retry for a face with one very large net, the per-face
integration budget (integrand, deviation sample and selected-boundary gap evaluations spend work),
and the shared moment integration. A remembered refused along-v trial is keyed by the
integrand's width, whether mass is observed and whether the loops are the selected trim,
since the two boundaries are different loops. The choice is parameter-space data (each
selection's pcurve and interval, accepted by distances in millimetres), so a rigid
motion leaves it unchanged: carrying a face's integrals through a motion carries the
boundary choice with its mass, and the moved part's refusals keep their boundary bit.
Where the moment group cannot be integrated against world coordinates (a small selected
trim far from the origin), it is integrated once more against the B-spline net
translated to the face's moment origin, over the same boundary and error test.

## Following the selected boundary in the mesh

The mesher samples each topological edge once for all its incident faces. It
prefers a geometrically qualified selected trim on the carrier face,
then checks the other incident faces for that same selection. Every receiving
face must be able to resolve the selected trim on its own surface. An edge
without compatible receiving trims or a sampleable selection uses
its existing spatial samples. The faces map those common points to their own
parameter charts. On a closed spline, an inverse stopped at the seam also
tries the other end of the chart.
If a selected loop still refuses triangulation after the usual thinning
retries, the mesher retries with spatial edges for that loop's face. Every
incident face uses the same replacement samples on those edges.
This is one compatibility retry, not proof that the selected geometry was
invalid: a qualified selected loop may also exceed the mesher's capabilities.
Its replacement can differ from the boundary used by the face integral. The
mesher does not recompute or change the accepted mass boundary. If the retry
also fails, the existing `MeshRefusal` is returned.

Ordinary boundary selection reads the existing geometric boundary cache without
requesting a face integral or changing the accepted mass-boundary decision.
Each mesh call constructs its own shared edge plans and parameter charts. A
boundary fallback rebuilds all charts and plans; this version deliberately
keeps the original retry and interior-refinement order. Further work reuse and
specialised evaluators are separate performance changes.

The mesh keeps the shared STEP vertices at the ends of an edge. An unresolved
run of samples next to both the STEP vertex and the selected curve's true end
does not add a second boundary key there.

A selected spherical cap can also be triangulated in a central projection.
All its boundary points must lie well inside one hemisphere and its measured
area must fit in a hemisphere. In that chart, the signed planar area of a
triangle has the same sign as the determinant of the three spatial directions
from the sphere centre. Refinement places points back on the same sphere.
The mesh still reports the surface's usual parameters and original shared
boundary positions.

This chart also explains most of cgb202's triangle-count reduction. With the
same Windows build and tolerances, the original selected-boundary implementation
has 259,936 triangles versus 272,196 at `6e56fd77`: spherical faces account for
a reduction of 12,598, offset by 338 additional triangles elsewhere. Disabling
only the central projection increases the selected-boundary result to 279,944;
only 174 spherical faces change, adding 10,004 interior vertices while keeping
their shared spatial boundary points identical. Non-spherical faces are
unchanged in this ablation. A spherical pole can retain a different equivalent
longitude in the reported UV coordinates. The scoped sampling rule below
gives 260,290 triangles on Windows. Of the 354 added relative to the original
PR, spherical faces account for 187, freeform faces for 150, cylinders for 16
and tori for one. All versions retain the single refusal at face 399. Exact
totals can differ slightly between Windows and ARM macOS.

### Separate tessellation samples from integration panels

Integration partitions a selected trim at spline knots and transferred carrier
knots. Those panels are not uniform parameter stations, even when the spatial
curve is smooth. Reusing their samples in the mesh planner can stagger the two
sides of a narrow freeform ribbon and introduce folded triangles.

The mesh planner uses 256 uniform parameter segments before chord refinement
when every incident face is freeform, or when a non-freeform carrier supplies
the selected trim to a freeform face. It uses the surface that owns the pcurve,
including the source surface of a transferred trim. Each of the six reported
ribbons shares a cylinder-owned edge with a cylindrical face. Freeform-owned
edges with an analytic neighbour and edges with only analytic incident faces
use the original knot-based stations. All incident faces use one shared
spatial edge plan. Integration accesses its own knot partitions and samples.

The focused whole-part regression checks the six narrow faces of
`threaded_connector_109` independently of the corpus perimeter allowance: each
must be within 1% of its integral area and drop no folded triangles. On two
other faces of this part (16 and 138), the regression checks winding against
the surface normals. They retain their known area errors. In a 192-segment
trial these two faces gained more than 3,000 inward triangles. The cgb243
test still checks both single-face and whole-part calls against its independent
area reference. On Windows both calls give 475 triangles and 0.877668% area
error. With 128 segments on every selected edge, #50870 and #60095 gained
two and five inward triangles respectively. The scoped rule preserves outward
orientation on both. On #60132 itself, eight retained triangles face inward
under the corpus normal check and two folded triangles were dropped. With the
original placement and boundary fixed, the first inversion appeared in the
second refinement round when an interior midpoint was placed on the surface.
The two 37-degree placement checks give 0.787233% and 0.760832% with recomputed
default deflections and 0.877683% and 0.926941% at the original deflection.
Across 72 additional placements the largest area error was 0.889194% at the
recomputed deflection. A separate set of 128 rotations gave a maximum of 0.887017%.
With 128 starting segments the corresponding maxima were 1.010172% and
0.996699%. These figures apply to this fixture and these deflections.

In eight paired Linux x86-64 rounds, one STEP read and one whole-part mesh
used 3.906, 2.023 and 1.551 times the retired user-space instructions of the
spatial-edge baseline (`6e56fd77`) on cgb243, cgb202 and string_post. The
counter followed the worker thread from before the read through bounds, meshing
and release. Increasing the uniform seed from 128 to 256 segments added 2.41%
on cgb243 and 0.77% on cgb202 in that run. An earlier run of the original PR
with the same inputs and counter gave baseline ratios of 5.152, 2.003 and
1.519 respectively. The Windows resource tables below precede the sampling
change.

## The native spline integration rule introduced in #281

The original #281 fix tried this specialised integral after a refused projection.
The current selector tries it first when a native, nonperiodic, noncollapsed
B-spline or Bézier face has one complete, explicit master-pcurve loop. The
general boundary integrator handles mixed representations and other face types.
Near parallel surface tangents
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

The work-reuse tests retain their independent face-area and moment references,
placement checks, inventory floors and tolerances. Repeated-moment speedups measure
cache reuse; they are not claims about cold integrations or whole-part recognition.


## Selected-mesh resource measurements

Inputs are from `pzfreo/quiddity-corpus` at `3ef06590a7a27ed65ca1f1a6e7f7ea4d5c328f1f`. Each Part is read with `haecceity::read_step_file`. Both timed calls use `part.mesh(deflection, 0.3)`, with `deflection = 0.001 * diagonal` for a finite positive `part.bounds().diagonal()` (otherwise `0.001`). Bounds and deflection setup occur before timing. The first mesh remains alive during the second timed call for exact output comparison; both are checked and released outside timing. The separate memory probe drops each mesh before the next call.

| Input | Corpus-relative file | Paired timing rounds |
| --- | --- | --- |
| cgb245 | `cadgenbench_inputs/cgb245.step` | 30 |
| string_post | `gramel/string_post.step` | 30 |
| cgb243 | `cadgenbench_inputs/cgb243.step.gz` | 30 |
| cgb202 | `cadgenbench_inputs/cgb202.step.gz` | 10 |
| cgb203 | `cadgenbench_inputs/cgb203.step` | 10 |
| cgb207 | `cadgenbench_inputs/cgb207.step` | 10 |
| cgb217 | `cadgenbench_inputs/cgb217.step.gz` | 10 |
| cgb242 | `cadgenbench_inputs/cgb242.step.gz` | 10 |

### Resource comparison: Use STEP-selected boundaries for face meshing

Windows/i7-11800H, Rust 1.99.1/MSVC release, x86-64-v3, one pinned CPU. A single balanced five-version comparison used 30 paired rounds for cgb245, string_post and cgb243, and ten for each other input. Every version used a fresh Part and two mesh calls; import and deflection setup were outside mesh timing, with no warm-up or discarded rounds. Ratios below 1 are faster; intervals are paired 95% bootstrap intervals (5,000 samples, seed 317). The comparison is against merged #317.

| Input | First-call ratio [95% CI] | Repeated-call ratio [95% CI] |
| --- | --- | --- |
| cgb245 | 2.750 [2.684, 2.813] | 1.390 [1.358, 1.417] |
| string_post | 2.167 [2.108, 2.218] | 1.296 [1.268, 1.328] |
| cgb243 | 7.878 [7.793, 7.960] | 4.498 [4.452, 4.539] |
| cgb202 | 2.818 [2.782, 2.853] | 1.071 [1.056, 1.083] |
| cgb203 | 1.077 [1.055, 1.098] | 1.025 [1.009, 1.038] |
| cgb207 | 1.839 [1.726, 1.924] | 1.119 [1.091, 1.144] |
| cgb217 | 1.340 [1.296, 1.383] | 1.032 [0.995, 1.064] |
| cgb242 | 1.929 [1.874, 1.978] | 1.033 [1.014, 1.056] |

| Input | Import ratio [95% CI] | Import plus first mesh ratio [95% CI] |
| --- | --- | --- |
| cgb245 | 1.031 [1.012, 1.049] | 1.481 [1.456, 1.505] |
| string_post | 1.007 [0.981, 1.031] | 1.563 [1.534, 1.592] |
| cgb243 | 1.019 [1.002, 1.038] | 5.350 [5.300, 5.398] |
| cgb202 | 1.019 [1.004, 1.033] | 2.026 [2.006, 2.046] |
| cgb203 | 0.948 [0.840, 1.047] | 1.046 [1.019, 1.072] |
| cgb207 | 0.879 [0.682, 1.024] | 1.493 [1.297, 1.626] |
| cgb217 | 1.022 [0.976, 1.073] | 1.206 [1.167, 1.246] |
| cgb242 | 1.001 [0.944, 1.054] | 1.583 [1.543, 1.622] |

Import plus first mesh excludes bounds/deflection setup, validation and teardown. It checks for work moved into preparation without claiming end-to-end application latency.

Eight-input memory measurements used five fresh processes per version/input and ten calls per process. Median peak private commit changed by +9.0% to +92.4%; cgb243 changed from 140.11 to 221.57 MiB. The correctness fix increases resource use; this is an observed implementation cost, not a lower bound on the cost of correct boundaries. Import plus first mesh is reported separately in the source documentation, including intervals crossing 1. These measurements are not application timings or corpus-wide resource bounds.

| Input | Peak private commit: preceding / current (MiB) | Peak working set: preceding / current (MiB) | Retained Rust increase: preceding / current (MiB) |
| --- | --- | --- | --- |
| cgb245 | 31.5000 / 60.6133 | 36.9414 / 64.6094 | 0.0000 / 17.4228 |
| string_post | 8.4766 / 12.8711 | 14.7578 / 18.9609 | 0.0000 / 2.2241 |
| cgb243 | 140.1094 / 221.5703 | 140.7148 / 219.9688 | 0.0000 / 48.6644 |
| cgb202 | 314.7812 / 427.1523 | 312.0820 / 421.0781 | 0.0000 / 80.3429 |
| cgb203 | 74.1289 / 80.7773 | 78.2891 / 84.7109 | 0.0000 / 5.3766 |
| cgb207 | 48.9102 / 67.7812 | 54.1133 / 72.4453 | 0.0000 / 12.9470 |
| cgb217 | 71.1250 / 103.0117 | 75.1641 / 105.4336 | 0.0000 / 20.9102 |
| cgb242 | 119.1289 / 179.7969 | 122.1211 / 181.6172 | 0.0000 / 43.0728 |

Process peaks cover import and meshing. Private commit is not resident physical memory; working set is reported separately. Rust counts measure requested allocation bytes. Retained allocation is the increase over the post-import, post-deflection-setup state, with the mesh dropped and the Part alive. Every process releases each mesh before the next call and drops the Part at the end. Retained Rust bytes did not grow over ten calls. Valid triangulations need not be bit-identical between stages; geometric regression tests retain their reference bands and fallbacks.
