//! Family wiring (review findings M1 and H5): every family `features::recognise` returns is
//! either given its defining faces, record by record, or derived from another family's records;
//! and every leaf of every family's records has a role in its fingerprint table
//! (`correspondence::fingerprint::FAMILIES`), every path in a table being some leaf's, and every
//! feature's fingerprint has a size. Run over the STEP fixtures and, when present, the corpus.

use super::common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use quiddity::correspondence::fingerprint::{
    FAMILIES, classify, family, fingerprint, records_json,
};
use quiddity::features::{self, gussets, oriented_slots, recess_patterns};
use serde_json::Value;

/// Each leaf of *v* (a scalar or null) by its path: `a.b` into a nested record, `a[]` into a
/// list's items.
fn leaves(v: &Value, path: &str, out: &mut BTreeSet<String>) {
    match v {
        Value::Object(fields) => {
            for (k, x) in fields {
                let p = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                leaves(x, &p, out);
            }
        }
        Value::Array(items) => {
            for x in items {
                leaves(x, &format!("{path}[]"), out);
            }
        }
        _ => {
            out.insert(path.to_string());
        }
    }
}

/// What one part's recognition gets wrong in its wiring, and the leaf paths of its records by
/// family.
fn check(name: &str, json: &Value, defining: &features::Defining) -> (Vec<String>, Leaves) {
    let mut problems = Vec::new();
    let mut found = Leaves::new();
    let object = json.as_object().unwrap();
    for (key, records) in object {
        let Some(records) = records.as_array() else {
            continue;
        };
        let Some(table) = family(key) else {
            problems.push(format!("{name}: family {key} has no fingerprint table"));
            continue;
        };
        match (defining.get(key.as_str()), table.members) {
            (Some(d), None) if d.len() == records.len() => {}
            (Some(d), None) => problems.push(format!(
                "{name}: {key} has {} records but {} defining-face lists",
                records.len(),
                d.len()
            )),
            (Some(_), Some(_)) => problems.push(format!(
                "{name}: {key} is derived but also has defining faces"
            )),
            (None, Some((field, base))) => {
                if !defining.contains_key(base) {
                    problems.push(format!("{name}: {key}'s base {base} has no defining faces"));
                }
                for r in records {
                    if !r.get(field).is_some_and(Value::is_array) {
                        problems.push(format!("{name}: a {key} record has no member list {field}"));
                    }
                }
            }
            (None, None) => problems.push(format!(
                "{name}: {key} has no defining faces and is not derived"
            )),
        }
        let paths = found.entry(key.clone()).or_default();
        for r in records {
            leaves(r, "", paths);
        }
    }
    for key in defining.keys() {
        if !object.get(*key).is_some_and(Value::is_array) {
            problems.push(format!(
                "{name}: defining faces under {key}, which is no family"
            ));
        }
    }
    (problems, found)
}

type Leaves = BTreeMap<String, BTreeSet<String>>;

/// The STEP fixtures and the corpus's files (`corpus.json`), when the corpus is present.
fn parts() -> (Vec<PathBuf>, bool) {
    let mut out: Vec<PathBuf> = std::fs::read_dir(common::fixtures())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "step"))
        .collect();
    // Exercise the optional obstruction diagnostics, absent from the older STEP inputs.
    out.push(common::fixtures().join("partial_hole_exits.step.gz"));
    out.sort();
    // Nested compressed positives must participate in the field-by-field family audit.
    // No corpus part contains a spring, so the registry otherwise appears unused.
    out.push(common::fixtures().join("helical_springs/segmented.step.gz"));
    let corpus = common::corpus_dir();
    assert!(
        corpus.is_some() || std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
        "QUIDDITY_CORPUS_REQUIRED is set but the corpus was not found"
    );
    if let Some(dir) = &corpus {
        for entry in common::load("corpus.json")["files"].as_array().unwrap() {
            out.push(dir.join(entry["file"].as_str().unwrap()));
        }
    }
    (out, corpus.is_some())
}

