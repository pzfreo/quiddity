//! Complete, open-ended V channels, proved from two rectangular planar walls meeting
//! concavely along the trough, either at a sharp edge or through one concave round tangent
//! to both. All four end edges and both lips must meet exterior planes convexly, as must a
//! round's two ends. No preferred section angle, scale or stock dimensions are assumed.
use serde::Serialize;

use super::Context;
use super::evidence::{self, EvidenceStatus, Occurrence};
use super::graph::normal;
use super::regions::length_tolerance;
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::{self, Curve, Surface, V3};

/// A V channel in millimetres. `at` is the midpoint of the trough edge (of the flanks'
/// intersection where the trough is rounded); the sign of `axis` is a run gauge. `opening`
/// points out of the mouth, perpendicular to its lip plane. `depth` runs from that sharp or
/// virtual trough to the lip plane. `trough_radius` is the radius of a concave round between
/// the flanks, and absent for a sharp trough.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct VChannel {
    pub axis: V3,
    pub opening: V3,
    pub at: V3,
    pub length: f64,
    pub width: f64,
    pub depth: f64,
    pub included_angle_degrees: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trough_radius: Option<f64>,
}

pub(super) struct Wall {
    pub(super) lip: usize,
    pub(super) ends: [usize; 2],
    pub(super) offset: V3,
}

fn has_vertex(part: &Part, edge: usize, vertex: usize) -> bool {
    let (a, b) = part.edges[edge].vertices;
    a == vertex || b == vertex
}

/// Read a genuine rectangular wall, preserving the trough edge's endpoint roles. Vertex
/// identities establish adjacency; equal distances cannot substitute another wall's claim.
pub(super) fn wall(
    part: &Part,
    face: usize,
    trough: usize,
    axis: V3,
    tolerance: f64,
) -> Option<Wall> {
    let f = &part.faces[face];
    if !matches!(f.surface, Surface::Plane { .. })
        || f.loops.len() != 1
        || f.loops[0].edges.len() != 4
    {
        return None;
    }
    let edges = part.face_edges(face);
    if edges.len() != 4
        || edges
            .iter()
            .any(|&e| !matches!(part.edges[e].curve, Curve::Line { .. }))
    {
        return None;
    }
    let t = &part.edges[trough];
    let lips: Vec<_> = edges
        .iter()
        .copied()
        .filter(|&e| {
            e != trough && !has_vertex(part, e, t.vertices.0) && !has_vertex(part, e, t.vertices.1)
        })
        .collect();
    let [lip] = lips[..] else { return None };
    let mut ends = [0; 2];
    let mut offsets = [[0.; 3]; 2];
    for (i, (vertex, point)) in [(t.vertices.0, t.start), (t.vertices.1, t.end)]
        .into_iter()
        .enumerate()
    {
        let candidates: Vec<_> = edges
            .iter()
            .copied()
            .filter(|&e| e != trough && has_vertex(part, e, vertex))
            .collect();
        let [end] = candidates[..] else { return None };
        let e = &part.edges[end];
        let (other, at) = if e.vertices.0 == vertex {
            (e.vertices.1, e.end)
        } else {
            (e.vertices.0, e.start)
        };
        if !has_vertex(part, lip, other) {
            return None;
        }
        ends[i] = end;
        offsets[i] = geom::sub(at, point);
    }
    if geom::dist(offsets[0], offsets[1]) > tolerance
        || geom::dot(offsets[0], axis).abs() > tolerance
        || geom::norm(offsets[0]) <= tolerance
    {
        return None;
    }
    Some(Wall {
        lip,
        ends,
        offset: offsets[0],
    })
}

pub(super) fn exterior(part: &Part, wall: usize, edge: usize, direction: V3) -> Option<usize> {
    let [a, b] = part.edge_faces()[edge][..] else {
        return None;
    };
    let face = if a == wall {
        b
    } else if b == wall {
        a
    } else {
        return None;
    };
    (matches!(part.faces[face].surface, Surface::Plane { .. })
        && geom::dot(normal(part, face)?, direction) >= 1. - 1e-9
        && part.arc(wall, face) == Some(Arc::Convex))
    .then_some(face)
}

