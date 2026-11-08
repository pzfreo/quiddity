//! The convex-corner probe shared by the bevel and blend families (`quiddity._bevel`).

use std::collections::BTreeMap;

use super::Context;
use super::policy::{AXIS_ALIGNED_COS, INTERIOR_PROBE_FRAC};
use crate::kernel::brep::Part;
use crate::kernel::classify::material_at;
use crate::kernel::geom::{Bounds, Surface, V3};

/// The in-plane component below which a normal runs along that axis, so the face is a
/// single-axis bevel rather than a compound corner.
const RUN_AXIS_COS: f64 = 0.05;

/// Why a face is not a single-axis oblique planar bevel (`BevelReject`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BevelReject {
    Nonplanar,
    /// Axis-aligned, or a shallow draft: a real face, not a bevel.
    Aligned,
    /// Oblique on all three axes: a corner bevel.
    Compound,
}

/// A single-axis oblique planar bevel (`classify_bevel`).
#[derive(Clone, Copy, Debug)]
pub struct Bevel {
    /// The axis the bevelled edge runs along.
    pub edge_axis: usize,
    pub normal: V3,
    pub bounds: Bounds,
    /// The two in-plane leg lengths, longer first.
    pub leg_hi: f64,
    pub leg_lo: f64,
}

internal_entry! {
pub fn classify_bevel(part: &Part, face: usize) -> Result<Bevel, BevelReject> {
    let Surface::Plane { .. } = part.faces[face].surface else {
        return Err(BevelReject::Nonplanar);
    };
    let normal = part
        .face_normal(face, 0.0, 0.0)
        .ok_or(BevelReject::Nonplanar)?;
    if normal.iter().any(|c| c.abs() > AXIS_ALIGNED_COS) {
        return Err(BevelReject::Aligned);
    }
    let edge_axis = (0..3)
        .find(|&i| normal[i].abs() < RUN_AXIS_COS)
        .ok_or(BevelReject::Compound)?;
    let bounds = part.face_bounds(face);
    let legs: Vec<f64> = (0..3)
        .filter(|&j| j != edge_axis)
        .map(|j| bounds.max[j] - bounds.min[j])
        .collect();
    Ok(Bevel {
        edge_axis,
        normal,
        bounds,
        leg_hi: legs[0].max(legs[1]),
        leg_lo: legs[0].min(legs[1]),
    })
}
}

internal_entry! {
/// The point just off the virtual sharp corner a bevel replaces: the corner sits where the two
/// neighbour planes cross, at the bevel's own position along its edge; *toward* 1 nudges it to
/// the bevel's side, -1 to the far side (`_near_corner`).
pub fn near_corner(centre: V3, edge_axis: usize, planes: &BTreeMap<usize, f64>, toward: f64) -> V3 {
    let mut corner = [0.0; 3];
    corner[edge_axis] = centre[edge_axis];
    for (&axis, &coord) in planes {
        corner[axis] = coord;
    }
    let step = toward * INTERIOR_PROBE_FRAC;
    [0, 1, 2].map(|i| corner[i] + step * (centre[i] - corner[i]))
}
}

internal_entry! {
/// Is there material just beyond the virtual corner, away from the bevel
/// (`material_beyond_corner`)? A bevel on a recess wall replaces the corner where two of its
/// walls meet, with the stock behind it; a bevel on an edge of the part has free space there.
pub fn material_beyond_corner(
    ctx: &Context<'_>,
    centre: V3,
    edge_axis: usize,
    planes: &BTreeMap<usize, f64>,
) -> bool {
    material_at(
        ctx.classifier(),
        near_corner(centre, edge_axis, planes, -1.0),
    )
}
}

internal_entry! {
/// Does the virtual sharp corner the bevel replaces lie outside the solid (`convex_bevel`)?
/// Only a clean `IN` counts as material, as with `BRepClass3d`.
pub fn convex_bevel(
    ctx: &Context<'_>,
    centre: V3,
    edge_axis: usize,
    planes: &BTreeMap<usize, f64>,
) -> bool {
    !material_at(
        ctx.classifier(),
        near_corner(centre, edge_axis, planes, 1.0),
    )
}
}

