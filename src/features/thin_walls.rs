//! Thin-wall bodies (`quiddity.thin_walls`): solids whose skins pair up across locally constant
//! wall thicknesses, measured by casting each face's normal through the material to the
//! opposite skin.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::Serialize;

use super::Context;
use super::body::BodyKey;
use super::evidence::{self, EvidenceError, Occurrence};
use super::policy::length_tol;
use super::probes::{Sample, probe_samples};
use crate::kernel::brep::Part;
use crate::kernel::classify::State;
use crate::kernel::geom::{self, COORD_FLOOR, Surface, V3};
use crate::kernel::py;

const PAIR_REL_TOL: f64 = 3e-4;
const SPLINE_BLEND_REL_TOL: f64 = 5e-3;
const OPPOSED_NORMAL_COS: f64 = -0.9999;
const MIN_PAIRED_AREA_FRAC: f64 = 0.85;
const MIN_CLASS_AREA_FRAC: f64 = 0.05;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct WallFacePair {
    pub first_face: usize,
    pub second_face: usize,
    pub thickness: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct UnpairedWallFace {
    pub face: usize,
    pub kind: &'static str,
}

/// An explicitly heuristic reading of how the shell was made; the finished B-rep cannot prove it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct ShellHistoryHint {
    pub basis: &'static str,
    pub direction: Option<&'static str>,
    pub outer_faces: Vec<usize>,
    pub inner_faces: Vec<usize>,
    pub opening_rims: Vec<Vec<usize>>,
    pub before_shell_collar_pairs: Vec<WallFacePair>,
    pub after_shell_cut_faces: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct ThinWallBody {
    pub body_index: usize,
    pub body_key: Option<BodyKey>,
    pub thickness: f64,
    pub face_pairs: Vec<WallFacePair>,
    pub unpaired_faces: Vec<usize>,
    pub rim_regions: Vec<Vec<usize>>,
    pub paired_area_fraction: f64,
    pub history_hint: ShellHistoryHint,
    pub unpaired_face_classes: Vec<UnpairedWallFace>,
}

internal_entry! {
/// `recognise_thin_wall_bodies`.
pub fn recognise_thin_wall_bodies(part: &Part) -> Vec<ThinWallBody> {
    super::records(discover(&Context::new(part)))
}
}

/// The surface classes the wall heuristics distinguish (`BRepAdaptor_Surface.GetType`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Plane,
    Cylinder,
    BSpline,
    Other,
}

fn kind(surface: &Surface) -> Kind {
    match surface {
        Surface::Plane { .. } => Kind::Plane,
        Surface::Cylinder { .. } => Kind::Cylinder,
        Surface::Freeform {
            kind: "BSPLINE", ..
        } => Kind::BSpline,
        _ => Kind::Other,
    }
}

#[derive(Clone, Copy, Debug)]
struct Hit {
    target: usize,
    distance: f64,
}

/// `_samples`: the probes inside the face, or, when fewer than two land inside (a narrow patch
/// or a long concave trim), up to five more spread over the face's interior. Python takes the
/// centroids of its mesh's five largest triangles there; any interior points serve, and these
/// come from the exact trimmed face.
fn samples(part: &Part, face: usize) -> Vec<Sample> {
    let mut points = probe_samples(part, face);
    if points.len() >= 2 {
        return points;
    }
    let Some(domain) = part.domain(face) else {
        return points;
    };
    const N: usize = 9;
    let inside: Vec<Sample> = (0..N * N)
        .filter_map(|k| {
            let (i, j) = (k / N, k % N);
            let (u, v) = ((i as f64 + 0.5) / N as f64, (j as f64 + 0.5) / N as f64);
            part.position_at(face, u, v)
        })
        .filter(|(_, (u, v))| domain.contains(*u, *v))
        .map(|(point, uv)| Sample { point, uv })
        .collect();
    let n = inside.len();
    if n <= 5 {
        points.extend(inside);
    } else {
        let mut taken: Vec<Option<Sample>> = inside.into_iter().map(Some).collect();
        for k in 0..5 {
            if let Some(s) = taken[k * (n - 1) / 4].take() {
                points.push(s);
            }
        }
    }
    points
}

