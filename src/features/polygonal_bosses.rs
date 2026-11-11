//! Regular square and hexagonal bosses, and hexagonal whole-stock prisms
//! (`quiddity.polygonal_bosses`).
//!
//! A boss is a closed ring of four or six planar side faces along a principal axis, evenly
//! turned and opposed in pairs at one across-flats distance, outward-facing, and terminated at
//! both ends by one cap each, both facing the same way: the support face it stands on and its
//! top. Where a side ring is rounded at its corners, the convex single-face blend chains
//! between consecutive sides count as the sides' shared edges. Stock is the exact eight-face
//! hexagonal prism: the whole part is one solid made of the ring and its two opposite caps.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::Context;
use super::evidence::{EvidenceError, Occurrence};
use super::experimental_geometry::{GeometryGraph, SurfaceKind};
use super::geometry_evidence::GeometryEvidence;
use super::placed::{Grid, Placed};
use super::policy::AXIS_ALIGNED_COS;
use crate::kernel::brep::Part;
use crate::kernel::geom::V3;
use crate::kernel::py;

/// `_TOL`: a minimum-evidence threshold, deliberately absolute (Python's ADR 0008); also the
/// minimum boss height and support span.
const TOL: f64 = 0.2;

/// `_SIDE_VERTICAL_COS`: a side face's normal has essentially no component along the axis.
const SIDE_VERTICAL_COS: f64 = 0.02;

/// The axes in the order they are searched, with their names.
const AXES: [(usize, &str); 3] = [(2, "z"), (0, "x"), (1, "y")];

/// `PolygonalBoss` and `PolygonalStock`, which share their fields: a regular prism along
/// principal `axis` (`"x"`, `"y"` or `"z"`), `side_count` 4 or 6 (stock: 6), its `center`,
/// `across_flats`, the axial `base` and `top`, each side's outward heading in ring order and a
/// point on each side face (its centroid).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PolygonalPrism {
    pub axis: String,
    pub center: Placed<V3>,
    pub side_count: usize,
    pub across_flats: f64,
    pub base: Placed<f64>,
    pub top: Placed<f64>,
    pub flat_directions: Vec<V3>,
    pub flat_centres: Vec<Placed<V3>>,
}

/// A polygonal boss attached to a support face.
pub type PolygonalBoss = PolygonalPrism;
/// A whole solid proved to be a regular hexagonal prism.
pub type PolygonalStock = PolygonalPrism;

impl PolygonalPrism {
    /// `height` / `length`: the axial length.
    pub fn length(&self) -> f64 {
        *self.top - *self.base
    }
}

/// Options named as `recognise_polygonal_bosses`' and `recognise_polygonal_stock`'s keyword
/// arguments.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct PolygonalOptions {
    /// The span and fit threshold; 0.2 mm when `None`.
    pub tol: Option<f64>,
    /// How far, in radians, the ring may depart from regular.
    pub angle_tol: f64,
}

impl Default for PolygonalOptions {
    fn default() -> Self {
        PolygonalOptions {
            tol: None,
            angle_tol: 2f64.to_radians(),
        }
    }
}

/// The dataclass order: field by field, tuples lexicographically.
fn record_order(a: &PolygonalPrism, b: &PolygonalPrism) -> Ordering {
    let points = |x: &[V3], y: &[V3]| {
        x.iter()
            .zip(y)
            .map(|(p, q)| py::tuple_order(p, q))
            .find(|o| o.is_ne())
            .unwrap_or_else(|| x.len().cmp(&y.len()))
    };
    a.axis
        .cmp(&b.axis)
        .then_with(|| py::tuple_order(&a.center[..], &b.center[..]))
        .then_with(|| a.side_count.cmp(&b.side_count))
        .then_with(|| py::order(a.across_flats, b.across_flats))
        .then_with(|| py::order(*a.base, *b.base))
        .then_with(|| py::order(*a.top, *b.top))
        .then_with(|| points(&a.flat_directions, &b.flat_directions))
        .then_with(|| {
            let centres =
                |p: &PolygonalPrism| p.flat_centres.iter().map(|c| **c).collect::<Vec<_>>();
            points(&centres(a), &centres(b))
        })
}

/// `_PolygonalProposal`: the record, its sides in ring order, its two caps, and (a boss) the
/// cap at its free end.
struct Proposal {
    record: PolygonalPrism,
    sides: Vec<usize>,
    lower_cap: usize,
    upper_cap: usize,
    terminal_cap: Option<usize>,
}

