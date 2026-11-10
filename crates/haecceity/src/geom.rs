//! Exact analytic geometry: vectors, placements, surfaces and edge curves.
//!
//! Surface parametrisations follow OpenCascade's conventions (`gp_Cylinder`, `gp_Torus`, ...)
//! so that a parameter range read here means what `BRepAdaptor_Surface` means in the Python
//! implementation.

use std::f64::consts::{PI, TAU};

use super::nurbs::{NurbsCurve, NurbsSurface};
use super::poly;

pub type V3 = [f64; 3];

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn scale(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
pub fn unit(a: V3) -> Option<V3> {
    let n = norm(a);
    (n.is_finite() && n > 1e-300).then(|| scale(a, 1.0 / n))
}
pub fn dist(a: V3, b: V3) -> f64 {
    norm(sub(a, b))
}

/// An axis-aligned box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min: V3,
    pub max: V3,
}

impl Bounds {
    pub fn empty() -> Self {
        Bounds {
            min: [f64::INFINITY; 3],
            max: [f64::NEG_INFINITY; 3],
        }
    }
    pub fn add(&mut self, p: V3) {
        for (i, x) in p.into_iter().enumerate() {
            self.min[i] = self.min[i].min(x);
            self.max[i] = self.max[i].max(x);
        }
    }
    pub fn merge(&mut self, other: &Bounds) {
        self.add(other.min);
        self.add(other.max);
    }
    pub fn centre(&self) -> V3 {
        [
            0.5 * (self.min[0] + self.max[0]),
            0.5 * (self.min[1] + self.max[1]),
            0.5 * (self.min[2] + self.max[2]),
        ]
    }
    pub fn max_extent(&self) -> f64 {
        (0..3)
            .map(|i| self.max[i] - self.min[i])
            .fold(f64::NEG_INFINITY, f64::max)
    }
    /// Whether *p* lies within the box grown by *pad*.
    pub fn contains(&self, p: V3, pad: f64) -> bool {
        (0..3).all(|i| p[i] >= self.min[i] - pad && p[i] <= self.max[i] + pad)
    }
    pub fn diagonal(&self) -> f64 {
        dist(self.min, self.max)
    }
}

/// The index of the largest component, the first on a tie (Python's
/// `max(range(3), key=...)`).
pub fn dominant_axis(v: V3) -> usize {
    let mut best = 0;
    for i in 1..3 {
        if v[i] > v[best] {
            best = i;
        }
    }
    best
}

/// The dominant axis of a direction, preferring z then y on numerical ties
/// (`_geometry._axis_letter_of`): a discrete routing choice must not flip on a last-bit tie.
pub fn dominant_axis_preferring_z(v: V3) -> usize {
    let c = v.map(f64::abs);
    let peak = c[0].max(c[1]).max(c[2]);
    (0..3)
        .rev()
        .find(|&i| peak - c[i] <= 1e-12)
        // Only a direction with no finite component has no peak.
        .unwrap_or(2)
}

/// The x axis `gp_Ax2(P, V)` chooses for a z axis given without a reference direction.
pub fn default_x_axis(z: V3) -> V3 {
    let [a, b, c] = z;
    let (aa, ba, ca) = (a.abs(), b.abs(), c.abs());
    let d = if ba <= aa && ba <= ca {
        if aa > ca { [-c, 0.0, a] } else { [c, 0.0, -a] }
    } else if aa <= ba && aa <= ca {
        if ba > ca { [0.0, -c, b] } else { [0.0, c, -b] }
    } else if aa > ba {
        [-b, a, 0.0]
    } else {
        [b, -a, 0.0]
    };
    // A unit z gives a non-zero x; only a degenerate z falls back to the x axis.
    unit(d).unwrap_or([1.0, 0.0, 0.0])
}

/// A right-handed placement: origin plus three unit axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub origin: V3,
    pub x: V3,
    pub y: V3,
    pub z: V3,
}

impl Frame {
    pub fn to_local(&self, p: V3) -> V3 {
        let d = sub(p, self.origin);
        [dot(d, self.x), dot(d, self.y), dot(d, self.z)]
    }
    pub fn dir_to_world(&self, l: V3) -> V3 {
        add(
            scale(self.x, l[0]),
            add(scale(self.y, l[1]), scale(self.z, l[2])),
        )
    }
    pub fn dir_to_local(&self, d: V3) -> V3 {
        [dot(d, self.x), dot(d, self.y), dot(d, self.z)]
    }
    pub fn to_world(&self, l: V3) -> V3 {
        add(
            self.origin,
            add(
                scale(self.x, l[0]),
                add(scale(self.y, l[1]), scale(self.z, l[2])),
            ),
        )
    }
    /// `gp_Ax3::Direct()`: whether the frame is right-handed.
    pub fn direct(&self) -> bool {
        dot(cross(self.x, self.y), self.z) > 0.0
    }
}

