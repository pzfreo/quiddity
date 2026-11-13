//! Rust recognition evidence snapshots, structural checks and unchanged motion references.

use super::common;

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use quiddity::Part;
use quiddity::evidence_view::{RecognitionEvidence, build_recognition_evidence};
use quiddity::kernel::step::{Placement, read_step_file, read_step_file_placed};
use serde_json::{Value, json};

const CAPTURE: &str = "captured/recognition_evidence/capture.json.gz";
const KNOWN: &str = "references/recognition_evidence_motion.json";

#[test]
fn document_record_serialization_matches_the_individual_accessor_bytes() {
    let mut compared = 0;
    let mut recesses = 0;
    let mut source_refusals = 0;
    for file in [
        "golden_blind_pockets_and_pocket_patterns.step",
        "golden_counterbored_and_countersunk_holes.step",
        "golden_open_channels.step",
        "held_side_knot.step",
        "unowned_recess_solid.step.gz",
        "unowned_recess_shell.step.gz",
        "unowned_recess_mixed.step.gz",
    ] {
        let part = read_step_file(&common::fixtures().join(file)).unwrap();
        let view = build_recognition_evidence(&part).unwrap();
        let document = quiddity::evidence_view::document(&part, false).unwrap();
        assert_eq!(document.features.len(), view.features.len(), "{file}");
        for (position, record) in document.features.iter().enumerate() {
            assert_eq!(
                serde_json::to_vec(&record.record).unwrap(),
                serde_json::to_vec(&view.record(position)).unwrap(),
                "{file} {position}"
            );
            assert_eq!(record.family, view.features[position].family);
            assert_eq!(record.index, view.features[position].index);
            compared += 1;
            recesses += usize::from(record.family == "section_recesses");
            source_refusals += usize::from(record.family == "section_recess_source_refusals");
        }
    }
    assert!(compared > 0 && recesses > 0 && source_refusals > 0);
}

fn corpus() -> Option<PathBuf> {
    let corpus = common::corpus_dir();
    if corpus.is_none() {
        assert!(
            std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
            "QUIDDITY_CORPUS_REQUIRED is set but the corpus was not found"
        );
    }
    corpus
}

fn capture() -> Value {
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(common::fixtures().join(CAPTURE)).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

fn parts(capture: &Value) -> &Vec<Value> {
    capture["parts"].as_array().unwrap()
}

fn read(dir: &Path, file: &str) -> Part {
    read_step_file(&dir.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"))
}

/// One side's view in the capture's JSON shape, or its refusal.
fn port_view(part: &Part) -> Result<Value, String> {
    let view = build_recognition_evidence(part).map_err(|e| e.to_string())?;
    Ok(as_json(&view))
}

fn as_json(view: &RecognitionEvidence<'_>) -> Value {
    let candidates: Vec<Value> = view
        .candidates
        .iter()
        .map(|c| {
            json!({"family": c.family, "index": c.index, "outcome": c.outcome.value(),
                   "reason": c.reason_value(), "defining": c.defining,
                   "constituent": c.constituent, "related": c.related})
        })
        .collect();
    json!({"features": view.features, "candidates": candidates,
           "rejected": view.rejected_candidates})
}

type Key = (String, Vec<u64>);

fn key(item: &Value) -> Key {
    let defining = item["defining"].as_array().unwrap();
    (
        item["family"].as_str().unwrap().to_owned(),
        defining.iter().map(|f| f.as_u64().unwrap()).collect(),
    )
}

/// How two views are compared: the port with unmoved Rust, or the port with itself moved (where the
/// order of records, a candidate's index, a pattern's member order and a circular pattern's
/// group order follow the placement, and step levels and risers, read along world Z, are
/// compared only when the motion keeps Z vertical).
#[derive(Clone, Copy, PartialEq)]
enum Compare {
    Moved { z_vertical: bool },
}

impl Compare {
    fn skips(self, family: &str) -> bool {
        self == (Compare::Moved { z_vertical: false }) && matches!(family, "step_levels" | "risers")
    }
}

/// Features by key, each with its comparable fields (members by their keys).
fn features(view: &Value, mode: Compare) -> BTreeMap<Key, Vec<BTreeMap<&'static str, Value>>> {
    let list = view["features"].as_array().unwrap();
    let mut out: BTreeMap<Key, Vec<BTreeMap<&'static str, Value>>> = BTreeMap::new();
    for f in list {
        if mode.skips(f["family"].as_str().unwrap()) {
            continue;
        }
        let mut groups = f["groups"].as_array().unwrap().clone();
        let mut members: Vec<Value> = f["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                let (family, defining) = key(&list[m.as_u64().unwrap() as usize]);
                json!([family, defining])
            })
            .collect();
        {
            groups.sort_by_key(|g| g.to_string());
            members.sort_by_key(|m| m.to_string());
        }
        let fields = BTreeMap::from([
            ("constituent", f["constituent"].clone()),
            ("hosts", f["hosts"].clone()),
            ("groups", Value::Array(groups)),
            ("members", Value::Array(members)),
        ]);
        out.entry(key(f)).or_default().push(fields);
    }
    out
}

