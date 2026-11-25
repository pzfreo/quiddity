//! Hidden-line projection and section views — what OpenCascade's `HLRBRep` gives build123d's
//! `project_to_viewport`, and draftwright's section A–A without its boolean: a part's edges and
//! silhouettes seen along a direction, each split into its visible and hidden stretches.
//!
//! The curves drawn are the solids' B-rep edges (less seams and the joins between pieces of one
//! surface, which OpenCascade leaves out) — sharp, or smooth where the faces meet tangentially —
//! and each curved face's silhouette, the contour where its normal is perpendicular to the view.
//! Seen in the view, a curve can only pass behind or come out from behind something where its
//! projection crosses another curve's or ends on one, so each curve is cut at every such point
//! and each piece's visibility decided once, exactly, by a ray from its middle towards the
//! viewer (a drawing with a piece no ray can decide is refused).
//!
//! A section keeps the curves on one side of a plane and adds the plane's own contours across
//! the faces; a ray then runs only to the plane, where it is stopped by the cut face if it
//! arrives inside the material.

use std::collections::HashMap;

use super::brep::{Arc, Part};
use super::classify::{Classifier, ON_TOLERANCE, State};
use super::geom::{self, Curve, Surface, V3};
use super::rays::{RayCaster, polyline_distance};

/// How a projected curve arises (OpenCascade's sharp `V`/`H`, `Rg1Line` and `OutLine`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    Sharp,
    Smooth,
    Outline,
}

/// The Part-local B-rep element from which a projected curve originates.
///
/// Silhouettes and section contours traced on a face report `Face`; model edges and section
/// boundaries that follow an existing edge report `Edge`. These are indices into the current
/// `Part`, not STEP entity numbers; use [`Part::face_source`] or [`Part::edge_source`] to map them
/// to STEP source records. They are valid only for the `Part` they came from: re-exporting or
/// rereading the model may renumber them, so do not store them as lasting identifiers.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProjectionOrigin {
    /// A model edge or section boundary that follows that edge.
    Edge(usize),
    /// A silhouette or a section contour traced over that face.
    Face(usize),
}

/// One stretch of a curve as seen: in view coordinates, and visible or hidden.
#[derive(Clone, Debug)]
pub struct Projected {
    pub class: Class,
    pub visible: bool,
    pub points: Vec<[f64; 2]>,
    /// The stretch exactly, when it is a stretch of an edge whose curve projects to a curve of
    /// its own kind (silhouettes and section outlines are traced, and have only their points).
    pub exact: Option<Exact>,
}

/// A projected curve exactly, in view coordinates, from its first point to its last.
#[derive(Clone, Debug, PartialEq)]
pub enum Exact {
    Line([f64; 2], [f64; 2]),
    /// `centre + radii[0] cos t x_axis + radii[1] sin t y_axis`, for t from `t0` to `t1`
    /// (the axes perpendicular unit vectors, `radii[0] >= radii[1]`).
    Conic {
        centre: [f64; 2],
        radii: [f64; 2],
        x_axis: [f64; 2],
        y_axis: [f64; 2],
        t0: f64,
        t1: f64,
    },
    /// A rational B-spline (its poles projected, its weights and knots unchanged), for t from
    /// `t0` to `t1`.
    Nurbs {
        degree: usize,
        poles: Vec<[f64; 2]>,
        weights: Vec<f64>,
        knots: Vec<f64>,
        t0: f64,
        t1: f64,
    },
}

impl Exact {
    /// The point at parameter *t* (for a line, the fraction along it).
    pub fn at(&self, t: f64) -> [f64; 2] {
        match self {
            Exact::Line(a, b) => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t],
            Exact::Conic {
                centre,
                radii,
                x_axis,
                y_axis,
                ..
            } => {
                let (c, s) = (radii[0] * t.cos(), radii[1] * t.sin());
                [
                    centre[0] + c * x_axis[0] + s * y_axis[0],
                    centre[1] + c * x_axis[1] + s * y_axis[1],
                ]
            }
            Exact::Nurbs {
                degree,
                poles,
                weights,
                knots,
                ..
            } => {
                let p: Vec<V3> = poles.iter().map(|q| [q[0], q[1], 0.0]).collect();
                #[allow(
                    clippy::expect_used,
                    reason = "exact() projects the poles of a curve NurbsCurve::new accepted"
                )]
                let curve =
                    super::nurbs::NurbsCurve::new(*degree, p, weights.clone(), knots.clone())
                        .expect("projected from a well-formed curve");
                let q = curve.value(t);
                [q[0], q[1]]
            }
        }
    }

    /// The parameter range drawn, first to last.
    pub fn range(&self) -> (f64, f64) {
        match self {
            Exact::Line(..) => (0.0, 1.0),
            Exact::Conic { t0, t1, .. } | Exact::Nurbs { t0, t1, .. } => (*t0, *t1),
        }
    }
}

/// The stretch *run* (points along *curve*, in order) seen in *view*, exactly: lines stay
/// lines, a circle or an ellipse becomes an ellipse (by its principal axes), a NURBS curve the
/// curve of its projected poles. `None` where the curve is seen edge-on or its parameters do
/// not run steadily along the stretch.
fn exact(curve: &Curve, view: &View, run: &[V3]) -> Option<Exact> {
    let (first, last) = (view.map(run[0]), view.map(*run.last()?));
    if let Curve::Line { .. } = curve {
        return Some(Exact::Line(first, last));
    }
    // The stretch's parameters, unwrapped along it where the curve is closed.
    let period = match curve {
        Curve::Circle { .. } | Curve::Ellipse { .. } => Some(std::f64::consts::TAU),
        Curve::Nurbs(n) if n.is_closed() => Some(n.domain().1 - n.domain().0),
        _ => None,
    };
    let mut ts: Vec<f64> = Vec::with_capacity(run.len());
    for p in run {
        let mut t = curve.parameter(*p);
        if let (Some(period), Some(&prev)) = (period, ts.last()) {
            t += period * ((prev - t) / period).round();
        }
        ts.push(t);
    }
    let rising = ts[ts.len() - 1] > ts[0];
    if ts
        .windows(2)
        .any(|w| (w[1] > w[0]) != rising && (w[1] - w[0]).abs() > 1e-9)
    {
        return None;
    }
    let (t0, t1) = (ts[0], ts[ts.len() - 1]);
    match curve {
        Curve::Circle { frame, radius } => conic(
            view,
            frame.origin,
            geom::scale(frame.x, *radius),
            geom::scale(frame.y, *radius),
            t0,
            t1,
        ),
        Curve::Ellipse {
            frame,
            major,
            minor,
        } => conic(
            view,
            frame.origin,
            geom::scale(frame.x, *major),
            geom::scale(frame.y, *minor),
            t0,
            t1,
        ),
        Curve::Nurbs(n) => Some(Exact::Nurbs {
            degree: n.degree(),
            poles: n.control_points().iter().map(|p| view.map(*p)).collect(),
            weights: n.weights().to_vec(),
            knots: n.knots().to_vec(),
            t0,
            t1,
        }),
        #[allow(clippy::unreachable, reason = "lines return above")]
        Curve::Line { .. } => unreachable!("returned above"),
    }
}

/// The projection of `centre + cos t a + sin t b` (a and b conjugate semi-diameters once seen)
/// as an ellipse by its principal axes, its parameter shifted to match.
fn conic(view: &View, centre: V3, a: V3, b: V3, t0: f64, t1: f64) -> Option<Exact> {
    let (a, b) = (view.map(a), view.map(b));
    let dot = |p: [f64; 2], q: [f64; 2]| p[0] * q[0] + p[1] * q[1];
    // At the principal axes the derivative is perpendicular to the radius.
    let shift = 0.5 * (2.0 * dot(a, b)).atan2(dot(a, a) - dot(b, b));
    let (c, s) = (shift.cos(), shift.sin());
    let mut major = [a[0] * c + b[0] * s, a[1] * c + b[1] * s];
    let mut minor = [-a[0] * s + b[0] * c, -a[1] * s + b[1] * c];
    let (mut ra, mut rb) = (dot(major, major).sqrt(), dot(minor, minor).sqrt());
    let mut shift = shift;
    if rb > ra {
        // The other principal axis is the longer: start a quarter turn on.
        (major, minor) = (minor, [-major[0], -major[1]]);
        (ra, rb) = (rb, ra);
        shift += std::f64::consts::FRAC_PI_2;
    }
    if rb <= 1e-9 * ra {
        return None; // seen edge-on: a segment, drawn by its points
    }
    Some(Exact::Conic {
        centre: view.map(centre),
        radii: [ra, rb],
        x_axis: [major[0] / ra, major[1] / ra],
        y_axis: [minor[0] / rb, minor[1] / rb],
        t0: t0 - shift,
        t1: t1 - shift,
    })
}

