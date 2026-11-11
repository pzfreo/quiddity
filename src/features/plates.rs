//! Thin slabs of multi-plate prismatic parts (`quiddity.plates.recognise_plates`): the plate
//! and wall thicknesses of an L-, T- or U-bracket and kin.
//!
//! A plate along an axis is solid material between an outward −axis planar face group at the
//! low coordinate and an outward +axis group at the high one, adjacent along the axis (a pairing
//! that skips an intervening group would span an air gap). Each group must cover a fraction of
//! the body's cross-section, measured as the smallest envelope over the directions the body's
//! own supporting planes establish (so it does not change as the part rolls about the plate
//! normal), and the slab must be thin against the body's extent on that axis.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::Context;
use super::body::BodyKey;
use super::evidence::{self, EvidenceError, Occurrence};
use super::placed::{Grid, Placed};
use super::planes::unit_plane_normal;
use super::policy::{self, AXIS_ALIGNED_COS};
use super::turned::axis_letter;
use crate::kernel::brep::Part;
use crate::kernel::geom::{self, Bounds, Surface, V3};
use crate::kernel::py;

/// The minimum slab thickness and coordinate-clustering tolerance: a minimum-evidence
/// threshold, absolute by design (ADR 0008).
const TOL: f64 = 0.5;
/// Directions agreeing to nine decimal places of a degree are one direction.
const ORIENTED_ANGLE_DIGITS: usize = 9;

/// A recognised thin slab. `axis` is the thickness axis, `lo`/`hi` its bounding coordinates
/// along it, and `u`/`v` the area-weighted centre of its two supporting skins (including
/// projected convex edge treatments) on the other two axes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Plate {
    pub axis: char,
    pub lo: Placed<f64>,
    pub hi: Placed<f64>,
    pub u: f64,
    pub v: f64,
    pub body_key: Option<BodyKey>,
}

impl Plate {
    pub fn thickness(&self) -> f64 {
        *self.hi - *self.lo
    }
}

/// Options named as `recognise_plates`' keyword arguments.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct PlateOptions {
    /// Each face group covers at least this fraction of the body's cross-section.
    pub min_area_frac: f64,
    /// The slab is thinner than this fraction of the body's extent on its axis.
    pub max_thick_frac: f64,
    /// Minimum thickness and clustering tolerance; 0.5 mm by default.
    pub tol: Option<f64>,
}

impl Default for PlateOptions {
    fn default() -> Self {
        PlateOptions {
            min_area_frac: 0.4,
            max_thick_frac: 0.5,
            tol: None,
        }
    }
}

internal_entry! {
/// `recognise_plates`: deduplicated by (axis, lo, hi) within each body, sorted by axis, `lo`,
/// `hi`, `u` and `v`.
pub fn recognise_plates(part: &Part, opts: &PlateOptions) -> Vec<Plate> {
    super::records(discover(&Context::new(part), opts))
}
}

internal_entry! {
/// The evidence path: each plate's low and high face groups, all on one valid solid, with no
/// face defining two plates and no plate key bound to two different face groups.
pub fn discover_verified(
    ctx: &Context<'_>,
    opts: &PlateOptions,
) -> Result<Vec<Occurrence<Plate>>, EvidenceError> {
    let found = evidence::verified(ctx.part, discover(ctx, opts))?;
    let mut used: Vec<usize> = Vec::new();
    for o in &found {
        if o.defining.iter().any(|f| used.contains(f)) {
            return Err(EvidenceError::SharedEvidence);
        }
        used.extend(&o.defining);
    }
    Ok(found)
}
}

internal_entry! {
/// The evidence path as Python's locally degraded run takes it (`_discover_plates` on a graph
/// with `local_degradation` set): a plate any of whose low or high faces proves no one valid
/// solid ([`evidence::locally_valid_solid`]) is skipped instead of refusing every plate, before
/// plates are deduplicated by key; the reuse check is made over the plates kept.
pub fn discover_locally_degraded(
    ctx: &Context<'_>,
    opts: &PlateOptions,
) -> Result<Vec<Occurrence<Plate>>, EvidenceError> {
    evidence::locally_degraded(|| discover_verified(ctx, opts))
}
}

