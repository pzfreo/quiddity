//! Measured, constant-section helical screw threads. No nominal thread designation is inferred.
use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::{PI, TAU};

use serde::Serialize;

use super::Context;
use super::evidence::{self, EvidenceError, Occurrence};
use crate::kernel::brep::Part;
use crate::kernel::geom::{self, Curve, Surface, V3};

/// A line, with unit direction and the point on the line closest to the origin.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct ThreadAxis {
    pub point: V3,
    pub direction: V3,
}

impl ThreadAxis {
    fn new(point: V3, direction: V3) -> Option<Self> {
        let mut direction = geom::unit(direction)?;
        if direction[geom::dominant_axis_preferring_z(direction)] < 0.0 {
            direction = geom::scale(direction, -1.0);
        }
        Some(Self {
            point: geom::sub(point, geom::scale(direction, geom::dot(point, direction))),
            direction,
        })
    }

    #[cfg(test)]
    fn reference_polar(self, point: V3) -> (f64, f64, f64) {
        let q = geom::sub(point, self.point);
        let x = geom::default_x_axis(self.direction);
        let y = geom::cross(self.direction, x);
        let (u, v) = (geom::dot(q, x), geom::dot(q, y));
        (u.hypot(v), v.atan2(u), geom::dot(q, self.direction))
    }

    fn polar(self, point: V3) -> (f64, f64, f64) {
        ThreadProjection::from(self).polar(point)
    }
}

/// Fixed coordinates for one candidate axis. Each dot product keeps its original
/// operands and order; radius-only filters do not need an angle or axial height.
#[derive(Clone, Copy)]
struct ThreadProjection {
    axis: ThreadAxis,
    x: V3,
    y: V3,
}

/// Bounds of the existing edge samples, not a replacement for the source curve.
#[derive(Clone, Copy)]
struct SampleBounds {
    lo: V3,
    hi: V3,
}

impl SampleBounds {
    fn of(points: &[V3]) -> Option<Self> {
        let &first = points.first()?;
        let mut bounds = Self {
            lo: first,
            hi: first,
        };
        for &point in points {
            if !point.iter().all(|x| x.is_finite()) {
                return None;
            }
            for (k, &value) in point.iter().enumerate() {
                bounds.lo[k] = bounds.lo[k].min(value);
                bounds.hi[k] = bounds.hi[k].max(value);
            }
        }
        Some(bounds)
    }
}

impl From<ThreadAxis> for ThreadProjection {
    fn from(axis: ThreadAxis) -> Self {
        let x = geom::default_x_axis(axis.direction);
        Self {
            axis,
            x,
            y: geom::cross(axis.direction, x),
        }
    }
}

impl ThreadProjection {
    /// Enclose the *rounded* coordinates using the same subtraction and dot product.
    /// Each operation is monotone in its operands; negative coefficients reverse
    /// the selected endpoints. Non-finite endpoints cannot establish a quadrant.
    fn coordinate_ranges(self, bounds: SampleBounds) -> Option<[(f64, f64); 2]> {
        let lo = geom::sub(bounds.lo, self.axis.point);
        let hi = geom::sub(bounds.hi, self.axis.point);
        if !lo.iter().chain(&hi).all(|x| x.is_finite()) {
            return None;
        }
        let ranges = [self.x, self.y].map(|direction| {
            let low = std::array::from_fn(|k| if direction[k] < 0.0 { hi[k] } else { lo[k] });
            let high = std::array::from_fn(|k| if direction[k] < 0.0 { lo[k] } else { hi[k] });
            (geom::dot(low, direction), geom::dot(high, direction))
        });
        ranges
            .iter()
            .all(|&(lo, hi)| lo.is_finite() && hi.is_finite())
            .then_some(ranges)
    }

    /// A single strict quadrant cannot wind. Its nearest-turn steps all subtract
    /// zero, so the original fit's endpoint-angle rejection can be evaluated now.
    /// The strict comparison retains the complete path at the quarter-turn boundary.
    fn short_quadrant(self, bounds: SampleBounds, points: &[V3]) -> bool {
        let Some(ranges) = self.coordinate_ranges(bounds) else {
            return false;
        };
        if !ranges.iter().all(|&(lo, hi)| lo > 0.0 || hi < 0.0) {
            return false;
        }
        let (_, u0, v0) = self.coordinates(points[0]);
        let (_, u1, v1) = self.coordinates(*points.last().unwrap());
        let first = v0.atan2(u0);
        let last = geom::nearest_turn(v1.atan2(u1), first);
        (last - first).abs() < PI * 0.5
    }

    fn coordinates(self, point: V3) -> (V3, f64, f64) {
        let q = geom::sub(point, self.axis.point);
        (q, geom::dot(q, self.x), geom::dot(q, self.y))
    }

    fn radius(self, point: V3) -> f64 {
        let (_, u, v) = self.coordinates(point);
        u.hypot(v)
    }

    fn height(self, point: V3) -> f64 {
        geom::dot(geom::sub(point, self.axis.point), self.axis.direction)
    }

    fn polar(self, point: V3) -> (f64, f64, f64) {
        let (q, u, v) = self.coordinates(point);
        (u.hypot(v), v.atan2(u), geom::dot(q, self.axis.direction))
    }
}

/// Physical measurements, in millimetres. The span is measured from `axis.point` along
/// `axis.direction`; `length = end - start`. Hand is intrinsic and survives axis reversal.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct ModelledThread {
    pub axis: ThreadAxis,
    pub side: &'static str,
    pub major_diameter: f64,
    pub minor_diameter: f64,
    pub pitch: f64,
    pub hand: &'static str,
    pub starts: usize,
    pub start: f64,
    pub end: f64,
    pub length: f64,
    pub tolerances: ThreadTolerances,
    pub flank_faces: Vec<usize>,
    pub crest_faces: Vec<usize>,
    pub root_faces: Vec<usize>,
    pub runout_faces: Vec<usize>,
}

