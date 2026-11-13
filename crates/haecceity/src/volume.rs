//! The volume a probe region shares with a solid — what the Python implementation asks
//! `BRepAlgoAPI_Common` for (`_volume_probe.probe_volume`), without a boolean.
//!
//! The shared volume is the integral over height of the shared area of each slice, and that
//! area the integral across the slice of the shared length of lines within it. The length on
//! each line is exact (ray crossings). It bends only where a line passes a corner of the
//! solid ∩ probe arrangement, so both integrals are split at those corners. In height: the two
//! shapes' vertices and edge turning points, and where either shape's edges pierce the other's
//! faces. Across a slice: where either shape's edges cross it, where the line two planar faces
//! (one from each shape) share crosses it, and where a planar face of one shape cuts a curved
//! face of the other within it. Between those breaks, with planar faces only, the length is
//! linear and the area quadratic, so two-point Gauss is exact; curved faces refine adaptively.
//! The two exact shortcuts Python takes come first.

use std::f64::consts::{PI, TAU};

use super::brep::Edge;
use super::classify::{Classifier, State};
use super::geom::{self, Bounds, COORD_FLOOR, Curve, Frame, Surface, V3};
use super::rays::{RayCaster, ray_meets_box};

/// A probe region: an axis-aligned box, a solid (its part's first solid), or a planar region
/// swept along a coordinate axis (`Solid.extrude(face, ...)`).
pub enum Probe<'a> {
    Box(Bounds),
    Solid(RayCaster<'a>),
    Prism(Prism),
}

/// A planar region normal to coordinate *axis*, swept from `lo` to `hi` along it. The region is
/// given by its boundary loops (outer and holes alike, any orientation: inside is decided by
/// crossing parity); only the two coordinates across *axis* of their points are read.
pub struct Prism {
    pub axis: usize,
    pub lo: f64,
    pub hi: f64,
    pub loops: Vec<Vec<PrismEdge>>,
}

/// One boundary edge of a prism's region, as its samples in order, and whether it is a
/// straight segment.
pub struct PrismEdge {
    pub points: Vec<V3>,
    pub straight: bool,
}

impl Prism {
    /// The two coordinates across the axis.
    fn across(&self) -> (usize, usize) {
        ((self.axis + 1) % 3, (self.axis + 2) % 3)
    }

    fn segments(&self) -> impl Iterator<Item = ([f64; 2], [f64; 2])> + '_ {
        let (u, v) = self.across();
        self.loops
            .iter()
            .flatten()
            .flat_map(|e| e.points.windows(2))
            .map(move |w| ([w[0][u], w[0][v]], [w[1][u], w[1][v]]))
    }

    /// The region's area: the largest loop less the others (its holes).
    pub fn area(&self) -> f64 {
        let (u, v) = self.across();
        let mut areas: Vec<f64> = self
            .loops
            .iter()
            .map(|lp| {
                let twice: f64 = lp
                    .iter()
                    .flat_map(|e| e.points.windows(2))
                    .map(|w| w[0][u] * w[1][v] - w[1][u] * w[0][v])
                    .sum();
                0.5 * twice.abs()
            })
            .collect();
        areas.sort_by(|a, b| b.total_cmp(a));
        areas
            .first()
            .map_or(0.0, |outer| outer - areas[1..].iter().sum::<f64>())
    }

    fn bounds(&self) -> Bounds {
        let mut b = Bounds::empty();
        for e in self.loops.iter().flatten() {
            for p in &e.points {
                b.add(*p);
            }
        }
        b.min[self.axis] = self.lo;
        b.max[self.axis] = self.hi;
        b
    }

    /// Where the line `o + t d` crosses the region's boundary, across the axis, sorted, over
    /// the whole line. Half-open at segment ends, so a line through a vertex crosses once.
    fn crossings(&self, o: [f64; 2], d: [f64; 2]) -> Vec<f64> {
        let side = |p: [f64; 2]| d[0] * (p[1] - o[1]) - d[1] * (p[0] - o[0]);
        let dd = d[0] * d[0] + d[1] * d[1];
        let mut ts: Vec<f64> = self
            .segments()
            .filter_map(|(p, q)| {
                let (s0, s1) = (side(p), side(q));
                if (s0 > 0.0) == (s1 > 0.0) {
                    return None;
                }
                let k = s0 / (s0 - s1);
                let x = [p[0] + k * (q[0] - p[0]), p[1] + k * (q[1] - p[1])];
                Some(((x[0] - o[0]) * d[0] + (x[1] - o[1]) * d[1]) / dd)
            })
            .collect();
        ts.sort_by(f64::total_cmp);
        ts
    }

    /// The prism's intervals along a line, within [0, reach].
    fn intervals(&self, origin: V3, dir: V3, reach: f64) -> Vec<(f64, f64)> {
        let a = self.axis;
        let (mut lo, mut hi) = (0.0f64, reach);
        if dir[a].abs() < 1e-300 {
            if origin[a] < self.lo || origin[a] > self.hi {
                return Vec::new();
            }
        } else {
            let (p, q) = (
                (self.lo - origin[a]) / dir[a],
                (self.hi - origin[a]) / dir[a],
            );
            (lo, hi) = (lo.max(p.min(q)), hi.min(p.max(q)));
        }
        if lo >= hi {
            return Vec::new();
        }
        let (u, v) = self.across();
        let (o, d) = ([origin[u], origin[v]], [dir[u], dir[v]]);
        if d[0].hypot(d[1]) < 1e-12 {
            // Along the axis: inside the region or not, by the parity of a ray across it.
            let beyond = self
                .crossings(o, [1.0, 0.0])
                .iter()
                .filter(|&&t| t > 0.0)
                .count();
            return if beyond % 2 == 1 {
                vec![(lo, hi)]
            } else {
                Vec::new()
            };
        }
        self.crossings(o, d)
            .as_chunks::<2>()
            .0
            .iter()
            .filter_map(|&[enter, leave]| {
                let (a, b) = (enter.max(lo), leave.min(hi));
                (a < b).then_some((a, b))
            })
            .collect()
    }
}

