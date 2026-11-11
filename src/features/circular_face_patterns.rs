//! Circular face patterns (`quiddity.circular_face_patterns`): connected groups of a solid's
//! faces that repeat under rotation about an axis, proven by comparing the groups' sampled
//! surfaces rather than their face counts (STEP can split one blade differently at a seam).
//!
//! Python samples OpenCascade meshes; the port samples the exact surfaces (`kernel::cloud`), so
//! `fit_error` — a sampling-dependent bound, not a measurement — is its own.

use std::collections::BTreeSet;
use std::f64::consts::{PI, TAU};

use serde::Serialize;

use super::Context;
use super::evidence::{self, EvidenceError, Occurrence};
use super::placed::{Grid, Placed};
use crate::kernel::brep::Part;
use crate::kernel::cloud::{FaceProbe, fit_distance, quantile99};
use crate::kernel::geom::{COORD_FLOOR, Surface, V3, add, cross, scale, sub};
use crate::kernel::py;

const MIN_COUNT: usize = 3;
const MAX_COUNT: usize = 16;
const MAX_FACES: usize = 300;
const MAX_CLOUD_POINTS: usize = 1_000_000;
const MESH_DEFLECTION_FRAC: f64 = 0.0025;
const MATCH_DEFLECTIONS: f64 = 2.5;
const GROUP_AREA_FRAC: f64 = 0.005;
const CENTRAL_CENTRE_FRAC: f64 = 0.01;
const AXIS_LINE_FRAC: f64 = 0.0001;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct CircularFacePattern {
    pub axis_origin: Placed<V3>,
    pub axis_direction: V3,
    pub count: usize,
    pub pitch_degrees: f64,
    pub seed_index: usize,
    pub fit_error: f64,
}

impl CircularFacePattern {
    /// The fields in the order Python's `__lt__` compares them.
    fn sort_key(&self) -> Vec<f64> {
        let mut key = self.axis_origin.to_vec();
        key.extend(self.axis_direction);
        key.extend([
            self.count as f64,
            self.pitch_degrees,
            self.seed_index as f64,
            self.fit_error,
        ]);
        key
    }
}

internal_entry! {
/// `recognise_circular_face_patterns`.
pub fn recognise_circular_face_patterns(part: &Part) -> Vec<CircularFacePattern> {
    super::records(discover(&Context::new(part)))
}
}

internal_entry! {
/// The evidence path: each pattern with all its groups' faces.
pub fn discover_verified(
    ctx: &Context<'_>,
) -> Result<Vec<Occurrence<CircularFacePattern>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx))
}
}

#[derive(Clone, Copy)]
struct Axis {
    origin: V3,
    direction: V3,
    score: f64,
}

struct FaceFact {
    face: usize,
    centre: V3,
    area: f64,
    kind: &'static str,
    axis: Option<Axis>,
}

/// `_face_axis`: a cylinder's or torus's axis, pointing along its dominant component.
fn face_axis(part: &Part, face: usize, area: f64) -> Option<Axis> {
    let frame = match &part.faces[face].surface {
        Surface::Cylinder { frame, .. } | Surface::Torus { frame, .. } => frame,
        _ => return None,
    };
    let mut direction = py::unit(frame.z)?;
    let dominant = py::first_max(&direction, |c| c.abs());
    if direction[dominant] < 0.0 {
        direction = scale(direction, -1.0);
    }
    Some(Axis {
        origin: frame.origin,
        direction,
        score: area,
    })
}

fn same_axis(a: &Axis, b: &Axis, part_scale: f64) -> bool {
    py::dot(&a.direction, &b.direction).abs() > 1.0 - 1e-8
        && length(cross(sub(a.origin, b.origin), a.direction))
            <= COORD_FLOOR.max(part_scale * AXIS_LINE_FRAC)
}

/// `_length`: a plain square root of the exactly rounded dot product.
fn length(v: V3) -> f64 {
    py::dot(&v, &v).sqrt()
}

