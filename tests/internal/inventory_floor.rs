//! Recognition inventories at 5fb1706: motion stability alone cannot detect a lost feature.
use super::common;
use quiddity::{correspondence, features, read_step_file};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// #159 preserves the historical per-torus-patch inventory even when several
/// interrupted patches now share one callout. Cylinder rounds (including their
/// spherical caps) still count once. No baseline or loss allowance is lowered.
pub(super) fn fillet_patch_count(part: &quiddity::Part, defining: &[Vec<usize>]) -> usize {
    defining
        .iter()
        .map(|faces| {
            faces
                .iter()
                .filter(|&&f| {
                    matches!(
                        part.faces[f].surface,
                        quiddity::kernel::geom::Surface::Torus { .. }
                    )
                })
                .count()
                .max(1)
        })
        .sum()
}

fn counts(part: &quiddity::Part, f: &features::Features) -> BTreeMap<String, usize> {
    let mut counts: BTreeMap<String, usize> = serde_json::to_value(f)
        .unwrap()
        .as_object()
        .unwrap()
        .iter()
        .filter_map(|(k, v)| v.as_array().map(|v| (k.clone(), v.len())))
        .collect();
    counts.insert(
        "section_recess_patterns".into(),
        f.section_recess_patterns.len(),
    );
    if let Some(defining) = f.defining.get("fillets") {
        counts.insert("fillets".into(), fillet_patch_count(part, defining));
    }
    counts
}

fn census() -> BTreeMap<String, Value> {
    let root = common::fixtures();
    let mut paths = BTreeMap::new();
    for entry in std::fs::read_dir(&root).unwrap() {
        let p = entry.unwrap().path();
        let file = p.file_name().unwrap().to_str().unwrap();
        if file.starts_with("golden_") && file.ends_with(".step") {
            paths.insert(format!("golden/{file}"), p);
        }
    }
    if let Some(dir) = common::corpus_dir() {
        for entry in common::load("corpus.json")["files"].as_array().unwrap() {
            let file = entry["file"].as_str().unwrap();
            paths.insert(format!("corpus/{file}"), dir.join(file));
        }
    }
    for file in [
        "nist_ctc_02_asme1_ap242-e2.stp.gz",
        "nist_ftc_07_asme1_ap242-e2.stp.gz",
    ] {
        paths.insert(format!("fixture/{file}"), root.join(file));
    }
    let calls: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("captured/calls.json")).unwrap())
            .unwrap();
    for call in calls["calls"].as_array().unwrap() {
        if let Some(file) = call["file"].as_str() {
            paths.insert(format!("captured/{file}"), root.join("captured").join(file));
        }
    }
    let paths: Vec<_> = paths.into_iter().collect();
    common::parallel::map(&paths, |(key, path)| {
        let part = read_step_file(path).unwrap();
        let inventory = features::inventory(&part).unwrap();
        let mut raw = inventory.physical.clone();
        // Derived families must be counted over the raw members as well as accepted members.
        raw.hole_patterns = features::hole_patterns::recognise_hole_patterns(&raw.holes);
        raw.gusset_rib_patterns =
            features::gussets::recognise_gusset_rib_patterns(&raw.gusset_ribs);
        raw.slot_patterns = features::recess_patterns::recognise_slot_patterns(&raw.slots);
        raw.pocket_patterns = features::recess_patterns::recognise_pocket_patterns(&raw.pockets);
        raw.oriented_slot_patterns =
            features::oriented_slots::recognise_oriented_slot_patterns(&raw.oriented_slots);
        let mut accepted = inventory.accepted();
        // Include the public aggregate's unified recess projection, not just its sources.
        // The public projection API performs its own inventory; this is test-only work.
        if let Ok(projection) = features::section_recess_family::section_recess_projection(&part) {
            accepted.section_recesses = projection.section_recesses;
            accepted.section_recess_patterns = projection.patterns;
        }
        let mut result =
            json!({"physical": counts(&part, &raw), "accepted": counts(&part, &accepted)});
        if key.starts_with("corpus/") || key.starts_with("fixture/") {
            let found = correspondence::recognise(&part);
            let working_counts = counts(&part, &found.features);
            let published: BTreeMap<_, _> = found.features.families().into_iter().collect();
            for (old, plain) in [
                ("oriented_slots", "slots"),
                ("oriented_chamfers", "chamfers"),
                ("oriented_slot_patterns", "slot_patterns"),
            ] {
                assert!(!published.contains_key(old), "{key}: old published family");
                assert_eq!(
                    published[plain].len(),
                    working_counts[plain] + working_counts[old],
                    "{key}: merged {plain} inventory"
                );
            }
            result["framed"] = json!(working_counts);
        }
        (key.clone(), result)
    })
    .into_iter()
    .collect()
}

