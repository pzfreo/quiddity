//! A face in its surface's parameter space: boundary loops unwrapped across periodic
//! parameters (with singular points routed along their parameter line), OpenCascade-compatible
//! parameter ranges, and point containment.

use std::f64::consts::{PI, TAU};

use super::brep::{Face, Loop, Part, Pcurve};
use super::geom::{self, Frame, Surface, V3};
use super::sampling::{edge_interval, extremes_along};

/// The face's boundary as (u, v) polylines.
///
/// Containment is decided by crossing parity, which needs no loop orientation. That matters:
/// OpenCascade re-derives wire orientation from geometry when it reads a file, and the files it
/// writes carry `FACE_BOUND` flags its own reader then corrects, so the flags are not evidence.
/// Parity does need every loop closed in parameter space; [`crate::brep::Part::uv_loops`]
/// closes a loop that runs round a sphere through its pole.
#[derive(Debug)]
pub struct FaceDomain {
    periodic: (bool, bool),
    u_range: (f64, f64),
    v_range: (f64, f64),
    /// The segments binned by the ray's fixed coordinate, so a query tests only those that can
    /// cross its ray.
    index: SegmentIndex,
    /// Per parameter, how it is read: x ↦ (x − start)·factor. A B-spline surface that closes in
    /// a parameter is read as turning once round its span, so the periodic rules serve it.
    scale: [(f64, f64); 2],
}

fn range<'a>(
    points: impl Iterator<Item = &'a (f64, f64)>,
    pick: impl Fn(&(f64, f64)) -> f64,
) -> (f64, f64) {
    points
        .map(pick)
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| {
            (lo.min(x), hi.max(x))
        })
}

impl FaceDomain {
    pub fn new(loops: &[UvLoop], periodic: (bool, bool)) -> Self {
        Self::scaled(loops, periodic, [(0.0, 1.0); 2])
    }

    /// The domain of `loops` given in parameters already read through `scale`.
    fn scaled(loops: &[UvLoop], periodic: (bool, bool), scale: [(f64, f64); 2]) -> Self {
        let all = || loops.iter().flat_map(|l| l.points.iter());
        let (u_range, v_range) = (range(all(), |p| p.0), range(all(), |p| p.1));
        let loops: Vec<Vec<(f64, f64)>> = loops.iter().map(|l| l.points.clone()).collect();
        // The ray runs in +u (swapped) on a face closed in v but open in u, else in +v.
        let swap = periodic.1 && v_range.1 - v_range.0 >= TAU - 1e-9;
        let periodic_a = if swap { true } else { periodic.0 };
        let index = SegmentIndex::new(&loops, swap, periodic_a);
        FaceDomain {
            periodic,
            u_range,
            v_range,
            index,
            scale,
        }
    }

    pub fn u_range(&self) -> (f64, f64) {
        let (start, factor) = self.scale[0];
        (
            start + self.u_range.0 / factor,
            start + self.u_range.1 / factor,
        )
    }

    pub fn v_range(&self) -> (f64, f64) {
        let (start, factor) = self.scale[1];
        (
            start + self.v_range.0 / factor,
            start + self.v_range.1 / factor,
        )
    }

    /// Whether (u, v) lies on the face, by the parity of a parameter-space ray.
    ///
    /// The ray runs in +v. A periodic v needs a finite end outside the face's v range; a
    /// periodic u wraps every segment next to the point. A face closed in v but open in u
    /// casts in +u instead, and a face closed in both is the whole surface.
    pub fn contains(&self, u: f64, v: f64) -> bool {
        let read = |x: f64, (start, factor): (f64, f64)| (x - start) * factor;
        let (u, v) = (read(u, self.scale[0]), read(v, self.scale[1]));
        let (pu, pv) = self.periodic;
        let (vmin, vmax) = self.v_range;
        let (umin, umax) = self.u_range;
        if pv && vmax - vmin >= TAU - 1e-9 {
            if !pu || umax - umin >= TAU - 1e-9 {
                return true;
            }
            let u = umin + (u - umin).rem_euclid(TAU);
            if u > umax {
                return false;
            }
            return self.crossing_parity(v, u, umax + 0.5 * (TAU - (umax - umin)));
        }
        let mut v = v;
        let mut v_end = f64::INFINITY;
        if pv {
            v = vmin + (v - vmin).rem_euclid(TAU);
            if v > vmax {
                return false;
            }
            v_end = vmax + 0.5 * (TAU - (vmax - vmin));
        }
        let u = if pu { u.rem_euclid(TAU) } else { u };
        self.crossing_parity(u, v, v_end)
    }

    /// Parity of the crossings of the parameter-space ray `(a, b) → (a, b_end)` with the loops,
    /// where `a` is each loop point's first coordinate (its second when the index is swapped).
    /// A periodic `a` wraps each segment next to `a`.
    fn crossing_parity(&self, a: f64, b: f64, b_end: f64) -> bool {
        let periodic = self.index.periodic;
        let mut count = 0usize;
        for &s in self.index.candidates(a) {
            // The bin runs from the highest segment down: one wholly below the ray's start
            // cannot cross it above, nor can any after it (the margin far exceeds the rounding
            // of the crossing's `bc`, which lies between the segment's ends).
            let top = self.index.top[s as usize];
            if top + 1e-9 * (1.0 + top.abs()) < b {
                break;
            }
            let [mut a0, b0, mut a1, b1] = self.index.segments[s as usize];
            if periodic {
                let shift = geom::nearest_turn(a0, a) - a0;
                a0 += shift;
                a1 += shift;
            }
            if (a0 > a) == (a1 > a) {
                continue;
            }
            let bc = b0 + (a - a0) / (a1 - a0) * (b1 - b0);
            if bc > b && bc < b_end {
                count += 1;
            }
        }
        count % 2 == 1
    }
}

/// Loop segments, as their ends' `(a, b)` coordinates, binned (about one bin per segment) by
/// their span in the ray's fixed coordinate `a`: over the period when `a` is periodic (a segment
/// spanning more than half a turn goes in every bin, as which turn it is wrapped to depends on
/// the query), else over the loops' range. Bins are padded, so a segment is only ever left out
/// of a bin it cannot cross a ray in. A segment along the ray's direction (one `a` at both
/// ends) crosses no ray and is in no bin; each bin lists its segments from the highest `b` down
/// (a dense boundary along one side of a face, as cgb243's combs have, fills one bin with
/// thousands of segments, most of them below any ray's start).
#[derive(Debug)]
struct SegmentIndex {
    periodic: bool,
    lo: f64,
    width: f64,
    /// Every segment's `[a0, b0, a1, b1]`, loop by loop.
    segments: Vec<[f64; 4]>,
    /// Per bin, the segments (indices into `segments`) that may cross a ray in it, by
    /// decreasing `top`.
    bins: Vec<Vec<u32>>,
    /// Per segment, its higher end's `b`.
    top: Vec<f64>,
}

