//! The run's effective-surface query (`quiddity._effective_surfaces`): each face's effective
//! analytic fact or its typed refusal, `recovery_nominal` / `recovery_tolerance`, and surface
//! uses carrying an independently proved material side (`MaterialSideCertificate`), which
//! rectangular pads, the cylindrical channel / pocket / passage proofs and section recesses read.
//!
//! | Python | Rust | Read by |
//! |---|---|---|
//! | `SurfaceKind` | `analytic_surfaces::SurfaceKind` | channels, pockets, passages, pads |
//! | `AnalyticSurfaceFact`, `RefusedSurfaceFact` | [`SurfaceFact`], [`SurfaceRefusalReason`] (`analytic_surfaces::AnalyticFact` is its kind, parameters and provenance) | all five |
//! | `SurfaceProvenance`, `OrientationCapability` | [`SurfaceProvenance`], [`SurfaceFact::oriented`] | channels, pockets, passages |
//! | `RecoveryCertificate` | partly: [`SurfaceFact::requested_tolerance`] is the bound; no OCCT version | none |
//! | `EffectiveSurfaceIndex.fact` / `oriented_fact` | [`EffectiveFaces::fact`] / [`EffectiveFaces::oriented_fact`] (`analytic_surfaces::effective_fact`, unchanged, agrees) | all five |
//! | `effective_faces_for_graph` / `_for_part`, the query protocols | `EffectiveFaces::new` over a `Context` | pads (section recesses pass it on) |
//! | `use`, `SurfaceUse`, `SurfaceUseRefusal` | [`EffectiveFaces::surface_use`], [`SurfaceUse`], [`SurfaceUseRefusal`] | pads |
//! | `MaterialSideCertificate`, `MaterialSideRefusalReason` | [`MaterialSideCertificate`], [`MaterialSideRefusalReason`] | pads (`outward`) |
//! | `cylinder_surface_dependency` | `cylinder_surface_dependency` | holes, bosses, circular blind steps (ported without it) |
//! | `recovery_nominal`, `recovery_tolerance` | `recovery_nominal`, `recovery_tolerance` (`recover.rs` fits against its own, from the edge samples' chords) | `_cylinder_substrate` |
//! | `SURFACE_READER_ROSTER`, `_SITES`, `SurfaceReaderDisposition` | not ported: Python source governance its architecture test reads | none |
//! | use snapshots, forgery and run checks | not applicable: these values are immutable and borrow their run | none |
//!
//! Where the kernels differ: a freeform face haecceity does not recover is refused as
//! [`SurfaceRefusalReason::NotRecovered`], which stands for Python's `fit-unavailable`,
//! `residual-exceeded` and `ambiguous-primitive` (`recover.rs` keeps the cause private), and a
//! recovered fact has no `kernel_reported_gap` (OpenCascade's own residual). The material side is
//! sampled at UV grid points of the face rather than OpenCascade mesh triangle centroids, and
//! classified by `classify.rs` rather than `BRepClass3d`, so the sample points differ while the
//! sign, the plane's outward normal and the probe distance are what consumers decide from.

use std::sync::OnceLock;

use super::analytic_surfaces::{AnalyticFact, SurfaceKind, validated_parameters};
use super::context::Context;
use super::evidence::common_valid_solid;
use super::regions::edge_length;
use crate::kernel::brep::Part;
use crate::kernel::classify::{ON_TOLERANCE, State};
use crate::kernel::geom::{self, COORD_FLOOR, Curve, Surface, V3};
use crate::kernel::py;
use crate::kernel::sampling::edge_interval;

const RECOVERY_REL: f64 = 1e-6;
const MATERIAL_PROBE_REL: f64 = 1e-4;
const MATERIAL_PROBE_CAP: f64 = 0.02;
const MATERIAL_MIN_SAMPLES: usize = 2;
const MATERIAL_MAX_SAMPLES: usize = 4;
/// Candidate sample cells across the face's parameter range, each way.
const SAMPLE_GRID: usize = 16;
/// `_MATERIAL_SIDE_AUTHORITY`, for the port's own sampling and classifier.
pub const MATERIAL_SIDE_AUTHORITY: &str =
    "original-face UV grid samples and haecceity ray-parity closed-solid side probes";

