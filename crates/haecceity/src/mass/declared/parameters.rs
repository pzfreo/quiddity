//! Map STEP swept-surface parameters into the evaluator's bounded NURBS patch.
//! Extrusion distance includes the declared vector magnitude. A rational
//! quadratic quadrant has a different parameter from its polar angle.
use super::*;
use crate::brep::{RawProfile, SweptSource};
use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Axis {
    Linear { scale: f64, offset: f64 },
    Angle { radians: f64 },
}
impl Axis {
    pub(super) const ID: Self = Self::Linear {
        scale: 1.,
        offset: 0.,
    };
    pub(super) fn at(self, t: f64) -> (f64, f64) {
        match self {
            Self::Linear { scale, offset } => (t * scale + offset, scale),
            Self::Angle { radians } => {
                let a = t * radians;
                let q = (a / FRAC_PI_2).floor();
                let h = 0.5 * (a - q * FRAC_PI_2);
                let p = h.sin();
                let sum = p + (0.5 * FRAC_PI_2 - h).sin();
                (q + p / sum, radians * 0.5 * FRAC_1_SQRT_2 / (sum * sum))
            }
        }
    }
    pub(super) fn inverse(self, t: f64) -> f64 {
        match self {
            Self::Linear { scale, offset } => (t - offset) / scale,
            Self::Angle { radians } => {
                let q = t.floor();
                let f = t - q;
                (q * FRAC_PI_2 + 2. * (f * FRAC_1_SQRT_2).atan2(1. - f + f * FRAC_1_SQRT_2))
                    / radians
            }
        }
    }
}

pub(super) fn swept_axes(part: &Part, face: usize) -> Option<[Axis; 2]> {
    let Surface::Freeform { surface: s, .. } = &part.faces[face].surface else {
        return None;
    };
    let (u0, u1, v0, v1) = s.domain();
    let (mm, rad) = part.file_units()?;
    let at = part.face_placement(face);
    let vector = |p: V3| std::array::from_fn(|i| (0..3).map(|k| at[i][k] * p[k]).sum());
    let point = |p: V3| geom::add(vector(geom::scale(p, mm)), at.map(|row| row[3]));
    let linear = |a: f64, b: f64, x: f64, y: f64| {
        let scale = (y - x) / (b - a);
        (scale.is_finite() && scale != 0.).then_some(Axis::Linear {
            scale,
            offset: x - scale * a,
        })
    };
    // The first control of these four clamped profile forms is their start.
    let start = |profile: &RawProfile| -> Option<V3> {
        Some(match profile {
            RawProfile::Line { point: p, .. } => point(*p),
            RawProfile::BSpline { points, .. } => point(*points.first()?),
            RawProfile::Circle {
                origin,
                frame,
                radius,
                ..
            }
            | RawProfile::Ellipse {
                origin,
                frame,
                semi_axis_1: radius,
                ..
            } => {
                let z = geom::unit(frame.axis.unwrap_or([0., 0., 1.]))?;
                let x = frame.reference.unwrap_or_else(|| geom::default_x_axis(z));
                let x = geom::unit(geom::sub(x, geom::scale(z, geom::dot(x, z))))?;
                point(geom::add(*origin, geom::scale(x, *radius)))
            }
        })
    };
    let profile_axis = |profile: &RawProfile, ends: (V3, V3), domain: (f64, f64)| {
        Some(match profile {
            RawProfile::BSpline { .. } => Axis::ID,
            RawProfile::Circle { .. } | RawProfile::Ellipse { .. } => Axis::Angle { radians: rad },
            RawProfile::Line {
                point: p,
                direction,
                magnitude,
            } => {
                let d = vector(geom::unit(*direction)?);
                let t = |x| geom::dot(geom::sub(x, point(*p)), d) / (mm * magnitude);
                linear(t(ends.0), t(ends.1), domain.0, domain.1)?
            }
        })
    };
    match part.face_swept(face)? {
        SweptSource::Revolution { profile, .. } => Some([
            Axis::Angle { radians: rad },
            profile_axis(profile, (s.value(u0, v0), s.value(u0, v1)), (v0, v1))?,
        ]),
        SweptSource::Extrusion {
            profile,
            direction,
            magnitude,
            ..
        } => {
            let d = vector(geom::unit(*direction)?);
            let p = start(profile)?;
            let height = |x| geom::dot(geom::sub(x, p), d) / (mm * magnitude);
            let bottom = s.value(u0, v0);
            let ta = height(bottom);
            let tb = height(s.value(u0, v1));
            let shift = geom::scale(d, ta * mm * magnitude);
            Some([
                profile_axis(
                    profile,
                    (geom::sub(bottom, shift), geom::sub(s.value(u1, v0), shift)),
                    (u0, u1),
                )?,
                linear(ta, tb, v0, v1)?,
            ])
        }
    }
}
