//! Oriented slots (`quiddity.oriented_slots`): rectangular through slots whose in-plane width and
//! long directions are not principal in the part's frame, and their linear arrays and grids.
//!
//! An oriented slot is a reissue of an accepted rectangular [`SectionPassage`], not a new
//! discovery: the passage's serialized section must be a rectangle (opposite sides equal and
//! antiparallel, adjacent ones perpendicular, to within the serialization error) that is not a
//! square, and at least one of its in-plane directions must be off the principal axes (a
//! principal rectangle belongs to the axis-letter [`super::slots`] family). The public
//! `recognise_oriented_slots` reads the section-ring proposals directly, as Python's does; the
//! evidence path (`discover`) reads the completed passages and claims exactly the walls each
//! one proved.
//!
//! Patterns (`recognise_oriented_slot_patterns`) are derived from the slots as slot patterns
//! are from slots ([`super::recess_patterns`]): grouped by directions, size, run, depth plane and
//! body; a slot whose body is ambiguous joins none. Python's `local_degradation` graph mode is
//! not ported, as for passages: the default inventory retries in it on three corpus parts, and
//! this family's check of the flag is reached on none of them (`tests/local_degradation.rs`).

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use super::Context;
use super::body::BodyKey;
use super::evidence::{Occurrence, common_valid_solid};
use super::passages::{PassageError, SectionPassage, section_passage_record};
use super::pattern_geometry::{Candidate, Located, linear_array_candidates, plane_uv, rect_grid};
use super::policy::AXIS_ALIGNED_COS;
use super::section_passages::section_ring_proposals;
use crate::kernel::brep::Part;
use crate::kernel::geom::V3;
use crate::kernel::py;

/// Dimensions are serialized to 0.001 model units; these projection bounds stay conservative
/// for the section's 0.0001 points. They are not a feature-size tolerance.
const SERIALIZATION_QUANTUM: f64 = 1e-3;
const VECTOR_ERROR: f64 = 4.0 * SERIALIZATION_QUANTUM;

type Checked<T> = Result<T, PassageError>;

/// A rectangular through slot with free in-plane width and long directions (`OrientedSlot`).
/// `source` is the accepted rectangular passage it was issued from: its frame's run and its run
/// interval are the slot's depth direction and span. Directions to 6 decimals, sizes and centre
/// to 3; `body_key` is `None` when the slot's body is ambiguous.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct OrientedSlot {
    pub source: SectionPassage,
    pub width_direction: V3,
    pub long_direction: V3,
    pub width: f64,
    pub length: f64,
    pub center: V3,
    pub body_key: Option<BodyKey>,
}

impl OrientedSlot {
    /// The source passage's run direction (`depth_direction`).
    pub fn depth_direction(&self) -> V3 {
        self.source.frame.run
    }

    /// The fields in the order Python's `__lt__` compares them. Every oriented slot's section
    /// has four vertices, so flattening keeps the comparison lexicographic. Python cannot order
    /// a `None` body key against a key (it raises); here `None` sorts first.
    fn sort_key(&self) -> Vec<f64> {
        let s = &self.source;
        let mut key: Vec<f64> = [*s.frame.origin, s.frame.run, s.frame.u, s.frame.v].concat();
        key.extend([s.run_interval.0, s.run_interval.1]);
        for v in &s.section.boundary {
            key.extend([v.point[0], v.point[1], v.bulge]);
        }
        let ends = &s.ends;
        key.extend([
            f64::from(u8::from(ends.low_capped)),
            f64::from(u8::from(ends.high_capped)),
        ]);
        key.extend(ends.low_gradient);
        key.extend(ends.high_gradient);
        key.extend(self.width_direction);
        key.extend(self.long_direction);
        key.extend([self.width, self.length]);
        key.extend(self.center);
        key.push(if self.body_key.is_some() { 1.0 } else { 0.0 });
        key.extend(self.body_key.iter().flat_map(|k| k.iter()));
        key
    }
}

impl Located for OrientedSlot {
    fn location(&self) -> V3 {
        self.center
    }
}

/// A linear array (`OrientedSlotArray`) or grid (`OrientedSlotGrid`) of identical oriented
/// slots.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum OrientedSlotPattern {
    OrientedSlotGrid {
        slots: Vec<OrientedSlot>,
        rows: usize,
        cols: usize,
        row_pitch: f64,
        col_pitch: f64,
        angle: f64,
        center: V3,
    },
    OrientedSlotArray {
        slots: Vec<OrientedSlot>,
        pitch: f64,
        direction: V3,
    },
}

fn length(v: &[f64]) -> f64 {
    py::dot(v, v).sqrt()
}

