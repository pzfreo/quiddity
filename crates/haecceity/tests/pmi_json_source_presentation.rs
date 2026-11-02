#![cfg(all(feature = "json-import", feature = "presentation"))]
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
fn ink_bounds(lines: &[Vec<[f64; 3]>]) -> haecceity::geom::Bounds {
    assert!(!lines.is_empty(), "missing tessellated ink");
    let mut bounds = haecceity::geom::Bounds::empty();
    for &p in lines.iter().flatten() {
        bounds.add(p);
    }
    assert!(bounds.max_extent().is_finite());
    bounds
}
fn assert_point(actual: [f64; 3], expected: [f64; 3]) {
    for k in 0..3 {
        assert!(
            (actual[k] - expected[k]).abs() < 1e-9,
            "{actual:?} != {expected:?}"
        );
    }
}
fn assert_bottom_edge(end: [f64; 3], bounds: haecceity::geom::Bounds) {
    assert!((end[1] - bounds.min[1]).abs() < 1e-9);
    assert!((end[2] - bounds.min[2]).abs() < 1e-9);
    assert!(end[0] >= bounds.min[0] - 1e-9 && end[0] <= bounds.max[0] + 1e-9);
}

#[test]
fn positions_planes_leaders_and_individual_notes_read_back() {
    let (doc, defs, semantic) = cube();
    let source = input(&semantic);
    presentation::validate(&serde_json::to_value(&source).unwrap()).unwrap();
    let (bytes, _) = write(&doc, &defs, &semantic, source.clone()).unwrap();
    let read = read_placed(bytes);
    {
        let callout = haecceity::presentation::dimension(&semantic.pmi.dimensions[0]);
        let ink = &read.items[&semantic.ids["dimensions"][0]][0].polylines;
        let count = callout.polylines().len();
        assert_eq!(
            ink.len(),
            count + 1,
            "all glyph/frame strokes and one leader"
        );
        let text = ink_bounds(&ink[callout.frame_strokes..count]);
        assert_point(text.centre(), [80., 90., 70.]);
        let frame = ink_bounds(&ink[..callout.frame_strokes]);
        let leader = &ink[count];
        assert_point(leader[0], [1., 2., 3.]);
        assert_bottom_edge(*leader.last().unwrap(), frame);
    }
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
    for fault in 0..6 {
        let mut source = input(&semantic);
        match fault {
            0 => {
                source.items.insert("unknown-item".into(), vec![]);
            }
            1 => source.planes[0].normal = [0.; 3],
            2 => source.planes[0].x_axis = [0., 0., 1.],
            3 => source.items.get_mut(id).unwrap()[0].text_placements.clear(),
            4 => source.items.get_mut(id).unwrap()[0].text_placements[0].x_axis = [0.; 3],
            _ => {
                source.planes[0].normal = [1.; 3];
                source.planes[0].x_axis = [1.; 3];
            }
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
    let canonical = json::identify(pmi::write::standard_form(&semantic.pmi), None).unwrap();
    source.items.insert(
        canonical.ids["attributes"][0].clone(),
        vec![serde_json::from_value(graphic([100., 110., 70.])).unwrap()],
    );
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
    // The ordinary Rust route retains native STEP reading: the migrated
    // semantic-text property can read as a Note rather than an AttributeSet.
    assert!(read.items.iter().any(|(id, graphics)| {
        (id.starts_with("notes:") || id.starts_with("attributes:"))
            && graphics.iter().any(|g| {
                g.name.contains("MASK BEFORE COATING")
                    && g.text_placements
                        .iter()
                        .any(|p| p.origin == [100., 110., 70.])
            })
    }));
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

#[test]
fn text_frame_can_supply_the_plane_and_later_view_variants_use_main_position() {
    let (doc, defs, semantic) = cube();
    let mut source = input(&semantic);
    let dim = semantic.ids["dimensions"][0].clone();
    source.items.retain(|id, _| id == &dim);
    source.planes.clear();
    let main = &mut source.items.get_mut(&dim).unwrap()[0];
    main.plane_id = None;
    let mut variant = main.clone();
    variant.text_placements[0].origin = [999., 998., 997.];
    source.items.get_mut(&dim).unwrap().push(variant);
    let (bytes, _) = write(&doc, &defs, &semantic, source).unwrap();
    let read = read_placed(bytes);
    let g = &read.items[&dim][0];
    assert_eq!(g.text_placements[0].origin, [80., 90., 70.]);
    let plane = read
        .planes
        .iter()
        .find(|p| Some(&p.id) == g.plane_id.as_ref())
        .unwrap();
    assert_eq!(plane.origin, [80., 90., 70.]);
}

#[test]
fn malformed_text_position_names_its_semantic_owner() {
    let (_, _, semantic) = cube();
    let mut value = serde_json::to_value(input(&semantic)).unwrap();
    let id = &semantic.ids["dimensions"][0];
    value["items"][id][0]["text_placements"][0]
        .as_object_mut()
        .unwrap()
        .remove("origin");
    let error = presentation::from_value(value).unwrap_err().to_string();
    assert!(error.contains(id) && error.contains("origin"), "{error}");
}

#[test]
fn supplied_leaders_clamp_to_the_frame_towards_the_tip() {
    let (doc, defs, semantic) = cube();
    let id = &semantic.ids["dimensions"][0];
    for tip in [[20., 60., 70.], [80., 60., 70.], [140., 60., 70.]] {
        let mut source = input(&semantic);
        source.items.retain(|key, _| key == id);
        source.items.get_mut(id).unwrap()[0].leader = Some(tip);
        let (bytes, _) = write(&doc, &defs, &semantic, source).unwrap();
        let read = read_placed(bytes);
        let lines = &read.items[id][0].polylines;
        let callout = haecceity::presentation::dimension(&semantic.pmi.dimensions[0]);
        assert_eq!(lines.len(), callout.polylines().len() + 1);
        let frame = ink_bounds(&lines[..callout.frame_strokes]);
        let leader = lines.last().unwrap();
        assert_point(leader[0], tip);
        let end = *leader.last().unwrap();
        assert_bottom_edge(end, frame);
        assert!((end[0] - tip[0].clamp(frame.min[0], frame.max[0])).abs() < 1e-9);
    }
}

#[test]
fn all_supported_alignments_place_actual_text_ink_at_the_anchor() {
    let (doc, defs, semantic) = cube();
    let id = &semantic.ids["notes"][1];
    let origin = [100., 110., 70.];
    let cases = [
        (None, [0.5, 0.5]),
        (Some("centre"), [0.5, 0.5]),
        (Some("center"), [0.5, 0.5]),
        (Some("middle centre"), [0.5, 0.5]),
        (Some("middle center"), [0.5, 0.5]),
        (Some("baseline left"), [0., 0.]),
        (Some("bottom left"), [0., 0.]),
        (Some("baseline centre"), [0.5, 0.]),
        (Some("baseline center"), [0.5, 0.]),
        (Some("bottom centre"), [0.5, 0.]),
        (Some("bottom center"), [0.5, 0.]),
        (Some("baseline right"), [1., 0.]),
        (Some("bottom right"), [1., 0.]),
        (Some("middle left"), [0., 0.5]),
        (Some("middle right"), [1., 0.5]),
        (Some("top left"), [0., 1.]),
        (Some("top centre"), [0.5, 1.]),
        (Some("top center"), [0.5, 1.]),
        (Some("top right"), [1., 1.]),
        (Some("Top_Right"), [1., 1.]),
    ];
    for (alignment, fraction) in cases {
        let mut source = input(&semantic);
        source.items.retain(|key, _| key == id);
        let g = &mut source.items.get_mut(id).unwrap()[0];
        g.leader = None;
        let text = &mut g.text_placements[0];
        text.origin = origin;
        text.normal = [1., 0., 0.];
        text.x_axis = [0., 1., 0.];
        text.alignment = alignment.map(str::to_owned);
        source.planes[0].normal = text.normal;
        source.planes[0].x_axis = text.x_axis;
        source.planes[0].origin = [100., 0., 0.];
        let (bytes, _) = write(&doc, &defs, &semantic, source).unwrap();
        let read = read_placed(bytes);
        let g = &read.items[id][0];
        assert_eq!(
            g.polylines.len(),
            haecceity::presentation::label(&semantic.pmi.notes[1].text)
                .polylines()
                .len()
        );
        let bounds = ink_bounds(&g.polylines);
        // Text X is model Y; text Y is model Z. The tested point comes only from ink.
        let aligned = [
            bounds.centre()[0],
            bounds.min[1] + fraction[0] * (bounds.max[1] - bounds.min[1]),
            bounds.min[2] + fraction[1] * (bounds.max[2] - bounds.min[2]),
        ];
        assert_point(aligned, origin);
        assert_eq!(g.text_placements[0].alignment.as_deref(), alignment);
    }
}

#[test]
fn unsupported_alignment_is_atomic_and_names_the_item() {
    let (doc, defs, semantic) = cube();
    let original = doc.bytes().to_vec();
    let id = &semantic.ids["dimensions"][0];
    let mut source = input(&semantic);
    source.items.get_mut(id).unwrap()[0].text_placements[0].alignment = Some("justify".into());
    let error = write(&doc, &defs, &semantic, source)
        .unwrap_err()
        .to_string();
    assert!(error.contains(id) && error.contains("alignment"), "{error}");
    assert_eq!(doc.bytes(), original);
}

#[test]
fn two_placements_on_a_shared_datum_symbol_are_refused() {
    let (doc, defs, semantic) = cube();
    let mut model = semantic.pmi;
    model
        .datums
        .push(Datum::new(DatumLabel::new("B").unwrap(), Some(FeatureId(0)), vec![]).unwrap());
    let semantic = json::identify(model, None).unwrap();
    let mut source = input(&semantic);
    source.items.clear();
    let a = &semantic.ids["datums"][0];
    let b = &semantic.ids["datums"][1];
    source.items.insert(
        a.clone(),
        vec![serde_json::from_value(graphic([80., 90., 70.])).unwrap()],
    );
    source.items.insert(
        b.clone(),
        vec![serde_json::from_value(graphic([100., 90., 70.])).unwrap()],
    );
    let original = doc.bytes().to_vec();
    let error = write(&doc, &defs, &semantic, source)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(a) && error.contains(b) && error.contains("shares a callout definition"),
        "{error}"
    );
    assert_eq!(doc.bytes(), original);
}

#[test]
fn asymmetric_gdt_ink_honours_the_supplied_alignment() {
    let (doc, defs, semantic) = cube();
    let mut model = semantic.pmi;
    model.tolerances.push(GeometricTolerance {
        kind: ToleranceKind::LineProfile,
        target: ToleranceTarget::Feature(FeatureId(0)),
        magnitude: Some(Value::length(
            Decimal::parse("0.02").unwrap(),
            LengthUnit::Millimetre,
        )),
        zone: None,
        modifiers: vec![],
        unit_basis: None,
        maximum: None,
        unequal: None,
        datums: None,
        auxiliary: vec![],
        description: None,
    });
    let semantic = json::identify(model, None).unwrap();
    let id = &semantic.ids["tolerances"][0];
    for (alignment, fraction) in [(None, 0.5), (Some("top right"), 1.)] {
        let mut source = input(&semantic);
        source.items.clear();
        let mut g: presentation::Graphic =
            serde_json::from_value(graphic([80., 90., 70.])).unwrap();
        g.leader = None;
        g.text_placements[0].alignment = alignment.map(str::to_owned);
        source.items.insert(id.clone(), vec![g]);
        let (bytes, _) = write(&doc, &defs, &semantic, source).unwrap();
        let read = read_placed(bytes);
        let lines = &read.items[id][0].polylines;
        let callout =
            haecceity::presentation::tolerance(&semantic.pmi.tolerances[0], &semantic.pmi);
        assert_eq!(lines.len(), callout.polylines().len());
        // The profile characteristic is the first arc, after frame/other straight strokes.
        let bounds = ink_bounds(&lines[callout.strokes.len()..callout.strokes.len() + 1]);
        assert_point(
            [
                bounds.min[0] + fraction * (bounds.max[0] - bounds.min[0]),
                bounds.min[1] + fraction * (bounds.max[1] - bounds.min[1]),
                bounds.centre()[2],
            ],
            [80., 90., 70.],
        );
    }
}

#[test]
fn boxed_datum_lettering_is_not_clipped_for_alignment() {
    let (doc, defs, semantic) = cube();
    let mut model = semantic.pmi;
    model.datums[0] = Datum::new(
        DatumLabel::new("LONG_LABEL").unwrap(),
        Some(FeatureId(0)),
        vec![],
    )
    .unwrap();
    let semantic = json::identify(model, None).unwrap();
    let id = &semantic.ids["datums"][0];
    let mut source = input(&semantic);
    source.items.clear();
    let mut g: presentation::Graphic = serde_json::from_value(graphic([80., 90., 70.])).unwrap();
    g.leader = None;
    g.text_placements[0].alignment = Some("top right".into());
    source.items.insert(id.clone(), vec![g]);
    let (bytes, _) = write(&doc, &defs, &semantic, source).unwrap();
    let read = read_placed(bytes);
    let lines = &read.items[id][0].polylines;
    let callout = haecceity::presentation::datum("LONG_LABEL");
    assert_eq!(lines.len(), callout.polylines().len());
    let bounds = ink_bounds(&lines[callout.frame_strokes..]);
    assert_point(bounds.max, [80., 90., 70.]);
}