/// Candidates by key, each as its summary (related by their keys).
fn candidates(view: &Value, mode: Compare) -> BTreeMap<Key, Vec<Value>> {
    let list = view["candidates"].as_array().unwrap();
    let mut out: BTreeMap<Key, Vec<Value>> = BTreeMap::new();
    for c in list {
        if mode.skips(c["family"].as_str().unwrap()) {
            continue;
        }
        let mut related: Vec<Value> = c["related"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                let (family, defining) = key(&list[r.as_u64().unwrap() as usize]);
                json!([family, defining])
            })
            .collect();
        let mut summary = json!({
            "index": c["index"], "outcome": c["outcome"], "reason": c["reason"],
            "constituent": c["constituent"],
        });
        {
            summary.as_object_mut().unwrap().remove("index");
            related.sort_by_key(|r| r.to_string());
        }
        summary["related"] = Value::Array(related);
        out.entry(key(c)).or_default().push(summary);
    }
    out
}

fn one_or_all<T: Clone + serde::Serialize>(v: Option<&Vec<T>>) -> Value {
    match v {
        None => Value::Null,
        Some(v) if v.len() == 1 => json!(v[0]),
        Some(v) => json!(v),
    }
}

/// Every difference between two views, by list: features, candidates, errors.
fn differences(
    file: &str,
    left: &Result<Value, String>,
    right: &Result<Value, String>,
    mode: Compare,
) -> BTreeMap<&'static str, Vec<Value>> {
    let mut out: BTreeMap<&'static str, Vec<Value>> = BTreeMap::new();
    let (l, r) = match (left, right) {
        (Ok(l), Ok(r)) => (l, r),
        _ => {
            if left.as_ref().err() != right.as_ref().err() {
                let side = |s: &Result<Value, String>| match s {
                    Ok(_) => json!("viewed"),
                    Err(e) => json!({"error": e}),
                };
                out.entry("errors")
                    .or_default()
                    .push(json!({"file": file, "unmoved": side(left), "rust": side(right)}));
            }
            return out;
        }
    };
    let (lf, rf) = (features(l, mode), features(r, mode));
    let mut keys: Vec<&Key> = lf.keys().chain(rf.keys()).collect();
    keys.sort();
    keys.dedup();
    for k in keys {
        let entry = |field: &str, p: Value, q: Value| {
            json!({"file": file, "family": k.0, "defining": k.1, "field": field,
                   "unmoved": p, "rust": q})
        };
        match (lf.get(k), rf.get(k)) {
            (Some(a), Some(b)) if a.len() == 1 && b.len() == 1 => {
                for field in ["constituent", "hosts", "groups", "members"] {
                    if a[0][field] != b[0][field] {
                        out.entry("features").or_default().push(entry(
                            field,
                            a[0][field].clone(),
                            b[0][field].clone(),
                        ));
                    }
                }
            }
            (a, b) => {
                let count = |v: Option<&Vec<_>>| v.map_or(0, Vec::len);
                if a != b {
                    out.entry("features").or_default().push(entry(
                        "present",
                        json!(count(a)),
                        json!(count(b)),
                    ));
                }
            }
        }
    }
    let (lc, rc) = (candidates(l, mode), candidates(r, mode));
    let mut keys: Vec<&Key> = lc.keys().chain(rc.keys()).collect();
    keys.sort();
    keys.dedup();
    for k in keys {
        let (a, b) = (one_or_all(lc.get(k)), one_or_all(rc.get(k)));
        if a != b {
            out.entry("candidates").or_default().push(
                json!({"file": file, "family": k.0, "defining": k.1, "unmoved": a, "rust": b}),
            );
        }
    }
    out
}

/// Every difference must be listed once in *list*, and every listed one found.
fn check_known(found: Vec<Value>, list: &str) {
    let problems = known_problems(found, list);
    assert!(problems.is_empty(), "{problems}");
}

