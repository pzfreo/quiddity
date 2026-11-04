#![cfg(all(feature = "json-import", feature = "presentation"))]
mod common;
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
    common::pmi_conformance::assert_conforms(&Document::parse(bytes.clone()).unwrap());
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

// Shared geometry assertions inspect strokes, independently of layout cells.
fn projected_ink_overlap(a: &[Vec<[f64; 3]>], b: &[Vec<[f64; 3]>], view: [f64; 3]) -> bool {
    common::ink::within_clearance(a, b, view, 1e-8)
}

#[test]
fn ctc_01_notes_block_clears_kept_callout_ink() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/ap242/nist/nist_ctc_01_asme1_ap242-e1.stp.gz");
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap())
        .read_to_end(&mut bytes)
        .unwrap();
    let before = read_placed(bytes.clone());
    let defs = read_part_definitions(&bytes).unwrap();
    let doc = Document::parse(bytes).unwrap();
    // Part settings from the committed nist_ctc_01_merge Specify intent.
    let pmi = PartPmi {
        material: Some(Material {
            id: "Aluminium 6082-T6".into(),
            name: None,
            density: None,
        }),
        general: vec![GeneralTolerance::Class {
            text: "ISO 2768-m".into(),
            standard: pmi::standards::Iso2768::recognise("ISO 2768-m"),
        }],
        notes: vec![
            Note {
                kind: "surface finish".into(),
                text: "Ra 3.2".into(),
                on: None,
                callouts: vec![],
            },
            Note {
                kind: "edge condition".into(),
                text: "Break sharp edges 0.2 max".into(),
                on: None,
                callouts: vec![],
            },
        ],
        ..Default::default()
    };
    let mut rebuilt = pmi::read(&doc, &defs).unwrap().parts.remove(0);
    // The source has four nominal-less dimensions the typed writer refuses;
    // keep the eight stated dimensions for the regeneration case, without
    // inventing values. Its controls all own features, not dimension indices.
    assert!(
        rebuilt
            .tolerances
            .iter()
            .all(|t| !matches!(t.target, ToleranceTarget::Dimension(_)))
    );
    rebuilt.dimensions.retain(|d| d.nominal.is_some());
    assert_eq!(rebuilt.dimensions.len(), 8);
    rebuilt.material = pmi.material.clone();
    rebuilt.general.extend(pmi.general.clone());
    rebuilt.notes.extend(pmi.notes.clone());
    let all = |p: Presentation| {
        p.items
            .into_values()
            .flatten()
            .chain(p.unlinked)
            .collect::<Vec<_>>()
    };
    let kept = all(before)
        .into_iter()
        .filter(|g| !g.polylines.is_empty())
        .collect::<Vec<_>>();
    assert!(kept.len() > 10, "must exercise the file's own callouts");
    for (name, requirements, mode) in [
        ("kept", pmi, pmi::write::Mode::Add),
        ("regenerated", rebuilt, pmi::write::Mode::Replace),
    ] {
        let (edit, _) = pmi::write(
            &doc,
            &defs,
            &[(PartId(0), requirements.clone())],
            mode,
            pmi::write::PresentationPolicy::RemovePresentation,
        )
        .unwrap();
        let output = doc.apply(&edit).unwrap().bytes;
        if let Ok(dir) = std::env::var("QUIDDITY_SOURCE_PLACEMENT_EXPORT") {
            std::fs::write(
                std::path::Path::new(&dir).join(format!("ctc-01-notes-{name}.step")),
                &output,
            )
            .unwrap();
        }
        let after = all(read_placed(output));
        let blocks = after
            .iter()
            .filter(|g| g.name == "General notes")
            .collect::<Vec<_>>();
        assert!(!blocks.is_empty(), "missing notes ink");
        let other = after
            .iter()
            .filter(|g| g.name != "General notes" && !g.polylines.is_empty())
            .collect::<Vec<_>>();
        for block in blocks {
            for view in haecceity::presentation::linear::VIEWS
                .into_iter()
                .filter(|v| v[1].abs() > 1e-8)
            {
                for graphic in &other {
                    assert!(
                        !projected_ink_overlap(&block.polylines, &graphic.polylines, view),
                        "{name}: notes overlap {} in {view:?}",
                        graphic.name
                    );
                }
                if mode == pmi::write::Mode::Add {
                    for graphic in &kept {
                        assert!(
                            !projected_ink_overlap(&block.polylines, &graphic.polylines, view),
                            "notes overlap kept {} in {view:?}",
                            graphic.name
                        );
                    }
                }
            }
        }
        if mode == pmi::write::Mode::Replace {
            let part = haecceity::step::read_part(doc.bytes(), &defs[0]).unwrap();
            let mut layout = haecceity::presentation::placement::Layout::new(part.bounds());
            let c = haecceity::presentation::notes::notes_block(
                &haecceity::presentation::notes::general_notes(&pmi::write::standard_form(
                    &requirements,
                )),
            );
            let baseline = layout.notes(&c);
            assert!(
                other.iter().any(|g| g.name.contains("⌀35")
                    && projected_ink_overlap(&baseline.lines, &g.polylines, [1., 1., -1.])),
                "old notes slot must overlap CTC-01's diameter ink"
            );
        }
    }
}

