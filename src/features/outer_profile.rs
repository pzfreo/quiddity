//! Planar outer-profile evidence (`quiddity._outer_profile`, `_outer_profile_geometry` and
//! `RecognitionEvidence.planar_outer_profile`): one planar face's outer wire as ordered line and
//! arc supports, traversed with the material on the left about the face's outward normal, with
//! its inner loops counted and excluded, the faces of the one valid body it belongs to, and each
//! support's exact source edge. Concave and arc-only wires are kept (`schema_version` 2); a face
//! that is not a native plane, has a curve other than a line or circle, or no proved body
//! refuses with Python's reason.
//!
//! No feature, constituent association or angle requirement is made here: draftwright's profile
//! angles choose which faces to dimension and extend the supplied lines themselves.

use serde::Serialize;

use super::evidence::common_valid_solid;
use crate::kernel::brep::Part;
use crate::kernel::geom::{Curve, Surface, V3, cross, sub};
use crate::kernel::py;
use crate::kernel::sampling::edge_interval;

/// Original analytic support coincidence, in model length units (normally mm), consistent with
/// ADR0008's 1e-6 source endpoint bound. No public rounding is used.
const POSITION_TOL: f64 = 1e-6;
const DIRECTION_TOL: f64 = 2e-8;

/// An oriented finite straight support, traversed from start to end (`ProfileLine`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct ProfileLine {
    pub start: V3,
    pub end: V3,
}

impl ProfileLine {
    /// The unit direction from start to end.
    pub fn direction(&self) -> V3 {
        let length = py::dist(&self.start, &self.end);
        [0, 1, 2].map(|i| (self.end[i] - self.start[i]) / length)
    }
}

/// A finite circular arc; its signed sweep is radians about the profile normal (`ProfileArc`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct ProfileArc {
    pub start: V3,
    pub end: V3,
    pub center: V3,
    pub radius: f64,
    pub sweep: f64,
}

/// One support of an outer profile; serialises as Python's `to_dict`, with its `kind`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
#[non_exhaustive]
pub enum ProfileSupport {
    Line(ProfileLine),
    Arc(ProfileArc),
}

impl ProfileSupport {
    pub fn start(&self) -> V3 {
        match self {
            ProfileSupport::Line(l) => l.start,
            ProfileSupport::Arc(a) => a.start,
        }
    }

    pub fn end(&self) -> V3 {
        match self {
            ProfileSupport::Line(l) => l.end,
            ProfileSupport::Arc(a) => a.end,
        }
    }

    /// The same support traversed the other way.
    fn reversed(&self) -> Self {
        match self {
            ProfileSupport::Line(l) => ProfileSupport::Line(ProfileLine {
                start: l.end,
                end: l.start,
            }),
            ProfileSupport::Arc(a) => ProfileSupport::Arc(ProfileArc {
                start: a.end,
                end: a.start,
                sweep: -a.sweep,
                ..a.clone()
            }),
        }
    }
}

/// Schema 1 (`PlanarOuterProfile`): one outer wire, oriented with material left of traversal.
///
/// `normal` is the face's outward normal. Coordinates are unrounded, in the part's space.
/// Consecutive supports (including last/first) meet exactly at their finite endpoints; the
/// first support starts at the wire's least vertex (lexicographically). Circular transitions
/// stay explicit: their adjacent lines can be extended to a virtual intersection. Schema 2 marks
/// a profile with a concave turn or fewer than two lines. A serialised value carries geometry
/// only, never source or body identity.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct PlanarOuterProfile {
    pub origin: V3,
    pub normal: V3,
    pub supports: Vec<ProfileSupport>,
    pub inner_loop_count: usize,
    pub schema_version: u8,
    pub boundary_kind: &'static str,
}

/// The closed unsupported outcomes (`OuterProfileRefusalReason`). Python's inspection never
/// returns `InsufficientLineSupports` or `ConcaveProfile` (it keeps such wires as schema 2); they
/// are part of its public enumeration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum OuterProfileRefusalReason {
    NotPlanar,
    AmbiguousBody,
    UnsupportedCurve,
    InsufficientLineSupports,
    ConcaveProfile,
    InvalidBoundary,
}