fn from_edge(part: &Part, edge: usize) -> Option<Occurrence<VChannel>> {
    let e = &part.edges[edge];
    if !matches!(e.curve, Curve::Line { .. }) || e.is_closed() {
        return None;
    }
    let [a, b] = part.edge_faces()[edge][..] else {
        return None;
    };
    // The cheap boundary gate avoids asking for arcs or geometry on unrelated part edges.
    if [a, b].iter().any(|&f| {
        !matches!(part.faces[f].surface, Surface::Plane { .. })
            || part.faces[f].loops.len() != 1
            || part.faces[f].loops[0].edges.len() != 4
    }) {
        return None;
    }
    if part.arc(a, b) != Some(Arc::Concave) {
        return None;
    }
    let delta = geom::sub(e.end, e.start);
    let length = geom::norm(delta);
    let axis = geom::unit(delta)?;
    let tolerance = length_tolerance(&[length]);
    let (left, right) = (
        wall(part, a, edge, axis, tolerance)?,
        wall(part, b, edge, axis, tolerance)?,
    );
    let at = geom::scale(geom::add(e.start, e.end), 0.5);
    let (left_offset, right_offset) = (left.offset, right.offset);
    channel(
        part,
        [(a, left, left_offset), (b, right, right_offset)],
        axis,
        at,
        length,
        tolerance,
        Vec::new(),
        None,
    )
}

/// A rounded trough: one concave cylinder along the run whose two straight edges each meet a
/// flank tangentially. The flanks are measured from their intersection, the virtual sharp
/// trough the round replaces; both round ends must open onto exterior planes.
fn from_round(part: &Part, face: usize) -> Option<Occurrence<VChannel>> {
    let f = &part.faces[face];
    let Surface::Cylinder { frame, radius } = f.surface else {
        return None;
    };
    if f.loops.len() != 1
        || f.loops[0].edges.len() != 4
        || part.frame_points_outward(face) != Some(false)
    {
        return None;
    }
    let edges = part.face_edges(face);
    let (lines, ends): (Vec<usize>, Vec<usize>) = edges
        .into_iter()
        .partition(|&e| matches!(part.edges[e].curve, Curve::Line { .. }));
    let (&[la, lb], 2) = (&lines[..], ends.len()) else {
        return None;
    };
    let flank = |edge: usize| -> Option<usize> {
        let [x, y] = part.edge_faces()[edge][..] else {
            return None;
        };
        let other = if x == face { y } else { x };
        (other != face
            && matches!(part.faces[other].surface, Surface::Plane { .. })
            && part.arc(face, other) == Some(Arc::Smooth))
        .then_some(other)
    };
    let (a, b) = (flank(la)?, flank(lb)?);
    if a == b {
        return None;
    }
    let (ea, eb) = (&part.edges[la], &part.edges[lb]);
    let delta = geom::sub(ea.end, ea.start);
    let length = geom::norm(delta);
    let axis = geom::unit(delta)?;
    let tolerance = length_tolerance(&[length, radius]);
    if geom::norm(geom::cross(axis, frame.z)) > 1e-9
        || (geom::dist(eb.start, eb.end) - length).abs() > tolerance
    {
        return None;
    }
    let (left, right) = (
        wall(part, a, la, axis, tolerance)?,
        wall(part, b, lb, axis, tolerance)?,
    );
    // Both flank lines in the section through `ea.start`, then their intersection.
    let ta = ea.start;
    let tb = geom::sub(
        eb.start,
        geom::scale(axis, geom::dot(geom::sub(eb.start, ta), axis)),
    );
    let (ua, ub) = (geom::unit(left.offset)?, geom::unit(right.offset)?);
    let normal = geom::cross(ua, ub);
    let denominator = geom::dot(normal, normal);
    if denominator <= 1e-12 {
        return None;
    }
    let between = geom::sub(tb, ta);
    let s = geom::dot(geom::cross(between, ub), normal) / denominator;
    let t = geom::dot(geom::cross(between, ua), normal) / denominator;
    let apex = geom::add(ta, geom::scale(ua, s));
    // The round replaces the sharp trough: each tangent line lies beyond the apex, toward
    // its own lip, and both reach the same apex.
    if s >= -tolerance
        || t >= -tolerance
        || geom::dist(apex, geom::add(tb, geom::scale(ub, t))) > tolerance
    {
        return None;
    }
    // Each flank's offset from the virtual trough to its lip.
    let left_offset = geom::add(geom::sub(ta, apex), left.offset);
    let right_offset = geom::add(geom::sub(tb, apex), right.offset);
    let at = geom::add(apex, geom::scale(delta, 0.5));
    let mut context = vec![face];
    for &end in &ends {
        let e = &part.edges[end];
        let side = geom::dot(geom::sub(e.midpoint(), ta), axis);
        let direction = if side > 0.5 * length {
            axis
        } else {
            geom::scale(axis, -1.)
        };
        context.push(exterior(part, face, end, direction)?);
    }
    channel(
        part,
        [(a, left, left_offset), (b, right, right_offset)],
        axis,
        at,
        length,
        tolerance,
        context,
        Some(radius),
    )
}

