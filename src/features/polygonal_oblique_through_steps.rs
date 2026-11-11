//! Polygonal principal walls of open oblique steps. Kept in a separate family so that
//! adding these occurrences cannot renumber existing oblique-step Spec references.

use serde::Serialize;

use super::Context;
use super::body::BodyKey;
use super::evidence::{self, EvidenceError, Occurrence};
use super::oblique_through_steps::{
    linear_outer_quad, outer_vertices, principal_plane, seam_endpoints,
};
use super::placed::{Grid, Placed};
use super::planes::{coordinates, normalized, unit_plane_normal};
use super::policy::length_tol;
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::{
    Bounds, COORD_FLOOR, Curve, SMOOTH_ARC_GAP, Surface, V3, add, cross, dot, norm, scale, sub,
};
use crate::kernel::py;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct PolygonalObliqueThroughStep {
    pub run: V3,
    pub length: f64,
    pub at: Placed<V3>,
    pub depth_direction: V3,
    /// Height above the principal wall, measured to the complete convex lip.
    pub depth: f64,
    pub across_direction: V3,
    /// Unoriented unit axis normal to the two stock-envelope run terminals.
    pub through_axis: V3,
    /// Acute angle between the seam and through_axis, in degrees.
    pub slant_degrees: f64,
    /// Principal floor's corners in (run, across) coordinates from the seam start.
    pub floor_outline: Vec<[f64; 2]>,
    pub body_key: Option<BodyKey>,
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= COORD_FLOOR
}

fn canonical_outline(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let points: Vec<_> = points
        .iter()
        .map(|p| p.map(py::without_negative_zero))
        .collect();
    let mut best = points.clone();
    for reverse in [false, true] {
        let mut path = points.clone();
        if reverse {
            path.reverse();
        }
        for offset in 0..path.len() {
            let candidate: Vec<_> = path
                .iter()
                .cycle()
                .skip(offset)
                .take(path.len())
                .copied()
                .collect();
            let flat = |p: &Vec<[f64; 2]>| p.iter().flatten().copied().collect::<Vec<_>>();
            if py::tuple_order(&flat(&candidate), &flat(&best)).is_lt() {
                best = candidate;
            }
        }
    }
    best
}

/// Non-adjacent segments may neither cross nor touch. Reject uncertain separation
/// within the kernel's length floor, including repeated corners and overlapping edges.
fn simple_outline(points: &[[f64; 2]]) -> bool {
    let n = points.len();
    let side = |a: [f64; 2], b: [f64; 2], p: [f64; 2]| {
        let cross = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
        let tolerance = COORD_FLOOR * (b[0] - a[0]).hypot(b[1] - a[1]);
        if cross > tolerance {
            1
        } else if cross < -tolerance {
            -1
        } else {
            0
        }
    };
    for i in 0..n {
        let (a, b) = (points[i], points[(i + 1) % n]);
        if (b[0] - a[0]).hypot(b[1] - a[1]) <= COORD_FLOOR {
            return false;
        }
        if side(a, b, points[(i + 2) % n]) == 0 {
            return false;
        }
        for j in i + 1..n {
            let (c, d) = (points[j], points[(j + 1) % n]);
            if (a[0] - c[0]).hypot(a[1] - c[1]) <= COORD_FLOOR {
                return false;
            }
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            let boxes_overlap = (0..2).all(|axis| {
                a[axis].min(b[axis]) <= c[axis].max(d[axis]) + COORD_FLOOR
                    && c[axis].min(d[axis]) <= a[axis].max(b[axis]) + COORD_FLOOR
            });
            if boxes_overlap
                && side(a, b, c) * side(a, b, d) <= 0
                && side(c, d, a) * side(c, d, b) <= 0
            {
                return false;
            }
        }
    }
    true
}

