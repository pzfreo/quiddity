//! Reader-only #190 acceptance: no CLI, Python, layouts or recognition changes.
#[path = "common/parallel.rs"]
mod parallel;
use haecceity::p21::streaming::{self, Event};
use haecceity::step::{
    StepFile,
    manifest::{self, Manifest, ScanOptions},
};
use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    if !dir.is_dir() {
        return;
    }
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().unwrap() != "rust-snapshot" {
                files(&path, out);
            }
        } else {
            let name = path.to_string_lossy().to_ascii_lowercase();
            if [".step", ".stp", ".step.gz", ".stp.gz"]
                .iter()
                .any(|s| name.ends_with(s))
            {
                out.push(path);
            }
        }
    }
}
fn inputs() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    files(&root.join("tests/fixtures"), &mut out);
    files(&root.join("crates/haecceity/tests/fixtures"), &mut out);
    let corpus = std::env::var_os("QUIDDITY_CORPUS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Users/paul/repos/quiddity/tests/corpus"));
    if std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_some() {
        assert!(
            corpus.is_dir(),
            "required corpus missing: {}",
            corpus.display()
        );
    }
    files(&corpus, &mut out);
    out.sort();
    out.dedup();
    out
}
fn bytes(path: &Path) -> Vec<u8> {
    let raw = std::fs::read(path).unwrap();
    if raw.starts_with(&[31, 139]) {
        let mut b = Vec::new();
        flate2::read::GzDecoder::new(raw.as_slice())
            .read_to_end(&mut b)
            .unwrap();
        b
    } else {
        raw
    }
}
fn statements(b: &[u8], chunk: usize) -> Result<Vec<streaming::Instance>, String> {
    let mut out = Vec::new();
    streaming::scan(b, chunk, |e| {
        if let Event::Instance(i, _) = e {
            out.push(i);
        }
        Ok(())
    })
    .map_err(|e| e.to_string())?;
    Ok(out)
}
#[test]
fn statement_chunks_every_corpus_and_fixture() {
    let inputs = inputs();
    assert!(!inputs.is_empty());
    for p in &inputs {
        let b = bytes(p);
        let whole = statements(&b, b.len().max(1));
        for chunk in [1, 7, 4096] {
            assert_eq!(
                statements(&b, chunk),
                whole,
                "{} chunk {chunk}",
                p.display()
            );
        }
    }
    eprintln!(
        "statement chunk equivalence: {} files x 4 chunk sizes",
        inputs.len()
    );
}
fn check(path: &Path) -> usize {
    let b = bytes(path);
    let baseline = StepFile::read(&b);
    let scanned = manifest::scan(Cursor::new(&b), &ScanOptions::default());
    let baseline = match baseline {
        Ok(b) => b,
        Err(_) => {
            return 0;
        }
    };
    let m = scanned.unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(
        m.assemblies,
        baseline.assemblies,
        "{} hierarchy",
        path.display()
    );
    assert_eq!(
        m.products.len(),
        baseline.parts.len(),
        "{} products",
        path.display()
    );
    assert!(
        m.refusals.is_empty(),
        "{}: {:?}",
        path.display(),
        m.refusals
    );
    let mut temp = temp_source(&b);
    for (p, d) in m.products.iter().zip(&baseline.parts) {
        assert_eq!(
            p.product_definition,
            d.product_definition,
            "{} definition",
            path.display()
        );
        assert_eq!(p.shape, d.shape);
        assert_eq!(p.name, d.name);
        assert_eq!(p.product_id, d.product_id);
        assert_eq!(p.faces, d.faces, "{} {} faces", path.display(), p.id);
        assert_eq!(p.edges, d.edges, "{} {} edges", path.display(), p.id);
        for instance in m.instances.iter().filter(|i| i.product == p.id) {
            let mut parent = None;
            for &occurrence in &instance.path {
                let (assembly, component) = m
                    .assemblies
                    .iter()
                    .flat_map(|a| a.components.iter().map(move |c| (a, c)))
                    .find(|(_, c)| c.occurrence == occurrence)
                    .expect("path occurrence in hierarchy");
                if let Some(pd) = parent {
                    assert_eq!(assembly.product_definition, pd);
                }
                parent = Some(component.definition);
            }
            if let Some(pd) = parent {
                assert_eq!(pd, p.product_definition);
            }
        }
        let got: Vec<_> = m
            .instances
            .iter()
            .filter(|i| i.product == p.id)
            .map(|i| i.placement)
            .collect();
        let want: Vec<_> = d
            .placements
            .iter()
            .flat_map(|p| p.instances.iter().map(|_| p.placement))
            .collect();
        assert_eq!(got, want, "{} {} placements", path.display(), p.id);
        let cut = m
            .read_product(&temp, &p.id)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let whole = haecceity::step::read_part(&b, d).unwrap();
        assert_eq!(
            (0..cut.faces.len())
                .map(|i| cut.face_source(i).unwrap().entity)
                .collect::<Vec<_>>(),
            d.faces
        );
        assert_eq!(
            (0..cut.edges.len())
                .map(|i| cut.edge_source(i).unwrap().entity)
                .collect::<Vec<_>>(),
            (0..whole.edges.len())
                .map(|i| whole.edge_source(i).unwrap().entity)
                .collect::<Vec<_>>(),
            "{} {} edge storage order",
            path.display(),
            p.id
        );
        let mut got = (0..cut.edges.len())
            .map(|i| cut.edge_source(i).unwrap().entity)
            .collect::<Vec<_>>();
        got.sort_unstable();
        got.dedup();
        let mut want = d.edges.clone();
        want.sort_unstable();
        assert_eq!(got, want);
        assert_eq!(
            serde_json::to_value(quiddity::correspondence::recognise(&cut)).unwrap(),
            serde_json::to_value(quiddity::correspondence::recognise(&whole)).unwrap(),
            "{} {} recognition",
            path.display(),
            p.id
        );
    }
    m.verify_source(Cursor::new(&b)).unwrap();
    let json = serde_json::to_vec(&m).unwrap();
    let round: Manifest = serde_json::from_slice(&json).unwrap();
    assert_eq!(m, round);
    // Keep File live through all product reads, including platforms with shared file cursors.
    temp.flush().unwrap();
    m.products.len()
}
fn temp_source(b: &[u8]) -> File {
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(b).unwrap();
    file
}
#[test]
fn every_product_corpus_and_fixtures() {
    let paths = inputs();
    let count: usize = parallel::map(&paths, |path| check(path)).into_iter().sum();
    eprintln!(
        "manifest equivalence: {} files, {count} readable distinct parts",
        paths.len()
    );
}
#[test]
fn assembly_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ap242/assembly");
    let mut paths = Vec::new();
    files(&root, &mut paths);
    paths.sort();
    assert!(!paths.is_empty());
    let parts: usize = paths.iter().map(|p| check(p)).sum();
    eprintln!(
        "assembly fixtures: {} files, {parts} distinct parts",
        paths.len()
    );
}
#[test]
fn gramil_pack() {
    let dir = std::env::var_os("QUIDDITY_GRAMIL")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Users/paul/repos/gramil/dist/step"));
    if !dir.is_dir() {
        assert!(
            std::env::var_os("QUIDDITY_GRAMIL_REQUIRED").is_none(),
            "required gramil pack missing"
        );
        return;
    }
    let mut paths = Vec::new();
    files(&dir, &mut paths);
    paths.sort();
    let mut count = 0;
    for path in &paths {
        count += check(path);
    }
    eprintln!(
        "gramil equivalence: {} files, {count} distinct parts",
        paths.len()
    );
}

