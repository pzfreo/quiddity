//! Interior voids (`quiddity.interior_voids`, `quiddity._interior_void_grid`): enclosed air
//! inside a solid, found by sampling the solid on a grid of lines cast through the exact B-rep,
//! then traced to the original faces that bound it and the faces through which it opens.

use std::collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque};

use serde::Serialize;

use super::Context;
use super::body::BodyKey;
use super::evidence::{self, EvidenceError, Occurrence};
use super::policy::length_tol;
use super::probes::probe_samples;
use crate::kernel::brep::Part;
use crate::kernel::classify::State;
use crate::kernel::geom::{self, Bounds, COORD_FLOOR, Surface, V3};

const AXES: [V3; 6] = [
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
];
const CELLS_ON_LONGEST_AXIS: f64 = 64.0;
/// Fewer cells cannot establish a three-dimensional region.
const MIN_COMPONENT_SPAN: i64 = 3;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct InteriorVoid {
    pub body_index: usize,
    pub body_key: Option<BodyKey>,
    pub void_faces: Vec<usize>,
    pub openings: Vec<Vec<usize>>,
    pub estimated_volume: f64,
    pub volume_method: &'static str,
    pub grid_pitch: f64,
    pub enclosed_samples: usize,
    pub air_samples: usize,
}

/// A bounded-air candidate whose sampled throat does not establish an interior chamber.
/// Retain its complete measurements and source anatomy in the recognition document.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct InteriorVoidRefusal {
    #[serde(flatten)]
    pub candidate: InteriorVoid,
    pub reason: &'static str,
}

#[derive(Default)]
pub(super) struct Analysis {
    pub accepted: Vec<Occurrence<InteriorVoid>>,
    pub refusals: Vec<InteriorVoidRefusal>,
}

/// Keep the grid's historical record slots, replacing sampled sealed skins in place.
/// Only previously undetected shells are appended, so they cannot renumber existing voids.
fn with_sealed_shells(mut sampled: Analysis, sealed: Vec<Occurrence<InteriorVoid>>) -> Analysis {
    let faces: BTreeSet<_> = sealed
        .iter()
        .flat_map(|s| s.defining.iter().copied())
        .collect();
    let mut emitted = BTreeSet::new();
    sampled.accepted.retain_mut(|occurrence| {
        if let Some((index, replacement)) = sealed.iter().enumerate().find(|(_, s)| {
            s.record.body_index == occurrence.record.body_index && s.defining == occurrence.defining
        }) {
            if !emitted.insert(index) {
                return false;
            }
            *occurrence = replacement.clone();
            true
        } else {
            // A coarse component spanning a proved shell and other air cannot own that skin.
            !occurrence.defining.iter().any(|f| faces.contains(f))
        }
    });
    sampled
        .refusals
        .retain(|r| !r.candidate.void_faces.iter().any(|f| faces.contains(f)));
    sampled.accepted.extend(
        sealed
            .into_iter()
            .enumerate()
            .filter_map(|(index, occurrence)| (!emitted.contains(&index)).then_some(occurrence)),
    );
    sampled
}

internal_entry! {
/// `recognise_interior_voids`.
pub fn recognise_interior_voids(part: &Part) -> Vec<InteriorVoid> {
    super::records(discover(&Context::new(part)))
}
}

type Cell = [i64; 3];

/// The sampled grid (`VoidGrid`): cells are material, air, or neither (no clean evidence).
struct Grid {
    origin: V3,
    pitch: f64,
    dims: [i64; 3],
    material: Vec<bool>,
    air: Vec<bool>,
    components: Vec<Vec<Cell>>,
    air_samples: usize,
}

impl Grid {
    fn index(&self, c: Cell) -> Option<usize> {
        let [nx, ny, nz] = self.dims;
        ((0..nx).contains(&c[0]) && (0..ny).contains(&c[1]) && (0..nz).contains(&c[2]))
            .then(|| ((c[0] * ny + c[1]) * nz + c[2]) as usize)
    }
    fn is_material(&self, c: Cell) -> bool {
        self.index(c).is_some_and(|i| self.material[i])
    }
    fn is_air(&self, c: Cell) -> bool {
        self.index(c).is_some_and(|i| self.air[i])
    }
    fn centre(&self, c: Cell) -> V3 {
        [0, 1, 2].map(|a| self.origin[a] + (c[a] as f64 + 0.5) * self.pitch)
    }
}

