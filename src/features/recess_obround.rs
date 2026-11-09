//! Slots and pockets whose ends are half-cylinders (`quiddity._recess_obround`). A stubby
//! obround recess has flat walls too short to pair, so it is recovered from its two end caps
//! (`recognise_obround_from_ends`); one already found by its walls is extended by a radius at
//! each end once both caps are found (`extend_obround_proposals`).

use std::collections::BTreeSet;

use super::policy::length_tol;
use super::policy::round_measurement;
use super::recess_faces::{
    Cylinder, Face, MERGE_TOL, center, floor_end_faces, has_side_walls, union_bb,
};
use super::recess_records::{AXES, Pocket, Recess, Slot};
use super::recess_reduce::Proposal;
use crate::kernel::geom::{Bounds, V3};
use crate::kernel::py;

/// How close a cap's radius must be to half the record's width, as a fraction of it (ADR 0008).
pub(crate) const END_RADIUS_FRAC: f64 = 0.0375;
/// How close a half-cylinder's in-plane extents must be to 2r across and r along the bulge.
const OBROUND_RATIO_TOL: f64 = 0.1;
/// Coaxial cap patches (a STEP split of one semicircle) cluster within this fraction of the
/// radius.
const CAP_CLUSTER_FRAC: f64 = 0.075;

/// One physical obround end: a cluster of coaxial concave cylinder patches whose union is a
/// half-cylinder (`_obround_end`'s tuple). `flat` is the axis position along the long axis
/// (the straight-wall junction) and `direction` the side the cap bulges to.
#[derive(Clone, Debug)]
pub struct End {
    pub width_axis: usize,
    pub long_axis: usize,
    pub axis: usize,
    pub radius: f64,
    pub w_center: f64,
    pub flat: f64,
    pub direction: i32,
    pub d_lo: f64,
    pub d_hi: f64,
    pub patches: BTreeSet<usize>,
}

/// Classify a cluster's union box as a half-cylinder end, or `None` (`_obround_end`): 2r
/// across its width axis and r along its bulge; a round hole's full cylinder is 2r × 2r.
fn obround_end(
    radius: f64,
    axis: usize,
    location: V3,
    bb: &Bounds,
    patches: BTreeSet<usize>,
) -> Option<End> {
    if radius <= 0.0 {
        return None;
    }
    let others: Vec<usize> = (0..3).filter(|&a| a != axis).collect();
    let ext = |a: usize| bb.max[a] - bb.min[a];
    let across: Vec<usize> = others
        .iter()
        .copied()
        .filter(|&a| (ext(a) / radius - 2.0).abs() <= OBROUND_RATIO_TOL)
        .collect();
    let bulge: Vec<usize> = others
        .iter()
        .copied()
        .filter(|&a| (ext(a) / radius - 1.0).abs() <= OBROUND_RATIO_TOL)
        .collect();
    let (&[width_axis], &[long_axis]) = (&across[..], &bulge[..]) else {
        return None;
    };
    if width_axis == long_axis {
        return None;
    }
    let flat = location[long_axis];
    let direction = if (flat - bb.min[long_axis]) > (bb.max[long_axis] - flat) {
        -1
    } else {
        1
    };
    Some(End {
        width_axis,
        long_axis,
        axis,
        radius,
        // The union box's centre, not the axis location: a STEP split of one end into two
        // quarters puts their axis locations ~0.02 mm apart across the diameter.
        w_center: center(bb, width_axis),
        flat,
        direction,
        d_lo: bb.min[axis],
        d_hi: bb.max[axis],
        patches,
    })
}

internal_entry! {
/// The solid's obround ends (`_obround_ends`): concave cylinders clustered by axis, radius,
/// depth (to 0.1 mm) and axis line, each cluster's union box classified as a half-cylinder.
pub fn obround_ends(cylinders: &[Cylinder]) -> Vec<End> {
    struct Cluster {
        axis: usize,
        radius: f64,
        location: V3,
        ip: (f64, f64),
        dz: (f64, f64),
        bb: Bounds,
        nodes: BTreeSet<usize>,
    }
    let mut clusters: Vec<Cluster> = Vec::new();
    for c in cylinders {
        if !c.concave || c.radius <= 0.0 {
            continue;
        }
        let o: Vec<usize> = (0..3).filter(|&a| a != c.axis).collect();
        let ip = (c.location[o[0]], c.location[o[1]]);
        let dz = (
            py::round_to(c.bb.min[c.axis], 1),
            py::round_to(c.bb.max[c.axis], 1),
        );
        let near = |cl: &Cluster| {
            cl.axis == c.axis
                && (cl.radius - c.radius).abs() <= length_tol(c.radius, END_RADIUS_FRAC)
                && cl.dz == dz
                && (cl.ip.0 - ip.0).abs() <= length_tol(c.radius, CAP_CLUSTER_FRAC)
                && (cl.ip.1 - ip.1).abs() <= length_tol(c.radius, CAP_CLUSTER_FRAC)
        };
        match clusters.iter_mut().find(|cl| near(cl)) {
            Some(cl) => {
                cl.bb = union_bb(&cl.bb, &c.bb);
                cl.nodes.insert(c.node);
            }
            None => clusters.push(Cluster {
                axis: c.axis,
                radius: c.radius,
                location: c.location,
                ip,
                dz,
                bb: c.bb,
                nodes: BTreeSet::from([c.node]),
            }),
        }
    }
    clusters
        .into_iter()
        .filter_map(|cl| obround_end(cl.radius, cl.axis, cl.location, &cl.bb, cl.nodes))
        .collect()
}
}