/// `_rounded`: rounded, a negative zero made positive.
fn rounded(value: f64, digits: usize) -> f64 {
    py::without_negative_zero(py::round_to(value, digits))
}

/// `_connected_components`: *items* grouped where *joined* links them, each component
/// ascending, in order of its least item.
fn connected_components(items: &[usize], joined: impl Fn(usize, usize) -> bool) -> Vec<Vec<usize>> {
    let mut unseen: BTreeSet<usize> = items.iter().copied().collect();
    let mut components = Vec::new();
    while let Some(first) = unseen.pop_first() {
        let mut connected = vec![first];
        let mut frontier = vec![first];
        while let Some(current) = frontier.pop() {
            let attached: Vec<usize> = unseen
                .iter()
                .copied()
                .filter(|&o| joined(current, o))
                .collect();
            for &o in &attached {
                unseen.remove(&o);
            }
            connected.extend(&attached);
            frontier.extend(attached);
        }
        connected.sort_unstable();
        components.push(connected);
    }
    components
}

/// `_cap_coordinate`: the axial coordinate of *face* if it can terminate the ring: planar,
/// facing squarely along the axis the required way, at one axial coordinate, on the right side
/// of the wall.
fn cap_coordinate(
    graph: &GeometryGraph<'_>,
    face: usize,
    tol: f64,
    axis: usize,
    positive: bool,
    lower_than: Option<f64>,
    higher_than: Option<f64>,
) -> Option<f64> {
    if !graph.is_planar(face) {
        return None;
    }
    let normal = graph.normal(face)?;
    if (positive && normal[axis] < AXIS_ALIGNED_COS)
        || (!positive && normal[axis] > -AXIS_ALIGNED_COS)
    {
        return None;
    }
    let (lo, hi) = graph.bounds(face)[axis];
    if hi - lo > tol {
        return None;
    }
    let coordinate = (lo + hi) / 2.0;
    if lower_than.is_some_and(|l| coordinate > l + tol)
        || higher_than.is_some_and(|h| coordinate < h - tol)
    {
        return None;
    }
    Some(coordinate)
}

/// `_common_cap`: the one cap every side of the ring reaches through exactly one neighbour each,
/// and the face (and coordinate) that terminates it, or `None` when any choice is ambiguous.
#[allow(clippy::too_many_arguments)]
fn common_cap(
    component: &[usize],
    graph: &GeometryGraph<'_>,
    tol: f64,
    axis: usize,
    upper: bool,
    positive: bool,
    wall_lo: f64,
    wall_hi: f64,
) -> Option<(usize, f64)> {
    let mut boundary = BTreeSet::new();
    for &side in component {
        let choices: Vec<usize> = graph
            .neighbours(side)
            .into_iter()
            .filter(|o| !component.contains(o))
            .filter(|&o| {
                let (lo, hi) = graph.bounds(o)[axis];
                if upper {
                    (lo - wall_hi).abs() <= tol
                } else {
                    (hi - wall_lo).abs() <= tol
                }
            })
            .collect();
        let &[choice] = choices.as_slice() else {
            return None;
        };
        boundary.insert(choice);
    }
    let candidates: Vec<usize> = if boundary.len() == 1 {
        boundary.iter().copied().collect()
    } else {
        let mut faces = boundary.iter();
        let mut common: BTreeSet<usize> = graph
            .neighbours(*faces.next().expect("a ring has sides"))
            .into_iter()
            .collect();
        for &f in faces {
            let next: BTreeSet<usize> = graph.neighbours(f).into_iter().collect();
            common.retain(|n| next.contains(n));
        }
        common
            .into_iter()
            .filter(|n| !component.contains(n) && !boundary.contains(n))
            .collect()
    };
    let (lower_than, higher_than) = if upper {
        (None, Some(wall_hi))
    } else {
        (Some(wall_lo), None)
    };
    let caps: Vec<(usize, f64)> = candidates
        .into_iter()
        .filter_map(|n| {
            cap_coordinate(graph, n, tol, axis, positive, lower_than, higher_than).map(|c| (n, c))
        })
        .collect();
    match caps.as_slice() {
        &[cap] => Some(cap),
        _ => None,
    }
}

