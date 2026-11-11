//! Rectangular through-step recognition (`quiddity.through_steps`): exactly two principal-plane
//! regions joined by one concave seam, open across the complete run of one valid solid, with the
//! removed quadrant proved empty by a volume probe. Channels, pockets, capped cuts, tapered or
//! curved walls, interrupted seams and partial-run steps are not through steps.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use serde::Serialize;

use super::Context;
use super::body::BodyKey;
use super::evidence::{self, EvidenceError, Occurrence};
use super::placed::{Grid, Placed};
use super::planes::axis_aligned_axis;
use super::volume_probe::{Spans, prism_is_empty};
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::{COORD_FLOOR, Curve, SMOOTH_ARC_GAP};
use crate::kernel::py;

const SPAN_EPS: f64 = COORD_FLOOR;
const AXES: [char; 3] = ['x', 'y', 'z'];

/// Whether a section endpoint is the solid's envelope or a convex local material boundary.
pub type EndpointScope = &'static str;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct ThroughStep {
    pub axis: char,
    pub length: f64,
    pub at: Placed<[f64; 3]>,
    /// Boundary endpoint, concave corner, boundary endpoint, in the two non-run coordinates.
    pub section: [Placed<[f64; 2]>; 3],
    pub body_key: Option<BodyKey>,
    pub endpoint_scopes: [EndpointScope; 2],
}

impl ThroughStep {
    /// `ThroughStep._order_key`.
    fn order(&self, other: &Self) -> Ordering {
        let flat = |s: &[Placed<[f64; 2]>; 3]| s.map(|p| *p).concat();
        self.axis
            .cmp(&other.axis)
            .then_with(|| py::order(self.length, other.length))
            .then_with(|| py::tuple_order(&self.at[..], &other.at[..]))
            .then_with(|| py::tuple_order(&flat(&self.section), &flat(&other.section)))
            .then_with(|| self.endpoint_scopes.cmp(&other.endpoint_scopes))
            .then_with(|| self.body_key.is_some().cmp(&other.body_key.is_some()))
            .then_with(|| {
                py::tuple_order(
                    self.body_key.as_ref().map_or(&[][..], |k| k.as_slice()),
                    other.body_key.as_ref().map_or(&[][..], |k| k.as_slice()),
                )
            })
    }
}

internal_entry! {
/// `recognise_through_steps`.
pub fn recognise_through_steps(part: &Part) -> Vec<ThroughStep> {
    super::records(discover(&Context::new(part)))
}
}

internal_entry! {
/// The evidence path: each step with both wall regions defining it.
pub fn discover_verified(ctx: &Context<'_>) -> Result<Vec<Occurrence<ThroughStep>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx))
}
}

/// A connected set of coplanar principal-plane faces.
pub(super) struct Region {
    pub(super) nodes: Vec<usize>,
    pub(super) normal_axis: usize,
    pub(super) coordinate: f64,
    pub(super) bounds: [(f64, f64); 3],
}

pub(super) type Planes = Vec<Option<(usize, f64)>>;

/// The connected, smoothly joined same-principal-plane faces containing *seed*
/// (`_coplanar_region`).
pub(super) fn coplanar_region(part: &Part, seed: usize, planes: &Planes) -> BTreeSet<usize> {
    let Some(plane) = planes[seed] else {
        return BTreeSet::new();
    };
    let mut found = BTreeSet::from([seed]);
    let mut pending = vec![seed];
    while let Some(current) = pending.pop() {
        for neighbour in part.neighbours(current) {
            if found.contains(&neighbour) {
                continue;
            }
            if part.arc(current, neighbour) == Some(Arc::Smooth)
                && planes[neighbour]
                    .is_some_and(|o| o.0 == plane.0 && (o.1 - plane.1).abs() <= COORD_FLOOR)
            {
                found.insert(neighbour);
                pending.push(neighbour);
            }
        }
    }
    found
}

