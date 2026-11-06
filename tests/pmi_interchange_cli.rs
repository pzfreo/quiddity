#![cfg(feature = "json-import")]
use quiddity::kernel::{
    json::{self, Anchors},
    p21::Document,
    pmi::*,
    step::read_part_definitions,
};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};
fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quiddity"))
        .args(args)
        .output()
        .unwrap()
}
fn scratch() -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("pmi_interchange_cli");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
fn model() -> PartPmi {
    PartPmi {
        features: vec![Feature::Items(vec![Anchor::Face(FaceIndex(0))])],
        dimensions: vec![Dimension {
            kind: DimensionKind::Size {
                feature: FeatureId(0),
                kind: SizeKind::Thickness,
                path: None,
                angle: None,
            },
            nominal: Some(Value::length(
                Decimal::parse("10.000").unwrap(),
                LengthUnit::Millimetre,
            )),
            tolerance: DimTolerance::Basic,
            qualifier: None,
            modifiers: vec![],
            principle: None,
        }],
        notes: vec![Note {
            kind: "general note".into(),
            text: "Deburr all edges".into(),
            on: None,
            callouts: vec![],
        }],
        ..PartPmi::default()
    }
}
#[test]
fn export_validate_import_and_presentation() {
    let cube = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ties/cube.step");
    let dir = scratch();
    let path = dir.join("pmi.json");
    let output = dir.join("output.step.gz");
    let exported = cli(&["pmi", "json", "export", cube]);
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stderr)
    );
    std::fs::write(&path, &exported.stdout).unwrap();
    let checked = cli(&["pmi", "json", "validate", path.to_str().unwrap()]);
    assert!(checked.status.success());
    let mut semantic = json::identify(model(), None).unwrap();
    semantic.ids.get_mut("dimensions").unwrap()[0] = "caller inspection 17".into();
    std::fs::write(&path, json::to_string(&semantic).unwrap()).unwrap();
    let imported = cli(&[
        "pmi",
        "json",
        "import",
        cube,
        path.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
    ]);
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    let exported = cli(&["pmi", "json", "export", output.to_str().unwrap()]);
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&exported.stdout).unwrap();
    assert_eq!(value["pmi"]["dimensions"][0]["id"], "caller inspection 17");
    let input = std::fs::read(cube).unwrap();
    let doc = Document::parse(input.clone()).unwrap();
    let parts = read_part_definitions(&input).unwrap();
    let decoded = json::from_value(&value, &Anchors::read(&doc, &parts[0])).unwrap();
    assert!(json::identity_differences(&model(), &decoded.pmi).is_empty());
    let nist = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/ap242/nist/nist_ctc_01_asme1_ap242-e1.stp.gz"
    );
    let drawn = cli(&["pmi", "json", "presentation", nist]);
    assert!(
        drawn.status.success(),
        "{}",
        String::from_utf8_lossy(&drawn.stderr)
    );
    std::fs::write(&path, &drawn.stdout).unwrap();
    assert!(
        cli(&["pmi", "json", "validate", path.to_str().unwrap()])
            .status
            .success()
    );
}
#[test]
fn malformed_and_unknown_faces_refuse_without_output() {
    let cube = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ties/cube.step");
    let dir = scratch();
    let path = dir.join("invalid.json");
    let out = dir.join("refused.step");
    let _ = std::fs::remove_file(&out);
    let mut value = json::to_value(&json::identify(model(), None).unwrap()).unwrap();
    value["pmi"]["features"][0]["value"]["items"][0]["face"] = json!({"name":"missing"});
    std::fs::write(&path, value.to_string()).unwrap();
    let result = cli(&[
        "pmi",
        "json",
        "import",
        cube,
        path.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("unknown face name"));
    assert!(!out.exists());
    std::fs::write(
        &path,
        r#"{"schema":"haecceity.pmi/1","schema":"haecceity.pmi/1"}"#,
    )
    .unwrap();
    let result = cli(&["pmi", "json", "validate", path.to_str().unwrap()]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("duplicate key"));
}

/// Every authored semantic fixture accepted by CLI validate must import with its geometry.
#[test]
fn every_accepted_semantic_fixture_imports() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/haecceity/tests/fixtures");
    let mut pending = vec![root];
    let mut accepted = 0;
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            if value["schema"] != "haecceity.pmi/1" {
                continue;
            }
            let checked = cli(&["pmi", "json", "validate", path.to_str().unwrap()]);
            if !checked.status.success() {
                continue;
            }
            accepted += 1;
            let parent = path.parent().unwrap();
            let geometry = if parent.ends_with("xcaf") {
                parent.join("pmi_plate.geom.step.gz")
            } else {
                parent.join("geometry.step.gz")
            };
            assert!(
                geometry.is_file(),
                "accepted fixture needs geometry: {}",
                path.display()
            );
            let output = scratch().join(format!("fixture-{accepted}.step"));
            let imported = cli(&[
                "pmi",
                "json",
                "import",
                geometry.to_str().unwrap(),
                path.to_str().unwrap(),
                "-o",
                output.to_str().unwrap(),
            ]);
            assert!(
                imported.status.success(),
                "{}: {}",
                path.display(),
                String::from_utf8_lossy(&imported.stderr)
            );
            let exported = cli(&["pmi", "json", "export", output.to_str().unwrap()]);
            assert!(
                exported.status.success(),
                "{}",
                String::from_utf8_lossy(&exported.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&exported.stdout).unwrap(),
                value
            );
        }
    }
    assert!(
        accepted >= 4,
        "the xcaf fixture and all three #142 producer regressions must be accepted"
    );
}