/// `_principal_side_faces`: planar faces whose normal is perpendicular to the axis and whose
/// axial span is longer than *tol*.
fn principal_side_faces(graph: &GeometryGraph<'_>, tol: f64, axis: usize) -> Vec<usize> {
    graph
        .faces()
        .iter()
        .copied()
        .filter(|&f| {
            graph.is_planar(f)
                && graph
                    .normal(f)
                    .is_some_and(|n| n[axis].abs() <= SIDE_VERTICAL_COS)
                && {
                    let (lo, hi) = graph.bounds(f)[axis];
                    hi - lo > tol
                }
        })
        .collect()
}

/// An unordered face pair.
fn pair(a: usize, b: usize) -> (usize, usize) {
    (a.min(b), a.max(b))
}

/// `_support_cycle_indices`: the indices of *pairs* forming disjoint cycles of exactly
/// *side_count* distinct pairs over *side_count* supports, each support in exactly two.
fn support_cycle_indices(pairs: &[(usize, usize)], side_count: usize) -> Vec<usize> {
    let mut remaining: BTreeSet<usize> = (0..pairs.len()).collect();
    let mut selected = Vec::new();
    while let Some(seed) = remaining.pop_first() {
        let mut component = BTreeSet::from([seed]);
        let mut supports = BTreeSet::from([pairs[seed].0, pairs[seed].1]);
        let mut changed = true;
        while changed {
            changed = false;
            for at in remaining.clone() {
                let (l, r) = pairs[at];
                if supports.contains(&l) || supports.contains(&r) {
                    remaining.remove(&at);
                    component.insert(at);
                    supports.extend([l, r]);
                    changed = true;
                }
            }
        }
        let component_pairs: Vec<(usize, usize)> = component.iter().map(|&i| pairs[i]).collect();
        let distinct: BTreeSet<&(usize, usize)> = component_pairs.iter().collect();
        if component.len() != side_count
            || supports.len() != side_count
            || distinct.len() != side_count
        {
            continue;
        }
        if supports.iter().any(|s| {
            component_pairs
                .iter()
                .filter(|(l, r)| l == s || r == s)
                .count()
                != 2
        }) {
            continue;
        }
        selected.extend(component);
    }
    selected
}

/// `contains_cycle`: at least four pairs, and some component of the faces they join has four
/// or six faces with at least as many pairs inside it.
fn contains_cycle(pairs: &[(usize, usize)]) -> bool {
    if pairs.len() < 4 {
        return false;
    }
    let supports: Vec<usize> = pairs
        .iter()
        .flat_map(|&(l, r)| [l, r])
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    connected_components(&supports, |l, r| pairs.contains(&pair(l, r)))
        .iter()
        .any(|component| {
            matches!(component.len(), 4 | 6)
                && pairs
                    .iter()
                    .filter(|(l, r)| component.contains(l) && component.contains(r))
                    .count()
                    >= component.len()
        })
}

