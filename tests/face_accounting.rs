//! Complete membership and a reproducible, uncommitted census of the goldens and corpus.
use std::collections::BTreeSet;
use std::path::PathBuf;

use quiddity::recognition::{FaceAccounting, RecognitionDocument};

fn check(doc: &RecognitionDocument) -> [usize; 5] {
    let groups = doc.face_accounting.as_ref().unwrap();
    assert_eq!(groups.len(), doc.face_count);
    // A nested cone must be claimed while preserving the parent's core face set.
    // Check the published parameter references independently of claimed_faces().
    for record in doc.records.iter().filter(|r| r.family == "holes") {
        for treatment in record
            .parameters
            .get("mouth_chamfers")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            for face in treatment["faces"].as_array().unwrap() {
                let face = usize::try_from(face.as_u64().unwrap()).unwrap();
                assert!(!record.faces.contains(&face));
                let FaceAccounting::Feature { record_ids } = &groups[face] else {
                    panic!("nested treatment is not accounted as a feature")
                };
                assert!(record_ids.contains(&record.id));
            }
        }
    }
    let mut counts = [0; 5]; // feature, body, refused, unexplained, unclaimed before
    for (face, group) in groups.iter().enumerate() {
        let records: Vec<_> = doc
            .records
            .iter()
            .filter(|r| r.claimed_faces().any(|claimed| claimed == face))
            .collect();
        let feature_ids: Vec<_> = records
            .iter()
            .filter(|r| !r.is_refusal())
            .map(|r| r.id.clone())
            .collect();
        let refused: Vec<_> = records.iter().filter(|r| r.is_refusal()).collect();
        if feature_ids.is_empty() {
            counts[4] += 1;
        }
        match group {
            FaceAccounting::Feature { record_ids } => {
                assert!(!record_ids.is_empty());
                assert_eq!(*record_ids, feature_ids);
                counts[0] += 1;
            }
            FaceAccounting::Refused { refusals } => {
                assert!(feature_ids.is_empty());
                assert_eq!(refusals.len(), refused.len());
                assert!(!refusals.is_empty());
                for (reference, record) in refusals.iter().zip(refused) {
                    assert_eq!(reference.record_id, record.id);
                    assert_eq!(
                        reference.reason,
                        record.parameters["reason"].as_str().unwrap()
                    );
                }
                counts[2] += 1;
            }
            FaceAccounting::Body { .. } => {
                assert!(records.is_empty());
                counts[1] += 1;
            }
            FaceAccounting::Unexplained => {
                assert!(records.is_empty());
                counts[3] += 1;
            }
        }
    }
    assert_eq!(counts[..4].iter().sum::<usize>(), doc.face_count);
    counts
}

