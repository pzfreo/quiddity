//! Reader-only #190 acceptance: no CLI, Python, layouts or recognition changes.
#[path = "common/parallel.rs"]
mod parallel;
#[path = "common/torus_products.rs"]
mod torus_products;
use haecceity::p21::streaming::{self, Event};
use haecceity::step::{
    StepFile,
    manifest::{self, Manifest, ScanOptions},
};
use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use torus_products::torus_products;

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
// These are intentionally geometry-free EXPRESS/removal/wire examples or committed broken
// inputs. Every refusal is asserted explicitly; check() never hides a whole-file failure.
const REFUSAL_FIXTURES: &[(&str, &str)] = &[
    (
        "tests/fixtures/ap242/removal/handmade.stp",
        "no solid or shell with faces",
    ),
    (
        "tests/fixtures/ap242/rules/invalid.stp",
        "no solid or shell with faces",
    ),
    (
        "tests/fixtures/ap242/rules/valid.stp",
        "no solid or shell with faces",
    ),
    (
        "tests/fixtures/broken/deleted_face.step",
        "#109 is referenced but not in the file",
    ),
    (
        "tests/fixtures/broken/empty_data.step",
        "no solid or shell with faces",
    ),
    (
        "tests/fixtures/broken/golden_simple_through_hole_no_bounds.step",
        "NonstandardReference: ADVANCED_FACE: 'bounds' references",
    ),
    (
        "tests/fixtures/broken/golden_complex_loop_edges.step",
        "NonstandardReference: EDGE_LOOP: 'edge_list' references",
    ),
    (
        "tests/fixtures/broken/golden_complex_bound_loop.step",
        "NonstandardReference: FACE_BOUND: 'bound' references",
    ),
    (
        "tests/fixtures/broken/golden_complex_oriented_edge.step",
        "NonstandardReference: EDGE_LOOP: 'edge_list' references",
    ),
    (
        "tests/fixtures/broken/golden_complex_inner_bound.step",
        "NonstandardReference: ADVANCED_FACE: 'bounds' references",
    ),
    (
        "tests/fixtures/broken/golden_complex_outer_bound.step",
        "NonstandardReference: ADVANCED_FACE: 'bounds' references",
    ),
    (
        "tests/fixtures/broken/golden_complex_edge_element.step",
        "NonstandardReference: ORIENTED_EDGE: 'edge_element' references",
    ),
    (
        "tests/fixtures/broken/golden_complex_face_surface.step",
        "NonstandardReference: ADVANCED_FACE: 'face_geometry' references",
    ),
    (
        "tests/fixtures/broken/header_only.step",
        "unexpected end of file",
    ),
    (
        "tests/fixtures/broken/kissing_cylinders_knife_no_bounds.step",
        "NonstandardReference: ADVANCED_FACE: 'bounds' references",
    ),
    (
        "tests/fixtures/broken/truck_pub_shaft_hole.step.gz",
        "duplicate entity #539",
    ),
    (
        "tests/fixtures/broken/truncated.step",
        "unexpected end of file",
    ),
    (
        "tests/fixtures/broken/truncated_closed.step",
        "#105 is referenced but not in the file",
    ),
    (
        "tests/fixtures/broken/unresolved_closed_curve.step",
        "closed edge's curve (Hyperbola) does not resolve",
    ),
    (
        "tests/fixtures/occt_trimmed_parabola.step",
        "no solid or shell with faces",
    ),
];
fn expected_fixture_refusal(path: &Path) -> Option<&'static str> {
    let relative = path
        .strip_prefix(env!("CARGO_MANIFEST_DIR"))
        .ok()?
        .to_str()?;
    REFUSAL_FIXTURES
        .iter()
        .find_map(|&(file, reason)| (file == relative).then_some(reason))
}
#[test]
fn whole_file_refusals_are_explicit_negative_fixture_contracts() {
    for &(file, reason) in REFUSAL_FIXTURES {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
        let b = bytes(&path);
        let error = StepFile::read(&b)
            .err()
            .unwrap_or_else(|| panic!("expected whole-file refusal for {file}"));
        assert!(error.to_string().contains(reason), "{file}: {error}");
    }
}
fn inputs() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    files(&root.join("tests/fixtures"), &mut out);
    files(&root.join("crates/haecceity/tests/fixtures"), &mut out);
    let corpus = std::env::var_os("QUIDDITY_CORPUS")
        .filter(|v| v != "off")
        .map(PathBuf::from);
    if std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_some() {
        assert!(
            corpus.as_ref().is_some_and(|p| p.is_dir()),
            "required corpus missing; set QUIDDITY_CORPUS"
        );
    }
    if let Some(corpus) = corpus.filter(|p| p.is_dir()) {
        files(&corpus, &mut out);
    }
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
#[derive(Debug, PartialEq, Eq)]
struct StatementSnapshot {
    id: u64,
    offset: u64,
    length: u32,
    gap_start: u64,
    types: Vec<String>,
    references: Vec<u64>,
}
fn statements(b: &[u8], chunk: usize) -> Result<Vec<StatementSnapshot>, String> {
    let mut out = Vec::new();
    streaming::scan_structure(b, chunk, |e| {
        if let Event::Instance(i, _) = e {
            out.push(StatementSnapshot {
                id: i.id,
                offset: i.offset,
                length: i.length,
                gap_start: i.gap_start,
                types: i.types.iter().map(str::to_owned).collect(),
                references: i.references.to_vec(),
            });
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
    check_bytes(&path.display().to_string(), &bytes(path))
}
fn check_bytes(label: &str, b: &[u8]) -> usize {
    let baseline = StepFile::read(b);
    let scanned = manifest::scan(Cursor::new(&b), &ScanOptions::default());
    let baseline = baseline.unwrap_or_else(|e| panic!("whole-file read failed for {}: {e}", label));
    let m = scanned.unwrap_or_else(|e| panic!("{}: {e}", label));
    assert_eq!(m.assemblies, baseline.assemblies, "{} hierarchy", label);
    assert_eq!(m.products.len(), baseline.parts.len(), "{} products", label);
    assert!(m.refusals.is_empty(), "{}: {:?}", label, m.refusals);
    let mut temp = temp_source(b);
    for (p, d) in m.products.iter().zip(&baseline.parts) {
        assert_eq!(
            p.product_definition, d.product_definition,
            "{} definition",
            label
        );
        assert_eq!(p.shape, d.shape);
        assert_eq!(p.name, d.name);
        assert_eq!(p.product_id, d.product_id);
        assert_eq!(p.faces, d.faces, "{} {} faces", label, p.id);
        assert_eq!(
            p.edge_count as usize,
            d.edges.len(),
            "{label} {} edges",
            p.id
        );
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
        let want: Vec<_> = d.placements.iter().map(|p| p.placement).collect();
        assert_eq!(got, want, "{} {} placements", label, p.id);
        let cut = m
            .read_product(&temp, &p.id)
            .unwrap_or_else(|e| panic!("{}: {e}", label));
        let whole = haecceity::step::read_part(b, d).unwrap();
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
            label,
            p.id
        );
        // Normals-only inverted-shell repair can change PartDefinition's edge traversal.
        // Edge indices belong to the product read; compare entity counts and sets here.
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
            label,
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
    let readable: Vec<_> = paths
        .iter()
        .filter(|p| expected_fixture_refusal(p).is_none())
        .collect();
    let count: usize = parallel::map(&readable, |path| check(path))
        .into_iter()
        .sum();
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
    incompatible.reader.haecceity = "another-reader".into();
    assert!(incompatible.read_product(&temp_source(&b), &p.id).is_err());
    let size_only = manifest::scan(
        Cursor::new(&b),
        &ScanOptions::default().with_no_product_hashes(true),
    )
    .unwrap();
    assert!(size_only.common.sha256.is_none());
    assert!(size_only.products[0].sha256.is_none());
    size_only.read_product(&temp_source(&b), &p.id).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mapped = manifest::scan(
        Cursor::new(&b),
        &ScanOptions::default()
            .with_index_dir(Some(dir.path().into()))
            .with_chunk_size(7),
    )
    .unwrap();
    assert_eq!(mapped, m);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    let error = manifest::scan(
        Cursor::new(&b),
        &ScanOptions::default().with_instance_limit(0),
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
#[path = "common/shared_geometry.rs"]
mod shared_geometry;

#[test]
fn shared_geometry_names_products_and_entity() {
    let shared = 23;
    let text = shared_geometry::source(torus(), shared);
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

#[test]
fn presentation_dependencies_stay_unowned_and_do_not_merge_with_geometry() {
    let original = torus();
    let base = manifest::scan(Cursor::new(&original), &ScanOptions::default()).unwrap();
    let mut text = String::from_utf8(original).unwrap();
    let at = text.rfind("ENDSEC;").unwrap();
    let added = "\n#1000=SURFACE_SIDE_STYLE('annotation',());\n#1001=TEXT_LITERAL('annotation',#26,$,.RIGHT.);\n#1002=COMPOSITE_TEXT('annotation',(#1001));\n#1003=EXTERNALLY_DEFINED_TEXT_FONT('annotation');\n#1004=DEFINED_SYMBOL('annotation');\n#1005=ITEM_IDENTIFIED_REPRESENTATION_USAGE('annotation','',#4,#10,(#17));\n#1006=COMMON_DATUM('annotation');\n#1007=LIMITS_AND_FITS('annotation');\n#1008=FUTURE_METADATA('other');\n";
    text.insert_str(at, added);
    let m = manifest::scan(Cursor::new(text.as_bytes()), &ScanOptions::default()).unwrap();
    assert_eq!(
        m.unowned.presentation.entities,
        base.unowned.presentation.entities + 8
    );
    assert_eq!(m.unowned.other.entities, base.unowned.other.entities + 1);
    assert_eq!(m.products, base.products);
    assert!(m.refusals.is_empty());
    let changed = text.replace("annotation", "Annotation");
    m.read_product(&temp_source(changed.as_bytes()), "p0")
        .unwrap();
    assert!(m.verify_source(Cursor::new(changed.as_bytes())).is_err());
}

#[test]
fn truck_repair_edges_are_compared_as_sets() {
    // The existing normals-only repair changes PartDefinition's edge traversal, but never
    // the identity of its EDGE_CURVEs. Face entity order remains exact.
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/truck_master_shaft_only.step.gz");
    assert_eq!(check(&path), 1);
}

#[path = "common/manifest_layouts.rs"]
mod manifest_layouts;
use manifest_layouts::layouts;

fn check_layouts(path: &Path) {
    let b = bytes(path);
    let original = StepFile::read(&b).unwrap();
    for (layout, b) in layouts(&b).iter().enumerate() {
        let rewritten = StepFile::read(b).unwrap();
        assert_eq!(rewritten.assemblies, original.assemblies);
        assert_eq!(rewritten.parts.len(), original.parts.len());
        for (a, b) in rewritten.parts.iter().zip(&original.parts) {
            assert_eq!(a.faces, b.faces);
            assert_eq!(a.product_definition, b.product_definition);
        }
        assert_eq!(
            check_bytes(&format!("{} layout {layout}", path.display()), b),
            original.parts.len()
        );
    }
}
#[test]
fn assembly_fixture_layouts() {
    check_layouts(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ap242/assembly/assembly.step"),
    );
}
#[test]
fn gramil_assembly_layouts() {
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
    check_layouts(&dir.join("gramil_assembly.step"));
}
#[test]
fn a_few_thousand_products_use_indexed_definition_lookups() {
    use haecceity::p21::{Attribute, RawEntity};
    fn remap(a: &mut Attribute, offset: u64, shared: &[u64]) {
        match a {
            Attribute::EntityRef(id) if !shared.contains(id) => *id += offset,
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
    let doc = haecceity::p21::Document::parse(b.clone()).unwrap();
    let stride = doc.graph().entities.keys().max().unwrap() + 1;
    // Contexts and the standard frame are deliberately shared, exercising cheap forward
    // references, boundary ownership and the absence of quadratic owner-prefix sets too.
    let shared = [1, 2, 8, 9, 11, 12, 13, 14, 73, 74, 75, 76, 77];
    let count = 3000;
    let mut text = String::from_utf8(b).unwrap();
    let end = text.rfind("ENDSEC;").unwrap();
    let mut extra = String::new();
    for product in 1..count {
        for (&id, e) in &doc.graph().entities {
            if shared.contains(&id) {
                continue;
            }
            let offset = product as u64 * stride;
            let mut e = e.clone();
            match &mut e {
                RawEntity::Simple { attributes, .. } => {
                    for a in attributes {
                        remap(a, offset, &shared)
                    }
                }
                RawEntity::Complex { parts, .. } => {
                    for a in parts.iter_mut().flat_map(|p| &mut p.attributes) {
                        remap(a, offset, &shared)
                    }
                }
            }
            extra.push_str(&format!(
                "#{}={};\n",
                id + offset,
                haecceity::p21::encode(&e).unwrap()
            ));
        }
    }
    text.insert_str(end, &extra);
    let m = manifest::scan(Cursor::new(text.as_bytes()), &ScanOptions::default()).unwrap();
    assert_eq!(m.products.len(), count);
    assert_eq!(m.instances.len(), count);
    assert!(m.refusals.is_empty());
    assert!(m.products.iter().all(|p| p.faces.len() == 1));
}

#[test]
fn header_hash_versions_bounds_and_two_forward_passes() {
    use std::io::{Seek, SeekFrom};
    struct Replay {
        bytes: Cursor<Vec<u8>>,
        seeks: Vec<u64>,
        read_bytes: usize,
    }
    impl Read for Replay {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            let n = self.bytes.read(b)?;
            self.read_bytes += n;
            Ok(n)
        }
    }
    impl Seek for Replay {
        fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
            let at = self.bytes.seek(from)?;
            self.seeks.push(at);
            Ok(at)
        }
    }
    let b = torus();
    let mut compressed = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    compressed.write_all(&b).unwrap();
    let gz = compressed.finish().unwrap();
    for bytes in [b.clone(), gz] {
        let len = bytes.len();
        let mut replay = Replay {
            bytes: Cursor::new(bytes),
            seeks: Vec::new(),
            read_bytes: 0,
        };
        let m = manifest::scan(&mut replay, &ScanOptions::default()).unwrap();
        assert_eq!(
            replay.seeks,
            [0, 0, 0],
            "only the magic probe and one source rewind; post-scan work never seeks backwards"
        );
        assert!(replay.read_bytes <= 2 * len + 2);
        assert_eq!(m.products.len(), 1);
    }
    let m = manifest::scan(Cursor::new(&b), &ScanOptions::default()).unwrap();
    let mut changed = b.clone();
    let at = changed
        .windows(b"Open CASCADE Model".len())
        .position(|w| w == b"Open CASCADE Model")
        .unwrap();
    changed[at] = b'o';
    let e = m
        .read_product(&temp_source(&changed), "p0")
        .unwrap_err()
        .to_string();
    assert!(e.contains("p0") && e.contains("common sha256"), "{e}");
    let mut stale = m.clone();
    stale.reader.haecceity = "old-reader".into();
    let e = stale
        .read_product(&temp_source(&b), "p0")
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("old-reader") && e.contains(env!("CARGO_PKG_VERSION")),
        "{e}"
    );
    let mut huge = m;
    let size = manifest::MAX_HEADER_BYTES + 1;
    huge.source.size = size;
    huge.source.header = [0, size];
    let file = temp_source(&b);
    file.set_len(size).unwrap();
    let e = huge.read_product(&file, "p0").unwrap_err().to_string();
    assert!(e.contains("header exceeds"), "{e}");
}

#[test]
fn local_and_shared_geometry_contexts_are_product_ranges() {
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
    let doc = haecceity::p21::Document::parse(b.clone()).unwrap();
    let offset = doc.graph().entities.keys().max().unwrap() + 1;
    let shared = 41;
    let mut text = String::from_utf8(b).unwrap();
    let at = text.rfind("ENDSEC;").unwrap();
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
    text.insert_str(at, &extra);
    let m = manifest::scan(Cursor::new(text.as_bytes()), &ScanOptions::default()).unwrap();
    assert_eq!(m.products.len(), 2);
    assert!(m.refusals.is_empty());
    let rows = statements(text.as_bytes(), 4096).unwrap();
    let context = rows.iter().find(|r| r.id == shared).unwrap();
    let contains = |ranges: &[manifest::ByteRange]| {
        ranges
            .iter()
            .any(|r| r[0] <= context.offset && context.offset < r[1])
    };
    assert!(!contains(&m.common.ranges));
    assert!(m.products.iter().all(|p| contains(&p.ranges)));
    check_bytes("shared geometry-only context", text.as_bytes());
    let original = m.common.entities;
    let at = text.rfind("ENDSEC;").unwrap();
    let mut unused = String::new();
    for id in 1000..2000 {
        unused.push_str(&format!("#{id}=(GEOMETRIC_REPRESENTATION_CONTEXT(2) PARAMETRIC_REPRESENTATION_CONTEXT() REPRESENTATION_CONTEXT('unused',''));\n"));
    }
    // An unrepresented shape representation must not promote its context into common.
    unused.push_str("#2001=SHAPE_REPRESENTATION('',(),#1000);\n");
    text.insert_str(at, &unused);
    let m = manifest::scan(Cursor::new(text.as_bytes()), &ScanOptions::default()).unwrap();
    assert_eq!(m.common.entities, original);
    check_bytes("unowned contexts", text.as_bytes());
}

#[test]
fn occurrence_representation_contexts_are_reached_from_assembly_chains() {
    use haecceity::p21::{Attribute, RawEntity};
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ap242/assembly/assembly.step");
    let b = bytes(&path);
    let doc = haecceity::p21::Document::parse(b.clone()).unwrap();
    let (&relationship,raw)=doc.graph().entities.iter().find(|(_,e)|matches!(e,RawEntity::Complex{parts,..} if parts.iter().any(|p|p.name=="REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION"))).unwrap();
    let mut relation = raw.clone();
    let RawEntity::Complex { parts, .. } = &mut relation else {
        unreachable!()
    };
    let part = parts
        .iter_mut()
        .find(|p| p.name == "REPRESENTATION_RELATIONSHIP")
        .unwrap();
    let Attribute::EntityRef(rep) = &part.attributes[2] else {
        unreachable!()
    };
    let mut representation = doc.graph().entities[rep].clone();
    let next = doc.graph().entities.keys().max().unwrap() + 1;
    part.attributes[2] = Attribute::EntityRef(next);
    let RawEntity::Simple { attributes, .. } = &mut representation else {
        unreachable!()
    };
    let Attribute::EntityRef(context) = attributes[2] else {
        unreachable!()
    };
    let context = doc.graph().entities[&context].clone();
    attributes[2] = Attribute::EntityRef(next + 1);
    let rows = statements(&b, 4096).unwrap();
    let first = rows.first().unwrap().offset as usize;
    let last = rows.last().unwrap();
    let end = (last.offset + u64::from(last.length)) as usize;
    let mut out = b[..first].to_vec();
    for row in rows {
        if row.id == relationship {
            out.extend_from_slice(
                format!(
                    "#{}={};",
                    row.id,
                    haecceity::p21::encode(&relation).unwrap()
                )
                .as_bytes(),
            );
        } else {
            out.extend_from_slice(
                &b[row.offset as usize..row.offset as usize + row.length as usize],
            );
        }
        out.push(b'\n');
    }
    out.extend_from_slice(
        format!(
            "#{next}={};\n#{}={};\n",
            haecceity::p21::encode(&representation).unwrap(),
            next + 1,
            haecceity::p21::encode(&context).unwrap()
        )
        .as_bytes(),
    );
    out.extend_from_slice(&b[end..]);
    let m = manifest::scan(Cursor::new(&out), &ScanOptions::default()).unwrap();
    let rows = statements(&out, 4096).unwrap();
    let context = rows.iter().find(|r| r.id == next + 1).unwrap();
    let within = |ranges: &[manifest::ByteRange]| {
        ranges
            .iter()
            .any(|r| r[0] <= context.offset && context.offset < r[1])
    };
    // The occurrence representation relates to one part's representation, so its context
    // travels with that product alone rather than sitting in common. The placements, which
    // read it too, still agree with the whole-file read below.
    assert!(!within(&m.common.ranges));
    assert_eq!(m.products.iter().filter(|p| within(&p.ranges)).count(), 1);
    check_bytes("occurrence-only representation context", &out);
}

#[test]
fn common_does_not_grow_with_products_that_have_their_own_contexts() {
    // Each product's shape representation, product context and definition context are its
    // own (#73 with its units and uncertainty, #8, #9 in the torus), so they belong to that
    // product's ranges. Only what every product shares may stay in common.
    let mut entities = Vec::new();
    for copies in [2, 4, 8] {
        let (text, stride) = torus_products(copies);
        let m = manifest::scan(Cursor::new(&text), &ScanOptions::default()).unwrap();
        assert_eq!(m.products.len() as u64, copies);
        assert!(m.refusals.is_empty());
        entities.push(m.common.entities);
        let rows = statements(&text, 4096).unwrap();
        let contains = |ranges: &[manifest::ByteRange], id: u64| {
            let row = rows.iter().find(|r| r.id == id).unwrap();
            ranges
                .iter()
                .any(|r| r[0] <= row.offset && row.offset < r[1])
        };
        for copy in 0..copies {
            let own = copy * stride;
            // #5 is the torus's product definition.
            let product = m
                .products
                .iter()
                .find(|p| p.product_definition == 5 + own)
                .unwrap();
            // The shape representation's context with its units and uncertainty, and the
            // product's context and definition context, in each copy.
            for id in [8, 9, 73, 74, 75, 76, 77] {
                assert!(
                    contains(&product.ranges, id + own),
                    "{} lacks #{}",
                    product.id,
                    id + own
                );
                assert!(!contains(&m.common.ranges, id + own));
            }
        }
        assert!(
            contains(&m.common.ranges, 2),
            "the shared application context"
        );
        check_bytes("products with their own contexts", &text);
    }
    assert!(
        entities.windows(2).all(|w| w[0] == w[1]),
        "common grew with the product count: {entities:?}"
    );
}

/// `subs` sub-assemblies under one root, each holding one torus from `torus_products`. As the
/// OCCT writer does, every assembly product has its own product context, definition context and
/// geometric context with units, and every link its own placement.
fn nested_torus_assemblies(subs: u64) -> Vec<u8> {
    struct Ids {
        next: u64,
        text: String,
    }
    impl Ids {
        fn add(&mut self, entity: String) -> u64 {
            let id = self.next;
            self.next += 1;
            self.text.push_str(&format!("#{id}={entity};\n"));
            id
        }
        /// An assembly product with its own contexts; returns its definition and the
        /// representation (placing the origin and `placements` frames) its shape definition uses.
        fn assembly(&mut self, name: &str, placements: &[u64]) -> (u64, u64) {
            let pc = self.add("PRODUCT_CONTEXT('',#2,'mechanical')".into());
            let dc = self.add("PRODUCT_DEFINITION_CONTEXT('part definition',#2,'design')".into());
            let product = self.add(format!("PRODUCT('{name}','{name}','',(#{pc}))"));
            let formation = self.add(format!("PRODUCT_DEFINITION_FORMATION('','',#{product})"));
            let pd = self.add(format!(
                "PRODUCT_DEFINITION('design','',#{formation},#{dc})"
            ));
            let shape = self.add(format!("PRODUCT_DEFINITION_SHAPE('','',#{pd})"));
            let (len, angle, solid, uncertainty) =
                (self.next + 1, self.next + 2, self.next + 3, self.next + 4);
            let context = self.add(format!(
                "( GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{uncertainty})) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{len},#{angle},#{solid})) REPRESENTATION_CONTEXT('Context','3D Context with UNIT and UNCERTAINTY') )"
            ));
            self.add("( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) )".into());
            self.add("( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.) )".into());
            self.add("( NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT() )".into());
            self.add(format!(
                "UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-07),#{len},'distance_accuracy_value','confusion accuracy')"
            ));
            let items: Vec<_> = std::iter::once(11)
                .chain(placements.iter().copied())
                .map(|i| format!("#{i}"))
                .collect();
            let rep = self.add(format!(
                "SHAPE_REPRESENTATION('',({}),#{context})",
                items.join(",")
            ));
            self.add(format!("SHAPE_DEFINITION_REPRESENTATION(#{shape},#{rep})"));
            (pd, rep)
        }
        fn link(&mut self, k: u64, parent: (u64, u64), child: (u64, u64), placement: u64) {
            let usage = self.add(format!(
                "NEXT_ASSEMBLY_USAGE_OCCURRENCE('{k}','','',#{},#{},$)",
                parent.0, child.0
            ));
            let shape = self.add(format!(
                "PRODUCT_DEFINITION_SHAPE('Placement','Placement of an item',#{usage})"
            ));
            let transform = self.add(format!(
                "ITEM_DEFINED_TRANSFORMATION('','',#11,#{placement})"
            ));
            let relation = self.add(format!(
                "( REPRESENTATION_RELATIONSHIP('','',#{},#{}) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{transform}) SHAPE_REPRESENTATION_RELATIONSHIP() )",
                child.1, parent.1
            ));
            self.add(format!(
                "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{relation},#{shape})"
            ));
        }
        fn frame(&mut self, x: u64) -> u64 {
            let point = self.add(format!("CARTESIAN_POINT('',({x}.,0.,0.))"));
            self.add(format!("AXIS2_PLACEMENT_3D('',#{point},#13,#14)"))
        }
    }
    let (text, stride) = torus_products(subs);
    let mut text = String::from_utf8(text).unwrap();
    let mut ids = Ids {
        next: subs * stride + 1,
        text: String::new(),
    };
    let mut children = Vec::new();
    for sub in 0..subs {
        let place = ids.frame(sub * 10);
        let node = ids.assembly(&format!("sub{sub}"), &[place]);
        // The torus of this copy: its definition #5 and representation #10.
        let part = (5 + sub * stride, 10 + sub * stride);
        ids.link(sub, node, part, place);
        children.push(node);
    }
    let places: Vec<_> = (0..subs).map(|s| ids.frame(s * 100)).collect();
    let root = ids.assembly("root", &places);
    for (k, (&child, &place)) in children.iter().zip(&places).enumerate() {
        ids.link(subs + k as u64, root, child, place);
    }
    let end = text.rfind("ENDSEC;").unwrap();
    text.insert_str(end, &ids.text);
    text.into_bytes()
}

#[test]
fn common_does_not_grow_with_sub_assemblies_that_have_their_own_contexts() {
    let mut entities = Vec::new();
    for subs in [2, 4, 8] {
        let text = nested_torus_assemblies(subs);
        let m = manifest::scan(Cursor::new(&text), &ScanOptions::default()).unwrap();
        assert_eq!(m.products.len() as u64, subs);
        assert_eq!(
            m.instances.len() as u64,
            subs,
            "one occurrence of each torus"
        );
        assert!(m.refusals.is_empty());
        entities.push(m.common.entities);
        // Reading and placing still work with the assembly contexts out of common.
        check_bytes("sub-assemblies with their own contexts", &text);
    }
    assert!(
        entities.windows(2).all(|w| w[0] == w[1]),
        "common grew with the sub-assembly count: {entities:?}"
    );
}

#[test]
fn threads_share_one_file_to_read_products() {
    let (text, _) = torus_products(6);
    let m = manifest::scan(Cursor::new(&text), &ScanOptions::default()).unwrap();
    let file = temp_source(&text);
    let faces = |id: &str| {
        let part = m.read_product(&file, id).unwrap();
        (0..part.faces.len())
            .map(|f| part.face_source(f).unwrap().entity)
            .collect::<Vec<_>>()
    };
    let expected: Vec<_> = m.products.iter().map(|p| faces(&p.id)).collect();
    #[cfg(unix)]
    let cursor = std::io::Seek::stream_position(&mut &file).unwrap();
    std::thread::scope(|scope| {
        for thread in 0..4 {
            let (m, expected, faces) = (&m, &expected, &faces);
            scope.spawn(move || {
                for round in 0..10 {
                    for step in 0..m.products.len() {
                        let i = (step + thread + round) % m.products.len();
                        assert_eq!(&faces(&m.products[i].id), &expected[i]);
                    }
                }
            });
        }
    });
    // Positional reads leave the shared cursor where it was.
    #[cfg(unix)]
    assert_eq!(std::io::Seek::stream_position(&mut &file).unwrap(), cursor);
}

#[test]
fn assembly_product_contexts_stay_with_their_products() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ap242/assembly/assembly.step");
    let b = bytes(&path);
    let m = manifest::scan(Cursor::new(&b), &ScanOptions::default()).unwrap();
    let rows = statements(&b, 4096).unwrap();
    let contains = |ranges: &[manifest::ByteRange], id: u64| {
        let row = rows.iter().find(|r| r.id == id).unwrap();
        ranges
            .iter()
            .any(|r| r[0] <= row.offset && row.offset < r[1])
    };
    // #27 is the assembly's own context: only the assembly reads it, so it is neither common
    // nor a part's. #959 (plate) and #1256 (pin) are the parts'.
    assert!(!contains(&m.common.ranges, 27));
    assert!(m.products.iter().all(|p| !contains(&p.ranges, 27)));
    assert!(contains(&m.unowned.other.ranges, 27));
    for (name, context) in [("plate", 959), ("pin", 1256)] {
        let product = m.products.iter().find(|p| p.name == name).unwrap();
        assert!(contains(&product.ranges, context), "{name}");
        assert!(!contains(&m.common.ranges, context), "{name}");
    }
    check_bytes("assembly with per-part contexts", &b);
}

#[test]
fn edge_count_excludes_unused_edges_in_the_same_geometry_component() {
    let b = torus();
    let doc = haecceity::p21::Document::parse(b.clone()).unwrap();
    let edge = doc
        .graph()
        .entities
        .values()
        .find(
            |e| matches!(e, haecceity::p21::RawEntity::Simple { name, .. } if name == "EDGE_CURVE"),
        )
        .unwrap();
    // This duplicate shares vertices and a curve with the product but no face loop uses it.
    let id = doc.graph().entities.keys().max().unwrap() + 1;
    let mut text = String::from_utf8(b).unwrap();
    let at = text.rfind("ENDSEC;").unwrap();
    text.insert_str(
        at,
        &format!("#{id}={};\n", haecceity::p21::encode(edge).unwrap()),
    );
    check_bytes("unused edge in owned geometry", text.as_bytes());
}

#[test]
fn product_lookup_survives_serde_and_explicit_membership_changes() {
    let mut m = manifest::scan(Cursor::new(torus()), &ScanOptions::default()).unwrap();
    let original = m.products[0].clone();
    m.products = (0..4000)
        .map(|i| {
            let mut p = original.clone();
            p.id = format!("p{i}");
            p.product_definition = i;
            p
        })
        .collect();
    m.refusals.push(manifest::Refusal::new(
        vec!["p3999".into()],
        23,
        "indexed refusal".into(),
    ));
    m.reindex_products().unwrap();
    let value = serde_json::to_value(&m).unwrap();
    assert!(value.get("product_index").is_none());
    let mut round: Manifest = serde_json::from_value(value).unwrap();
    assert_eq!(round, m);
    let file = temp_source(&torus());
    assert!(
        round
            .read_product(&file, "p3999")
            .unwrap_err()
            .to_string()
            .contains("indexed refusal")
    );
    for i in 0..4000 {
        assert_eq!(
            round.product(&format!("p{i}")).unwrap().product_definition,
            i
        );
    }
    assert!(
        round
            .product("missing")
            .unwrap_err()
            .to_string()
            .contains("missing")
    );
    round.products.swap(0, 3999);
    assert!(
        round
            .product("p0")
            .unwrap_err()
            .to_string()
            .contains("reindex_products")
    );
    round.reindex_products().unwrap();
    assert_eq!(round.product("p0").unwrap().product_definition, 0);
    round.products[0].id = round.products[1].id.clone();
    assert!(
        round
            .reindex_products()
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
    assert!(serde_json::from_value::<Manifest>(serde_json::to_value(&round).unwrap()).is_err());
}

#[path = "common/manifest_occurrences.rs"]
mod occurrences;

#[test]
fn multi_solid_occurrence_places_every_product_face_once() {
    let b = include_bytes!("fixtures/two_solids.step");
    let whole = StepFile::read(b).unwrap();
    assert_eq!(whole.parts[0].placements[0].instances.len(), 2);
    let m = manifest::scan(Cursor::new(b), &ScanOptions::default()).unwrap();
    assert_eq!(m.products.len(), 1);
    assert_eq!(m.instances.len(), 1);
    assert!(m.instances[0].path.is_empty());
    assert_eq!(m.products[0].faces, whole.parts[0].faces);
    let part = m.read_product(&temp_source(b), "p0").unwrap();
    assert_eq!(part.faces.len(), whole.parts[0].faces.len());
    check_bytes("two-solid occurrence", b);
}

#[test]
fn nested_multi_solid_instances_count_leaf_occurrences() {
    let b = occurrences::nested();
    let m = manifest::scan(Cursor::new(&b), &ScanOptions::default()).unwrap();
    assert_eq!(m.products.len(), 2);
    assert_eq!(m.assemblies.len(), 3);
    assert_eq!(m.instances.len(), 5);
    assert!(m.instances.iter().all(|i| i.path.len() == 2));
    let multi = m.products.iter().find(|p| p.faces.len() == 3).unwrap();
    assert_eq!(
        m.instances.iter().filter(|i| i.product == multi.id).count(),
        2
    );
    check_bytes("nested multi-solid occurrences", &b);
    let error = manifest::scan(
        Cursor::new(&b),
        &ScanOptions::default().with_instance_limit(7),
    )
    .unwrap_err();
    assert!(error.to_string().contains("manifest instance limit 7"));
}

#[test]
fn old_manifest_format_requires_rescan() {
    let m = manifest::scan(Cursor::new(torus()), &ScanOptions::default()).unwrap();
    let mut old = serde_json::to_value(&m).unwrap();
    old["format"] = "quiddity-manifest/1".into();
    let bytes = serde_json::to_vec(&old).unwrap();
    let error = serde_json::from_slice::<Manifest>(&bytes)
        .unwrap_err()
        .to_string();
    assert!(error.contains("rescan the source"), "{error}");
    let error = Manifest::read_selected(Cursor::new(bytes), &["p0".into()], |_| Ok(()))
        .unwrap_err()
        .to_string();
    assert!(error.contains("rescan the source"), "{error}");
}
