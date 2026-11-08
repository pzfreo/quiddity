//! Blind annular face grooves: a planar annulus between two complete coaxial cylindrical
//! walls, both rising to the same open plane. Coplanar floor splits and circular edge splits
//! preserve the feature. Interrupted walls and treated rims remain refused.

use super::Context;
use super::evidence::{self, EvidenceStatus, Occurrence};
use super::graph::normal;
use super::regions::{edge_length, length_tolerance};
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::{self, Curve, Surface, V3};
use crate::kernel::sampling::edge_interval;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::TAU;

/// A rectangular-section annular recess, in millimetres. `axis` is the unit direction from
/// `floor_center` towards the mouth; both cylinder walls have the same `depth`. No nominal
/// seal dimensions, machining process or intended function are inferred.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AnnularGroove {
    pub axis: V3,
    pub floor_center: V3,
    pub inner_radius: f64,
    pub outer_radius: f64,
    pub depth: f64,
}

#[derive(Clone)]
pub(super) struct Ring {
    pub(super) edges: Vec<usize>,
    pub(super) center: V3,
    pub(super) radius: f64,
}

/// Edge identity, not a nearest-point walk, proves disjoint closed boundary cycles. A
/// branch, a touching pair of cycles, or more than one successor refuses.
fn cycles(part: &Part, edges: &BTreeSet<usize>) -> Option<Vec<Vec<usize>>> {
    let mut incident: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for &edge in edges {
        let (a, b) = part.edges[edge].vertices;
        incident.entry(a).or_default().push(edge);
        incident.entry(b).or_default().push(edge);
    }
    if incident.values().any(|v| v.len() != 2) {
        return None;
    }
    let mut pending = edges.clone();
    let mut found = Vec::new();
    while let Some(edge) = pending.pop_first() {
        let (start, mut at) = part.edges[edge].vertices;
        let mut cycle = vec![edge];
        while at != start {
            let next: Vec<_> = incident[&at]
                .iter()
                .copied()
                .filter(|e| pending.contains(e))
                .collect();
            let [next] = next[..] else { return None };
            pending.remove(&next);
            cycle.push(next);
            let (a, b) = part.edges[next].vertices;
            at = if a == at { b } else { a };
        }
        found.push(cycle);
    }
    Some(found)
}

/// A whole circular rim, possibly split into arcs. Closed topology plus a one-turn arc sum
/// proves the complete circumference; matching radii/centres alone would admit fragments.
pub(super) fn ring(part: &Part, edges: Vec<usize>, axis: V3) -> Option<Ring> {
    let set: BTreeSet<_> = edges.iter().copied().collect();
    if set.len() != edges.len() || cycles(part, &set)?.len() != 1 {
        return None;
    }
    let first = *edges.first()?;
    let Curve::Circle { frame, radius } = part.edges[first].curve else {
        return None;
    };
    let tolerance = length_tolerance(&[radius]);
    if radius <= tolerance {
        return None;
    }
    for &edge in &edges {
        let Curve::Circle {
            frame: other,
            radius: r,
        } = part.edges[edge].curve
        else {
            return None;
        };
        if geom::dot(other.z, axis).abs() < 1. - 1e-9
            || (r - radius).abs() > tolerance
            || geom::dist(frame.origin, other.origin) > tolerance
        {
            return None;
        }
    }
    let length: f64 = edges.iter().map(|&e| edge_length(part, e)).sum();
    if (length - TAU * radius).abs() > length_tolerance(&[TAU * radius]) {
        return None;
    }
    // Unsigned arc length alone could count the same semicircle twice. Require the
    // union of the actual parameter intervals to cover the whole circle as well.
    let angular_tolerance = length_tolerance(&[TAU * radius]) / radius;
    let mut intervals = Vec::new();
    for &edge in &edges {
        let e = &part.edges[edge];
        let Curve::Circle { frame: other, .. } = e.curve else {
            unreachable!()
        };
        let (a, b) = edge_interval(&e.curve, e.start, e.end, e.same_sense, e.is_closed());
        let orientation =
            geom::dot(geom::cross(frame.x, frame.y), geom::cross(other.x, other.y)).signum();
        let span = (b - a) * orientation;
        let p = frame.to_local(e.start);
        let start = p[1].atan2(p[0]);
        let low = (start + span.min(0.)).rem_euclid(TAU);
        let high = low + span.abs();
        if span.abs() >= TAU - angular_tolerance {
            intervals.push((0., TAU));
        } else if high > TAU {
            intervals.extend([(low, TAU), (0., high - TAU)]);
        } else {
            intervals.push((low, high));
        }
    }
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut covered = 0.0_f64;
    for (low, high) in intervals {
        if low > covered + angular_tolerance {
            return None;
        }
        covered = covered.max(high);
    }
    if covered < TAU - angular_tolerance {
        return None;
    }
    Some(Ring {
        edges,
        center: frame.origin,
        radius,
    })
}

