//! Canonical line/arc sections and run-local placement values (`quiddity._sections`): the
//! geometry values the section-ring families (passages, oriented slots, section recesses) share.
//!
//! Nothing here is a record or a kernel value. A section is a closed loop of 2-D vertices, each
//! carrying the bulge (tangent of a quarter of the sweep) of the edge to the next; it is
//! validated simple, turned counter-clockwise and started at its least serialized vertex, so one
//! shape has one value. A frame places a section along a run. Every check refuses with
//! Python's message rather than repairing the value.
//!
//! Python guards its frozen dataclasses against mutation and its body references against forgery
//! by object identity; here the fields are private and the values are built only through their
//! validating constructors, so those guards are the type system's.

use std::cmp::Ordering;
use std::f64::consts::PI;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use serde::Serialize;

use super::placed::{Grid, Placed};
use crate::kernel::geom::V3;
use crate::kernel::py;

pub type V2 = [f64; 2];

const EPS: f64 = 1e-9;
const POSITION_TOL: f64 = 8e-4;
const OCCURRENCE_TOL: f64 = 2e-3;
const BULGE_DIGITS: usize = 12;

/// A refusal, with the Python implementation's `ValueError` message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionError(pub &'static str);

impl fmt::Display for SectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for SectionError {}

type Checked<T> = Result<T, SectionError>;

fn refuse<T>(message: &'static str) -> Checked<T> {
    Err(SectionError(message))
}

fn finite(values: &[f64]) -> bool {
    values.iter().all(|v| v.is_finite())
}

fn cross(a: V3, b: V3) -> V3 {
    crate::kernel::geom::cross(a, b)
}

fn dot(a: V3, b: V3) -> f64 {
    py::dot(&a, &b)
}

/// `quiddity._geometry.unit`: refuses a nonfinite or degenerate direction.
fn unit(v: V3) -> Checked<V3> {
    py::unit(v).map_or_else(|| refuse("direction is nonfinite or degenerate"), Ok)
}

internal_entry! {
/// `_round_clean`: Python's `round`, without a negative zero.
pub fn round_clean(value: f64, digits: usize) -> f64 {
    py::without_negative_zero(py::round_to(value, digits))
}
}

/// A right-handed placement frame (`LocalFrame`): a section's `u`, `v` across the `run`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct LocalFrame {
    pub origin: V3,
    pub run: V3,
    pub u: V3,
    pub v: V3,
}

impl LocalFrame {
    /// The frame, refused unless finite, unit, orthogonal and right handed.
    pub fn new(origin: V3, run: V3, u: V3, v: V3) -> Checked<Self> {
        if !finite(&[origin, run, u, v].concat()) {
            return refuse("frame values must be finite");
        }
        for d in [run, u, v] {
            if !py::isclose(dot(d, d), 1.0, 1e-9, EPS) {
                return refuse("frame directions must be unit length");
            }
        }
        if [(run, u), (run, v), (u, v)]
            .iter()
            .any(|&(a, b)| dot(a, b).abs() > EPS)
        {
            return refuse("frame directions must be orthogonal");
        }
        if cross(run, u)
            .iter()
            .zip(&v)
            .any(|(a, b)| (a - b).abs() > EPS)
        {
            return refuse("frame must be right handed: run cross u equals v");
        }
        Ok(LocalFrame { origin, run, u, v })
    }

    /// The canonical principal-axis basis through *centroid* (`LocalFrame.principal`).
    pub fn principal(axis: &str, centroid: V3) -> Checked<Self> {
        let (run, u, v) = match axis {
            "x" => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
            "y" => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
            "z" => ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            _ => return refuse("axis must be 'x', 'y', or 'z'"),
        };
        if !finite(&centroid) {
            return refuse("centroid must be finite");
        }
        let along = dot(centroid, run);
        let origin = [0, 1, 2].map(|i| centroid[i] - along * run[i]);
        LocalFrame::new(origin, run, u, v)
    }

    /// The deterministic free-axis frame (`LocalFrame.canonical`): the run turned to point along
    /// its dominant serialized component (ties to z, then y), `u` the next axis's projection.
    pub fn canonical(run: V3, centroid: V3) -> Checked<Self> {
        let mut direction = unit(run)?;
        let serialized = direction.map(|c| round_clean(c, 6));
        let components = serialized.map(f64::abs);
        let peak = components.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let dominant = [2, 1, 0]
            .into_iter()
            .find(|&i| components[i] == peak)
            .expect("the peak is a component");
        if serialized[dominant] < 0.0 {
            direction = direction.map(|c| -c);
        }
        let seed = [[0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]][dominant];
        let along_seed = dot(seed, direction);
        let u = unit([0, 1, 2].map(|i| seed[i] - along_seed * direction[i]))?;
        let v = cross(direction, u);
        if !finite(&centroid) {
            return refuse("centroid must be finite");
        }
        let along = dot(centroid, direction);
        let origin = [0, 1, 2].map(|i| centroid[i] - along * direction[i]);
        LocalFrame::new(origin, direction, u, v)
    }
}

