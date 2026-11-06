//! Geometric congruence of two parts: a proper rigid motion taking one exactly onto the other,
//! proven face by face (`docs/correspondence.md`, "Congruence"). Unlike [`super::align`], which
//! finds the likeliest motion between two revisions, this only answers when every face of one
//! part lies on a face of the other and back, so a symmetry of the fingerprints that is not a
//! symmetry of the B-rep (a hexagon's ±120° turn that moves the seams) is never accepted.

use std::collections::BTreeSet;

use crate::features::Features;
use crate::kernel::brep::Part;
use crate::kernel::cloud::FaceProbe;
use crate::kernel::geom::{self, V3};

use super::align::{Motion, align};
use super::fingerprint::{FaceFingerprint, Fingerprints, fingerprint};

/// Positions and distances agree within this, in millimetres.
const DISTANCE: f64 = 1e-5;
/// Directions agree within this (the length of the unit vectors' difference).
const DIRECTION: f64 = 1e-6;
/// Intrinsic scalars (areas, surface parameters, the scale) agree within this relative
/// difference, with an absolute floor of the same size.
const RELATIVE: f64 = 1e-7;
/// The chordal deflection, in millimetres, of the samples taken on each trimmed face.
const DEFLECTION: f64 = 0.1;

/// A proper rigid motion taking *a* exactly onto *b*, with each face of *a* paired with the face
/// of *b* it lands on (`(a's face, b's face)`, in *a*'s face order), or `None`.
///
/// Only geometry is read, never names or other metadata. Candidates are tried in order: an
/// exact B-rep match ([`Part::same_brep`]) or the identity, the translation taking *a*'s
/// centroid onto *b*'s, then the fingerprint alignment ([`align`]). A candidate is accepted
/// only when every face pairs one to one with equal surface type, parameters, area, adjacency,
/// centroid and mean normal, and samples of each trimmed face lie on its partner within 1e-5
/// mm, in both directions. A reflection is never considered, so a mirror image is not
/// congruent unless it is also a rotated copy (the part is achiral).
///
/// A part with a face whose area the kernel cannot integrate, or with an unresolved face or
/// edge, matches only through [`Part::same_brep`]: an exact copy of itself.
///
/// It fingerprints both parts; [`congruent_with`] takes fingerprints already made, for a caller
/// comparing one part against many.
pub fn congruent(a: &Part, b: &Part) -> Option<(Motion, Vec<(usize, usize)>)> {
    if a.same_brep(b) {
        return Some(identity(a));
    }
    if !comparable(a, b) {
        return None;
    }
    let none = Features::default();
    congruent_with(a, &fingerprint(a, &none), b, &fingerprint(b, &none))
}

/// [`congruent`], with *af* and *bf* the [`fingerprint`]s of *a* and *b* (with any features: only
/// the faces are proven, the features only help [`align`]).
pub fn congruent_with(
    a: &Part,
    af: &Fingerprints,
    b: &Part,
    bf: &Fingerprints,
) -> Option<(Motion, Vec<(usize, usize)>)> {
    if a.same_brep(b) {
        return Some(identity(a));
    }
    if !comparable(a, b)
        || !close(af.scale, bf.scale)
        || af.faces.len() != a.faces.len()
        || bf.faces.len() != b.faces.len()
    {
        return None;
    }
    let prove = |m: &Motion| proper(m).then(|| equal_at(a, af, b, bf, m)).flatten();
    if let Some(faces) = prove(&Motion::IDENTITY) {
        return Some((Motion::IDENTITY, faces));
    }
    if let (Some(ca), Some(cb)) = (centroid(af), centroid(bf)) {
        let shift = Motion {
            t: geom::sub(cb, ca),
            ..Motion::IDENTITY
        };
        if let Some(faces) = prove(&shift) {
            return Some((shift, faces));
        }
    }
    let (motion, alignment) = align(af, bf, &Default::default());
    alignment
        .found
        .then(|| prove(&motion))
        .flatten()
        .map(|faces| (motion, faces))
}

fn identity(a: &Part) -> (Motion, Vec<(usize, usize)>) {
    (
        Motion::IDENTITY,
        (0..a.faces.len()).map(|i| (i, i)).collect(),
    )
}

/// Equal topology counts, and nothing the face proof cannot read.
fn comparable(a: &Part, b: &Part) -> bool {
    a.faces.len() == b.faces.len()
        && a.edges.len() == b.edges.len()
        && a.solids.len() == b.solids.len()
        && !a.faces.is_empty()
        && [a, b]
            .iter()
            .all(|p| p.unresolved_faces().is_empty() && p.unresolved_edges().is_empty())
}

