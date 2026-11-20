//! Tapers and V-grooves of a turned body as records of their own (#307 item 6).
//!
//! Both are read from the meridian profile that `turned_bodies` already publishes, so the
//! profile itself is unchanged. They are appended after every other family and never
//! override one: a cone that another record defines (a chamfer, a countersink, a hole's
//! drill point, a thread) is left to that record. Faces stay co-owned with the turned body.
use std::collections::BTreeSet;

use serde::Serialize;

use super::turned_bodies::{self, ProfileSegment, TurnedBody};
use super::{Context, Features, Kept, kept};
use crate::features::evidence::{self, EvidenceStatus, Occurrence};
use crate::kernel::brep::Part;
use crate::kernel::geom::{self, Curve, Surface, V3};

/// A coaxial conical band of a turned body between two coaxial cylinders, or a cylinder
/// and a plane, or two planes: `large_diameter` and `small_diameter` at its two circles,
/// `length` along the axis and `included_angle` (the full cone angle, degrees). `axis`
/// points from the large end to the small end; `axis_point` is the large circle's centre.
/// `turned_body` is the index of the `turned_bodies` record whose profile holds the cone.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Taper {
    pub axis: V3,
    pub axis_point: V3,
    pub side: &'static str,
    pub large_diameter: f64,
    pub small_diameter: f64,
    pub length: f64,
    pub included_angle: f64,
    pub turned_body: usize,
    pub faces: Vec<usize>,
}

/// An annular groove of a turned body whose section is two coaxial cones meeting at a sharp
/// bottom circle, or at a tiny flat or round. `axis` is the turning axis (positive
/// dominant component), `axis_point` the centre of the mouth, `mouth_diameter` the diameter
/// of the two equal rims, `bottom_diameter` the diameter at the deepest circle, `width` the
/// axial distance between the rims, `depth` the radial distance from rim to bottom and
/// `included_angle` the angle between the two flanks (degrees). `turned_body` indexes the
/// `turned_bodies` record.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct VGroove {
    pub axis: V3,
    pub axis_point: V3,
    pub side: &'static str,
    pub width: f64,
    pub depth: f64,
    pub included_angle: f64,
    pub mouth_diameter: f64,
    pub bottom_diameter: f64,
    pub turned_body: usize,
    pub faces: Vec<usize>,
}

/// Rim radii of a V groove agree, and a flat is a tiny one, within this fraction of the
/// groove's width; absolute equality is within the published precision (a 0.001 mm cell).
const BOTTOM_FRAC: f64 = 0.1;
const EQUAL_RIMS: f64 = 0.002;
/// A cone is an edge break, not a taper, when narrow (its larger leg under this fraction of its larger diameter) or when
/// its legs are equal (a 45 degree break, as `chamfers` reads it).
const EDGE_BREAK_FRAC: f64 = 0.1;
/// A bore cone that meets a plane (a mouth or floor) is a seat or a drill point, unless it
/// is as slender as a taper socket.
const MAX_BORE_TAPER_DEG: f64 = 30.0;
/// A taper's included angle is at most this; wider cones are transitions, chamfers or seats.
const MAX_TAPER_DEG: f64 = 60.0;
/// A V groove opens between these angles; narrower is a slit, wider a shallow dish.
const V_ANGLE_RANGE: (f64, f64) = (20.0, 140.0);

fn sign(side: &str) -> Option<f64> {
    match side {
        "external" => Some(1.),
        "internal" => Some(-1.),
        _ => None,
    }
}

