//! Horizontal face levels and step risers (`quiddity.levels`): the Z levels of a prismatic
//! part's horizontal planes, and the walls and ramps that rise between them, each found within
//! one solid so that support never bridges two bodies.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use super::Context;
use super::body::BodyKey;
use super::evidence::{self, EvidenceError, Occurrence};
use super::placed::{Grid, Placed};
use super::policy::{AXIS_ALIGNED_COS, AXIS_ZERO_COS, clears_threshold, cluster_coordinates};
use crate::kernel::brep::Part;
use crate::kernel::geom::{Bounds, COORD_FLOOR, Surface};
use crate::kernel::py;

/// Band within which two horizontal faces are one level (absolute, ADR 0008).
const TOL: f64 = 0.5;
/// A level whose faces cover less of the plan footprint is an incidental face, not a step.
const STEP_MIN_AREA_FRAC: f64 = 0.01;
/// A bounded slanted face is a structural ramp only at this fraction of the part on each axis.
const STRUCTURAL_RAMP_MIN_FRAC: f64 = 0.1;
/// A bounded riser must fill this fraction of its own footprint.
const BOUNDED_RISER_AREA_FRAC: f64 = 0.5;
/// The default end exclusion of a step level, absolute (`STEP_LADDER_BOUNDARY_MARGIN`, ADR
/// 0006)...
const STEP_LADDER_BOUNDARY_MARGIN: f64 = 0.6;
/// ...but never more than this fraction of the span (`_END_MARGIN_MAX_FRAC`, ADR 0008).
const END_MARGIN_MAX_FRAC: f64 = 0.25;
/// The aggregate's riser area floor (`_discover_risers`' `min_area_frac`).
const AGGREGATE_RISER_MIN_AREA_FRAC: f64 = 0.15;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct FaceLevel {
    pub z: f64,
    pub x_span: Option<(f64, f64)>,
    pub y_span: Option<(f64, f64)>,
    pub body_key: Option<BodyKey>,
}

impl FaceLevel {
    /// Python's `_order_key` comparison.
    fn order(&self, other: &Self) -> Ordering {
        let span = |s: &Option<(f64, f64)>| s.map(|(a, b)| vec![a, b]).unwrap_or_default();
        py::order(self.z, other.z)
            .then(self.x_span.is_some().cmp(&other.x_span.is_some()))
            .then_with(|| py::tuple_order(&span(&self.x_span), &span(&other.x_span)))
            .then(self.y_span.is_some().cmp(&other.y_span.is_some()))
            .then_with(|| py::tuple_order(&span(&self.y_span), &span(&other.y_span)))
            .then(self.body_key.is_some().cmp(&other.body_key.is_some()))
            .then_with(|| key_order(&self.body_key, &other.body_key))
    }
}

fn values(positions: &[Placed<f64>]) -> Vec<f64> {
    positions.iter().map(|p| **p).collect()
}

