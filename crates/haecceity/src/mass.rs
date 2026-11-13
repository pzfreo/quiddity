//! Areas and volumes (`BRepGProp`): surface integrals over each face's trimmed parameter
//! domain, reduced by Green's theorem to integrals along the exact edge curves'
//! foot points on the surface. A refused projection may recover using the file's
//! explicit master pcurves, measuring the region they bound. That region can differ
//! from a 3D-edge answer by the file's own 2D/3D disagreement, bounded by a sampled
//! interior check. Recovery uses bounded inner and outer numerical error checks;
//! see `docs/face-integration-work.md`, "Recovery from an explicit master trim".
//! `tests/face_areas.rs` checks every corpus face against OpenCascade.
//!
//! ∬_D f du dv = −∮ H du with H(u, v) = ∫ f(u, s) ds from a fixed v, or ∮ G dv with
//! G(u, v) = ∫ f(s, v) ds from a fixed u. The first ignores boundary pieces of constant u
//! (seams, which files may omit) and serves faces whose loops run round u; the second ignores
//! pieces of constant v (a sphere's poles, a cone's apex) and serves every other face. Where a
//! loop runs round u and ends at a singular line, H starts from that line, and G starts from a
//! B-spline side that collapses to a point, so the line itself contributes nothing and the
//! parameter left undetermined there does not matter. Along each edge the integral is
//! adaptive (Gauss–Kronrod), split where the foot point turns a corner onto or off the
//! surface's boundary or crosses a knot line. Each loop's direction is read from the geometry
//! (which side of it the face lies on), not from the file's orientation flags.

mod preferred;

use std::sync::OnceLock;

use super::brep::Part;
use super::geom::{self, Curve, Surface, V3};
use super::nurbs::breaks;
use super::sampling::{edge_interval, extreme_parameters};
use super::uv::UvLoop;

/// Gauss–Legendre nodes and weights on [-1, 1], in ascending order.
fn gauss_nodes() -> &'static [(f64, f64)] {
    static NODES: OnceLock<Vec<(f64, f64)>> = OnceLock::new();
    NODES.get_or_init(|| {
        let n = 16;
        (0..n)
            .map(|i| {
                // Newton on the Legendre polynomial from Chebyshev's estimate of the root.
                let mut x = (std::f64::consts::PI * (i as f64 + 0.75) / (n as f64 + 0.5)).cos();
                let mut derivative = 0.0;
                for _ in 0..100 {
                    let (mut p0, mut p1) = (1.0, x);
                    for k in 2..=n {
                        let k = k as f64;
                        (p0, p1) = (p1, ((2.0 * k - 1.0) * x * p1 - (k - 1.0) * p0) / k);
                    }
                    derivative = n as f64 * (x * p1 - p0) / (x * x - 1.0);
                    let step = p1 / derivative;
                    x -= step;
                    if step.abs() < 1e-16 {
                        break;
                    }
                }
                (x, 2.0 / ((1.0 - x * x) * derivative * derivative))
            })
            .rev() // ascending, so a panel's nodes are visited in walking order
            .collect()
    })
}

/// ∫ f over the interval running through `cuts` in order, one Gauss panel between each
/// consecutive pair, for every component of f at once.
fn integrate<const N: usize>(cuts: &[f64], f: impl Fn(f64) -> [f64; N]) -> [f64; N] {
    let mut total = [0.0; N];
    for w in cuts.windows(2) {
        let (half, centre) = (0.5 * (w[1] - w[0]), 0.5 * (w[1] + w[0]));
        for &(x, weight) in gauss_nodes() {
            let y = f(centre + half * x);
            for k in 0..N {
                total[k] += weight * half * y[k];
            }
        }
    }
    total
}

/// Most panels or periods an integral is cut into. An angular range is a few turns and a conic's
/// a few radians; the longest is a straight run in a surface's parameters, one panel per unit
/// (a cylinder's v is a length), so a million is a kilometre at a millimetre. Only nonsense
/// geometry (a coordinate of 1e288 from a corrupt file) reaches it, and the integral is then
/// left undetermined rather than allocated or silently coarsened (#52).
const MAX_PANELS: usize = 1 << 20;

/// The cuts from a to b: `panels` equal pieces, each further split at any of `breaks` (where
/// the integrand's polynomial pieces meet) strictly inside it; `None` past [`MAX_PANELS`].
fn cuts(a: f64, b: f64, panels: usize, breaks: &[f64]) -> Option<Vec<f64>> {
    if panels > MAX_PANELS || !(a.is_finite() && b.is_finite()) {
        return None;
    }
    let panels = panels.max(1);
    let mut out: Vec<f64> = (0..=panels)
        .map(|k| a + (b - a) * k as f64 / panels as f64)
        .chain(breaks.iter().copied().filter(|&x| (x - a) * (x - b) < 0.0))
        .collect();
    out.sort_by(|x, y| {
        if b < a {
            y.total_cmp(x)
        } else {
            x.total_cmp(y)
        }
    });
    out.dedup();
    Some(out)
}

/// How an integral along one surface parameter is cut: in one piece where the integrands are
/// polynomial in it (planes, and the straight direction of cylinders and cones), at the knots
/// of a B-spline surface (repeated each `span` round, where the surface closes in that
/// parameter), else every radian.
fn surface_cuts(surface: &Surface, span: f64, along_v: bool, a: f64, b: f64) -> Option<Vec<f64>> {
    match surface {
        Surface::Plane { .. } => Some(vec![a, b]),
        Surface::Cylinder { .. } | Surface::Cone { .. } if along_v => Some(vec![a, b]),
        Surface::Freeform { surface, .. } => {
            let mut knots = if along_v {
                breaks(&surface.knots_v, surface.degree_v)
            } else {
                breaks(&surface.knots_u, surface.degree_u)
            };
            if span.is_finite() && !knots.is_empty() {
                let lo = knots[0];
                let turns = |x: f64| ((x - lo) / span).floor() as i64;
                let (first, last) = (turns(a.min(b)), turns(a.max(b)));
                if last.saturating_sub(first) > MAX_PANELS as i64 {
                    return None;
                }
                let one = knots.clone();
                knots = (first..=last)
                    .flat_map(|n| one.iter().map(move |k| k + n as f64 * span))
                    .collect();
            }
            cuts(a, b, 1, &knots)
        }
        _ => cuts(a, b, (b - a).abs().ceil() as usize, &[]),
    }
}

/// How an integral along an edge is cut: lines in one piece, conics every half radian, B-spline
/// curves at their knots (and in a few pieces besides, for the surface's own variation) — and
/// wherever the edge passes through a sphere's pole, where u jumps and the integrand with it.
fn curve_cuts(surface: &Surface, curve: &Curve, t0: f64, t1: f64) -> Option<Vec<f64>> {
    let mut breaks = match surface {
        // A pole is where the curve is extreme along the axis; a pass near it (within the
        // band boundary samples route along the pole) is cut there too, which is harmless.
        Surface::Sphere { frame, radius } => extreme_parameters(curve, (t0, t1), &[frame.z])
            .into_iter()
            .filter(|&t| {
                let height = geom::dot(geom::sub(curve.value(t), frame.origin), frame.z);
                height.abs() > radius * 0.02f64.cos()
            })
            .collect(),
        _ => Vec::new(),
    };
    match curve {
        Curve::Line { .. } => cuts(t0, t1, 1, &breaks),
        Curve::Circle { .. } | Curve::Ellipse { .. } => {
            cuts(t0, t1, ((t1 - t0).abs() / 0.5).ceil() as usize, &breaks)
        }
        Curve::Nurbs(n) => {
            // Includes the reader's exact quadratic parabola and hyperbola spans.
            // A closed curve's edge may run past its domain's end.
            let period = curve.period().unwrap_or(0.0);
            breaks.extend(
                n.breaks()
                    .into_iter()
                    .flat_map(|k| [k - period, k, k + period]),
            );
            cuts(t0, t1, 4, &breaks)
        }
    }
}

/// A B-spline surface's side of constant u, and of constant v, that collapses to a single
/// point (as at the tip of a surface closing like a cone), if any.
pub(crate) fn collapsed_sides(surface: &Surface) -> (Option<f64>, Option<f64>) {
    let Surface::Freeform { surface, .. } = surface else {
        return (None, None);
    };
    let (u0, u1, v0, v1) = surface.domain();
    let (lo, hi) = surface.control_bounds();
    let tiny = 1e-9 * geom::dist(lo, hi).max(1e-300);
    let point = |f: &dyn Fn(f64) -> V3| {
        let first = f(0.0);
        (1..=8).all(|k| geom::dist(f(k as f64 / 8.0), first) <= tiny)
    };
    let side_u = |u: f64| point(&|s| surface.value(u, v0 + (v1 - v0) * s));
    let side_v = |v: f64| point(&|s| surface.value(u0 + (u1 - u0) * s, v));
    (
        [u0, u1].into_iter().find(|&u| side_u(u)),
        [v0, v1].into_iter().find(|&v| side_v(v)),
    )
}

/// The parameter spans across which a B-spline surface closes on itself (its sides of
/// constant u, or of constant v, coincide), infinite where it does not.
pub(crate) fn closed_spans(surface: &Surface) -> (f64, f64) {
    let Surface::Freeform { surface, .. } = surface else {
        return (f64::INFINITY, f64::INFINITY);
    };
    let (u0, u1, v0, v1) = surface.domain();
    let (lo, hi) = surface.control_bounds();
    let tiny = 1e-9 * geom::dist(lo, hi).max(1e-300);
    let same = |f: &dyn Fn(f64) -> (V3, V3)| {
        (0..=8).all(|k| {
            let (a, b) = f(k as f64 / 8.0);
            geom::dist(a, b) <= tiny
        })
    };
    let u_closed = same(&|s| {
        let v = v0 + (v1 - v0) * s;
        (surface.value(u0, v), surface.value(u1, v))
    });
    let v_closed = same(&|s| {
        let u = u0 + (u1 - u0) * s;
        (surface.value(u, v0), surface.value(u, v1))
    });
    (
        if u_closed { u1 - u0 } else { f64::INFINITY },
        if v_closed { v1 - v0 } else { f64::INFINITY },
    )
}

