//! B-rep STEP export: a loaded [`Part`] written out as a new AP242 file (step-io's
//! [`StepBuilder`] for the skeleton, products, solids and placements; its authoring layer for the
//! topology and geometry the builder does not take). Design, guarantee and refusals:
//! `docs/brep-writer.md`.
//!
//! This is for output that is ours — a transformed, repaired or otherwise produced part. A file
//! we were given keeps its own bytes: PMI edits to it go through [`crate::pmi::write()`], which
//! changes nothing else.
//!
//! **What is written.** One product in millimetres and radians, with the source file's length
//! uncertainty (else 1e-7 mm), holding every solid of the part as a `MANIFOLD_SOLID_BREP` (a
//! `BREP_WITH_VOIDS` when a solid's faces form several shells: the first is the outer shell, the
//! rest its voids, written as authored) and the faces in no solid in an `OPEN_SHELL` of a
//! `SHELL_BASED_SURFACE_MODEL`. Faces, edges and vertices are written so that the file reads
//! back with the part's numbering. Surfaces: planes, cylinders, cones, spheres and tori as
//! themselves, B-splines exactly, surfaces of revolution and extrusion as the file stated them;
//! curves: lines, circles, ellipses, parabolas and hyperbolas as themselves, every other curve as
//! its exact B-spline; pcurves as the file stated them where its parameters are the written
//! surface's, else those the part holds in full. A placed instance is written in its own frame
//! in a component placed by the file's placement levels; a part moved after reading is written
//! where it lies.
//!
//! **Exactly.** The reader keeps what writing back needs beside what it evaluates (the file's
//! frame and direction ratios, which it normalises; pcurve knots; swept surfaces; placement
//! levels), so reading the written file gives the part bit for bit. Where it keeps nothing (a
//! part moved after reading) directions are written as the nearest ratios that normalise back to
//! the held ones.
//!
//! **What it reports** ([`WriteReport`]): what reading the file back cannot give again —
//! freeform surfaces written as B-splines of another kind, pcurves not written, face and edge
//! order, placements flattened — and the PMI carried and refused.
//!
//! **What it refuses** ([`WriteError::Unsupported`], naming every face and edge): a face whose
//! surface did not resolve, an edge whose curve did not resolve (the part holds its chord), a
//! loop with neither edges nor a vertex.
//!
//! **PMI** ([`WriteOptions::pmi`]): written by [`crate::pmi::write()`] into the new file,
//! anchored to the written faces and edges by index. Items that writer refuses, hole definitions
//! (which it neither writes nor refuses) and items that depend on either are not carried, each
//! named with its reason in [`PmiCarried::refused`].
//!
//! The output is deterministic: the same part and options give the same bytes.

use std::collections::{BTreeMap, HashMap};

use step_io::build::LengthUnit;
use step_io::model as m;
use step_io::{AuthorError, StepBuilder};

use crate::brep::{
    Edge, Face, Part, Pcurve, ProductLabel, RawCurve2d, RawFrame, RawProfile, SweptSource,
};
use crate::geom::{self, Conic, Curve, Frame, Surface, V3};
use crate::nurbs::{NurbsCurve, NurbsSurface};
use crate::pmi::{self, PartPmi};

/// The length uncertainty written (millimetres) for a part read from no file, or from one that
/// states none: OpenCascade's own export value.
pub const UNCERTAINTY_MM: f64 = 1e-7;

/// How to write a part.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct WriteOptions<'a> {
    /// The product's name and id when the part holds none (a part not read from a file, or
    /// read through no product); the source products' names are kept where the part holds them.
    /// Generated fallback product/header names are bounded by
    /// [`pmi::write::MAX_STRING_LITERAL_BYTES`]; copied source labels warn instead.
    pub name: &'a str,
    /// The part's semantic PMI, anchored to its faces and edges by index; `None` writes none.
    pub pmi: Option<&'a PartPmi>,
}

impl<'a> WriteOptions<'a> {
    /// Write geometry under this fallback product name, without semantic PMI.
    pub fn new(name: &'a str) -> Self {
        Self { name, pmi: None }
    }
    pub fn with_pmi(mut self, pmi: Option<&'a PartPmi>) -> Self {
        self.pmi = pmi;
        self
    }
}

/// What a write did, beyond the geometry it wrote exactly.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct WriteReport {
    pub solids: usize,
    /// Copied source labels exceeding the interoperability bound; non-fatal.
    pub long_strings: Vec<pmi::write::LongStringWarning>,
    /// Void shells written (`BREP_WITH_VOIDS`).
    pub void_shells: usize,
    pub faces: usize,
    /// Faces written in the surface model (in no solid).
    pub surface_model_faces: usize,
    pub edges: usize,
    pub vertices: usize,
    pub pcurves_written: usize,
    /// Pcurves the part holds that were not written, each with its face, edge and reason.
    pub pcurves_not_written: Vec<String>,
    /// Freeform faces whose surface was another kind than a B-spline (a surface of revolution,
    /// an extrusion, a Bézier patch…), written as their exact B-spline form: face index and
    /// the kind they were held as. Read back, they are B-splines.
    pub surface_kinds_changed: Vec<(usize, &'static str)>,
    /// Whether reading the file back numbers faces as the part does (solid by solid, then the
    /// faces in no solid).
    pub face_order_preserved: bool,
    /// Whether reading the file back numbers edges as the part does (first use, shell by shell).
    pub edge_order_preserved: bool,
    /// Components written to place instances as the file the part was read from placed them
    /// (one per assembly level of each placed instance).
    pub placed_components: usize,
    /// Whether instances read through a placement were written where the part holds them (the
    /// part was moved after reading, or a placement was not stated in the standard form): the
    /// geometry is then the part's, but reading it back normalises directions the part holds
    /// unnormalised, so it can come back an ulp away.
    pub placements_flattened: bool,
    /// Products written under another name than the source gave them, or source products not
    /// written, each described.
    pub products_renamed: Vec<String>,
    /// The repairs reading made to the part ([`Part::input_repairs`]): the file holds the part
    /// repaired, so reading it back needs and records none.
    pub repairs_written: Vec<String>,
    /// The PMI carried, when [`WriteOptions::pmi`] was given.
    pub pmi: Option<PmiCarried>,
}

/// The PMI a write carried.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct PmiCarried {
    /// What was written (the input less [`Self::refused`]); reading the file back gives its
    /// standard form ([`pmi::write::standard_form`]).
    pub written: PartPmi,
    /// Each item not carried, and why.
    pub refused: Vec<PmiRefusal>,
}

/// One PMI item not carried, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PmiRefusal {
    pub item: pmi::write::ItemRef,
    pub why: String,
}

/// Why nothing was written.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum WriteError {
    /// Geometry or topology the writer cannot write faithfully, each named.
    Unsupported(Vec<String>),
    /// step-io's authoring layer rejected an instance (a writer defect or degenerate input).
    Author(String),
    /// The PMI could not be written at all.
    Pmi(String),
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::Unsupported(v) => write!(f, "cannot write: {}", v.join("; ")),
            WriteError::Author(s) => write!(f, "step-io rejected an instance: {s}"),
            WriteError::Pmi(s) => write!(f, "PMI not written: {s}"),
        }
    }
}

impl std::error::Error for WriteError {}

impl From<AuthorError> for WriteError {
    fn from(e: AuthorError) -> Self {
        WriteError::Author(e.to_string())
    }
}

/// Write *part* as an AP242 STEP file. Returns the file's bytes and what the write did.
///
/// # Errors
/// [`WriteError::Unsupported`] names every face and edge that cannot be written faithfully;
/// [`WriteError::Pmi`] when the PMI is not the part's or cannot be written at all.
pub fn write_step(
    part: &Part,
    options: &WriteOptions<'_>,
) -> Result<(Vec<u8>, WriteReport), WriteError> {
    refuse_unwritable(part)?;
    let mut report = WriteReport {
        repairs_written: part.input_repairs().to_vec(),
        ..WriteReport::default()
    };
    let bytes = Writer::new(part, options.name)?.write(&mut report)?;
    let bytes = match options.pmi {
        Some(p) => {
            let (bytes, carried) = carry_pmi(bytes, p)?;
            report.pmi = Some(carried);
            bytes
        }
        None => bytes,
    };
    report.long_strings = crate::pmi::text::long_strings(&bytes);
    Ok((bytes, report))
}