/// A 2-D vertex whose bulge describes the edge to the next vertex (`SectionVertex`): zero for a
/// line. Negative zeros are cleared.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionVertex {
    pub point: V2,
    pub bulge: f64,
}

impl SectionVertex {
    pub fn new(point: V2, bulge: f64) -> Checked<Self> {
        if !finite(&[point[0], point[1], bulge]) {
            return refuse("section vertex must be finite");
        }
        Ok(SectionVertex {
            point: point.map(py::without_negative_zero),
            bulge: py::without_negative_zero(bulge),
        })
    }

    fn line(point: V2) -> Self {
        SectionVertex {
            point: point.map(py::without_negative_zero),
            bulge: 0.0,
        }
    }
}

/// The circle of a bulged edge (`_Arc`): its centre, radius, start angle and signed sweep.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Arc {
    pub(crate) centre: V2,
    pub(crate) radius: f64,
    pub(crate) start: f64,
    pub(crate) sweep: f64,
}

/// The arc from *a* to *b* (`_arc`): `None` for a straight edge.
pub(crate) fn arc(a: &SectionVertex, b: &SectionVertex) -> Checked<Option<Arc>> {
    let bulge = a.bulge;
    if bulge == 0.0 {
        return Ok(None);
    }
    let (dx, dy) = (b.point[0] - a.point[0], b.point[1] - a.point[1]);
    let chord = py::hypot(&[dx, dy]);
    if chord <= EPS {
        return refuse("arc endpoints must be distinct");
    }
    let offset = chord * (1.0 - bulge * bulge) / (4.0 * bulge);
    let centre = [
        0.5 * (a.point[0] + b.point[0]) - dy * offset / chord,
        0.5 * (a.point[1] + b.point[1]) + dx * offset / chord,
    ];
    Ok(Some(Arc {
        centre,
        radius: chord * (1.0 + bulge * bulge) / (4.0 * bulge.abs()),
        start: (a.point[1] - centre[1]).atan2(a.point[0] - centre[0]),
        sweep: 4.0 * bulge.atan(),
    }))
}

/// Green-theorem area and first moments of a straight edge (the same fields as the arcs', so a
/// mixed loop's centroid is translation invariant).
fn line_moments(a: V2, b: V2) -> [f64; 3] {
    let cross = a[0] * b[1] - b[0] * a[1];
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let mx = dy * (a[0] * a[0] + a[0] * b[0] + b[0] * b[0]) / 6.0;
    let my = -dx * (a[1] * a[1] + a[1] * b[1] + b[1] * b[1]) / 6.0;
    [0.5 * cross, mx, my]
}

fn arc_moments(arc: &Arc) -> [f64; 3] {
    let [cx, cy] = arc.centre;
    let (radius, start, end) = (arc.radius, arc.start, arc.start + arc.sweep);
    let dsin = end.sin() - start.sin();
    let dcos = end.cos() - start.cos();
    let dsin2 = (2.0 * end).sin() - (2.0 * start).sin();
    let area = 0.5 * (radius * cx * dsin - radius * cy * dcos + radius * radius * arc.sweep);
    let int_cos2 = 0.5 * arc.sweep + 0.25 * dsin2;
    let int_cos3 =
        (end.sin() - end.sin().powi(3) / 3.0) - (start.sin() - start.sin().powi(3) / 3.0);
    let x2dy = radius * (cx * cx * dsin + 2.0 * cx * radius * int_cos2 + radius.powi(2) * int_cos3);
    let int_sin = -dcos;
    let int_sin2 = 0.5 * arc.sweep - 0.25 * dsin2;
    let int_sin3 =
        (-end.cos() + end.cos().powi(3) / 3.0) - (-start.cos() + start.cos().powi(3) / 3.0);
    let y2dx =
        -radius * (cy * cy * int_sin + 2.0 * cy * radius * int_sin2 + radius.powi(2) * int_sin3);
    [area, 0.5 * x2dy, -0.5 * y2dx]
}

