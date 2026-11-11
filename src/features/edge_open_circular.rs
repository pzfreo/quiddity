//! Blind circular-ended recesses whose section has one interrupted end
//! (`quiddity.edge_open_circular_recesses`).
//!
//! A planar floor across a principal axis meets, concavely, a chain of four walls joined
//! smoothly: cylinder, plane, cylinder, plane in turn. The walls share the floor's span along
//! the axis, one face caps their far end (the mouth), and the floor is proved by volume probes.
//! The section is the chain of the floor's edges with the walls: two lines and two arcs of one
//! radius, exactly one of them an intact semicircle, the other cut short by the stock's edge.

use std::cmp::Ordering;
use std::f64::consts::{PI, TAU};

use serde::Serialize;

use super::Context;
use super::edge_open::{SPAN_EPS, dist2, floor_proof, mouths, principal_plane};
use super::evidence::{self, EvidenceError, Occurrence, common_valid_solid};
use super::graph::{is_planar, normal, ordered_chain, shared_occurrences, span};
use super::placed::{Grid, Placed};
use super::policy::AXIS_ZERO_COS;
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::{Curve, Surface, V3};
use crate::kernel::py;

const AXES: [char; 3] = ['x', 'y', 'z'];
const POINT_DIGITS: usize = 4;
const ANGLE_DIGITS: usize = 7;
const EPS: f64 = 1e-9;

type Point = [f64; 2];

/// One physical line or circular arc of an open section chain.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct OpenCircularSectionSegment {
    pub kind: &'static str,
    pub start: Placed<Point>,
    pub end: Placed<Point>,
    pub center: Option<Placed<Point>>,
    pub radius: Option<f64>,
    pub sweep: Option<f64>,
}

/// The alternating wall chain and the gap between its loose ends.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct OpenCircularSection {
    pub segments: Vec<OpenCircularSectionSegment>,
    pub opening: [Placed<Point>; 2],
}

/// A blind constant-depth recess with an interrupted circular-end profile. `run_interval` is
/// floor to mouth along `axis`, and `open_sign` the side of the mouth.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct EdgeOpenCircularPocket {
    pub axis: char,
    pub run_interval: Placed<[f64; 2]>,
    pub open_sign: i32,
    pub section: OpenCircularSection,
}

fn point_order(a: &Point, b: &Point) -> Ordering {
    py::tuple_order(a, b)
}

/// `None < Some` never arises: two segments of one kind carry values alike, and the kinds differ
/// first otherwise.
fn option_order<T>(a: &Option<T>, b: &Option<T>, cmp: impl Fn(&T, &T) -> Ordering) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => cmp(x, y),
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
    }
}

impl OpenCircularSectionSegment {
    fn reversed(&self) -> Self {
        OpenCircularSectionSegment {
            kind: self.kind,
            start: self.end,
            end: self.start,
            center: self.center,
            radius: self.radius,
            sweep: self.sweep.map(|s| -s),
        }
    }

    /// The dataclass's field-tuple order.
    fn order(&self, other: &Self) -> Ordering {
        self.kind
            .cmp(other.kind)
            .then_with(|| point_order(&self.start, &other.start))
            .then_with(|| point_order(&self.end, &other.end))
            .then_with(|| option_order(&self.center, &other.center, |a, b| point_order(a, b)))
            .then_with(|| option_order(&self.radius, &other.radius, |a, b| py::order(*a, *b)))
            .then_with(|| option_order(&self.sweep, &other.sweep, |a, b| py::order(*a, *b)))
    }

    fn line(start: Placed<Point>, end: Placed<Point>) -> Option<Self> {
        (dist2(*start, *end) > EPS).then_some(OpenCircularSectionSegment {
            kind: "line",
            start,
            end,
            center: None,
            radius: None,
            sweep: None,
        })
    }

    /// An arc, or `None` where the dataclass refuses one (`ValueError`).
    fn arc(
        start: Placed<Point>,
        end: Placed<Point>,
        center: Placed<Point>,
        radius: f64,
        sweep: f64,
    ) -> Option<Self> {
        let (placed, center) = ((start, end, center), *center);
        let (start, end) = (*start, *end);
        if dist2(start, end) <= EPS
            || radius <= 0.0
            || sweep.abs() <= EPS
            || sweep.abs() > PI + 1e-6
        {
            return None;
        }
        // The ADR-0008 reconstruction allowance, independent of discovery tolerances.
        let radial_tol = 0.002;
        if [start, end]
            .iter()
            .any(|p| (dist2(*p, center) - radius).abs() > radial_tol)
        {
            return None;
        }
        let v = [start[0] - center[0], start[1] - center[1]];
        let (sine, cosine) = sweep.sin_cos();
        let swept = [
            center[0] + v[0] * cosine - v[1] * sine,
            center[1] + v[0] * sine + v[1] * cosine,
        ];
        (dist2(swept, end) <= radial_tol).then_some(OpenCircularSectionSegment {
            kind: "arc",
            start: placed.0,
            end: placed.1,
            center: Some(placed.2),
            radius: Some(radius),
            sweep: Some(sweep),
        })
    }
}

