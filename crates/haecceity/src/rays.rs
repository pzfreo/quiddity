//! Rays against a solid's faces — the stand-in for `IntCurvesFace_ShapeIntersector`: every
//! point where a ray meets a face within its trimming loops, with the face it meets. A hit on a
//! face's boundary counts as on the face (OpenCascade reports `ON` points too), so a ray through
//! an edge meets both faces there.
//!
//! Faces are found through a bounding-volume hierarchy of their boxes, so a ray costs about
//! the logarithm of the face count plus the faces it actually passes near.

use super::brep::{Edge, Part};
use super::geom::{self, Bounds, COORD_FLOOR, Surface, V3};
use super::sampling::{CHORD_TOLERANCE, edge_interval};

/// The largest cosine between a ray and a surface's normal at which a tangential root is a touch
/// rather than a crossing: a double root sits where the ray is square to the normal, to within
/// the discriminant's rounding.
const TOUCH_COS: f64 = 1e-6;

/// How many bands of a straying edge a crossing past the neighbouring surface may lie from the
/// edge and still be taken for the edge's overshoot ([`RayCaster::overshoots`]).
///
/// Empirical, not derived (verdict: undetermined). Over the corpus tests (classify, probes,
/// drawings, the recognisers' corpus run) the overshoots that matter are nist_ftc_10's cross
/// bores (face 175, edge 402), up to 1.64 bands from the edge; below 1.8 bands
/// `_recess_reduce` #5 there is refused. The next points past a neighbour lie 22.6 bands and
/// more from their edge (cgb207's faces 175 and 64), and taking those for overshoots breaks
/// parity at 70 bands (classify point cgb207 #286). Every value from 1.8 to 50 gives the same
/// test results; within it the drawings' visibility rays alone see a difference (two dozen
/// of their points on cgb217 and nist_ctc_05 lie 2 to 4 bands from an edge), which no view's
/// score shows. A derivation would follow how far along the surface an edge's stray carries the
/// boundary (the stray over the sine of the angle between the faces); none is made.
const OVERSHOOT_BANDS: f64 = 4.0;

/// One meeting of a ray with a face, `t` along the (unit) direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub t: f64,
    pub face: usize,
}

/// How a ray's crossing meets its face: inside its trim, within the polyline tolerance of an
/// edge (on the boundary, where a neighbour may claim the same crossing), or only in the band
/// of an edge that strays from the surface.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Contact {
    Interior,
    /// Within the tolerance of an edge; *inside* says whether the trim itself contains it.
    Edge {
        inside: bool,
    },
    Band(usize, f64),
}

/// A crossing with how it meets its face and whether the ray touches the surface there
/// tangentially (and so may not cross it at all).
struct Crossing {
    hit: Hit,
    contact: Contact,
    tangent: bool,
    /// The surface parameters of the crossing, where the intersection itself found them.
    uv: Option<(f64, f64)>,
}

enum Node {
    Leaf(Vec<usize>),
    Split(Box<Bounds>, Box<Node>, Box<Bounds>, Box<Node>),
}

/// Geometry-only edge queries shared by all ray casters borrowing the same part.
#[derive(Debug)]
pub(super) struct RayEdges {
    boxes: Vec<Bounds>,
    runs: Vec<Vec<Bounds>>,
    cracks: Vec<std::sync::OnceLock<f64>>,
}

pub struct RayCaster<'a> {
    pub(super) part: &'a Part,
    /// The faces rays meet.
    pub(super) faces: Vec<usize>,
    /// Per part face: a box that certainly contains it (freeform faces use their control points).
    pub(super) face_boxes: std::borrow::Cow<'a, [Bounds]>,
    pub(super) edge_boxes: &'a [Bounds],
    /// Per part edge: the box of each run of [`RUN`] polyline segments, padded by
    /// [`run_slack`], so that a search near an edge skips the runs out of reach
    /// ([`Self::polyline_within`]).
    edge_runs: &'a [Vec<Bounds>],
    root: Node,
    pub(super) root_box: Bounds,
    /// A hit this close to a face's boundary is on the boundary. It exceeds the edges' polyline
    /// error, so a hit at a shared edge is never missed by both faces.
    pub(super) edge_tol: f64,
    /// Per part edge, once asked: the furthest it strays from any face's surface it bounds
    /// ([`Self::crack`]).
    cracks: &'a [std::sync::OnceLock<f64>],
    /// Per part face: whether rays meet it (it is one of `faces`).
    member: Vec<bool>,
    /// Whether every face rays meet can be intersected, so that [`Self::hits`] never gives up.
    intersectable: bool,
}

impl<'a> RayCaster<'a> {
    /// Rays against the faces of one solid.
    pub fn for_solid(part: &'a Part, solid: usize) -> Self {
        Self::for_faces(part, part.solids[solid].faces.clone())
    }

    pub fn for_faces(part: &'a Part, faces: Vec<usize>) -> Self {
        let face_boxes = part.ray_face_boxes.get_or_init(|| {
            (0..part.faces.len())
                .map(|i| match &part.faces[i].surface {
                    // A freeform face's box is sampled, so padded to be sure of it: its control
                    // net's box is certain but can be far larger (a helix's spans its cylinder).
                    Surface::Freeform { surface, .. } => {
                        let b = part.face_bounds(i);
                        let pad = 0.02 * b.diagonal() + 1e-3;
                        let (min, max) = surface.control_bounds();
                        Bounds {
                            min: [0, 1, 2].map(|k| (b.min[k] - pad).max(min[k])),
                            max: [0, 1, 2].map(|k| (b.max[k] + pad).min(max[k])),
                        }
                    }
                    _ => part.face_bounds(i),
                })
                .collect()
        });
        Self::with_geometry(part, faces, std::borrow::Cow::Borrowed(face_boxes))
    }

    /// Rays against *faces*, each pruned by its box in *face_boxes* (one per part face), which
    /// must certainly contain the face.
    pub(crate) fn with_face_boxes(
        part: &'a Part,
        faces: Vec<usize>,
        face_boxes: Vec<Bounds>,
    ) -> Self {
        Self::with_geometry(part, faces, std::borrow::Cow::Owned(face_boxes))
    }

    fn with_geometry(
        part: &'a Part,
        faces: Vec<usize>,
        face_boxes: std::borrow::Cow<'a, [Bounds]>,
    ) -> Self {
        let edges = part.ray_edges.get_or_init(|| RayEdges {
            boxes: part
                .edges
                .iter()
                .map(|e| {
                    let mut b = Bounds::empty();
                    e.samples.iter().for_each(|p| b.add(*p));
                    b
                })
                .collect(),
            runs: part.edges.iter().map(|e| runs(&e.samples)).collect(),
            cracks: (0..part.edges.len())
                .map(|_| std::sync::OnceLock::new())
                .collect(),
        });
        let mut root_box = Bounds::empty();
        for &f in &faces {
            root_box.merge(&face_boxes[f]);
        }
        let edge_tol = (4.0 * CHORD_TOLERANCE).max(root_box.diagonal() * 1e-9);
        let root = build(faces.clone(), &face_boxes);
        let mut member = vec![false; part.faces.len()];
        faces.iter().for_each(|&f| member[f] = true);
        let intersectable = faces
            .iter()
            .all(|&f| !matches!(part.faces[f].surface, Surface::Other { .. }));
        RayCaster {
            part,
            faces,
            face_boxes,
            edge_boxes: &edges.boxes,
            edge_runs: &edges.runs,
            root,
            root_box,
            edge_tol,
            cracks: &edges.cracks,
            member,
            intersectable,
        }
    }

