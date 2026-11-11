//! Complete repeating radial boundary profiles (`quiddity.repeating_profiles`): the outer wire of
//! a solid's extremal principal plane face proved invariant under one sector rotation, every
//! edge in a bijective orbit, and the same sector inventory on the opposite extremal face.
//!
//! No gear semantics are attached: module, pressure angle and tooth form stay authored
//! requirements. Repeats below five, wires of circular arcs alone, and inner wires are not
//! profiles. Each edge is read as Python reads it: its curve kind, its length, and nine points
//! at equal fractions of its arc length (`Edge.position_at`), which the port measures on the
//! exact curve.
//!
//! Python's `local_degradation` path (an occurrence without one valid solid skipped rather than
//! refused) is not ported: the default inventory retries degraded on three corpus parts, and
//! this family's check of the flag is reached on none of them (`tests/local_degradation.rs`).

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

use super::Context;
use super::evidence::{EvidenceError, Occurrence, common_valid_solid};
use super::profiled_bores::principal_boundary_plane;
use super::solid_properties;
use crate::kernel::brep::Part;
use crate::kernel::geom::{self, Bounds, Curve, V3};
use crate::kernel::py;
use crate::kernel::sampling::{edge_interval, extremes_along};

const SAMPLES_PER_CURVE: usize = 9;
/// The only production consumer supports declarations with z >= 5; lower-order symmetry would
/// make ordinary prismatic stock enter this inventory.
const MIN_REPEAT_COUNT: usize = 5;
const AXES: [&str; 3] = ["x", "y", "z"];

/// One sector curve in a signature: kind, rounded length, and its canonical polar shape.
pub type SectorCurve = (String, f64, Vec<(f64, f64)>);

/// A complete physical profile proved invariant under one sector rotation.
/// `sector_signature` is the traversal- and phase-neutral inventory of one sector's curves.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct RepeatingRadialProfile {
    pub axis: String,
    pub centre: V3,
    pub span: [f64; 2],
    pub repeat_count: usize,
    pub edge_count: usize,
    pub sector_signature: Vec<SectorCurve>,
}

impl RepeatingRadialProfile {
    /// The dataclass order: every field in turn.
    fn order(&self, other: &Self) -> Ordering {
        self.axis
            .cmp(&other.axis)
            .then_with(|| py::tuple_order(&self.centre, &other.centre))
            .then_with(|| py::tuple_order(&self.span, &other.span))
            .then_with(|| self.repeat_count.cmp(&other.repeat_count))
            .then_with(|| self.edge_count.cmp(&other.edge_count))
            .then_with(|| {
                sequence_order(
                    &self.sector_signature,
                    &other.sector_signature,
                    sector_order,
                )
            })
    }
}

/// `recognise_repeating_radial_profiles`' keyword arguments.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct RepeatingRadialProfileOptions {
    pub tol: f64,
}

impl Default for RepeatingRadialProfileOptions {
    fn default() -> Self {
        RepeatingRadialProfileOptions { tol: 1e-5 }
    }
}

/// `_CurveEvidence`: one outer-wire edge as kind, length and sampled in-plane points.
#[derive(Clone, Debug)]
struct CurveEvidence {
    kind: &'static str,
    length: f64,
    points: Vec<(f64, f64)>,
}

/// `_BoundaryEvidence`: an extremal face whose outer wire repeats about `centre`.
struct BoundaryEvidence {
    face: usize,
    axis: usize,
    at: f64,
    plane_axes: [usize; 2],
    centre: (f64, f64),
    repeat_count: usize,
    edges: Vec<CurveEvidence>,
    orbits: Vec<Vec<usize>>,
}

internal_entry! {
/// `recognise_repeating_radial_profiles`.
pub fn recognise_repeating_radial_profiles(
    part: &Part,
    opts: &RepeatingRadialProfileOptions,
) -> Vec<RepeatingRadialProfile> {
    super::records(discover_with(part, opts.tol))
}
}

internal_entry! {
/// Every profile of every solid with its two opposed source faces, in record order.
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<RepeatingRadialProfile>> {
    discover_with(ctx.part, RepeatingRadialProfileOptions::default().tol)
}
}

