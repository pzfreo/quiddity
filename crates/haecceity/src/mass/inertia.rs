//! Volume second moments from the signed boundary flux.
use super::{BoundaryMethod, Components, IntegralRule, Part, Surface, V3, geom};
use std::sync::OnceLock;

pub(crate) const SYMMETRIC: [(usize, usize); 6] = [(0, 0), (1, 1), (2, 2), (0, 1), (0, 2), (1, 2)];

type Tensor = [[f64; 3]; 3];

pub(crate) type SolidInertiaCache = OnceLock<Vec<OnceLock<Option<SolidInertia>>>>;

/// Signed solid inertia for uniform density 1, expressed in the part's coordinate axes.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct SolidInertia {
    /// Signed volume in mm³, evaluated about the centroid as in [`Part::volume_centroid`].
    /// Open seams within STEP tolerances can make it differ slightly from [`Part::solid_mass`].
    pub volume: f64,
    /// Centre of mass in mm.
    pub centroid: V3,
    /// Centroidal inertia in mm⁵. Diagonal entries are moments about the three axes;
    /// off-diagonal entries are negated products of inertia.
    pub tensor: [[f64; 3]; 3],
}

impl SolidInertia {
    /// Inertia in mm⁵ about axes through `point` (in mm), parallel to the part's coordinate axes.
    /// The parallel-axis term uses the same signed volume as the centroidal tensor.
    pub fn about(&self, point: V3) -> [[f64; 3]; 3] {
        let [x, y, z] = geom::sub(self.centroid, point);
        let shift = [
            y * y + z * z,
            x * x + z * z,
            x * x + y * y,
            -x * y,
            -x * z,
            -y * z,
        ];
        symmetric_tensor(|k, i, j| self.tensor[i][j] + self.volume * shift[k])
    }

    fn is_measurable(&self) -> bool {
        self.volume != 0.0
            && self.volume.is_finite()
            && self
                .centroid
                .iter()
                .chain(self.tensor.iter().flatten())
                .all(|x| x.is_finite())
    }

    fn placed(self, at: &crate::step::Placement) -> Self {
        Self {
            centroid: at.map(|r| geom::dot([r[0], r[1], r[2]], self.centroid) + r[3]),
            tensor: symmetric_tensor(|_, i, j| {
                (0..3)
                    .flat_map(|k| (0..3).map(move |l| at[i][k] * self.tensor[k][l] * at[j][l]))
                    .sum()
            }),
            ..self
        }
    }
}

pub(super) fn symmetric_tensor(mut term: impl FnMut(usize, usize, usize) -> f64) -> Tensor {
    let mut tensor = [[0.0; 3]; 3];
    for (k, (i, j)) in SYMMETRIC.into_iter().enumerate() {
        let value = term(k, i, j);
        tensor[i][j] = value;
        tensor[j][i] = value;
    }
    tensor
}

impl Part {
    /// The inertia of one solid, including its void shells, at uniform density 1.
    /// Integrates signed boundary flux, so reversing all its faces negates volume and tensor
    /// and preserves the centroid. [`SolidInertia::about`] shifts to any other point.
    /// Returns `None` for an absent solid, zero volume, or a refused face integral.
    /// The result is cached for this immutable part.
    pub fn solid_inertia(&self, solid: usize) -> Option<SolidInertia> {
        self.solids.get(solid)?;
        let cache = self
            .solid_inertias
            .get_or_init(|| self.solids.iter().map(|_| OnceLock::new()).collect());
        *cache[solid].get_or_init(|| self.compute_solid_inertia(solid))
    }