/// `_polygonal_boss_blend_bridges`: the side pairs joined by a convex single-face blend chain
/// between two equal-span planar walls, where such chains close unambiguous four- or six-support
/// cycles. Each is checked against the bridge the collapsed view draws for it.
fn blend_bridges(
    graph: &GeometryGraph<'_>,
    sides: &[usize],
    tol: f64,
    axis: usize,
) -> Result<BTreeSet<(usize, usize)>, super::discovery::DiscoveryRefusal> {
    let same_span = |a: usize, b: usize| {
        let (sa, sb) = (graph.bounds(a)[axis], graph.bounds(b)[axis]);
        (sa.0 - sb.0).abs() <= tol && (sa.1 - sb.1).abs() <= tol
    };
    let mut possible: Vec<(usize, (usize, usize))> = Vec::new();
    for &node in graph.faces() {
        if sides.contains(&node) {
            continue;
        }
        let supports: BTreeSet<usize> = graph
            .neighbours(node)
            .into_iter()
            .filter(|n| sides.contains(n))
            .collect();
        let &[left, right] = supports.iter().copied().collect::<Vec<_>>().as_slice() else {
            continue;
        };
        if !same_span(left, right) {
            continue;
        }
        possible.push((node, (left, right)));
    }
    let possible_pairs: Vec<(usize, usize)> = possible.iter().map(|&(_, p)| p).collect();
    if !contains_cycle(&possible_pairs) {
        return Ok(BTreeSet::new());
    }
    let cylindrical: Vec<(usize, usize)> = possible
        .iter()
        .filter(|&&(node, _)| {
            graph
                .surface_fact(node)
                .is_some_and(|f| f.kind == SurfaceKind::Cylinder)
        })
        .map(|&(_, p)| p)
        .collect();
    if !contains_cycle(&cylindrical) {
        return Ok(BTreeSet::new());
    }
    let plane = |f: usize| {
        graph
            .surface_fact(f)
            .is_some_and(|s| s.kind == SurfaceKind::Plane)
    };
    let wall = |f: usize| {
        graph
            .normal(f)
            .is_some_and(|n| n[axis].abs() <= SIDE_VERTICAL_COS)
    };
    let mut eligible: Vec<(usize, (usize, usize))> = Vec::new();
    for (index, fact) in graph.blend_facts().iter().enumerate() {
        if fact.side != super::blend_view::SmoothSide::Convex || fact.blend_faces.len() != 1 {
            continue;
        }
        let [l, r] = &fact.supports;
        let (&[left], &[right]) = (l.as_slice(), r.as_slice()) else {
            continue;
        };
        if left == right || !plane(left) || !plane(right) || !wall(left) || !wall(right) {
            continue;
        }
        if !same_span(left, right) {
            continue;
        }
        eligible.push((index, pair(left, right)));
    }
    let eligible_pairs: Vec<(usize, usize)> = eligible.iter().map(|&(_, p)| p).collect();
    let selected: BTreeSet<usize> = [4, 6]
        .into_iter()
        .flat_map(|count| support_cycle_indices(&eligible_pairs, count))
        .collect();
    if selected.is_empty() {
        return Ok(BTreeSet::new());
    }
    let chains: Vec<usize> = selected.iter().map(|&at| eligible[at].0).collect();
    let facts = graph.blend_facts();
    let faces: BTreeSet<_> = chains
        .iter()
        .flat_map(|&i| {
            facts[i]
                .blend_faces
                .iter()
                .chain(facts[i].supports.iter().flatten())
        })
        .copied()
        .collect();
    let refuse = |reason| super::discovery::DiscoveryRefusal::new(reason, faces.iter().copied());
    // Support cycles alone do not prove that their rolling faces and original arcs are
    // disjoint. Keep a collapse refusal as source evidence instead of trusting that graph.
    let bridges = graph.collapsed_bridges(&chains).map_err(refuse)?;
    for &at in &selected {
        let (chain, support_pair) = eligible[at];
        let arcs: Vec<_> = bridges
            .iter()
            .filter(|b| pair(b.supports.0, b.supports.1) == support_pair)
            .collect();
        let &[bridge] = arcs.as_slice() else {
            return Err(refuse(
                "selected Polygonal Boss blend chain has no unique logical bridge",
            ));
        };
        let fact = &facts[chain];
        let mut expected: Vec<usize> = fact
            .blend_faces
            .iter()
            .chain(fact.supports.iter().flatten())
            .copied()
            .collect();
        expected.sort_unstable();
        let mut boundary = fact.boundary.clone();
        boundary.sort_by_key(super::blend_view::Shared::key);
        if bridge.faces != expected || bridge.boundary != boundary {
            return Err(refuse(
                "selected Polygonal Boss bridge lost original provenance",
            ));
        }
    }
    Ok(selected.iter().map(|&at| eligible[at].1).collect())
}

/// `_regular_ring_order`: the ring ordered by heading angle, or `None` unless the headings are
/// evenly spaced and each faces directly away from the one opposite.
fn regular_ring_order(
    component: &[usize],
    headings: &dyn Fn(usize) -> (f64, f64),
    angle_tol: f64,
) -> Option<Vec<usize>> {
    let n = component.len();
    let angle = |key: usize| {
        let (across, along) = headings(key);
        along.atan2(across)
    };
    let mut ordered = component.to_vec();
    // Python's sort is stable; ties are equal angles, which the gap test refuses.
    ordered.sort_by(|&a, &b| py::order(angle(a), angle(b)));
    let turn = 2.0 * std::f64::consts::PI;
    let angles: Vec<f64> = ordered
        .iter()
        .map(|&i| py::modulo(angle(i), turn))
        .collect();
    let expected = turn / n as f64;
    if (0..n)
        .any(|i| (py::modulo(angles[(i + 1) % n] - angles[i], turn) - expected).abs() > angle_tol)
    {
        return None;
    }
    let opposite = n / 2;
    if (0..opposite).any(|i| {
        let (a, b) = (headings(ordered[i]), headings(ordered[i + opposite]));
        a.0 * b.0 + a.1 * b.1 > -angle_tol.cos()
    }) {
        return None;
    }
    Some(ordered)
}

