//! Axial steps of a stepped turned part (`quiddity.turned.recognise_turned_steps`): the segments
//! between the shoulders of one coaxial external profile, each with the outside diameter over
//! it, so every step *length* can be dimensioned.
//!
//! 1. The turning axis is the commonest among external cylinders with more than half a turn of
//!    support; bands on a supported axis line belong to its profile even when interrupted.
//! 2. Shoulders and ends are the body's planes square to the axis whose outer radius reaches
//!    the local outside diameter, within a chamfer allowance.
//! 3. Consecutive shoulders delimit steps; adjacent steps of one diameter coalesce, and a span
//!    with no band over it is a gap, not a step.

use serde::{Deserialize, Serialize};

use super::Context;
use super::body::BodyKey;
use super::cylinders::{
    CylinderEvidence, canonical_axis_direction, full_cylinders, line_key, z_then_cross,
};
use super::evidence::{self, EvidenceError, Occurrence};
use super::placed::{Grid, Placed};
use super::planes::effective_plane;
use super::turned::{TurnedProfileKey, axis_letter, profile_key_from_bands};
use crate::kernel::brep::Part;
use crate::kernel::geom::{Bounds, dominant_axis_preferring_z};
use crate::kernel::py;

/// Pad on a band's axial span when no band contains a position (an edge break between bands).
/// Absolute by design (ADR 0008): it spans an edge break, which does not grow with the shaft.
const OD_SPAN_PAD: f64 = 0.7;
/// A transverse face is a shoulder when its outer radius is within this of the local radius
/// (an absolute edge break plus a chamfer scaling with the feature), capped at half the radius.
const CHAMFER_ALLOWANCE_ABS: f64 = 0.5;
const CHAMFER_ALLOWANCE_FRAC: f64 = 0.12;
/// A turned body's cross-section square to the axis is roughly square and filled by its OD.
const SQUARENESS_TOL: f64 = 0.15;
const OD_FILL_MIN: f64 = 0.6;

/// One axial segment of a stepped shaft, between two shoulders (or ends), measured along `axis`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TurnedStep {
    pub axis: char,
    pub lo: Placed<f64>,
    pub hi: Placed<f64>,
    /// The external diameter over this segment.
    pub diameter: f64,
    pub profile: Option<TurnedProfileKey>,
}

impl TurnedStep {
    pub fn length(&self) -> f64 {
        *self.hi - *self.lo
    }
}

internal_entry! {
/// `recognise_turned_steps`: sorted by profile, then axis, `lo`, `hi` and diameter.
pub fn recognise_turned_steps(part: &Part) -> Vec<TurnedStep> {
    sorted(super::records(discover(&Context::new(part))))
}
}

internal_entry! {
/// Steps in the order `recognise_turned_steps` returns them (`discover` keeps proposal order,
/// which is the order Python issues their evidence in).
pub fn sorted(mut steps: Vec<TurnedStep>) -> Vec<TurnedStep> {
    steps.sort_by(order);
    steps
}
}

internal_entry! {
/// Occurrences in the order [`sorted`] gives their records.
pub fn sorted_occurrences(mut found: Vec<Occurrence<TurnedStep>>) -> Vec<Occurrence<TurnedStep>> {
    found.sort_by(|a, b| order(&a.record, &b.record));
    found
}
}

fn order(a: &TurnedStep, b: &TurnedStep) -> std::cmp::Ordering {
    a.profile
        .is_some()
        .cmp(&b.profile.is_some())
        .then_with(|| match (&a.profile, &b.profile) {
            (Some(p), Some(q)) => p.order(q),
            _ => std::cmp::Ordering::Equal,
        })
        .then(a.axis.cmp(&b.axis))
        .then(py::order(*a.lo, *b.lo))
        .then(py::order(*a.hi, *b.hi))
        .then(py::order(a.diameter, b.diameter))
}

internal_entry! {
/// The evidence path: every step with the bands that set its diameter, all on one valid solid.
pub fn discover_verified(ctx: &Context<'_>) -> Result<Vec<Occurrence<TurnedStep>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx))
}
}

/// One body the profile is read within: a solid, or the whole part when it has none.
struct Scope<'c> {
    faces: Vec<usize>,
    bounds: Bounds,
    key: Option<BodyKey>,
    /// The inventory's external and internal cylinders of this body, z-axis ones first.
    cyls: Vec<&'c CylinderEvidence>,
}