/// One planar face on one side: its plane coordinate, area, area-weighted centre on the two
/// other axes, and the face.
type Entry = (f64, f64, f64, f64, usize);

/// One side's faces at one coordinate cluster: its lowest and highest plane coordinates, total
/// area, area-weighted centre sums on the two other axes, and the faces.
struct Group {
    low: f64,
    high: f64,
    area: f64,
    u_sum: f64,
    v_sum: f64,
    faces: Vec<usize>,
}

/// One body: a solid, or the whole part when it has none.
struct Scope {
    faces: Vec<usize>,
    bounds: Bounds,
    key: Option<BodyKey>,
}

internal_entry! {
/// Every plate with its low and high face groups as defining faces, in the order
/// `recognise_plates` returns them.
pub fn discover(ctx: &Context<'_>, opts: &PlateOptions) -> Vec<Occurrence<Plate>> {
    let part = ctx.part;
    let tol = opts.tol.unwrap_or(TOL);
    let scopes: Vec<Scope> = if part.solids.is_empty() {
        vec![Scope {
            faces: (0..part.faces.len()).collect(),
            bounds: *ctx.bounds(),
            key: None,
        }]
    } else {
        let keys = ctx.body_keys(true);
        part.solids
            .iter()
            .enumerate()
            .map(|(s, solid)| Scope {
                faces: solid.faces.clone(),
                bounds: part.solid_bounds(s),
                key: keys[s].clone(),
            })
            .collect()
    };
    let mut out: Vec<Occurrence<Plate>> = Vec::new();
    for scope in &scopes {
        let mut seen: Vec<(char, f64, f64)> = Vec::new();
        for mut o in proposals(ctx, scope, opts, tol) {
            // A locally degraded run skips a plate whose faces prove no one valid solid
            // (`_discover_plates` on a degraded graph, before it binds plate keys).
            if evidence::local_degradation()
                && evidence::locally_valid_solid(part, &o.defining).is_none()
            {
                continue;
            }
            let key = (o.record.axis, *o.record.lo, *o.record.hi);
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            o.record.body_key = scope.key.clone();
            out.push(o);
        }
    }
    out.sort_by(|a, b| {
        let (a, b) = (&a.record, &b.record);
        a.axis
            .cmp(&b.axis)
            .then(py::order(*a.lo, *b.lo))
            .then(py::order(*a.hi, *b.hi))
            .then(py::order(a.u, b.u))
            .then(py::order(a.v, b.v))
    });
    out
}
}

