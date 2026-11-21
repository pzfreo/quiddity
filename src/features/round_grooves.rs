//! Round-bottomed (toroidal) annular grooves (#307): an O-ring gland or a ball groove cut with
//! one torus. The torus faces are the whole defining claim. The bore, shaft or planar faces the
//! groove cuts into are consulted evidence only, so the record does not depend on how a host
//! hole or turned body is split into records and never claims their land faces.
use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::{PI, TAU};

use serde::Serialize;

use super::annular_grooves::{Ring, floor_rings};
use super::evidence::{self, EvidenceStatus, Occurrence};
use super::graph::normal;
use super::regions::length_tolerance;
use super::{Context, Features, Kept, kept};
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::{self, Surface, V3};

/// What the groove is cut into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RoundGrooveHost {
    /// The wall of a cylindrical bore: the groove widens the bore outwards.
    Bore,
    /// The outside of a cylindrical shaft: the groove cuts inwards.
    Shaft,
    /// A planar face: a ring-shaped furrow below that plane (a face O-ring gland).
    Face,
}

/// A groove whose bottom is a torus. `axis` is the torus axis (for a `face` groove it points
/// from the groove bottom out of the face) and `center` the torus centre on it, in file
/// coordinates. `ring_diameter` is the diameter of the circle through the section centres and
/// `bottom_radius` the section radius. `width` is measured across the mouth where the torus
/// meets the host (along the axis for a bore or shaft, radially for a face), `depth` from the
/// host surface to the deepest point. `host_diameter` is the bore or shaft diameter beside the
/// groove. `faces` are the torus faces; `host_faces` the consulted bore, shaft or planar faces.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct RoundGroove {
    pub host: RoundGrooveHost,
    pub axis: V3,
    pub center: V3,
    pub ring_diameter: f64,
    pub bottom_radius: f64,
    pub width: f64,
    pub depth: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_diameter: Option<f64>,
    pub faces: Vec<usize>,
    pub host_faces: Vec<usize>,
}

/// Publish after the existing families. Existing records are never changed. A reserved
/// refusal on any defining face suppresses the groove, except an unproved interior void
/// that is exactly the groove's own faces (the widening the groove explains).
pub(crate) fn append(ctx: &Context<'_>, features: &mut Features) {
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
        .filter(|o| {
            o.defining.iter().all(|f| !claimed.contains(f))
                && features
                    .interior_void_refusals
                    .iter()
                    .filter(|r| {
                        r.candidate
                            .void_faces
                            .iter()
                            .any(|f| o.defining.contains(f))
                    })
                    .all(|r| {
                        // The refusal must be exactly this groove: the same face set.
                        let mut void = r.candidate.void_faces.clone();
                        void.sort_unstable();
                        void.dedup();
                        void == o.defining
                    })
        })
        .collect();
    if found.is_empty() {
        return;
    }
    let mut registration = Kept {
        part: ctx.part,
        unmeasured: super::unmeasured_faces(ctx.part).into_iter().collect(),
        claimed: Default::default(),
        defining: Default::default(),
        constituent: Default::default(),
        evidence_status: Default::default(),
    };
    features.round_grooves = kept(&mut registration, "round_grooves", found);
    features.defining.extend(registration.defining);
    features
        .evidence_status
        .extend(registration.evidence_status);
}

/// Torus faces sharing one torus surface, connected through shared edges.
fn cluster(part: &Part, seed: usize) -> BTreeSet<usize> {
    let Surface::Torus {
        frame,
        major,
        minor,
    } = part.faces[seed].surface
    else {
        unreachable!("seeded from a torus face")
    };
    let tolerance = length_tolerance(&[major]);
    let mut found = BTreeSet::from([seed]);
    let mut pending = vec![seed];
    while let Some(face) = pending.pop() {
        for next in part.neighbours(face) {
            let Surface::Torus {
                frame: other,
                major: m,
                minor: n,
            } = part.faces[next].surface
            else {
                continue;
            };
            if !found.contains(&next)
                && geom::dot(other.z, frame.z).abs() > 1. - 1e-9
                && geom::dist(other.origin, frame.origin) <= tolerance
                && (m - major).abs() <= tolerance
                && (n - minor).abs() <= tolerance
            {
                found.insert(next);
                pending.push(next);
            }
        }
    }
    found
}

/// The one face on the far side of a complete rim, joined to the torus by a convex crease.
fn land(part: &Part, rim: &Ring, torus: &BTreeSet<usize>) -> Option<usize> {
    let mut owner = None;
    for &edge in &rim.edges {
        let mut outside = Vec::new();
        for &f in &part.edge_faces()[edge] {
            if !torus.contains(&f) {
                outside.push(f);
            }
        }
        let [face] = outside[..] else { return None };
        if owner.is_some_and(|old| old != face) {
            return None;
        }
        owner = Some(face);
        for &f in &part.edge_faces()[edge] {
            if torus.contains(&f) && part.arc(f, face) != Some(Arc::Convex) {
                return None;
            }
        }
    }
    owner
}

/// Whether some torus face's minor-angle range contains `target` (minor angle 0 points away
/// from the axis, pi towards it).
fn reaches(part: &Part, torus: &BTreeSet<usize>, target: f64) -> bool {
    torus.iter().any(|&f| {
        part.uv_bounds(f).is_some_and(|(_, _, lo, hi)| {
            hi - lo < TAU - 1e-9 && (target - lo).rem_euclid(TAU) <= hi - lo + 1e-9
        })
    })
}

