//! Edge-open, round-bottom blind slots with one internal cap (`quiddity.round_bottom_slots`).
//!
//! The constant section is an open U: a flat floor joined tangentially to two equal
//! quarter-cylinders, open at a face of the solid's box in depth, and swept from a mouth on the
//! box along the run to one planar blind cap.

use std::cmp::Ordering;
use std::f64::consts::PI;

use serde::Serialize;

use super::Context;
use super::evidence::{self, EvidenceError, Occurrence};
use super::placed::{Grid, Placed};
use super::planes::axis_aligned_axis;
use super::policy::{AXIS_ALIGNED_COS, AXIS_ZERO_COS};
use super::regions::{
    Region, RunKind, WireEdge, boundary_runs, common_convex_context, coplanar_region, edge_radius,
    empty_sweep, length_tolerance, principal_rectangle, region_boundary, region_bounds, relation,
    run_length, same_span,
};
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::{Surface, V3};
use crate::kernel::py;

const AXES: [char; 3] = ['x', 'y', 'z'];

/// One capped, edge-open U-section slot. `axis` is the run direction and `open_sign` the side
/// of its mouth; `depth_sign` the side the U opens to along `depth_axis`. The opening is
/// `flat_width + 2 * radius` wide and `radius` deep.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct RoundBottomBlindSlot {
    pub axis: char,
    pub open_sign: i32,
    pub length: f64,
    pub width_axis: char,
    pub depth_axis: char,
    pub depth_sign: i32,
    pub radius: f64,
    pub flat_width: f64,
    pub at: Placed<V3>,
}

impl RoundBottomBlindSlot {
    pub fn width(&self) -> f64 {
        self.flat_width + 2.0 * self.radius
    }

    pub fn depth(&self) -> f64 {
        self.radius
    }

    /// The dataclass's field-tuple order.
    fn order(&self, other: &Self) -> Ordering {
        self.axis
            .cmp(&other.axis)
            .then(self.open_sign.cmp(&other.open_sign))
            .then(self.length.total_cmp(&other.length))
            .then(self.width_axis.cmp(&other.width_axis))
            .then(self.depth_axis.cmp(&other.depth_axis))
            .then(self.depth_sign.cmp(&other.depth_sign))
            .then(self.radius.total_cmp(&other.radius))
            .then(self.flat_width.total_cmp(&other.flat_width))
            .then_with(|| py::tuple_order(&self.at[..], &other.at[..]))
    }
}

/// A logical cylinder: its faces, radius, axis and a point on the axis.
#[derive(Clone, Debug)]
struct Cylinder {
    nodes: Region,
    radius: f64,
    axis: usize,
}

/// A native cylinder whose axis is a coordinate axis (`_cylinder_surface`).
fn cylinder_surface(part: &Part, face: usize) -> Option<(f64, usize, V3)> {
    let Surface::Cylinder { frame, radius } = &part.faces[face].surface else {
        return None;
    };
    let d = frame.z;
    let aligned: Vec<usize> = (0..3)
        .filter(|&a| {
            d[a].abs() >= AXIS_ALIGNED_COS && (0..3).all(|i| i == a || d[i].abs() <= AXIS_ZERO_COS)
        })
        .collect();
    (aligned.len() == 1).then(|| (*radius, aligned[0], frame.origin))
}

/// `_same_cylinder`.
fn same_cylinder(left: (f64, usize, V3), right: (f64, usize, V3)) -> bool {
    let ((radius, axis, centre), (other_radius, other_axis, other_centre)) = (left, right);
    let tolerance = length_tolerance(&[radius, other_radius]);
    axis == other_axis
        && (radius - other_radius).abs() <= tolerance
        && (0..3).all(|i| i == axis || (centre[i] - other_centre[i]).abs() <= tolerance)
}

/// The seed's cylinder joined across smooth seams to the faces of the same cylinder
/// (`_cylinder_region`).
fn cylinder_region(part: &Part, seed: usize) -> Option<Cylinder> {
    let surface = cylinder_surface(part, seed)?;
    let mut found = Region::from([seed]);
    let mut pending = vec![seed];
    while let Some(current) = pending.pop() {
        for neighbour in part.neighbours(current) {
            if found.contains(&neighbour)
                || !cylinder_surface(part, neighbour).is_some_and(|c| same_cylinder(surface, c))
                || part.arc(current, neighbour) != Some(Arc::Smooth)
            {
                continue;
            }
            found.insert(neighbour);
            pending.push(neighbour);
        }
    }
    let (radius, axis, _) = surface;
    Some(Cylinder {
        nodes: found,
        radius,
        axis,
    })
}

/// The two straight and two circular runs, alternating, of one U boundary
/// (`_alternating_profile_runs`).
type Runs = Vec<Vec<WireEdge>>;
fn alternating_profile_runs(part: &Part, wire: &[WireEdge]) -> Option<(Runs, Runs)> {
    let groups = boundary_runs(part, wire)?;
    if groups.len() != 4 || (0..4).any(|i| groups[i].0 == groups[(i + 1) % 4].0) {
        return None;
    }
    let of = |kind: RunKind| -> Runs {
        groups
            .iter()
            .filter(|(k, _)| *k == kind)
            .map(|(_, members)| members.clone())
            .collect()
    };
    Some((of(RunKind::Line), of(RunKind::Circle)))
}

