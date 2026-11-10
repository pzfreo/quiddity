//! Conservative, reported normal repair of negative closed solid boundaries.
//!
//! A negative flux is only a candidate: mixed face senses can also give a negative volume.
//! Ray parity is independent of the senses and supplies the material side. Every face must
//! have three regular interior witnesses proving its material side, at two offsets.
//! Unresolved faces, open topology or insufficient evidence leave the entire solid
//! unchanged. A global inversion requires every face inward; a negative mixed boundary
//! may instead have only its inward normals
//! corrected if every material side is proved and its unchanged wires then form a
//! coherent outward boundary. Coherently inward boundary wires reverse with the normals;
//! outward wires survive a normals-only repair. This does not sew gaps or repair mixed wires.

use crate::brep::{Part, holds};
use crate::classify::{Classifier, ON_TOLERANCE, State};
use crate::geom;

impl Part {
    /// Repair a closed boundary only when its signed volume is negative and every face's
    /// material side is established. STEP import applies this before returning the part;
    /// callers building in-memory geometry can apply it explicitly before any other queries.
    /// A negative mixed boundary may instead have only its proved inward normals reversed,
    /// provided its unchanged wires are coherent under the corrected senses and every face
    /// of the candidate is proved outward. The negative-mass gate precedes all material-side
    /// proofs, so an outward face encountered first cannot hide a mixed negative shell.
    /// Coherently inward boundary wires are reversed too, preserving face validity and
    /// convexity; already outward wires are retained. Each repaired solid is named in
    /// [`Self::input_repairs`]. Mixed wires or unproved boundaries
    /// retain their original face senses, signed mass and classification behaviour, as does a
    /// solid whose faces have only seam or vertex loops (a full sphere or torus), whose wire
    /// sense cannot be established.
    ///
    /// Every topologically valid solid gets one signed mass check. Positive or unavailable
    /// mass skips the material-side proofs. Its face quadrature is cached for later mass,
    /// moment and recognition queries; no per-face ray classification runs on ordinary
    /// positive solids. Cold import alone therefore does more quadrature than the former
    /// one-face outward shortcut; that shortcut could not exclude a mixed negative shell.
    pub fn repair_inverted_shells(mut self) -> Self {
        for solid in 0..self.solids.len() {
            if !self.solid_is_valid(solid) {
                continue;
            }
            if !self
                .solid_mass(solid)
                .is_some_and(|(v, _)| v.is_finite() && v < 0.0)
            {
                continue;
            }
            let classifier = Classifier::for_solid(&self, solid);
            let sides: Vec<_> = self.solids[solid]
                .faces
                .iter()
                .map(|&f| match face_side(&self, &classifier, f) {
                    Side::Outward
                        if face_side_with_normal(&self, &classifier, f, true) != Side::Inward =>
                    {
                        Side::Unproved
                    }
                    side => side,
                })
                .collect();
            if sides.iter().all(|&s| s == Side::Inward) {
                if let Some(reverse_wires) = inward_wires(&self, solid) {
                    self.record_inverted_shell_repair(solid, reverse_wires);
                }
            } else if !sides.contains(&Side::Unproved) {
                // A mixed negative boundary is not a global inversion. Repair only its
                // normals, and only if the existing wires already describe a coherent
                // outward boundary under the independently proved material sides.
                let flips: Vec<_> = self.solids[solid]
                    .faces
                    .iter()
                    .zip(sides)
                    .filter_map(|(&f, side)| (side == Side::Inward).then_some(f))
                    .collect();
                let mut faces = self.faces.clone();
                for &f in &flips {
                    faces[f].reversed = !faces[f].reversed;
                }
                let candidate = Part::new(faces, self.edges.clone(), self.solids.clone());
                if candidate.solid_is_valid(solid)
                    && inward_wires(&candidate, solid) == Some(true)
                    && candidate
                        .solid_mass(solid)
                        .is_some_and(|(v, _)| v.is_finite() && v > 0.0)
                {
                    let classifier = Classifier::for_solid(&candidate, solid);
                    if candidate.solids[solid]
                        .faces
                        .iter()
                        // Reversing only the witness normal reuses the full inward
                        // proof: three witnesses at two offsets, not the single
                        // outward witness sufficient to refuse a global inversion.
                        .all(|&f| {
                            face_side_with_normal(&candidate, &classifier, f, true) == Side::Inward
                        })
                    {
                        self.record_mixed_normal_repair(solid, &flips);
                    }
                }
            }
        }
        self
    }
}