/// The analytic surface kinds the recognisers read. Anything else is carried by name so a
/// caller can refuse it explicitly rather than mistake it for a supported surface.
#[derive(Clone, Debug)]
pub enum Surface {
    Plane {
        frame: Frame,
    },
    Cylinder {
        frame: Frame,
        radius: f64,
    },
    Cone {
        frame: Frame,
        radius: f64,
        semi_angle: f64,
    },
    Sphere {
        frame: Frame,
        radius: f64,
    },
    Torus {
        frame: Frame,
        major: f64,
        minor: f64,
    },
    /// Any other surface, in exact NURBS form; *kind* names what the file called it.
    Freeform {
        kind: &'static str,
        surface: Box<NurbsSurface>,
    },
    /// A surface that could not be resolved at all.
    Other {
        kind: &'static str,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceType {
    Plane,
    Cylinder,
    Cone,
    Sphere,
    Torus,
    Freeform,
    Other,
}

impl Surface {
    pub fn kind(&self) -> SurfaceType {
        match self {
            Surface::Plane { .. } => SurfaceType::Plane,
            Surface::Cylinder { .. } => SurfaceType::Cylinder,
            Surface::Cone { .. } => SurfaceType::Cone,
            Surface::Sphere { .. } => SurfaceType::Sphere,
            Surface::Torus { .. } => SurfaceType::Torus,
            Surface::Freeform { .. } => SurfaceType::Freeform,
            Surface::Other { .. } => SurfaceType::Other,
        }
    }

    /// (u periodic, v periodic) — both with period 2π where periodic.
    pub fn periodic(&self) -> (bool, bool) {
        match self {
            Surface::Plane { .. } | Surface::Freeform { .. } | Surface::Other { .. } => {
                (false, false)
            }
            Surface::Cylinder { .. } | Surface::Cone { .. } | Surface::Sphere { .. } => {
                (true, false)
            }
            Surface::Torus { .. } => (true, true),
        }
    }

    /// The first partial derivatives (Su, Sv) at (u, v). Their cross product is the surface's
    /// natural normal (outward for the closed analytic kinds).
    pub fn partials(&self, u: f64, v: f64) -> (V3, V3) {
        let (cu, su) = (u.cos(), u.sin());
        match *self {
            Surface::Plane { frame } => (frame.x, frame.y),
            Surface::Cylinder { frame, radius } => (
                frame.dir_to_world([-radius * su, radius * cu, 0.0]),
                frame.z,
            ),
            Surface::Cone {
                frame,
                radius,
                semi_angle,
            } => {
                let rho = radius + v * semi_angle.sin();
                (
                    frame.dir_to_world([-rho * su, rho * cu, 0.0]),
                    frame.dir_to_world([
                        semi_angle.sin() * cu,
                        semi_angle.sin() * su,
                        semi_angle.cos(),
                    ]),
                )
            }
            Surface::Sphere { frame, radius } => (
                frame.dir_to_world([-radius * v.cos() * su, radius * v.cos() * cu, 0.0]),
                frame.dir_to_world([
                    -radius * v.sin() * cu,
                    -radius * v.sin() * su,
                    radius * v.cos(),
                ]),
            ),
            Surface::Torus {
                frame,
                major,
                minor,
            } => {
                let rho = major + minor * v.cos();
                (
                    frame.dir_to_world([-rho * su, rho * cu, 0.0]),
                    frame.dir_to_world([
                        -minor * v.sin() * cu,
                        -minor * v.sin() * su,
                        minor * v.cos(),
                    ]),
                )
            }
            Surface::Freeform { ref surface, .. } => {
                let (_, du, dv) = surface.value_and_partials(u, v);
                (du, dv)
            }
            Surface::Other { .. } => ([f64::NAN; 3], [f64::NAN; 3]),
        }
    }

    /// The unit natural normal at (u, v), `None` at a singular point.
    pub fn normal(&self, u: f64, v: f64) -> Option<V3> {
        let (du, dv) = self.partials(u, v);
        unit(cross(du, dv))
    }