    /// The box of the faces rays meet.
    pub fn bounds(&self) -> Bounds {
        self.root_box
    }

    /// Whether any face's box shares volume with *b* (boxes that only touch do not).
    pub fn any_face_box_meets(&self, b: &Bounds) -> bool {
        self.faces.iter().any(|&f| {
            let fb = &self.face_boxes[f];
            (0..3).all(|i| fb.min[i] < b.max[i] && b.min[i] < fb.max[i])
        })
    }

    /// Every hit with `0 < t <= t_max` along the unit direction *dir*, nearest first (equal
    /// distances in face order). `None` when a face the ray reaches has a surface the kernel
    /// cannot intersect.
    pub fn hits(&self, origin: V3, dir: V3, t_max: f64) -> Option<Vec<Hit>> {
        let (crossings, _) = self.crossings(origin, dir, t_max)?;
        Some(crossings.into_iter().map(|c| c.hit).collect())
    }

    /// The hits on *faces* alone, exactly as [`Self::hits`] lists them, where those faces
    /// decide them: `None` when some face rays meet cannot be intersected (`hits` may then give
    /// up) or one of these faces' crossings lies only in an edge's band (it yields to a face
    /// across the edge, which these faces alone cannot say). A ray along a thread's axis
    /// crosses every turn, so asking only about the faces wanted saves the rest.
    pub fn hits_on(&self, faces: &[usize], origin: V3, dir: V3, t_max: f64) -> Option<Vec<Hit>> {
        if !self.intersectable {
            return None;
        }
        let mut out = Vec::new();
        let mut in_surface = false;
        let mut asked: Vec<usize> = faces.iter().copied().filter(|&f| self.member[f]).collect();
        asked.sort_unstable();
        asked.dedup();
        for i in asked {
            // The hierarchy's boxes hold the face's, so it reaches the face exactly when the
            // ray meets the face's own box.
            if ray_meets_box(origin, dir, t_max, &self.face_boxes[i], self.edge_tol) {
                self.face_crossings(i, (origin, dir, t_max), &mut out, &mut in_surface)?;
            }
        }
        if out.iter().any(|c| matches!(c.contact, Contact::Band(..))) {
            return None;
        }
        out.sort_by(|a, b| crossing_order(&a.hit, &b.hit));
        Some(out.into_iter().map(|c| c.hit).collect())
    }

    /// Whether the ray meets any face beyond `t_min` (and within `t_max`): the question
    /// visibility asks, answered at the first such face. `None` when no face is met but one the
    /// ray reaches has a surface the kernel cannot intersect (as [`Self::hits`]).
    pub fn any_hit(&self, origin: V3, dir: V3, t_min: f64, t_max: f64) -> Option<bool> {
        let mut unanswered = false;
        let met = self.near_first(origin, dir, t_max, |i| {
            let Some((ts, _)) = surface_hits(&self.part.faces[i].surface, origin, dir, t_max)
            else {
                unanswered = true;
                return false;
            };
            ts.iter().filter(|&(t, _)| t > t_min).any(|(t, uv)| {
                self.contact(i, geom::add(origin, geom::scale(dir, t)), uv, false)
                    .is_some()
            })
        });
        if met {
            return Some(true);
        }
        (!unanswered).then_some(false)
    }

    /// Whether [`Self::hits`] lists a hit beyond `after` and up to `until` on a face not among
    /// *skip*, where that is decided without the crossings of *skip* (and of the faces beyond
    /// the first such hit): `None` when some face rays meet cannot be intersected, or the only
    /// crossings found in that stretch lie in an edge's band (they may yield to a face across
    /// the edge). Asked from a face about the others, the face's own crossings, often the
    /// costliest (the ray grazes it at a silhouette), are left out.
    pub fn other_hit(
        &self,
        (origin, dir, t_max): (V3, V3, f64),
        skip: &[usize],
        (after, until): (f64, f64),
    ) -> Option<bool> {
        if !self.intersectable {
            return None;
        }
        let mut banded = false;
        let mut out = Vec::new();
        let met = self.near_first(origin, dir, t_max, |i| {
            if skip.contains(&i) {
                return false;
            }
            out.clear();
            let mut in_surface = false;
            // Every face here can be intersected.
            let _ = self.face_crossings(i, (origin, dir, t_max), &mut out, &mut in_surface);
            out.iter()
                .filter(|c| c.hit.t > after && c.hit.t <= until)
                .any(|c| {
                    let band = matches!(c.contact, Contact::Band(..));
                    banded |= band;
                    !band
                })
        });
        if met {
            Some(true)
        } else {
            (!banded).then_some(false)
        }
    }