/// Parameters a whole number of `spans` round a closed B-spline surface, brought back into its
/// domain (the lower side of each closed direction, where the domain starts).
fn wrap_closed(surface: &Surface, spans: (f64, f64), (u, v): (f64, f64)) -> (f64, f64) {
    let Surface::Freeform { surface: s, .. } = surface else {
        return (u, v);
    };
    let (u0, u1, v0, v1) = s.domain();
    let wrap = |x: f64, lo: f64, hi: f64, span: f64| {
        if span.is_finite() && (x < lo || x > hi) {
            lo + (x - lo).rem_euclid(span)
        } else {
            x
        }
    };
    (wrap(u, u0, u1, spans.0), wrap(v, v0, v1, spans.1))
}

/// A boundary node met on the walk: its curve parameter, its surface parameters and their
/// velocity.
type Met = (f64, (f64, f64), (f64, f64));

/// Where the walk round an edge stands: the parameters the next node unwraps against, how far
/// it has got through the edge's anchors and past any singular point, and the first and latest
/// nodes it has met.
#[derive(Clone, Copy)]
struct Walk {
    last: (f64, f64),
    next_anchor: usize,
    regular_seen: bool,
    in_band: bool,
    crossed: bool,
    first: Option<Met>,
    latest: Option<Met>,
    /// ∫ (u − u_ref) dv so far: the parameter-space area the walk sweeps (Green's theorem).
    swept: f64,
}

/// The 21-point Gauss–Kronrod rule on [-1, 1] (QUADPACK's `qk21`) in ascending order: each
/// node with its Kronrod weight and its weight in the embedded 10-point Gauss rule (zero at the
/// Kronrod-only nodes).
fn kronrod_nodes() -> &'static [(f64, f64, f64)] {
    static NODES: OnceLock<Vec<(f64, f64, f64)>> = OnceLock::new();
    NODES.get_or_init(|| {
        const X: [f64; 11] = [
            0.9956571630258081,
            0.9739065285171717,
            0.9301574913557082,
            0.8650633666889845,
            0.7808177265864169,
            0.6794095682990244,
            0.5627571346686047,
            0.4333953941292472,
            0.2943928627014602,
            0.14887433898163122,
            0.0,
        ];
        const K: [f64; 11] = [
            0.011694638867371874,
            0.032558162307964725,
            0.054755896574351995,
            0.07503967481091996,
            0.0931254545836976,
            0.10938715880229764,
            0.12349197626206584,
            0.13470921731147334,
            0.14277593857706009,
            0.14773910490133849,
            0.1494455540029169,
        ];
        const G: [f64; 5] = [
            0.06667134430868814,
            0.1494513491505806,
            0.21908636251598204,
            0.26926671930999635,
            0.29552422471475287,
        ];
        let gauss = |i: usize| if i % 2 == 1 { G[i / 2] } else { 0.0 };
        let lower = (0..11).map(|i| (-X[i], K[i], gauss(i)));
        let upper = (0..10).rev().map(|i| (X[i], K[i], gauss(i)));
        lower.chain(upper).collect()
    })
}

/// One Gauss–Kronrod rule over a panel: the Kronrod sum, its error estimate (the difference
/// from the embedded Gauss rule, on the area scale), the first component's terms' magnitudes
/// and the state after its last node. The first component is an area density. Errors in the remaining components are scaled by
/// their terms' magnitudes to the area scale: a constant area density can hide variation in
/// first and second moments. Using terms' magnitudes also handles cancelling fluxes.
type Panel<S, const N: usize> = ([f64; N], f64, f64, S);

/// Which components drive refinement. Moments retain their own density and scaling;
/// the five mass components use exactly the rule of a standalone `face_mass` call.
#[derive(Clone, Copy)]
enum Components {
    Prefix(usize),
    Mass([usize; 5]),
}

#[derive(Clone, Copy)]
struct IntegralRule {
    components: Components,
    observe_mass: Option<[usize; 5]>,
}

type Estimate = (f64, f64); // error estimate, magnitude of the density's terms

fn estimate<const N: usize>(
    sum: &[f64; N],
    gauss: &[f64; N],
    size: &[f64; N],
    components: Components,
) -> Estimate {
    fn over<const N: usize>(
        sum: &[f64; N],
        gauss: &[f64; N],
        size: &[f64; N],
        first: usize,
        indices: impl Iterator<Item = usize>,
    ) -> Estimate {
        let error = indices
            .map(|k| {
                let scale = size[first].max(size[k]);
                if scale > 0.0 {
                    (sum[k] - gauss[k]).abs() * (size[first] / scale)
                } else {
                    0.0
                }
            })
            .fold(0.0, f64::max);
        (error, size[first])
    }
    match components {
        Components::Prefix(n) => over(sum, gauss, size, 0, 0..n),
        Components::Mass(indices) => over(sum, gauss, size, indices[0], indices.into_iter()),
    }
}

#[cfg(test)]
fn kronrod<S, const N: usize>(
    state: S,
    a: f64,
    b: f64,
    term: impl FnMut(&mut S, f64) -> Option<[f64; N]>,
) -> Option<Panel<S, N>> {
    kronrod_rule(
        state,
        a,
        b,
        term,
        IntegralRule {
            components: Components::Prefix(N),
            observe_mass: None,
        },
    )
    .map(|p| p.0)
}

/// The rule over (a, b) of `term`, visited in ascending order of the panel's parameter from
/// `state`.
fn kronrod_rule<S, const N: usize>(
    mut state: S,
    a: f64,
    b: f64,
    mut term: impl FnMut(&mut S, f64) -> Option<[f64; N]>,
    rule: IntegralRule,
) -> Option<(Panel<S, N>, Estimate)> {
    let (half, centre) = (0.5 * (b - a), 0.5 * (b + a));
    let (mut sum, mut gauss, mut size) = ([0.0; N], [0.0; N], [0.0; N]);
    for &(x, wk, wg) in kronrod_nodes() {
        let y = term(&mut state, centre + half * x)?;
        if y.iter().any(|x| !x.is_finite()) {
            return None;
        }
        for k in 0..N {
            sum[k] += wk * half * y[k];
            gauss[k] += wg * half * y[k];
            size[k] += (wk * half * y[k]).abs();
        }
    }
    let (error, magnitude) = estimate(&sum, &gauss, &size, rule.components);
    let mass = rule.observe_mass.map_or((0.0, 0.0), |indices| {
        let (error, size) = estimate(&sum, &gauss, &size, Components::Mass(indices));
        (if error <= 1e-9 * size { 0.0 } else { error }, size)
    });
    Some(((sum, error, magnitude, state), mass))
}

/// ∫ over the interval running through `cuts`: the sum, the largest error estimate of a panel
/// not resolved relative to its own terms, the sum of the terms' magnitudes, and the state at
/// the end. Without a `floor`, one rule per panel. With one, adaptively: a panel is halved,
/// each half in turn, until its error estimate is below its terms' scale or the floor (the
/// scale below which the integral is not resolved). The integrand is visited in walking order,
/// carrying a state (`eval` runs one rule over (a, b) from a state, and may name a kink inside,
/// where the panel is then split instead of halved; a kink its rule cannot have seen, short of
/// its outermost nodes, splits it even where its error estimate is resolved), so a refined half
/// restarts from where the walk stood. Kink splits are budgeted per interval, not per panel: both
/// halves of a split inherit the panel's count, so a per-panel budget bounds only each path, and
/// a kink that reappears just past every split (an inversion jumping between branches along a
/// stretch of boundary) would grow the tree as 2^`MAX_KINKS`.
/// The last argument to `eval` requests a kink location only when it can be used.
/// Quadrature nodes and the walking state are evaluated in their original order.
/// With `stop_on_excess`, a result whose excess exceeds the fixed floor may be only
/// a prefix: callers must propagate that rejection before using its integral or state.
/// Exhaustive mode is retained for tests of the quadrature and its work bound.
fn adaptive<S: Copy, const N: usize>(
    cuts: &[f64],
    start: S,
    floor: Option<f64>,
    stop_on_excess: bool,
    eval: &mut impl FnMut(S, f64, f64, bool) -> Option<(Panel<S, N>, Option<(f64, bool)>)>,
) -> Option<Panel<S, N>> {
    let mut state = start;
    let (mut total, mut excess, mut magnitude) = ([0.0; N], 0.0f64, 0.0);
    let resolved = |error: f64, size: f64, floor: f64| error <= (1e-9 * size).max(floor);
    for w in cuts.windows(2) {
        // Pending panels, the next on top, with how often they have been halved; and the
        // interval's kink splits left (finite in number, so splitting there spends no halving).
        let mut stack = vec![(w[0], w[1], 0)];
        let mut kinks = MAX_KINKS;
        while let Some((a, b, depth)) = stack.pop() {
            let locate_kinks = floor.is_some() && depth < MAX_DEPTH && kinks > 0;
            let ((sum, error, size, end), kink) = eval(state, a, b, locate_kinks)?;
            let inside = |t: f64| (t - a) * (t - b) < 0.0 && t != a && t != b;
            let kink = kink.filter(|&(t, _)| inside(t) && kinks > 0);
            let unseen = kink.is_some_and(|(_, unseen)| unseen);
            let settled = match floor {
                None => true,
                Some(floor) => depth >= MAX_DEPTH || (resolved(error, size, floor) && !unseen),
            };
            if settled {
                for k in 0..N {
                    total[k] += sum[k];
                }
                magnitude += size;
                if !resolved(error, size, 0.0) {
                    excess = excess.max(error);
                }
                state = end;
                if stop_on_excess && floor.is_some_and(|floor| excess > floor) {
                    return Some((total, excess, magnitude, state));
                }
            } else {
                let (m, depth) = match kink {
                    Some((t, _)) => {
                        kinks -= 1;
                        (t, depth)
                    }
                    None => (0.5 * (a + b), depth + 1),
                };
                stack.push((m, b, depth));
                stack.push((a, m, depth));
            }
        }
    }
    Some((total, excess, magnitude, state))
}

/// How many times a boundary panel may be halved (to a 4096th), and an interval split at kinks.
const MAX_DEPTH: usize = 12;
const MAX_KINKS: usize = 64;

