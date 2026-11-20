//! Coaxial cylinder segments read at their ends (`quiddity._cylinder_stacks`): which faces lie
//! beyond an axial end, and whether that end is open, flat, a drill point, or unknown. Shared by
//! holes (internal segments) and bosses (external ones).

use super::cylinders::{STACK_GAP_FRAC, Segment, axis_point_at};
use super::policy;
use super::probes::probe_samples;
use crate::kernel::brep::Part;
use crate::kernel::geom::{self, Surface, V3};
use crate::kernel::py;

/// What lies beyond one axial end of a segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Open,
    Flat,
    DrillPoint,
    Unknown,
}

internal_entry! {
/// The faces beyond one axial end of a segment: partners across the segment's edges that lie at
/// that end (within a margin for lips dipping off the end plane), nearest first
/// (`_end_partners`).
pub fn end_partners(part: &Part, seg: &Segment, s_end: f64) -> Vec<usize> {
    ranked_end_partners(part, seg, s_end)
        .into_iter()
        .map(|(_, face)| face)
        .collect()
}
}

fn ranked_end_partners(part: &Part, seg: &Segment, s_end: f64) -> Vec<(f64, usize)> {
    let d = seg.direction;
    let margin = policy::length_tol(seg.diameter, STACK_GAP_FRAC)
        .max((0.45 * (seg.s_hi - seg.s_lo)).min(0.5 * seg.diameter));
    let edge_faces = part.edge_faces();
    // (distance, first-seen order, face)
    let mut ranked: Vec<(f64, usize, usize)> = Vec::new();
    for &face in &seg.faces {
        for edge in part.face_edges(face) {
            let e = &part.edges[edge];
            let distance = std::iter::once(e.midpoint())
                .chain(e.vertex_points())
                .map(|p| (geom::dot(p, d) - s_end).abs())
                .fold(f64::NEG_INFINITY, f64::max);
            if distance > margin {
                continue;
            }
            for &partner in &edge_faces[edge] {
                if seg.faces.contains(&partner) {
                    continue;
                }
                match ranked.iter_mut().find(|r| r.2 == partner) {
                    None => {
                        let order = ranked.len();
                        ranked.push((distance, order, partner));
                    }
                    Some(r) if distance < r.0 => r.0 = distance,
                    Some(_) => {}
                }
            }
        }
    }
    ranked.sort_by(|a, b| py::order(a.0, b.0).then(a.1.cmp(&b.1)));
    ranked.into_iter().map(|r| (r.0, r.2)).collect()
}

internal_entry! {
/// Classify one axial end of a segment from the face beyond it (`_classify_end`), with the
/// faces that close it (its `terminal_faces`). Planes, cones and tori decide; a curved wall is a
/// weak signal that counts only when nothing decides.
pub fn classify_end(part: &Part, seg: &Segment, s_end: f64, hi_end: bool) -> (End, Vec<usize>) {
    let mut strong = Vec::new();
    let mut weak = Vec::new();
    for (distance, partner) in ranked_end_partners(part, seg, s_end) {
        if let Some((state, faces, is_strong)) = partner_state(part, seg, s_end, hi_end, partner) {
            let target = if is_strong { &mut strong } else { &mut weak };
            target.push((distance, state, faces));
        }
    }
    // A plane only closes a bore it actually lies under. Slivers of plane left at the rim of
    // an exit into a curved face (a tunnel, a crossing cavity) are not a floor; with them set
    // aside, the curved partner that is left reads the end as open. Without such a partner
    // (a floor with a smaller hole continuing from it) the plane stays the verdict.
    if !seg.external
        && weak.iter().any(|(_, state, _)| *state == End::Open)
        && !strong.is_empty()
        && floor_misses_axis(part, seg, s_end, &strong)
    {
        strong.retain(|(_, state, faces)| !(*state == End::Flat && is_plane_floor(part, faces)));
    }
    let answers = if strong.is_empty() { weak } else { strong };
    let Some((nearest, state, _)) = answers.first() else {
        return (End::Unknown, vec![]);
    };
    let mut terminal = Vec::new();
    for (distance, other, faces) in &answers {
        if distance - nearest > geom::COORD_FLOOR {
            break;
        }
        if other != state {
            return (End::Unknown, vec![]);
        }
        terminal.extend(faces);
    }
    terminal.sort_unstable();
    terminal.dedup();
    (*state, terminal)
}
}