internal_entry! {
/// Each step in proposal order (body, then axis line, then along the axis) with the bands that
/// set its diameter as its defining faces.
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<TurnedStep>> {
    let part = ctx.part;
    let (z, cross) = z_then_cross(ctx.cylinders());
    let inventory: Vec<&CylinderEvidence> = z.iter().chain(&cross).collect();
    let scopes: Vec<Scope> = if part.solids.is_empty() {
        vec![Scope {
            faces: (0..part.faces.len()).collect(),
            bounds: *ctx.bounds(),
            key: None,
            cyls: inventory.clone(),
        }]
    } else {
        let keys = ctx.body_keys(true);
        let single = part.solids.len() == 1;
        part.solids
            .iter()
            .enumerate()
            .map(|(s, solid)| Scope {
                faces: solid.faces.clone(),
                bounds: part.solid_bounds(s),
                key: keys[s].clone(),
                cyls: inventory
                    .iter()
                    .copied()
                    .filter(|c| single || c.solid == s)
                    .collect(),
            })
            .collect()
    };
    let mut out = Vec::new();
    for scope in &scopes {
        for (step, over) in proposals_one(ctx, scope) {
            out.push(Occurrence {
                record: step,
                defining: over.iter().map(|c| c.face).collect(),
                context: vec![],
            });
        }
    }
    out
}
}

type Proposal<'c> = (TurnedStep, Vec<&'c CylinderEvidence>);

/// `_turned_step_proposals_one`: the body's profiles, one per supported axis line.
fn proposals_one<'c>(ctx: &Context<'_>, scope: &Scope<'c>) -> Vec<Proposal<'c>> {
    let ext: Vec<&CylinderEvidence> = scope.cyls.iter().copied().filter(|c| c.external).collect();
    let owned: Vec<CylinderEvidence> = ext.iter().map(|c| (*c).clone()).collect();
    let supported = full_cylinders(&owned);
    if supported.is_empty() {
        return vec![];
    }
    // Cylinder support establishes a unique modal axis; equal support refuses.
    // Input order and the z-first inventory are not material-axis evidence.
    let mut counts: Vec<(usize, usize)> = Vec::new();
    for c in &supported {
        match counts.iter_mut().find(|(a, _)| *a == c.axis) {
            Some((_, n)) => *n += 1,
            None => counts.push((c.axis, 1)),
        }
    }
    let most = counts.iter().map(|c| c.1).max().unwrap();
    let modes: Vec<_> = counts.iter().filter(|c| c.1 == most).collect();
    let [mode] = modes.as_slice() else {
        return vec![];
    };
    let axis = mode.0;
    let lines: Vec<Vec<f64>> = supported
        .iter()
        .filter(|c| c.axis == axis)
        .map(line_key)
        .collect();
    let mut groups: Vec<(Vec<f64>, Vec<&CylinderEvidence>)> = Vec::new();
    for band in ext {
        let key = line_key(band);
        if !lines.contains(&key) {
            continue;
        }
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, members)) => members.push(band),
            None => groups.push((key, vec![band])),
        }
    }
    groups.sort_by(|a, b| py::tuple_order(&a.0, &b.0));
    groups
        .iter()
        .flat_map(|(_, bands)| proposals_coaxial(ctx, scope, axis, bands))
        .collect()
}