fn neighbours(c: Cell) -> [Cell; 6] {
    let [x, y, z] = c;
    [
        [x - 1, y, z],
        [x + 1, y, z],
        [x, y - 1, z],
        [x, y + 1, z],
        [x, y, z - 1],
        [x, y, z + 1],
    ]
}

/// A box-centred lattice, including stable cell counts at integral pitch ratios.
fn grid_layout(bounds: &Bounds) -> Option<(V3, f64, [i64; 3])> {
    let extents = geom::sub(bounds.max, bounds.min);
    let longest = extents[0].max(extents[1]).max(extents[2]);
    if !longest.is_finite() || longest <= COORD_FLOOR {
        return None;
    }
    let pitch = longest / CELLS_ON_LONGEST_AXIS;
    let dims = extents.map(|e| {
        let cells = e / pitch;
        let nearest = cells.round();
        if (cells - nearest).abs() * pitch <= COORD_FLOOR {
            nearest as i64
        } else {
            cells.ceil() as i64
        }
    });
    // Equal padding on both sides makes the sample lattice map onto itself under
    // axis reversal and interchange, even when an extent is not a multiple of pitch.
    let origin = [0, 1, 2].map(|a| 0.5 * (bounds.min[a] + bounds.max[a] - dims[a] as f64 * pitch));
    Some((origin, pitch, dims))
}

/// `sample_void_grid`: lines along each axis through every cell centre; a cell is material when
/// all three lines through it say so, air when all three say not, and bounded air when on every
/// axis there is material on both sides.
fn sample_grid(ctx: &Context<'_>, solid: usize, bounds: &Bounds) -> Option<Grid> {
    let (origin, pitch, dims) = grid_layout(bounds)?;
    let [nx, ny, nz] = dims;
    let size = (nx * ny * nz) as usize;
    let (mut valid, mut material_votes, mut bounded_votes) =
        (vec![0u8; size], vec![0u8; size], vec![0u8; size]);
    let rays = ctx.solid_rays(solid);
    let coincidence = length_tol(pitch, 1e-6);
    for axis in 0..3 {
        let other: Vec<usize> = (0..3).filter(|&a| a != axis).collect();
        let dir = [0, 1, 2].map(|a| if a == axis { 1.0 } else { 0.0 });
        for first in 0..dims[other[0]] {
            for second in 0..dims[other[1]] {
                let mut start = [0.0; 3];
                start[axis] = origin[axis] - pitch;
                start[other[0]] = origin[other[0]] + (first as f64 + 0.5) * pitch;
                start[other[1]] = origin[other[1]] + (second as f64 + 0.5) * pitch;
                let Some(hits) = rays.hits(start, dir, (dims[axis] + 2) as f64 * pitch) else {
                    continue;
                };
                let mut crossings: Vec<f64> = Vec::new();
                for t in hits.iter().map(|h| h.t).filter(|&t| t > COORD_FLOOR) {
                    if crossings.last().is_none_or(|&last| t - last > coincidence) {
                        crossings.push(t);
                    }
                }
                // A generic line through a valid solid crosses it an even number of times; an
                // odd count is a graze or an edge, and no evidence.
                if crossings.len() % 2 == 1 {
                    continue;
                }
                for position in 0..dims[axis] {
                    let distance = (position as f64 + 1.5) * pitch;
                    if crossings
                        .iter()
                        .any(|h| (distance - h).abs() <= coincidence)
                    {
                        continue;
                    }
                    let mut c = [0i64; 3];
                    c[axis] = position;
                    c[other[0]] = first;
                    c[other[1]] = second;
                    let i = ((c[0] * ny + c[1]) * nz + c[2]) as usize;
                    valid[i] += 1;
                    let before = crossings.partition_point(|&h| h <= distance);
                    material_votes[i] += (before % 2) as u8;
                    bounded_votes[i] += u8::from(
                        !crossings.is_empty()
                            && crossings[0] < distance
                            && distance < crossings[crossings.len() - 1],
                    );
                }
            }
        }
    }
    let (mut material, mut air) = (vec![false; size], vec![false; size]);
    let mut bounded: BTreeSet<Cell> = BTreeSet::new();
    let mut air_samples = 0;
    for x in 0..nx {
        for y in 0..ny {
            for z in 0..nz {
                let i = ((x * ny + y) * nz + z) as usize;
                if valid[i] != 3 {
                    continue;
                }
                if material_votes[i] == 3 {
                    material[i] = true;
                } else if material_votes[i] == 0 {
                    air_samples += 1;
                    air[i] = true;
                    if bounded_votes[i] == 3 {
                        bounded.insert([x, y, z]);
                    }
                }
            }
        }
    }
    Some(Grid {
        origin,
        pitch,
        dims,
        material,
        air,
        components: components(bounded),
        air_samples,
    })
}

