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
    let mut current = recognise("prismatic.step");
    // A single feature isolates JSON compatibility from the intentionally reduced evidence
    // for distinguishing repeated features across an unknown historical vintage.
    current.features.truncate(1);
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
    let mut new = recognise("sealed_cylindrical_voids.step.gz");
    // Use one real cavity from this three-cavity part; all three have identical volume and
    // graph, so excluding their placement would deliberately leave them ambiguous.
    let id = new
        .features
        .iter()
        .find(|f| f.family == "interior_voids")
        .unwrap()
        .id
        .clone();
    new.features
        .retain(|f| f.family != "interior_voids" || f.id == id);
    let mut old = new.clone();
    let void = old
        .features
        .iter_mut()
        .find(|f| f.family == "interior_voids")
        .unwrap();
    assert_eq!(void.id, id);
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
fn excluded_semantics_can_still_supply_alignment_anchors() {
    use correspondence::FeatureFingerprint;
    let mut new = Fingerprints {
        fingerprint_version: correspondence::FINGERPRINT_VERSION.into(),
        fingerprint_revision: FINGERPRINT_REVISION,
        scale: 10.0,
        features: Vec::new(),
        faces: Vec::new(),
    };
    for (i, position) in [[0., 0., 0.], [10., 0., 0.], [0., 20., 0.]]
        .into_iter()
        .enumerate()
    {
        new.features.push(FeatureFingerprint {
            id: format!("pockets/{i}"),
            family: "pockets".into(),
            traits: [("rectangular_boundary".into(), "true".into())].into(),
            sizes: [("width".into(), 10.0 * (i + 1) as f64)].into(),
            position: Some(position),
            axis: Some([0., 0., 1.]),
            neighbourhood: Vec::new(),
            faces: Vec::new(),
        });
    }
    let mut old = new.clone();
    for f in &mut old.features {
        f.traits.remove("rectangular_boundary");
        f.position = f.position.map(|p| [p[0] + 3., p[1] + 4., p[2] + 5.]);
    }
    assert!(
        !correspondence::correspond(&old, &new)
            .unwrap()
            .alignment
            .found
    );
    old.fingerprint_revision = 0;
    let c = correspondence::correspond(&old, &new).unwrap();
    assert!(c.alignment.found);
    assert_eq!(c.alignment.inliers, 3);
    assert!(c.features.iter().all(|e| e.class == Class::Carried));
    assert_eq!(c.excluded_keys.len(), 1);
    assert_eq!(c.excluded_keys[0].key, "traits.rectangular_boundary");
}

