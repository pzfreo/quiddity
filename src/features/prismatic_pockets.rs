//! Pockets of any prismatic cross-section (`quiddity.prismatic_pockets`): a closed ring of
//! planar walls with a floor filling one end and the other end open, the capped sibling of a
//! passage.
//!
//! Three sources, in order. Every principal-axis ring (`rings`) capped at exactly one end. Then
//! two recoveries for a ring a partial treatment broke: from one exterior mouth (an inner wire
//! of a principal plane meeting the cavity convexly), the cavity region reached by concave or
//! smooth turns, with one floor level and a wall cycle behind the treatment; and from an intact
//! polygonal floor whose straight-edged boundary meets one concave wall per edge, all
//! rising to one side. Both recoveries prove the section empty from mouth to floor, nothing at
//! the mouth, and material behind the floor, by swept-section volume probes. A wall set already
//! reported is not reported again.
//!
//! Each pocket is defined by its walls; the floor (and, for a mouth-recovered pocket, the rest
//! of its cavity region) is consulted during discovery. After reconciliation the shared rule
//! in `holes` adds a uniquely closed planar floor to the accepted claim.
//! Python's `local_degradation` filter (dropping a pocket
//! whose faces share no valid solid on a degraded graph) is not ported, and the evidence path
//! refuses such a pocket instead: the default inventory retries degraded on three corpus parts,
//! and the filter runs on each but drops nothing (`tests/local_degradation.rs`).

use std::cmp::Ordering;
use std::collections::BTreeSet;

use serde::Serialize;

use super::Context;
use super::edge_open::SPAN_EPS;
use super::evidence::{self, EvidenceError, Occurrence, common_valid_solid};
use super::graph::{is_planar, normal, span};
use super::placed::{Grid, Placed};
use super::policy::AXIS_ZERO_COS;
use super::rings::{Ring, canonical, capped_ends, centroid, cross_section, is_void, rings};
use super::sections::V2;
use super::wire_seed::{inner_wires, wire_seed};
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::Curve;
use crate::kernel::py;
use crate::kernel::volume::{Prism, PrismEdge, Probe, common_volume, probe_volume};

const AXES: [&str; 3] = ["x", "y", "z"];

/// The least thickness of an end probe (`_END_PROBE`).
const END_PROBE: f64 = 2e-5;
/// The material fraction that still counts as none, or as full (`_MATERIAL_VOL_FRAC`).
const MATERIAL_VOL_FRAC: f64 = 1e-9;

/// A floored recess of constant planar cross-section, open at one end (`PrismaticPocket`).
/// `axis` is the direction the walls run, `sides` the number of walls, `depth` from the open end
/// to the floor, `open_sign` +1 when the opening is at the high end of `axis`, `at` the void's
/// centre, and `section` its corners in the two other axes, walked canonically.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PrismaticPocket {
    pub axis: String,
    pub sides: usize,
    pub depth: f64,
    pub open_sign: i32,
    pub at: Placed<[f64; 3]>,
    pub section: Vec<Placed<V2>>,
}

impl PrismaticPocket {
    /// The section's published corners.
    pub fn section_points(&self) -> Vec<V2> {
        self.section.iter().map(|p| **p).collect()
    }

    /// Python's sort key `(axis, at, section)`.
    fn order(&self, other: &Self) -> Ordering {
        self.axis
            .cmp(&other.axis)
            .then_with(|| py::tuple_order(&self.at[..], &other.at[..]))
            .then_with(|| section_order(&self.section_points(), &other.section_points()))
    }
}

/// Python's order between two corner tuples.
fn section_order(a: &[V2], b: &[V2]) -> Ordering {
    a.iter()
        .zip(b)
        .map(|(x, y)| py::tuple_order(x, y))
        .find(|o| o.is_ne())
        .unwrap_or_else(|| a.len().cmp(&b.len()))
}

/// One interrupted wall ring proved from an exterior mouth to one floor (`_RecoveredPocket`).
struct Recovered {
    axis: usize,
    low: f64,
    high: f64,
    open_sign: i32,
    section: Vec<V2>,
    /// Ascending.
    walls: Vec<usize>,
    constituent: BTreeSet<usize>,
}

fn others(axis: usize) -> [usize; 2] {
    match axis {
        0 => [1, 2],
        1 => [0, 2],
        _ => [0, 1],
    }
}

