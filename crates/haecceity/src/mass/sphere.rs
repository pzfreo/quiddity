//! Integrate an oriented spherical patch in space. A repeated seam cancels before
//! quadrature, and the answer has no dependence on the carrier's parameter poles.
use super::{
    BoundaryMethod, Curve, DeclaredBoundary, FaceMass, Part, Surface, V3, adaptive, curve_cuts,
    edge_interval, geom, kronrod, spend_work, valid_mass,
};
use std::{
    collections::BTreeMap,
    f64::consts::{PI, TAU},
};

const WHOLE: f64 = 4.0 * PI;
use super::inertia::{SYMMETRIC, SolidInertia, symmetric_tensor};

const CUBIC: [(usize, usize, usize); 10] = [
    (0, 0, 0),
    (0, 0, 1),
    (0, 0, 2),
    (0, 1, 1),
    (0, 1, 2),
    (0, 2, 2),
    (1, 1, 1),
    (1, 1, 2),
    (1, 2, 2),
    (2, 2, 2),
];

#[derive(Clone, Copy, Debug)]
pub(crate) struct SpherePatch {
    centre: V3,
    radius: f64,
    sense: f64,
    area: f64,
    first: V3,
    second: [[f64; 3]; 3],
    method: BoundaryMethod,
    cap: Option<CircularCap>,
}

#[derive(Clone, Copy, Debug)]
struct CircularCap {
    axis: V3,
    rim_centre: V3,
    height: f64,
}

impl SpherePatch {
    pub(super) fn needs_circular_centroid_check(&self) -> bool {
        // A small solid angle concentrates the boundary terms while the
        // enclosed cap volume shrinks quadratically with area.
        self.cap.is_none()
            && self.method == BoundaryMethod::Projected
            && self.area <= 1e-4 * self.radius * self.radius
    }

    pub(super) fn matches_area(&self, area: f64) -> bool {
        (area - self.area).abs() <= 1e-8 * self.area
    }

    pub(super) fn transformed(
        self,
        centre: V3,
        turn: impl Fn(V3) -> V3,
        turn_tensor: impl Fn([[f64; 3]; 3]) -> [[f64; 3]; 3],
    ) -> Self {
        Self {
            centre,
            first: turn(self.first),
            second: turn_tensor(self.second),
            cap: self.cap.map(|cap| CircularCap {
                axis: turn(cap.axis),
                rim_centre: geom::add(centre, turn(geom::sub(cap.rim_centre, self.centre))),
                ..cap
            }),
            ..self
        }
    }

    fn signed_cap_volume(&self, cap: CircularCap, reversed: bool) -> (f64, V3) {
        let sense = if reversed { -self.sense } else { self.sense };
        let height = cap.height * self.radius;
        let volume = PI * height * height * (self.radius - height / 3.0);
        // Integral of circular slices, measured up from the observed rim.
        // Its in-plane position preserves a small offset from the carrier axis.
        let offset = height * (4.0 * self.radius - height) / (4.0 * (3.0 * self.radius - height));
        (
            sense * volume,
            geom::add(cap.rim_centre, geom::scale(cap.axis, offset)),
        )
    }

    fn cap_inertia(&self, cap: CircularCap, reversed: bool) -> SolidInertia {
        let (volume, centroid) = self.signed_cap_volume(cap, reversed);
        let r = self.radius;
        let h = r * cap.height;
        // Circular slices a distance z from the tip have squared radius z(2r − z).
        // Integrate their axial inertia and the variance of z over [0, h].
        let axial = volume * h * (4.0 * r * r / 3.0 - r * h + h * h / 5.0) / (2.0 * (r - h / 3.0));
        let mean = h * (2.0 * r / 3.0 - h / 4.0) / (r - h / 3.0);
        let mean_square = h * h * (r / 2.0 - h / 5.0) / (r - h / 3.0);
        let transverse = 0.5 * axial + volume * (mean_square - mean * mean);
        let tensor = symmetric_tensor(|_, i, j| {
            transverse * delta(i, j) + (axial - transverse) * cap.axis[i] * cap.axis[j]
        });
        SolidInertia {
            volume,
            centroid,
            tensor,
        }
    }

    fn mass(&self) -> Option<[f64; 5]> {
        let n = geom::scale(self.first, self.sense);
        valid_mass([
            self.area,
            self.sense * self.radius * self.area + geom::dot(self.centre, n),
            n[0],
            n[1],
            n[2],
        ])
    }

