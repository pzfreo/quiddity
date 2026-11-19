//! Chamfer recognition (`quiddity.chamfers`).
//!
//! A prismatic chamfer is a small oblique planar face bevelling a convex edge between two
//! axis-aligned walls; a turned chamfer is a conical band meeting a coaxial external cylinder.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::Context;
use super::bevel::{classify_bevel, proved_bevel_planes, transverse_corner};
use super::cylinders::CylinderEvidence;
use super::evidence::{self, EvidenceError, Occurrence};
use super::placed::{Grid, Placed};
use super::policy::AXIS_ALIGNED_COS;
use super::turned::{self, cone_rims};
use crate::kernel::brep::Part;
use crate::kernel::classify::material_at;
use crate::kernel::geom::{self, COORD_FLOOR, Surface, V3, dominant_axis};
use crate::kernel::py;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Chamfer {
    pub axis: char,
    pub leg1: f64,
    pub leg2: f64,
    pub angle: f64,
    pub at: Placed<V3>,
    pub turned: bool,
    /// The virtual sharp corner a planar chamfer replaces; `None` for turned chamfers
    /// or when reconstruction/empty-wedge proof fails.
    pub corner: Option<Placed<V3>>,
}

/// Options named as `recognise_chamfers`' keyword arguments.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct ChamferOptions {
    /// The minimum leg (an edge break below it is not reported); 0 by default.
    pub tol: Option<f64>,
    pub max_leg_frac: f64,
    /// `false` for a part already classified as rotational: only conical chamfers are reported.
    pub include_planar: bool,
}

impl Default for ChamferOptions {
    fn default() -> Self {
        ChamferOptions {
            tol: None,
            max_leg_frac: 0.45,
            include_planar: true,
        }
    }
}

internal_entry! {
/// The evidence path: each chamfer with its defining face, published only if every one is
/// owned by a valid solid.
pub fn discover_verified(
    ctx: &Context<'_>,
    opts: &ChamferOptions,
) -> Result<Vec<Occurrence<Chamfer>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx, opts))
}
}

internal_entry! {
/// `recognise_chamfers`.
pub fn recognise_chamfers(part: &Part, opts: &ChamferOptions) -> Vec<Chamfer> {
    discover(&Context::new(part), opts)
        .into_iter()
        .map(|o| o.record)
        .collect()
}
}

/// Whether a measured leg is materially below the evidence floor (`_below_minimum_evidence`).
fn below_minimum(value: f64, minimum: f64) -> bool {
    value < minimum && (value - minimum).abs() > COORD_FLOOR
}

fn chamfer(
    axis: usize,
    leg_hi: f64,
    leg_lo: f64,
    at: V3,
    turned: bool,
    corner: Option<V3>,
) -> Chamfer {
    Chamfer {
        axis: ['x', 'y', 'z'][axis],
        leg1: py::round_to3(leg_hi),
        leg2: py::round_to3(leg_lo),
        angle: py::round_to(leg_lo.atan2(leg_hi).to_degrees(), 2),
        at: Placed::new(at, Grid::Decimals(3)),
        turned,
        corner: corner.map(|c| Placed::new(c, Grid::Decimals(3))),
    }
}

internal_entry! {
pub fn discover(ctx: &Context<'_>, opts: &ChamferOptions) -> Vec<Occurrence<Chamfer>> {
    let part = ctx.part;
    let tol = opts.tol.unwrap_or(0.0);
    let max_leg = opts.max_leg_frac * ctx.bounds().max_extent();
    let mut out = Vec::new();
    if opts.include_planar {
        out.extend((0..part.faces.len()).filter_map(|f| {
            planar(ctx, f, tol, max_leg).or_else(|| rolled_planar(ctx, f, tol, max_leg))
        }));
    }
    prune_polygon_sides(part, &mut out);
    let external = turned::external_cylinders(ctx);
    out.extend((0..part.faces.len()).filter_map(|f| turned(part, f, tol, max_leg, &external)));
    for o in &mut out {
        if let Some(corner) = o.record.corner {
            let r = &o.record;
            o.record.corner = super::bevel::proved_published_corner(ctx, &o.defining,
                match r.axis {'x'=>0,'y'=>1,_=>2}, *r.at.raw(), *corner.raw(),
                [r.leg1,r.leg2], r.angle);
        }
    }
    out.sort_by(|a, b| {
        a.record
            .axis
            .cmp(&b.record.axis)
            .then_with(|| py::tuple_order(&a.record.at[..], &b.record.at[..]))
    });
    // The edge breaks of conical rims are appended after the sorted records, so a saved rank of any
    // earlier chamfer does not move (#298).
    let rims: Vec<Occurrence<Chamfer>> = (0..part.faces.len())
        .filter(|f| !out.iter().any(|o| o.defining.contains(f)))
        .filter_map(|f| cone_body_rim(part, f, tol, max_leg))
        .collect();
    out.extend(rims);
    out
}
}

