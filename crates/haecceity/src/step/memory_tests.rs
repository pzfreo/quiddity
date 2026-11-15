//! Directed equivalence checks and an opt-in, single-input import memory probe.

use super::*;
use std::collections::hash_map::DefaultHasher;
use std::fmt::Write;
use std::hash::Hasher;
use std::time::Instant;

fn ordinary(bytes: &[u8]) -> Parsed<step_io::parser::Graph> {
    let (model, report) = step_io::read(bytes).unwrap();
    let graph = step_io::parser::parse(bytes).unwrap();
    let declared_units = crate::pmi::declared_length_units(&graph, &graph.header, bytes);
    Parsed {
        model,
        report,
        graph,
        declared_units,
    }
}

struct Fingerprint(DefaultHasher);

impl std::fmt::Write for Fingerprint {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        self.0.write(value.as_bytes());
        Ok(())
    }
}

// Stream every Debug field into the same-toolchain hash without allocating a
// whole-part String. This includes native/source data and initially empty caches.
fn fingerprint(part: &Part) -> u64 {
    let mut hash = Fingerprint(DefaultHasher::new());
    write!(&mut hash, "{part:?}").unwrap();
    hash.0.finish()
}

#[test]
fn a_simple_complex_keyword_does_not_become_a_complex_record() {
    let bytes = b"ISO-10303-21;HEADER;FILE_DESCRIPTION((''),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));ENDSEC;DATA;#1=COMPLEX();#2=(ALPHA() BETA());ENDSEC;END-ISO-10303-21;";
    fn check(graph: &impl RecordGraph) {
        assert_eq!(graph.kind(1), graph.kind(2));
        assert!(!graph.is_complex(1));
        assert!(graph.is_complex(2));
    }
    check(&step_io::parser::parse(bytes.as_slice()).unwrap());
    check(&step_io::parser::parse_compact(bytes.as_slice()).unwrap());
}

#[test]
fn packed_declared_units_keep_the_exact_selected_real_token() {
    use crate::pmi::{Decimal, LengthUnit};
    let bytes = b"ISO-10303-21;HEADER;FILE_DESCRIPTION((''),'2;1');\
FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));ENDSEC;DATA;\
#1=SHAPE_DEFINITION_REPRESENTATION(#2,#3);\
#2=PRODUCT_DEFINITION_SHAPE('','',#4);#4=PRODUCT_DEFINITION();\
#3=SHAPE_REPRESENTATION('',(#5),#6);#5=ADVANCED_FACE();\
#6=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#7))REPRESENTATION_CONTEXT('',''));\
#7=(CONVERSION_BASED_UNIT('custom',#8)LENGTH_UNIT()NAMED_UNIT(*));\
#8=(EXTRA(0.0254000000000000000002)MEASURE_WITH_UNIT(LENGTH_MEASURE(0.0254000000000000000001),#10));\
#10=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.));ENDSEC;END-ISO-10303-21;";
    let raw = step_io::parser::parse(bytes.as_slice()).unwrap();
    let packed = step_io::parser::parse_compact(bytes.as_slice()).unwrap();
    let a = crate::pmi::declared_length_units(&raw, &raw.header, bytes);
    let b = crate::pmi::declared_length_units(&packed, &packed.header, bytes);
    let expected = Some(LengthUnit::from_metres(
        "custom",
        Decimal::parse("0.0254000000000000000001").unwrap(),
    ));
    assert_eq!(a.definitions.get(&4), Some(&expected));
    assert_eq!(a.faces.get(&5), Some(&expected));
    assert_eq!(a.definitions, b.definitions);
    assert_eq!(a.faces, b.faces);
}

#[test]
fn owned_snapshot_keeps_geometry_placement_and_error_columns() {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let bytes = std::fs::read(fixtures.join("prismatic.step")).unwrap();
    let at = [[1., 0., 0., 17.], [0., 1., 0., -2.], [0., 0., 1., 3.]];
    assert_eq!(
        fingerprint(&read_step_placed(&bytes, &at).unwrap()),
        fingerprint(&read_step_placed_owned(bytes, &at).unwrap())
    );
    let bytes =
        super::step_file_bytes(&fixtures.join("broken/truck_pub_shaft_hole.step.gz")).unwrap();
    assert_eq!(
        read_step(&bytes).unwrap_err().to_string(),
        read_step_owned(bytes).unwrap_err().to_string()
    );
}