    pub(super) fn local_moments(&self) -> [f64; 26] {
        // In the face moment frame a point's coordinates are its unit radial direction.
        let mut out = [0.0; 26];
        out[0] = self.area;
        out[1..4].copy_from_slice(&geom::scale(self.first, self.sense));
        out[4..7].copy_from_slice(&self.first);
        for (k, (i, j)) in SYMMETRIC.into_iter().enumerate() {
            out[7 + k] = self.second[i][j];
        }
        out
    }

    pub(super) fn volume_moments(&self, origin: V3, scale: f64) -> [f64; 26] {
        let d = geom::scale(geom::sub(self.centre, origin), scale.recip());
        let r = self.radius / scale;
        let flux = geom::dot(d, self.first) + r * self.area;
        let mut out = self.local_moments();
        out[13] = self.sense * flux;
        for i in 0..3 {
            for j in 0..3 {
                out[14 + 3 * i + j] = self.sense * (d[i] * self.first[j] + r * self.second[i][j]);
            }
            out[23 + i] =
                self.sense * (d[i] * flux + r * (geom::dot(self.second[i], d) + r * self.first[i]));
        }
        out
    }

    pub(super) fn projected(&self, direction: V3, reversed: bool) -> Option<(f64, V3)> {
        let area = geom::dot(direction, self.first);
        if !area.is_finite() || area.abs() <= f64::EPSILON {
            return None;
        }
        let centroid = std::array::from_fn(|i| {
            self.centre[i] + self.radius * geom::dot(self.second[i], direction) / area
        });
        let sense = if reversed { -self.sense } else { self.sense };
        centroid
            .iter()
            .all(|x| x.is_finite())
            .then_some((sense * area, centroid))
    }
}

#[derive(Clone, Copy)]
struct OrientedCap<'a> {
    patch: &'a SpherePatch,
    cap: CircularCap,
    reversed: bool,
}

impl OrientedCap<'_> {
    fn volume_centroid(self) -> (f64, V3) {
        self.patch.signed_cap_volume(self.cap, self.reversed)
    }

    fn inertia(self) -> SolidInertia {
        self.patch.cap_inertia(self.cap, self.reversed)
    }
}

fn combine_cap_centroids((a, ca): (f64, V3), (b, cb): (f64, V3)) -> (f64, V3) {
    let volume = a + b;
    let centroid = geom::add(ca, geom::scale(geom::sub(cb, ca), b / volume));
    (volume, centroid)
}

impl Part {
    pub(super) fn spherical_cap_solid(&self, faces: &[usize]) -> Option<(f64, V3)> {
        let (a, b) = self.closed_spherical_caps(faces)?;
        let a = a.volume_centroid();
        Some(b.map_or(a, |b| combine_cap_centroids(a, b.volume_centroid())))
    }

    pub(super) fn spherical_cap_inertia(&self, faces: &[usize]) -> Option<SolidInertia> {
        let (a, b) = self.closed_spherical_caps(faces)?;
        let a = a.inertia();
        Some(b.map_or(a, |b| {
            let b = b.inertia();
            let (volume, centroid) =
                combine_cap_centroids((a.volume, a.centroid), (b.volume, b.centroid));
            let (ia, ib) = (a.about(centroid), b.about(centroid));
            let tensor = symmetric_tensor(|_, i, j| ia[i][j] + ib[i][j]);
            SolidInertia {
                volume,
                centroid,
                tensor,
            }
        }))
    }

