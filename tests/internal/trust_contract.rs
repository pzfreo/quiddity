//! Additive source identity and completed-run provenance, without changing feature payloads.
use super::common;

use quiddity::correspondence;
use quiddity::kernel::step::{IDENTITY, read_step_file};
use quiddity::recognition::{self, RunMode};
use quiddity::serve;
use sha2::{Digest, Sha256};

#[test]
fn source_digest_is_of_the_exact_input_bytes_in_cli_and_serve() {
    use std::io::Write;
    use std::process::Command;
    let path = common::fixtures().join("rejected_cylinder.step");
    let bytes = std::fs::read(&path).unwrap();
    let expected = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let (output, _) = serve::recognise_file(path.to_str().unwrap()).unwrap();
    assert_eq!(
        output.document.source_sha256.as_deref(),
        Some(expected.as_str())
    );
    assert_eq!(output.document.run_mode, RunMode::Strict);
    let cli = Command::new(common::quiddity_binary())
        .arg(&path)
        .output()
        .unwrap();
    assert!(cli.status.success());
    let cli: serde_json::Value = serde_json::from_slice(&cli.stdout).unwrap();
    let response =
        serve::respond(&serde_json::json!({"id":1,"op":"recognise","step":path}).to_string())
            .unwrap();
    let response: serde_json::Value = serde_json::from_str(&response).unwrap();
    assert_eq!(cli, response["result"]);
    assert_eq!(cli["document"]["source_sha256"], expected);

    let gz_path =
        std::env::temp_dir().join(format!("quiddity-trust-{}.step.gz", std::process::id()));
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&bytes).unwrap();
    let compressed = encoder.finish().unwrap();
    std::fs::write(&gz_path, &compressed).unwrap();
    let (gz, _) = serve::recognise_file(gz_path.to_str().unwrap()).unwrap();
    std::fs::remove_file(gz_path).unwrap();
    assert_eq!(
        gz.document.source_sha256,
        Some(
            Sha256::digest(&compressed)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        )
    );
    assert_ne!(gz.document.source_sha256, output.document.source_sha256);
    assert_eq!(gz.document.records, output.document.records);
}

#[test]
fn in_memory_geometry_has_no_invented_source_digest() {
    let part = read_step_file(&common::fixtures().join("rejected_cylinder.step")).unwrap();
    let rec = correspondence::recognise(&part);
    let doc = recognition::document(&rec, part.faces.len());
    assert_eq!(doc.source_sha256, None);
    assert_eq!(doc.run_mode, RunMode::Strict);
    let mut old = serde_json::to_value(doc).unwrap();
    let old = old.as_object_mut().unwrap();
    old.remove("source_sha256");
    old.remove("run_mode");
    let old: recognition::RecognitionDocument =
        serde_json::from_value(serde_json::json!(old)).unwrap();
    assert_eq!(old.source_sha256, None);
    assert_eq!(old.run_mode, RunMode::Unknown);
}

#[test]
fn retry_mode_survives_frame_mapping_and_thread_local_restoration() {
    let Some(dir) = common::corpus_dir() else {
        assert!(
            std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
            "corpus required"
        );
        return;
    };
    for name in ["13975", "14052"] {
        let path = dir.join(format!("inventory_refusal/{name}.step.gz"));
        let part = read_step_file(&path).unwrap();
        let rec = correspondence::recognise(&part);
        assert_eq!(
            recognition::document(&rec, part.faces.len()).run_mode,
            RunMode::LocallyDegraded,
            "{name}"
        );
        let (part, rec, _) = serve::recognise_step(path.to_str().unwrap(), &IDENTITY).unwrap();
        assert_eq!(
            recognition::document(&rec, part.faces.len()).run_mode,
            RunMode::LocallyDegraded,
            "{name}"
        );
        assert!(!quiddity::features::evidence::local_degradation());
    }
    let (sound, _) = serve::recognise_file(
        common::fixtures()
            .join("rejected_cylinder.step")
            .to_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(sound.document.run_mode, RunMode::Strict);
}

#[test]
fn broken_geometry_records_remain_visible_with_truthful_cli_and_serve_warnings() {
    use std::process::Command;
    for (name, family, count) in [
        ("broken/unresolved_surface.step", "bosses", 1),
        ("open_shell.step", "fillets", 2),
    ] {
        let path = common::fixtures().join(name);
        let cli = Command::new(common::quiddity_binary())
            .arg(&path)
            .output()
            .unwrap();
        assert!(cli.status.success(), "{name}");
        let json: serde_json::Value = serde_json::from_slice(&cli.stdout).unwrap();
        let records = json["document"]["records"].as_array().unwrap();
        let found: Vec<_> = records.iter().filter(|r| r["family"] == family).collect();
        assert_eq!(found.len(), count, "{name}");
        assert_eq!(json[family].as_array().unwrap().len(), count, "{name}");
        let stderr = String::from_utf8(cli.stderr).unwrap();
        assert!(!stderr.contains("are not recognised"), "{stderr}");
        assert!(
            stderr.contains(&format!("retained {} records", records.len())),
            "{stderr}"
        );
        for r in records {
            assert_eq!(r["evidence_status"], "unproved", "{name}: {r}");
            assert!(
                stderr.contains(r["id"].as_str().unwrap()),
                "{name}: {stderr}"
            );
        }
        let response =
            serve::respond(&serde_json::json!({"id":1,"op":"recognise","step":path}).to_string())
                .unwrap();
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["result"], json);
        let warnings: Vec<_> = response["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(
            warnings
                .iter()
                .map(|w| format!("quiddity: {w}\n"))
                .collect::<String>(),
            stderr
        );
        assert!(warnings[0].contains(if family == "bosses" {
            "geometry did not resolve (faces [1], edges [])"
        } else {
            "no solid authority"
        }));
    }
}

