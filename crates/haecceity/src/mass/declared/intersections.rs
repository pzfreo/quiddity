//! Intersections of finite coedges in the receiving face's parameter space.
use super::{DeclaredBoundary, EdgeTrim, on_carrier};
use crate::brep::{Edge, Part};
use crate::geom::{self, Surface, V3};
use crate::mass::{foot, uv_velocity};
use crate::sampling::edge_interval;

#[derive(Clone, Copy)]
struct Span {
    edge: usize,
    sample: usize,
    last: bool,
    uv: [V3; 2],
}

impl Span {
    fn vertex(&self, part: &Part, fraction: f64) -> Option<usize> {
        let ends = part.edges[self.edge].vertices;
        if self.sample == 0 && fraction <= 16. * f64::EPSILON {
            return Some(ends.0);
        }
        if self.last && 1. - fraction <= 16. * f64::EPSILON {
            return Some(ends.1);
        }
        None
    }

    fn left(&self) -> f64 {
        self.uv[0][0].min(self.uv[1][0])
    }
}

fn cross(a: V3, b: V3) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn near(x: f64, reference: f64, period: f64) -> f64 {
    if period.is_finite() {
        x - ((x - reference) / period).round() * period
    } else {
        x
    }
}

/// Rule out finite-curve candidates using coedge boxes before the global sweep.
/// Monotone sample runs have no nonadjacent overlaps; other runs are checked by
/// a local sweep. Every remaining pair is tested in both operand orders, so a
/// tie in the global unstable sort cannot hide a candidate. Unresolved pairs or
/// an exhausted linear work budget defer to the original complete sweep.
fn grouped_clear(spans: &[Span], vertex: impl Fn(&Span, f64) -> Option<usize>) -> bool {
    if spans
        .iter()
        .any(|s| s.uv.iter().flatten().any(|x| !x.is_finite()))
    {
        return false;
    }
    let bounds = |s: &Span| {
        [
            s.uv[0][0].min(s.uv[1][0]),
            s.uv[0][0].max(s.uv[1][0]),
            s.uv[0][1].min(s.uv[1][1]),
            s.uv[0][1].max(s.uv[1][1]),
        ]
    };
    let disjoint =
        |a: [f64; 4], b: [f64; 4]| a[0] > b[1] || b[0] > a[1] || a[2] > b[3] || b[2] > a[3];
    let monotone = |run: &[Span], axis: usize, reverse: bool| {
        let mut previous: Option<&Span> = None;
        for step in 0..run.len() {
            let i = if reverse { run.len() - 1 - step } else { step };
            let span = &run[i];
            let lo = span.uv[0][axis].min(span.uv[1][axis]);
            let hi = span.uv[0][axis].max(span.uv[1][axis]);
            if lo >= hi {
                return false;
            }
            if let Some(previous) = previous {
                let end = previous.uv[0][axis].max(previous.uv[1][axis]);
                if lo < end || (lo == end && span.sample.abs_diff(previous.sample) > 1) {
                    return false;
                }
            }
            previous = Some(span);
        }
        true
    };
    let needs_confirmation = |a: &Span, b: &Span| {
        if a.edge == b.edge && a.sample.abs_diff(b.sample) <= 1 {
            return false;
        }
        let da = geom::sub(a.uv[1], a.uv[0]);
        let db = geom::sub(b.uv[1], b.uv[0]);
        let offset = geom::sub(b.uv[0], a.uv[0]);
        let det = cross(da, db);
        if det == 0. {
            return false;
        }
        let fa = cross(offset, db) / det;
        let fb = cross(offset, da) / det;
        if !(0. ..=1.).contains(&fa) || !(0. ..=1.).contains(&fb) {
            return false;
        }
        if let Some(v) = vertex(a, fa)
            && vertex(b, fb) == Some(v)
        {
            return false;
        }
        true
    };
    let mut budget = spans.len().saturating_mul(8);
    let mut groups = Vec::new();
    for run in spans.chunk_by(|a, b| a.edge == b.edge) {
        if !(0..2).any(|axis| monotone(run, axis, false) || monotone(run, axis, true)) {
            let variation = run.iter().fold([0., 0.], |mut total, span| {
                for (axis, sum) in total.iter_mut().enumerate() {
                    *sum += (span.uv[1][axis] - span.uv[0][axis]).abs();
                }
                total
            });
            let axis = usize::from(variation[1] > variation[0]);
            let mut ordered: Vec<_> = run.iter().collect();
            ordered.sort_unstable_by(|a, b| {
                a.uv[0][axis]
                    .min(a.uv[1][axis])
                    .total_cmp(&b.uv[0][axis].min(b.uv[1][axis]))
            });
            for (i, a) in ordered.iter().enumerate() {
                let right = a.uv[0][axis].max(a.uv[1][axis]);
                let box_a = bounds(a);
                for b in &ordered[i + 1..] {
                    if b.uv[0][axis].min(b.uv[1][axis]) > right {
                        break;
                    }
                    if budget == 0 {
                        return false;
                    }
                    budget -= 1;
                    if disjoint(box_a, bounds(b)) {
                        continue;
                    }
                    if needs_confirmation(a, b) || needs_confirmation(b, a) {
                        return false;
                    }
                }
            }
        }
        let mut box_ = [
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ];
        for span in run {
            let b = bounds(span);
            box_[0] = box_[0].min(b[0]);
            box_[1] = box_[1].max(b[1]);
            box_[2] = box_[2].min(b[2]);
            box_[3] = box_[3].max(b[3]);
        }
        groups.push((run, box_));
    }
    for (i, &(a, box_a)) in groups.iter().enumerate() {
        for &(b, box_b) in &groups[i + 1..] {
            if budget == 0 {
                return false;
            }
            budget -= 1;
            if disjoint(box_a, box_b) {
                continue;
            }
            for sa in a {
                if budget == 0 {
                    return false;
                }
                budget -= 1;
                let bound_a = bounds(sa);
                if disjoint(bound_a, box_b) {
                    continue;
                }
                for sb in b {
                    if budget == 0 {
                        return false;
                    }
                    budget -= 1;
                    if disjoint(bound_a, bounds(sb)) {
                        continue;
                    }
                    // Both orders cover every tie order of the original unstable u sort.
                    if needs_confirmation(sa, sb) || needs_confirmation(sb, sa) {
                        return false;
                    }
                }
            }
        }
    }
    true
}