/// The connected floor plane, without a preferred coordinate axis.
pub(super) fn floor_region(part: &Part, seed: usize) -> Option<BTreeSet<usize>> {
    let Surface::Plane { frame } = part.faces[seed].surface else {
        return None;
    };
    let axis = normal(part, seed)?;
    let tolerance = length_tolerance(&[part.face_bounds(seed).max_extent()]);
    let mut found = BTreeSet::from([seed]);
    let mut pending = vec![seed];
    while let Some(face) = pending.pop() {
        for next in part.neighbours(face) {
            if found.contains(&next) {
                continue;
            }
            let Surface::Plane { frame: other } = part.faces[next].surface else {
                continue;
            };
            if geom::dot(normal(part, next)?, axis) < 1. - 1e-9
                || geom::dot(geom::sub(other.origin, frame.origin), axis).abs() > tolerance
                || part.arc(face, next) != Some(Arc::Smooth)
            {
                continue;
            }
            found.insert(next);
            pending.push(next);
        }
    }
    Some(found)
}

pub(super) fn floor_rings(part: &Part, floor: &BTreeSet<usize>, axis: V3) -> Option<Vec<Ring>> {
    let mut uses: BTreeMap<usize, Vec<bool>> = BTreeMap::new();
    for &face in floor {
        for lp in &part.faces[face].loops {
            for &(edge, forward) in &lp.edges {
                uses.entry(edge).or_default().push(forward);
            }
        }
    }
    let mut boundary = BTreeSet::new();
    for (edge, directions) in uses {
        match directions[..] {
            [_] => {
                boundary.insert(edge);
            }
            [a, b] if a != b => {}
            _ => return None,
        }
    }
    let cycles = cycles(part, &boundary)?;
    if cycles.len() != 2 {
        return None;
    }
    cycles
        .into_iter()
        .map(|edges| ring(part, edges, axis))
        .collect()
}

/// One complete cylinder wall, with one straight seam used in opposite senses. Floor and
/// mouth rims may consist of multiple arcs; no additional boundary is accepted.
fn wall(
    part: &Part,
    floor: &BTreeSet<usize>,
    rim: &Ring,
    axis: V3,
    outer: bool,
) -> Option<(usize, Vec<usize>, f64)> {
    let mut owner = None;
    for &edge in &rim.edges {
        let outside: Vec<_> = part.edge_faces()[edge]
            .iter()
            .copied()
            .filter(|f| !floor.contains(f))
            .collect();
        let [face] = outside[..] else { return None };
        if owner.is_some_and(|previous| previous != face) {
            return None;
        }
        owner = Some(face);
        for &f in &part.edge_faces()[edge] {
            if floor.contains(&f) && part.arc(f, face) != Some(Arc::Concave) {
                return None;
            }
        }
    }
    let face = owner?;
    let f = &part.faces[face];
    let Surface::Cylinder { frame, radius } = f.surface else {
        return None;
    };
    let tolerance = length_tolerance(&[radius, rim.radius]);
    if f.reversed != outer
        || geom::dot(frame.z, axis).abs() < 1. - 1e-9
        || geom::norm(geom::cross(geom::sub(rim.center, frame.origin), axis)) > tolerance
        || (radius - rim.radius).abs() > tolerance
    {
        return None;
    }
    let [lp] = &f.loops[..] else { return None };
    let mut uses: BTreeMap<usize, Vec<bool>> = BTreeMap::new();
    for &(edge, forward) in &lp.edges {
        uses.entry(edge).or_default().push(forward);
    }
    for edge in &rim.edges {
        if uses.remove(edge)?.len() != 1 {
            return None;
        }
    }
    let mut other_edges = Vec::new();
    let mut seams = 0;
    for (edge, directions) in uses {
        match part.edges[edge].curve {
            Curve::Circle { .. } if directions.len() == 1 => other_edges.push(edge),
            Curve::Line { dir, .. }
                if directions.len() == 2
                    && directions[0] != directions[1]
                    && geom::dot(geom::unit(dir)?, axis).abs() > 1. - 1e-9 =>
            {
                seams += 1
            }
            _ => return None,
        }
    }
    if seams != 1 {
        return None;
    }
    let other = ring(part, other_edges, axis)?;
    let delta = geom::sub(other.center, rim.center);
    let depth = geom::dot(delta, axis);
    if depth <= tolerance
        || geom::norm(geom::cross(delta, axis)) > tolerance
        || (other.radius - rim.radius).abs() > tolerance
    {
        return None;
    }
    let mut context = Vec::new();
    for edge in other.edges {
        let [a, b] = part.edge_faces()[edge][..] else {
            return None;
        };
        let mouth = if a == face {
            b
        } else if b == face {
            a
        } else {
            return None;
        };
        if !matches!(part.faces[mouth].surface, Surface::Plane { .. })
            || geom::dot(normal(part, mouth)?, axis) < 1. - 1e-9
            || part.arc(face, mouth) != Some(Arc::Convex)
        {
            return None;
        }
        context.push(mouth);
    }
    Some((face, context, depth))
}