/// The foot point of `p` on the surface, inverted from `seed` (within the domain): where a
/// B-spline surface's inversion is held on the domain's boundary in one parameter, the nearest
/// point along that side. The inversion clamps its two-parameter steps, so the free parameter
/// it settles at is not the side's nearest point, and the boundary integrals need the point
/// whose motion [`uv_velocity`] gives. A side where the surface closes (`spans`) is its seam,
/// not a boundary: an inversion stopped there from one side is tried again from the other.
fn foot(surface: &Surface, spans: (f64, f64), p: V3, seed: (f64, f64)) -> Option<(f64, f64)> {
    let (mut u, mut v) = surface.parameters(p, Some(seed))?;
    let Surface::Freeform { surface: s, .. } = surface else {
        return Some((u, v));
    };
    let (u0, u1, v0, v1) = s.domain();
    let across = |x: f64, lo: f64, hi: f64, span: f64| {
        (span.is_finite() && (x == lo || x == hi)).then_some(if x == lo { hi } else { lo })
    };
    let other = match (across(u, u0, u1, spans.0), across(v, v0, v1, spans.1)) {
        (Some(u), _) => Some((u, v)),
        (None, Some(v)) => Some((u, v)),
        (None, None) => None,
    };
    if let Some(retry) = other.and_then(|q| surface.parameters(p, Some(q)))
        && geom::dist(s.value(retry.0, retry.1), p) < geom::dist(s.value(u, v), p)
    {
        (u, v) = retry;
    }
    // Within round-off of a side is on it (the inversion may stop a hair inside).
    let snap = |x: &mut f64, lo: f64, hi: f64, span: f64| {
        if span.is_finite() {
            return false;
        }
        let near = 1e-12 * (hi - lo);
        if *x <= lo + near {
            *x = lo;
        } else if *x >= hi - near {
            *x = hi;
        }
        *x == lo || *x == hi
    };
    let (on_u, on_v) = (snap(&mut u, u0, u1, spans.0), snap(&mut v, v0, v1, spans.1));
    if on_u == on_v {
        return Some((u, v));
    }
    for _ in 0..50 {
        let (q, su, sv) = s.value_and_partials(u, v);
        let r = geom::sub(p, q);
        let (before, x) = if on_v { (u, &mut u) } else { (v, &mut v) };
        let (d, lo, hi) = if on_v { (su, u0, u1) } else { (sv, v0, v1) };
        let dd = geom::dot(d, d);
        if dd <= 0.0 {
            break;
        }
        *x = (before + geom::dot(d, r) / dd).clamp(lo, hi);
        if (*x - before).abs() <= 1e-15 * (hi - lo) {
            break;
        }
    }
    Some((u, v))
}

/// The surface's point and first partials at (u, v).
fn point_and_partials(surface: &Surface, u: f64, v: f64) -> (V3, V3, V3) {
    match surface {
        Surface::Freeform { surface, .. } => surface.value_and_partials(u, v),
        _ => {
            let (su, sv) = surface.partials(u, v);
            (surface.value(u, v), su, sv)
        }
    }
}

/// The parameter-space velocity (du/dt, dv/dt) of the foot point (u, v) on the surface of a
/// curve point `p` moving at `c`. With r = p − S(u, v) normal to the surface, differentiating
/// r·Su = r·Sv = 0 gives [Su·Su − r·Suu, Su·Sv − r·Suv; Su·Sv − r·Suv, Sv·Sv − r·Svv]·(du, dv)
/// = (Su·c, Sv·c): an edge off its surface (within its tolerance) has its foot point moving
/// faster or slower than its tangent's projection, by the distance over the radius of
/// curvature, and the boundary integrals must follow the nodes they are evaluated at. The
/// second partials, needed only in that small correction, are central differences. A foot
/// point held on a B-spline surface's boundary (the edge runs just outside it) moves only
/// along it: the held parameter's row drops out. Sides where the surface closes (`spans`)
/// hold nothing.
fn uv_velocity(surface: &Surface, spans: (f64, f64), u: f64, v: f64, p: V3, c: V3) -> (f64, f64) {
    let (s, su, sv) = point_and_partials(surface, u, v);
    let r = geom::sub(p, s);
    let (mut a, mut b, mut d) = (geom::dot(su, su), geom::dot(su, sv), geom::dot(sv, sv));
    let (p, q) = (geom::dot(su, c), geom::dot(sv, c));
    // Undisturbed, the first fundamental form: at a degenerate point it decides alone.
    // Compare rank with the product of the two tangent metrics, not their largest
    // square: rescaling either UV coordinate must not turn a regular patch singular.
    let metric = (a, d);
    let (hu, hv, (u0, u1, v0, v1), piece) = match surface {
        Surface::Freeform { surface, .. } => {
            let (u0, u1, v0, v1) = surface.domain();
            // The polynomial piece the point is in (a knot belongs to the piece above it, as
            // the walk's kinks are found): the second partials jump across a knot line, and a
            // difference straddling it would smear that jump over the stencil's width, where
            // no panel is split.
            let span = |x: f64, knots: &[f64], degree: usize| {
                let b = breaks(knots, degree);
                let i = b.partition_point(|&k| k <= x).clamp(1, b.len() - 1);
                (b[i - 1], b[i])
            };
            let (pu, pv) = (
                span(u, &surface.knots_u, surface.degree_u),
                span(v, &surface.knots_v, surface.degree_v),
            );
            (
                1e-5 * (u1 - u0),
                1e-5 * (v1 - v0),
                (u0, u1, v0, v1),
                (pu.0, pu.1, pv.0, pv.1),
            )
        }
        _ => {
            let all = (f64::MIN, f64::MAX, f64::MIN, f64::MAX);
            (1e-5, 1e-5, all, all)
        }
    };
    if geom::norm(r) > 0.0 {
        // Differences inside the point's piece, one-sided at its edges; a stencil point that
        // would reach the piece's upper knot stops just short of it, where a partial that is
        // discontinuous there (a knot of full multiplicity) is still this piece's.
        let (ua, ub) = ((u - hu).max(piece.0), (u + hu).min(piece.1 - 1e-3 * hu));
        let (va, vb) = ((v - hv).max(piece.2), (v + hv).min(piece.3 - 1e-3 * hv));
        let (_, su_a, _) = point_and_partials(surface, ua, v);
        let (_, su_b, _) = point_and_partials(surface, ub, v);
        let (_, su_c, sv_c) = point_and_partials(surface, u, va);
        let (_, su_d, sv_d) = point_and_partials(surface, u, vb);
        let along = |x: V3, y: V3, h: f64| geom::dot(r, geom::sub(y, x)) / h;
        a -= along(su_a, su_b, ub - ua);
        b -= along(su_c, su_d, vb - va);
        d -= along(sv_c, sv_d, vb - va);
    }
    let one = |num: f64, den: f64, scale: f64| {
        if den.abs() <= 1e-6 * scale {
            0.0
        } else {
            num / den
        }
    };
    match (
        !spans.0.is_finite() && (u <= u0 || u >= u1),
        !spans.1.is_finite() && (v <= v0 || v >= v1),
    ) {
        (true, true) => return (0.0, 0.0),
        (true, false) => return (0.0, one(q, d, metric.1)),
        (false, true) => return (one(p, a, metric.0), 0.0),
        (false, false) => {}
    }
    let det = a * d - b * b;
    // At a degenerate point (a pole, a collapsed B-spline edge) the parameters are not
    // determined; such points contribute nothing to the boundary integrals.
    if det.abs() <= 1e-12 * metric.0 * metric.1 {
        return (0.0, 0.0);
    }
    ((d * p - b * q) / det, (a * q - b * p) / det)
}

fn mass_density(p: V3, su: V3, sv: V3) -> [f64; 5] {
    let n = geom::cross(su, sv);
    [geom::norm(n), geom::dot(p, n), n[0], n[1], n[2]]
}

fn valid_mass(m: [f64; 5]) -> Option<[f64; 5]> {
    (m[0] > 0. && m.iter().all(|x| x.is_finite())).then_some(m)
}

/// Canonical mass and its boundary authority are published together. A concurrent
/// first moment query cannot publish a legacy region after mass selected the master trim.
#[derive(Debug)]
pub(super) struct FaceMass {
    terms: Option<[f64; 5]>,
    preferred: bool,
}

impl FaceMass {
    fn legacy(terms: Option<[f64; 5]>) -> Self {
        Self {
            terms,
            preferred: false,
        }
    }
}

impl Part {
    /// The solid's volume and area (`solid.volume`, `solid.area`). The volume is a third of the
    /// flux of the position vector out through its faces (a void's faces subtract), measured
    /// from the mean of the solid's vertex uses (OpenCascade's `roughBaryCenter`). A file's
    /// faces rarely close exactly (edges stray from their surfaces within tolerance, so the
    /// outward area vectors leave a residue: −0.0225 mm² along z on nist_ctc_01), and the flux
    /// through such a shell depends on the point it is measured from; one that moves with the
    /// part keeps the volume unchanged by a placement, as OpenCascade's is.
    pub fn solid_mass(&self, solid: usize) -> Option<(f64, f64)> {
        self.shell_mass(&self.solids[solid].faces)
    }

    /// Signed volume and area of closed, oriented shells. An outer shell has positive
    /// volume; a solid's inward-facing void shell has negative volume. The reference point
    /// follows these faces so the integral remains stable under rigid motion.
    pub fn shell_mass(&self, faces: &[usize]) -> Option<(f64, f64)> {
        let mut centre = [0.0; 3];
        let mut uses = 0usize;
        for &face in faces {
            for lp in &self.faces[face].loops {
                // TopExp_Explorer meets each edge's two vertices at each use, and a vertex loop
                // as the two ends of the degenerate edge OpenCascade makes of it.
                let points = lp
                    .edges
                    .iter()
                    .flat_map(|&(e, _)| [self.edges[e].start, self.edges[e].end])
                    .chain(lp.vertex.into_iter().flat_map(|p| [p, p]));
                for p in points {
                    centre = geom::add(centre, p);
                    uses += 1;
                }
            }
        }
        let centre = geom::scale(centre, 1.0 / uses.max(1) as f64);
        let (mut flux, mut area) = (0.0, 0.0);
        for &face in faces {
            let [a, f, nx, ny, nz] = self.face_terms(face)?;
            let f = f - geom::dot(centre, [nx, ny, nz]);
            area += a;
            flux += if self.faces[face].reversed { -f } else { f };
        }
        Some((flux / 3.0, area))
    }

    /// The face's area and the flux of the position vector through it along the surface's
    /// natural normal.
    pub fn face_mass(&self, face: usize) -> Option<[f64; 2]> {
        self.face_terms(face).map(|m| [m[0], m[1]])
    }

    /// The face's area, the flux of the position vector through it and its area vector ∬ n dA,
    /// along the surface's natural normal.
    fn face_terms(&self, face: usize) -> Option<[f64; 5]> {
        self.cache[face]
            .mass
            .get_or_init(|| self.compute_face_mass(face))
            .terms
    }