/// An orthographic view: looking along `-toward` (`toward` points at the viewer), with `up`
/// the view's y.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub x: V3,
    pub y: V3,
    pub toward: V3,
}

impl View {
    pub fn new(toward: V3, up: V3) -> Option<Self> {
        let toward = geom::unit(toward)?;
        let x = geom::unit(geom::cross(up, toward))?;
        Some(View {
            x,
            y: geom::cross(toward, x),
            toward,
        })
    }

    pub fn map(&self, p: V3) -> [f64; 2] {
        [geom::dot(p, self.x), geom::dot(p, self.y)]
    }
}

/// Grid cells across a face's parameter range, each way, when tracing a contour.
const CONTOUR_GRID: usize = 64;

/// A cutting plane: the material kept lies on the side `normal` points to.
#[derive(Clone, Copy, Debug)]
pub struct Plane {
    pub origin: V3,
    pub normal: V3,
}

impl Plane {
    pub fn new(origin: V3, normal: V3) -> Option<Self> {
        Some(Plane {
            origin,
            normal: geom::unit(normal)?,
        })
    }

    /// Signed distance from the plane, positive on the kept side.
    pub fn distance(&self, p: V3) -> f64 {
        geom::dot(geom::sub(p, self.origin), self.normal)
    }
}

/// The drawn faces and their edges whose geometry did not resolve on reading
/// ([`Part::unresolved_faces`], [`Part::unresolved_edges`]). Such a face can neither be drawn
/// nor hide anything, and such an edge has only its chord, so a drawing of the part is refused
/// rather than drawn wrong. Alike when a stretch's visibility cannot be decided (a ray from it
/// reaches a face the kernel cannot intersect, and nothing it can intersect stops it): then
/// `faces` are those faces and `edges` the stretch's edge (none for a silhouette or a section's
/// outline).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unresolved {
    pub faces: Vec<usize>,
    pub edges: Vec<usize>,
}

impl std::fmt::Display for Unresolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "geometry did not resolve: faces {:?}, edges {:?}",
            self.faces, self.edges
        )
    }
}

impl std::error::Error for Unresolved {}

/// The part's edges and silhouettes seen in *view*, split into visible and hidden stretches.
pub fn project(part: &Part, view: &View) -> Result<Vec<Projected>, Unresolved> {
    Ok(draw(part, view, None)?
        .into_iter()
        .map(|(projected, _)| projected)
        .collect())
}

/// Project the part and return the origin of each visible or hidden stretch.
///
/// Each origin is a Part-local edge or face index; see [`ProjectionOrigin`]. Returns the same
/// projected geometry and visibility as [`project`], paired with its origin.
pub fn project_with_origin(
    part: &Part,
    view: &View,
) -> Result<Vec<(Projected, ProjectionOrigin)>, Unresolved> {
    draw(part, view, None)
}

/// A section view: the part cut by *plane*, the half on its normal's side kept and seen in
/// *view* (from the side removed), with the section's outline drawn as sharp edges.
pub fn project_section(
    part: &Part,
    view: &View,
    plane: &Plane,
) -> Result<Vec<Projected>, Unresolved> {
    Ok(draw(part, view, Some(plane))?
        .into_iter()
        .map(|(projected, _)| projected)
        .collect())
}

/// Project a section and return the origin of each visible or hidden stretch.
///
/// Surface-plane contours originate from their face; section boundaries following an existing
/// edge originate from that edge. Origins are Part-local indices; see [`ProjectionOrigin`].
/// Returns the same projected geometry and visibility as [`project_section`], paired with its
/// origin.
pub fn project_section_with_origin(
    part: &Part,
    view: &View,
    plane: &Plane,
) -> Result<Vec<(Projected, ProjectionOrigin)>, Unresolved> {
    draw(part, view, Some(plane))
}

/// Where *plane* cuts the part: each face's curve of intersection with it, and each edge
/// lying in it that bounds material on one side. The cut face, which draftwright hatches by
/// the even-odd rule, is the region these bound.
pub fn section(part: &Part, plane: &Plane) -> Result<Vec<Vec<V3>>, Unresolved> {
    let faces = drawn_faces(part);
    resolved(part, &faces)?;
    Ok(cut(part, plane, &faces)?
        .into_iter()
        .map(|(curve, _)| curve)
        .collect())
}

/// Refuses *faces* when any of them, or any of their edges, did not resolve.
fn resolved(part: &Part, faces: &[usize]) -> Result<(), Unresolved> {
    let unresolved = Unresolved {
        faces: faces
            .iter()
            .copied()
            .filter(|f| part.unresolved_faces().contains(f))
            .collect(),
        edges: part
            .unresolved_edges()
            .iter()
            .copied()
            .filter(|&e| part.edge_faces()[e].iter().any(|f| faces.contains(f)))
            .collect(),
    };
    if unresolved.faces.is_empty() && unresolved.edges.is_empty() {
        Ok(())
    } else {
        Err(unresolved)
    }
}

/// [`section`] across *faces*, the drawn faces.
///
/// The cut face is the material on both sides of the plane. Where the part crosses the plane,
/// its outline is the crossing faces' contours. Where faces of the part lie in the plane (a
/// part split there, a step face at the plane's height), their surface does not cross it, so
/// they have no contour: the outline there is the edges lying in the plane, each kept where the
/// plane has material across it on one side only. A point on a face lying in the plane has
/// material on one side of the plane alone, so it is outside the cut face, as a point of no
/// material is. Lying in the plane is decided within [`ON_TOLERANCE`], the kernel's tolerance
/// for a point on a face: a face whose distance from the plane stays within it on one side
/// touches the plane rather than crossing it, and a face lying in the plane within it puts its
/// vertices in the plane, as the file's topology does, wherever their coordinates stand; and a
/// contour running along the face's own edge, within the edge's stand-off from the face, is
/// that edge. An edge in the plane whose sides cannot be classified, and that is not a seam or
/// join of one surface crossing the plane, is refused.
fn cut(
    part: &Part,
    plane: &Plane,
    faces: &[usize],
) -> Result<Vec<(Vec<V3>, ProjectionOrigin)>, Unresolved> {
    let mut edges: Vec<usize> = faces.iter().flat_map(|&f| part.face_edges(f)).collect();
    edges.sort_unstable();
    edges.dedup();
    // The vertices of the faces lying in the plane: a plane parallel to it, within the
    // tolerance across the face's extent.
    let mut in_plane: Vec<usize> = faces
        .iter()
        .filter(|&&f| match &part.faces[f].surface {
            Surface::Plane { frame } => {
                let tilt = geom::norm(geom::cross(frame.z, plane.normal));
                plane.distance(frame.origin).abs() <= ON_TOLERANCE
                    && tilt * part.face_bounds(f).diagonal() <= ON_TOLERANCE
            }
            _ => false,
        })
        .flat_map(|&f| part.face_edges(f))
        .flat_map(|e| [part.edges[e].vertices.0, part.edges[e].vertices.1])
        .collect();
    in_plane.sort_unstable();
    in_plane.dedup();
    let mut out = Vec::new();
    for &face in faces {
        let surface = &part.faces[face].surface;
        let g = |(u, v): (f64, f64)| plane.distance(surface.value(u, v));
        out.extend(
            contour(part, face, &g, Some((ON_TOLERANCE, &in_plane)))
                .into_iter()
                .map(|curve| (curve, ProjectionOrigin::Face(face))),
        );
    }
    let classifier = Classifier::for_faces(part, faces.to_vec());
    let step = 1e-4 * classifier.rays().bounds().diagonal().max(1.0);
    for e in edges {
        let edge = &part.edges[e];
        let s = &edge.samples;
        let near = |p: &V3| plane.distance(*p).abs() <= ON_TOLERANCE;
        let end_in = |p: &V3, v: usize| near(p) || in_plane.binary_search(&v).is_ok();
        if s.len() < 2
            || !s[1..s.len() - 1].iter().all(near)
            || !end_in(&s[0], edge.vertices.0)
            || !end_in(&s[s.len() - 1], edge.vertices.1)
        {
            continue;
        }
        let k = s.len() / 2;
        let along = geom::sub(s[k], s[k - 1]);
        let mid = geom::scale(geom::add(s[k], s[k - 1]), 0.5);
        let mid = edge.curve.value(edge.curve.parameter(mid));
        let Some(across) = geom::unit(geom::cross(plane.normal, along)) else {
            continue;
        };
        let side = |side: f64| match classifier
            .classify(geom::add(mid, geom::scale(across, side * step)))
        {
            State::In => Some(true),
            State::Out | State::On => Some(false),
            State::Unknown => None,
        };
        match (side(1.0), side(-1.0)) {
            (Some(a), Some(b)) => {
                if a != b {
                    out.push((s.clone(), ProjectionOrigin::Edge(e)));
                }
            }
            // Where the classifier cannot say, a seam or a join within one surface that crosses
            // the plane there (its normal off the plane's by more than the tolerance across the
            // probe's step) has material across it on one side only, by the surface alone.
            _ if crosses_along(part, e, mid, plane, step) => {
                out.push((s.clone(), ProjectionOrigin::Edge(e)));
            }
            _ => {
                return Err(Unresolved {
                    faces: part.edge_faces()[e].clone(),
                    edges: vec![e],
                });
            }
        }
    }
    Ok(out)
}

