//! The boundary-representation model the recognisers read: faces, edges and solids, in the
//! traversal order OpenCascade gives a STEP import, so face indices mean the same thing on both
//! sides of the port.

use std::f64::consts::TAU;
use std::sync::OnceLock;

use super::geom::{self, Bounds, Curve, Surface, V3};
use super::sampling::{edge_interval, extremes_along, sampled_extremes};
use super::uv::{FaceDomain, UvLoop, touches_singular_point};

const AXES: [V3; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

#[derive(Clone, Debug)]
pub struct Edge {
    pub curve: Curve,
    pub start: V3,
    pub end: V3,
    /// Vertex identities (indices into the part's vertices): an edge is closed when its two
    /// ends are the same vertex, not merely coincident points.
    pub vertices: (usize, usize),
    /// Whether start → end follows the curve's parameter direction (`EDGE_CURVE.same_sense`).
    pub same_sense: bool,
    /// Points along the edge, start → end, dense enough to stand in for the exact curve.
    pub samples: Vec<V3>,
}

#[derive(Clone, Debug)]
pub struct Loop {
    /// Edge indices with their use direction in this loop (`true` = start → end).
    pub edges: Vec<(usize, bool)>,
    /// A loop made of a single vertex (`VERTEX_LOOP`): the apex of a cone, the pole of a sphere.
    pub vertex: Option<V3>,
}

#[derive(Clone, Debug)]
pub struct Face {
    pub surface: Surface,
    /// `TopAbs_REVERSED`: the face's material side is against the surface normal.
    pub reversed: bool,
    pub loops: Vec<Loop>,
    pub solid: Option<usize>,
    /// The file's parameter-space curves (pcurves) for this face's edges, by edge. OpenCascade
    /// sizes a face's parameter range from its pcurves, not from the 3D edges (which may sit
    /// off the surface within tolerance), so [`Part::uv_bounds`] uses them where given.
    pub pcurves: Vec<(usize, Pcurve)>,
}

/// How a solid turns where two faces meet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arc {
    /// The material forms a wedge at the edge.
    Convex,
    /// The material wraps round the edge.
    Concave,
    /// The outward normals agree there (a split face, or a tangent blend).
    Smooth,
    Unknown,
}

/// A pcurve, reduced to what sizing a parameter range needs.
#[derive(Clone, Debug)]
pub enum Pcurve {
    /// A B-spline spanning exactly its edge: its control polygon (OpenCascade boxes poles).
    Poles(Vec<(f64, f64)>),
    /// A straight line in parameter space: a point on it and its direction.
    Line { point: (f64, f64), dir: (f64, f64) },
}

/// The B-spline form of a pcurve held as [`Pcurve::Poles`] (degree, distinct knots with their
/// multiplicities, and weights when rational), as the file gave it: what writing it back needs.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PcurveSpline {
    pub(crate) degree: usize,
    pub(crate) knots: Vec<f64>,
    pub(crate) multiplicities: Vec<usize>,
    pub(crate) weights: Option<Vec<f64>>,
}

/// An `AXIS2_PLACEMENT_3D`'s axis and reference direction ratios as the file gives them,
/// before the reader normalises them (`None` where the file leaves one out).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RawFrame {
    pub(crate) axis: Option<V3>,
    pub(crate) reference: Option<V3>,
}

/// A swept surface as the file states it (file units, before the reader's conversion to
/// NURBS): writing it again in a file of the same length unit converts back to the same
/// NURBS, so the face keeps its kind.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SweptSource {
    Revolution {
        profile: RawProfile,
        location: V3,
        axis: Option<V3>,
        /// The file's length unit in millimetres.
        to_mm: f64,
        /// The file's plane angle unit in radians.
        to_rad: f64,
    },
    Extrusion {
        profile: RawProfile,
        direction: V3,
        magnitude: f64,
        to_mm: f64,
    },
}

impl SweptSource {
    pub(crate) fn to_mm(&self) -> f64 {
        match self {
            SweptSource::Revolution { to_mm, .. } | SweptSource::Extrusion { to_mm, .. } => *to_mm,
        }
    }
}

/// A pcurve's 2D curve as the file states it (parameter values unscaled).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RawCurve2d {
    Line {
        point: [f64; 2],
        direction: [f64; 2],
        magnitude: f64,
    },
    BSpline {
        degree: i64,
        points: Vec<[f64; 2]>,
        multiplicities: Vec<i64>,
        knots: Vec<f64>,
        weights: Option<Vec<f64>>,
    },
}

/// A swept surface's profile curve as the file states it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RawProfile {
    Line {
        point: V3,
        direction: V3,
        magnitude: f64,
    },
    Circle {
        origin: V3,
        frame: RawFrame,
        radius: f64,
    },
    Ellipse {
        origin: V3,
        frame: RawFrame,
        semi_axis_1: f64,
        semi_axis_2: f64,
    },
    BSpline {
        degree: i64,
        points: Vec<V3>,
        multiplicities: Vec<i64>,
        knots: Vec<f64>,
        weights: Option<Vec<f64>>,
    },
}

#[derive(Clone, Debug)]
pub struct Solid {
    pub faces: Vec<usize>,
}

/// One assembly level's placement as the file states it, when its `from` frame is the
/// standard one (origin, z and x axes): the `to` frame's origin (in millimetres) and its axis
/// and reference direction ratios with defaults filled. Writing it again as the placement of a
/// component reproduces the reader's transform bit for bit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct InstanceTransform {
    pub(crate) origin: V3,
    pub(crate) axis: V3,
    pub(crate) reference: V3,
}

/// A product as the file names it: `PRODUCT.id` and `PRODUCT.name`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProductLabel {
    pub(crate) id: String,
    pub(crate) name: String,
}

/// Retained native instance placement, composed in the reader's original order.
#[derive(Clone, Debug)]
pub(super) struct GeometryPlacement {
    outer: super::step::Placement,
    path: std::sync::Arc<[super::step::Placement]>,
    /// Per level of `path`, its placement as the file states it (`None` when not standard).
    raw: std::sync::Arc<[Option<InstanceTransform>]>,
    /// The products from the root definition down to the instance's own (one more than `path`
    /// has levels; empty where the reader placed the shape through no product).
    products: std::sync::Arc<[ProductLabel]>,
}

impl GeometryPlacement {
    pub(super) fn new(
        outer: super::step::Placement,
        path: std::sync::Arc<[super::step::Placement]>,
        raw: std::sync::Arc<[Option<InstanceTransform>]>,
        products: std::sync::Arc<[ProductLabel]>,
    ) -> Self {
        Self {
            outer,
            path,
            raw,
            products,
        }
    }
    fn placement(&self) -> super::step::Placement {
        self.path
            .iter()
            .fold(self.outer, |at, child| super::step::compose(&at, child))
    }
}

