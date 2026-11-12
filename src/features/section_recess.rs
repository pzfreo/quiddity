//! The section-recess records and their validation (`quiddity._section_recess`, ADR 0019):
//! a constant-section recess's reconstructible geometry (frame, run interval, closed or open
//! profile, and each end's condition and analytic surface), its classification and face
//! evidence, the patterns among recesses and the published document.
//!
//! Nothing here discovers or proves anything: the provers (`_section_recess_geometry`) and
//! `recognise_section_recesses` build these values, and every check refuses with Python's
//! `ValueError` message rather than repairing the value. Python also refuses inputs of the wrong
//! type or shape (a boolean, a list for a tuple, a record of another class, a negative or
//! non-integer index); the port's types cannot carry those. This module's fields are private and
//! its values are built only through their validating constructors, so Python's frozen
//! dataclasses' immutability is the type system's. The reused passage types (`PassageFrame`,
//! `PassageSectionVertex`) still have public fields, so a vertex written as a struct literal
//! reaches these constructors without the checks Python ran when it was built.

use std::cmp::Ordering;
use std::fmt;

use serde::Serialize;

use super::cylindrical_end_surface::CylindricalEndSurface;
use super::passages::{PassageError, PassageFrame, PassageSection, PassageSectionVertex};
use super::placed::Placed;
use super::sections::{
    SectionError, SectionVertex, V2, moments, validate_section_end_separation, validate_simple,
};
use crate::kernel::geom::V3;
use crate::kernel::py;

/// A refusal, with the Python implementation's `ValueError` message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionRecessError(pub &'static str);

impl fmt::Display for SectionRecessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for SectionRecessError {}

impl From<SectionError> for SectionRecessError {
    fn from(e: SectionError) -> Self {
        SectionRecessError(e.0)
    }
}

impl From<PassageError> for SectionRecessError {
    fn from(e: PassageError) -> Self {
        SectionRecessError(e.0)
    }
}

type Checked<T> = Result<T, SectionRecessError>;

fn refuse<T>(message: &'static str) -> Checked<T> {
    Err(SectionRecessError(message))
}

/// `_numbers`: every value finite, else *message* (Python's `"{name} must contain {size} finite
/// numbers"`). Unlike the passage records' `_numbers`, negative zeros are kept.
fn numbers<const N: usize>(values: [f64; N], message: &'static str) -> Checked<[f64; N]> {
    if values.iter().any(|v| !v.is_finite()) {
        return refuse(message);
    }
    Ok(values)
}

fn unrounded(value: f64, digits: usize) -> bool {
    py::round_to(value, digits) != value
}

fn section_vertices(boundary: &[PassageSectionVertex]) -> Checked<Vec<SectionVertex>> {
    Ok(boundary
        .iter()
        .map(|v| SectionVertex::new(v.point, v.bulge))
        .collect::<Result<_, _>>()?)
}

/// Python's tuple comparison of two boundaries: vertex by vertex, each by point then bulge.
fn boundary_order(a: &[PassageSectionVertex], b: &[PassageSectionVertex]) -> Ordering {
    let flat = |s: &[PassageSectionVertex]| -> Vec<f64> {
        s.iter()
            .flat_map(|v| [v.point[0], v.point[1], v.bulge])
            .collect()
    };
    py::tuple_order(&flat(a), &flat(b))
}

/// Whether a closed polygon's turns all have one sign (collinear corners allowed).
fn convex(points: &[V2]) -> bool {
    let n = points.len();
    let turns: Vec<f64> = (0..n)
        .map(|at| {
            let (previous, point, following) =
                (points[(at + n - 1) % n], points[at], points[(at + 1) % n]);
            (point[0] - previous[0]) * (following[1] - point[1])
                - (point[1] - previous[1]) * (following[0] - point[0])
        })
        .collect();
    let low = turns.iter().copied().fold(f64::INFINITY, f64::min);
    let high = turns.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    !(low < 0.0 && 0.0 < high)
}

fn min_of(values: &[f64]) -> f64 {
    values.iter().copied().fold(f64::INFINITY, f64::min)
}

fn max_of(values: &[f64]) -> f64 {
    values.iter().copied().fold(f64::NEG_INFINITY, f64::max)
}

/// One canonical physical closed line/arc boundary (`ClosedSectionProfile`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct ClosedSectionProfile {
    closure: &'static str,
    boundary: Vec<PassageSectionVertex>,
}