/// The loop's signed area and centroid, integrated about its first vertex (`_moments`).
pub(crate) fn moments(vertices: &[SectionVertex]) -> Checked<(f64, V2)> {
    let anchor = vertices[0].point;
    let local: Vec<SectionVertex> = vertices
        .iter()
        .map(|v| SectionVertex {
            point: [v.point[0] - anchor[0], v.point[1] - anchor[1]],
            bulge: v.bulge,
        })
        .collect();
    let (mut area, mut mx, mut my) = (0.0, 0.0, 0.0);
    for (i, vertex) in local.iter().enumerate() {
        let following = &local[(i + 1) % local.len()];
        let c = match arc(vertex, following)? {
            None => line_moments(vertex.point, following.point),
            Some(arc) => arc_moments(&arc),
        };
        area += c[0];
        mx += c[1];
        my += c[2];
    }
    if !area.is_finite() || area.abs() <= EPS {
        return refuse("section must enclose nonzero area");
    }
    Ok((area, [mx / area + anchor[0], my / area + anchor[1]]))
}

fn reverse(vertices: &[SectionVertex]) -> Vec<SectionVertex> {
    let n = vertices.len();
    (0..n)
        .map(|i| SectionVertex {
            point: vertices[(n - i) % n].point,
            bulge: py::without_negative_zero(-vertices[n - 1 - i].bulge),
        })
        .collect()
}

/// A vertex as published: its point to 3 decimals, its bulge to 12, refused where the rounding
/// would turn an arc into a line.
fn serialized(vertex: &SectionVertex) -> Checked<[f64; 3]> {
    let bulge = py::round_to(vertex.bulge, BULGE_DIGITS);
    if vertex.bulge != 0.0 && bulge == 0.0 {
        return refuse("serialization would collapse a nonzero arc");
    }
    Ok([
        round_clean(vertex.point[0], 3),
        round_clean(vertex.point[1], 3),
        py::without_negative_zero(bulge),
    ])
}

/// The rotation of the loop whose serialized vertices compare least (the first of ties).
fn canonical_start(vertices: &[SectionVertex]) -> Checked<Vec<SectionVertex>> {
    let keys: Vec<f64> = vertices
        .iter()
        .map(serialized)
        .collect::<Checked<Vec<_>>>()?
        .concat();
    let n = vertices.len();
    let rotated = |s: usize| -> Vec<f64> {
        (0..n)
            .flat_map(|i| keys[3 * ((s + i) % n)..][..3].to_vec())
            .collect()
    };
    let mut best = 0;
    for s in 1..n {
        if py::tuple_order(&rotated(s), &rotated(best)) == Ordering::Less {
            best = s;
        }
    }
    Ok((0..n).map(|i| vertices[(best + i) % n]).collect())
}

fn complex_mul(a: V2, b: V2) -> V2 {
    [a[0] * b[0] - a[1] * b[1], a[0] * b[1] + a[1] * b[0]]
}

fn complex_abs(a: V2) -> f64 {
    py::hypot(&a)
}

/// How far serialization moves the boundary, bounded exactly: the vertices' displacement, and
/// for each arc the largest displacement between the two equal-sweep arcs (at an end or where
/// the rotating radius difference aligns with the centre difference) plus the chord bound on the
/// tiny sweep change.
fn projection_bound(vertices: &[SectionVertex]) -> Checked<f64> {
    let projected: Vec<SectionVertex> = vertices
        .iter()
        .map(|v| {
            serialized(v).map(|s| SectionVertex {
                point: [s[0], s[1]],
                bulge: s[2],
            })
        })
        .collect::<Checked<_>>()?;
    let n = vertices.len();
    let mut bound: f64 = 0.0;
    for (i, vertex) in vertices.iter().enumerate() {
        bound = bound.max(py::dist(&vertex.point, &projected[i].point));
        if vertex.bulge == 0.0 {
            continue;
        }
        let (Some(original), Some(rounded)) = (
            arc(vertex, &vertices[(i + 1) % n])?,
            arc(&projected[i], &projected[(i + 1) % n])?,
        ) else {
            unreachable!("a nonzero bulge serializes nonzero");
        };
        let centre_delta = [
            original.centre[0] - rounded.centre[0],
            original.centre[1] - rounded.centre[1],
        ];
        let radial_delta = [
            original.radius * original.start.cos() - rounded.radius * rounded.start.cos(),
            original.radius * original.start.sin() - rounded.radius * rounded.start.sin(),
        ];
        let mut angles = vec![0.0, original.sweep];
        if complex_abs(centre_delta) > 0.0 && complex_abs(radial_delta) > 0.0 {
            let aligned =
                centre_delta[1].atan2(centre_delta[0]) - radial_delta[1].atan2(radial_delta[0]);
            for turn in [-2.0 * PI, 0.0, 2.0 * PI] {
                let angle = aligned + turn;
                if 0.0f64.min(original.sweep) <= angle && angle <= 0.0f64.max(original.sweep) {
                    angles.push(angle);
                }
            }
        }
        let largest = angles
            .iter()
            .map(|&a| {
                let turned = complex_mul(radial_delta, [a.cos(), a.sin()]);
                complex_abs([centre_delta[0] + turned[0], centre_delta[1] + turned[1]])
            })
            .fold(f64::NEG_INFINITY, f64::max);
        bound = bound.max(largest + rounded.radius * (original.sweep - rounded.sweep).abs());
    }
    Ok(bound)
}

