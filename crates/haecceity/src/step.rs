//! STEP import through `step-io`, mapped into [`Part`].
//!
//! Mirrors `quiddity.import_step_geometry`: geometry only, assemblies flattened, lengths in
//! millimetres (OpenCascade's default import unit).

#[cfg(feature = "manifest")]
pub mod manifest;

use std::collections::HashMap;

use step_io::model as m;
use step_io::parser::{Attribute, RawEntity};
use step_io::scene::{Edge as StepEdge, Face as StepFace, SurfaceKind};

use super::brep::{
    Edge, Face, GeometryPlacement, InstanceTransform, Loop, Part, Pcurve, PcurveSpline,
    ProductLabel, RawCurve2d, RawFrame, RawProfile, Solid, Source, SweptSource,
};
use super::geom::{self, Curve, Frame, Surface, V3};
use super::nurbs::{NurbsCurve, NurbsSurface};
use super::sampling::sample_edge;

#[derive(Debug)]
#[non_exhaustive]
pub enum StepError {
    Parse(String),
    Unsupported(String),
    /// The file parses but its shapes are not all there: no solid or shell at all, or
    /// geometry or topology step-io dropped (missing, malformed or of an unknown type).
    Incomplete(String),
    /// The file exceeds a bound the reader keeps so that hostile input cannot exhaust the
    /// stack or memory (a WASM build has little of either): which bound, and by what.
    Limit(String),
}

impl std::fmt::Display for StepError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StepError::Parse(s) => write!(f, "STEP parse error: {s}"),
            StepError::Unsupported(s) => write!(f, "unsupported STEP content: {s}"),
            StepError::Incomplete(s) => write!(f, "incomplete STEP geometry: {s}"),
            StepError::Limit(s) => write!(f, "STEP input exceeds a reader bound: {s}"),
        }
    }
}

impl std::error::Error for StepError {}

/// A reference that names no entity of the model. step-io drops an entity whose reference
/// dangles, with everything that references it, so this is a refusal for a broken invariant
/// rather than for any file.
fn dangling() -> StepError {
    StepError::Incomplete("a reference names no entity of the model".into())
}

/// Look-ups for the paths that cannot return a refusal (iterator filters, `Vec` builders).
trait Resolved {
    fn resolved<I: m::EntityId>(&self, id: I) -> &I::Entity;
}

impl Resolved for m::StepModel {
    /// step-io resolves every typed reference against the model it builds and drops an entity
    /// whose reference dangles, together with its referrers, so an id read out of a model
    /// always names an entity of that model.
    #[allow(
        clippy::expect_used,
        reason = "step-io resolves every reference it keeps"
    )]
    fn resolved<I: m::EntityId>(&self, id: I) -> &I::Entity {
        self.get(id)
            .expect("step-io resolves every reference it keeps")
    }
}

/// Bounds on what one file can make the reader follow or build, so that hostile input is
/// refused with [`StepError::Limit`] instead of exhausting the stack or memory (#52). Each is far
/// above any file of the corpus or the fixtures (`tests/robustness.rs` reads them all).
///
/// The others on the read path: step-io refuses parameters nested deeper than
/// [`step_io::parser::MAX_NESTING_DEPTH`] and B-splines of degree above 25; [`NurbsCurve`] and
/// [`NurbsSurface`] refuse degrees above [`crate::nurbs::MAX_DEGREE`]; a B-spline whose
/// multiplicities exceed its knot count is malformed, and is left unresolved without expanding.
pub mod limits {
    /// Assembly levels below the root (`NEXT_ASSEMBLY_USAGE_OCCURRENCE` nesting).
    pub const ASSEMBLY_DEPTH: usize = 64;
    /// Product definitions placed, and solid instances, across every assembly level: a few
    /// levels of repeated occurrences multiply.
    pub const INSTANCES: usize = 100_000;
    /// Faces read across every placed instance (the corpus's most is 2 157).
    pub const FACES: usize = 500_000;
    /// Edges in one face bound (the corpus's most is 92). Several kernel queries walk a
    /// bound's edges per edge.
    pub const LOOP_EDGES: usize = 10_000;
    /// `SURFACE_CURVE`-like wrappers (surface, seam, bounded surface and intersection curves)
    /// between an edge and its 3D curve; a cyclic chain has no end and is refused here.
    pub const CURVE_WRAPPERS: usize = 16;
}

/// Refuse a model whose curve wrappers nest deeper than [`limits::CURVE_WRAPPERS`] (or cycle):
/// the reader and step-io's scene follow them to the curve.
fn check_curve_wrappers(model: &m::StepModel) -> Result<(), StepError> {
    let inner = |r: &m::CurveRef| -> Option<m::CurveRef> {
        Some(match r {
            m::CurveRef::SurfaceCurve(i) => model.get(*i)?.curve_3d.clone(),
            m::CurveRef::SeamCurve(i) => model.get(*i)?.curve_3d.clone(),
            m::CurveRef::BoundedSurfaceCurve(i) => model.get(*i)?.curve_3d.clone(),
            m::CurveRef::IntersectionCurve(i) => model.get(*i)?.curve_3d.clone(),
            _ => return None,
        })
    };
    let starts = model
        .all::<m::SurfaceCurve>()
        .map(|(_, c)| &c.curve_3d)
        .chain(model.all::<m::SeamCurve>().map(|(_, c)| &c.curve_3d))
        .chain(
            model
                .all::<m::BoundedSurfaceCurve>()
                .map(|(_, c)| &c.curve_3d),
        )
        .chain(
            model
                .all::<m::IntersectionCurve>()
                .map(|(_, c)| &c.curve_3d),
        );
    for start in starts {
        let mut r = start.clone();
        let mut depth = 1;
        while let Some(next) = inner(&r) {
            depth += 1;
            if depth > limits::CURVE_WRAPPERS {
                return Err(StepError::Limit(format!(
                    "surface curves nested more than {} deep (or cyclic)",
                    limits::CURVE_WRAPPERS
                )));
            }
            r = next;
        }
    }
    Ok(())
}

/// A rigid placement, rows of `[R | t]` with `t` in millimetres.
pub type Placement = [[f64; 4]; 3];

pub const IDENTITY: Placement = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

/// Validate a finite, right-handed rigid motion (no scale, shear or reflection).
pub(crate) fn validate_placement(outer: &Placement) -> Result<(), StepError> {
    let r = outer.map(|row| [row[0], row[1], row[2]]);
    let det = r[0][0] * (r[1][1] * r[2][2] - r[1][2] * r[2][1])
        - r[0][1] * (r[1][0] * r[2][2] - r[1][2] * r[2][0])
        + r[0][2] * (r[1][0] * r[2][1] - r[1][1] * r[2][0]);
    if !outer.iter().flatten().all(|v| v.is_finite())
        || (det - 1.0).abs() > 1e-12
        || (0..3).any(|i| {
            (0..3).any(|j| (geom::dot(r[i], r[j]) - if i == j { 1.0 } else { 0.0 }).abs() > 1e-12)
        })
    {
        // Frames are rebuilt right-handed (y = z × x), so a reflection would not be honoured.
        return Err(StepError::Unsupported(
            "outer placement must be a proper rotation".into(),
        ));
    }
    Ok(())
}

pub(crate) fn compose(outer: &Placement, inner: &Placement) -> Placement {
    let mut out = [[0.0; 4]; 3];
    for i in 0..3 {
        for j in 0..4 {
            out[i][j] = (0..3).map(|k| outer[i][k] * inner[k][j]).sum::<f64>()
                + if j == 3 { outer[i][3] } else { 0.0 };
        }
    }
    out
}

struct Reader<'a> {
    model: &'a m::StepModel,
    /// Length unit → millimetres.
    to_mm: f64,
    /// Plane-angle unit → radians.
    to_rad: f64,
    /// The placement of the solid instance being read.
    placement: Placement,
}