/// Whether edge *e*, lying in *plane*, is a seam or a join within one surface that crosses the
/// plane at *mid*, a point of the edge: the surface tilts off the plane by more than
/// [`ON_TOLERANCE`] over *step*.
fn crosses_along(part: &Part, e: usize, mid: V3, plane: &Plane, step: f64) -> bool {
    let faces = &part.edge_faces()[e];
    let one_surface = match faces.as_slice() {
        [_] => true,
        [a, b] => same_surface(&part.faces[*a].surface, &part.faces[*b].surface),
        _ => false,
    };
    let surface = &part.faces[faces[0]].surface;
    one_surface
        && surface
            .parameters(mid, None)
            .and_then(|(u, v)| surface.normal(u, v))
            .is_some_and(|n| geom::norm(geom::cross(n, plane.normal)) * step > ON_TOLERANCE)
}

/// The faces drawn: the solids' (as draftwright projects them, leaving out loose sheets and
/// construction faces), or every face of a part without solids.
fn drawn_faces(part: &Part) -> Vec<usize> {
    let mut faces: Vec<usize> = part
        .solids
        .iter()
        .flat_map(|s| s.faces.iter().copied())
        .collect();
    if faces.is_empty() {
        faces = (0..part.faces.len()).collect();
    }
    faces.sort_unstable();
    faces.dedup();
    faces
}

/// What a drawn curve lies on, which decides where its pieces are judged.
#[derive(Clone, Copy)]
enum On<'p> {
    Edge(&'p Curve, usize),
    /// A face's silhouette.
    Silhouette(usize),
    /// A section's outline, its chords in the cutting plane, and their model origin.
    Section(ProjectionOrigin),
}

