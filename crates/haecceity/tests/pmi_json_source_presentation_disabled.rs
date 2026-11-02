#![cfg(all(feature = "json-import", not(feature = "presentation")))]

use haecceity::{
    json::{self, presentation::Presentation},
    p21::Document,
    pmi::{self, PartId, PartPmi, write::PresentationPolicy},
    step::read_part_definitions,
};

#[test]
fn source_input_is_refused_without_compiling_presentation_drawing() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/ties/cube.step"
    ))
    .unwrap();
    let parts = read_part_definitions(&bytes).unwrap();
    let doc = Document::parse(bytes.clone()).unwrap();
    let pmi = [(PartId(0), PartPmi::default())];
    let policy = PresentationPolicy::RemovePresentation;
    let source = [(PartId(0), Presentation::default())];
    let error =
        pmi::write::write_with_presentation(&doc, &parts, &pmi, pmi::Mode::Add, policy, &source)
            .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("requires the `presentation` feature")
    );
    let semantic = [(PartId(0), json::identify(PartPmi::default(), None).unwrap())];
    let error =
        json::write_with_presentation(&doc, &parts, &semantic, policy, &source).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("requires the `presentation` feature")
    );
    assert_eq!(doc.bytes(), bytes);

    // Omitting source input keeps the established minimal writer route available.
    let (old, _) = pmi::write(&doc, &parts, &pmi, pmi::Mode::Add, policy).unwrap();
    let (new, _) =
        pmi::write::write_with_presentation(&doc, &parts, &pmi, pmi::Mode::Add, policy, &[])
            .unwrap();
    assert_eq!(
        doc.apply(&old).unwrap().bytes,
        doc.apply(&new).unwrap().bytes
    );
    assert!(json::write(&doc, &parts, &semantic, policy).is_ok());
}
