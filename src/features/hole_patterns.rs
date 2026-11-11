//! Hole patterns (`quiddity.holes.recognise_hole_patterns`): bolt circles, linear arrays,
//! rectangular grids and four-corner rectangles among holes of one machining spec.
//!
//! Pure arithmetic over [`HoleRecord`]s. Holes are grouped by spec and drilling axis; circles,
//! grids and rectangles are found per shared opening plane, linear arrays in world space. All
//! candidates are allocated greedily, largest first, so each hole joins at most one pattern.

use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};

use super::holes::{Bottom, CounterBore, HoleRecord};
use super::pattern_geometry::{
    Candidate, Located, PATTERN_ABS_TOL, linear_array_candidates, mean_location, pattern_tol,
    plane_uv, rect_grid,
};
use super::policy;
use crate::kernel::geom::{self, V3};
use crate::kernel::py;

/// Bolt-circle gaps may differ from even spacing by this fraction of it.
const BC_SPACING_FRAC: f64 = 0.04;
/// Opening points share a plane within this fraction of the group's span (the axis snap's
/// bound), plus the coordinate-noise floor.
const OPENING_PLANE_REL_TOL: f64 = 2e-6;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HolePattern {
    RectGrid {
        holes: Vec<HoleRecord>,
        rows: usize,
        cols: usize,
        row_pitch: f64,
        col_pitch: f64,
        angle: f64,
        center: V3,
    },
    RectangularHoleSet {
        holes: Vec<HoleRecord>,
        center: V3,
        width: f64,
        height: f64,
        angle: f64,
    },
    BoltCircle {
        holes: Vec<HoleRecord>,
        center: V3,
        diameter: f64,
    },
    LinearArray {
        holes: Vec<HoleRecord>,
        pitch: f64,
        direction: V3,
    },
}

impl Located for HoleRecord {
    fn location(&self) -> V3 {
        *self.location
    }
}

fn hole_grid(
    holes: &[&HoleRecord],
    rows: usize,
    cols: usize,
    row_pitch: f64,
    col_pitch: f64,
    angle: f64,
    center: V3,
) -> HolePattern {
    HolePattern::RectGrid {
        holes: owned(holes),
        rows,
        cols,
        row_pitch,
        col_pitch,
        angle,
        center,
    }
}

fn hole_linear(holes: Vec<&HoleRecord>, pitch: f64, direction: V3) -> HolePattern {
    HolePattern::LinearArray {
        holes: owned(&holes),
        pitch,
        direction,
    }
}

fn hypot2(a: (f64, f64), b: (f64, f64)) -> f64 {
    py::hypot(&[b.0 - a.0, b.1 - a.1])
}

/// The machining spec holes of one drilled feature share (`HoleSpec.from_hole`): compared with
/// `==`, as a frozen dataclass is in a dict.
#[derive(Clone, Debug, PartialEq)]
struct HoleSpec {
    axis: V3,
    diameter: f64,
    depth: Option<f64>,
    bottom: Bottom,
    cbore: Option<CounterBore>,
    spotface: Option<CounterBore>,
    outer_steps: Vec<CounterBore>,
    csink: Option<(f64, f64)>,
}

impl HoleSpec {
    fn of(h: &HoleRecord) -> Self {
        HoleSpec {
            axis: h.axis.map(|c| {
                if c.abs() < 1e-6 {
                    0.0
                } else {
                    py::round_to(c, 6)
                }
            }),
            diameter: h.diameter,
            depth: (h.bottom != Bottom::Through).then_some(h.depth),
            bottom: h.bottom,
            cbore: h.cbore.clone(),
            spotface: h.spotface.clone(),
            outer_steps: h.outer_steps.clone(),
            csink: h
                .csink
                .as_ref()
                .map(|c| (c.major_diameter, c.included_angle)),
        }
    }
}

/// Holes whose openings share one plane perpendicular to *axis* (`_opening_plane_clusters`).
fn opening_plane_clusters(members: &[&HoleRecord], axis: V3) -> Vec<Vec<usize>> {
    let direction = py::unit(axis).expect("unit axis");
    let origin = *members[0].location;
    let offsets: Vec<f64> = members
        .iter()
        .map(|m| py::dot(&geom::sub(*m.location, origin), &direction))
        .collect();
    let scale = members
        .iter()
        .map(|m| py::dist(&origin, &m.location[..]))
        .fold(members[0].diameter, f64::max);
    policy::cluster_coordinates(&offsets, policy::length_tol(scale, OPENING_PLANE_REL_TOL))
}

fn owned(holes: &[&HoleRecord]) -> Vec<HoleRecord> {
    holes.iter().map(|h| (*h).clone()).collect()
}