impl ClosedSectionProfile {
    /// The profile, refused unless *closure* is `"closed"` and the boundary is a canonical,
    /// origin-centred passage section.
    pub fn new(closure: &str, boundary: Vec<PassageSectionVertex>) -> Checked<Self> {
        if closure != "closed" {
            return refuse("closed section profile closure must be 'closed'");
        }
        PassageSection::new(boundary.clone())?;
        Ok(ClosedSectionProfile {
            closure: "closed",
            boundary,
        })
    }

    pub fn boundary(&self) -> &[PassageSectionVertex] {
        &self.boundary
    }
}

/// One canonical physical open line/arc chain plus its explicitly absent boundary
/// (`OpenSectionProfile`).
///
/// `material_side` is `"left"` or `"right"` of the directed physical chain: the side that
/// remains solid. `None` exists only for schema version 1 callers; recognition always publishes
/// the proved relation.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct OpenSectionProfile {
    closure: &'static str,
    boundary: Vec<PassageSectionVertex>,
    opening: [V2; 2],
    material_side: Option<&'static str>,
}

impl OpenSectionProfile {
    /// The profile, refused unless it is a simple chain of distinct vertices in its canonical
    /// direction whose final vertex implies no closing segment, its opening runs from the
    /// chain's end to its start, and its material side (when given) is the boundary's.
    pub fn new(
        closure: &str,
        boundary: Vec<PassageSectionVertex>,
        opening: [V2; 2],
        material_side: Option<&str>,
    ) -> Checked<Self> {
        if closure != "open" {
            return refuse("open section profile closure must be 'open'");
        }
        let n = boundary.len();
        if n < 2 {
            return refuse("open section profile requires at least two physical vertices");
        }
        if boundary[n - 1].bulge != 0.0 {
            return refuse("the final open-profile vertex cannot imply a closing segment");
        }
        if (0..n).any(|i| (0..i).any(|j| boundary[i].point == boundary[j].point)) {
            return refuse("open section profile vertices must be distinct");
        }
        validate_simple(&section_vertices(&boundary)?, false)?;
        let opening = [
            numbers(opening[0], "opening endpoint must contain 2 finite numbers")?,
            numbers(opening[1], "opening endpoint must contain 2 finite numbers")?,
        ];
        if opening[0] != boundary[n - 1].point || opening[1] != boundary[0].point {
            return refuse("opening must run from the physical chain end to its start");
        }
        let reversed: Vec<PassageSectionVertex> = (0..n)
            .map(|index| PassageSectionVertex {
                point: boundary[n - 1 - index].point,
                bulge: if index < n - 1 {
                    -boundary[n - 2 - index].bulge
                } else {
                    0.0
                },
            })
            .collect();
        if boundary_order(&reversed, &boundary) == Ordering::Less {
            return refuse("open section profile must use its canonical direction");
        }
        let material_side = match material_side {
            None => None,
            Some("left") => Some("left"),
            Some("right") => Some("right"),
            Some(_) => return refuse("open section profile material_side must be left or right"),
        };
        if let Some(side) = material_side
            && side != open_profile_material_side(&boundary)?
        {
            return refuse("open section profile material_side disagrees with its boundary");
        }
        Ok(OpenSectionProfile {
            closure: "open",
            boundary,
            opening,
            material_side,
        })
    }

    pub fn boundary(&self) -> &[PassageSectionVertex] {
        &self.boundary
    }

    pub fn opening(&self) -> [V2; 2] {
        self.opening
    }

    pub fn material_side(&self) -> Option<&'static str> {
        self.material_side
    }
}

internal_entry! {
/// The solid side of a canonical recess wall chain (`_open_profile_material_side`).
///
/// Closing the absent opening only for this orientation calculation gives the removed
/// section's winding; material is on the opposite side of its directed physical boundary.
pub fn open_profile_material_side(boundary: &[PassageSectionVertex]) -> Checked<&'static str> {
    let (area, _) = moments(&section_vertices(boundary)?)?;
    Ok(if area > 0.0 { "right" } else { "left" })
}
}

/// A section recess's profile: closed (pocket, passage) or open (edge-open recess, channel).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum SectionProfile {
    Closed(ClosedSectionProfile),
    Open(OpenSectionProfile),
}

impl SectionProfile {
    pub fn boundary(&self) -> &[PassageSectionVertex] {
        match self {
            SectionProfile::Closed(p) => p.boundary(),
            SectionProfile::Open(p) => p.boundary(),
        }
    }

    pub fn is_closed(&self) -> bool {
        matches!(self, SectionProfile::Closed(_))
    }
}

/// A plane through the centroid end coordinate, with local section gradients
/// (`PlanarEndSurface`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct PlanarEndSurface {
    #[serde(rename = "type")]
    kind: &'static str,
    gradient: V2,
}