/// *vector* as a unit vector whose largest component (the last of equals) is positive, with
/// round-off components cleared (`_canonical_direction`).
fn canonical_direction(vector: V3) -> Checked<V3> {
    let norm = length(&vector);
    if norm <= 0.0 {
        return Err(PassageError("slot direction must be nonzero"));
    }
    let mut result = vector.map(|c| c / norm);
    let pivot = (0..3)
        .max_by(|&a, &b| py::order(result[a].abs(), result[b].abs()).then(a.cmp(&b)))
        .expect("three axes");
    if result[pivot] < 0.0 {
        result = result.map(|c| -c);
    }
    Ok(result.map(|c| if c.abs() < 5e-13 { 0.0 } else { c }))
}

/// A section-plane vector in world coordinates, canonical (`_world_direction`).
fn world_direction(source: &SectionPassage, vector: [f64; 2]) -> Checked<V3> {
    let (u, v) = (source.frame.u, source.frame.v);
    canonical_direction([0, 1, 2].map(|a| vector[0] * u[a] + vector[1] * v[a]))
}

/// Within the axis-alignment cosine of a principal axis (`_principal`).
fn principal(direction: V3) -> bool {
    direction
        .iter()
        .map(|c| c.abs())
        .fold(f64::NEG_INFINITY, f64::max)
        >= AXIS_ALIGNED_COS
}

/// The width and long directions and the width and length of a straight-sided, non-square
/// rectangular section (`_rectangle`), or `None` for any other section.
fn rectangle(source: &SectionPassage) -> Checked<Option<(V3, V3, f64, f64)>> {
    let boundary = &source.section.boundary;
    if boundary.len() != 4 || boundary.iter().any(|v| v.bulge != 0.0) {
        return Ok(None);
    }
    let edges: Vec<[f64; 2]> = (0..4)
        .map(|at| {
            let (p, q) = (boundary[at].point, boundary[(at + 1) % 4].point);
            [q[0] - p[0], q[1] - p[1]]
        })
        .collect();
    let lengths: Vec<f64> = edges.iter().map(|e| length(e)).collect();
    if lengths.iter().copied().fold(f64::INFINITY, f64::min) <= 2.0 * VECTOR_ERROR {
        return Ok(None);
    }
    if (0..2).any(|at| (lengths[at] - lengths[at + 2]).abs() > VECTOR_ERROR) {
        return Ok(None);
    }
    if (0..2).any(|at| {
        let sum = [0, 1].map(|i| edges[at][i] + edges[at + 2][i]);
        length(&sum) > VECTOR_ERROR
    }) {
        return Ok(None);
    }
    let orthogonal_error = VECTOR_ERROR / lengths[0].min(lengths[1]);
    if (py::dot(&edges[0], &edges[1]) / (lengths[0] * lengths[1])).abs() > orthogonal_error {
        return Ok(None);
    }
    if (lengths[0] - lengths[1]).abs() <= VECTOR_ERROR {
        return Ok(None);
    }
    let long_at = if lengths[0] > lengths[1] { 0 } else { 1 };
    let width_at = 1 - long_at;
    Ok(Some((
        world_direction(source, edges[width_at])?,
        world_direction(source, edges[long_at])?,
        lengths[width_at],
        lengths[long_at],
    )))
}

/// The oriented slot *source* is, or `None` when it is no rectangle or a principal one
/// (`_project`).
fn project(source: &SectionPassage, body_key: Option<BodyKey>) -> Checked<Option<OrientedSlot>> {
    let Some((width_direction, long_direction, width, length)) = rectangle(source)? else {
        return Ok(None);
    };
    // Principal sections belong to the axis-letter slot family; this one exists only where that
    // schema cannot represent the directions.
    if principal(width_direction) && principal(long_direction) {
        return Ok(None);
    }
    let f = &source.frame;
    let run_midpoint = 0.5 * py::sum([source.run_interval.0, source.run_interval.1]);
    Ok(Some(OrientedSlot {
        source: source.clone(),
        width_direction: width_direction.map(|c| py::round_to(c, 6)),
        long_direction: long_direction.map(|c| py::round_to(c, 6)),
        width: py::round_to(width, 3),
        length: py::round_to(length, 3),
        center: [0, 1, 2].map(|a| py::round_to(f.origin[a] + run_midpoint * f.run[a], 3)),
        body_key,
    }))
}

fn by_record(a: &OrientedSlot, b: &OrientedSlot) -> Ordering {
    py::tuple_order(&a.sort_key(), &b.sort_key())
}