/// A bolt circle when the 2D points are equally spaced on one circle (`_as_bolt_circle`).
fn as_bolt_circle(holes: &[&HoleRecord], pts: &[(f64, f64)]) -> Option<HolePattern> {
    let n = pts.len() as f64;
    let c = (
        py::sum(pts.iter().map(|p| p.0)) / n,
        py::sum(pts.iter().map(|p| p.1)) / n,
    );
    let radii: Vec<f64> = pts.iter().map(|p| hypot2(c, *p)).collect();
    let r = py::sum(radii.iter().copied()) / n;
    if r < PATTERN_ABS_TOL || radii.iter().any(|ri| (ri - r).abs() > pattern_tol(r)) {
        return None;
    }
    let mut angles: Vec<f64> = pts.iter().map(|p| (p.1 - c.1).atan2(p.0 - c.0)).collect();
    angles.sort_by(|a, b| py::order(*a, *b));
    let mut gaps: Vec<f64> = angles.windows(2).map(|w| w[1] - w[0]).collect();
    gaps.push(TAU - (angles[angles.len() - 1] - angles[0]));
    let even = TAU / n;
    if gaps
        .iter()
        .any(|g| (g - even).abs() > BC_SPACING_FRAC * even)
    {
        return None;
    }
    Some(HolePattern::BoltCircle {
        holes: owned(holes),
        center: mean_location(holes),
        diameter: py::round_to(2.0 * r, 2),
    })
}

/// The circle through three 2D points, `None` when they are collinear (`_circumcircle`).
fn circumcircle(p0: (f64, f64), p1: (f64, f64), p2: (f64, f64)) -> Option<(f64, f64, f64)> {
    let ((ax, ay), (bx, by), (cx, cy)) = (p0, p1, p2);
    let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
    if d.abs() < 1e-9 {
        return None;
    }
    let (a2, b2, c2) = (ax * ax + ay * ay, bx * bx + by * by, cx * cx + cy * cy);
    let ux = (a2 * (by - cy) + b2 * (cy - ay) + c2 * (ay - by)) / d;
    let uy = (a2 * (cx - bx) + b2 * (ax - cx) + c2 * (bx - ax)) / d;
    Some((ux, uy, py::hypot(&[ax - ux, ay - uy])))
}

/// Every bolt circle in a group: each triple seeds a circle, the points on it are gathered and
/// kept if evenly populated (`_bolt_circle_candidates`). Indices are local to *pts*.
fn bolt_circle_candidates(
    members: &[&HoleRecord],
    pts: &[(f64, f64)],
) -> Vec<Candidate<HolePattern>> {
    let n = pts.len();
    let (mut out, mut seen) = (Vec::new(), Vec::<[f64; 3]>::new());
    for i in 0..n {
        for j in i + 1..n {
            for k in j + 1..n {
                let Some((cx, cy, r)) = circumcircle(pts[i], pts[j], pts[k]) else {
                    continue;
                };
                if r < PATTERN_ABS_TOL {
                    continue;
                }
                let key = [py::round_to(cx, 2), py::round_to(cy, 2), py::round_to(r, 2)];
                if seen.contains(&key) {
                    continue;
                }
                seen.push(key);
                let tol = pattern_tol(r);
                let idx: Vec<usize> = (0..n)
                    .filter(|&m| (hypot2((cx, cy), pts[m]) - r).abs() <= tol)
                    .collect();
                if idx.len() < 3 {
                    continue;
                }
                let sub: Vec<&HoleRecord> = idx.iter().map(|&m| members[m]).collect();
                let sub_pts: Vec<(f64, f64)> = idx.iter().map(|&m| pts[m]).collect();
                if let Some(p) = as_bolt_circle(&sub, &sub_pts) {
                    out.push((p, idx));
                }
            }
        }
    }
    out
}

