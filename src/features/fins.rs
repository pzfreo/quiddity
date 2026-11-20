//! Fins and ribs (`quiddity.fins`): a thin plate-like wall standing on a host surface, one
//! record per wall. The family is the explicit rib premise the stock-body veto anticipates: it
//! claims only the wall's two sides and its free tips, never the host and never a face that is
//! larger than the wall (a boss, pad or housing top the wall ends flush with).

use std::collections::BTreeSet;

use serde::Serialize;

use super::Context;
use super::evidence::{self, EvidenceError, Occurrence, common_valid_solid};
use super::graph::face_vertices;
use super::placed::{Grid, Placed};
use super::planes::{coordinates, unit_plane_normal};
use super::policy::AXIS_ZERO_COS;
use crate::kernel::brep::{Arc, Part};
use crate::kernel::classify::State;
use crate::kernel::geom::{self, COORD_FLOOR, Curve, Surface, V3};
use crate::kernel::py;

/// The sides' normals must be opposed to within this much (their dot product from -1).
const PARALLEL_TOL: f64 = 5e-5;
/// A wall is thin when its thickness is at most this fraction of both its height and length.
const MAX_THICKNESS_FRAC: f64 = 0.25;
/// A slender wall (a low ridge on a long run) is thin when its thickness is at most this
/// fraction of its length and of its height.
const SLENDER_LENGTH_FRAC: f64 = 0.1;
const SLENDER_HEIGHT_FRAC: f64 = 0.6;
/// A tip face is no larger than this multiple of thickness times its edge with a side; a
/// rounded (half-round) tip is 1.57 times that of a flat one.
const MAX_TIP_AREA_FACTOR: f64 = 2.0;

/// A thin wall standing proud of a host plane or cylinder (`Fin`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Fin {
    /// The sides' unit normal; the sign is chosen in the recognition frame, first non-zero component positive.
    pub normal: V3,
    /// The distance between the two parallel side planes.
    pub thickness: f64,
    /// Along the root: the extent of a side along the run direction.
    pub length: f64,
    /// From the root towards the free tip: the extent of a side along the rise direction.
    pub height: f64,
    /// The unit direction of the wall's longest straight root edge, first non-zero component
    /// positive.
    pub run: V3,
    /// The unit direction from the root to the free tip, in the sides' plane.
    pub rise: V3,
    /// The wall's centre: the middle of its run and rise extents, on the midplane.
    pub at: Placed<V3>,
    /// What the wall stands on: `plane`, `cylinder` or `mixed` (both, as a fin between a tube
    /// and a flange).
    pub host: &'static str,
    /// The two side faces, ascending.
    pub side_faces: Vec<usize>,
    /// The free tip faces (planar or rounded) between the sides.
    pub tip_faces: Vec<usize>,
    /// The host faces the wall's root meets, ascending: context, never claimed.
    pub host_faces: Vec<usize>,
}

/// A wall's side: a planar face on a single loop of straight or circular edges.
fn side_face(part: &Part, face: usize) -> bool {
    matches!(part.faces[face].surface, Surface::Plane { .. })
        && part.faces[face].loops.len() == 1
        && part.face_edges(face).iter().all(|&e| {
            matches!(
                part.edges[e].curve,
                Curve::Line { .. } | Curve::Circle { .. }
            )
        })
}

/// One surface read as a plane (unit normal, offset) or a cylinder (axis point, axis, radius),
/// for the test that two host faces are the same surface.
fn same_surface(part: &Part, a: usize, b: usize, tol: f64) -> bool {
    if a == b {
        return true;
    }
    match (&part.faces[a].surface, &part.faces[b].surface) {
        (Surface::Plane { frame: fa }, Surface::Plane { frame: fb }) => {
            geom::dot(fa.z, fb.z).abs() > 1.0 - PARALLEL_TOL
                && geom::dot(geom::sub(fb.origin, fa.origin), fa.z).abs() <= tol
        }
        (
            Surface::Cylinder {
                frame: fa,
                radius: ra,
            },
            Surface::Cylinder {
                frame: fb,
                radius: rb,
            },
        ) => {
            let off = geom::sub(fb.origin, fa.origin);
            let along = geom::dot(off, fa.z);
            let perp = [0, 1, 2].map(|i| off[i] - along * fa.z[i]);
            (ra - rb).abs() <= tol
                && geom::dot(fa.z, fb.z).abs() > 1.0 - PARALLEL_TOL
                && geom::norm(perp) <= tol
        }
        _ => false,
    }
}