/// Halvings allowed within one interval between breaks, where curved faces bend the length.
const MAX_DEPTH: usize = 5;

/// The volume *probe* shares with the material classified by *solid*.
///
/// Emptiness and fullness are decided exactly, on the probe drawn in by `COORD_FLOOR`: if no
/// material reaches that interior the probe is empty, and if no air does it is full. Material
/// or air that reaches no deeper than the coordinate tolerance is where the probe's boundary
/// lies on the solid's within the file's own precision, and is what Python's 1e-6 insets exist
/// to ignore. Anything between is measured on the probe itself.
///
/// `None` when the question cannot be answered: a line the rays cannot resolve (a face they
/// cannot intersect, or crossings whose parity stays odd), a probe clear of every face whose
/// centre the classifier cannot place, or a solid probe whose own volume is unknown. An
/// unanswered line is never read as air.
pub fn common_volume(solid: &Classifier<'_>, probe: &Probe<'_>) -> Option<f64> {
    let ([material, air], _) = shared(solid, probe, COORD_FLOOR, &|_| false)?;
    if material == 0.0 {
        Some(0.0)
    } else if air == 0.0 {
        probe_volume(probe)
    } else {
        Some(shared(solid, probe, 0.0, &|_| false)?.0[0])
    }
}

/// Whether *solid*'s material fills at most *limit* of *probe*'s volume: [`common_volume`]
/// over the probe's own (positive) volume at most *limit*, an unanswered volume never.
///
/// The same answer, measured only as far as it needs: each pass stops once the volume measured
/// so far settles it (material found where fullness would already exceed *limit*, both kinds
/// otherwise, or more exact material than *limit* allows), since a running total only grows.
/// A pass stopped short has not met every line, and a line unresolved further on would have
/// left the volume unanswered:
/// the first pass is finished before an at-most answer stands on it.
pub fn fills_at_most(solid: &Classifier<'_>, probe: &Probe<'_>, limit: f64) -> bool {
    let Some(whole) = probe_volume(probe).filter(|&v| v > 0.0) else {
        return false;
    };
    let at_most = |volume: Option<f64>| volume.is_some_and(|v| v / whole <= limit);
    let Some(([material, air], short)) = shared(solid, probe, COORD_FLOOR, &|[m, a]| {
        m > 0.0 && (limit < 1.0 || a > 0.0)
    }) else {
        return false;
    };
    if material == 0.0 {
        return at_most(Some(0.0));
    }
    if air == 0.0 && !short {
        return at_most(probe_volume(probe));
    }
    match shared(solid, probe, 0.0, &|[m, _]| m / whole > limit) {
        Some(([m, _], false)) => {
            if !at_most(Some(m)) {
                return false;
            }
            if short {
                // A prefix containing material cannot be empty, but it need not
                // have reached air or an unanswered line. Before accepting, finish
                // the inset pass and retain its exact empty/full conventions.
                match shared(solid, probe, COORD_FLOOR, &|_| false) {
                    Some(([0.0, _], _)) => return at_most(Some(0.0)),
                    Some(([_, 0.0], _)) => return at_most(probe_volume(probe)),
                    Some(_) => {}
                    None => return false,
                }
            }
            true
        }
        // Unanswered, or the exact-pass prefix strictly exceeds *limit*. Nonnegative
        // lengths and positive weights cannot reduce it; equality does not reject.
        _ => false,
    }
}

/// The probe's own volume; `None` for a solid probe whose mass cannot be integrated.
pub fn probe_volume(probe: &Probe<'_>) -> Option<f64> {
    match probe {
        Probe::Box(b) => {
            let s = geom::sub(b.max, b.min);
            Some(s[0] * s[1] * s[2])
        }
        Probe::Solid(rays) => rays.part.solid_mass(0).map(|m| m.0),
        Probe::Prism(prism) => Some(prism.area() * (prism.hi - prism.lo)),
    }
}

impl Probe<'_> {
    fn bounds(&self) -> Bounds {
        match self {
            Probe::Box(b) => *b,
            Probe::Solid(rays) => rays.bounds(),
            Probe::Prism(prism) => prism.bounds(),
        }
    }

    /// The probe's intervals along a line that starts outside it; `None` where a solid probe's
    /// rays cannot resolve the line.
    fn intervals(&self, origin: V3, dir: V3, reach: f64) -> Option<Vec<(f64, f64)>> {
        Some(match self {
            Probe::Box(b) => {
                // Slabs: the parameter range inside every axis's pair of planes.
                let (mut lo, mut hi) = (0.0f64, reach);
                for i in 0..3 {
                    if dir[i].abs() < 1e-300 {
                        if origin[i] < b.min[i] || origin[i] > b.max[i] {
                            return Some(Vec::new());
                        }
                        continue;
                    }
                    let (a, c) = (
                        (b.min[i] - origin[i]) / dir[i],
                        (b.max[i] - origin[i]) / dir[i],
                    );
                    (lo, hi) = (lo.max(a.min(c)), hi.min(a.max(c)));
                }
                if lo < hi { vec![(lo, hi)] } else { Vec::new() }
            }
            Probe::Solid(rays) => intervals(rays, origin, dir, reach)?,
            Probe::Prism(prism) => prism.intervals(origin, dir, reach),
        })
    }

    /// Where a straight segment p–q pierces the probe's faces; `None` where a solid probe's
    /// rays cannot resolve the segment.
    fn pierce(&self, p: V3, q: V3) -> Option<Vec<V3>> {
        let Some(dir) = geom::unit(geom::sub(q, p)) else {
            return Some(Vec::new());
        };
        let span = geom::dist(p, q);
        let ts: Vec<f64> = match self {
            Probe::Box(_) | Probe::Prism(_) => self
                .intervals(p, dir, span)?
                .into_iter()
                .flat_map(|(a, b)| [a, b])
                .collect(),
            Probe::Solid(rays) => rays.hits(p, dir, span)?.iter().map(|h| h.t).collect(),
        };
        Some(
            ts.into_iter()
                .map(|t| geom::add(p, geom::scale(dir, t)))
                .collect(),
        )
    }
}

