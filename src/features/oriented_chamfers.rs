//! Oriented-chamfer recognition (`quiddity.oriented_chamfers`): an external planar bevel on a
//! straight convex edge oblique to the principal frame. Its two planar supports meet at a
//! virtual edge; the legs are measured along the supports.

use serde::{Deserialize, Serialize};

use super::Context;
use super::body::BodyKey;
use super::evidence::{self, EvidenceError, Occurrence};
use super::placed::{Grid, Placed};
use super::planes::{coordinates, linear_quad, normalized, unit_plane_normal};
use super::policy::{AXIS_ALIGNED_COS, INTERIOR_PROBE_FRAC};
use crate::kernel::brep::{Arc, Part};
use crate::kernel::classify::{Classifier, material_at};
use crate::kernel::geom::{
    Bounds, COORD_FLOOR, Curve, SMOOTH_ARC_GAP, Surface, V3, add, dot, norm, scale, sub,
};
use crate::kernel::py;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct OrientedChamfer {
    pub run: V3,
    pub length: f64,
    pub at: Placed<V3>,
    pub corner: Placed<V3>,
    pub leg1: f64,
    pub leg2: f64,
    pub leg1_direction: V3,
    pub leg2_direction: V3,
    pub support_spans: [[f64; 2]; 2],
    pub angle: f64,
    pub body_key: Option<BodyKey>,
}

impl OrientedChamfer {
    /// The fields in the order Python's `__lt__` compares them.
    fn sort_key(&self) -> Vec<f64> {
        let mut key: Vec<f64> = self.run.to_vec();
        key.push(self.length);
        key.extend(*self.at);
        key.extend(*self.corner);
        key.extend([self.leg1, self.leg2]);
        key.extend(self.leg1_direction);
        key.extend(self.leg2_direction);
        key.extend(self.support_spans.iter().flatten());
        key.push(self.angle);
        key.push(if self.body_key.is_some() { 1.0 } else { 0.0 });
        key.extend(self.body_key.iter().flat_map(|k| k.iter()));
        key
    }
}

/// Options named as `recognise_oriented_chamfers`' keyword arguments.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct OrientedChamferOptions {
    /// The largest leg, as a fraction of the owning solid's largest box extent.
    pub max_leg_frac: f64,
}

impl Default for OrientedChamferOptions {
    fn default() -> Self {
        OrientedChamferOptions { max_leg_frac: 0.45 }
    }
}

internal_entry! {
/// `recognise_oriented_chamfers`.
pub fn recognise_oriented_chamfers(
    part: &Part,
    opts: &OrientedChamferOptions,
) -> Vec<OrientedChamfer> {
    super::records(discover(&Context::new(part), opts))
}
}

internal_entry! {
/// The evidence path: each chamfer with its bevel face.
pub fn discover_verified(
    ctx: &Context<'_>,
    opts: &OrientedChamferOptions,
) -> Result<Vec<Occurrence<OrientedChamfer>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx, opts))
}
}

/// The one straight edge a convex planar neighbour shares with the bevel, and the neighbour's
/// normal (`_edge_info`).
fn edge_info(part: &Part, bevel: usize, neighbour: usize) -> Option<(usize, V3)> {
    if part.arc(bevel, neighbour) != Some(Arc::Convex) {
        return None;
    }
    let Surface::Plane { .. } = part.faces[neighbour].surface else {
        return None;
    };
    let edges = part.shared_edges(bevel, neighbour);
    let [edge] = edges[..] else { return None };
    let Curve::Line { .. } = part.edges[edge].curve else {
        return None;
    };
    Some((edge, unit_plane_normal(part, neighbour)?))
}

/// The run the probes and limits of one solid share.
struct Body<'c, 'a> {
    classifier: &'c Classifier<'a>,
    stock_size: f64,
    bounds: Bounds,
    max_leg_frac: f64,
}