impl PlanarEndSurface {
    /// The surface, refused unless *kind* is `"plane"` and the gradient is finite on the
    /// six-decimal grid.
    pub fn new(kind: &str, gradient: V2) -> Checked<Self> {
        if kind != "plane" {
            return refuse("planar end surface type must be 'plane'");
        }
        let gradient = numbers(gradient, "gradient must contain 2 finite numbers")?;
        if gradient.iter().any(|&v| unrounded(v, 6)) {
            return refuse("section end gradient must serialize at six decimal places");
        }
        Ok(PlanarEndSurface {
            kind: "plane",
            gradient,
        })
    }

    pub fn gradient(&self) -> V2 {
        self.gradient
    }
}

/// One absolute local-run affine term of an observed plane envelope (`PlanarEndTerm`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct PlanarEndTerm {
    height: Placed<f64>,
    gradient: V2,
}

impl PlanarEndTerm {
    /// The term, refused unless finite on the six-decimal grid.
    pub fn new(height: Placed<f64>, gradient: V2) -> Checked<Self> {
        let [value] = numbers([*height], "plane height must contain 1 finite numbers")?;
        let height = if value == *height {
            height
        } else {
            Placed::exact(value)
        };
        let gradient = numbers(gradient, "plane gradient must contain 2 finite numbers")?;
        if [*height, gradient[0], gradient[1]]
            .iter()
            .any(|&v| unrounded(v, 6))
        {
            return refuse("plane term must serialize at six decimals");
        }
        Ok(PlanarEndTerm { height, gradient })
    }

    pub fn height(&self) -> f64 {
        *self.height
    }

    /// The height with the value it was published from.
    pub fn placed_height(&self) -> Placed<f64> {
        self.height
    }

    pub fn gradient(&self) -> V2 {
        self.gradient
    }

    /// The plane's run coordinate over a section point (`at`).
    pub fn at(&self, point: V2) -> Checked<f64> {
        let [u, v] = numbers(point, "section point must contain 2 finite numbers")?;
        Ok(*self.height + self.gradient[0] * u + self.gradient[1] * v)
    }

    /// Python's dataclass order: by height, then gradient.
    fn order(&self, other: &Self) -> Ordering {
        py::tuple_order(
            &[*self.height, self.gradient[0], self.gradient[1]],
            &[*other.height, other.gradient[0], other.gradient[1]],
        )
    }
}

/// The minimum or maximum of two canonically ordered observed planes
/// (`PlanarEnvelopeEndSurface`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct PlanarEnvelopeEndSurface {
    #[serde(rename = "type")]
    kind: &'static str,
    operator: &'static str,
    terms: [PlanarEndTerm; 2],
}

impl PlanarEnvelopeEndSurface {
    /// The envelope, refused unless explicitly a `"min"` or `"max"` `"plane_envelope"` of two
    /// distinct, non-parallel terms in their canonical order.
    pub fn new(kind: &str, operator: &str, terms: [PlanarEndTerm; 2]) -> Checked<Self> {
        let operator = match (kind, operator) {
            ("plane_envelope", "min") => "min",
            ("plane_envelope", "max") => "max",
            _ => return refuse("plane envelope requires an explicit min/max discriminator"),
        };
        if terms[0].order(&terms[1]) != Ordering::Less {
            return refuse("plane envelope terms must be distinct and canonically ordered");
        }
        if terms[0].gradient == terms[1].gradient {
            return refuse("plane envelope terms must not be parallel");
        }
        Ok(PlanarEnvelopeEndSurface {
            kind: "plane_envelope",
            operator,
            terms,
        })
    }

    pub fn operator(&self) -> &'static str {
        self.operator
    }

    pub fn terms(&self) -> &[PlanarEndTerm; 2] {
        &self.terms
    }

    /// The envelope's run coordinate over a section point (`height`).
    pub fn height(&self, point: V2) -> Checked<f64> {
        let (a, b) = (self.terms[0].at(point)?, self.terms[1].at(point)?);
        // Python's `min`/`max` keep the first of ties.
        Ok(if self.operator == "min" {
            if b < a { b } else { a }
        } else if b > a {
            b
        } else {
            a
        })
    }
}

/// A section end's explicitly discriminated analytic surface.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum EndSurface {
    Planar(PlanarEndSurface),
    Cylindrical(CylindricalEndSurface),
    PlanarEnvelope(PlanarEnvelopeEndSurface),
}

/// A physical end condition and its analytic surface (`SectionEnd`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct SectionEnd {
    condition: &'static str,
    surface: EndSurface,
}