/// Acceptance bounds in millimetres, including interpolation error in the STEP support.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ThreadTolerances {
    pub diameter: f64,
    pub pitch: f64,
    pub axial: f64,
}

/// A helical candidate that could not establish a constant screw thread. It claims no faces.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct ModelledThreadRefusal {
    pub axis: ThreadAxis,
    pub faces: Vec<usize>,
    pub reason: &'static str,
}

/// A recogniser run's measurements and explicit refusals.
#[derive(Clone, Debug, Default)]
pub struct ThreadAnalysis {
    pub threads: Vec<Occurrence<ModelledThread>>,
    pub refusals: Vec<ModelledThreadRefusal>,
}

/// Radial measurement tolerance: 0.001 mm or 0.1% of the radius.
fn tolerance(size: f64) -> f64 {
    1e-3_f64.max(size.abs() * 1e-3)
}

#[derive(Clone, Debug)]
struct Helix {
    edge: usize,
    lead: f64,
    start: f64,
    end: f64,
    turns: f64,
}

/// Unwrap the samples in traversal order and fit z = intercept + lead * theta / 2π.
/// Both radius and every axial residual must agree, including the middle of the curve.
fn fit_helix(axis: impl Into<ThreadProjection>, edge: usize, points: &[V3]) -> Option<Helix> {
    if points.len() < 9 {
        return None;
    }
    let axis = axis.into();
    let mut polar = Vec::with_capacity(points.len());
    let mut previous = None;
    for &p in points {
        let (r, a, z) = axis.polar(p);
        let a = previous.map_or(a, |prev| geom::nearest_turn(a, prev));
        previous = Some(a);
        polar.push((r, a, z));
    }
    let n = polar.len() as f64;
    let radius = polar.iter().map(|p| p.0).sum::<f64>() / n;
    if radius <= 0.0
        || polar
            .iter()
            .any(|p| (p.0 - radius).abs() > tolerance(radius))
    {
        return None;
    }
    let a_mean = polar.iter().map(|p| p.1).sum::<f64>() / n;
    let z_mean = polar.iter().map(|p| p.2).sum::<f64>() / n;
    let da = polar.last()?.1 - polar[0].1;
    let lead = (polar.last()?.2 - polar[0].2) * TAU / da;
    let slope = lead / TAU;
    if !lead.is_finite() || lead.abs() < 1e-3 || da.abs() < PI * 0.5 {
        return None;
    }
    if polar
        .iter()
        .any(|p| (p.2 - z_mean - slope * (p.1 - a_mean)).abs() > axial_tolerance(lead))
        || polar.windows(2).any(|w| (w[1].1 - w[0].1) * da < -1e-8)
    {
        return None;
    }
    Some(Helix {
        edge,
        lead,
        start: polar.iter().map(|p| p.2).fold(f64::INFINITY, f64::min),
        end: polar.iter().map(|p| p.2).fold(f64::NEG_INFINITY, f64::max),
        turns: (da / TAU).abs(),
    })
}

/// Cylinder/cone/toroid supports and circular boundaries supply candidate lines. The line
/// must then be established by helical geometry; an analytic surface alone is never a thread.
fn axes(part: &Part) -> Vec<(Option<usize>, ThreadAxis)> {
    let mut result: Vec<(Option<usize>, ThreadAxis)> = Vec::new();
    let mut by_solid: BTreeMap<Option<usize>, Vec<usize>> = BTreeMap::new();
    let mut add = |solid, point, direction| {
        let Some(axis) = ThreadAxis::new(point, direction) else {
            return;
        };
        let indices = by_solid.entry(solid).or_default();
        if !indices.iter().any(|&i| {
            let a = &result[i].1;
            geom::dist(a.point, axis.point) <= 1e-6
                && geom::dist(a.direction, axis.direction) <= 1e-8
        }) {
            indices.push(result.len());
            result.push((solid, axis));
        }
    };
    for f in &part.faces {
        match &f.surface {
            Surface::Cylinder { frame, .. }
            | Surface::Cone { frame, .. }
            | Surface::Torus { frame, .. } => add(f.solid, frame.origin, frame.z),
            _ => {}
        }
        for &(e, _) in f.loops.iter().flat_map(|l| &l.edges) {
            if let Curve::Circle { frame, .. } = &part.edges[e].curve {
                add(f.solid, frame.origin, frame.z);
            }
        }
    }
    result
}

/// Preserve the original edge order and eligibility predicate for every owner,
/// including unowned faces and edges incident to faces with different owners.
fn eligible_edges(part: &Part) -> BTreeMap<Option<usize>, Vec<usize>> {
    let mut result: BTreeMap<Option<usize>, Vec<usize>> = BTreeMap::new();
    for (e, edge) in part.edges.iter().enumerate() {
        if edge.samples.len() < 9 || !matches!(edge.curve, Curve::Nurbs(_)) {
            continue;
        }
        let owners: BTreeSet<_> = part.edge_faces()[e]
            .iter()
            .map(|&f| part.faces[f].solid)
            .collect();
        for owner in owners {
            result.entry(owner).or_default().push(e);
        }
    }
    result
}