/// `SurfaceProvenance`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceProvenance {
    Native,
    Recovered,
}

/// `SurfaceRefusalReason`, without `unsupported-occt-contract` (no OpenCascade version gates the
/// port's recovery) and with `NotRecovered` for the three recovery outcomes `recover.rs` does
/// not tell apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SurfaceRefusalReason {
    /// An extrusion or revolution surface: not native, not a B-spline or Bezier.
    UnsupportedKind,
    UnsupportedTorusRecovery,
    /// A surface the reader could not resolve, or a freeform face without a finite positive
    /// area (its recovery tolerance is undefined).
    InvalidInput,
    /// A native or recovered primitive whose parameters do not validate.
    InvalidResult,
    /// No one primitive fits the freeform face within its recovery tolerance: none, or more than
    /// one (Python's `fit-unavailable`, `residual-exceeded`, `ambiguous-primitive`).
    NotRecovered,
}

impl SurfaceRefusalReason {
    /// Python's value for the reason (`NotRecovered` gives the three it stands for).
    pub fn python_values(self) -> &'static [&'static str] {
        match self {
            Self::UnsupportedKind => &["unsupported-kind"],
            Self::UnsupportedTorusRecovery => &["unsupported-torus-recovery"],
            Self::InvalidInput => &["invalid-input"],
            Self::InvalidResult => &["invalid-result"],
            Self::NotRecovered => &[
                "fit-unavailable",
                "residual-exceeded",
                "ambiguous-primitive",
            ],
        }
    }
}

/// `MaterialSideRefusalReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MaterialSideRefusalReason {
    SurfaceUnavailable,
    UnsupportedPrimitive,
    OwnerUnproven,
    SampleUnavailable,
    SampleNearBoundary,
    DifferentialDegenerate,
    ProbeIndeterminate,
    SamplesDisagree,
}

impl MaterialSideRefusalReason {
    pub fn python_value(self) -> &'static str {
        match self {
            Self::SurfaceUnavailable => "surface-unavailable",
            Self::UnsupportedPrimitive => "unsupported-primitive",
            Self::OwnerUnproven => "owner-unproven",
            Self::SampleUnavailable => "sample-unavailable",
            Self::SampleNearBoundary => "sample-near-boundary",
            Self::DifferentialDegenerate => "differential-degenerate",
            Self::ProbeIndeterminate => "probe-indeterminate",
            Self::SamplesDisagree => "samples-disagree",
        }
    }
}

/// `AnalyticSurfaceFact`: the face's kind, validated parameters and provenance, and for a
/// recovered surface the tolerance it was certified within (zero for a native one).
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceFact {
    pub face: usize,
    pub analytic: AnalyticFact,
    pub requested_tolerance: f64,
    /// `kernel_reported_gap`: zero for a native surface; `None` for a recovered one, whose
    /// fitted residual `recover.rs` keeps (it is within `requested_tolerance`).
    pub kernel_reported_gap: Option<f64>,
}

impl SurfaceFact {
    pub fn kind(&self) -> SurfaceKind {
        self.analytic.kind
    }

    pub fn parameters(&self) -> &[f64] {
        &self.analytic.parameters
    }

    pub fn provenance(&self) -> SurfaceProvenance {
        if self.analytic.native {
            SurfaceProvenance::Native
        } else {
            SurfaceProvenance::Recovered
        }
    }

    /// `OrientationCapability.NATIVE_ORIENTED`: only a native surface carries the face's
    /// orientation; a recovered one is a fit.
    pub fn oriented(&self) -> bool {
        self.analytic.native
    }
}

