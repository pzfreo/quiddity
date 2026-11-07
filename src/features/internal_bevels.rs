//! Bounded straight internal bevels: a filled triangular wedge between two perpendicular
//! planar supports. Kept separate from chamfers so legacy family ranks remain stable.

use serde::Serialize;

use super::Context;
use super::evidence::{self, EvidenceError, Occurrence};
use super::placed::{Grid, Placed};
use super::planes::{coordinates, linear_quad, unit_plane_normal};
use crate::kernel::brep::Arc;
use crate::kernel::classify::State;
use crate::kernel::geom::{self, COORD_FLOOR, SMOOTH_ARC_GAP, V3};
use crate::kernel::py;
use crate::kernel::rays::RayCaster;
use crate::kernel::volume::{Probe, common_volume, probe_volume};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InternalBevel {
    pub run: V3,
    pub length: f64,
    /// Midpoint of the complete bevel strip, and the virtual corner at that run station.
    pub at: Placed<V3>,
    pub corner: Placed<V3>,
    /// Support legs in decreasing length order; directions point from corner to spring edge.
    pub leg1: f64,
    pub leg2: f64,
    pub leg1_direction: V3,
    pub leg2_direction: V3,
    /// Acute section angle, atan2(leg2, leg1), in degrees.
    pub angle: f64,
}

/// Ignore sub-grid noise only when both length and angle publication stay identical.
/// This keeps equal legs stable under numerical motion without swapping unequal sizes.
fn leg_order(a: &(f64, V3), b: &(f64, V3)) -> std::cmp::Ordering {
    let order = py::order(-py::round_to(a.0, 6), -py::round_to(b.0, 6));
    if order.is_ne() {
        return order;
    }
    let angle = |x: f64, y: f64| py::round_to(y.atan2(x).to_degrees(), 2);
    if py::round_to3(a.0) != py::round_to3(b.0) || angle(a.0, b.0) != angle(b.0, a.0) {
        return py::order(b.0, a.0);
    }
    py::tuple_order(&coordinates(a.1, 6), &coordinates(b.1, 6))
}

