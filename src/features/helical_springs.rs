//! Complete, constant round-wire helical tubes. A spring claim requires the whole solid,
//! a connected unbranched tube chain, and one helix fitted to every centreline station.
use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::{PI, TAU};

use serde::Serialize;

use super::Context;
use super::evidence::{self, EvidenceStatus, Occurrence};
use super::modelled_threads::ThreadAxis;
use crate::kernel::brep::Part;
use crate::kernel::geom::{self, Surface, V3};

/// Measurements in mm, coils in turns. `axis.point` is the point on the axis nearest
/// the file origin. `start`/`end` bound the physical wire along its unit direction.
/// Unknown active coils are deliberately null: geometry does not establish seating/load.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HelicalSpring {
    pub axis: ThreadAxis,
    pub hand: &'static str,
    pub wire_diameter: f64,
    pub mean_diameter: f64,
    pub outer_diameter: f64,
    pub inner_diameter: f64,
    pub free_length: f64,
    pub total_coils: f64,
    pub total_coils_basis: &'static str,
    pub active_coils: Option<f64>,
    pub active_coils_basis: &'static str,
    pub pitch: f64,
    pub end_type: &'static str,
    pub start: f64,
    pub end: f64,
    pub tolerances: SpringTolerances,
    pub faces: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SpringTolerances {
    pub wire_radius: f64,
    pub helix_radius: f64,
    pub axial: f64,
    pub maximum_segment_angle_degrees: f64,
    pub tangent_angle_radians: f64,
}

fn tolerance(r: f64) -> f64 {
    0.001_f64.max(r.abs() * 0.001)
}

#[derive(Clone)]
struct Tube {
    faces: Vec<usize>,
    points: Vec<V3>,
    tangents: Vec<V3>,
    radius: f64,
    line: bool,
}

fn same_line(a: V3, d: V3, b: V3, e: V3) -> bool {
    geom::norm(geom::cross(d, e)) < 1e-7 && geom::norm(geom::cross(geom::sub(a, b), d)) < 1e-6
}

/// Recover round cross sections from either parameter direction. Opposite parameter
/// stations must be diametrically opposed; all sampled points must lie on that circle.
/// This works for rational swept BSplines as well as full minor circles on tori.
fn curved_tube(part: &Part, face: usize) -> Option<Tube> {
    let surface = &part.faces[face].surface;
    let (u0, u1, v0, v1) = part.uv_bounds(face)?;
    for circle_u in [true, false] {
        let at = |s: f64, t: f64| {
            if circle_u {
                surface.value(u0 + (u1 - u0) * s, v0 + (v1 - v0) * t)
            } else {
                surface.value(u0 + (u1 - u0) * t, v0 + (v1 - v0) * s)
            }
        };
        if geom::dist(at(0., 0.5), at(1., 0.5)) > 1e-5 {
            continue;
        }
        let mut points = Vec::new();
        let mut tangents = Vec::new();
        let mut radius = 0.;
        let mut valid = true;
        // At least 32 stations per turn even for long swept supports. STEP spline knot
        // count is also a lower bound, avoiding aliasing a many-turn spline to a few turns.
        let stations = match surface {
            Surface::Freeform { surface, .. } => {
                let n = if circle_u {
                    surface.knots_v.len()
                } else {
                    surface.knots_u.len()
                };
                (n * 8).clamp(128, 8192)
            }
            _ => 32,
        };
        for i in 0..=stations {
            let t = i as f64 / stations as f64;
            let c = geom::scale(geom::add(at(0., t), at(0.5, t)), 0.5);
            let x = geom::sub(at(0., t), c);
            let y = geom::sub(at(0.25, t), c);
            let Some(normal) = geom::unit(geom::cross(x, y)) else {
                valid = false;
                break;
            };
            let r = geom::norm(x);
            if i == 0 {
                radius = r;
            }
            if radius <= 0. || (r - radius).abs() > tolerance(radius) {
                valid = false;
                break;
            }
            for k in 0..16 {
                let q = geom::sub(at(k as f64 / 16., t), c);
                if (geom::norm(q) - radius).abs() > tolerance(radius)
                    || geom::dot(q, normal).abs() > tolerance(radius)
                {
                    valid = false;
                    break;
                }
            }
            if !valid {
                break;
            }
            points.push(c);
            tangents.push(normal);
        }
        if valid {
            return Some(Tube {
                faces: vec![face],
                points,
                tangents,
                radius,
                line: false,
            });
        }
    }
    None
}

