//! Rectangular step remnants with at least one run end inside the solid's envelope.
//! Both ends must be unique perpendicular exterior faces, convex to both complete walls.
//! Concave closing walls belong to blind recesses; a shortened seam alone proves nothing.

use std::collections::BTreeSet;

use serde::Serialize;

use super::Context;
use super::evidence::{self, EvidenceError, Occurrence};
use super::placed::{Grid, Placed};
use super::planes::axis_aligned_axis;
use super::through_steps::{
    Planes, Region, convex_local_boundary, coplanar_region, covers_run, line, regions, relation,
    section_and_spans, shared_run_is_complete,
};
use super::volume_probe::prism_is_empty;
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::{self, COORD_FLOOR, V3};
use crate::kernel::py;

/// A perpendicular exterior boundary of the run, consulted rather than claimed.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LocalStepEnd {
    /// `solid` at the envelope, `local` at an interior station.
    pub scope: &'static str,
    pub faces: Vec<usize>,
}

/// Lengths are millimetres. Endpoints lie on the concave seam; the two transverse
/// directions point from it towards the convex lips. Height is the first leg of the
/// canonical section, depth the second, with no implication of world gravity.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LocalStep {
    pub run_direction: V3,
    pub run_start: Placed<V3>,
    pub run_end: Placed<V3>,
    pub length: f64,
    pub height_direction: V3,
    pub height: f64,
    pub depth_direction: V3,
    pub depth: f64,
    /// In run_start, run_end order; these faces are evidence, not defining faces.
    pub ends: [LocalStepEnd; 2],
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= COORD_FLOOR
}

/// Each external edge lies on one of the four sides and every side is covered.
/// This admits smooth subdivisions, but no holes, tapered ends or interrupted walls.
fn rectangular(part: &Part, region: &Region, run: usize) -> bool {
    let across = 3 - region.normal_axis - run;
    let mut sides: [Vec<(f64, f64)>; 4] = Default::default();
    for &face in &region.nodes {
        for edge in part.face_edges(face) {
            if region
                .nodes
                .iter()
                .filter(|&&f| part.face_edges(f).contains(&edge))
                .count()
                == 2
            {
                continue;
            }
            let Some((span, _)) = line(part, edge) else {
                return false;
            };
            let mut on_side = false;
            for (i, fixed, varying) in [(0, run, across), (2, across, run)] {
                for (j, at) in [region.bounds[fixed].0, region.bounds[fixed].1]
                    .into_iter()
                    .enumerate()
                {
                    if near(span[fixed].0, at) && near(span[fixed].1, at) {
                        sides[i + j].push(span[varying]);
                        on_side = true;
                    }
                }
            }
            if !on_side {
                return false;
            }
        }
    }
    sides.into_iter().enumerate().all(|(i, spans)| {
        let (lo, hi) = region.bounds[if i < 2 { across } else { run }];
        covers_run(spans, lo, hi)
    })
}

/// Both entire end legs must meet the same single coplanar region convexly. A
/// different support per leg, a curved wall, gap or competing region is ambiguous.
fn terminal(
    part: &Part,
    walls: [&Region; 2],
    run: usize,
    station: f64,
    planes: &Planes,
    solid_nodes: &BTreeSet<usize>,
) -> Option<Vec<usize>> {
    let mut supports = BTreeSet::new();
    for wall in walls {
        let across = 3 - run - wall.normal_axis;
        let mut spans = Vec::new();
        for &face in &wall.nodes {
            for edge in part.face_edges(face) {
                let (span, _) = line(part, edge)?;
                if !near(span[run].0, station) || !near(span[run].1, station) {
                    continue;
                }
                let neighbours: Vec<_> = part
                    .neighbours(face)
                    .into_iter()
                    .filter(|&n| part.shared_edges(face, n).contains(&edge))
                    .collect();
                let [other] = neighbours[..] else {
                    return None;
                };
                if !solid_nodes.contains(&other)
                    || part.arc(face, other) != Some(Arc::Convex)
                    || !planes[other].is_some_and(|(axis, at)| axis == run && near(at, station))
                {
                    return None;
                }
                supports.insert(other);
                spans.push(span[across]);
            }
        }
        if !covers_run(spans, wall.bounds[across].0, wall.bounds[across].1) {
            return None;
        }
    }
    let region = coplanar_region(part, *supports.first()?, planes);
    (supports.is_subset(&region) && region.is_subset(solid_nodes))
        .then(|| region.into_iter().collect())
}

internal_entry! {
pub fn discover_verified(ctx: &Context<'_>) -> Result<Vec<Occurrence<LocalStep>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx))
}
}

internal_entry! {
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<LocalStep>> {
    discover_candidates(ctx)
}
}

