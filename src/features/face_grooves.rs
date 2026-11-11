//! Intrinsic annular recesses in a planar face (#123). The wall surfaces establish the
//! axis; neither a principal frame nor a turned-body classification is required.
use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::annular_grooves::{Ring, floor_region, floor_rings, ring};
use super::evidence::{self, EvidenceStatus, Occurrence};
use super::graph::normal;
use super::regions::length_tolerance;
use super::{Context, Features, Kept, kept};
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::{self, Surface, V3};

/// Diameters are measured at the actual mouth, including any rim treatment. Angles are
/// degrees to the axis (90 for a cylindrical wall). `axis_point` is the mouth centre and
/// `axis` points from floor to mouth. Separate occurrences are never counted as a pattern.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct FaceGroove {
    pub axis: V3,
    pub axis_point: V3,
    pub outer_diameter: f64,
    pub inner_diameter: f64,
    pub depth: f64,
    pub outer_wall_angle: f64,
    pub inner_wall_angle: f64,
    pub floor: Vec<usize>,
    pub faces: Vec<usize>,
}

/// Publish after the existing families, including residual profiles: adding this family
/// must never change an existing record. Body classifications retain co-ownership.
pub(super) fn append(ctx: &Context<'_>, features: &mut Features) {
    let claimed: BTreeSet<_> = features
        .defining
        .iter()
        .filter(|(family, _)| {
            !matches!(
                **family,
                "thin_wall_bodies"
                    | "sheet_metal_bodies"
                    | "plates"
                    | "polygonal_stock"
                    | "turned_bodies"
            )
        })
        .flat_map(|(_, records)| records.iter().flatten().copied())
        .chain(
            features
                .discovery_refusals
                .values()
                .flatten()
                .flat_map(|r| r.faces.iter().copied()),
        )
        .chain(
            features
                .interior_void_refusals
                .iter()
                .flat_map(|r| r.candidate.void_faces.iter().copied()),
        )
        .chain(
            features
                .modelled_thread_refusals
                .iter()
                .flat_map(|r| r.faces.iter().copied()),
        )
        .chain(
            features
                .section_recess_refusals
                .iter()
                .flat_map(|r| r.evidence().defining_faces().iter().copied()),
        )
        .collect();
    let found: Vec<_> = discover(ctx)
        .into_iter()
        .filter(|o| o.defining.iter().all(|f| !claimed.contains(f)))
        .collect();
    if found.is_empty() {
        return;
    }
    let mut registration = Kept {
        part: ctx.part,
        claimed: Default::default(),
        defining: Default::default(),
        constituent: Default::default(),
        evidence_status: Default::default(),
    };
    features.face_grooves = kept(&mut registration, "face_grooves", found);
    features.defining.extend(registration.defining);
    features
        .evidence_status
        .extend(registration.evidence_status);
}

/// Each edge on a complete circular interface must lead to the same next face.
fn across(part: &Part, rim: &Ring, previous: &BTreeSet<usize>) -> Option<usize> {
    let mut owner = None;
    for &edge in &rim.edges {
        let outside: Vec<_> = part.edge_faces()[edge]
            .iter()
            .copied()
            .filter(|f| !previous.contains(f))
            .collect();
        let [face] = outside[..] else { return None };
        if owner.is_some_and(|old| old != face) {
            return None;
        }
        owner = Some(face);
    }
    owner
}

/// A complete surface-of-revolution band has exactly two whole circular rims and a seam
/// used twice in opposite senses. No extra opening, interrupted wall or partial arc passes.
fn other_rim(part: &Part, face: usize, incoming: &Ring, axis: V3) -> Option<Ring> {
    let [lp] = &part.faces[face].loops[..] else {
        return None;
    };
    let mut uses = BTreeMap::<usize, Vec<bool>>::new();
    for &(edge, forward) in &lp.edges {
        uses.entry(edge).or_default().push(forward);
    }
    for edge in &incoming.edges {
        if uses.remove(edge)?.len() != 1 {
            return None;
        }
    }
    let mut outgoing = Vec::new();
    let mut seams = 0;
    for (edge, directions) in uses {
        match directions[..] {
            [_] => outgoing.push(edge),
            [a, b] if a != b => seams += 1,
            _ => return None,
        }
    }
    if seams != 1 {
        return None;
    }
    ring(part, outgoing, axis)
}

