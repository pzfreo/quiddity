//! Constant-section planar-wall rings on any run direction (`quiddity._section_passages`): the
//! proposals passages and oriented slots publish from.
//!
//! Three readings propose a ring, each proved empty along its run and open beyond both ends by
//! volume probes:
//!
//! - a cycle of planar walls meeting along straight junctions parallel to one run, each wall
//!   spanning the same interval, with a stock plane at both ends;
//! - two planar mouths (inner wires of planar faces) bounding one inner region: parallel opposed
//!   mouths across the run, or mouths at any slope to a run the walls themselves prove, the
//!   section then cut by the two termination planes;
//! - one mouth whose far end is chamfered, its bevels explained by
//!   [`super::entry_treatments`].
//!
//! The probes are polyhedra built exactly (`super::support_patches::ruled_prism`), measured by
//! the kernel's volume probe rather than a boolean.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use super::Context;
use super::entry_treatments::{material_at_most, prove_entry_treatments};
use super::evidence::common_valid_solid;
use super::graph::{face_vertices, is_planar, normal};
use super::sections::{
    BodyRef, BodyRefIssuer, LocalFrame, PlanarSection, SectionEnds, SectionError,
    SectionOccurrence, V2, validate_occurrence,
};
use super::support_patches::ruled_prism;
use super::wire_seed::wire_seed;
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::{Curve, V3};
use crate::kernel::py;

const DIRECTION_TOL: f64 = 2e-8;
const INTERVAL_TOL: f64 = 1e-6;
const END_PROBE: f64 = 2e-5;
const COORD_FLOOR: f64 = 1e-6;
const MATERIAL_VOL_FRAC: f64 = 1e-9;

/// Two planar faces meeting along a straight edge, and its canonical direction.
type Junction = (usize, usize, V3);

/// One proposed ring (`SectionRingProposal`): the placed section, its defining wall faces in
/// order, the solid, the faces it is made of (empty for a wall cycle), and for sloped mouths the
/// termination planes' gradients across the section.
#[derive(Clone, Debug)]
pub struct SectionRingProposal {
    pub occurrence: SectionOccurrence,
    pub nodes: Vec<usize>,
    pub solid: usize,
    pub constituent: BTreeSet<usize>,
    pub low_gradient: V2,
    pub high_gradient: V2,
}

/// One body identity per graph solid (`_BodyAdapter`).
#[derive(Default)]
struct BodyAdapter {
    issuer: BodyRefIssuer,
    pairs: BTreeMap<usize, BodyRef>,
}

impl BodyAdapter {
    fn body(&mut self, solid: usize) -> Result<BodyRef, SectionError> {
        if let Some(body) = self.pairs.get(&solid) {
            return Ok(body.clone());
        }
        let body = self.issuer.issue(None)?;
        self.pairs.insert(solid, body.clone());
        Ok(body)
    }

    fn occurrence(
        &mut self,
        solid: usize,
        frame: LocalFrame,
        interval: (f64, f64),
        section: PlanarSection,
    ) -> Result<SectionOccurrence, SectionError> {
        let occurrence = SectionOccurrence::new(
            self.body(solid)?,
            frame,
            interval,
            section,
            SectionEnds::new(false, false)?,
        )?;
        validate_occurrence(&occurrence, &self.issuer)?;
        Ok(occurrence)
    }
}

fn dot(a: V3, b: V3) -> f64 {
    py::dot(&a, &b)
}

/// A straight edge's direction in canonical sense (`_canonical_run`); `None` for any other edge.
fn canonical_run(part: &Part, edge: usize) -> Option<V3> {
    let Curve::Line { dir, .. } = part.edges[edge].curve else {
        return None;
    };
    let tangent = crate::kernel::geom::unit(dir)?;
    LocalFrame::canonical(tangent, [0.0; 3]).ok().map(|f| f.run)
}

fn parallel(left: V3, right: V3) -> bool {
    (dot(left, right).abs() - 1.0).abs() <= DIRECTION_TOL
}

fn min_run(runs: impl IntoIterator<Item = V3>) -> Option<V3> {
    runs.into_iter().reduce(|a, b| {
        if py::tuple_order(&b, &a) == Ordering::Less {
            b
        } else {
            a
        }
    })
}