    /// A cone's apex (`gp_Cone::Apex`).
    pub fn cone_apex(&self) -> Option<V3> {
        let Surface::Cone {
            frame,
            radius,
            semi_angle,
        } = *self
        else {
            return None;
        };
        Some(add(
            frame.origin,
            scale(frame.z, -radius / semi_angle.tan()),
        ))
    }

    pub fn value(&self, u: f64, v: f64) -> V3 {
        match *self {
            Surface::Plane { frame } => frame.to_world([u, v, 0.0]),
            Surface::Cylinder { frame, radius } => {
                frame.to_world([radius * u.cos(), radius * u.sin(), v])
            }
            Surface::Cone {
                frame,
                radius,
                semi_angle,
            } => {
                let rho = radius + v * semi_angle.sin();
                frame.to_world([rho * u.cos(), rho * u.sin(), v * semi_angle.cos()])
            }
            Surface::Sphere { frame, radius } => frame.to_world([
                radius * v.cos() * u.cos(),
                radius * v.cos() * u.sin(),
                radius * v.sin(),
            ]),
            Surface::Torus {
                frame,
                major,
                minor,
            } => {
                let rho = major + minor * v.cos();
                frame.to_world([rho * u.cos(), rho * u.sin(), minor * v.sin()])
            }
            Surface::Freeform { ref surface, .. } => surface.value(u, v),
            Surface::Other { .. } => [f64::NAN; 3],
        }
    }

    /// The point of an analytic surface (plane, cylinder, cone, sphere, torus) nearest *p*, by
    /// its closed-form inversion; `None` on the others.
    pub fn foot(&self, p: V3) -> Option<V3> {
        match self {
            Surface::Freeform { .. } | Surface::Other { .. } => None,
            _ => self.parameters(p, None).map(|(u, v)| self.value(u, v)),
        }
    }

    /// The (u, v) parameters of a point on (or near) the surface. Periodic parameters are
    /// returned in `[0, 2π)`; callers unwrap them. *hint* seeds the iterative inversion of a
    /// freeform surface (the analytic kinds invert in closed form and ignore it).
    pub fn parameters(&self, p: V3, hint: Option<(f64, f64)>) -> Option<(f64, f64)> {
        let angle = |l: V3| l[1].atan2(l[0]).rem_euclid(TAU);
        match *self {
            Surface::Plane { frame } => {
                let l = frame.to_local(p);
                Some((l[0], l[1]))
            }
            Surface::Cylinder { frame, .. } => {
                let l = frame.to_local(p);
                Some((angle(l), l[2]))
            }
            Surface::Cone {
                frame,
                radius,
                semi_angle,
            } => {
                let l = frame.to_local(p);
                let rho = l[0].hypot(l[1]);
                Some((
                    angle(l),
                    (rho - radius) * semi_angle.sin() + l[2] * semi_angle.cos(),
                ))
            }
            Surface::Sphere { frame, .. } => {
                let l = frame.to_local(p);
                let rho = l[0].hypot(l[1]);
                Some((angle(l), l[2].atan2(rho)))
            }
            Surface::Torus { frame, major, .. } => {
                let l = frame.to_local(p);
                let rho = l[0].hypot(l[1]);
                Some((angle(l), l[2].atan2(rho - major).rem_euclid(TAU)))
            }
            Surface::Freeform { ref surface, .. } => Some(surface.invert(p, hint)),
            Surface::Other { .. } => None,
        }
    }

    /// The v of the singular line near (·, v) — a sphere's pole or a cone's apex, where u is
    /// undefined and a boundary through the point runs along the whole u line — if any.
    pub fn singular_v(&self, v: f64) -> Option<f64> {
        match *self {
            // Within a degree of the pole the angle swings too fast for boundary samples to
            // follow; route anything that close along the pole line.
            Surface::Sphere { .. } if (v.abs() - PI / 2.0).abs() < 0.02 => {
                Some(v.signum() * PI / 2.0)
            }
            Surface::Cone {
                radius, semi_angle, ..
            } if (radius + v * semi_angle.sin()).abs() < 1e-7 * (1.0 + radius) => {
                Some(-radius / semi_angle.sin())
            }
            _ => None,
        }
    }