/// Every distinct cap cluster matching one end of a record (`_matching_end_groups`).
fn matching_end_groups<R: Recess>(ends: &[End], record: &R, coord: f64) -> Vec<BTreeSet<usize>> {
    let radius = record.width() / 2.0;
    let mut groups: Vec<BTreeSet<usize>> = Vec::new();
    for end in ends {
        if end.axis == record.depth_axis()
            && (end.radius - radius).abs() <= length_tol(radius, END_RADIUS_FRAC)
            && (end.flat - coord).abs() <= MERGE_TOL
            && (end.w_center - record.w_center()).abs() <= MERGE_TOL
            && (end.d_lo - record.d_lo()).abs() <= MERGE_TOL
            && (end.d_hi - record.d_hi()).abs() <= MERGE_TOL
            && !end.patches.is_empty()
            && !groups.contains(&end.patches)
        {
            groups.push(end.patches.clone());
        }
    }
    groups
}

/// Two cap clusters compete for one end of a record: which faces end it is undecided.
#[derive(Debug, PartialEq, Eq)]
pub struct CompetingCaps;

internal_entry! {
/// Each record with a cap at both ends extended by its half-width at each end, keeping both
/// cap clusters (`_extend_obround_proposals`). With *strict*, two clusters at one end refuse.
pub fn extend_obround_proposals<R: Recess>(
    proposals: Vec<Proposal<R>>,
    cylinders: &[Cylinder],
    strict: bool,
) -> Result<Vec<Proposal<R>>, CompetingCaps> {
    let ends = obround_ends(cylinders);
    let mut out = Vec::with_capacity(proposals.len());
    for proposal in proposals {
        let record = &proposal.record;
        let low = matching_end_groups(&ends, record, record.lo());
        let high = matching_end_groups(&ends, record, record.hi());
        if low.len() > 1 || high.len() > 1 {
            if strict {
                return Err(CompetingCaps);
            }
            continue; // the record path also refuses; no first cap cluster owns the claim
        }
        if let (Some(low), Some(high)) = (low.first(), high.first()) {
            let low_radius = ends.iter().find(|e| e.patches == *low).unwrap().radius;
            let high_radius = ends.iter().find(|e| e.patches == *high).unwrap().radius;
            let mut extended = record.spanned(
                record.lo() - low_radius,
                record.hi() + high_radius,
                round_measurement(record.hi() - record.lo() + low_radius + high_radius, 2),
            );
            extended.set_end_radius(round_measurement(0.5 * (low_radius + high_radius), 2));
            let mut caps = proposal.caps.clone();
            for group in [low, high] {
                if !caps.contains(group) {
                    caps.push(group.clone());
                }
            }
            out.push(Proposal {
                record: extended,
                planar: proposal.planar,
                caps,
                floors: proposal.floors,
                constituent: BTreeSet::new(),
            });
        } else {
            out.push(proposal);
        }
    }
    Ok(out)
}
}

/// The legacy key ends are grouped by: axes, and radius, centreline and depth to 0.01 mm.
fn legacy_key(e: &End) -> (usize, usize, usize, [f64; 4]) {
    (
        e.width_axis,
        e.long_axis,
        e.axis,
        [e.radius, e.w_center, e.d_lo, e.d_hi].map(|v| py::round_to(v, 2)),
    )
}

