//! Bounds-relative regression evidence for generated and serialized notes.
#![cfg(all(feature = "presentation", feature = "json-import"))]
mod common;

use haecceity::{
    geom::Bounds,
    json,
    p21::Document,
    pmi::{self, PartId},
    presentation::{notes, placement::Layout},
    step,
};
use std::{io::Read, path::Path};

fn load(path: &str) -> Document {
    let bytes = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap();
    let mut raw = Vec::new();
    flate2::read::GzDecoder::new(bytes.as_slice())
        .read_to_end(&mut raw)
        .unwrap();
    Document::parse(raw).unwrap()
}
fn ink(lines: &[Vec<[f64; 3]>]) -> Bounds {
    let mut b = Bounds::empty();
    for &p in lines.iter().flatten() {
        b.add(p);
    }
    b
}
#[test]
fn notes_stay_beside_part_in_json_and_written_step() {
    for (name, path) in [
        ("spindle", "tests/fixtures/presentation/spindle.step.gz"),
        (
            "CTC-01",
            "../../tests/fixtures/ap242/nist/nist_ctc_01_asme1_ap242-e1.stp.gz",
        ),
        (
            "plate",
            "../../tests/fixtures/ap242/presentation-placement/plate.step.gz",
        ),
        ("cadgen plate", "tests/fixtures/xcaf/pmi_plate.geom.step.gz"),
    ] {
        let doc = load(path);
        let defs = step::read_part_definitions(doc.bytes()).unwrap();
        let read = pmi::read(&doc, &defs).unwrap();
        let mut semantic = if name == "cadgen plate" {
            json::from_str(
                &std::fs::read_to_string(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/fixtures/xcaf/pmi_plate.pmi.json"),
                )
                .unwrap(),
                &json::Anchors::read(&doc, &defs[0]),
            )
            .unwrap()
        } else {
            json::identify(read.parts[0].clone(), Some(&read.identifiers.parts[0])).unwrap()
        };
        if notes::general_notes(&semantic.pmi).is_empty() {
            // These files have no part settings: apply the spindle's settings
            // to exercise the same block on all four geometries.
            let source = load("tests/fixtures/presentation/spindle.step.gz");
            let source_defs = step::read_part_definitions(source.bytes()).unwrap();
            let source_read = pmi::read(&source, &source_defs).unwrap();
            semantic.pmi.general = source_read.parts[0].general.clone();
            semantic.pmi.surface_textures.extend(
                source_read.parts[0]
                    .surface_textures
                    .iter()
                    .filter(|t| t.on.is_none())
                    .cloned(),
            );
            semantic.pmi.attributes.extend(
                source_read.parts[0]
                    .attributes
                    .iter()
                    .filter(|a| a.on.is_none())
                    .cloned(),
            );
            semantic = json::identify(semantic.pmi, None).unwrap();
        }
        let part = step::read_part(doc.bytes(), &defs[0]).unwrap();
        let generated = json::presentation::generated(&semantic, &part).unwrap();
        assert_eq!(unique_notes(&generated).len(), 1);
        assert_notes_beside_and_clear(name, &part, &generated, &unique_notes(&generated));
        let mut writable = semantic.pmi.clone();
        // CTC-01 omits four nominal values; the writer correctly refuses those.
        writable.dimensions.retain(|d| d.nominal.is_some());
        let rows = notes::general_notes(&writable);
        let (edit, _) = pmi::write(
            &doc,
            &defs,
            &[(PartId(0), writable)],
            pmi::write::Mode::Replace,
            pmi::write::PresentationPolicy::RemovePresentation,
        )
        .unwrap();
        let written = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
        let written_defs = step::read_part_definitions(written.bytes()).unwrap();
        let written_read = pmi::read(&written, &written_defs).unwrap();
        let written_semantic = json::identify(
            written_read.parts[0].clone(),
            Some(&written_read.identifiers.parts[0]),
        )
        .unwrap();
        let drawn = json::presentation::read(&written, &written_read, PartId(0), &written_semantic)
            .unwrap();
        let serialized_notes = unique_notes(&drawn);
        assert_eq!(serialized_notes.len(), 1);
        assert_notes_beside_and_clear(name, &part, &drawn, &serialized_notes);
        // Exercise the production placement entry point with all final ink
        // reserved, as the writer does immediately before placing this block.
        // Do not reproduce the slide search here or assume an empty layout.
        let mut layout = Layout::new(part.bounds());
        let mut seen = std::collections::BTreeSet::new();
        for graphic in drawn.items.values().flatten().chain(&drawn.unlinked) {
            if graphic.name != "General notes" && seen.insert(graphic.id.clone()) {
                layout.reserve(&graphic.polylines);
            }
        }
        let expected = layout.notes(&notes::notes_block(&rows));
        for block in serialized_notes {
            assert!(
                strokes(&block.polylines) == strokes(&expected.lines),
                "{name}: writer JSON ink against occupied layout"
            );
            let root = block
                .id
                .as_ref()
                .unwrap()
                .strip_prefix("step:")
                .unwrap()
                .parse::<u64>()
                .unwrap();
            let actual = step_block_ink(&written, root);
            assert_eq!(
                actual.len(),
                block.polylines.len(),
                "{name}: STEP/JSON strip count"
            );
            assert!(
                strokes(&actual) == strokes(&block.polylines),
                "{name}: STEP ink against writer JSON"
            );
        }
    }
}

