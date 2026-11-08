//! Rust publication snapshots on 30 quality parts and pattern inputs, plus unchanged inferred-frame, additivity and geometry checks.

use super::common;

use quiddity::correspondence;
use quiddity::kernel::step::IDENTITY;
use quiddity::recognition::{self, RecognitionDocument};
use quiddity::serve;
use serde_json::{Value, json};

struct Run {
    baseline: Value,
    caller_unpublished: RecognitionDocument,
    framed_unpublished: RecognitionDocument,
    caller: RecognitionDocument,
    framed: RecognitionDocument,
    framed_recesses: Value,
}

fn runs() -> Vec<Run> {
    let Some(corpus) = common::corpus_dir() else {
        assert!(
            std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
            "QUIDDITY_CORPUS_REQUIRED is set but the corpus was not found"
        );
        return Vec::new();
    };
    let baseline = common::load("section_recess_publication_baseline.json");
    let rows = baseline["runs"].as_array().unwrap();
    assert_eq!(rows.len(), 30);
    common::parallel::map(rows, |baseline| {
        let file = baseline["file"].as_str().unwrap();
        let path = corpus.join(file);
        let part = quiddity::read_step_file(&path).unwrap();
        let caller = correspondence::caller_space_recognise(&part);
        assert!(caller.features.section_recess_refusal.is_none(), "{file}");
        let caller_doc = recognition::document(&caller, part.faces.len());
        let (_, framed, _) = serve::recognise_step(path.to_str().unwrap(), &IDENTITY).unwrap();
        Run {
            baseline: baseline.clone(),
            caller_unpublished: without_section_recess_publication(&caller, part.faces.len()),
            framed_unpublished: without_section_recess_publication(&framed, part.faces.len()),
            caller: caller_doc,
            framed: recognition::document(&framed, part.faces.len()),
            framed_recesses: json!(
                recognition::families(&framed.features)
                    .into_iter()
                    .find(|(f, _)| f == "section_recesses")
                    .unwrap()
                    .1
            ),
        }
    })
}

fn recesses(doc: &RecognitionDocument) -> Vec<Value> {
    doc.records
        .iter()
        .filter(|r| r.family == "section_recesses")
        .enumerate()
        .map(|(index, r)| {
            assert_eq!(r.id, format!("section_recesses/{index}"));
            assert_eq!(
                r.record_type,
                if r.parameters.contains_key("reason") {
                    "SectionRecessRefusal"
                } else {
                    "SectionRecess"
                }
            );
            assert_eq!(json!(r.faces), r.parameters["evidence"]["defining_faces"]);
            Value::Object(r.parameters.clone())
        })
        .collect()
}

#[test]
fn rust_snapshot_quality_section_recesses() {
    let runs = runs();
    if runs.is_empty() {
        return;
    }
    let actual: Vec<Value> = runs
        .iter()
        .map(|run| json!({"file":run.baseline["file"],"records":recesses(&run.caller)}))
        .collect();
    common::snapshot::check(
        &common::fixtures(),
        "quality_recess_publication",
        &json!(actual),
    );
    inferred_frame_section_recesses_are_published(&runs);
    section_recess_publication_is_additive(&runs);
}

fn inferred_frame_section_recesses_are_published(runs: &[Run]) {
    let mut published = 0;
    for run in runs {
        let file = run.baseline["file"].as_str().unwrap();
        let got = json!(recesses(&run.framed));
        assert_eq!(
            got, run.framed_recesses,
            "{file}: framed records lost or changed"
        );
        published += got.as_array().unwrap().len();
        let bytes = serde_json::to_vec(&run.framed).unwrap();
        let back: RecognitionDocument = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back, run.framed, "{file}: document round trip");
    }
    assert!(published > 0, "no framed section recesses published");
}

/// Build through the normal document builder with only section-recess publication disabled.
/// Recognition, numeric measurements, fingerprints and frame mapping are the same run's.
fn without_section_recess_publication(
    recognition: &correspondence::Recognition,
    face_count: usize,
) -> RecognitionDocument {
    let mut unpublished = recognition.clone();
    unpublished.features.section_recesses.clear();
    unpublished.features.section_recess_refusals.clear();
    unpublished.features.section_recess_patterns.clear();
    recognition::document(&unpublished, face_count)
}

/// Project both documents onto the pre-publication contract. Later additive trust fields
/// and void diagnostics have their own payload checks; no measured field is normalised.
/// Keep record order, IDs, face claims, parameters, local paths, frame and package version.
fn existing_document_bytes(document: &RecognitionDocument) -> Vec<u8> {
    let mut value = serde_json::to_value(document).unwrap();
    let object = value.as_object_mut().unwrap();
    // Accounting depends on the published records, so omit it in this legacy projection.
    object.remove("face_accounting");
    object.remove("source_sha256");
    object.remove("run_mode");
    let records = object["records"].as_array_mut().unwrap();
    records.retain(|r| {
        !matches!(
            r["family"].as_str().unwrap(),
            "section_recesses" | "section_recess_patterns"
        ) && r["record_type"] != "InteriorVoidRefusal"
    });
    for record in records {
        record.as_object_mut().unwrap().remove("evidence_status");
    }
    serde_json::to_vec(&value).unwrap()
}