fn key_order(a: &Option<BodyKey>, b: &Option<BodyKey>) -> Ordering {
    py::tuple_order(
        a.as_ref().map_or(&[][..], |k| k.as_slice()),
        b.as_ref().map_or(&[][..], |k| k.as_slice()),
    )
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct RiserEvidence {
    pub vertical: bool,
    pub axis: char,
    pub positions: Vec<Placed<f64>>,
    pub other_axis: char,
    pub other_positions: Vec<Placed<f64>>,
    pub z_lo: f64,
    pub z_hi: f64,
    pub lo_at_envelope: bool,
    pub hi_at_envelope: bool,
    pub tol: f64,
    pub body_levels: Vec<FaceLevel>,
    pub body_key: Option<BodyKey>,
}

impl RiserEvidence {
    /// Python's `_order_key` comparison (recogniser records always carry `body_levels`).
    fn order(&self, other: &Self) -> Ordering {
        self.vertical
            .cmp(&other.vertical)
            .then(self.axis.cmp(&other.axis))
            .then_with(|| py::tuple_order(&values(&self.positions), &values(&other.positions)))
            .then(self.other_axis.cmp(&other.other_axis))
            .then_with(|| {
                py::tuple_order(
                    &values(&self.other_positions),
                    &values(&other.other_positions),
                )
            })
            .then(py::order(self.z_lo, other.z_lo))
            .then(py::order(self.z_hi, other.z_hi))
            .then(self.lo_at_envelope.cmp(&other.lo_at_envelope))
            .then(self.hi_at_envelope.cmp(&other.hi_at_envelope))
            .then(py::order(self.tol, other.tol))
            .then_with(|| {
                // Tuples of levels: element by element, then by length.
                self.body_levels
                    .iter()
                    .zip(&other.body_levels)
                    .map(|(a, b)| a.order(b))
                    .find(|o| o.is_ne())
                    .unwrap_or(self.body_levels.len().cmp(&other.body_levels.len()))
            })
            .then(self.body_key.is_some().cmp(&other.body_key.is_some()))
            .then_with(|| key_order(&self.body_key, &other.body_key))
    }
}

/// Options named as `recognise_face_levels`' keyword arguments.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct FaceLevelOptions {
    pub tol: Option<f64>,
    pub min_area_frac: f64,
}

/// Options named as `recognise_risers`' keyword arguments.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct RiserOptions {
    pub min_area_frac: f64,
    pub tol: Option<f64>,
}

impl Default for RiserOptions {
    fn default() -> Self {
        RiserOptions {
            min_area_frac: 0.15,
            tol: None,
        }
    }
}

/// One body the levels are read within: a solid, or the whole part when it has none.
struct Scope {
    faces: Vec<usize>,
    bounds: Bounds,
    key: Option<BodyKey>,
    solid: Option<usize>,
}

fn scopes(ctx: &Context<'_>) -> Vec<Scope> {
    let part = ctx.part;
    if part.solids.is_empty() {
        return vec![Scope {
            faces: (0..part.faces.len()).collect(),
            bounds: part.bounds(),
            key: None,
            solid: None,
        }];
    }
    let keys = ctx.body_keys(true);
    part.solids
        .iter()
        .enumerate()
        .map(|(s, solid)| Scope {
            faces: solid.faces.clone(),
            bounds: part.solid_bounds(s),
            key: keys[s].clone(),
            solid: Some(s),
        })
        .collect()
}

/// The levels of one scope with the faces supporting each (`_face_level_proposals_one`).
fn level_proposals(
    part: &Part,
    scope: &Scope,
    tol: f64,
    min_area_frac: f64,
) -> Vec<(FaceLevel, Vec<usize>)> {
    let mut zs = Vec::new();
    let mut faces = Vec::new();
    for &f in &scope.faces {
        if let Surface::Plane { frame } = part.faces[f].surface
            && frame.z[2].abs() > AXIS_ALIGNED_COS
        {
            zs.push(frame.origin[2]);
            faces.push(f);
        }
    }
    let b = &scope.bounds;
    let threshold = min_area_frac * (b.max[0] - b.min[0]) * (b.max[1] - b.min[1]);
    let mut out = Vec::new();
    for cluster in cluster_coordinates(&zs, tol) {
        // A level whose area cannot be integrated is not proposed (not read as area 0).
        if min_area_frac > 0.0 {
            let areas: Option<Vec<f64>> = cluster
                .iter()
                .map(|&i| part.face_mass(faces[i]).map(|m| m[0]))
                .collect();
            if !areas.is_some_and(|a| clears_threshold(py::sum(a), threshold)) {
                continue;
            }
        }
        let spans: Vec<Bounds> = cluster
            .iter()
            .map(|&i| part.face_bounds(faces[i]))
            .collect();
        // Python's `min`/`max`: the first of equals wins, which decides a zero's sign.
        let fold = |pick: fn(&Bounds) -> f64, max: bool| {
            let mut it = spans.iter().map(pick);
            let first = it.next().unwrap();
            it.fold(first, |a, x| {
                let better = if max { x > a } else { x < a };
                if better { x } else { a }
            })
        };
        out.push((
            FaceLevel {
                z: zs[cluster[py::first_min(&cluster, |&i| zs[i])]],
                x_span: Some((fold(|b| b.min[0], false), fold(|b| b.max[0], true))),
                y_span: Some((fold(|b| b.min[1], false), fold(|b| b.max[1], true))),
                body_key: scope.key.clone(),
            },
            cluster.iter().map(|&i| faces[i]).collect(),
        ));
    }
    out
}

