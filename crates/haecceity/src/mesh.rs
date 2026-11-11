//! Faces as triangle meshes — what OpenCascade's `BRepMesh_IncrementalMesh` gives the Python
//! implementations (specify-core's mesh command and 3-D face picker): each face triangulated to
//! a chordal and an angular deflection, its vertices on the exact surface, its triangles wound
//! by the face's outward normal.
//!
//! 1. Boundary. Each edge is discretised once, for every face that uses it: its samples thinned
//!    so a chord stands for the samples it skips in space and, on each face's surface, its
//!    straight line in that face's parameters stands for them too, and the normals along it
//!    turn by no more than the angular deflection; then divided where a face's surface needs a
//!    closer spacing along it than the chord gives. Samples a face routes along a pole's line,
//!    or that stray within the deflection past a parameter line its boundary follows (an edge
//!    meeting a torus's top circle tangentially, see `straying`), are kept by neither face, and
//!    an edge running through a pole takes the pole as a point.
//!    Both faces of an edge take the same points ([`BoundaryPoint`]), at the edge's own
//!    positions in space, so a closed shell's mesh is watertight.
//! 2. Polygons. A face's boundary loops ([`Part::uv_loops`]: unwrapped across periodic
//!    parameters, with poles and apexes routed along their parameter lines) carry the edges'
//!    points into its parameter space, unwrapped again across the seam of a closed freeform
//!    surface (which the reader marks non-periodic). A periodic face with no seam edge has two
//!    loops that each run once round (a seamless band, a cone up to its apex, a whole sphere
//!    between its poles): it is cut along a straight parameter line between a point of each, one
//!    that misses every hole, and both sides of the cut are the same points. Holes are moved by
//!    whole periods into the region they belong to. A side along a parameter line whose image
//!    strays from its chord (haecceity's closing of a loop along a pole) is sampled; any other
//!    side that strays is a jump across the surface and the face is refused.
//! 3. A constrained Delaunay triangulation (spade) of the polygons runs in parameter
//!    coordinates scaled to the spacing the surface needs in each direction (its normal's
//!    turning and its bending), so triangles lie along a cylinder's rulings rather than across
//!    them. It is refined by inserting edge midpoints until the surface strays from no triangle
//!    by more than the chordal deflection, the normals at an edge's ends differ by at most the
//!    angular deflection, and no triangle leans from the surface by more than it. Boundary sides
//!    are never split. A plane is meshed from its boundary alone.
//! 4. The triangles inside the polygons (by crossing parity, flooding the triangulation) are
//!    mapped onto the exact surface and wound so their normal is the face's outward one:
//!    counter-clockwise in (u, v) is along the surface's natural normal `Su × Sv`, which a
//!    reversed face points against. Points that are one point in space (a pole's parameter
//!    line, the two sides of a cut) are one vertex. A sliver folded against the surface is left
//!    out and counted ([`FaceMesh::folded_dropped`]); a triangle too thin to see is kept.
//!
//! Honest failure: a face whose surface did not resolve, whose loops do not close or collapse,
//! whose boundary crosses itself in parameter space even unthinned, whose boundary jumps across
//! the surface, or whose periodic loops cannot be cut is refused with the reason
//! ([`MeshRefusal`]). No patch is guessed, and no face is dropped silently.
//!
//! The method is specify-core-rust's stand-in (its `triangulate.rs`, upstream need U4), with
//! the boundary discretised per edge instead of per face.

use std::collections::{BTreeMap, VecDeque};
use std::f64::consts::TAU;

use spade::{ConstrainedDelaunayTriangulation, HasPosition, Point2, Triangulation};

use super::brep::Part;
use super::geom::{self, Surface, V3};
use super::uv::UvLoop;

/// Refinement stops after this many rounds of insertions, however far from the tolerance.
const MAX_ROUNDS: usize = 40;
/// A face's triangulation never grows past this many vertices: it bounds the time and memory a
/// single face can take (a large freeform face at a fine deflection). A face stopped by it is
/// still meshed, coarser than asked, and says so ([`FaceMesh::converged`]).
const MAX_VERTICES: usize = 60_000;
/// Samples taken along a parameter line added to a face's boundary (a cut, a pole's line),
/// before thinning.
const CUT_SAMPLES: usize = 128;
/// Boundary thinning, as shares of the chordal deflection: thinning can make two close
/// stretches of a boundary cross in parameter space (a narrow neck of the face), so a face that
/// crosses is tried again with its edges thinned less, down to the samples themselves.
const BOUNDARY_LEVELS: [f64; 4] = [1.0, 0.25, 1.0 / 16.0, 1e-6];

/// Why a face's boundary cannot be triangulated as it is thinned.
const CROSSES: &str = "its boundary crosses itself in parameter space";

/// A mesh vertex on the part's edges, the same in every face that uses that edge: a vertex of
/// the part (the least-numbered one at its point), or a point of an edge's discretisation (its
/// index along the edge, from the start).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BoundaryPoint {
    Vertex(usize),
    Edge { edge: usize, index: usize },
}

/// One face's triangles, wound outward.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct FaceMesh {
    /// Vertices on the exact surface (boundary vertices at their edge's positions).
    pub points: Vec<V3>,
    /// Each vertex's surface parameters (for a vertex the face meets twice, a cut or a seam,
    /// the first).
    pub uv: Vec<(f64, f64)>,
    /// Vertex indices, counter-clockwise seen from outside the solid.
    pub triangles: Vec<[usize; 3]>,
    /// Per vertex, where it lies on the part's edges; `None` inside the face.
    pub boundary: Vec<Option<BoundaryPoint>>,
    /// Whether refinement met the deflections everywhere; false when [`MAX_VERTICES`] or
    /// [`MAX_ROUNDS`] stopped it first.
    pub converged: bool,
    /// Triangles left out because they faced against the surface (a sliver or speck folded at a
    /// degenerate side or corner, see `finish`), and their total area: kept count of so the gap
    /// they leave is reported, not hidden.
    pub folded_dropped: (usize, f64),
}

impl FaceMesh {
    /// The triangle's area in space.
    pub fn triangle_area(&self, t: usize) -> f64 {
        let [a, b, c] = self.triangles[t].map(|i| self.points[i]);
        0.5 * geom::norm(geom::cross(geom::sub(b, a), geom::sub(c, a)))
    }

    /// The total area of the triangles.
    pub fn area(&self) -> f64 {
        (0..self.triangles.len())
            .map(|t| self.triangle_area(t))
            .sum()
    }
}

/// Why a face was not triangulated.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MeshRefusal {
    pub reason: String,
}

impl std::fmt::Display for MeshRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}

impl std::error::Error for MeshRefusal {}

impl Part {
    /// `face` triangulated to the chordal `deflection` (mm) and `angular` deflection (radians),
    /// or why it cannot be. specify-core meshes at a deflection of 0.001 of the part's diagonal
    /// and 0.3 rad.
    ///
    /// The face is meshed on its own: its edges are thinned as finely as it needs. In
    /// [`Part::mesh`] an edge is thinned as finely as either of its faces needs, so where a
    /// neighbour needed its shared edge thinned less (a narrow neck of that face) this face's
    /// mesh differs from its mesh there.
    pub fn triangulate(
        &self,
        face: usize,
        deflection: f64,
        angular: f64,
    ) -> Result<FaceMesh, MeshRefusal> {
        let mut mesher = PartMesher::new(self, deflection, angular)?;
        let mut result = Err(CROSSES.to_owned());
        for level in 0..BOUNDARY_LEVELS.len() {
            result = mesher.face(face, &|_| level);
            if !matches!(&result, Err(why) if why == CROSSES) {
                break;
            }
        }
        result.map_err(|reason| MeshRefusal { reason })
    }

    /// Every face triangulated to the chordal `deflection` (mm) and `angular` deflection
    /// (radians), by face index; a face that cannot be is refused with the reason. Neighbouring
    /// faces share their edges' points, so the mesh of a closed shell whose faces all
    /// triangulate is watertight.
    pub fn mesh(&self, deflection: f64, angular: f64) -> Vec<Result<FaceMesh, MeshRefusal>> {
        let n = self.faces.len();
        let mut mesher = match PartMesher::new(self, deflection, angular) {
            Ok(m) => m,
            Err(refusal) => return vec![Err(refusal); n],
        };
        let mut level = vec![0usize; self.edges.len()];
        let mut out: Vec<Option<Result<FaceMesh, String>>> = vec![None; n];
        let mut dirty = vec![true; n];
        let edge_faces = self.edge_faces();
        loop {
            for f in 0..n {
                if dirty[f] {
                    out[f] = Some(mesher.face(f, &|e| level[e]));
                    dirty[f] = false;
                }
            }
            // A face whose boundary crosses itself: its edges at its finest-thinned level are
            // thinned one level less, and every face on them meshed again.
            let mut raised = false;
            for (f, result) in out.iter().enumerate() {
                if !matches!(result, Some(Err(why)) if why == CROSSES) {
                    continue;
                }
                let edges = self.face_edges(f);
                let Some(least) = edges.iter().map(|&e| level[e]).min() else {
                    continue;
                };
                if least + 1 >= BOUNDARY_LEVELS.len() {
                    continue;
                }
                for &e in &edges {
                    if level[e] == least {
                        level[e] = least + 1;
                        for &g in &edge_faces[e] {
                            dirty[g] = true;
                        }
                    }
                }
                raised = true;
            }
            if !raised {
                break;
            }
        }
        out.into_iter()
            .map(|r| {
                r.unwrap_or_else(|| Err("the face was not meshed".into()))
                    .map_err(|reason| MeshRefusal { reason })
            })
            .collect()
    }
}