fn orient(p: V2, q: V2, r: V2) -> f64 {
    (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
}

fn on_segment(p: V2, q: V2, r: V2) -> bool {
    p[0].min(r[0]) - EPS <= q[0]
        && q[0] <= p[0].max(r[0]) + EPS
        && p[1].min(r[1]) - EPS <= q[1]
        && q[1] <= p[1].max(r[1]) + EPS
}

fn line_intersection(a: V2, b: V2, c: V2, d: V2) -> bool {
    let (o1, o2, o3, o4) = (
        orient(a, b, c),
        orient(a, b, d),
        orient(c, d, a),
        orient(c, d, b),
    );
    if (o1.abs() <= EPS && on_segment(a, c, b))
        || (o2.abs() <= EPS && on_segment(a, d, b))
        || (o3.abs() <= EPS && on_segment(c, a, d))
        || (o4.abs() <= EPS && on_segment(c, b, d))
    {
        return true;
    }
    (o1 > 0.0) != (o2 > 0.0) && (o3 > 0.0) != (o4 > 0.0)
}

fn angle_on_arc(angle: f64, arc: &Arc, tolerance: f64) -> bool {
    let (delta, sweep) = if arc.sweep < 0.0 {
        (py::modulo(arc.start - angle, 2.0 * PI), -arc.sweep)
    } else {
        (py::modulo(angle - arc.start, 2.0 * PI), arc.sweep)
    };
    -tolerance <= delta && delta <= sweep + tolerance
}

fn point_on_arc(point: V2, arc: &Arc, tolerance: f64) -> bool {
    let (dx, dy) = (point[0] - arc.centre[0], point[1] - arc.centre[1]);
    if (py::hypot(&[dx, dy]) - arc.radius).abs() > EPS {
        return false;
    }
    angle_on_arc(dy.atan2(dx), arc, tolerance)
}

fn line_arc_points(a: V2, b: V2, arc: &Arc) -> Vec<V2> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let (fx, fy) = (a[0] - arc.centre[0], a[1] - arc.centre[1]);
    let aa = dx * dx + dy * dy;
    let bb = 2.0 * (fx * dx + fy * dy);
    let cc = fx * fx + fy * fy - arc.radius * arc.radius;
    let discriminant = bb * bb - 4.0 * aa * cc;
    if discriminant < -EPS {
        return Vec::new();
    }
    let roots = if discriminant.abs() <= EPS {
        vec![-bb / (2.0 * aa)]
    } else {
        vec![
            (-bb - discriminant.sqrt()) / (2.0 * aa),
            (-bb + discriminant.sqrt()) / (2.0 * aa),
        ]
    };
    roots
        .into_iter()
        .filter(|root| (-EPS..=1.0 + EPS).contains(root))
        .map(|root| [a[0] + root * dx, a[1] + root * dy])
        .filter(|&p| point_on_arc(p, arc, EPS))
        .collect()
}

/// The points two circles' arcs share; `None` for one circle (concentric, equal radii).
fn arc_arc_points(first: &Arc, second: &Arc) -> Option<Vec<V2>> {
    let (dx, dy) = (
        second.centre[0] - first.centre[0],
        second.centre[1] - first.centre[1],
    );
    let distance = py::hypot(&[dx, dy]);
    if distance <= EPS {
        return ((first.radius - second.radius).abs() > EPS).then(Vec::new);
    }
    if distance > first.radius + second.radius + EPS
        || distance < (first.radius - second.radius).abs() - EPS
    {
        return Some(Vec::new());
    }
    let along =
        (first.radius.powi(2) - second.radius.powi(2) + distance.powi(2)) / (2.0 * distance);
    let height2 = first.radius.powi(2) - along.powi(2);
    if height2 < -EPS {
        return Some(Vec::new());
    }
    let height = height2.max(0.0).sqrt();
    let base = [
        first.centre[0] + along * dx / distance,
        first.centre[1] + along * dy / distance,
    ];
    let points = [
        [
            base[0] - height * dy / distance,
            base[1] + height * dx / distance,
        ],
        [
            base[0] + height * dy / distance,
            base[1] - height * dx / distance,
        ],
    ];
    Some(
        points
            .into_iter()
            .filter(|&p| point_on_arc(p, first, EPS) && point_on_arc(p, second, EPS))
            .collect(),
    )
}

