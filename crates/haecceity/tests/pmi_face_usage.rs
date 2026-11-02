//! Written-file regression for CAx-IF PMI v4.1 §5.1 / §6.5.1–2.
mod common;
use haecceity::{
    p21::Document,
    pmi::{
        self, PartId,
        write::{Mode, PresentationPolicy},
    },
    step::read_part_definitions,
};
use std::{
    io::Read,
    path::{Path, PathBuf},
};
fn bytes(path: &Path) -> Vec<u8> {
    let b = std::fs::read(path).unwrap();
    if path.extension().is_some_and(|e| e == "gz") {
        let mut out = vec![];
        flate2::read::GzDecoder::new(b.as_slice())
            .read_to_end(&mut out)
            .unwrap();
        out
    } else {
        b
    }
}
fn cases() -> Vec<PathBuf> {
    let mut out = vec![];
    for dir in [
        common::fixtures().join("ap242/specify"),
        common::fixtures().join("ap242/nist"),
        common::fixtures().join("ap242/presentation-placement"),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xcaf"),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/presentation/thread-callouts"),
    ] {
        out.extend(
            std::fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| {
                    p.to_string_lossy().ends_with(".step.gz")
                        || p.to_string_lossy().ends_with(".stp.gz")
                }),
        );
    }
    if let Some(root) = common::corpus_dir() {
        out.extend(
            std::fs::read_dir(root.join("nist"))
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p.extension().is_some_and(|e| e == "stp")),
        );
    } else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
    }
    if let Some(root) = std::env::var_os("HAECCEITY_NIST_PMI") {
        out.extend(
            std::fs::read_dir(root)
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p.extension().is_some_and(|e| e == "stp")),
        );
    }
    for name in ["30-cadgen_pmi_plate.step", "31-xcaf_pmi_plate.step"] {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(name);
        if p.exists() {
            out.push(p);
        }
    }
    out.sort();
    out.dedup();
    out
}
fn check(name: &str, doc: &Document, problems: &mut Vec<String>) {
    let v = common::pmi_usage::violations(doc);
    eprintln!("{name}: {} uniqueness/forwarding violations", v.len());
    problems.extend(v.into_iter().map(|v| format!("{name}: {v}")));
    if let Some(dir) = std::env::var_os("HAECCEITY_USAGE_OUTPUT") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Path::new(&dir).join(format!("{name}.step")), doc.bytes()).unwrap();
    }
}
#[test]
fn written_pmi_reuses_each_faces_one_aspect_and_usage() {
    let mut problems = vec![];
    for path in cases() {
        let name = path.file_name().unwrap().to_string_lossy();
        let b = bytes(&path);
        let parts = read_part_definitions(&b).unwrap();
        let doc = Document::parse(b).unwrap();
        let read = pmi::read(&doc, &parts).unwrap();
        // Match pmi_roundtrip's established refusal projection; never suppress linking
        // failures. Missing nominals, hole authoring and ambiguous UDA units are unrelated.
        let mut expected = read.parts.clone();
        let mut edit = None;
        for _ in 0..8 {
            let models: Vec<_> = expected
                .iter()
                .enumerate()
                .map(|(i, p)| (PartId(i), p.clone()))
                .collect();
            match pmi::write(
                &doc,
                &parts,
                &models,
                Mode::Replace,
                PresentationPolicy::RemovePresentation,
            ) {
                Ok((e, _)) => {
                    edit = Some(e);
                    break;
                }
                Err(pmi::write::WriteError::Refused(refusals)) => {
                    eprintln!(
                        "{name}: projecting {} existing authoring refusals",
                        refusals.len()
                    );
                    for (i, p) in expected.iter_mut().enumerate() {
                        let items: Vec<_> = refusals
                            .iter()
                            .filter(|r| r.part == PartId(i))
                            .map(|r| r.item)
                            .collect();
                        *p = pmi::without(p, &items);
                    }
                }
                Err(e) => {
                    problems.push(format!("{name}: {e}"));
                    eprintln!("{name}: write failed: {e}");
                    break;
                }
            }
        }
        let Some(edit) = edit else {
            assert!(
                problems.iter().any(|p| p.starts_with(name.as_ref())),
                "{name}: authoring refusals did not settle"
            );
            continue;
        };
        let output = doc.apply(&edit).unwrap().bytes;
        let after_parts = read_part_definitions(&output).unwrap();
        let after = Document::parse(output).unwrap();
        let back = pmi::read(&after, &after_parts).unwrap();
        for (i, p) in expected.iter().enumerate() {
            let d =
                pmi::write::differences_as_stated(&pmi::write::standard_form(p), &back.parts[i]);
            assert!(d.is_empty(), "{name}: semantic change: {d:?}");
        }
        check(&name, &after, &mut problems);
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
#[cfg(feature = "json-import")]
#[test]
fn plate_pmi_json_reuses_the_datum_feature() {
    use haecceity::json::{self, Anchors};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xcaf");
    let b = bytes(&root.join("pmi_plate.geom.step.gz"));
    let parts = read_part_definitions(&b).unwrap();
    let doc = Document::parse(b).unwrap();
    let wire =
        serde_json::from_slice(&std::fs::read(root.join("pmi_plate.pmi.json")).unwrap()).unwrap();
    let semantic = json::from_value(&wire, &Anchors::read(&doc, &parts[0])).unwrap();
    let expected = semantic.pmi.clone();
    let (output, _) = json::write(
        &doc,
        &parts,
        &[(PartId(0), semantic)],
        PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let after_parts = read_part_definitions(&output).unwrap();
    let after = Document::parse(output).unwrap();
    let back = pmi::read(&after, &after_parts).unwrap();
    assert!(json::identity_differences(&expected, &back.parts[0]).is_empty());
    let mut problems = vec![];
    check("plate-pmi-json", &after, &mut problems);
    assert!(problems.is_empty(), "{problems:?}");

    // Recreate the old JSON writer's forwarding endpoint without changing its ordering
    // record. Restoring equal-feature aliases must retain all consumed native provenance.
    use haecceity::p21::{self, Attribute as A, Edit, RawEntity};
    let loc = after
        .ids()
        .find(|id| common::pmi_usage::is_a(&after, *id, "dimensional_location"))
        .unwrap();
    let owner = common::pmi_usage::reference(
        &after,
        loc,
        "shape_aspect_relationship",
        "relating_shape_aspect",
    )
    .unwrap();
    assert!(common::pmi_usage::is_a(&after, owner, "datum_feature"));
    let shape = common::pmi_usage::reference(&after, owner, "shape_aspect", "of_shape").unwrap();
    let mut edit = Edit::new();
    let alias = edit.add(p21::simple(
        "SHAPE_ASPECT",
        vec![
            A::String("DIMENSION".into()),
            A::String("".into()),
            A::EntityRef(shape),
            A::Enum("T".into()),
        ],
    ));
    edit.add(p21::simple(
        "SHAPE_ASPECT_RELATIONSHIP",
        vec![
            A::String("".into()),
            A::String("".into()),
            alias.to_ref(),
            A::EntityRef(owner),
        ],
    ));
    let mut record = after.get(loc).unwrap().clone();
    let RawEntity::Simple { attributes, .. } = &mut record else {
        panic!("plain location")
    };
    attributes[2] = alias.to_ref();
    edit.replace(loc, record);
    let legacy = Document::parse(after.apply(&edit).unwrap().bytes).unwrap();
    assert_eq!(common::pmi_usage::violations(&legacy).len(), 1);
    let legacy_read = pmi::read(&legacy, &after_parts).unwrap();
    assert_eq!(
        legacy_read.parts, back.parts,
        "legacy JSON semantics are unchanged"
    );
    let models: Vec<_> = legacy_read
        .parts
        .into_iter()
        .enumerate()
        .map(|(i, p)| (PartId(i), p))
        .collect();
    let (edit, _) = pmi::write(
        &legacy,
        &after_parts,
        &models,
        Mode::Replace,
        PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let migrated = Document::parse(legacy.apply(&edit).unwrap().bytes).unwrap();
    assert!(common::pmi_usage::violations(&migrated).is_empty());
}

#[test]
fn legacy_faceless_location_reads_as_the_same_datum_feature() {
    use common::pmi_usage::{reference, violations};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xcaf");
    let b = bytes(&root.join("plate-legacy-datum-location.step.gz"));
    let parts = read_part_definitions(&b).unwrap();
    let doc = Document::parse(b).unwrap();
    assert_eq!(
        violations(&doc).len(),
        1,
        "checker must catch the legacy forwarding aspect"
    );
    let old = pmi::read(&doc, &parts).unwrap();
    let p = old.parts.iter().find(|p| !p.dimensions.is_empty()).unwrap();
    let pmi::DimensionKind::Location { from, .. } = p.dimensions[0].kind else {
        panic!("location")
    };
    assert_eq!(
        p.features[p.datums[0].feature().unwrap().0],
        p.features[from.0]
    );
    assert_eq!(
        p.features[from.0],
        pmi::Feature::Items(vec![pmi::Anchor::Face(pmi::FaceIndex(
            parts
                .iter()
                .find(|p| p.faces.contains(&360))
                .unwrap()
                .faces
                .iter()
                .position(|id| *id == 360)
                .unwrap()
        ))])
    );
    let models: Vec<_> = old
        .parts
        .iter()
        .enumerate()
        .map(|(i, p)| (PartId(i), p.clone()))
        .collect();
    let (edit, _) = pmi::write(
        &doc,
        &parts,
        &models,
        Mode::Replace,
        PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let after = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
    assert!(violations(&after).is_empty());
    let loc = after
        .ids()
        .find(|id| common::pmi_usage::is_a(&after, *id, "dimensional_location"))
        .unwrap();
    let datum = reference(
        &after,
        loc,
        "shape_aspect_relationship",
        "relating_shape_aspect",
    )
    .unwrap();
    assert!(common::pmi_usage::is_a(&after, datum, "datum_feature"));
}

#[test]
fn uniqueness_check_catches_gisu_and_set_iiru_duplicate_ownership() {
    use haecceity::p21::{self, Attribute as A, Edit};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xcaf");
    let doc = Document::parse(bytes(&root.join("plate-legacy-datum-location.step.gz"))).unwrap();
    for kind in [
        "GEOMETRIC_ITEM_SPECIFIC_USAGE",
        "ITEM_IDENTIFIED_REPRESENTATION_USAGE",
    ] {
        let mut edit = Edit::new();
        let aspect = edit.add(p21::simple(
            "SHAPE_ASPECT",
            vec![
                A::String("duplicate".into()),
                A::String("".into()),
                A::EntityRef(26),
                A::Enum("T".into()),
            ],
        ));
        let face = A::EntityRef(360);
        let item = if kind == "GEOMETRIC_ITEM_SPECIFIC_USAGE" {
            face
        } else {
            A::Typed {
                type_name: "SET_REPRESENTATION_ITEM".into(),
                value: Box::new(A::List(vec![face])),
            }
        };
        edit.add(p21::simple(
            kind,
            vec![
                A::String("".into()),
                A::String("".into()),
                aspect.to_ref(),
                A::EntityRef(32),
                item,
            ],
        ));
        let doc = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
        let violations = common::pmi_usage::violations(&doc);
        assert_eq!(violations.len(), 2, "{kind}: {violations:?}");
        assert!(
            violations
                .iter()
                .any(|v| v.starts_with("face #360 identified"))
        );
    }
}