internal_entry! {
/// `recognise_face_levels`.
pub fn recognise_face_levels(part: &Part, opts: &FaceLevelOptions) -> Vec<FaceLevel> {
    face_levels_with_faces(part, opts)
        .into_iter()
        .map(|(level, _)| level)
        .collect()
}
}

internal_entry! {
/// `recognise_face_levels`' levels, each with the faces it was read from (not an evidence
/// path: the invariance test compares them).
pub fn face_levels_with_faces(
    part: &Part,
    opts: &FaceLevelOptions,
) -> Vec<(FaceLevel, Vec<usize>)> {
    let ctx = Context::new(part);
    let tol = opts.tol.unwrap_or(TOL);
    let mut out: Vec<(FaceLevel, Vec<usize>)> = scopes(&ctx)
        .iter()
        .flat_map(|scope| level_proposals(part, scope, tol, opts.min_area_frac))
        .collect();
    out.sort_by(|a, b| a.0.order(&b.0));
    out
}
}

/// The scope's interior step levels with their faces: area-filtered, strictly inside its height
/// by *margin*.
fn interior_levels(part: &Part, scope: &Scope, margin: f64) -> Vec<(FaceLevel, Vec<usize>)> {
    let (lo, hi) = (scope.bounds.min[2], scope.bounds.max[2]);
    level_proposals(part, scope, TOL, STEP_MIN_AREA_FRAC)
        .into_iter()
        .filter(|(level, _)| lo + margin < level.z && level.z < hi - margin)
        .collect()
}

/// The scope's interior step levels: area-filtered, strictly inside its height by *tol*.
fn body_levels(part: &Part, scope: &Scope, tol: f64) -> Vec<FaceLevel> {
    interior_levels(part, scope, tol)
        .into_iter()
        .map(|(level, _)| level)
        .collect()
}

internal_entry! {
/// The end exclusion for a span (`bounded_end_margin`): absolute, but never more than a quarter
/// of it.
pub fn bounded_end_margin(span: f64) -> f64 {
    STEP_LADDER_BOUNDARY_MARGIN.min(span.max(0.0) * END_MARGIN_MAX_FRAC)
}
}

/// The scope's end exclusion: *tol*, or by default [`bounded_end_margin`] of its height.
fn end_margin(scope: &Scope, tol: Option<f64>) -> f64 {
    tol.unwrap_or_else(|| bounded_end_margin(scope.bounds.max[2] - scope.bounds.min[2]))
}

internal_entry! {
/// `step_level_records`: the area-filtered interior face levels of each body, strictly inside
/// its height by *tol* (by default [`bounded_end_margin`] of that height).
pub fn step_level_records(part: &Part, tol: Option<f64>) -> Vec<FaceLevel> {
    let ctx = Context::new(part);
    let mut out: Vec<FaceLevel> = scopes(&ctx)
        .iter()
        .flat_map(|scope| interior_levels(part, scope, end_margin(scope, tol)))
        .map(|(level, _)| level)
        .collect();
    out.sort_by(|a, b| a.order(b));
    out
}
}