fn refuse_unwritable(part: &Part) -> Result<(), WriteError> {
    let mut problems = Vec::new();
    for &f in part.unresolved_faces() {
        problems.push(format!("face {f}: its surface did not resolve"));
    }
    for &e in part.unresolved_edges() {
        problems.push(format!(
            "edge {e}: its curve did not resolve (the part holds its chord)"
        ));
    }
    for (f, face) in part.faces.iter().enumerate() {
        if face.loops.is_empty() {
            problems.push(format!("face {f}: no bounds"));
        }
        if let Surface::Other { kind } = &face.surface
            && !part.unresolved_faces().contains(&f)
        {
            problems.push(format!("face {f}: no surface ({kind})"));
        }
        for (l, lp) in face.loops.iter().enumerate() {
            if lp.edges.is_empty() && lp.vertex.is_none() {
                problems.push(format!("face {f} loop {l}: neither edges nor a vertex"));
            }
            for &(e, _) in &lp.edges {
                if e >= part.edges.len() {
                    problems.push(format!("face {f} loop {l}: edge {e} does not exist"));
                }
            }
        }
    }
    for (s, solid) in part.solids.iter().enumerate() {
        if solid.faces.is_empty() {
            problems.push(format!("solid {s}: no faces"));
        }
        if let Some(&f) = solid.faces.iter().find(|&&f| f >= part.faces.len()) {
            problems.push(format!("solid {s}: face {f} does not exist"));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(WriteError::Unsupported(problems))
    }
}

/// The shells written, in order: each solid's shells (outer first), then the surface model's.
struct Shells {
    /// Per solid, its shells' faces.
    solids: Vec<Vec<Vec<usize>>>,
    open: Vec<usize>,
}

/// A solid's faces split into connected shells (faces sharing an edge), the first face's
/// shell first and each shell in the solid's face order.
fn shells_of(part: &Part, faces: &[usize]) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..faces.len()).collect();
    fn find(parent: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while parent[r] != r {
            r = parent[r];
        }
        let mut i = i;
        while parent[i] != r {
            let next = parent[i];
            parent[i] = r;
            i = next;
        }
        r
    }
    let mut first_use: HashMap<usize, usize> = HashMap::new();
    for (i, &f) in faces.iter().enumerate() {
        for lp in &part.faces[f].loops {
            for &(e, _) in &lp.edges {
                match first_use.get(&e) {
                    Some(&j) => {
                        let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                        if a != b {
                            parent[a.max(b)] = a.min(b);
                        }
                    }
                    None => {
                        first_use.insert(e, i);
                    }
                }
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, &f) in faces.iter().enumerate() {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(f);
    }
    groups.into_values().collect()
}

struct Writer<'p> {
    part: &'p Part,
    b: StepBuilder,
    name: String,
    ctx2d: Option<m::ComplexEntityId>,
    /// Whether the geometry is written as read (each instance's native geometry in a component
    /// placed as the file placed it) rather than where the part holds it.
    native: bool,
    /// Per freeform face, whether its surface is written with the file's own parameterisation,
    /// so the file's pcurves on it apply.
    file_parameters: Vec<bool>,
    /// The length uncertainty written, in millimetres.
    uncertainty: f64,
}

/// *x* moved by *k* units in the last place (towards +∞ for positive *k*).
fn ulps(x: f64, k: i32) -> f64 {
    let mut y = x;
    for _ in 0..k.unsigned_abs() {
        y = if k > 0 { y.next_up() } else { y.next_down() };
    }
    y
}

/// The values to write so that reading them back gives exactly *want*: the reader normalises
/// the directions it reads (and rescales some parameters), so a held value written as it is
/// can come back an ulp away. Candidates are *want* and *want* divided by scales a few ulps
/// from 1 (a normalisation multiplies by a reciprocal norm near 1), each moved up to three
/// ulps per component; the first whose reading is *want* is taken, nearest first, and *want*
/// itself when none is.
fn preimage<const N: usize>(
    want: [f64; N],
    read: impl Fn([f64; N]) -> Option<[f64; N]>,
) -> [f64; N] {
    if read(want) == Some(want) {
        return want;
    }
    let near = |k: i32| [0, k, -k];
    let steps: Vec<i32> = std::iter::once(0)
        .chain((1..=3).flat_map(|k| [k, -k]))
        .collect();
    for scale in std::iter::once(0).chain((1..=4).flat_map(|k| near(k).into_iter().skip(1))) {
        let s = ulps(1.0, scale);
        let base: [f64; N] = std::array::from_fn(|c| want[c] / s);
        let mut best: Option<(i32, [f64; N])> = None;
        let mut index = [0usize; N];
        loop {
            let distance: i32 = index.iter().map(|&i| steps[i].abs()).sum();
            if best.is_none_or(|(d, _)| distance < d) {
                let candidate: [f64; N] = std::array::from_fn(|c| ulps(base[c], steps[index[c]]));
                if read(candidate) == Some(want) {
                    best = Some((distance, candidate));
                }
            }
            let mut c = 0;
            while c < N {
                index[c] += 1;
                if index[c] < steps.len() {
                    break;
                }
                index[c] = 0;
                c += 1;
            }
            if c == N {
                break;
            }
        }
        if let Some((_, v)) = best {
            return v;
        }
    }
    want
}

/// A frame's axis and reference direction, as `AXIS2_PLACEMENT_3D` ratios the reader turns
/// back into exactly this frame's z and x (it normalises the axis and the reference direction
/// and removes the reference direction's component along the axis).
fn frame_ratios(f: &Frame) -> (V3, V3) {
    let axis = preimage(f.z, geom::unit);
    let z = geom::unit(axis).unwrap_or(f.z);
    let reference = preimage(f.x, |r| {
        let r = geom::unit(r)?;
        geom::unit(geom::sub(r, geom::scale(z, geom::dot(r, z))))
    });
    (axis, reference)
}

/// Write a frame as an `AXIS2_PLACEMENT_3D`. *raw* is the file's own ratios for it, with the
/// placement from that file's frame to the part's: in the file's frame they are written as they
/// were (absent ones left out), which reads back as exactly this frame; otherwise the ratios
/// are found by [`frame_ratios`].
fn frame_of(
    b: &mut StepBuilder,
    f: &Frame,
    raw: Option<(&RawFrame, crate::step::Placement)>,
) -> Result<m::Axis2Placement3dId, AuthorError> {
    let (axis, reference) = match raw {
        Some((raw, at)) if at == crate::step::IDENTITY => (raw.axis, raw.reference),
        _ => {
            let (axis, reference) = frame_ratios(f);
            (Some(axis), Some(reference))
        }
    };
    let a = b.author();
    let location = a.add_cartesian_point("", f.origin.into())?;
    let axis = match axis {
        Some(d) => Some(m::DirectionRef::Direction(a.add_direction("", d.into())?)),
        None => None,
    };
    let ref_direction = match reference {
        Some(d) => Some(m::DirectionRef::Direction(a.add_direction("", d.into())?)),
        None => None,
    };
    a.add_axis2_placement3d(
        "",
        m::CartesianPointRef::CartesianPoint(location),
        axis,
        ref_direction,
    )
}

/// Expanded knots as STEP's distinct knots and multiplicities (exact equality groups).
fn compress_knots(expanded: &[f64]) -> (Vec<f64>, Vec<i64>) {
    let mut knots: Vec<f64> = Vec::new();
    let mut multiplicities: Vec<i64> = Vec::new();
    for &k in expanded {
        if let (Some(&last), Some(m)) = (knots.last(), multiplicities.last_mut())
            && last == k
        {
            *m += 1;
        } else {
            knots.push(k);
            multiplicities.push(1);
        }
    }
    (knots, multiplicities)
}

fn degree(d: usize) -> i64 {
    i64::try_from(d).unwrap_or(i64::MAX)
}

/// A (rational) B-spline curve over the given control points: plain when every weight is 1,
/// the customary complex instance otherwise.
fn bspline_curve(
    b: &mut StepBuilder,
    d: usize,
    points: Vec<m::CartesianPointRef>,
    knots: Vec<f64>,
    multiplicities: Vec<i64>,
    weights: Option<&[f64]>,
) -> Result<m::CurveRef, AuthorError> {
    let a = b.author();
    match weights.filter(|w| w.iter().any(|&w| w != 1.0)) {
        None => Ok(m::CurveRef::BSplineCurveWithKnots(
            a.add_b_spline_curve_with_knots(
                "",
                degree(d),
                points,
                m::BSplineCurveForm::Unspecified,
                m::Logical::Unknown,
                m::Logical::Unknown,
                multiplicities,
                knots,
                m::KnotType::Unspecified,
            )?,
        )),
        Some(weights) => {
            let no_name = a.text("");
            Ok(m::CurveRef::Complex(a.add_complex(vec![
                m::PartialEntity::BoundedCurve,
                m::PartialEntity::BSplineCurve {
                    degree: degree(d),
                    control_points_list: points,
                    curve_form: m::BSplineCurveForm::Unspecified,
                    closed_curve: m::Logical::Unknown,
                    self_intersect: m::Logical::Unknown,
                },
                m::PartialEntity::BSplineCurveWithKnots {
                    knot_multiplicities: multiplicities,
                    knots,
                    knot_spec: m::KnotType::Unspecified,
                },
                m::PartialEntity::Curve,
                m::PartialEntity::GeometricRepresentationItem,
                m::PartialEntity::RationalBSplineCurve {
                    weights_data: weights.to_vec(),
                },
                m::PartialEntity::RepresentationItem { name: no_name },
            ])?))
        }
    }
}

