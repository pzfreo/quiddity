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
    fn right(&self) -> f64 {
        self.uv[0][0].max(self.uv[1][0])
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

/// A broad phase over the UV samples already used to build the integration domain.
/// Each span refers back to the actual finite curve for the intersection proof.
pub(crate) struct TrimCrossings {
    spans: Vec<Span>,
    periods: [f64; 2],
    closed: (f64, f64),
}

impl TrimCrossings {
    pub(crate) fn new(surface: &Surface, boundary: &DeclaredBoundary) -> Option<Self> {
        let closed = boundary.edges.first_key_value()?.1.spans;
        let (pu, pv) = surface.periodic();
        Some(Self {
            spans: Vec::new(),
            periods: [
                if pu { std::f64::consts::TAU } else { closed.0 },
                if pv { std::f64::consts::TAU } else { closed.1 },
            ],
            closed,
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

    pub(crate) fn crosses(
        mut self,
        boundary: &DeclaredBoundary,
        part: &Part,
        surface: &Surface,
        tolerance: f64,
    ) -> bool {
        self.spans
            .sort_unstable_by(|a, b| a.left().total_cmp(&b.left()));
        for (i, a) in self.spans.iter().enumerate() {
            let lo = a.uv[0][1].min(a.uv[1][1]);
            let hi = a.uv[0][1].max(a.uv[1][1]);
            for b in &self.spans[i + 1..] {
                if b.left() > a.right() {
                    break;
                }
                if a.edge == b.edge && a.sample.abs_diff(b.sample) <= 1 {
                    continue;
                }
                if b.uv[0][1].min(b.uv[1][1]) > hi || b.uv[0][1].max(b.uv[1][1]) < lo {
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
                // An intersection at the shared topological vertex closes the region.
                // Only candidate interior intersections need curve evaluation.
                if let Some(vertex) = a.vertex(part, fa)
                    && b.vertex(part, fb) == Some(vertex)
                {
                    continue;
                }
                let seed = geom::add(a.uv[0], geom::scale(da, fa));
                let spans = [a, b];
                let params = spans.map(|span| {
                    let edge = &part.edges[span.edge];
                    let trim = boundary
                        .edges
                        .get(&span.edge)
                        .filter(|t| t.samples(surface, edge).is_some());
                    FiniteCurve::from_samples(edge, trim, surface, span.sample)
                });
                if let [Some(a), Some(b)] = params
                    && self.confirms(
                        [a, b],
                        [fa, fb],
                        seed,
                        geom::norm(da).max(geom::norm(db)),
                        surface,
                        tolerance,
                    )
                {
                    return true;
                }
            }
        }
        false
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
    ) -> Option<Self> {
        let (a, b) = if let Some(trim) = trim {
            let ts = &trim.samples(surface, edge)?.ts;
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
            return Some((trim.curve.value(t), trim.curve.derivative(t)));
        }
        let p = self.edge.curve.value(t);
        let d = self.edge.curve.derivative(t);
        let (u, v) = foot(surface, closed, p, Some((seed[0], seed[1])))?;
        let (du, dv) = uv_velocity(surface, closed, u, v, p, d);
        Some(([u, v, 0.], [du, dv, 0.]))
    }
}