    fn compute_solid_inertia(&self, solid: usize) -> Option<SolidInertia> {
        let faces = self.solids[solid].faces.as_slice();
        if faces.iter().any(|&face| self.integration_refused(face)) {
            return None;
        }
        if let Some(origin) = &self.integral_origin
            && self.boundary_policy != super::BoundaryPolicy::Recognition
        {
            return Some(
                origin
                    .source
                    .solid_inertia(solid)?
                    .placed(&origin.placement),
            );
        }
        let only_solid = self.solids.len() == 1 && faces.len() == self.faces.len();
        let (origin, length) = if only_solid {
            self.integral_reference()?
        } else {
            // A small solid must not inherit the integration length and origin of an
            // unrelated body in the file.
            let mut bounds = geom::Bounds::empty();
            for &face in faces {
                let b = self.face_bounds(face);
                if b.diagonal().is_finite() {
                    bounds.merge(&b);
                } else if let Surface::Sphere { frame, radius } = self.faces[face].surface {
                    bounds.add(frame.origin.map(|x| x - radius));
                    bounds.add(frame.origin.map(|x| x + radius));
                } else if let Some(surface) = self.closed_spline(face) {
                    let (min, max) = surface.control_bounds();
                    bounds.add(min);
                    bounds.add(max);
                }
            }
            let length = bounds.diagonal();
            if !(length.is_finite() && length > 0.0) {
                return None;
            }
            (bounds.centre(), length)
        };
        // In a remote placement even forming p before subtracting the reference
        // loses digits. Translate the primitives before evaluating their points.
        if geom::norm(origin) > 32.0 * length {
            let mut local = crate::step::IDENTITY;
            for i in 0..3 {
                local[i][3] = -origin[i];
            }
            let mut result = self
                .transformed_geometry(&local)
                .ok()?
                .solid_inertia(solid)?;
            result.centroid = geom::add(result.centroid, origin);
            return Some(result);
        }
        if let Some(cap) = self.spherical_cap_inertia(faces) {
            return cap.is_measurable().then_some(cap);
        }
        let (volume, offset) = self.centroid_flux(std::iter::once(faces), origin, length, |f| {
            if only_solid {
                self.face_raw_moments(f).centre
            } else {
                self.inertia_centroid_face(f, origin, length)
            }
        })?;
        let centroid = geom::add(origin, offset);
        if !volume.is_finite() || volume == 0.0 || !centroid.iter().all(|x| x.is_finite()) {
            return None;
        }

        // div(q qᵢ qⱼ) = 5 qᵢ qⱼ for dimensionless offsets q about the centroid.
        // Restore the three remaining length factors after surface quadrature.
        let mut flux = Vec::with_capacity(faces.len());
        for &face in faces {
            let fc = &self.faces[face];
            let second = if matches!(fc.surface, Surface::Sphere { .. }) {
                self.spherical_second_flux(face, centroid, length)?
            } else {
                let [_, xx, yy, zz, xy, xz, yz] = self.inertia_integral(face, |p, su, sv| {
                    let n = geom::cross(su, sv);
                    let q = geom::scale(geom::sub(p, centroid), length.recip());
                    let weight = geom::dot(q, n);
                    let [x, y, z] = q;
                    [
                        geom::norm(n),
                        x * x * weight,
                        y * y * weight,
                        z * z * weight,
                        x * y * weight,
                        x * z * weight,
                        y * z * weight,
                    ]
                })?;
                [xx, yy, zz, xy, xz, yz]
            };
            let sense = if fc.reversed { -1.0 } else { 1.0 };
            let [xx, yy, zz, xy, xz, yz] = second;
            flux.push([yy + zz, xx + zz, xx + yy, -xy, -xz, -yz].map(|v| v * sense));
        }
        let scale = length.powi(3) / 5.0;
        let tensor = symmetric_tensor(|k, _, _| crate::py::fsum(flux.iter().map(|f| f[k])) * scale);
        let inertia = SolidInertia {
            volume,
            centroid,
            tensor,
        };
        inertia.is_measurable().then_some(inertia)
    }

    fn inertia_centroid_face(&self, face: usize, origin: V3, length: f64) -> Option<[f64; 26]> {
        if matches!(self.faces[face].surface, Surface::Sphere { .. }) {
            return Some(self.spherical_patch(face)?.volume_moments(origin, length));
        }
        self.inertia_integral(face, |p, su, sv| {
            let n = geom::cross(su, sv);
            let q = geom::scale(geom::sub(p, origin), length.recip());
            let qn = geom::dot(q, n);
            let mut out = [0.0; 26];
            out[0] = geom::norm(n);
            out[1..4].copy_from_slice(&n);
            out[13] = qn;
            for i in 0..3 {
                out[23 + i] = q[i] * qn;
                for j in 0..3 {
                    out[14 + 3 * i + j] = q[i] * n[j];
                }
            }
            out
        })
    }

    fn inertia_integral<const N: usize>(
        &self,
        face: usize,
        density: impl Fn(V3, V3, V3) -> [f64; N],
    ) -> Option<[f64; N]> {
        self.face_terms(face)?;
        let components = Components::Prefix(N);
        let method = self.cache[face].mass.get()?.method;
        if method == BoundaryMethod::CertifiedPatch
            && let Some(sum) = self.integrate_preferred_face(face, &density, components, false)
        {
            return Some(sum);
        }
        self.integrate_boundary(
            face,
            self.measured_boundary(face),
            None,
            density,
            IntegralRule {
                components,
                observe_mass: None,
            },
            &mut None,
            false,
        )
    }
}