/// The section swept along *axis* over `low..high`, drawn in by `SPAN_EPS` at each end
/// (`_section_prism`); `None` where Python refuses a run of at most twice that.
fn section_prism(section: &[V2], axis: usize, low: f64, high: f64) -> Option<Probe<'static>> {
    if high - low <= 2.0 * SPAN_EPS {
        return None; // "pocket probe prism is too short"
    }
    let [a0, a1] = others(axis);
    let lo = low + SPAN_EPS;
    let corner = |p: V2| {
        let mut point = [0.0; 3];
        point[axis] = lo;
        point[a0] = p[0];
        point[a1] = p[1];
        point
    };
    let sides = (0..section.len())
        .map(|i| PrismEdge {
            points: vec![corner(section[i]), corner(section[(i + 1) % section.len()])],
            straight: true,
        })
        .collect();
    Some(Probe::Prism(Prism {
        axis,
        lo,
        hi: lo + (high - low - 2.0 * SPAN_EPS),
        loops: vec![sides],
    }))
}

/// The thin slab of the section on the *sign* side of `at` (`_section_slab`).
fn section_slab(
    section: &[V2],
    axis: usize,
    at: f64,
    sign: f64,
    thickness: f64,
) -> Option<Probe<'static>> {
    let (a, b) = (at + sign * SPAN_EPS, at + sign * thickness);
    let (low, high) = (a.min(b), a.max(b));
    section_prism(section, axis, low - SPAN_EPS, high + SPAN_EPS)
}

/// How much of *probe* the solid's material fills (`material_fraction`); `None` when the volume
/// probe cannot answer.
fn material_fraction(ctx: &Context<'_>, solid: usize, probe: &Probe<'_>) -> Option<f64> {
    Some(common_volume(ctx.solid_classifier(solid), probe)? / probe_volume(probe)?)
}

/// Is the section empty over the run, open at the mouth and closed at the floor
/// (`_void_open_and_floored`)? A probe that cannot be built or answered proves nothing.
fn void_open_and_floored(
    ctx: &Context<'_>,
    solid: usize,
    section: &[V2],
    axis: usize,
    mouth_at: f64,
    floor_at: f64,
) -> bool {
    let (low, high) = (mouth_at.min(floor_at), mouth_at.max(floor_at));
    let centre = centroid(section);
    let radius = section
        .iter()
        .map(|p| py::dist(p, &centre))
        .fold(f64::NEG_INFINITY, f64::max);
    let thickness = END_PROBE.max((high - low) * 1e-4).max(radius * 1e-4);
    let mouth_sign = if mouth_at == high { 1.0 } else { -1.0 };
    // Python builds all three probes before asking any: one that cannot be built fails all.
    let (Some(interior), Some(mouth), Some(floor)) = (
        section_prism(section, axis, low, high),
        section_slab(section, axis, mouth_at, mouth_sign, thickness),
        section_slab(section, axis, floor_at, -mouth_sign, thickness),
    ) else {
        return false;
    };
    let fraction = |probe: &Probe<'_>| material_fraction(ctx, solid, probe);
    fraction(&interior).is_some_and(|f| f <= MATERIAL_VOL_FRAC)
        && fraction(&mouth).is_some_and(|f| f <= MATERIAL_VOL_FRAC)
        && fraction(&floor).is_some_and(|f| f >= 1.0 - MATERIAL_VOL_FRAC)
}

/// The cavity reached from *seed* by concave or smooth turns, never through *opening*
/// (`_inner_region`).
fn inner_region(part: &Part, opening: usize, seed: &BTreeSet<usize>) -> BTreeSet<usize> {
    let mut region = seed.clone();
    let mut pending: Vec<usize> = seed.iter().copied().collect();
    while let Some(current) = pending.pop() {
        for neighbour in part.neighbours(current) {
            if neighbour == opening || region.contains(&neighbour) {
                continue;
            }
            if matches!(
                part.arc(current, neighbour),
                Some(Arc::Concave | Arc::Smooth)
            ) {
                region.insert(neighbour);
                pending.push(neighbour);
            }
        }
    }
    region
}

/// The one principal axis a face's normal runs along, if any (`_axis_for_opening`).
fn principal_axis(part: &Part, face: usize) -> Option<usize> {
    let n = normal(part, face)?;
    let axes: Vec<usize> = (0..3)
        .filter(|&a| n[a].abs() >= 1.0 - AXIS_ZERO_COS)
        .collect();
    (axes.len() == 1).then(|| axes[0])
}