fn arc_arc_intersection(first: &Arc, second: &Arc) -> bool {
    if let Some(points) = arc_arc_points(first, second) {
        return !points.is_empty();
    }
    let ends = [
        first.start,
        first.start + first.sweep,
        second.start,
        second.start + second.sweep,
    ];
    ends.iter()
        .enumerate()
        .any(|(i, &a)| angle_on_arc(a, if i < 2 { second } else { first }, EPS))
}

/// Whether a line meets an arc it shares an end with a second time: the known shared root is
/// factored out, so a tangent double root is not split by cancellation.
fn adjacent_line_arc_crosses(shared: V2, end: V2, arc: &Arc) -> bool {
    let (dx, dy) = (end[0] - shared[0], end[1] - shared[1]);
    let (fx, fy) = (shared[0] - arc.centre[0], shared[1] - arc.centre[1]);
    let fraction = -2.0 * (fx * dx + fy * dy) / (dx * dx + dy * dy);
    let parameter_tolerance = EPS / py::hypot(&[dx, dy]);
    if !(-parameter_tolerance <= fraction && fraction <= 1.0 + parameter_tolerance) {
        return false;
    }
    let point = [shared[0] + fraction * dx, shared[1] + fraction * dy];
    py::dist(&point, &shared) > EPS && point_on_arc(point, arc, EPS / arc.radius)
}

fn validate_adjacent(
    first_start: &SectionVertex,
    shared: &SectionVertex,
    second_end: &SectionVertex,
) -> Checked<()> {
    let first = arc(first_start, shared)?;
    let second = arc(shared, second_end)?;
    match (first, second) {
        (None, None) => {
            let left = [
                shared.point[0] - first_start.point[0],
                shared.point[1] - first_start.point[1],
            ];
            let right = [
                second_end.point[0] - shared.point[0],
                second_end.point[1] - shared.point[1],
            ];
            let cross = left[0] * right[1] - left[1] * right[0];
            if cross.abs() <= EPS && left[0] * right[0] + left[1] * right[1] <= 0.0 {
                return refuse("adjacent section edges overlap or backtrack");
            }
        }
        (None, Some(second)) => {
            if adjacent_line_arc_crosses(shared.point, first_start.point, &second) {
                return refuse("adjacent section edges meet away from their shared endpoint");
            }
        }
        (Some(first), None) => {
            if adjacent_line_arc_crosses(shared.point, second_end.point, &first) {
                return refuse("adjacent section edges meet away from their shared endpoint");
            }
        }
        (Some(first), Some(second)) => match arc_arc_points(&first, &second) {
            None => {
                let first_end = first.start + first.sweep;
                let (s1, s2) = (1.0f64.copysign(first.sweep), 1.0f64.copysign(second.sweep));
                let first_tangent = [s1 * -first_end.sin(), s1 * first_end.cos()];
                let second_tangent = [s2 * -second.start.sin(), s2 * second.start.cos()];
                let aligned = py::sum([
                    first_tangent[0] * second_tangent[0],
                    first_tangent[1] * second_tangent[1],
                ]);
                if aligned < 1.0 - EPS || first.sweep.abs() + second.sweep.abs() > 2.0 * PI + EPS {
                    return refuse("adjacent circular arcs overlap or backtrack");
                }
            }
            Some(points) => {
                if points.iter().any(|p| py::dist(p, &shared.point) > EPS) {
                    return refuse("adjacent section edges meet away from their shared endpoint");
                }
            }
        },
    }
    Ok(())
}

/// Refuse a loop (or, not *closed*, an open chain with no closing edge) whose edges overlap,
/// backtrack or cross (`_validate_simple`).
pub(crate) fn validate_simple(vertices: &[SectionVertex], closed: bool) -> Checked<()> {
    let n = vertices.len();
    if n > 2 {
        let shared = if closed { 0..n } else { 1..n - 1 };
        for i in shared {
            validate_adjacent(
                &vertices[(i + n - 1) % n],
                &vertices[i],
                &vertices[(i + 1) % n],
            )?;
        }
    }
    let edges = if closed { n } else { n.saturating_sub(1) };
    for left in 0..edges {
        let (a, b) = (&vertices[left], &vertices[(left + 1) % n]);
        for right in left + 1..edges {
            if right == left + 1 || (closed && left == 0 && right == n - 1) {
                continue;
            }
            let (c, d) = (&vertices[right], &vertices[(right + 1) % n]);
            let intersects = match (arc(a, b)?, arc(c, d)?) {
                (None, None) => line_intersection(a.point, b.point, c.point, d.point),
                (None, Some(second)) => !line_arc_points(a.point, b.point, &second).is_empty(),
                (Some(first), None) => !line_arc_points(c.point, d.point, &first).is_empty(),
                (Some(first), Some(second)) => arc_arc_intersection(&first, &second),
            };
            if intersects {
                return refuse("section boundary must be simple");
            }
        }
    }
    Ok(())
}