/// One collinear junction of two walls as `(u, v, t_low, t_high)` in *frame* (`_pair_line`):
/// every shared edge straight along the run, on one line, the pieces joined without gaps.
pub(crate) fn pair_line(
    part: &Part,
    left: usize,
    right: usize,
    frame: &LocalFrame,
) -> Option<[f64; 4]> {
    let mut samples: Vec<V3> = Vec::new();
    let mut segments: Vec<[f64; 2]> = Vec::new();
    for edge in part.shared_edges(left, right) {
        let run = canonical_run(part, edge)?;
        if !parallel(run, frame.run) {
            return None;
        }
        let e = &part.edges[edge];
        let projected =
            [e.start, e.end].map(|p| [dot(p, frame.u), dot(p, frame.v), dot(p, frame.run)]);
        samples.extend(projected);
        let (a, b) = (projected[0][2], projected[1][2]);
        segments.push(if py::order(b, a) == Ordering::Less {
            [b, a]
        } else {
            [a, b]
        });
    }
    if samples.is_empty() {
        return None;
    }
    let n = samples.len() as f64;
    let u = py::sum(samples.iter().map(|s| s[0])) / n;
    let v = py::sum(samples.iter().map(|s| s[1])) / n;
    if samples
        .iter()
        .any(|s| py::hypot(&[s[0] - u, s[1] - v]) > INTERVAL_TOL)
    {
        return None;
    }
    segments.sort_by(|a, b| py::tuple_order(a, b));
    for w in segments.windows(2) {
        let delta = w[1][0] - w[0][1];
        if !(-INTERVAL_TOL..=INTERVAL_TOL).contains(&delta) {
            return None;
        }
    }
    Some([u, v, segments[0][0], segments[segments.len() - 1][1]])
}

/// The face's vertices' extent along *run* (`_face_interval`).
fn face_interval(part: &Part, face: usize, run: V3) -> Option<(f64, f64)> {
    let values: Vec<f64> = face_vertices(part, face)
        .iter()
        .map(|&p| dot(p, run))
        .collect();
    let low = values.iter().copied().reduce(f64::min)?;
    let high = values.iter().copied().reduce(f64::max)?;
    Some((low, high))
}

/// The region a mouth's walls bound: the seed grown across concave and smooth arcs, never
/// through the opening (`_bounded_inner_region`).
fn bounded_inner_region(part: &Part, opening: usize, seed: &BTreeSet<usize>) -> BTreeSet<usize> {
    let mut region = seed.clone();
    let mut pending: Vec<usize> = seed.iter().copied().collect();
    while let Some(current) = pending.pop() {
        for neighbour in part.neighbours(current) {
            if neighbour == opening || region.contains(&neighbour) {
                continue;
            }
            if matches!(
                part.arc(current, neighbour),
                Some(Arc::Concave) | Some(Arc::Smooth)
            ) {
                region.insert(neighbour);
                pending.push(neighbour);
            }
        }
    }
    region
}

/// A planar mouth: the opening face, its inner wire as edge uses, and the walls the wire bounds.
#[derive(Clone)]
struct Mouth {
    opening: usize,
    wire: Vec<(usize, bool)>,
    seed: BTreeSet<usize>,
}

/// Every planar mouth whose walls are at least three planes turning convex from the opening,
/// grouped by the inner region they bound (`_mouth_regions`), regions by their sorted faces,
/// mouths by opening.
fn mouth_regions(part: &Part) -> Vec<(BTreeSet<usize>, Vec<Mouth>)> {
    let mut by_region: BTreeMap<BTreeSet<usize>, Vec<Mouth>> = BTreeMap::new();
    for opening in 0..part.faces.len() {
        if !is_planar(part, opening) {
            continue;
        }
        let outer = part.outer_loop(opening);
        for (l, lp) in part.faces[opening].loops.iter().enumerate() {
            if Some(l) == outer {
                continue;
            }
            let mut wire: Vec<(usize, bool)> = Vec::new();
            for &(e, forward) in &lp.edges {
                if !wire.iter().any(|w| w.0 == e) {
                    wire.push((e, forward));
                }
            }
            let edges: Vec<usize> = wire.iter().map(|w| w.0).collect();
            let seed = wire_seed(part, opening, &edges);
            if seed.len() < 3
                || seed.iter().any(|&n| !is_planar(part, n))
                || !seed
                    .iter()
                    .all(|&n| part.arc(opening, n) == Some(Arc::Convex))
            {
                continue;
            }
            by_region
                .entry(bounded_inner_region(part, opening, &seed))
                .or_default()
                .push(Mouth {
                    opening,
                    wire,
                    seed,
                });
        }
    }
    // Openings are visited in order, so each region's mouths are already by opening.
    by_region.into_iter().collect()
}

