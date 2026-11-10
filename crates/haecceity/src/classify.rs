//! Point-in-solid classification by ray parity — the stand-in for
//! `BRepClass3d_SolidClassifier`.
//!
//! A ray is cast from the point; each crossing of a face (the untrimmed surface hit, kept when
//! the hit lies inside the face's trimming loops) flips inside/outside. A ray that passes too
//! close to an edge or grazes a surface is discarded and another direction tried, so the answer
//! never rests on a knife-edge hit.

use super::brep::Part;
use super::geom::{self, V3};
use super::rays::{RayCaster, polyline_distance};

/// Fixed, deliberately irrational-looking directions: none is parallel to a principal axis or a
/// principal diagonal, which is where modelled geometry concentrates.
// 0.7071 is a ray component, not an approximation of 1/√2; changing it would move the rays.
#[allow(clippy::approx_constant)]
const DIRECTIONS: [V3; 7] = [
    [0.5773, 0.6123, 0.5401],
    [-0.4364, 0.8018, 0.4082],
    [0.7071, -0.3015, 0.6396],
    [-0.6247, -0.5217, -0.5812],
    [0.2673, 0.5345, -0.8018],
    [0.9012, 0.1234, -0.4153],
    [-0.1111, -0.9333, 0.3412],
];

/// A point this close to a face is on the boundary.
pub const ON_TOLERANCE: f64 = 1e-6;

/// Point classification against one part or solid, casting through its [`RayCaster`].
pub struct Classifier<'a> {
    rays: RayCaster<'a>,
    /// The edges of the faces classified against.
    edges: Vec<usize>,
    reach: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    In,
    Out,
    /// Within [`ON_TOLERANCE`] of a face (`TopAbs_ON`).
    On,
    /// Every ray was ambiguous, the clean rays disagreed with no majority of two, or the part
    /// has a surface this classifier cannot intersect.
    Unknown,
}

impl<'a> Classifier<'a> {
    /// Classifies against the whole part.
    pub fn new(part: &'a Part) -> Self {
        Self::from_rays(RayCaster::for_faces(part, (0..part.faces.len()).collect()))
    }

    /// Classifies against one solid alone, as when the Python implementation probes a solid
    /// of a compound rather than the part.
    pub fn for_solid(part: &'a Part, solid: usize) -> Self {
        Self::from_rays(RayCaster::for_solid(part, solid))
    }

    /// Classifies against the given faces (closed shells between them).
    pub fn for_faces(part: &'a Part, faces: Vec<usize>) -> Self {
        Self::from_rays(RayCaster::for_faces(part, faces))
    }

    /// Classifies against *faces*, pruning by *face_boxes* (one per part face, each certainly
    /// containing its face) in place of the faces' own sampled boxes.
    pub(crate) fn with_face_boxes(
        part: &'a Part,
        faces: Vec<usize>,
        face_boxes: Vec<crate::geom::Bounds>,
    ) -> Self {
        Self::from_rays(RayCaster::with_face_boxes(part, faces, face_boxes))
    }

    fn from_rays(rays: RayCaster<'a>) -> Self {
        let mut edges: Vec<usize> = rays
            .faces
            .iter()
            .flat_map(|&f| rays.part.face_edges(f))
            .collect();
        edges.sort_unstable();
        edges.dedup();
        let reach = rays.root_box.diagonal() * 2.0 + 1.0;
        Classifier { rays, edges, reach }
    }

    /// The rays this classifier casts, for callers that need the hits themselves.
    pub fn rays(&self) -> &RayCaster<'a> {
        &self.rays
    }

    pub fn classify(&self, p: V3) -> State {
        if self.on_boundary(p) {
            return State::On;
        }
        let mut votes = (0usize, 0usize);
        for dir in DIRECTIONS {
            let Some(dir) = geom::unit(dir) else { continue };
            match self.rays.crossing_count(p, dir, self.reach) {
                Some(n) if n % 2 == 1 => votes.0 += 1,
                Some(_) => votes.1 += 1,
                None => continue,
            }
            // Two agreeing clean rays settle it, while no clean ray has disagreed.
            if votes.0 >= 2 && votes.1 == 0 {
                return State::In;
            }
            if votes.1 >= 2 && votes.0 == 0 {
                return State::Out;
            }
        }
        // Once clean rays disagree, one side has missed or invented a crossing: every ray has
        // been cast, and only a majority of two or more settles it.
        match votes {
            (i, o) if i >= o + 2 || (i > 0 && o == 0) => State::In,
            (i, o) if o >= i + 2 || (o > 0 && i == 0) => State::Out,
            _ => State::Unknown,
        }
    }

    /// Whether *p* lies on a face, within [`ON_TOLERANCE`] (the tolerance the Python
    /// implementation hands `BRepClass3d_SolidClassifier::Perform`).
    fn on_boundary(&self, p: V3) -> bool {
        let (part, rays) = (self.rays.part, &self.rays);
        // On an edge, a face's own containment test is at its boundary and may say no.
        let on_edge = self.edges.iter().any(|&e| {
            let edge = &part.edges[e];
            rays.edge_boxes[e].contains(p, rays.edge_tol)
                && polyline_distance(p, &edge.samples) <= rays.edge_tol
                && geom::dist(edge.curve.value(edge.curve.parameter(p)), p) <= ON_TOLERANCE
        });
        on_edge
            || rays.faces.iter().any(|&i| {
                if !rays.face_boxes[i].contains(p, ON_TOLERANCE) {
                    return false;
                }
                let face = &part.faces[i];
                let Some((u, v)) = face.surface.parameters(p, None) else {
                    return false;
                };
                geom::dist(face.surface.value(u, v), p) <= ON_TOLERANCE
                    && part.domain(i).is_some_and(|d| d.contains(u, v))
            })
    }

    /// Each probe ray's crossing count (`None` when ambiguous), for diagnosing a classification.
    pub fn explain(&self, p: V3) -> Vec<Option<usize>> {
        DIRECTIONS
            .iter()
            .filter_map(|d| geom::unit(*d))
            .map(|d| self.rays.crossing_count(p, d, self.reach))
            .collect()
    }
}

/// Is *p* inside the material of *part*? (`_bevel._material_at`: only a clean `IN` counts.)
pub fn material_at(classifier: &Classifier<'_>, p: V3) -> bool {
    classifier.classify(p) == State::In
}
