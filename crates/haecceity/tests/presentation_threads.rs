#![cfg(all(feature = "presentation", feature = "json-import"))]
use haecceity::{
    geom::{self, V3},
    json::{
        self,
        presentation::{self, Graphic, Presentation},
    },
    p21::Document,
    pmi::{self, PartId},
    presentation as notation, step,
};
use std::{
    collections::BTreeSet,
    io::Read,
    path::{Path, PathBuf},
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/presentation")
}
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
fn cases() -> Vec<(PathBuf, Option<PathBuf>)> {
    let mut cases = std::fs::read_dir(root().join("thread-callouts"))
        .unwrap()
        .map(|p| p.unwrap().path())
        .filter(|p| {
            p.to_string_lossy().contains("GRM-") && p.to_string_lossy().ends_with(".step.gz")
        })
        .map(|p| {
            let semantic = p.with_file_name(format!(
                "{}.pmi.json.gz",
                p.file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .trim_end_matches(".gz")
            ));
            (p, Some(semantic))
        })
        .collect::<Vec<_>>();
    cases.sort();
    cases.extend([
        (
            root().join("thread-callouts/GRM-01_shank.step.gz"),
            Some(root().join("thread-callouts/shank-editor.pmi.json.gz")),
        ),
        (root().join("spindle-q137.step.gz"), None),
        (
            root()
                .join("../../../../../tests/fixtures/ap242/nist/nist_ctc_01_asme1_ap242-e1.stp.gz"),
            None,
        ),
        (
            root().join("../../../../../tests/fixtures/ap242/presentation-placement/plate.step.gz"),
            None,
        ),
        (
            root().join("../xcaf/pmi_plate.geom.step.gz"),
            Some(root().join("../xcaf/pmi_plate.pmi.json")),
        ),
        (
            root().join("thread-callouts/spindle_pmi_before.step.gz"),
            None,
        ),
        (
            root().join("thread-callouts/spindle_pmi_after.step.gz"),
            None,
        ),
    ]);
    cases
}

// Explicit exclusions: these describe identity/support or relationships, not ink.
// Attributes with drawing wording are NOT excluded: the notes compiler binds them.
const NO_DRAWING_FORM: &[(&str, &str)] = &[
    (
        "features",
        "shape support/group; requirement graphics can alias these",
    ),
    (
        "geometry",
        "supplemental geometric support, not a requirement",
    ),
    (
        "derived_geometry",
        "construction/support definition, not a requirement",
    ),
    (
        "tolerance_relations",
        "relationship between drawn control frames",
    ),
];
const NON_DRAWN_ATTRIBUTES: &[(&str, &str)] = &[
    ("pmi-assist answers", "legacy interview resume state"),
    ("specify answers", "interview resume state"),
    ("specify original", "original source provenance"),
    ("specify pmi origin", "requirement provenance"),
    (
        "specify dimension plan",
        "planner quantity identity/support",
    ),
    (
        "specify characteristic numbers",
        "inspection-number registry",
    ),
    (
        "thread manufacturing",
        "pitch/depth are incorporated in the thread callout; drilling parameters are not independent drawing requirements",
    ),
    ("Modeled By", "CAD author metadata"),
    ("CAGE Code", "organization identifier"),
    ("Company", "CAD organization metadata"),
    ("", "opaque CAD presentation payload, not drawing prose"),
];
fn coverage(name: &str, semantic: &json::Semantic, scene: &Presentation) {
    let mut total = 0;
    let mut drawn = 0;
    for (kind, ids) in &semantic.ids {
        for (i, id) in ids.iter().enumerate() {
            total += 1;
            if scene.items.get(id).is_some_and(|g| !g.is_empty()) {
                drawn += 1;
                continue;
            }
            if kind == "attributes" {
                let a = &semantic.pmi.attributes[i];
                assert!(
                    NON_DRAWN_ATTRIBUTES.iter().any(|(name, _)| *name == a.name),
                    "{name}: undrawn attribute {} {id}",
                    a.name
                );
                continue;
            }
            assert!(
                NO_DRAWING_FORM.iter().any(|(k, _)| *k == kind),
                "{name}: missing {kind}: {id}"
            );
        }
    }
    if semantic.pmi.material.is_some() {
        assert!(
            scene
                .items
                .values()
                .flatten()
                .chain(&scene.unlinked)
                .any(|g| g
                    .text
                    .iter()
                    .any(|s| s.to_ascii_lowercase().contains("material")))
        );
    }
    println!("{name}: {drawn}/{total} drawn; all drawing forms covered");
}