/// Four holes at the corners of one proved rectangle (`_rectangular_hole_set`).
fn rectangular_hole_set(holes: &[&HoleRecord], pts: &[(f64, f64)]) -> Option<HolePattern> {
    if holes.len() != 4 || pts.len() != 4 {
        return None;
    }
    let span = pts
        .iter()
        .flat_map(|a| pts.iter().map(move |b| hypot2(*a, *b)))
        .fold(f64::NEG_INFINITY, f64::max);
    if span <= PATTERN_ABS_TOL {
        return None;
    }
    let tol = pattern_tol(span);
    let mid = |a: usize, b: usize| ((pts[a].0 + pts[b].0) / 2.0, (pts[a].1 + pts[b].1) / 2.0);
    [((0, 1), (2, 3)), ((0, 2), (1, 3)), ((0, 3), (1, 2))]
        .into_iter()
        .find(|&(f, s)| {
            hypot2(mid(f.0, f.1), mid(s.0, s.1)) <= tol
                && (hypot2(pts[f.0], pts[f.1]) - hypot2(pts[s.0], pts[s.1])).abs() <= tol
        })?;
    let c = (
        py::sum(pts.iter().map(|p| p.0)) / 4.0,
        py::sum(pts.iter().map(|p| p.1)) / 4.0,
    );
    let mut order: Vec<usize> = (0..4).collect();
    order.sort_by(|&a, &b| {
        let angle = |i: usize| (pts[i].1 - c.1).atan2(pts[i].0 - c.0);
        py::order(angle(a), angle(b))
    });
    let vectors: Vec<(f64, f64)> = (0..4)
        .map(|i| {
            (
                pts[order[(i + 1) % 4]].0 - pts[order[i]].0,
                pts[order[(i + 1) % 4]].1 - pts[order[i]].1,
            )
        })
        .collect();
    let lengths: Vec<f64> = vectors.iter().map(|v| py::hypot(&[v.0, v.1])).collect();
    let shortest = lengths.iter().copied().fold(f64::INFINITY, f64::min);
    if shortest <= tol
        || (lengths[0] - lengths[2]).abs() > tol
        || (lengths[1] - lengths[3]).abs() > tol
    {
        return None;
    }
    if (vectors[0].0 * vectors[1].0 + vectors[0].1 * vectors[1].1).abs() > tol * span {
        return None;
    }
    let long = if lengths[0] >= lengths[1] { 0 } else { 1 };
    let (width, height) = (lengths[long], lengths[1 - long]);
    let mut angle = py::modulo(vectors[long].1.atan2(vectors[long].0).to_degrees(), 180.0);
    if (width - height).abs() <= tol {
        angle = py::modulo(angle, 90.0);
    }
    let ordered: Vec<&HoleRecord> = order.iter().map(|&i| holes[i]).collect();
    Some(HolePattern::RectangularHoleSet {
        holes: owned(&ordered),
        center: mean_location(holes),
        width: py::round_to(width, 2),
        height: py::round_to(height, 2),
        angle: py::round_to(angle, 2),
    })
}

internal_entry! {
/// `recognise_hole_patterns`.
pub fn recognise_hole_patterns(holes: &[HoleRecord]) -> Vec<HolePattern> {
    let mut groups: Vec<(HoleSpec, Vec<&HoleRecord>)> = Vec::new();
    for h in holes {
        let spec = HoleSpec::of(h);
        match groups.iter_mut().find(|(s, _)| *s == spec) {
            Some((_, members)) => members.push(h),
            None => groups.push((spec, vec![h])),
        }
    }
    let mut patterns = Vec::new();
    for (spec, members) in groups {
        if members.len() < 3 {
            continue;
        }
        let (u, v) = plane_uv(spec.axis);
        let pts: Vec<(f64, f64)> = members
            .iter()
            .map(|h| {
                (
                    py::sum((0..3).map(|i| h.location[i] * u[i])),
                    py::sum((0..3).map(|i| h.location[i] * v[i])),
                )
            })
            .collect();
        let planes = opening_plane_clusters(&members, spec.axis);
        if planes.len() == 1
            && let Some(grid) = rect_grid(&members, &pts, hole_grid)
        {
            patterns.push(grid);
            continue;
        }
        let mut candidates: Vec<Candidate<HolePattern>> = Vec::new();
        for indices in &planes {
            if indices.len() < 3 {
                continue;
            }
            let plane_members: Vec<&HoleRecord> = indices.iter().map(|&i| members[i]).collect();
            let plane_points: Vec<(f64, f64)> = indices.iter().map(|&i| pts[i]).collect();
            if let Some(rectangle) = rectangular_hole_set(&plane_members, &plane_points) {
                candidates.push((rectangle, indices.clone()));
            }
            if let Some(grid) = rect_grid(&plane_members, &plane_points, hole_grid) {
                candidates.push((grid, indices.clone()));
            }
            for (pattern, local) in bolt_circle_candidates(&plane_members, &plane_points) {
                candidates.push((pattern, local.iter().map(|&l| indices[l]).collect()));
            }
        }
        let locations: Vec<V3> = members.iter().map(|m| *m.location).collect();
        candidates.extend(linear_array_candidates(&members, &locations, hole_linear));
        patterns.extend(super::pattern_geometry::allocate_candidates(
            candidates, &locations,
        ));
    }
    patterns
}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clusters_are_bounded_not_chained() {
        let c = policy::cluster_coordinates(&[0.0, 0.4, 0.8, 1.2], 0.5);
        assert_eq!(c, vec![vec![0, 1], vec![2, 3]]);
    }
}
