//! #341: accepted fillet faces, tangent components, and radius groups.

#[path = "common/public.rs"]
mod common;

use std::collections::{BTreeMap, BTreeSet};

use quiddity::correspondence::recognise;
use quiddity::kernel::brep::Arc;
use quiddity::kernel::geom::{self, Curve, Surface};
use quiddity::recognition::{RecognitionDocument, Record, document};
use quiddity::{Part, read_step_file};
use serde_json::Value;

fn family<'a>(doc: &'a RecognitionDocument, family: &str) -> Vec<&'a Record> {
    doc.records.iter().filter(|r| r.family == family).collect()
}

fn index_list(value: &Value) -> Vec<usize> {
    serde_json::from_value(value.clone()).unwrap()
}

fn fixture(name: &str) -> (Part, RecognitionDocument) {
    let part = read_step_file(&common::fixtures().join(name)).unwrap();
    let doc = document(&recognise(&part), part.faces.len());
    (part, doc)
}

fn blend_radius(surface: &Surface) -> Option<f64> {
    match *surface {
        Surface::Cylinder { radius, .. } | Surface::Sphere { radius, .. } => Some(radius),
        Surface::Torus { minor, .. } => Some(minor),
        _ => None,
    }
}

fn blend_points_outward(part: &Part, face: usize) -> Option<bool> {
    let Surface::Torus {
        frame,
        major,
        minor,
    } = part.faces[face].surface
    else {
        return part.frame_points_outward(face);
    };
    let (u0, u1, v0, v1) = part.uv_bounds(face)?;
    let outward = |fraction: f64| {
        let u = u0 + fraction * (u1 - u0);
        let v = v0 + fraction * (v1 - v0);
        let rho = major + minor * v.cos();
        // In parameter coordinates, subtract the closest point on the spine circle.
        let radial = rho - major.copysign(rho);
        let from_spine = frame.dir_to_world([radial * u.cos(), radial * u.sin(), minor * v.sin()]);
        let alignment = geom::dot(part.face_normal(face, u, v)?, from_spine);
        (alignment.is_finite() && alignment != 0.0).then_some(alignment > 0.0)
    };
    let side = outward(0.5)?;
    [0.25, 0.75]
        .into_iter()
        .all(|fraction| outward(fraction) == Some(side))
        .then_some(side)
}