/// The point of a vertex identity, read from an edge ending there.
fn vertex_point(part: &Part, edge: usize, vertex: usize) -> V3 {
    let e = &part.edges[edge];
    if e.vertices.0 == vertex {
        e.start
    } else {
        e.end
    }
}

/// The wire's first vertex (`wire.vertices()[0]`): its first edge's start.
fn first_vertex(part: &Part, wire: &[(usize, bool)]) -> V3 {
    part.edges[wire[0].0].start
}

/// A straight-edged wire as a section in *base*'s `u`, `v`, centred on its centroid, with that
/// centroid placed in the world (`_line_section`).
pub(crate) fn line_section(
    part: &Part,
    wire: &[(usize, bool)],
    base: &LocalFrame,
) -> Option<(PlanarSection, V3)> {
    if wire
        .iter()
        .any(|&(e, _)| !matches!(part.edges[e].curve, Curve::Line { .. }))
    {
        return None;
    }
    let ends = |e: usize| {
        let v = part.edges[e].vertices;
        if v.0 == v.1 {
            vec![v.0]
        } else {
            vec![v.0, v.1]
        }
    };
    let mut points: Vec<V3> = Vec::new();
    for (i, &(e, _)) in wire.iter().enumerate() {
        let previous = wire[(i + wire.len() - 1) % wire.len()].0;
        let mine = ends(e);
        let shared: Vec<usize> = ends(previous)
            .into_iter()
            .filter(|v| mine.contains(v))
            .collect();
        let &[vertex] = shared.as_slice() else {
            return None;
        };
        points.push(vertex_point(part, e, vertex));
    }
    if points.len() < 3 {
        return None;
    }
    let raw = PlanarSection::polygon(
        &points
            .iter()
            .map(|&p| [dot(p, base.u), dot(p, base.v)])
            .collect::<Vec<_>>(),
    )
    .ok()?;
    let centre = raw.centroid();
    let world = [0, 1, 2].map(|i| centre[0] * base.u[i] + centre[1] * base.v[i]);
    let section = raw.translated([-centre[0], -centre[1]]).ok()?;
    Some((section, world))
}

fn same_section(left: &PlanarSection, right: &PlanarSection) -> bool {
    left.boundary().len() == right.boundary().len()
        && left
            .boundary()
            .iter()
            .zip(right.boundary())
            .all(|(a, b)| py::dist(&a.point, &b.point) <= INTERVAL_TOL)
}

/// The one straight junction direction a planar wall region proves (`_wall_run`): the region a
/// cycle, every junction straight and parallel, every wall along it.
fn wall_run(part: &Part, region: &BTreeSet<usize>) -> Option<V3> {
    if region.len() < 3 || region.iter().any(|&n| !is_planar(part, n)) {
        return None;
    }
    let mut runs: Vec<V3> = Vec::new();
    let mut degree: BTreeMap<usize, usize> = BTreeMap::new();
    for &left in region {
        for right in part.neighbours(left) {
            if !region.contains(&right) || right <= left {
                continue;
            }
            let edge_runs: Vec<Option<V3>> = part
                .shared_edges(left, right)
                .into_iter()
                .map(|e| canonical_run(part, e))
                .collect();
            if edge_runs.is_empty() || edge_runs.iter().any(Option::is_none) {
                return None;
            }
            runs.extend(edge_runs.into_iter().flatten());
            *degree.entry(left).or_default() += 1;
            *degree.entry(right).or_default() += 1;
        }
    }
    if runs.is_empty() || region.iter().any(|n| degree.get(n) != Some(&2)) {
        return None;
    }
    let run = min_run(runs.iter().copied())?;
    if runs.iter().any(|&r| !parallel(r, run)) {
        return None;
    }
    if region
        .iter()
        .any(|&n| normal(part, n).is_none_or(|m| dot(m, run).abs() > DIRECTION_TOL))
    {
        return None;
    }
    Some(run)
}

/// A planar mouth as `t = at + du*x + dv*y` in *frame* (`_termination_plane`).
fn termination_plane(
    part: &Part,
    normal: V3,
    wire: &[(usize, bool)],
    frame: &LocalFrame,
) -> Option<(f64, V2)> {
    let along = dot(normal, frame.run);
    if along.abs() <= DIRECTION_TOL {
        return None;
    }
    let point = first_vertex(part, wire);
    let delta = [0, 1, 2].map(|i| point[i] - frame.origin[i]);
    let at = dot(normal, delta) / along;
    let gradient = [-dot(normal, frame.u) / along, -dot(normal, frame.v) / along];
    [at, gradient[0], gradient[1]]
        .iter()
        .all(|v| v.is_finite())
        .then_some((at, gradient))
}

