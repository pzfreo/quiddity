//! Edge fillet recognition (`quiddity.fillets`).
//!
//! A prismatic fillet is a partial cylinder rounding a convex or concave edge between two tangent
//! walls; a turned fillet is a short external torus meeting a coaxial external cylinder.

use std::collections::BTreeMap;
use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

use super::Context;
use super::bevel::{cylinder_support_tolerance, tangent_cylinder_corner};
use super::cylinders::CylinderEvidence;
use super::evidence::{self, EvidenceError, Occurrence};
use super::placed::{Grid, Placed};
use super::policy::AXIS_ALIGNED_COS;
use super::turned;
use crate::kernel::brep::Part;
use crate::kernel::classify::material_at;
use crate::kernel::geom::{self, Surface, V3, dominant_axis};
use crate::kernel::py;

/// A minimum-evidence threshold, deliberately absolute (ADR 0008).
pub(super) const MIN_RADIUS: f64 = 0.6;
/// Nearly half-turn rounds between nearly parallel walls are groove/slot full-round
/// bottoms, not edge fillets (#120). Keep walls at least 45° apart.
const FILLET_MAX_EXTENT: f64 = 3.0 * PI / 4.0;
/// Reject gross sweep mismatches (a truncated bend skin), while allowing the angular
/// noise of modelled geometry such as CTC-03's 90.0246° round (#120).
const FILLET_SWEEP_TOL_DEG: f64 = 0.5;
/// A turned edge fillet is a quarter circle in section; a bead is a half or full one.
pub(super) const TURNED_FILLET_MAX_EXTENT: f64 = PI / 2.0 * 1.05;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Fillet {
    pub axis: FilletAxis,
    pub radius: f64,
    pub at: Placed<V3>,
    pub turned: bool,
    pub side: &'static str,
}

/// Principal runs keep their existing letter and serialization. Oblique runs carry a
/// full-precision unit direction, canonical with its dominant component positive.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum FilletAxis {
    Principal(char),
    Direction(V3),
}

impl FilletAxis {
    fn order(self, other: Self) -> std::cmp::Ordering {
        use FilletAxis::{Direction, Principal};
        match (self, other) {
            (Principal(a), Principal(b)) => a.cmp(&b),
            (Principal(_), Direction(_)) => std::cmp::Ordering::Less,
            (Direction(_), Principal(_)) => std::cmp::Ordering::Greater,
            (Direction(a), Direction(b)) => py::tuple_order(&a, &b),
        }
    }
}

/// Options named as `recognise_fillets`' keyword arguments.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FilletOptions {
    pub min_radius: Option<f64>,
    pub max_radius_frac: f64,
    /// `false` for a part already classified as rotational: only toroidal fillets are reported.
    pub include_cylindrical: bool,
}

impl Default for FilletOptions {
    fn default() -> Self {
        FilletOptions {
            min_radius: None,
            max_radius_frac: 0.45,
            include_cylindrical: true,
        }
    }
}

internal_entry! {
/// `recognise_fillets`: the records only.
pub fn recognise_fillets(part: &Part, opts: &FilletOptions) -> Vec<Fillet> {
    discover(&Context::new(part), opts)
        .into_iter()
        .map(|o| o.record)
        .collect()
}
}

internal_entry! {
/// The evidence path: each fillet with its defining face, published only if every one is
/// owned by a valid solid.
pub fn discover_verified(
    ctx: &Context<'_>,
    opts: &FilletOptions,
) -> Result<Vec<Occurrence<Fillet>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx, opts))
}
}

internal_entry! {
/// The leader-tip anchor: the surface point at the middle of the face's trimmed UV range.
pub fn fillet_anchor(part: &Part, face: usize) -> Option<V3> {
    let (u0, u1, v0, v1) = part.uv_bounds(face)?;
    Some(
        part.faces[face]
            .surface
            .value(0.5 * (u0 + u1), 0.5 * (v0 + v1)),
    )
}
}

fn occurrence(
    axis: FilletAxis,
    radius: f64,
    at: V3,
    turned: bool,
    face: usize,
    context: Vec<usize>,
) -> Occurrence<Fillet> {
    Occurrence {
        record: Fillet {
            axis,
            radius: py::round_to3(radius),
            at: Placed::new(at, Grid::Decimals(3)),
            turned,
            side: "convex",
        },
        defining: vec![face],
        context,
    }
}

