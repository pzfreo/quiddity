//! Generator invariants, computed from the input and generated manifests.
#[allow(dead_code)]
#[path = "../examples/support/assembly_layout.rs"]
mod engine;
use engine::{Expected, Layout, Options, Scale};
use haecceity::{
    p21::{
        self,
        streaming::{self, Event},
    },
    step::{
        StepFile,
        manifest::{self, ScanOptions},
    },
};
use std::{collections::BTreeMap, fs::File, io::Cursor};
fn source() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/ap242/assembly/assembly.step"
    ))
    .unwrap()
}
fn statements(bytes: &[u8]) -> BTreeMap<u64, Vec<u8>> {
    let mut rows = BTreeMap::new();
    streaming::scan(bytes, 17, |e| {
        if let Event::Instance(i, b) = e {
            assert!(rows.insert(i.id, b.to_vec()).is_none());
        }
        Ok(())
    })
    .unwrap();
    rows
}
#[test]
fn layouts_preserve_every_statement_and_seed() {
    let bytes = source();
    let expected = statements(&bytes);
    for layout in [Layout::Contiguous, Layout::Runs, Layout::Shuffled] {
        let options = Options {
            layout,
            runs: 7,
            seed: 42,
        };
        let mut a = Vec::new();
        engine::rewrite(&bytes, options, &mut a).unwrap();
        assert_eq!(statements(&a), expected);
        let mut b = Vec::new();
        engine::rewrite(&bytes, options, &mut b).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, bytes);
    }
    let mut a = Vec::new();
    let mut b = Vec::new();
    engine::rewrite(
        &bytes,
        Options {
            layout: Layout::Shuffled,
            seed: 1,
            ..Options::default()
        },
        &mut a,
    )
    .unwrap();
    engine::rewrite(
        &bytes,
        Options {
            layout: Layout::Shuffled,
            seed: 2,
            ..Options::default()
        },
        &mut b,
    )
    .unwrap();
    assert_ne!(a, b);
    assert!(
        engine::rewrite(
            &bytes,
            Options {
                runs: 0,
                ..Options::default()
            },
            Vec::new()
        )
        .is_err()
    );
}
fn check_scale(layout: Layout, distinct: usize, instances: usize, target_bytes: u64) {
    let mut bytes = Vec::new();
    let mut sidecar = Vec::new();
    engine::scale(
        &source(),
        Options {
            layout,
            ..Options::default()
        },
        Scale {
            distinct,
            instances,
            target_bytes,
        },
        &mut bytes,
        &mut sidecar,
    )
    .unwrap();
    let expected: Expected = serde_json::from_slice(&sidecar).unwrap();
    assert!(expected.products >= distinct);
    assert!(expected.instances >= instances);
    assert!(expected.bytes >= target_bytes);
    assert_eq!(expected.bytes, bytes.len() as u64);
    assert_eq!(expected.entities, statements(&bytes).len() as u64);
    let m = manifest::scan(Cursor::new(&bytes), &ScanOptions::default()).unwrap();
    assert!(m.refusals.is_empty());
    assert_eq!(m.products.len(), expected.products);
    assert_eq!(m.instances.len(), expected.instances);
    let whole = StepFile::read(&bytes).unwrap();
    for e in expected.per_product {
        let p = m
            .products
            .iter()
            .find(|p| p.product_definition == e.product_definition)
            .unwrap();
        assert_eq!(p.faces.len(), e.faces);
        assert_eq!(p.entities, e.entities);
        assert!(p.product_id.contains("-copy-"));
        let i = whole
            .parts
            .iter()
            .position(|p| p.product_definition == e.product_definition)
            .unwrap();
        let mut file = tempfile::tempfile().unwrap();
        std::io::Write::write_all(&mut file, &bytes).unwrap();
        assert_eq!(
            quiddity::file_api::recognise_part(
                &m.read_product(&file, &p.id).unwrap(),
                &m.source.sha256
            )
            .unwrap(),
            quiddity::file_api::recognise_part(&whole.part(i).unwrap(), &m.source.sha256).unwrap()
        );
    }
    for (i, p) in m.instances.iter().enumerate() {
        assert_eq!(p.placement[0][3], (i % 100) as f64 * 1000.);
        assert_eq!(p.placement[1][3], (i / 100) as f64 * 1000.);
    }
}
#[test]
fn scale_sidecar_placements_and_recognition_all_layouts() {
    for layout in [Layout::Contiguous, Layout::Runs, Layout::Shuffled] {
        check_scale(layout, 5, 8, 0);
        check_scale(layout, 1, 1, 300_000);
    }
}
#[test]
fn scale_200mb() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scale.step");
    let mut sidecar = Vec::new();
    engine::scale(
        &source(),
        Options::default(),
        Scale {
            distinct: 1,
            instances: 55_000,
            target_bytes: 170_000_000,
        },
        std::io::BufWriter::new(File::create(&path).unwrap()),
        &mut sidecar,
    )
    .unwrap();
    let e: Expected = serde_json::from_slice(&sidecar).unwrap();
    for mapped in [false, true] {
        let opts = ScanOptions {
            index_dir: mapped.then(|| dir.path().to_owned()),
            ..ScanOptions::default()
        };
        let m = manifest::scan(File::open(&path).unwrap(), &opts).unwrap();
        assert_eq!(m.products.len(), e.products);
        assert_eq!(m.instances.len(), e.instances);
        assert_eq!(m.source.size, e.bytes);
        for (p, e) in m.products.iter().zip(&e.per_product) {
            assert_eq!(p.product_definition, e.product_definition);
            assert_eq!(p.entities, e.entities);
            assert_eq!(p.faces.len(), e.faces);
        }
        // Total placed faces exceed the whole-file limit; a selected read remains small.
        assert!(
            m.instances
                .iter()
                .map(|i| m.product(&i.product).unwrap().faces.len())
                .sum::<usize>()
                > haecceity::step::limits::FACES
        );
        m.read_product(&File::open(&path).unwrap(), &m.products[0].id)
            .unwrap();
    }
    let opts = ScanOptions {
        instance_limit: 10,
        ..ScanOptions::default()
    };
    assert!(
        manifest::scan(File::open(&path).unwrap(), &opts)
            .unwrap_err()
            .to_string()
            .contains("limit")
    );
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        1,
        "mapped scratch index must be deleted"
    );
    eprintln!(
        "200 MB acceptance: {} bytes, {} entities, {} products, {} instances",
        e.bytes, e.entities, e.products, e.instances
    );
}
#[test]
fn encoder_remaps_only_references_not_string_hashes() {
    // Names containing #N must survive duplication and escaping through p21.
    let mut bytes = String::from_utf8(source()).unwrap();
    bytes = bytes.replace("PRODUCT('plate'", "PRODUCT('plate #123 ''name'''");
    let mut out = Vec::new();
    engine::scale(
        bytes.as_bytes(),
        Options::default(),
        Scale {
            distinct: 1,
            instances: 1,
            target_bytes: 0,
        },
        &mut out,
        Vec::new(),
    )
    .unwrap();
    let doc = p21::Document::parse(out).unwrap();
    assert!(doc.graph().entities.values().any(|e|matches!(e,p21::RawEntity::Simple {name,attributes,..} if name=="PRODUCT" && matches!(&attributes[0],p21::Attribute::String(s) if s.starts_with("plate #123 'name'-copy-")))));
}