impl OuterProfileRefusalReason {
    /// Python's enum value.
    pub fn python_value(self) -> &'static str {
        match self {
            Self::NotPlanar => "not_planar",
            Self::AmbiguousBody => "ambiguous_body",
            Self::UnsupportedCurve => "unsupported_curve",
            Self::InsufficientLineSupports => "insufficient_line_supports",
            Self::ConcaveProfile => "concave_profile",
            Self::InvalidBoundary => "invalid_boundary",
        }
    }
}

/// `RefusedPlanarOuterProfile`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RefusedPlanarOuterProfile {
    pub reason: OuterProfileRefusalReason,
}

/// `PlanarOuterProfileEvidence`: one profile bound to its source in this part. Body identity is
/// the exact face roster, so equal-valued bodies have distinct `body_faces`.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanarOuterProfileEvidence {
    pub profile: PlanarOuterProfile,
    /// The inspected face.
    pub face: usize,
    /// Every face of the valid solid the face belongs to, ascending.
    pub body_faces: Vec<usize>,
    /// Per support, in order, its source edge (an index into `Part::edges`; `profile_edge`).
    pub edges: Vec<usize>,
}

fn refused(reason: OuterProfileRefusalReason) -> RefusedPlanarOuterProfile {
    RefusedPlanarOuterProfile { reason }
}

/// The support's unit tangent at its end (or start) about *normal* (`_tangent`); `None` where
/// Python raises (an arc with no in-plane tangent).
fn tangent(support: &ProfileSupport, normal: V3, end: bool) -> Option<V3> {
    match support {
        ProfileSupport::Line(l) => Some(l.direction()),
        ProfileSupport::Arc(a) => {
            let radius = sub(if end { a.end } else { a.start }, a.center);
            let t = cross(normal, radius);
            let length = py::hypot(&t);
            if length == 0.0 {
                return None;
            }
            let sign = 1.0f64.copysign(a.sweep);
            Some(t.map(|v| v * sign / length))
        }
    }
}

/// The signed turn from each support into the next about *normal* (`_turns`).
///
/// Unlike Python, a cusp (a support turning straight back: a line leaving the bottom of an arc
/// it is tangent to, a fillet meeting a larger arc it is externally tangent to) takes its sign
/// from the supports' curvatures: their tangents are antiparallel, so their cross product is
/// round-off and Python's `atan2` gives ±π by its sign, which follows the part's placement.
/// Leaving the cusp, the two supports run back side by side, offset across the incoming tangent
/// by half their signed curvatures (about the normal) times the distance squared, in opposite
/// senses; the material, left of both, lies between them (a spike of material, turning +π)
/// exactly when the curvatures sum below zero, and outside both (a slit, −π) when above. Where
/// they cancel (two lines folding back) the turn is left as computed.
fn turns(supports: &[ProfileSupport], normal: V3) -> Option<Vec<f64>> {
    let n = supports.len();
    let curvature = |s: &ProfileSupport| match s {
        ProfileSupport::Line(_) => 0.0,
        ProfileSupport::Arc(a) => 1.0f64.copysign(a.sweep) / a.radius,
    };
    (0..n)
        .map(|at| {
            let (first, second) = (&supports[at], &supports[(at + 1) % n]);
            let before = tangent(first, normal, true)?;
            let after = tangent(second, normal, false)?;
            let turn = py::dot(&cross(before, after), &normal).atan2(py::dot(&before, &after));
            let (k1, k2) = (curvature(first), curvature(second));
            let sum = k1 + k2;
            Some(
                if turn.abs() > std::f64::consts::PI - DIRECTION_TOL
                    && sum.abs() > 1e-9 * (k1.abs() + k2.abs())
                {
                    -std::f64::consts::PI.copysign(sum)
                } else {
                    turn
                },
            )
        })
        .collect()
}