/// The channel two flanks prove, each with the offset from the (sharp or virtual) trough to its
/// lip; *context* holds faces a rounded trough already consulted.
#[allow(clippy::too_many_arguments)]
fn channel(
    part: &Part,
    [(a, left, left_offset), (b, right, right_offset)]: [(usize, Wall, V3); 2],
    axis: V3,
    at: V3,
    length: f64,
    tolerance: f64,
    mut context: Vec<usize>,
    trough_radius: Option<f64>,
) -> Option<Occurrence<VChannel>> {
    let width_vector = geom::sub(right_offset, left_offset);
    let width = geom::norm(width_vector);
    let width_axis = geom::unit(width_vector)?;
    let mut opening = geom::unit(geom::cross(axis, width_axis))?;
    if geom::dot(opening, left_offset) < 0. {
        opening = geom::scale(opening, -1.)
    }
    let depth = geom::dot(opening, left_offset);
    if depth <= tolerance
        || (geom::dot(opening, right_offset) - depth).abs() > tolerance
        || geom::dot(left_offset, width_axis) >= -tolerance
        || geom::dot(right_offset, width_axis) <= tolerance
    {
        return None;
    }
    context.extend([
        exterior(part, a, left.lip, opening)?,
        exterior(part, b, right.lip, opening)?,
    ]);
    for (wall, profile) in [(a, &left), (b, &right)] {
        context.push(exterior(
            part,
            wall,
            profile.ends[0],
            geom::scale(axis, -1.),
        )?);
        context.push(exterior(part, wall, profile.ends[1], axis)?);
    }
    context.sort_unstable();
    context.dedup();
    let mut defining = vec![a, b];
    defining.sort_unstable();
    if context.iter().any(|f| defining.contains(f))
        || evidence::strict_status(part, &defining, &context) != EvidenceStatus::Verified
    {
        return None;
    }
    let angle = geom::dot(geom::unit(left_offset)?, geom::unit(right_offset)?)
        .clamp(-1., 1.)
        .acos()
        .to_degrees();
    Some(Occurrence {
        record: VChannel {
            axis,
            opening,
            at,
            length,
            width,
            depth,
            included_angle_degrees: angle,
            trough_radius,
        },
        defining,
        context,
    })
}

internal_entry! {
/// All complete V channels on strictly valid solids. A face shared by competing channels
/// gives neither a defining claim.
pub fn discover(
    ctx: &Context<'_>,
    closed_passages: &[Occurrence<super::passages::SectionPassage>],
) -> Vec<Occurrence<VChannel>> {
    let found: Vec<_> = (0..ctx.part.edges.len())
        .filter_map(|e| from_edge(ctx.part, e))
        .chain((0..ctx.part.faces.len()).filter_map(|f| from_round(ctx.part, f)))
        .collect();
    found.iter()
        // A corner of a proved closed passage belongs to that complete section,
        // not an independent open channel. No passage record changes.
        .filter(|o| !closed_passages.iter().any(|p|
            o.defining.iter().all(|f| p.defining.contains(f))))
        .filter(|o| !found.iter().any(|p| !std::ptr::eq(*o, p)
            && p.defining.iter().any(|f| o.defining.contains(f))))
        .cloned().collect()
}
}

#[cfg(test)]
mod tests {
    use super::super::cylindrical_channels::{Cap, capped_prism};
    use super::*;
    use crate::kernel::step::IDENTITY;

    fn stock() -> Part {
        let points = [
            [0., 0., 0.],
            [30., 0., 0.],
            [30., 20., 0.],
            [20., 20., 0.],
            [15., 10., 0.],
            [10., 20., 0.],
            [0., 20., 0.],
        ];
        capped_prism([0., 0., 1.], &points, Cap::Plane(0.), Cap::Plane(23.)).unwrap()
    }

    #[test]
    fn v_channel_dimensions_claims_and_publication_survive_motion_and_source_permutation() {
        let base = stock();
        let (s, c) = 0.37_f64.sin_cos();
        for motion in [
            IDENTITY,
            [[c, 0., s, 123.456], [0., 1., 0., -78.9], [-s, 0., c, 41.3]],
        ] {
            for reverse in [false, true] {
                let part = base.transformed(&motion).unwrap();
                let part = if reverse {
                    let mut faces = part.faces.clone();
                    faces.reverse();
                    Part::new(faces, part.edges.clone(), part.solids.clone())
                } else {
                    part
                };
                let found = discover(&Context::new(&part), &[]);
                assert_eq!(found.len(), 1);
                let r = &found[0].record;
                assert!((r.length - 23.).abs() < 1e-9);
                assert!((r.width - 10.).abs() < 1e-9);
                assert!((r.depth - 10.).abs() < 1e-9);
                assert!((r.included_angle_degrees - 53.130102354156).abs() < 1e-9);
                let recognition = crate::correspondence::recognise(&part);
                let doc = crate::recognition::document(&recognition, part.faces.len());
                let records: Vec<_> = doc
                    .records
                    .iter()
                    .filter(|r| r.family == "v_channels")
                    .collect();
                assert_eq!(records.len(), 1);
                let at: V3 = serde_json::from_value(records[0].parameters["at"].clone()).unwrap();
                assert!(geom::dist(at, r.at) < 1e-9);
                assert_eq!(records[0].evidence_status, EvidenceStatus::Verified);
            }
        }
    }