fn step_block_ink(doc: &Document, root: u64) -> Vec<Vec<[f64; 3]>> {
    use haecceity::p21::{Attribute as A, RawEntity};
    let number = |a: &A| match a {
        A::Real(x) => *x,
        A::Integer(x) => *x as f64,
        _ => panic!("non-numeric STEP coordinate"),
    };
    refs(doc, root)
        .into_iter()
        .flat_map(|occurrence| {
            assert!(is(doc, occurrence, "tessellated_annotation_occurrence"));
            let set = *refs(doc, occurrence).last().unwrap();
            let curve = refs(doc, set)[0];
            assert!(is(doc, curve, "tessellated_curve_set"));
            let coords = refs(doc, curve)[0];
            let Some(RawEntity::Simple { attributes, .. }) = doc.get(coords) else {
                panic!("coordinates list")
            };
            let Some(A::List(points)) = attributes.last() else {
                panic!("position coordinates")
            };
            let points: Vec<[f64; 3]> = points
                .iter()
                .map(|point| {
                    let A::List(xyz) = point else {
                        panic!("STEP point")
                    };
                    assert_eq!(xyz.len(), 3);
                    [number(&xyz[0]), number(&xyz[1]), number(&xyz[2])]
                })
                .collect();
            let Some(RawEntity::Simple { attributes, .. }) = doc.get(curve) else {
                panic!("curve set")
            };
            let Some(A::List(strips)) = attributes.last() else {
                panic!("line strips")
            };
            strips
                .iter()
                .map(|strip| {
                    let A::List(indices) = strip else {
                        panic!("curve strip")
                    };
                    indices
                        .iter()
                        .map(|index| {
                            let A::Integer(index) = index else {
                                panic!("curve index")
                            };
                            points[usize::try_from(*index).unwrap().checked_sub(1).unwrap()]
                        })
                        .collect()
                })
                .collect::<Vec<Vec<_>>>()
        })
        .collect()
}