/// A face's surface with its periods: 2π in the periodic directions of the analytic kinds, and
/// the parameter range of a freeform surface in a direction where its two sides meet (a closed
/// B-spline surface, which haecceity marks non-periodic). A closed freeform direction is
/// evaluated wrapped back into the surface's range, so a loop unwrapped across its seam, and a
/// band cut open along it, can be triangulated like a cylinder's.
#[derive(Clone, Copy)]
struct Sheet<'a> {
    surface: &'a Surface,
    period: [Option<f64>; 2],
    /// Per direction, the start of the range a closed freeform direction wraps into.
    wrap: [Option<f64>; 2],
}

impl<'a> Sheet<'a> {
    fn new(surface: &'a Surface) -> Self {
        let (pu, pv) = surface.periodic();
        let mut sheet = Sheet {
            surface,
            period: [pu.then_some(TAU), pv.then_some(TAU)],
            wrap: [None, None],
        };
        if let Surface::Freeform { surface: nurbs, .. } = surface {
            let (u0, u1, v0, v1) = nurbs.domain();
            // The two sides of the range meet if they are the same points in space.
            let meets = |side: &dyn Fn(f64) -> (V3, V3)| {
                (0..=8).all(|i| {
                    let (a, b) = side(i as f64 / 8.0);
                    geom::dist(a, b) <= 1e-6 * (1.0 + geom::norm(a))
                })
            };
            if u1 > u0
                && meets(&|t| {
                    (
                        surface.value(u0, v0 + t * (v1 - v0)),
                        surface.value(u1, v0 + t * (v1 - v0)),
                    )
                })
            {
                sheet.period[0] = Some(u1 - u0);
                sheet.wrap[0] = Some(u0);
            }
            if v1 > v0
                && meets(&|t| {
                    (
                        surface.value(u0 + t * (u1 - u0), v0),
                        surface.value(u0 + t * (u1 - u0), v1),
                    )
                })
            {
                sheet.period[1] = Some(v1 - v0);
                sheet.wrap[1] = Some(v0);
            }
        }
        sheet
    }

    /// (u, v) wrapped into a closed freeform surface's range.
    fn wrapped(&self, u: f64, v: f64) -> (f64, f64) {
        let fold = |x: f64, axis: usize| match (self.wrap[axis], self.period[axis]) {
            (Some(start), Some(period)) => start + (x - start).rem_euclid(period),
            _ => x,
        };
        (fold(u, 0), fold(v, 1))
    }

    fn value(&self, u: f64, v: f64) -> V3 {
        let (u, v) = self.wrapped(u, v);
        self.surface.value(u, v)
    }

    fn normal(&self, u: f64, v: f64) -> Option<V3> {
        let (u, v) = self.wrapped(u, v);
        self.surface.normal(u, v)
    }

    /// `x` moved by whole periods of direction `axis` to lie nearest `reference`.
    fn nearest(&self, x: f64, reference: f64, axis: usize) -> f64 {
        match self.period[axis] {
            Some(period) => x + period * ((reference - x) / period).round(),
            None => x,
        }
    }
}

/// What every edge discretisation and face polygon reads of a face: its sheet, its loops
/// unwrapped, each edge sample's parameters, and the parameter scale its triangulation uses.
struct Setup<'a> {
    sheet: Sheet<'a>,
    /// Per loop, its points unwrapped across a closed freeform surface's seam too.
    loops: Vec<Vec<(f64, f64)>>,
    /// (edge, sample index) → the sample's parameters in this face (its first use).
    uv: BTreeMap<(usize, usize), (f64, f64)>,
    scale: (f64, f64),
}

/// A face's view of an edge's samples: its setup, and each sample's parameters and normal
/// there (`None` where it routes the sample along a pole's line).
type View<'s, 'a> = (&'s Setup<'a>, Vec<Option<(f64, f64)>>, Vec<Option<V3>>);

/// An edge's points, shared by the faces that use it, in order from the start.
#[derive(Clone, Debug)]
struct Plan {
    points: Vec<V3>,
    /// Per sample, its index in `points` if kept.
    at_sample: Vec<Option<usize>>,
    /// Per segment (sample `i` to `i + 1`), the points inside it and their fractions along it,
    /// in order.
    between: Vec<Vec<(usize, f64)>>,
}

/// A mesh vertex's identity across the faces and within one: a point on the part's edges, a
/// singular parameter line (one point in space: the part's vertex there, if it has one), a
/// point of a cut (both sides), or a point sampled on a side along a parameter line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Shared(BoundaryPoint),
    Pole(u8, Option<BoundaryPoint>),
    Cut(usize),
    Side(usize),
}

/// A boundary polygon's point.
#[derive(Clone, Copy, Debug)]
struct BNode {
    uv: (f64, f64),
    point: V3,
    key: Option<Key>,
}

struct PartMesher<'a> {
    part: &'a Part,
    deflection: f64,
    /// The boundary's angular deflection: half the triangles'.
    angular: f64,
    setups: Vec<Option<Result<Setup<'a>, String>>>,
    plans: BTreeMap<(usize, usize), Plan>,
    /// Per vertex of the part, the first vertex at the same point: two loops touching at a
    /// point each with its own vertex there meet at one mesh vertex.
    alias: Vec<usize>,
}

impl<'a> PartMesher<'a> {
    fn new(part: &'a Part, deflection: f64, angular: f64) -> Result<Self, MeshRefusal> {
        if !(deflection > 0.0 && deflection.is_finite()) {
            return Err(MeshRefusal {
                reason: format!("deflection {deflection} is not a positive length"),
            });
        }
        if !(angular > 0.0 && angular.is_finite()) {
            return Err(MeshRefusal {
                reason: format!("angular deflection {angular} is not a positive angle"),
            });
        }
        Ok(PartMesher {
            part,
            deflection,
            // OpenCascade's BRepMesh, given an angular deflection, divides a full circle into
            // segments spanning about half of it (42 per circle at 0.3 rad, whatever the
            // radius): the boundary is thinned to half the angular deflection, the triangles
            // held to the whole of it.
            angular: 0.5 * angular,
            setups: (0..part.faces.len()).map(|_| None).collect(),
            plans: BTreeMap::new(),
            alias: vertex_alias(part),
        })
    }

    fn setup(&mut self, face: usize) {
        if self.setups[face].is_none() {
            self.setups[face] = Some(self.make_setup(face));
        }
    }

