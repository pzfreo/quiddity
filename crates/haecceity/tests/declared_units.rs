//! Declared units are representation metadata; geometry continues to use the existing scale.
mod common;

use std::io::Read;
use std::path::Path;

use haecceity::Part;
use haecceity::p21::{Document, Edit, RawEntity};
use haecceity::pmi::LengthUnit;
use haecceity::step::{IDENTITY, StepFile, read_part, read_part_definitions, read_step};

fn bytes(path: &Path) -> Vec<u8> {
    let raw = std::fs::read(path).unwrap();
    if path.extension().is_some_and(|e| e == "gz") {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(raw.as_slice())
            .read_to_end(&mut out)
            .unwrap();
        out
    } else {
        raw
    }
}

fn assembly() -> String {
    String::from_utf8(bytes(
        &common::fixtures().join("ap242/assembly/assembly.step"),
    ))
    .unwrap()
}

fn check_all(bytes: &[u8], expected: Option<LengthUnit>) {
    let file = StepFile::read(bytes).unwrap();
    assert!(!file.parts.is_empty());
    assert_eq!(read_step(bytes).unwrap().declared_length_unit(), expected);
    for (index, def) in file.parts.iter().enumerate() {
        assert_eq!(def.declared_length_unit, expected, "{}", def.name);
        let own = file.part(index).unwrap();
        assert_eq!(own.declared_length_unit(), expected);
        assert_eq!(
            own.transformed(&IDENTITY).unwrap().declared_length_unit(),
            expected
        );
        assert_eq!(
            read_part(bytes, def).unwrap().declared_length_unit(),
            expected
        );
        assert_eq!(
            file.placed(&def.placements[0].instances)
                .unwrap()
                .declared_length_unit(),
            expected
        );
    }
    for def in read_part_definitions(bytes).unwrap() {
        assert_eq!(def.declared_length_unit, expected);
    }
}

#[test]
fn nist_ap242_declares_inches() {
    check_all(
        &bytes(&common::fixtures().join("ap242/nist/nist_ftc_07_asme1_ap242-e2.stp.gz")),
        Some(LengthUnit::Inch),
    );
}

#[test]
fn millimetres_and_si_prefixes() {
    let source = assembly();
    for (token, unit) in [
        (".MILLI.", LengthUnit::Millimetre),
        (".MICRO.", LengthUnit::Micrometre),
        (".CENTI.", LengthUnit::Centimetre),
        ("$", LengthUnit::Metre),
    ] {
        let changed = source.replace(
            "SI_UNIT(.MILLI.,.METRE.)",
            &format!("SI_UNIT({token},.METRE.)"),
        );
        check_all(changed.as_bytes(), Some(unit));
    }
}

#[test]
fn missing_context_declaration_is_not_a_millimetre_default() {
    let doc = Document::parse(assembly().into_bytes()).unwrap();
    let mut edit = Edit::new();
    for (&id, entity) in &doc.graph().entities {
        let mut changed = entity.clone();
        if let RawEntity::Complex { parts, .. } = &mut changed
            && parts
                .iter()
                .any(|p| p.name == "GLOBAL_UNIT_ASSIGNED_CONTEXT")
        {
            parts.retain(|p| p.name != "GLOBAL_UNIT_ASSIGNED_CONTEXT");
            edit.replace(id, changed);
        }
    }
    let changed = doc.apply(&edit).unwrap().bytes;
    check_all(&changed, None);
    assert_eq!(
        Part::new(vec![], vec![], vec![]).declared_length_unit(),
        None
    );
}