// Identity reads already own their native primitives: retain only a shared context until
// the first transform. Each instance has one context, not one matrix per primitive.
type Native<T> = (Option<std::sync::Arc<T>>, std::sync::Arc<GeometryPlacement>);

/// Per-face values derived from the geometry, computed on first use.
#[derive(Debug, Default)]
pub(super) struct FaceCache {
    pub(super) uv_loops: OnceLock<Option<Vec<UvLoop>>>,
    pub(super) domain: OnceLock<Option<FaceDomain>>,
    pub(super) bounds: OnceLock<Bounds>,
    pub(super) mass: OnceLock<Option<[f64; 5]>>,
    pub(super) moments: OnceLock<Option<super::mass::FaceMoments>>,
    pub(super) edge_deviation: OnceLock<Vec<(usize, f64)>>,
    pub(super) uv_bounds: OnceLock<Option<(f64, f64, f64, f64)>>,
    pub(super) recovered: OnceLock<Option<Surface>>,
}

impl Edge {
    /// Whether the edge starts and ends at the same vertex (a full circle, a closed spline).
    pub fn is_closed(&self) -> bool {
        self.vertices.0 == self.vertices.1
    }

    /// The point halfway along the edge by arc length (build123d `Edge.center()`): exact for
    /// lines and circular arcs, measured along the samples otherwise.
    pub fn midpoint(&self) -> V3 {
        match &self.curve {
            Curve::Line { .. } => geom::scale(geom::add(self.start, self.end), 0.5),
            Curve::Circle { .. } => {
                let (a, b) = edge_interval(
                    &self.curve,
                    self.start,
                    self.end,
                    self.same_sense,
                    self.is_closed(),
                );
                self.curve.value(0.5 * (a + b))
            }
            _ => {
                let lengths: Vec<f64> = self
                    .samples
                    .windows(2)
                    .map(|w| geom::dist(w[0], w[1]))
                    .collect();
                let half = 0.5 * lengths.iter().sum::<f64>();
                let mut run = 0.0;
                for (w, len) in self.samples.windows(2).zip(&lengths) {
                    if run + len >= half && *len > 0.0 {
                        let t = (half - run) / len;
                        return geom::add(w[0], geom::scale(geom::sub(w[1], w[0]), t));
                    }
                    run += len;
                }
                self.end
            }
        }
    }

    /// The vertex points (one for a closed edge), as build123d's `edge.vertices()` gives them.
    pub fn vertex_points(&self) -> Vec<V3> {
        if self.is_closed() {
            vec![self.start]
        } else {
            vec![self.start, self.end]
        }
    }
}

/// Whether the face's domain holds a neighbourhood of (u, v): the points a step of *h* away
/// in u and in v, each way, are all inside it (the point itself is never read). A point on the
/// domain's boundary, a seam included, is inside or not by round-off, so the answer there would
/// follow the part's placement; this one is "no" there, and placement-independent elsewhere.
pub(super) fn holds(domain: &FaceDomain, u: f64, v: f64, (hu, hv): (f64, f64)) -> bool {
    [(-hu, 0.0), (hu, 0.0), (0.0, -hv), (0.0, hv)]
        .iter()
        .all(|&(du, dv)| domain.contains(u + du, v + dv))
}

/// The file instance a face or edge of a [`Part`] was read from: the `#N` of its
/// `ADVANCED_FACE` (or `FACE_SURFACE`) or `EDGE_CURVE`, and the placed instance it belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Source {
    /// The instance id (`#N`) in the file.
    pub entity: u64,
    /// Which placed instance of a shape the face or edge is part of: the reader's placed solids,
    /// numbered in whole-file reading order, then each placement of a surface model.
    /// A solid's instance number equals its index in the whole-file read's [`Part::solids`].
    /// Reading selected bodies or one part definition keeps that original number, which
    /// need not equal the solid's index in the resulting part.
    pub instance: usize,
}

pub type FaceSource = Source;
pub type EdgeSource = Source;

/// A part's boundary representation: faces, edges and solids in OpenCascade's traversal order.
///
/// Immutable once queried: [`Part::repair_inverted_shells`] (which STEP import applies) is the
/// only change, and it takes the part by value before any query. `faces`, `edges` and `solids`
/// are public for reading only:
/// the per-face caches (parameter-space loops and bounds, boxes, mass, recovered surfaces),
/// the edge-to-face map and the solid validity are computed lazily from them on first use and
/// never recomputed, so a change to the topology after any query leaves those answers stale.
/// Build a new `Part` instead.
#[derive(Debug)]
pub struct Part {
    pub faces: Vec<Face>,
    pub edges: Vec<Edge>,
    pub solids: Vec<Solid>,
    pub(super) cache: Vec<FaceCache>,
    /// Default ray boxes, indexed by face of this immutable part. Transforms start fresh;
    /// orientation repairs clear these and the edge queries with the affected face caches.
    pub(super) ray_face_boxes: OnceLock<Vec<Bounds>>,
    /// Edge boxes, polyline run boxes and lazy cracks, indexed by edge of this part.
    pub(super) ray_edges: OnceLock<super::rays::RayEdges>,
    edge_faces: OnceLock<Vec<Vec<usize>>>,
    valid_solids: OnceLock<Vec<bool>>,
    bad_faces: OnceLock<Vec<Vec<usize>>>,
    unresolved_faces: Vec<usize>,
    unresolved_edges: Vec<usize>,
    face_sources: Vec<Source>,
    edge_sources: Vec<Source>,
    input_repairs: Vec<String>,
    // STEP LINE direction ratios before normalisation, for reader-equivalent rotations.
    line_directions: std::collections::BTreeMap<usize, V3>,
    conics: std::collections::BTreeMap<usize, geom::Conic>,
    // Per face, per entry of its `pcurves`, the B-spline form of a `Pcurve::Poles`.
    pcurve_splines: Vec<Vec<Option<PcurveSpline>>>,
    // The file's frame ratios of analytic surfaces and of circle, ellipse and conic edges.
    face_frames: std::collections::BTreeMap<usize, RawFrame>,
    edge_frames: std::collections::BTreeMap<usize, RawFrame>,
    swept: std::collections::BTreeMap<usize, SweptSource>,
    // Every face's pcurves as the file states them, by edge (unscaled; the reader holds a
    // reduced form of an analytic face's, and none of a freeform face's).
    file_pcurves: std::collections::BTreeMap<usize, Vec<(usize, RawCurve2d)>>,
    // The file's length and plane angle units in millimetres and radians.
    file_units: Option<(f64, f64)>,
    // The file's length uncertainty, in millimetres.
    file_uncertainty: Option<f64>,
    native_faces: Vec<Native<Face>>,
    native_edges: Vec<Native<Edge>>,
}

