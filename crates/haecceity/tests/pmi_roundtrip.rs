//! The AP242 PMI writer's round trips: read → replace (every part with what was read) →
//! read gives the same PMI, for every NIST AP242 file and every specify-core input.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use haecceity::p21::Document;
use haecceity::pmi::write::{
    ItemRef, Mode, PresentationPolicy, WriteError, differences_as_stated as differences,
};
use haecceity::pmi::{self, *};
use haecceity::step::{PartDefinition, read_part_definitions};

fn file_bytes(path: &Path) -> Vec<u8> {
    let raw = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    if path.extension().is_some_and(|e| e == "gz") {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(raw.as_slice())
            .read_to_end(&mut out)
            .unwrap();
        out
    } else {
        raw
    }
}

/// `(kind of item, reason)` of a refusal, without the item's index: the pin key.
fn refusal_key(item: ItemRef, why: &str) -> String {
    let kind = format!("{item:?}");
    let kind = kind.split('(').next().unwrap_or("").to_string();
    // Numbers in reasons (counts, face numbers) and datum labels vary per item: keep the
    // words.
    let why = match why.strip_prefix("datum ").and_then(|r| r.split_once(' ')) {
        Some((_, rest)) if kind == "Datum" => format!("datum … {rest}"),
        _ => why.to_string(),
    };
    let why: String = why
        .chars()
        .map(|c| if c.is_ascii_digit() { 'N' } else { c })
        .collect();
    format!("{kind}: {why}")
}

struct Outcome {
    /// Refused items by key, with counts.
    refused: BTreeMap<String, usize>,
    /// The PMI written per part (the read PMI without what was refused).
    written: Vec<PartPmi>,
    result: Result<(Vec<u8>, pmi::write::WriteReport), WriteError>,
}

/// Replace every part with its read PMI; refused items are removed (with what depends on
/// them) and the write repeated, until it is written or fails otherwise.
fn replace_all(
    doc: &Document,
    parts: &[PartDefinition],
    read: &PmiRead,
    policy: PresentationPolicy,
) -> Outcome {
    let mut written: Vec<PartPmi> = read.parts.clone();
    let mut refused = BTreeMap::new();
    for _ in 0..8 {
        let pmi: Vec<(PartId, PartPmi)> = written
            .iter()
            .enumerate()
            .map(|(i, p)| (PartId(i), p.clone()))
            .collect();
        match pmi::write(doc, parts, &pmi, Mode::Replace, policy) {
            Err(WriteError::Refused(list)) => {
                for r in &list {
                    *refused.entry(refusal_key(r.item, &r.why)).or_insert(0) += 1;
                }
                for (i, p) in written.iter_mut().enumerate() {
                    let items: Vec<ItemRef> = list
                        .iter()
                        .filter(|r| r.part == PartId(i))
                        .map(|r| r.item)
                        .collect();
                    if !items.is_empty() {
                        *p = pmi::without(p, &items);
                    }
                }
            }
            Err(e) => {
                return Outcome {
                    refused,
                    written,
                    result: Err(e),
                };
            }
            Ok((edit, report)) => {
                let bytes = doc.apply(&edit).expect("the edit applies").bytes;
                return Outcome {
                    refused,
                    written,
                    result: Ok((bytes, report)),
                };
            }
        }
    }
    panic!("refusals did not settle");
}

/// NIST's MBE PMI test models: the `.stp` files of the directory `HAECCEITY_NIST_PMI` names
/// (NIST's `NIST-PMI-STEP-Files`); none when it is not set, unless
/// `HAECCEITY_NIST_PMI_REQUIRED` is.
fn nist_files() -> Vec<PathBuf> {
    let Some(dir) = std::env::var_os("HAECCEITY_NIST_PMI").map(PathBuf::from) else {
        assert!(
            std::env::var_os("HAECCEITY_NIST_PMI_REQUIRED").is_none(),
            "HAECCEITY_NIST_PMI_REQUIRED is set but HAECCEITY_NIST_PMI is not"
        );
        eprintln!("HAECCEITY_NIST_PMI not set; NIST files skipped");
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "stp"))
        .collect();
    out.sort();
    out
}