fn close(a: f64, b: f64) -> bool {
    a.is_finite() && b.is_finite() && (a - b).abs() <= RELATIVE * a.abs().max(b.abs()).max(1.0)
}

/// An orthonormal rotation with determinant +1: no reflection. Defence in depth: the face proof
/// already rejects a mirror image (a reflection moves some trimmed face off its partner), and
/// [`align`] solves proper rotations only, but a reflected candidate is refused before it.
fn proper(m: &Motion) -> bool {
    let r = m.r;
    let orthonormal = (0..3).all(|i| {
        (0..3).all(|j| {
            let dot: f64 = (0..3).map(|k| r[k][i] * r[k][j]).sum();
            (dot - f64::from(u8::from(i == j))).abs() <= 1e-9
        })
    });
    orthonormal && geom::dot(r[0], geom::cross(r[1], r[2])) > 0.0
}

/// The area-weighted mean of the face centroids, where every face has an area.
fn centroid(f: &Fingerprints) -> Option<V3> {
    let mut sum = [0.0; 3];
    let mut total = 0.0;
    for face in &f.faces {
        let area = face.area?;
        sum = geom::add(sum, geom::scale(face.centroid, area));
        total += area;
    }
    (total > 0.0).then(|| geom::scale(sum, 1.0 / total))
}

fn face_equal(a: &FaceFingerprint, b: &FaceFingerprint, m: &Motion) -> bool {
    a.surface == b.surface
        && a.adjacency == b.adjacency
        && a.parameters.len() == b.parameters.len()
        && a.parameters
            .iter()
            .all(|(k, x)| b.parameters.get(k).is_some_and(|y| close(*x, *y)))
        && matches!((a.area, b.area), (Some(x), Some(y)) if close(x, y))
        && geom::dist(m.point(a.centroid), b.centroid) <= DISTANCE
        && match (a.normal, b.normal) {
            (Some(x), Some(y)) => geom::dist(m.dir(x), y) <= DIRECTION,
            (None, None) => true,
            _ => false,
        }
}

/// The face pairs under *m*, when every face is proven equal to exactly one partner.
fn equal_at(
    a: &Part,
    af: &Fingerprints,
    b: &Part,
    bf: &Fingerprints,
    m: &Motion,
) -> Option<Vec<(usize, usize)>> {
    // The inverse of a proper rigid motion: p ↦ Rᵀ(p − t).
    let inverse = |p: V3| {
        let q = geom::sub(p, m.t);
        [0, 1, 2].map(|i| (0..3).map(|k| m.r[k][i] * q[k]).sum())
    };
    let mut pairs = Vec::with_capacity(af.faces.len());
    let mut unused: BTreeSet<usize> = (0..bf.faces.len()).collect();
    for face in &af.faces {
        let j = unused
            .iter()
            .copied()
            .find(|&j| face_equal(face, &bf.faces[j], m))?;
        let (i, bj) = (face.index, bf.faces[j].index);
        // Moments cannot tell equal boxes with different trims, or freeform surfaces, apart:
        // sample the exact trimmed faces against each other, both ways.
        let onto = FaceProbe::new(b, bj);
        if !a
            .face_cloud(i, DEFLECTION)
            .iter()
            .all(|&p| onto.distance(m.point(p)) <= DISTANCE)
        {
            return None;
        }
        let back = FaceProbe::new(a, i);
        if !b
            .face_cloud(bj, DEFLECTION)
            .iter()
            .all(|&p| back.distance(inverse(p)) <= DISTANCE)
        {
            return None;
        }
        unused.remove(&j);
        pairs.push((i, bj));
    }
    Some(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflections_are_not_proper() {
        let mirror = Motion {
            r: [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            t: [0.0; 3],
        };
        assert!(!proper(&mirror));
        let inversion = Motion {
            r: [[-1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]],
            t: [1.0, 2.0, 3.0],
        };
        assert!(!proper(&inversion));
        let scaled = Motion {
            r: [[2.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            t: [0.0; 3],
        };
        assert!(!proper(&scaled));
        let (c, s) = (0.6, 0.8);
        let turn = Motion {
            r: [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]],
            t: [5.0, 0.0, 0.0],
        };
        assert!(proper(&turn));
        assert!(proper(&Motion::IDENTITY));
    }
}