// Fit actual stroke vertices to a font run, in the FILE'S stated plane. This
// detects reversed advance/up independently; it is not an axes-only assertion.
fn stroke_fit(g: &Graphic, plane: &presentation::Plane, text: &str) -> Option<(f64, f64, V3)> {
    let ink = notation::font().text(text, 3.);
    let local = &ink.lines;
    if local.is_empty() {
        return None;
    }
    let up = geom::cross(plane.normal, plane.x_axis);
    for lines in g.polylines.windows(local.len()) {
        if lines.iter().zip(local).any(|(a, b)| a.len() != b.len()) {
            continue;
        }
        let points = lines
            .iter()
            .flatten()
            .zip(local.iter().flatten())
            .collect::<Vec<_>>();
        let n = points.len() as f64;
        let mean = points
            .iter()
            .fold([0.; 3], |s, (p, _)| geom::add(s, **p))
            .map(|v| v / n);
        let mid = [0, 1].map(|k| points.iter().map(|(_, q)| q[k]).sum::<f64>() / n);
        let axes = [plane.x_axis, up];
        let scales = [0, 1].map(|k| {
            let variance = points
                .iter()
                .map(|(_, q)| (q[k] - mid[k]).powi(2))
                .sum::<f64>();
            if variance < 1e-10 {
                return 0.;
            }
            points
                .iter()
                .map(|(p, q)| (q[k] - mid[k]) * geom::dot(geom::sub(**p, mean), axes[k]))
                .sum::<f64>()
                / variance
        });
        if scales[0].abs() < 1e-8 || (scales[0].abs() - scales[1].abs()).abs() > 1e-5 {
            continue;
        }
        let origin = geom::sub(
            mean,
            geom::add(
                geom::scale(axes[0], mid[0] * scales[0]),
                geom::scale(axes[1], mid[1] * scales[1]),
            ),
        );
        if points.iter().all(|(p, q)| {
            geom::dist(
                **p,
                geom::add(
                    origin,
                    geom::add(
                        geom::scale(axes[0], q[0] * scales[0]),
                        geom::scale(axes[1], q[1] * scales[1]),
                    ),
                ),
            ) < 3e-6
        }) {
            return Some((scales[0], scales[1], origin));
        }
    }
    None
}
fn callout(kind: &str, i: usize, p: &pmi::PartPmi) -> Option<notation::Callout> {
    match kind {
        "dimensions" => Some(notation::dimension(&p.dimensions[i])),
        "tolerances" => Some(notation::tolerance(&p.tolerances[i], p)),
        "datums" => Some(notation::datum(p.datums[i].label().as_str())),
        "threads" => Some(notation::thread(&p.threads[i], p)),
        _ => None,
    }
}
#[test]
fn all_drawing_forms_have_graphics_and_glyphs_agree_with_planes() {
    for (path, companion) in cases() {
        let name = path.file_name().unwrap().to_str().unwrap();
        let doc = Document::parse(bytes(&path)).unwrap();
        let defs = step::read_part_definitions(doc.bytes()).unwrap();
        let read = pmi::read(&doc, &defs).unwrap();
        let semantic = if let Some(c) = companion {
            json::from_str(
                std::str::from_utf8(&bytes(&c)).unwrap(),
                &json::Anchors::read(&doc, &defs[0]),
            )
            .unwrap()
        } else {
            json::identify(read.parts[0].clone(), Some(&read.identifiers.parts[0])).unwrap()
        };
        let part = step::read_part(doc.bytes(), &defs[0]).unwrap();
        let generated = presentation::generated(&semantic, &part).unwrap();
        coverage(name, &semantic, &generated);
        clear_callouts(name, &generated);
        glyph_placements(name, &generated);
        for (i, t) in semantic.pmi.threads.iter().enumerate() {
            let site = notation::placement::feature_site(&part, &semantic.pmi, t.feature)
                .unwrap()
                .0;
            let g = &generated.items[&semantic.ids["threads"][i]][0];
            let limit = 2. * notation::placement::text_height(part.bounds().max_extent());
            assert!(
                geom::dist(site, g.text_placements[0].origin) <= limit + 3e-6,
                "{name}: thread too far from feature: {}",
                g.name
            );
        }
        presentation::validate(&serde_json::to_value(&generated).unwrap()).unwrap();
        let saved = presentation::read(&doc, &read, PartId(0), &semantic).unwrap();
        let mut checked = BTreeSet::new();
        let mut file_checked = BTreeSet::new();
        for (kind, ids) in &semantic.ids {
            for (i, id) in ids.iter().enumerate() {
                for g in generated.items.get(id).into_iter().flatten() {
                    if !checked.insert(g.id.clone()) {
                        continue;
                    }
                    assert_eq!(g.polylines.len(), g.polyline_roles.as_ref().unwrap().len());
                    if kind == "tolerances" {
                        let c = notation::tolerance(&semantic.pmi.tolerances[i], &semantic.pmi);
                        let roles = g.polyline_roles.as_ref().unwrap();
                        // Replacing whole cell strings must also remove their ISO
                        // glyph strokes, while keeping frame and standalone rings.
                        assert_eq!(
                            roles
                                .iter()
                                .filter(|r| **r == presentation::PolylineRole::Frame)
                                .count(),
                            c.frame_strokes
                        );
                        assert_eq!(
                            roles
                                .iter()
                                .filter(|r| **r == presentation::PolylineRole::Symbol)
                                .count(),
                            usize::from(c.leader_rings)
                        );
                        assert_eq!(
                            roles
                                .iter()
                                .filter(|r| **r == presentation::PolylineRole::Text)
                                .count(),
                            c.polylines().len() - c.frame_strokes
                        );
                    }
                    let plane = generated
                        .planes
                        .iter()
                        .find(|p| Some(&p.id) == g.plane_id.as_ref())
                        .unwrap();
                    for text in &g.text_placements {
                        assert_eq!(text.x_axis, plane.x_axis);
                        assert_eq!(text.normal, plane.normal);
                        let b = text.cell_box.unwrap();
                        assert!(b[0][0] <= 0. && b[1][0] >= 0. && b[0][1] <= 0. && b[1][1] >= 0.);
                    }
                    if let Some(c) = callout(kind, i, &semantic.pmi) {
                        for t in &c.letters {
                            if notation::text_width(&t.text, 3.) == 0. {
                                continue;
                            }
                            let (x, y, _) = stroke_fit(g, plane, &t.text).unwrap_or_else(|| {
                                panic!("{name}: cannot fit generated {}", t.text)
                            });
                            assert!(x > 0. && y > 0., "{name}: reversed generated {}", g.name);
                        }
                    }
                }
                if let Some(c) = callout(kind, i, &semantic.pmi) {
                    for g in saved.items.get(id).into_iter().flatten() {
                        let Some(plane) = saved
                            .planes
                            .iter()
                            .find(|p| Some(&p.id) == g.plane_id.as_ref())
                        else {
                            continue;
                        };
                        for t in &c.letters {
                            if let Some((x, y, _)) = stroke_fit(g, plane, &t.text) {
                                file_checked.insert(g.id.clone());
                                assert!(
                                    x > 0. && y > 0.,
                                    "FILE DATA MISMATCH {name} {:?} {}: advance={x} up={y}, plane={plane:?}",
                                    g.id,
                                    g.name
                                );
                            }
                        }
                    }
                }
            }
        }
        // Graphic-only notes may have no semantic link. Inspect their own
        // readable names as well; NIST's filled CAD-font triangles have no
        // single-stroke font run to fit and are not stroked glyphs.
        for g in saved.items.values().flatten().chain(&saved.unlinked) {
            let Some(plane) = saved
                .planes
                .iter()
                .find(|p| Some(&p.id) == g.plane_id.as_ref())
            else {
                continue;
            };
            let title = (g.name == "General notes").then_some("GENERAL NOTES");
            for line in g.name.lines().chain(title) {
                if let Some((x, y, _)) = stroke_fit(g, plane, line) {
                    assert!(
                        x > 0. && y > 0.,
                        "{name}: reversed source glyphs in {:?}",
                        g.id
                    );
                    file_checked.insert(g.id.clone());
                }
            }
        }
        println!(
            "{name}: glyph direction checked on {} saved graphics",
            file_checked.len()
        );
    }
}