/// `_components`: face-connected groups of cells at least three cells across on every axis,
/// ordered by their smallest cell.
fn components(mut remaining: BTreeSet<Cell>) -> Vec<Vec<Cell>> {
    let mut out: Vec<Vec<Cell>> = Vec::new();
    while let Some(seed) = remaining.pop_first() {
        let mut found = vec![seed];
        let mut pending = vec![seed];
        while let Some(current) = pending.pop() {
            for n in neighbours(current) {
                if remaining.remove(&n) {
                    found.push(n);
                    pending.push(n);
                }
            }
        }
        let spans = (0..3).all(|a| {
            let (lo, hi) = found.iter().fold((i64::MAX, i64::MIN), |(lo, hi), c| {
                (lo.min(c[a]), hi.max(c[a]))
            });
            hi - lo + 1 >= MIN_COMPONENT_SPAN
        });
        if spans {
            found.sort_unstable();
            out.push(found);
        }
    }
    out.sort_by_key(|c| c[0]);
    out
}

/// `expanded_air_components`: the components whose core is wider than every sampled route to
/// the outside — a cavity behind a throat, not a constant-width pocket.
fn expanded_components(grid: &Grid) -> BTreeSet<usize> {
    let [nx, ny, nz] = grid.dims;
    let cells =
        || (0..nx).flat_map(move |x| (0..ny).flat_map(move |y| (0..nz).map(move |z| [x, y, z])));
    // Clearance: the grid distance to material.
    let mut clearance: BTreeMap<Cell, i64> = BTreeMap::new();
    let mut pending: VecDeque<Cell> = VecDeque::new();
    for c in cells().filter(|&c| grid.is_air(c)) {
        if neighbours(c).iter().any(|&n| grid.is_material(n)) {
            clearance.insert(c, 1);
            pending.push_back(c);
        }
    }
    while let Some(c) = pending.pop_front() {
        let d = clearance[&c] + 1;
        for n in neighbours(c) {
            if grid.is_air(n) && !clearance.contains_key(&n) {
                clearance.insert(n, d);
                pending.push_back(n);
            }
        }
    }
    // Widest path from the box boundary: the largest clearance a route in can keep.
    let infinity = nx.max(ny).max(nz) + 1;
    let mut widest: BTreeMap<Cell, i64> = BTreeMap::new();
    let mut frontier: BinaryHeap<(i64, std::cmp::Reverse<Cell>)> = BinaryHeap::new();
    for c in cells().filter(|&c| grid.is_air(c)) {
        if c.contains(&0) || c[0] == nx - 1 || c[1] == ny - 1 || c[2] == nz - 1 {
            widest.insert(c, infinity);
            frontier.push((infinity, std::cmp::Reverse(c)));
        }
    }
    while let Some((width, std::cmp::Reverse(c))) = frontier.pop() {
        if width < widest[&c] {
            continue;
        }
        for n in neighbours(c) {
            let Some(&clear) = clearance.get(&n) else {
                continue;
            };
            if !grid.is_air(n) {
                continue;
            }
            let next = width.min(clear);
            if next > widest.get(&n).copied().unwrap_or(0) {
                widest.insert(n, next);
                frontier.push((next, std::cmp::Reverse(n)));
            }
        }
    }
    grid.components
        .iter()
        .enumerate()
        .filter(|(_, comp)| {
            let core = comp
                .iter()
                .map(|c| clearance.get(c).copied().unwrap_or(0))
                .max()
                .unwrap_or(0);
            let throat = comp
                .iter()
                .map(|c| widest.get(c).copied().unwrap_or(0))
                .max()
                .unwrap_or(0);
            core > 0 && (throat == 0 || core as f64 >= 1.5 * throat as f64)
        })
        .map(|(i, _)| i)
        .collect()
}