    /// Parameters of the points where a doubly-curved surface is extreme along each of the
    /// unit directions *dirs* (both senses).
    pub fn extreme_parameters_along(&self, dirs: &[V3]) -> Vec<(f64, f64)> {
        let mut out = Vec::new();
        match *self {
            Surface::Sphere { frame, .. } => {
                for &a in dirs {
                    for sign in [1.0, -1.0] {
                        let w = frame.dir_to_local(scale(a, sign));
                        out.push((
                            w[1].atan2(w[0]).rem_euclid(TAU),
                            w[2].clamp(-1.0, 1.0).asin(),
                        ));
                    }
                }
            }
            Surface::Torus { frame, .. } => {
                for &a in dirs {
                    let w = frame.dir_to_local(a);
                    if w[0].hypot(w[1]) < 1e-12 {
                        // Along the axis the extremes are whole circles at the tube's top and bottom.
                        for k in 0..16 {
                            let u = TAU * k as f64 / 16.0;
                            out.push((u, PI / 2.0));
                            out.push((u, 3.0 * PI / 2.0));
                        }
                        continue;
                    }
                    for u in [w[1].atan2(w[0]), w[1].atan2(w[0]) + PI] {
                        let v = w[2].atan2(w[0] * u.cos() + w[1] * u.sin());
                        out.push((u.rem_euclid(TAU), v.rem_euclid(TAU)));
                        out.push((u.rem_euclid(TAU), (v + PI).rem_euclid(TAU)));
                    }
                }
            }
            _ => {}
        }
        out
    }