/// Whether every arc run is a quarter circle of *radius* (`_quarter_cylinder` and
/// `_cap_matches_profile` share this reading).
fn quarter_arcs(part: &Part, arcs: &[Vec<WireEdge>], radius: f64) -> bool {
    arcs.iter().all(|run| {
        run.iter().all(|w| {
            edge_radius(part, w.edge)
                .is_some_and(|r| (r - radius).abs() <= length_tolerance(&[radius]))
        }) && (run_length(part, run) - PI * radius / 2.0).abs() <= length_tolerance(&[radius])
    })
}

/// Whether the cylinder's faces are bounded by two straight runs the length of the run span
/// and two quarter arcs (`_quarter_cylinder`).
fn quarter_cylinder(part: &Part, cylinder: &Cylinder, run_span: (f64, f64)) -> bool {
    let Some((lines, arcs)) = region_boundary(part, &cylinder.nodes, false)
        .and_then(|wire| alternating_profile_runs(part, &wire))
    else {
        return false;
    };
    let length = run_span.1 - run_span.0;
    lines.len() == 2
        && arcs.len() == 2
        && lines
            .iter()
            .all(|run| (run_length(part, run) - length).abs() <= length_tolerance(&[length]))
        && quarter_arcs(part, &arcs, cylinder.radius)
}

/// Whether the cap's boundary is the U profile: two quarter arcs, the floor's width and the
/// opening's (`_cap_matches_profile`).
fn cap_matches_profile(part: &Part, cap: &Region, radius: f64, flat_width: f64) -> bool {
    let Some((lines, arcs)) =
        region_boundary(part, cap, true).and_then(|wire| alternating_profile_runs(part, &wire))
    else {
        return false;
    };
    let expected_width = flat_width + 2.0 * radius;
    let lengths: Vec<f64> = lines.iter().map(|run| run_length(part, run)).collect();
    let shortest = lengths.iter().copied().fold(f64::INFINITY, f64::min);
    let longest = lengths.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    arcs.len() == 2
        && lines.len() == 2
        && quarter_arcs(part, &arcs, radius)
        && (shortest - flat_width).abs() <= length_tolerance(&[flat_width])
        && (longest - expected_width).abs() <= length_tolerance(&[expected_width])
}

internal_entry! {
/// `recognise_round_bottom_blind_slots`.
pub fn recognise_round_bottom_blind_slots(part: &Part) -> Vec<RoundBottomBlindSlot> {
    super::records(discover(&Context::new(part)))
}
}

internal_entry! {
/// The evidence path: each slot with its two curved sides, floor and cap.
pub fn discover_verified(
    ctx: &Context<'_>,
) -> Result<Vec<Occurrence<RoundBottomBlindSlot>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx))
}
}

internal_entry! {
/// Every slot of every valid solid, in record order.
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<RoundBottomBlindSlot>> {
    let mut found: Vec<Occurrence<RoundBottomBlindSlot>> = (0..ctx.part.solids.len())
        .filter(|&s| ctx.part.solid_is_valid(s))
        .flat_map(|s| recognise_one(ctx, s))
        .collect();
    found.sort_by(|a, b| a.record.order(&b.record));
    found
}
}

