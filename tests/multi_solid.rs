//! Several solids in one part, and void shells, are stated in the document (#300).
use std::path::Path;
use std::process::Command;

use quiddity::recognition::{Bodies, RecognitionDocument};

fn run(name: &str) -> (RecognitionDocument, String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/multi_solid")
        .join(name);
    run_path(&path)
}

fn run_path(path: &Path) -> (RecognitionDocument, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_quiddity"))
        .arg(path)
        .output()
        .expect("quiddity runs");
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let document = serde_json::from_value(json["document"].clone()).unwrap();
    (document, String::from_utf8_lossy(&out.stderr).into_owned())
}

#[test]
fn separate_solids_are_counted_and_warned() {
    let (document, stderr) = run("sphere_and_box.step.gz");
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

#[test]
fn two_boxes_have_their_own_face_lists_in_an_inferred_frame() {
    let (document, stderr) = run("two_boxes.step.gz");
    assert!(matches!(
        document.frame,
        quiddity::frames::RecordFrame::Inferred(_)
    ));
    let bodies = document.bodies.expect("two solids are stated");
    assert_eq!((bodies.solids, bodies.void_shells), (2, 0));
    assert_eq!(
        bodies.solid_faces,
        [(0..6).collect::<Vec<_>>(), (6..12).collect::<Vec<_>>()]
    );
    assert!(
        stderr.contains("2 solids recognised together as one part"),
        "{stderr}"
    );
}

#[test]
fn void_shells_are_the_declared_ones_not_disjoint_lumps() {
    // One MANIFOLD_SOLID_BREP whose shell holds both boxes' faces: two edge-connected
    // groups, but no void shell is declared.
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multi_solid/two_boxes.step.gz"),
    )
    .unwrap();
    let mut text = String::new();
    std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(&bytes[..]), &mut text)
        .unwrap();
    let text = text
        .replace(
            "(#155,#233,#272,#295,#318,#325));",
            "(#155,#233,#272,#295,#318,#325,#1155,#1233,#1272,#1295,#1318,#1325));",
        )
        .replace("(#18,#327,#1327)", "(#18,#327)");
    assert!(text.contains("#1155,#1233"));
    let path = std::env::temp_dir().join(format!("quiddity-lumps-{}.step", std::process::id()));
    std::fs::write(&path, text).unwrap();
    let (document, stderr) = run_path(&path);
    std::fs::remove_file(&path).unwrap();
    assert!(!stderr.contains("solids recognised together"), "{stderr}");
    assert_eq!(document.face_count, 12);
    assert_eq!(
        document.bodies, None,
        "one solid, no declared void shell: nothing to state"
    );
}