impl Reader<'_> {
    fn place_point(&self, p: V3) -> V3 {
        let x = &self.placement;
        [0, 1, 2].map(|i| x[i][0] * p[0] + x[i][1] * p[1] + x[i][2] * p[2] + x[i][3])
    }

    fn place_direction(&self, d: V3) -> V3 {
        let x = &self.placement;
        [0, 1, 2].map(|i| x[i][0] * d[0] + x[i][1] * d[1] + x[i][2] * d[2])
    }

    fn point(&self, r: &m::CartesianPointRef) -> Option<V3> {
        let m::CartesianPointRef::CartesianPoint(i) = r else {
            return None;
        };
        let c = &self.model.get(m::CartesianPointId(i.0))?.coordinates;
        let get = |k: usize| c.get(k).copied().unwrap_or(0.0) * self.to_mm;
        Some(self.place_point([get(0), get(1), get(2)]))
    }

    fn direction(&self, r: &m::DirectionRef) -> Option<V3> {
        let m::DirectionRef::Direction(i) = r else {
            return None;
        };
        let c = &self.model.get(m::DirectionId(i.0))?.direction_ratios;
        let get = |k: usize| c.get(k).copied().unwrap_or(0.0);
        geom::unit(self.place_direction([get(0), get(1), get(2)]))
    }

    /// A file direction before the instance placement is applied.
    fn raw_direction(&self, r: &m::DirectionRef) -> Option<V3> {
        let m::DirectionRef::Direction(i) = r else {
            return None;
        };
        let c = &self.model.get(m::DirectionId(i.0))?.direction_ratios;
        let get = |k: usize| c.get(k).copied().unwrap_or(0.0);
        geom::unit([get(0), get(1), get(2)])
    }

    /// `AXIS2_PLACEMENT_3D` as OpenCascade builds it: a missing or degenerate reference
    /// direction falls back to `gp_Ax2`'s own choice of x axis for the given z.
    fn frame3(&self, id: m::Axis2Placement3dId) -> Option<Frame> {
        let a = self.model.get(m::Axis2Placement3dId(id.0))?;
        let origin = self.point(&a.location)?;
        let z = match &a.axis {
            Some(d) => self.raw_direction(d)?,
            None => [0.0, 0.0, 1.0],
        };
        let x = a
            .ref_direction
            .as_ref()
            .and_then(|d| self.raw_direction(d))
            .and_then(|r| geom::unit(geom::sub(r, geom::scale(z, geom::dot(r, z)))))
            .unwrap_or_else(|| geom::default_x_axis(z));
        let (x, z) = (self.place_direction(x), self.place_direction(z));
        Some(Frame {
            origin,
            x,
            y: geom::cross(z, x),
            z,
        })
    }

    /// An `AXIS2_PLACEMENT_3D`'s direction ratios as the file gives them.
    fn raw_frame(&self, id: m::Axis2Placement3dId) -> RawFrame {
        let a = self.model.resolved(id);
        let ratios = |d: &m::DirectionRef| {
            let m::DirectionRef::Direction(i) = d else {
                return None;
            };
            let c = &self.model.get(m::DirectionId(i.0))?.direction_ratios;
            Some([0, 1, 2].map(|k| c.get(k).copied().unwrap_or(0.0)))
        };
        RawFrame {
            axis: a.axis.as_ref().and_then(ratios),
            reference: a.ref_direction.as_ref().and_then(ratios),
        }
    }

    /// A cartesian point's coordinates as the file gives them.
    fn raw_point(&self, r: &m::CartesianPointRef) -> Option<V3> {
        let m::CartesianPointRef::CartesianPoint(i) = r else {
            return None;
        };
        let c = &self.model.get(m::CartesianPointId(i.0))?.coordinates;
        Some([0, 1, 2].map(|k| c.get(k).copied().unwrap_or(0.0)))
    }

    /// A direction's ratios as the file gives them.
    fn raw_ratios(&self, r: &m::DirectionRef) -> Option<V3> {
        let m::DirectionRef::Direction(i) = r else {
            return None;
        };
        let c = &self.model.get(m::DirectionId(i.0))?.direction_ratios;
        Some([0, 1, 2].map(|k| c.get(k).copied().unwrap_or(0.0)))
    }

    /// A vector's direction ratios and magnitude as the file gives them.
    fn raw_vector(&self, r: &m::VectorRef) -> Option<(V3, f64)> {
        let m::VectorRef::Vector(i) = r else {
            return None;
        };
        let v = self.model.get(m::VectorId(i.0))?;
        Some((self.raw_ratios(&v.orientation)?, v.magnitude))
    }

    /// A swept surface's profile as the file states it (lines, circles, ellipses and
    /// B-splines; `None` for other profiles).
    fn raw_profile(&self, r: &m::CurveRef) -> Option<RawProfile> {
        let model = self.model;
        let placed = |p: &m::Axis2PlacementRef| {
            let m::Axis2PlacementRef::Axis2Placement3d(id) = p else {
                return None;
            };
            let a = model.get(*id)?;
            Some((self.raw_point(&a.location)?, self.raw_frame(*id)))
        };
        Some(match r {
            m::CurveRef::Line(i) => {
                let l = model.get(m::LineId(i.0))?;
                let (direction, magnitude) = self.raw_vector(&l.dir)?;
                RawProfile::Line {
                    point: self.raw_point(&l.pnt)?,
                    direction,
                    magnitude,
                }
            }
            m::CurveRef::Circle(i) => {
                let c = model.get(m::CircleId(i.0))?;
                let (origin, frame) = placed(&c.position)?;
                RawProfile::Circle {
                    origin,
                    frame,
                    radius: c.radius,
                }
            }
            m::CurveRef::Ellipse(i) => {
                let e = model.get(m::EllipseId(i.0))?;
                let (origin, frame) = placed(&e.position)?;
                RawProfile::Ellipse {
                    origin,
                    frame,
                    semi_axis_1: e.semi_axis_1,
                    semi_axis_2: e.semi_axis_2,
                }
            }
            m::CurveRef::BSplineCurveWithKnots(i) => {
                let b = model.get(m::BSplineCurveWithKnotsId(i.0))?;
                RawProfile::BSpline {
                    degree: b.degree,
                    points: b
                        .control_points_list
                        .iter()
                        .map(|p| self.raw_point(p))
                        .collect::<Option<_>>()?,
                    multiplicities: b.knot_multiplicities.clone(),
                    knots: b.knots.clone(),
                    weights: None,
                }
            }
            m::CurveRef::Complex(c) => {
                let parts = &model.get(m::ComplexEntityId(c.0))?.parts;
                let (degree, refs) = parts.iter().find_map(|p| match p {
                    m::PartialEntity::BSplineCurve {
                        degree,
                        control_points_list,
                        ..
                    } => Some((*degree, control_points_list)),
                    _ => None,
                })?;
                let (multiplicities, knots) = parts.iter().find_map(|p| match p {
                    m::PartialEntity::BSplineCurveWithKnots {
                        knot_multiplicities,
                        knots,
                        ..
                    } => Some((knot_multiplicities.clone(), knots.clone())),
                    _ => None,
                })?;
                let weights = parts.iter().find_map(|p| match p {
                    m::PartialEntity::RationalBSplineCurve { weights_data } => {
                        Some(weights_data.clone())
                    }
                    _ => None,
                });
                RawProfile::BSpline {
                    degree,
                    points: refs
                        .iter()
                        .map(|p| self.raw_point(p))
                        .collect::<Option<_>>()?,
                    multiplicities,
                    knots,
                    weights,
                }
            }
            _ => return None,
        })
    }

    /// A surface of revolution or linear extrusion as the file states it.
    fn swept_source(&self, face: &StepFace<'_>) -> Option<SweptSource> {
        match face.surface().kind() {
            SurfaceKind::Revolution(s) => {
                let m::Axis1PlacementRef::Axis1Placement(i) = &s.axis_position;
                let axis = self.model.get(m::Axis1PlacementId(i.0))?;
                Some(SweptSource::Revolution {
                    profile: self.raw_profile(&s.swept_curve)?,
                    location: self.raw_point(&axis.location)?,
                    axis: match &axis.axis {
                        Some(d) => Some(self.raw_ratios(d)?),
                        None => None,
                    },
                    to_mm: self.to_mm,
                    to_rad: self.to_rad,
                })
            }
            SurfaceKind::LinearExtrusion(s) => {
                let (direction, magnitude) = self.raw_vector(&s.extrusion_axis)?;
                Some(SweptSource::Extrusion {
                    profile: self.raw_profile(&s.swept_curve)?,
                    direction,
                    magnitude,
                    to_mm: self.to_mm,
                })
            }
            _ => None,
        }
    }

    /// A 2D line or B-spline as the file states it.
    fn raw_curve_2d(&self, item: &m::RepresentationItemRef) -> Option<RawCurve2d> {
        let model = self.model;
        let point = |r: &m::CartesianPointRef| {
            let c = self.raw_point(r)?;
            Some([c[0], c[1]])
        };
        Some(match item {
            m::RepresentationItemRef::Line(i) => {
                let l = model.get(m::LineId(i.0))?;
                let (d, magnitude) = self.raw_vector(&l.dir)?;
                RawCurve2d::Line {
                    point: point(&l.pnt)?,
                    direction: [d[0], d[1]],
                    magnitude,
                }
            }
            m::RepresentationItemRef::BSplineCurveWithKnots(i) => {
                let b = model.get(m::BSplineCurveWithKnotsId(i.0))?;
                RawCurve2d::BSpline {
                    degree: b.degree,
                    points: b
                        .control_points_list
                        .iter()
                        .map(point)
                        .collect::<Option<_>>()?,
                    multiplicities: b.knot_multiplicities.clone(),
                    knots: b.knots.clone(),
                    weights: None,
                }
            }
            m::RepresentationItemRef::Complex(c) => {
                let RawProfile::BSpline {
                    degree,
                    points,
                    multiplicities,
                    knots,
                    weights,
                } = self.raw_profile(&m::CurveRef::Complex(*c))?
                else {
                    return None;
                };
                RawCurve2d::BSpline {
                    degree,
                    points: points.into_iter().map(|p| [p[0], p[1]]).collect(),
                    multiplicities,
                    knots,
                    weights,
                }
            }
            _ => return None,
        })
    }

    /// The pcurves the file gives on a face's surface, by edge index, as it states them (kept to
    /// be written again).
    fn file_pcurves(
        &self,
        face: &StepFace<'_>,
        edge_index: &HashMap<m::EntityKey, usize>,
        collect_preferred: bool,
    ) -> (Vec<(usize, RawCurve2d)>, Vec<usize>) {
        let basis = face.surface().key();
        let mut out = Vec::new();
        let mut primary = Vec::new();
        for bound in face.bounds() {
            for edge in bound.edges() {
                let Some(&index) = edge_index.get(&edge.key()) else {
                    continue;
                };
                let (associated, master) = match &raw_edge_curve(self.model, &edge) {
                    m::CurveRef::SurfaceCurve(i) => {
                        let curve = self.model.resolved(m::SurfaceCurveId(i.0));
                        (&curve.associated_geometry, curve.master_representation)
                    }
                    m::CurveRef::SeamCurve(i) => {
                        let curve = self.model.resolved(m::SeamCurveId(i.0));
                        (&curve.associated_geometry, curve.master_representation)
                    }
                    _ => continue,
                };
                for (slot, g) in associated.iter().enumerate() {
                    let m::PcurveOrSurfaceRef::Pcurve(p) = g else {
                        continue;
                    };
                    let pcurve = self.model.resolved(m::PcurveId(p.0));
                    if pcurve.basis_surface.entity_key() != basis {
                        continue;
                    }
                    let m::DefinitionalRepresentationRef::DefinitionalRepresentation(d) =
                        &pcurve.reference_to_curve
                    else {
                        continue;
                    };
                    for item in &self
                        .model
                        .resolved(m::DefinitionalRepresentationId(d.0))
                        .items
                    {
                        if let Some(curve) = self.raw_curve_2d(item) {
                            if collect_preferred
                                && matches!(
                                    (master, slot),
                                    (m::PreferredSurfaceCurveRepresentation::PcurveS1, 0)
                                        | (m::PreferredSurfaceCurveRepresentation::PcurveS2, 1)
                                )
                            {
                                primary.push(out.len());
                            }
                            out.push((index, curve));
                        }
                    }
                }
            }
        }
        (out, primary)
    }

    /// The file's frame ratios of an analytic surface.
    fn surface_raw_frame(&self, face: &StepFace<'_>) -> Option<RawFrame> {
        let m::Axis2Placement3dRef::Axis2Placement3d(id) = match face.surface().kind() {
            SurfaceKind::Plane(p) => &p.position,
            SurfaceKind::Cylindrical(c) => &c.position,
            SurfaceKind::Conical(c) => &c.position,
            SurfaceKind::Spherical(s) => &s.position,
            SurfaceKind::Toroidal(t) => &t.position,
            _ => return None,
        };
        Some(self.raw_frame(*id))
    }

    /// The file's frame ratios of a circle, ellipse, parabola or hyperbola, looking through
    /// surface and seam curves.
    fn curve_raw_frame(&self, r: &m::CurveRef) -> Option<RawFrame> {
        let model = self.model;
        let position = match r {
            m::CurveRef::Circle(i) => &model.get(m::CircleId(i.0))?.position,
            m::CurveRef::Ellipse(i) => &model.get(m::EllipseId(i.0))?.position,
            m::CurveRef::Hyperbola(i) => &model.get(m::HyperbolaId(i.0))?.position,
            m::CurveRef::Parabola(i) => &model.get(m::ParabolaId(i.0))?.position,
            m::CurveRef::SurfaceCurve(i) => {
                return self.curve_raw_frame(&model.get(m::SurfaceCurveId(i.0))?.curve_3d);
            }
            m::CurveRef::SeamCurve(i) => {
                return self.curve_raw_frame(&model.get(m::SeamCurveId(i.0))?.curve_3d);
            }
            _ => return None,
        };
        let m::Axis2PlacementRef::Axis2Placement3d(id) = position else {
            return None;
        };
        Some(self.raw_frame(*id))
    }

    fn placement(&self, r: &m::Axis2Placement3dRef) -> Option<Frame> {
        let m::Axis2Placement3dRef::Axis2Placement3d(id) = r;
        self.frame3(*id)
    }

    /// The face's surface, or `None` when no form of it resolves.
    fn surface(&self, face: &StepFace<'_>) -> Option<Surface> {
        let mm = self.to_mm;
        match face.surface().kind() {
            SurfaceKind::Plane(p) => self
                .placement(&p.position)
                .map(|frame| Surface::Plane { frame }),
            SurfaceKind::Cylindrical(c) => {
                self.placement(&c.position).map(|frame| Surface::Cylinder {
                    frame,
                    radius: c.radius * mm,
                })
            }
            SurfaceKind::Conical(c) => self.placement(&c.position).map(|frame| Surface::Cone {
                frame,
                radius: c.radius * mm,
                semi_angle: c.semi_angle * self.to_rad,
            }),
            SurfaceKind::Spherical(s) => self.placement(&s.position).map(|frame| Surface::Sphere {
                frame,
                radius: s.radius * mm,
            }),
            SurfaceKind::Toroidal(t) => self.placement(&t.position).map(|frame| Surface::Torus {
                frame,
                major: t.major_radius * mm,
                minor: t.minor_radius * mm,
            }),
            // Every B-spline form, rational ones included (written as complex entities), is
            // one GeomAbs_BSplineSurface to OpenCascade.
            SurfaceKind::BSpline(_)
            | SurfaceKind::BSplineWithKnots(_)
            | SurfaceKind::QuasiUniform(_)
            | SurfaceKind::Uniform(_)
            | SurfaceKind::Other("RATIONAL_B_SPLINE_SURFACE" | "COMPLEX") => {
                self.freeform(face, "BSPLINE")
            }
            SurfaceKind::Bezier(_) => self.freeform(face, "BEZIER"),
            SurfaceKind::LinearExtrusion(_) => self.freeform(face, "EXTRUSION"),
            SurfaceKind::Revolution(_) => self.freeform(face, "REVOLUTION"),
            // step-io 0.3 adds these conversions independently of conic support.
            // Preserve haecceity's existing unresolved-surface behaviour for them.
            SurfaceKind::Offset(_) | SurfaceKind::DegenerateToroidal(_) => None,
            _ => self.freeform(face, "OTHER"),
        }
    }

    /// The point of a `VERTEX_LOOP` bound, if that is what this bound is.
    fn vertex_loop(&self, bound: &step_io::scene::Bound<'_>) -> Option<V3> {
        let r = match bound.key() {
            m::EntityKey::FaceBound(i) => &self.model.get(m::FaceBoundId(i.0))?.bound,
            m::EntityKey::FaceOuterBound(i) => &self.model.get(m::FaceOuterBoundId(i.0))?.bound,
            _ => return None,
        };
        let m::LoopRef::VertexLoop(id) = r else {
            return None;
        };
        let m::VertexRef::VertexPoint(v) = &self.model.get(m::VertexLoopId(id.0))?.loop_vertex
        else {
            return None;
        };
        let m::PointRef::CartesianPoint(c) =
            &self.model.get(m::VertexPointId(v.0))?.vertex_geometry
        else {
            return None;
        };
        self.point(&m::CartesianPointRef::CartesianPoint(*c))
    }

    /// The pcurves the file gives for this face's edges, by edge index, in the surface's
    /// parameter units (lengths scaled to millimetres, angles to radians).
    fn pcurves(
        &self,
        face: &StepFace<'_>,
        surface: &Surface,
        edge_index: &HashMap<m::EntityKey, usize>,
        edges: &[Edge],
    ) -> (Vec<(usize, Pcurve)>, Vec<Option<PcurveSpline>>) {
        let basis = face.surface().key();
        let (su, sv) = match surface {
            Surface::Plane { .. } => (self.to_mm, self.to_mm),
            Surface::Cylinder { .. } => (self.to_rad, self.to_mm),
            // STEP's cone v is axial height; OpenCascade's runs along the slant.
            Surface::Cone { semi_angle, .. } => (self.to_rad, self.to_mm / semi_angle.cos()),
            Surface::Sphere { .. } | Surface::Torus { .. } => (self.to_rad, self.to_rad),
            Surface::Freeform { .. } | Surface::Other { .. } => return (Vec::new(), Vec::new()),
        };
        let scale = |(u, v): (f64, f64)| (u * su, v * sv);
        let mut out = Vec::new();
        // Per pcurve held, its B-spline form when it is one (for writing it back).
        let mut splines = Vec::new();
        for bound in face.bounds() {
            for edge in bound.edges() {
                let Some(&index) = edge_index.get(&edge.key()) else {
                    continue;
                };
                let vertex = |v: Option<step_io::scene::Vertex<'_>>| {
                    v.and_then(|v| v.point())
                        .map(|p| self.place_point(geom::scale(p.xyz(), self.to_mm)))
                };
                let (Some(a), Some(b)) = (vertex(edge.start()), vertex(edge.end())) else {
                    continue;
                };
                let associated = match &raw_edge_curve(self.model, &edge) {
                    m::CurveRef::SurfaceCurve(i) => {
                        &self
                            .model
                            .resolved(m::SurfaceCurveId(i.0))
                            .associated_geometry
                    }
                    m::CurveRef::SeamCurve(i) => {
                        &self.model.resolved(m::SeamCurveId(i.0)).associated_geometry
                    }
                    _ => continue,
                };
                for g in associated {
                    let m::PcurveOrSurfaceRef::Pcurve(p) = g else {
                        continue;
                    };
                    let pcurve = self.model.resolved(m::PcurveId(p.0));
                    if pcurve.basis_surface.entity_key() != basis {
                        continue;
                    }
                    let m::DefinitionalRepresentationRef::DefinitionalRepresentation(d) =
                        &pcurve.reference_to_curve
                    else {
                        continue;
                    };
                    for item in &self
                        .model
                        .resolved(m::DefinitionalRepresentationId(d.0))
                        .items
                    {
                        if let Some((point, dir)) = self.line_2d(item) {
                            let dir = (dir.0 * su, dir.1 * sv);
                            out.push((
                                index,
                                Pcurve::Line {
                                    point: scale(point),
                                    dir,
                                },
                            ));
                            splines.push(None);
                        } else if let Some(points) = self.bspline_poles_2d(item) {
                            let points: Vec<(f64, f64)> = points.into_iter().map(scale).collect();
                            // OpenCascade boxes the pcurve trimmed to the edge. A clamped curve's
                            // end poles are its end points, so the polygon stands for the edge
                            // only when they land on the edge's vertices.
                            let length: f64 = edges[index]
                                .samples
                                .windows(2)
                                .map(|w| geom::dist(w[0], w[1]))
                                .sum();
                            if spans_edge(surface, &points, (a, b), length) {
                                out.push((index, Pcurve::Poles(points)));
                                splines.push(self.bspline_form_2d(item));
                            }
                        }
                    }
                }
            }
        }
        (out, splines)
    }

    /// A 2D B-spline's degree, knots and weights (its poles are [`Self::bspline_poles_2d`]).
    fn bspline_form_2d(&self, item: &m::RepresentationItemRef) -> Option<PcurveSpline> {
        let (degree, multiplicities, knots, weights) = match item {
            m::RepresentationItemRef::BSplineCurveWithKnots(i) => {
                let c = self.model.get(m::BSplineCurveWithKnotsId(i.0))?;
                (c.degree, &c.knot_multiplicities, &c.knots, None)
            }
            m::RepresentationItemRef::Complex(c) => {
                let parts = &self.model.get(m::ComplexEntityId(c.0))?.parts;
                let degree = parts.iter().find_map(|p| match p {
                    m::PartialEntity::BSplineCurve { degree, .. } => Some(*degree),
                    _ => None,
                })?;
                let (mults, knots) = parts.iter().find_map(|p| match p {
                    m::PartialEntity::BSplineCurveWithKnots {
                        knot_multiplicities,
                        knots,
                        ..
                    } => Some((knot_multiplicities, knots)),
                    _ => None,
                })?;
                let weights = parts.iter().find_map(|p| match p {
                    m::PartialEntity::RationalBSplineCurve { weights_data } => {
                        Some(weights_data.clone())
                    }
                    _ => None,
                });
                (degree, mults, knots, weights)
            }
            _ => return None,
        };
        Some(PcurveSpline {
            degree: usize::try_from(degree).ok()?,
            multiplicities: multiplicities
                .iter()
                .map(|&m| usize::try_from(m).ok())
                .collect::<Option<_>>()?,
            knots: knots.clone(),
            weights,
        })
    }

    /// A (rational) B-spline curve written as a STEP complex entity — `B_SPLINE_CURVE` +
    /// `B_SPLINE_CURVE_WITH_KNOTS` (+ `RATIONAL_B_SPLINE_CURVE`) — in NURBS form.
    fn complex_bspline(&self, parts: &[m::PartialEntity]) -> Option<Curve> {
        let (degree, refs) = parts.iter().find_map(|p| match p {
            m::PartialEntity::BSplineCurve {
                degree,
                control_points_list,
                ..
            } => Some((*degree, control_points_list)),
            _ => None,
        })?;
        let (mults, values) = parts.iter().find_map(|p| match p {
            m::PartialEntity::BSplineCurveWithKnots {
                knot_multiplicities,
                knots,
                ..
            } => Some((knot_multiplicities, knots)),
            _ => None,
        })?;
        let points: Vec<V3> = refs.iter().map(|r| self.point(r)).collect::<Option<_>>()?;
        let weights = parts
            .iter()
            .find_map(|p| match p {
                m::PartialEntity::RationalBSplineCurve { weights_data } => {
                    Some(weights_data.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| vec![1.0; points.len()]);
        // A curve has as many knots as poles plus degree plus one; more is malformed (and
        // refused by NurbsCurve::new), so it is not expanded.
        let count = |m: i64| usize::try_from(m).unwrap_or(0);
        let total = mults
            .iter()
            .take(values.len())
            .try_fold(0usize, |t, &m| t.checked_add(count(m)))?;
        if total > points.len().saturating_add(crate::nurbs::MAX_DEGREE + 1) {
            return None;
        }
        let knots: Vec<f64> = values
            .iter()
            .zip(mults)
            .flat_map(|(k, &m)| std::iter::repeat_n(*k, count(m)))
            .collect();
        NurbsCurve::new(usize::try_from(degree).ok()?, points, weights, knots).map(Curve::Nurbs)
    }

    /// A 2D `LINE`'s point and direction, unscaled.
    fn line_2d(&self, item: &m::RepresentationItemRef) -> Option<((f64, f64), (f64, f64))> {
        let m::RepresentationItemRef::Line(i) = item else {
            return None;
        };
        let line = self.model.get(m::LineId(i.0))?;
        let m::CartesianPointRef::CartesianPoint(p) = &line.pnt else {
            return None;
        };
        let c = &self.model.get(m::CartesianPointId(p.0))?.coordinates;
        let m::VectorRef::Vector(v) = &line.dir else {
            return None;
        };
        let m::DirectionRef::Direction(d) = &self.model.get(m::VectorId(v.0))?.orientation else {
            return None;
        };
        let r = &self.model.get(m::DirectionId(d.0))?.direction_ratios;
        Some(((*c.first()?, *c.get(1)?), (*r.first()?, *r.get(1)?)))
    }

    fn bspline_poles_2d(&self, item: &m::RepresentationItemRef) -> Option<Vec<(f64, f64)>> {
        let refs = match item {
            m::RepresentationItemRef::BSplineCurveWithKnots(i) => {
                &self
                    .model
                    .get(m::BSplineCurveWithKnotsId(i.0))?
                    .control_points_list
            }
            m::RepresentationItemRef::Complex(c) => self
                .model
                .get(m::ComplexEntityId(c.0))?
                .parts
                .iter()
                .find_map(|part| match part {
                    m::PartialEntity::BSplineCurve {
                        control_points_list,
                        ..
                    } => Some(control_points_list),
                    _ => None,
                })?,
            _ => return None,
        };
        refs.iter()
            .map(|r| {
                let m::CartesianPointRef::CartesianPoint(i) = r else {
                    return None;
                };
                let c = &self.model.get(m::CartesianPointId(i.0))?.coordinates;
                Some((*c.first()?, *c.get(1)?))
            })
            .collect()
    }

    fn freeform(&self, face: &StepFace<'_>, kind: &'static str) -> Option<Surface> {
        let mut n = face.to_nurbs()?;
        // An unbounded radial LINE has two preimages for every radius. step-io's
        // conservative cover includes both, folding the disk over itself at the
        // axis. Inversion can then change sheets, preserving |Su × Sv| but reversing
        // the signed normal used by mass properties. Keep the half-line containing
        // the profile's origin (the forward half when the origin is on the axis),
        // retaining the profile's direction and a single cover of the disk.
        if let SurfaceKind::Revolution(s) = face.surface().kind()
            && let m::CurveRef::Line(i) = &s.swept_curve
        {
            // Work in file coordinates: deciding which sheet to retain must not
            // depend on cancellation after a large instance translation.
            let point = |r: &m::CartesianPointRef| {
                let m::CartesianPointRef::CartesianPoint(i) = r else {
                    return None;
                };
                let p = &self.model.get(m::CartesianPointId(i.0))?.coordinates;
                Some([0, 1, 2].map(|k| p.get(k).copied().unwrap_or(0.0)))
            };
            let line = self.model.get(m::LineId(i.0))?;
            let base = point(&line.pnt)?;
            let m::VectorRef::Vector(i) = &line.dir else {
                return None;
            };
            let dir = self.raw_direction(&self.model.get(m::VectorId(i.0))?.orientation)?;
            let m::Axis1PlacementRef::Axis1Placement(i) = &s.axis_position;
            let axis = self.model.get(m::Axis1PlacementId(i.0))?;
            let origin = point(&axis.location)?;
            let z = match &axis.axis {
                Some(a) => self.raw_direction(a)?,
                None => [0.0, 0.0, 1.0],
            };
            let delta = geom::sub(base, origin);
            let radial = geom::sub(delta, geom::scale(z, geom::dot(delta, z)));
            let along = geom::dot(radial, dir);
            if geom::dot(dir, z).abs() <= 1e-12
                && geom::norm(geom::sub(radial, geom::scale(dir, along)))
                    <= 1e-12 * geom::norm(delta).max(1.0)
            {
                // This correction requires a linear, two-pole profile. Refuse an
                // unexpected dependency conversion instead of indexing or changing
                // a higher-degree surface as though it were a line segment.
                if n.degree_v != 1 || n.control_points.iter().any(|row| row.len() != 2) {
                    return None;
                }
                let centre = geom::add(origin, geom::scale(z, geom::dot(delta, z)));
                // Every rotated row meets the axis at the same profile parameter.
                let first = n.control_points.first()?;
                let (a, b) = (first[0], first[1]);
                let ab = geom::sub(b, a);
                let t = geom::dot(geom::sub(centre, a), ab) / geom::dot(ab, ab);
                // An origin on the axis can acquire either sign through cancellation.
                // Within 1e-12 of the profile cover's length it is on the axis: retain
                // the forward half. A resolved far-side origin still retains its half.
                let backward = along < -1e-12 * geom::norm(ab);
                for row in &mut n.control_points {
                    let at_axis = geom::add(row[0], geom::scale(geom::sub(row[1], row[0]), t));
                    row[usize::from(backward)] = at_axis;
                }
            }
        }
        let place = |row: &Vec<[f64; 3]>| -> Vec<V3> {
            row.iter()
                .map(|p| self.place_point(geom::scale(*p, self.to_mm)))
                .collect()
        };
        let surface = NurbsSurface::new(
            n.degree_u,
            n.degree_v,
            n.control_points.iter().map(place).collect(),
            n.weights,
            n.knots_u,
            n.knots_v,
        )?;
        Some(Surface::Freeform {
            kind,
            surface: Box::new(surface),
        })
    }

    // Retain a LINE's original direction ratios and placement: normalising before rotating
    // loses bits compared with normalising the rotated ratios (the STEP reader's convention).
    fn line_direction(&self, curve: &m::CurveRef) -> Option<V3> {
        match curve {
            m::CurveRef::Line(i) => {
                let m::VectorRef::Vector(v) = &self.model.get(m::LineId(i.0))?.dir else {
                    return None;
                };
                let m::DirectionRef::Direction(d) = &self.model.get(m::VectorId(v.0))?.orientation
                else {
                    return None;
                };
                let ratios = &self.model.get(m::DirectionId(d.0))?.direction_ratios;
                Some([0, 1, 2].map(|i| ratios.get(i).copied().unwrap_or(0.0)))
            }
            m::CurveRef::SurfaceCurve(i) => {
                self.line_direction(&self.model.get(m::SurfaceCurveId(i.0))?.curve_3d)
            }
            m::CurveRef::SeamCurve(i) => {
                self.line_direction(&self.model.get(m::SeamCurveId(i.0))?.curve_3d)
            }
            _ => None,
        }
    }

    /// The exact 3D curve under a curve reference, looking through surface/seam curves.
    fn curve(&self, r: &m::CurveRef) -> Option<Curve> {
        let mm = self.to_mm;
        match r {
            m::CurveRef::Line(i) => {
                let l = self.model.get(m::LineId(i.0))?;
                let origin = self.point(&l.pnt)?;
                let m::VectorRef::Vector(v) = &l.dir else {
                    return None;
                };
                let v = self.model.get(m::VectorId(v.0))?;
                let dir = self.direction(&v.orientation)?;
                Some(Curve::Line { origin, dir })
            }
            m::CurveRef::Circle(i) => {
                let c = self.model.get(m::CircleId(i.0))?;
                let m::Axis2PlacementRef::Axis2Placement3d(p) = &c.position else {
                    return None;
                };
                Some(Curve::Circle {
                    frame: self.frame3(*p)?,
                    radius: c.radius * mm,
                })
            }
            m::CurveRef::Ellipse(i) => {
                let e = self.model.get(m::EllipseId(i.0))?;
                let m::Axis2PlacementRef::Axis2Placement3d(p) = &e.position else {
                    return None;
                };
                Some(Curve::Ellipse {
                    frame: self.frame3(*p)?,
                    major: e.semi_axis_1 * mm,
                    minor: e.semi_axis_2 * mm,
                })
            }
            m::CurveRef::Complex(c) => {
                self.complex_bspline(&self.model.get(m::ComplexEntityId(c.0))?.parts)
            }
            m::CurveRef::SurfaceCurve(i) => {
                self.curve(&self.model.get(m::SurfaceCurveId(i.0))?.curve_3d)
            }
            m::CurveRef::SeamCurve(i) => self.curve(&self.model.get(m::SeamCurveId(i.0))?.curve_3d),
            _ => None,
        }
    }

    /// Use step-io's typed conic, unwrapping its 3D surface-curve containers. Keep the
    /// established hyperbola endpoint inversion/pole arithmetic for byte-identical results.
    fn conic(&self, edge: &StepEdge<'_>) -> Option<geom::Conic> {
        let mut curve = edge.curve();
        while let Some(basis) = curve.curve_3d() {
            curve = basis;
        }
        match curve.kind() {
            step_io::scene::CurveKind::Hyperbola(h) => {
                let m::Axis2PlacementRef::Axis2Placement3d(p) = &h.position else {
                    return None;
                };
                let frame = self.frame3(*p)?;
                let (major, minor) = (h.semi_axis * self.to_mm, h.semi_imag_axis * self.to_mm);
                (major > 0.0 && minor > 0.0).then_some(geom::Conic::Hyperbola(geom::Hyperbola {
                    frame,
                    major,
                    minor,
                }))
            }
            step_io::scene::CurveKind::Parabola(p) => {
                let m::Axis2PlacementRef::Axis2Placement3d(at) = &p.position else {
                    return None;
                };
                let focal_dist = p.focal_dist * self.to_mm;
                (focal_dist > 0.0).then_some(geom::Conic::Parabola {
                    frame: self.frame3(*at)?,
                    focal_dist,
                })
            }
            _ => None,
        }
    }

    /// *vertex_id* numbers the vertices of the instance being read. The flag is `false` when
    /// the edge's curve did not resolve and its chord stands in for it.
    fn edge(
        &self,
        edge: &StepEdge<'_>,
        vertex_id: &mut impl FnMut(m::EntityKey) -> usize,
    ) -> Result<(Edge, bool), StepError> {
        let vertex = |v: Option<step_io::scene::Vertex<'_>>| -> Option<V3> {
            let p = v?.point()?.xyz();
            Some(self.place_point(geom::scale(p, self.to_mm)))
        };
        let (Some(start), Some(end)) = (vertex(edge.start()), vertex(edge.end())) else {
            return Err(StepError::Unsupported(
                "edge without cartesian vertices".into(),
            ));
        };
        let (Some(v0), Some(v1)) = (edge.start(), edge.end()) else {
            return Err(StepError::Unsupported(
                "edge without cartesian vertices".into(),
            ));
        };
        let vertices = (vertex_id(v0.key()), vertex_id(v1.key()));
        // Exact analytic curves first; then an unbounded conic's arc between the vertices, exactly;
        // otherwise the whole curve in NURBS form, which the edge is cut from by inverting its
        // vertices (wrapping round a closed curve if it must). The
        // edge-bounded NURBS step-io offers is the last resort: it cuts the short span out of a
        // nearly closed curve even when the edge runs the long way round.
        let to_curve = |n: step_io::scene::NurbsCurve| {
            let points = n
                .control_points
                .iter()
                .map(|p| self.place_point(geom::scale(*p, self.to_mm)))
                .collect();
            NurbsCurve::new(n.degree, points, n.weights, n.knots).map(Curve::Nurbs)
        };
        let raw = raw_edge_curve(self.model, edge);
        let conic = self.conic(edge);
        let curve = self
            .curve(&raw)
            .or_else(|| conic.as_ref()?.arc(start, end))
            .or_else(|| {
                // Our conic arc checks that the vertices actually lie on the curve.
                // A failed check must not be bypassed by a conversion that projects
                // them onto it: that would claim a displaced boundary was resolved.
                if conic.is_some() {
                    None
                } else {
                    edge.curve()
                        .to_nurbs()
                        .and_then(to_curve)
                        .or_else(|| edge.to_nurbs().and_then(to_curve))
                }
            });
        let resolved = curve.is_some();
        // A closed edge has no chord to stand in for its curve.
        if !resolved && vertices.0 == vertices.1 {
            let model = self.model;
            let kind = format!(
                "{:?}",
                match &raw {
                    m::CurveRef::SurfaceCurve(i) =>
                        &model
                            .get(m::SurfaceCurveId(i.0))
                            .ok_or_else(dangling)?
                            .curve_3d,
                    m::CurveRef::SeamCurve(i) =>
                        &model
                            .get(m::SeamCurveId(i.0))
                            .ok_or_else(dangling)?
                            .curve_3d,
                    other => other,
                }
            );
            let kind = kind.split('(').next().unwrap_or_default();
            return Err(StepError::Unsupported(format!(
                "a closed edge's curve ({kind}) does not resolve"
            )));
        }
        // An open edge's curve no form resolves is replaced by its chord, which keeps the loop
        // closed where dropping the edge would open it; the part records the substitution.
        let curve = curve.unwrap_or(Curve::Line {
            origin: start,
            dir: geom::sub(end, start),
        });
        let samples = sample_edge(
            &curve,
            start,
            end,
            edge.same_sense(),
            vertices.0 == vertices.1,
        );
        let edge = Edge {
            curve,
            start,
            end,
            vertices,
            same_sense: edge.same_sense(),
            samples,
        };
        Ok((edge, resolved))
    }
}

/// Whether a pcurve's end poles map onto the edge's two vertices (in either order).
fn spans_edge(surface: &Surface, poles: &[(f64, f64)], (a, b): (V3, V3), length: f64) -> bool {
    let (Some(&first), Some(&last)) = (poles.first(), poles.last()) else {
        return false;
    };
    let (p, q) = (
        surface.value(first.0, first.1),
        surface.value(last.0, last.1),
    );
    // Files approximate their pcurves; a curve longer than its edge misses a vertex by a large
    // fraction of the edge's length, an approximation by far less.
    let tol = (0.01 * length).max(1e-4);
    let near = |x: V3, y: V3| geom::dist(x, y) <= tol;
    (near(p, a) && near(q, b)) || (near(p, b) && near(q, a))
}

/// The key of a representation named on one side of a `SHAPE_REPRESENTATION_RELATIONSHIP`,
/// for the shape-carrying kinds.
fn rep_key(r: &m::RepresentationOrRepresentationReferenceRef) -> Option<m::EntityKey> {
    use m::RepresentationOrRepresentationReferenceRef as R;
    Some(match r {
        R::ShapeRepresentation(i) => m::EntityKey::ShapeRepresentation(*i),
        R::AdvancedBrepShapeRepresentation(i) => m::EntityKey::AdvancedBrepShapeRepresentation(*i),
        R::ManifoldSurfaceShapeRepresentation(i) => {
            m::EntityKey::ManifoldSurfaceShapeRepresentation(*i)
        }
        _ => return None,
    })
}

/// Surface models OpenCascade transfers: those listed by a representation that a product shape
/// uses, directly or through a plain `SHAPE_REPRESENTATION_RELATIONSHIP`. An unreferenced model
/// is construction geometry the importer never reaches.
fn reachable_surface_models(
    model: &m::StepModel,
    rg: &step_io::RefGraph,
) -> Vec<m::ShellBasedSurfaceModelId> {
    let used_directly = |rep: m::EntityKey| {
        rg.referrers(rep)
            .iter()
            .any(|r| matches!(r, m::EntityKey::ShapeDefinitionRepresentation(_)))
    };
    let used = |rep: m::EntityKey| {
        used_directly(rep)
            || rg.referrers(rep).iter().any(|r| {
                let m::EntityKey::ShapeRepresentationRelationship(i) = r else {
                    return false;
                };
                let srr = model.resolved(m::ShapeRepresentationRelationshipId(i.0));
                [&srr.rep_1, &srr.rep_2]
                    .into_iter()
                    .filter_map(rep_key)
                    .any(|k| k != rep && used_directly(k))
            })
    };
    model
        .all::<m::ShellBasedSurfaceModel>()
        .map(|(id, _)| id)
        .filter(|&id| {
            let key = m::EntityKey::ShellBasedSurfaceModel(id);
            rg.referrers(key).iter().any(|&rep| used(rep))
        })
        .collect()
}

/// The product definitions whose shape lists surface model *index*, directly or through a
/// plain `SHAPE_REPRESENTATION_RELATIONSHIP`.
fn surface_model_owners(
    model: &m::StepModel,
    rg: &step_io::RefGraph,
    index: m::ShellBasedSurfaceModelId,
) -> Vec<m::EntityKey> {
    let key = m::EntityKey::ShellBasedSurfaceModel(index);
    let mut reps: Vec<m::EntityKey> = rg.referrers(key).to_vec();
    for rep in reps.clone() {
        for r in rg.referrers(rep) {
            if let m::EntityKey::ShapeRepresentationRelationship(i) = r {
                let srr = model.resolved(m::ShapeRepresentationRelationshipId(i.0));
                reps.extend([&srr.rep_1, &srr.rep_2].into_iter().filter_map(rep_key));
            }
        }
    }
    let mut owners = Vec::new();
    for rep in reps {
        for r in rg.referrers(rep) {
            let m::EntityKey::ShapeDefinitionRepresentation(i) = r else {
                continue;
            };
            let sdr = model.resolved(m::ShapeDefinitionRepresentationId(i.0));
            let m::RepresentedDefinitionRef::ProductDefinitionShape(pds) = &sdr.definition else {
                continue;
            };
            match model
                .resolved(m::ProductDefinitionShapeId(pds.0))
                .definition
            {
                m::CharacterizedDefinitionRef::ProductDefinition(d) => {
                    owners.push(m::EntityKey::ProductDefinition(d))
                }
                m::CharacterizedDefinitionRef::ProductDefinitionWithAssociatedDocuments(d) => {
                    owners.push(m::EntityKey::ProductDefinitionWithAssociatedDocuments(d))
                }
                _ => {}
            }
        }
    }
    owners
}

#[derive(Default)]
struct PlacementPaths {
    current: Vec<Placement>,
    definitions: Vec<std::sync::Arc<[Placement]>>,
    current_raw: Vec<Option<InstanceTransform>>,
    raw: Vec<std::sync::Arc<[Option<InstanceTransform>]>>,
    current_products: Vec<ProductLabel>,
    products: Vec<std::sync::Arc<[ProductLabel]>>,
}

/// Every solid reachable from an assembly definition, with its accumulated placement and the
/// index in *placed* of the placed definition it belongs to.
fn collect_instances<'m>(
    def: step_io::scene::ProductDef<'m>,
    placement: Placement,
    to_mm: f64,
    out: &mut Vec<(step_io::scene::Solid<'m>, Placement, Option<usize>)>,
    placed: &mut Vec<(step_io::scene::ProductDef<'m>, Placement)>,
    depth: usize,
    paths: &mut PlacementPaths,
) -> Result<(), StepError> {
    if depth > limits::ASSEMBLY_DEPTH {
        return Err(StepError::Limit(format!(
            "assembly nested more than {} levels deep (or cyclic)",
            limits::ASSEMBLY_DEPTH
        )));
    }
    let too_many = || {
        StepError::Limit(format!(
            "more than {} product or solid instances",
            limits::INSTANCES
        ))
    };
    if placed.len() >= limits::INSTANCES {
        return Err(too_many());
    }
    placed.push((def, placement));
    paths.definitions.push(paths.current.clone().into());
    paths.raw.push(paths.current_raw.clone().into());
    paths.current_products.push(
        def.product()
            .map(|p| ProductLabel {
                id: p.id().to_owned(),
                name: p.name().to_owned(),
            })
            .unwrap_or(ProductLabel {
                id: String::new(),
                name: String::new(),
            }),
    );
    paths.products.push(paths.current_products.clone().into());
    for solid in def.solids() {
        if out.len() >= limits::INSTANCES {
            return Err(too_many());
        }
        out.push((solid, placement, Some(placed.len() - 1)));
    }
    for occurrence in def.occurrences() {
        let Some(child) = occurrence.definition() else {
            continue;
        };
        let local = occurrence
            .transform()
            .and_then(|t| t.matrix())
            .map_or(IDENTITY, |mtx| {
                [0, 1, 2].map(|i| [mtx[i][0], mtx[i][1], mtx[i][2], mtx[i][3] * to_mm])
            });
        // The level as the file states it, when its `from` frame is the standard one.
        const STANDARD: ([f64; 3], [f64; 3], [f64; 3]) =
            ([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        let raw = match occurrence.transform() {
            None => Some(InstanceTransform {
                origin: STANDARD.0,
                axis: STANDARD.1,
                reference: STANDARD.2,
            }),
            Some(t)
                if t.matrix().is_some()
                    && (t.from.origin, t.from.axis, t.from.ref_direction) == STANDARD =>
            {
                Some(InstanceTransform {
                    origin: t.to.origin.map(|c| c * to_mm),
                    axis: t.to.axis,
                    reference: t.to.ref_direction,
                })
            }
            Some(_) => None,
        };
        paths.current.push(local);
        paths.current_raw.push(raw);
        collect_instances(
            child,
            compose(&placement, &local),
            to_mm,
            out,
            placed,
            depth + 1,
            paths,
        )?;
        paths.current.pop();
        paths.current_raw.pop();
    }
    paths.current_products.pop();
    Ok(())
}

#[allow(
    clippy::unreachable,
    reason = "a step-io scene edge is built from an EDGE_CURVE id"
)]
fn raw_edge_curve(model: &m::StepModel, edge: &StepEdge<'_>) -> m::CurveRef {
    let m::EntityKey::EdgeCurve(id) = edge.key() else {
        unreachable!("edges are EDGE_CURVEs")
    };
    model.resolved(m::EdgeCurveId(id.0)).edge_geometry.clone()
}

/// The file instance (`#N`) behind each face, edge and product definition step-io read.
///
/// step-io keeps no instance ids: it fills each arena of a simple entity type in ascending id
/// order over the instances it keeps, so arena index *k* of a type is the *k*-th kept instance
/// of that type (kept: not in the report's dropped list; the only instances its normalization
/// adds are `COLOUR`s, numbered after every instance of the file). The map is rebuilt from the
/// raw graph on that basis and every face and edge is checked against its record before its id
/// is given out (see [`FileIds::face`], [`FileIds::edge`]); a disagreement is an error, never a
/// guess.
struct FileIds<'a> {
    graph: &'a std::collections::BTreeMap<u64, step_io::parser::RawEntity>,
    model: &'a m::StepModel,
    /// Kept instances of each simple type below, in ascending id order: arena index → `#N`.
    by_type: HashMap<&'static str, Vec<u64>>,
    /// Faces and edges already checked.
    checked: HashMap<m::EntityKey, u64>,
}

/// The entity types whose instance ids the reader gives out.
const ID_TYPES: [&str; 6] = [
    "ADVANCED_FACE",
    "FACE_SURFACE",
    "EDGE_CURVE",
    "PRODUCT_DEFINITION",
    "PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS",
    "PRODUCT_DEFINITION_SHAPE",
];

impl<'a> FileIds<'a> {
    fn new(
        graph: &'a step_io::parser::Graph,
        model: &'a m::StepModel,
        report: &step_io::Report,
    ) -> Self {
        let dropped: std::collections::HashSet<u64> =
            report.dropped.iter().map(|(id, _)| *id).collect();
        let mut by_type: HashMap<&'static str, Vec<u64>> = HashMap::new();
        for (&id, entity) in &graph.entities {
            if let step_io::parser::RawEntity::Simple { name, .. } = entity
                && let Some(&kind) = ID_TYPES.iter().find(|&&t| t == name)
                && !dropped.contains(&id)
            {
                by_type.entry(kind).or_default().push(id);
            }
        }
        FileIds {
            graph: &graph.entities,
            model,
            by_type,
            checked: HashMap::new(),
        }
    }

    /// The `#N` of a face, edge, product definition or product definition shape.
    fn id(&self, key: m::EntityKey) -> Result<u64, StepError> {
        let (kind, index) = match key {
            m::EntityKey::AdvancedFace(i) => ("ADVANCED_FACE", i.0),
            m::EntityKey::FaceSurface(i) => ("FACE_SURFACE", i.0),
            m::EntityKey::EdgeCurve(i) => ("EDGE_CURVE", i.0),
            m::EntityKey::ProductDefinition(i) => ("PRODUCT_DEFINITION", i.0),
            m::EntityKey::ProductDefinitionWithAssociatedDocuments(i) => {
                ("PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS", i.0)
            }
            m::EntityKey::ProductDefinitionShape(i) => ("PRODUCT_DEFINITION_SHAPE", i.0),
            other => {
                return Err(StepError::Unsupported(format!(
                    "no file instance is kept for {}",
                    step_name(other)
                )));
            }
        };
        self.by_type
            .get(kind)
            .and_then(|ids| ids.get(index as usize))
            .copied()
            .ok_or_else(|| {
                StepError::Unsupported(format!("{kind} {index} read has no instance in the file"))
            })
    }

    /// The `#N` of a face, checked against its record: entity type, bound count, the surface's
    /// entity type, `same_sense`, the surface's location point (its placement's, an axis's, or a
    /// B-spline's first control point) and the `EDGE_CURVE`s of its loops. A surface with no
    /// location point of its own (`SURFACE_OF_LINEAR_EXTRUSION`, `OFFSET_SURFACE`,
    /// `RECTANGULAR_TRIMMED_SURFACE` and other kinds `raw_location` does not name) has no
    /// position check: there the loops' `EDGE_CURVE`s, whose vertices [`FileIds::edge`] checks
    /// by position, are what tells two such faces apart.
    fn face(&mut self, face: &StepFace<'_>) -> Result<u64, StepError> {
        let key = face.key();
        if let Some(&id) = self.checked.get(&key) {
            return Ok(id);
        }
        let id = self.id(key)?;
        self.check_face(face, id).map_err(|why| {
            StepError::Unsupported(format!(
                "face {} read as #{id} does not match that instance: {why}",
                step_name(key)
            ))
        })?;
        self.checked.insert(key, id);
        Ok(id)
    }

    /// The `#N` of an edge, checked against its record: the vertices' and the curve's entity
    /// types, `same_sense` and both vertices' coordinates.
    fn edge(&mut self, edge: &StepEdge<'_>) -> Result<u64, StepError> {
        let key = edge.key();
        if let Some(&id) = self.checked.get(&key) {
            return Ok(id);
        }
        let id = self.id(key)?;
        self.check_edge(edge, id).map_err(|why| {
            StepError::Unsupported(format!(
                "EDGE_CURVE read as #{id} does not match that instance: {why}"
            ))
        })?;
        self.checked.insert(key, id);
        Ok(id)
    }

    /// The `product_definition_shape` a part's shape is represented through: the one of its
    /// definition a `shape_definition_representation` uses. Its id is checked to name the
    /// definition it is read for.
    fn definition_shape(
        &self,
        def: &step_io::scene::ProductDef<'_>,
        rg: &step_io::RefGraph,
    ) -> Result<u64, String> {
        let me = def.key();
        let mut found = Vec::new();
        for r in rg.referrers(me) {
            let m::EntityKey::ProductDefinitionShape(i) = *r else {
                continue;
            };
            let pds = self
                .model
                .get(m::ProductDefinitionShapeId(i.0))
                .ok_or("a dangling reference")?;
            let names_me = match pds.definition {
                m::CharacterizedDefinitionRef::ProductDefinition(d) => {
                    m::EntityKey::ProductDefinition(d) == me
                }
                m::CharacterizedDefinitionRef::ProductDefinitionWithAssociatedDocuments(d) => {
                    m::EntityKey::ProductDefinitionWithAssociatedDocuments(d) == me
                }
                _ => false,
            };
            let represented = rg
                .referrers(*r)
                .iter()
                .any(|s| matches!(s, m::EntityKey::ShapeDefinitionRepresentation(_)));
            if names_me && represented {
                found.push(*r);
            }
        }
        let [shape] = found[..] else {
            return Err(format!(
                "{} product_definition_shapes with a shape representation",
                found.len()
            ));
        };
        let (id, definition) = (
            self.id(shape).map_err(|e| e.to_string())?,
            self.id(me).map_err(|e| e.to_string())?,
        );
        match self.attribute(id, 2) {
            Some(Attribute::EntityRef(d)) if *d == definition => Ok(id),
            _ => Err(format!(
                "#{id} read as the shape of #{definition} does not name it"
            )),
        }
    }

    /// Attribute *index* of simple instance *id*.
    fn attribute(&self, id: u64, index: usize) -> Option<&'a Attribute> {
        match self.graph.get(&id)? {
            RawEntity::Simple { attributes, .. } => attributes.get(index),
            RawEntity::Complex { .. } => None,
        }
    }

    /// The entity type of instance *id*, `COMPLEX` for a complex instance.
    fn kind(&self, id: u64) -> Option<&'a str> {
        Some(match self.graph.get(&id)? {
            RawEntity::Simple { name, .. } => name,
            RawEntity::Complex { .. } => "COMPLEX",
        })
    }

    /// The instance attribute *index* of *id* refers to.
    fn reference(&self, id: u64, index: usize) -> Option<u64> {
        match self.attribute(id, index)? {
            Attribute::EntityRef(r) => Some(*r),
            _ => None,
        }
    }

    /// A `CARTESIAN_POINT`'s coordinates as written.
    fn raw_point(&self, id: u64) -> Option<Vec<f64>> {
        if self.kind(id)? != "CARTESIAN_POINT" {
            return None;
        }
        let Attribute::List(coordinates) = self.attribute(id, 1)? else {
            return None;
        };
        coordinates
            .iter()
            .map(|c| match c {
                Attribute::Real(x) => Some(*x),
                Attribute::Integer(n) => Some(*n as f64),
                _ => None,
            })
            .collect()
    }

    /// The location point of surface instance *id* as written (see [`FileIds::face`]).
    fn raw_location(&self, id: u64) -> Option<Vec<f64>> {
        let first_pole = |list: &Attribute| match list {
            Attribute::List(rows) => match rows.first()? {
                Attribute::List(row) => match row.first()? {
                    Attribute::EntityRef(p) => self.raw_point(*p),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        };
        match self.graph.get(&id)? {
            RawEntity::Simple { name, .. } => match name.as_str() {
                "PLANE"
                | "CYLINDRICAL_SURFACE"
                | "CONICAL_SURFACE"
                | "SPHERICAL_SURFACE"
                | "TOROIDAL_SURFACE"
                | "DEGENERATE_TOROIDAL_SURFACE" => {
                    let axis = self.reference(id, 1)?;
                    self.raw_point(self.reference(axis, 1)?)
                }
                "SURFACE_OF_REVOLUTION" => {
                    let axis = self.reference(id, 2)?;
                    self.raw_point(self.reference(axis, 1)?)
                }
                "B_SPLINE_SURFACE"
                | "B_SPLINE_SURFACE_WITH_KNOTS"
                | "BEZIER_SURFACE"
                | "UNIFORM_SURFACE"
                | "QUASI_UNIFORM_SURFACE" => first_pole(self.attribute(id, 3)?),
                _ => None,
            },
            RawEntity::Complex { parts, .. } => {
                let part = parts.iter().find(|p| p.name == "B_SPLINE_SURFACE")?;
                first_pole(part.attributes.get(2)?)
            }
        }
    }

    fn typed_point(&self, r: &m::CartesianPointRef) -> Option<Vec<f64>> {
        let m::CartesianPointRef::CartesianPoint(i) = r else {
            return None;
        };
        Some(
            self.model
                .get(m::CartesianPointId(i.0))?
                .coordinates
                .to_vec(),
        )
    }

    /// The location point of a face's surface as step-io read it.
    fn typed_location(&self, face: &StepFace<'_>) -> Option<Vec<f64>> {
        let model = self.model;
        let placed = |r: &m::Axis2Placement3dRef| {
            let m::Axis2Placement3dRef::Axis2Placement3d(i) = r;
            self.typed_point(&model.get(m::Axis2Placement3dId(i.0))?.location)
        };
        let first =
            |rows: &Vec<Vec<m::CartesianPointRef>>| self.typed_point(rows.first()?.first()?);
        match face.surface().kind() {
            SurfaceKind::Plane(s) => placed(&s.position),
            SurfaceKind::Cylindrical(s) => placed(&s.position),
            SurfaceKind::Conical(s) => placed(&s.position),
            SurfaceKind::Spherical(s) => placed(&s.position),
            SurfaceKind::Toroidal(s) => placed(&s.position),
            SurfaceKind::Revolution(s) => {
                let m::Axis1PlacementRef::Axis1Placement(i) = &s.axis_position;
                self.typed_point(&model.get(m::Axis1PlacementId(i.0))?.location)
            }
            SurfaceKind::BSpline(s) => first(&s.control_points_list),
            SurfaceKind::BSplineWithKnots(s) => first(&s.control_points_list),
            SurfaceKind::QuasiUniform(s) => first(&s.control_points_list),
            SurfaceKind::Uniform(s) => first(&s.control_points_list),
            SurfaceKind::Bezier(s) => first(&s.control_points_list),
            _ => match face.surface().key() {
                m::EntityKey::DegenerateToroidalSurface(i) => {
                    placed(&model.get(m::DegenerateToroidalSurfaceId(i.0))?.position)
                }
                m::EntityKey::ComplexEntity(i) => model
                    .get(m::ComplexEntityId(i.0))?
                    .parts
                    .iter()
                    .find_map(|p| match p {
                        m::PartialEntity::BSplineSurface {
                            control_points_list,
                            ..
                        } => first(control_points_list),
                        _ => None,
                    }),
                _ => None,
            },
        }
    }

    fn check_face(&self, face: &StepFace<'_>, id: u64) -> Result<(), String> {
        let (bounds, surface, same_sense) = match face.key() {
            m::EntityKey::AdvancedFace(i) => {
                let f = self
                    .model
                    .get(m::AdvancedFaceId(i.0))
                    .ok_or("a dangling reference")?;
                (f.bounds.len(), f.face_geometry.entity_key(), f.same_sense)
            }
            m::EntityKey::FaceSurface(i) => {
                let f = self
                    .model
                    .get(m::FaceSurfaceId(i.0))
                    .ok_or("a dangling reference")?;
                (f.bounds.len(), f.face_geometry.entity_key(), f.same_sense)
            }
            other => return Err(format!("a face of type {}", step_name(other))),
        };
        let kind = self.kind(id).unwrap_or("nothing");
        if kind != step_name(face.key()) {
            return Err(format!("it is {kind}"));
        }
        match self.attribute(id, 1) {
            Some(Attribute::List(l)) if l.len() == bounds => {}
            _ => return Err(format!("its bound count is not {bounds}")),
        }
        if face.bounds().count() != bounds {
            return Err("its bound count differs from the readable scene bounds".into());
        }
        let surface_id = self.reference(id, 2).ok_or("no surface reference")?;
        let surface_kind = self.kind(surface_id).unwrap_or("nothing");
        if surface_kind != step_name(surface) {
            return Err(format!(
                "its surface #{surface_id} is {surface_kind}, not {}",
                step_name(surface)
            ));
        }
        if logical(self.attribute(id, 3)) != Some(same_sense) {
            return Err(format!("its same_sense is not {same_sense}"));
        }
        if !same_point(
            self.raw_location(surface_id).as_deref(),
            self.typed_location(face).as_deref(),
        ) {
            return Err(format!(
                "its surface #{surface_id} is not where it was read"
            ));
        }
        // The EDGE_CURVEs of its EDGE_LOOPs, as written and as read.
        let mut written = Vec::new();
        if let Some(Attribute::List(list)) = self.attribute(id, 1) {
            for (bound, read_bound) in list.iter().zip(face.bounds()) {
                let Attribute::EntityRef(bound) = bound else {
                    continue;
                };
                if !matches!(self.kind(*bound), Some("FACE_BOUND" | "FACE_OUTER_BOUND")) {
                    continue;
                }
                let Some(lp) = self.reference(*bound, 1) else {
                    continue;
                };
                if self.kind(lp) != Some("EDGE_LOOP") {
                    continue;
                }
                let Some(Attribute::List(oriented)) = self.attribute(lp, 1) else {
                    continue;
                };
                if oriented.len() != read_bound.oriented_edges().count() {
                    return Err(format!(
                        "loop #{lp}'s edge count differs from the readable scene edges"
                    ));
                }
                for oe in oriented {
                    let Attribute::EntityRef(oe) = oe else {
                        continue;
                    };
                    if self.kind(*oe) != Some("ORIENTED_EDGE") {
                        continue;
                    }
                    if let Some(edge) = self.reference(*oe, 3)
                        && self.kind(edge) == Some("EDGE_CURVE")
                    {
                        written.push(edge);
                    }
                }
            }
        }
        let read = face
            .bounds()
            .flat_map(|b| b.edges().collect::<Vec<_>>())
            .map(|e| self.id(e.key()).map_err(|e| e.to_string()))
            .collect::<Result<Vec<u64>, String>>()?;
        if written != read {
            return Err("its loops' edges are not the edges read".into());
        }
        Ok(())
    }

    fn check_edge(&self, edge: &StepEdge<'_>, id: u64) -> Result<(), String> {
        let m::EntityKey::EdgeCurve(i) = edge.key() else {
            return Err(format!("an edge of type {}", step_name(edge.key())));
        };
        let e = self
            .model
            .get(m::EdgeCurveId(i.0))
            .ok_or("a dangling reference")?;
        let kind = self.kind(id).unwrap_or("nothing");
        if kind != "EDGE_CURVE" {
            return Err(format!("it is {kind}"));
        }
        for (index, vertex) in [(1, &e.edge_start), (2, &e.edge_end)] {
            let written = self.reference(id, index).ok_or("no vertex reference")?;
            let typed = vertex.entity_key();
            if self.kind(written) != Some(&step_name(typed)) {
                return Err(format!("vertex #{written} is not {}", step_name(typed)));
            }
            let read = match vertex {
                m::VertexRef::VertexPoint(v) => {
                    match &self
                        .model
                        .get(m::VertexPointId(v.0))
                        .ok_or("a dangling reference")?
                        .vertex_geometry
                    {
                        m::PointRef::CartesianPoint(c) => Some(
                            self.model
                                .get(m::CartesianPointId(c.0))
                                .ok_or("a dangling reference")?
                                .coordinates
                                .to_vec(),
                        ),
                        _ => None,
                    }
                }
                _ => None,
            };
            let written_point = (self.kind(written) == Some("VERTEX_POINT"))
                .then(|| self.reference(written, 1).and_then(|p| self.raw_point(p)))
                .flatten();
            if !same_point(written_point.as_deref(), read.as_deref()) {
                return Err(format!("vertex #{written} is not where it was read"));
            }
        }
        let curve = self.reference(id, 3).ok_or("no curve reference")?;
        let typed = e.edge_geometry.entity_key();
        if self.kind(curve) != Some(&step_name(typed)) {
            return Err(format!("curve #{curve} is not {}", step_name(typed)));
        }
        if logical(self.attribute(id, 4)) != Some(e.same_sense) {
            return Err(format!("its same_sense is not {}", e.same_sense));
        }
        Ok(())
    }
}

/// A BOOLEAN attribute (`.T.` / `.F.`).
fn logical(a: Option<&Attribute>) -> Option<bool> {
    match a? {
        Attribute::Enum(e) if e == "T" => Some(true),
        Attribute::Enum(e) if e == "F" => Some(false),
        _ => None,
    }
}

/// Points exactly equal: step-io's typed coordinates are the same parsed REAL tokens (an
/// INTEGER written where a REAL belongs converts exactly), so any difference means the two are
/// not the same instance. Equal as numbers, not bit for bit: step-io reads a written `-0.0` as
/// `0.0` (NIST CTC-04, FTC-07 and FTC-10, edition 2).
fn same_point(written: Option<&[f64]>, read: Option<&[f64]>) -> bool {
    match (written, read) {
        (Some(a), Some(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x == y),
        (None, None) => true,
        _ => false,
    }
}

/// The STEP entity type of a typed key (`BSplineSurfaceWithKnots` → `B_SPLINE_SURFACE_WITH_KNOTS`),
/// `COMPLEX` for a complex instance.
fn step_name(key: m::EntityKey) -> String {
    let debug = format!("{key:?}");
    let variant = debug.split('(').next().unwrap_or_default();
    if variant == "ComplexEntity" {
        return "COMPLEX".into();
    }
    let mut out = String::new();
    for (i, c) in variant.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
    }
    out
}

/// Read a STEP file's bytes into a [`Part`].
pub fn read_step(bytes: &[u8]) -> Result<Part, StepError> {
    read_step_placed(bytes, &IDENTITY)
}

/// Read a STEP file's bytes into a [`Part`] moved by *outer*, a proper rigid motion applied
/// above every instance placement, as if the file's roots were placed by it.
pub fn read_step_placed(bytes: &[u8], outer: &Placement) -> Result<Part, StepError> {
    read_placed(bytes, outer).map(|read| read.part)
}

/// A product definition the reader placed (an assembly or a part), as file instances.
struct PlacedDefinition {
    /// The `#N` of its `product_definition` (or the `_with_associated_documents` subtype).
    definition: u64,
    /// The `#N` of the `product_definition_shape` its shape is represented through, or why
    /// there is not exactly one.
    shape: Result<u64, String>,
    /// Its product's name ([`product_name`]).
    name: String,
    /// Its product's id ([`product_id`]).
    product_id: String,
    placement: Placement,
}

/// A part read with what placed each of its instances.
struct Read {
    part: Part,
    /// Per instance ([`Source::instance`]), the index in `placed` of the definition placed
    /// there, if a product definition holds it.
    instances: Vec<Option<usize>>,
    placed: Vec<PlacedDefinition>,
}

fn read_placed(bytes: &[u8], outer: &Placement) -> Result<Read, StepError> {
    validate_placement(outer)?;
    build(&Parsed::new(bytes)?, Scope::Placed(*outer))
}

/// A STEP file parsed, its shapes checked complete, before any geometry is built.
struct Parsed {
    model: m::StepModel,
    report: step_io::Report,
    graph: step_io::parser::Graph,
}

#[cfg(test)]
thread_local! {
    static GEOMETRY_PARSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Parsed {
    fn new(bytes: &[u8]) -> Result<Parsed, StepError> {
        #[cfg(test)]
        GEOMETRY_PARSES.with(|n| n.set(n.get() + 1));
        let parse_error = |e| StepError::Parse(crate::p21::parse_message(bytes, &e));
        let (model, report) = step_io::read(bytes).map_err(parse_error)?;
        let graph = step_io::parser::parse(bytes).map_err(parse_error)?;
        let complex_drops = crate::express::complex_reference_drops(&graph);
        refuse_dropped_shapes(&graph, &report, &complex_drops)?;
        check_curve_wrappers(&model)?;
        Ok(Parsed {
            model,
            report,
            graph,
        })
    }
}

/// Which shapes [`build`] reads, and where.
#[derive(Clone, Copy)]
enum Scope<'a> {
    /// Every instance, the file's roots placed by this proper rigid motion.
    Placed(Placement),
    /// Only these whole-file instance numbers, at their original placements.
    Selected(&'a [usize]),
    /// Only the instances of product definition `#N` where it is first placed, in its own
    /// coordinates (as its shape representation states them, no placement applied).
    Own(u64),
}

fn build(parsed: &Parsed, scope: Scope<'_>) -> Result<Read, StepError> {
    let Parsed {
        model,
        report,
        graph,
    } = parsed;
    let outer = match scope {
        Scope::Placed(outer) => outer,
        Scope::Own(_) | Scope::Selected(_) => IDENTITY,
    };
    let outer = &outer;
    let mut ids = FileIds::new(graph, model, report);
    let scene = model.scene();
    let units = scene.units();
    let mut reader = Reader {
        model,
        to_mm: units.length.as_ref().map_or(1.0, |u| u.to_si * 1000.0),
        to_rad: units.angle.as_ref().map_or(1.0, |u| u.to_si),
        placement: IDENTITY,
    };

    let faces_by_key: HashMap<m::EntityKey, StepFace<'_>> =
        scene.all_faces().map(|f| (f.key(), f)).collect();
    let face_of = |r: &m::FaceRef| -> Option<StepFace<'_>> {
        let key = match r {
            m::FaceRef::AdvancedFace(i) => m::EntityKey::AdvancedFace(*i),
            m::FaceRef::FaceSurface(i) => m::EntityKey::FaceSurface(*i),
            _ => return None,
        };
        faces_by_key.get(&key).copied()
    };

    // Shells in the order OpenCascade's explorer meets them: placed solid instances, then open
    // shells. Each is read as one instance (a placed solid, or a placement of a surface model),
    // with the placed definition it belongs to.
    let mut shells = Vec::new();
    let mut instance_of: Vec<Option<usize>> = Vec::new();
    let (mut instances, mut placed) = (Vec::new(), Vec::new());
    let mut placement_paths = PlacementPaths::default();
    for root in scene.root_definitions() {
        collect_instances(
            root,
            *outer,
            reader.to_mm,
            &mut instances,
            &mut placed,
            0,
            &mut placement_paths,
        )?;
    }
    if instances.is_empty() {
        instances = scene.all_solids().map(|s| (s, *outer, None)).collect();
    }
    for (solid, placement, def) in instances {
        // The outer shell, then each void shell (whose faces face inward, hence the flip).
        let mut faces: Vec<(StepFace<'_>, bool)> = solid.faces().map(|f| (f, false)).collect();
        for (shell, orientation) in solid.voids() {
            let flip = orientation == step_io::Orientation::Reversed;
            faces.extend(shell.faces().map(|f| (f, flip)));
        }
        shells.push((true, placement, faces, instance_of.len()));
        instance_of.push(def);
    }
    let rg = model.ref_graph();
    for index in reachable_surface_models(model, &rg) {
        let sbsm = model.get(index).ok_or_else(dangling)?;
        // Placed like the assembly instances of the product that owns it, once per instance.
        let owners = surface_model_owners(model, &rg, index);
        let mut placements: Vec<(Placement, Option<usize>)> = placed
            .iter()
            .enumerate()
            .filter(|(_, (def, _))| owners.contains(&def.key()))
            .map(|(i, (_, p))| (*p, Some(i)))
            .collect();
        if placements.is_empty() {
            placements.push((*outer, None));
        }
        for (placement, def) in placements {
            let instance = instance_of.len();
            instance_of.push(def);
            for shell in &sbsm.sbsm_boundary {
                let faces = match shell {
                    m::ShellRef::OpenShell(i) => {
                        &model
                            .get(m::OpenShellId(i.0))
                            .ok_or_else(dangling)?
                            .cfs_faces
                    }
                    m::ShellRef::ClosedShell(i) => {
                        &model
                            .get(m::ClosedShellId(i.0))
                            .ok_or_else(dangling)?
                            .cfs_faces
                    }
                    _ => continue,
                };
                shells.push((
                    false,
                    placement,
                    faces
                        .iter()
                        .filter_map(&face_of)
                        .map(|f| (f, false))
                        .collect(),
                    instance,
                ));
            }
        }
    }

    // Select after enumerating every instance, before building any geometry. Filtering the
    // enumeration itself would renumber the retained bodies and lose their source identity.
    let selected = if let Scope::Selected(keep) = scope {
        if keep.is_empty() {
            return Err(StepError::Unsupported(
                "no placed body instances selected".into(),
            ));
        }
        let mut selected = vec![false; instance_of.len()];
        for &instance in keep {
            let entry = selected.get_mut(instance).ok_or_else(|| {
                StepError::Unsupported(format!(
                    "placed body instance {instance} is out of range ({} instances)",
                    instance_of.len()
                ))
            })?;
            *entry = true;
        }
        Some(selected)
    } else {
        None
    };

    // Own frame: only the shells of the definition's first placement are read, unplaced; every
    // instance keeps the number the whole file's read gives it.
    let own = match scope {
        Scope::Placed(_) | Scope::Selected(_) => None,
        Scope::Own(definition) => {
            let mut first = None;
            for (index, (def, _)) in placed.iter().enumerate() {
                if ids.id(def.key())? == definition {
                    first = Some(index);
                    break;
                }
            }
            Some(first.ok_or_else(|| {
                StepError::Unsupported(format!(
                    "product definition #{definition} is not placed in the file"
                ))
            })?)
        }
    };

    let (mut out_faces, mut edges_out, mut solids) = (Vec::new(), Vec::new(), Vec::new());
    let (mut unresolved_faces, mut unresolved_edges) = (Vec::new(), Vec::new());
    let (mut face_sources, mut edge_sources) = (Vec::new(), Vec::new());
    let mut line_directions = std::collections::BTreeMap::new();
    let mut conics = std::collections::BTreeMap::new();
    let mut pcurve_splines = Vec::new();
    let (mut face_frames, mut edge_frames) = (
        std::collections::BTreeMap::new(),
        std::collections::BTreeMap::new(),
    );
    let mut swept = std::collections::BTreeMap::new();
    let mut file_pcurves = std::collections::BTreeMap::new();
    let mut primary_pcurves = std::collections::BTreeMap::new();
    let (mut native_faces, mut native_edges) = (Vec::new(), Vec::new());
    let mut vertex_count = 0;
    for (is_solid, placement, faces, instance) in shells {
        if selected.as_ref().is_some_and(|keep| !keep[instance]) {
            continue;
        }
        reader.placement = match own {
            None => placement,
            Some(own) if instance_of[instance] == Some(own) => IDENTITY,
            Some(_) => continue,
        };
        let context = std::sync::Arc::new(GeometryPlacement::new(
            if own.is_some() { IDENTITY } else { *outer },
            if own.is_some() {
                std::sync::Arc::from([])
            } else {
                instance_of[instance]
                    .map(|i| placement_paths.definitions[i].clone())
                    .unwrap_or_else(|| std::sync::Arc::from([]))
            },
            if own.is_some() {
                std::sync::Arc::from([])
            } else {
                instance_of[instance]
                    .map(|i| placement_paths.raw[i].clone())
                    .unwrap_or_else(|| std::sync::Arc::from([]))
            },
            // Read in its own frame, a definition is its own root.
            match instance_of[instance] {
                Some(i) if own.is_some() => placement_paths.products[i]
                    .last()
                    .cloned()
                    .into_iter()
                    .collect(),
                Some(i) => placement_paths.products[i].clone(),
                None => std::sync::Arc::from([]),
            },
        ));
        // Edges are shared within one placed shell only; another instance gets its own copies.
        let mut edge_index: HashMap<m::EntityKey, usize> = HashMap::new();
        let mut vertex_index: HashMap<m::EntityKey, usize> = HashMap::new();
        let mut vertex_id = |key: m::EntityKey| {
            let next = vertex_count + vertex_index.len();
            *vertex_index.entry(key).or_insert(next)
        };
        let mut members = Vec::new();
        for (face, flipped) in faces {
            // ADVANCED_FACE and FACE_SURFACE bounds are SET [1:?]. step-io can
            // retain a typed reference to an incompatible complex instance while
            // its scene iterator skips that bound. Refuse before building a Face.
            if face.bounds().next().is_none() {
                return Err(StepError::Incomplete(format!(
                    "face #{} has no readable bounds (expected at least one face loop)",
                    ids.id(face.key())?
                )));
            }
            // Validate raw/scene cardinalities before building partial topology.
            ids.face(&face)?;
            let mut loops = Vec::new();
            for bound in face.bounds() {
                let mut edges = Vec::new();
                for (edge, forward) in bound.oriented_edges() {
                    if edges.len() >= limits::LOOP_EDGES {
                        return Err(StepError::Limit(format!(
                            "a face bound of more than {} edges",
                            limits::LOOP_EDGES
                        )));
                    }
                    let index = match edge_index.get(&edge.key()) {
                        Some(&i) => i,
                        None => {
                            let (read, resolved) = reader.edge(&edge, &mut vertex_id)?;
                            if !resolved {
                                unresolved_edges.push(edges_out.len());
                            }
                            // A substituted chord keeps its own parameterisation, even if
                            // the unresolved STEP entity names an analytic curve.
                            if resolved
                                && matches!(&read.curve, Curve::Line { .. })
                                && let Some(d) =
                                    reader.line_direction(&raw_edge_curve(reader.model, &edge))
                            {
                                line_directions.insert(edges_out.len(), d);
                            }
                            let native = if reader.placement != IDENTITY {
                                let native_reader = Reader {
                                    placement: IDENTITY,
                                    ..reader
                                };
                                let mut native = native_reader.edge(&edge, &mut vertex_id)?.0;
                                native.samples = Vec::new();
                                Some(std::sync::Arc::new(native))
                            } else {
                                None
                            };
                            let native_reader = Reader {
                                placement: IDENTITY,
                                ..reader
                            };
                            if resolved
                                && matches!(&read.curve, Curve::Nurbs(_))
                                && let Some(h) = native_reader.conic(&edge)
                            {
                                conics.insert(edges_out.len(), h);
                            }
                            if let Some(frame) =
                                reader.curve_raw_frame(&raw_edge_curve(reader.model, &edge))
                            {
                                edge_frames.insert(edges_out.len(), frame);
                            }
                            native_edges.push((native, context.clone()));
                            edges_out.push(read);
                            edge_sources.push(Source {
                                entity: ids.edge(&edge)?,
                                instance,
                            });
                            edge_index.insert(edge.key(), edges_out.len() - 1);
                            edges_out.len() - 1
                        }
                    };
                    edges.push((index, forward == step_io::Orientation::Forward));
                }
                // A face of a void shell used reversed is itself used reversed, its bounds with
                // it (ISO 10303-42 `oriented_face`; OpenCascade's reversed shell composes onto
                // its faces' edges the same way).
                if (bound.orientation() == step_io::Orientation::Forward) == flipped {
                    edges.reverse();
                    for e in &mut edges {
                        e.1 = !e.1;
                    }
                }
                let vertex = if edges.is_empty() {
                    reader.vertex_loop(&bound)
                } else {
                    None
                };
                if edges.is_empty() && vertex.is_none() {
                    return Err(StepError::Incomplete(format!(
                        "face #{} has a loop with no edges or vertex",
                        ids.id(face.key())?
                    )));
                }
                loops.push(Loop { edges, vertex });
            }
            members.push(out_faces.len());
            // A surface no form resolves stays in the part, unevaluable, and is recorded.
            let surface = reader.surface(&face).unwrap_or_else(|| {
                unresolved_faces.push(out_faces.len());
                Surface::Other { kind: "UNRESOLVED" }
            });
            close_cone_at_apex(&surface, &mut loops, &edges_out);
            let (pcurves, splines) = reader.pcurves(&face, &surface, &edge_index, &edges_out);
            pcurve_splines.push(splines);
            if let Some(frame) = reader.surface_raw_frame(&face) {
                face_frames.insert(out_faces.len(), frame);
            }
            if matches!(surface, Surface::Freeform { .. })
                && let Some(source) = reader.swept_source(&face)
            {
                swept.insert(out_faces.len(), source);
            }
            let collect_preferred =
                matches!(&surface,Surface::Freeform{kind,..} if matches!(*kind,"BSPLINE"|"BEZIER"));
            let (raw, primary) = reader.file_pcurves(&face, &edge_index, collect_preferred);
            if !primary.is_empty() {
                primary_pcurves.insert(out_faces.len(), primary);
            }
            if !raw.is_empty() {
                file_pcurves.insert(out_faces.len(), raw);
            }
            let placed_face = Face {
                surface,
                reversed: (face.orientation() == step_io::Orientation::Forward) == flipped,
                loops,
                solid: is_solid.then_some(solids.len()),
                pcurves,
            };
            let native = if reader.placement != IDENTITY {
                let native_reader = Reader {
                    placement: IDENTITY,
                    ..reader
                };
                let mut native = placed_face.clone();
                native.surface = native_reader
                    .surface(&face)
                    .unwrap_or(Surface::Other { kind: "UNRESOLVED" });
                let bounds: Vec<_> = face.bounds().collect();
                for (i, lp) in native.loops.iter_mut().enumerate() {
                    if lp.edges.is_empty() {
                        lp.vertex = match bounds.get(i) {
                            Some(bound) => native_reader.vertex_loop(bound),
                            None => native.surface.cone_apex(),
                        };
                    }
                }
                Some(std::sync::Arc::new(native))
            } else {
                None
            };
            if out_faces.len() >= limits::FACES {
                return Err(StepError::Limit(format!(
                    "more than {} faces across the placed instances",
                    limits::FACES
                )));
            }
            out_faces.push(placed_face);
            native_faces.push((native, context.clone()));
            face_sources.push(Source {
                entity: ids.face(&face)?,
                instance,
            });
        }
        drop(vertex_id);
        vertex_count += vertex_index.len();
        if is_solid {
            solids.push(Solid { faces: members });
        }
    }
    if out_faces.is_empty() {
        return Err(StepError::Incomplete("no solid or shell with faces".into()));
    }
    let placed = placed
        .into_iter()
        .map(|(def, placement)| {
            let definition = ids.id(def.key())?;
            Ok(PlacedDefinition {
                definition,
                shape: ids.definition_shape(&def, &rg),
                name: product_name(&graph.entities, definition),
                product_id: product_id(&graph.entities, definition),
                placement,
            })
        })
        .collect::<Result<_, StepError>>()?;
    let part = Part::new(out_faces, edges_out, solids)
        .with_unresolved(unresolved_faces, unresolved_edges)
        .with_sources(face_sources, edge_sources)
        .with_line_directions(line_directions)
        .with_conics(conics)
        .with_pcurve_splines(pcurve_splines)
        .with_raw_frames(face_frames, edge_frames)
        .with_swept(swept)
        .with_file_pcurves(
            file_pcurves,
            primary_pcurves,
            (reader.to_mm, reader.to_rad),
            units.precision.map(|p| p * reader.to_mm),
        )
        .with_native_geometry(native_faces, native_edges)
        .repair_inverted_shells();
    Ok(Read {
        part,
        instances: instance_of,
        placed,
    })
}

/// One distinct part of a file: a product definition whose shape holds faces, with the numbering
/// of its faces and edges that PMI anchors to. Faces are numbered as specify-core numbers them
/// (OpenCascade's `TopExp::MapShapes` over the part's own shape, not over a placed instance).
/// Edges are haecceity's own numbering, one per `EDGE_CURVE`, in the order the faces' loops give
/// them: it is not specify-core's, whose edges come after OpenCascade's healing (which adds seam
/// and split edges, and reverses some loops on geometric grounds), so an edge index from
/// specify-core does not carry over; anchor edges by their `#N`
/// (`tests/fixtures/references/face_sources.json` lists every file where the two differ).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct PartDefinition {
    /// The `#N` of its `product_definition` (or `product_definition_with_associated_documents`).
    pub product_definition: u64,
    /// The `#N` of the `product_definition_shape` its shape is represented through, which the
    /// part's shape aspects name as `of_shape`.
    pub shape: u64,
    /// Its display name, as OpenCascade's XCAF gives it and specify-core reports it: the name
    /// of an assembly that holds the part as its only component (an exporter's wrapper round a
    /// solid), else its product's name ([`product_name`]: `PRODUCT.name`, or `PRODUCT.id` when
    /// the name is empty, escapes decoded).
    pub name: String,
    /// Its product's id: `PRODUCT.id`, escapes decoded, as the file states it whatever the
    /// name (empty when the product chain does not resolve). Reported beside the part's index,
    /// which is how a part is identified; it need not be unique (exporters often repeat the
    /// name).
    pub product_id: String,
    /// Face index → the `#N` of the `ADVANCED_FACE` (or `FACE_SURFACE`) it was read from.
    pub faces: Vec<u64>,
    /// Edge index → the `#N` of its `EDGE_CURVE`.
    pub edges: Vec<u64>,
    /// Where each instance of the part is placed, in the reader's order.
    pub placements: Vec<PartPlacement>,
    face_index: HashMap<u64, usize>,
    edge_index: HashMap<u64, usize>,
}

/// One placement of a part: rows of `[R | t]` with `t` in millimetres, and the instances of the
/// [`Part`] read with it ([`crate::brep::Source::instance`]): one per solid or surface model of
/// the part.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct PartPlacement {
    pub placement: Placement,
    pub instances: Vec<usize>,
}

