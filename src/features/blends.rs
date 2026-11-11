//! Complete rolling-ball blend paths (`quiddity.blends`).
//!
//! A straight path is a native cylindrical chain ([`super::blend_view`]) sprung from two
//! supports; a circular path is a complete ring of native torus faces sprung smoothly from one
//! plane and one coaxial cylinder. Each carries one radius and one proved material side:
//! `convex` (an external round) or `concave` (an internal one). Every rolling-surface face is
//! defining evidence.

use std::cmp::Ordering;
use std::collections::{BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use super::Context;
use super::analytic_surfaces::{SurfaceKind, equivalent_parameters};
use super::blend_view::{
    BlendChain, BlendGraph, Half, SmoothSide, closed_in_face, edge_occurrences, face_area,
    ownership, shared_occurrences,
};
use super::cylinders::{canonical_axis_direction, coaxial_axis_lines};
use super::evidence::{self, EvidenceError, Occurrence, common_valid_solid};
use super::placed::{Grid, Placed};
use super::policy::length_tol;
use crate::kernel::brep::Part;
use crate::kernel::geom::{self, SMOOTH_ARC_GAP, Surface, V3};
use crate::kernel::py;

/// One straight rolling path: a point on the cylinder axis (the area centre projected onto
/// it) and the canonical axis direction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct StraightBlendPath {
    pub at: Placed<V3>,
    pub direction: V3,
}

/// One complete circular rolling path: the torus centre-line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CircularBlendPath {
    pub center: Placed<V3>,
    pub normal: V3,
    pub radius: f64,
}

/// `BlendPath`: serialised untagged, as the Python dataclasses are.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum BlendPath {
    Straight(StraightBlendPath),
    Circular(CircularBlendPath),
}

/// `Blend`: one complete rolling-ball occurrence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Blend {
    pub radius: f64,
    pub side: String,
    pub path: BlendPath,
}

/// `_canonical_direction`: canonical along the first dominant component.
fn canonical_direction(d: V3) -> V3 {
    canonical_axis_direction(py::first_max(&d, |c| c.abs()), d)
}

/// `_path_sort_key`, then radius and side.
fn blend_order(a: &Blend, b: &Blend) -> Ordering {
    let key = |p: &BlendPath| match p {
        BlendPath::Straight(s) => ("straight", [*s.at, s.direction].concat()),
        BlendPath::Circular(c) => ("circular", [&c.center[..], &c.normal, &[c.radius]].concat()),
    };
    let ((ka, va), (kb, vb)) = (key(&a.path), key(&b.path));
    ka.cmp(kb)
        .then_with(|| py::tuple_order(&va, &vb))
        .then_with(|| py::order(a.radius, b.radius))
        .then_with(|| a.side.cmp(&b.side))
}

fn side_name(side: SmoothSide) -> &'static str {
    match side {
        SmoothSide::Convex => "convex",
        SmoothSide::Concave => "concave",
        other => unreachable!("a chain's side is proved convex or concave, not {other:?}"),
    }
}

/// `_NativeTorus`, at full precision.
#[derive(Clone, Copy, Debug)]
struct NativeTorus {
    center: V3,
    normal: V3,
    major: f64,
    minor: f64,
}

/// `_native_torus`.
fn native_torus(part: &Part, face: usize) -> Option<NativeTorus> {
    let Surface::Torus {
        frame,
        major,
        minor,
    } = part.faces[face].surface
    else {
        return None;
    };
    let finite = frame.origin.iter().chain(&frame.z).all(|v| v.is_finite())
        && major.is_finite()
        && minor.is_finite();
    (finite && major > 0.0 && minor > 0.0).then_some(NativeTorus {
        center: frame.origin,
        normal: frame.z,
        major,
        minor,
    })
}

/// `_same_torus`.
fn same_torus(a: &NativeTorus, b: &NativeTorus, local: f64) -> bool {
    let tolerance = length_tol(local, 1e-9);
    py::dist(&a.center, &b.center) <= tolerance
        && 1.0 - py::dot(&a.normal, &b.normal).abs() <= SMOOTH_ARC_GAP
        && (a.major - b.major).abs() <= tolerance
        && (a.minor - b.minor).abs() <= tolerance
}