impl Part {
    /// Build caller-supplied topology with fresh query caches. Each face must have
    /// at least one loop and all indices must refer to members of this part.
    /// STEP import enforces the nonempty-bounds invariant before constructing faces.
    pub fn new(faces: Vec<Face>, edges: Vec<Edge>, solids: Vec<Solid>) -> Self {
        let cache = faces.iter().map(|_| FaceCache::default()).collect();
        Part {
            faces,
            edges,
            solids,
            cache,
            ray_face_boxes: OnceLock::new(),
            ray_edges: OnceLock::new(),
            edge_faces: OnceLock::new(),
            valid_solids: OnceLock::new(),
            bad_faces: OnceLock::new(),
            unresolved_faces: Vec::new(),
            unresolved_edges: Vec::new(),
            face_sources: Vec::new(),
            edge_sources: Vec::new(),
            input_repairs: Vec::new(),
            line_directions: Default::default(),
            conics: Default::default(),
            pcurve_splines: Vec::new(),
            face_frames: Default::default(),
            edge_frames: Default::default(),
            swept: Default::default(),
            file_pcurves: Default::default(),
            file_units: None,
            file_uncertainty: None,
            native_faces: Vec::new(),
            native_edges: Vec::new(),
        }
    }

    /// Copy this loaded part through a finite proper rigid motion, without reading STEP.
    /// Face, edge, solid, vertex and source indices and parameter-space curves are preserved.
    /// Derived boxes, quadratures, inversion grids and classification caches are invalidated
    /// and rebuilt lazily from the transformed geometry; no cached caller-space data survives.
    /// Imported native primitives and their placement chains are retained so composition
    /// precedes coordinate rounding, including on a part transformed more than once.
    pub fn transformed(
        &self,
        placement: &super::step::Placement,
    ) -> Result<Self, super::step::StepError> {
        super::step::validate_placement(placement)?;
        let direction = |at: &super::step::Placement, p: V3| {
            at.map(|r| r[0] * p[0] + r[1] * p[1] + r[2] * p[2])
        };
        let point = |at: &super::step::Placement, p: V3| {
            let d = direction(at, p);
            [0, 1, 2].map(|i| d[i] + at[i][3])
        };
        let frame = |at: &super::step::Placement, f: &mut geom::Frame| {
            let direct = f.direct();
            f.origin = point(at, f.origin);
            f.x = direction(at, f.x);
            f.z = direction(at, f.z);
            f.y = geom::scale(geom::cross(f.z, f.x), if direct { 1.0 } else { -1.0 });
        };
        let identity = std::sync::Arc::new(GeometryPlacement::new(
            super::step::IDENTITY,
            std::sync::Arc::from([]),
            std::sync::Arc::from([]),
            std::sync::Arc::from([]),
        ));
        // Share both the imported context and its composed replacement across all primitives
        // of an instance. Native geometry is copied only when a part is actually transformed.
        let mut contexts = std::collections::HashMap::new();
        let mut moved_context = |at: &std::sync::Arc<GeometryPlacement>| {
            contexts
                .entry(std::sync::Arc::as_ptr(at) as usize)
                .or_insert_with(|| {
                    let mut moved = (**at).clone();
                    moved.outer = super::step::compose(placement, &moved.outer);
                    std::sync::Arc::new(moved)
                })
                .clone()
        };
        let native_faces: Vec<_> = self
            .faces
            .iter()
            .enumerate()
            .map(|(i, face)| {
                let (native, context) = self
                    .native_faces
                    .get(i)
                    .map_or((None, &identity), |(native, context)| {
                        (native.as_ref(), context)
                    });
                (
                    Some(
                        native
                            .cloned()
                            .unwrap_or_else(|| std::sync::Arc::new(face.clone())),
                    ),
                    moved_context(context),
                )
            })
            .collect();
        let native_edges: Vec<_> = self
            .edges
            .iter()
            .enumerate()
            .map(|(i, edge)| {
                let (native, context) = self
                    .native_edges
                    .get(i)
                    .map_or((None, &identity), |(native, context)| {
                        (native.as_ref(), context)
                    });
                (
                    Some(native.cloned().unwrap_or_else(|| {
                        let mut native = edge.clone();
                        native.samples = Vec::new();
                        std::sync::Arc::new(native)
                    })),
                    moved_context(context),
                )
            })
            .collect();
        let mut faces: Vec<_> = self
            .faces
            .iter()
            .enumerate()
            .map(|(i, face)| {
                let source = self
                    .native_faces
                    .get(i)
                    .and_then(|(native, _)| native.as_deref())
                    .unwrap_or(face);
                Face {
                    surface: source.surface.clone(),
                    reversed: face.reversed,
                    loops: face.loops.clone(),
                    solid: face.solid,
                    pcurves: face.pcurves.clone(),
                }
            })
            .collect();
        for (index, face) in faces.iter_mut().enumerate() {
            let (native, context) = &native_faces[index];
            let at = context.placement();
            // Every face has a native form here (built above).
            if let Some(native) = native {
                for (bound, source) in face.loops.iter_mut().zip(&native.loops) {
                    bound.vertex = source.vertex;
                }
            }
            let point = |p| point(&at, p);
            match &mut face.surface {
                Surface::Plane { frame: f }
                | Surface::Cylinder { frame: f, .. }
                | Surface::Cone { frame: f, .. }
                | Surface::Sphere { frame: f, .. }
                | Surface::Torus { frame: f, .. } => frame(&at, f),
                Surface::Freeform { surface, .. } => surface.transform_points(point),
                Surface::Other { .. } => {}
            }
            for bound in &mut face.loops {
                bound.vertex = bound.vertex.map(point);
            }
        }
        let mut edges: Vec<_> = self
            .edges
            .iter()
            .enumerate()
            .map(|(i, edge)| {
                let source = self
                    .native_edges
                    .get(i)
                    .and_then(|(native, _)| native.as_deref())
                    .unwrap_or(edge);
                Edge {
                    curve: source.curve.clone(),
                    start: source.start,
                    end: source.end,
                    vertices: edge.vertices,
                    same_sense: edge.same_sense,
                    samples: Vec::new(),
                }
            })
            .collect();
        for (index, edge) in edges.iter_mut().enumerate() {
            let at = native_edges[index].1.placement();
            let point = |p| point(&at, p);
            edge.start = point(edge.start);
            edge.end = point(edge.end);
            match &mut edge.curve {
                Curve::Line { origin, dir } => {
                    *origin = point(*origin);
                    *dir = if let Some(ratios) = self.line_directions.get(&index) {
                        let moved =
                            at.map(|r| r[0] * ratios[0] + r[1] * ratios[1] + r[2] * ratios[2]);
                        geom::unit(moved).unwrap_or(moved)
                    } else {
                        direction(&at, *dir)
                    };
                }
                Curve::Circle { frame: f, .. } | Curve::Ellipse { frame: f, .. } => frame(&at, f),
                Curve::Nurbs(curve) => curve.transform_points(point),
            }
            if let Some(native) = self.conics.get(&index) {
                let mut conic = native.clone();
                frame(&at, conic.frame_mut());
                if let Some(curve) = conic.arc(edge.start, edge.end) {
                    edge.curve = curve;
                }
            }
            edge.samples = super::sampling::sample_edge(
                &edge.curve,
                edge.start,
                edge.end,
                edge.same_sense,
                edge.is_closed(),
            );
        }
        let mut part = Self::new(faces, edges, self.solids.clone());
        part.unresolved_faces = self.unresolved_faces.clone();
        part.unresolved_edges = self.unresolved_edges.clone();
        part.face_sources = self.face_sources.clone();
        part.edge_sources = self.edge_sources.clone();
        part.input_repairs = self.input_repairs.clone();
        part.line_directions = self.line_directions.clone();
        part.conics = self.conics.clone();
        part.pcurve_splines = self.pcurve_splines.clone();
        part.face_frames = self.face_frames.clone();
        part.edge_frames = self.edge_frames.clone();
        part.swept = self.swept.clone();
        part.file_pcurves = self.file_pcurves.clone();
        part.file_units = self.file_units;
        part.file_uncertainty = self.file_uncertainty;
        part.native_faces = native_faces;
        part.native_edges = native_edges;
        Ok(part)
    }