/// The run axis of a principal-direction surface, or `None` for a compound (oblique) one.
fn principal_axis(direction: V3) -> Option<usize> {
    let comp = direction.map(f64::abs);
    (comp[0].max(comp[1]).max(comp[2]) > AXIS_ALIGNED_COS).then(|| dominant_axis(comp))
}

internal_entry! {
pub fn discover(ctx: &Context<'_>, opts: &FilletOptions) -> Vec<Occurrence<Fillet>> {
    let part = ctx.part;
    let min_radius = opts.min_radius.unwrap_or(MIN_RADIUS);
    let max_radius = opts.max_radius_frac * ctx.bounds().max_extent();
    let mut out = Vec::new();
    if opts.include_cylindrical {
        for face in 0..part.faces.len() {
            if let Some(found) = prismatic(ctx, face, min_radius, max_radius) {
                out.push(found);
            }
        }
    }
    let external = turned::external_cylinders(ctx);
    for face in 0..part.faces.len() {
        if let Some(found) = turned(part, face, min_radius, max_radius, &external) {
            out.push(found);
        }
    }
    attach_spherical_corners(part, &mut out);
    out.sort_by(|a, b| {
        let key = |o: &Occurrence<Fillet>| (o.record.axis, o.record.at);
        let (ka, kb) = (key(a), key(b));
        ka.0.order(kb.0).then_with(|| py::tuple_order(&ka.1[..], &kb.1[..]))
    });
    group_turned_arcs(part, out)
}
}

/// One interrupted edge round keeps a leader on its first actual arc. Compare the
/// unrounded analytic torus and meridian, never rounded radii or leader positions.
/// Equal minor radii on different rings/solids or opposite meridian sides stay separate.
fn same_turned_surface(part: &Part, a: &Occurrence<Fillet>, b: &Occurrence<Fillet>) -> bool {
    if !a.record.turned || !b.record.turned || a.record.side != b.record.side {
        return false;
    }
    let (fa, fb) = (a.defining[0], b.defining[0]);
    let Surface::Torus {
        frame: x,
        major: r,
        minor: s,
    } = part.faces[fa].surface
    else {
        return false;
    };
    let Surface::Torus {
        frame: y,
        major: t,
        minor: u,
    } = part.faces[fb].surface
    else {
        return false;
    };
    let tol = cylinder_support_tolerance(part);
    if (r - t).abs() > tol
        || (s - u).abs() > tol
        || geom::dist(x.origin, y.origin) > tol
        || geom::norm(geom::cross(x.z, y.z)) * (r + s) > tol
    {
        return false;
    }
    let meridian = |face| -> Option<[[f64; 2]; 3]> {
        let (u0, u1, v0, v1) = part.uv_bounds(face)?;
        let mut points = [v0, (v0 + v1) / 2., v1].map(|v| {
            let p = geom::sub(part.faces[face].surface.value((u0 + u1) / 2., v), x.origin);
            let z = geom::dot(p, x.z);
            [z, geom::norm(geom::sub(p, geom::scale(x.z, z)))]
        });
        points.sort_by(|a, b| py::tuple_order(a, b));
        Some(points)
    };
    let (Some(pa), Some(pb)) = (meridian(fa), meridian(fb)) else {
        return false;
    };
    if pa
        .iter()
        .zip(pb)
        .any(|(a, b)| a.iter().zip(b).any(|(a, b)| (a - b).abs() > tol))
    {
        return false;
    }
    let faces: Vec<_> = a.defining.iter().chain(&b.defining).copied().collect();
    evidence::common_valid_solid(part, &faces).is_some()
}

fn group_turned_arcs(part: &Part, found: Vec<Occurrence<Fillet>>) -> Vec<Occurrence<Fillet>> {
    let mut out: Vec<Occurrence<Fillet>> = Vec::new();
    for arc in found {
        if let Some(group) = out.iter_mut().find(|g| same_turned_surface(part, g, &arc)) {
            group.defining.extend(arc.defining);
            group.defining.sort_unstable();
            group.defining.dedup();
            group.context.extend(arc.context);
            group.context.sort_unstable();
            group.context.dedup();
        } else {
            out.push(arc);
        }
    }
    out
}

