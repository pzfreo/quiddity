//! The `quiddity` command line: usage, its output (the recognition document beside the records
//! and fingerprints), and honest failure on broken STEP files.
//!
//! The broken files under `tests/fixtures/broken/` are derived from `rejected_cylinder.step`
//! (a three-face cylinder): an empty DATA section, a write cut off mid-entity, a file cut at an
//! entity boundary and closed, one `ADVANCED_FACE` deleted, a closed edge's circle replaced by
//! a hyperbola (which the kernel does not model), and, read but reported, an open edge's line
//! replaced by a hyperbola.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn quiddity(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quiddity"))
        .args(args)
        .output()
        .expect("quiddity runs")
}

fn native_pmi(path: &Path) -> haecceity::pmi::PmiRead {
    let raw = std::fs::read(path).unwrap();
    let bytes = if path.extension().is_some_and(|e| e == "gz") {
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(
            &mut flate2::read::GzDecoder::new(raw.as_slice()),
            &mut bytes,
        )
        .unwrap();
        bytes
    } else {
        raw
    };
    let doc = haecceity::p21::Document::parse(bytes.clone()).unwrap();
    let parts = haecceity::step::read_part_definitions(&bytes).unwrap();
    haecceity::pmi::read(&doc, &parts).unwrap()
}

fn broken(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/broken")
        .join(name)
}

