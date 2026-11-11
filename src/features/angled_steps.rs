//! Angled-step recognition (`quiddity.angled_steps`): an oblique planar face breaking a convex
//! corner of the part, larger than a chamfer, and closed at an end by a triangular flat.

use serde::Serialize;

use super::Context;
use super::bevel::{classify_bevel, material_beyond_corner, proved_bevel_planes};
use super::evidence::{self, EvidenceError, Occurrence, common_valid_solid};
use super::placed::{Grid, Placed};
use super::planes::axis_aligned_axis;
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::{self, Curve, V3};
use crate::kernel::py;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct AngledStep {
    pub axis: char,
    pub leg1: f64,
    pub leg2: f64,
    pub angle: f64,
    pub length: f64,
    pub at: Placed<V3>,
    pub corner: Option<Placed<V3>>,
}

internal_entry! {
/// `recognise_angled_steps`.
pub fn recognise_angled_steps(part: &Part) -> Vec<AngledStep> {
    super::records(discover(&Context::new(part)))
}
}

internal_entry! {
/// The evidence path: each step with its slant face and the triangular flats closing it.
pub fn discover_verified(ctx: &Context<'_>) -> Result<Vec<Occurrence<AngledStep>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx))
}
}

/// Corners of an outer loop made only of lines: direction changes between consecutive sides,
/// collinear runs counted once (`_effective_linear_sides`).
fn effective_linear_sides(part: &Part, face: usize) -> Option<usize> {
    let lp = &part.faces[face].loops[part.outer_loop(face)?];
    let mut directions = Vec::new();
    for &(e, forward) in &lp.edges {
        let edge = &part.edges[e];
        let Curve::Line { .. } = edge.curve else {
            return None;
        };
        let d = py::unit(geom::sub(edge.end, edge.start))?;
        directions.push(if forward { d } else { geom::scale(d, -1.0) });
    }
    if directions.is_empty() {
        return None;
    }
    let n = directions.len();
    Some(
        (0..n)
            .filter(|&i| {
                1.0 - geom::dot(directions[i], directions[(i + 1) % n]) > geom::SMOOTH_ARC_GAP
            })
            .count(),
    )
}

/// The axis-aligned triangular flats among the face's neighbours (`_terminal_read`).
fn terminals(part: &Part, face: usize) -> Vec<usize> {
    part.neighbours(face)
        .into_iter()
        .filter(|&other| {
            axis_aligned_axis(part, other).is_some()
                && (part.face_edges(other).len() == 3 || {
                    let outer = part.outer_edges(other).len();
                    outer == 3 || (outer > 3 && effective_linear_sides(part, other) == Some(3))
                })
        })
        .collect()
}

internal_entry! {
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<AngledStep>> {
    let part = ctx.part;
    let mut out = Vec::new();
    for face in 0..part.faces.len() {
        let Ok(bevel) = classify_bevel(part, face) else {
            continue;
        };
        let (centre, axis) = (bevel.bounds.centre(), bevel.edge_axis);
        let Some(planes) = proved_bevel_planes(ctx, face, centre, axis) else {
            continue;
        };
        if material_beyond_corner(ctx, centre, axis, &planes) {
            continue; // the corner two recess walls meet at, not a corner of the part
        }
        let closing = terminals(part, face);
        if closing.is_empty() {
            continue; // runs edge to edge: a chamfer
        }
        let Some(at) = part.face_centre(face) else {
            continue;
        };
        let corner = [0, 1, 2].map(|i| if i == axis { centre[i] } else { planes[&i] });
        let mut defining = vec![face];
        let mut context = Vec::new();
        for terminal in closing {
            // A perpendicular triangular flat joined concavely to the slant is the
            // blind cut's cap. Convex stock terminals only establish its opening.
            if part.arc(face, terminal) == Some(Arc::Concave)
                && axis_aligned_axis(part, terminal).is_some_and(|p| p.0 == axis)
                && common_valid_solid(part, &[face, terminal]).is_some()
            {
                defining.push(terminal);
            } else {
                context.push(terminal);
            }
        }
        out.push(Occurrence {
            record: AngledStep {
                axis: ['x', 'y', 'z'][axis],
                leg1: py::round_to3(bevel.leg_hi),
                leg2: py::round_to3(bevel.leg_lo),
                angle: py::round_to(bevel.leg_lo.atan2(bevel.leg_hi).to_degrees(), 2),
                length: py::round_to3(bevel.bounds.max[axis] - bevel.bounds.min[axis]),
                at: Placed::new(at, Grid::Decimals(3)),
                corner: super::bevel::proved_published_corner(ctx, &defining, axis, at, corner,
                    [bevel.leg_hi, bevel.leg_lo], bevel.leg_lo.atan2(bevel.leg_hi).to_degrees()),
            },
            defining,
            context,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concave_blind_cap_defines_the_angled_cut() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/golden_angled_blind_step.step");
        let part = crate::read_step_file(&path).unwrap();
        let found = discover(&Context::new(&part));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].defining, vec![2, 3]);
        assert_eq!(part.arc(2, 3), Some(Arc::Concave));
        assert!(
            !found[0].defining.contains(&0),
            "convex stock face is context"
        );
    }
}