/// A failed fit is reportable when a cylindrical winding itself is unambiguous: two
/// complete turns, monotone advance, and no more than 10% radial drift. Short blend edges
/// never enter this refusal path.
fn sustained_winding(axis: impl Into<ThreadProjection>, points: &[V3]) -> bool {
    if points.len() < 17 {
        return false;
    }
    let axis = axis.into();
    let (radius, mut angle, z0) = axis.polar(points[0]);
    if radius <= 0.0 {
        return false;
    }
    let first = angle;
    let mut previous_z = z0;
    let dz = axis.height(*points.last().unwrap()) - z0;
    if dz.abs() < 0.1 {
        return false;
    }
    for &p in &points[1..] {
        let (q, u, v) = axis.coordinates(p);
        let r = u.hypot(v);
        if (r - radius).abs() > 0.1 * radius {
            return false;
        }
        let z = geom::dot(q, axis.axis.direction);
        if (z - previous_z) * dz < -1e-7 {
            return false;
        }
        angle = geom::nearest_turn(v.atan2(u), angle);
        previous_z = z;
    }
    (angle - first).abs() >= TAU * 1.99
}

internal_entry! {
/// Measured threads and unsupported helical candidates, in source face order.
pub fn analyse_modelled_threads(part: &Part) -> ThreadAnalysis {
    let mut result = ThreadAnalysis::default();
    if !part
        .faces
        .iter()
        .any(|f| matches!(f.surface, Surface::Freeform { .. }))
    {
        return result;
    }
    let candidate_axes = {
        axes(part)
    };
    let edges_by_solid = {
        eligible_edges(part)
    };
    let edge_bounds = {
        let mut bounds = vec![None; part.edges.len()];
        let eligible: BTreeSet<_> = edges_by_solid.values().flatten().copied().collect();
        for e in eligible {
            bounds[e] = SampleBounds::of(&part.edges[e].samples);
        }
        bounds
    };
    let mut claimed = BTreeSet::new();
    for (solid, axis) in candidate_axes {
        let projection = ThreadProjection::from(axis);
        let mut helices = BTreeMap::new();
        let mut varying = BTreeSet::new();
        for &e in edges_by_solid.get(&solid).into_iter().flatten() {
            let edge = &part.edges[e];
            if edge_bounds[e].is_some_and(|bounds| projection.short_quadrant(bounds, &edge.samples)) {
                continue;
            }
            // Cheap radius rejection before unwrapping a potentially long edge.
            let r = projection.radius(edge.start);
            if [edge.end, edge.samples[edge.samples.len() / 2]]
                .iter()
                .any(|&p| (projection.radius(p) - r).abs() > 2.0 * tolerance(r))
            {
                if sustained_winding(projection, &edge.samples) {
                    varying.extend(part.edge_faces()[e].iter().copied());
                }
                continue;
            }
            if let Some(helix) = fit_helix(projection, e, &edge.samples) {
                helices.insert(e, helix);
            } else if sustained_winding(projection, &edge.samples) {
                varying.extend(part.edge_faces()[e].iter().copied());
            }
        }
        let mut remaining: BTreeSet<_> = helices.keys().copied().collect();
        while let Some(seed) = remaining.pop_first() {
            let mut component = BTreeSet::from([seed]);
            let mut pending = vec![seed];
            while let Some(e) = pending.pop() {
                for &f in &part.edge_faces()[e] {
                    for other in part.face_edges(f) {
                        if remaining.remove(&other) {
                            component.insert(other);
                            pending.push(other);
                        }
                    }
                }
            }
            let group: Vec<_> = component.iter().map(|e| &helices[e]).collect();
            if component
                .iter()
                .any(|&e| part.edge_faces()[e].iter().any(|f| claimed.contains(f)))
            {
                continue;
            }
            let before = result.threads.len();
            measure(part, axis, &group, &mut result);
            if result.threads.len() > before {
                claimed.extend(result.threads.last().unwrap().defining.iter().copied());
            }
        }
        varying.retain(|f| !claimed.contains(f));
        if !varying.is_empty()
            && !result
                .refusals
                .iter()
                .any(|r| r.faces.iter().any(|f| varying.contains(f)))
        {
            refuse(
                &mut result,
                axis,
                &varying,
                "inconsistent_pitch_or_diameters",
            );
        }
    }
    result.threads.sort_by_key(|o| o.defining.first().copied());
    result
        .refusals
        .sort_by(|a, b| a.faces.cmp(&b.faces).then_with(|| a.reason.cmp(b.reason)));
    result
        .refusals
        .dedup_by(|a, b| a.faces == b.faces && a.reason == b.reason);
    result
}
}

internal_entry! {
/// Records only; [`analyse_modelled_threads`] also reports refused candidates.
pub fn recognise_modelled_threads(part: &Part) -> Vec<ModelledThread> {
    super::records(discover(&Context::new(part)))
}
}

internal_entry! {
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<ModelledThread>> {
    ctx.modelled_threads().threads.clone()
}
}

internal_entry! {
pub fn discover_verified(
    ctx: &Context<'_>,
) -> Result<Vec<Occurrence<ModelledThread>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx))
}
}

/// Axial shape residual, allowing the interpolation error of STEP approximated helices.
fn axial_tolerance(lead: f64) -> f64 {
    0.003_f64.max(lead.abs() * 0.005)
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    let m = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[m - 1] + values[m]) * 0.5
    } else {
        values[m]
    }
}

#[derive(Clone)]
struct Samples {
    points: Vec<V3>,
    normals: Vec<(V3, V3)>,
}

