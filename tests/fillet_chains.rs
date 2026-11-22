//! #341: accepted fillet faces, tangent components, and radius groups.

#[path = "common/public.rs"]
mod common;

use std::collections::{BTreeMap, BTreeSet};

use quiddity::correspondence::recognise;
use quiddity::kernel::brep::Arc;
use quiddity::recognition::{RecognitionDocument, document};
use quiddity::{Part, read_step_file};
use serde_json::Value;

fn check_partition(part: &Part, doc: &RecognitionDocument) {
    let rows = |family: &str| {
        doc.records
            .iter()
            .filter(|r| r.family == family)
            .collect::<Vec<_>>()
    };
    let (fillets, chains, groups) = (
        rows("fillets"),
        rows("fillet_chains"),
        rows("fillet_groups"),
    );
    assert!(!fillets.is_empty(), "the part must contain legacy fillets");
    assert!(!chains.is_empty(), "the part must contain fillet chains");
    assert!(!groups.is_empty(), "the part must contain radius groups");
    let index_list = |p: &Value| serde_json::from_value::<Vec<usize>>(p.clone()).unwrap();
    let face_list = |r: &quiddity::recognition::Record| {
        let faces = index_list(&r.parameters["faces"]);
        assert!(!faces.is_empty(), "{}", r.id);
        assert!(faces.is_sorted_by(|a, b| a < b), "{}", r.id);
        assert_eq!(faces, r.faces, "{}", r.id);
        faces
    };
    let mut owner = BTreeMap::new();
    for (i, fillet) in fillets.iter().enumerate() {
        for f in face_list(fillet) {
            assert!(f < part.faces.len() && owner.insert(f, i).is_none());
        }
    }
    let mut chain_of = BTreeMap::new();
    for (i, chain) in chains.iter().enumerate() {
        let faces = face_list(chain);
        for &face in &faces {
            assert!(chain_of.insert(face, i).is_none());
        }
        let expected_fillets: BTreeSet<_> = faces.iter().map(|f| owner[f]).collect();
        assert_eq!(
            index_list(&chain.parameters["fillets"]),
            expected_fillets.into_iter().collect::<Vec<_>>()
        );
        for edge in index_list(&chain.parameters["edges"]) {
            let incident = &part.edge_faces()[edge];
            assert_eq!(incident.len(), 2);
            assert_eq!(incident.iter().filter(|f| faces.contains(f)).count(), 1);
            assert_eq!(incident.iter().filter(|f| owner.contains_key(f)).count(), 1);
        }
    }
    assert_eq!(
        chain_of.keys().collect::<Vec<_>>(),
        owner.keys().collect::<Vec<_>>()
    );
    for fillet in &fillets {
        let expected: BTreeSet<_> = fillet.faces.iter().map(|f| chain_of[f]).collect();
        assert_eq!(
            index_list(&fillet.parameters["chains"]),
            expected.into_iter().collect::<Vec<_>>()
        );
    }
    let mut group_faces = BTreeSet::new();
    let mut grouped_chains = BTreeSet::new();
    for group in groups {
        let faces = face_list(group);
        let members = index_list(&group.parameters["chains"]);
        assert!(!members.is_empty());
        assert_eq!(group.parameters["chain_count"], members.len());
        assert_eq!(group.parameters["face_count"], faces.len());
        let mut union = Vec::new();
        for i in members {
            assert!(grouped_chains.insert(i));
            assert_eq!(group.parameters["side"], chains[i].parameters["side"]);
            union.extend_from_slice(&chains[i].faces);
        }
        union.sort_unstable();
        assert_eq!(union, faces);
        for face in faces {
            assert!(group_faces.insert(face));
        }
    }
    assert_eq!(grouped_chains.len(), chains.len());
    assert_eq!(group_faces, owner.into_keys().collect());
}