/// How many of a solid's faces [`outward_proved`] tries before leaving the solid to the full
/// test.
#[cfg(test)]
const PROOF_FACES: usize = 8;

/// Whether one of the solid's first [`PROOF_FACES`] faces has an outward witness (a point a
/// little in front of it outside the material, a little behind it inside, at two offsets) by
/// ray parity against boxes that certainly contain each face ([`certain_box`]). A face whose
/// normal leaves the material at one regular point does so everywhere, so the boundary is not
/// entirely inward. This former import shortcut is retained only as a test witness:
/// it cannot rule out a negative mixed boundary. A sampled
/// box prunes the full classifier's rays; the certain boxes here prune none of the crossings the
/// kernel's own contact test admits, and need no face's interior sampled to be built. A face
/// found inward, or no witness either way, leaves the solid to the full test.
///
/// The proof is that the full test would say the same, not only that the parity is right: this
/// classifier differs from [`Classifier::for_solid`] only in its pruning boxes, and each certain
/// box contains the face's sampled box ([`Part::face_bounds`]) padded by at least 0.1 mm, so a
/// witness found outward here is found outward by the full classifier too, ruling out a
/// global inversion. `outward_proved_excludes_global_inversion` checks this.
#[cfg(test)]
fn outward_proved(part: &Part, solid: usize) -> bool {
    let faces = &part.solids[solid].faces;
    let mut boxes = vec![crate::geom::Bounds::empty(); part.faces.len()];
    for &f in faces {
        let Some(b) = certain_box(part, f) else {
            return false;
        };
        boxes[f] = b;
    }
    let classifier = Classifier::with_face_boxes(part, faces.clone(), boxes);
    for &f in faces.iter().take(PROOF_FACES) {
        match face_side(part, &classifier, f) {
            Side::Outward => return true,
            Side::Inward => return false,
            Side::Unproved => {}
        }
    }
    false
}

/// A box that certainly holds face *f* and the bands of its edges, without sampling its
/// interior. A planar, cylindrical or conical face lies on lines (any line in the plane, the
/// rulings) that leave it through its boundary, or at a cone's apex, so it lies in the convex
/// hull of its boundary and apex; a sphere or torus face lies in its whole surface's box, and a
/// B-spline face in its control points' (the convex-hull property, for positive weights). The
/// boundary is bounded by its curves': a segment's ends, a conic's whole circle, a B-spline
/// curve's control points. The pad covers edges straying from the surface (and the bands about
/// them that contact admits) far beyond any tolerance a file states. `None` for a surface or
/// curve whose box is not certain.
#[cfg(test)]
fn certain_box(part: &Part, f: usize) -> Option<crate::geom::Bounds> {
    use crate::geom::{Bounds, Curve, Surface};
    let around = |c: [f64; 3], r: f64| Bounds {
        min: c.map(|x| x - r),
        max: c.map(|x| x + r),
    };
    let face = &part.faces[f];
    let mut b = Bounds::empty();
    let boundary = |b: &mut Bounds| -> Option<()> {
        for lp in &face.loops {
            if let Some(p) = lp.vertex {
                b.add(p);
            }
            for &(e, _) in &lp.edges {
                let edge = &part.edges[e];
                b.add(edge.start);
                b.add(edge.end);
                match &edge.curve {
                    Curve::Line { .. } => {}
                    Curve::Circle { frame, radius } => b.merge(&around(frame.origin, *radius)),
                    Curve::Ellipse { frame, major, .. } => b.merge(&around(frame.origin, *major)),
                    Curve::Nurbs(c) => {
                        if !c.weights().iter().all(|&w| w > 0.0) {
                            return None;
                        }
                        c.control_points().iter().for_each(|&p| b.add(p));
                    }
                }
            }
        }
        Some(())
    };
    match &face.surface {
        Surface::Plane { .. } | Surface::Cylinder { .. } => boundary(&mut b)?,
        Surface::Cone { .. } => {
            boundary(&mut b)?;
            b.add(face.surface.cone_apex()?);
        }
        Surface::Sphere { frame, radius } => b = around(frame.origin, *radius),
        Surface::Torus {
            frame,
            major,
            minor,
        } => b = around(frame.origin, major + minor),
        Surface::Freeform { surface, .. } => {
            if !surface.weights.iter().flatten().all(|&w| w > 0.0) {
                return None;
            }
            let (min, max) = surface.control_bounds();
            b = Bounds { min, max };
        }
        Surface::Other { .. } => return None,
    }
    let pad = 0.1 + 1e-3 * b.diagonal();
    (pad.is_finite() && b.min.iter().chain(&b.max).all(|x| x.is_finite())).then(|| Bounds {
        min: b.min.map(|x| x - pad),
        max: b.max.map(|x| x + pad),
    })
}

