//! Oblique through-step recognition (`quiddity.oblique_through_steps`): an open step whose run
//! is oblique in the principal frame, cut by one axis-aligned principal wall and one rectangular
//! oblique wall perpendicular to it. Their concave seam reaches the solid's envelope at both
//! ends with convex terminals there, and the volume swept off the principal wall is empty.

use serde::Serialize;

use super::Context;
use super::body::BodyKey;
use super::evidence::{self, EvidenceError, Occurrence};
use super::placed::{Grid, Placed};
use super::planes::{axis_aligned_axis, coordinates, normalized, unit_plane_normal};
use super::policy::length_tol;
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::{
    Bounds, COORD_FLOOR, Curve, SMOOTH_ARC_GAP, Surface, V3, add, cross, dot, norm, scale, sub,
};
use crate::kernel::py;

const END_EPS: f64 = COORD_FLOOR;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ObliqueThroughStep {
    pub run: V3,
    pub length: f64,
    pub at: Placed<V3>,
    pub depth_direction: V3,
    pub depth: f64,
    pub across_direction: V3,
    pub wall_outline: [[f64; 2]; 4],
    pub body_key: Option<BodyKey>,
}

impl ObliqueThroughStep {
    /// The fields in the order Python's `__lt__` compares them.
    fn sort_key(&self) -> Vec<f64> {
        let mut key: Vec<f64> = self.run.to_vec();
        key.push(self.length);
        key.extend(*self.at);
        key.extend(self.depth_direction);
        key.push(self.depth);
        key.extend(self.across_direction);
        key.extend(self.wall_outline.iter().flatten());
        key.push(if self.body_key.is_some() { 1.0 } else { 0.0 });
        key.extend(self.body_key.iter().flat_map(|k| k.iter()));
        key
    }
}

internal_entry! {
/// `recognise_oblique_through_steps`.
pub fn recognise_oblique_through_steps(part: &Part) -> Vec<ObliqueThroughStep> {
    super::records(discover(&Context::new(part)))
}
}

internal_entry! {
/// The evidence path: each step with its principal and oblique walls.
pub fn discover_verified(
    ctx: &Context<'_>,
) -> Result<Vec<Occurrence<ObliqueThroughStep>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx))
}
}

/// The outer loop's vertices in boundary order (`outer_wire().vertices()`).
pub(super) fn outer_vertices(part: &Part, face: usize) -> Vec<V3> {
    let Some(outer) = part.outer_loop(face) else {
        return Vec::new();
    };
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for &(e, forward) in &part.faces[face].loops[outer].edges {
        let edge = &part.edges[e];
        let (v, p) = if forward {
            (edge.vertices.0, edge.start)
        } else {
            (edge.vertices.1, edge.end)
        };
        if !seen.contains(&v) {
            seen.push(v);
            out.push(p);
        }
    }
    out
}

/// `_canonical_outline`: the least rotation of the outline or its reverse, without -0.0.
fn canonical_outline(vertices: &[[f64; 2]]) -> [[f64; 2]; 4] {
    let normalized: Vec<[f64; 2]> = vertices
        .iter()
        .map(|p| p.map(py::without_negative_zero))
        .collect();
    let mut reversed = normalized.clone();
    reversed.reverse();
    let mut best: Option<[[f64; 2]; 4]> = None;
    for path in [&normalized, &reversed] {
        for i in 0..4 {
            let candidate: [[f64; 2]; 4] = std::array::from_fn(|k| path[(i + k) % 4]);
            let flat = |c: &[[f64; 2]; 4]| c.iter().flatten().copied().collect::<Vec<f64>>();
            if best
                .as_ref()
                .is_none_or(|b| py::tuple_order(&flat(&candidate), &flat(b)).is_lt())
            {
                best = Some(candidate);
            }
        }
    }
    best.expect("four rotations")
}

fn endpoint_on_envelope(point: V3, bounds: &Bounds, principal_axis: usize) -> bool {
    (0..3).filter(|&a| a != principal_axis).any(|a| {
        [bounds.min[a], bounds.max[a]]
            .iter()
            .any(|b| (point[a] - b).abs() <= END_EPS)
    })
}

/// Whether two faces share an edge with a vertex at *endpoint*.
fn meet_at(part: &Part, a: usize, b: usize, endpoint: V3) -> bool {
    part.shared_edges(a, b).iter().any(|&e| {
        part.edges[e]
            .vertex_points()
            .iter()
            .any(|p| norm(sub(*p, endpoint)) <= END_EPS)
    })
}