fn tubes(part: &Part, faces: &[usize]) -> Option<Vec<Tube>> {
    let mut tubes: Vec<Tube> = Vec::new();
    for &f in faces {
        match &part.faces[f].surface {
            Surface::Cylinder { frame, radius } => {
                if part.faces[f].reversed {
                    return None;
                }
                let (_, _, lo, hi) = part.uv_bounds(f)?;
                let a = frame.to_world([0., 0., lo]);
                let b = frame.to_world([0., 0., hi]);
                if let Some(tube) = tubes
                    .iter_mut()
                    .find(|t| t.line && same_line(t.points[0], t.tangents[0], a, frame.z))
                {
                    if (tube.radius - radius).abs() > tolerance(*radius) {
                        return None;
                    }
                    let d = tube.tangents[0];
                    for p in [a, b] {
                        if geom::dot(geom::sub(p, tube.points[0]), d) < 0. {
                            tube.points[0] = p;
                        }
                        if geom::dot(geom::sub(p, tube.points[1]), d) > 0. {
                            tube.points[1] = p;
                        }
                    }
                    tube.faces.push(f);
                } else {
                    tubes.push(Tube {
                        faces: vec![f],
                        points: vec![a, b],
                        tangents: vec![frame.z; 2],
                        radius: *radius,
                        line: true,
                    });
                }
            }
            Surface::Torus { .. } | Surface::Freeform { .. } => tubes.push(curved_tube(part, f)?),
            Surface::Plane { .. } => (),
            _ => return None,
        }
    }
    let radius = tubes.first()?.radius;
    if tubes
        .iter()
        .any(|t| (t.radius - radius).abs() > tolerance(radius))
    {
        return None;
    }
    // Every planar patch must cap a tube, including the tiny exposed caps of overlapping
    // cylinder approximations. A supporting plate or a shaft cannot be swallowed.
    for &f in faces {
        if let Surface::Plane { frame } = &part.faces[f].surface {
            let cap = tubes.iter().any(|t| {
                [0, t.points.len() - 1].into_iter().any(|i| {
                    geom::norm(geom::cross(frame.z, t.tangents[i])) < 1e-5
                        && geom::dot(geom::sub(frame.origin, t.points[i]), t.tangents[i]).abs()
                            < tolerance(radius)
                        && part.face_edges(f).into_iter().all(|e| {
                            part.edges[e]
                                .samples
                                .iter()
                                .all(|&p| geom::dist(p, t.points[i]) <= radius + tolerance(radius))
                        })
                })
            });
            if !cap {
                return None;
            }
        }
    }
    Some(tubes)
}

