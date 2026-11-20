//! Face levels and step risers (`quiddity.levels`): the levels of a prismatic part's planes
//! square to one frame axis, and the walls and ramps that rise between them, each found within
//! one solid so that support never bridges two bodies. The aggregate reads them along the
//! frame's z first ([`discover_step_levels`]), then along its x and y ([`other_axes`]); a level
//! or riser carries its `height_axis`, and its `x_span`/`y_span` (`axis`/`other_axis`) are the
//! two axes across it in cyclic order ([`plane_axes`]).

use std::cmp::Ordering;
use std::collections::BTreeSet;

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
    /// The level's height along `height_axis`.
    pub z: f64,
    /// Its extent along the first axis across it ([`plane_axes`]: x for a z level).
    pub x_span: Option<(f64, f64)>,
    /// Its extent along the second axis across it (y for a z level).
    pub y_span: Option<(f64, f64)>,
    pub body_key: Option<BodyKey>,
    /// The frame axis the level is a height along; `'z'` (not serialised) for the original
    /// horizontal levels.
    #[serde(skip_serializing_if = "is_z")]
    pub height_axis: char,
}

fn is_z(axis: &char) -> bool {
    *axis == 'z'
}

/// Height axes in output order: the frame's z (the original levels), then x, then y.
fn axis_rank(axis: char) -> usize {
    match axis {
        'z' => 0,
        'x' => 1,
        _ => 2,
    }
}

const LETTERS: [char; 3] = ['x', 'y', 'z'];

internal_entry! {
/// The frame axes `[across₁, across₂, height]` of a level along frame axis *height*: the
/// cyclic order, so a z level reads x then y, an x level y then z, a y level z then x.
pub fn plane_axes(height: usize) -> [usize; 3] {
    [(height + 1) % 3, (height + 2) % 3, height]
}
}