fn draw(
    part: &Part,
    view: &View,
    plane: Option<&Plane>,
) -> Result<Vec<(Projected, ProjectionOrigin)>, Unresolved> {
    let faces = drawn_faces(part);
    resolved(part, &faces)?;
    let rays = RayCaster::for_faces(part, faces.clone());
    let scale = rays.bounds().diagonal().max(1.0);
    // Visibility asks only whether some face stops the ray, so the faces are asked in two
    // groups, those a ray meets in closed form first: a ray they stop never reaches the
    // freeform faces, whose intersection is costly. The answer is the one all the faces give
    // together (each group judges a boundary contact with the same tolerance).
    let groups: Vec<RayCaster> = {
        let (freeform, closed): (Vec<usize>, Vec<usize>) = faces
            .iter()
            .partition(|&&f| matches!(part.faces[f].surface, Surface::Freeform { .. }));
        [closed, freeform]
            .into_iter()
            .filter(|g| !g.is_empty())
            .map(|g| {
                let mut group = RayCaster::for_faces(part, g);
                group.edge_tol = rays.edge_tol;
                group
            })
            .collect()
    };
    let any_hit = |p: V3, start: f64, end: f64| -> Option<bool> {
        let mut unanswered = false;
        for group in &groups {
            match group.any_hit(p, view.toward, start, end) {
                Some(true) => return Some(true),
                Some(false) => {}
                None => unanswered = true,
            }
        }
        (!unanswered).then_some(false)
    };
    let mut curves: Vec<(Class, Vec<V3>, On)> = edges(part, view, &faces);
    for &face in &faces {
        let found = silhouette(part, face, view);
        curves.extend(
            found
                .into_iter()
                .map(|c| (Class::Outline, c, On::Silhouette(face))),
        );
    }
    let classifier = plane.map(|_| Classifier::for_faces(part, faces.clone()));
    if let Some(plane) = plane {
        curves = curves
            .into_iter()
            .flat_map(|(class, c, on)| {
                let curve = match on {
                    On::Edge(curve, _) => Some(curve),
                    _ => None,
                };
                kept(&c, plane, 1e-9 * scale, curve)
                    .into_iter()
                    .map(move |k| (class, k, on))
            })
            .collect();
        curves.extend(
            cut(part, plane, &faces)?
                .into_iter()
                .map(|(c, origin)| (Class::Sharp, c, On::Section(origin))),
        );
    }
    // Curves seen end-on draw nothing.
    curves.retain(|(_, c, _)| {
        c.windows(2)
            .map(|w| {
                let (a, b) = (view.map(w[0]), view.map(w[1]));
                (a[0] - b[0]).hypot(a[1] - b[1])
            })
            .sum::<f64>()
            > 1e-9 * scale
    });
    let flat: Vec<Vec<[f64; 2]>> = curves
        .iter()
        .map(|(_, c, _)| c.iter().map(|p| view.map(*p)).collect())
        .collect();
    let cuts = crossings(&flat, scale / 64.0, 1e-9 * scale);
    let reach = 2.0 * scale + 1.0;
    // Hidden from *p*, the ray to the viewer starting *start* along; `None` when that is not
    // known: the ray reaches a face the kernel cannot intersect, and no face it can intersect
    // (nor, in a section, the cut face) stops it.
    let hidden = |p: V3, start: f64| -> Option<bool> {
        let Some((plane, classifier)) = plane.zip(classifier.as_ref()) else {
            return any_hit(p, start, reach);
        };
        // Cut away: the ray runs only to the plane, and is stopped there by the cut face when
        // it reaches the plane within the material.
        let rate = -geom::dot(view.toward, plane.normal);
        let to_plane = if rate > 0.0 {
            plane.distance(p) / rate
        } else {
            reach
        };
        if to_plane <= 1e-6 * scale {
            return Some(false);
        }
        let met = any_hit(p, start, to_plane.min(reach));
        if met == Some(true)
            || (to_plane < reach
                && classifier.classify(geom::add(p, geom::scale(view.toward, to_plane)))
                    == State::In)
        {
            return Some(true);
        }
        met
    };
    // Each curve's stretches, the curves shared out over every core (each judged alone, so the
    // drawing does not depend on how): the costliest first, so that none is left to the end.
    let mut order: Vec<usize> = (0..curves.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(curves[i].1.len() + cuts[i].len()));
    let crack_free = |face: usize| {
        part.edge_deviation(face)
            .iter()
            .all(|&(_, d)| d <= 1e-6 * scale)
    };
    // On *face*, a hair outside it: the ray to the viewer runs along the face where the face
    // turns away, and from just inside it (a chord's middle, or a join standing off the
    // surface) would strike it again.
    let off_face = |face: usize, p: V3| {
        let surface = &part.faces[face].surface;
        surface
            .parameters(p, None)
            .and_then(|(u, v)| {
                let out = part.face_normal(face, u, v)?;
                Some(geom::add(
                    surface.value(u, v),
                    geom::scale(out, 1e-5 * scale),
                ))
            })
            .unwrap_or(p)
    };
    let judged = parallel(&order, |i| {
        let (class, points, on) = &curves[i];
        // An edge standing off its faces (within the file's tolerance) can start its ray just
        // inside them: the ray starts clear of that stand-off, so the edge does not hide itself.
        let start = 1e-6 * scale
            + match on {
                On::Edge(_, e) => {
                    let stand_off = part.edge_faces()[*e]
                        .iter()
                        .flat_map(|&f| part.edge_deviation(f).iter())
                        .filter(|(x, _)| x == e)
                        .map(|&(_, d)| d)
                        .fold(0.0, f64::max);
                    4.0 * stand_off
                }
                _ => 0.0,
            };
        let judge = |p: V3| {
            let at = match on {
                // A seam or join within one surface (or a curvature-continuous blend) on the
                // outline is the surface's silhouette, judged as one: off its first drawn face
                // (its faces are tangent there). From the curve, which may stand a hair inside
                // the surface (ctc05's edge 375, 1.5e-8), the ray grazing the surface would meet
                // the face across the join.
                On::Edge(c, e) if *class == Class::Outline => {
                    let at = c.value(c.parameter(p));
                    part.edge_faces()[*e]
                        .iter()
                        .find(|f| faces.binary_search(f).is_ok())
                        .map_or(at, |&face| off_face(face, at))
                }
                // On the curve itself: a chord's middle can lie exactly on another face's
                // boundary drawn by the same samples.
                On::Edge(c, _) => c.value(c.parameter(p)),
                // From a chord's middle (inside a curved face) the ray would strike the face.
                On::Silhouette(face) => off_face(*face, p),
                On::Section(_) => p,
            };
            let verdict = hidden(at, start).ok_or(at)?;
            match on {
                On::Edge(_, e) if verdict && start > 1e-6 * scale => {
                    let ray = (view, reach, 1e-6 * scale);
                    let hidden_from = |start: f64| hidden(at, start);
                    Ok(
                        clear_of_own_faces(part, &rays, *e, at, ray, start, hidden_from)
                            .ok_or(at)?,
                    )
                }
                // A hair off a hole's wall the ray can leave through the hole's mouth, where from
                // the wall itself it runs into the material: from the face, any other face it
                // meets (before the plane, in a section) stands in the way. Only on a face its
                // edges lie on (within 1e-6 of the part's size): along the cracks a file leaves
                // between faces, a neighbour's band would stop a ray that only passes it.
                On::Silhouette(face) if !verdict && crack_free(*face) => {
                    let surface = &part.faces[*face].surface;
                    let Some(on_face) = surface
                        .parameters(p, None)
                        .map(|(u, v)| surface.value(u, v))
                    else {
                        return Ok(false);
                    };
                    let end = plane.map_or(reach, |plane| {
                        let rate = -geom::dot(view.toward, plane.normal);
                        if rate > 0.0 {
                            (plane.distance(on_face) / rate).min(reach)
                        } else {
                            reach
                        }
                    });
                    let ray = (on_face, view.toward, end);
                    Ok(
                        match rays.other_hit(ray, &[*face], (start, f64::INFINITY)) {
                            Some(met) => met,
                            None => rays.hits(on_face, view.toward, end).is_some_and(|hits| {
                                hits.iter().any(|h| h.face != *face && h.t > start)
                            }),
                        },
                    )
                }
                _ => Ok(verdict),
            }
        };
        // A stretch's visibility can ask along the same ray up to four times (`hidden`, the
        // edge's own faces, the faces before them and `hidden` again from further along).
        let judge = |p: V3| super::rays::remembering(|| judge(p));
        pieces(points, &cuts[i], (view, 1e-9 * scale), &judge)
    });
    let mut out = Vec::new();
    for ((class, _, on), runs) in curves.iter().zip(judged) {
        // A stretch whose visibility is not known is drawn neither way: the drawing is refused,
        // naming the faces its ray could not be intersected with, and its edge.
        let runs = runs.map_err(|at| Unresolved {
            faces: faces
                .iter()
                .copied()
                .filter(|&f| {
                    part.faces[f]
                        .surface
                        .ray_hits(at, view.toward, reach)
                        .is_none()
                })
                .collect(),
            edges: match on {
                On::Edge(_, e) => vec![*e],
                _ => Vec::new(),
            },
        })?;
        for (visible, run) in runs {
            let exact = match on {
                On::Edge(curve, _) => exact(curve, view, &run),
                _ => None,
            };
            let origin = match on {
                On::Edge(_, edge) => ProjectionOrigin::Edge(*edge),
                On::Silhouette(face) => ProjectionOrigin::Face(*face),
                On::Section(origin) => *origin,
            };
            out.push((
                Projected {
                    class: *class,
                    visible,
                    points: run.iter().map(|p| view.map(*p)).collect(),
                    exact,
                },
                origin,
            ));
        }
    }
    Ok(out)
}

/// Where the ray from *p*, on edge *e* standing off its faces, may start so that it does not
/// meet one of those faces merely by crossing the stand-off. At a shallow slant (a rim seen
/// past its own face at a grazing angle) the ray runs on through the band the stand-off leaves
/// for far longer than the 4 d *start* allows for. So each of the edge's faces the ray meets is
/// asked again from *p* moved onto it (where the edge would lie were it exact): when the face
/// does not stand in the way of that ray (beyond `base`, the start of an exact edge's ray), the
/// ray may start past its meetings with it, unless another face is met first. The answer is
/// *hidden_from* that start: whether the ray from *p* starting there is stopped.
fn clear_of_own_faces(
    part: &Part,
    rays: &RayCaster,
    e: usize,
    p: V3,
    (view, reach, base): (&View, f64, f64),
    start: f64,
    hidden_from: impl Fn(f64) -> Option<bool>,
) -> Option<bool> {
    let own = &part.edge_faces()[e];
    // Every hit along the ray is costly (a ray along a thread's axis crosses every turn), and
    // only the edge's own faces' hits are read until the ray may start later: those are taken
    // from the faces alone wherever that gives the same answer (`RayCaster::hits_on`).
    let mut all: Option<Vec<super::rays::Hit>> = None;
    let mine = match rays.hits_on(own, p, view.toward, reach) {
        Some(hits) => hits,
        None => {
            let Some(hits) = rays.hits(p, view.toward, reach) else {
                return hidden_from(start);
            };
            all.insert(hits).clone()
        }
    };
    // The ray starts at most just past the own faces' last meetings. Stopped from there, it is
    // stopped from wherever it starts before (a later start only leaves out meetings), so
    // asking the faces again is needed only when it is not.
    let latest = own
        .iter()
        .filter_map(|&face| {
            mine.iter()
                .filter(|h| h.face == face && h.t > start)
                .map(|h| h.t * (1.0 + 1e-9) + 1e-12)
                .reduce(f64::max)
        })
        .fold(start, f64::max);
    if latest > start && hidden_from(latest) == Some(true) {
        return Some(true);
    }
    let mut clear = start;
    for &face in own {
        let Some(last) = mine
            .iter()
            .filter(|h| h.face == face && h.t > start)
            .map(|h| h.t)
            .reduce(f64::max)
        else {
            continue;
        };
        let surface = &part.faces[face].surface;
        let Some(on) = surface
            .parameters(p, None)
            .map(|(u, v)| surface.value(u, v))
        else {
            continue;
        };
        let again = match rays.hits_on(&[face], on, view.toward, reach) {
            Some(hits) => Some(hits),
            None => rays.hits(on, view.toward, reach),
        };
        if again.is_some_and(|hits| !hits.iter().any(|h| h.face == face && h.t > base)) {
            clear = clear.max(last * (1.0 + 1e-9) + 1e-12);
        }
    }
    if clear == start {
        return hidden_from(start);
    }
    // Any other face met before that hides the edge itself.
    let other = match rays.other_hit((p, view.toward, reach), own, (start, clear)) {
        Some(met) => met,
        None => {
            let Some(hits) = all.or_else(|| rays.hits(p, view.toward, reach)) else {
                return hidden_from(start);
            };
            hits.iter()
                .any(|h| !own.contains(&h.face) && h.t > start && h.t <= clear)
        }
    };
    hidden_from(if other { start } else { clear })
}