fn assert_notes_beside_and_clear(
    name: &str,
    part: &haecceity::Part,
    drawn: &json::presentation::Presentation,
    blocks: &[&json::presentation::Graphic],
) {
    use haecceity::{geom, hlr, presentation::linear};
    let bounds = part.bounds();
    let gap = 2. * Layout::new(bounds).text_height();
    // Check the real part outline in the block's own readable front plane.
    // The bounds checks also rule out ink enclosed inside that outline.
    let front = hlr::View::new([0., -1., 0.], [0., 0., 1.]).unwrap();
    let outline: Vec<Vec<_>> = hlr::project(part, &front)
        .unwrap()
        .into_iter()
        .filter(|curve| curve.visible)
        .map(|curve| {
            curve
                .points
                .into_iter()
                .map(|p| geom::add(geom::scale(front.x, p[0]), geom::scale(front.y, p[1])))
                .collect()
        })
        .collect();
    assert!(
        !outline.is_empty(),
        "{name}: missing projected part outline"
    );
    for block in blocks {
        let b = ink(&block.polylines);
        assert!(
            (b.min[0] - bounds.max[0] - gap).abs() < 1e-6,
            "{name}: notes stay beside max X"
        );
        assert!(
            b.max[2] <= bounds.min[2] - gap + 1e-6,
            "{name}: notes stay below the part"
        );
        assert!(
            (b.min[1] - bounds.centre()[1]).abs() < 1e-6,
            "{name}: front-plane notes"
        );
        assert!(
            !common::ink::within_clearance(&block.polylines, &outline, front.toward, 1e-6),
            "{name}: notes overlap the part outline"
        );
        let mut seen = std::collections::BTreeSet::new();
        for other in drawn.items.values().flatten().chain(&drawn.unlinked) {
            if other.id == block.id || !seen.insert(other.id.clone()) {
                continue;
            }
            for view in linear::VIEWS.into_iter().filter(|v| v[1].abs() > 1e-8) {
                assert!(
                    !common::ink::within_clearance(&block.polylines, &other.polylines, view, 1e-6),
                    "{name}: notes overlap {} in {view:?}",
                    other.name
                );
            }
        }
    }
}

fn unique_notes(drawn: &json::presentation::Presentation) -> Vec<&json::presentation::Graphic> {
    let mut seen = std::collections::BTreeSet::new();
    drawn
        .items
        .values()
        .flatten()
        .chain(&drawn.unlinked)
        .filter(|g| g.name == "General notes" && seen.insert(g.id.clone()))
        .collect()
}

fn refs(doc: &Document, id: u64) -> Vec<u64> {
    let mut out = Vec::new();
    haecceity::p21::visit_attributes(doc.get(id).unwrap(), &mut |a| {
        if let haecceity::p21::Attribute::EntityRef(id) = a {
            out.push(*id);
        }
    });
    out
}
fn is(doc: &Document, id: u64, kind: &str) -> bool {
    matches!(doc.get(id), Some(haecceity::p21::RawEntity::Simple { name, .. }) if name.eq_ignore_ascii_case(kind))
}
fn strokes(lines: &[Vec<[f64; 3]>]) -> Vec<Vec<[i64; 3]>> {
    let mut lines: Vec<_> = lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|p| p.map(|v| (v * 1e8).round() as i64))
                .collect::<Vec<_>>()
        })
        .collect();
    lines.sort();
    lines
}

