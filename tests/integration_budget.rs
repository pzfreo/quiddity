//! The caller's per-face integration budget (#307 item 3): a face whose area and moment
//! integration would take more work units than the budget is refused alone, as an unmeasured
//! face is (#304), whatever the face order or placement; without a budget nothing changes.

use std::path::{Path, PathBuf};
use std::process::Command;

use quiddity::kernel::brep::{Part, Solid};
use quiddity::recognition::{FaceAccounting, RecognitionDocument};
use quiddity::{RecognitionOptions, read_step_file};
use serde_json::Value;

/// A small part whose one B-spline face's integration costs far more than any other face's
const EXPENSIVE: &str = "integration_budget_sheet.step.gz";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn options(budget: Option<u64>) -> RecognitionOptions {
    let mut options = RecognitionOptions::default();
    options.integration_budget = budget;
    options
}

/// Every face's integration work, unbounded.
fn work(part: &Part) -> Vec<u64> {
    part.with_integration_budget(u64::MAX)
        .integration_budget()
        .unwrap()
        .work
        .clone()
}

/// The costliest face and a budget halfway between its work and the next costliest face's.
fn expensive_face(part: &Part) -> (usize, u64) {
    let work = work(part);
    let mut order: Vec<usize> = (0..work.len()).collect();
    order.sort_by_key(|&f| std::cmp::Reverse(work[f]));
    let (top, next) = (work[order[0]], work[order[1]]);
    assert!(
        top > 2 * next,
        "one face must dominate: {top} against {next}"
    );
    (order[0], next + (top - next) / 2)
}

fn document(path: &Path, budget: Option<u64>) -> RecognitionDocument {
    quiddity::serve::recognise_file_with(path.to_str().unwrap(), &options(budget))
        .unwrap()
        .0
        .document
}

#[test]
fn only_the_expensive_face_is_refused() {
    let path = fixture(EXPENSIVE);
    let part = read_step_file(&path).unwrap();
    let (face, budget) = expensive_face(&part);
    let budgeted = part.with_integration_budget(budget);
    let report = budgeted.integration_budget().unwrap();
    assert_eq!(report.refused, [face]);
    // Abandoned as soon as the count passed the limit.
    assert_eq!(report.work[face], budget + 1);
    assert!(budgeted.face_moments(face).is_none());
    assert!(budgeted.face_mass(face).is_none());
    for other in (0..part.faces.len()).filter(|&f| f != face) {
        assert_eq!(
            budgeted.face_moments(other).map(|m| m.area.to_bits()),
            part.face_moments(other).map(|m| m.area.to_bits()),
            "face {other} is integrated as without a budget"
        );
    }

    let doc = document(&path, Some(budget));
    let summary = doc.budget.as_ref().expect("the budget is reported");
    assert_eq!(summary.limit, budget);
    assert_eq!(summary.refused_faces, [face]);
    assert_eq!(summary.count, 1);
    let refusals: Vec<_> = doc
        .records
        .iter()
        .filter(|r| r.family == "integration_budget_refusals")
        .collect();
    assert_eq!(refusals.len(), 1);
    assert!(refusals[0].is_refusal());
    assert_eq!(refusals[0].record_type, "IntegrationBudgetRefusal");
    assert_eq!(refusals[0].parameters["reason"], "integration_budget");
    assert_eq!(refusals[0].faces, [face]);
    let FaceAccounting::Refused { refusals: why } = &doc.face_accounting.as_ref().unwrap()[face]
    else {
        panic!("the refused face is not accounted as refused")
    };
    assert_eq!(why[0].record_id, refusals[0].id);
    // No record defines the refused face; the frame needs every face's volume, so it is
    // refused for the budget and records stay in file coordinates.
    assert!(
        doc.records
            .iter()
            .all(|r| r.is_refusal() || !r.claimed_faces().any(|f| f == face))
    );
    assert!(summary.frame_refused, "the frame needs the refused face");
    let frame = serde_json::to_value(&doc.frame).unwrap();
    assert_eq!(frame["reason"], "integration-budget", "{frame}");
    assert_eq!(doc.coordinate_space, "file");
    assert!(
        !doc.records
            .iter()
            .any(|r| r.family == "unmeasured_face_refusals")
    );
}