/// Preserve every genuine corner; only remove vertices strictly inside a straight edge.
fn polygon(part: &Part, face: usize) -> Option<Vec<V3>> {
    if part.faces[face].loops.len() != 1
        || part
            .outer_edges(face)
            .iter()
            .any(|&e| !matches!(part.edges[e].curve, Curve::Line { .. }))
    {
        return None;
    }
    let vertices = outer_vertices(part, face);
    let n = vertices.len();
    // The legacy vertex walk deduplicates IDs. A repeated corner here is an
    // ambiguous/self-touching boundary, rather than a vertex to silently discard.
    if n < 5 || n != part.faces[face].loops[part.outer_loop(face)?].edges.len() {
        return None;
    }
    let corners: Vec<_> = (0..n)
        .filter_map(|i| {
            let (a, p, b) = (
                vertices[(i + n - 1) % n],
                vertices[i],
                vertices[(i + 1) % n],
            );
            let chord = sub(b, a);
            (!(dot(sub(p, a), sub(b, p)) >= 0.0
                && norm(cross(sub(p, a), chord)) <= COORD_FLOOR * norm(chord)))
            .then_some(p)
        })
        .collect();
    (corners.len() > 4).then_some(corners)
}

/// Exactly one exterior support owns each entire non-seam edge. Nonmanifold,
/// concave, or competing boundary supports do not establish an open boundary.
/// A curved support may trim a straight polygon edge; its normal is not a datum.
/// The measured far lip and both run terminals are separately proved planar.
fn boundary_supports(
    part: &Part,
    face: usize,
    other: usize,
    solid: &[usize],
) -> Option<Vec<usize>> {
    let mut supports = Vec::new();
    for e in part.face_edges(face) {
        let adjacent: Vec<_> = part
            .neighbours(face)
            .into_iter()
            .filter(|&f| part.face_edges(f).contains(&e))
            .collect();
        let [f] = adjacent[..] else {
            return None;
        };
        if f == other {
            continue;
        }
        if !solid.contains(&f) || part.arc(face, f) != Some(Arc::Convex) {
            return None;
        }
        supports.push(f);
    }
    supports.sort_unstable();
    supports.dedup();
    Some(supports)
}