#[test]
fn spindle_numbered_notes_share_one_block_and_keep_each_requirement_link() {
    let doc = load("tests/fixtures/presentation/spindle-q137.step.gz");
    let defs = step::read_part_definitions(doc.bytes()).unwrap();
    let read = pmi::read(&doc, &defs).unwrap();
    let source = &read.parts[0];
    let settings = pmi::PartPmi {
        general: source.general.clone(),
        surface_textures: source
            .surface_textures
            .iter()
            .filter(|t| t.on.is_none())
            .cloned()
            .collect(),
        attributes: source
            .attributes
            .iter()
            // Submit the three defaults; the file's additional authored note
            // stays retained, and is covered by the byte-preservation check.
            .filter(|a| {
                a.on.is_none()
                    && a.name == "semantic text"
                    && a.items
                        .iter()
                        .any(|(key, _)| matches!(key.as_str(), "edge condition" | "edges"))
            })
            .cloned()
            .collect(),
        ..pmi::PartPmi::default()
    };
    assert_eq!(
        notes::general_notes(&settings)
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>(),
        [
            "GENERAL TOLERANCES ISO 2768-m",
            "SURFACE FINISH Ra 3.2 UNLESS OTHERWISE STATED",
            "Break sharp edges 0.2 max",
        ]
    );
    let part = step::read_part(doc.bytes(), &defs[0]).unwrap();
    for count in [3, 1, 0] {
        let mut settings = settings.clone();
        if count < 3 {
            settings.surface_textures.clear();
            settings.attributes.clear();
        }
        if count == 0 {
            settings.general.clear();
        }
        let semantic = json::identify(settings.clone(), None).unwrap();
        let generated = json::presentation::generated(&semantic, &part).unwrap();
        json::presentation::validate(&serde_json::to_value(&generated).unwrap()).unwrap();
        let blocks = unique_notes(&generated);
        assert_eq!(blocks.len(), usize::from(count != 0));
        if let Some(block) = blocks.first() {
            assert_eq!(block.text.len(), count + 1);
            assert_eq!(block.text[0], "GENERAL NOTES");
            assert_eq!(
                block
                    .text
                    .iter()
                    .filter(|s| s.as_str() == "GENERAL NOTES")
                    .count(),
                1
            );
            for (i, text) in block.text.iter().skip(1).enumerate() {
                assert!(text.starts_with(&format!("{}  ", i + 1)), "{text}");
            }
            assert_eq!(
                generated
                    .items
                    .values()
                    .filter(|gs| gs.iter().any(|g| g.id == block.id))
                    .count(),
                count
            );
            let rows = notes::general_notes(&settings);
            let expected = Layout::new(part.bounds()).notes(&notes::notes_block(&rows));
            assert_eq!(strokes(&block.polylines), strokes(&expected.lines));
            // All item aliases share one plane and its view lists the block once.
            let view = generated
                .views
                .iter()
                .find(|v| v.graphics.contains(block.id.as_ref().unwrap()))
                .unwrap();
            assert_eq!(view.graphics, [block.id.clone().unwrap()]);
        }
        // Add preserves the old file's private links and every retained entity.
        let (edit, _) = pmi::write(
            &doc,
            &defs,
            &[(PartId(0), settings)],
            pmi::write::Mode::Add,
            pmi::write::PresentationPolicy::Refuse,
        )
        .unwrap();
        let written = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
        for id in doc.ids() {
            assert_eq!(
                &doc.bytes()[doc.span(id).unwrap()],
                &written.bytes()[written.span(id).unwrap()]
            );
        }
        let written_defs = step::read_part_definitions(written.bytes()).unwrap();
        let written_read = pmi::read(&written, &written_defs).unwrap();
        let written_semantic = json::identify(
            written_read.parts[0].clone(),
            Some(&written_read.identifiers.parts[0]),
        )
        .unwrap();
        let drawn = json::presentation::read(&written, &written_read, PartId(0), &written_semantic)
            .unwrap();
        let new_blocks: Vec<_> = unique_notes(&drawn)
            .into_iter()
            .filter(|g| {
                g.id.as_ref()
                    .unwrap()
                    .strip_prefix("step:")
                    .unwrap()
                    .parse::<u64>()
                    .unwrap()
                    > doc.max_id()
            })
            .collect();
        assert_eq!(new_blocks.len(), usize::from(count != 0));
        if let Some(block) = new_blocks.first() {
            assert_eq!(
                strokes(&block.polylines),
                strokes(&blocks[0].polylines),
                "JSON/STEP notes ink"
            );
            let root = block
                .id
                .as_ref()
                .unwrap()
                .strip_prefix("step:")
                .unwrap()
                .parse::<u64>()
                .unwrap();
            let occurrences = refs(&written, root);
            assert_eq!(occurrences.len(), count);
            let mut definitions = std::collections::BTreeSet::new();
            for occurrence in occurrences {
                let links: Vec<_> = written
                    .ids()
                    .filter(|&id| {
                        is(&written, id, "draughting_model_item_association")
                            && refs(&written, id).last() == Some(&occurrence)
                    })
                    .collect();
                assert_eq!(links.len(), 1, "each row owns its presentation link");
                let link = refs(&written, links[0]);
                assert!(is(&written, link[0], "property_definition"));
                assert!(definitions.insert(link[0]));
            }
            assert_eq!(definitions.len(), count);
        }
    }
}