impl PartDefinition {
    /// The index of the face read from `ADVANCED_FACE` *entity*, if it is one of this part's.
    pub fn face_index(&self, entity: u64) -> Option<usize> {
        self.face_index.get(&entity).copied()
    }

    /// The index of the edge read from `EDGE_CURVE` *entity*, if it is one of this part's.
    pub fn edge_index(&self, entity: u64) -> Option<usize> {
        self.edge_index.get(&entity).copied()
    }
}

/// A product definition that holds others: the relating side of one or more
/// `next_assembly_usage_occurrence`s.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "manifest", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct Assembly {
    /// The `#N` of its `product_definition`.
    pub product_definition: u64,
    /// Its product's name ([`product_name`]).
    pub name: String,
    /// Its components, in file order (ascending `#N` of their occurrences).
    pub components: Vec<Component>,
}

/// One component of an [`Assembly`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "manifest", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct Component {
    /// The `#N` of the `next_assembly_usage_occurrence` that places it.
    pub occurrence: u64,
    /// The `#N` of the `product_definition` it places.
    pub definition: u64,
}

/// A STEP file read once: its distinct parts and its assemblies, and each part's [`Part`] in
/// its own coordinates ([`StepFile::part`]), or chosen placed bodies ([`StepFile::placed`]),
/// on demand without parsing the file again.
#[non_exhaustive]
pub struct StepFile {
    parsed: Parsed,
    /// The distinct parts, as [`read_part_definitions`] gives them.
    pub parts: Vec<PartDefinition>,
    /// The assemblies, depth first from the roots (the assemblies no occurrence places,
    /// ascending `#N`), each where it is first met, as specify-core visits XCAF's assembly
    /// labels.
    pub assemblies: Vec<Assembly>,
}