/// The faces other records define. The body classifications co-own every face and are not
/// competing claims; neither are the two families here.
fn claimed(features: &Features) -> BTreeSet<usize> {
    features
        .defining
        .iter()
        .filter(|(family, _)| {
            !matches!(
                **family,
                "thin_wall_bodies"
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
        .collect()
}

/// A cone face that runs a whole turn: exactly two closed circular rims, and every other
/// edge a seam used twice.
fn complete_cone(part: &Part, face: usize) -> bool {
    if !matches!(part.faces[face].surface, Surface::Cone { .. }) {
        return false;
    }
    let mut uses = std::collections::BTreeMap::<usize, usize>::new();
    for lp in &part.faces[face].loops {
        for &(e, _) in &lp.edges {
            *uses.entry(e).or_default() += 1;
        }
    }
    let mut circles = 0;
    for (&e, &n) in &uses {
        let edge = &part.edges[e];
        match (&edge.curve, n) {
            (Curve::Circle { .. }, 1) if edge.is_closed() => circles += 1,
            (Curve::Line { .. }, 2) => {}
            _ => return false,
        }
    }
    circles == 2
}

struct Meridian<'a> {
    part: &'a Part,
    segs: &'a [ProfileSegment],
    tol: f64,
}

impl Meridian<'_> {
    fn near(&self, a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).hypot(a[1] - b[1]) <= self.tol
    }

    /// Segments other than `except` with an endpoint at `point`.
    fn at(&self, point: [f64; 2], except: &[usize]) -> Vec<usize> {
        (0..self.segs.len())
            .filter(|i| !except.contains(i))
            .filter(|&i| self.near(self.segs[i].start, point) || self.near(self.segs[i].end, point))
            .collect()
    }

    /// The end of segment `i` that is not at `point`.
    fn far(&self, i: usize, point: [f64; 2]) -> [f64; 2] {
        if self.near(self.segs[i].start, point) {
            self.segs[i].end
        } else {
            self.segs[i].start
        }
    }

    /// The one segment at `point` besides `except`, if there is exactly one.
    fn only(&self, point: [f64; 2], except: &[usize]) -> Option<usize> {
        match self.at(point, except)[..] {
            [i] => Some(i),
            _ => None,
        }
    }

    /// Whether the faces `i` bounds are claimed or not whole cones.
    fn cone_ok(&self, i: usize, claimed: &BTreeSet<usize>) -> bool {
        let s = &self.segs[i];
        s.kind == "cone"
            && !claimed.contains(&s.face)
            && complete_cone(self.part, s.face)
            && (s.end[0] - s.start[0]) > self.tol
            && (s.end[1] - s.start[1]).abs() > self.tol
            && s.start[1].min(s.end[1]) > self.tol
    }
}

fn meridian_tolerance(part: &Part) -> f64 {
    turned_bodies::tolerance(part).max(1e-5)
}