/// The planar faces sharing a straight edge parallel to `run` with `face`.
fn lateral_planes(part: &Part, face: usize, run: V3) -> Vec<usize> {
    part.neighbours(face)
        .into_iter()
        .filter(|&n| matches!(part.faces[n].surface, Surface::Plane { .. }))
        .filter(|&n| {
            part.shared_edges(face, n).into_iter().any(|e| {
                let edge = &part.edges[e];
                let span = geom::sub(edge.end, edge.start);
                matches!(edge.curve, geom::Curve::Line { .. })
                    && geom::norm(geom::cross(span, run)) <= 1e-6 * geom::norm(span)
            })
        })
        .collect()
}

/// Whether the chain of convexly joined run-parallel planes from `via` (beyond `from`) comes
/// back round to `start`: a closed, wholly convex section, as of a polygonal prism.
fn closes_convexly(part: &Part, run: V3, start: usize, from: usize, via: usize) -> bool {
    let convex = |a: usize, b: usize| part.arc(a, b) == Some(crate::kernel::brep::Arc::Convex);
    let (mut previous, mut current) = (from, via);
    for _ in 0..part.faces.len() {
        if current == start {
            return true;
        }
        if !convex(current, previous) {
            return false;
        }
        let onward: Vec<usize> = lateral_planes(part, current, run)
            .into_iter()
            .filter(|&n| n != previous)
            .collect();
        let [next] = onward[..] else { return false };
        (previous, current) = (current, next);
    }
    false
}

/// The share of a wall's width that a bevel's leg along it must take before the wall's far end
/// is asked to meet a perpendicular wall or another bevel.
const POLYGON_SIDE_SHARE: f64 = 1. / 3.;

/// A flat of a polygonal section is not a chamfer of its neighbours. Two walls and the bevel
/// between them are read as a chamfered corner only when each wall's far end also meets a
/// perpendicular wall, another bevel or a blend: a wall that runs on into an oblique plane that
/// is no bevel belongs to a polygonal section (a hexagonal bar, say), whose flats could equally
/// be called chamfers of one another (#301). Only a bevel whose leg takes a third or more of a
/// wall's width is held to this; a short break beside some other oblique face is still a chamfer.
/// A removed bevel can orphan its neighbours, so repeat until nothing changes.
fn prune_polygon_sides(part: &Part, out: &mut Vec<Occurrence<Chamfer>>) {
    loop {
        let bevels: Vec<usize> = out
            .iter()
            .filter(|o| !o.record.turned)
            .map(|o| o.defining[0])
            .collect();
        let before = out.len();
        out.retain(|o| {
            if o.record.turned {
                return true;
            }
            let bevel = o.defining[0];
            let mut run = [0.; 3];
            run[match o.record.axis {
                'x' => 0,
                'y' => 1,
                _ => 2,
            }] = 1.;
            let walls = lateral_planes(part, bevel, run)
                .into_iter()
                .filter(|&wall| part.arc(bevel, wall) == Some(crate::kernel::brep::Arc::Convex));
            walls.into_iter().all(|wall| {
                let Some(wall_normal) = super::planes::unit_plane_normal(part, wall) else {
                    return true;
                };
                // A short break does not make its wall a side of a polygon: only a bevel whose
                // leg takes a real share of the wall's own width asks for a square far end.
                let across = geom::cross(run, wall_normal);
                let width = |face: usize| {
                    let (lo, hi) = part.extent_along(&[face], across);
                    hi - lo
                };
                if width(bevel) <= POLYGON_SIDE_SHARE * width(wall) {
                    return true;
                }
                lateral_planes(part, wall, run).into_iter().all(|far| {
                    far == bevel
                        || bevels.contains(&far)
                        // A concave far end (an internal bevel) is another feature.
                        || part.arc(wall, far) != Some(crate::kernel::brep::Arc::Convex)
                        || super::planes::unit_plane_normal(part, far)
                            .is_none_or(|n| geom::dot(n, wall_normal).abs() <= 1e-6)
                        // An oblique plane only makes a polygon side if the section closes
                        // convexly through it; a groove, dovetail or step does not.
                        || !closes_convexly(part, run, bevel, wall, far)
                })
            })
        });
        if out.len() == before {
            return;
        }
    }
}