/// A partial cylinder rounding a convex or concave edge between two tangent walls, in any direction.
pub(super) fn prismatic(
    ctx: &Context<'_>,
    face: usize,
    min_radius: f64,
    max_radius: f64,
) -> Option<Occurrence<Fillet>> {
    let part = ctx.part;
    let Surface::Cylinder { frame, radius } = part.faces[face].surface else {
        return None;
    };
    let (u0, u1, _, _) = part.uv_bounds(face)?;
    if radius < min_radius || radius > max_radius {
        return None;
    }
    // Convexity belongs to the cylinder's oriented material side, independently of the
    // query axes. The corner probe alone cannot prove the surface's material side.
    let outward = part.frame_points_outward(face)?;
    let at = fillet_anchor(part, face)?;
    let corner =
        tangent_cylinder_corner(part, face, at, frame.z, frame.origin, radius).or_else(|| {
            (!outward)
                .then(|| curved_wall_corner(part, face, at, frame.z, frame.origin, radius))
                .flatten()
        })?;
    // The planes must be tangent to this cylinder, not unrelated nearby walls.
    if corner.normals.iter().any(|&n| {
        (geom::dot(geom::sub(frame.origin, corner.point), n).abs() - radius).abs()
            > cylinder_support_tolerance(part)
    }) || !sweep_matches_walls(u1 - u0, corner.normals)
    {
        return None;
    }
    let near = geom::add(
        corner.point,
        geom::scale(
            geom::sub(at, corner.point),
            super::policy::INTERIOR_PROBE_FRAC,
        ),
    );
    if material_at(ctx.classifier(), near) == outward {
        return None;
    }
    let axis = run_axis(frame.z)?;
    let mut found = occurrence(axis, radius, at, false, face, vec![]);
    found.record.side = if outward { "convex" } else { "concave" };
    Some(found)
}

/// A straight inside round can also spring from a parallel cylindrical wall (#127).
/// Use its tangent plane at the shared straight spring line. Both analytic cylinders
/// must contain that line at model precision and their oriented normals must agree.
/// Planar-wall rounds retain #120's existing support path unchanged.
pub(crate) fn curved_wall_corner(
    part: &Part,
    face: usize,
    at: V3,
    run: V3,
    origin: V3,
    radius: f64,
) -> Option<super::bevel::TransverseCorner> {
    let tol = cylinder_support_tolerance(part);
    let mut supports: Vec<(V3, V3)> = Vec::new();
    let mut curved = false;
    for n in part.neighbours(face) {
        let (point, normal) = match part.faces[n].surface {
            Surface::Plane { frame } => (frame.origin, super::planes::unit_plane_normal(part, n)?),
            Surface::Cylinder {
                frame,
                radius: wall_radius,
            } => {
                if geom::norm(geom::cross(frame.z, run)) > geom::SMOOTH_ARC_GAP
                    || part.arc(face, n) != Some(crate::kernel::brep::Arc::Smooth)
                {
                    continue;
                }
                let delta = geom::sub(frame.origin, origin);
                let transverse = geom::sub(delta, geom::scale(run, geom::dot(delta, run)));
                let direction = geom::unit(transverse)?;
                let station = geom::add(
                    origin,
                    geom::scale(run, geom::dot(geom::sub(at, origin), run)),
                );
                let shared = part.shared_edges(face, n);
                if shared.is_empty() {
                    return None;
                }
                let mut contact = None;
                for sign in [-1., 1.] {
                    let point = geom::add(station, geom::scale(direction, sign * radius));
                    if (geom::norm(geom::cross(geom::sub(point, frame.origin), frame.z))
                        - wall_radius)
                        .abs()
                        > tol
                    {
                        continue;
                    }
                    let (u, v) = part.faces[n].surface.parameters(point, None)?;
                    let normal = part.face_normal(n, u, v)?;
                    let (u, v) = part.faces[face].surface.parameters(point, None)?;
                    if geom::norm(geom::sub(normal, part.face_normal(face, u, v)?))
                        > geom::SMOOTH_ARC_GAP
                    {
                        continue;
                    }
                    // Native axes prove parallelism. STEP spring lines may be slightly
                    // tilted within modelling precision: test their whole trimmed length
                    // against the analytic tangent generator, without an angular allowance.
                    if shared.iter().any(|&e| {
                        !matches!(part.edges[e].curve, crate::kernel::geom::Curve::Line { .. })
                            || [part.edges[e].start, part.edges[e].end]
                                .iter()
                                .any(|&p| geom::norm(geom::cross(geom::sub(p, point), run)) > tol)
                    }) {
                        continue;
                    }
                    if contact.is_some() {
                        return None;
                    }
                    contact = Some((point, normal));
                }
                curved = true;
                contact?
            }
            _ => continue,
        };
        if geom::dot(normal, run).abs() > geom::SMOOTH_ARC_GAP
            || (geom::dot(geom::sub(origin, point), normal).abs() - radius).abs() > tol
        {
            continue;
        }
        if let Some((p, n)) = supports
            .iter()
            .find(|(_, n)| geom::dot(*n, normal).abs() >= 1. - geom::SMOOTH_ARC_GAP)
        {
            // Keep #120's ambiguity guard: model precision admits contact, not
            // distinct competing parallel walls.
            if geom::dot(geom::sub(*p, point), *n).abs() > geom::COORD_FLOOR {
                return None;
            }
        } else {
            supports.push((point, normal));
        }
    }
    let [(pa, a), (pb, b)] = supports.as_slice() else {
        return None;
    };
    if !curved {
        return None;
    }
    let d = geom::dot(*a, *b);
    let da = geom::dot(geom::sub(*pa, at), *a);
    let db = geom::dot(geom::sub(*pb, at), *b);
    Some(super::bevel::TransverseCorner {
        point: geom::add(
            at,
            geom::add(
                geom::scale(*a, (da - d * db) / (1. - d * d)),
                geom::scale(*b, (db - d * da) / (1. - d * d)),
            ),
        ),
        normals: [*a, *b],
        areas: [0.; 2],
    })
}

