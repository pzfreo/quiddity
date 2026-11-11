//! Part-relative recognition with an explicit caller-space frame (`quiddity.frames`).
//!
//! The supported entry point ([`crate::correspondence::recognise`]) infers the frame from
//! the loaded part, transforms its geometry in memory and maps records back to caller space.
//! Face and edge indices are preserved. Caller-space recognition and the legacy reader-based
//! frame preparation helpers are available only with the crate
//! tests, so parity checks keep exercising Python's entry-point semantics.
//!
//! The origin is the centroid of the part's solids ([`Part::volume_centroid`]), where Python
//! takes `BRepGProp::VolumeProperties` of the whole imported shape at its default rule: that also
//! counts a loose shell's open faces, and is 1e-9 to 1e-8 off on some analytic parts, enough to
//! make a mirror-symmetric part's rounded face offsets look asymmetric
//! (`docs/history/python-parity.md` lists each difference).
//!
//! Python's evidence projection onto caller faces (`_project_recognition_evidence`) is not
//! ported: the face indices already are the caller's.

use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::features;
#[cfg(test)]
use crate::features::Features;
use crate::kernel::brep::Part;
use crate::kernel::geom::{self, Surface, V3, cross};
use crate::kernel::py;
use crate::kernel::step::Placement;
#[cfg(test)]
use crate::kernel::step::{read_step_file_placed, read_step_placed};

/// Angular band for area ranking (the legacy `_PARALLEL_COS`), not for direction votes.
const PARALLEL_COS: f64 = 0.999;
/// Match material direction lines to numerical precision, rather than swallowing a real 1°
/// feature into whichever class happened to be read first (~0.0008° angular tolerance).
const DIRECTION_COS: f64 = 1.0 - 1e-10;
/// `_ORTHOGONAL_COS`: two direction lines at least this close to perpendicular make a basis.
const ORTHOGONAL_COS: f64 = 1.0 - PARALLEL_COS;
/// `_COMPONENT_EPS`: components this close to 0 or ±1 are snapped there.
const COMPONENT_EPS: f64 = 1e-12;

/// `FrameGauge`: how much of the returned basis the solid establishes. `Full`: a directed,
/// ordered basis. `Orthogonal`: two perpendicular direction lines, with a sign or an interchange
/// unobservable. `Axial`: one direction line, roll about it unobservable. In the latter two the
/// axes are representatives of the gauge, not material directions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FrameGauge {
    Full,
    Orthogonal,
    Axial,
}

/// `FrameRefusalReason`, plus `UnmeasuredFace` (the port only): the kernel cannot integrate a
/// plane or cylinder face's area, or a solid's volume, where OpenCascade always returns a
/// number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum FrameRefusal {
    NoMaterial,
    /// No unique analytic direction can establish the required frame or axial line.
    NoAnalyticDirection,
    NonfiniteGeometry,
    UnmeasuredFace,
    /// The inferred normalisation could not be applied as a finite proper rigid motion.
    NonrigidFrame,
}

impl FrameRefusal {
    /// Python's `FrameRefusalReason` value.
    pub fn as_str(self) -> &'static str {
        match self {
            FrameRefusal::NoMaterial => "no-material",
            FrameRefusal::NoAnalyticDirection => "no-analytic-direction",
            FrameRefusal::NonfiniteGeometry => "nonfinite-geometry",
            FrameRefusal::UnmeasuredFace => "unmeasured-face",
            FrameRefusal::NonrigidFrame => "nonrigid-frame",
        }
    }
}

/// `PartFrame`: the caller-space placement of the local recognition coordinates. A caller-space
/// point `p` is `(dot(p - origin, x), dot(p - origin, y), dot(p - origin, z))` locally.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PartFrame {
    pub origin: V3,
    pub x: V3,
    pub y: V3,
    pub z: V3,
    pub gauge: FrameGauge,
}

impl PartFrame {
    pub fn to_local(&self, p: V3) -> V3 {
        let d = [0, 1, 2].map(|i| p[i] - self.origin[i]);
        [self.x, self.y, self.z].map(|axis| py::dot(&d, &axis))
    }

    pub fn to_world(&self, p: V3) -> V3 {
        [0, 1, 2].map(|i| self.origin[i] + p[0] * self.x[i] + p[1] * self.y[i] + p[2] * self.z[i])
    }