/// Topological adjacency, reduced over coaxial cylinder patches. No spatial pattern of
/// disconnected pins qualifies; every tube must be in this one open, nonbranching chain.
fn order(part: &Part, tubes: &[Tube]) -> Option<Vec<(usize, bool)>> {
    if tubes.len() == 1 {
        return Some(vec![(0, false)]);
    }
    let owners: BTreeMap<_, _> = tubes
        .iter()
        .enumerate()
        .flat_map(|(i, t)| t.faces.iter().map(move |&f| (f, i)))
        .collect();
    let mut neighbors = vec![BTreeSet::new(); tubes.len()];
    for (&f, &i) in &owners {
        for e in part.face_edges(f) {
            for neighbor in &part.edge_faces()[e] {
                if let Some(&j) = owners.get(neighbor)
                    && i != j
                {
                    neighbors[i].insert(j);
                }
            }
        }
    }
    if neighbors.iter().any(|n| n.is_empty() || n.len() > 2) {
        return None;
    }
    let ends: Vec<_> = neighbors
        .iter()
        .enumerate()
        .filter(|(_, n)| n.len() == 1)
        .map(|(i, _)| i)
        .collect();
    if ends.len() != 2 {
        return None;
    }
    let mut result = Vec::new();
    let mut prev = None;
    let mut current = ends[0];
    loop {
        let next = neighbors[current]
            .iter()
            .copied()
            .find(|j| Some(*j) != prev);
        let tube = &tubes[current];
        let other = next.or(prev)?;
        let target = &tubes[other];
        let distance = |p| {
            target
                .points
                .iter()
                .map(|&q| geom::dist(p, q))
                .fold(f64::INFINITY, f64::min)
        };
        let reverse = if next.is_some() {
            distance(tube.points[0]) < distance(*tube.points.last()?)
        } else {
            distance(tube.points[0]) > distance(*tube.points.last()?)
        };
        result.push((current, reverse));
        if result.len() > tubes.len() {
            return None;
        }
        let Some(next) = next else { break };
        prev = Some(current);
        current = next;
    }
    (result.len() == tubes.len()).then_some(result)
}

fn line_joint(a: &Tube, b: &Tube) -> Option<V3> {
    let (p, d, q, e) = (a.points[0], a.tangents[0], b.points[0], b.tangents[0]);
    let w = geom::sub(q, p);
    let dot = geom::dot(d, e);
    let det = 1. - dot * dot;
    if det < 1e-8 {
        return None;
    }
    let s = (geom::dot(w, d) - dot * geom::dot(w, e)) / det;
    let t = (dot * geom::dot(w, d) - geom::dot(w, e)) / det;
    let (x, y) = (
        geom::add(p, geom::scale(d, s)),
        geom::add(q, geom::scale(e, t)),
    );
    (geom::dist(x, y) <= tolerance(a.radius)).then_some(geom::scale(geom::add(x, y), 0.5))
}

struct Fit {
    axis: ThreadAxis,
    radius: f64,
    pitch: f64,
    angles: Vec<f64>,
    axial_intercept: f64,
}