/// `_escapes`: from just outside the face, does some axis (or the face's own cylinder axis)
/// reach open air? `Some(false)` when every probe was bounded on every direction, `None` when
/// no probe could be made (or the face's area, which sets the probe offset, is unknown).
fn escapes(
    ctx: &Context<'_>,
    solid: usize,
    face: usize,
    pitch: f64,
    max_distance: f64,
) -> Option<bool> {
    let part = ctx.part;
    let mut directions: Vec<V3> = AXES.to_vec();
    if let Surface::Cylinder { frame, .. } = part.faces[face].surface {
        directions.extend([frame.z, geom::scale(frame.z, -1.0)]);
    }
    let area = part.face_mass(face)?[0];
    let offset = (COORD_FLOOR * 10.0).max(pitch.min(area.sqrt()) * 1e-3);
    let mut bounded_sample = false;
    for s in probe_samples(part, face) {
        let Some(normal) = part.face_normal(face, s.uv.0, s.uv.1) else {
            continue;
        };
        let outside: Vec<_> = [-1.0, 1.0]
            .iter()
            .map(|sign| geom::add(s.point, geom::scale(normal, sign * offset)))
            .filter(|&p| ctx.solid_classifier(solid).classify(p) == State::Out)
            .collect();
        let [outside] = outside.as_slice() else {
            continue;
        };
        let outside = *outside;
        let mut all_hit = true;
        for &dir in &directions {
            let Some(hits) = ctx.solid_rays(solid).hits(outside, dir, max_distance) else {
                all_hit = false;
                continue;
            };
            if !hits.iter().any(|h| h.t > 2.0 * offset) {
                return Some(true);
            }
        }
        bounded_sample |= all_hit;
    }
    bounded_sample.then_some(false)
}

/// `_contact_faces`: from each cell of the component next to material, the first face along
/// the axis towards it.
fn contact_faces(
    ctx: &Context<'_>,
    solid: usize,
    component: &[Cell],
    grid: &Grid,
) -> Option<BTreeSet<usize>> {
    let mut found = BTreeSet::new();
    for &cell in component {
        let source = grid.centre(cell);
        for dir in AXES {
            let n = [0, 1, 2].map(|a| cell[a] + dir[a] as i64);
            if !grid.is_material(n) {
                continue;
            }
            let hits = ctx.solid_rays(solid).hits(source, dir, grid.pitch)?;
            let contacts: Vec<_> = hits
                .iter()
                .filter(|h| COORD_FLOOR * 10.0 < h.t && h.t < grid.pitch)
                .map(|h| (h.t, h.face))
                .collect();
            let nearest = nearest_contacts(&contacts)?;
            found.extend(nearest);
        }
    }
    (!found.is_empty()).then_some(found)
}

/// A ray through a shared boundary may meet several source faces at once. All equally near
/// faces are evidence of the same contact; neither ray hit order nor face number owns it.
fn nearest_contacts(hits: &[(f64, usize)]) -> Option<BTreeSet<usize>> {
    let first = hits.iter().map(|h| h.0).min_by(f64::total_cmp)?;
    Some(
        hits.iter()
            .filter(|h| (h.0 - first).abs() <= COORD_FLOOR)
            .map(|h| h.1)
            .collect(),
    )
}

/// `_opening_regions`: the faces next to the void's skin but not in it, grouped by adjacency.
fn opening_regions(
    skin: &BTreeSet<usize>,
    adjacent: &BTreeMap<usize, BTreeSet<usize>>,
) -> Vec<Vec<usize>> {
    let mut remaining: BTreeSet<usize> = skin
        .iter()
        .flat_map(|f| &adjacent[f])
        .filter(|o| !skin.contains(o))
        .copied()
        .collect();
    let mut regions = Vec::new();
    while let Some(seed) = remaining.pop_first() {
        let mut region = BTreeSet::from([seed]);
        let mut pending = vec![seed];
        while let Some(f) = pending.pop() {
            for o in &adjacent[&f] {
                if remaining.remove(o) {
                    region.insert(*o);
                    pending.push(*o);
                }
            }
        }
        regions.push(region.into_iter().collect::<Vec<_>>());
    }
    regions.sort();
    regions
}

/// `_region_faces`: grow the seed faces through every neighbour that cannot escape.
fn region_faces(
    ctx: &Context<'_>,
    solid: usize,
    seeds: &BTreeSet<usize>,
    adjacent: &BTreeMap<usize, BTreeSet<usize>>,
    pitch: f64,
    max_distance: f64,
) -> Option<(Vec<usize>, Vec<Vec<usize>>)> {
    let mut skin = seeds.clone();
    let mut decisions: BTreeMap<usize, Option<bool>> = BTreeMap::new();
    loop {
        let boundary: BTreeSet<usize> = skin
            .iter()
            .flat_map(|f| &adjacent[f])
            .filter(|o| !skin.contains(o))
            .copied()
            .collect();
        let mut enclosed = Vec::new();
        for f in boundary {
            let d = *decisions
                .entry(f)
                .or_insert_with(|| escapes(ctx, solid, f, pitch, max_distance));
            match d {
                None => return None,
                Some(false) => enclosed.push(f),
                Some(true) => {}
            }
        }
        if enclosed.is_empty() {
            break;
        }
        skin.extend(enclosed);
    }
    let openings = opening_regions(&skin, adjacent);
    Some((skin.into_iter().collect(), openings))
}