/// One shape's part in the arrangement near the region: its edges, the planes of its planar
/// faces (with a box holding the face, and the face of a solid), the curved faces of a solid,
/// and whether it has curved faces there (a prism's curved sides are not faces).
struct Side<'a> {
    edges: Vec<Polyline<'a>>,
    planes: Vec<(V3, f64, Bounds, Option<usize>)>,
    curved_faces: Vec<usize>,
    curved: bool,
    /// The solid's rays, whose faces are the ones named.
    rays: Option<&'a RayCaster<'a>>,
}

/// An edge as its samples, with its curve when crossings must be solved on it, and whether
/// it is a straight segment (whose piercing points are exact corners of the arrangement).
struct Polyline<'a> {
    points: std::borrow::Cow<'a, [V3]>,
    curve: Option<&'a Curve>,
    straight: bool,
}

impl<'a> Side<'a> {
    fn of_solid(rays: &'a RayCaster<'a>, region: &Bounds) -> Self {
        let faces = faces_meeting(rays, region);
        let part = rays.part;
        let mut ids: Vec<usize> = faces.iter().flat_map(|&f| part.face_edges(f)).collect();
        ids.sort_unstable();
        ids.dedup();
        let edges = ids
            .into_iter()
            .map(|e| Polyline::of(&part.edges[e]))
            .collect();
        let mut planes = Vec::new();
        let mut curved_faces = Vec::new();
        for &f in &faces {
            match part.faces[f].surface {
                Surface::Plane { frame } => planes.push((
                    frame.z,
                    geom::dot(frame.z, frame.origin),
                    rays.face_boxes[f],
                    Some(f),
                )),
                _ => curved_faces.push(f),
            }
        }
        Side {
            edges,
            planes,
            curved: !curved_faces.is_empty(),
            curved_faces,
            rays: Some(rays),
        }
    }

    fn of_box(b: &Bounds) -> Self {
        let corner =
            |i: usize| [0, 1, 2].map(|k| if i >> k & 1 == 0 { b.min[k] } else { b.max[k] });
        let mut edges = Vec::new();
        for i in 0..8 {
            for k in 0..3 {
                if i >> k & 1 == 0 {
                    let points = vec![corner(i), corner(i | 1 << k)];
                    edges.push(Polyline {
                        points: points.into(),
                        curve: None,
                        straight: true,
                    });
                }
            }
        }
        let unit = |k: usize| [0, 1, 2].map(|j| if j == k { 1.0 } else { 0.0 });
        let planes = (0..3)
            .flat_map(|k| [(unit(k), b.min[k], *b, None), (unit(k), b.max[k], *b, None)])
            .collect();
        Side {
            edges,
            planes,
            curved_faces: Vec::new(),
            curved: false,
            rays: None,
        }
    }
}

impl Side<'_> {
    /// A prism's edges at both ends and along the axis from each edge's start, the planes of
    /// its ends and straight sides, and whether it has curved sides.
    fn of_prism(prism: &Prism) -> Self {
        let a = prism.axis;
        let at = |p: V3, x: f64| {
            let mut q = p;
            q[a] = x;
            q
        };
        let unit_axis = [0, 1, 2].map(|k| if k == a { 1.0 } else { 0.0 });
        let mut edges = Vec::new();
        let b = prism.bounds();
        let mut planes = vec![
            (unit_axis, prism.lo, b, None),
            (unit_axis, prism.hi, b, None),
        ];
        let mut curved = false;
        for e in prism.loops.iter().flatten() {
            let (Some(&first), Some(&last)) = (e.points.first(), e.points.last()) else {
                continue;
            };
            for x in [prism.lo, prism.hi] {
                let points: Vec<V3> = e.points.iter().map(|&p| at(p, x)).collect();
                edges.push(Polyline {
                    points: points.into(),
                    curve: None,
                    straight: e.straight,
                });
            }
            edges.push(Polyline {
                points: vec![at(first, prism.lo), at(first, prism.hi)].into(),
                curve: None,
                straight: true,
            });
            if !e.straight {
                curved = true;
            } else if let Some(n) = geom::unit(geom::cross(geom::sub(last, first), unit_axis)) {
                planes.push((n, geom::dot(n, at(first, prism.lo)), b, None));
            }
        }
        Side {
            edges,
            planes,
            curved_faces: Vec::new(),
            curved,
            rays: None,
        }
    }
}

impl<'a> Polyline<'a> {
    fn of(edge: &'a Edge) -> Self {
        // Conics invert in closed form; other curves keep their samples' crossing, within the
        // chordal error, and the adaptive rule absorbs the difference.
        let conic = matches!(edge.curve, Curve::Circle { .. } | Curve::Ellipse { .. });
        let curve = conic.then_some(&edge.curve);
        let straight = matches!(edge.curve, Curve::Line { .. });
        Polyline {
            points: edge.samples.as_slice().into(),
            curve,
            straight,
        }
    }
}