fn fit(points: &[V3], tangents: &[V3]) -> Option<Fit> {
    if points.len() < 9 || tangents.len() < 9 {
        return None;
    }
    let mean = geom::scale(
        tangents.iter().copied().fold([0.; 3], geom::add),
        1. / tangents.len() as f64,
    );
    let mut scatter = [[0.; 3]; 3];
    for &t in tangents {
        let q = geom::sub(t, mean);
        for i in 0..3 {
            for j in 0..3 {
                scatter[i][j] += q[i] * q[j];
            }
        }
    }
    let (values, vectors) = eigen(scatter);
    if values[1] < tangents.len() as f64 * 0.01 {
        return None;
    }
    let mut d = vectors[0];
    if d[geom::dominant_axis_preferring_z(d)] < 0. {
        d = geom::scale(d, -1.);
    }
    let x = geom::default_x_axis(d);
    let y = geom::cross(d, x);
    let origin = points[0];
    let mut matrix = [[0.; 3]; 3];
    let mut rhs = [0.; 3];
    for &p in points {
        let p = geom::sub(p, origin);
        let (u, v) = (geom::dot(p, x), geom::dot(p, y));
        let a = [u, v, 1.];
        for i in 0..3 {
            rhs[i] += a[i] * (u * u + v * v);
            for j in 0..3 {
                matrix[i][j] += a[i] * a[j];
            }
        }
    }
    let [cx, cy, _] = solve3(matrix, rhs)?;
    let center = geom::add(
        origin,
        geom::add(geom::scale(x, cx / 2.), geom::scale(y, cy / 2.)),
    );
    let center = geom::sub(center, geom::scale(d, geom::dot(center, d)));
    let mut angles = Vec::new();
    let mut heights = Vec::new();
    let mut radii = Vec::new();
    for &p in points {
        let q = geom::sub(p, center);
        let (u, v) = (geom::dot(q, x), geom::dot(q, y));
        let angle = v.atan2(u);
        let angle = angles
            .last()
            .map_or(angle, |&a| geom::nearest_turn(angle, a));
        angles.push(angle);
        heights.push(geom::dot(q, d));
        radii.push(u.hypot(v));
    }
    let n = points.len() as f64;
    let radius = radii.iter().sum::<f64>() / n;
    let am = angles.iter().sum::<f64>() / n;
    let zm = heights.iter().sum::<f64>() / n;
    let slope = angles
        .iter()
        .zip(&heights)
        .map(|(a, z)| (a - am) * (z - zm))
        .sum::<f64>()
        / angles.iter().map(|a| (a - am).powi(2)).sum::<f64>();
    let pitch = slope * TAU;
    let span = angles.last()? - angles[0];
    if !pitch.is_finite()
        || span.abs() < TAU * 2. - 1e-5
        || pitch.abs() < 0.001
        || radii.iter().any(|r| (r - radius).abs() > tolerance(radius))
        || angles
            .iter()
            .zip(&heights)
            .any(|(a, z)| (z - zm - slope * (a - am)).abs() > tolerance(pitch))
        || angles
            .windows(2)
            .any(|w| (w[1] - w[0]) * span <= 0. || (w[1] - w[0]).abs() > PI / 6. + 1e-6)
    {
        return None;
    }
    Some(Fit {
        axis: ThreadAxis {
            point: center,
            direction: d,
        },
        radius,
        pitch,
        angles,
        axial_intercept: zm - slope * am,
    })
}