    fn make_setup(&self, face: usize) -> Result<Setup<'a>, String> {
        let part = self.part;
        let f = &part.faces[face];
        if let Surface::Other { kind } = &f.surface {
            return Err(format!("its surface ({kind}) did not resolve"));
        }
        let sheet = Sheet::new(&f.surface);
        let raw = part
            .uv_loops(face)
            .ok_or("its boundary has no parameter-space loops")?;
        let mut loops = Vec::with_capacity(raw.len());
        let mut uv = BTreeMap::new();
        let mut winding_u = 0;
        let mut box_points: Vec<(f64, f64)> = Vec::new();
        for (li, lp) in raw.iter().enumerate() {
            let mut points = lp.points.clone();
            for k in 1..points.len() {
                let (last, here) = (points[k - 1], points[k]);
                points[k] = (
                    sheet.nearest(here.0, last.0, 0),
                    sheet.nearest(here.1, last.1, 1),
                );
            }
            for (k, &p) in points.iter().enumerate() {
                if let Some(Some((place, sample))) = lp.sources.get(k) {
                    let e = f.loops[li].edges[*place].0;
                    uv.entry((e, *sample)).or_insert(p);
                }
            }
            if let (Some(a), Some(b)) = (points.first(), points.last())
                && sheet.period[0].is_some_and(|p| (b.0 - a.0).abs() > 0.5 * p)
            {
                winding_u += 1;
            }
            for (e, sample) in straying(part, face, li, &sheet, lp, &points, self.deflection) {
                uv.remove(&(e, sample));
            }
            box_points.extend(&points);
            loops.push(points);
        }
        // A cone face bounded by one circle runs to its apex, which may have no vertex loop.
        if winding_u == 1
            && let Surface::Cone {
                radius, semi_angle, ..
            } = f.surface
        {
            box_points.push((0.0, -radius / semi_angle.sin()));
        }
        let scale = parameter_scale(&sheet, &box_points, self.deflection, self.angular)?;
        Ok(Setup {
            sheet,
            loops,
            uv,
            scale,
        })
    }

    fn plan(&mut self, edge: usize, level: usize) {
        if self.plans.contains_key(&(edge, level)) {
            return;
        }
        for &f in &self.part.edge_faces()[edge] {
            self.setup(f);
        }
        let plan = self.make_plan(edge, level);
        self.plans.insert((edge, level), plan);
    }

    /// Edge `edge`'s points at thinning level `level` (see [`BOUNDARY_LEVELS`]).
    fn make_plan(&self, edge: usize, level: usize) -> Plan {
        let part = self.part;
        let samples = &part.edges[edge].samples;
        let n = samples.len();
        let boundary = BOUNDARY_LEVELS[level] * self.deflection;
        // The faces' views of the samples.
        let mut views: Vec<View<'_, 'a>> = Vec::new();
        for &f in &part.edge_faces()[edge] {
            let Some(Ok(setup)) = &self.setups[f] else {
                continue;
            };
            if views.iter().any(|v| std::ptr::eq(v.0, setup)) {
                continue;
            }
            let uv: Vec<Option<(f64, f64)>> =
                (0..n).map(|k| setup.uv.get(&(edge, k)).copied()).collect();
            let normals = uv
                .iter()
                .map(|p| p.and_then(|(u, v)| setup.sheet.normal(u, v)))
                .collect();
            views.push((setup, uv, normals));
        }
        // A sample some face has no parameters for lies within a degree of a pole there, where
        // that face routes its boundary along the pole's line instead: neither face keeps it.
        let skip: Vec<bool> = (0..n)
            .map(|k| views.iter().any(|v| v.1[k].is_none()))
            .collect();
        let on: Vec<OnSurface<'_>> = views
            .iter()
            .map(|(setup, uv, normals)| OnSurface {
                sheet: setup.sheet,
                uv,
                normals,
            })
            .collect();
        let kept = thin(samples, &[], &skip, &on, boundary, Some(self.angular));
        // The points by place: each kept sample, and divisions where a kept chord is longer
        // than a curved face's spacing across it.
        let mut at: BTreeMap<(usize, u64), V3> = BTreeMap::new();
        let lerp = |s: usize, t: f64| {
            if t == 0.0 {
                samples[s]
            } else {
                geom::add(
                    samples[s],
                    geom::scale(geom::sub(samples[s + 1], samples[s]), t),
                )
            }
        };
        let add = |at: &mut BTreeMap<(usize, u64), V3>, s: usize, t: f64, p: V3| {
            at.entry((s, t.to_bits())).or_insert(p);
        };
        for (w, &i) in kept.iter().enumerate() {
            add(&mut at, i, 0.0, samples[i]);
            let Some(&j) = kept.get(w + 1) else { break };
            let pieces = views
                .iter()
                .filter(|v| !matches!(v.0.sheet.surface, Surface::Plane { .. }))
                .filter_map(|(setup, uv, _)| {
                    let (a, b) = (uv[i]?, uv[j]?);
                    let (su, sv) = setup.scale;
                    let length = ((b.0 - a.0) * su).hypot((b.1 - a.1) * sv);
                    Some(length.round().clamp(1.0, 1e4) as usize)
                })
                .max()
                .unwrap_or(1);
            // Across a pole's window a face's parameters say nothing of the spacing, and the
            // samples there are skipped: no division. A face routes its boundary there through
            // the pole itself, so where the edge runs through a pole (to the deflection) the
            // pole is the edge's point too.
            if skip[i + 1..j].iter().any(|&s| s) {
                let pole = views
                    .iter()
                    .flat_map(|v| singular_points(&v.0.sheet))
                    .filter_map(|(_, p)| {
                        // Not a pole at the chord's ends (a vertex there is the pole already).
                        let apart = |q: V3| geom::dist(q, p) > 1e-3 * self.deflection;
                        let (k, d) = (i + 1..j)
                            .map(|k| (k, geom::dist(samples[k], p)))
                            .min_by(|a, b| a.1.total_cmp(&b.1))?;
                        (d <= boundary && apart(samples[i]) && apart(samples[j])).then_some((k, p))
                    })
                    .next();
                if let Some((k, p)) = pole {
                    add(&mut at, k.min(j - 1), 0.5, p);
                }
                continue;
            }
            if pieces <= 1 {
                continue;
            }
            if j - i >= pieces {
                // Enough samples between: keep evenly spaced ones.
                for m in 1..pieces {
                    let k = i + (m * (j - i) + pieces / 2) / pieces;
                    if k > i && k < j {
                        add(&mut at, k, 0.0, samples[k]);
                    }
                }
            } else {
                // Each sample between, and each segment divided evenly along its chord (exact
                // on a line; within the samples' chord tolerance of a curve).
                let per = pieces.div_ceil(j - i);
                for s in i..j {
                    add(&mut at, s, 0.0, lerp(s, 0.0));
                    for m in 1..per {
                        let t = m as f64 / per as f64;
                        add(&mut at, s, t, lerp(s, t));
                    }
                }
            }
        }
        let mut plan = Plan {
            points: Vec::with_capacity(at.len()),
            at_sample: vec![None; n],
            between: vec![Vec::new(); n.saturating_sub(1)],
        };
        for ((s, t), p) in at {
            let (index, t) = (plan.points.len(), f64::from_bits(t));
            plan.points.push(p);
            if t == 0.0 {
                plan.at_sample[s] = Some(index);
            } else {
                plan.between[s].push((index, t));
            }
        }
        plan
    }

    /// The face meshed, each edge `e` at thinning level `level(e)`.
    fn face(&mut self, face: usize, level: &dyn Fn(usize) -> usize) -> Result<FaceMesh, String> {
        let part = self.part;
        let edges = part.face_edges(face);
        self.setup(face);
        for &e in &edges {
            self.plan(e, level(e));
        }
        let setup = match &self.setups[face] {
            Some(Ok(s)) => s,
            Some(Err(why)) => return Err(why.clone()),
            None => return Err("the face was not set up".into()),
        };
        let plans: BTreeMap<usize, &Plan> = edges
            .iter()
            .map(|&e| (e, &self.plans[&(e, level(e))]))
            .collect();
        triangulate_face(
            part,
            face,
            setup,
            &plans,
            &self.alias,
            self.deflection,
            self.angular,
        )
    }
}

/// The samples (edge, index) of a loop that stray past a parameter line its boundary follows,
/// within the deflection of it, where they meet it: neither face of their edge keeps them (see
/// [`PartMesher::make_plan`]), so the loop is a simple polygon.
///
/// An edge of the loop all of whose points share one parameter value (a torus face's side
/// along v = π/2) is a line the face lies on one side of. An edge meeting it at a vertex
/// tangentially may run along beyond the line, off the face, within the file's tolerance
/// before it leaves on the face's side (cgb207 face 50's edge 63 reaches v = 1.5719 near
/// vertex 48, 0.1 µm off the torus's top circle), and its samples there cross the line's.
/// Walking each neighbouring edge from the shared vertex, the samples within the deflection of
/// the line (in space) up to the first one clearly off it are the meeting; those of them beside
/// the line (within its span in the other parameter, where they can cross it) on its far side
/// from that one are the strays. Dropped, the edge's chord from the vertex stands for them:
/// they lie within the deflection of the line, and of the face. A stray farther off is kept,
/// and the face is then refused if its boundary crosses.
fn straying(
    part: &Part,
    face: usize,
    li: usize,
    sheet: &Sheet<'_>,
    lp: &UvLoop,
    points: &[(f64, f64)],
    deflection: f64,
) -> Vec<(usize, usize)> {
    let f = &part.faces[face];
    let edges = &f.loops[li].edges;
    let n = edges.len();
    // Per edge of the loop, its points' indices in loop order.
    let mut at: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (k, source) in lp.sources.iter().enumerate() {
        if let Some((place, _)) = source {
            at[*place].push(k);
        }
    }
    // How many times the face's loops use each edge, counted once rather than per stray.
    let mut used: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for &(g, _) in f.loops.iter().flat_map(|l| &l.edges) {
        *used.entry(g).or_insert(0) += 1;
    }
    let uses = |e: usize| used.get(&e).copied().unwrap_or(0);
    let mut out = Vec::new();
    for line in 0..n {
        let ks = &at[line];
        if ks.len() < 2 {
            continue;
        }
        for axis in 0..2 {
            let pick = |p: (f64, f64)| if axis == 0 { p.0 } else { p.1 };
            let c = pick(points[ks[0]]);
            let tiny = 1e-9 * (1.0 + c.abs());
            if ks.iter().any(|&k| (pick(points[k]) - c).abs() > tiny) {
                continue;
            }
            // The line's span in the other parameter: only a stray beside it can cross it.
            let across = |p: (f64, f64)| if axis == 0 { p.1 } else { p.0 };
            let (lo, hi) = ks
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &k| {
                    (lo.min(across(points[k])), hi.max(across(points[k])))
                });
            // The edge before the line, walked back from its end, and the edge after it,
            // walked on from its start.
            let before: Vec<usize> = at[(line + n - 1) % n].iter().rev().copied().collect();
            let after: Vec<usize> = at[(line + 1) % n].clone();
            for walk in [before, after] {
                let mut meeting: Vec<(usize, f64)> = Vec::new();
                let mut inside = None;
                for &k in &walk {
                    let p = points[k];
                    let d = pick(p) - c;
                    let on = if axis == 0 { (c, p.1) } else { (p.0, c) };
                    let Some((place, sample)) = lp.sources[k] else {
                        break;
                    };
                    let e = edges[place].0;
                    let off = geom::dist(sheet.value(on.0, on.1), part.edges[e].samples[sample]);
                    if off > deflection {
                        inside = Some(d.signum());
                        break;
                    }
                    if d.abs() > tiny && across(p) > lo && across(p) < hi {
                        meeting.push((k, d));
                    }
                }
                let Some(side) = inside else { continue };
                for (k, d) in meeting {
                    // Only sourced samples meet the edge (above).
                    let Some((place, sample)) = lp.sources[k] else {
                        continue;
                    };
                    let e = edges[place].0;
                    if d.signum() != side && uses(e) == 1 {
                        out.push((e, sample));
                    }
                }
            }
        }
    }
    out
}

/// Per vertex of the part, the least-numbered vertex at exactly the same point.
fn vertex_alias(part: &Part) -> Vec<usize> {
    let mut at: Vec<(usize, V3)> = Vec::new();
    for edge in &part.edges {
        at.push((edge.vertices.0, edge.start));
        at.push((edge.vertices.1, edge.end));
    }
    let count = at.iter().map(|v| v.0 + 1).max().unwrap_or(0);
    let mut first: BTreeMap<[u64; 3], usize> = BTreeMap::new();
    let mut alias: Vec<usize> = (0..count).collect();
    at.sort_by_key(|v| v.0);
    for (v, p) in at {
        let id = *first.entry(p.map(f64::to_bits)).or_insert(v);
        alias[v] = alias[v].min(id);
    }
    alias
}