/// Where a face across *axis* lies along it, when it is flat to `SPAN_EPS` (`_plane_at`).
fn plane_at(part: &Part, face: usize, axis: usize) -> Option<f64> {
    let n = normal(part, face)?;
    if n[axis].abs() < 1.0 - AXIS_ZERO_COS {
        return None;
    }
    let (low, high) = span(part, face, axis);
    (high - low <= SPAN_EPS).then_some(0.5 * (low + high))
}

/// Whether a face is a planar wall running along *axis*.
fn is_wall(part: &Part, face: usize, axis: usize) -> bool {
    is_planar(part, face) && normal(part, face).is_some_and(|n| n[axis].abs() <= AXIS_ZERO_COS)
}

/// One straight-edged floor boundary as canonical corners, each the vertex consecutive edges
/// share, in wire order (`_floor_section`).
fn floor_section(part: &Part, wire: &[usize], axis: usize) -> Option<Vec<V2>> {
    if wire.len() < 3
        || wire
            .iter()
            .any(|&e| !matches!(part.edges[e].curve, Curve::Line { .. }))
    {
        return None;
    }
    let [a0, a1] = others(axis);
    let mut corners = Vec::with_capacity(wire.len());
    for (index, &e) in wire.iter().enumerate() {
        let before = &part.edges[wire[(index + wire.len() - 1) % wire.len()]];
        let edge = &part.edges[e];
        let ends = |ed: &crate::kernel::brep::Edge| {
            let mut out = vec![(ed.vertices.0, ed.start)];
            if ed.vertices.1 != ed.vertices.0 {
                out.push((ed.vertices.1, ed.end));
            }
            out
        };
        let shared: Vec<[f64; 3]> = ends(before)
            .into_iter()
            .filter(|(v, _)| ends(edge).iter().any(|(w, _)| w == v))
            .map(|(_, p)| p)
            .collect();
        let [point] = shared[..] else {
            return None;
        };
        corners.push([point[a0], point[a1]]);
    }
    canonical(corners)
}

/// Python's order `(axis, centroid, low, high)` between recovered pockets.
fn recovered_order(a: &Recovered, b: &Recovered) -> Ordering {
    a.axis
        .cmp(&b.axis)
        .then_with(|| py::tuple_order(&centroid(&a.section), &centroid(&b.section)))
        .then_with(|| py::order(a.low, b.low))
        .then_with(|| py::order(a.high, b.high))
}

/// Pockets whose intact polygonal floor outlives a broken mouth-side ring
/// (`_floor_seeded_regions`). Every boundary edge must meet an observed concave wall running
/// along the floor normal; the swept section and both end slabs still prove the closed floor,
/// empty cavity and open mouth, including for a four-sided section.
fn floor_seeded_regions(ctx: &Context<'_>) -> Vec<Recovered> {
    let part = ctx.part;
    let mut recovered = Vec::new();
    for floor in 0..part.faces.len() {
        if !is_planar(part, floor) {
            continue;
        }
        let Some(axis) = principal_axis(part, floor) else {
            continue;
        };
        let [only] = &part.faces[floor].loops[..] else {
            continue;
        };
        let wire: Vec<usize> = only.edges.iter().map(|&(e, _)| e).collect();
        let Some(section) = floor_section(part, &wire, axis) else {
            continue;
        };
        let walls: Vec<usize> = wire_seed(part, floor, &wire).into_iter().collect();
        if walls.len() != section.len()
            || walls
                .iter()
                .any(|&w| !is_wall(part, w, axis) || part.arc(floor, w) != Some(Arc::Concave))
        {
            continue;
        }
        let Some(floor_at) = plane_at(part, floor, axis) else {
            continue;
        };
        // Each wall must reach the floor; its other end is how far it rises.
        let Some(far) = walls
            .iter()
            .map(|&w| {
                let (low, high) = span(part, w, axis);
                if (low - floor_at).abs() <= SPAN_EPS {
                    Some(high)
                } else if (high - floor_at).abs() <= SPAN_EPS {
                    Some(low)
                } else {
                    None
                }
            })
            .collect::<Option<Vec<f64>>>()
        else {
            continue;
        };
        let rises = far.iter().filter(|&&v| v > floor_at).count();
        if rises != 0 && rises != far.len() {
            continue;
        }
        let direction = if rises > 0 { 1 } else { -1 };
        let mouth_at = if direction > 0 {
            far.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        } else {
            far.iter().copied().fold(f64::INFINITY, f64::min)
        };
        if (mouth_at - floor_at).abs() <= SPAN_EPS {
            continue;
        }
        let mut faces = walls.clone();
        faces.push(floor);
        let owner = common_valid_solid(part, &faces);
        let (low, high) = (mouth_at.min(floor_at), mouth_at.max(floor_at));
        let members: BTreeSet<usize> = walls.iter().copied().collect();
        let caps = capped_ends(ctx, &walls, &members, axis, low, high);
        let (floor_cap, mouth_cap) = if floor_at == low {
            (&caps.0, &caps.1)
        } else {
            (&caps.1, &caps.0)
        };
        let Some(owner) = owner else {
            continue;
        };
        if *floor_cap != BTreeSet::from([floor])
            || !mouth_cap.is_empty()
            || !void_open_and_floored(ctx, owner, &section, axis, mouth_at, floor_at)
        {
            continue;
        }
        recovered.push(Recovered {
            axis,
            low,
            high,
            open_sign: direction,
            section,
            constituent: faces.into_iter().collect(),
            walls,
        });
    }
    recovered.sort_by(recovered_order);
    recovered
}