fn points(b: &mut StepBuilder, ps: &[V3]) -> Result<Vec<m::CartesianPointRef>, AuthorError> {
    ps.iter()
        .map(|p| {
            b.author()
                .add_cartesian_point("", (*p).into())
                .map(m::CartesianPointRef::CartesianPoint)
        })
        .collect()
}

fn nurbs_curve(b: &mut StepBuilder, c: &NurbsCurve) -> Result<m::CurveRef, AuthorError> {
    let cps = points(b, c.control_points())?;
    let (knots, multiplicities) = compress_knots(c.knots());
    bspline_curve(b, c.degree(), cps, knots, multiplicities, Some(c.weights()))
}

fn nurbs_surface(b: &mut StepBuilder, s: &NurbsSurface) -> Result<m::SurfaceRef, AuthorError> {
    let mut cps = Vec::with_capacity(s.control_points.len());
    for row in &s.control_points {
        cps.push(points(b, row)?);
    }
    let (u_knots, u_multiplicities) = compress_knots(&s.knots_u);
    let (v_knots, v_multiplicities) = compress_knots(&s.knots_v);
    let a = b.author();
    if s.weights.iter().flatten().all(|&w| w == 1.0) {
        return Ok(m::SurfaceRef::BSplineSurfaceWithKnots(
            a.add_b_spline_surface_with_knots(
                "",
                degree(s.degree_u),
                degree(s.degree_v),
                cps,
                m::BSplineSurfaceForm::Unspecified,
                m::Logical::Unknown,
                m::Logical::Unknown,
                m::Logical::Unknown,
                u_multiplicities,
                v_multiplicities,
                u_knots,
                v_knots,
                m::KnotType::Unspecified,
            )?,
        ));
    }
    let no_name = a.text("");
    Ok(m::SurfaceRef::Complex(a.add_complex(vec![
        m::PartialEntity::BoundedSurface,
        m::PartialEntity::BSplineSurface {
            u_degree: degree(s.degree_u),
            v_degree: degree(s.degree_v),
            control_points_list: cps,
            surface_form: m::BSplineSurfaceForm::Unspecified,
            u_closed: m::Logical::Unknown,
            v_closed: m::Logical::Unknown,
            self_intersect: m::Logical::Unknown,
        },
        m::PartialEntity::BSplineSurfaceWithKnots {
            u_multiplicities,
            v_multiplicities,
            u_knots,
            v_knots,
            knot_spec: m::KnotType::Unspecified,
        },
        m::PartialEntity::GeometricRepresentationItem,
        m::PartialEntity::RationalBSplineSurface {
            weights_data: s.weights.clone(),
        },
        m::PartialEntity::RepresentationItem { name: no_name },
        m::PartialEntity::Surface,
    ])?))
}

fn raw_axis2(
    b: &mut StepBuilder,
    origin: V3,
    frame: &RawFrame,
) -> Result<m::Axis2Placement3dId, AuthorError> {
    let a = b.author();
    let location = a.add_cartesian_point("", origin.into())?;
    let axis = match frame.axis {
        Some(d) => Some(m::DirectionRef::Direction(a.add_direction("", d.into())?)),
        None => None,
    };
    let reference = match frame.reference {
        Some(d) => Some(m::DirectionRef::Direction(a.add_direction("", d.into())?)),
        None => None,
    };
    a.add_axis2_placement3d(
        "",
        m::CartesianPointRef::CartesianPoint(location),
        axis,
        reference,
    )
}

fn raw_vector(b: &mut StepBuilder, d: V3, magnitude: f64) -> Result<m::VectorId, AuthorError> {
    let a = b.author();
    let d = a.add_direction("", d.into())?;
    a.add_vector("", m::DirectionRef::Direction(d), magnitude)
}

/// A swept surface's profile, as the file stated it.
fn raw_profile(b: &mut StepBuilder, p: &RawProfile) -> Result<m::CurveRef, AuthorError> {
    Ok(match p {
        RawProfile::Line {
            point,
            direction,
            magnitude,
        } => {
            let v = raw_vector(b, *direction, *magnitude)?;
            let a = b.author();
            let pnt = a.add_cartesian_point("", (*point).into())?;
            m::CurveRef::Line(a.add_line(
                "",
                m::CartesianPointRef::CartesianPoint(pnt),
                m::VectorRef::Vector(v),
            )?)
        }
        RawProfile::Circle {
            origin,
            frame,
            radius,
        } => {
            let p = raw_axis2(b, *origin, frame)?;
            m::CurveRef::Circle(b.author().add_circle(
                "",
                m::Axis2PlacementRef::Axis2Placement3d(p),
                *radius,
            )?)
        }
        RawProfile::Ellipse {
            origin,
            frame,
            semi_axis_1,
            semi_axis_2,
        } => {
            let p = raw_axis2(b, *origin, frame)?;
            m::CurveRef::Ellipse(b.author().add_ellipse(
                "",
                m::Axis2PlacementRef::Axis2Placement3d(p),
                *semi_axis_1,
                *semi_axis_2,
            )?)
        }
        RawProfile::BSpline {
            degree,
            points: ps,
            multiplicities,
            knots,
            weights,
        } => {
            let cps = points(b, ps)?;
            bspline_curve(
                b,
                usize::try_from(*degree).unwrap_or(usize::MAX),
                cps,
                knots.clone(),
                multiplicities.clone(),
                weights.as_deref(),
            )?
        }
    })
}

/// A surface of revolution or linear extrusion, as the file stated it.
fn swept_surface(b: &mut StepBuilder, s: &SweptSource) -> Result<m::SurfaceRef, AuthorError> {
    Ok(match s {
        SweptSource::Revolution {
            profile,
            location,
            axis,
            ..
        } => {
            let curve = raw_profile(b, profile)?;
            let a = b.author();
            let location = a.add_cartesian_point("", (*location).into())?;
            let axis = match axis {
                Some(d) => Some(m::DirectionRef::Direction(
                    a.add_direction("", (*d).into())?,
                )),
                None => None,
            };
            let position =
                a.add_axis1_placement("", m::CartesianPointRef::CartesianPoint(location), axis)?;
            m::SurfaceRef::SurfaceOfRevolution(a.add_surface_of_revolution(
                "",
                curve,
                m::Axis1PlacementRef::Axis1Placement(position),
            )?)
        }
        SweptSource::Extrusion {
            profile,
            direction,
            magnitude,
            ..
        } => {
            let curve = raw_profile(b, profile)?;
            let v = raw_vector(b, *direction, *magnitude)?;
            m::SurfaceRef::SurfaceOfLinearExtrusion(b.author().add_surface_of_linear_extrusion(
                "",
                curve,
                m::VectorRef::Vector(v),
            )?)
        }
    })
}

/// A swept surface's definition moved by a placement: points and directions alike, frames
/// with the axes the reader would take (a missing axis or reference direction made explicit).
fn place_swept(at: &crate::step::Placement, s: &SweptSource) -> SweptSource {
    let point = |p: V3| {
        let r = rotate(at, p);
        [0, 1, 2].map(|i| r[i] + at[i][3])
    };
    let frame = |f: &RawFrame| {
        let z = f.axis.unwrap_or([0.0, 0.0, 1.0]);
        let x = f
            .reference
            .unwrap_or_else(|| geom::default_x_axis(geom::unit(z).unwrap_or([0.0, 0.0, 1.0])));
        RawFrame {
            axis: Some(rotate(at, z)),
            reference: Some(rotate(at, x)),
        }
    };
    let profile = |p: &RawProfile| match p {
        RawProfile::Line {
            point: o,
            direction,
            magnitude,
        } => RawProfile::Line {
            point: point(*o),
            direction: rotate(at, *direction),
            magnitude: *magnitude,
        },
        RawProfile::Circle {
            origin,
            frame: f,
            radius,
        } => RawProfile::Circle {
            origin: point(*origin),
            frame: frame(f),
            radius: *radius,
        },
        RawProfile::Ellipse {
            origin,
            frame: f,
            semi_axis_1,
            semi_axis_2,
        } => RawProfile::Ellipse {
            origin: point(*origin),
            frame: frame(f),
            semi_axis_1: *semi_axis_1,
            semi_axis_2: *semi_axis_2,
        },
        RawProfile::BSpline {
            degree,
            points,
            multiplicities,
            knots,
            weights,
        } => RawProfile::BSpline {
            degree: *degree,
            points: points.iter().map(|&p| point(p)).collect(),
            multiplicities: multiplicities.clone(),
            knots: knots.clone(),
            weights: weights.clone(),
        },
    };
    match s {
        SweptSource::Revolution {
            profile: p,
            location,
            axis,
            to_mm,
            to_rad,
        } => SweptSource::Revolution {
            profile: profile(p),
            location: point(*location),
            axis: Some(rotate(at, axis.unwrap_or([0.0, 0.0, 1.0]))),
            to_mm: *to_mm,
            to_rad: *to_rad,
        },
        SweptSource::Extrusion {
            profile: p,
            direction,
            magnitude,
            to_mm,
        } => SweptSource::Extrusion {
            profile: profile(p),
            direction: rotate(at, *direction),
            magnitude: *magnitude,
            to_mm: *to_mm,
        },
    }
}