/// `EffectiveSurfaceFact`: the fact, or why the face has none.
pub type EffectiveSurfaceFact = Result<SurfaceFact, SurfaceRefusalReason>;

/// `MaterialSideCertificate`: which way the primitive's candidate normal points out of the face's
/// one valid solid. `outward` is the plane's normal times the sign, or a cylinder's first
/// sample's radial direction times it; consumers classify a cylinder by the sign.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialSideCertificate {
    pub face: usize,
    pub solid: usize,
    pub outward: V3,
    pub candidate_outward_sign: i32,
    pub outward_samples: Vec<V3>,
    pub sample_points: Vec<V3>,
    pub probe_distance: f64,
    pub classifier_tolerance: f64,
    /// The face's own orientation (`TopAbs_REVERSED`), recorded as Python records it.
    pub reversed: bool,
    pub authority: &'static str,
}

/// `SurfaceUse`: the provenance a consumer retains for one face.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceUse {
    pub face: usize,
    pub surface: SurfaceFact,
    pub material_side: Option<MaterialSideCertificate>,
}

/// `SurfaceUseRefusal`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceUseRefusal {
    pub face: usize,
    pub reason: MaterialSideRefusalReason,
}

/// `SurfaceUseResult`.
pub type SurfaceUseResult = Result<SurfaceUse, SurfaceUseRefusal>;

/// Why `oriented_fact` refuses (Python raises `ValueError`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum OrientedRefusal {
    Unavailable(SurfaceRefusalReason),
    /// `ORIENTATION_UNPROVEN`: a recovered surface.
    OrientationUnproven,
}

/// `effective_faces_for_graph`: one run's facts and uses, each computed once per face.
pub struct EffectiveFaces<'c, 'a> {
    ctx: &'c Context<'a>,
    facts: Vec<OnceLock<EffectiveSurfaceFact>>,
    uses: Vec<[OnceLock<SurfaceUseResult>; 2]>,
}

impl<'c, 'a> EffectiveFaces<'c, 'a> {
    internal_entry! {
        pub fn new(ctx: &'c Context<'a>) -> Self {
            let n = ctx.part.faces.len();
            EffectiveFaces {
                ctx,
                facts: (0..n).map(|_| OnceLock::new()).collect(),
                uses: (0..n).map(|_| [OnceLock::new(), OnceLock::new()]).collect(),
            }
        }
    }

    /// `EffectiveSurfaceIndex.fact`.
    pub fn fact(&self, face: usize) -> &EffectiveSurfaceFact {
        self.facts[face].get_or_init(|| derive(self.ctx.part, face))
    }

    /// `EffectiveSurfaceIndex.oriented_fact`: the fact when it is native.
    pub fn oriented_fact(&self, face: usize) -> Result<&SurfaceFact, OrientedRefusal> {
        match self.fact(face) {
            Err(reason) => Err(OrientedRefusal::Unavailable(*reason)),
            Ok(fact) if !fact.oriented() => Err(OrientedRefusal::OrientationUnproven),
            Ok(fact) => Ok(fact),
        }
    }

    /// `use(face, material_side=…)`: the face's surface use, with a certified material side when
    /// asked for one.
    pub fn surface_use(&self, face: usize, material_side: bool) -> &SurfaceUseResult {
        self.uses[face][usize::from(material_side)].get_or_init(|| {
            let refuse = |reason| Err(SurfaceUseRefusal { face, reason });
            let Ok(surface) = self.fact(face) else {
                return refuse(MaterialSideRefusalReason::SurfaceUnavailable);
            };
            let certificate = if material_side {
                match self.certify_material_side(face, surface) {
                    Ok(c) => Some(c),
                    Err(reason) => return refuse(reason),
                }
            } else {
                None
            };
            Ok(SurfaceUse {
                face,
                surface: surface.clone(),
                material_side: certificate,
            })
        })
    }