    /// `_normalization_location`: the rigid motion taking caller space to local coordinates.
    pub fn normalisation(&self) -> Placement {
        [self.x, self.y, self.z].map(|a| [a[0], a[1], a[2], -py::dot(&a, &self.origin)])
    }
}

/// A plane's normals or a cylinder's axes, gathered as one direction line (`_DirectionClass`).
struct DirectionClass {
    direction: V3,
    area: f64,
    /// Distinct infinite analytic supports, independent of B-rep face splits.
    surfaces: Vec<AnalyticSupport>,
    face_areas: Vec<f64>,
    /// Each face's centroid offset from the origin along `direction`, with its area.
    face_offsets: Vec<(f64, f64)>,
}

/// One plane or infinite cylinder within a tight direction class. A cylinder's axial
/// placement and a plane's in-plane placement do not distinguish their supporting surfaces.
struct AnalyticSupport {
    origin: V3,
    radius: Option<f64>,
}

impl AnalyticSupport {
    fn same_surface(&self, other: &Self, direction: V3) -> bool {
        let delta = geom::sub(self.origin, other.origin);
        let along = geom::dot(delta, direction);
        match (self.radius, other.radius) {
            (None, None) => along.abs() <= geom::COORD_FLOOR,
            (Some(a), Some(b)) => {
                (a - b).abs() <= geom::COORD_FLOOR
                    && geom::norm(geom::sub(delta, geom::scale(direction, along)))
                        <= geom::COORD_FLOOR
            }
            _ => false,
        }
    }
}

/// `_DirectionClass.signature`, rounded to 9 decimals: the rotation-invariant rank of a class.
type Signature = (f64, Vec<f64>, Vec<[f64; 2]>);

fn descending(a: &[f64; 2], b: &[f64; 2]) -> std::cmp::Ordering {
    py::tuple_order(b, a)
}

impl DirectionClass {
    fn signature(&self) -> Signature {
        let r = |v: f64| py::round_to(v, 9);
        let mut areas: Vec<f64> = self.face_areas.iter().map(|&a| r(a)).collect();
        areas.sort_by(|a, b| py::order(*b, *a));
        let mut offsets: Vec<[f64; 2]> = self
            .face_offsets
            .iter()
            .map(|&(o, a)| [r(o.abs()), r(a)])
            .collect();
        offsets.sort_by(descending);
        (r(self.area), areas, offsets)
    }

    /// `oriented`: the geometry-directed representative, and whether its sign is observable
    /// (the face offsets are not symmetric about the origin).
    fn oriented(&self) -> (V3, bool) {
        let side = |sign: f64| {
            let mut out: Vec<[f64; 2]> = self
                .face_offsets
                .iter()
                .map(|&(o, a)| [py::round_to(sign * o, 9), py::round_to(a, 9)])
                .collect();
            out.sort_by(descending);
            out
        };
        let (forward, reverse) = (side(1.0), side(-1.0));
        let order = forward
            .iter()
            .zip(&reverse)
            .map(|(f, r)| py::tuple_order(f, r))
            .find(|o| o.is_ne());
        match order {
            None => (self.direction, false),
            Some(o) => {
                let sign = if o.is_gt() { 1.0 } else { -1.0 };
                (self.direction.map(|c| sign * c), true)
            }
        }
    }
}

fn signature_order(a: &Signature, b: &Signature) -> std::cmp::Ordering {
    py::order(a.0, b.0)
        .then_with(|| py::tuple_order(&a.1, &b.1))
        .then_with(|| {
            a.2.iter()
                .zip(&b.2)
                .map(|(x, y)| py::tuple_order(x, y))
                .find(|o| o.is_ne())
                .unwrap_or_else(|| a.2.len().cmp(&b.2.len()))
        })
}

/// `_canonical_sign`: the sign that makes the largest component (the last of equals) positive.
fn canonical_sign(v: V3) -> V3 {
    let pivot = (0..3)
        .max_by(|&i, &j| py::order(v[i].abs(), v[j].abs()).then(i.cmp(&j)))
        .unwrap();
    let sign = if v[pivot] < 0.0 { -1.0 } else { 1.0 };
    v.map(|c| sign * c)
}

/// `_clean`: components within 1e-12 of 0 or ±1 snapped there.
fn clean(v: V3) -> V3 {
    v.map(|c| {
        if c.abs() <= COMPONENT_EPS {
            0.0
        } else if (c.abs() - 1.0).abs() <= COMPONENT_EPS {
            1f64.copysign(c)
        } else {
            c
        }
    })
}

