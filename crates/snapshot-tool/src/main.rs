//! Orchestrate Rust-owned snapshot producers; expected values and deterministic gzip are
//! produced by the tests, never by a second implementation of recognition.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
type Digests = BTreeMap<String, String>;
const PREFIX: &str = "tests/fixtures/rust-snapshot";
const INVOCATION: &str = "cargo run -p snapshot-tool --release --";

#[derive(Default)]
struct Args {
    check: bool,
    coverage_only: bool,
    diff: Option<String>,
    file: Option<String>,
    verify_dir: Option<PathBuf>,
}
impl Args {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self> {
        let mut parsed = Self::default();
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--check" => parsed.check = true,
                "--coverage-only" => parsed.coverage_only = true,
                "--diff" => parsed.diff = Some(args.next().ok_or("--diff requires REF")?),
                "--file" => parsed.file = Some(args.next().ok_or("--file requires a suffix")?),
                "--verify-dir" => {
                    parsed.verify_dir = Some(args.next().ok_or("--verify-dir requires DIR")?.into())
                }
                _ => return Err(format!("unknown argument: {arg}").into()),
            }
        }
        if parsed.check && parsed.diff.is_some() {
            return Err("--check and --diff are mutually exclusive".into());
        }
        if parsed.coverage_only && !parsed.check {
            return Err("--coverage-only requires --check".into());
        }
        if parsed.file.is_some() && parsed.diff.is_none() {
            return Err("--file requires --diff".into());
        }
        if parsed.verify_dir.is_some() && !(parsed.check && parsed.coverage_only) {
            return Err("--verify-dir requires --check --coverage-only".into());
        }
        Ok(parsed)
    }
}

fn output(root: &Path, args: &[&str]) -> Result<Output> {
    Ok(Command::new("git").args(args).current_dir(root).output()?)
}
fn git(root: &Path, args: &[&str]) -> Result<String> {
    let out = output(root, args)?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned().into());
    }
    Ok(String::from_utf8(out.stdout)?.trim_end_matches('\n').into())
}
fn clean(root: &Path) -> Result<()> {
    // Snapshot edits alone are allowed during intentional regeneration, as before.
    let status = git(root, &["status", "--porcelain", "--untracked-files=all"])?;
    if status.lines().any(|line| {
        !line
            .get(3..)
            .is_some_and(|s| s.starts_with(&format!("{PREFIX}/")))
    }) {
        return Err("commit source/input changes before regenerating the Rust snapshot".into());
    }
    Ok(())
}
fn digest(path: &Path) -> Result<String> {
    Ok(Sha256::digest(fs::read(path)?)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
fn files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            found.extend(files(&path)?);
        } else if path.is_file() {
            found.push(path);
        }
    }
    found.sort();
    Ok(found)
}
fn snapshot_digests(dir: &Path) -> Result<Digests> {
    files(dir)?
        .iter()
        .filter(|p| p.to_string_lossy().ends_with(".json.gz"))
        .map(|p| {
            Ok((
                p.strip_prefix(dir)?.to_string_lossy().replace('\\', "/"),
                digest(p)?,
            ))
        })
        .collect()
}
fn inputs(root: &Path, corpus: &Path) -> Result<Digests> {
    let mut found = Digests::new();
    for path in files(&root.join("tests/fixtures"))? {
        if !path.starts_with(root.join(PREFIX)) {
            found.insert(
                path.strip_prefix(root)?
                    .to_string_lossy()
                    .replace('\\', "/"),
                digest(&path)?,
            );
        }
    }
    let manifest: Value =
        serde_json::from_slice(&fs::read(root.join("tests/fixtures/corpus.json"))?)?;
    for row in manifest["files"]
        .as_array()
        .ok_or("corpus files must be an array")?
    {
        let name = row["file"].as_str().ok_or("missing corpus file")?;
        let value = digest(&corpus.join(name))?;
        if row["sha256"] != value {
            return Err(format!("corpus input changed: {name}").into());
        }
        found.insert(format!("corpus/{name}"), value);
    }
    Ok(found)
}