fn from_floor(part: &Part, floor: &BTreeSet<usize>) -> Option<Occurrence<AnnularGroove>> {
    let axis = normal(part, *floor.first()?)?;
    let mut rings = floor_rings(part, floor, axis)?;
    rings.sort_by(|a, b| a.radius.total_cmp(&b.radius));
    let (inner, outer) = (&rings[0], &rings[1]);
    let tolerance = length_tolerance(&[outer.radius]);
    if outer.radius - inner.radius <= tolerance
        || geom::dist(inner.center, outer.center) > tolerance
    {
        return None;
    }
    let (iw, mut context, depth) = wall(part, floor, inner, axis, false)?;
    let (ow, mouth, other_depth) = wall(part, floor, outer, axis, true)?;
    if (depth - other_depth).abs() > length_tolerance(&[depth, other_depth]) {
        return None;
    }
    let mut defining: Vec<_> = floor.iter().copied().chain([iw, ow]).collect();
    defining.sort_unstable();
    defining.dedup();
    context.extend(mouth);
    context.sort_unstable();
    context.dedup();
    if defining.len() != floor.len() + 2
        || context.iter().any(|f| defining.contains(f))
        || evidence::strict_status(part, &defining, &context) != EvidenceStatus::Verified
    {
        return None;
    }
    Some(Occurrence {
        record: AnnularGroove {
            axis,
            floor_center: outer.center,
            inner_radius: inner.radius,
            outer_radius: outer.radius,
            depth: 0.5 * (depth + other_depth),
        },
        defining,
        context,
    })
}