/// The solid's principal-plane regions, seeded in face order (`_regions`).
pub(super) fn regions(part: &Part, solid_nodes: &BTreeSet<usize>, planes: &Planes) -> Vec<Region> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for &seed in solid_nodes {
        if seen.contains(&seed) {
            continue;
        }
        let Some((axis, coordinate)) = planes[seed] else {
            continue;
        };
        let region: Vec<usize> = coplanar_region(part, seed, planes)
            .intersection(solid_nodes)
            .copied()
            .collect();
        seen.extend(region.iter().copied());
        let bounds = [0, 1, 2].map(|i| {
            let lo = region
                .iter()
                .map(|&f| part.face_bounds(f).min[i])
                .fold(f64::INFINITY, f64::min);
            let hi = region
                .iter()
                .map(|&f| part.face_bounds(f).max[i])
                .fold(f64::NEG_INFINITY, f64::max);
            (lo, hi)
        });
        out.push(Region {
            nodes: region,
            normal_axis: axis,
            coordinate,
            bounds,
        });
    }
    out
}

/// The one arc kind every meeting of the two regions has, if there is exactly one (`_relation`).
pub(super) fn relation(part: &Part, left: &Region, right: &Region) -> Option<Arc> {
    let mut kinds = Vec::new();
    for &a in &left.nodes {
        for &b in &right.nodes {
            if let Some(kind) = part.arc(a, b)
                && !kinds.contains(&kind)
            {
                kinds.push(kind);
            }
        }
    }
    (kinds.len() == 1).then(|| kinds[0])
}

/// A line edge's box per axis and its direction; `None` for any other curve.
pub(super) fn line(part: &Part, edge: usize) -> Option<(Spans, [f64; 3])> {
    let e = &part.edges[edge];
    let Curve::Line { dir, .. } = e.curve else {
        return None;
    };
    let span = [0, 1, 2].map(|i| (e.start[i].min(e.end[i]), e.start[i].max(e.end[i])));
    Some((span, py::unit(dir)?))
}