    fn compute_face_mass(&self, face: usize) -> FaceMass {
        let surface = &self.faces[face].surface;
        // A whole sphere has no boundary to integrate along; its area and flux (3V) are exact,
        // and its area vector vanishes.
        if let Surface::Sphere { frame, radius } = surface
            && self.whole_sphere(face)
        {
            let sense = if frame.direct() { 1.0 } else { -1.0 };
            return FaceMass::legacy(Some([
                4.0 * std::f64::consts::PI * radius * radius,
                sense * 4.0 * std::f64::consts::PI * radius.powi(3),
                0.0,
                0.0,
                0.0,
            ]));
        }
        let density = mass_density;
        let rule = IntegralRule {
            components: Components::Prefix(5),
            observe_mass: None,
        };
        if let Some(value) = self.integrate_face_legacy(face, density, rule, &mut None) {
            return FaceMass::legacy(valid_mass(value));
        }
        self.recover_face_mass(face)
    }

    /// Only a known legacy refusal reaches here. Ineligible trims do no further
    /// integration; eligible trims certify canonical mass on their own boundary.
    fn recover_face_mass(&self, face: usize) -> FaceMass {
        let terms = self
            .integrate_preferred_face(face, mass_density, Components::Prefix(5))
            .and_then(valid_mass);
        FaceMass {
            preferred: terms.is_some(),
            terms,
        }
    }

    /// Share the cold coarse pass, but choose and publish canonical mass and its
    /// boundary under one OnceLock before any moment result can escape.
    fn moment_integral<const N: usize, const M: usize>(
        &self,
        face: usize,
        mass: [usize; 5],
        f: impl Fn(V3, V3, V3) -> [f64; N],
    ) -> Option<[f64; M]> {
        let mut shared = None;
        let mut attempted = false;
        let choice = self.cache[face].mass.get_or_init(|| {
            attempted = true;
            let mut observed = None;
            shared = self.integrate_face_legacy(
                face,
                &f,
                IntegralRule {
                    components: Components::Prefix(M),
                    observe_mass: Some(mass),
                },
                &mut observed,
            );
            match observed {
                Some(Some(terms)) => FaceMass::legacy(Some(terms)),
                Some(None) => self.recover_face_mass(face),
                // An early failure in additional fields did not evaluate canonical
                // mass. Determine it independently rather than poisoning its cache.
                None => self.compute_face_mass(face),
            }
        });
        choice.terms?;
        if choice.preferred {
            return self.integrate_preferred_face(
                face,
                |p, su, sv| {
                    let y = f(p, su, sv);
                    std::array::from_fn(|k| y[k])
                },
                Components::Prefix(M),
            );
        }
        if attempted {
            return shared.map(|y| std::array::from_fn(|k| y[k]));
        }
        self.integrate_face_legacy(
            face,
            |p, su, sv| {
                let y = f(p, su, sv);
                std::array::from_fn(|k| y[k])
            },
            IntegralRule {
                components: Components::Prefix(M),
                observe_mass: None,
            },
            &mut None,
        )
    }

    fn integrate_face_legacy<const N: usize>(
        &self,
        face: usize,
        f: impl Fn(V3, V3, V3) -> [f64; N],
        rule: IntegralRule,
        observed: &mut Option<Option<[f64; 5]>>,
    ) -> Option<[f64; N]> {
        let fc = &self.faces[face];
        let surface = &fc.surface;
        let (pu, pv) = surface.periodic();
        let uv = self.uv_loops(face)?;
        let domain = self.domain(face)?;
        let (_, shifts) = self.loop_placement(face)?;
        // A loop runs round u when its boundary does (on a closed B-spline surface, which the
        // reader does not mark periodic, when it ends most of the parameter range from where
        // it began).
        let open = |lp: &UvLoop, pick: fn(&(f64, f64)) -> f64, far: f64| match (
            lp.points.first(),
            lp.points.last(),
        ) {
            (Some(a), Some(b)) => (pick(b) - pick(a)).abs() > far,
            _ => false,
        };
        let half = |range: (f64, f64)| 0.5 * (range.1 - range.0);
        let far_v = if pv { 1.0 } else { half(domain.v_range()) };
        let along_v = if pu {
            uv.iter().any(|lp| lp.winds_u)
        } else {
            uv.iter()
                .any(|lp| open(lp, |p| p.0, half(domain.u_range())))
        };
        if along_v && uv.iter().any(|lp| open(lp, |p| p.1, far_v)) {
            return None;
        }
        // On a closed B-spline surface the walk unwraps its parameters across the surface's
        // seam (a face's own seam need not lie on it), so the integrands read the surface a
        // whole span round.
        let spans = closed_spans(surface);
        let evaluator = match surface {
            Surface::Freeform { surface, .. } => Some(std::cell::RefCell::new(
                crate::nurbs::SurfaceEvaluation::new(surface),
            )),
            _ => None,
        };
        let f = |u: f64, v: f64| {
            let (u, v) = wrap_closed(surface, spans, (u, v));
            let (p, su, sv) = match &evaluator {
                Some(evaluator) => evaluator.borrow_mut().value_and_partials(u, v),
                None => point_and_partials(surface, u, v),
            };
            f(p, su, sv)
        };
        let first = (0..fc.loops.len()).find(|&i| fc.loops[i].vertex.is_none())?;
        let start = uv[first].points.first().copied()?;
        let start = (start.0 + shifts[first].0, start.1 + shifts[first].1);
        // Where a B-spline side collapses to a point, the parameter along it is undetermined
        // beside it, and the inner integral starts from that side, so that it vanishes there as
        // it does from a sphere's pole.
        let collapsed = collapsed_sides(surface);
        let reference = if along_v {
            let (lo, hi) = domain.v_range();
            [lo, hi]
                .into_iter()
                .find_map(|v| surface.singular_v(v).filter(|s| (s - v).abs() < 1e-6))
                .or(collapsed.1)
                .unwrap_or(start.1)
        } else {
            collapsed.0.unwrap_or(start.0)
        };
        // (The swept area, read only where the loops do not run round u, is measured from the
        // same reference.)
        let u_ref = if along_v { 0.0 } else { reference };
        let term = |u: f64, v: f64, du: f64, dv: f64| {
            let (weight, y) = if along_v {
                let cuts = surface_cuts(surface, spans.1, true, reference, v)?;
                (-du, integrate(&cuts, |s| f(u, s)))
            } else {
                let cuts = surface_cuts(surface, spans.0, false, reference, u)?;
                (dv, integrate(&cuts, |s| f(s, v)))
            };
            Some(y.map(|y| weight * y))
        };
        let loops: Vec<usize> = (0..fc.loops.len())
            .filter(|&i| fc.loops[i].vertex.is_none())
            .collect();
        // One rule per panel; then again, refining where needed, for a loop with a panel
        // unresolved at the scale of the whole face's terms.
        let mut passes = Vec::with_capacity(loops.len());
        for &i in &loops {
            passes.push(self.boundary_integral(face, i, shifts[i], &term, (None, rule), u_ref)?);
        }
        // Canonical mass validity uses its own errors, floor, refinement and loop sense,
        // irrespective of which additional components this caller needs.
        if let Some(indices) = rule.observe_mass {
            let mass_floor = 1e-11 * passes.iter().map(|p| p.1.1).sum::<f64>();
            let mut mass = [0.0; 5];
            for (&i, (panel, estimate)) in loops.iter().zip(&passes) {
                let (sum, swept) = if estimate.0 > mass_floor {
                    let (refined, _) = self.boundary_integral(
                        face,
                        i,
                        shifts[i],
                        &term,
                        (
                            Some(mass_floor),
                            IntegralRule {
                                components: Components::Mass(indices),
                                observe_mass: None,
                            },
                        ),
                        u_ref,
                    )?;
                    if refined.1 > mass_floor {
                        *observed = Some(None);
                        return None;
                    }
                    (refined.0, refined.3)
                } else {
                    (panel.0, panel.3)
                };
                let winding = along_v || uv[i].winds_v;
                let direction = self.loop_direction(face, i, (!winding).then_some(swept))?;
                for k in 0..5 {
                    mass[k] += direction * sum[indices[k]];
                }
            }
            let mass = (mass[0] > 0.0 && mass.iter().all(|x| x.is_finite())).then_some(mass);
            *observed = Some(mass);
            mass?;
        }
        let floor = 1e-11 * passes.iter().map(|p| p.0.2).sum::<f64>();
        let mut total = [0.0; N];
        for (&i, ((mut sum, excess, _, mut swept), _)) in loops.iter().zip(passes) {
            if excess > floor {
                let (refined, _) = self.boundary_integral(
                    face,
                    i,
                    shifts[i],
                    &term,
                    (
                        Some(floor),
                        IntegralRule {
                            observe_mass: None,
                            ..rule
                        },
                    ),
                    u_ref,
                )?;
                // A panel still unresolved at the floor after the deepest halving leaves the
                // area unknown to that scale: refused rather than reported as if exact.
                if refined.1 > floor {
                    return None;
                }
                sum = refined.0;
                swept = refined.3;
            }
            // A loop that winds round either parameter has no signed area to read.
            let winding = along_v || uv[i].winds_v;
            let direction = self.loop_direction(face, i, (!winding).then_some(swept))?;
            for k in 0..N {
                total[k] += direction * sum[k];
            }
        }
        Some(total)
    }