/// `_toroidal_components`: native tori joined smoothly, each the same torus as its first face.
fn toroidal_components(graph: &BlendGraph<'_>) -> Vec<(Vec<usize>, NativeTorus)> {
    let part = graph.part;
    let facts: Vec<Option<NativeTorus>> = (0..part.faces.len())
        .map(|f| native_torus(part, f))
        .collect();
    let mut pending: BTreeSet<usize> = (0..facts.len()).filter(|&f| facts[f].is_some()).collect();
    let mut found = Vec::new();
    while let Some(first) = pending.pop_first() {
        let torus = facts[first].unwrap();
        let local = py_min3(torus.major, torus.minor, face_area(part, first).sqrt());
        let mut component = vec![first];
        let mut queue = VecDeque::from([first]);
        while let Some(current) = queue.pop_front() {
            for n in graph.neighbours(current) {
                if pending.contains(&n)
                    && graph.smooth(current, n)
                    && same_torus(&torus, &facts[n].unwrap(), local)
                {
                    pending.remove(&n);
                    component.push(n);
                    queue.push_back(n);
                }
            }
        }
        component.sort_unstable();
        found.push((component, torus));
    }
    found
}

fn py_min3(a: f64, b: f64, c: f64) -> f64 {
    let v = [a, b, c];
    v[py::first_min(&v, |x| *x)]
}

/// `_support_region` (blends): smooth neighbours from *first*, outside *excluded*, with the
/// same effective analytic surface; a face with none is a region of its own.
fn support_region(graph: &BlendGraph<'_>, first: usize, excluded: &[usize]) -> Vec<usize> {
    let part = graph.part;
    let Some(initial) = graph.effective(first) else {
        return vec![first];
    };
    let mut found = BTreeSet::from([first]);
    let mut queue = VecDeque::from([first]);
    while let Some(current) = queue.pop_front() {
        for n in graph.neighbours(current) {
            if found.contains(&n) || excluded.contains(&n) || !graph.smooth(current, n) {
                continue;
            }
            let local = face_area(part, first).sqrt().min(face_area(part, n).sqrt());
            let same = graph.effective(n).is_some_and(|fact| {
                fact.kind == initial.kind
                    && equivalent_parameters(
                        initial.kind,
                        &initial.parameters,
                        &fact.parameters,
                        local,
                    ) == Some(true)
            });
            if same {
                found.insert(n);
                queue.push_back(n);
            }
        }
    }
    found.into_iter().collect()
}

/// `_covers_complete_circle`: the faces' u ranges (about the torus axis) cover a whole turn.
fn covers_complete_circle(part: &Part, nodes: &[usize]) -> bool {
    use std::f64::consts::TAU;
    let mut intervals: Vec<(f64, f64)> = Vec::new();
    for &node in nodes {
        let Some((u0, u1, _, _)) = part.uv_bounds(node) else {
            return false;
        };
        let span = u1 - u0;
        if !u0.is_finite() || !span.is_finite() || span <= 0.0 {
            return false;
        }
        if span >= TAU - 1e-9 {
            return true;
        }
        let start = py::modulo(u0, TAU);
        let end = start + span;
        if end <= TAU {
            intervals.push((start, end));
        } else {
            intervals.extend([(start, TAU), (0.0, end - TAU)]);
        }
    }
    intervals.sort_by(|a, b| py::tuple_order(&[a.0, a.1], &[b.0, b.1]));
    let (mut covered, mut end) = (0.0, 0.0);
    for (start, stop) in intervals {
        if start > end + 1e-9 {
            return false;
        }
        if stop > end {
            covered += stop - start.max(end);
            end = stop;
        }
    }
    end >= TAU - 1e-9 && covered >= TAU - 1e-9
}

