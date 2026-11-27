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
    source: Arc<Curve>,
    axes: [Axis; 2],
    transfer: Option<Box<Transfer>>,
}

/// A selected pcurve in one pair of parameter units. Incident faces can
/// share its immutable geometry and lazily built inversion table.
#[derive(Debug)]
pub(crate) struct SelectedCurve {
    scales: [u64; 2],
    source: Option<Arc<Curve>>,
}

/// Share only identical immutable NURBS operands within one part. The key
/// includes every stored scalar bit and the parameter units; topological
/// identities, intervals, carriers, transfers and samples remain per edge.
#[derive(Debug, Default)]
pub(crate) struct SelectedCurvePool(
    std::sync::Mutex<std::collections::HashMap<Vec<u64>, Arc<Curve>>>,
);

impl SelectedCurvePool {
    fn intern(&self, source: Arc<Curve>, scales: [u64; 2]) -> Arc<Curve> {
        let Curve::Nurbs(curve) = &*source else {
            return source;
        };
        let n = curve.control_points().len();
        let mut key = Vec::with_capacity(6 + 5 * n + curve.degree());
        key.extend(scales);
        key.extend([
            curve.degree() as u64,
            n as u64,
            u64::from(curve.is_closed()),
        ]);
        key.extend(curve.control_points().iter().flatten().map(|x| x.to_bits()));
        key.extend(curve.weights().iter().map(|x| x.to_bits()));
        key.extend(curve.knots().iter().map(|x| x.to_bits()));
        let Ok(mut curves) = self.0.lock() else {
            return source;
        };
        curves.entry(key).or_insert(source).clone()
    }
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
    fn face_uv_near(&self, point: V3, hint: Option<(f64, f64)>) -> Option<(f64, f64)> {
        self.face_uv_with(point, hint, None)
    }

