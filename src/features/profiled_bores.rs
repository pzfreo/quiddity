//! Principal-axis through double-D bores (`quiddity.profiled_bores`): two parallel chords joined
//! by two arcs of one circle, the same profile opening on both opposite extremal faces of a
//! solid, and the whole profile prism between them proved void.
//!
//! The metric checks (one parent circle, opposed chords, chord and arc lengths) are what tell a
//! double-D from an obround or a lens with the same edge count. Through bores only: a single
//! opening could be a blind recess, whose floor needs its own proof.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::Context;
use super::evidence::{EvidenceError, Occurrence, common_valid_solid};
use super::regions::edge_length;
use crate::kernel::brep::Part;
use crate::kernel::classify::Classifier;
use crate::kernel::geom::{self, Bounds, Curve, Surface, V3};
use crate::kernel::py;
use crate::kernel::sampling::arc_extremes;
use crate::kernel::volume::{Prism, PrismEdge, Probe, common_volume};

/// One geometrically proven through double-D bore. `major_diameter` is the parent circle's,
/// `across_flats` the distance between the chords, and `flat_direction` their canonical unit
/// normal. `location` is the profile centre on the bore's high end.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct DoubleDBore {
    pub axis: V3,
    pub location: V3,
    pub major_diameter: f64,
    pub across_flats: f64,
    pub depth: f64,
    pub through: bool,
    pub flat_direction: V3,
}

impl DoubleDBore {
    /// `(axis, location, major_diameter)`, the order Python sorts bores in.
    fn order(&self, other: &Self) -> Ordering {
        py::tuple_order(&self.axis, &other.axis)
            .then_with(|| py::tuple_order(&self.location, &other.location))
            .then_with(|| py::order(self.major_diameter, other.major_diameter))
    }
}

/// `recognise_double_d_bores`' keyword arguments.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct DoubleDBoreOptions {
    pub tol: f64,
}

impl Default for DoubleDBoreOptions {
    fn default() -> Self {
        DoubleDBoreOptions { tol: 1e-5 }
    }
}

/// The reading of one double-D wire (`DoubleDProfile`).
#[derive(Clone, Copy, Debug)]
struct Profile {
    centre: V3,
    major_diameter: f64,
    across_flats: f64,
    flat_direction: V3,
}

/// A double-D inner wire of an extremal principal plane face.
struct Opening {
    axis: usize,
    at: f64,
    profile: Profile,
    face: usize,
    /// The wire's edges, each with its direction in the wire.
    wire: Vec<(usize, bool)>,
}

internal_entry! {
/// `recognise_double_d_bores`.
pub fn recognise_double_d_bores(part: &Part, opts: &DoubleDBoreOptions) -> Vec<DoubleDBore> {
    super::records(discover_with(&Context::new(part), opts))
}
}

internal_entry! {
/// Every bore of every solid with its complete lateral wall (empty when the wall could not be
/// proved), in record order.
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<DoubleDBore>> {
    discover_with(ctx, &DoubleDBoreOptions::default())
}
}

fn discover_with(ctx: &Context<'_>, opts: &DoubleDBoreOptions) -> Vec<Occurrence<DoubleDBore>> {
    let part = ctx.part;
    // Each solid owns its own extrema, so two bodies cannot pair into one bore across a gap.
    let mut found: Vec<Occurrence<DoubleDBore>> = if part.solids.is_empty() {
        let faces: Vec<usize> = (0..part.faces.len()).collect();
        recognise_one(part, ctx.classifier(), &faces, part.bounds(), opts.tol)
    } else {
        (0..part.solids.len())
            .flat_map(|s| {
                recognise_one(
                    part,
                    ctx.solid_classifier(s),
                    &part.solids[s].faces,
                    part.solid_bounds(s),
                    opts.tol,
                )
            })
            .collect()
    };
    found.sort_by(|a, b| a.record.order(&b.record));
    found
}