/// A spherical corner has no straight/circular rolling path of its own. Keep the existing
/// fillet record and include the cap in one of the three rounds it closes: straight runs
/// or a toroidal continuation (#127). The lowest defining cylinder index owns it;
/// the other rounds are consulted evidence.
/// This is geometric incidence, independent of the query frame or rounded record values.
fn attach_spherical_corners(part: &Part, found: &mut [Occurrence<Fillet>]) {
    let cylinders: BTreeMap<usize, usize> = found
        .iter()
        .enumerate()
        .filter(|(_, o)| !o.record.turned && o.record.side == "concave")
        .map(|(i, o)| (o.defining[0], i))
        .collect();
    for face in 0..part.faces.len() {
        let Some(rounds) = spherical_corner(part, face, &cylinders) else {
            continue;
        };
        let owner = cylinders[&rounds[0]];
        found[owner].defining.push(face);
        found[owner].defining.sort_unstable();
        for round in part
            .neighbours(face)
            .into_iter()
            .filter(|&n| n != rounds[0])
        {
            if !found[owner].context.contains(&round) {
                found[owner].context.push(round);
            }
        }
    }
}

fn spherical_corner(
    part: &Part,
    face: usize,
    cylinders: &BTreeMap<usize, usize>,
) -> Option<Vec<usize>> {
    let Surface::Sphere { frame, radius } = part.faces[face].surface else {
        return None;
    };
    if part.frame_points_outward(face) != Some(false) {
        return None;
    }
    let mut neighbours = part.neighbours(face);
    neighbours.sort_unstable();
    let rounds: [usize; 3] = neighbours.try_into().ok()?;
    let tol = cylinder_support_tolerance(part);
    let mut axes = BTreeMap::new();
    let mut straight = Vec::new();
    for &n in &rounds {
        let run = match part.faces[n].surface {
            Surface::Cylinder {
                frame: run,
                radius: r,
            } => {
                cylinders.get(&n)?;
                if (radius - r).abs() > tol
                    || geom::norm(geom::cross(geom::sub(frame.origin, run.origin), run.z)) > tol
                {
                    return None;
                }
                straight.push(n);
                run.z
            }
            Surface::Torus {
                frame: ring,
                major,
                minor,
            } => {
                let relative = geom::sub(frame.origin, ring.origin);
                if (radius - minor).abs() > tol
                    || geom::dot(relative, ring.z).abs() > tol
                    || (geom::norm(relative) - major).abs() > tol
                {
                    return None;
                }
                geom::unit(geom::cross(ring.z, relative))?
            }
            _ => return None,
        };
        if part.arc(face, n) != Some(crate::kernel::brep::Arc::Smooth) {
            return None;
        }
        axes.insert(n, run);
    }
    if straight.len() < 2 {
        return None;
    }
    // Three independent runs close a corner; a split single run or a band does not.
    if geom::dot(
        axes[&rounds[0]],
        geom::cross(axes[&rounds[1]], axes[&rounds[2]]),
    )
    .abs()
        <= crate::kernel::geom::SMOOTH_ARC_GAP
    {
        return None;
    }
    let edges = part.face_edges(face);
    if edges.is_empty() {
        return None;
    }
    for edge in edges {
        let owners = &part.edge_faces()[edge];
        if owners.len() != 2 {
            return None;
        }
        let n = *owners.iter().find(|&&n| n != face)?;
        if !rounds.contains(&n) {
            return None;
        }
        let run = axes[&n];
        let crate::kernel::geom::Curve::Circle {
            frame: arc,
            radius: r,
        } = part.edges[edge].curve
        else {
            return None;
        };
        if (r - radius).abs() > tol
            || geom::norm(geom::sub(arc.origin, frame.origin)) > tol
            || geom::norm(geom::cross(arc.z, run)) > crate::kernel::geom::SMOOTH_ARC_GAP
        {
            return None;
        }
    }
    Some(straight)
}

