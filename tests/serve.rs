//! `quiddity serve` (`quiddity::serve`): JSON-lines recognition and correspondence. The results
//! are the CLI's, ids are echoed, every failure is a structured error and the server keeps
//! serving after one, and the same request gives the same bytes. (The panic catch is unit-tested
//! in `src/serve.rs`: no request triggers a panic honestly.)

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use quiddity::serve::{respond, serve};
use serde_json::{Value, json};

fn fixture(name: &str) -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    path.to_str().unwrap().to_owned()
}

/// `quiddity <args>`'s stdout as JSON, asserting it succeeded.
fn cli(args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_quiddity"))
        .args(args)
        .output()
        .expect("quiddity runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

/// `quiddity serve` fed *input*: its stdout, asserting it ended cleanly.
fn served(input: &str) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quiddity"))
        .arg("serve")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("quiddity serve runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// The library's response to one request, parsed.
fn ask(request: &Value) -> Value {
    let line = respond(&request.to_string()).expect("a response");
    assert!(!line.contains('\n'), "a response is one line");
    serde_json::from_str(&line).unwrap()
}

/// Asserts *response* is a failure with *code*, echoing *id*; returns its message.
fn failure(response: &Value, id: Value, code: &str) -> String {
    assert_eq!(response["id"], id, "{response}");
    assert_eq!(response["error"]["code"], code, "{response}");
    assert!(response.get("result").is_none(), "{response}");
    response["error"]["message"].as_str().unwrap().to_owned()
}

#[test]
fn recognise_returns_what_the_cli_writes() {
    for name in ["filleted_plate_with_holes.step", "rejected_cylinder.step"] {
        let path = fixture(name);
        let response = ask(&json!({"id": name, "op": "recognise", "step": path}));
        assert_eq!(response["id"], name);
        assert!(response.get("warnings").is_none(), "{name}");
        assert_eq!(response["result"], cli(&[&path]), "{name}");
        assert_eq!(
            response["result"]["document"]["schema_version"],
            quiddity::recognition::SCHEMA_VERSION
        );
    }
    let path = fixture("sheet-metal/coaxial-flanges.step.gz");
    let response = ask(&json!({"id": 1, "op": "recognise", "step": path, "k_factor": null}));
    assert!(response.get("result").is_some(), "{response}");
    assert_eq!(response["result"], cli(&[&path]));

    let response = ask(&json!({"id": 1, "op": "recognise", "step": path, "k_factor": 0.45}));
    assert_eq!(response["result"], cli(&[&path, "--k-factor", "0.45"]));
}

#[test]
fn the_result_keeps_the_cli_field_order() {
    let line = respond(
        &json!({"id": 1, "op": "recognise", "step": fixture("rejected_cylinder.step")}).to_string(),
    )
    .unwrap();
    assert!(
        line.starts_with("{\"id\":1,\"result\":{\"fillets\":["),
        "{line}"
    );
}

#[test]
fn correspond_returns_what_the_cli_writes_from_paths_or_documents() {
    let (old, new) = (
        fixture("revisions/hole_moved.old.step.gz"),
        fixture("revisions/hole_moved.new.step.gz"),
    );
    let expected = cli(&["correspond", &old, &new]);
    let response = ask(&json!({"id": 7, "op": "correspond", "old": old, "new": new}));
    assert_eq!(response["id"], 7);
    assert_eq!(response["result"], expected);
    // The hole moved and was matched, adapted (tests/correspondence.rs checks the classes).
    assert!(
        response["result"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["class"] == "adapted" && f["old"].as_str().unwrap().starts_with("holes/"))
    );

    // Previously returned recognise results, inline, give the same correspondence.
    let recognised =
        |path: &str| ask(&json!({"id": 0, "op": "recognise", "step": path}))["result"].clone();
    let (old_doc, new_doc) = (recognised(&old), recognised(&new));
    let inline = ask(&json!({"id": 8, "op": "correspond", "old": old_doc, "new": new_doc}));
    assert_eq!(inline["result"], expected);
    // And bare fingerprints too.
    let bare = ask(&json!({
        "id": 9, "op": "correspond",
        "old": old_doc["fingerprints"], "new": new_doc["fingerprints"],
    }));
    assert_eq!(bare["result"], expected);
}

#[test]
fn a_correspond_revision_may_be_a_recognition_json_file() {
    let step = fixture("filleted_plate_with_holes.step");
    let dir = std::env::temp_dir().join(format!("quiddity-serve-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let json_path = dir.join("plate.json");
    std::fs::write(&json_path, cli(&[&step]).to_string()).unwrap();
    let response = ask(&json!({
        "id": 1, "op": "correspond", "old": json_path.to_str().unwrap(), "new": step,
    }));
    std::fs::remove_dir_all(&dir).unwrap();
    let result = &response["result"];
    assert!(
        result["features"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["class"] == "carried"),
        "{result}"
    );
    assert_eq!(result["new_features"], json!([]));
}

#[test]
fn unresolved_geometry_is_reported_as_a_warning() {
    let path = fixture("broken/unresolved_open_curve.step");
    let response = ask(&json!({"id": 1, "op": "recognise", "step": path}));
    assert!(response["result"]["fingerprints"].is_object());
    let warnings = response["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0]
            .as_str()
            .unwrap()
            .contains("geometry did not resolve (faces [], edges [1])"),
        "{warnings:?}"
    );
}

/// A part with no frame is recognised as placed in the file, with a warning (the CLI's stderr
/// one) and the refusal in the document.
#[test]
fn a_part_without_a_frame_is_reported_as_a_warning() {
    let path = fixture("captured/frames/sphere.step");
    let response = ask(&json!({"id": 1, "op": "recognise", "step": path}));
    assert_eq!(
        response["result"]["document"]["frame"],
        json!({"status": "refused", "reason": "no-analytic-direction"})
    );
    let warnings = response["warnings"].as_array().unwrap();
    // This fixture already fails the kernel's strict validity check. The trust warning
    // now exposes that result alongside the existing frame warning; no records are dropped.
    let part = quiddity::read_step_file(std::path::Path::new(&path)).unwrap();
    assert!(!part.solid_is_geometrically_valid(0));
    assert_eq!(response["result"]["document"]["records"], json!([]));
    assert_eq!(warnings.len(), 2);
    assert!(warnings[0].as_str().unwrap().ends_with(
        "solids [0] fail strict geometric validity; retained 0 records; unproved records []"
    ));
    assert!(
        warnings[1]
            .as_str()
            .unwrap()
            .contains("no part frame (no-analytic-direction)"),
        "{warnings:?}"
    );
    assert_eq!(response["result"], cli(&[&path]));
}

#[test]
fn malformed_requests_are_refused_with_their_codes() {
    let parsed = |line: &str| -> Value { serde_json::from_str(&respond(line).unwrap()).unwrap() };
    let message = failure(&parsed("{\"id\": 1, \"op\""), Value::Null, "bad_json");
    assert!(message.contains("not JSON"), "{message}");
    failure(&parsed("[1, 2]"), Value::Null, "bad_request");
    // A repeated key is refused, not resolved by keeping one of its values.
    let message = failure(
        &parsed("{\"id\": 1, \"id\": 2, \"op\": \"recognise\"}"),
        Value::Null,
        "bad_request",
    );
    assert!(message.contains("repeats `id`"), "{message}");
    failure(&parsed("\"recognise\""), Value::Null, "bad_request");
    // No id to echo, or one that is neither a string nor an integer.
    failure(
        &ask(&json!({"op": "recognise"})),
        Value::Null,
        "missing_field",
    );
    failure(
        &ask(&json!({"id": 1.5, "op": "recognise"})),
        Value::Null,
        "bad_request",
    );
    failure(
        &ask(&json!({"id": [1], "op": "recognise"})),
        Value::Null,
        "bad_request",
    );

    failure(&ask(&json!({"id": "a"})), json!("a"), "missing_field");
    failure(&ask(&json!({"id": 2, "op": 3})), json!(2), "bad_request");
    let message = failure(
        &ask(&json!({"id": 3, "op": "draw"})),
        json!(3),
        "unknown_op",
    );
    assert!(message.contains("`draw`"), "{message}");

    let message = failure(
        &ask(&json!({"id": 4, "op": "recognise"})),
        json!(4),
        "missing_field",
    );
    assert!(message.contains("`step`"), "{message}");
    failure(
        &ask(&json!({"id": 5, "op": "recognise", "step": 5})),
        json!(5),
        "bad_request",
    );
    let step = fixture("rejected_cylinder.step");
    let message = failure(
        &ask(&json!({"id": 6, "op": "recognise", "step": step, "frame": "part"})),
        json!(6),
        "bad_request",
    );
    assert!(message.contains("`frame`"), "{message}");

    let message = failure(
        &ask(&json!({"id": 6, "op": "recognise", "step": step, "k_factor": 0.0})),
        json!(6),
        "bad_request",
    );
    assert_eq!(message, "k_factor must be finite and in (0, 1] (got 0)");
    for k in [json!("0.45"), json!(true), json!([]), json!({})] {
        let message = failure(
            &ask(&json!({"id": 6, "op": "recognise", "step": step, "k_factor": k})),
            json!(6),
            "bad_request",
        );
        assert_eq!(message, "`k_factor` must be a number");
    }

    let message = failure(
        &ask(&json!({"id": 7, "op": "correspond", "old": step})),
        json!(7),
        "missing_field",
    );
    assert!(message.contains("`new`"), "{message}");
    failure(
        &ask(&json!({"id": 8, "op": "correspond", "old": step, "new": 3})),
        json!(8),
        "bad_request",
    );
}

#[test]
fn files_that_cannot_be_read_are_refused() {
    let missing = fixture("no_such_part.step");
    let message = failure(
        &ask(&json!({"id": 1, "op": "recognise", "step": missing})),
        json!(1),
        "unreadable",
    );
    assert!(message.contains("no_such_part.step"), "{message}");
    failure(
        &ask(
            &json!({"id": 2, "op": "correspond", "old": fixture("no_such.json"), "new": fixture("rejected_cylinder.step")}),
        ),
        json!(2),
        "unreadable",
    );
}

#[test]
fn broken_step_files_are_refused_with_the_kernel_refusal() {
    for (name, expected) in [
        ("broken/truncated.step", "parse error"),
        ("broken/empty_data.step", "no solid or shell"),
        ("broken/header_only.step", "STEP parse error"),
        (
            "broken/deleted_face.step",
            "#109 is referenced but not in the file",
        ),
        (
            "broken/unresolved_closed_curve.step",
            "closed edge's curve (Hyperbola",
        ),
    ] {
        let response = ask(&json!({"id": name, "op": "recognise", "step": fixture(name)}));
        let message = failure(&response, json!(name), "broken_step");
        assert!(message.contains(expected), "{name}: {message}");
        // A correspond revision is refused the same way.
        let response = ask(&json!({
            "id": 1, "op": "correspond", "old": fixture("rejected_cylinder.step"), "new": fixture(name),
        }));
        failure(&response, json!(1), "broken_step");
    }
}

#[test]
fn revisions_without_valid_fingerprints_are_refused() {
    let step = fixture("rejected_cylinder.step");
    // A JSON file that is not JSON (a STEP file's text, named .json).
    let dir = std::env::temp_dir().join(format!("quiddity-serve-doc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let not_json = dir.join("part.json");
    std::fs::copy(&step, &not_json).unwrap();
    let response = ask(&json!({
        "id": 1, "op": "correspond", "old": not_json.to_str().unwrap(), "new": step,
    }));
    std::fs::remove_dir_all(&dir).unwrap();
    let message = failure(&response, json!(1), "bad_document");
    assert!(message.contains("not JSON"), "{message}");

    // The recognition document alone carries no fingerprints.
    let recognised = ask(&json!({"id": 0, "op": "recognise", "step": step}))["result"].clone();
    let message = failure(
        &ask(&json!({"id": 2, "op": "correspond", "old": recognised["document"], "new": step})),
        json!(2),
        "bad_document",
    );
    assert!(message.contains("no fingerprints"), "{message}");

    // Fingerprints that do not hold together: a face's neighbour beyond the faces listed.
    let mut fingerprints = recognised["fingerprints"].clone();
    fingerprints["faces"].as_array_mut().unwrap().pop();
    let message = failure(
        &ask(&json!({"id": 3, "op": "correspond", "old": recognised, "new": fingerprints})),
        json!(3),
        "bad_document",
    );
    assert!(message.contains("the new fingerprints"), "{message}");
}

#[test]
fn one_stream_serves_every_request_in_order_and_keeps_serving_after_failures() {
    let step = fixture("rejected_cylinder.step");
    let requests = [
        json!({"id": 1, "op": "recognise", "step": step}).to_string(),
        "not json".to_owned(),
        String::new(),
        json!({"id": "two", "op": "nothing"}).to_string(),
        json!({"id": 3, "op": "recognise", "step": fixture("broken/truncated.step")}).to_string(),
        json!({"id": 4, "op": "correspond", "old": step, "new": step}).to_string(),
    ];
    let input = requests.join("\n") + "\n";
    let output = served(&input);
    let responses: Vec<Value> = output
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(responses.len(), 5, "the blank line is skipped: {output}");
    assert_eq!(responses[0]["result"], cli(&[&step]));
    failure(&responses[1], Value::Null, "bad_json");
    failure(&responses[2], json!("two"), "unknown_op");
    failure(&responses[3], json!(3), "broken_step");
    assert_eq!(responses[4]["id"], 4);
    assert_eq!(responses[4]["result"], cli(&["correspond", &step, &step]));

    // The binary and the library write the same lines.
    let mut library = Vec::new();
    serve(input.as_bytes(), &mut library).unwrap();
    assert_eq!(String::from_utf8(library).unwrap(), output);
}

#[test]
fn a_line_that_is_not_utf8_is_refused_and_serving_goes_on() {
    let mut input = b"{\"id\":1,\"op\":\"x\"}\n\xff\xfe\n".to_vec();
    input.extend_from_slice(b"{\"id\":2,\"op\":\"x\"}\r\n");
    let mut output = Vec::new();
    serve(input.as_slice(), &mut output).unwrap();
    let output = String::from_utf8(output).unwrap();
    let responses: Vec<Value> = output
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(responses.len(), 3, "{output}");
    failure(&responses[0], json!(1), "unknown_op");
    let message = failure(&responses[1], Value::Null, "bad_json");
    assert!(message.contains("not UTF-8"), "{message}");
    failure(&responses[2], json!(2), "unknown_op");

    // The binary too keeps serving.
    let mut child = Command::new(env!("CARGO_BIN_EXE_quiddity"))
        .arg("serve")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("quiddity serve runs");
    child.stdin.take().unwrap().write_all(&input).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), output);
}

#[test]
fn the_same_request_gives_the_same_bytes() {
    let (old, new) = (
        fixture("revisions/pattern_grown.old.step.gz"),
        fixture("revisions/pattern_grown.new.step.gz"),
    );
    let requests = [
        json!({"id": 1, "op": "recognise", "step": fixture("filleted_plate_with_holes.step")}),
        json!({"id": 2, "op": "correspond", "old": old, "new": new}),
    ];
    let input = requests
        .iter()
        .chain(requests.iter())
        .map(|r| r.to_string() + "\n")
        .collect::<String>();
    let first = served(&input);
    let lines: Vec<&str> = first.lines().collect();
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[0], lines[2], "twice in one process");
    assert_eq!(lines[1], lines[3], "twice in one process");
    assert_eq!(
        served(&input),
        first,
        "in another process (other hash seeds)"
    );
}