internal_entry! {
/// The evidence path (`_discover_repeating_radial_profiles` with a writer): refused when a
/// profile's two faces are one face, when a face is claimed by two profiles, or when the faces
/// do not share one valid solid.
pub fn discover_verified(
    ctx: &Context<'_>,
) -> Result<Vec<Occurrence<RepeatingRadialProfile>>, EvidenceError> {
    let found = discover(ctx);
    let mut used = BTreeSet::new();
    for o in &found {
        if o.defining[0] == o.defining[1] || o.defining.iter().any(|f| used.contains(f)) {
            return Err(EvidenceError::SharedEvidence);
        }
        if common_valid_solid(ctx.part, &o.defining).is_none() {
            return Err(EvidenceError::NoValidSolid);
        }
        used.extend(o.defining.iter().copied());
    }
    Ok(found)
}
}

fn discover_with(part: &Part, tol: f64) -> Vec<Occurrence<RepeatingRadialProfile>> {
    let mut found: Vec<Occurrence<RepeatingRadialProfile>> = if part.solids.is_empty() {
        let faces: Vec<usize> = (0..part.faces.len()).collect();
        recognise_solid(part, &faces, part.bounds(), tol)
    } else {
        (0..part.solids.len())
            .flat_map(|s| {
                recognise_solid(
                    part,
                    &part.solids[s].faces,
                    solid_properties::bounding_box(part, s),
                    tol,
                )
            })
            .collect()
    };
    found.sort_by(|a, b| a.record.order(&b.record));
    found
}

/// `_recognise_solid`: each lower boundary paired with the one upper boundary that corresponds
/// to it, and to no other lower boundary.
fn recognise_solid(
    part: &Part,
    faces: &[usize],
    bbox: Bounds,
    tol: f64,
) -> Vec<Occurrence<RepeatingRadialProfile>> {
    let metric_tol = tol.max(bbox.max_extent() * 1e-5);
    let boundaries: Vec<BoundaryEvidence> = faces
        .iter()
        .filter_map(|&face| prove_boundary(part, face, &bbox, metric_tol))
        .collect();
    let mut found = Vec::new();
    for axis in 0..3 {
        let (lo, hi) = (bbox.min[axis], bbox.max[axis]);
        let lowers: Vec<&BoundaryEvidence> = boundaries
            .iter()
            .filter(|b| b.axis == axis && (b.at - lo).abs() <= metric_tol)
            .collect();
        let uppers: Vec<&BoundaryEvidence> = boundaries
            .iter()
            .filter(|b| b.axis == axis && (b.at - hi).abs() <= metric_tol)
            .collect();
        for lower in &lowers {
            let matches: Vec<&&BoundaryEvidence> = uppers
                .iter()
                .filter(|upper| profiles_correspond(lower, upper, metric_tol))
                .collect();
            let [upper] = matches[..] else {
                continue;
            };
            // Source-face correspondence is one-to-one in both directions.
            if lowers
                .iter()
                .filter(|candidate| profiles_correspond(candidate, upper, metric_tol))
                .count()
                != 1
            {
                continue;
            }
            let mut centre = [0.0; 3];
            centre[lower.plane_axes[0]] = lower.centre.0;
            centre[lower.plane_axes[1]] = lower.centre.1;
            centre[axis] = (lo + hi) / 2.0;
            found.push(Occurrence {
                record: RepeatingRadialProfile {
                    axis: AXES[axis].to_string(),
                    centre,
                    span: [lo, hi],
                    repeat_count: lower.repeat_count,
                    edge_count: lower.edges.len(),
                    sector_signature: sector_signature(lower),
                },
                defining: vec![lower.face, upper.face],
                context: Vec::new(),
            });
        }
    }
    found
}

/// `_prove_boundary`: the face's outer wire, sampled, with the largest repeat count whose
/// sector rotation maps every edge bijectively.
fn prove_boundary(part: &Part, face: usize, bbox: &Bounds, tol: f64) -> Option<BoundaryEvidence> {
    let (axis, at) = principal_boundary_plane(part, face, bbox)?;
    let plane_axes = plane_axes(axis);
    let wire = outer_wire(part, face)?;
    let edges = sample_wire(part, &wire, plane_axes)?;
    // Common-circle arcs alone are a candidate-count proxy, never the full profile proof.
    if edges.is_empty() || edges.iter().all(|e| e.kind == "CIRCLE") {
        return None;
    }
    // The tip arcs' common centre is the rotation axis where there is one; the wire's box
    // centre otherwise. The complete-wire bijection remains the acceptance guard.
    let centre = common_circle_centre(part, &wire, plane_axes, tol).unwrap_or_else(|| {
        let c = wire_bounds(part, &wire).centre();
        (c[plane_axes[0]], c[plane_axes[1]])
    });
    (MIN_REPEAT_COUNT..edges.len())
        .rev()
        .filter(|count| edges.len() % count == 0)
        .find_map(|repeat_count| {
            cyclic_edge_orbits(&edges, centre, repeat_count, tol).map(|orbits| BoundaryEvidence {
                face,
                axis,
                at,
                plane_axes,
                centre,
                repeat_count,
                edges: edges.clone(),
                orbits,
            })
        })
}