/// A broad phase over the UV samples already used to build the integration domain.
/// Each span refers back to the actual finite curve for the intersection proof.
pub(crate) struct TrimCrossings {
    spans: Vec<Span>,
    periods: [f64; 2],
    closed: (f64, f64),
    sweep_axes: bool,
}

impl TrimCrossings {
    pub(crate) fn new(
        surface: &Surface,
        boundary: &DeclaredBoundary,
        sweep_axes: bool,
        capacity: usize,
    ) -> Option<Self> {
        let closed = boundary.edges.first_key_value()?.1.spans;
        let (pu, pv) = surface.periodic();
        Some(Self {
            spans: Vec::with_capacity(capacity),
            periods: [
                if pu { std::f64::consts::TAU } else { closed.0 },
                if pv { std::f64::consts::TAU } else { closed.1 },
            ],
            closed,
            sweep_axes,
        })
    }

    pub(crate) fn add_edge(&mut self, edge: usize, forward: bool, points: &[(f64, f64)]) {
        for (i, pair) in points.windows(2).enumerate() {
            let (a, b) = if forward {
                (pair[0], pair[1])
            } else {
                (pair[1], pair[0])
            };
            let mut span = Span {
                edge,
                sample: if forward { i } else { points.len() - 2 - i },
                last: if forward {
                    i + 2 == points.len()
                } else {
                    i == 0
                },
                uv: [[a.0, a.1, 0.], [b.0, b.1, 0.]],
            };
            let mut copies = [0.; 2];
            for (k, &period) in self.periods.iter().enumerate() {
                if period.is_finite() {
                    let a = span.uv[0][k];
                    let b = near(span.uv[1][k], a, period);
                    let shift = ((a + b) / (2. * period)).floor() * period;
                    span.uv[0][k] = a - shift;
                    span.uv[1][k] = b - shift;
                    if span.uv[0][k].min(span.uv[1][k]) <= 0. {
                        copies[k] = period;
                    } else if span.uv[0][k].max(span.uv[1][k]) >= period {
                        copies[k] = -period;
                    }
                }
            }
            self.spans.push(span);
            for shift in [[copies[0], 0., 0.], [0., copies[1], 0.]] {
                if shift != [0.; 3] {
                    self.spans.push(Span {
                        uv: span.uv.map(|p| geom::add(p, shift)),
                        ..span
                    });
                }
            }
            if copies[0] != 0. && copies[1] != 0. {
                let shift = [copies[0], copies[1], 0.];
                self.spans.push(Span {
                    uv: span.uv.map(|p| geom::add(p, shift)),
                    ..span
                });
            }
        }
    }