fn is_plane_floor(part: &Part, faces: &[usize]) -> bool {
    !faces.is_empty()
        && faces
            .iter()
            .all(|&f| matches!(part.faces[f].surface, Surface::Plane { .. }))
}

/// Whether the plane faces of this end's floor leave the bore open: no plane face lies under
/// the bore axis at the end (that axis point, carried along the axis onto each plane, is outside
/// every face) AND together they cover less than half the bore's own section. Slivers left at
/// the rim of an exit into a curved wall satisfy both. A floor cut by a thin passage across
/// the axis, or one with a strip left across it, fails one and stays a floor. Faces without a
/// domain never qualify, so existing readings stay.
fn floor_misses_axis(
    part: &Part,
    seg: &Segment,
    s_end: f64,
    strong: &[(f64, End, Vec<usize>)],
) -> bool {
    let mut floors: Vec<usize> = strong
        .iter()
        .filter(|(_, state, faces)| *state == End::Flat && is_plane_floor(part, faces))
        .flat_map(|(_, _, faces)| faces.iter().copied())
        .collect();
    floors.sort_unstable();
    floors.dedup();
    if floors.is_empty() || floors.iter().any(|&f| part.domain(f).is_none()) {
        return false;
    }
    let d = seg.direction;
    let centre = axis_point_at(seg, s_end);
    let covers = |point: V3| {
        floors.iter().any(|&f| {
            let Surface::Plane { frame } = part.faces[f].surface else {
                return false;
            };
            let slope = geom::dot(frame.z, d);
            if slope.abs() < 1e-9 {
                return false;
            }
            // Carry the section point along the axis onto the floor plane.
            let travel = geom::dot(geom::sub(frame.origin, point), frame.z) / slope;
            let on_plane = geom::add(point, geom::scale(d, travel));
            part.faces[f]
                .surface
                .parameters(on_plane, None)
                .and_then(|(pu, pv)| Some(part.domain(f)?.contains(pu, pv)))
                .unwrap_or(false)
        })
    };
    let axis_covered = covers(centre);
    let Some(u) = geom::unit(geom::cross(
        d,
        if d[0].abs() < 0.9 {
            [1., 0., 0.]
        } else {
            [0., 1., 0.]
        },
    )) else {
        return false;
    };
    let v = geom::cross(d, u);
    const RINGS: usize = 16;
    const SECTORS: usize = 64;
    let radius = seg.diameter / 2.;
    let mut covered = 0;
    for ring in 0..RINGS {
        let r = radius * ((ring as f64 + 0.5) / RINGS as f64).sqrt();
        for sector in 0..SECTORS {
            let angle = std::f64::consts::TAU * (sector as f64 + 0.5) / SECTORS as f64;
            let (sin, cos) = angle.sin_cos();
            let point = geom::add(
                centre,
                geom::add(geom::scale(u, r * cos), geom::scale(v, r * sin)),
            );
            if covers(point) {
                covered += 1;
            }
        }
    }
    !axis_covered && (covered as f64) < 0.5 * (RINGS * SECTORS) as f64
}

