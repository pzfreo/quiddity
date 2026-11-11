//! Paired ramp steps (`quiddity.paired_ramp_steps`): two oblique planar ramps meeting in a
//! concave valley along one axis, closed by an internal flat at one end and open to the part's
//! exterior at the other.

use serde::Serialize;

use super::Context;
use super::evidence::{self, EvidenceError, Occurrence};
use super::placed::{Grid, Placed};
use super::policy::{self, AXIS_ALIGNED_COS};
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::{Curve, SMOOTH_ARC_GAP, Surface, V3};
use crate::kernel::py;

/// The shared valley edge must run along the ramps' axis this closely.
const RUN_DIRECTION_COS: f64 = 1.0 - SMOOTH_ARC_GAP;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct PairedRampStep {
    pub axis: char,
    pub angle: f64,
    pub length: f64,
    pub at: Placed<V3>,
    pub opening_direction: V3,
    pub half_width: f64,
    /// Each side's width, low side first, when the two ramps differ.
    pub half_widths: Option<(f64, f64)>,
    /// Working measurement axis for unequal half widths; published only when required.
    #[serde(skip)]
    pub(crate) half_width_axis: char,
}

internal_entry! {
/// `recognise_paired_ramp_steps`.
pub fn recognise_paired_ramp_steps(part: &Part) -> Vec<PairedRampStep> {
    super::records(discover(&Context::new(part)))
}
}

internal_entry! {
/// The evidence path: both ramps and the internal terminal define each step.
pub fn discover_verified(
    ctx: &Context<'_>,
) -> Result<Vec<Occurrence<PairedRampStep>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx))
}
}

/// A planar face running along exactly one axis: (axis, normal, per-axis spans).
struct Ramp {
    axis: usize,
    normal: V3,
    spans: [(f64, f64); 3],
}

fn read_ramp(part: &Part, face: usize) -> Option<Ramp> {
    let Surface::Plane { .. } = part.faces[face].surface else {
        return None;
    };
    let normal = part.face_normal(face, 0.0, 0.0)?;
    let run: Vec<usize> = (0..3)
        .filter(|&i| normal[i].abs() <= SMOOTH_ARC_GAP)
        .collect();
    let [axis] = run[..] else { return None };
    let b = part.face_bounds(face);
    Some(Ramp {
        axis,
        normal,
        spans: [0, 1, 2].map(|i| (b.min[i], b.max[i])),
    })
}

fn mid(span: (f64, f64)) -> f64 {
    0.5 * (span.0 + span.1)
}

internal_entry! {
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<PairedRampStep>> {
    let part = ctx.part;
    let ramps: Vec<Option<Ramp>> = (0..part.faces.len()).map(|f| read_ramp(part, f)).collect();
    let mut found = Vec::new();
    for left in 0..part.faces.len() {
        let Some(left_read) = &ramps[left] else {
            continue;
        };
        for right in part.neighbours(left) {
            if right <= left {
                continue;
            }
            let Some(right_read) = &ramps[right] else {
                continue;
            };
            if let Some(o) = candidate(part, left, right, left_read, right_read) {
                found.push(o);
            }
        }
    }
    found.sort_by(|a, b| order(&a.record, &b.record));
    found
}
}

/// The dataclass's field order (`order=True`).
fn order(a: &PairedRampStep, b: &PairedRampStep) -> std::cmp::Ordering {
    a.axis
        .cmp(&b.axis)
        .then(py::order(a.angle, b.angle))
        .then(py::order(a.length, b.length))
        .then_with(|| py::tuple_order(&a.at[..], &b.at[..]))
        .then_with(|| py::tuple_order(&a.opening_direction, &b.opening_direction))
        .then(py::order(a.half_width, b.half_width))
}

/// An alternate full-run concave sidewall path between the ramps closes a pocket section.
/// Blind and opening caps touch only one axial station, so they cannot close this path.
fn closed_sidewall_cycle(
    part: &Part,
    left: usize,
    right: usize,
    axis: usize,
    run: (f64, f64),
) -> bool {
    let tolerance = policy::length_tol(run.1 - run.0, 1e-9);
    if run.1 - run.0 <= 2.0 * tolerance {
        return false;
    }
    let complete_join = |a, b| {
        let mut intervals: Vec<_> = part
            .shared_edges(a, b)
            .into_iter()
            .filter_map(|edge| {
                let points = part.edges[edge].vertex_points();
                let low = points.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min);
                let high = points
                    .iter()
                    .map(|p| p[axis])
                    .fold(f64::NEG_INFINITY, f64::max);
                (high - low > tolerance).then_some((low, high))
            })
            .collect();
        intervals.sort_by(|a, b| py::order(a.0, b.0));
        let mut covered = run.0;
        for (low, high) in intervals {
            if low > covered + tolerance {
                return false;
            }
            covered = covered.max(high);
        }
        covered >= run.1 - tolerance
    };
    let mut seen = vec![false; part.faces.len()];
    seen[left] = true;
    let mut pending = vec![left];
    while let Some(face) = pending.pop() {
        for next in part.neighbours(face) {
            if seen[next]
                || (face == left && next == right)
                || !matches!(part.arc(face, next), Some(Arc::Concave | Arc::Smooth))
                || !complete_join(face, next)
            {
                continue;
            }
            if next == right {
                return true;
            }
            seen[next] = true;
            pending.push(next);
        }
    }
    false
}

