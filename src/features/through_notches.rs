//! Rectangular through-notches in a recognised plate's edge: a bottom wall and two parallel,
//! facing side walls, each spanning exactly the plate's thickness, joined at sharp concave
//! corners or through concave corner rounds, with the mouth on the plate's outer loop.
//!
//! The geometry is the U a cutter leaves when it passes through the slab from its edge. Every
//! wall must meet the plate's own faces at **both** slab ends convexly, along an edge of that
//! face's outer loop, and each side wall must end at the mouth in a convex edge with an
//! outward wall facing the same way as the bottom. Thus a closed slot (no mouth, inner
//! loops), an edge-open recess with a floor (walls stop at the floor) and the waist between
//! two thicker blocks (side walls taller than the slab) are not notches. A notch whose walls a
//! full-span channel already defines belongs to that channel. No dimension, ratio or
//! orientation beyond the plate's own axis is preferred.

use serde::Serialize;

use super::Context;
use super::channels::Channel;
use super::evidence::{self, EvidenceStatus, Occurrence};
use super::graph::normal;
use super::plates::Plate;
use super::regions::length_tolerance;
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::{self, Curve, Surface, V3};
use crate::kernel::py;

/// A rectangular through-notch in millimetres. `axis` is the plate's thickness direction (its
/// sign is a gauge), `opening` the outward normal of the bottom wall, pointing out through the
/// mouth, and `at` the centre of the bottom wall, midway between the side walls and through
/// the thickness. `width` separates the side walls, `depth` runs from the bottom wall to the
/// mouth plane and `thickness` is the slab's. `corner_radius` is that of the two concave
/// corner rounds, absent where the corners are sharp.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct RectangularThroughNotch {
    pub axis: V3,
    pub opening: V3,
    pub at: V3,
    pub width: f64,
    pub depth: f64,
    pub thickness: f64,
    pub corner_radius: Option<f64>,
    /// The index of the plate candidate the notch is cut through, in the same run.
    #[serde(skip)]
    pub(crate) plate: usize,
}

struct Slab<'a> {
    axis: usize,
    lo: f64,
    hi: f64,
    caps: &'a [usize],
    tol: f64,
}

/// The parallel-to-axis direction cosine tolerance (principal frame, as body evidence).
const PERPENDICULAR: f64 = 1e-9;

fn line(part: &Part, edge: usize) -> bool {
    matches!(part.edges[edge].curve, Curve::Line { .. })
}

impl Slab<'_> {
    fn along(&self, edge: &crate::kernel::brep::Edge) -> bool {
        let d = geom::sub(edge.end, edge.start);
        let (lo, hi) = (edge.start[self.axis].min(edge.end[self.axis]), {
            edge.start[self.axis].max(edge.end[self.axis])
        });
        (0..3)
            .filter(|&k| k != self.axis)
            .all(|k| d[k].abs() <= self.tol)
            && (lo - self.lo).abs() <= self.tol
            && (hi - self.hi).abs() <= self.tol
    }

    /// The cap faces a wall meets convexly along their outer loops, at both slab ends.
    fn ends(&self, part: &Part, face: usize) -> Option<Vec<usize>> {
        let b = part.face_bounds(face);
        if (b.min[self.axis] - self.lo).abs() > self.tol
            || (b.max[self.axis] - self.hi).abs() > self.tol
        {
            return None;
        }
        let edges = part.face_edges(face);
        let mut caps = Vec::new();
        for level in [self.lo, self.hi] {
            let cap = self.caps.iter().copied().find(|&g| {
                let gb = part.face_bounds(g);
                (gb.min[self.axis] - level).abs() <= self.tol
                    && (gb.max[self.axis] - level).abs() <= self.tol
                    && part.arc(face, g) == Some(Arc::Convex)
                    && part.outer_edges(g).iter().any(|e| edges.contains(e))
            })?;
            caps.push(cap);
        }
        Some(caps)
    }

    /// A planar wall across the slab: one loop of four lines, two of them full-thickness
    /// lines along the axis, the other two on the caps' outer loops. Its unit normal, its two
    /// axial edges, and the caps.
    fn wall(&self, part: &Part, face: usize) -> Option<(V3, [usize; 2], Vec<usize>)> {
        let f = &part.faces[face];
        if !matches!(f.surface, Surface::Plane { .. })
            || f.loops.len() != 1
            || f.loops[0].edges.len() != 4
        {
            return None;
        }
        let n = normal(part, face)?;
        if n[self.axis].abs() > PERPENDICULAR {
            return None;
        }
        let edges = part.face_edges(face);
        if edges.len() != 4 || !edges.iter().all(|&e| line(part, e)) {
            return None;
        }
        let axial: Vec<usize> = edges
            .iter()
            .copied()
            .filter(|&e| self.along(&part.edges[e]))
            .collect();
        let &[a, b] = axial.as_slice() else {
            return None;
        };
        Some((n, [a, b], self.ends(part, face)?))
    }
}