/// The material and the air shared with the probe drawn in by *inset* on every side, or
/// `None` when a line through it, or the state of a probe clear of every face, is unresolved;
/// with whether it stopped short, at the first slab whose running total satisfies *stop*.
fn shared(
    solid: &Classifier<'_>,
    probe: &Probe<'_>,
    inset: f64,
    stop: &dyn Fn(Pair) -> bool,
) -> Option<(Pair, bool)> {
    let rays = solid.rays();
    let (sb, pb) = (rays.bounds(), probe.bounds());
    let whole = probe_volume(probe)?;
    let Some(region) = overlap(&sb, &pb) else {
        return Some(([0.0, whole], false));
    };
    // A probe whose box meets no face's box is wholly inside or wholly outside. Its centre lies
    // clear of every face, so `On` there is as unresolved as `Unknown`.
    if !rays.any_face_box_meets(&pb) {
        return match solid.classify(pb.centre()) {
            State::In => Some(([whole, 0.0], false)),
            State::Out => Some(([0.0, whole], false)),
            State::On | State::Unknown => None,
        };
    }
    let mine = Side::of_solid(rays, &region);
    let theirs = match probe {
        Probe::Box(b) => Side::of_box(b),
        Probe::Solid(p) => Side::of_solid(p, &region),
        Probe::Prism(p) => Side::of_prism(p),
    };
    // Slices across the probe's longest side, lines along the next.
    let size = geom::sub(pb.max, pb.min);
    let z = (0..3)
        .max_by(|&i, &j| size[i].total_cmp(&size[j]))
        .unwrap_or(2);
    let unit = |i: usize| [0, 1, 2].map(|k| if k == i { 1.0 } else { 0.0 });
    let frame = Frame {
        origin: [0.0; 3],
        x: unit((z + 1) % 3),
        y: unit((z + 2) % 3),
        z: unit(z),
    };
    let (h0, h1) = (region.min[z] + inset, region.max[z] - inset);
    let across = (
        region.min[(z + 1) % 3] + inset,
        region.max[(z + 1) % 3] - inset,
    );
    if h1 <= h0 || across.1 <= across.0 {
        return Some(([0.0, 0.0], false));
    }
    let y = (z + 2) % 3;
    let start = sb.min[y].min(pb.min[y]) - 1.0;
    let reach = sb.max[y].max(pb.max[y]) + 1.0 - start;
    // The integrators take plain lengths: a line either shape cannot resolve is noted here, and
    // the whole answer refused.
    let unresolved = std::cell::Cell::new(false);
    let length = |h: f64, s: f64| {
        // One unanswered line already refuses this whole pass. Later samples
        // cannot restore an answer, and the accumulated numbers will be discarded:
        // these placeholder zeros must never become a published air/material result.
        if unresolved.get() {
            return [0.0, 0.0];
        }
        #[cfg(test)]
        tests::SHARED_LINES.with(|count| count.set(count.get() + 1));
        let origin = geom::add(
            geom::add(geom::scale(frame.z, h), geom::scale(frame.x, s)),
            geom::scale(frame.y, start),
        );
        let (Some(material), Some(probed)) = (
            intervals(rays, origin, frame.y, reach),
            probe.intervals(origin, frame.y, reach),
        ) else {
            unresolved.set(true);
            return [0.0, 0.0];
        };
        let inside: Vec<(f64, f64)> = probed
            .into_iter()
            .map(|(a, b)| (a + inset, b - inset))
            .filter(|(a, b)| a < b)
            .collect();
        [
            overlap_length(&material, &inside),
            overlap_length(&gaps(&material, reach), &inside),
        ]
    };
    let rule = if mine.curved || theirs.curved {
        Rule::Adaptive(1e-10 * whole)
    } else {
        Rule::Exact
    };

    // Breaks in height: both shapes' vertices and turning points, and each shape's straight
    // edges piercing the other's faces.
    let at = |v: V3, axis: V3| geom::dot(v, axis);
    let mut hs: Vec<f64> = Vec::new();
    for e in mine.edges.iter().chain(&theirs.edges) {
        hs.extend(turning_points(&e.points, frame.z));
    }
    for e in mine.edges.iter().filter(|e| e.straight) {
        for w in e.points.windows(2) {
            hs.extend(probe.pierce(w[0], w[1])?.iter().map(|v| at(*v, frame.z)));
        }
    }
    for e in theirs.edges.iter().filter(|e| e.straight) {
        for w in e.points.windows(2) {
            if let Some(dir) = geom::unit(geom::sub(w[1], w[0])) {
                let hits = rays.hits(w[0], dir, geom::dist(w[0], w[1]))?;
                hs.extend(
                    hits.iter()
                        .map(|hit| at(geom::add(w[0], geom::scale(dir, hit.t)), frame.z)),
                );
            }
        }
    }
    // Breaks across a slice: both shapes' edges crossing it, the lines shared by a plane of
    // each crossing it, and the curves where a plane of one meets a curved face of the other:
    // a sliver of material between such a curve and the nearest edge can be narrower than the
    // spacing of the Gauss points and be missed by both rules.
    let area = |h: f64| {
        if unresolved.get() {
            return [0.0, 0.0];
        }
        let mut ss: Vec<f64> = Vec::new();
        for e in mine.edges.iter().chain(&theirs.edges) {
            ss.extend(crossings(e, frame.z, h, frame.x));
        }
        for &(n1, d1, ..) in &mine.planes {
            for &(n2, d2, ..) in &theirs.planes {
                ss.extend(three_planes(n1, d1, n2, d2, frame.z, h).map(|v| at(v, frame.x)));
            }
        }
        for (curved, flat) in [(&mine, &theirs), (&theirs, &mine)] {
            ss.extend(
                plane_cuts(curved, flat, frame.z, h, &region)
                    .into_iter()
                    .map(|v| at(v, frame.x)),
            );
        }
        integrate(&|s| length(h, s), across, ss, rule.per(h1 - h0))
    };
    let total = integrate_until(&area, (h0, h1), hs, rule, &|total| {
        unresolved.get() || stop(total)
    });
    (!unresolved.get()).then_some(total)
}

