//! A face whose parameter rectangle closes in space can have only seam and pole
//! uses. Those uses do not bound an ordinary Green integral in the lifted UV plane.
use super::{
    Components, Curve, DeclaredBoundary, Part, Surface, V3, estimate, gauss_nodes, geom,
    spend_work, split_panels,
};
use crate::nurbs::{NurbsSurface, SurfaceEvaluation, breaks};
use std::f64::consts::{PI, TAU};

impl Part {
    /// The outer sheet of a spindle torus closes at two axial points. A seam
    /// on that sheet identifies either the whole sweep or a cap ending at
    /// a circular rim. In both cases the missing side in UV is an axial point.
    pub(super) fn spindle_sweep(
        &self,
        face: usize,
        boundary: Option<&DeclaredBoundary>,
    ) -> Option<(f64, f64)> {
        let fc = &self.faces[face];
        let Surface::Torus {
            frame,
            major,
            minor,
        } = fc.surface
        else {
            return None;
        };
        if !(major > 0.0 && major < minor) {
            return None;
        }
        let rim = self.material_boundary(face)?;
        let pole = (minor * minor - major * major).sqrt();
        let limit = (-major / minor).acos();
        let tolerance = 1e-8 * minor;
        let cylindrical = |p| {
            let q = geom::sub(p, frame.origin);
            let z = geom::dot(q, frame.z);
            (geom::norm(geom::sub(q, geom::scale(frame.z, z))), z)
        };
        let rim_v = match rim.as_slice() {
            [] => None,
            &[(e, _)] => {
                let edge = &self.edges[e];
                let Curve::Circle {
                    frame: circle,
                    radius,
                } = edge.curve
                else {
                    return None;
                };
                let (radial, z) = cylindrical(circle.origin);
                if !edge.is_closed()
                    || radial > tolerance
                    || geom::norm(geom::cross(frame.z, circle.z)) > 1e-8
                    || ((radius - major).hypot(z) - minor).abs() > tolerance
                {
                    return None;
                }
                Some((e, z.atan2(radius - major)))
            }
            _ => return None,
        };
        let mut ends = [false; 2];
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for &p in fc
            .loops
            .iter()
            .flat_map(|lp| lp.edges.iter().flat_map(|&(e, _)| &self.edges[e].samples))
        {
            let (radial, z) = cylindrical(p);
            if ((radial - major).hypot(z) - minor).abs() > tolerance {
                return None;
            }
            ends[0] |= radial <= tolerance && (z - pole).abs() <= tolerance;
            ends[1] |= radial <= tolerance && (z + pole).abs() <= tolerance;
            let v = z.atan2(radial - major);
            lo = lo.min(v);
            hi = hi.max(v);
        }
        let latitude = |e, spatial| {
            let Some(trim) = boundary.and_then(|b| b.edges.get(&e)) else {
                return Some(spatial);
            };
            // Use the declared rim when it is itself a full latitude. An
            // arbitrary pcurve can change the domain and needs UV integration.
            let v = trim.full_u_latitude(spatial)?;
            (-limit..limit).contains(&v).then_some(v)
        };
        match (ends, rim_v) {
            ([true, true], None) => Some((-limit, limit)),
            ([true, false], Some((e, v))) if lo >= v - 1e-8 => Some((latitude(e, v)?, limit)),
            ([false, true], Some((e, v))) if hi <= v + 1e-8 => Some((-limit, latitude(e, v)?)),
            _ => None,
        }
    }