/// The wire's total turning: its corners' turns plus its arcs' sweeps, each summed exactly.
fn winding(turns: &[f64], supports: &[ProfileSupport]) -> f64 {
    py::fsum(turns.iter().copied())
        + py::fsum(supports.iter().filter_map(|s| match s {
            ProfileSupport::Arc(a) => Some(a.sweep),
            ProfileSupport::Line(_) => None,
        }))
}

fn finite(p: V3) -> bool {
    p.iter().all(|v| v.is_finite())
}

/// `ProfileLine.__post_init__`.
fn line(start: V3, end: V3) -> Option<ProfileLine> {
    let length = py::dist(&start, &end);
    (finite(start) && finite(end) && length.is_finite() && length != 0.0)
        .then_some(ProfileLine { start, end })
}

/// `ProfileArc.__post_init__`.
fn arc(start: V3, end: V3, center: V3, radius: f64, sweep: f64) -> Option<ProfileArc> {
    (finite(start)
        && finite(end)
        && finite(center)
        && radius.is_finite()
        && radius > 0.0
        && sweep.is_finite()
        && 0.0 < sweep.abs()
        && sweep.abs() < std::f64::consts::TAU)
        .then_some(ProfileArc {
            start,
            end,
            center,
            radius,
            sweep,
        })
}

/// `PlanarOuterProfile.__post_init__`: the profile, or `None` where Python raises.
fn profile(
    origin: V3,
    normal: V3,
    supports: Vec<ProfileSupport>,
    inner_loop_count: usize,
) -> Option<PlanarOuterProfile> {
    if !finite(origin) || !finite(normal) || (py::hypot(&normal) - 1.0).abs() > 1e-8 {
        return None;
    }
    let n = supports.len();
    if n < 2 {
        return None;
    }
    let off_plane = |p: V3| py::dot(&sub(p, origin), &normal).abs() > 1e-6;
    for (at, support) in supports.iter().enumerate() {
        if support.end() != supports[(at + 1) % n].start() {
            return None;
        }
        if off_plane(support.start()) || off_plane(support.end()) {
            return None;
        }
        if let ProfileSupport::Arc(a) = support {
            if off_plane(a.center)
                || [a.start, a.end]
                    .iter()
                    .any(|p| (py::dist(p, &a.center) - a.radius).abs() > 1e-6)
            {
                return None;
            }
            let radial = sub(a.start, a.center);
            let crossed = cross(normal, radial);
            let reconstructed = [0, 1, 2]
                .map(|i| a.center[i] + a.sweep.cos() * radial[i] + a.sweep.sin() * crossed[i]);
            if py::dist(&reconstructed, &a.end) > 1e-6 {
                return None;
            }
        }
    }
    let turns = turns(&supports, normal)?;
    if (winding(&turns, &supports) - std::f64::consts::TAU).abs() > 2e-8 {
        return None;
    }
    let lines = supports
        .iter()
        .filter(|s| matches!(s, ProfileSupport::Line(_)))
        .count();
    let schema_version = if turns.iter().any(|&t| t < -2e-8) || lines < 2 {
        2
    } else {
        1
    };
    Some(PlanarOuterProfile {
        origin,
        normal,
        supports,
        inner_loop_count,
        schema_version,
        boundary_kind: "outer",
    })
}