/// Where a plane of *flat* meets a curved face of *curved* in the slice `z · p = h`: the line
/// the plane shares with the slice, cut by the face's surface, where both faces claim the point.
fn plane_cuts(curved: &Side<'_>, flat: &Side<'_>, z: V3, h: f64, region: &Bounds) -> Vec<V3> {
    let Some(rays) = curved.rays else {
        return Vec::new();
    };
    let reach = region.diagonal();
    let mut out = Vec::new();
    for &(n, d, plane_box, plane_face) in &flat.planes {
        let Some(dir) = geom::unit(geom::cross(n, z)) else {
            continue;
        };
        let Some(mid) = three_planes(n, d, z, h, dir, geom::dot(region.centre(), dir)) else {
            continue;
        };
        let origin = geom::sub(mid, geom::scale(dir, reach));
        if !ray_meets_box(origin, dir, 2.0 * reach, &plane_box, COORD_FLOOR) {
            continue;
        }
        for &f in &curved.curved_faces {
            if !ray_meets_box(origin, dir, 2.0 * reach, &rays.face_boxes[f], COORD_FLOOR) {
                continue;
            }
            // A line that grazes the surface lies along it (a face lying on the plane): there the
            // faces coincide rather than cut.
            let Some((ts, false)) = rays.part.faces[f]
                .surface
                .ray_hits(origin, dir, 2.0 * reach)
            else {
                continue;
            };
            for t in ts {
                let v = geom::add(origin, geom::scale(dir, t));
                let on_plane = match (flat.rays, plane_face) {
                    (Some(owner), Some(g)) => owner.claims(g, v),
                    _ => plane_box.contains(v, COORD_FLOOR),
                };
                if on_plane && rays.claims(f, v) {
                    out.push(v);
                }
            }
        }
    }
    out
}

/// The point three planes `n · p = d` share, if they meet in one.
fn three_planes(n1: V3, d1: f64, n2: V3, d2: f64, n3: V3, d3: f64) -> Option<V3> {
    let det = geom::dot(n1, geom::cross(n2, n3));
    if det.abs() <= 1e-12 {
        return None;
    }
    let p = geom::add(
        geom::add(
            geom::scale(geom::cross(n2, n3), d1),
            geom::scale(geom::cross(n3, n1), d2),
        ),
        geom::scale(geom::cross(n1, n2), d3),
    );
    Some(geom::scale(p, 1.0 / det))
}

fn overlap(a: &Bounds, b: &Bounds) -> Option<Bounds> {
    let min = [0, 1, 2].map(|i| a.min[i].max(b.min[i]));
    let max = [0, 1, 2].map(|i| a.max[i].min(b.max[i]));
    (0..3)
        .all(|i| min[i] < max[i])
        .then_some(Bounds { min, max })
}

/// The faces whose boxes reach the region: only they can bend the length inside it.
fn faces_meeting(rays: &RayCaster<'_>, region: &Bounds) -> Vec<usize> {
    let meets = |b: &Bounds| (0..3).all(|i| b.min[i] <= region.max[i] && region.min[i] <= b.max[i]);
    rays.faces
        .iter()
        .copied()
        .filter(|&f| meets(&rays.face_boxes[f]))
        .collect()
}

/// The material intervals along a ray that starts outside the solid, by the parity of its
/// crossings. Crossings closer than the kernel can tell apart are one (a ray through an edge
/// meets both faces there); a line still left odd (a graze) is nudged off it, first by
/// nanometres, then by up to the coordinate floor: a line that close to a vertex or edge
/// passes where the file itself does not place the boundary (vertices can lie off their curves:
/// 3e-8 on the tilted pocket parts, `tests/tilted_probe.rs`), and the faces there can each put
/// the crossing outside their trims, or each claim it. Between breaks the length varies
/// continuously, so a line that near stands for the one asked (breaks closer than the floor put
/// Gauss points that near a corner: a probe inset 1e-6 from a face's plane has its vertices
/// 1e-6 from the face's). The nudge is square to the line, so the hits keep their place along
/// it and line up with the other shape's unnudged intervals. `None` when a face the line meets
/// cannot be intersected, or the parity is still odd after the nudges.
fn intervals(rays: &RayCaster<'_>, origin: V3, dir: V3, reach: f64) -> Option<Vec<(f64, f64)>> {
    // A fixed skew direction with its part along the line removed; the second candidate
    // serves a line running along the first.
    let square = |v: V3| geom::sub(v, geom::scale(dir, geom::dot(v, dir)));
    let side = [[1.0, 2.0, 3.0], [3.0, -1.0, 2.0]]
        .map(square)
        .into_iter()
        .max_by(|a, b| geom::norm(*a).total_cmp(&geom::norm(*b)))
        .and_then(geom::unit)?;
    for nudge in [0.0, 1e-9, 2e-9, 3e-9, 1e-7, 3e-7, COORD_FLOOR] {
        let o = geom::add(origin, geom::scale(side, nudge));
        let hits = rays.trimmed_hits(o, dir, reach)?;
        let mut ts: Vec<f64> = Vec::new();
        for h in hits {
            if ts.last().is_none_or(|&last| h.t - last > 1e-9) {
                ts.push(h.t);
            }
        }
        if ts.len().is_multiple_of(2) {
            return Some(ts.chunks(2).map(|c| (c[0], c[1])).collect());
        }
    }
    None
}