impl SegmentIndex {
    fn new(loops: &[Vec<(f64, f64)>], swap: bool, periodic: bool) -> Self {
        let a = |p: &(f64, f64)| if swap { p.1 } else { p.0 };
        let segments: Vec<(u32, u32)> = loops
            .iter()
            .enumerate()
            .flat_map(|(l, lp)| (0..lp.len().saturating_sub(1)).map(move |k| (l as u32, k as u32)))
            .collect();
        let (lo, hi) = if periodic {
            (0.0, TAU)
        } else {
            range(loops.iter().flatten(), a)
        };
        let n = segments.len().clamp(1, 4096);
        let width = ((hi - lo) / n as f64).max(1e-300);
        let mut bins = vec![Vec::new(); n];
        let pad = 1e-9 * (1.0 + (hi - lo).abs());
        for (s, &(l, k)) in segments.iter().enumerate() {
            let s = s as u32;
            let lp = &loops[l as usize];
            let (a0, a1) = (a(&lp[k as usize]), a(&lp[k as usize + 1]));
            if a0 == a1 {
                continue;
            }
            let (mut s0, mut s1) = (a0.min(a1), a0.max(a1));
            if periodic {
                if s1 - s0 > std::f64::consts::PI {
                    bins.iter_mut().for_each(|b| b.push(s));
                    continue;
                }
                let base = a0.rem_euclid(TAU) - a0;
                (s0, s1) = (s0 + base, s1 + base);
            }
            let first = ((s0 - pad - lo) / width).floor() as i64;
            let last = ((s1 + pad - lo) / width).floor() as i64;
            let n = n as i64;
            let (first, last) = if periodic {
                (first, last.min(first + n - 1))
            } else {
                (first.max(0), last.min(n - 1))
            };
            for b in first..=last {
                bins[b.rem_euclid(n) as usize].push(s);
            }
        }
        for b in &mut bins {
            b.dedup();
        }
        let pick = |p: &(f64, f64)| if swap { (p.1, p.0) } else { (p.0, p.1) };
        let segments: Vec<[f64; 4]> = segments
            .into_iter()
            .map(|(l, k)| {
                let lp = &loops[l as usize];
                let ((a0, b0), (a1, b1)) = (pick(&lp[k as usize]), pick(&lp[k as usize + 1]));
                [a0, b0, a1, b1]
            })
            .collect();
        let top: Vec<f64> = segments.iter().map(|s| s[1].max(s[3])).collect();
        for b in &mut bins {
            b.sort_by(|&x, &y| top[y as usize].total_cmp(&top[x as usize]));
        }
        SegmentIndex {
            periodic,
            lo,
            width,
            segments,
            bins,
            top,
        }
    }

    /// The segments that may cross a ray at `a`.
    fn candidates(&self, a: f64) -> &[u32] {
        let n = self.bins.len() as i64;
        let x = if self.periodic { a.rem_euclid(TAU) } else { a };
        let b = ((x - self.lo) / self.width).floor() as i64;
        let b = if self.periodic {
            b.rem_euclid(n)
        } else {
            b.clamp(0, n - 1)
        };
        &self.bins[b as usize]
    }
}

/// One loop of a face in its surface's (u, v) space, unwrapped across periodic parameters.
#[derive(Clone, Debug)]
pub struct UvLoop {
    pub points: Vec<(f64, f64)>,
    /// Per edge of the loop, where each run of its regular samples (not at a pole or apex)
    /// starts in `points`: the turn the edge is on there, which continuity alone cannot carry
    /// across a singular point.
    pub anchors: Vec<Vec<(f64, f64)>>,
    /// Whether the boundary itself runs round u (before any closing along a pole).
    pub winds_u: bool,
    /// Whether it runs round v (a torus elbow with no seam edge).
    pub winds_v: bool,
    /// Per point, the edge sample it was inverted from: (the edge's place in the face loop, the
    /// sample's index in the edge's own `samples`). `None` for a point routed along a singular
    /// line or closing the loop; empty for a loop not made from edges.
    pub sources: Vec<Option<(usize, usize)>>,
}

impl Part {
    /// The face's outer loop (`BRepTools::OuterWire`): the first loop, replaced by any later loop
    /// whose parameter-space box contains the current one's (a later tie wins).
    pub fn outer_loop(&self, face: usize) -> Option<usize> {
        self.loop_placement(face).map(|(outer, _)| outer)
    }

    /// The distinct edges of the face's outer loop (`outer_wire().edges()`).
    pub fn outer_edges(&self, face: usize) -> Vec<usize> {
        let Some(outer) = self.outer_loop(face) else {
            return Vec::new();
        };
        let mut edges: Vec<usize> = self.faces[face].loops[outer]
            .edges
            .iter()
            .map(|e| e.0)
            .collect();
        edges.sort_unstable();
        edges.dedup();
        edges
    }

    /// The outer loop and, per loop, the shift (a whole number of periods) that places it in one
    /// parameter period with the others. Loops are first centred in `[0, 2π)` — where
    /// OpenCascade's pcurves sit — to choose the outer loop; every other loop is then moved to
    /// the period nearest the outer loop's centre, so a hole far round a partial face from the
    /// face's first sample still lands inside it.
    pub(super) fn loop_placement(&self, face: usize) -> Option<(usize, Vec<(f64, f64)>)> {
        let loops = self.uv_loops(face)?;
        if loops.is_empty() {
            return None;
        }
        let (pu, pv) = self.faces[face].surface.periodic();
        let boxes: Vec<[f64; 4]> = loops
            .iter()
            .map(|lp| {
                lp.points.iter().fold(
                    [
                        f64::INFINITY,
                        f64::NEG_INFINITY,
                        f64::INFINITY,
                        f64::NEG_INFINITY,
                    ],
                    |b, &(u, v)| [b[0].min(u), b[1].max(u), b[2].min(v), b[3].max(v)],
                )
            })
            .collect();
        let centre = |b: &[f64; 4]| (0.5 * (b[0] + b[1]), 0.5 * (b[2] + b[3]));
        let first: Vec<(f64, f64)> = boxes
            .iter()
            .map(|b| {
                let c = centre(b);
                (into_period(c.0, pu), into_period(c.1, pv))
            })
            .collect();
        let placed = |i: usize| {
            let b = boxes[i];
            [
                b[0] + first[i].0,
                b[1] + first[i].0,
                b[2] + first[i].1,
                b[3] + first[i].1,
            ]
        };
        let mut outer = 0;
        for i in 1..loops.len() {
            let (o, c) = (placed(outer), placed(i));
            if c[0] <= o[0] && c[1] >= o[1] && c[2] <= o[2] && c[3] >= o[3] {
                outer = i;
            }
        }
        let mut anchor = centre(&placed(outer));
        // A seamless torus band can wind round either parameter. Its other coordinate
        // lies on the face's side of the outer loop, which need not be the nearest turn
        // (a band can cover more than half a period).
        let band = |along_v: bool| -> Vec<Option<f64>> {
            let winds = |lp: &UvLoop| if along_v { lp.winds_v } else { lp.winds_u };
            let coord = |p: (f64, f64)| if along_v { p.0 } else { p.1 };
            (0..loops.len())
                .map(|i| {
                    if i == outer || !(pu && pv && winds(&loops[outer]) && winds(&loops[i])) {
                        return None;
                    }
                    let lp = &loops[outer].points;
                    let rising = if along_v {
                        lp.last()?.1 > lp.first()?.1
                    } else {
                        lp.last()?.0 > lp.first()?.0
                    };
                    let left = rising != self.faces[face].reversed;
                    let side = if left == along_v { -1.0 } else { 1.0 };
                    let (from, x) = (coord(anchor), coord(centre(&boxes[i])));
                    Some(from + side * ((x - from) * side).rem_euclid(TAU) - x)
                })
                .collect()
        };
        let (band_u, band_v) = (band(true), band(false));
        if let Some(i) = band_u.iter().position(Option::is_some) {
            anchor.0 = 0.5 * (anchor.0 + centre(&boxes[i]).0 + band_u[i].unwrap_or(0.0));
        }
        if let Some(i) = band_v.iter().position(Option::is_some) {
            anchor.1 = 0.5 * (anchor.1 + centre(&boxes[i]).1 + band_v[i].unwrap_or(0.0));
        }
        let shifts = (0..loops.len())
            .map(|i| {
                if i == outer {
                    return first[i];
                }
                let c = centre(&boxes[i]);
                let su = if let Some(su) = band_u[i] {
                    su
                } else if pu {
                    geom::nearest_turn(c.0, anchor.0) - c.0
                } else {
                    0.0
                };
                let sv = if let Some(sv) = band_v[i] {
                    sv
                } else if pv {
                    geom::nearest_turn(c.1, anchor.1) - c.1
                } else {
                    0.0
                };
                (su, sv)
            })
            .collect();
        Some((outer, shifts))
    }