internal_entry! {
/// Prove the span between two termination planes (`span + gradient · p`) positive at every
/// vertex and every arc's extreme along the gradient (`validate_section_end_separation`); an
/// open chain has no closing edge.
pub fn validate_section_end_separation(
    vertices: &[SectionVertex],
    span: f64,
    gradient: V2,
    closed: bool,
) -> Checked<()> {
    let n = vertices.len();
    let mut points: Vec<V2> = vertices.iter().map(|v| v.point).collect();
    let angle = gradient[1].atan2(gradient[0]);
    let edges = if closed { n } else { n.saturating_sub(1) };
    for i in 0..edges {
        if let Some(arc) = arc(&vertices[i], &vertices[(i + 1) % n])?
            && gradient != [0.0, 0.0]
        {
            for extremum in [angle, angle + PI] {
                if angle_on_arc(extremum, &arc, EPS) {
                    points.push([
                        arc.centre[0] + arc.radius * extremum.cos(),
                        arc.centre[1] + arc.radius * extremum.sin(),
                    ]);
                }
            }
        }
    }
    if points
        .iter()
        .any(|&[x, y]| span + gradient[0] * x + gradient[1] * y <= EPS)
    {
        return refuse("section termination planes must not cross or touch the profile");
    }
    Ok(())
}
}

/// A canonical closed line/arc loop (`PlanarSection`): simple, counter-clockwise, started at its
/// least serialized vertex, and moved no further than 8e-4 by serialization.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanarSection {
    boundary: Vec<SectionVertex>,
}

impl PlanarSection {
    pub fn new(boundary: Vec<SectionVertex>) -> Checked<Self> {
        let n = boundary.len();
        if n < 2 {
            return refuse("section needs at least two vertices");
        }
        if (0..n).any(|i| py::dist(&boundary[i].point, &boundary[(i + 1) % n].point) <= EPS) {
            return refuse("adjacent section vertices must be distinct");
        }
        validate_simple(&boundary, true)?;
        let (area, _) = moments(&boundary)?;
        let turned = if area < 0.0 {
            reverse(&boundary)
        } else {
            boundary
        };
        let vertices = canonical_start(&turned)?;
        let keys: Vec<[f64; 3]> = vertices.iter().map(serialized).collect::<Checked<_>>()?;
        for (i, a) in keys.iter().enumerate() {
            if keys[..i].iter().any(|b| a[0] == b[0] && a[1] == b[1]) {
                return refuse("serialization would collapse distinct vertices");
            }
        }
        if projection_bound(&vertices)? > POSITION_TOL {
            return refuse("serialized section moves its boundary beyond local tolerance");
        }
        Ok(PlanarSection { boundary: vertices })
    }

    /// A loop of straight edges through *points*.
    pub fn polygon(points: &[V2]) -> Checked<Self> {
        PlanarSection::new(points.iter().map(|&p| SectionVertex::line(p)).collect())
    }

    pub fn boundary(&self) -> &[SectionVertex] {
        &self.boundary
    }

    pub fn area(&self) -> f64 {
        moments(&self.boundary)
            .expect("validated on construction")
            .0
    }

    pub fn centroid(&self) -> V2 {
        moments(&self.boundary)
            .expect("validated on construction")
            .1
    }

    /// The same section moved by *offset*, re-canonicalised.
    pub fn translated(&self, offset: V2) -> Checked<Self> {
        PlanarSection::new(
            self.boundary
                .iter()
                .map(|v| SectionVertex {
                    point: [v.point[0] + offset[0], v.point[1] + offset[1]]
                        .map(py::without_negative_zero),
                    bulge: v.bulge,
                })
                .collect(),
        )
    }
}

/// Which ends of an occurrence are capped (`SectionEnds`): never both.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct SectionEnds {
    pub low_capped: bool,
    pub high_capped: bool,
}