fn candidate(
    part: &Part,
    left: usize,
    right: usize,
    l: &Ramp,
    r: &Ramp,
) -> Option<Occurrence<PairedRampStep>> {
    if r.axis != l.axis {
        return None;
    }
    let axis = l.axis;
    let cross: Vec<usize> = (0..3).filter(|&i| i != axis).collect();
    let opposed: Vec<usize> = cross
        .iter()
        .copied()
        .filter(|&i| l.normal[i] * r.normal[i] < 0.0)
        .collect();
    let same: Vec<usize> = cross
        .iter()
        .copied()
        .filter(|&i| l.normal[i] * r.normal[i] > 0.0)
        .collect();
    let (&[opposed], &[same]) = (&opposed[..], &same[..]) else {
        return None;
    };
    if cross
        .iter()
        .any(|&i| (l.normal[i].abs() - r.normal[i].abs()).abs() > SMOOTH_ARC_GAP)
    {
        return None;
    }
    let shared = part.shared_edges(left, right);
    let [edge] = shared[..] else { return None };
    let ed = &part.edges[edge];
    let Curve::Line { .. } = ed.curve else {
        return None;
    };
    let tangent = py::unit(crate::kernel::geom::sub(ed.end, ed.start))?;
    if tangent[axis].abs() < RUN_DIRECTION_COS || part.arc(left, right) != Some(Arc::Concave) {
        return None;
    }
    let right_neighbours = part.neighbours(right);
    let mut terminals: Vec<usize> = part
        .neighbours(left)
        .into_iter()
        .filter(|n| right_neighbours.contains(n))
        .filter(|&n| {
            matches!(part.faces[n].surface, Surface::Plane { .. })
                && part
                    .face_normal(n, 0.0, 0.0)
                    .is_some_and(|nv| nv[axis].abs() >= AXIS_ALIGNED_COS)
        })
        .collect();
    terminals.sort_unstable();
    let [t0, t1] = terminals[..] else { return None };
    // One valid solid owns all four faces.
    let solid = part.faces[left].solid?;
    if !part.solid_is_valid(solid)
        || [right, t0, t1]
            .iter()
            .any(|&f| part.faces[f].solid != Some(solid))
    {
        return None;
    }
    let bounds = part.solid_bounds(solid);
    let tolerance = policy::length_tol(bounds.max_extent(), 1e-9);
    // The convex opening and concave internal terminal establish a blind valley along
    // this axis; its role does not depend on the unrelated stock dimensions.
    let coordinate = |f: usize| {
        let b = part.face_bounds(f);
        mid((b.min[axis], b.max[axis]))
    };
    let on_exterior = |f: usize| {
        let c = coordinate(f);
        (c - bounds.min[axis])
            .abs()
            .min((c - bounds.max[axis]).abs())
            <= tolerance
    };
    let exterior: Vec<usize> = [t0, t1].into_iter().filter(|&f| on_exterior(f)).collect();
    let internal: Vec<usize> = [t0, t1]
        .into_iter()
        .filter(|f| !exterior.contains(f))
        .collect();
    let (&[outside], &[inside]) = (&exterior[..], &internal[..]) else {
        return None;
    };
    let arc = |a, b| part.arc(a, b);
    if arc(left, outside) != Some(Arc::Convex) || arc(right, outside) != Some(Arc::Convex) {
        return None;
    }
    if arc(left, inside) != Some(Arc::Concave) || arc(right, inside) != Some(Arc::Concave) {
        return None;
    }
    if closed_sidewall_cycle(part, left, right, axis, l.spans[axis]) {
        return None;
    }
    let edge_bounds = {
        let mut b = crate::kernel::geom::Bounds::empty();
        ed.samples.iter().for_each(|p| b.add(*p));
        [0, 1, 2].map(|i| (b.min[i], b.max[i]))
    };
    let run_tolerance = policy::length_tol(
        (l.spans[axis].1 - l.spans[axis].0).max(r.spans[axis].1 - r.spans[axis].0),
        1e-9,
    );
    let run = edge_bounds[axis];
    if [l.spans[axis], r.spans[axis]].iter().any(|span| {
        (span.0 - run.0).abs() > run_tolerance || (span.1 - run.1).abs() > run_tolerance
    }) {
        return None;
    }
    let at = Placed::new(edge_bounds.map(mid), Grid::Decimals(3));
    let angle = l.normal[opposed]
        .abs()
        .atan2(l.normal[same].abs())
        .to_degrees();
    let sign = if coordinate(outside) > coordinate(inside) {
        1.0
    } else {
        -1.0
    };
    let opening_direction = [0, 1, 2].map(|i| if i == axis { sign } else { 0.0 });
    let widths = (
        l.spans[opposed].1 - l.spans[opposed].0,
        r.spans[opposed].1 - r.spans[opposed].0,
    );
    let mut half_widths = None;
    if (widths.0 - widths.1).abs() > tolerance {
        let ridge = mid(edge_bounds[opposed]);
        let (lm, rm) = (mid(l.spans[opposed]), mid(r.spans[opposed]));
        if lm < ridge - tolerance && rm > ridge + tolerance {
            half_widths = Some((py::round_to3(widths.0), py::round_to3(widths.1)));
        } else if rm < ridge - tolerance && lm > ridge + tolerance {
            half_widths = Some((py::round_to3(widths.1), py::round_to3(widths.0)));
        } else {
            return None;
        }
    }
    Some(Occurrence {
        record: PairedRampStep {
            axis: ['x', 'y', 'z'][axis],
            angle: py::round_to(angle, 2),
            length: py::round_to3(run.1 - run.0),
            at,
            opening_direction,
            half_width: py::round_to3(0.5 * (widths.0 + widths.1)),
            half_widths,
            half_width_axis: ['x', 'y', 'z'][opposed],
        },
        defining: vec![left, right, inside],
        context: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::support_patches::polygon_part;

    /// A V cut open at y=0 and closed at y=5, in stock whose length along y varies.
    fn valley(stock_length: f64) -> Part {
        let points = [
            [0.0, 0.0, 0.0],
            [20.0, 0.0, 0.0],
            [20.0, 0.0, 40.0],
            [10.0, 0.0, 20.0],
            [0.0, 0.0, 40.0],
            [0.0, 5.0, 0.0],
            [20.0, 5.0, 0.0],
            [20.0, 5.0, 40.0],
            [10.0, 5.0, 20.0],
            [0.0, 5.0, 40.0],
            [0.0, stock_length, 0.0],
            [20.0, stock_length, 0.0],
            [20.0, stock_length, 40.0],
            [0.0, stock_length, 40.0],
        ];
        let faces = [
            vec![0, 1, 2, 3, 4],
            vec![4, 3, 8, 9],
            vec![3, 2, 7, 8],
            vec![9, 8, 7],
            vec![9, 7, 12, 13],
            vec![10, 13, 12, 11],
            vec![0, 4, 9, 13, 10],
            vec![1, 11, 12, 7, 2],
            vec![0, 10, 11, 1],
        ];
        polygon_part(&points, &faces, true).unwrap()
    }

    #[test]
    fn triangular_and_hexagonal_pocket_corners_are_not_paired_ramp_cuts() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/golden_triangular_and_hex_pockets.step");
        let part = crate::read_step_file(&path).unwrap();
        for degrees in [0.0_f64, 30.0] {
            let (sin, cos) = degrees.to_radians().sin_cos();
            let moved = part
                .transformed(&[
                    [cos, -sin, 0.0, 0.0],
                    [sin, cos, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                ])
                .unwrap();
            assert!(discover(&Context::new(&moved)).is_empty(), "pose {degrees}");
        }
    }

    #[test]
    fn stock_aspect_ratio_does_not_change_a_proved_valley() {
        let read = |length| {
            let part = valley(length);
            assert!(part.solid_is_valid(0));
            let found = discover(&Context::new(&part));
            assert_eq!(found.len(), 1, "stock length {length}");
            assert_eq!(found[0].defining, vec![1, 2, 3]);
            found[0].record.clone()
        };
        let short = read(10.0);
        assert_eq!(short, read(60.0));
        assert_eq!(short.axis, 'y');
        assert_eq!(short.length, 5.0);
        assert_eq!(short.half_width, 10.0);
        assert_eq!(short.opening_direction, [0.0, -1.0, 0.0]);
    }
}
