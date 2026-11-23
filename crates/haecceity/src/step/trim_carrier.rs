//! Native spline carriers referenced only by a pcurve have no topological face
//! through which step-io can return a surface handle. Their poles, knots and
//! weights are sufficient to evaluate them without inventing a face or bounds.
use super::*;

pub(super) fn standard_knots(
    degree: i64,
    count: usize,
    interior: Option<i64>,
) -> Option<(Vec<f64>, Vec<i64>)> {
    let d = usize::try_from(degree).ok()?;
    if d == 0 || d > crate::nurbs::MAX_DEGREE || d >= count {
        return None;
    }
    let mult = if let Some(interior) = interior {
        let inner = usize::try_from(interior).ok()?;
        if inner == 0 || !(count - d - 1).is_multiple_of(inner) {
            return None;
        }
        let spans = (count - d - 1) / inner + 1;
        let mut counts = vec![interior; spans + 1];
        counts[0] = degree + 1;
        counts[spans] = degree + 1;
        counts
    } else {
        vec![1; count.checked_add(d + 1)?]
    };
    Some(((0..mult.len()).map(|i| i as f64).collect(), mult))
}

enum Knots<'a> {
    Explicit([(&'a [f64], &'a [i64]); 2]),
    Bezier,
    QuasiUniform,
    Uniform,
}

impl Knots<'_> {
    fn expanded(&self, axis: usize, degree: i64, count: usize) -> Option<Vec<f64>> {
        let d = usize::try_from(degree).ok()?;
        if d == 0 || d > crate::nurbs::MAX_DEGREE || d >= count {
            return None;
        }
        let standard;
        let (knots, mult) = match self {
            Self::Explicit(k) => k[axis],
            _ => {
                let interior = match self {
                    Self::Bezier => Some(degree),
                    Self::QuasiUniform => Some(1),
                    _ => None,
                };
                standard = standard_knots(degree, count, interior)?;
                (standard.0.as_slice(), standard.1.as_slice())
            }
        };
        let size = count.checked_add(d + 1)?;
        if knots.len() != mult.len()
            || !knots.iter().all(|k| k.is_finite())
            || !knots.windows(2).all(|v| v[0] < v[1])
            || !mult.iter().all(|&n| n > 0 && n <= degree + 1)
            || mult
                .iter()
                .try_fold(0usize, |a, &n| a.checked_add(n as usize))?
                != size
        {
            return None;
        }
        Some(
            knots
                .iter()
                .zip(mult)
                .flat_map(|(&k, &n)| std::iter::repeat_n(k, n as usize))
                .collect(),
        )
    }
}

impl Reader<'_> {
    pub(super) fn independent_spline(&self, source: &m::SurfaceRef) -> Option<Surface> {
        let mut weights = None;
        let (du, dv, rows, knots) = match source {
            m::SurfaceRef::BSplineSurfaceWithKnots(i) => {
                let s = self.model.get(m::BSplineSurfaceWithKnotsId(i.0))?;
                (
                    s.u_degree,
                    s.v_degree,
                    &s.control_points_list,
                    Knots::Explicit([
                        (&s.u_knots, &s.u_multiplicities),
                        (&s.v_knots, &s.v_multiplicities),
                    ]),
                )
            }
            m::SurfaceRef::BezierSurface(i) => {
                let s = self.model.get(m::BezierSurfaceId(i.0))?;
                (
                    s.u_degree,
                    s.v_degree,
                    &s.control_points_list,
                    Knots::Bezier,
                )
            }
            m::SurfaceRef::QuasiUniformSurface(i) => {
                let s = self.model.get(m::QuasiUniformSurfaceId(i.0))?;
                (
                    s.u_degree,
                    s.v_degree,
                    &s.control_points_list,
                    Knots::QuasiUniform,
                )
            }
            m::SurfaceRef::UniformSurface(i) => {
                let s = self.model.get(m::UniformSurfaceId(i.0))?;
                (
                    s.u_degree,
                    s.v_degree,
                    &s.control_points_list,
                    Knots::Uniform,
                )
            }
            m::SurfaceRef::Complex(i) => {
                let mut spline = None;
                let mut knots = None;
                for part in &self.model.get(m::ComplexEntityId(i.0))?.parts {
                    match part {
                        m::PartialEntity::BSplineSurface {
                            u_degree,
                            v_degree,
                            control_points_list,
                            ..
                        } => spline = Some((*u_degree, *v_degree, control_points_list)),
                        m::PartialEntity::BSplineSurfaceWithKnots {
                            u_knots,
                            u_multiplicities,
                            v_knots,
                            v_multiplicities,
                            ..
                        } => {
                            knots = Some(Knots::Explicit([
                                (u_knots, u_multiplicities),
                                (v_knots, v_multiplicities),
                            ]))
                        }
                        m::PartialEntity::BezierSurface => knots = Some(Knots::Bezier),
                        m::PartialEntity::QuasiUniformSurface => knots = Some(Knots::QuasiUniform),
                        m::PartialEntity::UniformSurface => knots = Some(Knots::Uniform),
                        m::PartialEntity::RationalBSplineSurface { weights_data } => {
                            weights = Some(weights_data)
                        }
                        _ => {}
                    }
                }
                let (du, dv, rows) = spline?;
                (du, dv, rows, knots?)
            }
            _ => return None,
        };
        let nu = rows.len();
        let nv = rows.first()?.len();
        if rows.iter().any(|r| r.len() != nv) {
            return None;
        }
        let ku = knots.expanded(0, du, nu)?;
        let kv = knots.expanded(1, dv, nv)?;
        let points = rows
            .iter()
            .map(|row| row.iter().map(|r| self.point(r)).collect())
            .collect::<Option<Vec<_>>>()?;
        let weights = match weights {
            Some(w) if w.iter().flatten().all(|v| v.is_finite()) => w.clone(),
            Some(_) => return None,
            None => vec![vec![1.; nv]; nu],
        };
        let surface = NurbsSurface::new(du as usize, dv as usize, points, weights, ku, kv)?;
        Some(Surface::Freeform {
            kind: if matches!(knots, Knots::Bezier) {
                "BEZIER"
            } else {
                "BSPLINE"
            },
            surface: Box::new(surface),
        })
    }
}