/// The two coordinates across *axis*, in order.
fn plane_axes(axis: usize) -> [usize; 2] {
    match axis {
        0 => [1, 2],
        1 => [0, 2],
        _ => [0, 1],
    }
}

/// The face's outer wire, each edge once (`face.outer_wire().edges()`), with its direction.
fn outer_wire(part: &Part, face: usize) -> Option<Vec<(usize, bool)>> {
    let outer = part.outer_loop(face)?;
    let mut wire: Vec<(usize, bool)> = Vec::new();
    for &(e, forward) in &part.faces[face].loops[outer].edges {
        if !wire.iter().any(|w| w.0 == e) {
            wire.push((e, forward));
        }
    }
    Some(wire)
}

/// `_sample_wire`: every edge's kind, length and points at nine equal fractions of its arc
/// length, in its direction in the wire. `None` when an edge's length cannot be measured.
fn sample_wire(
    part: &Part,
    wire: &[(usize, bool)],
    plane_axes: [usize; 2],
) -> Option<Vec<CurveEvidence>> {
    wire.iter()
        .map(|&(e, forward)| {
            let edge = &part.edges[e];
            let measure = ArcLength::new(edge)?;
            let points = (0..SAMPLES_PER_CURVE)
                .map(|i| {
                    let s = i as f64 / (SAMPLES_PER_CURVE - 1) as f64;
                    let p = measure.point(if forward { s } else { 1.0 - s });
                    (p[plane_axes[0]], p[plane_axes[1]])
                })
                .collect();
            let kind = match edge.curve {
                Curve::Line { .. } => "LINE",
                Curve::Circle { .. } => "CIRCLE",
                Curve::Ellipse { .. } => "ELLIPSE",
                Curve::Nurbs(_) => "BSPLINE",
            };
            Some(CurveEvidence {
                kind,
                length: measure.total,
                points,
            })
        })
        .collect()
}

/// An edge measured along its arc: its length, and the point at any fraction of it
/// (`Edge.length`, `Edge.position_at` by `GCPnts_AbscissaPoint`).
struct ArcLength<'a> {
    curve: &'a Curve,
    start: V3,
    end: V3,
    /// The edge's parameter interval, start → end.
    interval: (f64, f64),
    /// Cuts of the interval (knots, half radians of an ellipse), with the length up to each.
    cuts: Vec<(f64, f64)>,
    total: f64,
}

impl<'a> ArcLength<'a> {
    fn new(edge: &'a crate::kernel::brep::Edge) -> Option<Self> {
        let curve = &edge.curve;
        let (a, b) = edge_interval(
            curve,
            edge.start,
            edge.end,
            edge.same_sense,
            edge.is_closed(),
        );
        let mut params = vec![a];
        match curve {
            Curve::Line { .. } | Curve::Circle { .. } => {}
            Curve::Ellipse { .. } => {
                let n = ((b - a).abs() / 0.5).ceil().max(1.0) as usize;
                params.extend((1..n).map(|i| a + (b - a) * i as f64 / n as f64));
            }
            Curve::Nurbs(n) => {
                let period = curve.period().unwrap_or(0.0);
                let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                let mut inner: Vec<f64> = n
                    .breaks()
                    .into_iter()
                    .flat_map(|k| [k - period, k, k + period])
                    .filter(|&k| k > lo && k < hi)
                    .collect();
                inner.sort_by(|x, y| x.total_cmp(y));
                if a > b {
                    inner.reverse();
                }
                params.extend(inner);
            }
        }
        params.push(b);
        let mut cuts = vec![(a, 0.0)];
        let mut total = 0.0;
        for w in params.windows(2) {
            total += match curve {
                Curve::Line { .. } => geom::dist(edge.start, edge.end),
                Curve::Circle { radius, .. } => radius * (w[1] - w[0]).abs(),
                _ => adaptive_length(curve, w[0], w[1], 0),
            };
            cuts.push((w[1], total));
        }
        (total.is_finite() && total > 0.0).then_some(ArcLength {
            curve,
            start: edge.start,
            end: edge.end,
            interval: (a, b),
            cuts,
            total,
        })
    }