fn across(part: &Part, edge: usize, face: usize) -> Option<usize> {
    let [a, b] = part.edge_faces()[edge][..] else {
        return None;
    };
    match (a == face, b == face) {
        (true, false) => Some(b),
        (false, true) => Some(a),
        _ => None,
    }
}

/// One side of the notch from the bottom wall's axial edge: the side wall, its normal, the
/// corner round and its radius (if any), the mouth wall, the axial mouth edge and the caps.
struct Side {
    wall: usize,
    normal: V3,
    round: Option<(usize, f64)>,
    mouth: usize,
    mouth_edge: usize,
    caps: Vec<usize>,
}

fn side(part: &Part, slab: &Slab<'_>, bottom: usize, opening: V3, edge: usize) -> Option<Side> {
    let next = across(part, edge, bottom)?;
    let mut caps = Vec::new();
    let (wall, from, round) = match part.faces[next].surface {
        Surface::Plane { .. } => {
            (part.arc(bottom, next) == Some(Arc::Concave)).then_some(())?;
            (next, edge, None)
        }
        Surface::Cylinder { frame, radius } => {
            let f = &part.faces[next];
            if part.arc(bottom, next) != Some(Arc::Smooth)
                || part.frame_points_outward(next) != Some(false)
                || f.loops.len() != 1
                || f.loops[0].edges.len() != 4
                || (0..3)
                    .filter(|&k| k != slab.axis)
                    .any(|k| frame.z[k].abs() > PERPENDICULAR)
            {
                return None;
            }
            caps.extend(slab.ends(part, next)?);
            let axial: Vec<usize> = part
                .face_edges(next)
                .into_iter()
                .filter(|&e| e != edge && line(part, e) && slab.along(&part.edges[e]))
                .collect();
            let &[other] = axial.as_slice() else {
                return None;
            };
            let wall = across(part, other, next)?;
            if part.arc(next, wall) != Some(Arc::Smooth) {
                return None;
            }
            (wall, other, Some((next, radius)))
        }
        _ => return None,
    };
    if wall == bottom {
        return None;
    }
    let (n, axial, ends) = slab.wall(part, wall)?;
    if geom::dot(n, opening).abs() > PERPENDICULAR {
        return None;
    }
    caps.extend(ends);
    let mouth_edge = if axial[0] == from {
        axial[1]
    } else if axial[1] == from {
        axial[0]
    } else {
        return None;
    };
    let mouth = across(part, mouth_edge, wall)?;
    if !matches!(part.faces[mouth].surface, Surface::Plane { .. })
        || part.arc(wall, mouth) != Some(Arc::Convex)
        || geom::dot(normal(part, mouth)?, opening) < 1. - PERPENDICULAR
    {
        return None;
    }
    Some(Side {
        wall,
        normal: n,
        round,
        mouth,
        mouth_edge,
        caps,
    })
}

fn from_bottom(
    part: &Part,
    slab: &Slab<'_>,
    plate: usize,
    bottom: usize,
) -> Option<Occurrence<RectangularThroughNotch>> {
    let (opening, axial, caps) = slab.wall(part, bottom)?;
    let sides = [
        side(part, slab, bottom, opening, axial[0])?,
        side(part, slab, bottom, opening, axial[1])?,
    ];
    let [left, right] = &sides;
    if left.wall == right.wall || left.mouth == bottom || right.mouth == bottom {
        return None;
    }
    // Parallel side walls facing each other across the notch.
    let at_wall = |s: &Side| part.edges[s.mouth_edge].start;
    let width = geom::dot(left.normal, geom::sub(at_wall(right), at_wall(left)));
    if geom::dot(left.normal, right.normal) > -1. + PERPENDICULAR
        || width <= slab.tol
        || geom::dot(right.normal, geom::sub(at_wall(left), at_wall(right))) <= slab.tol
    {
        return None;
    }
    // Both mouths on one outward plane beyond the bottom wall.
    let base = part.edges[axial[0]].start;
    let depth = |s: &Side| geom::dot(opening, geom::sub(at_wall(s), base));
    let (d0, d1) = (depth(left), depth(right));
    if d0 <= slab.tol || (d0 - d1).abs() > slab.tol {
        return None;
    }
    let corner_radius = match (left.round, right.round) {
        (None, None) => None,
        (Some((_, r0)), Some((_, r1))) if (r0 - r1).abs() <= slab.tol => Some(0.5 * (r0 + r1)),
        _ => return None,
    };
    let mut defining = vec![bottom, left.wall, right.wall];
    defining.sort_unstable();
    defining.dedup();
    if defining.len() != 3 {
        return None;
    }
    let mut context: Vec<usize> = caps
        .into_iter()
        .chain(sides.iter().flat_map(|s| {
            s.caps
                .iter()
                .copied()
                .chain([s.mouth])
                .chain(s.round.map(|r| r.0))
        }))
        .collect();
    context.sort_unstable();
    context.dedup();
    if context.iter().any(|f| defining.contains(f))
        || evidence::strict_status(part, &defining, &context) != EvidenceStatus::Verified
    {
        return None;
    }
    let mid = |e: usize| part.edges[e].midpoint();
    let mut axis = [0.; 3];
    axis[slab.axis] = 1.;
    Some(Occurrence {
        record: RectangularThroughNotch {
            axis,
            opening,
            at: geom::scale(geom::add(mid(axial[0]), mid(axial[1])), 0.5),
            width,
            depth: 0.5 * (d0 + d1),
            thickness: slab.hi - slab.lo,
            corner_radius,
            plate,
        },
        defining,
        context,
    })
}

