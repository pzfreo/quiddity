//! Graphic PMI accounting is narrower than the presentation family used by safe removal.
use haecceity::{p21::Document, pmi};

fn count(instances: &str) -> usize {
    let text = format!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('presentation accounting'),'2;1');\nFILE_NAME('','',(''),(''),'','','');\nFILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));\nENDSEC;\nDATA;\n{instances}\nENDSEC;\nEND-ISO-10303-21;"
    );
    let doc = Document::parse(text.into_bytes()).unwrap();
    pmi::read(&doc, &[]).unwrap().accounting.presentation
}

#[test]
fn geometry_styles_and_empty_models_are_not_graphic_pmi() {
    assert_eq!(
        count(
            r#"
#1=COLOUR_RGB('',1.,0.,0.);
#2=DRAUGHTING_PRE_DEFINED_CURVE_FONT('continuous');
#3=CURVE_STYLE('',#2,POSITIVE_LENGTH_MEASURE(0.1),#1);
#4=PRESENTATION_STYLE_ASSIGNMENT((#3));
#5=CARTESIAN_POINT('',(0.,0.,0.));
#6=STYLED_ITEM('',(#4),#5);
#7=PRESENTATION_LAYER_ASSIGNMENT('geometry','',(#6));
#8=INVISIBILITY((#6));
#9=DRAUGHTING_MODEL('',(#6),#20);
#10=MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION('',(#6),#20);
#11=PRESENTATION_VIEW('',(#6),#20);
#12=MODEL_GEOMETRIC_VIEW('','',#5,#9);
#20=GEOMETRIC_REPRESENTATION_CONTEXT(3);
"#
        ),
        0
    );
}

#[test]
fn counts_annotation_subtypes_and_complex_instances_once() {
    for entity in [
        "ANNOTATION_OCCURRENCE",
        "ANNOTATION_CURVE_OCCURRENCE",
        "ANNOTATION_FILL_AREA_OCCURRENCE",
        "ANNOTATION_TEXT_OCCURRENCE",
        "ANNOTATION_SYMBOL_OCCURRENCE",
        "ANNOTATION_PLACEHOLDER_OCCURRENCE",
        "TESSELLATED_ANNOTATION_OCCURRENCE",
        "ANNOTATION_PLANE",
        "DRAUGHTING_CALLOUT",
        "DIMENSION_CURVE_DIRECTED_CALLOUT",
        "STRUCTURED_DIMENSION_CALLOUT",
    ] {
        assert_eq!(count(&format!("#1={entity}('',(),$);")), 1, "{entity}");
    }
    assert_eq!(
        count(
            "#1=(ANNOTATION_OCCURRENCE() ANNOTATION_CURVE_OCCURRENCE() REPRESENTATION_ITEM('') STYLED_ITEM((),$));"
        ),
        1
    );
}

#[test]
fn counts_only_models_and_views_carrying_annotations_including_mapped_content() {
    assert_eq!(
        count(
            r#"
#1=ANNOTATION_TEXT_OCCURRENCE('',(),$);
#2=DRAUGHTING_CALLOUT('',(#1));
#3=ANNOTATION_PLANE('',(),$,(#2));
#4=DRAUGHTING_MODEL('',(#3),#20);
#5=REPRESENTATION_MAP(#21,#4);
#6=MAPPED_ITEM('',#5,#21);
#7=PRESENTATION_VIEW('',(#6),#20);
#8=MODEL_GEOMETRIC_VIEW('','',#21,#4);
#9=DRAUGHTING_MODEL('',(#21),#20);
#10=PRESENTATION_VIEW('',(#21),#20);
#11=MODEL_GEOMETRIC_VIEW('','',#21,#9);
#20=GEOMETRIC_REPRESENTATION_CONTEXT(3);
#21=CARTESIAN_POINT('',(0.,0.,0.));
"#
        ),
        6
    );
    // Styling a graphic annotation does not hide it from its carrying model, but neither
    // the wrapper nor a compound item counts as an additional graphic PMI instance.
    assert_eq!(
        count(
            "#1=ANNOTATION_TEXT_OCCURRENCE('',(),$); #2=STYLED_ITEM('',(),#1); #3=COMPOUND_REPRESENTATION_ITEM('',SET_REPRESENTATION_ITEM((#2))); #4=DRAUGHTING_MODEL('',(#3),$);"
        ),
        2
    );
    // A cyclic mapping with no annotation must terminate and count nothing.
    assert_eq!(
        count(
            "#1=DRAUGHTING_MODEL('',(#2),$); #2=MAPPED_ITEM('',#3,$); #3=REPRESENTATION_MAP($,#1);"
        ),
        0
    );
}

