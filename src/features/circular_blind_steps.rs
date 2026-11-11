//! Circular blind-step recognition (`quiddity.circular_blind_steps`): an inward quarter
//! cylinder cut from a stock corner along a principal axis, open at one end of the solid's
//! envelope and closed at the other by a perpendicular planar terminal. Two convex side joins
//! prove the corner opening, and the terminal swept to the opening must meet no material.
//! A proved native conical entry may precede the straight wall. Its cone is consulted,
//! the published length measures the cylinder, and the void proof reaches the exterior mouth.

use std::f64::consts::FRAC_PI_2;

use serde::Serialize;

use super::Context;
use super::cylinders::CylinderEvidence;
use super::evidence::{self, EvidenceError, Occurrence, common_valid_solid};
use super::placed::{Grid, Placed};
use super::planes::{axis_aligned_axis, unit_plane_normal};
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::{self, COORD_FLOOR, SMOOTH_ARC_GAP, Surface};
use crate::kernel::py;

/// Radians of cylindrical parameter noise admitted around an exact quarter turn
/// (`QUARTER_TURN_RAD_TOL`).
const QUARTER_TURN_RAD_TOL: f64 = 1e-7;
const PRINCIPAL_AXIS_COS: f64 = 1.0 - SMOOTH_ARC_GAP;

type Point2 = [f64; 2];
type Point3 = [f64; 3];

#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct CircularBlindStep {
    pub axis: char,
    pub radius: f64,
    pub length: f64,
    /// Blind terminal, then the open end, on the cylinder's axis.
    pub centreline: [Placed<Point3>; 2],
    /// Arc endpoint, cylinder centre, arc endpoint, in the two transverse coordinates.
    pub section: [Placed<Point2>; 3],
}

impl CircularBlindStep {
    /// The fields after `axis`, in the order the Python dataclass orders them.
    fn sort_key(&self) -> Vec<f64> {
        let mut key = vec![self.radius, self.length];
        key.extend(self.centreline.iter().flatten());
        key.extend(self.section.iter().flatten());
        key
    }
}

internal_entry! {
/// `recognise_circular_blind_steps`.
pub fn recognise_circular_blind_steps(part: &Part) -> Vec<CircularBlindStep> {
    super::records(discover(&Context::new(part)))
}
}

internal_entry! {
/// The evidence path: each step with its cylindrical wall and terminal.
pub fn discover_verified(
    ctx: &Context<'_>,
) -> Result<Vec<Occurrence<CircularBlindStep>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx))
}
}

/// `math.isclose(a, b, abs_tol=COORD_FLOOR)`.
fn close(a: f64, b: f64) -> bool {
    py::isclose(a, b, 1e-9, COORD_FLOOR)
}

/// The two distinct arc endpoints the wall and terminal share, in transverse coordinates,
/// ascending (`_shared_arc_endpoints`).
fn shared_arc_endpoints(
    part: &Part,
    cylinder: usize,
    terminal: usize,
    transverse: [usize; 2],
) -> Option<(Point2, Point2)> {
    let mut points: Vec<Point2> = Vec::new();
    for e in part.shared_edges(cylinder, terminal) {
        for p in part.edges[e].vertex_points() {
            let projected = [p[transverse[0]], p[transverse[1]]];
            if !points
                .iter()
                .any(|q| py::dist(&projected, q) <= COORD_FLOOR)
            {
                points.push(projected);
            }
        }
    }
    let [mut a, mut b] = points[..] else {
        return None;
    };
    if py::tuple_order(&b, &a).is_lt() {
        std::mem::swap(&mut a, &mut b);
    }
    Some((a, b))
}