/// `_terminal`: convex faces closing both walls at a seam end, one face or two convex-joined.
fn terminal(part: &Part, principal: usize, oblique: usize, endpoint: V3, solid: &[usize]) -> bool {
    let convex_terminals = |source: usize| -> Vec<usize> {
        part.neighbours(source)
            .into_iter()
            .filter(|&n| {
                solid.contains(&n)
                    && n != principal
                    && n != oblique
                    && part.arc(source, n) == Some(Arc::Convex)
                    && meet_at(part, source, n, endpoint)
            })
            .collect()
    };
    let rights = convex_terminals(oblique);
    convex_terminals(principal).into_iter().any(|left| {
        rights.iter().any(|&right| {
            left == right
                || (part.arc(left, right) == Some(Arc::Convex)
                    && meet_at(part, left, right, endpoint))
        })
    })
}

/// A principal support must have no transverse normal component. The shared coarse plane
/// classifier also admits shallow ramps, which must remain oblique walls here.
pub(super) fn principal_plane(part: &Part, face: usize) -> Option<(usize, f64)> {
    let plane = axis_aligned_axis(part, face)?;
    let normal = unit_plane_normal(part, face)?;
    (0..3)
        .filter(|&axis| axis != plane.0)
        .all(|axis| normal[axis].abs() <= SMOOTH_ARC_GAP)
        .then_some(plane)
}

/// The rectangular outer wall boundary is independent of holes drilled through the wall.
/// Inner wires are still checked by the convex-neighbour and exact swept-volume proofs.
pub(super) fn linear_outer_quad(part: &Part, face: usize) -> Option<Vec<V3>> {
    if !matches!(part.faces[face].surface, Surface::Plane { .. })
        || !part
            .outer_edges(face)
            .iter()
            .all(|&edge| matches!(part.edges[edge].curve, Curve::Line { .. }))
    {
        return None;
    }
    let vertices = outer_vertices(part, face);
    let n = vertices.len();
    if n < 4 {
        return None;
    }
    if n == 4 {
        return Some(vertices);
    }
    let corners: Vec<_> = (0..n)
        .filter_map(|i| {
            let (a, p, b) = (
                vertices[(i + n - 1) % n],
                vertices[i],
                vertices[(i + 1) % n],
            );
            let chord = sub(b, a);
            let on_segment = dot(sub(p, a), sub(b, p)) >= 0.0
                && norm(cross(sub(p, a), chord)) <= COORD_FLOOR * norm(chord);
            (!on_segment).then_some(p)
        })
        .collect();
    (corners.len() == 4).then_some(corners)
}

/// One complete straight seam may be subdivided into several B-rep edges.
pub(super) fn seam_endpoints(part: &Part, a: usize, b: usize) -> Option<(V3, V3)> {
    let edges = part.shared_edges(a, b);
    if edges.is_empty()
        || edges
            .iter()
            .any(|&e| !matches!(part.edges[e].curve, Curve::Line { .. }))
    {
        return None;
    }
    let points: Vec<_> = edges
        .iter()
        .flat_map(|&e| part.edges[e].vertex_points())
        .collect();
    let first = *points.first()?;
    let start = *points
        .iter()
        .max_by(|a, b| py::order(norm(sub(**a, first)), norm(sub(**b, first))))?;
    let end = *points
        .iter()
        .max_by(|a, b| py::order(norm(sub(**a, start)), norm(sub(**b, start))))?;
    let (start, end) = if py::tuple_order(&start, &end).is_gt() {
        (end, start)
    } else {
        (start, end)
    };
    let length = norm(sub(end, start));
    let run = normalized(sub(end, start))?;
    if points
        .iter()
        .any(|&p| norm(cross(sub(p, start), run)) > COORD_FLOOR)
    {
        return None;
    }
    let mut intervals: Vec<_> = edges
        .iter()
        .map(|&e| {
            let edge = &part.edges[e];
            let a = dot(sub(edge.start, start), run);
            let b = dot(sub(edge.end, start), run);
            (a.min(b), a.max(b))
        })
        .collect();
    intervals.sort_by(|a, b| py::order(a.0, b.0));
    let mut covered = 0.0_f64;
    for (low, high) in intervals {
        if low > covered + COORD_FLOOR {
            return None;
        }
        covered = covered.max(high);
    }
    (length > COORD_FLOOR && (covered - length).abs() <= COORD_FLOOR).then_some((start, end))
}