fn recognise_solid(part: &Part, faces: &[usize]) -> Option<HelicalSpring> {
    // Avoid expensive spline/UV work on general parts. Only entire round-wire tube solids
    // are eligible; threads, bosses on plates and mixed mechanical parts fail here.
    if faces.iter().any(|&f| {
        !matches!(
            part.faces[f].surface,
            Surface::Plane { .. }
                | Surface::Cylinder { .. }
                | Surface::Torus { .. }
                | Surface::Freeform { .. }
        )
    }) {
        return None;
    }
    let tubes = tubes(part, faces)?;
    let sequence = order(part, &tubes)?;
    let mut ordered: Vec<_> = sequence
        .iter()
        .map(|&(i, reverse)| {
            let mut t = tubes[i].clone();
            if reverse {
                t.points.reverse();
                t.tangents.reverse();
            }
            let direction = geom::sub(*t.points.last().unwrap(), t.points[0]);
            for d in &mut t.tangents {
                if geom::dot(*d, direction) < 0. {
                    *d = geom::scale(*d, -1.);
                }
            }
            t
        })
        .collect();
    // Curved tubes may span whole turns; orient each section by the local centreline,
    // rather than by the chord joining the tube's ends.
    for t in &mut ordered {
        if !t.line {
            for i in 0..t.points.len() {
                let direction = geom::sub(
                    t.points[(i + 1).min(t.points.len() - 1)],
                    t.points[i.saturating_sub(1)],
                );
                if geom::dot(t.tangents[i], direction) < 0. {
                    t.tangents[i] = geom::scale(t.tangents[i], -1.);
                }
            }
        }
    }
    let radius = ordered[0].radius;
    let mut points = Vec::new();
    let mut tangents = Vec::new();
    for (i, t) in ordered.iter().enumerate() {
        tangents.extend(&t.tangents);
        if !t.line {
            points.extend(&t.points);
        }
        if let Some(next) = ordered.get(i + 1) {
            let joint = if t.line && next.line {
                line_joint(t, next)?
            } else {
                let a = *t.points.last()?;
                let b = next.points[0];
                if geom::dist(a, b) > tolerance(radius) {
                    return None;
                }
                geom::scale(geom::add(a, b), 0.5)
            };
            if points.last().is_none_or(|&p| geom::dist(p, joint) > 1e-7) {
                points.push(joint);
            }
        }
    }
    // Remove duplicate stations at curved-piece interfaces.
    points.dedup_by(|a, b| geom::dist(*a, *b) < 1e-7);
    let fit = fit(&points, &tangents)?;
    if fit.radius <= radius * 2. || fit.pitch.abs() <= radius * 2. + tolerance(radius) {
        return None;
    }
    let polar = |p: V3, near: f64| {
        let q = geom::sub(p, fit.axis.point);
        let x = geom::default_x_axis(fit.axis.direction);
        let y = geom::cross(fit.axis.direction, x);
        geom::nearest_turn(geom::dot(q, y).atan2(geom::dot(q, x)), near)
    };
    let mut first = fit.angles[0];
    let mut last = *fit.angles.last()?;
    // Check every straight cylinder's full interval against consecutive helix joints.
    // Chord extensions are allowed only up to one wire radius at either end, as in
    // overlapping-cylinder approximations; arbitrary tangent pins cannot pass this.
    for (i, t) in ordered.iter().enumerate() {
        if !t.line {
            for (&p, &normal) in t.points.iter().zip(&t.tangents) {
                let q = geom::sub(p, fit.axis.point);
                let expected = geom::unit(geom::add(
                    geom::cross(fit.axis.direction, q),
                    geom::scale(fit.axis.direction, fit.pitch / TAU),
                ))?;
                if geom::norm(geom::cross(expected, normal)) > 0.01 {
                    return None;
                }
            }
            continue;
        }
        let a = if i == 0 {
            None
        } else if ordered[i - 1].line {
            Some(line_joint(&ordered[i - 1], t)?)
        } else {
            Some(ordered[i - 1].points.last().copied()?)
        };
        let b = if i + 1 == ordered.len() {
            None
        } else if ordered[i + 1].line {
            Some(line_joint(t, &ordered[i + 1])?)
        } else {
            Some(ordered[i + 1].points[0])
        };
        let d = geom::unit(geom::sub(t.points[1], t.points[0]))?;
        // An endpoint without a neighbour is constrained by the same local chord
        // inferred from its neighbouring joint, including its measured cap extension.
        let length = geom::dist(t.points[0], t.points[1]);
        if length > fit.radius * PI / 6. + 2. * radius {
            return None;
        }
        for p in [a, b].into_iter().flatten() {
            let along = geom::dot(geom::sub(p, t.points[0]), d);
            if along < -tolerance(radius) || along > length + tolerance(radius) {
                return None;
            }
        }
        let target = geom::scale(geom::add(t.points[0], t.points[1]), 0.5);
        let q = geom::sub(target, fit.axis.point);
        let z = geom::dot(q, fit.axis.direction);
        let radial = geom::sub(q, geom::scale(fit.axis.direction, z));
        let az = geom::dot(d, fit.axis.direction);
        let horizontal = geom::norm(geom::cross(d, fit.axis.direction));
        let tangent = geom::scale(geom::cross(fit.axis.direction, radial), fit.pitch.signum());
        // A helix chord is perpendicular to its midpoint radial vector, and its
        // axial rise agrees with the fitted pitch (chord, rather than arc, correction).
        if geom::dot(d, geom::unit(tangent)?).abs() < 0.9
            || geom::dot(radial, d).abs() > tolerance(fit.radius)
        {
            return None;
        }
        let transverse = geom::sub(d, geom::scale(fit.axis.direction, az));
        let q0 = geom::sub(t.points[0], fit.axis.point);
        let radial0 = geom::sub(
            q0,
            geom::scale(fit.axis.direction, geom::dot(q0, fit.axis.direction)),
        );
        let aa = geom::dot(transverse, transverse);
        let bb = geom::dot(radial0, transverse);
        let cc = geom::dot(radial0, radial0) - fit.radius * fit.radius;
        let discriminant = bb * bb - aa * cc;
        if discriminant <= 0. {
            return None;
        }
        let (s0, s1) = (
            (-bb - discriminant.sqrt()) / aa,
            (-bb + discriminant.sqrt()) / aa,
        );
        if s0.abs() > radius + tolerance(radius) || (length - s1).abs() > radius + tolerance(radius)
        {
            return None;
        }
        // Both circle intersections, including the two terminal ones with no neighbour,
        // must lie on this same fitted helix. A reversed-pitch terminal pin cannot pass
        // by agreeing at just its one joint with the rest of the chain.
        for station in [s0, s1] {
            let p = geom::add(t.points[0], geom::scale(d, station));
            let height = geom::dot(p, fit.axis.direction);
            let angle = polar(p, (height - fit.axial_intercept) * TAU / fit.pitch);
            if (height - fit.axial_intercept - fit.pitch * angle / TAU).abs() > tolerance(fit.pitch)
            {
                return None;
            }
        }
        let angle = 2. * (geom::norm(radial) / fit.radius).clamp(-1., 1.).acos();
        if angle > PI / 6. + 1e-6 {
            return None;
        }
        let expected = fit.pitch.abs() / TAU
            * if angle < 1e-6 {
                1. / fit.radius
            } else {
                angle / (2. * fit.radius * (angle / 2.).sin())
            };
        if (az.abs() / horizontal - expected).abs() > 0.001 {
            return None;
        }
        if i == 0 {
            first = polar(t.points[0], first);
        }
        if i + 1 == ordered.len() {
            last = polar(t.points[1], last);
        }
    }
    // Tight physical axial bounds, including caps and the swept section's axial extent.
    let mut start = f64::INFINITY;
    let mut end = f64::NEG_INFINITY;
    for &f in faces {
        let (lo, hi) = part.extent_along(&[f], fit.axis.direction);
        start = start.min(lo);
        end = end.max(hi);
    }
    // Heights are measured from axis.point, whose axial coordinate is zero.
    if !start.is_finite() || !end.is_finite() {
        return None;
    }
    let mut claimed = faces.to_vec();
    claimed.sort_unstable();
    Some(HelicalSpring {
        axis: fit.axis,
        hand: if fit.pitch > 0. { "right" } else { "left" },
        wire_diameter: 2. * radius,
        mean_diameter: 2. * fit.radius,
        outer_diameter: 2. * (fit.radius + radius),
        inner_diameter: 2. * (fit.radius - radius),
        free_length: end - start,
        total_coils: (last - first).abs() / TAU,
        total_coils_basis: "centreline angular sweep including terminal extensions",
        active_coils: None,
        active_coils_basis: "unknown: seating and inactive end coils are not established",
        pitch: fit.pitch.abs(),
        end_type: "plain",
        start,
        end,
        tolerances: SpringTolerances {
            wire_radius: tolerance(radius),
            helix_radius: tolerance(fit.radius),
            axial: tolerance(fit.pitch),
            maximum_segment_angle_degrees: 30.,
            tangent_angle_radians: 0.01,
        },
        faces: claimed,
    })
}

