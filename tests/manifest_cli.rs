//! Manifest CLI acceptance against whole-file recognition computed in this test.
#[path = "common/manifest_layouts.rs"]
mod layouts;
use haecceity::step::{StepFile, manifest::Manifest};
use quiddity::{file_api, serve::RecognitionOutput};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quiddity"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}
fn success(output: Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
}
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
fn fixture() -> PathBuf {
    root().join("tests/fixtures/ap242/assembly/assembly.step")
}
fn scan(dir: &Path, path: &Path) -> Manifest {
    success(run(
        dir,
        &["manifest", path.to_str().unwrap(), "-o", "manifest.json"],
    ));
    serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap()
}
fn recognise(dir: &Path, path: &Path, extra: &[&str]) -> Output {
    let mut args = vec![
        "recognise",
        path.to_str().unwrap(),
        "--manifest",
        "manifest.json",
        "--out-dir",
        "results",
    ];
    args.extend_from_slice(extra);
    run(dir, &args)
}
fn value(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}
fn refusal_values(dir: &Path) -> Vec<Value> {
    let mut paths: Vec<_> = std::fs::read_dir(dir.join("results/refusals"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    paths.sort();
    paths.iter().map(|p| value(p)).collect()
}
fn check(path: &Path) -> usize {
    let dir = tempfile::tempdir().unwrap();
    let m = scan(dir.path(), path);
    success(recognise(dir.path(), path, &[]));
    assert!(refusal_values(dir.path()).is_empty());
    assert!(!dir.path().join("results/refusals.json").exists());
    let bytes = std::fs::read(path).unwrap();
    let whole = StepFile::read(&bytes).unwrap();
    let source_sha256: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let assembly = value(&dir.path().join("results/assembly.json"));
    assert_eq!(assembly["source"], serde_json::to_value(&m.source).unwrap());
    assert_eq!(
        assembly["instances"],
        serde_json::to_value(&m.instances).unwrap()
    );
    assert_eq!(m.products.len(), whole.parts.len());
    for (i, product) in m.products.iter().enumerate() {
        let definition = &whole.parts[i];
        assert_eq!(definition.product_definition, product.product_definition);
        assert_eq!(definition.faces, product.faces);
        assert_eq!(definition.edges.len() as u64, product.edge_count);
        let placements: Vec<_> = definition
            .placements
            .iter()
            .flat_map(|p| p.instances.iter().map(|_| p.placement))
            .collect();
        let got: Vec<_> = m
            .instances
            .iter()
            .filter(|k| k.product == product.id)
            .map(|k| k.placement)
            .collect();
        assert_eq!(got, placements);
        let part = whole.part(i).unwrap();
        // Independent whole-file baseline: do not use the new product wrapper for expected JSON.
        let mut recognition = quiddity::correspondence::recognise(&part);
        recognition.source_sha256 = Some(source_sha256.clone());
        let document = quiddity::recognition::document(&recognition, part.faces.len());
        let expected = file_api::json(&RecognitionOutput {
            recognition,
            document,
        })
        .unwrap();
        let actual = std::fs::read_to_string(
            dir.path()
                .join(format!("results/products/{}.json", product.id)),
        )
        .unwrap();
        assert_eq!(actual, expected, "{} {}", path.display(), product.id);
    }
    m.products.len()
}
#[test]
fn every_assembly_fixture() {
    let mut files: Vec<_> = std::fs::read_dir(root().join("tests/fixtures/ap242/assembly"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "step"))
        .collect();
    files.sort();
    assert_eq!(files.len(), 4);
    let count: usize = files.iter().map(|p| check(p)).sum();
    eprintln!(
        "CLI assembly equivalence: {} files, {count} products",
        files.len()
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
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "step"))
        .collect();
    files.sort();
    assert!(!files.is_empty());
    let count: usize = files.iter().map(|p| check(p)).sum();
    eprintln!(
        "CLI gramil equivalence: {} files, {count} products",
        files.len()
    );
}
#[test]
fn shuffled_layout() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("shuffled.step");
    let bytes = std::fs::read(fixture()).unwrap();
    std::fs::write(&source, layouts::layouts(&bytes).pop().unwrap()).unwrap();
    check(&source);
}
#[test]
fn writes_only_output_and_cleans_index() {
    let dir = tempfile::tempdir().unwrap();
    let index = tempfile::tempdir().unwrap();
    success(run(
        dir.path(),
        &[
            "manifest",
            fixture().to_str().unwrap(),
            "-o",
            "only.json",
            "--index-dir",
            index.path().to_str().unwrap(),
        ],
    ));
    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(entries, ["only.json"]);
    assert_eq!(std::fs::read_dir(index.path()).unwrap().count(), 0);
    // Cleanup also applies when a scan refuses malformed input.
    let broken = root().join("tests/fixtures/broken/truncated.step");
    let out = run(
        dir.path(),
        &[
            "manifest",
            broken.to_str().unwrap(),
            "-o",
            "bad.json",
            "--index-dir",
            index.path().to_str().unwrap(),
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(!dir.path().join("bad.json").exists());
    assert_eq!(std::fs::read_dir(index.path()).unwrap().count(), 0);
}
#[test]
fn selection_and_unknown_ids() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture();
    let m = scan(dir.path(), &path);
    let id = &m.products[0].id;
    success(recognise(dir.path(), &path, &["--parts", id]));
    assert_eq!(
        std::fs::read_dir(dir.path().join("results/products"))
            .unwrap()
            .count(),
        1
    );
    let a = value(&dir.path().join("results/assembly.json"));
    assert_eq!(a["instances"], serde_json::to_value(&m.instances).unwrap());
    let output = recognise(dir.path(), &path, &["--parts", "p999,p998"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown manifest product ids: p999, p998"),
        "{stderr}"
    );
}
#[test]
fn compressed_scan_but_no_implicit_unpack() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.gz");
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&std::fs::read(fixture()).unwrap()).unwrap();
    std::fs::write(&path, gz.finish().unwrap()).unwrap();
    assert!(scan(dir.path(), &path).source.compressed);
    let out = recognise(dir.path(), &path, &[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("compressed source"));
    assert!(!dir.path().join("results").exists());
    // Detect gzip magic even when the manifest itself describes an uncompressed file.
    scan(dir.path(), &fixture());
    let out = recognise(dir.path(), &path, &[]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("compressed source"));
}
#[test]
fn staleness_product_common_and_full_source() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.step");
    let original = std::fs::read(fixture()).unwrap();
    std::fs::write(&path, &original).unwrap();
    let m = scan(dir.path(), &path);
    let product = &m.products[0];
    // Change a byte inside the PRODUCT name, keeping the STEP valid and the source size fixed.
    let mut changed = original.clone();
    let name = product.name.as_bytes();
    let at = product
        .ranges
        .iter()
        .find_map(|r| {
            let start = r[0] as usize;
            original[start..r[1] as usize]
                .windows(name.len())
                .position(|w| w == name)
                .map(|i| start + i)
        })
        .unwrap();
    changed[at] = if changed[at] == b'X' { b'Y' } else { b'X' };
    std::fs::write(&path, &changed).unwrap();
    let out = recognise(dir.path(), &path, &["--parts", &product.id]);
    assert_eq!(out.status.code(), Some(1));
    let refusals = refusal_values(dir.path());
    assert_eq!(refusals[0]["product"], product.id);
    assert!(
        refusals[0]["reason"]
            .as_str()
            .unwrap()
            .contains("sha256 mismatch")
    );
    assert!(
        !dir.path()
            .join(format!("results/products/{}.json", product.id))
            .exists()
    );
    success(recognise(
        dir.path(),
        &path,
        &["--parts", &product.id, "--no-product-hashes"],
    ));
    let out = recognise(
        dir.path(),
        &path,
        &["--no-product-hashes", "--verify-source"],
    );
    assert_eq!(out.status.code(), Some(1));
    // Header is included in the common hash.
    changed = original.clone();
    let at = original.windows(9).position(|w| w == b"FILE_NAME").unwrap() + 11;
    changed[at] = b'X';
    std::fs::write(&path, &changed).unwrap();
    assert_eq!(recognise(dir.path(), &path, &[]).status.code(), Some(1));
    let refusals = refusal_values(dir.path());
    assert!(
        refusals[0]["reason"]
            .as_str()
            .unwrap()
            .contains("common sha256 mismatch")
    );
    // Whitespace after the last statement lies outside header/common/product ranges.
    changed = original.clone();
    let at = changed.iter().rposition(|&b| b == b'\n').unwrap();
    assert!(at >= m.source.header[1] as usize);
    assert!(
        !m.products
            .iter()
            .flat_map(|p| &p.ranges)
            .chain(&m.common.ranges)
            .any(|r| r[0] <= at as u64 && (at as u64) < r[1])
    );
    changed[at] = b' ';
    std::fs::write(&path, &changed).unwrap();
    success(recognise(dir.path(), &path, &[]));
    let out = recognise(dir.path(), &path, &["--verify-source"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("sha256"));
}
#[test]
fn unsafe_manifest_ids_cannot_write_outside_products() {
    let dir = tempfile::tempdir().unwrap();
    scan(dir.path(), &fixture());
    let mut m = value(&dir.path().join("manifest.json"));
    m["products"][0]["id"] = "../../escape".into();
    std::fs::write(
        dir.path().join("manifest.json"),
        serde_json::to_vec(&m).unwrap(),
    )
    .unwrap();
    assert_eq!(
        recognise(dir.path(), &fixture(), &[]).status.code(),
        Some(1)
    );
    assert!(!dir.path().join("results").exists());
}

#[path = "common/shared_geometry.rs"]
mod shared_geometry;
#[test]
fn shared_geometry_refuses_and_explicit_fallback_completes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shared.step");
    let torus = std::fs::read(root().join("tests/fixtures/full_torus.step")).unwrap();
    std::fs::write(&path, shared_geometry::source(torus, 23)).unwrap();
    let m = scan(dir.path(), &path);
    assert_eq!(m.products.len(), 2);
    assert!(!m.refusals.is_empty());
    let out = recognise(dir.path(), &path, &[]);
    assert_eq!(out.status.code(), Some(1));
    let refusals = refusal_values(dir.path());
    assert_eq!(refusals.len(), 2);
    assert!(refusals.iter().all(|r| {
        r["recovered"] == false
            && r["reason"]
                .as_str()
                .unwrap()
                .contains("--fallback-whole-file")
    }));
    success(recognise(dir.path(), &path, &["--fallback-whole-file"]));
    success(recognise(
        dir.path(),
        &path,
        &["--fallback-whole-file", "--verify-source"],
    ));
    let refusals = refusal_values(dir.path());
    assert!(refusals.iter().all(|r| r["recovered"] == true));
    let bytes = std::fs::read(&path).unwrap();
    let whole = StepFile::read(&bytes).unwrap();
    for (i, p) in m.products.iter().enumerate() {
        let expected = file_api::recognise_part(&whole.part(i).unwrap(), &m.source.sha256).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join(format!("results/products/{}.json", p.id)))
                .unwrap(),
            expected
        );
    }
    // Default fallback binds the whole source; an explicit size-only policy still applies.
    let mut changed = bytes;
    let last = changed.len() - 1;
    changed[last] = b' ';
    std::fs::write(&path, changed).unwrap();
    assert_eq!(
        recognise(dir.path(), &path, &["--fallback-whole-file"])
            .status
            .code(),
        Some(1)
    );
    success(recognise(
        dir.path(),
        &path,
        &["--fallback-whole-file", "--no-product-hashes"],
    ));
    assert_eq!(
        recognise(
            dir.path(),
            &path,
            &[
                "--fallback-whole-file",
                "--no-product-hashes",
                "--verify-source"
            ]
        )
        .status
        .code(),
        Some(1)
    );
}

