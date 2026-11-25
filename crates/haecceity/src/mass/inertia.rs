//! Volume second moments from the signed boundary flux.
use super::{IntegralOrigin, Part, Surface, V3, geom};
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
        // Put one square root of the mass in each factor so large distances
        // need never be squared before a small volume can scale them down.
        let root_mass = self.volume.abs().sqrt();
        let [x, y, z] = std::array::from_fn(|i| {
            let offset = self.centroid[i] - point[i];
            if offset.is_finite() {
                root_mass * offset
            } else {
                root_mass * self.centroid[i] - root_mass * point[i]
            }
        });
        let shift = [
            y * y + z * z,
            x * x + z * z,
            x * x + y * y,
            -x * y,
            -x * z,
            -y * z,
        ];
        symmetric_tensor(|k, i, j| self.tensor[i][j] + self.volume.signum() * shift[k])
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

    fn placed(self, origin: &IntegralOrigin) -> Self {
        let at = &origin.placement;
        Self {
            centroid: origin.place_point(self.centroid),
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
    /// For shallow spherical faces the local chart also requires a radius of at
    /// most 1024 times the diameter of a disk with the same patch area. A remote
    /// assembly solid may be refused when its world-coordinate resolution is too coarse.
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
        // Geometry rebasing and source placements must respect the original
        // part's mass verdict and the boundary authority it selected.
        for &face in faces {
            self.face_terms(face)?;
        }
        if let Some(origin) = &self.integral_origin
            && self.boundary_policy != super::BoundaryPolicy::Recognition
        {
            if self.spherical_cap_solid(faces).is_none() {
                for &face in faces {
                    self.face_raw_moments(face).centre?;
                }
            }
            return Some(origin.source.solid_inertia(solid)?.placed(origin));
        }
        let (origin, length) = self.solid_integral_reference(faces)?;
        if Self::coarse_world_coordinates(origin, length) {
            if self.shallow_solid_reference(faces).is_none()
                && self.spherical_cap_solid(faces).is_none()
            {
                for &face in faces {
                    self.face_raw_moments(face).centre?;
                }
            }
            let local = self.rebase_solid_geometry(faces, origin)?;
            let mut inertia = local.solid_inertia(0)?;
            inertia.centroid = geom::add(origin, inertia.centroid);
            return inertia.is_measurable().then_some(inertia);
        }
        if let Some(cap) = self.spherical_cap_inertia(faces) {
            return cap.is_measurable().then_some(cap);
        }
        let (volume, offset) = self.solid_centroid_flux(faces, (origin, length))?;
        let remote = geom::norm(origin) > 32.0 * length;
        let local_origin = remote.then_some(origin);
        let centroid = geom::add(origin, offset);
        if !volume.is_finite() || volume == 0.0 || !centroid.iter().all(|x| x.is_finite()) {
            return None;
        }

        // div(q qᵢ qⱼ) = 5 qᵢ qⱼ for dimensionless offsets q about the centroid.
        // Restore the three remaining length factors after surface quadrature.
        let mut flux = Vec::with_capacity(faces.len());
        for &face in faces {
            let fc = &self.faces[face];
            let moments = |p: V3, n: V3| {
                let q = geom::scale(p, length.recip());
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
            };
            let second = if self.shallow_sphere(face, faces) {
                let [_, xx, yy, zz, xy, xz, yz] =
                    self.spherical_local_flux(face, (origin, offset), moments)?;
                [xx, yy, zz, xy, xz, yz]
            } else if matches!(fc.surface, Surface::Sphere { .. }) {
                self.spherical_second_flux(face, (origin, offset), length)?
            } else {
                let local = local_origin.map(|o| self.local_face_surface(face, o));
                let reference = if remote { offset } else { centroid };
                let [_, xx, yy, zz, xy, xz, yz] =
                    self.accepted_face_integral(face, local.as_ref(), |p, su, sv| {
                        moments(geom::sub(p, reference), geom::cross(su, sv))
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
}