    /// ∮ (x dy − y dx)/2, ∮ x² dy/2 and −∮ y² dx/2 along one edge in the plane of *frame*, by
    /// Gauss–Legendre quadrature on short pieces of the exact curve.
    fn edge_moments(
        &self,
        edge: usize,
        forward: bool,
        frame: &crate::geom::Frame,
    ) -> (f64, f64, f64) {
        const NODES: [(f64, f64); 8] = [
            (-0.960_289_856_497_536_3, 0.101_228_536_290_376_26),
            (-0.796_666_477_413_626_7, 0.222_381_034_453_374_47),
            (-0.525_532_409_916_329, 0.313_706_645_877_887_3),
            (-0.183_434_642_495_649_8, 0.362_683_783_378_362),
            (0.183_434_642_495_649_8, 0.362_683_783_378_362),
            (0.525_532_409_916_329, 0.313_706_645_877_887_3),
            (0.796_666_477_413_626_7, 0.222_381_034_453_374_47),
            (0.960_289_856_497_536_3, 0.101_228_536_290_376_26),
        ];
        let ed = &self.edges[edge];
        let (mut t0, mut t1) = crate::sampling::edge_interval(
            &ed.curve,
            ed.start,
            ed.end,
            ed.same_sense,
            ed.is_closed(),
        );
        if !forward {
            std::mem::swap(&mut t0, &mut t1);
        }
        let pieces = match ed.curve {
            crate::geom::Curve::Line { .. } => 1,
            _ => 32,
        };
        let local = |t: f64| {
            let l = frame.to_local(ed.curve.value(t));
            (l[0], l[1])
        };
        let (mut a, mut mx, mut my) = (0.0, 0.0, 0.0);
        for k in 0..pieces {
            let (lo, hi) = (
                t0 + (t1 - t0) * k as f64 / pieces as f64,
                t0 + (t1 - t0) * (k + 1) as f64 / pieces as f64,
            );
            let (half, centre) = (0.5 * (hi - lo), 0.5 * (hi + lo));
            let h = 1e-6 * half.abs().max(1e-12);
            for (x, w) in NODES {
                let t = centre + half * x;
                let (px, py) = local(t);
                let (p1, p0) = (local(t + h), local(t - h));
                let (dx, dy) = ((p1.0 - p0.0) / (2.0 * h), (p1.1 - p0.1) / (2.0 * h));
                let weight = w * half;
                a += weight * 0.5 * (px * dy - py * dx);
                mx += weight * 0.5 * px * px * dy;
                my -= weight * 0.5 * py * py * dx;
            }
        }
        (a, mx, my)
    }

    /// build123d's `Face.center()`: the area centroid of a planar face, otherwise the surface
    /// point at the middle of the face's parameter range.
    pub fn face_centre(&self, face: usize) -> Option<V3> {
        let f = &self.faces[face];
        let Surface::Plane { frame } = f.surface else {
            let (u0, u1, v0, v1) = self.uv_bounds(face)?;
            return Some(f.surface.value(0.5 * (u0 + u1), 0.5 * (v0 + v1)));
        };
        // Green's theorem along each loop's exact edge curves, in plane coordinates; the largest
        // loop is the outer boundary and the rest are holes in it (loop orientation is not
        // relied on). Polyline shoelaces would carry the samples' chord error.
        let mut loops: Vec<(f64, f64, f64)> = f
            .loops
            .iter()
            .filter(|lp| !lp.edges.is_empty())
            .map(|lp| {
                let (mut a, mut mx, mut my) = (0.0, 0.0, 0.0);
                for &(e, forward) in &lp.edges {
                    let (da, dmx, dmy) = self.edge_moments(e, forward, &frame);
                    (a, mx, my) = (a + da, mx + dmx, my + dmy);
                }
                (a.abs(), mx / a, my / a)
            })
            .collect();
        loops.sort_by(|a, b| b.0.total_cmp(&a.0));
        let (outer, holes) = loops.split_first()?;
        let area = outer.0 - holes.iter().map(|h| h.0).sum::<f64>();
        let cx = (outer.0 * outer.1 - holes.iter().map(|h| h.0 * h.1).sum::<f64>()) / area;
        let cy = (outer.0 * outer.2 - holes.iter().map(|h| h.0 * h.2).sum::<f64>()) / area;
        Some(frame.to_world([cx, cy, 0.0]))
    }

    /// The face's trimmed region in parameter space.
    pub fn domain(&self, face: usize) -> Option<&FaceDomain> {
        self.cache[face]
            .domain
            .get_or_init(|| {
                let f = &self.faces[face];
                if self.whole_sphere(face) {
                    // The band between the two pole lines, as a sphere zone between circles is.
                    let pole = |v: f64| UvLoop {
                        points: periodic_line(0.0, TAU, v),
                        anchors: Vec::new(),
                        winds_u: false,
                        winds_v: false,
                        sources: Vec::new(),
                    };
                    return Some(FaceDomain::new(
                        &[pole(-PI / 2.0), pole(PI / 2.0)],
                        f.surface.periodic(),
                    ));
                }
                let loops = self.uv_loops(face)?;
                let spans = crate::mass::closed_spans(&f.surface);
                let Surface::Freeform { surface, .. } = &f.surface else {
                    let (pu, pv) = f.surface.periodic();
                    if pu && pv && loops.iter().any(|lp| lp.winds_u || lp.winds_v) {
                        // Containment and Green's theorem must read the same side of a
                        // seamless band, including one crossing the other parameter's seam.
                        let (_, shifts) = self.loop_placement(face)?;
                        let placed: Vec<UvLoop> = loops
                            .iter()
                            .zip(shifts)
                            .map(|(lp, shift)| {
                                let mut lp = lp.clone();
                                for p in &mut lp.points {
                                    p.0 += shift.0;
                                    p.1 += shift.1;
                                }
                                lp
                            })
                            .collect();
                        return Some(FaceDomain::new(&placed, (pu, pv)));
                    }
                    return Some(FaceDomain::new(loops, (pu, pv)));
                };
                // A closed B-spline surface (which the reader does not mark periodic) whose face
                // boundary crosses the surface's seam (a face's own seam need not lie on it)
                // jumps by the span there: such a direction is read as turning once round the
                // span, its loops unwrapped across the seam.
                let jumps = |pick: fn(&(f64, f64)) -> f64, span: f64| {
                    span.is_finite()
                        && loops.iter().any(|lp| {
                            lp.points
                                .windows(2)
                                .any(|w| (pick(&w[1]) - pick(&w[0])).abs() > 0.5 * span)
                        })
                };
                let closed = (jumps(|p| p.0, spans.0), jumps(|p| p.1, spans.1));
                if !closed.0 && !closed.1 {
                    return Some(FaceDomain::new(loops, f.surface.periodic()));
                }
                let (u0, _, v0, _) = surface.domain();
                let read = |start: f64, span: f64, closed: bool| {
                    if closed {
                        (start, TAU / span)
                    } else {
                        (0.0, 1.0)
                    }
                };
                let scale = [read(u0, spans.0, closed.0), read(v0, spans.1, closed.1)];
                let turned: Vec<UvLoop> = loops
                    .iter()
                    .map(|lp| {
                        let mut points: Vec<(f64, f64)> = Vec::with_capacity(lp.points.len());
                        for &(u, v) in &lp.points {
                            let (mut u, mut v) =
                                ((u - scale[0].0) * scale[0].1, (v - scale[1].0) * scale[1].1);
                            if let Some(&(lu, lv)) = points.last() {
                                if closed.0 {
                                    u = geom::nearest_turn(u, lu);
                                }
                                if closed.1 {
                                    v = geom::nearest_turn(v, lv);
                                }
                            }
                            points.push((u, v));
                        }
                        UvLoop {
                            points,
                            anchors: Vec::new(),
                            winds_u: false,
                            winds_v: false,
                            sources: Vec::new(),
                        }
                    })
                    .collect();
                Some(FaceDomain::scaled(&turned, closed, scale))
            })
            .as_ref()
    }