/// `_torus_side`: whether the face's outward normal points away from the tube's centre-line
/// (convex) or toward it (concave), agreed at three interior samples.
fn torus_side(part: &Part, face: usize, torus: &NativeTorus) -> Option<&'static str> {
    let (u0, u1, v0, v1) = part.uv_bounds(face)?;
    let surface = &part.faces[face].surface;
    let mut signs = BTreeSet::new();
    for fraction in [0.25, 0.5, 0.75] {
        let (u, v) = (u0 + fraction * (u1 - u0), v0 + fraction * (v1 - v0));
        let point = surface.value(u, v);
        let (du, dv) = surface.partials(u, v);
        let normal = geom::cross(du, dv);
        let magnitude = geom::norm(normal);
        if !(magnitude.is_finite() && magnitude > 0.0) {
            return None;
        }
        let sign = if part.faces[face].reversed { -1.0 } else { 1.0 };
        let normal = geom::scale(normal, sign / magnitude);
        let relative = geom::sub(point, torus.center);
        let along = py::fsum((0..3).map(|i| relative[i] * torus.normal[i]));
        let radial: V3 = [0, 1, 2].map(|i| relative[i] - along * torus.normal[i]);
        let radial_length = py::hypot(&radial);
        if !(radial_length.is_finite() && radial_length > 0.0) {
            return None;
        }
        let tube_center: V3 =
            [0, 1, 2].map(|i| torus.center[i] + torus.major * radial[i] / radial_length);
        let minor = geom::sub(point, tube_center);
        let dot = py::fsum((0..3).map(|i| minor[i] * normal[i]));
        if !dot.is_finite() || dot.abs() <= length_tol(torus.minor, 1e-9) {
            return None;
        }
        signs.insert(dot > 0.0);
    }
    (signs.len() == 1).then(|| {
        if signs.contains(&true) {
            "convex"
        } else {
            "concave"
        }
    })
}

/// `_circular_proposal`: one complete toroidal edge path, or `None`.
fn circular_proposal(
    graph: &BlendGraph<'_>,
    component: &[usize],
    torus: &NativeTorus,
) -> Option<Occurrence<Blend>> {
    let part = graph.part;
    if component.is_empty() || !covers_complete_circle(part, component) {
        return None;
    }
    let solid = common_valid_solid(part, component)?;
    let mut accounted: BTreeSet<Half> = BTreeSet::new();
    let mut cache: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    let mut support_sets: BTreeSet<Vec<usize>> = BTreeSet::new();
    let mut sides = BTreeSet::new();
    for &node in component {
        sides.insert(torus_side(part, node, torus)?);
        for n in graph.neighbours(node) {
            let occurrences = shared_occurrences(part, node, n);
            if occurrences.is_empty()
                || occurrences
                    .iter()
                    .any(|o| ownership(part, o) != Some(solid))
            {
                return None;
            }
            accounted.extend(occurrences.iter().flat_map(|o| [o.halves.0, o.halves.1]));
            if !graph.smooth(node, n) {
                return None;
            }
            if component.contains(&n) {
                continue;
            }
            let region = match cache.get(&n) {
                Some(r) => r.clone(),
                None => {
                    let r = support_region(graph, n, component);
                    for &m in &r {
                        cache.insert(m, r.clone());
                    }
                    r
                }
            };
            support_sets.insert(region);
        }
        for half in edge_occurrences(part, node) {
            if !accounted.contains(&half) && !closed_in_face(part, half.edge, node) {
                return None;
            }
        }
    }
    if sides.len() != 1 || support_sets.len() != 2 {
        return None;
    }
    if support_sets.iter().any(|support| {
        let faces: Vec<usize> = component.iter().chain(support).copied().collect();
        common_valid_solid(part, &faces) != Some(solid)
    }) {
        return None;
    }
    let (mut plane, mut cylinder) = (None, None);
    for region in &support_sets {
        if region.iter().any(|&f| graph.effective(f).is_none()) {
            return None;
        }
        let first = graph.effective(region[0]).unwrap();
        let slot = match first.kind {
            SurfaceKind::Plane => &mut plane,
            SurfaceKind::Cylinder => &mut cylinder,
            _ => return None,
        };
        if slot.is_some() {
            return None;
        }
        *slot = Some(first);
    }
    let (plane, cylinder) = (plane?, cylinder?);
    let c = &cylinder.parameters;
    if 1.0 - py::dot(&plane.parameters[..3], &torus.normal).abs() > SMOOTH_ARC_GAP
        || !coaxial_axis_lines(
            torus.center,
            torus.normal,
            [c[0], c[1], c[2]],
            [c[3], c[4], c[5]],
            length_tol(torus.major.max(c[6]), 1e-9),
        )
    {
        return None;
    }
    let path = CircularBlendPath {
        center: Placed::new(torus.center, Grid::Decimals(3)),
        normal: canonical_direction(torus.normal),
        radius: py::round_to3(torus.major),
    };
    Some(Occurrence {
        record: Blend {
            radius: py::round_to3(torus.minor),
            side: sides.pop_first().unwrap().into(),
            path: BlendPath::Circular(path),
        },
        defining: component.to_vec(),
        context: Vec::new(),
    })
}