#[cfg(feature = "presentation")]
#[test]
fn plate_companion_import_uses_requirement_ids_and_places_actual_ink() {
    use quiddity::kernel::{geom::Bounds, presentation};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/haecceity/tests/fixtures/xcaf");
    let geometry = root.join("pmi_plate.geom.step.gz");
    let semantic_path = root.join("pmi_plate.source.pmi.json");
    let source_path = root.join("pmi_plate.source.presentation.json");
    let output = scratch().join("plate-source-positions.step");
    let imported = cli(&[
        "pmi",
        "json",
        "import",
        geometry.to_str().unwrap(),
        semantic_path.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--source-presentation",
        source_path.to_str().unwrap(),
    ]);
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    let drawn = cli(&["pmi", "json", "presentation", output.to_str().unwrap()]);
    assert!(
        drawn.status.success(),
        "{}",
        String::from_utf8_lossy(&drawn.stderr)
    );
    let drawn: json::presentation::Presentation = serde_json::from_slice(&drawn.stdout).unwrap();
    let source: json::presentation::Presentation =
        serde_json::from_slice(&std::fs::read(&source_path).unwrap()).unwrap();
    assert_eq!(source.items.len(), 11);
    for (id, graphics) in &source.items {
        assert_eq!(
            drawn.items[id][0].text_placements[0].origin, graphics[0].text_placements[0].origin,
            "{id}"
        );
    }
    let bytes = std::fs::read(&output).unwrap();
    let defs = read_part_definitions(&bytes).unwrap();
    let doc = Document::parse(bytes.clone()).unwrap();
    let semantic = json::from_value(
        &serde_json::from_slice(&std::fs::read(&semantic_path).unwrap()).unwrap(),
        &Anchors::read(&doc, &defs[0]),
    )
    .unwrap();
    for (i, dimension) in semantic.pmi.dimensions.iter().enumerate() {
        let id = &semantic.ids["dimensions"][i];
        let callout = presentation::dimension(dimension);
        let lines = &drawn.items[id][0].polylines;
        assert_eq!(lines.len(), callout.polylines().len(), "{id}: missing ink");
        let mut ink = Bounds::empty();
        for &p in lines[callout.frame_strokes..].iter().flatten() {
            ink.add(p);
        }
        let expected = source.items[id][0].text_placements[0].origin;
        for (actual, expected) in ink.centre().iter().zip(expected) {
            assert!(
                (actual - expected).abs() < 1e-9,
                "{id}: {:?} != {expected:?}",
                ink.centre()
            );
        }
    }
    // Bad source alignment refuses without replacing an existing output.
    let mut bad: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&source_path).unwrap()).unwrap();
    bad["items"]["thickness"][0]["text_placements"][0]["alignment"] = json!("justify");
    let bad_path = scratch().join("plate-bad-alignment.presentation.json");
    std::fs::write(&bad_path, bad.to_string()).unwrap();
    let refused = cli(&[
        "pmi",
        "json",
        "import",
        geometry.to_str().unwrap(),
        semantic_path.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--source-presentation",
        bad_path.to_str().unwrap(),
    ]);
    assert!(!refused.status.success());
    let error = String::from_utf8_lossy(&refused.stderr);
    assert!(
        error.contains("thickness") && error.contains("alignment"),
        "{error}"
    );
    assert_eq!(std::fs::read(&output).unwrap(), bytes);
}

#[cfg(not(feature = "presentation"))]
#[test]
fn json_import_source_requires_presentation_drawing() {
    let cube = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ties/cube.step");
    let input = scratch().join("source-disabled.pmi.json");
    let source = scratch().join("source-disabled.presentation.json");
    let output = scratch().join("source-disabled-output.step");
    let _ = std::fs::remove_file(&output);
    std::fs::write(
        &input,
        json::to_string(&json::identify(model(), None).unwrap()).unwrap(),
    )
    .unwrap();
    std::fs::write(
        &source,
        serde_json::to_string(&json::presentation::Presentation::default()).unwrap(),
    )
    .unwrap();
    let result = cli(&[
        "pmi",
        "json",
        "import",
        cube,
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--source-presentation",
        source.to_str().unwrap(),
    ]);
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("requires the `presentation` feature")
    );
    assert!(!output.exists());
}

/// `pmi json`'s usage errors are clap's (exit 2, nothing on stdout); a bad `--part` value is
/// still refused by quiddity with its own message (exit 1).
#[test]
fn json_usage_errors_exit_2_and_a_bad_part_exits_1() {
    let cube = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ties/cube.step");
    for args in [
        &["pmi", "json"][..],
        &["pmi", "json", "frob"],
        &["pmi", "json", "export"],
        &["pmi", "json", "export", cube, "--part"],
        &["pmi", "json", "export", cube, "--part", "0", "--part", "0"],
        &["pmi", "json", "export", cube, "-o", "out.step"],
        &["pmi", "json", "import", cube, "pmi.json"],
        &["pmi", "json", "validate"],
    ] {
        let out = cli(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        assert!(!out.stderr.is_empty(), "{args:?}");
    }
    for command in ["export", "presentation"] {
        let out = cli(&["pmi", "json", command, cube, "--part", "q"]);
        assert_eq!(out.status.code(), Some(1), "{command}");
        assert!(out.stdout.is_empty(), "{command}");
        assert_eq!(
            String::from_utf8_lossy(&out.stderr),
            "quiddity: --part must be an index\n",
            "{command}"
        );
    }
}