impl StepFile {
    /// Reads `bytes`; refused as [`read_part_definitions`] refuses.
    pub fn read(bytes: &[u8]) -> Result<StepFile, StepError> {
        let parsed = Parsed::new(bytes)?;
        let assemblies = assemblies(&parsed.graph.entities);
        let parts = part_definitions(build(&parsed, Scope::Placed(IDENTITY))?, &assemblies)?;
        Ok(StepFile {
            parsed,
            parts,
            assemblies,
        })
    }

    /// Part *index* of [`StepFile::parts`] in its own coordinates, as [`read_part`] reads it.
    /// Panics if there is no such part, as indexing does.
    pub fn part(&self, index: usize) -> Result<Part, StepError> {
        own_part(&self.parsed, &self.parts[index])
    }

    /// Read the placed bodies selected by their whole-file [`Source::instance`] numbers,
    /// as [`read_step`] places them. These are body-instance numbers, not indices into
    /// [`Self::parts`]; a part definition can have several bodies and placements.
    ///
    /// Every retained face and edge keeps its original source entity and instance number.
    /// Local face, edge, vertex and solid indices are rebuilt, including unresolved-face
    /// and unresolved-edge indices. Geometry stays in file coordinates, without merging
    /// coincident bodies or deciding which copies a caller should keep.
    ///
    /// `keep` is a set: duplicates are ignored and the result follows whole-file traversal
    /// order, regardless of selection order. An empty selection or any unknown instance
    /// number returns [`StepError::Unsupported`]. The parsed file is reused; the selected
    /// geometry is built afresh and existing parts are not changed.
    pub fn placed(&self, keep: &[usize]) -> Result<Part, StepError> {
        build(&self.parsed, Scope::Selected(keep)).map(|read| read.part)
    }
}

