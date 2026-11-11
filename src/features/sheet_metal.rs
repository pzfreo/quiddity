//! Sheet-metal bodies (`quiddity.sheet_metal`): thin-wall bodies whose two skins are developable
//! (planes and cylinders), read as flanges joined by cylindrical bends, with a neutral-axis flat
//! pattern unfolded from them and checked for overlap.
//!
//! The bend allowance uses an explicit k-factor; the material and its forming history are absent
//! from a STEP solid, so neither is inferred. Python triangulates each flange face with
//! OpenCascade's mesher (`Face.tessellate(0.1)`); the port triangulates the same planar region
//! from its boundary, thinned to the same deflections, so the flat faces' vertices and
//! triangles, and the largest triangle overlap a witness reports, are another triangulation of
//! one region.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

use super::Context;
use super::body::BodyKey;
use super::evidence::{self, EvidenceError, Occurrence};
use super::graph;
use super::planes::unit_plane_normal;
use super::policy::length_tol;
use super::regions::edge_length;
use super::thin_walls::{self, ThinWallBody, WallFacePair};
use crate::kernel::brep::{Arc, Part};
use crate::kernel::cloud::thin;
use crate::kernel::geom::{self, Curve, Surface, V3};
use crate::kernel::py;
use crate::kernel::sampling::edge_interval;

const MIN_SKIN_COVERAGE: f64 = 0.98;
const MIN_CUT_BRIDGE_AREA: f64 = 0.8;
const TESSELLATION_TOLERANCE: f64 = 0.1;

/// Options named as `recognise_sheet_metal_bodies`' keyword arguments.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct SheetMetalOptions {
    /// Where the neutral axis sits through the thickness, from the inner skin (0..=1).
    pub k_factor: f64,
}

impl Default for SheetMetalOptions {
    fn default() -> Self {
        SheetMetalOptions { k_factor: 0.5 }
    }
}

/// A k-factor outside `0..=1` (or not finite), refused as Python raises `ValueError`.
#[derive(Debug, PartialEq)]
pub struct InvalidKFactor(pub f64);

impl std::fmt::Display for InvalidKFactor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "k_factor must be finite and between zero and one (got {})",
            self.0
        )
    }
}

impl std::error::Error for InvalidKFactor {}

/// One planar region on the reference side and its opposite skin patches.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct SheetFlange {
    pub index: usize,
    pub reference_faces: Vec<usize>,
    pub mate_faces: Vec<usize>,
    pub origin: V3,
    pub normal: V3,
    pub area: f64,
}

/// A cylindrical bend pair between two flanges.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct SheetBend {
    pub index: usize,
    pub face_pairs: Vec<WallFacePair>,
    pub first_flange: usize,
    pub second_flange: usize,
    pub axis_origin: V3,
    pub axis_direction: V3,
    pub angle_degrees: f64,
    pub inner_radius: f64,
    pub reference_skin: &'static str,
    pub neutral_radius: f64,
    pub bend_allowance: f64,
}

/// A triangulated planar source face in the neutral flat frame.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct UnfoldedFlangeFace {
    pub source_face: usize,
    pub flange: usize,
    pub vertices: Vec<[f64; 2]>,
    pub triangles: Vec<[usize; 3]>,
}

/// The neutral-axis developed rectangle of one cylindrical bend segment.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct UnfoldedBendStrip {
    pub bend: usize,
    pub source_face_pair: WallFacePair,
    pub corners: [[f64; 2]; 4],
}

/// One triangle overlap proving the development is not a single blank.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct FlatOverlapWitness {
    pub first_kind: &'static str,
    pub first_index: usize,
    pub second_kind: &'static str,
    pub second_index: usize,
    pub area: f64,
}

/// A neutral development of flange faces and bend strips; `valid_blank` false keeps overlap
/// witnesses and is no manufacturing blank claim.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct FlatPatternPlan {
    pub k_factor: f64,
    pub base_flange: usize,
    pub tree_bends: Vec<usize>,
    pub flat_faces: Vec<UnfoldedFlangeFace>,
    pub bend_strips: Vec<UnfoldedBendStrip>,
    pub tessellation_tolerance: f64,
    pub valid_blank: bool,
    pub overlap_witnesses: Vec<FlatOverlapWitness>,
}

/// A local curved region with partial offset-skin evidence.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct FormedSheetFeature {
    pub faces: Vec<usize>,
    pub paired_faces: Vec<usize>,
}

/// One rounded or chamfered contour face, kept by its source face.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct SheetEdgeTreatment {
    pub face: usize,
    pub kind: &'static str,
    pub radius: Option<f64>,
}

/// A dominant developable sheet with its flange and bend geometry.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct SheetMetalBody {
    pub body_index: usize,
    pub body_key: Option<BodyKey>,
    pub thickness: f64,
    pub first_side_faces: Vec<usize>,
    pub second_side_faces: Vec<usize>,
    pub cut_edge_faces: Vec<usize>,
    pub formed_features: Vec<FormedSheetFeature>,
    pub flanges: Vec<SheetFlange>,
    pub bends: Vec<SheetBend>,
    pub flat_pattern: Option<FlatPatternPlan>,
    pub paired_area_fraction: f64,
    pub flat_pattern_status: &'static str,
    pub edge_treatments: Vec<SheetEdgeTreatment>,
}

internal_entry! {
/// `recognise_sheet_metal_bodies`.
pub fn recognise_sheet_metal_bodies(
    part: &Part,
    opts: &SheetMetalOptions,
) -> Result<Vec<SheetMetalBody>, InvalidKFactor> {
    Ok(super::records(discover_with(&Context::new(part), opts)?))
}
}

internal_entry! {
/// The evidence path: each sheet with every face it names, all on one valid solid.
pub fn discover_verified(
    ctx: &Context<'_>,
) -> Result<Vec<Occurrence<SheetMetalBody>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx))
}
}

internal_entry! {
/// Every sheet-metal body at the default k-factor (`_discover`).
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<SheetMetalBody>> {
    discover_with(ctx, &SheetMetalOptions::default()).expect("the default k-factor is valid")
}
}