/// The face's polygons from its loops and its edges' plans, triangulated.
fn triangulate_face(
    part: &Part,
    face: usize,
    setup: &Setup<'_>,
    plans: &BTreeMap<usize, &Plan>,
    alias: &[usize],
    deflection: f64,
    angular: f64,
) -> Result<FaceMesh, String> {
    let f = &part.faces[face];
    let sheet = setup.sheet;
    let surface = sheet.surface;
    let period = sheet.period;
    let raw = part
        .uv_loops(face)
        .ok_or("its boundary has no parameter-space loops")?;
    let poles = Poles::new(part, face, &sheet, plans, alias, deflection);

    // Each loop as polygon points, sorted into closed loops and loops that run once round.
    let mut closed: Vec<Vec<BNode>> = Vec::new();
    let mut winding_u: Vec<Vec<BNode>> = Vec::new();
    let mut winding_v: Vec<Vec<BNode>> = Vec::new();
    for (li, lp) in raw.iter().enumerate() {
        let points = &setup.loops[li];
        if points.len() < 2 {
            // A vertex loop on a surface with no periodic direction: a single point, no
            // boundary to triangulate against.
            continue;
        }
        let nodes = loop_nodes(
            part,
            face,
            li,
            &lp.sources,
            &lp.points,
            points,
            plans,
            alias,
            &poles,
        );
        let (first, last) = (nodes[0], nodes[nodes.len() - 1]);
        let (du, dv) = (last.uv.0 - first.uv.0, last.uv.1 - first.uv.1);
        // Runs once round a periodic direction: ends about a period from where it began.
        let round = |d: f64, axis: usize| period[axis].is_some_and(|p| d.abs() > 0.5 * p);
        if round(du, 0) && !round(dv, 1) {
            winding_u.push(nodes);
        } else if round(dv, 1) && !round(du, 0) {
            winding_v.push(nodes);
        } else {
            if round(du, 0) || round(dv, 1) {
                return Err("a boundary loop runs round both directions".into());
            }
            let mut nodes = nodes;
            if !(first.key.is_some() && first.key == last.key) {
                let gap = geom::dist(first.point, last.point);
                if gap > deflection {
                    return Err(format!(
                        "a boundary loop does not close in parameter space (gap {gap:.3e} mm)"
                    ));
                }
            }
            nodes.pop();
            if nodes.len() < 3 {
                return Err("a boundary loop has fewer than three points".into());
            }
            closed.push(nodes);
        }
    }
    // A cone face bounded by one circle runs to its apex, which may have no vertex loop.
    if winding_u.len() == 1
        && winding_v.is_empty()
        && let Surface::Cone {
            radius, semi_angle, ..
        } = *surface
    {
        let v = -radius / semi_angle.sin();
        winding_u.push(
            (0..=64)
                .map(|i| poles.node((TAU * i as f64 / 64.0, v)))
                .collect(),
        );
    }

    let mut polygons = match (winding_u.len(), winding_v.len()) {
        (0, 0) => outer_and_holes(closed, period)?,
        (2, 0) => strip(
            &sheet,
            setup.scale,
            winding_u,
            closed,
            deflection,
            angular,
            false,
        )?,
        (0, 2) => strip(
            &sheet,
            setup.scale,
            winding_v,
            closed,
            deflection,
            angular,
            true,
        )?,
        (u, v) => {
            return Err(format!(
                "{u} loop(s) run round u and {v} round v: no single cut makes it a polygon"
            ));
        }
    };
    // Points sampled on sides along parameter lines, each one vertex wherever it recurs.
    let mut side_points: Vec<V3> = Vec::new();
    for polygon in &mut polygons {
        dedup_consecutive(polygon);
        if polygon.len() < 3 {
            return Err("a boundary polygon collapses to fewer than three points".into());
        }
        // Each side, a straight line in parameter space, must lie along the boundary in space.
        // A side along a parameter line that strays from its chord is a line of the parameter
        // region itself (haecceity closes a loop round a sphere along its pole's line and the
        // meridian to it, with no samples between): it is sampled like an edge. Any other side
        // that strays jumps across the surface (a seam of a closed surface the loop's
        // parameters were not unwrapped across): no triangulation of that polygon is the face.
        let mut sampled = Vec::with_capacity(polygon.len());
        for i in 0..polygon.len() {
            let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
            sampled.push(a);
            let mid = sheet.value(0.5 * (a.uv.0 + b.uv.0), 0.5 * (a.uv.1 + b.uv.1));
            let off = segment_distance(mid, a.point, b.point);
            if off <= deflection {
                continue;
            }
            // A side between two points of one edge is that edge's chord, thinned to the
            // deflection at its samples (the exact curve may stray a hair further between them):
            // both faces of the edge keep it.
            let on_edge = matches!((a.key, b.key), (Some(Key::Shared(_)), Some(Key::Shared(_))));
            let (a, b) = (a.uv, b.uv);
            let scale = 1.0 + a.0.abs().max(a.1.abs());
            let iso = (a.0 - b.0).abs() <= 1e-9 * scale || (a.1 - b.1).abs() <= 1e-9 * scale;
            if iso && !on_edge {
                // Sampled from its lower end, so a meridian the polygon runs up on one side and
                // down on the other (a loop closed round a sphere through its pole) is sampled
                // alike both ways, and its points are one vertex each.
                let rising = (a.1, a.0) <= (b.1, b.0);
                let mut points = if rising {
                    straight_side(&sheet, None, a, b, deflection, angular)
                } else {
                    straight_side(&sheet, None, b, a, deflection, angular)
                };
                if !rising {
                    points.reverse();
                }
                for uv in points {
                    let mut node = poles.node(uv);
                    if node.key.is_none() {
                        let id = match side_points
                            .iter()
                            .position(|&p| geom::dist(p, node.point) <= 1e-6 * deflection)
                        {
                            Some(id) => id,
                            None => {
                                side_points.push(node.point);
                                side_points.len() - 1
                            }
                        };
                        node.point = side_points[id];
                        node.key = Some(Key::Side(id));
                    }
                    sampled.push(node);
                }
            } else if !iso && off > 4.0 * deflection {
                return Err(format!(
                    "a boundary side jumps across the surface in parameter space ({a:?} to {b:?})"
                ));
            }
        }
        *polygon = sampled;
    }

    let refine = !matches!(surface, Surface::Plane { .. });
    let mut mesher = Mesher::new(sheet, setup.scale, &polygons, deflection)?;
    let converged = if refine {
        mesher.refine(deflection, 2.0 * angular)
    } else {
        true
    };
    let mut mesh = mesher.finish(f.reversed, deflection);
    mesh.converged = converged;
    Ok(mesh)
}

/// The face's singular parameter lines (a sphere's poles, a cone's apex): a point on one is the
/// pole, the part's vertex there if its edges have one.
struct Poles<'a> {
    sheet: Sheet<'a>,
    /// Per singular line found: its v, its key and its point.
    lines: Vec<(f64, Key, V3)>,
}

/// The surface's singular parameter lines (a sphere's poles, a cone's apex) and their points.
fn singular_points(sheet: &Sheet<'_>) -> Vec<(f64, V3)> {
    let lines = match *sheet.surface {
        Surface::Sphere { .. } => vec![-0.5 * std::f64::consts::PI, 0.5 * std::f64::consts::PI],
        Surface::Cone {
            radius, semi_angle, ..
        } => vec![-radius / semi_angle.sin()],
        _ => Vec::new(),
    };
    lines
        .into_iter()
        .map(|v| (v, sheet.value(0.0, v)))
        .collect()
}

impl<'a> Poles<'a> {
    /// The face's poles, each the part's vertex there, or else an edge's point there (see
    /// [`PartMesher::make_plan`]), if its edges have one.
    fn new(
        part: &Part,
        face: usize,
        sheet: &Sheet<'a>,
        plans: &BTreeMap<usize, &Plan>,
        alias: &[usize],
        deflection: f64,
    ) -> Self {
        let mut lines = Vec::new();
        for (i, (v, pole)) in singular_points(sheet).into_iter().enumerate() {
            let mut key = Key::Pole(i as u8, None);
            let mut point = pole;
            let near = |p: V3| geom::dist(p, pole) <= 1e-3 * deflection;
            // A vertex the face's boundary routes along the pole's line (on a sphere, anything
            // within a degree of the pole, see `Surface::singular_v`) is the pole: the nearest.
            let window = match *sheet.surface {
                Surface::Sphere { radius, .. } => 0.021 * radius,
                _ => 0.0,
            }
            .max(1e-3 * deflection);
            let mut best = window;
            for e in part.face_edges(face) {
                let edge = &part.edges[e];
                for (end, vertex) in [(edge.start, edge.vertices.0), (edge.end, edge.vertices.1)] {
                    let d = geom::dist(end, pole);
                    if d <= best && !(d == best && key != Key::Pole(i as u8, None)) {
                        best = d;
                        key = Key::Pole(i as u8, Some(BoundaryPoint::Vertex(alias[vertex])));
                        point = end;
                    }
                }
            }
            if key == Key::Pole(i as u8, None) {
                'plans: for (&e, plan) in plans {
                    for (index, &p) in plan.points.iter().enumerate() {
                        if near(p) && index > 0 && index + 1 < plan.points.len() {
                            key = Key::Pole(i as u8, Some(BoundaryPoint::Edge { edge: e, index }));
                            point = p;
                            break 'plans;
                        }
                    }
                }
            }
            lines.push((v, key, point));
        }
        Poles {
            sheet: *sheet,
            lines,
        }
    }

    /// A polygon point at `uv` that is not on an edge: the pole if it lies on a singular line.
    fn node(&self, uv: (f64, f64)) -> BNode {
        for &(v, key, point) in &self.lines {
            if (uv.1 - v).abs() <= 1e-6 * (1.0 + v.abs()) {
                return BNode {
                    uv,
                    point,
                    key: Some(key),
                };
            }
        }
        BNode {
            uv,
            point: self.sheet.value(uv.0, uv.1),
            key: None,
        }
    }
}

/// Point `index` of edge `e`'s plan as a mesh vertex across the part.
fn boundary_point(
    part: &Part,
    alias: &[usize],
    e: usize,
    index: usize,
    plan: &Plan,
) -> BoundaryPoint {
    let edge = &part.edges[e];
    if index == 0 {
        BoundaryPoint::Vertex(alias[edge.vertices.0])
    } else if index + 1 == plan.points.len() {
        BoundaryPoint::Vertex(alias[edge.vertices.1])
    } else {
        BoundaryPoint::Edge { edge: e, index }
    }
}