    /// Recognise a cap with its disk, or two caps with the same rim, from their
    /// actual coedges. Sum their closed-form volumes and moments to keep the
    /// tiny moments of thin caps even when the separate fluxes nearly cancel.
    fn closed_spherical_caps(
        &self,
        faces: &[usize],
    ) -> Option<(OrientedCap<'_>, Option<OrientedCap<'_>>)> {
        let &[mut a, mut b] = faces else {
            return None;
        };
        if !matches!(self.faces[a].surface, Surface::Sphere { .. }) {
            if !matches!(self.faces[b].surface, Surface::Sphere { .. }) {
                return None;
            }
            std::mem::swap(&mut a, &mut b);
        }
        let patch = self.spherical_patch(a)?;
        let cap = patch.cap?;
        let a_edges = self.material_boundary(a)?;
        let b_edges = self.material_boundary(b)?;
        if a_edges.len() != b_edges.len()
            || !a_edges
                .iter()
                .zip(b_edges)
                .all(|(&(e, f), (g, h))| e == g && f != h)
        {
            return None;
        }
        let a = OrientedCap {
            patch,
            cap,
            reversed: self.faces[a].reversed,
        };
        let a_sense = if a.reversed {
            -a.patch.sense
        } else {
            a.patch.sense
        };
        let b_face = &self.faces[b];
        match b_face.surface {
            Surface::Plane { frame } => {
                self.face_terms(b)?;
                if self.measured_boundary(b).is_some() {
                    return None;
                }
                let mut normal = geom::cross(frame.x, frame.y);
                if b_face.reversed {
                    normal = geom::scale(normal, -1.0);
                }
                let off = geom::dot(geom::sub(frame.origin, cap.rim_centre), normal).abs();
                (off <= 128.0 * f64::EPSILON * patch.radius
                    && geom::dot(normal, cap.axis) * a_sense < -1.0 + 1e-12)
                    .then_some((a, None))
            }
            Surface::Sphere { .. } => {
                let other = self.spherical_patch(b)?;
                let other_cap = other.cap?;
                let b_sense = if b_face.reversed {
                    -other.sense
                } else {
                    other.sense
                };
                if a_sense != b_sense || geom::dot(cap.axis, other_cap.axis) >= -1.0 + 1e-12 {
                    return None;
                }
                Some((
                    a,
                    Some(OrientedCap {
                        patch: other,
                        cap: other_cap,
                        reversed: b_face.reversed,
                    }),
                ))
            }
            _ => None,
        }
    }

    pub(super) fn spherical_mass(&self, face: usize) -> FaceMass {
        self.spherical_patch(face).map_or_else(
            || FaceMass::legacy(None),
            |patch| FaceMass {
                terms: patch.mass(),
                method: patch.method,
            },
        )
    }

    pub(super) fn spherical_patch(&self, face: usize) -> Option<&SpherePatch> {
        if self.integration_refused(face) {
            return None;
        }
        self.ensure_file_integrals(face);
        self.cache[face]
            .sphere_patch
            .get_or_init(|| self.compute_spherical_patch(face).map(Box::new))
            .as_deref()
    }

    fn compute_spherical_patch(&self, face: usize) -> Option<SpherePatch> {
        let fc = &self.faces[face];
        let Surface::Sphere { frame, radius } = fc.surface else {
            return None;
        };
        if !(radius.is_finite() && radius > 0.0) {
            return None;
        }
        let edges = self.material_boundary(face)?;
        let sense = if frame.direct() { 1.0 } else { -1.0 };
        let orientation = if fc.reversed { -sense } else { sense };
        let (terms, method) = if edges.is_empty() {
            (
                [WHOLE, 0., 0., 0., 0., 0., 0., 0., 0., 0.],
                BoundaryMethod::Projected,
            )
        } else if let Some(boundary) = self.declared_boundary(face)
            && let Some(terms) =
                self.spherical_boundary::<10>(face, &edges, orientation, Some(boundary))
        {
            (terms, BoundaryMethod::Declared)
        } else if let Some((cap, axis)) =
            self.circular_cap(frame.origin, radius, &edges, orientation)
        {
            // A thin cap's transverse second moments are much smaller than its
            // area. Use the closed forms to avoid subtracting comparable
            // boundary terms and magnifying their round-off in the centroid.
            let height = cap.height;
            let area = TAU * radius * radius * height;
            let axial = area * (1.0 - height + height * height / 3.0);
            let transverse = area * height * (3.0 - height) / 6.0;
            let second = std::array::from_fn(|i| {
                std::array::from_fn(|j| {
                    let diagonal = if i == j { transverse } else { 0.0 };
                    diagonal + (axial - transverse) * axis[i] * axis[j]
                })
            });
            let patch = SpherePatch {
                centre: frame.origin,
                radius,
                sense,
                area,
                first: geom::scale(axis, area * (1.0 - 0.5 * height)),
                second,
                method: BoundaryMethod::Projected,
                cap: Some(cap),
            };
            patch.mass()?;
            return Some(patch);
        } else {
            (
                self.spherical_boundary::<10>(face, &edges, orientation, None)?,
                BoundaryMethod::Projected,
            )
        };
        let scale = radius * radius;
        let area = terms[0] * scale;
        let first = [terms[1], terms[2], terms[3]].map(|x| 0.5 * scale * x);
        let mut second = [[0.0; 3]; 3];
        for (k, (i, j)) in SYMMETRIC.into_iter().enumerate() {
            // On the unit sphere Δ(nᵢnⱼ) = 2δᵢⱼ − 6nᵢnⱼ. The outward
            // conormal along a positively oriented wire is dn × n.
            let v = scale * terms[4 + k] / 6.0 + if i == j { area / 3.0 } else { 0.0 };
            second[i][j] = v;
            second[j][i] = v;
        }
        let patch = SpherePatch {
            centre: frame.origin,
            radius,
            sense,
            area,
            first,
            second,
            method,
            cap: None,
        };
        patch.mass()?;
        Some(patch)
    }

