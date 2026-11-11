//! Revision correspondence (`docs/correspondence.md`, "Tests").
//!
//! - Invariance: every corpus part, read under each rigid motion, corresponds with its unmoved
//!   self with every feature and every face carried, each to the very feature (same defining
//!   faces) and face (same index) it is. It runs in `tests/invariance.rs`, on the same readings
//!   as recognition's invariance.
//! - Revision pairs: build123d parts revised as a designer would (`tools/make_revision_fixtures.py`,
//!   `tests/fixtures/revisions/`), each feature's class checked against the expected one, each
//!   carried feature's faces (patterns aside) checked carried onto its partner's, and every face of a whole-part
//!   motion checked carried.
//!
//! Differences are listed with verdicts in `tests/fixtures/references/correspondence.json`; the test
//! fails on an unlisted one and on a listed one that has gone.

use super::common;

use std::collections::BTreeMap;

use quiddity::correspondence::{self, Class, Correspondence, Fingerprints};
use serde_json::Value;

fn faces_of<'a>(f: &'a Fingerprints, id: &str) -> &'a [usize] {
    &f.features.iter().find(|x| x.id == id).unwrap().faces
}

fn known() -> Vec<Value> {
    let known = common::load("references/correspondence.json");
    let known = known.as_array().unwrap().clone();
    common::check_verdicts("references/correspondence.json", &known);
    known
}