    /// A sphere bounded only by vertex loops at its poles is the whole sphere (OpenCascade adds
    /// the missing seam on import and ranges it over every u and v).
    pub(super) fn whole_sphere(&self, face: usize) -> bool {
        let f = &self.faces[face];
        matches!(f.surface, Surface::Sphere { .. })
            && !f.loops.is_empty()
            && f.loops.iter().all(|lp| lp.vertex.is_some())
    }

    /// Each loop as a closed polyline in (u, v), with periodic parameters unwrapped so each
    /// polyline is continuous. A loop around a periodic direction ends one period away from where
    /// it began.
    pub fn uv_loops(&self, face: usize) -> Option<&[UvLoop]> {
        self.cache[face]
            .uv_loops
            .get_or_init(|| self.compute_uv_loops(face))
            .as_deref()
    }

    fn compute_uv_loops(&self, face: usize) -> Option<Vec<UvLoop>> {
        let f = &self.faces[face];
        let (pu, pv) = f.surface.periodic();
        let mut loops = Vec::new();
        for lp in &f.loops {
            let mut pts: Vec<(f64, f64)> = Vec::new();
            if let Some(apex) = lp.vertex {
                // A degenerate loop: the whole periodic u line at the apex's v (one entry per
                // face loop either way, so loop indices agree with the face's).
                let (u, v) = f.surface.parameters(apex, None)?;
                let points = if pu {
                    periodic_line(0.0, TAU, v)
                } else {
                    vec![(u, v)]
                };
                loops.push(UvLoop {
                    points,
                    anchors: Vec::new(),
                    winds_u: false,
                    winds_v: false,
                    sources: Vec::new(),
                });
                continue;
            }
            let mut raw: Vec<(f64, f64)> = Vec::new();
            let mut points: Vec<V3> = Vec::new();
            let mut firsts = Vec::with_capacity(lp.edges.len());
            let mut origin: Vec<(usize, usize)> = Vec::new();
            // A point on a B-spline side collapsed to a point has its parameter along the side
            // wherever its own seed left it: followed on from, it would start the next sample's
            // inversion anywhere along that side, where the surface's derivative along it
            // vanishes and the search can stall on the side, far from the sample (cgb202 face
            // 1034's samples beside its apex stopped there 0.05 mm off). The next sample is
            // inverted from seeds beside the side instead (`beside_collapsed`).
            let collapsed = crate::mass::collapsed_sides(&f.surface);
            let on_collapsed = |q: (f64, f64)| {
                let on = |c: Option<f64>, x: f64| c.is_some_and(|c| (x - c).abs() <= 1e-9);
                on(collapsed.0, q.0) || on(collapsed.1, q.1)
            };
            let invert = |p: V3, hint: Option<(f64, f64)>| match hint {
                Some(q) if on_collapsed(q) => beside_collapsed(&f.surface, p, collapsed),
                _ => f.surface.parameters(p, hint),
            };
            for (place, &(e, forward)) in lp.edges.iter().enumerate() {
                firsts.push(raw.len());
                let samples = &self.edges[e].samples;
                let n = samples.len();
                origin.extend((0..n).map(|k| (place, if forward { k } else { n - 1 - k })));
                let ordered: Box<dyn Iterator<Item = &V3>> = if forward {
                    Box::new(samples.iter())
                } else {
                    Box::new(samples.iter().rev())
                };
                for p in ordered {
                    raw.push(invert(*p, raw.last().copied())?);
                    points.push(*p);
                }
            }
            // The loop's first samples are inverted with nothing to follow; on a closed B-spline
            // surface (not marked periodic) a seam point has two parameter values. Walk the start
            // again following on from the loop's end, until it agrees with the first pass.
            if !pu && !pv && raw.len() > 2 {
                let mut hint = raw[raw.len() - 1];
                for k in 0..raw.len() - 1 {
                    let again = invert(points[k], Some(hint))?;
                    let same = (again.0 - raw[k].0).abs() + (again.1 - raw[k].1).abs() < 1e-9;
                    raw[k] = again;
                    hint = again;
                    if same {
                        break;
                    }
                }
            }
            route_collapsed_sides(&f.surface, &points, &mut raw);
            let (raw, placed) = route_singular_points(&f.surface, raw, f.reversed);
            for (mut u, mut v) in raw {
                if let Some(&(lu, lv)) = pts.last() {
                    if pu {
                        u = geom::nearest_turn(u, lu);
                    }
                    if pv {
                        v = geom::nearest_turn(v, lv);
                    }
                }
                pts.push((u, v));
            }
            let (winds_u, winds_v) = match (pts.first(), pts.last()) {
                (Some(a), Some(b)) => {
                    (pu && (b.0 - a.0).abs() > 1.0, pv && (b.1 - a.1).abs() > 1.0)
                }
                _ => (false, false),
            };
            close_through_pole(&f.surface, &mut pts, f.reversed);
            let ends: Vec<usize> = firsts
                .iter()
                .skip(1)
                .copied()
                .chain([placed.len()])
                .collect();
            let anchors = firsts
                .iter()
                .zip(&ends)
                .map(|(&a, &b)| {
                    (a..b)
                        .filter(|&k| k == a || placed[k - 1].is_none())
                        .filter_map(|k| placed[k])
                        .map(|p| pts[p])
                        .collect()
                })
                .collect();
            let mut sources = vec![None; pts.len()];
            for (k, at) in placed.iter().enumerate() {
                if let Some(i) = *at {
                    sources[i] = Some(origin[k]);
                }
            }
            loops.push(UvLoop {
                points: pts,
                anchors,
                winds_u,
                winds_v,
                sources,
            });
        }
        Some(loops)
    }

    /// `BRepTools::UVBounds` — (u_first, u_last, v_first, v_last) of the face's trimmed range.
    ///
    /// A periodic direction the face closes around (a seam edge, or a loop that winds once
    /// round) spans exactly one period starting at the seam.
    pub fn uv_bounds(&self, face: usize) -> Option<(f64, f64, f64, f64)> {
        *self.cache[face]
            .uv_bounds
            .get_or_init(|| self.compute_uv_bounds(face))
    }