/// `_read_profile`: the face's outer wire as a profile with each support's edge.
fn read_profile(
    part: &Part,
    face: usize,
) -> Result<(PlanarOuterProfile, Vec<usize>), RefusedPlanarOuterProfile> {
    use OuterProfileRefusalReason::*;
    let f = &part.faces[face];
    if !matches!(f.surface, Surface::Plane { .. }) {
        return Err(refused(NotPlanar));
    }
    let invalid = || refused(InvalidBoundary);
    let outer = part.outer_loop(face).ok_or_else(invalid)?;
    let inner_loop_count = f.loops.len() - 1;
    let wire = &f.loops[outer].edges;
    // `BRepTools_WireExplorer`: each edge once, its vertices joined in order (an edge run twice,
    // a seam, makes `wire.edges()` shorter than the traversal).
    if wire.is_empty() || (1..wire.len()).any(|i| wire[..i].iter().any(|w| w.0 == wire[i].0)) {
        return Err(invalid());
    }
    // An edge whose curve the reader could not resolve carries its chord in its place: the
    // chord is not the edge's support, so the wire refuses rather than publish it as a line.
    if wire.iter().any(|&(e, _)| {
        part.unresolved_edges().contains(&e)
            || !matches!(
                part.edges[e].curve,
                Curve::Line { .. } | Curve::Circle { .. }
            )
    }) {
        return Err(refused(UnsupportedCurve));
    }
    if wire.len() < 2 {
        return Err(invalid());
    }
    let normal = part.face_normal(face, 0.0, 0.0).ok_or_else(invalid)?;
    // Each traversed edge's first vertex, by identity and point.
    let first = |&(e, forward): &(usize, bool)| {
        let edge = &part.edges[e];
        if forward {
            (edge.vertices.0, edge.start)
        } else {
            (edge.vertices.1, edge.end)
        }
    };
    let last = |&(e, forward): &(usize, bool)| {
        let edge = &part.edges[e];
        if forward {
            edge.vertices.1
        } else {
            edge.vertices.0
        }
    };
    let points: Vec<V3> = wire.iter().map(|w| first(w).1).collect();
    let origin = points[0];
    let mut supports = Vec::with_capacity(wire.len());
    for (at, w) in wire.iter().enumerate() {
        let following = (at + 1) % wire.len();
        if last(w) != first(&wire[following]).0 {
            return Err(invalid());
        }
        let (start, end) = (points[at], points[following]);
        if py::dist(&start, &end) == 0.0 {
            return Err(invalid());
        }
        let edge = &part.edges[w.0];
        // The trimmed curve's own ends (`position_at(0)`, `position_at(1)`): OpenCascade can
        // accept a vertex displaced from them within a larger imported tolerance, and using that
        // vertex would invent a different finite support though the wire stays connected.
        let (a, b) = edge_interval(
            &edge.curve,
            edge.start,
            edge.end,
            edge.same_sense,
            edge.is_closed(),
        );
        let curve_ends = (edge.curve.value(a), edge.curve.value(b));
        let apart = |(p, q): (V3, V3)| py::dist(&start, &p).max(py::dist(&end, &q));
        if apart(curve_ends).min(apart((curve_ends.1, curve_ends.0))) > POSITION_TOL {
            return Err(invalid());
        }
        let midpoint = edge.curve.value(0.5 * (a + b));
        if [start, end, midpoint]
            .iter()
            .any(|p| py::dot(&sub(*p, origin), &normal).abs() > POSITION_TOL)
        {
            return Err(invalid());
        }
        match &edge.curve {
            Curve::Line { .. } => {
                let support = line(start, end).ok_or_else(invalid)?;
                let source = line(curve_ends.0, curve_ends.1)
                    .ok_or_else(invalid)?
                    .direction();
                let d = support.direction();
                if py::dist(&d, &source).min(py::dist(&d, &source.map(|v| -v))) > DIRECTION_TOL {
                    return Err(invalid());
                }
                supports.push(ProfileSupport::Line(support));
            }
            Curve::Circle { frame, radius } => {
                let center = frame.origin;
                let sense = py::dot(&cross(sub(start, center), sub(midpoint, center)), &normal);
                if sense == 0.0 {
                    return Err(invalid());
                }
                let length = radius * (b - a).abs();
                let sweep = (length / radius).copysign(sense);
                supports.push(ProfileSupport::Arc(
                    arc(start, end, center, *radius, sweep).ok_or_else(invalid)?,
                ));
            }
            _ => unreachable!("curves were checked above"),
        }
    }
    let mut edges: Vec<usize> = wire.iter().map(|w| w.0).collect();
    // The wire's own orientation can differ from the face's outward one: choose the whole
    // boundary's winding, not an individual edge's parameter direction.
    let turns = turns(&supports, normal).ok_or_else(invalid)?;
    let mut winding = winding(&turns, &supports);
    if winding < 0.0 {
        supports = supports
            .iter()
            .rev()
            .map(ProfileSupport::reversed)
            .collect();
        edges.reverse();
        winding = -winding;
    }
    if (winding - std::f64::consts::TAU).abs() > DIRECTION_TOL {
        return Err(invalid());
    }
    let starts: Vec<V3> = supports.iter().map(ProfileSupport::start).collect();
    let mut least = 0;
    for at in 1..starts.len() {
        if py::tuple_order(&starts[at], &starts[least]).is_lt() {
            least = at;
        }
    }
    supports.rotate_left(least);
    edges.rotate_left(least);
    let profile =
        profile(supports[0].start(), normal, supports, inner_loop_count).ok_or_else(invalid)?;
    Ok((profile, edges))
}

