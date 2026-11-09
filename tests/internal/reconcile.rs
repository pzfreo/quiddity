//! Rust reconciliation snapshots, analytic face-group scenarios and unchanged decision motion references.

use super::common;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use quiddity::Part;
use quiddity::features;
use quiddity::features::passage_compat::PassageCompatibilityView;
use quiddity::features::reconcile::{self, Disposition, Evidence};
use quiddity::kernel::step::{Placement, read_step_file, read_step_file_placed};
use serde_json::{Value, json};

const CAPTURE: &str = "captured/reconcile/capture.json.gz";
const KNOWN: &str = "references/reconcile_motion.json";

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

/// One side's decisions by (family, defining faces): outcome, reason and related faces (sorted;
/// the related family is the reason's).
type Decided = BTreeMap<(String, Vec<usize>), Vec<Value>>;

fn summary(outcome: &str, reason: &str, mut related: Vec<Vec<usize>>) -> Value {
    related.sort();
    json!({"outcome": outcome, "reason": reason, "related": related})
}

fn faces(v: &Value) -> Vec<usize> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_u64().unwrap() as usize)
        .collect()
}

/// The port's decisions on *part*, or its refusal.
fn port_decisions(part: &Part) -> Result<Decided, String> {
    decided(features::inventory(part).map(|i| i.dispositions))
}

fn decided(
    decisions: Result<Vec<Disposition>, reconcile::ReconcileError>,
) -> Result<Decided, String> {
    let decisions = decisions.map_err(|e| e.to_string())?;
    let mut out = Decided::new();
    for d in decisions {
        out.entry((d.candidate.family.value().to_owned(), d.candidate.defining))
            .or_default()
            .push(summary(
                d.outcome.value(),
                d.reason.value(),
                d.related.into_iter().map(|r| r.defining).collect(),
            ));
    }
    Ok(out)
}

/// Each candidate either side decided differently, as an entry without its verdict.
fn differences(
    file: &str,
    unmoved: &Result<Decided, String>,
    rust: &Result<Decided, String>,
) -> Vec<Value> {
    let (unmoved, rust) = match (unmoved, rust) {
        (Ok(p), Ok(r)) => (p, r),
        _ => {
            let side = |s: &Result<Decided, String>| match s {
                Ok(_) => json!("decided"),
                Err(e) => json!({"error": e}),
            };
            if unmoved.as_ref().err() == rust.as_ref().err() {
                return Vec::new();
            }
            return vec![json!({"file": file, "family": null, "faces": null,
                               "unmoved": side(unmoved), "rust": side(rust)})];
        }
    };
    let mut keys: Vec<&(String, Vec<usize>)> = unmoved.keys().chain(rust.keys()).collect();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .filter_map(|key| {
            let (p, r) = (unmoved.get(key), rust.get(key));
            let sorted = |v: Option<&Vec<Value>>| {
                v.map(|v| {
                    let mut v = v.clone();
                    v.sort_by_key(|s| s.to_string());
                    if v.len() == 1 {
                        v.remove(0)
                    } else {
                        Value::Array(v)
                    }
                })
            };
            let (p, r) = (sorted(p), sorted(r));
            (p != r).then(
                || json!({"file": file, "family": key.0, "faces": key.1, "unmoved": p, "rust": r}),
            )
        })
        .collect()
}