/// A section point at run coordinate *t* in the world (`_world`).
fn world(frame: &LocalFrame, t: f64, point: V2) -> V3 {
    [0, 1, 2]
        .map(|i| frame.origin[i] + t * frame.run[i] + point[0] * frame.u[i] + point[1] * frame.v[i])
}

/// The section's corners on the plane `t = at + gradient · p`.
fn plane_corners(frame: &LocalFrame, at: f64, gradient: V2, section: &PlanarSection) -> Vec<V3> {
    section
        .boundary()
        .iter()
        .map(|v| {
            let t = at + gradient[0] * v.point[0] + gradient[1] * v.point[1];
            world(frame, t, v.point)
        })
        .collect()
}

/// The section's prism cut by two termination planes (`_between_planes`, a ruled loft);
/// `None` where the planes cross over the section, as Python refuses.
fn between_planes(
    frame: &LocalFrame,
    low: (f64, V2),
    high: (f64, V2),
    section: &PlanarSection,
) -> Option<Part> {
    let at = |plane: (f64, V2), p: V2| plane.0 + plane.1[0] * p[0] + plane.1[1] * p[1];
    if section
        .boundary()
        .iter()
        .any(|v| at(low, v.point) >= at(high, v.point))
    {
        return None;
    }
    ruled_prism(
        &plane_corners(frame, low.0, low.1, section),
        &plane_corners(frame, high.0, high.1, section),
    )
}

fn empty(ctx: &Context<'_>, solid: usize, probe: Option<&Part>) -> bool {
    // A probe that cannot be built or answered proves nothing.
    probe.is_some_and(|p| material_at_most(ctx, solid, p, MATERIAL_VOL_FRAC))
}

fn end_thickness(span: f64, section: &PlanarSection) -> f64 {
    let scale = span.max(1.0);
    let radius = section
        .boundary()
        .iter()
        .map(|v| py::hypot(&v.point))
        .fold(f64::NEG_INFINITY, f64::max);
    END_PROBE.max(scale * 1e-4).max(radius * 1e-4)
}

/// The prism between two sloped mouths empty, and thin slabs beyond both empty
/// (`_void_and_planar_open`).
fn void_and_planar_open(
    ctx: &Context<'_>,
    solid: usize,
    frame: &LocalFrame,
    low: (f64, V2),
    high: (f64, V2),
    section: &PlanarSection,
) -> bool {
    let thickness = end_thickness(high.0 - low.0, section);
    let inner = between_planes(
        frame,
        (low.0 + COORD_FLOOR, low.1),
        (high.0 - COORD_FLOOR, high.1),
        section,
    );
    let low_slab = between_planes(
        frame,
        (low.0 - thickness, low.1),
        (low.0 - COORD_FLOOR, low.1),
        section,
    );
    let high_slab = between_planes(
        frame,
        (high.0 + COORD_FLOOR, high.1),
        (high.0 + thickness, high.1),
        section,
    );
    if inner.is_none() || low_slab.is_none() || high_slab.is_none() {
        return false;
    }
    empty(ctx, solid, inner.as_ref())
        && empty(ctx, solid, low_slab.as_ref())
        && empty(ctx, solid, high_slab.as_ref())
}

/// The section swept along the run between *low* and *high*.
fn extruded(frame: &LocalFrame, low: f64, high: f64, section: &PlanarSection) -> Option<Part> {
    ruled_prism(
        &plane_corners(frame, low, [0.0, 0.0], section),
        &plane_corners(frame, high, [0.0, 0.0], section),
    )
}

internal_entry! {
/// The run's prism drawn in by the coordinate floor at both ends (`_probe_prism`).
pub fn probe_prism(
    frame: &LocalFrame,
    interval: (f64, f64),
    section: &PlanarSection,
) -> Option<Part> {
    let (low, high) = interval;
    if high - low <= 2.0 * COORD_FLOOR {
        return None;
    }
    let start = low + COORD_FLOOR;
    extruded(
        frame,
        start,
        start + (high - low - 2.0 * COORD_FLOOR),
        section,
    )
}
}