/// Preserve the legacy principal-direction representation, including its alignment band.
fn run_axis(direction: V3) -> Option<FilletAxis> {
    Some(if let Some(axis) = principal_axis(direction) {
        FilletAxis::Principal(['x', 'y', 'z'][axis])
    } else {
        let direction = geom::unit(direction)?;
        let sign = if direction[dominant_axis(direction.map(f64::abs))] < 0.0 {
            -1.0
        } else {
            1.0
        };
        FilletAxis::Direction(geom::scale(direction, sign))
    })
}

/// The outward wall normals turn through the swept arc: PI minus the interior wall
/// angle. The sweep tolerance accommodates modelled geometry, not a truncated bend skin.
/// The 135° cap separates edge rounds from full-round groove/slot bottoms.
fn sweep_matches_walls(sweep: f64, normals: [V3; 2]) -> bool {
    if !(sweep > 0.0 && sweep <= FILLET_MAX_EXTENT) {
        return false;
    }
    let expected =
        geom::norm(geom::cross(normals[0], normals[1])).atan2(geom::dot(normals[0], normals[1]));
    (sweep - expected).abs() <= FILLET_SWEEP_TOL_DEG.to_radians()
}

/// A short external torus: a lathe-swept edge round, proved by a coaxial external cylinder.
fn turned(
    part: &Part,
    face: usize,
    min_radius: f64,
    max_radius: f64,
    external: &BTreeMap<usize, &CylinderEvidence>,
) -> Option<Occurrence<Fillet>> {
    let Surface::Torus { frame, minor, .. } = part.faces[face].surface else {
        return None;
    };
    if minor < min_radius || minor > max_radius {
        return None;
    }
    let (_, _, v0, v1) = part.uv_bounds(face)?;
    if (v1 - v0).abs() > TURNED_FILLET_MAX_EXTENT {
        return None;
    }
    let edge_axis = principal_axis(frame.z)?;
    let context = turned_context(part, face, &part.neighbours(face), external)?;
    let at = fillet_anchor(part, face)?;
    let mut found = occurrence(
        FilletAxis::Principal(['x', 'y', 'z'][edge_axis]),
        minor,
        at,
        true,
        face,
        context,
    );
    found.record.side = torus_side(part, face)?;
    Some(found)
}

/// Material is inside the tube for a convex torus, outside it for a concave root
/// round. Tangency alone cannot distinguish them. Require agreement at three samples.
pub(super) fn torus_side(part: &Part, face: usize) -> Option<&'static str> {
    let Surface::Torus {
        frame,
        major,
        minor,
    } = part.faces[face].surface
    else {
        return None;
    };
    let (u0, u1, v0, v1) = part.uv_bounds(face)?;
    let mut answer = None;
    for t in [0.25, 0.5, 0.75] {
        let (u, v) = (u0 + t * (u1 - u0), v0 + t * (v1 - v0));
        let p = part.faces[face].surface.value(u, v);
        let delta = geom::sub(p, frame.origin);
        let radial = geom::unit(geom::sub(
            delta,
            geom::scale(frame.z, geom::dot(delta, frame.z)),
        ))?;
        let centre = geom::add(frame.origin, geom::scale(radial, major));
        let dot = geom::dot(part.face_normal(face, u, v)?, geom::sub(p, centre));
        if !dot.is_finite() || dot.abs() <= super::policy::length_tol(minor, 1e-9) {
            return None;
        }
        let side = if dot > 0. { "convex" } else { "concave" };
        if answer.is_some_and(|s| s != side) {
            return None;
        }
        answer = Some(side);
    }
    answer
}