/// `_ring_profile`: the axis position `(cx, cy)` (least-squares meet of the opposed pairs'
/// midplanes) and the across-flats, or `None` unless every side stands out the same distance
/// and every opposed pair is the same distance apart.
fn ring_profile(
    headings: &[(f64, f64)],
    centres: &[(f64, f64)],
    tol: f64,
) -> Option<(f64, f64, f64)> {
    let n = headings.len();
    let opposite = n / 2;
    let offsets: Vec<f64> = headings
        .iter()
        .zip(centres)
        .map(|(h, p)| h.0 * p.0 + h.1 * p.1)
        .collect();
    let midplanes: Vec<(f64, f64, f64)> = (0..opposite)
        .map(|i| {
            (
                headings[i].0,
                headings[i].1,
                (offsets[i] - offsets[i + opposite]) / 2.0,
            )
        })
        .collect();
    let sxx = py::sum(midplanes.iter().map(|m| m.0 * m.0));
    let sxy = py::sum(midplanes.iter().map(|m| m.0 * m.1));
    let syy = py::sum(midplanes.iter().map(|m| m.1 * m.1));
    let bx = py::sum(midplanes.iter().map(|m| m.0 * m.2));
    let by = py::sum(midplanes.iter().map(|m| m.1 * m.2));
    let determinant = sxx * syy - sxy * sxy;
    // Normals that passed the regular-ring gate span the plane.
    let cx = (bx * syy - by * sxy) / determinant;
    let cy = (sxx * by - sxy * bx) / determinant;
    let supports: Vec<f64> = headings
        .iter()
        .zip(&offsets)
        .map(|(h, offset)| offset - h.0 * cx - h.1 * cy)
        .collect();
    if supports.iter().copied().fold(f64::INFINITY, f64::min) <= tol {
        return None; // inward-facing walls describe a recess, not material projecting out
    }
    let across_values: Vec<f64> = (0..opposite)
        .map(|i| supports[i] + supports[i + opposite])
        .collect();
    let across = py::sum(across_values.iter().copied()) / across_values.len() as f64;
    if across_values.iter().any(|v| (v - across).abs() > tol)
        || supports.iter().any(|v| (v - across / 2.0).abs() > tol)
    {
        return None;
    }
    Some((cx, cy, across))
}

