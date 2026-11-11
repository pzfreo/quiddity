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
    out
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