    pub(crate) fn crosses<'a>(
        mut self,
        boundary: &DeclaredBoundary,
        part: &Part,
        surface: &Surface,
        tolerance: f64,
        mesh: Option<&dyn Fn(usize) -> Option<crate::mesh::MeshEdge<'a>>>,
    ) -> bool {
        if self.sweep_axes
            && grouped_clear(&self.spans, |span, fraction| span.vertex(part, fraction))
        {
            return false;
        }
        self.spans
            .sort_unstable_by(|a, b| a.left().total_cmp(&b.left()));
        let interval = |span: &Span, axis: usize| {
            (
                span.uv[0][axis].min(span.uv[1][axis]),
                span.uv[0][axis].max(span.uv[1][axis]),
            )
        };
        let intersects = |ai: usize, bi: usize, axis: usize, lo: f64, hi: f64| {
            let (sweep_a, sweep_b) = (&self.spans[ai], &self.spans[bi]);
            if sweep_a.edge == sweep_b.edge && sweep_a.sample.abs_diff(sweep_b.sample) <= 1 {
                return false;
            }
            let (b_lo, b_hi) = interval(sweep_b, 1 - axis);
            if b_lo > hi || b_hi < lo {
                return false;
            }
            // Preserve the original u-sweep's operand order in the exact check.
            let (a, b) = if ai < bi {
                (sweep_a, sweep_b)
            } else {
                (sweep_b, sweep_a)
            };
            let da = geom::sub(a.uv[1], a.uv[0]);
            let db = geom::sub(b.uv[1], b.uv[0]);
            let offset = geom::sub(b.uv[0], a.uv[0]);
            let det = cross(da, db);
            if det == 0. {
                return false;
            }
            let fa = cross(offset, db) / det;
            let fb = cross(offset, da) / det;
            if !(0. ..=1.).contains(&fa) || !(0. ..=1.).contains(&fb) {
                return false;
            }
            // An intersection at the shared topological vertex closes the region.
            if let Some(vertex) = a.vertex(part, fa)
                && b.vertex(part, fb) == Some(vertex)
            {
                return false;
            }
            let seed = geom::add(a.uv[0], geom::scale(da, fa));
            let params = [a, b].map(|span| {
                let edge = &part.edges[span.edge];
                let selected = mesh.and_then(|mesh| mesh(span.edge));
                let trim = boundary.edges.get(&span.edge).filter(|t| {
                    if mesh.is_some() {
                        selected.is_some()
                    } else {
                        t.samples(surface, edge).is_some()
                    }
                });
                FiniteCurve::from_samples(
                    edge,
                    trim,
                    surface,
                    span.sample,
                    selected.map(|s| s.samples.ts.as_slice()),
                )
            });
            if let [Some(a), Some(b)] = params {
                self.confirms(
                    [a, b],
                    [fa, fb],
                    seed,
                    geom::norm(da).max(geom::norm(db)),
                    surface,
                    tolerance,
                )
            } else {
                false
            }
        };
        // Sparse u sweeps can finish without counting every overlap first.
        // A dense sweep spends only linear work here before falling back to
        // the existing axis selection, with the same finite-curve checks.
        if self.sweep_axes
            && let Some(result) = self.scan_u(self.spans.len().saturating_mul(4), |a, b, lo, hi| {
                intersects(a, b, 0, lo, hi)
            })
        {
            return result;
        }
        let mut axis = 0;
        let mut order: Vec<usize> = (0..self.spans.len()).collect();
        let count_by_search = self
            .spans
            .iter()
            .all(|s| s.uv.iter().flatten().all(|x| x.is_finite()));
        if self.sweep_axes && self.spans.len() > 32 {
            let overlaps = |indices: &[usize], axis| {
                if count_by_search {
                    let starts: Vec<_> = indices
                        .iter()
                        .map(|&i| interval(&self.spans[i], axis).0)
                        .collect();
                    let mut count = 0u64;
                    for (i, &index) in indices.iter().enumerate() {
                        let right = interval(&self.spans[index], axis).1;
                        let tail = &starts[i + 1..];
                        let mut reach = 1usize;
                        while reach < tail.len() && tail[reach] <= right {
                            reach = reach.saturating_mul(2);
                        }
                        let end = reach.saturating_add(1).min(tail.len());
                        count = count.saturating_add(
                            tail[..end].partition_point(|&left| left <= right) as u64,
                        );
                    }
                    return count;
                }
                let mut ends: Vec<_> = self.spans.iter().map(|s| interval(s, axis).1).collect();
                ends.sort_unstable_by(f64::total_cmp);
                let (mut expired, mut count) = (0usize, 0u64);
                for (i, &index) in indices.iter().enumerate() {
                    let left = interval(&self.spans[index], axis).0;
                    while expired < ends.len() && ends[expired] < left {
                        expired += 1;
                    }
                    count = count.saturating_add(i.saturating_sub(expired) as u64);
                }
                count
            };
            let first = overlaps(&order, 0);
            // A bounded number of candidates per span already gives a
            // linear scan. Only pay for another sort when that bound fails.
            // Both orders retain the same exact finite-curve checks below.
            if first > 4 * self.spans.len() as u64 {
                let mut other = order.clone();
                other.sort_unstable_by(|&i, &j| {
                    interval(&self.spans[i], 1)
                        .0
                        .total_cmp(&interval(&self.spans[j], 1).0)
                });
                if overlaps(&other, 1) < first {
                    axis = 1;
                    order = other;
                }
            }
        }
        for (i, &ai) in order.iter().enumerate() {
            let sweep_a = &self.spans[ai];
            let (lo, hi) = interval(sweep_a, 1 - axis);
            let right = interval(sweep_a, axis).1;
            for &bi in &order[i + 1..] {
                let sweep_b = &self.spans[bi];
                if interval(sweep_b, axis).0 > right {
                    break;
                }
                if intersects(ai, bi, axis, lo, hi) {
                    return true;
                }
            }
        }
        false
    }

    /// `None` requests the full sweep after exhausting a bounded prefix.
    fn scan_u(
        &self,
        budget: usize,
        mut intersects: impl FnMut(usize, usize, f64, f64) -> bool,
    ) -> Option<bool> {
        let mut visited = 0;
        for (i, a) in self.spans.iter().enumerate() {
            let right = a.uv[0][0].max(a.uv[1][0]);
            let lo = a.uv[0][1].min(a.uv[1][1]);
            let hi = a.uv[0][1].max(a.uv[1][1]);
            for (j, b) in self.spans.iter().enumerate().skip(i + 1) {
                if b.left() > right {
                    break;
                }
                if visited == budget {
                    return None;
                }
                visited += 1;
                if intersects(i, j, lo, hi) {
                    return Some(true);
                }
            }
        }
        Some(false)
    }

    fn confirms(
        &self,
        paths: [FiniteCurve<'_>; 2],
        fractions: [f64; 2],
        seed: V3,
        chord_scale: f64,
        surface: &Surface,
        tolerance: f64,
    ) -> bool {
        let mut t = std::array::from_fn::<_, 2, _>(|i| {
            paths[i].a + fractions[i] * (paths[i].b - paths[i].a)
        });
        for _ in 0..12 {
            let [Some((a, du)), Some((b, dv))] = std::array::from_fn(|i| {
                let (mut p, d) = paths[i].value(surface, self.closed, t[i], seed)?;
                for k in 0..2 {
                    p[k] = near(p[k], seed[k], self.periods[k]);
                }
                Some((p, d))
            }) else {
                return false;
            };
            let jac = cross(du, dv);
            if !jac.is_finite() || jac.abs() <= 1e-10 * geom::norm(du) * geom::norm(dv) {
                return false;
            }
            let delta = geom::sub(a, b);
            let scale = geom::norm(a)
                .max(geom::norm(b))
                .max(geom::norm(seed))
                .max(chord_scale);
            if geom::norm(delta) <= 64. * f64::EPSILON * scale {
                let Some((u, v)) = on_carrier(surface, self.closed, a) else {
                    return false;
                };
                let (su, sv) = surface.partials(u, v);
                if surface
                    .singular_v(v)
                    .is_some_and(|pole| (pole - v).abs() <= 64. * f64::EPSILON * pole.abs().max(1.))
                    || geom::norm(geom::cross(su, sv)) <= 1e-10 * geom::norm(su) * geom::norm(sv)
                {
                    return false;
                }
                let p = surface.value(u, v);
                return paths.iter().all(|curve| {
                    geom::dist(p, curve.edge.start) > tolerance
                        && geom::dist(p, curve.edge.end) > tolerance
                });
            }
            t[0] -= cross(delta, dv) / jac;
            t[1] -= cross(delta, du) / jac;
            if (0..2)
                .any(|i| !(paths[i].a.min(paths[i].b)..=paths[i].a.max(paths[i].b)).contains(&t[i]))
            {
                return false;
            }
        }
        false
    }
}

