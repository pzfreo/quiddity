//! Rational B-spline curves and surfaces: evaluation, point inversion and ray intersection.
//!
//! Every surface the analytic set does not cover (B-splines, extrusions, revolutions) reaches the
//! recognisers in this form, via `step-io`'s exact NURBS conversion.

use super::geom::{self, COORD_FLOOR, V3};

fn find_span(knots: &[f64], degree: usize, n: usize, t: f64) -> usize {
    // n = number of control points; valid spans are degree..n-1: the last whose knot is <= t.
    degree + knots[degree + 1..n.max(degree + 1)].partition_point(|&k| k <= t)
}

/// OpenCascade's largest B-spline degree; basis values live on the stack up to it.
pub const MAX_DEGREE: usize = 25;

/// Up to `MAX_DEGREE + 1` basis values, read as a slice.
#[derive(Clone, Copy)]
struct Basis {
    values: [f64; MAX_DEGREE + 1],
    len: usize,
}

impl std::ops::Deref for Basis {
    type Target = [f64];
    fn deref(&self) -> &[f64] {
        &self.values[..self.len]
    }
}

/// The `degree + 1` non-zero basis functions at *t* in *span* (Piegl & Tiller A2.2).
fn basis(knots: &[f64], degree: usize, span: usize, t: f64) -> Basis {
    if degree < SMALL_DEGREE {
        return basis_in::<SMALL_DEGREE>(knots, degree, span, t);
    }
    basis_in::<{ MAX_DEGREE + 1 }>(knots, degree, span, t)
}

/// Degrees below this (nearly all) work in arrays this long rather than `MAX_DEGREE + 1`, so
/// a basis does not clear hundreds of bytes it never reads. The same numbers either way.
const SMALL_DEGREE: usize = 8;