    /// A circular rim, possibly split into arcs, and the axis of its projection
    /// on the sphere. Keep the original rim plane for measuring a closed solid.
    fn circular_cap(
        &self,
        centre: V3,
        sphere_radius: f64,
        edges: &[(usize, bool)],
        orientation: f64,
    ) -> Option<(CircularCap, V3)> {
        let Curve::Circle { frame, radius } = self.edges[edges.first()?.0].curve else {
            return None;
        };
        let tolerance = 128.0 * f64::EPSILON * sphere_radius;
        let d = geom::sub(frame.origin, centre);
        let skew = geom::norm(geom::cross(frame.z, d));
        // The first-order correction assumes a small shift relative to the
        // rim radius. Bound its quadratic remainder as well as the file gap.
        let off_axis = skew > tolerance;
        if off_axis
            && skew
                > self
                    .file_uncertainty()
                    .unwrap_or(geom::COORD_FLOOR)
                    .min(1e-4 * radius)
        {
            return None;
        }
        if (geom::norm(d).hypot(radius) - sphere_radius).abs() > tolerance {
            return None;
        }
        let mut sweep = 0.0;
        for &(e, forward) in edges {
            let edge = &self.edges[e];
            let Curve::Circle {
                frame: other,
                radius: r,
            } = edge.curve
            else {
                return None;
            };
            if geom::dist(frame.origin, other.origin) > tolerance
                || (r - radius).abs() > tolerance
                || geom::norm(geom::cross(frame.z, other.z)) > 128.0 * f64::EPSILON
            {
                return None;
            }
            let (a, b) = edge_interval(
                &edge.curve,
                edge.start,
                edge.end,
                edge.same_sense,
                edge.is_closed(),
            );
            let own_normal = geom::cross(other.x, other.y);
            let forward = if forward { 1.0 } else { -1.0 };
            sweep += forward * (b - a) * geom::dot(frame.z, own_normal);
        }
        if (sweep.abs() - TAU).abs() > 1e-9 {
            return None;
        }
        let axis = geom::scale(frame.z, (orientation * sweep).signum());
        let c = (geom::dot(axis, d) / sphere_radius).clamp(-1.0, 1.0);
        let height = if c > 0.0 {
            (radius / sphere_radius).powi(2) / (1.0 + c)
        } else {
            1.0 - c
        };
        let projected_axis = if off_axis {
            // For polar angle θ, the first-order displacement in θ is cos θ
            // times the transverse shift along that meridian, divided by R.
            let transverse = geom::sub(d, geom::scale(axis, geom::dot(axis, d)));
            geom::unit(geom::add(axis, geom::scale(transverse, c / sphere_radius)))?
        } else {
            axis
        };
        Some((
            CircularCap {
                axis,
                height,
                rim_centre: frame.origin,
            },
            projected_axis,
        ))
    }

    /// The boundary as oriented coedges, with opposite uses of a seam removed.
    /// Duplicate uses in the same sense, or more than two uses, cannot define a patch.
    pub(super) fn material_boundary(&self, face: usize) -> Option<Vec<(usize, bool)>> {
        let mut uses = BTreeMap::new();
        for &(e, forward) in self.faces[face].loops.iter().flat_map(|lp| &lp.edges) {
            match uses.entry(e).or_insert((0, forward)) {
                (count @ 0, _) => *count = 1,
                (count @ 1, first) if *first != forward => *count = 2,
                _ => return None,
            }
        }
        Some(
            uses.into_iter()
                .filter_map(|(e, (count, forward))| (count == 1).then_some((e, forward)))
                .collect(),
        )
    }