#[test]
fn amys_unplaced_flag_note_has_its_own_hole_leader() {
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
    let (bytes, _) = json::write_preserving(&doc, &defs, &[(PartId(0), semantic.clone())]).unwrap();
    let read = read_placed(bytes.clone());
    let note = &semantic.pmi.notes[1];
    let graphics = &read.items[&semantic.ids["notes"][1]];
    assert_eq!(
        graphics.len(),
        1,
        "one callout linked to the flag's property"
    );
    let graphic = &graphics[0];
    assert_eq!(graphic.name, note.text);
    let c = haecceity::presentation::label(&note.text);
    assert_eq!(
        graphic.polylines.len(),
        c.polylines().len() + 1,
        "text plus leader"
    );
    let Some(NoteOwner::Feature(feature)) = note.on else {
        panic!("flag must own a feature")
    };
    let part = haecceity::step::read_part(doc.bytes(), &defs[0]).unwrap();
    let (tip, _, face) =
        haecceity::presentation::placement::feature_site(&part, &semantic.pmi, feature).unwrap();
    let leader = graphic.polylines.last().unwrap();
    assert!(
        haecceity::geom::dist(leader[0], tip) < 2e-6,
        "leader must land inside the hole face {face}"
    );
    let Feature::Items(items) = &semantic.pmi.features[feature.0] else {
        panic!("face feature")
    };
    assert!(items.contains(&Anchor::Face(FaceIndex(face))));
    assert!(
        read.items[&semantic.ids["notes"][0]]
            .iter()
            .all(|g| g.name == "General notes"),
        "part note remains in the block"
    );
    if let Ok(dir) = std::env::var("QUIDDITY_SOURCE_PLACEMENT_EXPORT") {
        std::fs::write(
            std::path::Path::new(&dir).join("plate-unplaced.step"),
            bytes,
        )
        .unwrap();
    }
}