/// An oblique planar bevel on a convex edge between two axis-aligned walls.
fn planar(ctx: &Context<'_>, face: usize, tol: f64, max_leg: f64) -> Option<Occurrence<Chamfer>> {
    let part = ctx.part;
    let bevel = classify_bevel(part, face).ok()?;
    let centre = bevel.bounds.centre();
    if below_minimum(bevel.leg_lo, tol) || bevel.leg_hi > max_leg {
        return None;
    }
    let planes = proved_bevel_planes(ctx, face, centre, bevel.edge_axis)?;
    let corner = [0, 1, 2].map(|i| {
        if i == bevel.edge_axis {
            centre[i]
        } else {
            planes[&i]
        }
    });
    let at = part.face_centre(face)?;
    Some(Occurrence {
        record: chamfer(
            bevel.edge_axis,
            bevel.leg_hi,
            bevel.leg_lo,
            at,
            false,
            Some(corner),
        ),
        defining: vec![face],
        context: vec![],
    })
}

/// A principal-run bevel with rolled supporting walls. Keep the legacy record and its
/// principal run, measuring the legs on the actual supports instead of a coordinate box.
fn rolled_planar(
    ctx: &Context<'_>,
    face: usize,
    tol: f64,
    max_leg: f64,
) -> Option<Occurrence<Chamfer>> {
    use super::planes::unit_plane_normal;
    use crate::kernel::geom::SMOOTH_ARC_GAP;
    let part = ctx.part;
    let Surface::Plane { .. } = part.faces[face].surface else {
        return None;
    };
    // Preserve the principal-run / oblique-face contract of this axis-letter family.
    let axis = classify_bevel(part, face).ok()?.edge_axis;
    if !super::planes::linear_quad(part, face) {
        return None;
    }
    let normal = unit_plane_normal(part, face)?;
    let at = part.face_centre(face)?;
    let mut run = [0.; 3];
    run[axis] = 1.;
    let corner = transverse_corner(part, face, at, run)?;
    // A broad stock wall between two small bevel strips has the same local plane
    // arrangement. It is not a third chamfer: the supporting walls must dominate the strip.
    let area = part.face_mass(face)?[0];
    if corner
        .areas
        .iter()
        .any(|&support| !super::policy::clears_threshold(support, area))
    {
        return None;
    }
    let supports = corner.normals;
    let corner = corner.point;
    // A bevel faces out between its supports and replaces their outer virtual edge.
    if supports
        .iter()
        .any(|&n| geom::dot(normal, n) <= SMOOTH_ARC_GAP)
        || geom::dot(geom::sub(at, corner), normal) >= -COORD_FLOOR
    {
        return None;
    }
    // This path repairs rolled supports only; an axis-aligned ambiguity must still
    // be resolved by the existing corner proof, never bypassed here.
    if !supports
        .iter()
        .any(|n| n.iter().all(|c| c.abs() < AXIS_ALIGNED_COS))
    {
        return None;
    }
    let distance = geom::dot(geom::sub(at, corner), normal).abs();
    // The supports can differ from a right angle by the existing angular tolerance.
    // Measure on each actual support, rather than treating its partner's normal as a tangent.
    let legs = supports.map(|n| {
        let tangent = geom::cross(run, n);
        distance * geom::norm(tangent) / geom::dot(tangent, normal).abs()
    });
    let (leg_lo, leg_hi) = (legs[0].min(legs[1]), legs[0].max(legs[1]));
    if below_minimum(leg_lo, tol) || leg_hi > max_leg || leg_lo <= COORD_FLOOR {
        return None;
    }
    let near = geom::add(
        corner,
        geom::scale(geom::sub(at, corner), super::policy::INTERIOR_PROBE_FRAC),
    );
    if material_at(ctx.classifier(), near) {
        return None;
    }
    Some(Occurrence {
        record: chamfer(axis, leg_hi, leg_lo, at, false, Some(corner)),
        defining: vec![face],
        context: vec![],
    })
}

/// A conical band on turned stock, proved by a coaxial external cylinder and either a
/// transverse shoulder plane or a second band.
fn turned(
    part: &Part,
    face: usize,
    tol: f64,
    max_leg: f64,
    external: &BTreeMap<usize, &CylinderEvidence>,
) -> Option<Occurrence<Chamfer>> {
    let Surface::Cone { frame, .. } = part.faces[face].surface else {
        return None;
    };
    let ((minor_r, minor_c), (major_r, major_c), _) = cone_rims(part, face)?;
    let d = frame.z;
    let edge_axis = dominant_axis(d.map(f64::abs));
    if d[edge_axis].abs() < AXIS_ALIGNED_COS {
        return None;
    }
    let radial = major_r - minor_r;
    let axial = geom::dot(geom::sub(major_c, minor_c), d).abs();
    let (leg_hi, leg_lo) = (axial.max(radial), axial.min(radial));
    if below_minimum(leg_lo, tol) || leg_hi > max_leg {
        return None;
    }
    let neighbours = part.neighbours(face);
    let coaxial = turned::coaxial_cylinders(&neighbours, external, frame.origin, d);
    if coaxial.is_empty() {
        return None;
    }
    let transverse = !turned::transverse_planes(part, &neighbours, d).is_empty();
    if !transverse && !turned::bridges_two_bands(&coaxial) {
        return None; // a taper meeting one band, not a bevelled edge
    }
    let at = part.face_centre(face)?;
    Some(Occurrence {
        record: chamfer(edge_axis, leg_hi, leg_lo, at, true, None),
        defining: vec![face],
        context: vec![],
    })
}

