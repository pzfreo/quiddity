//! Optional editor layout metadata, from the source graph rather than regenerated notation.
#![cfg(feature = "json-import")]
use haecceity::{
    json::{
        self,
        presentation::{self, Graphic, Presentation},
    },
    p21::{self, Attribute as A, Document},
    pmi::{self, PartId},
    step::read_part_definitions,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, io::Read, path::Path};

fn load(path: &str) -> Document {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    let bytes = std::fs::read(path).unwrap();
    let mut raw = vec![];
    flate2::read::GzDecoder::new(bytes.as_slice())
        .read_to_end(&mut raw)
        .unwrap();
    Document::parse(raw).unwrap()
}
fn mixed() -> Document {
    load("tests/fixtures/presentation/ctc01-text-filled.step.gz")
}
fn read(doc: &Document) -> (json::Semantic, Presentation) {
    let parts = read_part_definitions(doc.bytes()).unwrap();
    let native = pmi::read(doc, &parts).unwrap();
    let semantic =
        json::identify(native.parts[0].clone(), Some(&native.identifiers.parts[0])).unwrap();
    let drawn = presentation::read(doc, &native, PartId(0), &semantic).unwrap();
    presentation::validate(&serde_json::to_value(&drawn).unwrap()).unwrap();
    presentation::validate_links(&drawn, &semantic).unwrap();
    (semantic, drawn)
}
fn unique(drawn: &Presentation) -> BTreeMap<&str, &Graphic> {
    drawn
        .items
        .values()
        .flatten()
        .chain(&drawn.unlinked)
        .map(|g| (g.id.as_deref().unwrap(), g))
        .collect()
}
fn near(actual: [f64; 3], expected: [f64; 3]) {
    for i in 0..3 {
        assert!(
            (actual[i] - expected[i]).abs() < 1e-6,
            "{actual:?} != {expected:?}"
        );
    }
}

#[test]
fn shared_ctc01_derived_fixture_pins_text_and_filled_triangles() {
    let original = load("../../tests/fixtures/ap242/nist/nist_ctc_01_asme1_ap242-e1.stp.gz");
    let (_, native) = read(&original);
    let original_graphics = unique(&native);
    // NIST's actual source has tessellated glyphs, with no TEXT_LITERAL at all.
    assert_eq!(
        original_graphics
            .values()
            .map(|g| g.text.len())
            .sum::<usize>(),
        0
    );
    assert_eq!(
        original_graphics
            .values()
            .map(|g| g.triangles.len())
            .sum::<usize>(),
        2754
    );
    let doc = mixed();
    let (_, drawn) = read(&doc);
    let graphics = unique(&drawn);
    assert_eq!(graphics.len(), 24);
    assert_eq!(graphics.values().map(|g| g.text.len()).sum::<usize>(), 1);
    assert_eq!(
        graphics.values().map(|g| g.triangles.len()).sum::<usize>(),
        2754
    );
    assert!(
        haecceity::express::validate_document(&doc)
            .iter()
            .all(|v| v.id <= 4376)
    );
    let literal = graphics["step:4389"];
    assert_eq!(literal.text, ["CTC-01 editable test text"]);
    let placed = &literal.text_placements[0];
    assert_eq!(placed.text_index, Some(0));
    assert_eq!(
        placed.stroked, None,
        "the reader did not stroke this literal"
    );
    near(placed.origin, [10., 20., 30.]);
    near(placed.x_axis, [1., 0., 0.]);
    near(placed.normal, [0., -1., 0.]);
    assert_eq!(placed.height, Some(3.));
    assert_eq!(placed.alignment.as_deref(), Some("baseline left"));
    assert_eq!(placed.cell_box, Some([[0., 0.], [60., 3.]]));
    assert_eq!(literal.plane_id.as_deref(), Some("step:4391"));
    assert_eq!(drawn.planes.len(), 24);
    let saved = drawn.views.iter().find(|v| v.name == "MBD_0").unwrap();
    assert_eq!(saved.graphics.len(), 24);
    assert!(saved.graphics.iter().any(|id| id == "step:4389"));
    // #4218's Z and X are read by hand from the NIST source, not from this reader.
    near(
        saved.direction.unwrap(),
        [0.588012775519021, -0.566487095927986, 0.577350280136327],
    );
    near(
        saved.up.unwrap(),
        [-0.41578778129416627, 0.4005669318602988, 0.8164965731872372],
    );
}