    /// ∮ `term`(u, v, du/dt, dv/dt) dt round loop *i* of the face in parameter space (placed by
    /// `shift`): along each edge's exact curve, and straight across any gap between one edge's
    /// end and the next one's start (a collapsed B-spline side, a pole), less whole turns. The
    /// swept parameter area is ∮ (u − `u_ref`) dv, measured from the inner integral's reference
    /// as the terms are: where the foot point jumps along a collapsed side u = `u_ref` (its v
    /// undetermined there), neither counts the jump.
    fn boundary_integral<const N: usize>(
        &self,
        face: usize,
        i: usize,
        shift: (f64, f64),
        term: &dyn Fn(f64, f64, f64, f64) -> Option<[f64; N]>,
        (floor, rule): (Option<f64>, IntegralRule),
        u_ref: f64,
    ) -> Option<(Panel<f64, N>, Estimate)> {
        debug_assert!(rule.observe_mass.is_none() || floor.is_none());
        let mut mass_estimate: Estimate = (0.0, 0.0);
        let fc = &self.faces[face];
        let surface = &fc.surface;
        let (pu, pv) = surface.periodic();
        let lp = &self.uv_loops(face)?[i];
        let spans = closed_spans(surface);
        let collapsed = collapsed_sides(surface);
        let has_pole = collapsed.0.is_some() || collapsed.1.is_some();
        let poles = match surface {
            Surface::Freeform { surface: s, .. } => {
                let (u0, _, v0, _) = s.domain();
                [
                    collapsed.0.map(|u| surface.value(u, v0)),
                    collapsed.1.map(|v| surface.value(u0, v)),
                ]
            }
            _ => [None, None],
        };
        // The turn (or, round a closed B-spline surface, the whole span) nearest `near`.
        let unwrap = |(u, v): (f64, f64), near: (f64, f64)| {
            let span = |x: f64, near: f64, span: f64| {
                if span.is_finite() {
                    x - span * ((x - near) / span).round()
                } else {
                    x
                }
            };
            (
                if pu {
                    geom::nearest_turn(u, near.0)
                } else {
                    span(u, near.0, spans.0)
                },
                if pv {
                    geom::nearest_turn(v, near.1)
                } else {
                    span(v, near.1, spans.1)
                },
            )
        };
        let wrap = |p: (f64, f64)| wrap_closed(surface, spans, p);
        let placed = |p: &(f64, f64)| (p.0 + shift.0, p.1 + shift.1);
        let mut total: Panel<f64, N> = ([0.0; N], 0.0, 0.0, 0.0);
        let mut add = |(sum, excess, size, swept): Panel<f64, N>| {
            for (t, s) in total.0.iter_mut().zip(sum) {
                *t += s;
            }
            total.1 = total.1.max(excess);
            total.2 += size;
            total.3 += swept;
        };
        let mut ends: Vec<((f64, f64), (f64, f64))> = Vec::new();
        let mut last = placed(lp.points.first()?);
        for (k, &(e, forward)) in fc.loops[i].edges.iter().enumerate() {
            // Turns exist only on periodic surfaces; elsewhere the previous node is the better
            // inversion seed (an edge may start on a collapsed B-spline side). The edge starts on
            // the turn of its first regular sample and, past each pole or apex it crosses, takes
            // up the turn of the next.
            let anchors = match lp.anchors.get(k) {
                Some(a) if pu || pv => a.as_slice(),
                _ => &[],
            };
            if let Some(p) = anchors.first() {
                last = placed(p);
            }
            let ed = &self.edges[e];
            let (mut t0, mut t1) =
                edge_interval(&ed.curve, ed.start, ed.end, ed.same_sense, ed.is_closed());
            if !forward {
                std::mem::swap(&mut t0, &mut t1);
            }
            // One node: its parameters, unwrapped against the walk's, and their velocity.
            let node = |walk: &mut Walk, t: f64| -> Option<((f64, f64), (f64, f64))> {
                let point = ed.curve.value(t);
                // On a collapsed B-spline side, where the other parameter is undetermined,
                // Newton cannot leave the seed; it starts a little inside instead.
                let seed = match surface {
                    Surface::Freeform { surface: s, .. } => {
                        let (u0, u1, v0, v1) = s.domain();
                        let (u, v) = wrap(walk.last);
                        let (_, su, sv) = point_and_partials(surface, u, v);
                        let (a, b) = (geom::norm(su), geom::norm(sv));
                        let inward = |x: f64, lo: f64, hi: f64| {
                            x + 1e-3 * (hi - lo) * if x - lo < hi - x { 1.0 } else { -1.0 }
                        };
                        if b <= 1e-6 * a.max(b) {
                            (inward(u, u0, u1), v)
                        } else if a <= 1e-6 * a.max(b) {
                            (u, inward(v, v0, v1))
                        } else {
                            (u, v)
                        }
                    }
                    _ => walk.last,
                };
                let (mut u, mut v) = foot(surface, spans, point, seed)?;
                // Round a closed B-spline surface the point takes the parameters a whole span
                // round that lie beside the walk (on the surface's seam it has both, and a
                // boundary may cross that seam anywhere: a face's own seam need not lie on it).
                let (mut su, mut sv) = surface.partials(u, v);
                if matches!(surface, Surface::Freeform { .. }) {
                    let small_jacobian = geom::norm(su) <= 1e-12 * geom::norm(sv)
                        || geom::norm(sv) <= 1e-12 * geom::norm(su);
                    if small_jacobian || has_pole {
                        let residual = geom::dist(surface.value(u, v), point);
                        // Preserve the existing singular-Jacobian recovery. A last-ulp change in a
                        // control point can also stall Newton *beside* the pole with a small
                        // nonzero Jacobian, so rank alone must not gate the retry.
                        let rank_stalled = small_jacobian && residual > 1e-9;
                        // Each collapsed side supplies a known point on the surface. If the
                        // proposed foot is no closer than that pole (within 1e-9 mm), it may
                        // be a false local minimum. Accept this additional retry only for a
                        // geometric improvement exceeding 1e-9 mm, not a last-ulp residual tie.
                        let false_pole = !rank_stalled
                            && has_pole
                            && poles.iter().flatten().any(|&pole| {
                                let distance = geom::dist(point, pole);
                                distance > 1e-9 && residual >= distance - 1e-9
                            });
                        if rank_stalled || false_pole {
                            let retry = crate::uv::beside_collapsed(surface, point, collapsed)?;
                            if rank_stalled
                                || geom::dist(surface.value(retry.0, retry.1), point) + 1e-9
                                    < residual
                            {
                                (u, v) = retry;
                                (su, sv) = surface.partials(u, v);
                            }
                        }
                    }
                }
                if surface.singular_v(v).is_some() && geom::norm(su) <= 1e-6 * geom::norm(sv) {
                    // At the singular point itself u is round-off: it stays where the walk is,
                    // rather than setting the turn the rest of the edge unwraps against.
                    u = walk.last.0;
                } else if surface.singular_v(v).is_some() {
                    // Through the singular point u jumps (once per pass near it, as the loop's
                    // samples are split).
                    walk.in_band = walk.regular_seen;
                    let jump = (geom::nearest_turn(u, walk.last.0) - walk.last.0).abs();
                    if walk.in_band && !walk.crossed && jump > std::f64::consts::FRAC_PI_2 {
                        if let Some(p) = anchors.get(walk.next_anchor) {
                            walk.last = placed(p);
                            walk.next_anchor += 1;
                        }
                        walk.crossed = true;
                    }
                } else {
                    if walk.in_band
                        && !walk.crossed
                        && let Some(p) = anchors.get(walk.next_anchor)
                    {
                        walk.last = placed(p);
                        walk.next_anchor += 1;
                    }
                    (walk.regular_seen, walk.in_band, walk.crossed) = (true, false, false);
                }
                let velocity = uv_velocity(surface, spans, u, v, point, ed.curve.derivative(t));
                let (u, v) = unwrap((u, v), walk.last);
                walk.last = (u, v);
                walk.first.get_or_insert((t, (u, v), velocity));
                walk.latest = Some((t, (u, v), velocity));
                Some(((u, v), velocity))
            };
            // Where a foot point starts or stops being held on a B-spline surface's boundary
            // its path turns a corner, and where it crosses a knot line the integrand's
            // derivatives jump; a panel is split there, located by bisection on which piece of
            // the surface the foot point is in. The bisection reads the foot point as the nodes
            // do ([`foot`]): along a held side the inversion's own parameter is not the
            // foot point, and a split where it crosses the knot line misses the real crossing
            // by enough to leave the jump inside a panel (1e-8 of cgb242 face 546's area, and a
            // different answer with each placement).
            let knots = match surface {
                Surface::Freeform { surface, .. } => (
                    breaks(&surface.knots_u, surface.degree_u),
                    breaks(&surface.knots_v, surface.degree_v),
                ),
                _ => (Vec::new(), Vec::new()),
            };
            // (A closed direction's sides are its seam, which the foot point crosses freely:
            // they hold nothing, and its pieces count on round the turns.)
            let held = |(u, v): (f64, f64)| match surface {
                Surface::Freeform { surface: s, .. } => {
                    let (u0, u1, v0, v1) = s.domain();
                    let (wu, wv) = wrap((u, v));
                    let piece = |x: f64, w: f64, span: f64, knots: &[f64]| {
                        let turns = if span.is_finite() {
                            ((x - w) / span).round() as i64
                        } else {
                            0
                        };
                        turns * knots.len() as i64 + knots.partition_point(|&k| k <= w) as i64
                    };
                    (
                        !spans.0.is_finite() && (u <= u0 || u >= u1),
                        !spans.1.is_finite() && (v <= v0 || v >= v1),
                        piece(u, wu, spans.0, &knots.0),
                        piece(v, wv, spans.1, &knots.1),
                    )
                }
                _ => (false, false, 0, 0),
            };
            let corner = |(ta, pa): (f64, (f64, f64)), tb: f64| {
                let (mut lo, mut hi) = (ta, tb);
                for _ in 0..60 {
                    let mid = 0.5 * (lo + hi);
                    let point = ed.curve.value(mid);
                    match foot(surface, spans, point, wrap(pa)).map(|q| unwrap(q, pa)) {
                        Some(q) if held(q) == held(pa) => lo = mid,
                        _ => hi = mid,
                    }
                }
                0.5 * (lo + hi)
            };
            // Nodes are visited in walking order, so periodic parameters unwrap node by node.
            let mut edge_mass: Estimate = (0.0, 0.0);
            let mut eval = |walk: Walk, a: f64, b: f64, locate_kinks: bool| {
                let mut met: Vec<(f64, (f64, f64))> =
                    Vec::with_capacity(if locate_kinks { 21 } else { 0 });
                let mut weights = kronrod_nodes().iter().map(|n| 0.5 * (b - a) * n.1);
                let (panel, mass) = kronrod_rule(
                    walk,
                    a,
                    b,
                    |walk, t| {
                        let ((u, v), (du, dv)) = node(walk, t)?;
                        walk.swept += weights.next().unwrap_or(0.0) * (u - u_ref) * dv;
                        if locate_kinks {
                            met.push((t, (u, v)));
                        }
                        term(u, v, du, dv)
                    },
                    rule,
                )?;
                edge_mass.0 = edge_mass.0.max(mass.0);
                edge_mass.1 += mass.1;
                if !locate_kinks {
                    return Some((panel, None));
                }
                let kink = met
                    .windows(2)
                    .find(|w| held(w[0].1) != held(w[1].1))
                    .map(|w| (corner(w[0], w[1].0), false));
                // A turn between an end of the panel and the node beside it is beyond the rule's
                // reach: cgb217 face 34's edge 89 leaves the side v = 1 1.2e-5 before crossing the
                // knot line u = 0.25, where the walk splits, so the step off the side fell short of
                // the next panel's first node (2.2e-6 in v, 5e-5 of the face's area). The foot
                // point at each end is compared with the node beside it (a turn within round-off
                // of the end is the split's own).
                let unseen = || {
                    let (first, last) = (*met.first()?, *met.last()?);
                    [(first, a), (last, b)]
                        .into_iter()
                        .find_map(|((tn, pn), te)| {
                            let q = foot(surface, spans, ed.curve.value(te), wrap(pn))
                                .map(|q| unwrap(q, pn))?;
                            let t = (held(q) != held(pn)).then(|| corner((tn, pn), te))?;
                            ((t - te).abs() > 1e-9 * (b - a).abs()).then_some((t, true))
                        })
                };
                // (Only refinement splits, so a single pass need not look.)
                let kink = match surface {
                    Surface::Freeform { .. } if kink.is_none() && floor.is_some() => unseen(),
                    _ => kink,
                };
                Some((panel, kink))
            };
            let walk = Walk {
                last,
                next_anchor: 1,
                regular_seen: false,
                in_band: false,
                crossed: false,
                first: None,
                latest: None,
                swept: 0.0,
            };
            let mut cuts = curve_cuts(surface, &ed.curve, t0, t1)?;
            // A foot point that turns a corner between a cut and the node beside it (the rule's
            // nodes stop short of a panel's ends) leaves the turn inside that panel, unseen by
            // its error estimate (2.3e-4 of cgb203 face 3, whose edge leaves the surface's side
            // v = 1 within 1e-4 of its end): the edge is cut there too.
            if matches!(surface, Surface::Freeform { .. }) && cuts.len() >= 2 {
                let x = 0.5 * (1.0 + kronrod_nodes()[0].0);
                let at = |t: f64, near: (f64, f64)| {
                    foot(surface, spans, ed.curve.value(t), wrap(near))
                        .map(|q| (t, unwrap(q, near)))
                };
                let mut more = Vec::new();
                let mut near = last;
                for k in 0..cuts.len() {
                    let Some(here) = at(cuts[k], near) else {
                        continue;
                    };
                    near = here.1;
                    let sides = [k.checked_sub(1), (k + 1 < cuts.len()).then_some(k + 1)];
                    for j in sides.into_iter().flatten() {
                        let inner = cuts[k] + x * (cuts[j] - cuts[k]);
                        if let Some(i) = at(inner, here.1)
                            && held(here.1) != held(i.1)
                        {
                            let t = corner(i, cuts[k]);
                            // (A turn within round-off of the cut is the cut's own.)
                            if (t - cuts[k]).abs() > 1e-7 * (cuts[j] - cuts[k]).abs() {
                                more.push(t);
                            }
                        }
                    }
                }
                cuts.extend(more);
                cuts.sort_by(|a, b| {
                    if t1 < t0 {
                        b.total_cmp(a)
                    } else {
                        a.total_cmp(b)
                    }
                });
                cuts.dedup();
            }
            let (sum, excess, size, walk) = adaptive(&cuts, walk, floor, true, &mut eval)?;
            mass_estimate.0 = mass_estimate.0.max(edge_mass.0);
            mass_estimate.1 += edge_mass.1;
            add((sum, excess, size, walk.swept));
            // Excess is a maximum over accepted leaves. The caller already refuses
            // this loop at the same fixed floor; no later edge can lower it.
            if floor.is_some_and(|floor| excess > floor) {
                return Some((total, mass_estimate));
            }
            last = walk.last;
            // The edge's exact ends, inverted from the nodes beside them; next to a collapsed
            // B-spline side an inversion can jump across it, so one far from where the node's
            // own motion leads is replaced by that extrapolation. An edge too short for its
            // interval to hold a node (its two ends invert to one curve parameter) stands where
            // the walk is.
            let still = (t0, last, (0.0, 0.0));
            let (t_first, p_first, v_first) = walk.first.unwrap_or(still);
            let (t_last, p_last, v_last) = walk.latest.unwrap_or(still);
            let end = |t: f64, from_t: f64, p: (f64, f64), vel: (f64, f64)| {
                let guess = (p.0 + (t - from_t) * vel.0, p.1 + (t - from_t) * vel.1);
                let step = (guess.0 - p.0).hypot(guess.1 - p.1);
                let exact = foot(surface, spans, ed.curve.value(t), wrap(p)).map(|q| unwrap(q, p));
                match exact {
                    Some(q) if (q.0 - guess.0).hypot(q.1 - guess.1) <= 10.0 * step + 1e-9 => q,
                    _ => guess,
                }
            };
            ends.push((
                end(t0, t_first, p_first, v_first),
                end(t1, t_last, p_last, v_last),
            ));
        }
        for k in 0..ends.len() {
            let from = ends[k].1;
            let to = unwrap(ends[(k + 1) % ends.len()].0, from);
            // Across a closed B-spline surface's seam the two ends are one point.
            let across = |x: f64, near: f64, span: f64| {
                if span.is_finite() {
                    x - span * ((x - near) / span).round()
                } else {
                    x
                }
            };
            let to = (across(to.0, from.0, spans.0), across(to.1, from.1, spans.1));
            let (gu, gv) = (to.0 - from.0, to.1 - from.1);
            if gu.hypot(gv) < 1e-12 {
                continue;
            }
            let mut gap_mass: Estimate = (0.0, 0.0);
            let mut eval = |(): (), a: f64, b: f64, _: bool| {
                let (panel, mass) = kronrod_rule(
                    (),
                    a,
                    b,
                    |(), s| term(from.0 + gu * s, from.1 + gv * s, gu, gv),
                    rule,
                )?;
                gap_mass.0 = gap_mass.0.max(mass.0);
                gap_mass.1 += mass.1;
                Some((panel, None))
            };
            let pieces = (gu.hypot(gv).ceil() as usize).max(1);
            let (sum, excess, size, ()) =
                adaptive(&cuts(0.0, 1.0, pieces, &[])?, (), floor, true, &mut eval)?;
            mass_estimate.0 = mass_estimate.0.max(gap_mass.0);
            mass_estimate.1 += gap_mass.1;
            add((sum, excess, size, gv * (from.0 + 0.5 * gu - u_ref)));
            if floor.is_some_and(|floor| excess > floor) {
                return Some((total, mass_estimate));
            }
        }
        Some((total, mass_estimate))
    }