    /// `_certify_material_side`: probe either side of the face at a few interior samples, a
    /// probe distance along the primitive's candidate normal, against the face's one valid solid.
    fn certify_material_side(
        &self,
        face: usize,
        surface: &SurfaceFact,
    ) -> Result<MaterialSideCertificate, MaterialSideRefusalReason> {
        use MaterialSideRefusalReason as R;
        if !matches!(surface.kind(), SurfaceKind::Plane | SurfaceKind::Cylinder) {
            return Err(R::UnsupportedPrimitive);
        }
        let part = self.ctx.part;
        let solid = common_valid_solid(part, &[face]).ok_or(R::OwnerUnproven)?;
        let nominal = recovery_nominal(part, face).ok_or(R::SampleUnavailable)?;
        let probe_distance =
            MATERIAL_PROBE_CAP.min((MATERIAL_PROBE_REL * nominal).max(10.0 * COORD_FLOOR));
        let samples = grid_samples(part, face, probe_distance)?;
        let tolerance = RECOVERY_REL * nominal + COORD_FLOOR;
        let classifier = self.ctx.solid_classifier(solid);
        let (mut signs, mut points, mut normals) = (Vec::new(), Vec::new(), Vec::new());
        for (point, uv) in samples {
            let (point, direction) = if surface.kind() == SurfaceKind::Plane {
                let direction =
                    candidate_normal(surface, point).ok_or(R::DifferentialDegenerate)?;
                if !regular_differential(part, face, point, uv, direction, tolerance) {
                    return Err(R::DifferentialDegenerate);
                }
                (point, direction)
            } else {
                // One sample on a seam does not refuse a side the other samples prove
                // (`_regular_cylinder_sample` returning `None`).
                match cylinder_sample(part, face, point, uv, surface, probe_distance, tolerance) {
                    Some(sample) => sample,
                    None => continue,
                }
            };
            let state = |sign: f64| {
                classifier.classify(geom::add(
                    point,
                    geom::scale(direction, sign * probe_distance),
                ))
            };
            signs.push(match (state(1.0), state(-1.0)) {
                (State::Out, State::In) => 1,
                (State::In, State::Out) => -1,
                _ => return Err(R::ProbeIndeterminate),
            });
            points.push(point);
            normals.push(direction);
        }
        if signs.len() < MATERIAL_MIN_SAMPLES {
            return Err(R::DifferentialDegenerate);
        }
        if signs.iter().any(|&s| s != signs[0]) {
            return Err(R::SamplesDisagree);
        }
        let sign = signs[0];
        let outward_samples: Vec<V3> = normals
            .iter()
            .map(|n| geom::scale(*n, f64::from(sign)))
            .collect();
        Ok(MaterialSideCertificate {
            face,
            solid,
            outward: outward_samples[0],
            candidate_outward_sign: sign,
            outward_samples,
            sample_points: points,
            probe_distance,
            classifier_tolerance: ON_TOLERANCE,
            reversed: part.faces[face].reversed,
            authority: MATERIAL_SIDE_AUTHORITY,
        })
    }
}

internal_entry! {
/// `cylinder_surface_dependency`: a native cylinder's use as it is; a recovered one's with its
/// material side certified, since the fit carries no orientation.
pub fn cylinder_surface_dependency<'q>(
    effective: &'q EffectiveFaces<'_, '_>,
    face: usize,
) -> &'q SurfaceUseResult {
    let recovered = effective
        .fact(face)
        .as_ref()
        .is_ok_and(|f| f.provenance() == SurfaceProvenance::Recovered);
    effective.surface_use(face, recovered)
}
}

