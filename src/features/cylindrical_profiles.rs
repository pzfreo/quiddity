//! Residual cylindrical profile surfaces, after all accepted feature claims (#127).
//!
//! This is a projection of the completed recognition, like section recesses: rejected
//! candidates cannot reserve faces, and neither rounds nor blends can be displaced.
use std::collections::BTreeSet;
use std::f64::consts::{PI, TAU};

use serde::Serialize;

use super::cylinders::coaxial_axis_lines;
use super::fillets::{self, FilletOptions};
use super::placed::{Grid, Placed};
use super::{Context, Features, Kept, Occurrence, kept, skip_unproved};
use crate::kernel::brep::Part;
use crate::kernel::geom::{self, COORD_FLOOR, Frame, Surface, V3, dominant_axis};
use crate::kernel::py;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ProfileAxis {
    Principal(char),
    Direction(V3),
}

impl ProfileAxis {
    /// Letters are exact principal directions, not an eligibility gate or approximation.
    fn new(direction: V3) -> Self {
        let axis = dominant_axis(direction.map(f64::abs));
        let direction = geom::scale(direction, if direction[axis] < 0. { -1. } else { 1. });
        if (0..3).all(|i| i == axis || direction[i].abs() <= 1e-12) {
            Self::Principal(['x', 'y', 'z'][axis])
        } else {
            Self::Direction(direction)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct CylindricalProfile {
    pub radius: f64,
    pub axis: ProfileAxis,
    /// A point on the actual surface axis, at the middle of the faces' axial extent.
    pub axis_point: Placed<V3>,
    pub side: &'static str,
    /// Angular coverage in radians, with overlapping axial splits counted once.
    pub sweep: f64,
    pub faces: Vec<usize>,
}

/// Register only after reconciliation, projected recesses and completed defining claims.
pub(super) fn append(ctx: &Context<'_>, features: &mut Features) {
    let claimed = features
        .defining
        .iter()
        .filter(|(family, _)| {
            !matches!(
                **family,
                "thin_wall_bodies"
                    | "sheet_metal_bodies"
                    | "plates"
                    | "polygonal_stock"
                    | "turned_bodies"
            )
        })
        .flat_map(|(_, records)| records.iter().flatten().copied())
        .chain(
            features
                .discovery_refusals
                .values()
                .flatten()
                .flat_map(|r| r.faces.iter().copied()),
        )
        .chain(
            features
                .interior_void_refusals
                .iter()
                .flat_map(|r| r.candidate.void_faces.iter().copied()),
        )
        .chain(
            features
                .modelled_thread_refusals
                .iter()
                .flat_map(|r| r.faces.iter().copied()),
        )
        .chain(
            features
                .section_recess_refusals
                .iter()
                .flat_map(|r| r.evidence().defining_faces().iter().copied()),
        )
        .collect();
    let found = skip_unproved(ctx.part, discover(ctx, &claimed));
    if found.is_empty() {
        return;
    }
    let mut registration = Kept {
        part: ctx.part,
        claimed: Default::default(),
        defining: Default::default(),
        constituent: Default::default(),
        evidence_status: Default::default(),
    };
    features.cylindrical_profiles = kept(&mut registration, "cylindrical_profiles", found);
    features.defining.extend(registration.defining);
    features
        .evidence_status
        .extend(registration.evidence_status);
}

#[derive(Clone, Copy)]
struct Patch {
    face: usize,
    frame: Frame,
    radius: f64,
    uv: (f64, f64, f64, f64),
    convex: bool,
}

fn same_surface(part: &Part, a: Patch, b: Patch) -> bool {
    let tolerance = part
        .file_uncertainty()
        .unwrap_or(COORD_FLOOR)
        .max(COORD_FLOOR);
    part.faces[a.face].solid == part.faces[b.face].solid
        && a.convex == b.convex
        && (a.radius - b.radius).abs() <= tolerance
        && coaxial_axis_lines(
            a.frame.origin,
            a.frame.z,
            b.frame.origin,
            b.frame.z,
            tolerance,
        )
}

/// Connected patches of one surface, never a pattern of congruent separate faces.
fn components(part: &Part, patches: &[Patch]) -> Vec<Vec<Patch>> {
    let mut remaining: BTreeSet<_> = (0..patches.len()).collect();
    let mut out = Vec::new();
    while let Some(seed) = remaining.pop_first() {
        let mut group = vec![patches[seed]];
        let mut visit = 0;
        while visit < group.len() {
            let patch = group[visit];
            let neighbours = part.neighbours(patch.face);
            let next: Vec<_> = remaining
                .iter()
                .copied()
                .filter(|&i| {
                    neighbours.contains(&patches[i].face)
                        && same_surface(part, patches[seed], patches[i])
                })
                .collect();
            for i in next {
                remaining.remove(&i);
                group.push(patches[i]);
            }
            visit += 1;
        }
        group.sort_by_key(|p| p.face);
        out.push(group);
    }
    out
}

/// Angular union in one geometric basis, including opposite/seam-crossing UV frames.
fn sweep(patches: &[Patch]) -> f64 {
    let reference = patches[0].frame;
    let x = reference.x;
    let y = geom::cross(reference.z, x);
    let mut intervals = Vec::new();
    for p in patches {
        let (u0, u1, _, _) = p.uv;
        let extent = u1 - u0;
        if extent >= TAU {
            return TAU;
        }
        let radial = geom::add(
            geom::scale(p.frame.x, u0.cos()),
            geom::scale(p.frame.y, u0.sin()),
        );
        let angle = geom::dot(radial, y).atan2(geom::dot(radial, x));
        let positive = geom::dot(geom::cross(p.frame.x, p.frame.y), reference.z) > 0.;
        let start = (angle - if positive { 0. } else { extent }).rem_euclid(TAU);
        let end = start + extent;
        intervals.push((start, end.min(TAU)));
        if end > TAU {
            intervals.push((0., end - TAU));
        }
    }
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (mut total, mut end) = (0_f64, 0_f64);
    for (lo, hi) in intervals {
        total += (hi - lo.max(end)).max(0.);
        end = end.max(hi);
    }
    total
}

internal_entry! {
/// Native cylinders, with intrinsic axes and no frame-axis eligibility gate.
pub fn discover(ctx: &Context<'_>, claimed: &BTreeSet<usize>) -> Vec<Occurrence<CylindricalProfile>> {
    let part = ctx.part;
    let bounds = ctx.bounds();
    let smallest = (0..3).map(|i| bounds.max[i] - bounds.min[i]).fold(f64::INFINITY, f64::min);
    let concave_min = 2_f64.min(0.05 * smallest);
    let patches: Vec<_> = part.faces.iter().enumerate().filter_map(|(face, f)| {
        let Surface::Cylinder { frame, radius } = f.surface else { return None; };
        let uv = part.uv_bounds(face)?;
        (radius.is_finite() && radius > 0. && uv.1 > uv.0).then_some(Patch {
            face, frame, radius, uv, convex: part.frame_points_outward(face)?,
        })
    }).collect();
    if !patches.iter().any(|p| !claimed.contains(&p.face)
        && p.uv.1 - p.uv.0 < PI && (p.convex || p.radius >= concave_min)) {
        return Vec::new();
    }
    // Test the fillet proof even outside its default reporting radius band (#127).
    let rounds: BTreeSet<_> = fillets::discover(ctx, &FilletOptions {
        min_radius: Some(0.), max_radius_frac: f64::INFINITY, include_cylindrical: true,
    }).into_iter().flat_map(|o| o.defining).collect();
    let mut out = Vec::new();
    for surface in components(part, &patches) {
        // Check the entire connected surface before filtering claims: splitting a bore
        // into sub-180-degree patches must never turn it into a profile.
        if sweep(&surface) >= PI { continue; }
        let eligible: Vec<_> = surface.into_iter().filter(|p| {
            !claimed.contains(&p.face) && !rounds.contains(&p.face)
                && (p.convex || p.radius >= concave_min)
        }).collect();
        for group in components(part, &eligible) {
            let p = group[0];
            let direction = p.frame.z;
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for patch in &group {
                for v in [patch.uv.2, patch.uv.3] {
                    let point = geom::add(patch.frame.origin, geom::scale(patch.frame.z, v));
                    let along = geom::dot(geom::sub(point, p.frame.origin), direction);
                    lo = lo.min(along);
                    hi = hi.max(along);
                }
            }
            let faces: Vec<_> = group.iter().map(|p| p.face).collect();
            out.push(Occurrence {
                record: CylindricalProfile {
                    radius: py::round_to3(p.radius),
                    axis: ProfileAxis::new(direction),
                    axis_point: Placed::new(geom::add(p.frame.origin, geom::scale(direction, 0.5 * (lo + hi))), Grid::Decimals(3)),
                    side: if p.convex { "convex" } else { "concave" },
                    sweep: sweep(&group),
                    faces: faces.clone(),
                },
                defining: faces,
                context: vec![],
            });
        }
    }
    // Face order is deterministic and preserves separate congruent occurrences.
    out.sort_by(|a, b| a.defining.cmp(&b.defining));
    out
}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::brep::{Edge, Face, Loop, Pcurve};
    use crate::kernel::geom::Curve;

    fn frame(origin: V3) -> Frame {
        Frame {
            origin,
            x: [1., 0., 0.],
            y: [0., 1., 0.],
            z: [0., 0., 1.],
        }
    }

    /// Analytic rectangular cylinder patches, sharing an edge exactly when split.
    fn patch(
        faces: &mut Vec<Face>,
        edges: &mut Vec<Edge>,
        frame: Frame,
        radius: f64,
        uv: (f64, f64, f64, f64),
        concave: bool,
    ) {
        let surface = Surface::Cylinder { frame, radius };
        let (u0, u1, v0, v1) = uv;
        let corners = [(u0, v0), (u1, v0), (u1, v1), (u0, v1)];
        let mut uses = Vec::new();
        let mut pcurves = Vec::new();
        for i in 0..4 {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            let start = surface.value(a.0, a.1);
            let end = surface.value(b.0, b.1);
            let shared = edges.iter().enumerate().find_map(|(e, edge)| {
                if geom::dist(start, edge.start) < 1e-10 && geom::dist(end, edge.end) < 1e-10 {
                    Some((e, true))
                } else if geom::dist(start, edge.end) < 1e-10 && geom::dist(end, edge.start) < 1e-10
                {
                    Some((e, false))
                } else {
                    None
                }
            });
            let (e, forward) = shared.unwrap_or_else(|| {
                let e = edges.len();
                let curve = if a.1 == b.1 {
                    Curve::Circle {
                        frame: Frame {
                            origin: geom::add(frame.origin, geom::scale(frame.z, a.1)),
                            ..frame
                        },
                        radius,
                    }
                } else {
                    Curve::Line {
                        origin: start,
                        dir: geom::unit(geom::sub(end, start)).unwrap(),
                    }
                };
                let samples = (0..=16)
                    .map(|j| {
                        let t = j as f64 / 16.;
                        surface.value(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
                    })
                    .collect();
                edges.push(Edge {
                    curve,
                    start,
                    end,
                    vertices: (2 * e, 2 * e + 1),
                    // Top arcs run against the circle parameter; transformed Parts
                    // resample from this sense rather than our explicit samples.
                    same_sense: a.1 != b.1 || b.0 > a.0,
                    samples,
                });
                (e, true)
            });
            uses.push((e, forward));
            pcurves.push((e, Pcurve::Poles(vec![a, b])));
        }
        faces.push(Face {
            surface,
            reversed: concave,
            loops: vec![Loop {
                edges: uses,
                vertex: None,
            }],
            solid: None,
            pcurves,
        });
    }

    fn profiles(part: &Part) -> Vec<Occurrence<CylindricalProfile>> {
        discover(&Context::new(part), &BTreeSet::new())
    }

    #[test]
    fn constructed_convex_and_concave_profiles_in_any_direction() {
        let (mut faces, mut edges) = (Vec::new(), Vec::new());
        patch(
            &mut faces,
            &mut edges,
            frame([0.; 3]),
            10.,
            (0., 1., 0., 20.),
            false,
        );
        patch(
            &mut faces,
            &mut edges,
            frame([30., 0., 0.]),
            8.,
            (0., 1.2, 0., 20.),
            true,
        );
        let part = Part::new(faces, edges, vec![]);
        let found = profiles(&part);
        assert_eq!(found.len(), 2);
        assert_eq!(
            (found[0].record.radius, found[0].record.side),
            (10., "convex")
        );
        assert_eq!(
            (found[1].record.radius, found[1].record.side),
            (8., "concave")
        );
        assert_eq!(found[0].record.axis, ProfileAxis::Principal('z'));
        assert_eq!(*found[0].record.axis_point, [0., 0., 10.]);
        let (s, c) = 37_f64.to_radians().sin_cos();
        let moved = part
            .transformed(&[[c, 0., s, 12.], [0., 1., 0., -4.], [-s, 0., c, 7.]])
            .unwrap();
        let found = profiles(&moved);
        assert_eq!(found.len(), 2);
        let ProfileAxis::Direction(axis) = found[0].record.axis else {
            panic!("oblique axis lost");
        };
        assert!(geom::dist(axis, [s, 0., c]) < 1e-12);
        assert_eq!(found[0].defining, vec![0]);
        assert_eq!(found[1].record.side, "concave");
        assert!((found[1].record.sweep - 1.2).abs() < 1e-12);
        // Use the same registration as the aggregate to pin evidence and fingerprint roles.
        let mut f = Features::default();
        append(&Context::new(&moved), &mut f);
        let fingerprints = crate::correspondence::fingerprint::fingerprint(&moved, &f);
        assert_eq!(fingerprints.features.len(), 2);
        for fp in fingerprints.features {
            assert_eq!(fp.family, "cylindrical_profiles");
            assert!(fp.sizes.contains_key("radius"));
            assert!(fp.sizes.contains_key("sweep"));
            assert!(!fp.sizes.contains_key("faces"));
            assert!(fp.traits.contains_key("side"));
        }
    }

    #[test]
    fn split_surfaces_merge_but_separate_congruent_faces_do_not() {
        let (mut faces, mut edges) = (Vec::new(), Vec::new());
        // Both angular and axial splits; crossing the 2PI seam is intentional.
        for uv in [
            (5.8, 6.3, 0., 10.),
            (6.3, 6.8, 0., 10.),
            (5.8, 6.3, 10., 20.),
            (6.3, 6.8, 10., 20.),
        ] {
            patch(&mut faces, &mut edges, frame([0.; 3]), 10., uv, false);
        }
        patch(
            &mut faces,
            &mut edges,
            frame([30., 0., 0.]),
            10.,
            (5.8, 6.8, 0., 20.),
            false,
        );
        patch(
            &mut faces,
            &mut edges,
            frame([0.; 3]),
            10.,
            (5.8, 6.8, 30., 50.),
            false,
        );
        let part = Part::new(faces, edges, vec![]);
        let found = profiles(&part);
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].record.faces, vec![0, 1, 2, 3]);
        assert_eq!(found[0].defining, vec![0, 1, 2, 3]);
        assert!((found[0].record.sweep - 1.).abs() < 1e-12);
        assert_eq!(found[1].defining, vec![4]);
        assert_eq!(found[2].defining, vec![5]);
    }

    #[test]
    fn half_turns_and_split_bores_are_excluded_even_with_partial_claims() {
        for span in [PI, TAU] {
            let (mut faces, mut edges) = (Vec::new(), Vec::new());
            for i in 0..3 {
                patch(
                    &mut faces,
                    &mut edges,
                    frame([0.; 3]),
                    10.,
                    (i as f64 * span / 3., (i + 1) as f64 * span / 3., 0., 20.),
                    true,
                );
            }
            let part = Part::new(faces, edges, vec![]);
            assert!(profiles(&part).is_empty(), "span {span}");
            assert!(discover(&Context::new(&part), &BTreeSet::from([0])).is_empty());
        }
    }

    #[test]
    fn concave_small_radius_split_has_strict_boundary_and_no_convex_cutoff() {
        // The box is 10 mm deep, so the threshold is 0.5 mm, not 2 mm.
        let (mut faces, mut edges) = (Vec::new(), Vec::new());
        for (x, r, concave) in [(0., 0.499, true), (10., 0.5, true), (20., 0.1, false)] {
            patch(
                &mut faces,
                &mut edges,
                frame([x, 0., 0.]),
                r,
                (0., 1., 0., 10.),
                concave,
            );
        }
        // Extend Y beyond 10 mm without changing the smallest dimension (Z).
        patch(
            &mut faces,
            &mut edges,
            frame([0., 30., 0.]),
            10.,
            (0., 1., 0., 10.),
            false,
        );
        let part = Part::new(faces, edges, vec![]);
        let found = profiles(&part);
        assert_eq!(
            found.iter().map(|o| o.defining[0]).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        // At a large box the absolute 2 mm cap governs.
        let mut faces = part.faces.clone();
        let mut edges = part.edges.clone();
        patch(
            &mut faces,
            &mut edges,
            frame([100., 100., 100.]),
            2.,
            (0., 1., 0., 100.),
            true,
        );
        patch(
            &mut faces,
            &mut edges,
            frame([130., 100., 100.]),
            1.999,
            (0., 1., 0., 100.),
            true,
        );
        let part = Part::new(faces, edges, vec![]);
        let found = profiles(&part);
        assert!(found.iter().any(|o| o.defining == vec![4]));
        assert!(found.iter().all(|o| !o.defining.contains(&5)));
    }

    #[test]
    fn accepted_feature_claims_block_profiles_and_body_classifications_coown() {
        let (mut faces, mut edges) = (Vec::new(), Vec::new());
        patch(
            &mut faces,
            &mut edges,
            frame([0.; 3]),
            10.,
            (0., 1., 0., 20.),
            false,
        );
        let part = Part::new(faces, edges, vec![]);
        for family in ["blends", "holes", "section_recesses", "freeform_surfaces"] {
            let mut f = Features::default();
            f.defining.insert(family, vec![vec![0]]);
            append(&Context::new(&part), &mut f);
            assert!(f.cylindrical_profiles.is_empty(), "{family}");
        }
        let mut f = Features::default();
        f.defining.insert("thin_wall_bodies", vec![vec![0]]);
        append(&Context::new(&part), &mut f);
        assert_eq!(f.cylindrical_profiles.len(), 1);
        assert_eq!(f.defining["thin_wall_bodies"], vec![vec![0]]);
    }

    #[test]
    fn a_constructed_tangent_edge_round_stays_a_fillet_even_outside_reporting_limits() {
        use super::super::support_patches::polygon_face;
        for radius in [0.1, 1., 10.] {
            let (mut faces, mut edges) = (Vec::new(), Vec::new());
            patch(
                &mut faces,
                &mut edges,
                frame([0.; 3]),
                radius,
                (0., PI / 2., 0., 20.),
                false,
            );
            // The two tangent walls share the run edges of the quarter cylinder.
            for (a, b) in [
                ([radius, 0., 0.], [radius, -5., 0.]),
                ([0., radius, 0.], [-5., radius, 0.]),
            ] {
                let patch = polygon_face(&[
                    a,
                    b,
                    geom::add(b, [0., 0., 20.]),
                    geom::add(a, [0., 0., 20.]),
                ])
                .unwrap();
                let mut wall = patch.faces[0].clone();
                let mut remap = Vec::new();
                for e in patch.edges {
                    let shared = edges.iter().enumerate().find(|(_, other)| {
                        (geom::dist(e.start, other.start) < 1e-10
                            && geom::dist(e.end, other.end) < 1e-10)
                            || (geom::dist(e.start, other.end) < 1e-10
                                && geom::dist(e.end, other.start) < 1e-10)
                    });
                    if let Some((i, other)) = shared {
                        remap.push((i, geom::dist(e.start, other.start) < 1e-10));
                    } else {
                        remap.push((edges.len(), true));
                        edges.push(e);
                    }
                }
                for lp in &mut wall.loops {
                    for (i, forward) in &mut lp.edges {
                        let (j, same) = remap[*i];
                        *i = j;
                        *forward = *forward == same;
                    }
                }
                faces.push(wall);
            }
            let part = Part::new(faces, edges, vec![]);
            let rounds = fillets::discover(
                &Context::new(&part),
                &FilletOptions {
                    min_radius: Some(0.),
                    max_radius_frac: f64::INFINITY,
                    include_cylindrical: true,
                },
            );
            assert_eq!(rounds.len(), 1, "R{radius}");
            assert_eq!(rounds[0].defining, vec![0]);
            assert!(profiles(&part).is_empty(), "R{radius}");
        }
    }
}