    /// Ray parameters `t > 0` where `origin + t * dir` meets the untrimmed surface, plus a flag
    /// that is set when a hit is tangential (and so cannot be counted reliably).
    pub fn ray_hits(&self, origin: V3, dir: V3, t_max: f64) -> Option<(Vec<f64>, bool)> {
        let mut hits = Vec::new();
        let mut grazing = false;
        let mut quadratic = |a: f64, b: f64, c: f64| {
            if a.abs() < 1e-300 {
                if b.abs() > 1e-300 {
                    hits.push(-c / b);
                }
                return;
            }
            let disc = b * b - 4.0 * a * c;
            let scale_ = (b * b).max((4.0 * a * c).abs()).max(1e-300);
            if disc.abs() <= 1e-12 * scale_ {
                grazing = true;
                hits.push(-b / (2.0 * a));
            } else if disc > 0.0 {
                let s = disc.sqrt();
                let q = -0.5 * (b + b.signum() * s);
                hits.push(q / a);
                if q.abs() > 1e-300 {
                    hits.push(c / q);
                }
            }
        };
        match *self {
            Surface::Plane { frame } => {
                let denom = dot(dir, frame.z);
                if denom.abs() < 1e-12 {
                    return Some((vec![], dot(sub(origin, frame.origin), frame.z).abs() < 1e-9));
                }
                hits.push(dot(sub(frame.origin, origin), frame.z) / denom);
            }
            Surface::Cylinder { frame, radius } => {
                let o = frame.to_local(origin);
                let d = frame.dir_to_local(dir);
                quadratic(
                    d[0] * d[0] + d[1] * d[1],
                    2.0 * (o[0] * d[0] + o[1] * d[1]),
                    o[0] * o[0] + o[1] * o[1] - radius * radius,
                );
            }
            Surface::Cone {
                frame,
                radius,
                semi_angle,
            } => {
                let o = frame.to_local(origin);
                let d = frame.dir_to_local(dir);
                let k = semi_angle.tan();
                // x² + y² = (r + z k)²
                let (rz0, rz1) = (radius + o[2] * k, d[2] * k);
                quadratic(
                    d[0] * d[0] + d[1] * d[1] - rz1 * rz1,
                    2.0 * (o[0] * d[0] + o[1] * d[1] - rz0 * rz1),
                    o[0] * o[0] + o[1] * o[1] - rz0 * rz0,
                );
                // Only the nappe the parametrisation covers (non-negative radius).
                hits.retain(|t| radius + (o[2] + t * d[2]) * k >= -1e-12);
            }
            Surface::Sphere { frame, radius } => {
                let o = sub(origin, frame.origin);
                quadratic(
                    dot(dir, dir),
                    2.0 * dot(o, dir),
                    dot(o, o) - radius * radius,
                );
            }
            Surface::Torus {
                frame,
                major,
                minor,
            } => {
                let o = frame.to_local(origin);
                let d = frame.dir_to_local(dir);
                // Within the bounding sphere only, and measured from where the ray enters it, so
                // the quartic's coefficients stay well conditioned however far away the origin is.
                let reach = major + minor;
                let b = dot(o, d) / dot(d, d);
                let disc = b * b - (dot(o, o) - reach * reach) / dot(d, d);
                if disc > 0.0 {
                    let (t0, t1) = ((-b - disc.sqrt()).max(0.0), (-b + disc.sqrt()).min(t_max));
                    if t1 > t0 {
                        let e = add(o, scale(d, t0));
                        // (|p|² + R² − r²)² − 4R²(px² + py²) along p = e + s d.
                        let q = [
                            dot(e, e) + major * major - minor * minor,
                            2.0 * dot(e, d),
                            dot(d, d),
                        ];
                        let w = [
                            e[0] * e[0] + e[1] * e[1],
                            2.0 * (e[0] * d[0] + e[1] * d[1]),
                            d[0] * d[0] + d[1] * d[1],
                        ];
                        let r2 = 4.0 * major * major;
                        let c = [
                            q[0] * q[0] - r2 * w[0],
                            2.0 * q[0] * q[1] - r2 * w[1],
                            q[1] * q[1] + 2.0 * q[0] * q[2] - r2 * w[2],
                            2.0 * q[1] * q[2],
                            q[2] * q[2],
                        ];
                        for root in poly::roots(&c, 0.0, t1 - t0) {
                            grazing |= root.touching;
                            hits.push(t0 + root.x);
                        }
                    }
                }
            }
            Surface::Freeform { ref surface, .. } => {
                let (found, graze) = surface.ray_hits(origin, dir, t_max);
                grazing |= graze;
                hits.extend(found.into_iter().map(|h| h.0));
            }
            Surface::Other { .. } => return None,
        }
        hits.retain(|t| *t > 1e-12 && *t <= t_max);
        Some((hits, grazing))
    }
}

/// The unbounded conic retained behind a reader's trimmed rational-quadratic edge.
/// Re-derive its poles after placement, using the reader's endpoint inversion arithmetic.
#[derive(Clone, Debug)]
pub(super) struct Hyperbola {
    pub(super) frame: Frame,
    pub(super) major: f64,
    pub(super) minor: f64,
}

impl Hyperbola {
    pub(super) fn arc(&self, a: V3, b: V3) -> Option<Curve> {
        let (frame, major, minor) = (&self.frame, self.major, self.minor);
        // ISO 10303-42: C + major·cosh(t)·x + minor·sinh(t)·y, the branch on +x only.
        let on = |t: f64, w: f64| {
            add(
                frame.origin,
                frame.dir_to_world([major * t.cosh() / w, minor * t.sinh() / w, 0.0]),
            )
        };
        let param = |p: V3| {
            let t = (frame.to_local(p)[1] / minor).asinh();
            (dist(on(t, 1.0), p) <= 1e-4).then_some(t)
        };
        let (ta, tb) = (param(a)?, param(b)?);
        let (t0, t1) = (ta.min(tb), ta.max(tb));
        if t1 - t0 < 1e-9 {
            return None;
        }
        // The conic arc's middle pole is where the end tangents meet, weighted cosh of the
        // half-span (the hyperbolic counterpart of a circular arc's cos of half its angle).
        let half = 0.5 * (t1 - t0);
        let w = half.cosh();
        let points = vec![on(t0, 1.0), on(0.5 * (t0 + t1), w), on(t1, 1.0)];
        let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        NurbsCurve::new(2, points, vec![1.0, w, 1.0], knots).map(Curve::Nurbs)
    }
}

/// Analytic conics behind exact edge-bounded quadratic B-splines. Retaining the native
/// frame lets rigid transformations reconstruct poles with the reader's arithmetic.
#[derive(Clone, Debug)]
pub(super) enum Conic {
    Hyperbola(Hyperbola),
    /// ISO 10303-42: (F t², 2F t, 0), with focal distance F.
    Parabola {
        frame: Frame,
        focal_dist: f64,
    },
}

impl Conic {
    pub(super) fn frame_mut(&mut self) -> &mut Frame {
        match self {
            Self::Hyperbola(h) => &mut h.frame,
            Self::Parabola { frame, .. } => frame,
        }
    }