fn specify_files() -> Vec<PathBuf> {
    let dir = common::fixtures().join("ap242/specify");
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".step.gz"))
        .collect();
    out.sort();
    out.push(common::fixtures().join("ap242/assembly/assembly.step"));
    // The NIST models committed with the reader's fixtures (one of them, STC-09, in edition
    // 4, the target), so the round trip runs without the download too.
    let nist = common::fixtures().join("ap242/nist");
    let mut committed: Vec<PathBuf> = std::fs::read_dir(&nist)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".stp.gz"))
        .collect();
    committed.sort();
    out.extend(committed);
    out
}

fn stem(p: &Path) -> String {
    let n = p.file_name().unwrap().to_string_lossy().to_string();
    n.trim_end_matches(".gz")
        .trim_end_matches(".step")
        .trim_end_matches(".stp")
        .to_string()
}

/// The pin key of a write that failed other than by refusal.
fn error_key(e: &WriteError) -> String {
    match e {
        WriteError::Removal(r) => {
            let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
            for b in &r.blockers {
                *kinds
                    .entry(format!("{} {}", b.family.as_str(), b.entity))
                    .or_insert(0) += 1;
            }
            for e in &r.emptied {
                *kinds.entry(format!("emptied {}", e.attribute)).or_insert(0) += 1;
            }
            format!("removal refused: {kinds:?}")
        }
        WriteError::Edition { violations, .. } => {
            format!("edition: {} violations", violations.len())
        }
        e => format!("{e}"),
    }
}

fn load_pins() -> serde_json::Value {
    serde_json::from_str(
        &std::fs::read_to_string(common::fixtures().join("references/pmi_write.json")).unwrap(),
    )
    .unwrap()
}