impl SectionEnds {
    pub fn new(low_capped: bool, high_capped: bool) -> Checked<Self> {
        if low_capped && high_capped {
            return refuse("an occurrence cannot be capped at both ends");
        }
        Ok(SectionEnds {
            low_capped,
            high_capped,
        })
    }
}

/// A run-local body identity (`BodyRef`), only ever issued by a [`BodyRefIssuer`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BodyRef {
    issuer: u64,
    ordinal: usize,
    signature: Option<String>,
}

impl BodyRef {
    pub fn signature(&self) -> Option<&str> {
        self.signature.as_deref()
    }
}

/// Issues and validates one run's body identities (`BodyRefIssuer`).
#[derive(Debug)]
pub struct BodyRefIssuer {
    token: u64,
    issued: Vec<Option<String>>,
}

impl Default for BodyRefIssuer {
    fn default() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        BodyRefIssuer {
            token: NEXT.fetch_add(1, AtomicOrdering::Relaxed),
            issued: Vec::new(),
        }
    }
}

impl BodyRefIssuer {
    pub fn issue(&mut self, signature: Option<&str>) -> Checked<BodyRef> {
        if let Some(s) = signature {
            if s.is_empty() {
                return refuse("body signature must be a nonempty string or None");
            }
            if self.issued.iter().any(|t| t.as_deref() == Some(s)) {
                return refuse("body signature must be unambiguous within one run");
            }
        }
        self.issued.push(signature.map(str::to_owned));
        Ok(BodyRef {
            issuer: self.token,
            ordinal: self.issued.len() - 1,
            signature: signature.map(str::to_owned),
        })
    }

    pub fn validate(&self, body: &BodyRef) -> Checked<()> {
        if body.issuer != self.token || self.issued.get(body.ordinal) != Some(&body.signature) {
            return refuse("body reference was not issued by this run or was mutated");
        }
        Ok(())
    }
}

/// One placed section over a run interval of one body (`SectionOccurrence`): the frame
/// canonical for its run, the section centred on the origin, the origin across the run.
#[derive(Clone, Debug, PartialEq)]
pub struct SectionOccurrence {
    body: BodyRef,
    frame: LocalFrame,
    run_interval: (f64, f64),
    section: PlanarSection,
    ends: SectionEnds,
}

impl SectionOccurrence {
    pub fn new(
        body: BodyRef,
        frame: LocalFrame,
        run_interval: (f64, f64),
        section: PlanarSection,
        ends: SectionEnds,
    ) -> Checked<Self> {
        let occurrence = SectionOccurrence {
            body,
            frame,
            run_interval,
            section,
            ends,
        };
        validate_occurrence_value(&occurrence)?;
        Ok(occurrence)
    }

    pub fn body(&self) -> &BodyRef {
        &self.body
    }
    pub fn frame(&self) -> &LocalFrame {
        &self.frame
    }
    pub fn run_interval(&self) -> (f64, f64) {
        self.run_interval
    }
    pub fn section(&self) -> &PlanarSection {
        &self.section
    }
    pub fn ends(&self) -> SectionEnds {
        self.ends
    }
}

fn validate_occurrence_value(o: &SectionOccurrence) -> Checked<()> {
    let f = &o.frame;
    let canonical = LocalFrame::canonical(f.run, f.origin)?;
    if [
        (f.run, canonical.run),
        (f.u, canonical.u),
        (f.v, canonical.v),
    ]
    .iter()
    .any(|(a, b)| a.iter().zip(b).any(|(x, y)| (x - y).abs() > EPS))
    {
        return refuse("section occurrence requires the canonical run and in-plane basis");
    }
    let (lo, hi) = o.run_interval;
    if !finite(&[lo, hi]) || hi - lo <= EPS {
        return refuse("run interval must be finite and increasing");
    }
    if py::hypot(&o.section.centroid()) > EPS {
        return refuse("section occurrence requires an origin-centred intrinsic section");
    }
    if dot(f.origin, f.run).abs() > EPS {
        return refuse("section occurrence frame origin must be perpendicular to its run");
    }
    Ok(())
}

internal_entry! {
/// Revalidate an occurrence and its run-owned body (`validate_occurrence`).
pub fn validate_occurrence(o: &SectionOccurrence, body_refs: &BodyRefIssuer) -> Checked<()> {
    body_refs.validate(&o.body)?;
    validate_occurrence_value(o)
}
}

/// A published vertex (`section_vertex_dict`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct VertexValue {
    pub point: V2,
    pub bulge: f64,
}