/// A point or direction moved by a placement's rotation (and translation, for a point).
fn rotate(at: &crate::step::Placement, d: V3) -> V3 {
    at.map(|r| r[0] * d[0] + r[1] * d[1] + r[2] * d[2])
}

fn place_frame(at: &crate::step::Placement, f: &Frame) -> Frame {
    let o = rotate(at, f.origin);
    let (x, z) = (rotate(at, f.x), rotate(at, f.z));
    Frame {
        origin: [0, 1, 2].map(|i| o[i] + at[i][3]),
        x,
        y: geom::cross(z, x),
        z,
    }
}

impl<'p> Writer<'p> {
    fn new(part: &'p Part, name: &str) -> Result<Self, WriteError> {
        // The file's own uncertainty where the part keeps one: readers that heal to it
        // (OpenCascade) then build the same shape.
        let uncertainty = part
            .file_uncertainty()
            .filter(|u| u.is_finite() && *u > 0.0)
            .unwrap_or(UNCERTAINTY_MM);
        let mut b = StepBuilder::new_with(&step_io::build::UnitsInput {
            length: LengthUnit::Millimetre,
            uncertainty,
        })?;
        let header = b.header_mut();
        // Reproducible output: no time stamp.
        header.time_stamp = String::new();
        // The file is named as its root product is.
        header.file_name = (0..part.faces.len())
            .find_map(|f| part.face_products(f).first())
            .map_or(name, |l| l.name.as_str())
            .to_owned();
        header.originating_system = concat!("haecceity ", env!("CARGO_PKG_VERSION")).to_owned();
        // Native when every face's instance is placed by levels the file stated in the
        // standard form (or not placed at all).
        let native = (0..part.faces.len()).all(|f| {
            part.face_instance(f)
                .is_some_and(|(_, levels, _)| levels.iter().all(Option::is_some))
        });
        Ok(Writer {
            part,
            b,
            name: name.to_owned(),
            ctx2d: None,
            native,
            file_parameters: vec![false; part.faces.len()],
            uncertainty,
        })
    }

