//! Connected, tangent runs and radius groups of accepted fillet faces.

use std::collections::BTreeMap;

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
}

/// The raw rolling-ball radius of each reported analytic face.
fn radius(surface: &Surface) -> f64 {
    match *surface {
        Surface::Cylinder { radius, .. } | Surface::Sphere { radius, .. } => radius,
        Surface::Torus { minor, .. } => minor,
        _ => unreachable!("a legacy fillet claims only cylindrical, toroidal or spherical faces"),
    }
}

struct Patch {
    face: usize,
    fillet: usize,
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

pub(super) fn derive(part: &Part, fillets: &[Fillet]) -> Vec<(FilletChain, Radii)> {
    if fillets.is_empty() {
        return Vec::new();
    }
    let contact = cylinder_support_tolerance(part);
    let tolerance = radius_tolerance(part);
    let mut patches = Vec::new();
    let mut by_face = vec![None; part.faces.len()];
    for (fillet, record) in fillets.iter().enumerate() {
        for &face in &record.faces {
            let i = patches.len();
            let r = radius(&part.faces[face].surface);
            by_face[face] = Some(i);
            patches.push(Patch {
                face,
                fillet,
                root: i,
                radii: Radii { min: r, max: r },
            });
        }
    }

    let mut walls = Vec::new();
    for (edge, incident) in part.edge_faces().iter().enumerate() {
        let &[a, b] = incident.as_slice() else {
            continue;
        };
        let (i, j) = match (by_face[a], by_face[b]) {
            (Some(i), Some(j)) => (i, j),
            (Some(i), None) | (None, Some(i)) => {
                if sample_contact(part, a, b, &part.edges[edge], contact) {
                    walls.push((i, edge));
                }
                continue;
            }
            (None, None) => continue,
        };
        let (x, y) = (root(&mut patches, i), root(&mut patches, j));
        let combined = patches[x].radii.combined(patches[y].radii);
        if x == y
            || !combined.fits(tolerance)
            || fillets[patches[i].fillet].side != fillets[patches[j].fillet].side
        {
            continue;
        }
        // A spherical corner has a point spine. Attach it to its legacy owner only,
        // so it cannot link independent rolling runs around a three-way junction.
        if patches[i].fillet != patches[j].fillet
            && [a, b]
                .iter()
                .any(|&f| matches!(part.faces[f].surface, Surface::Sphere { .. }))
        {
            continue;
        }
        if tangent(part, a, b, &part.edges[edge], contact) {
            patches[y].root = x;
            patches[x].radii = combined;
        }
    }

    let mut components = BTreeMap::new();
    for i in 0..patches.len() {
        let r = root(&mut patches, i);
        let radii = patches[r].radii;
        let patch = &patches[i];
        let (chain, _) = components.entry(r).or_insert_with(|| {
            (
                FilletChain {
                    radius: radii.min,
                    side: fillets[patch.fillet].side,
                    faces: Vec::new(),
                    edges: Vec::new(),
                    fillets: Vec::new(),
                },
                radii,
            )
        });
        chain.faces.push(patch.face);
        chain.fillets.push(patch.fillet);
    }
    for (i, edge) in walls {
        components
            .get_mut(&root(&mut patches, i))
            .unwrap()
            .0
            .edges
            .push(edge);
    }
    let mut chains: Vec<_> = components.into_values().collect();
    for (chain, _) in &mut chains {
        chain.faces.sort_unstable();
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
    chains
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
    }
}

// The kernel's smooth-arc gap is 1 - cos(theta). Its chord equivalent compares
// unit-normal distances directly, limiting the first-order cross-edge slope to
// about 45 microradians for a smooth join, independently of the length tolerance.
fn normal_gap() -> f64 {
    (2.0 * geom::SMOOTH_ARC_GAP).sqrt()
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
    edge.samples
        .iter()
        .chain([&mid])
        .all(|&p| matches!((at(a, p), at(b, p)), (Some(x), Some(y)) if dist(x, y) <= normal_gap()))
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
        radius(&face.surface),
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
        if dist(x.fixed, y.fixed) + (x.radial - y.radial).abs() + x.error + y.error <= normal_gap())
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
    let chains = guarded(&mut features.failures, "fillet_chains", "discover", || {
        derive(part, &features.fillets)
    });
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
        chain_status.push(inherited(fillet_status, &chain.fillets));
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
