//! Connected, tangent runs and radius groups of accepted fillet faces.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::bevel::cylinder_support_tolerance;
use super::fillets::Fillet;
use crate::kernel::brep::{Edge, Part};
use crate::kernel::geom::{
    self, Curve, Frame, Surface, V3, cross, dist, dot, norm, scale, sub, unit,
};
use crate::kernel::sampling::edge_interval;

/// A connected run of equal-radius, same-side fillet faces.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct FilletChain {
    /// Smallest unrounded surface radius in the chain, in millimetres.
    pub radius: f64,
    pub side: &'static str,
    pub faces: Vec<usize>,
    /// Spring edges where the chain meets its tangent walls, in part edge order.
    pub edges: Vec<usize>,
    /// Indices into the legacy fillets array. A record can span multiple chains.
    pub fillets: Vec<usize>,
    /// Sorted chain faces with no legacy fillet record.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blend_faces: Vec<usize>,
}

/// Chains of equal radius and side, irrespective of their location.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct FilletGroup {
    /// Smallest unrounded surface radius in the group, in millimetres.
    pub radius: f64,
    pub side: &'static str,
    pub chains: Vec<usize>,
    pub faces: Vec<usize>,
    pub face_count: usize,
    pub chain_count: usize,
    /// Number of distinct legacy fillet records represented by the member chains.
    pub fillet_count: usize,
}

#[derive(Clone, Copy)]
pub(super) struct Radii {
    min: f64,
    max: f64,
}

impl Radii {
    fn combined(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    fn fits(self, tolerance: f64) -> bool {
        self.max - self.min <= tolerance
    }

    fn order(self, other: Self) -> Ordering {
        self.max
            .total_cmp(&other.max)
            .then(self.min.total_cmp(&other.min))
    }
}

/// The raw rolling-ball radius of each reported analytic face.
fn radius(surface: &Surface) -> Result<f64, &'static str> {
    match *surface {
        Surface::Cylinder { radius, .. } | Surface::Sphere { radius, .. } => Ok(radius),
        Surface::Torus { minor, .. } => Ok(minor),
        _ => Err("has no analytic rolling-ball radius"),
    }
}

fn material_side(part: &Part, face: usize) -> Option<&'static str> {
    match part.faces[face].surface {
        Surface::Cylinder { .. } | Surface::Sphere { .. } => part
            .frame_points_outward(face)
            .map(|outward| if outward { "convex" } else { "concave" }),
        Surface::Torus { .. } => super::fillets::torus_side(part, face),
        _ => None,
    }
}

struct Patch {
    face: usize,
    fillet: Option<usize>,
    side: &'static str,
    root: usize,
    radii: Radii,
}

fn root(patches: &mut [Patch], mut i: usize) -> usize {
    while patches[i].root != i {
        patches[i].root = patches[patches[i].root].root;
        i = patches[i].root;
    }
    i
}

/// Keep the full radius span within uncertainty, including transitive joins.
fn merge(patches: &mut [Patch], [i, j]: [usize; 2], tolerance: f64) {
    let a = root(patches, i);
    let b = root(patches, j);
    let span = patches[a].radii.combined(patches[b].radii);
    if span.fits(tolerance) {
        patches[b].root = a;
        patches[a].radii = span;
    }
}