/// The record for a bevel between two support faces, or `None` (`_pair`).
fn pair(
    part: &Part,
    body: &Body<'_, '_>,
    bevel: usize,
    left: usize,
    right: usize,
) -> Option<OrientedChamfer> {
    let (edge1, normal1) = edge_info(part, bevel, left)?;
    let (edge2, normal2) = edge_info(part, bevel, right)?;
    if dot(normal1, normal2).abs() > SMOOTH_ARC_GAP {
        return None;
    }
    let ends = |e: usize| {
        let mut p = part.edges[e].vertex_points();
        p.sort_by(|a, b| py::tuple_order(a, b));
        p
    };
    let (ends1, ends2) = (ends(edge1), ends(edge2));
    if ends1.len() != 2 || ends2.len() != 2 {
        return None;
    }
    let (span1, span2) = (sub(ends1[1], ends1[0]), sub(ends2[1], ends2[0]));
    if norm(span1).min(norm(span2)) <= COORD_FLOOR {
        return None;
    }
    let run = normalized(span1)?;
    let parallel = dot(run, normalized(span2)?);
    // Lexicographic sorts can oppose parallel runs after round-off under a rigid motion.
    // All endpoint uses below are order-symmetric, so only the direction line matters.
    if parallel.abs() < 1.0 - SMOOTH_ARC_GAP {
        return None;
    }
    if run.iter().map(|c| c.abs()).fold(0.0, f64::max) >= AXIS_ALIGNED_COS {
        return None;
    }
    if dot(normal1, run).abs() > SMOOTH_ARC_GAP || dot(normal2, run).abs() > SMOOTH_ARC_GAP {
        return None;
    }
    // The two support edges must be opposite, not two sides of one triangular end.
    let closest = ends1
        .iter()
        .flat_map(|a| ends2.iter().map(move |b| norm(sub(*a, *b))))
        .fold(f64::INFINITY, f64::min);
    if closest <= COORD_FLOOR {
        return None;
    }
    let origin = ends1[0];
    let first_span = [0.0, norm(span1)];
    let mut second_span = [
        dot(sub(ends2[0], origin), run),
        dot(sub(ends2[1], origin), run),
    ];
    second_span.sort_by(|a, b| py::order(*a, *b));
    let (common_start, common_end) = (
        first_span[0].max(second_span[0]),
        first_span[1].min(second_span[1]),
    );
    let run_length = common_end - common_start;
    if run_length <= COORD_FLOOR {
        return None;
    }
    let station = (common_start + common_end) * 0.5;
    let surface_normal = unit_plane_normal(part, bevel)?;
    if dot(surface_normal, normal1).min(dot(surface_normal, normal2)) <= SMOOTH_ARC_GAP {
        // A replacement bevel faces out between its two support normals; an ordinary side
        // wall meeting a top and an end face is not such a bevel.
        return None;
    }
    let at = part.face_centre(bevel)?;
    let cross = add(at, scale(run, station - dot(sub(at, origin), run)));
    let mid = |e: &[V3]| {
        let m = scale(add(e[0], e[1]), 0.5);
        add(m, scale(run, station - dot(sub(m, origin), run)))
    };
    let (mid1, mid2) = (mid(&ends1), mid(&ends2));
    let d = dot(normal1, normal2);
    let denominator = 1.0 - d * d;
    let offset1 = dot(sub(mid1, cross), normal1);
    let offset2 = dot(sub(mid2, cross), normal2);
    let corner = add(
        add(cross, scale(normal1, (offset1 - d * offset2) / denominator)),
        scale(normal2, (offset2 - d * offset1) / denominator),
    );
    let (leg_vec1, leg_vec2) = (sub(mid1, corner), sub(mid2, corner));
    let (leg1, leg2) = (norm(leg_vec1), norm(leg_vec2));
    if leg1.min(leg2) <= COORD_FLOOR || leg1.max(leg2) > body.max_leg_frac * body.stock_size {
        return None;
    }
    // A broad terminal face between two real bevel strips can imitate a bevel; its virtual
    // edge lies outside the solid's box, where a real edge break cannot.
    let envelope_tol = COORD_FLOOR.max(body.stock_size * 1e-6);
    if (0..3).any(|i| {
        corner[i] < body.bounds.min[i] - envelope_tol
            || corner[i] > body.bounds.max[i] + envelope_tol
    }) {
        return None;
    }
    if dot(normalized(leg_vec1)?, normalized(leg_vec2)?).abs() > SMOOTH_ARC_GAP {
        return None;
    }
    // The corner-to-face side is removed material, and so must the far side of the virtual
    // edge be (a concave gusset or web fails); just inside the bevel is material, just
    // outside is not.
    let material = |p: V3| material_at(body.classifier, p);
    let toward = scale(sub(cross, corner), INTERIOR_PROBE_FRAC);
    if material(add(corner, toward)) || material(sub(corner, toward)) {
        return None;
    }
    let offset = leg1.min(leg2) * INTERIOR_PROBE_FRAC;
    if !material(sub(cross, scale(surface_normal, offset)))
        || material(add(cross, scale(surface_normal, offset)))
    {
        return None;
    }
    let mut legs = [
        (leg1, normalized(leg_vec1)?, first_span),
        (leg2, normalized(leg_vec2)?, second_span),
    ];
    legs.sort_by(leg_order);
    let span = |s: [f64; 2]| [py::round_to3(s[0] - station), py::round_to3(s[1] - station)];
    Some(OrientedChamfer {
        run: coordinates(run, 6),
        length: py::round_to3(run_length),
        at: Placed::new(at, Grid::CleanDecimals(3)),
        corner: Placed::new(corner, Grid::CleanDecimals(3)),
        leg1: py::round_to3(legs[0].0),
        leg2: py::round_to3(legs[1].0),
        leg1_direction: coordinates(legs[0].1, 6),
        leg2_direction: coordinates(legs[1].1, 6),
        support_spans: [span(legs[0].2), span(legs[1].2)],
        angle: py::round_to(legs[1].0.atan2(legs[0].0).to_degrees(), 2),
        body_key: None, // filled in by `discover`
    })
}