    /// The point at fraction *s* of the length from the edge's start.
    fn point(&self, s: f64) -> V3 {
        match self.curve {
            Curve::Line { .. } => {
                return geom::add(self.start, geom::scale(geom::sub(self.end, self.start), s));
            }
            Curve::Circle { .. } => {
                let (a, b) = self.interval;
                return self.curve.value(a + (b - a) * s);
            }
            _ => {}
        }
        let target = self.total * s;
        let i = self
            .cuts
            .windows(2)
            .position(|w| target <= w[1].1)
            .unwrap_or(self.cuts.len() - 2);
        let ((t0, l0), (t1, l1)) = (self.cuts[i], self.cuts[i + 1]);
        if target <= l0 {
            return self.curve.value(t0);
        }
        if target >= l1 {
            return self.curve.value(t1);
        }
        // Newton on the fraction u of the cut (t = t0 + u (t1 - t0)), along which the length
        // grows whichever way the parameter runs, kept inside a shrinking bracket.
        let at = |u: f64| t0 + (t1 - t0) * u;
        let (mut lo, mut hi) = (0.0, 1.0);
        let mut u = (target - l0) / (l1 - l0);
        for _ in 0..100 {
            let f = l0 + adaptive_length(self.curve, t0, at(u), 0) - target;
            if f.abs() <= 1e-13 * self.total.max(1.0) {
                break;
            }
            if f > 0.0 {
                hi = u;
            } else {
                lo = u;
            }
            let slope = geom::norm(self.curve.derivative(at(u))) * (t1 - t0).abs();
            let next = u - f / slope;
            u = if next.is_finite() && next > lo && next < hi {
                next
            } else {
                (lo + hi) / 2.0
            };
        }
        self.curve.value(at(u))
    }
}

/// The curve's length between parameters *a* and *b*: Gauss–Legendre, halved until the halves
/// agree with the whole.
fn adaptive_length(curve: &Curve, a: f64, b: f64, depth: usize) -> f64 {
    let whole = gauss_length(curve, a, b);
    let m = (a + b) / 2.0;
    let halves = gauss_length(curve, a, m) + gauss_length(curve, m, b);
    if depth >= 24 || (whole - halves).abs() <= 1e-14 * halves.abs().max(1e-3) {
        return halves;
    }
    adaptive_length(curve, a, m, depth + 1) + adaptive_length(curve, m, b, depth + 1)
}

/// Eight-point Gauss–Legendre on [a, b] of the curve's speed (positive whichever way it runs).
fn gauss_length(curve: &Curve, a: f64, b: f64) -> f64 {
    const NODES: [(f64, f64); 4] = [
        (0.183_434_642_495_649_8, 0.362_683_783_378_362),
        (0.525_532_409_916_329, 0.313_706_645_877_887_3),
        (0.796_666_477_413_626_7, 0.222_381_034_453_374_5),
        (0.960_289_856_497_536_3, 0.101_228_536_290_376_3),
    ];
    let (m, h) = ((a + b) / 2.0, (b - a) / 2.0);
    let sum: f64 = NODES
        .iter()
        .map(|&(x, w)| {
            w * (geom::norm(curve.derivative(m - h * x)) + geom::norm(curve.derivative(m + h * x)))
        })
        .sum();
    sum * h.abs()
}