#[test]
fn sound_features_and_patterns_have_verified_strict_evidence() {
    use quiddity::recognition::EvidenceStatus;
    for name in [
        "filleted_plate_with_holes.step",
        "golden_gusset_rib_patterns.step",
        "golden_straight_and_obround_slots.step",
    ] {
        let (out, warnings) =
            serve::recognise_file(common::fixtures().join(name).to_str().unwrap()).unwrap();
        assert!(warnings.is_empty(), "{name}: {warnings:?}");
        assert!(!out.document.records.is_empty());
        assert!(
            out.document
                .records
                .iter()
                .all(|r| r.evidence_status == EvidenceStatus::Verified),
            "{name}"
        );
    }
}

#[test]
fn patterns_inherit_unproved_member_context_and_legacy_records_default_to_unproved() {
    use quiddity::recognition::EvidenceStatus;
    let path = common::fixtures().join("golden_bolt_circle_and_rectangular_grid.step");
    let part = read_step_file(&path).unwrap();
    let mut rec = correspondence::recognise(&part);
    let before = recognition::document(&rec, part.faces.len());
    assert!(before.records.iter().any(|r| r.family == "hole_patterns"));
    rec.features
        .evidence_status
        .get_mut("holes")
        .unwrap()
        .fill(EvidenceStatus::Unproved);
    let after = recognition::document(&rec, part.faces.len());
    assert_eq!(before.records.len(), after.records.len());
    for (before, after) in before.records.iter().zip(&after.records) {
        assert_eq!(before.parameters, after.parameters);
        if matches!(after.family.as_str(), "holes" | "hole_patterns") {
            assert_eq!(after.evidence_status, EvidenceStatus::Unproved);
        }
    }
    let mut legacy = serde_json::to_value(before).unwrap();
    for record in legacy["records"].as_array_mut().unwrap() {
        record.as_object_mut().unwrap().remove("evidence_status");
    }
    let legacy: recognition::RecognitionDocument = serde_json::from_value(legacy).unwrap();
    assert!(
        legacy
            .records
            .iter()
            .all(|r| r.evidence_status == EvidenceStatus::Unproved)
    );
}

#[test]
fn source_refusals_never_become_verified_features() {
    use quiddity::recognition::EvidenceStatus;
    for (name, kind) in [
        (
            "modelled_threads/varying_pitch_sweep.step.gz",
            "ModelledThreadRefusal",
        ),
        (
            "captured/section_recesses/parts/1354b0665897eeba.step.gz",
            "SectionRecessRefusal",
        ),
    ] {
        let part = read_step_file(&common::fixtures().join(name)).unwrap();
        let rec = correspondence::recognise(&part);
        let doc = recognition::document(&rec, part.faces.len());
        let refusals: Vec<_> = doc
            .records
            .iter()
            .filter(|r| r.record_type == kind)
            .collect();
        assert!(!refusals.is_empty(), "{name}");
        assert!(
            refusals
                .iter()
                .all(|r| r.evidence_status == EvidenceStatus::Unproved),
            "{name}"
        );
    }
}