/// Coordinate order is only a presentation tie when every published measurement is equal.
/// The six-decimal comparison bucket must not swap unequal millimetre legs or angles.
fn leg_order(a: &(f64, V3, [f64; 2]), b: &(f64, V3, [f64; 2])) -> std::cmp::Ordering {
    let primary = py::order(-py::round_to(a.0, 6), -py::round_to(b.0, 6));
    if primary.is_ne() {
        return primary;
    }
    let angle = |x: f64, y: f64| py::round_to(y.atan2(x).to_degrees(), 2);
    if py::round_to3(a.0) != py::round_to3(b.0) || angle(a.0, b.0) != angle(b.0, a.0) {
        return py::order(b.0, a.0);
    }
    py::tuple_order(&coordinates(a.1, 6), &coordinates(b.1, 6))
}

internal_entry! {
/// Panics unless `0 < max_leg_frac < 1`, where Python raises `ValueError`.
pub fn discover(
    ctx: &Context<'_>,
    opts: &OrientedChamferOptions,
) -> Vec<Occurrence<OrientedChamfer>> {
    assert!(
        0.0 < opts.max_leg_frac && opts.max_leg_frac < 1.0,
        "max_leg_frac must be between zero and one"
    );
    let part = ctx.part;
    let mut found = Vec::new();
    for (s, solid) in part.solids.iter().enumerate() {
        // A one-body part is probed whole, a compound's body alone.
        let classifier = if part.solids.len() == 1 {
            ctx.classifier()
        } else {
            ctx.solid_classifier(s)
        };
        let bounds = part.solid_bounds(s);
        let body = Body {
            classifier,
            stock_size: bounds.max_extent(),
            bounds,
            max_leg_frac: opts.max_leg_frac,
        };
        let mut faces = solid.faces.clone();
        faces.sort_unstable();
        for bevel in faces {
            if !linear_quad(part, bevel) {
                continue;
            }
            let neighbours: Vec<usize> = part
                .neighbours(bevel)
                .into_iter()
                .filter(|n| solid.faces.contains(n))
                .collect();
            if neighbours.len() != 4 {
                continue;
            }
            // A triangular blind terminal makes the slant an angled step.
            if neighbours.iter().any(|&n| {
                matches!(part.faces[n].surface, Surface::Plane { .. })
                    && part.outer_edges(n).len() == 3
            }) {
                continue;
            }
            let records: Vec<OrientedChamfer> = (0..4)
                .flat_map(|i| (i + 1..4).map(move |j| (i, j)))
                .filter_map(|(i, j)| pair(part, &body, bevel, neighbours[i], neighbours[j]))
                .collect();
            if let [record] = &records[..] {
                found.push((s, record.clone(), bevel));
            }
        }
    }
    // Body keys need every solid's exact mass, so they are only computed for parts with records.
    let keys = if found.is_empty() {
        vec![]
    } else {
        ctx.body_keys(true)
    };
    let mut out: Vec<_> = found
        .into_iter()
        .map(|(s, record, bevel)| Occurrence {
            record: OrientedChamfer {
                body_key: keys[s].clone(),
                ..record
            },
            defining: vec![bevel],
            context: vec![],
        })
        .collect();
    out.sort_by(|a, b| py::tuple_order(&a.record.sort_key(), &b.record.sort_key()));
    out
}
}

#[cfg(test)]
mod leg_tie_tests {
    use super::*;
    #[test]
    fn serialized_direction_ties_cannot_swap_unequal_published_legs_under_motion() {
        for angle in [0_f64, 30., 90., 180.] {
            let (s, c) = angle.to_radians().sin_cos();
            let mut legs = [
                (0.7474998, [c, s, 0.], [0., 3.]),
                (0.7475002, [-s, c, 0.], [0., 3.]),
            ];
            assert_eq!(py::round_to(legs[0].0, 6), py::round_to(legs[1].0, 6));
            legs.sort_by(leg_order);
            assert_eq!(
                (py::round_to3(legs[0].0), py::round_to3(legs[1].0)),
                (0.748, 0.747)
            );
            legs.reverse();
            legs.sort_by(leg_order);
            assert_eq!(legs[0].0, 0.7475002);
        }
    }
}