/// One loop's polygon points: each edge sample its edge's plan keeps, with the plan's points
/// between samples, at their shared positions; points routed along a singular line are the
/// pole. The closing copy of the first point is left out.
#[allow(clippy::too_many_arguments)]
fn loop_nodes(
    part: &Part,
    face: usize,
    li: usize,
    sources: &[Option<(usize, usize)>],
    raw: &[(f64, f64)],
    points: &[(f64, f64)],
    plans: &BTreeMap<usize, &Plan>,
    alias: &[usize],
    poles: &Poles<'_>,
) -> Vec<BNode> {
    let lp = &part.faces[face].loops[li];
    let shared = |e: usize, index: usize, plan: &Plan| boundary_point(part, alias, e, index, plan);
    let mut out: Vec<BNode> = Vec::with_capacity(points.len());
    // The previous point's edge sample: (edge, sample, its parameters).
    let mut previous: Option<(usize, usize, (f64, f64))> = None;
    for (k, &uv) in points.iter().enumerate() {
        let Some(Some((place, sample))) = sources.get(k).copied() else {
            previous = None;
            // The loop closed explicitly on its first point (after a singular run).
            if k > 0 && k + 1 == points.len() && raw[k] == raw[0] && !out.is_empty() {
                out.push(BNode { uv, ..out[0] });
                continue;
            }
            out.push(poles.node(uv));
            continue;
        };
        let e = lp.edges[place].0;
        let plan = plans[&e];
        if let Some((pe, ps, puv)) = previous
            && pe == e
            && ps.abs_diff(sample) == 1
        {
            // A point on the chord between two samples sits at its place along their parameter
            // chord (inverting it instead could land anywhere on a collapsed side or a seam).
            let segment = ps.min(sample);
            let (from, to) = if sample > ps { (puv, uv) } else { (uv, puv) };
            let mut inside = plan.between[segment].clone();
            if sample < ps {
                inside.reverse();
            }
            for (index, t) in inside {
                out.push(BNode {
                    uv: (from.0 + t * (to.0 - from.0), from.1 + t * (to.1 - from.1)),
                    point: plan.points[index],
                    key: Some(Key::Shared(shared(e, index, plan))),
                });
            }
        }
        if let Some(index) = plan.at_sample[sample] {
            out.push(BNode {
                uv,
                point: plan.points[index],
                key: Some(Key::Shared(shared(e, index, plan))),
            });
        }
        previous = Some((e, sample, uv));
    }
    out
}

/// A boundary polyline's place on a surface, for [`thin`]: each point's parameters and normal
/// (`None` where it has none, on a singular line).
struct OnSurface<'a> {
    sheet: Sheet<'a>,
    uv: &'a [Option<(f64, f64)>],
    normals: &'a [Option<V3>],
}