/// `_first_material_hit`: along the face's normal one way, then the other, the first face the
/// ray meets through material, provided it faces back. `Err` when the sample gives no ray at
/// all (a degenerate normal).
fn first_material_hit(
    ctx: &Context<'_>,
    solid: usize,
    face: usize,
    s: &Sample,
    local: &HashMap<usize, usize>,
    span: f64,
) -> Result<Option<Hit>, ()> {
    let part = ctx.part;
    let normal = part.face_normal(face, s.uv.0, s.uv.1).ok_or(())?;
    for sign in [-1.0, 1.0] {
        let dir = geom::scale(normal, sign);
        let hits = ctx.solid_rays(solid).hits(s.point, dir, span).ok_or(())?;
        let Some(first) = hits.iter().find(|h| h.t > COORD_FLOOR * 10.0) else {
            continue;
        };
        let midpoint = geom::add(s.point, geom::scale(dir, first.t / 2.0));
        if ctx.solid_classifier(solid).classify(midpoint) != State::In {
            continue;
        }
        let Some(&target) = local.get(&first.face) else {
            continue;
        };
        let end = geom::add(s.point, geom::scale(dir, first.t));
        let opposite = part.normal_at_point(first.face, end).ok_or(())?;
        if geom::dot(normal, opposite) > OPPOSED_NORMAL_COS {
            continue;
        }
        return Ok(Some(Hit {
            target,
            distance: first.t,
        }));
    }
    Ok(None)
}

type BodyPairs = (f64, Vec<(usize, usize, f64)>, f64);