fn chain_order(a: &[OpenCircularSectionSegment], b: &[OpenCircularSectionSegment]) -> Ordering {
    a.iter()
        .zip(b)
        .map(|(x, y)| x.order(y))
        .find(|o| o.is_ne())
        .unwrap_or_else(|| a.len().cmp(&b.len()))
}

impl EdgeOpenCircularPocket {
    /// The dataclass's field-tuple order.
    fn order(&self, other: &Self) -> Ordering {
        self.axis
            .cmp(&other.axis)
            .then_with(|| py::tuple_order(&self.run_interval[..], &other.run_interval[..]))
            .then(self.open_sign.cmp(&other.open_sign))
            .then_with(|| chain_order(&self.section.segments, &other.section.segments))
            .then_with(|| {
                let flat = |o: &[Placed<Point>; 2]| [o[0][0], o[0][1], o[1][0], o[1][1]];
                py::tuple_order(&flat(&self.section.opening), &flat(&other.section.opening))
            })
    }
}

fn project(p: V3, axis: usize) -> Point {
    let others: Vec<usize> = (0..3).filter(|&a| a != axis).collect();
    [p[others[0]], p[others[1]]]
}

fn rounded(p: Point) -> Placed<Point> {
    Placed::new(p, Grid::CleanDecimals(POINT_DIGITS as u8))
}

/// The signed sweep from *start* to *end* about the arc's centre that passes its midpoint
/// (`_arc_sweep`).
fn arc_sweep(centre: Point, middle: Point, start: Point, end: Point) -> f64 {
    let angle = |p: Point| (p[1] - centre[1]).atan2(p[0] - centre[0]);
    let (first, last, mid) = (angle(start), angle(end), angle(middle));
    let positive = py::modulo(last - first, TAU);
    let contains_mid = py::modulo(mid - first, TAU) <= positive + 1e-9;
    if contains_mid {
        positive
    } else {
        positive - TAU
    }
}

/// The section segment the floor shares with one wall (`_segment`).
fn segment(
    part: &Part,
    floor: usize,
    wall: usize,
    axis: usize,
) -> Option<OpenCircularSectionSegment> {
    let shared = shared_occurrences(part, floor, wall);
    let &[edge] = shared.as_slice() else {
        return None;
    };
    let e = &part.edges[edge];
    if e.is_closed() {
        return None;
    }
    // `position_at(0)` and `position_at(1)`: which end is which does not survive
    // `orient_segments`, which tries both and keeps the canonical direction.
    let start = rounded(project(e.start, axis));
    let end = rounded(project(e.end, axis));
    match &e.curve {
        Curve::Line { .. } => OpenCircularSectionSegment::line(start, end),
        Curve::Circle { frame, radius } => {
            let centre = project(frame.origin, axis);
            let sweep = arc_sweep(centre, project(e.midpoint(), axis), *start, *end);
            OpenCircularSectionSegment::arc(
                start,
                end,
                rounded(centre),
                py::round_to(*radius, POINT_DIGITS),
                py::round_to(sweep, ANGLE_DIGITS),
            )
        }
        _ => None,
    }
}

/// The segments joined end to start into one chain, in the canonical of its two directions
/// (`_orient_segments`).
fn orient_segments(
    segments: &[OpenCircularSectionSegment],
) -> Option<Vec<OpenCircularSectionSegment>> {
    for (first_index, first) in segments.iter().enumerate() {
        for candidate in [first.clone(), first.reversed()] {
            let mut ordered = vec![candidate];
            let mut unused: Vec<OpenCircularSectionSegment> = segments
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != first_index)
                .map(|(_, s)| s.clone())
                .collect();
            while !unused.is_empty() {
                let last = *ordered.last().expect("started").end;
                let mut matches = Vec::new();
                for (index, item) in unused.iter().enumerate() {
                    for oriented in [item.clone(), item.reversed()] {
                        if dist2(last, *oriented.start) <= 2e-4 {
                            matches.push((index, oriented));
                        }
                    }
                }
                if matches.len() != 1 {
                    break;
                }
                let (index, item) = matches.pop().expect("one match");
                ordered.push(item);
                unused.remove(index);
            }
            if unused.is_empty() {
                let reverse: Vec<_> = ordered.iter().rev().map(|s| s.reversed()).collect();
                return Some(if chain_order(&reverse, &ordered).is_lt() {
                    reverse
                } else {
                    ordered
                });
            }
        }
    }
    None
}

/// Whether the section is one the record admits (`OpenCircularSection.__post_init__`): Python
/// raises rather than declines where these fail after the recogniser's own checks.
fn valid_section(segments: &[OpenCircularSectionSegment]) -> bool {
    let kinds: Vec<&str> = segments.iter().map(|s| s.kind).collect();
    segments.len() == 4
        && (kinds == ["arc", "line", "arc", "line"] || kinds == ["line", "arc", "line", "arc"])
        && segments.windows(2).all(|w| w[0].end == w[1].start)
}