internal_entry! {
/// The section slab strictly outside one end (`_end_slab`).
pub fn end_slab(
    frame: &LocalFrame,
    end: f64,
    sign: f64,
    thickness: f64,
    section: &PlanarSection,
) -> Option<Part> {
    let inner = end + sign * COORD_FLOOR;
    let outer = end + sign * thickness;
    let (low, high) = (inner.min(outer), inner.max(outer));
    if high - low <= COORD_FLOOR {
        return None;
    }
    extruded(frame, low, low + (high - low), section)
}
}

/// The run empty and both ends open (`_void_and_open`).
fn void_and_open(
    ctx: &Context<'_>,
    solid: usize,
    frame: &LocalFrame,
    interval: (f64, f64),
    section: &PlanarSection,
) -> bool {
    if !empty(ctx, solid, probe_prism(frame, interval, section).as_ref()) {
        return false;
    }
    let thickness = end_thickness(interval.1 - interval.0, section);
    [(interval.0, -1.0), (interval.1, 1.0)]
        .iter()
        .all(|&(end, sign)| {
            empty(
                ctx,
                solid,
                end_slab(frame, end, sign, thickness, section).as_ref(),
            )
        })
}

/// A stock plane at both ends of every wall (`_observed_planar_ring_ends`): an untreated ring
/// ends where it meets a plane facing out along the run, not merely where a probe finds air.
fn observed_planar_ring_ends(
    part: &Part,
    walls: &[usize],
    run: V3,
    interval: (f64, f64),
    owner: usize,
) -> bool {
    for (at, sign) in [(interval.0, -1.0), (interval.1, 1.0)] {
        for &wall in walls {
            let found = part.neighbours(wall).into_iter().any(|neighbour| {
                if !is_planar(part, neighbour) {
                    return false;
                }
                let Some(n) = normal(part, neighbour) else {
                    return false;
                };
                if dot(n, run) * sign < 1.0 - DIRECTION_TOL {
                    return false;
                }
                let mut members = walls.to_vec();
                members.push(neighbour);
                face_interval(part, neighbour, run).is_some_and(|(lo, hi)| {
                    (lo - at).abs() <= INTERVAL_TOL && (hi - at).abs() <= INTERVAL_TOL
                }) && common_valid_solid(part, &members) == Some(owner)
            });
            if !found {
                return false;
            }
        }
    }
    true
}

/// The cycle through *members* chosen by its corner sequence, never by traversal order
/// (`_ordered_cycle`).
pub(crate) fn ordered_cycle(
    members: &[usize],
    adjacency: &BTreeMap<usize, BTreeSet<usize>>,
    pair_lines: &BTreeMap<(usize, usize), [f64; 4]>,
) -> Result<Vec<usize>, SectionError> {
    let line = |a: usize, b: usize| pair_lines[&(a.min(b), a.max(b))];
    let mut best: Option<(Vec<f64>, Vec<usize>)> = None;
    for &start in members {
        for &first in &adjacency[&start] {
            let mut order = vec![start, first];
            while order.len() < members.len() {
                let previous = order[order.len() - 2];
                let choices: Vec<usize> = adjacency[order.last().expect("started")]
                    .iter()
                    .copied()
                    .filter(|&c| c != previous)
                    .collect();
                let &[next] = choices.as_slice() else {
                    return Err(SectionError(
                        "section wall component is not one simple cycle",
                    ));
                };
                order.push(next);
            }
            let corners: Vec<f64> = (0..order.len())
                .flat_map(|i| {
                    let l = line(order[i], order[(i + 1) % order.len()]);
                    [l[0], l[1]]
                })
                .collect();
            if best
                .as_ref()
                .is_none_or(|(c, _)| py::tuple_order(&corners, c) == Ordering::Less)
            {
                best = Some((corners, order));
            }
        }
    }
    Ok(best.expect("a component has members").1)
}

/// A section's points recentred on its centroid, with the frame through that centroid.
fn centred(
    base: &LocalFrame,
    raw: &PlanarSection,
) -> Result<(LocalFrame, PlanarSection), SectionError> {
    let centre = raw.centroid();
    let frame = LocalFrame::canonical(
        base.run,
        [0, 1, 2].map(|i| centre[0] * base.u[i] + centre[1] * base.v[i]),
    )?;
    Ok((frame, raw.translated([-centre[0], -centre[1]])?))
}