#[test]
fn assembly_parts_keep_their_own_contexts() {
    // Existing fixture has separate contexts for the plate (#959) and pin (#1256).
    // Only the pin declares inches; the assembly and plate still declare millimetres.
    let source = assembly();
    let old = "#1257 = ( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );";
    assert!(source.contains(old));
    let changed = source.replace(
        old,
        "#1257 = ( CONVERSION_BASED_UNIT('in',#20001) LENGTH_UNIT() NAMED_UNIT(#20002) );\n\
         #20001 = LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#960);\n\
         #20002 = DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.);",
    );
    let file = StepFile::read(changed.as_bytes()).unwrap();
    assert_eq!(file.parts.len(), 2);
    for (index, expected) in [LengthUnit::Millimetre, LengthUnit::Inch]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            file.parts[index].declared_length_unit,
            Some(expected.clone())
        );
        assert_eq!(
            file.part(index).unwrap().declared_length_unit(),
            Some(expected.clone())
        );
        assert_eq!(
            file.placed(&file.parts[index].placements[0].instances)
                .unwrap()
                .declared_length_unit(),
            Some(expected)
        );
    }
    // Sharing the same topology through another product's representation must still
    // preserve that product's own declaration, rather than collapsing by face entity id.
    let shared = changed.replace("('',(#11,#978),#1256)", "('',(#11,#41),#1256)");
    assert_ne!(shared, changed);
    let shared = StepFile::read(shared.as_bytes()).unwrap();
    assert_eq!(
        shared.parts[0].declared_length_unit,
        Some(LengthUnit::Millimetre)
    );
    assert_eq!(shared.parts[1].declared_length_unit, Some(LengthUnit::Inch));
    assert_eq!(
        shared.part(0).unwrap().declared_length_unit(),
        Some(LengthUnit::Millimetre)
    );
    assert_eq!(
        shared.part(1).unwrap().declared_length_unit(),
        Some(LengthUnit::Inch)
    );
    assert_eq!(
        read_step(changed.as_bytes())
            .unwrap()
            .declared_length_unit(),
        None
    );
}

#[test]
fn shared_shape_with_conflicting_representation_contexts_is_ambiguous() {
    let source = assembly();
    // Another representation of the plate's very same solid (#41), with an inch context.
    let changed = source.replace("ENDSEC;\nEND-ISO-10303-21;", "\
#20005 = SHAPE_REPRESENTATION_RELATIONSHIP('','',#40,#20000);\n\
#20000 = ADVANCED_BREP_SHAPE_REPRESENTATION('',(#41),#20001);\n\
#20001 = ( GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#20002)) REPRESENTATION_CONTEXT('','') );\n\
#20002 = ( CONVERSION_BASED_UNIT('inch',#20003) LENGTH_UNIT() NAMED_UNIT(#20004) );\n\
#20003 = LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#960);\n\
#20004 = DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.);\n\
ENDSEC;\nEND-ISO-10303-21;");
    assert_ne!(changed, source);
    let file = StepFile::read(changed.as_bytes()).unwrap();
    assert_eq!(file.parts[0].declared_length_unit, None);
    assert_eq!(file.part(0).unwrap().declared_length_unit(), None);
    assert_eq!(
        read_step(changed.as_bytes()).unwrap().bounds(),
        read_step(source.as_bytes()).unwrap().bounds(),
        "metadata ambiguity must not change geometry"
    );
    assert_eq!(
        file.parts[1].declared_length_unit,
        Some(LengthUnit::Millimetre)
    );
}

#[test]
fn corpus_millimetre_part() {
    let Some(corpus) = common::corpus_dir() else {
        assert_ne!(
            std::env::var("QUIDDITY_CORPUS_REQUIRED").as_deref(),
            Ok("1"),
            "corpus required"
        );
        return;
    };
    check_all(
        &bytes(&corpus.join("cadgenbench/flanged_spool_132.step")),
        Some(LengthUnit::Millimetre),
    );
}

#[test]
fn metadata_does_not_impose_the_p21_editors_reserved_id_range() {
    let source = assembly();
    let mut changed = String::new();
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        changed.push(c);
        if c == '#' && chars.peek().is_some_and(char::is_ascii_digit) {
            let mut id = String::new();
            while chars.peek().is_some_and(char::is_ascii_digit) {
                id.push(chars.next().unwrap());
            }
            changed.push_str(&((1_u64 << 63) + id.parse::<u64>().unwrap()).to_string());
        }
    }
    // These ids are valid for geometry import, although the lossless editor reserves
    // them for provisional references. Metadata import cannot narrow accepted geometry.
    check_all(changed.as_bytes(), Some(LengthUnit::Millimetre));
    assert_eq!(
        read_step(changed.as_bytes()).unwrap().bounds(),
        read_step(source.as_bytes()).unwrap().bounds()
    );
}

fn same_geometry(original: &[u8], changed: &[u8]) {
    let a = read_step(original).unwrap();
    let b = read_step(changed).unwrap();
    assert_eq!(a.bounds(), b.bounds());
    assert_eq!(
        (a.faces.len(), a.edges.len(), a.solids.len()),
        (b.faces.len(), b.edges.len(), b.solids.len())
    );
    for face in 0..a.faces.len() {
        assert_eq!(a.face_source(face), b.face_source(face));
        assert_eq!(
            a.face_moments(face).map(|m| m.area),
            b.face_moments(face).map(|m| m.area)
        );
    }
}