// Snapping each component independently can break orthogonality by more than the
// kernel's rigid-placement tolerance. Preserve already rigid bases byte for byte; repair
// only the exceptional basis, without snapping it a second time.
fn clean_basis(x: V3, y: V3) -> Result<(V3, V3, V3), FrameRefusal> {
    let (x, y) = (clean(x), clean(y));
    let z = clean(unit(cross(x, y))?);
    let axes = [x, y, z];
    // Use precisely the kernel's acceptance test: changing a basis already accepted by
    // the reader would unnecessarily change serialized coordinates on captured parts.
    let det = x[0] * (y[1] * z[2] - y[2] * z[1]) - x[1] * (y[0] * z[2] - y[2] * z[0])
        + x[2] * (y[0] * z[1] - y[1] * z[0]);
    if (det - 1.0).abs() <= 1e-12
        && (0..3).all(|i| {
            (0..3).all(|j| {
                (geom::dot(axes[i], axes[j]) - if i == j { 1.0 } else { 0.0 }).abs() <= 1e-12
            })
        })
    {
        return Ok((x, y, z));
    }
    let x = unit(x)?;
    let along = py::dot(&x, &y);
    let y = unit([0, 1, 2].map(|i| y[i] - along * x[i]))?;
    let z = unit(cross(x, y))?;
    let y = unit(cross(z, x))?;
    Ok((x, y, z))
}

fn unit(v: V3) -> Result<V3, FrameRefusal> {
    py::unit(v).ok_or(FrameRefusal::NonfiniteGeometry)
}

/// `_material_origin`: the centre of mass of the part's solids.
fn material_origin(part: &Part) -> Result<V3, FrameRefusal> {
    if part.solids.is_empty() {
        return Err(FrameRefusal::NoMaterial);
    }
    let (mass, centre) = part.volume_centroid().ok_or(FrameRefusal::UnmeasuredFace)?;
    if !mass.is_finite() {
        return Err(FrameRefusal::NonfiniteGeometry);
    }
    if mass <= COMPONENT_EPS {
        return Err(FrameRefusal::NoMaterial);
    }
    if !centre.iter().all(|c| c.is_finite()) {
        return Err(FrameRefusal::NonfiniteGeometry);
    }
    Ok(centre)
}