    #[test]
    fn a_v_corner_of_a_closed_passage_is_not_a_second_channel() {
        use crate::kernel::brep::{Edge, Face, Loop};
        use crate::kernel::geom::Frame;
        let outer = [
            [-15., -15., 0.],
            [15., -15., 0.],
            [15., 15., 0.],
            [-15., 15., 0.],
        ];
        let inner = [
            [-5., -5., 0.],
            [-5., 5., 0.],
            [-2., 5., 0.],
            [0., 9., 0.],
            [2., 5., 0.],
            [5., 5., 0.],
            [5., -5., 0.],
        ];
        let mut edges = Vec::new();
        let mut loops = Vec::new();
        let mut vertex_base = 0;
        for points in [&outer[..], &inner[..]] {
            let mut boundary = Vec::new();
            for i in 0..points.len() {
                let j = (i + 1) % points.len();
                let (start, end) = (points[i], points[j]);
                boundary.push((edges.len(), true));
                edges.push(Edge {
                    curve: Curve::Line {
                        origin: start,
                        dir: geom::sub(end, start),
                    },
                    start,
                    end,
                    vertices: (vertex_base + i, vertex_base + j),
                    same_sense: true,
                    samples: vec![start, end],
                });
            }
            loops.push(Loop {
                edges: boundary,
                vertex: None,
            });
            vertex_base += points.len();
        }
        let section = Part::new(
            vec![Face {
                surface: Surface::Plane {
                    frame: Frame {
                        origin: [0.; 3],
                        x: [1., 0., 0.],
                        y: [0., 1., 0.],
                        z: [0., 0., 1.],
                    },
                },
                reversed: false,
                loops,
                solid: None,
                pcurves: Vec::new(),
            }],
            edges,
            Vec::new(),
        );
        let part = crate::kernel::sweep::extrude_face(&section, 0, [0.; 3], [0., 0., 23.]).unwrap();
        let ctx = Context::new(&part);
        assert_eq!(discover(&ctx, &[]).len(), 1);
        let passages = super::super::passages::discover(&ctx).unwrap();
        assert_eq!(passages.len(), 1);
        assert!(discover(&ctx, &passages).is_empty());
        let features = super::super::recognise(&part);
        assert!(features.v_channels.is_empty());
        assert!(!features.section_passages.is_empty());
    }

    #[test]
    fn missing_wall_boundary_and_wrong_material_or_ownership_refuse() {
        for fault in 0..3 {
            let mut part = stock();
            match fault {
                0 => {
                    part.faces[5].loops[0].edges.pop();
                }
                1 => part.faces[5].reversed = true,
                _ => part.faces[5].solid = None,
            }
            assert!(
                discover(&Context::new(&part), &[]).is_empty(),
                "fault {fault}"
            );
        }
    }

    #[test]
    fn a_rounded_trough_is_measured_from_the_flanks_intersection_and_must_be_concave() {
        // The fixture's V notch: flanks 3 and 7 at 60 degrees, an R5 trough round 5, and a
        // virtual apex 35 below the mouth at y = 80, through a 12 thick plate.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/plate_edge_notches.step");
        let part = crate::read_step_file(&path).unwrap();
        let found = discover(&Context::new(&part), &[]);
        assert_eq!(found.len(), 1);
        let o = &found[0];
        assert_eq!(o.defining, [3, 7]);
        assert!(o.context.contains(&5));
        let r = &o.record;
        assert!((r.trough_radius.unwrap() - 5.).abs() < 1e-9);
        assert!((r.depth - 35.).abs() < 1e-9);
        assert!((r.length - 12.).abs() < 1e-9);
        assert!((r.included_angle_degrees - 60.).abs() < 1e-9);
        assert!((r.width - 70. * 30_f64.to_radians().tan()).abs() < 1e-6);
        assert!(geom::dist(r.at, [90., 45., 6.]) < 1e-9);
        // A convex round (material on the other side) is not a trough.
        let mut convex = Part::new(part.faces.clone(), part.edges.clone(), part.solids.clone());
        convex.faces[5].reversed = !convex.faces[5].reversed;
        assert!(discover(&Context::new(&convex), &[]).is_empty());
    }
}