/// `_axes`: the face axes merged by line and ranked by the area on them (at most four), or the
/// world axes through the centre when no face has one; each moved to pass nearest the centre.
fn axes(facts: &[FaceFact], centre: V3, part_scale: f64) -> Vec<Axis> {
    let mut merged: Vec<Axis> = Vec::new();
    for axis in facts.iter().filter_map(|f| f.axis) {
        match merged.iter_mut().find(|m| same_axis(m, &axis, part_scale)) {
            Some(m) => m.score += axis.score,
            None => merged.push(axis),
        }
    }
    // A stable sort on the negated score, as Python's.
    merged.sort_by(|a, b| (-a.score).total_cmp(&-b.score));
    // A four-axis budget may not select the first of equally scored physical axes.
    if merged.len() > 4 && merged[3].score == merged[4].score {
        // Only the cutoff group is ambiguous. Higher-supported axes remain valid trials;
        // refusing the entire list would lose a uniquely supported principal pattern.
        let cutoff = merged[3].score;
        merged.retain(|a| a.score > cutoff);
    } else {
        merged.truncate(4);
    }
    if merged.is_empty() {
        // Trial directions must come from the part, rather than three caller-space axes.
        return Vec::new();
    }
    merged
        .into_iter()
        .map(|a| Axis {
            origin: add(
                a.origin,
                scale(a.direction, py::dot(&sub(centre, a.origin), &a.direction)),
            ),
            ..a
        })
        .collect()
}

/// `_basis`: two directions completing the axis to a right-handed frame.
fn basis(direction: V3) -> Option<(V3, V3)> {
    let smallest = py::first_min(&direction, |c| c.abs());
    let mut reference = [0.0; 3];
    reference[smallest] = 1.0;
    let first = py::unit(cross(direction, reference))?;
    Some((first, cross(direction, first)))
}

fn angle(centre: V3, axis: &Axis, (b0, b1): (V3, V3)) -> f64 {
    let offset = sub(centre, axis.origin);
    py::modulo(py::dot(&offset, &b1).atan2(py::dot(&offset, &b0)), TAU)
}

fn connected(part: &Part, group: &BTreeSet<usize>) -> bool {
    let Some(&first) = group.iter().next() else {
        return false;
    };
    let mut reached = BTreeSet::from([first]);
    let mut pending = vec![first];
    while let Some(f) = pending.pop() {
        for n in part.neighbours(f) {
            if group.contains(&n) && reached.insert(n) {
                pending.push(n);
            }
        }
    }
    reached.len() == group.len()
}

/// `_groups`: each way of splitting the moving faces into *count* sectors (phases midway
/// between the faces' angular residues) whose groups have matching areas, per surface kind
/// too, and are each connected.
fn groups(
    part: &Part,
    facts: &[FaceFact],
    axis: &Axis,
    count: usize,
    excluded: &BTreeSet<usize>,
) -> Vec<Vec<BTreeSet<usize>>> {
    let moving: Vec<&FaceFact> = facts
        .iter()
        .filter(|f| !excluded.contains(&f.face))
        .collect();
    if moving.len() < count * 2 {
        return Vec::new();
    }
    let pitch = TAU / count as f64;
    let Some(frame) = basis(axis.direction) else {
        return Vec::new();
    };
    let angles: Vec<f64> = moving
        .iter()
        .map(|f| angle(f.centre, axis, frame))
        .collect();
    let mut residues: Vec<f64> = angles.iter().map(|&a| py::modulo(a, pitch)).collect();
    residues.sort_by(f64::total_cmp);
    residues.dedup();
    let phases: Vec<f64> = (0..residues.len())
        .map(|i| {
            let next = residues.get(i + 1).copied().unwrap_or(residues[0] + pitch);
            (residues[i] + next) / 2.0
        })
        .collect();
    let mut seen: BTreeSet<Vec<Vec<usize>>> = BTreeSet::new();
    let mut out = Vec::new();
    for phase in phases {
        let mut groups = vec![BTreeSet::new(); count];
        for (f, &a) in moving.iter().zip(&angles) {
            let index = (py::modulo(a - phase, TAU) / pitch) as usize;
            if let Some(g) = groups.get_mut(index) {
                g.insert(f.face);
            }
        }
        if groups.iter().any(|g| g.len() < 2) {
            continue;
        }
        let identity: Vec<Vec<usize>> =
            groups.iter().map(|g| g.iter().copied().collect()).collect();
        if !seen.insert(identity) {
            continue;
        }
        let fact = |face: usize| moving.iter().find(|f| f.face == face).unwrap();
        let area_of = |g: &BTreeSet<usize>, kind: Option<&str>| {
            py::sum(
                g.iter()
                    .map(|&n| fact(n))
                    .filter(|f| kind.is_none_or(|k| f.kind == k))
                    .map(|f| f.area),
            )
        };
        let areas: Vec<f64> = groups.iter().map(|g| area_of(g, None)).collect();
        let mean = py::sum(areas.iter().copied()) / count as f64;
        if areas
            .iter()
            .any(|a| (a - mean).abs() > mean * GROUP_AREA_FRAC)
        {
            continue;
        }
        let kinds = |g: &BTreeSet<usize>| -> BTreeSet<&'static str> {
            g.iter().map(|&n| fact(n).kind).collect()
        };
        let first = &groups[0];
        let mismatched = groups[1..].iter().any(|other| {
            kinds(first).union(&kinds(other)).any(|&k| {
                (area_of(first, Some(k)) - area_of(other, Some(k))).abs() > mean * GROUP_AREA_FRAC
            })
        });
        if mismatched {
            continue;
        }
        if groups.iter().all(|g| connected(part, g)) {
            out.push(groups);
        }
    }
    out
}

