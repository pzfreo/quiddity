//! Edge-open rectangular blind slots with one internal cap (`quiddity.rectangular_blind_slots`).
//!
//! The constant section is an open rectangle: two parallel principal-plane sides and a floor,
//! each meeting the cap concavely, open at a face of the solid's box in depth, and swept from a
//! mouth on the box along the run to one planar blind cap.

use std::cmp::Ordering;

use serde::Serialize;

use super::Context;
use super::evidence::{self, EvidenceError, Occurrence};
use super::placed::{Grid, Placed};
use super::planes::axis_aligned_axis;
use super::regions::{
    Region, common_convex_context, coplanar_region, empty_sweep, length_tolerance,
    principal_rectangle, region_boundary, region_bounds, relation,
};
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::{Surface, V3};
use crate::kernel::py;

const AXES: [char; 3] = ['x', 'y', 'z'];

/// One capped, edge-open rectangular U-section slot. `axis` is the run direction and
/// `open_sign` the side of its mouth; `depth_sign` the side the section opens to along
/// `depth_axis`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct RectangularBlindSlot {
    pub axis: char,
    pub open_sign: i32,
    pub length: f64,
    pub width_axis: char,
    pub depth_axis: char,
    pub depth_sign: i32,
    pub width: f64,
    pub depth: f64,
    pub at: Placed<V3>,
}

impl RectangularBlindSlot {
    /// The dataclass's field-tuple order.
    fn order(&self, other: &Self) -> Ordering {
        self.axis
            .cmp(&other.axis)
            .then(self.open_sign.cmp(&other.open_sign))
            .then(self.length.total_cmp(&other.length))
            .then(self.width_axis.cmp(&other.width_axis))
            .then(self.depth_axis.cmp(&other.depth_axis))
            .then(self.depth_sign.cmp(&other.depth_sign))
            .then(self.width.total_cmp(&other.width))
            .then(self.depth.total_cmp(&other.depth))
            .then_with(|| py::tuple_order(&self.at[..], &other.at[..]))
    }
}

/// Whether *actual* covers *expected* within the tolerance of *nominal* (`_contains_span`).
fn contains_span(actual: (f64, f64), expected: (f64, f64), nominal: f64) -> bool {
    let tolerance = length_tolerance(&[nominal]);
    actual.0 <= expected.0 + tolerance && actual.1 >= expected.1 - tolerance
}

/// A run at least as wide as the section and distinctly longer than its depth
/// (`_has_unambiguous_slot_roles`).
fn has_unambiguous_slot_roles(length: f64, width: f64, depth: f64) -> bool {
    let tolerance = length_tolerance(&[length, width, depth]);
    length + tolerance >= width && length > depth + tolerance
}

internal_entry! {
/// `recognise_rectangular_blind_slots`.
pub fn recognise_rectangular_blind_slots(part: &Part) -> Vec<RectangularBlindSlot> {
    super::records(discover(&Context::new(part)))
}
}

internal_entry! {
/// The evidence path: each slot with its two sides, floor and cap.
pub fn discover_verified(
    ctx: &Context<'_>,
) -> Result<Vec<Occurrence<RectangularBlindSlot>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx))
}
}

internal_entry! {
/// Every slot of every valid solid, in record order.
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<RectangularBlindSlot>> {
    let mut found: Vec<Occurrence<RectangularBlindSlot>> = (0..ctx.part.solids.len())
        .filter(|&s| ctx.part.solid_is_valid(s))
        .flat_map(|s| recognise_one(ctx, s))
        .collect();
    found.sort_by(|a, b| a.record.order(&b.record));
    found
}
}

/// A region within the solid, from its coplanar seed.
fn solid_region(part: &Part, seed: usize, solid_nodes: &Region) -> Region {
    coplanar_region(part, seed)
        .intersection(solid_nodes)
        .copied()
        .collect()
}

/// Whether every face of a region lies on *plane*.
fn on_plane(part: &Part, region: &Region, plane: (usize, f64)) -> bool {
    region
        .iter()
        .all(|&n| axis_aligned_axis(part, n) == Some(plane))
}

