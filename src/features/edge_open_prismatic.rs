//! Blind prismatic recesses with one side open to the stock's exterior
//! (`quiddity.edge_open_prismatic_recesses`).
//!
//! A planar floor across a principal axis meets, concavely, an open chain of at least three
//! planar walls parallel to the axis; everything else round the floor turns convex (the
//! opening). The walls rise from the floor to one common mouth face, carry no holes or branches,
//! and the first and last are not parallel. The floor is proved by volume probes. The section is
//! the walls' chain of corners on the floor, with the opening between its loose ends.

use std::cmp::Ordering;

use serde::Serialize;

use super::Context;
use super::edge_open::{SPAN_EPS, dist2, floor_proof, mouths, principal_plane};
use super::evidence::{self, EvidenceError, Occurrence, common_valid_solid};
use super::graph::{is_planar, normal, ordered_chain, shared_occurrences, span};
use super::placed::{Grid, Placed};
use super::policy::AXIS_ZERO_COS;
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::Curve;
use crate::kernel::py;

const AXES: [char; 3] = ['x', 'y', 'z'];
const EPS: f64 = 1e-9;

type Point = [f64; 2];

/// The physical endpoints of the absent wall, with no implied segment between them.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct OpenSectionOpening {
    pub start: Placed<Point>,
    pub end: Placed<Point>,
}

/// The canonical wall chain and its opening side.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct OpenPolygonalSection {
    pub wall_chain: Vec<Placed<Point>>,
    pub opening: OpenSectionOpening,
}

impl OpenPolygonalSection {
    /// The wall chain's published corners.
    pub fn wall_points(&self) -> Vec<Point> {
        self.wall_chain.iter().map(|p| **p).collect()
    }
}

/// A blind prismatic recess with one physical side open. `run_interval` is floor to mouth
/// along `axis`, and `open_sign` the side of the mouth.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct EdgeOpenPrismaticRecess {
    pub axis: char,
    pub run_interval: Placed<[f64; 2]>,
    pub open_sign: i32,
    pub section: OpenPolygonalSection,
}

fn chain_order(a: &[Point], b: &[Point]) -> Ordering {
    a.iter()
        .zip(b)
        .map(|(x, y)| py::tuple_order(x, y))
        .find(|o| o.is_ne())
        .unwrap_or_else(|| a.len().cmp(&b.len()))
}

impl EdgeOpenPrismaticRecess {
    /// The dataclass's field-tuple order.
    fn order(&self, other: &Self) -> Ordering {
        let (s, o) = (&self.section, &other.section);
        self.axis
            .cmp(&other.axis)
            .then_with(|| py::tuple_order(&self.run_interval[..], &other.run_interval[..]))
            .then(self.open_sign.cmp(&other.open_sign))
            .then_with(|| chain_order(&s.wall_points(), &o.wall_points()))
            .then_with(|| py::tuple_order(&s.opening.start[..], &o.opening.start[..]))
            .then_with(|| py::tuple_order(&s.opening.end[..], &o.opening.end[..]))
    }
}

fn turn(a: Point, b: Point, c: Point) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// Whether two segments touch or cross (`_crosses`).
fn crosses(first: (Point, Point), second: (Point, Point)) -> bool {
    let ((a, b), (c, d)) = (first, second);
    if a[0].max(b[0]) + EPS < c[0].min(d[0])
        || c[0].max(d[0]) + EPS < a[0].min(b[0])
        || a[1].max(b[1]) + EPS < c[1].min(d[1])
        || c[1].max(d[1]) + EPS < a[1].min(b[1])
    {
        return false;
    }
    turn(a, b, c) * turn(a, b, d) <= EPS && turn(c, d, a) * turn(c, d, b) <= EPS
}

/// Whether the chain is a section the record admits (`OpenPolygonalSection.__post_init__`).
fn valid_chain(chain: &[Point]) -> bool {
    if chain.len() < 4 || chain.windows(2).any(|w| dist2(w[0], w[1]) <= EPS) {
        return false;
    }
    let edges: Vec<(Point, Point)> = chain.windows(2).map(|w| (w[0], w[1])).collect();
    for (index, &edge) in edges.iter().enumerate() {
        for &other in edges.iter().skip(index + 2) {
            if crosses(edge, other) {
                return false;
            }
        }
    }
    dist2(chain[chain.len() - 1], chain[0]) > EPS
}