internal_entry! {
/// Every sheet-metal body among the thin-wall bodies, defined by every face its record names:
/// skins, cut edges, treatments, formed features, flanges and bends.
pub fn discover_with(
    ctx: &Context<'_>,
    opts: &SheetMetalOptions,
) -> Result<Vec<Occurrence<SheetMetalBody>>, InvalidKFactor> {
    let k = opts.k_factor;
    if !k.is_finite() || !(0.0..=1.0).contains(&k) {
        return Err(InvalidKFactor(k));
    }
    Ok(thin_walls::discover(ctx)
        .into_iter()
        .filter_map(|wall| body(ctx.part, &wall.record, k))
        .map(|record| {
            let mut faces: BTreeSet<usize> = BTreeSet::new();
            faces.extend(&record.first_side_faces);
            faces.extend(&record.second_side_faces);
            faces.extend(&record.cut_edge_faces);
            faces.extend(record.edge_treatments.iter().map(|t| t.face));
            faces.extend(record.formed_features.iter().flat_map(|f| &f.faces));
            for flange in &record.flanges {
                faces.extend(&flange.reference_faces);
                faces.extend(&flange.mate_faces);
            }
            for bend in &record.bends {
                faces.extend(
                    bend.face_pairs
                        .iter()
                        .flat_map(|p| [p.first_face, p.second_face]),
                );
            }
            Occurrence {
                record,
                defining: faces.into_iter().collect(),
                context: Vec::new(),
            }
        })
        .collect())
}
}

/// The surface classes the sheet reading distinguishes (`BRepAdaptor_Surface.GetType`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Plane,
    Cylinder,
    Sphere,
    Torus,
    BSpline,
    Other,
}

/// One body's reading: the part, the wall, and every body face's area (a body with a face whose
/// area cannot be integrated is not read: a failed area is not taken as 0).
struct Sheet<'a> {
    part: &'a Part,
    wall: &'a ThinWallBody,
    area: BTreeMap<usize, f64>,
    paired: BTreeSet<usize>,
}

impl Sheet<'_> {
    fn kind(&self, face: usize) -> Kind {
        match self.part.faces[face].surface {
            Surface::Plane { .. } => Kind::Plane,
            Surface::Cylinder { .. } => Kind::Cylinder,
            Surface::Sphere { .. } => Kind::Sphere,
            Surface::Torus { .. } => Kind::Torus,
            Surface::Freeform {
                kind: "BSPLINE", ..
            } => Kind::BSpline,
            _ => Kind::Other,
        }
    }

    fn area_of(&self, faces: impl IntoIterator<Item = usize>) -> f64 {
        py::fsum(faces.into_iter().map(|f| self.area[&f]))
    }

    /// `graph.smooth_region`: the faces reachable from *face* across smooth arcs only.
    fn smooth_region(&self, face: usize) -> BTreeSet<usize> {
        let mut found = BTreeSet::from([face]);
        let mut pending = vec![face];
        while let Some(current) = pending.pop() {
            for n in self.part.neighbours(current) {
                if !found.contains(&n) && self.part.arc(current, n) == Some(Arc::Smooth) {
                    found.insert(n);
                    pending.push(n);
                }
            }
        }
        found
    }

    /// The face's angular span (`LastUParameter - FirstUParameter` on the face's adaptor).
    fn u_span(&self, face: usize) -> Option<f64> {
        self.part.uv_bounds(face).map(|(u0, u1, _, _)| u1 - u0)
    }
}

fn cylinder(part: &Part, face: usize) -> Option<(V3, V3, f64)> {
    match part.faces[face].surface {
        Surface::Cylinder { frame, radius } => Some((frame.origin, frame.z, radius)),
        _ => None,
    }
}

/// `_same_axis`: parallel directions (either sense) through points within *tolerance*.
fn same_axis(first: (V3, V3), second: (V3, V3), tolerance: f64) -> bool {
    let ((o1, d1), (o2, d2)) = (first, second);
    if (py::sum((0..3).map(|i| d1[i] * d2[i])).abs() - 1.0).abs() > 1e-5 {
        return false;
    }
    let along = py::sum((0..3).map(|i| (o2[i] - o1[i]) * d1[i]));
    let projected = [0, 1, 2].map(|i| o1[i] + along * d1[i]);
    py::dist(&projected, &o2) <= tolerance
}

/// How the face's loops use *edge*: `true` start → end (`Edge.is_forward` in the face).
fn forward_in(part: &Part, face: usize, edge: usize) -> bool {
    part.faces[face]
        .loops
        .iter()
        .flat_map(|l| &l.edges)
        .find(|e| e.0 == edge)
        .is_none_or(|e| e.1)
}

/// `edge.position_at(fraction)` for the edge as *face* uses it: the point at that fraction of
/// its length from where the face's boundary enters it (`GCPnts_AbscissaPoint`). Exact on lines
/// and circles; along the edge's samples, projected onto the curve, otherwise.
fn edge_point(part: &Part, face: usize, edge: usize, fraction: f64) -> V3 {
    let e = &part.edges[edge];
    let fraction = if forward_in(part, face, edge) {
        fraction
    } else {
        1.0 - fraction
    };
    match e.curve {
        Curve::Line { .. } => geom::add(e.start, geom::scale(geom::sub(e.end, e.start), fraction)),
        Curve::Circle { .. } => {
            let (t0, t1) = edge_interval(&e.curve, e.start, e.end, e.same_sense, e.is_closed());
            e.curve.value(t0 + fraction * (t1 - t0))
        }
        _ => {
            let lengths: Vec<f64> = e
                .samples
                .windows(2)
                .map(|w| geom::dist(w[0], w[1]))
                .collect();
            let target = fraction * lengths.iter().sum::<f64>();
            let mut run = 0.0;
            for (w, len) in e.samples.windows(2).zip(&lengths) {
                if run + len >= target && *len > 0.0 {
                    let q = geom::add(
                        w[0],
                        geom::scale(geom::sub(w[1], w[0]), (target - run) / len),
                    );
                    return e.curve.value(e.curve.parameter(q));
                }
                run += len;
            }
            e.end
        }
    }
}