fn torus() -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/full_torus.step"))
        .unwrap()
}
#[test]
fn staleness_versions_size_only_and_mapped_index() {
    let b = torus();
    let m = manifest::scan(Cursor::new(&b), &ScanOptions::default()).unwrap();
    assert_eq!(m.products.len(), 1);
    let p = &m.products[0];
    let mut changed = b.clone();
    let at = p.ranges[0][0] as usize;
    changed[at] = b'!';
    let error = m
        .read_product(&temp_source(&changed), &p.id)
        .unwrap_err()
        .to_string();
    assert!(error.contains(&p.id) && error.contains("sha256"), "{error}");
    let mut changed = b.clone();
    let at = b.len() - 1;
    assert!(changed[at].is_ascii_whitespace());
    changed[at] = if changed[at] == b'\n' { b' ' } else { b'\n' };
    m.read_product(&temp_source(&changed), &p.id).unwrap();
    assert!(m.verify_source(Cursor::new(&changed)).is_err());
    let mut shorter = b.clone();
    shorter.pop();
    let e = m
        .read_product(&temp_source(&shorter), &p.id)
        .unwrap_err()
        .to_string();
    assert!(e.contains(&p.id) && e.contains("size"));
    let mut value = serde_json::to_value(&m).unwrap();
    value["reader"]["haecceity"] = "another-reader".into();
    assert!(serde_json::from_value::<Manifest>(value).is_err());
    let mut incompatible = m.clone();
    incompatible.reader.quiddity = "another-reader".into();
    assert!(incompatible.read_product(&temp_source(&b), &p.id).is_err());
    let size_only = manifest::scan(
        Cursor::new(&b),
        &ScanOptions {
            no_product_hashes: true,
            ..ScanOptions::default()
        },
    )
    .unwrap();
    assert!(size_only.common.sha256.is_none());
    assert!(size_only.products[0].sha256.is_none());
    size_only.read_product(&temp_source(&b), &p.id).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mapped = manifest::scan(
        Cursor::new(&b),
        &ScanOptions {
            index_dir: Some(dir.path().into()),
            chunk_size: 7,
            ..ScanOptions::default()
        },
    )
    .unwrap();
    assert_eq!(mapped, m);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    let error = manifest::scan(
        Cursor::new(&b),
        &ScanOptions {
            instance_limit: 0,
            ..ScanOptions::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("manifest instance limit"));
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&b).unwrap();
    let compressed = encoder.finish().unwrap();
    let gz = manifest::scan(Cursor::new(&compressed), &ScanOptions::default()).unwrap();
    assert!(gz.source.compressed);
    assert_eq!(gz.products, m.products);
    assert_eq!(gz.source.sha256, m.source.sha256);
    gz.verify_source(Cursor::new(&compressed)).unwrap();
    let e = gz
        .read_product(&temp_source(&compressed), &p.id)
        .unwrap_err()
        .to_string();
    assert!(e.contains("compressed"));
}
#[test]
fn shared_geometry_names_products_and_entity() {
    use haecceity::p21::{Attribute, RawEntity};
    fn remap(a: &mut Attribute, offset: u64, shared: u64) {
        match a {
            Attribute::EntityRef(id) if *id != shared => *id += offset,
            Attribute::List(v) => {
                for a in v {
                    remap(a, offset, shared)
                }
            }
            Attribute::Typed { value, .. } => remap(value, offset, shared),
            _ => {}
        }
    }
    let b = torus();
    let mut text = String::from_utf8(b.clone()).unwrap();
    let doc = haecceity::p21::Document::parse(b).unwrap();
    let offset = doc.graph().entities.keys().max().unwrap() + 1;
    // Duplicate an otherwise independent part. Only the vertex's CARTESIAN_POINT #23 is
    // shared; every other geometry, placement, product and context gets a fresh number.
    let shared = 23;
    let mut extra = String::new();
    for (&id, e) in &doc.graph().entities {
        if id == shared {
            continue;
        }
        let mut e = e.clone();
        match &mut e {
            RawEntity::Simple { attributes, .. } => {
                for a in attributes {
                    remap(a, offset, shared)
                }
            }
            RawEntity::Complex { parts, .. } => {
                for a in parts.iter_mut().flat_map(|p| &mut p.attributes) {
                    remap(a, offset, shared)
                }
            }
        }
        extra.push_str(&format!(
            "#{}={};\n",
            id + offset,
            haecceity::p21::encode(&e).unwrap()
        ));
    }
    let end = text.rfind("ENDSEC;").unwrap();
    text.insert_str(end, &extra);
    let m = manifest::scan(Cursor::new(text.as_bytes()), &ScanOptions::default()).unwrap();
    assert_eq!(m.products.len(), 2);
    assert_eq!(m.refusals.len(), 1);
    let r = &m.refusals[0];
    assert_eq!(r.products, vec!["p0", "p1"]);
    assert_eq!(r.entity, shared);
    assert!(r.reason.contains("#23"));
    let file = temp_source(text.as_bytes());
    for p in &m.products {
        let e = m.read_product(&file, &p.id).unwrap_err().to_string();
        assert!(e.contains(&p.id) && e.contains("shared geometry"));
    }
}