#[test]
fn cgb202_pins_side_groups_and_legacy_radius_totals() {
    let Some(corpus) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        eprintln!("SKIPPED cgb202 group counts: QUIDDITY_CORPUS unavailable");
        return;
    };
    let part = read_step_file(&corpus.join("cadgenbench_inputs/cgb202.step.gz")).unwrap();
    let doc = document(&recognise(&part), part.faces.len());
    let chains: Vec<_> = doc
        .records
        .iter()
        .filter(|r| r.family == "fillet_chains")
        .collect();
    assert_eq!(chains.len(), 667, "cgb202 chain count");
    let mut by_side = BTreeMap::new();
    let mut by_radius = BTreeMap::new();
    for g in doc.records.iter().filter(|r| r.family == "fillet_groups") {
        let p = &g.parameters;
        // Milli-mm keys reproduce the issue's displayed precision for these known radii.
        let radius = (1000. * p["radius"].as_f64().unwrap()).round() as u32;
        let side = p["side"].as_str().unwrap();
        let old_records: BTreeSet<_> = p["chains"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|i| {
                chains[i.as_u64().unwrap() as usize].parameters["fillets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| r.as_u64().unwrap())
            })
            .collect();
        assert!(
            by_side
                .insert((radius, side), (old_records.len(), g.faces.len()))
                .is_none()
        );
        *by_radius.entry(radius).or_insert(0) += old_records.len();
    }
    // The issue's counts refer to legacy records. Some of those records own several faces.
    assert_eq!(
        by_side,
        BTreeMap::from([
            ((1000, "concave"), (3, 7)),
            ((1000, "convex"), (6, 6)),
            ((2000, "convex"), (50, 66)),
            ((2500, "concave"), (7, 7)),
            ((3000, "concave"), (353, 558)),
            ((3000, "convex"), (28, 28)),
            ((4000, "concave"), (59, 59)),
            ((4000, "convex"), (1, 11)),
            ((6000, "concave"), (22, 25)),
            ((6000, "convex"), (1, 1)),
            ((9598, "convex"), (1, 1)),
            ((30000, "concave"), (2, 3)),
        ])
    );
    assert_eq!(
        by_radius,
        BTreeMap::from([
            (1000, 9),
            (2000, 50),
            (2500, 7),
            (3000, 381),
            (4000, 60),
            (6000, 23),
            (9598, 1),
            (30000, 2),
        ])
    );
}

#[test]
fn sharp_mitres_split_adjacent_equal_radius_fillets() {
    let path = common::fixtures().join("large_block_small_fillets.step");
    let part = read_step_file(&path).unwrap();
    let doc = document(&recognise(&part), part.faces.len());
    assert_eq!(part.arc(2, 8), Some(Arc::Convex));
    assert_eq!(part.shared_edges(2, 8).len(), 1);
    let chains: Vec<_> = doc
        .records
        .iter()
        .filter(|r| r.family == "fillet_chains")
        .collect();
    assert_eq!(
        chains
            .iter()
            .map(|c| c.faces.as_slice())
            .collect::<Vec<_>>(),
        [&[2], &[6], &[8], &[9]],
        "sharp mitres split the four equal-radius fillets"
    );
    for c in chains {
        assert_eq!(c.parameters["radius"], 1.0);
        assert_eq!(c.parameters["side"], "convex");
        assert_eq!(c.parameters["edges"].as_array().unwrap().len(), 2);
    }
    check_partition(&part, &doc);
}

#[test]
fn accepted_fillet_faces_partition_on_three_corpus_parts() {
    let Some(corpus) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        eprintln!("SKIPPED fillet coverage: QUIDDITY_CORPUS unavailable");
        return;
    };
    for (name, expected) in [("cgb202", 772), ("cgb242", 312), ("cgb243", 66)] {
        let path = corpus.join(format!("cadgenbench_inputs/{name}.step.gz"));
        let part = read_step_file(&path).unwrap();
        let doc = document(&recognise(&part), part.faces.len());
        check_partition(&part, &doc);
        let faces: usize = doc
            .records
            .iter()
            .filter(|r| r.family == "fillets")
            .map(|r| r.faces.len())
            .sum();
        assert_eq!(faces, expected, "{name}");
    }
}