fn check_partition(part: &Part, doc: &RecognitionDocument) {
    let (fillets, chains, groups) = (
        family(doc, "fillets"),
        family(doc, "fillet_chains"),
        family(doc, "fillet_groups"),
    );
    assert!(!fillets.is_empty(), "the part must contain legacy fillets");
    assert!(!chains.is_empty(), "the part must contain fillet chains");
    assert!(!groups.is_empty(), "the part must contain radius groups");
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
    let tolerance = part.file_uncertainty().unwrap_or(geom::COORD_FLOOR);
    let mut chain_of = BTreeMap::new();
    for (i, chain) in chains.iter().enumerate() {
        let faces = face_list(chain);
        for &face in &faces {
            assert!(chain_of.insert(face, i).is_none());
        }
        let expected_fillets: BTreeSet<_> =
            faces.iter().filter_map(|f| owner.get(f).copied()).collect();
        assert!(!expected_fillets.is_empty(), "{} has no seed", chain.id);
        let blend: Vec<_> = faces
            .iter()
            .copied()
            .filter(|f| !owner.contains_key(f))
            .collect();
        let reported = chain
            .parameters
            .get("blend_faces")
            .map_or_else(Vec::new, index_list);
        assert_eq!(reported, blend);
        for f in blend {
            assert!(matches!(
                part.faces[f].surface,
                Surface::Sphere { .. } | Surface::Torus { .. }
            ));
        }
        assert_eq!(
            index_list(&chain.parameters["fillets"]),
            expected_fillets.into_iter().collect::<Vec<_>>()
        );
        for edge in index_list(&chain.parameters["edges"]) {
            let incident = &part.edge_faces()[edge];
            assert_eq!(incident.len(), 2);
            assert_eq!(incident.iter().filter(|f| faces.contains(f)).count(), 1);
            let (blend, neighbour) = if faces.contains(&incident[0]) {
                (incident[0], incident[1])
            } else {
                (incident[1], incident[0])
            };
            let radius = blend_radius(&part.faces[blend].surface).unwrap();
            let outward = chain.parameters["side"] == "convex";
            let same_blend = blend_radius(&part.faces[neighbour].surface)
                .is_some_and(|other| (other - radius).abs() <= tolerance)
                && blend_points_outward(part, neighbour) == Some(outward);
            assert!(
                !same_blend,
                "{} spring edge {edge} borders equal-radius same-side blend face {neighbour}",
                chain.id
            );
        }
    }
    for chain in &chains {
        for edge in index_list(&chain.parameters["edges"]) {
            assert_eq!(
                part.edge_faces()[edge]
                    .iter()
                    .filter(|f| chain_of.contains_key(f))
                    .count(),
                1,
                "spring edge {edge} must border a wall"
            );
        }
    }
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
        let mut members_fillets = BTreeSet::new();
        for i in members {
            assert!(grouped_chains.insert(i));
            assert_eq!(group.parameters["side"], chains[i].parameters["side"]);
            union.extend_from_slice(&chains[i].faces);
            members_fillets.extend(index_list(&chains[i].parameters["fillets"]));
        }
        assert_eq!(group.parameters["fillet_count"], members_fillets.len());
        union.sort_unstable();
        assert_eq!(union, faces);
        for face in faces {
            assert!(group_faces.insert(face));
        }
    }
    assert_eq!(grouped_chains.len(), chains.len());
    assert_eq!(group_faces, chain_of.into_keys().collect());
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
    // All proved branches join, including paths through previously unowned corners.
    assert_eq!(chains.len(), 344, "cgb202 geometric chain count");
    assert_eq!(
        chains.iter().filter(|c| c.faces.len() == 1).count(),
        216,
        "cgb202 singleton count"
    );
    assert!(
        chains
            .iter()
            .all(|c| !index_list(&c.parameters["edges"]).is_empty()),
        "all cgb202 chains have spring edges"
    );
    let chain_of = |face| chains.iter().find(|c| c.faces.contains(&face));
    for (sphere, neighbours) in [(4, [3, 5, 1215]), (10, [9, 11, 1219])] {
        for neighbour in neighbours {
            assert_eq!(part.arc(sphere, neighbour), Some(Arc::Smooth));
            assert_eq!(
                chain_of(sphere).unwrap().id,
                chain_of(neighbour).unwrap().id
            );
        }
    }
    assert_eq!(chain_of(74).unwrap().id, chain_of(72).unwrap().id);
    assert!(index_list(&chain_of(74).unwrap().parameters["blend_faces"]).contains(&72));
    for face in [72, 76, 887] {
        assert_eq!(part.arc(74, face), Some(Arc::Smooth));
        assert_eq!(chain_of(74).unwrap().id, chain_of(face).unwrap().id);
    }

    assert_eq!(part.edge_faces()[2212], [848, 850]);
    assert_eq!(part.arc(848, 850), Some(Arc::Smooth));
    assert_eq!(part.frame_points_outward(850), Some(false));
    assert!(matches!(
        part.faces[850].surface,
        Surface::Sphere { radius: 3.0, .. }
    ));
    assert!(matches!(
        part.faces[848].surface,
        Surface::Cylinder { radius: 3.0, .. }
    ));
    assert_eq!(chain_of(848).unwrap().parameters["side"], "concave");
    assert_eq!(chain_of(848).unwrap().id, chain_of(850).unwrap().id);
    assert!(index_list(&chain_of(848).unwrap().parameters["blend_faces"]).contains(&850));
    assert!(
        !chain_of(848).unwrap().parameters["edges"]
            .as_array()
            .unwrap()
            .contains(&Value::from(2212)),
        "the tangent equal-radius continuation e2212 is not a spring edge"
    );
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
        assert_eq!(p["fillet_count"], old_records.len());
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
            ((1000, "concave"), (3, 10)),
            ((1000, "convex"), (6, 6)),
            ((2000, "convex"), (50, 68)),
            ((2500, "concave"), (7, 7)),
            ((3000, "concave"), (353, 807)),
            ((3000, "convex"), (28, 51)),
            ((4000, "concave"), (59, 62)),
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
    for (name, owned, total) in [
        ("cgb202", 772, 1052),
        ("cgb242", 312, 374),
        ("cgb243", 66, 77),
    ] {
        let path = corpus.join(format!("cadgenbench_inputs/{name}.step.gz"));
        let part = read_step_file(&path).unwrap();
        let doc = document(&recognise(&part), part.faces.len());
        let blend_count: usize = family(&doc, "fillet_chains")
            .iter()
            .map(|c| c.faces.len())
            .sum();
        assert_eq!(blend_count, total, "{name} connected blend face coverage");
        check_partition(&part, &doc);
        let faces: usize = doc
            .records
            .iter()
            .filter(|r| r.family == "fillets")
            .map(|r| r.faces.len())
            .sum();
        assert_eq!(faces, owned, "{name} legacy face coverage");
    }
}

#[test]
fn straight_creases_between_equal_rounds_split_on_normal_disagreement() {
    let part = read_step_file(&common::fixtures().join("fillet_chain_crease.step.gz")).unwrap();
    let doc = document(&recognise(&part), part.faces.len());
    let fillets: Vec<_> = doc
        .records
        .iter()
        .filter(|r| r.family == "fillets")
        .collect();
    assert_eq!(
        fillets
            .iter()
            .map(|f| f.faces.as_slice())
            .collect::<Vec<_>>(),
        [&[2], &[7]]
    );
    for f in fillets {
        assert_eq!(f.parameters["radius"], 2.0);
        assert_eq!(f.parameters["side"], "convex");
    }
    let shared = part.shared_edges(2, 7);
    assert_eq!(shared, [12, 14]);
    assert!(
        shared
            .iter()
            .all(|&e| matches!(part.edges[e].curve, Curve::Line { .. }))
    );
    assert_eq!(part.arc(2, 7), Some(Arc::Concave));
    let chains: Vec<_> = doc
        .records
        .iter()
        .filter(|r| r.family == "fillet_chains")
        .map(|c| c.faces.as_slice())
        .collect();
    assert_eq!(
        chains,
        [&[2], &[7]],
        "straight creases must split equal convex R2 fillets"
    );
    check_partition(&part, &doc);
}