#[test]
fn signature_sections_accepted_by_geometry_do_not_require_the_document_scanner() {
    let source = assembly();
    let changed = source.replacen("DATA;", "SIGNATURE;\nENDSEC;\nDATA;", 1);
    assert!(
        Document::parse(changed.as_bytes().to_vec())
            .unwrap_err()
            .to_string()
            .contains("SIGNATURE")
    );
    check_all(changed.as_bytes(), Some(LengthUnit::Millimetre));
    same_geometry(source.as_bytes(), changed.as_bytes());
}

#[test]
fn scanner_graph_mismatch_does_not_change_import() {
    // step-io recovers missing HEADER semicolons at the next keyword. The editor's
    // scanner merges the two headers and refuses its count mismatch instead.
    let source = assembly();
    let record = "FILE_DESCRIPTION(('Open CASCADE Model'),'2;1');";
    assert!(source.contains(record));
    let changed = source.replace(record, record.trim_end_matches(';'));
    assert!(matches!(
        Document::parse(changed.as_bytes().to_vec()),
        Err(haecceity::p21::P21Error::Mismatch(_))
    ));
    check_all(changed.as_bytes(), Some(LengthUnit::Millimetre));
    same_geometry(source.as_bytes(), changed.as_bytes());
}

#[test]
fn a_metadata_record_error_discards_all_declarations_without_refusing_geometry() {
    let source = assembly().replace(
        "#960 = ( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );",
        "#960 = ( CONVERSION_BASED_UNIT('INCH',#20001) LENGTH_UNIT() NAMED_UNIT(#20002) );\n\
         #20001 = LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#28);\n\
         #20002 = DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.);",
    );
    let changed = source.replace(
        "#20001 = LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#28);",
        "#20001 = LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),#28);",
    );
    assert_ne!(source, changed);
    check_all(changed.as_bytes(), None);
    same_geometry(source.as_bytes(), changed.as_bytes());
}

#[test]
fn deep_conversion_chains_keep_main_geometry_import_without_using_the_stack() {
    // Main accepts this unused-by-geometry chain because the file's geometric scale comes
    // from its first context. Metadata must also read it without recursively using the stack.
    let source = assembly();
    let mut records = String::from("#20002=DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.);\n");
    let mut last = 28;
    for i in 0..60_000 {
        let factor = 50_000 + 2 * i;
        let unit = factor + 1;
        records.push_str(&format!("#{factor}=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),#{last});\n#{unit}=(CONVERSION_BASED_UNIT('unit',#{factor})LENGTH_UNIT()NAMED_UNIT(#20002));\n"));
        last = unit;
    }
    records.push_str(&format!(
        "#20001=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),#{last});\n"
    ));
    let changed = source.replace(
        "#1257 = ( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );",
        "#1257=(CONVERSION_BASED_UNIT('unit',#20001)LENGTH_UNIT()NAMED_UNIT(#20002));",
    );
    let at = changed.rfind("ENDSEC;").unwrap();
    let changed = format!("{}{records}{}", &changed[..at], &changed[at..]);
    let file = StepFile::read(changed.as_bytes()).unwrap();
    assert!(
        file.parts
            .iter()
            .all(|p| p.declared_length_unit == Some(LengthUnit::Millimetre))
    );
    assert_eq!(
        file.part(1).unwrap().declared_length_unit(),
        Some(LengthUnit::Millimetre)
    );
    same_geometry(source.as_bytes(), changed.as_bytes());
}

#[test]
fn declared_angle_uncertainty_uses_its_own_unit() {
    let source = assembly();
    let input = source.replace(
        "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#31))",
        "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#31,#20000))",
    ).replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#20000 = UNCERTAINTY_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(0.02),#20001,'','');\n\
         #20001 = ( CONVERSION_BASED_UNIT('degree',#20002) NAMED_UNIT(#20003) PLANE_ANGLE_UNIT() );\n\
         #20002 = PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(0.017453292519943295),#29);\n\
         #20003 = DIMENSIONAL_EXPONENTS(0.,0.,0.,0.,0.,0.,0.);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let baseline = read_step(source.as_bytes()).unwrap();
    assert_eq!(baseline.file_angle_uncertainty(), None);
    let part = read_step(input.as_bytes()).unwrap();
    assert_eq!(part.file_uncertainty(), baseline.file_uncertainty());
    assert_eq!(part.file_angle_uncertainty(), Some(0.02_f64.to_radians()));
    assert_eq!(
        part.transformed(&IDENTITY)
            .unwrap()
            .file_angle_uncertainty(),
        part.file_angle_uncertainty()
    );
}
