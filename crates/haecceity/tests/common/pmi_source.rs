//! Synthetic source-presentation fixture shared with the writer/OCCT goldens.
use haecceity::{
    json::{self, presentation::Presentation},
    p21::Document,
    pmi::*,
    step::read_part_definitions,
};
use serde_json::json;

pub fn cube() -> (
    Document,
    Vec<haecceity::step::PartDefinition>,
    json::Semantic,
) {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/ties/cube.step"
    ))
    .unwrap();
    let defs = read_part_definitions(&bytes).unwrap();
    let doc = Document::parse(bytes).unwrap();
    let p = PartPmi {
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
        notes: vec![
            Note {
                kind: "general note".into(),
                text: "FLAG 1 MASK BEFORE COATING".into(),
                on: Some(NoteOwner::Feature(FeatureId(0))),
                callouts: vec![],
            },
            Note {
                kind: "general note".into(),
                text: "PLACED GENERAL NOTE".into(),
                on: None,
                callouts: vec![],
            },
            Note {
                kind: "general note".into(),
                text: "UNPLACED GENERAL NOTE".into(),
                on: None,
                callouts: vec![],
            },
        ],
        ..Default::default()
    };
    (doc, defs, json::identify(p, None).unwrap())
}
pub fn graphic(origin: [f64; 3]) -> serde_json::Value {
    json!({"source":"file", "name":"source", "polylines":[], "triangles":[], "text":[], "step_ids":[], "unsupported":[], "plane_id":"source-plane", "leader":[1.,2.,3.], "text_placements":[{"origin":origin,"x_axis":[1.,0.,0.],"normal":[0.,0.,1.],"height":2.5}]})
}
pub fn input(semantic: &json::Semantic) -> Presentation {
    serde_json::from_value(json!({"schema":"haecceity.presentation/1","length_unit":"millimetre","semantic_ids":[], "items": {
        semantic.ids["dimensions"][0].clone(): [graphic([80.,90.,70.])],
        semantic.ids["notes"][0].clone(): [graphic([100.,110.,70.])],
        semantic.ids["notes"][1].clone(): [graphic([140.,110.,70.])],
    }, "unlinked":[], "planes":[{"id":"source-plane","name":"source","origin":[0.,0.,70.],"x_axis":[1.,0.,0.],"normal":[0.,0.,1.]}]})).unwrap()
}

pub fn written() -> Vec<u8> {
    let (doc, defs, semantic) = cube();
    let mut source = input(&semantic);
    let mut datum = source.items[&semantic.ids["dimensions"][0]][0].clone();
    datum.text_placements[0].origin = [60., 110., 70.];
    source
        .items
        .insert(semantic.ids["datums"][0].clone(), vec![datum]);
    for graphics in source.items.values_mut() {
        graphics[0].text_placements[0].alignment = Some("top right".into());
    }
    json::write_with_presentation(
        &doc,
        &defs,
        &[(PartId(0), semantic)],
        haecceity::pmi::write::PresentationPolicy::RemovePresentation,
        &[(PartId(0), source)],
    )
    .unwrap()
    .0
}