#[test]
fn notes_block_moves_past_a_kept_dimension_in_its_old_slot() {
    let (doc, defs, mut semantic) = cube();
    let mut source = input(&semantic);
    semantic.pmi.notes.clear();
    semantic = json::identify(semantic.pmi, None).unwrap();
    let settings = PartPmi {
        material: Some(Material {
            id: "Steel".into(),
            name: None,
            density: None,
        }),
        ..Default::default()
    };
    let part = haecceity::step::read_part(doc.bytes(), &defs[0]).unwrap();
    let mut layout = haecceity::presentation::placement::Layout::new(part.bounds());
    let c = haecceity::presentation::notes::notes_block(
        &haecceity::presentation::notes::general_notes(&settings),
    );
    let unreserved = layout.notes(&c);
    source
        .items
        .retain(|id, _| id == &semantic.ids["dimensions"][0]);
    source.planes[0].normal = [0., -1., 0.];
    let graphic = &mut source
        .items
        .get_mut(&semantic.ids["dimensions"][0])
        .unwrap()[0];
    // Put real leader ink into the old slot, rather than merely enclosing
    // the block in a dimension's empty frame or coordinate bounds.
    graphic.leader = Some(unreserved.lines[0][0]);
    graphic.text_placements[0].origin = ink_bounds(&unreserved.lines).centre();
    graphic.text_placements[0].normal = [0., -1., 0.];
    let (bytes, _) = write(&doc, &defs, &semantic, source).unwrap();
    let before = read_placed(bytes.clone());
    let dimension = &before.items[&semantic.ids["dimensions"][0]][0];
    assert!(
        projected_ink_overlap(&unreserved.lines, &dimension.polylines, [0., 1., 0.]),
        "fixture must obstruct the old notes slot"
    );
    let defs = read_part_definitions(&bytes).unwrap();
    let doc = Document::parse(bytes).unwrap();
    let (edit, _) = pmi::write(
        &doc,
        &defs,
        &[(PartId(0), settings)],
        pmi::write::Mode::Add,
        pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let after = read_placed(doc.apply(&edit).unwrap().bytes);
    let blocks = after
        .items
        .values()
        .flatten()
        .chain(&after.unlinked)
        .filter(|g| g.name == "General notes")
        .collect::<Vec<_>>();
    assert!(!blocks.is_empty());
    for block in blocks {
        assert!(
            !projected_ink_overlap(&block.polylines, &dimension.polylines, [0., 1., 0.]),
            "kept dimension must be an obstacle"
        );
    }
    assert_eq!(
        after.items[&semantic.ids["dimensions"][0]][0].polylines, dimension.polylines,
        "never move kept ink"
    );
}

#[test]
fn feature_text_without_drawable_ink_warns_without_losing_semantics() {
    let (doc, defs, semantic) = cube();
    for (text, reason) in [
        ("   ", "no visible text ink"),
        ("\u{f055}", "unsupported glyph"),
    ] {
        let mut model = semantic.pmi.clone();
        model.notes.truncate(1);
        model.notes[0].text = text.into();
        model.datums.clear();
        model.dimensions.clear();
        let semantic = json::identify(model, None).unwrap();
        let (bytes, report) =
            json::write_preserving(&doc, &defs, &[(PartId(0), semantic)]).unwrap();
        assert!(
            report
                .presentation_warnings
                .iter()
                .any(|w| w.contains("feature note") && w.contains(reason)),
            "{report:?}"
        );
        let defs = read_part_definitions(&bytes).unwrap();
        let doc = Document::parse(bytes).unwrap();
        let read = pmi::read(&doc, &defs).unwrap();
        assert!(
            read.parts[0]
                .notes
                .iter()
                .any(|n| n.text == text && n.on.is_some()),
            "retain the original annotation text"
        );
    }
}

#[test]
fn string_post_short_dimension_does_not_push_notes_out_of_empty_space() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/ap242/write-presentation/string_post_tapped_intent.step.gz");
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap())
        .read_to_end(&mut bytes)
        .unwrap();
    let read = read_placed(bytes.clone());
    let graphics: Vec<_> = read
        .items
        .values()
        .flatten()
        .chain(&read.unlinked)
        .collect();
    assert!(
        graphics.iter().any(|g| g.name == "1.2"),
        "exercise the short dimension from the review"
    );
    let blocks: Vec<_> = graphics
        .iter()
        .copied()
        .filter(|g| g.name == "General notes")
        .collect();
    assert!(!blocks.is_empty());
    let lines: Vec<_> = blocks.iter().flat_map(|g| g.polylines.clone()).collect();
    let defs = read_part_definitions(&bytes).unwrap();
    let part = haecceity::step::read_part(&bytes, &defs[0]).unwrap();
    let expected_top = part.bounds().min[2]
        - 2. * haecceity::presentation::placement::text_height(part.bounds().max_extent());
    assert!(
        (ink_bounds(&lines).max[2] - expected_top).abs() < 1e-9,
        "no slide is needed where the ink is clear"
    );
    for block in blocks {
        for other in graphics.iter().filter(|g| g.name != "General notes") {
            for view in haecceity::presentation::linear::VIEWS
                .into_iter()
                .filter(|v| v[1].abs() > 1e-8)
            {
                assert!(
                    !projected_ink_overlap(&block.polylines, &other.polylines, view),
                    "notes intersect {}",
                    other.name
                );
            }
        }
    }
}

#[test]
fn placed_notes_alignment_and_leaders_survive_two_writes() {
    let (doc, defs, semantic) = cube();
    let mut source = input(&semantic);
    for graphics in source.items.values_mut() {
        graphics[0].text_placements[0].alignment = Some("top right".into());
    }
    let (first, _) = write(&doc, &defs, &semantic, source).unwrap();
    let placed = read_placed(first.clone());
    let defs = read_part_definitions(&first).unwrap();
    let doc = Document::parse(first).unwrap();
    let read = pmi::read(&doc, &defs).unwrap();
    let semantic = json::identify(read.parts[0].clone(), Some(&read.identifiers.parts[0])).unwrap();
    let (second, _) = write(&doc, &defs, &semantic, placed.clone()).unwrap();
    let doc = Document::parse(second.clone()).unwrap();
    common::pmi_conformance::assert_conforms(&doc);
    let reread = read_placed(second.clone());
    for (id, graphics) in &placed.items {
        let first = &graphics[0];
        let next = &reread.items[id][0];
        assert_eq!(next.text_placements, first.text_placements, "{id}");
        assert_eq!(next.leader, first.leader, "{id}");
        assert_eq!(next.polylines, first.polylines, "{id}");
    }
    let read = pmi::read(&doc, &defs).unwrap();
    assert_eq!(read.parts[0].notes, semantic.pmi.notes);
    // Optional live XCAF probe, using the repository's existing OCP environment.
    if let Ok(path) = std::env::var("QUIDDITY_SFA_SYNTHETIC_OUTPUT") {
        std::fs::write(path, second).unwrap();
    }
}

