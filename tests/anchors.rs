//! `file_api::anchors` parses once and answers what repeated `file_api::anchor` calls answer.

use quiddity::{file_api, kernel::step};
use serde_json::Value;

fn plate() -> quiddity::Part {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/filleted_plate_with_holes.step");
    step::read_step_file(&path).unwrap()
}

#[test]
fn batch_equals_repeated_single_calls_on_every_face() {
    let part = plate();
    let n = part.faces.len();
    assert!(n > 10, "a multi-face fixture");
    let rows: Vec<Value> = serde_json::from_str(&file_api::anchors(&part, None).unwrap()).unwrap();
    assert_eq!(rows.len(), n);
    for (face, row) in rows.iter().enumerate() {
        match file_api::anchor(&part, face) {
            Ok(single) => assert_eq!(*row, serde_json::from_str::<Value>(&single).unwrap()),
            Err(why) => {
                assert_eq!(row["refused"], why);
                assert!(row["point"].is_null() && row["uv"].is_null());
            }
        }
    }
}

#[test]
fn listed_faces_keep_order_and_repeats() {
    let part = plate();
    let want = [5, 0, 5];
    let rows: Vec<Value> =
        serde_json::from_str(&file_api::anchors(&part, Some(&want)).unwrap()).unwrap();
    let faces: Vec<u64> = rows.iter().map(|r| r["face"].as_u64().unwrap()).collect();
    assert_eq!(faces, [5, 0, 5]);
    assert_eq!(rows[0], rows[2]);
    assert_eq!(
        rows[1],
        serde_json::from_str::<Value>(&file_api::anchor(&part, 0).unwrap()).unwrap()
    );
}

#[test]
fn out_of_range_refuses_the_whole_call() {
    let part = plate();
    let err = file_api::anchors(&part, Some(&[0, part.faces.len()])).unwrap_err();
    assert_eq!(err, file_api::anchor(&part, part.faces.len()).unwrap_err());
    assert_eq!(file_api::anchors(&part, Some(&[])).unwrap().trim(), "[]");
}