internal_entry! {
/// `recovery_nominal`: the face's controlling length, min(√area, 2·area / perimeter), the
/// perimeter being its physical trim (seams, used twice by the face, left out); `None` where
/// Python raises (no finite positive area).
pub fn recovery_nominal(part: &Part, face: usize) -> Option<f64> {
    let area = part.face_mass(face)?[0];
    if !(area.is_finite() && area > 0.0) {
        return None;
    }
    let perimeter = physical_boundary_length(part, face);
    if !(perimeter.is_finite() && perimeter >= 0.0) {
        return None;
    }
    let scale = area.sqrt();
    Some(if perimeter > 0.0 {
        scale.min(2.0 * area / perimeter)
    } else {
        scale
    })
}
}

/// `_physical_boundary_length`: the face's trim perimeter, without its seams (edges its loops use
/// twice). A degenerate edge (a pole) has no length.
pub(crate) fn physical_boundary_length(part: &Part, face: usize) -> f64 {
    let mut uses = std::collections::BTreeMap::<usize, usize>::new();
    for lp in &part.faces[face].loops {
        for &(e, _) in &lp.edges {
            *uses.entry(e).or_default() += 1;
        }
    }
    py::fsum(
        uses.iter()
            .filter(|&(_, &n)| n == 1)
            .map(|(&e, _)| arc_length(part, e)),
    )
}

/// An edge's length (`Edge.length`, `GCPnts_AbscissaPoint`): exact for lines and circular arcs,
/// otherwise by chords on the exact curve, each span halved until its Richardson-corrected length
/// (the chords' error falls as the square of the span) agrees with its halves' to 1e-12 of the
/// edge's length, or the chords' round-off (the edge samples' chords fall short by ~1e-5
/// relative). Chords, not the integrated
/// speed: a B-spline's short knot span can carry a fast stretch that fixed integration panels
/// step over (nist_ftc_10 face 178's edge, 3e-6 short at 64 Gauss panels).
pub(crate) fn arc_length(part: &Part, edge: usize) -> f64 {
    let e = &part.edges[edge];
    let curve = &e.curve;
    if matches!(curve, Curve::Line { .. } | Curve::Circle { .. }) {
        return edge_length(part, edge);
    }
    let (a, b) = edge_interval(curve, e.start, e.end, e.same_sense, e.is_closed());
    const START: usize = 64;
    let at = |i: usize| a + (b - a) * i as f64 / START as f64;
    let points: Vec<V3> = (0..=START).map(|i| curve.value(at(i))).collect();
    let rough: f64 = points.windows(2).map(|w| geom::dist(w[0], w[1])).sum();
    // Not below the chords' own round-off at these coordinates.
    let reach = points.iter().flatten().fold(0.0f64, |m, c| m.max(c.abs()));
    let tolerance = (1e-12 * rough).max(64.0 * f64::EPSILON * reach);
    py::fsum((0..START).map(|i| {
        let chord = geom::dist(points[i], points[i + 1]);
        span(
            curve,
            (at(i), points[i]),
            (at(i + 1), points[i + 1]),
            chord,
            tolerance,
            0,
        )
    }))
}

/// One span's length: its halves' chords, Richardson-corrected against the span's own chord,
/// compared with the same for each half; split further until they agree within *tolerance*.
fn span(
    curve: &Curve,
    (a, pa): (f64, V3),
    (b, pb): (f64, V3),
    chord: f64,
    tolerance: f64,
    depth: usize,
) -> f64 {
    let corrected = |chord: f64, halves: f64| halves + (halves - chord) / 3.0;
    let halves = |a: f64, pa: V3, b: f64, pb: V3| {
        let pm = curve.value(0.5 * (a + b));
        (pm, geom::dist(pa, pm), geom::dist(pm, pb))
    };
    let m = 0.5 * (a + b);
    let (pm, left, right) = halves(a, pa, b, pb);
    let whole = corrected(chord, left + right);
    let (_, ll, lr) = halves(a, pa, m, pm);
    let (_, rl, rr) = halves(m, pm, b, pb);
    let parts = corrected(left, ll + lr) + corrected(right, rl + rr);
    if depth >= 24 || (depth >= 2 && (parts - whole).abs() <= tolerance) {
        return parts;
    }
    span(curve, (a, pa), (m, pm), left, tolerance, depth + 1)
        + span(curve, (m, pm), (b, pb), right, tolerance, depth + 1)
}