/// The revision pairs: each case's expected class for named features, by the feature's
/// family and position (fixture `expected.json`, written by `tools/make_revision_fixtures.py`).
#[test]
fn revision_pairs_have_their_expected_classes() {
    let root = common::fixtures().join("revisions");
    let cases = common::load("revisions/expected.json");
    let known: Vec<(String, String)> = known()
        .iter()
        .filter(|k| k["test"] == "revisions")
        .map(|k| {
            (
                k["case"].as_str().unwrap().to_string(),
                k["check"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let mut problems = Vec::new();
    let mut seen = Vec::new();
    let mut checked = 0;
    for case in cases["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let read = |which: &str| {
            let file = case[format!("{which}_file")]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{name}.{which}.step.gz"));
            let part = quiddity::read_step_file(&root.join(file)).unwrap();
            correspondence::caller_space_recognise(&part).fingerprints
        };
        let (old, new) = (read("old"), read("new"));
        let c = correspondence::correspond(&old, &new).unwrap();
        // Equal revisions are the pre-existing matcher; this file's expectations below were
        // written before revisions existed. Across the revision gap the result must agree
        // with it exactly on every corpus pair.
        assert!(c.excluded_keys.is_empty());
        let mut gap_old = old.clone();
        gap_old.fingerprint_revision = 0;
        let g = correspondence::correspond(&gap_old, &new).unwrap();
        assert_same_classification(&new, &c, &g, &format!("{name}: revision 0 against current"));
        // A part against itself across the gap is all carried.
        let s = correspondence::correspond(&gap_old, &old).unwrap();
        assert!(
            s.features.iter().all(|e| e.class == Class::Carried)
                && s.faces.iter().all(|e| e.class == Class::Carried),
            "{name}: old against itself across the gap"
        );
        let mut results: Vec<(String, bool, String)> = Vec::new();
        // Alignment.
        let aligned = case["aligned"].as_bool().unwrap();
        results.push((
            "aligned".into(),
            c.alignment.found == aligned,
            format!("expected aligned {aligned}, got {:?}", c.alignment),
        ));
        // Each expectation names a feature by family and nearest old (or new) position.
        for exp in case["expect"].as_array().unwrap() {
            let family = exp["family"].as_str().unwrap();
            let class = exp["class"].as_str().unwrap();
            let label = exp["label"].as_str().unwrap();
            let check = format!("{label}: {family} {class}");
            let (ok, got) = check_expectation(&old, &new, &c, exp);
            results.push((check, ok, got));
        }
        // Every other feature: carried.
        let expected_ids = expected_ids(&old, &new, case);
        for e in &c.features {
            if expected_ids.contains(&e.old) {
                continue;
            }
            results.push((
                format!("other {}: carried", e.old),
                e.class == Class::Carried,
                format!("{:?} {:?} {:?}", e.class, e.candidates, e.changes),
            ));
        }
        for id in &c.new_features {
            if !expected_ids.contains(id) {
                results.push((format!("other new {id}"), false, "unexpectedly new".into()));
            }
        }
        // Faces: each carried feature's faces carried onto its partner's faces, and with nothing
        // expected to change (a whole-part motion), every face carried and none new.
        // (A pattern's faces are its members', which are checked as features in their own right.)
        // For #134 a carried supporting feature may have an adapted, trimmed face. Check
        // those explicitly, including the partner, without weakening untouched-face checks.
        let face_expect: Vec<_> = case["face_expect"]
            .as_array()
            .into_iter()
            .flatten()
            .collect();
        for exp in &face_expect {
            let f = exp["old"].as_u64().unwrap() as usize;
            let fe = &c.faces[f];
            results.push((
                format!("#134 trimmed face {f}"),
                serde_json::to_value(fe.class).unwrap() == exp["class"]
                    && serde_json::to_value(fe.new).unwrap() == exp["new"]
                    && exp["changes"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .all(|k| fe.changes.contains_key(k.as_str().unwrap())),
                format!("{fe:?}"),
            ));
        }
        for e in &c.features {
            if e.class != Class::Carried || e.old.split('/').next().unwrap().ends_with("_patterns")
            {
                continue;
            }
            let theirs = faces_of(&new, e.new.as_deref().unwrap());
            for &f in faces_of(&old, &e.old) {
                if face_expect
                    .iter()
                    .any(|exp| exp["old"].as_u64() == Some(f as u64))
                {
                    continue;
                }
                let fe = &c.faces[f];
                results.push((
                    format!("faces of {}: face {f} carried", e.old),
                    fe.class == Class::Carried && fe.new.is_some_and(|n| theirs.contains(&n)),
                    format!("{:?} → {:?} {:?}", fe.class, fe.new, fe.changes),
                ));
            }
        }
        if case["expect"].as_array().unwrap().is_empty() {
            let moved: Vec<String> = c
                .faces
                .iter()
                .filter(|e| e.class != Class::Carried)
                .map(|e| format!("{} {:?}", e.old, e.class))
                .collect();
            results.push((
                "every face carried".into(),
                moved.is_empty() && c.new_faces.is_empty(),
                format!("{moved:?}, new {:?}", c.new_faces),
            ));
        }
        for (check, ok, got) in results {
            checked += 1;
            let key = (name.to_string(), check.clone());
            if known.contains(&key) {
                if ok {
                    problems.push(format!("{key:?} is listed but now holds: remove it"));
                }
                seen.push(key);
            } else if !ok {
                problems.push(format!("{name}: {check}: {got}"));
            }
        }
    }
    for key in &known {
        if !seen.contains(key) {
            problems.push(format!("{key:?} is listed but was not checked"));
        }
    }
    eprintln!("{checked} revision checks");
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The feature of *family* in *f* nearest *at*.
fn nearest(f: &Fingerprints, family: &str, at: &Value) -> Option<String> {
    let at: Vec<f64> = at.as_array()?.iter().map(|x| x.as_f64().unwrap()).collect();
    f.features
        .iter()
        .filter(|x| x.family == family && x.position.is_some())
        .min_by(|a, b| {
            let d = |p: [f64; 3]| (0..3).map(|i| (p[i] - at[i]).powi(2)).sum::<f64>();
            d(a.position.unwrap()).total_cmp(&d(b.position.unwrap()))
        })
        .map(|x| x.id.clone())
}

fn expected_ids(old: &Fingerprints, new: &Fingerprints, case: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for exp in case["expect"].as_array().unwrap() {
        let family = exp["family"].as_str().unwrap();
        if let Some(id) = nearest(old, family, &exp["old_at"]) {
            out.push(id);
        }
        if let Some(id) = nearest(new, family, &exp["new_at"]) {
            out.push(id);
        }
        for at in exp["also"].as_array().into_iter().flatten() {
            if let Some(id) = nearest(old, family, at) {
                out.push(id);
            }
        }
    }
    out
}

/// Whether the correspondence gives the expectation's feature its class (and, where given, its
/// partner and changed fields).
fn check_expectation(
    old: &Fingerprints,
    new: &Fingerprints,
    c: &Correspondence,
    exp: &Value,
) -> (bool, String) {
    let family = exp["family"].as_str().unwrap();
    let class = exp["class"].as_str().unwrap();
    if class == "absent" {
        let absent = !old
            .features
            .iter()
            .chain(&new.features)
            .any(|f| f.family == family);
        return (absent, format!("{family} must be absent in both revisions"));
    }
    let new_id = nearest(new, family, &exp["new_at"]);
    if class == "new" {
        let Some(id) = new_id else {
            return (false, format!("no new {family} near {}", exp["new_at"]));
        };
        return (
            c.new_features.contains(&id),
            format!("{id} is not new: {:?}", c.new_features),
        );
    }
    let Some(id) = nearest(old, family, &exp["old_at"]) else {
        return (false, format!("no old {family} near {}", exp["old_at"]));
    };
    let e = c.features.iter().find(|e| e.old == id).unwrap();
    let got = format!(
        "{id}: {:?} → {:?} {:?} {:?}",
        e.class, e.new, e.candidates, e.changes
    );
    let class_ok = serde_json::to_value(e.class).unwrap() == class;
    let partner_ok = match class {
        "carried" | "adapted" => e.new.is_some() && e.new == new_id,
        "ambiguous" => new_id.is_none_or(|n| e.candidates.contains(&n)),
        _ => true,
    };
    let changes: BTreeMap<&str, ()> = exp["changes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|k| (k.as_str().unwrap(), ()))
        .collect();
    let changes_ok = changes.keys().all(|k| e.changes.contains_key(*k));
    (class_ok && partner_ok && changes_ok, got)
}

/// The same class, partner and candidates for every feature, and class and partner for every face.
fn assert_same_classification(
    new: &Fingerprints,
    equal: &Correspondence,
    gap: &Correspondence,
    what: &str,
) {
    // A feature whose equal-revision changes name a key the gap excludes may differ: a real
    // difference in a key of unknown vintage is the table's purpose.
    let excluded = |family: &str, key: &str| {
        gap.excluded_keys
            .iter()
            .any(|e| e.family == family && e.key == key)
    };
    // A position change excluded only along the axis is excusable when it is wholly axial.
    let axial = |e: &correspondence::Entry<String>| {
        let family = e.old.split('/').next().unwrap();
        let (Some(change), Some(id)) = (e.changes.get("position"), &e.new) else {
            return false;
        };
        let (Some(axis), true) = (
            new.features
                .iter()
                .find(|f| &f.id == id)
                .and_then(|f| f.axis),
            excluded(family, "position.axial"),
        ) else {
            return false;
        };
        let v = |x: &Value| -> [f64; 3] { std::array::from_fn(|i| x[i].as_f64().unwrap()) };
        let (a, b) = (v(&change.old), v(&change.new));
        let d: [f64; 3] = std::array::from_fn(|i| b[i] - a[i]);
        let along: f64 = (0..3).map(|i| d[i] * axis[i]).sum();
        let across = (0..3)
            .map(|i| (d[i] - along * axis[i]).powi(2))
            .sum::<f64>()
            .sqrt();
        across < 2e-3
    };
    for (e, g) in equal.features.iter().zip(&gap.features) {
        assert_eq!(e.old, g.old);
        let family = e.old.split('/').next().unwrap();
        if e.changes.keys().any(|k| excluded(family, k)) || axial(e) {
            eprintln!("{what}: {} differs in an excluded key", e.old);
            continue;
        }
        assert_eq!(
            (e.class, &e.new, &e.candidates),
            (g.class, &g.new, &g.candidates),
            "{what}: {}",
            e.old
        );
    }
    for (e, g) in equal.faces.iter().zip(&gap.faces) {
        assert_eq!(
            (e.class, &e.new),
            (g.class, &g.new),
            "{what}: face {}",
            e.old
        );
    }
}
