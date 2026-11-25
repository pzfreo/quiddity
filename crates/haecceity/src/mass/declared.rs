//! STEP selects the master representation per edge. Each usable selection keeps its
//! own parameter interval. The 3D edge and a pcurve need not use the same parameter.
use super::*;
use crate::brep::{RawCurve2d, TrimCarrier};
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};
mod deviation;
mod intersections;
pub(crate) use deviation::{BoundaryPolicy, EdgeDeviations};
pub(crate) use intersections::TrimCrossings;
mod parameters;
use parameters::Axis;

#[derive(Clone, Debug)]
pub(crate) struct ParamCurve {
    source: Curve,
    axes: [Axis; 2],
    transfer: Option<Box<Transfer>>,
}

#[derive(Clone, Debug)]
struct Transfer {
    carrier: Arc<Surface>,
    face: Arc<Surface>,
    carrier_spans: (f64, f64),
    face_spans: (f64, f64),
    face_collapsed: (Option<f64>, Option<f64>),
    placement: crate::step::Placement,
}

impl Transfer {
    fn face_uv(&self, point: V3) -> Option<(f64, f64)> {
        let uv = foot(&self.face, self.face_spans, point, None)?;
        if self.face_collapsed.0 == Some(uv.0) || self.face_collapsed.1 == Some(uv.1) {
            // The inverse can land on a side whose tangent vanishes even for
            // an interior point. Compare the physical distances before using
            // that singular parameter for the transferred point or tangent.
            let residual = geom::dist(self.face.value(uv.0, uv.1), point);
            if residual > geom::COORD_FLOOR
                && let Some(q) = crate::uv::beside_collapsed(&self.face, point, self.face_collapsed)
                && geom::dist(self.face.value(q.0, q.1), point) < residual
            {
                return Some(q);
            }
        }
        Some(uv)
    }
}

/// The bounded evaluator clamps its inputs. A trim outside a carrier's native
/// patch has no point or tangent there. Crossing a genuinely closed seam is
/// different and is handled by wrapping before checking the domain.
fn on_carrier(surface: &Surface, spans: (f64, f64), q: V3) -> Option<(f64, f64)> {
    carrier_uv(surface, spans, q, || Some(()))
}

fn carrier_uv(
    surface: &Surface,
    spans: (f64, f64),
    q: V3,
    spend: impl Fn() -> Option<()>,
) -> Option<(f64, f64)> {
    if !q[0].is_finite() || !q[1].is_finite() {
        return None;
    }
    let (u, v) = wrap_closed(surface, spans, (q[0], q[1]));
    if let Surface::Freeform { surface, .. } = surface {
        let (u0, u1, v0, v1) = surface.domain();
        let (uc, vc) = (u.clamp(u0, u1), v.clamp(v0, v1));
        if (uc, vc) != (u, v) {
            // STEP can round a pcurve endpoint just beyond a finite knot.
            // Use the inverse's boundary precision over each parameter range
            // and bound the displacement in millimetres on this surface.
            let near = |x: f64, edge: f64, lo: f64, hi: f64| {
                let precision =
                    (1e-12 * (hi - lo)).max(64. * f64::EPSILON * lo.abs().max(hi.abs()));
                // Expand the knot before comparing rounded STEP coordinates.
                // At u = 1, the decimal 1.000000000001 rounds to 1. + 1e-12.
                // Subtracting 1 from that value first puts it outside the band.
                (edge - precision..=edge + precision).contains(&x)
            };
            if !near(u, uc, u0, u1) || !near(v, vc, v0, v1) {
                return None;
            }
            spend()?;
            let (_, su, sv) = surface.value_and_partials(uc, vc);
            let distance = (u - uc).abs() * geom::norm(su) + (v - vc).abs() * geom::norm(sv);
            if !distance.is_finite() || distance > geom::COORD_FLOOR {
                return None;
            }
        }
        return Some((uc, vc));
    }
    Some((u, v))
}