/// How one side meets its neighbours.
struct Rim {
    /// Convex neighbours.
    convex: Vec<usize>,
    /// Concave neighbours, each meeting the side along straight edges only.
    hosts: Vec<usize>,
}

/// Split a side's neighbours into convex and concave ones. A tangent join is allowed only to a
/// half-round tip: a cylinder of radius half the thickness. Any other join (tangent, unknown),
/// a concave join along a curved edge, or a host that is neither plane nor cylinder refuses.
fn rim(part: &Part, side: usize, thickness: f64, tol: f64) -> Option<Rim> {
    let mut convex = Vec::new();
    let mut hosts = Vec::new();
    for n in part.neighbours(side) {
        match part.arc(side, n)? {
            Arc::Convex => convex.push(n),
            Arc::Concave => {
                if !matches!(
                    part.faces[n].surface,
                    Surface::Plane { .. } | Surface::Cylinder { .. }
                ) || !part
                    .shared_edges(side, n)
                    .iter()
                    .all(|&e| matches!(part.edges[e].curve, Curve::Line { .. }))
                {
                    return None;
                }
                hosts.push(n);
            }
            Arc::Smooth => {
                let Surface::Cylinder { radius, .. } = part.faces[n].surface else {
                    return None;
                };
                if (radius - 0.5 * thickness).abs() > 4. * tol {
                    return None;
                }
                convex.push(n);
            }
            Arc::Unknown => return None,
        }
    }
    convex.sort_unstable();
    hosts.sort_unstable();
    Some(Rim { convex, hosts })
}

/// Thin: a quarter of both height and length at most, or slender: a tenth of the length and
/// 0.6 of the height at most.
fn is_thin(t: f64, length: f64, height: f64) -> bool {
    t <= MAX_THICKNESS_FRAC * length.min(height)
        || (t <= SLENDER_LENGTH_FRAC * length && t <= SLENDER_HEIGHT_FRAC * height)
}

fn edge_length(part: &Part, e: usize) -> f64 {
    part.edges[e]
        .samples
        .windows(2)
        .map(|w| geom::norm(geom::sub(w[1], w[0])))
        .sum()
}

/// The ends of every edge *a* shares with the tip, each moved by *shift*, are ends of the
/// edges *b* shares with it (and the counts agree): the tip bridges the sides as a prism.
fn bridges(part: &Part, tip: usize, a: usize, b: usize, shift: V3, tol: f64) -> bool {
    let ends = |side: usize| -> Vec<V3> {
        part.shared_edges(side, tip)
            .iter()
            .flat_map(|&e| [part.edges[e].start, part.edges[e].end])
            .collect()
    };
    let (ea, eb) = (ends(a), ends(b));
    !ea.is_empty()
        && ea.len() == eb.len()
        && ea.iter().all(|&p| {
            let q = geom::add(p, shift);
            eb.iter().any(|&r| geom::norm(geom::sub(q, r)) <= tol)
        })
}

fn is_perpendicular_plane(part: &Part, face: usize, normal: V3) -> bool {
    matches!(part.faces[face].surface, Surface::Plane { .. })
        && unit_plane_normal(part, face)
            .is_some_and(|n| geom::dot(n, normal).abs() <= AXIS_ZERO_COS * 0.1)
}

