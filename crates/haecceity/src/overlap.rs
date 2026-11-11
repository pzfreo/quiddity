//! The common area of two coplanar planar faces (`BRepAlgoAPI_Common` of the two faces and the
//! surface area of the result), without a face boolean: both faces' boundary loops, placed and
//! projected into the first face's plane, intersected as polygons with holes (the `i_overlay`
//! crate, even-odd fill), and the area of the result. specify-core's contacts (the area two
//! facing flat faces of different parts share) read it.
//!
//! Straight edges are exact. Circular arcs are sampled afresh from their exact curves, finely
//! enough that a chord strays at most [`ARC_SAGITTA`] of the radius from its arc, so faces bounded
//! by lines and circles give the exact area to 1e-8 of a circle's. Other curves use the edge's
//! samples (within the sampling's chord tolerance, 2e-4 mm, of the curve), so their area can
//! differ from the exact one in the fourth decimal. A loop that does not close, or runs along an
//! edge whose curve did not resolve, is refused rather than closed by a guess.

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;

use crate::brep::{Edge, Part};
use crate::geom::{self, Curve, Surface, V3};
use crate::sampling::edge_interval;
use crate::step::{IDENTITY, Placement};

/// How far a chord of a circular boundary may stray from its arc, as a fraction of the radius:
/// about 70 000 chords to a full circle, so the area lost to the chords is about 4e-9 of the
/// circle's.
pub const ARC_SAGITTA: f64 = 1e-9;
/// How far apart, mm, consecutive edges of a loop (or its last and first points) may end and
/// the loop still close.
pub const LOOP_GAP: f64 = 0.01;
/// How far from parallel (the sine of the angle) two planes may be and count as coplanar.
const PARALLEL: f64 = 1e-6;

/// Why the common area of two faces was not measured.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct OverlapRefusal {
    pub reason: String,
}

impl std::fmt::Display for OverlapRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}

impl std::error::Error for OverlapRefusal {}

/// The area two planar faces have in common, each given as (part, face index), both parts in one
/// frame. See [`common_area_placed`].
pub fn common_area(a: (&Part, usize), b: (&Part, usize)) -> Result<f64, OverlapRefusal> {
    common_area_placed((a.0, a.1, &IDENTITY), (b.0, b.1, &IDENTITY))
}

/// The area two planar faces have in common, each given as (part, face index, the placement of
/// that part: rows of `[R | t]`, as [`crate::step::PartPlacement`] gives them): `b` is projected
/// into `a`'s plane and the two regions intersected there. How far apart the planes may be is
/// the caller's to decide (the projection does not look); the faces' orientations do not
/// matter. Refused, naming the face, when a face is not planar, the planes are not parallel, a
/// loop runs along an edge whose curve did not resolve, or a loop does not close (consecutive
/// edges, or its last and first points, more than [`LOOP_GAP`] apart).
pub fn common_area_placed(
    a: (&Part, usize, &Placement),
    b: (&Part, usize, &Placement),
) -> Result<f64, OverlapRefusal> {
    let refuse = |reason: String| Err(OverlapRefusal { reason });
    let frame = |(part, face, placement): (&Part, usize, &Placement)| match part.faces[face].surface
    {
        Surface::Plane { frame } => Ok((
            place_point(placement, frame.origin),
            place_direction(placement, frame.x),
            place_direction(placement, frame.y),
            place_direction(placement, frame.z),
        )),
        _ => Err(OverlapRefusal {
            reason: format!("face {face} is not planar"),
        }),
    };
    let (origin, x, y, z) = frame(a)?;
    let (_, _, _, zb) = frame(b)?;
    if geom::norm(geom::cross(z, zb)) > PARALLEL {
        return refuse(format!("faces {} and {} are not parallel", a.1, b.1));
    }
    let project = |(part, face, placement): (&Part, usize, &Placement)| {
        loops(part, face).map(|loops| {
            loops
                .iter()
                .map(|l| {
                    l.iter()
                        .map(|&p| {
                            let d = geom::sub(place_point(placement, p), origin);
                            [geom::dot(d, x), geom::dot(d, y)]
                        })
                        .collect::<Vec<[f64; 2]>>()
                })
                .collect::<Vec<_>>()
        })
    };
    let (subject, clip) = (project(a)?, project(b)?);
    let shapes = subject.overlay(&clip, OverlayRule::Intersect, FillRule::EvenOdd);
    // Each shape is an outer contour and its holes: the outer's area less the holes'.
    Ok(shapes
        .iter()
        .map(|shape| {
            let mut contours = shape.iter().map(|c| shoelace(c).abs());
            let outer = contours.next().unwrap_or(0.0);
            outer - contours.sum::<f64>()
        })
        .sum())
}