/// Prove a complete quarter-wall boundary before selecting supports among extra neighbours.
/// Two circular rims and two complete generators allow subdivided straight edges, but no
/// holes, intermediate trims or interrupted walls. The quarter angle is checked by `candidate`.
fn complete_quarter_wall(
    part: &Part,
    cylinder: usize,
    evidence: &CylinderEvidence,
    low: f64,
    high: f64,
) -> bool {
    if part.faces[cylinder].loops.len() != 1 {
        return false;
    }
    let Surface::Cylinder {
        radius: wall_radius,
        ..
    } = part.faces[cylinder].surface
    else {
        return false;
    };
    let axis = evidence.axis;
    let mut rims = [0; 2];
    type Generator = (Point3, Vec<(f64, f64)>);
    let mut generators: Vec<Generator> = Vec::new();
    for edge in part.face_edges(cylinder) {
        let e = &part.edges[edge];
        match e.curve {
            geom::Curve::Line { dir, .. } => {
                let Some(unit) = py::unit(dir) else {
                    return false;
                };
                if (0..3).any(|i| i != axis && unit[i].abs() > SMOOTH_ARC_GAP) {
                    return false;
                }
                let mut transverse = e.start;
                transverse[axis] = 0.;
                let interval = (
                    e.start[axis].min(e.end[axis]),
                    e.start[axis].max(e.end[axis]),
                );
                if let Some((_, spans)) = generators
                    .iter_mut()
                    .find(|(p, _)| py::dist(p, &transverse) <= COORD_FLOOR)
                {
                    spans.push(interval);
                } else {
                    generators.push((transverse, vec![interval]));
                }
            }
            geom::Curve::Circle { frame, radius } => {
                if (1.0 - frame.z[axis].abs()) > SMOOTH_ARC_GAP
                    || (radius - wall_radius).abs() > COORD_FLOOR
                    || (0..3).any(|i| {
                        i != axis && (frame.origin[i] - evidence.axis_point[i]).abs() > COORD_FLOOR
                    })
                {
                    return false;
                }
                let end = if close(frame.origin[axis], low) {
                    0
                } else if close(frame.origin[axis], high) {
                    1
                } else {
                    return false;
                };
                rims[end] += 1;
            }
            _ => return false,
        }
    }
    rims == [1, 1]
        && generators.len() == 2
        && generators.iter_mut().all(|(_, spans)| {
            spans.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut at = low;
            for &(start, end) in spans.iter() {
                if (start - at).abs() > COORD_FLOOR {
                    return false;
                }
                at = end;
            }
            close(at, high)
        })
}

/// A native, coaxial quarter-cone flaring from the complete cylindrical rim to
/// one perpendicular exterior plane. Both corner planes and all four boundary
/// edges must be observed; a taper into material or an interrupted treatment is refused.
fn conical_entry(
    part: &Part,
    cylinder: usize,
    cone: usize,
    evidence: &CylinderEvidence,
    station: f64,
    direction: f64,
) -> Option<f64> {
    let Surface::Cone {
        frame,
        radius,
        semi_angle,
    } = part.faces[cone].surface
    else {
        return None;
    };
    let axis = evidence.axis;
    if !part.faces[cone].reversed
        || part.faces[cone].loops.len() != 1
        || frame.z[axis].abs() < PRINCIPAL_AXIS_COS
        || (0..3)
            .any(|i| i != axis && (frame.origin[i] - evidence.axis_point[i]).abs() > COORD_FLOOR)
    {
        return None;
    }
    let (u0, u1, _, _) = part.uv_bounds(cone)?;
    if !py::isclose(u1 - u0, FRAC_PI_2, 0.0, QUARTER_TURN_RAD_TOL) {
        return None;
    }
    let mut rims = Vec::new();
    let mut generators = 0;
    for edge in part.face_edges(cone) {
        let e = &part.edges[edge];
        match e.curve {
            geom::Curve::Circle {
                frame: rim,
                radius: r,
            } => {
                if rim.z[axis].abs() < PRINCIPAL_AXIS_COS
                    || (0..3).any(|i| {
                        i != axis && (rim.origin[i] - evidence.axis_point[i]).abs() > COORD_FLOOR
                    })
                {
                    return None;
                }
                let expected = radius
                    + (rim.origin[axis] - frame.origin[axis]) / frame.z[axis] * semi_angle.tan();
                if !close(r, expected) {
                    return None;
                }
                rims.push((rim.origin[axis], r, edge));
            }
            geom::Curve::Line { .. } => generators += 1,
            _ => return None,
        }
    }
    if rims.len() != 2 || generators != 2 {
        return None;
    }
    // CylinderEvidence's published diameter is rounded; seam incidence uses
    // the original native radius, not that presentation value.
    let Surface::Cylinder {
        radius: wall_radius,
        ..
    } = part.faces[cylinder].surface
    else {
        return None;
    };
    let join = rims.iter().position(|&(at, r, edge)| {
        close(at, station)
            && close(r, wall_radius)
            && part.shared_edges(cylinder, cone).contains(&edge)
    })?;
    let (outer, outer_radius, _) = rims[1 - join];
    if direction * (outer - station) <= COORD_FLOOR || outer_radius <= rims[join].1 + COORD_FLOOR {
        return None;
    }
    let neighbours: Vec<_> = part
        .neighbours(cone)
        .into_iter()
        .filter(|&f| f != cylinder)
        .collect();
    if neighbours.len() != 3 {
        return None;
    }
    let mut end = 0;
    let mut sides = Vec::new();
    for &face in &neighbours {
        if part.arc(cone, face) != Some(Arc::Convex) {
            return None;
        }
        let Surface::Plane { frame: plane } = part.faces[face].surface else {
            return None;
        };
        let n = unit_plane_normal(part, face)?;
        if n[axis] * direction >= PRINCIPAL_AXIS_COS && close(plane.origin[axis], outer) {
            end += 1;
        } else if n[axis].abs() <= SMOOTH_ARC_GAP
            && geom::dot(geom::sub(evidence.axis_point, plane.origin), n).abs() <= COORD_FLOOR
        {
            sides.push(n);
        } else {
            return None;
        }
    }
    if end != 1 || sides.len() != 2 || geom::dot(sides[0], sides[1]).abs() > SMOOTH_ARC_GAP {
        return None;
    }
    let mut members = neighbours;
    members.extend([cylinder, cone]);
    common_valid_solid(part, &members)?;
    Some(direction * (outer - station))
}