/// The wire's exact box (`wire.bounding_box()`, OpenCascade's optimal box): its edges' samples
/// and ends with every curve's interior extremes along the three axes.
fn wire_bounds(part: &Part, wire: &[(usize, bool)]) -> Bounds {
    let mut b = Bounds::empty();
    for &(e, _) in wire {
        let edge = &part.edges[e];
        let interval = edge_interval(
            &edge.curve,
            edge.start,
            edge.end,
            edge.same_sense,
            edge.is_closed(),
        );
        for p in edge.samples.iter().copied().chain(extremes_along(
            &edge.curve,
            interval,
            &[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        )) {
            b.add(p);
        }
    }
    b
}

/// `_common_circle_centre`: the unanimous centre of at least two circular outer-wire curves.
fn common_circle_centre(
    part: &Part,
    wire: &[(usize, bool)],
    plane_axes: [usize; 2],
    tol: f64,
) -> Option<(f64, f64)> {
    let centres: Vec<(f64, f64)> = wire
        .iter()
        .filter_map(|&(e, _)| match &part.edges[e].curve {
            Curve::Circle { frame, .. } => {
                Some((frame.origin[plane_axes[0]], frame.origin[plane_axes[1]]))
            }
            _ => None,
        })
        .collect();
    if centres.len() < 2 {
        return None;
    }
    let n = centres.len() as f64;
    let mean = (
        py::sum(centres.iter().map(|c| c.0)) / n,
        py::sum(centres.iter().map(|c| c.1)) / n,
    );
    centres
        .iter()
        .all(|&c| distance(c, mean) <= tol)
        .then_some(mean)
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// `_one_closed_cycle`: the edges' ends form one connected graph of degree two, whatever the
/// traversal order.
fn one_closed_cycle(edges: &[CurveEvidence], tol: f64) -> bool {
    if edges.is_empty() {
        return false;
    }
    let mut nodes: Vec<(f64, f64)> = Vec::new();
    let mut incident: Vec<Vec<usize>> = Vec::new();
    let mut edge_nodes: Vec<[usize; 2]> = Vec::new();
    for (edge_index, edge) in edges.iter().enumerate() {
        let mut ends = [0; 2];
        for (k, point) in [edge.points[0], edge.points[edge.points.len() - 1]]
            .into_iter()
            .enumerate()
        {
            let matches: Vec<usize> = (0..nodes.len())
                .filter(|&i| distance(point, nodes[i]) <= tol)
                .collect();
            let node = match matches[..] {
                [] => {
                    nodes.push(point);
                    incident.push(Vec::new());
                    nodes.len() - 1
                }
                [i] => i,
                _ => return false,
            };
            incident[node].push(edge_index);
            ends[k] = node;
        }
        if ends[0] == ends[1] {
            return false;
        }
        edge_nodes.push(ends);
    }
    if incident.iter().any(|ids| ids.len() != 2) {
        return false;
    }
    let mut reached = BTreeSet::new();
    let mut frontier = vec![0];
    while let Some(edge) = frontier.pop() {
        if !reached.insert(edge) {
            continue;
        }
        for &node in &edge_nodes[edge] {
            frontier.extend(incident[node].iter().filter(|n| !reached.contains(n)));
        }
    }
    reached.len() == edges.len()
}

fn rotate(point: (f64, f64), angle: f64, centre: (f64, f64)) -> (f64, f64) {
    let (x, y) = (point.0 - centre.0, point.1 - centre.1);
    (
        centre.0 + x * angle.cos() - y * angle.sin(),
        centre.1 + x * angle.sin() + y * angle.cos(),
    )
}

/// `_curves_match`: *source* turned by *angle* lies on *target*, in either direction.
fn curves_match(
    source: &CurveEvidence,
    target: &CurveEvidence,
    angle: f64,
    centre: (f64, f64),
    tol: f64,
) -> bool {
    if source.kind != target.kind || (source.length - target.length).abs() > tol {
        return false;
    }
    let rotated: Vec<(f64, f64)> = source
        .points
        .iter()
        .map(|&p| rotate(p, angle, centre))
        .collect();
    let worst = |candidate: &mut dyn Iterator<Item = &(f64, f64)>| {
        rotated
            .iter()
            .zip(candidate)
            .map(|(&l, &r)| distance(l, r))
            .fold(f64::NEG_INFINITY, f64::max)
    };
    worst(&mut target.points.iter()) <= tol || worst(&mut target.points.iter().rev()) <= tol
}

/// `_cyclic_edge_orbits`: the orbits of a bijective mapping of every edge under one sector
/// rotation, each of exactly *repeat_count* edges.
fn cyclic_edge_orbits(
    edges: &[CurveEvidence],
    centre: (f64, f64),
    repeat_count: usize,
    tol: f64,
) -> Option<Vec<Vec<usize>>> {
    if edges.len() <= repeat_count
        || !edges.len().is_multiple_of(repeat_count)
        || !one_closed_cycle(edges, tol)
    {
        return None;
    }
    let angle = 2.0 * PI / repeat_count as f64;
    let mut mapping = Vec::with_capacity(edges.len());
    for edge in edges {
        let matches: Vec<usize> = (0..edges.len())
            .filter(|&i| curves_match(edge, &edges[i], angle, centre, tol))
            .collect();
        let [image] = matches[..] else {
            return None;
        };
        mapping.push(image);
    }
    if mapping.iter().collect::<BTreeSet<_>>().len() != edges.len() {
        return None;
    }
    let mut unseen: BTreeSet<usize> = (0..edges.len()).collect();
    let mut orbits = Vec::new();
    while let Some(&start) = unseen.first() {
        let mut orbit = Vec::new();
        let mut current = start;
        while !orbit.contains(&current) {
            orbit.push(current);
            current = mapping[current];
        }
        if current != start || orbit.len() != repeat_count {
            return None;
        }
        for i in &orbit {
            unseen.remove(i);
        }
        orbits.push(orbit);
    }
    Some(orbits)
}

/// `_profiles_correspond`: one axis, repeat and edge count, centre, and sector inventory.
fn profiles_correspond(lower: &BoundaryEvidence, upper: &BoundaryEvidence, tol: f64) -> bool {
    lower.axis == upper.axis
        && lower.plane_axes == upper.plane_axes
        && lower.repeat_count == upper.repeat_count
        && lower.edges.len() == upper.edges.len()
        && (lower.centre.0 - upper.centre.0).abs() <= tol
        && (lower.centre.1 - upper.centre.1).abs() <= tol
        && sequence_order(
            &sector_signature(lower),
            &sector_signature(upper),
            sector_order,
        )
        .is_eq()
}

/// `_polar_signature`: a sampled curve's shape about *centre*, modulo phase, reflection and
/// traversal direction.
fn polar_signature(points: &[(f64, f64)], centre: (f64, f64)) -> Vec<(f64, f64)> {
    let one_direction = |candidate: Vec<(f64, f64)>| {
        let polar: Vec<(f64, f64)> = candidate
            .iter()
            .map(|&p| (distance(p, centre), (p.1 - centre.1).atan2(p.0 - centre.0)))
            .collect();
        let mut unwrapped = vec![polar[0].1];
        for &(_, angle) in &polar[1..] {
            let previous = unwrapped[unwrapped.len() - 1];
            let mut angle = angle;
            while angle - previous > PI {
                angle -= 2.0 * PI;
            }
            while angle - previous < -PI {
                angle += 2.0 * PI;
            }
            unwrapped.push(angle);
        }
        let phase = unwrapped[0];
        let relative: Vec<(f64, f64)> = polar
            .iter()
            .zip(&unwrapped)
            .map(|(&(radius, _), &angle)| (py::round_to(radius, 6), py::round_to(angle - phase, 6)))
            .collect();
        let reflected: Vec<(f64, f64)> = relative.iter().map(|&(r, a)| (r, -a)).collect();
        first_min(relative, reflected)
    };
    let forward = one_direction(points.to_vec());
    let backward = one_direction(points.iter().rev().copied().collect());
    first_min(forward, backward)
}

/// Python's `min(a, b)` of point tuples: the first unless the second is smaller.
fn first_min(a: Vec<(f64, f64)>, b: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    if sequence_order(&b, &a, pair_order).is_lt() {
        b
    } else {
        a
    }
}

/// `_sector_signature`: each orbit's first edge as (kind, rounded length, polar shape), sorted.
fn sector_signature(boundary: &BoundaryEvidence) -> Vec<SectorCurve> {
    let mut signature: Vec<SectorCurve> = boundary
        .orbits
        .iter()
        .map(|orbit| {
            let edge = &boundary.edges[orbit[0]];
            (
                edge.kind.to_string(),
                py::round_to(edge.length, 6),
                polar_signature(&edge.points, boundary.centre),
            )
        })
        .collect();
    signature.sort_by(sector_order);
    signature
}

fn pair_order(a: &(f64, f64), b: &(f64, f64)) -> Ordering {
    py::order(a.0, b.0).then_with(|| py::order(a.1, b.1))
}

fn sector_order(a: &SectorCurve, b: &SectorCurve) -> Ordering {
    a.0.cmp(&b.0)
        .then_with(|| py::order(a.1, b.1))
        .then_with(|| sequence_order(&a.2, &b.2, pair_order))
}

/// Python's ordering of tuples: element by element, then by length.
fn sequence_order<T>(a: &[T], b: &[T], order: impl Fn(&T, &T) -> Ordering) -> Ordering {
    a.iter()
        .zip(b)
        .map(|(x, y)| order(x, y))
        .find(|o| o.is_ne())
        .unwrap_or_else(|| a.len().cmp(&b.len()))
}