#[test]
fn packed_provenance_keeps_geometry_assemblies_and_refusals() {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    for name in [
        "prismatic.step",
        "golden_simple_through_hole.step",
        "occt_trimmed_parabola.step",
        "ap242/assembly/assembly.step",
        "broken/truncated_closed.step",
        "broken/deleted_face.step",
        "broken/golden_simple_through_hole_no_bounds.step",
        "broken/unresolved_surface.step",
    ] {
        let bytes = std::fs::read(fixtures.join(name)).unwrap();
        let raw = ordinary(&bytes);
        let graph = step_io::parser::parse_compact(bytes.as_slice()).unwrap();
        let raw_drops = crate::express::complex_reference_drops(&raw.graph);
        let packed_drops = crate::express::complex_reference_drops(&graph);
        assert_eq!(raw_drops, packed_drops, "{name}");
        let refusal = |graph: &dyn std::fmt::Debug| format!("{graph:?}");
        let a =
            refuse_dropped_shapes(&raw.graph, &raw.report, &raw_drops).map_err(|e| e.to_string());
        let b =
            refuse_dropped_shapes(&graph, &raw.report, &packed_drops).map_err(|e| e.to_string());
        assert_eq!(a, b, "{name}");
        assert_eq!(
            refusal(&assemblies(&raw.graph)),
            refusal(&assemblies(&graph)),
            "{name}"
        );
        if a.is_ok() {
            let packed = Parsed::new(&bytes).unwrap();
            assert_eq!(
                raw.declared_units.definitions, packed.declared_units.definitions,
                "{name}"
            );
            assert_eq!(
                raw.declared_units.faces, packed.declared_units.faces,
                "{name}"
            );
            let a = build(&raw, Scope::Placed(IDENTITY))
                .map(|r| fingerprint(&r.part))
                .map_err(|e| e.to_string());
            let b = build(&packed, Scope::Placed(IDENTITY))
                .map(|r| fingerprint(&r.part))
                .map_err(|e| e.to_string());
            assert_eq!(a, b, "{name}");
        }
    }
}

#[test]
#[ignore = "opt-in real-file memory experiment; requires STEP_MEMORY_INPUT"]
fn real_import_memory_probe() {
    let path = std::env::var("STEP_MEMORY_INPUT").unwrap();
    let representation = std::env::var("STEP_MEMORY_REPRESENTATION").unwrap();
    let storage = std::env::var("STEP_MEMORY_STORAGE").unwrap();
    let started = Instant::now();
    let mut bytes = Some(std::fs::read(&path).unwrap());
    eprintln!(
        "STAGE input elapsed={:.6} length={} storage={storage}",
        started.elapsed().as_secs_f64(),
        bytes.as_ref().unwrap().len()
    );
    fn run(parsed: &Parsed<impl RecordGraph>, started: Instant) {
        eprintln!(
            "STAGE checked elapsed={:.6}",
            started.elapsed().as_secs_f64()
        );
        let part = build(parsed, Scope::Placed(IDENTITY)).unwrap().part;
        eprintln!(
            "STAGE geometry elapsed={:.6} faces={} edges={} solids={}",
            started.elapsed().as_secs_f64(),
            part.faces.len(),
            part.edges.len(),
            part.solids.len()
        );
        let hash = fingerprint(&part);
        eprintln!(
            "RESULT faces={} edges={} solids={} unresolved_faces={} unresolved_edges={} geometry_fingerprint={hash:016x} elapsed={:.6}",
            part.faces.len(),
            part.edges.len(),
            part.solids.len(),
            part.unresolved_faces().len(),
            part.unresolved_edges().len(),
            started.elapsed().as_secs_f64()
        );
        std::hint::black_box(parsed);
        std::hint::black_box(&part);
    }
    if representation == "compact" {
        let parsed = if storage == "owned" {
            Parsed::owned(bytes.take().unwrap()).unwrap()
        } else {
            assert_eq!(storage, "buffer");
            Parsed::new(bytes.as_deref().unwrap()).unwrap()
        };
        run(&parsed, started);
    } else {
        assert_eq!(representation, "raw");
        assert_eq!(storage, "buffer");
        let parsed = ordinary(bytes.as_deref().unwrap());
        let drops = crate::express::complex_reference_drops(&parsed.graph);
        refuse_dropped_shapes(&parsed.graph, &parsed.report, &drops).unwrap();
        check_curve_wrappers(&parsed.model).unwrap();
        run(&parsed, started);
    }
    std::hint::black_box(&bytes);
}