/// A bevel's virtual corner must stand off its own cutting plane, into free space. A nearest
/// support tie may include the bevel's terminal edge, which is not a removed sharp corner.
/// Evaluate all tied support pairs and accept only one geometrically distinct proved corner.
pub(crate) fn proved_bevel_planes(
    ctx: &super::Context<'_>,
    face: usize,
    centre: V3,
    edge_axis: usize,
) -> Option<BTreeMap<usize, f64>> {
    let crate::kernel::geom::Surface::Plane { frame } = ctx.part.faces[face].surface else {
        return None;
    };
    let candidates =
        super::planes::nearest_axis_aligned_plane_candidates(ctx.part, face, centre, edge_axis);
    // Additional evidence resolves competing supports; preserve the existing single-support
    // API when no tie exists. Auditing untied bevel hypotheses is a separate geometric task.
    let ambiguous = candidates.values().any(|coords| {
        let lo = coords.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = coords.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        hi - lo > crate::kernel::geom::COORD_FLOOR
    });
    let others: Vec<_> = (0..3).filter(|&i| i != edge_axis).collect();
    let mut proved: Option<(V3, BTreeMap<usize, f64>)> = None;
    for &a in candidates.get(&others[0])? {
        for &b in candidates.get(&others[1])? {
            let planes = BTreeMap::from([(others[0], a), (others[1], b)]);
            let mut corner = centre;
            corner[others[0]] = a;
            corner[others[1]] = b;
            if (ambiguous
                && crate::kernel::geom::dot(
                    crate::kernel::geom::sub(corner, frame.origin),
                    frame.z,
                )
                .abs()
                    <= crate::kernel::geom::COORD_FLOOR)
                || !convex_bevel(ctx, centre, edge_axis, &planes)
            {
                continue;
            }
            if let Some((previous, _)) = &proved {
                if crate::kernel::py::dist(previous, &corner) > crate::kernel::geom::COORD_FLOOR {
                    return None;
                }
            } else {
                proved = Some((corner, planes));
            }
        }
    }
    proved.map(|(_, planes)| planes)
}