    fn face_uv_with(
        &self,
        point: V3,
        hint: Option<(f64, f64)>,
        inverse: Option<&super::SurfaceInversion<'_>>,
    ) -> Option<(f64, f64)> {
        let uv = match inverse {
            Some(inverse) => inverse.foot(self.face_spans, point, hint),
            None => foot(&self.face, self.face_spans, point, hint),
        }?;
        if self.face_collapsed.0 == Some(uv.0) || self.face_collapsed.1 == Some(uv.1) {
            // The inverse can land on a side whose tangent vanishes even for
            // an interior point. Compare the physical distances before using
            // that singular parameter for the transferred point or tangent.
            let residual = geom::dist(self.face.value(uv.0, uv.1), point);
            if residual > geom::COORD_FLOOR
                && let Some(q) = match inverse {
                    Some(inverse) => inverse.beside(point, self.face_collapsed),
                    None => crate::uv::beside_collapsed(&self.face, point, self.face_collapsed),
                }
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
        self.value_near(t, None)
    }

    fn value_near(&self, t: f64, hint: Option<(f64, f64)>) -> V3 {
        if let Some(tr) = &self.transfer {
            self.lifted(t, tr)
                .and_then(|(_, p)| tr.face_uv_near(p, hint))
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
        let Some((u, v)) = tr.face_uv_near(xyz, None) else {
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
    mesh: bool,
    uniform_mesh_stations: bool,
    witnesses: Vec<ParamCurve>,
    samples: OnceLock<Option<TrimSamples>>,
    nodes: OnceLock<Option<Vec<f64>>>,
    length: OnceLock<Option<f64>>,
}

#[derive(Clone, Debug)]
pub(crate) struct TrimSamples {
    pub(crate) ts: Vec<f64>,
    pub(crate) points: Vec<V3>,
    pub(crate) native_uv: Option<Vec<(f64, f64)>>,
}

#[derive(Debug, Default)]
pub(crate) struct DeclaredBoundary {
    pub(crate) edges: BTreeMap<usize, EdgeTrim>,
    pub(crate) loops: OnceLock<Option<Vec<UvLoop>>>,
    pub(crate) domain: OnceLock<Option<crate::uv::FaceDomain>>,
}

impl EdgeTrim {
    /// A direct line which traverses one full u period at constant v.
    pub(super) fn full_u_latitude(&self, near: f64) -> Option<f64> {
        if self.curve.transfer.is_some()
            || !matches!(self.curve.source.as_ref(), Curve::Line { .. })
        {
            return None;
        }
        let (a, b) = self.interval;
        let [u0, v0, _] = self.curve.value(a);
        let [u1, v1, _] = self.curve.value(b);
        ((v1 - v0).abs() <= 128.0 * f64::EPSILON * v0.abs().max(1.0)
            && ((u1 - u0).abs() - std::f64::consts::TAU).abs() <= 1e-9)
            .then(|| geom::nearest_turn(v0, near))
    }

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
                adaptive(nodes, (), Some(geom::COORD_FLOOR), true, &mut panel)?;
            (excess <= geom::COORD_FLOOR).then_some(length.abs())
        })
    }

    /// A bound on the lifted curve's second derivative and on evaluation
    /// rounding over this interval. The interpolation error at a midpoint is
    /// at most M * (b - a)^2 / 8. Only analytic carriers and affine parameter
    /// axes are covered; unresolved or poorly scaled inputs keep the checks.
    fn midpoint_bound(
        curve: &ParamCurve,
        surface: &Surface,
        interval: (f64, f64),
    ) -> Option<(f64, f64)> {
        let bounded = |x: f64| x.is_finite() && x.abs() < 1e50;
        let frame_ok = |f: &geom::Frame| {
            f.origin
                .iter()
                .chain(&f.x)
                .chain(&f.y)
                .chain(&f.z)
                .all(|&x| bounded(x))
        };
        if !bounded(interval.0) || !bounded(interval.1) {
            return None;
        }
        let t = interval.0.abs().max(interval.1.abs());
        let (position, first, second) = match &*curve.source {
            Curve::Line { origin, dir } if origin.iter().chain(dir).all(|&x| bounded(x)) => (
                [0, 1].map(|k| origin[k].abs() + dir[k].abs() * t),
                [dir[0].abs(), dir[1].abs()],
                [0.; 2],
            ),
            Curve::Circle { frame, radius } if frame_ok(frame) && bounded(*radius) => {
                let extent = [0, 1].map(|k| (frame.x[k].abs() + frame.y[k].abs()) * radius.abs());
                (
                    [0, 1].map(|k| frame.origin[k].abs() + extent[k]),
                    extent,
                    extent,
                )
            }
            Curve::Ellipse {
                frame,
                major,
                minor,
            } if frame_ok(frame) && bounded(*major) && bounded(*minor) => {
                let extent =
                    [0, 1].map(|k| frame.x[k].abs() * major.abs() + frame.y[k].abs() * minor.abs());
                (
                    [0, 1].map(|k| frame.origin[k].abs() + extent[k]),
                    extent,
                    extent,
                )
            }
            _ => return None,
        };
        let (mut uv, mut d, mut dd) = ([0.; 2], [0.; 2], [0.; 2]);
        let mut scales = [0.; 2];
        for k in 0..2 {
            let Axis::Linear { scale, offset } = curve.axes[k] else {
                return None;
            };
            if !bounded(scale) || !bounded(offset) {
                return None;
            }
            scales[k] = scale;
            uv[k] = position[k] * scale.abs() + offset.abs();
            d[k] = first[k] * scale.abs();
            dd[k] = second[k] * scale.abs();
        }
        let carrier = curve.transfer.as_ref().map_or(surface, |tr| &tr.carrier);
        let (frame, local, partials) = match carrier {
            Surface::Plane { frame } => (frame, [uv[0], uv[1], 0.], [1., 1., 0., 0., 0.]),
            Surface::Cylinder { frame, radius } if bounded(*radius) => {
                let r = radius.abs();
                (frame, [r, r, uv[1]], [r, 1., r, 0., 0.])
            }
            Surface::Cone {
                frame,
                radius,
                semi_angle,
            } if bounded(*radius) && bounded(*semi_angle) => {
                let r = radius.abs() + uv[1];
                (frame, [r, r, uv[1]], [r, 1., r, 1., 0.])
            }
            Surface::Sphere { frame, radius } if bounded(*radius) => {
                let r = radius.abs();
                (frame, [r; 3], [r; 5])
            }
            Surface::Torus {
                frame,
                major,
                minor,
            } if bounded(*major) && bounded(*minor) => {
                let r = major.abs() + minor.abs();
                let m = minor.abs();
                (frame, [r, r, m], [r, m, r, m, m])
            }
            _ => return None,
        };
        if !frame_ok(frame) || !uv.iter().chain(&d).chain(&dd).all(|&x| bounded(x)) {
            return None;
        }
        // The sum of all frame-axis magnitudes bounds every local partial,
        // including distorted frames. Absolute intermediate sums retain large
        // placement and cancellation costs in the rounding allowance.
        let axes: f64 = frame
            .x
            .iter()
            .chain(&frame.y)
            .chain(&frame.z)
            .map(|x| x.abs())
            .sum();
        let world: f64 = (0..3)
            .map(|k| {
                frame.origin[k].abs()
                    + frame.x[k].abs() * local[0]
                    + frame.y[k].abs() * local[1]
                    + frame.z[k].abs() * local[2]
            })
            .sum();
        let [fu, fv, fuu, fuv, fvv] = partials.map(|p| p * axes);
        let speed = fu * d[0] + fv * d[1];
        let mut curvature = 2.
            * (fuu * d[0] * d[0]
                + 2. * fuv * d[0] * d[1]
                + fvv * d[1] * d[1]
                + fu * dd[0]
                + fv * dd[1]);
        let rounding =
            4096. * f64::EPSILON * (1. + world + fu * uv[0] + fv * uv[1] + speed * (1. + t));
        if !bounded(world) || !bounded(curvature) || !bounded(rounding) {
            return None;
        }
        // For a harmonic path, the largest eigenvalue of the two-vector
        // Gram matrix bounds |A cos(t) + B sin(t)|^2. Its largest absolute
        // row sum avoids an eigensolve and is exact for an orthogonal circle.
        let harmonic = |a, b| (geom::dot(a, a).max(geom::dot(b, b)) + geom::dot(a, b).abs()).sqrt();
        let tight = match (carrier, &*curve.source) {
            (Surface::Cylinder { frame, radius }, Curve::Line { .. }) => {
                Some(radius.abs() * d[0] * d[0] * harmonic(frame.x, frame.y))
            }
            (Surface::Plane { frame }, source) => {
                let conic = match source {
                    Curve::Circle { frame, radius } => Some((frame, *radius, *radius)),
                    Curve::Ellipse {
                        frame,
                        major,
                        minor,
                    } => Some((frame, *major, *minor)),
                    _ => None,
                };
                conic.map(|(source, major, minor)| {
                    let lift = |p: V3, radius| {
                        geom::add(
                            geom::scale(frame.x, p[0] * radius * scales[0]),
                            geom::scale(frame.y, p[1] * radius * scales[1]),
                        )
                    };
                    harmonic(lift(source.x, major), lift(source.y, minor))
                })
            }
            _ => None,
        };
        if let Some(tight) = tight.filter(|&m| bounded(m)) {
            curvature = curvature.min(tight + 4096. * f64::EPSILON * curvature);
        }
        Some((curvature, rounding))
    }

    /// Whether the selected spatial curve is affine in its own parameter.
    /// A straight pcurve on a curved carrier is not generally a spatial line.
    fn affine_spatial_size(
        curve: &ParamCurve,
        surface: &Surface,
        interval: (f64, f64),
    ) -> Option<f64> {
        let Curve::Line { origin, dir } = &*curve.source else {
            return None;
        };
        if !origin.iter().chain(dir).all(|x| x.is_finite())
            || !curve.axes.iter().all(|axis| {
                matches!(axis,
                Axis::Linear { scale, offset } if scale.is_finite() && offset.is_finite())
            })
        {
            return None;
        }
        let carrier = curve.transfer.as_ref().map_or(surface, |tr| &tr.carrier);
        let t = interval.0.abs().max(interval.1.abs());
        let mut uv = [0.; 2];
        for k in 0..2 {
            let Axis::Linear { scale, offset } = curve.axes[k] else {
                return None;
            };
            uv[k] = (origin[k].abs() + dir[k].abs() * t) * scale.abs() + offset.abs();
        }
        let (frame, local) = match carrier {
            Surface::Plane { frame } => (frame, [uv[0], uv[1], 0.]),
            Surface::Cylinder { frame, radius } if dir[0] == 0. => {
                (frame, [radius.abs(), radius.abs(), uv[1]])
            }
            Surface::Cone {
                frame,
                radius,
                semi_angle,
            } if dir[0] == 0. && semi_angle.is_finite() => {
                let rho = radius.abs() + uv[1];
                (frame, [rho, rho, uv[1]])
            }
            _ => return None,
        };
        // Bound intermediate sums as well as final coordinates: cancellation
        // at a large placement must not hide a large evaluation error.
        let bounds = std::array::from_fn::<_, 3, _>(|k| {
            frame.origin[k].abs()
                + frame.x[k].abs() * local[0]
                + frame.y[k].abs() * local[1]
                + frame.z[k].abs() * local[2]
        });
        (bounds.iter().chain(&uv).all(|x| x.is_finite()))
            .then(|| bounds.into_iter().fold(1_f64, f64::max))
    }

    pub(crate) fn is_transferred(&self) -> bool {
        self.curve.transfer.is_some()
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
            .get_or_init(|| self.sample(s, edge, crate::sampling::CHORD_TOLERANCE))
            .as_ref()
    }

    pub(crate) fn mesh_samples(
        &self,
        s: &Surface,
        edge: &crate::brep::Edge,
        deflection: f64,
    ) -> Option<std::borrow::Cow<'_, TrimSamples>> {
        use std::borrow::Cow;
        let tolerance = (0.5 * deflection).min(crate::sampling::CHORD_TOLERANCE);
        if tolerance < crate::sampling::CHORD_TOLERANCE {
            self.sample(s, edge, tolerance).map(Cow::Owned)
        } else {
            self.samples(s, edge).map(Cow::Borrowed)
        }
    }