internal_entry! {
/// The aggregate's step levels (`_discover_step_levels`), each defined by its body's horizontal
/// faces at that level, as [`step_level_records`] gives them by default. Not proved: see
/// [`proved`].
pub fn discover_step_levels(ctx: &Context<'_>) -> Vec<Occurrence<FaceLevel>> {
    let mut out: Vec<Occurrence<FaceLevel>> = scopes(ctx)
        .iter()
        .flat_map(|scope| interior_levels(ctx.part, scope, end_margin(scope, None)))
        .map(|(record, defining)| Occurrence {
            record,
            defining,
            context: Vec::new(),
        })
        .collect();
    out.sort_by(|a, b| a.record.order(&b.record));
    out
}
}

internal_entry! {
/// The aggregate's risers (`_discover_risers`: `min_area_frac` 0.15, the default tolerance),
/// each defined by the faces it was read from and carrying, as its `body_levels`, the run's
/// *levels* on its own body ([`discover_step_levels`], after any proof). Not proved: see
/// [`proved`].
pub fn discover_risers(
    ctx: &Context<'_>,
    levels: &[Occurrence<FaceLevel>],
) -> Vec<Occurrence<RiserEvidence>> {
    let part = ctx.part;
    let body = |o: &Occurrence<FaceLevel>| o.defining.first().and_then(|&f| part.faces[f].solid);
    let mut out: Vec<Occurrence<RiserEvidence>> = Vec::new();
    for scope in scopes(ctx) {
        let own: Vec<FaceLevel> = levels
            .iter()
            .filter(|o| body(o) == scope.solid)
            .map(|o| o.record.clone())
            .collect();
        for (mut record, defining) in
            riser_proposals(part, &scope, AGGREGATE_RISER_MIN_AREA_FRAC, TOL, vec![])
        {
            record.body_levels = own.clone();
            out.push(Occurrence {
                record,
                defining,
                context: Vec::new(),
            });
        }
    }
    out.sort_by(|a, b| a.record.order(&b.record));
    out
}
}

internal_entry! {
/// The aggregate's proof of step levels or risers: strictly (`local_degradation` false) every
/// occurrence's faces on one valid solid, or Python's refusal; under local degradation, those a
/// locally degraded run proves ([`evidence::locally_valid_solid`]), the rest skipped as Python
/// skips them.
pub fn proved<R>(
    part: &Part,
    found: Vec<Occurrence<R>>,
    local_degradation: bool,
) -> Result<Vec<Occurrence<R>>, EvidenceError> {
    if !local_degradation {
        return evidence::verified(part, found);
    }
    Ok(found
        .into_iter()
        .filter(|o| evidence::locally_valid_solid(part, &o.defining).is_some())
        .collect())
}
}

internal_entry! {
/// `recognise_risers`.
pub fn recognise_risers(part: &Part, opts: &RiserOptions) -> Vec<RiserEvidence> {
    risers_with_faces(part, opts)
        .into_iter()
        .map(|(riser, _)| riser)
        .collect()
}
}

internal_entry! {
/// `recognise_risers`' risers, each with the faces it was read from (not an evidence path: the
/// invariance test compares them).
pub fn risers_with_faces(part: &Part, opts: &RiserOptions) -> Vec<(RiserEvidence, Vec<usize>)> {
    let ctx = Context::new(part);
    let tol = opts.tol.unwrap_or(TOL);
    let mut out: Vec<(RiserEvidence, Vec<usize>)> = scopes(&ctx)
        .iter()
        .flat_map(|scope| {
            let levels = body_levels(part, scope, tol);
            riser_proposals(part, scope, opts.min_area_frac, tol, levels)
        })
        .collect();
    out.sort_by(|a, b| a.0.order(&b.0));
    out
}
}

/// A riser's facing: `(vertical, axis)` when the normal lies along exactly one in-plane axis
/// (`_riser_orientation`).
fn riser_orientation(n: [f64; 3]) -> Option<(bool, usize)> {
    let on_x = n[0].abs() > AXIS_ZERO_COS && n[1].abs() <= AXIS_ZERO_COS;
    let on_y = n[1].abs() > AXIS_ZERO_COS && n[0].abs() <= AXIS_ZERO_COS;
    if !(on_x || on_y) {
        return None;
    }
    Some((n[2].abs() <= AXIS_ZERO_COS, if on_x { 0 } else { 1 }))
}