internal_entry! {
/// Recognise the oriented slots of *part* (`recognise_oriented_slots`): one per rectangular
/// section-ring proposal with a non-principal direction, sorted. A proposal whose serialization
/// Python refuses is refused here too.
pub fn recognise_oriented_slots(part: &Part) -> Checked<Vec<OrientedSlot>> {
    let ctx = Context::new(part);
    // Ambiguity is a property of the whole input, not only of the bodies with a slot.
    let keys = ctx.body_keys(true);
    let mut found = Vec::new();
    for proposal in section_ring_proposals(&ctx)? {
        let record = section_passage_record(&proposal)?;
        found.extend(project(&record, keys[proposal.solid].clone())?);
    }
    found.sort_by(by_record);
    Ok(found)
}
}

internal_entry! {
/// The oriented slots among the run's completed section *passages* (`_discover`), each claiming
/// its passage's walls, sorted by record.
pub fn discover(
    ctx: &Context<'_>,
    passages: &[Occurrence<SectionPassage>],
) -> Checked<Vec<Occurrence<OrientedSlot>>> {
    let keys = ctx.body_keys(true);
    let mut found = Vec::new();
    for occurrence in passages {
        let Some(solid) = common_valid_solid(ctx.part, &occurrence.defining) else {
            return Err(PassageError(
                "completed SectionPassage occurrence has no valid solid",
            ));
        };
        if let Some(record) = project(&occurrence.record, keys[solid].clone())? {
            found.push(Occurrence {
                record,
                defining: occurrence.defining.clone(),
                context: Vec::new(),
            });
        }
    }
    found.sort_by(|a, b| by_record(&a.record, &b.record));
    Ok(found)
}
}

internal_entry! {
/// Like `discover` over the run's own passages, every occurrence's faces checked to share one
/// valid solid; for the evidence comparison.
pub fn discover_verified(ctx: &Context<'_>) -> Result<Vec<Occurrence<OrientedSlot>>, String> {
    let passages = super::passages::discover(ctx).map_err(|e| e.to_string())?;
    let found = discover(ctx, &passages).map_err(|e| e.to_string())?;
    super::evidence::verified(ctx.part, found).map_err(|e| e.to_string())
}
}

type PatternKey = (Vec<f64>, BodyKey);

/// `_pattern_key`: directions, size, run, run interval, depth plane and body. `None` when the
/// body is ambiguous: ambiguous ownership cannot authorise a pattern.
fn pattern_key(slot: &OrientedSlot) -> Option<PatternKey> {
    let body = slot.body_key.clone()?;
    let run = slot.source.frame.run;
    let mut key: Vec<f64> = [slot.width_direction, slot.long_direction].concat();
    key.extend([py::round_to(slot.width, 3), py::round_to(slot.length, 3)]);
    key.extend(run);
    key.extend([slot.source.run_interval.0, slot.source.run_interval.1]);
    key.push(py::round_to(py::dot(&slot.center, &run), 3));
    Some((key, body))
}

internal_entry! {
/// Recognise the coplanar, same-body arrays and grids of identical oriented slots
/// (`recognise_oriented_slot_patterns`): per group of three or more, the grid of the whole
/// group and every linear array, kept largest first while their members are free.
pub fn recognise_oriented_slot_patterns(slots: &[OrientedSlot]) -> Vec<OrientedSlotPattern> {
    let mut groups: Vec<(PatternKey, Vec<&OrientedSlot>)> = Vec::new();
    for slot in slots {
        let Some(key) = pattern_key(slot) else {
            continue;
        };
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, members)) => members.push(slot),
            None => groups.push((key, vec![slot])),
        }
    }
    let mut out = Vec::new();
    for (_, members) in groups {
        if members.len() < 3 {
            continue;
        }
        let (u, v) = plane_uv(members[0].depth_direction());
        let pts: Vec<(f64, f64)> = members
            .iter()
            .map(|m| {
                let c = m.center;
                (
                    py::sum((0..3).map(|i| c[i] * u[i])),
                    py::sum((0..3).map(|i| c[i] * v[i])),
                )
            })
            .collect();
        let mut candidates: Vec<Candidate<OrientedSlotPattern>> = Vec::new();
        let grid = rect_grid(
            &members,
            &pts,
            |members, rows, cols, row_pitch, col_pitch, angle, center| {
                OrientedSlotPattern::OrientedSlotGrid {
                    slots: members.iter().map(|m| (*m).clone()).collect(),
                    rows,
                    cols,
                    row_pitch,
                    col_pitch,
                    angle,
                    center,
                }
            },
        );
        if let Some(g) = grid {
            candidates.push((g, (0..members.len()).collect()));
        }
        let flat: Vec<[f64; 2]> = pts.iter().map(|p| [p.0, p.1]).collect();
        candidates.extend(linear_array_candidates(
            &members,
            &flat,
            |members: Vec<&OrientedSlot>, pitch, direction| {
                OrientedSlotPattern::OrientedSlotArray {
                    slots: members.into_iter().cloned().collect(),
                    pitch,
                    direction,
                }
            },
        ));
        out.extend(super::pattern_geometry::allocate_candidates(
            candidates, &flat,
        ));
    }
    out
}
}