/// Check a corner in the actual support basis, before the caller-coordinate publication
/// grid is applied. Point/leg intervals include that grid's uncertainty; a fixed angular
/// epsilon would incorrectly reject short, correctly rounded legs. Probe the open wedge:
/// contact with its boundary, including the bevel itself, is allowed at model/grid precision.
pub(crate) fn proved_published_corner(
    ctx: &Context<'_>,
    faces: &[usize],
    axis: usize,
    at: V3,
    corner: V3,
    legs: [f64; 2],
    angle: f64,
) -> Option<super::placed::Placed<V3>> {
    use super::placed::{Grid, Placed};
    use crate::kernel::{
        geom,
        rays::RayCaster,
        volume::{Probe, fills_at_most},
    };
    let face = *faces.first()?; // caps are evidence, not an extension of the bevel's run
    let mut run = [0.; 3];
    run[axis] = 1.;
    let transverse = match axis {
        0 => [1, 2],
        1 => [0, 2],
        _ => [0, 1],
    };
    let mut basis = transverse.map(|i| {
        let mut n = [0.; 3];
        n[i] = 1.;
        n
    });
    // Principal-run records can have rolled walls. Their legs lie on the actual supports,
    // not on the caller's coordinate axes; retain the existing representation and corner.
    if let Some(support) = transverse_corner(ctx.part, face, at, run)
        && support
            .normals
            .iter()
            .any(|n| n.iter().all(|c| c.abs() < AXIS_ALIGNED_COS))
    {
        let tangents = support.normals.map(|n| geom::unit(geom::cross(run, n)));
        basis = [tangents[0]?, tangents[1]?];
    }
    let delta = geom::sub(at, corner);
    let projected = basis.map(|n| 2. * geom::dot(delta, n));
    // Supports may differ slightly from a right angle within their existing tolerance.
    // Resolve in their basis, rather than mistaking dot products for coordinates.
    let skew = geom::dot(basis[0], basis[1]);
    let components = [
        (projected[0] - skew * projected[1]) / (1. - skew * skew),
        (projected[1] - skew * projected[0]) / (1. - skew * skew),
    ];
    let mut implied = components.map(f64::abs);
    implied.sort_by(|a, b| b.total_cmp(a));
    let mut legs = legs;
    legs.sort_by(|a, b| b.total_cmp(a));
    // Each point coordinate has half a 0.001 mm cell of uncertainty. Subtracting two
    // points, doubling, and expressing them in an arbitrary unit basis costs sqrt(3)*.002;
    // the measured leg contributes another half cell. The angle has a .01 degree cell.
    let model_tol = cylinder_support_tolerance(ctx.part);
    let length_tol = 3f64.sqrt() * 0.002 + 0.0005 + model_tol;
    let angle_lo = (implied[1] - length_tol)
        .max(0.)
        .atan2(implied[0] + length_tol)
        .to_degrees();
    let angle_hi = (implied[1] + length_tol)
        .atan2((implied[0] - length_tol).max(0.))
        .to_degrees();
    if implied
        .iter()
        .zip(legs)
        .any(|(a, b)| (a - b).abs() > length_tol)
        || angle + 0.005 < angle_lo
        || angle - 0.005 > angle_hi
    {
        return None;
    }
    let triangle = [
        corner,
        geom::add(corner, geom::scale(basis[0], components[0])),
        geom::add(corner, geom::scale(basis[1], components[1])),
    ];
    // Inset all three sides by the coordinate uncertainty of 2*at-corner. A wedge
    // touching the part on its boundary has no material interior. Never allow a filled
    // fraction: positive volume beyond this dimensional tolerance still refuses the corner.
    let inset = 3f64.sqrt() * 0.0015 + model_tol;
    let sides = [
        geom::norm(geom::sub(triangle[1], triangle[2])),
        geom::norm(geom::sub(triangle[0], triangle[2])),
        geom::norm(geom::sub(triangle[0], triangle[1])),
    ];
    let perimeter: f64 = sides.iter().sum();
    let radius = geom::norm(geom::cross(
        geom::sub(triangle[1], triangle[0]),
        geom::sub(triangle[2], triangle[0]),
    )) / perimeter;
    if radius <= inset {
        return None;
    }
    let centre = (0..3).fold([0.; 3], |sum, i| {
        geom::add(sum, geom::scale(triangle[i], sides[i] / perimeter))
    });
    let triangle = triangle.map(|p| {
        geom::add(
            centre,
            geom::scale(geom::sub(p, centre), 1. - inset / radius),
        )
    });
    // The full section runs only where both spring edges exist. A curved/oblique
    // terminal can extend the face box beyond one spring edge, into adjacent material.
    // Accumulate split straight edges on each reconstructed leg's carrier line.
    let mut spring = [(f64::INFINITY, f64::NEG_INFINITY); 2];
    for e in ctx.part.outer_edges(face) {
        let edge = &ctx.part.edges[e];
        if !matches!(edge.curve, geom::Curve::Line { .. }) {
            continue;
        }
        let offset = geom::sub(edge.end, edge.start);
        if geom::norm(geom::cross(offset, run)) > model_tol {
            continue;
        }
        for (i, endpoint) in [
            geom::add(corner, geom::scale(basis[0], components[0])),
            geom::add(corner, geom::scale(basis[1], components[1])),
        ]
        .into_iter()
        .enumerate()
        {
            let d = geom::sub(edge.start, endpoint);
            if geom::norm(geom::sub(d, geom::scale(run, geom::dot(d, run)))) > length_tol {
                continue;
            }
            for p in edge.vertex_points() {
                let s = geom::dot(p, run);
                spring[i].0 = spring[i].0.min(s);
                spring[i].1 = spring[i].1.max(s);
            }
        }
    }
    let (lo, hi) = if spring.iter().all(|&(lo, hi)| lo < hi) {
        (spring[0].0.max(spring[1].0), spring[0].1.min(spring[1].1))
    } else {
        // No pair of straight spring edges: probe the entire bevel-face span. A cap is
        // still excluded, and lack of a run proof never bypasses the volume test.
        ctx.part.extent_along(&[face], run)
    };
    if hi - lo <= 2. * inset {
        return None;
    }
    let station = |p, s| geom::add(p, geom::scale(run, s - geom::dot(p, run)));
    let low = triangle.map(|p| station(p, lo + inset));
    let high = triangle.map(|p| station(p, hi - inset));
    let wedge = super::support_patches::ruled_prism(&low, &high)?;
    let probe = Probe::Solid(RayCaster::for_solid(&wedge, 0));
    fills_at_most(ctx.classifier(), &probe, 0.).then_some(Placed::new(corner, Grid::Decimals(3)))
}