/// One distinct part of a file (*part*, from [`read_part_definitions`] of the same bytes) as a
/// [`Part`] in its own coordinates: its shape where it is first placed, read with no placement
/// applied, so a part placed several times, or placed away from the origin, is read once as its
/// shape representation states it. Faces are in `part.faces` order and edges in the order its
/// loops first use them; each face and edge keeps its source (`Source::instance` is the
/// instance's number in the whole file's read, one of `part.placements[0].instances`) and the
/// unresolved faces and edges are recorded as [`read_step`] records them. No placement is
/// inverted, so none needs to be invertible. Refused, naming the part, when its product
/// definition is not placed in the file or a face of *part* is not read exactly once.
pub fn read_part(bytes: &[u8], part: &PartDefinition) -> Result<Part, StepError> {
    own_part(&Parsed::new(bytes)?, part)
}

/// Parsed geometry shared by all callout-bearing parts of one PMI write. Unlike StepFile,
/// the caller already has checked part definitions, so no placed assembly is rebuilt.
pub(crate) struct PartReader(Parsed);
impl PartReader {
    pub(crate) fn read(bytes: &[u8]) -> Result<Self, StepError> {
        Parsed::new(bytes).map(Self)
    }
    pub(crate) fn part(&self, def: &PartDefinition) -> Result<Part, StepError> {
        own_part(&self.0, def)
    }
}