/// [`basis`], working in arrays of `N > degree`.
fn basis_in<const N: usize>(knots: &[f64], degree: usize, span: usize, t: f64) -> Basis {
    let mut n = [0.0; N];
    let mut left = [0.0; N];
    let mut right = [0.0; N];
    n[0] = 1.0;
    for j in 1..=degree {
        left[j] = t - knots[span + 1 - j];
        right[j] = knots[span + j] - t;
        let mut saved = 0.0;
        for r in 0..j {
            let denom = right[r + 1] + left[j - r];
            let temp = if denom.abs() < 1e-300 {
                0.0
            } else {
                n[r] / denom
            };
            n[r] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        n[j] = saved;
    }
    let mut values = [0.0; MAX_DEGREE + 1];
    values[..N].copy_from_slice(&n);
    Basis {
        values,
        len: degree + 1,
    }
}

/// [`basis`] of *degree*, and of the degree below it: A2.2 builds each degree from the one
/// below, so the lower one is its last step but one, the same numbers `basis` gives it, for
/// one pass instead of two.
fn basis_and_lower(knots: &[f64], degree: usize, span: usize, t: f64) -> (Basis, Basis) {
    if degree < SMALL_DEGREE {
        return basis_and_lower_in::<SMALL_DEGREE>(knots, degree, span, t);
    }
    basis_and_lower_in::<{ MAX_DEGREE + 1 }>(knots, degree, span, t)
}

/// [`basis_and_lower`], working in arrays of `N > degree`.
fn basis_and_lower_in<const N: usize>(
    knots: &[f64],
    degree: usize,
    span: usize,
    t: f64,
) -> (Basis, Basis) {
    let mut n = [0.0; N];
    let mut left = [0.0; N];
    let mut right = [0.0; N];
    n[0] = 1.0;
    let mut below = n;
    for j in 1..=degree {
        below = n;
        left[j] = t - knots[span + 1 - j];
        right[j] = knots[span + j] - t;
        let mut saved = 0.0;
        for r in 0..j {
            let denom = right[r + 1] + left[j - r];
            let temp = if denom.abs() < 1e-300 {
                0.0
            } else {
                n[r] / denom
            };
            n[r] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        n[j] = saved;
    }
    let read = |n: [f64; N], len: usize| {
        let mut values = [0.0; MAX_DEGREE + 1];
        values[..N].copy_from_slice(&n);
        Basis { values, len }
    };
    (read(n, degree + 1), read(below, degree))
}

/// The basis functions and their first derivatives at *t* in *span*.
fn basis_and_derivative(knots: &[f64], degree: usize, span: usize, t: f64) -> (Basis, Basis) {
    let (n, lower) = basis_and_lower(knots, degree, span, t);
    let mut d = Basis {
        values: [0.0; MAX_DEGREE + 1],
        len: degree + 1,
    };
    if degree == 0 {
        return (n, d);
    }
    // dN(i,p) = p (N(i,p-1) / (k[i+p]-k[i]) - N(i+1,p-1) / (k[i+p+1]-k[i+1])), N(·,p-1) from
    // the same pass.
    let p = degree as f64;
    let ratio = |num: f64, den: f64| if den.abs() < 1e-300 { 0.0 } else { num / den };
    for j in 0..=degree {
        let i = span - degree + j;
        let left = if j >= 1 { lower[j - 1] } else { 0.0 };
        let right = if j < degree { lower[j] } else { 0.0 };
        d.values[j] = p
            * (ratio(left, knots[i + degree] - knots[i])
                - ratio(right, knots[i + degree + 1] - knots[i + 1]));
    }
    (n, d)
}

/// A rational B-spline curve in world coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct NurbsCurve {
    degree: usize,
    control_points: Vec<V3>,
    weights: Vec<f64>,
    knots: Vec<f64>,
    closed: bool,
    /// Samples over the domain seeding inversion, built on first use.
    table: std::sync::OnceLock<Vec<(f64, V3)>>,
}

/// Whether the arrays describe a well-formed B-spline: `knots = n + degree + 1`, positive
/// weights, non-decreasing knots and a non-empty domain.
fn well_formed(degree: usize, n: usize, weights: usize, knots: &[f64]) -> bool {
    degree <= MAX_DEGREE
        && n > degree
        && weights == n
        && knots.len() == n + degree + 1
        && knots.windows(2).all(|w| w[0] <= w[1])
        && knots[degree] < knots[knots.len() - degree - 1]
}

impl NurbsCurve {
    pub(crate) fn transform_points(&mut self, point: impl Fn(V3) -> V3) {
        for p in self.control_points.iter_mut() {
            *p = point(*p);
        }
        self.table = std::sync::OnceLock::new();
    }

    /// `None` for malformed arrays, so a bad file degrades to an unresolved curve rather than
    /// panicking in evaluation.
    pub fn new(
        degree: usize,
        control_points: Vec<V3>,
        weights: Vec<f64>,
        knots: Vec<f64>,
    ) -> Option<Self> {
        if !well_formed(degree, control_points.len(), weights.len(), &knots)
            || weights.iter().any(|w| *w <= 0.0)
        {
            return None;
        }
        let mut curve = NurbsCurve {
            degree,
            control_points,
            weights,
            knots,
            closed: false,
            table: std::sync::OnceLock::new(),
        };
        let (lo, hi) = curve.domain();
        let size = curve
            .control_points
            .iter()
            .flatten()
            .fold(1.0f64, |m, c| m.max(c.abs()));
        curve.closed = geom::dist(curve.value(lo), curve.value(hi)) <= 1e-9 * size;
        Some(curve)
    }

    pub fn domain(&self) -> (f64, f64) {
        (
            self.knots[self.degree],
            self.knots[self.knots.len() - self.degree - 1],
        )
    }

    pub fn degree(&self) -> usize {
        self.degree
    }

    pub fn control_points(&self) -> &[V3] {
        &self.control_points
    }

    pub fn weights(&self) -> &[f64] {
        &self.weights
    }

    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    pub fn value(&self, t: f64) -> V3 {
        let (lo, hi) = self.domain();
        let t = t.clamp(lo, hi);
        let n = self.control_points.len();
        let span = find_span(&self.knots, self.degree, n, t);
        let b = basis(&self.knots, self.degree, span, t);
        let mut acc = [0.0; 4];
        for (j, bj) in b.iter().enumerate() {
            let i = span - self.degree + j;
            let w = self.weights[i] * bj;
            let c = self.control_points[i];
            acc = [
                acc[0] + c[0] * w,
                acc[1] + c[1] * w,
                acc[2] + c[2] * w,
                acc[3] + w,
            ];
        }
        [acc[0] / acc[3], acc[1] / acc[3], acc[2] / acc[3]]
    }

    /// The exact first derivative (quotient rule on the homogeneous form).
    pub fn derivative(&self, t: f64) -> V3 {
        let (lo, hi) = self.domain();
        let t = t.clamp(lo, hi);
        let span = find_span(&self.knots, self.degree, self.control_points.len(), t);
        let (b, db) = basis_and_derivative(&self.knots, self.degree, span, t);
        let (mut a, mut a_t) = ([0.0; 4], [0.0; 4]);
        for j in 0..b.len() {
            let i = span - self.degree + j;
            let w = self.weights[i];
            let c = self.control_points[i];
            let h = [c[0] * w, c[1] * w, c[2] * w, w];
            for k in 0..4 {
                a[k] += b[j] * h[k];
                a_t[k] += db[j] * h[k];
            }
        }
        [0, 1, 2].map(|k| (a_t[k] - a_t[3] * a[k] / a[3]) / a[3])
    }
}

/// The distinct knots across a B-spline's domain, where its polynomial pieces meet.
pub fn breaks(knots: &[f64], degree: usize) -> Vec<f64> {
    let (lo, hi) = (knots[degree], knots[knots.len() - degree - 1]);
    let mut out: Vec<f64> = knots
        .iter()
        .copied()
        .filter(|k| (lo..=hi).contains(k))
        .collect();
    out.dedup();
    out
}

impl NurbsCurve {
    pub fn breaks(&self) -> Vec<f64> {
        breaks(&self.knots, self.degree)
    }

    /// Whether the curve ends where it starts, so that it can be run round as a periodic curve.
    pub fn is_closed(&self) -> bool {
        let (lo, hi) = self.domain();
        let size = self
            .control_points
            .iter()
            .flatten()
            .fold(1.0f64, |m, c| m.max(c.abs()));
        geom::dist(self.value(lo), self.value(hi)) <= 1e-9 * size
    }

    /// The parameter of the curve point nearest *p*: the closest of a dense sampling, refined
    /// by Newton steps on the squared distance.
    pub fn invert(&self, p: V3) -> f64 {
        let (lo, hi) = self.domain();
        let table = self.table.get_or_init(|| {
            let n = 512;
            (0..=n)
                .map(|i| lo + (hi - lo) * i as f64 / n as f64)
                .map(|t| (t, self.value(t)))
                .collect()
        });
        let mut t = table
            .iter()
            .map(|&(t, q)| (geom::dist(q, p), t))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            // The table always holds 513 samples.
            .map_or(lo, |(_, t)| t);
        let h = (hi - lo) * 1e-7;
        for _ in 0..30 {
            let (ta, tb) = ((t - h).max(lo), (t + h).min(hi));
            let d = geom::scale(geom::sub(self.value(tb), self.value(ta)), 1.0 / (tb - ta));
            let dd = geom::dot(d, d);
            if dd < 1e-300 {
                break;
            }
            let step = geom::dot(geom::sub(p, self.value(t)), d) / dd;
            let next = (t + step).clamp(lo, hi);
            if (next - t).abs() < 1e-15 * (1.0 + t.abs()) {
                t = next;
                break;
            }
            t = next;
        }
        t
    }
}

/// A rational B-spline surface, control points indexed `[u][v]`.
#[derive(Clone)]
pub struct NurbsSurface {
    pub degree_u: usize,
    pub degree_v: usize,
    pub control_points: Vec<Vec<V3>>,
    pub weights: Vec<Vec<f64>>,
    pub knots_u: Vec<f64>,
    pub knots_v: Vec<f64>,
    /// A sampled grid over the domain: seeds for inversion and ray intersection, built on
    /// first use so that reading a file pays nothing for surfaces no query touches.
    grid: std::sync::OnceLock<Grid>,
}

/// The surface's own data, without the grid: a cache built on first use, which would make the
/// output depend on which queries have been asked.
impl std::fmt::Debug for NurbsSurface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NurbsSurface")
            .field("degree_u", &self.degree_u)
            .field("degree_v", &self.degree_v)
            .field("control_points", &self.control_points)
            .field("weights", &self.weights)
            .field("knots_u", &self.knots_u)
            .field("knots_v", &self.knots_v)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Default)]
struct Grid {
    us: Vec<f64>,
    vs: Vec<f64>,
    points: Vec<Vec<V3>>,
    /// Per cell: how far the surface strays from the cell's triangles (sampled).
    deviation: Vec<Vec<f64>>,
    /// Blocks of cells (`i0..i1`, `j0..j1`) with a box holding every seed their cells can
    /// give, so a ray tests only the cells of blocks it passes.
    blocks: Vec<(usize, usize, usize, usize, [V3; 2])>,
}