/// `f` of each of *items* (a permutation of `0..items.len()`), on every core, each thread taking
/// the next item left; the results in the items' order as numbers, not as listed.
fn parallel<R: Send>(items: &[usize], f: impl Fn(usize) -> R + Sync) -> Vec<R> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    // HAECCEITY_THREADS caps the threads (benchmarks: 1 times HLR as wasm32 runs it).
    let cap = std::env::var("HAECCEITY_THREADS")
        .ok()
        .and_then(|v| v.parse().ok());
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(cap.unwrap_or(usize::MAX))
        .min(items.len());
    // One thread, or none to be had (wasm32-unknown-unknown spawns none): work here, in order.
    if threads <= 1 {
        let mut done: Vec<(usize, R)> = items.iter().map(|&i| (i, f(i))).collect();
        done.sort_by_key(|(i, _)| *i);
        return done.into_iter().map(|(_, r)| r).collect();
    }
    let next = AtomicUsize::new(0);
    let mut done: Vec<(usize, R)> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut mine = Vec::new();
                    while let Some(&i) = items.get(next.fetch_add(1, Ordering::Relaxed)) {
                        mine.push((i, f(i)));
                    }
                    mine
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
            .collect()
    });
    done.sort_by_key(|(i, _)| *i);
    done.into_iter().map(|(_, r)| r).collect()
}

/// The stretches of polyline *c* on the plane's kept side (or within *tol* of it), each
/// crossing placed on the chord and moved onto *curve* when there is one.
fn kept(c: &[V3], plane: &Plane, tol: f64, curve: Option<&Curve>) -> Vec<Vec<V3>> {
    let d: Vec<f64> = c.iter().map(|p| plane.distance(*p)).collect();
    let crossing = |i: usize| {
        let t = (d[i] / (d[i] - d[i + 1])).clamp(0.0, 1.0);
        let p = geom::add(c[i], geom::scale(geom::sub(c[i + 1], c[i]), t));
        curve.map_or(p, |k| k.value(k.parameter(p)))
    };
    let mut out = Vec::new();
    let mut run: Vec<V3> = Vec::new();
    for i in 0..c.len() {
        let inside = d[i] >= -tol;
        if inside {
            if run.is_empty() && i > 0 {
                run.push(crossing(i - 1));
            }
            run.push(c[i]);
        } else if !run.is_empty() {
            run.push(crossing(i - 1));
            out.push(std::mem::take(&mut run));
        }
    }
    if run.len() > 1 {
        out.push(run);
    }
    out.retain(|r| r.len() > 1);
    out
}

/// The edges drawn, of the *drawn* faces: every edge between two that is not a seam or a join within one
/// surface, classed smooth where the faces meet tangentially, and every free edge. A seam or
/// join that runs along the surface's silhouette is drawn, as the outline it is there.
fn edges<'p>(part: &'p Part, view: &View, drawn: &[usize]) -> Vec<(Class, Vec<V3>, On<'p>)> {
    let on_outline = |e: usize, face: usize| {
        // On the curve halfway along a middle chord (a line's samples are its two vertices,
        // which a file may leave off the curve: cgb242's edge 83, 2e-4 at one end), and within
        // 1e-6 rad of the view (the curve itself may stray off its face by 1e-8, ftc_09's).
        let (edge, s) = (&part.edges[e], &part.edges[e].samples);
        let k = (s.len() / 2).max(1);
        let mid = edge.curve.value(
            edge.curve
                .parameter(geom::scale(geom::add(s[k - 1], s[k]), 0.5)),
        );
        part.faces[face]
            .surface
            .parameters(mid, None)
            .is_some_and(|(u, v)| {
                part.faces[face]
                    .surface
                    .normal(u, v)
                    .is_some_and(|n| geom::dot(n, view.toward).abs() <= 1e-6)
            })
    };
    // A seam: an edge one face's loops use twice (listed once among its faces).
    let mut member = vec![false; part.faces.len()];
    for &f in drawn {
        member[f] = true;
    }
    let mut seam = vec![false; part.edges.len()];
    for f in drawn.iter().map(|&f| &part.faces[f]) {
        let mut used = std::collections::HashSet::new();
        for lp in &f.loops {
            for &(e, _) in &lp.edges {
                if !used.insert(e) {
                    seam[e] = true;
                }
            }
        }
    }
    let mut out = Vec::new();
    for (e, faces) in part.edge_faces().iter().enumerate() {
        let faces: Vec<usize> = faces.iter().copied().filter(|&f| member[f]).collect();
        if faces.is_empty() {
            continue;
        }
        let class = match faces.as_slice() {
            [a] if seam[e] => {
                if !on_outline(e, *a) {
                    continue;
                }
                Class::Outline
            }
            [a, b] if a == b || same_surface(&part.faces[*a].surface, &part.faces[*b].surface) => {
                if !on_outline(e, *a) {
                    continue; // a seam, or a join within one surface
                }
                Class::Outline
            }
            [a, b] => match part.arc(*a, *b) {
                // A tangent join whose curvature across it is continuous too (a corner blend
                // meeting the edge blend of its radius) is as smooth as a join within one
                // surface, and left out alike (OpenCascade's `RgN`, against `Rg1` drawn).
                Some(Arc::Smooth) if curvature_continuous(part, e, *a, *b) => {
                    if !on_outline(e, *a) {
                        continue;
                    }
                    Class::Outline
                }
                Some(Arc::Smooth) => Class::Smooth,
                _ => Class::Sharp,
            },
            _ => Class::Sharp,
        };
        let edge = &part.edges[e];
        out.push((class, edge.samples.clone(), On::Edge(&edge.curve, e)));
    }
    out
}

/// Whether faces *a* and *b* bend alike across edge *e*, at its middle: their normal
/// curvatures across it, measured on each untrimmed surface against one normal, agree.
fn curvature_continuous(part: &Part, e: usize, a: usize, b: usize) -> bool {
    let edge = &part.edges[e];
    let s = &edge.samples;
    let k = (s.len() / 2).max(1);
    let p = edge.curve.value(
        edge.curve
            .parameter(geom::scale(geom::add(s[k - 1], s[k]), 0.5)),
    );
    let surface = |f: usize| &part.faces[f].surface;
    let Some(n) = surface(a)
        .parameters(p, None)
        .and_then(|(u, v)| surface(a).normal(u, v))
    else {
        return false;
    };
    let Some(across) = geom::unit(geom::cross(n, geom::sub(s[k], s[k - 1]))) else {
        return false;
    };
    // The surface's rise along *n* a step either side, against the tangent plane.
    const H: f64 = 1e-2;
    let bend = |f: usize| -> Option<f64> {
        let mut rise = 0.0;
        for side in [-1.0, 1.0] {
            let x = geom::add(p, geom::scale(across, side * H));
            let (u, v) = surface(f).parameters(x, None)?;
            rise += geom::dot(geom::sub(surface(f).value(u, v), p), n);
        }
        Some(rise / (H * H))
    };
    match (bend(a), bend(b)) {
        (Some(ka), Some(kb)) => (ka - kb).abs() <= 1e-3 * ka.abs().max(kb.abs()) + 1e-6,
        _ => false,
    }
}