/// `_body_pairs`, over one solid's faces by their local index.
fn body_pairs(ctx: &Context<'_>, solid: usize) -> Option<BodyPairs> {
    let part = ctx.part;
    let faces = &part.solids[solid].faces;
    if faces.len() < 4 {
        return None;
    }
    let b = part.solid_bounds(solid);
    let size = geom::sub(b.max, b.min);
    let span = size[0].max(size[1]).max(size[2]);
    let areas: Vec<f64> = faces
        .iter()
        .map(|&f| part.face_mass(f).map(|m| m[0]))
        .collect::<Option<_>>()?;
    let total_area = py::fsum(areas.iter().copied());
    let (volume, _) = part.solid_mass(solid)?;
    if total_area <= 0.0 || 2.0 * volume / total_area > 0.2 * span {
        return None;
    }
    let local: HashMap<usize, usize> = faces.iter().enumerate().map(|(i, &f)| (f, i)).collect();
    let kinds: Vec<Kind> = faces
        .iter()
        .map(|&f| kind(&part.faces[f].surface))
        .collect();
    let matches = |source: usize, hit: &Hit, thickness: f64| {
        let rel = if kinds[source] == Kind::BSpline && kinds[hit.target] == Kind::BSpline {
            SPLINE_BLEND_REL_TOL
        } else {
            PAIR_REL_TOL
        };
        (hit.distance - thickness).abs() <= length_tol(thickness, rel)
    };

    let mut hits: BTreeMap<usize, Vec<Hit>> = BTreeMap::new();
    for (index, &face) in faces.iter().enumerate() {
        let found: Vec<Hit> = samples(part, face)
            .iter()
            .filter_map(|s| {
                first_material_hit(ctx, solid, face, s, &local, span)
                    .ok()
                    .flatten()
            })
            .filter(|h| h.target != index)
            .collect();
        if !found.is_empty() {
            hits.insert(index, found);
        }
    }

    let mut distances: Vec<f64> = hits.values().flatten().map(|h| h.distance).collect();
    if distances.len() < 4 {
        return None;
    }
    distances.sort_by(|a, b| py::order(*a, *b));
    let mut clusters: Vec<Vec<f64>> = Vec::new();
    for d in distances {
        match clusters.last_mut() {
            Some(c) if (d - c[0]).abs() <= length_tol(c[0], PAIR_REL_TOL) => c.push(d),
            _ => clusters.push(vec![d]),
        }
    }
    clusters.sort_by(|a, b| b.len().cmp(&a.len()).then(py::order(a[0], b[0])));

    let mut pair_thickness: BTreeMap<(usize, usize), f64> = BTreeMap::new();
    let mut class_areas: Vec<(f64, f64)> = Vec::new();
    let mut class_pair_counts: Vec<usize> = Vec::new();
    for cluster in &clusters {
        if cluster.len() < 4 {
            continue;
        }
        let thickness = py::fsum(cluster.iter().copied()) / cluster.len() as f64;
        let mut local_pairs: BTreeSet<(usize, usize)> = BTreeSet::new();
        for (&source, samples) in &hits {
            let matching: Vec<&Hit> = samples
                .iter()
                .filter(|h| matches(source, h, thickness))
                .collect();
            if matching.len() < 2 || matching.len() * 5 < samples.len() * 4 {
                continue;
            }
            for hit in matching {
                let back = hits.get(&hit.target).map_or(&[][..], |v| v.as_slice());
                if back
                    .iter()
                    .any(|b| b.target == source && matches(hit.target, b, thickness))
                {
                    local_pairs.insert((source.min(hit.target), source.max(hit.target)));
                }
            }
        }
        let owned: BTreeSet<usize> = pair_thickness.keys().flat_map(|&(a, b)| [a, b]).collect();
        let new_pairs: BTreeSet<(usize, usize)> = local_pairs
            .into_iter()
            .filter(|p| {
                !pair_thickness.contains_key(p) && !owned.contains(&p.0) && !owned.contains(&p.1)
            })
            .collect();
        if new_pairs.is_empty() {
            continue;
        }
        let class_faces: BTreeSet<usize> = new_pairs.iter().flat_map(|&(a, b)| [a, b]).collect();
        let class_area = py::fsum(class_faces.iter().map(|&i| areas[i]));
        let largest = class_faces
            .iter()
            .map(|&i| areas[i])
            .fold(f64::NEG_INFINITY, f64::max);
        if thickness > 0.3 * largest.sqrt() || class_area < MIN_CLASS_AREA_FRAC * total_area {
            continue;
        }
        class_areas.push((class_area, thickness));
        class_pair_counts.push(new_pairs.len());
        for &(a, b) in &new_pairs {
            let ds: Vec<f64> = [(a, b), (b, a)]
                .iter()
                .flat_map(|&(source, target)| {
                    hits.get(&source)
                        .into_iter()
                        .flatten()
                        .filter(move |h| h.target == target)
                        .filter(move |h| matches(source, h, thickness))
                        .map(|h| h.distance)
                })
                .collect();
            pair_thickness.insert((a, b), py::fsum(ds.iter().copied()) / ds.len() as f64);
        }
    }
    if pair_thickness.is_empty()
        || (class_areas.len() > 1 && class_pair_counts.iter().max() < Some(&2))
    {
        return None;
    }
    // The largest class's thickness; on equal area the thinner, then the first.
    let mut thickness = class_areas[0];
    for &c in &class_areas[1..] {
        if c.0 > thickness.0 || (c.0 == thickness.0 && -c.1 > -thickness.1) {
            thickness = c;
        }
    }
    let thickness = thickness.1;
    let paired: BTreeSet<usize> = pair_thickness.keys().flat_map(|&(a, b)| [a, b]).collect();
    let paired_fraction = py::fsum(paired.iter().map(|&i| areas[i])) / total_area;
    if paired_fraction < MIN_PAIRED_AREA_FRAC {
        return None;
    }
    // One opposed planar pair is a slab, not a shell: require curved skin or a second
    // planar direction.
    let mut weighted: Vec<(f64, usize, usize)> = pair_thickness
        .keys()
        .map(|&(a, b)| (areas[a] + areas[b], a, b))
        .collect();
    weighted.sort_by(|x, y| py::order(y.0, x.0).then(y.1.cmp(&x.1)).then(y.2.cmp(&x.2)));
    let curved_area = py::fsum(
        weighted
            .iter()
            .filter(|(_, a, b)| kinds[*a] != Kind::Plane || kinds[*b] != Kind::Plane)
            .map(|w| w.0),
    );
    let paired_area = py::fsum(weighted.iter().map(|w| w.0));
    let plane_normal = |i: usize| match part.faces[faces[i]].surface {
        Surface::Plane { frame } => Some(frame.z),
        _ => None,
    };
    let nonparallel_area = minimum_nonparallel_support(&weighted, plane_normal);
    if curved_area.max(nonparallel_area) < 0.1 * paired_area {
        return None;
    }
    let ratio = 2.0 * volume / total_area;
    let minimum = pair_thickness
        .values()
        .copied()
        .fold(f64::INFINITY, f64::min);
    let maximum = pair_thickness
        .values()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    if !(0.5 * minimum <= ratio && ratio <= 1.1 * maximum) {
        return None;
    }
    Some((
        thickness,
        pair_thickness
            .iter()
            .map(|(&(a, b), &t)| (a, b, t))
            .collect(),
        paired_fraction,
    ))
}