/// Whether the 99th percentile of the points' distances is within *tolerance*. Points are
/// measured in a spread-out order, stopping once more than the top 1% are beyond it.
fn within_quantile(points: &[V3], tolerance: f64, distance: impl Fn(V3) -> f64) -> bool {
    let n = points.len();
    let beyond_allowed = n - ((n - 1) as f64 * 0.99).floor() as usize;
    // A stride coprime with n visits every point once.
    let stride = (1..)
        .map(|k| 7919 * k + 1)
        .find(|s| gcd(*s, n) == 1)
        .unwrap_or(1);
    let mut d = Vec::with_capacity(n);
    let mut beyond = 0;
    for i in 0..n {
        let x = distance(points[i * stride % n]);
        beyond += usize::from(x > tolerance);
        if beyond >= beyond_allowed {
            return false;
        }
        d.push(x);
    }
    quantile99(d) <= tolerance
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// `_rotated`: the points turned by *angle* about the axis.
fn rotated(points: &[V3], axis: &Axis, angle: f64) -> Vec<V3> {
    let (c, s) = (angle.cos(), angle.sin());
    let d = axis.direction;
    points
        .iter()
        .map(|&p| {
            let offset = sub(p, axis.origin);
            let parallel = scale(d, crate::kernel::geom::dot(offset, d));
            let transverse = sub(offset, parallel);
            add(
                add(add(axis.origin, parallel), scale(transverse, c)),
                scale(cross(d, transverse), s),
            )
        })
        .collect()
}

/// Face clouds sampled on first use, refusing past a total budget (Python's mesh budget).
struct Clouds<'a> {
    part: &'a Part,
    deflection: f64,
    by_face: std::collections::HashMap<usize, Vec<V3>>,
    total: usize,
}

impl Clouds<'_> {
    fn get(&mut self, face: usize) -> Option<&[V3]> {
        if !self.by_face.contains_key(&face) {
            let cloud = self.part.face_cloud(face, self.deflection);
            self.total += cloud.len();
            if self.total > MAX_CLOUD_POINTS {
                return None;
            }
            self.by_face.insert(face, cloud);
        }
        self.by_face.get(&face).map(Vec::as_slice)
    }
}