fn output_tree(dir: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, files: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    std::fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = std::collections::BTreeMap::new();
    walk(dir, dir, &mut files);
    files
}

fn check_workers(path: &Path, refused: bool) {
    let full = tempfile::tempdir().unwrap();
    let workers = tempfile::tempdir().unwrap();
    let m = scan(full.path(), path);
    assert!(m.products.len() > 1);
    let complete = recognise(full.path(), path, &[]);
    assert_eq!(
        complete.status.code(),
        Some(i32::from(refused)),
        "{}",
        String::from_utf8_lossy(&complete.stderr)
    );
    scan(workers.path(), path);
    let expected = output_tree(&full.path().join("results"));
    // Serial workers retain the first worker's products/refusals and full assembly metadata.
    for p in &m.products {
        let run = recognise(workers.path(), path, &["--parts", &p.id]);
        assert_eq!(run.status.code(), Some(i32::from(refused)));
        assert_eq!(
            std::fs::read(workers.path().join("results/assembly.json")).unwrap(),
            expected[Path::new("assembly.json")]
        );
    }
    assert_eq!(output_tree(&workers.path().join("results")), expected);
    // Exercise independent processes racing to replace the same assembly.json too.
    std::fs::remove_dir_all(workers.path().join("results")).unwrap();
    let jobs: Vec<_> = m
        .products
        .iter()
        .map(|p| {
            Command::new(env!("CARGO_BIN_EXE_quiddity"))
                .current_dir(workers.path())
                .args([
                    "recognise",
                    path.to_str().unwrap(),
                    "--manifest",
                    "manifest.json",
                    "--out-dir",
                    "results",
                    "--parts",
                    &p.id,
                ])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for job in jobs {
        assert_eq!(
            job.wait_with_output().unwrap().status.code(),
            Some(i32::from(refused))
        );
    }
    assert_eq!(output_tree(&workers.path().join("results")), expected);
}

#[test]
fn disjoint_workers_share_output_directory() {
    check_workers(&fixture(), false);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shared.step");
    let torus = std::fs::read(root().join("tests/fixtures/full_torus.step")).unwrap();
    std::fs::write(&path, shared_geometry::source(torus, 23)).unwrap();
    check_workers(&path, true);
}

#[test]
fn product_warnings_match_legacy_stderr_with_id_prefix() {
    let path = root().join("tests/fixtures/full_torus.step");
    let dir = tempfile::tempdir().unwrap();
    let legacy = run(dir.path(), &[path.to_str().unwrap()]);
    assert!(legacy.status.success());
    let warnings = String::from_utf8(legacy.stderr).unwrap();
    assert!(
        !warnings.is_empty(),
        "fixture must exercise recognition warnings"
    );
    let m = scan(dir.path(), &path);
    let result = recognise(dir.path(), &path, &[]);
    assert!(result.status.success());
    let expected: Vec<_> = warnings
        .lines()
        .map(|line| {
            format!(
                "quiddity: product {}: {}",
                m.products[0].id,
                line.strip_prefix("quiddity: ").unwrap()
            )
        })
        .collect();
    assert_eq!(
        String::from_utf8(result.stderr)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn selective_loader_validates_unselected_ids_and_streams_exact_assembly() {
    let dir = tempfile::tempdir().unwrap();
    let m = scan(dir.path(), &fixture());
    success(recognise(
        dir.path(),
        &fixture(),
        &["--parts", &m.products[0].id],
    ));
    let expected =
        quiddity::file_api::json(&serde_json::json!({"source":m.source,"instances":m.instances}))
            .unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("results/assembly.json")).unwrap(),
        expected
    );
    for bad in ["../bad", &m.products[0].id] {
        let isolated = tempfile::tempdir().unwrap();
        let mut v = serde_json::to_value(&m).unwrap();
        v["products"][1]["id"] = bad.into();
        std::fs::write(
            isolated.path().join("manifest.json"),
            serde_json::to_vec(&v).unwrap(),
        )
        .unwrap();
        assert_eq!(
            recognise(isolated.path(), &fixture(), &["--parts", &m.products[0].id])
                .status
                .code(),
            Some(1)
        );
        assert!(!isolated.path().join("results").exists());
        assert_eq!(std::fs::read_dir(isolated.path()).unwrap().count(), 1);
    }
}