    /// Calls *f* with each face whose box the ray meets (within `t_max`), nearer boxes first,
    /// until it answers true; whether it did. Nearer first because any face met answers a
    /// question of whether one is, and along a stack of faces (a thread's turns seen along its
    /// axis) the nearest is the likeliest to.
    fn near_first(
        &self,
        origin: V3,
        dir: V3,
        t_max: f64,
        mut f: impl FnMut(usize) -> bool,
    ) -> bool {
        let entry = |b: &Bounds| ray_box_entry(origin, dir, t_max, b, self.edge_tol);
        let Some(root) = entry(&self.root_box) else {
            return false;
        };
        let mut stack = vec![(root, &self.root)];
        while let Some((_, node)) = stack.pop() {
            match node {
                Node::Split(lb, l, rb, r) => {
                    let mut children = [(entry(lb), &**l), (entry(rb), &**r)];
                    // The farther pushed first, so the nearer is taken next.
                    if children[0].0 < children[1].0 {
                        children.swap(0, 1);
                    }
                    for (at, child) in children {
                        if let Some(at) = at {
                            stack.push((at, child));
                        }
                    }
                }
                Node::Leaf(faces) => {
                    for &i in faces {
                        if ray_meets_box(origin, dir, t_max, &self.face_boxes[i], self.edge_tol)
                            && f(i)
                        {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// The hits that lie on their faces' trimmed regions themselves, not merely within the
    /// tolerance of an edge: the crossings that bound material along a line. A line passing an
    /// edge at a distance sees one face or the other there, never both and never neither. A
    /// line that only touches a face at its boundary (a double root on an edge, as along the
    /// circle where a blend meets a face tangentially) meets it at that one point and does not
    /// pass through it: whatever the line crosses there belongs to the faces across the edge.
    pub fn trimmed_hits(&self, origin: V3, dir: V3, t_max: f64) -> Option<Vec<Hit>> {
        let (crossings, _) = self.crossings(origin, dir, t_max)?;
        let on_trim = |c: &Crossing| match c.contact {
            Contact::Edge { inside } => {
                let q = geom::add(origin, geom::scale(dir, c.hit.t));
                inside && !(c.tangent && self.touches(c.hit.face, q, c.uv, dir))
            }
            _ => true,
        };
        Some(
            crossings
                .into_iter()
                .filter(on_trim)
                .map(|c| c.hit)
                .collect(),
        )
    }

    /// Whether a ray along *dir* only touches face *i*'s surface at *q* (at parameters *uv*,
    /// where known): the surface's normal there is square to the ray.
    fn touches(&self, i: usize, q: V3, uv: Option<(f64, f64)>, dir: V3) -> bool {
        let surface = &self.part.faces[i].surface;
        uv.or_else(|| surface.parameters(q, None))
            .and_then(|(u, v)| surface.normal(u, v))
            .is_some_and(|n| geom::dot(n, dir).abs() <= TOUCH_COS)
    }

    /// How many times the ray crosses the faces, for parity; `None` when that cannot be
    /// trusted: the ray runs along a surface, touches one tangentially, passes through an
    /// edge (where its neighbours may both claim, or both miss, the crossing), or crosses two
    /// faces at one point (faces lying on each other, as where two solids touch).
    pub fn crossing_count(&self, origin: V3, dir: V3, t_max: f64) -> Option<usize> {
        let (crossings, in_surface) = self.crossings(origin, dir, t_max)?;
        let clean = !in_surface
            && crossings
                .iter()
                .all(|c| !c.tangent && !matches!(c.contact, Contact::Edge { .. }))
            && crossings
                .windows(2)
                .all(|w| w[1].hit.t - w[0].hit.t > self.edge_tol);
        clean.then_some(crossings.len())
    }

    /// The crossings along a ray, nearest first, with whether the ray lies in a surface. A
    /// crossing found only in a crack's band yields to the face across that edge: the ray
    /// crosses the crack once, whichever side of it the surfaces put the crossing.
    fn crossings(&self, origin: V3, dir: V3, t_max: f64) -> Option<(Vec<Crossing>, bool)> {
        let mut out = Vec::new();
        let mut in_surface = false;
        let mut stack = vec![(&self.root_box, &self.root)];
        while let Some((b, node)) = stack.pop() {
            if !ray_meets_box(origin, dir, t_max, b, self.edge_tol) {
                continue;
            }
            match node {
                Node::Split(lb, l, rb, r) => {
                    stack.push((lb, l));
                    stack.push((rb, r));
                }
                Node::Leaf(faces) => {
                    for &i in faces {
                        if !ray_meets_box(origin, dir, t_max, &self.face_boxes[i], self.edge_tol) {
                            continue;
                        }
                        self.face_crossings(i, (origin, dir, t_max), &mut out, &mut in_surface)?;
                    }
                }
            }
        }
        let order = crossing_order;
        out.sort_by(|a, b| order(&a.hit, &b.hit));
        if !out.iter().any(|c| matches!(c.contact, Contact::Band(..))) {
            return Some((out, in_surface));
        }
        let edge_faces = self.part.edge_faces();
        let (mut kept, banded): (Vec<Crossing>, Vec<Crossing>) = out
            .into_iter()
            .partition(|c| !matches!(c.contact, Contact::Band(..)));
        for c in banded {
            let Contact::Band(e, band) = c.contact else {
                continue;
            };
            let partnered = kept.iter().any(|k| {
                k.hit.face != c.hit.face
                    && edge_faces[e].contains(&k.hit.face)
                    && (k.hit.t - c.hit.t).abs() <= 10.0 * band
            });
            if !partnered {
                kept.push(c);
            }
        }
        kept.sort_by(|a, b| order(&a.hit, &b.hit));
        Some((kept, in_surface))
    }

    /// Face *i*'s crossings along the ray, before any yields to a face across an edge
    /// ([`Self::crossings`]), added to *out*; *in_surface* set when the ray lies in its surface.
    /// `None` when its surface cannot be intersected.
    fn face_crossings(
        &self,
        i: usize,
        (origin, dir, t_max): (V3, V3, f64),
        out: &mut Vec<Crossing>,
        in_surface: &mut bool,
    ) -> Option<()> {
        let (ts, tangent) = surface_hits(&self.part.faces[i].surface, origin, dir, t_max)?;
        *in_surface |= tangent && ts.is_empty();
        for (t, uv) in ts.iter() {
            let q = geom::add(origin, geom::scale(dir, t));
            if let Some(contact) = self.contact(i, q, uv, true)
                && !self.overshoots(i, q)
            {
                let hit = Hit { t, face: i };
                out.push(Crossing {
                    hit,
                    contact,
                    tangent,
                    uv,
                });
            }
        }
        Some(())
    }

    /// Whether face *i* holds a point *q* of its surface (by its trim, or an edge's band) only
    /// because an edge that strays from the surface overshoots: *q* lies within a few bands of
    /// the edge and past the surface of the face across it. The file leaves the boundary
    /// uncertain there (a B-spline edge standing for the intersection of two cylinders,
    /// displaced into the opening it bounds), and the two surfaces themselves place the point;
    /// along the surface the uncertainty exceeds the edge's stray, hence the reach. Both faces
    /// must be analytic: a B-spline face may itself approximate its neighbour, and does not
    /// place the point (applied to them, the classifier's rays disagree at four more corpus
    /// points).
    fn overshoots(&self, i: usize, q: V3) -> bool {
        let part = self.part;
        if !exact(&part.faces[i].surface) {
            return false;
        }
        part.edge_deviation(i).iter().any(|&(e, deviation)| {
            let band = COORD_FLOOR + deviation;
            deviation > COORD_FLOOR
                && self.edge_boxes[e].contains(q, OVERSHOOT_BANDS * band)
                && geom::dist(nearest_on_edge(&part.edges[e], q).0, q) <= OVERSHOOT_BANDS * band
                && self.past_neighbour(i, e, band, q)
        })
    }

    /// Whether a point *q* on face *i*'s surface, near its straying edge *e*, lies past the
    /// surface of a face across that edge: on the other side of it from face *i*'s own region
    /// (a point of the face ten bands from the edge, on the side of it the trim holds). The
    /// crossing is then the neighbour's territory, even where the line never crosses the
    /// neighbour (it runs along it). Undecided (false) where either point lies on the
    /// neighbour's surface within the coordinate floor, or the trim holds neither side or both.
    fn past_neighbour(&self, i: usize, e: usize, band: f64, q: V3) -> bool {
        let part = self.part;
        let surface = &part.faces[i].surface;
        // The edge strays from the surface: the way across it is taken from its foot on the
        // surface, so that it runs along the surface.
        let onto = |p: V3| {
            surface
                .parameters(p, None)
                .map(|(u, v)| surface.value(u, v))
        };
        let Some(foot) = onto(nearest_on_edge(&part.edges[e], q).0) else {
            return false;
        };
        let Some(away) = geom::unit(geom::sub(q, foot)) else {
            return false;
        };
        let at = |k: f64| onto(geom::add(foot, geom::scale(away, k * 10.0 * band)));
        let inside: Vec<V3> = [1.0, -1.0]
            .into_iter()
            .filter_map(at)
            .filter(|&p| self.contact(i, p, None, false) == Some(Contact::Interior))
            .collect();
        let [within] = inside[..] else {
            return false;
        };
        let side = |p: V3, s: &Surface| {
            let (u, v) = s.parameters(p, None)?;
            let n = s.normal(u, v)?;
            Some(geom::dot(n, geom::sub(p, s.value(u, v))))
        };
        part.edge_faces()[e].iter().filter(|&&b| b != i).any(|&b| {
            let s = &part.faces[b].surface;
            if !exact(s) {
                return false;
            }
            match (side(q, s), side(within, s)) {
                (Some(x), Some(y)) => {
                    x.abs() > COORD_FLOOR && y.abs() > COORD_FLOOR && (x > 0.0) != (y > 0.0)
                }
                _ => false,
            }
        })
    }

    /// Whether edge *e*'s polyline passes within *reach* of *q*: `polyline_distance(q, samples)
    /// <= reach`, the same distances compared.
    fn polyline_within(&self, e: usize, q: V3, reach: f64) -> bool {
        segments_near(&self.part.edges[e].samples, &self.edge_runs[e], q, reach)
            .any(|w| point_segment_distance(q, w[0], w[1]) <= reach)
    }

    /// Whether a point on face *i*'s surface lies on the face: inside its trim, on its boundary
    /// or in the band of an edge that strays from its surface.
    pub(super) fn claims(&self, i: usize, q: V3) -> bool {
        self.contact(i, q, None, false).is_some()
    }

    /// The furthest edge *e* strays from the surface of any face it bounds (sampled): the
    /// widest crack it can leave between them anywhere along it, which bounds the search for
    /// a point in it ([`Self::stray_across`] says how wide it is at the point).
    fn crack(&self, e: usize) -> f64 {
        *self.cracks[e].get_or_init(|| {
            let part = self.part;
            part.edge_faces()[e]
                .iter()
                .flat_map(|&f| part.edge_deviation(f).iter())
                .filter(|&&(edge, _)| edge == e)
                .fold(0.0, |widest, &(_, deviation)| widest.max(deviation))
        })
    }

    /// How far edge *e*'s point *c* lies from the surfaces of the faces across it from face
    /// *i*: the width of the crack the edge leaves there on their side.
    fn stray_across(&self, i: usize, e: usize, c: V3) -> f64 {
        let part = self.part;
        part.edge_faces()[e]
            .iter()
            .filter(|&&b| b != i)
            .filter_map(|&b| {
                let s = &part.faces[b].surface;
                s.parameters(c, None)
                    .map(|(u, v)| geom::dist(s.value(u, v), c))
            })
            .fold(0.0, f64::max)
    }

    /// How a point on face *i*'s surface meets the face, if it does. The band is where the file
    /// itself leaves the boundary uncertain: an edge that strays from this face's surface (a
    /// B-spline face approximating its neighbour) leaves a crack as wide as the stray, which
    /// OpenCascade covers with the edge tolerance it sets on import.
    ///
    /// With *across* (a crossing being counted, where a banded crossing yields to the face
    /// across the edge, [`Self::crossings`]) the band also reaches as far as the edge strays,
    /// at its point nearest *q*, from a face across it: the crack between the two faces is
    /// that wide there whichever face's surface the edge leaves. cgb217's cylinder face 51
    /// meets B-spline face 43 at an edge 2.0 µm off the cylinder and 11.9 µm off face 43
    /// there; a line crossing the cylinder 2.83 µm from it, outside the trim, meets face 43's
    /// surface nowhere near, and OpenCascade (edge tolerance 9.7 µm) counts the crossing on
    /// face 51, as parity requires. The edge's widest stray elsewhere along it is no evidence
    /// at *q* (cgb202's edge between faces 509 and 1863 strays 0.41 mm from face 509 somewhere,
    /// 0.19 µm where a line passes 0.37 mm from it). Without *across* (a hit asked about alone:
    /// visibility, probes) nothing would give the band up to a face across it that the ray
    /// also meets, so it keeps to this face's own stray.
    ///
    /// The trim is tested on polylines, which stray from a curved edge by up to the chord
    /// tolerance, so a point that close to an edge could be put on either side of it. On an
    /// analytic surface the side of each edge near is taken from its curve instead, so the two
    /// faces of an edge decide by the same exact curve and a crossing next to it, or through
    /// it, is claimed by exactly the faces the line crosses. A point nearest an edge's end lies
    /// outside that edge's sliver (chord and arc meet there), and an edge off the surface (a
    /// band) has no side on it to read, so neither changes the answer; nor does anything on a
    /// B-spline face, whose surface cannot be followed past its edges.
    ///
    /// *uv* are the point's parameters where the ray's intersection found them; otherwise they
    /// are found by inverting the surface at *q*.
    fn contact(&self, i: usize, q: V3, uv: Option<(f64, f64)>, across: bool) -> Option<Contact> {
        let part = self.part;
        let surface = &part.faces[i].surface;
        let exact = !matches!(surface, Surface::Freeform { .. });
        let domain = part.domain(i);
        let uv = uv.or_else(|| surface.parameters(q, None));
        let mut inside = domain.is_some_and(|d| uv.is_some_and(|(u, v)| d.contains(u, v)));
        let (mut on_edge, mut in_band) = (false, None);
        // Each edge whose curve passes within reach of the point between its ends: the edge,
        // its nearest point and the reach.
        let mut near: Vec<(usize, V3, f64)> = Vec::new();
        for &(e, deviation) in part.edge_deviation(i) {
            let reach = self.edge_tol + deviation;
            // The widest a crack across the edge can be (it holds this face's own stray).
            let crack = if across { self.crack(e) } else { deviation };
            let edge = &part.edges[e];
            // The polyline strays from the curve by less than the edge tolerance.
            let find = self.edge_tol + crack;
            if !self.edge_boxes[e].contains(q, find) || !self.polyline_within(e, q, find) {
                continue;
            }
            // The polyline only finds the edges near; the distance that decides is the curve's,
            // at the tolerance OpenCascade's intersector is loaded with.
            let (c, between_ends) = nearest_on_edge(edge, q);
            let d = geom::dist(c, q);
            on_edge |= d <= COORD_FLOOR;
            if in_band.is_none() {
                // Past this face's own stray, only where the edge strays as far from a face
                // across it, there: the crack's widest elsewhere is no evidence at this point.
                let stray = if d <= COORD_FLOOR + deviation {
                    Some(deviation)
                } else if d <= COORD_FLOOR + crack {
                    let wide = self.stray_across(i, e, c).min(crack);
                    (d <= COORD_FLOOR + wide).then_some(wide)
                } else {
                    None
                };
                in_band = stray.map(|s| Contact::Band(e, COORD_FLOOR + s));
            }
            // Nearer an end than the point is to the curve, the sliver is too thin to hold it.
            let clear_of_ends = between_ends
                && (edge.is_closed() || geom::dist(c, edge.start).min(geom::dist(c, edge.end)) > d);
            if exact && d <= reach && clear_of_ends && deviation <= COORD_FLOOR {
                near.push((e, c, reach));
            }
        }
        if let Some(uv) = uv {
            for &(e, c, reach) in &near {
                if misplaces(
                    surface,
                    q,
                    uv,
                    &part.edges[e].samples,
                    &self.edge_runs[e],
                    c,
                    reach,
                ) {
                    inside = !inside;
                }
            }
        }
        if on_edge {
            return Some(Contact::Edge { inside });
        }
        if inside {
            Some(Contact::Interior)
        } else {
            in_band
        }
    }
}

/// Hits nearest first, equal distances in face order.
fn crossing_order(a: &Hit, b: &Hit) -> std::cmp::Ordering {
    a.t.total_cmp(&b.t).then(a.face.cmp(&b.face))
}

/// A ray's hit on a surface: its distance and, where known, its surface parameters.
type SurfaceHit = (f64, Option<(f64, f64)>);

/// [`Surface::ray_hits`] with each hit's surface parameters where the intersection finds them:
/// a B-spline crossing is solved for `(t, u, v)` together, and those parameters are the point's
/// exactly. Inverting the surface at the point again need not find them: from the nearest
/// sample of a folded surface (cgb243's combs, faces 225, 223, 238 and 239) the inversion can
/// settle on a boundary 1.3 to 1.5 mm from the point, and the trim test then misses a crossing
/// well inside the face.
fn surface_hits(surface: &Surface, origin: V3, dir: V3, t_max: f64) -> Option<(SurfaceHits, bool)> {
    if let Surface::Freeform { surface, .. } = surface {
        let solve = || {
            let (mut hits, grazing) = surface.ray_hits(origin, dir, t_max);
            hits.retain(|&(t, _, _)| t > 1e-12 && t <= t_max);
            (hits, grazing)
        };
        let (hits, grazing) = remembered(surface, (origin, dir, t_max), solve);
        return Some((SurfaceHits::Freeform(hits), grazing));
    }
    let (ts, tangent) = surface.ray_hits(origin, dir, t_max)?;
    Some((SurfaceHits::Analytic(ts), tangent))
}

/// One B-spline surface's hits along one ray, as [`surface_hits`] gives them.
type FreeformHits = (Vec<(f64, f64, f64)>, bool);

/// The rays [`remembering`] keeps the B-spline hits of, at most: one point's visibility asks
/// along its ray to the viewer, and from a point or two beside it, several times.
const REMEMBERED_RAYS: usize = 4;

/// Per ray (origin, direction and reach, as bits), each B-spline surface's hits along it (the
/// surface by address): while [`remembering`], the latest rays asked.
type Remembered = Vec<([u64; 7], Vec<(usize, FreeformHits)>)>;

thread_local! {
    static REMEMBERED: std::cell::RefCell<Option<Remembered>> =
        const { std::cell::RefCell::new(None) };
}

/// Runs *f* with this thread remembering B-spline surfaces' hits along the rays it asks
/// ([`REMEMBERED_RAYS`] at a time), so that asking a ray again (the same surface, origin,
/// direction and reach) reuses the answer rather than solving it again: the answer is the
/// same. Within one question about one part only, as a surface is known by its address.
pub(super) fn remembering<R>(f: impl FnOnce() -> R) -> R {
    struct Forget;
    impl Drop for Forget {
        fn drop(&mut self) {
            REMEMBERED.with(|r| *r.borrow_mut() = None);
        }
    }
    REMEMBERED.with(|r| *r.borrow_mut() = Some(Vec::new()));
    let _forget = Forget;
    f()
}

/// *solve*'s hits of *surface* along *ray*, remembered while [`remembering`].
fn remembered(
    surface: &super::nurbs::NurbsSurface,
    (origin, dir, t_max): (V3, V3, f64),
    solve: impl FnOnce() -> FreeformHits,
) -> FreeformHits {
    let key = [
        origin[0], origin[1], origin[2], dir[0], dir[1], dir[2], t_max,
    ]
    .map(f64::to_bits);
    let id = surface as *const _ as usize;
    let known = REMEMBERED.with(|r| {
        let r = r.borrow();
        let rays = r.as_ref()?;
        let (_, surfaces) = rays.iter().find(|(k, _)| *k == key)?;
        surfaces
            .iter()
            .find(|(s, _)| *s == id)
            .map(|(_, h)| h.clone())
    });
    if let Some(hits) = known {
        return hits;
    }
    let hits = solve();
    REMEMBERED.with(|r| {
        let mut r = r.borrow_mut();
        let Some(rays) = r.as_mut() else {
            return;
        };
        let at = match rays.iter().position(|(k, _)| *k == key) {
            Some(at) => at,
            None => {
                if rays.len() == REMEMBERED_RAYS {
                    rays.remove(0);
                }
                rays.push((key, Vec::new()));
                rays.len() - 1
            }
        };
        rays[at].1.push((id, hits.clone()));
    });
    hits
}

/// A surface's hits as the intersection returns them, read as [`SurfaceHit`]s without copying
/// them: a ray meets a great many surfaces.
enum SurfaceHits {
    Analytic(Vec<f64>),
    Freeform(Vec<(f64, f64, f64)>),
}

impl SurfaceHits {
    fn is_empty(&self) -> bool {
        match self {
            SurfaceHits::Analytic(ts) => ts.is_empty(),
            SurfaceHits::Freeform(hits) => hits.is_empty(),
        }
    }

    fn iter(&self) -> impl Iterator<Item = SurfaceHit> + '_ {
        let (ts, hits): (&[f64], &[(f64, f64, f64)]) = match self {
            SurfaceHits::Analytic(ts) => (ts, &[]),
            SurfaceHits::Freeform(hits) => (&[], hits),
        };
        ts.iter()
            .map(|&t| (t, None))
            .chain(hits.iter().map(|&(t, u, v)| (t, Some((u, v)))))
    }
}

/// Whether a surface is analytic (exact), not a B-spline that may approximate a neighbour.
fn exact(surface: &Surface) -> bool {
    !matches!(surface, Surface::Freeform { .. } | Surface::Other { .. })
}

/// The point of an edge nearest *q*, and whether it lies between the edge's ends (else it is
/// the nearer end).
fn nearest_on_edge(edge: &Edge, q: V3) -> (V3, bool) {
    let t = edge.curve.parameter(q);
    if edge.is_closed() {
        return (edge.curve.value(t), true);
    }
    let (a, b) = edge_interval(&edge.curve, edge.start, edge.end, edge.same_sense, false);
    let (lo, hi) = (a.min(b), a.max(b));
    let t = edge
        .curve
        .period()
        .map_or(t, |period| lo + (t - lo).rem_euclid(period));
    if (lo..=hi).contains(&t) {
        (edge.curve.value(t), true)
    } else if geom::dist(edge.start, q) <= geom::dist(edge.end, q) {
        (edge.start, false)
    } else {
        (edge.end, false)
    }
}

/// Whether an edge's polyline, in *surface*'s parameters as the trim test sees it, puts the
/// point *q* (at parameters *uv*) on the other side of the edge from where its curve does: the
/// way from *q* through *c*, the curve's nearest point, to *beyond* past it crosses the curve
/// once, and must cross the polyline an odd number of times too. Only segments within the
/// length of that way of *q* can cross it.
fn misplaces(
    surface: &Surface,
    q: V3,
    uv: (f64, f64),
    samples: &[V3],
    runs: &[Bounds],
    c: V3,
    beyond: f64,
) -> bool {
    let (pu, pv) = surface.periodic();
    let at = |p: V3| {
        surface.parameters(p, Some(uv)).map(|(u, v)| {
            (
                if pu { geom::nearest_turn(u, uv.0) } else { u },
                if pv { geom::nearest_turn(v, uv.1) } else { v },
            )
        })
    };
    let d = geom::dist(q, c);
    // On the curve to within rounding there is no side to read.
    if d <= 1e-10 {
        return false;
    }
    let end = geom::add(c, geom::scale(geom::sub(c, q), beyond / d));
    let Some(e2) = at(end) else {
        return false;
    };
    let way = (e2.0 - uv.0, e2.1 - uv.1);
    let side = |p: (f64, f64)| way.0 * (p.1 - uv.1) - way.1 * (p.0 - uv.0);
    let along = |p: (f64, f64)| way.0 * (p.0 - uv.0) + way.1 * (p.1 - uv.1);
    let mut crossings = 0;
    for w in segments_near(samples, runs, q, d + beyond) {
        if point_segment_distance(q, w[0], w[1]) > d + beyond {
            continue;
        }
        let (Some(a), Some(b)) = (at(w[0]), at(w[1])) else {
            continue;
        };
        let (sa, sb) = (side(a), side(b));
        if (sa > 0.0) == (sb > 0.0) {
            continue;
        }
        // Where the segment meets the line of the way, as a fraction of it.
        let k = sa / (sa - sb);
        let x = (a.0 + k * (b.0 - a.0), a.1 + k * (b.1 - a.1));
        let s = along(x) / (way.0 * way.0 + way.1 * way.1);
        if s > 0.0 && s < 1.0 {
            crossings += 1;
        }
    }
    crossings % 2 == 0
}

/// How many polyline segments a box of [`RayCaster::edge_runs`] holds.
const RUN: usize = 8;

/// The boxes of a polyline's runs of [`RUN`] segments, each padded by [`run_slack`]: a
/// segment's computed distance from a point outside a box so grown by a reach exceeds the
/// reach, whatever the rounding of the nearest point and its distance.
fn runs(samples: &[V3]) -> Vec<Bounds> {
    (0..samples.len().saturating_sub(1).div_ceil(RUN))
        .map(|r| {
            let mut b = Bounds::empty();
            samples[r * RUN..samples.len().min(r * RUN + RUN + 1)]
                .iter()
                .for_each(|p| b.add(*p));
            let slack = run_slack(&b);
            Bounds {
                min: b.min.map(|x| x - slack),
                max: b.max.map(|x| x + slack),
            }
        })
        .collect()
}

/// The padding of a run's box: far beyond the rounding of a nearest point on its segments
/// (a few units in the last place of its coordinates) and of a distance from it.
fn run_slack(b: &Bounds) -> f64 {
    let size = (0..3).fold(0.0f64, |m, k| m.max(b.min[k].abs()).max(b.max[k].abs()));
    1e-9 * (1.0 + size)
}

/// A polyline's segments (as sample pairs, in order) but for the runs whose box (of [`runs`])
/// leaves *q* out by more than *reach*: those segments all lie further than *reach* from it.
fn segments_near<'s>(
    samples: &'s [V3],
    runs: &'s [Bounds],
    q: V3,
    reach: f64,
) -> impl Iterator<Item = &'s [V3]> {
    runs.iter()
        .enumerate()
        .filter(move |(_, b)| b.contains(q, reach))
        .flat_map(move |(r, _)| samples[r * RUN..samples.len().min(r * RUN + RUN + 1)].windows(2))
}

/// The distance from *p* to a polyline.
pub(super) fn polyline_distance(p: V3, samples: &[V3]) -> f64 {
    samples
        .windows(2)
        .map(|w| point_segment_distance(p, w[0], w[1]))
        .fold(f64::INFINITY, f64::min)
}

impl Part {
    /// Each of the face's edges with the furthest its samples lie from the face's surface.
    pub fn edge_deviation(&self, face: usize) -> &[(usize, f64)] {
        self.cache[face].edge_deviation.get_or_init(|| {
            let surface = &self.faces[face].surface;
            self.face_edges(face)
                .into_iter()
                .map(|e| {
                    let mut hint = None;
                    let worst = self.edges[e].samples.iter().fold(0.0f64, |worst, &p| {
                        let Some(uv) = surface.parameters(p, hint) else {
                            return worst;
                        };
                        hint = Some(uv);
                        worst.max(geom::dist(surface.value(uv.0, uv.1), p))
                    });
                    (e, worst)
                })
                .collect()
        })
    }

    /// `Face.position_at(u, v)`: the surface point at fractions (u, v) of the face's
    /// `BRepTools::UVBounds`, with its parameters.
    pub fn position_at(&self, face: usize, u: f64, v: f64) -> Option<(V3, (f64, f64))> {
        let (u0, u1, v0, v1) = self.uv_bounds(face)?;
        let (pu, pv) = (u0 + u * (u1 - u0), v0 + v * (v1 - v0));
        Some((self.faces[face].surface.value(pu, pv), (pu, pv)))
    }

    /// `Face.normal_at(point)`: the face's outward normal at the surface point nearest *p*;
    /// `None` where the surface has no normal (a pole or an apex).
    pub fn normal_at_point(&self, face: usize, p: V3) -> Option<V3> {
        let (u, v) = self.faces[face].surface.parameters(p, None)?;
        self.face_normal(face, u, v)
    }
}

const LEAF: usize = 4;

fn build(mut faces: Vec<usize>, boxes: &[Bounds]) -> Node {
    if faces.len() <= LEAF {
        return Node::Leaf(faces);
    }
    let mut all = Bounds::empty();
    for &f in &faces {
        all.merge(&boxes[f]);
    }
    let extent = geom::sub(all.max, all.min);
    let axis = (0..3)
        .max_by(|&a, &b| extent[a].total_cmp(&extent[b]))
        .unwrap_or(0);
    let centre = |f: usize| boxes[f].min[axis] + boxes[f].max[axis];
    faces.sort_by(|&a, &b| centre(a).total_cmp(&centre(b)).then(a.cmp(&b)));
    let right = faces.split_off(faces.len() / 2);
    let span = |fs: &[usize]| {
        let mut b = Bounds::empty();
        fs.iter().for_each(|&f| b.merge(&boxes[f]));
        b
    };
    let (lb, rb) = (span(&faces), span(&right));
    Node::Split(
        Box::new(lb),
        Box::new(build(faces, boxes)),
        Box::new(rb),
        Box::new(build(right, boxes)),
    )
}

fn point_segment_distance(p: V3, a: V3, b: V3) -> f64 {
    let ab = geom::sub(b, a);
    let len2 = geom::dot(ab, ab);
    let t = if len2 <= 0.0 {
        0.0
    } else {
        (geom::dot(geom::sub(p, a), ab) / len2).clamp(0.0, 1.0)
    };
    geom::dist(p, geom::add(a, geom::scale(ab, t)))
}

pub(super) fn ray_meets_box(p: V3, dir: V3, t_max: f64, b: &Bounds, pad: f64) -> bool {
    ray_box_entry(p, dir, t_max, b, pad).is_some()
}

/// Where the ray enters box *b* (padded by *pad*), as [`ray_meets_box`] decides it meets the
/// box; `None` when it does not.
fn ray_box_entry(p: V3, dir: V3, t_max: f64, b: &Bounds, pad: f64) -> Option<f64> {
    let (mut t0, mut t1) = (0.0f64, t_max + pad);
    for i in 0..3 {
        let (lo, hi) = (b.min[i] - pad, b.max[i] + pad);
        if dir[i].abs() < 1e-300 {
            if p[i] < lo || p[i] > hi {
                return None;
            }
            continue;
        }
        let (mut a, mut c) = ((lo - p[i]) / dir[i], (hi - p[i]) / dir[i]);
        if a > c {
            std::mem::swap(&mut a, &mut c);
        }
        t0 = t0.max(a);
        t1 = t1.min(c);
        if t0 > t1 {
            return None;
        }
    }
    Some(t0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_geometry_is_shared_but_membership_and_custom_boxes_are_not() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/two_solids.step");
        let loaded = crate::read_step_file(&path).unwrap();
        let part = Part::new(
            loaded.faces.clone(),
            loaded.edges.clone(),
            loaded.solids.clone(),
        );
        let custom_boxes: Vec<_> = (0..part.faces.len()).map(|f| part.face_bounds(f)).collect();
        let custom = RayCaster::with_face_boxes(&part, vec![0], custom_boxes.clone());
        assert!(part.ray_face_boxes.get().is_none());
        assert!(matches!(custom.face_boxes, std::borrow::Cow::Owned(_)));
        assert_eq!(&*custom.face_boxes, custom_boxes);
        let first = RayCaster::for_solid(&part, 0);
        let second = RayCaster::for_solid(&part, 1);
        assert!(std::ptr::eq(
            first.face_boxes.as_ptr(),
            second.face_boxes.as_ptr()
        ));
        assert!(std::ptr::eq(
            first.edge_boxes.as_ptr(),
            second.edge_boxes.as_ptr()
        ));
        assert!(std::ptr::eq(
            first.edge_runs.as_ptr(),
            second.edge_runs.as_ptr()
        ));
        assert!(std::ptr::eq(first.cracks.as_ptr(), second.cracks.as_ptr()));
        assert!(std::ptr::eq(
            first.edge_boxes.as_ptr(),
            custom.edge_boxes.as_ptr()
        ));
        assert_ne!(first.faces, second.faces);
        assert_ne!(first.member, second.member);
        assert_ne!(first.root_box, second.root_box);
    }

    #[test]
    fn shared_ray_queries_keep_hits_and_classification_from_fresh_parts() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
        for name in [
            "two_solids.step",
            "rejected_bored_box.step",
            "held_side_knot.step",
        ] {
            let part = crate::read_step_file(&dir.join(name)).unwrap();
            let bounds = part.bounds();
            let span = bounds.diagonal() + 1.0;
            for solid in 0..part.solids.len() {
                let shared = crate::classify::Classifier::for_solid(&part, solid);
                let independent =
                    Part::new(part.faces.clone(), part.edges.clone(), part.solids.clone());
                let fresh = crate::classify::Classifier::for_solid(&independent, solid);
                for point in [bounds.centre(), bounds.min, bounds.max, [0.0; 3]] {
                    assert_eq!(shared.classify(point), fresh.classify(point), "{name}");
                    for direction in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
                        let bits = |hits: Option<Vec<Hit>>| {
                            hits.map(|v| {
                                v.into_iter()
                                    .map(|h| (h.face, h.t.to_bits()))
                                    .collect::<Vec<_>>()
                            })
                        };
                        assert_eq!(
                            bits(shared.rays().hits(point, direction, span)),
                            bits(fresh.rays().hits(point, direction, span)),
                            "{name} solid {solid}"
                        );
                    }
                }
                for edge in part.solids[solid]
                    .faces
                    .iter()
                    .flat_map(|&f| part.face_edges(f))
                {
                    assert_eq!(
                        shared.rays().crack(edge).to_bits(),
                        fresh.rays().crack(edge).to_bits()
                    );
                }
            }
        }
    }

    #[test]
    fn ray_geometry_is_invalidated_by_transform_and_orientation_repair() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/two_solids.step");
        let mut part = crate::read_step_file(&path).unwrap();
        let before = RayCaster::for_solid(&part, 0);
        let moved = part
            .transformed(&[
                [1.0, 0.0, 0.0, 10.0],
                [0.0, 1.0, 0.0, -7.0],
                [0.0, 0.0, 1.0, 4.0],
            ])
            .unwrap();
        assert!(moved.ray_face_boxes.get().is_none());
        assert!(moved.ray_edges.get().is_none());
        let after = RayCaster::for_solid(&moved, 0);
        assert!(!std::ptr::eq(
            before.edge_boxes.as_ptr(),
            after.edge_boxes.as_ptr()
        ));
        assert_ne!(before.root_box, after.root_box);
        drop(before);
        part.record_inverted_shell_repair(0, true);
        assert!(part.ray_face_boxes.get().is_none());
        assert!(part.ray_edges.get().is_none());
        let independent = Part::new(part.faces.clone(), part.edges.clone(), part.solids.clone());
        let repaired = RayCaster::for_solid(&part, 0);
        let fresh = RayCaster::for_solid(&independent, 0);
        assert_eq!(repaired.face_boxes, fresh.face_boxes);
        assert_eq!(repaired.edge_boxes, fresh.edge_boxes);
        assert_eq!(repaired.edge_runs, fresh.edge_runs);
        drop(repaired);
        let flipped = part.solids[0].faces[0];
        part.record_mixed_normal_repair(0, &[flipped]);
        assert!(part.ray_face_boxes.get().is_none());
        assert!(part.ray_edges.get().is_none());
        let independent = Part::new(part.faces.clone(), part.edges.clone(), part.solids.clone());
        let repaired = RayCaster::for_solid(&part, 0);
        let fresh = RayCaster::for_solid(&independent, 0);
        assert_eq!(repaired.face_boxes, fresh.face_boxes);
        assert_eq!(repaired.edge_boxes, fresh.edge_boxes);
        assert_eq!(repaired.edge_runs, fresh.edge_runs);
    }

