//! Gusset ribs (`quiddity.gussets`): right-triangular material ribs joining two perpendicular
//! axis-aligned support planes, and the constant-pitch arrays and centred mirror pairs they form.
//!
//! A rib is two parallel triangular end caps, each with its legs on the two supports (concave
//! corners) and its hypotenuse on one shared slant face, directly or through a smooth blend. The
//! material between the caps is proved by probing the smaller cap's prism against the solid.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use super::Context;
use super::body::BodyKey;
use super::evidence::{self, EvidenceError, Occurrence, common_valid_solid};
use super::graph::{face_vertices_along_loops, is_planar};
use super::pattern_geometry::pattern_tol;
use super::placed::{Grid, Placed};
use super::planes::axis_aligned_axis;
use super::policy::length_tol;
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::{self, Curve, Surface, V3};
use crate::kernel::py;
use crate::kernel::volume::{Prism, PrismEdge, Probe, common_volume, probe_volume};

const AXES: [char; 3] = ['x', 'y', 'z'];

/// A right-triangular rib between two perpendicular support planes (`GussetRib`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct GussetRib {
    pub thickness_axis: char,
    pub thickness_bounds: Placed<(f64, f64)>,
    pub supports: ((char, Placed<f64>), (char, Placed<f64>)),
    pub legs: (f64, f64),
    pub directions: (i32, i32),
    /// Equal signatures on separate solids are ambiguous, and give `None`.
    pub body_key: Option<BodyKey>,
}

/// Congruent ribs of one body: a constant-pitch array (`GussetRibArray`) or a pair reflected
/// across the body's centre plane (`GussetRibMirrorPair`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum GussetRibPattern {
    Array {
        ribs: Vec<GussetRib>,
        axis: char,
        pitch: f64,
    },
    MirrorPair {
        ribs: Vec<GussetRib>,
        mirror_plane: (char, f64),
    },
}

impl GussetRibPattern {
    pub fn ribs(&self) -> &[GussetRib] {
        match self {
            GussetRibPattern::Array { ribs, .. } | GussetRibPattern::MirrorPair { ribs, .. } => {
                ribs
            }
        }
    }
}

/// A triangular end cap (`_Cap`).
struct Cap {
    face: usize,
    axis: usize,
    at: f64,
    /// The corners in loop order, for the material probe.
    corners: Vec<V3>,
    /// The corners on the other two axes, sorted.
    profile: Vec<[f64; 2]>,
    supports: (usize, usize),
    support_coords: [f64; 2],
    slant: usize,
    transition: usize,
}

fn others(axis: usize) -> [usize; 2] {
    match axis {
        0 => [1, 2],
        1 => [0, 2],
        _ => [0, 1],
    }
}

