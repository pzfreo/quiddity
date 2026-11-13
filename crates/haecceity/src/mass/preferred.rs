//! Recovery from an unresolved projection using STEP's explicitly preferred trim.
//! This integrates the master pcurve region, which need not equal the projection
//! of a separately approximated 3D curve. Successful legacy integrals never enter.

use super::*;

/// Refine the largest remaining error first, retaining the same depth limit.
/// The sum of all remaining estimates must fit the caller's absolute budget.
/// An interval at MAX_DEPTH keeps contributing its error; exhaustion refuses.
fn integrate_inner<const N: usize>(
    cuts: &[f64],
    budget: f64,
    f: impl Fn(f64) -> [f64; N],
    rule: IntegralRule,
) -> Option<([f64; N], f64)> {
    if !(budget.is_finite() && budget >= 0.) || cuts.is_empty() || cuts.len() > MAX_PANELS {
        return None;
    }
    if cuts.len() == 1 && cuts[0].is_finite() {
        return Some(([0.; N], 0.));
    }
    let eval = |a, b, depth| {
        let (p, _) = kronrod_rule((), a, b, |(), t| Some(f(t)), rule)?;
        if !p.1.is_finite() || p.0.iter().any(|x| !x.is_finite()) {
            return None;
        }
        Some((a, b, depth, p.0, p.1))
    };
    let mut panels = Vec::new();
    let mut heap = std::collections::BinaryHeap::new();
    let mut error = 0.;
    for w in cuts.windows(2) {
        let p = eval(w[0], w[1], 0)?;
        error += p.4;
        heap.push((p.4.to_bits(), panels.len()));
        panels.push(Some(p));
    }
    loop {
        if error <= budget {
            let mut sum = [0.; N];
            let mut verified = 0.;
            for p in panels.iter().flatten() {
                for (sum, value) in sum.iter_mut().zip(p.3) {
                    *sum += value;
                }
                verified += p.4;
            }
            if verified <= budget {
                return Some((sum, verified));
            }
            error = verified;
        }
        let (_, i) = heap.pop()?;
        let (a, b, depth, sum, e) = panels[i].take()?;
        if depth >= MAX_DEPTH {
            panels[i] = Some((a, b, depth, sum, e));
            continue;
        }
        if panels.len() >= MAX_PANELS {
            return None;
        }
        let m = 0.5 * (a + b);
        if m == a || m == b {
            return None;
        }
        error = (error - e).max(0.);
        for (a, b) in [(a, m), (m, b)] {
            let p = eval(a, b, depth + 1)?;
            error += p.4;
            heap.push((p.4.to_bits(), panels.len()));
            panels.push(Some(p));
        }
    }
}
fn raw_param_curve(pc: &crate::brep::RawCurve2d) -> Option<Curve> {
    match pc {
        crate::brep::RawCurve2d::Line {
            point,
            direction,
            magnitude,
        } => Some(Curve::Line {
            origin: [point[0], point[1], 0.],
            dir: [direction[0] * magnitude, direction[1] * magnitude, 0.],
        }),
        crate::brep::RawCurve2d::BSpline {
            degree,
            points,
            multiplicities,
            knots,
            weights,
        } => {
            let degree = usize::try_from(*degree).ok()?;
            let multiplicities = multiplicities
                .iter()
                .map(|&n| usize::try_from(n).ok())
                .collect::<Option<Vec<_>>>()?;
            let count = multiplicities
                .iter()
                .try_fold(0usize, |a, &b| a.checked_add(b))?;
            if degree > crate::nurbs::MAX_DEGREE
                || points.len() > MAX_PANELS
                || knots.len() != multiplicities.len()
                || knots.iter().any(|k| !k.is_finite())
                || count != points.len().checked_add(degree + 1)?
                || weights
                    .as_ref()
                    .is_some_and(|w| w.iter().any(|x| !x.is_finite()))
            {
                return None;
            }
            crate::nurbs::NurbsCurve::new(
                degree,
                points.iter().map(|p| [p[0], p[1], 0.]).collect(),
                weights.clone().unwrap_or_else(|| vec![1.; points.len()]),
                knots
                    .iter()
                    .zip(multiplicities)
                    .flat_map(|(&k, n)| std::iter::repeat_n(k, n))
                    .collect(),
            )
            .map(Curve::Nurbs)
        }
    }
}
/// An upper bound on total variation of one rational coordinate. Partition of
/// unity gives sum |N'_i| <= 2p/min_span; positive weights bound division by W.
/// The squared weight ratio is conservative. Lines have an exact bound.
fn variation(curve: &Curve, a: f64, b: f64, k: usize) -> f64 {
    match curve {
        Curve::Line { dir, .. } => dir[k].abs() * (b - a).abs(),
        Curve::Nurbs(c) => {
            let lo = c
                .control_points()
                .iter()
                .map(|p| p[k])
                .fold(f64::INFINITY, f64::min);
            let hi = c
                .control_points()
                .iter()
                .map(|p| p[k])
                .fold(f64::NEG_INFINITY, f64::max);
            let span = c
                .knots()
                .windows(2)
                .filter_map(|w| (w[1] > w[0]).then_some(w[1] - w[0]))
                .fold(f64::INFINITY, f64::min);
            let low = c.weights().iter().copied().fold(f64::INFINITY, f64::min);
            let high = c.weights().iter().copied().fold(0., f64::max);
            2. * c.degree() as f64 * (hi - lo) / span * (high / low).powi(2) * (b - a).abs()
        }
        _ => f64::INFINITY,
    }
}