internal_entry! {
/// The evidence path: each bore claims its complete original lateral wall. Refused, as Python
/// refuses, when a wall is incomplete, has no one valid owner, or is claimed twice.
pub fn discover_verified(ctx: &Context<'_>) -> Result<Vec<Occurrence<DoubleDBore>>, EvidenceError> {
    let found = discover(ctx);
    let mut assigned = BTreeSet::new();
    for o in &found {
        if o.defining.is_empty() || common_valid_solid(ctx.part, &o.defining).is_none() {
            return Err(EvidenceError::NoValidSolid);
        }
        if o.defining.iter().any(|f| !assigned.insert(*f)) {
            return Err(EvidenceError::SharedEvidence);
        }
    }
    Ok(found)
}
}

/// `_recognise_double_d_bores_one`: the bores within one solid's own boundary.
fn recognise_one(
    part: &Part,
    classifier: &Classifier<'_>,
    faces: &[usize],
    bbox: Bounds,
    tol: f64,
) -> Vec<Occurrence<DoubleDBore>> {
    let scan_tol = tol.max(bbox.max_extent() * 1e-5);
    let mut openings = Vec::new();
    for &face in faces {
        let Some((axis, at)) = principal_boundary_plane(part, face, &bbox) else {
            continue;
        };
        let outer = part.outer_loop(face);
        for (l, lp) in part.faces[face].loops.iter().enumerate() {
            if Some(l) == outer || lp.edges.is_empty() {
                continue;
            }
            let mut wire: Vec<(usize, bool)> = Vec::new();
            for &(e, forward) in &lp.edges {
                if !wire.iter().any(|w| w.0 == e) {
                    wire.push((e, forward));
                }
            }
            if let Some(profile) = double_d_profile(part, &wire, axis, scan_tol) {
                openings.push(Opening {
                    axis,
                    at,
                    profile,
                    face,
                    wire,
                });
            }
        }
    }
    let mut out = Vec::new();
    for axis in 0..3 {
        let (lo, hi) = (bbox.min[axis], bbox.max[axis]);
        let low = openings
            .iter()
            .filter(|o| o.axis == axis && (o.at - lo).abs() <= scan_tol);
        let high: Vec<&Opening> = openings
            .iter()
            .filter(|o| o.axis == axis && (o.at - hi).abs() <= scan_tol)
            .collect();
        for low in low {
            let Some(high) = high
                .iter()
                .find(|h| profiles_correspond(&h.profile, &low.profile, axis, scan_tol))
            else {
                continue;
            };
            // The actual opening wire swept across the claimed depth must hold no material: two
            // blind recesses could otherwise leave a web between equal openings.
            let prism = Prism {
                axis,
                lo: low.at,
                hi: low.at + (hi - lo),
                loops: vec![
                    low.wire
                        .iter()
                        .map(|&(e, forward)| {
                            let edge = &part.edges[e];
                            let mut points = edge.samples.clone();
                            if !forward {
                                points.reverse();
                            }
                            PrismEdge {
                                points,
                                straight: matches!(edge.curve, Curve::Line { .. }),
                            }
                        })
                        .collect(),
                ],
            };
            let volume_tol = scan_tol.powi(3).max(prism.area() * (hi - lo) * 1e-6);
            // The claim is that the swept opening is clear; an unanswered probe does not show
            // that, so it refuses the bore like material would.
            if common_volume(classifier, &Probe::Prism(prism)).is_none_or(|v| v > volume_tol) {
                continue;
            }
            let mut axis_vector = [0.0; 3];
            axis_vector[axis] = 1.0;
            let mut location = high.profile.centre;
            location[axis] = hi;
            let record = DoubleDBore {
                axis: axis_vector,
                location,
                major_diameter: high.profile.major_diameter,
                across_flats: high.profile.across_flats,
                depth: py::round_to(hi - lo, 4),
                through: true,
                flat_direction: high.profile.flat_direction,
            };
            let walls = complete_wall_component(part, faces, low, high, axis, lo, hi, scan_tol);
            out.push(Occurrence {
                record,
                defining: walls,
                context: Vec::new(),
            });
        }
    }
    out
}