    fn compute_uv_bounds(&self, face: usize) -> Option<(f64, f64, f64, f64)> {
        let f = &self.faces[face];
        if self.whole_sphere(face) {
            return Some((0.0, TAU, -PI / 2.0, PI / 2.0));
        }
        if let Surface::Plane { frame } = f.surface {
            return self.plane_uv_bounds(face, frame);
        }
        let periodic = f.surface.periodic();
        let (pu, pv) = periodic;
        let loops = self.uv_loops(face)?;
        let mut range = NO_RANGE;
        // Loops are placed in one period by `loop_placement`; whether a loop winds round a
        // periodic direction is read from its ends.
        let mut winds = (false, false);
        for lp in loops {
            let (Some(&first), Some(&last)) = (lp.points.first(), lp.points.last()) else {
                continue;
            };
            winds.0 |= pu && (last.0 - first.0).abs() > 1.0;
            winds.1 |= pv && (last.1 - first.1).abs() > 1.0;
        }
        let (_, shifts) = self.loop_placement(face)?;
        // Edge by edge, from the edges themselves: where the file gives an edge's pcurve, it
        // widens the range too (aligned to the edge's own samples to pick the period).
        for ((lp, uv), shift) in f.loops.iter().zip(loops).zip(&shifts) {
            if let Some(vertex) = lp.vertex {
                // A vertex loop at a pole or apex reaches that parameter line; its u is
                // meaningless there.
                let (u, v) = f.surface.parameters(vertex, None)?;
                match f.surface.singular_v(v) {
                    Some(line) => widen_v(&mut range, line),
                    None => widen(&mut range, (u + shift.0, v + shift.1)),
                }
                continue;
            }
            self.widen_by_loop_edges(f, lp, uv, *shift, periodic, &mut range)?;
            // A loop running once round a sphere is closed through a pole, which the face then
            // reaches (`close_through_pole`).
            if matches!(f.surface, Surface::Sphere { .. })
                && uv.winds_u
                && let Some(&(_, pole)) = uv.points.len().checked_sub(2).map(|k| &uv.points[k])
            {
                widen_v(&mut range, pole);
            }
        }
        let seam = self.seam_parameters(face);
        for (periodic, wound, lo, hi, seam_value) in
            [(pu, winds.0, 0, 1, seam.0), (pv, winds.1, 2, 3, seam.1)]
        {
            if !periodic {
                continue;
            }
            if wound || seam_value.is_some() || range[hi] - range[lo] > TAU - 1e-9 {
                let start = seam_value.unwrap_or(0.0);
                range[lo] = start;
                range[hi] = start + TAU;
            }
        }
        Some((range[0], range[1], range[2], range[3]))
    }

    /// A plane's range is exactly its edges' extent in plane coordinates.
    fn plane_uv_bounds(&self, face: usize, frame: Frame) -> Option<(f64, f64, f64, f64)> {
        let mut range = NO_RANGE;
        for e in self.face_edges(face) {
            let ed = &self.edges[e];
            let interval =
                edge_interval(&ed.curve, ed.start, ed.end, ed.same_sense, ed.is_closed());
            let ends = [ed.curve.value(interval.0), ed.curve.value(interval.1)];
            for p in
                ends.into_iter()
                    .chain(extremes_along(&ed.curve, interval, &[frame.x, frame.y]))
            {
                let l = frame.to_local(p);
                widen(&mut range, (l[0], l[1]));
            }
        }
        range[0]
            .is_finite()
            .then_some((range[0], range[1], range[2], range[3]))
    }

    /// Widens *range* by one loop of face *f*, edge by edge, from the edges' samples or, where
    /// the file gives them, their pcurves. *uv* is the loop unwrapped and *shift* its placement.
    fn widen_by_loop_edges(
        &self,
        f: &Face,
        lp: &Loop,
        uv: &UvLoop,
        shift: (f64, f64),
        periodic: (bool, bool),
        range: &mut [f64; 4],
    ) -> Option<()> {
        // Start this loop where its own unwrapped samples sit, in its placed period.
        let Some(&start) = uv.points.first() else {
            return Some(());
        };
        let mut last = (start.0 + shift.0, start.1 + shift.1);
        let (lu, lv) = (
            self::range(uv.points.iter(), |p| p.0),
            self::range(uv.points.iter(), |p| p.1),
        );
        let loop_centre = (0.5 * (lu.0 + lu.1) + shift.0, 0.5 * (lv.0 + lv.1) + shift.1);
        let collapsed = crate::mass::collapsed_sides(&f.surface);
        for &(e, forward) in &lp.edges {
            // A sphere's pcurves running into a pole turn in u where its 3D edges keep their
            // longitude: they do not size its range.
            let own: Vec<&Pcurve> = f
                .pcurves
                .iter()
                .filter(|(pe, _)| *pe == e && !matches!(f.surface, Surface::Sphere { .. }))
                .map(|(_, c)| c)
                .collect();
            let (mut pts, partial) =
                self.edge_uv_points(f, e, forward, periodic, collapsed, &mut last)?;
            for (u, v) in partial {
                if let Some(u) = u {
                    range[0] = range[0].min(u);
                    range[1] = range[1].max(u);
                }
                if let Some(v) = v {
                    widen_v(range, v);
                }
            }
            // A pcurve constant in v fixes the edge's v exactly; otherwise its samples would
            // undercut its interior extremes.
            if !own.iter().any(|c| fixed_parameters(c).1.is_some()) {
                for v in self.axial_extremes(f, e)? {
                    widen_v(range, v);
                }
            }
            if pts.is_empty() {
                continue;
            }
            // Put the edge in the period its loop occupies: across a skipped pole or apex
            // sample, continuity alone cannot tell which turn the edge is on.
            let n = pts.len() as f64;
            let mean = (
                pts.iter().map(|p| p.0).sum::<f64>() / n,
                pts.iter().map(|p| p.1).sum::<f64>() / n,
            );
            let placed = near(periodic, mean, loop_centre);
            let (du, dv) = (placed.0 - mean.0, placed.1 - mean.1);
            pts.iter_mut().for_each(|p| *p = (p.0 + du, p.1 + dv));
            // Not empty (checked above).
            if let Some(&p) = pts.last() {
                last = p;
            }
            if own.is_empty() {
                pts.iter().for_each(|&p| widen(range, p));
                continue;
            }
            widen_by_pcurves(&f.surface, &own, &pts, placed, periodic, range);
        }
        Some(())
    }

    /// Edge *e*'s points on face *f*, walked *forward* or back, in (u, v) continuing from
    /// *last* (which follows them): its samples, its ends taken on its curve (a vertex may
    /// stand off the curve, and the face, by its tolerance); and, apart, the one parameter a
    /// sample fixes where the other is arbitrary: v at a pole or apex, or on a B-spline side
    /// *collapsed* to a point (`collapsed_sides`) the parameter across that side.
    fn edge_uv_points(
        &self,
        f: &Face,
        e: usize,
        forward: bool,
        periodic: (bool, bool),
        collapsed: (Option<f64>, Option<f64>),
        last: &mut (f64, f64),
    ) -> Option<EdgeUv> {
        let ed = &self.edges[e];
        let samples = &ed.samples;
        let (t0, t1) = edge_interval(&ed.curve, ed.start, ed.end, ed.same_sense, ed.is_closed());
        let n = samples.len();
        let at = |k: usize| match k {
            0 => ed.curve.value(t0),
            k if k + 1 == n => ed.curve.value(t1),
            k => samples[k],
        };
        let ordered: Box<dyn Iterator<Item = usize>> = if forward {
            Box::new(0..n)
        } else {
            Box::new((0..n).rev())
        };
        // A closed B-spline surface (not marked periodic) gives a seam point two parameter
        // values: follow on from the last point.
        let follow = matches!(f.surface, Surface::Freeform { .. });
        let mut pts = Vec::with_capacity(n);
        let mut partial = Vec::new();
        for k in ordered {
            let p = at(k);
            let q = f.surface.parameters(p, follow.then_some(*last))?;
            if let Some(line) = f.surface.singular_v(q.1) {
                // A sample at the pole or apex itself (to 1e-9) is on its line, whatever
                // round-off the vertex's coordinates leave in its v.
                let v = if (q.1 - line).abs() <= 1e-9 {
                    line
                } else {
                    q.1
                };
                partial.push((None, Some(v)));
                continue;
            }
            let on = |u: f64, v: f64| geom::dist(p, f.surface.value(u, v)) <= 1e-6;
            if let Some(u) = collapsed.0.filter(|&u| on(u, q.1)) {
                partial.push((Some(u), None));
                continue;
            }
            if let Some(v) = collapsed.1.filter(|&v| on(q.0, v)) {
                partial.push((None, Some(v)));
                continue;
            }
            let q = near(periodic, q, *last);
            pts.push(q);
            *last = q;
        }
        Some((pts, partial))
    }