#[cfg(test)]
mod tests {
    //! Branches the captured calls and the corpus (one oriented slot, 467.step) do not reach.
    use super::*;
    use crate::features::passages::{
        PassageEnds, PassageFrame, PassageSection, PassageSectionVertex,
    };
    use crate::features::sections::{PlanarSection, SectionVertex};

    /// A passage along z through 0..10 at (*x*, 2) whose section is *points* turned *degrees*,
    /// serialized and canonicalised as a published one is (the validating constructors).
    fn passage(points: &[[f64; 2]], degrees: f64, x: f64) -> SectionPassage {
        let (s, c) = degrees.to_radians().sin_cos();
        let turned = points
            .iter()
            .map(|p| {
                let q = [c * p[0] - s * p[1], s * p[0] + c * p[1]];
                SectionVertex::new(q.map(|v| py::round_to(v, 4)), 0.0).unwrap()
            })
            .collect();
        let canonical = PlanarSection::new(turned).unwrap();
        let boundary = canonical
            .boundary()
            .iter()
            .map(|v| PassageSectionVertex::new(v.point, v.bulge).unwrap())
            .collect();
        SectionPassage::new(
            PassageFrame::new(
                [x, 2.0, 0.0].into(),
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
            )
            .unwrap(),
            (0.0, 10.0).into(),
            PassageSection::new(boundary).unwrap(),
            PassageEnds::new(false, false, [0.0; 2], [0.0; 2]).unwrap(),
        )
        .unwrap()
    }

    const RECT: [[f64; 2]; 4] = [[-4.0, -1.0], [4.0, -1.0], [4.0, 1.0], [-4.0, 1.0]];

    #[test]
    fn a_turned_rectangle_is_an_oriented_slot_and_a_principal_one_is_not() {
        let slot = project(&passage(&RECT, 30.0, 1.0), None).unwrap().unwrap();
        assert_eq!(
            (slot.width, slot.length, slot.center),
            (2.0, 8.0, [1.0, 2.0, 5.0])
        );
        // Canonical: the largest component positive.
        assert_eq!(slot.long_direction, [0.866025, 0.5, 0.0]);
        // From the 4-decimal section points, as Python has it.
        assert_eq!(slot.width_direction, [-0.500011, 0.866019, 0.0]);
        assert_eq!(project(&passage(&RECT, 0.0, 1.0), None).unwrap(), None);
        // 5° is still within the axis-alignment cosine of x and y.
        assert_eq!(project(&passage(&RECT, 5.0, 1.0), None).unwrap(), None);
    }

    #[test]
    fn squares_parallelograms_and_curved_sections_are_not_slots() {
        let square = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
        assert_eq!(rectangle(&passage(&square, 30.0, 1.0)).unwrap(), None);
        let sheared = [[-4.5, -1.0], [3.5, -1.0], [4.5, 1.0], [-3.5, 1.0]];
        assert_eq!(rectangle(&passage(&sheared, 30.0, 1.0)).unwrap(), None);
        let mut curved = passage(&RECT, 30.0, 1.0);
        curved.section.boundary[0].bulge = 0.1;
        assert_eq!(rectangle(&curved).unwrap(), None);
        // Narrower than twice the serialization error.
        let thin = [[-4.0, -0.003], [4.0, -0.003], [4.0, 0.003], [-4.0, 0.003]];
        assert_eq!(rectangle(&passage(&thin, 30.0, 1.0)).unwrap(), None);
    }

    #[test]
    fn a_direction_tie_takes_the_last_axis_positive() {
        let d = canonical_direction([1.0, -1.0, 1e-14]).unwrap();
        assert_eq!(d, [-0.7071067811865475, 0.7071067811865475, 0.0]);
        assert!(canonical_direction([0.0; 3]).is_err());
    }

    #[test]
    fn ambiguous_bodies_join_no_pattern_and_sort_first() {
        let at = |x: f64, key: Option<BodyKey>| {
            let mut p = passage(&RECT, 30.0, 1.0);
            p.frame.origin = [x, 0.0, 0.0].into();
            project(&p, key).unwrap().unwrap()
        };
        let keyed: Vec<OrientedSlot> = [0.0, 20.0, 40.0]
            .map(|x| at(x, Some(vec![1.0].into())))
            .into();
        assert_eq!(recognise_oriented_slot_patterns(&keyed).len(), 1);
        let mut mixed = keyed.clone();
        mixed[1].body_key = None;
        assert!(recognise_oriented_slot_patterns(&mixed).is_empty());
        let (a, b) = (at(0.0, Some(vec![1.0].into())), at(0.0, None));
        assert_eq!(by_record(&b, &a), Ordering::Less);
    }
}