/// Every difference must be listed once, and every listed one found.
fn check_known(found: Vec<Value>, list: &str) {
    let known = common::load(KNOWN);
    let entries = known[list].as_array().unwrap();
    common::check_verdicts(&format!("reconcile/known.json {list}"), entries);
    let key = |e: &Value| {
        let mut e = e.clone();
        let map = e.as_object_mut().unwrap();
        map.remove("verdict");
        map.remove("reason");
        e.to_string()
    };
    let mut listed: BTreeMap<String, bool> = BTreeMap::new();
    let mut problems = Vec::new();
    for e in entries {
        if listed.insert(key(e), false).is_some() {
            problems.push(format!("listed twice: {e}"));
        }
    }
    let mut needed = Vec::new();
    for difference in found {
        match listed.get_mut(&key(&difference)) {
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
    assert!(problems.is_empty(), "{}", problems.join("\n"));
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

fn read(dir: &Path, file: &str) -> Part {
    read_step_file(&dir.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"))
}

/// The `Features` fields (`Defining` keys) the rules read, by unmoved Rust family value.
const FIELDS: [(&str, &str); 19] = [
    ("angled_steps", "angled_steps"),
    ("blends", "blends"),
    ("bosses", "bosses"),
    ("chamfers", "chamfers"),
    ("circular_blind_steps", "circular_blind_steps"),
    ("double_d_bores", "double_d_bores"),
    ("edge_open_circular_pockets", "edge_open_circular_pockets"),
    ("fillets", "fillets"),
    ("grooves", "grooves"),
    ("holes", "holes"),
    ("oriented_slots", "oriented_slots"),
    ("passages", "section_passages"),
    ("plates", "plates"),
    ("pockets", "pockets"),
    ("prismatic_pockets", "prismatic_pockets"),
    ("rectangular_blind_slots", "rectangular_blind_slots"),
    ("slots", "slots"),
    ("thin_wall_bodies", "thin_wall_bodies"),
    ("turned_steps", "turned_steps"),
];

fn face_lists(v: Option<&Value>) -> Vec<Vec<usize>> {
    v.map_or_else(Vec::new, |v| {
        v.as_array().unwrap().iter().map(faces).collect()
    })
}

/// unmoved Rust's decisions on the synthetic scenarios that reach the branches the corpus does not
/// (`tools/capture_reconcile.py --scenarios`), rebuilt from the same inputs: every one agrees.
fn snapshot_reconcile_scenarios(mut reasons: BTreeSet<String>, with_corpus: bool) {
    let captured = common::load("captured/reconcile/scenarios.json");
    let mut scenarios = captured["scenarios"].as_array().unwrap().clone();
    // Geometry proofs populate this extra role evidence; these analytic face
    // groups independently exercise exclusive ownership and the new reasons.
    for family in ["fillets", "blends"] {
        let mut families = serde_json::Map::new();
        families.insert("pockets".into(), json!([[0, 1, 2]]));
        families.insert(family.into(), json!([[2]]));
        scenarios.push(json!({
            "name": format!("pocket_floor_{family}"), "families": families,
            "pocket_constituent": [[0, 1, 2, 3]], "pocket_floor_fillets": [[2]]
        }));
    }
    let mut actual = Vec::new();
    for s in &scenarios {
        let families = &s["families"];
        for name in families.as_object().unwrap().keys() {
            assert!(
                name == "risers" || FIELDS.iter().any(|(f, _)| f == name),
                "{name}: no field"
            );
        }
        let defining: features::Defining = FIELDS
            .iter()
            .map(|&(family, field)| (field, face_lists(families.get(family))))
            .collect();
        let sides: Vec<usize> = s.get("prismatic_sides").map_or_else(Vec::new, |v| {
            v.as_array()
                .unwrap()
                .iter()
                .map(|n| n.as_u64().unwrap() as usize)
                .collect()
        });
        let groupings = s
            .get("passage_groupings")
            .map_or_else(Vec::new, |v| v.as_array().unwrap().clone());
        let view = |g: &Value| {
            if g.is_null() {
                return PassageCompatibilityView::new(None, None, None, None, None, None, false);
            }
            let axis = ["x", "y", "z"]
                .into_iter()
                .find(|a| *a == g[0])
                .expect("an axis");
            let section = g[1]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()])
                .collect();
            let sides = g[2].as_u64().map(|n| n as usize);
            PassageCompatibilityView::new(Some(axis), Some(section), sides, None, None, None, false)
        };
        let evidence = Evidence {
            pocket_floor_fillets: s.get("pocket_floor_fillets").map_or_else(
                || vec![Vec::new(); defining["pockets"].len()],
                |v| face_lists(Some(v)),
            ),
            pocket_constituent: face_lists(s.get("pocket_constituent")),
            passage_compatibility: groupings.iter().map(|g| view(g).unwrap()).collect(),
            risers: face_lists(families.get("risers")),
        };
        let rust = decided(reconcile::reconcile_faces(&defining, &sides, &evidence));
        if let Ok(map) = &rust {
            for d in map.values().flatten() {
                reasons.insert(d["reason"].as_str().unwrap().to_owned());
            }
        }
        actual.push(json!({"name":s["name"],"result":decision_json(rust)}));
    }
    common::snapshot::check(&common::fixtures(), "reconcile_scenarios", &json!(actual));
    if with_corpus {
        let missing: Vec<_> = reconcile::ReasonCode::ALL
            .iter()
            .filter(|r| !reasons.contains(r.value()))
            .collect();
        assert!(missing.is_empty(), "reasons never exercised: {missing:?}");
    }
}

/// Two of `tests/invariance.rs`'s rigid motions: one moving every axis and the origin, and one
/// reversing two axes.
const MOTIONS: [(&str, Placement); 2] = [
    (
        "rot_zx_moved",
        [
            [0.0, 0.0, 1.0, 123.456],
            [1.0, 0.0, 0.0, -78.9],
            [0.0, 1.0, 0.0, 41.3],
        ],
    ),
    (
        "rot_y180",
        [
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, -1.0, 0.0],
        ],
    ),
];

/// The port's decisions are the same faces however the part is placed.
#[test]
fn rust_snapshot_reconcile_and_decisions_are_placement_independent() {
    let Some(dir) = corpus() else {
        snapshot_reconcile_scenarios(BTreeSet::new(), false);
        return;
    };
    let capture = capture();
    let found = common::parallel::map(parts(&capture), |p| {
        let file = p["file"].as_str().unwrap();
        let path = dir.join(file);
        let inventory = features::inventory(&read(&dir, file));
        let want = decided(inventory.clone().map(|i| i.dispositions));
        let reasons: BTreeSet<String> = want
            .as_ref()
            .map(|map| {
                map.values()
                    .flatten()
                    .map(|d| d["reason"].as_str().unwrap().to_owned())
                    .collect()
            })
            .unwrap_or_default();
        let accepted = inventory
            .map(|i| i.accepted().defining)
            .map_err(|e| e.to_string());
        let snapshot =
            json!({"file":file,"decisions":decision_json(want.clone()),"accepted":accepted});
        let mut out = Vec::new();
        for (motion, placement) in &MOTIONS {
            let moved = read_step_file_placed(&path, placement).unwrap();
            let got = port_decisions(&moved);
            for mut d in differences(file, &want, &got) {
                let map = d.as_object_mut().unwrap();
                map.insert("motion".into(), json!(motion));
                map.insert("moved".into(), map["rust"].clone());
                map.insert("rust".into(), map["unmoved"].clone());
                map.remove("unmoved");
                out.push(d);
            }
        }
        (snapshot, out, reasons)
    });
    let mut snapshots = Vec::new();
    let mut motions = Vec::new();
    let mut reasons = BTreeSet::new();
    for (snapshot, motion, codes) in found {
        snapshots.push(snapshot);
        motions.extend(motion);
        reasons.extend(codes);
    }
    common::snapshot::check(&common::fixtures(), "reconcile", &json!(snapshots));
    snapshot_reconcile_scenarios(reasons, true);
    check_known(motions, "motions");
}

fn decision_json(result: Result<Decided, String>) -> Value {
    match result {
        Ok(map) => {
            json!({"decisions":map.into_iter().map(|((family,faces),decisions)|json!({"family":family,"faces":faces,"decisions":decisions})).collect::<Vec<_>>()})
        }
        Err(e) => json!({"error":e}),
    }
}