    /// The v of edge *e*'s interior extremes along the axis of face *f*'s cylinder, cone or
    /// sphere, whose v grows with the axial coordinate (none on other surfaces).
    fn axial_extremes(&self, f: &Face, e: usize) -> Option<Vec<f64>> {
        let (Surface::Cylinder { frame, .. }
        | Surface::Cone { frame, .. }
        | Surface::Sphere { frame, .. }) = f.surface
        else {
            return Some(Vec::new());
        };
        let ed = &self.edges[e];
        let interval = edge_interval(&ed.curve, ed.start, ed.end, ed.same_sense, ed.is_closed());
        extremes_along(&ed.curve, interval, &[frame.z])
            .into_iter()
            .map(|p| f.surface.parameters(p, None).map(|q| q.1))
            .collect()
    }

    /// The constant periodic parameter of a seam edge (an edge used twice by this face), if
    /// any, normalised to `[0, 2π)` — the side OpenCascade starts its parameter range from.
    fn seam_parameters(&self, face: usize) -> (Option<f64>, Option<f64>) {
        let f = &self.faces[face];
        let mut seen = std::collections::BTreeMap::new();
        for lp in &f.loops {
            for &(e, _) in &lp.edges {
                *seen.entry(e).or_insert(0) += 1;
            }
        }
        let mut out = (None, None);
        for (&e, &count) in &seen {
            if count < 2 {
                continue;
            }
            let samples = &self.edges[e].samples;
            let Some((u0, v0)) = f.surface.parameters(samples[0], None) else {
                continue;
            };
            let mid = samples[samples.len() / 2];
            let Some((u1, v1)) = f.surface.parameters(mid, None) else {
                continue;
            };
            if (geom::nearest_turn(u1, u0) - u0).abs() < 1e-7 {
                out.0 = Some(round_seam(u0));
            } else if (geom::nearest_turn(v1, v0) - v0).abs() < 1e-7 {
                out.1 = Some(round_seam(v0));
            }
        }
        out
    }
}

/// An empty (u_min, u_max, v_min, v_max) range.
const NO_RANGE: [f64; 4] = [
    f64::INFINITY,
    f64::NEG_INFINITY,
    f64::INFINITY,
    f64::NEG_INFINITY,
];

/// Widens a (u_min, u_max, v_min, v_max) range to take in (u, v).
fn widen(range: &mut [f64; 4], (u, v): (f64, f64)) {
    *range = [
        range[0].min(u),
        range[1].max(u),
        range[2].min(v),
        range[3].max(v),
    ];
}

/// One parameter of a boundary point whose other parameter is arbitrary: (u, v), one `None`.
type Partial = (Option<f64>, Option<f64>);

/// An edge's points in (u, v), and the points that fix one parameter alone.
type EdgeUv = (Vec<(f64, f64)>, Vec<Partial>);

/// Widens a (u_min, u_max, v_min, v_max) range's v to take in *v*.
fn widen_v(range: &mut [f64; 4], v: f64) {
    range[2] = range[2].min(v);
    range[3] = range[3].max(v);
}

/// (u, v) moved by whole turns in its periodic parameters to lie nearest (ru, rv).
fn near(periodic: (bool, bool), (u, v): (f64, f64), (ru, rv): (f64, f64)) -> (f64, f64) {
    (
        if periodic.0 {
            geom::nearest_turn(u, ru)
        } else {
            u
        },
        if periodic.1 {
            geom::nearest_turn(v, rv)
        } else {
            v
        },
    )
}

/// Widens *range* by an edge's own pcurves, each aligned to the period of *centre*, where the
/// edge's samples *pts* sit. A pole at a pole or apex of *surface* bounds v alone: its u is
/// arbitrary there.
fn widen_by_pcurves(
    surface: &Surface,
    own: &[&Pcurve],
    pts: &[(f64, f64)],
    centre: (f64, f64),
    periodic: (bool, bool),
    range: &mut [f64; 4],
) {
    for curve in own {
        if let Pcurve::Poles(poles) = curve {
            let m = poles.len() as f64;
            let mid = (
                poles.iter().map(|p| p.0).sum::<f64>() / m,
                poles.iter().map(|p| p.1).sum::<f64>() / m,
            );
            let aligned = near(periodic, mid, centre);
            for &(u, v) in poles.iter() {
                let (u, v) = (u + aligned.0 - mid.0, v + aligned.1 - mid.1);
                match surface.singular_v(v) {
                    Some(_) => widen_v(range, v),
                    None => widen(range, (u, v)),
                }
            }
        }
        // A pcurve constant in one parameter fixes that parameter exactly; the other comes from
        // the edge's samples. Otherwise the samples widen the range too: the poles hold their
        // curve, but a file's pcurve may stray from its edge (within tolerance), and the range
        // must still hold the edge.
        let (fixed_u, fixed_v) = fixed_parameters(curve);
        let place = |x: Option<f64>, c: f64, p: bool| {
            x.map(|x| if p { geom::nearest_turn(x, c) } else { x })
        };
        let (fixed_u, fixed_v) = (
            place(fixed_u, centre.0, periodic.0),
            place(fixed_v, centre.1, periodic.1),
        );
        for &(u, v) in pts {
            widen(range, (fixed_u.unwrap_or(u), fixed_v.unwrap_or(v)));
        }
    }
}

/// The parameters a pcurve holds constant, (u, v): a line along v or u, or a control polygon
/// whose poles share their u or v.
fn fixed_parameters(curve: &Pcurve) -> (Option<f64>, Option<f64>) {
    match curve {
        Pcurve::Line { point, dir } => (
            (dir.0.abs() <= 1e-12 * dir.1.abs()).then_some(point.0),
            (dir.1.abs() <= 1e-12 * dir.0.abs()).then_some(point.1),
        ),
        Pcurve::Poles(poles) => {
            let shared = |pick: fn(&(f64, f64)) -> f64| {
                let first = pick(poles.first()?);
                poles
                    .iter()
                    .all(|p| (pick(p) - first).abs() <= 1e-12 * (1.0 + first.abs()))
                    .then_some(first)
            };
            (shared(|p| p.0), shared(|p| p.1))
        }
    }
}