#[test]
fn datum_symbols_are_keyed_to_datums_and_retain_the_feature_alias() {
    for (path, names) in [
        (
            "tests/fixtures/presentation/spindle.step.gz",
            vec!["Datum A", "Datum B"],
        ),
        (
            "../../tests/fixtures/ap242/nist/nist_ctc_01_asme1_ap242-e1.stp.gz",
            vec!["Simple Datum.1", "Simple Datum.2", "Simple Datum.3"],
        ),
    ] {
        let (semantic, drawn) = read(&load(path));
        assert_eq!(semantic.pmi.datums.len(), names.len());
        for (i, datum) in semantic.pmi.datums.iter().enumerate() {
            let datum_id = &semantic.ids["datums"][i];
            let graphics = &drawn.items[datum_id];
            assert!(graphics.iter().any(|g| names.contains(&g.name.as_str())));
            let feature_id = &semantic.ids["features"][datum.feature().unwrap().0];
            for graphic in graphics {
                assert!(drawn.items[feature_id].iter().any(|g| g.id == graphic.id));
                assert!(graphic.plane_id.is_some());
            }
        }
    }
}

#[test]
fn spindle_plane_positions_and_unoriented_draughting_group_are_source_values() {
    let (_, drawn) = read(&load("tests/fixtures/presentation/spindle.step.gz"));
    assert_eq!(drawn.planes.len(), 14);
    let plane = drawn.planes.iter().find(|p| p.id == "step:1899").unwrap();
    near(plane.origin, [16.8732, 0., 4.70608]);
    near(plane.x_axis, [1., 0., 0.]);
    near(plane.normal, [0., -1., 0.]);
    let view = drawn
        .views
        .iter()
        .find(|v| v.name == "PMI callouts")
        .unwrap();
    assert_eq!(view.graphics.len(), 14);
    assert!(
        view.direction.is_none() && view.up.is_none(),
        "no camera is invented for this model"
    );
}

#[test]
fn placeholders_keep_position_and_line_spacing_without_inventing_text_or_height() {
    let (_, drawn) = read(&load(
        "../../tests/fixtures/ap242/nist/nist_ctc_02_asme1_ap242-e2.stp.gz",
    ));
    let placeholders: Vec<_> = unique(&drawn)
        .values()
        .flat_map(|g| &g.text_placements)
        .filter(|p| p.text_index.is_none())
        .collect();
    let graphics = unique(&drawn);
    let first = graphics
        .values()
        .find(|g| g.step_ids.contains(&232))
        .unwrap();
    assert!((first.text_placements[0].line_spacing.unwrap() - 2.1).abs() < 1e-6);
    assert_eq!(placeholders.len(), 46);
    assert!(placeholders.iter().all(|p| p.height.is_none()));
    assert_eq!(
        placeholders
            .iter()
            .filter(|p| p.line_spacing == Some(0.))
            .count(),
        17
    );
    assert_eq!(
        placeholders
            .iter()
            .filter(|p| p.line_spacing.is_some_and(|n| (n - 2.1).abs() < 1e-6))
            .count(),
        29
    );
    assert_eq!(
        graphics
            .values()
            .filter(|g| g
                .unsupported
                .iter()
                .any(|why| why.contains("non-positive or unresolved placeholder line spacing")))
            .count(),
        17
    );
    assert!(
        placeholders
            .iter()
            .all(|p| p.origin.iter().all(|n| n.is_finite()))
    );
}

#[test]
fn mapped_two_dimensional_text_uses_both_mapping_frames() {
    let doc = mixed();
    let mut edit = p21::Edit::new();
    let point = |x, y| {
        p21::simple(
            "CARTESIAN_POINT",
            vec![A::String("".into()), A::List(vec![A::Real(x), A::Real(y)])],
        )
    };
    let source_point = edit.add(point(2., 3.));
    let source_x = edit.add(p21::simple(
        "DIRECTION",
        vec![
            A::String("".into()),
            A::List(vec![A::Real(0.), A::Real(1.)]),
        ],
    ));
    let source_axis = edit.add(p21::simple(
        "AXIS2_PLACEMENT_2D",
        vec![
            A::String("".into()),
            source_point.to_ref(),
            source_x.to_ref(),
        ],
    ));
    let origin_point = edit.add(point(1., 1.));
    let origin = edit.add(p21::simple(
        "AXIS2_PLACEMENT_2D",
        vec![A::String("".into()), origin_point.to_ref(), A::Unset],
    ));
    let mut literal = doc.get(4383).unwrap().clone();
    if let p21::RawEntity::Simple { attributes, .. } = &mut literal {
        attributes[2] = source_axis.to_ref();
    }
    edit.replace(4383, literal);
    let rep = edit.add(p21::simple(
        "TEXT_STRING_REPRESENTATION",
        vec![
            A::String("mapped literal".into()),
            A::List(vec![A::EntityRef(4383)]),
            A::EntityRef(4351),
        ],
    ));
    let mapping = edit.add(p21::simple(
        "REPRESENTATION_MAP",
        vec![origin.to_ref(), rep.to_ref()],
    ));
    let target_point = edit.add(p21::simple(
        "CARTESIAN_POINT",
        vec![
            A::String("".into()),
            A::List(vec![A::Real(100.), A::Real(200.), A::Real(300.)]),
        ],
    ));
    let target = edit.add(p21::simple(
        "AXIS2_PLACEMENT_3D",
        vec![
            A::String("".into()),
            target_point.to_ref(),
            A::EntityRef(4378),
            A::EntityRef(4379),
        ],
    ));
    let annotation = edit.add(p21::simple(
        "ANNOTATION_TEXT",
        vec![
            A::String("mapped literal".into()),
            mapping.to_ref(),
            target.to_ref(),
        ],
    ));
    let mut occurrence = doc.get(4388).unwrap().clone();
    if let p21::RawEntity::Simple { attributes, .. } = &mut occurrence {
        attributes[2] = annotation.to_ref();
    }
    edit.replace(4388, occurrence);
    edit.replace(
        4390,
        p21::simple(
            "PLANE",
            vec![A::String("mapped literal".into()), target.to_ref()],
        ),
    );
    let doc = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
    let (_, drawn) = read(&doc);
    let graphics = unique(&drawn);
    let literal = &graphics["step:4389"].text_placements[0];
    near(literal.origin, [101., 200., 302.]);
    near(literal.x_axis, [0., 0., 1.]);
    near(literal.normal, [0., -1., 0.]);
    assert_eq!(literal.height, Some(3.));
}