/// The two supporting planes intersected at a station along the run. Their normals and the
/// corner are measured in caller coordinates; the planes need not be principal planes.
pub(crate) struct TransverseCorner {
    pub point: V3,
    pub normals: [V3; 2],
    pub areas: [f64; 2],
}

/// Read two distinct transverse supports in one neighbour traversal. Coplanar splits are one
/// support; a third direction or a distinct parallel plane is a competing corner claim.
pub(crate) fn transverse_corner(
    part: &Part,
    face: usize,
    centre: V3,
    run: V3,
) -> Option<TransverseCorner> {
    let corner = transverse_wall_corner(part, face, centre, run)?;
    (crate::kernel::geom::dot(corner.normals[0], corner.normals[1]).abs()
        <= crate::kernel::geom::SMOOTH_ARC_GAP)
        .then_some(corner)
}

/// The same complete support discovery with an explicit feature angular/contact policy.
/// Legacy bevel and blend callers retain their existing defaults above/below.
pub(crate) fn transverse_corner_with_tolerance(
    part: &Part,
    face: usize,
    centre: V3,
    run: V3,
    angular_tolerance: f64,
    contact_tolerance: f64,
) -> Option<TransverseCorner> {
    let corner = transverse_wall_corner_where(
        part,
        face,
        centre,
        run,
        angular_tolerance,
        contact_tolerance,
        |_, _| true,
    )?;
    (crate::kernel::geom::dot(corner.normals[0], corner.normals[1]).abs() <= angular_tolerance)
        .then_some(corner)
}

/// Two nonparallel transverse walls, at any angle. Callers prove the treatment's own
/// angle contract; chamfers retain their perpendicular-wall requirement above.
pub(crate) fn transverse_wall_corner(
    part: &Part,
    face: usize,
    centre: V3,
    run: V3,
) -> Option<TransverseCorner> {
    transverse_wall_corner_where(
        part,
        face,
        centre,
        run,
        crate::kernel::geom::SMOOTH_ARC_GAP,
        crate::kernel::geom::COORD_FLOOR,
        |_, _| true,
    )
}

/// Contact cannot require more precision than the STEP input or kernel coordinates.
pub(crate) fn cylinder_support_tolerance(part: &Part) -> f64 {
    // The STEP reader converts its declared modelling precision to millimetres. Never
    // demand more accurate contact than the input states, or less than kernel coordinates.
    part.file_uncertainty()
        .unwrap_or(crate::kernel::geom::COORD_FLOOR)
        .max(crate::kernel::geom::COORD_FLOOR)
}

/// A cylinder's supports must be tangent before they can compete. A transverse cut/end
/// plane that is not tangent does not propose another rounded corner (#120).
pub(crate) fn tangent_cylinder_corner(
    part: &Part,
    face: usize,
    centre: V3,
    run: V3,
    origin: V3,
    radius: f64,
) -> Option<TransverseCorner> {
    transverse_wall_corner_where(
        part,
        face,
        centre,
        run,
        crate::kernel::geom::SMOOTH_ARC_GAP,
        crate::kernel::geom::COORD_FLOOR,
        |point, normal| {
            (crate::kernel::geom::dot(crate::kernel::geom::sub(origin, point), normal).abs()
                - radius)
                .abs()
                <= cylinder_support_tolerance(part)
        },
    )
}

