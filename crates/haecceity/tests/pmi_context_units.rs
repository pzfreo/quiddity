//! Semantic PMI keeps its historical lenient unit policy; declaration metadata is strict.
mod common;

use haecceity::p21::{Attribute as A, Document, Edit, RawEntity};
use haecceity::pmi::{self, LengthUnit};
use haecceity::step::StepFile;
use std::io::Read;

fn append(source: &str, records: &str) -> String {
    let at = source.rfind("ENDSEC;").unwrap();
    format!("{}{records}\n{}", &source[..at], &source[at..])
}

// Conflicting SET order is significant for PMI compatibility, unlike declared metadata.
fn cases() -> [(&'static str, &'static str, LengthUnit); 4] {
    [
        ("two units: inch first", "#6,#3", LengthUnit::Inch),
        (
            "two units: millimetre first",
            "#3,#6",
            LengthUnit::Millimetre,
        ),
        (
            "unresolvable length unit first",
            "#20001,#6",
            LengthUnit::Inch,
        ),
        (
            "SI unit without LENGTH_UNIT",
            "#20003",
            LengthUnit::Millimetre,
        ),
    ]
}

const EXTRA_UNITS: &str = "
#20001=(CONVERSION_BASED_UNIT('INCH',#20002)LENGTH_UNIT()NAMED_UNIT(#5));
#20002=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),#3);
#20003=(NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));
";

#[test]
fn pmi_hole_placements_keep_the_first_resolvable_context_unit() {
    let raw =
        std::fs::read(common::fixtures().join("ap242/nist/nist_stc_09_asme1_ap242-e4.stp.gz"))
            .unwrap();
    let mut source = String::new();
    flate2::read::GzDecoder::new(raw.as_slice())
        .read_to_string(&mut source)
        .unwrap();
    let source = append(&source, EXTRA_UNITS);
    for (case, units, expected) in cases() {
        let doc = Document::parse(source.as_bytes().to_vec()).unwrap();
        let mut context = doc.get(8).unwrap().clone();
        let RawEntity::Complex { parts, .. } = &mut context else {
            panic!("context");
        };
        let assigned = parts
            .iter_mut()
            .find(|p| p.name == "GLOBAL_UNIT_ASSIGNED_CONTEXT")
            .unwrap();
        assigned.attributes = vec![A::List(
            units
                .split(',')
                .map(|id| A::EntityRef(id.trim_start_matches('#').parse().unwrap()))
                .collect(),
        )];
        let mut edit = Edit::new();
        edit.replace(8, context);
        let bytes = doc.apply(&edit).unwrap().bytes;
        let file = StepFile::read(&bytes).unwrap();
        let doc = Document::parse(bytes).unwrap();
        let read = pmi::read(&doc, &file.parts).unwrap();
        assert_eq!(read.parts[0].holes.len(), 25, "{case}: {:?}", read.findings);
        let hole = read.parts[0]
            .holes
            .iter()
            .find(|h| read.provenance.parts[0].features[h.feature.0].contains(&8369))
            .unwrap();
        let placed = hole
            .placement
            .as_ref()
            .unwrap_or_else(|| panic!("{case}: lost placement"));
        assert_eq!(placed.origin[0].unit, expected, "{case}");
        assert!(
            (placed.origin[0].mm() - 0.75 * expected.millimetres()).abs() < 1e-12,
            "{case}"
        );
        assert!(
            hole.definition
                .placement
                .origin
                .iter()
                .all(|p| p.unit == expected),
            "{case}"
        );
        assert!(
            file.parts.iter().all(|p| p.declared_length_unit.is_none()),
            "metadata remains strict: {case}"
        );
    }
}

#[test]
fn pmi_supplemental_geometry_keeps_the_lenient_context_unit() {
    let assembly =
        std::fs::read_to_string(common::fixtures().join("ap242/assembly/assembly.step")).unwrap();
    for (case, units, expected) in cases() {
        let units = units
            .split(',')
            .map(|id| match id {
                "#6" => "#30006",
                "#3" => "#28",
                "#20001" => "#30001",
                "#20003" => "#30003",
                _ => panic!("unknown test unit"),
            })
            .collect::<Vec<_>>()
            .join(",");
        let records = format!("
#30001=(CONVERSION_BASED_UNIT('INCH',#30002)LENGTH_UNIT()NAMED_UNIT(#30005));
#30002=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),#28);
#30003=(NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));
#30004=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#28);
#30005=DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.);
#30006=(CONVERSION_BASED_UNIT('INCH',#30004)LENGTH_UNIT()NAMED_UNIT(#30005));
#30010=SHAPE_ASPECT('axis','',#34,.T.);
#30011=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#30010,#30012,#30013);
#30012=CONSTRUCTIVE_GEOMETRY_REPRESENTATION('',(#30013),#30014);
#30013=AXIS2_PLACEMENT_3D('',#30015,#13,$);
#30014=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT(({units}))REPRESENTATION_CONTEXT('',''));
#30015=CARTESIAN_POINT('',(1.,2.,3.));
#30020=DIMENSIONAL_SIZE(#30010,'diameter');
#30021=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#30020,#30022);
#30022=SHAPE_DIMENSION_REPRESENTATION('',(#30023),#30014);
#30023=(LENGTH_MEASURE_WITH_UNIT()MEASURE_REPRESENTATION_ITEM()MEASURE_WITH_UNIT(LENGTH_MEASURE(2.),#28)REPRESENTATION_ITEM('nominal value'));
");
        let source = append(&assembly, &records);
        let file = StepFile::read(source.as_bytes()).unwrap();
        let doc = Document::parse(source.into_bytes()).unwrap();
        let read = pmi::read(&doc, &file.parts).unwrap();
        let index = read.provenance.parts[0]
            .geometry_items
            .iter()
            .position(|&id| id == 30013)
            .unwrap_or_else(|| panic!("{case}: no supplemental geometry: {:?}", read.findings));
        assert_eq!(
            read.parts[0].geometry[index].length_unit,
            Some(expected),
            "{case}"
        );
    }
}