    #[test]
    fn any_hit_is_unknown_where_only_an_unintersectable_face_is_reached() {
        // A 30 × 30 × 20 block about the origin, bored through along z.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/rejected_bored_box.step");
        let mut part = crate::read_step_file(&path).unwrap();
        let (origin, dir) = ([20.0, 10.0, 0.0], [-1.0, 0.0, 0.0]);
        let all: Vec<usize> = (0..part.faces.len()).collect();
        let side = RayCaster::for_faces(&part, all.clone())
            .hits(origin, dir, 100.0)
            .unwrap()[0];
        assert!((side.t - 5.0).abs() < 1e-9, "the x = 15 side first");
        part.faces[side.face].surface = Surface::Other { kind: "TEST" };
        let rays = RayCaster::for_faces(&part, all);
        // Only the unanswerable side lies within reach; beyond it, the far side is met.
        assert_eq!(rays.any_hit(origin, dir, 0.0, 10.0), None);
        assert_eq!(rays.any_hit(origin, dir, 0.0, 100.0), Some(true));
        assert_eq!(rays.hits(origin, dir, 10.0), None);
        // A ray that reaches no face at all is a clean miss.
        assert_eq!(
            rays.any_hit(origin, [1.0, 0.0, 0.0], 0.0, 100.0),
            Some(false)
        );
    }