    // Keep native geometry for placed instances so later rigid motions compose their
    // placements before touching coordinates, just as STEP import does (one rounding).
    pub(super) fn with_native_geometry(
        mut self,
        faces: Vec<Native<Face>>,
        edges: Vec<Native<Edge>>,
    ) -> Self {
        self.native_faces = faces;
        self.native_edges = edges;
        self
    }

    pub(super) fn with_conics(
        mut self,
        conics: std::collections::BTreeMap<usize, geom::Conic>,
    ) -> Self {
        self.conics = conics;
        self
    }

    pub(super) fn with_pcurve_splines(mut self, splines: Vec<Vec<Option<PcurveSpline>>>) -> Self {
        self.pcurve_splines = splines;
        self
    }

    pub(super) fn with_raw_frames(
        mut self,
        faces: std::collections::BTreeMap<usize, RawFrame>,
        edges: std::collections::BTreeMap<usize, RawFrame>,
    ) -> Self {
        self.face_frames = faces;
        self.edge_frames = edges;
        self
    }

    pub(super) fn with_swept(
        mut self,
        swept: std::collections::BTreeMap<usize, SweptSource>,
    ) -> Self {
        self.swept = swept;
        self
    }

    pub(super) fn with_file_pcurves(
        mut self,
        pcurves: std::collections::BTreeMap<usize, Vec<(usize, RawCurve2d)>>,
        units: (f64, f64),
        uncertainty: Option<f64>,
    ) -> Self {
        self.file_pcurves = pcurves;
        self.file_units = Some(units);
        self.file_uncertainty = uncertainty;
        self
    }

    /// The length uncertainty (modelling precision) of the file the part was read from, in
    /// millimetres.
    pub fn file_uncertainty(&self) -> Option<f64> {
        self.file_uncertainty
    }

    /// The pcurves the file gives on face *face*'s surface, by edge, as it states them.
    pub(crate) fn file_pcurves(&self, face: usize) -> &[(usize, RawCurve2d)] {
        self.file_pcurves.get(&face).map_or(&[], Vec::as_slice)
    }

    /// The length and plane angle units (millimetres, radians) of the file the part was read
    /// from.
    pub(crate) fn file_units(&self) -> Option<(f64, f64)> {
        self.file_units
    }

    /// Face *face*'s swept surface as the file states it, before [`Self::face_placement`].
    pub(crate) fn face_swept(&self, face: usize) -> Option<&SweptSource> {
        self.swept.get(&face)
    }

    /// The file's frame ratios of face *face*'s analytic surface, before
    /// [`Self::face_placement`].
    pub(crate) fn face_frame(&self, face: usize) -> Option<&RawFrame> {
        self.face_frames.get(&face)
    }

    /// The file's frame ratios of edge *edge*'s circle, ellipse or conic, before
    /// [`Self::edge_placement`].
    pub(crate) fn edge_frame(&self, edge: usize) -> Option<&RawFrame> {
        self.edge_frames.get(&edge)
    }

    /// Face *face*'s native geometry (in the frame of the file definition it was read from)
    /// with that definition's placement levels as the file states them, when the part holds
    /// them all: `None` for a part moved after reading, or placed by a level the file states
    /// another way. An empty list for a face read where it lies.
    pub(crate) fn face_instance(
        &self,
        face: usize,
    ) -> Option<(&Face, &[Option<InstanceTransform>], usize)> {
        let (native, context) = self.native_faces.get(face)?;
        if context.outer != super::step::IDENTITY {
            return None;
        }
        let key = std::sync::Arc::as_ptr(context) as usize;
        Some((
            native.as_deref().unwrap_or(&self.faces[face]),
            &context.raw,
            key,
        ))
    }

    /// The products face *face*'s instance was read through, from the root definition down to
    /// its own; empty when the part holds none.
    pub(crate) fn face_products(&self, face: usize) -> &[ProductLabel] {
        self.native_faces
            .get(face)
            .map_or(&[], |(_, context)| &context.products)
    }

    /// Edge *edge*'s native geometry (see [`Self::face_instance`]).
    pub(crate) fn native_edge(&self, edge: usize) -> &Edge {
        self.native_edges
            .get(edge)
            .and_then(|(native, _)| native.as_deref())
            .unwrap_or(&self.edges[edge])
    }

    /// The placement from the frame face *face* was read in to the part's coordinates.
    pub(crate) fn face_placement(&self, face: usize) -> super::step::Placement {
        self.native_faces
            .get(face)
            .map_or(super::step::IDENTITY, |(_, context)| context.placement())
    }

    /// The B-spline form of entry *slot* of face *face*'s `pcurves`, when the file gave one.
    pub(crate) fn pcurve_spline(&self, face: usize, slot: usize) -> Option<&PcurveSpline> {
        self.pcurve_splines.get(face)?.get(slot)?.as_ref()
    }