impl Grid {
    fn new(us: Vec<f64>, vs: Vec<f64>, points: Vec<Vec<V3>>, deviation: Vec<Vec<f64>>) -> Self {
        let mut blocks = Vec::new();
        for i0 in (0..us.len() - 1).step_by(BLOCK) {
            for j0 in (0..vs.len() - 1).step_by(BLOCK) {
                let (i1, j1) = (
                    (i0 + BLOCK).min(us.len() - 1),
                    (j0 + BLOCK).min(vs.len() - 1),
                );
                let (mut lo, mut hi, mut pad) =
                    ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3], 0.0f64);
                for i in i0..i1 {
                    for j in j0..j1 {
                        pad = pad.max(seed_pad(deviation[i][j]));
                        for p in [
                            points[i][j],
                            points[i + 1][j],
                            points[i + 1][j + 1],
                            points[i][j + 1],
                        ] {
                            for k in 0..3 {
                                lo[k] = lo[k].min(p[k]);
                                hi[k] = hi[k].max(p[k]);
                            }
                        }
                    }
                }
                let box_ = [lo.map(|c| c - pad), hi.map(|c| c + pad)];
                blocks.push((i0, i1, j0, j1, box_));
            }
        }
        Grid {
            us,
            vs,
            points,
            deviation,
            blocks,
        }
    }
}

/// Cells per block side.
const BLOCK: usize = 8;

/// Whether a ray reaches a box within `t_max`.
fn ray_meets(origin: V3, dir: V3, t_max: f64, [lo, hi]: [V3; 2]) -> bool {
    let (mut t0, mut t1) = (0.0f64, t_max);
    for i in 0..3 {
        if dir[i].abs() < 1e-300 {
            if origin[i] < lo[i] || origin[i] > hi[i] {
                return false;
            }
            continue;
        }
        let (a, b) = ((lo[i] - origin[i]) / dir[i], (hi[i] - origin[i]) / dir[i]);
        (t0, t1) = (t0.max(a.min(b)), t1.min(a.max(b)));
        if t0 > t1 {
            return false;
        }
    }
    true
}

/// How far (mm) a hinted inversion may land from its point before a global search is tried.
const HINT_RETRY_DISTANCE: f64 = 0.05;

/// How many times a ray may halve a cell that strays from its triangles by more than a quarter
/// of its diagonal (where the grid's refinement stopped at `GRID_POINTS`).
const REFINE_DEPTH: usize = 6;

/// A crossing this near tangent (the cosine between ray and normal) is a graze: Newton's method
/// settles a touching ray within 1e-10 of its surface, where the cosine is still about
/// `sqrt(2e-10 / radius)`, and a ray this close to tangent may cross twice within a cell.
const GRAZING_COS: f64 = 1e-3;

/// Grid subdivisions per knot span in each direction, before any refinement.
const GRID_PER_SPAN: usize = 6;

/// The grid is refined until its cells' triangles stray from the surface by no more than this
/// fraction of the surface's size...
const GRID_DEVIATION: f64 = 1e-3;

/// ...or until it would have more points than this.
const GRID_POINTS: usize = 1 << 18;

fn grid_params(knots: &[f64], degree: usize, per_span: usize) -> Vec<f64> {
    let (lo, hi) = (knots[degree], knots[knots.len() - degree - 1]);
    let mut out = Vec::new();
    for w in breaks(knots, degree).windows(2) {
        for i in 0..per_span {
            out.push(w[0] + (w[1] - w[0]) * i as f64 / per_span as f64);
        }
    }
    out.push(hi);
    // Very coarse surfaces (one span) still need enough samples to seed Newton.
    let least = 16 * per_span / GRID_PER_SPAN;
    if out.len() <= least {
        out = (0..=least)
            .map(|i| lo + (hi - lo) * i as f64 / least as f64)
            .collect();
    }
    out
}

impl NurbsSurface {
    pub(crate) fn transform_points(&mut self, point: impl Fn(V3) -> V3) {
        for p in self.control_points.iter_mut().flatten() {
            *p = point(*p);
        }
        self.grid = std::sync::OnceLock::new();
    }

    /// `None` for malformed arrays (see [`NurbsCurve::new`]).
    pub fn new(
        degree_u: usize,
        degree_v: usize,
        control_points: Vec<Vec<V3>>,
        weights: Vec<Vec<f64>>,
        knots_u: Vec<f64>,
        knots_v: Vec<f64>,
    ) -> Option<Self> {
        let nv = control_points.first().map_or(0, Vec::len);
        let rows_ok = control_points.len() == weights.len()
            && control_points
                .iter()
                .zip(&weights)
                .all(|(c, w)| c.len() == nv && w.len() == nv)
            && weights.iter().flatten().all(|w| *w > 0.0);
        if !rows_ok
            || !well_formed(degree_u, control_points.len(), weights.len(), &knots_u)
            || !well_formed(degree_v, nv, nv, &knots_v)
        {
            return None;
        }
        Some(NurbsSurface {
            degree_u,
            degree_v,
            control_points,
            weights,
            knots_u,
            knots_v,
            grid: std::sync::OnceLock::new(),
        })
    }