/// A polyline thinned to the chordal `deflection` and, if given, the angular deflection: the
/// indices of the points kept, always the first and last and every point `keep` marks, never
/// one `skip` marks (unless it is first or last; a chord over such points stands for them even
/// if they stray). A chord
/// stands for the points it skips while none lies farther than `deflection` from it and no
/// skipped segment turns more than half the angular deflection from it (so the polyline turns by
/// at most the angular deflection at each kept point).
///
/// On each surface the polyline bounds (`on`), the chord is a straight line in parameter space,
/// so it must also stand for the points there: the surface point at each skipped point's place
/// along the parameter chord lies within `deflection` of it (a straight line in parameters is a
/// curve on most surfaces). And a chord longer than the deflection stands only for points whose
/// normals are within the angular deflection of its ends' normals, so the triangles along the
/// boundary meet the angular deflection too.
fn thin(
    points: &[V3],
    keep: &[bool],
    skip: &[bool],
    on: &[OnSurface<'_>],
    deflection: f64,
    angular: Option<f64>,
) -> Vec<usize> {
    let n = points.len();
    if n <= 2 {
        return (0..n).collect();
    }
    let mut kept = vec![false; n];
    kept[0] = true;
    kept[n - 1] = true;
    for (k, &forced) in keep.iter().enumerate().take(n) {
        kept[k] |= forced;
    }
    // Whether the chord from point i to point j stands for the points between.
    let stands = |i: usize, j: usize| -> bool {
        let (a, b) = (points[i], points[j]);
        let chord = geom::unit(geom::sub(b, a));
        for k in i + 1..j {
            let mut off = segment_distance(points[k], a, b);
            for on in on {
                let (Some(ua), Some(ub), Some(uk)) = (on.uv[i], on.uv[j], on.uv[k]) else {
                    continue;
                };
                let d = (ub.0 - ua.0, ub.1 - ua.1);
                let len2 = d.0 * d.0 + d.1 * d.1;
                let t = if len2 > 0.0 {
                    (((uk.0 - ua.0) * d.0 + (uk.1 - ua.1) * d.1) / len2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let image = on.sheet.value(ua.0 + t * d.0, ua.1 + t * d.1);
                off = off.max(geom::dist(image, points[k]));
            }
            if off > deflection {
                return false;
            }
            if let (Some(angular), Some(chord)) = (angular, chord) {
                for (p, q) in [(points[k - 1], points[k]), (points[k], points[k + 1])] {
                    // A sub-segment far shorter than the deflection has no direction to speak
                    // of (sample noise at an edge's ends).
                    if geom::dist(p, q) < 1e-6 * deflection {
                        continue;
                    }
                    if geom::unit(geom::sub(q, p)).is_some_and(|d| angle(d, chord) > 0.5 * angular)
                    {
                        return false;
                    }
                }
            }
            if let Some(angular) = angular
                && geom::dist(a, b) > deflection
            {
                for on in on {
                    if let (Some(nk), Some(ni), Some(nj)) =
                        (on.normals[k], on.normals[i], on.normals[j])
                        && angle(ni, nk).max(angle(nk, nj)) > angular
                    {
                        return false;
                    }
                }
            }
        }
        true
    };
    // Greedily, each chord as long as it can be (found by doubling, then halving the step):
    // halving a failed chord instead could keep up to twice the points needed.
    // The chord ends are positions in the candidates between two kept points.
    let breaks: Vec<usize> = (0..n).filter(|&k| kept[k]).collect();
    for w in breaks.windows(2) {
        let candidates: Vec<usize> = (w[0]..=w[1])
            .filter(|&k| k == w[0] || k == w[1] || !skip.get(k).copied().unwrap_or(false))
            .collect();
        let at = |p: usize| candidates[p];
        let (mut i, end) = (0, candidates.len() - 1);
        while i < end {
            let (mut good, mut step) = (i + 1, 1);
            while good < end && stands(at(i), at((i + 2 * step).min(end))) {
                good = (i + 2 * step).min(end);
                step *= 2;
            }
            let mut bad = (i + 2 * step).min(end);
            if good < end {
                // stands(i, good) and not stands(i, bad): narrow the gap.
                while bad > good + 1 {
                    let mid = (good + bad) / 2;
                    if stands(at(i), at(mid)) {
                        good = mid;
                    } else {
                        bad = mid;
                    }
                }
            }
            kept[at(good)] = true;
            i = good;
        }
    }
    (0..n).filter(|&k| kept[k]).collect()
}

/// Whether three triangulation vertices lie on one line in parameter space, to rounding (a
/// point inserted on a straight boundary side that the triangulation took as just beside it):
/// such a triangle covers no part of the face, and its plane in space is noise.
fn parameter_sliver(a: &Node, b: &Node, c: &Node) -> bool {
    let (pa, pb, pc) = (a.position, b.position, c.position);
    let cross = (pb.x - pa.x) * (pc.y - pa.y) - (pb.y - pa.y) * (pc.x - pa.x);
    let longest = [(pa, pb), (pb, pc), (pc, pa)]
        .iter()
        .map(|(p, q)| (q.x - p.x).hypot(q.y - p.y))
        .fold(0.0, f64::max);
    cross.abs() <= 1e-6 * longest * longest
}

/// The angle between two unit vectors.
fn angle(a: V3, b: V3) -> f64 {
    geom::dot(a, b).clamp(-1.0, 1.0).acos()
}

/// The distance from `p` to the segment `a`–`b`.
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

/// The distance from `p` to the triangle `a b c` (to the plane where `p` projects inside it,
/// else to the nearest side): how far the surface strays from the triangle near its centroid.
/// Not the plane's distance alone, which a sliver's unsteady plane would inflate.
fn triangle_distance(p: V3, a: V3, b: V3, c: V3) -> f64 {
    let sides = || {
        segment_distance(p, a, b)
            .min(segment_distance(p, b, c))
            .min(segment_distance(p, c, a))
    };
    let Some(n) = geom::unit(geom::cross(geom::sub(b, a), geom::sub(c, a))) else {
        return sides();
    };
    let height = geom::dot(geom::sub(p, a), n);
    let q = geom::sub(p, geom::scale(n, height));
    let inside = [(a, b), (b, c), (c, a)]
        .iter()
        .all(|&(x, y)| geom::dot(geom::cross(geom::sub(y, x), geom::sub(q, x)), n) >= 0.0);
    if inside { height.abs() } else { sides() }
}

/// Consecutive points that are one (the same edge point, or the same parameters) are kept once,
/// round the closed polygon too.
fn dedup_consecutive(polygon: &mut Vec<BNode>) {
    let same = |a: &BNode, b: &BNode| {
        a.uv == b.uv || (matches!(a.key, Some(Key::Shared(_))) && a.key == b.key)
    };
    polygon.dedup_by(|b, a| same(a, b));
    while polygon.len() > 1 && same(&polygon[0], &polygon[polygon.len() - 1]) {
        polygon.pop();
    }
}

/// Twice the signed area of a closed polygon.
fn signed_area(polygon: &[BNode]) -> f64 {
    let n = polygon.len();
    (0..n)
        .map(|i| {
            let (a, b) = (polygon[i].uv, polygon[(i + 1) % n].uv);
            a.0 * b.1 - b.0 * a.1
        })
        .sum()
}

/// Whether `p` lies inside the closed polygon, by crossing parity.
fn inside(polygon: &[BNode], p: (f64, f64)) -> bool {
    let n = polygon.len();
    let mut odd = false;
    for i in 0..n {
        let (a, b) = (polygon[i].uv, polygon[(i + 1) % n].uv);
        if (a.1 > p.1) != (b.1 > p.1) {
            let x = a.0 + (p.1 - a.1) / (b.1 - a.1) * (b.0 - a.0);
            if x > p.0 {
                odd = !odd;
            }
        }
    }
    odd
}

/// Closed loops only: the largest is the outer boundary, the rest holes, each moved by whole
/// periods so it lies in the outer boundary.
fn outer_and_holes(
    mut closed: Vec<Vec<BNode>>,
    period: [Option<f64>; 2],
) -> Result<Vec<Vec<BNode>>, String> {
    if closed.is_empty() {
        return Err("it has no boundary loop".into());
    }
    // The largest by area in parameter space; the first of equals.
    let mut outer = 0;
    for (i, lp) in closed.iter().enumerate() {
        if signed_area(lp).abs() > signed_area(&closed[outer]).abs() {
            outer = i;
        }
    }
    let outer_loop = closed.remove(outer);
    let mut polygons = vec![outer_loop];
    for hole in closed {
        polygons.push(place_hole(&polygons[0], hole, period)?);
    }
    Ok(polygons)
}

/// `hole` moved by whole periods (in the periodic directions) until its first point lies in
/// `region`.
fn place_hole(
    region: &[BNode],
    mut hole: Vec<BNode>,
    period: [Option<f64>; 2],
) -> Result<Vec<BNode>, String> {
    let turns = |p: Option<f64>| if p.is_some() { -2..=2 } else { 0..=0 };
    let start = hole[0].uv;
    let mut found = None;
    let (pu, pv) = (period[0].unwrap_or(0.0), period[1].unwrap_or(0.0));
    'search: for ku in turns(period[0]) {
        for kv in turns(period[1]) {
            let shift = (ku as f64 * pu, kv as f64 * pv);
            if inside(region, (start.0 + shift.0, start.1 + shift.1)) {
                found = Some(shift);
                break 'search;
            }
        }
    }
    let (su, sv) = found.ok_or("an inner loop lies outside the face's outer loop")?;
    for p in &mut hole {
        p.uv.0 += su;
        p.uv.1 += sv;
    }
    Ok(hole)
}

/// A face bounded by two loops that each run once round a periodic direction (u, or v when
/// `swapped`): cut along a straight parameter line from a point of one loop to a point of the
/// other, one that misses every hole, giving one polygon (the band between the loops, both
/// sides of the cut the same points) plus its holes.
fn strip(
    sheet: &Sheet<'_>,
    scale: (f64, f64),
    winding: Vec<Vec<BNode>>,
    holes: Vec<Vec<BNode>>,
    deflection: f64,
    angular: f64,
    swapped: bool,
) -> Result<Vec<Vec<BNode>>, String> {
    let (axis, other) = if swapped { (1, 0) } else { (0, 1) };
    let turn = sheet.period[axis].ok_or("a loop runs round a direction that is not periodic")?;
    // Work as if the winding direction were u.
    let swap = |p: (f64, f64)| if swapped { (p.1, p.0) } else { p };
    let swap_nodes = |lp: Vec<BNode>| -> Vec<BNode> {
        lp.into_iter()
            .map(|n| BNode {
                uv: swap(n.uv),
                ..n
            })
            .collect()
    };
    let mut loops: Vec<Vec<BNode>> = Vec::with_capacity(2);
    for lp in winding {
        let mut lp = swap_nodes(lp);
        if lp[lp.len() - 1].uv.0 < lp[0].uv.0 {
            lp.reverse();
        }
        // Once round and back to its start: the last point is the first a turn on.
        let (first, last) = (lp[0], lp[lp.len() - 1]);
        if !(first.key.is_some() && first.key == last.key) {
            return Err("a loop running round the surface does not return to its start".into());
        }
        lp.pop();
        dedup_consecutive(&mut lp);
        if lp.len() < 2 {
            return Err("a loop running round the surface collapses".into());
        }
        loops.push(lp);
    }
    let holes: Vec<Vec<BNode>> = holes.into_iter().map(swap_nodes).collect();
    let span = |lp: &[BNode]| {
        lp.iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                (lo.min(p.uv.0), hi.max(p.uv.0))
            })
    };
    // The point of a loop nearest the line u = c, by whole turns.
    let nearest_point = |lp: &[BNode], c: f64| {
        let mut best = (f64::INFINITY, 0, 0.0);
        for (k, p) in lp.iter().enumerate() {
            let shift = turn * ((c - p.uv.0) / turn).round();
            let d = (p.uv.0 + shift - c).abs();
            if d < best.0 {
                best = (d, k, shift);
            }
        }
        (best.1, best.2)
    };
    // Candidate cuts, spread over the turn at an angle no modelled symmetry is likely to share.
    let margin = 1e-6 * turn;
    let cut = (0..37)
        .map(|j| 0.123_456_789 * turn / TAU + turn * j as f64 / 37.0)
        .find_map(|c| {
            let ends = [nearest_point(&loops[0], c), nearest_point(&loops[1], c)];
            let us = [0, 1].map(|i| loops[i][ends[i].0].uv.0 + ends[i].1);
            let (lo, hi) = (us[0].min(us[1]), us[0].max(us[1]));
            if hi - lo > 0.25 * turn {
                return None;
            }
            // Each loop crosses the cut's span once (no fold back across it).
            let once = loops.iter().all(|lp| {
                let n = lp.len();
                let mut count = 0;
                for i in 0..n {
                    let a = lp[i].uv.0;
                    let b = if i + 1 < n {
                        lp[i + 1].uv.0
                    } else {
                        lp[0].uv.0 + turn
                    };
                    for line in [lo - margin, hi + margin] {
                        count += turn_index(a, line, turn).abs_diff(turn_index(b, line, turn));
                    }
                }
                count == 2
            });
            let clear = holes.iter().all(|h| {
                let (hlo, hhi) = span(h);
                // The hole moved to start just after the cut must end before its next turn.
                let k = ((hi + margin - hlo) / turn).floor() + 1.0;
                hhi + k * turn < lo + turn - margin
            });
            (once && clear).then_some(ends)
        })
        .ok_or("no parameter line crosses each of its periodic loops once and misses its holes")?;

    // Each loop from its cut point once round, the end a turn on from the start.
    let rotated: Vec<Vec<BNode>> = [0, 1]
        .map(|i| {
            let (lp, (k, shift)) = (&loops[i], cut[i]);
            let n = lp.len();
            (0..=n)
                .map(|m| {
                    let p = lp[(k + m) % n];
                    let wraps = if k + m >= n { turn } else { 0.0 };
                    BNode {
                        uv: (p.uv.0 + shift + wraps, p.uv.1),
                        ..p
                    }
                })
                .collect()
        })
        .into_iter()
        .collect();
    let (a, b) = (&rotated[0], &rotated[1]);
    // The cut, from a's start to b's start, and the same a turn on. Both sides are one curve
    // in space: each point is one vertex.
    let line: Vec<BNode> = straight_side(
        sheet,
        Some(scale),
        swap(a[0].uv),
        swap(b[0].uv),
        deflection,
        angular,
    )
    .into_iter()
    .enumerate()
    .map(|(k, uv)| {
        let uv = swap(uv);
        let point = sheet.value(swap(uv).0, swap(uv).1);
        BNode {
            uv,
            point,
            key: Some(Key::Cut(k)),
        }
    })
    .collect();
    let on = |n: &BNode| BNode {
        uv: (n.uv.0 + turn, n.uv.1),
        ..*n
    };
    let mut band: Vec<BNode> = a.clone();
    // a ends a turn on from its start: along the cut to b's end, then back along b.
    band.extend(line.iter().map(on));
    band.extend(b.iter().rev());
    // b starts on the cut: back along it to a's start.
    band.extend(line.iter().rev());

    let mut polygons = vec![band];
    for hole in holes {
        // Into the band by whole turns round, then by whole periods across if that is periodic
        // too.
        let (lo, _) = span(&hole);
        let k = ((a[0].uv.0 - lo) / turn).ceil();
        let shifted: Vec<BNode> = hole
            .into_iter()
            .map(|n| BNode {
                uv: (n.uv.0 + k * turn, n.uv.1),
                ..n
            })
            .collect();
        polygons.push(place_hole(
            &polygons[0],
            shifted,
            [None, sheet.period[other]],
        )?);
    }
    Ok(polygons
        .into_iter()
        .map(|p| {
            p.into_iter()
                .map(|n| BNode {
                    uv: swap(n.uv),
                    ..n
                })
                .collect()
        })
        .collect())
}

/// The points strictly between `a` and `b` along the straight parameter line joining them (a
/// cut, or a pole's closing line), sampled and thinned on the surface like a boundary edge and,
/// given the triangulation's `scale`, divided no coarser than its spacing.
fn straight_side(
    sheet: &Sheet<'_>,
    scale: Option<(f64, f64)>,
    a: (f64, f64),
    b: (f64, f64),
    deflection: f64,
    angular: f64,
) -> Vec<(f64, f64)> {
    let uv: Vec<Option<(f64, f64)>> = (0..=CUT_SAMPLES)
        .map(|i| {
            let t = i as f64 / CUT_SAMPLES as f64;
            Some((a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1)))
        })
        .collect();
    let space: Vec<V3> = uv
        .iter()
        .flatten()
        .map(|&(u, v)| sheet.value(u, v))
        .collect();
    let normals: Vec<Option<V3>> = uv
        .iter()
        .flatten()
        .map(|&(u, v)| sheet.normal(u, v))
        .collect();
    let on = [OnSurface {
        sheet: *sheet,
        uv: &uv,
        normals: &normals,
    }];
    let mut kept = thin(&space, &[], &[], &on, deflection, Some(angular));
    if let Some((su, sv)) = scale
        && !matches!(sheet.surface, Surface::Plane { .. })
    {
        let length = ((b.0 - a.0) * su).hypot((b.1 - a.1) * sv);
        let pieces = (length.round() as usize).clamp(1, CUT_SAMPLES);
        kept.extend((1..pieces).map(|m| (m * CUT_SAMPLES + pieces / 2) / pieces));
        kept.sort_unstable();
        kept.dedup();
    }
    kept[1..kept.len() - 1]
        .iter()
        // Every sample of the line has parameters (built above).
        .filter_map(|&k| uv[k])
        .collect()
}