internal_entry! {
    /// Recognise each complete valid round-wire solid independently.
    pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<HelicalSpring>> {
        ctx.part.solids.iter().filter_map(|solid| {
            let record = recognise_solid(ctx.part, &solid.faces)?;
            // Whole-solid ownership remains strict even during a degraded retry.
            if evidence::strict_status(ctx.part, &record.faces, &[]) != EvidenceStatus::Verified {
                return None;
            }
            Some(Occurrence {
                defining: record.faces.clone(),
                context: Vec::new(),
                record,
            })
        }).collect()
    }
}

/// Eigenvalues (ascending) and unit eigenvectors of a symmetric 3×3 matrix, by Jacobi rotations.
fn eigen(mut a: [[f64; 3]; 3]) -> ([f64; 3], [V3; 3]) {
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..50 {
        let (mut p, mut q, mut big) = (0, 1, 0.0);
        for (i, row) in a.iter().enumerate() {
            for (j, value) in row.iter().enumerate().skip(i + 1) {
                if value.abs() > big {
                    (p, q, big) = (i, j, value.abs());
                }
            }
        }
        if big <= 1e-300 {
            break;
        }
        let theta = 0.5 * (a[q][q] - a[p][p]) / a[p][q];
        let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
        let t = if theta == 0.0 { 1.0 } else { t };
        let (c, s) = (1.0 / (t * t + 1.0).sqrt(), t / (t * t + 1.0).sqrt());
        for row in &mut a {
            let (akp, akq) = (row[p], row[q]);
            row[p] = c * akp - s * akq;
            row[q] = s * akp + c * akq;
        }
        let (row_p, row_q) = (a[p], a[q]);
        for (k, (&apk, aqk)) in row_p.iter().zip(row_q).enumerate() {
            a[p][k] = c * apk - s * aqk;
            a[q][k] = s * apk + c * aqk;
        }
        for row in &mut v {
            let (vp, vq) = (row[p], row[q]);
            row[p] = c * vp - s * vq;
            row[q] = s * vp + c * vq;
        }
    }
    let mut order = [0, 1, 2];
    order.sort_by(|&i, &j| a[i][i].total_cmp(&a[j][j]));
    let values = order.map(|i| a[i][i]);
    let vectors = order.map(|i| geom::unit([v[0][i], v[1][i], v[2][i]]).unwrap_or([0.0, 0.0, 1.0]));
    (values, vectors)
}

