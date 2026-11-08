//! Cylindrical boss recognition (`quiddity.bosses`): each external full-cylinder segment, its
//! free end found from the faces beyond its two axial ends.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::Context;
use super::cylinders::{
    Segment, axis_point_at, coaxial_axis_lines, full_cylinders, segments, z_then_cross,
};
use super::evidence::{self, EvidenceError, Occurrence};
use super::stacks::{End, classify_end};
use crate::kernel::brep::Part;
use crate::kernel::geom::{self, COORD_FLOOR, Surface, V3};
use crate::kernel::py;

/// A boss: `axis` points from its base to its free end, `location` is the axis point at the free
/// end, `height` the axial length including convex end chamfers and rounds, and `location`
/// includes the free end's treatment too. When geometry leaves the two ends interchangeable,
/// axis/location represent an end gauge, not an asserted material base/free direction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BossRecord {
    /// Optional additive reference to the identical/superseding published feature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    pub axis: V3,
    pub location: V3,
    pub diameter: f64,
    pub height: f64,
}

internal_entry! {
/// `recognise_bosses`.
pub fn recognise_bosses(part: &Part) -> Vec<BossRecord> {
    super::records(discover(&Context::new(part)))
}
}

internal_entry! {
/// The evidence path: each boss with its cylinder faces and the face at its free end.
pub fn discover_verified(ctx: &Context<'_>) -> Result<Vec<Occurrence<BossRecord>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx))
}
}

/// An intrinsic body-relative origin, using the boundary-area distribution already cached by
/// the kernel. Missing surface measurements cannot establish a direction.
fn body_origin(ctx: &Context<'_>, solid: usize) -> Option<V3> {
    let part = ctx.part;
    let faces = part
        .solids
        .get(solid)
        .map_or_else(|| (0..part.faces.len()).collect(), |s| s.faces.clone());
    let weighted = faces
        .into_iter()
        .map(|f| {
            let moments = ctx.surface_moments(f)?;
            Some((moments.area, moments.centroid))
        })
        .collect::<Option<Vec<_>>>()?;
    let area = py::fsum(weighted.iter().map(|(a, _)| *a));
    if !area.is_finite() || area <= 0. {
        return None;
    }
    let origin = [0, 1, 2].map(|i| py::fsum(weighted.iter().map(|(a, c)| a * c[i])) / area);
    origin.iter().all(|x| x.is_finite()).then_some(origin)
}

fn free_end(lo: End, hi: End, relative_body_origin: Option<f64>) -> (bool, bool) {
    match (lo == End::Open, hi == End::Open) {
        (true, false) => (false, false),
        (false, true) => (true, false),
        _ => {
            // When end states tie, the free end points away from the body's boundary-area
            // centre. With no established direction, retain the OD using its existing
            // undirected cylinder-axis representative. Both ends are presentation-equivalent.
            match relative_body_origin.filter(|offset| offset.abs() > COORD_FLOOR) {
                Some(offset) => (offset < 0., false),
                None => (true, true),
            }
        }
    }
}

/// Internal measurements are cached with recognition, including the unrounded midpoint.
pub(super) struct Analysis {
    found: Vec<Occurrence<BossRecord>>,
    midpoints: Vec<V3>,
}

struct Treatment {
    face: usize,
    lo: f64,
    hi: f64,
    context: Vec<usize>,
}

/// An end break is subordinate to its own boss: neither leg may exceed the radius or
/// half the straight cylindrical length. This rejects a long taper even on a large part.
/// At a smooth round, convexity is the tube/sphere material side, not tangency alone.
fn end_treatments(ctx: &Context<'_>, seg: &Segment) -> Vec<Treatment> {
    use crate::kernel::brep::Arc;
    let part = ctx.part;
    let limit = (seg.diameter / 2.).min((seg.s_hi - seg.s_lo) / 2.);
    let neighbours: std::collections::BTreeSet<_> =
        seg.faces.iter().flat_map(|&f| part.neighbours(f)).collect();
    let mut out = Vec::new();
    for face in neighbours {
        let surface = &part.faces[face].surface;
        let smooth_convex = match *surface {
            Surface::Cone { frame, .. } | Surface::Torus { frame, .. } => {
                if !coaxial_axis_lines(
                    frame.origin,
                    frame.z,
                    seg.axis_point,
                    seg.direction,
                    super::policy::length_tol(seg.diameter, 1e-4),
                ) {
                    continue;
                }
                if let Surface::Torus { minor, .. } = *surface {
                    let Some((_, _, v0, v1)) = part.uv_bounds(face) else {
                        continue;
                    };
                    let mut radii = vec![minor * v0.cos(), minor * v1.cos()];
                    for k in (v0 / std::f64::consts::PI).ceil() as i32
                        ..=(v1 / std::f64::consts::PI).floor() as i32
                    {
                        radii.push(minor * (f64::from(k) * std::f64::consts::PI).cos());
                    }
                    let leg = radii.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                        - radii.iter().copied().fold(f64::INFINITY, f64::min);
                    if leg > limit + COORD_FLOOR
                        || super::fillets::torus_side(part, face) != Some("convex")
                    {
                        continue;
                    }
                    true
                } else {
                    false
                }
            }
            Surface::Sphere { frame, radius } => {
                let s = geom::dot(frame.origin, seg.direction);
                if (2. * radius - seg.diameter).abs() > COORD_FLOOR
                    || geom::dist(frame.origin, axis_point_at(seg, s)) > COORD_FLOOR
                    || part.frame_points_outward(face) != Some(true)
                {
                    continue;
                }
                true
            }
            _ => continue,
        };
        let junctions: Vec<_> = seg
            .faces
            .iter()
            .filter(|&&f| part.neighbours(f).contains(&face))
            .map(|&f| part.arc(f, face))
            .collect();
        if junctions.is_empty()
            || !junctions
                .iter()
                .all(|a| *a == Some(Arc::Convex) || (smooth_convex && *a == Some(Arc::Smooth)))
        {
            continue;
        }
        let (lo, hi) = part.extent_along(&[face], seg.direction);
        let extension = if lo >= seg.s_hi - COORD_FLOOR {
            hi - seg.s_hi
        } else if hi <= seg.s_lo + COORD_FLOOR {
            seg.s_lo - lo
        } else {
            continue;
        };
        if extension <= COORD_FLOOR || extension > limit + COORD_FLOOR {
            continue;
        }
        let mut context = vec![face];
        if !matches!(surface, Surface::Sphere { .. }) {
            let Some(treatment) = ctx
                .edge_treatments()
                .iter()
                .find(|o| o.defining.contains(&face))
            else {
                continue;
            };
            if matches!(surface, Surface::Cone { .. }) {
                let Some(((r0, _), (r1, _), _)) = super::turned::cone_rims(part, face) else {
                    continue;
                };
                if r1 - r0 > limit + COORD_FLOOR {
                    continue;
                }
            }
            context.extend(&treatment.defining);
            context.extend(&treatment.context);
        }
        out.push(Treatment {
            face,
            lo,
            hi,
            context,
        });
    }
    out
}