fn pair(ctx: &Context<'_>, s1: usize, s2: usize) -> Option<Occurrence<Fin>> {
    let part = ctx.part;
    let tol = super::bevel::cylinder_support_tolerance(part);
    let (n1, n2) = (unit_plane_normal(part, s1)?, unit_plane_normal(part, s2)?);
    if geom::dot(n1, n2) > -1.0 + PARALLEL_TOL {
        return None;
    }
    let (Surface::Plane { frame: f1 }, Surface::Plane { frame: f2 }) =
        (&part.faces[s1].surface, &part.faces[s2].surface)
    else {
        return None;
    };
    // The second side lies behind the first, against its outward normal: material between.
    let gap = geom::dot(geom::sub(f2.origin, f1.origin), n1);
    let thickness = -gap;
    if thickness <= 4. * tol {
        return None;
    }
    let solid = part.faces[s1].solid?;
    if part.faces[s2].solid != Some(solid) {
        return None;
    }
    let rim1 = rim(part, s1, thickness, tol)?;
    let rim2 = rim(part, s2, thickness, tol)?;
    // The faces the sides share, convex, are tips (thin) or faces the wall ends flush with.
    if rim1.convex != rim2.convex || rim1.convex.is_empty() {
        return None;
    }
    let shift = geom::scale(n1, -thickness);
    let (mut tips, mut flush) = (Vec::new(), Vec::new());
    for &f in &rim1.convex {
        let shared = part.shared_edges(s1, f);
        let edge_len: f64 = shared.iter().map(|&e| edge_length(part, e)).sum();
        let area = part.face_mass(f).map(|m| m[0]);
        let thin = area.is_some_and(|a| a <= MAX_TIP_AREA_FACTOR * thickness * edge_len + tol);
        if thin {
            if !bridges(part, f, s1, s2, shift, 2. * tol) {
                return None;
            }
            tips.push(f);
        } else if is_perpendicular_plane(part, f, n1) {
            flush.push(f);
        } else {
            return None;
        }
    }
    if tips.is_empty() {
        return None;
    }
    // Hosts: each concave neighbour of a side is met, on the other side, by the same surface.
    if rim1.hosts.is_empty()
        || rim2.hosts.is_empty()
        || !rim1.hosts.iter().all(|&h| {
            rim2.hosts
                .iter()
                .any(|&g| same_surface(part, h, g, 4. * tol))
        })
        || !rim2.hosts.iter().all(|&h| {
            rim1.hosts
                .iter()
                .any(|&g| same_surface(part, h, g, 4. * tol))
        })
    {
        return None;
    }
    // Junction: a host that is a side of another wall (a rib crossing or meeting this one; its other
    // side lies across a tip), whose other side meets a face coplanar with one of ours, means the wall runs on beyond it: each
    // piece's length would be wrong, so the wall is refused.
    let coplanar = |g: usize, side: usize, normal: V3, origin: V3| {
        g != side
            && unit_plane_normal(part, g)
                .is_some_and(|ng| geom::dot(ng, normal) > 1. - PARALLEL_TOL)
            && matches!(&part.faces[g].surface, Surface::Plane { frame }
                if geom::dot(geom::sub(frame.origin, origin), normal).abs() <= 4. * tol)
    };
    for &h in rim1.hosts.iter().chain(&rim2.hosts) {
        if !matches!(part.faces[h].surface, Surface::Plane { .. }) {
            continue;
        }
        let Some(nh) = unit_plane_normal(part, h) else {
            continue;
        };
        let across: BTreeSet<usize> = part
            .neighbours(h)
            .into_iter()
            .flat_map(|f| part.neighbours(f))
            .filter(|&f| f != h)
            .collect();
        for h2 in across {
            if !matches!(part.faces[h2].surface, Surface::Plane { .. })
                || unit_plane_normal(part, h2).is_none_or(|n| geom::dot(n, nh) > -1. + PARALLEL_TOL)
            {
                continue;
            }
            if part
                .neighbours(h2)
                .into_iter()
                .any(|g| coplanar(g, s1, n1, f1.origin) || coplanar(g, s2, n2, f2.origin))
            {
                return None;
            }
        }
    }
    // A host must not be the wall's own side or tip.
    let mut hosts: Vec<usize> = rim1.hosts.iter().chain(&rim2.hosts).copied().collect();
    hosts.sort_unstable();
    hosts.dedup();
    if hosts.iter().any(|h| tips.contains(h) || flush.contains(h)) {
        return None;
    }
    // Measurements on the first side. The run is its longest root edge.
    let root: Vec<usize> = rim1
        .hosts
        .iter()
        .flat_map(|&h| part.shared_edges(s1, h))
        .collect();
    let longest = *root
        .iter()
        .max_by(|&&a, &&b| edge_length(part, a).total_cmp(&edge_length(part, b)))?;
    let mut run = geom::unit(geom::sub(
        part.edges[longest].end,
        part.edges[longest].start,
    ))?;
    if geom::dot(run, n1).abs() > AXIS_ZERO_COS {
        return None;
    }
    let mut rise = geom::unit(geom::cross(n1, run))?;
    let mid = |edges: &[usize]| -> V3 {
        let pts: Vec<V3> = edges
            .iter()
            .flat_map(|&e| [part.edges[e].start, part.edges[e].end])
            .collect();
        geom::scale(
            pts.iter().fold([0.; 3], |s, &p| geom::add(s, p)),
            1. / pts.len() as f64,
        )
    };
    let tip_edges: Vec<usize> = tips
        .iter()
        .flat_map(|&t| part.shared_edges(s1, t))
        .collect();
    if geom::dot(geom::sub(mid(&tip_edges), mid(&root)), rise) < 0. {
        rise = geom::scale(rise, -1.);
    }
    if geom::dot(geom::sub(mid(&tip_edges), mid(&root)), rise) <= 2. * tol {
        return None;
    }
    let points: Vec<V3> = part
        .face_edges(s1)
        .iter()
        .flat_map(|&e| part.edges[e].samples.iter().copied())
        .collect();
    // A half-round tip bulges past its tangent edges by its radius.
    let bulges: Vec<(V3, V3, f64)> = tips
        .iter()
        .filter_map(|&t| match part.faces[t].surface {
            Surface::Cylinder { frame, radius } => (geom::dot(frame.z, n1).abs() <= AXIS_ZERO_COS)
                .then_some((frame.origin, frame.z, radius)),
            _ => None,
        })
        .collect();
    let extent = |d: V3| {
        let (mut lo, mut hi) =
            points
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), &p| {
                    let s = geom::dot(p, d);
                    (l.min(s), h.max(s))
                });
        for (o, axis, r) in &bulges {
            if geom::dot(*axis, d).abs() > AXIS_ZERO_COS {
                continue;
            }
            let s = geom::dot(*o, d);
            lo = lo.min(s - r);
            hi = hi.max(s + r);
        }
        (lo, hi)
    };
    let ((run_lo, run_hi), (rise_lo, rise_hi)) = (extent(run), extent(rise));
    let (length, height) = (run_hi - run_lo, rise_hi - rise_lo);
    if length <= 0. || height <= 0. || !is_thin(thickness, length, height) {
        return None;
    }
    // The wall is material between its sides.
    let verts = face_vertices(part, s1);
    let centre = geom::scale(
        verts.iter().fold([0.; 3], |s, &p| geom::add(s, p)),
        1. / verts.len() as f64,
    );
    let mid = geom::add(centre, geom::scale(shift, 0.5));
    // The wall's centre: the middle of its extents on the midplane.
    let at = geom::add(
        geom::add(
            geom::scale(run, 0.5 * (run_lo + run_hi)),
            geom::scale(rise, 0.5 * (rise_lo + rise_hi)),
        ),
        geom::scale(n1, geom::dot(f1.origin, n1) - 0.5 * thickness),
    );
    if ctx.solid_classifier(solid).classify(mid) != State::In
        || ctx
            .solid_classifier(solid)
            .classify(geom::add(centre, geom::scale(shift, 0.05)))
            != State::In
    {
        return None;
    }
    let faces: Vec<usize> = [s1, s2].into_iter().chain(tips.iter().copied()).collect();
    let mut all = faces.clone();
    all.extend(&hosts);
    all.extend(&flush);
    common_valid_solid(part, &all)?;
    let kinds: BTreeSet<bool> = hosts
        .iter()
        .map(|&h| matches!(part.faces[h].surface, Surface::Plane { .. }))
        .collect();
    let host = match (kinds.contains(&true), kinds.contains(&false)) {
        (true, false) => "plane",
        (false, true) => "cylinder",
        _ => "mixed",
    };
    if run
        .iter()
        .find(|c| c.abs() > COORD_FLOOR)
        .is_some_and(|&c| c < 0.)
    {
        run = geom::scale(run, -1.);
    }
    let canonical = |v: V3| {
        if v.iter()
            .find(|c| c.abs() > COORD_FLOOR)
            .is_some_and(|&c| c < 0.)
        {
            geom::scale(v, -1.)
        } else {
            v
        }
    };
    let mut sides = vec![s1, s2];
    sides.sort_unstable();
    tips.sort_unstable();
    let mut context = hosts.clone();
    context.extend(&flush);
    context.sort_unstable();
    context.dedup();
    let mut defining = faces;
    defining.sort_unstable();
    Some(Occurrence {
        record: Fin {
            normal: coordinates(canonical(n1), 6),
            thickness: py::round_to3(thickness),
            length: py::round_to3(length),
            height: py::round_to3(height),
            run: coordinates(run, 6),
            rise: coordinates(rise, 6),
            at: Placed::new(at, Grid::Decimals(3)),
            host,
            side_faces: sides,
            tip_faces: tips,
            host_faces: hosts,
        },
        defining,
        context,
    })
}