/// `infer_part_frame`: a rigid-equivariant material origin and a local basis from the part's
/// plane normals and cylinder axes. Choose the orthogonal basis supported by the most analytic
/// surfaces (one vote per coplanar plane or coaxial equal-radius cylinder, not per split face).
/// Directions match within `DIRECTION_COS`; plane offsets, cylinder axis distances and radii
/// match within the kernel's `COORD_FLOOR`. Supports need not share a topological edge.
/// File axes win an equal support count; otherwise the frame follows the material directions.
/// The vote settles the winner's leading line; a less-voted basis about that line whose box
/// around the material is smaller then sets the roll ([`tightest_roll`], #91), so a repeated
/// feature direction does not turn a plate away from its own outline.
/// Within the winning set, area in the legacy angular band ranks the axes, followed by the
/// exact class signatures; face offsets orient them.
/// Thus a rigid motion carries the intrinsic frame unless the moved file axes win this vote.
/// An axial part retains its explicit roll gauge; no unsupported second material axis is added.
pub fn infer_part_frame(part: &Part) -> Result<PartFrame, FrameRefusal> {
    let origin = material_origin(part)?;
    let mut classes: Vec<DirectionClass> = Vec::new();
    for (index, face) in part.faces.iter().enumerate() {
        // Python reads `face.normal_at()` for a plane; its sign is dropped by the canonical sign.
        let raw = match &face.surface {
            Surface::Plane { frame } | Surface::Cylinder { frame, .. } => frame.z,
            _ => continue,
        };
        let direction = canonical_sign(unit(raw)?);
        let surface = match &face.surface {
            Surface::Plane { frame } => AnalyticSupport {
                origin: frame.origin,
                radius: None,
            },
            Surface::Cylinder { frame, radius } => AnalyticSupport {
                origin: frame.origin,
                radius: Some(*radius),
            },
            _ => unreachable!(),
        };
        let moments = part
            .face_moments(index)
            .ok_or(FrameRefusal::UnmeasuredFace)?;
        let (area, centre) = (moments.area, moments.centroid);
        if !area.is_finite() || !centre.iter().all(|c| c.is_finite()) {
            return Err(FrameRefusal::NonfiniteGeometry);
        }
        let relative = [0, 1, 2].map(|i| centre[i] - origin[i]);
        let mut offset = py::dot(&relative, &direction);
        match classes
            .iter_mut()
            .find(|c| py::dot(&direction, &c.direction).abs() >= DIRECTION_COS)
        {
            Some(class) => {
                if py::dot(&direction, &class.direction) < 0.0 {
                    offset = -offset;
                }
                if !class
                    .surfaces
                    .iter()
                    .any(|s| s.same_surface(&surface, class.direction))
                {
                    class.surfaces.push(surface);
                }
                class.area += area;
                class.face_areas.push(area);
                class.face_offsets.push((offset, area));
            }
            None => classes.push(DirectionClass {
                direction,
                area,
                surfaces: vec![surface],
                face_areas: vec![area],
                face_offsets: vec![(offset, area)],
            }),
        }
    }

    // Keep the area ranking's existing angular band without merging its real directions.
    // Sum independently about each line, so face order cannot choose which 1° line represents
    // a class. This preserves the axis order of aligned rc/rd parts and follows rigid motion.
    let signatures: Vec<Signature> = classes.iter().map(DirectionClass::signature).collect();
    let rank_areas: Vec<_> = classes
        .iter()
        .map(|a| {
            py::round_to(
                classes
                    .iter()
                    .filter(|b| py::dot(&a.direction, &b.direction).abs() >= PARALLEL_COS)
                    .map(|b| b.area)
                    .sum(),
                9,
            )
        })
        .collect();
    let rank_order = |a: usize, b: usize| {
        py::order(rank_areas[a], rank_areas[b])
            .then_with(|| signature_order(&signatures[a], &signatures[b]))
    };
    let mut ranked: Vec<usize> = (0..classes.len()).collect();
    ranked.sort_by(|&a, &b| rank_order(b, a));
    // Any equal-ranked class leaves a possible axis interchange, even when it is not chosen.
    let distinct = (1..ranked.len()).all(|i| rank_order(ranked[i - 1], ranked[i]).is_ne());
    // Compare complete sets of three lines, not just the largest face-area class. In
    // particular, a shared dominant line must not let a minority angled feature choose roll.
    let support = |axes: [V3; 3]| -> usize {
        classes
            .iter()
            .filter(|class| {
                axes.iter()
                    .any(|axis| py::dot(axis, &class.direction).abs() >= DIRECTION_COS)
            })
            .map(|class| class.surfaces.len())
            .sum()
    };
    let file_axes = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
    let file_support = support(file_axes);
    let mut candidates: Vec<PartFrame> = Vec::new();
    let mut votes_of: Vec<usize> = Vec::new();
    let mut best: Option<(usize, usize)> = None;
    let mut aligned = None;
    for (k, &a) in ranked.iter().enumerate() {
        for &b in &ranked[k + 1..] {
            let (first, first_signed) = classes[a].oriented();
            let (second, second_signed) = classes[b].oriented();
            let along = py::dot(&first, &second);
            if along.abs() > ORTHOGONAL_COS {
                continue;
            }
            let y = unit([0, 1, 2].map(|i| second[i] - along * first[i]))?;
            let (x, y, z) = clean_basis(first, y)?;
            let gauge = if first_signed && second_signed && distinct {
                FrameGauge::Full
            } else {
                FrameGauge::Orthogonal
            };
            candidates.push(PartFrame {
                origin,
                x,
                y,
                z,
                gauge,
            });
            let index = candidates.len() - 1;
            // Keep the second material class's own votes when its direction is adjusted
            // by Gram-Schmidt (the perpendicular tolerance is wider than DIRECTION_COS).
            let votes = support([first, second, z]);
            votes_of.push(votes);
            // Strict improvement preserves the invariant signature ordering at a tie.
            if best.is_none_or(|(count, _)| votes > count) {
                best = Some((votes, index));
            }
            let file_line = |v: V3| {
                file_axes
                    .iter()
                    .find(|axis| py::dot(&v, *axis).abs() >= DIRECTION_COS)
                    .map(|axis| clean(axis.map(|c| c * py::dot(&v, axis).signum())))
            };
            if aligned.is_none()
                && let (Some(x), Some(y)) = (file_line(first), file_line(second))
                && py::dot(&x, &y).abs() < ORTHOGONAL_COS
            {
                // This is the file-axis direction set, but its material representative
                // must retain a real tiny rotation. Snapping recognition geometry here
                // leaves tilted pocket walls in an unrotated working frame (#70).
                // Only publication may snap, bounded by movement at the coordinate floor.
                aligned = Some(index);
            }
        }
    }
    if let Some((votes, winner)) = best {
        let winner = match aligned {
            Some(file) if file_support >= votes => file,
            _ => winner,
        };
        return Ok(tightest_roll(part, &candidates, &votes_of, winner));
    }
    let Some(&first) = ranked.first() else {
        return Err(FrameRefusal::NoAnalyticDirection);
    };
    // An axial gauge still needs one geometric direction line. Two equally ranked oblique
    // lines establish their acute bisector (the unique principal line of their scatter tensor),
    // independently of either support's sign or order. Three or more tied directions are not
    // resolved by a first face; the existing refusal reports the missing unique analytic line.
    let tied: Vec<_> = ranked
        .iter()
        .copied()
        .take_while(|&i| rank_order(first, i).is_eq())
        .collect();
    let x = axial_line(&classes, &tied)?;
    // Roll around this established line is an explicit gauge; a world seed represents it.
    let seeds = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let seed = seeds[py::first_min(&seeds, |s| py::dot(&x, s).abs())];
    let along = py::dot(&x, &seed);
    let y = unit([0, 1, 2].map(|i| seed[i] - along * x[i]))?;
    let (x, y, z) = clean_basis(x, y)?;
    Ok(PartFrame {
        origin,
        x,
        y,
        z,
        gauge: FrameGauge::Axial,
    })
}

