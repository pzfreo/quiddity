//! Additive references over final accepted claims. Never feed these back into recognition.
use std::collections::{BTreeMap, BTreeSet};

use super::Features;
use crate::kernel::{brep::Part, geom::Surface};

// The eight specific families consumed by the unified section-recess projection.
// Lexicographic order is also the stable tie-break for multiple exact sources.
const RECESS_SOURCES: [&str; 8] = [
    "channels",
    "edge_open_circular_pockets",
    "edge_open_prismatic_recesses",
    "pockets",
    "prismatic_pockets",
    "rectangular_blind_slots",
    "round_bottom_blind_slots",
    "section_passages",
];

fn face_set(faces: &[usize]) -> BTreeSet<usize> {
    faces.iter().copied().collect()
}

pub(super) fn mark(part: &Part, features: &mut Features) {
    let groove_for = |faces: &[usize]| {
        if faces.is_empty() {
            return None;
        }
        features
            .defining
            .get("annular_grooves")
            .into_iter()
            .flatten()
            .position(|groove| {
                faces.iter().all(|face| {
                    groove.contains(face)
                        && matches!(part.faces[*face].surface, Surface::Cylinder { .. })
                })
            })
            .map(|index| format!("annular_grooves/{index}"))
    };
    for (record, faces) in features
        .holes
        .iter_mut()
        .zip(features.defining.get("holes").into_iter().flatten())
    {
        record.superseded_by = groove_for(faces);
    }
    for (record, faces) in features
        .bosses
        .iter_mut()
        .zip(features.defining.get("bosses").into_iter().flatten())
    {
        record.superseded_by = groove_for(faces);
    }

    let mut sources = BTreeMap::new();
    for family in RECESS_SOURCES {
        for (index, faces) in features
            .defining
            .get(family)
            .into_iter()
            .flatten()
            .enumerate()
        {
            if !faces.is_empty() {
                sources
                    .entry(face_set(faces))
                    .or_insert_with(|| format!("{family}/{index}"));
            }
        }
    }
    for record in &mut features.section_recesses {
        record.source = sources
            .get(&face_set(record.evidence().defining_faces()))
            .cloned();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_metadata_preserves_pattern_faces_fingerprints_and_evidence() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/flat_floor_patterns.step.gz");
        let part = crate::read_step_file(&path).unwrap();
        let mut rec = crate::correspondence::caller_space_recognise(&part);
        assert!(!rec.features.hole_patterns.is_empty());
        let before = crate::recognition::document(&rec, part.faces.len());
        let unmarked = serde_json::to_value(&before).unwrap();
        let fingerprints = crate::correspondence::fingerprint::fingerprint(&part, &rec.features);
        for hole in &mut rec.features.holes {
            hole.superseded_by = Some("annular_grooves/0".into());
        }
        let after = crate::recognition::document(&rec, part.faces.len());
        assert_eq!(
            serde_json::to_value(fingerprints).unwrap(),
            serde_json::to_value(crate::correspondence::fingerprint::fingerprint(
                &part,
                &rec.features
            ))
            .unwrap()
        );
        let mut marked = serde_json::to_value(&after).unwrap();
        for record in marked["records"].as_array_mut().unwrap() {
            if record["family"] == "holes" {
                assert!(
                    record["parameters"]
                        .as_object_mut()
                        .unwrap()
                        .remove("superseded_by")
                        .is_some()
                );
            }
        }
        // The only permitted document change is the annotation itself. In particular,
        // embedded members still resolve to the original faces and evidence status.
        assert_eq!(unmarked, marked);
    }

    #[test]
    fn exact_source_ties_choose_family_then_numeric_index_and_ignore_face_order() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/golden_open_channels.step");
        let part = crate::read_step_file(&path).unwrap();
        let mut features = super::super::recognise(&part);
        let recess = features.section_recesses.first().unwrap().clone();
        let faces = recess.evidence().defining_faces().to_vec();
        let mut reversed = faces.clone();
        reversed.reverse();
        features.section_recesses = vec![recess];
        for family in RECESS_SOURCES {
            features.defining.insert(family, Vec::new());
        }
        // The reference contract permits multiple equal accepted claims; there is no
        // global uniqueness invariant across the source families. Check its tie-break
        // independently of whichever projection wins discovery on this fixture.
        features.defining.insert("pockets", vec![faces.clone()]);
        features
            .defining
            .insert("channels", vec![reversed.clone(), faces.clone()]);
        mark(&part, &mut features);
        assert_eq!(
            features.section_recesses[0].source.as_deref(),
            Some("channels/0")
        );
        features.defining.insert("channels", Vec::new());
        features.defining.insert("pockets", vec![reversed]);
        mark(&part, &mut features);
        assert_eq!(
            features.section_recesses[0].source.as_deref(),
            Some("pockets/0")
        );
        // A union of two partial sources, or a proper superset, is insufficient.
        features
            .defining
            .insert("pockets", vec![faces[..1].to_vec(), faces[1..].to_vec()]);
        mark(&part, &mut features);
        assert!(features.section_recesses[0].source.is_none());
        let mut superset = faces;
        superset.push(part.faces.len());
        features.defining.insert("pockets", vec![superset]);
        mark(&part, &mut features);
        assert!(features.section_recesses[0].source.is_none());
    }
}
