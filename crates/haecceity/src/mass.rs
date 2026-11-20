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

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

use super::brep::Part;
use super::geom::{self, Curve, Surface, V3};
use super::nurbs::breaks;
use super::sampling::{edge_interval, extreme_parameters};
use super::uv::UvLoop;

fn bisect_cut(mut lo: f64, mut hi: f64, before_cut: impl Fn(f64) -> bool) -> f64 {
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        let endpoint = if before_cut(mid) { &mut lo } else { &mut hi };
        // A fixed bracket gives the same midpoint and the same decision on every
        // remaining step. Compare bits so signed zero cannot hide an interval change.
        if mid.to_bits() == endpoint.to_bits() {
            return mid;
        }
        *endpoint = mid;
    }
    0.5 * (lo + hi)
}

const MAX_CACHED_CUTS: usize = 2048;

/// Cut searches within one legacy integral of a face. The surface, its parameter
/// domain, and its edges are fixed across the coarse and refined passes. The edge
/// index, ordered bracket and original UV seed then determine the entire search,
/// including how a failed projection contributes to the bracket.
#[derive(Default)]
struct CutCache {
    cuts: HashMap<(usize, [u64; 4]), f64>,
}

impl CutCache {
    fn search(
        &mut self,
        edge: usize,
        bracket: (f64, f64),
        seed: (f64, f64),
        find: impl FnOnce() -> f64,
    ) -> f64 {
        let parameters = [bracket.0, bracket.1, seed.0, seed.1].map(f64::to_bits);
        let key = (edge, parameters);
        if let Some(&cut) = self.cuts.get(&key) {
            return cut;
        }
        let cut = find();
        if self.cuts.len() < MAX_CACHED_CUTS {
            self.cuts.insert(key, cut);
        }
        cut
    }
}

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

/// The face's shared moment integration, one result per consumer (`Part::face_raw_moments`).
#[derive(Clone, Copy, Debug)]
pub(super) struct RawMoments {
    local: Option<[f64; 26]>,
    centre: Option<[f64; 26]>,
}