impl ParamCurve {
    fn carrier_uv(&self, t: f64) -> V3 {
        let p = self.source.value(t);
        [self.axes[0].at(p[0]).0, self.axes[1].at(p[1]).0, 0.]
    }
    fn lifted(&self, t: f64, tr: &Transfer) -> Option<((f64, f64), V3)> {
        let uv = on_carrier(&tr.carrier, tr.carrier_spans, self.carrier_uv(t))?;
        Some((uv, tr.carrier.value(uv.0, uv.1)))
    }
    pub(crate) fn value(&self, t: f64) -> V3 {
        if let Some(tr) = &self.transfer {
            self.lifted(t, tr)
                .and_then(|(_, p)| tr.face_uv(p))
                .map_or([f64::NAN; 3], |(u, v)| [u, v, 0.])
        } else {
            self.carrier_uv(t)
        }
    }
    /// Evaluate the point and tangent from the same inverse on the receiving face.
    pub(crate) fn value_and_derivative(&self, t: f64) -> (V3, V3) {
        let p = self.source.value(t);
        let d = self.source.derivative(t);
        let (u, du) = self.axes[0].at(p[0]);
        let (v, dv) = self.axes[1].at(p[1]);
        let velocity = [du * d[0], dv * d[1], 0.];
        let Some(tr) = &self.transfer else {
            return ([u, v, 0.], velocity);
        };
        let Some((u, v)) = on_carrier(&tr.carrier, tr.carrier_spans, [u, v, 0.]) else {
            return ([f64::NAN; 3], [f64::NAN; 3]);
        };
        let xyz = tr.carrier.value(u, v);
        let (su, sv) = tr.carrier.partials(u, v);
        let tangent = geom::add(geom::scale(su, velocity[0]), geom::scale(sv, velocity[1]));
        let Some((u, v)) = tr.face_uv(xyz) else {
            return ([f64::NAN; 3], [f64::NAN; 3]);
        };
        let (du, dv) = uv_velocity(&tr.face, tr.face_spans, u, v, xyz, tangent);
        ([u, v, 0.], [du, dv, 0.])
    }
    fn parameter(&self, q: V3) -> f64 {
        let q = if let Some(tr) = &self.transfer {
            let (u, v) = wrap_closed(&tr.face, tr.face_spans, (q[0], q[1]));
            let p = tr.face.value(u, v);
            let Some((u, v)) = foot(&tr.carrier, tr.carrier_spans, p, None) else {
                return f64::NAN;
            };
            [u, v, 0.]
        } else {
            q
        };
        self.source
            .parameter([self.axes[0].inverse(q[0]), self.axes[1].inverse(q[1]), 0.])
    }
}

#[derive(Debug)]
pub(crate) struct EdgeTrim {
    pub(crate) curve: ParamCurve,
    spans: (f64, f64),
    pub(crate) interval: (f64, f64),
    samples: OnceLock<Option<TrimSamples>>,
    mesh_samples: OnceLock<Option<TrimSamples>>,
    nodes: OnceLock<Option<Vec<f64>>>,
    length: OnceLock<Option<f64>>,
}

#[derive(Debug)]
pub(crate) struct TrimSamples {
    pub(crate) ts: Vec<f64>,
    pub(crate) points: Vec<V3>,
}

#[derive(Debug, Default)]
pub(crate) struct DeclaredBoundary {
    pub(crate) edges: BTreeMap<usize, EdgeTrim>,
    pub(crate) loops: OnceLock<Option<Vec<UvLoop>>>,
    pub(crate) domain: OnceLock<Option<crate::uv::FaceDomain>>,
}

impl EdgeTrim {
    pub(super) fn length(&self, surface: &Surface) -> Option<f64> {
        *self.length.get_or_init(|| {
            let nodes = self.nodes()?;
            let mut panel = |(), a, b, _| {
                let rule = kronrod((), a, b, |(), t| {
                    let (p, d) = self.checked_uv_and_derivative(surface, t)?;
                    let (u, v) = wrap_closed(surface, self.spans, (p[0], p[1]));
                    let (su, sv) = surface.partials(u, v);
                    let speed = geom::norm(geom::add(geom::scale(su, d[0]), geom::scale(sv, d[1])));
                    speed.is_finite().then_some([speed])
                })?;
                Some((rule, None))
            };
            let ([length], excess, _, ()) =
                adaptive(&nodes, (), Some(geom::COORD_FLOOR), true, &mut panel)?;
            (excess <= geom::COORD_FLOOR).then_some(length.abs())
        })
    }