#[test]
fn clean_source_refusals_remain_published_without_a_feature_evidence_warning() {
    use quiddity::recognition::EvidenceStatus;
    use std::process::Command;
    for (name, kind) in [
        (
            "captured/section_recesses/parts/1354b0665897eeba.step.gz",
            "SectionRecessRefusal",
        ),
        (
            "modelled_threads/varying_pitch_sweep.step.gz",
            "ModelledThreadRefusal",
        ),
    ] {
        let path = common::fixtures().join(name);
        let part = read_step_file(&path).unwrap();
        assert!(!part.solids.is_empty());
        assert!((0..part.solids.len()).all(|s| part.solid_is_geometrically_valid(s)));
        let (output, warnings) = serve::recognise_file(path.to_str().unwrap()).unwrap();
        assert_eq!(output.document.run_mode, RunMode::Strict);
        assert_eq!(output.document.records.len(), 8, "{name}");
        let unproved: Vec<_> = output
            .document
            .records
            .iter()
            .filter(|r| r.evidence_status == EvidenceStatus::Unproved)
            .collect();
        assert_eq!(unproved.len(), 1, "{name}");
        assert_eq!(unproved[0].record_type, kind);
        assert!(
            output
                .document
                .records
                .iter()
                .all(|r| r.is_refusal() == (r.record_type == kind))
        );
        assert!(warnings.is_empty(), "{name}: {warnings:?}");

        let cli = Command::new(common::quiddity_binary())
            .arg(&path)
            .output()
            .unwrap();
        assert!(cli.status.success());
        assert!(
            cli.stderr.is_empty(),
            "{name}: {}",
            String::from_utf8_lossy(&cli.stderr)
        );
        let cli: serde_json::Value = serde_json::from_slice(&cli.stdout).unwrap();
        assert_eq!(cli, serde_json::to_value(&output).unwrap());
        let response =
            serve::respond(&serde_json::json!({"id":1,"op":"recognise","step":path}).to_string())
                .unwrap();
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["result"], cli);
        assert!(response.get("warnings").is_none(), "{name}: {response}");
    }
}

#[test]
fn inverted_shell_repair_is_reported_in_documents_cli_and_serve() {
    use std::process::Command;
    let path = common::fixtures().join("truck_master_shaft_only.step.gz");
    let message = "solid 0: inverted closed shell repaired";
    let part = read_step_file(&path).unwrap();
    let recognition = correspondence::recognise(&part);
    let doc = recognition::document(&recognition, part.faces.len());
    assert_eq!(doc.input_repairs, [message]);
    let (output, warnings) = serve::recognise_file(path.to_str().unwrap()).unwrap();
    assert_eq!(output.document.input_repairs, [message]);
    assert!(warnings.iter().any(|w| w.contains(message)));
    let cli = Command::new(common::quiddity_binary())
        .arg(&path)
        .output()
        .unwrap();
    assert!(cli.status.success());
    assert!(String::from_utf8(cli.stderr).unwrap().contains(message));
    let cli: serde_json::Value = serde_json::from_slice(&cli.stdout).unwrap();
    let response =
        serve::respond(&serde_json::json!({"id":53,"op":"recognise","step":path}).to_string())
            .unwrap();
    let response: serde_json::Value = serde_json::from_str(&response).unwrap();
    assert_eq!(response["result"], cli);
    assert_eq!(
        cli["document"]["input_repairs"],
        serde_json::json!([message])
    );
    assert_eq!(
        cli["document"]["schema_version"],
        output.document.schema_version
    );
    assert!(
        response["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains(message))
    );

    let mut old = serde_json::to_value(output.document).unwrap();
    old.as_object_mut().unwrap().remove("input_repairs");
    let old: recognition::RecognitionDocument = serde_json::from_value(old).unwrap();
    assert!(old.input_repairs.is_empty());
    assert!(
        serde_json::to_value(old)
            .unwrap()
            .get("input_repairs")
            .is_none()
    );
}

/// The mixed shell's corrected material side changes the outer Ø30×40
/// cylinder from an erroneous flat-bottomed hole to an external boss.
#[test]
fn mixed_shaft_publishes_outer_boss_instead_of_hole() {
    let part = read_step_file(&common::fixtures().join("truck_master_shaft_hole.step.gz")).unwrap();
    let recognised = correspondence::recognise(&part);
    let bosses: Vec<_> = recognised
        .features
        .bosses
        .iter()
        .filter(|b| (b.diameter - 30.0).abs() < 1e-6 && (b.height - 40.0).abs() < 1e-6)
        .collect();
    assert_eq!(bosses.len(), 1, "{:?}", recognised.features.bosses);
    assert!(
        !recognised
            .features
            .holes
            .iter()
            .any(|h| (h.diameter - 30.0).abs() < 1e-6 && (h.depth - 40.0).abs() < 1e-6)
    );
    assert_eq!(
        recognition::document(&recognised, part.faces.len()).input_repairs,
        ["solid 0: mixed face senses repaired (20 faces)"]
    );
    assert_eq!(
        quiddity::frames::infer_part_frame(&part)
            .unwrap_err()
            .as_str(),
        "no-analytic-direction"
    );
}