fn compare_inventory(generated: &Digests, committed: &Digests, coverage_only: bool) -> Result<()> {
    if !generated.contains_key("corpus.json.gz") || !generated.contains_key("pmi.json.gz") {
        return Err("snapshot replay did not cover corpus and PMI".into());
    }
    let missing: Vec<_> = committed
        .keys()
        .filter(|k| !generated.contains_key(*k))
        .collect();
    let added: Vec<_> = generated
        .keys()
        .filter(|k| !committed.contains_key(*k))
        .collect();
    let changed: Vec<_> = generated
        .iter()
        .filter_map(|(k, v)| committed.get(k).filter(|w| *w != v).map(|_| k))
        .collect();
    if !missing.is_empty() || !added.is_empty() || (!coverage_only && !changed.is_empty()) {
        return Err(format!(
            "Rust snapshot differs: missing={missing:?}, added={added:?}, changed={changed:?}"
        )
        .into());
    }
    Ok(())
}
fn check(
    root: &Path,
    generated: &Path,
    before_inputs: &Digests,
    coverage_only: bool,
) -> Result<()> {
    let committed = snapshot_digests(&root.join(PREFIX))?;
    let produced = snapshot_digests(generated)?;
    compare_inventory(&produced, &committed, coverage_only)?;
    let provenance: Value =
        serde_json::from_slice(&fs::read(root.join(PREFIX).join("provenance.json"))?)?;
    if provenance["inputs"] != json!(before_inputs) || provenance["snapshots"] != json!(committed) {
        return Err("snapshot provenance/input digests differ".into());
    }
    if coverage_only {
        println!(
            "HEAD checked all {} snapshots under their original comparison bands; no orphaned files",
            produced.len()
        );
    } else {
        println!(
            "HEAD reproduces all {} snapshots byte for byte",
            produced.len()
        );
    }
    Ok(())
}