/// Whether two faces lie on one surface (a join OpenCascade does not draw).
fn same_surface(a: &Surface, b: &Surface) -> bool {
    const TOL: f64 = 1e-9;
    let near = |p: V3, q: V3| geom::dist(p, q) <= TOL * (1.0 + geom::norm(p));
    let parallel = |p: V3, q: V3| geom::dot(p, q).abs() > 1.0 - 1e-12;
    let on_axis = |o: V3, p: V3, z: V3| {
        let d = geom::sub(p, o);
        geom::norm(geom::sub(d, geom::scale(z, geom::dot(d, z)))) <= TOL * (1.0 + geom::norm(o))
    };
    match (a, b) {
        // Two planes meeting along an edge are one plane when they are parallel (a sheet's
        // faces can differ by a few nanoradians, which over their width is more than TOL).
        (Surface::Plane { frame: f }, Surface::Plane { frame: g }) => parallel(f.z, g.z),
        (
            Surface::Cylinder {
                frame: f,
                radius: r,
            },
            Surface::Cylinder {
                frame: g,
                radius: s,
            },
        ) => parallel(f.z, g.z) && on_axis(f.origin, g.origin, f.z) && (r - s).abs() <= TOL * r,
        (
            Surface::Sphere {
                frame: f,
                radius: r,
            },
            Surface::Sphere {
                frame: g,
                radius: s,
            },
        ) => near(f.origin, g.origin) && (r - s).abs() <= TOL * r,
        (
            Surface::Cone {
                frame: f,
                semi_angle: x,
                ..
            },
            Surface::Cone {
                frame: g,
                semi_angle: y,
                ..
            },
        ) => {
            a.cone_apex()
                .zip(b.cone_apex())
                .is_some_and(|(p, q)| near(p, q))
                && parallel(f.z, g.z)
                && (x - y).abs() <= TOL
        }
        (
            Surface::Torus {
                frame: f,
                major: r1,
                minor: s1,
            },
            Surface::Torus {
                frame: g,
                major: r2,
                minor: s2,
            },
        ) => {
            near(f.origin, g.origin)
                && parallel(f.z, g.z)
                && (r1 - r2).abs() <= TOL * r1
                && (s1 - s2).abs() <= TOL * s1
        }
        _ => false,
    }
}

/// A curved face's silhouette: where its normal is perpendicular to the view.
fn silhouette(part: &Part, face: usize, view: &View) -> Vec<Vec<V3>> {
    let surface = &part.faces[face].surface;
    if matches!(surface, Surface::Plane { .. } | Surface::Other { .. }) {
        return Vec::new();
    }
    let g = |(u, v): (f64, f64)| {
        surface
            .normal(u, v)
            .map_or(f64::NAN, |nrm| geom::dot(nrm, view.toward))
    };
    contour(part, face, &g, None)
}

/// The contour `g = 0` across a face, traced over its parameter range by marching squares,
/// each crossing solved on its cell edge, refined to the edges' sampling tolerance, and kept
/// where it lies on the face and off its boundary (those edges are drawn already).
///
/// With *touch* (a tolerance and vertices where g is taken to vanish), a face on which g stays
/// within the tolerance of one sign, never below `-touch` or never above `touch`, across its
/// parameter range and along its edges has no contour: it meets `g = 0` at most where it
/// touches it, along its boundary or at a tangency, not across it.
fn contour(
    part: &Part,
    face: usize,
    g: &dyn Fn((f64, f64)) -> f64,
    touch: Option<(f64, &[usize])>,
) -> Vec<Vec<V3>> {
    let surface = &part.faces[face].surface;
    let (Some((u0, u1, mut v0, mut v1)), Some(domain)) = (part.uv_bounds(face), part.domain(face))
    else {
        return Vec::new();
    };
    // A cone closed at its apex by no edge (a drill point bounded by its rim alone) has a range
    // from its edges that stops at the rim, though the face runs on to the apex.
    if let Surface::Cone {
        radius, semi_angle, ..
    } = *surface
    {
        let apex = -radius / semi_angle.sin();
        let near = if apex < v0 { v0 } else { v1 };
        if !(v0..=v1).contains(&apex) && domain.contains(0.5 * (u0 + u1), 0.5 * (apex + near)) {
            (v0, v1) = (v0.min(apex), v1.max(apex));
        }
    }
    // The grid is set off its range by an irrational fraction of a cell, and one cell wider:
    // a contour at a symmetric place (a silhouette at u = 0, π on an axis-aligned cylinder)
    // would otherwise lie along a grid line, where the sign of g is rounding.
    const OFFSET: f64 = 0.381_966_011_250_105_1;
    let n = CONTOUR_GRID + 1;
    let (hu, hv) = (
        (u1 - u0) / CONTOUR_GRID as f64,
        (v1 - v0) / CONTOUR_GRID as f64,
    );
    let at = |i: usize, j: usize| {
        (
            u0 + hu * (i as f64 - 1.0 + OFFSET),
            v0 + hv * (j as f64 - 1.0 + OFFSET),
        )
    };
    let values: Vec<Vec<f64>> = (0..=n)
        .map(|i| (0..=n).map(|j| g(at(i, j))).collect())
        .collect();
    // g vanishing everywhere (a cylinder seen along its axis, a face in the cutting plane)
    // gives no contour of the face's own: its outline is its edges.
    if values
        .iter()
        .flatten()
        .all(|v| v.is_nan() || v.abs() <= 1e-9)
    {
        return Vec::new();
    }
    if let Some((touch, vanish)) = touch {
        // Judged within the range alone: the grid's extra cells run past the face's boundary
        // (a half cylinder split along the plane reaches the other half's side).
        let ranged: Vec<f64> = (0..=n)
            .flat_map(|i| (0..=n).map(move |j| (i, j)))
            .filter(|&(i, j)| {
                let (u, v) = at(i, j);
                (u0..=u1).contains(&u) && (v0..=v1).contains(&v)
            })
            .map(|(i, j)| values[i][j])
            .chain(part.face_edges(face).into_iter().flat_map(|e| {
                let edge = &part.edges[e];
                let last = edge.samples.len().saturating_sub(1);
                edge.samples
                    .iter()
                    .enumerate()
                    .filter_map(|(k, p)| match k {
                        0 if vanish.binary_search(&edge.vertices.0).is_ok() => Some(0.0),
                        k if k == last && vanish.binary_search(&edge.vertices.1).is_ok() => {
                            Some(0.0)
                        }
                        _ => surface.parameters(*p, None).map(g),
                    })
                    .collect::<Vec<_>>()
            }))
            .filter(|v| !v.is_nan())
            .collect();
        if ranged.iter().all(|v| *v >= -touch) || ranged.iter().all(|v| *v <= touch) {
            return Vec::new();
        }
    }
    // Where g changes sign along a cell edge, solved there by bisection.
    let zero = |a: (f64, f64), b: (f64, f64), ga: f64| {
        let (mut lo, mut hi, mut glo) = (a, b, ga);
        for _ in 0..50 {
            let mid = (0.5 * (lo.0 + hi.0), 0.5 * (lo.1 + hi.1));
            let gm = g(mid);
            if gm.is_nan() {
                break;
            }
            if (gm > 0.0) == (glo > 0.0) {
                (lo, glo) = (mid, gm);
            } else {
                hi = mid;
            }
        }
        (0.5 * (lo.0 + hi.0), 0.5 * (lo.1 + hi.1))
    };
    // Each boundary edge with how far it may stand off the contour where it runs along it: on
    // a section (with *touch*), by its stand-off from the face, within the file's tolerance.
    let boundary: Vec<(usize, f64)> = match touch {
        Some(_) => part.edge_deviation(face).to_vec(),
        None => part
            .face_edges(face)
            .into_iter()
            .map(|e| (e, 0.0))
            .collect(),
    };
    let mut out = Vec::new();
    for i in 0..n {
        for j in 0..n {
            let corners = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
            let mut crossing = Vec::new();
            for k in 0..4 {
                let (a, b) = (corners[k], corners[(k + 1) % 4]);
                let (ga, gb) = (values[a.0][a.1], values[b.0][b.1]);
                if ga.is_nan() || gb.is_nan() || (ga > 0.0) == (gb > 0.0) {
                    continue;
                }
                crossing.push(zero(at(a.0, a.1), at(b.0, b.1), ga));
            }
            // Two crossings make a segment; four (a saddle) make two, paired around the cell.
            // Kept within the parameter range as well as the face: a periodic face's domain
            // also takes the range's wrapped copy, which the grid's extra cells reach.
            let inside = |q: (f64, f64)| {
                (u0..=u1).contains(&q.0) && (v0..=v1).contains(&q.1) && domain.contains(q.0, q.1)
            };
            for &[a, b] in crossing.as_chunks::<2>().0 {
                let Some((a, b)) = clipped(a, b, &inside) else {
                    continue;
                };
                let mut uv = vec![a];
                refine(a, b, (hu, hv), g, &|q| surface.value(q.0, q.1), &mut uv, 0);
                let curve: Vec<V3> = uv.iter().map(|q| surface.value(q.0, q.1)).collect();
                // Kept as one chain, broken where it runs along the face's own boundary (that
                // edge, drawn already).
                let mut chain: Vec<V3> = Vec::new();
                for w in curve.windows(2) {
                    let (p, q) = (w[0], w[1]);
                    // Judged at its ends, which lie on the contour (the chord's middle stands
                    // off a curved edge by its sagitta), and on the face: an edge standing off
                    // the face lies that much further from them (counted on a section).
                    let on_edge = |&(e, stand_off): &(usize, f64)| {
                        let edge = &part.edges[e];
                        let on = |x: V3| {
                            polyline_distance(x, &edge.samples) <= 1e-3
                                && geom::dist(edge.curve.value(edge.curve.parameter(x)), x)
                                    <= ON_TOLERANCE + stand_off
                        };
                        on(p) && on(q)
                    };
                    if boundary.iter().any(on_edge) {
                        if chain.len() > 1 {
                            out.push(std::mem::take(&mut chain));
                        }
                        chain.clear();
                        continue;
                    }
                    if chain.is_empty() {
                        chain.push(p);
                    }
                    chain.push(q);
                }
                if chain.len() > 1 {
                    out.push(chain);
                }
            }
        }
    }
    out
}