struct FiniteCurve<'a> {
    trim: Option<&'a EdgeTrim>,
    edge: &'a Edge,
    a: f64,
    b: f64,
}

impl<'a> FiniteCurve<'a> {
    fn from_samples(
        edge: &'a Edge,
        trim: Option<&'a EdgeTrim>,
        surface: &Surface,
        sample: usize,
        shared: Option<&[f64]>,
    ) -> Option<Self> {
        let (a, b) = if let Some(trim) = trim {
            let ts = match shared {
                Some(ts) => ts,
                None => &trim.samples(surface, edge)?.ts,
            };
            (*ts.get(sample)?, *ts.get(sample + 1)?)
        } else {
            let (a, b) = edge_interval(
                &edge.curve,
                edge.start,
                edge.end,
                edge.same_sense,
                edge.is_closed(),
            );
            let at = |i| {
                if i == 0 {
                    return Some(a);
                }
                if i + 1 == edge.samples.len() {
                    return Some(b);
                }
                let t = edge.curve.parameter(*edge.samples.get(i)?);
                Some(if let Some(period) = edge.curve.period() {
                    if a <= b {
                        a + (t - a).rem_euclid(period)
                    } else {
                        a - (a - t).rem_euclid(period)
                    }
                } else {
                    t
                })
            };
            (at(sample)?, at(sample + 1)?)
        };
        Some(Self { trim, edge, a, b })
    }