#[test]
fn scanner_literals_and_retained_state_are_structure_only() {
    let b=b"ISO-10303-21;HEADER;ENDSEC;DATA;#123=(A('do not retain #99; ''quote''',\"0ABC\",/* #44 */#7)B(#8));ENDSEC;END-ISO-10303-21;";
    let want = statements(b, b.len()).unwrap();
    assert_eq!(want.len(), 1);
    assert_eq!(want[0].id, 123);
    assert_eq!(want[0].types, ["A", "B"]);
    assert_eq!(want[0].references, [7, 8]);
    assert!(!format!("{:?}", want).contains("do not retain"));
    for chunk in [1, 7, 4096] {
        assert_eq!(statements(b, chunk).unwrap(), want);
    }
    let edition3=b"ISO-10303-21;HEADER;ENDSEC;ANCHOR;<name;with'quote#99>=#7;ENDSEC;REFERENCE;#80=<file;name#2>;ENDSEC;DATA;#7=THING('ok');ENDSEC;END-ISO-10303-21;";
    let want = statements(edition3, edition3.len()).unwrap();
    assert_eq!(want.len(), 1);
    assert_eq!(want[0].id, 7);
    for chunk in [1, 7, 4096] {
        assert_eq!(statements(edition3, chunk).unwrap(), want);
    }
    for bad in [
        "ISO-10303-21;SIGNATURE;ENDSEC;END-ISO-10303-21;",
        "ISO-10303-21;DATA;#1=A('unterminated);ENDSEC;END-ISO-10303-21;",
        "ISO-10303-21;HEADER;/* unterminated",
        "ISO-10303-21;END-ISO-10303-21;JUNK;",
        "ISO-10303-21;DATA;A();ENDSEC;END-ISO-10303-21;",
    ] {
        for chunk in [1, 7, 4096] {
            assert!(statements(bad.as_bytes(), chunk).is_err());
        }
    }
}