    /// +1 when the face lies to the left of loop *i* walked in its listed direction (so the
    /// walk runs anticlockwise round the face's parameter domain), −1 when to the right.
    ///
    /// A loop closed in parameter space has the face on its left when it runs anticlockwise
    /// round the outer boundary or clockwise round a hole; `swept` is the signed parameter area
    /// its walk encloses (the walk's own lift of the loop, so it agrees with the integrals). A
    /// loop running round a periodic direction (`None`) encloses nothing, so step either side
    /// of its longest edge used once by the face.
    pub(crate) fn loop_direction(&self, face: usize, i: usize, swept: Option<f64>) -> Option<f64> {
        if let Some(swept) = swept {
            let (outer, _) = self.loop_placement(face)?;
            let anticlockwise = if swept > 0.0 { 1.0 } else { -1.0 };
            return Some(if i == outer {
                anticlockwise
            } else {
                -anticlockwise
            });
        }
        let fc = &self.faces[face];
        let surface = &fc.surface;
        let spans = closed_spans(surface);
        let domain = self.domain(face)?;
        let uses = |e: usize| {
            fc.loops
                .iter()
                .flat_map(|l| &l.edges)
                .filter(|x| x.0 == e)
                .count()
        };
        let mut edges: Vec<(usize, bool)> = fc.loops[i]
            .edges
            .iter()
            .copied()
            .filter(|&(e, _)| uses(e) == 1)
            .collect();
        let length = |e: usize| {
            let s = &self.edges[e].samples;
            s.windows(2).map(|w| geom::dist(w[0], w[1])).sum::<f64>()
        };
        edges.sort_by(|a, b| length(b.0).total_cmp(&length(a.0)));
        let (du_range, dv_range) = (domain.u_range(), domain.v_range());
        let scale = (du_range.1 - du_range.0)
            .max(dv_range.1 - dv_range.0)
            .max(1e-9);
        for (e, forward) in edges {
            let ed = &self.edges[e];
            let (t0, t1) =
                edge_interval(&ed.curve, ed.start, ed.end, ed.same_sense, ed.is_closed());
            let t = 0.5 * (t0 + t1);
            let Some((u, v)) = surface.parameters(ed.curve.value(t), None) else {
                continue;
            };
            let (mut du, mut dv) = uv_velocity(
                surface,
                spans,
                u,
                v,
                ed.curve.value(t),
                ed.curve.derivative(t),
            );
            if (t1 >= t0) != forward {
                (du, dv) = (-du, -dv);
            }
            let norm = du.hypot(dv);
            if norm == 0.0 {
                continue;
            }
            for step in [1e-6, 1e-4] {
                let (nu, nv) = (-dv / norm * step * scale, du / norm * step * scale);
                let left = domain.contains(u + nu, v + nv);
                let right = domain.contains(u - nu, v - nv);
                if left != right {
                    return Some(if left { 1.0 } else { -1.0 });
                }
            }
        }
        None
    }
}

/// A face's area, first and second moments.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceMoments {
    pub area: f64,
    /// The area-weighted mean point of the face (on a curved face, generally off it).
    pub centroid: V3,
    /// ∬ n dA with n the outward unit normal: the face's projected area along each axis. Zero for
    /// a closed face (a whole sphere) and for a full cylinder band.
    pub area_vector: V3,
    /// Central second moments ∫ (pᵢ − centroidᵢ)(pⱼ − centroidⱼ) dA, in mm⁴.
    /// The inertia tensor is trace(second_moments) I − second_moments.
    pub second_moments: [[f64; 3]; 3],
}

