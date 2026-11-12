//! Shared analysis must reproduce both existing contracts, including refusals.
use super::common;
use quiddity::{
    correspondence, evidence_view, file_api, frames::RecordFrame, kernel::step, recognition,
};
use serde_json::{Value, json};

fn compare(part: &quiddity::Part) {
    let separate = correspondence::recognise(part);
    let framed = matches!(separate.frame, RecordFrame::Inferred(_));
    let separate_evidence = evidence_view::document(part, framed)
        .map(|doc| serde_json::to_value(doc).unwrap())
        .map_err(|error| error.to_string());
    let (shared, shared_evidence) = correspondence::recognise_with_evidence(part);
    assert_eq!(
        serde_json::to_value(&shared).unwrap(),
        serde_json::to_value(&separate).unwrap()
    );
    assert_eq!(
        serde_json::to_value(recognition::document(&shared, part.faces.len())).unwrap(),
        serde_json::to_value(recognition::document(&separate, part.faces.len())).unwrap(),
    );
    assert_eq!(
        shared_evidence
            .map(|doc| serde_json::to_value(doc).unwrap())
            .map_err(|error| error.to_string()),
        separate_evidence
    );
}

#[test]
fn shared_analysis_matches_complete_documents() {
    for file in [
        "golden_simple_through_hole.step",
        "golden_bolt_circle_and_rectangular_grid.step",
        "golden_oriented_slots.step",
        "golden_blind_pockets_and_pocket_patterns.step",
        "golden_gusset_rib_patterns.step",
        "golden_oriented_chamfer.step",
        "pocket_floor_fillet.step",
        "pocket_partial_floor_fillet.step",
        "pocket_floor_island.step.gz",
        "open_shell.step",
        "broken/unresolved_surface.step",
        "unowned_recess_solid.step.gz",
        "unowned_recess_shell.step.gz",
        "unowned_recess_mixed.step.gz",
    ] {
        eprintln!("shared analysis: {file}");
        let part = step::read_step_file(&common::fixtures().join(file)).unwrap();
        compare(&part);
    }
}

#[test]
fn shared_analysis_preserves_moved_geometry_and_face_sources() {
    let part = step::read_step_file_placed(
        &common::fixtures().join("golden_simple_through_hole.step"),
        &[
            [0.8, -0.6, 0.0, 101.0],
            [0.6, 0.8, 0.0, -37.0],
            [0.0, 0.0, 1.0, 11.0],
        ],
    )
    .unwrap();
    compare(&part);
}

#[test]
fn shared_analysis_keeps_digest_and_bytes_file_equivalence() {
    let path = common::fixtures().join("golden_simple_through_hole.step");
    let bytes = std::fs::read(&path).unwrap();
    let shared: Value =
        serde_json::from_str(&file_api::recognise_with_evidence_bytes(bytes.clone()).unwrap())
            .unwrap();
    let from_file: Value = serde_json::from_str(
        &file_api::recognise_with_evidence_file(path.to_str().unwrap()).unwrap(),
    )
    .unwrap();
    let original: Value = serde_json::from_str(&file_api::recognise_bytes(bytes).unwrap()).unwrap();
    assert_eq!(shared, from_file);
    assert_eq!(shared["recognition"], original);
    assert_eq!(shared["schema_version"], "quiddity-rust/analysis/1");
    assert_eq!(shared["evidence_error"], Value::Null);
    assert_eq!(
        shared["evidence"]["schema_version"],
        "quiddity-rust/evidence/1"
    );
    assert!(
        !shared["evidence"]["features"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(file_api::recognise_with_evidence_bytes(b"not a STEP file".to_vec()).is_err());
}

#[test]
fn shared_analysis_refused_frame_stays_in_file_coordinates() {
    let part =
        step::read_step_file(&common::fixtures().join("captured/frames/sphere.step")).unwrap();
    let (rec, evidence) = correspondence::recognise_with_evidence(&part);
    assert!(matches!(rec.frame, RecordFrame::Refused { .. }));
    let evidence = serde_json::to_value(evidence.unwrap()).unwrap();
    assert_eq!(evidence["coordinate_space"], json!("file"));
    assert_eq!(evidence["frame"], Value::Null);
    compare(&part);
}

#[test]
fn shared_analysis_retains_local_degradation_retry() {
    let Some(corpus) = common::corpus_dir() else {
        assert!(
            std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
            "QUIDDITY_CORPUS_REQUIRED is set but the corpus was not found"
        );
        return;
    };
    // Reuse the pinned corpus inputs without duplicating or changing snapshot inputs.
    for name in ["13975.step.gz", "14052.step.gz"] {
        let part = step::read_step_file(&corpus.join("inventory_refusal").join(name)).unwrap();
        let (rec, _) = correspondence::recognise_with_evidence(&part);
        assert_eq!(
            recognition::document(&rec, part.faces.len()).run_mode,
            recognition::RunMode::LocallyDegraded,
            "{name}",
        );
        compare(&part);
        assert!(!quiddity::features::evidence::local_degradation());
    }
}