    pub(super) fn arc(&self, a: V3, b: V3) -> Option<Curve> {
        match self {
            Self::Hyperbola(h) => h.arc(a, b),
            Self::Parabola {
                frame,
                focal_dist: f,
            } => {
                let on = |t: f64| frame.to_world([f * t * t, 2.0 * f * t, 0.0]);
                let parameter = |p| {
                    let t = frame.to_local(p)[1] / (2.0 * f);
                    (dist(on(t), p) <= 1e-4).then_some(t)
                };
                let (ta, tb) = (parameter(a)?, parameter(b)?);
                let (t0, t1) = (ta.min(tb), ta.max(tb));
                if t1 - t0 < 1e-9 {
                    return None;
                }
                // The middle Bezier pole is the intersection of the end tangents.
                let middle = frame.to_world([f * t0 * t1, f * (t0 + t1), 0.0]);
                NurbsCurve::new(
                    2,
                    vec![on(t0), middle, on(t1)],
                    vec![1.0; 3],
                    vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                )
                .map(Curve::Nurbs)
            }
        }
    }
}

/// The curve under an edge.
#[derive(Clone, Debug, PartialEq)]
pub enum Curve {
    Line {
        origin: V3,
        dir: V3,
    },
    Circle {
        frame: Frame,
        radius: f64,
    },
    Ellipse {
        frame: Frame,
        major: f64,
        minor: f64,
    },
    /// Any other curve, as its whole curve in NURBS form; an edge on it is cut out by
    /// inverting its vertices.
    Nurbs(NurbsCurve),
}

impl Curve {
    /// The curve's derivative at t.
    pub fn derivative(&self, t: f64) -> V3 {
        match self {
            Curve::Line { dir, .. } => *dir,
            Curve::Circle { frame, radius } => {
                frame.dir_to_world([-radius * t.sin(), radius * t.cos(), 0.0])
            }
            Curve::Ellipse {
                frame,
                major,
                minor,
            } => frame.dir_to_world([-major * t.sin(), minor * t.cos(), 0.0]),
            Curve::Nurbs(c) => c.derivative(Self::nurbs_parameter(c, t)),
        }
    }

    pub fn value(&self, t: f64) -> V3 {
        match self {
            Curve::Line { origin, dir } => add(*origin, scale(*dir, t)),
            Curve::Circle { frame, radius } => {
                frame.to_world([radius * t.cos(), radius * t.sin(), 0.0])
            }
            Curve::Ellipse {
                frame,
                major,
                minor,
            } => frame.to_world([major * t.cos(), minor * t.sin(), 0.0]),
            Curve::Nurbs(n) => n.value(Self::nurbs_parameter(n, t)),
        }
    }

    // A closed edge may run past the knot domain. Values and tangents must use the same
    // turn; clamping the tangent while wrapping the point changes the boundary integral.
    fn nurbs_parameter(n: &NurbsCurve, t: f64) -> f64 {
        let (lo, hi) = n.domain();
        if (t < lo || t > hi) && n.is_closed() {
            lo + (t - lo).rem_euclid(hi - lo)
        } else {
            t
        }
    }

    /// The curve parameter of a point on (or near) the curve.
    pub fn parameter(&self, p: V3) -> f64 {
        match self {
            Curve::Line { origin, dir } => dot(sub(p, *origin), *dir) / dot(*dir, *dir),
            Curve::Circle { frame, .. } => {
                let l = frame.to_local(p);
                l[1].atan2(l[0]).rem_euclid(TAU)
            }
            Curve::Ellipse {
                frame,
                major,
                minor,
            } => {
                // The eccentric angle atan2(y/b, x/a) is exact only on the ellipse; a vertex
                // within tolerance off it takes the parameter of its foot instead.
                let l = frame.to_local(p);
                let (x, y) = ellipse_foot(*major, *minor, l[0], l[1]);
                (y / minor).atan2(x / major).rem_euclid(TAU)
            }
            Curve::Nurbs(n) => n.invert(p),
        }
    }