/// `_recognise_one`: the regular prisms along one axis among the graph's faces. Python's
/// aggregate run hands a single-solid part's whole graph down and limits only the side faces to
/// the solid's; a graph here is always the solid's own, which differs only where loose faces
/// share edges with the solid (no corpus part or captured call differs by it).
fn recognise_one(
    graph: &GeometryGraph<'_>,
    tol: f64,
    angle_tol: f64,
    whole_stock: bool,
    (axis, axis_name): (usize, &str),
    refusals: &mut Vec<super::discovery::DiscoveryRefusal>,
) -> Vec<Proposal> {
    let part = graph.part;
    let transverse = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let sides = principal_side_faces(graph, tol, axis);
    if sides.len() < if whole_stock { 6 } else { 4 } {
        return Vec::new();
    }
    let bridges = if whole_stock {
        BTreeSet::new()
    } else {
        match blend_bridges(graph, &sides, tol, axis) {
            Ok(bridges) => bridges,
            Err(refusal) => {
                refusals.push(refusal);
                BTreeSet::new()
            }
        }
    };
    let shares_edge =
        |i: usize, j: usize| graph.neighbours(i).contains(&j) || bridges.contains(&pair(i, j));
    let same_span = |i: usize, j: usize| {
        let (si, sj) = (graph.bounds(i)[axis], graph.bounds(j)[axis]);
        (si.0 - sj.0).abs() <= tol && (si.1 - sj.1).abs() <= tol
    };
    let components = connected_components(&sides, |i, j| same_span(i, j) && shares_edge(i, j));
    let mut found = Vec::new();
    for component in components {
        let side_count = component.len();
        if !(side_count == 6 || (!whole_stock && side_count == 4)) {
            continue;
        }
        // Whole stock is the exact prism: one closed solid of this ring and its two caps.
        if whole_stock && graph.len() != side_count + 2 {
            continue;
        }
        if component.iter().any(|&side| {
            component
                .iter()
                .filter(|&&o| o != side && shares_edge(side, o))
                .count()
                != 2
        }) {
            continue;
        }
        // A zero component's sign is the plane's representation, not its geometry, but it moves
        // `atan2` between -π and π and with it where the ring starts: read as +0.0, a side facing
        // straight back along the first transverse axis comes last. Python reads the sign
        // OpenCascade's `normal_at` gives, which is either: over the corpus its rings carry 217
        // exact zeros as -0.0 and 156 as +0.0, and in 61 rings the start moves when -0.0 is read
        // as +0.0, but none of those rings is a record (Python's records are unchanged with every
        // -0.0 read as +0.0). The one record that turns on the sign, nist_ftc_07's squares, has
        // +0.0 in Python and -0.0 in this kernel, and differs without this rule. Verdict
        // equivalent where Python would carry -0.0 on a record's ring: the same sides in the same
        // turn, Python's `flat_directions` and `flat_centres` starting at the backward-facing
        // side and the port's at the next; no corpus or captured record does (the captured
        // calls match with or without this rule).
        let heading = |f: usize| {
            let n = graph.normal(f).expect("side faces have normals");
            (
                py::without_negative_zero(n[transverse.0]),
                py::without_negative_zero(n[transverse.1]),
            )
        };
        let Some(ordered) = regular_ring_order(&component, &heading, angle_tol) else {
            continue;
        };
        // A planar face's centre is its centroid (`Face.center()`); a face the quadrature cannot
        // integrate gives the ring no centres, and the ring is refused rather than guessed.
        let Some(centres) = ordered
            .iter()
            .map(|&f| part.face_moments(f).map(|m| m.centroid))
            .collect::<Option<Vec<V3>>>()
        else {
            continue;
        };
        let headings: Vec<(f64, f64)> = ordered.iter().map(|&f| heading(f)).collect();
        let planar: Vec<(f64, f64)> = centres
            .iter()
            .map(|c| (c[transverse.0], c[transverse.1]))
            .collect();
        let Some((cx, cy, across)) = ring_profile(&headings, &planar, tol) else {
            continue;
        };
        let wall_lo =
            py::sum(component.iter().map(|&f| graph.bounds(f)[axis].0)) / side_count as f64;
        let wall_hi =
            py::sum(component.iter().map(|&f| graph.bounds(f)[axis].1)) / side_count as f64;
        let directions: &[bool] = if whole_stock { &[true] } else { &[true, false] };
        let mut cap_pairs = Vec::new();
        for &positive in directions {
            let cap = |upper: bool| {
                let facing = if whole_stock { upper } else { positive };
                common_cap(
                    &component, graph, tol, axis, upper, facing, wall_lo, wall_hi,
                )
            };
            if let (Some(base), Some(top)) = (cap(false), cap(true)) {
                cap_pairs.push((base, top, positive));
            }
        }
        let &[(base, top, positive)] = cap_pairs.as_slice() else {
            continue;
        };
        if top.1 - base.1 <= tol {
            continue;
        }
        if whole_stock && ((base.1 - wall_lo).abs() > tol || (top.1 - wall_hi).abs() > tol) {
            continue;
        }
        let mut center = [0.0; 3];
        center[transverse.0] = cx;
        center[transverse.1] = cy;
        center[axis] = (base.1 + top.1) / 2.0;
        let flat_direction = |(across, along): (f64, f64)| {
            let mut d = [0.0; 3];
            d[transverse.0] = rounded(across, 3);
            d[transverse.1] = rounded(along, 3);
            d
        };
        found.push(Proposal {
            record: PolygonalPrism {
                axis: axis_name.to_owned(),
                center: Placed::new(center, Grid::CleanDecimals(4)),
                side_count,
                across_flats: rounded(across, 4),
                base: Placed::new(base.1, Grid::CleanDecimals(4)),
                top: Placed::new(top.1, Grid::CleanDecimals(4)),
                flat_directions: headings.iter().map(|&h| flat_direction(h)).collect(),
                flat_centres: centres
                    .iter()
                    .map(|&c| Placed::new(c, Grid::CleanDecimals(3)))
                    .collect(),
            },
            sides: ordered,
            lower_cap: base.0,
            upper_cap: top.0,
            terminal_cap: (!whole_stock).then_some(if positive { top.0 } else { base.0 }),
        });
    }
    found
}