fn derive(part: &Part, fillets: &[Fillet]) -> Result<Vec<(FilletChain, Radii)>, String> {
    if fillets.is_empty() {
        return Ok(Vec::new());
    }
    let contact = cylinder_support_tolerance(part);
    let tolerance = radius_tolerance(part);
    let mut patches = Vec::new();
    let mut by_face = vec![None; part.faces.len()];
    for (fillet, record) in fillets.iter().enumerate() {
        for &face in &record.faces {
            let i = patches.len();
            let r = radius(&part.faces[face].surface)
                .map_err(|why| format!("fillet face {face} {why}"))?;
            by_face[face] = Some(i);
            patches.push(Patch {
                face,
                fillet: Some(fillet),
                side: record.side,
                root: i,
                radii: Radii { min: r, max: r },
            });
        }
    }

    // Unowned cylinders may belong to holes or bosses. Only spheres and tori
    // have an eligible rolling-ball radius here. Planes and freeform surfaces
    // cannot supply that radius and the normal proof used by this pass.
    for (face, slot) in by_face.iter_mut().enumerate() {
        let surface = &part.faces[face].surface;
        if slot.is_some()
            || !matches!(surface, Surface::Sphere { .. } | Surface::Torus { .. })
            || part.face_moments(face).is_none()
        {
            continue;
        }
        let (Ok(r), Some(side)) = (radius(surface), material_side(part, face)) else {
            continue;
        };
        let i = patches.len();
        *slot = Some(i);
        patches.push(Patch {
            face,
            fillet: None,
            side,
            root: i,
            radii: Radii { min: r, max: r },
        });
    }

    let mut joins = Vec::new();
    for (edge, incident) in part.edge_faces().iter().enumerate() {
        let &[a, b] = incident.as_slice() else {
            continue;
        };
        let (Some(i), Some(j)) = (by_face[a], by_face[b]) else {
            continue;
        };
        let span = patches[i].radii.combined(patches[j].radii);
        if span.fits(tolerance)
            && patches[i].side == patches[j].side
            && tangent(part, a, b, &part.edges[edge], contact)
        {
            joins.push((span, [i, j]));
        }
    }

    // Consider lower radius spans first. Ties have the same radius interval: two
    // admissible joins with that interval cannot make one another inadmissible.
    // The constrained partition thus does not depend on STEP edge or face numbering.
    joins.sort_unstable_by(|(a, _), (b, _)| a.order(*b));
    for (_, pair) in joins {
        merge(&mut patches, pair, tolerance);
    }

    // A component is reportable only if reachable from a legacy fillet face.
    let seeds: Vec<_> = (0..patches.len())
        .filter(|&i| patches[i].fillet.is_some())
        .collect();
    let seeded: BTreeSet<_> = seeds.into_iter().map(|i| root(&mut patches, i)).collect();
    let mut components = BTreeMap::new();
    for i in 0..patches.len() {
        let r = root(&mut patches, i);
        let patch = &patches[i];
        if !seeded.contains(&r) {
            by_face[patch.face] = None;
            continue;
        }
        let radii = patches[r].radii;
        let (chain, _) = components.entry(r).or_insert_with(|| {
            (
                FilletChain {
                    radius: radii.min,
                    side: patch.side,
                    faces: Vec::new(),
                    edges: Vec::new(),
                    fillets: Vec::new(),
                    blend_faces: Vec::new(),
                },
                radii,
            )
        });
        chain.faces.push(patch.face);
        if let Some(fillet) = patch.fillet {
            chain.fillets.push(fillet);
        } else {
            chain.blend_faces.push(patch.face);
        }
        by_face[patch.face] = Some(r);
    }
    for (edge, incident) in part.edge_faces().iter().enumerate() {
        let &[a, b] = incident.as_slice() else {
            continue;
        };
        let (chain, blend, other) = match (by_face[a], by_face[b]) {
            (Some(chain), None) => (chain, a, b),
            (None, Some(chain)) => (chain, b, a),
            _ => continue,
        };
        let (record, _) = components.get_mut(&chain).unwrap();
        let r = radius(&part.faces[blend].surface)
            .map_err(|why| format!("fillet face {blend} {why}"))?;
        // An equal-radius, same-side analytic blend is never a support wall,
        // including an unowned cylinder or a patch outside this published region.
        let same_blend = radius(&part.faces[other].surface)
            .is_ok_and(|other| (other - r).abs() <= tolerance)
            && material_side(part, other) == Some(record.side);
        if !same_blend && sample_contact(part, a, b, &part.edges[edge], contact) {
            record.edges.push(edge);
        }
    }
    let mut chains: Vec<_> = components.into_values().collect();
    for (chain, _) in &mut chains {
        chain.faces.sort_unstable();
        chain.blend_faces.sort_unstable();
        chain.fillets.sort_unstable();
        chain.fillets.dedup();
    }
    chains.sort_by(|(a, _), (b, _)| {
        b.faces
            .len()
            .cmp(&a.faces.len())
            .then(a.radius.total_cmp(&b.radius))
            .then(a.side.cmp(b.side))
            .then(a.faces[0].cmp(&b.faces[0]))
    });
    Ok(chains)
}

