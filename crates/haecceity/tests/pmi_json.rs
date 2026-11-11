#![cfg(feature = "json-import")]
use haecceity::{
    json::{self, Anchors},
    p21::Document,
    pmi::{self, *},
    step::read_part_definitions,
};
use serde_json::json;
fn cube() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/ties/cube.step"
    ))
    .unwrap()
}
fn test_model() -> PartPmi {
    PartPmi {
        features: vec![Feature::Items(vec![Anchor::Face(FaceIndex(0))])],
        datums: vec![
            Datum::new(DatumLabel::new("A").unwrap(), Some(FeatureId(0)), vec![]).unwrap(),
        ],
        dimensions: vec![Dimension {
            kind: DimensionKind::Size {
                feature: FeatureId(0),
                kind: SizeKind::Thickness,
                path: None,
                angle: None,
            },
            nominal: Some(Value::length(
                Decimal::parse("10.000").unwrap(),
                LengthUnit::Millimetre,
            )),
            tolerance: DimTolerance::Basic,
            qualifier: None,
            modifiers: vec![],
            principle: None,
        }],
        ..PartPmi::default()
    }
}
fn anchors() -> Anchors {
    Anchors {
        names: vec![Some("top".into())],
        fingerprints: vec![Some("support-key".into())],
        edge_count: 0,
    }
}
#[test]
fn all_anchor_forms_and_precise_units() {
    let p = test_model();
    let semantic = json::identify(p.clone(), None).unwrap();
    let v = json::to_value(&semantic).unwrap();
    assert_eq!(v["units"]["length"][0]["value"], json!("millimetre"));
    for face in [
        json!(0),
        json!({"name":"top"}),
        json!({"fingerprint":"support-key"}),
    ] {
        let mut v = v.clone();
        v["pmi"]["features"][0]["value"]["items"][0]["face"] = face;
        let decoded = json::from_value(&v, &anchors()).unwrap();
        assert_eq!(p, decoded.pmi);
        assert_eq!(json::to_value(&decoded).unwrap(), v);
        assert_eq!(
            v["pmi"]["dimensions"][0]["value"]["nominal"]["quantity"]["length"]["value"],
            "10.000"
        );
    }
}
#[test]
fn refusals_are_explained() {
    let base = json::to_value(&json::identify(test_model(), None).unwrap()).unwrap();
    for (path, value, reason) in [
        ("/schema", json!("haecceity.pmi/2"), "schema violation"),
        (
            "/pmi/features/0/value/items/0/face",
            json!({"name":"absent"}),
            "unknown face name",
        ),
        (
            "/pmi/features/0/value/items/0/face",
            json!(1),
            "unknown face index",
        ),
        (
            "/pmi/datums/0/id",
            base["pmi"]["dimensions"][0]["id"].clone(),
            "duplicate item id",
        ),
        (
            "/pmi/dimensions/0/value/nominal/quantity/length/value",
            json!(1.25),
            "schema violation",
        ),
        (
            "/pmi/dimensions/0/value/nominal/quantity/length/unit",
            json!("absent unit"),
            "undefined length unit",
        ),
    ] {
        let mut v = base.clone();
        *v.pointer_mut(path).unwrap() = value;
        let e = json::from_value(&v, &anchors()).unwrap_err().to_string();
        assert!(e.contains(reason), "{e}");
    }
    let mut v = base.clone();
    v["pmi"]["bogus"] = json!(true);
    assert!(
        json::validate_schema(&v)
            .unwrap_err()
            .to_string()
            .contains("unknown property")
    );
}
#[test]
fn caller_ids_survive_step_write() {
    let bytes = cube();
    let doc = Document::parse(bytes.clone()).unwrap();
    let parts = read_part_definitions(&bytes).unwrap();
    let mut p = json::identify(test_model(), None).unwrap();
    p.ids.get_mut("dimensions").unwrap()[0] = "inspection-26".into();
    let (written, _) = json::write(
        &doc,
        &parts,
        &[(PartId(0), p)],
        pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let doc = Document::parse(written.clone()).unwrap();
    let parts = read_part_definitions(&written).unwrap();
    let read = pmi::read(&doc, &parts).unwrap();
    assert_eq!(
        read.identifiers.parts[0].dimensions,
        vec![Some("inspection-26".into())]
    );
    assert!(json::identity_differences(&test_model(), &read.parts[0]).is_empty());
    let reread = pmi::read(&doc, &parts).unwrap();
    assert_eq!(read.identifiers, reread.identifiers);
}

#[test]
fn undefined_datums_and_disallowed_feature_roles() {
    let mut p = test_model();
    p.tolerances.push(GeometricTolerance {
        kind: ToleranceKind::Position,
        target: ToleranceTarget::Feature(FeatureId(0)),
        magnitude: None,
        zone: None,
        modifiers: vec![],
        unit_basis: None,
        maximum: None,
        unequal: None,
        datums: Some(
            DatumSystem::new(vec![Compartment {
                references: vec![DatumReference {
                    datum: DatumId(0),
                    modifiers: vec![],
                }],
                modifiers: vec![],
            }])
            .unwrap(),
        ),
        auxiliary: vec![],
        description: None,
    });
    let mut v = json::to_value(&json::identify(p, None).unwrap()).unwrap();
    v["pmi"]["tolerances"][0]["value"]["datums"][0]["references"][0]["datum"] =
        json!("absent datum");
    assert!(
        json::from_value(&v, &anchors())
            .unwrap_err()
            .to_string()
            .contains("unknown datums id")
    );
    let mut p = test_model();
    p.features.push(Feature::Derived {
        kind: DerivedKind::Apex,
        from: vec![FeatureId(0)],
        geometry: None,
    });
    p.datums = vec![Datum::new(DatumLabel::new("A").unwrap(), Some(FeatureId(1)), vec![]).unwrap()];
    let v = json::to_value(&json::identify(p, None).unwrap()).unwrap();
    assert!(
        json::from_value(&v, &anchors())
            .unwrap_err()
            .to_string()
            .contains("kind not allowed")
    );
}

#[path = "common/pmi_gaps.rs"]
mod gaps;
#[test]
fn full_model_and_schema_reference_closure() {
    let p = gaps::every_kind();
    let a = Anchors {
        names: vec![None; 2],
        fingerprints: vec![],
        edge_count: 0,
    };
    let encoded = json::to_value(&json::identify(p.clone(), None).unwrap()).unwrap();
    assert_eq!(json::from_value(&encoded, &a).unwrap().pmi, p);
    let s: serde_json::Value = serde_json::from_str(json::PMI_SCHEMA).unwrap();
    fn refs(v: &serde_json::Value, root: &serde_json::Value) {
        if let Some(r) = v["$ref"].as_str() {
            assert!(root.pointer(r.strip_prefix('#').unwrap()).is_some(), "{r}");
        }
        match v {
            serde_json::Value::Object(m) => m.values().for_each(|v| refs(v, root)),
            serde_json::Value::Array(a) => a.iter().for_each(|v| refs(v, root)),
            _ => {}
        }
    }
    refs(&s, &s);
}

#[test]
fn specify_face_fingerprint_support_key() {
    let mut a = anchors();
    a.bind_support(2, "verified-face-hash").unwrap();
    let mut v = json::to_value(&json::identify(test_model(), None).unwrap()).unwrap();
    v["pmi"]["features"][0]["value"]["items"][0]["face"] =
        json!({"fingerprint":{"schema":2,"faces":"verified-face-hash","support":[0]}});
    assert_eq!(json::from_value(&v, &a).unwrap().pmi, test_model());
    v["pmi"]["features"][0]["value"]["items"][0]["face"]["fingerprint"]["faces"] =
        json!("another-part");
    assert!(
        json::from_value(&v, &a)
            .unwrap_err()
            .to_string()
            .contains("unknown face fingerprint")
    );
}

#[test]
fn text_parser_refuses_duplicates_at_every_depth() {
    for text in [
        r#"{"schema":"haecceity.pmi/1","schema":"haecceity.pmi/1"}"#,
        r#"{"pmi":{"features":[],"features":[]}}"#,
    ] {
        assert!(
            json::parse(text)
                .unwrap_err()
                .to_string()
                .contains("duplicate key")
        );
    }
    assert!(json::parse("{} trailing").is_err());
    let model = json::identify(test_model(), None).unwrap();
    assert_eq!(
        json::from_str(&json::to_string(&model).unwrap(), &anchors())
            .unwrap()
            .pmi,
        model.pmi
    );
}

#[test]
fn named_step_faces_use_mesh_face_order_and_keep_equivalent_native_pmi() {
    use haecceity::p21::{self, Attribute, RawEntity};
    let doc = Document::parse(cube()).unwrap();
    let parts = read_part_definitions(doc.bytes()).unwrap();
    let mut face = doc.get(parts[0].faces[0]).unwrap().clone();
    if let RawEntity::Simple { attributes, .. } = &mut face {
        attributes[0] = Attribute::String("OCCT named top".into());
    } else {
        panic!("cube face is simple");
    }
    let mut edit = p21::Edit::new();
    edit.replace(parts[0].faces[0], face);
    let bytes = doc.apply(&edit).unwrap().bytes;
    let doc = Document::parse(bytes.clone()).unwrap();
    let parts = read_part_definitions(&bytes).unwrap();
    let mesh_part = haecceity::step::read_part(&bytes, &parts[0]).unwrap();
    assert_eq!(mesh_part.faces.len(), parts[0].faces.len());
    for (index, entity) in parts[0].faces.iter().enumerate() {
        assert_eq!(mesh_part.face_source(index).unwrap().entity, *entity);
    }
    let mut anchors = Anchors::read(&doc, &parts[0]);
    assert_eq!(anchors.names[0].as_deref(), Some("OCCT named top"));
    anchors.bind_support(2, "verified").unwrap();
    let v = json::to_value(&json::identify(test_model(), None).unwrap()).unwrap();
    let mut outputs = vec![];
    for face in [
        json!(0),
        json!({"name":"OCCT named top"}),
        json!({"fingerprint":{"schema":2,"faces":"verified","support":[0]}}),
    ] {
        let mut v = v.clone();
        v["pmi"]["features"][0]["value"]["items"][0]["face"] = face;
        let decoded = json::from_value(&v, &anchors).unwrap();
        let (output, _) = json::write(
            &doc,
            &parts,
            &[(PartId(0), decoded)],
            pmi::write::PresentationPolicy::RemovePresentation,
        )
        .unwrap();
        let after = Document::parse(output).unwrap();
        let definitions = read_part_definitions(after.bytes()).unwrap();
        assert_eq!(definitions[0].faces, parts[0].faces);
        let read = pmi::read(&after, &definitions).unwrap();
        let semantic =
            json::identify(read.parts[0].clone(), Some(&read.identifiers.parts[0])).unwrap();
        assert_eq!(json::to_value(&semantic).unwrap(), v);
        // Wire-form metadata differs intentionally; authoritative native PMI and
        // its actual faces are identical in all three cases.
        outputs.push(pmi::read_native(&after, &definitions).unwrap().parts);
    }
    assert_eq!(outputs[0], outputs[1]);
    assert_eq!(outputs[0], outputs[2]);
}

#[test]
fn every_written_kind_keeps_its_caller_id_through_migration() {
    use std::collections::BTreeSet;
    let bytes = cube();
    let doc = Document::parse(bytes.clone()).unwrap();
    let parts = read_part_definitions(&bytes).unwrap();
    let input = json::identify(gaps::every_kind(), None).unwrap();
    let expected: BTreeSet<_> = input.ids.values().flatten().cloned().collect();
    let (written, _) = json::write(
        &doc,
        &parts,
        &[(PartId(0), input)],
        pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let defs = read_part_definitions(&written).unwrap();
    let read = pmi::read(&Document::parse(written).unwrap(), &defs).unwrap();
    let output = json::identify(read.parts[0].clone(), Some(&read.identifiers.parts[0])).unwrap();
    let actual: BTreeSet<_> = output.ids.values().flatten().cloned().collect();
    assert!(
        expected.is_subset(&actual),
        "caller ids lost: {:?}",
        expected.difference(&actual).collect::<Vec<_>>()
    );
    // Support aspects stay structural; independent requirement ids survive migration.
    let again = json::identify(read.parts[0].clone(), Some(&read.identifiers.parts[0])).unwrap();
    assert_eq!(output.ids, again.ids);
}

#[test]
fn read_only_extensions_mixed_units_and_iso_classes_are_lossless() {
    let mut p = test_model();
    let length = |text: &str| Length {
        value: Decimal::parse(text).unwrap(),
        unit: LengthUnit::Foot,
    };
    let placement = Placement {
        origin: [length("0.000"), length("1.25"), length("-2.5")],
        axis: None,
        ref_direction: None,
    };
    p.holes.push(Hole {
        feature: FeatureId(0),
        definition: RoundHole {
            name: "source hole".into(),
            diameter: length("0.5"),
            diameter_tolerance: None,
            depth: Some(length("1.0")),
            depth_tolerance: None,
            through: false,
            placement: placement.clone(),
        },
        placement: Some(placement),
    });
    p.general.push(GeneralTolerance::Class {
        text: "ISO 2768-mK".into(),
        standard: standards::Iso2768::recognise("ISO 2768-mK"),
    });
    p.attributes.push(AttributeSet {
        name: "custom units".into(),
        on: None,
        description: None,
        general_property: None,
        items: vec![
            (
                "length".into(),
                AttributeValue::Measure(Value::length(
                    Decimal::parse("0.2500").unwrap(),
                    LengthUnit::Other {
                        name: "custom length".into(),
                        metres: Decimal::parse("0.0025").unwrap(),
                    },
                )),
            ),
            (
                "angle".into(),
                AttributeValue::Measure(Value::angle(
                    Decimal::parse("12.3400").unwrap(),
                    AngleUnit::Other {
                        name: "custom angle".into(),
                        radians: Decimal::parse("0.01").unwrap(),
                    },
                )),
            ),
        ],
    });
    let v = json::to_value(&json::identify(p.clone(), None).unwrap()).unwrap();
    assert_eq!(json::from_value(&v, &anchors()).unwrap().pmi, p);
}

#[test]
fn structural_support_aliases_do_not_duplicate_requirement_ids() {
    let input = cube();
    let doc = Document::parse(input.clone()).unwrap();
    let parts = read_part_definitions(&input).unwrap();
    let mut p = test_model();
    p.features.push(p.features[0].clone());
    let semantic = json::identify(p.clone(), None).unwrap();
    let (written, _) = json::write(
        &doc,
        &parts,
        &[(PartId(0), semantic)],
        pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let definitions = read_part_definitions(&written).unwrap();
    let read = pmi::read(&Document::parse(written).unwrap(), &definitions).unwrap();
    assert!(json::identity_differences(&p, &read.parts[0]).is_empty());
    assert_eq!(
        read.identifiers.parts[0].dimensions,
        json::identify(p.clone(), None).unwrap().ids["dimensions"]
            .iter()
            .cloned()
            .map(Some)
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_target_and_its_support_aspect_share_one_file_identifier() {
    use std::io::Read;
    let raw = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/ap242/nist/nist_ftc_10_asme1_ap242-e2.stp.gz"
    ))
    .unwrap();
    let mut input = vec![];
    flate2::read::GzDecoder::new(raw.as_slice())
        .read_to_end(&mut input)
        .unwrap();
    let doc = Document::parse(input.clone()).unwrap();
    let parts = read_part_definitions(&input).unwrap();
    let read = pmi::read(&doc, &parts).unwrap();
    let semantic = json::identify(read.parts[0].clone(), Some(&read.identifiers.parts[0])).unwrap();
    let encoded = json::to_value(&semantic).unwrap();
    assert_eq!(
        json::from_value(&encoded, &Anchors::read(&doc, &parts[0]))
            .unwrap()
            .pmi,
        read.parts[0]
    );
    assert!(
        semantic.ids["datum_targets"]
            .iter()
            .any(|id| id == "DT #18373 on DATUM #18319")
    );
}

#[test]
fn json_schema_integer_values_can_have_a_decimal_point() {
    let mut a = anchors();
    a.bind_support(2, "verified").unwrap();
    let mut expected = test_model();
    expected.decimal_places = Some(3);
    let mut value = json::to_value(&json::identify(expected.clone(), None).unwrap()).unwrap();
    value["pmi"]["decimal_places"] = json!(3.0);
    for face in [
        json!(0.0),
        json!({"fingerprint":{"schema":2.0,"faces":"verified","support":[0.0]}}),
    ] {
        value["pmi"]["features"][0]["value"]["items"][0]["face"] = face;
        json::validate_schema(&value).unwrap();
        assert_eq!(json::from_value(&value, &a).unwrap().pmi, expected);
    }
}

#[test]
fn id_references_refuse_indices_and_unknown_ids() {
    let semantic = json::identify(test_model(), None).unwrap();
    let mut v = json::to_value(&semantic).unwrap();
    assert_eq!(
        v["pmi"]["dimensions"][0]["value"]["kind"]["size"]["feature"],
        semantic.ids["features"][0]
    );
    v["pmi"]["dimensions"][0]["value"]["kind"]["size"]["feature"] = json!(0);
    assert!(
        json::from_value(&v, &anchors())
            .unwrap_err()
            .to_string()
            .contains("schema violation")
    );
    v["pmi"]["dimensions"][0]["value"]["kind"]["size"]["feature"] = json!("unknown support");
    assert!(
        json::from_value(&v, &anchors())
            .unwrap_err()
            .to_string()
            .contains("unknown features id")
    );
}

#[test]
fn content_ids_survive_unrelated_insertion_and_feature_reindexing() {
    let original = test_model();
    let a = json::identify_with_anchors(original.clone(), None, &anchors()).unwrap();
    let mut changed = original.clone();
    changed
        .features
        .insert(0, Feature::Items(vec![Anchor::Face(FaceIndex(1))]));
    changed.datums[0] =
        Datum::new(DatumLabel::new("A").unwrap(), Some(FeatureId(1)), vec![]).unwrap();
    if let DimensionKind::Size { feature, .. } = &mut changed.dimensions[0].kind {
        *feature = FeatureId(1);
    }
    let mut extra = changed.dimensions[0].clone();
    extra.nominal = Some(Value::length(
        Decimal::parse("99.").unwrap(),
        LengthUnit::Millimetre,
    ));
    changed.dimensions.insert(0, extra);
    let b = json::identify_with_anchors(changed, None, &anchors()).unwrap();
    assert_eq!(a.ids["features"][0], b.ids["features"][1]);
    assert_eq!(a.ids["dimensions"][0], b.ids["dimensions"][1]);
    assert_eq!(a.ids["datums"], b.ids["datums"]);
    let mut twins = original;
    twins.dimensions.push(twins.dimensions[0].clone());
    let twins = json::identify(twins, None).unwrap();
    assert_eq!(
        twins.ids["dimensions"][1],
        format!("{}~2", twins.ids["dimensions"][0])
    );
}

#[test]
fn multi_face_support_resolves_every_face_or_refuses() {
    let mut a = anchors();
    a.names.push(Some("second".into()));
    a.bind_support(2, "verified").unwrap();
    let mut v = json::to_value(&json::identify(test_model(), None).unwrap()).unwrap();
    v["pmi"]["features"][0]["value"]["items"][0]["face"] =
        json!({"fingerprint":{"schema":2,"faces":"verified","support":[0,1]}});
    let decoded = json::from_value(&v, &a).unwrap();
    assert_eq!(
        decoded.pmi.features[0],
        Feature::Items(vec![Anchor::Face(FaceIndex(0)), Anchor::Face(FaceIndex(1))])
    );
    v["pmi"]["features"][0]["value"]["items"][0]["face"]["fingerprint"]["support"] =
        json!([0, 100]);
    assert!(
        json::from_value(&v, &a)
            .unwrap_err()
            .to_string()
            .contains("unknown face fingerprint")
    );
}

#[test]
fn identity_differences_detect_projection_order_attribute_fields_and_note_reclassification() {
    let mut p = test_model();
    p.features
        .push(Feature::Items(vec![Anchor::Face(FaceIndex(1))]));
    p.attributes = vec![
        AttributeSet {
            name: "a".into(),
            on: None,
            items: vec![("value".into(), AttributeValue::Text("x".into()))],
            description: None,
            general_property: None,
        },
        AttributeSet {
            name: "b".into(),
            on: None,
            items: vec![("value".into(), AttributeValue::Text("y".into()))],
            description: Some("".into()),
            general_property: None,
        },
    ];
    p.notes.push(Note {
        kind: "semantic text".into(),
        text: "Remove all burrs".into(),
        on: None,
        callouts: vec![],
    });
    let mut changed = p.clone();
    changed.features.swap(0, 1);
    assert!(!json::identity_differences(&p, &changed).is_empty());
    let mut changed = p.clone();
    changed.attributes.swap(0, 1);
    assert!(!json::identity_differences(&p, &changed).is_empty());
    let mut changed = p.clone();
    changed.attributes[0].description = Some("user defined attribute".into());
    assert!(!json::identity_differences(&p, &changed).is_empty());
    let mut changed = p.clone();
    changed.dimensions.push(changed.dimensions[0].clone());
    assert!(
        json::identity_differences(&p, &changed)
            .iter()
            .any(|d| d.starts_with("dimensions:"))
    );
    let mut changed = p.clone();
    changed.notes[0].callouts.push(Callout {
        name: "Observed ink".into(),
        features: vec![FeatureId(0)],
    });
    assert!(!json::identity_differences(&p, &changed).is_empty());
    let mut changed = p.clone();
    changed.notes.clear();
    changed.attributes.push(AttributeSet {
        name: "semantic text".into(),
        on: None,
        items: vec![(
            "semantic text".into(),
            AttributeValue::Text("Remove all burrs".into()),
        )],
        description: None,
        general_property: None,
    });
    assert!(!json::identity_differences(&p, &changed).is_empty());
}

#[test]
fn all_note_kinds_owners_and_optional_attribute_fields_are_identity() {
    let bytes = cube();
    let doc = Document::parse(bytes.clone()).unwrap();
    let parts = read_part_definitions(&bytes).unwrap();
    let mut p = gaps::every_kind();
    p.dimensions = test_model().dimensions;
    p.datum_targets.push(
        DatumTarget::new(
            1,
            TargetShape::Point,
            Some(Placement {
                origin: std::array::from_fn(|_| Length {
                    value: Decimal::parse("0.").unwrap(),
                    unit: LengthUnit::Millimetre,
                }),
                axis: None,
                ref_direction: None,
            }),
            None,
            None,
        )
        .unwrap(),
    );
    p.datums
        .push(Datum::new(DatumLabel::new("Z").unwrap(), None, vec![DatumTargetId(0)]).unwrap());
    for kind in [
        "semantic text",
        "surface texture",
        "general note",
        "coating",
        "edges",
        "custom 🚀",
        "",
    ] {
        for on in [
            None,
            Some(NoteOwner::Feature(FeatureId(0))),
            Some(NoteOwner::Dimension(DimensionId(0))),
            Some(NoteOwner::Tolerance(ToleranceId(0))),
            Some(NoteOwner::DatumTarget(DatumTargetId(0))),
        ] {
            p.notes.push(Note {
                kind: kind.into(),
                text: "Remove all burrs…".into(),
                on,
                callouts: vec![],
            });
        }
    }
    for description in [None, Some("".into()), Some("description".into())] {
        p.attributes.push(AttributeSet {
            name: "custom attribute".into(),
            on: None,
            description,
            general_property: None,
            items: vec![("field".into(), AttributeValue::Text("stated".into()))],
        });
    }
    let semantic = json::identify(p, None).unwrap();
    let expected = json::to_value(&semantic).unwrap();
    let (output, _) = json::write(
        &doc,
        &parts,
        &[(PartId(0), semantic)],
        pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let doc = Document::parse(output).unwrap();
    let parts = read_part_definitions(doc.bytes()).unwrap();
    let read = pmi::read(&doc, &parts).unwrap();
    let actual = json::to_value(
        &json::identify(read.parts[0].clone(), Some(&read.identifiers.parts[0])).unwrap(),
    )
    .unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn one_identified_property_with_two_tolerance_classes_has_distinct_ids() {
    use haecceity::p21::{self, Attribute as A, RawEntity};
    let bytes = cube();
    let doc = Document::parse(bytes.clone()).unwrap();
    let parts = read_part_definitions(&bytes).unwrap();
    let p = PartPmi {
        general: vec![GeneralTolerance::Class {
            text: "ISO 2768-m".into(),
            standard: standards::Iso2768::recognise("ISO 2768-m"),
        }],
        ..PartPmi::default()
    };
    let (edit, _) = pmi::write(
        &doc,
        &parts,
        &[(PartId(0), p)],
        pmi::Mode::Replace,
        pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let doc = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
    let pdef=doc.ids().find(|id|matches!(doc.get(*id),Some(RawEntity::Simple{name,attributes,..}) if name.eq_ignore_ascii_case("property_definition") && attributes.first()==Some(&A::String("default tolerances".into())))).unwrap();
    let rep=doc.ids().find(|id|matches!(doc.get(*id),Some(RawEntity::Simple{name,attributes,..}) if name.eq_ignore_ascii_case("representation") && attributes.first()==Some(&A::String("default tolerances".into())))).unwrap();
    let mut edit = p21::Edit::new();
    edit.add(p21::simple(
        "ID_ATTRIBUTE",
        vec![A::String("class-property".into()), A::EntityRef(pdef)],
    ));
    let extra = edit.add(p21::simple(
        "DESCRIPTIVE_REPRESENTATION_ITEM",
        vec![
            A::String("tolerance class".into()),
            A::String("ISO 2768-f".into()),
        ],
    ));
    let mut record = doc.get(rep).unwrap().clone();
    if let RawEntity::Simple { attributes, .. } = &mut record
        && let A::List(items) = &mut attributes[1]
    {
        items.push(extra.to_ref());
    }
    edit.replace(rep, record);
    let doc = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
    let parts = read_part_definitions(doc.bytes()).unwrap();
    let read = pmi::read(&doc, &parts).unwrap();
    let class_id = |text: &str| {
        format!(
            "class-property/tolerance-class/{}",
            text.as_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        )
    };
    assert_eq!(
        read.identifiers.parts[0].general,
        vec![Some(class_id("ISO 2768-m")), Some(class_id("ISO 2768-f"))]
    );
    let mut edit = p21::Edit::new();
    let extra = edit.add(p21::simple(
        "DESCRIPTIVE_REPRESENTATION_ITEM",
        vec![
            A::String("tolerance class".into()),
            A::String("ISO 2768-c".into()),
        ],
    ));
    let mut record = doc.get(rep).unwrap().clone();
    if let RawEntity::Simple { attributes, .. } = &mut record
        && let A::List(items) = &mut attributes[1]
    {
        items.reverse();
        items.insert(0, extra.to_ref());
    }
    edit.replace(rep, record);
    let after = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
    let updated = pmi::read(&after, &parts).unwrap();
    assert_eq!(
        updated.identifiers.parts[0].general,
        vec![
            Some(class_id("ISO 2768-c")),
            Some(class_id("ISO 2768-f")),
            Some(class_id("ISO 2768-m"))
        ]
    );
    json::identify(read.parts[0].clone(), Some(&read.identifiers.parts[0])).unwrap();
}

#[test]
fn reference_sequences_retain_equal_support_twins() {
    let bytes = cube();
    let doc = Document::parse(bytes.clone()).unwrap();
    let parts = read_part_definitions(&bytes).unwrap();
    let mut p = test_model();
    p.features.push(p.features[0].clone());
    p.features.push(Feature::Group {
        kind: GroupKind::PatternOfFeatures,
        members: vec![FeatureId(1), FeatureId(0)],
    });
    let semantic = json::identify(p, None).unwrap();
    let expected = json::to_value(&semantic).unwrap();
    let (bytes, _) = json::write_preserving(&doc, &parts, &[(PartId(0), semantic)]).unwrap();
    let doc = Document::parse(bytes).unwrap();
    let parts = read_part_definitions(doc.bytes()).unwrap();
    let read = pmi::read(&doc, &parts).unwrap();
    let actual = json::to_value(
        &json::identify(read.parts[0].clone(), Some(&read.identifiers.parts[0])).unwrap(),
    )
    .unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn unit_references_are_ids_and_keep_caller_names_and_unused_units() {
    let bytes = cube();
    let doc = Document::parse(bytes.clone()).unwrap();
    let parts = read_part_definitions(&bytes).unwrap();
    for model in [test_model(), PartPmi::default()] {
        let mut value = json::to_value(&json::identify(model, None).unwrap()).unwrap();
        if !value["pmi"]["dimensions"].as_array().unwrap().is_empty() {
            value["units"]["length"][0]["id"] = json!("caller-mm");
            value["pmi"]["dimensions"][0]["value"]["nominal"]["quantity"]["length"]["unit"] =
                json!("caller-mm");
        }
        value["units"]["angle"] = json!([{"id":"unused-angle","value":"radian"}]);
        let semantic = json::from_value(&value, &Anchors::read(&doc, &parts[0])).unwrap();
        assert_eq!(json::to_value(&semantic).unwrap(), value);
        let (out, _) = json::write_preserving(&doc, &parts, &[(PartId(0), semantic)]).unwrap();
        let after = Document::parse(out).unwrap();
        let parts = read_part_definitions(after.bytes()).unwrap();
        let read = pmi::read(&after, &parts).unwrap();
        let actual = json::to_value(
            &json::identify(read.parts[0].clone(), Some(&read.identifiers.parts[0])).unwrap(),
        )
        .unwrap();
        assert_eq!(actual, value);
        let mut duplicate = value.clone();
        duplicate["units"]["angle"] =
            json!([{"id":"a","value":"radian"},{"id":"b","value":"radian"}]);
        assert!(
            json::from_value(&duplicate, &Anchors::read(&doc, &parts[0]))
                .unwrap_err()
                .to_string()
                .contains("duplicate angle unit definition")
        );
    }
}

#[test]
fn readback_refuses_ids_moved_to_another_collection_or_item() {
    use haecceity::p21::{self, Attribute as A, RawEntity};
    let bytes = cube();
    let doc = Document::parse(bytes.clone()).unwrap();
    let parts = read_part_definitions(&bytes).unwrap();
    let mut model = test_model();
    let mut second = model.dimensions[0].clone();
    second.nominal = Some(Value::length(
        Decimal::parse("11.").unwrap(),
        LengthUnit::Millimetre,
    ));
    model.dimensions.push(second);
    let mut semantic = json::identify(model, None).unwrap();
    semantic
        .ids
        .insert("dimensions".into(), vec!["dim-a".into(), "dim-b".into()]);
    semantic.ids.insert("datums".into(), vec!["datum-a".into()]);
    let (output, _) = json::write_preserving(&doc, &parts, &[(PartId(0), semantic)]).unwrap();
    let doc = Document::parse(output).unwrap();
    let parts = read_part_definitions(doc.bytes()).unwrap();
    for (a, b) in [("dim-a", "datum-a"), ("dim-a", "dim-b")] {
        let mut edit = p21::Edit::new();
        let mut count = 0;
        for id in doc.ids() {
            let Some(RawEntity::Simple {
                name, attributes, ..
            }) = doc.get(id)
            else {
                continue;
            };
            if !name.eq_ignore_ascii_case("id_attribute") {
                continue;
            }
            let Some(A::String(value)) = attributes.first() else {
                continue;
            };
            if value != a && value != b {
                continue;
            }
            let mut record = doc.get(id).unwrap().clone();
            if let RawEntity::Simple { attributes, .. } = &mut record {
                attributes[0] = A::String(if value == a { b } else { a }.into());
            }
            edit.replace(id, record);
            count += 1;
        }
        assert_eq!(
            count, 2,
            "the complete id set remains present, only carrier ownership changes"
        );
        let after = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
        let error = pmi::read(&after, &parts).unwrap_err().to_string();
        assert!(
            error.contains("ordering id") || error.contains("native content changed"),
            "{error}"
        );
    }
}

#[test]
fn a_new_true_twin_can_follow_an_existing_content_id_from_step() {
    let mut model = test_model();
    let previous = json::identify(model.clone(), None).unwrap();
    let old = previous.ids["dimensions"][0].clone();
    model.dimensions.push(model.dimensions[0].clone());
    let mut identifiers = PartIdentifiers::default();
    identifiers.dimensions = vec![Some(old.clone()), None];
    let identified = json::identify(model, Some(&identifiers)).unwrap();
    assert_eq!(
        identified.ids["dimensions"],
        vec![old.clone(), format!("{old}~2")]
    );
}

#[test]
fn content_id_has_a_fixed_canonical_json_vector() {
    // FNV-1a 128 over the documented sorted-key JSON of the resolved dimension,
    // including its stated "10.000" millimetres and the face-zero support.
    assert_eq!(
        json::identify(test_model(), None).unwrap().ids["dimensions"][0],
        "dimensions:54593f8dc8d8dfaa8c91406d8a1898ee"
    );
}

#[test]
fn unused_supplemental_geometry_roundtrips_as_native_geometry() {
    let bytes = cube();
    let doc = Document::parse(bytes.clone()).unwrap();
    let parts = read_part_definitions(&bytes).unwrap();
    let geometry = gaps::every_kind().geometry;
    let input = json::identify(
        PartPmi {
            geometry,
            ..Default::default()
        },
        None,
    )
    .unwrap();
    let expected = json::to_value(&input).unwrap();
    let (out, _) = json::write_preserving(&doc, &parts, &[(PartId(0), input)]).unwrap();
    let after = Document::parse(out).unwrap();
    let parts = read_part_definitions(after.bytes()).unwrap();
    let read = pmi::read(&after, &parts).unwrap();
    let actual = json::to_value(
        &json::identify(read.parts[0].clone(), Some(&read.identifiers.parts[0])).unwrap(),
    )
    .unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn native_reference_to_an_equal_valued_twin_is_not_repaired_by_metadata() {
    use haecceity::p21::{self, Attribute as A, RawEntity};
    let bytes = cube();
    let doc = Document::parse(bytes.clone()).unwrap();
    let parts = read_part_definitions(&bytes).unwrap();
    let mut model = test_model();
    model.dimensions.push(model.dimensions[0].clone());
    model.notes.push(Note {
        kind: "semantic text".into(),
        text: "Inspect first dimension".into(),
        on: Some(NoteOwner::Dimension(DimensionId(0))),
        callouts: vec![],
    });
    let mut semantic = json::identify(model, None).unwrap();
    semantic
        .ids
        .insert("dimensions".into(), vec!["dim-a".into(), "dim-b".into()]);
    semantic.ids.insert("notes".into(), vec!["note-a".into()]);
    let (bytes, _) = json::write_preserving(&doc, &parts, &[(PartId(0), semantic)]).unwrap();
    let doc = Document::parse(bytes).unwrap();
    let parts = read_part_definitions(doc.bytes()).unwrap();
    let carrier = |wanted: &str| {
        doc.ids()
            .find_map(|id| match doc.get(id) {
                Some(RawEntity::Simple {
                    name, attributes, ..
                }) if name.eq_ignore_ascii_case("id_attribute") => match attributes.as_slice() {
                    [A::String(value), A::EntityRef(target)] if value == wanted => Some(*target),
                    _ => None,
                },
                _ => None,
            })
            .unwrap()
    };
    let note = carrier("note-a");
    let dim_b = carrier("dim-b");
    let mut record = doc.get(note).unwrap().clone();
    if let RawEntity::Simple {
        name, attributes, ..
    } = &mut record
    {
        assert!(name.eq_ignore_ascii_case("property_definition"));
        attributes[2] = A::EntityRef(dim_b);
    } else {
        panic!("note property expected");
    }
    let mut edit = p21::Edit::new();
    edit.replace(note, record);
    let changed = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
    let native = pmi::read_native(&changed, &parts).unwrap();
    let Some(NoteOwner::Dimension(owner)) = native.parts[0].notes[0].on else {
        panic!("dimension owner expected");
    };
    assert_eq!(
        native.identifiers.parts[0].dimensions[owner.0].as_deref(),
        Some("dim-b")
    );
    let error = pmi::read(&changed, &parts).unwrap_err().to_string();
    assert!(
        error.contains("native reference") && error.contains("dim-b"),
        "{error}"
    );
}

#[test]
fn wire_fingerprint_forms_and_multiple_support_faces_are_json_step_identity() {
    let bytes = cube();
    let doc = Document::parse(bytes.clone()).unwrap();
    let parts = read_part_definitions(&bytes).unwrap();
    let mut anchors = Anchors::read(&doc, &parts[0]);
    anchors
        .bind_support(2, "verified-part-fingerprint")
        .unwrap();
    let base = json::to_value(&json::identify(test_model(), None).unwrap()).unwrap();
    for face in [
        json!({"fingerprint":"2:verified-part-fingerprint/0"}),
        json!({"fingerprint":{"schema":2,"faces":"verified-part-fingerprint","support":[1,0]}}),
    ] {
        let mut input = base.clone();
        input["pmi"]["features"][0]["value"]["items"][0]["face"] = face;
        let semantic = json::from_value(&input, &anchors).unwrap();
        assert_eq!(json::to_value(&semantic).unwrap(), input);
        let (output, _) = json::write_preserving(&doc, &parts, &[(PartId(0), semantic)]).unwrap();
        let doc = Document::parse(output).unwrap();
        let parts = read_part_definitions(doc.bytes()).unwrap();
        let read = pmi::read(&doc, &parts).unwrap();
        let semantic =
            json::identify(read.parts[0].clone(), Some(&read.identifiers.parts[0])).unwrap();
        assert_eq!(json::to_value(&semantic).unwrap(), input);
    }
}

#[test]
fn integer_mantissas_are_identity_in_every_writer_kind_including_real_attributes() {
    let doc = Document::parse(cube()).unwrap();
    let parts = read_part_definitions(doc.bytes()).unwrap();
    let mut raw = serde_json::to_value(gaps::every_kind()).unwrap();
    fn mantissas(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(text)
                if Decimal::parse(text).is_ok() && text.contains('.') =>
            {
                let (mantissa, exp) = text
                    .split_once(['e', 'E'])
                    .map_or((text.as_str(), 0), |(m, e)| (m, e.parse::<i64>().unwrap()));
                let fraction = mantissa.split_once('.').unwrap().1.len();
                *text = format!("{}e{}", mantissa.replace('.', ""), exp - fraction as i64);
            }
            serde_json::Value::Array(array) => array.iter_mut().for_each(mantissas),
            serde_json::Value::Object(object) => object.values_mut().for_each(mantissas),
            _ => {}
        }
    }
    mantissas(&mut raw);
    let mut pmi: PartPmi = serde_json::from_value(raw).unwrap();
    pmi.attributes.push(AttributeSet {
        name: "REAL path coverage".into(),
        on: None,
        description: None,
        general_property: None,
        items: vec![
            (
                "integer REAL".into(),
                AttributeValue::Real(Decimal::parse("42").unwrap()),
            ),
            (
                "typed REAL".into(),
                AttributeValue::OtherMeasure {
                    measure: "solid_angle_measure".into(),
                    value: Decimal::parse("2e0").unwrap(),
                    unit: "steradian".into(),
                },
            ),
            (
                "length scale".into(),
                AttributeValue::Measure(Value::length(
                    Decimal::parse("3").unwrap(),
                    LengthUnit::Other {
                        name: "double metre".into(),
                        metres: Decimal::parse("2e0").unwrap(),
                    },
                )),
            ),
            (
                "angle scale".into(),
                AttributeValue::Measure(Value::angle(
                    Decimal::parse("4").unwrap(),
                    AngleUnit::Other {
                        name: "double radian".into(),
                        radians: Decimal::parse("2").unwrap(),
                    },
                )),
            ),
        ],
    });
    let input = json::identify(pmi, None).unwrap();
    let expected = json::to_value(&input).unwrap();
    json::validate(&expected).unwrap();
    let (written, _) = json::write(
        &doc,
        &parts,
        &[(PartId(0), input)],
        pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let doc = Document::parse(written).unwrap();
    let parts = read_part_definitions(doc.bytes()).unwrap();
    let read = pmi::read(&doc, &parts).unwrap();
    let actual = json::identify(read.parts[0].clone(), Some(&read.identifiers.parts[0])).unwrap();
    assert_eq!(json::to_value(&actual).unwrap(), expected);
}