/// The record for one principal wall and its oblique neighbour, or `None` (`_one_pair`).
fn one_pair(
    ctx: &Context<'_>,
    s: usize,
    principal: usize,
    oblique: usize,
    bounds: &Bounds,
    solid: &[usize],
) -> Option<ObliqueThroughStep> {
    let part = ctx.part;
    if part.arc(principal, oblique) != Some(Arc::Concave) {
        return None;
    }
    let (axis, coordinate) = principal_plane(part, principal)?;
    if !matches!(part.faces[oblique].surface, Surface::Plane { .. })
        || principal_plane(part, oblique).is_some()
    {
        return None;
    }
    let principal_vertices = linear_outer_quad(part, principal)?;
    let oblique_vertices = linear_outer_quad(part, oblique)?;
    let principal_normal = unit_plane_normal(part, principal)?;
    let oblique_normal = unit_plane_normal(part, oblique)?;
    if dot(principal_normal, oblique_normal).abs() > SMOOTH_ARC_GAP {
        return None;
    }
    let (start, end) = seam_endpoints(part, principal, oblique)?;
    let run_length = norm(sub(end, start));
    if run_length <= COORD_FLOOR {
        return None;
    }
    let run = normalized(sub(end, start))?;
    if dot(run, principal_normal).abs() > SMOOTH_ARC_GAP
        || dot(run, oblique_normal).abs() > SMOOTH_ARC_GAP
    {
        return None;
    }
    if !endpoint_on_envelope(start, bounds, axis) || !endpoint_on_envelope(end, bounds, axis) {
        return None;
    }
    if !terminal(part, principal, oblique, start, solid)
        || !terminal(part, principal, oblique, end, solid)
    {
        return None;
    }
    let all_convex = |node: usize, other: usize| {
        part.neighbours(node)
            .into_iter()
            .filter(|&n| n != other)
            .all(|n| part.arc(node, n) == Some(Arc::Convex))
    };
    if !all_convex(principal, oblique) || !all_convex(oblique, principal) {
        return None;
    }
    // The oblique wall is the far side of the sweep: each seam end on the principal wall and
    // again at one common positive depth.
    let mut depths: Vec<f64> = oblique_vertices
        .iter()
        .map(|v| py::round_to(dot(sub(*v, start), principal_normal), 6))
        .collect();
    depths.sort_by(|a, b| py::order(*a, *b));
    depths.dedup();
    let [near, depth] = depths[..] else {
        return None;
    };
    if near.abs() > END_EPS || depth <= COORD_FLOOR {
        return None;
    }
    let far = coordinate + depth * principal_normal[axis];
    if (far - bounds.min[axis]).abs() > END_EPS && (far - bounds.max[axis]).abs() > END_EPS {
        return None;
    }
    for v in &oblique_vertices {
        let offset = sub(*v, start);
        if dot(offset, oblique_normal).abs() > END_EPS {
            return None;
        }
        let along = dot(offset, run);
        if along.abs().min((along - run_length).abs()) > END_EPS {
            return None;
        }
        let deep = dot(offset, principal_normal);
        if deep.abs().min((deep - depth).abs()) > END_EPS {
            return None;
        }
    }
    if principal_vertices.len() != 4 {
        return None;
    }
    let outer: Vec<V3> = principal_vertices
        .iter()
        .copied()
        .filter(|v| norm(sub(*v, start)).min(norm(sub(*v, end))) > END_EPS)
        .collect();
    let [o0, o1] = outer[..] else { return None };
    let outer_edge = sub(o1, o0);
    // A skew connecting edge closes a triangular blind-step wedge; a through step's opposite
    // wall boundary is a principal stock edge.
    if norm(outer_edge) <= COORD_FLOOR
        || !(0..3)
            .filter(|&c| c != axis)
            .any(|c| normalized(outer_edge).is_some_and(|n| 1.0 - n[c].abs() <= SMOOTH_ARC_GAP))
    {
        return None;
    }
    let across_values: Vec<f64> = principal_vertices
        .iter()
        .map(|v| dot(sub(*v, start), oblique_normal))
        .collect();
    let lo = across_values.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = across_values
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    if lo < -END_EPS && hi > END_EPS {
        return None;
    }
    let across = if hi > END_EPS {
        oblique_normal
    } else {
        scale(oblique_normal, -1.0)
    };
    let outline: Vec<[f64; 2]> = principal_vertices
        .iter()
        .map(|v| {
            let o = sub(*v, start);
            [py::round_to3(dot(o, run)), py::round_to3(dot(o, across))]
        })
        .collect();
    if outline.iter().filter(|p| p[1] > END_EPS).count() != 2 {
        return None;
    }
    // The wall swept through the depth, inset only along it so that boundary contact is not
    // counted as material.
    let inset = length_tol(depth, 1e-6).min(depth / 4.0);
    let volume = ctx.swept_face_volume(
        s,
        principal,
        scale(principal_normal, inset),
        scale(principal_normal, depth - 2.0 * inset),
    )?;
    if volume != 0.0 {
        return None;
    }
    Some(ObliqueThroughStep {
        run: coordinates(run, 6),
        length: py::round_to3(run_length),
        at: Placed::new(scale(add(start, end), 0.5), Grid::CleanDecimals(3)),
        depth_direction: coordinates(principal_normal, 6),
        depth: py::round_to3(depth),
        across_direction: coordinates(across, 6),
        wall_outline: canonical_outline(&outline),
        body_key: None, // filled in by `discover`
    })
}