    /// Edge *edge*'s exact parabola or hyperbola where it lies on one (its curve is then the
    /// arc in NURBS form), in the frame of the file it was read from, before
    /// [`Self::edge_placement`].
    pub(crate) fn edge_conic(&self, edge: usize) -> Option<&geom::Conic> {
        self.conics.get(&edge)
    }

    /// A straight edge's `LINE` direction ratios as the file gave them, before
    /// [`Self::edge_placement`] and normalisation.
    pub(crate) fn edge_line_ratios(&self, edge: usize) -> Option<V3> {
        self.line_directions.get(&edge).copied()
    }

    /// The placement from the frame edge *edge* was read in to the part's coordinates.
    pub(crate) fn edge_placement(&self, edge: usize) -> super::step::Placement {
        self.native_edges
            .get(edge)
            .map_or(super::step::IDENTITY, |(_, context)| context.placement())
    }

    pub(super) fn with_line_directions(
        mut self,
        directions: std::collections::BTreeMap<usize, V3>,
    ) -> Self {
        self.line_directions = directions;
        self
    }

    /// Records, for every face and every edge in index order, the instance it was read from.
    pub(super) fn with_sources(mut self, faces: Vec<Source>, edges: Vec<Source>) -> Self {
        assert_eq!(faces.len(), self.faces.len());
        assert_eq!(edges.len(), self.edges.len());
        self.face_sources = faces;
        self.edge_sources = edges;
        self
    }

    /// The file instance face *face* was read from; `None` for a part not read from a file.
    pub fn face_source(&self, face: usize) -> Option<FaceSource> {
        self.face_sources.get(face).copied()
    }

    /// The file instance edge *edge* was read from; `None` for a part not read from a file.
    pub fn edge_source(&self, edge: usize) -> Option<EdgeSource> {
        self.edge_sources.get(edge).copied()
    }

    /// Records the faces whose surface (`Surface::Other`, unevaluable) and the edges whose curve
    /// (its chord stands in) the reader could not resolve.
    pub(super) fn with_unresolved(mut self, faces: Vec<usize>, edges: Vec<usize>) -> Self {
        self.unresolved_faces = faces;
        self.unresolved_edges = edges;
        self
    }

    /// The faces whose surface did not resolve: they carry `Surface::Other`, which evaluates
    /// to NaN and meets no ray.
    pub fn unresolved_faces(&self) -> &[usize] {
        &self.unresolved_faces
    }

    /// The edges whose curve did not resolve: each carries its chord in its place.
    pub fn unresolved_edges(&self) -> &[usize] {
        &self.unresolved_edges
    }

    /// Whether *other* is exactly this B-rep: the same faces, edges and solids in the same
    /// order, with bit-equal defining geometry (surfaces, curves, vertices, pcurves) and trim
    /// topology. Entity IDs, names and cached samples are not compared. Stricter than geometric
    /// equality, it also covers surfaces whose area the kernel cannot integrate. A part with an
    /// unresolved face or edge, or no faces, is never the same as anything.
    pub fn same_brep(&self, other: &Part) -> bool {
        let (a, b) = (self, other);
        a.faces.len() == b.faces.len()
            && a.edges.len() == b.edges.len()
            && a.solids.len() == b.solids.len()
            && !a.faces.is_empty()
            && a.unresolved_faces.is_empty()
            && b.unresolved_faces.is_empty()
            && a.unresolved_edges.is_empty()
            && b.unresolved_edges.is_empty()
            && a.solids
                .iter()
                .zip(&b.solids)
                .all(|(a, b)| a.faces == b.faces)
            && a.edges.iter().zip(&b.edges).all(|(a, b)| {
                a.curve == b.curve
                    && a.start == b.start
                    && a.end == b.end
                    && a.vertices == b.vertices
                    && a.same_sense == b.same_sense
            })
            && a.faces.iter().zip(&b.faces).all(|(a, b)| {
                a.reversed == b.reversed
                    && a.solid == b.solid
                    && same_surface(&a.surface, &b.surface)
                    && a.loops.len() == b.loops.len()
                    && a.loops
                        .iter()
                        .zip(&b.loops)
                        .all(|(a, b)| a.edges == b.edges && a.vertex == b.vertex)
                    && a.pcurves.len() == b.pcurves.len()
                    && a.pcurves
                        .iter()
                        .zip(&b.pcurves)
                        .all(|((ai, a), (bi, b))| ai == bi && same_pcurve(a, b))
            })
    }

    /// Repairs applied while constructing this part, naming the affected solid (zero-based).
    /// Empty when the input was retained without repair.
    pub fn input_repairs(&self) -> &[String] {
        &self.input_repairs
    }

    pub(super) fn record_inverted_shell_repair(&mut self, solid: usize, reverse_wires: bool) {
        self.input_repairs
            .push(format!("solid {solid}: inverted closed shell repaired"));
        // Domains can use the face sense to choose a periodic band; no derived answer from
        // the input orientation may survive the repair.
        for &f in &self.solids[solid].faces {
            self.faces[f].reversed = !self.faces[f].reversed;
            if reverse_wires {
                for lp in &mut self.faces[f].loops {
                    lp.edges.reverse();
                    for (_, forward) in &mut lp.edges {
                        *forward = !*forward;
                    }
                }
            }
            self.cache[f] = FaceCache::default();
        }
        self.bad_faces = OnceLock::new();
        self.ray_face_boxes = OnceLock::new();
        self.ray_edges = OnceLock::new();
    }

    pub(super) fn record_mixed_normal_repair(&mut self, solid: usize, flips: &[usize]) {
        self.input_repairs.push(format!(
            "solid {solid}: mixed face senses repaired ({} faces)",
            flips.len()
        ));
        for &f in &self.solids[solid].faces {
            if flips.contains(&f) {
                self.faces[f].reversed = !self.faces[f].reversed;
            }
            self.cache[f] = FaceCache::default();
        }
        self.bad_faces = OnceLock::new();
        self.ray_face_boxes = OnceLock::new();
        self.ray_edges = OnceLock::new();
    }

    /// The face's axis-aligned box: its boundary plus any interior axis extremes.
    pub fn face_bounds(&self, face: usize) -> Bounds {
        *self.cache[face]
            .bounds
            .get_or_init(|| self.compute_face_bounds(face))
    }