/// Every file of `files`, read → replace (each part with what was read, refused items removed
/// with what depends on them) → read: the same PMI per part, by meaning (`differences`, which
/// compares every value's text and unit, so inch values keep their stated digits); no finding
/// about what was written; every other finding as before; no instance the reader consumed for
/// a replaced part survives (supplemental geometry is the part's and is kept). Refusals and
/// other failures are pinned per file with a reason (`references/pmi_write.json` "roundtrip").
fn roundtrip(
    files: &[PathBuf],
    problems: &mut Vec<String>,
    actual: &mut serde_json::Map<String, serde_json::Value>,
) {
    for path in files {
        let stem = stem(path);
        let bytes = file_bytes(path);
        let Ok(parts) = read_part_definitions(&bytes) else {
            actual.insert(stem, serde_json::json!({"error": "no part definitions"}));
            continue;
        };
        let doc = Document::parse(bytes).unwrap();
        let mut entry = serde_json::Map::new();
        entry.insert("sha256".into(), common::sha256_hex(doc.bytes()).into());
        let r0 = pmi::read(&doc, &parts).unwrap();
        let o = replace_all(&doc, &parts, &r0, PresentationPolicy::RemovePresentation);
        if !o.refused.is_empty() {
            entry.insert("refused".into(), serde_json::to_value(&o.refused).unwrap());
        }
        // What the round trip leaves out, refused items and everything depending on them.
        let left_out = left_out(&r0.parts, &o.written);
        if !left_out.is_empty() {
            entry.insert("left out".into(), serde_json::to_value(&left_out).unwrap());
        }
        match &o.result {
            Err(e) => {
                entry.insert("error".into(), error_key(e).into());
            }
            Ok((bytes, report)) => {
                let parts1 = read_part_definitions(bytes).unwrap();
                let doc1 = Document::parse(bytes.clone()).unwrap();
                let r1 = pmi::read(&doc1, &parts1).unwrap();
                // Every datum with a face before writing must still have one afterwards.
                // In particular, a fully resolved input must have no unresolved output.
                // Existing target-only datums (e.g. CTC-02 A/B/C/K) are not feature faces.
                let before = common::pmi_datums::unresolved(&r0);
                let after = common::pmi_datums::unresolved(&r1);
                assert!(
                    after.is_subset(&before),
                    "{stem}: newly unresolved datum features: {:?}",
                    after.difference(&before).collect::<Vec<_>>()
                );
                for (i, p) in o.written.iter().enumerate() {
                    for d in differences(&pmi::write::standard_form(p), &r1.parts[i]) {
                        problems.push(format!("{stem} part {i}: {d}"));
                    }
                }
                let max = doc.max_id();
                for f in &r1.findings {
                    if f.ids.iter().any(|&id| id > max) {
                        problems.push(format!("{stem}: a finding on what was written: {f:?}"));
                    } else if !r0.findings.contains(f)
                        && !(f.kind == FindingKind::Unconsumed
                            && f.ids
                                .first()
                                .is_some_and(|id| report.retained_shared.contains(id)))
                    {
                        problems.push(format!("{stem}: a new finding: {f:?}"));
                    }
                }
                for (i, prov) in r0.provenance.parts.iter().enumerate() {
                    let geometry: std::collections::BTreeSet<u64> =
                        prov.geometry.iter().flatten().copied().collect();
                    for id in prov.all() {
                        if !geometry.contains(&id)
                            && !report.retained_shared.contains(&id)
                            && doc1.get(id).is_some()
                        {
                            problems.push(format!("{stem} part {i}: consumed #{id} survives"));
                        }
                    }
                }
                assert_eq!(
                    doc1.file_schema().unwrap(),
                    doc.file_schema().unwrap(),
                    "{stem}: an AP242 file keeps its schema"
                );
                if !report.retained_shared.is_empty() {
                    // This support remains used solely by unread PMI after replacement;
                    // its accounting changes from consumed to explicitly reported.
                    entry.insert(
                        "retained shared support".into(),
                        serde_json::json!(report.retained_shared),
                    );
                }
                entry.insert(
                    "presentation removed".into(),
                    report.presentation_removed.len().into(),
                );
                if let Some(file) = &report.edition_undetermined {
                    entry.insert(
                        "edition".into(),
                        format!(
                            "undetermined: {file} has no table; validated against {}",
                            report.validated_against
                        )
                        .into(),
                    );
                }
                if !report.practice_schema_conflicts.is_empty() {
                    let mut counts = BTreeMap::<&str, usize>::new();
                    for v in &report.practice_schema_conflicts {
                        *counts.entry(v.rule).or_default() += 1;
                    }
                    entry.insert(
                        "practice/schema conflicts".into(),
                        serde_json::to_value(counts).unwrap(),
                    );
                }
                // The same with policy Refuse: refused, naming the presentation, or written.
                let pmi: Vec<(PartId, PartPmi)> = o
                    .written
                    .iter()
                    .enumerate()
                    .map(|(i, p)| (PartId(i), p.clone()))
                    .collect();
                let refuse = match pmi::write(
                    &doc,
                    &parts,
                    &pmi,
                    Mode::Replace,
                    PresentationPolicy::Refuse,
                ) {
                    Ok(_) => "written".to_string(),
                    Err(WriteError::Removal(r)) => {
                        let presentation = r
                            .blockers
                            .iter()
                            .filter(|b| {
                                b.family != haecceity::express::Family::Other
                                    && b.family != haecceity::express::Family::SemanticPmi
                            })
                            .count();
                        format!(
                            "refused: {presentation} presentation blockers of {}",
                            r.blockers.len() + r.emptied.len()
                        )
                    }
                    Err(e) => error_key(&e),
                };
                entry.insert("refuse policy".into(), refuse.into());
            }
        }
        actual.insert(stem, serde_json::Value::Object(entry));
    }
}

/// Per kind, how many of the read items the written PMI lacks (all parts together).
fn left_out(read: &[PartPmi], written: &[PartPmi]) -> BTreeMap<&'static str, usize> {
    let mut out = BTreeMap::new();
    for (r, w) in read.iter().zip(written) {
        for (kind, a, b) in [
            ("standards", r.standards.len(), w.standards.len()),
            ("features", r.features.len(), w.features.len()),
            (
                "datum targets",
                r.datum_targets.len(),
                w.datum_targets.len(),
            ),
            ("datums", r.datums.len(), w.datums.len()),
            ("dimensions", r.dimensions.len(), w.dimensions.len()),
            ("tolerances", r.tolerances.len(), w.tolerances.len()),
            (
                "tolerance relations",
                r.tolerance_relations.len(),
                w.tolerance_relations.len(),
            ),
            ("general", r.general.len(), w.general.len()),
            ("threads", r.threads.len(), w.threads.len()),
            ("knurls", r.knurls.len(), w.knurls.len()),
            ("notes", r.notes.len(), w.notes.len()),
            ("attributes", r.attributes.len(), w.attributes.len()),
            (
                "material",
                usize::from(r.material.is_some()),
                usize::from(w.material.is_some()),
            ),
            (
                "decimal places",
                usize::from(r.decimal_places.is_some()),
                usize::from(w.decimal_places.is_some()),
            ),
        ] {
            if a > b {
                *out.entry(kind).or_insert(0) += a - b;
            }
        }
    }
    out
}

