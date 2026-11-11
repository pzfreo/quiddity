//! A point proved to lie on a face's trimmed region, with its surface parameters: where a note
//! about the face can be anchored (Python quiddity's `inspect_face(face).anchor`).
//! [`Part::face_centre`] is build123d's `Face.center()`, which on a curved face is the middle of
//! the parameter range and can fall outside a trimmed face; it is used only where the face's
//! [`crate::uv::FaceDomain`] contains it.

use crate::brep::Part;
use crate::geom::{Surface, V3};

/// specify-core's mesh deflections: chordal, as a share of the part's diagonal, and angular.
const MESH_DEFLECTION: f64 = 1e-3;
const MESH_ANGULAR: f64 = 0.3;

/// A point on a face and its surface parameters: `point` is the surface's point at `uv`, and
/// the face's trimming domain contains `uv`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct FaceAnchor {
    pub point: V3,
    pub uv: (f64, f64),
}

/// Why no point was proved on a face.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct AnchorRefusal {
    pub reason: String,
}

impl std::fmt::Display for AnchorRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}

impl std::error::Error for AnchorRefusal {}

impl Part {
    /// A point on the face's trimmed region: [`Part::face_centre`] where the face's domain
    /// contains its parameters; otherwise, from the face's triangulation ([`Part::triangulate`]
    /// at specify-core's deflections, 1e-3 of the part's diagonal and 0.3 rad), the surface's
    /// point at the parameters of the centroid of the largest triangle whose centroid the domain
    /// contains (larger triangles first, ties by index). Refused, naming the face, where the face
    /// has no trimming domain, cannot be triangulated, or no triangle's centroid is inside.
    pub fn face_anchor(&self, face: usize) -> Result<FaceAnchor, AnchorRefusal> {
        let refuse = |why: String| {
            Err(AnchorRefusal {
                reason: format!("face {face}: {why}"),
            })
        };
        let surface = &self.faces[face].surface;
        let Some(domain) = self.domain(face) else {
            return refuse("its trimming domain is not known".to_owned());
        };
        let centre = match *surface {
            Surface::Plane { frame } => self.face_centre(face).map(|p| {
                let local = frame.to_local(p);
                (local[0], local[1])
            }),
            _ => self
                .uv_bounds(face)
                .map(|(u0, u1, v0, v1)| (0.5 * (u0 + u1), 0.5 * (v0 + v1))),
        };
        if let Some(uv) = centre
            && domain.contains(uv.0, uv.1)
        {
            return Ok(FaceAnchor {
                point: surface.value(uv.0, uv.1),
                uv,
            });
        }
        let b = self.bounds();
        let diagonal = crate::geom::dist(b.min, b.max);
        let mesh = match self.triangulate(face, MESH_DEFLECTION * diagonal, MESH_ANGULAR) {
            Ok(mesh) => mesh,
            Err(why) => return refuse(format!("its centre is outside it and {why}")),
        };
        let mut order: Vec<usize> = (0..mesh.triangles.len()).collect();
        let areas: Vec<f64> = order.iter().map(|&t| mesh.triangle_area(t)).collect();
        order.sort_by(|&a, &b| areas[b].total_cmp(&areas[a]).then(a.cmp(&b)));
        // A vertex on a seam carries the parameters of one side: the others are taken the
        // nearest turn round from the first vertex's.
        let (pu, pv) = surface.periodic();
        let near = |x: f64, to: f64, periodic: bool| {
            if periodic {
                crate::geom::nearest_turn(x, to)
            } else {
                x
            }
        };
        for t in order {
            let [i, j, k] = mesh.triangles[t];
            let a = mesh.uv[i];
            let [b, c] = [mesh.uv[j], mesh.uv[k]].map(|p| (near(p.0, a.0, pu), near(p.1, a.1, pv)));
            let uv = ((a.0 + b.0 + c.0) / 3.0, (a.1 + b.1 + c.1) / 3.0);
            if domain.contains(uv.0, uv.1) {
                return Ok(FaceAnchor {
                    point: surface.value(uv.0, uv.1),
                    uv,
                });
            }
        }
        refuse(format!(
            "neither its centre nor any of its {} triangles' centroids is inside it",
            mesh.triangles.len()
        ))
    }
}