/// A support's verdict; `false` means a curved-wall fallback, subordinate to strong evidence.
fn partner_state(
    part: &Part,
    seg: &Segment,
    s_end: f64,
    hi_end: bool,
    partner: usize,
) -> Option<(End, Vec<usize>, bool)> {
    let d = seg.direction;
    let e_sign = if hi_end { 1.0 } else { -1.0 };
    let surface = &part.faces[partner].surface;
    match surface {
        Surface::Cone { .. } => {
            let apex = surface.cone_apex()?;
            let outward = (geom::dot(apex, d) - s_end) * e_sign > 0.;
            if seg.external {
                return Some((if outward { End::Open } else { End::Flat }, vec![], true));
            }
            if !outward {
                return Some((End::Open, vec![], true));
            }
            let mut floors = Vec::new();
            for e in part.face_edges(partner) {
                for &n in &part.edge_faces()[e] {
                    if n != partner
                        && !seg.faces.contains(&n)
                        && native_plane_normal(part, n)
                            .is_some_and(|normal| geom::dot(normal, d).abs() > 0.9)
                    {
                        floors.push(n);
                    }
                }
            }
            floors.sort_unstable();
            floors.dedup();
            if floors.is_empty() {
                Some((End::DrillPoint, vec![partner], true))
            } else {
                Some((End::Flat, floors, true))
            }
        }
        Surface::Torus { major, .. } => {
            let curls_in = *major < seg.diameter / 2.;
            // A groove cut into the wall between two cylinders (itself in halves, perhaps),
            // wholly outside the bore diameter, is not a round at the end: it says nothing about
            // whether the bore stops here. Only a torus the old rule would read as closing the
            // end (major radius under the bore radius) is set aside; one with a larger major
            // radius already read the end as open. A round at a mouth or a floor also borders a
            // plane or a cone, so it is not set aside.
            if !seg.external
                && curls_in
                && part.neighbours(partner).iter().all(|&n| {
                    matches!(
                        part.faces[n].surface,
                        Surface::Cylinder { .. } | Surface::Torus { .. }
                    )
                })
                && reach_beyond_bore(part, seg, &[partner]).is_some()
            {
                return None;
            }
            Some((
                if seg.external != curls_in {
                    End::Flat
                } else {
                    End::Open
                },
                vec![],
                true,
            ))
        }
        Surface::Plane { .. } | Surface::Freeform { .. } => {
            let normal = if matches!(surface, Surface::Plane { .. }) {
                native_plane_normal(part, partner)?
            } else {
                if !matches!(part.recovered(partner), Some(Surface::Plane { .. })) {
                    return None;
                }
                let at = probe_samples(part, partner).first()?.point;
                part.outward_at(partner, at)?
            };
            let alignment = py::dot(&normal, &d) * e_sign;
            if alignment < -0.5 {
                Some((End::Flat, vec![partner], true))
            } else if alignment > 0.5 {
                Some((End::Open, vec![partner], true))
            } else {
                None
            }
        }
        Surface::Sphere { .. } => {
            let convex = part.frame_points_outward(partner).unwrap_or(false);
            Some((
                if seg.external == convex {
                    End::Flat
                } else {
                    End::Open
                },
                vec![partner],
                false,
            ))
        }
        Surface::Cylinder { .. } => Some((
            if seg.external { End::Flat } else { End::Open },
            vec![],
            false,
        )),
        _ => None,
    }
}

/// How far beyond the bore radius these faces reach from the bore's axis, when every sampled
/// point and edge point of them lies at or beyond that radius and some point beyond it: faces
/// that widen the bore into the wall and leave no material inside the bore diameter (a
/// groove), as opposed to a round at a floor or a ridge that narrows the bore. `None` for
/// anything else.
pub(super) fn reach_beyond_bore(part: &Part, seg: &Segment, faces: &[usize]) -> Option<f64> {
    let radius = seg.diameter / 2.;
    let tolerance = policy::length_tol(radius, STACK_GAP_FRAC);
    let mut reach: f64 = 0.;
    for &face in faces {
        let edge_points = part.face_edges(face).into_iter().flat_map(|edge| {
            let e = &part.edges[edge];
            std::iter::once(e.midpoint()).chain(e.vertex_points())
        });
        let samples = probe_samples(part, face).into_iter().map(|s| s.point);
        for point in edge_points.chain(samples) {
            let axial = geom::dot(point, seg.direction);
            let off = py::dist(&point, &axis_point_at(seg, axial));
            if off < radius - tolerance {
                return None;
            }
            reach = reach.max(off - radius);
        }
    }
    (reach > tolerance).then_some(reach)
}

/// A native plane's outward normal as the face reports it, not renormalised
/// (`partner.normal_at(partner.center())` on the planes `_cylinder_stacks` reads); `None` for
/// any other surface.
fn native_plane_normal(part: &Part, face: usize) -> Option<V3> {
    match part.faces[face].surface {
        Surface::Plane { .. } => part.face_normal(face, 0.0, 0.0),
        _ => None,
    }
}