/// `_recognise_solid`: the first axis and count (most first) whose groups match after one
/// pitch, with the faces that turn onto themselves (hubs, bores) left out.
fn recognise_solid(
    part: &Part,
    solid: usize,
    faces: &[usize],
) -> Option<(CircularFacePattern, Vec<Vec<usize>>)> {
    if faces.len() < MIN_COUNT * 2 || faces.len() > MAX_FACES {
        return None;
    }
    let bounds = part.solid_bounds(solid);
    let part_scale = bounds.max_extent();
    if part_scale <= COORD_FLOOR {
        return None;
    }
    let deflection = part_scale * MESH_DEFLECTION_FRAC;
    let tolerance = deflection * MATCH_DEFLECTIONS;
    // A face whose area cannot be integrated leaves the solid unread (not read as area 0).
    let mut facts: Vec<FaceFact> = faces
        .iter()
        .map(|&face| {
            let area = part.face_mass(face)?[0];
            Some(FaceFact {
                face,
                centre: part.face_centre(face).unwrap_or([f64::NAN; 3]),
                area,
                kind: kind_name(&part.faces[face].surface),
                axis: face_axis(part, face, area),
            })
        })
        .collect::<Option<_>>()?;
    if facts.iter().all(|f| f.axis.is_none()) {
        // Planar fallback trials pass through the surface-area centroid, which moves with
        // this solid. A plane's STEP parameter origin need not sit on a symmetry axis.
        let area = py::fsum(facts.iter().map(|f| f.area));
        if area <= 0.0 {
            return None;
        }
        let centre = [0, 1, 2].map(|i| py::fsum(facts.iter().map(|f| f.area * f.centre[i])) / area);
        for fact in &mut facts {
            if let Surface::Plane { frame } = part.faces[fact.face].surface {
                fact.axis = Some(Axis {
                    origin: centre,
                    direction: frame.z,
                    score: fact.area,
                });
            }
        }
    }
    let mut clouds = Clouds {
        part,
        deflection,
        by_face: Default::default(),
        total: 0,
    };
    let mut probes = std::collections::HashMap::new();
    let mut winner = None;
    let mut winner_score = None;
    'axis: for axis in axes(&facts, bounds.centre(), part_scale) {
        if winner_score.is_some_and(|score| axis.score < score) {
            break;
        }
        'count: for count in (MIN_COUNT..=MAX_COUNT).rev() {
            let pitch = TAU / count as f64;
            let mut excluded = BTreeSet::new();
            for fact in &facts {
                let central = length(cross(sub(fact.centre, axis.origin), axis.direction))
                    <= part_scale * CENTRAL_CENTRE_FRAC
                    || fact.axis.is_some_and(|a| same_axis(&a, &axis, part_scale));
                if !central {
                    continue;
                }
                let Some(cloud) = clouds.get(fact.face) else {
                    continue 'count;
                };
                if cloud.is_empty() {
                    continue 'count;
                }
                // Whether the face turns onto itself, measured exactly (Python's mesh test
                // answers by how its point counts happen to divide).
                let probe = probes
                    .entry(fact.face)
                    .or_insert_with(|| FaceProbe::new(part, fact.face));
                let onto_itself = [pitch, -pitch].into_iter().all(|turn| {
                    within_quantile(&rotated(cloud, &axis, turn), tolerance, |q| {
                        probe.distance(q)
                    })
                });
                if onto_itself {
                    excluded.insert(fact.face);
                }
            }
            for groups in groups(part, &facts, &axis, count, &excluded) {
                let mut group_clouds = Vec::new();
                for g in &groups {
                    let mut cloud = Vec::new();
                    for &f in g {
                        let Some(c) = clouds.get(f) else {
                            continue 'count;
                        };
                        cloud.extend_from_slice(c);
                    }
                    group_clouds.push(cloud);
                }
                let mut fit_error: f64 = 0.0;
                for i in 0..count {
                    let turned = rotated(&group_clouds[i], &axis, pitch);
                    let Some(d) = fit_distance(&turned, &group_clouds[(i + 1) % count]) else {
                        continue 'count;
                    };
                    fit_error = fit_error.max(d);
                }
                if fit_error <= tolerance {
                    let record = CircularFacePattern {
                        axis_origin: Placed::new(axis.origin, Grid::Decimals(6)),
                        axis_direction: axis.direction.map(|c| py::round_to(c, 9)),
                        count,
                        pitch_degrees: py::round_to(pitch * (180.0 / PI), 9),
                        seed_index: 0,
                        fit_error: py::round_to(fit_error, 6),
                    };
                    let groups = groups
                        .into_iter()
                        .map(|g| g.into_iter().collect())
                        .collect();
                    // Equal-ranked distinct physical axes prove no unique circular reading.
                    if winner.is_some() {
                        return None;
                    }
                    winner_score = Some(axis.score);
                    winner = Some((record, groups));
                    continue 'axis;
                }
            }
        }
    }
    winner
}