/// The faces of *part* in the order `order` (new face i is old face `order[i]`), one solid.
fn reordered(part: &Part, order: &[usize]) -> Part {
    let mut position = vec![0; order.len()];
    for (new, &old) in order.iter().enumerate() {
        position[old] = new;
    }
    let faces = order.iter().map(|&old| part.faces[old].clone()).collect();
    let solids = part
        .solids
        .iter()
        .map(|s| {
            let mut solid: Solid = s.clone();
            solid.faces = s.faces.iter().map(|&f| position[f]).collect();
            solid
        })
        .collect();
    Part::new(faces, part.edges.clone(), solids)
}

#[test]
fn the_verdict_follows_the_face_not_its_order_or_placement() {
    let part = read_step_file(&fixture(EXPENSIVE)).unwrap();
    let (face, budget) = expensive_face(&part);
    let n = part.faces.len();

    // Face order: each face's work is its own, wherever it is listed.
    let forward = reordered(&part, &(0..n).collect::<Vec<_>>());
    let reverse_order: Vec<usize> = (0..n).rev().collect();
    let reversed = reordered(&part, &reverse_order);
    let (a, b) = (work(&forward), work(&reversed));
    for (new, &old) in reverse_order.iter().enumerate() {
        assert_eq!(b[new], a[old], "face {old}");
    }
    let refused = reversed.with_integration_budget(budget);
    assert_eq!(
        refused.integration_budget().unwrap().refused,
        [reverse_order.iter().position(|&f| f == face).unwrap()]
    );

    // Rigid motion: the same face is refused.
    let (s, c) = (0.6_f64.sin(), 0.6_f64.cos());
    let moved = part
        .transformed(&[
            [c, -s, 0.0, 120.0],
            [s * 0.8, c * 0.8, 0.6, -35.0],
            [-s * 0.6, -c * 0.6, 0.8, 7.5],
        ])
        .unwrap();
    assert_eq!(
        moved
            .with_integration_budget(budget)
            .integration_budget()
            .unwrap()
            .refused,
        [face]
    );
    // A transform carries the verdict rather than measuring again.
    let carried = part
        .with_integration_budget(budget)
        .transformed(&[
            [1.0, 0.0, 0.0, 5.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .unwrap();
    assert!(carried.integration_refused(face));
    assert!(carried.face_moments(face).is_none());
}

#[test]
fn no_budget_is_byte_identical() {
    for name in [
        EXPENSIVE,
        "unmeasured_face_closed.step",
        "filleted_plate_with_holes.step",
    ] {
        let path = fixture(name);
        let path = path.to_str().unwrap();
        let plain = quiddity::file_api::recognise_file(path).unwrap();
        assert_eq!(
            quiddity::file_api::recognise_file_with(path, &options(None)).unwrap(),
            plain,
            "{name}"
        );
        assert!(!plain.contains("\"budget\""), "{name}");
        let part = read_step_file(Path::new(path)).unwrap();
        assert_eq!(
            serde_json::to_string(&quiddity::recognise_with(&part, &options(None)).fingerprints)
                .unwrap(),
            serde_json::to_string(&quiddity::correspondence::recognise(&part).fingerprints)
                .unwrap(),
            "{name}"
        );
        assert_eq!(
            quiddity::file_api::evidence_file_with(path, false, &options(None)).unwrap(),
            quiddity::file_api::evidence_file(path, false).unwrap(),
            "{name}"
        );
    }
}

#[test]
fn a_budget_that_refuses_nothing_reports_an_empty_list() {
    let path = fixture(EXPENSIVE);
    let doc = document(&path, Some(u64::MAX));
    let summary = doc.budget.unwrap();
    assert!(summary.refused_faces.is_empty());
    assert_eq!(summary.count, 0);
    let plain = document(&path, None);
    assert_eq!(doc.records, plain.records);
    assert_eq!(doc.frame, plain.frame);
}

#[test]
fn the_command_line_takes_a_budget() {
    let path = fixture(EXPENSIVE);
    let part = read_step_file(&path).unwrap();
    let (face, budget) = expensive_face(&part);
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_quiddity"))
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        (
            serde_json::from_slice::<Value>(&out.stdout).unwrap(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    let file = path.to_str().unwrap();
    let budget = budget.to_string();
    let (doc, stderr) = run(&[file, "--integration-budget", &budget]);
    assert_eq!(
        doc["document"]["budget"]["refused_faces"],
        serde_json::json!([face])
    );
    assert!(stderr.contains("integration budget"), "{stderr}");
    let (evidence, _) = run(&["evidence", file, "--integration-budget", &budget]);
    assert_eq!(
        evidence["budget"]["refused_faces"],
        serde_json::json!([face])
    );
    let (fingerprints, _) = run(&["fingerprints", file, "--integration-budget", &budget]);
    assert_eq!(fingerprints, doc["fingerprints"]);
    let (plain, _) = run(&["fingerprints", file]);
    assert_ne!(plain, fingerprints);
}

#[test]
fn serve_manifest_and_json_fingerprints_honour_the_budget() {
    let path = fixture(EXPENSIVE);
    let part = read_step_file(&path).unwrap();
    let (face, budget) = expensive_face(&part);
    let dir = tempfile::tempdir().unwrap();
    // The manifest reads uncompressed sources by ranges.
    let step = dir.path().join("sheet.step");
    let mut text = Vec::new();
    std::io::Read::read_to_end(
        &mut flate2::read::GzDecoder::new(std::fs::File::open(&path).unwrap()),
        &mut text,
    )
    .unwrap();
    std::fs::write(&step, &text).unwrap();
    let run = |args: &[&str], input: Option<&str>| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_quiddity"))
            .current_dir(dir.path())
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        std::io::Write::write_all(&mut stdin, input.unwrap_or("").as_bytes()).unwrap();
        drop(stdin);
        child.wait_with_output().unwrap()
    };
    let budget_text = budget.to_string();
    let step_text = step.to_str().unwrap();

    // Manifest recognition.
    assert!(
        run(&["manifest", step_text, "-o", "manifest.json"], None)
            .status
            .success()
    );
    let out = run(
        &[
            "recognise",
            step_text,
            "--manifest",
            "manifest.json",
            "--out-dir",
            "results",
            "--integration-budget",
            &budget_text,
        ],
        None,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let product: Value = serde_json::from_slice(
        &std::fs::read(dir.path().join("results/products/p0.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        product["document"]["budget"]["refused_faces"],
        serde_json::json!([face])
    );

    // `serve`'s recognise request, with and without the optional field.
    let request =
        |extra: &str| format!("{{\"id\":1,\"op\":\"recognise\",\"step\":{step_text:?}{extra}}}\n");
    let lines = format!(
        "{}{}{}",
        request(&format!(",\"integration_budget\":{budget}")),
        request(""),
        request(",\"integration_budget\":-1"),
    );
    let out = run(&["serve"], Some(&lines));
    let responses: Vec<Value> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(responses.len(), 3);
    assert_eq!(
        responses[0]["result"]["document"]["budget"]["refused_faces"],
        serde_json::json!([face]),
        "{}",
        responses[0]
    );
    assert!(responses[1]["result"]["document"].get("budget").is_none());
    assert_eq!(
        responses[2]["error"]["code"], "bad_request",
        "{}",
        responses[2]
    );

    // Fingerprints already written as JSON cannot be budgeted.
    let json = dir.path().join("sheet.json");
    let plain = run(&[step_text], None);
    std::fs::write(&json, &plain.stdout).unwrap();
    let refused = run(
        &[
            "fingerprints",
            json.to_str().unwrap(),
            "--integration-budget",
            &budget_text,
        ],
        None,
    );
    assert_eq!(refused.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains("only to a STEP file"), "{stderr}");
    let failure =
        quiddity::serve::fingerprints_file_with(json.to_str().unwrap(), &options(Some(budget)))
            .unwrap_err();
    assert!(
        failure.message.contains("only to a STEP file"),
        "{}",
        failure.message
    );
    assert!(
        quiddity::serve::fingerprints_file_with(json.to_str().unwrap(), &options(None)).is_ok()
    );
}