fn v_groove_of(
    m: &Meridian<'_>,
    claimed: &BTreeSet<usize>,
    a: usize,
    body: usize,
) -> Option<(usize, usize, Option<usize>, VGroove)> {
    let sa = &m.segs[a];
    let s = sign(sa.side)?;
    if !m.cone_ok(a, claimed) {
        return None;
    }
    // The left flank descends towards the groove's bottom as the station rises.
    let (outer_a, deep_a) = (sa.start, sa.end);
    if s * outer_a[1] <= s * deep_a[1] + m.tol {
        return None;
    }
    // Next: either the other flank at the same point, or one tiny flat or round.
    let at_deep = m.at(deep_a, &[a]);
    let (bottom, b, deep_b) = match at_deep[..] {
        [b] if m.segs[b].kind == "cone" => (None, b, deep_a),
        [k] if matches!(m.segs[k].kind, "cylinder" | "torus") && m.segs[k].side == sa.side => {
            let far = m.far(k, deep_a);
            (Some(k), m.only(far, &[k])?, far)
        }
        _ => return None,
    };
    let sb = &m.segs[b];
    if sb.side != sa.side || !m.cone_ok(b, claimed) || !m.near(sb.start, deep_b) {
        return None;
    }
    let outer_b = sb.end;
    if s * outer_b[1] <= s * deep_b[1] + m.tol || (outer_a[1] - outer_b[1]).abs() > EQUAL_RIMS {
        return None;
    }
    let width = outer_b[0] - outer_a[0];
    // The two rims lie on equal lands of the same body.
    for (end, at) in [(a, outer_a), (b, outer_b)] {
        let land = m.only(at, &[end])?;
        if m.segs[land].kind != "cylinder" || m.segs[land].side != sa.side {
            return None;
        }
    }
    // The bottom: a sharp circle, or a flat or round within a tenth of the width.
    let bottom_radius = match bottom {
        None => deep_a[1],
        Some(k) => {
            let sk = &m.segs[k];
            if sk.end[0] - sk.start[0] > BOTTOM_FRAC * width {
                return None;
            }
            if sk.kind == "cylinder" {
                sk.start[1]
            } else {
                let (c, r) = (sk.arc_center?, sk.arc_radius?);
                if c[0] < sk.start[0] - m.tol || c[0] > sk.end[0] + m.tol {
                    return None;
                }
                // The tube circle's deepest point, towards the axis for an external groove.
                c[1] - s * r
            }
        }
    };
    let depth = s * (outer_a[1] - bottom_radius);
    if depth <= m.tol || width <= m.tol {
        return None;
    }
    let unit = |p: [f64; 2], q: [f64; 2]| {
        let n = (q[0] - p[0]).hypot(q[1] - p[1]);
        [(q[0] - p[0]) / n, (q[1] - p[1]) / n]
    };
    let (ua, ub) = (unit(deep_a, outer_a), unit(deep_b, outer_b));
    let included = (ua[0] * ub[0] + ua[1] * ub[1])
        .clamp(-1., 1.)
        .acos()
        .to_degrees();
    if included < V_ANGLE_RANGE.0 || included > V_ANGLE_RANGE.1 {
        return None;
    }
    let mut faces = vec![sa.face, sb.face];
    if let Some(k) = bottom {
        faces.push(m.segs[k].face);
    }
    faces.sort_unstable();
    Some((
        a,
        b,
        bottom,
        VGroove {
            axis: [0.; 3],
            axis_point: [0.; 3],
            side: sa.side,
            width,
            depth,
            included_angle: included,
            mouth_diameter: 2. * outer_a[1],
            bottom_diameter: 2. * bottom_radius,
            turned_body: body,
            faces,
        },
    ))
}

fn body_frame(body: &TurnedBody) -> (V3, V3) {
    (turned_bodies::direction(body.axis), *body.axis_point)
}

fn complete<R>(
    part: &Part,
    features: &mut Features,
    family: &'static str,
    found: Vec<Occurrence<R>>,
) -> Vec<R> {
    let mut registration = Kept {
        part,
        unmeasured: super::unmeasured_faces(part).into_iter().collect(),
        claimed: Default::default(),
        defining: Default::default(),
        constituent: Default::default(),
        evidence_status: Default::default(),
    };
    let records = kept(&mut registration, family, found);
    features.defining.extend(registration.defining);
    features
        .evidence_status
        .extend(registration.evidence_status);
    records
}

/// Publish V-grooves, after the existing families: no existing record changes.
pub(super) fn append_v_grooves(ctx: &Context<'_>, features: &mut Features) {
    let claimed = claimed(features);
    let mut found = Vec::new();
    for (bi, body) in features.turned_bodies.iter().enumerate() {
        let m = Meridian {
            part: ctx.part,
            segs: &body.profile,
            tol: meridian_tolerance(ctx.part),
        };
        let (d, origin) = body_frame(body);
        let mut used = BTreeSet::new();
        for a in 0..m.segs.len() {
            let Some((a, b, _, mut record)) = v_groove_of(&m, &claimed, a, bi) else {
                continue;
            };
            if used.contains(&a) || used.contains(&b) {
                continue;
            }
            used.extend([a, b]);
            let mid = (m.segs[a].start[0] + m.segs[b].end[0]) / 2.;
            record.axis = d;
            record.axis_point = geom::add(origin, geom::scale(d, mid));
            let defining = record.faces.clone();
            if evidence::strict_status(ctx.part, &defining, &[]) == EvidenceStatus::Verified {
                found.push(Occurrence::new(record, defining, Vec::new()));
            }
        }
    }
    if found.is_empty() {
        return;
    }
    features.v_grooves = complete(ctx.part, features, "v_grooves", found);
}

