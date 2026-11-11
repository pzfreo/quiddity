//! Slot and pocket patterns (`quiddity._recess_patterns`): linear arrays and rectangular grids
//! among slots or pockets of one milled spec.
//!
//! Pure arithmetic over [`Slot`] and [`Pocket`] records, derived from them as hole patterns are
//! from holes. Records are grouped by orientation, size, plane and body; each group's centres
//! are projected into the plane across its depth axis, and the grid and linear-array candidates
//! (`super::pattern_geometry`) are allocated greedily, largest first, so each record joins at
//! most one pattern. A record whose body is ambiguous (`body_key` `None`) joins none.

use serde::Serialize;

use super::body::BodyKey;
use super::pattern_geometry::{Candidate, Located, linear_array_candidates, plane_uv, rect_grid};
use super::recess_records::{Pocket, Recess, Slot};
use crate::kernel::geom::V3;
use crate::kernel::py;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum SlotPattern {
    SlotGrid {
        slots: Vec<Slot>,
        rows: usize,
        cols: usize,
        row_pitch: f64,
        col_pitch: f64,
        angle: f64,
        center: V3,
    },
    SlotArray {
        slots: Vec<Slot>,
        pitch: f64,
        direction: V3,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum PocketPattern {
    PocketGrid {
        pockets: Vec<Pocket>,
        rows: usize,
        cols: usize,
        row_pitch: f64,
        col_pitch: f64,
        angle: f64,
        center: V3,
    },
    PocketArray {
        pockets: Vec<Pocket>,
        pitch: f64,
        direction: V3,
    },
}

impl Located for Slot {
    fn location(&self) -> V3 {
        self.region_center()
    }
}

impl Located for Pocket {
    fn location(&self) -> V3 {
        self.region_center()
    }
}

/// A float of a spec key, snapped to 3 dp so boolean-op noise does not split an array.
fn snap(x: f64) -> f64 {
    py::round_to(x, 3)
}

/// `_slot_spec_key`: orientation, size, the through-axis extent (so slots on different-height
/// faces do not merge), proved radii and body. A slot has no floor, so no depth or opening side.
#[derive(Debug, PartialEq)]
struct SlotSpec {
    width_axis: char,
    long_axis: char,
    sizes: [f64; 4],
    end_radius: Option<f64>,
    corner_radius: Option<f64>,
    body_key: BodyKey,
}

impl SlotSpec {
    /// `None` when the slot's body is ambiguous: ambiguous ownership cannot authorise a pattern.
    fn of(s: &Slot) -> Option<Self> {
        Some(SlotSpec {
            width_axis: s.width_axis,
            long_axis: s.long_axis,
            sizes: [s.width, s.length, *s.d_lo, *s.d_hi].map(snap),
            end_radius: s.end_radius.map(snap),
            corner_radius: s.corner_radius.map(snap),
            body_key: s.body_key.clone()?,
        })
    }
}

/// `_pocket_spec_key`: as [`SlotSpec`], with the depth, the side the pocket opens to (opposite
/// pockets over one depth range are on different faces) and whether it is a corner notch.
#[derive(Debug, PartialEq)]
struct PocketSpec {
    floor_radius: Option<f64>,
    width_axis: char,
    long_axis: char,
    sizes: [f64; 5],
    open_sign: i32,
    edge_anchored: bool,
    end_radius: Option<f64>,
    corner_radius: Option<f64>,
    body_key: BodyKey,
}

impl PocketSpec {
    /// `None` when the pocket's body is ambiguous.
    fn of(p: &Pocket) -> Option<Self> {
        // A mixed boundary needs an intrinsic edge-layout comparison before it
        // can establish a shared machining spec. Do not group it by radius alone.
        if !p.floor_fillets.is_empty() && p.floor_radius.is_none() {
            return None;
        }
        Some(PocketSpec {
            floor_radius: p.floor_radius.map(snap),
            width_axis: p.width_axis,
            long_axis: p.long_axis,
            sizes: [p.width, p.length, p.depth, *p.d_lo, *p.d_hi].map(snap),
            open_sign: p.open_sign,
            edge_anchored: p.edge_anchored,
            end_radius: p.end_radius.map(snap),
            corner_radius: p.corner_radius.map(snap),
            body_key: p.body_key.clone()?,
        })
    }
}

fn owned<R: Clone>(members: &[&R]) -> Vec<R> {
    members.iter().map(|m| (*m).clone()).collect()
}

/// The patterns among *records* grouped by *spec* (`recognise_pocket_patterns` and
/// `recognise_slot_patterns` share this body): per group of three or more, the grid of the
/// whole group and every linear array, kept largest first while their members are free.
fn patterns<R, K, P>(
    records: &[R],
    spec: impl Fn(&R) -> Option<K>,
    grid: impl Fn(&[&R], usize, usize, f64, f64, f64, V3) -> P,
    linear: impl Fn(Vec<&R>, f64, V3) -> P,
) -> Vec<P>
where
    R: Recess + Located,
    K: PartialEq,
{
    let mut groups: Vec<(K, Vec<&R>)> = Vec::new();
    for r in records {
        let Some(key) = spec(r) else {
            continue;
        };
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, members)) => members.push(r),
            None => groups.push((key, vec![r])),
        }
    }
    let mut out = Vec::new();
    for (_, members) in groups {
        if members.len() < 3 {
            continue;
        }
        let mut axis = [0.0; 3];
        axis[members[0].depth_axis()] = 1.0;
        let (u, v) = plane_uv(axis);
        let pts: Vec<(f64, f64)> = members
            .iter()
            .map(|m| {
                let c = m.location();
                (
                    py::sum((0..3).map(|i| c[i] * u[i])),
                    py::sum((0..3).map(|i| c[i] * v[i])),
                )
            })
            .collect();
        let mut candidates: Vec<Candidate<P>> = Vec::new();
        if let Some(g) = rect_grid(&members, &pts, &grid) {
            candidates.push((g, (0..members.len()).collect()));
        }
        let flat: Vec<[f64; 2]> = pts.iter().map(|p| [p.0, p.1]).collect();
        candidates.extend(linear_array_candidates(&members, &flat, &linear));
        out.extend(super::pattern_geometry::allocate_candidates(
            candidates, &flat,
        ));
    }
    out
}