#[test]
fn style_height_and_display_coordinates_use_the_representation_unit() {
    let doc = mixed();
    let mut edit = p21::Edit::new();
    edit.replace(
        4386,
        p21::simple(
            "TEXT_STYLE_WITH_BOX_CHARACTERISTICS",
            vec![
                A::String("".into()),
                A::EntityRef(4385),
                A::List(vec![A::Typed {
                    type_name: "BOX_HEIGHT".into(),
                    value: Box::new(A::Real(4.5)),
                }]),
            ],
        ),
    );
    edit.replace(
        4366,
        p21::Complex::new([
            p21::leaf("LENGTH_UNIT", vec![]),
            p21::leaf("NAMED_UNIT", vec![A::Derived]),
            p21::leaf(
                "SI_UNIT",
                vec![A::Enum("CENTI".into()), A::Enum("METRE".into())],
            ),
        ])
        .unwrap()
        .into_record(),
    );
    let doc = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
    let (_, drawn) = read(&doc);
    let graphics = unique(&drawn);
    let text = &graphics["step:4389"].text_placements[0];
    near(text.origin, [100., 200., 300.]);
    assert_eq!(
        text.height,
        Some(45.),
        "style height takes precedence over the extent"
    );
    let plane = drawn.planes.iter().find(|p| p.id == "step:4391").unwrap();
    near(plane.origin, [100., 200., 300.]);
}

#[test]
fn saved_views_only_reference_their_own_graphics_once_despite_semantic_aliases() {
    let doc = mixed();
    let mut edit = p21::Edit::new();
    edit.add(p21::simple(
        "DRAUGHTING_MODEL",
        vec![
            A::String("only editable text".into()),
            A::List(vec![A::EntityRef(16), A::EntityRef(4391)]),
            A::EntityRef(4351),
        ],
    ));
    let other = edit.add(p21::simple(
        "DRAUGHTING_CALLOUT",
        vec![
            A::String("same literal in another callout".into()),
            A::List(vec![A::EntityRef(4388)]),
        ],
    ));
    edit.add(p21::simple(
        "DRAUGHTING_MODEL",
        vec![
            A::String("other graphic".into()),
            A::List(vec![other.to_ref()]),
            A::EntityRef(4351),
        ],
    ));
    let doc = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
    let (_, drawn) = read(&doc);
    // The camera's name is MBD_0 in both models; identities include the model.
    let subset = drawn
        .views
        .iter()
        .find(|v| v.graphics == ["step:4389"])
        .unwrap();
    assert!(subset.direction.is_some() && subset.up.is_some());
    assert!(
        drawn
            .views
            .iter()
            .any(|v| v.name == "MBD_0" && v.graphics.len() == 24)
    );
    let other = drawn
        .views
        .iter()
        .find(|v| v.name == "other graphic")
        .unwrap();
    assert_eq!(other.graphics.len(), 1);
    assert_ne!(
        other.graphics, subset.graphics,
        "shared literal geometry must not leak between views"
    );
    assert_eq!(
        drawn
            .views
            .iter()
            .map(|v| &v.id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        drawn.views.len()
    );
}

#[test]
fn old_documents_remain_valid_and_optional_references_are_checked() {
    let (_, drawn) = read(&mixed());
    let mut value = serde_json::to_value(drawn).unwrap();
    let mut old = value.clone();
    old.as_object_mut().unwrap().remove("planes");
    old.as_object_mut().unwrap().remove("views");
    for graphic in old["items"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .flat_map(|g| g.as_array_mut().unwrap())
    {
        for key in ["id", "plane_id", "text_placements"] {
            graphic.as_object_mut().unwrap().remove(key);
        }
    }
    for graphic in old["unlinked"].as_array_mut().unwrap() {
        for key in ["id", "plane_id", "text_placements"] {
            graphic.as_object_mut().unwrap().remove(key);
        }
    }
    presentation::validate(&old).unwrap();
    let decoded: Presentation = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), old);
    value["views"][0]["graphics"][0] = json!("absent graphic");
    assert!(
        presentation::validate(&value)
            .unwrap_err()
            .to_string()
            .contains("unknown graphic")
    );
    let mut value: Value = serde_json::to_value(read(&mixed()).1).unwrap();
    value["unlinked"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|g| g["id"] == "step:4389")
        .unwrap()["text_placements"][0]["text_index"] = json!(99);
    assert!(
        presentation::validate(&value)
            .unwrap_err()
            .to_string()
            .contains("index out of range")
    );
}