fn own_part(parsed: &Parsed, def: &PartDefinition) -> Result<Part, StepError> {
    let refuse = |why: String| {
        StepError::Unsupported(format!(
            "part #{} ({}): {why}",
            def.product_definition, def.name
        ))
    };
    let part = build(parsed, Scope::Own(def.product_definition))
        .map_err(|e| refuse(e.to_string()))?
        .part;
    let mut read: HashMap<u64, usize> = HashMap::new();
    for f in 0..part.faces.len() {
        if let Some(s) = part.face_source(f) {
            *read.entry(s.entity).or_default() += 1;
        }
    }
    for (index, entity) in def.faces.iter().enumerate() {
        let n = read.get(entity).copied().unwrap_or(0);
        if n != 1 {
            return Err(refuse(format!(
                "face {index} (#{entity}) is read {n} times, not once"
            )));
        }
    }
    let in_order = part.faces.len() == def.faces.len()
        && (0..part.faces.len())
            .all(|f| part.face_source(f).map(|s| s.entity) == Some(def.faces[f]));
    if !in_order {
        return Err(refuse(format!(
            "its shape reads {} faces, not its {} in their order",
            part.faces.len(),
            def.faces.len()
        )));
    }
    Ok(part)
}

/// A product definition's product name as OpenCascade's XCAF names its label:
/// `PRODUCT_DEFINITION.formation` → `PRODUCT_DEFINITION_FORMATION.of_product` →
/// `PRODUCT.name`, or `PRODUCT.id` when the name is empty (NIST FTC-08 and CTC-04 state their
/// title as the id), with escapes decoded by step-io. Malformed escapes are retained
/// by the parser (a name is never refused for that); empty when the
/// chain does not resolve.
fn product_name(graph: &std::collections::BTreeMap<u64, RawEntity>, definition: u64) -> String {
    let [id, name] = product_texts(graph, definition);
    if name.is_empty() { id } else { name }
}