/// Ends grouped by their rounded key, two singletons merged only where they are one stubby
/// recess's two ends split across a rounding boundary and each other's only partner
/// (`_compatible_end_groups`).
fn compatible_end_groups(ends: &[End]) -> Vec<Vec<&End>> {
    let mut legacy: Vec<(_, Vec<&End>)> = Vec::new();
    for end in ends {
        let key = legacy_key(end);
        match legacy.iter_mut().find(|(k, _)| *k == key) {
            Some((_, group)) => group.push(end),
            None => legacy.push((key, vec![end])),
        }
    }
    let groups: Vec<Vec<&End>> = legacy.into_iter().map(|(_, g)| g).collect();
    let mut partners: Vec<Vec<(usize, &End, &End)>> = vec![Vec::new(); groups.len()];
    for index in 0..groups.len() {
        if groups[index].len() != 1 {
            continue;
        }
        for other_index in index + 1..groups.len() {
            if groups[other_index].len() != 1 {
                continue;
            }
            let (a, b) = (groups[index][0], groups[other_index][0]);
            // Sorted by flat; equal flats keep their order.
            let (low, high) = if py::order(b.flat, a.flat).is_lt() {
                (b, a)
            } else {
                (a, b)
            };
            let radius = py::round_to(low.radius, 2);
            let tolerance = length_tol(radius, CAP_CLUSTER_FRAC);
            let (lk, hk) = (legacy_key(low), legacy_key(high));
            let base_matches = (lk.0, lk.1, lk.2) == (hk.0, hk.1, hk.2)
                && lk.3[0] == hk.3[0]
                && lk.3[2] == hk.3[2]
                && lk.3[3] == hk.3[3];
            if base_matches
                && (low.w_center - high.w_center).abs() <= tolerance
                && low.direction == -1
                && high.direction == 1
                && high.flat - low.flat <= 2.0 * radius + tolerance
            {
                partners[index].push((other_index, low, high));
                partners[other_index].push((index, low, high));
            }
        }
    }
    let mut out = Vec::new();
    let mut consumed = vec![false; groups.len()];
    for (index, group) in groups.iter().enumerate() {
        if consumed[index] {
            continue;
        }
        if group.len() != 1 {
            out.push(group.clone());
            continue;
        }
        // Admissible only when both ends name each other as their sole partner.
        match partners[index][..] {
            [(other, low, high)] if partners[other].len() == 1 => {
                consumed[other] = true;
                out.push(vec![low, high]);
            }
            _ => out.push(group.clone()),
        }
    }
    out
}

/// Obround recesses recovered from their end caps (`_recognise_obround_from_ends`): ends of one
/// centreline, radius and depth sorted along the run; a cap bulging to −long followed by one
/// bulging to +long, more than `MERGE_TOL` apart, is one recess's two ends, confirmed by its
/// side walls and routed by its floors: through (no floor) to `Slot`, blind (exactly one) to
/// `Pocket`. `lo`/`hi` are the straight-wall junctions, extended later as the wall path is.
fn obround_from_ends(faces: &[Face], cylinders: &[Cylinder], blind: bool) -> Vec<Recovered> {
    let ends = obround_ends(cylinders);
    let mut out = Vec::new();
    for group in compatible_end_groups(&ends) {
        let first = group[0];
        let rad = first.radius;
        let (dlo, dhi) = (first.d_lo, first.d_hi);
        let wc = group.iter().map(|e| e.w_center).sum::<f64>() / group.len() as f64;
        let mut run = group.clone();
        run.sort_by(|a, b| py::order(a.flat, b.flat));
        let mut i = 0;
        while i + 1 < run.len() {
            let (lo_end, hi_end) = (run[i], run[i + 1]);
            if !(lo_end.direction == -1
                && hi_end.direction == 1
                && hi_end.flat - lo_end.flat > MERGE_TOL)
            {
                i += 1;
                continue;
            }
            let (lo_f, hi_f) = (lo_end.flat, hi_end.flat);
            let s = Slot {
                width_axis: AXES[first.width_axis],
                long_axis: AXES[first.long_axis],
                width: round_measurement(2.0 * rad, 2),
                length: round_measurement(hi_f - lo_f, 2),
                w_center: wc.into(),
                lo: lo_f.into(),
                hi: hi_f.into(),
                d_lo: dlo.into(),
                d_hi: dhi.into(),
                body_key: Some(Vec::new().into()),
                end_radius: None,
                corner_radius: None,
            };
            // Side walls join the ends, rather than two D-cutouts bridging solid; on failure
            // the caps may belong to a later pair.
            if !has_side_walls(faces, &s) {
                i += 1;
                continue;
            }
            let (floor_lo, floor_hi) = floor_end_faces(faces, &s);
            let floors = usize::from(!floor_lo.is_empty()) + usize::from(!floor_hi.is_empty());
            let caps = vec![lo_end.patches.clone(), hi_end.patches.clone()];
            if blind && floors == 1 {
                let record = Pocket {
                    width_axis: s.width_axis,
                    long_axis: s.long_axis,
                    width: s.width,
                    length: s.length,
                    depth: round_measurement(first.d_hi - first.d_lo, 2),
                    w_center: s.w_center,
                    lo: lo_f.into(),
                    hi: hi_f.into(),
                    d_lo: s.d_lo,
                    d_hi: s.d_hi,
                    open_sign: if floor_lo.is_empty() { -1 } else { 1 },
                    edge_anchored: false,
                    rectangular_boundary: false,
                    floor_radius: None,
                    floor_fillets: Vec::new(),
                    body_key: Some(Vec::new().into()),
                    end_radius: None,
                    corner_radius: None,
                };
                let selected = if floor_lo.is_empty() {
                    floor_hi
                } else {
                    floor_lo
                };
                out.push(Recovered::Pocket(Proposal {
                    caps,
                    floors: selected.into_iter().collect(),
                    ..Proposal::new(record)
                }));
                i += 2;
            } else if !blind && floors == 0 {
                out.push(Recovered::Slot(Proposal {
                    caps,
                    ..Proposal::new(s)
                }));
                i += 2;
            } else {
                // A pocket in a slot scan, a slot in a pocket scan, or a sealed void.
                i += 1;
            }
        }
    }
    out
}