fn transverse_wall_corner_where(
    part: &Part,
    face: usize,
    centre: V3,
    run: V3,
    angular_tolerance: f64,
    contact_tolerance: f64,
    include: impl Fn(V3, V3) -> bool,
) -> Option<TransverseCorner> {
    use super::planes::unit_plane_normal;
    use crate::kernel::geom;
    let mut supports: Vec<(V3, V3, f64)> = Vec::with_capacity(2);
    for f in part.neighbours(face) {
        let Surface::Plane { frame } = part.faces[f].surface else {
            continue;
        };
        let normal = unit_plane_normal(part, f)?;
        if geom::dot(normal, run).abs() > angular_tolerance || !include(frame.origin, normal) {
            continue;
        }
        let area = part.face_mass(f)?[0];
        if let Some((_, origin, total)) = supports
            .iter_mut()
            .find(|(n, _, _)| geom::dot(*n, normal).abs() >= 1. - angular_tolerance)
        {
            if geom::dot(geom::sub(frame.origin, *origin), normal).abs() > contact_tolerance {
                return None;
            }
            *total += area;
            continue;
        }
        if supports.len() == 2 {
            return None;
        }
        supports.push((normal, frame.origin, area));
    }
    let [(a, pa, aa), (b, pb, ab)] = supports.as_slice() else {
        return None;
    };
    let d = geom::dot(*a, *b);
    let da = geom::dot(geom::sub(*pa, centre), *a);
    let db = geom::dot(geom::sub(*pb, centre), *b);
    Some(TransverseCorner {
        point: geom::add(
            centre,
            geom::add(
                geom::scale(*a, (da - d * db) / (1. - d * d)),
                geom::scale(*b, (db - d * da) / (1. - d * d)),
            ),
        ),
        normals: [*a, *b],
        areas: [*aa, *ab],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::support_patches::polygon_face;
    use crate::kernel::brep::{Face, Loop};
    use crate::kernel::geom::{self, Frame};

    /// A support-query B-rep built entirely in code: an R10 cylinder with two tangent
    /// rectangular walls (98° interior), plus one transverse rectangle. Shared run edges
    /// establish incidence; no solid/classification claim is made by this helper test.
    fn cylinder_supports(extra_degrees: f64, extra_distance: f64) -> Part {
        let run = [0., 0., 1.];
        let mut edges = Vec::new();
        let mut walls = Vec::new();
        let mut shared = Vec::new();
        for (degrees, distance) in [(0_f64, 10.), (82., 10.), (extra_degrees, extra_distance)] {
            let (s, c) = degrees.to_radians().sin_cos();
            let normal = [c, s, 0.];
            let across = geom::cross(run, normal);
            // The shared edge lies on the cylinder even for an inset, non-tangent plane.
            let point = geom::add(
                geom::scale(normal, distance),
                geom::scale(across, (100. - distance * distance).max(0.).sqrt()),
            );
            let far = geom::add(point, geom::scale(across, 2.));
            let patch = polygon_face(&[
                point,
                far,
                geom::add(far, geom::scale(run, 5.)),
                geom::add(point, geom::scale(run, 5.)),
            ])
            .unwrap();
            let offset = edges.len();
            let mut wall = patch.faces[0].clone();
            for lp in &mut wall.loops {
                for (edge, _) in &mut lp.edges {
                    *edge += offset;
                }
            }
            shared.push((offset + 3, true));
            edges.extend(patch.edges);
            walls.push(wall);
        }
        let mut faces = vec![Face {
            surface: crate::kernel::geom::Surface::Cylinder {
                frame: Frame {
                    origin: [0.; 3],
                    x: [1., 0., 0.],
                    y: [0., 1., 0.],
                    z: run,
                },
                radius: 10.,
            },
            reversed: false,
            solid: None,
            pcurves: vec![],
            loops: vec![Loop {
                edges: shared,
                vertex: None,
            }],
        }];
        faces.extend(walls);
        Part::new(faces, edges, vec![])
    }

    #[test]
    fn only_tangent_planes_compete_for_a_cylinder_corner() {
        let origin = [0.; 3];
        let run = [0., 0., 1.];
        let part = cylinder_supports(30., 10. - 0.164);
        assert!(transverse_wall_corner(&part, 0, origin, run).is_none());
        let corner = tangent_cylinder_corner(&part, 0, origin, run, origin, 10.).unwrap();
        assert!(
            (geom::dot(corner.normals[0], corner.normals[1]) - 82_f64.to_radians().cos()).abs()
                < 1e-12
        );
        for n in corner.normals {
            assert!((geom::dot(corner.point, n).abs() - 10.).abs() < 1e-12);
        }
        // Preserve the original guard's competing direction and distinct parallel-plane
        // refusals. A coplanar split is still one support, with both patches' area.
        for (degrees, accepted) in [(30., false), (180., false), (0., true)] {
            let part = cylinder_supports(degrees, 10.);
            assert_eq!(
                tangent_cylinder_corner(&part, 0, origin, run, origin, 10.).is_some(),
                accepted
            );
        }
    }
}