/// `_turned_step_proposals_coaxial`: shoulders and local diameters along one axis line.
fn proposals_coaxial<'c>(
    ctx: &Context<'_>,
    scope: &Scope<'c>,
    axis: usize,
    bands: &[&'c CylinderEvidence],
) -> Vec<Proposal<'c>> {
    let part = ctx.part;
    let mut diameters: Vec<f64> = bands.iter().map(|c| py::round_to(c.diameter, 2)).collect();
    diameters.sort_by(|a, b| py::order(*a, *b));
    diameters.dedup();
    if diameters.len() < 2 {
        return vec![]; // one OD: not a stepped turned part
    }
    // A turned body fills a roughly square cross-section with its largest band; an incidental
    // cylinder on a prismatic part does not.
    let b = &scope.bounds;
    let perp: Vec<f64> = (0..3)
        .filter(|&i| i != axis)
        .map(|i| b.max[i] - b.min[i])
        .collect();
    let cross = perp[0].max(perp[1]);
    let max_od = bands
        .iter()
        .map(|c| c.diameter)
        .fold(f64::NEG_INFINITY, f64::max);
    if cross <= 0.0
        || max_od < OD_FILL_MIN * cross
        || ((perp[0] - perp[1]).abs() > SQUARENESS_TOL * cross
            && super::turned_bodies::flat_interrupted_faces(ctx, &scope.faces).is_none())
    {
        return vec![];
    }
    let profile = profile_key_from_bands(axis, bands, &scope.bounds, scope.key.clone());

    // The widest bands over *pos*: those containing it, or, only when none does, those whose
    // span padded by an edge break reaches it.
    let bands_over = |pos: f64| -> Vec<&'c CylinderEvidence> {
        let mut over: Vec<&CylinderEvidence> = bands
            .iter()
            .copied()
            .filter(|c| c.s_lo <= pos && pos <= c.s_hi)
            .collect();
        if over.is_empty() {
            over = bands
                .iter()
                .copied()
                .filter(|c| {
                    let pad = OD_SPAN_PAD.min((c.s_hi - c.s_lo) / 2.0);
                    c.s_lo - pad <= pos && pos <= c.s_hi + pad
                })
                .collect();
        }
        let widest = over
            .iter()
            .map(|c| c.diameter)
            .fold(f64::NEG_INFINITY, f64::max);
        over.retain(|c| c.diameter == widest);
        over
    };
    let local_od = |pos: f64| bands_over(pos).first().map_or(0.0, |c| c.diameter / 2.0);

    let planes = shoulder_stations(part, &scope.faces, axis, &profile, local_od);
    if planes.len() < 3 {
        return vec![]; // fewer than two steps
    }
    let mut found: Vec<Proposal<'c>> = Vec::new();
    for w in planes.windows(2) {
        let (lo, hi) = (w[0], w[1]);
        let mut over = bands_over((*lo + *hi) / 2.0);
        if let Some(first) = over.first() {
            // Every source patch of this diameter within the step, not only those at its middle
            // (split thread crests); the padded selection when there are none.
            let within: Vec<&CylinderEvidence> = bands
                .iter()
                .copied()
                .filter(|c| c.diameter == first.diameter && c.s_hi > *lo && c.s_lo < *hi)
                .collect();
            if !within.is_empty() {
                over = within;
            }
        }
        let diameter = over.first().map_or(0.0, |c| c.diameter);
        if diameter <= 0.0 {
            continue; // a gap between disconnected bands, not a step
        }
        match found.last_mut() {
            Some((previous, sources)) if previous.hi == lo && previous.diameter == diameter => {
                previous.hi = hi;
                for band in over {
                    if !sources.iter().any(|s| std::ptr::eq(*s, band)) {
                        sources.push(band);
                    }
                }
            }
            _ => found.push((
                TurnedStep {
                    axis: axis_letter(axis),
                    lo,
                    hi,
                    diameter,
                    profile: Some(profile.clone()),
                },
                over,
            )),
        }
    }
    if found.len() < 2 {
        return vec![]; // fewer than two real steps: nothing to dimension axially
    }
    found
}

/// `_shoulder_stations`: the axial positions (to 3 dp, ascending, distinct) of the body's planes
/// square to the axis whose outer radius about the profile's axis reaches the local radius.
fn shoulder_stations(
    part: &Part,
    faces: &[usize],
    axis: usize,
    profile: &TurnedProfileKey,
    local_od: impl Fn(f64) -> f64,
) -> Vec<Placed<f64>> {
    let origin = profile.axis_origin;
    let mut shoulders: Vec<Placed<f64>> = Vec::new();
    for &face in faces {
        let Some((normal, offset)) = effective_plane(part, face) else {
            continue;
        };
        if dominant_axis_preferring_z(normal) != axis || !axis_aligned(axis, normal) {
            continue;
        }
        let bb = part.face_bounds(face);
        let others = (0..3).filter(|&j| j != axis);
        let pos = (offset - py::sum(others.clone().map(|j| normal[j] * origin[j]))) / normal[axis];
        let outer = others
            .map(|j| {
                (bb.min[j] - origin[j])
                    .abs()
                    .max((bb.max[j] - origin[j]).abs())
            })
            .fold(f64::NEG_INFINITY, f64::max);
        let od = local_od(pos);
        let allowance = (CHAMFER_ALLOWANCE_ABS + CHAMFER_ALLOWANCE_FRAC * od).min(od / 2.0);
        if outer >= od - allowance {
            let station = Placed::new(pos, Grid::Decimals(3));
            // Equal published stations are one shoulder; it keeps the least unrounded value,
            // so what is published in the file does not depend on face order.
            match shoulders.iter_mut().find(|s| **s == station) {
                Some(s) if station.raw() < s.raw() => *s = station,
                Some(_) => {}
                None => shoulders.push(station),
            }
        }
    }
    shoulders.sort_by(|a, b| py::order(**a, **b));
    shoulders
}

/// `_axis_direction_is_aligned`: the canonical direction is the axis within 1e-3.
fn axis_aligned(axis: usize, direction: [f64; 3]) -> bool {
    let v = canonical_axis_direction(axis, direction);
    (0..3).all(|i| {
        if i == axis {
            (v[i] - 1.0).abs() <= 1e-3
        } else {
            v[i].abs() <= 1e-3
        }
    })
}