/// Two material envelopes this close (relative) are one: a rigid motion moves the sampled
/// points by round-off only, and a near-round profile's rolls differ by far less than this.
const ENVELOPE_RELATIVE_TOL: f64 = 1e-4;

/// The volume of the box, aligned with *axes*, that holds every point.
fn envelope_volume(points: &[V3], axes: [V3; 3]) -> f64 {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in points {
        for (i, axis) in axes.iter().enumerate() {
            let t = geom::dot(*p, *axis);
            lo[i] = lo[i].min(t);
            hi[i] = hi[i].max(t);
        }
    }
    (0..3).map(|i| (hi[i] - lo[i]).max(0.0)).product()
}

/// The roll about the winning basis's leading line (#91). The vote settles that line; a
/// less-voted candidate basis that contains it replaces the winner when its box around the
/// material (every edge's sampled points) is smaller, so a repeated feature direction cannot
/// turn a part away from its own outline. An equal vote keeps the existing tie rule, a box
/// smaller by no more than [`ENVELOPE_RELATIVE_TOL`] keeps the winner, and the first of equally
/// tight others is taken in the vote's (intrinsic) candidate order. The envelope never reads
/// the file's axes.
fn tightest_roll(
    part: &Part,
    candidates: &[PartFrame],
    votes: &[usize],
    winner: usize,
) -> PartFrame {
    let lead = candidates[winner].x;
    let rival = |i: usize| {
        let c = &candidates[i];
        votes[i] < votes[winner]
            && [c.x, c.y, c.z]
                .iter()
                .any(|a| py::dot(a, &lead).abs() >= DIRECTION_COS)
    };
    if (0..candidates.len()).all(|i| !rival(i)) {
        return candidates[winner];
    }
    let points: Vec<V3> = part
        .edges
        .iter()
        .flat_map(|e| e.samples.iter().copied().chain([e.start, e.end]))
        .collect();
    let volume = |c: &PartFrame| envelope_volume(&points, [c.x, c.y, c.z]);
    let mut chosen = (winner, volume(&candidates[winner]));
    for (i, c) in candidates.iter().enumerate() {
        if !rival(i) {
            continue;
        }
        let v = volume(c);
        if v < chosen.1 - ENVELOPE_RELATIVE_TOL * chosen.1 {
            chosen = (i, v);
        }
    }
    candidates[chosen.0]
}

/// The unique line established by the top direction class, or by two tied oblique lines.
fn axial_line(classes: &[DirectionClass], tied: &[usize]) -> Result<V3, FrameRefusal> {
    Ok(match tied {
        [_] => classes[tied[0]].oriented().0,
        [a, b] => {
            let (a, b) = (classes[*a].direction, classes[*b].direction);
            let sign = if py::dot(&a, &b) < 0.0 { -1.0 } else { 1.0 };
            canonical_sign(unit([0, 1, 2].map(|i| a[i] + sign * b[i]))?)
        }
        _ => return Err(FrameRefusal::NoAnalyticDirection),
    })
}