struct Wall {
    faces: BTreeSet<usize>,
    mouth: Ring,
    context: BTreeSet<usize>,
    angle: f64,
    axis: V3,
}

/// Walk one meridian from floor to open land. Tori must be tangent at both ends and
/// confined to a meridian arc shorter than a half turn; cones may also act as small
/// floor/rim chamfers. The longest axial cylinder/cone segment defines the wall angle;
/// a tie must agree.
fn wall(part: &Part, floor: &BTreeSet<usize>, start: &Ring, outer: bool) -> Option<Wall> {
    let floor_axis = normal(part, *floor.first()?)?;
    let first = across(part, start, floor)?;
    let frame = match part.faces[first].surface {
        Surface::Cylinder { frame, .. }
        | Surface::Cone { frame, .. }
        | Surface::Torus { frame, .. } => frame,
        _ => return None,
    };
    let axis = geom::scale(frame.z, geom::dot(frame.z, floor_axis).signum());
    if geom::dot(axis, floor_axis) < 1. - 1e-9 {
        return None;
    }
    let tolerance = length_tolerance(&[start.radius]);
    let mut previous = floor.clone();
    let mut incoming = start.clone();
    let mut faces = BTreeSet::new();
    let mut main_wall: Option<(f64, f64)> = None;
    let mut last_torus = false;
    loop {
        let face = across(part, &incoming, &previous)?;
        if let Surface::Plane { frame } = part.faces[face].surface {
            if faces.is_empty()
                || main_wall.is_none()
                || geom::dot(normal(part, face)?, axis) < 1. - 1e-9
                || geom::dot(geom::sub(incoming.center, frame.origin), axis).abs() > tolerance
            {
                return None;
            }
            for &p in &previous {
                let expected = if last_torus { Arc::Smooth } else { Arc::Convex };
                if part.arc(p, face) != Some(expected) {
                    return None;
                }
            }
            return Some(Wall {
                faces,
                mouth: incoming,
                context: BTreeSet::from([face]),
                angle: main_wall?.1,
                axis,
            });
        }
        if !faces.insert(face) {
            return None;
        }
        let (frame, angle, torus) = match part.faces[face].surface {
            Surface::Cylinder { frame, .. } => (frame, Some(90.), false),
            Surface::Cone {
                frame, semi_angle, ..
            } => (frame, Some(semi_angle.abs().to_degrees()), false),
            Surface::Torus {
                frame,
                major,
                minor,
            } if minor > tolerance && major > minor => {
                let (_, _, lo, hi) = part.uv_bounds(face)?;
                if hi - lo >= std::f64::consts::PI - 1e-9 {
                    return None;
                }
                (frame, None, true)
            }
            _ => return None,
        };
        if geom::dot(frame.z, axis).abs() < 1. - 1e-9
            || geom::norm(geom::cross(geom::sub(frame.origin, start.center), axis)) > tolerance
            || (!torus && part.faces[face].reversed != outer)
        {
            return None;
        }
        for &p in &previous {
            let Some(arc) = part.arc(p, face) else {
                continue;
            };
            if (torus || last_torus) && arc != Arc::Smooth
                || (previous == *floor && !torus && arc != Arc::Concave)
            {
                return None;
            }
        }
        let outgoing = other_rim(part, face, &incoming, axis)?;
        let delta = geom::sub(outgoing.center, incoming.center);
        let height = geom::dot(delta, axis);
        if height <= tolerance || geom::norm(geom::cross(delta, axis)) > tolerance {
            return None;
        }
        // Stay between the floor and mouth levels. A toroidal corner may have a radial
        // extremum between its ends, especially where a wall leans under the rim.
        for edge in part.face_edges(face) {
            for &p in &part.edges[edge].samples {
                let d = geom::sub(p, incoming.center);
                let z = geom::dot(d, axis);
                let r = geom::norm(geom::cross(d, axis));
                if z < -tolerance
                    || z > height + tolerance
                    || (!torus
                        && (r < incoming.radius.min(outgoing.radius) - tolerance
                            || r > incoming.radius.max(outgoing.radius) + tolerance))
                {
                    return None;
                }
            }
        }
        if let Some(angle) = angle {
            match main_wall {
                Some((h, a)) if (height - h).abs() <= tolerance && (angle - a).abs() > 1e-9 => {
                    return None;
                }
                Some((h, _)) if height <= h => {}
                _ => main_wall = Some((height, angle)),
            }
        }
        previous = BTreeSet::from([face]);
        incoming = outgoing;
        last_torus = torus;
    }
}