/// Compares `actual` with the pins of `section`: each file's entry equal to its pin (the
/// pin's sha256 is the file's, so a pin is never matched against another file of the same
/// name), every key with a reason, and every pin `must_run` names actually run (no stale
/// pins).
fn compare_pins(
    section: &str,
    actual: &serde_json::Map<String, serde_json::Value>,
    must_run: impl Fn(&str) -> bool,
    problems: &mut Vec<String>,
) {
    if let Some(dir) = std::env::var_os("HAECCEITY_PMI_WRITE_ACTUAL") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            Path::new(&dir).join(format!("{section}.json")),
            serde_json::to_vec_pretty(actual).unwrap(),
        )
        .unwrap();
    }
    let pins = load_pins();
    let reasons = pins["reasons"].as_object().unwrap();
    let pinned = pins[section].as_object().unwrap();
    for (stem, got) in actual {
        match pinned.get(stem) {
            Some(p) if p == got => {}
            Some(p) => problems.push(format!(
                "{section} {stem}: changed\n  pinned {p}\n  actual {got}"
            )),
            None => problems.push(format!("{section} {stem}: not pinned: {got}")),
        }
        let mut keys: Vec<String> = got
            .get("refused")
            .and_then(|r| r.as_object())
            .map(|r| r.keys().cloned().collect())
            .unwrap_or_default();
        if let Some(e) = got.get("error").and_then(|e| e.as_str()) {
            keys.push(e.split(':').next().unwrap_or("").to_string());
        }
        for k in keys {
            if !reasons
                .get(&k)
                .and_then(|r| r.as_str())
                .is_some_and(|r| !r.is_empty())
            {
                problems.push(format!("{section} {stem}: no reason for {k:?}"));
            }
        }
    }
    for stem in pinned.keys() {
        if must_run(stem) && !actual.contains_key(stem) {
            problems.push(format!("{section} {stem}: pinned but not run"));
        }
    }
}

/// The stems of the NIST models committed with the reader's fixtures.
fn committed_nist() -> BTreeSet<String> {
    std::fs::read_dir(common::fixtures().join("ap242/nist"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".stp.gz"))
        .map(|p| stem(&p))
        .collect()
}

#[test]
fn control_datum_features_resolve_after_write_read() {
    let bytes = file_bytes(&common::fixtures().join("ap242/reexport/01-NIST-control-ctc01.stp.gz"));
    let parts = read_part_definitions(&bytes).unwrap();
    let doc = Document::parse(bytes).unwrap();
    let read = pmi::read(&doc, &parts).unwrap();
    assert!(common::pmi_datums::unresolved(&read).is_empty());
    // All twelve dimensions, including the four with no stated value, are kept.
    let outcome = replace_all(&doc, &parts, &read, PresentationPolicy::RemovePresentation);
    assert!(outcome.refused.is_empty());
    let (bytes, _) = outcome.result.unwrap();
    let parts = read_part_definitions(&bytes).unwrap();
    let doc = Document::parse(bytes).unwrap();
    let written = pmi::read(&doc, &parts).unwrap();
    assert!(common::pmi_datums::unresolved(&written).is_empty());
    for callout in &read.callouts {
        for target in callout.links.iter().flat_map(|link| &link.targets) {
            if let PresentedItem::Dimension(d) = target.item
                && read.parts[target.part.0].dimensions[d.0].nominal.is_none()
            {
                assert!(
                    written.callouts.iter().any(|c| c.id == callout.id
                        && c.links
                            .iter()
                            .flat_map(|link| &link.targets)
                            .any(|t| t == target)),
                    "dimension without a value lost its original callout #{}",
                    callout.id
                );
            }
        }
    }
    let labels: BTreeSet<_> = written.parts[0]
        .datums
        .iter()
        .map(|d| d.label().to_string())
        .collect();
    assert_eq!(labels, BTreeSet::from(["A".into(), "B".into(), "C".into()]));
}