impl SectionEnd {
    /// The end, refused unless *condition* is `"open"` or `"capped"`. Python's default surface
    /// is a flat plane (`PlanarEndSurface::new("plane", [0.0, 0.0])`).
    pub fn new(condition: &str, surface: EndSurface) -> Checked<Self> {
        let condition = match condition {
            "open" => "open",
            "capped" => "capped",
            _ => return refuse("section end condition must be 'open' or 'capped'"),
        };
        Ok(SectionEnd { condition, surface })
    }

    pub fn condition(&self) -> &'static str {
        self.condition
    }

    pub fn surface(&self) -> &EndSurface {
        &self.surface
    }

    fn capped(&self) -> bool {
        self.condition == "capped"
    }

    /// The flat plane's gradient check Python makes on the end opposite a curved or envelope
    /// end: a planar surface with zero gradient.
    fn flat(&self) -> bool {
        matches!(&self.surface, EndSurface::Planar(p) if p.gradient == [0.0, 0.0])
    }
}

/// A section recess's low and high ends (`SectionRecessEnds`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct SectionRecessEnds {
    low: SectionEnd,
    high: SectionEnd,
}

impl SectionRecessEnds {
    /// Both ends; Python's only check (that both are `SectionEnd`s) is the types'.
    pub fn new(low: SectionEnd, high: SectionEnd) -> Checked<Self> {
        Ok(SectionRecessEnds { low, high })
    }

    pub fn low(&self) -> &SectionEnd {
        &self.low
    }

    pub fn high(&self) -> &SectionEnd {
        &self.high
    }
}

/// The reconstructible constant-section geometry selected by ADR 0019
/// (`SectionRecessGeometry`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct SectionRecessGeometry {
    #[serde(rename = "type")]
    kind: &'static str,
    frame: PassageFrame,
    run_interval: Placed<(f64, f64)>,
    profile: SectionProfile,
    ends: SectionRecessEnds,
}

impl SectionRecessGeometry {
    /// The geometry, refused unless its interval increases on the three-decimal grid and its
    /// ends are a proved configuration for the profile: two planes that do not cross or touch
    /// it, one plane envelope over a convex line-only polygon, or one cylindrical end.
    pub fn new(
        kind: &str,
        frame: PassageFrame,
        run_interval: Placed<(f64, f64)>,
        profile: SectionProfile,
        ends: SectionRecessEnds,
    ) -> Checked<Self> {
        if kind != "section_recess" {
            return refuse("geometry type must be 'section_recess'");
        }
        let interval = numbers(
            [run_interval.0, run_interval.1],
            "run_interval must contain 2 finite numbers",
        )?;
        if interval[1] - interval[0] <= 1e-9 || interval.iter().any(|&v| unrounded(v, 3)) {
            return refuse("run_interval must increase and serialize at three decimals");
        }
        let run_interval = if (interval[0], interval[1]) == *run_interval {
            run_interval
        } else {
            Placed::exact((interval[0], interval[1]))
        };
        let geometry = SectionRecessGeometry {
            kind: "section_recess",
            frame,
            run_interval,
            profile,
            ends,
        };
        match (&geometry.ends.low.surface, &geometry.ends.high.surface) {
            (EndSurface::Planar(low), EndSurface::Planar(high)) => {
                validate_section_end_separation(
                    &section_vertices(geometry.profile.boundary())?,
                    interval[1] - interval[0],
                    [
                        high.gradient[0] - low.gradient[0],
                        high.gradient[1] - low.gradient[1],
                    ],
                    geometry.profile.is_closed(),
                )?;
            }
            (EndSurface::PlanarEnvelope(_), _) | (_, EndSurface::PlanarEnvelope(_)) => {
                geometry.validate_plane_envelope_end(interval)?;
            }
            _ => geometry.validate_cylindrical_end(interval)?,
        }
        Ok(geometry)
    }

    pub fn frame(&self) -> &PassageFrame {
        &self.frame
    }

    pub fn run_interval(&self) -> (f64, f64) {
        *self.run_interval
    }

    /// The run interval with the values it was published from.
    pub fn placed_run_interval(&self) -> Placed<(f64, f64)> {
        self.run_interval
    }

    pub fn profile(&self) -> &SectionProfile {
        &self.profile
    }

    pub fn ends(&self) -> &SectionRecessEnds {
        &self.ends
    }

    fn points(&self) -> Vec<V2> {
        self.profile.boundary().iter().map(|v| v.point).collect()
    }

