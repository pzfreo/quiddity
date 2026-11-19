//! Several solids in one part, and void shells, are stated in the document (#300).
use std::path::Path;
use std::process::Command;

use quiddity::recognition::{Bodies, RecognitionDocument};

fn run(name: &str) -> (RecognitionDocument, String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/multi_solid")
        .join(name);
    let out = Command::new(env!("CARGO_BIN_EXE_quiddity"))
        .arg(&path)
        .output()
        .expect("quiddity runs");
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let document = serde_json::from_value(json["document"].clone()).unwrap();
    (document, String::from_utf8_lossy(&out.stderr).into_owned())
}

#[test]
fn separate_solids_are_counted_and_warned() {
    let (document, stderr) = run("two_boxes.step.gz");
    let bodies = document.bodies.expect("two solids are stated");
    assert_eq!((bodies.solids, bodies.void_shells), (2, 0));
    // Every face belongs to exactly one solid, and the lists partition the part.
    let mut faces: Vec<usize> = bodies.solid_faces.concat();
    faces.sort_unstable();
    assert_eq!(faces, (0..document.face_count).collect::<Vec<_>>());
    assert!(bodies.solid_faces.iter().all(|f| !f.is_empty()));
    assert!(
        stderr.contains("2 solids recognised together as one part"),
        "{stderr}"
    );
}

#[test]
fn void_shells_are_counted_without_a_solid_warning() {
    let (document, stderr) = run("brep_with_voids.step.gz");
    let bodies = document.bodies.expect("a void shell is stated");
    assert_eq!((bodies.solids, bodies.void_shells), (1, 1));
    assert!(bodies.solid_faces.is_empty());
    assert!(!stderr.contains("solids recognised together"), "{stderr}");
}

#[test]
fn a_single_solid_states_nothing_and_old_documents_still_read() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/filleted_box_one_edge.step");
    let out = Command::new(env!("CARGO_BIN_EXE_quiddity"))
        .arg(&path)
        .output()
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(json["document"].get("bodies").is_none());
    let document: RecognitionDocument = serde_json::from_value(json["document"].clone()).unwrap();
    assert_eq!(document.bodies, None::<Bodies>);
}