/// `edge.distance_to(point)`: the least distance from *p* to the edge, along its samples (which
/// follow the curve within 0.2 µm).
fn edge_distance(part: &Part, edge: usize, p: V3) -> f64 {
    let s = &part.edges[edge].samples;
    if s.len() == 1 {
        return geom::dist(s[0], p);
    }
    s.windows(2)
        .map(|w| {
            let d = geom::sub(w[1], w[0]);
            let len2 = geom::dot(d, d);
            let t = if len2 > 0.0 {
                (geom::dot(geom::sub(p, w[0]), d) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            geom::dist(p, geom::add(w[0], geom::scale(d, t)))
        })
        .fold(f64::INFINITY, f64::min)
}

/// `_sides`: the smooth regions of the largest planar pair's two faces, disjoint and covering
/// the paired skin.
fn sides(s: &Sheet<'_>) -> Option<(BTreeSet<usize>, BTreeSet<usize>)> {
    let plane_pairs: Vec<&WallFacePair> = s
        .wall
        .face_pairs
        .iter()
        .filter(|p| s.kind(p.first_face) == Kind::Plane)
        .collect();
    if plane_pairs.len() < 2 {
        return None;
    }
    let score = |p: &WallFacePair| s.area[&p.first_face] + s.area[&p.second_face];
    let largest = plane_pairs
        .iter()
        .map(|p| score(p))
        .fold(f64::NEG_INFINITY, f64::max);
    let mut readings = plane_pairs
        .iter()
        .filter(|p| py::round_to(score(p), 9) == py::round_to(largest, 9))
        .map(|p| {
            (
                s.smooth_region(p.first_face),
                s.smooth_region(p.second_face),
            )
        });
    let (first, second) = readings.next()?;
    if readings.any(|(a, b)| !((a == first && b == second) || (a == second && b == first))) {
        return None;
    }
    if !first.is_disjoint(&second) {
        return None;
    }
    let covered = s.area_of(
        first
            .union(&second)
            .copied()
            .filter(|f| s.paired.contains(f)),
    );
    let coverage = covered / s.area_of(s.paired.iter().copied());
    (coverage >= MIN_SKIN_COVERAGE).then_some((first, second))
}

/// `_cut_faces`: the remaining body faces that bridge the two skins at the wall thickness (cut
/// edges), and the rest. `None` when a bridging face's edges stand off the other skin at
/// another distance, or the bridges cover too little of the remainder.
fn cut_faces(
    s: &Sheet<'_>,
    first: &BTreeSet<usize>,
    second: &BTreeSet<usize>,
) -> Option<(Vec<usize>, Vec<usize>)> {
    let part = s.part;
    let thickness = s.wall.thickness;
    let body: BTreeSet<usize> = s
        .paired
        .iter()
        .chain(&s.wall.unpaired_faces)
        .copied()
        .collect();
    let remaining: Vec<usize> = body
        .iter()
        .copied()
        .filter(|f| !first.contains(f) && !second.contains(f))
        .collect();
    let mut cut = Vec::new();
    for &index in &remaining {
        let (mut first_edges, mut second_edges) = (Vec::new(), Vec::new());
        for n in part.neighbours(index) {
            if first.contains(&n) {
                first_edges.extend(part.shared_edges(index, n));
            }
            if second.contains(&n) {
                second_edges.extend(part.shared_edges(index, n));
            }
        }
        if first_edges.is_empty() || second_edges.is_empty() {
            continue;
        }
        let tolerance = length_tol(thickness, 0.01);
        for (source, opposite) in [(&first_edges, &second_edges), (&second_edges, &first_edges)] {
            for &edge in source {
                for fraction in [0.2, 0.5, 0.8] {
                    let p = edge_point(part, index, edge, fraction);
                    let distance = opposite
                        .iter()
                        .map(|&o| edge_distance(part, o, p))
                        .fold(f64::INFINITY, f64::min);
                    if (distance - thickness).abs() > tolerance {
                        return None;
                    }
                }
            }
        }
        cut.push(index);
    }
    let total = s.area_of(remaining.iter().copied());
    let bridged = s.area_of(cut.iter().copied());
    if total <= 0.0 || bridged / total < MIN_CUT_BRIDGE_AREA {
        return None;
    }
    let rest = remaining.into_iter().filter(|f| !cut.contains(f)).collect();
    Some((cut, rest))
}

/// `_edge_treatments`: rounded and chamfered cut faces, and small B-spline corner patches among
/// the unresolved faces (which then leave the unresolved set).
fn edge_treatments(
    s: &Sheet<'_>,
    cut: &[usize],
    unresolved: &[usize],
    first: &BTreeSet<usize>,
    second: &BTreeSet<usize>,
) -> Option<(Vec<SheetEdgeTreatment>, Vec<usize>)> {
    let part = s.part;
    let thickness = s.wall.thickness;
    let mut treatments = Vec::new();
    for &index in cut {
        let treatment = |kind, radius| SheetEdgeTreatment {
            face: index,
            kind,
            radius,
        };
        match part.faces[index].surface {
            // A full cylindrical cut wall may be a hole; a partial arc keeps the rounded
            // contour without assigning a hole operation.
            Surface::Cylinder { radius, .. } => {
                if s.u_span(index)? < 2.0 * PI - 1e-4 {
                    treatments.push(treatment("rounded_cut", Some(radius)));
                }
            }
            Surface::Torus { minor, .. } => treatments.push(treatment("rounded_cut", Some(minor))),
            Surface::Plane { .. } => {
                let normal = graph::normal(part, index)?;
                let mut chamfered = false;
                for other in part.neighbours(index) {
                    if first.contains(&other) || second.contains(&other) {
                        let d = geom::dot(normal, graph::normal(part, other)?).abs();
                        chamfered |= 0.1 < d && d < 0.9;
                    }
                }
                if chamfered {
                    treatments.push(treatment("chamfered_cut", None));
                }
            }
            _ => {}
        }
    }
    let mut remaining = Vec::new();
    for &index in unresolved {
        let size = {
            let b = part.face_bounds(index);
            geom::sub(b.max, b.min)
        };
        let neighbours: BTreeSet<usize> = part.neighbours(index).into_iter().collect();
        let kinds: BTreeSet<u8> = neighbours.iter().map(|&n| s.kind(n) as u8).collect();
        let local_corner = s.kind(index) == Kind::BSpline
            && s.area[&index] <= 10.0 * thickness.powi(2)
            && size[0].min(size[1]).min(size[2]) <= thickness
            && neighbours.len() >= 3
            && (neighbours.is_subset(first) || neighbours.is_subset(second))
            && kinds.contains(&(Kind::Plane as u8))
            && kinds.contains(&(Kind::Cylinder as u8));
        if local_corner {
            treatments.push(SheetEdgeTreatment {
                face: index,
                kind: "freeform_corner",
                radius: None,
            });
        } else {
            remaining.push(index);
        }
    }
    treatments.sort_by_key(|t| t.face);
    Some((treatments, remaining))
}

/// `_formed_features`: each connected group of unresolved faces must keep paired skin with a
/// cylinder in it (local forming), else the body is refused (`None`).
fn formed_features(s: &Sheet<'_>, unresolved: &[usize]) -> Option<Vec<FormedSheetFeature>> {
    let mut remaining: BTreeSet<usize> = unresolved.iter().copied().collect();
    let mut mates: BTreeMap<usize, usize> = BTreeMap::new();
    for p in &s.wall.face_pairs {
        mates.insert(p.first_face, p.second_face);
        mates.insert(p.second_face, p.first_face);
    }
    let mut result = Vec::new();
    while let Some(seed) = remaining.pop_first() {
        let mut component = BTreeSet::from([seed]);
        let mut pending = vec![seed];
        while let Some(current) = pending.pop() {
            for n in s.part.neighbours(current) {
                if remaining.remove(&n) {
                    component.insert(n);
                    pending.push(n);
                }
            }
        }
        let supported: BTreeSet<usize> = component
            .iter()
            .copied()
            .filter(|f| s.paired.contains(f))
            .collect();
        if !supported.iter().any(|&f| s.kind(f) == Kind::Cylinder) {
            return None;
        }
        let faces: BTreeSet<usize> = component
            .iter()
            .copied()
            .chain(supported.iter().map(|f| mates[f]))
            .collect();
        result.push(FormedSheetFeature {
            faces: faces.into_iter().collect(),
            paired_faces: supported.into_iter().collect(),
        });
    }
    Some(result)
}

/// `_flanges`: planar pairs grouped by reference face (on the first side where the pair
/// straddles it), coplanar references sharing a bend axis joined into one flange.
fn flanges(
    s: &Sheet<'_>,
    first: &BTreeSet<usize>,
    second: &BTreeSet<usize>,
    excluded: &BTreeSet<usize>,
) -> Option<(Vec<SheetFlange>, BTreeMap<usize, usize>)> {
    let part = s.part;
    let thickness = s.wall.thickness;
    let mut mates: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for p in &s.wall.face_pairs {
        let (a, b) = (p.first_face, p.second_face);
        if excluded.contains(&a) || excluded.contains(&b) || s.kind(a) != Kind::Plane {
            continue;
        }
        let (reference, mate) = if first.contains(&a) && !first.contains(&b) {
            (a, b)
        } else if (first.contains(&b) && !first.contains(&a))
            || (second.contains(&a) && !second.contains(&b))
        {
            (b, a)
        } else if second.contains(&b) && !second.contains(&a) {
            (a, b)
        } else {
            return None;
        };
        let set = mates.entry(reference).or_default();
        if !set.contains(&mate) {
            set.push(mate);
        }
    }
    let axes: BTreeMap<usize, Vec<(V3, V3)>> = mates
        .keys()
        .map(|&r| {
            let found = part
                .neighbours(r)
                .into_iter()
                .filter_map(|n| cylinder(part, n).map(|(o, d, _)| (o, d)))
                .collect();
            (r, found)
        })
        .collect();
    let tolerance = length_tol(thickness, 0.001);
    let mut out: Vec<SheetFlange> = Vec::new();
    let mut face_to_flange = BTreeMap::new();
    for (&reference, mate_faces) in &mates {
        let Surface::Plane { frame } = part.faces[reference].surface else {
            unreachable!("flange references are planes");
        };
        let origin = frame.origin;
        let normal = graph::normal(part, reference)?;
        let group = out.iter().position(|flange| {
            py::dist(&normal, &flange.normal) < 1e-5
                && py::sum((0..3).map(|i| normal[i] * (origin[i] - flange.origin[i]))).abs()
                    <= tolerance
                && flange.reference_faces.iter().any(|existing| {
                    axes[&reference].iter().any(|&left| {
                        axes[existing]
                            .iter()
                            .any(|&right| same_axis(left, right, tolerance))
                    })
                })
        });
        let area = s.area[&reference];
        let group = match group {
            Some(g) => {
                let prior = &mut out[g];
                prior.reference_faces.push(reference);
                prior.mate_faces.extend(mate_faces);
                prior.mate_faces.sort_unstable();
                prior.area += area;
                g
            }
            None => {
                let mut sorted = mate_faces.clone();
                sorted.sort_unstable();
                out.push(SheetFlange {
                    index: out.len(),
                    reference_faces: vec![reference],
                    mate_faces: sorted,
                    origin,
                    normal,
                    area,
                });
                out.len() - 1
            }
        };
        face_to_flange.insert(reference, group);
    }
    Some((out, face_to_flange))
}

/// `_bends`: cylindrical pairs, one wall thickness apart in radius, each between exactly two
/// flanges; coaxial pairs of one bend (same flanges, angle, radius and side) joined.
fn bends(
    s: &Sheet<'_>,
    first: &BTreeSet<usize>,
    face_to_flange: &BTreeMap<usize, usize>,
    k_factor: f64,
    excluded: &BTreeSet<usize>,
) -> Option<Vec<SheetBend>> {
    let part = s.part;
    let thickness = s.wall.thickness;
    let tolerance = length_tol(thickness, 0.001);
    let mut result: Vec<SheetBend> = Vec::new();
    for pair in &s.wall.face_pairs {
        let (a, b) = (pair.first_face, pair.second_face);
        if excluded.contains(&a) || excluded.contains(&b) {
            continue;
        }
        let Some((origin, direction, radius_a)) = cylinder(part, a) else {
            continue;
        };
        let (_, _, radius_b) = cylinder(part, b)?;
        if ((radius_a - radius_b).abs() - thickness).abs() > tolerance {
            return None;
        }
        let reference = if first.contains(&a) { a } else { b };
        if !first.contains(&reference) {
            return None;
        }
        // Relief cuts can trim the opposite skin to a shorter angular span; the continuous
        // reference-side patch keeps the bend's full sweep.
        let angle = s.u_span(reference)?.abs();
        if !(0.0 < angle && angle < 2.0 * PI - 1e-3) {
            return None;
        }
        let adjacent: BTreeSet<usize> = part
            .neighbours(reference)
            .iter()
            .filter_map(|n| face_to_flange.get(n).copied())
            .collect();
        let &[first_flange, second_flange] =
            adjacent.iter().copied().collect::<Vec<_>>().as_slice()
        else {
            return None;
        };
        let inner = radius_a.min(radius_b);
        let neutral_radius = inner + k_factor * thickness;
        let reference_radius = if reference == a { radius_a } else { radius_b };
        let reference_skin = if (reference_radius - inner).abs() <= tolerance {
            "inner"
        } else {
            "outer"
        };
        let degrees = angle.to_degrees();
        let found = result.iter().position(|bend| {
            (bend.first_flange, bend.second_flange) == (first_flange, second_flange)
                && (bend.angle_degrees - degrees).abs() < 0.01
                && (bend.inner_radius - inner).abs() <= tolerance
                && bend.reference_skin == reference_skin
                && same_axis(
                    (origin, direction),
                    (bend.axis_origin, bend.axis_direction),
                    tolerance,
                )
        });
        if let Some(i) = found {
            result[i].face_pairs.push(pair.clone());
            continue;
        }
        result.push(SheetBend {
            index: result.len(),
            face_pairs: vec![pair.clone()],
            first_flange,
            second_flange,
            axis_origin: origin,
            axis_direction: direction,
            angle_degrees: degrees,
            inner_radius: inner,
            reference_skin,
            neutral_radius,
            bend_allowance: neutral_radius * angle,
        });
    }
    (!result.is_empty()).then_some(result)
}

type Matrix = [V3; 3];
const IDENTITY: Matrix = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn mat_vec(m: &Matrix, v: V3) -> V3 {
    [py::dot(&m[0], &v), py::dot(&m[1], &v), py::dot(&m[2], &v)]
}

fn mat_mul(l: &Matrix, r: &Matrix) -> Matrix {
    [0, 1, 2].map(|i| [0, 1, 2].map(|j| py::sum((0..3).map(|k| l[i][k] * r[k][j]))))
}

/// `_rotation`: the rotation by *angle* about the unit *axis* (Rodrigues).
fn rotation(axis: V3, angle: f64) -> Matrix {
    let [x, y, z] = axis;
    let (c, s) = (angle.cos(), angle.sin());
    let t = 1.0 - c;
    [
        [t * x * x + c, t * x * y - s * z, t * x * z + s * y],
        [t * x * y + s * z, t * y * y + c, t * y * z - s * x],
        [t * x * z - s * y, t * y * z + s * x, t * z * z + c],
    ]
}

type V2 = [f64; 2];

fn cross2(a: V2, b: V2) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn area2(polygon: &[V2]) -> f64 {
    0.5 * py::sum((0..polygon.len()).map(|i| cross2(polygon[i], polygon[(i + 1) % polygon.len()])))
}

/// `_overlap_area`: the area two triangles share (one clipped by the other); shared boundaries
/// overlap by zero.
fn overlap_area(first: &[V2; 3], second: &[V2; 3]) -> f64 {
    let mut subject: Vec<V2> = first.to_vec();
    let mut clip: Vec<V2> = second.to_vec();
    if area2(&clip) < 0.0 {
        clip.reverse();
    }
    for i in 0..3 {
        let (start, end) = (clip[i], clip[(i + 1) % 3]);
        let edge = [end[0] - start[0], end[1] - start[1]];
        let Some(&last) = subject.last() else {
            return 0.0;
        };
        let side_of = |p: V2| cross2(edge, [p[0] - start[0], p[1] - start[1]]);
        let mut output = Vec::new();
        let (mut previous, mut previous_side) = (last, side_of(last));
        for &point in &subject {
            let side = side_of(point);
            if (side >= 0.0) != (previous_side >= 0.0) {
                let fraction = previous_side / (previous_side - side);
                output.push([
                    previous[0] + fraction * (point[0] - previous[0]),
                    previous[1] + fraction * (point[1] - previous[1]),
                ]);
            }
            if side >= 0.0 {
                output.push(point);
            }
            (previous, previous_side) = (point, side);
        }
        subject = output;
    }
    if subject.len() >= 3 {
        area2(&subject).abs()
    } else {
        0.0
    }
}

/// `_joint_edges`: for the first of the bend's pairs whose reference face meets both flanges,
/// that face and its longest shared edge with each flange.
fn joint_edges(
    s: &Sheet<'_>,
    pairs: &[WallFacePair],
    parent: &SheetFlange,
    child: &SheetFlange,
    first: &BTreeSet<usize>,
) -> Option<(usize, usize, usize)> {
    let part = s.part;
    let longest = |edges: Vec<usize>| -> Option<usize> {
        let lengths: Vec<f64> = edges.iter().map(|&e| edge_length(part, e)).collect();
        let best = py::first_max(&lengths, |l| *l);
        for (i, &length) in lengths.iter().enumerate() {
            if (length - lengths[best]).abs() <= geom::COORD_FLOOR
                && !same_joint_line(part, edges[best], edges[i])
            {
                return None;
            }
        }
        Some(edges[best])
    };
    let mut found = None;
    for pair in pairs {
        let reference = if first.contains(&pair.first_face) {
            pair.first_face
        } else {
            pair.second_face
        };
        let shared = |flange: &SheetFlange| -> Vec<usize> {
            flange
                .reference_faces
                .iter()
                .flat_map(|&f| part.shared_edges(reference, f))
                .collect()
        };
        let (parent_edges, child_edges) = (shared(parent), shared(child));
        if !parent_edges.is_empty() && !child_edges.is_empty() {
            let candidate = (reference, longest(parent_edges)?, longest(child_edges)?);
            if let Some((_, a, b)) = found {
                if !same_joint_line(part, a, candidate.1) || !same_joint_line(part, b, candidate.2)
                {
                    return None;
                }
            } else {
                found = Some(candidate);
            }
        }
    }
    found
}

fn same_joint_line(part: &Part, a: usize, b: usize) -> bool {
    let (a, b) = (&part.edges[a], &part.edges[b]);
    joint_lines_agree(a.start, a.end, b.start, b.end)
}
fn joint_lines_agree(a: V3, b: V3, c: V3, d: V3) -> bool {
    let Some(axis) = py::unit(geom::sub(b, a)) else {
        return false;
    };
    [c, d]
        .iter()
        .all(|&p| geom::norm(geom::cross(geom::sub(p, a), axis)) <= geom::COORD_FLOOR)
}

/// `_flat_pattern_plan`: the flanges unfolded about their bends into the base (largest)
/// flange's plane, as a tree from it; each flange face triangulated and each bend pair's strip
/// laid out, overlaps between them witnessed. `None` when the bends do not form a tree over
/// the flanges, a joint cannot be read, an unfolded point leaves the base plane, or the port's
/// own triangulation of a flange face fails (`triangulate_plane`). That last is not one of
/// Python's outcomes (OpenCascade's mesher triangulates the face), yet `body` treats it as it
/// does the others: the sheet's status becomes `not_proven` (the bend count already matched),
/// or, with no edge treatment and no formed-feature fallback that proves a blank, the body is
/// not read. No corpus or captured part reaches it. Refusing it instead needs a refusal
/// `recognise` can carry: the open panic-versus-carried-refusal question.
fn flat_pattern_plan(
    s: &Sheet<'_>,
    flanges: &[SheetFlange],
    bends: &[SheetBend],
    k_factor: f64,
    first: &BTreeSet<usize>,
) -> Option<FlatPatternPlan> {
    let part = s.part;
    let thickness = s.wall.thickness;
    if flanges.is_empty() || bends.len() != flanges.len() - 1 {
        return None;
    }
    let base = flanges[py::first_max(flanges, |f| f.area)].index;
    let root = &flanges[base];
    let root_normal = root.normal;
    let horizontal = if root_normal[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let x_axis = py::unit(geom::cross(horizontal, root_normal))?;
    let y_axis = geom::cross(root_normal, x_axis);
    let mut placement: BTreeMap<usize, (Matrix, V3)> =
        BTreeMap::from([(base, (IDENTITY, [0.0; 3]))]);
    let mut tree = Vec::new();
    let mut pending = VecDeque::from([base]);
    // The face's centre and the approach from a joint point towards it, across the parent plane.
    let approach_from = |reference: usize, point: V3, parent: &SheetFlange| -> Option<V3> {
        let centre = part.face_centre(reference)?;
        let approach = geom::sub(centre, point);
        let approach = geom::sub(
            approach,
            geom::scale(parent.normal, py::dot(&approach, &parent.normal)),
        );
        py::unit(approach)
    };
    while let Some(current) = pending.pop_front() {
        for bend in bends {
            let other = if bend.first_flange == current {
                bend.second_flange
            } else if bend.second_flange == current {
                bend.first_flange
            } else {
                continue;
            };
            if placement.contains_key(&other) {
                continue;
            }
            let (parent, child) = (&flanges[current], &flanges[other]);
            let (reference, parent_edge, child_edge) =
                joint_edges(s, &bend.face_pairs, parent, child, first)?;
            let parent_point = edge_point(part, reference, parent_edge, 0.5);
            let child_point = edge_point(part, reference, child_edge, 0.5);
            let unit_approach = approach_from(reference, parent_point, parent)?;
            let axis = bend.axis_direction;
            let angle = py::dot(&axis, &geom::cross(child.normal, parent.normal))
                .atan2(py::dot(&child.normal, &parent.normal));
            let turn = rotation(axis, angle);
            let target = geom::add(
                parent_point,
                geom::scale(unit_approach, bend.bend_allowance),
            );
            let shift = geom::sub(target, mat_vec(&turn, child_point));
            let (parent_matrix, parent_shift) = placement[&current];
            placement.insert(
                other,
                (
                    mat_mul(&parent_matrix, &turn),
                    geom::add(mat_vec(&parent_matrix, shift), parent_shift),
                ),
            );
            tree.push(bend.index);
            pending.push_back(other);
        }
    }
    if placement.len() != flanges.len() {
        return None;
    }

    let mut deviations: Vec<f64> = Vec::new();
    let mut flat = |point: V3, flange: usize| -> V2 {
        let (matrix, shift) = placement[&flange];
        let placed = geom::sub(geom::add(mat_vec(&matrix, point), shift), root.origin);
        deviations.push(py::dot(&placed, &root_normal).abs());
        [py::dot(&placed, &x_axis), py::dot(&placed, &y_axis)]
    };
    let mut flat_faces = Vec::new();
    // Each triangle with its source: a flange face (its index), or a bend strip (-1 - strip).
    let mut triangles: Vec<(i64, [V2; 3])> = Vec::new();
    for flange in flanges {
        for &face in &flange.reference_faces {
            let (vertices, cells) = triangulate_plane(part, face, TESSELLATION_TOLERANCE)?;
            let projected: Vec<V2> = vertices.iter().map(|&v| flat(v, flange.index)).collect();
            triangles.extend(cells.iter().map(|c| (face as i64, c.map(|i| projected[i]))));
            flat_faces.push(UnfoldedFlangeFace {
                source_face: face,
                flange: flange.index,
                vertices: projected,
                triangles: cells,
            });
        }
    }
    let mut strips: Vec<UnfoldedBendStrip> = Vec::new();
    for bend in bends {
        let parent = &flanges[bend.first_flange];
        let child = &flanges[bend.second_flange];
        for pair in &bend.face_pairs {
            let (reference, edge, _) =
                joint_edges(s, std::slice::from_ref(pair), parent, child, first)?;
            let p0 = edge_point(part, reference, edge, 0.0);
            let p1 = edge_point(part, reference, edge, 1.0);
            let unit_approach =
                approach_from(reference, edge_point(part, reference, edge, 0.5), parent)?;
            let q0 = geom::add(p0, geom::scale(unit_approach, bend.bend_allowance));
            let q1 = geom::add(p1, geom::scale(unit_approach, bend.bend_allowance));
            let corners = [p0, p1, q1, q0].map(|p| flat(p, parent.index));
            strips.push(UnfoldedBendStrip {
                bend: bend.index,
                source_face_pair: pair.clone(),
                corners,
            });
            let id = -(strips.len() as i64);
            triangles.push((id, [corners[0], corners[1], corners[2]]));
            triangles.push((id, [corners[0], corners[2], corners[3]]));
        }
    }
    let worst = deviations.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if deviations.is_empty() || worst > length_tol(thickness, 0.001) {
        return None;
    }
    let mut overlaps: BTreeMap<(i64, i64), f64> = BTreeMap::new();
    let range = |t: &[V2; 3], k: usize| {
        let lo = t[0][k].min(t[1][k]).min(t[2][k]);
        let hi = t[0][k].max(t[1][k]).max(t[2][k]);
        (lo, hi)
    };
    for (i, (first_id, first_triangle)) in triangles.iter().enumerate() {
        let ((fx0, fx1), (fy0, fy1)) = (range(first_triangle, 0), range(first_triangle, 1));
        for (second_id, second_triangle) in &triangles[i + 1..] {
            if first_id == second_id {
                continue;
            }
            let ((sx0, sx1), (sy0, sy1)) = (range(second_triangle, 0), range(second_triangle, 1));
            if fx1 <= sx0 + 1e-6 || sx1 <= fx0 + 1e-6 || fy1 <= sy0 + 1e-6 || sy1 <= fy0 + 1e-6 {
                continue;
            }
            let area = overlap_area(first_triangle, second_triangle);
            if area > 0.01 * thickness.powi(2) {
                let key = (*first_id.min(second_id), *first_id.max(second_id));
                let entry = overlaps.entry(key).or_insert(0.0);
                *entry = entry.max(area);
            }
        }
    }
    let source = |id: i64| -> (&'static str, usize) {
        if id >= 0 {
            ("flange_face", id as usize)
        } else {
            ("bend_strip", (-id - 1) as usize)
        }
    };
    let witnesses: Vec<FlatOverlapWitness> = overlaps
        .into_iter()
        .map(|((a, b), area)| {
            let ((first_kind, first_index), (second_kind, second_index)) = (source(a), source(b));
            FlatOverlapWitness {
                first_kind,
                first_index,
                second_kind,
                second_index,
                area,
            }
        })
        .collect();
    Some(FlatPatternPlan {
        k_factor,
        base_flange: base,
        tree_bends: tree,
        flat_faces,
        bend_strips: strips,
        tessellation_tolerance: TESSELLATION_TOLERANCE,
        valid_blank: witnesses.is_empty(),
        overlap_witnesses: witnesses,
    })
}

/// A planar face's triangulation (`Face.tessellate`): its boundary loops, each edge thinned to
/// the linear *deflection* and 0.1 rad, holes bridged into the outer loop and the result
/// ear-clipped. Triangles turn anticlockwise about the face's outward normal. `None` for a face
/// that is not a plane or whose boundary does not close into a polygon.
fn triangulate_plane(
    part: &Part,
    face: usize,
    deflection: f64,
) -> Option<(Vec<V3>, Vec<[usize; 3]>)> {
    let f = &part.faces[face];
    let Surface::Plane { frame } = f.surface else {
        return None;
    };
    let normal = unit_plane_normal(part, face)?;
    let x = frame.x;
    let y = geom::cross(normal, x);
    let mut vertices: Vec<V3> = Vec::new();
    let mut loops: Vec<Vec<usize>> = Vec::new();
    for lp in &f.loops {
        let mut ring = Vec::new();
        for &(e, forward) in &lp.edges {
            let mut points = Vec::new();
            thin(&part.edges[e].samples, deflection, &mut points);
            if !forward {
                points.reverse();
            }
            // Each edge's last point is the next one's first.
            points.pop();
            for p in points {
                ring.push(vertices.len());
                vertices.push(p);
            }
        }
        if ring.len() >= 3 {
            loops.push(ring);
        }
    }
    let flat: Vec<V2> = vertices
        .iter()
        .map(|&p| {
            let d = geom::sub(p, frame.origin);
            [geom::dot(d, x), geom::dot(d, y)]
        })
        .collect();
    let signed = |ring: &[usize]| area2(&ring.iter().map(|&i| flat[i]).collect::<Vec<_>>());
    loops.sort_by(|a, b| py::order(signed(b).abs(), signed(a).abs()));
    let mut loops = loops.into_iter();
    let mut polygon = loops.next()?;
    if signed(&polygon) < 0.0 {
        polygon.reverse();
    }
    let mut holes: Vec<Vec<usize>> = loops
        .map(|mut h| {
            if signed(&h) > 0.0 {
                h.reverse();
            }
            h
        })
        .collect();
    // Holes from the rightmost in, each bridged from its rightmost vertex to the nearest
    // polygon vertex the bridge reaches without crossing any boundary.
    holes.sort_by(|a, b| {
        let right = |h: &Vec<usize>| h.iter().map(|&i| flat[i][0]).fold(f64::MIN, f64::max);
        py::order(right(b), right(a))
    });
    for k in 0..holes.len() {
        let hole = &holes[k];
        let start = (0..hole.len())
            .max_by(|&a, &b| py::order(flat[hole[a]][0], flat[hole[b]][0]))
            .unwrap_or(0);
        let m = hole[start];
        let mut candidates: Vec<usize> = (0..polygon.len()).collect();
        candidates.sort_by(|&a, &b| {
            py::order(
                dist2(flat[polygon[a]], flat[m]),
                dist2(flat[polygon[b]], flat[m]),
            )
        });
        let crosses = |p: usize, ring: &[usize]| {
            (0..ring.len()).any(|i| {
                let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
                a != p
                    && b != p
                    && a != m
                    && b != m
                    && segments_cross(flat[p], flat[m], flat[a], flat[b])
            })
        };
        let at = candidates.into_iter().find(|&c| {
            let p = polygon[c];
            !crosses(p, &polygon) && !holes[k..].iter().any(|h| crosses(p, h))
        })?;
        let mut bridged: Vec<usize> = polygon[..=at].to_vec();
        bridged.extend((0..=hole.len()).map(|i| hole[(start + i) % hole.len()]));
        bridged.extend(&polygon[at..]);
        polygon = bridged;
    }
    let triangles = ear_clip(&flat, polygon)?;
    Some((vertices, triangles))
}

fn dist2(a: V2, b: V2) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

fn orient(a: V2, b: V2, c: V2) -> f64 {
    cross2([b[0] - a[0], b[1] - a[1]], [c[0] - a[0], c[1] - a[1]])
}

/// Whether segments pq and ab cross at a point interior to both.
fn segments_cross(p: V2, q: V2, a: V2, b: V2) -> bool {
    let (d1, d2) = (orient(p, q, a), orient(p, q, b));
    let (d3, d4) = (orient(a, b, p), orient(a, b, q));
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

/// Ear clipping of an anticlockwise polygon (vertex indices; a bridge repeats two of them). An
/// ear is a convex corner whose triangle holds no other polygon vertex; when none is left (round-
/// off on nearly straight runs) the flattest convex or straight corner goes. `None` when every
/// remaining corner is reflex: there is no further fallback.
fn ear_clip(flat: &[V2], mut polygon: Vec<usize>) -> Option<Vec<[usize; 3]>> {
    let mut out = Vec::new();
    while polygon.len() > 3 {
        let n = polygon.len();
        let corner = |i: usize| (polygon[(i + n - 1) % n], polygon[i], polygon[(i + 1) % n]);
        let is_ear = |i: usize| {
            let (a, b, c) = corner(i);
            if orient(flat[a], flat[b], flat[c]) <= 0.0 {
                return false;
            }
            polygon.iter().all(|&p| {
                p == a
                    || p == b
                    || p == c
                    || !(orient(flat[a], flat[b], flat[p]) >= 0.0
                        && orient(flat[b], flat[c], flat[p]) >= 0.0
                        && orient(flat[c], flat[a], flat[p]) >= 0.0)
            })
        };
        let pick = (0..n).find(|&i| is_ear(i)).or_else(|| {
            (0..n)
                .filter(|&i| {
                    let (a, b, c) = corner(i);
                    orient(flat[a], flat[b], flat[c]) >= 0.0
                })
                .min_by(|&i, &j| {
                    let area = |k: usize| {
                        let (a, b, c) = corner(k);
                        orient(flat[a], flat[b], flat[c])
                    };
                    py::order(area(i), area(j))
                })
        })?;
        let (a, b, c) = corner(pick);
        out.push([a, b, c]);
        polygon.remove(pick);
    }
    if polygon.len() == 3 {
        out.push([polygon[0], polygon[1], polygon[2]]);
    }
    Some(out)
}

/// `_body`: the wall read as a sheet, or `None` when it is not one.
fn body(part: &Part, wall: &ThinWallBody, k_factor: f64) -> Option<SheetMetalBody> {
    let paired: BTreeSet<usize> = wall
        .face_pairs
        .iter()
        .flat_map(|p| [p.first_face, p.second_face])
        .collect();
    let area: BTreeMap<usize, f64> = paired
        .iter()
        .chain(&wall.unpaired_faces)
        .map(|&f| part.face_mass(f).map(|m| (f, m[0])))
        .collect::<Option<_>>()?;
    let s = Sheet {
        part,
        wall,
        area,
        paired,
    };
    if !s.paired.iter().any(|&f| s.kind(f) == Kind::Cylinder) {
        return None;
    }
    let nondevelopable: BTreeSet<usize> = s
        .paired
        .iter()
        .copied()
        .filter(|&f| !matches!(s.kind(f), Kind::Plane | Kind::Cylinder))
        .collect();
    let paired_area = s.area_of(s.paired.iter().copied());
    if nondevelopable
        .iter()
        .any(|&f| !matches!(s.kind(f), Kind::Sphere | Kind::BSpline))
        || s.area_of(nondevelopable.iter().copied()) > 0.005 * paired_area
    {
        return None;
    }
    let (first, second) = sides(&s)?;
    let (cut, unresolved) = cut_faces(&s, &first, &second)?;
    if cut
        .iter()
        .any(|&f| !matches!(s.kind(f), Kind::Plane | Kind::Cylinder | Kind::Torus))
    {
        return None;
    }
    let (treatments, unresolved) = edge_treatments(&s, &cut, &unresolved, &first, &second)?;
    let mut by_face: BTreeMap<usize, SheetEdgeTreatment> =
        treatments.into_iter().map(|t| (t.face, t)).collect();
    for &f in &nondevelopable {
        by_face
            .entry(f)
            .or_insert_with(|| match part.faces[f].surface {
                Surface::Sphere { radius, .. } => SheetEdgeTreatment {
                    face: f,
                    kind: "rounded_corner",
                    radius: Some(radius),
                },
                _ => SheetEdgeTreatment {
                    face: f,
                    kind: "freeform_corner",
                    radius: None,
                },
            });
    }
    let edge_treatments: Vec<SheetEdgeTreatment> = by_face.into_values().collect();
    if unresolved
        .iter()
        .any(|&f| !matches!(s.kind(f), Kind::Plane | Kind::Cylinder))
    {
        return None;
    }
    let formed_candidates = formed_features(&s, &unresolved)?;

    type Built = (Vec<SheetFlange>, Vec<SheetBend>, Option<FlatPatternPlan>);
    let build = |excluded: &BTreeSet<usize>| -> Option<Built> {
        let (flanges, face_to_flange) = flanges(&s, &first, &second, excluded)?;
        let bends = bends(&s, &first, &face_to_flange, k_factor, excluded)?;
        let plan = flat_pattern_plan(&s, &flanges, &bends, k_factor, &first);
        Some((flanges, bends, plan))
    };
    let full = build(&BTreeSet::new());
    let accepted = full.as_ref().is_some_and(|(_, _, plan)| match plan {
        Some(plan) => plan.valid_blank || !formed_candidates.is_empty(),
        None => !edge_treatments.is_empty(),
    });
    let (formed, excluded, (flanges, bends, plan)) = if accepted {
        (Vec::new(), BTreeSet::new(), full?)
    } else {
        let excluded: BTreeSet<usize> = formed_candidates
            .iter()
            .flat_map(|f| f.faces.iter().copied())
            .collect();
        let fallback = build(&excluded)?;
        if !fallback.2.as_ref().is_some_and(|p| p.valid_blank) {
            return None;
        }
        (formed_candidates, excluded, fallback)
    };
    if plan.is_none() && edge_treatments.is_empty() {
        return None;
    }
    let status = match &plan {
        Some(p) if p.valid_blank => "checked",
        Some(_) => "overlap",
        None if bends.len() + 1 != flanges.len() => "non_tree",
        None => "not_proven",
    };
    let side = |faces: &BTreeSet<usize>| -> Vec<usize> {
        faces
            .iter()
            .copied()
            .filter(|f| !excluded.contains(f))
            .collect()
    };
    Some(SheetMetalBody {
        body_index: wall.body_index,
        body_key: wall.body_key.clone(),
        thickness: wall.thickness,
        first_side_faces: side(&first),
        second_side_faces: side(&second),
        cut_edge_faces: cut,
        formed_features: formed,
        flanges,
        bends,
        flat_pattern: plan,
        paired_area_fraction: wall.paired_area_fraction,
        flat_pattern_status: status,
        edge_treatments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_k_factor_outside_zero_to_one_is_refused() {
        let part = Part::new(Vec::new(), Vec::new(), Vec::new());
        for k in [-0.1, 1.1, f64::NAN] {
            let opts = SheetMetalOptions { k_factor: k };
            assert!(recognise_sheet_metal_bodies(&part, &opts).is_err());
        }
        let opts = SheetMetalOptions { k_factor: 1.0 };
        assert_eq!(recognise_sheet_metal_bodies(&part, &opts), Ok(Vec::new()));
    }

    /// The flat faces are the port's own triangulation, which Python cannot check: each must
    /// cover its source face's area (sm-hanger, whose faces have holes and curved boundaries).
    #[test]
    fn flat_faces_cover_their_source_faces() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/captured/876837d9cf19ff05.step.gz");
        let part = crate::kernel::step::read_step_file(&path).unwrap();
        let sheets = recognise_sheet_metal_bodies(&part, &SheetMetalOptions::default()).unwrap();
        let plan = sheets[0].flat_pattern.as_ref().unwrap();
        assert_eq!(plan.flat_faces.len(), 11);
        for face in &plan.flat_faces {
            let v = &face.vertices;
            let area: f64 = face
                .triangles
                .iter()
                .map(|t| orient(v[t[0]], v[t[1]], v[t[2]]) / 2.0)
                .sum();
            assert!(
                face.triangles
                    .iter()
                    .all(|t| orient(v[t[0]], v[t[1]], v[t[2]]) >= 0.0)
            );
            let exact = part.face_mass(face.source_face).unwrap()[0];
            assert!(
                (area - exact).abs() <= 5e-4 * exact,
                "face {}: triangles {area}, face {exact}",
                face.source_face
            );
        }
    }

    #[test]
    fn overlap_of_a_triangle_with_itself_is_its_area_and_a_shared_edge_is_zero() {
        let t = [[0.0, 0.0], [2.0, 0.0], [0.0, 2.0]];
        assert!((overlap_area(&t, &t) - 2.0).abs() < 1e-12);
        let u = [[2.0, 0.0], [0.0, 2.0], [2.0, 2.0]];
        assert!(overlap_area(&t, &u).abs() < 1e-12);
    }

    #[test]
    fn ear_clipping_covers_a_concave_polygon() {
        // An L shape, area 3.
        let flat = [
            [0.0, 0.0],
            [2.0, 0.0],
            [2.0, 1.0],
            [1.0, 1.0],
            [1.0, 2.0],
            [0.0, 2.0],
        ];
        let cells = ear_clip(&flat, (0..6).collect()).unwrap();
        assert_eq!(cells.len(), 4);
        let area: f64 = cells
            .iter()
            .map(|c| orient(flat[c[0]], flat[c[1]], flat[c[2]]) / 2.0)
            .sum();
        assert!((area - 3.0).abs() < 1e-12);
        assert!(
            cells
                .iter()
                .all(|c| orient(flat[c[0]], flat[c[1]], flat[c[2]]) > 0.0)
        );
    }
}

#[cfg(test)]
mod hinge_tie_tests {
    use super::*;
    #[test]
    fn equal_hinges_require_the_same_geometric_line_after_motion() {
        for angle in [0_f64, 30., 90.] {
            let (s, c) = angle.to_radians().sin_cos();
            let moved = |p: V3| {
                [
                    c * p[0] - s * p[1] + 123.456,
                    s * p[0] + c * p[1] - 78.9,
                    p[2] + 41.3,
                ]
            };
            let [a, b, c, d] = [[0., 0., 0.], [2., 0., 0.], [2., 0., 0.], [4., 0., 0.]].map(moved);
            assert!(joint_lines_agree(a, b, c, d));
            let [c, d] = [[0., 1., 0.], [2., 1., 0.]].map(moved);
            assert!(!joint_lines_agree(a, b, c, d));
            assert!(!joint_lines_agree(c, d, a, b));
        }
    }
}