/// Connected groups of *members* through shared edges, each seeded from its smallest face.
fn components(part: &Part, members: &BTreeSet<usize>) -> Vec<BTreeSet<usize>> {
    let mut remaining = members.clone();
    let mut out = Vec::new();
    while let Some(seed) = remaining.pop_first() {
        let mut group = BTreeSet::from([seed]);
        let mut pending = vec![seed];
        while let Some(current) = pending.pop() {
            for n in part.neighbours(current) {
                if remaining.remove(&n) {
                    group.insert(n);
                    pending.push(n);
                }
            }
        }
        out.push(group);
    }
    out
}

/// The faces' areas, by part face index; `None` when one cannot be integrated (a failed area
/// is not read as 0).
fn face_areas(part: &Part, faces: impl IntoIterator<Item = usize>) -> Option<BTreeMap<usize, f64>> {
    faces
        .into_iter()
        .map(|f| part.face_mass(f).map(|m| (f, m[0])))
        .collect()
}

/// All equally largest planar pairs must support the eligibility decision; selecting the
/// first pair could turn a slab into a shell when another tied direction sees more area.
fn minimum_nonparallel_support(
    weighted: &[(f64, usize, usize)],
    normal: impl Fn(usize) -> Option<V3>,
) -> f64 {
    let largest = weighted[0].0;
    weighted
        .iter()
        .take_while(|w| py::round_to(w.0, 9) == py::round_to(largest, 9))
        .map(|&(_, a, _)| match normal(a) {
            Some(primary) => py::fsum(
                weighted
                    .iter()
                    .filter(|(_, face, _)| {
                        normal(*face).is_some_and(|n| geom::dot(primary, n).abs() < 0.95)
                    })
                    .map(|w| w.0),
            ),
            None => 0.0,
        })
        .fold(f64::INFINITY, f64::min)
}

/// Whether the two largest connected skins are selected by area without a third tied skin.
fn unique_skin_pair(comps: &[BTreeSet<usize>], area: &BTreeMap<usize, f64>) -> bool {
    let size = |g: &BTreeSet<usize>| py::round_to(py::fsum(g.iter().map(|&i| area[&i])), 9);
    comps.len() >= 2 && (comps.len() == 2 || size(&comps[1]) > size(&comps[2]))
}

/// `_skin_components`: the paired faces' connected skins, largest area first.
fn skin_components(
    part: &Part,
    area: &BTreeMap<usize, f64>,
    paired: &BTreeSet<usize>,
) -> Vec<BTreeSet<usize>> {
    let mut groups = components(part, paired);
    let size = |g: &BTreeSet<usize>| py::fsum(g.iter().map(|&i| area[&i]));
    groups.sort_by(|a, b| py::order(size(b), size(a)));
    groups
}

fn pair_faces(pairs: &[WallFacePair]) -> BTreeSet<usize> {
    pairs
        .iter()
        .flat_map(|p| [p.first_face, p.second_face])
        .collect()
}

/// `_rim_regions`: unpaired faces next to both faces of a pair, grouped by adjacency.
fn rim_regions(part: &Part, unpaired: &[usize], pairs: &[WallFacePair]) -> Vec<Vec<usize>> {
    let rims: BTreeSet<usize> = unpaired
        .iter()
        .copied()
        .filter(|&i| {
            let n: BTreeSet<usize> = part.neighbours(i).into_iter().collect();
            pairs
                .iter()
                .any(|p| n.contains(&p.first_face) && n.contains(&p.second_face))
        })
        .collect();
    components(part, &rims)
        .into_iter()
        .map(|g| g.into_iter().collect())
        .collect()
}