/// Why framed recognition gave no result: the part could not be read, or the frame was refused.
#[cfg(test)]
#[doc(hidden)]
#[derive(Debug)]
#[non_exhaustive]
pub enum FramedError {
    Read(Box<dyn std::error::Error>),
    Frame(FrameRefusal),
}

#[cfg(test)]
#[doc(hidden)]
impl std::fmt::Display for FramedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FramedError::Read(e) => e.fmt(f),
            FramedError::Frame(r) => write!(f, "no part frame: {}", r.as_str()),
        }
    }
}

#[cfg(test)]
#[doc(hidden)]
impl std::error::Error for FramedError {}

/// `PreparedFramedPart`: the frame inferred on the caller's part, and the working part (the
/// caller's part in the frame's local coordinates, its faces under the caller's indices).
#[cfg(test)]
#[doc(hidden)]
pub struct FramedPart {
    pub frame: PartFrame,
    pub part: Part,
}

/// `outer` then `inner`, as one placement.
#[cfg(test)]
fn then(outer: &Placement, inner: &Placement) -> Placement {
    let mut out = [[0.0; 4]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, value) in row.iter_mut().enumerate() {
            *value = (0..3).map(|k| inner[i][k] * outer[k][j]).sum::<f64>();
        }
        row[3] += inner[i][3];
    }
    out
}

/// A part's source: its STEP file read under a placement ([`read_step_placed`] on the file's
/// bytes, or [`read_step_file_placed`]).
#[cfg(test)]
type Read<'a> = &'a dyn Fn(&Placement) -> Result<Part, Box<dyn std::error::Error>>;

/// `prepare_framed_part` for the part *read* gives under *placement* (the caller's space):
/// infers the frame on that part and reads it again with the frame's normalisation composed on.
/// This legacy reread route is retained only for unchanged Python-parity tests; supported
/// recognition transforms the loaded part in memory.
#[cfg(test)]
#[doc(hidden)]
pub fn prepare_framed(read: Read<'_>, placement: &Placement) -> Result<FramedPart, FramedError> {
    let caller = read(placement).map_err(FramedError::Read)?;
    let frame = infer_part_frame(&caller).map_err(FramedError::Frame)?;
    let part = working_part(read, placement, &caller, &frame).map_err(FramedError::Read)?;
    Ok(FramedPart { frame, part })
}

/// The working part of *caller*, the part *read* gives under *placement*, for *frame* (inferred
/// on *caller*): read again with the frame's normalisation composed on.
#[cfg(test)]
#[doc(hidden)]
pub fn working_part(
    read: Read<'_>,
    placement: &Placement,
    caller: &Part,
    frame: &PartFrame,
) -> Result<Part, Box<dyn std::error::Error>> {
    let part = read(&then(placement, &frame.normalisation()))?;
    assert_eq!(
        part.faces.len(),
        caller.faces.len(),
        "a placement does not change what is read"
    );
    Ok(part)
}

/// Where a recognition's records were found, as the recognition document reports it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum RecordFrame {
    /// Recognised in the part's own frame ([`infer_part_frame`]) and carried back to the file's
    /// coordinates. This frame is provenance; published record bases are self-contained.
    Inferred(PartFrame),
    /// No frame could be inferred, so the part was recognised as placed in the file: records
    /// depend on that placement (review M8), as caller-space recognition's do.
    Refused { reason: FrameRefusal },
    /// Test-only caller-space recognition, with no frame inferred.
    #[cfg(test)]
    #[doc(hidden)]
    CallerSpace,
}

/// [`prepare_framed`] for a STEP file's bytes.
#[cfg(test)]
#[doc(hidden)]
pub fn prepare_framed_step(bytes: &[u8], placement: &Placement) -> Result<FramedPart, FramedError> {
    prepare_framed(&|p| Ok(read_step_placed(bytes, p)?), placement)
}

/// [`prepare_framed`] for a STEP file on disk (gzipped when its name ends in `.gz`).
#[cfg(test)]
#[doc(hidden)]
pub fn prepare_framed_file(
    path: &std::path::Path,
    placement: &Placement,
) -> Result<FramedPart, FramedError> {
    prepare_framed(&|p| read_step_file_placed(path, p), placement)
}