#[cfg(feature = "presentation")]
#[test]
fn generated_text_metadata_matches_the_placed_glyph_ink() {
    use haecceity::pmi::*;
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/ties/cube.step"
    ))
    .unwrap();
    let parts = read_part_definitions(&bytes).unwrap();
    let model = PartPmi {
        features: vec![Feature::Items(vec![Anchor::Face(FaceIndex(0))])],
        dimensions: vec![Dimension {
            kind: DimensionKind::Size {
                feature: FeatureId(0),
                kind: SizeKind::Thickness,
                path: None,
                angle: None,
            },
            nominal: Some(haecceity::pmi::Value::length(
                Decimal::parse("10.").unwrap(),
                LengthUnit::Millimetre,
            )),
            tolerance: DimTolerance::None,
            qualifier: None,
            modifiers: vec![],
            principle: None,
        }],
        ..PartPmi::default()
    };
    let callout = haecceity::presentation::dimension(&model.dimensions[0]);
    let semantic = json::identify(model, None).unwrap();
    let part = haecceity::step::read_part(&bytes, &parts[0]).unwrap();
    let generated = presentation::generated(&semantic, &part).unwrap();
    let graphic = &generated.items[&semantic.ids["dimensions"][0]][0];
    let text = &graphic.text_placements[0];
    let mut ink = haecceity::geom::Bounds::empty();
    for p in graphic
        .polylines
        .iter()
        .skip(10) // Measured linear dimensions precede lettering with ten witness/arrow lines.
        .take(callout.polylines().len())
        .flatten()
    {
        ink.add(*p);
    }
    near(text.origin, ink.centre());
    assert!(text.height.unwrap() > 0.);
    assert_eq!(text.stroked, Some(true));
    let encoded = serde_json::to_value(&generated).unwrap();
    presentation::validate(&encoded).unwrap();
    assert_eq!(
        serde_json::from_value::<Presentation>(encoded).unwrap(),
        generated
    );
    assert_eq!(generated.views.len(), generated.planes.len());
    for plane in &generated.planes {
        let view = generated
            .views
            .iter()
            .find(|v| v.id == format!("{}/view", plane.id))
            .unwrap();
        near(view.direction.unwrap(), plane.normal.map(|n| -n));
        near(
            view.up.unwrap(),
            haecceity::geom::cross(plane.normal, plane.x_axis),
        );
        assert_eq!(view.graphics.len(), 1);
        assert_eq!(view.graphics[0], graphic.id.as_deref().unwrap());
    }
    assert_eq!(text.alignment.as_deref(), Some("centre"));
    assert!(
        generated
            .planes
            .iter()
            .any(|plane| Some(&plane.id) == graphic.plane_id.as_ref())
    );
}

#[test]
fn stroked_is_optional_boolean_independent_of_graphic_source() {
    let (_, drawn) = read(&mixed());
    let mut value = serde_json::to_value(drawn).unwrap();
    let literal = value["unlinked"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|g| g["id"] == "step:4389")
        .unwrap();
    literal["source"] = json!("generated");
    assert!(literal["text_placements"][0].get("stroked").is_none());
    for flag in [json!(true), json!(false)] {
        let literal = value["unlinked"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|g| g["id"] == "step:4389")
            .unwrap();
        literal["text_placements"][0]["stroked"] = flag;
        presentation::validate(&value).unwrap();
        let decoded: Presentation = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), value);
    }
    let literal = value["unlinked"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|g| g["id"] == "step:4389")
        .unwrap();
    literal["text_placements"][0]["stroked"] = json!("yes");
    assert!(presentation::validate(&value).is_err());
}