mod common;

/// The exact previous family-based presentation rule, for before/after regression evidence.
fn legacy_presentation_count(doc: &Document) -> usize {
    use haecceity::express::{self, Family};
    use haecceity::p21::RawEntity;
    let schema = doc.file_schema().unwrap();
    let edition = express::edition(&schema[0]).unwrap_or(express::TARGET);
    // Reproduce the previous accounting rule without changing the broad family itself.
    doc.ids()
        .filter(|&id| {
            let names: Vec<_> = match doc.get(id).unwrap() {
                RawEntity::Simple { name, .. } => vec![name.to_ascii_lowercase()],
                RawEntity::Complex { parts, .. } => {
                    parts.iter().map(|p| p.name.to_ascii_lowercase()).collect()
                }
            };
            names.iter().any(|n| {
                edition
                    .family(n)
                    .or_else(|| express::TARGET.family(n))
                    .or_else(|| express::AP242_ED1.family(n))
                    == Some(Family::Presentation)
            })
        })
        .count()
}

#[test]
fn nist_corpus_graphic_presentation_counts() {
    use haecceity::step::read_part_definitions;

    let Some(root) = common::corpus_dir() else {
        assert!(
            std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
            "corpus required"
        );
        eprintln!("SKIPPED: set QUIDDITY_CORPUS to check NIST presentation counts");
        return;
    };
    let mut files: Vec<_> = std::fs::read_dir(root.join("nist"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "stp"))
        .collect();
    files.sort();
    assert_eq!(files.len(), 10);
    for path in files {
        let bytes = std::fs::read(&path).unwrap();
        let parts = read_part_definitions(&bytes).unwrap();
        let doc = Document::parse(bytes).unwrap();
        let old = legacy_presentation_count(&doc);
        let reading = pmi::read(&doc, &parts).unwrap();
        let new = reading.accounting.presentation;
        let name = path.file_name().unwrap().to_str().unwrap();
        println!("{name}: {old} -> {new}");
        assert_eq!(new, 0, "{name}: geometry-only NIST corpus file");
        assert!(new < old, "{name}: geometry styling excluded");
    }
}

#[test]
fn nist_ap242_fixtures_retain_graphic_pmi() {
    use haecceity::step::read_part_definitions;
    use std::io::Read;
    let mut files: Vec<_> = std::fs::read_dir(common::fixtures().join("ap242/nist"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "gz"))
        .collect();
    files.sort();
    assert_eq!(files.len(), 13);
    for path in files {
        let raw = std::fs::read(&path).unwrap();
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(raw.as_slice())
            .read_to_end(&mut bytes)
            .unwrap();
        let parts = read_part_definitions(&bytes).unwrap();
        let doc = Document::parse(bytes).unwrap();
        let reading = pmi::read(&doc, &parts).unwrap();
        let new = reading.accounting.presentation;
        let name = path.file_name().unwrap().to_str().unwrap();
        let old = legacy_presentation_count(&doc);
        println!("{name}: {old} -> {new}");
        let expected = match name {
            "nist_ctc_01_asme1_ap242-e1.stp.gz" => 71,
            "nist_ctc_02_asme1_ap242-e2.stp.gz" => 191,
            // Six further supplied CTC/FTC files make the strict #142 gate self-contained.
            "nist_ctc_03_asme1_ap242-e2.stp.gz" => 119,
            "nist_ctc_04_asme1_ap242.stp.gz" => 84,
            "nist_ctc_05_asme1_ap242.stp.gz" => 54,
            "nist_ftc_06_asme1_ap242-e2.stp.gz" => 275,
            "nist_ftc_08_asme1_ap242-e2.stp.gz" => 162,
            "nist_ftc_11_asme1_ap242-e3.stp.gz" => 46,
            "nist_ftc_07_asme1_ap242-e2.stp.gz" => 168,
            "nist_ftc_10_asme1_ap242-e2.stp.gz" => 279,
            "nist_stc_06_asme1_ap242-e3.stp.gz" => 257,
            "nist_stc_09_asme1_ap242-e4.stp.gz" => 179,
            "nist_stc_10_asme1_ap242-e2.stp.gz" => 283,
            _ => panic!("unexpected NIST fixture {name}"),
        };
        assert_eq!(new, expected, "{name}: graphic PMI retained");
        assert!(
            new < old,
            "{name}: styles and annotation scaffolding excluded"
        );
    }
}