    fn value(&self, surface: &Surface, closed: (f64, f64), t: f64, seed: V3) -> Option<(V3, V3)> {
        if let Some(trim) = self.trim {
            return Some(trim.curve.value_and_derivative(t));
        }
        let p = self.edge.curve.value(t);
        let d = self.edge.curve.derivative(t);
        let (u, v) = foot(surface, closed, p, Some((seed[0], seed[1])))?;
        let (du, dv) = uv_velocity(surface, closed, u, v, p, d);
        Some(([u, v, 0.], [du, dv, 0.]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_u_sweep_keeps_the_original_pair_prefix() {
        let mut state = 317u64;
        let mut random = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            ((state >> 32) % 64) as f64 * 0.25
        };
        for count in [0, 1, 2, 31, 32, 33, 127, 256] {
            for kind in 0..6 {
                let mut spans: Vec<_> = (0..count)
                    .map(|i| {
                        let (a, b) = match kind {
                            0 => (i as f64, i as f64 + 0.5),
                            1 => (0., count as f64),
                            2 => (random(), random()),
                            3 => (0., -0.),
                            4 => (f64::NAN, random()),
                            _ => (f64::NEG_INFINITY, f64::INFINITY),
                        };
                        Span {
                            edge: i / 8,
                            sample: i % 8,
                            last: i % 8 == 7,
                            uv: [[a, random(), 0.], [b, random(), 0.]],
                        }
                    })
                    .collect();
                spans.sort_unstable_by(|a, b| a.left().total_cmp(&b.left()));
                let expected: Vec<_> = (0..spans.len())
                    .flat_map(|i| {
                        let right = spans[i].uv[0][0].max(spans[i].uv[1][0]);
                        let lo = spans[i].uv[0][1].min(spans[i].uv[1][1]).to_bits();
                        let hi = spans[i].uv[0][1].max(spans[i].uv[1][1]).to_bits();
                        let end = (i + 1..spans.len())
                            .find(|&j| spans[j].left() > right)
                            .unwrap_or(spans.len());
                        (i + 1..end).map(move |j| (i, j, lo, hi))
                    })
                    .collect();
                let sweep = TrimCrossings {
                    spans,
                    periods: [f64::INFINITY; 2],
                    closed: (f64::INFINITY, f64::INFINITY),
                    sweep_axes: true,
                };
                for budget in [0, 1, count * 4, expected.len(), expected.len() + 1] {
                    let mut actual = Vec::new();
                    let result = sweep.scan_u(budget, |i, j, lo, hi| {
                        actual.push((i, j, lo.to_bits(), hi.to_bits()));
                        false
                    });
                    assert_eq!(actual, expected[..expected.len().min(budget)]);
                    assert_eq!(result, (budget >= expected.len()).then_some(false));
                    if let Some(&(ai, bi, _, _)) = expected.get(expected.len() / 2) {
                        let stop = expected.len() / 2;
                        let mut calls = 0;
                        let result = sweep.scan_u(budget, |i, j, _, _| {
                            calls += 1;
                            (i, j) == (ai, bi)
                        });
                        assert_eq!(result, (budget > stop).then_some(true));
                        assert_eq!(calls, budget.min(stop + 1));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod grouped_tests {
    use super::*;

    fn requires_confirmation(
        spans: &[Span],
        vertex: &impl Fn(&Span, f64) -> Option<usize>,
    ) -> bool {
        for a in spans {
            for b in spans {
                if a.edge == b.edge && a.sample.abs_diff(b.sample) <= 1 {
                    continue;
                }
                let separated = (0..2).any(|axis| {
                    a.uv[0][axis].max(a.uv[1][axis]) < b.uv[0][axis].min(b.uv[1][axis])
                        || b.uv[0][axis].max(b.uv[1][axis]) < a.uv[0][axis].min(a.uv[1][axis])
                });
                if separated {
                    continue;
                }
                let da = geom::sub(a.uv[1], a.uv[0]);
                let db = geom::sub(b.uv[1], b.uv[0]);
                let offset = geom::sub(b.uv[0], a.uv[0]);
                let det = cross(da, db);
                if det == 0. {
                    continue;
                }
                let fa = cross(offset, db) / det;
                let fb = cross(offset, da) / det;
                if !(0. ..=1.).contains(&fa) || !(0. ..=1.).contains(&fb) {
                    continue;
                }
                if let Some(v) = vertex(a, fa)
                    && vertex(b, fb) == Some(v)
                {
                    continue;
                }
                return true;
            }
        }
        false
    }

    #[test]
    fn grouped_exclusion_never_hides_a_finite_curve_candidate() {
        let mut state = 317u64;
        let mut random = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            ((state >> 32) % 17) as f64 - 8.
        };
        let mut cleared = 0;
        let mut deferred = 0;
        for scale in [1e-150, 1e-10, 1., 1e10, 1e150] {
            for count in [1, 2, 3, 9, 17] {
                for kind in 0..8 {
                    let mut spans = Vec::new();
                    for edge in 0..8 {
                        let origin = if kind < 2 {
                            [edge as f64 * 8., 0., 0.]
                        } else {
                            [random(), random(), 0.]
                        };
                        let mut points = Vec::new();
                        for k in 0..=count {
                            let t = k as f64 / count as f64;
                            let p = match kind {
                                0 => [origin[0] + t, t, 0.],
                                1 => [origin[0], t, 0.],
                                2 => [origin[0] + t, origin[1] + t, 0.],
                                3 => [origin[0] + t.cos(), origin[1] + t.sin(), 0.],
                                4 => [random(), random(), 0.],
                                5 => [0., if k % 2 == 0 { 0. } else { -0. }, 0.],
                                6 => [t, (4. * std::f64::consts::TAU * t).sin(), 0.],
                                _ => [origin[0] + t, origin[1] + t * f64::EPSILON, 0.],
                            };
                            points.push(p.map(|x| x * scale));
                        }
                        for (sample, pair) in points.windows(2).enumerate() {
                            spans.push(Span {
                                edge,
                                sample,
                                last: sample + 1 == count,
                                uv: [pair[0], pair[1]],
                            });
                        }
                    }
                    let vertex = |s: &Span, f: f64| {
                        if s.sample == 0 && f <= 16. * f64::EPSILON {
                            Some(2 * s.edge)
                        } else if s.last && 1. - f <= 16. * f64::EPSILON {
                            Some(2 * s.edge + 1)
                        } else {
                            None
                        }
                    };
                    let result = grouped_clear(&spans, vertex);
                    assert!(
                        !result || !requires_confirmation(&spans, &vertex),
                        "{scale} {count} {kind}"
                    );
                    cleared += usize::from(result);
                    deferred += usize::from(!result);
                }
            }
        }
        assert!(
            cleared > 50 && deferred > 50,
            "{cleared} clear, {deferred} deferred"
        );
    }

    #[test]
    fn grouped_exclusion_respects_shared_vertices_and_nonfinite_fallback() {
        let points = [[0., 0., 0.], [2., 0., 0.], [2., 2., 0.], [0., 2., 0.]];
        let mut spans: Vec<_> = (0..4)
            .map(|edge| Span {
                edge,
                sample: 0,
                last: true,
                uv: [points[edge], points[(edge + 1) % 4]],
            })
            .collect();
        let shared = |s: &Span, f: f64| {
            if f <= 16. * f64::EPSILON {
                Some(s.edge)
            } else if 1. - f <= 16. * f64::EPSILON {
                Some((s.edge + 1) % 4)
            } else {
                None
            }
        };
        assert!(grouped_clear(&spans, shared));
        assert!(!grouped_clear(&spans, |_, _| None));
        spans.push(Span {
            edge: 4,
            sample: 0,
            last: true,
            uv: [[-1., 1., 0.], [3., 1., 0.]],
        });
        assert!(!grouped_clear(&spans, shared));
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            spans[0].uv[0][0] = value;
            assert!(!grouped_clear(&spans, shared));
        }
    }
}