fn cap(part: &Part, face: usize) -> Option<Cap> {
    if !is_planar(part, face) {
        return None;
    }
    let (axis, _) = axis_aligned_axis(part, face)?;
    let edges = part.face_edges(face);
    if edges.len() != 3
        || edges
            .iter()
            .any(|&e| !matches!(part.edges[e].curve, Curve::Line { .. }))
    {
        return None;
    }
    let vertices = face_vertices_along_loops(part, face);
    if vertices.len() != 3 {
        return None;
    }
    let axes = others(axis);
    let shortest = edges
        .iter()
        .map(|&e| geom::dist(part.edges[e].start, part.edges[e].end))
        .fold(f64::INFINITY, f64::min);
    let tol = length_tol(shortest, 1e-7);
    let levels = vertices.iter().map(|v| v[axis]);
    let spread =
        levels.clone().fold(f64::NEG_INFINITY, f64::max) - levels.fold(f64::INFINITY, f64::min);
    if spread > tol {
        return None;
    }
    let profile: Vec<[f64; 2]> = vertices.iter().map(|v| [v[axes[0]], v[axes[1]]]).collect();
    let square = *profile.iter().find(|p| {
        (0..2).all(|k| {
            profile
                .iter()
                .any(|o| (p[k] - o[k]).abs() <= tol && o != *p)
        })
    })?;
    let edge_faces = part.edge_faces();
    let mut supports: [Option<usize>; 3] = [None; 3];
    let mut transition = None;
    for &e in &edges {
        let incident = &edge_faces[e];
        if incident.len() != 2 {
            return None;
        }
        let other = *incident.iter().find(|&&f| f != face)?;
        let edge = &part.edges[e];
        let (p, q) = (edge.start, edge.end);
        let Some(fixed) = axes.iter().position(|&i| (p[i] - q[i]).abs() <= tol) else {
            if transition.is_some() {
                return None;
            }
            transition = Some(other);
            continue;
        };
        let coord = square[fixed];
        let plane = axis_aligned_axis(part, other);
        if supports[axes[fixed]].is_some()
            || (p[axes[fixed]] - coord).abs() > tol
            || plane.is_none_or(|(ax, at)| ax != axes[fixed] || (at - coord).abs() > tol)
            || part.arc(face, other) != Some(Arc::Concave)
        {
            return None;
        }
        supports[axes[fixed]] = Some(other);
    }
    let transition = transition?;
    let (Some(first), Some(second)) = (supports[axes[0]], supports[axes[1]]) else {
        return None;
    };
    let arc = part.arc(face, transition);
    let slant = match part.faces[transition].surface {
        Surface::Plane { .. } if arc == Some(Arc::Convex) => transition,
        Surface::Cylinder { .. } if arc == Some(Arc::Smooth) => {
            let candidates: Vec<usize> = part
                .neighbours(transition)
                .into_iter()
                .filter(|&other| {
                    other != face
                        && is_planar(part, other)
                        && axis_aligned_axis(part, other).is_none()
                        && part.arc(transition, other) == Some(Arc::Smooth)
                })
                .collect();
            let [only] = candidates[..] else {
                return None;
            };
            only
        }
        _ => return None,
    };
    let mut sorted = profile.clone();
    sorted.sort_by(|a, b| py::tuple_order(a, b));
    Some(Cap {
        face,
        axis,
        at: vertices.iter().fold(0.0, |s, v| s + v[axis]) / 3.0,
        corners: vertices,
        profile: sorted,
        supports: (first, second),
        support_coords: square,
        slant,
        transition,
    })
}

internal_entry! {
/// `recognise_gusset_ribs`.
pub fn recognise_gusset_ribs(part: &Part) -> Vec<GussetRib> {
    super::records(discover(&Context::new(part)))
}
}

internal_entry! {
/// The evidence path: each rib with its end caps, slant and edge blends; the supports are
/// consulted.
pub fn discover_verified(ctx: &Context<'_>) -> Result<Vec<Occurrence<GussetRib>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx))
}
}

/// Python's ordering of `GussetRib` (a frozen, ordered dataclass): field by field.
fn rib_order(a: &GussetRib, b: &GussetRib) -> Ordering {
    let pair = |x: (f64, f64)| [x.0, x.1];
    a.thickness_axis
        .cmp(&b.thickness_axis)
        .then_with(|| py::tuple_order(&pair(*a.thickness_bounds), &pair(*b.thickness_bounds)))
        .then_with(|| {
            let (s, t) = (a.supports, b.supports);
            s.0.0
                .cmp(&t.0.0)
                .then_with(|| py::order(*s.0.1, *t.0.1))
                .then_with(|| s.1.0.cmp(&t.1.0))
                .then_with(|| py::order(*s.1.1, *t.1.1))
        })
        .then_with(|| py::tuple_order(&pair(a.legs), &pair(b.legs)))
        .then_with(|| a.directions.cmp(&b.directions))
        .then_with(|| match (&a.body_key, &b.body_key) {
            (Some(x), Some(y)) => py::tuple_order(x, y),
            (x, y) => x.is_some().cmp(&y.is_some()),
        })
}