fn solve3(a: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let det = |m: [[f64; 3]; 3]| geom::dot(m[0], geom::cross(m[1], m[2]));
    let d = det(a);
    if d.abs() <= 1e-300 {
        return None;
    }
    Some([0, 1, 2].map(|k| {
        let mut m = a;
        for i in 0..3 {
            m[i][k] = b[i];
        }
        det(m) / d
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn helix() -> (Vec<V3>, Vec<V3>) {
        (0..=96)
            .map(|i| {
                let a = i as f64 * TAU / 24.;
                (
                    [12. * a.cos(), 12. * a.sin(), 4. * a / TAU],
                    geom::unit([-12. * a.sin(), 12. * a.cos(), 4. / TAU]).unwrap(),
                )
            })
            .unzip()
    }
    #[test]
    fn whole_chain_fit_refuses_local_and_accumulated_drift() {
        let (points, tangents) = helix();
        assert!(fit(&points, &tangents).is_some());
        let mut varying = points.clone();
        varying[42][0] += 0.2;
        assert!(fit(&varying, &tangents).is_none());
        let varying: Vec<_> = points
            .iter()
            .enumerate()
            .map(|(i, &p)| [p[0], p[1], p[2] + 0.002 * (i as f64).powi(2)])
            .collect();
        assert!(fit(&varying, &tangents).is_none());
        let varying: Vec<_> = points
            .iter()
            .enumerate()
            .map(|(i, &p)| geom::add(p, [0.003 * (i as f64).powi(2), 0., 0.]))
            .collect();
        assert!(fit(&varying, &tangents).is_none());
        let flat: Vec<_> = points.iter().map(|&p| [p[0], p[1], 0.]).collect();
        assert!(fit(&flat, &tangents).is_none());
        let mut reversed = points;
        reversed.swap(40, 41);
        assert!(fit(&reversed, &tangents).is_none());
    }
}