internal_entry! {
/// `recovery_tolerance`: `1e-6 × nominal + COORD_FLOOR`.
pub fn recovery_tolerance(part: &Part, face: usize) -> Option<f64> {
    Some(RECOVERY_REL * recovery_nominal(part, face)? + COORD_FLOOR)
}
}

/// `EffectiveSurfaceIndex._derive`.
fn derive(part: &Part, face: usize) -> EffectiveSurfaceFact {
    use SurfaceRefusalReason as R;
    let surface = &part.faces[face].surface;
    let (kind, parameters, native, tolerance) = match surface {
        Surface::Plane { .. }
        | Surface::Cylinder { .. }
        | Surface::Cone { .. }
        | Surface::Sphere { .. } => {
            let (kind, parameters) = validated_parameters(surface).ok_or(R::InvalidResult)?;
            (kind, parameters, true, 0.0)
        }
        Surface::Torus { .. } => return Err(R::UnsupportedTorusRecovery),
        Surface::Freeform {
            kind: "BSPLINE" | "BEZIER",
            ..
        } => {
            let tolerance = recovery_tolerance(part, face).ok_or(R::InvalidInput)?;
            let recovered = part.recovered(face).ok_or(R::NotRecovered)?;
            let (kind, parameters) = validated_parameters(recovered).ok_or(R::InvalidResult)?;
            (kind, parameters, false, tolerance)
        }
        Surface::Freeform { .. } => return Err(R::UnsupportedKind),
        Surface::Other { .. } => return Err(R::InvalidInput),
    };
    Ok(SurfaceFact {
        face,
        analytic: AnalyticFact {
            kind,
            parameters,
            native,
        },
        requested_tolerance: tolerance,
        kernel_reported_gap: native.then_some(0.0),
    })
}

/// A sample point with its parameters on the face's own surface.
type Sample = (V3, (f64, f64));

/// `_triangle_samples`, on the port's own sampling: the centres of a grid of cells over the
/// face's parameter range that lie on the face, at least four probe distances from every edge,
/// the clearest first (ties in grid order), at most four of them.
fn grid_samples(
    part: &Part,
    face: usize,
    probe_distance: f64,
) -> Result<Vec<Sample>, MaterialSideRefusalReason> {
    use MaterialSideRefusalReason as R;
    let (Some((u0, u1, v0, v1)), Some(domain)) = (part.uv_bounds(face), part.domain(face)) else {
        return Err(R::SampleUnavailable);
    };
    let surface = &part.faces[face].surface;
    let edges = part.face_edges(face);
    let mut candidates: Vec<(f64, V3, (f64, f64))> = Vec::new();
    for i in 0..SAMPLE_GRID {
        for j in 0..SAMPLE_GRID {
            let u = u0 + (u1 - u0) * (i as f64 + 0.5) / SAMPLE_GRID as f64;
            let v = v0 + (v1 - v0) * (j as f64 + 0.5) / SAMPLE_GRID as f64;
            if !domain.contains(u, v) {
                continue;
            }
            let p = surface.value(u, v);
            if !p.iter().all(|c| c.is_finite()) {
                continue;
            }
            candidates.push((clearance(part, &edges, p), p, (u, v)));
        }
    }
    if candidates.is_empty() {
        return Err(R::SampleUnavailable);
    }
    // Stable: equal clearances keep grid order.
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0));
    let cleared: Vec<Sample> = candidates
        .into_iter()
        .filter(|(c, _, _)| c.is_finite() && *c >= 4.0 * probe_distance)
        .take(MATERIAL_MAX_SAMPLES)
        .map(|(_, p, uv)| (p, uv))
        .collect();
    if cleared.len() < MATERIAL_MIN_SAMPLES {
        return Err(R::SampleNearBoundary);
    }
    Ok(cleared)
}