internal_entry! {
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<GussetRib>> {
    let part = ctx.part;
    let caps: Vec<Cap> = (0..part.faces.len())
        .filter_map(|face| cap(part, face))
        .collect();
    let mut links: Vec<Vec<usize>> = vec![Vec::new(); caps.len()];
    for (i, left) in caps.iter().enumerate() {
        for (j, right) in caps.iter().enumerate().skip(i + 1) {
            if left.axis != right.axis || left.slant != right.slant {
                continue;
            }
            let gap = (right.at - left.at).abs();
            let tol = length_tol(gap, 1e-7);
            if gap <= tol || left.supports != right.supports {
                continue;
            }
            let matching = left
                .profile
                .iter()
                .zip(&right.profile)
                .all(|(a, b)| (0..2).all(|k| (a[k] - b[k]).abs() <= tol));
            let rounded = left.transition != left.slant || right.transition != right.slant;
            if matching || rounded {
                links[i].push(j);
                links[j].push(i);
            }
        }
    }
    let mut found: Vec<(Occurrence<GussetRib>, usize)> = Vec::new();
    for (i, left) in caps.iter().enumerate() {
        let [j] = links[i][..] else {
            continue;
        };
        if j < i || links[j].len() != 1 {
            continue;
        }
        let right = &caps[j];
        let mut defining: Vec<usize> = Vec::new();
        for f in [
            left.face,
            right.face,
            left.slant,
            left.transition,
            right.transition,
        ] {
            if !defining.contains(&f) {
                defining.push(f);
            }
        }
        let supports = vec![left.supports.0, left.supports.1];
        let consulted: Vec<usize> = defining.iter().chain(&supports).copied().collect();
        let Some(solid) = common_valid_solid(part, &consulted) else {
            continue;
        };
        let (low, high) = if right.at < left.at {
            (right, left)
        } else {
            (left, right)
        };
        let thickness = high.at - low.at;
        // One-sided fillets leave unequal end caps. Their smaller triangle must still be filled
        // all the way to the opposite cap; the removed blend lies outside that probe.
        let area = |c: &Cap| part.face_mass(c.face).map(|m| m[0]);
        // Without both caps' areas the smaller cannot be chosen: refuse rather than guess.
        let (Some(high_area), Some(low_area)) = (area(high), area(low)) else {
            continue;
        };
        let probe_cap = if high_area < low_area { high } else { low };
        // The cap's triangle swept along the rib's axis from one cap to the other.
        let corners = &probe_cap.corners;
        let sides = (0..corners.len())
            .map(|i| PrismEdge {
                points: vec![corners[i], corners[(i + 1) % corners.len()]],
                straight: true,
            })
            .collect();
        let probe = Probe::Prism(Prism {
            axis: left.axis,
            lo: low.at,
            hi: high.at,
            loops: vec![sides],
        });
        let Some(whole) = probe_volume(&probe).filter(|&w| w > 0.0) else {
            continue;
        };
        // The claim is that the triangle is filled; an unanswered probe does not show that.
        let Some(filled) = common_volume(ctx.solid_classifier(solid), &probe) else {
            continue;
        };
        let residual = whole - filled;
        if residual > whole / thickness * length_tol(thickness, 1e-7) {
            continue;
        }
        let axes = others(left.axis);
        let bounds = part.face_bounds(left.slant);
        let span = |ax: usize| [bounds.min[ax], bounds.max[ax]];
        let legs = axes.map(|ax| span(ax)[1] - span(ax)[0]);
        let tol = length_tol(legs[0].min(legs[1]), 1e-7);
        if legs.iter().any(|&leg| leg <= tol) {
            continue;
        }
        let near = |k: usize| {
            span(axes[k])
                .iter()
                .map(|side| (left.support_coords[k] - side).abs())
                .fold(f64::INFINITY, f64::min)
                <= tol
        };
        if !near(0) || !near(1) {
            continue;
        }
        let direction = |k: usize| {
            if (left.support_coords[k] - span(axes[k])[0]).abs() <= tol {
                1
            } else {
                -1
            }
        };
        let record = GussetRib {
            thickness_axis: AXES[left.axis],
            thickness_bounds: Placed::new((low.at, high.at), Grid::Decimals(3)),
            supports: (
                (
                    AXES[axes[0]],
                    Placed::new(left.support_coords[0], Grid::Decimals(3)),
                ),
                (
                    AXES[axes[1]],
                    Placed::new(left.support_coords[1], Grid::Decimals(3)),
                ),
            ),
            legs: (py::round_to3(legs[0]), py::round_to3(legs[1])),
            directions: (direction(0), direction(1)),
            body_key: None,
        };
        found.push((
            Occurrence {
                record,
                defining,
                context: supports,
            },
            solid,
        ));
    }
    if !found.is_empty() {
        let keys = ctx.body_keys(true);
        for (o, solid) in &mut found {
            o.record.body_key = keys[*solid].clone();
        }
    }
    let mut out: Vec<Occurrence<GussetRib>> = found.into_iter().map(|(o, _)| o).collect();
    out.sort_by(|a, b| rib_order(&a.record, &b.record));
    out
}
}