    /// The face as written: its native geometry, or where the part holds it.
    fn face(&self, f: usize) -> &'p Face {
        let part = self.part;
        // Native mode is chosen only when every face has its instance.
        match part.face_instance(f).filter(|_| self.native) {
            Some((face, ..)) => face,
            None => &part.faces[f],
        }
    }

    fn edge(&self, e: usize) -> &'p Edge {
        let part = self.part;
        if self.native {
            part.native_edge(e)
        } else {
            &part.edges[e]
        }
    }

    /// The placement from the file a face's geometry was read in to the geometry written.
    fn face_at(&self, f: usize) -> crate::step::Placement {
        if self.native {
            crate::step::IDENTITY
        } else {
            self.part.face_placement(f)
        }
    }

    fn edge_at(&self, e: usize) -> crate::step::Placement {
        if self.native {
            crate::step::IDENTITY
        } else {
            self.part.edge_placement(e)
        }
    }

    /// The parameter-space context pcurves are defined in, made on first use.
    fn ctx2d(&mut self) -> Result<m::ComplexEntityId, AuthorError> {
        if let Some(c) = self.ctx2d {
            return Ok(c);
        }
        let a = self.b.author();
        let (identifier, kind) = (a.text("2D SPACE"), a.text(""));
        let c = a.add_complex(vec![
            m::PartialEntity::GeometricRepresentationContext {
                coordinate_space_dimension: 2,
            },
            m::PartialEntity::ParametricRepresentationContext,
            m::PartialEntity::RepresentationContext {
                context_identifier: identifier,
                context_type: kind,
            },
        ])?;
        self.ctx2d = Some(c);
        Ok(c)
    }

    fn surface(
        &mut self,
        face: usize,
        report: &mut WriteReport,
    ) -> Result<m::SurfaceRef, WriteError> {
        let at = self.face_at(face);
        let raw = self.part.face_frame(face).map(|r| (r, at));
        let held = self.face(face);
        let b = &mut self.b;
        Ok(match &held.surface {
            Surface::Plane { frame } => {
                let p = frame_of(b, frame, raw)?;
                m::SurfaceRef::Plane(
                    b.author()
                        .add_plane("", m::Axis2Placement3dRef::Axis2Placement3d(p))?,
                )
            }
            Surface::Cylinder { frame, radius } => {
                let p = frame_of(b, frame, raw)?;
                m::SurfaceRef::CylindricalSurface(b.author().add_cylindrical_surface(
                    "",
                    m::Axis2Placement3dRef::Axis2Placement3d(p),
                    *radius,
                )?)
            }
            Surface::Cone {
                frame,
                radius,
                semi_angle,
            } => {
                let p = frame_of(b, frame, raw)?;
                m::SurfaceRef::ConicalSurface(b.author().add_conical_surface(
                    "",
                    m::Axis2Placement3dRef::Axis2Placement3d(p),
                    *radius,
                    *semi_angle,
                )?)
            }
            Surface::Sphere { frame, radius } => {
                let p = frame_of(b, frame, raw)?;
                m::SurfaceRef::SphericalSurface(b.author().add_spherical_surface(
                    "",
                    m::Axis2Placement3dRef::Axis2Placement3d(p),
                    *radius,
                )?)
            }
            Surface::Torus {
                frame,
                major,
                minor,
            } => {
                let p = frame_of(b, frame, raw)?;
                m::SurfaceRef::ToroidalSurface(b.author().add_toroidal_surface(
                    "",
                    m::Axis2Placement3dRef::Axis2Placement3d(p),
                    *major,
                    *minor,
                )?)
            }
            Surface::Freeform { kind, surface } => {
                // A swept surface the file stated in millimetres is written as the file
                // stated it, which converts back to the same NURBS.
                // Moved, it is the file's definition moved alike (its NURBS then within
                // round-off of the part's), so it keeps its kind and parameters.
                let swept = self
                    .part
                    .face_swept(face)
                    .filter(|s| s.to_mm() == 1.0)
                    .map(|s| {
                        if at == crate::step::IDENTITY {
                            s.clone()
                        } else {
                            place_swept(&at, s)
                        }
                    });
                match &swept {
                    Some(source) => {
                        // A revolution's u is an angle in the file's unit.
                        self.file_parameters[face] = match source {
                            SweptSource::Revolution { to_rad, .. } => *to_rad == 1.0,
                            SweptSource::Extrusion { .. } => true,
                        };
                        swept_surface(b, source)?
                    }
                    None => {
                        if *kind != "BSPLINE" {
                            report.surface_kinds_changed.push((face, kind));
                        }
                        // The B-spline as read keeps the file's knots, so its parameters; a
                        // Bézier patch's are its standard ones.
                        self.file_parameters[face] = matches!(*kind, "BSPLINE" | "BEZIER");
                        nurbs_surface(b, surface)?
                    }
                }
            }
            Surface::Other { kind } => {
                return Err(WriteError::Unsupported(vec![format!(
                    "face {face}: no surface ({kind})"
                )]));
            }
        })
    }

    /// The edge's 3D curve.
    fn curve(&mut self, edge: usize) -> Result<m::CurveRef, WriteError> {
        let part = self.part;
        let e = self.edge(edge);
        let at = self.edge_at(edge);
        let raw = part.edge_frame(edge).map(|r| (r, at));
        let placed = |f: &Frame| {
            if at == crate::step::IDENTITY {
                *f
            } else {
                place_frame(&at, f)
            }
        };
        let b = &mut self.b;
        if let Some(conic) = part.edge_conic(edge) {
            return Ok(match conic {
                Conic::Hyperbola(h) => {
                    let p = frame_of(b, &placed(&h.frame), raw)?;
                    m::CurveRef::Hyperbola(b.author().add_hyperbola(
                        "",
                        m::Axis2PlacementRef::Axis2Placement3d(p),
                        h.major,
                        h.minor,
                    )?)
                }
                Conic::Parabola { frame, focal_dist } => {
                    let p = frame_of(b, &placed(frame), raw)?;
                    m::CurveRef::Parabola(b.author().add_parabola(
                        "",
                        m::Axis2PlacementRef::Axis2Placement3d(p),
                        *focal_dist,
                    )?)
                }
            });
        }
        Ok(match &e.curve {
            Curve::Line { origin, dir } => {
                // The file's direction ratios moved as the reader moves them, so reading them
                // back normalises the same numbers.
                let ratios = part.edge_line_ratios(edge).map_or_else(
                    || preimage(*dir, geom::unit),
                    |r| {
                        if at == crate::step::IDENTITY {
                            r
                        } else {
                            rotate(&at, r)
                        }
                    },
                );
                let a = b.author();
                let pnt = a.add_cartesian_point("", (*origin).into())?;
                let d = a.add_direction("", ratios.into())?;
                let v = a.add_vector("", m::DirectionRef::Direction(d), 1.0)?;
                m::CurveRef::Line(a.add_line(
                    "",
                    m::CartesianPointRef::CartesianPoint(pnt),
                    m::VectorRef::Vector(v),
                )?)
            }
            Curve::Circle { frame, radius } => {
                let p = frame_of(b, frame, raw)?;
                m::CurveRef::Circle(b.author().add_circle(
                    "",
                    m::Axis2PlacementRef::Axis2Placement3d(p),
                    *radius,
                )?)
            }
            Curve::Ellipse {
                frame,
                major,
                minor,
            } => {
                let p = frame_of(b, frame, raw)?;
                m::CurveRef::Ellipse(b.author().add_ellipse(
                    "",
                    m::Axis2PlacementRef::Axis2Placement3d(p),
                    *major,
                    *minor,
                )?)
            }
            Curve::Nurbs(c) => nurbs_curve(b, c)?,
        })
    }

    /// A pcurve of *face* (entry *slot* of its `pcurves`, or of the file's pcurves on it when
    /// *raw*) on the face's written surface, in the file's parameter units; `None`
    /// with the reason when the part does not hold it in full.
    fn pcurve(
        &mut self,
        face: usize,
        slot: usize,
        raw: bool,
        surface: &m::SurfaceRef,
    ) -> Result<Result<m::PcurveId, String>, AuthorError> {
        let part = self.part;
        if raw {
            let item = match &part.file_pcurves(face)[slot].1 {
                RawCurve2d::Line {
                    point,
                    direction,
                    magnitude,
                } => {
                    let a = self.b.author();
                    let p = a.add_cartesian_point("", (*point).into())?;
                    let d = a.add_direction("", (*direction).into())?;
                    let v = a.add_vector("", m::DirectionRef::Direction(d), *magnitude)?;
                    m::RepresentationItemRef::Line(a.add_line(
                        "",
                        m::CartesianPointRef::CartesianPoint(p),
                        m::VectorRef::Vector(v),
                    )?)
                }
                RawCurve2d::BSpline {
                    degree,
                    points,
                    multiplicities,
                    knots,
                    weights,
                } => {
                    let a = self.b.author();
                    let cps = points
                        .iter()
                        .map(|&p| {
                            a.add_cartesian_point("", p.into())
                                .map(m::CartesianPointRef::CartesianPoint)
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    match bspline_curve(
                        &mut self.b,
                        usize::try_from(*degree).unwrap_or(usize::MAX),
                        cps,
                        knots.clone(),
                        multiplicities.clone(),
                        weights.as_deref(),
                    )? {
                        m::CurveRef::BSplineCurveWithKnots(i) => {
                            m::RepresentationItemRef::BSplineCurveWithKnots(i)
                        }
                        m::CurveRef::Complex(i) => m::RepresentationItemRef::Complex(i),
                        #[allow(
                            clippy::unreachable,
                            reason = "bspline_curve returns only a B-spline curve or its rational complex"
                        )]
                        _ => unreachable!("bspline_curve makes a B-spline"),
                    }
                }
            };
            return self.pcurve_on(item, surface).map(Ok);
        }
        let f = &part.faces[face];
        // The reader scales STEP's parameters to the held ones; this file's are millimetres
        // and radians, so only a cone's v (STEP's axial height, held along the slant) differs.
        let sv = match &f.surface {
            Surface::Cone { semi_angle, .. } => 1.0 / semi_angle.cos(),
            _ => 1.0,
        };
        let unscale = |(u, v): (f64, f64)| [u, preimage([v / sv], |[w]| Some([w * sv]))[0]];
        let item = match &f.pcurves[slot].1 {
            Pcurve::Line { point, dir } => {
                let a = self.b.author();
                let p = a.add_cartesian_point("", unscale(*point).into())?;
                let d = a.add_direction("", unscale(*dir).into())?;
                let v = a.add_vector("", m::DirectionRef::Direction(d), 1.0)?;
                m::RepresentationItemRef::Line(a.add_line(
                    "",
                    m::CartesianPointRef::CartesianPoint(p),
                    m::VectorRef::Vector(v),
                )?)
            }
            Pcurve::Poles(poles) => {
                let Some(spline) = part.pcurve_spline(face, slot) else {
                    return Ok(Err("a B-spline pcurve held without its knots".to_owned()));
                };
                let a = self.b.author();
                let cps = poles
                    .iter()
                    .map(|&p| {
                        a.add_cartesian_point("", unscale(p).into())
                            .map(m::CartesianPointRef::CartesianPoint)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let multiplicities = spline
                    .multiplicities
                    .iter()
                    .map(|&k| i64::try_from(k).unwrap_or(i64::MAX))
                    .collect();
                match bspline_curve(
                    &mut self.b,
                    spline.degree,
                    cps,
                    spline.knots.clone(),
                    multiplicities,
                    spline.weights.as_deref(),
                )? {
                    m::CurveRef::BSplineCurveWithKnots(i) => {
                        m::RepresentationItemRef::BSplineCurveWithKnots(i)
                    }
                    m::CurveRef::Complex(i) => m::RepresentationItemRef::Complex(i),
                    #[allow(
                        clippy::unreachable,
                        reason = "bspline_curve returns only a B-spline curve or its rational complex"
                    )]
                    _ => unreachable!("bspline_curve makes a B-spline"),
                }
            }
        };
        self.pcurve_on(item, surface).map(Ok)
    }

    /// A `PCURVE` of a 2D curve on a written surface.
    fn pcurve_on(
        &mut self,
        item: m::RepresentationItemRef,
        surface: &m::SurfaceRef,
    ) -> Result<m::PcurveId, AuthorError> {
        let ctx = self.ctx2d()?;
        let a = self.b.author();
        let rep = a.add_definitional_representation(
            "",
            vec![item],
            m::RepresentationContextRef::Complex(ctx),
        )?;
        a.add_pcurve(
            "",
            surface.clone(),
            m::DefinitionalRepresentationRef::DefinitionalRepresentation(rep),
        )
    }

    fn write(mut self, report: &mut WriteReport) -> Result<Vec<u8>, WriteError> {
        let part = self.part;
        let shells = Shells {
            solids: part
                .solids
                .iter()
                .map(|s| shells_of(part, &s.faces))
                .collect(),
            open: (0..part.faces.len())
                .filter(|&f| part.faces[f].solid.is_none())
                .collect(),
        };
        // The order reading the file back gives faces, edges and vertices.
        let face_order: Vec<usize> = shells
            .solids
            .iter()
            .flatten()
            .flatten()
            .chain(&shells.open)
            .copied()
            .collect();
        report.face_order_preserved = face_order.iter().copied().eq(0..part.faces.len());
        let shell_list: Vec<&Vec<usize>> = shells
            .solids
            .iter()
            .flatten()
            .chain(std::iter::once(&shells.open))
            .collect();
        // The reader numbers edges by first use, shell by shell, in the file's loop order, and
        // holds a loop the face uses reversed (`FACE_BOUND.orientation` false) reversed. Each
        // loop is written in whichever direction makes the file's order number its edges as
        // the part does.
        let mut edge_order = Vec::new();
        let mut reversed_bound: HashMap<(usize, usize), bool> = HashMap::new();
        for shell in &shell_list {
            let mut seen = std::collections::HashSet::new();
            for &f in shell.iter() {
                for (l, lp) in part.faces[f].loops.iter().enumerate() {
                    let new_edges = |order: &mut dyn Iterator<Item = usize>| {
                        let mut local = std::collections::HashSet::new();
                        order
                            .filter(|e| !seen.contains(e) && local.insert(*e))
                            .collect::<Vec<_>>()
                    };
                    let forward = new_edges(&mut lp.edges.iter().map(|&(e, _)| e));
                    let backward = new_edges(&mut lp.edges.iter().rev().map(|&(e, _)| e));
                    let next = edge_order.len();
                    let in_order = |v: &[usize]| v.iter().enumerate().all(|(i, &e)| e == next + i);
                    // When numbering cannot tell (no new edges), the reader's order of the
                    // face's pcurves (the file's loop order) does.
                    let pcurve_order: Vec<usize> = {
                        let mut seen_here = std::collections::HashSet::new();
                        part.faces[f]
                            .pcurves
                            .iter()
                            .map(|(e, _)| *e)
                            .filter(|e| lp.edges.iter().any(|(le, _)| le == e))
                            .filter(|e| seen_here.insert(*e))
                            .collect()
                    };
                    let follows = |order: &mut dyn Iterator<Item = usize>| {
                        let order: Vec<usize> = order.collect();
                        let rank = |e: usize| order.iter().position(|&o| o == e);
                        pcurve_order.windows(2).all(|w| rank(w[0]) < rank(w[1]))
                    };
                    let (ahead, behind) = (in_order(&forward), in_order(&backward));
                    let reverse = (!ahead && behind)
                        || (ahead
                            && behind
                            && !follows(&mut lp.edges.iter().map(|&(e, _)| e))
                            && follows(&mut lp.edges.iter().rev().map(|&(e, _)| e)));
                    reversed_bound.insert((f, l), reverse);
                    for e in if reverse { backward } else { forward } {
                        seen.insert(e);
                        edge_order.push(e);
                    }
                }
            }
        }
        report.edge_order_preserved = edge_order.iter().copied().eq(0..part.edges.len());

        // The products: the part itself, and in native mode a chain of components per placed
        // instance, each level placed as the file placed it.
        // The products are named as the source named them where the part holds them.
        let label = |b: &mut StepBuilder, l: &ProductLabel| {
            b.part_with(
                &l.name,
                &step_io::build::PartOptions {
                    id: Some(l.id.clone()),
                    ..Default::default()
                },
            )
        };
        let roots: Vec<&ProductLabel> = {
            let mut roots: Vec<&ProductLabel> = Vec::new();
            for f in 0..part.faces.len() {
                if let Some(r) = part.face_products(f).first()
                    && !roots.contains(&r)
                {
                    roots.push(r);
                }
            }
            roots
        };
        let generated_name = |name: &str| -> Result<(), WriteError> {
            let size = crate::pmi::text::literal_len(name);
            if size > pmi::write::MAX_STRING_LITERAL_BYTES {
                return Err(WriteError::Author(format!(
                    "generated product/header name is an indivisible string of {size} encoded bytes (limit {}); cannot split losslessly",
                    pmi::write::MAX_STRING_LITERAL_BYTES
                )));
            }
            Ok(())
        };
        let root = match roots.first() {
            Some(r) => {
                for other in &roots[1..] {
                    report.products_renamed.push(format!(
                        "root product {:?} written as part of {:?}",
                        other.name, r.name
                    ));
                }
                label(&mut self.b, r)
            }
            None => {
                generated_name(&self.name)?;
                report.products_renamed.push(format!(
                    "the part holds no product name: written as {:?}",
                    self.name
                ));
                self.b.part(&self.name.clone())
            }
        };
        let mut product_of_face = Vec::with_capacity(part.faces.len());
        let mut instances: HashMap<usize, step_io::build::Part> = HashMap::new();
        for f in 0..part.faces.len() {
            let product = match part.face_instance(f).filter(|_| self.native) {
                Some((_, levels, key)) if !levels.is_empty() => match instances.get(&key) {
                    Some(&p) => p,
                    None => {
                        let mut parent = root;
                        let products = part.face_products(f);
                        for (i, level) in levels.iter().flatten().enumerate() {
                            report.placed_components += 1;
                            let child = match products.get(i + 1) {
                                Some(l) => label(&mut self.b, l),
                                None => {
                                    let name = format!(
                                        "{} component {}",
                                        self.name, report.placed_components
                                    );
                                    generated_name(&name)?;
                                    report.products_renamed.push(format!(
                                        "a component the part holds no name for: written as \
                                         {name:?}"
                                    ));
                                    self.b.part(&name)
                                }
                            };
                            self.b.place(
                                parent,
                                child,
                                step_io::build::Frame {
                                    origin: level.origin,
                                    axis: level.axis,
                                    ref_dir: level.reference,
                                },
                            )?;
                            parent = child;
                        }
                        instances.insert(key, parent);
                        parent
                    }
                },
                _ => root,
            };
            product_of_face.push(product);
        }
        report.placements_flattened = !self.native
            && (0..part.faces.len()).any(|f| part.face_placement(f) != crate::step::IDENTITY);
        if !self.native {
            let mut flattened: Vec<&str> = Vec::new();
            for f in 0..part.faces.len() {
                for l in part.face_products(f).iter().skip(1) {
                    if !flattened.contains(&l.name.as_str()) {
                        flattened.push(&l.name);
                    }
                }
            }
            for name in flattened {
                report.products_renamed.push(format!(
                    "component {name:?} not written: its instances are written in the root \
                     product, where the part holds them"
                ));
            }
        }

        // Surfaces first: pcurves name them.
        let mut surfaces = Vec::with_capacity(part.faces.len());
        for f in 0..part.faces.len() {
            surfaces.push(self.surface(f, report)?);
        }

        // Vertices, by the part's vertex identities, in first-use order.
        let mut vertex_ids: BTreeMap<usize, m::VertexPointId> = BTreeMap::new();
        let mut vertex = |b: &mut StepBuilder, id: usize, p: V3| -> Result<_, AuthorError> {
            if let Some(&v) = vertex_ids.get(&id) {
                return Ok(v);
            }
            let a = b.author();
            let pt = a.add_cartesian_point("", p.into())?;
            let v = a.add_vertex_point("", m::PointRef::CartesianPoint(pt))?;
            vertex_ids.insert(id, v);
            Ok(v)
        };

        // Each edge's pcurves: per face using it, its distinct entries (an edge a face uses
        // twice, a seam, has its pcurves listed once per use).
        // (face, slot, whether the slot is of the file's pcurves on a freeform face)
        let mut uses: Vec<Vec<(usize, usize, bool)>> = vec![Vec::new(); part.edges.len()];
        let mut seam: Vec<bool> = vec![false; part.edges.len()];
        for (f, face) in part.faces.iter().enumerate() {
            let mut count: BTreeMap<usize, usize> = BTreeMap::new();
            for lp in &face.loops {
                for &(e, _) in &lp.edges {
                    *count.entry(e).or_default() += 1;
                }
            }
            for (&e, &k) in &count {
                if k > 1 {
                    seam[e] = true;
                }
                // The file's own pcurves where its parameters are this file's: on a freeform
                // surface written with them, on an analytic one when the file was in
                // millimetres and radians (the held ones are those, reduced).
                let analytic = !matches!(face.surface, Surface::Freeform { .. });
                let raw = !part.file_pcurves(f).is_empty()
                    && if analytic {
                        part.file_units() == Some((1.0, 1.0))
                    } else {
                        self.file_parameters[f]
                    };
                // The file's pcurves on this face that are not written (a freeform surface
                // written without the file's parameters, an analytic one from a file in other
                // units, where only the reduced held ones can be): reported.
                if !raw {
                    let file = part
                        .file_pcurves(f)
                        .iter()
                        .filter(|(pe, _)| *pe == e)
                        .count();
                    let held = face.pcurves.iter().filter(|(pe, _)| *pe == e).count();
                    let missing = file.saturating_sub(held) / k;
                    if missing > 0 {
                        report.pcurves_not_written.push(format!(
                            "face {f} edge {e}: {missing} of the file's pcurves (the surface is \
                             not written with the file's parameters, or the file's units are not \
                             millimetres and radians)"
                        ));
                    }
                }
                let edges: Vec<usize> = if raw {
                    part.file_pcurves(f).iter().map(|(pe, _)| *pe).collect()
                } else {
                    face.pcurves.iter().map(|(pe, _)| *pe).collect()
                };
                let slots: Vec<usize> = (0..edges.len()).filter(|&i| edges[i] == e).collect();
                let distinct = if slots.len().is_multiple_of(k) {
                    &slots[..slots.len() / k]
                } else {
                    &slots[..]
                };
                uses[e].extend(distinct.iter().map(|&s| (f, s, raw)));
            }
        }

        let mut edge_ids = Vec::with_capacity(part.edges.len());
        for i in 0..part.edges.len() {
            let e = self.edge(i);
            let start = vertex(&mut self.b, e.vertices.0, e.start)?;
            let end = vertex(&mut self.b, e.vertices.1, e.end)?;
            let curve = self.curve(i)?;
            let mut pcurves = Vec::new();
            let mut master = m::PreferredSurfaceCurveRepresentation::Curve3d;
            for &(f, slot, raw) in &uses[i] {
                if pcurves.len() == 2 {
                    report.pcurves_not_written.push(format!(
                        "face {f} edge {i}: an edge carries at most two pcurves"
                    ));
                    continue;
                }
                match self.pcurve(f, slot, raw, &surfaces[f])? {
                    Ok(p) => {
                        if raw && self.part.recovered_master_pcurve(f, slot) {
                            master = if pcurves.is_empty() {
                                m::PreferredSurfaceCurveRepresentation::PcurveS1
                            } else {
                                m::PreferredSurfaceCurveRepresentation::PcurveS2
                            };
                        }
                        pcurves.push(m::PcurveOrSurfaceRef::Pcurve(p));
                    }
                    Err(why) => report
                        .pcurves_not_written
                        .push(format!("face {f} edge {i}: {why}")),
                }
            }
            report.pcurves_written += pcurves.len();
            let a = self.b.author();
            let geometry = if pcurves.is_empty() {
                curve
            } else if seam[i] && pcurves.len() == 2 {
                m::CurveRef::SeamCurve(a.add_seam_curve("", curve, pcurves, master)?)
            } else {
                m::CurveRef::SurfaceCurve(a.add_surface_curve("", curve, pcurves, master)?)
            };
            edge_ids.push(a.add_edge_curve(
                "",
                m::VertexRef::VertexPoint(start),
                m::VertexRef::VertexPoint(end),
                geometry,
                e.same_sense,
            )?);
        }

        let mut face_ids = Vec::with_capacity(part.faces.len());
        for (f, face) in part.faces.iter().enumerate() {
            let mut bounds = Vec::with_capacity(face.loops.len());
            for (l, lp) in face.loops.iter().enumerate() {
                let reverse = reversed_bound.get(&(f, l)).copied().unwrap_or(false);
                let bound = if lp.edges.is_empty() {
                    let p = self.face(f).loops[l].vertex.ok_or_else(|| {
                        WriteError::Unsupported(vec![format!(
                            "face {f} bound {l} has neither edges nor a vertex"
                        )])
                    })?;
                    let a = self.b.author();
                    let pt = a.add_cartesian_point("", p.into())?;
                    let v = a.add_vertex_point("", m::PointRef::CartesianPoint(pt))?;
                    m::LoopRef::VertexLoop(a.add_vertex_loop("", m::VertexRef::VertexPoint(v))?)
                } else {
                    // A bound used reversed lists its edges the other way, each used the other
                    // way, which is the same loop.
                    let a = self.b.author();
                    let mut list = Vec::with_capacity(lp.edges.len());
                    let mut edges = lp.edges.clone();
                    if reverse {
                        edges.reverse();
                    }
                    for (e, forward) in edges {
                        list.push(m::OrientedEdgeRef::OrientedEdge(a.add_oriented_edge(
                            "",
                            m::EdgeRef::EdgeCurve(edge_ids[e]),
                            forward != reverse,
                        )?));
                    }
                    m::LoopRef::EdgeLoop(a.add_edge_loop("", list)?)
                };
                bounds.push(m::FaceBoundRef::FaceBound(
                    self.b.author().add_face_bound("", bound, !reverse)?,
                ));
            }
            face_ids.push(self.b.author().add_advanced_face(
                "",
                bounds,
                surfaces[f].clone(),
                !face.reversed,
            )?);
        }

        for shells in &shells.solids {
            let product = product_of_face[shells[0][0]];
            let faces = |shell: &Vec<usize>| shell.iter().map(|&f| face_ids[f]).collect();
            let outer = faces(&shells[0]);
            if shells.len() == 1 {
                self.b.solid(product, "", outer)?;
            } else {
                report.void_shells += shells.len() - 1;
                self.b.solid_with_voids(
                    product,
                    "",
                    outer,
                    shells[1..].iter().map(faces).collect(),
                    step_io::build::VoidShellNormals::AwayFromMaterial,
                )?;
            }
        }
        // The faces in no solid, a surface model per product, in the part's order.
        let mut models: Vec<(step_io::build::Part, Vec<usize>)> = Vec::new();
        for &f in &shells.open {
            let product = product_of_face[f];
            match models.iter_mut().find(|(p, _)| *p == product) {
                Some((_, faces)) => faces.push(f),
                None => models.push((product, vec![f])),
            }
        }
        for (product, faces) in &models {
            self.surface_model(*product, faces, &face_ids)?;
        }

        report.solids = part.solids.len();
        report.faces = part.faces.len();
        report.surface_model_faces = shells.open.len();
        report.edges = part.edges.len();
        report.vertices = vertex_ids.len()
            + part
                .faces
                .iter()
                .flat_map(|f| &f.loops)
                .filter(|l| l.edges.is_empty())
                .count();

        let mut out = Vec::new();
        step_io::write(&self.b.finish(), &mut out)
            .map_err(|e| WriteError::Author(format!("writing Part 21: {e}")))?;
        Ok(out)
    }

    /// The faces in no solid, as one open shell of a surface model in its own representation
    /// of the product's shape.
    fn surface_model(
        &mut self,
        product: step_io::build::Part,
        open: &[usize],
        face_ids: &[m::AdvancedFaceId],
    ) -> Result<(), AuthorError> {
        let shape = self.b.part_ids(product).shape;
        let a = self.b.author();
        let shell = a.add_open_shell(
            "",
            open.iter()
                .map(|&f| m::FaceRef::AdvancedFace(face_ids[f]))
                .collect(),
        )?;
        let model = a.add_shell_based_surface_model("", vec![m::ShellRef::OpenShell(shell)])?;
        let ctx = geometric_context(a, self.uncertainty)?;
        let rep = a.add_manifold_surface_shape_representation(
            "",
            vec![m::RepresentationItemRef::ShellBasedSurfaceModel(model)],
            m::RepresentationContextRef::Complex(ctx),
        )?;
        a.add_shape_definition_representation(
            m::RepresentedDefinitionRef::ProductDefinitionShape(shape),
            m::RepresentationRef::ManifoldSurfaceShapeRepresentation(rep),
        )?;
        Ok(())
    }
}