/// `_plate_proposals`: one body's plates, sorted by axis, `lo` and `hi`.
fn proposals(
    ctx: &Context<'_>,
    scope: &Scope,
    opts: &PlateOptions,
    tol: f64,
) -> Vec<Occurrence<Plate>> {
    let part = ctx.part;
    let b = &scope.bounds;
    let extents: V3 = std::array::from_fn(|i| b.max[i] - b.min[i]);
    let faces: Vec<usize> = scope
        .faces
        .iter()
        .copied()
        .filter(|&f| matches!(part.faces[f].surface, Surface::Plane { .. }))
        .collect();
    let mut out = Vec::new();
    for i in 0..3 {
        let oi: Vec<usize> = (0..3).filter(|&j| j != i).collect();
        // (location, area, u moment, v moment, face) per side: outward −axis, then +axis.
        let mut sides: [Vec<Entry>; 2] = [Vec::new(), Vec::new()];
        for &face in &faces {
            let Some(normal) = unit_plane_normal(part, face) else {
                continue;
            };
            let component = normal[i];
            if component.abs() < AXIS_ALIGNED_COS {
                continue;
            }
            let (Some([area, _]), Some(centre)) = (part.face_mass(face), part.face_centre(face))
            else {
                continue;
            };
            let Surface::Plane { frame } = part.faces[face].surface else {
                unreachable!("planar faces only");
            };
            sides[usize::from(component > 0.0)].push((
                frame.origin[i],
                area,
                centre[oi[0]] * area,
                centre[oi[1]] * area,
                face,
            ));
        }
        let [negative, positive] = sides.map(|side| group(&side, tol));
        let maximum_thickness = opts.max_thick_frac * extents[i];
        // No ordered thin opposed span: no area threshold could publish a plate here.
        if !negative.keys().any(|&low| {
            positive.keys().any(|&high| {
                tol < high.0 - low.0 && policy::clears_threshold(maximum_thickness, high.0 - low.0)
            })
        }) {
            continue;
        }
        let cross = oriented_cross_area(part, scope, &faces, i, extents);
        if cross <= 0.0 {
            continue;
        }
        let threshold = opts.min_area_frac * cross;
        let coverage = |g: &Group, sign| {
            policy::clears_threshold(g.area, threshold)
                || policy::clears_threshold(trimmed_coverage(ctx, g, i, sign).0.area, threshold)
        };
        let mut events: Vec<(f64, i8, &Group)> = negative
            .iter()
            .filter(|(_, g)| coverage(g, -1.0))
            .map(|(c, g)| (c.0, -1, g))
            .chain(
                positive
                    .iter()
                    .filter(|(_, g)| coverage(g, 1.0))
                    .map(|(c, g)| (c.0, 1, g)),
            )
            .collect();
        events.sort_by(|a, b| py::order(a.0, b.0).then(a.1.cmp(&b.1)));
        for w in events.windows(2) {
            let ((low, low_sign, low_group), (high, high_sign, high_group)) = (w[0], w[1]);
            if low_sign != -1 || high_sign != 1 {
                continue;
            }
            // Acceptance uses the band midpoints (the event keys), so it is symmetric under
            // reversal. The published lo/hi below are the bands' outer planes, so a wide band
            // can publish a plate thicker than `maximum_thickness` (the captured 6d359acf
            // fixture is accepted on 8.24995 and publishes 8.4999); the band, not its outer
            // plane, is what the threshold judges (#71).
            let thickness = high - low;
            if thickness <= tol || !policy::clears_threshold(maximum_thickness, thickness) {
                continue;
            }
            // Coordinate bands can include a nearby tab projecting beyond the opposing
            // skin (#85). Such a face is evidence for the band, but cannot extend a
            // plate whose opposite skin has no support there. Keep the original bands
            // for admission (#71); select both skins against those same original bands,
            // so exchanging the sides under axis reversal gives the same pair.
            let low_skin = supported_skin(part, low_group, high_group, i);
            let high_skin = supported_skin(part, high_group, low_group, i);
            let low_group = low_skin.as_ref().unwrap_or(low_group);
            let high_group = high_skin.as_ref().unwrap_or(high_group);
            let (low_group, mut context) = trimmed_coverage(ctx, low_group, i, -1.0);
            let (high_group, hi_context) = trimmed_coverage(ctx, high_group, i, 1.0);
            context.extend(hi_context);
            let combined_area = low_group.area + high_group.area;
            context.sort_unstable();
            context.dedup();
            // The plate is bounded by its skins' outermost real planes (#71).
            out.push(Occurrence {
                record: Plate {
                    axis: axis_letter(i),
                    // Recognition and fingerprints use the exact planes (#71).
                    // Retain them through subsequent publication and motions (#69, #84).
                    lo: Placed::from_parts(low_group.low, low_group.low, Grid::Decimals(3)),
                    hi: Placed::from_parts(high_group.high, high_group.high, Grid::Decimals(3)),
                    u: (low_group.u_sum + high_group.u_sum) / combined_area,
                    v: (low_group.v_sum + high_group.v_sum) / combined_area,
                    body_key: None,
                },
                defining: low_group
                    .faces
                    .iter()
                    .chain(&high_group.faces)
                    .copied()
                    .collect(),
                context,
            });
        }
    }
    out.sort_by(|a, b| {
        let (a, b) = (&a.record, &b.record);
        a.axis
            .cmp(&b.axis)
            .then(py::order(*a.lo, *b.lo))
            .then(py::order(*a.hi, *b.hi))
    });
    out
}