/// `_encloses`: *outer*'s box contains *inner*'s and is wider on at least two axes.
fn encloses(part: &Part, outer: &BTreeSet<usize>, inner: &BTreeSet<usize>, tol: f64) -> bool {
    let span = |faces: &BTreeSet<usize>, axis: usize| {
        faces
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &f| {
                let b = part.face_bounds(f);
                (lo.min(b.min[axis]), hi.max(b.max[axis]))
            })
    };
    let mut wider = 0;
    for axis in 0..3 {
        let ((ol, oh), (il, ih)) = (span(outer, axis), span(inner, axis));
        if ol > il + tol || oh < ih - tol {
            return false;
        }
        wider += usize::from(ol < il - tol || oh > ih + tol);
    }
    wider >= 2
}

/// `None` when the part's largest face, which the skins are compared with, has a face whose
/// area cannot be integrated.
fn history_hint(
    part: &Part,
    area: &BTreeMap<usize, f64>,
    pairs: &[WallFacePair],
    unpaired: &[usize],
    rims: &[Vec<usize>],
) -> Option<ShellHistoryHint> {
    let paired = pair_faces(pairs);
    let comps = skin_components(part, area, &paired);
    let (mut outer, mut inner) = (BTreeSet::new(), BTreeSet::new());
    if unique_skin_pair(&comps, area) {
        let (first, second) = (&comps[0], &comps[1]);
        let cross = pairs
            .iter()
            .filter(|p| {
                (first.contains(&p.first_face) && second.contains(&p.second_face))
                    || (first.contains(&p.second_face) && second.contains(&p.first_face))
            })
            .count();
        if cross * 2 >= first.len().min(second.len()) {
            let all = face_areas(part, 0..part.faces.len())?;
            let largest = all.into_values().fold(f64::NEG_INFINITY, f64::max);
            let tol = length_tol(largest.sqrt(), 1e-6);
            if encloses(part, first, second, tol) {
                (outer, inner) = (first.clone(), second.clone());
            } else if encloses(part, second, first, tol) {
                (outer, inner) = (second.clone(), first.clone());
            }
        }
    }
    let total_paired_area = py::fsum(paired.iter().map(|&i| area[&i]));
    let half_turn = |f: usize| {
        matches!(part.faces[f].surface, Surface::Cylinder { .. })
            && part
                .uv_bounds(f)
                .is_some_and(|(u0, u1, _, _)| u1 - u0 >= std::f64::consts::PI - 1e-3)
    };
    let collars: Vec<WallFacePair> = pairs
        .iter()
        .filter(|p| {
            half_turn(p.first_face)
                && half_turn(p.second_face)
                && area[&p.first_face] + area[&p.second_face] < 0.2 * total_paired_area
        })
        .cloned()
        .collect();
    let shelled = !outer.is_empty() && !inner.is_empty();
    let after: Vec<usize> = unpaired
        .iter()
        .copied()
        .filter(|&i| {
            shelled && matches!(part.faces[i].surface, Surface::Cylinder { .. }) && {
                let n = part.neighbours(i);
                n.iter().any(|f| outer.contains(f)) && n.iter().any(|f| inner.contains(f))
            }
        })
        .collect();
    let opening_rims: Vec<Vec<usize>> = if shelled {
        rims.iter()
            .filter(|r| !r.iter().any(|i| after.contains(i)))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    Some(ShellHistoryHint {
        basis: "heuristic",
        direction: (shelled && !opening_rims.is_empty()).then_some("inward"),
        outer_faces: outer.into_iter().collect(),
        inner_faces: inner.into_iter().collect(),
        opening_rims,
        before_shell_collar_pairs: collars,
        after_shell_cut_faces: after,
    })
}

fn classify_unpaired(
    part: &Part,
    unpaired: &[usize],
    pairs: &[WallFacePair],
    rims: &[Vec<usize>],
    hint: &ShellHistoryHint,
) -> Vec<UnpairedWallFace> {
    let paired = pair_faces(pairs);
    let rim_faces: BTreeSet<usize> = rims
        .iter()
        .flatten()
        .chain(&hint.after_shell_cut_faces)
        .copied()
        .collect();
    unpaired
        .iter()
        .map(|&i| {
            let near_paired = part
                .neighbours(i)
                .into_iter()
                .filter(|n| paired.contains(n))
                .count();
            let kind = if rim_faces.contains(&i) {
                "cut_edge"
            } else if !matches!(part.faces[i].surface, Surface::Plane { .. }) && near_paired >= 2 {
                "joint_blend"
            } else {
                "non_wall_feature"
            };
            UnpairedWallFace { face: i, kind }
        })
        .collect()
}

internal_entry! {
/// The evidence path: each thin-wall body with its faces, published only from one valid solid.
pub fn discover_verified(
    ctx: &Context<'_>,
) -> Result<Vec<Occurrence<ThinWallBody>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx))
}
}

