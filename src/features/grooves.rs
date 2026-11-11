//! Turned / circlip grooves on round stock (`quiddity.grooves.recognise_grooves`): a narrow band
//! of the outside diameter that is a strict local minimum between two contiguous larger bands on
//! the same shaft, whose walls step back to (nearly) the same OD.
//!
//! The two walls are implied by the band structure, so no wall-face search is needed. Bands
//! count as contiguous when they touch, when a coaxial cone lands on both rims (a chamfered
//! lead-in), or when a chain of coaxial tori and transverse planes joins them (a radiused one).
//! Bands are grouped by axis line and solid, so grooves on parallel shafts or butted bodies are
//! never confused.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::Context;
use super::cylinders::{CylinderEvidence, z_then_cross};
use super::evidence::{self, EvidenceError, Occurrence};
use super::placed::{Grid, Placed};
use super::policy;
use super::turned::{TurnedProfileKey, axis_letter, profile_key_from_bands};
use crate::kernel::brep::Part;
use crate::kernel::geom::{self, Curve, Surface, V3};
use crate::kernel::py;

// Every gate is a fraction of the band it judges (ADR 0008), except the minimum-evidence
// thresholds, which are absolute.
/// Two bands are axially contiguous when their gap is within this fraction of the band diameter.
const ADJ_FRAC: f64 = 0.0125;
/// A neighbour must be wider than the floor by more than this (mm of diameter).
const DIA_MARGIN: f64 = 0.2;
/// The two walls step back to the same OD within this fraction of the wider wall.
const WALL_DIA_FRAC: f64 = 0.0625;
/// The floor is narrower than the wider wall by more than this (mm).
const WIDTH_MARGIN: f64 = 0.05;

/// A recognised turned groove. `axis` is the turning axis, `width` the floor's axial span,
/// `diameter` the floor diameter and `at` the floor face's box centre (the leader tip).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Groove {
    pub axis: char,
    pub width: f64,
    pub diameter: f64,
    pub at: Placed<V3>,
    pub profile: Option<TurnedProfileKey>,
}

internal_entry! {
/// `recognise_grooves`: sorted by axis, then `at`.
pub fn recognise_grooves(part: &Part) -> Vec<Groove> {
    super::records(discover(&Context::new(part)))
}
}

internal_entry! {
/// The evidence path: each groove with its floor band, on one valid solid, and no turned
/// profile key shared by two solids.
pub fn discover_verified(ctx: &Context<'_>) -> Result<Vec<Occurrence<Groove>>, EvidenceError> {
    let found = evidence::verified(ctx.part, discover(ctx))?;
    let mut owners: Vec<(&TurnedProfileKey, usize)> = Vec::new();
    for o in &found {
        let owner = evidence::common_valid_solid(ctx.part, &o.defining)
            .ok_or(EvidenceError::NoValidSolid)?;
        let profile = o.record.profile.as_ref().expect("grooves carry a profile");
        match owners.iter().find(|(p, _)| *p == profile) {
            Some(&(_, previous)) if previous != owner => {
                return Err(EvidenceError::AmbiguousProfile);
            }
            Some(_) => {}
            None => owners.push((profile, owner)),
        }
    }
    Ok(found)
}
}

/// A conical face as its axis direction and its circular rims (centre, radius).
type ConeJoin = (V3, Vec<(V3, f64)>);

/// `_cone_joins`: every native cone with at least two circular edges.
fn cone_joins(part: &Part) -> Vec<ConeJoin> {
    let mut joins = Vec::new();
    for (face, f) in part.faces.iter().enumerate() {
        let Surface::Cone { frame, .. } = f.surface else {
            continue;
        };
        let rims: Vec<(V3, f64)> = part
            .face_edges(face)
            .into_iter()
            .filter_map(|e| match part.edges[e].curve {
                Curve::Circle { frame, radius } => Some((frame.origin, radius)),
                _ => None,
            })
            .collect();
        if rims.len() < 2 {
            continue; // a drill-point cone joins nothing
        }
        joins.push((frame.z, rims));
    }
    joins
}