/// A product definition's `PRODUCT.id`, through the same chain as [`product_name`] and decoded
/// the same way; empty when the chain does not resolve.
fn product_id(graph: &std::collections::BTreeMap<u64, RawEntity>, definition: u64) -> String {
    let [id, _] = product_texts(graph, definition);
    id
}

/// `[PRODUCT.id, PRODUCT.name]` of a product definition (see [`product_name`]).
fn product_texts(
    graph: &std::collections::BTreeMap<u64, RawEntity>,
    definition: u64,
) -> [String; 2] {
    let attribute = |id: u64, index: usize| match graph.get(&id)? {
        RawEntity::Simple { attributes, .. } => attributes.get(index),
        RawEntity::Complex { .. } => None,
    };
    let reference = |id: u64, index: usize| match attribute(id, index)? {
        Attribute::EntityRef(r) => Some(*r),
        _ => None,
    };
    let Some(product) = reference(definition, 2).and_then(|formation| reference(formation, 2))
    else {
        return [String::new(), String::new()];
    };
    let text = |index: usize| match attribute(product, index) {
        Some(Attribute::String(raw)) => raw.clone(),
        _ => String::new(),
    };
    [text(0), text(1)]
}

/// The file's assemblies (see [`StepFile::assemblies`]).
fn assemblies(graph: &std::collections::BTreeMap<u64, RawEntity>) -> Vec<Assembly> {
    use std::collections::{BTreeMap, HashSet};
    let reference = |a: Option<&Attribute>| match a {
        Some(Attribute::EntityRef(n)) => Some(*n),
        _ => None,
    };
    // Assembly → its components, in file order (the graph iterates ids ascending).
    let mut components: BTreeMap<u64, Vec<Component>> = BTreeMap::new();
    let mut placed = HashSet::new();
    for (&occurrence, entity) in graph {
        if let RawEntity::Simple {
            name, attributes, ..
        } = entity
            && name == "NEXT_ASSEMBLY_USAGE_OCCURRENCE"
            && let (Some(relating), Some(definition)) =
                (reference(attributes.get(3)), reference(attributes.get(4)))
        {
            components.entry(relating).or_default().push(Component {
                occurrence,
                definition,
            });
            placed.insert(definition);
        }
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut stack: Vec<u64> = components
        .keys()
        .copied()
        .filter(|k| !placed.contains(k))
        .rev()
        .collect();
    while let Some(definition) = stack.pop() {
        let Some(children) = components.get(&definition) else {
            continue;
        };
        if !seen.insert(definition) {
            continue;
        }
        stack.extend(children.iter().rev().map(|c| c.definition));
        out.push(Assembly {
            product_definition: definition,
            name: product_name(graph, definition),
            components: children.clone(),
        });
    }
    out
}

/// The distinct parts of a STEP file, in specify-core's order (depth first through the assembly
/// from its roots, each part where it is first placed; a part placed twice is one part with two
/// placements), each with its faces and edges numbered as in its own shape. Refused when a shape
/// with faces belongs to no product definition (nothing to anchor its PMI to), or a part's
/// product definition has not exactly one represented `product_definition_shape`.
/// [`StepFile::read`] gives the same parts with the assemblies, and each part's [`Part`].
pub fn read_part_definitions(bytes: &[u8]) -> Result<Vec<PartDefinition>, StepError> {
    StepFile::read(bytes).map(|file| file.parts)
}

/// The distinct parts of a whole file's read (see [`read_part_definitions`]), each named after
/// an assembly of `assemblies` that holds it alone, if one does.
fn part_definitions(read: Read, assemblies: &[Assembly]) -> Result<Vec<PartDefinition>, StepError> {
    let Read {
        part,
        instances,
        placed,
    } = read;
    if let Some(orphan) = instances.iter().position(Option::is_none) {
        return Err(StepError::Unsupported(format!(
            "instance {orphan} of the file's shapes belongs to no product definition"
        )));
    }
    let mut parts: Vec<PartDefinition> = Vec::new();
    for (index, def) in placed.iter().enumerate() {
        let mine: Vec<usize> = (0..instances.len())
            .filter(|&i| instances[i] == Some(index))
            .collect();
        if mine.is_empty() {
            continue;
        }
        let placement = PartPlacement {
            placement: def.placement,
            instances: mine.clone(),
        };
        if let Some(known) = parts
            .iter_mut()
            .find(|p| p.product_definition == def.definition)
        {
            known.placements.push(placement);
            continue;
        }
        let shape = def.shape.clone().map_err(|why| {
            StepError::Unsupported(format!("part #{} ({}): {why}", def.definition, def.name))
        })?;
        let ours: Vec<usize> = (0..part.faces.len())
            .filter(|&f| {
                part.face_source(f)
                    .is_some_and(|s| mine.contains(&s.instance))
            })
            .collect();
        let faces: Vec<u64> = ours
            .iter()
            .filter_map(|&f| part.face_source(f).map(|s| s.entity))
            .collect();
        // Edges as OpenCascade's explorer meets them: face by face, loop by loop, a loop's edges
        // in the order the file lists them, backwards when the bound's orientation differs from
        // the face's `same_sense` (a loop's edges here are already backwards when its bound is
        // used reversed, and again when its face is, in a void shell used reversed: so backwards
        // exactly when the face is reversed); an edge two faces or two of the part's solids
        // share is one edge.
        let mut edges: Vec<u64> = Vec::new();
        let mut edge_index = HashMap::new();
        for &f in &ours {
            let in_order = |l: &Loop| -> Vec<usize> {
                let order = l.edges.iter().map(|&(e, _)| e);
                if !part.faces[f].reversed {
                    order.collect()
                } else {
                    order.rev().collect()
                }
            };
            for e in part.faces[f].loops.iter().flat_map(in_order) {
                if let Some(s) = part.edge_source(e)
                    && !edge_index.contains_key(&s.entity)
                {
                    edge_index.insert(s.entity, edges.len());
                    edges.push(s.entity);
                }
            }
        }
        let face_index = faces.iter().enumerate().map(|(i, &f)| (f, i)).collect();
        // Named after the first assembly (in visiting order) that holds it alone and has a
        // name, as specify-core's `_part_name` names it.
        let name = assemblies
            .iter()
            .find(|a| {
                matches!(a.components[..], [c] if c.definition == def.definition)
                    && !a.name.is_empty()
            })
            .map_or_else(|| def.name.clone(), |a| a.name.clone());
        parts.push(PartDefinition {
            product_definition: def.definition,
            shape,
            name,
            product_id: def.product_id.clone(),
            faces,
            edges,
            placements: vec![placement],
            face_index,
            edge_index,
        });
    }
    Ok(parts)
}

/// Refuses a file whose shapes lost entities when step-io read it: an entity a shape
/// representation reaches through its items that is missing from the file or was dropped
/// (malformed, or of a type step-io does not model), or a shape representation dropped itself.
/// Entities no shape reaches (presentation, PMI) may be dropped without harm.
fn refuse_dropped_shapes(
    graph: &step_io::parser::Graph,
    report: &step_io::Report,
    complex_drops: &[(u64, String)],
) -> Result<(), StepError> {
    use std::collections::{BTreeSet, HashSet};
    use step_io::parser::{Attribute, RawEntity};

    fn refs(a: &Attribute, out: &mut Vec<u64>) {
        match a {
            Attribute::EntityRef(n) => out.push(*n),
            Attribute::List(l) => l.iter().for_each(|x| refs(x, out)),
            Attribute::Typed { value, .. } => refs(value, out),
            _ => {}
        }
    }
    fn name(e: &RawEntity) -> String {
        match e {
            RawEntity::Simple { name, .. } => name.clone(),
            RawEntity::Complex { parts, .. } => {
                let names: Vec<&str> = parts.iter().map(|p| p.name.as_str()).collect();
                format!("({})", names.join(" "))
            }
        }
    }

    if report.dropped.is_empty() && complex_drops.is_empty() {
        return Ok(());
    }
    let dropped: HashMap<u64, (step_io::DropKind, &str)> = complex_drops
        .iter()
        .map(|(id, detail)| {
            (
                *id,
                (step_io::DropKind::NonstandardReference, detail.as_str()),
            )
        })
        .chain(
            report
                .dropped
                .iter()
                .map(|(id, r)| (*id, (r.kind.clone(), r.detail.as_str()))),
        )
        .collect();
    let (mut lost, mut missing) = (BTreeSet::new(), BTreeSet::new());
    let mut stack = Vec::new();
    for (&id, entity) in &graph.entities {
        // A representation's items, not its context: units and tolerances are not shapes.
        let items = match entity {
            RawEntity::Simple {
                name, attributes, ..
            } if name.ends_with("SHAPE_REPRESENTATION") => attributes.get(1),
            RawEntity::Complex { parts, .. }
                if parts
                    .iter()
                    .any(|p| p.name.ends_with("SHAPE_REPRESENTATION")) =>
            {
                parts
                    .iter()
                    .find(|p| p.name == "REPRESENTATION")
                    .and_then(|p| p.attributes.get(1))
            }
            _ => continue,
        };
        if dropped.contains_key(&id) {
            lost.insert(id);
        }
        if let Some(items) = items {
            refs(items, &mut stack);
        }
    }
    let mut seen = HashSet::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(entity) = graph.entities.get(&id) else {
            missing.insert(id);
            continue;
        };
        if dropped.contains_key(&id) {
            lost.insert(id);
        }
        match entity {
            RawEntity::Simple { attributes, .. } => {
                attributes.iter().for_each(|a| refs(a, &mut stack))
            }
            RawEntity::Complex { parts, .. } => parts
                .iter()
                .flat_map(|p| &p.attributes)
                .for_each(|a| refs(a, &mut stack)),
        }
    }
    if lost.is_empty() && missing.is_empty() {
        return Ok(());
    }
    // Name the causes: the missing entities, and the dropped ones not dropped merely because
    // something they refer to was.
    let mut causes: Vec<String> = missing
        .iter()
        .map(|id| format!("#{id} is referenced but not in the file"))
        .collect();
    causes.extend(lost.iter().filter_map(|id| {
        let reason = &dropped[id];
        (reason.0 != step_io::DropKind::Cascade).then(|| {
            format!(
                "#{id} {} was dropped ({:?}: {})",
                name(&graph.entities[id]),
                reason.0,
                reason.1
            )
        })
    }));
    if causes.is_empty() {
        causes.extend(
            lost.iter()
                .map(|id| format!("#{id} {} was dropped", name(&graph.entities[id]))),
        );
    }
    let shown = causes.len().min(5);
    let more = if causes.len() > shown {
        format!(" and {} more", causes.len() - shown)
    } else {
        String::new()
    };
    Err(StepError::Incomplete(format!(
        "{}{more} ({} shape entities lost)",
        causes[..shown].join("; "),
        lost.len()
    )))
}

/// A cone face bounded by a single loop running round the axis (a drill point written as just
/// its rim circle, with no seam and no `VERTEX_LOOP`) can only be the part of the cone between
/// that loop and the apex — the other side is unbounded. OpenCascade's import completes it with
/// a seam and a degenerate edge at the apex; here it gets the apex's vertex loop, the form such
/// a face takes when the file writes it out, so its domain, area and bounds reach the apex.
fn close_cone_at_apex(surface: &Surface, loops: &mut Vec<Loop>, edges: &[Edge]) {
    let (Surface::Cone { frame, .. }, Some(apex)) = (surface, surface.cone_apex()) else {
        return;
    };
    if loops
        .iter()
        .any(|lp| lp.vertex.is_some() || lp.edges.is_empty())
    {
        return;
    }
    // A loop that already reaches the apex (along a seam, as OpenCascade writes a cone) bounds
    // the face there itself.
    let reach = frame.to_local(apex)[2].abs().max(1.0) * 1e-7;
    if loops.iter().flat_map(|lp| &lp.edges).any(|&(e, _)| {
        edges[e]
            .samples
            .iter()
            .any(|&p| geom::dist(p, apex) < reach)
    }) {
        return;
    }
    // Turns round the axis: the swept angle of the loop's samples, each step taken the short
    // way round.
    let turns = |lp: &Loop| {
        let angles: Vec<f64> = lp
            .edges
            .iter()
            .flat_map(|&(e, forward)| {
                let samples = &edges[e].samples;
                let ordered: Box<dyn Iterator<Item = &V3>> = if forward {
                    Box::new(samples.iter())
                } else {
                    Box::new(samples.iter().rev())
                };
                ordered.map(|&p| {
                    let l = frame.to_local(p);
                    l[1].atan2(l[0])
                })
            })
            .collect();
        let swept: f64 = angles
            .iter()
            .zip(angles.iter().cycle().skip(1))
            .map(|(a, b)| {
                let d = b - a;
                d - std::f64::consts::TAU * (d / std::f64::consts::TAU).round()
            })
            .sum();
        (swept / std::f64::consts::TAU).round() != 0.0
    };
    if loops.iter().filter(|lp| turns(lp)).count() == 1 {
        loops.push(Loop {
            edges: Vec::new(),
            vertex: Some(apex),
        });
    }
}

/// Read a STEP file from disk, transparently gunzipping `*.gz`.
pub fn read_step_file(path: &std::path::Path) -> Result<Part, Box<dyn std::error::Error>> {
    Ok(read_step(&step_file_bytes(path)?)?)
}

/// [`read_step_file`] moved by *outer* (see [`read_step_placed`]).
pub fn read_step_file_placed(
    path: &std::path::Path,
    outer: &Placement,
) -> Result<Part, Box<dyn std::error::Error>> {
    Ok(read_step_placed(&step_file_bytes(path)?, outer)?)
}

/// A STEP file's bytes, gunzipped when the name ends in `.gz`.
fn step_file_bytes(path: &std::path::Path) -> std::io::Result<Vec<u8>> {
    decode_step_file_bytes(path, std::fs::read(path)?)
}

/// Decode a file's byte snapshot, gunzipping only when its path ends in `.gz`.
/// Callers can hash the original bytes before consuming them here, without rereading the file.
pub fn decode_step_file_bytes(path: &std::path::Path, bytes: Vec<u8>) -> std::io::Result<Vec<u8>> {
    if path.extension().is_some_and(|e| e == "gz") {
        use std::io::Read;
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(&bytes[..]).read_to_end(&mut out)?;
        Ok(out)
    } else {
        Ok(bytes)
    }
}