    fn line_only(&self) -> bool {
        self.profile.boundary().iter().all(|v| v.bulge == 0.0)
    }

    /// `_validate_plane_envelope_end`.
    fn validate_plane_envelope_end(&self, interval: [f64; 2]) -> Checked<()> {
        if !self.profile.is_closed() || !self.line_only() {
            return refuse("plane envelope requires a closed line-only polygon");
        }
        let ends = [&self.ends.low, &self.ends.high];
        let indices: Vec<usize> = (0..2)
            .filter(|&i| matches!(ends[i].surface, EndSurface::PlanarEnvelope(_)))
            .collect();
        let &[index] = indices.as_slice() else {
            return refuse("section requires exactly one plane envelope end");
        };
        let EndSurface::PlanarEnvelope(envelope) = &ends[index].surface else {
            unreachable!("filtered on the envelope variant")
        };
        let planar = ends[1 - index];
        if ends.iter().any(|end| end.condition != "open")
            || !planar.flat()
            || envelope.operator != (if index == 1 { "min" } else { "max" })
        {
            return refuse("plane envelope requires a convex roof and opposite planar mouth");
        }
        let points = self.points();
        if !convex(&points) {
            return refuse("plane envelope requires a convex polygon");
        }
        let differences = points
            .iter()
            .map(|&p| Ok(envelope.terms[0].at(p)? - envelope.terms[1].at(p)?))
            .collect::<Checked<Vec<f64>>>()?;
        if !(min_of(&differences) < -1e-9 && max_of(&differences) > 1e-9) {
            return refuse("both plane terms must contribute positive-area end patches");
        }
        let gaps = points
            .iter()
            .map(|&p| {
                let height = envelope.height(p)?;
                Ok(if index == 1 {
                    height - interval[0]
                } else {
                    interval[1] - height
                })
            })
            .collect::<Checked<Vec<f64>>>()?;
        if min_of(&gaps) <= 1e-9 {
            return refuse("plane envelope ends must remain strictly separated");
        }
        if py::round_to(envelope.height([0.0, 0.0])?, 3) != interval[index] {
            return refuse("run interval must agree with the plane envelope centroid");
        }
        Ok(())
    }

    /// `_validate_cylindrical_end`.
    fn validate_cylindrical_end(&self, interval: [f64; 2]) -> Checked<()> {
        if !self.line_only() {
            return refuse("cylindrical end requires a line-only profile");
        }
        let closed = self.profile.is_closed();
        if !closed {
            self.validate_cylindrical_channel_profile()?;
        }
        let ends = [&self.ends.low, &self.ends.high];
        let indices: Vec<usize> = (0..2)
            .filter(|&i| matches!(ends[i].surface, EndSurface::Cylindrical(_)))
            .collect();
        let &[index] = indices.as_slice() else {
            return refuse("cylindrical section requires exactly one cylindrical end");
        };
        let EndSurface::Cylindrical(curved) = &ends[index].surface else {
            unreachable!("filtered on the cylindrical variant")
        };
        let planar = ends[1 - index];
        let passage = closed && planar.condition == "open";
        if passage && !convex(&self.points()) {
            return refuse("cylindrical passage requires a convex polygon");
        }
        let pocket = closed && !passage;
        let branch = match (pocket, index == 1) {
            (true, true) | (false, false) => "positive",
            (true, false) | (false, true) => "negative",
        };
        if ends[index].condition != "open"
            || planar.condition != (if pocket { "capped" } else { "open" })
            || !planar.flat()
            || curved.branch() != branch
        {
            return refuse(
                "cylindrical section requires the proved pocket, passage or open-channel end \
                 configuration",
            );
        }
        let (low, high) = curved.polygon_height_bounds(&self.points())?;
        let separation = if index == 1 {
            low - interval[0]
        } else {
            interval[1] - high
        };
        if separation <= 1e-9 {
            return refuse("cylindrical section ends must remain strictly separated");
        }
        if py::round_to(curved.height([0.0, 0.0])?, 3) != interval[index] {
            return refuse("run interval must agree with the cylindrical centroid intersection");
        }
        Ok(())
    }