internal_entry! {
/// Complete annular recesses on one strictly valid solid; all defining claims competing for
/// a source face are refused. The first floor-face traversal order is retained.
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<AnnularGroove>> {
    let mut seen=BTreeSet::<usize>::new();let mut found=Vec::new();
    for seed in 0..ctx.part.faces.len() {
        if seen.contains(&seed) || !matches!(ctx.part.faces[seed].surface,Surface::Plane{..})
            || !ctx.part.face_edges(seed).iter().any(|&e|matches!(ctx.part.edges[e].curve,Curve::Circle{..})) { continue }
        let Some(floor)=floor_region(ctx.part,seed) else { continue };
        seen.extend(floor.iter().copied());
        if let Some(o)=from_floor(ctx.part,&floor) { found.push(o); }
    }
    let mut owners=BTreeMap::<usize,usize>::new();
    for o in &found { for &face in &o.defining { *owners.entry(face).or_default()+=1; } }
    found.into_iter().filter(|o|o.defining.iter().all(|f|owners[f]==1)).collect()
}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::brep::{Edge, Face, Loop, Solid};
    use crate::kernel::geom::Frame;
    use crate::kernel::step::IDENTITY;
    use std::f64::consts::TAU;

    /// A turned cylinder with an independently constructed 5..8 mm radial, 4 mm deep
    /// face groove. The core and outer land share the same mouth plane.
    fn seal_housing() -> Part {
        seal_housing_with_core_height(10.)
    }

    fn seal_housing_with_core_height(core_height: f64) -> Part {
        let frame = |z| Frame {
            origin: [0., 0., z],
            x: [1., 0., 0.],
            y: [0., 1., 0.],
            z: [0., 0., 1.],
        };
        let rings = [
            (12., 0.),
            (12., 10.),
            (8., 10.),
            (8., 6.),
            (5., core_height),
            (5., 6.),
        ];
        let mut edges: Vec<_> = rings
            .iter()
            .enumerate()
            .map(|(i, &(radius, z))| {
                let curve = Curve::Circle {
                    frame: frame(z),
                    radius,
                };
                Edge {
                    start: curve.value(0.),
                    end: curve.value(0.),
                    samples: (0..=32)
                        .map(|j| curve.value(TAU * j as f64 / 32.))
                        .collect(),
                    curve,
                    vertices: (i, i),
                    same_sense: true,
                }
            })
            .collect();
        for (bottom, top) in [(0, 1), (3, 2), (5, 4)] {
            let (start, end) = (edges[bottom].start, edges[top].start);
            edges.push(Edge {
                curve: Curve::Line {
                    origin: start,
                    dir: [0., 0., 1.],
                },
                start,
                end,
                vertices: (bottom, top),
                same_sense: true,
                samples: vec![start, end],
            });
        }
        let face = |surface, reversed, boundaries: Vec<Vec<(usize, bool)>>| Face {
            surface,
            reversed,
            solid: Some(0),
            pcurves: Vec::new(),
            loops: boundaries
                .into_iter()
                .map(|edges| Loop {
                    edges,
                    vertex: None,
                })
                .collect(),
        };
        let cylinder = |radius| Surface::Cylinder {
            frame: frame(0.),
            radius,
        };
        let plane = |z| Surface::Plane { frame: frame(z) };
        let faces = vec![
            face(
                cylinder(12.),
                false,
                vec![vec![(0, true), (6, true), (1, false), (6, false)]],
            ),
            face(plane(10.), false, vec![vec![(1, true)], vec![(2, false)]]),
            face(
                cylinder(8.),
                true,
                vec![vec![(3, false), (7, true), (2, true), (7, false)]],
            ),
            face(plane(6.), false, vec![vec![(3, true)], vec![(5, false)]]),
            face(
                cylinder(5.),
                false,
                vec![vec![(5, true), (8, true), (4, false), (8, false)]],
            ),
            face(plane(core_height), false, vec![vec![(4, true)]]),
            face(plane(0.), true, vec![vec![(0, false)]]),
        ];
        Part::new(
            faces,
            edges,
            vec![Solid {
                faces: (0..7).collect(),
            }],
        )
    }

    #[test]
    fn face_seal_groove_has_measured_dimensions_and_strict_evidence_after_motion() {
        let base = seal_housing();
        assert!(base.solid_is_geometrically_valid(0));
        let angle: f64 = 0.37;
        let (s, c) = angle.sin_cos();
        for motion in [
            IDENTITY,
            [[c, 0., s, 123.456], [0., 1., 0., -78.9], [-s, 0., c, 41.3]],
        ] {
            let part = base.transformed(&motion).unwrap();
            let found = discover(&Context::new(&part));
            assert_eq!(found.len(), 1);
            let o = &found[0];
            assert_eq!(o.defining, [2, 3, 4]);
            assert_eq!(o.context, [1, 5]);
            assert_eq!(o.record.inner_radius, 5.);
            assert_eq!(o.record.outer_radius, 8.);
            assert!((o.record.depth - 4.).abs() < 1e-9);
            for (i, row) in motion.iter().enumerate() {
                assert!((o.record.axis[i] - row[2]).abs() < 1e-9);
                assert!((o.record.floor_center[i] - (6. * row[2] + row[3])).abs() < 1e-9);
            }
            let recognition = crate::correspondence::recognise(&part);
            let doc = crate::recognition::document(&recognition, part.faces.len());
            let records: Vec<_> = doc
                .records
                .iter()
                .filter(|r| r.family == "annular_grooves")
                .collect();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].faces, [2, 3, 4]);
            assert_eq!(records[0].evidence_status, EvidenceStatus::Verified);
            assert_eq!(doc.schema_version, "quiddity-rust/recognition/6");
            for (family, wall) in [("holes", 2), ("bosses", 4)] {
                let legacy: Vec<_> = doc
                    .records
                    .iter()
                    .filter(|r| r.family == family && r.faces.contains(&wall))
                    .collect();
                assert_eq!(legacy.len(), 1);
                assert_eq!(legacy[0].parameters["superseded_by"], "annular_grooves/0");
                assert_eq!(legacy[0].faces, [wall]);
            }

            let axis: V3 = serde_json::from_value(records[0].parameters["axis"].clone()).unwrap();
            let center: V3 =
                serde_json::from_value(records[0].parameters["floor_center"].clone()).unwrap();
            assert!(geom::dist(axis, o.record.axis) < 1e-9);
            assert!(geom::dist(center, o.record.floor_center) < 1e-9);
        }
    }

    #[test]
    fn unequal_wall_depths_keep_unmarked_hole_and_boss() {
        let part = seal_housing_with_core_height(9.);
        assert!(part.solid_is_geometrically_valid(0));
        let found = crate::features::recognise(&part);
        assert!(found.annular_grooves.is_empty());
        assert!(found.holes.iter().any(|h| h.diameter == 16.));
        assert!(found.bosses.iter().any(|b| b.diameter == 10.));
        for record in found.holes {
            assert!(record.superseded_by.is_none());
            assert!(
                serde_json::to_value(record)
                    .unwrap()
                    .get("superseded_by")
                    .is_none()
            );
        }
        for record in found.bosses {
            assert!(record.superseded_by.is_none());
            assert!(
                serde_json::to_value(record)
                    .unwrap()
                    .get("superseded_by")
                    .is_none()
            );
        }
    }

    #[test]
    fn a_double_covered_semicircle_is_not_a_complete_rim() {
        let frame = Frame {
            origin: [0.; 3],
            x: [1., 0., 0.],
            y: [0., 1., 0.],
            z: [0., 0., 1.],
        };
        let curve = Curve::Circle { frame, radius: 8. };
        let edge = |a, b, vertices, same_sense| Edge {
            curve: curve.clone(),
            start: a,
            end: b,
            vertices,
            same_sense,
            samples: Vec::new(),
        };
        for (sense, complete) in [(false, false), (true, true)] {
            let part = Part::new(
                Vec::new(),
                vec![
                    edge([8., 0., 0.], [-8., 0., 0.], (0, 1), true),
                    edge([-8., 0., 0.], [8., 0., 0.], (1, 0), sense),
                ],
                Vec::new(),
            );
            assert_eq!(ring(&part, vec![0, 1], [0., 0., 1.]).is_some(), complete);
        }
    }

    #[test]
    fn split_floor_and_split_rim_preserve_the_whole_annular_feature() {
        let base = seal_housing();
        let mut faces = base.faces.clone();
        let mut edges = base.edges.clone();
        let Curve::Circle { frame, radius } = edges[3].curve else {
            unreachable!()
        };
        let angle = (-0.75_f64).acos();
        let arc = |lo: f64, hi: f64, vertices| Edge {
            curve: Curve::Circle { frame, radius },
            start: frame.to_world([radius * lo.cos(), radius * lo.sin(), 0.]),
            end: frame.to_world([radius * hi.cos(), radius * hi.sin(), 0.]),
            vertices,
            same_sense: true,
            samples: (0..=32)
                .map(|i| {
                    let a = lo + (hi - lo) * i as f64 / 32.;
                    frame.to_world([radius * a.cos(), radius * a.sin(), 0.])
                })
                .collect(),
        };
        edges[3] = arc(0., angle, (3, 6));
        let b = edges.len();
        edges.push(arc(angle, TAU - angle, (6, 7)));
        let c = edges.len();
        edges.push(arc(TAU - angle, TAU, (7, 3)));
        let (start, end) = (edges[3].end, edges[b].end);
        let chord = edges.len();
        edges.push(Edge {
            curve: Curve::Line {
                origin: start,
                dir: geom::sub(end, start),
            },
            start,
            end,
            vertices: (6, 7),
            same_sense: true,
            samples: vec![start, end],
        });
        faces[2].loops[0].edges = vec![
            (c, false),
            (b, false),
            (3, false),
            (7, true),
            (2, true),
            (7, false),
        ];
        faces[3].loops[0].edges = vec![(3, true), (chord, true), (c, true)];
        let mut patch = faces[3].clone();
        patch.loops = vec![Loop {
            edges: vec![(b, true), (chord, false)],
            vertex: None,
        }];
        faces.push(patch);
        let part = Part::new(
            faces,
            edges,
            vec![Solid {
                faces: (0..8).collect(),
            }],
        );
        assert!(part.solid_is_geometrically_valid(0));
        let (s, c) = 0.37_f64.sin_cos();
        for motion in [
            IDENTITY,
            [[c, 0., s, 123.456], [0., 1., 0., -78.9], [-s, 0., c, 41.3]],
        ] {
            let moved = part.transformed(&motion).unwrap();
            let found = discover(&Context::new(&moved));
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].defining, [2, 3, 4, 7]);
            assert!((found[0].record.depth - 4.).abs() < 1e-9);
            assert_eq!(found[0].record.inner_radius, 5.);
            assert_eq!(found[0].record.outer_radius, 8.);
        }
    }

    #[test]
    fn reversed_material_missing_core_and_interrupted_walls_do_not_become_grooves() {
        for fault in 0..4 {
            let mut part = seal_housing();
            match fault {
                0 => part.faces[2].reversed = false,
                1 => part.faces[3].loops.pop().map(|_| ()).unwrap(),
                2 => part.faces[2].loops[0].edges.pop().map(|_| ()).unwrap(),
                _ => part.faces[4].solid = None,
            }
            assert!(discover(&Context::new(&part)).is_empty(), "fault {fault}");
        }
    }
}