/// Cached supporting-plane moments. Projection and deduplication are shared by
/// recognition coverage, published transverse centres and fingerprint placement.
pub(super) struct Skin {
    normal: V3,
    area: f64,
    sum: V3,
    strips: Vec<Strip>,
}
struct Strip {
    face: usize,
    area: f64,
    sum: V3,
    context: Vec<usize>,
}
pub(crate) struct SupportingMoments {
    pub area: f64,
    pub sum: V3,
    pub context: Vec<usize>,
}

pub(super) fn measure_skin(ctx: &Context<'_>, face: usize) -> Option<Skin> {
    use crate::kernel::brep::Arc;
    let part = ctx.part;
    let normal = unit_plane_normal(part, face)?;
    let [area, _] = part.face_mass(face)?;
    let centre = part.face_centre(face)?;
    let plane = geom::dot(centre, normal);
    let neighbours = part.neighbours(face);
    let mut strips = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for treatment in ctx.edge_treatments() {
        for &g in &treatment.defining {
            if !neighbours.contains(&g) || !seen.insert(g) {
                continue;
            }
            // A root/concave junction does not trim this supporting skin.
            if !matches!(part.arc(face, g), Some(Arc::Convex | Arc::Smooth)) {
                continue;
            }
            let (_, hi) = part.extent_along(&[g], normal);
            if hi > plane + geom::COORD_FLOOR {
                continue;
            }
            let projected = if let Surface::Plane { .. } = part.faces[g].surface {
                // Constant Jacobian: reuse planar Green moments, rather than integrating
                // a surface with a constant projection factor a second time.
                part.face_mass(g)
                    .zip(part.face_centre(g))
                    .zip(unit_plane_normal(part, g))
                    .map(|(([area, _], centre), n)| (area * geom::dot(n, normal), centre))
            } else if matches!(part.faces[g].surface, Surface::Cone { frame, .. }
                if geom::norm(geom::cross(frame.z, normal)) <= 1e-12)
            {
                // A cone's outward normal has constant axial component. Its axial
                // projection therefore has the same centroid as its surface area.
                // Reuse these moments when the boss's body origin reads this face.
                ctx.surface_moments(g)
                    .map(|m| (geom::dot(m.area_vector, normal), m.centroid))
            } else {
                part.face_projected_moments(g, normal)
            };
            let Some((area, centre)) = projected else {
                continue;
            };
            if area <= 0. {
                continue;
            }
            let centre = geom::add(
                centre,
                geom::scale(normal, plane - geom::dot(centre, normal)),
            );
            strips.push(Strip {
                face: g,
                area,
                sum: geom::scale(centre, area),
                context: treatment
                    .defining
                    .iter()
                    .chain(&treatment.context)
                    .copied()
                    .collect(),
            });
        }
    }
    Some(Skin {
        normal,
        area,
        sum: geom::scale(centre, area),
        strips,
    })
}

pub(crate) fn supporting_moments(ctx: &Context<'_>, faces: &[usize]) -> Option<SupportingMoments> {
    let reference = ctx.skin(*faces.first()?)?.normal;
    let mut out = SupportingMoments {
        area: 0.,
        sum: [0.; 3],
        context: Vec::new(),
    };
    let mut counted = std::collections::BTreeSet::new();
    for &f in faces {
        let skin = ctx.skin(f)?;
        out.area += skin.area;
        out.sum = geom::add(out.sum, skin.sum);
        for strip in &skin.strips {
            if counted.insert((strip.face, geom::dot(skin.normal, reference) > 0.)) {
                out.area += strip.area;
                out.sum = geom::add(out.sum, strip.sum);
                out.context.extend(&strip.context);
            }
        }
    }
    out.context.sort_unstable();
    out.context.dedup();
    Some(out)
}