/// Whether the wires follow the inward normals too, requiring reversal with the faces.
/// A normals-only inversion leaves the wires outward and needs no wire change. Reuse the
/// integrator's local boundary-side test: a parameter polygon's winding is not evidence
/// around a pole or a periodic seam. Mixed or unproved wire senses refuse the repair.
fn inward_wires(part: &Part, solid: usize) -> Option<bool> {
    let mut sense = None;
    for &face in &part.solids[solid].faces {
        let f = &part.faces[face];
        for (i, lp) in f.loops.iter().enumerate() {
            // A vertex or a seam bounds no neighbourhood on the face by itself.
            let boundary = lp.edges.iter().any(|&(e, _)| {
                f.loops
                    .iter()
                    .flat_map(|l| &l.edges)
                    .filter(|&&(other, _)| other == e)
                    .count()
                    == 1
            });
            if !boundary {
                continue;
            }
            let left = (part.loop_direction(face, i, None)? > 0.0) ^ f.reversed;
            if sense.is_some_and(|s| s != left) {
                return None;
            }
            sense = Some(left);
        }
    }
    sense
}

/// Which way a face faces material, by its witnesses.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    /// Three regular interior witnesses face into material, and none out of it.
    Inward,
    /// A witness faces out of material.
    Outward,
    /// Neither established.
    Unproved,
}

fn face_side(part: &Part, classifier: &Classifier<'_>, face: usize) -> Side {
    face_side_with_normal(part, classifier, face, false)
}