    /// Boundary and surface extrema projected onto a unit direction, without copying
    /// or rotating the part. Uses the same conic/NURBS extrema as `face_bounds`.
    pub fn face_projection_bounds(&self, face: usize, direction: V3) -> Option<(f64, f64)> {
        let direction = geom::unit(direction)?;
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for point in self.face_extreme_points(face, &[direction]) {
            let t = geom::dot(point, direction);
            if !t.is_finite() {
                return None;
            }
            lo = lo.min(t);
            hi = hi.max(t);
        }
        (lo <= hi).then_some((lo, hi))
    }

    fn compute_face_bounds(&self, face: usize) -> Bounds {
        let mut b = Bounds::empty();
        for p in self.face_extreme_points(face, &AXES) {
            b.add(p);
        }
        b
    }

    /// The face's boundary samples plus every point where it may be extreme along one of the
    /// unit directions *dirs* (both senses): circle arcs' exact extremes and the interior
    /// extremes of doubly-curved faces (exact for spheres and tori, sampled for freeform
    /// surfaces) that lie on the face. Their extent along each direction is the face's.
    fn face_extreme_points(&self, face: usize, dirs: &[V3]) -> Vec<V3> {
        let f = &self.faces[face];
        let mut out = Vec::new();
        for lp in &f.loops {
            if let Some(p) = lp.vertex {
                out.push(p);
            }
            for &(e, _) in &lp.edges {
                let edge = &self.edges[e];
                out.extend(&edge.samples);
                // Conics' extremes in closed form: their samples are taken in the part's
                // placement, so would put the box's sampling error there too. A B-spline's
                // samples undercut its extremes between them (cgb207 edge 452, 2.7e-4 short).
                if !matches!(edge.curve, Curve::Line { .. }) {
                    let interval = edge_interval(
                        &edge.curve,
                        edge.start,
                        edge.end,
                        edge.same_sense,
                        edge.is_closed(),
                    );
                    out.extend(match edge.curve {
                        Curve::Nurbs(_) => {
                            sampled_extremes(&edge.curve, interval, &edge.samples, dirs)
                        }
                        _ => extremes_along(&edge.curve, interval, dirs),
                    });
                }
            }
        }
        // The face lies on its surface: an edge straying from an analytic surface (within its
        // tolerance) does not carry the box past it (cgb202 torus face 1870: edge 1077 dips
        // 0.0122 below the torus's lowest y). A point within the coordinate floor of the
        // surface is on it, and keeps its exact coordinates.
        for p in &mut out {
            if let Some(q) = f.surface.foot(*p)
                && geom::dist(*p, q) > geom::COORD_FLOOR
            {
                *p = q;
            }
        }
        // Doubly-curved faces can bulge past their boundary: add their interior extremes that
        // lie on the face.
        let Some(domain) = self.domain(face) else {
            return out;
        };
        // A candidate counts where the face holds a neighbourhood of it (`holds`): one on the
        // face's boundary adds nothing the edges do not.
        match &f.surface {
            Surface::Sphere { .. } | Surface::Torus { .. } => {
                for (u, v) in f.surface.extreme_parameters_along(dirs) {
                    if holds(domain, u, v, (1e-6 * TAU, 1e-6 * TAU))
                        || touches_singular_point(&f.surface, domain, v)
                    {
                        out.push(f.surface.value(u, v));
                    }
                }
            }
            // A cone face can run to its apex without a vertex there to bound it. The apex is a
            // whole parameter line (u is undefined there), so whether the face reaches it is read
            // round that line, on both sides, between the u steps that a seam at u = 0 would
            // put on the face's boundary.
            Surface::Cone {
                radius, semi_angle, ..
            } => {
                let apex = -radius / semi_angle.sin();
                let reaches = [apex - 1e-4, apex + 1e-4]
                    .into_iter()
                    .any(|v| (0..64).any(|k| domain.contains(TAU * (k as f64 + 0.5) / 64.0, v)));
                if reaches {
                    out.push(f.surface.value(0.0, apex));
                }
            }
            // Sampled: a grid over the surface's parameter domain.
            Surface::Freeform { surface, .. } => {
                let (u0, u1, v0, v1) = surface.domain();
                let n = 12;
                let h = (1e-6 * (u1 - u0), 1e-6 * (v1 - v0));
                let mut held = Vec::new();
                for i in 0..=n {
                    for j in 0..=n {
                        let u = u0 + (u1 - u0) * i as f64 / n as f64;
                        let v = v0 + (v1 - v0) * j as f64 / n as f64;
                        if holds(domain, u, v, h) {
                            held.push((u, v));
                            out.push(f.surface.value(u, v));
                        }
                    }
                }
                // The grid undercuts an interior bulge (cgb207 face 22, 3.1e-5 short): refine
                // the best grid point along each direction within a grid step.
                let step = ((u1 - u0) / n as f64, (v1 - v0) / n as f64);
                for d in dirs.iter().flat_map(|&d| [d, geom::scale(d, -1.0)]) {
                    let along = |&(u, v): &(f64, f64)| geom::dot(f.surface.value(u, v), d);
                    let Some(best) = held.iter().max_by(|a, b| along(a).total_cmp(&along(b)))
                    else {
                        continue;
                    };
                    let (u, v) = surface.extreme_near(*best, step, d);
                    if holds(domain, u, v, h) {
                        out.push(f.surface.value(u, v));
                    }
                }
            }
            _ => {}
        }
        out
    }

    /// The least and greatest coordinate of *faces* along the unit direction *d*: the extent
    /// along *d* of the box OpenCascade gives the shape turned so that *d* is a coordinate axis.
    pub fn extent_along(&self, faces: &[usize], d: V3) -> (f64, f64) {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for &face in faces {
            for p in self.face_extreme_points(face, &[d]) {
                let s = geom::dot(p, d);
                lo = lo.min(s);
                hi = hi.max(s);
            }
        }
        (lo, hi)
    }

    pub fn bounds(&self) -> Bounds {
        let mut b = Bounds::empty();
        for i in 0..self.faces.len() {
            b.merge(&self.face_bounds(i));
        }
        b
    }

    /// Faces of each edge, in face order (`edge_face_map`). A seam edge lists its face once.
    pub fn edge_faces(&self) -> &[Vec<usize>] {
        self.edge_faces.get_or_init(|| {
            let mut out = vec![Vec::new(); self.edges.len()];
            for face in 0..self.faces.len() {
                for e in self.face_edges(face) {
                    out[e].push(face);
                }
            }
            out
        })
    }

    /// The face's distinct edges in loop order (build123d `face.edges()`): a seam once.
    pub fn face_edges(&self, face: usize) -> Vec<usize> {
        let mut out = Vec::new();
        for lp in &self.faces[face].loops {
            for &(e, _) in &lp.edges {
                if !out.contains(&e) {
                    out.push(e);
                }
            }
        }
        out
    }