fn taper_of(m: &Meridian<'_>, claimed: &BTreeSet<usize>, i: usize, body: usize) -> Option<Taper> {
    let seg = &m.segs[i];
    let s = sign(seg.side)?;
    if !m.cone_ok(i, claimed) {
        return None;
    }
    let (p0, p1) = (seg.start, seg.end);
    let (length, dr) = (p1[0] - p0[0], (p1[1] - p0[1]).abs());
    let ends = [
        m.only(p0, &[i]).filter(|&n| m.segs[n].kind != "cone"),
        m.only(p1, &[i]).filter(|&n| m.segs[n].kind != "cone"),
    ];
    let mut planes = 0;
    for n in ends {
        let n = &m.segs[n?];
        match (n.kind, n.side) {
            ("cylinder", side) if side == seg.side => {}
            ("plane_shoulder", _) => planes += 1,
            _ => return None,
        }
    }
    let included = 2. * dr.atan2(length).to_degrees();
    // A taper is slender: a wider cone is a profile transition, a chamfer or a seat.
    if included > MAX_TAPER_DEG {
        return None;
    }
    // No edge-break-sized cone is a taper, whatever it joins: equal legs (a 45 degree break,
    // as `chamfers` reads it) or a larger leg under a tenth of the larger diameter.
    let big = 2. * p0[1].max(p1[1]);
    if (length - dr).abs() <= EQUAL_RIMS || length.max(dr) < EDGE_BREAK_FRAC * big {
        return None;
    }
    if planes > 0 && s < 0. && included > MAX_BORE_TAPER_DEG {
        return None;
    }
    // A cone that leads down to a floor which rises again is a groove's flank.
    let deep = if s * p0[1] < s * p1[1] { p0 } else { p1 };
    if let Some(floor) = m.only(deep, &[i]).filter(|&n| m.segs[n].kind == "cylinder") {
        let far = m.far(floor, deep);
        if m.at(far, &[floor])
            .into_iter()
            .any(|k| s * m.far(k, far)[1] > s * deep[1] + m.tol)
        {
            return None;
        }
    }
    Some(Taper {
        axis: [0.; 3],
        axis_point: [0.; 3],
        side: seg.side,
        large_diameter: 2. * p0[1].max(p1[1]),
        small_diameter: 2. * p0[1].min(p1[1]),
        length,
        included_angle: included,
        turned_body: body,
        faces: vec![seg.face],
    })
}

/// Publish tapers after the V-grooves, so a groove's flanks are never also tapers.
pub(super) fn append_tapers(ctx: &Context<'_>, features: &mut Features) {
    let claimed = claimed(features);
    let mut found = Vec::new();
    for (bi, body) in features.turned_bodies.iter().enumerate() {
        let m = Meridian {
            part: ctx.part,
            segs: &body.profile,
            tol: meridian_tolerance(ctx.part),
        };
        let (d, origin) = body_frame(body);
        for i in 0..m.segs.len() {
            let Some(mut record) = taper_of(&m, &claimed, i, bi) else {
                continue;
            };
            let seg = &m.segs[i];
            // Large end first: the axis points to the small end.
            let (large, forward) = if seg.start[1] > seg.end[1] {
                (seg.start, d)
            } else {
                (seg.end, geom::scale(d, -1.))
            };
            record.axis = forward;
            record.axis_point = geom::add(origin, geom::scale(d, large[0]));
            let defining = record.faces.clone();
            if evidence::strict_status(ctx.part, &defining, &[]) == EvidenceStatus::Verified {
                found.push(Occurrence::new(record, defining, Vec::new()));
            }
        }
    }
    if found.is_empty() {
        return;
    }
    features.tapers = complete(ctx.part, features, "tapers", found);
}