fn section_recess_publication_is_additive(runs: &[Run]) {
    for run in runs {
        let file = run.baseline["file"].as_str().unwrap();
        for (name, published, unpublished) in [
            ("caller", &run.caller, &run.caller_unpublished),
            ("framed", &run.framed, &run.framed_unpublished),
        ] {
            assert!(
                unpublished.records.iter().all(|r| !matches!(
                    r.family.as_str(),
                    "section_recesses" | "section_recess_patterns"
                )),
                "{file}: {name} unpublished path emitted section recesses"
            );
            assert_eq!(
                existing_document_bytes(published),
                existing_document_bytes(unpublished),
                "{file}: {name} section-recess publication changed existing document bytes"
            );
        }
    }
}

/// The payload actually published, including patterns that used to be discarded. The Python
/// capture includes rectangular, obround and rounded profiles, arrays and grids.
#[test]
fn rust_snapshot_pattern_publication() {
    let input: Value = serde_json::from_reader(flate2::read::GzDecoder::new(
        std::fs::File::open(common::fixtures().join("captured/section_recesses/documents.json.gz"))
            .unwrap(),
    ))
    .unwrap();
    let mut checked = 0;
    for run in input["runs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["source"] == "test" && r["pattern_input"] == true)
    {
        let file = run["file"].as_str().unwrap();
        let path = common::fixtures()
            .join("captured/section_recesses")
            .join(file);
        let part = quiddity::read_step_file(&path).unwrap();
        let rec = correspondence::caller_space_recognise(&part);
        let doc = recognition::document(&rec, part.faces.len());
        assert_eq!(doc.schema_version, "quiddity-rust/recognition/6");
        let patterns: Vec<Value> = doc
            .records
            .iter()
            .filter(|r| r.family == "section_recess_patterns")
            .map(|r| Value::Object(r.parameters.clone()))
            .collect();
        common::snapshot::check(
            &common::fixtures(),
            &format!("pattern_publication/{file}"),
            &json!(patterns),
        );
        for p in doc
            .records
            .iter()
            .filter(|r| r.family == "section_recess_patterns")
        {
            let mut faces: Vec<usize> = Vec::new();
            for m in p.parameters["members"].as_array().unwrap() {
                let member = doc
                    .records
                    .iter()
                    .find(|r| {
                        r.family == "section_recesses"
                            && r.record_type == "SectionRecess"
                            && r.parameters["index"] == *m
                    })
                    .unwrap();
                faces.extend(&member.faces);
            }
            faces.sort_unstable();
            faces.dedup();
            assert_eq!(p.faces, faces);
        }
        checked += 1;
    }
    assert!(checked >= 2);
}

#[test]
fn k3_degraded_records_keep_their_geometry_in_the_published_document() {
    let Some(corpus) = common::corpus_dir() else {
        return;
    };
    for (name, count) in [("13975", 0), ("14052", 1)] {
        let part =
            quiddity::read_step_file(&corpus.join(format!("inventory_refusal/{name}.step.gz")))
                .unwrap();
        let rec = correspondence::caller_space_recognise(&part);
        assert!(rec.features.section_recess_refusal.is_none());
        let doc = recognition::document(&rec, part.faces.len());
        let records: Vec<_> = doc
            .records
            .iter()
            .filter(|r| r.family == "section_recesses" && r.record_type == "SectionRecess")
            .collect();
        assert_eq!(records.len(), count, "{name}");
        if count == 1 {
            assert_eq!(
                records[0].parameters["geometry"]["profile"]["boundary"]
                    .as_array()
                    .unwrap()
                    .len(),
                3
            );
        }
    }
}

#[test]
fn exact_recess_sources_survive_publication_and_motion() {
    let Some(corpus) = common::corpus_dir() else {
        return;
    };
    let base = quiddity::read_step_file(&corpus.join("mfcadpp/1000.step")).unwrap();
    let (s, c) = 0.37_f64.sin_cos();
    for placement in [
        IDENTITY,
        [[c, 0., s, 123.456], [0., 1., 0., -78.9], [-s, 0., c, 41.3]],
    ] {
        let part = base.transformed(&placement).unwrap();
        let rec = correspondence::caller_space_recognise(&part);
        let doc = recognition::document(&rec, part.faces.len());
        let duplicate = doc
            .records
            .iter()
            .find(|r| r.id == "section_recesses/5")
            .unwrap();
        assert_eq!(duplicate.parameters["source"], "section_passages/0");
        let source = doc
            .records
            .iter()
            .find(|r| r.id == "section_passages/0")
            .unwrap();
        assert_eq!(duplicate.faces, source.faces);
        // A partial overlap is retained, without pretending to be an identical source.
        let partial = doc
            .records
            .iter()
            .find(|r| r.id == "section_recesses/0")
            .unwrap();
        assert!(!partial.parameters.contains_key("source"));
        assert!(
            doc.records
                .iter()
                .filter(|r| r.family != "section_recesses")
                .all(|r| r.faces != partial.faces)
        );
    }
}

#[test]
fn recess_only_record_has_no_source_reference() {
    let Some(corpus) = common::corpus_dir() else {
        return;
    };
    let part = quiddity::read_step_file(&corpus.join("inventory_refusal/14052.step.gz")).unwrap();
    let rec = correspondence::caller_space_recognise(&part);
    assert_eq!(rec.features.section_recesses.len(), 1);
    assert!(rec.features.section_recesses[0].source.is_none());
    let doc = recognition::document(&rec, part.faces.len());
    let recess = doc
        .records
        .iter()
        .find(|r| r.id == "section_recesses/0")
        .unwrap();
    assert!(!recess.parameters.contains_key("source"));
    assert!(
        doc.records
            .iter()
            .filter(|r| r.family != "section_recesses")
            .all(|r| r.faces != recess.faces)
    );
}