/// `(normal axis, coordinate)` of a native plane face lying flat on one face of the box
/// (`principal_boundary_plane`).
pub(crate) fn principal_boundary_plane(
    part: &Part,
    face: usize,
    bbox: &Bounds,
) -> Option<(usize, f64)> {
    if !matches!(part.faces[face].surface, Surface::Plane { .. }) {
        return None;
    }
    let fbb = part.face_bounds(face);
    let tol = 1e-5_f64.max(bbox.max_extent() * 1e-5);
    let flat: Vec<usize> = (0..3).filter(|&a| fbb.max[a] - fbb.min[a] <= tol).collect();
    let [axis] = flat[..] else {
        return None;
    };
    let at = (fbb.min[axis] + fbb.max[axis]) / 2.0;
    ((at - bbox.min[axis]).abs().min((at - bbox.max[axis]).abs()) <= tol).then_some((axis, at))
}

/// The box of a wire's edges (`wire.bounding_box()`).
fn wire_bounds(part: &Part, wire: &[(usize, bool)]) -> Bounds {
    let mut b = Bounds::empty();
    for &(e, _) in wire {
        let edge = &part.edges[e];
        for p in edge.samples.iter().copied().chain(arc_extremes(
            &edge.curve,
            edge.start,
            edge.end,
            edge.same_sense,
            edge.is_closed(),
            &[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        )) {
            b.add(p);
        }
    }
    b
}

/// The two coordinates across *axis*, in order.
fn plane_axes(axis: usize) -> [usize; 2] {
    match axis {
        0 => [1, 2],
        1 => [0, 2],
        _ => [0, 1],
    }
}

/// Read a double-D wire in the plane normal to *axis*, rejecting merely topology-similar loops
/// (`double_d_profile`).
fn double_d_profile(part: &Part, wire: &[(usize, bool)], axis: usize, tol: f64) -> Option<Profile> {
    let edges: Vec<usize> = wire.iter().map(|w| w.0).collect();
    let lines: Vec<usize> = edges
        .iter()
        .copied()
        .filter(|&e| matches!(part.edges[e].curve, Curve::Line { .. }))
        .collect();
    let arcs: Vec<(usize, V3, f64)> = edges
        .iter()
        .filter_map(|&e| match &part.edges[e].curve {
            Curve::Circle { frame, radius } => Some((e, frame.origin, *radius)),
            _ => None,
        })
        .collect();
    if edges.len() != 4 || lines.len() != 2 || arcs.len() != 2 {
        return None;
    }
    let [u_i, v_i] = plane_axes(axis);
    let wbb = wire_bounds(part, wire);
    let profile_scale = (wbb.max[u_i] - wbb.min[u_i]).max(wbb.max[v_i] - wbb.min[v_i]);
    let metric_tol = (8.0 * tol).max(profile_scale * 1e-3);
    let (_, centre, radius) = arcs[0];
    if radius <= tol {
        return None;
    }
    if arcs[1..].iter().any(|&(_, c, r)| {
        (r - radius).abs() > metric_tol
            || [u_i, v_i]
                .iter()
                .any(|&a| (c[a] - centre[a]).abs() > metric_tol)
    }) {
        return None;
    }
    let mut directions: Vec<V3> = Vec::new();
    let mut midpoints: Vec<V3> = Vec::new();
    for &line in &lines {
        let ends = part.edges[line].vertex_points();
        let [a, b] = ends[..] else {
            return None;
        };
        let delta = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let length = (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
        if length <= tol || delta[axis].abs() > metric_tol {
            return None;
        }
        directions.push(delta.map(|d| d / length));
        midpoints.push([0, 1, 2].map(|i| (a[i] + b[i]) / 2.0));
        if [a, b]
            .iter()
            .any(|&end| (geom::dist(end, centre) - radius).abs() > metric_tol)
        {
            return None;
        }
    }
    let parallel = (0..3)
        .map(|i| directions[0][i] * directions[1][i])
        .sum::<f64>()
        .abs();
    if (parallel - 1.0).abs() > 1e-4 {
        return None;
    }
    let direction = directions[0];
    let mut flat_direction = [0.0; 3];
    flat_direction[u_i] = -direction[v_i];
    flat_direction[v_i] = direction[u_i];
    if flat_direction
        .iter()
        .find(|c| c.abs() > 1e-12)
        .is_some_and(|&c| c < 0.0)
    {
        flat_direction = flat_direction.map(|c| -c);
    }
    let mut offsets: Vec<f64> = midpoints
        .iter()
        .map(|m| (0..3).map(|i| (m[i] - centre[i]) * flat_direction[i]).sum())
        .collect();
    offsets.sort_by(f64::total_cmp);
    if offsets[0] >= -tol || offsets[1] <= tol || (offsets[0] + offsets[1]).abs() > metric_tol {
        return None;
    }
    let half_af = (offsets[1] - offsets[0]) / 2.0;
    if half_af <= tol || half_af >= radius - tol {
        return None;
    }
    let expected_chord = 2.0 * (radius * radius - half_af * half_af).sqrt();
    if lines
        .iter()
        .any(|&l| (edge_length(part, l) - expected_chord).abs() > metric_tol)
    {
        return None;
    }
    let expected_arc = 2.0 * radius * (half_af / radius).asin();
    if arcs
        .iter()
        .any(|&(e, _, _)| (edge_length(part, e) - expected_arc).abs() > metric_tol)
    {
        return None;
    }
    Some(Profile {
        centre,
        major_diameter: py::round_to(2.0 * radius, 4),
        across_flats: py::round_to(2.0 * half_af, 4),
        flat_direction: flat_direction.map(|c| {
            if c.abs() <= 1e-12 {
                0.0
            } else {
                py::round_to(c, 12)
            }
        }),
    })
}

/// Whether two end profiles prove one coaxial double-D cross-section (`_profiles_correspond`).
fn profiles_correspond(first: &Profile, second: &Profile, axis: usize, tol: f64) -> bool {
    (first.major_diameter - second.major_diameter).abs() <= tol
        && (first.across_flats - second.across_flats).abs() <= tol
        && (0..3)
            .all(|i| (first.flat_direction[i] - second.flat_direction[i]).abs() <= tol.max(1e-6))
        && plane_axes(axis)
            .iter()
            .all(|&a| (first.centre[a] - second.centre[a]).abs() <= tol)
}

/// A lateral face's supporting surface, canonically signed (`support`).
#[derive(Clone, Copy, Debug)]
enum Support {
    Plane { normal: V3, offset: f64 },
    Cylinder { direction: V3, rest: [f64; 3] },
}

/// The connected original lateral wall faces between two openings, in face order
/// (`_complete_wall_component`); empty when they do not form four proven wall chains.
///
/// Opening edges seed both ends. Traversal never crosses either end plane, so middle axial
/// subdivisions are kept while exterior stock reachable only through an end plane is not.
#[allow(clippy::too_many_arguments)]
fn complete_wall_component(
    part: &Part,
    faces: &[usize],
    low: &Opening,
    high: &Opening,
    axis: usize,
    lo: f64,
    hi: f64,
    tol: f64,
) -> Vec<usize> {
    let scope: BTreeSet<usize> = faces.iter().copied().collect();
    let incidence = |edge: usize| -> Vec<usize> {
        part.edge_faces()[edge]
            .iter()
            .copied()
            .filter(|f| scope.contains(f))
            .collect()
    };
    let seeds = |opening: &Opening| -> Vec<usize> {
        let mut found = Vec::new();
        for &(edge, _) in &opening.wire {
            let partners: Vec<usize> = incidence(edge)
                .into_iter()
                .filter(|&f| f != opening.face)
                .collect();
            let [partner] = partners[..] else {
                return Vec::new();
            };
            found.push(partner);
        }
        found
    };
    let low_seeds = seeds(low);
    let high_seeds = seeds(high);
    if low_seeds.len() != 4 || high_seeds.len() != 4 {
        return Vec::new();
    }
    let profile = &low.profile;
    let centre = profile.centre;
    let flat_direction = profile.flat_direction;
    let metric_tol = tol.max(profile.major_diameter * 1e-3);
    let dot = |a: V3, b: V3| (0..3).map(|i| a[i] * b[i]).sum::<f64>();

    let lateral = |face: usize| -> bool {
        if face == low.face || face == high.face {
            return false;
        }
        let fbb = part.face_bounds(face);
        let (face_lo, face_hi) = (fbb.min[axis], fbb.max[axis]);
        let valid_support = match &part.faces[face].surface {
            Surface::Plane { frame } => {
                if face_hi - face_lo <= 1e-9
                    || face_lo < lo - metric_tol
                    || face_hi > hi + metric_tol
                    || (dot(frame.z, flat_direction).abs() - 1.0).abs() > 1e-4
                {
                    return false;
                }
                let offset = dot(
                    [0, 1, 2].map(|i| frame.origin[i] - centre[i]),
                    flat_direction,
                )
                .abs();
                (offset - profile.across_flats / 2.0).abs() <= metric_tol
            }
            Surface::Cylinder { frame, radius } => {
                if face_hi - face_lo <= 1e-9
                    || face_lo < lo - metric_tol
                    || face_hi > hi + metric_tol
                    || (frame.z[axis].abs() - 1.0).abs() > 1e-4
                {
                    return false;
                }
                (radius - profile.major_diameter / 2.0).abs() <= metric_tol
                    && (0..3)
                        .all(|i| i == axis || (frame.origin[i] - centre[i]).abs() <= metric_tol)
            }
            _ => return false,
        };
        if !valid_support {
            return false;
        }
        let Some((u0, u1, v0, v1)) = part.uv_bounds(face) else {
            return false;
        };
        let (u, v) = (0.5 * (u0 + u1), 0.5 * (v0 + v1));
        let point = part.faces[face].surface.value(u, v);
        let Some(normal) = part.face_normal(face, u, v) else {
            return false;
        };
        // A void wall's material-outward normal points from the wall into the profile void.
        let mut radial_to_void = [0, 1, 2].map(|i| centre[i] - point[i]);
        radial_to_void[axis] = 0.0;
        dot(radial_to_void, normal) > metric_tol
    };

    let support = |face: usize| -> Support {
        match &part.faces[face].surface {
            Surface::Plane { frame } => {
                let mut normal = frame.z;
                let mut offset = dot(frame.origin, normal);
                if normal
                    .iter()
                    .find(|c| c.abs() > 1e-9)
                    .is_some_and(|&c| c < 0.0)
                {
                    normal = normal.map(|c| -c);
                    offset = -offset;
                }
                Support::Plane { normal, offset }
            }
            Surface::Cylinder { frame, radius } => {
                let mut direction = frame.z;
                if direction
                    .iter()
                    .find(|c| c.abs() > 1e-12)
                    .is_some_and(|&c| c < 0.0)
                {
                    direction = direction.map(|c| -c);
                }
                let [a, b] = plane_axes(axis);
                Support::Cylinder {
                    direction,
                    rest: [frame.origin[a], frame.origin[b], *radius],
                }
            }
            _ => unreachable!("only lateral faces have a support"),
        }
    };
    let close = |a: &V3, b: &V3, within: f64| (0..3).all(|i| (a[i] - b[i]).abs() <= within);
    let same_support = |left: Support, right: Support| match (left, right) {
        (
            Support::Plane { normal, offset },
            Support::Plane {
                normal: n,
                offset: o,
            },
        ) => close(&normal, &n, 1e-4) && (offset - o).abs() <= metric_tol,
        (
            Support::Cylinder { direction, rest },
            Support::Cylinder {
                direction: d,
                rest: r,
            },
        ) => close(&direction, &d, 1e-4) && close(&rest, &r, metric_tol),
        _ => false,
    };

    let chain = |seed: usize| -> Vec<usize> {
        if !lateral(seed) {
            return Vec::new();
        }
        let role = support(seed);
        let mut pending = vec![seed];
        let mut found: Vec<usize> = Vec::new();
        while let Some(face) = pending.pop() {
            if found.contains(&face) || !lateral(face) || !same_support(support(face), role) {
                continue;
            }
            found.push(face);
            for edge in part.face_edges(face) {
                for other in incidence(edge) {
                    if !found.contains(&other) {
                        pending.push(other);
                    }
                }
            }
        }
        found
    };

    let chains: Vec<Vec<usize>> = low_seeds.iter().map(|&s| chain(s)).collect();
    if chains.iter().any(|c| c.is_empty()) {
        return Vec::new();
    }
    // Each low role must reach exactly one high role, and every high role is consumed once.
    let mut high_assignments = Vec::new();
    for chain_faces in &chains {
        let matches: Vec<usize> = (0..high_seeds.len())
            .filter(|&at| chain_faces.contains(&high_seeds[at]))
            .collect();
        let [only] = matches[..] else {
            return Vec::new();
        };
        high_assignments.push(only);
    }
    let mut intervals: BTreeMap<usize, (f64, f64)> = BTreeMap::new();
    let mut edge_pairs: Vec<(usize, usize)> = Vec::new();
    let mut visited_edges: BTreeSet<usize> = BTreeSet::new();
    for chain_faces in &chains {
        for &face in chain_faces {
            let fbb = part.face_bounds(face);
            intervals.insert(face, (fbb.min[axis], fbb.max[axis]));
            for edge in part.face_edges(face) {
                if visited_edges.contains(&edge) {
                    continue;
                }
                let partners: Vec<usize> = incidence(edge)
                    .into_iter()
                    .filter(|o| chain_faces.contains(o) && *o != face)
                    .collect();
                if let Some(&other) = partners.first() {
                    visited_edges.insert(edge);
                    edge_pairs.push((face, other));
                }
            }
        }
    }
    if !valid_wall_chain_facts(
        &chains,
        &high_assignments,
        &intervals,
        &edge_pairs,
        lo,
        hi,
        metric_tol,
    ) {
        return Vec::new();
    }
    let seen: BTreeSet<usize> = chains.iter().flatten().copied().collect();
    faces.iter().copied().filter(|f| seen.contains(f)).collect()
}

/// Whether four logical lateral-wall chains each span the bore from end to end without gaps or
/// overlaps, and each multi-face chain is one simple path (`_valid_wall_chain_facts`).
fn valid_wall_chain_facts(
    chains: &[Vec<usize>],
    high_assignments: &[usize],
    intervals: &BTreeMap<usize, (f64, f64)>,
    edges: &[(usize, usize)],
    lo: f64,
    hi: f64,
    tol: f64,
) -> bool {
    if ![lo, hi, tol].iter().all(|v| v.is_finite()) || tol < 0.0 || hi <= lo {
        return false;
    }
    if chains.len() != 4 || chains.iter().any(|c| c.is_empty()) {
        return false;
    }
    let mut sorted_assignments = high_assignments.to_vec();
    sorted_assignments.sort_unstable();
    if sorted_assignments != [0, 1, 2, 3] {
        return false;
    }
    let flattened: Vec<usize> = chains.iter().flatten().copied().collect();
    let required: BTreeSet<usize> = flattened.iter().copied().collect();
    if flattened.len() != required.len() {
        return false;
    }
    if intervals.keys().copied().collect::<BTreeSet<_>>() != required {
        return false;
    }
    if intervals
        .values()
        .any(|&(a, b)| !a.is_finite() || !b.is_finite() || b <= a)
    {
        return false;
    }
    if edges
        .iter()
        .any(|(l, r)| !required.contains(l) || !required.contains(r))
    {
        return false;
    }
    for chain in chains {
        let mut ordered: Vec<(f64, f64)> = chain.iter().map(|p| intervals[p]).collect();
        ordered.sort_by(|a, b| py::tuple_order(&[a.0, a.1], &[b.0, b.1]));
        let mut cursor = lo;
        for (start, end) in ordered {
            if start > cursor + tol || start < cursor - tol || end <= start {
                return false;
            }
            cursor = end;
        }
        if (cursor - hi).abs() > tol {
            return false;
        }
        let chain_edges: Vec<&(usize, usize)> = edges
            .iter()
            .filter(|(l, r)| chain.contains(l) && chain.contains(r))
            .collect();
        if chain.len() == 1 {
            if !chain_edges.is_empty() {
                return false;
            }
            continue;
        }
        if chain_edges.len() != chain.len() - 1 {
            return false;
        }
        let mut degrees: BTreeMap<usize, usize> = chain.iter().map(|&p| (p, 0)).collect();
        for &&(l, r) in &chain_edges {
            if l == r {
                return false;
            }
            *degrees.get_mut(&l).unwrap() += 1;
            *degrees.get_mut(&r).unwrap() += 1;
        }
        if degrees.values().any(|&d| d > 2) || degrees.values().filter(|&&d| d == 1).count() != 2 {
            return false;
        }
    }
    true
}