/// A 3D context with the builder's units (millimetre, radian, steradian) and uncertainty.
fn geometric_context(
    a: &mut step_io::Ap242Author,
    uncertainty: f64,
) -> Result<m::ComplexEntityId, AuthorError> {
    let mm = a.add_complex(vec![
        m::PartialEntity::LengthUnit,
        m::PartialEntity::NamedUnit { dimensions: None },
        m::PartialEntity::SiUnit {
            prefix: Some(m::SiPrefix::Milli),
            name: m::SiUnitName::Metre,
        },
    ])?;
    let rad = a.add_complex(vec![
        m::PartialEntity::NamedUnit { dimensions: None },
        m::PartialEntity::PlaneAngleUnit,
        m::PartialEntity::SiUnit {
            prefix: None,
            name: m::SiUnitName::Radian,
        },
    ])?;
    let sr = a.add_complex(vec![
        m::PartialEntity::NamedUnit { dimensions: None },
        m::PartialEntity::SiUnit {
            prefix: None,
            name: m::SiUnitName::Steradian,
        },
        m::PartialEntity::SolidAngleUnit,
    ])?;
    let length_measure = a.text("LENGTH_MEASURE");
    let uncertainty = a.add_uncertainty_measure_with_unit(
        m::MeasureValue {
            type_name: Some(length_measure),
            value: m::MeasureScalar::Real(uncertainty),
        },
        m::UnitRef::Complex(mm),
        "distance_accuracy_value",
        Some("confusion accuracy"),
    )?;
    let (no_identifier, context_type) = (a.text(""), a.text("3D"));
    a.add_complex(vec![
        m::PartialEntity::GeometricRepresentationContext {
            coordinate_space_dimension: 3,
        },
        m::PartialEntity::GlobalUncertaintyAssignedContext {
            uncertainty: vec![
                m::UncertaintyMeasureWithUnitRef::UncertaintyMeasureWithUnit(uncertainty),
            ],
        },
        m::PartialEntity::GlobalUnitAssignedContext {
            units: vec![
                m::UnitRef::Complex(mm),
                m::UnitRef::Complex(rad),
                m::UnitRef::Complex(sr),
            ],
        },
        m::PartialEntity::RepresentationContext {
            context_identifier: no_identifier,
            context_type,
        },
    ])
}