    /// Only an origin-centred rectangular U admits the private probe closure
    /// (`_validate_cylindrical_channel_profile`).
    fn validate_cylindrical_channel_profile(&self) -> Checked<()> {
        let points = self.points();
        if points.len() != 4 {
            return refuse("cylindrical channel requires a rectangular three-line U-profile");
        }
        let [first, middle, last] =
            [0, 1, 2].map(|k| [0, 1].map(|i| points[k + 1][i] - points[k][i]));
        let square = py::sum((0..2).map(|i| first[i] * middle[i]));
        let centre = [0, 1].map(|i| py::sum(points.iter().map(|p| p[i])) / 4.0);
        if (0..2)
            .map(|i| (first[i] + last[i]).abs())
            .fold(f64::NEG_INFINITY, f64::max)
            > 1e-9
            || square.abs() > 1e-8 * py::hypot(&first) * py::hypot(&middle)
            || centre
                .iter()
                .map(|c| c.abs())
                .fold(f64::NEG_INFINITY, f64::max)
                > 1e-9
        {
            return refuse("cylindrical channel probe domain must be an origin-centred rectangle");
        }
        Ok(())
    }
}

const FEATURE_KINDS: [&str; 4] = ["pocket", "edge_open_recess", "passage", "channel"];
const SECTION_SHAPES: [&str; 7] = [
    "rectangular",
    "circular",
    "obround",
    "triangular",
    "hexagonal",
    "polygonal",
    "general",
];

/// A recess's authoritative feature kind and section shape (`SectionRecessClassification`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct SectionRecessClassification {
    feature_kind: &'static str,
    section_shape: &'static str,
}

impl SectionRecessClassification {
    pub fn new(feature_kind: &str, section_shape: &str) -> Checked<Self> {
        let Some(feature_kind) = FEATURE_KINDS.into_iter().find(|&k| k == feature_kind) else {
            return refuse("unsupported section recess feature_kind");
        };
        let Some(section_shape) = SECTION_SHAPES.into_iter().find(|&s| s == section_shape) else {
            return refuse("unsupported section recess section_shape");
        };
        Ok(SectionRecessClassification {
            feature_kind,
            section_shape,
        })
    }

    pub fn feature_kind(&self) -> &'static str {
        self.feature_kind
    }

    pub fn section_shape(&self) -> &'static str {
        self.section_shape
    }
}

/// A recess's defining (wall) faces and its complete constituent set
/// (`SectionRecessEvidence`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct SectionRecessEvidence {
    defining_faces: Vec<usize>,
    constituent_faces: Vec<usize>,
}

impl SectionRecessEvidence {
    /// The evidence, refused unless both are sorted unique indices and every defining face is
    /// a constituent.
    pub fn new(defining_faces: Vec<usize>, constituent_faces: Vec<usize>) -> Checked<Self> {
        if !defining_faces.is_sorted_by(|a, b| a < b) {
            return refuse("defining_faces must be sorted unique non-negative indices");
        }
        if !constituent_faces.is_sorted_by(|a, b| a < b) {
            return refuse("constituent_faces must be sorted unique non-negative indices");
        }
        if defining_faces
            .iter()
            .any(|f| constituent_faces.binary_search(f).is_err())
        {
            return refuse("defining faces must be constituent faces");
        }
        Ok(SectionRecessEvidence {
            defining_faces,
            constituent_faces,
        })
    }

    pub fn defining_faces(&self) -> &[usize] {
        &self.defining_faces
    }

    pub fn constituent_faces(&self) -> &[usize] {
        &self.constituent_faces
    }
}

/// A body in the document's roster (`SectionRecessBodyRef`); Python's only check (a
/// non-negative int) is the type's.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct SectionRecessBodyRef {
    pub index: usize,
}

/// A face in the document's roster (`SectionRecessFaceRef`); Python's only check (a
/// non-negative int) is the type's.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct SectionRecessFaceRef {
    pub index: usize,
}

/// One published constant-section recess (`SectionRecess`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct SectionRecess {
    /// Optional additive reference to the identical/superseding published feature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    index: usize,
    body: usize,
    geometry: SectionRecessGeometry,
    classification: SectionRecessClassification,
    evidence: SectionRecessEvidence,
}