    /// ∯ qᵢ qⱼ (q·n) dA using the same spatial patch as mass. Write q = d + r u
    /// with u the unit radial direction, then contract its first three area moments.
    pub(super) fn spherical_second_flux(
        &self,
        face: usize,
        origin: V3,
        length: f64,
    ) -> Option<[f64; 6]> {
        let patch = self.spherical_patch(face)?;
        let third = self.spherical_third_moments(face, patch)?;
        let d = geom::scale(geom::sub(patch.centre, origin), length.recip());
        let r = patch.radius / length;
        let flux = geom::dot(d, patch.first) + r * patch.area;
        let first: V3 = std::array::from_fn(|i| geom::dot(patch.second[i], d) + r * patch.first[i]);
        Some(SYMMETRIC.map(|(i, j)| {
            let second =
                (0..3).map(|k| d[k] * third[i][j][k]).sum::<f64>() + r * patch.second[i][j];
            patch.sense
                * (d[i] * d[j] * flux + r * (d[i] * first[j] + d[j] * first[i]) + r * r * second)
        }))
    }

    fn spherical_third_moments(
        &self,
        face: usize,
        patch: &SpherePatch,
    ) -> Option<[[[f64; 3]; 3]; 3]> {
        let mut third = [[[0.0; 3]; 3]; 3];
        let mut insert = |(i, j, k): (usize, usize, usize), value: f64| {
            for (a, b, c) in [
                (i, j, k),
                (i, k, j),
                (j, i, k),
                (j, k, i),
                (k, i, j),
                (k, j, i),
            ] {
                third[a][b][c] = value;
            }
        };
        if let Some(cap) = patch.cap {
            let c = 1.0 - cap.height;
            let axial = patch.area * (1.0 + c) * (1.0 + c * c) / 4.0;
            let mixed = patch.area * cap.height * (1.0 + c).powi(2) / 8.0;
            if let Some(axis) = geom::unit(patch.first) {
                for (i, j, k) in CUBIC {
                    let trace =
                        delta(i, j) * axis[k] + delta(i, k) * axis[j] + delta(j, k) * axis[i];
                    insert(
                        (i, j, k),
                        (axial - 3.0 * mixed) * axis[i] * axis[j] * axis[k] + mixed * trace,
                    );
                }
            }
        } else {
            let edges = self.material_boundary(face)?;
            if !edges.is_empty() {
                let orientation = if self.faces[face].reversed {
                    -patch.sense
                } else {
                    patch.sense
                };
                let boundary = self.measured_boundary(face);
                let terms = self.spherical_boundary::<20>(face, &edges, orientation, boundary)?;
                for (index, (i, j, k)) in CUBIC.into_iter().enumerate() {
                    // Δ(uᵢuⱼuₖ) = 2(δᵢⱼuₖ + δᵢₖuⱼ + δⱼₖuᵢ) − 12uᵢuⱼuₖ.
                    let trace = delta(i, j) * patch.first[k]
                        + delta(i, k) * patch.first[j]
                        + delta(j, k) * patch.first[i];
                    insert(
                        (i, j, k),
                        (2.0 * trace + patch.radius.powi(2) * terms[10 + index]) / 12.0,
                    );
                }
            }
        }
        Some(third)
    }