impl Assembly {
    pub fn new(product_definition: u64, name: String, components: Vec<Component>) -> Self {
        Self {
            product_definition,
            name,
            components,
        }
    }
}
impl Component {
    pub fn new(occurrence: u64, definition: u64) -> Self {
        Self {
            occurrence,
            definition,
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "presentation")]
    #[test]
    fn pmi_writer_shares_one_geometry_parse_between_parts() {
        use crate::{
            p21::Document,
            pmi::{
                self, PartId,
                write::{Mode, PresentationPolicy},
            },
        };
        use std::io::Read;
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../tests/fixtures/ap242/write-presentation/assembly_plate_pin_intent.step.gz",
        );
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap())
            .read_to_end(&mut bytes)
            .unwrap();
        let defs = super::read_part_definitions(&bytes).unwrap();
        let doc = Document::parse(bytes).unwrap();
        let read = pmi::read(&doc, &defs).unwrap();
        assert_eq!(defs.len(), 2);
        assert!(read.parts.iter().all(|p| !p.dimensions.is_empty()));
        let requests: Vec<_> = read
            .parts
            .into_iter()
            .enumerate()
            .map(|(i, p)| (PartId(i), p))
            .collect();
        let before = super::GEOMETRY_PARSES.with(std::cell::Cell::get);
        pmi::write(
            &doc,
            &defs,
            &requests,
            Mode::Replace,
            PresentationPolicy::RemovePresentation,
        )
        .unwrap();
        assert_eq!(
            super::GEOMETRY_PARSES.with(std::cell::Cell::get) - before,
            1,
            "all callout-bearing parts must share one parse, not one parse per part"
        );
    }
    use super::*;
    use crate::hlr::{Plane, Unresolved, View, project, section};

    #[test]
    fn selected_bodies_reuse_the_files_geometry_parse() {
        let bytes = std::fs::read(fixture("ap242/assembly/assembly.step")).unwrap();
        let before = GEOMETRY_PARSES.with(std::cell::Cell::get);
        let file = StepFile::read(&bytes).unwrap();
        file.placed(&[0, 2]).unwrap();
        file.placed(&[1]).unwrap();
        file.part(1).unwrap();
        assert_eq!(GEOMETRY_PARSES.with(std::cell::Cell::get) - before, 1);
    }

    #[test]
    fn byte_snapshot_decoding_uses_the_same_extension_policy_as_file_reading() {
        use std::io::Write;
        use std::path::Path;
        let plain = b"ISO-10303-21;";
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(plain).unwrap();
        let compressed = encoder.finish().unwrap();
        assert_eq!(
            decode_step_file_bytes(Path::new("part.step.gz"), compressed.clone()).unwrap(),
            plain
        );
        assert_eq!(
            decode_step_file_bytes(Path::new("part.step"), plain.to_vec()).unwrap(),
            plain
        );
        assert_eq!(
            decode_step_file_bytes(Path::new("part.step"), compressed.clone()).unwrap(),
            compressed
        );
        assert!(decode_step_file_bytes(Path::new("part.step.gz"), plain.to_vec()).is_err());
    }

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name)
    }

    /// `rejected_cylinder.step` with its top plane written as an `OFFSET_SURFACE`, which the
    /// kernel does not model: the face is kept and recorded, its solid is not valid, and a
    /// drawing of the part is refused.
    #[test]
    fn an_unresolved_surface_is_recorded() {
        let part = read_step_file(&fixture("broken/unresolved_surface.step")).unwrap();
        assert_eq!(part.faces.len(), 3);
        assert_eq!(part.unresolved_faces(), [1]);
        assert!(part.unresolved_edges().is_empty());
        assert!(matches!(part.faces[1].surface, Surface::Other { .. }));
        assert!(!part.solid_is_valid(0));
        let refused = Unresolved {
            faces: vec![1],
            edges: Vec::new(),
        };
        let view = View::new([0.0, -1.0, 0.0], [0.0, 0.0, 1.0]).unwrap();
        assert_eq!(project(&part, &view).unwrap_err(), refused);
        let plane = Plane::new([0.0; 3], [0.0, 1.0, 0.0]).unwrap();
        assert_eq!(section(&part, &plane).unwrap_err(), refused);
    }

    /// The same cylinder with its seam line written as a hyperbola that misses one of its
    /// vertices: the open edge's chord stands in for it, recorded, and the solid is not valid.
    #[test]
    fn an_unresolved_open_edge_is_recorded() {
        let part = read_step_file(&fixture("broken/unresolved_open_curve.step")).unwrap();
        assert!(part.unresolved_faces().is_empty());
        assert_eq!(part.unresolved_edges(), [1]);
        assert!(!part.solid_is_valid(0));
        let view = View::new([0.0, -1.0, 0.0], [0.0, 0.0, 1.0]).unwrap();
        assert_eq!(project(&part, &view).unwrap_err().edges, [1]);
    }

    /// A cone frustum (radii 10 and 5, height 10) cut by the plane x = 4, which meets the cone
    /// in a hyperbola. OpenCascade 7.6.3 wrote the file (`BRepAlgoAPI_Cut`, AP214) and gives
    /// the volume 1484.900072670101 and area 793.444301743642. The hyperbola edges resolve
    /// exactly: each lies on both the plane and the cone, and the solid is valid.
    #[test]
    fn a_hyperbola_edge_resolves_exactly() {
        let part = read_step_file(&fixture("hyperbola_cut_cone.step")).unwrap();
        assert!(part.unresolved_faces().is_empty() && part.unresolved_edges().is_empty());
        assert!(part.solid_is_valid(0));
        let on_cut = |p: V3| {
            let off_plane = (p[0] - 4.0).abs();
            let off_cone = (p[0].hypot(p[1]) - (10.0 - 0.5 * p[2])).abs();
            off_plane.max(off_cone)
        };
        let hyperbolas: Vec<_> = part
            .edges
            .iter()
            .filter_map(|e| match &e.curve {
                Curve::Nurbs(c) => Some(c),
                _ => None,
            })
            .collect();
        assert!(!hyperbolas.is_empty());
        for c in hyperbolas {
            for i in 0..=64 {
                assert!(on_cut(c.value(f64::from(i) / 64.0)) < 1e-9);
            }
        }
        let (volume, area) = part.solid_mass(0).unwrap();
        assert!(
            (volume - 1484.900072670101).abs() < 1e-6 * 1484.9,
            "{volume}"
        );
        assert!((area - 793.444301743642).abs() < 1e-6 * 793.4, "{area}");
    }

    /// Open CASCADE 8.0, cone (10, 5, 10) cut by x + z/2 = 4 parallel to a
    /// generator. Independent 60-digit integration: with r=10-z/2, c=r-6,
    /// A(r)=r²(π/2+asin(c/r))+c*sqrt(r²-c²), V=2∫[5,10] A(r)dr.
    /// Area sums the two caps, the remaining cone and the planar section.
    /// OCCT's pcurve integrals give V=1178.246345909645 (1.37e-7 low) and
    /// A=723.863727273429 (6.55e-8 low), so use the analytic reference.
    #[test]
    fn a_parabola_boundary_resolves_and_moves_exactly() {
        let path = fixture("parabola_cut_cone.step");
        let part = read_step_file(&path).unwrap();
        assert!(part.unresolved_faces().is_empty() && part.unresolved_edges().is_empty());
        assert_eq!(part.solids.len(), 1);
        assert!(part.solid_is_valid(0));
        let conics: Vec<_> = part
            .edges
            .iter()
            .filter_map(|e| match &e.curve {
                Curve::Nurbs(c) => Some(c),
                _ => None,
            })
            .collect();
        assert!(!conics.is_empty());
        for c in conics {
            for i in 0..=64 {
                let p = c.value(f64::from(i) / 64.0);
                assert!((p[0] + 0.5 * p[2] - 4.0).abs() < 1e-9);
                assert!((p[0].hypot(p[1]) - (10.0 - 0.5 * p[2])).abs() < 1e-9);
            }
        }
        let check_mass = |part: &Part| {
            let (v, a) = part.solid_mass(0).unwrap();
            assert!((v - 1178.246346046747).abs() < 1e-8, "{v}");
            assert!((a - 723.8637273389423).abs() < 1e-8, "{a}");
        };
        check_mass(&part);
        let (s, c) = 0.37_f64.sin_cos();
        let at = [
            [c, -s, 0.0, 17.0],
            [s, c, 0.0, -23.0],
            [0.0, 0.0, 1.0, 11.0],
        ];
        let moved = part.transformed(&at).unwrap();
        let reread = read_step_file_placed(&path, &at).unwrap();
        assert!(moved.solid_is_valid(0));
        check_mass(&moved);
        check_mass(&reread);
        for (a, b) in moved.edges.iter().zip(&reread.edges) {
            assert_eq!(a.vertices, b.vertices);
            assert_eq!(a.samples, b.samples);
        }
    }

    fn trimmed_parabola_endpoints(bytes: &[u8]) -> (V3, V3) {
        let (model, report) = step_io::read(bytes).unwrap();
        assert!(report.dropped.is_empty());
        let scene = model.scene();
        let n = scene
            .all_geometric_sets()
            .flat_map(|s| s.elements().collect::<Vec<_>>())
            .find_map(|e| {
                if let step_io::scene::SetElement::Curve(c) = e {
                    c.to_nurbs()
                } else {
                    None
                }
            })
            .unwrap();
        let c = NurbsCurve::new(n.degree, n.control_points, n.weights, n.knots).unwrap();
        let (a, b) = c.domain();
        (c.value(a), c.value(b))
    }

    /// OCCT writes length-valued parameters ±4 for F=2, and points (2, ±4, 0).
    /// ISO's parameters at those points are ±1. Even with .PARAMETER. as master,
    /// the supplied points determine the span, matching step-io and OCCT's geometry.
    #[test]
    fn an_occt_trimmed_parabola_follows_its_points() {
        let bytes = std::fs::read(fixture("occt_trimmed_parabola.step")).unwrap();
        let (a, b) = trimmed_parabola_endpoints(&bytes);
        assert!(geom::dist(a, [2.0, -4.0, 0.0]) < 1e-12, "{a:?}");
        assert!(geom::dist(b, [2.0, 4.0, 0.0]) < 1e-12, "{b:?}");
        // Put the OCCT-written trimmed curve under an edge as well, exercising
        // haecceity's reader fallback and sampling, not only step-io's conversion.
        let mut text = String::from_utf8(bytes).unwrap();
        let end = text.rfind("ENDSEC;").unwrap();
        text.insert_str(end, "#30=VERTEX_POINT('',#22);\n#31=VERTEX_POINT('',#23);\n#32=EDGE_CURVE('',#30,#31,#16,.T.);\n");
        let (model, report) = step_io::read(text.as_bytes()).unwrap();
        assert!(report.dropped.is_empty());
        let scene = model.scene();
        let edge = scene.all_edges().next().unwrap();
        let reader = Reader {
            model: &model,
            to_mm: 1.0,
            to_rad: 1.0,
            placement: IDENTITY,
        };
        let (edge, resolved) = reader
            .edge(&edge, &mut |key| match key {
                m::EntityKey::VertexPoint(i) => i.0 as usize,
                _ => unreachable!(),
            })
            .unwrap();
        assert!(resolved);
        assert!(edge.samples.len() > 2);
        for p in edge.samples {
            assert!((p[0] - p[1] * p[1] / 8.0).abs() < 1e-10, "{p:?}");
            assert!(p[1].abs() <= 4.0 + 1e-12, "{p:?}");
        }
    }

    /// With no trim points, assume ISO 10303-42's dimensionless u:
    /// (F*u², 2F*u). There is no geometric evidence for an OCCT-specific rescaling.
    #[test]
    fn a_parameter_only_parabola_uses_the_standard_convention() {
        let text = std::fs::read_to_string(fixture("occt_trimmed_parabola.step")).unwrap();
        let text = text
            .replace("(#22,PARAMETER_VALUE(-4.))", "(PARAMETER_VALUE(-4.))")
            .replace("(#23,\n    PARAMETER_VALUE(4.))", "(PARAMETER_VALUE(4.))");
        assert!(!text.contains("(#22,") && !text.contains("(#23,"));
        let (a, b) = trimmed_parabola_endpoints(text.as_bytes());
        assert!(geom::dist(a, [32.0, -16.0, 0.0]) < 1e-12, "{a:?}");
        assert!(geom::dist(b, [32.0, 16.0, 0.0]) < 1e-12, "{b:?}");
    }

    /// The unbroken cylinder records nothing, stays valid and is drawn.
    #[test]
    fn a_resolved_part_records_nothing() {
        let part = read_step_file(&fixture("rejected_cylinder.step")).unwrap();
        assert!(part.unresolved_faces().is_empty() && part.unresolved_edges().is_empty());
        assert!(part.solid_is_valid(0));
        let view = View::new([0.0, -1.0, 0.0], [0.0, 0.0, 1.0]).unwrap();
        assert!(!project(&part, &view).unwrap().is_empty());
    }

    /// The id map is checked against the records, not only against entity types: two planar
    /// faces of one loop each, with the same sense, swapped in the map, pass every type check
    /// and are refused on where their planes are; likewise two edges on one kind of curve on where
    /// their vertices are.
    #[test]
    fn a_permuted_instance_map_is_refused() {
        let bytes = std::fs::read(fixture("prismatic.step")).unwrap();
        let (model, report) = step_io::read(bytes.as_slice()).unwrap();
        let graph = step_io::parser::parse(bytes.as_slice()).unwrap();
        let scene = model.scene();
        let faces: Vec<StepFace<'_>> = scene.all_faces().collect();
        let edges: Vec<StepEdge<'_>> = faces
            .iter()
            .flat_map(|f| {
                f.bounds()
                    .flat_map(|b| b.edges().collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut ids = FileIds::new(&graph, &model, &report);
        for f in &faces {
            ids.face(f).unwrap();
        }
        for e in &edges {
            ids.edge(e).unwrap();
        }

        let index = |key: m::EntityKey| match key {
            m::EntityKey::AdvancedFace(i) => i.0,
            m::EntityKey::EdgeCurve(i) => i.0,
            _ => unreachable!(),
        };
        let planar = |f: &&StepFace<'_>| {
            matches!(f.surface().kind(), SurfaceKind::Plane(_)) && f.bounds().count() == 1
        };
        // Planes at different points (the edge check alone would catch planes at one point).
        let location = |f: &StepFace<'_>| {
            let id = ids.id(f.key()).unwrap();
            ids.raw_location(ids.reference(id, 2).unwrap())
        };
        let (a, b) = faces
            .iter()
            .filter(planar)
            .flat_map(|a| faces.iter().filter(planar).map(move |b| (a, b)))
            .find(|(a, b)| a.orientation() == b.orientation() && location(a) != location(b))
            .expect("two planar one-loop faces of one sense");
        let mut permuted = FileIds::new(&graph, &model, &report);
        permuted
            .by_type
            .get_mut("ADVANCED_FACE")
            .unwrap()
            .swap(index(a.key()) as usize, index(b.key()) as usize);
        let (ia, ib) = (permuted.id(a.key()).unwrap(), permuted.id(b.key()).unwrap());
        assert_eq!(permuted.kind(ia), permuted.kind(ib));
        let surface = |id| permuted.kind(permuted.reference(id, 2).unwrap());
        assert_eq!(surface(ia), Some("PLANE"));
        assert_eq!(surface(ia), surface(ib));
        let error = permuted.face(a).unwrap_err().to_string();
        assert!(error.contains("is not where it was read"), "{error}");

        let curve = |e: &StepEdge<'_>| step_name(raw_edge_curve(&model, e).entity_key());
        let (c, d) = edges
            .iter()
            .flat_map(|c| edges.iter().map(move |d| (c, d)))
            .find(|(c, d)| {
                c.key() != d.key() && c.same_sense() == d.same_sense() && curve(c) == curve(d)
            })
            .expect("two edges on one kind of curve, of one sense");
        let mut permuted = FileIds::new(&graph, &model, &report);
        permuted
            .by_type
            .get_mut("EDGE_CURVE")
            .unwrap()
            .swap(index(c.key()) as usize, index(d.key()) as usize);
        let error = permuted.edge(c).unwrap_err().to_string();
        assert!(error.contains("is not where it was read"), "{error}");
    }

    #[test]
    fn broken_files_are_refused() {
        let error = |name: &str| read_step_file(&fixture(name)).unwrap_err().to_string();
        for name in [
            "golden_simple_through_hole_no_bounds.step",
            "kissing_cylinders_knife_no_bounds.step",
        ] {
            let err = error(&format!("broken/{name}"));
            assert!(
                err.contains("NonstandardReference: ADVANCED_FACE: 'bounds' references"),
                "{name}: {err}"
            );
        }
        assert!(error("broken/empty_data.step").contains("no solid or shell"));
        assert!(error("broken/truncated.step").contains("parse error"));
        assert!(error("broken/truncated_closed.step").contains("#105 is referenced"));
        assert!(error("broken/deleted_face.step").contains("#109 is referenced"));
        assert!(error("broken/unresolved_closed_curve.step").contains("closed edge's curve"));
        assert_eq!(
            error("broken/truck_pub_shaft_hole.step.gz"),
            "STEP parse error: parse error at line 639, column 1: duplicate entity #539"
        );
    }
}
