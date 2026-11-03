#![cfg(feature = "json-import")]
use haecceity::{
    json::{self, Anchors},
    p21::Document,
    pmi::{self, PartId},
    step::read_part_definitions,
};
use std::{io::Read, path::Path};
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
fn whole_json_identity_ctc02_ftc07_xcaf_and_corpus() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/ap242");
    let mut cases = vec![
        (
            "CTC-02",
            root.join("nist/nist_ctc_02_asme1_ap242-e2.stp.gz"),
            None,
        ),
        (
            "FTC-07",
            root.join("nist/nist_ftc_07_asme1_ap242-e2.stp.gz"),
            None,
        ),
        (
            "xcaf plate",
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/xcaf/pmi_plate.geom.step.gz"),
            Some(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/xcaf/pmi_plate.pmi.json"),
            ),
        ),
        (
            "corpus spool",
            root.join("specify/spool_fits.step.gz"),
            None,
        ),
    ];
    if let Some(dir) = std::env::var_os("HAECCEITY_NIST_PMI") {
        let path = Path::new(&dir).join("nist_ftc_08_asme1_ap242-e2.stp");
        assert!(path.is_file(), "full NIST archive must contain FTC-08");
        cases.push(("FTC-08", path, None));
    }
    for (name, path, input) in cases {
        let doc = Document::parse(bytes(&path)).unwrap();
        let parts = read_part_definitions(doc.bytes()).unwrap();
        let read = pmi::read(&doc, &parts).unwrap();
        let mut requested = vec![];
        let mut expected = vec![];
        for (i, p) in read.parts.iter().enumerate() {
            let anchors = Anchors::read(&doc, &parts[i]);
            let semantic = if let Some(path) = &input {
                json::from_str(&std::fs::read_to_string(path).unwrap(), &anchors).unwrap()
            } else {
                json::identify(p.clone(), Some(&read.identifiers.parts[i])).unwrap()
            };
            let encoded = json::to_value(&semantic).unwrap();
            if let Some(path) = &input {
                let original: serde_json::Value =
                    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
                assert_eq!(
                    encoded, original,
                    "producer JSON is compared without resolving away its anchor forms"
                );
            }
            expected.push(encoded);
            requested.push((PartId(i), semantic));
        }
        let drawn_before =
            json::presentation::read(&doc, &read, PartId(0), &requested[0].1).unwrap();
        let (output, report) = json::write(
            &doc,
            &parts,
            &requested,
            pmi::write::PresentationPolicy::RemovePresentation,
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        let after = Document::parse(output).unwrap();
        let definitions = read_part_definitions(after.bytes()).unwrap();
        let reread = pmi::read(&after, &definitions).unwrap();
        if name == "CTC-02" || name == "FTC-08" {
            let after_semantic =
                json::identify(reread.parts[0].clone(), Some(&reread.identifiers.parts[0]))
                    .unwrap();
            let drawn_after =
                json::presentation::read(&after, &reread, PartId(0), &after_semantic).unwrap();
            let file_graphics = |p: &json::presentation::Presentation| {
                p.items
                    .values()
                    .flatten()
                    .filter(|g| g.source == json::presentation::Source::File)
                    .count()
            };
            for (id, graphics) in &drawn_before.items {
                for graphic in graphics
                    .iter()
                    .filter(|g| g.source == json::presentation::Source::File)
                {
                    assert!(
                        drawn_after
                            .items
                            .get(id)
                            .is_some_and(|after| after.iter().any(|g| g.source
                                == json::presentation::Source::File
                                && g.polylines == graphic.polylines
                                && g.triangles == graphic.triangles
                                && g.text == graphic.text)),
                        "foreign graphic {:?} lost from {id}, source {:?}",
                        graphic.name,
                        graphic.step_ids
                    );
                }
            }
            eprintln!(
                "{name} foreign linked graphics before {}, after {}; removed {:?}",
                file_graphics(&drawn_before),
                file_graphics(&drawn_after),
                &report.presentation_removed[..report.presentation_removed.len().min(8)]
            );
        }
        for (i, model) in reread.parts.iter().enumerate() {
            let actual = json::to_value(
                &json::identify(model.clone(), Some(&reread.identifiers.parts[i])).unwrap(),
            )
            .unwrap();
            assert_eq!(actual, expected[i], "{name} part {i}: whole canonical JSON");
            assert!(json::identity_differences(&requested[i].1.pmi, model).is_empty());
        }
        eprintln!(
            "{name}: whole JSON identity passed, {} parts; {} presentation removals",
            parts.len(),
            report.presentation_removed.len()
        );
    }
}
