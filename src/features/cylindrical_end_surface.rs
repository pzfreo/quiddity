//! Kernel-free cylindrical end values and complete polygon-domain bounds
//! (`quiddity._cylindrical_end_surface`).
//!
//! The cylinder axis lies in the section plane. Heights are measured along the frame's run.
//! A section end publishes this value through its explicitly tagged surface contract.

use serde::Serialize;

use super::placed::Placed;
use super::sections::{SectionError, V2};
use crate::kernel::geom::V3;
use crate::kernel::py;

type Checked<T> = Result<T, SectionError>;

fn refuse<T>(message: &'static str) -> Checked<T> {
    Err(SectionError(message))
}

fn finite(values: &[f64]) -> bool {
    values.iter().all(|v| v.is_finite())
}

/// One explicit branch of a cylinder perpendicular to the section run (`CylindricalEndSurface`).
///
/// The axis point is the point on the axis closest to the section-frame origin. Direction is
/// normalized on interpretation after six-decimal serialization. Python also refuses a boolean
/// radius by type; the port's types cannot carry one.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct CylindricalEndSurface {
    #[serde(rename = "type")]
    kind: &'static str,
    axis_point: Placed<V3>,
    axis_direction: V2,
    radius: f64,
    branch: &'static str,
}

impl CylindricalEndSurface {
    /// The value, refused (with Python's message) unless it is a cylinder on an explicit branch
    /// with finite six-decimal values, a unit canonical-sign direction and the axis point closest
    /// to the frame origin (`__post_init__`).
    pub fn new(
        kind: &str,
        axis_point: Placed<V3>,
        axis_direction: V2,
        radius: f64,
        branch: &str,
    ) -> Checked<Self> {
        let placed = axis_point;
        let axis_point = *placed;
        let branch = match branch {
            "positive" => "positive",
            "negative" => "negative",
            _ => return refuse("cylindrical end needs an explicit cylinder type and branch"),
        };
        if kind != "cylinder" {
            return refuse("cylindrical end needs an explicit cylinder type and branch");
        }
        let values = [
            axis_point[0],
            axis_point[1],
            axis_point[2],
            axis_direction[0],
            axis_direction[1],
            radius,
        ];
        if !finite(&values) || radius <= 0.0 {
            return refuse("cylindrical end needs finite axis values and positive radius");
        }
        if values.iter().any(|&v| py::round_to(v, 6) != v) {
            return refuse("cylindrical end values serialize at six decimal places");
        }
        let norm = py::hypot(&axis_direction);
        if (norm - 1.0).abs() > 2e-6 {
            return refuse("cylinder axis direction must be unit length");
        }
        // The larger component, ties to the second.
        let dominant = if axis_direction[0].abs() > axis_direction[1].abs() {
            0
        } else {
            1
        };
        if axis_direction[dominant] <= 0.0 {
            return refuse("cylinder axis direction must have canonical sign");
        }
        let along = 0.0 + axis_point[0] * axis_direction[0] + axis_point[1] * axis_direction[1];
        if along.abs() > 2e-6 {
            return refuse("cylinder axis point must be closest to the frame origin");
        }
        Ok(CylindricalEndSurface {
            kind: "cylinder",
            axis_point: placed,
            axis_direction,
            radius,
            branch,
        })
    }

    /// Which branch of the cylinder the end lies on: `"positive"` or `"negative"`.
    pub fn branch(&self) -> &'static str {
        self.branch
    }

    /// The axis point: its section coordinates, then its height along the run.
    pub fn axis_point(&self) -> V3 {
        *self.axis_point
    }

    /// The axis point with the values it was published from.
    pub fn placed_axis_point(&self) -> Placed<V3> {
        self.axis_point
    }

    pub fn axis_direction(&self) -> V2 {
        self.axis_direction
    }

    pub fn radius(&self) -> f64 {
        self.radius
    }

    /// The signed distance of a section point from the axis, across it (`_offset`).
    fn offset(&self, point: V2) -> Checked<f64> {
        if !finite(&point) {
            return refuse("section point must contain two finite values");
        }
        let [x, y] = self.axis_direction;
        Ok(
            (-y * (point[0] - self.axis_point[0]) + x * (point[1] - self.axis_point[1]))
                / py::hypot(&[x, y]),
        )
    }

    /// The branch's height at *offset* from the axis (`_height`).
    fn height_at(&self, offset: f64) -> Checked<f64> {
        // Factored discriminant avoids cancellation near the branch boundary.
        let discriminant = (self.radius - offset.abs()) * (self.radius + offset.abs());
        if discriminant <= 0.0 {
            return refuse("cylinder branch must exist strictly over the complete profile");
        }
        let sign = if self.branch == "positive" { 1.0 } else { -1.0 };
        Ok(self.axis_point[2] + sign * discriminant.sqrt())
    }

    /// The end's height over a section point (`height`).
    pub fn height(&self, point: V2) -> Checked<f64> {
        self.height_at(self.offset(point)?)
    }

    /// Exact height bounds for an already validated closed line-only polygon
    /// (`polygon_height_bounds`).
    ///
    /// The transverse coordinate is affine, so its extrema occur at vertices. The interior may
    /// cross the cylinder crest even when no vertex lies there.
    pub fn polygon_height_bounds(&self, points: &[V2]) -> Checked<(f64, f64)> {
        if points.len() < 3 {
            return refuse("height bounds need a complete polygon");
        }
        let offsets: Vec<f64> = points
            .iter()
            .map(|&p| self.offset(p))
            .collect::<Checked<_>>()?;
        let low = offsets.iter().copied().fold(f64::INFINITY, f64::min);
        let high = offsets.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let farthest = low.abs().max(high.abs());
        let nearest = if low <= 0.0 && 0.0 <= high {
            0.0
        } else {
            low.abs().min(high.abs())
        };
        let heights = [self.height_at(farthest)?, self.height_at(nearest)?];
        Ok((heights[0].min(heights[1]), heights[0].max(heights[1])))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No Python test asks for bounds over fewer than three points; Python's answer to
    /// `CylindricalEndSurface("cylinder", (0, 0, 0), (1, 0), 20, "positive")
    /// .polygon_height_bounds(((0, 0), (1, 1)))` is this refusal.
    #[test]
    fn bounds_over_an_incomplete_polygon_are_refused() {
        let surface =
            CylindricalEndSurface::new("cylinder", [0.0; 3].into(), [1.0, 0.0], 20.0, "positive")
                .unwrap();
        let refused = surface.polygon_height_bounds(&[[0.0, 0.0], [1.0, 1.0]]);
        assert_eq!(
            refused.unwrap_err().0,
            "height bounds need a complete polygon"
        );
    }
}