    fn sample(&self, s: &Surface, edge: &crate::brep::Edge, tolerance: f64) -> Option<TrimSamples> {
        if self.mesh
            && !self.uniform_mesh_stations
            && let Some(size) = Self::affine_spatial_size(&self.curve, s, self.interval)
        {
            let (a, b) = self.interval;
            let ends = [self.point(s, a), self.point(s, b)];
            // The exact lifted line stays in the convex hull of its ends.
            // A small offset at the shared vertices can dominate a thin
            // face's area. Keep interior samples when pinning moves an end.
            let margin = 0.25 * tolerance;
            if 256. * f64::EPSILON * size < margin && ends == [edge.start, edge.end] {
                return Some(TrimSamples {
                    ts: vec![a, b],
                    points: vec![edge.start, edge.end],
                    native_uv: (!self.is_transferred())
                        .then(|| vec![self.surface_uv(s, a), self.surface_uv(s, b)]),
                });
            }
        }
        let native_nodes;
        let nodes = if self.mesh && !self.is_transferred() && matches!(s, Surface::Freeform { .. })
        {
            native_nodes = self.compute_nodes(Some((s, self.spans)))?;
            native_nodes.as_slice()
        } else {
            self.nodes()?
        };
        let stations;
        let (ts, breaks): (&[f64], &[f64]) = if self.uniform_mesh_stations {
            let (a, b) = self.interval;
            stations = cuts(a, b, 256, &[])?;
            (&stations, nodes)
        } else {
            (nodes, &[])
        };
        let native = self.mesh && !self.is_transferred();
        let surface_value = super::SurfaceValue::new(s, self.mesh);
        let eval = |t| {
            if native {
                let uv = self.surface_uv(s, t);
                (surface_value.value(uv.0, uv.1), uv)
            } else {
                (self.point(s, t), (f64::NAN, f64::NAN))
            }
        };
        let mut samples = TrimSamples {
            ts: Vec::with_capacity(ts.len()),
            points: Vec::with_capacity(ts.len()),
            native_uv: native.then(|| Vec::with_capacity(ts.len())),
        };
        samples.ts.push(*ts.first()?);
        samples.points.push(edge.start);
        if let Some(uvs) = &mut samples.native_uv {
            uvs.push(self.surface_uv(s, ts[0]));
        }
        let mut stack = Vec::with_capacity(1);
        let mut previous = eval(ts[0]);
        let bound = (self.mesh && breaks.is_empty())
            .then(|| Self::midpoint_bound(&self.curve, s, self.interval))
            .flatten();
        for w in ts.windows(2) {
            // Only midpoint splits halve the parameter width. When a knot
            // witness can move the split, check the actual child spans.
            let mut depth_limit = 14;
            if let Some((curvature, rounding)) = bound
                && rounding < 0.99 * tolerance
            {
                depth_limit = 0;
                let mut width = (w[1] - w[0]).abs();
                let padding = 16. * f64::EPSILON * (w[0].abs() + w[1].abs());
                while depth_limit < 14 {
                    let padded = width + padding;
                    if curvature * padded * padded / 8. + rounding < 0.99 * tolerance {
                        break;
                    }
                    depth_limit += 1;
                    width *= 0.5;
                }
            }
            let next = eval(w[1]);
            stack.push((w[0], previous, w[1], next, 0));
            previous = next;
            while let Some((a, p, b, q, depth)) = stack.pop() {
                if samples.points.len() >= MAX_PANELS {
                    return None;
                }
                let t = 0.5 * (a + b);
                let split = if depth < depth_limit {
                    let m = eval(t);
                    if geom::dist(m.0, geom::scale(geom::add(p.0, q.0), 0.5)) > tolerance {
                        Some((t, m))
                    } else {
                        // A spline excursion can lie between a station and its midpoint.
                        knot_witness(breaks, a, b, |u| {
                            let v = eval(u);
                            (crate::mesh::segment_distance(v.0, p.0, q.0) > tolerance)
                                .then_some((u, v))
                        })
                    }
                } else {
                    None
                };
                if let Some((t, m)) = split {
                    stack.push((t, m, b, q, depth + 1));
                    stack.push((a, p, t, m, depth + 1));
                } else {
                    samples.ts.push(b);
                    samples.points.push(q.0);
                    if let Some(uvs) = &mut samples.native_uv {
                        uvs.push(q.1);
                    }
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
    pub(crate) fn nodes(&self) -> Option<&[f64]> {
        self.nodes
            .get_or_init(|| {
                self.compute_nodes(
                    self.curve
                        .transfer
                        .as_ref()
                        .map(|tr| (&*tr.carrier, tr.carrier_spans)),
                )
            })
            .as_deref()
    }

    fn compute_nodes(&self, carrier: Option<(&Surface, (f64, f64))>) -> Option<Vec<f64>> {
        let (a, b) = self.interval;
        let breaks = match &*self.curve.source {
            Curve::Nurbs(c) => knots_on_interval(c.breaks(), self.curve.source.period(), a, b)?,
            _ => Vec::new(),
        };
        let mut ts = cuts(a, b, 16, &breaks)?;
        // A foreign spline's partials can jump at a knot even when this
        // pcurve and the receiving face are smooth. Split the path there
        // before budgeting the higher-moment integrals on the receiving face.
        if let Some((Surface::Freeform { surface, .. }, spans)) = carrier {
            let spans = [spans.0, spans.1];
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

/// Inspect each interior knot and the midpoint of each piece it separates, in parameter order.
fn knot_witness<T>(
    breaks: &[f64],
    a: f64,
    b: f64,
    mut check: impl FnMut(f64) -> Option<T>,
) -> Option<T> {
    if breaks.is_empty() || a == b {
        return None;
    }
    let start = breaks.partition_point(|&k| if a < b { k <= a } else { k >= a });
    let end = breaks.partition_point(|&k| if a < b { k < b } else { k > b });
    let inside = &breaks[start..end];
    if inside.is_empty() {
        return None;
    }
    let mut last = a;
    for &k in inside {
        if let Some(hit) = check(last.midpoint(k)).or_else(|| check(k)) {
            return Some(hit);
        }
        last = k;
    }
    check(last.midpoint(b))
}

impl DeclaredBoundary {
    /// Keep the source-frame intervals and UV transfer. Caller-space samples and loops
    /// are rebuilt on demand.
    fn placed(&self, placement: &crate::step::Placement) -> Self {
        let edges = self
            .edges
            .iter()
            .map(|(&id, trim)| {
                let mut curve = trim.curve.clone();
                if let Some(transfer) = &mut curve.transfer {
                    transfer.placement = crate::step::compose(placement, &transfer.placement);
                }
                (
                    id,
                    EdgeTrim {
                        curve,
                        spans: trim.spans,
                        interval: trim.interval,
                        samples: OnceLock::new(),
                        mesh: trim.mesh,
                        uniform_mesh_stations: trim.uniform_mesh_stations,
                        witnesses: Vec::new(),
                        nodes: trim.nodes.clone(),
                        length: trim.length.clone(),
                    },
                )
            })
            .collect();
        Self {
            edges,
            ..DeclaredBoundary::default()
        }
    }
}

impl Part {
    pub(crate) fn carry_declared_boundary(
        &self,
        face: usize,
        to: &crate::brep::FaceCache,
        placement: &crate::step::Placement,
    ) {
        if let Some(boundary) = self.cache[face].declared.get() {
            to.declared
                .get_or_init(|| boundary.as_ref().map(|b| Box::new(b.placed(placement))));
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
        self.boundary_in_file_frame(face, false)
    }

    pub(crate) fn mesh_declared_boundary(&self, face: usize) -> Option<&DeclaredBoundary> {
        self.boundary_in_file_frame(face, true)
    }

    fn boundary_in_file_frame(&self, face: usize, mesh: bool) -> Option<&DeclaredBoundary> {
        let cache = &self.cache[face];
        let selected = if mesh {
            &cache.mesh_declared
        } else {
            &cache.declared
        };
        selected
            .get_or_init(|| {
                // Both modes qualify in file coordinates, including near the uncertainty limit.
                let boundary = if let Some(origin) = &self.integral_origin {
                    origin
                        .source
                        .boundary_in_file_frame(face, mesh)?
                        .placed(&origin.placement)
                } else {
                    self.read_declared_boundary(face, mesh)?
                };
                Some(Box::new(boundary))
            })
            .as_deref()
    }

    fn read_declared_boundary(&self, face: usize, mesh: bool) -> Option<DeclaredBoundary> {
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
        let make_curve =
            |raw: &RawCurve2d, carrier: &Surface, owner: Option<usize>, edge: Option<usize>| {
                let (scales, axes) = match carrier.file_parameter_scales(units) {
                    Some(scales) => (scales, [Axis::ID; 2]),
                    None => ((1., 1.), parameters::swept_axes(self, owner?)?),
                };
                let source = || scaled_pcurve(raw, scales.0, scales.1).map(Arc::new);
                let source = if let Some(edge) = edge {
                    let curves = self
                        .selected_curves
                        .get_or_init(|| self.edges.iter().map(|_| OnceLock::new()).collect());
                    let key = [scales.0.to_bits(), scales.1.to_bits()];
                    let cached = curves[edge].get_or_init(|| SelectedCurve {
                        scales: key,
                        source: source().map(|curve| self.selected_curve_pool.intern(curve, key)),
                    });
                    if cached.scales == key {
                        cached.source.clone()
                    } else {
                        // Native occurrences can use different parameter units.
                        // Their curves must not reuse the first occurrence's scale.
                        source().map(|curve| self.selected_curve_pool.intern(curve, key))
                    }
                } else {
                    source()
                }?;
                Some(ParamCurve {
                    source,
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
        let spans = self.face_closed_spans(face);
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
                    make_curve(raw, surface, Some(face), Some(e))?
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
                    let mut curve = make_curve(raw, &carrier, owner, Some(e))?;
                    curve.transfer = Some(Box::new(Transfer {
                        carrier_spans: owner.map_or_else(
                            || closed_spans(&carrier),
                            |owner| self.face_closed_spans(owner),
                        ),
                        face_spans: spans,
                        face_collapsed: self.face_collapsed_sides(face),
                        carrier,
                        face: shared(face),
                        placement: crate::step::IDENTITY,
                    }));
                    curve
                }
            };
            let witnesses: Vec<_> = if mesh && curve.transfer.is_some() {
                self.file_pcurves(face)
                    .iter()
                    .filter(|(edge, _)| *edge == e)
                    .filter_map(|(_, raw)| make_curve(raw, surface, Some(face), None))
                    .collect()
            } else {
                Vec::new()
            };
            let mut trim = resolve(
                (surface, spans),
                (&self.edges[e], self.edge_interval(e)),
                curve,
                tolerance,
                mesh,
                witnesses,
            )?;
            if mesh {
                // Keep the opposite sides of a narrow freeform strip aligned.
                // A freeform-owned trim transferred to an analytic neighbour
                // retains its knot stations to preserve that neighbour's winding.
                let carrier = trim
                    .curve
                    .transfer
                    .as_ref()
                    .map_or(surface, |tr| &tr.carrier);
                let incident = &self.edge_faces()[e];
                let freeform =
                    |&i: &usize| matches!(self.faces[i].surface, Surface::Freeform { .. });
                trim.uniform_mesh_stations = incident.iter().any(freeform)
                    && (!matches!(carrier, Surface::Freeform { .. })
                        || incident.iter().all(freeform));
            }
            result.edges.insert(e, trim);
        }
        // The mesher checks the shared boundary in the chart it triangulates.
        if !mesh {
            self.loops_on(face, Some(&result))?;
        }
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
    (surface, spans): (&Surface, (f64, f64)),
    (edge, spatial): (&crate::brep::Edge, (f64, f64)),
    curve: ParamCurve,
    tolerance: f64,
    mesh: bool,
    witnesses: Vec<ParamCurve>,
) -> Option<EdgeTrim> {
    let period = curve.source.period();
    let interval = resolve_interval(
        (surface, spans),
        (edge, spatial),
        &curve,
        tolerance,
        mesh,
        || Some(()),
    )?;
    let trim = EdgeTrim {
        curve,
        spans,
        interval,
        mesh,
        uniform_mesh_stations: false,
        witnesses,
        samples: OnceLock::new(),
        nodes: OnceLock::new(),
        length: OnceLock::new(),
    };
    if mesh
        && !trim.is_transferred()
        && EdgeTrim::midpoint_bound(&trim.curve, surface, interval).is_some()
    {
        // These bounded analytic evaluations cannot leave a finite carrier
        // domain or become nonfinite. Foreign curves still need the residual
        // checks below against the receiving face.
        return Some(trim);
    }
    if mesh
        && let Some(size) = EdgeTrim::affine_spatial_size(&trim.curve, surface, interval)
        && size < 1e100
        && interval.0.abs().max(interval.1.abs()) < 1e100
    {
        if !trim.is_transferred() {
            // This analytic line has no finite carrier domain to leave, and
            // the bounded affine arithmetic stays finite on the whole interval.
            return Some(trim);
        }
        let selected = [
            trim.point(surface, interval.0),
            trim.point(surface, interval.1),
        ];
        for witness in &trim.witnesses {
            let Some(local_size) = EdgeTrim::affine_spatial_size(witness, surface, interval) else {
                continue;
            };
            let error = 256. * f64::EPSILON * (size + local_size);
            if !error.is_finite() || error >= 0.25 * tolerance {
                continue;
            }
            let lift = |t| {
                let q = witness.value(t);
                let uv = on_carrier(surface, spans, q)?;
                Some(surface.value(uv.0, uv.1))
            };
            let [Some(a), Some(b)] = [lift(interval.0), lift(interval.1)] else {
                continue;
            };
            // Both spatial curves are affine under this affine parameter map.
            // Their difference lies in the convex hull of its endpoint
            // differences. Leave half the existing STEP tolerance unused,
            // including a bound based on intermediate magnitudes for rounding.
            let residuals = [geom::dist(selected[0], a), geom::dist(selected[1], b)];
            if residuals.iter().all(|x| x.is_finite())
                && residuals[0].max(residuals[1]) + error < 0.5 * tolerance
            {
                return Some(trim);
            }
        }
    }
    let (a, b) = interval;
    // Check the selected curve on the finite parameter range it actually uses.
    // A foreign pcurve has to describe points on this face within STEP's
    // uncertainty. The alternate 3D curve does not define this region.
    let knots = match &*trim.curve.source {
        Curve::Nurbs(c) => knots_on_interval(c.breaks(), period, a, b)?,
        _ => Vec::new(),
    };
    let validation_spans = cuts(a, b, 1, &knots)?;
    let mut hint = None;
    let inverse = super::SurfaceInversion::new(surface, mesh);
    let surface_value = super::SurfaceValue::new(surface, mesh);
    let carrier_value = trim
        .curve
        .transfer
        .as_ref()
        .map(|tr| super::SurfaceValue::new(&tr.carrier, mesh));
    for span in validation_spans.windows(2) {
        for j in 0..=32 {
            let t = span[0] + (span[1] - span[0]) * (j as f64 / 32.);
            let target = if mesh {
                trim.curve.transfer.as_ref().and_then(|tr| {
                    let uv = on_carrier(&tr.carrier, tr.carrier_spans, trim.curve.carrier_uv(t))?;
                    Some(carrier_value.as_ref()?.value(uv.0, uv.1))
                })
            } else {
                None
            };
            let checked = |hint| {
                let q = if let (Some(tr), Some(p)) = (&trim.curve.transfer, target) {
                    let (u, v) = tr.face_uv_with(p, hint, Some(&inverse))?;
                    [u, v, 0.]
                } else {
                    trim.curve.value_near(t, hint)
                };
                let uv = on_carrier(surface, spans, q)?;
                let p = surface_value.value(uv.0, uv.1);
                if !p.iter().all(|x| x.is_finite()) {
                    return None;
                }
                if let Some(tr) = &trim.curve.transfer {
                    let xyz = match target {
                        Some(p) => p,
                        None => trim.curve.lifted(t, tr)?.1,
                    };
                    let residual = geom::dist(p, xyz);
                    if !residual.is_finite() || residual > tolerance {
                        return None;
                    }
                }
                Some(uv)
            };
            // The local pcurve is only a witness that the selected point lies
            // on this carrier. It never supplies the shared spatial boundary.
            let mut closer = None;
            let mut witnessed = || {
                let tr = trim.curve.transfer.as_ref()?;
                if trim.witnesses.is_empty() {
                    return None;
                }
                let xyz = match target {
                    Some(p) => p,
                    None => trim.curve.lifted(t, tr)?.1,
                };
                trim.witnesses.iter().find_map(|witness| {
                    let uv = on_carrier(surface, spans, witness.value(t))?;
                    let residual = geom::dist(surface_value.value(uv.0, uv.1), xyz);
                    if mesh
                        && residual.is_finite()
                        && closer.is_none_or(|(_, previous)| residual < previous)
                    {
                        closer = Some((uv, residual));
                    }
                    (residual.is_finite() && residual <= tolerance).then_some(uv)
                })
            };
            if let Some(uv) = witnessed() {
                // An accepted witness already supplies this iteration's hint;
                // the inverse-seed comparison below would not be used.
                hint = Some(uv);
                continue;
            }
            let initial = if mesh { hint } else { None };
            let proposed = match (closer, target) {
                (Some((uv, residual)), Some(p))
                    if initial.is_none_or(|q| {
                        residual < geom::dist(surface_value.value(q.0, q.1), p)
                    }) =>
                {
                    Some(uv)
                }
                _ => initial,
            };
            let bits = |q: Option<(f64, f64)>| q.map(|q| (q.0.to_bits(), q.1.to_bits()));
            hint = Some(
                checked(proposed)
                    .or_else(|| {
                        (bits(proposed) != bits(initial))
                            .then(|| checked(initial))
                            .flatten()
                    })
                    .or_else(|| mesh.then(|| checked(None)).flatten())?,
            );
        }
    }
    Some(trim)
}

fn resolve_interval(
    (surface, spans): (&Surface, (f64, f64)),
    (edge, spatial): (&crate::brep::Edge, (f64, f64)),
    curve: &ParamCurve,
    tolerance: f64,
    mesh: bool,
    spend: impl Fn() -> Option<()>,
) -> Option<(f64, f64)> {
    let lift = |t| {
        let q = curve.value(t);
        let (u, v) = wrap_closed(surface, spans, (q[0], q[1]));
        spend()?;
        Some(surface.value(u, v))
    };
    let natural = match &*curve.source {
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
        if mesh {
            let endpoint = |t, expected| {
                let q = curve.value(t);
                if carrier_uv(surface, spans, q, &spend).is_none() || spend().is_none() {
                    return false;
                }
                let uv = wrap_closed(surface, spans, (q[0], q[1]));
                geom::dist(surface.value(uv.0, uv.1), expected) <= tolerance
            };
            return in_domain && endpoint(a, edge.start) && endpoint(b, edge.end);
        }
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
    let follows_spatial_branch = |(a, b): (f64, f64)| {
        if edge.is_closed() {
            return Some(true);
        }
        if let Some(turn) = edge.curve.period() {
            // Both endpoints also fit the complementary arc. The midpoint's
            // foot on the full spatial curve identifies the branch even if the
            // pcurve is offset or uses a different parameter direction.
            let p = lift(a.midpoint(b))?;
            let t = edge.curve.parameter(p);
            let t = t + turn * ((spatial.0.midpoint(spatial.1) - t) / turn).round();
            return Some((spatial.0.min(spatial.1)..=spatial.0.max(spatial.1)).contains(&t));
        }
        if !matches!(edge.curve, Curve::Nurbs(_)) {
            return Some(true);
        }
        // A spatial spline can contain just the finite arc and have no period.
        // If both endpoint tangents point in opposite directions along the same
        // tangent lines, the inferred path leaves and enters on the wrong sides.
        let orientation = if (b > a) == (spatial.1 > spatial.0) {
            1.
        } else {
            -1.
        };
        for (t, s) in [(a, spatial.0), (b, spatial.1)] {
            spend()?;
            let (uv, d) = curve.value_and_derivative(t);
            let (u, v) = wrap_closed(surface, spans, (uv[0], uv[1]));
            let (su, sv) = surface.partials(u, v);
            let tangent = geom::add(geom::scale(su, d[0]), geom::scale(sv, d[1]));
            let opposite = geom::unit(tangent)
                .zip(geom::unit(edge.curve.derivative(s)))
                .is_some_and(|(x, y)| orientation * geom::dot(x, y) < -1. + 1e-8);
            if !opposite {
                return Some(true);
            }
        }
        Some(false)
    };
    [
        Some(spatial),
        natural.filter(|_| edge.same_sense),
        natural.map(|(a, b)| (b, a)).filter(|_| !edge.same_sense),
    ]
    .into_iter()
    .flatten()
    .find(|&ab| valid(ab))
    .or_else(|| {
        let ab = guessed()?;
        (valid(ab) && follows_spatial_branch(ab)?).then_some(ab)
    })
}

#[cfg(test)]
mod tests;