fn pair(ctx: &Context<'_>, face: usize, supports: [usize; 2]) -> Option<InternalBevel> {
    let part = ctx.part;
    let [a, b] = [
        unit_plane_normal(part, supports[0])?,
        unit_plane_normal(part, supports[1])?,
    ];
    if geom::dot(a, b).abs() > SMOOTH_ARC_GAP {
        return None;
    }
    let normal = unit_plane_normal(part, face)?;
    // Both supports must face into the same cavity, between the bevel normal's rays.
    // Exclude shallow draft relative to either actual support (not the caller axes).
    if [a, b].iter().any(|&n| geom::dot(normal, n) <= 0.05) {
        return None;
    }
    let edges: Vec<_> = supports
        .iter()
        .map(|&s| {
            let shared = part.shared_edges(face, s);
            (shared.len() == 1).then(|| shared[0])
        })
        .collect::<Option<_>>()?;
    let ends = edges
        .iter()
        .map(|&e| [part.edges[e].start, part.edges[e].end])
        .collect::<Vec<_>>();
    let mut run = geom::unit(geom::sub(ends[0][1], ends[0][0]))?;
    if run
        .iter()
        .find(|c| c.abs() > COORD_FLOOR)
        .is_some_and(|&c| c < 0.)
    {
        run = geom::scale(run, -1.);
    }
    let tolerance = super::bevel::cylinder_support_tolerance(part);
    if [a, b, normal]
        .iter()
        .any(|&n| geom::dot(n, run).abs() > SMOOTH_ARC_GAP)
    {
        return None;
    }
    let spans: Vec<_> = ends
        .iter()
        .map(|p| {
            let mut s = p.map(|p| geom::dot(p, run));
            s.sort_by(f64::total_cmp);
            s
        })
        .collect();
    if (0..2).any(|i| (spans[0][i] - spans[1][i]).abs() > tolerance)
        || geom::norm(geom::cross(geom::sub(ends[1][1], ends[1][0]), run)) > tolerance
    {
        return None;
    }
    let [lo, hi] = spans[0];
    let length = hi - lo;
    let station = (lo + hi) * 0.5;
    let spring = ends
        .iter()
        .map(|p| {
            let mid = geom::scale(geom::add(p[0], p[1]), 0.5);
            geom::add(mid, geom::scale(run, station - geom::dot(mid, run)))
        })
        .collect::<Vec<_>>();
    let at = geom::scale(geom::add(spring[0], spring[1]), 0.5);
    let support = super::bevel::transverse_corner(part, face, at, run)?;
    let corner = support.point;
    let vectors = [geom::sub(spring[0], corner), geom::sub(spring[1], corner)];
    let mut legs = [
        (geom::norm(vectors[0]), geom::unit(vectors[0])?),
        (geom::norm(vectors[1]), geom::unit(vectors[1])?),
    ];
    let leg_lo = legs[0].0.min(legs[1].0);
    let leg_hi = legs[0].0.max(legs[1].0);
    if leg_order(&legs[0], &legs[1]).is_gt() {
        legs.swap(0, 1);
    }
    let solid = part.faces[face].solid?;
    if leg_lo <= 2. * tolerance
        || leg_hi > 0.45 * part.solid_bounds(solid).max_extent()
        || geom::dot(legs[0].1, legs[1].1).abs() > SMOOTH_ARC_GAP
        || geom::dot(geom::sub(at, corner), normal) <= tolerance
    {
        return None;
    }
    let area = part.face_mass(face)?[0];
    if support
        .areas
        .iter()
        .any(|&s| !super::policy::clears_threshold(s, area))
    {
        return None;
    }
    let classifier = ctx.solid_classifier(solid);
    // Check the face's two material sides as well as the filled virtual corner.
    let offset = leg_lo * super::policy::INTERIOR_PROBE_FRAC;
    if classifier.classify(geom::sub(at, geom::scale(normal, offset))) != State::In
        || classifier.classify(geom::add(at, geom::scale(normal, offset))) != State::Out
    {
        return None;
    }
    // Prove the whole open wedge, inset by input precision at its boundaries. No allowed
    // missing fraction and no point-sampling substitute for unresolved volume integration.
    let triangle = [corner, spring[0], spring[1]];
    // Opposite side lengths weight the incenter; keep the original spring ordering.
    let sides = [
        geom::norm(geom::sub(spring[0], spring[1])),
        geom::norm(geom::sub(spring[1], corner)),
        geom::norm(geom::sub(spring[0], corner)),
    ];
    let perimeter: f64 = sides.iter().sum();
    let radius = geom::norm(geom::cross(
        geom::sub(spring[0], corner),
        geom::sub(spring[1], corner),
    )) / perimeter;
    if radius <= tolerance || length <= 2. * tolerance {
        return None;
    }
    let centre = (0..3).fold([0.; 3], |sum, i| {
        geom::add(sum, geom::scale(triangle[i], sides[i] / perimeter))
    });
    let triangle = triangle.map(|p| {
        geom::add(
            centre,
            geom::scale(geom::sub(p, centre), 1. - tolerance / radius),
        )
    });
    let section = |s| triangle.map(|p| geom::add(p, geom::scale(run, s - geom::dot(p, run))));
    let wedge =
        super::support_patches::ruled_prism(&section(lo + tolerance), &section(hi - tolerance))?;
    let probe = Probe::Solid(RayCaster::for_solid(&wedge, 0));
    let whole = probe_volume(&probe)?;
    if whole <= 0. || common_volume(classifier, &probe)? < whole {
        return None;
    }
    Some(InternalBevel {
        run: coordinates(run, 6),
        length: py::round_to3(length),
        at: Placed::new(at, Grid::Decimals(3)),
        corner: Placed::new(corner, Grid::Decimals(3)),
        leg1: py::round_to3(legs[0].0),
        leg2: py::round_to3(legs[1].0),
        leg1_direction: coordinates(legs[0].1, 6),
        leg2_direction: coordinates(legs[1].1, 6),
        angle: py::round_to(legs[1].0.atan2(legs[0].0).to_degrees(), 2),
    })
}

internal_entry! {
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<InternalBevel>> {
    let part = ctx.part;
    let mut out = Vec::new();
    for face in 0..part.faces.len() {
        if !linear_quad(part, face) { continue; }
        let mut neighbours = part.neighbours(face);
        neighbours.sort_unstable();
        // Bounded cavity strip: both spring joins and both terminal joins are concave.
        if neighbours.len() != 4 || neighbours.iter().any(|&n| part.arc(face, n) != Some(Arc::Concave)
            || !matches!(part.faces[n].surface, geom::Surface::Plane { .. })) { continue; }
        let records: Vec<_> = (0..4).flat_map(|i| (i+1..4).map(move |j| (i,j)))
            .filter_map(|(i,j)| pair(ctx, face, [neighbours[i], neighbours[j]])).collect();
        if let [record] = records.as_slice() {
            out.push(Occurrence { record: record.clone(), defining: vec![face], context: neighbours });
        }
    }
    out.sort_by(|a,b| py::tuple_order(a.record.at.raw(), b.record.at.raw()));
    out
}
}

internal_entry! {
pub fn discover_verified(ctx: &Context<'_>) -> Result<Vec<Occurrence<InternalBevel>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx))
}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tied_published_legs_do_not_exchange_unequal_lengths() {
        for degrees in [0_f64, 30., 90., 180.] {
            let (s, c) = degrees.to_radians().sin_cos();
            let mut legs = [(0.7474998, [c, s, 0.]), (0.7475002, [-s, c, 0.])];
            assert_eq!(py::round_to(legs[0].0, 6), py::round_to(legs[1].0, 6));
            legs.sort_by(leg_order);
            assert_eq!(
                (py::round_to3(legs[0].0), py::round_to3(legs[1].0)),
                (0.748, 0.747)
            );
            legs.reverse();
            legs.sort_by(leg_order);
            assert_eq!(legs[0].0, 0.7475002);
        }
    }
}