internal_entry! {
/// `RecognitionEvidence.planar_outer_profile`: inspect one face's line/arc outer wire. Inner
/// loops are counted and excluded; non-native curves and unproved body ownership refuse (the
/// body check first, as Python's source reads it).
///
/// Body ownership is the topological validity `Part::solid_is_valid` proves, where Python's
/// graph asks `BRepCheck`. Python inspects each face once per evidence view and returns the same
/// object again; this is a pure function of the part, so callers that ask repeatedly cache it.
pub fn planar_outer_profile(
    part: &Part,
    face: usize,
) -> Result<PlanarOuterProfileEvidence, RefusedPlanarOuterProfile> {
    let ambiguous = || refused(OuterProfileRefusalReason::AmbiguousBody);
    let owner = common_valid_solid(part, &[face]).ok_or_else(ambiguous)?;
    // Exact ownership: equal geometry or touching bodies cannot join.
    let mut body_faces = part.solids[owner].faces.clone();
    if common_valid_solid(part, &body_faces) != Some(owner) {
        return Err(ambiguous());
    }
    body_faces.sort_unstable();
    body_faces.dedup();
    let (profile, edges) = read_profile(part, face)?;
    Ok(PlanarOuterProfileEvidence {
        profile,
        face,
        body_faces,
        edges,
    })
}
}

impl PlanarOuterProfileEvidence {
    /// `RecognitionEvidence.profile_edge`: support *index*'s exact source edge, an index into
    /// `Part::edges`. Panics on an index outside the roster, as Python raises `IndexError`.
    pub fn profile_edge(&self, index: usize) -> usize {
        self.edges[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(z: f64) -> Vec<ProfileSupport> {
        let p = [[0.0, 0.0, z], [1.0, 0.0, z], [1.0, 1.0, z], [0.0, 1.0, z]];
        (0..4)
            .map(|i| {
                ProfileSupport::Line(ProfileLine {
                    start: p[i],
                    end: p[(i + 1) % 4],
                })
            })
            .collect()
    }

    #[test]
    fn a_square_is_schema_one_and_its_reverse_is_refused() {
        let z = [0.0, 0.0, 1.0];
        let p = profile([0.0; 3], z, square(0.0), 0).unwrap();
        assert_eq!(p.schema_version, 1);
        let reversed: Vec<_> = square(0.0)
            .iter()
            .rev()
            .map(ProfileSupport::reversed)
            .collect();
        assert!(profile([0.0; 3], z, reversed, 0).is_none());
    }

    /// `test_profile_schema_rejects_incoherent_hand_built_geometry`: an origin off the plane, a
    /// zero normal and an open wire are refused.
    #[test]
    fn incoherent_profiles_are_refused() {
        let z = [0.0, 0.0, 1.0];
        assert!(profile([0.0, 0.0, 7.0], z, square(0.0), 0).is_none());
        assert!(profile([0.0; 3], [0.0; 3], square(0.0), 0).is_none());
        let mut open = square(0.0);
        open.pop();
        assert!(profile([0.0; 3], z, open, 0).is_none());
    }

    #[test]
    fn serialises_as_python_to_dict() {
        let p = profile([0.0; 3], [0.0, 0.0, 1.0], square(0.0), 2).unwrap();
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["supports"][0]["kind"], "line");
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["boundary_kind"], "outer");
        assert_eq!(v["inner_loop_count"], 2);
    }
}