    /// The edges two faces share, in *a*'s edge order.
    pub fn shared_edges(&self, a: usize, b: usize) -> Vec<usize> {
        let theirs = self.face_edges(b);
        self.face_edges(a)
            .into_iter()
            .filter(|e| theirs.contains(e))
            .collect()
    }

    /// The box of every face of a solid.
    pub fn solid_bounds(&self, solid: usize) -> Bounds {
        let mut b = Bounds::empty();
        for &f in &self.solids[solid].faces {
            b.merge(&self.face_bounds(f));
        }
        b
    }

    /// How the solid turns where two faces meet (`FaceGraph.arc`): `None` when they share no
    /// edge, `Unknown` when their shared edges disagree or a normal cannot be read.
    pub fn arc(&self, a: usize, b: usize) -> Option<Arc> {
        let shared = self.shared_edges(a, b);
        let first = self.arc_at(a, b, *shared.first()?);
        Some(if shared.iter().all(|&e| self.arc_at(a, b, e) == first) {
            first
        } else {
            Arc::Unknown
        })
    }

    /// The turn at one shared edge: walking the edge in *a*'s boundary direction, left (in *a*'s
    /// surface) points into *a*; which side of *b* that is decides convex or concave.
    fn arc_at(&self, a: usize, b: usize, edge: usize) -> Arc {
        let Some(forward) = self.faces[a]
            .loops
            .iter()
            .flat_map(|l| &l.edges)
            .find(|e| e.0 == edge)
            .map(|e| e.1)
        else {
            return Arc::Unknown;
        };
        let ed = &self.edges[edge];
        let (t0, t1) = edge_interval(&ed.curve, ed.start, ed.end, ed.same_sense, ed.is_closed());
        let middle = 0.5 * (t0 + t1);
        let h = 1e-6 * (t1 - t0).abs().max(1e-12);
        let tangent = geom::sub(ed.curve.value(middle + h), ed.curve.value(middle - h));
        // The interval already runs start → end; the loop may use the edge the other way.
        let walk = if (t1 >= t0) == forward {
            tangent
        } else {
            geom::scale(tangent, -1.0)
        };
        let (Some(direction), point) = (geom::unit(walk), ed.curve.value(middle)) else {
            return Arc::Unknown;
        };
        let normal = |face: usize| {
            let (u, v) = self.faces[face].surface.parameters(point, None)?;
            self.face_normal(face, u, v)
        };
        let (Some(na), Some(nb)) = (normal(a), normal(b)) else {
            return Arc::Unknown;
        };
        if 1.0 - geom::dot(na, nb) <= geom::SMOOTH_ARC_GAP {
            return Arc::Smooth;
        }
        // A closed edge's recorded direction is not evidence (see `solid_is_valid`): step to
        // its left in *a* and walk the other way if that leaves the face.
        let mut direction = direction;
        if ed.is_closed() {
            let step = 1e-4 * self.face_bounds(a).diagonal().max(1e-9);
            let surface = &self.faces[a].surface;
            let hint = surface.parameters(point, None);
            let on_face = |side: f64| {
                let left = geom::scale(geom::cross(na, direction), side * step);
                let (u, v) = surface.parameters(geom::add(point, left), hint)?;
                Some(self.domain(a)?.contains(u, v))
            };
            // Only clear evidence (the face on the right, not the left) overrides the record.
            if on_face(1.0) == Some(false) && on_face(-1.0) == Some(true) {
                direction = geom::scale(direction, -1.0);
            }
        }
        let into_a = geom::cross(na, direction);
        let first = geom::dot(into_a, nb);
        // (Faces tangent but for a tilt along the edge, normals nearly equal, also leave the
        // first order at round-off; they keep its sign, as OpenCascade's reading does.)
        if first.abs() > geom::SMOOTH_ARC_GAP || geom::dot(na, nb) > 0.0 {
            return if first < 0.0 {
                Arc::Convex
            } else {
                Arc::Concave
            };
        }
        // Outward normals opposite (two surfaces kissing): the first order is round-off, so
        // read the second. Both faces leave the edge along `into_a`; step h along each, place
        // the point on its surface, and measure how far it has moved along the other face's
        // normal from the edge point's own foot (an edge may lie off its surfaces within
        // tolerance). A positive sum puts *a* on the outside of *b*: the material fills all
        // round the edge but a zero-angle notch (concave); a negative one leaves it a
        // zero-angle knife (convex).
        let step = 1e-4 * self.face_bounds(a).diagonal().max(1e-9);
        let lift = |face: usize, other: V3| {
            let surface = &self.faces[face].surface;
            let (u0, v0) = surface.parameters(point, None)?;
            let off = geom::add(point, geom::scale(into_a, step));
            let (u, v) = surface.parameters(off, Some((u0, v0)))?;
            let moved = geom::sub(surface.value(u, v), surface.value(u0, v0));
            self.domain(face)?
                .contains(u, v)
                .then(|| geom::dot(moved, other))
        };
        let (Some(la), Some(lb)) = (lift(a, nb), lift(b, na)) else {
            return Arc::Unknown;
        };
        // Below this the lifts are round-off in the placed points (coplanar faces back to back).
        let floor = 1e-12 * point.iter().fold(1.0f64, |m, c| m.max(c.abs()));
        let sum = la + lb;
        if sum.abs() <= floor {
            Arc::Unknown
        } else if sum < 0.0 {
            Arc::Convex
        } else {
            Arc::Concave
        }
    }

    /// The face's outward normal at (u, v): the surface normal, flipped for a reversed face.
    pub fn face_normal(&self, face: usize, u: f64, v: f64) -> Option<V3> {
        let f = &self.faces[face];
        let n = f.surface.normal(u, v)?;
        Some(if f.reversed { geom::scale(n, -1.0) } else { n })
    }

    /// The distinct faces sharing an edge with *face*, in the face's own edge order
    /// (`_adjacency.neighbours`).
    pub fn neighbours(&self, face: usize) -> Vec<usize> {
        let edge_faces = self.edge_faces();
        let mut out = Vec::new();
        for lp in &self.faces[face].loops {
            for &(e, _) in &lp.edges {
                for &other in &edge_faces[e] {
                    if other != face && !out.contains(&other) {
                        out.push(other);
                    }
                }
            }
        }
        out
    }

    /// Does this face's surface frame already point out of the solid? `None` for surfaces with
    /// no single frame direction to read (`_adjacency.frame_points_outward`).
    pub fn frame_points_outward(&self, face: usize) -> Option<bool> {
        let f = &self.faces[face];
        let frame = match &f.surface {
            Surface::Plane { frame }
            | Surface::Cylinder { frame, .. }
            | Surface::Sphere { frame, .. } => frame,
            _ => return None,
        };
        Some(!f.reversed == frame.direct())
    }