/// Fingerprints reuse the unrounded midpoint from the same recognition analysis.
pub(crate) fn midpoint(ctx: &Context<'_>, boss: &BossRecord, faces: &[usize]) -> V3 {
    let analysis = ctx.boss_analysis();
    // Fingerprints canonicalise face order; segment discovery preserves patch order.
    if let Some(i) = analysis.found.iter().position(|o| {
        o.defining.len() == faces.len() && o.defining.iter().all(|f| faces.contains(f))
    }) {
        return analysis.midpoints[i];
    }
    // Caller-supplied feature records can lack a corresponding recognised segment.
    let (lo, hi) = ctx.part.extent_along(faces, boss.axis);
    geom::add(
        boss.location,
        geom::scale(
            boss.axis,
            (lo + hi) / 2. - geom::dot(boss.location, boss.axis),
        ),
    )
}

internal_entry! {
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<BossRecord>> {
    ctx.boss_analysis().found.clone()
}
}

pub(super) fn analyse(ctx: &Context<'_>) -> Analysis {
    let part = ctx.part;
    let (z, cross) = z_then_cross(ctx.cylinders());
    let external: Vec<_> = full_cylinders(&z)
        .into_iter()
        .chain(full_cylinders(&cross))
        .filter(|c| c.external)
        .collect();
    let mut out = Vec::new();
    let mut origins = BTreeMap::new();
    let segments = segments(&external);
    let candidates: Vec<_> = segments
        .iter()
        .map(|seg| end_treatments(ctx, seg))
        .collect();
    let mut owners = BTreeMap::<usize, usize>::new();
    for end in &candidates {
        for treatment in end {
            *owners.entry(treatment.face).or_default() += 1;
        }
    }
    let mut midpoints = Vec::new();
    for (seg, treatments) in segments.iter().zip(candidates) {
        let (lo_state, lo_faces) = classify_end(part, seg, seg.s_lo, false);
        let (hi_state, hi_faces) = classify_end(part, seg, seg.s_hi, true);
        let relative_origin = if (lo_state == End::Open) == (hi_state == End::Open) {
            origins
                .entry(seg.solid)
                .or_insert_with(|| body_origin(ctx, seg.solid))
                .map(|origin| {
                    let middle = axis_point_at(seg, (seg.s_lo + seg.s_hi) / 2.);
                    geom::dot(geom::sub(origin, middle), seg.direction)
                })
        } else {
            None
        };
        let (from_hi, presentation) = free_end(lo_state, hi_state, relative_origin);
        let gauge_faces = if presentation {
            let mut faces: Vec<_> = lo_faces.iter().chain(&hi_faces).copied().collect();
            faces.sort_unstable();
            faces.dedup();
            faces
        } else {
            vec![]
        };
        let (free_state, free_faces) = if from_hi {
            (hi_state, hi_faces)
        } else {
            (lo_state, lo_faces)
        };
        let d = seg.direction;
        let (mut lo, mut hi) = (seg.s_lo, seg.s_hi);
        let mut treatment_faces = Vec::new();
        for treatment in treatments {
            // Ambiguity cannot make one physical span lengthen two bosses.
            if owners[&treatment.face] != 1 {
                continue;
            }
            lo = lo.min(treatment.lo);
            hi = hi.max(treatment.hi);
            treatment_faces.extend(treatment.context);
        }
        midpoints.push(axis_point_at(seg, (lo + hi) / 2.));
        treatment_faces.extend(if presentation && free_state == End::Open {
            gauge_faces
        } else if free_state == End::Open {
            free_faces
        } else {
            vec![]
        });
        treatment_faces.sort_unstable();
        treatment_faces.dedup();
        out.push(Occurrence {
            record: BossRecord {
                superseded_by: None,
                axis: if from_hi { d } else { geom::scale(d, -1.0) }.map(py::without_negative_zero),
                location: axis_point_at(seg, if from_hi { hi } else { lo }),
                diameter: seg.diameter,
                height: py::round_to(hi - lo, 2),
            },
            defining: seg.faces.clone(),
            context: treatment_faces,
        });
    }
    Analysis {
        found: out,
        midpoints,
    }
}