/// A recess recovered from its ends: the slot scan's or the pocket scan's.
enum Recovered {
    Slot(Proposal<Slot>),
    Pocket(Proposal<Pocket>),
}

internal_entry! {
/// The through obround slots recovered from their end caps.
pub fn slots_from_ends(faces: &[Face], cylinders: &[Cylinder]) -> Vec<Proposal<Slot>> {
    obround_from_ends(faces, cylinders, false)
        .into_iter()
        .filter_map(|r| match r {
            Recovered::Slot(p) => Some(p),
            Recovered::Pocket(_) => None,
        })
        .collect()
}
}

internal_entry! {
/// The blind obround pockets recovered from their end caps.
pub fn pockets_from_ends(faces: &[Face], cylinders: &[Cylinder]) -> Vec<Proposal<Pocket>> {
    obround_from_ends(faces, cylinders, true)
        .into_iter()
        .filter_map(|r| match r {
            Recovered::Pocket(p) => Some(p),
            Recovered::Slot(_) => None,
        })
        .collect()
}
}

#[cfg(test)]
mod ambiguity_tests {
    use super::*;
    #[test]
    fn competing_caps_refuse_on_both_paths_after_axis_motion_and_input_permutation() {
        for [w, l, d] in [
            [0, 1, 2],
            [1, 0, 2],
            [2, 1, 0],
            [0, 2, 1],
            [1, 2, 0],
            [2, 0, 1],
        ] {
            for shift in [0., 123.456] {
                let cap = |node, centre, flat, sign: f64| {
                    let mut location = [shift; 3];
                    location[w] += centre;
                    location[l] += flat;
                    let mut min = [shift; 3];
                    let mut max = [shift; 3];
                    min[w] += centre - 2.;
                    max[w] += centre + 2.;
                    min[l] += flat + (-2. * sign).min(0.);
                    max[l] += flat + (-2. * sign).max(0.);
                    max[d] += 3.;
                    Cylinder {
                        node,
                        axis: d,
                        radius: 2.,
                        location,
                        bb: crate::kernel::geom::Bounds { min, max },
                        concave: true,
                    }
                };
                let mut cylinders =
                    vec![cap(9, 0., 0., 1.), cap(2, 0.2, 0., 1.), cap(5, 0., 8., -1.)];
                let record = Slot {
                    width_axis: AXES[w],
                    long_axis: AXES[l],
                    width: 4.,
                    length: 8.,
                    w_center: shift.into(),
                    lo: shift.into(),
                    hi: (shift + 8.).into(),
                    d_lo: shift.into(),
                    d_hi: (shift + 3.).into(),
                    body_key: None,
                    end_radius: None,
                    corner_radius: None,
                };
                for _ in 0..3 {
                    assert_eq!(
                        extend_obround_proposals(
                            vec![Proposal::new(record.clone())],
                            &cylinders,
                            true
                        )
                        .unwrap_err(),
                        CompetingCaps
                    );
                    assert!(
                        extend_obround_proposals(
                            vec![Proposal::new(record.clone())],
                            &cylinders,
                            false
                        )
                        .unwrap()
                        .is_empty()
                    );
                    cylinders.rotate_left(1);
                }
            }
        }
    }
}