/// Visit every knot span as well as a uniform grid: local changes in a long B-spline
/// cannot fall entirely between the uniform stations.
fn sample_parameters(lo: f64, hi: f64, knots: &[f64]) -> Vec<f64> {
    let mut values: Vec<_> = (0..=16).map(|i| lo + (hi - lo) * i as f64 / 16.0).collect();
    for w in knots.windows(2) {
        let a = w[0].max(lo);
        let b = w[1].min(hi);
        if b > a {
            values.extend([a, a + (b - a) * 0.25, (a + b) * 0.5, a + (b - a) * 0.75, b]);
        }
    }
    values.sort_by(f64::total_cmp);
    values.dedup_by(|a, b| (*a - *b).abs() <= 1e-12 * (hi - lo).abs());
    values
}

fn sample_face(part: &Part, face: usize) -> Samples {
    let mut points: Vec<_> = part
        .face_edges(face)
        .iter()
        .flat_map(|&e| part.edges[e].samples.iter().copied())
        .collect();
    let mut normals = Vec::new();
    if let Some((u0, u1, v0, v1)) = part.uv_bounds(face) {
        let (ku, kv) = match &part.faces[face].surface {
            Surface::Freeform { surface, .. } => {
                (surface.knots_u.as_slice(), surface.knots_v.as_slice())
            }
            _ => (&[][..], &[][..]),
        };
        let us = sample_parameters(u0, u1, ku);
        let vs = sample_parameters(v0, v1, kv);
        for &u in &us {
            for &v in &vs {
                if !part.domain(face).is_some_and(|d| d.contains(u, v)) {
                    continue;
                }
                let p = part.faces[face].surface.value(u, v);
                points.push(p);
                if let Some(n) = part.face_normal(face, u, v) {
                    normals.push((p, n));
                }
            }
        }
    }
    Samples { points, normals }
}

fn ranges(axis: ThreadAxis, samples: &Samples) -> (f64, f64, f64, f64) {
    samples.points.iter().fold(
        (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ),
        |(r0, r1, z0, z1), &p| {
            let (r, _, z) = axis.polar(p);
            (r0.min(r), r1.max(r), z0.min(z), z1.max(z))
        },
    )
}

fn refuse(
    result: &mut ThreadAnalysis,
    axis: ThreadAxis,
    faces: &BTreeSet<usize>,
    reason: &'static str,
) {
    result.refusals.push(ModelledThreadRefusal {
        axis,
        faces: faces.iter().copied().collect(),
        reason,
    });
}

/// A constant straight flank in unwrapped cylindrical coordinates. The slope is measured
/// from its normals; z - lead*theta/2π - slope*r must be invariant, modulo the lead.
struct Flank {
    face: usize,
    slope: f64,
    phase: f64,
    radial_sign: f64,
}

fn flank(axis: ThreadAxis, face: usize, samples: &Samples, lead: f64) -> Option<Flank> {
    let mut slopes = Vec::new();
    let mut signs = Vec::new();
    for &(p, n) in &samples.normals {
        let q = geom::sub(p, axis.point);
        let radial = geom::unit(geom::sub(
            q,
            geom::scale(axis.direction, geom::dot(q, axis.direction)),
        ))?;
        let (nz, nr) = (geom::dot(n, axis.direction), geom::dot(n, radial));
        if nz.abs() < 0.2 || nr.abs() < 0.1 {
            return None;
        }
        slopes.push(-nr / nz);
        signs.push(nr.signum());
    }
    if slopes.len() < 9 || signs.iter().any(|s| *s != signs[0]) {
        return None;
    }
    let slope = median(&mut slopes);
    if slopes
        .iter()
        .any(|s| (s - slope).abs() > 0.02 * slope.abs().max(1.0))
    {
        return None;
    }
    let mut phases: Vec<_> = samples
        .normals
        .iter()
        .map(|&(p, _)| {
            let (r, a, z) = axis.polar(p);
            (z - lead * a / TAU - slope * r).rem_euclid(lead.abs())
        })
        .collect();
    let reference = phases[0];
    for phase in &mut phases {
        *phase = reference + (*phase - reference + lead.abs() * 0.5).rem_euclid(lead.abs())
            - lead.abs() * 0.5;
    }
    let phase = median(&mut phases);
    if phases
        .iter()
        .any(|p| (p - phase).abs() > axial_tolerance(lead))
    {
        return None;
    }
    Some(Flank {
        face,
        slope,
        phase: phase.rem_euclid(lead.abs()),
        radial_sign: signs[0],
    })
}