/// Per kind, the index in the caller's PMI of each item still carried.
#[derive(Clone, Debug, Default)]
struct Origins {
    standards: Vec<usize>,
    decimal_places: bool,
    features: Vec<usize>,
    datum_targets: Vec<usize>,
    datums: Vec<usize>,
    dimensions: Vec<usize>,
    tolerances: Vec<usize>,
    tolerance_relations: Vec<usize>,
    general: Vec<usize>,
    threads: Vec<usize>,
    knurls: Vec<usize>,
    material: bool,
    notes: Vec<usize>,
    surface_textures: Vec<usize>,
    attributes: Vec<usize>,
}

impl Origins {
    fn of(p: &PartPmi) -> Self {
        let all = |n: usize| (0..n).collect();
        Origins {
            standards: all(p.standards.len()),
            decimal_places: p.decimal_places.is_some(),
            features: all(p.features.len()),
            datum_targets: all(p.datum_targets.len()),
            datums: all(p.datums.len()),
            dimensions: all(p.dimensions.len()),
            tolerances: all(p.tolerances.len()),
            tolerance_relations: all(p.tolerance_relations.len()),
            general: all(p.general.len()),
            threads: all(p.threads.len()),
            knurls: all(p.knurls.len()),
            material: p.material.is_some(),
            notes: all(p.notes.len()),
            surface_textures: all(p.surface_textures.len()),
            attributes: all(p.attributes.len()),
        }
    }