fn recognise_one(ctx: &Context<'_>, solid: usize) -> Vec<Occurrence<RoundBottomBlindSlot>> {
    let part = ctx.part;
    let solid_nodes: Region = part.solids[solid].faces.iter().copied().collect();
    let envelope = part.solid_bounds(solid);
    let mut out = Vec::new();
    let mut seen_caps = Region::new();
    for &cap in &solid_nodes {
        if seen_caps.contains(&cap) {
            continue;
        }
        let Some(cap_plane) = axis_aligned_axis(part, cap) else {
            continue;
        };
        // A defining cap touches one of its curved walls concavely: the cheap test first.
        if !part.neighbours(cap).into_iter().any(|n| {
            matches!(part.faces[n].surface, Surface::Cylinder { .. })
                && part.arc(cap, n) == Some(Arc::Concave)
        }) {
            continue;
        }
        let cap_region: Region = coplanar_region(part, cap)
            .intersection(&solid_nodes)
            .copied()
            .collect();
        seen_caps.extend(&cap_region);
        if cap_region
            .iter()
            .any(|&n| axis_aligned_axis(part, n) != Some(cap_plane))
        {
            continue;
        }
        let (run, cap_station) = cap_plane;
        let mut neighbours: Vec<usize> = Vec::new();
        for &source in &cap_region {
            for n in part.neighbours(source) {
                if !cap_region.contains(&n) && !neighbours.contains(&n) {
                    neighbours.push(n);
                }
            }
        }
        // By their faces within the solid, in first-seen order; a later seed's reading wins.
        let mut cylinder_by_nodes: Vec<Cylinder> = Vec::new();
        let mut planar_regions: Vec<Region> = Vec::new();
        for node in neighbours {
            if let Some(mut cylinder) = cylinder_region(part, node) {
                cylinder.nodes = cylinder.nodes.intersection(&solid_nodes).copied().collect();
                match cylinder_by_nodes
                    .iter_mut()
                    .find(|c| c.nodes == cylinder.nodes)
                {
                    Some(slot) => *slot = cylinder,
                    None => cylinder_by_nodes.push(cylinder),
                }
            } else if matches!(part.faces[node].surface, Surface::Plane { .. }) {
                let region: Region = coplanar_region(part, node)
                    .intersection(&solid_nodes)
                    .copied()
                    .collect();
                if !planar_regions.contains(&region) {
                    planar_regions.push(region);
                }
            }
        }
        let concave = |r: &Region| relation(part, &cap_region, r) == Some(Arc::Concave);
        let cylinders: Vec<&Cylinder> = cylinder_by_nodes
            .iter()
            .filter(|c| concave(&c.nodes))
            .collect();
        let planar: Vec<&Region> = planar_regions.iter().filter(|r| concave(r)).collect();
        let ([left, right], [floor_region]) = (cylinders.as_slice(), planar.as_slice()) else {
            continue;
        };
        let floor = *floor_region.first().expect("a related region has faces");
        let Some((depth, floor_coord)) = axis_aligned_axis(part, floor) else {
            continue;
        };
        if left.axis != run
            || right.axis != run
            || depth == run
            || !principal_rectangle(part, floor_region, depth)
            || (left.radius - right.radius).abs() > length_tolerance(&[left.radius, right.radius])
            || relation(part, &left.nodes, floor_region) != Some(Arc::Smooth)
            || relation(part, &right.nodes, floor_region) != Some(Arc::Smooth)
        {
            continue;
        }
        let width = 3 - run - depth;
        let side_regions = [&left.nodes, *floor_region, &right.nodes];
        let Some((low, high)) = same_span(part, &side_regions, run) else {
            continue;
        };
        if !quarter_cylinder(part, left, (low, high)) || !quarter_cylinder(part, right, (low, high))
        {
            continue;
        }
        let run_tolerance = length_tolerance(&[high - low]);
        let (open_sign, open_station) = if (cap_station - low).abs() <= run_tolerance
            && (high - envelope.max[run]).abs() <= run_tolerance
        {
            (1, high)
        } else if (cap_station - high).abs() <= run_tolerance
            && (low - envelope.min[run]).abs() <= run_tolerance
        {
            (-1, low)
        } else {
            continue;
        };
        let floor_bounds = region_bounds(part, floor_region);
        let flat_width = floor_bounds.max[width] - floor_bounds.min[width];
        let radius = (left.radius + right.radius) / 2.0;
        let sides = [
            region_bounds(part, &left.nodes),
            region_bounds(part, &right.nodes),
        ];
        let profile_low = sides[0].min[width].min(sides[1].min[width]);
        let profile_high = sides[0].max[width].max(sides[1].max[width]);
        let depth_low = sides[0].min[depth].min(sides[1].min[depth]);
        let depth_high = sides[0].max[depth].max(sides[1].max[depth]);
        let profile_tolerance = length_tolerance(&[radius, flat_width]);
        let depth_open = if (depth_low - floor_coord).abs() <= profile_tolerance {
            depth_high
        } else {
            depth_low
        };
        let depth_sign = if depth_open > floor_coord { 1 } else { -1 };
        if flat_width <= 0.0
            || ((profile_high - profile_low) - (flat_width + 2.0 * radius)).abs()
                > profile_tolerance
            || ((depth_open - floor_coord).abs() - radius).abs() > profile_tolerance
            || ![envelope.min[depth], envelope.max[depth]]
                .iter()
                .any(|end| (depth_open - end).abs() <= profile_tolerance)
            || !cap_matches_profile(part, &cap_region, radius, flat_width)
            || !common_convex_context(part, &side_regions, run, open_station, high - low)
            || !common_convex_context(
                part,
                &[&left.nodes, &cap_region, &right.nodes],
                depth,
                depth_open,
                radius,
            )
        {
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
        centre[width] = (profile_low + profile_high) / 2.0;
        centre[depth] = (floor_coord + depth_open) / 2.0;
        let mut defining: Vec<usize> = side_regions
            .iter()
            .flat_map(|r| r.iter().copied())
            .chain(cap_region.iter().copied())
            .collect();
        defining.sort_unstable();
        defining.dedup();
        out.push(Occurrence {
            record: RoundBottomBlindSlot {
                axis: AXES[run],
                open_sign,
                length: py::round_to3(high - low),
                width_axis: AXES[width],
                depth_axis: AXES[depth],
                depth_sign,
                radius: py::round_to3(radius),
                flat_width: py::round_to3(flat_width),
                at: Placed::new(centre, Grid::Decimals(3)),
            },
            defining,
            context: Vec::new(),
        });
    }
    out
}