internal_entry! {
/// The evidence path: each interior void with its faces, published only from one valid solid.
pub fn discover_verified(
    ctx: &Context<'_>,
) -> Result<Vec<Occurrence<InteriorVoid>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx))
}
}

internal_entry! {
/// Every interior void, its skin faces defining it and its opening faces consulted.
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<InteriorVoid>> {
    analyse(ctx).accepted
}
}

pub(super) fn analyse(ctx: &Context<'_>) -> Analysis {
    let part = ctx.part;
    let keys = ctx.body_keys(true);
    let sealed = ctx.sealed_void_shells();
    let sealed_records: Vec<_> = sealed
        .iter()
        .map(|shell| Occurrence {
            record: InteriorVoid {
                body_index: shell.solid,
                body_key: keys[shell.solid].clone(),
                void_faces: shell.faces.clone(),
                openings: Vec::new(),
                estimated_volume: shell.volume,
                volume_method: "closed_shell",
                grid_pitch: 0.0,
                enclosed_samples: 0,
                air_samples: 0,
            },
            defining: shell.faces.clone(),
            context: Vec::new(),
        })
        .collect();
    let mut out = Vec::new();
    let mut refusals = Vec::new();
    for (body_index, key) in keys.into_iter().enumerate() {
        if !part.solid_is_valid(body_index) {
            continue;
        }
        let faces = &part.solids[body_index].faces;
        let bounds = part.solid_bounds(body_index);
        let size = geom::sub(bounds.max, bounds.min);
        let max_distance = (size[0] * size[0] + size[1] * size[1] + size[2] * size[2]).sqrt() * 2.0;
        // A cheap original-face witness gates the much larger volume sample.
        if !faces
            .iter()
            .any(|&f| escapes(ctx, body_index, f, max_distance / 64.0, max_distance) == Some(false))
        {
            continue;
        }
        let Some(grid) = sample_grid(ctx, body_index, &bounds) else {
            continue;
        };
        if grid.components.is_empty() {
            continue;
        }
        let body: BTreeSet<usize> = faces.iter().copied().collect();
        let adjacent: BTreeMap<usize, BTreeSet<usize>> = faces
            .iter()
            .map(|&f| {
                let n = part
                    .neighbours(f)
                    .into_iter()
                    .filter(|o| body.contains(o))
                    .collect();
                (f, n)
            })
            .collect();
        let mut expanded: Option<BTreeSet<usize>> = None;
        let mut seen: BTreeSet<Vec<usize>> = BTreeSet::new();
        for (index, component) in grid.components.iter().enumerate() {
            // Spot-check the grid's parity against the solid itself.
            let picks: BTreeSet<usize> = [0, component.len() / 2, component.len() - 1].into();
            if picks.iter().any(|&i| {
                ctx.solid_classifier(body_index)
                    .classify(grid.centre(component[i]))
                    != State::Out
            }) {
                continue;
            }
            let Some(seeds) = contact_faces(ctx, body_index, component, &grid) else {
                continue;
            };
            if !seeds.is_subset(&body) {
                continue;
            }
            let Some((skin, openings)) =
                region_faces(ctx, body_index, &seeds, &adjacent, grid.pitch, max_distance)
            else {
                continue;
            };
            if seen.contains(&skin) {
                continue;
            }
            let record = InteriorVoid {
                body_index,
                body_key: key.clone(),
                void_faces: skin.clone(),
                openings,
                estimated_volume: component.len() as f64 * grid.pitch.powf(3.0),
                volume_method: "six_axis_grid",
                grid_pitch: grid.pitch,
                enclosed_samples: component.len(),
                air_samples: grid.air_samples,
            };
            // A curved core can stay broad right to its opening; otherwise require a sampled
            // widening behind a throat. Refused candidates remain visible, without claiming
            // recognised geometry or acquiring a feature fingerprint.
            let spheres = skin
                .iter()
                .filter(|&&f| matches!(part.faces[f].surface, Surface::Sphere { .. }))
                .count();
            if spheres < 2 && !record.openings.is_empty() {
                let wide = expanded.get_or_insert_with(|| expanded_components(&grid));
                if !wide.contains(&index) {
                    refusals.push(InteriorVoidRefusal {
                        candidate: record,
                        reason: "widening_behind_throat_unproved",
                    });
                    continue;
                }
            }
            seen.insert(skin.clone());
            let context: Vec<usize> = record.openings.iter().flatten().copied().collect();
            out.push(Occurrence {
                record,
                defining: skin,
                context,
            });
        }
    }
    refusals.sort_by(|a, b| {
        a.candidate.void_faces.cmp(&b.candidate.void_faces).then(
            a.candidate
                .enclosed_samples
                .cmp(&b.candidate.enclosed_samples),
        )
    });
    with_sealed_shells(
        Analysis {
            accepted: out,
            refusals,
        },
        sealed_records,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampled_void_ids_survive_sealed_replacements_and_new_shells() {
        let occurrence = |body_index, face, volume_method| Occurrence {
            record: InteriorVoid {
                body_index,
                body_key: None,
                void_faces: vec![face],
                openings: if volume_method == "six_axis_grid" {
                    vec![vec![face + 1]]
                } else {
                    vec![]
                },
                estimated_volume: 12.0,
                volume_method,
                grid_pitch: if volume_method == "six_axis_grid" {
                    1.0
                } else {
                    0.0
                },
                enclosed_samples: if volume_method == "six_axis_grid" {
                    12
                } else {
                    0
                },
                air_samples: if volume_method == "six_axis_grid" {
                    24
                } else {
                    0
                },
            },
            defining: vec![face],
            context: Vec::new(),
        };
        let open = occurrence(0, 20, "six_axis_grid");
        let mut sampled_sealed = occurrence(0, 10, "six_axis_grid");
        sampled_sealed.record.openings.clear();
        let other_body = occurrence(1, 40, "six_axis_grid");
        let canonical = occurrence(0, 10, "closed_shell");
        let unseen = occurrence(0, 5, "closed_shell");
        let merged = with_sealed_shells(
            Analysis {
                accepted: vec![open.clone(), sampled_sealed, other_body.clone()],
                refusals: Vec::new(),
            },
            vec![unseen.clone(), canonical.clone()],
        );
        // /0 and /2 remain the old open records; /1 is updated in place. The new
        // shell is /3 even though it precedes all of them in body/face ordering.
        assert_eq!(merged.accepted, [open, canonical, other_body, unseen]);
    }

    #[test]
    fn lattice_maps_onto_itself_when_a_nonintegral_box_is_turned_over() {
        let bounds = Bounds {
            min: [-3.1, -1.7, 0.3],
            max: [6.4, 2.9, 4.6],
        };
        let (o, pitch, dims) = grid_layout(&bounds).unwrap();
        let translated = Bounds {
            min: bounds.max.map(|v| 123.456 - v),
            max: bounds.min.map(|v| 123.456 - v),
        };
        let (other, other_pitch, other_dims) = grid_layout(&translated).unwrap();
        assert_eq!(dims, other_dims);
        assert!((pitch - other_pitch).abs() < 1e-12);
        for axis in 0..3 {
            for cell in 0..dims[axis] {
                let point = o[axis] + (cell as f64 + 0.5) * pitch;
                let mirrored = other[axis] + (dims[axis] as f64 - cell as f64 - 0.5) * other_pitch;
                assert!((point + mirrored - 123.456).abs() < 1e-12);
            }
        }
    }
}

#[cfg(test)]
mod contact_tie_tests {
    use super::*;
    #[test]
    fn equal_contact_faces_survive_motion_and_hit_permutation() {
        for angle in [0_f64, 30., 90.] {
            let (s, c) = angle.to_radians().sin_cos();
            let moved = |p: V3| {
                [
                    c * p[0] - s * p[1] + 123.456,
                    s * p[0] + c * p[1] - 78.9,
                    p[2] + 41.3,
                ]
            };
            let origin = moved([0.; 3]);
            let contact = moved([1., 0., 0.]);
            let distance = geom::dist(origin, contact);
            let mut hits = vec![(distance, 7), (distance, 2), (distance + 1., 5)];
            assert_eq!(nearest_contacts(&hits), Some(BTreeSet::from([2, 7])));
            hits.reverse();
            assert_eq!(nearest_contacts(&hits), Some(BTreeSet::from([2, 7])));
        }
    }
}
