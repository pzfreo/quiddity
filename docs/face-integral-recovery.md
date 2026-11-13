# Recovering unresolved spline face integrals

The ordinary 3D-edge projection and quadrature run first. Their successful values
and operation order are retained. A separate retry can use an explicit STEP master
pcurve boundary when projecting the separately approximated 3D curves refuses.
This measures the region specified by the master pcurves; it does not assert that
inconsistent 3D and 2D representations describe identical regions. Neither an
unselected pcurve nor a larger tolerance is a recovery authority.

Near parallel surface tangents make the closest-point equations ill-conditioned.
Gauss–Newton can oscillate for a curve offset normally from its surface, and the
velocity of an almost singular foot point can vary sharply even when its position
residual is small. Deeper halving need not resolve this projection-generated
integrand. An explicit trim supplies UV positions and derivatives without solving
those equations. The regression constructs S(u,v)=(u+v,epsilon*v,v²), a rectangular
trim and polynomial normal-offset 3D curves. Its projected region, area, flux,
centroid and central moments have independent one-dimensional references.

The retry currently supports one nonperiodic, noncollapsed native B-spline or Bézier face loop. Every
edge must have exactly one explicitly selected master pcurve, a compatible range,
finite controls inside the surface domain and matching endpoints within the file's
stated modelling precision plus the kernel coordinate floor. Small endpoint gaps
are closed as in the ordinary walk; a gap exceeding that physical noise band is
refused. Unsupported, absent, ambiguous or unresolved trims remain unavailable.

The outer pass still fixes F=1e-11 times its coarse term magnitudes and retains the
1e-9 local relative criterion, MAX_DEPTH=12 and kink budget. The retry evaluates
surface controls relative to a face point to avoid cancellation of homogeneous
world-coordinate derivatives. Its inner primitives use adaptive Gauss–Kronrod,
with summed remaining error estimates and the same depth limit. A conservative
pcurve variation bound allocates half of F to those primitives. Their weighted
errors are added to the outer estimates; no unresolved prefix is published.
Both levels use numerical estimates, as the ordinary integrator does, rather than
rigorous mathematical true-error bounds. Canonical mass is recovered independently
before a failed shared moment pass can publish or cache a mass result. Its cache
records which boundary supplied mass so later moments use the same region. The
B-rep writer preserves that master choice for recovered faces; it resolves mass
before choosing the flag, keeping its output independent of query order. Other
written faces retain their existing representation choices.

ADR 0008's tolerance distinction is retained: endpoint connectivity uses a modelling
noise band; numerical publication still uses the face-relative quadrature policy.
Neither F nor the existing reference comparison bands are enlarged. No input or
measurement from a privately supplied model belongs in this document or a fixture.