#[test]
fn complete_partition_on_all_goldens_and_recorded_corpus() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut paths: Vec<_> = std::fs::read_dir(&fixtures)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("golden_")
                && p.extension().is_some_and(|e| e == "step")
        })
        .collect();
    paths.sort();
    assert_eq!(paths.len(), 34);
    if let Some(root) = std::env::var_os("QUIDDITY_CORPUS").filter(|r| r != "off") {
        let corpus: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/corpus.json")).unwrap();
        paths.extend(
            corpus["files"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| PathBuf::from(&root).join(e["file"].as_str().unwrap())),
        );
    } else {
        assert!(
            std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
            "QUIDDITY_CORPUS required"
        );
    }
    let mut total = [0; 5];
    for path in paths {
        let (part, recognition, _) = quiddity::serve::recognise_step(
            path.to_str().unwrap(),
            &quiddity::kernel::step::IDENTITY,
        )
        .unwrap();
        let doc = quiddity::recognition::document(&recognition, part.faces.len());
        let counts = check(&doc);
        for (sum, count) in total.iter_mut().zip(counts) {
            *sum += count;
        }
        eprintln!("ACCOUNTING {} {counts:?}", path.display());
        let body: BTreeSet<_> = doc
            .face_accounting
            .as_ref()
            .unwrap()
            .iter()
            .enumerate()
            .filter_map(|(f, g)| matches!(g, FaceAccounting::Body { .. }).then_some(f))
            .collect();
        eprintln!("BODY {} {body:?}", path.display());
        if let quiddity::frames::RecordFrame::Inferred(frame) = &doc.frame {
            let working = part.transformed(&frame.normalisation()).unwrap();
            for f in body {
                let face = &working.faces[f];
                let bounds = working.solid_bounds(face.solid.unwrap());
                let vertices: Vec<_> = face
                    .loops
                    .iter()
                    .filter(|lp| lp.edges.len() == 4)
                    .map(|lp| {
                        lp.edges
                            .iter()
                            .map(|&(e, forward)| {
                                let edge = &working.edges[e];
                                if forward { edge.start } else { edge.end }
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect();
                eprintln!(
                    "GEOMETRY {} face={f} normal={:?} solid_box={bounds:?} loops={} rectangle_vertices={vertices:?}",
                    path.display(),
                    working.face_normal(f, 0.0, 0.0),
                    face.loops.len()
                );
            }
        }
        let back: RecognitionDocument =
            serde_json::from_value(serde_json::to_value(&doc).unwrap()).unwrap();
        assert_eq!(back, doc);
    }
    eprintln!("ACCOUNTING TOTAL {total:?}");
}

#[test]
fn original_q244_counterbore_mouth_claim_preserves_the_saved_hole_anchor() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/hole_mouth/cbore_chamfer.step");
    let part = quiddity::read_step_file(&path).unwrap();
    let recognition = quiddity::correspondence::recognise(&part);
    let doc = quiddity::recognition::document(&recognition, part.faces.len());
    check(&doc);
    let hole = doc.records.iter().find(|r| r.id == "holes/0").unwrap();
    assert_eq!(hole.faces, [3, 10]);
    assert_eq!(
        hole.parameters["mouth_chamfers"][0]["faces"],
        serde_json::json!([1])
    );
    assert_eq!(hole.parameters["mouth_chamfers"][0]["size"], 0.25);
    assert_eq!(hole.parameters["mouth_chamfers"][0]["angle"], 45.);
    assert_eq!(hole.parameters["cbore"]["depth"], 2.);
    assert_eq!(
        doc.face_accounting.as_ref().unwrap()[1],
        FaceAccounting::Feature {
            record_ids: vec!["holes/0".into()]
        }
    );
}

#[test]
fn legacy_missing_accounting_is_unknown() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/golden_simple_through_hole.step");
    let (output, _) = quiddity::serve::recognise_file(path.to_str().unwrap()).unwrap();
    for version in [3, 4] {
        let mut old = serde_json::to_value(&output.document).unwrap();
        old["schema_version"] = format!("quiddity-rust/recognition/{version}").into();
        old.as_object_mut().unwrap().remove("face_accounting");
        let old: RecognitionDocument = serde_json::from_value(old).unwrap();
        assert!(old.face_accounting.is_none());
        assert!(
            serde_json::to_value(old)
                .unwrap()
                .get("face_accounting")
                .is_none()
        );
    }
}

#[test]
fn ctc01_e1_accounting_records_and_main_correspondence() {
    use quiddity::correspondence::{self, Fingerprints};
    use std::io::Read;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ap242/nist/nist_ctc_01_asme1_ap242-e1.stp.gz");
    let (output, _) = quiddity::serve::recognise_file(path.to_str().unwrap()).unwrap();
    let doc = &output.document;
    assert_eq!(doc.schema_version, "quiddity-rust/recognition/6");
    use quiddity::recognition::BodyReason;
    assert_eq!(check(doc), [97, 20, 0, 0, 20]);
    // Main had 90 feature faces; the V-notch flanks (11, 13, 15, 17), the through-notch
    // bottoms (33, 39) and the hexagon boss top (104) are now claimed too.
    let feature_faces = [
        0, 1, 2, 3, 5, 7, 8, 9, 11, 12, 13, 15, 16, 17, 19, 20, 21, 23, 25, 27, 28, 29, 31, 32, 33,
        34, 35, 37, 38, 39, 40, 41, 43, 44, 45, 50, 51, 52, 53, 56, 57, 58, 59, 63, 64, 65, 66, 67,
        68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90,
        91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110,
        111, 112, 113, 114, 115, 116,
    ];
    let mut expected = vec![FaceAccounting::Unexplained; 117];
    for (reason, faces) in [
        (BodyReason::StockBoxNotched, &[4, 24][..]),
        (
            BodyReason::StockBoxSplit,
            &[6, 22, 26, 46, 47, 49, 54, 55][..],
        ),
        (BodyReason::PlateEdge, &[10, 14, 18, 30, 36, 42][..]),
        (BodyReason::StockWall, &[48, 60, 61, 62][..]),
    ] {
        for &face in faces {
            expected[face] = FaceAccounting::Body { reason };
        }
    }
    for (face, group) in doc.face_accounting.as_ref().unwrap().iter().enumerate() {
        if feature_faces.contains(&face) {
            assert!(
                matches!(group, FaceAccounting::Feature { .. }),
                "face {face}"
            );
        } else {
            assert_eq!(*group, expected[face], "face {face}");
        }
    }
    let unexplained: Vec<_> = doc
        .face_accounting
        .as_ref()
        .unwrap()
        .iter()
        .enumerate()
        .filter_map(|(f, g)| matches!(g, FaceAccounting::Unexplained).then_some(f))
        .collect();
    assert!(unexplained.is_empty(), "{unexplained:?}");
    let part = quiddity::read_step_file(&path).unwrap();
    let near = |a: f64, b: f64| assert!((a - b).abs() < 1e-6, "{a} != {b}");
    // Verify issue face hypotheses on file-space geometry, not numbering alone.
    for f in [10, 14, 18, 30, 36, 42] {
        let b = part.face_bounds(f);
        near(b.min[2], -50.);
        near(b.max[2], 0.);
        near(b.min[1].abs(), 125.);
        near(b.min[1], b.max[1]);
    }
    for f in [8, 20, 28, 44] {
        let b = part.face_bounds(f);
        near(b.min[0].abs(), 250.);
        near(b.min[0], b.max[0]);
        near(b.min[2], -100.);
        near(b.max[2], 0.);
    }
    for f in [48, 60, 61, 62] {
        let b = part.face_bounds(f);
        near(b.min[1].abs(), 125.);
        near(b.min[1], b.max[1]);
        near(b.min[2], -100.);
        near(b.max[2], -60.);
    }
    for f in [31, 33, 35, 37, 39, 41] {
        let b = part.face_bounds(f);
        near(b.min[2], -50.);
        near(b.max[2], 0.);
        assert!(matches!(
            part.faces[f].surface,
            quiddity::kernel::geom::Surface::Plane { .. }
        ));
    }
    // Four additional V-notch flanks: oblique planes, full slab, turning smoothly
    // into R10 concave rounds 12/16. They are cut walls, not exposed plate edges.
    for (f, blend) in [(11, 12), (13, 12), (15, 16), (17, 16)] {
        let b = part.face_bounds(f);
        near(b.min[2], -50.);
        near(b.max[2], 0.);
        let n = part.face_normal(f, 0., 0.).unwrap();
        near(n[0].abs(), 3_f64.sqrt() / 2.);
        near(n[1], -0.5);
        assert_eq!(
            part.arc(f, blend),
            Some(quiddity::kernel::brep::Arc::Smooth)
        );
        assert_eq!(part.frame_points_outward(blend), Some(false));
    }
    for (f, x) in [(56, 30.), (57, 30.), (58, -30.), (59, -30.)] {
        let apex = part.faces[f].surface.cone_apex().unwrap();
        near(apex[0], x);
        near(apex[1], -73.9913938097245);
        near(apex[2], -25.);
    }
    for (f, x) in [(82, 235.), (86, 335.)] {
        let b = part.face_bounds(f);
        near(b.min[0], x);
        near(b.max[0], x);
        near(b.max[1] - b.min[1], 30.);
        near(b.min[2], -50.);
        near(b.max[2], 0.);
    }
    near(part.face_bounds(104).min[2], 50.);
    // Each through-notch owns its bottom and both side walls; the existing step-level
    // evidence on four side walls stays beside it. For #127, the R10 corner rounds
    // now have the full concave-fillet proof, including the V-notch rounds 12/16.
    for (f, ids) in [
        (31, &["rectangular_through_notches/0", "step_levels/3"][..]),
        (33, &["rectangular_through_notches/0"][..]),
        (35, &["rectangular_through_notches/0", "step_levels/4"][..]),
        (37, &["rectangular_through_notches/1", "step_levels/7"][..]),
        (39, &["rectangular_through_notches/1"][..]),
        (41, &["rectangular_through_notches/1", "step_levels/8"][..]),
        (11, &["v_channels/0"][..]),
        (13, &["v_channels/0"][..]),
        (15, &["v_channels/1"][..]),
        (17, &["v_channels/1"][..]),
    ] {
        assert_eq!(
            doc.face_accounting.as_ref().unwrap()[f],
            FaceAccounting::Feature {
                record_ids: ids.iter().map(|&id| id.into()).collect()
            },
            "face {f}"
        );
    }
    for (id, faces) in [
        ("holes/1", vec![50, 51, 56, 57]),
        ("holes/2", vec![52, 53, 58, 59]),
        ("slots/1", vec![82, 84, 86, 88]),
        (
            "polygonal_bosses/0",
            vec![104, 105, 106, 107, 108, 109, 110],
        ),
        ("v_channels/0", vec![11, 13]),
        ("v_channels/1", vec![15, 17]),
        ("rectangular_through_notches/0", vec![31, 33, 35]),
        ("rectangular_through_notches/1", vec![37, 39, 41]),
    ] {
        assert_eq!(
            doc.records.iter().find(|r| r.id == id).unwrap().faces,
            faces
        );
    }
    // Independent origin/main eb13004 CLI output, on these exact compressed input
    // bytes. Keep the complete old face graph so this exercises real alignment
    // and assignment across the changed claims, rather than self-correspondence.
    let mut old = String::new();
    flate2::read::GzDecoder::new(
        &include_bytes!("fixtures/references/ctc01-main-fingerprints.json.gz")[..],
    )
    .read_to_string(&mut old)
    .unwrap();
    let old: Fingerprints = serde_json::from_str(&old).unwrap();
    let new = &output.recognition.fingerprints;
    assert_eq!(new.fingerprint_version, "quiddity-rust/fingerprint/3");
    let by_id = |id: &str| new.features.iter().find(|f| f.id == id);
    // For #127, every old blend is an audited, fully proved concave-fillet move,
    // not an ordinal shift. Pin the exact old/new ids, face and radius against the
    // old-main fingerprints; the existing convex fillets keep ids 0..6.
    let promoted = [
        ("blends/0", "fillets/21", 67, 10.0),
        ("blends/1", "fillets/22", 66, 10.0),
        ("blends/2", "fillets/27", 68, 10.0),
        ("blends/3", "fillets/28", 65, 10.0),
        ("blends/4", "fillets/29", 69, 10.0),
        ("blends/5", "fillets/30", 64, 10.0),
        ("blends/6", "fillets/23", 70, 10.0),
        ("blends/7", "fillets/24", 63, 10.0),
        ("blends/8", "fillets/7", 19, 25.0),
        ("blends/9", "fillets/8", 9, 25.0),
        ("blends/10", "fillets/9", 29, 25.0),
        ("blends/11", "fillets/10", 43, 25.0),
        ("blends/12", "fillets/11", 16, 10.0),
        ("blends/13", "fillets/12", 12, 10.0),
        ("blends/14", "fillets/13", 85, 10.0),
        ("blends/15", "fillets/14", 83, 10.0),
        ("blends/16", "fillets/15", 87, 10.0),
        ("blends/17", "fillets/16", 81, 10.0),
        ("blends/18", "fillets/17", 32, 10.0),
        ("blends/19", "fillets/18", 34, 10.0),
        ("blends/20", "fillets/19", 38, 10.0),
        ("blends/21", "fillets/20", 40, 10.0),
        ("blends/22", "fillets/31", 102, 5.0),
        ("blends/23", "fillets/33", 101, 5.0),
        ("blends/24", "fillets/25", 103, 5.0),
        ("blends/25", "fillets/26", 100, 5.0),
        ("blends/26", "fillets/34", 98, 5.0),
        ("blends/27", "fillets/32", 99, 5.0),
    ];
    for (old_id, new_id, face, radius) in promoted {
        let a = old.features.iter().find(|f| f.id == old_id).unwrap();
        let b = by_id(new_id).unwrap();
        assert!(by_id(old_id).is_none(), "{old_id}");
        assert_eq!(a.family, "blends");
        assert_eq!(b.family, "fillets");
        assert_eq!(a.faces, [face], "{old_id}");
        assert_eq!(b.faces, a.faces, "{new_id}");
        assert_eq!(a.sizes["radius"], radius, "{old_id}");
        assert_eq!(b.sizes, a.sizes, "{new_id}");
        assert_eq!(a.traits["side"], "concave");
        assert_eq!(b.traits["side"], "concave");
        assert_eq!(b.traits["turned"], "false");
    }
    // The boss additionally gains its top; the two V channels and two notches are
    // new. Every other old record keeps its id and complete fingerprint.
    let changed: Vec<_> = old
        .features
        .iter()
        .filter(|a| by_id(&a.id).is_none_or(|b| a.faces != b.faces))
        .map(|a| a.id.as_str())
        .collect();
    assert_eq!(
        changed,
        [
            "blends/0",
            "blends/1",
            "blends/2",
            "blends/3",
            "blends/4",
            "blends/5",
            "blends/6",
            "blends/7",
            "blends/8",
            "blends/9",
            "blends/10",
            "blends/11",
            "blends/12",
            "blends/13",
            "blends/14",
            "blends/15",
            "blends/16",
            "blends/17",
            "blends/18",
            "blends/19",
            "blends/20",
            "blends/21",
            "blends/22",
            "blends/23",
            "blends/24",
            "blends/25",
            "blends/26",
            "blends/27",
            "polygonal_bosses/0"
        ]
    );
    let added: Vec<_> = new
        .features
        .iter()
        .filter(|b| !old.features.iter().any(|a| a.id == b.id))
        .map(|b| b.id.as_str())
        .collect();
    assert_eq!(
        added,
        [
            "fillets/7",
            "fillets/8",
            "fillets/9",
            "fillets/10",
            "fillets/11",
            "fillets/12",
            "fillets/13",
            "fillets/14",
            "fillets/15",
            "fillets/16",
            "fillets/17",
            "fillets/18",
            "fillets/19",
            "fillets/20",
            "fillets/21",
            "fillets/22",
            "fillets/23",
            "fillets/24",
            "fillets/25",
            "fillets/26",
            "fillets/27",
            "fillets/28",
            "fillets/29",
            "fillets/30",
            "fillets/31",
            "fillets/32",
            "fillets/33",
            "fillets/34",
            "rectangular_through_notches/0",
            "rectangular_through_notches/1",
            "v_channels/0",
            "v_channels/1"
        ]
    );
    // Fingerprint floats differ in the last digits between builds and platforms, so
    // everything else is compared exactly and numbers to a relative 1e-9.
    let differing: Vec<_> = old
        .features
        .iter()
        .filter(|a| by_id(&a.id).is_none_or(|b| !same(*a, b)))
        .map(|a| a.id.as_str())
        .collect();
    // For #134 the supporting plate contracts its convex edge treatments. Shared
    // supporting-plane Green moments replace surface quadrature for the planar
    // skins: this also corrects ~1e-7 mm of numerical error in its centroid.
    // The centroid moves by under 1e-6 mm (exact digits differ by platform libm);
    // faces and sizes stay identical under the existing 1e-9 comparison.
    let a = old.features.iter().find(|f| f.id == "plates/0").unwrap();
    let b = by_id("plates/0").unwrap();
    let (pa, pb) = (a.position.unwrap(), b.position.unwrap());
    let moved = (0..3).map(|i| (pa[i] - pb[i]).powi(2)).sum::<f64>().sqrt();
    assert!(moved < 1e-6, "#134: plates/0 centroid moved {moved} mm");
    let mut restored = a.clone();
    restored.neighbourhood = b.neighbourhood.clone();
    restored.position = b.position;
    assert!(
        same(&restored, b),
        "#134: shared supporting-plane moments and contracted treatments"
    );
    let count = |f: &correspondence::FeatureFingerprint, text: &str| {
        f.neighbourhood
            .iter()
            .filter(|x| x.as_str() == text)
            .count()
    };
    assert_eq!(count(a, "plane>out:cylinder:convex"), 63);
    assert_eq!(count(b, "plane>out:cylinder:convex"), 56);
    assert_eq!(count(a, "plane>out:plane:convex"), 57);
    assert_eq!(count(b, "plane>out:plane:convex"), 60);
    let mut expected_differing = changed.clone();
    expected_differing.push("plates/0");
    expected_differing.sort_unstable();
    let mut differing = differing;
    differing.sort_unstable();
    assert_eq!(differing, expected_differing);
    let old_claims: BTreeSet<_> = old.features.iter().flat_map(|f| &f.faces).collect();
    assert_eq!(old_claims.len(), 90); // main: feature 90, body 20, unexplained 7
    assert_eq!(old.faces.len(), new.faces.len());
    for (a, b) in old.faces.iter().zip(&new.faces) {
        assert!(same(a, b), "{a:?}\n{b:?}");
    }
    assert_eq!(
        old.features.len() + added.len(),
        new.features.len() + promoted.len()
    );
    let correspondence = correspondence::correspond(&old, new).unwrap();
    for entry in &correspondence.features {
        // #127 moves between families: correspondence orphans the exact old
        // blends and reports their proved fillet replacements as new features.
        if promoted.iter().any(|(id, _, _, _)| *id == entry.old) {
            assert!(entry.new.is_none(), "{entry:?}");
            assert_eq!(entry.class, correspondence::Class::Orphaned, "{entry:?}");
        } else {
            assert_eq!(entry.new.as_deref(), Some(entry.old.as_str()), "{entry:?}");
        }
        assert!(entry.candidates.is_empty());
    }
    let mut unmatched: Vec<_> = correspondence
        .new_features
        .iter()
        .map(String::as_str)
        .collect();
    unmatched.sort_unstable();
    let mut expected_unmatched = added.clone();
    expected_unmatched.sort_unstable();
    assert_eq!(unmatched, expected_unmatched);
    let (sz, cz) = 17_f64.to_radians().sin_cos();
    let (sx, cx) = 23_f64.to_radians().sin_cos();
    let motion = [
        [cz, -sz, 0., 137.],
        [cx * sz, cx * cz, -sx, -83.],
        [sx * sz, sx * cz, cx, 49.],
    ];
    let moved = part.transformed(&motion).unwrap();
    let moved = quiddity::correspondence::recognise(&moved);
    let moved_doc = quiddity::recognition::document(&moved, moved.fingerprints.faces.len());
    assert_eq!(moved_doc.face_accounting, doc.face_accounting);
}

fn same(a: &impl serde::Serialize, b: &impl serde::Serialize) -> bool {
    fn close(a: &serde_json::Value, b: &serde_json::Value) -> bool {
        use serde_json::Value::{Array, Number, Object};
        match (a, b) {
            (Number(x), Number(y)) => {
                let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
                (x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0)
            }
            (Array(x), Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(x, y)| close(x, y)),
            (Object(x), Object(y)) => {
                x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| close(v, w)))
            }
            _ => a == b,
        }
    }
    close(
        &serde_json::to_value(a).unwrap(),
        &serde_json::to_value(b).unwrap(),
    )
}