    pub(crate) fn point(&self, s: &Surface, t: f64) -> V3 {
        if let Some(tr) = &self.curve.transfer {
            return self.curve.lifted(t, tr).map_or([f64::NAN; 3], |(_, p)| {
                if tr.placement == crate::step::IDENTITY {
                    p
                } else {
                    tr.placement
                        .map(|r| r[0] * p[0] + r[1] * p[1] + r[2] * p[2] + r[3])
                }
            });
        }
        let (u, v) = self.surface_uv(s, t);
        s.value(u, v)
    }

    pub(crate) fn samples(&self, s: &Surface, edge: &crate::brep::Edge) -> Option<&TrimSamples> {
        self.samples
            .get_or_init(|| self.sample(s, edge, &self.nodes()?))
            .as_ref()
    }

    /// Uniform parameter stations keep opposite sides of a narrow freeform strip
    /// aligned. On a mixed edge, use them when the selected curve comes from the
    /// analytic carrier. A freeform trim transferred to an analytic face can
    /// produce inward slivers there if its boundary stations change.
    pub(crate) fn mesh_samples(
        &self,
        part: &Part,
        face: usize,
        edge: usize,
    ) -> Option<&TrimSamples> {
        let surface = &part.faces[face].surface;
        let carrier = self
            .curve
            .transfer
            .as_ref()
            .map_or(surface, |tr| &tr.carrier);
        let incident = &part.edge_faces()[edge];
        let freeform = |&i: &usize| matches!(part.faces[i].surface, Surface::Freeform { .. });
        let uniform = incident.iter().any(freeform)
            && (!matches!(carrier, Surface::Freeform { .. }) || incident.iter().all(freeform));
        let edge = &part.edges[edge];
        if !uniform {
            return self.samples(surface, edge);
        }
        self.mesh_samples
            .get_or_init(|| {
                let (a, b) = self.interval;
                let n = 256;
                let ts: Vec<_> = (0..=n).map(|i| a + (b - a) * i as f64 / n as f64).collect();
                self.sample(surface, edge, &ts)
            })
            .as_ref()
    }

    fn sample(&self, s: &Surface, edge: &crate::brep::Edge, ts: &[f64]) -> Option<TrimSamples> {
        let eval = |t| self.point(s, t);
        let mut samples = TrimSamples {
            ts: vec![*ts.first()?],
            points: vec![edge.start],
        };
        for w in ts.windows(2) {
            let mut stack = vec![(w[0], eval(w[0]), w[1], eval(w[1]), 0)];
            while let Some((a, p, b, q, depth)) = stack.pop() {
                if samples.points.len() >= MAX_PANELS {
                    return None;
                }
                let t = 0.5 * (a + b);
                let m = eval(t);
                if depth < 14
                    && geom::dist(m, geom::scale(geom::add(p, q), 0.5))
                        > crate::sampling::CHORD_TOLERANCE
                {
                    stack.push((t, m, b, q, depth + 1));
                    stack.push((a, p, t, m, depth + 1));
                } else {
                    samples.ts.push(b);
                    samples.points.push(q);
                }
            }
        }
        *samples.points.last_mut()? = edge.end;
        Some(samples)
    }
    pub(crate) fn surface_uv(&self, s: &Surface, t: f64) -> (f64, f64) {
        let p = self.curve.value(t);
        wrap_closed(s, self.spans, (p[0], p[1]))
    }
    pub(super) fn checked_uv_and_derivative(&self, s: &Surface, t: f64) -> Option<(V3, V3)> {
        let (p, d) = self.curve.value_and_derivative(t);
        on_carrier(s, self.spans, p)?;
        Some((p, d))
    }
    pub(crate) fn nodes(&self) -> Option<Vec<f64>> {
        self.nodes.get_or_init(|| self.compute_nodes()).clone()
    }