internal_entry! {
/// `recognise_edge_open_circular_pockets`.
pub fn recognise_edge_open_circular_pockets(part: &Part) -> Vec<EdgeOpenCircularPocket> {
    super::records(discover(&Context::new(part)))
}
}

internal_entry! {
/// The evidence path: each pocket defined by its four walls, its floor consulted.
pub fn discover_verified(
    ctx: &Context<'_>,
) -> Result<Vec<Occurrence<EdgeOpenCircularPocket>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx))
}
}

internal_entry! {
/// Every pocket, in record order.
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<EdgeOpenCircularPocket>> {
    let part = ctx.part;
    let mut found: Vec<Occurrence<EdgeOpenCircularPocket>> = (0..part.faces.len())
        .filter_map(|floor| one_floor(ctx, floor))
        .collect();
    found.sort_by(|a, b| a.record.order(&b.record));
    found
}
}

fn one_floor(ctx: &Context<'_>, floor: usize) -> Option<Occurrence<EdgeOpenCircularPocket>> {
    let part = ctx.part;
    if !is_planar(part, floor) {
        return None;
    }
    let (axis, floor_at) = principal_plane(part, floor)?;
    let neighbours = part.neighbours(floor);
    let concave = |n: usize| part.arc(floor, n) == Some(Arc::Concave);
    let cylinders: Vec<usize> = neighbours
        .iter()
        .copied()
        .filter(|&n| matches!(part.faces[n].surface, Surface::Cylinder { .. }) && concave(n))
        .collect();
    let walls: Vec<usize> = neighbours
        .iter()
        .copied()
        .filter(|&n| {
            is_planar(part, n)
                && concave(n)
                && normal(part, n).is_some_and(|v| v[axis].abs() <= AXIS_ZERO_COS)
        })
        .collect();
    let boundary: Vec<usize> = cylinders.iter().chain(&walls).copied().collect();
    if cylinders.len() != 2 || walls.len() != 2 {
        return None;
    }
    // `_ordered_chain`: the boundary faces joined by smooth arcs.
    let ordered = ordered_chain(part, &boundary, |a, b| part.arc(a, b) == Some(Arc::Smooth))?;
    let plane = |n: usize| is_planar(part, n);
    let alternating = (0..4).all(|i| plane(ordered[i]) == (i % 2 == 1))
        || (0..4).all(|i| plane(ordered[i]) == (i % 2 == 0));
    if !alternating {
        return None;
    }
    let spans: Vec<(f64, f64)> = boundary.iter().map(|&n| span(part, n, axis)).collect();
    let low = spans.iter().map(|s| s.0).fold(f64::INFINITY, f64::min);
    let high = spans.iter().map(|s| s.1).fold(f64::NEG_INFINITY, f64::max);
    if high - low <= SPAN_EPS
        || spans
            .iter()
            .any(|&(a, b)| (a - low).abs() > SPAN_EPS || (b - high).abs() > SPAN_EPS)
    {
        return None;
    }
    if (floor_at - low).abs().min((floor_at - high).abs()) > SPAN_EPS {
        return None;
    }
    let mouth_at = if (floor_at - low).abs() <= SPAN_EPS {
        high
    } else {
        low
    };
    let mut members = boundary.clone();
    members.push(floor);
    let owner = common_valid_solid(part, &members);
    if mouths(part, &boundary, axis, mouth_at).len() != 1 {
        return None;
    }
    let owner = owner?;
    let raw: Vec<OpenCircularSectionSegment> = ordered
        .iter()
        .map(|&wall| segment(part, floor, wall, axis))
        .collect::<Option<_>>()?;
    let segments = orient_segments(&raw)?;
    let arcs: Vec<&OpenCircularSectionSegment> =
        segments.iter().filter(|s| s.kind == "arc").collect();
    let sweep = |s: &OpenCircularSectionSegment| s.sweep.unwrap_or(0.0).abs();
    if arcs.len() != 2
        || arcs[0].radius.is_none()
        || arcs[0].radius != arcs[1].radius
        || arcs
            .iter()
            .filter(|s| (sweep(s) - PI).abs() <= 1e-4)
            .count()
            != 1
        || !arcs.iter().all(|s| 0.0 < sweep(s) && sweep(s) <= PI + 1e-6)
    {
        return None;
    }
    if !floor_proof(ctx, owner, floor, axis, floor_at, mouth_at) {
        return None;
    }
    // Python raises on a chain whose rounded joints do not meet exactly; the port declines it.
    if !valid_section(&segments) {
        return None;
    }
    let opening = [segments[3].end, segments[0].start];
    Some(Occurrence {
        record: EdgeOpenCircularPocket {
            axis: AXES[axis],
            run_interval: Placed::new(
                [floor_at.min(mouth_at), floor_at.max(mouth_at)],
                Grid::Decimals(3),
            ),
            open_sign: if mouth_at > floor_at { 1 } else { -1 },
            section: OpenCircularSection { segments, opening },
        },
        defining: ordered,
        context: vec![floor],
    })
}