    /// The topological half of `BRepCheck` validity that the evidence path depends on: every
    /// edge of the solid is used by exactly two faces (or twice by one, as a seam), and two
    /// different faces sharing an open edge run it in opposite directions, so the shell is closed
    /// and orientable. An edge used more often is valid as `BRepCheck_Shell` reads it: as often
    /// each way, the shell not parted there ([`Self::multi_edges_join_one_set`]).
    ///
    /// A closed edge (a full circle) is exempt from the direction test: it joins itself
    /// whichever way it is run, so its recorded direction is not evidence. OpenCascade writes
    /// some toroidal faces' circles the wrong way round and re-derives the direction from the
    /// parameter-space curves on reading.
    ///
    /// A solid with a face or edge whose geometry did not resolve is not valid: its shape is
    /// not known.
    pub fn solid_is_valid(&self, solid: usize) -> bool {
        self.valid_solids.get_or_init(|| {
            (0..self.solids.len())
                .map(|s| self.check_solid(s))
                .collect()
        })[solid]
    }

    /// The faces of solid *solid* whose geometry `BRepCheck`'s face check faults, ascending:
    /// loops that cross or are wound against the outward normal ([`crate::validity`], planar
    /// faces).
    pub fn bad_faces(&self, solid: usize) -> &[usize] {
        &self.bad_faces.get_or_init(|| {
            self.solids
                .iter()
                .map(|s| {
                    let mut bad: Vec<usize> = s
                        .faces
                        .iter()
                        .copied()
                        .filter(|&f| self.face_loops_faulted(f))
                        .collect();
                    bad.sort_unstable();
                    bad.dedup();
                    bad
                })
                .collect()
        })[solid]
    }

    /// Valid as `BRepCheck_Analyzer::IsValid` reads it, as far as the kernel checks: the
    /// topology sound ([`Part::solid_is_valid`]) and no face faulted ([`Part::bad_faces`]).
    pub fn solid_is_geometrically_valid(&self, solid: usize) -> bool {
        self.solid_is_valid(solid) && self.bad_faces(solid).is_empty()
    }

    fn check_solid(&self, solid: usize) -> bool {
        let faces = &self.solids[solid].faces;
        if faces.iter().any(|f| self.unresolved_faces.contains(f)) {
            return false;
        }
        let mut uses: std::collections::BTreeMap<usize, Vec<(usize, bool)>> = Default::default();
        for &f in faces {
            for lp in &self.faces[f].loops {
                for &(e, forward) in &lp.edges {
                    uses.entry(e).or_default().push((f, forward));
                }
            }
        }
        let sound = !uses.is_empty()
            && !uses.keys().any(|e| self.unresolved_edges.contains(e))
            && uses.iter().all(|(&e, u)| match u.as_slice() {
                [(fa, da), (fb, db)] => fa == fb || da != db || self.edges[e].is_closed(),
                [_] => false,
                _ => {
                    let forward = u.iter().filter(|(_, d)| *d).count();
                    2 * forward == u.len()
                }
            });
        sound && self.multi_edges_join_one_set(&uses)
    }

    /// `BRepCheck_Shell`'s multiple connexity: an edge used more than twice (by more than two
    /// face sides, a seam's two counted) is valid when its uses run as often each way and the
    /// faces around every such edge are one set joined through the other edges. A bore
    /// tangent inside a wall meets the wall along one edge four times (the wall, its neighbour
    /// and the bore's seam twice) and stays one shell; two boxes sewn at an edge fall apart into
    /// two sets there (`InvalidMultiConnexity`).
    fn multi_edges_join_one_set(
        &self,
        uses: &std::collections::BTreeMap<usize, Vec<(usize, bool)>>,
    ) -> bool {
        let multi: Vec<&Vec<(usize, bool)>> = uses.values().filter(|u| u.len() > 2).collect();
        if multi.is_empty() {
            return true;
        }
        let mut set: std::collections::BTreeMap<usize, usize> = Default::default();
        fn root(set: &mut std::collections::BTreeMap<usize, usize>, f: usize) -> usize {
            let parent = *set.entry(f).or_insert(f);
            if parent == f {
                return f;
            }
            let top = root(set, parent);
            set.insert(f, top);
            top
        }
        for u in uses.values().filter(|u| u.len() <= 2) {
            let (a, b) = (root(&mut set, u[0].0), root(&mut set, u[u.len() - 1].0));
            set.insert(a, b);
        }
        let mut roots = multi.iter().flat_map(|u| u.iter()).map(|&(f, _)| f);
        let first = roots.next().map(|f| root(&mut set, f));
        roots.all(|f| Some(root(&mut set, f)) == first)
    }
}

/// Bit-equal defining geometry, for [`Part::same_brep`].
fn same_surface(a: &Surface, b: &Surface) -> bool {
    use Surface::*;
    match (a, b) {
        (Plane { frame: a }, Plane { frame: b }) => a == b,
        (
            Cylinder {
                frame: a,
                radius: ar,
            },
            Cylinder {
                frame: b,
                radius: br,
            },
        )
        | (
            Sphere {
                frame: a,
                radius: ar,
            },
            Sphere {
                frame: b,
                radius: br,
            },
        ) => a == b && ar == br,
        (
            Cone {
                frame: a,
                radius: ar,
                semi_angle: aa,
            },
            Cone {
                frame: b,
                radius: br,
                semi_angle: ba,
            },
        ) => a == b && ar == br && aa == ba,
        (
            Torus {
                frame: a,
                major: am,
                minor: an,
            },
            Torus {
                frame: b,
                major: bm,
                minor: bn,
            },
        ) => a == b && am == bm && an == bn,
        (
            Freeform {
                kind: ak,
                surface: a,
            },
            Freeform {
                kind: bk,
                surface: b,
            },
        ) => {
            ak == bk
                && a.degree_u == b.degree_u
                && a.degree_v == b.degree_v
                && a.control_points == b.control_points
                && a.weights == b.weights
                && a.knots_u == b.knots_u
                && a.knots_v == b.knots_v
        }
        _ => false,
    }
}

fn same_pcurve(a: &Pcurve, b: &Pcurve) -> bool {
    match (a, b) {
        (Pcurve::Poles(a), Pcurve::Poles(b)) => a == b,
        (Pcurve::Line { point: a, dir: ad }, Pcurve::Line { point: b, dir: bd }) => {
            a == b && ad == bd
        }
        _ => false,
    }
}