/// Every side wall has one boundary, meets the floor, the mouth and its chain neighbours once
/// each, and only the end walls meet anything else — exterior faces reached convexly
/// (`_complete_wall_boundaries`).
fn complete_wall_boundaries(
    part: &Part,
    floor: usize,
    walls: &[usize],
    mouth: usize,
    exterior: &[usize],
) -> bool {
    for (index, &wall) in walls.iter().enumerate() {
        if part.faces[wall].loops.len() != 1 {
            return false;
        }
        let mut required = vec![floor, mouth];
        if index > 0 {
            required.push(walls[index - 1]);
        }
        if index + 1 < walls.len() {
            required.push(walls[index + 1]);
        }
        let neighbours = part.neighbours(wall);
        if !required.iter().all(|r| neighbours.contains(r)) {
            return false;
        }
        // An occurrence, not merely a face: a second breakout can meet an end wall again
        // through the same exterior face.
        if neighbours
            .iter()
            .any(|&n| part.shared_edges(wall, n).len() != 1)
        {
            return false;
        }
        let extra: Vec<usize> = neighbours
            .iter()
            .copied()
            .filter(|n| !required.contains(n))
            .collect();
        if 0 < index && index < walls.len() - 1 {
            if !extra.is_empty() {
                return false;
            }
            continue;
        }
        if extra.is_empty()
            || !extra
                .iter()
                .all(|&n| part.arc(wall, n) == Some(Arc::Convex))
        {
            return false;
        }
        let mut pending: Vec<usize> = extra
            .iter()
            .copied()
            .filter(|n| exterior.contains(n))
            .collect();
        let mut reached = pending.clone();
        while let Some(current) = pending.pop() {
            let around = part.neighbours(current);
            for &node in &extra {
                if !reached.contains(&node)
                    && around.contains(&node)
                    && part.arc(current, node) == Some(Arc::Convex)
                {
                    reached.push(node);
                    pending.push(node);
                }
            }
        }
        if reached.len() != extra.len() {
            return false;
        }
    }
    true
}

/// The straight edge the floor shares with a wall, as its two vertices across *axis*
/// (`_shared_segment`).
fn shared_segment(part: &Part, floor: usize, wall: usize, axis: usize) -> Option<(Point, Point)> {
    let others: Vec<usize> = (0..3).filter(|&a| a != axis).collect();
    let across = |p: [f64; 3]| [p[others[0]], p[others[1]]];
    let found: Vec<(Point, Point)> = shared_occurrences(part, floor, wall)
        .into_iter()
        .filter_map(|e| {
            let edge = &part.edges[e];
            (matches!(edge.curve, Curve::Line { .. }) && !edge.is_closed())
                .then(|| (across(edge.start), across(edge.end)))
        })
        .collect();
    let &[segment] = found.as_slice() else {
        return None;
    };
    Some(segment)
}

/// The wall chain's corners on the floor, rounded and in canonical direction
/// (`_section_from_walls`).
fn section_from_walls(
    part: &Part,
    floor: usize,
    walls: &[usize],
    axis: usize,
) -> Option<OpenPolygonalSection> {
    let physical: Vec<(Point, Point)> = walls
        .iter()
        .map(|&w| shared_segment(part, floor, w, axis))
        .collect::<Option<_>>()?;
    let mut joins: Vec<Point> = Vec::new();
    for pair in physical.windows(2) {
        let (left, right) = (pair[0], pair[1]);
        let shared: Vec<Point> = [left.0, left.1]
            .into_iter()
            .flat_map(|a| {
                [right.0, right.1]
                    .into_iter()
                    .filter(move |b| dist2(a, *b) <= SPAN_EPS)
                    .map(move |_| a)
            })
            .collect();
        let &[join] = shared.as_slice() else {
            return None;
        };
        joins.push(join);
    }
    let away = |segment: (Point, Point), from: Point| {
        [segment.0, segment.1]
            .into_iter()
            .find(|p| dist2(*p, from) > SPAN_EPS)
    };
    let first = away(physical[0], joins[0])?;
    let last = away(physical[physical.len() - 1], joins[joins.len() - 1])?;
    let mut placed: Vec<Placed<Point>> = std::iter::once(first)
        .chain(joins)
        .chain(std::iter::once(last))
        .map(|p| Placed::new(p, Grid::CleanDecimals(4)))
        .collect();
    let mut chain: Vec<Point> = placed.iter().map(|p| **p).collect();
    let reversed: Vec<Point> = chain.iter().rev().copied().collect();
    if chain_order(&reversed, &chain).is_lt() {
        chain = reversed;
        placed.reverse();
    }
    if !valid_chain(&chain) {
        return None;
    }
    let opening = OpenSectionOpening {
        start: placed[placed.len() - 1],
        end: placed[0],
    };
    Some(OpenPolygonalSection {
        wall_chain: placed,
        opening,
    })
}

