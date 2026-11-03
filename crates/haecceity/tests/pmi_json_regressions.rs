//! Minimal xcaf-pmi producer regressions for #142.
#![cfg(feature = "json-import")]
use haecceity::{
    json::{self, Anchors},
    p21::Document,
    pmi::{self, PartId},
    step::read_part_definitions,
};
use serde_json::{Value, json};
use std::io::Read;

fn fixture(name: &str) -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/q142")
        .join(name);
    json::parse(&std::fs::read_to_string(path).unwrap()).unwrap()
}
fn geometry() -> Document {
    let mut bytes = vec![];
    flate2::read::GzDecoder::new(&include_bytes!("fixtures/q142/geometry.step.gz")[..])
        .read_to_end(&mut bytes)
        .unwrap();
    Document::parse(bytes).unwrap()
}
fn identity(value: &Value) -> (Document, pmi::write::WriteReport) {
    json::validate(value).unwrap();
    let doc = geometry();
    let parts = read_part_definitions(doc.bytes()).unwrap();
    let semantic = json::from_value(value, &Anchors::read(&doc, &parts[0])).unwrap();
    let (bytes, report) = json::write(
        &doc,
        &parts,
        &[(PartId(0), semantic)],
        pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let after = Document::parse(bytes).unwrap();
    let parts = read_part_definitions(after.bytes()).unwrap();
    let read = pmi::read(&after, &parts).unwrap();
    let actual = json::identify(read.parts[0].clone(), Some(&read.identifiers.parts[0])).unwrap();
    assert_eq!(json::to_value(&actual).unwrap(), *value);
    assert!(haecceity::express::validate_document(after.graph()).is_empty());
    for violation in haecceity::express_rules::check_all(&after) {
        assert!(
            report.practice_schema_conflicts.contains(&violation),
            "{violation:?}"
        );
    }
    (after, report)
}

#[test]
fn producer_datum_target_decimals_are_stated_verbatim() {
    let original = fixture("datum-target-decimals.json");
    identity(&original);
    for (zero, one, size, coordinate) in [
        ("0.0", "1.0", "8.0", "-30.0"),
        ("0.", "1.", "8.", "-30."),
        ("0.5", "1.5", "8.5", "-30.5"),
        ("0.E+0", "1.00E0", "8.E+0", "-3.0E+1"),
    ] {
        for shape in [
            json!("point"),
            json!({"line":{"length":{"unit":"unit/mm","value":size}}}),
            json!({"rectangle":{"length":{"unit":"unit/mm","value":size},"width":{"unit":"unit/mm","value":size}}}),
            json!({"circle":{"diameter":{"unit":"unit/mm","value":size}}}),
            json!({"circular curve":{"diameter":{"unit":"unit/mm","value":size}}}),
        ] {
            let mut value = original.clone();
            for target in value["pmi"]["datum_targets"].as_array_mut().unwrap() {
                target["value"]["shape"] = shape.clone();
                let p = &mut target["value"]["placement"];
                p["origin"] = json!([
                    {"unit":"unit/mm","value":coordinate},
                    {"unit":"unit/mm","value":zero},
                    {"unit":"unit/mm","value":one}
                ]);
                // Equal REAL values with distinct tokens in the same record must survive.
                p["axis"] = json!([zero, "0.00", one]);
                p["ref_direction"] = json!([one, "0.E-1", "-0.0"]);
                if shape.get("line").is_some() {
                    target["value"]["movable"] = json!([one, "0.00", "0.E0"]);
                }
            }
            identity(&value);
        }
    }
    let mut value = original;
    value["pmi"]["geometry"] = json!([{"id":"supplemental point", "value":{
        "length_unit":"unit/mm",
        "item":{"leaves":[["CARTESIAN_POINT", [
            {"text":""}, {"list":[{"real":"0.0"},{"real":"0.E+0"},{"real":"1.00"}]}
        ]]]}
    }}]);
    identity(&value);
    value["pmi"]["geometry"][0]["value"]["item"]["leaves"][0][1][1]["list"][0]["real"] =
        json!("0e0");
    identity(&value);
}

#[test]
fn producer_requirement_notes_use_characterized_items_in_a_supported_edition() {
    let mut value = fixture("requirement-note.json");
    for owner in [
        json!({"tolerance":"0:1:4:10"}),
        json!({"dimension":"0:1:4:7"}),
    ] {
        value["pmi"]["notes"][0]["value"]["on"] = owner;
        let (after, report) = identity(&value);
        assert_eq!(
            after.file_schema().unwrap(),
            [haecceity::express::TARGET_SCHEMA]
        );
        assert!(matches!(
            report.schema,
            pmi::write::SchemaChange::Upgraded { .. }
        ));
        // The native reader resolves the requirement owner without transport aliases.
        let parts = read_part_definitions(after.bytes()).unwrap();
        let native = pmi::read_native(&after, &parts).unwrap();
        assert!(matches!(
            native.parts[0].notes[0].on,
            Some(pmi::NoteOwner::Dimension(_) | pmi::NoteOwner::Tolerance(_))
        ));
    }
}

#[test]
fn every_note_owner_is_json_step_json_identity_even_on_shared_features() {
    let mut value = fixture("requirement-note.json");
    let targets = fixture("datum-target-decimals.json");
    for collection in ["features", "datum_targets", "datums"] {
        value["pmi"][collection].as_array_mut().unwrap().extend(
            targets["pmi"][collection]
                .as_array()
                .unwrap()
                .iter()
                .cloned(),
        );
    }
    let mut dimension = value["pmi"]["dimensions"][0].clone();
    dimension["id"] = json!("size twin");
    value["pmi"]["dimensions"]
        .as_array_mut()
        .unwrap()
        .push(dimension.clone());
    dimension["id"] = json!("location");
    dimension["value"]["kind"] = json!({"location":{
        "from":"feature/7","to":"feature/12","kind":"linear distance",
        "angle":null,"directed":false,"path":null
    }});
    dimension["value"]["tolerance"] = json!("none");
    value["pmi"]["dimensions"]
        .as_array_mut()
        .unwrap()
        .push(dimension);
    let mut tolerance = value["pmi"]["tolerances"][0].clone();
    tolerance["id"] = json!("tolerance twin");
    value["pmi"]["tolerances"]
        .as_array_mut()
        .unwrap()
        .push(tolerance);
    value["pmi"]["notes"] = json!([
        null,
        {"feature":"feature/7"},
        {"dimension":"0:1:4:7"},
        {"dimension":"size twin"},
        {"dimension":"location"},
        {"tolerance":"0:1:4:10"},
        {"tolerance":"tolerance twin"},
        {"datum_target":"datum_target/D1"}
    ])
    .as_array()
    .unwrap()
    .iter()
    .enumerate()
    .map(|(i, owner)| {
        json!({"id":format!("note/{i}"),"value":{
            "kind":"semantic text","text":"Check before assembly","on":owner,"callouts":[]
        }})
    })
    .collect::<Vec<_>>()
    .into();
    identity(&value);
}

#[test]
fn dimension_without_stated_value_is_valid() {
    let mut value = fixture("requirement-note.json");
    value["pmi"]["dimensions"][0]["value"]["nominal"] = Value::Null;
    value["pmi"]["dimensions"][0]["value"]["tolerance"] = json!("none");
    // This is a source statement, not a reason to invent a value or drop the item.
    assert!(json::validate(&value).is_ok());
    identity(&value);
}

#[test]
fn validate_refuses_geometry_independent_import_failures() {
    let original = fixture("requirement-note.json");
    for (path, replacement, reason) in [
        // A fit still needs a nominal to which its class applies.
        (
            "/pmi/dimensions/0/value/nominal",
            Value::Null,
            "without a nominal",
        ),
        (
            "/pmi/notes/0/value/on",
            json!({"tolerance":"missing"}),
            "unknown tolerances id",
        ),
        (
            "/pmi/tolerances/0/value/zone",
            json!({"form":"cylindrical or circular","projected":null,"non_uniform":null,
                "runout_angle":null,"affected_plane":null,"affected_plane_kind":"intersection_plane"}),
            "affected plane kind has no affected plane feature",
        ),
        (
            "/pmi/tolerances/0/value/zone",
            json!({"form":"cylindrical or circular","projected":null,"non_uniform":null,
                "runout_angle":null,"affected_plane":"feature/12","affected_plane_kind":"intersection_plane"}),
            "affected plane requires an explicit",
        ),
    ] {
        let mut value = original.clone();
        *value.pointer_mut(path).unwrap() = replacement;
        assert!(json::validate_schema(&value).is_ok());
        let error = json::validate(&value).unwrap_err().to_string();
        assert!(error.contains(reason), "{error}");
        let doc = geometry();
        let parts = read_part_definitions(doc.bytes()).unwrap();
        let imported =
            json::from_value(&value, &Anchors::read(&doc, &parts[0])).and_then(|semantic| {
                json::write(
                    &doc,
                    &parts,
                    &[(PartId(0), semantic)],
                    pmi::write::PresentationPolicy::RemovePresentation,
                )
            });
        assert!(imported.is_err());
    }
    let mut target = fixture("datum-target-decimals.json");
    target["pmi"]["datum_targets"][0]["value"]["shape"] = json!({"curve":"feature/3"});
    assert!(json::validate_schema(&target).is_ok());
    assert!(
        json::validate(&target)
            .unwrap_err()
            .to_string()
            .contains("explicit area/curve target requires stated face/curve geometry")
    );
}

#[test]
fn integer_density_exponents_and_values_preserve_identity_and_use_annex_c_units() {
    let original = fixture("density-integer-exponents.json");
    for (mass, length) in [
        ("gram", "centimetre"),
        ("kilogram", "metre"),
        ("milligram", "millimetre"),
    ] {
        for (one, minus_three, value) in [
            ("1", "-3", "7"),
            ("1e0", "-3E0", "27e-1"),
            ("1.", "-3.", "2.700"),
        ] {
            let mut input = original.clone();
            input["units"]["density"][0]["value"] = json!([[mass, one], [length, minus_three]]);
            input["pmi"]["material"]["density"]["value"] = json!(value);
            let (doc, _) = identity(&input);
            let text = String::from_utf8_lossy(doc.bytes());
            assert!(!text.contains("SI_UNIT($,.GRAM.)"));
            assert!(!text.contains("SI_UNIT(.MILLI.,.GRAM.)"));
            assert!(text.contains("SI_UNIT(.KILO.,.GRAM.)"));
            if mass != "kilogram" {
                assert!(text.contains(&format!("CONVERSION_BASED_UNIT('{mass}',")));
            }
        }
    }
    let mut invalid = original;
    invalid["units"]["density"][0]["value"] = json!([["gram", "3"], ["centimetre", "2"]]);
    assert!(
        json::validate(&invalid)
            .unwrap_err()
            .to_string()
            .contains("mass 1, length -3")
    );
}

#[test]
fn integer_and_exponent_mantissas_work_in_dimensions_and_all_target_geometry_reals() {
    for name in ["requirement-note.json", "datum-target-decimals.json"] {
        let mut value = fixture(name);
        fn rewrite(value: &mut Value) {
            match value {
                Value::String(text) if pmi::Decimal::parse(text).is_ok() => {
                    if let Some((whole, fraction)) = text.split_once('.')
                        && !text.contains(['e', 'E'])
                    {
                        *text = format!("{whole}{fraction}e-{}", fraction.len());
                    }
                }
                Value::Array(a) => a.iter_mut().for_each(rewrite),
                Value::Object(o) => o.values_mut().for_each(rewrite),
                _ => {}
            }
        }
        rewrite(&mut value);
        identity(&value);
    }
}

#[test]
fn compact_ordering_omits_native_singleton_anchors_empty_fields_and_collections() {
    let mut value = fixture("requirement-note.json");
    value["pmi"]["features"][0]["value"]["items"] = json!([{"face":0}]);
    value["pmi"]["features"][1]["value"]["items"] = json!([{"face":1}]);
    let (doc, _) = identity(&value);
    let metadata = doc
        .ids()
        .find_map(|id| match doc.get(id) {
            Some(haecceity::p21::RawEntity::Simple {
                name, attributes, ..
            }) if name.eq_ignore_ascii_case("descriptive_representation_item")
                && attributes.first()
                    == Some(&haecceity::p21::Attribute::String("ordering".into())) =>
            {
                if let Some(haecceity::p21::Attribute::String(s)) = attributes.get(1) {
                    Some(s.clone())
                } else {
                    None
                }
            }
            _ => None,
        })
        .unwrap();
    let compact: Value = serde_json::from_str(&metadata).unwrap();
    assert!(compact["items"].get("threads").is_none());
    assert!(compact.get("wire_anchors").is_none());
    assert!(compact["items"]["features"][0].get("anchors").is_none());
    assert!(compact["items"]["features"][0].get("references").is_none());
    assert!(compact["items"]["notes"][0].get("callouts").is_none());
    // The old format repeated defaults even on a simple plate. Reconstruct its size.
    let mut verbose = compact.clone();
    for c in [
        "features",
        "geometry",
        "derived_geometry",
        "datum_targets",
        "datums",
        "dimensions",
        "tolerances",
        "tolerance_relations",
        "general",
        "threads",
        "knurls",
        "notes",
        "surface_textures",
        "attributes",
    ] {
        if verbose["items"].get(c).is_none() {
            verbose["items"][c] = json!([]);
        }
        for e in verbose["items"][c].as_array_mut().unwrap() {
            for (key, default) in [
                ("references", json!({})),
                ("sequences", json!({})),
                ("anchors", Value::Null),
                ("callouts", Value::Null),
            ] {
                if e.get(key).is_none() {
                    e[key] = default;
                }
            }
        }
    }
    let collections = [
        "features",
        "geometry",
        "derived_geometry",
        "datum_targets",
        "datums",
        "dimensions",
        "tolerances",
        "tolerance_relations",
        "general",
        "threads",
        "knurls",
        "notes",
        "surface_textures",
        "attributes",
    ];
    fn legacy(reference: &Value, compact: &Value, collections: &[&str]) -> Value {
        let collection = collections[reference[0].as_u64().unwrap() as usize];
        json!({"collection":collection,"id":compact["items"][collection][reference[1].as_u64().unwrap() as usize]["id"]})
    }
    fn legacy_markers(value: &mut Value, compact: &Value, collections: &[&str]) {
        if let Some(reference) = value.get("$reference") {
            *value = json!({"$reference":legacy(reference,compact,collections)});
        } else {
            match value {
                Value::Array(array) => array
                    .iter_mut()
                    .for_each(|v| legacy_markers(v, compact, collections)),
                Value::Object(object) => object
                    .values_mut()
                    .for_each(|v| legacy_markers(v, compact, collections)),
                _ => {}
            }
        }
    }
    for entries in verbose["items"].as_object_mut().unwrap().values_mut() {
        for entry in entries.as_array_mut().unwrap() {
            for reference in entry["references"].as_object_mut().unwrap().values_mut() {
                *reference = legacy(reference, &compact, &collections);
            }
            legacy_markers(&mut entry["sequences"], &compact, &collections);
        }
    }
    verbose.as_object_mut().unwrap().remove("v");
    verbose["wire_anchors"] = json!([]);
    assert!(metadata.len() * 4 < serde_json::to_string(&verbose).unwrap().len() * 3);
    // Old producers' verbose form must still read.
    let changed = String::from_utf8_lossy(doc.bytes()).replace(
        &metadata.replace("'", "''"),
        &serde_json::to_string(&verbose).unwrap().replace("'", "''"),
    );
    let old = Document::parse(changed.into_bytes()).unwrap();
    let parts = read_part_definitions(old.bytes()).unwrap();
    pmi::read(&old, &parts).unwrap();
}

#[test]
fn decimal_spellings_cannot_replace_text_or_change_a_native_value() {
    let mut input = fixture("density-integer-exponents.json");
    input["pmi"]["material"]["id"] = json!("1");
    let (doc, _) = identity(&input);
    let text = String::from_utf8_lossy(doc.bytes());
    let original = doc
        .ids()
        .find_map(|id| match doc.get(id) {
            Some(haecceity::p21::RawEntity::Simple {
                name, attributes, ..
            }) if name.eq_ignore_ascii_case("descriptive_representation_item")
                && attributes.first()
                    == Some(&haecceity::p21::Attribute::String("ordering".into())) =>
            {
                if let Some(haecceity::p21::Attribute::String(s)) = attributes.get(1) {
                    Some(s.clone())
                } else {
                    None
                }
            }
            _ => None,
        })
        .unwrap();
    for (path, value, reason) in [
        ("/material/id", "1.0", "not a decimal"),
        ("/material/density/value", "3.7", "changes native value"),
    ] {
        let mut metadata: Value = serde_json::from_str(&original).unwrap();
        metadata["spellings"][path] = json!(value);
        let changed = text.replace(
            &original.replace("'", "''"),
            &serde_json::to_string(&metadata).unwrap().replace("'", "''"),
        );
        let doc = Document::parse(changed.into_bytes()).unwrap();
        let parts = read_part_definitions(doc.bytes()).unwrap();
        assert!(
            pmi::read(&doc, &parts)
                .unwrap_err()
                .to_string()
                .contains(reason)
        );
    }
}

#[test]
fn conversion_unit_factor_tokens_are_identity_even_when_no_other_spelling_binding_is_needed() {
    for factor in ["2", "2.", "2.00", "2e0", "20E-1"] {
        let mut value = fixture("requirement-note.json");
        value["units"]["length"][0]["value"] =
            json!({"other":{"name":"double metre","metres":factor}});
        identity(&value);
    }
}