    /// A box that certainly contains the surface: its control points' (the convex-hull property).
    pub fn control_bounds(&self) -> (V3, V3) {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in self.control_points.iter().flatten() {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    }

    fn grid(&self) -> &Grid {
        self.grid.get_or_init(|| {
            // Halve the cells in each direction whose triangles stray further than
            // GRID_DEVIATION of the surface's size, so that a seed's pad stays small.
            let mut per_span = [GRID_PER_SPAN; 2];
            loop {
                let us = grid_params(&self.knots_u, self.degree_u, per_span[0]);
                let vs = grid_params(&self.knots_v, self.degree_v, per_span[1]);
                let points: Vec<Vec<V3>> = us
                    .iter()
                    .map(|&u| vs.iter().map(|&v| self.value(u, v)).collect())
                    .collect();
                let mut worst = [0.0f64; 3];
                let deviation: Vec<Vec<f64>> = (0..us.len() - 1)
                    .map(|i| {
                        (0..vs.len() - 1)
                            .map(|j| {
                                let quad = [
                                    points[i][j],
                                    points[i + 1][j],
                                    points[i + 1][j + 1],
                                    points[i][j + 1],
                                ];
                                let d = self.deviation([us[i], us[i + 1], vs[j], vs[j + 1]], &quad);
                                (0..3).for_each(|k| worst[k] = worst[k].max(d[k]));
                                d[0].max(d[1]).max(d[2])
                            })
                            .collect()
                    })
                    .collect();
                let mut lo = [f64::INFINITY; 3];
                let mut hi = [f64::NEG_INFINITY; 3];
                for p in points.iter().flatten() {
                    for k in 0..3 {
                        lo[k] = lo[k].min(p[k]);
                        hi[k] = hi[k].max(p[k]);
                    }
                }
                let target = GRID_DEVIATION * geom::dist(lo, hi);
                // Along u, along v, or (a twist, seen only at the centre) both.
                let mut finer = [worst[0] > target, worst[1] > target];
                if worst[2] > target && !finer[0] && !finer[1] {
                    finer = [true, true];
                }
                let room = us.len() * vs.len() * 4 <= GRID_POINTS;
                if !room || finer == [false, false] {
                    return Grid::new(us, vs, points, deviation);
                }
                for k in 0..2 {
                    if finer[k] {
                        per_span[k] *= 2;
                    }
                }
            }
        })
    }

    /// How far the surface strays over `[u0, u1] × [v0, v1]` from the two triangles of its
    /// corners *quad* (in the grid's corner order, split along `quad[0]`–`quad[2]`): sampled at
    /// the midpoints of the edges along u, of those along v, and at the centre.
    fn deviation(&self, [u0, u1, v0, v1]: [f64; 4], quad: &[V3; 4]) -> [f64; 3] {
        let (um, vm) = (0.5 * (u0 + u1), 0.5 * (v0 + v1));
        let mid = |a: V3, b: V3| geom::scale(geom::add(a, b), 0.5);
        let off = |u: f64, v: f64, chord: V3| geom::dist(self.value(u, v), chord);
        [
            off(um, v0, mid(quad[0], quad[1])).max(off(um, v1, mid(quad[2], quad[3]))),
            off(u1, vm, mid(quad[1], quad[2])).max(off(u0, vm, mid(quad[3], quad[0]))),
            off(um, vm, mid(quad[0], quad[2])),
        ]
    }

    pub fn domain(&self) -> (f64, f64, f64, f64) {
        let (pu, pv) = (self.degree_u, self.degree_v);
        (
            self.knots_u[pu],
            self.knots_u[self.knots_u.len() - pu - 1],
            self.knots_v[pv],
            self.knots_v[self.knots_v.len() - pv - 1],
        )
    }

    pub fn value(&self, u: f64, v: f64) -> V3 {
        let (u0, u1, v0, v1) = self.domain();
        let (u, v) = (u.clamp(u0, u1), v.clamp(v0, v1));
        let nu = self.control_points.len();
        let nv = self.control_points[0].len();
        let su = find_span(&self.knots_u, self.degree_u, nu, u);
        let sv = find_span(&self.knots_v, self.degree_v, nv, v);
        let bu = basis(&self.knots_u, self.degree_u, su, u);
        let bv = basis(&self.knots_v, self.degree_v, sv, v);
        let mut acc = [0.0; 4];
        for (a, bua) in bu.iter().enumerate() {
            let i = su - self.degree_u + a;
            for (b, bvb) in bv.iter().enumerate() {
                let j = sv - self.degree_v + b;
                let w = self.weights[i][j] * bua * bvb;
                let c = self.control_points[i][j];
                acc = [
                    acc[0] + c[0] * w,
                    acc[1] + c[1] * w,
                    acc[2] + c[2] * w,
                    acc[3] + w,
                ];
            }
        }
        [acc[0] / acc[3], acc[1] / acc[3], acc[2] / acc[3]]
    }

    /// Where the surface is furthest along *d* within *reach* = (du, dv) of (u, v), inside its
    /// domain: alternate golden-section searches along u and v from the start, which should be
    /// the best of a grid of that spacing. Never worse than the start.
    pub fn extreme_near(&self, (u, v): (f64, f64), (du, dv): (f64, f64), d: V3) -> (f64, f64) {
        let (u0, u1, v0, v1) = self.domain();
        let g = |u: f64, v: f64| geom::dot(self.value(u, v), d);
        let r = 0.5 * (5f64.sqrt() - 1.0);
        let search = |lo: f64, hi: f64, f: &dyn Fn(f64) -> f64| {
            let (mut x0, mut x1) = (lo, hi);
            for _ in 0..50 {
                let (c, e) = (x1 - r * (x1 - x0), x0 + r * (x1 - x0));
                if f(c) > f(e) {
                    x1 = e;
                } else {
                    x0 = c;
                }
            }
            0.5 * (x0 + x1)
        };
        let (mut bu, mut bv) = (u, v);
        for _ in 0..4 {
            let nu = search((u - du).max(u0), (u + du).min(u1), &|s| g(s, bv));
            if g(nu, bv) > g(bu, bv) {
                bu = nu;
            }
            let nv = search((v - dv).max(v0), (v + dv).min(v1), &|s| g(bu, s));
            if g(bu, nv) > g(bu, bv) {
                bv = nv;
            }
        }
        (bu, bv)
    }

    /// The point and its exact first partial derivatives (quotient rule on the homogeneous form).
    pub fn value_and_partials(&self, u: f64, v: f64) -> (V3, V3, V3) {
        let (u0, u1, v0, v1) = self.domain();
        let (u, v) = (u.clamp(u0, u1), v.clamp(v0, v1));
        let nu = self.control_points.len();
        let nv = self.control_points[0].len();
        let su = find_span(&self.knots_u, self.degree_u, nu, u);
        let sv = find_span(&self.knots_v, self.degree_v, nv, v);
        let (bu, du) = basis_and_derivative(&self.knots_u, self.degree_u, su, u);
        let (bv, dv) = basis_and_derivative(&self.knots_v, self.degree_v, sv, v);
        let (mut a, mut a_u, mut a_v) = ([0.0; 4], [0.0; 4], [0.0; 4]);
        for i in 0..bu.len() {
            let ci = su - self.degree_u + i;
            for j in 0..bv.len() {
                let cj = sv - self.degree_v + j;
                let w = self.weights[ci][cj];
                let c = self.control_points[ci][cj];
                let h = [c[0] * w, c[1] * w, c[2] * w, w];
                for k in 0..4 {
                    a[k] += bu[i] * bv[j] * h[k];
                    a_u[k] += du[i] * bv[j] * h[k];
                    a_v[k] += bu[i] * dv[j] * h[k];
                }
            }
        }
        let point = [a[0] / a[3], a[1] / a[3], a[2] / a[3]];
        let derive = |d: [f64; 4]| [0, 1, 2].map(|k| (d[k] - d[3] * point[k]) / a[3]);
        (point, derive(a_u), derive(a_v))
    }

    pub fn invert(&self, p: V3, hint: Option<(f64, f64)>) -> (f64, f64) {
        let (mut u, mut v) = hint.unwrap_or_else(|| self.nearest_grid(p));
        let (u0, u1, v0, v1) = self.domain();
        let settle = 1e-12 * (u1 - u0).abs().max((v1 - v0).abs()).max(1e-300);
        let mut residual = f64::INFINITY;
        for _ in 0..50 {
            let (s, su, sv) = self.value_and_partials(u, v);
            let r = geom::sub(p, s);
            residual = geom::norm(r);
            let (a, b, c) = (geom::dot(su, su), geom::dot(su, sv), geom::dot(sv, sv));
            let (g0, g1) = (geom::dot(su, r), geom::dot(sv, r));
            let det = a * c - b * b;
            if det.abs() < 1e-300 {
                break;
            }
            let du = (c * g0 - b * g1) / det;
            let dv = (a * g1 - b * g0) / det;
            let (nu, nv) = ((u + du).clamp(u0, u1), (v + dv).clamp(v0, v1));
            let moved = (nu - u).abs() + (nv - v).abs();
            u = nu;
            v = nv;
            if moved < settle {
                break;
            }
        }
        // Boundary samples sit off their surface by up to the edge tolerance; only a hinted
        // start that lands clearly further away has found a wrong local minimum.
        if hint.is_some() && residual > HINT_RETRY_DISTANCE {
            let fresh = self.invert(p, None);
            if geom::dist(self.value(fresh.0, fresh.1), p) < residual {
                return fresh;
            }
        }
        (u, v)
    }

    fn nearest_grid(&self, p: V3) -> (f64, f64) {
        let g = self.grid();
        let mut best = (f64::INFINITY, 0, 0);
        for (i, row) in g.points.iter().enumerate() {
            for (j, q) in row.iter().enumerate() {
                let d = geom::dist(*q, p);
                if d < best.0 {
                    best = (d, i, j);
                }
            }
        }
        (g.us[best.1], g.vs[best.2])
    }

    /// Ray hits `(t, u, v)`, plus a grazing flag when any hit is near-tangent or when the ray
    /// comes within [`COORD_FLOOR`] of the surface somewhere no crossing could be resolved (so
    /// the hits cannot be trusted for parity).
    pub fn ray_hits(&self, origin: V3, dir: V3, t_max: f64) -> (Vec<(f64, f64, f64)>, bool) {
        let g = self.grid();
        let mut seeds = Vec::new();
        let cells = g
            .blocks
            .iter()
            .filter(|b| ray_meets(origin, dir, t_max, b.4))
            .flat_map(|&(i0, i1, j0, j1, _)| {
                (i0..i1).flat_map(move |i| (j0..j1).map(move |j| (i, j)))
            });
        for (i, j) in cells {
            let quad = [
                g.points[i][j],
                g.points[i + 1][j],
                g.points[i + 1][j + 1],
                g.points[i][j + 1],
            ];
            let deviation = g.deviation[i][j];
            // A cell seeds only from its two triangles, where the line passes within the pad of
            // them, unless it is split (`seed_cell`): the line then passes the corners' padded
            // box, a far cheaper test than the triangles' when it misses.
            if deviation <= 0.25 * diagonal(&quad)
                && !line_near(origin, dir, &quad, seed_pad(deviation))
            {
                continue;
            }
            let cell = [g.us[i], g.us[i + 1], g.vs[j], g.vs[j + 1]];
            let ray = (origin, dir, t_max);
            self.seed_cell(ray, cell, quad, deviation, 0, &mut seeds);
        }
        let mut hits: Vec<(f64, f64, f64)> = Vec::new();
        let mut grazing = false;
        // Nearest first. Every seed is refined, even one beside a hit already found: a ray can
        // cross a curved cell twice.
        seeds.sort_by(|a, b| {
            (a.0.total_cmp(&b.0))
                .then(a.1.total_cmp(&b.1))
                .then(a.2.total_cmp(&b.2))
        });
        for (_, u, v) in seeds {
            // Where the ray comes nearest the surface from this seed: clear of it, a miss;
            // otherwise a crossing for Newton's method to resolve, or the ray is ambiguous (but
            // for a touch where the ray starts, on the surface itself).
            let (gap, near) = self.closest_approach(origin, dir, (u, v));
            if gap > COORD_FLOOR || near.0 < -COORD_FLOOR || near.0 > t_max + COORD_FLOOR {
                continue;
            }
            let Some((t, u, v)) = self.ray_newton(origin, dir, near) else {
                grazing |= near.0 > COORD_FLOOR;
                continue;
            };
            if t <= 1e-12 || t > t_max {
                continue;
            }
            if hits
                .iter()
                .any(|h| (h.0 - t).abs() < 1e-7 * (1.0 + t.abs()))
            {
                continue;
            }
            let (_, su, sv) = self.value_and_partials(u, v);
            let normal = geom::cross(su, sv);
            let cos = geom::dot(normal, dir).abs() / (geom::norm(normal).max(1e-300));
            if cos < GRAZING_COS {
                grazing = true;
            }
            hits.push((t, u, v));
        }
        (hits, grazing)
    }

    /// Seeds `(t, u, v)` from each triangle of the cell `[u0, u1, v0, v1]` (corners *quad*)
    /// that the ray passes near, padded for the cell's chordal *deviation*. A cell that strays
    /// from its triangles by more than a quarter of its diagonal (a grid refinement cut short)
    /// is split instead, so that a seed stays near enough its crossing for Newton's method.
    fn seed_cell(
        &self,
        ray: (V3, V3, f64),
        cell: [f64; 4],
        quad: [V3; 4],
        deviation: f64,
        depth: usize,
        seeds: &mut Vec<(f64, f64, f64)>,
    ) {
        let (origin, dir, t_max) = ray;
        let [u0, u1, v0, v1] = cell;
        if deviation > 0.25 * diagonal(&quad) && depth < REFINE_DEPTH {
            let (us, vs) = ([u0, 0.5 * (u0 + u1), u1], [v0, 0.5 * (v0 + v1), v1]);
            let corner = |i: usize, j: usize| match (i, j) {
                (0, 0) => quad[0],
                (2, 0) => quad[1],
                (2, 2) => quad[2],
                (0, 2) => quad[3],
                _ => self.value(us[i], vs[j]),
            };
            let p = [0, 1, 2].map(|i| [0, 1, 2].map(|j| corner(i, j)));
            for (a, b) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let q = [p[a][b], p[a + 1][b], p[a + 1][b + 1], p[a][b + 1]];
                let sub = [us[a], us[a + 1], vs[b], vs[b + 1]];
                let dev = self.deviation(sub, &q).into_iter().fold(0.0, f64::max);
                let reach = seed_pad(dev);
                let mut lo = [f64::INFINITY; 3];
                let mut hi = [f64::NEG_INFINITY; 3];
                for c in q {
                    for k in 0..3 {
                        lo[k] = lo[k].min(c[k] - reach);
                        hi[k] = hi[k].max(c[k] + reach);
                    }
                }
                if ray_meets(origin, dir, t_max, [lo, hi]) {
                    self.seed_cell(ray, sub, q, dev, depth + 1, seeds);
                }
            }
            return;
        }
        let pad = seed_pad(deviation);
        for tri in [[0, 1, 2], [0, 2, 3]] {
            if let Some((t, a, b)) =
                ray_triangle(origin, dir, quad[tri[0]], quad[tri[1]], quad[tri[2]], pad)
            {
                let (pu, pv) = match tri {
                    [0, 1, 2] => (u0 + (u1 - u0) * (a + b), v0 + (v1 - v0) * b),
                    _ => (u0 + (u1 - u0) * a, v0 + (v1 - v0) * (a + b)),
                };
                seeds.push((t, pu.clamp(u0, u1), pv.clamp(v0, v1)));
            }
        }
    }