/// Replace each run of singular samples (where u is undefined) by the singular v line from the
/// u the boundary arrives at to the u it leaves at.
///
/// Which way round is a question only orientation answers — a hemisphere bounded by one
/// meridian circle arrives and leaves half a turn apart — so the face's side decides it: STEP
/// keeps the face on a loop's left, which in parameter space is the -u side of a boundary
/// rising to the top line (+u for a reversed face), mirrored at the bottom.
///
/// Also returns where each regular input sample lands in the output (`None` for a singular one).
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "past the early return the loop has both a regular and a singular sample"
)]
fn route_singular_points(
    surface: &Surface,
    raw: Vec<(f64, f64)>,
    reversed: bool,
) -> (Vec<(f64, f64)>, Vec<Option<usize>>) {
    let n = raw.len();
    let singular: Vec<bool> = raw
        .iter()
        .map(|&(_, v)| surface.singular_v(v).is_some())
        .collect();
    if !singular.iter().any(|s| *s) || singular.iter().all(|s| *s) {
        return (raw, (0..n).map(Some).collect());
    }
    let mut placed = vec![None; n];
    // Start on a regular sample so that no singular run straddles the loop's ends.
    let first_regular = singular.iter().position(|s| !s).expect("a regular sample");
    let raw: Vec<(f64, f64)> = raw[first_regular..]
        .iter()
        .chain(&raw[..first_regular])
        .copied()
        .collect();
    let singular: Vec<bool> = singular[first_regular..]
        .iter()
        .chain(&singular[..first_regular])
        .copied()
        .collect();
    // With a single singular run, route the way that leaves the closed loop turning least round
    // u (a face that does run round the axis is described as well by a loop that does not wind
    // but spans the turn); only a tie needs the orientation.
    let runs = (0..n)
        .filter(|&k| singular[k] && !singular[(k + n - 1) % n])
        .count();
    let regular_turn: f64 = (0..n)
        .filter(|&k| !singular[k] && !singular[(k + 1) % n])
        .map(|k| {
            let (a, b) = (raw[k].0, raw[(k + 1) % n].0);
            geom::nearest_turn(b, a) - a
        })
        .sum();
    let mut out = Vec::with_capacity(n + 64);
    let mut i = 0;
    while i < n {
        if !singular[i] {
            placed[(i + first_regular) % n] = Some(out.len());
            out.push(raw[i]);
            i += 1;
            continue;
        }
        let start = i;
        while i < n && singular[i] {
            i += 1;
        }
        // Neighbours on either side, wrapping round the closed loop.
        let before = (0..n)
            .map(|k| (start + n - 1 - k) % n)
            .find(|&k| !singular[k])
            .expect("a regular sample");
        let after = (0..n)
            .map(|k| (i + k) % n)
            .find(|&k| !singular[k])
            .expect("a regular sample");
        let v = surface.singular_v(raw[start].1).expect("a singular sample");
        let (ua, ub) = (raw[before].0, raw[after].0);
        let mut ub = geom::nearest_turn(ub, ua);
        let gap = |ub: f64| (regular_turn + ub - ua).abs();
        let unambiguous = runs == 1
            && (gap(ub + TAU) - gap(ub)).abs() > 1e-6
            && (gap(ub - TAU) - gap(ub)).abs() > 1e-6;
        if unambiguous {
            ub = [ub - TAU, ub, ub + TAU]
                .into_iter()
                .min_by(|a, b| gap(*a).total_cmp(&gap(*b)))
                .unwrap();
        } else if (ub - ua).abs() > 1e-6 {
            // Rising to the top line (v above the boundary's) runs -u for a forward face.
            let rising = v > raw[before].1;
            let toward = if rising != reversed { -1.0 } else { 1.0 };
            if (ub - ua) * toward < 0.0 {
                ub += toward * TAU;
            }
        }
        out.extend(periodic_line(ua, ub, v));
    }
    // Close explicitly: the loop's first sample is regular, so it is where the walk returns.
    if out.last() != out.first() {
        out.push(out[0]);
    }
    (out, placed)
}

/// Route a closed loop along a B-spline side that collapses to a point (`collapsed_sides`).
/// A sample at that point fixes only the parameter across the side; the other is arbitrary, and
/// inversion leaves it wherever its seed was, so the loop would cut straight across the face to
/// the next sample (cgb242 face 715 lost half its domain so). Each run of such samples (the
/// shared vertex of two edges, at least) instead runs along the side from where the boundary
/// arrives to where it leaves: its first sample takes the arriving sample's other parameter, the
/// rest the leaving one's. *points* are the samples, *raw* their parameters, in loop order.
fn route_collapsed_sides(surface: &Surface, points: &[V3], raw: &mut [(f64, f64)]) {
    let collapsed = crate::mass::collapsed_sides(surface);
    let n = raw.len();
    for (side, c) in [(0, collapsed.0), (1, collapsed.1)] {
        let Some(c) = c else { continue };
        let pick = |q: (f64, f64)| if side == 0 { q.0 } else { q.1 };
        let on: Vec<bool> = (0..n)
            .map(|k| {
                let q = raw[k];
                let at = if side == 0 { (c, q.1) } else { (q.0, c) };
                (pick(q) - c).abs() <= 1e-9
                    && geom::dist(points[k], surface.value(at.0, at.1)) <= 1e-6
            })
            .collect();
        let Some(start) = on.iter().position(|o| !o) else {
            continue; // the whole loop on the side: nothing to route from
        };
        let mut k = 0;
        while k < n {
            let i = (start + k) % n;
            if !on[i] {
                k += 1;
                continue;
            }
            let run: Vec<usize> = (k..n)
                .map(|j| (start + j) % n)
                .take_while(|&j| on[j])
                .collect();
            k += run.len();
            let before = raw[(run[0] + n - 1) % n];
            let after = raw[(run[run.len() - 1] + 1) % n];
            for (m, &j) in run.iter().enumerate() {
                let other = if m == 0 { before } else { after };
                raw[j] = if side == 0 {
                    (c, other.1)
                } else {
                    (other.0, c)
                };
            }
        }
    }
}

/// *p*'s parameters on a B-spline surface with a side *collapsed* to a point (`collapsed_sides`),
/// found with no point to follow on from: the best of an unhinted inversion and inversions seeded
/// a hundredth of the range inside each collapsed side, at seventeen places along it. Seeded on
/// the side itself (or at a grid node there, as the unhinted inversion may be) the search cannot
/// leave it, the surface's derivative along the side being zero.
pub(crate) fn beside_collapsed(
    surface: &Surface,
    p: V3,
    collapsed: (Option<f64>, Option<f64>),
) -> Option<(f64, f64)> {
    let mut best = surface.parameters(p, None)?;
    let Surface::Freeform { surface: nurbs, .. } = surface else {
        return Some(best);
    };
    let (u0, u1, v0, v1) = nurbs.domain();
    let off = |q: (f64, f64)| geom::dist(surface.value(q.0, q.1), p);
    let mut residual = off(best);
    let inside = |c: f64, lo: f64, hi: f64| c + 0.01 * (hi - lo) * if c <= lo { 1.0 } else { -1.0 };
    for k in 0..=16 {
        let t = k as f64 / 16.0;
        let seeds = [
            collapsed.0.map(|c| (inside(c, u0, u1), v0 + t * (v1 - v0))),
            collapsed.1.map(|c| (u0 + t * (u1 - u0), inside(c, v0, v1))),
        ];
        for seed in seeds.into_iter().flatten() {
            let q = surface.parameters(p, Some(seed))?;
            let d = off(q);
            if d < residual {
                (best, residual) = (q, d);
            }
        }
    }
    Some(best)
}

/// Whether the face reaches the singular point (pole or apex) at *v*: the point is a whole
/// parameter line, which lies on the face's boundary rather than strictly inside it, so look
/// just off the line instead.
pub(super) fn touches_singular_point(surface: &Surface, domain: &FaceDomain, v: f64) -> bool {
    let Some(line) = surface.singular_v(v) else {
        return false;
    };
    let off = line - line.signum() * 1e-4;
    (0..64).any(|k| domain.contains(TAU * k as f64 / 64.0, off))
}

/// A loop that runs once round a sphere ends one turn from where it began; close it along a
/// pole's parameter line so that it bounds a region. The pole is the one it reaches along a
/// seam if any; otherwise the one on the face's side of it (the left of a loop running +u on a
/// forward face is +v, the north pole).
fn close_through_pole(surface: &Surface, pts: &mut Vec<(f64, f64)>, reversed: bool) {
    if !matches!(surface, Surface::Sphere { .. }) || pts.len() < 2 {
        return;
    }
    let (first, last) = (pts[0], pts[pts.len() - 1]);
    if (last.0 - first.0).abs() < std::f64::consts::PI {
        return;
    }
    let half = std::f64::consts::FRAC_PI_2;
    let touched = pts
        .iter()
        .map(|p| p.1)
        .find(|v| (v.abs() - half).abs() < 1e-6);
    let pole = match touched {
        Some(v) => v.signum() * half,
        None if (last.0 > first.0) != reversed => half,
        None => -half,
    };
    pts.extend(periodic_line(last.0, first.0, pole));
    pts.push(first);
}