    /// The period of a closed curve, which an edge on it may run across.
    pub fn period(&self) -> Option<f64> {
        match self {
            Curve::Circle { .. } | Curve::Ellipse { .. } => Some(TAU),
            Curve::Nurbs(n) if n.is_closed() => {
                let (lo, hi) = n.domain();
                Some(hi - lo)
            }
            _ => None,
        }
    }
}

/// The point of the ellipse x²/a² + y²/b² = 1 nearest (x, y). Eberly's reduction ("Distance
/// from a point to an ellipse"): in the first quadrant, with a ≥ b, the foot is
/// (r x / (s + r), y / (s + 1)) for r = (a/b)² and s the one root of a decreasing function on a
/// known bracket, found by bisection to the last bit (so it cannot fail to converge, and the
/// answer does not depend on a seed). A point on an axis inside the evolute has two feet; the
/// one with the non-negative other coordinate is taken.
fn ellipse_foot(a: f64, b: f64, x: f64, y: f64) -> (f64, f64) {
    if a < b {
        let (fy, fx) = ellipse_foot(b, a, y, x);
        return (fx, fy);
    }
    let (ax, ay) = (x.abs(), y.abs());
    let (fx, fy) = if ay > 0.0 {
        if ax > 0.0 {
            let (z0, z1) = (ax / a, ay / b);
            let g = z0 * z0 + z1 * z1 - 1.0;
            if g == 0.0 {
                (ax, ay)
            } else {
                let r = (a / b) * (a / b);
                let (n0, n1) = (r * z0, z1);
                let mut lo = z1 - 1.0;
                let mut hi = if g < 0.0 { 0.0 } else { n0.hypot(n1) - 1.0 };
                let mut s = lo;
                // Each step halves the bracket; it stops when the midpoint is an end, well
                // within the cap (an f64 bracket cannot be halved more than ~2100 times).
                for _ in 0..2200 {
                    s = 0.5 * (lo + hi);
                    if s == lo || s == hi {
                        break;
                    }
                    let (q0, q1) = (n0 / (s + r), n1 / (s + 1.0));
                    let gs = q0 * q0 + q1 * q1 - 1.0;
                    if gs > 0.0 {
                        lo = s;
                    } else if gs < 0.0 {
                        hi = s;
                    } else {
                        break;
                    }
                }
                (r * ax / (s + r), ay / (s + 1.0))
            }
        } else {
            (0.0, b)
        }
    } else {
        let (numer, denom) = (a * ax, a * a - b * b);
        if numer < denom {
            let c = numer / denom;
            (a * c, b * (1.0 - c * c).sqrt())
        } else {
            (a, 0.0)
        }
    };
    (fx.copysign(x), if y < 0.0 { -fy } else { fy })
}

/// Wrap `a` into `[reference - π, reference + π)`.
pub fn nearest_turn(a: f64, reference: f64) -> f64 {
    a - TAU * ((a - reference + PI) / TAU).floor()
}

pub const COORD_FLOOR: f64 = 1e-6;
/// Unit normals or directions within this of parallel are one (`SMOOTH_ARC_GAP`).
pub const SMOOTH_ARC_GAP: f64 = 1e-9;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_x_axis_follows_gp_ax2() {
        assert_eq!(default_x_axis([0.0, 0.0, 1.0]), [1.0, 0.0, 0.0]);
        assert_eq!(default_x_axis([0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]);
        assert_eq!(default_x_axis([1.0, 0.0, 0.0]), [0.0, 0.0, 1.0]);
        let z = unit([0.3, -0.5, 0.8]).unwrap();
        assert!(dot(default_x_axis(z), z).abs() < 1e-15);
    }

    #[test]
    fn nearest_turn_wraps_into_half_open_window() {
        assert!((nearest_turn(7.0, 0.5) - (7.0 - TAU)).abs() < 1e-15);
        assert!((nearest_turn(-3.0, 3.0) - (-3.0 + TAU)).abs() < 1e-15);
        assert_eq!(nearest_turn(1.0, 1.0), 1.0);
    }

    fn tilted_frame() -> Frame {
        let z = unit([0.2, 0.3, 0.9]).unwrap();
        let x = default_x_axis(z);
        Frame {
            origin: [1.0, -2.0, 3.0],
            x,
            y: cross(z, x),
            z,
        }
    }

    #[test]
    fn analytic_parameters_invert_value() {
        let frame = tilted_frame();
        let surfaces = [
            Surface::Plane { frame },
            Surface::Cylinder { frame, radius: 2.5 },
            Surface::Cone {
                frame,
                radius: 2.0,
                semi_angle: 0.4,
            },
            Surface::Sphere { frame, radius: 3.0 },
            Surface::Torus {
                frame,
                major: 5.0,
                minor: 1.5,
            },
        ];
        for s in &surfaces {
            for (u, v) in [(0.3, 0.2), (2.0, -0.7), (5.5, 1.1)] {
                let p = s.value(u, v);
                let (pu, pv) = s.parameters(p, None).unwrap();
                let back = s.value(pu, pv);
                assert!(dist(back, p) < 1e-9, "{s:?} at ({u}, {v})");
            }
        }
    }

    #[test]
    fn closed_spline_tangents_follow_wrapped_points() {
        let curve = Curve::Nurbs(
            NurbsCurve::new(
                1,
                vec![
                    [0.0, 0.0, 0.0],
                    [2.0, 0.0, 0.0],
                    [0.0, 3.0, 0.0],
                    [0.0, 0.0, 0.0],
                ],
                vec![1.0; 4],
                vec![0.0, 0.0, 1.0, 2.0, 3.0, 3.0],
            )
            .unwrap(),
        );
        let period = curve.period().unwrap();
        for t in [0.25, 1.25, 2.25] {
            for turn in [-3.0, -1.0, 0.0, 1.0, 4.0] {
                let shifted = t + turn * period;
                assert!(dist(curve.value(shifted), curve.value(t)) < 1e-14);
                assert!(dist(curve.derivative(shifted), curve.derivative(t)) < 1e-14);
                let h = 1e-5;
                let finite_difference = scale(
                    sub(curve.value(shifted + h), curve.value(shifted - h)),
                    0.5 / h,
                );
                assert!(dist(curve.derivative(shifted), finite_difference) < 1e-8);
            }
        }
    }

    /// A point off an ellipse along its normal takes its foot's parameter (cgb207 edge 360's
    /// vertex lies 0.012 mm off its ellipse; its eccentric angle is 7.5e-4 rad from the foot's).
    #[test]
    fn ellipse_parameter_is_the_foot_point() {
        let frame = tilted_frame();
        for (major, minor) in [(5.0, 2.0), (2.0, 5.0), (3.0, 3.0)] {
            let curve = Curve::Ellipse {
                frame,
                major,
                minor,
            };
            for t in [0.0, 0.4, 1.3, PI / 2.0, 2.5, PI, 3.988586, 5.9] {
                let tangent = curve.derivative(t);
                let normal = unit(cross(frame.z, tangent)).unwrap();
                for off in [0.0, 0.012, -0.012, 0.5] {
                    let p = add(add(curve.value(t), scale(normal, off)), scale(frame.z, 0.3));
                    let got = curve.parameter(p);
                    let gap = (nearest_turn(got, t) - t).abs();
                    assert!(gap < 1e-12, "{major} x {minor} at {t} off {off}: {got}");
                }
            }
        }
        // On an axis inside the evolute the foot is off the axis (two of them; y >= 0 is taken).
        let curve = Curve::Ellipse {
            frame: Frame {
                origin: [0.0; 3],
                x: [1.0, 0.0, 0.0],
                y: [0.0, 1.0, 0.0],
                z: [0.0, 0.0, 1.0],
            },
            major: 5.0,
            minor: 2.0,
        };
        let t = curve.parameter([1.0, 0.0, 0.0]);
        let foot = curve.value(t);
        assert!(
            (foot[0] - 25.0 / 21.0).abs() < 1e-12 && foot[1] > 0.0,
            "{foot:?}"
        );
    }

    #[test]
    fn ray_hits_count_crossings() {
        let frame = Frame {
            origin: [0.0; 3],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            z: [0.0, 0.0, 1.0],
        };
        let cyl = Surface::Cylinder { frame, radius: 1.0 };
        let (hits, grazing) = cyl
            .ray_hits([-5.0, 0.0, 0.0], [1.0, 0.0, 0.0], 100.0)
            .unwrap();
        assert_eq!(hits.len(), 2);
        assert!(!grazing);
        // A ray lying in a plane reports no hit but flags itself as grazing.
        let plane = Surface::Plane { frame };
        let (hits, grazing) = plane
            .ray_hits([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 100.0)
            .unwrap();
        assert!(hits.is_empty() && grazing);
        // A line through a torus's centre in its mid-plane crosses the tube four times.
        let torus = Surface::Torus {
            frame,
            major: 5.0,
            minor: 1.0,
        };
        let (hits, _) = torus
            .ray_hits([-10.0, 0.0, 0.0], [1.0, 0.0, 0.0], 100.0)
            .unwrap();
        assert_eq!(hits.len(), 4);
        // A cone's other nappe is not part of the surface.
        let cone = Surface::Cone {
            frame,
            radius: 1.0,
            semi_angle: 0.5,
        };
        let (hits, _) = cone
            .ray_hits([0.0, 0.0, 10.0], [0.0, 0.0, -1.0], 100.0)
            .unwrap();
        assert_eq!(hits.len(), 1, "only the apex of the covered nappe");
    }
}