    fn compute_nodes(&self) -> Option<Vec<f64>> {
        let (a, b) = self.interval;
        let breaks = match &self.curve.source {
            Curve::Nurbs(c) => knots_on_interval(c.breaks(), self.curve.source.period(), a, b)?,
            _ => Vec::new(),
        };
        let mut ts = cuts(a, b, 16, &breaks)?;
        // A foreign spline's partials can jump at a knot even when this
        // pcurve and the receiving face are smooth. Split the path there
        // before budgeting the higher-moment integrals on the receiving face.
        if let Some(tr) = &self.curve.transfer
            && let Surface::Freeform { surface, .. } = &*tr.carrier
        {
            let spans = [tr.carrier_spans.0, tr.carrier_spans.1];
            let knots = [
                crate::nurbs::breaks(&surface.knots_u, surface.degree_u),
                crate::nurbs::breaks(&surface.knots_v, surface.degree_v),
            ];
            let grid = ts.clone();
            for w in grid.windows(2) {
                let from = self.curve.carrier_uv(w[0]);
                let to = self.curve.carrier_uv(w[1]);
                for axis in 0..2 {
                    let (lo, hi) = (from[axis].min(to[axis]), from[axis].max(to[axis]));
                    if hi <= lo {
                        continue;
                    }
                    for &knot in &knots[axis] {
                        let span = spans[axis];
                        let mut crossing = if span.is_finite() {
                            knot + ((lo - knot) / span).ceil() * span
                        } else {
                            knot
                        };
                        while crossing < hi {
                            if crossing > lo {
                                let (mut left, mut right) = (w[0], w[1]);
                                for _ in 0..52 {
                                    let mid = 0.5 * (left + right);
                                    if (self.curve.carrier_uv(mid)[axis] < crossing)
                                        == (from[axis] < crossing)
                                    {
                                        left = mid;
                                    } else {
                                        right = mid;
                                    }
                                }
                                ts.push(0.5 * (left + right));
                                if ts.len() >= MAX_PANELS {
                                    return None;
                                }
                            }
                            if !span.is_finite() {
                                break;
                            }
                            let next = crossing + span;
                            if next <= crossing {
                                return None;
                            }
                            crossing = next;
                        }
                    }
                }
            }
            ts.sort_by(|x, y| {
                if a <= b {
                    x.total_cmp(y)
                } else {
                    y.total_cmp(x)
                }
            });
            ts.dedup();
        }
        Some(ts)
    }
}

impl Part {
    /// Keep the resolved master intervals and their source-frame UV transfer.
    /// Samples in caller coordinates and mixed loops are rebuilt on demand.
    pub(crate) fn carry_declared_boundary(
        &self,
        face: usize,
        to: &crate::brep::FaceCache,
        placement: &crate::step::Placement,
    ) {
        if let Some(boundary) = self.cache[face].declared.get() {
            let _ = to.declared.set(boundary.as_ref().map(|boundary| {
                let edges = boundary
                    .edges
                    .iter()
                    .map(|(&id, trim)| {
                        let mut curve = trim.curve.clone();
                        if let Some(transfer) = &mut curve.transfer {
                            transfer.placement =
                                crate::step::compose(placement, &transfer.placement);
                        }
                        (
                            id,
                            EdgeTrim {
                                curve,
                                spans: trim.spans,
                                interval: trim.interval,
                                samples: OnceLock::new(),
                                mesh_samples: OnceLock::new(),
                                nodes: trim.nodes.clone(),
                                length: trim.length.clone(),
                            },
                        )
                    })
                    .collect();
                Box::new(DeclaredBoundary {
                    edges,
                    ..DeclaredBoundary::default()
                })
            }));
        }
    }

    /// A local scale for the integration error budget, independent of the selected trim.
    pub(super) fn trim_reference_length(&self, face: usize) -> f64 {
        let mut bounds = geom::Bounds::empty();
        for lp in &self.faces[face].loops {
            for &(e, _) in &lp.edges {
                for &p in &self.edges[e].samples {
                    bounds.add(p);
                }
            }
            if let Some(p) = lp.vertex {
                bounds.add(p);
            }
        }
        bounds.diagonal()
    }

    pub(crate) fn declared_boundary(&self, face: usize) -> Option<&DeclaredBoundary> {
        self.cache[face]
            .declared
            .get_or_init(|| self.read_declared_boundary(face).map(Box::new))
            .as_deref()
    }