internal_entry! {
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<Fin>> {
    let part = ctx.part;
    let mut out = Vec::new();
    for s1 in 0..part.faces.len() {
        if !side_face(part, s1) {
            continue;
        }
        let mut partners = BTreeSet::new();
        for f in part.neighbours(s1) {
            for s2 in part.neighbours(f) {
                if s2 > s1 && side_face(part, s2) {
                    partners.insert(s2);
                }
            }
        }
        for s2 in partners {
            if let Some(o) = pair(ctx, s1, s2) {
                out.push(o);
            }
        }
    }
    out.sort_by(|a, b| py::tuple_order(a.record.at.raw(), b.record.at.raw()));
    out
}
}

internal_entry! {
pub fn discover_verified(ctx: &Context<'_>) -> Result<Vec<Occurrence<Fin>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx))
}
}

/// Publish after the existing families: a wall whose side or tip another family claims is not
/// published, and adding this family never changes an existing record. The evidence families
/// `step_levels` and `risers` and the whole-body classifications co-own faces and do not
/// count as claims.
pub(super) fn append(ctx: &Context<'_>, features: &mut super::Features) {
    let claimed: BTreeSet<usize> = features
        .defining
        .iter()
        .filter(|(family, _)| {
            !matches!(
                **family,
                "step_levels"
                    | "risers"
                    | "thin_wall_bodies"
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
    let mut taken = BTreeSet::new();
    let found: Vec<_> = discover(ctx)
        .into_iter()
        // A claimed side drops the wall; a claimed tip is left to its claimant.
        .filter(|o| o.record.side_faces.iter().all(|f| !claimed.contains(f)))
        .map(|mut o| {
            o.defining.retain(|f| !claimed.contains(f));
            o
        })
        // A face is one wall's: an ambiguous second claim drops that wall.
        .filter(|o| {
            let fresh = o.defining.iter().all(|f| !taken.contains(f));
            taken.extend(o.defining.iter().copied());
            fresh
        })
        .collect();
    if found.is_empty() {
        return;
    }
    let mut registration = super::Kept {
        part: ctx.part,
        unmeasured: super::unmeasured_faces(ctx.part).into_iter().collect(),
        claimed: Default::default(),
        defining: Default::default(),
        constituent: Default::default(),
        evidence_status: Default::default(),
    };
    features.fins = super::kept(&mut registration, "fins", found);
    features.defining.extend(registration.defining);
    features
        .evidence_status
        .extend(registration.evidence_status);
}