#[test]
fn blind_internal_and_external_threads_are_drawn_and_written_without_a_size_dimension() {
    use pmi::{
        AttributeSet, AttributeValue, Decimal, FeatureId, Hand, LengthUnit, NoteOwner, PartPmi,
        ThreadSide, Value,
    };
    let path = root().join("thread-callouts/GRM-01_shank.step.gz");
    let doc = Document::parse(bytes(&path)).unwrap();
    let defs = step::read_part_definitions(doc.bytes()).unwrap();
    let semantic = json::from_str(
        std::str::from_utf8(&bytes(
            &root().join("thread-callouts/GRM-01_shank.step.pmi.json.gz"),
        ))
        .unwrap(),
        &json::Anchors::read(&doc, &defs[0]),
    )
    .unwrap();
    let mut thread = semantic.pmi.threads[0].clone();
    thread.feature = FeatureId(0);
    thread.partial_area = Some(FeatureId(0));
    thread.qualifier = Some("M3x0.5".into());
    thread.fit_class = "6H".into();
    let value = |s| Value::length(Decimal::parse(s).unwrap(), LengthUnit::Millimetre);
    for (side, depth, expected) in [
        (ThreadSide::Internal, "12.", "M3×0.5-6H ↧ 12"),
        (ThreadSide::External, "8.", "M3×0.5-6g LH ↧ 8"),
    ] {
        thread.side = side;
        if side == ThreadSide::External {
            thread.fit_class = "6g".into();
            thread.hand = Hand::Left;
        }
        let p = PartPmi {
            features: vec![semantic.pmi.features[semantic.pmi.threads[0].feature.0].clone()],
            threads: vec![thread.clone()],
            attributes: vec![AttributeSet {
                name: "thread manufacturing".into(),
                on: Some(NoteOwner::Feature(FeatureId(0))),
                description: None,
                general_property: None,
                items: vec![(
                    if side == ThreadSide::Internal {
                        "minimum full thread"
                    } else {
                        "thread length"
                    }
                    .into(),
                    AttributeValue::Measure(value(depth)),
                )],
            }],
            ..PartPmi::default()
        };
        assert_eq!(notation::thread(&p.threads[0], &p).notation(), expected);
        let mut paired = p.clone();
        paired.threads[0].fit_class_2 = Some("5H".into());
        assert!(
            notation::thread(&paired.threads[0], &paired)
                .notation()
                .contains("/5H")
        );
        let part = step::read_part(doc.bytes(), &defs[0]).unwrap();
        let generated =
            presentation::generated(&json::identify(p.clone(), None).unwrap(), &part).unwrap();
        let g = generated.items.values().flatten().next().unwrap();
        assert_eq!(g.text, [expected]);
        let site = notation::placement::feature_site(&part, &p, FeatureId(0))
            .unwrap()
            .0;
        assert!(
            geom::dist(site, g.text_placements[0].origin)
                <= 2. * notation::placement::text_height(part.bounds().max_extent()) + 2e-6
        );
        let (edit, _) = pmi::write(
            &doc,
            &defs,
            &[(PartId(0), p)],
            pmi::write::Mode::Add,
            pmi::write::PresentationPolicy::RemovePresentation,
        )
        .unwrap();
        let written = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
        let written_defs = step::read_part_definitions(written.bytes()).unwrap();
        let read = pmi::read(&written, &written_defs).unwrap();
        assert!(
            read.callouts.iter().any(|g| g.name == expected),
            "thread-only STEP needs a callout"
        );
        let semantic =
            json::identify(read.parts[0].clone(), Some(&read.identifiers.parts[0])).unwrap();
        let file = presentation::read(&written, &read, PartId(0), &semantic).unwrap();
        assert!(
            file.items[&semantic.ids["threads"][0]]
                .iter()
                .any(|g| g.name == expected)
        );
    }
}