#[test]
fn read_replace_read_keeps_specify_inputs_and_committed_nist_files() {
    let mut problems = Vec::new();
    let mut actual = serde_json::Map::new();
    roundtrip(&specify_files(), &mut problems, &mut actual);
    let committed = committed_nist();
    compare_pins(
        "roundtrip",
        &actual,
        |stem| !stem.contains("_asme1_ap242") || committed.contains(stem),
        &mut problems,
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn read_replace_read_keeps_nist_files() {
    let mut problems = Vec::new();
    let mut actual = serde_json::Map::new();
    let files = nist_files();
    let run = !files.is_empty();
    roundtrip(&files, &mut problems, &mut actual);
    // With NIST's directory, every NIST pin is of one of its files.
    compare_pins(
        "roundtrip",
        &actual,
        |stem| run && stem.contains("_asme1_ap242"),
        &mut problems,
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn newly_written_kinds_roundtrip_into_a_clean_copy() {
    let bytes = file_bytes(&common::fixtures().join("ap242/specify/spool_fits.step.gz"));
    let parts = read_part_definitions(&bytes).unwrap();
    let doc = Document::parse(bytes).unwrap();
    let mut schema = haecceity::p21::Edit::new();
    schema.set_file_schema(vec![haecceity::express::TARGET_SCHEMA.into()]);
    let doc = Document::parse(doc.apply(&schema).unwrap().bytes).unwrap();
    let original = common::pmi_gaps::every_kind();
    let (edit, _) = pmi::write(
        &doc,
        &parts,
        &[(PartId(0), original.clone())],
        Mode::Replace,
        PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let first = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
    let read = pmi::read(&first, &parts).unwrap();
    assert!(differences(&pmi::write::standard_form(&original), &read.parts[0]).is_empty());
    let (edit, _) = pmi::write(
        &doc,
        &parts,
        &[(PartId(0), read.parts[0].clone())],
        Mode::Replace,
        PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let second = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
    let read2 = pmi::read(&second, &parts).unwrap();
    assert!(differences(&read.parts[0], &read2.parts[0]).is_empty());
}

#[test]
fn read_replace_read_keeps_corpus_nist_requirements() {
    let Some(corpus) = common::corpus_dir() else {
        return;
    };
    let mut files: Vec<_> = std::fs::read_dir(corpus.join("nist"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "stp"))
        .collect();
    files.sort();
    assert_eq!(
        files.len(),
        10,
        "update coverage explicitly if the corpus changes"
    );
    let mut problems = Vec::new();
    let mut actual = serde_json::Map::new();
    roundtrip(&files, &mut problems, &mut actual);
    compare_pins("corpus_roundtrip", &actual, |_| true, &mut problems);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn practice_targets_and_precision_roundtrip_into_a_clean_copy() {
    let bytes = file_bytes(&common::fixtures().join("ap242/specify/spool_fits.step.gz"));
    let parts = read_part_definitions(&bytes).unwrap();
    let original_doc = Document::parse(bytes).unwrap();
    let (remove, _) = pmi::write(
        &original_doc,
        &parts,
        &[(PartId(0), PartPmi::default())],
        Mode::Remove,
        PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let clean = Document::parse(original_doc.apply(&remove).unwrap().bytes).unwrap();
    for original in [
        common::pmi_targets::every_kind(),
        PartPmi {
            decimal_places: Some(3),
            ..PartPmi::default()
        },
    ] {
        let (edit, report) = pmi::write(
            &clean,
            &parts,
            &[(PartId(0), original.clone())],
            Mode::Replace,
            PresentationPolicy::RemovePresentation,
        )
        .unwrap();
        assert!(!report.practice_schema_conflicts.is_empty());
        let doc = Document::parse(clean.apply(&edit).unwrap().bytes).unwrap();
        let first = pmi::read(&doc, &parts).unwrap();
        assert!(differences(&original, &first.parts[0]).is_empty());
        let (edit, _) = pmi::write(
            &clean,
            &parts,
            &[(PartId(0), first.parts[0].clone())],
            Mode::Replace,
            PresentationPolicy::RemovePresentation,
        )
        .unwrap();
        let doc = Document::parse(clean.apply(&edit).unwrap().bytes).unwrap();
        let second = pmi::read(&doc, &parts).unwrap();
        assert!(differences(&first.parts[0], &second.parts[0]).is_empty());
    }
}

#[test]
fn thirteen_nist_target_subsets_with_datums_and_required_features_roundtrip_into_clean_copies() {
    let mut total = 0;
    for (name, count) in [
        ("nist_ctc_02_asme1_ap242-e2", 9),
        ("nist_ftc_10_asme1_ap242-e2", 4),
    ] {
        let bytes = file_bytes(&common::fixtures().join(format!("ap242/nist/{name}.stp.gz")));
        let parts = read_part_definitions(&bytes).unwrap();
        let source = Document::parse(bytes).unwrap();
        let original = pmi::read(&source, &parts).unwrap();
        let wanted = common::pmi_targets::from_read(&original.parts[0]);
        assert_eq!(wanted.datum_targets.len(), count);
        let (edit, _) = pmi::write(
            &source,
            &parts,
            &[(PartId(0), PartPmi::default())],
            Mode::Remove,
            PresentationPolicy::RemovePresentation,
        )
        .unwrap();
        let clean = Document::parse(source.apply(&edit).unwrap().bytes).unwrap();
        assert!(
            pmi::read(&clean, &parts).unwrap().parts[0]
                .datum_targets
                .is_empty()
        );
        let (edit, report) = pmi::write(
            &clean,
            &parts,
            &[(PartId(0), wanted.clone())],
            Mode::Replace,
            PresentationPolicy::RemovePresentation,
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        let written = clean.apply(&edit).unwrap();
        assert!(
            report.practice_schema_conflicts.iter().all(|v| written
                .ids
                .values()
                .any(|&id| id == v.id)
                && v.rule == "datum_target.WR5")
        );
        assert_eq!(
            report
                .practice_schema_conflicts
                .iter()
                .filter(|v| v.rule == "datum_target.WR5")
                .count(),
            count
        );
        let doc = Document::parse(written.bytes).unwrap();
        let read = pmi::read(&doc, &parts).unwrap();
        let diff = differences(&wanted, &read.parts[0]);
        assert!(diff.is_empty(), "{name}: {}", diff.join("\n"));
        assert_eq!(read.parts[0].datum_targets.len(), count);
        total += count;
    }
    assert_eq!(total, 13);
}

/// Unread #348/#414 keep their auxiliary support. After the first write that
/// support is unconsumed, and its compartments must still follow equal datums.
#[test]
fn unread_shared_datum_support_survives_repeated_replacement() {
    let mut input =
        file_bytes(&common::fixtures().join("ap242/nist/nist_ftc_06_asme1_ap242-e2.stp.gz"));
    for pass in 0..2 {
        let parts = read_part_definitions(&input).unwrap();
        let doc = Document::parse(input).unwrap();
        let read = pmi::read(&doc, &parts).unwrap();
        let requested: Vec<_> = read
            .parts
            .iter()
            .enumerate()
            .map(|(i, p)| (PartId(i), p.clone()))
            .collect();
        let (edit, report) = pmi::write(
            &doc,
            &parts,
            &requested,
            Mode::Replace,
            PresentationPolicy::RemovePresentation,
        )
        .unwrap();
        let bytes = doc.apply(&edit).unwrap().bytes;
        let definitions = read_part_definitions(&bytes).unwrap();
        let after = Document::parse(bytes.clone()).unwrap();
        let reread = pmi::read(&after, &definitions).unwrap();
        for (i, p) in read.parts.iter().enumerate() {
            assert!(
                differences(&pmi::write::standard_form(p), &reread.parts[i]).is_empty(),
                "pass {pass}"
            );
        }
        for id in [348, 414] {
            assert_eq!(
                &doc.bytes()[doc.span(id).unwrap()],
                &after.bytes()[after.span(id).unwrap()],
                "unread #{id}, pass {pass}"
            );
        }
        for id in [302, 303, 304] {
            assert!(
                report.retained_shared.contains(&id),
                "pass {pass}: {report:?}"
            );
        }
        input = bytes;
    }
}