fn radius_tolerance(part: &Part) -> f64 {
    // A declared uncertainty is already in millimetres. Apply the kernel coordinate floor
    // as a fallback when the input has no uncertainty, as for the wall contact tolerance.
    part.file_uncertainty().unwrap_or(geom::COORD_FLOOR)
}

pub(super) fn group(part: &Part, chains: &[(FilletChain, Radii)]) -> Vec<FilletGroup> {
    let tolerance = radius_tolerance(part);
    let mut order: Vec<_> = (0..chains.len()).collect();
    order.sort_by(|&a, &b| chains[a].1.min.total_cmp(&chains[b].1.min).then(a.cmp(&b)));
    let mut active = BTreeMap::new();
    let mut groups: Vec<FilletGroup> = Vec::new();
    for i in order {
        let (chain, radii) = &chains[i];
        let current = active.entry(chain.side).or_insert_with(|| {
            groups.push(empty_group(chain));
            groups.len() - 1
        });
        if radii.max - groups[*current].radius > tolerance {
            groups.push(empty_group(chain));
            *current = groups.len() - 1;
        }
        let group = &mut groups[*current];
        group.chains.push(i);
        group.faces.extend(&chain.faces);
    }
    for group in &mut groups {
        group.chains.sort_unstable();
        group.faces.sort_unstable();
        group.chain_count = group.chains.len();
        group.face_count = group.faces.len();
        group.fillet_count = group
            .chains
            .iter()
            .flat_map(|&i| chains[i].0.fillets.iter().copied())
            .collect::<BTreeSet<_>>()
            .len();
    }
    groups.sort_by(|a, b| {
        b.face_count
            .cmp(&a.face_count)
            .then(a.radius.total_cmp(&b.radius))
            .then(a.side.cmp(b.side))
            .then(a.faces[0].cmp(&b.faces[0]))
    });
    groups
}

fn empty_group(chain: &FilletChain) -> FilletGroup {
    FilletGroup {
        radius: chain.radius,
        side: chain.side,
        chains: Vec::new(),
        faces: Vec::new(),
        chain_count: 0,
        face_count: 0,
        fillet_count: 0,
    }
}

// The kernel's smooth-arc gap is 1 - cos(theta). Its chord equivalent compares
// unit-normal distances directly, limiting the first-order cross-edge slope to
// about 45 microradians for a smooth join, independently of the length tolerance.
fn normal_gap(part: &Part) -> f64 {
    part.file_angle_uncertainty().map_or_else(
        || (2.0 * geom::SMOOTH_ARC_GAP).sqrt(),
        // Unit normals are at most pi radians apart. Convert angle to chord.
        |radians| 2.0 * (radians.min(std::f64::consts::PI) / 2.0).sin(),
    )
}

fn sample_contact(part: &Part, a: usize, b: usize, edge: &Edge, tol: f64) -> bool {
    let (lo, hi) = edge_interval(
        &edge.curve,
        edge.start,
        edge.end,
        edge.same_sense,
        edge.is_closed(),
    );
    if !lo.is_finite() || !hi.is_finite() || lo == hi {
        return false;
    }
    // Include the interior of straight edges, whose kernel samples are only their vertices.
    let mid = edge.curve.value(f64::midpoint(lo, hi));
    let at = |face: usize, point: V3| {
        let surface = &part.faces[face].surface;
        let (u, v) = surface.parameters(point, None)?;
        (dist(surface.value(u, v), point) <= tol)
            .then(|| part.face_normal(face, u, v))
            .flatten()
    };
    let gap = normal_gap(part);
    edge.samples
        .iter()
        .chain([&mid])
        .all(|&p| matches!((at(a, p), at(b, p)), (Some(x), Some(y)) if dist(x, y) <= gap))
}