internal_entry! {
/// `recognise_slot_patterns`.
pub fn recognise_slot_patterns(slots: &[Slot]) -> Vec<SlotPattern> {
    patterns(
        slots,
        SlotSpec::of,
        |members, rows, cols, row_pitch, col_pitch, angle, center| SlotPattern::SlotGrid {
            slots: owned(members),
            rows,
            cols,
            row_pitch,
            col_pitch,
            angle,
            center,
        },
        |members, pitch, direction| SlotPattern::SlotArray {
            slots: owned(&members),
            pitch,
            direction,
        },
    )
}
}

internal_entry! {
/// `recognise_pocket_patterns`.
pub fn recognise_pocket_patterns(pockets: &[Pocket]) -> Vec<PocketPattern> {
    patterns(
        pockets,
        PocketSpec::of,
        |members, rows, cols, row_pitch, col_pitch, angle, center| PocketPattern::PocketGrid {
            pockets: owned(members),
            rows,
            cols,
            row_pitch,
            col_pitch,
            angle,
            center,
        },
        |members, pitch, direction| PocketPattern::PocketArray {
            pockets: owned(&members),
            pitch,
            direction,
        },
    )
}
}

#[cfg(test)]
mod tests {
    //! Branches the captured calls (one pattern per call) do not reach, each answer Python's
    //! for the same records at the corpus revision.
    use super::*;

    fn pocket(x: f64, y: f64, body: Option<BodyKey>, open_sign: i32) -> Pocket {
        Pocket {
            width_axis: 'y',
            long_axis: 'x',
            width: 4.0,
            length: 6.0,
            depth: 2.0,
            w_center: y.into(),
            lo: (x - 3.0).into(),
            hi: (x + 3.0).into(),
            d_lo: 8.0.into(),
            d_hi: 10.0.into(),
            open_sign,
            edge_anchored: false,
            rectangular_boundary: false,
            floor_radius: None,
            floor_fillets: Vec::new(),
            body_key: body,
            end_radius: None,
            corner_radius: None,
        }
    }

    fn row(xs: &[f64], y: f64) -> Vec<Pocket> {
        xs.iter()
            .map(|&x| pocket(x, y, Some(vec![1.0].into()), 1))
            .collect()
    }

    fn slot(x: f64, y: f64) -> Slot {
        Slot {
            width_axis: 'x',
            long_axis: 'y',
            width: 3.0,
            length: 8.0,
            w_center: x.into(),
            lo: (y - 4.0).into(),
            hi: (y + 4.0).into(),
            d_lo: 0.0.into(),
            d_hi: 5.0.into(),
            body_key: Some(vec![1.0].into()),
            end_radius: None,
            corner_radius: None,
        }
    }