impl SectionRecess {
    /// The occurrence, refused unless its classification agrees with its profile's closure and
    /// its count of capped ends: a pocket is closed with one cap, an edge-open recess open with
    /// one, a passage closed with none and a channel open with none.
    pub fn new(
        index: usize,
        body: usize,
        geometry: SectionRecessGeometry,
        classification: SectionRecessClassification,
        evidence: SectionRecessEvidence,
    ) -> Checked<Self> {
        let capped =
            usize::from(geometry.ends.low.capped()) + usize::from(geometry.ends.high.capped());
        let closed = geometry.profile.is_closed();
        let admitted = match classification.feature_kind {
            "pocket" => closed && capped == 1,
            "edge_open_recess" => !closed && capped == 1,
            "passage" => closed && capped == 0,
            _ => !closed && capped == 0,
        };
        if !admitted {
            return refuse("classification, profile closure and end topology are inconsistent");
        }
        Ok(SectionRecess {
            source: None,
            index,
            body,
            geometry,
            classification,
            evidence,
        })
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn body(&self) -> usize {
        self.body
    }

    pub fn geometry(&self) -> &SectionRecessGeometry {
        &self.geometry
    }

    /// Carry an already validated measuring frame without changing its relative geometry.
    pub(crate) fn with_frame(mut self, frame: PassageFrame) -> Self {
        self.geometry.frame = frame;
        self
    }

    pub fn classification(&self) -> &SectionRecessClassification {
        &self.classification
    }

    pub fn evidence(&self) -> &SectionRecessEvidence {
        &self.evidence
    }
}

/// Source evidence for an internal candidate that cannot issue truthful unified geometry
/// (`SectionRecessRefusal`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct SectionRecessRefusal {
    body: usize,
    reason: &'static str,
    evidence: SectionRecessEvidence,
}

impl SectionRecessRefusal {
    pub fn new(body: usize, reason: &str, evidence: SectionRecessEvidence) -> Checked<Self> {
        if reason != "unsupported_support_geometry" {
            return refuse("unsupported section projection refusal");
        }
        Ok(SectionRecessRefusal {
            body,
            reason: "unsupported_support_geometry",
            evidence,
        })
    }

    pub fn body(&self) -> usize {
        self.body
    }

    pub fn evidence(&self) -> &SectionRecessEvidence {
        &self.evidence
    }
}

/// `_pattern_members`.
fn pattern_members(members: &[usize]) -> Checked<()> {
    if members.len() < 2 || (0..members.len()).any(|i| members[..i].contains(&members[i])) {
        return refuse("pattern requires distinct non-negative occurrence indices");
    }
    Ok(())
}

/// Whether a direction is unit length to 1e-6 (Python's `isclose` of its squared norm to 1).
fn unit_length(direction: V3) -> bool {
    py::isclose(py::sum(direction.map(|v| v * v)), 1.0, 1e-9, 1e-6)
}

/// A line of member section-centroid run midpoints, ordered along `direction`
/// (`SectionRecessArray`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct SectionRecessArray {
    members: Vec<usize>,
    pitch: f64,
    direction: V3,
}

impl SectionRecessArray {
    pub fn new(members: Vec<usize>, pitch: f64, direction: V3) -> Checked<Self> {
        pattern_members(&members)?;
        let [checked] = numbers([pitch], "array pitch must contain 1 finite numbers")?;
        if checked <= 0.0 {
            return refuse("array pitch must be positive");
        }
        let checked = numbers(direction, "array direction must contain 3 finite numbers")?;
        if !unit_length(checked) {
            return refuse("array direction must be unit length");
        }
        // Python validates converted copies and keeps the fields as given.
        Ok(SectionRecessArray {
            members,
            pitch,
            direction,
        })
    }

    pub fn members(&self) -> &[usize] {
        &self.members
    }

    pub fn pitch(&self) -> f64 {
        self.pitch
    }

    pub fn direction(&self) -> V3 {
        self.direction
    }
}

/// A row-major lattice of member section-centroid run midpoints (`SectionRecessGrid`).
///
/// `center` is their arithmetic mean in the recognition coordinate system; it is not an
/// envelope centre or the centroid of removed volume.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct SectionRecessGrid {
    members: Vec<usize>,
    rows: usize,
    cols: usize,
    row_pitch: f64,
    col_pitch: f64,
    row_direction: V3,
    col_direction: V3,
    center: V3,
}

impl SectionRecessGrid {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        members: Vec<usize>,
        rows: usize,
        cols: usize,
        row_pitch: f64,
        col_pitch: f64,
        row_direction: V3,
        col_direction: V3,
        center: V3,
    ) -> Checked<Self> {
        pattern_members(&members)?;
        if rows < 2 || cols < 2 {
            return refuse("grid requires at least two rows and columns");
        }
        if rows.checked_mul(cols) != Some(members.len()) {
            return refuse("grid dimensions must match member count");
        }
        let pitches = numbers(
            [row_pitch, col_pitch],
            "grid pitches must contain 2 finite numbers",
        )?;
        if pitches.iter().any(|&p| p <= 0.0) {
            return refuse("grid pitches must be positive");
        }
        let row = numbers(
            row_direction,
            "grid row direction must contain 3 finite numbers",
        )?;
        let col = numbers(
            col_direction,
            "grid column direction must contain 3 finite numbers",
        )?;
        if !unit_length(row) || !unit_length(col) {
            return refuse("grid directions must be unit length");
        }
        if py::sum((0..3).map(|i| row[i] * col[i])).abs() > 1e-6 {
            return refuse("grid directions must be perpendicular");
        }
        numbers(center, "grid center must contain 3 finite numbers")?;
        Ok(SectionRecessGrid {
            members,
            rows,
            cols,
            row_pitch,
            col_pitch,
            row_direction,
            col_direction,
            center,
        })
    }

    pub fn members(&self) -> &[usize] {
        &self.members
    }

    /// Carry the lattice into another rigid coordinate frame, retaining its member indices.
    pub fn mapped(&self, direction: impl Fn(V3) -> V3, point: impl Fn(V3) -> V3) -> Checked<Self> {
        Self::new(
            self.members.clone(),
            self.rows,
            self.cols,
            self.row_pitch,
            self.col_pitch,
            direction(self.row_direction),
            direction(self.col_direction),
            point(self.center),
        )
    }
}