/// Points of the contour `g = 0` between its points *a* and *b* (pushed after *a*, ending
/// with *b*), placed until each chord stands within the edges' sampling tolerance of it: the
/// middle is moved back onto the contour across the chord by bisection, and each half refined.
fn refine(
    a: (f64, f64),
    b: (f64, f64),
    cell: (f64, f64),
    g: &dyn Fn((f64, f64)) -> f64,
    value: &dyn Fn((f64, f64)) -> V3,
    out: &mut Vec<(f64, f64)>,
    depth: usize,
) {
    let m = (0.5 * (a.0 + b.0), 0.5 * (a.1 + b.1));
    // Across the chord, but within a cell each way: u and v are in different units (an angle
    // and a length), and a wider search can reach another branch of the contour.
    let (au, av) = (-(b.1 - a.1) * 0.5, (b.0 - a.0) * 0.5);
    let shrink = (cell.0 / au.abs().max(f64::MIN_POSITIVE))
        .min(cell.1 / av.abs().max(f64::MIN_POSITIVE))
        .min(1.0);
    let across = (au * shrink, av * shrink);
    let at = |s: f64| (m.0 + across.0 * s, m.1 + across.1 * s);
    let (mut lo, mut hi) = (-1.0, 1.0);
    let (glo, ghi) = (g(at(lo)), g(at(hi)));
    let on = if glo.is_finite() && ghi.is_finite() && (glo > 0.0) != (ghi > 0.0) {
        let mut gl = glo;
        for _ in 0..50 {
            let mid = 0.5 * (lo + hi);
            let gm = g(at(mid));
            if (gm > 0.0) == (gl > 0.0) {
                (lo, gl) = (mid, gm);
            } else {
                hi = mid;
            }
        }
        at(0.5 * (lo + hi))
    } else {
        m
    };
    let chord_mid = geom::scale(geom::add(value(a), value(b)), 0.5);
    if depth >= 16 || geom::dist(value(on), chord_mid) <= super::sampling::CHORD_TOLERANCE {
        out.push(b);
        return;
    }
    refine(a, on, cell, g, value, out, depth + 1);
    refine(on, b, cell, g, value, out, depth + 1);
}

/// The part of segment a–b inside a region (whose boundary it crosses at most once), its
/// crossing found by bisection; `None` when it lies outside.
fn clipped(
    a: (f64, f64),
    b: (f64, f64),
    inside: &dyn Fn((f64, f64)) -> bool,
) -> Option<((f64, f64), (f64, f64))> {
    let lerp = |t: f64| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
    let boundary = |mut t_in: f64, mut t_out: f64| {
        for _ in 0..40 {
            let mid = 0.5 * (t_in + t_out);
            if inside(lerp(mid)) {
                t_in = mid;
            } else {
                t_out = mid;
            }
        }
        lerp(t_in)
    };
    match (inside(a), inside(b)) {
        (true, true) => Some((a, b)),
        (true, false) => Some((a, boundary(0.0, 1.0))),
        (false, true) => Some((boundary(1.0, 0.0), b)),
        (false, false) => None,
    }
}

/// For each curve, the points along it (segment index and fraction) where its projection
/// crosses another curve's, or another ends on it within *touch*, found through a grid of the
/// projected segments.
fn crossings(curves: &[Vec<[f64; 2]>], cell: f64, touch: f64) -> Vec<Vec<(usize, f64)>> {
    let key = |p: [f64; 2]| ((p[0] / cell).floor() as i64, (p[1] / cell).floor() as i64);
    let mut grid: HashMap<(i64, i64), Vec<(usize, usize)>> = HashMap::new();
    for (c, pts) in curves.iter().enumerate() {
        for s in 0..pts.len().saturating_sub(1) {
            let (a, b) = (key(pts[s]), key(pts[s + 1]));
            for x in a.0.min(b.0)..=a.0.max(b.0) {
                for y in a.1.min(b.1)..=a.1.max(b.1) {
                    grid.entry((x, y)).or_default().push((c, s));
                }
            }
        }
    }
    let mut cuts: Vec<Vec<(usize, f64)>> = curves.iter().map(|_| Vec::new()).collect();
    for members in grid.values() {
        for (k, &(c1, s1)) in members.iter().enumerate() {
            for &(c2, s2) in &members[k + 1..] {
                if c1 == c2 {
                    continue;
                }
                let (p, q) = (curves[c1][s1], curves[c1][s1 + 1]);
                let (r, s) = (curves[c2][s2], curves[c2][s2 + 1]);
                if let Some((t, u)) = segment_crossing(p, q, r, s) {
                    cuts[c1].push((s1, t));
                    cuts[c2].push((s2, u));
                }
                // A curve ending on another (a section's outline meeting an edge, a
                // silhouette its face's boundary) cuts it there too, whether or not rounding
                // leaves the end a hair short of crossing.
                let last = |c: usize, s: usize| s + 2 == curves[c].len();
                for (c, seg, (a, b), ends) in [
                    (
                        c1,
                        s1,
                        (p, q),
                        [(s2 == 0).then_some(r), last(c2, s2).then_some(s)],
                    ),
                    (
                        c2,
                        s2,
                        (r, s),
                        [(s1 == 0).then_some(p), last(c1, s1).then_some(q)],
                    ),
                ] {
                    for end in ends.into_iter().flatten() {
                        if let Some(t) = touching(end, a, b, touch) {
                            cuts[c].push((seg, t));
                        }
                    }
                }
            }
        }
    }
    for c in &mut cuts {
        c.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        c.dedup_by(|a, b| a.0 == b.0 && (a.1 - b.1).abs() <= 1e-12);
    }
    cuts
}

/// Where point *p* lies within *tol* of segment a–b, strictly inside it, as a fraction along it.
fn touching(p: [f64; 2], a: [f64; 2], b: [f64; 2], tol: f64) -> Option<f64> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return None;
    }
    let t = ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2;
    let off = (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy);
    (t > 0.0 && t < 1.0 && off <= tol).then_some(t)
}

/// Where segments p–q and r–s cross, as fractions along each.
fn segment_crossing(p: [f64; 2], q: [f64; 2], r: [f64; 2], s: [f64; 2]) -> Option<(f64, f64)> {
    let (d1, d2) = ([q[0] - p[0], q[1] - p[1]], [s[0] - r[0], s[1] - r[1]]);
    let denom = d1[0] * d2[1] - d1[1] * d2[0];
    // Parallel, or near enough that the crossing is rounding: overlapping projections do not
    // cut each other (visibility along them changes only where others cross).
    if denom.abs() <= 1e-9 * d1[0].hypot(d1[1]) * d2[0].hypot(d2[1]) {
        return None;
    }
    let (ra, rb) = (r[0] - p[0], r[1] - p[1]);
    let t = (ra * d2[1] - rb * d2[0]) / denom;
    let u = (ra * d1[1] - rb * d1[0]) / denom;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some((t, u))
}