/// The risers of one scope, equal records merged with all their faces (`_riser_proposals_one`).
fn riser_proposals(
    part: &Part,
    scope: &Scope,
    min_area_frac: f64,
    tol: f64,
    body_levels: Vec<FaceLevel>,
) -> Vec<(RiserEvidence, Vec<usize>)> {
    let bb = &scope.bounds;
    let ext = |i: usize| bb.max[i] - bb.min[i];
    let mut found: Vec<(RiserEvidence, Vec<usize>)> = Vec::new();
    for &f in &scope.faces {
        let Surface::Plane { frame } = part.faces[f].surface else {
            continue;
        };
        let Some(normal) = part.face_normal(f, 0.0, 0.0) else {
            continue;
        };
        let Some((vertical, axis)) = riser_orientation(normal) else {
            continue;
        };
        let fb = part.face_bounds(f);
        let other = 1 - axis;
        let (flo, fhi) = (fb.min[other], fb.max[other]);
        // A step crosses its whole body on the other in-plane axis; a pad's or a blind
        // pocket's walls are bounded.
        let full_span = flo <= bb.min[other] + tol && fhi >= bb.max[other] - tol;
        let interior = |i: usize, p: f64| {
            p - bb.min[i] > tol + COORD_FLOOR && bb.max[i] - p > tol + COORD_FLOOR
        };
        let height = fb.max[2] - fb.min[2];
        let (positions, other_positions) = if vertical {
            let pos = frame.origin[axis];
            if !full_span || !interior(axis, pos) {
                continue;
            }
            (vec![pos], vec![])
        } else {
            if tol >= height {
                continue;
            }
            let along = vec![fb.min[axis], fb.max[axis]];
            if full_span {
                (along, vec![])
            } else if fhi - flo < STRUCTURAL_RAMP_MIN_FRAC * ext(other)
                || along[1] - along[0] < STRUCTURAL_RAMP_MIN_FRAC * ext(axis)
                || STRUCTURAL_RAMP_MIN_FRAC * ext(2) > height
            {
                continue; // an edge-break chamfer, not a structural ramp
            } else {
                (along, vec![flo, fhi])
            }
        };
        let cross = ext(other) * ext(2);
        let area_floor = if full_span {
            min_area_frac * cross
        } else {
            BOUNDED_RISER_AREA_FRAC * ((fhi - flo) * height)
        };
        let Some([area, _]) = part.face_mass(f) else {
            continue;
        };
        if cross <= 0.0 || area < area_floor {
            continue;
        }
        let at_envelope = |z: f64| (z - bb.min[2]).abs() < tol || (z - bb.max[2]).abs() < tol;
        let record = RiserEvidence {
            vertical,
            axis: ['x', 'y'][axis],
            positions: positions
                .into_iter()
                .filter(|&p| interior(axis, p))
                .map(|p| Placed::new(p, Grid::Decimals(3)))
                .collect(),
            other_axis: ['x', 'y'][other],
            other_positions: other_positions
                .into_iter()
                .filter(|&p| interior(other, p))
                .map(|p| Placed::new(p, Grid::Decimals(3)))
                .collect(),
            z_lo: fb.min[2],
            z_hi: fb.max[2],
            lo_at_envelope: at_envelope(fb.min[2]),
            hi_at_envelope: at_envelope(fb.max[2]),
            tol,
            body_levels: body_levels.clone(),
            body_key: scope.key.clone(),
        };
        match found.iter_mut().find(|(r, _)| *r == record) {
            Some((_, faces)) => faces.push(f),
            None => found.push((record, vec![f])),
        }
    }
    found.sort_by(|a, b| a.0.order(&b.0));
    found
}