impl Part {
    /// Signed orthogonal projected area and its centroid in space, along unit `direction`.
    /// This integrates `n·direction` rather than surface area. Overlapping sheets of a
    /// folded projection cancel. Used to measure a skin's adjoining edge-treatment strips.
    pub fn face_projected_moments(&self, face: usize, direction: V3) -> Option<(f64, V3)> {
        let fc = &self.faces[face];
        let origin = fc.loops.iter().find_map(|lp| {
            lp.vertex
                .or_else(|| lp.edges.first().map(|&(e, _)| self.edges[e].start))
        })?;
        let [area, x, y, z] = self.moment_integral(face, [4, 5, 6, 7, 8], |p, su, sv| {
            let n = geom::cross(su, sv);
            let w = geom::dot(n, direction);
            let q = geom::sub(p, origin);
            [
                w,
                w * q[0],
                w * q[1],
                w * q[2],
                geom::norm(n),
                geom::dot(p, n),
                n[0],
                n[1],
                n[2],
            ]
        })?;
        if !area.is_finite() || area.abs() <= f64::EPSILON {
            return None;
        }
        let centroid = geom::add(origin, [x / area, y / area, z / area]);
        centroid
            .iter()
            .all(|x| x.is_finite())
            .then_some((if fc.reversed { -area } else { area }, centroid))
    }

    /// The face's area, centroid, outward area vector and central second moments,
    /// by the same boundary quadrature as
    /// [`Part::face_mass`]; `None` where that cannot integrate the face.
    /// Cached, including `None`, for this immutable part's geometry and placement.
    pub fn face_moments(&self, face: usize) -> Option<FaceMoments> {
        *self.cache[face]
            .moments
            .get_or_init(|| self.compute_face_moments(face))
    }

    fn compute_face_moments(&self, face: usize) -> Option<FaceMoments> {
        let fc = &self.faces[face];
        let surface = &fc.surface;
        if let Surface::Sphere { frame, radius } = surface
            && self.whole_sphere(face)
        {
            return Some(FaceMoments {
                area: 4.0 * std::f64::consts::PI * radius * radius,
                centroid: frame.origin,
                area_vector: [0.0; 3],
                second_moments: std::array::from_fn(|i| {
                    std::array::from_fn(|j| {
                        if i == j {
                            4.0 * std::f64::consts::PI * radius.powi(4) / 3.0
                        } else {
                            0.0
                        }
                    })
                }),
            });
        }
        // Integrate dimensionless offsets from a point on the face, avoiding subtraction of
        // large world-coordinate moments. Scaling puts every component on the area scale for
        // quadrature's error test, including faces with a constant area density.
        // A boundary vertex moves rigidly with the face. Existing edge samples give a cheap
        // length scale without asking for extrema (which require surface inversion and can
        // cost more than the integral itself on B-splines).
        let origin = fc.loops.iter().find_map(|lp| {
            lp.vertex
                .or_else(|| lp.edges.first().map(|&(e, _)| self.edges[e].start))
        })?;
        let scale = fc
            .loops
            .iter()
            .flat_map(|lp| {
                lp.edges
                    .iter()
                    .flat_map(|&(e, _)| self.edges[e].samples.iter().copied())
                    .chain(lp.vertex)
            })
            .map(|p| {
                let d = geom::sub(p, origin);
                geom::dot(d, d)
            })
            .fold(0.0, f64::max)
            .sqrt();
        if !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        let [area, mx, my, mz, nx, ny, nz, xx, yy, zz, xy, xz, yz] =
            self.moment_integral(face, [0, 13, 4, 5, 6], |p, su, sv| {
                let n = geom::cross(su, sv);
                let w = geom::norm(n);
                let [x, y, z] = geom::scale(geom::sub(p, origin), 1.0 / scale);
                [
                    w,
                    x * w,
                    y * w,
                    z * w,
                    n[0],
                    n[1],
                    n[2],
                    x * x * w,
                    y * y * w,
                    z * z * w,
                    x * y * w,
                    x * z * w,
                    y * z * w,
                    geom::dot(p, n),
                ]
            })?;
        if area <= 0.0 || !area.is_finite() {
            return None;
        }
        let mean = [mx / area, my / area, mz / area];
        let raw = [[xx, xy, xz], [xy, yy, yz], [xz, yz, zz]];
        let second_moments = std::array::from_fn(|i| {
            std::array::from_fn(|j| (raw[i][j] - area * mean[i] * mean[j]) * scale * scale)
        });
        let centroid = geom::add(origin, geom::scale(mean, scale));
        let area_vector = [nx, ny, nz].map(|c| if fc.reversed { -c } else { c });
        if !centroid
            .iter()
            .chain(area_vector.iter())
            .chain(second_moments.iter().flatten())
            .all(|x| x.is_finite())
        {
            return None;
        }
        Some(FaceMoments {
            area,
            centroid,
            area_vector,
            second_moments,
        })
    }