fn dot3(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `_axis_span`: the face box's conservative span along *axis*.
fn axis_span(part: &Part, face: usize, axis: V3) -> (f64, f64) {
    let bb = part.face_bounds(face);
    let lo = (0..3)
        .map(|i| (axis[i] * bb.min[i]).min(axis[i] * bb.max[i]))
        .fold(0.0, |a, b| a + b);
    let hi = (0..3)
        .map(|i| (axis[i] * bb.min[i]).max(axis[i] * bb.max[i]))
        .fold(0.0, |a, b| a + b);
    (lo, hi)
}

/// `_torus_joined`: whether a walk from *lower* through coaxial tori and transverse planes in
/// the axial gap, crossing at least one torus, reaches *upper*.
fn torus_joined(part: &Part, lower: &CylinderEvidence, upper: &CylinderEvidence, tol: f64) -> bool {
    let axis = lower.direction;
    let (lo, hi) = (lower.s_hi - tol, upper.s_lo + tol);
    let axis_point = lower.axis_point;
    let transition = |face: usize| -> bool {
        match part.faces[face].surface {
            Surface::Torus { frame, .. } => {
                if dot3(frame.z, axis).abs() < 1.0 - 1e-6 {
                    return false;
                }
                let offset = geom::sub(frame.origin, axis_point);
                let axial = dot3(offset, axis);
                let off: f64 = (0..3)
                    .map(|i| (offset[i] - axial * axis[i]).powi(2))
                    .fold(0.0, |a, b| a + b);
                if off > tol * tol {
                    return false;
                }
            }
            Surface::Plane { frame } => {
                if dot3(frame.z, axis).abs() < 1.0 - 1e-6 {
                    return false;
                }
            }
            _ => return false,
        }
        let (face_lo, face_hi) = axis_span(part, face, axis);
        face_hi >= lo && face_lo <= hi
    };
    let mut seen = BTreeSet::from([(lower.face, false)]);
    let mut pending = vec![(lower.face, false)];
    while let Some((face, crossed)) = pending.pop() {
        for other in part.neighbours(face) {
            if other == upper.face {
                if crossed {
                    return true;
                }
                continue;
            }
            if !transition(other) {
                continue;
            }
            let state = (
                other,
                crossed || matches!(part.faces[other].surface, Surface::Torus { .. }),
            );
            if seen.insert(state) {
                pending.push(state);
            }
        }
    }
    false
}

/// `_joined`: whether *upper* follows *lower* along the shaft, directly, across a conical
/// lead-in landing on both rims, or across a radiused one.
fn joined(
    part: &Part,
    lower: &CylinderEvidence,
    upper: &CylinderEvidence,
    tol: f64,
    cones: &[ConeJoin],
    has_tori: bool,
) -> bool {
    if (lower.s_hi - upper.s_lo).abs() <= tol {
        return true;
    }
    let axis = lower.direction;
    for (direction, rims) in cones {
        if dot3(*direction, axis).abs() < 1.0 - 1e-6 {
            continue; // not coaxial with this shaft
        }
        let mut ends: Vec<[f64; 2]> = rims.iter().map(|(c, r)| [dot3(*c, axis), *r]).collect();
        ends.sort_by(|a, b| py::tuple_order(a, b));
        let ([s_lo, r_lo], [s_hi, r_hi]) = (ends[0], ends[ends.len() - 1]);
        if (s_lo - lower.s_hi).abs() <= tol
            && (s_hi - upper.s_lo).abs() <= tol
            && (2.0 * r_lo - lower.diameter).abs() <= tol
            && (2.0 * r_hi - upper.diameter).abs() <= tol
        {
            return true;
        }
    }
    has_tori && torus_joined(part, lower, upper, tol)
}

/// The axis index, the owning solid and the axis line's point nearest the origin.
type ShaftKey = (usize, usize, V3);

/// `_shaft_key`: the axis letter, the owning solid and the axis line's point on the plane
/// through the origin square to it (to 2 dp).
fn shaft_key(c: &CylinderEvidence) -> ShaftKey {
    let [px, py, pz] = c.axis_point;
    let [dx, dy, dz] = c.direction;
    let t = px * dx + py * dy + pz * dz;
    (
        c.axis,
        c.solid,
        [
            py::round_to(px - t * dx, 2),
            py::round_to(py - t * dy, 2),
            py::round_to(pz - t * dz, 2),
        ],
    )
}

internal_entry! {
/// Each groove with its floor band as the defining face, sorted by axis then `at`.
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<Groove>> {
    let part = ctx.part;
    let (z, cross) = z_then_cross(ctx.cylinders());
    let ext: Vec<&CylinderEvidence> = z.iter().chain(&cross).filter(|c| c.external).collect();
    if ext.is_empty() {
        return vec![];
    }
    let mut shafts: Vec<(ShaftKey, Vec<&CylinderEvidence>)> = Vec::new();
    for c in ext {
        let key = shaft_key(c);
        match shafts.iter_mut().find(|(k, _)| *k == key) {
            Some((_, members)) => members.push(c),
            None => shafts.push((key, vec![c])),
        }
    }
    let cones = cone_joins(part);
    let has_tori = part
        .faces
        .iter()
        .any(|f| matches!(f.surface, Surface::Torus { .. }));
    let keys = ctx.body_keys(true);
    let mut out: Vec<Occurrence<Groove>> = Vec::new();
    for ((axis, solid, _), bands) in shafts {
        let (bounds, key) = if part.solids.is_empty() {
            (*ctx.bounds(), None)
        } else {
            (part.solid_bounds(solid), keys[solid].clone())
        };
        let profile = profile_key_from_bands(axis, &bands, &bounds, key);
        for &cur in &bands {
            let adj_tol = policy::length_tol(cur.diameter, ADJ_FRAC);
            // Split and overlapping bands need not be neighbours in start-coordinate order.
            // Find actual supports at BOTH floor ends. Reversing the axis exchanges these
            // sets, leaving the existence of a matching pair unchanged.
            let lower: Vec<_> = bands
                .iter()
                .copied()
                .filter(|b| {
                    b.diameter >= cur.diameter + DIA_MARGIN
                        && b.s_lo < cur.s_lo - geom::COORD_FLOOR
                        && b.s_hi <= cur.s_lo + adj_tol
                        && joined(part, b, cur, adj_tol, &cones, has_tori)
                })
                .collect();
            let upper: Vec<_> = bands
                .iter()
                .copied()
                .filter(|b| {
                    b.diameter >= cur.diameter + DIA_MARGIN
                        && b.s_hi > cur.s_hi + geom::COORD_FLOOR
                        && b.s_lo >= cur.s_hi - adj_tol
                        && joined(part, cur, b, adj_tol, &cones, has_tori)
                })
                .collect();
            let cur_w = cur.s_hi - cur.s_lo;
            if !lower.iter().any(|prev| {
                upper.iter().any(|next| {
                    let wider_dia = prev.diameter.max(next.diameter);
                    let wider_wall = (prev.s_hi - prev.s_lo).max(next.s_hi - next.s_lo);
                    (prev.diameter - next.diameter).abs()
                        <= policy::length_tol(wider_dia, WALL_DIA_FRAC)
                        && cur_w < wider_wall - WIDTH_MARGIN
                })
            }) {
                continue;
            }
            // A helical root may sit between two larger bands too. Its independently proved
            // thread record carries the root and all measurements; it is not an annular groove.
            if ctx
                .modelled_threads()
                .threads
                .iter()
                .any(|t| t.record.root_faces.contains(&cur.face))
            {
                continue;
            }
            let bb = part.face_bounds(cur.face);
            let at = Placed::new(
                std::array::from_fn(|k| 0.5 * (bb.min[k] + bb.max[k])),
                Grid::Decimals(3),
            );
            out.push(Occurrence {
                record: Groove {
                    axis: axis_letter(cur.axis),
                    width: py::round_to(cur.s_hi - cur.s_lo, 3),
                    diameter: py::round_to(cur.diameter, 3),
                    at,
                    profile: Some(profile.clone()),
                },
                defining: vec![cur.face],
                context: vec![],
            });
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