fn face_side_with_normal(
    part: &Part,
    classifier: &Classifier<'_>,
    face: usize,
    reverse_normal: bool,
) -> Side {
    let Some(domain) = part.domain(face) else {
        return Side::Unproved;
    };
    let (u0, u1) = domain.u_range();
    let (v0, v1) = domain.v_range();
    let h = (part.face_bounds(face).diagonal() * 1e-5).max(64.0 * ON_TOLERANCE);
    let mut witnesses = 0;
    for a in [0.2113, 0.4331, 0.677, 0.819, 0.5] {
        for b in [0.413, 0.673, 0.293, 0.839, 0.5] {
            let (u, v) = (u0 + a * (u1 - u0), v0 + b * (v1 - v0));
            if !domain.contains(u, v) || !holds(domain, u, v, (1e-6 * (u1 - u0), 1e-6 * (v1 - v0)))
            {
                continue;
            }
            let Some(n) = part.face_normal(face, u, v) else {
                continue;
            };
            let n = if reverse_normal {
                geom::scale(n, -1.0)
            } else {
                n
            };
            let p = part.faces[face].surface.value(u, v);
            let mut inward = false;
            for distance in [h, h / 4.0, h / 16.0] {
                if distance < 8.0 * ON_TOLERANCE {
                    break;
                }
                let sides = |d| {
                    (
                        classifier.classify(geom::add(p, geom::scale(n, d))),
                        classifier.classify(geom::sub(p, geom::scale(n, d))),
                    )
                };
                let (far, near) = (sides(distance), sides(distance / 2.0));
                if far == (State::Out, State::In) || near == (State::Out, State::In) {
                    return Side::Outward;
                }
                if far == (State::In, State::Out) && near == far {
                    inward = true;
                    break;
                }
            }
            if inward {
                witnesses += 1;
                if witnesses == 3 {
                    return Side::Inward;
                }
                // Spread the witnesses across the face rather than keeping three at one u.
                break;
            }
        }
    }
    Side::Unproved
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brep::Part;

    /// Whether the full test finds every face of the solid inward.
    fn all_inward(part: &Part, solid: usize) -> bool {
        let classifier = Classifier::for_solid(part, solid);
        part.solids[solid]
            .faces
            .iter()
            .all(|&f| face_side(part, &classifier, f) == Side::Inward)
    }

    /// The part with its face senses flipped (all but face `keep`), and the wires too when
    /// `wires`.
    fn flipped(part: &Part, wires: bool, keep: Option<usize>) -> Part {
        let mut faces = part.faces.clone();
        for (i, face) in faces.iter_mut().enumerate() {
            if Some(i) == keep {
                continue;
            }
            face.reversed = !face.reversed;
            if wires {
                for lp in &mut face.loops {
                    lp.edges.reverse();
                    for (_, forward) in &mut lp.edges {
                        *forward = !*forward;
                    }
                }
            }
        }
        Part::new(faces, part.edges.clone(), part.solids.clone())
    }

    /// The cheap proof excludes a global inversion,
    /// on the #53/#59 fixtures read outward, inverted (normals only, and with the wires) and
    /// mixed. It must also prove the outward readings and never the inverted ones.
    #[test]
    fn outward_proved_excludes_global_inversion() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
        let (mut proved, mut checked) = (0, 0);
        for name in [
            "revolution_shaft.step.gz",
            "revolution_line_shaft.step.gz",
            "revolution_cone.step.gz",
            "truck_master_shaft_only.step.gz",
            "truck_master_shaft_hole.step.gz",
            "captured/frames/box_moved.step",
            "full_torus.step",
            "filleted_box_one_edge.step",
            "blend_r5.step",
        ] {
            let part = crate::read_step_file(&dir.join(name)).unwrap();
            for (reading, p, inverted) in [
                (
                    "outward",
                    Part::new(part.faces.clone(), part.edges.clone(), part.solids.clone()),
                    false,
                ),
                ("normals inverted", flipped(&part, false, None), true),
                ("wires inverted", flipped(&part, true, None), true),
                ("mixed", flipped(&part, false, Some(0)), false),
            ] {
                for solid in 0..p.solids.len() {
                    if !p.solid_is_valid(solid) {
                        continue;
                    }
                    checked += 1;
                    let cheap = outward_proved(&p, solid);
                    let full = all_inward(&p, solid);
                    assert!(
                        !(cheap && full),
                        "{name} {reading} solid {solid}: proved outward, but the full test finds every face inward"
                    );
                    // The shaft's mixed input normals are now proved and repaired at
                    // import, so these synthetic readings start from a coherent boundary.
                    if inverted {
                        assert!(
                            !cheap,
                            "{name} {reading} solid {solid}: inverted shell proved outward"
                        );
                    } else if reading == "outward" {
                        assert!(cheap, "{name} solid {solid}: outward shell not proved");
                    }
                    proved += usize::from(cheap);
                }
            }
        }
        assert!(checked >= 36 && proved > 0, "{proved} proved of {checked}");
    }
}