/// A pattern among a document's occurrences.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum SectionRecessPattern {
    Array(SectionRecessArray),
    Grid(SectionRecessGrid),
}

impl SectionRecessPattern {
    pub fn members(&self) -> &[usize] {
        match self {
            SectionRecessPattern::Array(a) => a.members(),
            SectionRecessPattern::Grid(g) => g.members(),
        }
    }
}

/// The published section-recess document, schema version 4 (`SectionRecessDocument`): dense
/// body, face and occurrence rosters, refusals and patterns, every reference inside them.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct SectionRecessDocument {
    schema_version: i64,
    reference_scope: &'static str,
    bodies: Vec<SectionRecessBodyRef>,
    faces: Vec<SectionRecessFaceRef>,
    occurrences: Vec<SectionRecess>,
    refusals: Vec<SectionRecessRefusal>,
    patterns: Vec<SectionRecessPattern>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    source_refusals: Vec<super::discovery::DiscoveryRefusal>,
}

impl SectionRecessDocument {
    pub fn new(
        schema_version: i64,
        reference_scope: &str,
        bodies: Vec<SectionRecessBodyRef>,
        faces: Vec<SectionRecessFaceRef>,
        occurrences: Vec<SectionRecess>,
        refusals: Vec<SectionRecessRefusal>,
        patterns: Vec<SectionRecessPattern>,
    ) -> Checked<Self> {
        if schema_version != 4 || reference_scope != "result" {
            return refuse("unsupported section-recess document");
        }
        if bodies.iter().enumerate().any(|(i, b)| b.index != i) {
            return refuse("body roster must be dense and ordered");
        }
        if faces.iter().enumerate().any(|(i, f)| f.index != i) {
            return refuse("face roster must be dense and ordered");
        }
        if occurrences.iter().enumerate().any(|(i, o)| o.index != i) {
            return refuse("occurrence roster must be dense and ordered");
        }
        let referenced = occurrences
            .iter()
            .map(|o| (o.body, &o.evidence))
            .chain(refusals.iter().map(|r| (r.body, &r.evidence)));
        for (body, evidence) in referenced {
            if body >= bodies.len() {
                return refuse("occurrence body index is outside the document roster");
            }
            if evidence
                .defining_faces
                .iter()
                .chain(&evidence.constituent_faces)
                .any(|&f| f >= faces.len())
            {
                return refuse("occurrence face index is outside the document roster");
            }
        }
        if patterns
            .iter()
            .any(|p| p.members().iter().any(|&m| m >= occurrences.len()))
        {
            return refuse("pattern member is outside the occurrence roster");
        }
        Ok(SectionRecessDocument {
            schema_version,
            reference_scope: "result",
            bodies,
            faces,
            occurrences,
            refusals,
            patterns,
            source_refusals: Vec::new(),
        })
    }

    pub(crate) fn with_source_refusals(
        mut self,
        refusals: Vec<super::discovery::DiscoveryRefusal>,
    ) -> Checked<Self> {
        if refusals
            .iter()
            .flat_map(|r| &r.faces)
            .any(|&f| f >= self.faces.len())
        {
            return refuse("source refusal face index is outside the document roster");
        }
        self.source_refusals = refusals;
        Ok(self)
    }

    pub fn source_refusals(&self) -> &[super::discovery::DiscoveryRefusal] {
        &self.source_refusals
    }

    pub fn occurrences(&self) -> &[SectionRecess] {
        &self.occurrences
    }

    pub fn refusals(&self) -> &[SectionRecessRefusal] {
        &self.refusals
    }

    pub fn patterns(&self) -> &[SectionRecessPattern] {
        &self.patterns
    }
}