fn recognise_one(ctx: &Context<'_>, solid: usize) -> Vec<Occurrence<RectangularBlindSlot>> {
    let part = ctx.part;
    let solid_nodes: Region = part.solids[solid].faces.iter().copied().collect();
    let envelope = part.solid_bounds(solid);
    let is_planar = |n: usize| matches!(part.faces[n].surface, Surface::Plane { .. });
    let mut found: Vec<(RectangularBlindSlot, Region)> = Vec::new();
    let mut seen_caps = Region::new();
    for &cap in &solid_nodes {
        if seen_caps.contains(&cap) {
            continue;
        }
        let Some(cap_plane) = axis_aligned_axis(part, cap) else {
            continue;
        };
        // A cap meets at least two of its planar walls concavely: the cheap test first.
        let concave_planar = part
            .neighbours(cap)
            .into_iter()
            .filter(|&n| is_planar(n) && part.arc(cap, n) == Some(Arc::Concave))
            .count();
        if concave_planar < 2 {
            continue;
        }
        let cap_region = solid_region(part, cap, &solid_nodes);
        seen_caps.extend(&cap_region);
        let (run, cap_station) = cap_plane;
        if !on_plane(part, &cap_region, cap_plane) || !principal_rectangle(part, &cap_region, run) {
            continue;
        }
        let neighbours: Region = cap_region
            .iter()
            .flat_map(|&source| part.neighbours(source))
            .filter(|n| !cap_region.contains(n) && is_planar(*n))
            .collect();
        let mut regions: Vec<Region> = Vec::new();
        for node in neighbours {
            let region = solid_region(part, node, &solid_nodes);
            if !regions.contains(&region) {
                regions.push(region);
            }
        }
        let concave: Vec<&Region> = regions
            .iter()
            .filter(|r| relation(part, &cap_region, r) == Some(Arc::Concave))
            .collect();
        if concave.len() != 3 {
            continue;
        }
        // Each concave region on one principal plane across the run; Python breaks out of the
        // cap otherwise (unreachable for coplanar regions, which share their seed's plane).
        let mut planes: Vec<(&Region, (usize, f64))> = Vec::new();
        for region in concave {
            let seed = *region.first().expect("a related region has faces");
            match axis_aligned_axis(part, seed) {
                Some(plane) if plane.0 != run && on_plane(part, region, plane) => {
                    planes.push((region, plane))
                }
                _ => break,
            }
        }
        if planes.len() != 3 {
            continue;
        }
        let count = |axis: usize| planes.iter().filter(|(_, p)| p.0 == axis).count();
        let width_axes: Vec<usize> = (0..3).filter(|&a| count(a) == 2).collect();
        let depth_axes: Vec<usize> = (0..3).filter(|&a| count(a) == 1).collect();
        let ([width], [depth]) = (width_axes.as_slice(), depth_axes.as_slice()) else {
            continue;
        };
        let (width, depth) = (*width, *depth);
        if run + width + depth != 3 || run == width || run == depth {
            continue;
        }
        let sides: Vec<&Region> = planes
            .iter()
            .filter(|(_, p)| p.0 == width)
            .map(|(r, _)| *r)
            .collect();
        let (floor, floor_plane) = *planes
            .iter()
            .find(|(_, p)| p.0 == depth)
            .expect("one region across the depth");
        if sides.iter().any(|r| !principal_rectangle(part, r, width))
            || !principal_rectangle(part, floor, depth)
        {
            continue;
        }
        let mut side_planes: Vec<f64> = planes
            .iter()
            .filter(|(_, p)| p.0 == width)
            .map(|(_, p)| p.1)
            .collect();
        side_planes.sort_by(f64::total_cmp);
        let cap_bounds = region_bounds(part, &cap_region);
        let width_span = (cap_bounds.min[width], cap_bounds.max[width]);
        let depth_span = (cap_bounds.min[depth], cap_bounds.max[depth]);
        let section_width = width_span.1 - width_span.0;
        let section_depth = depth_span.1 - depth_span.0;
        let section_tolerance = length_tolerance(&[section_width, section_depth]);
        let floor_coord = floor_plane.1;
        if section_width <= 0.0
            || section_depth <= 0.0
            || (side_planes[0] - width_span.0).abs() > section_tolerance
            || (side_planes[1] - width_span.1).abs() > section_tolerance
            || (floor_coord - depth_span.0)
                .abs()
                .min((floor_coord - depth_span.1).abs())
                > section_tolerance
            || relation(part, sides[0], floor) != Some(Arc::Concave)
            || relation(part, sides[1], floor) != Some(Arc::Concave)
        {
            continue;
        }
        let depth_open = if (floor_coord - depth_span.0).abs() <= section_tolerance {
            depth_span.1
        } else {
            depth_span.0
        };
        let depth_sign = if depth_open > floor_coord { 1 } else { -1 };
        if ![envelope.min[depth], envelope.max[depth]]
            .iter()
            .any(|station| (depth_open - station).abs() <= section_tolerance)
        {
            continue;
        }
        let side_bounds = [region_bounds(part, sides[0]), region_bounds(part, sides[1])];
        let floor_bounds = region_bounds(part, floor);
        for (open_sign, open_station) in [(-1, envelope.min[run]), (1, envelope.max[run])] {
            let (low, high) = (cap_station.min(open_station), cap_station.max(open_station));
            let length = high - low;
            if length <= 0.0 || !has_unambiguous_slot_roles(length, section_width, section_depth) {
                continue;
            }
            if !side_bounds
                .iter()
                .chain([&floor_bounds])
                .all(|b| contains_span((b.min[run], b.max[run]), (low, high), length))
            {
                continue;
            }
            let side_regions = [sides[0], floor, sides[1]];
            if !common_convex_context(part, &side_regions, run, open_station, length) {
                continue;
            }
            let Some(cap_wire) = region_boundary(part, &cap_region, true) else {
                continue;
            };
            if !empty_sweep(
                ctx.solid_classifier(solid),
                part,
                &cap_wire,
                run,
                cap_station,
                open_station,
            ) {
                continue;
            }
            let mut centre = [0.0; 3];
            centre[run] = (low + high) / 2.0;
            centre[width] = (width_span.0 + width_span.1) / 2.0;
            centre[depth] = (depth_span.0 + depth_span.1) / 2.0;
            let record = RectangularBlindSlot {
                axis: AXES[run],
                open_sign,
                length: py::round_to3(length),
                width_axis: AXES[width],
                depth_axis: AXES[depth],
                depth_sign,
                width: py::round_to3(section_width),
                depth: py::round_to3(section_depth),
                at: Placed::new(centre, Grid::Decimals(3)),
            };
            let nodes: Region = cap_region
                .iter()
                .chain(sides[0])
                .chain(sides[1])
                .chain(floor)
                .copied()
                .collect();
            found.push((record, nodes));
        }
    }
    // One boundary cannot prove two different role assignments: equal rediscovery is kept once,
    // competing records over the same faces are refused.
    let mut by_nodes: Vec<(Region, Vec<RectangularBlindSlot>)> = Vec::new();
    for (record, nodes) in found {
        match by_nodes.iter_mut().find(|(n, _)| *n == nodes) {
            Some((_, records)) => {
                if !records.contains(&record) {
                    records.push(record);
                }
            }
            None => by_nodes.push((nodes, vec![record])),
        }
    }
    by_nodes
        .into_iter()
        .filter(|(_, records)| records.len() == 1)
        .map(|(nodes, mut records)| Occurrence {
            record: records.remove(0),
            defining: nodes.into_iter().collect(),
            context: Vec::new(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // `test_local_span_tolerance_accepts_and_rejects_both_sides`.
    #[test]
    fn span_tolerance_accepts_and_rejects_both_sides() {
        let tolerance = length_tolerance(&[20.0]);
        let expected = (0.0, 20.0);
        assert!(contains_span(
            (0.5 * tolerance, 20.0 - 0.5 * tolerance),
            expected,
            20.0
        ));
        assert!(!contains_span((2.0 * tolerance, 20.0), expected, 20.0));
        assert!(!contains_span(
            (0.0, 20.0 - 2.0 * tolerance),
            expected,
            20.0
        ));
    }

    // `test_role_tie_refuses_without_an_axis_or_traversal_tiebreak`.
    #[test]
    fn role_tie_refuses() {
        let tolerance = length_tolerance(&[20.0]);
        assert!(has_unambiguous_slot_roles(
            20.0,
            20.0,
            20.0 - 2.0 * tolerance
        ));
        assert!(!has_unambiguous_slot_roles(
            20.0,
            20.0,
            20.0 - 0.5 * tolerance
        ));
        assert!(!has_unambiguous_slot_roles(
            20.0,
            20.0 + 2.0 * tolerance,
            5.0
        ));
    }
}