/// `_parallel_planar_supports`: both supports prove parallel planes where the chain springs
/// from them, so there is no edge for the chain to round (a slot's circular end).
fn parallel_planar_supports(graph: &BlendGraph<'_>, chain: &BlendChain) -> bool {
    let mut normals: Vec<Vec<f64>> = Vec::new();
    for support in &chain.supports {
        let spring: BTreeSet<usize> = chain
            .spring_arcs
            .iter()
            .flat_map(|a| [a.endpoints.0, a.endpoints.1])
            .filter(|n| support.contains(n))
            .collect();
        let mut support_normals = Vec::new();
        for &node in &spring {
            match graph.effective(node) {
                Some(f) if f.kind == SurfaceKind::Plane => {
                    support_normals.push(f.parameters[..3].to_vec())
                }
                _ => return false,
            }
        }
        let Some(first) = support_normals.first().cloned() else {
            return false;
        };
        if support_normals[1..]
            .iter()
            .any(|o| 1.0 - py::dot(&first, o).abs() > SMOOTH_ARC_GAP)
        {
            return false;
        }
        normals.push(first);
    }
    1.0 - py::dot(&normals[0], &normals[1]).abs() <= SMOOTH_ARC_GAP
}

/// `_straight_path_anchor`: the faces' area centre projected onto the cylinder axis.
fn straight_path_anchor(part: &Part, nodes: &[usize], parameters: &[f64]) -> Option<V3> {
    let mut weighted = Vec::new();
    for &node in nodes {
        let m = part.face_moments(node)?;
        if !(m.area.is_finite() && m.area > 0.0) || m.centroid.iter().any(|c| !c.is_finite()) {
            return None;
        }
        weighted.push((m.area, m.centroid));
    }
    let total = py::fsum(weighted.iter().map(|(a, _)| *a));
    if !(total.is_finite() && total > 0.0) {
        return None;
    }
    let centre: V3 = [0, 1, 2].map(|i| py::fsum(weighted.iter().map(|(a, p)| a * p[i])) / total);
    let (origin, direction) = (&parameters[..3], &parameters[3..6]);
    let relative: V3 = [0, 1, 2].map(|i| centre[i] - origin[i]);
    let along = py::fsum((0..3).map(|i| relative[i] * direction[i]));
    Some([0, 1, 2].map(|i| origin[i] + along * direction[i]))
}

/// `_proposal`: a straight path for one chain.
fn straight_proposal(graph: &BlendGraph<'_>, chain: &BlendChain) -> Option<Occurrence<Blend>> {
    if chain.side == SmoothSide::Concave && parallel_planar_supports(graph, chain) {
        return None;
    }
    let nodes = &chain.blend_nodes;
    let fact = graph.effective(*nodes.first()?)?;
    if fact.parameters.len() != 7 {
        return None;
    }
    let p = &fact.parameters;
    let direction = canonical_direction([p[3], p[4], p[5]]);
    let anchor = straight_path_anchor(graph.part, nodes, p)?;
    Some(Occurrence {
        record: Blend {
            radius: py::round_to3(chain.radius),
            side: side_name(chain.side).into(),
            path: BlendPath::Straight(StraightBlendPath {
                at: Placed::new(anchor, Grid::Decimals(3)),
                direction,
            }),
        },
        defining: nodes.clone(),
        context: Vec::new(),
    })
}

internal_entry! {
/// `recognise_blends`.
pub fn recognise_blends(part: &Part) -> Vec<Blend> {
    super::records(discover(&Context::new(part)))
}
}

internal_entry! {
/// `_discover_blends`: straight then circular proposals, sorted by path, radius and side.
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<Blend>> {
    let graph = BlendGraph::new(ctx.part);
    let mut found: Vec<Occurrence<Blend>> = graph
        .chains()
        .iter()
        .filter_map(|chain| straight_proposal(&graph, chain))
        .collect();
    found.extend(
        toroidal_components(&graph)
            .iter()
            .filter_map(|(component, torus)| circular_proposal(&graph, component, torus)),
    );
    found.sort_by(|a, b| blend_order(&a.record, &b.record));
    found
}
}

internal_entry! {
/// The evidence path (`_discover_blends` with a writer): every blend's faces on one valid
/// solid, or the run refused.
pub fn discover_verified(ctx: &Context<'_>) -> Result<Vec<Occurrence<Blend>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx))
}
}