/// Which turn `u` is on, counting turns of length `turn` from `cut`: `u` lies in
/// `[cut + k turn, cut + (k + 1) turn)`.
fn turn_index(u: f64, cut: f64, turn: f64) -> i64 {
    ((u - cut) / turn).floor() as i64
}

/// Per-parameter scale factors for the triangulation's coordinates: the reciprocal of the
/// spacing each parameter needs to meet the deflections, so a needed triangle is about one unit
/// across either way and the Delaunay triangulation lays its edges along the directions the
/// surface does not bend in (a cylinder's rulings) instead of across them.
///
/// The spacing along a parameter is the least of what the angular deflection allows (the
/// normal's turning rate), what the chordal deflection allows (the second derivative), and the
/// face's extent in it; rates are root-mean-square over a grid on the box of `points` (the
/// face's boundary), measured by finite differences. A plane is left unscaled.
fn parameter_scale(
    sheet: &Sheet<'_>,
    points: &[(f64, f64)],
    deflection: f64,
    angular: f64,
) -> Result<(f64, f64), String> {
    if matches!(sheet.surface, Surface::Plane { .. }) {
        return Ok((1.0, 1.0));
    }
    let (mut u0, mut u1, mut v0, mut v1) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for &(u, v) in points {
        (u0, u1, v0, v1) = (u0.min(u), u1.max(u), v0.min(v), v1.max(v));
    }
    let (eu, ev) = (u1 - u0, v1 - v0);
    if !(eu > 0.0 && ev > 0.0 && eu.is_finite() && ev.is_finite()) {
        return Err("its boundary spans no area in parameter space".into());
    }
    // Squared speed, turning rate of the normal and second derivative, per parameter.
    let n = 5;
    let (mut speed, mut turn, mut bend) = ([0.0_f64; 2], [0.0_f64; 2], [0.0_f64; 2]);
    let mut samples = 0;
    for i in 0..n {
        for j in 0..n {
            let u = u0 + eu * (i as f64 + 0.5) / n as f64;
            let v = v0 + ev * (j as f64 + 0.5) / n as f64;
            let centre = sheet.value(u, v);
            if sheet.normal(u, v).is_none() {
                continue;
            }
            samples += 1;
            for (axis, h) in [(0, 1e-4 * eu), (1, 1e-4 * ev)] {
                let at = |t: f64| if axis == 0 { (u + t, v) } else { (u, v + t) };
                let (a, b) = (at(-h), at(h));
                let (pa, pb) = (sheet.value(a.0, a.1), sheet.value(b.0, b.1));
                let first = geom::dist(pa, pb) / (2.0 * h);
                speed[axis] += first * first;
                let second = geom::scale(
                    geom::sub(geom::add(pa, pb), geom::scale(centre, 2.0)),
                    1.0 / (h * h),
                );
                bend[axis] += geom::dot(second, second);
                if let (Some(na), Some(nb)) = (sheet.normal(a.0, a.1), sheet.normal(b.0, b.1)) {
                    let rate = geom::norm(geom::sub(nb, na)) / (2.0 * h);
                    turn[axis] += rate * rate;
                }
            }
        }
    }
    if samples == 0 {
        return Err("its surface has no normal over the face".into());
    }
    let rms = |x: [f64; 2], axis: usize| (x[axis] / samples as f64).sqrt();
    let spacing = |axis: usize, extent: f64| {
        let (turn, bend) = (rms(turn, axis), rms(bend, axis));
        let mut h = extent;
        if turn > 0.0 {
            h = h.min(angular / turn);
        }
        if bend > 0.0 {
            h = h.min((8.0 * deflection / bend).sqrt());
        }
        h
    };
    let (mut hu, mut hv) = (spacing(0, eu), spacing(1, ev));
    // A triangle long along a curved direction and short across it stands askew of the surface
    // (its long side's sag tilts it by about sag / height): the long side is held to a sag of
    // half the angular deflection times the short side.
    let (su, sv) = (rms(speed, 0), rms(speed, 1));
    if su > 0.0 && sv > 0.0 {
        let (lu, lv) = (hu * su, hv * sv);
        let (ku, kv) = (rms(turn, 0) / su, rms(turn, 1) / sv);
        if lu > lv && ku > 0.0 {
            hu = hu.min((4.0 * angular * lv / ku).sqrt() / su);
        } else if lv > lu && kv > 0.0 {
            hv = hv.min((4.0 * angular * lu / kv).sqrt() / sv);
        }
    }
    if hu > 0.0 && hv > 0.0 && hu.is_finite() && hv.is_finite() {
        Ok((1.0 / hu, 1.0 / hv))
    } else {
        Err("its surface has no usable parameter speed over the face".into())
    }
}

/// A triangulation vertex: its scaled position, parameters, point and normal on the surface,
/// and its identity if it is a boundary point.
#[derive(Clone, Copy, Debug)]
struct Node {
    position: Point2<f64>,
    uv: (f64, f64),
    point: V3,
    normal: Option<V3>,
    key: Option<Key>,
}

impl HasPosition for Node {
    type Scalar = f64;
    fn position(&self) -> Point2<f64> {
        self.position
    }
}

struct Mesher<'a> {
    surface: Sheet<'a>,
    scale: (f64, f64),
    cdt: ConstrainedDelaunayTriangulation<Node>,
}

impl<'a> Mesher<'a> {
    /// The constrained triangulation of the polygons' points and sides; `Err` when a side
    /// crosses another (the boundary crosses itself in parameter space) or two boundary points
    /// fall on one place in parameter space.
    fn new(
        surface: Sheet<'a>,
        scale: (f64, f64),
        polygons: &[Vec<BNode>],
        deflection: f64,
    ) -> Result<Self, String> {
        let mut mesher = Mesher {
            surface,
            scale,
            cdt: ConstrainedDelaunayTriangulation::new(),
        };
        for polygon in polygons {
            let mut handles = Vec::with_capacity(polygon.len());
            for b in polygon {
                let node = Node {
                    point: b.point,
                    key: b.key,
                    ..mesher.node(b.uv)
                };
                if let Some(existing) = mesher.cdt.locate_vertex(node.position) {
                    let data = existing.data();
                    // Two loops touching at a point (each with its own vertex there) meet at
                    // one triangulation vertex; any other two points in one place cross.
                    let touching = data.key.is_some()
                        && node.key.is_some()
                        && geom::dist(data.point, node.point) <= 1e-6 * deflection;
                    if data.key != node.key && !touching {
                        return Err(CROSSES.into());
                    }
                    handles.push(existing.fix());
                    continue;
                }
                let handle = mesher
                    .cdt
                    .insert(node)
                    .map_err(|e| format!("a boundary point cannot be triangulated ({e:?})"))?;
                handles.push(handle);
            }
            for i in 0..handles.len() {
                let (a, b) = (handles[i], handles[(i + 1) % handles.len()]);
                if a == b {
                    continue;
                }
                if !mesher.cdt.can_add_constraint(a, b) {
                    return Err(CROSSES.into());
                }
                mesher.cdt.add_constraint(a, b);
            }
        }
        Ok(mesher)
    }

    fn node(&self, uv: (f64, f64)) -> Node {
        // spade refuses coordinates nearer zero than about 1e-38 (but not zero itself).
        let flush = |x: f64| if x.abs() < 1e-30 { 0.0 } else { x };
        Node {
            position: Point2::new(flush(uv.0 * self.scale.0), flush(uv.1 * self.scale.1)),
            uv,
            point: self.surface.value(uv.0, uv.1),
            normal: self.surface.normal(uv.0, uv.1),
            key: None,
        }
    }

    /// Whether each face (by index) lies inside the polygons: flooded from the convex hull,
    /// crossing a constraint (a polygon side) toggles inside and outside.
    fn inside_faces(&self) -> Vec<bool> {
        let mut state: Vec<Option<bool>> = vec![None; self.cdt.num_all_faces()];
        let mut queue = VecDeque::new();
        for edge in self.cdt.convex_hull() {
            for side in [edge, edge.rev()] {
                if let Some(face) = side.face().as_inner() {
                    let index = face.fix().index();
                    if state[index].is_none() {
                        state[index] = Some(edge.is_constraint_edge());
                        queue.push_back(face.fix());
                    }
                }
            }
        }
        while let Some(fixed) = queue.pop_front() {
            let face = self.cdt.face(fixed);
            // A face is queued only once its state is set.
            let Some(here) = state[fixed.index()] else {
                continue;
            };
            for edge in face.adjacent_edges() {
                if let Some(next) = edge.rev().face().as_inner() {
                    let index = next.fix().index();
                    if state[index].is_none() {
                        state[index] = Some(here != edge.is_constraint_edge());
                        queue.push_back(next.fix());
                    }
                }
            }
        }
        state.into_iter().map(|s| s == Some(true)).collect()
    }