/// The total length the two sets of intervals share. A shared stretch shorter than a nanometre
/// is two coincident ends (a probe face lying on a solid face) seen through rounding, and shares
/// nothing.
fn overlap_length(a: &[(f64, f64)], b: &[(f64, f64)]) -> f64 {
    let mut total = 0.0;
    for &(a0, a1) in a {
        for &(b0, b1) in b {
            let shared = a1.min(b1) - a0.max(b0);
            if shared > 1e-9 {
                total += shared;
            }
        }
    }
    total
}

/// The stretches of [0, reach] the intervals leave uncovered.
fn gaps(intervals: &[(f64, f64)], reach: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut from = 0.0;
    for &(a, b) in intervals {
        out.push((from, a));
        from = b;
    }
    out.push((from, reach));
    out
}

/// A polyline's ends and turning points along *axis*: where its crossings of a plane normal to
/// *axis* appear, vanish or swap (an edge's samples include a circle's extremes).
fn turning_points(points: &[V3], axis: V3) -> Vec<f64> {
    let c: Vec<f64> = points.iter().map(|p| geom::dot(*p, axis)).collect();
    let mut out = vec![c[0], c[c.len() - 1]];
    // Where the direction strictly reverses; a run level along the axis is no turn.
    let mut rising: Option<bool> = None;
    for w in c.windows(2) {
        if w[1] == w[0] {
            continue;
        }
        let up = w[1] > w[0];
        if rising.is_some_and(|r| r != up) {
            out.push(w[0]);
        }
        rising = Some(up);
    }
    out
}

/// Where an edge crosses the plane `axis · p = x`, as coordinates along *across*: found between
/// its samples, then, on a curve, solved on the curve itself.
fn crossings(edge: &Polyline<'_>, axis: V3, x: f64, across: V3) -> Vec<f64> {
    let mut out = Vec::new();
    for w in edge.points.windows(2) {
        let (p, q) = (w[0], w[1]);
        let (dp, dq) = (geom::dot(p, axis) - x, geom::dot(q, axis) - x);
        if dp * dq > 0.0 || dp == dq {
            continue;
        }
        let linear = geom::add(p, geom::scale(geom::sub(q, p), dp / (dp - dq)));
        let at = match edge.curve {
            None => linear,
            Some(curve) => on_curve(curve, p, q, axis, x).unwrap_or(linear),
        };
        out.push(geom::dot(at, across));
    }
    out
}

/// The point of *curve* between its points *p* and *q* where `axis · point = x`, by bisection.
fn on_curve(curve: &Curve, p: V3, q: V3, axis: V3, x: f64) -> Option<V3> {
    let (mut t0, mut t1) = (curve.parameter(p), curve.parameter(q));
    // A closed curve's parameter wraps; the samples are close, so take the short way round.
    if matches!(curve, Curve::Circle { .. } | Curve::Ellipse { .. }) && (t1 - t0).abs() > PI {
        t1 += if t1 < t0 { TAU } else { -TAU };
    }
    let f = |t: f64| geom::dot(curve.value(t), axis) - x;
    let mut f0 = f(t0);
    if f0 * f(t1) > 0.0 {
        return None;
    }
    for _ in 0..60 {
        let tm = 0.5 * (t0 + t1);
        let fm = f(tm);
        if fm * f0 <= 0.0 {
            t1 = tm;
        } else {
            (t0, f0) = (tm, fm);
        }
    }
    Some(curve.value(0.5 * (t0 + t1)))
}

/// How an interval between breaks is integrated: exactly by two-point Gauss (planar faces and a
/// straight outline), or by four-point Gauss halved until the halves agree.
#[derive(Clone, Copy)]
enum Rule {
    Exact,
    Adaptive(f64),
}

impl Rule {
    /// The rule for an inner integral, its tolerance shared over an outer extent.
    fn per(self, extent: f64) -> Rule {
        match self {
            Rule::Exact => Rule::Exact,
            Rule::Adaptive(tol) => Rule::Adaptive(tol / extent),
        }
    }
}

const GAUSS4: [(f64, f64); 4] = [
    (-0.861_136_311_594_052_6, 0.347_854_845_137_453_9),
    (-0.339_981_043_584_856_3, 0.652_145_154_862_546_1),
    (0.339_981_043_584_856_3, 0.652_145_154_862_546_1),
    (0.861_136_311_594_052_6, 0.347_854_845_137_453_9),
];

/// ∫ f over [lo, hi], by *rule* on each interval between the *breaks* that fall inside it.
fn integrate(f: &dyn Fn(f64) -> Pair, range: (f64, f64), breaks: Vec<f64>, rule: Rule) -> Pair {
    integrate_until(f, range, breaks, rule, &|_| false).0
}

/// [`integrate`], stopped after the first interval whose running total satisfies *stop*, with
/// whether it stopped short of *hi*. Every length is at least zero and every weight positive,
/// so the running total only grows, rounding included: the whole integral satisfies any test
/// that only a larger total can pass, and a stopped total is the whole one's exact prefix.
fn integrate_until(
    f: &dyn Fn(f64) -> Pair,
    (lo, hi): (f64, f64),
    breaks: Vec<f64>,
    rule: Rule,
    stop: &dyn Fn(Pair) -> bool,
) -> (Pair, bool) {
    if hi <= lo {
        return ([0.0, 0.0], false);
    }
    let mut breaks: Vec<f64> = breaks.into_iter().filter(|&x| lo < x && x < hi).collect();
    breaks.extend([lo, hi]);
    breaks.sort_by(f64::total_cmp);
    breaks.dedup_by(|x, y| *x - *y <= 1e-9 * (hi - lo));
    let mut total = [0.0, 0.0];
    for (at, w) in breaks.windows(2).enumerate() {
        let part = match rule {
            Rule::Exact => gauss2(f, w[0], w[1]),
            Rule::Adaptive(tol) => {
                let share = tol * (w[1] - w[0]) / (hi - lo);
                let (coarse, fine) = (gauss2(f, w[0], w[1]), gauss4(f, w[0], w[1]));
                if (0..2).all(|i| (fine[i] - coarse[i]).abs() <= share) {
                    fine
                } else {
                    adapt(f, w[0], w[1], fine, share, 0)
                }
            }
        };
        total = add(total, part);
        if at + 2 < breaks.len() && stop(total) {
            return (total, true);
        }
    }
    (total, false)
}