fn trimmed_coverage(ctx: &Context<'_>, g: &Group, axis: usize, _sign: f64) -> (Group, Vec<usize>) {
    let mut restored = Group {
        low: g.low,
        high: g.high,
        area: g.area,
        u_sum: g.u_sum,
        v_sum: g.v_sum,
        faces: g.faces.clone(),
    };
    let Some(m) = supporting_moments(ctx, &g.faces) else {
        return (restored, Vec::new());
    };
    let other: Vec<_> = (0..3).filter(|&i| i != axis).collect();
    restored.area = m.area;
    restored.u_sum = m.sum[other[0]];
    restored.v_sum = m.sum[other[1]];
    (restored, m.context)
}

/// A coordinate usable as an ordered map key (never NaN here).
#[derive(Clone, Copy, PartialEq)]
struct Coord(f64);
impl Eq for Coord {}
impl PartialOrd for Coord {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Coord {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        py::order(self.0, other.0)
    }
}

/// One side's faces clustered by plane coordinate (`cluster_coordinates`), keyed by each
/// cluster's midpoint, with sums in cluster order (unchanged by axis reversal).
fn group(side: &[Entry], tol: f64) -> BTreeMap<Coord, Group> {
    let locations: Vec<f64> = side.iter().map(|e| e.0).collect();
    let mut groups = BTreeMap::new();
    for cluster in policy::cluster_coordinates(&locations, tol) {
        let low = cluster
            .iter()
            .map(|&k| side[k].0)
            .fold(f64::INFINITY, f64::min);
        let high = cluster
            .iter()
            .map(|&k| side[k].0)
            .fold(f64::NEG_INFINITY, f64::max);
        let sum = |f: fn(&Entry) -> f64| cluster.iter().fold(0.0, |acc, &k| acc + f(&side[k]));
        groups.insert(
            Coord(0.5 * (low + high)),
            Group {
                low,
                high,
                area: sum(|e| e.1),
                u_sum: sum(|e| e.2),
                v_sum: sum(|e| e.3),
                faces: cluster.iter().map(|&k| side[k].4).collect(),
            },
        );
    }
    groups
}

/// A face outside the opposing band's transverse envelope belongs to a projecting
/// detail, rather than the plate skin. Prune it only when the band also contains a
/// predominant skin within that envelope. A small contained patch cannot replace
/// a larger real skin (for example holdout/808's stepped underside, #71). Otherwise
/// retain the band's outer real planes. Single-plane bands cannot move.
fn supported_skin(part: &Part, band: &Group, opposite: &Group, axis: usize) -> Option<Group> {
    let floor = crate::kernel::geom::COORD_FLOOR;
    if band.high - band.low <= floor {
        return None;
    }
    let mut envelope = Bounds::empty();
    for &face in &opposite.faces {
        envelope.merge(&part.face_bounds(face));
    }
    let faces: Vec<usize> = band
        .faces
        .iter()
        .copied()
        .filter(|&face| {
            let bounds = part.face_bounds(face);
            (0..3).filter(|&j| j != axis).all(|j| {
                bounds.min[j] >= envelope.min[j] - floor && bounds.max[j] <= envelope.max[j] + floor
            })
        })
        .collect();
    if faces.is_empty() || faces.len() == band.faces.len() {
        return None;
    }
    let other: Vec<usize> = (0..3).filter(|&j| j != axis).collect();
    let entries: Vec<Entry> = faces
        .into_iter()
        .map(|face| {
            let Surface::Plane { frame } = part.faces[face].surface else {
                unreachable!("planar face band");
            };
            let [area, _] = part.face_mass(face).unwrap();
            let centre = part.face_centre(face).unwrap();
            (
                frame.origin[axis],
                area,
                centre[other[0]] * area,
                centre[other[1]] * area,
                face,
            )
        })
        .collect();
    // A subset of this already-established band remains one cluster. Keeping its
    // original width avoids applying the default tolerance to a custom-options run.
    let skin = group(&entries, band.high - band.low).into_values().next()?;
    if !policy::clears_threshold(skin.area, band.area - skin.area)
        || (skin.low == band.low && skin.high == band.high)
    {
        return None;
    }
    Some(skin)
}