    /// Inserts edge midpoints and centroids until every inside triangle meets the deflections,
    /// or a cap stops it. Returns whether it converged.
    ///
    /// An edge is split at its parameter midpoint when the surface there strays from it by more
    /// than the chordal deflection, or when the normals at its ends differ by more than the
    /// angular deflection (only for an edge longer than the chordal deflection: near a singular
    /// point the normal turns fast however short the edge). A triangle none of whose edges is
    /// split is refined when the surface near its centroid strays from it by more than the
    /// chordal deflection, or when its own normal leans from the surface's at a corner by more
    /// than the angular deflection (a thin triangle across a curve can meet the chordal test
    /// and still stand askew, overstating the area: the Schwarz lantern); it is split along its
    /// longest inner edge, or at its centroid when every edge is the boundary. Boundary edges
    /// are never split.
    fn refine(&mut self, deflection: f64, angular: f64) -> bool {
        for _ in 0..MAX_ROUNDS {
            let inside = self.inside_faces();
            let mut seen = vec![false; self.cdt.num_undirected_edges()];
            let mut wanted: Vec<(f64, f64)> = Vec::new();
            let mut want_edge = |index: usize, a: &Node, b: &Node, wanted: &mut Vec<(f64, f64)>| {
                if !seen[index] {
                    seen[index] = true;
                    wanted.push(((a.uv.0 + b.uv.0) / 2.0, (a.uv.1 + b.uv.1) / 2.0));
                }
            };
            for face in self.cdt.inner_faces() {
                if !inside[face.fix().index()] {
                    continue;
                }
                let mut split = false;
                // The longest inner edge, for a triangle refined as a whole.
                let mut longest: Option<(f64, usize, Node, Node)> = None;
                for edge in face.adjacent_edges() {
                    let undirected = edge.as_undirected();
                    if undirected.is_constraint_edge() {
                        continue;
                    }
                    let index = undirected.fix().index();
                    let [a, b] = edge.vertices().map(|v| *v.data());
                    let length = geom::dist(a.point, b.point);
                    if longest.is_none_or(|l| length > l.0) {
                        longest = Some((length, index, a, b));
                    }
                    let mid = ((a.uv.0 + b.uv.0) / 2.0, (a.uv.1 + b.uv.1) / 2.0);
                    let bent = match (a.normal, b.normal) {
                        (Some(na), Some(nb)) => angle(na, nb) > angular && length > deflection,
                        _ => false,
                    };
                    // The surface's distance from the edge (not from its midpoint: where the
                    // parameterisation is uneven the parameter midpoint lies well along it).
                    let off = segment_distance(self.surface.value(mid.0, mid.1), a.point, b.point);
                    if bent || off > deflection {
                        split = true;
                        want_edge(index, &a, &b, &mut wanted);
                    }
                }
                if split {
                    continue;
                }
                let [a, b, c] = face.vertices().map(|v| *v.data());
                if parameter_sliver(&a, &b, &c) {
                    continue;
                }
                let uv = (
                    (a.uv.0 + b.uv.0 + c.uv.0) / 3.0,
                    (a.uv.1 + b.uv.1 + c.uv.1) / 3.0,
                );
                let off =
                    triangle_distance(self.surface.value(uv.0, uv.1), a.point, b.point, c.point);
                let size = geom::dist(a.point, b.point)
                    .max(geom::dist(b.point, c.point))
                    .max(geom::dist(c.point, a.point));
                // A sliver (its height a thousandth of its length, three points nearly on one
                // line) has no steady plane to lean: it is not judged by its lean (one folded
                // against the surface is left out of the mesh, see `finish`).
                let doubled = geom::cross(geom::sub(b.point, a.point), geom::sub(c.point, a.point));
                let askew = size > 1e-3 * deflection
                    && geom::norm(doubled) > 1e-3 * size * size
                    && geom::unit(doubled).is_some_and(|n| {
                        [a.normal, b.normal, c.normal]
                            .into_iter()
                            .flatten()
                            .any(|m| angle(n, m) > angular)
                    });
                if off > deflection || askew {
                    match longest {
                        Some((_, index, a, b)) => want_edge(index, &a, &b, &mut wanted),
                        None => wanted.push(uv),
                    }
                }
            }
            if wanted.is_empty() {
                return true;
            }
            for uv in wanted {
                if self.cdt.num_vertices() >= MAX_VERTICES {
                    return false;
                }
                // A point the triangulation already has is skipped; the round after judges the
                // result again.
                let node = self.node(uv);
                if self.cdt.locate_vertex(node.position).is_none() {
                    let _ = self.cdt.insert(node);
                }
            }
        }
        false
    }

    /// The inside triangles on the surface, wound outward, with degenerate ones (no area in
    /// space: two corners one point, as on a pole) left out.
    fn finish(self, reversed: bool, deflection: f64) -> FaceMesh {
        let inside = self.inside_faces();
        let mut index = vec![usize::MAX; self.cdt.num_vertices()];
        let mut by_key: BTreeMap<Key, usize> = BTreeMap::new();
        let mut points = Vec::new();
        let mut uv = Vec::new();
        let mut boundary = Vec::new();
        let mut triangles = Vec::new();
        // A sliver (a thousandth as high as it is long), or a speck (less than a millionth of a
        // deflection square), folded against the surface is left out and counted: next to a
        // degenerate side or corner, where a parameter barely moves the point, the triangle's
        // plane is set by rounding and curvature, not by the surface it stands for. A triangle
        // less than a millionth of the deflection high, or flat in parameter space (three
        // points on one boundary line, or a point inserted beside it), covers nothing to see
        // and its normal is noise, but it is kept: its sides are its neighbours' sides, and
        // leaving it out would open the mesh.
        let thin = 1e-6 * deflection;
        let mut folded_dropped = (0, 0.0);
        for face in self.cdt.inner_faces() {
            if !inside[face.fix().index()] {
                continue;
            }
            let corners = face.vertices();
            let [a, b, c] = corners.map(|v| v.data().point);
            let longest = geom::dist(a, b).max(geom::dist(b, c)).max(geom::dist(c, a));
            let [na, nb, nc] = corners.map(|v| *v.data());
            let doubled = geom::cross(geom::sub(b, a), geom::sub(c, a));
            let normals = [na.normal, nb.normal, nc.normal];
            let folded = normals.iter().flatten().count() > 0
                && normals
                    .iter()
                    .flatten()
                    .all(|&m| geom::dot(doubled, m) < 0.0);
            let sliver = geom::norm(doubled) <= 1e-3 * longest * longest;
            let speck = geom::norm(doubled) <= 2.0 * (1e-3 * deflection).powi(2);
            // A triangle too thin to see, or flat in parameter space, is kept whatever its
            // normal: leaving it out would open the mesh.
            let flat = parameter_sliver(&na, &nb, &nc);
            if geom::norm(doubled) > thin * longest
                && !flat
                && ((sliver && folded)
                    || (speck && {
                        // A speck at a degenerate corner can have one corner's normal anything: it
                        // is judged by the surface's normal at its middle.
                        let mid = (
                            (na.uv.0 + nb.uv.0 + nc.uv.0) / 3.0,
                            (na.uv.1 + nb.uv.1 + nc.uv.1) / 3.0,
                        );
                        self.surface
                            .normal(mid.0, mid.1)
                            .is_some_and(|m| geom::dot(doubled, m) < 0.0)
                    }))
            {
                folded_dropped.0 += 1;
                folded_dropped.1 += 0.5 * geom::norm(doubled);
                continue;
            }
            let mut tri = corners.map(|v| {
                let k = v.fix().index();
                if index[k] == usize::MAX {
                    let data = v.data();
                    let mut add = || {
                        points.push(data.point);
                        uv.push(data.uv);
                        boundary.push(match data.key {
                            Some(Key::Shared(p) | Key::Pole(_, Some(p))) => Some(p),
                            _ => None,
                        });
                        points.len() - 1
                    };
                    index[k] = match data.key {
                        // A pole at the part's vertex is that vertex.
                        Some(Key::Pole(_, Some(p))) => {
                            *by_key.entry(Key::Shared(p)).or_insert_with(add)
                        }
                        Some(key) => *by_key.entry(key).or_insert_with(add),
                        None => add(),
                    };
                }
                index[k]
            });
            if tri[0] == tri[1] || tri[1] == tri[2] || tri[2] == tri[0] {
                continue;
            }
            // Counter-clockwise in (u, v) is along Su × Sv; a reversed face's outside is not.
            if reversed {
                tri.swap(1, 2);
            }
            triangles.push(tri);
        }
        FaceMesh {
            points,
            uv,
            triangles,
            boundary,
            converged: true,
            folded_dropped,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thinning_keeps_the_ends_and_drops_collinear_points() {
        let points: Vec<V3> = (0..=10).map(|i| [i as f64, 0.0, 0.0]).collect();
        assert_eq!(thin(&points, &[], &[], &[], 0.01, Some(0.3)), vec![0, 10]);
    }

    #[test]
    fn thinning_a_circle_meets_both_deflections() {
        let r = 10.0;
        let points: Vec<V3> = (0..=1000)
            .map(|i| {
                let t = TAU * i as f64 / 1000.0;
                [r * t.cos(), r * t.sin(), 0.0]
            })
            .collect();
        // Coarse chordal deflection: the angular one (0.3 rad per segment) decides.
        let kept = thin(&points, &[], &[], &[], 1.0, Some(0.3));
        assert!(kept.len() > (TAU / 0.3).ceil() as usize, "{}", kept.len());
        // The samples' directions stand for the tangent, so a segment may span the angular
        // deflection plus one sample step, and no more; greedy chords use most of it.
        let step = TAU / 1000.0;
        for w in kept.windows(2) {
            let turn = step * (w[1] - w[0]) as f64;
            assert!(turn <= 0.3 + step + 1e-9, "a segment spans {turn} rad");
        }
        assert!(
            kept.len() <= (TAU / 0.3).ceil() as usize + 3,
            "{}",
            kept.len()
        );
        // Chordal alone.
        let kept = thin(&points, &[], &[], &[], 0.01, None);
        for w in kept.windows(2) {
            let turn = TAU * (w[1] - w[0]) as f64 / 1000.0;
            assert!(r * (1.0 - (turn / 2.0).cos()) <= 0.01 + 1e-9);
        }
    }

    #[test]
    fn a_forced_point_is_kept() {
        let points: Vec<V3> = (0..=10).map(|i| [i as f64, 0.0, 0.0]).collect();
        let mut keep = vec![false; 11];
        keep[4] = true;
        assert_eq!(thin(&points, &keep, &[], &[], 0.01, None), vec![0, 4, 10]);
    }
}