#[cfg(test)]
#[doc(hidden)]
impl FramedPart {
    /// `PreparedFramedPart.recognise`: the working part recognised, with its frame.
    #[cfg(test)]
    #[doc(hidden)]
    pub fn recognise(self) -> FramedRecognition {
        let features = features::recognise(&self.part);
        FramedRecognition {
            frame: self.frame,
            part: self.part,
            features,
        }
    }
}

/// `FramedRecognitionResult`: the records found in the part's own frame, the frame, and the
/// working part they were found on.
#[cfg(test)]
#[doc(hidden)]
#[non_exhaustive]
pub struct FramedRecognition {
    pub frame: PartFrame,
    pub part: Part,
    pub features: Features,
}

#[cfg(test)]
mod axial_tie_tests {
    use super::*;
    #[test]
    fn oblique_bisector_follows_motion_and_ignores_support_sign_and_order() {
        for (axis, degrees) in [(2, 0_f64), (2, 30.), (0, 30.), (0, 90.)] {
            let (s, c) = degrees.to_radians().sin_cos();
            let rotate = |mut v: V3| {
                let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
                let (x, y) = (v[a], v[b]);
                v[a] = c * x - s * y;
                v[b] = s * x + c * y;
                v
            };
            let expected = rotate([3_f64.sqrt() / 2., 0.5, 0.]);
            for sign in [-1., 1.] {
                let classes: Vec<_> = [
                    rotate([1., 0., 0.]),
                    rotate([0.5, 3_f64.sqrt() / 2., 0.]).map(|c| c * sign),
                ]
                .into_iter()
                .map(|direction| DirectionClass {
                    direction,
                    area: 1.,
                    surfaces: vec![],
                    face_areas: vec![1.],
                    face_offsets: vec![(0., 1.)],
                })
                .collect();
                for order in [[0, 1], [1, 0]] {
                    let actual = axial_line(&classes, &order).unwrap();
                    assert!((py::dot(&actual, &expected).abs() - 1.).abs() < 1e-12);
                }
            }
        }
    }
}

#[cfg(test)]
mod rigid_basis_tests {
    use super::*;

    #[test]
    fn accepted_snapped_basis_is_preserved_byte_for_byte() {
        let x = unit([0.6, 0.8, 3e-13]).unwrap();
        let seed = [0.1, 0.2, 1.];
        let along = py::dot(&x, &seed);
        let y = unit([0, 1, 2].map(|i| seed[i] - along * x[i])).unwrap();
        let expected = (
            clean(x),
            clean(y),
            clean(unit(cross(clean(x), clean(y))).unwrap()),
        );
        assert!(geom::dot(expected.0, expected.1).abs() > 1e-13);
        let frame = PartFrame {
            origin: [1., 2., 3.],
            x: expected.0,
            y: expected.1,
            z: expected.2,
            gauge: FrameGauge::Full,
        };
        assert!(
            Part::new(vec![], vec![], vec![])
                .transformed(&frame.normalisation())
                .is_ok()
        );
        assert_eq!(clean_basis(x, y).unwrap(), expected);
    }

    #[test]
    fn near_orthonormal_basis_is_repaired_after_component_snapping() {
        let x = unit([1., 9e-13, 9e-13]).unwrap();
        let seed = [0., 1., 1.];
        let along = py::dot(&x, &seed);
        let y = unit([0, 1, 2].map(|i| seed[i] - along * x[i])).unwrap();
        assert!(py::dot(&x, &y).abs() < 1e-15);
        let snapped = PartFrame {
            origin: [1., 2., 3.],
            x: clean(x),
            y: clean(y),
            z: clean(unit(cross(x, y)).unwrap()),
            gauge: FrameGauge::Full,
        };
        let part = Part::new(vec![], vec![], vec![]);
        assert!(part.transformed(&snapped.normalisation()).is_err());
        let (x, y, z) = clean_basis(x, y).unwrap();
        let frame = PartFrame {
            origin: [1., 2., 3.],
            x,
            y,
            z,
            gauge: FrameGauge::Full,
        };
        let part = Part::new(vec![], vec![], vec![]);
        assert!(part.transformed(&frame.normalisation()).is_ok());
        for (i, a) in [x, y, z].iter().enumerate() {
            for (j, b) in [x, y, z].iter().enumerate() {
                assert!((py::dot(a, b) - if i == j { 1.0 } else { 0.0 }).abs() < 1e-14);
            }
        }
    }
}
