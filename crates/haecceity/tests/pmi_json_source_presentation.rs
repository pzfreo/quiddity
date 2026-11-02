#![cfg(feature = "json-import")]
use haecceity::{
    json::{
        self, Anchors,
        presentation::{self, Presentation},
    },
    p21::Document,
    pmi::{self, *},
    step::read_part_definitions,
};
use serde_json::json;
use std::io::Read;

fn cube() -> (
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
fn graphic(origin: [f64; 3]) -> serde_json::Value {
    json!({"source":"file", "name":"source", "polylines":[], "triangles":[], "text":[], "step_ids":[], "unsupported":[], "plane_id":"source-plane", "leader":[1.,2.,3.], "text_placements":[{"origin":origin,"x_axis":[1.,0.,0.],"normal":[0.,0.,1.],"height":2.5}]})
}
fn input(semantic: &json::Semantic) -> Presentation {
    serde_json::from_value(json!({"schema":"haecceity.presentation/1","length_unit":"millimetre","semantic_ids":[], "items": {
        semantic.ids["dimensions"][0].clone(): [graphic([80.,90.,70.])],
        semantic.ids["notes"][0].clone(): [graphic([100.,110.,70.])],
        semantic.ids["notes"][1].clone(): [graphic([140.,110.,70.])],
    }, "unlinked":[], "planes":[{"id":"source-plane","name":"source","origin":[0.,0.,70.],"x_axis":[1.,0.,0.],"normal":[0.,0.,1.]}]})).unwrap()
}
fn read_placed(bytes: Vec<u8>) -> Presentation {
    let defs = read_part_definitions(&bytes).unwrap();
    let doc = Document::parse(bytes).unwrap();
    let read = pmi::read(&doc, &defs).unwrap();
    let semantic = json::identify(read.parts[0].clone(), Some(&read.identifiers.parts[0])).unwrap();
    presentation::read(&doc, &read, PartId(0), &semantic).unwrap()
}
fn write(
    doc: &Document,
    defs: &[haecceity::step::PartDefinition],
    semantic: &json::Semantic,
    source: Presentation,
) -> json::Result<(Vec<u8>, pmi::write::WriteReport)> {
    json::write_with_presentation(
        doc,
        defs,
        &[(PartId(0), semantic.clone())],
        pmi::write::PresentationPolicy::RemovePresentation,
        &[(PartId(0), source)],
    )
}
#[test]
fn positions_planes_leaders_and_individual_notes_read_back() {
    let (doc, defs, semantic) = cube();
    let source = input(&semantic);
    presentation::validate(&serde_json::to_value(&source).unwrap()).unwrap();
    let (bytes, _) = write(&doc, &defs, &semantic, source.clone()).unwrap();
    let read = read_placed(bytes);
    for (id, gs) in &source.items {
        assert_eq!(
            read.items[id].len(),
            1,
            "{id} has a duplicate generated callout"
        );
        let g = &read.items[id][0];
        assert_eq!(g.text_placements[0].origin, gs[0].text_placements[0].origin);
        assert_eq!(g.text_placements[0].height, Some(2.5));
        assert_eq!(g.leader, Some([1., 2., 3.]));
        let plane = read
            .planes
            .iter()
            .find(|p| Some(&p.id) == g.plane_id.as_ref())
            .unwrap();
        assert_eq!(plane.origin, source.planes[0].origin);
        assert_eq!(plane.normal, source.planes[0].normal);
        assert_eq!(plane.x_axis, source.planes[0].x_axis);
        assert_ne!(g.name, "General notes");
    }
    #[cfg(feature = "presentation")]
    assert!(
        read.items[&semantic.ids["notes"][2]][0]
            .name
            .contains("General notes")
    );
}
#[test]
fn bad_source_is_refused_with_item_and_original_is_untouched() {
    let (doc, defs, semantic) = cube();
    let original = doc.bytes().to_vec();
    let id = &semantic.ids["dimensions"][0];
    for fault in 0..5 {
        let mut source = input(&semantic);
        match fault {
            0 => {
                source.items.insert("unknown-item".into(), vec![]);
            }
            1 => source.planes[0].normal = [0.; 3],
            2 => source.planes[0].x_axis = [0., 0., 1.],
            3 => source.items.get_mut(id).unwrap()[0].text_placements.clear(),
            _ => source.items.get_mut(id).unwrap()[0].text_placements[0].x_axis = [0.; 3],
        }
        let error = write(&doc, &defs, &semantic, source)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(if fault == 0 { "unknown-item" } else { id }),
            "{error}"
        );
        assert_eq!(doc.bytes(), original);
    }
}
#[test]
fn edge_on_and_overlap_warn_without_moving_text() {
    let (doc, defs, semantic) = cube();
    let mut source = input(&semantic);
    let dim = &semantic.ids["dimensions"][0];
    let note = &semantic.ids["notes"][0];
    source.items.get_mut(note).unwrap()[0].text_placements[0].origin = [80., 90., 70.];
    let (bytes, report) = write(&doc, &defs, &semantic, source.clone()).unwrap();
    assert!(
        report
            .presentation_warnings
            .iter()
            .any(|s| s.contains("overlaps")),
        "{report:?}"
    );
    assert_eq!(
        read_placed(bytes).items[dim][0].text_placements[0].origin,
        [80., 90., 70.]
    );
    source.items.get_mut(dim).unwrap()[0].text_placements[0].normal = [0., 1., 0.];
    let (_, report) = write(&doc, &defs, &semantic, source).unwrap();
    assert!(
        report
            .presentation_warnings
            .iter()
            .any(|s| s.contains(dim) && s.contains("edge-on"))
    );
}
#[test]
fn rust_route_accepts_source_and_empty_companion_keeps_existing_bytes() {
    let (doc, defs, semantic) = cube();
    let pmi = [(PartId(0), semantic.pmi.clone())];
    let policy = pmi::write::PresentationPolicy::RemovePresentation;
    let (old, _) = pmi::write(&doc, &defs, &pmi, pmi::Mode::Add, policy).unwrap();
    let (new, _) = pmi::write::write_with_presentation(
        &doc,
        &defs,
        &pmi,
        pmi::Mode::Add,
        policy,
        &[(PartId(0), Presentation::default())],
    )
    .unwrap();
    assert_eq!(
        doc.apply(&old).unwrap().bytes,
        doc.apply(&new).unwrap().bytes
    );
    // Rust migrates legacy notes before assigning stable ids.
    let mut source = input(&semantic);
    source
        .items
        .retain(|id, _| id == &semantic.ids["dimensions"][0]);
    let (edit, _) = pmi::write::write_with_presentation(
        &doc,
        &defs,
        &pmi,
        pmi::Mode::Add,
        policy,
        &[(PartId(0), source)],
    )
    .unwrap();
    let read = read_placed(doc.apply(&edit).unwrap().bytes);
    assert!(read.items.values().flatten().any(|g| {
        g.text_placements
            .iter()
            .any(|p| p.origin == [80., 90., 70.])
    }));
}
#[test]
fn amys_source_companion_and_handmade_flag_note_roundtrip() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xcaf");
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(std::fs::File::open(root.join("pmi_plate.geom.step.gz")).unwrap())
        .read_to_end(&mut bytes)
        .unwrap();
    let defs = read_part_definitions(&bytes).unwrap();
    let doc = Document::parse(bytes).unwrap();
    let semantic = json::from_str(
        &std::fs::read_to_string(root.join("pmi_plate.source.pmi.json")).unwrap(),
        &Anchors::read(&doc, &defs[0]),
    )
    .unwrap();
    let source: Presentation = serde_json::from_str(
        &std::fs::read_to_string(root.join("pmi_plate.source.presentation.json")).unwrap(),
    )
    .unwrap();
    let (bytes, _) = write(&doc, &defs, &semantic, source.clone()).unwrap();
    let read = read_placed(bytes.clone());
    assert_eq!(source.items.len(), 11);
    for (id, gs) in &source.items {
        assert_eq!(
            read.items[id][0].text_placements[0].origin, gs[0].text_placements[0].origin,
            "{id}"
        );
        let plane = read
            .planes
            .iter()
            .find(|p| Some(&p.id) == read.items[id][0].plane_id.as_ref())
            .unwrap();
        let expected = source
            .planes
            .iter()
            .find(|p| Some(&p.id) == gs[0].plane_id.as_ref())
            .unwrap();
        assert_eq!(plane.origin, expected.origin, "{id}");
        assert_eq!(plane.normal, expected.normal, "{id}");
        assert_eq!(plane.x_axis, expected.x_axis, "{id}");
    }
    let mut handmade = source.clone();
    let mut flag = source.items["hole-diameter"][0].clone();
    flag.text_placements[0].origin = [25., 35., 5.];
    flag.leader = Some([5., 0., 5.]);
    let note = semantic.ids["notes"][1].clone();
    handmade.items.insert(note.clone(), vec![flag]);
    let (placed, _) = write(&doc, &defs, &semantic, handmade).unwrap();
    assert_eq!(
        read_placed(placed.clone()).items[&note][0].text_placements[0].origin,
        [25., 35., 5.]
    );
    // Optional local exports for the OCCT cross-check; never golden expectations.
    if let Ok(dir) = std::env::var("QUIDDITY_SOURCE_PLACEMENT_EXPORT") {
        std::fs::write(std::path::Path::new(&dir).join("plate-source.step"), bytes).unwrap();
        std::fs::write(std::path::Path::new(&dir).join("plate-flag.step"), placed).unwrap();
    }
}