internal_entry! {
/// Every rectangular through-notch in the edges of *plates* (this run's plate candidates). A
/// face proposed by two notches defines neither, and a notch whose walls a full-span channel
/// in *channels* already defines is that channel. Sorted by position.
pub fn discover(
    ctx: &Context<'_>,
    plates: &[Occurrence<Plate>],
    channels: &[Occurrence<Channel>],
) -> Vec<Occurrence<RectangularThroughNotch>> {
    let part = ctx.part;
    let mut found = Vec::new();
    for (index, plate) in plates.iter().enumerate() {
        let Some(axis) = ['x', 'y', 'z'].iter().position(|&c| c == plate.record.axis) else {
            continue;
        };
        let Some(solid) = plate.defining.first().and_then(|&f| part.faces[f].solid) else {
            continue;
        };
        let (lo, hi) = (*plate.record.lo.raw(), *plate.record.hi.raw());
        let bounds = part.solid_bounds(solid);
        let size = (0..3)
            .flat_map(|k| [bounds.min[k], bounds.max[k]])
            .fold(0.0_f64, |m, v| m.max(v.abs()));
        let slab = Slab {
            axis,
            lo,
            hi,
            caps: &plate.defining,
            tol: length_tolerance(&[size, lo, hi]),
        };
        found.extend(
            part.solids[solid]
                .faces
                .iter()
                .filter(|f| !plate.defining.contains(f))
                .filter_map(|&f| from_bottom(part, &slab, index, f)),
        );
    }
    let mut kept: Vec<Occurrence<RectangularThroughNotch>> = found
        .iter()
        .filter(|o| {
            !found
                .iter()
                .any(|p| !std::ptr::eq(*o, p) && p.defining.iter().any(|f| o.defining.contains(f)))
                && !channels
                    .iter()
                    .any(|c| c.defining.iter().any(|f| o.defining.contains(f)))
        })
        .cloned()
        .collect();
    kept.sort_by(|a, b| {
        py::tuple_order(&a.record.at, &b.record.at)
            .then(py::order(a.record.width, b.record.width))
    });
    kept
}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::step::IDENTITY;

    /// A 300 x 160 x 12 plate (z 0..12) between two 50 x 220 end blocks (z -30..12), built
    /// with build123d: on its +y edge a 40 x 25 notch with R6 corners at x = -90, a sharp
    /// 30 x 20 notch at x = 0 and a 60 degree V notch with an R5 trough at x = 90; from its -y
    /// edge a blind 40 x 25 recess with a floor at z = 6; a closed 60 x 20 through slot with R4
    /// corners at (60, -20); R8 rounds where the plate edges meet the blocks. Generated by
    /// tools/make_plate_edge_notches_fixture.py.
    fn fixture() -> Part {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/plate_edge_notches.step");
        crate::read_step_file(&path).unwrap()
    }

    fn near(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-6, "{a} != {b}");
    }

    #[test]
    fn rounded_and_sharp_notches_are_found_and_measured_under_motion() {
        let base = fixture();
        let (s, c) = 0.41_f64.sin_cos();
        for motion in [
            IDENTITY,
            [[c, -s, 0., 31.5], [s, c, 0., -7.25], [0., 0., 1., 12.]],
        ] {
            let part = base.transformed(&motion).unwrap();
            let features = super::super::recognise(&part);
            let notches = &features.rectangular_through_notches;
            assert_eq!(notches.len(), 2, "{notches:?}");
            let faces = &features.defining["rectangular_through_notches"];
            let mut seen: Vec<_> = notches.iter().zip(faces).collect();
            seen.sort_by(|a, b| py::order(a.0.width, b.0.width));
            let (sharp, sharp_faces) = seen[0];
            let (round, round_faces) = seen[1];
            near(sharp.width, 30.);
            near(sharp.depth, 20.);
            near(sharp.thickness, 12.);
            assert_eq!(sharp.corner_radius, None);
            near(round.width, 40.);
            near(round.depth, 25.);
            near(round.thickness, 12.);
            near(round.corner_radius.unwrap(), 6.);
            // File faces: rounded notch bottom 22 and sides 18, 25; sharp 12 with 11, 14.
            assert_eq!(*round_faces, [18, 22, 25]);
            assert_eq!(*sharp_faces, [11, 12, 14]);
            // The rounded V notch is a V channel, the closed slot a slot; neither, nor the
            // floored recess (17, 19, 21, 42) or the waist walls (4, 26), is a notch.
            assert_eq!(features.defining["v_channels"], [vec![3, 7]]);
            near(features.v_channels[0].trough_radius.unwrap(), 5.);
            near(features.v_channels[0].depth, 35.);
            near(features.v_channels[0].included_angle_degrees, 60.);
            for f in [4, 17, 19, 21, 26, 42, 32, 35, 36, 39] {
                assert!(!faces.iter().flatten().any(|&g| g == f), "face {f}");
            }
        }
    }

    #[test]
    fn walls_that_do_not_span_the_slab_or_lack_a_mouth_are_not_notches() {
        let part = fixture();
        let ctx = Context::new(&part);
        let plates = super::super::plates::discover(&ctx, &Default::default());
        let plate = plates.iter().position(|p| p.record.axis == 'z').unwrap();
        assert_eq!(plates[plate].defining, [40, 0]);
        let slab = Slab {
            axis: 2,
            lo: 0.,
            hi: 12.,
            caps: &plates[plate].defining,
            tol: 1e-6,
        };
        // Bottoms of both notches qualify; every other face, as a bottom, does not: the
        // closed slot's walls (no mouth, inner cap loop), the recess walls (stop at a floor),
        // the waist (block walls taller than the slab) and plain plate edges (convex).
        for f in 0..part.faces.len() {
            let found = from_bottom(&part, &slab, plate, f).is_some();
            assert_eq!(found, f == 12 || f == 22, "face {f}");
        }
        // A thicker slab reading is not this plate: nothing spans it.
        let thick = Slab { lo: -30., ..slab };
        assert!((0..part.faces.len()).all(|f| from_bottom(&part, &thick, plate, f).is_none()));
        // A notch a channel already defines is the channel's.
        let found = discover(&ctx, &plates, &[]);
        assert_eq!(found.len(), 2);
        use super::super::placed::{Grid, Placed};
        let at = |v| Placed::new(v, Grid::Decimals(3));
        let channel = Occurrence {
            record: Channel {
                width_axis: 'x',
                long_axis: 'z',
                width: 30.,
                w_center: at(0.),
                lo: at(0.),
                hi: at(12.),
                d_lo: at(60.),
                d_hi: at(80.),
                open_sign: 1,
                body_key: None,
            },
            defining: vec![11, 14],
            context: Vec::new(),
        };
        assert_eq!(discover(&ctx, &plates, &[channel]).len(), 1);
    }

    #[test]
    fn notches_leave_with_their_rejected_plate() {
        use super::super::reconcile::{Candidate, Disposition, Family, Outcome, ReasonCode};
        let part = fixture();
        let inventory = super::super::inventory(&part).unwrap();
        let notches = &inventory.physical.rectangular_through_notches;
        assert_eq!(notches.len(), 2);
        let plate = notches[0].plate;
        assert!(notches.iter().all(|n| n.plate == plate));
        // `plate` indexes the unfiltered plate candidates, as the rejections do.
        assert_eq!(inventory.physical.plates[plate].axis, 'z');
        let plates = inventory.physical.plates.len();
        let reject = |index: usize| {
            let mut inventory = inventory.clone();
            inventory.dispositions.push(Disposition {
                candidate: Candidate {
                    family: Family::Plates,
                    index,
                    defining: Vec::new(),
                },
                outcome: Outcome::Rejected,
                reason: ReasonCode::PlateSupersededByThinWall,
                related: vec![Candidate {
                    family: Family::ThinWallBodies,
                    index: 0,
                    defining: Vec::new(),
                }],
            });
            inventory.accepted()
        };
        let kept = inventory.clone().accepted();
        assert_eq!(kept.rectangular_through_notches.len(), 2);
        let after = reject(plate);
        assert_eq!(after.plates.len(), plates - 1);
        assert!(after.rectangular_through_notches.is_empty());
        assert!(after.defining["rectangular_through_notches"].is_empty());
        assert!(after.evidence_status["rectangular_through_notches"].is_empty());
        // Rejecting another plate keeps both notches.
        if plates > 1 {
            let other = (plate + 1) % plates;
            assert_eq!(reject(other).rectangular_through_notches.len(), 2);
        }
    }
}