    fn read_declared_boundary(&self, face: usize) -> Option<DeclaredBoundary> {
        match self.boundary_policy {
            BoundaryPolicy::Master => {}
            BoundaryPolicy::Projected => return None,
            BoundaryPolicy::Recognition if !self.declares_disagreeing_edge(face)? => return None,
            BoundaryPolicy::Recognition => {}
        }
        let coedges = || self.faces[face].loops.iter().flat_map(|lp| &lp.edges);
        if self.measure_trims.is_empty()
            || !coedges().any(|(e, _)| self.measure_trims.contains_key(e))
        {
            return None;
        }
        let surface = &self.faces[face].surface;
        let units = self.file_units()?;
        let make_curve = |raw: &RawCurve2d, carrier: &Surface, owner: Option<usize>| {
            let (scales, axes) = match carrier.file_parameter_scales(units) {
                Some(scales) => (scales, [Axis::ID; 2]),
                None => ((1., 1.), parameters::swept_axes(self, owner?)?),
            };
            Some(ParamCurve {
                source: scaled_pcurve(raw, scales.0, scales.1)?,
                axes,
                transfer: None,
            })
        };
        let tolerance = self.file_uncertainty().unwrap_or(0.).max(0.) + geom::COORD_FLOOR;
        let mut result = DeclaredBoundary::default();
        let shared = |f: usize| {
            self.cache[f]
                .carrier
                .get_or_init(|| Arc::new(self.faces[f].surface.clone()))
                .clone()
        };
        let collapsed = std::cell::OnceCell::new();
        for &(e, _) in self.faces[face].loops.iter().flat_map(|lp| &lp.edges) {
            if result.edges.contains_key(&e) {
                continue;
            }
            let Some(selection) = self.measure_trims.get(&e) else {
                continue;
            };
            let (carrier, raw) = selection.as_ref()?;
            let curve = match carrier {
                TrimCarrier::Faces(owners) if owners.contains(&face) => {
                    make_curve(raw, surface, Some(face))?
                }
                _ => {
                    let (carrier, owner) = match carrier {
                        TrimCarrier::Faces(owners) => {
                            (shared(*owners.first()?), owners.first().copied())
                        }
                        TrimCarrier::Detached(s) => {
                            (Arc::new(s.placed(&self.edge_placement(e))), None)
                        }
                    };
                    let mut curve = make_curve(raw, &carrier, owner)?;
                    curve.transfer = Some(Box::new(Transfer {
                        carrier_spans: closed_spans(&carrier),
                        face_spans: closed_spans(surface),
                        face_collapsed: *collapsed.get_or_init(|| collapsed_sides(surface)),
                        carrier,
                        face: shared(face),
                        placement: crate::step::IDENTITY,
                    }));
                    curve
                }
            };
            result
                .edges
                .insert(e, resolve(surface, &self.edges[e], curve, tolerance)?);
        }
        self.loops_on(face, Some(&result))?;
        Some(result)
    }
}

fn scaled_pcurve(raw: &RawCurve2d, su: f64, sv: f64) -> Option<Curve> {
    // A plane scales both axes equally. Preserve the circle or ellipse and
    // its angular parameter so its integration interval remains angular.
    if let RawCurve2d::Conic {
        origin,
        reference,
        major,
        minor,
    } = raw
        && su == sv
        && su.is_finite()
        && su > 0.
    {
        let [x, y] = reference.unwrap_or([1., 0.]);
        let x = geom::unit([x, y, 0.])?;
        let z = [0., 0., 1.];
        let frame = geom::Frame {
            origin: [origin[0] * su, origin[1] * sv, 0.],
            x,
            y: geom::cross(z, x),
            z,
        };
        let a = major * su;
        let b = minor.unwrap_or(*major) * sv;
        if !(a.is_finite() && b.is_finite() && a > 0. && b > 0.) {
            return None;
        }
        return Some(if a == b {
            Curve::Circle { frame, radius: a }
        } else {
            Curve::Ellipse {
                frame,
                major: a,
                minor: b,
            }
        });
    }
    preferred::raw_param_curve(&raw.scaled(su, sv)?)
}