    /// Newton's method for the ray's crossing from the seed `(t, u, v)`; `None` when it does
    /// not converge.
    fn ray_newton(&self, origin: V3, dir: V3, seed: (f64, f64, f64)) -> Option<(f64, f64, f64)> {
        let (u0, u1, v0, v1) = self.domain();
        let (mut t, mut u, mut v) = seed;
        for _ in 0..40 {
            let (s, su, sv) = self.value_and_partials(u, v);
            let f = geom::sub(s, geom::add(origin, geom::scale(dir, t)));
            if geom::norm(f) < 1e-10 {
                return Some((t, u, v));
            }
            // Solve [su sv -dir] (du dv dt) = -f by Cramer's rule.
            let m = [su, sv, geom::scale(dir, -1.0)];
            let det = geom::dot(m[0], geom::cross(m[1], m[2]));
            if det.abs() < 1e-300 {
                return None;
            }
            let rhs = geom::scale(f, -1.0);
            let du = geom::dot(rhs, geom::cross(m[1], m[2])) / det;
            let dv = geom::dot(m[0], geom::cross(rhs, m[2])) / det;
            let dt = geom::dot(m[0], geom::cross(m[1], rhs)) / det;
            u = (u + du).clamp(u0, u1);
            v = (v + dv).clamp(v0, v1);
            t += dt;
        }
        None
    }