/// Whether *intervals* merge into one interval from *low* to *high*.
pub(super) fn covers_run(mut intervals: Vec<(f64, f64)>, low: f64, high: f64) -> bool {
    intervals.sort_by(|a, b| py::tuple_order(&[a.0, a.1], &[b.0, b.1]));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (start, end) in intervals {
        match merged.last_mut() {
            Some(last) if start <= last.1 + SPAN_EPS => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged.len() == 1
        && (merged[0].0 - low).abs() <= SPAN_EPS
        && (merged[0].1 - high).abs() <= SPAN_EPS
}

/// The seam: straight edges along the run, together spanning it unbroken
/// (`_shared_run_is_complete`).
pub(super) fn shared_run_is_complete(
    part: &Part,
    left: &Region,
    right: &Region,
    run: usize,
    low: f64,
    high: f64,
) -> bool {
    let shared: Vec<usize> = left
        .nodes
        .iter()
        .flat_map(|&a| {
            right
                .nodes
                .iter()
                .flat_map(move |&b| part.shared_edges(a, b))
        })
        .collect();
    if shared.is_empty() {
        return false;
    }
    let mut intervals = Vec::new();
    for edge in shared {
        let Some((span, direction)) = line(part, edge) else {
            return false;
        };
        if (0..3).any(|axis| axis != run && direction[axis].abs() > SMOOTH_ARC_GAP) {
            return false;
        }
        intervals.push(span[run]);
    }
    covers_run(intervals, low, high)
}

/// A face region in the plane closing the run at *station*, meeting both walls and convex to
/// each (`_common_terminal`).
fn common_terminal(
    part: &Part,
    left: &Region,
    right: &Region,
    run: usize,
    station: f64,
    planes: &Planes,
) -> bool {
    let around = |r: &Region| -> BTreeSet<usize> {
        r.nodes.iter().flat_map(|&f| part.neighbours(f)).collect()
    };
    let (left_neighbours, right_neighbours) = (around(left), around(right));
    let mut seen = BTreeSet::new();
    for &seed in left_neighbours.union(&right_neighbours) {
        if seen.contains(&seed) {
            continue;
        }
        let region = coplanar_region(part, seed, planes);
        seen.extend(region.iter().copied());
        match planes[seed] {
            Some((axis, coordinate)) if axis == run && (coordinate - station).abs() <= SPAN_EPS => {
            }
            _ => continue,
        }
        if region.is_disjoint(&left_neighbours) || region.is_disjoint(&right_neighbours) {
            continue;
        }
        let arcs = |r: &Region| -> Vec<Option<Arc>> {
            r.nodes
                .iter()
                .flat_map(|&source| {
                    part.neighbours(source)
                        .into_iter()
                        .filter(|n| region.contains(n))
                        .map(move |n| part.arc(source, n))
                })
                .collect()
        };
        let (left_arcs, right_arcs) = (arcs(left), arcs(right));
        if !left_arcs.is_empty()
            && !right_arcs.is_empty()
            && left_arcs
                .iter()
                .chain(&right_arcs)
                .all(|k| *k == Some(Arc::Convex))
        {
            return true;
        }
    }
    false
}

/// Whether a leg that stops short of the solid's envelope ends at a convex straight edge of the
/// same solid along the whole run (`_convex_local_boundary`).
#[allow(clippy::too_many_arguments)]
pub(super) fn convex_local_boundary(
    part: &Part,
    region: &Region,
    varying: usize,
    endpoint: f64,
    run: usize,
    low: f64,
    high: f64,
    solid_nodes: &BTreeSet<usize>,
) -> bool {
    let mut intervals = Vec::new();
    for &source in &region.nodes {
        for neighbour in part.neighbours(source) {
            if !solid_nodes.contains(&neighbour) || part.arc(source, neighbour) != Some(Arc::Convex)
            {
                continue;
            }
            for edge in part.shared_edges(source, neighbour) {
                let Some((axes, _)) = line(part, edge) else {
                    continue;
                };
                let n = region.normal_axis;
                if (axes[varying].0 - endpoint).abs() > SPAN_EPS
                    || (axes[varying].1 - endpoint).abs() > SPAN_EPS
                    || (axes[n].0 - region.coordinate).abs() > SPAN_EPS
                    || (axes[n].1 - region.coordinate).abs() > SPAN_EPS
                {
                    continue;
                }
                intervals.push(axes[run]);
            }
        }
    }
    !intervals.is_empty() && covers_run(intervals, low, high)
}

type Section = [[f64; 2]; 3];

/// The open section, the removed prism's spans and each endpoint's scope
/// (`_section_and_spans`).
pub(super) fn section_and_spans(
    part: &Part,
    left: &Region,
    right: &Region,
    run: usize,
    solid_bounds: &[(f64, f64); 3],
    solid_nodes: &BTreeSet<usize>,
) -> Option<(Section, Spans, [EndpointScope; 2])> {
    let section_axes: Vec<usize> = (0..3).filter(|&i| i != run).collect();
    let (a, b) = (section_axes[0], section_axes[1]);
    let normals = BTreeSet::from([left.normal_axis, right.normal_axis]);
    if normals != BTreeSet::from([a, b]) {
        return None;
    }
    let mut corner = [0.0; 3];
    corner[left.normal_axis] = left.coordinate;
    corner[right.normal_axis] = right.coordinate;
    let mut endpoint = [0.0; 3];
    let mut scope: [EndpointScope; 3] = ["", "", ""];
    let (low_run, high_run) = solid_bounds[run];
    for (region, varying) in [(left, right.normal_axis), (right, left.normal_axis)] {
        let (low, high) = region.bounds[varying];
        let at = corner[varying];
        let (value, envelope) = if (low - at).abs() <= SPAN_EPS && high - at > SPAN_EPS {
            (high, solid_bounds[varying].1)
        } else if (high - at).abs() <= SPAN_EPS && at - low > SPAN_EPS {
            (low, solid_bounds[varying].0)
        } else {
            return None;
        };
        endpoint[varying] = value;
        scope[varying] = if (value - envelope).abs() <= SPAN_EPS {
            "solid"
        } else if convex_local_boundary(
            part,
            region,
            varying,
            value,
            run,
            low_run,
            high_run,
            solid_nodes,
        ) {
            "local"
        } else {
            return None;
        };
    }
    let mut points = [
        if left.normal_axis == a {
            [corner[a], endpoint[b]]
        } else {
            [endpoint[a], corner[b]]
        },
        [corner[a], corner[b]],
        if right.normal_axis == b {
            [endpoint[a], corner[b]]
        } else {
            [corner[a], endpoint[b]]
        },
    ];
    let mut scopes = if left.normal_axis == a {
        [scope[b], scope[a]]
    } else {
        [scope[a], scope[b]]
    };
    // Two local ends describe an internal staircase or groove, not an open step at the edge.
    if scopes == ["local", "local"] {
        return None;
    }
    let reversed = [points[2], points[1], points[0]];
    if py::tuple_order(&reversed.concat(), &points.concat()) == Ordering::Less {
        points = reversed;
        scopes = [scopes[1], scopes[0]];
    }
    let mut spans = [(0.0, 0.0); 3];
    spans[run] = left.bounds[run];
    for axis in [a, b] {
        spans[axis] = (
            corner[axis].min(endpoint[axis]),
            corner[axis].max(endpoint[axis]),
        );
    }
    Some((points, spans, scopes))
}

/// The through steps of one solid (`_recognise_one`).
fn recognise_one(
    ctx: &Context<'_>,
    solid: usize,
    planes: &Planes,
    body_key: &Option<BodyKey>,
) -> Vec<Occurrence<ThroughStep>> {
    let part = ctx.part;
    let solid_nodes: BTreeSet<usize> = part.solids[solid].faces.iter().copied().collect();
    let regions = regions(part, &solid_nodes, planes);
    let sb = part.solid_bounds(solid);
    let solid_bounds = [0, 1, 2].map(|i| (sb.min[i], sb.max[i]));
    let near = |x: f64, y: f64| (x - y).abs() <= SPAN_EPS;
    let mut out = Vec::new();
    let mut claimed: Vec<Vec<usize>> = Vec::new();
    for (index, left) in regions.iter().enumerate() {
        for (offset, right) in regions[index + 1..].iter().enumerate() {
            if left.normal_axis == right.normal_axis {
                continue;
            }
            let run = 3 - left.normal_axis - right.normal_axis;
            let (low, high) = left.bounds[run];
            if !near(low, right.bounds[run].0)
                || !near(high, right.bounds[run].1)
                || !near(low, solid_bounds[run].0)
                || !near(high, solid_bounds[run].1)
                || relation(part, left, right) != Some(Arc::Concave)
            {
                continue;
            }
            let Some((section, spans, endpoint_scopes)) =
                section_and_spans(part, left, right, run, &solid_bounds, &solid_nodes)
            else {
                continue;
            };
            if !shared_run_is_complete(part, left, right, run, low, high)
                || !common_terminal(part, left, right, run, low, planes)
                || !common_terminal(part, left, right, run, high, planes)
            {
                continue;
            }
            let pair = [index, index + 1 + offset];
            let another_concave_wall = regions.iter().enumerate().any(|(c, candidate)| {
                !pair.contains(&c)
                    && near(candidate.bounds[run].0, low)
                    && near(candidate.bounds[run].1, high)
                    && (relation(part, left, candidate) == Some(Arc::Concave)
                        || relation(part, right, candidate) == Some(Arc::Concave))
            });
            if another_concave_wall {
                continue;
            }
            if !prism_is_empty(ctx, solid, &spans, COORD_FLOOR) {
                continue;
            }
            let mut nodes: Vec<usize> = left.nodes.iter().chain(&right.nodes).copied().collect();
            nodes.sort_unstable();
            nodes.dedup();
            if claimed.contains(&nodes) || !part.solid_is_valid(solid) {
                continue;
            }
            claimed.push(nodes.clone());
            let at = Placed::new(
                [0, 1, 2].map(|i| (spans[i].0 + spans[i].1) / 2.0),
                Grid::Decimals(3),
            );
            out.push(Occurrence {
                record: ThroughStep {
                    axis: AXES[run],
                    length: py::round_to3(high - low),
                    at,
                    section: section.map(|p| Placed::new(p, Grid::Decimals(3))),
                    body_key: body_key.clone(),
                    endpoint_scopes,
                },
                defining: nodes,
                context: Vec::new(),
            });
        }
    }
    out
}

internal_entry! {
/// Every through step, ordered as Python sorts the records. Python raises when a step's faces
/// share no valid solid; the port leaves such a step out on both paths (as thin walls and
/// interior voids do), so the evidence path does not refuse either.
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<ThroughStep>> {
    let part = ctx.part;
    let planes: Planes = (0..part.faces.len())
        .map(|f| axis_aligned_axis(part, f))
        .collect();
    let keys = ctx.body_keys(true);
    let mut out: Vec<Occurrence<ThroughStep>> = keys
        .iter()
        .enumerate()
        .flat_map(|(solid, key)| recognise_one(ctx, solid, &planes, key))
        .collect();
    out.sort_by(|a, b| a.record.order(&b.record));
    out
}
}