/// Material and air, integrated together from the same lines.
type Pair = [f64; 2];

fn add(a: Pair, b: Pair) -> Pair {
    [a[0] + b[0], a[1] + b[1]]
}

fn scaled(a: Pair, k: f64) -> Pair {
    [a[0] * k, a[1] * k]
}

fn gauss2(f: &dyn Fn(f64) -> Pair, x0: f64, x1: f64) -> Pair {
    let (h, c) = (0.5 * (x1 - x0), 0.5 * (x0 + x1));
    let t = h / 3f64.sqrt();
    scaled(add(f(c - t), f(c + t)), h)
}

fn gauss4(f: &dyn Fn(f64) -> Pair, x0: f64, x1: f64) -> Pair {
    let (h, c) = (0.5 * (x1 - x0), 0.5 * (x0 + x1));
    let sum = GAUSS4
        .iter()
        .fold([0.0, 0.0], |acc, &(t, w)| add(acc, scaled(f(c + h * t), w)));
    scaled(sum, h)
}

fn adapt(f: &dyn Fn(f64) -> Pair, x0: f64, x1: f64, whole: Pair, tol: f64, depth: usize) -> Pair {
    let xm = 0.5 * (x0 + x1);
    let (l, r) = (gauss4(f, x0, xm), gauss4(f, xm, x1));
    let both = add(l, r);
    let settled = (0..2).all(|i| (both[i] - whole[i]).abs() <= tol);
    if settled || depth >= MAX_DEPTH {
        return both;
    }
    add(
        adapt(f, x0, xm, l, tol / 2.0, depth + 1),
        adapt(f, xm, x1, r, tol / 2.0, depth + 1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        pub(super) static SHARED_LINES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    #[test]
    fn an_unanswered_line_refuses_the_probe_without_sampling_later_lines() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/rejected_bored_box.step");
        let mut part = crate::read_step_file(&path).unwrap();
        // The probe edges stay outside the block. Its interior integration lines,
        // along x, reach the side whose surface will become unanswerable.
        let probe = Probe::Box(Bounds {
            min: [-16.0, -16.0, 2.0],
            max: [16.0, 16.0, 3.0],
        });
        assert!(common_volume(&Classifier::new(&part), &probe).is_some_and(|v| v > 0.0));
        let side = RayCaster::for_faces(&part, (0..part.faces.len()).collect())
            .hits([20.0, 10.0, 2.5], [-1.0, 0.0, 0.0], 10.0)
            .unwrap()[0];
        assert!((side.t - 5.0).abs() < 1e-9);
        part.faces[side.face].surface = Surface::Other { kind: "TEST" };
        let solid = Classifier::new(&part);
        SHARED_LINES.set(0);
        assert_eq!(common_volume(&solid, &probe), None);
        assert_eq!(
            SHARED_LINES.get(),
            1,
            "the first unanswered line settles refusal"
        );
        for limit in [0.0, 0.5, 1.0] {
            SHARED_LINES.set(0);
            assert!(!fills_at_most(&solid, &probe, limit));
            assert_eq!(SHARED_LINES.get(), 1);
        }
    }

    #[test]
    fn a_sliver_where_a_probe_face_cuts_a_curved_face_is_measured() {
        // Box(30, 30, 20) - Cylinder(5, 20): x, y in [-15, 15], z in [-10, 10], bored along z.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/rejected_bored_box.step");
        let part = crate::read_step_file(&path).unwrap();
        let solid = Classifier::new(&part);
        // A box reaching y = b just under the bore's top, across |x| <= c, where the bore takes
        // in all of y in [3, b], and on by e past -c: the material is the sliver between the
        // bore and the box's face y = b over x in [-c - e, -c], narrower than the Gauss points
        // across the box come to its side. Lines run along y, so the sliver ends where the face
        // y = b cuts the bore, an x no edge marks (before, the probe read as empty).
        let (b, e) = (4.98f64, 0.02);
        let c = (25.0 - b * b).sqrt();
        let probe = Probe::Box(Bounds {
            min: [-c - e, 3.0, -10.0],
            max: [c, b, 10.0],
        });
        let primitive = |x: f64| 0.5 * (x * (25.0 - x * x).sqrt() + 25.0 * (x / 5.0).asin());
        let want = 20.0 * (b * e - (primitive(-c) - primitive(-c - e)));
        let got = common_volume(&solid, &probe).unwrap();
        assert!(want > 0.0);
        assert!((got - want).abs() <= 1e-6 * want, "{got} vs {want}");
    }

    #[test]
    fn fills_at_most_answers_as_the_whole_volume_does() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/rejected_bored_box.step");
        let part = crate::read_step_file(&path).unwrap();
        let solid = Classifier::new(&part);
        let boxed = |min: V3, max: V3| Probe::Box(Bounds { min, max });
        let probes = [
            // In the bore (empty), in the wall (full), across the bore's side, the sliver above,
            // across the whole part and beyond it.
            boxed([-2.0, -2.0, -10.0], [2.0, 2.0, 10.0]),
            boxed([10.0, 10.0, -5.0], [14.0, 14.0, 5.0]),
            boxed([-8.0, -1.0, -10.0], [0.0, 1.0, 10.0]),
            boxed([-4.0, 3.0, -10.0], [4.0, 4.98, 10.0]),
            boxed([-20.0, -20.0, -12.0], [20.0, 20.0, 12.0]),
            boxed([20.0, 20.0, 20.0], [21.0, 21.0, 21.0]),
        ];
        for probe in &probes {
            let whole = probe_volume(probe).unwrap();
            let fraction = common_volume(&solid, probe).unwrap() / whole;
            for limit in [0.0, 1e-9, 0.5 * fraction, fraction, 0.5, 1.0 - 1e-9, 1.0] {
                assert_eq!(
                    fills_at_most(&solid, probe, limit),
                    fraction <= limit,
                    "fraction {fraction}, limit {limit}"
                );
            }
        }
    }

    #[test]
    fn a_material_prefix_can_reject_without_waiting_to_find_air() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/two_solids.step");
        let part = crate::read_step_file(&path).unwrap();
        assert_eq!(part.solids.len(), 2);
        let boxes = [0, 1].map(|i| RayCaster::for_solid(&part, i).bounds());
        let axis = (0..3)
            .find(|&a| boxes[0].max[a] < boxes[1].min[a] || boxes[1].max[a] < boxes[0].min[a])
            .expect("separate solids along a coordinate axis");
        let (first, last) = if boxes[0].min[axis] < boxes[1].min[axis] {
            (boxes[0], boxes[1])
        } else {
            (boxes[1], boxes[0])
        };
        let mut bounds = Bounds::empty();
        for a in 0..3 {
            if a == axis {
                bounds.min[a] = first.centre()[a];
                bounds.max[a] = last.centre()[a];
            } else {
                let lo = first.min[a].max(last.min[a]);
                let hi = first.max[a].min(last.max[a]);
                assert!(lo < hi);
                // A narrow column through material, away from the central bores.
                bounds.min[a] = lo + 0.70 * (hi - lo);
                bounds.max[a] = lo + 0.72 * (hi - lo);
            }
        }
        let probe = Probe::Box(bounds);
        let solid = Classifier::new(&part);
        let Some(([material, air], short)) = shared(&solid, &probe, COORD_FLOOR, &|[m, _]| m > 0.0)
        else {
            panic!("the first material slab must be answerable");
        };
        assert!(short && material > 0.0 && air == 0.0);
        SHARED_LINES.set(0);
        let fraction = common_volume(&solid, &probe).unwrap() / probe_volume(&probe).unwrap();
        let full_queries = SHARED_LINES.get();
        assert!(fraction > 1e-9 && fraction < 1.0);
        SHARED_LINES.set(0);
        assert!(!fills_at_most(&solid, &probe, 1e-9));
        assert!(SHARED_LINES.get() < full_queries);
        for limit in [
            0.0,
            fraction / 2.0,
            fraction,
            fraction.next_up(),
            1.0,
            f64::NAN,
        ] {
            assert_eq!(fills_at_most(&solid, &probe, limit), fraction <= limit);
        }

        // An unknown crossing in the later solid must still prevent acceptance,
        // even though the known material prefix can settle an earlier rejection.
        let ray_axis = (axis + 2) % 3;
        let mut origin = bounds.centre();
        origin[axis] = last.min[axis] + 0.25 * (last.max[axis] - last.min[axis]);
        origin[ray_axis] = last.max[ray_axis] + 1.0;
        let direction = [0, 1, 2].map(|a| if a == ray_axis { -1.0 } else { 0.0 });
        let face = solid
            .rays()
            .hits(origin, direction, last.diagonal() + 2.0)
            .unwrap()[0]
            .face;
        let mut unresolved_part = crate::read_step_file(&path).unwrap();
        unresolved_part.faces[face].surface = Surface::Other { kind: "TEST" };
        let unresolved = Classifier::new(&unresolved_part);
        assert!(
            shared(&unresolved, &probe, COORD_FLOOR, &|[m, _]| m > 0.0)
                .is_some_and(|([m, a], short)| short && m > 0.0 && a == 0.0)
        );
        assert_eq!(common_volume(&unresolved, &probe), None);
        for limit in [1e-9, fraction, 1.0] {
            assert!(!fills_at_most(&unresolved, &probe, limit));
        }
    }

    #[test]
    fn a_full_inset_prefix_keeps_the_original_full_volume_convention() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/rejected_bored_box.step");
        let part = crate::read_step_file(&path).unwrap();
        let solid = Classifier::new(&part);
        // This box is outside the radius-five bore but overlaps its face box.
        // The bore's x extreme splits a wholly material probe into two slabs.
        let probe = Probe::Box(Bounds {
            min: [4.0, 4.0, 1.0],
            max: [6.0, 4.2, 1.1],
        });
        let ([material, air], short) =
            shared(&solid, &probe, COORD_FLOOR, &|[m, _]| m > 0.0).unwrap();
        assert!(short && material > 0.0 && air == 0.0);
        let whole = probe_volume(&probe).unwrap();
        assert_eq!(common_volume(&solid, &probe), Some(whole));
        // Ray parameters are measured from outside the solid. Their subtraction
        // rounds this short axial span below the box's own length: the exact pass
        // alone would wrongly accept its fraction as an upper limit on fullness.
        let measured = shared(&solid, &probe, 0.0, &|_| false).unwrap().0[0] / whole;
        assert!(measured > 0.0 && measured < 1.0, "{measured}");
        assert!(!fills_at_most(&solid, &probe, measured));
        assert!(!fills_at_most(&solid, &probe, 1.0f64.next_down()));
        assert!(fills_at_most(&solid, &probe, 1.0));
    }
}