/// The face's boundary loops as closed polylines ([`edge_points`]), each edge run in its loop's
/// direction; a loop of one vertex (no area) is left out.
fn loops(part: &Part, face: usize) -> Result<Vec<Vec<V3>>, OverlapRefusal> {
    let refuse = |reason: String| Err(OverlapRefusal { reason });
    let mut out = Vec::new();
    for (k, lp) in part.faces[face].loops.iter().enumerate() {
        if lp.edges.is_empty() {
            continue;
        }
        let mut points: Vec<V3> = Vec::new();
        for &(e, forward) in &lp.edges {
            if part.unresolved_edges().contains(&e) {
                return refuse(format!(
                    "loop {k} of face {face} runs along edge {e}, whose curve did not resolve"
                ));
            }
            let mut run = edge_points(&part.edges[e]);
            if !forward {
                run.reverse();
            }
            let Some(&first) = run.first() else {
                return refuse(format!("edge {e} of face {face} has no samples"));
            };
            match points.last() {
                Some(&last) if geom::dist(last, first) > LOOP_GAP => {
                    return refuse(format!(
                        "loop {k} of face {face} does not close: edge {e} starts {:.3} mm from \
                         where the edge before it ends",
                        geom::dist(last, first)
                    ));
                }
                Some(_) => points.extend_from_slice(&run[1..]),
                None => points.extend_from_slice(&run),
            }
        }
        let (first, last) = (points[0], points[points.len() - 1]);
        if geom::dist(first, last) > LOOP_GAP {
            return refuse(format!(
                "loop {k} of face {face} does not close: it ends {:.3} mm from its start",
                geom::dist(first, last)
            ));
        }
        if points.len() > 1 {
            points.pop();
        }
        out.push(points);
    }
    Ok(out)
}

/// Points along `edge`, start to end: a circular arc sampled afresh from its exact curve (chords
/// within [`ARC_SAGITTA`] of its radius), any other edge its samples.
fn edge_points(edge: &Edge) -> Vec<V3> {
    let Curve::Circle { .. } = edge.curve else {
        return edge.samples.clone();
    };
    let (t0, t1) = edge_interval(
        &edge.curve,
        edge.start,
        edge.end,
        edge.same_sense,
        edge.is_closed(),
    );
    // A chord over angle θ strays r·(1 − cos(θ/2)) ≈ r·θ²/8 from its arc.
    let step = (8.0 * ARC_SAGITTA).sqrt();
    let n = (((t1 - t0).abs() / step).ceil() as usize).max(1);
    let mut points: Vec<V3> = (0..=n)
        .map(|i| edge.curve.value(t0 + (t1 - t0) * i as f64 / n as f64))
        .collect();
    // The ends are the edge's own vertices, so loops join exactly.
    points[0] = edge.start;
    points[n] = edge.end;
    points
}

/// The signed area of a closed polygon (last point not repeated).
fn shoelace(points: &[[f64; 2]]) -> f64 {
    let n = points.len();
    (0..n)
        .map(|i| {
            let (p, q) = (points[i], points[(i + 1) % n]);
            p[0] * q[1] - q[0] * p[1]
        })
        .sum::<f64>()
        / 2.0
}

/// `p` placed: `R p + t`.
fn place_point(placement: &Placement, p: V3) -> V3 {
    geom::add(place_direction(placement, p), placement.map(|row| row[3]))
}

/// `d` turned: `R d`.
fn place_direction(placement: &Placement, d: V3) -> V3 {
    placement.map(|row| row[0] * d[0] + row[1] * d[1] + row[2] * d[2])
}