/// A mouth: the opening face, its inner wire's seed faces, and the axis it lies across.
type Mouth = (usize, BTreeSet<usize>, usize);

/// Pockets recovered from one exterior mouth whose wall spans a treatment interrupted
/// (`_one_ended_regions`): the cavity region behind an inner wire of a principal plane, unique
/// to that mouth and shared with no other region, with one floor level.
fn one_ended_regions(
    ctx: &Context<'_>,
    refusals: &mut Vec<super::discovery::DiscoveryRefusal>,
) -> Vec<Recovered> {
    let part = ctx.part;
    // Regions in the order Python's dict first meets them, each with its mouths.
    let mut raw: Vec<(BTreeSet<usize>, Vec<Mouth>)> = Vec::new();
    for opening in 0..part.faces.len() {
        if !is_planar(part, opening) {
            continue;
        }
        let Some(axis) = principal_axis(part, opening) else {
            continue;
        };
        for wire in inner_wires(part, opening) {
            let seed = wire_seed(part, opening, &wire);
            let arcs: Vec<Option<Arc>> = seed.iter().map(|&n| part.arc(opening, n)).collect();
            if seed.is_empty()
                || !arcs
                    .iter()
                    .all(|a| matches!(a, Some(Arc::Convex | Arc::Smooth)))
                || !arcs.contains(&Some(Arc::Convex))
            {
                continue;
            }
            let region = inner_region(part, opening, &seed);
            match raw.iter_mut().find(|(r, _)| *r == region) {
                Some((_, mouths)) => mouths.push((opening, seed, axis)),
                None => raw.push((region, vec![(opening, seed, axis)])),
            }
        }
    }

    let mut recovered = Vec::new();
    for (region, mouths) in &raw {
        let intersects = raw
            .iter()
            .any(|(other, _)| other != region && !other.is_disjoint(region));
        let [(opening, seed, axis)] = &mouths[..] else {
            continue;
        };
        if intersects {
            continue;
        }
        let (opening, axis) = (*opening, *axis);
        let Some(mouth_at) = plane_at(part, opening, axis) else {
            continue;
        };
        let mut floor_planes: Vec<(f64, usize)> = region
            .iter()
            .filter_map(|&n| plane_at(part, n, axis).map(|at| (at, n)))
            .filter(|&(at, _)| (at - mouth_at).abs() > SPAN_EPS)
            .collect();
        floor_planes.sort_by(|a, b| py::order(a.0, b.0).then(a.1.cmp(&b.1)));
        // Levels grouped within `SPAN_EPS` of each group's first.
        let mut floor_groups: Vec<(f64, BTreeSet<usize>)> = Vec::new();
        for (at, node) in floor_planes {
            match floor_groups.last_mut() {
                Some((first, nodes)) if (at - *first).abs() <= SPAN_EPS => {
                    nodes.insert(node);
                }
                _ => floor_groups.push((at, BTreeSet::from([node]))),
            }
        }
        let [(floor_at, floor_nodes)] = &floor_groups[..] else {
            continue;
        };
        let floor_at = *floor_at;
        let walls: Vec<usize> = region
            .iter()
            .copied()
            .filter(|&n| is_wall(part, n, axis))
            .collect();
        let wall_set: BTreeSet<usize> = walls.iter().copied().collect();
        let interrupted_only_at_the_mouth = region
            .iter()
            .filter(|n| !wall_set.contains(n) && !floor_nodes.contains(n))
            .all(|n| seed.contains(n));
        if !interrupted_only_at_the_mouth
            || walls.len() < 3
            || walls.iter().any(|&w| {
                part.neighbours(w)
                    .into_iter()
                    .collect::<BTreeSet<usize>>()
                    .intersection(&wall_set)
                    .count()
                    != 2
            })
        {
            continue;
        }
        let Some(section) = cross_section(ctx, &walls, &wall_set, axis) else {
            refusals.push(super::discovery::DiscoveryRefusal::new(
                "wall boundary is not one simple prismatic polygon",
                region.iter().copied().chain([opening]),
            ));
            continue;
        };
        let (low, high) = (mouth_at.min(floor_at), mouth_at.max(floor_at));
        let mut faces: Vec<usize> = region.iter().copied().collect();
        faces.push(opening);
        let solid = common_valid_solid(part, &faces);
        let Some(solid) = solid else {
            continue;
        };
        if high - low <= SPAN_EPS
            || !is_void(ctx.solid_classifier(solid), &section, axis, low, high)
            || !void_open_and_floored(ctx, solid, &section, axis, mouth_at, floor_at)
        {
            continue;
        }
        recovered.push(Recovered {
            axis,
            low,
            high,
            open_sign: if mouth_at > floor_at { 1 } else { -1 },
            section,
            walls,
            constituent: region.clone(),
        });
    }
    recovered.sort_by(recovered_order);
    recovered
}