/// A curve split at its cuts into runs, each visible or hidden as its middle is (*hidden*
/// answers, or gives where it could not, which is returned), with neighbouring runs of one
/// visibility joined. A breakpoint seen within *tol* of the one before (in *view*) is passed
/// over: a stretch that short draws nothing.
fn pieces(
    points: &[V3],
    cuts: &[(usize, f64)],
    (view, tol): (&View, f64),
    hidden: &impl Fn(V3) -> Result<bool, V3>,
) -> Result<Vec<(bool, Vec<V3>)>, V3> {
    let lerp = |s: usize, t: f64| {
        geom::add(
            points[s],
            geom::scale(geom::sub(points[s + 1], points[s]), t),
        )
    };
    // The curve's breakpoints, in order: its vertices and its cuts.
    let mut marks: Vec<(usize, f64)> = (0..points.len()).map(|i| (i, 0.0)).collect();
    marks.extend(cuts.iter().copied());
    marks.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let place = |&(s, t): &(usize, f64)| {
        if s + 1 < points.len() {
            lerp(s, t)
        } else {
            points[s]
        }
    };
    let apart = |p: V3, q: V3| {
        let (a, b) = (view.map(p), view.map(q));
        (a[0] - b[0]).hypot(a[1] - b[1]) > tol
    };
    let mut at: Vec<V3> = Vec::with_capacity(marks.len());
    for p in marks.iter().map(place) {
        if at.last().is_none_or(|q| apart(*q, p)) {
            at.push(p);
        }
    }
    // The curve ends at its end, not at a breakpoint just short of it.
    let end = points[points.len() - 1];
    while at.len() > 1 && !apart(at[at.len() - 1], end) {
        at.pop();
    }
    at.push(end);
    if at.len() == 2 && !apart(at[0], end) {
        return Ok(Vec::new());
    }
    let mut out: Vec<(bool, Vec<V3>)> = Vec::new();
    let mut run: Vec<V3> = vec![at[0]];
    let mut run_visible: Option<bool> = None;
    for w in at.windows(2) {
        let (a, b) = (w[0], w[1]);
        let visible = !hidden(geom::scale(geom::add(a, b), 0.5))?;
        if let Some(v) = run_visible
            && v != visible
        {
            out.push((v, std::mem::take(&mut run)));
            run.push(a);
        }
        run_visible = Some(visible);
        run.push(b);
    }
    if let Some(v) = run_visible {
        out.push((v, run));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    /// The origin remains paired with each stretch when visibility splitting produces fragments.
    #[test]
    fn projection_origin_survives_visibility_splitting() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/golden_simple_through_hole.step");
        let part = crate::read_step_file(&path).unwrap();
        let view = View::new([1.0, -1.0, 1.0], [0.0, 0.0, 1.0]).unwrap();
        let plain = project(&part, &view).unwrap();
        let with_origin = project_with_origin(&part, &view).unwrap();

        assert_eq!(plain.len(), with_origin.len());
        let origins: HashSet<_> = with_origin.iter().map(|(_, origin)| *origin).collect();
        assert!(
            origins
                .iter()
                .any(|o| matches!(o, ProjectionOrigin::Edge(_)))
        );
        assert!(
            origins
                .iter()
                .any(|o| matches!(o, ProjectionOrigin::Face(_)))
        );
        for (plain, (projected, origin)) in plain.iter().zip(&with_origin) {
            assert_eq!(plain.class, projected.class);
            assert_eq!(plain.visible, projected.visible);
            assert_eq!(plain.points, projected.points);
            assert_eq!(plain.exact, projected.exact);
            match *origin {
                ProjectionOrigin::Edge(edge) => assert!(edge < part.edges.len()),
                ProjectionOrigin::Face(face) => assert!(face < part.faces.len()),
            }
        }
    }

    /// Surface-plane section contours retain their face origin without changing legacy geometry.
    #[test]
    fn section_origin_identifies_the_traced_face() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/golden_simple_through_hole.step");
        let part = crate::read_step_file(&path).unwrap();
        let plane = Plane::new(part.bounds().centre(), [0.0, 0.0, 1.0]).unwrap();
        let view = View::new([0.0, 0.0, -1.0], [0.0, 1.0, 0.0]).unwrap();
        let plain = project_section(&part, &view, &plane).unwrap();
        let with_origin = project_section_with_origin(&part, &view, &plane).unwrap();

        assert_eq!(plain.len(), with_origin.len());
        assert!(
            with_origin
                .iter()
                .any(|(_, origin)| matches!(origin, ProjectionOrigin::Face(_)))
        );
        for (plain, (projected, origin)) in plain.iter().zip(&with_origin) {
            assert_eq!(plain.class, projected.class);
            assert_eq!(plain.visible, projected.visible);
            assert_eq!(plain.points, projected.points);
            assert_eq!(plain.exact, projected.exact);
            match *origin {
                ProjectionOrigin::Edge(edge) => assert!(edge < part.edges.len()),
                ProjectionOrigin::Face(face) => assert!(face < part.faces.len()),
            }
        }
    }

    /// A 30 × 30 × 20 block about the origin, bored through along z, with the x = 15 side
    /// (that face's index) made one the kernel cannot intersect.
    fn unintersectable_side() -> (Part, usize) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/rejected_bored_box.step");
        let mut part = crate::read_step_file(&path).unwrap();
        let all: Vec<usize> = (0..part.faces.len()).collect();
        let side = RayCaster::for_faces(&part, all)
            .hits([20.0, 10.0, 0.0], [-1.0, 0.0, 0.0], 100.0)
            .unwrap()[0]
            .face;
        part.faces[side].surface = Surface::Other { kind: "TEST" };
        (part, side)
    }

    #[test]
    fn a_face_that_cannot_be_intersected_is_not_seen_through() {
        let (part, side) = unintersectable_side();
        assert!(part.unresolved_faces().is_empty() && part.unresolved_edges().is_empty());
        // Seen from beyond it, the far side's edges are behind it, and only it: nothing decides
        // whether they are hidden, so the drawing is refused, naming it.
        let facing = View::new([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]).unwrap();
        let refused = project(&part, &facing).unwrap_err();
        assert_eq!(refused.faces, [side]);
        assert!(refused.edges.len() <= 1);
        let plane = Plane::new([0.0; 3], [0.0, 1.0, 0.0]).unwrap();
        assert_eq!(
            project_section(&part, &facing, &plane).unwrap_err().faces,
            [side]
        );
        // Seen from the other side, every ray that reaches it is stopped first by a face that
        // can be intersected, or meets nothing: the drawing is made.
        let away = View::new([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]).unwrap();
        let drawn = project(&part, &away).unwrap();
        assert!(drawn.iter().any(|p| p.visible) && drawn.iter().any(|p| !p.visible));
    }

    #[test]
    fn a_stretch_too_short_to_see_is_not_drawn() {
        // A straight curve cut a hair before its end, where the hair alone is judged hidden (as
        // a ray from beside a vertex can be): before, a stretch of no length came out.
        let view = View::new([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]).unwrap();
        let points = [[0.0, 0.0, 0.0], [5.0, 0.0, 0.0], [10.0, 0.0, 0.0]];
        let cuts = [(0, 0.5), (1, 1.0 - 1e-11)];
        let hair = |p: V3| Ok(p[0] > 10.0 - 1e-10);
        let runs = pieces(&points, &cuts, (&view, 1e-9), &hair).unwrap();
        assert_eq!(runs.len(), 1);
        let (visible, run) = &runs[0];
        assert!(*visible);
        assert_eq!(run.first(), Some(&points[0]));
        assert_eq!(run.last(), Some(&points[2]));
        // A cut that far in makes a stretch of its own, which ends at the curve's end.
        let cuts = [(1, 1.0 - 1e-6)];
        let tail = |p: V3| Ok(p[0] > 10.0 - 1e-5);
        let runs = pieces(&points, &cuts, (&view, 1e-9), &tail).unwrap();
        assert_eq!(runs.iter().map(|r| r.0).collect::<Vec<_>>(), [true, false]);
        assert_eq!(runs[1].1.last(), Some(&points[2]));
        // A stretch not known is returned, with where it was judged.
        let unknown = pieces(&points, &cuts, (&view, 1e-9), &|p: V3| Err(p));
        assert_eq!(unknown.unwrap_err(), [2.5, 0.0, 0.0]);
    }
}