/// [`check_known`]'s problems, or empty.
fn known_problems(found: Vec<Value>, list: &str) -> String {
    let known = common::load(KNOWN);
    let entries = known[list].as_array().unwrap();
    common::check_verdicts(
        &format!("recognition_evidence/known_differences.json {list}"),
        entries,
    );
    let strip = |e: &Value| {
        let mut e = e.clone();
        let map = e.as_object_mut().unwrap();
        map.remove("verdict");
        map.remove("reason");
        map.remove("resolved");
        e.to_string()
    };
    let mut listed: BTreeMap<String, bool> = BTreeMap::new();
    let mut resolved = std::collections::BTreeSet::new();
    let mut problems = Vec::new();
    for e in entries {
        // Keep the historical expectation, but ratchet a proved fix: this exact difference
        // must stay absent. A resolved entry cannot permit a reintroduced discrepancy.
        if e.get("resolved").and_then(Value::as_bool) == Some(true) {
            if !resolved.insert(strip(e)) {
                problems.push(format!("resolved twice: {e}"));
            }
            continue;
        }
        if listed.insert(strip(e), false).is_some() {
            problems.push(format!("listed twice: {e}"));
        }
    }
    let mut needed = Vec::new();
    for difference in found {
        if resolved.contains(&strip(&difference)) {
            problems.push(format!("resolved difference returned: {difference}"));
            continue;
        }
        match listed.get_mut(&strip(&difference)) {
            Some(seen) => *seen = true,
            None => {
                problems.push(format!("unlisted: {difference}"));
                let mut entry = difference.clone();
                entry["verdict"] = json!("undetermined");
                entry["reason"] = json!("TODO");
                needed.push(entry);
            }
        }
    }
    problems.extend(
        listed
            .iter()
            .filter(|(_, seen)| !**seen)
            .map(|(k, _)| format!("stale: {k}")),
    );
    if !needed.is_empty() {
        problems.push(format!(
            "entries needed:\n{}",
            serde_json::to_string_pretty(&needed).unwrap()
        ));
    }
    if problems.is_empty() {
        String::new()
    } else {
        format!("{list}:\n{}", problems.join("\n"))
    }
}

/// The capture is of this corpus, at the revision `corpus.json` records (every file and its
/// sha256).
#[test]
fn capture_covers_the_corpus() {
    let capture = capture();
    let corpus = common::load("corpus.json");
    assert_eq!(capture["quiddity_revision"], corpus["quiddity_revision"]);
    let want: Vec<(&Value, &Value)> = corpus["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (&f["file"], &f["sha256"]))
        .collect();
    let got: Vec<(&Value, &Value)> = parts(&capture)
        .iter()
        .map(|p| (&p["file"], &p["sha256"]))
        .collect();
    assert_eq!(got, want);
}

/// Two of `tests/invariance.rs`'s rigid motions: one moving every axis and the origin, and one
/// reversing two axes (keeping Z vertical), each with whether it keeps Z vertical.
const MOTIONS: [(&str, Placement, bool); 2] = [
    (
        "rot_zx_moved",
        [
            [0.0, 0.0, 1.0, 123.456],
            [1.0, 0.0, 0.0, -78.9],
            [0.0, 1.0, 0.0, 41.3],
        ],
        false,
    ),
    (
        "rot_y180",
        [
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, -1.0, 0.0],
        ],
        true,
    ),
];

/// The view is the same faces however the part is placed.
#[test]
fn rust_snapshot_recognition_evidence_and_view_is_placement_independent() {
    let Some(dir) = corpus() else { return };
    let capture = capture();
    let found = common::parallel::map(parts(&capture), |p| {
        let file = p["file"].as_str().unwrap();
        let path = dir.join(file);
        let part = read(&dir, file);
        let want = build_recognition_evidence(&part)
            .map(|view| {
                for f in &view.features {
                    for &m in &f.members {
                        assert_eq!(view.features[m].family, "holes", "{file}");
                    }
                }
                assert!(view.rejected_candidates.iter().all(|&c| {
                    view.candidates[c].outcome == quiddity::features::reconcile::Outcome::Rejected
                }));
                for (at, f) in view.features.iter().enumerate() {
                    let record = view.record(at);
                    assert!(record.is_object(), "{file}: {} has no record", f.family);
                }
                as_json(&view)
            })
            .map_err(|e| e.to_string());
        let mut out = Vec::new();
        for (motion, placement, z_vertical) in &MOTIONS {
            let moved = read_step_file_placed(&path, placement).unwrap();
            let got = port_view(&moved);
            let mode = Compare::Moved {
                z_vertical: *z_vertical,
            };
            for (list, entries) in differences(file, &want, &got, mode) {
                for mut d in entries {
                    let map = d.as_object_mut().unwrap();
                    map.insert("list".into(), json!(list));
                    map.insert("motion".into(), json!(motion));
                    map.insert("moved".into(), map["rust"].clone());
                    map.insert("rust".into(), map["unmoved"].clone());
                    map.remove("unmoved");
                    out.push(d);
                }
            }
        }
        (json!({"file":file,"view":want}), out)
    });
    let (snapshots, motions): (Vec<_>, Vec<_>) = found.into_iter().unzip();
    common::snapshot::check(
        &common::fixtures(),
        "recognition_evidence",
        &json!(snapshots),
    );
    check_known(motions.into_iter().flatten().collect(), "motions");
}