fn record(
    axis: usize,
    sides: usize,
    low: f64,
    high: f64,
    open_sign: i32,
    section: &[V2],
) -> PrismaticPocket {
    let [a0, a1] = others(axis);
    let middle = centroid(section);
    let mut at = [0.0; 3];
    at[axis] = 0.5 * (low + high);
    at[a0] = middle[0];
    at[a1] = middle[1];
    PrismaticPocket {
        axis: AXES[axis].into(),
        sides,
        depth: py::round_to(high - low, 3),
        open_sign,
        at: Placed::new(at, Grid::Decimals(3)),
        section: section
            .iter()
            .map(|&p| Placed::new(p, Grid::Decimals(3)))
            .collect(),
    }
}

internal_entry! {
/// `recognise_prismatic_pockets`.
pub fn recognise_prismatic_pockets(part: &Part) -> Vec<PrismaticPocket> {
    super::records(discover(&Context::new(part)))
}
}

internal_entry! {
/// The evidence path: each pocket defined by its walls, its floor (or cavity region) consulted.
pub fn discover_verified(
    ctx: &Context<'_>,
) -> Result<Vec<Occurrence<PrismaticPocket>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx))
}
}

internal_entry! {
/// Every pocket, in record order (`_discover_prismatic_pockets`).
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<PrismaticPocket>> {
    analyse(ctx).accepted
}
}

