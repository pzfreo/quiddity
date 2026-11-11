//! Local SFA handoff samples. This is deliberately separate from strict closure:
//! source requirements the existing writer refuses are recorded beside each sample.
#![cfg(feature = "json-import")]
use haecceity::{
    json::{self, Anchors},
    p21::Document,
    pmi::{self, PartId},
    step::read_part_definitions,
};
use std::{
    io::Read,
    path::{Path, PathBuf},
};
fn bytes(path: &Path) -> Vec<u8> {
    let raw = std::fs::read(path).unwrap();
    if path.extension().is_some_and(|e| e == "gz") {
        let mut b = vec![];
        flate2::read::GzDecoder::new(raw.as_slice())
            .read_to_end(&mut b)
            .unwrap();
        b
    } else {
        raw
    }
}
#[test]
#[ignore = "writes local SFA audit samples when HAECCEITY_JSON_SAMPLES is set; may use an explicitly recorded supported subset"]
fn write_sfa_samples_from_json() {
    let out =
        PathBuf::from(std::env::var_os("HAECCEITY_JSON_SAMPLES").expect("HAECCEITY_JSON_SAMPLES"));
    std::fs::create_dir_all(&out).unwrap();
    let nist = PathBuf::from(std::env::var_os("HAECCEITY_NIST_PMI").expect("HAECCEITY_NIST_PMI"));
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/ap242/specify/spool_fits.step.gz");
    for (name, path) in [
        ("CTC-01", nist.join("nist_ctc_01_asme1_ap242-e1.stp")),
        ("FTC-06", nist.join("nist_ftc_06_asme1_ap242-e2.stp")),
        ("corpus", fixture),
    ] {
        let input = bytes(&path);
        let doc = Document::parse(input.clone()).unwrap();
        let parts = read_part_definitions(&input).unwrap();
        let read = pmi::read(&doc, &parts).unwrap();
        let selected = read
            .parts
            .iter()
            .enumerate()
            .max_by_key(|(_, p)| p.dimensions.len() + p.tolerances.len())
            .map(|(i, _)| i)
            .unwrap();
        let geometry = haecceity::step::read_part(&input, &parts[selected]).unwrap();
        let (clean, geometry_report) = haecceity::brep_write::write_step(
            &geometry,
            &haecceity::brep_write::WriteOptions::new(name).with_pmi(None),
        )
        .unwrap();
        assert!(geometry_report.face_order_preserved);
        let parts = read_part_definitions(&clean).unwrap();
        assert_eq!(parts.len(), 1);
        let doc = Document::parse(clean).unwrap();
        let mut models = vec![read.parts[selected].clone()];
        let mut refused = vec![];
        for attempt in 0..16 {
            let submitted: Vec<_> = models
                .iter()
                .enumerate()
                .map(|(i, p)| (PartId(i), p.clone()))
                .collect();
            match pmi::write(
                &doc,
                &parts,
                &submitted,
                pmi::Mode::Replace,
                pmi::write::PresentationPolicy::RemovePresentation,
            ) {
                Err(pmi::write::WriteError::Refused(list)) => {
                    assert!(attempt < 15, "refusals did not settle");
                    for r in &list {
                        refused.push(serde_json::json!({"part":r.part.0,"item":format!("{:?}",r.item),"why":r.why}));
                    }
                    for (i, p) in models.iter_mut().enumerate() {
                        let items: Vec<_> = list
                            .iter()
                            .filter(|r| r.part.0 == i)
                            .map(|r| r.item)
                            .collect();
                        if !items.is_empty() {
                            *p = pmi::without(p, &items);
                        }
                    }
                }
                Err(e) => panic!("{name}: {e}"),
                Ok(_) => break,
            }
        }
        let mut semantic = vec![];
        let mut wire = vec![];
        for (i, p) in models.iter().enumerate() {
            let value = json::to_value(&json::identify(p.clone(), None).unwrap()).unwrap();
            let decoded = json::from_value(&value, &Anchors::read(&doc, &parts[i])).unwrap();
            wire.push(value);
            semantic.push((PartId(i), decoded));
        }
        let result = json::write(
            &doc,
            &parts,
            &semantic,
            pmi::write::PresentationPolicy::RemovePresentation,
        )
        .and_then(|(bytes, report)| {
            let candidate = Document::parse(bytes.clone()).unwrap();
            if !haecceity::express::validate_document(candidate.graph()).is_empty()
                || !haecceity::express_rules::check_all(&candidate).is_empty()
            {
                return Err(json::Error(
                    "the full subset needs the existing writer's practice/schema exceptions".into(),
                ));
            }
            Ok((bytes, report))
        });
        let (written, report) = match result {
            Ok(result) => result,
            Err(e) => {
                refused.push(serde_json::json!({"scope":"all but one source dimension","why":format!("full identified subset refused: {e}")}));
                let original = &read.parts[selected];
                models = vec![one_source_dimension(original)];
                let value =
                    json::to_value(&json::identify(models[0].clone(), None).unwrap()).unwrap();
                let decoded = json::from_value(&value, &Anchors::read(&doc, &parts[0])).unwrap();
                wire = vec![value];
                json::write(
                    &doc,
                    &parts,
                    &[(PartId(0), decoded)],
                    pmi::write::PresentationPolicy::RemovePresentation,
                )
                .unwrap()
            }
        };
        let after = Document::parse(written.clone()).unwrap();
        let defs = read_part_definitions(&written).unwrap();
        let reread = pmi::read(&after, &defs).unwrap();
        assert!(
            haecceity::express::validate_document(after.graph()).is_empty(),
            "{name}: full EXPRESS validation"
        );
        assert!(
            haecceity::express_rules::check_all(&after).is_empty(),
            "{name}: full rule validation"
        );
        for def in &defs {
            let part = haecceity::step::read_part(&written, def).unwrap();
            for solid in 0..part.solids.len() {
                assert!(part.solid_is_valid(solid), "{name}: topology");
                assert!(
                    part.solid_is_geometrically_valid(solid),
                    "{name}: geometric validity"
                );
            }
        }

        for (i, p) in models.iter().enumerate() {
            let expected = p.clone();
            let differences = json::identity_differences(&expected, &reread.parts[i]);
            assert!(differences.is_empty(), "{name}: {differences:?}");
        }
        std::fs::write(out.join(format!("{name}.step")), written).unwrap();
        std::fs::write(
            out.join(format!("{name}.pmi.json")),
            serde_json::to_vec_pretty(&wire[0]).unwrap(),
        )
        .unwrap();
        let audit = serde_json::json!({"source":path,"selected_source_part":selected,"geometry":"clean AP242 B-rep copy with preserved face order","complete_source_roundtrip":false,"legacy_note_migration":"none: JSON retains Note items","corpus_geometry_source":if name == "corpus" { Some("cadgenbench/flanged_spool_132.step") } else { None },"refused":refused,"parts":models.len(),"source_parts":read.parts.len(),"source_dimensions":read.parts[selected].dimensions.len(),"sample_dimensions":models[0].dimensions.len(),"source_tolerances":read.parts[selected].tolerances.len(),"sample_tolerances":models[0].tolerances.len(),"schema":report.validated_against,"differences":"empty against explicitly recorded supported subset"});
        std::fs::write(
            out.join(format!("{name}.audit.json")),
            serde_json::to_vec_pretty(&audit).unwrap(),
        )
        .unwrap();
        eprintln!(
            "{name}: JSON-written STEP sample; {} explicit source-item refusals recorded",
            refused.len()
        );
    }
}