internal_entry! {
/// The material-side proof for a toroidal blend: the coaxial external cylinders it meets, plus
/// the transverse planes or spherical caps that make it a rounded edge rather than a bead.
/// `None` when the torus is not a turned edge fillet.
pub fn turned_context(
    part: &Part,
    torus: usize,
    neighbours: &[usize],
    external: &BTreeMap<usize, &CylinderEvidence>,
) -> Option<Vec<usize>> {
    let Surface::Torus { frame, .. } = part.faces[torus].surface else {
        return None;
    };
    let coaxial = turned::coaxial_cylinders(neighbours, external, frame.origin, frame.z);
    if coaxial.is_empty() {
        return None;
    }
    let transverse = turned::transverse_planes(part, neighbours, frame.z);
    // A compound turned fillet can continue into its spherical corner cap.
    let continuations: Vec<usize> = neighbours
        .iter()
        .copied()
        .filter(|&n| matches!(part.faces[n].surface, Surface::Sphere { .. }))
        .collect();
    if transverse.is_empty() && !turned::bridges_two_bands(&coaxial) && continuations.is_empty() {
        return None; // a toroidal bead meeting one band, not a rounded edge
    }
    let mut context: Vec<usize> = Vec::new();
    for f in coaxial
        .iter()
        .map(|c| c.face)
        .chain(transverse)
        .chain(continuations)
    {
        if !context.contains(&f) {
            context.push(f);
        }
    }
    Some(context)
}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn torus_grouping_refuses_other_rings_radii_sides_and_positions() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/interrupted_fillet_hub.step");
        let part = crate::read_step_file(&path).unwrap();
        let ctx = Context::new(&part);
        let external = turned::external_cylinders(&ctx);
        let a = turned(&part, 5, MIN_RADIUS, 100., &external).unwrap();
        let b = turned(&part, 9, MIN_RADIUS, 100., &external).unwrap();
        assert!(same_turned_surface(&part, &a, &b));
        let mut reversed = b.clone();
        reversed.record.side = "convex";
        assert!(!same_turned_surface(&part, &a, &reversed));
        for perturb in 0..5 {
            let mut faces = part.faces.clone();
            let Surface::Torus {
                ref mut frame,
                ref mut major,
                ref mut minor,
            } = faces[9].surface
            else {
                panic!()
            };
            match perturb {
                0 => *major += 0.1,
                1 => *minor += 0.1,
                2 => frame.origin[2] += 0.1,
                3 => frame.origin[0] += 0.1,
                _ => frame.z = geom::unit([0.01, 0., 1.]).unwrap(),
            }
            let changed = Part::new(faces, part.edges.clone(), part.solids.clone());
            assert!(!same_turned_surface(&changed, &a, &b), "perturb {perturb}");
        }
    }

    #[test]
    fn principal_runs_keep_the_legacy_axis_letter_throughout_the_alignment_band() {
        for degrees in [0_f64, 2.94, 5., 8.] {
            let (s, c) = degrees.to_radians().sin_cos();
            for (axis, direction) in [('x', [c, s, 0.]), ('y', [0., c, s]), ('z', [s, 0., c])] {
                assert_eq!(run_axis(direction), Some(FilletAxis::Principal(axis)));
                assert_eq!(
                    run_axis(direction.map(|d| -d)),
                    Some(FilletAxis::Principal(axis))
                );
                assert_eq!(
                    serde_json::to_value(run_axis(direction).unwrap()).unwrap(),
                    axis.to_string()
                );
            }
        }
        let (s, c) = 9_f64.to_radians().sin_cos();
        assert!(matches!(
            run_axis([c, s, 0.]),
            Some(FilletAxis::Direction(_))
        ));
    }

    #[test]
    fn sweep_follows_acute_right_and_obtuse_walls() {
        let tolerance = FILLET_SWEEP_TOL_DEG.to_radians();
        for wall_degrees in [60_f64, 86., 90., 94., 120., 160.] {
            let sweep = PI - wall_degrees.to_radians();
            let walls = [[1., 0., 0.], [sweep.cos(), sweep.sin(), 0.]];
            assert!(sweep_matches_walls(sweep, walls));
            assert!(sweep_matches_walls(sweep + 0.5 * tolerance, walls));
            assert!(!sweep_matches_walls(sweep + 2.0 * tolerance, walls));
            assert!(!sweep_matches_walls(sweep - 2.0 * tolerance, walls));
            if wall_degrees != 90. {
                assert!(!sweep_matches_walls(PI - sweep, walls));
            }
            for invalid in [0., -sweep, PI, 2. * PI, f64::NAN] {
                assert!(!sweep_matches_walls(invalid, walls));
            }
        }
    }

    #[test]
    fn modelled_near_quarter_rounds_pass_but_truncated_bend_skins_do_not() {
        let walls = [[1., 0., 0.], [0., 1., 0.]];
        // CTC-03 face 36 and sm-hanger faces 73/76, expressed as constructed sections.
        assert!(sweep_matches_walls(
            90.02457403831906_f64.to_radians(),
            walls
        ));
        assert!(!sweep_matches_walls(
            25.376933525173_f64.to_radians(),
            walls
        ));
        assert!(sweep_matches_walls(90.499_f64.to_radians(), walls));
        assert!(!sweep_matches_walls(90.501_f64.to_radians(), walls));
        assert!(sweep_matches_walls(89.501_f64.to_radians(), walls));
        assert!(!sweep_matches_walls(89.499_f64.to_radians(), walls));
    }

    #[test]
    fn edge_rounds_stop_at_135_degrees() {
        for degrees in [45_f64, 40., 5.879068972873341] {
            let sweep = PI - degrees.to_radians();
            let walls = [[1., 0., 0.], [sweep.cos(), sweep.sin(), 0.]];
            assert_eq!(sweep_matches_walls(sweep, walls), degrees >= 45.);
        }
    }

    #[test]
    fn drafted_round_still_requires_tangent_walls_and_convex_material() {
        let part = crate::read_step_file(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/cast_bearing_unit.step.gz"),
        )
        .unwrap();
        let face = 24;
        let Surface::Cylinder { frame, radius } = part.faces[face].surface else {
            panic!()
        };
        let accepts = |p: &Part| prismatic(&Context::new(p), face, MIN_RADIUS, 100.).is_some();
        assert!(accepts(&part));
        // Perturb measured evidence in fresh Parts so caches cannot mask a failed proof.
        let mut faces = part.faces.clone();
        faces[face].surface = Surface::Cylinder {
            frame,
            radius: radius + 0.1,
        };
        assert!(!accepts(&Part::new(
            faces,
            part.edges.clone(),
            part.solids.clone()
        )));
        let mut faces = part.faces.clone();
        faces[face].reversed = !faces[face].reversed;
        assert!(!accepts(&Part::new(
            faces,
            part.edges.clone(),
            part.solids.clone()
        )));
        let mut faces = part.faces.clone();
        for n in part.neighbours(face) {
            if matches!(faces[n].surface, Surface::Plane { .. }) {
                faces[n].surface = Surface::Other {
                    kind: "missing support",
                };
            }
        }
        assert!(!accepts(&Part::new(
            faces,
            part.edges.clone(),
            part.solids.clone()
        )));
        assert!(prismatic(&Context::new(&part), face, radius + 0.1, 100.).is_none());
        assert!(prismatic(&Context::new(&part), face, MIN_RADIUS, radius - 0.1).is_none());
        let mut over_limit = 0;
        for f in 0..part.faces.len() {
            if matches!(part.faces[f].surface, Surface::Cylinder { .. })
                && let Some((u0, u1, _, _)) = part.uv_bounds(f)
                && u1 - u0 >= PI
            {
                over_limit += 1;
                assert!(prismatic(&Context::new(&part), f, MIN_RADIUS, 100.).is_none());
            }
        }
        assert!(
            over_limit > 0,
            "fixture must exercise the bore/extent guard"
        );
    }
}
