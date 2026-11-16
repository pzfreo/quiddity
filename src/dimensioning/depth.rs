//! Planned straight depths use the axis/entry-plane intersection, not the tilted rim's reach.
use crate::Part;
use crate::features::cylinders::CylinderEvidence;
use crate::features::holes::HoleRecord;
use crate::features::policy::round_measurement;
use crate::kernel::geom::{Surface, cross, dot, norm, sub};

pub(super) struct Depths<'a> {
    part: &'a Part,
    hole: &'a HoleRecord,
    cylinders: Vec<&'a CylinderEvidence>,
}

impl<'a> Depths<'a> {
    pub fn new(
        part: &'a Part,
        hole: &'a HoleRecord,
        faces: &[usize],
        cylinders: &'a [CylinderEvidence],
    ) -> Self {
        Self {
            part,
            hole,
            cylinders: cylinders
                .iter()
                .filter(|c| faces.contains(&c.face))
                .collect(),
        }
    }

    pub fn bore(&self) -> f64 {
        let bore = self.at_diameter(self.hole.diameter);
        let Some((start, _)) = self.bounds(&bore) else {
            return self.hole.depth;
        };
        // Recognition includes a blind hole's bottom relief in its deep station.
        let Some((_, end)) = self.bounds(&self.cylinders) else {
            return self.hole.depth;
        };
        self.axial_depth(&bore, start, end)
            .unwrap_or(self.hole.depth)
    }

    pub fn enlargement(&self, diameter: f64, nominal: f64) -> f64 {
        let mut remaining = self.at_diameter(diameter);
        let mut stages = Vec::new();
        // Identical enlargements can occur at opposite mouths. Keep their actual
        // connected walls separate before choosing the stage at the presented entry.
        while let Some(seed) = remaining.pop() {
            let mut stage = vec![seed];
            let mut edges = self.part.face_edges(seed.face);
            loop {
                let n = stage.len();
                remaining.retain(|c| {
                    let next = self.part.face_edges(c.face);
                    if next.iter().any(|e| edges.contains(e)) {
                        edges.extend(next);
                        stage.push(*c);
                        false
                    } else {
                        true
                    }
                });
                if stage.len() == n {
                    break;
                }
            }
            if let Some((start, end)) = self.bounds(&stage)
                && round_measurement(end - start, 2) == nominal
            {
                stages.push((start, end, stage));
            }
        }
        stages.sort_by(|a, b| a.0.total_cmp(&b.0));
        stages
            .first()
            .and_then(|(start, end, stage)| self.axial_depth(stage, *start, *end))
            .unwrap_or(nominal)
    }

    fn at_diameter(&self, diameter: f64) -> Vec<&'a CylinderEvidence> {
        self.cylinders
            .iter()
            .copied()
            .filter(|c| (c.diameter - diameter).abs() <= 1e-4 * diameter)
            .collect()
    }

    fn bounds(&self, cylinders: &[&CylinderEvidence]) -> Option<(f64, f64)> {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for c in cylinders {
            let sign = dot(c.direction, self.hole.axis);
            let (a, b) = (sign * c.s_lo, sign * c.s_hi);
            lo = lo.min(a.min(b));
            hi = hi.max(a.max(b));
        }
        (lo <= hi).then_some((lo, hi))
    }

    fn axial_depth(&self, cylinders: &[&CylinderEvidence], start: f64, end: f64) -> Option<f64> {
        let axis = self.hole.axis;
        let mut entries = Vec::new();
        for c in cylinders {
            // Use the geometric radius rather than the cylinder inventory's
            // quantised diameter when identifying a tilted rim's extreme.
            let radius = match &self.part.faces[c.face].surface {
                Surface::Cylinder { radius, .. } => *radius,
                _ => c.diameter / 2.,
            };
            let edges = self.part.face_edges(c.face);
            let adjacent: std::collections::BTreeSet<_> = edges
                .iter()
                .flat_map(|&e| self.part.edge_faces()[e].iter().copied())
                .collect();
            for f in adjacent {
                let face = &self.part.faces[f];
                let Surface::Plane { frame } = &face.surface else {
                    continue;
                };
                let cos = dot(frame.z, axis);
                let sin = norm(cross(frame.z, axis));
                // Perpendicular entries already have the right reading. A parallel
                // plane cannot intersect the axis at a unique entry station.
                if cos.abs() < 1e-9 || sin < 1e-9 {
                    continue;
                }
                let travel = dot(sub(frame.origin, c.axis_point), frame.z) / cos;
                let station = dot(c.axis_point, axis) + travel;
                let swing = radius * sin / cos.abs();
                // Only the plane trimming the entry extreme qualifies; a shared
                // internal shoulder or the opposite end is not the entry face.
                if (station - swing - start).abs() < 1e-6 && station < end {
                    entries.push(station);
                }
            }
        }
        // A nonplanar entry has no single entry plane defining this intersection.
        // Keep its recognised trimmed-cylinder extent rather than inventing a datum.
        let station = entries
            .into_iter()
            .min_by(|a, b| (a - start).abs().total_cmp(&(b - start).abs()))?;
        Some(round_measurement(end - station, 6))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blind_bore_uses_the_axis_intersection_but_a_nonplanar_entry_keeps_its_extent() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ap242/nist/nist_ctc_02_asme1_ap242-e2.stp.gz");
        let mut part = crate::read_step_file(&path).unwrap();
        let found = crate::features::recognise(&part);
        let mut hole = found
            .holes
            .iter()
            .zip(&found.defining["holes"])
            .find(|(_, faces)| faces.contains(&98))
            .unwrap()
            .0
            .clone();
        // Isolate CTC-02's cylindrical enlargement as a blind bore ending at
        // its shoulder, to exercise the same entry convention on bore.depth.
        hole.diameter = 115.;
        hole.depth = 23.49;
        hole.bottom = crate::features::holes::Bottom::Flat;
        let cylinders = crate::features::Context::new(&part).cylinders().to_vec();
        assert_eq!(
            Depths::new(&part, &hole, &[98, 99], &cylinders).bore(),
            21.484724
        );
        let Surface::Plane { frame } = part.faces[619].surface else {
            panic!("entry plane")
        };
        // Only the entry's surface classification changes; the recognised wall
        // evidence and its original depth stay fixed for the fallback check.
        part.faces[619].surface = Surface::Sphere { frame, radius: 1. };
        let depths = Depths::new(&part, &hole, &[98, 99], &cylinders);
        assert_eq!(depths.bore(), 23.49);
        assert_eq!(depths.enlargement(115., 23.49), 23.49);
    }
}