/// A circle's ideal normal has a fixed term plus a radial term rotating in its plane.
/// The error bounds its difference from the true unit normal over the whole circle.
struct NormalField {
    fixed: V3,
    radial: f64,
    error: f64,
}

impl NormalField {
    fn new(
        fixed: V3,
        radial: f64,
        deviation: f64,
        reversed: bool,
        tol: f64,
        radius: f64,
    ) -> Option<Self> {
        let magnitude = norm(fixed).hypot(radial);
        if deviation + (magnitude - radius).abs() > tol || magnitude <= deviation {
            return None;
        }
        let orientation = if reversed { -1. } else { 1. };
        Some(Self {
            fixed: scale(fixed, orientation / magnitude),
            radial: orientation * radial / magnitude,
            // Normalizing two vectors changes their difference by at most twice the
            // relative difference with respect to the reference vector's length.
            error: 2.0 * deviation / magnitude,
        })
    }
}

fn circle_field(part: &Part, f: usize, circle: Frame, r: f64, tol: f64) -> Option<NormalField> {
    let face = &part.faces[f];
    let (fixed, radial, deviation) = match face.surface {
        Surface::Cylinder { frame, .. } => {
            let d = sub(circle.origin, frame.origin);
            let deviation = norm(cross(d, frame.z)) + r * norm(cross(frame.z, circle.z));
            ([0.; 3], r, deviation)
        }
        Surface::Sphere { frame, .. } => {
            let d = sub(circle.origin, frame.origin);
            (
                scale(circle.z, dot(d, circle.z)),
                r,
                norm(cross(d, circle.z)),
            )
        }
        Surface::Torus {
            frame,
            major,
            minor,
        } if major > minor => {
            let d = sub(circle.origin, frame.origin);
            let height = dot(d, frame.z);
            let transverse = sub(d, scale(frame.z, height));
            let reach = norm(transverse);
            let axis = if dot(frame.z, circle.z) >= 0. {
                circle.z
            } else {
                scale(circle.z, -1.)
            };
            let tilt = norm(cross(frame.z, circle.z));
            let coaxial = reach + r * tilt;
            if coaxial <= tol {
                // A parallel circle has the same horizontal azimuth as its spine.
                let deviation =
                    reach + height.abs() * dist(axis, frame.z) + 2.0 * major * coaxial / r;
                (scale(axis, height), r - major, deviation)
            } else {
                // On a meridian the spine centre is fixed. Bound the azimuth error from
                // the maximum excursion perpendicular to the radial plane, divided by
                // the minimum positive radial distance to the torus axis.
                let radial = unit(transverse)?;
                let circumferential = cross(frame.z, radial);
                let lower = reach - r;
                if lower <= 0. {
                    return None;
                }
                let excursion = r * norm(cross(circumferential, circle.z));
                let deviation = dist(d, scale(radial, major)) + 2.0 * major * excursion / lower;
                ([0.; 3], r, deviation)
            }
        }
        _ => return None,
    };
    NormalField::new(
        fixed,
        radial,
        deviation,
        face.reversed,
        tol,
        radius(&face.surface).ok()?,
    )
}

fn line_field(part: &Part, f: usize, edge: &Edge, tol: f64) -> Option<NormalField> {
    let face = &part.faces[f];
    let Surface::Cylinder { frame, radius } = face.surface else {
        return None;
    };
    let radial = |point| {
        let d = sub(point, frame.origin);
        sub(d, scale(frame.z, dot(d, frame.z)))
    };
    let exact = |point| radial(edge.curve.value(edge.curve.parameter(point)));
    let (a, b) = (exact(edge.start), exact(edge.end));
    // The radial vector varies affinely along a line. Its largest possible deviation
    // from the first endpoint is attained at the other endpoint.
    NormalField::new(a, 0.0, dist(a, b), face.reversed, tol, radius)
}

