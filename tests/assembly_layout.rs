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
    let original = manifest::scan(Cursor::new(&bytes), &ScanOptions::default()).unwrap();
    let mut exclusive = BTreeMap::new();
    streaming::scan(bytes.as_slice(), 8192, |e| {
        if let Event::Instance(i, _) = e {
            let owners: Vec<_> = original
                .products
                .iter()
                .enumerate()
                .filter(|(_, p)| p.ranges.iter().any(|r| r[0] <= i.offset && i.offset < r[1]))
                .map(|(i, _)| i + 1)
                .collect();
            exclusive.insert(i.id, if owners.len() == 1 { owners[0] } else { 0 });
        }
        Ok(())
    })
    .unwrap();
    for layout in [Layout::Contiguous, Layout::Runs, Layout::Shuffled] {
        let options = Options {
            layout,
            runs: 7,
            seed: 42,
        };
        let mut a = Vec::new();
        engine::rewrite(&bytes, options, &mut a).unwrap();
        assert_eq!(statements(&a), expected);
        let mut runs = vec![0; original.products.len() + 1];
        let mut last = None;
        streaming::scan(a.as_slice(), 8192, |e| {
            if let Event::Instance(i, _) = e {
                let owner = exclusive[&i.id];
                if last != Some(owner) {
                    runs[owner] += 1;
                    last = Some(owner);
                }
            }
            Ok(())
        })
        .unwrap();
        for &count in &runs[1..] {
            match layout {
                Layout::Contiguous => assert_eq!(count, 1),
                Layout::Runs => assert_eq!(count, options.runs),
                Layout::Shuffled => assert!(count > options.runs),
            }
        }
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
    let max_id = p21::Document::parse(source()).unwrap().max_id();
    let original = manifest::scan(Cursor::new(source()), &ScanOptions::default()).unwrap();
    for (copy, e) in expected.per_product.into_iter().enumerate() {
        assert_eq!(
            e.product_definition,
            original.products[copy % original.products.len()].product_definition
                + (copy as u64 + 1) * max_id
        );
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

#[test]
fn scale_every_assembly_fixture() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ap242/assembly");
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "step"))
        .collect();
    paths.sort();
    assert_eq!(paths.len(), 4);
    for path in paths {
        let bytes = std::fs::read(&path).unwrap();
        let mut out = Vec::new();
        let mut sidecar = Vec::new();
        engine::scale(
            &bytes,
            Options::default(),
            Scale {
                distinct: 5,
                instances: 8,
                target_bytes: 0,
            },
            &mut out,
            &mut sidecar,
        )
        .unwrap();
        let expected: Expected = serde_json::from_slice(&sidecar).unwrap();
        let m = manifest::scan(Cursor::new(out), &ScanOptions::default()).unwrap();
        assert_eq!(m.products.len(), expected.products, "{}", path.display());
        assert_eq!(m.instances.len(), expected.instances, "{}", path.display());
        for (p, e) in m.products.iter().zip(expected.per_product) {
            assert_eq!(
                (p.product_definition, p.faces.len(), p.entities),
                (e.product_definition, e.faces, e.entities),
                "{}",
                path.display()
            );
        }
    }
}

#[test]
fn contiguous_uses_source_definition_order() {
    let bytes = source();
    let original = manifest::scan(Cursor::new(&bytes), &ScanOptions::default()).unwrap();
    let doc = p21::Document::parse(bytes.clone()).unwrap();
    let first = doc
        .ids()
        .filter_map(|id| doc.span(id).map(|s| s.start))
        .min()
        .unwrap();
    let pd = original.products[1].product_definition;
    let span = doc.span(pd).unwrap();
    let mut moved = bytes[..first].to_vec();
    moved.extend_from_slice(&bytes[span.clone()]);
    moved.push(b'\n');
    moved.extend_from_slice(&bytes[first..span.start]);
    moved.extend_from_slice(&bytes[span.end..]);
    let mut out = Vec::new();
    engine::rewrite(&moved, Options::default(), &mut out).unwrap();
    assert_eq!(statements(&out), statements(&bytes));
    let doc = p21::Document::parse(out).unwrap();
    assert!(
        doc.span(pd).unwrap().start
            < doc
                .span(original.products[0].product_definition)
                .unwrap()
                .start
    );
}

#[test]
fn scale_counts_solid_instances_instead_of_occurrences() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/two_solids.step"
    ))
    .unwrap();
    let original = manifest::scan(Cursor::new(&bytes), &ScanOptions::default()).unwrap();
    assert_eq!(original.products.len(), 1);
    assert_eq!(original.instances.len(), 2);
    for layout in [Layout::Contiguous, Layout::Runs, Layout::Shuffled] {
        let mut out = Vec::new();
        let mut sidecar = Vec::new();
        engine::scale(
            &bytes,
            Options {
                layout,
                ..Options::default()
            },
            Scale {
                distinct: 3,
                instances: 7,
                target_bytes: 0,
            },
            &mut out,
            &mut sidecar,
        )
        .unwrap();
        let e: Expected = serde_json::from_slice(&sidecar).unwrap();
        let m = manifest::scan(Cursor::new(&out), &ScanOptions::default()).unwrap();
        assert_eq!(e.products, 3);
        assert_eq!(e.instances, 8);
        assert_eq!(m.instances.len(), e.instances);
        assert_eq!(m.products.len(), e.products);
        assert_eq!(out.len() as u64, e.bytes);
        for (p, e) in m.products.iter().zip(e.per_product) {
            assert_eq!(p.faces.len(), e.faces);
            assert_eq!(p.entities, e.entities);
        }
        let paths: std::collections::BTreeSet<_> =
            m.instances.iter().map(|i| i.path.clone()).collect();
        assert_eq!(paths.len(), 4);
        let placement = |i: usize| {
            let p = &m.instances[i].placement;
            (p[0][3], p[1][3])
        };
        assert_eq!(placement(0), (0., 0.));
        assert_eq!(placement(2), (1000., 0.));
        assert_eq!(placement(4), (2000., 0.));
        assert_eq!(placement(6), (3000., 0.));
    }
}