    /// The nearest the line through *origin* along the unit *dir* comes to the surface, found
    /// from `(u, v)` by damped Gauss–Newton within the domain: the distance there and its
    /// `(t, u, v)`. A local minimum, which is what a seed's own cell needs; the search stops
    /// once the line is well within [`COORD_FLOOR`] (Newton's method finishes a crossing), or
    /// once a step gains under a hundredth while the line is still clear of it (a miss creeping
    /// towards its minimum).
    fn closest_approach(&self, origin: V3, dir: V3, (u, v): (f64, f64)) -> (f64, (f64, f64, f64)) {
        let (u0, u1, v0, v1) = self.domain();
        // A vector's part across the line, and a surface point's offset from the line.
        let across = |x: V3| geom::sub(x, geom::scale(dir, geom::dot(x, dir)));
        let gap = |u: f64, v: f64| geom::norm(across(geom::sub(self.value(u, v), origin)));
        let (mut u, mut v) = (u, v);
        for _ in 0..50 {
            let (s, su, sv) = self.value_and_partials(u, v);
            let r = across(geom::sub(s, origin));
            let here = geom::norm(r);
            if here <= 0.1 * COORD_FLOOR {
                break;
            }
            let (ju, jv) = (across(su), across(sv));
            let (a, b, c) = (geom::dot(ju, ju), geom::dot(ju, jv), geom::dot(jv, jv));
            let damping = 1e-9 * (a + c);
            let (a, c) = (a + damping, c + damping);
            let (g0, g1) = (geom::dot(ju, r), geom::dot(jv, r));
            let det = a * c - b * b;
            if det <= 0.0 {
                break;
            }
            let (mut du, mut dv) = (-(c * g0 - b * g1) / det, -(a * g1 - b * g0) / det);
            // Along a bound the step pushes past, search the other parameter alone.
            if (u <= u0 && du < 0.0) || (u >= u1 && du > 0.0) {
                (du, dv) = (0.0, -g1 / c);
            } else if (v <= v0 && dv < 0.0) || (v >= v1 && dv > 0.0) {
                (du, dv) = (-g0 / a, 0.0);
            }
            let mut step = 1.0;
            let mut next = None;
            for _ in 0..30 {
                let (nu, nv) = ((u + step * du).clamp(u0, u1), (v + step * dv).clamp(v0, v1));
                let there = gap(nu, nv);
                if there < here {
                    next = Some((nu, nv, there));
                    break;
                }
                step *= 0.5;
            }
            let Some((nu, nv, there)) = next else { break };
            (u, v) = (nu, nv);
            if there > COORD_FLOOR && there > 0.99 * here {
                break;
            }
        }
        let w = geom::sub(self.value(u, v), origin);
        (geom::norm(across(w)), (geom::dot(w, dir), u, v))
    }
}

/// A cell's longer diagonal.
fn diagonal(quad: &[V3; 4]) -> f64 {
    geom::dist(quad[0], quad[2]).max(geom::dist(quad[1], quad[3]))
}

/// How near a cell's triangles a ray must pass to seed a crossing: twice the cell's sampled
/// chordal deviation (its samples can miss the worst of it), and never less than
/// [`COORD_FLOOR`], within which a miss is unresolved rather than clean.
fn seed_pad(deviation: f64) -> f64 {
    2.0 * deviation + COORD_FLOOR
}

/// Where the line through *o* along the unit *d* meets the triangle, or else passes within
/// *pad* of it: `(t, a, b)`, the barycentrics of the meeting or of the nearest point of the
/// triangle (on an edge, where a line that misses a triangle comes nearest it).
fn ray_triangle(o: V3, d: V3, p0: V3, p1: V3, p2: V3, pad: f64) -> Option<(f64, f64, f64)> {
    let e1 = geom::sub(p1, p0);
    let e2 = geom::sub(p2, p0);
    // Möller–Trumbore.
    let h = geom::cross(d, e2);
    let det = geom::dot(e1, h);
    if det.abs() >= 1e-300 {
        let s = geom::sub(o, p0);
        let a = geom::dot(s, h) / det;
        let q = geom::cross(s, e1);
        let b = geom::dot(d, q) / det;
        if a >= 0.0 && b >= 0.0 && a + b <= 1.0 {
            return Some((geom::dot(e2, q) / det, a, b));
        }
    }
    let mut best: Option<(f64, f64, f64, f64)> = None;
    for (k, start, run) in [(0, p0, e1), (1, p0, e2), (2, p1, geom::sub(p2, p1))] {
        let (gap, t, s) = line_segment(o, d, start, run);
        if gap <= pad && best.is_none_or(|b| gap < b.0) {
            // The barycentrics of the point *s* along the edge.
            let (a, b) = match k {
                0 => (s, 0.0),
                1 => (0.0, s),
                _ => (1.0 - s, s),
            };
            best = Some((gap, t, a, b));
        }
    }
    best.map(|(_, t, a, b)| (t, a, b))
}

/// Whether the line through *o* along *d* (both ways, as [`ray_triangle`] takes it) passes
/// the box of *corners* padded by *pad*, and a hair more for rounding: it does wherever it
/// passes within *pad* of a triangle of them.
fn line_near(o: V3, d: V3, corners: &[V3; 4], pad: f64) -> bool {
    let pad = pad * (1.0 + 1e-9) + 1e-12;
    let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
    for k in 0..3 {
        let lo = corners.iter().map(|c| c[k]).fold(f64::INFINITY, f64::min) - pad;
        let hi = corners
            .iter()
            .map(|c| c[k])
            .fold(f64::NEG_INFINITY, f64::max)
            + pad;
        if d[k].abs() < 1e-300 {
            if o[k] < lo || o[k] > hi {
                return false;
            }
            continue;
        }
        let (a, b) = ((lo - o[k]) / d[k], (hi - o[k]) / d[k]);
        (t0, t1) = (t0.max(a.min(b)), t1.min(a.max(b)));
        if t0 > t1 {
            return false;
        }
    }
    true
}