#[test]
fn inventories_do_not_silently_shrink() {
    let actual = census();
    // An explicit output path permits the same census to run at the reference revision.
    if let Some(path) = std::env::var_os("QUIDDITY_INVENTORY_OUTPUT") {
        std::fs::write(path, serde_json::to_string_pretty(&actual).unwrap()).unwrap();
    }
    let path = common::fixtures().join("inventory_floor.json");
    if !path.exists() && std::env::var_os("QUIDDITY_INVENTORY_OUTPUT").is_some() {
        return;
    }
    // For #123, additive turned-body floors retain the historical feature baselines.
    let floor: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut failures = Vec::new();
    for (file, expected) in floor["files"].as_object().unwrap() {
        let Some(got) = actual.get(file) else {
            // The corpus is optional in developer runs; mandatory CI exports it.
            assert!(
                file.starts_with("corpus/") && common::corpus_dir().is_none(),
                "missing {file}"
            );
            continue;
        };
        for (mode, families) in expected.as_object().unwrap() {
            for (family, baseline) in families.as_object().unwrap() {
                let baseline = baseline.as_u64().unwrap();
                let now = got[mode][family].as_u64().unwrap_or(0);
                if now >= baseline {
                    continue;
                }
                let allowance =
                    floor["losses"].as_array().unwrap().iter().find(|e| {
                        e["file"] == *file && e["mode"] == *mode && e["family"] == *family
                    });
                let allowed = allowance.is_some_and(|e| {
                    !e["reason"].as_str().unwrap_or("").is_empty()
                        && !e["evidence"].as_str().unwrap_or("").is_empty()
                        && baseline - now <= e["max_lost"].as_u64().unwrap()
                });
                if !allowed {
                    failures.push(format!("{file} {mode}/{family}: {baseline} -> {now}; an evidence-backed loss entry is required"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn circular_step_363_replaces_only_its_subordinate_profile() {
    use quiddity::features::{Context, circular_blind_steps};
    use quiddity::kernel::{
        brep::Arc,
        geom::{self, Surface},
    };

    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let part = read_step_file(&dir.join("mfcadpp_holdout/363.step")).unwrap();
    assert!(part.solid_is_geometrically_valid(0));
    let Surface::Cylinder { frame, radius } = part.faces[20].surface else {
        panic!("face 20 must remain the native corner cylinder");
    };
    assert!((radius - 4.550383998164).abs() < 1e-12);
    assert!(frame.z[0].abs() < 1e-12 && frame.z[1].abs() < 1e-12);
    assert!((frame.z[2].abs() - 1.).abs() < 1e-12);
    assert!((frame.origin[0] - 33.316330673049).abs() < 1e-12);
    assert!(frame.origin[1].abs() < 1e-12);
    let bounds = part.face_bounds(20);
    assert!((bounds.min[2] - 17.138600034740).abs() < 1e-12);
    assert!((bounds.max[2] - 20.497241548961).abs() < 1e-12);
    let (u0, u1, _, _) = part.uv_bounds(20).unwrap();
    assert!((u1 - u0 - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    for (support, outward) in [(5, [0., -1., 0.]), (15, [1., 0., 0.])] {
        let Surface::Plane { frame: plane } = part.faces[support].surface else {
            panic!("corner support must remain planar");
        };
        let normal = part.face_normal(support, 0., 0.).unwrap();
        assert!(geom::dist(normal, outward) < 1e-12);
        assert!(geom::dot(geom::sub(frame.origin, plane.origin), normal).abs() < 1e-12);
        assert_eq!(part.arc(20, support), Some(Arc::Convex));
    }
    assert_eq!(part.arc(20, 18), Some(Arc::Convex));
    let Surface::Plane { frame: extra } = part.faces[18].surface else {
        panic!("secondary cut must remain planar");
    };
    assert!((extra.origin[1] - radius).abs() < 1e-12);
    let extra_normal = part.face_normal(18, 0., 0.).unwrap();
    assert!(geom::dist(extra_normal, [0., 1., 0.]) < 1e-12);
    assert!(
        (geom::dot(geom::sub(frame.origin, extra.origin), extra_normal).abs() - radius).abs()
            < 1e-12
    );
    assert_eq!(part.arc(20, 24), Some(Arc::Concave));
    let ctx = Context::new(&part);
    let cut = circular_blind_steps::discover_verified(&ctx)
        .unwrap()
        .into_iter()
        .find(|step| step.defining == [20, 24])
        .unwrap();
    assert_eq!(cut.record.axis, 'z');
    assert_eq!(cut.record.radius, 4.55039);
    assert_eq!(cut.record.length, 3.35864);
    assert_eq!(
        ctx.swept_face_volume(0, 24, [0.; 3], [0., 0., bounds.max[2] - bounds.min[2]]),
        Some(0.)
    );

    let recognised = correspondence::recognise(&part);
    let steps: std::collections::BTreeSet<_> = recognised.features.defining["circular_blind_steps"]
        .iter()
        .cloned()
        .collect();
    assert_eq!(
        steps,
        [vec![20, 24], vec![25, 26], vec![10, 27]]
            .into_iter()
            .collect()
    );
    assert_eq!(recognised.features.circular_blind_steps.len(), 3);
    assert_eq!(recognised.features.cylindrical_profiles.len(), 1);
    assert_eq!(recognised.features.cylindrical_profiles[0].faces, [14]);
    assert_eq!(
        recognised.features.defining["cylindrical_profiles"],
        [vec![14]]
    );
}

/// #228: the four walls and all eight floor rounds form one closed cavity.
/// The literal face groups pin the old inventory's unaffected ownership, rather
/// than letting a family count conceal loss of an unrelated slot or fillet.
#[test]
fn ftc08_floor_fillet_pocket_replaces_only_its_slot_and_floor_rounds() {
    use quiddity::kernel::{brep::Arc, geom::Surface};
    use std::collections::BTreeSet;

    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let part = read_step_file(&dir.join("nist/nist_ftc_08_asme1_rc.stp")).unwrap();
    assert!(part.solid_is_geometrically_valid(0));
    let near = |a: f64, b: f64| assert!((a - b).abs() < 1e-8, "{a} != {b}");
    let floor_faces = [61, 62, 63, 64, 82, 84, 85, 135];
    assert_eq!(
        part.neighbours(83).into_iter().collect::<BTreeSet<_>>(),
        BTreeSet::from(floor_faces)
    );
    let floor = part.face_bounds(83);
    near(floor.min[2], 46.99);
    near(floor.max[2], 46.99);
    near(floor.max[0] - floor.min[0], 11.176);
    near(floor.max[1] - floor.min[1], 157.226);
    assert!(part.face_normal(83, 0., 0.).unwrap()[2] > 1. - 1e-9);
    for f in floor_faces {
        assert_eq!(part.arc(83, f), Some(Arc::Smooth));
        let bounds = part.face_bounds(f);
        near(bounds.min[2], 46.99);
        near(bounds.max[2], 47.752);
        match part.faces[f].surface {
            Surface::Cylinder { frame, radius } => {
                assert!([82, 84, 85, 135].contains(&f));
                near(radius, 0.762);
                assert!(frame.z[2].abs() < 1e-9);
                assert_eq!(part.frame_points_outward(f), Some(false));
            }
            Surface::Torus { major, minor, .. } => {
                assert!([61, 62, 63, 64].contains(&f));
                near(major, 2.413);
                near(minor, 0.762);
                // A linear extrusion's lateral surfaces have zero Gaussian
                // curvature. These genuine toroidal patches do not. Nor are
                // tori supported planar/cylindrical/plane-envelope run ends.
                let (_, _, lo, hi) = part.uv_bounds(f).unwrap();
                let cos_v = ((lo + hi) / 2.).cos();
                let gaussian = cos_v / (minor * (major + minor * cos_v));
                assert!(gaussian.abs() > 1e-4);
            }
            _ => panic!("floor transition {f} is not an analytic round"),
        }
    }
    for f in [17, 18, 19, 20] {
        let Surface::Cylinder { frame, radius } = part.faces[f].surface else {
            panic!("vertical wall corner {f}");
        };
        near(radius, 3.175);
        assert!(frame.z[2].abs() > 1. - 1e-9);
    }
    let mut mouth = part.face_bounds(213);
    for f in [214, 215, 216] {
        mouth.merge(&part.face_bounds(f));
    }
    near(mouth.max[0] - mouth.min[0], 12.7);
    near(mouth.max[1] - mouth.min[1], 158.75);
    near(mouth.max[2], 48.26);
    near(mouth.min[2], 47.752);

    let inventory = features::inventory(&part).unwrap();
    let accepted = inventory.clone().accepted();
    let published = correspondence::recognise(&part);
    let claims = |groups: &[Vec<usize>]| -> BTreeSet<Vec<usize>> {
        groups
            .iter()
            .map(|g| {
                let mut g = g.clone();
                g.sort_unstable();
                g
            })
            .collect()
    };
    let expected_slots = BTreeSet::from([vec![117, 121], vec![152, 154], vec![220, 224]]);
    // The 91 surviving groups from the independent face audit: 85 singleton
    // rounds and six paired patches, retaining their original grouping.
    let singleton_fillets = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
        27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 47, 49, 51, 53, 55,
        56, 58, 60, 65, 66, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 86, 87, 88, 89,
        95, 96, 98, 99, 100, 107, 143, 144, 145, 146, 147, 148, 149, 150, 269,
    ];
    let mut expected_fillets: BTreeSet<_> =
        singleton_fillets.into_iter().map(|f| vec![f]).collect();
    expected_fillets.extend([
        vec![101, 108],
        vec![102, 109],
        vec![103, 110],
        vec![104, 111],
        vec![105, 112],
        vec![106, 113],
    ]);
    assert_eq!(expected_fillets.len(), 91);
    let replacement = vec![61, 62, 63, 64, 82, 84, 85, 135, 213, 214, 215, 216];
    for (mode, f, completed) in [
        ("physical", &inventory.physical, false),
        ("accepted", &accepted, false),
        ("framed", &*published.features, true),
    ] {
        assert_eq!(f.pockets.len(), 9, "{mode}");
        let p = &f.pockets[8];
        assert_eq!((p.width, p.length, p.depth), (12.7, 158.75, 1.27));
        assert_eq!(p.corner_radius, Some(3.18));
        assert_eq!(p.floor_radius, Some(0.762));
        assert_eq!(
            p.floor_fillets
                .iter()
                .map(|f| (f.face, f.wall_face))
                .collect::<Vec<_>>(),
            [
                (61, 17),
                (62, 18),
                (63, 19),
                (64, 20),
                (82, 216),
                (84, 215),
                (85, 214),
                (135, 213)
            ]
        );
        assert!(p.floor_fillets.iter().all(|f| f.radius == 0.762));
        let mut pocket_faces = replacement.clone();
        if completed {
            pocket_faces.push(83);
            pocket_faces.sort_unstable();
        }
        assert_eq!(f.defining["pockets"][8], pocket_faces, "{mode}");
        let mut slots = expected_slots.clone();
        if mode == "physical" {
            slots.insert(vec![214, 216]);
        }
        assert_eq!(f.slots.len(), slots.len(), "{mode}");
        assert_eq!(claims(&f.defining["slots"]), slots, "{mode}");
        let mut fillets = expected_fillets.clone();
        if mode == "physical" {
            fillets.extend([82, 84, 85, 135].map(|f| vec![f]));
        }
        assert_eq!(f.fillets.len(), fillets.len(), "{mode}");
        assert_eq!(claims(&f.defining["fillets"]), fillets, "{mode}");
    }

    // Main's six section records cover other cavities. None projected the old
    // slot, and every one remains; the seventh document row is a new refusal.
    let f = &published.features;
    let section_faces = [
        vec![156],
        vec![222],
        vec![236, 238, 239, 240, 241, 242, 243, 244, 245, 246, 247, 248],
        vec![
            249, 250, 251, 253, 254, 255, 256, 257, 258, 259, 260, 261, 262, 263, 264, 265, 266,
            267, 268, 269,
        ],
        vec![256, 266],
        vec![254, 268],
    ];
    assert_eq!(f.section_recesses.len(), 6);
    for (r, expected) in f.section_recesses.iter().zip(&section_faces) {
        assert_eq!(r.evidence().defining_faces(), expected);
        assert!(
            r.evidence()
                .constituent_faces()
                .iter()
                .all(|f| !replacement.contains(f) && *f != 83 && ![17, 18, 19, 20].contains(f))
        );
    }
    let [refusal] = f.section_recess_refusals.as_slice() else {
        panic!("one new projection refusal")
    };
    assert_eq!(refusal.evidence().defining_faces(), replacement);
    assert_eq!(
        refusal.evidence().constituent_faces(),
        [
            17, 18, 19, 20, 61, 62, 63, 64, 82, 83, 84, 85, 135, 213, 214, 215, 216
        ]
    );
}