    pub(super) fn closed_spline(&self, face: usize) -> Option<&NurbsSurface> {
        let fc = &self.faces[face];
        let Surface::Freeform { surface, .. } = &fc.surface else {
            return None;
        };
        if let Some(&(first, sense)) = fc.loops.iter().find_map(|lp| lp.edges.first())
            && !fc
                .loops
                .iter()
                .any(|lp| lp.edges.contains(&(first, !sense)))
        {
            return None;
        }
        if !self.material_boundary(face)?.is_empty() {
            return None;
        }
        let (u0, u1, v0, v1) = surface.domain();
        // With clamped end knots the boundary curves are exactly the perimeter
        // control rows. A matching pair of rational curves must have proportional
        // weights. A pair of collapsed sides can be two different points.
        if surface.knots_u[0] != u0
            || surface.knots_u.last().copied() != Some(u1)
            || surface.knots_v[0] != v0
            || surface.knots_v.last().copied() != Some(v1)
            || !surface
                .weights
                .iter()
                .flatten()
                .all(|&w| w.is_finite() && w > 0.0)
        {
            return None;
        }
        let (min, max) = surface.control_bounds();
        let tolerance = 1e-12 * geom::dist(min, max);
        let side = |along_u: bool| {
            let points = &surface.control_points;
            let weights = &surface.weights;
            let (count, last) = if along_u {
                (points.len(), points[0].len() - 1)
            } else {
                (points[0].len(), points.len() - 1)
            };
            let at = |i: usize, k: usize| {
                let (u, v) = if along_u { (i, k) } else { (k, i) };
                (points[u][v], weights[u][v])
            };
            let (start0, w0) = at(0, 0);
            let (start1, w1) = at(0, last);
            let collapsed = (0..count).all(|i| {
                geom::dist(at(i, 0).0, start0) <= tolerance
                    && geom::dist(at(i, last).0, start1) <= tolerance
            });
            let joined = (0..count).all(|i| {
                let (p, a) = at(i, 0);
                let (q, b) = at(i, last);
                geom::dist(p, q) <= tolerance && (a / w0 - b / w1).abs() <= 1e-12 * (a / w0)
            });
            (collapsed || joined, joined)
        };
        let (u_closed, u_joined) = side(false);
        let (v_closed, v_joined) = side(true);
        (u_closed && v_closed && (u_joined || v_joined)).then_some(surface)
    }
}

pub(super) fn integrate_spline<const N: usize>(
    surface: &NurbsSurface,
    f: impl Fn(V3, V3, V3) -> [f64; N],
    components: Components,
) -> Option<[f64; N]> {
    let u_knots = breaks(&surface.knots_u, surface.degree_u);
    let v_knots = breaks(&surface.knots_v, surface.degree_v);
    let mut eval = SurfaceEvaluation::new(surface);
    integrate_rectangle(
        &u_knots,
        &v_knots,
        |u, v| eval.value_and_partials(u, v),
        f,
        components,
    )
}

pub(super) fn integrate_spindle<const N: usize>(
    surface: &Surface,
    (lo, hi): (f64, f64),
    f: impl Fn(V3, V3, V3) -> [f64; N],
    components: Components,
) -> Option<[f64; N]> {
    integrate_rectangle(
        &[0.0, PI, TAU],
        &[lo, 0.5 * (lo + hi), hi],
        |u, v| super::point_and_partials(surface, u, v),
        f,
        components,
    )
}

fn integrate_rectangle<const N: usize>(
    u_knots: &[f64],
    v_knots: &[f64],
    mut eval: impl FnMut(f64, f64) -> (V3, V3, V3),
    f: impl Fn(V3, V3, V3) -> [f64; N],
    components: Components,
) -> Option<[f64; N]> {
    let mut pass = |split| {
        let us = split_panels(u_knots, split);
        let vs = split_panels(v_knots, split);
        let mut sum = [0.0; N];
        let mut size = [0.0; N];
        for u in us.windows(2) {
            for v in vs.windows(2) {
                let uh = (u[1] - u[0]) * 0.5;
                let vh = (v[1] - v[0]) * 0.5;
                for &(x, wx) in gauss_nodes() {
                    for &(y, wy) in gauss_nodes() {
                        spend_work()?;
                        let (p, su, sv) = eval(u[0] + uh * (1. + x), v[0] + vh * (1. + y));
                        let values = f(p, su, sv);
                        for k in 0..N {
                            let term = (wx * wy * uh * vh) * values[k];
                            if !term.is_finite() {
                                return None;
                            }
                            sum[k] += term;
                            size[k] += term.abs();
                        }
                    }
                }
            }
        }
        Some((sum, size))
    };
    let (mut coarse, _) = pass(1)?;
    for split in [2, 4, 8] {
        let (fine, size) = pass(split)?;
        let (error, magnitude) = estimate(&fine, &coarse, &size, components);
        if error <= 1e-11 * magnitude {
            return Some(fine);
        }
        coarse = fine;
    }
    None
}