pub(crate) fn analyse(ctx: &Context<'_>) -> super::discovery::Analysis<PrismaticPocket> {
    let mut refusals = Vec::new();
    let occurrence = |record, walls: &[usize], constituent: &BTreeSet<usize>| Occurrence {
        record,
        defining: walls.to_vec(),
        context: constituent
            .iter()
            .copied()
            .filter(|n| !walls.contains(n))
            .collect(),
    };
    let mut found: Vec<Occurrence<PrismaticPocket>> = Vec::new();
    let mut existing: Vec<BTreeSet<usize>> = Vec::new();
    for ring in rings(ctx) {
        let Ring {
            nodes,
            section,
            axis,
            low,
            high,
            cap_nodes,
        } = &ring;
        let (low_capped, high_capped) = ring.caps();
        if low_capped == high_capped {
            continue; // neither: a passage; both: an enclosed cavity, which no tool reaches
        }
        // The floor caps one end, so the opening is the other.
        let open_sign = if low_capped { 1 } else { -1 };
        let constituent: BTreeSet<usize> = nodes
            .iter()
            .chain(&cap_nodes.0)
            .chain(&cap_nodes.1)
            .copied()
            .collect();
        let pocket = record(*axis, nodes.len(), *low, *high, open_sign, section);
        existing.push(nodes.iter().copied().collect());
        found.push(occurrence(pocket, nodes, &constituent));
    }
    for r in one_ended_regions(ctx, &mut refusals)
        .into_iter()
        .chain(floor_seeded_regions(ctx))
    {
        let walls: BTreeSet<usize> = r.walls.iter().copied().collect();
        if existing.contains(&walls) {
            continue;
        }
        existing.push(walls);
        let pocket = record(
            r.axis,
            r.walls.len(),
            r.low,
            r.high,
            r.open_sign,
            &r.section,
        );
        found.push(occurrence(pocket, &r.walls, &r.constituent));
    }
    found.sort_by(|a, b| a.record.order(&b.record));
    super::discovery::Analysis {
        accepted: found,
        refusals,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disconnected_wall_cycles_are_a_visible_pocket_refusal() {
        let part = super::super::discovery::tests::island_pocket();
        assert!(part.solid_is_geometrically_valid(0));
        let ctx = Context::new(&part);
        let walls: Vec<_> = (6..10).chain(11..15).collect();
        let members = walls.iter().copied().collect();
        assert!(cross_section(&ctx, &walls, &members, 2).is_none());
        let analysis = analyse(&ctx);
        assert!(analysis.accepted.is_empty());
        assert_eq!(analysis.refusals.len(), 1);
        assert_eq!(analysis.refusals[0].faces, (5..15).collect::<Vec<_>>());
        let recognised = crate::correspondence::recognise(&part);
        let document = crate::recognition::document(&recognised, part.faces.len());
        let refusal = document
            .records
            .iter()
            .find(|r| r.record_type == "PrismaticPocketRefusal")
            .unwrap();
        assert!(refusal.is_refusal());
        assert_eq!(refusal.faces, analysis.refusals[0].faces);
        assert!(
            recognised
                .fingerprints
                .features
                .iter()
                .all(|f| !f.family.ends_with("refusals"))
        );
        assert!(
            document
                .face_accounting
                .as_ref()
                .unwrap()
                .iter()
                .any(|f| matches!(f, crate::recognition::FaceAccounting::Refused { .. }))
        );
    }

    #[test]
    fn an_intact_rectangular_floor_survives_an_intersected_mouth() {
        // Independent box-minus-boxes construction: stock 40 x 35 x 20, a 20 x 12
        // pocket from z=10, and an edge-open cut from z=15 shortening its x=8 end wall.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/interrupted_pocket_mouth.step");
        for axes in [[0, 1, 2], [1, 2, 0], [2, 0, 1]] {
            let mut placement = [[0.; 4]; 3];
            for i in 0..3 {
                placement[axes[i]][i] = 1.;
                placement[i][3] = [123.456, -78.9, 41.3][i];
            }
            let part = crate::kernel::step::read_step_file_placed(&path, &placement).unwrap();
            let found = discover_verified(&Context::new(&part)).unwrap();
            let [pocket] = &found[..] else {
                panic!("{found:?}")
            };
            assert_eq!(pocket.record.sides, 4);
            assert_eq!(pocket.record.depth, 10.);
            assert_eq!(pocket.defining, [9, 10, 11, 14]);
            assert_eq!(pocket.context, [15]);
        }
    }

    #[test]
    fn a_probe_prism_no_longer_than_its_insets_is_refused() {
        let section = [[-2.0, -2.0], [2.0, -2.0], [0.0, 2.0]];
        assert!(section_prism(&section, 2, 0.0, 2.0 * SPAN_EPS).is_none());
        let Some(Probe::Prism(prism)) = section_prism(&section, 2, 0.0, 1.0) else {
            panic!("a prism");
        };
        assert_eq!(prism.lo, SPAN_EPS);
        assert!((prism.hi - (1.0 - SPAN_EPS)).abs() < 1e-15);
        let volume = probe_volume(&Probe::Prism(prism)).unwrap();
        assert!((volume - 8.0 * (1.0 - 2.0 * SPAN_EPS)).abs() < 1e-12);
    }

    #[test]
    fn a_slab_lies_on_its_side_of_the_station() {
        let section = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        let bounds = |sign| match section_slab(&section, 0, 5.0, sign, 1e-3) {
            Some(Probe::Prism(p)) => (p.lo, p.hi),
            _ => panic!("a slab"),
        };
        let (lo, hi) = bounds(1.0);
        assert!((lo - (5.0 + SPAN_EPS)).abs() < 1e-12 && (hi - 5.001).abs() < 1e-12);
        let (lo, hi) = bounds(-1.0);
        assert!((lo - 4.999).abs() < 1e-12 && (hi - (5.0 - SPAN_EPS)).abs() < 1e-12);
    }
}