/// `_oriented_cross_area`: the smallest cross-envelope of the body across axis *i*, over the
/// directions its supporting transverse planes establish; the coordinate envelope when there
/// are none.
fn oriented_cross_area(part: &Part, scope: &Scope, faces: &[usize], i: usize, extents: V3) -> f64 {
    let other: [usize; 2] = match i {
        0 => [1, 2],
        1 => [0, 2],
        _ => [0, 1],
    };
    let vertices: Vec<V3> = scope
        .faces
        .iter()
        .flat_map(|&f| &part.faces[f].loops)
        .flat_map(|lp| {
            lp.vertex.into_iter().chain(
                lp.edges
                    .iter()
                    .flat_map(|&(e, _)| [part.edges[e].start, part.edges[e].end]),
            )
        })
        .collect();
    let support_eps = extents.iter().copied().fold(1.0, f64::max) * 1e-9;
    let project = |p: V3, n: V3| p[0] * n[0] + p[1] * n[1] + p[2] * n[2];
    let mut extreme_projections: Vec<(V3, f64)> = Vec::new();
    let mut angles: Vec<f64> = Vec::new();
    for &face in faces {
        let Some(normal) = unit_plane_normal(part, face) else {
            continue;
        };
        if normal[i].abs() > 1.0 - AXIS_ALIGNED_COS {
            continue;
        }
        if normal[other[0]].hypot(normal[other[1]]) < AXIS_ALIGNED_COS {
            continue;
        }
        let Surface::Plane { frame } = part.faces[face].surface else {
            continue;
        };
        let plane_projection = project(frame.origin, normal);
        if !vertices.is_empty() {
            let extreme = match extreme_projections.iter().find(|(n, _)| *n == normal) {
                Some(&(_, e)) => e,
                None => {
                    let e = vertices
                        .iter()
                        .map(|&v| project(v, normal))
                        .fold(f64::NEG_INFINITY, f64::max);
                    extreme_projections.push((normal, e));
                    e
                }
            };
            if extreme > plane_projection + support_eps {
                continue; // a concave or internal wall establishes no envelope direction
            }
        }
        let angle = py::modulo(normal[other[1]].atan2(normal[other[0]]).to_degrees(), 90.0);
        let angle = py::modulo(py::round_to(angle, ORIENTED_ANGLE_DIGITS), 90.0);
        if !angles.contains(&angle) {
            angles.push(angle);
        }
    }
    if angles.is_empty() {
        return extents[other[0]] * extents[other[1]];
    }
    // `part.rotate(axis, sign·angle)`: the turned body's extent along a coordinate axis is the
    // body's extent along that axis turned back.
    let sign = if i == 1 { 1.0 } else { -1.0 };
    let mut axis = [0.0; 3];
    axis[i] = 1.0;
    angles
        .iter()
        .map(|&angle| {
            if angle == 0.0 {
                return extents[other[0]] * extents[other[1]];
            }
            let (s, c) = (-sign * angle).to_radians().sin_cos();
            let size = |j: usize| {
                let mut e = [0.0; 3];
                e[j] = 1.0;
                let d = geom::add(geom::scale(e, c), geom::scale(geom::cross(axis, e), s));
                let (lo, hi) = part.extent_along(&scope.faces, d);
                hi - lo
            };
            size(other[0]) * size(other[1])
        })
        .fold(f64::INFINITY, f64::min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_band_midpoint_is_covariant_under_axis_reversal() {
        let side = [(0.0, 1.0, 0.0, 0.0, 0), (0.5, 1.0, 0.0, 0.0, 1)];
        let groups = group(&side, 0.5);
        assert_eq!(groups.len(), 1);
        let (key, g) = groups.first_key_value().unwrap();
        assert_eq!(key.0, 0.25);
        assert_eq!((g.low, g.high), (0.0, 0.5));
        let moved = side.map(|(c, a, u, v, f)| (123.456 - c, a, u, v, f));
        let groups = group(&moved, 0.5);
        assert_eq!(groups.len(), 1);
        let (key, g) = groups.first_key_value().unwrap();
        assert!((key.0 - (123.456 - 0.25)).abs() < 1e-12);
        // The real planes reverse with the part: the band's outermost plane on either side.
        assert_eq!((g.low, g.high), (123.456 - 0.5, 123.456));
    }
}