fn one_pair(
    ctx: &Context<'_>,
    s: usize,
    principal: usize,
    oblique: usize,
    bounds: &Bounds,
    solid: &[usize],
) -> Option<Occurrence<PolygonalObliqueThroughStep>> {
    let part = ctx.part;
    if part.arc(principal, oblique) != Some(Arc::Concave)
        || principal_plane(part, oblique).is_some()
    {
        return None;
    }
    let (axis, _) = principal_plane(part, principal)?;
    let vertices = polygon(part, principal)?;
    if part.faces[oblique].loops.len() != 1 {
        return None;
    }
    if outer_vertices(part, oblique).len()
        != part.faces[oblique].loops[part.outer_loop(oblique)?]
            .edges
            .len()
    {
        return None;
    }
    let wall = linear_outer_quad(part, oblique)?;
    let normal = unit_plane_normal(part, principal)?;
    let across = unit_plane_normal(part, oblique)?;
    if dot(normal, across).abs() > SMOOTH_ARC_GAP {
        return None;
    }
    let (start, end) = seam_endpoints(part, principal, oblique)?;
    let length = norm(sub(end, start));
    let run = normalized(sub(end, start))?;
    if dot(run, normal).abs() > SMOOTH_ARC_GAP || dot(run, across).abs() > SMOOTH_ARC_GAP {
        return None;
    }
    // Through runs cross opposite sides of the same stock axis. Adjacent stock
    // sides bound a triangular blind-step wedge, even when another cut adds corners.
    let end_axes: Vec<_> = (0..3)
        .filter(|&a| {
            a != axis
                && ((near(start[a], bounds.min[a]) && near(end[a], bounds.max[a]))
                    || (near(start[a], bounds.max[a]) && near(end[a], bounds.min[a])))
        })
        .collect();
    let [end_axis] = end_axes[..] else {
        return None;
    };
    let depth = wall
        .iter()
        .map(|&v| dot(sub(v, start), normal))
        .fold(f64::NEG_INFINITY, f64::max);
    if depth <= COORD_FLOOR {
        return None;
    }
    // All four distinct corners must be those of the complete oblique rectangle.
    for along in [0.0, length] {
        for deep in [0.0, depth] {
            let expected = add(start, add(scale(run, along), scale(normal, deep)));
            if wall
                .iter()
                .filter(|&&p| norm(sub(p, expected)) <= COORD_FLOOR)
                .count()
                != 1
            {
                return None;
            }
        }
    }
    let wall_outline: Vec<_> = wall
        .iter()
        .map(|&p| {
            let offset = sub(p, start);
            [dot(offset, run), dot(offset, normal)]
        })
        .collect();
    if !simple_outline(&wall_outline) {
        return None;
    }
    let principal_supports = boundary_supports(part, principal, oblique, solid)?;
    let wall_supports = boundary_supports(part, oblique, principal, solid)?;
    // The full opposite edge has exactly one parallel, same-facing convex lip.
    // This allows a secondary cut's local opening, without inferring a stock envelope.
    let lips: Vec<_> = wall_supports
        .iter()
        .copied()
        .filter(|&f| {
            matches!(part.faces[f].surface, Surface::Plane { .. })
                && unit_plane_normal(part, f)
                    .is_some_and(|n| norm(sub(n, normal)) <= SMOOTH_ARC_GAP)
                && seam_endpoints(part, oblique, f).is_some_and(|(a, b)| {
                    norm(sub(a, add(start, scale(normal, depth)))) <= COORD_FLOOR
                        && norm(sub(b, add(end, scale(normal, depth)))) <= COORD_FLOOR
                })
        })
        .collect();
    let [lip] = lips[..] else {
        return None;
    };
    let mut context = principal_supports.clone();
    context.extend(&wall_supports);
    for endpoint in [start, end] {
        let far = add(endpoint, scale(normal, depth));
        let ends: Vec<_> = principal_supports
            .iter()
            .copied()
            .filter(|f| {
                wall_supports.contains(f)
                    && *f != lip
                    && principal_plane(part, *f)
                        .is_some_and(|(a, c)| a == end_axis && near(c, endpoint[a]))
                    && part.shared_edges(oblique, *f).iter().any(|&e| {
                        let edge = &part.edges[e];
                        (norm(sub(edge.start, endpoint)) <= COORD_FLOOR
                            && norm(sub(edge.end, far)) <= COORD_FLOOR)
                            || (norm(sub(edge.end, endpoint)) <= COORD_FLOOR
                                && norm(sub(edge.start, far)) <= COORD_FLOOR)
                    })
            })
            .collect();
        if ends.len() != 1 {
            return None;
        }
    }
    // Keep the stock-axis staircase rule, also admitting a diagonal boundary
    // actually owned by a proved convex chamfer. This uses the source edge and
    // treatment evidence, rather than interpreting an arbitrary skew lip as a step.
    let seam_edges = part.shared_edges(principal, oblique);
    for e in part.outer_edges(principal) {
        if seam_edges.contains(&e) {
            continue;
        }
        let direction = normalized(sub(part.edges[e].end, part.edges[e].start))?;
        let principal_edge = (0..3)
            .filter(|&a| a != axis)
            .any(|a| 1.0 - direction[a].abs() <= SMOOTH_ARC_GAP);
        let chamfer_edge = principal_supports.iter().any(|&support| {
            part.shared_edges(principal, support).contains(&e)
                && ctx
                    .reported_chamfers()
                    .iter()
                    .any(|treatment| treatment.defining.contains(&support))
        });
        if !principal_edge && !chamfer_edge {
            return None;
        }
    }
    if vertices
        .iter()
        .any(|&v| dot(sub(v, start), across) < -COORD_FLOOR)
        || !vertices
            .iter()
            .any(|&v| dot(sub(v, start), across) > COORD_FLOOR)
    {
        return None;
    }
    let raw_outline: Vec<_> = vertices
        .iter()
        .map(|&v| {
            let offset = sub(v, start);
            [dot(offset, run), dot(offset, across)]
        })
        .collect();
    let outline: Vec<_> = raw_outline.iter().map(|p| p.map(py::round_to3)).collect();
    if !simple_outline(&raw_outline) || !simple_outline(&outline) {
        return None;
    }
    let inset = length_tol(depth, 1e-6).min(depth / 4.0);
    if ctx.swept_face_volume(
        s,
        principal,
        scale(normal, inset),
        scale(normal, depth - 2.0 * inset),
    )? != 0.0
    {
        return None;
    }
    context.sort_unstable();
    context.dedup();
    Some(Occurrence {
        record: PolygonalObliqueThroughStep {
            run: coordinates(run, 6),
            length: py::round_to3(length),
            at: Placed::new(scale(add(start, end), 0.5), Grid::CleanDecimals(3)),
            depth_direction: coordinates(normal, 6),
            depth: py::round_to3(depth),
            across_direction: coordinates(across, 6),
            through_axis: std::array::from_fn(|a| if a == end_axis { 1.0 } else { 0.0 }),
            slant_degrees: py::round_to(run[end_axis].abs().clamp(0.0, 1.0).acos().to_degrees(), 6),
            floor_outline: canonical_outline(&outline),
            body_key: None,
        },
        defining: vec![principal, oblique],
        context,
    })
}