#[test]
fn unstructured_part_notes_keep_native_ids_and_join_the_same_block() {
    let source = load("tests/fixtures/presentation/spindle-q137.step.gz");
    let source_defs = step::read_part_definitions(source.bytes()).unwrap();
    let source_read = pmi::read(&source, &source_defs).unwrap();
    let original = &source_read.parts[0];
    let doc = Document::parse(
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/ties/cube.step"),
        )
        .unwrap(),
    )
    .unwrap();
    let defs = step::read_part_definitions(doc.bytes()).unwrap();
    let part = step::read_part(doc.bytes(), &defs[0]).unwrap();
    for (finish_text, feature_note) in [
        (None, false),
        (Some("Ra 3.2"), false),
        (Some("Ra 3.20 µm"), false),
        (None, true),
    ] {
        let mut model = pmi::PartPmi {
            notes: vec![pmi::Note {
                kind: "general tolerances".into(),
                text: "ISO 2768-m".into(),
                on: None,
                callouts: vec![],
            }],
            surface_textures: original
                .surface_textures
                .iter()
                .filter(|t| t.on.is_none())
                .cloned()
                .collect(),
            attributes: original
                .attributes
                .iter()
                .filter(|a| {
                    a.on.is_none()
                        && a.name == "semantic text"
                        && a.items
                            .iter()
                            .any(|(key, _)| matches!(key.as_str(), "edge condition" | "edges"))
                })
                .cloned()
                .collect(),
            ..pmi::PartPmi::default()
        };
        if let Some(text) = finish_text {
            model.surface_textures.clear();
            model.notes.push(pmi::Note {
                kind: "surface finish".into(),
                text: text.into(),
                on: None,
                callouts: vec![],
            });
        }
        if feature_note {
            model
                .features
                .push(pmi::Feature::Items(vec![pmi::Anchor::Face(
                    pmi::FaceIndex(0),
                )]));
            model.notes.push(pmi::Note {
                kind: "semantic text".into(),
                text: "Face instruction".into(),
                on: Some(pmi::NoteOwner::Feature(pmi::FeatureId(0))),
                callouts: vec![],
            });
        }
        let semantic = json::identify(model.clone(), None).unwrap();
        let part_ids: Vec<_> = model
            .notes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.on.is_none())
            .map(|(i, _)| semantic.ids["notes"][i].clone())
            .collect();
        let generated = json::presentation::generated(&semantic, &part).unwrap();
        let blocks = unique_notes(&generated);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].text,
            [
                "GENERAL NOTES",
                "1  GENERAL TOLERANCES ISO 2768-m",
                if finish_text == Some("Ra 3.20 µm") {
                    "2  SURFACE FINISH Ra 3.20 UNLESS OTHERWISE STATED"
                } else {
                    "2  SURFACE FINISH Ra 3.2 UNLESS OTHERWISE STATED"
                },
                "3  Break sharp edges 0.2 max"
            ]
        );
        for id in &part_ids {
            assert_eq!(generated.items[id][0].id, blocks[0].id);
        }
        // Native JSON rewriting, as Specify uses, must preserve the note's id
        // rather than changing it into a different semantic collection.
        let (bytes, _) =
            json::write_preserving(&doc, &defs, &[(PartId(0), semantic.clone())]).unwrap();
        let written = Document::parse(bytes).unwrap();
        let written_defs = step::read_part_definitions(written.bytes()).unwrap();
        let read = pmi::read(&written, &written_defs).unwrap();
        let result =
            json::identify(read.parts[0].clone(), Some(&read.identifiers.parts[0])).unwrap();
        assert_eq!(result.ids["notes"], semantic.ids["notes"]);
        let drawn = json::presentation::read(&written, &read, PartId(0), &result).unwrap();
        assert_eq!(unique_notes(&drawn).len(), 1);
        assert_eq!(
            strokes(&unique_notes(&drawn)[0].polylines),
            strokes(&blocks[0].polylines)
        );
        for id in &part_ids {
            assert_eq!(drawn.items[id][0].id, unique_notes(&drawn)[0].id);
        }
        // The legacy writer normalizes notes to attributes/textures. Its block
        // must state the same items without changing the semantic migration.
        let normalized = pmi::write::standard_form(&model);
        assert_eq!(
            notes::notes_block(&notes::general_notes(&normalized)).cells,
            blocks[0].text
        );
    }
}