fn treated_entry_proposals(
    ctx: &Context<'_>,
    bodies: &mut BodyAdapter,
) -> Result<Vec<SectionRingProposal>, SectionError> {
    let part = ctx.part;
    let mut proposals = Vec::new();
    for (_, mouths) in mouth_regions(part) {
        for mouth in mouths {
            let Some(n) = normal(part, mouth.opening) else {
                continue;
            };
            let base = LocalFrame::canonical(n, [0.0; 3])?;
            if mouth
                .seed
                .iter()
                .any(|&w| normal(part, w).is_none_or(|m| dot(m, base.run).abs() > DIRECTION_TOL))
            {
                continue;
            }
            let Some((section, centre)) = line_section(part, &mouth.wire, &base) else {
                continue;
            };
            let frame = LocalFrame::canonical(base.run, centre)?;
            let at = dot(first_vertex(part, &mouth.wire), frame.run);
            let mut ends = Vec::new();
            let mut whole = true;
            for &w in &mouth.seed {
                let Some((low, high)) = face_interval(part, w, frame.run) else {
                    whole = false;
                    break;
                };
                if (low - at).abs() <= INTERVAL_TOL {
                    ends.push(high);
                } else if (high - at).abs() <= INTERVAL_TOL {
                    ends.push(low);
                } else {
                    whole = false;
                    break;
                }
            }
            if !whole
                || ends.is_empty()
                || !(ends.iter().all(|&e| e > at + INTERVAL_TOL)
                    || ends.iter().all(|&e| e < at - INTERVAL_TOL))
            {
                continue;
            }
            let far = if ends[0] > at {
                ends.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            } else {
                ends.iter().copied().fold(f64::INFINITY, f64::min)
            };
            let Some(proof) =
                prove_entry_treatments(ctx, &mouth.seed, &mouth.wire, frame.run, at, far)
            else {
                continue;
            };
            let mut members: Vec<usize> = mouth
                .seed
                .iter()
                .chain(&proof.treatments)
                .chain(&proof.stock)
                .copied()
                .collect();
            members.push(mouth.opening);
            let interval = (at.min(far), at.max(far));
            let Some(solid) = common_valid_solid(part, &members) else {
                continue;
            };
            if !void_and_open(ctx, solid, &frame, interval, &section) {
                continue;
            }
            let occurrence = bodies.occurrence(solid, frame, interval, section)?;
            proposals.push(SectionRingProposal {
                occurrence,
                nodes: mouth.seed.iter().copied().collect(),
                solid,
                constituent: mouth.seed.union(&proof.treatments).copied().collect(),
                low_gradient: [0.0, 0.0],
                high_gradient: [0.0, 0.0],
            });
        }
    }
    Ok(proposals)
}

fn enclosure_proposals(
    ctx: &Context<'_>,
    bodies: &mut BodyAdapter,
) -> Result<Vec<SectionRingProposal>, SectionError> {
    let part = ctx.part;
    let mut proposals = Vec::new();
    for (region, mouths) in mouth_regions(part) {
        let [first, second] = mouths.as_slice() else {
            continue;
        };
        let first_normal = normal(part, first.opening);
        let second_normal = normal(part, second.opening);
        let mut members: Vec<usize> = region.iter().copied().collect();
        members.extend([first.opening, second.opening]);
        let solid = common_valid_solid(part, &members);
        let (Some(first_normal), Some(second_normal), Some(solid)) =
            (first_normal, second_normal, solid)
        else {
            continue;
        };
        let parallel_mouths = parallel(first_normal, second_normal);
        if parallel_mouths && dot(first_normal, second_normal) > 0.0 {
            continue;
        }
        let defining: Vec<usize> = first.seed.union(&second.seed).copied().collect();
        let run = wall_run(part, &region);
        // Parallel stock faces need not be perpendicular to the passage: their normals
        // describe termination planes, not the independently proved run.
        if !parallel_mouths || run.is_some_and(|r| !parallel(r, first_normal)) {
            let Some(run) = run else {
                continue;
            };
            let base = LocalFrame::canonical(run, [0.0; 3])?;
            let (Some((section, centre)), Some((other, other_centre))) = (
                line_section(part, &first.wire, &base),
                line_section(part, &second.wire, &base),
            ) else {
                continue;
            };
            if !same_section(&section, &other) || py::dist(&centre, &other_centre) > INTERVAL_TOL {
                continue;
            }
            let frame = LocalFrame::canonical(base.run, centre)?;
            let (Some(a), Some(b)) = (
                termination_plane(part, first_normal, &first.wire, &frame),
                termination_plane(part, second_normal, &second.wire, &frame),
            ) else {
                continue;
            };
            let (low, high) = if py::order(b.0, a.0) == Ordering::Less {
                (b, a)
            } else {
                (a, b)
            };
            if high.0 - low.0 <= COORD_FLOOR
                || !void_and_planar_open(ctx, solid, &frame, low, high, &section)
            {
                continue;
            }
            let occurrence = bodies.occurrence(solid, frame, (low.0, high.0), section)?;
            proposals.push(SectionRingProposal {
                occurrence,
                nodes: defining,
                solid,
                constituent: region,
                low_gradient: low.1,
                high_gradient: high.1,
            });
            continue;
        }
        let base = LocalFrame::canonical(first_normal, [0.0; 3])?;
        let (Some((section, centre)), Some((other, _))) = (
            line_section(part, &first.wire, &base),
            line_section(part, &second.wire, &base),
        ) else {
            continue;
        };
        if !same_section(&section, &other) {
            continue;
        }
        let frame = LocalFrame::canonical(base.run, centre)?;
        let (a, b) = (
            dot(first_vertex(part, &first.wire), frame.run),
            dot(first_vertex(part, &second.wire), frame.run),
        );
        let interval = (a.min(b), a.max(b));
        if interval.1 - interval.0 <= COORD_FLOOR
            || !void_and_open(ctx, solid, &frame, interval, &section)
        {
            continue;
        }
        let occurrence = bodies.occurrence(solid, frame, interval, section)?;
        proposals.push(SectionRingProposal {
            occurrence,
            nodes: defining,
            solid,
            constituent: region,
            low_gradient: [0.0, 0.0],
            high_gradient: [0.0, 0.0],
        });
    }
    Ok(proposals)
}