internal_entry! {
/// `recognise_polygonal_bosses`: regular square and hexagonal bosses, each solid on its own,
/// sorted by record.
pub fn recognise_polygonal_bosses(part: &Part, opts: &PolygonalOptions) -> Vec<PolygonalBoss> {
    super::records(discover(&Context::new(part), opts))
}
}

internal_entry! {
/// `_discover_polygonal_bosses`: per solid (or over the whole part when it has none), along z,
/// x then y, sorted by record. Each boss is defined by its side ring; its claim covers its free
/// end's cap as well.
pub fn discover(ctx: &Context<'_>, opts: &PolygonalOptions) -> Vec<Occurrence<PolygonalBoss>> {
    analyse(ctx, opts).accepted
}
}

pub(crate) fn analyse(
    ctx: &Context<'_>,
    opts: &PolygonalOptions,
) -> super::discovery::Analysis<PolygonalBoss> {
    let mut refusals = Vec::new();
    let part = ctx.part;
    let tol = opts.tol.unwrap_or(TOL);
    let graphs: Vec<GeometryGraph<'_>> = if part.solids.is_empty() {
        vec![GeometryGraph::new(part)]
    } else {
        (0..part.solids.len())
            .map(|s| GeometryGraph::for_solid(part, s))
            .collect()
    };
    let mut found: Vec<(Proposal, usize)> = Vec::new();
    for (g, graph) in graphs.iter().enumerate() {
        for axis in AXES {
            found.extend(
                recognise_one(graph, tol, opts.angle_tol, false, axis, &mut refusals)
                    .into_iter()
                    .map(|p| (p, g)),
            );
        }
    }
    found.sort_by(|a, b| record_order(&a.0.record, &b.0.record));
    let accepted = found
        .into_iter()
        .map(|(p, g)| {
            let bridge = GeometryEvidence::new(&graphs[g]);
            let refs = bridge.refs(&p.sides);
            let terminal = p.terminal_cap.expect("a boss keeps its terminal cap");
            let mut constituent = refs.clone();
            constituent.push(terminal);
            bridge.occurrence(p.record, &refs, &constituent)
        })
        .collect();
    super::discovery::Analysis { accepted, refusals }
}

/// Extend a completed inventory's boss claim with its already-consulted free-end cap, after
/// reconciliation. The cap must be the boss's own top: a plane facing along the axis at the
/// boss's `base` or `top`, lying within the side ring's transverse extent, so a wider face the
/// ring merely ends on cannot be claimed.
pub(crate) fn claim_terminal_cap(
    part: &Part,
    record: &PolygonalBoss,
    faces: &mut Vec<usize>,
    constituent: &[usize],
) {
    let Some(axis) = AXES
        .iter()
        .find(|(_, n)| *n == record.axis)
        .map(|&(a, _)| a)
    else {
        return;
    };
    let caps: Vec<usize> = constituent
        .iter()
        .copied()
        .filter(|f| !faces.contains(f))
        .collect();
    let &[cap] = caps.as_slice() else {
        return;
    };
    let tol = TOL;
    let Some(normal) = super::graph::normal(part, cap) else {
        return;
    };
    let cb = part.face_bounds(cap);
    let level = (cb.min[axis] + cb.max[axis]) / 2.;
    if !matches!(
        part.faces[cap].surface,
        crate::kernel::geom::Surface::Plane { .. }
    ) || normal[axis].abs() < AXIS_ALIGNED_COS
        || cb.max[axis] - cb.min[axis] > tol
        || ![*record.base.raw(), *record.top.raw()]
            .iter()
            .any(|&at| (level - at).abs() <= tol)
    {
        return;
    }
    let inside = (0..3).filter(|&k| k != axis).all(|k| {
        let lo = faces
            .iter()
            .map(|&f| part.face_bounds(f).min[k])
            .fold(f64::INFINITY, f64::min);
        let hi = faces
            .iter()
            .map(|&f| part.face_bounds(f).max[k])
            .fold(f64::NEG_INFINITY, f64::max);
        cb.min[k] >= lo - tol && cb.max[k] <= hi + tol
    });
    if inside {
        faces.push(cap);
        faces.sort_unstable();
    }
}