/// The record for a quarter-cylinder wall closed by *terminal*, or `None` (`_candidate`).
fn candidate(
    ctx: &Context<'_>,
    cylinder: usize,
    terminal: usize,
    evidence: &CylinderEvidence,
) -> Option<(CircularBlindStep, Vec<usize>)> {
    let part = ctx.part;
    let axis = evidence.axis;
    if evidence.direction[axis].abs() < PRINCIPAL_AXIS_COS {
        return None;
    }
    let (plane_axis, terminal_at) = axis_aligned_axis(part, terminal)?;
    if plane_axis != axis || part.arc(cylinder, terminal) != Some(Arc::Concave) {
        return None;
    }
    if !py::isclose(evidence.u_extent, FRAC_PI_2, 0.0, QUARTER_TURN_RAD_TOL) || evidence.external {
        return None;
    }
    let solid = common_valid_solid(part, &[cylinder, terminal])?;
    let face = part.face_bounds(cylinder);
    let (low, high) = (face.min[axis], face.max[axis]);
    let body = part.solid_bounds(solid);
    let (direction, opening_at) = if close(terminal_at, low) && close(high, body.max[axis]) {
        (1.0, high)
    } else if close(terminal_at, high) && close(low, body.min[axis]) {
        (-1.0, low)
    } else if close(terminal_at, low) && !close(terminal_at, high) {
        (1.0, high)
    } else if close(terminal_at, high) && !close(terminal_at, low) {
        (-1.0, low)
    } else {
        return None;
    };
    let length = high - low;
    let on_envelope = if direction > 0.0 {
        close(opening_at, body.max[axis])
    } else {
        close(opening_at, body.min[axis])
    };

    let (mut axial, mut sides) = (Vec::new(), Vec::new());
    let mut side_axes = Vec::new();
    let mut entry_depth = 0.;
    let mut entry_face = None;
    let mut corner_supports: Vec<(geom::V3, Option<usize>)> = Vec::new();
    for neighbour in part.neighbours(cylinder) {
        if neighbour == terminal {
            continue;
        }
        if part.arc(cylinder, neighbour) != Some(Arc::Convex) {
            return None;
        }
        if let Some(depth) =
            conical_entry(part, cylinder, neighbour, evidence, opening_at, direction)
        {
            if !complete_quarter_wall(part, cylinder, evidence, low, high) {
                return None;
            }
            entry_depth = depth;
            entry_face = Some(neighbour);
            axial.push((neighbour, (axis, opening_at)));
            continue;
        }
        if let Some(plane) = axis_aligned_axis(part, neighbour)
            && plane.0 == axis
        {
            axial.push((neighbour, plane));
        } else {
            // The section contains actual transverse points, so its two side planes may
            // roll about the principal cylinder without changing the record contract.
            let Surface::Plane { frame } = part.faces[neighbour].surface else {
                return None;
            };
            let normal = unit_plane_normal(part, neighbour)?;
            let aligned = axis_aligned_axis(part, neighbour);
            // Preserve the existing principal-plane admission; only newly admitted rolled
            // supports need the additional intrinsic perpendicularity and axis-incidence proof.
            if (aligned.is_none() || !on_envelope)
                && (geom::dot(normal, evidence.direction).abs() > SMOOTH_ARC_GAP
                    || geom::dot(geom::sub(evidence.axis_point, frame.origin), normal).abs()
                        > COORD_FLOOR)
            {
                return None;
            }
            if geom::dot(normal, evidence.direction).abs() <= SMOOTH_ARC_GAP
                && geom::dot(geom::sub(evidence.axis_point, frame.origin), normal).abs()
                    <= COORD_FLOOR
                && !corner_supports
                    .iter()
                    .any(|(n, _)| geom::norm(geom::sub(*n, normal)) <= SMOOTH_ARC_GAP)
            {
                corner_supports.push((normal, aligned.map(|p| p.0)));
            }
            sides.push(normal);
            side_axes.push(aligned.map(|p| p.0));
        }
    }
    let transverse: [usize; 2] = match axis {
        0 => [1, 2],
        1 => [0, 2],
        _ => [0, 1],
    };
    // A secondary cut can add a convex plane along a subdivided generator without
    // changing the quarter wall. Count geometric corner supports only after proving
    // the entire wall; all neighbours have still passed the checks above.
    if sides.len() > 2
        && on_envelope
        && corner_supports.len() == 2
        && complete_quarter_wall(part, cylinder, evidence, low, high)
    {
        sides = corner_supports.iter().map(|(normal, _)| *normal).collect();
        side_axes = corner_supports.iter().map(|(_, axis)| *axis).collect();
    }
    if axial.len() != 1 || sides.len() != 2 {
        return None;
    }
    let perpendicular = match side_axes.as_slice() {
        [Some(a), Some(b)] => a != b,
        _ => geom::dot(sides[0], sides[1]).abs() <= SMOOTH_ARC_GAP,
    };
    if !perpendicular {
        return None;
    }
    if !close(axial[0].1.1, opening_at) {
        return None;
    }
    // A convex shoulder can open the cut inside the overall stock envelope. Its plane
    // must face out along the run; the side-plane incidence checks above establish the
    // same corner as the cylinder, and the complete terminal sweep below proves air.
    if !on_envelope {
        for face in [axial[0].0, terminal]
            .into_iter()
            .filter(|f| Some(*f) != entry_face)
        {
            let normal = unit_plane_normal(part, face)?;
            if normal[axis] * direction < PRINCIPAL_AXIS_COS
                || (0..3).any(|i| i != axis && normal[i].abs() > SMOOTH_ARC_GAP)
            {
                return None;
            }
        }
    }
    let mut sweep = [0.0; 3];
    sweep[axis] = direction * (length + entry_depth);
    if ctx.swept_face_volume(solid, terminal, [0.0; 3], sweep)? != 0.0 {
        return None;
    }

    let (first, second) = shared_arc_endpoints(part, cylinder, terminal, transverse)?;
    let anchor = evidence.axis_point;
    let centre = [anchor[transverse[0]], anchor[transverse[1]]];
    let (mut terminal_point, mut opening_point) = (anchor, anchor);
    terminal_point[axis] = terminal_at;
    opening_point[axis] = opening_at;
    let point2 = |p: Point2| Placed::new(p, Grid::Figures(6));
    let point3 = |p: Point3| Placed::new(p, Grid::Figures(6));
    let radius = if entry_face.is_some() {
        // This newly admitted native pair has an exact shared rim radius. Quantise
        // it once; rounding the cylinder inventory's diameter first loses a digit.
        let Surface::Cylinder { radius, .. } = part.faces[cylinder].surface else {
            return None;
        };
        radius
    } else {
        // Compatibility: native-radius publication changes existing planar-entry
        // snapshot dimensions. Retain their inventory-diameter rounding; the
        // conical-entry extension above already publishes native precision.
        evidence.diameter / 2.
    };
    Some((
        CircularBlindStep {
            axis: ['x', 'y', 'z'][axis],
            radius: py::quantise6(radius),
            length: py::quantise6(length),
            centreline: [point3(terminal_point), point3(opening_point)],
            section: [point2(first), point2(centre), point2(second)],
        },
        entry_face.into_iter().collect(),
    ))
}