/// The nearest the line through *o* along the unit *d* comes to the segment from *p* along
/// *e*: the distance, the line's `t` and the segment's fraction there.
fn line_segment(o: V3, d: V3, p: V3, e: V3) -> (f64, f64, f64) {
    let w = geom::sub(p, o);
    let (b, c) = (geom::dot(d, e), geom::dot(e, e));
    let (dw, ew) = (geom::dot(d, w), geom::dot(e, w));
    let across = c - b * b;
    let s = if across > 1e-300 {
        ((b * dw - ew) / across).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let t = dw + s * b;
    let gap = geom::norm(geom::sub(
        geom::add(w, geom::scale(e, s)),
        geom::scale(d, t),
    ));
    (gap, t, s)
}

/// Values for one exact, clamped parameter of an immutable surface.
struct PreparedAxis {
    parameter: u64,
    span: usize,
    basis: Basis,
    derivative: Basis,
}

/// Integral-local reuse of exact operands; nodes, arithmetic order and refusal stay unchanged.
pub(crate) struct SurfaceEvaluation<'a> {
    surface: &'a NurbsSurface,
    u: Option<PreparedAxis>,
    v: Option<PreparedAxis>,
    spans: Option<(usize, usize)>,
    controls: Vec<[f64; 4]>,
}

impl<'a> SurfaceEvaluation<'a> {
    pub(crate) fn new(surface: &'a NurbsSurface) -> Self {
        Self {
            surface,
            u: None,
            v: None,
            spans: None,
            controls: Vec::with_capacity((surface.degree_u + 1) * (surface.degree_v + 1)),
        }
    }