    /// The bored box (a 30 × 30 × 20 block about the origin, a radius-5 bore along z) and its
    /// rim at z = 10: the bore's centre there, a unit direction across the bore, and the widest
    /// sliver the rim's polyline leaves (chord to arc), in that direction.
    fn bore_rim() -> (Part, V3, V3, f64) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/rejected_bored_box.step");
        let part = crate::read_step_file(&path).unwrap();
        let edge = part
            .edges
            .iter()
            .find(|e| {
                matches!(e.curve, geom::Curve::Circle { .. }) && (e.start[2] - 10.0).abs() < 1e-9
            })
            .unwrap();
        let geom::Curve::Circle { frame, radius } = edge.curve else {
            unreachable!()
        };
        let chord = edge
            .samples
            .windows(2)
            .map(|w| geom::scale(geom::add(w[0], w[1]), 0.5))
            .min_by(|a, b| geom::dist(*a, frame.origin).total_cmp(&geom::dist(*b, frame.origin)))
            .unwrap();
        let across = geom::unit(geom::sub(chord, frame.origin)).unwrap();
        (
            part,
            frame.origin,
            across,
            radius - geom::dist(chord, frame.origin),
        )
    }

    #[test]
    fn a_crossing_beside_a_curved_edge_is_claimed_by_the_face_its_curve_puts_it_on() {
        let (part, centre, across, sliver) = bore_rim();
        assert!(
            sliver > 10.0 * COORD_FLOOR,
            "the polyline leaves a sliver: {sliver}"
        );
        let rays = RayCaster::for_solid(&part, 0);
        let at = |r: f64| geom::add(centre, geom::scale(across, 5.0 + r));
        let through = |q: V3, d: V3| (geom::sub(q, geom::scale(d, 40.0)), d);
        // Down and outwards through the sliver, inside the arc: the ray enters the bore's
        // opening (air) and crosses into the wall, so the top face must not claim it, though
        // its polyline puts the point on it. Then out through the bottom: two crossings.
        let down_out = geom::unit(geom::add(geom::scale(across, 0.3), [0.0, 0.0, -1.0])).unwrap();
        let (o, d) = through(at(-0.5 * sliver), down_out);
        assert_eq!(rays.crossing_count(o, d, 80.0), Some(2));
        let hits = rays.trimmed_hits(o, d, 80.0).unwrap();
        assert_eq!(hits.len(), 2);
        assert!(matches!(
            part.faces[hits[0].face].surface,
            Surface::Cylinder { .. }
        ));
        // Just outside the arc the top face claims it, and the wall takes the ray back into the
        // bore just below: two crossings again.
        let down_in = geom::unit(geom::add(geom::scale(across, -0.02), [0.0, 0.0, -1.0])).unwrap();
        let (o, d) = through(at(0.5 * sliver), down_in);
        assert_eq!(rays.crossing_count(o, d, 80.0), Some(2));
        let first = rays.hits(o, d, 80.0).unwrap()[0].face;
        assert!(matches!(part.faces[first].surface, Surface::Plane { .. }));
        // Through the edge itself (within the coordinate floor): an even count or none, never
        // an odd one, whichever side of the arc the ray passes.
        for r in [0.0, 0.5 * COORD_FLOOR, -0.5 * COORD_FLOOR] {
            for d in [down_out, down_in] {
                let (o, d) = through(at(r), d);
                if let Some(n) = rays.crossing_count(o, d, 80.0) {
                    assert_eq!(n % 2, 0, "{n} crossings through the edge at {r}");
                }
            }
        }
    }

    #[test]
    fn edge_and_tangent_contacts_on_analytic_faces() {
        let (part, centre, across, sliver) = bore_rim();
        let rays = RayCaster::for_solid(&part, 0);
        let at = |r: f64| geom::add(centre, geom::scale(across, 5.0 + r));
        // Up the bore and out through its opening inside the arc, in the sliver: nothing is
        // met, though the top face's polyline takes in the point.
        let up = geom::unit(geom::add(geom::scale(across, 0.01), [0.0, 0.0, 1.0])).unwrap();
        let o = geom::sub(at(-0.5 * sliver), geom::scale(up, 5.0));
        assert_eq!(rays.any_hit(o, up, 0.0, 20.0), Some(false));
        assert!(rays.hits(o, up, 20.0).unwrap().is_empty());
        // Through the rim itself the boundary is met (OpenCascade reports `ON` points), and the
        // count is not trusted.
        let o = geom::sub(at(0.0), geom::scale(up, 5.0));
        assert_eq!(rays.any_hit(o, up, 0.0, 20.0), Some(true));
        assert_eq!(rays.crossing_count(o, up, 20.0), None);
        // Along the bore's wall, touching it at mid-height: met, and not counted.
        let along = geom::cross(across, [0.0, 0.0, 1.0]);
        let touch = geom::sub(at(0.0), [0.0, 0.0, 10.0]);
        let o = geom::sub(touch, geom::scale(along, 3.0));
        assert_eq!(rays.any_hit(o, along, 0.0, 6.0), Some(true));
        assert_eq!(rays.crossing_count(o, along, 6.0), None);
    }

    #[test]
    fn the_runs_skip_only_segments_out_of_reach() {
        // Every edge of the bored box, from points about and on it, at reaches from rounding
        // to the box's size: the runs give the answer the whole polyline does.
        let (part, centre, ..) = bore_rim();
        let rays = RayCaster::for_solid(&part, 0);
        for (e, edge) in part.edges.iter().enumerate() {
            let probes = edge.samples.iter().flat_map(|&p| {
                [0.0, 1e-7, 0.3, 2.0].map(|k| geom::add(p, geom::scale(geom::sub(p, centre), k)))
            });
            for q in probes.chain([centre, [20.0, -3.0, 4.0]]) {
                let d = polyline_distance(q, &edge.samples);
                for reach in [1e-12, 1e-7, 0.01, 0.5, 3.0, 50.0, d, d * (1.0 - 1e-15)] {
                    assert_eq!(
                        rays.polyline_within(e, q, reach),
                        d <= reach,
                        "{e} {q:?} {reach}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_line_touching_a_face_at_its_edge_bounds_no_material_there() {
        // The bored box: a line along y at x = 5 touches the bore (radius 5 about z) at
        // (5, 0, 0), a double root on the bore's seam edge, between the block's sides at
        // y = ±15; it runs in the material all the way.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/rejected_bored_box.step");
        let part = crate::read_step_file(&path).unwrap();
        let rays = RayCaster::for_solid(&part, 0);
        let (o, d) = ([5.0, -20.0, 0.0], [0.0, 1.0, 0.0]);
        assert_eq!(rays.hits(o, d, 40.0).unwrap().len(), 3, "the touch is met");
        let ts: Vec<f64> = rays
            .trimmed_hits(o, d, 40.0)
            .unwrap()
            .iter()
            .map(|h| h.t)
            .collect();
        assert_eq!(ts, [5.0, 35.0]);
    }
}