/// Runs `quiddity <file>` and returns its stderr, asserting it failed with nothing on stdout.
fn refused(name: &str) -> String {
    let out = quiddity(&[broken(name).to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(1), "{name}: {stderr}");
    assert!(out.stdout.is_empty(), "{name} printed a result");
    stderr
}

#[test]
fn help_prints_usage_and_succeeds() {
    for flag in ["-h", "--help"] {
        let out = quiddity(&[flag]);
        assert!(out.status.success(), "{flag}");
        let usage = String::from_utf8_lossy(&out.stdout);
        assert!(usage.starts_with("usage: quiddity"));
        for command in [
            "quiddity correspond",
            "quiddity parts <file.step>",
            "quiddity pmi read <file.step> [--part N]",
            "quiddity pmi check <file.step> <pmi.json>",
            "quiddity pmi write <file.step> <pmi.json> -o <out.step> [--mode add|replace|remove]",
            "[--presentation refuse|remove]",
        ] {
            assert!(usage.contains(command), "{flag}: {usage}");
        }
        assert_eq!(
            usage.contains("--source-presentation FILE"),
            cfg!(feature = "json-import")
        );
        assert!(out.stderr.is_empty());
    }
}

#[test]
fn the_usage_lists_serve() {
    let out = quiddity(&["--help"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("quiddity serve"));
}

#[test]
fn serve_on_an_empty_input_succeeds_silently() {
    let out = Command::new(env!("CARGO_BIN_EXE_quiddity"))
        .arg("serve")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("quiddity runs");
    assert!(out.status.success());
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
}

#[test]
fn bad_arguments_print_usage_and_fail() {
    let step = broken("../rejected_cylinder.step");
    let step = step.to_str().unwrap();
    for args in [
        &[][..],
        &["parts"],
        &["parts", step, "extra"],
        &["pmi"],
        &["pmi", step],
        &["pmi", "read"],
        &["pmi", "write", step],
        &["pmi", "read", step, "--part"],
        &["pmi", "read", step, "--part", "-1"],
        &["pmi", "read", step, "--parts", "0"],
        &["pmi", "check", step],
        &["pmi", "write", step, "pmi.json"],
        &["pmi", "write", step, "-o", "out.step"],
        &["pmi", "write", step, "pmi.json", "-o"],
        &["pmi", "write", step, "pmi.json", "extra", "-o", "out.step"],
        &[
            "pmi", "write", step, "pmi.json", "-o", "a.step", "-o", "b.step",
        ],
        &[
            "pmi", "write", step, "pmi.json", "-o", "out.step", "--mode", "merge",
        ],
        &["pmi", "write", step, "pmi.json", "-o", "out.step", "--mode"],
        &[
            "pmi",
            "write",
            step,
            "pmi.json",
            "-o",
            "out.step",
            "--presentation",
            "keep",
        ],
        &[
            "pmi",
            "write",
            step,
            "pmi.json",
            "-o",
            "out.step",
            "--presentation",
            "remove",
        ],
        &[
            "pmi", "write", step, "pmi.json", "-o", "out.step", "--force",
        ],
        &["correspond"],
    ] {
        let out = quiddity(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("usage: quiddity"),
            "{args:?}"
        );
    }
}

#[test]
fn parts_and_pmi_read_refuse_a_broken_file() {
    for name in ["truncated.step", "deleted_face.step"] {
        let path = broken(name);
        let path = path.to_str().unwrap();
        for args in [&["parts", path][..], &["pmi", "read", path]] {
            let out = quiddity(args);
            assert_eq!(out.status.code(), Some(1), "{args:?}");
            assert!(out.stdout.is_empty(), "{args:?}");
            let err = String::from_utf8_lossy(&out.stderr);
            assert!(err.starts_with("quiddity: "), "{args:?}: {err}");
        }
    }
    let missing = broken("no_such_file.step");
    let out = quiddity(&["pmi", "read", missing.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn a_sound_file_is_recognised() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rejected_cylinder.step");
    let out = quiddity(&[path.to_str().unwrap()]);
    assert!(out.status.success());
    assert!(out.stderr.is_empty());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(json.get("fingerprints").is_some());
    let document = &json["document"];
    assert_eq!(
        document["schema_version"],
        quiddity::recognition::SCHEMA_VERSION
    );
    assert!(document["records"].is_array());
    // Recognised in its own frame, reported in the file's coordinates.
    assert_eq!(document["coordinate_space"], "file");
    assert_eq!(document["frame"]["status"], "inferred");
}

/// A part with no plane or cylinder has no frame: it is recognised as placed in the file, and
/// says so on stderr and in the document.
#[test]
fn a_part_without_a_frame_is_recognised_as_placed_with_a_warning() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/captured/frames/sphere.step");
    let out = quiddity(&[path.to_str().unwrap()]);
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("warning: no part frame (no-analytic-direction); recognised as placed"),
        "{stderr}"
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        json["document"]["frame"],
        serde_json::json!({"status": "refused", "reason": "no-analytic-direction"})
    );
}

#[test]
fn the_document_lists_each_feature_with_its_record_type_and_faces() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/filleted_plate_with_holes.step");
    let out = quiddity(&[path.to_str().unwrap()]);
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let records: Vec<(&str, &str, &serde_json::Value)> = json["document"]["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["id"].as_str().unwrap(),
                r["record_type"].as_str().unwrap(),
                &r["faces"],
            )
        })
        .collect();
    let faces = |v: &[usize]| serde_json::json!(v);
    // Records are in the part's frame's order (its x is the file's z, its z the file's -x),
    // which does not depend on where the part is placed.
    assert_eq!(
        records,
        [
            ("fillets/0", "Fillet", &faces(&[7])),
            ("fillets/1", "Fillet", &faces(&[2])),
            ("fillets/2", "Fillet", &faces(&[8])),
            ("fillets/3", "Fillet", &faces(&[3])),
            ("holes/0", "HoleRecord", &faces(&[10])),
            ("holes/1", "HoleRecord", &faces(&[11])),
            ("holes/2", "HoleRecord", &faces(&[12])),
            ("hole_patterns/0", "LinearArray", &faces(&[10, 11, 12])),
            // recognise_blends finds the fillets' faces too, as Python's does (4 blends), and
            // reconciliation rejects each as a chain every face of which is a fillet's
            // (`blend.chain_superseded_by_fillet`), as Python's aggregate does.
        ]
    );
    // The existing keys are kept: the records at the top level, the fingerprints beside them.
    assert_eq!(
        json["fillets"][0],
        json["document"]["records"][0]["parameters"]
    );
    assert!(json["fingerprints"]["features"].is_array());
}

#[test]
fn a_header_only_step_is_refused() {
    let err = refused("header_only.step");
    assert!(err.contains("STEP parse error"), "{err}");
}

#[test]
fn an_empty_data_section_is_refused() {
    let err = refused("empty_data.step");
    assert!(err.contains("no solid or shell"), "{err}");
}

#[test]
fn a_file_cut_off_mid_entity_is_refused() {
    let err = refused("truncated.step");
    assert!(err.contains("parse error"), "{err}");
}

#[test]
fn a_file_cut_at_an_entity_and_closed_is_refused() {
    let err = refused("truncated_closed.step");
    assert!(
        err.contains("#105 is referenced but not in the file"),
        "{err}"
    );
    assert!(
        err.contains("#109 is referenced but not in the file"),
        "{err}"
    );
}