    /// The volume and centre of mass of all the part's solids together (`BRepGProp::
    /// VolumeProperties` of the solids), or `None` where a face cannot be integrated; a void's
    /// faces subtract, as in [`Part::solid_mass`].
    ///
    /// About a point c, the volume is ⅓∯ (p − c)·n dA and ∭ (p − c) dV is ¼∯ (p − c)((p − c)·n) dA
    /// (the divergence of (p − c)((p − c)·e) is 4(p − c)·e). Both fields turn with the part, so
    /// what the faces leave out where they do not quite close (cracks between B-spline faces)
    /// turns with it too, and depends on c only through c's place in the part: c is taken as
    /// the centroid itself, the fixed point of c ↦ c + moment(c)/volume(c), found from one pass
    /// over the faces (the moments about any c are a quadratic in c), and the volume is the one
    /// about it. The centroid then moves with the part: over the corpus and the fixtures, to 1e-9
    /// of the part's size under a generic rotation (`tests/frames.rs`), where measured about a
    /// fixed point it moved by up to 0.08 mm on parts with such cracks.
    pub fn volume_centroid(&self) -> Option<(f64, V3)> {
        // Integrated about c0 (the box centre, inside the part) for conditioning: with
        // q = p − c0, s = ∯ q·n, m = ∯ n, a[i][j] = ∯ qᵢ nⱼ, b = ∯ q (q·n).
        let bounds = self.bounds();
        let c0 = bounds.centre();
        let length = geom::dist(bounds.min, bounds.max);
        if !length.is_finite() || length <= 0.0 {
            return None;
        }
        let (mut s, mut m, mut a, mut b) = (0.0, [0.0; 3], [[0.0; 3]; 3], [0.0; 3]);
        for solid in &self.solids {
            for &face in &solid.faces {
                let fc = &self.faces[face];
                let surface = &fc.surface;
                let sense = if fc.reversed { -1.0 } else { 1.0 };
                if let Surface::Sphere { frame, radius } = surface
                    && self.whole_sphere(face)
                {
                    // A closed ball of signed volume v: s = 3v, m = 0, a = v I, b = 4v (centre − c0).
                    let direct = if frame.direct() { 1.0 } else { -1.0 };
                    let v = sense * direct * 4.0 / 3.0 * std::f64::consts::PI * radius.powi(3);
                    s += 3.0 * v;
                    for i in 0..3 {
                        a[i][i] += v;
                        b[i] += 4.0 * v * (frame.origin[i] - c0[i]);
                    }
                    continue;
                }
                // Put the flux and its moments on the area scale too. Unscaled q² terms
                // amplify projection round-off on tiny faces far from c0: cgb242 face 483
                // was refused only after translation, although its area was resolved.
                // Restore physical units after quadrature; the fixed point below uses mm.
                let f = self.moment_integral::<18, 17>(face, [0, 17, 2, 3, 4], |p, su, sv| {
                    let n = geom::cross(su, sv);
                    let q = geom::scale(geom::sub(p, c0), 1.0 / length);
                    let qn = geom::dot(q, n);
                    let mut out = [0.0; 18];
                    out[0] = geom::norm(n);
                    out[1] = qn;
                    out[17] = geom::dot(p, n);
                    for i in 0..3 {
                        out[2 + i] = n[i];
                        out[14 + i] = q[i] * qn;
                        for j in 0..3 {
                            out[5 + 3 * i + j] = q[i] * n[j];
                        }
                    }
                    out
                })?;
                s += sense * f[1] * length;
                for i in 0..3 {
                    m[i] += sense * f[2 + i];
                    b[i] += sense * f[14 + i] * length * length;
                    for j in 0..3 {
                        a[i][j] += sense * f[5 + 3 * i + j] * length;
                    }
                }
            }
        }
        // About c = c0 + d: volume (s − d·m)/3, moment (b − a d − s d + d (d·m))/4.
        let volume = |d: V3| (s - geom::dot(d, m)) / 3.0;
        let moment = |d: V3| {
            let (dm, ad) = (geom::dot(d, m), [0, 1, 2].map(|i| geom::dot(a[i], d)));
            [0, 1, 2].map(|i| (b[i] - ad[i] - s * d[i] + d[i] * dm) / 4.0)
        };
        let mut d = [0.0; 3];
        for _ in 0..50 {
            let step = moment(d).map(|x| x / volume(d));
            d = geom::add(d, step);
            if geom::norm(step) <= 1e-15 * (1.0 + geom::norm(d)) {
                break;
            }
        }
        Some((volume(d), geom::add(c0, d)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mass_convergence_is_independent_of_moment_density() {
        // The requested moments are constant, but a mass flux oscillates. Checking only
        // the moments would accept this panel and lose the standalone mass refusal.
        let (panel, mass) = kronrod_rule(
            (),
            0.0,
            1.0,
            |(), t| Some([1.0, 1.0, (100.0 * t).sin(), 0.0, 0.0, 0.0]),
            IntegralRule {
                components: Components::Prefix(2),
                observe_mass: Some([0, 2, 3, 4, 5]),
            },
        )
        .unwrap();
        assert!(panel.1 < 1e-14);
        assert!(mass.0 > 1e-6);
        assert_eq!(mass.1, panel.2);
    }

    #[test]
    fn gauss_rule_is_exact_for_polynomials() {
        let pi = std::f64::consts::PI;
        let [x7, s] = integrate(&cuts(0.0, pi, 3, &[1.0]).unwrap(), |x| [x.powi(7), x.sin()]);
        assert!((x7 - pi.powi(8) / 8.0).abs() < 1e-11);
        assert!((s - 2.0).abs() < 1e-15);
        assert_eq!(
            cuts(2.0, 0.0, 2, &[0.5, 3.0]),
            Some(vec![2.0, 1.0, 0.5, 0.0])
        );
        // Past the bound the integral is refused, not coarsened (#52).
        assert_eq!(cuts(0.0, 1e300, usize::MAX, &[]), None);
        assert!(cuts(0.0, 1.0, MAX_PANELS, &[]).is_some());
    }

    #[test]
    fn kronrod_rule_and_its_gauss_rule_are_exact_to_their_degrees() {
        // The 21-point Kronrod rule integrates degree 31 exactly, its 10-point Gauss rule 19.
        let (sum, error, _, ()) = kronrod((), -1.0, 1.0, |(), x| Some([x.powi(30)])).unwrap();
        assert!((sum[0] - 2.0 / 31.0).abs() < 1e-15);
        assert!(error > 1e-6);
        let (sum, error, _, ()) = kronrod((), 0.0, 2.0, |(), x| Some([x.powi(19)])).unwrap();
        assert!((sum[0] - 2f64.powi(20) / 20.0).abs() < 1e-9);
        assert!(error < 1e-9);
        // Halving where the integrand turns a corner: |x − 1/3| on [0, 1].
        let mut eval = |(): (), a: f64, b: f64, _: bool| {
            let panel = kronrod((), a, b, |(), x| Some([(x - 1.0 / 3.0).abs()]))?;
            Some((panel, None))
        };
        let (sum, ..) = adaptive(&[0.0, 1.0], (), Some(0.0), false, &mut eval).unwrap();
        assert!((sum[0] - 5.0 / 18.0).abs() < 1e-9);
        let mut split = |(): (), a: f64, b: f64, _: bool| {
            let panel = kronrod((), a, b, |(), x| Some([(x - 1.0 / 3.0).abs()]))?;
            Some((panel, Some((1.0 / 3.0, false))))
        };
        let (sum, ..) = adaptive(&[0.0, 1.0], (), Some(0.0), false, &mut split).unwrap();
        assert!((sum[0] - 5.0 / 18.0).abs() < 1e-15);
    }

    #[test]
    fn moments_can_need_refinement_when_area_is_constant() {
        let mut eval = |(): (), a: f64, b: f64, _: bool| {
            Some((
                kronrod((), a, b, |(), x| Some([1.0, (100.0 * x).cos()]))?,
                None,
            ))
        };
        let (_, error, _, ()) = eval((), -1.0, 1.0, false).unwrap().0;
        assert!(
            error > 1e-3,
            "the constant area must not hide the moment error"
        );
        let (sum, error, _, ()) =
            adaptive(&[-1.0, 1.0], (), Some(1e-12), false, &mut eval).unwrap();
        assert!((sum[0] - 2.0).abs() < 1e-14);
        assert!((sum[1] - 2.0 * 100f64.sin() / 100.0).abs() < 1e-12);
        assert!(error < 1e-12);
    }

    /// A kink named just inside every panel, never resolved (as an inversion jumping between
    /// branches along a boundary names one, MFInstSeg 20221123_142528_16681's face 15): the
    /// interval's kink budget bounds the panels, where a per-panel one let both halves of every
    /// split spend it again.
    #[test]
    fn a_kink_named_in_every_panel_spends_one_budget() {
        let mut evals = 0usize;
        let mut eval = |(): (), a: f64, b: f64, _: bool| {
            evals += 1;
            let w = (b - a).abs();
            Some((([b - a], w, w, ()), Some((a + 1e-3 * (b - a), false))))
        };
        adaptive(&[0.0, 1.0], (), Some(0.0), false, &mut eval).unwrap();
        assert!(
            evals <= (MAX_KINKS + 1) << (MAX_DEPTH + 1),
            "{evals} panels"
        );
    }
    #[test]
    fn unused_kink_locations_do_not_change_quadrature_or_walk_state() {
        let run = |floor, always_locate| {
            let (mut located, mut panels) = (0usize, 0usize);
            let mut eval = |state: usize, a: f64, b: f64, requested: bool| {
                panels += 1;
                let kink = (always_locate || requested).then(|| {
                    located += 1;
                    (a + 1e-3 * (b - a), false)
                });
                let width = (b - a).abs();
                Some((([b - a], width, width, state + 1), kink))
            };
            let answer = adaptive(&[0.0, 1.0], 0, floor, false, &mut eval).unwrap();
            (answer, located, panels)
        };
        for floor in [None, Some(0.0)] {
            let eager = run(floor, true);
            let requested = run(floor, false);
            assert_eq!(eager.0.0.map(f64::to_bits), requested.0.0.map(f64::to_bits));
            assert_eq!(eager.0.1.to_bits(), requested.0.1.to_bits());
            assert_eq!(eager.0.2.to_bits(), requested.0.2.to_bits());
            assert_eq!(eager.0.3, requested.0.3);
            assert_eq!(eager.2, requested.2, "same quadrature panels");
            assert!(requested.1 < eager.1);
            // A location rounded onto an endpoint spends no split budget, so the
            // number of requested locations need not be bounded by MAX_KINKS.
            if floor.is_none() {
                assert_eq!(requested.1, 0);
            } else {
                assert!(requested.1 > 0);
            }
        }
    }

    fn moment_bits(m: FaceMoments) -> Vec<u64> {
        std::iter::once(m.area)
            .chain(m.centroid)
            .chain(m.area_vector)
            .chain(m.second_moments.into_iter().flatten())
            .map(f64::to_bits)
            .collect()
    }

    #[test]
    fn moments_reuse_complete_results_and_invalidate_on_motion() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/rejected_cylinder.step");
        let part = crate::step::read_step_file(&path).unwrap();
        for face in 0..part.faces.len() {
            assert!(part.cache[face].moments.get().is_none());
            let direct = part.compute_face_moments(face).map(moment_bits);
            let first = part.face_moments(face).map(moment_bits);
            assert_eq!(first, direct);
            assert!(part.cache[face].moments.get().is_some());
            assert_eq!(part.face_moments(face).map(moment_bits), first);
        }
        let mut placement = crate::step::IDENTITY;
        placement[0][3] = 127.0;
        placement[1][3] = -43.0;
        let moved = part.transformed(&placement).unwrap();
        for face in 0..moved.faces.len() {
            assert!(moved.cache[face].moments.get().is_none());
            let direct = moved.compute_face_moments(face).map(moment_bits);
            assert_eq!(moved.face_moments(face).map(moment_bits), direct);
        }
    }

    #[test]
    fn unmeasurable_moments_are_cached_as_missing() {
        let part = Part::new(
            vec![crate::brep::Face {
                surface: Surface::Other { kind: "unresolved" },
                reversed: false,
                loops: Vec::new(),
                solid: None,
                pcurves: Vec::new(),
            }],
            Vec::new(),
            Vec::new(),
        );
        assert!(part.cache[0].moments.get().is_none());
        assert_eq!(part.face_moments(0), None);
        assert_eq!(part.cache[0].moments.get(), Some(&None));
        assert_eq!(part.face_moments(0), None);
    }
    #[test]
    fn inevitable_refusal_stops_without_changing_accepted_integrals() {
        let run = |floor, relative_error, reject_unresolved| {
            let mut panels = 0usize;
            let mut eval = |state: usize, a: f64, b: f64, _: bool| {
                panels += 1;
                let width = (b - a).abs();
                Some((([b - a], relative_error * width, width, state + 1), None))
            };
            let answer = adaptive(&[0.0, 1.0], 0, floor, reject_unresolved, &mut eval);
            (answer, panels)
        };
        for relative_error in [1.0, 1e-10] {
            for floor in [
                None,
                Some(0.0),
                Some(1e-4),
                Some(1e-3),
                Some(1.0),
                Some(f64::INFINITY),
                Some(f64::NAN),
            ] {
                let (exhaustive, all_panels) = run(floor, relative_error, false);
                let expected = exhaustive.filter(|p| !floor.is_some_and(|floor| p.1 > floor));
                let (early, early_panels) = run(floor, relative_error, true);
                let early = early.filter(|p| !floor.is_some_and(|floor| p.1 > floor));
                assert_eq!(early, expected, "floor={floor:?}, error={relative_error}");
                if expected.is_none() {
                    assert!(early_panels < all_panels);
                    assert_eq!(early_panels, MAX_DEPTH + 1);
                } else {
                    assert_eq!(early_panels, all_panels);
                    assert_eq!(
                        early.unwrap().0.map(f64::to_bits),
                        exhaustive.unwrap().0.map(f64::to_bits)
                    );
                }
            }
        }
    }
    #[test]
    fn a_bad_parent_is_refined_and_relative_acceptance_is_preserved() {
        for descending in [false, true] {
            for relative in [false, true] {
                let cuts = if descending {
                    [1.0, 0.0, -1.0]
                } else {
                    [-1.0, 0.0, 1.0]
                };
                let run = |early| {
                    let mut visited = Vec::new();
                    let mut eval = |state: usize, a: f64, b: f64, _: bool| {
                        visited.push((a.to_bits(), b.to_bits(), state));
                        let width = (b - a).abs();
                        // A large parent estimate disappears after a single split.
                        // In the other case the error exceeds the face floor but
                        // meets the relative criterion and must not be rejected.
                        let (error, size) = if relative {
                            (2.0, 3e9)
                        } else {
                            (if width > 0.75 { 2.0 } else { 0.0 }, width)
                        };
                        Some((
                            ([b - a, (b * b - a * a) * 0.5], error, size, state + 1),
                            None,
                        ))
                    };
                    (
                        adaptive(&cuts, 0, Some(1e-12), early, &mut eval).unwrap(),
                        visited,
                    )
                };
                let full = run(false);
                let early = run(true);
                assert_eq!(early, full);
                assert_eq!(early.0.0[0], if descending { -2.0 } else { 2.0 });
                assert_eq!(early.0.1, 0.0);
                assert_eq!(early.1.len(), if relative { 2 } else { 6 });
            }
        }
    }

    #[test]
    fn the_floor_comparison_is_strict_and_an_unresolved_prefix_cannot_be_published() {
        let floor = 1.0f64;
        for error in [
            f64::from_bits(floor.to_bits() - 1),
            floor,
            f64::from_bits(floor.to_bits() + 1),
        ] {
            let run = |early| {
                let mut panels = 0;
                let mut eval = |state: usize, a: f64, b: f64, _: bool| {
                    panels += 1;
                    Some((([b - a], error, 1.0, state + 1), None))
                };
                (
                    adaptive(&[0.0, 1.0, 2.0], 0, Some(floor), early, &mut eval).unwrap(),
                    panels,
                )
            };
            let (full, full_panels) = run(false);
            let (prefix, prefix_panels) = run(true);
            assert_eq!(prefix.1.to_bits(), full.1.to_bits());
            assert_eq!(prefix.1 > floor, error > floor);
            if error > floor {
                assert_eq!(prefix_panels, MAX_DEPTH + 1);
                assert!(prefix_panels < full_panels);
                assert_ne!(prefix.0, full.0, "the partial sum must never be published");
            } else {
                assert_eq!(prefix, full);
                assert_eq!(prefix_panels, full_panels);
            }
        }
    }
}