internal_entry! {
/// `recognise_gusset_rib_patterns`: same-body ribs grouped into constant-pitch arrays or
/// centred mirror pairs.
pub fn recognise_gusset_rib_patterns(ribs: &[GussetRib]) -> Vec<GussetRibPattern> {
    type Spec<'a> = (
        &'a BodyKey,
        char,
        ((char, Placed<f64>), (char, Placed<f64>)),
        (f64, f64),
        (i32, i32),
        f64,
    );
    let mut groups: Vec<(Spec<'_>, Vec<&GussetRib>)> = Vec::new();
    for rib in ribs {
        let Some(key) = rib.body_key.as_ref().filter(|k| !k.is_empty()) else {
            continue;
        };
        let (lo, hi) = *rib.thickness_bounds;
        let spec = (
            key,
            rib.thickness_axis,
            rib.supports,
            rib.legs,
            rib.directions,
            py::round_to3(hi - lo),
        );
        match groups.iter_mut().find(|(s, _)| *s == spec) {
            Some((_, members)) => members.push(rib),
            None => groups.push((spec, vec![rib])),
        }
    }

    let bounds_order = |a: &GussetRib, b: &GussetRib| {
        let pair = |x: Placed<(f64, f64)>| [x.0, x.1];
        py::tuple_order(&pair(a.thickness_bounds), &pair(b.thickness_bounds))
    };
    let mut patterns = Vec::new();
    for (spec, mut members) in groups {
        members.sort_by(|a, b| bounds_order(a, b));
        if members.len() < 2 {
            continue;
        }
        let (body_key, axis) = (spec.0, spec.1);
        let width = members[0].thickness_bounds.1 - members[0].thickness_bounds.0;
        let k = AXES.iter().position(|&c| c == axis).unwrap();
        let body_mid = (body_key[k] + body_key[k + 3]) / 2.0;
        let centres: Vec<f64> = members
            .iter()
            .map(|r| r.thickness_bounds.0 + r.thickness_bounds.1)
            .map(|s| s / 2.0)
            .collect();
        let mut used = vec![false; members.len()];

        let mut start = 0;
        while start + 2 < members.len() {
            let pitch = centres[start + 1] - centres[start];
            let tol = if pitch > 0.0 { pattern_tol(pitch) } else { 0.0 };
            if pitch <= width + tol {
                start += 1;
                continue;
            }
            let mut end = start + 2;
            while end < members.len() && (centres[end] - centres[end - 1] - pitch).abs() <= tol {
                end += 1;
            }
            if end - start >= 3 {
                patterns.push(GussetRibPattern::Array {
                    ribs: members[start..end].iter().map(|r| (*r).clone()).collect(),
                    axis,
                    pitch: py::round_to3(
                        (centres[end - 1] - centres[start]) / (end - start - 1) as f64,
                    ),
                });
                used[start..end].fill(true);
                start = end;
            } else {
                start += 1;
            }
        }

        for index in 0..members.len() {
            if used[index] || centres[index] >= body_mid {
                continue;
            }
            let tol = pattern_tol((centres[index] - body_mid).abs().max(width));
            let matches: Vec<usize> = (index + 1..members.len())
                .filter(|&other| {
                    !used[other]
                        && centres[other] > body_mid
                        && (centres[index] + centres[other] - 2.0 * body_mid).abs() <= tol
                        && members[other].thickness_bounds.0 - members[index].thickness_bounds.1
                            > tol
                })
                .collect();
            if let [other] = matches[..] {
                patterns.push(GussetRibPattern::MirrorPair {
                    ribs: vec![members[index].clone(), members[other].clone()],
                    mirror_plane: (axis, py::round_to3(body_mid)),
                });
                used[index] = true;
                used[other] = true;
            }
        }
    }
    patterns.sort_by(|a, b| bounds_order(&a.ribs()[0], &b.ribs()[0]));
    patterns
}
}