fn measure(part: &Part, axis: ThreadAxis, group: &[&Helix], result: &mut ThreadAnalysis) {
    if group.iter().map(|h| h.turns).sum::<f64>() < 1.5 {
        return;
    }
    let mut leads: Vec<_> = group
        .iter()
        .filter(|h| h.turns >= 0.45)
        .map(|h| h.lead)
        .collect();
    if leads.is_empty() {
        return;
    }
    let lead = median(&mut leads);
    let mut faces: BTreeSet<_> = group
        .iter()
        .flat_map(|h| part.edge_faces()[h.edge].iter().copied())
        .collect();
    let mut samples: BTreeMap<_, _> = faces.iter().map(|&f| (f, sample_face(part, f))).collect();
    let (r0, r1) = samples.values().map(|s| ranges(axis, s)).fold(
        (f64::INFINITY, f64::NEG_INFINITY),
        |(lo, hi), (a, b, _, _)| (lo.min(a), hi.max(b)),
    );
    let depth = r1 - r0;
    if depth < 4.0 * tolerance(r1) || depth > lead.abs() * 2.0 {
        return;
    }
    let z0 = group.iter().map(|h| h.start).fold(f64::INFINITY, f64::min);
    let z1 = group
        .iter()
        .map(|h| h.end)
        .fold(f64::NEG_INFINITY, f64::max);
    let mut pending: Vec<_> = faces.iter().copied().collect();
    while let Some(face) = pending.pop() {
        for f in part.neighbours(face) {
            if faces.contains(&f)
                || part.faces[f].solid != part.faces[face].solid
                || !matches!(
                    part.faces[f].surface,
                    Surface::Freeform { .. }
                        | Surface::Cylinder { .. }
                        | Surface::Cone { .. }
                        | Surface::Torus { .. }
                )
            {
                continue;
            }
            let s = sample_face(part, f);
            let (a, b, c, d) = ranges(axis, &s);
            let pad = tolerance(r1);
            if a < r0 - pad || b > r1 + pad {
                // Follow a changing thread to refuse it as a whole; an ordinary shoulder
                // or blend outside the thread envelope ends the candidate instead.
                let continues = matches!(part.faces[f].surface, Surface::Freeform { .. })
                    && a >= r0 - 0.5 * depth
                    && b <= r1 + 0.5 * depth
                    && (flank(axis, f, &s, lead).is_some()
                        || part
                            .face_edges(f)
                            .iter()
                            .any(|&e| sustained_winding(axis, &part.edges[e].samples)));
                if !continues {
                    continue;
                }
            }
            let carrier = matches!(part.faces[f].surface, Surface::Cylinder { .. })
                && (c < z0 - lead.abs() || d > z1 + lead.abs());
            faces.insert(f);
            samples.insert(f, s);
            if !carrier {
                pending.push(f);
            }
        }
    }
    let all_edges: BTreeSet<_> = faces.iter().flat_map(|&f| part.face_edges(f)).collect();
    for e in all_edges {
        if let Some(h) = fit_helix(axis, e, &part.edges[e].samples)
            && h.turns >= 0.45
        {
            leads.push(h.lead);
        }
    }
    if leads
        .iter()
        .any(|p| (p - lead).abs() > axial_tolerance(lead))
    {
        refuse(result, axis, &faces, "inconsistent_pitch");
        return;
    }
    let mut flanks = Vec::new();
    let mut flat_low = Vec::new();
    let mut flat_high = Vec::new();
    let mut runouts = Vec::new();
    let mut low_radii = Vec::new();
    let mut high_radii = Vec::new();
    let mut start = f64::INFINITY;
    let mut end = f64::NEG_INFINITY;
    for &f in &faces {
        let s = &samples[&f];
        let (a, b, c, d) = ranges(axis, s);
        if b - a <= 2.0 * tolerance(r1) {
            let mut radii: Vec<_> = s.points.iter().map(|&p| axis.polar(p).0).collect();
            let radius = median(&mut radii);
            if (radius - r0).abs() <= 2.0 * tolerance(r1) {
                flat_low.push(f);
                low_radii.extend(radii);
            } else if (radius - r1).abs() <= 2.0 * tolerance(r1) {
                flat_high.push(f);
                high_radii.extend(radii);
            } else {
                refuse(result, axis, &faces, "inconsistent_diameters");
                return;
            }
            continue;
        }
        if let Some(flank) = matches!(part.faces[f].surface, Surface::Freeform { .. })
            .then(|| flank(axis, f, s, lead))
            .flatten()
        {
            if a > r0 + 0.2 * depth || b < r1 - 0.2 * depth {
                refuse(result, axis, &faces, "inconsistent_diameters");
                return;
            }
            flanks.push(flank);
            start = start.min(c);
            end = end.max(d);
        } else if d - c < lead.abs() && (c < z0 + lead.abs() || d > z1 - lead.abs()) {
            runouts.push(f);
            start = start.min(c);
            end = end.max(d);
        } else {
            refuse(result, axis, &faces, "inconsistent_flank_or_pitch");
            return;
        }
    }
    if flat_low.is_empty() || flat_high.is_empty() || flanks.len() < 2 {
        refuse(result, axis, &faces, "missing_crest_root_or_opposed_flanks");
        return;
    }
    let side = flanks[0].radial_sign;
    if flanks.iter().any(|f| f.radial_sign != side)
        || !flanks.iter().any(|f| f.slope > 0.0)
        || !flanks.iter().any(|f| f.slope < 0.0)
    {
        refuse(result, axis, &faces, "not_a_screw_thread_profile");
        return;
    }
    let starts = match count_starts(&flanks, lead) {
        Ok(n) => n,
        Err(reason) => {
            refuse(result, axis, &faces, reason);
            return;
        }
    };
    let pitch = lead.abs() / starts as f64;
    if end - start < 2.0 * pitch {
        refuse(result, axis, &faces, "insufficient_axial_repetition");
        return;
    }
    let minor_radius = median(&mut low_radii);
    let major_radius = median(&mut high_radii);
    if low_radii
        .iter()
        .any(|r| (r - minor_radius).abs() > 2.0 * tolerance(minor_radius))
        || high_radii
            .iter()
            .any(|r| (r - major_radius).abs() > 2.0 * tolerance(major_radius))
    {
        refuse(result, axis, &faces, "inconsistent_diameters");
        return;
    }
    // A shallow, isolated spiral channel does not establish an engagement profile.
    if depth / pitch < 0.2 || depth / pitch > 0.9 {
        refuse(result, axis, &faces, "groove_proportions");
        return;
    }
    let minor_diameter = 2.0 * minor_radius;
    let major_diameter = 2.0 * major_radius;
    let adjacent: BTreeSet<_> = faces.iter().flat_map(|&f| part.neighbours(f)).collect();
    for f in adjacent {
        if faces.contains(&f) || !matches!(part.faces[f].surface, Surface::Plane { .. }) {
            continue;
        }
        let s = sample_face(part, f);
        let (a, b, c, d) = ranges(axis, &s);
        if a >= minor_radius - tolerance(minor_radius)
            && b <= major_radius + tolerance(major_radius)
            && c >= start
            && d <= end
            && d - c > axial_tolerance(lead)
            && part
                .neighbours(f)
                .iter()
                .filter(|n| faces.contains(n))
                .count()
                >= 2
        {
            runouts.push(f);
            faces.insert(f);
        }
    }
    runouts.sort_unstable();
    let (crest_faces, root_faces) = if side > 0.0 {
        (flat_high, flat_low)
    } else {
        (flat_low, flat_high)
    };
    if evidence::common_valid_solid(part, &faces.iter().copied().collect::<Vec<_>>()).is_none() {
        refuse(result, axis, &faces, "no_valid_solid");
        return;
    }
    let defining: Vec<_> = faces.into_iter().collect();
    result.threads.push(Occurrence {
        defining,
        context: Vec::new(),
        record: ModelledThread {
            axis,
            side: if side > 0.0 { "external" } else { "internal" },
            major_diameter,
            minor_diameter,
            pitch,
            hand: if lead > 0.0 { "right" } else { "left" },
            starts,
            start,
            end,
            length: end - start,
            tolerances: ThreadTolerances {
                diameter: 4.0 * tolerance(major_radius),
                pitch: axial_tolerance(lead) / starts as f64,
                axial: axial_tolerance(lead),
            },
            flank_faces: flanks.iter().map(|f| f.face).collect(),
            crest_faces,
            root_faces,
            runout_faces: runouts,
        },
    });
}