#[test]
fn a_deleted_face_is_refused() {
    let err = refused("deleted_face.step");
    assert!(
        err.contains("#109 is referenced but not in the file"),
        "{err}"
    );
}

#[test]
fn an_unresolvable_closed_edge_curve_is_refused() {
    let err = refused("unresolved_closed_curve.step");
    assert!(err.contains("closed edge's curve (Hyperbola"), "{err}");
}

#[test]
fn an_unresolved_open_edge_curve_is_reported() {
    let out = quiddity(&[broken("unresolved_open_curve.step").to_str().unwrap()]);
    assert!(out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("geometry did not resolve (faces [], edges [1])"),
        "{err}"
    );
}

#[test]
fn brep_writes_the_part_and_reports() {
    let usage = String::from_utf8_lossy(&quiddity(&["--help"]).stdout).into_owned();
    assert!(
        usage.contains("quiddity brep <file.step> -o <out.step> [--pmi <pmi.json>]"),
        "{usage}"
    );
    let dir = std::env::temp_dir().join(format!("quiddity-cli-brep-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let input = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/golden_simple_through_hole.step");
    let out_path = dir.join("hole.step");
    let out = quiddity(&[
        "brep",
        input.to_str().unwrap(),
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let before = quiddity::read_step_file(&input).unwrap();
    let after = quiddity::read_step_file(&out_path).unwrap();
    assert_eq!(report["faces"], before.faces.len());
    assert_eq!(after.faces.len(), before.faces.len());
    assert_eq!(after.edges.len(), before.edges.len());
    assert_eq!(report["face_order_preserved"], true);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn brep_carries_the_parts_pmi_and_names_what_it_does_not() {
    let dir = std::env::temp_dir().join(format!("quiddity-cli-brep-pmi-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ap242");
    // All CTC-01 requirements are carried, including its four dimensions without a value.
    let input = fixtures.join("nist/nist_ctc_01_asme1_ap242-e1.stp.gz");
    let out_path = dir.join("ctc01.step");
    let out = quiddity(&[
        "brep",
        input.to_str().unwrap(),
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        report["pmi"]["carried_items"].as_u64().unwrap() > 40,
        "{report}"
    );
    let refused = report["pmi"]["refused"].as_array().unwrap();
    assert!(refused.is_empty(), "{report}");
    let read = quiddity(&["pmi", "read", out_path.to_str().unwrap()]);
    assert!(read.status.success());
    let after: serde_json::Value = serde_json::from_slice(&read.stdout).unwrap();
    let dimensions = after["parts"][0]["pmi"]["dimensions"].as_array().unwrap();
    assert_eq!(dimensions.len(), 12);
    assert_eq!(
        dimensions
            .iter()
            .filter(|d| d.get("nominal").is_none())
            .count(),
        4
    );
    let expected = native_pmi(&input);
    let actual = native_pmi(&out_path);
    let differences = haecceity::pmi::write::differences(
        &haecceity::pmi::write::standard_form(&expected.parts[0]),
        &actual.parts[0],
    );
    assert!(differences.is_empty(), "{differences:?}");
    // Several parts: their PMI is listed as not carried.
    let input = fixtures.join("write/editor_assembly_pin_alone.step.gz");
    let out_path = dir.join("assembly.step");
    let out = quiddity(&[
        "brep",
        input.to_str().unwrap(),
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        !report["pmi"]["parts_not_carried"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{report}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn brep_writes_the_pmi_document_given_in_place_of_the_files() {
    let dir = std::env::temp_dir().join(format!("quiddity-cli-brep-doc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ap242");
    let input = fixtures.join("nist/nist_ctc_01_asme1_ap242-e1.stp.gz");
    let input = input.to_str().unwrap();
    let tolerances = |path: &str| -> usize {
        let read = quiddity(&["pmi", "read", path]);
        assert!(
            read.status.success(),
            "{}",
            String::from_utf8_lossy(&read.stderr)
        );
        let doc: serde_json::Value = serde_json::from_slice(&read.stdout).unwrap();
        doc["parts"][0]["pmi"]["tolerances"]
            .as_array()
            .map_or(0, Vec::len)
    };
    // Edit the file's PMI as JSON: keep two of its six tolerances.
    let read = quiddity(&["pmi", "read", input]);
    let mut doc: serde_json::Value = serde_json::from_slice(&read.stdout).unwrap();
    assert_eq!(tolerances(input), 6);
    doc["parts"][0]["pmi"]["tolerances"]
        .as_array_mut()
        .unwrap()
        .truncate(2);
    let edited = dir.join("edited.json");
    std::fs::write(&edited, doc.to_string()).unwrap();
    let out_path = dir.join("ctc01.step");
    let out = quiddity(&[
        "brep",
        input,
        "-o",
        out_path.to_str().unwrap(),
        "--pmi",
        edited.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        report["pmi"]["carried_items"].as_u64().unwrap() > 0,
        "{report}"
    );
    assert_eq!(tolerances(out_path.to_str().unwrap()), 2);
    // A document bound to another file is refused, and nothing is written.
    let other = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/golden_simple_through_hole.step");
    let refused_path = dir.join("refused.step");
    let out = quiddity(&[
        "brep",
        other.to_str().unwrap(),
        "-o",
        refused_path.to_str().unwrap(),
        "--pmi",
        edited.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("not the file's sha256"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!refused_path.exists());
    // Usage errors.
    assert_eq!(quiddity(&["brep", input]).status.code(), Some(2));
    assert_eq!(
        quiddity(&["brep", input, "-o", "x.step", "--pmi"])
            .status
            .code(),
        Some(2)
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Genuine schema-5/v3 captures from v0.1.1 (a27b706): default golden recognition
/// and caller-space 10252 recognition. All five oriented features must follow the merge.
#[test]
fn v011_oriented_fingerprints_correspond_to_merged_step() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let bytes = include_bytes!("fixtures/recognition011_oriented.json.gz");
    let capture: serde_json::Value =
        serde_json::from_reader(flate2::read::GzDecoder::new(&bytes[..])).unwrap();
    assert_eq!(capture["source_tag"], "v0.1.1");
    assert_eq!(
        capture["source_commit"],
        "a27b7064a65358563f0168e97cb50d558397ece7"
    );
    let captured_oriented = capture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| {
            assert_eq!(
                case["document"]["schema_version"],
                "quiddity-rust/recognition/5"
            );
            assert_eq!(case["document"]["package"]["version"], "0.1.1");
            assert_eq!(
                case["fingerprints"]["fingerprint_version"],
                "quiddity-rust/fingerprint/3"
            );
            case["fingerprints"]["features"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|f| f["family"].as_str().unwrap().starts_with("oriented_"))
                .count()
        })
        .sum::<usize>();
    assert_eq!(
        captured_oriented, 5,
        "three slots, one pattern, one chamfer"
    );
    let has_corpus = std::env::var_os("QUIDDITY_CORPUS").is_some_and(|v| v != "off");
    let temporary = std::env::temp_dir().join(format!("quiddity-v011-{}", std::process::id()));
    std::fs::create_dir_all(&temporary).unwrap();
    let mut formerly_oriented = 0;
    for case in capture["cases"].as_array().unwrap() {
        let path = case["path"].as_str().unwrap();
        let step = if case["kind"] == "fixture" {
            root.join("tests/fixtures").join(path)
        } else if let Some(corpus) = std::env::var_os("QUIDDITY_CORPUS").filter(|v| v != "off") {
            PathBuf::from(corpus).join(path)
        } else {
            assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
            continue;
        };
        let old = temporary.join("v011.json");
        std::fs::write(&old, serde_json::to_vec(case).unwrap()).unwrap();
        let output = quiddity(&["correspond", old.to_str().unwrap(), step.to_str().unwrap()]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let recognised = quiddity(&[step.to_str().unwrap()]);
        assert!(recognised.status.success());
        let recognised: serde_json::Value = serde_json::from_slice(&recognised.stdout).unwrap();
        assert_eq!(
            recognised["document"]["schema_version"],
            "quiddity-rust/recognition/6"
        );
        assert_eq!(
            recognised["document"]["source_sha256"],
            case["input_sha256"]
        );
        assert_eq!(
            recognised["fingerprints"]["fingerprint_version"],
            "quiddity-rust/fingerprint/3"
        );
        assert_eq!(result["fingerprint_version"], "quiddity-rust/fingerprint/3");
        for old in case["fingerprints"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["family"].as_str().unwrap().starts_with("oriented_"))
        {
            let captured = case["document"]["records"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["id"] == old["id"])
                .expect("captured schema-5 record");
            assert_eq!(captured["family"], old["family"]);
            assert_eq!(captured["faces"], old["faces"]);
            formerly_oriented += 1;
            let pair = result["features"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["old"] == old["id"])
                .unwrap();
            // #91: inferred-frame recognition now keeps the golden plate on its file axes, so
            // it proves the same four walls per slot as file-axis recognition and every
            // cross-frame pair is carried (it was adapted while the frame turned 30 degrees).
            assert_eq!(pair["class"], "carried", "{path}: {pair}");
            let new = recognised["document"]["records"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["id"] == pair["new"])
                .expect("merged record");
            assert_eq!(
                new["family"],
                old["family"]
                    .as_str()
                    .unwrap()
                    .strip_prefix("oriented_")
                    .unwrap()
            );
            if case["kind"] == "fixture" {
                // #91: the captured oriented records claimed each slot's two side walls; on
                // the plate's file axes each slot also claims its two end walls, so the merged
                // record holds every captured face and as many again.
                let faces = new["faces"].as_array().unwrap();
                assert_eq!(
                    faces.len(),
                    if old["family"] == "oriented_slot_patterns" {
                        12
                    } else {
                        4
                    }
                );
                assert!(
                    old["faces"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|f| faces.contains(f))
                );
            } else {
                assert_eq!(new["faces"], old["faces"]);
            }
        }
    }
    assert_eq!(
        formerly_oriented,
        if has_corpus { 5 } else { 4 },
        "every available captured oriented feature must pair"
    );
    std::fs::remove_dir_all(temporary).unwrap();
}

#[cfg(not(feature = "json-import"))]
#[test]
fn source_presentation_flag_requires_json_import() {
    let out = quiddity(&[
        "pmi",
        "write",
        "geometry.step",
        "pmi.json",
        "-o",
        "out.step",
        "--source-presentation",
        "source.json",
    ]);
    assert!(!out.status.success());
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(error.contains("unknown option") && error.contains("--source-presentation"));
}

#[test]
fn brep_carries_portable_derived_units_from_nist() {
    let dir = std::env::temp_dir().join(format!("quiddity-cli-brep-units-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ap242/nist");
    for model in ["07", "10"] {
        let input = fixtures.join(format!("nist_ftc_{model}_asme1_ap242-e2.stp.gz"));
        let output = dir.join(format!("ftc{model}.step"));
        let result = quiddity(&[
            "brep",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ]);
        assert!(
            result.status.success(),
            "FTC-{model}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert!(
            report["pmi"]["refused"].as_array().unwrap().is_empty(),
            "{report}"
        );
        let before = native_pmi(&input);
        let after = native_pmi(&output);
        assert!(
            before
                .findings
                .iter()
                .all(|f| f.kind != haecceity::pmi::FindingKind::Unitless),
            "{:?}",
            before.findings
        );
        let units = |read: &haecceity::pmi::PmiRead| {
            let mut units: Vec<_> = read
                .parts
                .iter()
                .flat_map(|p| &p.attributes)
                .flat_map(|a| &a.items)
                .filter_map(|(_, value)| match value {
                    haecceity::pmi::AttributeValue::OtherMeasure { measure, unit, .. } => {
                        Some((measure.clone(), unit.clone()))
                    }
                    _ => None,
                })
                .collect();
            units.sort();
            units
        };
        let expected = units(&before);
        assert_eq!(expected.len(), 13);
        assert_eq!(
            expected
                .iter()
                .filter(|(_, unit)| unit.starts_with("step-unit:"))
                .count(),
            12
        );
        assert_eq!(units(&after), expected, "FTC-{model}");
        // Freshly emitted unit graphs can share equal named units at different ids.
        // A subsequent native replacement must reuse equal content without ambiguity.
        let bytes = std::fs::read(&output).unwrap();
        let doc = haecceity::p21::Document::parse(bytes.clone()).unwrap();
        let parts = haecceity::step::read_part_definitions(&bytes).unwrap();
        let requested: Vec<_> = after
            .parts
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, p)| (haecceity::pmi::PartId(i), p))
            .collect();
        let (edit, _) = haecceity::pmi::write(
            &doc,
            &parts,
            &requested,
            haecceity::pmi::Mode::Replace,
            haecceity::pmi::write::PresentationPolicy::RemovePresentation,
        )
        .unwrap();
        let written = doc.apply(&edit).unwrap().bytes;
        let defs = haecceity::step::read_part_definitions(&written).unwrap();
        let doc = haecceity::p21::Document::parse(written).unwrap();
        let again = haecceity::pmi::read(&doc, &defs).unwrap();
        assert_eq!(units(&again), expected, "FTC-{model}: second write");
    }
    std::fs::remove_dir_all(dir).unwrap();
}