internal_entry! {
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<CircularBlindStep>> {
    let part = ctx.part;
    let mut cylinders: Vec<&CylinderEvidence> = ctx.cylinders().iter().collect();
    cylinders.sort_by_key(|c| c.face);
    let mut out = Vec::new();
    for evidence in cylinders {
        let cylinder = evidence.face;
        for terminal in part.neighbours(cylinder) {
            if let Some((record, context)) = candidate(ctx, cylinder, terminal, evidence) {
                out.push(Occurrence {
                    record,
                    defining: vec![cylinder, terminal],
                    context,
                });
            }
        }
    }
    out.sort_by(|a, b| {
        a.record
            .axis
            .cmp(&b.record.axis)
            .then_with(|| py::tuple_order(&a.record.sort_key(), &b.record.sort_key()))
    });
    out
}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::support_patches::ruled_prism;

    /// Add a shallow notch to the golden stock, alongside the cylinder's upper lip.
    /// The deeper variant cuts into the cylinder itself, leaving an intermediate arc.
    fn notched_corner(interrupted: bool) -> (Part, Part) {
        use crate::features::support_patches::polygon_part;
        use crate::kernel::geom::{Curve, V3};
        let original = crate::read_step_file(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/golden_circular_blind_step.step"),
        )
        .unwrap();
        let lip = if interrupted {
            15. - 12_f64.sqrt()
        } else {
            11.
        };
        let mut points: Vec<V3> = Vec::new();
        let mut polygons = Vec::new();
        let mut face = |corners: Vec<V3>, normal: V3| {
            let mut corners = corners;
            let a = geom::sub(corners[1], corners[0]);
            let b = geom::sub(corners[2], corners[1]);
            if geom::dot(geom::cross(a, b), normal) < 0. {
                corners.reverse();
            }
            polygons.push(
                corners
                    .into_iter()
                    .map(|p| {
                        if let Some(i) = points.iter().position(|q| geom::dist(*q, p) < 1e-9) {
                            i
                        } else {
                            points.push(p);
                            points.len() - 1
                        }
                    })
                    .collect(),
            );
        };
        face(
            vec![
                [-20., -15., -10.],
                [-20., 15., -10.],
                [-20., 15., 10.],
                [-20., -15., 10.],
            ],
            [-1., 0., 0.],
        );
        face(
            vec![
                [-20., -15., -10.],
                [20., -15., -10.],
                [20., -15., 8.],
                [0., -15., 8.],
                [0., -15., 10.],
                [-20., -15., 10.],
            ],
            [0., -1., 0.],
        );
        face(
            vec![
                [-20., -15., 10.],
                [0., -15., 10.],
                [0., 11., 10.],
                [-5., 11., 10.],
                [-5., 15., 10.],
                [-20., 15., 10.],
            ],
            [0., 0., 1.],
        );
        face(
            vec![
                [20., 15., -10.],
                [-20., 15., -10.],
                [-20., 15., 10.],
                [-5., 15., 10.],
                [-5., 15., 6.],
                [20., 15., 6.],
            ],
            [0., 1., 0.],
        );
        face(
            vec![
                [-20., -15., -10.],
                [20., -15., -10.],
                [20., 15., -10.],
                [-20., 15., -10.],
            ],
            [0., 0., -1.],
        );
        let mut end = vec![[20., -15., -10.], [20., 15., -10.], [20., 15., 6.]];
        if !interrupted {
            end.push([20., 11., 10.]);
        }
        end.extend([[20., lip, 8.], [20., -15., 8.]]);
        face(end, [1., 0., 0.]);
        face(
            vec![[-5., 11., 10.], [-5., 15., 6.], [-5., 15., 10.]],
            [1., 0., 0.],
        );
        let mut wall = vec![[-5., 11., 10.], [0., 11., 10.]];
        if interrupted {
            wall.push([0., lip, 8.]);
            wall.push([20., lip, 8.]);
        } else {
            wall.push([20., 11., 10.]);
        }
        wall.extend([[20., 15., 6.], [-5., 15., 6.]]);
        face(wall, [0., 1., 1.]);
        face(
            vec![
                [0., -15., 8.],
                [20., -15., 8.],
                [20., lip, 8.],
                [0., lip, 8.],
            ],
            [0., 0., 1.],
        );
        let mut shoulder = vec![[0., -15., 8.], [0., lip, 8.]];
        shoulder.push([0., 11., 10.]);
        shoulder.push([0., -15., 10.]);
        face(shoulder, [1., 0., 0.]);
        if !interrupted {
            face(
                vec![
                    [0., 11., 8.],
                    [20., 11., 8.],
                    [20., 11., 10.],
                    [0., 11., 10.],
                ],
                [0., -1., 0.],
            );
        }
        let shell = polygon_part(&points, &polygons, true).unwrap();
        let mut faces = shell.faces.clone();
        let mut edges = shell.edges.clone();
        faces[7].surface = original.faces[7].surface.clone();
        faces[7].reversed = true;
        // Replace the three section chords (two for an intact wall) with exact arcs
        // on the fixture's cylinder. The remaining straight edges are its generators.
        let Surface::Cylinder { radius, .. } = faces[7].surface else {
            unreachable!()
        };
        let Curve::Circle { frame: rim, .. } = original.edges[17].curve else {
            unreachable!()
        };
        for &(index, _) in &faces[7].loops[0].edges {
            let edge = &mut edges[index];
            if (edge.start[0] - edge.end[0]).abs() > COORD_FLOOR {
                continue;
            }
            let mut frame = rim;
            frame.origin[0] = edge.start[0];
            edge.curve = Curve::Circle { frame, radius };
            let start = edge.curve.parameter(edge.start);
            let delta = (edge.curve.parameter(edge.end) - start + std::f64::consts::PI)
                .rem_euclid(std::f64::consts::TAU)
                - std::f64::consts::PI;
            edge.same_sense = delta > 0.;
            edge.samples = (0..=128)
                .map(|i| edge.curve.value(start + delta * i as f64 / 128.))
                .collect();
        }
        let part = Part::new(faces, edges, shell.solids.clone());
        assert!(
            part.solid_is_geometrically_valid(0),
            "bad faces {:?}",
            part.bad_faces(0)
        );
        (original, part)
    }

    #[test]
    fn a_third_convex_side_preserves_the_complete_corner_step() {
        let (original, part) = notched_corner(false);
        let ctx = Context::new(&part);
        let evidence = ctx.cylinders().iter().find(|c| c.face == 7).unwrap();
        assert_eq!(part.neighbours(7).len(), 5);
        assert_eq!(part.arc(7, 10), Some(Arc::Convex));
        assert!(complete_quarter_wall(&part, 7, evidence, -5., 20.));
        let expected = discover(&Context::new(&original));
        let found = discover_verified(&ctx).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].defining, vec![7, 6]);
        assert_eq!(found[0].record, expected[0].record);
    }

    #[test]
    fn a_notch_into_the_quarter_wall_is_not_a_complete_step() {
        let (_, part) = notched_corner(true);
        let ctx = Context::new(&part);
        let evidence = ctx.cylinders().iter().find(|c| c.face == 7).unwrap();
        assert_eq!(part.neighbours(7).len(), 6);
        assert!(
            part.neighbours(7)
                .into_iter()
                .filter(|&f| f != 6)
                .all(|f| part.arc(7, f) == Some(Arc::Convex))
        );
        assert!(!complete_quarter_wall(&part, 7, evidence, -5., 20.));
        assert!(discover_verified(&ctx).unwrap().is_empty());
    }

    #[test]
    fn a_remote_stock_extension_keeps_a_local_corner_opening() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/golden_circular_blind_step.step");
        let input = crate::read_step_file(&path).unwrap();
        let frame = crate::frames::infer_part_frame(&input).unwrap();
        let stock = input.transformed(&frame.normalisation()).unwrap();
        let expected = discover(&Context::new(&stock));
        assert_eq!(expected.len(), 1);
        let b = stock.bounds();
        // Attach a small rectangular boss to the opening plane, away from the circular
        // corner. The opening is unchanged, but is no longer the solid's axial extremum.
        let low = [
            [b.min[0] + 2.0, b.min[1] + 2.0, b.min[2]],
            [b.min[0] + 6.0, b.min[1] + 2.0, b.min[2]],
            [b.min[0] + 6.0, b.min[1] + 6.0, b.min[2]],
            [b.min[0] + 2.0, b.min[1] + 6.0, b.min[2]],
        ];
        let high = low.map(|p| [p[0], p[1], p[2] - 5.0]);
        let boss = ruled_prism(&low, &high).unwrap();
        let mut faces = stock.faces.clone();
        let mut edges = stock.edges.clone();
        let edge_offset = edges.len();
        let vertex_offset = edges
            .iter()
            .flat_map(|e| [e.vertices.0, e.vertices.1])
            .max()
            .unwrap()
            + 1;
        for mut edge in boss.edges.clone() {
            edge.vertices = (
                edge.vertices.0 + vertex_offset,
                edge.vertices.1 + vertex_offset,
            );
            edges.push(edge);
        }
        let owner = (0..stock.faces.len())
            .find(|&f| {
                stock
                    .face_normal(f, 0.0, 0.0)
                    .is_some_and(|n| n[2] < -PRINCIPAL_AXIS_COS)
                    && close(stock.face_bounds(f).min[2], b.min[2])
            })
            .unwrap();
        for (index, mut face) in boss.faces.into_iter().enumerate() {
            for lp in &mut face.loops {
                for (edge, _) in &mut lp.edges {
                    *edge += edge_offset;
                }
            }
            if index == 0 {
                faces[owner].loops.push(face.loops.remove(0));
            } else {
                faces.push(face);
            }
        }
        let solid = crate::kernel::brep::Solid {
            faces: (0..faces.len()).collect(),
        };
        let extended = Part::new(faces, edges, vec![solid]);
        assert!(
            extended.solid_is_geometrically_valid(0),
            "bad faces {:?}",
            extended.bad_faces(0)
        );
        let found = discover(&Context::new(&extended));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].defining, expected[0].defining);
        assert_eq!(found[0].record, expected[0].record);
    }
    /// A box minus a radius-4 quarter-cylinder (x=-5..18), with a native
    /// 45-degree conical entry x=18..20. The opening radius is 6 (or 2 for
    /// the inward-taper negative). Geometry is analytic, independent of datasets.
    fn corner_with_conical_entry(flared: bool) -> Part {
        corner_with_conical_entry_radius(flared, 4.)
    }

    fn corner_with_conical_entry_radius(flared: bool, wall_radius: f64) -> Part {
        use crate::features::support_patches::polygon_part;
        use crate::kernel::geom::{Curve, Frame, V3};
        let r = if flared {
            wall_radius + 2.
        } else {
            wall_radius - 2.
        };
        let mut points = Vec::<V3>::new();
        let mut polygons = Vec::new();
        let mut face = |mut corners: Vec<V3>, normal: V3| {
            if geom::dot(
                geom::cross(
                    geom::sub(corners[1], corners[0]),
                    geom::sub(corners[2], corners[1]),
                ),
                normal,
            ) < 0.
            {
                corners.reverse();
            }
            polygons.push(
                corners
                    .into_iter()
                    .map(|p| {
                        if let Some(i) = points.iter().position(|q| geom::dist(*q, p) < 1e-9) {
                            i
                        } else {
                            points.push(p);
                            points.len() - 1
                        }
                    })
                    .collect(),
            );
        };
        face(
            vec![
                [-20., -15., -10.],
                [-20., 15., -10.],
                [-20., 15., 10.],
                [-20., -15., 10.],
            ],
            [-1., 0., 0.],
        );
        face(
            vec![
                [-20., -15., -10.],
                [20., -15., -10.],
                [20., -15., 10.],
                [-20., -15., 10.],
            ],
            [0., -1., 0.],
        );
        face(
            vec![
                [-20., -15., 10.],
                [20., -15., 10.],
                [20., 15. - r, 10.],
                [18., 15. - wall_radius, 10.],
                [-5., 15. - wall_radius, 10.],
                [-5., 15., 10.],
                [-20., 15., 10.],
            ],
            [0., 0., 1.],
        );
        face(
            vec![
                [-20., 15., -10.],
                [20., 15., -10.],
                [20., 15., 10. - r],
                [18., 15., 10. - wall_radius],
                [-5., 15., 10. - wall_radius],
                [-5., 15., 10.],
                [-20., 15., 10.],
            ],
            [0., 1., 0.],
        );
        face(
            vec![
                [-20., -15., -10.],
                [20., -15., -10.],
                [20., 15., -10.],
                [-20., 15., -10.],
            ],
            [0., 0., -1.],
        );
        face(
            vec![
                [20., -15., -10.],
                [20., 15., -10.],
                [20., 15., 10. - r],
                [20., 15. - r, 10.],
                [20., -15., 10.],
            ],
            [1., 0., 0.],
        );
        face(
            vec![
                [-5., 15., 10.],
                [-5., 15. - wall_radius, 10.],
                [-5., 15., 10. - wall_radius],
            ],
            [1., 0., 0.],
        );
        face(
            vec![
                [-5., 15. - wall_radius, 10.],
                [-5., 15., 10. - wall_radius],
                [18., 15., 10. - wall_radius],
                [18., 15. - wall_radius, 10.],
            ],
            [0., 1., 1.],
        );
        face(
            vec![
                [18., 15. - wall_radius, 10.],
                [18., 15., 10. - wall_radius],
                [20., 15., 10. - r],
                [20., 15. - r, 10.],
            ],
            [if flared { 1. } else { -1. }, 1., 1.],
        );
        let shell = polygon_part(&points, &polygons, true).unwrap();
        let mut faces = shell.faces.clone();
        let mut edges = shell.edges.clone();
        let frame = Frame {
            origin: [18., 15., 10.],
            x: [0., 0., -1.],
            y: [0., 1., 0.],
            z: [1., 0., 0.],
        };
        faces[7].surface = Surface::Cylinder {
            frame,
            radius: wall_radius,
        };
        faces[7].reversed = true;
        faces[8].surface = Surface::Cone {
            frame,
            radius: wall_radius,
            semi_angle: if flared { 1_f64.atan() } else { -1_f64.atan() },
        };
        faces[8].reversed = true;
        for edge in &mut edges {
            if !close(edge.start[0], edge.end[0]) {
                continue;
            }
            let at = edge.start[0];
            let radius = if close(at, -5.) || close(at, 18.) {
                wall_radius
            } else if close(at, 20.) {
                r
            } else {
                continue;
            };
            let radial = |p: V3| (p[1] - 15.).hypot(p[2] - 10.);
            if !close(radial(edge.start), radius) || !close(radial(edge.end), radius) {
                continue;
            }
            let mut rim = frame;
            rim.origin[0] = at;
            edge.curve = Curve::Circle { frame: rim, radius };
            let start = edge.curve.parameter(edge.start);
            let delta = (edge.curve.parameter(edge.end) - start + std::f64::consts::PI)
                .rem_euclid(std::f64::consts::TAU)
                - std::f64::consts::PI;
            edge.same_sense = delta > 0.;
            edge.samples = (0..=128)
                .map(|i| edge.curve.value(start + delta * i as f64 / 128.))
                .collect();
        }
        Part::new(faces, edges, shell.solids)
    }

    #[test]
    fn a_native_flared_entry_keeps_the_straight_blind_step_and_its_floor() {
        let original = corner_with_conical_entry(true);
        assert!(
            original.solid_is_geometrically_valid(0),
            "bad {:?}",
            original.bad_faces(0)
        );
        let removed =
            std::f64::consts::PI * (4_f64.powi(2) * 23. / 4. + 2. * (16. + 24. + 36.) / 12.);
        assert!((original.solid_mass(0).unwrap().0 - (24000. - removed)).abs() < 1e-6);
        for axes in [[0, 1, 2], [1, 2, 0], [2, 0, 1]] {
            let mut placement = [[0.; 4]; 3];
            for i in 0..3 {
                placement[axes[i]][i] = 1.;
                placement[i][3] = [123.456, -78.9, 41.3][i];
            }
            let part = original.transformed(&placement).unwrap();
            let found = discover_verified(&Context::new(&part)).unwrap();
            assert_eq!(found.len(), 1, "{found:?}");
            assert_eq!(found[0].defining, vec![7, 6]);
            assert_eq!(found[0].context, vec![8]);
            assert_eq!(found[0].record.radius, 4.);
            assert_eq!(found[0].record.length, 23.);
            let published = crate::features::recognise(&part);
            assert_eq!(published.defining["circular_blind_steps"], vec![vec![7, 6]]);
        }
    }

    #[test]
    fn an_entry_taper_into_material_does_not_prove_an_open_step() {
        let part = corner_with_conical_entry(false);
        assert!(
            part.solid_is_geometrically_valid(0),
            "bad {:?}",
            part.bad_faces(0)
        );
        assert!(discover_verified(&Context::new(&part)).unwrap().is_empty());
    }

    #[test]
    fn cone_incidence_uses_the_native_radius_before_presentation_rounding() {
        let part = corner_with_conical_entry_radius(true, 4.123456789);
        assert!(part.solid_is_geometrically_valid(0));
        let found = discover_verified(&Context::new(&part)).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].record.radius, 4.12346);
        assert_eq!(found[0].record.length, 23.);
    }
}