fn from_torus(part: &Part, torus: &BTreeSet<usize>) -> Option<Occurrence<RoundGroove>> {
    let Surface::Torus {
        frame,
        major,
        minor,
    } = part.faces[*torus.first()?].surface
    else {
        return None;
    };
    let tolerance = length_tolerance(&[major + minor]);
    if minor <= tolerance || major <= minor + tolerance {
        return None;
    }
    let axis = geom::unit(frame.z)?;
    let mut rings = floor_rings(part, torus, axis)?;
    if rings.len() != 2 || rings.iter().any(|r| r.radius <= tolerance) {
        return None;
    }
    rings.sort_by(|a, b| geom::dot(geom::sub(a.center, b.center), axis).total_cmp(&0.));
    let lands = [land(part, &rings[0], torus)?, land(part, &rings[1], torus)?];
    let along = |p: V3| geom::dot(geom::sub(p, frame.origin), axis);
    let offset_from_axis = |p: V3| geom::norm(geom::cross(geom::sub(p, frame.origin), axis));
    if rings.iter().any(|r| offset_from_axis(r.center) > tolerance) {
        return None;
    }
    let host_faces: Vec<usize> = BTreeSet::from(lands).into_iter().collect();
    let (host, width, depth, host_diameter, record_axis);
    match (&part.faces[lands[0]].surface, &part.faces[lands[1]].surface) {
        (
            Surface::Cylinder {
                frame: a,
                radius: ra,
            },
            Surface::Cylinder {
                frame: b,
                radius: rb,
            },
        ) => {
            let (fa, fb) = (&part.faces[lands[0]], &part.faces[lands[1]]);
            if lands[0] == lands[1]
                || fa.reversed != fb.reversed
                || geom::dot(a.z, axis).abs() < 1. - 1e-9
                || geom::dot(b.z, axis).abs() < 1. - 1e-9
                || offset_from_axis(a.origin) > tolerance
                || offset_from_axis(b.origin) > tolerance
                || (ra - rb).abs() > tolerance
                || (ra - rings[0].radius).abs() > tolerance
                || (rb - rings[1].radius).abs() > tolerance
            {
                return None;
            }
            let span = along(rings[1].center) - along(rings[0].center);
            if span <= tolerance {
                return None;
            }
            let (internal, extent, target) = if fa.reversed {
                (true, major + minor - ra, 0.)
            } else {
                (false, ra - (major - minor), PI)
            };
            if extent <= tolerance || !reaches(part, torus, target) {
                return None;
            }
            host = if internal {
                RoundGrooveHost::Bore
            } else {
                RoundGrooveHost::Shaft
            };
            (width, depth, host_diameter, record_axis) = (span, extent, Some(2. * ra), axis);
        }
        (Surface::Plane { .. }, Surface::Plane { .. }) => {
            let (na, nb) = (normal(part, lands[0])?, normal(part, lands[1])?);
            let up = na;
            if geom::dot(na, nb) < 1. - 1e-9
                || geom::dot(na, axis).abs() < 1. - 1e-9
                || (along(rings[0].center) - along(rings[1].center)).abs() > tolerance
            {
                return None;
            }
            let mouth = along(rings[0].center) * geom::dot(up, axis);
            let depth_here = mouth + minor;
            let width_here = (rings[1].radius - rings[0].radius).abs();
            let target = if geom::dot(up, axis) > 0. {
                3. * PI / 2.
            } else {
                PI / 2.
            };
            if depth_here <= tolerance || width_here <= tolerance || !reaches(part, torus, target) {
                return None;
            }
            host = RoundGrooveHost::Face;
            (width, depth, host_diameter, record_axis) = (width_here, depth_here, None, up);
        }
        _ => return None,
    }
    let defining: Vec<usize> = torus.iter().copied().collect();
    if host_faces.iter().any(|f| torus.contains(f))
        || evidence::strict_status(part, &defining, &host_faces) != EvidenceStatus::Verified
    {
        return None;
    }
    Some(Occurrence {
        record: RoundGroove {
            host,
            axis: record_axis,
            center: frame.origin,
            ring_diameter: 2. * major,
            bottom_radius: minor,
            width,
            depth,
            host_diameter,
            faces: defining.clone(),
            host_faces: host_faces.clone(),
        },
        defining,
        context: host_faces,
    })
}

internal_entry! {
/// Each complete torus ring cut into a bore, a shaft or a planar face, in source order. A
/// torus face claimed by two candidates is refused.
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<RoundGroove>> {
    let mut seen = BTreeSet::<usize>::new();
    let mut found = Vec::new();
    for seed in 0..ctx.part.faces.len() {
        if seen.contains(&seed) || !matches!(ctx.part.faces[seed].surface, Surface::Torus { .. }) {
            continue;
        }
        let torus = cluster(ctx.part, seed);
        seen.extend(&torus);
        if let Some(o) = from_torus(ctx.part, &torus) {
            found.push(o);
        }
    }
    let mut owners = BTreeMap::<usize, usize>::new();
    for o in &found {
        for &f in &o.defining {
            *owners.entry(f).or_default() += 1;
        }
    }
    found
        .into_iter()
        .filter(|o| o.defining.iter().all(|f| owners[f] == 1))
        .collect()
}
}