    /// Every carried item, by its index in the caller's PMI.
    fn items(&self) -> Vec<pmi::write::ItemRef> {
        use pmi::write::ItemRef as I;
        let each = |v: &[usize], f: fn(usize) -> I| v.iter().map(|&i| f(i)).collect::<Vec<_>>();
        let mut out = each(&self.standards, I::Standard);
        if self.decimal_places {
            out.push(I::DecimalPlaces);
        }
        out.extend(each(&self.features, I::Feature));
        out.extend(each(&self.datum_targets, I::DatumTarget));
        out.extend(each(&self.datums, I::Datum));
        out.extend(each(&self.dimensions, I::Dimension));
        out.extend(each(&self.tolerances, I::Tolerance));
        out.extend(each(&self.tolerance_relations, I::ToleranceRelation));
        out.extend(each(&self.general, I::General));
        out.extend(each(&self.threads, I::Thread));
        out.extend(each(&self.knurls, I::Knurl));
        if self.material {
            out.push(I::Material);
        }
        out.extend(each(&self.notes, I::Note));
        out.extend(each(&self.surface_textures, I::SurfaceTexture));
        out.extend(each(&self.attributes, I::Attribute));
        out
    }

    /// The caller's index of an item of the PMI these origins describe.
    fn original(&self, item: pmi::write::ItemRef) -> pmi::write::ItemRef {
        use pmi::write::ItemRef as I;
        match item {
            I::Standard(i) => I::Standard(self.standards[i]),
            I::Feature(i) => I::Feature(self.features[i]),
            I::DatumTarget(i) => I::DatumTarget(self.datum_targets[i]),
            I::Datum(i) => I::Datum(self.datums[i]),
            I::Dimension(i) => I::Dimension(self.dimensions[i]),
            I::Tolerance(i) => I::Tolerance(self.tolerances[i]),
            I::ToleranceRelation(i) => I::ToleranceRelation(self.tolerance_relations[i]),
            I::General(i) => I::General(self.general[i]),
            I::Thread(i) => I::Thread(self.threads[i]),
            I::Knurl(i) => I::Knurl(self.knurls[i]),
            I::Note(i) => I::Note(self.notes[i]),
            I::SurfaceTexture(i) => I::SurfaceTexture(self.surface_textures[i]),
            I::Attribute(i) => I::Attribute(self.attributes[i]),
            other => other,
        }
    }

    /// These origins after [`pmi::without_kept`] kept *kept* of the items they describe.
    fn keep(&self, kept: &pmi::Kept) -> Self {
        let keep = |v: &[usize], k: &[bool]| {
            v.iter()
                .zip(k)
                .filter(|(_, k)| **k)
                .map(|(i, _)| *i)
                .collect()
        };
        Origins {
            standards: keep(&self.standards, &kept.standards),
            decimal_places: self.decimal_places && kept.decimal_places,
            features: keep(&self.features, &kept.features),
            datum_targets: keep(&self.datum_targets, &kept.datum_targets),
            datums: keep(&self.datums, &kept.datums),
            dimensions: keep(&self.dimensions, &kept.dimensions),
            tolerances: keep(&self.tolerances, &kept.tolerances),
            tolerance_relations: keep(&self.tolerance_relations, &kept.tolerance_relations),
            general: keep(&self.general, &kept.general),
            threads: keep(&self.threads, &kept.threads),
            knurls: keep(&self.knurls, &kept.knurls),
            material: self.material && kept.material,
            notes: keep(&self.notes, &kept.notes),
            surface_textures: keep(&self.surface_textures, &kept.surface_textures),
            attributes: keep(&self.attributes, &kept.attributes),
        }
    }
}

/// Write *pmi* into the new file's one part with [`pmi::write()`] (which validates what it
/// adds against the schema and its rules). Hole definitions are not carried (that writer
/// neither writes nor refuses them). Items that writer refuses are taken out, with every item
/// that refers to one of them, and the rest written; each is named with its reason. When it
/// fails otherwise (its schema or rule check of what it would add), no item is carried and
/// each is named with that failure.
fn carry_pmi(bytes: Vec<u8>, pmi: &PartPmi) -> Result<(Vec<u8>, PmiCarried), WriteError> {
    use pmi::write::ItemRef;
    let defs = crate::step::read_part_definitions(&bytes)
        .map_err(|e| WriteError::Pmi(format!("the written file does not read back: {e}")))?;
    if defs.len() != 1 {
        return Err(WriteError::Pmi(format!(
            "the written file has {} parts, not one",
            defs.len()
        )));
    }
    let doc = crate::p21::Document::parse(bytes)
        .map_err(|e| WriteError::Pmi(format!("the written file does not parse: {e}")))?;
    let mut refused: Vec<PmiRefusal> = (0..pmi.holes.len())
        .map(|i| PmiRefusal {
            item: ItemRef::Hole(i),
            why: "a hole definition: the PMI writer reads them only".to_owned(),
        })
        .collect();
    let mut written = pmi.clone();
    written.holes.clear();
    let mut origins = Origins::of(&written);
    loop {
        if written == PartPmi::default() {
            return Ok((doc.bytes().to_vec(), PmiCarried { written, refused }));
        }
        match pmi::write(
            &doc,
            &defs,
            &[(pmi::PartId(0), written.clone())],
            pmi::Mode::Add,
            pmi::write::PresentationPolicy::Refuse,
        ) {
            Ok((edit, _)) => {
                let out = doc
                    .apply(&edit)
                    .map_err(|e| WriteError::Pmi(format!("applying the PMI: {e}")))?;
                refused.sort_by_key(|r| r.item);
                return Ok((out.bytes, PmiCarried { written, refused }));
            }
            Err(pmi::write::WriteError::Refused(list)) => {
                let items: Vec<ItemRef> = list.iter().map(|r| r.item).collect();
                let before: Vec<ItemRef> = origins.items();
                refused.extend(list.into_iter().map(|r| PmiRefusal {
                    item: origins.original(r.item),
                    why: r.why,
                }));
                let (next, kept) = pmi::without_kept(&written, &items);
                origins = origins.keep(&kept);
                let after = origins.items();
                let named: Vec<ItemRef> = refused.iter().map(|r| r.item).collect();
                refused.extend(
                    before
                        .into_iter()
                        .filter(|i| !after.contains(i) && !named.contains(i))
                        .map(|item| PmiRefusal {
                            item,
                            why: "not carried: it refers to a refused item".to_owned(),
                        }),
                );
                if next == written {
                    return Err(WriteError::Pmi(
                        "the PMI writer refused items it was not given".to_owned(),
                    ));
                }
                written = next;
            }
            Err(e) => {
                // Nothing of the part's PMI can be written: every item is named with why.
                let why = format!("not carried: the PMI writer cannot write the part's PMI ({e})");
                refused.extend(origins.items().into_iter().map(|item| PmiRefusal {
                    item,
                    why: why.clone(),
                }));
                refused.sort_by_key(|r| r.item);
                return Ok((
                    doc.bytes().to_vec(),
                    PmiCarried {
                        written: PartPmi::default(),
                        refused,
                    },
                ));
            }
        }
    }
}