fn count_starts(flanks: &[Flank], lead: f64) -> Result<usize, &'static str> {
    let mut counts = Vec::new();
    for sign in [-1.0, 1.0] {
        let mut phases: Vec<_> = flanks
            .iter()
            .filter(|f| f.slope * sign > 0.0)
            .map(|f| f.phase)
            .collect();
        phases.sort_by(f64::total_cmp);
        let mut clusters: Vec<f64> = Vec::new();
        for phase in phases {
            if !clusters.iter().any(|&p| {
                ((phase - p + lead.abs() * 0.5).rem_euclid(lead.abs()) - lead.abs() * 0.5).abs()
                    < axial_tolerance(lead)
            }) {
                clusters.push(phase);
            }
        }
        let count = clusters.len();
        for i in 0..count {
            let gap = (clusters[(i + 1) % count] - clusters[i]).rem_euclid(lead.abs());
            if count > 1 && (gap - lead.abs() / count as f64).abs() > 2.0 * axial_tolerance(lead) {
                return Err("inconsistent_start_spacing");
            }
        }
        counts.push(count);
    }
    if counts[0] != counts[1] {
        return Err("unpaired_starts");
    }
    Ok(counts[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quadrant_bounds_enclose_coordinates_and_only_reject_failed_fits() {
        let axes = [
            axis(),
            ThreadAxis::new([1.0, -2.0, 3.0], [1.0, 2.0, 3.0]).unwrap(),
            ThreadAxis::new([1e12, -2e12, 3e12], [1e-9, -2e-9, 1.0]).unwrap(),
        ];
        let mut rejected = 0;
        for lead in [-2.0, 0.0, 2.0] {
            for turns in [0.0, 0.1, 0.25, 0.5, 1.0, 2.0, 4.0] {
                for offset in [0.0, 20.0, -20.0, 1e12, -1e12] {
                    for axis in axes {
                        let mut points = helix_points(lead, turns);
                        for p in &mut points {
                            p[0] += offset;
                            p[1] += offset;
                        }
                        let bounds = SampleBounds::of(&points).unwrap();
                        let projection = ThreadProjection::from(axis);
                        if let Some(ranges) = projection.coordinate_ranges(bounds) {
                            for &point in &points {
                                let (_, u, v) = projection.coordinates(point);
                                for (value, (lo, hi)) in [u, v].into_iter().zip(ranges) {
                                    assert!(lo <= value && value <= hi, "{lo} <= {value} <= {hi}");
                                }
                            }
                        }
                        for samples in [&points, &points.iter().rev().copied().collect::<Vec<_>>()]
                        {
                            if projection.short_quadrant(bounds, samples) {
                                rejected += 1;
                                // Both complete original checks remain independent of the gate.
                                assert!(fit_helix(projection, 0, samples).is_none());
                                assert!(!sustained_winding(projection, samples));
                            }
                        }
                    }
                }
            }
        }
        assert!(rejected > 100);
    }

    #[test]
    fn quadrant_gate_retains_full_turns_boundaries_and_nonfinite_inputs() {
        let projection = ThreadProjection::from(axis());
        let full = helix_points(2.0, 4.0);
        assert!(!projection.short_quadrant(SampleBounds::of(&full).unwrap(), &full));
        assert!(fit_helix(projection, 0, &full).is_some());
        assert!(sustained_winding(projection, &full));
        let boundary = [
            [f64::MIN_POSITIVE, 1e300, 0.0],
            [1e300, f64::MIN_POSITIVE, 1.0],
        ];
        assert!(!projection.short_quadrant(SampleBounds::of(&boundary).unwrap(), &boundary));
        assert!(SampleBounds::of(&[[f64::NAN, 1.0, 0.0]]).is_none());
        assert!(SampleBounds::of(&[[f64::INFINITY, 1.0, 0.0]]).is_none());
        assert!(SampleBounds::of(&[]).is_none());
        let crosses_zero = [[-0.0, 1.0, 0.0], [0.0, 2.0, 1.0]];
        assert!(
            !projection.short_quadrant(SampleBounds::of(&crosses_zero).unwrap(), &crosses_zero)
        );
        let overflow = ThreadProjection::from(ThreadAxis {
            point: [-f64::MAX, 0.0, 0.0],
            direction: [0.0, 0.0, 1.0],
        });
        let points = [[f64::MAX, 1.0, 0.0], [f64::MAX, 2.0, 1.0]];
        assert!(
            overflow
                .coordinate_ranges(SampleBounds::of(&points).unwrap())
                .is_none()
        );
    }

    #[test]
    fn prepared_projection_preserves_coordinate_bits() {
        let axes = [
            axis(),
            ThreadAxis::new([1.0, -2.0, 3.0], [1.0, 2.0, 3.0]).unwrap(),
            ThreadAxis::new([1e12, -2e12, 3e12], [1e-9, -2e-9, 1.0]).unwrap(),
            ThreadAxis {
                point: [-0.0, 0.0, 1.0],
                direction: [0.0, 0.0, -1.0],
            },
            ThreadAxis {
                point: [0.0; 3],
                direction: [0.0; 3],
            },
        ];
        let mut points = helix_points(2.0, 4.0);
        points.extend([
            [0.0, -0.0, 0.0],
            [f64::MIN_POSITIVE, -f64::MIN_POSITIVE, 0.0],
            [1e12, -2e12, 3e12],
            [1e150, -2e150, 3e150],
        ]);
        for axis in axes {
            let projection = ThreadProjection::from(axis);
            for &point in &points {
                let expected = axis.reference_polar(point);
                let actual = projection.polar(point);
                assert_eq!(
                    [actual.0, actual.1, actual.2].map(f64::to_bits),
                    [expected.0, expected.1, expected.2].map(f64::to_bits)
                );
                assert_eq!(projection.radius(point).to_bits(), expected.0.to_bits());
                assert_eq!(projection.height(point).to_bits(), expected.2.to_bits());
            }
        }
    }

    #[test]
    fn delayed_angles_preserve_winding_acceptance() {
        fn reference(axis: ThreadAxis, points: &[V3]) -> bool {
            if points.len() < 17 {
                return false;
            }
            let (radius, mut angle, z0) = axis.reference_polar(points[0]);
            if radius <= 0.0 {
                return false;
            }
            let first = angle;
            let mut previous_z = z0;
            let dz = axis.reference_polar(*points.last().unwrap()).2 - z0;
            if dz.abs() < 0.1 {
                return false;
            }
            for &point in &points[1..] {
                let (r, a, z) = axis.reference_polar(point);
                if (r - radius).abs() > 0.1 * radius || (z - previous_z) * dz < -1e-7 {
                    return false;
                }
                angle = geom::nearest_turn(a, angle);
                previous_z = z;
            }
            (angle - first).abs() >= TAU * 1.99
        }
        for lead in [-2.0, 0.0, 0.001, 2.0] {
            for turns in [0.2, 0.5, 1.98, 1.99, 2.0, 4.0] {
                for drift in [0.0, 0.002, 0.2] {
                    let mut points = helix_points(lead, turns);
                    for p in &mut points {
                        p[0] *= 1.0 + drift * p[2];
                        p[1] *= 1.0 + drift * p[2];
                    }
                    for length in [0, 1, 16, 17, 200, 401] {
                        let samples = &points[..length];
                        assert_eq!(
                            sustained_winding(axis(), samples),
                            reference(axis(), samples)
                        );
                    }
                    points.reverse();
                    assert_eq!(
                        sustained_winding(axis(), &points),
                        reference(axis(), &points)
                    );
                }
            }
        }
    }

    #[test]
    fn owner_indices_preserve_global_scan_membership_and_order() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/modelled_threads/left_two_start.step.gz");
        let mut part = crate::read_step_file(&path).unwrap();
        for synthetic_owners in [false, true] {
            if synthetic_owners {
                // Exercise index membership for unowned and shared edges. Geometry
                // and adjacency stay fixed; these labels are not a recognition input.
                for (i, face) in part.faces.iter_mut().enumerate() {
                    face.solid = [None, Some(0), Some(1)][i % 3];
                }
            }
            let indexed = eligible_edges(&part);
            assert!(!indexed.is_empty());
            for solid in [None, Some(0), Some(1), Some(usize::MAX)] {
                let expected: Vec<_> = part
                    .edges
                    .iter()
                    .enumerate()
                    .filter_map(|(e, edge)| {
                        (edge.samples.len() >= 9
                            && matches!(edge.curve, Curve::Nurbs(_))
                            && part.edge_faces()[e]
                                .iter()
                                .any(|&f| part.faces[f].solid == solid))
                        .then_some(e)
                    })
                    .collect();
                assert_eq!(
                    indexed.get(&solid).map(Vec::as_slice).unwrap_or(&[]),
                    expected
                );
            }

            // Keep the previous global deduplication as an independent reference:
            // approximate equality is not transitive, so order must also match.
            let mut expected: Vec<(Option<usize>, ThreadAxis)> = Vec::new();
            let mut add = |solid, point, direction| {
                let Some(axis) = ThreadAxis::new(point, direction) else {
                    return;
                };
                if !expected.iter().any(|(s, a)| {
                    *s == solid
                        && geom::dist(a.point, axis.point) <= 1e-6
                        && geom::dist(a.direction, axis.direction) <= 1e-8
                }) {
                    expected.push((solid, axis));
                }
            };
            for face in &part.faces {
                match &face.surface {
                    Surface::Cylinder { frame, .. }
                    | Surface::Cone { frame, .. }
                    | Surface::Torus { frame, .. } => add(face.solid, frame.origin, frame.z),
                    _ => {}
                }
                for &(edge, _) in face.loops.iter().flat_map(|l| &l.edges) {
                    if let Curve::Circle { frame, .. } = &part.edges[edge].curve {
                        add(face.solid, frame.origin, frame.z);
                    }
                }
            }
            assert!(!expected.is_empty());
            assert_eq!(
                serde_json::to_string(&axes(&part)).unwrap(),
                serde_json::to_string(&expected).unwrap()
            );
        }
    }

    fn axis() -> ThreadAxis {
        ThreadAxis::new([0.0; 3], [0.0, 0.0, 1.0]).unwrap()
    }

    fn helix_points(lead: f64, turns: f64) -> Vec<V3> {
        (0..=400)
            .map(|i| {
                let a = turns * TAU * i as f64 / 400.0;
                [10.0 * a.cos(), 10.0 * a.sin(), lead * a / TAU]
            })
            .collect()
    }

    #[test]
    fn measurements_survive_traversal_and_axis_reversal() {
        for lead in [-2.0, 2.0] {
            let mut points = helix_points(lead, 4.0);
            let a = fit_helix(axis(), 0, &points).unwrap();
            assert!((a.lead - lead).abs() < 1e-12);
            points.reverse();
            let reversed = fit_helix(axis(), 0, &points).unwrap();
            assert!((reversed.lead - lead).abs() < 1e-12);
            let opposite = ThreadAxis {
                point: [0.0; 3],
                direction: [0.0, 0.0, -1.0],
            };
            assert!((fit_helix(opposite, 0, &points).unwrap().lead - lead).abs() < 1e-12);
            assert!((a.end - a.start - 8.0).abs() < 1e-12);
        }
    }

    #[test]
    fn variable_pitch_and_taper_are_reportable_failed_fits() {
        let mut points = helix_points(2.0, 4.0);
        for p in &mut points {
            p[2] += 0.02 * p[2] * p[2];
        }
        assert!(fit_helix(axis(), 0, &points).is_none());
        assert!(sustained_winding(axis(), &points));
        let mut points = helix_points(2.0, 4.0);
        for p in &mut points {
            let scale = 1.0 + 0.001 * p[2];
            p[0] *= scale;
            p[1] *= scale;
        }
        assert!(fit_helix(axis(), 0, &points).is_none());
        assert!(sustained_winding(axis(), &points));
    }

    #[test]
    fn a_circle_a_longitudinal_spline_and_a_planar_spiral_are_not_helices() {
        let circle = helix_points(0.0, 3.0);
        assert!(fit_helix(axis(), 0, &circle).is_none());
        let line: Vec<_> = (0..=100).map(|i| [10.0, 0.0, i as f64]).collect();
        assert!(fit_helix(axis(), 0, &line).is_none());
        let mut spiral = circle;
        for (i, p) in spiral.iter_mut().enumerate() {
            p[0] *= 1.0 + i as f64 / 400.0;
            p[1] *= 1.0 + i as f64 / 400.0;
        }
        assert!(fit_helix(axis(), 0, &spiral).is_none());
        assert!(!sustained_winding(axis(), &spiral));
    }

    fn sampled_flank(lead: f64, variable: bool) -> Samples {
        let mut points = Vec::new();
        let mut normals = Vec::new();
        for i in 0..=40 {
            for j in 0..=4 {
                let a = 6.0 * PI * i as f64 / 40.0;
                let r = 9.0 + j as f64 / 4.0;
                let slope = 0.6;
                let drift = if variable { 0.001 * a * a } else { 0.0 };
                let p = [r * a.cos(), r * a.sin(), lead * a / TAU + slope * r + drift];
                let dr = [a.cos(), a.sin(), slope];
                let dt = [
                    -r * a.sin(),
                    r * a.cos(),
                    lead / TAU + if variable { 0.002 * a } else { 0.0 },
                ];
                let n = geom::scale(geom::unit(geom::cross(dr, dt)).unwrap(), -1.0);
                points.push(p);
                normals.push((p, n));
            }
        }
        Samples { points, normals }
    }

    #[test]
    fn surface_samples_must_establish_screw_invariance() {
        let good = flank(axis(), 4, &sampled_flank(2.0, false), 2.0).unwrap();
        assert!((good.slope - 0.6).abs() < 1e-12);
        assert_eq!(good.radial_sign, 1.0);
        assert!(flank(axis(), 4, &sampled_flank(2.0, true), 2.0).is_none());
        assert!(flank(axis(), 4, &sampled_flank(2.0, false), 2.1).is_none());
    }

    fn start_flanks(phases: &[f64], positive: bool) -> Vec<Flank> {
        phases
            .iter()
            .map(|&phase| Flank {
                face: 0,
                slope: if positive { 0.6 } else { -0.6 },
                phase,
                radial_sign: 1.0,
            })
            .collect()
    }

    #[test]
    fn starts_require_both_flanks_and_equal_spacing_including_wraparound() {
        let mut flanks = start_flanks(&[0.99, 1.99, 2.99, 0.9901], true);
        flanks.extend(start_flanks(&[0.2, 1.2, 2.2], false));
        assert_eq!(count_starts(&flanks, 3.0), Ok(3));
        flanks.pop();
        assert!(count_starts(&flanks, 3.0).is_err());
        let mut irregular = start_flanks(&[0.1, 1.2, 2.1], true);
        irregular.extend(start_flanks(&[0.2, 1.2, 2.2], false));
        assert_eq!(
            count_starts(&irregular, 3.0),
            Err("inconsistent_start_spacing")
        );
    }
}