fn one_source_dimension(original: &pmi::PartPmi) -> pmi::PartPmi {
    use pmi::{DimensionKind, Feature, FeatureId};
    use std::collections::BTreeMap;
    fn feature(
        old: FeatureId,
        original: &pmi::PartPmi,
        output: &mut pmi::PartPmi,
        map: &mut BTreeMap<usize, FeatureId>,
    ) -> FeatureId {
        if let Some(id) = map.get(&old.0) {
            return *id;
        }
        let value = match &original.features[old.0] {
            Feature::Items(a) => Feature::Items(a.clone()),
            Feature::Group { members, kind } => Feature::Group {
                members: members
                    .iter()
                    .map(|f| feature(*f, original, output, map))
                    .collect(),
                kind: kind.clone(),
            },
            Feature::Derived {
                kind,
                from,
                geometry,
            } => Feature::Derived {
                kind: *kind,
                from: from
                    .iter()
                    .map(|f| feature(*f, original, output, map))
                    .collect(),
                geometry: *geometry,
            },
        };
        let existing = output.features.iter().position(|f| match (f, &value) {
            (Feature::Items(a), Feature::Items(b)) => {
                a.iter().copied().collect::<std::collections::BTreeSet<_>>()
                    == b.iter().copied().collect()
            }
            _ => f == &value,
        });
        let id = if let Some(i) = existing {
            FeatureId(i)
        } else {
            let i = FeatureId(output.features.len());
            output.features.push(value);
            i
        };
        map.insert(old.0, id);
        id
    }
    let mut output = pmi::PartPmi {
        geometry: original.geometry.clone(),
        derived_geometry: original.derived_geometry.clone(),
        ..pmi::PartPmi::default()
    };
    let mut dimension = original
        .dimensions
        .iter()
        .find(|d| d.nominal.is_some())
        .expect("sample has a stated source dimension")
        .clone();
    let mut map = BTreeMap::new();
    match &mut dimension.kind {
        DimensionKind::Size {
            feature: f, path, ..
        } => {
            *f = feature(*f, original, &mut output, &mut map);
            if let Some(p) = path {
                *p = feature(*p, original, &mut output, &mut map);
            }
        }
        DimensionKind::Location { from, to, path, .. } => {
            *from = feature(*from, original, &mut output, &mut map);
            *to = feature(*to, original, &mut output, &mut map);
            if let Some(p) = path {
                *p = feature(*p, original, &mut output, &mut map);
            }
        }
    }
    output.dimensions.push(dimension);
    // This sample is the support closure of one source dimension, rather than
    // the original part's unrelated supplemental/computed geometry caches.
    let old_derived = std::mem::take(&mut output.derived_geometry);
    let needed: std::collections::BTreeSet<_> = output
        .features
        .iter()
        .filter_map(|f| match f {
            Feature::Derived {
                geometry: Some(g), ..
            } => Some(g.0),
            _ => None,
        })
        .collect();
    let map: BTreeMap<_, _> = needed
        .iter()
        .enumerate()
        .map(|(new, old)| (*old, new))
        .collect();
    output.derived_geometry = needed.iter().map(|i| old_derived[*i].clone()).collect();
    for feature in &mut output.features {
        if let Feature::Derived {
            geometry: Some(g), ..
        } = feature
        {
            g.0 = map[&g.0];
        }
    }
    let old_geometry = std::mem::take(&mut output.geometry);
    let mut needed = std::collections::BTreeSet::new();
    for feature in &output.features {
        if let Feature::Items(items) = feature {
            for a in items {
                if let pmi::Anchor::Geometry(g) = a {
                    needed.insert(g.0);
                }
            }
        }
    }
    for g in &output.derived_geometry {
        needed.extend(g.supplemental.iter().map(|g| g.0));
    }
    let map: BTreeMap<_, _> = needed
        .iter()
        .enumerate()
        .map(|(new, old)| (*old, new))
        .collect();
    output.geometry = needed.iter().map(|i| old_geometry[*i].clone()).collect();
    for feature in &mut output.features {
        if let Feature::Items(items) = feature {
            for a in items {
                if let pmi::Anchor::Geometry(g) = a {
                    g.0 = map[&g.0];
                }
            }
        }
    }
    for g in &mut output.derived_geometry {
        for g in &mut g.supplemental {
            g.0 = map[&g.0];
        }
    }
    output
}