fn clear_callouts(name: &str, scene: &Presentation) {
    use presentation::PolylineRole;
    let mut seen = BTreeSet::new();
    let graphics = scene
        .items
        .values()
        .flatten()
        .chain(&scene.unlinked)
        .filter(|g| seen.insert(g.id.clone()))
        .collect::<Vec<_>>();
    for (i, a) in graphics.iter().enumerate() {
        let plane = scene
            .planes
            .iter()
            .find(|p| Some(&p.id) == a.plane_id.as_ref())
            .unwrap();
        let up = geom::cross(plane.normal, plane.x_axis);
        let bounds = |g: &Graphic| {
            let mut b = geom::Bounds::empty();
            for (line, role) in g.polylines.iter().zip(g.polyline_roles.as_ref().unwrap()) {
                if matches!(
                    role,
                    PolylineRole::Leader | PolylineRole::Dimension | PolylineRole::Extension
                ) {
                    continue;
                }
                for &p in line {
                    b.add([geom::dot(p, plane.x_axis), geom::dot(p, up), 0.]);
                }
            }
            b
        };
        let x = bounds(a);
        for b in &graphics[i + 1..] {
            let other = scene
                .planes
                .iter()
                .find(|p| Some(&p.id) == b.plane_id.as_ref())
                .unwrap();
            if geom::dot(plane.normal, other.normal).abs() < 1. - 1e-8 {
                continue;
            }
            let y = bounds(b);
            let gap = (0..2)
                .map(|k| (x.min[k] - y.max[k]).max(y.min[k] - x.max[k]))
                .fold(f64::NEG_INFINITY, f64::max);
            assert!(
                gap >= -2e-6,
                "{name}: {} overlaps {}: {gap}",
                a.name,
                b.name
            );
        }
    }
}