fn resolve(
    surface: &Surface,
    edge: &crate::brep::Edge,
    curve: ParamCurve,
    tolerance: f64,
) -> Option<EdgeTrim> {
    let spatial = edge_interval(
        &edge.curve,
        edge.start,
        edge.end,
        edge.same_sense,
        edge.is_closed(),
    );
    let spans = closed_spans(surface);
    let period = curve.source.period();
    let interval = resolve_interval(surface, edge, &curve, spatial, tolerance, || Some(()))?;
    let trim = EdgeTrim {
        curve,
        spans,
        interval,
        samples: OnceLock::new(),
        mesh_samples: OnceLock::new(),
        nodes: OnceLock::new(),
        length: OnceLock::new(),
    };
    let (a, b) = interval;
    // Check the selected curve on the finite parameter range it actually uses.
    // A foreign pcurve has to describe points on this face within STEP's
    // uncertainty. The alternate 3D curve does not define this region.
    let knots = match &trim.curve.source {
        Curve::Nurbs(c) => knots_on_interval(c.breaks(), period, a, b)?,
        _ => Vec::new(),
    };
    for span in cuts(a, b, 1, &knots)?.windows(2) {
        for j in 0..=32 {
            let t = span[0] + (span[1] - span[0]) * (j as f64 / 32.);
            let (u, v) = on_carrier(surface, spans, trim.curve.value(t))?;
            let p = surface.value(u, v);
            if !p.iter().all(|x| x.is_finite()) {
                return None;
            }
            if let Some(tr) = &trim.curve.transfer {
                let (_, xyz) = trim.curve.lifted(t, tr)?;
                let residual = geom::dist(p, xyz);
                if !residual.is_finite() || residual > tolerance {
                    return None;
                }
            }
        }
    }
    Some(trim)
}

fn resolve_interval(
    surface: &Surface,
    edge: &crate::brep::Edge,
    curve: &ParamCurve,
    spatial: (f64, f64),
    tolerance: f64,
    spend: impl Fn() -> Option<()>,
) -> Option<(f64, f64)> {
    let spans = closed_spans(surface);
    let lift = |t| {
        let q = curve.value(t);
        let (u, v) = wrap_closed(surface, spans, (q[0], q[1]));
        spend()?;
        Some(surface.value(u, v))
    };
    let natural = match &curve.source {
        Curve::Nurbs(c) => Some(c.domain()),
        _ => None,
    };
    let period = curve.source.period();
    let valid = |(a, b): (f64, f64)| {
        if !a.is_finite() || !b.is_finite() || a == b {
            return false;
        }
        let in_domain = natural.is_none_or(|(lo, hi)| {
            let eps = 64. * f64::EPSILON * lo.abs().max(hi.abs()).max(hi - lo);
            if period.is_some() {
                (b - a).abs() <= hi - lo + eps
            } else {
                a.min(b) >= lo - eps && a.max(b) <= hi + eps
            }
        });
        // A bounded NURBS evaluator clamps outside parameters to its endpoint.
        // Such a match must not suppress inversion of the pcurve's own endpoints.
        let on_surface = |t| carrier_uv(surface, spans, curve.value(t), &spend).is_some();
        in_domain
            && on_surface(a)
            && on_surface(b)
            && lift(a).is_some_and(|p| geom::dist(p, edge.start) <= tolerance)
            && lift(b).is_some_and(|p| geom::dist(p, edge.end) <= tolerance)
    };
    let (pu, pv) = surface.periodic();
    let periods = [
        if pu { std::f64::consts::TAU } else { spans.0 },
        if pv { std::f64::consts::TAU } else { spans.1 },
    ];
    let guess = |p, near: V3| {
        let (u, v) = surface.parameters(p, Some((near[0], near[1])))?;
        let mut q = [u, v, 0.];
        for k in 0..2 {
            if periods[k].is_finite() {
                q[k] += periods[k] * ((near[k] - q[k]) / periods[k]).round();
            }
        }
        Some(curve.parameter(q))
    };
    let guessed = || {
        let a = guess(edge.start, curve.value(natural.map_or(0., |ab| ab.0)))?;
        let mut b = guess(edge.end, curve.value(a))?;
        if let Some(period) = period {
            if edge.is_closed() {
                b = a + if edge.same_sense { period } else { -period };
            } else if (b > a) != edge.same_sense {
                b += if edge.same_sense { period } else { -period };
            }
        }
        Some((a, b))
    };
    [
        Some(spatial),
        natural.filter(|_| edge.same_sense),
        natural.map(|(a, b)| (b, a)).filter(|_| !edge.same_sense),
    ]
    .into_iter()
    .flatten()
    .find(|&ab| valid(ab))
    .or_else(|| guessed().filter(|&ab| valid(ab)))
}

#[cfg(test)]
mod tests;