internal_entry! {
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<ObliqueThroughStep>> {
    let part = ctx.part;
    let mut found = Vec::new();
    for (s, solid) in part.solids.iter().enumerate() {
        // Python refuses (raises) a step whose walls do not belong to one valid solid; the
        // port publishes none from such a solid.
        if !part.solid_is_valid(s) {
            continue;
        }
        let bounds = part.solid_bounds(s);
        let mut faces = solid.faces.clone();
        faces.sort_unstable();
        for principal in faces {
            if principal_plane(part, principal).is_none() {
                continue;
            }
            for oblique in part.neighbours(principal) {
                if !solid.faces.contains(&oblique) {
                    continue;
                }
                if let Some(record) = one_pair(ctx, s, principal, oblique, &bounds, &solid.faces) {
                    found.push((s, record, principal, oblique));
                }
            }
        }
    }
    // Body keys need every solid's exact mass, so they are only computed for parts with records.
    let keys = if found.is_empty() {
        vec![]
    } else {
        ctx.body_keys(true)
    };
    let mut out: Vec<_> = found
        .into_iter()
        .map(|(s, record, principal, oblique)| Occurrence {
            record: ObliqueThroughStep {
                body_key: keys[s].clone(),
                ..record
            },
            defining: vec![principal, oblique],
            context: vec![],
        })
        .collect();
    out.sort_by(|a, b| py::tuple_order(&a.record.sort_key(), &b.record.sort_key()));
    out
}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::support_patches::ruled_prism;

    /// A rectangular stock with its upper-left corner cut away. The vertical wall moves
    /// laterally along the run, while the floor remains a principal plane.
    fn slanted_corner(slope: f64) -> Part {
        let section = |z: f64| {
            let wall = 5.0 + slope * z;
            vec![
                [0.0, 0.0, z],
                [20.0, 0.0, z],
                [20.0, 30.0, z],
                [wall, 30.0, z],
                [wall, 10.0, z],
                [0.0, 10.0, z],
            ]
        };
        ruled_prism(&section(0.0), &section(40.0)).expect("valid corner cut")
    }

    #[test]
    fn shallow_slanted_walls_are_not_principal_supports() {
        for slope in [0.001, 0.05, 0.2] {
            let part = slanted_corner(slope);
            let found = discover(&Context::new(&part));
            assert_eq!(found.len(), 1, "slope {slope}");
            assert_eq!(found[0].defining.len(), 2);
            assert_eq!(found[0].record.depth, 20.0);
            assert_eq!(
                found[0].record.length,
                py::round_to3(40.0 * (1.0 + slope * slope).sqrt())
            );
        }
    }

    #[test]
    fn drilled_floor_keeps_the_same_oblique_step() {
        let stock = slanted_corner(0.05);
        let expected = discover(&Context::new(&stock));
        let low = [
            [1.0, 0.0, 10.0],
            [2.0, 0.0, 10.0],
            [2.0, 0.0, 11.0],
            [1.0, 0.0, 11.0],
        ];
        let high = low.map(|p| [p[0], 10.0, p[2]]);
        let tunnel = ruled_prism(&low, &high).expect("square drill");
        let mut faces = stock.faces.clone();
        let mut edges = stock.edges.clone();
        let edge_offset = edges.len();
        let vertex_offset = edges
            .iter()
            .flat_map(|e| [e.vertices.0, e.vertices.1])
            .max()
            .unwrap()
            + 1;
        for mut edge in tunnel.edges.clone() {
            edge.vertices = (
                edge.vertices.0 + vertex_offset,
                edge.vertices.1 + vertex_offset,
            );
            edges.push(edge);
        }
        for (index, mut face) in tunnel.faces.into_iter().enumerate() {
            for lp in &mut face.loops {
                for (edge, _) in &mut lp.edges {
                    *edge += edge_offset;
                }
            }
            if index < 2 {
                let normal = if index == 0 { -1.0 } else { 1.0 };
                let coordinate = if index == 0 { 0.0 } else { 10.0 };
                let owner = (0..stock.faces.len())
                    .find(|&f| {
                        (stock.face_bounds(f).min[1] - coordinate).abs() <= COORD_FLOOR
                            && stock
                                .face_normal(f, 0.0, 0.0)
                                .is_some_and(|n| n[1] == normal)
                    })
                    .unwrap();
                let mut hole = face.loops.remove(0);
                hole.edges.reverse();
                for (_, forward) in &mut hole.edges {
                    *forward = !*forward;
                }
                faces[owner].loops.push(hole);
            } else {
                face.reversed = !face.reversed;
                for lp in &mut face.loops {
                    lp.edges.reverse();
                    for (_, forward) in &mut lp.edges {
                        *forward = !*forward;
                    }
                }
                faces.push(face);
            }
        }
        let solid = crate::kernel::brep::Solid {
            faces: (0..faces.len()).collect(),
        };
        let drilled = Part::new(faces, edges, vec![solid]);
        assert!(
            drilled.solid_is_geometrically_valid(0),
            "bad faces {:?}",
            drilled.bad_faces(0)
        );
        let found = discover(&Context::new(&drilled));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].defining, expected[0].defining);
        // Body mass changes, while the step's measured shape does not.
        let mut record = found[0].record.clone();
        record.body_key = expected[0].record.body_key.clone();
        assert_eq!(record, expected[0].record);
    }

    #[test]
    fn subdividing_straight_wall_edges_keeps_the_same_step() {
        let stock = slanted_corner(0.05);
        let expected = discover(&Context::new(&stock));
        let [principal, oblique] = expected[0].defining[..] else {
            panic!("two walls")
        };
        let seam = stock.shared_edges(principal, oblique)[0];
        let boundary = stock
            .face_edges(principal)
            .into_iter()
            .find(|&e| e != seam)
            .unwrap();
        let mut faces = stock.faces.clone();
        let mut edges = stock.edges.clone();
        let vertex_offset = edges
            .iter()
            .flat_map(|e| [e.vertices.0, e.vertices.1])
            .max()
            .unwrap()
            + 1;
        for (split, index) in [seam, boundary].into_iter().enumerate() {
            let vertex = vertex_offset + split;
            let mut first = edges[index].clone();
            let mut second = first.clone();
            let midpoint = scale(add(first.start, first.end), 0.5);
            first.end = midpoint;
            first.vertices.1 = vertex;
            first.samples = vec![first.start, first.end];
            second.start = midpoint;
            second.vertices.0 = vertex;
            second.samples = vec![second.start, second.end];
            let next = edges.len();
            edges[index] = first;
            edges.push(second);
            for face in &mut faces {
                for lp in &mut face.loops {
                    lp.edges = lp
                        .edges
                        .iter()
                        .flat_map(|&(e, forward)| {
                            if e != index {
                                vec![(e, forward)]
                            } else if forward {
                                vec![(index, true), (next, true)]
                            } else {
                                vec![(next, false), (index, false)]
                            }
                        })
                        .collect();
                }
            }
        }
        let solid = crate::kernel::brep::Solid {
            faces: (0..faces.len()).collect(),
        };
        let split = Part::new(faces, edges, vec![solid]);
        assert!(split.solid_is_geometrically_valid(0));
        let found = discover(&Context::new(&split));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].defining, expected[0].defining);
        assert_eq!(found[0].record, expected[0].record);
    }

    #[test]
    fn rectangular_corner_is_not_an_oblique_step() {
        let part = slanted_corner(0.0);
        assert!(discover(&Context::new(&part)).is_empty());
    }
}