#[test]
fn the_reported_spindle_frame_reads_along_positive_y_and_up_negative_z() {
    let doc = Document::parse(bytes(
        &root().join("thread-callouts/spindle_pmi_before.step.gz"),
    ))
    .unwrap();
    let defs = step::read_part_definitions(doc.bytes()).unwrap();
    let read = pmi::read(&doc, &defs).unwrap();
    let semantic = json::identify(read.parts[0].clone(), Some(&read.identifiers.parts[0])).unwrap();
    let scene = presentation::read(&doc, &read, PartId(0), &semantic).unwrap();
    let g = scene
        .items
        .values()
        .flatten()
        .find(|g| g.name == "⌖ ⌀0.2 A")
        .unwrap();
    let plane = scene
        .planes
        .iter()
        .find(|p| Some(&p.id) == g.plane_id.as_ref())
        .unwrap();
    assert!(geom::dist(plane.normal, [-1., 0., 0.]) < 1e-12);
    assert_eq!(plane.x_axis, [0., 1., 0.]);
    assert!(geom::dist(geom::cross(plane.normal, plane.x_axis), [0., 0., -1.]) < 1e-12);
    let (advance, up, _) = stroke_fit(g, plane, "0.2").unwrap();
    assert!(advance > 0. && up > 0.);
    println!(
        "Spindle #12: {advance} positive advance, {up} positive up; file plane and glyphs agree"
    );
}