impl FaceLevel {
    /// Python's `_order_key` comparison, after the height axis.
    fn order(&self, other: &Self) -> Ordering {
        let span = |s: &Option<(f64, f64)>| s.map(|(a, b)| vec![a, b]).unwrap_or_default();
        axis_rank(self.height_axis)
            .cmp(&axis_rank(other.height_axis))
            .then(py::order(self.z, other.z))
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
    /// The frame axis `z_lo`/`z_hi` and `vertical` refer to, as its levels'; `'z'` (not
    /// serialised) for the original risers.
    #[serde(skip_serializing_if = "is_z")]
    pub height_axis: char,
}

impl RiserEvidence {
    /// Python's `_order_key` comparison (recogniser records always carry `body_levels`),
    /// after the height axis.
    fn order(&self, other: &Self) -> Ordering {
        axis_rank(self.height_axis)
            .cmp(&axis_rank(other.height_axis))
            .then(self.vertical.cmp(&other.vertical))
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

/// The levels of one scope along frame axis *height* with the faces supporting each
/// (`_face_level_proposals_one`): its planes square to that axis, the area threshold taken in
/// the scope's own cross-section across it. A plane is read only when it is flat along that axis
/// ([`flat_along`]): a tilted face is not a level on any axis.
fn level_proposals(
    part: &Part,
    scope: &Scope,
    height: usize,
    tol: f64,
    min_area_frac: f64,
) -> Vec<(FaceLevel, Vec<usize>)> {
    let [u, v, h] = plane_axes(height);
    let mut zs = Vec::new();
    let mut faces = Vec::new();
    for &f in &scope.faces {
        if let Surface::Plane { frame } = part.faces[f].surface
            && frame.z[h].abs() > AXIS_ALIGNED_COS
            && flat_along(part, f, h)
        {
            zs.push(frame.origin[h]);
            faces.push(f);
        }
    }
    let b = &scope.bounds;
    let threshold = min_area_frac * (b.max[u] - b.min[u]) * (b.max[v] - b.min[v]);
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
        let fold = |i: usize, max: bool| {
            let pick = |b: &Bounds| if max { b.max[i] } else { b.min[i] };
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
                x_span: Some((fold(u, false), fold(u, true))),
                y_span: Some((fold(v, false), fold(v, true))),
                body_key: scope.key.clone(),
                height_axis: LETTERS[h],
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
        .flat_map(|scope| level_proposals(part, scope, 2, tol, opts.min_area_frac))
        .collect();
    out.sort_by(|a, b| a.0.order(&b.0));
    out
}
}

/// The scope's interior step levels along frame axis *height* with their faces: area-filtered,
/// strictly inside its extent along that axis by *margin*.
fn interior_levels(
    part: &Part,
    scope: &Scope,
    height: usize,
    margin: f64,
) -> Vec<(FaceLevel, Vec<usize>)> {
    let (lo, hi) = (scope.bounds.min[height], scope.bounds.max[height]);
    level_proposals(part, scope, height, TOL, STEP_MIN_AREA_FRAC)
        .into_iter()
        .filter(|(level, _)| lo + margin < level.z && level.z < hi - margin)
        .collect()
}

/// The scope's interior step levels: area-filtered, strictly inside its height by *tol*.
fn body_levels(part: &Part, scope: &Scope, tol: f64) -> Vec<FaceLevel> {
    interior_levels(part, scope, 2, tol)
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

/// The scope's end exclusion along frame axis *height*: *tol*, or by default
/// [`bounded_end_margin`] of its extent along that axis.
fn end_margin(scope: &Scope, height: usize, tol: Option<f64>) -> f64 {
    tol.unwrap_or_else(|| bounded_end_margin(scope.bounds.max[height] - scope.bounds.min[height]))
}

internal_entry! {
/// `step_level_records`: the area-filtered interior face levels of each body, strictly inside
/// its height by *tol* (by default [`bounded_end_margin`] of that height).
pub fn step_level_records(part: &Part, tol: Option<f64>) -> Vec<FaceLevel> {
    let ctx = Context::new(part);
    let mut out: Vec<FaceLevel> = scopes(&ctx)
        .iter()
        .flat_map(|scope| interior_levels(part, scope, 2, end_margin(scope, 2, tol)))
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
        .flat_map(|scope| interior_levels(ctx.part, scope, 2, end_margin(scope, 2, None)))
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
            riser_proposals(part, &scope, 2, AGGREGATE_RISER_MIN_AREA_FRAC, TOL, vec![])
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
            riser_proposals(part, scope, 2, opts.min_area_frac, tol, levels)
        })
        .collect();
    out.sort_by(|a, b| a.0.order(&b.0));
    out
}
}

/// A riser's facing across frame axis *height*: `(vertical, axis)` when the normal lies along
/// exactly one of the two axes across it (`_riser_orientation` for a z height).
fn riser_orientation(n: [f64; 3], height: usize) -> Option<(bool, usize)> {
    let [u, v, h] = plane_axes(height);
    let on_u = n[u].abs() > AXIS_ZERO_COS && n[v].abs() <= AXIS_ZERO_COS;
    let on_v = n[v].abs() > AXIS_ZERO_COS && n[u].abs() <= AXIS_ZERO_COS;
    if !(on_u || on_v) {
        return None;
    }
    Some((n[h].abs() <= AXIS_ZERO_COS, if on_u { u } else { v }))
}

/// The risers of one scope rising along frame axis *height*, equal records merged with all
/// their faces (`_riser_proposals_one` for a z height).
fn riser_proposals(
    part: &Part,
    scope: &Scope,
    height: usize,
    min_area_frac: f64,
    tol: f64,
    body_levels: Vec<FaceLevel>,
) -> Vec<(RiserEvidence, Vec<usize>)> {
    let bb = &scope.bounds;
    let ext = |i: usize| bb.max[i] - bb.min[i];
    let [u, v, h] = plane_axes(height);
    let mut found: Vec<(RiserEvidence, Vec<usize>)> = Vec::new();
    for &f in &scope.faces {
        let Surface::Plane { frame } = part.faces[f].surface else {
            continue;
        };
        let Some(normal) = part.face_normal(f, 0.0, 0.0) else {
            continue;
        };
        let Some((vertical, axis)) = riser_orientation(normal, height) else {
            continue;
        };
        let fb = part.face_bounds(f);
        let other = u + v - axis;
        let (flo, fhi) = (fb.min[other], fb.max[other]);
        // A step crosses its whole body on the other in-plane axis; a pad's or a blind
        // pocket's walls are bounded.
        let full_span = flo <= bb.min[other] + tol && fhi >= bb.max[other] - tol;
        let interior = |i: usize, p: f64| {
            p - bb.min[i] > tol + COORD_FLOOR && bb.max[i] - p > tol + COORD_FLOOR
        };
        let rise = fb.max[h] - fb.min[h];
        let (positions, other_positions) = if vertical {
            let pos = frame.origin[axis];
            if !full_span || !interior(axis, pos) {
                continue;
            }
            (vec![pos], vec![])
        } else {
            if tol >= rise {
                continue;
            }
            let along = vec![fb.min[axis], fb.max[axis]];
            if full_span {
                (along, vec![])
            } else if fhi - flo < STRUCTURAL_RAMP_MIN_FRAC * ext(other)
                || along[1] - along[0] < STRUCTURAL_RAMP_MIN_FRAC * ext(axis)
                || STRUCTURAL_RAMP_MIN_FRAC * ext(h) > rise
            {
                continue; // an edge-break chamfer, not a structural ramp
            } else {
                (along, vec![flo, fhi])
            }
        };
        let cross = ext(other) * ext(h);
        let area_floor = if full_span {
            min_area_frac * cross
        } else {
            BOUNDED_RISER_AREA_FRAC * ((fhi - flo) * rise)
        };
        let Some([area, _]) = part.face_mass(f) else {
            continue;
        };
        if cross <= 0.0 || area < area_floor {
            continue;
        }
        let at_envelope = |z: f64| (z - bb.min[h]).abs() < tol || (z - bb.max[h]).abs() < tol;
        let record = RiserEvidence {
            vertical,
            axis: LETTERS[axis],
            positions: positions
                .into_iter()
                .filter(|&p| interior(axis, p))
                .map(|p| Placed::new(p, Grid::Decimals(3)))
                .collect(),
            other_axis: LETTERS[other],
            other_positions: other_positions
                .into_iter()
                .filter(|&p| interior(other, p))
                .map(|p| Placed::new(p, Grid::Decimals(3)))
                .collect(),
            z_lo: fb.min[h],
            z_hi: fb.max[h],
            lo_at_envelope: at_envelope(fb.min[h]),
            hi_at_envelope: at_envelope(fb.max[h]),
            tol,
            body_levels: body_levels.clone(),
            body_key: scope.key.clone(),
            height_axis: LETTERS[h],
        };
        match found.iter_mut().find(|(r, _)| *r == record) {
            Some((_, faces)) => faces.push(f),
            None => found.push((record, vec![f])),
        }
    }
    found.sort_by(|a, b| a.0.order(&b.0));
    found
}

/// The aggregate's step levels and risers along the frame's x and y (#307 item 10), read with
/// the same rules as the z ones (area in the axis's own cross-section, the same end margin and
/// riser floors) and published with their `height_axis`. A plane has one normal, so a face is
/// a level along one axis at most. Unlike the z levels, which are evidence co-owning any face,
/// they are a miss rather than a second description: a level with a face that is one wall of a
/// slot or pocket ([`opposed_wall`]) is not proposed, and a level or riser is not proposed when
/// any of its faces is *occupied* (claimed by another family, refused, or body evidence) or already
/// *taken* by a z level or riser; a riser is read only along an axis its body has a level on,
/// not from a face of any level, and not from a face that is a riser along both x and y.
/// Risers carry their own axis's levels as `body_levels`.
pub(crate) fn other_axes(
    ctx: &Context<'_>,
    occupied: &BTreeSet<usize>,
    taken: &BTreeSet<usize>,
) -> (Vec<Occurrence<FaceLevel>>, Vec<Occurrence<RiserEvidence>>) {
    let part = ctx.part;
    let degraded = evidence::local_degradation();
    let free = |faces: &[usize], used: &BTreeSet<usize>| {
        faces
            .iter()
            .all(|f| !occupied.contains(f) && !taken.contains(f) && !used.contains(f))
            && (!degraded || evidence::locally_valid_solid(part, faces).is_some())
    };
    let none = BTreeSet::new();
    let mut levels: Vec<Occurrence<FaceLevel>> = Vec::new();
    let mut risers: Vec<Occurrence<RiserEvidence>> = Vec::new();
    for scope in scopes(ctx) {
        let found: Vec<Vec<(FaceLevel, Vec<usize>)>> = [0, 1]
            .map(|height| {
                interior_levels(part, &scope, height, end_margin(&scope, height, None))
                    .into_iter()
                    .filter(|(_, faces)| free(faces, &none))
                    .filter(|(_, faces)| {
                        !faces
                            .iter()
                            .any(|&f| opposed_wall(ctx, scope.solid, &scope.faces, f, height))
                    })
                    .collect()
            })
            .into();
        let used: BTreeSet<usize> = found
            .iter()
            .flatten()
            .flat_map(|(_, faces)| faces.iter().copied())
            .collect();
        // Each axis's risers, then those whose faces are risers along both x and y (a plane
        // square to the frame's z, say) dropped: which axis it rises along is ambiguous, and
        // taking the first would depend on how the part lies in its frame.
        let proposed: Vec<Vec<(RiserEvidence, Vec<usize>)>> = [0, 1]
            .into_iter()
            .zip(&found)
            .map(|(height, own)| {
                if own.is_empty() {
                    return Vec::new();
                }
                let own_levels: Vec<FaceLevel> = own.iter().map(|(l, _)| l.clone()).collect();
                riser_proposals(
                    part,
                    &scope,
                    height,
                    AGGREGATE_RISER_MIN_AREA_FRAC,
                    TOL,
                    own_levels,
                )
                .into_iter()
                .filter(|(_, defining)| free(defining, &used))
                .collect()
            })
            .collect();
        let faces_of = |k: usize| -> BTreeSet<usize> {
            proposed[k]
                .iter()
                .flat_map(|(_, faces)| faces.iter().copied())
                .collect()
        };
        let both: BTreeSet<usize> = faces_of(0).intersection(&faces_of(1)).copied().collect();
        risers.extend(
            proposed
                .into_iter()
                .flatten()
                .filter(|(_, defining)| defining.iter().all(|f| !both.contains(f)))
                .map(|(record, defining)| Occurrence {
                    record,
                    defining,
                    context: Vec::new(),
                }),
        );
        levels.extend(
            found
                .into_iter()
                .flatten()
                .map(|(record, defining)| Occurrence {
                    record,
                    defining,
                    context: Vec::new(),
                }),
        );
    }
    levels.sort_by(|a, b| a.record.order(&b.record));
    risers.sort_by(|a, b| a.record.order(&b.record));
    (levels, risers)
}

internal_entry! {
/// Whether face *f* spans at most the level band [`TOL`] along frame axis *h*: a level is a plane
/// square to its axis, not a drafted wall that [`AXIS_ALIGNED_COS`] (about 8°) admits.
pub fn flat_along(part: &Part, f: usize, h: usize) -> bool {
    let b = part.face_bounds(f);
    b.max[h] - b.min[h] <= TOL
}
}

internal_entry! {
/// Whether level face *f* (a plane square to frame axis *h*, in *scope*) is one wall of a slot or
/// pocket rather than a shelf: another plane of the same body, flat along *h* ([`flat_along`]),
/// faces it (its outward normal anti-parallel to *f*'s within [`AXIS_ALIGNED_COS`]) from *f*'s
/// outward side, more than [`TOL`] and less than the body's extent along *h* away; the overlap of
/// the two faces' boxes across *h* covers at least half of each box, and the gap is shorter than
/// the overlap's longer side (a channel narrower than its walls are long); and nine points evenly
/// spaced between the two planes, on the line through the centre of that overlap, all classify
/// as outside the body (space a proof finds empty, not merely unknown).
pub fn opposed_wall(ctx: &Context<'_>, solid: Option<usize>, faces: &[usize], f: usize, h: usize) -> bool {
    let part = ctx.part;
    let [u, v, _] = plane_axes(h);
    let (Surface::Plane { frame }, Some(n)) = (&part.faces[f].surface, part.face_normal(f, 0.0, 0.0))
    else {
        return false;
    };
    let extent = |b: &Bounds| b.max[h] - b.min[h];
    let reach = match solid {
        Some(s) => extent(&part.solid_bounds(s)),
        None => extent(&part.bounds()),
    };
    let fb = part.face_bounds(f);
    let sign = n[h].signum();
    faces.iter().any(|&g| {
        if g == f || !flat_along(part, g, h) {
            return false;
        }
        let (Surface::Plane { frame: other }, Some(m)) =
            (&part.faces[g].surface, part.face_normal(g, 0.0, 0.0))
        else {
            return false;
        };
        let gap = (other.origin[h] - frame.origin[h]) * sign;
        if crate::kernel::geom::dot(n, m) >= -AXIS_ALIGNED_COS || gap <= TOL || gap >= reach {
            return false;
        }
        let gb = part.face_bounds(g);
        let overlap = |k: usize| (fb.min[k].max(gb.min[k]), fb.max[k].min(gb.max[k]));
        let (ou, ov) = (overlap(u), overlap(v));
        let (wu, wv) = (ou.1 - ou.0, ov.1 - ov.0);
        if wu <= TOL || wv <= TOL {
            return false;
        }
        // A matched pair of walls, each covered at least half by the other across the axis,
        // nearer each other than the pair is long: a channel, not two shelves facing.
        let area = |b: &Bounds| (b.max[u] - b.min[u]) * (b.max[v] - b.min[v]);
        if wu * wv < 0.5 * area(&fb) || wu * wv < 0.5 * area(&gb) || gap >= wu.max(wv) {
            return false;
        }
        let classifier = match solid {
            Some(s) => ctx.solid_classifier(s),
            None => ctx.classifier(),
        };
        (1..10).all(|k| {
            let mut p = [0.0; 3];
            p[u] = 0.5 * (ou.0 + ou.1);
            p[v] = 0.5 * (ov.0 + ov.1);
            p[h] = frame.origin[h] + (other.origin[h] - frame.origin[h]) * f64::from(k) / 10.0;
            classifier.classify(p) == crate::kernel::classify::State::Out
        })
    })
}
}