#[cfg(test)]
thread_local! {
    /// Test hook: `Some(true)` makes the face-moment group's integrand non-finite,
    /// `Some(false)` the centroid group's.
    static POISONED_GROUP: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
    /// Integrand evaluations of face integrals on this thread: a structural cost for tests.
    static INTEGRAND_EVALUATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Knot spans in u from which a B-spline face's inner integral may run along v instead (#302).
const LARGE_NET_SPANS: usize = 32;

/// Sub-panels each v knot span is cut into by the cheaper form's second trial.
const REFINED_PANELS: usize = 4;

/// How small a node's motion across its inner integral's parameter, against its motion along
/// the other, is not counted (round-off in an edge along a parameter line of the surface).
const NEGLIGIBLE_MOTION: f64 = 1e-12;

/// [`integrate`], also reporting whether every panel's rule agrees with the same rule on its
/// two halves to 1e-9 of the terms' magnitudes in each component (and 1e-12 of the largest component's): a density with a narrow
/// dip (a strip whose normal nearly reverses) fails it, and its caller then uses the other
/// form. The sum is the whole-panel rule's, as [`integrate`]'s.
fn integrate_checked<const N: usize>(
    cuts: &[f64],
    f: impl Fn(f64) -> [f64; N],
) -> ([f64; N], bool) {
    let mut total = [0.0; N];
    let mut resolved = true;
    let rule = |a: f64, b: f64, y: &mut [f64; N], size: &mut [f64; N]| {
        let (half, centre) = (0.5 * (b - a), 0.5 * (b + a));
        for &(x, weight) in gauss_nodes() {
            let v = f(centre + half * x);
            for k in 0..N {
                y[k] += weight * half * v[k];
                size[k] += (weight * half * v[k]).abs();
            }
        }
    };
    for w in cuts.windows(2) {
        let (mut whole, mut halves, mut size) = ([0.0; N], [0.0; N], [0.0; N]);
        rule(w[0], w[1], &mut whole, &mut [0.0; N]);
        let mid = 0.5 * (w[0] + w[1]);
        rule(w[0], mid, &mut halves, &mut size);
        rule(mid, w[1], &mut halves, &mut size);
        // A component whose terms are all round-off beside the others' (a coordinate near
        // zero) cannot be judged against its own size.
        let largest = size.iter().fold(0.0f64, |m, &x| m.max(x));
        for k in 0..N {
            total[k] += whole[k];
            resolved &= (whole[k] - halves[k]).abs() <= 1e-9 * size[k] + 1e-12 * largest;
        }
    }
    (total, resolved && total.iter().all(|x| x.is_finite()))
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

/// `cuts` with each panel divided into `parts` equal ones.
fn split_panels(cuts: &[f64], parts: usize) -> Vec<f64> {
    let mut out = vec![cuts[0]];
    for w in cuts.windows(2) {
        // Each panel ends exactly where it did.
        out.extend((1..parts).map(|k| w[0] + (w[1] - w[0]) * k as f64 / parts as f64));
        out.push(w[1]);
    }
    out
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

/// Whether a loop on a surface closed in u (across `span`) winds round it: its parameters, each
/// step brought to the nearest whole span, add up to more than half of one; or a step is so long
/// (over a quarter span) that the way round it is not certain.
fn winds_round(lp: &UvLoop, span: f64) -> bool {
    if !span.is_finite() {
        return false;
    }
    let mut net = 0.0;
    for w in lp.points.windows(2) {
        let d = w[1].0 - w[0].0;
        let d = d - span * (d / span).round();
        if d.abs() > 0.25 * span {
            return true;
        }
        net += d;
    }
    net.abs() > 0.5 * span
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

/// The distinct knots (`knots`: the domain's, ascending) either side of `x`: the greatest
/// not above it and the least above it, or the first two (the last two) distinct knots where
/// `x` is below (not below) them all. The pair [`breaks`] would give, found by search.
fn piece_of(knots: &[f64], x: f64) -> (f64, f64) {
    let above = |t: f64| knots.partition_point(|&k| k <= t);
    let j = above(x);
    if j == 0 {
        (knots[0], knots[above(knots[0])])
    } else if j == knots.len() {
        let hi = knots[j - 1];
        (knots[knots.partition_point(|&k| k < hi) - 1], hi)
    } else {
        (knots[j - 1], knots[j])
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
                piece_of(&knots[degree..knots.len() - degree], x)
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

/// A completed but invalid mass sum must retain standalone mass's refusal;
/// only failure to resolve the legacy quadrature permits a master-trim retry.
enum MassPass {
    Refused,
    Evaluated(Option<[f64; 5]>),
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
        if let Some(value) = self.integrate_face_legacy(face, density, rule, &mut None, true) {
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
        remember: bool,
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
                remember,
            );
            match observed {
                Some(MassPass::Evaluated(terms)) => FaceMass::legacy(terms),
                Some(MassPass::Refused) => self.recover_face_mass(face),
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
            remember,
        )
    }

    /// [`Self::integrate_face_legacy_in`], first by the form of Green's theorem whose inner
    /// integral crosses fewer knot spans where that form is eligible (a face with one very
    /// large B-spline net in u and a few spans in v, #302), then, should it refuse or fail
    /// its own check, by the stock form. Only a completed, checked result of the cheaper form
    /// is returned. A face whose one rule per knot span fails the check is tried once more
    /// with each span cut into [`REFINED_PANELS`] (its area density is a high-degree
    /// polynomial in v, which one 16-point rule does not resolve), against the same check. A
    /// face both refuse is remembered in the face's cache when `remember` (the integrand is
    /// fixed by the face and the rule: not one that depends on a caller's argument), keyed by
    /// its width and whether mass is observed, so no later like integration repeats the trials.
    fn integrate_face_legacy<const N: usize>(
        &self,
        face: usize,
        f: impl Fn(V3, V3, V3) -> [f64; N],
        rule: IntegralRule,
        observed: &mut Option<MassPass>,
        remember: bool,
    ) -> Option<[f64; N]> {
        let key = (N, rule.observe_mass.is_some());
        let remembered = remember && self.cache[face].trial_refused(key);
        if self.cheaper_inner_form(face) && !remembered {
            for split in [1, REFINED_PANELS] {
                let unresolved = std::cell::Cell::new(false);
                let mut trial = None;
                let swapped = self.integrate_face_legacy_in(
                    face,
                    &f,
                    rule,
                    &mut trial,
                    Some((&unresolved, split)),
                );
                if swapped.is_some()
                    && !unresolved.get()
                    && matches!(trial, None | Some(MassPass::Evaluated(Some(_))))
                {
                    *observed = trial;
                    return swapped;
                }
                // Only a panel the half-panel check found unresolved is worth cutting finer.
                if !unresolved.get() {
                    break;
                }
            }
            if remember {
                self.cache[face].refuse_trial(key);
            }
        }
        self.integrate_face_legacy_in(face, f, rule, observed, None)
    }

    /// Whether the face's inner integral can run along v, where it crosses far fewer panels
    /// than along u: an unpinched B-spline face, not closed in v, whose u has at least
    /// [`LARGE_NET_SPANS`] spans and eight times those of v; or a cylinder or cone (one panel
    /// along v) whose loops, closed in the lifted parameters, run through at least
    /// [`LARGE_NET_SPANS`] radians of u (a thread's helical edge, which along u costs a panel
    /// per radian at every boundary node).
    fn cheaper_inner_form(&self, face: usize) -> bool {
        let surface = &self.faces[face].surface;
        match surface {
            Surface::Freeform { surface: s, .. } => {
                if surface.periodic() != (false, false) {
                    return false;
                }
                // Closed in u is allowed (the integrand wraps the lifted parameters across the
                // seam, as the stock form's does); a loop that winds round u never reaches the
                // cheaper form in `integrate_face_legacy_in`, which runs its own along-v form.
                let (_, sv) = closed_spans(surface);
                if sv.is_finite() || collapsed_sides(surface) != (None, None) {
                    return false;
                }
                let nu = breaks(&s.knots_u, s.degree_u).len();
                let nv = breaks(&s.knots_v, s.degree_v).len();
                nu >= LARGE_NET_SPANS && nu >= 8 * nv
            }
            Surface::Cylinder { .. } | Surface::Cone { .. } => {
                let (Some(loops), Some(domain)) = (self.uv_loops(face), self.domain(face)) else {
                    return false;
                };
                let (lo, hi) = domain.v_range();
                if [lo, hi]
                    .into_iter()
                    .any(|v| surface.singular_v(v).is_some())
                {
                    return false;
                }
                let us = loops.iter().flat_map(|l| l.points.iter().map(|p| p.0));
                let (min, max) = us.fold((f64::MAX, f64::MIN), |(a, b), u| (a.min(u), b.max(u)));
                max - min >= LARGE_NET_SPANS as f64
            }
            _ => false,
        }
    }

    fn integrate_face_legacy_in<const N: usize>(
        &self,
        face: usize,
        f: impl Fn(V3, V3, V3) -> [f64; N],
        rule: IntegralRule,
        observed: &mut Option<MassPass>,
        cheaper: Option<(&std::cell::Cell<bool>, usize)>,
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
        // The cheaper form reads no loop that winds round a surface closed in u (its parameters
        // wrap at the seam, so the loop's ends agree whatever it encircles): the stock form
        // decides such a face.
        if cheaper.is_some() && uv.iter().any(|lp| winds_round(lp, spans.0)) {
            return None;
        }
        let evaluator = match surface {
            Surface::Freeform { surface, .. } => Some(std::cell::RefCell::new(
                crate::nurbs::SurfaceEvaluation::new(surface),
            )),
            _ => None,
        };
        let f = |u: f64, v: f64| {
            #[cfg(test)]
            INTEGRAND_EVALUATIONS.with(|n| n.set(n.get() + 1));
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
        // The cheaper form (only a face [`Self::cheaper_inner_form`] passed, no loop round u)
        // integrates along v from the loop's first point; the swept area is still measured
        // along u from it, and the loops' direction still reads it.
        let cheaper = cheaper.filter(|_| !along_v);
        let reference = if along_v {
            let (lo, hi) = domain.v_range();
            [lo, hi]
                .into_iter()
                .find_map(|v| surface.singular_v(v).filter(|s| (s - v).abs() < 1e-6))
                .or(collapsed.1)
                .unwrap_or(start.1)
        } else if cheaper.is_some() {
            start.1
        } else {
            collapsed.0.unwrap_or(start.0)
        };
        // (The swept area, read only where the loops do not run round u, is measured from the
        // same reference.)
        let u_ref = if along_v {
            0.0
        } else if cheaper.is_some() {
            start.0
        } else {
            reference
        };
        // A node moving along its inner integral's own parameter line (an edge that is a side
        // of the surface: dv = 0 to round-off for the stock form) weighs nothing; where that
        // integral crosses many knot spans, its terms are not evaluated.
        let inner_spans = |along_v: bool| match surface {
            Surface::Freeform { surface: s, .. } if along_v => breaks(&s.knots_v, s.degree_v).len(),
            Surface::Freeform { surface: s, .. } => breaks(&s.knots_u, s.degree_u).len(),
            _ => 0,
        };
        let (cheap_spans, stock_spans) = (inner_spans(true), inner_spans(false));
        // Motions are compared as shares of their own active parameter ranges (the domain, not
        // the knot vector's ends, which a writer may pad), so a surface whose u and v differ in
        // scale is judged the same as a normalised one.
        let (u_extent, v_extent) = match surface {
            Surface::Freeform { surface: s, .. } => {
                let (u0, u1, v0, v1) = s.domain();
                let extent = |a: f64, b: f64| if b > a { b - a } else { 1.0 };
                (extent(u0, u1), extent(v0, v1))
            }
            _ => (1.0, 1.0),
        };
        // `weight` is the motion across the inner integral's line (u when that line runs along v),
        // `other` the motion along it.
        let negligible = |weight: f64, other: f64, spans: usize, inner_is_v: bool| {
            let (weight_extent, other_extent) = if inner_is_v {
                (u_extent, v_extent)
            } else {
                (v_extent, u_extent)
            };
            spans > LARGE_NET_SPANS
                && weight.abs() * other_extent <= NEGLIGIBLE_MOTION * other.abs() * weight_extent
        };
        let term = |u: f64, v: f64, du: f64, dv: f64| {
            let (weight, y) = if let Some((unresolved, split)) = cheaper {
                if negligible(du, dv, cheap_spans, true) {
                    return Some([0.0; N]);
                }
                let mut cuts = surface_cuts(surface, spans.1, true, reference, v)?;
                if split > 1 {
                    cuts = split_panels(&cuts, split);
                }
                let (y, resolved) = integrate_checked(&cuts, |s| f(u, s));
                if !resolved {
                    unresolved.set(true);
                }
                (-du, y)
            } else if along_v {
                if negligible(du, dv, cheap_spans, true) {
                    return Some([0.0; N]);
                }
                let cuts = surface_cuts(surface, spans.1, true, reference, v)?;
                (-du, integrate(&cuts, |s| f(u, s)))
            } else {
                if negligible(dv, du, stock_spans, false) {
                    return Some([0.0; N]);
                }
                let cuts = surface_cuts(surface, spans.0, false, reference, u)?;
                (dv, integrate(&cuts, |s| f(s, v)))
            };
            Some(y.map(|y| weight * y))
        };
        let loops: Vec<usize> = (0..fc.loops.len())
            .filter(|&i| fc.loops[i].vertex.is_none())
            .collect();
        let cut_cache = (matches!(
            surface,
            Surface::Freeform {
                kind: "BSPLINE" | "BEZIER",
                ..
            }
        ) && spans == (f64::INFINITY, f64::INFINITY)
            && collapsed == (None, None))
            .then(|| RefCell::new(CutCache::default()));
        let cut_cache = cut_cache.as_ref();
        // One rule per panel; then again, refining where needed, for a loop with a panel
        // unresolved at the scale of the whole face's terms.
        let mut passes = Vec::with_capacity(loops.len());
        for &i in &loops {
            passes.push(self.boundary_integral(
                face,
                i,
                shifts[i],
                &term,
                (None, rule, cut_cache),
                u_ref,
            )?);
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
                            cut_cache,
                        ),
                        u_ref,
                    )?;
                    if refined.1 > mass_floor {
                        *observed = Some(MassPass::Refused);
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
            *observed = Some(MassPass::Evaluated(mass));
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
                        cut_cache,
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
        (floor, rule, cut_cache): (Option<f64>, IntegralRule, Option<&RefCell<CutCache>>),
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
                let find = || {
                    let seed = wrap(pa);
                    let side = held(pa);
                    bisect_cut(ta, tb, |mid| {
                        foot(surface, spans, ed.curve.value(mid), seed)
                            .is_some_and(|q| held(unwrap(q, pa)) == side)
                    })
                };
                if let Some(cache) = cut_cache {
                    cache.borrow_mut().search(e, (ta, tb), pa, find)
                } else {
                    find()
                }
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
        let [area, x, y, z] = self.moment_integral(face, [4, 5, 6, 7, 8], false, |p, su, sv| {
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

    /// A point on the face and a length scale for its offsets: a boundary vertex moves rigidly
    /// with the face, and existing edge samples give a cheap length scale without asking for
    /// extrema (which require surface inversion and can cost more than the integral itself on
    /// B-splines).
    fn moment_frame(&self, face: usize) -> Option<(V3, f64)> {
        let fc = &self.faces[face];
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
        (scale.is_finite() && scale > 0.0).then_some((origin, scale))
    }

    /// The face's one moment integration, shared by [`Part::face_moments`] and
    /// [`Part::volume_centroid`] (each used to run its own). Cached, including refusals.
    /// Components, all on the area scale for quadrature's error test:
    /// 0 area w; 1..4 n; 4..7 and 7..13 the first and second moments of w about the face's
    /// own `moment_frame` (dimensionless: offsets over its scale), the face-moment group;
    /// 13 q·n; 14..23 qᵢnⱼ; 23..26 qᵢ(q·n), with q the offset from the box centre over the
    /// part's length (see `volume_centroid`), the centroid group; 26 the flux p·n, integrated
    /// to select the mass boundary and not returned. Each consumer is refused only for its
    /// own group: when the joint integration is refused, each group is integrated alone
    /// (the other's components left zero, which no error test reads), as before they were
    /// shared. A group whose frame is unusable is left zero and its consumer refuses it.
    fn face_raw_moments(&self, face: usize) -> RawMoments {
        *self.cache[face]
            .raw_moments
            .get_or_init(|| self.compute_face_raw_moments(face))
    }

    fn compute_face_raw_moments(&self, face: usize) -> RawMoments {
        if let joint @ Some(_) = self.raw_group_integral(face, true, true) {
            return RawMoments {
                local: joint,
                centre: joint,
            };
        }
        RawMoments {
            local: self.raw_group_integral(face, true, false),
            centre: self.raw_group_integral(face, false, true),
        }
    }

    fn raw_group_integral(&self, face: usize, local: bool, centre: bool) -> Option<[f64; 26]> {
        let frame = self.moment_frame(face).filter(|_| local);
        let bounds = self.bounds();
        let c0 = bounds.centre();
        let length = geom::dist(bounds.min, bounds.max);
        let length = (length.is_finite() && length > 0.0 && centre).then_some(length);
        self.moment_integral::<27, 26>(face, [0, 26, 1, 2, 3], true, |p, su, sv| {
            let n = geom::cross(su, sv);
            let w = geom::norm(n);
            let mut out = [0.0; 27];
            out[0] = w;
            out[1..4].copy_from_slice(&n);
            out[26] = geom::dot(p, n);
            if let Some((origin, scale)) = frame {
                let [x, y, z] = geom::scale(geom::sub(p, origin), 1.0 / scale);
                out[4..13].copy_from_slice(&[
                    x * w,
                    y * w,
                    z * w,
                    x * x * w,
                    y * y * w,
                    z * z * w,
                    x * y * w,
                    x * z * w,
                    y * z * w,
                ]);
            }
            if let Some(length) = length {
                let q = geom::scale(geom::sub(p, c0), 1.0 / length);
                let qn = geom::dot(q, n);
                out[13] = qn;
                for i in 0..3 {
                    out[23 + i] = q[i] * qn;
                    for j in 0..3 {
                        out[14 + 3 * i + j] = q[i] * n[j];
                    }
                }
            }
            #[cfg(test)]
            POISONED_GROUP.with(|g| match g.get() {
                Some(true) if frame.is_some() => out[4] = f64::NAN,
                Some(false) if length.is_some() => out[13] = f64::NAN,
                _ => {}
            });
            out
        })
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
        let (origin, scale) = self.moment_frame(face)?;
        let raw = self.face_raw_moments(face).local?;
        let [area, nx, ny, nz, mx, my, mz, xx, yy, zz, xy, xz, yz] =
            std::array::from_fn(|k| raw[k]);
        if area <= 0.0 || !area.is_finite() {
            return None;
        }
        let mean = [mx / area, my / area, mz / area];
        let second = [[xx, xy, xz], [xy, yy, yz], [xz, yz, zz]];
        let second_moments = std::array::from_fn(|i| {
            std::array::from_fn(|j| (second[i][j] - area * mean[i] * mean[j]) * scale * scale)
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
                // The shared face integration (`face_raw_moments`) holds the flux and its
                // moments on the area scale. Restore physical units after quadrature; the
                // fixed point below uses mm.
                let f = self.face_raw_moments(face).centre?;
                s += sense * f[13] * length;
                for i in 0..3 {
                    m[i] += sense * f[1 + i];
                    b[i] += sense * f[23 + i] * length * length;
                    for j in 0..3 {
                        a[i][j] += sense * f[14 + 3 * i + j] * length;
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
    fn stalled_cut_bisection_preserves_the_fixed_round_result() {
        use std::cell::Cell;

        for (start, end) in [(0.0, 1.0), (1.0, 0.0), (-0.0, 0.0), (1e8, 1e8 + 0.01)] {
            for split in [start, 0.37 * start + 0.63 * end, end] {
                // A failed projection at the split takes the existing false branch.
                let before = |t: f64| t != split && (t < split) == (start < end);
                let (mut lo, mut hi) = (start, end);
                for _ in 0..60 {
                    let mid = 0.5 * (lo + hi);
                    if before(mid) {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                let original = 0.5 * (lo + hi);
                let calls = Cell::new(0);
                let cut = bisect_cut(start, end, |t| {
                    calls.set(calls.get() + 1);
                    before(t)
                });
                assert_eq!(cut.to_bits(), original.to_bits());
                assert!(calls.get() <= 60);
                if start >= 1e8 {
                    assert!(calls.get() < 60);
                }
            }
        }
    }

    #[test]
    fn cut_search_cache_requires_the_same_edge_bracket_and_seed_bits() {
        let mut cache = CutCache::default();
        let first = cache.search(2, (0.0, 1.0), (1.0, 2.0), || -0.0);
        let again = cache.search(2, (0.0, 1.0), (1.0, 2.0), || unreachable!());
        assert_eq!(first.to_bits(), again.to_bits());
        for (edge, bracket, seed) in [
            (3, (0.0, 1.0), (1.0, 2.0)),
            (2, (1.0, 0.0), (1.0, 2.0)),
            (2, (-0.0, 1.0), (1.0, 2.0)),
            (2, (0.0, 1.0), (1.0, 2.0f64.next_up())),
        ] {
            assert_eq!(cache.search(edge, bracket, seed, || 0.5), 0.5);
        }
        for edge in 4..(MAX_CACHED_CUTS + 4) {
            cache.search(edge, (0.0, 1.0), (1.0, 2.0), || 0.0);
        }
        assert_eq!(cache.cuts.len(), MAX_CACHED_CUTS);
        assert_eq!(
            cache
                .search(2, (0.0, 1.0), (1.0, 2.0), || unreachable!())
                .to_bits(),
            first.to_bits()
        );
        assert_eq!(
            cache.search(usize::MAX, (0.0, 1.0), (1.0, 2.0), || 0.25),
            0.25
        );
        assert_eq!(
            cache.search(usize::MAX, (0.0, 1.0), (1.0, 2.0), || 0.75),
            0.75
        );
        assert_eq!(cache.cuts.len(), MAX_CACHED_CUTS);
    }

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

    /// A STEP file under construction: numbered entities, one per line.
    struct Entities {
        text: String,
        last: usize,
    }

    impl Entities {
        fn new() -> Self {
            Self {
                text: String::from(
                    "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('t'),'2;1');\nFILE_NAME('t','',(''),(''),'','','');\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));\nENDSEC;\nDATA;\n",
                ),
                last: 0,
            }
        }

        fn add(&mut self, body: String) -> usize {
            self.last += 1;
            self.text.push_str(&format!("#{} = {body};\n", self.last));
            self.last
        }

        fn point(&mut self, p: [f64; 3]) -> usize {
            self.add(format!(
                "CARTESIAN_POINT('',({:?},{:?},{:?}))",
                p[0], p[1], p[2]
            ))
        }

        fn vertex(&mut self, p: [f64; 3]) -> usize {
            let point = self.point(p);
            self.add(format!("VERTEX_POINT('',#{point})"))
        }

        /// A unit-speed line through `from` along `direction`.
        fn line(&mut self, from: [f64; 3], d: [f64; 3]) -> usize {
            let origin = self.point(from);
            let direction = self.add(format!("DIRECTION('',({:?},{:?},{:?}))", d[0], d[1], d[2]));
            let vector = self.add(format!("VECTOR('',#{direction},1.)"));
            self.add(format!("LINE('',#{origin},#{vector})"))
        }

        /// A degree-1 B-spline curve through the points `ids`, with knots 0 to their count.
        fn polyline(&mut self, ids: &[usize]) -> usize {
            let n = ids.len() - 1;
            let mut mult = vec!["2".to_string()];
            mult.extend(vec!["1".to_string(); n - 1]);
            mult.push("2".into());
            let knots: Vec<String> = (0..=n).map(|i| format!("{:?}", i as f64)).collect();
            let points: Vec<String> = ids.iter().map(|p| format!("#{p}")).collect();
            self.add(format!(
                "B_SPLINE_CURVE_WITH_KNOTS('',1,({}),.POLYLINE_FORM.,.F.,.F.,({}),({}),.UNSPECIFIED.)",
                points.join(","),
                mult.join(","),
                knots.join(",")
            ))
        }

        /// The file's closing entities: `face` in a solid, with its units.
        fn finish(mut self, face: usize) -> String {
            let shell = self.add(format!("CLOSED_SHELL('',(#{face}))"));
            let model = self.add(format!("MANIFOLD_SOLID_BREP('',#{shell})"));
            let units = self.last + 3;
            let context = self.add(format!(
                "( GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{},#{},#{})) REPRESENTATION_CONTEXT('c','3D') )",
                units,
                units + 1,
                units + 2
            ));
            self.add(format!(
                "ADVANCED_BREP_SHAPE_REPRESENTATION('',(#{model}),#{context})"
            ));
            self.add("( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) )".into());
            self.add("( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.) )".into());
            self.add("( NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT() )".into());
            self.text.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
            self.text
        }

        /// The oriented edge a → b along curve `c`.
        fn edge(&mut self, a: usize, b: usize, c: usize, forward: bool) -> usize {
            let e = self.add(format!("EDGE_CURVE('',#{a},#{b},#{c},.T.)"));
            let sense = if forward { "T" } else { "F" };
            self.add(format!("ORIENTED_EDGE('',*,*,#{e},.{sense}.)"))
        }
    }

    /// STEP text of one open face S(u, v) = (u, v, u²) on the unit square: `spans` quadratic
    /// Bézier spans in u (a net of 2·`spans` + 1 rows), one linear span in v, as a writer that
    /// emits dense B-spline surfaces produces (#302). Its area is √5/2 + asinh(2)/4.
    fn dense_ruled_face(spans: usize) -> String {
        let x = |i: usize| i as f64 / (2 * spans) as f64;
        // Bézier control heights of u ↦ u² over each span: a², ab, b².
        let z = |i: usize| {
            if i.is_multiple_of(2) {
                x(i) * x(i)
            } else {
                x(i - 1) * x(i + 1)
            }
        };
        let list = |items: Vec<String>| items.join(",");
        let mut e = Entities::new();
        let (mut grid, mut low, mut high) = (Vec::new(), Vec::new(), Vec::new());
        for i in 0..=2 * spans {
            let a = e.point([x(i), 0.0, z(i)]);
            let b = e.point([x(i), 1.0, z(i)]);
            grid.push(format!("(#{a},#{b})"));
            low.push(format!("#{a}"));
            high.push(format!("#{b}"));
        }
        let v = [
            e.vertex([0.0, 0.0, 0.0]),
            e.vertex([1.0, 0.0, 1.0]),
            e.vertex([1.0, 1.0, 1.0]),
            e.vertex([0.0, 1.0, 0.0]),
        ];
        let mut mult = vec!["3".to_string()];
        mult.extend(vec!["2".to_string(); spans - 1]);
        mult.push("3".into());
        let mult = list(mult);
        let knots = list(
            (0..=spans)
                .map(|i| format!("{:?}", i as f64 / spans as f64))
                .collect(),
        );
        let surface = e.add(format!(
            "B_SPLINE_SURFACE_WITH_KNOTS('',2,1,({}),.UNSPECIFIED.,.F.,.F.,.F.,({mult}),(2,2),({knots}),(0.,1.),.UNSPECIFIED.)",
            list(grid)
        ));
        let curve = |e: &mut Entities, points: Vec<String>| {
            e.add(format!(
                "B_SPLINE_CURVE_WITH_KNOTS('',2,({}),.UNSPECIFIED.,.F.,.F.,({mult}),({knots}),.UNSPECIFIED.)",
                list(points)
            ))
        };
        let (c_low, c_high) = (curve(&mut e, low), curve(&mut e, high));
        let l_right = e.line([1.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
        let l_left = e.line([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let sides = [
            e.edge(v[0], v[1], c_low, true),
            e.edge(v[1], v[2], l_right, true),
            e.edge(v[3], v[2], c_high, false),
            e.edge(v[0], v[3], l_left, false),
        ];
        let sides = list(sides.iter().map(|s| format!("#{s}")).collect());
        let lp = e.add(format!("EDGE_LOOP('',({sides}))"));
        let bound = e.add(format!("FACE_OUTER_BOUND('',#{lp},.T.)"));
        let face = e.add(format!("ADVANCED_FACE('',(#{bound}),#{surface},.T.)"));
        e.finish(face)
    }

    /// The face's area and the integrand evaluations its moments cost.
    fn dense_ruled_area(spans: usize) -> (f64, usize) {
        let part = crate::step::read_step(dense_ruled_face(spans).as_bytes()).unwrap();
        INTEGRAND_EVALUATIONS.with(|n| n.set(0));
        let area = part.face_moments(0).unwrap().area;
        (area, INTEGRAND_EVALUATIONS.with(|n| n.get()))
    }

    fn grooved_solid() -> Part {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/annular_face_groove.step");
        crate::step::read_step_file(&path).unwrap()
    }

    fn evaluations_of<T>(run: impl FnOnce() -> T) -> (T, usize) {
        INTEGRAND_EVALUATIONS.with(|n| n.set(0));
        let value = run();
        (value, INTEGRAND_EVALUATIONS.with(|n| n.get()))
    }

    /// Face moments and the solid's centroid read one integration per face: whichever is
    /// asked second evaluates no integrand.
    #[test]
    fn face_moments_and_volume_centroid_integrate_each_face_once() {
        let part = grooved_solid();
        let (_, cost) = evaluations_of(|| {
            for f in 0..part.faces.len() {
                let _ = part.face_moments(f);
            }
        });
        assert!(cost > 0);
        let (centroid, again) = evaluations_of(|| part.volume_centroid());
        assert!(centroid.is_some());
        assert_eq!(again, 0, "volume_centroid after face_moments");

        let part = grooved_solid();
        let (centroid, _) = evaluations_of(|| part.volume_centroid());
        assert!(centroid.is_some());
        let (moments, again) = evaluations_of(|| {
            (0..part.faces.len())
                .map(|f| part.face_moments(f))
                .collect::<Vec<_>>()
        });
        assert!(moments.iter().all(Option::is_some));
        assert_eq!(again, 0, "face_moments after volume_centroid");
    }

    /// A face whose face-moment components cannot be integrated refuses its face moments
    /// alone, and one whose centroid components cannot refuses the solid's centroid alone
    /// (forced by making one group's integrand non-finite).
    #[test]
    fn a_face_refuses_only_the_consumer_whose_group_is_unresolved() {
        for (poisoned, moments_refused) in [(true, true), (false, false)] {
            let part = grooved_solid();
            POISONED_GROUP.with(|g| g.set(Some(poisoned)));
            let moments = part.face_moments(0);
            let centroid = part.volume_centroid();
            POISONED_GROUP.with(|g| g.set(None));
            assert_eq!(moments.is_none(), moments_refused);
            assert_eq!(centroid.is_none(), !moments_refused);
            // The unpoisoned group equals what an unforced part gives.
            let clean = grooved_solid();
            if moments_refused {
                assert_eq!(centroid, clean.volume_centroid());
            } else {
                assert_eq!(moments, clean.face_moments(0));
            }
        }
    }

    /// A face with a large B-spline net costs surface evaluations in proportion to its spans,
    /// not their square (#302: integrating along u crossed every u span at every boundary
    /// sample).
    #[test]
    fn a_dense_bspline_face_costs_evaluations_linear_in_its_spans() {
        let exact = 5f64.sqrt() / 2.0 + 2f64.asinh() / 4.0;
        let ((a128, n128), (a256, n256)) = (dense_ruled_area(128), dense_ruled_area(256));
        assert!((a128 - exact).abs() < 1e-12, "{a128} vs {exact}");
        assert!((a256 - exact).abs() < 1e-12, "{a256} vs {exact}");
        // Linear: about 1 000 evaluations a span. Measured at 128 spans: 131 040 here; 43 008
        // with the along-v form switched off (the stationary-node skip alone); 5 591 040 for the
        // stock integral, along u at every boundary sample (43 680 a span, and quadruple the
        // total at 256 spans; xu9299's fixture for the same case). The cap sits 2.3 times above
        // this form and 18 times below stock.
        assert!(n128 < 300_000, "{n128} evaluations");
        assert!(
            n256 * 10 < n128 * 25,
            "{n128} -> {n256} evaluations is not linear"
        );
    }

    /// STEP text of the ruled strip S(u, v) = (u(1 − 2v), v, 0.01u) on the unit square as a
    /// bilinear B-spline face with `spans` equal spans in u (#302, the counterexample xu9299
    /// found: its normal nearly reverses at v = ½, so its area density has a narrow dip).
    fn reversing_strip_face(spans: usize) -> String {
        let at = |i: usize, v: f64| {
            let u = i as f64 / spans as f64;
            [u * (1.0 - 2.0 * v), v, 0.01 * u]
        };
        let mut e = Entities::new();
        let (mut grid, mut low, mut high) = (Vec::new(), Vec::new(), Vec::new());
        for i in 0..=spans {
            let a = e.point(at(i, 0.0));
            let b = e.point(at(i, 1.0));
            grid.push(format!("(#{a},#{b})"));
            low.push(a);
            high.push(b);
        }
        let v = [
            e.vertex(at(0, 0.0)),
            e.vertex(at(spans, 0.0)),
            e.vertex(at(spans, 1.0)),
            e.vertex(at(0, 1.0)),
        ];
        let mut mult = vec!["2".to_string()];
        mult.extend(vec!["1".to_string(); spans - 1]);
        mult.push("2".into());
        let mult = mult.join(",");
        let knots: Vec<String> = (0..=spans)
            .map(|i| format!("{:?}", i as f64 / spans as f64))
            .collect();
        let surface = e.add(format!(
            "B_SPLINE_SURFACE_WITH_KNOTS('',1,1,({}),.UNSPECIFIED.,.F.,.F.,.F.,({mult}),(2,2),({}),(0.,1.),.UNSPECIFIED.)",
            grid.join(","),
            knots.join(",")
        ));
        let (c_low, c_high) = (e.polyline(&low), e.polyline(&high));
        let (p_right, p_left) = (
            [e.point(at(spans, 0.0)), e.point(at(spans, 1.0))],
            [e.point(at(0, 0.0)), e.point(at(0, 1.0))],
        );
        let (c_right, c_left) = (e.polyline(&p_right), e.polyline(&p_left));
        let sides = [
            e.edge(v[0], v[1], c_low, true),
            e.edge(v[1], v[2], c_right, true),
            e.edge(v[3], v[2], c_high, false),
            e.edge(v[0], v[3], c_left, false),
        ];
        let sides = sides.map(|s| format!("#{s}")).join(",");
        let lp = e.add(format!("EDGE_LOOP('',({sides}))"));
        let bound = e.add(format!("FACE_OUTER_BOUND('',#{lp},.T.)"));
        let face = e.add(format!("ADVANCED_FACE('',(#{bound}),#{surface},.T.)"));
        e.finish(face)
    }

    /// STEP text of S(u, v) = (u, v, `height`·v^`degree`) on the unit square as a B-spline face:
    /// degree 1 over 40 spans in u, one Bézier span of `degree` in v. Its area density
    /// √(1 + (height·degree·v^(degree−1))²) is not a polynomial in v, and the nearer its complex
    /// singularities lie to [0, 1] (the larger `height`), the more panels the cheaper form's
    /// inner rule needs in v.
    fn steep_in_v_face(degree: usize, height: f64) -> String {
        let spans = 40;
        let at = |i: usize, j: usize| {
            let z = if j == degree { height } else { 0.0 };
            [i as f64 / spans as f64, j as f64 / degree as f64, z]
        };
        let mut e = Entities::new();
        let mut grid = Vec::new();
        for i in 0..=spans {
            let row: Vec<String> = (0..=degree)
                .map(|j| format!("#{}", e.point(at(i, j))))
                .collect();
            grid.push(format!("({})", row.join(",")));
        }
        let mut mult = vec!["2".to_string()];
        mult.extend(vec!["1".to_string(); spans - 1]);
        mult.push("2".into());
        let knots: Vec<String> = (0..=spans)
            .map(|i| format!("{:?}", i as f64 / spans as f64))
            .collect();
        let surface = e.add(format!(
            "B_SPLINE_SURFACE_WITH_KNOTS('',1,{degree},({}),.UNSPECIFIED.,.F.,.F.,.F.,({}),({degree1},{degree1}),({}),(0.,1.),.UNSPECIFIED.)",
            grid.join(","),
            mult.join(","),
            knots.join(","),
            degree1 = degree + 1
        ));
        // The sides along v are the surface's own curves there, with its control points.
        let side = |e: &mut Entities, i: usize| {
            let points: Vec<String> = (0..=degree)
                .map(|j| format!("#{}", e.point(at(i, j))))
                .collect();
            e.add(format!(
                "B_SPLINE_CURVE_WITH_KNOTS('',{degree},({}),.UNSPECIFIED.,.F.,.F.,({degree1},{degree1}),(0.,1.),.UNSPECIFIED.)",
                points.join(","),
                degree1 = degree + 1
            ))
        };
        let (c_left, c_right) = (side(&mut e, 0), side(&mut e, spans));
        let (low, high): (Vec<usize>, Vec<usize>) = (0..=spans)
            .map(|i| (e.point(at(i, 0)), e.point(at(i, degree))))
            .unzip();
        let (c_low, c_high) = (e.polyline(&low), e.polyline(&high));
        let v = [
            e.vertex(at(0, 0)),
            e.vertex(at(spans, 0)),
            e.vertex(at(spans, degree)),
            e.vertex(at(0, degree)),
        ];
        let sides = [
            e.edge(v[0], v[1], c_low, true),
            e.edge(v[1], v[2], c_right, true),
            e.edge(v[3], v[2], c_high, false),
            e.edge(v[0], v[3], c_left, false),
        ];
        let sides = sides.map(|s| format!("#{s}")).join(",");
        let lp = e.add(format!("EDGE_LOOP('',({sides}))"));
        let bound = e.add(format!("FACE_OUTER_BOUND('',#{lp},.T.)"));
        let face = e.add(format!("ADVANCED_FACE('',(#{bound}),#{surface},.T.)"));
        e.finish(face)
    }

    /// The face's area by [`Part::integrate_face_legacy`] (the cheaper form's trials, then the
    /// stock form), with the integrand evaluations it costs.
    fn steep_area(part: &Part) -> (f64, usize) {
        let area = |_: V3, su: V3, sv: V3| [geom::norm(geom::cross(su, sv))];
        let rule = IntegralRule {
            components: Components::Prefix(1),
            observe_mass: None,
        };
        let (value, n) =
            evaluations_of(|| part.integrate_face_legacy(0, area, rule, &mut None, true));
        (value.unwrap()[0], n)
    }

    /// The face's area from the cheaper form cut `split` times finer, whether its check passed,
    /// and its cost; or by the stock form (`split` 0, which always passes).
    fn steep_trial(part: &Part, split: usize) -> (f64, bool, usize) {
        let area = |_: V3, su: V3, sv: V3| [geom::norm(geom::cross(su, sv))];
        let rule = IntegralRule {
            components: Components::Prefix(1),
            observe_mass: None,
        };
        let unresolved = std::cell::Cell::new(false);
        let (value, n) = evaluations_of(|| {
            let trial = (split > 0).then_some((&unresolved, split));
            part.integrate_face_legacy_in(0, area, rule, &mut None, trial)
        });
        (value.unwrap()[0], !unresolved.get(), n)
    }

    fn steep_exact(degree: usize, height: f64) -> f64 {
        let (d, h) = (degree as f64, height);
        let cuts = cuts(0.0, 1.0, 2048, &[]).unwrap();
        integrate(&cuts, |v| {
            [(1.0 + (h * d * v.powi(degree as i32 - 1)).powi(2)).sqrt()]
        })[0]
    }

    /// A face the cheaper form's one rule per v knot span cannot resolve (its area density is
    /// no polynomial in v) takes the form once more with each span in four, and keeps that
    /// result when it passes the same check: the exact area, by the finer trial alone.
    #[test]
    fn a_face_unresolved_per_knot_span_is_integrated_with_finer_panels_along_v() {
        let (degree, height) = (6, 20.0);
        let part = crate::step::read_step(steep_in_v_face(degree, height).as_bytes()).unwrap();
        assert!(part.cheaper_inner_form(0));
        let (_, coarse_ok, coarse_cost) = steep_trial(&part, 1);
        let (fine, fine_ok, fine_cost) = steep_trial(&part, REFINED_PANELS);
        assert!(!coarse_ok && fine_ok);
        let exact = steep_exact(degree, height);
        assert!((fine - exact).abs() < 1e-11 * exact, "{fine} vs {exact}");

        let (area, cost) = steep_area(&part);
        assert_eq!(area, fine, "the finer trial's result is the face's");
        assert_eq!(cost, coarse_cost + fine_cost, "both trials ran, no stock");
        assert!(!part.cache[0].trial_refused((1, false)));
        // Once the finer trial has passed, a repeat runs the same two trials (nothing refused).
        assert_eq!(steep_area(&part).1, cost);
    }

    /// A refusal is not remembered for `face_projected_moments`, whose integrand depends on the
    /// direction: another direction still runs its own trials, bit for bit as with no refusal on record.
    #[test]
    fn a_refused_projected_direction_does_not_skip_the_trial_for_another() {
        let part = || crate::step::read_step(steep_in_v_face(4, 160.0).as_bytes()).unwrap();
        let (first, second) = ([0.0, 0.0, 1.0], [0.0, 0.6, 0.8]);
        let warm = part();
        assert!(warm.face_projected_moments(0, first).is_some());
        assert!(!warm.cache[0].trial_refused((9, true)));
        let (warm_result, warm_cost) = evaluations_of(|| warm.face_projected_moments(0, second));
        // The same call on a part that only integrated the face's moments, so that mass is
        // cached as in `warm` and no projected refusal exists: its trials run.
        let cold = part();
        assert!(cold.face_moments(0).is_some());
        let (cold_result, cold_cost) = evaluations_of(|| cold.face_projected_moments(0, second));
        assert!(cold_result.is_some());
        assert_eq!(
            warm_result.map(|(a, c)| (a.to_bits(), c.map(f64::to_bits))),
            cold_result.map(|(a, c)| (a.to_bits(), c.map(f64::to_bits)))
        );
        assert_eq!(warm_cost, cold_cost);
    }

    /// A face both trials refuse takes the stock form, and the refusal is remembered: a second
    /// integration of it runs no trial.
    #[test]
    fn a_face_refused_at_both_panel_counts_goes_straight_to_stock_afterwards() {
        let (degree, height) = (4, 160.0);
        let part = crate::step::read_step(steep_in_v_face(degree, height).as_bytes()).unwrap();
        assert!(part.cheaper_inner_form(0));
        let (_, coarse_ok, coarse_cost) = steep_trial(&part, 1);
        let (_, fine_ok, fine_cost) = steep_trial(&part, REFINED_PANELS);
        let (stock, _, stock_cost) = steep_trial(&part, 0);
        assert!(!coarse_ok && !fine_ok);
        assert!(!part.cache[0].trial_refused((1, false)));

        let (area, first) = steep_area(&part);
        assert_eq!(area, stock);
        assert_eq!(first, coarse_cost + fine_cost + stock_cost);
        assert!(part.cache[0].trial_refused((1, false)));
        let (area, second) = steep_area(&part);
        assert_eq!(area, stock);
        assert_eq!(second, stock_cost, "no trial on the second integration");
        // Another consumer's integrand (another width) has not been refused yet.
        assert!(!part.cache[0].trial_refused((5, false)));
        let exact = steep_exact(degree, height);
        assert!((stock - exact).abs() < 1e-9 * exact, "{stock} vs {exact}");
    }

    /// The strip's area is right where the cheaper form's inner rule would be 0.263% high on
    /// its shape check alone (#302): ∫₀¹ ½(√(1+k) + k·asinh(1/√k)) du with k = 1e-4 + 4e-4u²,
    /// the v integral done in closed form and the u integral by Simpson at 2 000, 4 000 and
    /// 8 000 intervals (all agreeing to 1e-16).
    #[test]
    fn a_strip_whose_normal_nearly_reverses_has_its_exact_area() {
        let exact = 0.500_619_693_849_426_7;
        let part = crate::step::read_step(reversing_strip_face(40).as_bytes()).unwrap();
        INTEGRAND_EVALUATIONS.with(|n| n.set(0));
        let area = part.face_moments(0).unwrap().area;
        assert!((area - exact).abs() < 1e-9 * exact, "{area} vs {exact}");
    }

    /// STEP text of the plane S(u, v) = (u, v, 0) as a degree-(1, 1) B-spline surface of 40 × 8
    /// uniform spans on [0, 1]², trimmed to the triangle (0, 0), (1, 0), (1, 1).
    fn trimmed_triangle_on_a_net() -> String {
        let (nu, nv) = (40usize, 8usize);
        let at = |i: usize, j: usize| [i as f64 / nu as f64, j as f64 / nv as f64, 0.0];
        let mut e = Entities::new();
        let mut grid = Vec::new();
        for i in 0..=nu {
            let row: Vec<String> = (0..=nv)
                .map(|j| format!("#{}", e.point(at(i, j))))
                .collect();
            grid.push(format!("({})", row.join(",")));
        }
        let v = [
            e.vertex([0.0, 0.0, 0.0]),
            e.vertex([1.0, 0.0, 0.0]),
            e.vertex([1.0, 1.0, 0.0]),
        ];
        let mults = |n: usize| {
            let mut m = vec!["2".to_string()];
            m.extend(vec!["1".to_string(); n - 1]);
            m.push("2".into());
            m.join(",")
        };
        let knots = |n: usize| {
            let k: Vec<String> = (0..=n)
                .map(|i| format!("{:?}", i as f64 / n as f64))
                .collect();
            k.join(",")
        };
        let surface = e.add(format!(
            "B_SPLINE_SURFACE_WITH_KNOTS('',1,1,({}),.UNSPECIFIED.,.F.,.F.,.F.,({}),({}),({}),({}),.UNSPECIFIED.)",
            grid.join(","),
            mults(nu),
            mults(nv),
            knots(nu),
            knots(nv)
        ));
        let r = std::f64::consts::FRAC_1_SQRT_2;
        let bottom = e.line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        let right = e.line([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let diagonal = e.line([1.0, 1.0, 0.0], [-r, -r, 0.0]);
        let sides = [
            e.edge(v[0], v[1], bottom, true),
            e.edge(v[1], v[2], right, true),
            e.edge(v[2], v[0], diagonal, true),
        ];
        let sides = sides.map(|s| format!("#{s}")).join(",");
        let lp = e.add(format!("EDGE_LOOP('',({sides}))"));
        let bound = e.add(format!("FACE_OUTER_BOUND('',#{lp},.T.)"));
        let face = e.add(format!("ADVANCED_FACE('',(#{bound}),#{surface},.T.)"));
        e.finish(face)
    }

    /// A huge knot outside the active domain does not change the face (#308, found by xu9299):
    /// the stationary-node skip judges motions by the active parameter ranges, so the diagonal
    /// edge's nodes are integrated and the triangle keeps its area ½ and centroid (⅔, ⅓, 0).
    #[test]
    fn phantom_outer_knots_do_not_hide_the_diagonal_edge() {
        let mut part = crate::step::read_step(trimmed_triangle_on_a_net().as_bytes()).unwrap();
        let Surface::Freeform { surface, .. } = &mut part.faces[0].surface else {
            panic!("not a B-spline face");
        };
        let last = surface.knots_v.len() - 1;
        surface.knots_v[0] = -1e13;
        surface.knots_v[last] = 1e13;
        let m = part.face_moments(0).unwrap();
        assert!((m.area - 0.5).abs() < 1e-9, "area {}", m.area);
        let c = m.centroid;
        for (got, want) in c.iter().zip([2.0 / 3.0, 1.0 / 3.0, 0.0]) {
            assert!((got - want).abs() < 1e-9, "centroid {c:?}");
        }
    }

    /// The cheaper form's inner rule is used only where it agrees with itself on halves: a
    /// density with a narrow dip (a strip whose normal nearly reverses) sends the face back to
    /// the stock form (#302).
    #[test]
    fn the_checked_inner_rule_rejects_a_narrow_dip() {
        let smooth = |s: f64| [(1.0 + s * s).sqrt(), s];
        let (sum, resolved) = integrate_checked(&[0.0, 1.0], smooth);
        assert!(resolved);
        assert_eq!(sum, integrate(&[0.0, 1.0], smooth));
        let dip = |s: f64| [(1e-6 + (s - 0.3) * (s - 0.3)).sqrt()];
        assert!(!integrate_checked(&[0.0, 1.0], dip).1);
    }

    /// The polynomial piece a parameter lies in, by search, is the pair `breaks` gave.
    #[test]
    fn the_piece_of_a_parameter_matches_the_distinct_knots_around_it() {
        for degree in [1, 2, 3, 7] {
            for inner in [
                vec![0.25],
                vec![0.25, 0.5, 0.5, 0.75],
                (1..40).map(|i| i as f64 / 40.0).collect(),
            ] {
                let mut knots = vec![0.0; degree + 1];
                knots.extend(&inner);
                knots.extend(vec![1.0; degree + 1]);
                let b = breaks(&knots, degree);
                let window = &knots[degree..knots.len() - degree];
                let mut xs = vec![-1.0, 0.0, 1.0, 2.0, f64::NAN];
                xs.extend(b.iter().flat_map(|&k| [k, k - 1e-9, k + 1e-9]));
                xs.extend((0..=100).map(|i| i as f64 / 100.0));
                for x in xs {
                    let i = b.partition_point(|&k| k <= x).clamp(1, b.len() - 1);
                    assert_eq!(piece_of(window, x), (b[i - 1], b[i]), "{degree} {x}");
                }
            }
        }
    }

    /// STEP text of a strip of cylinder (radius 5) between two helices an axial 1 apart, `turns`
    /// turns long: the face of a thread flank on a core, as a writer emits it (#302). Its area
    /// is 5 · 1 · (2π · turns).
    fn helical_strip(turns: usize) -> String {
        let (r, pitch, width) = (5.0f64, 0.2f64, 1.0f64);
        let segments = 128 * turns;
        let end = std::f64::consts::TAU * turns as f64;
        let mut e = Entities::new();
        let at = |u: f64, lift: f64| [r * u.cos(), r * u.sin(), pitch * u + lift];
        let (mut low, mut high) = (Vec::new(), Vec::new());
        for i in 0..=segments {
            let u = end * i as f64 / segments as f64;
            low.push(e.point(at(u, 0.0)));
            high.push(e.point(at(u, width)));
        }
        let v = [
            e.vertex(at(0.0, 0.0)),
            e.vertex(at(end, 0.0)),
            e.vertex(at(end, width)),
            e.vertex(at(0.0, width)),
        ];
        let (c_low, c_high) = (e.polyline(&low), e.polyline(&high));
        let l_end = e.line(at(end, 0.0), [0.0, 0.0, 1.0]);
        let l_start = e.line(at(0.0, 0.0), [0.0, 0.0, 1.0]);
        let sides = [
            e.edge(v[0], v[1], c_low, true),
            e.edge(v[1], v[2], l_end, true),
            e.edge(v[3], v[2], c_high, false),
            e.edge(v[0], v[3], l_start, false),
        ];
        let sides = sides.map(|s| format!("#{s}")).join(",");
        let origin = e.point([0.0; 3]);
        let z = e.add("DIRECTION('',(0.,0.,1.))".into());
        let x = e.add("DIRECTION('',(1.,0.,0.))".into());
        let frame = e.add(format!("AXIS2_PLACEMENT_3D('',#{origin},#{z},#{x})"));
        let surface = e.add(format!("CYLINDRICAL_SURFACE('',#{frame},{r:?})"));
        let lp = e.add(format!("EDGE_LOOP('',({sides}))"));
        let bound = e.add(format!("FACE_OUTER_BOUND('',#{lp},.T.)"));
        let face = e.add(format!("ADVANCED_FACE('',(#{bound}),#{surface},.T.)"));
        e.finish(face)
    }

    /// A cylinder face whose boundary winds through many turns costs integrand evaluations in
    /// proportion to its length, not its square (#302: the inner integral along u crossed a
    /// panel per radian at every boundary node).
    #[test]
    fn a_long_helical_strip_costs_evaluations_linear_in_its_turns() {
        let cost = |turns: usize| {
            let part = crate::step::read_step(helical_strip(turns).as_bytes()).unwrap();
            INTEGRAND_EVALUATIONS.with(|n| n.set(0));
            let area = part.face_moments(0).unwrap().area;
            let exact = 5.0 * std::f64::consts::TAU * turns as f64;
            assert!(
                (area - exact).abs() < 1e-3 * exact,
                "{turns} turns: {area} vs {exact}"
            );
            INTEGRAND_EVALUATIONS.with(|n| n.get())
        };
        let (n10, n20) = (cost(10), cost(20));
        assert!(
            n20 * 10 < n10 * 25,
            "{n10} -> {n20} evaluations is not linear"
        );
    }

    /// Per facet, how far the slanted strip of [`prism_face`] rises.
    const PRISM_SLOPE: f64 = 0.03;

    /// STEP text of part of the lateral surface of a regular `n`-gonal prism (circumradius 5,
    /// height 2) as a B-spline face closed in u, as a writer emits a cylinder it approximates:
    /// degree 1 in u over `n` spans, one span in v, its sides of constant u coinciding. The face
    /// is a strip a unit high rising [`PRISM_SLOPE`] a facet, from facet `from` to facet `to`
    /// (past `n`, across the surface's seam): a loop that winds round nowhere, whose edges
    /// along u move in v. Or, `whole`, it is all of the surface, between two loops that wind
    /// round u.
    fn prism_face(n: usize, from: usize, to: usize, whole: bool) -> String {
        let (r, h) = (5.0f64, 2.0f64);
        let at = |i: usize, z: f64| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            [r * a.cos(), r * a.sin(), z]
        };
        let (from, to) = if whole { (0, n) } else { (from, to) };
        let rise = |i: usize| PRISM_SLOPE * (i - from) as f64;
        let list = |items: Vec<String>| items.join(",");
        let mut e = Entities::new();
        let grid: Vec<String> = (0..=n)
            .map(|i| {
                let (a, b) = (e.point(at(i, 0.0)), e.point(at(i, h)));
                format!("(#{a},#{b})")
            })
            .collect();
        let mut mult = vec!["2".to_string()];
        mult.extend(vec!["1".to_string(); n - 1]);
        mult.push("2".into());
        let knots = list((0..=n).map(|i| format!("{:?}", i as f64)).collect());
        let surface = e.add(format!(
            "B_SPLINE_SURFACE_WITH_KNOTS('',1,1,({}),.UNSPECIFIED.,.F.,.F.,.F.,({}),(2,2),({knots}),(0.,1.),.UNSPECIFIED.)",
            list(grid),
            list(mult)
        ));
        let row = |e: &mut Entities, from: usize, to: usize, z: &dyn Fn(usize) -> f64| {
            let points: Vec<usize> = (from..=to).map(|i| e.point(at(i, z(i)))).collect();
            e.polyline(&points)
        };
        let bounds = if whole {
            // Each ring in two edges, as a writer splits a closed curve.
            let ring = |e: &mut Entities, z: f64, kind: &str| {
                let (a, b) = (e.vertex(at(0, z)), e.vertex(at(n / 2, z)));
                let (first, second) = (row(e, 0, n / 2, &|_| z), row(e, n / 2, n, &|_| z));
                let edges = [e.edge(a, b, first, true), e.edge(b, a, second, true)];
                let lp = e.add(format!("EDGE_LOOP('',(#{},#{}))", edges[0], edges[1]));
                e.add(format!("{kind}('',#{lp},.T.)"))
            };
            vec![
                ring(&mut e, 0.0, "FACE_OUTER_BOUND"),
                ring(&mut e, h, "FACE_BOUND"),
            ]
        } else {
            let band = 1.0;
            let (c_low, c_high) = (
                row(&mut e, from, to, &rise),
                row(&mut e, from, to, &|i| rise(i) + band),
            );
            let v = [
                e.vertex(at(from, rise(from))),
                e.vertex(at(to, rise(to))),
                e.vertex(at(to, rise(to) + band)),
                e.vertex(at(from, rise(from) + band)),
            ];
            let l_end = e.line(at(to, rise(to)), [0.0, 0.0, 1.0]);
            let l_start = e.line(at(from, rise(from)), [0.0, 0.0, 1.0]);
            let sides = [
                e.edge(v[0], v[1], c_low, true),
                e.edge(v[1], v[2], l_end, true),
                e.edge(v[3], v[2], c_high, false),
                e.edge(v[0], v[3], l_start, false),
            ];
            let sides = sides.map(|s| format!("#{s}")).join(",");
            let lp = e.add(format!("EDGE_LOOP('',({sides}))"));
            vec![e.add(format!("FACE_OUTER_BOUND('',#{lp},.T.)"))]
        };
        let bounds: Vec<String> = bounds.iter().map(|b| format!("#{b}")).collect();
        let face = e.add(format!(
            "ADVANCED_FACE('',({}),#{surface},.T.)",
            bounds.join(",")
        ));
        e.finish(face)
    }

    /// The exact area and centroid of the strip of [`prism_face`] over `facets`: each facet is
    /// a flat parallelogram of side 2·5·sin(π/n) and height 1, its centroid at the apothem
    /// along its mid-angle, half a unit above its mid-edge. (`whole`: height 2, level.)
    fn prism_exact(n: usize, facets: std::ops::Range<usize>, whole: bool) -> (f64, V3) {
        let r = 5.0f64;
        let (side, apothem) = (
            2.0 * r * (std::f64::consts::PI / n as f64).sin(),
            r * (std::f64::consts::PI / n as f64).cos(),
        );
        let (height, slope) = if whole {
            (2.0, 0.0)
        } else {
            (1.0, PRISM_SLOPE)
        };
        let mut sum = [0.0, 0.0, 0.0];
        for i in facets.clone() {
            let a = std::f64::consts::TAU * (i as f64 + 0.5) / n as f64;
            let z = slope * (i as f64 + 0.5 - facets.start as f64) + 0.5 * height;
            sum = geom::add(sum, [apothem * a.cos(), apothem * a.sin(), z]);
        }
        let facets = facets.len() as f64;
        (side * height * facets, geom::scale(sum, 1.0 / facets))
    }

    /// The face's area by the stock form (the integral along u) and by the form the face takes,
    /// with the integrand evaluations each costs.
    fn prism_areas(text: &str) -> ((f64, usize), (f64, usize)) {
        let area = |_: V3, su: V3, sv: V3| [geom::norm(geom::cross(su, sv))];
        let rule = IntegralRule {
            components: Components::Prefix(1),
            observe_mass: None,
        };
        let part = crate::step::read_step(text.as_bytes()).unwrap();
        INTEGRAND_EVALUATIONS.with(|n| n.set(0));
        let stock = part
            .integrate_face_legacy_in(0, area, rule, &mut None, None)
            .unwrap()[0];

        let stock = (stock, INTEGRAND_EVALUATIONS.with(|n| n.get()));
        INTEGRAND_EVALUATIONS.with(|n| n.set(0));
        let chosen = part.face_moments(0).unwrap().area;
        (stock, (chosen, INTEGRAND_EVALUATIONS.with(|n| n.get())))
    }

    /// A B-spline face closed in u whose loop does not wind round u, with a large net, takes the
    /// cheaper form (along v), whether or not it crosses the surface's seam, and keeps its area
    /// and centroid (a casing's closed faces spent most of the file's time along u).
    #[test]
    fn a_closed_in_u_face_not_winding_round_u_takes_the_cheaper_form() {
        let n = 96;
        for (from, to) in [(8, 40), (80, 112)] {
            let text = prism_face(n, from, to, false);
            let part = crate::step::read_step(text.as_bytes()).unwrap();
            assert!(
                closed_spans(&part.faces[0].surface).0.is_finite(),
                "the surface must be closed in u"
            );
            assert!(part.cheaper_inner_form(0));
            let ((stock, n_stock), (area, n_cheap)) = prism_areas(&text);
            let (exact, centroid) = prism_exact(n, from..to, false);
            let moments = part.face_moments(0).unwrap();
            assert!(
                (area - exact).abs() < 1e-12 * exact,
                "{from}..{to}: {area} vs {exact}"
            );
            assert!((stock - exact).abs() < 1e-12 * exact, "{stock} vs {exact}");
            for k in 0..3 {
                assert!(
                    (moments.centroid[k] - centroid[k]).abs() < 1e-12,
                    "{from}..{to}: {:?} vs {centroid:?}",
                    moments.centroid
                );
            }
            assert!(
                n_cheap * 4 < n_stock,
                "{from}..{to}: {n_cheap} evaluations against {n_stock} for the stock form"
            );
        }
    }

    /// A closed-in-u face whose loops wind round u is not read by the cheaper form (its
    /// parameters wrap at the seam, so such a loop looks closed): the form refuses it, and the
    /// face takes the path it always took (which, for this synthetic face, integrates nothing).
    #[test]
    fn a_closed_in_u_face_winding_round_u_keeps_the_stock_form() {
        let n = 96;
        let part = crate::step::read_step(prism_face(n, 0, n, true).as_bytes()).unwrap();
        assert!(part.cheaper_inner_form(0), "the surface alone is eligible");
        let area = |_: V3, su: V3, sv: V3| [geom::norm(geom::cross(su, sv))];
        let rule = IntegralRule {
            components: Components::Prefix(1),
            observe_mass: None,
        };
        let unresolved = std::cell::Cell::new(false);
        assert!(
            part.integrate_face_legacy_in(0, area, rule, &mut None, Some((&unresolved, 1)))
                .is_none(),
            "the cheaper form must refuse a winding loop"
        );
        // As before the cheaper form existed, the stock form does not integrate this face
        // (its loops close at the seam, and no area results).
        let stock = part.integrate_face_legacy_in(0, area, rule, &mut None, None);
        assert!(stock.is_none_or(|y| y[0] == 0.0), "{stock:?}");
        assert!(part.face_moments(0).is_none());
    }
}