/// The distance from *p* to the nearest of the face's edges (`Vertex.distance_to(edge)`), along
/// their samples.
fn clearance(part: &Part, edges: &[usize], p: V3) -> f64 {
    edges
        .iter()
        .flat_map(|&e| part.edges[e].samples.windows(2))
        .map(|w| segment_distance(p, w[0], w[1]))
        .fold(f64::INFINITY, f64::min)
}

fn segment_distance(p: V3, a: V3, b: V3) -> f64 {
    let ab = geom::sub(b, a);
    let len2 = geom::dot(ab, ab);
    let t = if len2 > 0.0 {
        (geom::dot(geom::sub(p, a), ab) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    geom::dist(p, geom::add(a, geom::scale(ab, t)))
}

/// `_candidate_normal`: the plane's normal, or the cylinder's radial direction at *point* when
/// the point lies on it within its tolerance.
fn candidate_normal(surface: &SurfaceFact, point: V3) -> Option<V3> {
    let p = surface.parameters();
    match surface.kind() {
        SurfaceKind::Plane => Some([p[0], p[1], p[2]]),
        SurfaceKind::Cylinder => {
            let relative = [0, 1, 2].map(|i| point[i] - p[i]);
            let along = py::fsum((0..3).map(|i| relative[i] * p[3 + i]));
            let radial = [0, 1, 2].map(|i| relative[i] - along * p[3 + i]);
            let magnitude = py::fsum(radial.iter().map(|c| c * c)).sqrt();
            if !magnitude.is_finite()
                || magnitude <= COORD_FLOOR
                || (magnitude - p[6]).abs() > surface.requested_tolerance.max(COORD_FLOOR)
            {
                return None;
            }
            Some(radial.map(|c| c / magnitude))
        }
        _ => None,
    }
}

/// `_regular_surface_differential`: the face's own surface, at the point's projection, has a
/// regular normal parallel to *direction* (within 1e-9 of the cosine), and the projection
/// recovers the point within the recovery tolerance.
fn regular_differential(
    part: &Part,
    face: usize,
    point: V3,
    hint: (f64, f64),
    direction: V3,
    tolerance: f64,
) -> bool {
    let surface = &part.faces[face].surface;
    let Some((u, v)) = surface.parameters(point, Some(hint)) else {
        return false;
    };
    let here = surface.value(u, v);
    let gap = py::fsum((0..3).map(|i| (here[i] - point[i]).powi(2))).sqrt();
    if !gap.is_finite() || gap > tolerance {
        return false;
    }
    let (du, dv) = surface.partials(u, v);
    let cross = geom::cross(du, dv);
    let magnitude = geom::norm(cross);
    if !magnitude.is_finite() || magnitude <= COORD_FLOOR * COORD_FLOOR {
        return false;
    }
    let alignment = geom::dot(geom::scale(cross, 1.0 / magnitude), direction).abs();
    alignment.is_finite() && alignment >= 1.0 - 1e-9
}

/// `_regular_cylinder_sample`: the sample projected onto the face's own surface, with the
/// recovered cylinder's radial normal there, when that normal is the face's and the point keeps
/// its clearance.
fn cylinder_sample(
    part: &Part,
    face: usize,
    seed: V3,
    hint: (f64, f64),
    surface: &SurfaceFact,
    probe_distance: f64,
    tolerance: f64,
) -> Option<(V3, V3)> {
    let own = &part.faces[face].surface;
    let (u, v) = own.parameters(seed, Some(hint))?;
    let point = own.value(u, v);
    let direction = candidate_normal(surface, point)?;
    if !regular_differential(part, face, point, (u, v), direction, tolerance) {
        return None;
    }
    let c = clearance(part, &part.face_edges(face), point);
    (c.is_finite() && c >= 4.0 * probe_distance).then_some((point, direction))
}