#[test]
fn excluded_placement_and_neighbourhood_have_no_cost() {
    for family in ["bosses", "slot_patterns"] {
        let mut new = recognise("prismatic.step");
        new.features.truncate(1);
        new.features[0].family = family.into();
        if family == "bosses" {
            new.features[0].axis = Some([0., 0., 1.]);
            new.features[0].position = Some([1., 2., 3.]);
        }
        let mut old = new.clone();
        old.fingerprint_revision = 0;
        if family == "bosses" {
            // Only the component along the axis is excluded.
            old.features[0].position = Some([1., 2., 3000.]);
        }
        old.features[0].neighbourhood = vec!["obsolete-graph".into()];
        if family == "slot_patterns" {
            old.features[0].axis = Some([1., 0., 0.]);
            new.features[0].axis = Some([0., 1., 0.]);
        }
        let c = correspondence::correspond(&old, &new).unwrap();
        assert_eq!(c.features[0].class, Class::Carried);
        assert_eq!(c.features[0].cost, Some(0.0));
        if family == "bosses" {
            assert!(c.excluded_keys.iter().any(|e| e.key == "position.axial"));
        }
        assert!(c.excluded_keys.iter().any(|e| e.key == "neighbourhood"));
        if family == "slot_patterns" {
            assert!(c.excluded_keys.iter().any(|e| e.key == "axis"));
        }
    }
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

#[test]
fn axial_exclusion_keeps_the_perpendicular_position() {
    let mut new = recognise("prismatic.step");
    new.features.truncate(1);
    let f = &mut new.features[0];
    f.family = "holes".into();
    f.axis = Some([0., 0., 1.]);
    f.position = Some([10., 20., 30.]);
    f.neighbourhood.clear();
    let mut old = new.clone();
    old.fingerprint_revision = 0;
    old.features[0].position = Some([10., 20., 35.]);
    let c = correspondence::correspond(&old, &new).unwrap();
    assert_eq!(c.features[0].class, Class::Carried);
    assert!(
        c.excluded_keys
            .iter()
            .any(|e| e.family == "holes" && e.key == "position.axial")
    );
    old.features[0].position = Some([16., 20., 35.]);
    let c = correspondence::correspond(&old, &new).unwrap();
    assert_eq!(c.features[0].class, Class::Adapted);
    assert!(c.features[0].changes.contains_key("position"));
}

#[test]
fn every_value_change_key_names_a_fingerprint_field() {
    use correspondence::fingerprint::{FAMILIES, Role, VALUE_CHANGES};
    for change in VALUE_CHANGES {
        for family in change.families {
            let table = FAMILIES
                .iter()
                .find(|f| f.family == *family)
                .unwrap_or_else(|| panic!("unknown family {family}"));
            for key in change.keys {
                let stem = key.strip_suffix(".*").unwrap_or(key);
                let ok = match *key {
                    // Sizes computed by `derived_sizes`, not record fields.
                    "sizes.thickness" if *family == "plates" => true,
                    "position" | "position.axial" | "neighbourhood" => true,
                    "axis" => table.fields.iter().any(|(_, r)| matches!(r, Role::Axis)),
                    _ => {
                        let path = stem
                            .strip_prefix("traits.")
                            .or_else(|| stem.strip_prefix("sizes."))
                            .unwrap_or_else(|| panic!("{key} is not a fingerprint key"));
                        // `outer_steps.0.diameter` is the field path `outer_steps[].diameter`.
                        let path: Vec<&str> = path
                            .split('.')
                            .map(|s| {
                                if s.chars().all(|c| c.is_ascii_digit()) {
                                    "[]"
                                } else {
                                    s
                                }
                            })
                            .collect();
                        let path = path.join(".").replace(".[]", "[]");
                        table.fields.iter().any(|(p, _)| {
                            let p = p.to_string();
                            p == path
                                || p.starts_with(&format!("{path}."))
                                || p.starts_with(&format!("{path}["))
                        })
                    }
                };
                assert!(ok, "{family}: {key} matches no FAMILIES field");
            }
        }
    }
}

#[test]
fn value_changes_name_only_released_vintages() {
    // Every entry predates recorded revisions; none may cite unreleased code.
    for change in correspondence::fingerprint::VALUE_CHANGES {
        assert_eq!(change.revision, 0);
        assert!(
            !change.evidence.contains("Unreleased"),
            "{}",
            change.evidence
        );
    }
}

#[test]
fn excluded_keys_need_the_family_on_both_sides() {
    let mut new = recognise("prismatic.step");
    new.features.truncate(1);
    new.features[0].family = "plates".into();
    let mut old = new.clone();
    old.fingerprint_revision = 0;
    old.features[0].family = "holes".into();
    let c = correspondence::correspond(&old, &new).unwrap();
    assert!(c.excluded_keys.is_empty(), "{:?}", c.excluded_keys);
}

#[test]
fn a_part_against_itself_across_the_gap_is_all_carried() {
    for name in ["filleted_plate_with_holes.step", "prismatic.step"] {
        let new = recognise(name);
        let mut old = new.clone();
        old.fingerprint_revision = 0;
        let c = correspondence::correspond(&old, &new).unwrap();
        assert!(
            c.features.iter().all(|e| e.class == Class::Carried)
                && c.faces.iter().all(|e| e.class == Class::Carried),
            "{name}"
        );
    }
}