#[test]
fn optional_roles_and_boxes_validate_without_breaking_old_documents() {
    let doc = Document::parse(bytes(&root().join("thread-callouts/GRM-01_shank.step.gz"))).unwrap();
    let defs = step::read_part_definitions(doc.bytes()).unwrap();
    let semantic = json::from_str(
        std::str::from_utf8(&bytes(
            &root().join("thread-callouts/GRM-01_shank.step.pmi.json.gz"),
        ))
        .unwrap(),
        &json::Anchors::read(&doc, &defs[0]),
    )
    .unwrap();
    let part = step::read_part(doc.bytes(), &defs[0]).unwrap();
    let scene = presentation::generated(&semantic, &part).unwrap();
    let mut value = serde_json::to_value(&scene).unwrap();
    for rows in value["items"].as_object_mut().unwrap().values_mut() {
        for g in rows.as_array_mut().unwrap() {
            g.as_object_mut().unwrap().remove("polyline_roles");
            for p in g["text_placements"].as_array_mut().unwrap() {
                p.as_object_mut().unwrap().remove("cell_box");
            }
        }
    }
    presentation::validate(&value).unwrap();
    let old: Presentation = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(old).unwrap(), value);
    let key = scene.items.keys().next().unwrap();
    let mut value = serde_json::to_value(&scene).unwrap();
    value["items"][key][0]["polyline_roles"] = serde_json::json!([]);
    assert!(presentation::validate(&value).is_err());
    let mut value = serde_json::to_value(&scene).unwrap();
    value["items"][key][0]["text_placements"][0]["cell_box"] =
        serde_json::json!([[2., 0.], [1., 1.]]);
    assert!(presentation::validate(&value).is_err());
}

// Match every ordinary generated run to its actual vertices and placement,
// including GENERAL NOTES, material-only unlinked blocks and face finishes.
// Control and datum frames draw all their text as CAD-font geometry (not the sheet font
// these runs are fitted against); their cell boxes and glyphs are checked in gdt's tests.
fn glyph_placements(name: &str, scene: &Presentation) {
    let mut seen = BTreeSet::new();
    for g in scene.items.values().flatten().chain(&scene.unlinked) {
        if !seen.insert(&g.id) {
            continue;
        }
        assert_eq!(g.polyline_roles.as_ref().unwrap().len(), g.polylines.len());
        let plane = scene
            .planes
            .iter()
            .find(|p| Some(&p.id) == g.plane_id.as_ref())
            .unwrap();
        for placed in &g.text_placements {
            assert_eq!(placed.x_axis, plane.x_axis);
            assert_eq!(placed.normal, plane.normal);
            assert!(placed.cell_box.is_some());
            let Some(i) = placed.text_index else {
                continue;
            };
            if g.id
                .as_deref()
                .is_some_and(|id| {
                    id.starts_with("generated:tolerances:") || id.starts_with("generated:datums:")
                })
            {
                continue;
            }
            if notation::font().text(&g.text[i], 3.).lines.is_empty() {
                continue;
            }
            let (x, y, origin) = stroke_fit(g, plane, &g.text[i])
                .unwrap_or_else(|| panic!("{name}: missing run {}", g.text[i]));
            assert!(x > 0. && y > 0., "{name}: reversed {}", g.name);
            assert!(
                geom::dist(origin, placed.origin) < 3e-6,
                "{name}: text origin disagrees with glyphs: {}",
                g.text[i]
            );
            assert!(
                (x * 3. - placed.height.unwrap()).abs() < 3e-6,
                "{name}: text height disagrees with glyphs"
            );
        }
    }
}