fn tangent(part: &Part, a: usize, b: usize, edge: &Edge, tol: f64) -> bool {
    let fields = match edge.curve {
        Curve::Circle { frame, radius } if radius > 0. => (
            circle_field(part, a, frame, radius, tol),
            circle_field(part, b, frame, radius, tol),
        ),
        Curve::Line { .. } => (
            line_field(part, a, edge, tol),
            line_field(part, b, edge, tol),
        ),
        _ => return false,
    };
    matches!(fields, (Some(x), Some(y))
        if dist(x.fixed, y.fixed) + (x.radial - y.radial).abs() + x.error + y.error <= normal_gap(part))
        && sample_contact(part, a, b, edge, tol)
}

/// Publish after the final face claims, in particular after pockets take any spherical
/// floor corners. The derived families read claims without creating new physical owners.
pub(super) fn publish(part: &Part, features: &mut super::Features) {
    use super::evidence::EvidenceStatus;
    use super::isolation::guarded;

    for (fillet, faces) in features
        .fillets
        .iter_mut()
        .zip(features.defining.get("fillets").into_iter().flatten())
    {
        fillet.faces.clone_from(faces);
    }
    let chains = match guarded(&mut features.failures, "fillet_chains", "discover", || {
        Some(derive(part, &features.fillets))
    }) {
        Some(Ok(chains)) => chains,
        Some(Err(message)) => {
            features.failures.push(super::FamilyFailure {
                family: "fillet_chains".into(),
                stage: "discover".into(),
                message: Some(message),
                blocked_by: Vec::new(),
            });
            Vec::new()
        }
        None => Vec::new(),
    };
    let groups = guarded(&mut features.failures, "fillet_groups", "discover", || {
        group(part, &chains)
    });
    features
        .failures
        .sort_by(|a, b| (&a.family, &a.stage).cmp(&(&b.family, &b.stage)));
    if chains.is_empty() {
        return;
    }

    let inherited = |base: &[EvidenceStatus], members: &[usize]| {
        if members.iter().all(|&i| base[i] == EvidenceStatus::Verified) {
            EvidenceStatus::Verified
        } else {
            EvidenceStatus::Unproved
        }
    };
    let fillet_status = &features.evidence_status["fillets"];
    let mut chain_status = Vec::new();
    for (i, (chain, _)) in chains.iter().enumerate() {
        for &f in &chain.fillets {
            features.fillets[f].chains.push(i);
        }
        let status = inherited(fillet_status, &chain.fillets);
        chain_status.push(
            if chain.blend_faces.is_empty() || status != EvidenceStatus::Verified {
                status
            } else {
                super::evidence::strict_status(part, &chain.faces, &[])
            },
        );
    }
    let group_status = groups
        .iter()
        .map(|g| inherited(&chain_status, &g.chains))
        .collect();
    features.fillet_chains = chains.into_iter().map(|(chain, _)| chain).collect();
    features.fillet_groups = groups;
    features.defining.insert(
        "fillet_chains",
        features
            .fillet_chains
            .iter()
            .map(|c| c.faces.clone())
            .collect(),
    );
    features.defining.insert(
        "fillet_groups",
        features
            .fillet_groups
            .iter()
            .map(|g| g.faces.clone())
            .collect(),
    );
    features
        .evidence_status
        .insert("fillet_chains", chain_status);
    features
        .evidence_status
        .insert("fillet_groups", group_status);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::fillets::FilletAxis;
    use crate::features::placed::{Grid, Placed};
    use crate::kernel::brep::{Face, Loop};
    use crate::kernel::nurbs::NurbsCurve;

    const FRAME: Frame = Frame {
        origin: [0., 0., 0.],
        x: [1., 0., 0.],
        y: [0., 1., 0.],
        z: [0., 0., 1.],
    };

    // Only the shared edges matter to this proof. Feed the analytic patches directly
    // to the chain pass, whose production input comes from legacy fillet recognition.
    fn patches(radii: &[f64], edges: Vec<([usize; 2], Edge)>) -> (Part, Vec<Fillet>) {
        let (mut faces, mut fillets) = (Vec::new(), Vec::new());
        for (i, &radius) in radii.iter().enumerate() {
            faces.push(Face {
                surface: Surface::Cylinder {
                    frame: FRAME,
                    radius,
                },
                reversed: false,
                loops: vec![Loop {
                    edges: edges
                        .iter()
                        .enumerate()
                        .filter_map(|(e, (pair, _))| pair.contains(&i).then_some((e, pair[0] == i)))
                        .collect(),
                    vertex: None,
                }],
                solid: None,
                pcurves: Vec::new(),
            });
            fillets.push(Fillet {
                axis: FilletAxis::Principal('z'),
                radius,
                side: "convex",
                turned: false,
                at: Placed::new(FRAME.origin, Grid::Exact),
                faces: vec![i],
                chains: Vec::new(),
            });
        }
        (
            Part::new(
                faces,
                edges.into_iter().map(|(_, e)| e).collect(),
                Vec::new(),
            ),
            fillets,
        )
    }

    fn chain_faces(part: &Part, fillets: &[Fillet]) -> Vec<Vec<usize>> {
        derive(part, fillets)
            .unwrap()
            .into_iter()
            .map(|(c, _)| c.faces)
            .collect()
    }

    #[test]
    fn sampled_smooth_spline_requires_a_whole_edge_proof() {
        let (start, end) = ([2., 0., 0.], [2., 0., 1.]);
        let edge = |curve| Edge {
            curve,
            start,
            end,
            vertices: (0, 1),
            same_sense: true,
            samples: vec![start, end],
        };
        let line = edge(Curve::Line {
            origin: start,
            dir: FRAME.z,
        });
        let (part, fillets) = patches(&[2., 2.], vec![([0, 1], line)]);
        assert_eq!(chain_faces(&part, &fillets), [vec![0, 1]]);

        // A degree-one spline encodes the same exact generator. The point samples
        // still pass, so a regression to the sample-only fallback would merge it.
        let spline =
            NurbsCurve::new(1, vec![start, end], vec![1., 1.], vec![0., 0., 1., 1.]).unwrap();
        let (part, fillets) = patches(&[2., 2.], vec![([0, 1], edge(Curve::Nurbs(spline)))]);
        assert!(sample_contact(
            &part,
            0,
            1,
            &part.edges[0],
            cylinder_support_tolerance(&part)
        ));
        assert_eq!(
            chain_faces(&part, &fillets),
            [vec![0], vec![1]],
            "sampled spline contact cannot prove a chain join"
        );
    }

    #[test]
    fn overlapping_radius_candidates_follow_radius_order() {
        let tol = geom::COORD_FLOOR;
        // The middle radius matches each neighbour, while the full span exceeds precision.
        let radii = [2., 2. + 0.75 * tol, 2. + 1.5 * tol];
        let circle = |i: usize| {
            let radius = f64::midpoint(radii[i], radii[i + 1]);
            let frame = Frame {
                origin: [0., 0., i as f64],
                ..FRAME
            };
            let (start, end) = (
                frame.to_world([radius, 0., 0.]),
                frame.to_world([0., radius, 0.]),
            );
            (
                [i, i + 1],
                Edge {
                    curve: Curve::Circle { frame, radius },
                    start,
                    end,
                    vertices: (2 * i, 2 * i + 1),
                    same_sense: true,
                    samples: vec![start, end],
                },
            )
        };
        for order in [[0, 1], [1, 0]] {
            let (part, fillets) = patches(&radii, order.into_iter().map(circle).collect());
            assert_eq!(
                chain_faces(&part, &fillets),
                [vec![0, 1], vec![2]],
                "radius ordering must survive reversed edges"
            );
        }
    }

    #[test]
    fn unsupported_legacy_face_returns_an_error() {
        let (mut part, fillets) = patches(&[2.], Vec::new());
        part.faces[0].surface = Surface::Plane { frame: FRAME };
        assert_eq!(
            derive(&part, &fillets).err().as_deref(),
            Some("fillet face 0 has no analytic rolling-ball radius")
        );
    }
}