fn discover_candidates(ctx: &Context<'_>) -> Vec<Occurrence<LocalStep>> {
    let part = ctx.part;
    let planes: Planes = (0..part.faces.len())
        .map(|f| axis_aligned_axis(part, f))
        .collect();
    let mut out = Vec::new();
    for (solid, body) in part.solids.iter().enumerate() {
        if !part.solid_is_geometrically_valid(solid) {
            continue;
        }
        let nodes: BTreeSet<_> = body.faces.iter().copied().collect();
        let regions = regions(part, &nodes, &planes);
        let b = part.solid_bounds(solid);
        let solid_bounds = [0, 1, 2].map(|i| (b.min[i], b.max[i]));
        for (i, left) in regions.iter().enumerate() {
            for right in &regions[i + 1..] {
                if left.normal_axis == right.normal_axis
                    || relation(part, left, right) != Some(Arc::Concave)
                {
                    continue;
                }
                let run = 3 - left.normal_axis - right.normal_axis;
                let (low, high) = left.bounds[run];
                if high - low <= 2. * COORD_FLOOR
                    || !near(low, right.bounds[run].0)
                    || !near(high, right.bounds[run].1)
                    || (near(low, solid_bounds[run].0) && near(high, solid_bounds[run].1))
                    || !rectangular(part, left, run)
                    || !rectangular(part, right, run)
                    || !shared_run_is_complete(part, left, right, run, low, high)
                {
                    continue;
                }
                // Keep the transverse envelope and endpoint-scope rules of through_steps,
                // measuring their local lip coverage only over this candidate's run.
                let mut local_bounds = solid_bounds;
                local_bounds[run] = (low, high);
                let Some((section, spans, _)) =
                    section_and_spans(part, left, right, run, &local_bounds, &nodes)
                else {
                    continue;
                };
                let axes: Vec<_> = (0..3).filter(|&k| k != run).collect();
                let corner = [0, 1, 2].map(|k| {
                    if k == run {
                        low
                    } else if k == left.normal_axis {
                        left.coordinate
                    } else {
                        right.coordinate
                    }
                });
                let lips_proved = [(left, right.normal_axis), (right, left.normal_axis)]
                    .into_iter()
                    .all(|(wall, varying)| {
                        let (lo, hi) = wall.bounds[varying];
                        let lip = if near(corner[varying], lo) { hi } else { lo };
                        convex_local_boundary(part, wall, varying, lip, run, low, high, &nodes)
                    });
                if !lips_proved {
                    continue;
                }
                let (Some(start_faces), Some(end_faces)) = (
                    terminal(part, [left, right], run, low, &planes, &nodes),
                    terminal(part, [left, right], run, high, &planes, &nodes),
                ) else {
                    continue;
                };
                if !prism_is_empty(ctx, solid, &spans, COORD_FLOOR) {
                    continue;
                }
                let vector = |p: [f64; 2]| {
                    let mut v = [0.; 3];
                    for (j, &k) in axes.iter().enumerate() {
                        v[k] = p[j] - corner[k];
                    }
                    v
                };
                let height = vector(section[0]);
                let depth = vector(section[2]);
                let mut end = corner;
                end[run] = high;
                let mut defining: Vec<_> = left.nodes.iter().chain(&right.nodes).copied().collect();
                defining.sort_unstable();
                // The whole solid was consulted by the empty-volume proof.
                let context = nodes
                    .iter()
                    .filter(|f| !defining.contains(f))
                    .copied()
                    .collect();
                out.push(Occurrence {
                    record: LocalStep {
                        run_direction: [0, 1, 2].map(|k| if k == run { 1. } else { 0. }),
                        run_start: Placed::new(corner, Grid::Decimals(3)),
                        run_end: Placed::new(end, Grid::Decimals(3)),
                        length: py::round_to3(high - low),
                        height_direction: py::unit(height).expect("positive section leg"),
                        height: py::round_to3(geom::norm(height)),
                        depth_direction: py::unit(depth).expect("positive section leg"),
                        depth: py::round_to3(geom::norm(depth)),
                        ends: [
                            LocalStepEnd {
                                scope: if near(low, solid_bounds[run].0) {
                                    "solid"
                                } else {
                                    "local"
                                },
                                faces: start_faces,
                            },
                            LocalStepEnd {
                                scope: if near(high, solid_bounds[run].1) {
                                    "solid"
                                } else {
                                    "local"
                                },
                                faces: end_faces,
                            },
                        ],
                    },
                    defining,
                    context,
                });
            }
        }
    }
    let key = |r: &LocalStep| {
        [
            r.run_direction.as_slice(),
            &[r.length, r.height, r.depth],
            &r.run_start[..],
            &r.run_end[..],
            &r.height_direction,
            &r.depth_direction,
        ]
        .concat()
    };
    out.sort_by(|a, b| py::tuple_order(&key(&a.record), &key(&b.record)));
    out
}