const CONE_RIM_PLANE_SHARE: f64 = 0.47;

/// An edge break on the rim of a conical body: a narrow coaxial cone between a transverse end
/// plane and the body's cone, both joined convexly (a chamfer of a cone's base or top). The
/// legs are measured along the end plane and along the body's cone, from the virtual sharp
/// rim where the two meet, as for a chamfer between any two faces (#298). The band must be
/// shorter than the body cone it breaks into, so the body itself is never read as a rim.
fn cone_body_rim(part: &Part, face: usize, tol: f64, max_leg: f64) -> Option<Occurrence<Chamfer>> {
    let Surface::Cone {
        frame: band,
        semi_angle: band_angle,
        ..
    } = part.faces[face].surface
    else {
        return None;
    };
    let d = band.z;
    let edge_axis = dominant_axis(d.map(f64::abs));
    if d[edge_axis].abs() < AXIS_ALIGNED_COS {
        return None;
    }
    let axial = |p: V3| geom::dot(geom::sub(p, band.origin), d);
    // Radius and axial position of the circle two faces share.
    let rim_with = |other: usize| {
        part.shared_edges(face, other)
            .into_iter()
            .find_map(|e| match part.edges[e].curve {
                geom::Curve::Circle { frame, radius } => Some((radius, axial(frame.origin))),
                _ => None,
            })
    };
    let slope = |frame: crate::kernel::geom::Frame, angle: f64| angle.tan() * geom::dot(frame.z, d);
    let mut plane = None;
    let mut body = None;
    for n in part.neighbours(face) {
        if part.arc(face, n) != Some(crate::kernel::brep::Arc::Convex) {
            continue;
        }
        let surface = &part.faces[n].surface;
        if let Surface::Plane { frame } = surface
            && geom::dot(frame.z, d).abs() > AXIS_ALIGNED_COS
        {
            if plane.replace((n, *frame)).is_some() {
                return None;
            }
        } else if let Surface::Cone {
            frame,
            radius,
            semi_angle,
        } = surface
            && geom::dot(frame.z, d).abs() > 1. - 1e-9
            && geom::norm(geom::cross(geom::sub(frame.origin, band.origin), d)) <= COORD_FLOOR
            && (slope(*frame, *semi_angle) - slope(band, band_angle)).abs() > 1e-6
            && body.replace((n, *frame, *radius, *semi_angle)).is_some()
        {
            return None;
        }
    }
    let ((plane_face, plane_frame), (body_face, body_frame, body_radius, body_angle)) =
        (plane?, body?);
    let (r_plane, z_plane) = rim_with(plane_face)?;
    let (r_body, z_body) = rim_with(body_face)?;
    // The body's own radius where the end plane cuts it: the virtual sharp rim.
    let r_corner = body_radius
        + geom::dot(
            geom::sub(plane_frame.origin, body_frame.origin),
            body_frame.z,
        ) * body_angle.tan();
    if r_plane <= 0. || r_body <= 0. || r_corner <= r_plane {
        return None;
    }
    let leg_plane = r_corner - r_plane;
    let leg_body = (r_corner - r_body).hypot(z_plane - z_body);
    let slant = match turned::cone_rims(part, body_face) {
        Some(((r0, c0), (r1, c1), _)) => (r1 - r0).hypot(geom::dot(geom::sub(c1, c0), d)),
        None => r_body / body_angle.sin().abs(),
    };
    let (leg_hi, leg_lo) = (leg_plane.max(leg_body), leg_plane.min(leg_body));
    // The break must be small against what it breaks. Along the end plane it may reach at most
    // 0.47 of the virtual rim's radius (the chamfer-rims cases reach 0.45; the symmetric
    // two-cone profile of a diamond reaches 0.49), and along the body it must be strictly
    // shorter than the band left, so a tie between two equal bands reads neither as the rim.
    if below_minimum(leg_lo, tol)
        || leg_hi > max_leg
        || leg_plane > CONE_RIM_PLANE_SHARE * r_corner
        || leg_body >= slant * (1. - 1e-9)
    {
        return None;
    }
    let at = part.face_centre(face)?;
    Some(Occurrence {
        record: chamfer(edge_axis, leg_hi, leg_lo, at, true, None),
        defining: vec![face],
        context: vec![],
    })
}