/// Deterministic samples on the union of both curves' knot spans. This bounds
/// sampled representation disagreement, separately from quadrature certification.
fn master_deviation(surface: &Surface, pc: &Curve, edge: &Curve, a: f64, b: f64) -> Option<f64> {
    let mut knots = Vec::new();
    for c in [pc, edge] {
        if let Curve::Nurbs(c) = c {
            knots.extend(c.breaks());
        }
    }
    let spans = cuts(a, b, 1, &knots)?;
    let mut maximum: f64 = 0.;
    for span in spans.windows(2) {
        // 33 samples per span (including endpoints), independent of quadrature nodes.
        for j in 0..=32 {
            let t = span[0] + (span[1] - span[0]) * (j as f64 / 32.);
            let p = pc.value(t);
            let distance = geom::dist(surface.value(p[0], p[1]), edge.value(t));
            if !distance.is_finite() {
                return None;
            }
            maximum = maximum.max(distance);
        }
    }
    Some(maximum)
}

impl Part {
    /// Only explicitly preferred, complete trims on open spline patches may replace
    /// an unresolved 3D projection. Unselected approximations are not an authority.
    fn integration_pcurves(&self, face: usize) -> Option<Vec<(usize, Curve)>> {
        self.primary_pcurves(face).next()?;
        let fc = &self.faces[face];
        let Surface::Freeform { surface, kind } = &fc.surface else {
            return None;
        };
        // Swept surfaces converted to NURBS can have different parameter units.
        // Only these native parameterizations are retained exactly by the reader.
        if !matches!(*kind, "BSPLINE" | "BEZIER") {
            return None;
        }
        if fc.loops.len() != 1
            || fc.loops[0].vertex.is_some()
            || closed_spans(&fc.surface) != (f64::INFINITY, f64::INFINITY)
            || collapsed_sides(&fc.surface) != (None, None)
        {
            return None;
        }
        let (u0, u1, v0, v1) = surface.domain();
        let inside = |p: V3| {
            p.iter().all(|x| x.is_finite()) && p[0] >= u0 && p[0] <= u1 && p[1] >= v0 && p[1] <= v1
        };
        // Use the same face bounding-box diagonal as centroid evidence (mm),
        // with no unit-size floor; inertia evidence uses its trace separately.
        let length = self.face_bounds(face).diagonal();
        if !length.is_finite() || length <= 0. {
            return None;
        }
        let mut out = Vec::new();
        for &(e, _) in &fc.loops[0].edges {
            if out.iter().any(|(pe, _)| *pe == e) {
                return None;
            }
            let mut choices = self.primary_pcurves(face).filter(|(pe, _)| *pe == e);
            let (_, raw) = choices.next()?;
            if choices.next().is_some() {
                return None;
            }
            let pc = raw_param_curve(raw)?;
            let ed = &self.edges[e];
            let (a, b) = edge_interval(&ed.curve, ed.start, ed.end, ed.same_sense, ed.is_closed());
            match &pc {
                Curve::Nurbs(c) => {
                    let (lo, hi) = c.domain();
                    let near = 64. * f64::EPSILON * lo.abs().max(hi.abs()).max(hi - lo);
                    if (a.min(b) - lo).abs() > near
                        || (a.max(b) - hi).abs() > near
                        || !c.control_points().iter().copied().all(inside)
                    {
                        return None;
                    }
                }
                Curve::Line { .. } => {
                    if !inside(pc.value(a)) || !inside(pc.value(b)) {
                        return None;
                    }
                }
                _ => return None,
            }
            // End matching establishes the range and direction, not equivalence of
            // the two interiors. The STEP master choice supplies the trim's authority.
            let noise = self
                .file_uncertainty()
                .filter(|x| x.is_finite() && *x >= 0.)
                .unwrap_or(0.)
                + geom::COORD_FLOOR;
            let at = |t| {
                let q = pc.value(t);
                fc.surface.value(q[0], q[1])
            };
            if geom::dist(at(a), ed.start) > noise || geom::dist(at(b), ed.end) > noise {
                return None;
            }
            if master_deviation(&fc.surface, &pc, &ed.curve, a, b)? / length > 0.001 {
                return None;
            }
            out.push((e, pc));
        }
        (!out.is_empty()).then_some(out)
    }