/// Points along a constant-v line from u `a` to `b`, short enough that no segment spans half a
/// period — containment wraps each segment to the nearest turn, which a long one would defeat.
fn periodic_line(a: f64, b: f64, v: f64) -> Vec<(f64, f64)> {
    let n = 64;
    (0..=n)
        .map(|i| (a + (b - a) * i as f64 / n as f64, v))
        .collect()
}

/// A seam at 2π is the same seam as one at 0; OpenCascade starts the range at 0.
fn round_seam(value: f64) -> f64 {
    if (value - TAU).abs() < 1e-9 || value.abs() < 1e-9 {
        0.0
    } else {
        value
    }
}

/// The whole-turn shift that puts a loop's box centre `c` in `[0, 2π)` (none if not periodic).
/// A loop centred on the seam (a range symmetric about 0) has a centre a few ulps either side of
/// it, which libm round-off decides; within 1e-9 of a turn it counts as on the seam and is kept
/// at 0, not moved to 2π.
fn into_period(c: f64, periodic: bool) -> f64 {
    if periodic {
        let c = c + 1e-9;
        c.rem_euclid(TAU) - c
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn empty_face_has_no_loop_placement() {
        let mut part = crate::read_step_file(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/golden_simple_through_hole.step"
        )))
        .unwrap();
        // Programmatic Parts can still be incomplete; queries must fail closed.
        part.faces[0].loops.clear();
        let part = Part::new(part.faces, part.edges, part.solids);
        assert!(part.loop_placement(0).is_none());
        assert!(part.outer_loop(0).is_none());
    }

    use super::*;

    fn domain(loops: Vec<Vec<(f64, f64)>>, periodic: (bool, bool)) -> FaceDomain {
        let loops: Vec<UvLoop> = loops
            .into_iter()
            .map(|points| UvLoop {
                points,
                anchors: Vec::new(),
                winds_u: false,
                winds_v: false,
                sources: Vec::new(),
            })
            .collect();
        FaceDomain::new(&loops, periodic)
    }

    fn square(lo: f64, hi: f64) -> Vec<(f64, f64)> {
        vec![(lo, lo), (hi, lo), (hi, hi), (lo, hi), (lo, lo)]
    }

    #[test]
    fn annulus_excludes_its_hole() {
        let d = domain(vec![square(0.0, 10.0), square(4.0, 6.0)], (false, false));
        assert!(d.contains(2.0, 2.0));
        assert!(!d.contains(5.0, 5.0));
        assert!(!d.contains(11.0, 5.0));
    }

    #[test]
    fn cylinder_band_between_two_circles() {
        // Two separate loops round a periodic u, as files without seam edges give them.
        let ring = |v: f64| periodic_line(0.0, TAU, v);
        let d = domain(vec![ring(0.0), ring(5.0)], (true, false));
        assert!(d.contains(1.0, 2.0) && d.contains(6.0, 2.0) && d.contains(-1.0, 2.0));
        assert!(!d.contains(1.0, -1.0) && !d.contains(1.0, 6.0));
    }

    #[test]
    fn seam_centred_loop_placed_regardless_of_round_off() {
        // cgb202 torus face 1840: v spans ±1.186 about the seam, its centre ~1e-16 either side
        // of 0 by platform; both signs (and a centre just below 2π) place the range at -1.186.
        let half = 1.186;
        for c in [1e-16, -1e-16, 0.0, TAU - 1e-16, -TAU + 1e-16] {
            let lo = c - half + into_period(c, true);
            assert!(
                (lo + half).abs() < 1e-12,
                "centre {c:e}: range starts at {lo}"
            );
        }
        assert_eq!(into_period(-1e-16, false), 0.0);
        assert!((into_period(3.0, true)).abs() < 1e-12);
        assert!((into_period(-3.0, true) - TAU).abs() < 1e-12);
    }

    #[test]
    fn torus_band_partial_in_v() {
        // A seam rectangle [0, 2π] × [0, π/2], as OpenCascade writes a turned fillet.
        let h = std::f64::consts::FRAC_PI_2;
        let mut lp = periodic_line(0.0, TAU, 0.0);
        lp.extend([(TAU, h)]);
        lp.extend(periodic_line(TAU, 0.0, h));
        lp.push((0.0, 0.0));
        let d = domain(vec![lp], (true, true));
        assert!(d.contains(3.0, 0.7));
        assert!(!d.contains(3.0, 2.0) && !d.contains(3.0, -0.5));
    }

    #[test]
    fn hemisphere_closed_through_its_pole() {
        let frame = crate::geom::Frame {
            origin: [0.0; 3],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            z: [0.0, 0.0, 1.0],
        };
        let sphere = Surface::Sphere { frame, radius: 1.0 };
        let h = std::f64::consts::FRAC_PI_2;
        // Seam up to the pole and back, then the equator once round.
        let mut pts = vec![(0.0, 0.0), (0.0, h), (0.0, 0.0)];
        pts.extend(periodic_line(0.0, TAU, 0.0));
        close_through_pole(&sphere, &mut pts, false);
        let d = domain(vec![pts], (true, false));
        assert!(d.contains(2.0, 0.5));
        assert!(!d.contains(2.0, -0.5));
    }

    #[test]
    fn binned_crossings_agree_with_every_segment() {
        // A wavy band round u with a hole, on a torus closed in v, and a plane annulus.
        let band: Vec<(f64, f64)> = (0..=200)
            .map(|k| {
                let u = TAU * k as f64 / 200.0;
                (u, 1.0 + 0.3 * (5.0 * u).sin())
            })
            .chain((0..=200).map(|k| {
                let u = TAU * (200 - k) as f64 / 200.0;
                (u, 3.0 + 0.2 * (3.0 * u).cos())
            }))
            .chain([(0.0, 1.0)])
            .collect();
        let hole: Vec<(f64, f64)> = (0..=40)
            .map(|k| {
                let t = TAU * k as f64 / 40.0;
                (2.0 + 0.4 * t.cos(), 2.0 + 0.4 * t.sin())
            })
            .collect();
        let tube: Vec<(f64, f64)> = band.iter().map(|&(u, v)| (v * 0.5, u)).collect();
        for (loops, periodic) in [
            (vec![band.clone(), hole.clone()], (true, false)),
            (vec![band, hole], (false, false)),
            (vec![tube], (true, true)),
        ] {
            let d = domain(loops.clone(), periodic);
            let wrap = d.index.periodic;
            // As the domain chooses its ray: in +u on a face closed in v.
            let swap = periodic.1 && d.v_range.1 - d.v_range.0 >= TAU - 1e-9;
            let pick = |p: &(f64, f64)| if swap { (p.1, p.0) } else { (p.0, p.1) };
            // Every segment, as the index's candidates are tested.
            let brute = |a: f64, b: f64, b_end: f64| {
                let mut count = 0;
                for lp in &loops {
                    for w in lp.windows(2) {
                        let ((mut a0, b0), (mut a1, b1)) = (pick(&w[0]), pick(&w[1]));
                        if wrap {
                            let shift = geom::nearest_turn(a0, a) - a0;
                            (a0, a1) = (a0 + shift, a1 + shift);
                        }
                        if (a0 > a) != (a1 > a) {
                            let bc = b0 + (a - a0) / (a1 - a0) * (b1 - b0);
                            count += usize::from(bc > b && bc < b_end);
                        }
                    }
                }
                count % 2 == 1
            };
            for i in 0..97 {
                for j in 0..61 {
                    let (a, b) = (-1.0 + 8.5 * i as f64 / 97.0, -0.5 + 4.5 * j as f64 / 61.0);
                    assert_eq!(d.crossing_parity(a, b, 1e9), brute(a, b, 1e9), "{a} {b}");
                }
            }
        }
    }
}