internal_entry! {
pub fn discover_verified(ctx: &Context<'_>) -> Result<Vec<Occurrence<PolygonalObliqueThroughStep>>, EvidenceError> {
    evidence::verified(ctx.part,discover(ctx))
}
}

internal_entry! {
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<PolygonalObliqueThroughStep>> {
    let part = ctx.part;
    let mut found = Vec::new();
    for (s,solid) in part.solids.iter().enumerate() {
        if !part.solid_is_geometrically_valid(s) { continue; }
        let bounds = part.solid_bounds(s);
        for &principal in &solid.faces {
            if principal_plane(part,principal).is_none() { continue; }
            for oblique in part.neighbours(principal) {
                if !solid.faces.contains(&oblique) { continue; }
                if let Some(record) = one_pair(ctx,s,principal,oblique,&bounds,&solid.faces) { found.push((s,record)); }
            }
        }
    }
    let keys = if found.is_empty() { vec![] } else { ctx.body_keys(true) };
    let mut found: Vec<_> = found.into_iter().map(|(s,mut occurrence)| { occurrence.record.body_key = keys[s].clone(); occurrence }).collect();
    found.sort_by(|a,b| {
        let key = |r: &PolygonalObliqueThroughStep| { let mut k = r.run.to_vec(); k.push(r.length); k.extend(*r.at); k.extend(r.depth_direction); k.push(r.depth); k.extend(r.across_direction); k.extend(r.floor_outline.iter().flatten()); k.extend(r.body_key.iter().flatten()); k };
        py::tuple_order(&key(&a.record),&key(&b.record))
    });
    found
}
}

#[cfg(test)]
mod tests {
    use super::simple_outline;

    #[test]
    fn crossed_touching_and_collapsed_polygons_are_ambiguous() {
        assert!(simple_outline(&[
            [0., 0.],
            [4., 0.],
            [4., 1.],
            [2., 1.],
            [2., 3.],
            [0., 3.]
        ]));
        assert!(!simple_outline(&[
            [0., 0.],
            [4., 3.],
            [0., 3.],
            [4., 0.],
            [4., 1.],
            [2., 1.]
        ]));
        assert!(!simple_outline(&[
            [0., 0.],
            [4., 0.],
            [4., 3.],
            [2., 0.],
            [0., 3.]
        ]));
        assert!(!simple_outline(&[
            [0., 0.],
            [4., 0.],
            [4., 0.],
            [4., 3.],
            [0., 3.]
        ]));
    }
}