/// The surface's `geom_type` name, as the area-per-kind comparison keys it.
fn kind_name(surface: &Surface) -> &'static str {
    match surface {
        Surface::Plane { .. } => "PLANE",
        Surface::Cylinder { .. } => "CYLINDER",
        Surface::Cone { .. } => "CONE",
        Surface::Sphere { .. } => "SPHERE",
        Surface::Torus { .. } => "TORUS",
        Surface::Freeform { kind, .. } | Surface::Other { kind } => kind,
    }
}

internal_entry! {
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<CircularFacePattern>> {
    discover_with_groups(ctx)
        .into_iter()
        .map(|(o, _)| o)
        .collect()
}
}

internal_entry! {
/// `discover`, each pattern with its repeated face groups in order (Python's
/// `Evidence.groups`; the occurrence's defining faces are their union).
pub fn discover_with_groups(
    ctx: &Context<'_>,
) -> Vec<(Occurrence<CircularFacePattern>, Vec<Vec<usize>>)> {
    let part = ctx.part;
    // Solids in order of their first face, each with its faces, when it is valid.
    let mut solids: Vec<(usize, Vec<usize>)> = Vec::new();
    for face in 0..part.faces.len() {
        let Some(s) = part.faces[face].solid.filter(|&s| part.solid_is_valid(s)) else {
            continue;
        };
        match solids.iter_mut().find(|(owner, _)| *owner == s) {
            Some((_, faces)) => faces.push(face),
            None => solids.push((s, vec![face])),
        }
    }
    let mut out: Vec<(Occurrence<CircularFacePattern>, Vec<Vec<usize>>)> = solids
        .iter()
        .filter_map(|(s, faces)| recognise_solid(part, *s, faces))
        .map(|(record, groups)| {
            let occurrence = Occurrence {
                record,
                defining: groups.iter().flatten().copied().collect(),
                context: Vec::new(),
            };
            (occurrence, groups)
        })
        .collect();
    out.sort_by(|a, b| py::tuple_order(&a.0.record.sort_key(), &b.0.record.sort_key()));
    out
}
}

#[cfg(test)]
mod axis_tie_tests {
    use super::*;
    #[test]
    fn axis_budget_refuses_equal_support_after_motion_and_input_reversal() {
        for angle in [0_f64, 30., 90.] {
            let (s, c) = angle.to_radians().sin_cos();
            let rotate = |v: V3| [c * v[0] - s * v[1], s * v[0] + c * v[1], v[2]];
            let mut facts: Vec<_> = [
                [1., 0., 0.],
                [0., 1., 0.],
                [0., 0., 1.],
                [1., 1., 0.],
                [1., 0., 1.],
            ]
            .into_iter()
            .enumerate()
            .map(|(face, d)| FaceFact {
                face,
                centre: [123.456, -78.9, 41.3],
                area: 1.,
                kind: "CYLINDER",
                axis: Some(Axis {
                    origin: [123.456, -78.9, 41.3],
                    direction: py::unit(rotate(d)).unwrap(),
                    score: 1.,
                }),
            })
            .collect();
            assert!(axes(&facts, [123.456, -78.9, 41.3], 10.).is_empty());
            facts.reverse();
            assert!(axes(&facts, [123.456, -78.9, 41.3], 10.).is_empty());
            // An ambiguous budget cutoff cannot discard a stronger, independent axis.
            facts[0].axis.as_mut().unwrap().score = 2.;
            let expected = facts[0].axis.unwrap().direction;
            let kept = axes(&facts, [123.456, -78.9, 41.3], 10.);
            assert_eq!(kept.len(), 1);
            assert_eq!(kept[0].direction, expected);
            facts.reverse();
            assert_eq!(axes(&facts, [123.456, -78.9, 41.3], 10.).len(), 1);
        }
    }
}