    /// Integrate the explicit STEP trim after projection has refused. The outer
    /// criterion is unchanged. Half its fixed floor budgets all inner primitives;
    /// their measured errors also enter each outer panel's error estimate.
    pub(super) fn integrate_preferred_face<const N: usize>(
        &self,
        face: usize,
        density: impl Fn(V3, V3, V3) -> [f64; N],
        components: Components,
    ) -> Option<[f64; N]> {
        let curves = self.integration_pcurves(face)?;
        let fc = &self.faces[face];
        let Surface::Freeform { surface, .. } = &fc.surface else {
            return None;
        };
        let mut paths = Vec::new();
        for &(e, forward) in &fc.loops[0].edges {
            let curve = &curves.iter().find(|(pe, _)| *pe == e)?.1;
            let ed = &self.edges[e];
            let (mut a, mut b) =
                edge_interval(&ed.curve, ed.start, ed.end, ed.same_sense, ed.is_closed());
            if !forward {
                std::mem::swap(&mut a, &mut b);
            }
            paths.push((curve.clone(), a, b));
        }
        let count = paths.len();
        let noise = self
            .file_uncertainty()
            .filter(|x| x.is_finite() && *x >= 0.)
            .unwrap_or(0.)
            + geom::COORD_FLOOR;
        // Complete the same small endpoint gaps as the ordinary boundary walk.
        // A gap with appreciable physical length is an incomplete trim, not a
        // licence to close an arbitrary region in UV.
        for i in 0..count {
            let from = paths[i].0.value(paths[i].2);
            let to = paths[(i + 1) % count].0.value(paths[(i + 1) % count].1);
            let delta = geom::sub(to, from);
            if delta == [0.; 3] {
                continue;
            }
            let length = integrate(&[0., 1.], |t| {
                let q = geom::add(from, geom::scale(delta, t));
                let (_, su, sv) = surface.value_and_partials(q[0], q[1]);
                [geom::norm(geom::add(
                    geom::scale(su, delta[0]),
                    geom::scale(sv, delta[1]),
                ))]
            })[0];
            if !length.is_finite() || length > noise {
                return None;
            }
            paths.push((
                Curve::Line {
                    origin: from,
                    dir: delta,
                },
                0.,
                1.,
            ));
        }
        let variation = paths
            .iter()
            .map(|(c, a, b)| variation(c, *a, *b, 1))
            .sum::<f64>();
        if !(variation.is_finite() && variation > 0.) {
            return None;
        }
        let reference = paths.first()?.0.value(paths[0].1)[0];
        // Rational derivatives in world coordinates can subtract large nearly
        // equal homogeneous terms. Translate controls before evaluating them;
        // add the origin only to the point passed to the physical density.
        let origin = self.edges[fc.loops[0].edges[0].0].start;
        let local = crate::nurbs::NurbsSurface::new(
            surface.degree_u,
            surface.degree_v,
            surface
                .control_points
                .iter()
                .map(|row| row.iter().map(|p| geom::sub(*p, origin)).collect())
                .collect(),
            surface.weights.clone(),
            surface.knots_u.clone(),
            surface.knots_v.clone(),
        )?;
        let evaluator = std::cell::RefCell::new(crate::nurbs::SurfaceEvaluation::new(&local));
        let f = |u, v| {
            let (p, su, sv) = evaluator.borrow_mut().value_and_partials(u, v);
            density(geom::add(origin, p), su, sv)
        };
        let rule = IntegralRule {
            components,
            observe_mass: None,
        };
        let budget = std::cell::Cell::new(None);
        let term = |u, v, dv: f64| {
            if dv == 0. {
                return Some(([0.; N], 0.));
            }
            let cuts = surface_cuts(&fc.surface, f64::INFINITY, false, reference, u)?;
            let (sum, error) = match budget.get() {
                None => (integrate(&cuts, |s| f(s, v)), 0.),
                Some(budget) => integrate_inner(&cuts, budget, |s| f(s, v), rule)?,
            };
            Some((sum.map(|y| y * dv), error * dv.abs()))
        };
        let knots = (
            breaks(&surface.knots_u, surface.degree_u),
            breaks(&surface.knots_v, surface.degree_v),
        );
        let piece = |p: V3| {
            (
                knots.0.partition_point(|&k| k <= p[0]),
                knots.1.partition_point(|&k| k <= p[1]),
            )
        };
        let walk = |floor| {
            let (mut total, mut magnitude, mut swept) = ([0.; N], 0., 0.);
            for (curve, a, b) in &paths {
                let mut eval = |state: f64, a: f64, b: f64, locate| {
                    let mut met = Vec::new();
                    let (half, centre) = (0.5 * (b - a), 0.5 * (b + a));
                    let (mut sum, mut gauss, mut size) = ([0.; N], [0.; N], [0.; N]);
                    let mut swept = state;
                    let mut inner_error = 0.;
                    // Pair each node with its weights explicitly. Error transport
                    // must not depend on another evaluator's callback order.
                    for &(x, wk, wg) in kronrod_nodes() {
                        let t = centre + half * x;
                        let weight = wk * half;
                        let p = curve.value(t);
                        let d = curve.derivative(t);
                        swept += weight * (p[0] - reference) * d[1];
                        if locate {
                            met.push((t, p));
                        }
                        let (y, error) = term(p[0], p[1], d[1])?;
                        if y.iter().any(|x| !x.is_finite()) {
                            return None;
                        }
                        inner_error += weight.abs() * error;
                        for k in 0..N {
                            sum[k] += wk * half * y[k];
                            gauss[k] += wg * half * y[k];
                            size[k] += (wk * half * y[k]).abs();
                        }
                    }
                    let (error, magnitude) = estimate(&sum, &gauss, &size, components);
                    let mut panel = (sum, error, magnitude, swept);
                    panel.1 += inner_error;
                    let corner = |(ta, pa): (f64, V3), tb| {
                        let (mut lo, mut hi) = (ta, tb);
                        for _ in 0..60 {
                            let mid = 0.5 * (lo + hi);
                            if piece(curve.value(mid)) == piece(pa) {
                                lo = mid;
                            } else {
                                hi = mid;
                            }
                        }
                        0.5 * (lo + hi)
                    };
                    let mut kink = met
                        .windows(2)
                        .find(|w| piece(w[0].1) != piece(w[1].1))
                        .map(|w| (corner(w[0], w[1].0), false));
                    if kink.is_none() && locate {
                        for (node, end) in [(*met.first()?, a), (*met.last()?, b)] {
                            if piece(node.1) != piece(curve.value(end)) {
                                let t = corner(node, end);
                                if (t - end).abs() > 1e-9 * (b - a).abs() {
                                    kink = Some((t, true));
                                    break;
                                }
                            }
                        }
                    }
                    Some((panel, kink))
                };
                let cut = curve_cuts(&fc.surface, curve, *a, *b)?;
                let (sum, error, size, area) = adaptive(&cut, 0., floor, true, &mut eval)?;
                if floor.is_some_and(|floor| error > floor) {
                    return None;
                }
                for k in 0..N {
                    total[k] += sum[k];
                }
                magnitude += size;
                swept += area;
            }
            Some((total, magnitude, swept))
        };
        let (_, magnitude, _) = walk(None)?;
        let floor = 1e-11 * magnitude;
        if !(floor.is_finite() && floor > 0.) {
            return None;
        }
        budget.set(Some(0.5 * floor / variation));
        let (total, _, swept) = walk(Some(floor))?;
        if !swept.is_finite() || swept == 0. {
            return None;
        }
        Some(total.map(|y| y * swept.signum()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Constructed here from a polynomial surface, never extracted from a model.
    // S(u,v)=(u+v,epsilon*v,v²). The 3D curves are normal offsets of the
    // rectangular trim: Gauss-Newton oscillates where the tangents are nearly
    // parallel, while the explicitly preferred pcurves specify the same foot.
    use crate::{
        brep::{Edge, Face, Loop, RawCurve2d},
        nurbs::{NurbsCurve, NurbsSurface},
    };
    fn narrow_parabola() -> Part {
        // The original width-one geometry remains a refusal regression below:
        // its representation disagreement exceeds the new interior band.
        narrow_parabola_with_width(2.)
    }

    fn narrow_parabola_with_width(width: f64) -> Part {
        let eps = 0.001;
        let h = 0.0001;
        let points = (0..2)
            .map(|i| {
                (0..3)
                    .map(|j| {
                        [
                            width * i as f64 + j as f64 / 2.,
                            eps * j as f64 / 2.,
                            if j == 2 { 1. } else { 0. },
                        ]
                    })
                    .collect()
            })
            .collect();
        let surface = Surface::Freeform {
            kind: "BSPLINE",
            surface: Box::new(
                NurbsSurface::new(
                    1,
                    2,
                    points,
                    vec![vec![1.; 3]; 2],
                    vec![0., 0., 1., 1.],
                    vec![0., 0., 0., 1., 1., 1.],
                )
                .unwrap(),
            ),
        };
        let point = |u: f64, v: f64| [width * u + v, (eps + 2. * h / eps) * v, v * v - h];
        let (ua, ub, va, vb) = (0.2, 0.8, 0.001, 0.005);
        let line = |v: f64| Curve::Line {
            origin: point(ua, v),
            dir: [1., 0., 0.],
        };
        let curve = |u: f64| {
            let a = point(u, va);
            let b = point(u, vb);
            let d = vb - va;
            let c = [
                a[0] + d / 2.,
                a[1] + (eps + 2. * h / eps) * d / 2.,
                a[2] + va * d,
            ];
            Curve::Nurbs(
                NurbsCurve::new(2, vec![a, c, b], vec![1.; 3], vec![0., 0., 0., 1., 1., 1.])
                    .unwrap(),
            )
        };
        let specs = [
            (line(va), point(ua, va), point(ub, va)),
            (curve(ub), point(ub, va), point(ub, vb)),
            (line(vb), point(ua, vb), point(ub, vb)),
            (curve(ua), point(ua, va), point(ua, vb)),
        ];
        let edges = specs
            .into_iter()
            .enumerate()
            .map(|(i, (curve, start, end))| Edge {
                samples: (0..=16)
                    .map(|k| {
                        let t = k as f64 / 16.;
                        if i % 2 == 0 {
                            [start[0] + (end[0] - start[0]) * t, start[1], start[2]]
                        } else {
                            curve.value(t)
                        }
                    })
                    .collect(),
                curve,
                start,
                end,
                vertices: (2 * i, 2 * i + 1),
                same_sense: true,
            })
            .collect();
        Part::new(
            vec![Face {
                surface,
                reversed: false,
                loops: vec![Loop {
                    edges: vec![(0, true), (1, true), (2, false), (3, false)],
                    vertex: None,
                }],
                solid: None,
                pcurves: vec![],
            }],
            edges,
            vec![],
        )
        .with_file_pcurves(
            std::collections::BTreeMap::from([(
                0,
                vec![
                    (
                        0,
                        RawCurve2d::Line {
                            point: [ua, va],
                            direction: [1., 0.],
                            magnitude: 1. / width,
                        },
                    ),
                    (
                        1,
                        RawCurve2d::BSpline {
                            degree: 1,
                            points: vec![[ub, va], [ub, vb]],
                            multiplicities: vec![2, 2],
                            knots: vec![0., 1.],
                            weights: None,
                        },
                    ),
                    (
                        2,
                        RawCurve2d::Line {
                            point: [ua, vb],
                            direction: [1., 0.],
                            magnitude: 1. / width,
                        },
                    ),
                    (
                        3,
                        RawCurve2d::BSpline {
                            degree: 1,
                            points: vec![[ua, va], [ua, vb]],
                            multiplicities: vec![2, 2],
                            knots: vec![0., 1.],
                            weights: None,
                        },
                    ),
                ],
            )]),
            std::collections::BTreeMap::from([(0, vec![0, 1, 2, 3])]),
            (1., 1.),
            Some(0.002),
        )
    }

    #[test]
    fn preferred_trim_rescues_a_nearly_singular_projection() {
        let eps: f64 = 0.001;
        let (a, b) = (0.001, 0.005);
        let primitive = |v: f64| {
            0.5 * v * (eps * eps + 4. * v * v).sqrt() + eps * eps / 4. * (2. * v / eps).asinh()
        };
        let area = 1.2 * (primitive(b) - primitive(a));
        let flux = -1.2 * eps * (b * b * b - a * a * a) / 3.;
        let n = 8192;
        let h = (b - a) / n as f64;
        let integrate = |power: i32| {
            let mut s = 0.;
            for j in 0..=n {
                let v = a + h * j as f64;
                let w = if j == 0 || j == n {
                    1.
                } else if j % 2 == 0 {
                    2.
                } else {
                    4.
                };
                s += w * v.powi(power) * (eps * eps + 4. * v * v).sqrt();
            }
            s * h / 3.
        };
        let means: [f64; 4] = std::array::from_fn(|j| integrate(j as i32 + 1) / integrate(0));
        let [v, v2, v3, v4] = means;
        let var = v2 - v * v;
        let cross = v3 - v * v2;
        let centre = [1. + v, eps * v, v2];
        let q = [
            [area * (0.12 + var), area * eps * var, area * cross],
            [area * eps * var, area * eps * eps * var, area * eps * cross],
            [area * cross, area * eps * cross, area * (v4 - v2 * v2)],
        ];
        let p = narrow_parabola();
        assert!(p.integration_pcurves(0).is_some(), "eligible primary trim");
        let mass = p.face_mass(0).expect("preferred trim has area");
        assert!((mass[0] - area).abs() < 1e-9 * area, "{mass:?} {area}");
        assert!(
            (mass[1] - flux).abs() < 1e-9 * flux.abs(),
            "{mass:?} {flux}"
        );
        let p = narrow_parabola();
        let m = p
            .face_moments(0)
            .expect("cold moments recover canonical mass first");
        assert_eq!(p.face_mass(0), Some(mass));
        assert!((m.area - area).abs() < 1e-9 * area);
        assert!(geom::dist(m.centroid, centre) < 1e-10, "{m:?} {centre:?}");
        for i in 0..3 {
            for j in 0..3 {
                assert!(
                    (m.second_moments[i][j] - q[i][j]).abs() < 1e-9 * area * 0.03,
                    "{m:?} {q:?}"
                );
            }
        }
        // Absence or ambiguity of an explicit master is not permission to use any pcurve.
        let p = narrow_parabola();
        let raw = std::collections::BTreeMap::from([(0, p.file_pcurves(0).to_vec())]);
        let p = p.with_file_pcurves(raw, Default::default(), (1., 1.), Some(0.002));
        assert!(p.face_mass(0).is_none());
        let p = narrow_parabola();
        let raw = std::collections::BTreeMap::from([(0, p.file_pcurves(0).to_vec())]);
        let p = p.with_file_pcurves(
            raw,
            std::collections::BTreeMap::from([(0, vec![0, 1, 2])]),
            (1., 1.),
            Some(0.002),
        );
        assert!(p.face_mass(0).is_none());
        let p = narrow_parabola();
        let raw = std::collections::BTreeMap::from([(0, p.file_pcurves(0).to_vec())]);
        let p = p.with_file_pcurves(
            raw,
            std::collections::BTreeMap::from([(0, vec![0, 1, 2, 3, 0])]),
            (1., 1.),
            Some(0.002),
        );
        assert!(p.face_mass(0).is_none());
    }

    #[test]
    fn master_trim_interior_disagreement_refuses_recovery() {
        // Retain the original regression's geometry: its 0.001005 mm
        // disagreement / ~0.604 mm box diagonal now correctly exceeds the band.
        let original = narrow_parabola_with_width(1.);
        assert!(original.integration_pcurves(0).is_none());
        assert!(original.face_mass(0).is_none());

        let p = narrow_parabola();
        assert!(p.integration_pcurves(0).is_some());
        let mut raw = p.file_pcurves(0).to_vec();
        // Same endpoints and knot domain as the lower horizontal edge, but a
        // quadratic interior bulge into the surface. Endpoint checks alone pass.
        raw[0].1 = RawCurve2d::BSpline {
            degree: 2,
            points: vec![[0.2, 0.001], [0.5, 0.5], [0.8, 0.001]],
            multiplicities: vec![3, 3],
            knots: vec![0., 1.2],
            weights: None,
        };
        let p = p.with_file_pcurves(
            std::collections::BTreeMap::from([(0, raw)]),
            std::collections::BTreeMap::from([(0, vec![0, 1, 2, 3])]),
            (1., 1.),
            Some(0.002),
        );
        let pc = raw_param_curve(&p.file_pcurves(0)[0].1).unwrap();
        for (t, end) in [(0., p.edges[0].start), (1.2, p.edges[0].end)] {
            let uv = pc.value(t);
            assert!(geom::dist(p.faces[0].surface.value(uv[0], uv[1]), end) < 0.002);
        }
        assert!(
            master_deviation(&p.faces[0].surface, &pc, &p.edges[0].curve, 0., 1.2).unwrap()
                / p.face_bounds(0).diagonal()
                > 0.001
        );
        assert!(p.integration_pcurves(0).is_none());
        assert!(p.face_mass(0).is_none());
        assert!(p.face_moments(0).is_none());
    }

    #[test]
    fn concurrent_first_queries_share_the_boundary_choice() {
        let expected = narrow_parabola().face_moments(0).unwrap();
        for _ in 0..8 {
            let p = narrow_parabola();
            let barrier = std::sync::Barrier::new(2);
            std::thread::scope(|scope| {
                let mass = scope.spawn(|| {
                    barrier.wait();
                    p.face_mass(0).unwrap()
                });
                let moments = scope.spawn(|| {
                    barrier.wait();
                    p.face_moments(0).unwrap()
                });
                let mass = mass.join().unwrap();
                let moments = moments.join().unwrap();
                assert_eq!(mass[0], moments.area);
                assert_eq!(moments.area, expected.area);
                assert_eq!(moments.centroid, expected.centroid);
                assert_eq!(moments.second_moments, expected.second_moments);
                assert!(p.cache[0].mass.get().unwrap().preferred);
            });
        }
    }

    #[test]
    fn inner_quadrature_refuses_an_unresolved_singularity() {
        let rule = IntegralRule {
            components: Components::Prefix(1),
            observe_mass: None,
        };
        assert!(integrate_inner(&[0., 1.], 1e-12, |t| [1. / (t - 0.3).powi(2)], rule).is_none());
        assert_eq!(
            integrate_inner(&[0.2], 0., |t| [t.exp()], rule),
            Some(([0.], 0.))
        );
        let (got, error) = integrate_inner(&[0., 1.], 1e-12, |t| [t.exp()], rule).unwrap();
        assert!((got[0] - (1f64.exp() - 1.)).abs() < 1e-12 && error <= 1e-12);
    }
    #[test]
    fn step_master_selection_and_rigid_motion_survive_the_retry() {
        let p = narrow_parabola();
        let raw = std::collections::BTreeMap::from([(0, p.file_pcurves(0).to_vec())]);
        let p = p.with_file_pcurves(raw, Default::default(), (1., 1.), Some(0.002));
        let (bytes, report) = crate::brep_write::write_step(
            &p,
            &crate::brep_write::WriteOptions::new("synthetic near-parallel tangents"),
        )
        .unwrap();
        assert!(report.pcurves_not_written.is_empty());
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains(".CURVE_3D."));
        // Author both STEP variants from our own polynomial geometry. The master
        // designation, not the mere presence of a pcurve, enables recovery.
        let unselected = crate::step::read_step(text.as_bytes()).unwrap();
        assert!(unselected.face_mass(0).is_none());
        let text = text.replace(".CURVE_3D.", ".PCURVE_S1.");
        let selected = crate::step::read_step(text.as_bytes()).unwrap();
        let m = selected
            .face_moments(0)
            .expect("reader retains explicit master");
        let (bytes, _) = crate::brep_write::write_step(
            &selected,
            &crate::brep_write::WriteOptions::new("recovered trim"),
        )
        .unwrap();
        let back = crate::step::read_step(&bytes).unwrap();
        let again = back
            .face_moments(0)
            .expect("writing preserves recovered master");
        assert!((again.area - m.area).abs() < 1e-9 * m.area);
        let fresh = crate::step::read_step(text.as_bytes()).unwrap();
        let (fresh_bytes, _) = crate::brep_write::write_step(
            &fresh,
            &crate::brep_write::WriteOptions::new("recovered trim"),
        )
        .unwrap();
        assert!(
            fresh.cache[0].mass.get().is_none(),
            "writing must not integrate"
        );
        assert_eq!(
            bytes, fresh_bytes,
            "written output must not depend on prior mass queries"
        );
        let at = [
            [0., -1., 0., 123.456],
            [1., 0., 0., -78.9],
            [0., 0., 1., 41.3],
        ];
        let moved = selected.transformed(&at).unwrap();
        let got = moved
            .face_moments(0)
            .expect("master metadata follows rigid motion");
        assert!((got.area - m.area).abs() < 1e-9 * m.area);
        let point = [
            -m.centroid[1] + at[0][3],
            m.centroid[0] + at[1][3],
            m.centroid[2] + at[2][3],
        ];
        assert!(geom::dist(got.centroid, point) < 1e-9);
        let q = std::array::from_fn::<_, 3, _>(|i| {
            std::array::from_fn::<_, 3, _>(|j| {
                (0..3)
                    .flat_map(|k| (0..3).map(move |l| at[i][k] * m.second_moments[k][l] * at[j][l]))
                    .sum::<f64>()
            })
        });
        for (row, want) in got.second_moments.iter().zip(q) {
            for (value, want) in row.iter().zip(want) {
                assert!((value - want).abs() < 1e-9 * m.area * 0.03);
            }
        }
    }
}