#[test]
fn families_are_wired_and_their_records_fingerprinted_field_by_field() {
    let (paths, corpus) = parts();
    // Each part spread over every core, the results in `paths`' order.
    let results: Vec<(Vec<String>, Leaves)> = common::parallel::map(&paths, |path| {
        // The corpus test reports read failures.
        let Ok(part) = quiddity::read_step_file(path) else {
            return None;
        };
        let features = features::recognise(&part);
        let json = records_json(&features);
        let name = path.file_name().unwrap().to_string_lossy();
        let (mut problems, leaves) = check(&name, &json, &features.defining);
        // A feature with no size would be carried through any resize.
        for f in fingerprint(&part, &features).features {
            if f.sizes.is_empty() {
                problems.push(format!("{name}: {} has no sizes", f.id));
            }
        }
        Some((problems, leaves))
    })
    .into_iter()
    .flatten()
    .collect();
    let mut problems = Vec::new();
    let mut found = Leaves::new();
    for (p, leaves) in results {
        problems.extend(p);
        for (key, paths) in leaves {
            found.entry(key).or_default().extend(paths);
        }
    }
    // For #123, exercise the new family even without the optional corpus or local repro.
    let disc = super::face_grooves::two_ends();
    let grooves = features::recognise(&disc).face_grooves;
    assert_eq!(grooves.len(), 2);
    leaves(
        &serde_json::to_value(&grooves[0]).unwrap(),
        "",
        found.entry("face_grooves".into()).or_default(),
    );
    // The array variant of a gusset rib pattern is in no fixture or corpus part.
    let array = gussets::GussetRibPattern::Array {
        ribs: Vec::new(),
        axis: 'x',
        pitch: 10.0,
    };
    leaves(
        &serde_json::to_value(array).unwrap(),
        "",
        found.entry("gusset_rib_patterns".into()).or_default(),
    );
    // Nor is a sheet with formed features (local forming beside the flanges).
    leaves(
        &serde_json::json!({"formed_features": [{"faces": [0, 1], "paired_faces": [0]}]}),
        "",
        found.entry("sheet_metal_bodies".into()).or_default(),
    );
    // Nor is a slot grid or a pocket array.
    let grid = recess_patterns::SlotPattern::SlotGrid {
        slots: Vec::new(),
        rows: 2,
        cols: 2,
        row_pitch: 10.0,
        col_pitch: 10.0,
        angle: 0.0,
        center: [0.0; 3],
    };
    leaves(
        &serde_json::to_value(grid).unwrap(),
        "",
        found.entry("slot_patterns".into()).or_default(),
    );
    let array = recess_patterns::PocketPattern::PocketArray {
        pockets: Vec::new(),
        pitch: 10.0,
        direction: [1.0, 0.0, 0.0],
    };
    leaves(
        &serde_json::to_value(array).unwrap(),
        "",
        found.entry("pocket_patterns".into()).or_default(),
    );
    // Nor is an oriented slot pattern (the corpus has one oriented slot, 467.step's).
    let slot: oriented_slots::OrientedSlot = serde_json::from_value(serde_json::json!({
        "source": {
            "frame": {"origin": [0.0, 0.0, 0.0], "run": [0.0, 0.0, 1.0],
                      "u": [1.0, 0.0, 0.0], "v": [0.0, 1.0, 0.0]},
            "run_interval": [0.0, 10.0],
            "section": {"boundary": [
                {"point": [-4.0, -1.0], "bulge": 0.0}, {"point": [4.0, -1.0], "bulge": 0.0},
                {"point": [4.0, 1.0], "bulge": 0.0}, {"point": [-4.0, 1.0], "bulge": 0.0}]},
            "ends": {"low_capped": false, "high_capped": false,
                     "low_gradient": [0.0, 0.0], "high_gradient": [0.0, 0.0]}},
        "width_direction": [0.0, 1.0, 0.0], "long_direction": [1.0, 0.0, 0.0],
        "width": 2.0, "length": 8.0, "center": [0.0, 0.0, 5.0], "body_key": null
    }))
    .unwrap();
    for pattern in [
        oriented_slots::OrientedSlotPattern::OrientedSlotGrid {
            slots: vec![slot.clone()],
            rows: 2,
            cols: 3,
            row_pitch: 10.0,
            col_pitch: 10.0,
            angle: 0.0,
            center: [0.0; 3],
        },
        oriented_slots::OrientedSlotPattern::OrientedSlotArray {
            slots: vec![slot],
            pitch: 10.0,
            direction: [1.0, 0.0, 0.0],
        },
    ] {
        leaves(
            &serde_json::to_value(pattern).unwrap(),
            "",
            found.entry("oriented_slot_patterns".into()).or_default(),
        );
    }
    let mut used: BTreeSet<(String, &str)> = BTreeSet::new();
    for (key, paths) in &found {
        for path in paths {
            match classify(key, path) {
                Some((entry, _)) => {
                    used.insert((key.clone(), entry));
                }
                None => problems.push(format!(
                    "{key}: record field {path} has no role in the fingerprint table"
                )),
            }
        }
    }
    let unused: Vec<String> = FAMILIES
        .iter()
        .flat_map(|f| f.fields.iter().map(move |(p, _)| (f.family, *p)))
        .filter(|(key, p)| !used.contains(&(key.to_string(), *p)))
        .map(|(key, p)| format!("{key}: the fingerprint table's {p} is no record field"))
        .collect();
    // Without the corpus, some families have no records to show their fields.
    if corpus {
        problems.extend(unused);
    }
    eprintln!(
        "{} parts, {} families, {} leaf paths",
        paths.len(),
        found.len(),
        found.values().map(BTreeSet::len).sum::<usize>()
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn original_spotface_fixture_proves_optional_hole_fingerprint_fields() {
    let part =
        quiddity::read_step_file(&common::fixtures().join("spotface_outer_steps.step")).unwrap();
    assert_eq!(part.solids.len(), 1);
    assert!(part.solid_is_geometrically_valid(0));
    let features = features::recognise(&part);
    assert_eq!(features.holes.len(), 1);
    let hole = &features.holes[0];
    use features::holes::CounterBore;
    assert_eq!(
        hole.spotface,
        Some(CounterBore {
            diameter: 30.,
            depth: 0.5
        })
    );
    assert_eq!(
        hole.cbore,
        Some(CounterBore {
            diameter: 12.,
            depth: 3.
        })
    );
    assert_eq!(
        hole.outer_steps,
        [CounterBore {
            diameter: 20.,
            depth: 2.
        }]
    );
    let fingerprints = fingerprint(&part, &features);
    let fp = fingerprints
        .features
        .iter()
        .find(|fp| fp.family == "holes")
        .unwrap();
    assert_eq!(fp.traits["spotface"], "present");
    assert_eq!(fp.sizes["spotface.diameter"], 30.);
    assert_eq!(fp.sizes["spotface.depth"], 0.5);
    assert_eq!(fp.sizes["outer_steps.count"], 1.);
    assert_eq!(fp.sizes["outer_steps.0.diameter"], 20.);
    assert_eq!(fp.sizes["outer_steps.0.depth"], 2.);
}