    fn spherical_boundary<const N: usize>(
        &self,
        face: usize,
        edges: &[(usize, bool)],
        orientation: f64,
        boundary: Option<&DeclaredBoundary>,
    ) -> Option<[f64; N]> {
        let surface = &self.faces[face].surface;
        let Surface::Sphere { frame, radius } = *surface else {
            return None;
        };
        // Place the area form's singularity at -axis, away from the boundary.
        let directions: Vec<_> = edges
            .iter()
            .flat_map(|&(e, _)| {
                self.edges[e]
                    .samples
                    .iter()
                    .filter_map(|&p| geom::unit(geom::sub(p, frame.origin)))
            })
            .collect();
        let clearance = |axis| {
            directions
                .iter()
                .map(|&n| geom::dot(axis, n))
                .fold(1.0, f64::min)
        };
        let axis = [frame.x, frame.y, frame.z]
            .into_iter()
            .flat_map(|v| [v, geom::scale(v, -1.0)])
            .max_by(|&a, &b| clearance(a).total_cmp(&clearance(b)))?;
        let density = |p: V3, dp: V3| {
            spend_work()?;
            // Radial projection is p / |p|, so n × dn = p × dp / |p|².
            let length = geom::norm(p);
            let n = geom::scale(p, length.recip());
            let turn = geom::scale(geom::cross(p, dp), (length * length).recip());
            let den = 1.0 + geom::dot(axis, n);
            if den <= 1e-4 {
                return None;
            }
            let mut out = [0.0; N];
            out[0] = geom::dot(axis, turn) / den;
            out[1..4].copy_from_slice(&turn);
            for (k, (i, j)) in SYMMETRIC.into_iter().enumerate() {
                out[4 + k] = turn[i] * n[j] + turn[j] * n[i];
            }
            if N > 10 {
                for (index, (i, j, k)) in CUBIC.into_iter().enumerate() {
                    out[10 + index] =
                        turn[i] * n[j] * n[k] + turn[j] * n[i] * n[k] + turn[k] * n[i] * n[j];
                }
            }
            Some(out)
        };
        let integral = |cuts: &[f64], term: &dyn Fn(f64) -> Option<[f64; N]>| {
            let mut panel = |(), a, b, _| Some((kronrod((), a, b, |(), t| term(t))?, None));
            let floor = 2e-13 / edges.len() as f64;
            let (sum, excess, _, ()) = adaptive(cuts, (), Some(floor), true, &mut panel)?;
            (excess <= floor).then_some(sum)
        };
        let mut result = [0.0; N];
        for &(e, forward) in edges {
            if self.unresolved_edges().binary_search(&e).is_ok() {
                return None;
            }
            let edge = &self.edges[e];
            let trim = boundary.and_then(|b| b.edges.get(&e));
            let cuts = if let Some(trim) = trim {
                trim.nodes()?
            } else {
                let (a, b) = edge_interval(
                    &edge.curve,
                    edge.start,
                    edge.end,
                    edge.same_sense,
                    edge.is_closed(),
                );
                curve_cuts(surface, &edge.curve, a, b)?
            };
            let at = |t| {
                if let Some(trim) = trim {
                    let (uv, duv) = trim.checked_uv_and_derivative(surface, t)?;
                    // Evaluate about the centre so a large coordinate origin is irrelevant.
                    let centred = Surface::Sphere {
                        frame: geom::Frame {
                            origin: [0.0; 3],
                            ..frame
                        },
                        radius,
                    };
                    let (p, su, sv) = super::point_and_partials(&centred, uv[0], uv[1]);
                    Some((
                        p,
                        geom::add(geom::scale(su, duv[0]), geom::scale(sv, duv[1])),
                    ))
                } else {
                    Some((
                        geom::sub(edge.curve.value(t), frame.origin),
                        edge.curve.derivative(t),
                    ))
                }
            };
            let edge_sum = integral(&cuts, &|t| {
                let (p, dp) = at(t)?;
                density(p, dp)
            })?;
            // Close the little gaps between each edge's evaluated ends and its vertices.
            // A straight chord before radial projection gives the short great-circle arc.
            let start = geom::sub(edge.start, frame.origin);
            let end = geom::sub(edge.end, frame.origin);
            let p = at(*cuts.first()?)?.0;
            let q = at(*cuts.last()?)?.0;
            let gap = |from, to| {
                let dp = geom::sub(to, from);
                if geom::norm(dp) <= 1e-14 * radius {
                    return Some([0.0; N]);
                }
                integral(&[0., 1.], &|t| {
                    density(geom::add(from, geom::scale(dp, t)), dp)
                })
            };
            let first = gap(start, p)?;
            let last = gap(q, end)?;
            let sign = if forward { orientation } else { -orientation };
            for k in 0..N {
                result[k] += sign * (first[k] + edge_sum[k] + last[k]);
            }
        }
        // The area form misses one point on the sphere. If that point lies inside
        // the oriented patch the boundary integral is its area minus 4π.
        if result[0] < 0.0 {
            result[0] += WHOLE;
        }
        (result[0] > 0.0 && result[0] < WHOLE).then_some(result)
    }
}

fn delta(i: usize, j: usize) -> f64 {
    f64::from(i == j)
}