internal_entry! {
/// `recognise_edge_open_prismatic_recesses`.
pub fn recognise_edge_open_prismatic_recesses(part: &Part) -> Vec<EdgeOpenPrismaticRecess> {
    super::records(discover(&Context::new(part)))
}
}

internal_entry! {
/// The evidence path: each recess defined by its walls, its floor consulted.
pub fn discover_verified(
    ctx: &Context<'_>,
) -> Result<Vec<Occurrence<EdgeOpenPrismaticRecess>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx))
}
}

internal_entry! {
/// Every recess, in record order.
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<EdgeOpenPrismaticRecess>> {
    let mut found: Vec<Occurrence<EdgeOpenPrismaticRecess>> = (0..ctx.part.faces.len())
        .filter_map(|floor| one_floor(ctx, floor))
        .collect();
    found.sort_by(|a, b| a.record.order(&b.record));
    found
}
}

fn one_floor(ctx: &Context<'_>, floor: usize) -> Option<Occurrence<EdgeOpenPrismaticRecess>> {
    let part = ctx.part;
    if !is_planar(part, floor) {
        return None;
    }
    let (axis, floor_at) = principal_plane(part, floor)?;
    let neighbours = part.neighbours(floor);
    let mut walls: Vec<usize> = neighbours
        .iter()
        .copied()
        .filter(|&n| {
            is_planar(part, n)
                && part.arc(floor, n) == Some(Arc::Concave)
                && normal(part, n).is_some_and(|v| v[axis].abs() <= AXIS_ZERO_COS)
        })
        .collect();
    walls.sort_unstable();
    if walls.len() < 3 {
        return None;
    }
    // `_ordered_open_chain`: every neighbouring wall is a link.
    let ordered = ordered_chain(part, &walls, |_, _| true)?;
    let exterior: Vec<usize> = neighbours
        .iter()
        .copied()
        .filter(|n| !walls.contains(n))
        .collect();
    if part.faces[floor].loops.len() != 1
        || exterior.is_empty()
        || !exterior
            .iter()
            .all(|&n| part.arc(floor, n) == Some(Arc::Convex))
    {
        return None;
    }
    let mut far = Vec::with_capacity(ordered.len());
    for &wall in &ordered {
        let (low, high) = span(part, wall, axis);
        if (low - floor_at).abs() <= SPAN_EPS {
            far.push(high);
        } else if (high - floor_at).abs() <= SPAN_EPS {
            far.push(low);
        } else {
            return None;
        }
    }
    let far_low = far.iter().copied().fold(f64::INFINITY, f64::min);
    let far_high = far.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if far_high - far_low > SPAN_EPS || (far[0] - floor_at).abs() <= SPAN_EPS {
        return None;
    }
    let mouth_at = py::sum(far.iter().copied()) / far.len() as f64;
    let &[mouth] = mouths(part, &ordered, axis, mouth_at).as_slice() else {
        return None;
    };
    let (first_normal, last_normal) = (
        normal(part, ordered[0])?,
        normal(part, *ordered.last().expect("three walls"))?,
    );
    let alignment = py::sum((0..3).map(|i| first_normal[i] * last_normal[i])).abs();
    if py::isclose(alignment, 1.0, 0.0, 1e-9)
        || !complete_wall_boundaries(part, floor, &ordered, mouth, &exterior)
    {
        return None;
    }
    let mut members = ordered.clone();
    members.push(floor);
    let owner = common_valid_solid(part, &members)?;
    let section = section_from_walls(part, floor, &ordered, axis)?;
    if !floor_proof(ctx, owner, floor, axis, floor_at, mouth_at) {
        return None;
    }
    Some(Occurrence {
        record: EdgeOpenPrismaticRecess {
            axis: AXES[axis],
            run_interval: Placed::new(
                [floor_at.min(mouth_at), floor_at.max(mouth_at)],
                Grid::Decimals(3),
            ),
            open_sign: if mouth_at > floor_at { 1 } else { -1 },
            section,
        },
        defining: ordered,
        context: vec![floor],
    })
}