#[test]
fn older_placeholder_names_still_supply_alignment() {
    let (doc, defs, semantic) = cube();
    let (bytes, _) = write(&doc, &defs, &semantic, input(&semantic)).unwrap();
    let doc = Document::parse(bytes).unwrap();
    let mut edit = haecceity::p21::Edit::new();
    for id in doc
        .ids()
        .filter(|&id| common::pmi_usage::is_a(&doc, id, "annotation_placeholder_occurrence"))
    {
        let mut record = doc.get(id).unwrap().clone();
        if let haecceity::p21::RawEntity::Simple { attributes, .. } = &mut record {
            attributes[0] =
                haecceity::p21::Attribute::String("haecceity placed text: top right".into());
        }
        edit.replace(id, record);
    }
    let read = read_placed(doc.apply(&edit).unwrap().bytes);
    assert!(
        read.items
            .values()
            .flatten()
            .flat_map(|g| &g.text_placements)
            .any(|p| p.alignment.as_deref() == Some("top right"))
    );
}

#[test]
fn conformance_check_detects_each_sfa_regression() {
    use haecceity::p21::{Attribute as A, Edit, RawEntity};
    let (doc, defs, semantic) = cube();
    let (bytes, _) = write(&doc, &defs, &semantic, input(&semantic)).unwrap();
    let doc = Document::parse(bytes).unwrap();
    let placeholder = doc
        .ids()
        .find(|&id| common::pmi_usage::is_a(&doc, id, "annotation_placeholder_occurrence"))
        .unwrap();
    let set = common::pmi_usage::reference(&doc, placeholder, "styled_item", "item").unwrap();
    let tao = doc
        .ids()
        .find(|&id| common::pmi_usage::is_a(&doc, id, "tessellated_annotation_occurrence"))
        .unwrap();
    let tset = common::pmi_usage::reference(&doc, tao, "styled_item", "item").unwrap();
    for fault in 0..6 {
        let mut edit = Edit::new();
        let id = match fault {
            0 => tao,
            1 | 2 => set,
            3 => tset,
            4 => placeholder,
            _ => tao,
        };
        let mut record = doc.get(id).unwrap().clone();
        let RawEntity::Simple {
            name, attributes, ..
        } = &mut record
        else {
            unreachable!()
        };
        match fault {
            0 => {
                *name = "ANNOTATION_CURVE_OCCURRENCE".into();
                let point = doc
                    .ids()
                    .find(|&id| common::pmi_usage::is_a(&doc, id, "cartesian_point"))
                    .unwrap();
                let line = edit.add(haecceity::p21::simple(
                    "POLYLINE",
                    vec![
                        A::String("".into()),
                        A::List(vec![A::EntityRef(point), A::EntityRef(point)]),
                    ],
                ));
                attributes[2] = line.to_ref();
            }
            1 => {
                if let A::List(members) = &mut attributes[1] {
                    members.retain(|a| !matches!(a, A::EntityRef(r) if common::pmi_usage::is_a(&doc, *r, "axis2_placement_3d")));
                }
            }
            2 => attributes[0] = A::String("".into()),
            3 => attributes[0] = A::String("feature note".into()),
            4 => attributes[0] = A::String("haecceity placed text: centre".into()),
            _ => {
                *name = "ANNOTATION_CURVE_OCCURRENCE".into();
                // Valid ACO item, but an invalid mixture with the other TAOs.
                let point = doc
                    .ids()
                    .find(|&id| common::pmi_usage::is_a(&doc, id, "cartesian_point"))
                    .unwrap();
                let line = edit.add(haecceity::p21::simple(
                    "POLYLINE",
                    vec![
                        A::String("".into()),
                        A::List(vec![A::EntityRef(point), A::EntityRef(point)]),
                    ],
                ));
                let set = edit.add(haecceity::p21::simple(
                    "GEOMETRIC_CURVE_SET",
                    vec![A::String("note".into()), A::List(vec![line.to_ref()])],
                ));
                attributes[2] = set.to_ref();
            }
        }
        edit.replace(id, record);
        let bad = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
        let issues = common::pmi_conformance::issues(&bad);
        let expected = [
            "item must be",
            "missing direct axis",
            "Table 18",
            "Table 18",
            "transport data",
            "mixed ACO",
        ][fault];
        assert!(
            issues.iter().any(|i| i.contains(expected)),
            "fault {fault}: {issues:?}"
        );
    }
    let no_ref = String::from_utf8(doc.bytes().to_vec()).unwrap().replace(
        common::pmi_conformance::PMI_RP,
        "unrelated header description",
    );
    assert!(
        common::pmi_conformance::issues(&Document::parse(no_ref.into_bytes()).unwrap())
            .iter()
            .any(|i| i.contains("§1.2"))
    );
}