#[test]
fn fully_rounded_box_has_one_chain_through_all_corners() {
    let (part, doc) = fixture("fillet_chain_box.step.gz");
    let chains = family(&doc, "fillet_chains");
    assert_eq!(
        chains.len(),
        1,
        "all R3 box rounds connect through the corners"
    );
    let chain = chains[0];
    assert_eq!(chain.parameters["radius"], 3.0);
    assert_eq!(chain.parameters["side"], "convex");
    assert_eq!(chain.faces.len(), 20);
    let corners = index_list(&chain.parameters["blend_faces"]);
    assert_eq!(corners.len(), 8);
    for face in corners {
        assert!(matches!(
            part.faces[face].surface,
            Surface::Sphere { radius: 3., .. }
        ));
        let incident_rounds = part
            .face_edges(face)
            .iter()
            .filter(|&&e| {
                part.edge_faces()[e]
                    .iter()
                    .any(|&f| f != face && chain.faces.contains(&f))
            })
            .count();
        assert_eq!(
            incident_rounds, 3,
            "each sphere joins three cylindrical rounds"
        );
    }
    let edges = index_list(&chain.parameters["edges"]);
    assert_eq!(edges.len(), 24);
    let mut walls = BTreeMap::new();
    for edge in edges {
        let wall = part.edge_faces()[edge]
            .iter()
            .copied()
            .find(|f| !chain.faces.contains(f))
            .unwrap();
        assert!(matches!(part.faces[wall].surface, Surface::Plane { .. }));
        *walls.entry(wall).or_insert(0) += 1;
    }
    assert_eq!(walls.len(), 6);
    assert!(walls.values().all(|&edges| edges == 4));
    let groups = family(&doc, "fillet_groups");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].parameters["fillet_count"], 12);
    check_partition(&part, &doc);
}

#[test]
fn opposite_box_rounds_are_two_chains_in_one_group() {
    let (part, doc) = fixture("fillet_chain_disjoint.step.gz");
    let chains = family(&doc, "fillet_chains");
    assert_eq!(
        chains.len(),
        2,
        "two disjoint R3 rounds form separate chains"
    );
    assert_eq!((chains[0].faces.len(), chains[1].faces.len()), (1, 1));
    assert!(
        part.shared_edges(chains[0].faces[0], chains[1].faces[0])
            .is_empty()
    );
    let groups = family(&doc, "fillet_groups");
    assert_eq!(groups.len(), 1);
    assert_eq!(
        groups[0].parameters["fillet_count"], 2,
        "the group counts both disjoint fillets"
    );
    check_partition(&part, &doc);
}

#[test]
fn tangent_different_radius_rounds_split_at_each_corner() {
    let (part, doc) = fixture("fillet_chain_mixed.step.gz");
    let chains = family(&doc, "fillet_chains");
    assert_eq!(
        chains.len(),
        5,
        "the R1 rim and four R3 rounds have separate chains"
    );
    let rim = chains[0];
    assert_eq!(rim.parameters["radius"], 1.0);
    assert_eq!(rim.faces.len(), 8);
    let groups = family(&doc, "fillet_groups");
    assert_eq!(groups.len(), 2);
    assert_eq!(
        groups[0].parameters["fillet_count"], 8,
        "R1 counts the whole rim"
    );
    assert_eq!(
        groups[1].parameters["fillet_count"], 4,
        "R3 counts the vertical rounds"
    );
    for chain in &chains[1..] {
        assert_eq!(chain.parameters["radius"], 3.0);
        assert_eq!(chain.parameters["side"], rim.parameters["side"]);
        assert_eq!(chain.faces.len(), 1);
        let round = chain.faces[0];
        let shared: Vec<_> = rim
            .faces
            .iter()
            .flat_map(|&f| part.shared_edges(f, round).into_iter().map(move |e| (f, e)))
            .collect();
        let [(corner, edge)] = shared.as_slice() else {
            panic!("each vertical round has one rim contact, got {shared:?}");
        };
        assert_eq!(part.arc(*corner, round), Some(Arc::Smooth));
        assert!(matches!(part.edges[*edge].curve, Curve::Circle { .. }));
        assert!(matches!(
            part.faces[*corner].surface,
            Surface::Torus { minor: 1., .. }
        ));
        assert!(!index_list(&rim.parameters["edges"]).contains(edge));
        assert!(!index_list(&chain.parameters["edges"]).contains(edge));
    }
    check_partition(&part, &doc);
}
