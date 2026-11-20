//! Flat recognition (`quiddity.flats`): a planar truncation of external round stock — a D-flat,
//! or one of an opposed pair across flats.

use serde::Serialize;

use super::Context;
use super::cylinders::{
    CylinderEvidence, axis_line_coordinates, canonical_axis_direction, z_then_cross,
};
use super::evidence::{self, EvidenceError, Occurrence};
use super::placed::{Grid, Placed};
use super::policy;
use crate::kernel::brep::Part;
use crate::kernel::geom::{self, COORD_FLOOR, Surface, V3};
use crate::kernel::py;

/// A flat's normal must be radial to within this much (its dot with the axis).
const RADIAL_TOL: f64 = 0.05;
const ANTIPARALLEL_TOL: f64 = 0.05;
const CHORD_MIN: f64 = 0.05;
const CHORD_MARGIN: f64 = 0.05;
/// A shallower cut is a tangent sliver, not a machined flat.
const MIN_FLAT_DEPTH: f64 = 0.5;
const OD_REACH_FRAC: f64 = 0.025;
const AXIS_LINE_FRAC: f64 = 0.025;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Flat {
    pub axis: char,
    /// The distance across the flats (or from one flat to the far side of the stock).
    pub across: f64,
    pub at: Placed<V3>,
    pub axis_line: Placed<(f64, f64)>,
    pub stock_span: Placed<(f64, f64)>,
    pub axis_direction: V3,
}

internal_entry! {
/// `recognise_flats`.
pub fn recognise_flats(part: &Part) -> Vec<Flat> {
    super::records(discover(&Context::new(part)))
}
}

internal_entry! {
/// The evidence path: each flat with its planar face.
pub fn discover_verified(ctx: &Context<'_>) -> Result<Vec<Occurrence<Flat>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx))
}
}

/// The flat truncates the stock: its chord has both ends reaching the stock's outside diameter
/// and straddles the point where the plane is nearest the axis. Not a recess wall ending on a
/// slot floor (an end inside the stock), and not a rib, fin or tab side (a chord wholly to one
/// side of the nearest point, where the edge to the stock is concave, or a vertex beyond the
/// stock). The flat may run on past the cylinder's own axial span (into a chamfer, taper,
/// land or hex collar), so only vertices strictly inside that span must lie within the stock.
fn chord_truncates_stock(vertices: &[V3], ax: V3, d: V3, n: V3, r: f64, span: (f64, f64)) -> bool {
    let c = geom::cross(n, d);
    let cm = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).powf(0.5);
    if cm < 1e-9 {
        return false;
    }
    let c = c.map(|x| x / cm);
    let reach = policy::length_tol(r, OD_REACH_FRAC);
    let (mut lo, mut hi) = (None::<(f64, f64)>, None::<(f64, f64)>);
    let mut within_stock = true;
    for &v in vertices {
        let rel = geom::sub(v, ax);
        let t = rel[0] * c[0] + rel[1] * c[1] + rel[2] * c[2];
        let along = rel[0] * d[0] + rel[1] * d[1] + rel[2] * d[2];
        let p = [0, 1, 2].map(|i| rel[i] - along * d[i]);
        let rad = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).powf(0.5);
        let s_axis = v[0] * d[0] + v[1] * d[1] + v[2] * d[2];
        if span.0 + COORD_FLOOR < s_axis && s_axis < span.1 - COORD_FLOOR && rad > r + reach {
            within_stock = false;
        }
        if lo.is_none_or(|(lt, _)| t < lt) {
            lo = Some((t, rad));
        }
        if hi.is_none_or(|(ht, _)| t > ht) {
            hi = Some((t, rad));
        }
    }
    matches!(
        (lo, hi),
        (Some((lt, a)), Some((ht, b)))
            if within_stock && lt < 0.0 && ht > 0.0 && a >= r - reach && b >= r - reach
    )
}

/// The face's distinct vertex points, in edge order.
fn vertices(part: &Part, face: usize) -> Vec<V3> {
    let mut ids = Vec::new();
    let mut out = Vec::new();
    for e in part.face_edges(face) {
        let edge = &part.edges[e];
        for (id, p) in [(edge.vertices.0, edge.start), (edge.vertices.1, edge.end)] {
            if !ids.contains(&id) {
                ids.push(id);
                out.push(p);
            }
        }
    }
    out
}

/// The cylinder reaches past the plane: at the direction of the plane's outward normal, some
/// face of the same cylinder surface exists over part of the flat's axial range. On a real
/// D-flat that part of the cylinder is cut away; a boss poking through a block side keeps it.
/// The cylinder may be split into several faces, with a seam or boundary exactly at the normal's
/// angle, so every face on the surface is probed, at the angle and just either side of it.
fn cylinder_reaches_past(part: &Part, cyl: &CylinderEvidence, normal: V3, vertices: &[V3]) -> bool {
    const PROBE_ANGLE: f64 = 0.005;
    let Surface::Cylinder { frame: own, radius } = part.faces[cyl.face].surface else {
        return false;
    };
    let tol = policy::length_tol(radius, 1e-6);
    for (face, f) in part.faces.iter().enumerate() {
        let Surface::Cylinder { frame, radius: r } = f.surface else {
            continue;
        };
        let off = geom::sub(frame.origin, own.origin);
        let along = geom::dot(off, own.z);
        let perp = [0, 1, 2].map(|i| off[i] - along * own.z[i]);
        let same = (r - radius).abs() <= tol
            && geom::dot(frame.z, own.z).abs() > 1.0 - 1e-9
            && geom::dot(perp, perp).sqrt() <= tol;
        if !same {
            continue;
        }
        let (Some((_, _, v0, v1)), Some(domain)) = (part.uv_bounds(face), part.domain(face)) else {
            continue;
        };
        let l = frame.dir_to_local(normal);
        let u = l[1].atan2(l[0]);
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for &p in vertices {
            let z = frame.to_local(p)[2];
            lo = lo.min(z);
            hi = hi.max(z);
        }
        let (lo, hi) = (lo.max(v0.min(v1)), hi.min(v0.max(v1)));
        if lo < hi
            && [-PROBE_ANGLE, 0.0, PROBE_ANGLE].iter().any(|du| {
                let u = (u + du).rem_euclid(std::f64::consts::TAU);
                [0.2, 0.4, 0.6, 0.8]
                    .iter()
                    .any(|f| domain.contains(u, lo + f * (hi - lo)))
            })
        {
            return true;
        }
    }
    false
}