struct Temporary(PathBuf);
impl Temporary {
    fn new() -> Result<Self> {
        let base = std::env::temp_dir();
        for n in 0..1000 {
            let path = base.join(format!("quiddity-rust-snapshot-{}-{n}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        }
        Err("could not create snapshot temporary directory".into())
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn decompress(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes).read_to_end(&mut out)?;
    Ok(out)
}
fn diff(root: &Path, reference: &str, only: Option<&str>) -> Result<()> {
    let old = git(
        root,
        &["ls-tree", "-r", "--name-only", reference, "--", PREFIX],
    )?;
    let mut names: BTreeSet<String> = old.lines().map(str::to_owned).collect();
    names.extend(
        files(&root.join(PREFIX))?
            .iter()
            .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().into_owned()),
    );
    let temporary = Temporary::new()?;
    for name in names {
        if !name.ends_with(".json.gz") || only.is_some_and(|suffix| !name.ends_with(suffix)) {
            continue;
        }
        let previous = output(root, &["show", &format!("{reference}:{name}")])?;
        let old = if previous.status.success() {
            decompress(&previous.stdout)?
        } else {
            Vec::new()
        };
        let path = root.join(&name);
        let new = if path.exists() {
            decompress(&fs::read(path)?)?
        } else {
            Vec::new()
        };
        let before = temporary.0.join("before");
        let after = temporary.0.join("after");
        fs::write(&before, old)?;
        fs::write(&after, new)?;
        // POSIX diff handles missing final newlines and bounded-memory unified diffs.
        let out = Command::new("diff")
            .arg("-u")
            .arg("--label")
            .arg(format!("{reference}/{name}"))
            .arg("--label")
            .arg(&name)
            .arg(before)
            .arg(after)
            .output()?;
        if !matches!(out.status.code(), Some(0 | 1)) {
            return Err(String::from_utf8_lossy(&out.stderr).into_owned().into());
        }
        print!("{}", String::from_utf8(out.stdout)?);
    }
    Ok(())
}
fn install(root: &Path, generated: &Path) -> Result<()> {
    let out = root.join(PREFIX);
    let backup = out.with_file_name(".rust-snapshot-previous");
    let staging = out.with_file_name(".rust-snapshot-next");
    if backup.exists() || staging.exists() {
        return Err(
            "resolve leftover .rust-snapshot-previous/.rust-snapshot-next before regenerating"
                .into(),
        );
    }
    fs::create_dir(&staging)?;
    for path in files(generated)? {
        let dest = staging.join(path.strip_prefix(generated)?);
        fs::create_dir_all(dest.parent().unwrap())?;
        fs::copy(path, dest)?;
    }
    if out.exists() {
        fs::rename(&out, &backup)?;
    }
    if let Err(e) = fs::rename(&staging, &out) {
        if backup.exists() && !out.exists() {
            fs::rename(&backup, &out)?;
        }
        return Err(e.into());
    }
    if backup.exists() {
        fs::remove_dir_all(backup)?;
    }
    Ok(())
}
fn run() -> Result<()> {
    if std::env::args().any(|s| s == "--help" || s == "-h") {
        println!(
            "{INVOCATION} [--check [--coverage-only [--verify-dir DIR]] | --diff REF [--file SUFFIX]]\n\nWithout flags, regenerate (forbidden in CI). --verify-dir checks already-passing shard output without replaying tests."
        );
        return Ok(());
    }
    let args = Args::parse(std::env::args().skip(1))?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    // A packaged binary still finds the checkout when run in CI or from another worktree.
    let root = PathBuf::from(git(root, &["rev-parse", "--show-toplevel"])?);
    if let Some(reference) = args.diff {
        return diff(&root, &reference, args.file.as_deref());
    }
    if !args.check && std::env::var_os("CI").is_some() {
        return Err("snapshot regeneration is forbidden in CI".into());
    }
    let corpus = std::env::var_os("QUIDDITY_CORPUS")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .ok_or("export QUIDDITY_CORPUS to the pinned corpus directory")?;
    clean(&root)?;
    let source = git(&root, &["rev-parse", "HEAD"])?;
    let before_inputs = inputs(&root, &corpus)?;
    if let Some(dir) = args.verify_dir {
        return check(&root, &dir, &before_inputs, true);
    }
    let temporary = Temporary::new()?;
    let generated = &temporary.0;
    let mut command = Command::new("cargo");
    command
        .current_dir(&root)
        .args([
            "test",
            "--workspace",
            "--release",
            "rust_snapshot",
            "--",
            "--nocapture",
        ])
        .env(
            "CARGO_BUILD_JOBS",
            std::env::var("CARGO_BUILD_JOBS").unwrap_or_else(|_| "6".into()),
        )
        .env(
            "RUST_TEST_THREADS",
            std::env::var("RUST_TEST_THREADS").unwrap_or_else(|_| "6".into()),
        )
        .env("QUIDDITY_CORPUS_REQUIRED", "1");
    for name in [
        "QUIDDITY_SNAPSHOT_DIR",
        "QUIDDITY_SNAPSHOT_REGENERATE",
        "QUIDDITY_SNAPSHOT_VERIFY_DIR",
    ] {
        command.env_remove(name);
    }
    if args.check {
        command.env("QUIDDITY_SNAPSHOT_VERIFY_DIR", generated);
    } else {
        command
            .env("QUIDDITY_SNAPSHOT_DIR", generated)
            .env("QUIDDITY_SNAPSHOT_REGENERATE", "1");
    }
    if !command.status()?.success() {
        return Err("Rust snapshot producers failed".into());
    }
    clean(&root)?;
    if inputs(&root, &corpus)? != before_inputs || git(&root, &["rev-parse", "HEAD"])? != source {
        return Err("source/inputs changed during generation".into());
    }
    if args.check {
        return check(&root, generated, &before_inputs, args.coverage_only);
    }
    let snapshots = snapshot_digests(generated)?;
    compare_inventory(&snapshots, &snapshots, false)?;
    let provenance = json!({"schema":1,"source_commit":source,"command":INVOCATION,
        "inputs":before_inputs,"snapshots":snapshots});
    fs::write(
        generated.join("provenance.json"),
        format!("{}\n", serde_json::to_string_pretty(&provenance)?),
    )?;
    install(&root, generated)?;
    println!(
        "Recorded Rust {source}: {} compressed snapshots",
        snapshots.len()
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_input_and_snapshot_keys_match_portable_provenance() {
        let root = Temporary::new().unwrap();
        let fixtures = root.0.join("tests/fixtures");
        fs::create_dir_all(fixtures.join("nested")).unwrap();
        fs::write(fixtures.join("nested/part.step"), b"STEP input").unwrap();
        fs::write(fixtures.join("corpus.json"), br#"{"files":[]}"#).unwrap();
        let found = inputs(&root.0, &root.0).unwrap();
        assert_eq!(found.len(), 2);
        assert!(found.contains_key("tests/fixtures/nested/part.step"));
        assert!(found.contains_key("tests/fixtures/corpus.json"));

        let out = root.0.join(PREFIX);
        fs::create_dir_all(out.join("nested")).unwrap();
        let snapshot = out.join("nested/part.json.gz");
        fs::write(&snapshot, b"expected").unwrap();
        assert_eq!(
            snapshot_digests(&out).unwrap(),
            Digests::from([("nested/part.json.gz".into(), digest(&snapshot).unwrap())])
        );
    }

    fn inventory() -> Digests {
        [
            ("corpus.json.gz", "a"),
            ("pmi.json.gz", "b"),
            ("kernel/part.json.gz", "c"),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect()
    }
    #[test]
    fn coverage_rejects_orphans_and_uncommitted_producers_but_allows_numeric_bands() {
        let committed = inventory();
        let mut generated = committed.clone();
        generated.insert("kernel/part.json.gz".into(), "roundoff".into());
        assert!(compare_inventory(&generated, &committed, true).is_ok());
        assert!(compare_inventory(&generated, &committed, false).is_err());
        generated.remove("kernel/part.json.gz");
        assert!(compare_inventory(&generated, &committed, true).is_err());
        generated = committed.clone();
        generated.insert("extra.json.gz".into(), "x".into());
        assert!(compare_inventory(&generated, &committed, true).is_err());
        for required in ["corpus.json.gz", "pmi.json.gz"] {
            let mut empty = committed.clone();
            empty.remove(required);
            assert!(compare_inventory(&empty, &empty, true).is_err());
        }
    }
    #[test]
    fn downloaded_coverage_checks_provenance_and_inputs_without_replay() {
        let root = Temporary::new().unwrap();
        let out = root.0.join(PREFIX);
        fs::create_dir_all(&out).unwrap();
        for name in ["corpus.json.gz", "pmi.json.gz"] {
            fs::write(out.join(name), b"committed").unwrap();
        }
        let verified = Temporary::new().unwrap();
        for name in ["corpus.json.gz", "pmi.json.gz"] {
            fs::write(verified.0.join(name), b"passing rounded values").unwrap();
        }
        let inputs = Digests::from([("fixture.step".into(), "sha256".into())]);
        let manifest = out.join("provenance.json");
        fs::write(
            &manifest,
            serde_json::to_vec(&json!({
                "inputs": inputs, "snapshots": snapshot_digests(&out).unwrap()
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(check(&root.0, &verified.0, &inputs, true).is_ok());
        assert!(check(&root.0, &verified.0, &Digests::new(), true).is_err());
        fs::write(out.join("pmi.json.gz"), b"edited expectation").unwrap();
        assert!(check(&root.0, &verified.0, &inputs, true).is_err());
    }
    #[test]
    fn invalid_cli_combinations_are_rejected() {
        for args in [
            vec!["--coverage-only"],
            vec!["--check", "--diff", "HEAD"],
            vec!["--file", "pmi"],
            vec!["--verify-dir", "/tmp"],
            vec!["--diff"],
        ] {
            assert!(Args::parse(args.into_iter().map(str::to_owned)).is_err());
        }
    }
}