/// The primitive-only version-1 proposal shape (`occurrence_geometry_dict`); the body identity
/// does not cross it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct OccurrenceGeometry {
    pub frame: LocalFrame,
    pub run_interval: (f64, f64),
    /// The frame's origin and the run interval with the values they were published from.
    #[serde(skip)]
    pub placed_origin: Placed<V3>,
    #[serde(skip)]
    pub placed_run_interval: Placed<(f64, f64)>,
    pub section: SectionValue,
    pub ends: SectionEnds,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct SectionValue {
    pub boundary: Vec<VertexValue>,
}

internal_entry! {
pub fn section_vertex_value(vertex: &SectionVertex) -> Checked<VertexValue> {
    let [x, y, bulge] = serialized(vertex)?;
    Ok(VertexValue {
        point: [x, y],
        bulge,
    })
}
}

internal_entry! {
/// The occurrence as published (`occurrence_geometry_dict`): origin and interval to 3
/// decimals, directions to 6, refused unless the rounded values still describe the geometry
/// within 2e-3.
pub fn occurrence_geometry(
    o: &SectionOccurrence,
    body_refs: &BodyRefIssuer,
) -> Checked<OccurrenceGeometry> {
    validate_occurrence(o, body_refs)?;
    let rounded = |v: V3, d: usize| v.map(|c| round_clean(c, d));
    let f = &o.frame;
    let (origin, run, u, v) = (
        rounded(f.origin, 3),
        rounded(f.run, 6),
        rounded(f.u, 6),
        rounded(f.v, 6),
    );
    let interval = (
        round_clean(o.run_interval.0, 3),
        round_clean(o.run_interval.1, 3),
    );
    if interval.1 <= interval.0 {
        return refuse("serialized run interval collapses");
    }
    let canonical = LocalFrame::canonical(run, [0.0; 3])?;
    let norm = |a: V3| dot(a, a).sqrt();
    if [run, u, v].iter().any(|&a| (norm(a) - 1.0).abs() > 1e-6)
        || [(run, u), (run, v), (u, v)]
            .iter()
            .any(|&(a, b)| dot(a, b).abs() > 2e-6)
        || py::dist(&cross(run, u), &v) > 3e-6
        || py::dist(&run, &canonical.run) > 1e-6
        || py::dist(&u, &canonical.u) > 3e-6
        || py::dist(&v, &canonical.v) > 3e-6
    {
        return refuse("serialized frame exceeds its canonical validation tolerances");
    }
    let perpendicular_bound = 0.000868 + 1e-6 * norm(origin);
    if dot(origin, run).abs() > perpendicular_bound {
        return refuse("serialized frame origin exceeds its perpendicularity tolerance");
    }
    let boundary = o.section.boundary();
    let projected = PlanarSection::new(
        boundary
            .iter()
            .map(|vertex| {
                let [x, y, bulge] = serialized(vertex)?;
                SectionVertex::new([x, y], bulge)
            })
            .collect::<Checked<_>>()?,
    )?;
    if py::hypot(&projected.centroid()) > POSITION_TOL {
        return refuse("serialized section centroid exceeds its canonical tolerance");
    }
    let origin_error = py::dist(&f.origin, &origin);
    let run_error = py::dist(&f.run, &run);
    let u_error = py::dist(&f.u, &u);
    let v_error = py::dist(&f.v, &v);
    let interval_error = (o.run_interval.0 - interval.0)
        .abs()
        .max((o.run_interval.1 - interval.1).abs());
    let mut transverse: f64 = f64::NEG_INFINITY;
    for (i, vertex) in boundary.iter().enumerate() {
        let extent = match arc(vertex, &boundary[(i + 1) % boundary.len()])? {
            None => py::hypot(&vertex.point),
            Some(a) => py::hypot(&a.centre) + a.radius,
        };
        transverse = transverse.max(extent);
    }
    let squares = |a: V3| py::sum(a.map(|c| c * c));
    let world_bound = origin_error
        + o.run_interval.0.abs().max(o.run_interval.1.abs()) * run_error
        + interval_error * squares(run).sqrt()
        + transverse * (u_error + v_error)
        + projection_bound(boundary)? * (squares(u) + squares(v)).sqrt();
    if world_bound > OCCURRENCE_TOL {
        return refuse("serialized occurrence moves its geometry beyond local tolerance");
    }
    Ok(OccurrenceGeometry {
        frame: LocalFrame { origin, run, u, v },
        run_interval: interval,
        placed_origin: Placed::new(f.origin, Grid::CleanDecimals(3)),
        placed_run_interval: Placed::new(o.run_interval, Grid::CleanDecimals(3)),
        section: SectionValue {
            boundary: boundary
                .iter()
                .map(section_vertex_value)
                .collect::<Checked<_>>()?,
        },
        ends: o.ends,
    })
}
}