    /// Each member's (w_center, mid-length).
    fn at<R: Recess>(members: &[R]) -> Vec<(f64, f64)> {
        members
            .iter()
            .map(|m| (m.w_center(), (m.lo() + m.hi()) / 2.0))
            .collect()
    }

    #[test]
    fn two_rows_of_unequal_pitch_are_two_arrays_not_a_grid() {
        let mut pockets = row(&[0.0, 10.0, 20.0, 30.0], 0.0);
        pockets.extend(row(&[0.0, 15.0, 30.0], 25.0));
        let found: Vec<_> = recognise_pocket_patterns(&pockets)
            .into_iter()
            .map(|p| match p {
                PocketPattern::PocketArray {
                    pockets,
                    pitch,
                    direction,
                } => (pitch, direction, at(&pockets)),
                other => panic!("not an array: {other:?}"),
            })
            .collect();
        assert_eq!(
            found,
            vec![
                (
                    10.0,
                    [1.0, 0.0, 0.0],
                    vec![(0.0, 0.0), (0.0, 10.0), (0.0, 20.0), (0.0, 30.0)]
                ),
                (
                    15.0,
                    [1.0, 0.0, 0.0],
                    vec![(25.0, 0.0), (25.0, 15.0), (25.0, 30.0)]
                ),
            ]
        );
    }

    /// The single array among *slots*: its members' places, pitch and direction.
    fn one_array(slots: &[Slot]) -> (Vec<(f64, f64)>, f64, V3) {
        match &recognise_slot_patterns(slots)[..] {
            [
                SlotPattern::SlotArray {
                    slots,
                    pitch,
                    direction,
                },
            ] => (at(slots), *pitch, *direction),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_broken_pitch_splits_runs_and_the_longer_takes_the_shared_member() {
        let slots: Vec<Slot> = [0.0, 10.0, 20.0, 35.0, 50.0, 65.0]
            .iter()
            .map(|&y| slot(0.0, y))
            .collect();
        assert_eq!(
            one_array(&slots),
            (
                vec![(0.0, 20.0), (0.0, 35.0), (0.0, 50.0), (0.0, 65.0)],
                15.0,
                [0.0, 1.0, 0.0]
            )
        );
    }

    #[test]
    fn a_diagonal_row_is_ordered_from_the_first_end_found() {
        let slots: Vec<Slot> = [3.0, 0.0, 2.0, 1.0]
            .iter()
            .map(|&i| slot(5.0 * i, 5.0 * i))
            .collect();
        // Python's value to the bit: 15/hypot(15, 15), one ulp off -1/√2.
        let d = -0.7071067811865475;
        assert_eq!(
            one_array(&slots),
            (
                vec![(15.0, 15.0), (10.0, 10.0), (5.0, 5.0), (0.0, 0.0)],
                7.07,
                [d, d, 0.0]
            )
        );
    }

    #[test]
    fn ambiguous_bodies_and_opposed_openings_form_no_pattern() {
        let mut pockets = row(&[0.0, 10.0], 0.0);
        pockets.push(pocket(20.0, 0.0, None, 1));
        assert_eq!(recognise_pocket_patterns(&pockets), vec![]);
        let opposed = vec![
            pocket(0.0, 0.0, Some(vec![1.0].into()), 1),
            pocket(10.0, 0.0, Some(vec![1.0].into()), -1),
            pocket(20.0, 0.0, Some(vec![1.0].into()), 1),
        ];
        assert_eq!(recognise_pocket_patterns(&opposed), vec![]);
    }

    #[test]
    fn a_grid_keeps_input_order_and_columns_run_along_the_shortest_pitch() {
        let slots: Vec<Slot> = [0.0, 12.0]
            .iter()
            .flat_map(|&y| [20.0, 0.0, 10.0].map(|x| slot(x, y)))
            .collect();
        assert_eq!(
            recognise_slot_patterns(&slots),
            vec![SlotPattern::SlotGrid {
                slots: slots.clone(),
                rows: 2,
                cols: 3,
                row_pitch: 12.0,
                col_pitch: 10.0,
                angle: 0.0,
                center: [10.0, 6.0, 2.5],
            }]
        );
    }
}
