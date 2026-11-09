//! Geometry-dependent discovery refusals. Like thread and section-recess refusals, these
//! retain the source faces and reason in the document without issuing a feature claim.

use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DiscoveryRefusal {
    pub reason: String,
    pub faces: Vec<usize>,
}

impl DiscoveryRefusal {
    pub(crate) fn new(reason: impl ToString, faces: impl IntoIterator<Item = usize>) -> Self {
        let mut faces: Vec<_> = faces.into_iter().collect();
        faces.sort_unstable();
        faces.dedup();
        Self {
            reason: reason.to_string(),
            faces,
        }
    }
}

pub(crate) struct Analysis<R> {
    pub accepted: Vec<super::Occurrence<R>>,
    pub refusals: Vec<DiscoveryRefusal>,
}

impl<R> Default for Analysis<R> {
    fn default() -> Self {
        Self {
            accepted: Vec::new(),
            refusals: Vec::new(),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::features::support_patches::polygon_part;
    use crate::kernel::brep::{Part, Solid};

    #[test]
    fn discovery_refusals_publish_reasons_and_faces_without_feature_claims() {
        use crate::correspondence::{Recognition, fingerprint::published_fingerprint};
        use crate::features::{Features, evidence::EvidenceStatus};
        use crate::frames::{FrameRefusal, RecordFrame};
        use crate::published_records::PublishedFeatures;
        use crate::recognition::{FaceAccounting, document};
        use std::collections::BTreeMap;
        let part = island_pocket();
        let families = [
            "pad_refusals",
            "pocket_refusals",
            "prismatic_pocket_refusals",
            "section_passage_refusals",
            "oriented_slot_refusals",
            "reconciliation_refusals",
            "polygonal_boss_refusals",
        ];
        let features = PublishedFeatures::from(Features {
            discovery_refusals: families
                .into_iter()
                .map(|family| {
                    (
                        family,
                        vec![super::DiscoveryRefusal::new(
                            "unproved source roles",
                            [1, 0, 1],
                        )],
                    )
                })
                .collect(),
            ..Features::default()
        });
        let recognition = Recognition {
            fingerprints: published_fingerprint(&part, &features),
            features,
            frame: RecordFrame::Refused {
                reason: FrameRefusal::NoAnalyticDirection,
            },
            source_sha256: None,
            input_repairs: Vec::new(),
            body_evidence: BTreeMap::new(),
        };
        assert!(recognition.fingerprints.features.is_empty());
        let doc = document(&recognition, part.faces.len());
        assert_eq!(doc.records.len(), families.len());
        for record in &doc.records {
            assert!(record.is_refusal());
            assert_eq!(record.faces, [0, 1]);
            assert_eq!(record.parameters["reason"], "unproved source roles");
            assert_eq!(record.evidence_status, EvidenceStatus::Unproved);
        }
        for face in &doc.face_accounting.as_ref().unwrap()[..2] {
            let FaceAccounting::Refused { refusals } = face else {
                panic!("lost refusal")
            };
            assert_eq!(refusals.len(), families.len());
        }
        let roundtrip: crate::recognition::RecognitionDocument =
            serde_json::from_value(serde_json::to_value(&doc).unwrap()).unwrap();
        assert!(roundtrip.records.iter().all(|r| r.is_refusal()));
    }

    /// Join an independently built hole loop to its surrounding planar face, then rebuild
    /// the part so its topology caches describe the final solid.
    fn with_holes(mut part: Part, holes: &[(usize, usize)]) -> Part {
        for &(face, hole) in holes {
            let lp = part.faces[hole].loops[0].clone();
            part.faces[face].loops.push(lp);
        }
        for &(_, hole) in holes.iter().rev() {
            part.faces.remove(hole);
        }
        let solid = Solid {
            faces: (0..part.faces.len()).collect(),
        };
        Part::new(part.faces, part.edges, vec![solid])
    }

    /// A square pocket with a square island. Its floor connects two disjoint wall cycles.
    pub(crate) fn island_pocket() -> Part {
        let mut points = Vec::new();
        for (half, z) in [(10., 0.), (10., 4.), (6., 4.), (6., 1.), (2., 1.), (2., 4.)] {
            points.extend([
                [-half, -half, z],
                [half, -half, z],
                [half, half, z],
                [-half, half, z],
            ]);
        }
        let mut faces = vec![vec![3, 2, 1, 0]];
        for i in 0..4 {
            let next = (i + 1) % 4;
            faces.push(vec![i, next, next + 4, i + 4]);
        }
        faces.extend([vec![4, 5, 6, 7], vec![11, 10, 9, 8]]);
        for i in 0..4 {
            let next = (i + 1) % 4;
            faces.push(vec![8 + i, 8 + next, 12 + next, 12 + i]);
        }
        faces.extend([vec![12, 13, 14, 15], vec![19, 18, 17, 16]]);
        for i in 0..4 {
            let next = (i + 1) % 4;
            faces.push(vec![16 + i, 16 + next, 20 + next, 20 + i]);
        }
        faces.push(vec![20, 21, 22, 23]);
        with_holes(
            polygon_part(&points, &faces, true).unwrap(),
            &[(5, 6), (11, 12)],
        )
    }

    /// A narrow rectangular top on a sloping support. On the three-decimal grid, the
    /// higher-base wall lies within the pad tolerance of both transverse bounds.
    pub(crate) fn narrow_pad() -> Part {
        let a = 0.1234;
        let b = a + f64::from_bits(0.2f64.to_bits() + 8);
        let points = [
            [-5., -5., -6.],
            [5., -5., -6.],
            [5., 5., -6.],
            [-5., 5., -6.],
            [-5., -5., 5.],
            [5., -5., -5.],
            [5., 5., -5.],
            [-5., 5., 5.],
            [a, -1., -a],
            [b, -1., -b],
            [b, 1., -b],
            [a, 1., -a],
            [a, -1., 6.],
            [b, -1., 6.],
            [b, 1., 6.],
            [a, 1., 6.],
        ];
        let faces = [
            vec![3, 2, 1, 0],
            vec![0, 1, 5, 4],
            vec![1, 2, 6, 5],
            vec![2, 3, 7, 6],
            vec![3, 0, 4, 7],
            vec![4, 5, 6, 7],
            vec![11, 10, 9, 8],
            vec![8, 9, 13, 12],
            vec![9, 10, 14, 13],
            vec![10, 11, 15, 14],
            vec![11, 8, 12, 15],
            vec![12, 13, 14, 15],
        ];
        with_holes(polygon_part(&points, &faces, true).unwrap(), &[(5, 6)])
    }
}