internal_entry! {
/// Every thin-wall body, its paired faces defining it and its rim faces consulted.
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<ThinWallBody>> {
    let part = ctx.part;
    let keys = ctx.body_keys(true);
    let mut out = Vec::new();
    for (body_index, key) in keys.into_iter().enumerate() {
        if !part.solid_is_valid(body_index) {
            continue;
        }
        let Some((thickness, pairs, fraction)) = body_pairs(ctx, body_index) else {
            continue;
        };
        let faces = &part.solids[body_index].faces;
        let pairs: Vec<WallFacePair> = pairs
            .into_iter()
            .map(|(a, b, t)| WallFacePair {
                first_face: faces[a],
                second_face: faces[b],
                thickness: Some(t),
            })
            .collect();
        let paired = pair_faces(&pairs);
        let unpaired: Vec<usize> = faces
            .iter()
            .copied()
            .filter(|f| !paired.contains(f))
            .collect();
        // A body with a paired face whose area cannot be integrated is not read.
        let Some(area) = face_areas(part, paired.iter().copied()) else {
            continue;
        };
        let comps = skin_components(part, &area, &paired);
        if !unique_skin_pair(&comps, &area) {
            continue;
        }
        let component_of = |f: usize| comps.iter().position(|g| g.contains(&f));
        let both = |p: &WallFacePair| area[&p.first_face] + area[&p.second_face];
        let core_area = py::fsum(pairs.iter().filter_map(|p| {
            let (a, b) = (component_of(p.first_face), component_of(p.second_face));
            ((a, b) == (Some(0), Some(1)) || (a, b) == (Some(1), Some(0))).then(|| both(p))
        }));
        let paired_area = py::fsum(pairs.iter().map(both));
        if core_area * 2.0 <= paired_area {
            continue;
        }
        let rims = rim_regions(part, &unpaired, &pairs);
        let Some(hint) = history_hint(part, &area, &pairs, &unpaired, &rims) else {
            continue;
        };
        let classes = classify_unpaired(part, &unpaired, &pairs, &rims, &hint);
        let rim_faces: Vec<usize> = rims.iter().flatten().copied().collect();
        out.push(Occurrence {
            record: ThinWallBody {
                body_index,
                body_key: key,
                thickness,
                face_pairs: pairs,
                unpaired_faces: unpaired,
                rim_regions: rims,
                paired_area_fraction: fraction,
                history_hint: hint,
                unpaired_face_classes: classes,
            },
            defining: paired.into_iter().collect(),
            context: rim_faces,
        });
    }
    out
}
}

#[cfg(test)]
mod tie_tests {
    use super::*;
    #[test]
    fn planar_support_tie_has_one_verdict_after_rotation_and_pair_permutation() {
        for angle in [0_f64, 30., 90.] {
            let (s, c) = angle.to_radians().sin_cos();
            let mut weighted = vec![(10., 0, 0), (10., 1, 1)];
            weighted.extend((2..102).map(|i| (9., i, i)));
            let normal = |i| Some(if i == 1 { [-s, c, 0.] } else { [c, s, 0.] });
            assert_eq!(minimum_nonparallel_support(&weighted, normal), 10.);
            weighted.swap(0, 1);
            assert_eq!(minimum_nonparallel_support(&weighted, normal), 10.);
        }
    }
    #[test]
    fn three_equal_skins_have_no_unique_pair() {
        let mut comps: Vec<_> = (0..3).map(|i| BTreeSet::from([i])).collect();
        let area = BTreeMap::from([(0, 10.), (1, 10.), (2, 10.)]);
        assert!(!unique_skin_pair(&comps, &area));
        comps.reverse();
        assert!(!unique_skin_pair(&comps, &area));
    }
}
