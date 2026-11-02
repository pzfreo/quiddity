//! Model-space linear dimensions. Arrow triangles and shortest-first, outward ladder rule
//! ported from Draftwright aaa7a73, dims/placement.rs (respace/place_linear_at/arrow) and
//! dims/leaders.rs (head), Paul Fremantle, at the copyright owner's direction for #137.
//! No sheets, lanes around other views, or authoring policy are imported.
use super::{Callout, placement::Placed};
use crate::{
    Part,
    geom::{self, Bounds, Surface, V3},
    pmi::*,
    rays::RayCaster,
};

const AXES: [V3; 3] = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
fn canonical(v: V3) -> V3 {
    let v = geom::unit(v).unwrap_or([1., 0., 0.]);
    if v.iter().find(|x| x.abs() > 1e-9).is_some_and(|x| *x < 0.) {
        geom::scale(v, -1.)
    } else {
        v
    }
}
fn range(b: Bounds, d: V3) -> [f64; 2] {
    let mut r = [f64::INFINITY, f64::NEG_INFINITY];
    for x in [b.min[0], b.max[0]] {
        for y in [b.min[1], b.max[1]] {
            for z in [b.min[2], b.max[2]] {
                let t = geom::dot([x, y, z], d);
                r[0] = r[0].min(t);
                r[1] = r[1].max(t);
            }
        }
    }
    r
}
#[derive(Clone, Debug)]
enum Element {
    Plane(usize),
    Axis(usize),
    Point(V3),
}
impl Element {
    fn bounds(&self, part: &Part) -> Bounds {
        match *self {
            Self::Plane(f) | Self::Axis(f) => part.face_bounds(f),
            Self::Point(p) => Bounds { min: p, max: p },
        }
    }
    fn point(&self, part: &Part) -> V3 {
        match *self {
            Self::Point(p) => p,
            Self::Axis(f) => match part.faces[f].surface {
                Surface::Cylinder { frame, .. }
                | Surface::Cone { frame, .. }
                | Surface::Torus { frame, .. } => frame.origin,
                _ => self.bounds(part).centre(),
            },
            Self::Plane(f) => match part.faces[f].surface {
                Surface::Plane { frame } => frame.origin,
                _ => self.bounds(part).centre(),
            },
        }
    }
    fn axis(&self, part: &Part) -> Option<V3> {
        match *self {
            Self::Axis(f) => match part.faces[f].surface {
                Surface::Cylinder { frame, .. }
                | Surface::Cone { frame, .. }
                | Surface::Torus { frame, .. } => Some(canonical(frame.z)),
                _ => None,
            },
            _ => None,
        }
    }
    fn normal(&self, part: &Part) -> Option<V3> {
        match *self {
            Self::Plane(f) => match part.faces[f].surface {
                Surface::Plane { frame } => Some(canonical(frame.z)),
                _ => None,
            },
            _ => None,
        }
    }
    fn foot(
        &self,
        part: &Part,
        caster: &RayCaster<'_>,
        d: V3,
        v: V3,
        w: V3,
        level: f64,
    ) -> Option<V3> {
        let p = self.point(part);
        match *self {
            Self::Point(p) => ((geom::dot(p, w) - level).abs() < 1e-6).then_some(p),
            Self::Axis(_) => {
                let a = self.axis(part)?;
                let den = geom::dot(a, w);
                if den.abs() < 1e-9 {
                    return ((geom::dot(p, w) - level).abs() < 1e-6).then_some(p);
                }
                Some(geom::add(
                    p,
                    geom::scale(a, (level - geom::dot(p, w)) / den),
                ))
            }
            Self::Plane(f) => {
                let extent = part.bounds().diagonal().max(1.);
                let along = range(part.bounds(), d)[0] - extent;
                let r = range(self.bounds(part), v);
                // Interior stations first, then trim endpoints. RayCaster tests the real face,
                // including its holes and split boundaries, not only its supporting plane.
                for fraction in [0.95, 0.75, 0.5, 0.25, 0.05, 1., 0.] {
                    let t = r[0] + fraction * (r[1] - r[0]);
                    let origin = geom::add(
                        geom::scale(d, along),
                        geom::add(geom::scale(v, t), geom::scale(w, level)),
                    );
                    if let Some(hit) = caster.hits_on(&[f], origin, d, extent * 4.)?.first() {
                        return Some(geom::add(origin, geom::scale(d, hit.t)));
                    }
                }
                None
            }
        }
    }
}
fn edge_midpoint(e: &crate::brep::Edge) -> V3 {
    // For splines/ellipses Edge::midpoint uses sampled arc length. Project its chord
    // interpolation back onto the actual curve before using it as an extension foot.
    e.curve.value(e.curve.parameter(e.midpoint()))
}
fn elements(
    part: &Part,
    edges: &[Option<usize>],
    p: &PartPmi,
    feature: FeatureId,
    depth: usize,
) -> Vec<Element> {
    if depth > p.features.len() {
        return Vec::new();
    }
    if let Feature::Derived {
        geometry: Some(g), ..
    } = &p.features[feature.0]
        && let Some(g) = p.derived_geometry.get(g.0)
        && g.kind == DerivedGeometryKind::MedianPlane
    {
        return vec![Element::Point(g.point.clone().map(|x| x.mm()))];
    }
    match &p.features[feature.0] {
        Feature::Items(items) => items
            .iter()
            .filter_map(|a| match a {
                Anchor::Face(f) => match part.faces.get(f.0)?.surface {
                    Surface::Plane { .. } => Some(Element::Plane(f.0)),
                    Surface::Cylinder { .. } | Surface::Cone { .. } | Surface::Torus { .. } => {
                        Some(Element::Axis(f.0))
                    }
                    _ => None,
                },
                Anchor::Edge(e) => edges
                    .get(e.0)
                    .copied()
                    .flatten()
                    .and_then(|i| part.edges.get(i))
                    .map(|e| Element::Point(edge_midpoint(e))),
                Anchor::Geometry(g) => {
                    let g = p.geometry.get(g.0)?;
                    crate::pmi::write::geometry_point(&g.item).map(|q| {
                        Element::Point(q.map(|x| {
                            x * g.length_unit.as_ref().map_or_else(
                                || part.file_units().map_or(1., |u| u.0),
                                LengthUnit::millimetres,
                            )
                        }))
                    })
                }
            })
            .collect(),
        Feature::Group { members, .. } | Feature::Derived { from: members, .. } => members
            .iter()
            .flat_map(|f| elements(part, edges, p, *f, depth + 1))
            .collect(),
    }
}
/// True for a straight distance or size, excluding angular and circular/radial notation.
pub fn is_linear(d: &Dimension) -> bool {
    super::dimension_type(d) == "linear dimension"
        && !matches!(
            d.kind,
            DimensionKind::Location {
                kind: LocationKind::CurvedDistance,
                ..
            } | DimensionKind::Size {
                kind: SizeKind::CurveLength,
                ..
            }
        )
}
/// Two real witness feet, the measured direction and an outward direction, in millimetres.
#[derive(Clone, Debug)]
pub struct Measurement {
    pub feet: [V3; 2],
    pub direction: V3,
    pub outward: V3,
}
impl Measurement {
    pub fn length(&self) -> f64 {
        geom::dot(geom::sub(self.feet[1], self.feet[0]), self.direction).abs()
    }
}
fn pair(
    part: &Part,
    caster: &RayCaster<'_>,
    a: &Element,
    b: &Element,
    nominal: f64,
) -> Option<(f64, Measurement)> {
    let delta = geom::sub(b.point(part), a.point(part));
    let mut dirs = Vec::new();
    dirs.extend(a.normal(part));
    dirs.extend(b.normal(part));
    dirs.extend(AXES);
    dirs.extend(geom::unit(delta));
    let mut best: Option<(f64, Measurement)> = None;
    for (priority, d) in dirs.into_iter().enumerate() {
        let d = canonical(d);
        if a.normal(part)
            .is_some_and(|n| geom::dot(n, d).abs() < 1. - 1e-8)
            || b.normal(part)
                .is_some_and(|n| geom::dot(n, d).abs() < 1. - 1e-8)
        {
            continue;
        }
        // An axis is a centreline, not a vertex on the cylindrical wall.
        if a.axis(part)
            .into_iter()
            .chain(b.axis(part))
            .any(|axis| geom::dot(axis, d).abs() > 1e-8)
        {
            continue;
        }
        let separation = geom::dot(delta, d).abs();
        if separation < 1e-9 {
            continue;
        }
        let score = (separation - nominal).abs() / nominal.max(1e-9) + priority as f64 * 1e-12;
        if best.as_ref().is_some_and(|(s, _)| *s <= score) {
            continue;
        }
        let views = if matches!((a, b), (Element::Point(_), Element::Point(_)))
            && geom::norm(geom::cross(d, delta)) > 1e-8
        {
            vec![canonical(geom::cross(d, delta))]
        } else if let Some(axis) = a.axis(part).or_else(|| b.axis(part)) {
            vec![axis]
        } else {
            let mut candidates = AXES;
            let mut extents = [0, 1, 2];
            extents.sort_by(|&a, &b| {
                (part.bounds().max[b] - part.bounds().min[b])
                    .total_cmp(&(part.bounds().max[a] - part.bounds().min[a]))
            });
            let rod = part.bounds().max_extent()
                > 3. * (part.bounds().max[extents[1]] - part.bounds().min[extents[1]]);
            candidates.sort_by(|x, y| {
                let bx = range(part.bounds(), *x);
                let by = range(part.bounds(), *y);
                if rod && geom::dot(d, AXES[extents[0]]).abs() > 1. - 1e-8 {
                    (by[1] - by[0]).total_cmp(&(bx[1] - bx[0]))
                } else {
                    (bx[1] - bx[0]).total_cmp(&(by[1] - by[0]))
                }
            });
            candidates
                .into_iter()
                .filter_map(|x| {
                    let p = geom::sub(x, geom::scale(d, geom::dot(x, d)));
                    (geom::norm(p) > 1e-8).then(|| canonical(p))
                })
                .collect::<Vec<_>>()
        };
        'view: for w in views {
            let w = canonical(w);
            let mut v = canonical(geom::cross(w, d));
            let centre = geom::scale(
                geom::add(a.bounds(part).centre(), b.bounds(part).centre()),
                0.5,
            );
            let axial_rod = (0..3).any(|k| {
                let transverse = (0..3)
                    .filter(|&j| j != k)
                    .map(|j| part.bounds().max[j] - part.bounds().min[j])
                    .fold(0., f64::max);
                geom::dot(d, AXES[k]).abs() > 1. - 1e-8
                    && part.bounds().max_extent() > 3. * transverse
            });
            // Shaft lengths sit above the part, clear of the retained datum/control frames
            // below it. Other distances use the nearer side of their measured elements.
            if !axial_rod && geom::dot(geom::sub(centre, part.bounds().centre()), v) < -1e-9 {
                v = geom::scale(v, -1.);
            }
            let ar = range(a.bounds(part), w);
            let br = range(b.bounds(part), w);
            let lo = ar[0].max(br[0]);
            let hi = ar[1].min(br[1]);
            if lo > hi + 1e-7 {
                continue;
            }
            // Bore witnesses meet the axis at the face/mouth. Other witnesses use a common
            // section through both trimmed faces, trying interior sections before boundaries.
            let fractions: &[f64] = if a.axis(part).is_some() || b.axis(part).is_some() {
                &[1., 0., 0.5]
            } else {
                &[0.5, 0.75, 0.25, 0.95, 0.05, 1., 0.]
            };
            for fraction in fractions {
                let level = lo + fraction * (hi - lo);
                if let (Some(pa), Some(pb)) = (
                    a.foot(part, caster, d, v, w, level),
                    b.foot(part, caster, d, v, w, level),
                ) {
                    best = Some((
                        score
                            + ((range(part.bounds(), v)[1] - geom::dot(pa, v))
                                + (range(part.bounds(), v)[1] - geom::dot(pb, v)))
                                / part.bounds().max_extent()
                                * 1e-9,
                        Measurement {
                            feet: [pa, pb],
                            direction: d,
                            outward: v,
                        },
                    ));
                    break 'view;
                }
            }
        }
    }
    best
}
// Exact boundary witnesses for elements without an analytic plane/axis description.
// They remain on the semantic targets; nominal values never manufacture geometry.
fn witnesses(
    part: &Part,
    edges: &[Option<usize>],
    p: &PartPmi,
    feature: FeatureId,
    depth: usize,
) -> Vec<Element> {
    if depth > p.features.len() {
        return Vec::new();
    }
    match &p.features[feature.0] {
        Feature::Items(items) => items
            .iter()
            .flat_map(|a| match a {
                Anchor::Face(f) => {
                    let face = &part.faces[f.0];
                    face.loops
                        .iter()
                        .flat_map(|lp| lp.edges.iter())
                        .flat_map(|&(e, _)| {
                            let edge = &part.edges[e];
                            [edge.start, edge.midpoint(), edge.end]
                        })
                        .take(32)
                        .map(Element::Point)
                        .collect::<Vec<_>>()
                }
                Anchor::Edge(e) => {
                    let Some(e) = edges
                        .get(e.0)
                        .copied()
                        .flatten()
                        .and_then(|i| part.edges.get(i))
                    else {
                        return Vec::new();
                    };
                    vec![
                        Element::Point(e.start),
                        Element::Point(edge_midpoint(e)),
                        Element::Point(e.end),
                    ]
                }
                Anchor::Geometry(_) => elements(part, edges, p, feature, depth + 1),
            })
            .take(32)
            .collect(),
        Feature::Group { members, .. } | Feature::Derived { from: members, .. } => members
            .iter()
            .flat_map(|f| witnesses(part, edges, p, *f, depth + 1))
            .take(32)
            .collect(),
    }
}
fn semantic_edges(part: &Part, def: &crate::step::PartDefinition) -> Vec<Option<usize>> {
    // PMI EdgeIndex indexes the definition's entity list, not loop traversal in the B-rep.
    // Reversed face loops can reverse first use of edges. Resolve by STEP source identity.
    let by_entity: std::collections::BTreeMap<_, _> = (0..part.edges.len())
        .filter_map(|i| part.edge_source(i).map(|s| (s.entity, i)))
        .collect();
    def.edges
        .iter()
        .map(|id| by_entity.get(id).copied())
        .collect()
}
/// Resolve the two semantic elements. Size aspects containing opposite faces are measured
/// between those faces; a single planar size face finds its opposite on the same part.
pub fn measurements(
    part: &Part,
    def: &crate::step::PartDefinition,
    p: &PartPmi,
) -> Vec<Option<Measurement>> {
    measurements_with_edges(part, p, &semantic_edges(part, def))
}
/// Resolve PMI edge indices from the same face/loop order as PartDefinition,
/// without reparsing geometry when generating an in-session presentation.
pub fn measurements_for_part(part: &Part, p: &PartPmi) -> Vec<Option<Measurement>> {
    let mut seen = std::collections::BTreeSet::new();
    let mut edges = Vec::new();
    for face in &part.faces {
        for boundary in &face.loops {
            for j in 0..boundary.edges.len() {
                let j = if face.reversed {
                    boundary.edges.len() - 1 - j
                } else {
                    j
                };
                let i = boundary.edges[j].0;
                let key = part.edge_source(i).map_or(i as u64, |s| s.entity);
                if seen.insert(key) {
                    edges.push(Some(i));
                }
            }
        }
    }
    measurements_with_edges(part, p, &edges)
}
fn measurements_with_edges(
    part: &Part,
    p: &PartPmi,
    edges: &[Option<usize>],
) -> Vec<Option<Measurement>> {
    let caster = RayCaster::for_faces(part, (0..part.faces.len()).collect());
    p.dimensions
        .iter()
        .map(|d| {
            if !is_linear(d) {
                return None;
            }
            let nominal = d.nominal.as_ref().map_or(0., Value::si).abs();
            if !nominal.is_finite() || nominal <= 1e-12 {
                return None;
            }
            let (a, b) = match d.kind {
                DimensionKind::Location { from, to, .. } => (
                    elements(part, edges, p, from, 0),
                    elements(part, edges, p, to, 0),
                ),
                DimensionKind::Size { feature, .. } => {
                    let a = elements(part, edges, p, feature, 0);
                    let b = if a.len() > 1 {
                        a.clone()
                    } else {
                        (0..part.faces.len())
                            .filter(|&f| matches!(part.faces[f].surface, Surface::Plane { .. }))
                            .map(Element::Plane)
                            .collect()
                    };
                    (a, b)
                }
            };
            let resolved = a
                .iter()
                .flat_map(|a| b.iter().filter_map(|b| pair(part, &caster, a, b, nominal)))
                .min_by(|a, b| a.0.total_cmp(&b.0));
            resolved
                .or_else(|| {
                    let (a, b) = match d.kind {
                        DimensionKind::Location { from, to, .. } => (
                            witnesses(part, edges, p, from, 0),
                            witnesses(part, edges, p, to, 0),
                        ),
                        DimensionKind::Size { feature, .. } => {
                            let a = witnesses(part, edges, p, feature, 0);
                            (a.clone(), a)
                        }
                    };
                    a.iter()
                        .flat_map(|a| b.iter().filter_map(|b| pair(part, &caster, a, b, nominal)))
                        .min_by(|a, b| a.0.total_cmp(&b.0))
                })
                .map(|(_, m)| m)
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct LinearPlaced {
    pub placed: Placed,
    pub feet: [V3; 2],
    pub dimension: [V3; 2],
    pub text_center: V3,
    pub text_bounds: Bounds,
    pub offset: f64,
    pub gap: f64,
}
/// Principal views and the four distinct isometric projections (opposites are identical).
pub const VIEWS: [V3; 7] = [
    [1., 0., 0.],
    [0., 1., 0.],
    [0., 0., 1.],
    [1., 1., 1.],
    [1., 1., -1.],
    [1., -1., 1.],
    [-1., 1., 1.],
];
fn rectangle(centre: V3, d: V3, v: V3, width: f64, height: f64) -> Vec<V3> {
    [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)]
        .map(|(x, y)| {
            geom::add(
                centre,
                geom::add(
                    geom::scale(d, x * width / 2.),
                    geom::scale(v, y * height / 2.),
                ),
            )
        })
        .to_vec()
}
/// Signed separating-axis clearance in a view, in model millimetres. Positive means
/// separated. Uses the actual planar rectangles, not an inflated 3D bounding box.
pub fn projected_clearance(a: &[V3], b: &[V3], view: V3) -> f64 {
    let Some(view) = geom::unit(view) else {
        return f64::NEG_INFINITY;
    };
    a.iter()
        .zip(a.iter().cycle().skip(1))
        .take(a.len())
        .chain(b.iter().zip(b.iter().cycle().skip(1)).take(b.len()))
        .filter_map(|(p, q)| geom::unit(geom::cross(view, geom::sub(*q, *p))))
        .map(|axis| {
            let interval = |points: &[V3]| {
                points
                    .iter()
                    .map(|p| geom::dot(*p, axis))
                    .fold([f64::INFINITY, f64::NEG_INFINITY], |r, t| {
                        [r[0].min(t), r[1].max(t)]
                    })
            };
            let x = interval(a);
            let y = interval(b);
            (x[0] - y[1]).max(y[0] - x[1])
        })
        .fold(f64::NEG_INFINITY, f64::max)
}
#[derive(Clone)]
struct Ink {
    normal: V3,
    polygons: [Vec<V3>; 2],
}
impl Ink {
    fn overlaps(&self, other: &Self, view_local: bool) -> bool {
        if view_local {
            return geom::dot(self.normal, other.normal).abs() > 1. - 1e-8
                && self.polygons.iter().any(|a| {
                    other
                        .polygons
                        .iter()
                        .any(|b| projected_clearance(a, b, self.normal) < 1e-8)
                });
        }
        VIEWS.into_iter().any(|view| {
            // Edge-on text has zero projected area and cannot be read in that view.
            geom::dot(self.normal, view).abs() > 1e-8
                && geom::dot(other.normal, view).abs() > 1e-8
                && self.polygons.iter().any(|a| {
                    other
                        .polygons
                        .iter()
                        .any(|b| projected_clearance(a, b, view) < 1e-8)
                })
        })
    }
}
/// Model-space ladder: shortest span first, ties in semantic order, then the first free
/// outward rung. This is Draftwright's nesting rule, without its sheet layout.
#[derive(Clone)]
pub struct Ladder {
    bounds: Bounds,
    groups: Vec<(V3, V3, usize)>,
    labels: Vec<Bounds>,
    fixed: Vec<(Bounds, V3)>,
    ink: Vec<Ink>,
    compact: bool,
    view_local: bool,
}
impl Ladder {
    pub fn new(bounds: Bounds) -> Self {
        Self {
            bounds,
            groups: Vec::new(),
            labels: Vec::new(),
            fixed: Vec::new(),
            ink: Vec::new(),
            compact: false,
            view_local: false,
        }
    }
    /// Fit-view ladder: start 0.6 capital heights outside the part and try
    /// 0.2-height stations against actual glyph ink rather than padded label cells.
    pub fn compact(bounds: Bounds) -> Self {
        Self {
            compact: true,
            ..Self::new(bounds)
        }
    }
    /// Compact packing for generated per-plane saved views. The writer uses
    /// `compact` and continues to check every readable principal/isometric view.
    #[cfg(feature = "json")]
    pub(crate) fn for_views(bounds: Bounds) -> Self {
        Self {
            view_local: true,
            ..Self::compact(bounds)
        }
    }
    pub fn reserve(&mut self, b: Bounds) {
        self.labels.push(b);
    }
    /// Retained flags and frames must clear the full dimension span in their
    /// parallel annotation plane, including the portion outside the lettering.
    pub fn reserve_annotation(&mut self, bounds: Bounds, normal: V3) {
        if !bounds.max_extent().is_finite() {
            return;
        }
        self.fixed.push((bounds, normal));
        self.reserve(bounds);
    }
    /// Reserve circular/angular dimension text in its real annotation plane.
    pub fn reserve_dimension(&mut self, bounds: Bounds, normal: V3, along: V3) {
        if !bounds.max_extent().is_finite() {
            return;
        }
        let up = geom::cross(normal, along);
        let x = range(bounds, along);
        let y = range(bounds, up);
        let polygon = rectangle(bounds.centre(), along, up, x[1] - x[0], y[1] - y[0]);
        self.ink.push(Ink {
            normal,
            polygons: [polygon.clone(), polygon],
        });
        self.reserve(bounds);
    }
    pub fn draw(&mut self, m: &Measurement, c: &Callout) -> LinearPlaced {
        self.draw_in_view(m, c, None)
    }
    pub(crate) fn draw_in_view(
        &mut self,
        m: &Measurement,
        c: &Callout,
        camera_normal: Option<V3>,
    ) -> LinearPlaced {
        let d = m.direction;
        let v = m.outward;
        // Front XZ reads from -Y; top XY from +Z; side YZ from +X.
        // Text orientation is independent of which side the extensions run toward.
        let mut normal = canonical(geom::cross(d, v));
        if geom::dominant_axis_preferring_z(normal) == 1 {
            normal = geom::scale(normal, -1.);
        }
        normal = camera_normal.unwrap_or(normal);
        let up = geom::cross(normal, d);
        // A ladder's plane is determined by the part, never by the individual witness
        // section. Thus top-face axes and mid-thickness face witnesses share one plane.
        let plane = range(self.bounds, normal)[1];
        let project = |p| geom::add(p, geom::scale(normal, plane - geom::dot(p, normal)));
        let length = m.length();
        let h = super::placement::text_height(self.bounds.max_extent());
        let scale = (h / 3.).min(length * 0.65 / c.size[0].max(1.));
        let arrow = (0.9 * h).min(length / 8.);
        let half = arrow / 6.;
        let gap = h / 3.;
        let overshoot = 2. * h / 3.;
        let edge = range(self.bounds, v)[1];
        let mut local_ink = Bounds::empty();
        for p in c.polylines().iter().flatten() {
            local_ink.add([p[0], p[1], 0.]);
        }
        let ink_size = if self.compact {
            [
                local_ink.max[0] - local_ink.min[0],
                local_ink.max[1] - local_ink.min[1],
            ]
        } else {
            c.size
        };
        let group = self
            .groups
            .iter()
            .position(|(u, w, _)| geom::dot(*u, d) > 1. - 1e-8 && geom::dot(*w, v) > 1. - 1e-8);
        let mut rung = group.map_or(0, |i| self.groups[i].2);
        let (centre, offset, label_bounds, ink) = loop {
            let offset = if self.compact {
                (0.6 + rung as f64 * 0.2) * h
            } else {
                (10. + rung as f64 * 7.) * h / 3.
            };
            let level = edge + offset;
            let a = project(geom::add(
                m.feet[0],
                geom::scale(v, level - geom::dot(m.feet[0], v)),
            ));
            let b = project(geom::add(
                m.feet[1],
                geom::scale(v, level - geom::dot(m.feet[1], v)),
            ));
            let centre = geom::scale(geom::add(a, b), 0.5);
            let mut bb = Bounds::empty();
            for x in [-ink_size[0] / 2., ink_size[0] / 2.] {
                for y in [-ink_size[1] / 2., ink_size[1] / 2.] {
                    bb.add(geom::add(
                        centre,
                        geom::add(geom::scale(d, x * scale), geom::scale(v, y * scale)),
                    ));
                }
            }
            let padding = h / 5.;
            let ink = Ink {
                normal,
                polygons: [
                    rectangle(
                        centre,
                        d,
                        v,
                        ink_size[0] * scale + padding,
                        ink_size[1] * scale + padding,
                    ),
                    rectangle(centre, d, v, length, h / 10.),
                ],
            };
            let overlap = !self.view_local
                && self
                    .labels
                    .iter()
                    .any(|b| super::placement::boxes_overlap(bb, *b, padding));
            let crosses_frame = self.fixed.iter().any(|(b, n)| {
                if geom::dot(normal, *n).abs() <= 1. - 1e-8 {
                    return false;
                }
                let x = range(*b, d);
                let y = range(*b, v);
                let frame = rectangle(b.centre(), d, v, x[1] - x[0], y[1] - y[0]);
                projected_clearance(&ink.polygons[1], &frame, normal) < 1e-8
            });
            rung += 1;
            if !overlap
                && !crosses_frame
                && !self
                    .ink
                    .iter()
                    .any(|other| ink.overlaps(other, self.view_local))
            {
                break (centre, offset, bb, ink);
            }
        };
        if let Some(i) = group {
            self.groups[i].2 = rung;
        } else {
            self.groups.push((d, v, rung));
        }
        self.labels.push(label_bounds);
        self.ink.push(ink);
        let level = edge + offset;
        let mut ends = m
            .feet
            .map(|p| project(geom::add(p, geom::scale(v, level - geom::dot(p, v)))));
        if geom::dot(geom::sub(ends[1], ends[0]), d) < 0. {
            ends.swap(0, 1);
        }
        let half_text = c.size[0] * scale / 2. + gap.min(length * 0.02);
        let mut lines = Vec::new();
        for foot in m.feet {
            lines.push(vec![
                geom::add(foot, geom::scale(v, gap)),
                project(geom::add(foot, geom::scale(v, gap))),
                project(geom::add(
                    foot,
                    geom::scale(v, level + overshoot - geom::dot(foot, v)),
                )),
            ]);
        }
        // The dimension line has a text break; its arrow-to-arrow span is the measurement.
        lines.push(vec![ends[0], geom::sub(centre, geom::scale(d, half_text))]);
        lines.push(vec![geom::add(centre, geom::scale(d, half_text)), ends[1]]);
        for (tip, sign) in [(ends[0], 1.), (ends[1], -1.)] {
            let tail = geom::add(tip, geom::scale(d, sign * arrow));
            let a = geom::add(tail, geom::scale(v, half));
            let b = geom::sub(tail, geom::scale(v, half));
            lines.extend([vec![tip, a], vec![a, b], vec![b, tip]]);
        }
        lines.extend(c.polylines().into_iter().map(|l| {
            l.into_iter()
                .map(|p| {
                    geom::add(
                        centre,
                        geom::add(
                            geom::scale(d, (p[0] - c.size[0] / 2.) * scale),
                            geom::scale(up, (p[1] - c.size[1] / 2.) * scale),
                        ),
                    )
                })
                .collect()
        }));
        LinearPlaced {
            placed: Placed {
                text_origin: geom::sub(
                    centre,
                    geom::add(
                        geom::scale(d, c.size[0] * scale / 2.),
                        geom::scale(up, c.size[1] * scale / 2.),
                    ),
                ),
                text_scale: scale,
                anchor: m.feet[0],
                origin: centre,
                normal,
                along: d,
                lines,
            },
            feet: m.feet,
            dimension: ends,
            text_center: centre,
            text_bounds: label_bounds,
            offset,
            gap,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn different_witness_sections_share_one_outer_ladder_plane() {
        let bounds = Bounds {
            min: [-50., -30., -5.],
            max: [50., 30., 5.],
        };
        let mut ladder = Ladder::new(bounds);
        let measurements = [
            Measurement {
                feet: [[-45., 0., 5.], [45., 0., 5.]],
                direction: AXES[0],
                outward: AXES[1],
            },
            Measurement {
                feet: [[-50., 0., 0.], [50., 0., 0.]],
                direction: AXES[0],
                outward: AXES[1],
            },
        ];
        let short = ladder.draw(&measurements[0], &super::super::label("90"));
        let long = ladder.draw(&measurements[1], &super::super::label("100"));
        assert!(long.offset > short.offset);
        assert_eq!(short.placed.normal, long.placed.normal);
        assert_eq!(short.dimension.map(|p| p[2]), [5.; 2]);
        assert_eq!(long.dimension.map(|p| p[2]), [5.; 2]);
        assert_eq!(long.feet, measurements[1].feet);
        for view in VIEWS {
            if geom::dot(short.placed.normal, view).abs() > 1e-8 {
                let a = &ladder.ink[0];
                let b = &ladder.ink[1];
                for x in &a.polygons {
                    for y in &b.polygons {
                        assert!(projected_clearance(x, y, view) > 0.);
                    }
                }
            }
        }
    }
    #[test]
    fn nist_primary_and_boundary_edge_witnesses_use_the_same_semantic_edge() {
        use std::io::Read;
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/ap242/specify/nist_ctc_01_merge.step.gz");
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap())
            .read_to_end(&mut bytes)
            .unwrap();
        let def = &crate::step::read_part_definitions(&bytes).unwrap()[0];
        let part = crate::step::read_part(&bytes, def).unwrap();
        let edges = semantic_edges(&part, def);
        let p = PartPmi {
            features: (0..def.edges.len())
                .map(|i| Feature::Items(vec![Anchor::Edge(EdgeIndex(i))]))
                .collect(),
            ..PartPmi::default()
        };
        assert_eq!(def.edges.len(), 318);
        for (i, &id) in def.edges.iter().enumerate() {
            let actual = (0..part.edges.len())
                .find(|&e| part.edge_source(e).is_some_and(|s| s.entity == id))
                .unwrap();
            let e = &part.edges[actual];
            let mid = e.curve.value(e.curve.parameter(e.midpoint()));
            assert_eq!(
                elements(&part, &edges, &p, FeatureId(i), 0)[0].point(&part),
                mid
            );
            let boundary: Vec<_> = witnesses(&part, &edges, &p, FeatureId(i), 0)
                .iter()
                .map(|e| e.point(&part))
                .collect();
            assert_eq!(boundary, [e.start, mid, e.end], "semantic edge #{id}");
        }
    }
    #[test]
    fn supplemental_points_respect_context_units_and_explicit_overrides() {
        let part = Part::new(vec![], vec![], vec![]).with_file_pcurves(
            std::collections::BTreeMap::new(),
            (25.4, 1.),
            None,
        );
        let mut p = PartPmi::default();
        p.geometry.push(Geometry {
            item: GeometryItem {
                leaves: vec![(
                    "cartesian_point".into(),
                    vec![
                        GeometryValue::Text(String::new()),
                        GeometryValue::List(
                            ["1.", "2.", "3."]
                                .map(|n| GeometryValue::Real(Decimal::parse(n).unwrap()))
                                .to_vec(),
                        ),
                    ],
                )],
            },
            length_unit: None,
        });
        p.features
            .push(Feature::Items(vec![Anchor::Geometry(GeometryId(0))]));
        let point = elements(&part, &[], &p, FeatureId(0), 0)[0].point(&part);
        assert!(geom::dist(point, [25.4, 50.8, 76.2]) < 1e-12);
        p.geometry[0].length_unit = Some(LengthUnit::Millimetre);
        assert_eq!(
            elements(&part, &[], &p, FeatureId(0), 0)[0].point(&part),
            [1., 2., 3.]
        );
    }
    #[test]
    fn both_sides_read_from_the_same_front_or_top_view() {
        for (d, v, normal) in [
            ([1., 0., 0.], [0., 0., 1.], [0., -1., 0.]),
            ([1., 0., 0.], [0., 1., 0.], [0., 0., 1.]),
        ] {
            for sign in [-1., 1.] {
                let m = Measurement {
                    feet: [[0.; 3], geom::scale(d, 10.)],
                    direction: d,
                    outward: geom::scale(v, sign),
                };
                let p = Ladder::new(Bounds {
                    min: [-10.; 3],
                    max: [10.; 3],
                })
                .draw(&m, &super::super::label("10"));
                assert!(geom::dist(p.placed.normal, normal) < 1e-12);
                assert_eq!(p.placed.along, d);
            }
        }
    }
    #[test]
    fn arrow_triangles_and_text_keep_their_frame_in_a_rotated_plane() {
        let d = canonical([1., 2., 3.]);
        let v = canonical(geom::cross(d, [0., 1., 0.]));
        let m = Measurement {
            feet: [
                [1., 2., 3.],
                geom::add(
                    [1., 2., 3.],
                    geom::add(geom::scale(d, 12.), geom::scale(v, 2.)),
                ),
            ],
            direction: d,
            outward: v,
        };
        let mut ladder = Ladder::new(Bounds {
            min: [-10.; 3],
            max: [10.; 3],
        });
        let c = super::super::label("12 ±0.1");
        let p = ladder.draw(&m, &c);
        assert!((geom::dist(p.dimension[0], p.dimension[1]) - 12.).abs() < 1e-12);
        assert!(
            geom::dist(
                p.text_center,
                geom::scale(geom::add(p.dimension[0], p.dimension[1]), 0.5)
            ) < 1e-12
        );
        assert!(
            (geom::dot(
                p.placed.normal,
                geom::cross(d, geom::cross(p.placed.normal, d))
            ) - 1.)
                .abs()
                < 1e-12
        );
        for q in p.placed.lines.iter().skip(2).flatten() {
            assert!(geom::dot(geom::sub(*q, p.text_center), p.placed.normal).abs() < 1e-12);
        }
        // Ported Draftwright head: length 0.9 text heights, flare half-width = length/6.
        let tip = p.placed.lines[4][0];
        let a = p.placed.lines[4][1];
        let b = p.placed.lines[5][1];
        let tail = geom::scale(geom::add(a, b), 0.5);
        assert!((geom::dist(a, b) / geom::dist(tip, tail) - 1. / 3.).abs() < 1e-12);
        let up = geom::cross(p.placed.normal, d);
        let scale = (super::super::placement::text_height(20.) / 3.).min(12. * 0.65 / c.size[0]);
        let expected: Vec<_> = c
            .polylines()
            .into_iter()
            .flatten()
            .map(|q| {
                [
                    (q[0] - c.size[0] / 2.) * scale,
                    (q[1] - c.size[1] / 2.) * scale,
                ]
            })
            .collect();
        for (q, e) in p.placed.lines.iter().skip(10).flatten().zip(expected) {
            let delta = geom::sub(*q, p.text_center);
            assert!(
                (geom::dot(delta, d) - e[0]).abs() < 1e-12
                    && (geom::dot(delta, up) - e[1]).abs() < 1e-12
            );
        }
    }
}