struct Candidate<'c> {
    cyl: &'c CylinderEvidence,
    normal: V3,
    s: f64,
    at: V3,
    face: usize,
}

internal_entry! {
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<Flat>> {
    let part = ctx.part;
    let (z, cross) = z_then_cross(ctx.cylinders());
    let external: Vec<&CylinderEvidence> = z.iter().chain(&cross).filter(|c| c.external).collect();
    if external.is_empty() {
        return Vec::new();
    }
    let mut candidates: Vec<Candidate<'_>> = Vec::new();
    for face in 0..part.faces.len() {
        let Surface::Plane { .. } = part.faces[face].surface else {
            continue;
        };
        let (Some(at), Some(normal)) = (part.face_centre(face), part.face_normal(face, 0.0, 0.0))
        else {
            continue;
        };
        let adjacent = part.neighbours(face);
        for &c in &external {
            if !adjacent.contains(&c.face) {
                continue;
            }
            let d = c.direction;
            if (normal[0] * d[0] + normal[1] * d[1] + normal[2] * d[2]).abs() > RADIAL_TOL {
                continue; // a transverse end or shoulder face
            }
            let ax = c.axis_point;
            let s = (at[0] - ax[0]) * normal[0]
                + (at[1] - ax[1]) * normal[1]
                + (at[2] - ax[2]) * normal[2];
            let r = c.diameter / 2.0;
            // A flat exactly as deep as the minimum qualifies wherever the part sits: within
            // COORD_FLOOR of it, the comparison would be decided by coordinate round-off.
            let too_shallow = r - s < MIN_FLAT_DEPTH && MIN_FLAT_DEPTH - (r - s) > COORD_FLOOR;
            if !(CHORD_MIN < s && s < r - CHORD_MARGIN) || too_shallow {
                continue; // facing the axis (a slot wall), outside the stock, or a sliver
            }
            if !chord_truncates_stock(&vertices(part, face), ax, d, normal, r, (c.s_lo, c.s_hi)) {
                continue; // a recess wall, or a rib, fin or tab side
            }
            if cylinder_reaches_past(part, c, normal, &vertices(part, face)) {
                continue; // a boss pokes through a block side: its cylinder survives the plane
            }
            candidates.push(Candidate {
                cyl: c,
                normal,
                s,
                at,
                face,
            });
            break;
        }
    }
    let stock = |c: &CylinderEvidence| (c.solid, py::round_to3(c.s_lo), py::round_to3(c.s_hi));
    let mut out = Vec::new();
    for (i, cand) in candidates.iter().enumerate() {
        let c = cand.cyl;
        let r = c.diameter / 2.0;
        let opposed = candidates.iter().enumerate().find(|(j, other)| {
            *j != i
                && other.cyl.axis == c.axis
                && same_axis_line(
                    c.axis,
                    c.axis_point,
                    c.direction,
                    other.cyl.axis_point,
                    other.cyl.direction,
                    r,
                )
                && stock(other.cyl) == stock(c)
                && (geom::dot(cand.normal, other.normal) + 1.0).abs() <= ANTIPARALLEL_TOL
        });
        let across = match opposed {
            Some((_, other)) => cand.s + other.s,
            None => cand.s + r,
        };
        out.push(Occurrence {
            record: Flat {
                axis: ['x', 'y', 'z'][c.axis],
                across: py::round_to3(across),
                at: Placed::new(cand.at, Grid::Decimals(3)),
                axis_line: axis_line_coordinates(c.axis, c.axis_point, c.direction),
                stock_span: Placed::new((c.s_lo, c.s_hi), Grid::Decimals(3)),
                axis_direction: canonical_axis_direction(c.axis, c.direction),
            },
            defining: vec![cand.face],
            context: vec![],
        });
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

/// Two cylinders share one axis line (`_same_axis_line`).
fn same_axis_line(axis: usize, a_ax: V3, a_dir: V3, b_ax: V3, b_dir: V3, radius: f64) -> bool {
    if canonical_axis_direction(axis, a_dir) != canonical_axis_direction(axis, b_dir) {
        return false;
    }
    let v = geom::sub(b_ax, a_ax);
    let along = v[0] * a_dir[0] + v[1] * a_dir[1] + v[2] * a_dir[2];
    let p = [0, 1, 2].map(|i| v[i] - along * a_dir[i]);
    (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).powf(0.5)
        <= policy::length_tol(radius, AXIS_LINE_FRAC)
}