internal_entry! {
/// The evidence path: every boss's side ring, and the ring with its free end's cap, on one
/// valid solid; no side face defining two bosses.
pub fn discover_verified(
    ctx: &Context<'_>,
    opts: &PolygonalOptions,
) -> Result<Vec<Occurrence<PolygonalBoss>>, EvidenceError> {
    let found = discover(ctx, opts);
    let graph = GeometryGraph::new(ctx.part);
    let bridge = GeometryEvidence::new(&graph);
    let mut used: Vec<usize> = Vec::new();
    for o in &found {
        if o.defining.iter().any(|f| used.contains(f)) {
            return Err(EvidenceError::SharedEvidence);
        }
        bridge.validate_defining(&o.defining)?;
        let constituent: Vec<usize> = o.defining.iter().chain(&o.context).copied().collect();
        bridge.validate_defining(&constituent)?;
        used.extend(&o.defining);
    }
    Ok(found)
}
}

internal_entry! {
/// `recognise_polygonal_stock`: one record when the whole part is a regular hexagonal prism.
pub fn recognise_polygonal_stock(part: &Part, opts: &PolygonalOptions) -> Vec<PolygonalStock> {
    super::records(discover_stock(&Context::new(part), opts))
}
}

internal_entry! {
/// `_discover_polygonal_stock`: only a part of one solid and eight faces, the first axis (z, x,
/// y) that proves the prism. The stock is defined by all eight faces.
pub fn discover_stock(
    ctx: &Context<'_>,
    opts: &PolygonalOptions,
) -> Vec<Occurrence<PolygonalStock>> {
    let part = ctx.part;
    if part.solids.len() != 1 || part.faces.len() != 8 {
        return Vec::new();
    }
    let tol = opts.tol.unwrap_or(TOL);
    let graph = GeometryGraph::new(part);
    let mut proposals = Vec::new();
    for axis in AXES {
        proposals = recognise_one(&graph, tol, opts.angle_tol, true, axis, &mut Vec::new());
        if !proposals.is_empty() {
            break;
        }
    }
    proposals.sort_by(|a, b| record_order(&a.record, &b.record));
    let bridge = GeometryEvidence::new(&graph);
    proposals
        .into_iter()
        .map(|p| {
            let mut faces = p.sides.clone();
            faces.extend([p.lower_cap, p.upper_cap]);
            let refs = bridge.refs(&faces);
            bridge.occurrence(p.record, &refs, &refs)
        })
        .collect()
}
}

internal_entry! {
/// The evidence path: the stock's complete eight-face boundary, on one valid solid.
pub fn discover_stock_verified(
    ctx: &Context<'_>,
    opts: &PolygonalOptions,
) -> Result<Vec<Occurrence<PolygonalStock>>, EvidenceError> {
    let found = discover_stock(ctx, opts);
    let graph = GeometryGraph::new(ctx.part);
    let bridge = GeometryEvidence::new(&graph);
    for o in &found {
        // Eight distinct faces of an eight-face part: the whole inventory, by construction.
        assert_eq!(o.defining.len(), graph.len(), "stock owns every face");
        bridge.validate_defining(&o.defining)?;
    }
    Ok(found)
}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_boss_claims_its_own_top_but_not_a_wider_face() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/golden_polygonal_boss.step");
        let part = crate::read_step_file(&path).unwrap();
        // Sides 6..=11 stand on the plate top 2; 12 is the hexagonal top at z = 35.
        let features = super::super::recognise(&part);
        assert_eq!(
            features.defining["polygonal_bosses"],
            [vec![6, 7, 8, 9, 10, 11, 12]]
        );
        let record = &features.polygonal_bosses[0];
        let sides = vec![6, 7, 8, 9, 10, 11];
        let mut faces = sides.clone();
        claim_terminal_cap(&part, record, &mut faces, &[2, 6, 7, 8, 9, 10, 11]);
        assert_eq!(faces, sides, "the support plane extends beyond the ring");
        claim_terminal_cap(&part, record, &mut faces, &[1, 6, 7, 8, 9, 10, 11]);
        assert_eq!(faces, sides, "a side of the block is not a cap");
    }
}