/// The components of *items* under the symmetric relation *adjacency*, each sorted, by their
/// least member (Python leaves both orders unspecified).
pub(crate) fn components(
    items: &BTreeSet<usize>,
    adjacency: &BTreeMap<usize, BTreeSet<usize>>,
) -> Vec<Vec<usize>> {
    let mut unseen = items.clone();
    let mut out = Vec::new();
    while let Some(&start) = unseen.iter().next() {
        unseen.remove(&start);
        let mut component = vec![start];
        let mut frontier = vec![start];
        while let Some(current) = frontier.pop() {
            for &other in adjacency.get(&current).into_iter().flatten() {
                if unseen.remove(&other) {
                    component.push(other);
                    frontier.push(other);
                }
            }
        }
        component.sort_unstable();
        out.push(component);
    }
    out
}

internal_entry! {
/// Every supported line-walled, constant-section void open at both ends
/// (`section_ring_proposals`), sorted by run, interval and origin. `Err` where Python raises: a
/// proposal whose occurrence fails its own validation.
pub fn section_ring_proposals(ctx: &Context<'_>) -> Result<Vec<SectionRingProposal>, SectionError> {
    let part = ctx.part;
    let mut bodies = BodyAdapter::default();
    let planar: Vec<usize> = (0..part.faces.len())
        .filter(|&f| is_planar(part, f))
        .collect();
    // Junction directions, bucketed by their rounding for presentation only.
    let mut direction_pairs: Vec<(V3, Vec<Junction>)> = Vec::new();
    let mut inspected: BTreeSet<(usize, usize)> = BTreeSet::new();
    for &left in &planar {
        for right in part.neighbours(left) {
            let pair = (left.min(right), left.max(right));
            if !is_planar(part, right) || !inspected.insert(pair) {
                continue;
            }
            for edge in part.shared_edges(left, right) {
                if let Some(run) = canonical_run(part, edge) {
                    let key = run.map(|c| py::without_negative_zero(py::round_to(c, 9)));
                    match direction_pairs.iter_mut().find(|(k, _)| *k == key) {
                        Some((_, items)) => items.push((left, right, run)),
                        None => direction_pairs.push((key, vec![(left, right, run)])),
                    }
                }
            }
        }
    }
    let discovered: Vec<Junction> = direction_pairs
        .iter()
        .flat_map(|(_, items)| items.iter().copied())
        .collect();
    let mut keys: Vec<usize> = (0..direction_pairs.len()).collect();
    keys.sort_by(|&a, &b| py::tuple_order(&direction_pairs[a].0, &direction_pairs[b].0));

    let mut proposals: Vec<SectionRingProposal> = Vec::new();
    let mut seen: BTreeSet<Vec<usize>> = BTreeSet::new();
    for key in keys {
        let run = min_run(direction_pairs[key].1.iter().map(|item| item.2)).expect("bucketed");
        let base = LocalFrame::canonical(run, [0.0; 3])?;
        let candidates: Vec<(usize, usize)> = discovered
            .iter()
            .filter(|c| parallel(c.2, base.run))
            .map(|c| (c.0, c.1))
            .collect();
        let walls: BTreeSet<usize> = candidates
            .iter()
            .flat_map(|&(l, r)| [l, r])
            .filter(|&n| normal(part, n).is_some_and(|m| dot(m, base.run).abs() <= DIRECTION_TOL))
            .collect();
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        for &(l, r) in &candidates {
            if !pairs
                .iter()
                .any(|&(a, b)| (a, b) == (l, r) || (a, b) == (r, l))
            {
                pairs.push((l, r));
            }
        }
        let mut pair_lines: BTreeMap<(usize, usize), [f64; 4]> = BTreeMap::new();
        let mut adjacency: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        for (left, right) in pairs {
            if !walls.contains(&left) || !walls.contains(&right) {
                continue;
            }
            let Some(line) = pair_line(part, left, right, &base) else {
                continue;
            };
            let (Some(ls), Some(rs)) = (
                face_interval(part, left, base.run),
                face_interval(part, right, base.run),
            ) else {
                continue;
            };
            if [ls.0, ls.1, rs.0, rs.1]
                .iter()
                .zip([line[2], line[3], line[2], line[3]])
                .any(|(actual, expected)| (actual - expected).abs() > INTERVAL_TOL)
            {
                continue;
            }
            pair_lines.insert((left.min(right), left.max(right)), line);
            adjacency.entry(left).or_default().insert(right);
            adjacency.entry(right).or_default().insert(left);
        }
        for component in components(&walls, &adjacency) {
            if component.len() < 3
                || component.iter().any(|n| {
                    adjacency
                        .get(n)
                        .map_or(0, |a| a.iter().filter(|m| component.contains(m)).count())
                        != 2
                })
            {
                continue;
            }
            if seen.contains(&component) {
                continue;
            }
            let Some(solid) = common_valid_solid(part, &component) else {
                continue;
            };
            let order = ordered_cycle(&component, &adjacency, &pair_lines)?;
            let lines: Vec<[f64; 4]> = (0..order.len())
                .map(|i| {
                    let (a, b) = (order[i], order[(i + 1) % order.len()]);
                    pair_lines[&(a.min(b), a.max(b))]
                })
                .collect();
            let Some(spans) = order
                .iter()
                .map(|&n| face_interval(part, n, base.run))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let (low, high) = spans[0];
            if spans
                .iter()
                .any(|s| (s.0 - low).abs() > INTERVAL_TOL || (s.1 - high).abs() > INTERVAL_TOL)
            {
                continue;
            }
            let Ok((frame, section)) =
                PlanarSection::polygon(&lines.iter().map(|l| [l[0], l[1]]).collect::<Vec<_>>())
                    .and_then(|raw| centred(&base, &raw))
            else {
                continue;
            };
            if !observed_planar_ring_ends(part, &order, base.run, (low, high), solid) {
                continue;
            }
            if !void_and_open(ctx, solid, &frame, (low, high), &section) {
                continue;
            }
            seen.insert(component);
            let occurrence = bodies.occurrence(solid, frame, (low, high), section)?;
            proposals.push(SectionRingProposal {
                occurrence,
                nodes: order,
                solid,
                constituent: BTreeSet::new(),
                low_gradient: [0.0, 0.0],
                high_gradient: [0.0, 0.0],
            });
        }
    }
    let mut more = enclosure_proposals(ctx, &mut bodies)?;
    more.extend(treated_entry_proposals(ctx, &mut bodies)?);
    for proposal in more {
        let mut identity = proposal.nodes.clone();
        identity.sort_unstable();
        if seen.insert(identity) {
            proposals.push(proposal);
        }
    }
    let key = |p: &SectionRingProposal| {
        let o = &p.occurrence;
        let (lo, hi) = o.run_interval();
        let mut k = o.frame().run.to_vec();
        k.extend([lo, hi]);
        k.extend(o.frame().origin);
        k
    };
    proposals.sort_by(|a, b| py::tuple_order(&key(a), &key(b)));
    Ok(proposals)
}
}