    pub(crate) fn value_and_partials(&mut self, u: f64, v: f64) -> (V3, V3, V3) {
        let surface = self.surface;
        let (u0, u1, v0, v1) = surface.domain();
        let (u, v) = (u.clamp(u0, u1), v.clamp(v0, v1));
        if !(u.is_finite() && v.is_finite()) {
            return surface.value_and_partials(u, v);
        }
        let prepare = |slot: &mut Option<PreparedAxis>, t: f64, knots: &[f64], degree, count| {
            if slot
                .as_ref()
                .is_none_or(|entry| entry.parameter != t.to_bits())
            {
                let span = find_span(knots, degree, count, t);
                let (basis, derivative) = basis_and_derivative(knots, degree, span, t);
                *slot = Some(PreparedAxis {
                    parameter: t.to_bits(),
                    span,
                    basis,
                    derivative,
                });
            }
        };
        prepare(
            &mut self.u,
            u,
            &surface.knots_u,
            surface.degree_u,
            surface.control_points.len(),
        );
        prepare(
            &mut self.v,
            v,
            &surface.knots_v,
            surface.degree_v,
            surface.control_points[0].len(),
        );
        let (Some(pu), Some(pv)) = (self.u.as_ref(), self.v.as_ref()) else {
            return surface.value_and_partials(u, v);
        };
        if self.spans != Some((pu.span, pv.span)) {
            self.controls.clear();
            for ci in pu.span - surface.degree_u..=pu.span {
                for cj in pv.span - surface.degree_v..=pv.span {
                    let c = surface.control_points[ci][cj];
                    let w = surface.weights[ci][cj];
                    self.controls.push([c[0] * w, c[1] * w, c[2] * w, w]);
                }
            }
            self.spans = Some((pu.span, pv.span));
        }
        let (bu, du, bv, dv) = (&pu.basis, &pu.derivative, &pv.basis, &pv.derivative);
        let (mut a, mut a_u, mut a_v) = ([0.0; 4], [0.0; 4], [0.0; 4]);
        // Keep the original u/v/component order, proving slice bounds once per row.
        for ((&bu, &du), row) in bu
            .iter()
            .zip(du.iter())
            .zip(self.controls.chunks_exact(bv.len()))
        {
            for ((&bv, &dv), h) in bv.iter().zip(dv.iter()).zip(row) {
                let (b, d_u, d_v) = (bu * bv, du * bv, bu * dv);
                for k in 0..4 {
                    a[k] += b * h[k];
                    a_u[k] += d_u * h[k];
                    a_v[k] += d_v * h[k];
                }
            }
        }
        let point = [a[0] / a[3], a[1] / a[3], a[2] / a[3]];
        let derive = |d: [f64; 4]| [0, 1, 2].map(|k| (d[k] - d[3] * point[k]) / a[3]);
        (point, derive(a_u), derive(a_v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integral_evaluator_preserves_bits_across_spans_and_clamps() {
        let bits = |(p, u, v): (V3, V3, V3)| [p, u, v].map(|v| v.map(f64::to_bits));
        for (degree_u, degree_v) in [(0, 1), (1, 3), (3, 2), (3, 3), (7, 8), (25, 25)] {
            for inner in [[0.2, 0.4, 0.7], [0.2, 0.4, 0.4]] {
                let knots = |degree| {
                    std::iter::repeat_n(0.0, degree + 1)
                        .chain(inner)
                        .chain(std::iter::repeat_n(1.0, degree + 1))
                        .collect()
                };
                let points = (0..degree_u + 4)
                    .map(|i| {
                        (0..degree_v + 4)
                            .map(|j| [i as f64 - 2.0, j as f64 - 3.0, (i * j) as f64 * 0.003])
                            .collect()
                    })
                    .collect();
                let weights = (0..degree_u + 4)
                    .map(|i| {
                        (0..degree_v + 4)
                            .map(|j| 0.5 + ((i + 2 * j) % 7) as f64 * 0.125)
                            .collect()
                    })
                    .collect();
                let surface = NurbsSurface::new(
                    degree_u,
                    degree_v,
                    points,
                    weights,
                    knots(degree_u),
                    knots(degree_v),
                )
                .expect("valid rational test surface");
                let mut evaluator = SurfaceEvaluation::new(&surface);
                let coordinates: Vec<f64> = [
                    -1.0,
                    -0.0,
                    0.0,
                    0.2,
                    0.399999999,
                    0.4,
                    0.400000001,
                    0.7,
                    1.0,
                    2.0,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NAN,
                ]
                .into_iter()
                .chain((0..80).map(|k| k as f64 / 79.0))
                .collect();
                for reverse in [false, true] {
                    for &u in &coordinates {
                        for &v in &coordinates {
                            let (u, v) = if reverse { (v, u) } else { (u, v) };
                            assert_eq!(
                                bits(evaluator.value_and_partials(u, v)),
                                bits(surface.value_and_partials(u, v)),
                                "degrees {degree_u}/{degree_v}, ({u}, {v})"
                            );
                        }
                    }
                }
                assert_eq!(evaluator.controls.len(), (degree_u + 1) * (degree_v + 1));
                assert_eq!(
                    evaluator.controls.capacity(),
                    (degree_u + 1) * (degree_v + 1)
                );
            }
        }
    }

    /// A rational biquadratic quarter-cylinder patch: exact circle in u, line in v.
    fn quarter_cylinder() -> NurbsSurface {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let rows = [[1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let control_points = rows
            .iter()
            .map(|&[x, y]| vec![[x, y, 0.0], [x, y, 2.0]])
            .collect();
        let weights = vec![vec![1.0, 1.0], vec![w, w], vec![1.0, 1.0]];
        NurbsSurface::new(
            2,
            1,
            control_points,
            weights,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap()
    }

    #[test]
    fn analytic_partials_match_finite_differences() {
        let s = quarter_cylinder();
        for (u, v) in [(0.2, 0.3), (0.5, 0.5), (0.9, 0.1)] {
            let (p, su, sv) = s.value_and_partials(u, v);
            assert!(
                (p[0].hypot(p[1]) - 1.0).abs() < 1e-12,
                "on the unit cylinder"
            );
            let h = 1e-6;
            let fu = geom::scale(geom::sub(s.value(u + h, v), s.value(u - h, v)), 0.5 / h);
            let fv = geom::scale(geom::sub(s.value(u, v + h), s.value(u, v - h)), 0.5 / h);
            assert!(geom::dist(su, fu) < 1e-6 && geom::dist(sv, fv) < 1e-6);
        }
    }

    #[test]
    fn inversion_recovers_parameters() {
        let s = quarter_cylinder();
        for (u, v) in [(0.1, 0.9), (0.6, 0.4)] {
            let (iu, iv) = s.invert(s.value(u, v), None);
            assert!((iu - u).abs() < 1e-9 && (iv - v).abs() < 1e-9);
        }
    }

    #[test]
    fn ray_hits_find_the_patch() {
        let s = quarter_cylinder();
        let (hits, grazing) =
            s.ray_hits([0.0, 0.0, 1.0], geom::unit([1.0, 1.0, 0.0]).unwrap(), 10.0);
        assert_eq!(hits.len(), 1);
        assert!(!grazing);
        assert!((hits[0].0 - 1.0).abs() < 1e-9);
    }

    /// A thin flat strip bent round a quarter circle (radii 54.36 to 55, the shape of a thread
    /// flank): its seed grid's chords cut inside the arc by more than the strip is wide.
    fn thin_arc_strip() -> NurbsSurface {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let rows = [[1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let control_points = rows
            .iter()
            .map(|&[x, y]| [54.36, 55.0].map(|r| [r * x, r * y, 0.0]).to_vec())
            .collect();
        let weights = vec![vec![1.0, 1.0], vec![w, w], vec![1.0, 1.0]];
        NurbsSurface::new(
            2,
            1,
            control_points,
            weights,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap()
    }

    #[test]
    fn ray_hits_find_crossings_outside_the_seed_grids_chords() {
        let s = thin_arc_strip();
        // Near the outer rim, midway between two grid columns, where the arc bulges furthest
        // beyond the chords (about 0.06 mm, against a strip 0.64 mm wide in 16 rows).
        for degrees in [47.8125f64, 20.0, 84.375] {
            let (sin, cos) = degrees.to_radians().sin_cos();
            let target = [54.99 * cos, 54.99 * sin, 0.0];
            let dir = geom::unit([0.3, -0.2, -1.0]).unwrap();
            let origin = geom::sub(target, geom::scale(dir, 10.0));
            let (hits, grazing) = s.ray_hits(origin, dir, 100.0);
            assert!(!grazing);
            assert_eq!(hits.len(), 1, "at {degrees}°");
            assert!((hits[0].0 - 10.0).abs() < 1e-9);
        }
    }

    #[test]
    fn ray_hits_flag_a_ray_that_touches_or_nearly_misses() {
        let s = quarter_cylinder();
        let (sin, cos) = 0.7f64.sin_cos();
        let tangent = [-sin, cos, 0.0];
        // Touching the cylinder along a tangent: not a clean crossing.
        let touch = [cos, sin, 1.0];
        let (_, grazing) = s.ray_hits(geom::sub(touch, geom::scale(tangent, 5.0)), tangent, 10.0);
        assert!(grazing);
        // Passing clearly outside it: a clean miss, not an ambiguous one.
        let clear = [1.001 * cos, 1.001 * sin, 1.0];
        let (hits, grazing) =
            s.ray_hits(geom::sub(clear, geom::scale(tangent, 5.0)), tangent, 10.0);
        assert!(hits.is_empty() && !grazing);
    }

    #[test]
    fn malformed_arrays_are_refused() {
        assert!(NurbsCurve::new(2, vec![[0.0; 3]; 3], vec![1.0; 3], vec![0.0, 1.0]).is_none());
        assert!(
            NurbsCurve::new(
                1,
                vec![[0.0; 3], [1.0; 3]],
                vec![1.0, -1.0],
                vec![0.0, 0.0, 1.0, 1.0]
            )
            .is_none()
        );
        assert!(
            NurbsSurface::new(
                1,
                1,
                vec![vec![[0.0; 3]; 2]; 2],
                vec![vec![1.0; 2]; 1],
                vec![0.0, 0.0, 1.0, 1.0],
                vec![0.0, 0.0, 1.0, 1.0]
            )
            .is_none()
        );
    }

    #[test]
    fn closed_curves_are_detected() {
        let square = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 0.0, 0.0],
        ];
        let closed =
            NurbsCurve::new(1, square, vec![1.0; 4], vec![0.0, 0.0, 1.0, 2.0, 3.0, 3.0]).unwrap();
        assert!(closed.is_closed());
        let open = NurbsCurve::new(
            1,
            vec![[0.0; 3], [1.0, 0.0, 0.0]],
            vec![1.0; 2],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        assert!(!open.is_closed());
        assert!((open.invert([0.25, 0.1, 0.0]) - 0.25).abs() < 1e-12);
    }

    #[test]
    fn curve_derivative_matches_finite_differences() {
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let arc = NurbsCurve::new(
            2,
            vec![[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
            vec![1.0, h, 1.0],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        )
        .unwrap();
        for t in [0.1, 0.5, 0.9] {
            let d = arc.derivative(t);
            let e = 1e-6;
            let (p, q) = (arc.value(t + e), arc.value(t - e));
            for k in 0..3 {
                assert!((d[k] - (p[k] - q[k]) / (2.0 * e)).abs() < 1e-7);
            }
            // Tangent to the unit circle.
            assert!(crate::geom::dot(d, arc.value(t)).abs() < 1e-12);
        }
    }
}
