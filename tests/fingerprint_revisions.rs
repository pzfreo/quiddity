//! Value vintages within fingerprint/3, independent of recognition releases.
use std::path::Path;

use quiddity::correspondence::{self, Class, Correspondence, FINGERPRINT_REVISION, Fingerprints};
use serde_json::json;

fn recognise(name: &str) -> Fingerprints {
    let part = quiddity::read_step_file(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap();
    correspondence::recognise(&part).fingerprints
}

#[test]
fn new_values_have_a_revision_and_repeat_structurally() {
    let a = recognise("prismatic.step");
    let b = recognise("prismatic.step");
    assert_eq!(a.fingerprint_revision, FINGERPRINT_REVISION);
    assert_eq!(a, b);
    assert_eq!(
        serde_json::to_value(&a).unwrap(),
        serde_json::to_value(&b).unwrap()
    );
}

#[test]
fn absent_revision_loads_as_zero_and_old_correspondence_still_loads() {
    let current = recognise("prismatic.step");
    let mut stored = serde_json::to_value(&current).unwrap();
    stored
        .as_object_mut()
        .unwrap()
        .remove("fingerprint_revision");
    let old: Fingerprints = serde_json::from_value(stored).unwrap();
    assert_eq!(old.fingerprint_revision, 0);
    let c = correspondence::correspond(&old, &current).unwrap();
    assert_eq!(c.revisions.old, 0);
    assert_eq!(c.revisions.new, FINGERPRINT_REVISION);
    assert!(c.features.iter().all(|e| e.class == Class::Carried));
    assert!(c.faces.iter().all(|e| e.class == Class::Carried));
    let mut stored = serde_json::to_value(&c).unwrap();
    stored.as_object_mut().unwrap().remove("revisions");
    stored.as_object_mut().unwrap().remove("excluded_keys");
    let c: Correspondence = serde_json::from_value(stored).unwrap();
    assert_eq!(c.revisions.old, 0);
    assert_eq!(c.revisions.new, 0);
    assert!(c.excluded_keys.is_empty());
}

#[test]
fn unknown_void_volume_is_excluded_but_same_revision_volume_is_adapted() {
    let new = recognise("sealed_cylindrical_voids.step.gz");
    let mut old = new.clone();
    let void = old
        .features
        .iter_mut()
        .find(|f| f.family == "interior_voids")
        .unwrap();
    let id = void.id.clone();
    *void.sizes.get_mut("estimated_volume").unwrap() *= 0.8;
    void.traits
        .insert("volume_method".into(), "air_grid".into());
    let equal = correspondence::correspond(&old, &new).unwrap();
    let entry = equal.features.iter().find(|e| e.old == id).unwrap();
    assert_eq!(entry.class, Class::Adapted);
    assert!(entry.changes.contains_key("sizes.estimated_volume"));
    assert!(entry.changes.contains_key("traits.volume_method"));
    assert!(equal.excluded_keys.is_empty());
    let json = serde_json::to_value(equal).unwrap();
    assert_eq!(
        json["revisions"],
        json!({"old": FINGERPRINT_REVISION, "new": FINGERPRINT_REVISION})
    );
    assert!(json.get("excluded_keys").is_none());

    old.fingerprint_revision = 0;
    let saved = old.clone();
    for (a, b) in [(&old, &new), (&new, &old)] {
        let c = correspondence::correspond(a, b).unwrap();
        let entry = c.features.iter().find(|e| e.old == id).unwrap();
        assert_eq!(entry.class, Class::Carried);
        assert_eq!(entry.new.as_ref(), Some(&id));
        assert!(entry.changes.is_empty());
        assert_eq!(entry.cost, Some(0.0));
        for key in ["sizes.estimated_volume", "traits.volume_method"] {
            assert!(
                c.excluded_keys
                    .iter()
                    .any(|e| e.family == "interior_voids" && e.key == key)
            );
        }
    }
    assert_eq!(old, saved, "comparison must not mutate stored fingerprints");
}

#[test]
fn exclusions_are_key_and_family_specific_and_expand_indexed_sizes() {
    let mut new = recognise("prismatic.step");
    // Keep the real face graph, using one uniquely identified feature to isolate the costs.
    new.features.truncate(1);
    let f = &mut new.features[0];
    f.family = "holes".into();
    f.sizes = [
        ("outer_steps.0.diameter".into(), 12.0),
        ("diameter".into(), 5.0),
    ]
    .into();
    f.traits.clear();
    let mut old = new.clone();
    old.fingerprint_revision = 0;
    old.features[0]
        .sizes
        .insert("outer_steps.0.diameter".into(), 15.0);
    old.features[0].sizes.insert("diameter".into(), 6.0);
    let c = correspondence::correspond(&old, &new).unwrap();
    assert_eq!(c.features[0].class, Class::Adapted);
    assert_eq!(c.features[0].changes.len(), 1);
    assert!(c.features[0].changes.contains_key("sizes.diameter"));
    assert!(
        c.excluded_keys
            .iter()
            .any(|e| e.key == "sizes.outer_steps.0.diameter")
    );
    // A listed name in another family must remain comparable.
    old.features[0].family = "unrelated".into();
    new.features[0].family = "unrelated".into();
    let c = correspondence::correspond(&old, &new).unwrap();
    assert!(c.excluded_keys.is_empty());
    assert!(
        c.features[0]
            .changes
            .contains_key("sizes.outer_steps.0.diameter")
    );
}

#[test]
fn cli_passes_revision_metadata_through_for_step_and_saved_json() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/prismatic.step");
    let cli = |args: &[&str]| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_quiddity"))
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    let current = cli(&["fingerprints", root.to_str().unwrap()]);
    assert_eq!(current["fingerprint_revision"], FINGERPRINT_REVISION);
    let equal = cli(&["correspond", root.to_str().unwrap(), root.to_str().unwrap()]);
    assert_eq!(
        equal["revisions"],
        json!({"old": FINGERPRINT_REVISION, "new": FINGERPRINT_REVISION})
    );
    assert!(equal.get("excluded_keys").is_none());
    let mut old = current;
    old.as_object_mut().unwrap().remove("fingerprint_revision");
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), serde_json::to_vec(&old).unwrap()).unwrap();
    let gap = cli(&[
        "correspond",
        file.path().to_str().unwrap(),
        root.to_str().unwrap(),
    ]);
    assert_eq!(
        gap["revisions"],
        json!({"old": 0, "new": FINGERPRINT_REVISION})
    );
    assert!(
        gap["excluded_keys"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
    );
}