fn from_floor(part: &Part, floor: &BTreeSet<usize>) -> Option<Occurrence<FaceGroove>> {
    let floor_axis = normal(part, *floor.first()?)?;
    let mut rings = floor_rings(part, floor, floor_axis)?;
    rings.sort_by(|a, b| a.radius.total_cmp(&b.radius));
    let (inner, outer) = (&rings[0], &rings[1]);
    let tolerance = length_tolerance(&[outer.radius]);
    if outer.radius - inner.radius <= tolerance
        || geom::dist(inner.center, outer.center) > tolerance
    {
        return None;
    }
    let iw = wall(part, floor, inner, false)?;
    let ow = wall(part, floor, outer, true)?;
    let axis = ow.axis;
    let depth = geom::dot(geom::sub(ow.mouth.center, outer.center), axis);
    if geom::dot(iw.axis, axis) < 1. - 1e-9
        || geom::dist(iw.mouth.center, ow.mouth.center) > tolerance
        || ow.mouth.radius - iw.mouth.radius <= tolerance
        || depth <= tolerance
        || !iw.faces.is_disjoint(&ow.faces)
    {
        return None;
    }
    let defining: Vec<_> = floor
        .iter()
        .chain(&iw.faces)
        .chain(&ow.faces)
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let context: Vec<_> = iw.context.union(&ow.context).copied().collect();
    if context.iter().any(|f| defining.contains(f))
        || evidence::strict_status(part, &defining, &context) != EvidenceStatus::Verified
    {
        return None;
    }
    Some(Occurrence {
        record: FaceGroove {
            axis,
            axis_point: ow.mouth.center,
            outer_diameter: 2. * ow.mouth.radius,
            inner_diameter: 2. * iw.mouth.radius,
            depth,
            outer_wall_angle: ow.angle,
            inner_wall_angle: iw.angle,
            floor: floor.iter().copied().collect(),
            faces: defining.clone(),
        },
        defining,
        context,
    })
}

internal_entry! {
/// Complete annular floor regions, in source traversal order. Ambiguous overlapping
/// candidates are all refused; each accepted occurrence carries strict solid evidence.
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<FaceGroove>> {
    let mut seen = BTreeSet::<usize>::new();
    let mut found = Vec::new();
    for seed in 0..ctx.part.faces.len() {
        if seen.contains(&seed) || !matches!(ctx.part.faces[seed].surface, Surface::Plane { .. }) {
            continue;
        }
        let Some(floor) = floor_region(ctx.part, seed) else { continue };
        seen.extend(&floor);
        if let Some(o) = from_floor(ctx.part, &floor) { found.push(o); }
    }
    let mut owners = BTreeMap::<usize, usize>::new();
    for o in &found { for &f in &o.defining { *owners.entry(f).or_default() += 1; } }
    found.into_iter().filter(|o| o.defining.iter().all(|f| owners[f] == 1)).collect()
}
}
