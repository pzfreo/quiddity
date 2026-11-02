//! `quiddity parts`, `quiddity pmi read`, `quiddity pmi check` and `quiddity pmi write`, and the
//! JSON form of the PMI model (`quiddity::pmi_json`), over the AP242 fixtures: the committed NIST
//! models, the specify-core-written inputs and the two-part assembly, plus every NIST AP242 file
//! when `HAECCEITY_NIST_PMI` names their directory (required when
//! `HAECCEITY_NIST_PMI_REQUIRED=1`), and a corpus part (`QUIDDITY_CORPUS`).

#[path = "common/public.rs"]
mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use quiddity::kernel::p21::Document;
use quiddity::kernel::pmi::write::differences_as_stated;
use quiddity::kernel::pmi::{self, PmiRead};
use quiddity::kernel::step::{PartDefinition, read_part_definitions};
use quiddity::pmi_json::{self, Binding};
use serde_json::{Value as Json, json};

fn quiddity(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quiddity"))
        .args(args)
        .output()
        .expect("quiddity runs")
}

fn ap242() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ap242")
}

fn scratch() -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("pmi_cli");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The committed AP242 inputs: NIST models, specify-core outputs, the assembly.
fn fixtures() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for (dir, suffix) in [("nist", ".stp.gz"), ("specify", ".step.gz")] {
        let mut names: Vec<PathBuf> = std::fs::read_dir(ap242().join(dir))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.to_string_lossy().ends_with(suffix))
            .collect();
        names.sort();
        out.extend(names);
    }
    out.push(ap242().join("assembly/assembly.step"));
    out
}

/// Every NIST AP242 file with a B-rep part, when `HAECCEITY_NIST_PMI` names their directory.
/// The tessellated-only FTC-08 edition (`-tg`) has no B-rep part to anchor PMI to: `parts` and
/// `pmi read` refuse it (checked here), never report it as a file without PMI.
fn nist_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for path in all_nist_files() {
        if name(&path).contains("-tg.") {
            let p = path.to_str().unwrap();
            for args in [&["parts", p][..], &["pmi", "read", p]] {
                let err = refused(args);
                assert!(err.contains("no solid or shell with faces"), "{err}");
            }
        } else {
            out.push(path);
        }
    }
    out
}

fn all_nist_files() -> Vec<PathBuf> {
    let Some(dir) = std::env::var_os("HAECCEITY_NIST_PMI").map(PathBuf::from) else {
        assert!(
            std::env::var_os("HAECCEITY_NIST_PMI_REQUIRED").is_none(),
            "HAECCEITY_NIST_PMI_REQUIRED is set but HAECCEITY_NIST_PMI is not"
        );
        return Vec::new();
    };
    let mut names: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| {
            let n = p.file_name().unwrap().to_string_lossy().to_lowercase();
            n.contains("ap242") && (n.ends_with(".stp") || n.ends_with(".step"))
        })
        .collect();
    names.sort();
    assert!(!names.is_empty(), "no AP242 files in {}", dir.display());
    names
}

fn bytes(path: &Path) -> Vec<u8> {
    let raw = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
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

fn read(path: &Path) -> (Vec<u8>, Vec<PartDefinition>, PmiRead) {
    let b = bytes(path);
    let defs = read_part_definitions(&b).expect("part definitions");
    let doc = Document::parse(b.clone()).expect("document");
    let r = pmi::read(&doc, &defs).expect("pmi");
    (b, defs, r)
}

fn name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

/// Runs a command that must succeed with empty stderr; its stdout as JSON and as bytes.
fn ok(args: &[&str]) -> (Json, Vec<u8>) {
    let out = quiddity(args);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{args:?}: {stderr}");
    assert!(stderr.is_empty(), "{args:?}: {stderr}");
    let json = serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    (json, out.stdout)
}

/// Runs a command that must be refused with exit code 1, nothing on stdout; its stderr.
fn refused(args: &[&str]) -> String {
    let out = quiddity(args);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(1), "{args:?}: {stderr}");
    assert!(out.stdout.is_empty(), "{args:?} printed a result");
    assert!(stderr.starts_with("quiddity: "), "{stderr}");
    stderr
}

#[test]
fn parts_lists_every_distinct_part_with_its_binding() {
    for path in fixtures() {
        let p = path.to_str().unwrap();
        let (json, _) = ok(&["parts", p]);
        let b = bytes(&path);
        let defs = read_part_definitions(&b).unwrap();
        assert_eq!(json["format"], "quiddity-parts", "{p}");
        assert_eq!(json["version"], pmi_json::VERSION, "{p}");
        assert_eq!(json["binding"]["sha256"], Binding::of(&b).sha256, "{p}");
        assert_eq!(json["binding"]["reader"], pmi_json::READER, "{p}");
        let parts = json["parts"].as_array().unwrap();
        assert_eq!(parts.len(), defs.len(), "{p}");
        for (i, (j, d)) in parts.iter().zip(&defs).enumerate() {
            assert_eq!(j["part"], i, "{p}");
            assert_eq!(j["name"], d.name.as_str(), "{p}");
            assert_eq!(j["faces"], d.faces.len(), "{p}");
            assert_eq!(j["edges"], d.edges.len(), "{p}");
            assert_eq!(
                j["placements"].as_array().unwrap().len(),
                d.placements.len(),
                "{p}"
            );
        }
    }
    // The assembly: a plate and a pin placed twice, one part with two placements. Its sha256
    // as `shasum -a 256` gives it.
    let (json, _) = ok(&[
        "parts",
        ap242().join("assembly/assembly.step").to_str().unwrap(),
    ]);
    assert_eq!(
        json["binding"]["sha256"],
        "ca5711668102f0e8bb9920469a46008e8a51d60e0bea7eb89a03089b0e464291"
    );
    let parts = json["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0]["name"], "plate");
    assert_eq!(parts[1]["name"], "pin");
    assert_eq!(parts[1]["placements"].as_array().unwrap().len(), 2);
    assert_eq!(parts[1]["placements"][0][0][3], -30.0);
}

#[test]
fn pmi_read_gives_every_part_and_finding_in_the_json_form() {
    for path in fixtures() {
        let p = path.to_str().unwrap();
        let (json, stdout) = ok(&["pmi", "read", p]);
        assert_eq!(json["format"], pmi_json::FORMAT, "{p}");
        assert_eq!(json["version"], pmi_json::VERSION, "{p}");
        let doc = pmi_json::document_from_json(&json).unwrap_or_else(|e| panic!("{p}: {e}"));
        let (b, defs, r) = read(&path);
        assert_eq!(doc.binding, Binding::of(&b), "{p}");
        assert_eq!(doc.parts.len(), r.parts.len(), "{p}");
        for (i, part) in doc.parts.iter().enumerate() {
            assert_eq!(part.part.0, i, "{p}");
            assert_eq!(part.name.as_deref(), Some(defs[i].name.as_str()), "{p}");
            assert_eq!(part.pmi, r.parts[i], "{p}: part {i}");
        }
        assert_eq!(doc.findings, r.findings, "{p}");
        // Deterministic: the same bytes on a second run (fresh hash seeds).
        assert_eq!(quiddity(&["pmi", "read", p]).stdout, stdout, "{p}");
    }
}

#[test]
fn pmi_read_of_one_part() {
    let path = ap242().join("specify/assembly_plate_pin.step.gz");
    let p = path.to_str().unwrap();
    let (all, _) = ok(&["pmi", "read", p]);
    let n = all["parts"].as_array().unwrap().len();
    assert!(n >= 2, "the assembly has two parts");
    for i in 0..n {
        let (one, _) = ok(&["pmi", "read", p, "--part", &i.to_string()]);
        let parts = one["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0], all["parts"][i]);
        for f in one["findings"].as_array().unwrap() {
            assert!(f.get("part").is_none_or(|q| *q == i), "{f}");
        }
    }
    let err = refused(&["pmi", "read", p, "--part", &n.to_string()]);
    assert!(err.contains(&format!("there is no part {n}")), "{err}");
    let out = quiddity(&["pmi", "read", p, "--part", "x"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage: quiddity"));
}

/// The reader version and the sha256 of `pmi read`'s output over the committed fixtures, pinned
/// together. `READER` is what `pmi check` holds a document's anchors to, so it must change
/// whenever the reader's output does: when this fails, bump `pmi_json::READER` if `pmi::read`
/// changed (a JSON-form change bumps `VERSION` instead), then re-pin both.
// Reader /15 retains explicitly authored standalone composition members without
// redundant aspects; old source semantics remain unchanged.
const READER_PIN: (&str, &str) = (
    "haecceity-pmi-read/15",
    "192fde06ad69cfd1675460e12fb63af6b63b77c0dfef183aba3d95ed99c8e6d6",
);

#[test]
fn reader_version_is_pinned_to_the_readers_output() {
    let mut all = Vec::new();
    for path in fixtures() {
        let (_, out) = ok(&["pmi", "read", path.to_str().unwrap()]);
        all.extend(name(&path).into_bytes());
        all.push(0);
        all.extend(out);
        all.push(0);
    }
    let digest = Binding::of(&all).sha256;
    assert_eq!(
        (pmi_json::READER, digest.as_str()),
        READER_PIN,
        "pmi read's output over the fixtures changed: bump READER if pmi::read changed, then \
         re-pin"
    );
}

#[test]
fn every_read_part_round_trips_through_json() {
    let mut files = fixtures();
    files.extend(nist_files());
    let mut parts = 0;
    for path in files {
        let (_, _, r) = read(&path);
        for (i, p) in r.parts.iter().enumerate() {
            let json = pmi_json::to_json(p);
            let text = serde_json::to_string(&json).unwrap();
            let back = pmi_json::from_json(&json, "pmi")
                .unwrap_or_else(|e| panic!("{}: part {i}: {e}", name(&path)));
            assert_eq!(&back, p, "{}: part {i}", name(&path));
            // The bytes are stable: the same text again, and from the decoded part.
            assert_eq!(serde_json::to_string(&pmi_json::to_json(p)).unwrap(), text);
            let again = serde_json::to_string(&pmi_json::to_json(&back)).unwrap();
            assert_eq!(again, text, "{}: part {i}", name(&path));
            // From the text itself, as another process would read it.
            let reparsed: Json = serde_json::from_str(&text).unwrap();
            assert_eq!(&pmi_json::from_json(&reparsed, "pmi").unwrap(), p);
            parts += 1;
        }
    }
    assert!(parts >= 16, "{parts} parts");
}

/// Every REAL-like token of a STEP text.
fn real_tokens(text: &str) -> BTreeSet<&str> {
    text.split(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'E' | 'e')))
        .filter(|t| t.contains('.'))
        .collect()
}

/// Every `{"value", "unit"}` object of a JSON tree.
fn values<'a>(j: &'a Json, out: &mut Vec<(&'a str, &'a Json)>) {
    match j {
        Json::Object(m) => {
            if let (Some(Json::String(v)), Some(u)) = (m.get("value"), m.get("unit")) {
                out.push((v, u));
            }
            m.values().for_each(|x| values(x, out));
        }
        Json::Array(a) => a.iter().for_each(|x| values(x, out)),
        _ => {}
    }
}

#[test]
fn inch_values_keep_their_text() {
    let mut files: Vec<PathBuf> = fixtures()
        .into_iter()
        .filter(|p| name(p).starts_with("nist_"))
        .collect();
    files.extend(nist_files());
    let mut inch_files = 0;
    for path in files {
        let p = path.to_str().unwrap();
        let (json, _) = ok(&["pmi", "read", p]);
        let b = bytes(&path);
        let text = String::from_utf8_lossy(&b);
        let tokens = real_tokens(&text);
        let mut vs = Vec::new();
        values(&json["parts"], &mut vs);
        let inch: Vec<&str> = vs
            .iter()
            .filter(|(_, u)| *u == "in")
            .map(|(v, _)| *v)
            .collect();
        if inch.is_empty() {
            continue;
        }
        inch_files += 1;
        for v in inch {
            assert!(
                tokens.contains(v),
                "{}: inch value {v:?} is not a REAL token of the file",
                name(&path)
            );
        }
    }
    // ftc_07, stc_06 and stc_09 among the committed files.
    assert!(inch_files >= 3, "{inch_files} inch files");
}

/// `pmi read` of spool_fits (one part: datums, fits, tolerances, material, general tolerance)
/// as JSON, and the STEP path.
fn spool() -> (Json, String) {
    let path = ap242().join("specify/spool_fits.step.gz");
    let p = path.to_str().unwrap().to_string();
    let (json, _) = ok(&["pmi", "read", &p]);
    (json, p)
}

/// `pmi check` of `doc` against `step`.
fn check(step: &str, doc: &str, label: &str) -> Output {
    let file = scratch().join(format!("{label}.json"));
    std::fs::write(&file, doc).unwrap();
    quiddity(&["pmi", "check", step, file.to_str().unwrap()])
}

fn check_refused(step: &str, doc: &Json, label: &str) -> String {
    let out = check(step, &doc.to_string(), label);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(1), "{label}: {stderr}");
    assert!(out.stdout.is_empty(), "{label}");
    stderr
}

#[test]
fn pmi_check_accepts_what_pmi_read_writes_and_prints_it_canonically() {
    for path in fixtures() {
        let p = path.to_str().unwrap();
        let (_, read_out) = ok(&["pmi", "read", p]);
        let out = check(p, &String::from_utf8_lossy(&read_out), &name(&path));
        assert_eq!(
            out.status.code(),
            Some(0),
            "{p}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, read_out, "{p}");
    }
}

#[test]
fn pmi_check_refusals_name_the_json_path() {
    let (json, step) = spool();
    let pmi = &json["parts"][0]["pmi"];
    assert!(pmi["dimensions"].as_array().unwrap().len() >= 4);

    // Not JSON.
    let out = check(&step, "{\"format\": ", "invalid");
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not JSON"));

    let mut bad = json.clone();
    bad["parts"][0]["pmi"]["tolerances"][0]["kind"] = json!("runout");
    let err = check_refused(&step, &bad, "unknown-kind");
    assert!(
        err.contains("parts[0].pmi.tolerances[0].kind: unknown term \"runout\""),
        "{err}"
    );

    // A misspelt standard term is refused, never kept as a free-text name; a name the practice
    // does not list must be written {"other": …}, and an `other` naming a standard term is refused.
    let size_kind = |kind: Json| {
        let mut d = json.clone();
        d["parts"][0]["pmi"]["dimensions"][0]["kind"]["size"]["kind"] = kind;
        d
    };
    let err = check_refused(&step, &size_kind(json!("Diamter")), "misspelt-size-kind");
    assert!(
        err.contains("parts[0].pmi.dimensions[0].kind.size.kind: unknown term \"Diamter\""),
        "{err}"
    );
    let err = check_refused(
        &step,
        &size_kind(json!({"other": "diameter"})),
        "other-standard-size-kind",
    );
    assert!(
        err.contains(
            "parts[0].pmi.dimensions[0].kind.size.kind.other: \"diameter\" is a standard term"
        ),
        "{err}"
    );
    let out = check(
        &step,
        &size_kind(json!({"other": "chord"})).to_string(),
        "other-size-kind",
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let mut bad = json.clone();
    bad["parts"][0]["pmi"]["dimensions"][0]["kind"] =
        json!({"location": {"from": 2, "to": 3, "kind": "linear distanse", "directed": false}});
    let err = check_refused(&step, &bad, "misspelt-location-kind");
    assert!(
        err.contains(
            "parts[0].pmi.dimensions[0].kind.location.kind: unknown term \"linear distanse\""
        ),
        "{err}"
    );

    let mut bad = json.clone();
    bad["parts"][0]["pmi"]["tolerances"][0]["zone"]["form"] = json!("within a cylindre");
    let err = check_refused(&step, &bad, "misspelt-zone-form");
    assert!(
        err.contains("parts[0].pmi.tolerances[0].zone.form: unknown term \"within a cylindre\""),
        "{err}"
    );

    let mut bad = json.clone();
    bad["parts"][0]["pmi"]["dimensions"][0]["qualifier"] = json!("maximun");
    let err = check_refused(&step, &bad, "misspelt-qualifier");
    assert!(
        err.contains("parts[0].pmi.dimensions[0].qualifier: unknown term \"maximun\""),
        "{err}"
    );

    // A key named twice is refused, not resolved to its last value.
    let text = serde_json::to_string(&json).unwrap();
    let dup = text.replacen("{\"binding\":", "{\"format\":\"x\",\"binding\":", 1);
    assert_ne!(dup, text);
    let out = check(&step, &dup, "duplicate-key");
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("duplicate key \"format\""), "{err}");

    let mut bad = json.clone();
    bad["parts"][0]["pmi"]["tolerances"][0]["modifiers"] = json!(["MAXIMUM_MATERIAL_REQUIREMENT"]);
    let err = check_refused(&step, &bad, "upper-case-modifier");
    assert!(
        err.contains("parts[0].pmi.tolerances[0].modifiers[0]: unknown term"),
        "{err}"
    );

    let mut bad = json.clone();
    bad["parts"][0]["pmi"]["dimensions"][0]["tolerance"] =
        json!({"fit": {"deviation": "Hx", "grade": "IT7"}});
    let err = check_refused(&step, &bad, "unknown-deviation");
    assert!(
        err.contains("parts[0].pmi.dimensions[0].tolerance.fit.deviation"),
        "{err}"
    );

    let mut bad = json.clone();
    bad["parts"][0]["pmi"]["datums"][0]["colour"] = json!("red");
    let err = check_refused(&step, &bad, "unknown-field");
    assert!(
        err.contains("parts[0].pmi.datums[0].colour: unknown field"),
        "{err}"
    );

    let mut bad = json.clone();
    bad["extra"] = json!(1);
    let err = check_refused(&step, &bad, "unknown-top-field");
    assert!(err.contains("extra: unknown field"), "{err}");

    let mut bad = json.clone();
    bad["parts"][0]["pmi"]["features"][0] = json!({"items": [{"face": 100000}]});
    let err = check_refused(&step, &bad, "face-out-of-range");
    assert!(
        err.contains("parts[0].pmi.features[0].items[0].face: part 0 has"),
        "{err}"
    );
    assert!(err.contains("there is no face 100000"), "{err}");

    // Invariants: deviations with upper below lower; a datum label twice; a value as a number;
    // a reference to a missing feature; a flatness with datums.
    let mut bad = json.clone();
    bad["parts"][0]["pmi"]["dimensions"][2]["tolerance"] = json!({"deviations": {
        "upper": {"value": "-0.039", "unit": "mm"},
        "lower": {"value": "-0.014", "unit": "mm"}}});
    let err = check_refused(&step, &bad, "upper-below-lower");
    assert!(
        err.contains("parts[0].pmi.dimensions[2].tolerance.deviations: upper bound"),
        "{err}"
    );

    let mut bad = json.clone();
    bad["parts"][0]["pmi"]["datums"][1]["label"] = json!("A");
    let err = check_refused(&step, &bad, "label-twice");
    assert!(
        err.contains("parts[0].pmi: ") && err.contains("datum label A is used twice"),
        "{err}"
    );

    let mut bad = json.clone();
    bad["parts"][0]["pmi"]["dimensions"][0]["nominal"]["value"] = json!(62.0);
    let err = check_refused(&step, &bad, "float-value");
    assert!(
        err.contains("parts[0].pmi.dimensions[0].nominal.value: expected a string"),
        "{err}"
    );

    let mut bad = json.clone();
    bad["parts"][0]["pmi"]["dimensions"][0]["nominal"]["value"] = json!("6,2");
    let err = check_refused(&step, &bad, "not-decimal");
    assert!(err.contains("is not a decimal"), "{err}");

    let mut bad = json.clone();
    bad["parts"][0]["pmi"]["datums"][0]["feature"] = json!(999);
    let err = check_refused(&step, &bad, "missing-feature");
    assert!(err.contains("names missing feature 999"), "{err}");

    let mut bad = json.clone();
    let flat = bad["parts"][0]["pmi"]["tolerances"]
        .as_array()
        .unwrap()
        .iter()
        .position(|t| t["kind"] == "flatness")
        .unwrap();
    bad["parts"][0]["pmi"]["tolerances"][flat]["datums"] = json!([{"references": [{"datum": 0}]}]);
    let err = check_refused(&step, &bad, "flatness-datums");
    assert!(err.contains("takes no datums"), "{err}");

    let mut bad = json.clone();
    bad["parts"][0]["pmi"]["tolerances"][0]["datums"] = json!([]);
    let err = check_refused(&step, &bad, "empty-system");
    assert!(
        err.contains("parts[0].pmi.tolerances[0].datums: a datum system has 0"),
        "{err}"
    );

    // A length where an angle belongs, and an angle unit on a tolerance magnitude.
    let mut bad = json.clone();
    bad["parts"][0]["pmi"]["tolerances"][0]["magnitude"]["unit"] = json!("deg");
    let err = check_refused(&step, &bad, "angle-magnitude");
    assert!(
        err.contains("parts[0].pmi.tolerances[0].magnitude.unit: expected a length"),
        "{err}"
    );

    // The binding: another file, another reader, another format version.
    let mut bad = json.clone();
    bad["binding"]["sha256"] = json!("0".repeat(64));
    let err = check_refused(&step, &bad, "sha256");
    assert!(err.contains("binding.sha256"), "{err}");
    let mut bad = json.clone();
    bad["binding"]["reader"] = json!("haecceity-pmi-read/0");
    let err = check_refused(&step, &bad, "reader");
    assert!(err.contains("binding.reader"), "{err}");
    let mut bad = json.clone();
    bad["version"] = json!(999);
    let err = check_refused(&step, &bad, "version");
    assert!(
        err.contains("version: version 999 is not supported"),
        "{err}"
    );
    let mut bad = json.clone();
    bad.as_object_mut().unwrap().remove("version");
    let err = check_refused(&step, &bad, "no-version");
    assert!(err.contains("version: missing"), "{err}");

    // A part the file does not have, a part listed twice, a wrong name.
    let mut bad = json.clone();
    bad["parts"][0]["part"] = json!(5);
    let err = check_refused(&step, &bad, "no-part");
    assert!(err.contains("parts[0].part: the file has 1 parts"), "{err}");
    let mut bad = json.clone();
    let p0 = bad["parts"][0].clone();
    bad["parts"].as_array_mut().unwrap().push(p0);
    let err = check_refused(&step, &bad, "part-twice");
    assert!(
        err.contains("parts[1].part: part 0 is listed twice"),
        "{err}"
    );
    let mut bad = json.clone();
    bad["parts"][0]["name"] = json!("not the part");
    let err = check_refused(&step, &bad, "name");
    assert!(err.contains("parts[0].name"), "{err}");
}

#[test]
fn hand_written_pmi_is_accepted_in_the_documented_form() {
    // What a consumer such as specify-core writes: datums A and B, Ø62 H7, a g6 shaft with
    // deviations wholly below nominal, a position with Ⓜ to A|B, a flatness, a general
    // tolerance; optional fields left out.
    let (json, step) = spool();
    let doc = json!({
        "format": "quiddity-pmi",
        "version": pmi_json::VERSION,
        "binding": json["binding"],
        "parts": [{"part": 0, "pmi": {
            "features": [
                {"items": [{"face": 69}]},
                {"items": [{"face": 55}]},
                {"items": [{"face": 0}, {"face": 16}]},
            ],
            "datums": [{"label": "A", "feature": 0}, {"label": "B", "feature": 1}],
            "dimensions": [
                {"kind": {"size": {"feature": 1, "kind": "diameter"}},
                 "nominal": {"value": "62", "unit": "mm"},
                 "tolerance": {"fit": {"deviation": "H", "grade": "IT7"}}},
                {"kind": {"size": {"feature": 2, "kind": "diameter"}},
                 "nominal": {"value": "130", "unit": "mm"},
                 "tolerance": {"deviations": {"upper": {"value": "-0.014", "unit": "mm"},
                                              "lower": {"value": "-0.039", "unit": "mm"}}}},
            ],
            "tolerances": [
                {"kind": "position", "target": {"feature": 2},
                 "magnitude": {"value": "0.2", "unit": "mm"},
                 "zone": {"form": "cylindrical or circular"},
                 "modifiers": ["maximum_material_requirement"],
                 "datums": [{"references": [{"datum": 0}]}, {"references": [{"datum": 1}]}]},
                {"kind": "flatness", "target": {"feature": 0},
                 "magnitude": {"value": "0.02", "unit": "mm"}},
            ],
            "general": [{"class": {"text": "ISO 2768-mK"}}],
            "material": {"id": "Aluminium 6082-T6"},
        }}],
    });
    let out = check(&step, &doc.to_string(), "hand-written");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let back: Json = serde_json::from_slice(&out.stdout).unwrap();
    let p = &back["parts"][0]["pmi"];
    // The text is kept as given; the ISO 2768 classes are recognised from it.
    assert_eq!(p["dimensions"][0]["nominal"]["value"], "62");
    assert_eq!(
        p["general"][0],
        json!({"class": {"text": "ISO 2768-mK", "standard": {"linear": "m", "geometric": "K"}}})
    );
    // A stated standard that disagrees with the text is refused.
    let mut bad = doc.clone();
    bad["parts"][0]["pmi"]["general"][0]["class"]["standard"] = json!({"linear": "f"});
    let err = check_refused(&step, &bad, "standard-disagrees");
    assert!(
        err.contains("parts[0].pmi.general[0].class.standard: does not agree"),
        "{err}"
    );
}

// ---------------------------------------------------------------------------------------------
// pmi write
// ---------------------------------------------------------------------------------------------

/// `pmi write <step> <label.json> -o <label.out.step> <extra>` with `doc` as the JSON; the
/// output's path (removed first) and the process's output.
fn write_cli(step: &str, doc: &Json, label: &str, extra: &[&str]) -> (PathBuf, Output) {
    let json = scratch().join(format!("{label}.json"));
    std::fs::write(&json, doc.to_string()).unwrap();
    let out = scratch().join(format!("{label}.out.step"));
    let _ = std::fs::remove_file(&out);
    let mut args = vec![
        "pmi",
        "write",
        step,
        json.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ];
    args.extend(extra);
    let output = quiddity(&args);
    (out, output)
}

/// A write that must be refused (exit 1, nothing on stdout, the output not created and no
/// temporary file left beside it); its stderr.
fn write_refused(out: &Path, output: &Output, label: &str) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert_eq!(output.status.code(), Some(1), "{label}: {stderr}");
    assert!(output.stdout.is_empty(), "{label} printed a result");
    assert!(stderr.starts_with("quiddity: "), "{label}: {stderr}");
    assert!(!out.exists(), "{label}: the output was created");
    no_temporary_left(out, label);
    stderr
}

/// No `.<name>.<pid>.tmp` beside `out`.
fn no_temporary_left(out: &Path, label: &str) {
    let name = out.file_name().unwrap().to_string_lossy().into_owned();
    for e in std::fs::read_dir(out.parent().unwrap()).unwrap() {
        let n = e.unwrap().file_name().to_string_lossy().into_owned();
        assert!(
            !(n.starts_with(&format!(".{name}.")) && n.ends_with(".tmp")),
            "{label}: temporary {n} left"
        );
    }
}

/// A write that must succeed with empty stderr: its report and stdout.
fn write_ok(output: &Output, label: &str) -> (Json, Vec<u8>) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "{label}: {stderr}");
    assert!(stderr.is_empty(), "{label}: {stderr}");
    let report: Json = serde_json::from_slice(&output.stdout).expect("the report is JSON");
    assert_eq!(report["format"], pmi_json::WRITE_FORMAT, "{label}");
    (report, output.stdout.clone())
}

/// The refusals a `write refused: not written: part P Item(i): why; part …` message lists:
/// `(part, item kind, index, reason)`.
fn refusals(stderr: &str) -> Vec<(usize, String, Option<usize>, String)> {
    let Some((_, list)) = stderr.split_once("write refused: not written: ") else {
        return Vec::new();
    };
    list.trim_end()
        .split("; part ")
        .map(|seg| {
            let seg = seg.strip_prefix("part ").unwrap_or(seg);
            let (part, rest) = seg.split_once(' ').unwrap();
            let (item, why) = rest.split_once(": ").unwrap();
            let (kind, index) = match item.split_once('(') {
                Some((k, i)) => (k, Some(i.trim_end_matches(')').parse().unwrap())),
                None => (item, None),
            };
            (
                part.parse().unwrap(),
                kind.to_string(),
                index,
                why.to_string(),
            )
        })
        .collect()
}

/// A refusal's pin key, as `references/pmi_write.json` "roundtrip" "refused" writes it (the
/// writer's `pmi_roundtrip.rs`): kind and reason, digits as `N`, a datum's label as `…`.
fn refusal_key(kind: &str, why: &str) -> String {
    let why = match why.strip_prefix("datum ").and_then(|r| r.split_once(' ')) {
        Some((_, rest)) if kind == "Datum" => format!("datum … {rest}"),
        _ => why.to_string(),
    };
    let why: String = why
        .chars()
        .map(|c| if c.is_ascii_digit() { 'N' } else { c })
        .collect();
    format!("{kind}: {why}")
}

/// The items no other item references (notes, tolerance relations, attribute sets, hole
/// definitions), by their refusal kind and JSON list: a document without them keeps every other
/// reference valid.
const LEAF_ITEMS: [(&str, &str); 4] = [
    ("Note", "notes"),
    ("ToleranceRelation", "tolerance_relations"),
    ("Attribute", "attributes"),
    ("Hole", "holes"),
];

/// `doc` without the refused leaf items, or `None` when a refused item is not a leaf.
fn without_refused_leaves(
    doc: &Json,
    refused: &[(usize, String, Option<usize>, String)],
) -> Option<Json> {
    let mut doc = doc.clone();
    let mut gone: BTreeMap<(usize, &str), BTreeSet<usize>> = BTreeMap::new();
    for (part, kind, index, _) in refused {
        let (_, list) = LEAF_ITEMS.iter().find(|(k, _)| k == kind)?;
        gone.entry((*part, list)).or_default().insert((*index)?);
    }
    for ((part, list), indices) in gone {
        let entry = doc["parts"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|p| p["part"] == part)
            .unwrap();
        let pmi = entry["pmi"].as_object_mut().unwrap();
        let kept: Vec<Json> = pmi[list]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .filter(|(i, _)| !indices.contains(i))
            .map(|(_, x)| x.clone())
            .collect();
        if kept.is_empty() {
            pmi.remove(list);
        } else {
            pmi.insert(list.to_string(), Json::Array(kept));
        }
    }
    Some(doc)
}

/// The stem `references/pmi_write.json` names a file by.
fn stem(path: &Path) -> String {
    name(path)
        .trim_end_matches(".gz")
        .trim_end_matches(".step")
        .trim_end_matches(".stp")
        .to_string()
}

/// Each written part of `doc` against the parts read back (`back`, a `pmi read` document), as
/// stated: values' text and units included. The problems.
fn read_back_differences(doc: &Json, back: &Json, label: &str) -> Vec<String> {
    let written = pmi_json::document_from_json(doc).unwrap();
    let back = pmi_json::document_from_json(back).unwrap();
    let mut out = Vec::new();
    for p in &written.parts {
        let read = &back.parts[p.part.0];
        assert_eq!(read.part, p.part);
        for d in differences_as_stated(&pmi::write::standard_form(&p.pmi), &read.pmi) {
            out.push(format!("{label} part {}: {d}", p.part.0));
        }
    }
    out
}

/// read → write (replace, presentation removed) → read through the command line, for every
/// fixture and NIST file, against the writer's own round trip pins (`references/pmi_write.json`
/// "roundtrip", of the same file by sha256):
///
/// - The JSON `pmi read` printed is written back as it is. Where the writer refuses items, the
///   write is refused (exit 1, nothing created), naming exactly the pinned refusals. Where those
///   are all items no other item references (notes, tolerance relations, attribute sets,
///   holes), they are left out of the JSON and the write repeated.
/// - A write the pin records as refused by the removal plan is refused so.
/// - Otherwise the output reads back as the JSON written (by meaning, every value's text and
///   unit as stated), with the pinned number of presentation instances removed and the pinned
///   edition note; the read-back JSON written again reads back as the same JSON, byte for byte
///   (a fixed point: the first round trip only merges features of equal items); and with
///   `--presentation refuse` the write is refused naming the pinned number of presentation
///   blockers, or written where the pin says so.
#[test]
fn pmi_write_replace_round_trips_through_the_command_line() {
    let pins: Json = serde_json::from_str(
        &std::fs::read_to_string(common::fixtures().join("references/pmi_write.json")).unwrap(),
    )
    .unwrap();
    let mut files = fixtures();
    let nist = nist_files();
    let with_nist = !nist.is_empty();
    // NIST's files that are not among the committed ones (by stem).
    let committed: BTreeSet<String> = files.iter().map(|p| stem(p)).collect();
    files.extend(nist.into_iter().filter(|p| !committed.contains(&stem(p))));
    let results = common::parallel::map(&files, |path| roundtrip_cli(path, &pins["roundtrip"]));
    let mut problems = Vec::new();
    let mut written = 0;
    for (p, w) in results {
        problems.extend(p);
        written += usize::from(w);
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    // The assembly, six of the seven specify-core inputs (nist_ctc_01_merge has dimensions
    // without a nominal value) and the committed NIST models whose refusals are leaves (ftc_07,
    // stc_06, stc_09, stc_10), plus CTC-02/FTC-10 now their 13 targets are written
    // under the 2026-10-20 practice exemption; of NIST's set, also ftc_08 and stc_07.
    assert_eq!(written, if with_nist { 15 } else { 13 }, "files written");
}

/// One file of [`pmi_write_replace_round_trips_through_the_command_line`]: its problems, and
/// whether it was written.
fn roundtrip_cli(path: &Path, pins: &Json) -> (Vec<String>, bool) {
    let mut problems = Vec::new();
    let stem = stem(path);
    let p = path.to_str().unwrap();
    let Some(pin) = pins.get(&stem) else {
        return (vec![format!("{stem}: not pinned")], false);
    };
    let (doc, _) = ok(&["pmi", "read", p]);
    if pin["sha256"] != doc["binding"]["sha256"] {
        return (
            vec![format!("{stem}: the pin is of another file of this name")],
            false,
        );
    }
    let replace = ["--mode", "replace", "--presentation", "remove"];
    let (out, output) = write_cli(p, &doc, &format!("rt-{stem}"), &replace);
    let mut doc = doc;
    let mut output = output;
    if let Some(refused) = pin.get("refused").and_then(|r| r.as_object()) {
        let stderr = write_refused(&out, &output, &stem);
        let listed = refusals(&stderr);
        let mut keys: BTreeMap<String, u64> = BTreeMap::new();
        for (_, kind, _, why) in &listed {
            *keys.entry(refusal_key(kind, why)).or_insert(0) += 1;
        }
        let pinned: BTreeMap<String, u64> = refused
            .iter()
            .map(|(k, v)| (k.clone(), v.as_u64().unwrap()))
            .collect();
        if keys != pinned {
            problems.push(format!(
                "{stem}: refusals {keys:?}, pinned {pinned:?}\n  {stderr}"
            ));
            return (problems, false);
        }
        let Some(stripped) = without_refused_leaves(&doc, &listed) else {
            return (problems, false);
        };
        doc = stripped;
        output = write_cli(p, &doc, &format!("rt-{stem}"), &replace).1;
    }
    if let Some(e) = pin.get("error").and_then(|e| e.as_str()) {
        let stderr = write_refused(&out, &output, &stem);
        if !e.starts_with("removal refused") || !stderr.contains("removal refused") {
            problems.push(format!("{stem}: pinned {e:?}, refused: {stderr}"));
        }
        return (problems, false);
    }
    if output.status.code() != Some(0) {
        problems.push(format!(
            "{stem}: not written: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
        return (problems, false);
    }
    let (report, _) = write_ok(&output, &stem);
    let note_count: usize = doc["parts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["pmi"]["notes"].as_array().map_or(0, Vec::len))
        .sum();
    assert_eq!(
        report["note_migrations"].as_array().unwrap().len(),
        note_count,
        "{stem}: each legacy note migration is reported"
    );
    if report["presentation_removed"].as_array().unwrap().len() as u64
        != pin["presentation removed"].as_u64().unwrap()
    {
        problems.push(format!(
            "{stem}: {} presentation instances removed, pinned {}",
            report["presentation_removed"].as_array().unwrap().len(),
            pin["presentation removed"]
        ));
    }
    if report.get("edition_undetermined").is_some() != pin.get("edition").is_some() {
        problems.push(format!("{stem}: the edition note is not the pinned one"));
    }
    let o = out.to_str().unwrap();
    let (back, _) = ok(&["pmi", "read", o]);
    if report["binding"] != back["binding"] {
        problems.push(format!("{stem}: the report's binding is not the output's"));
    }
    problems.extend(read_back_differences(&doc, &back, &stem));

    // A fixed point: the read-back JSON written again reads back as itself.
    let (out2, output2) = write_cli(o, &back, &format!("rt2-{stem}"), &replace);
    write_ok(&output2, &format!("{stem} again"));
    let (back2, _) = ok(&["pmi", "read", out2.to_str().unwrap()]);
    if back2["parts"] != back["parts"] {
        problems.push(format!("{stem}: the second round trip changed the JSON"));
    }

    // Policy Refuse: refused naming the presentation, or written, as pinned.
    let refuse = ["--mode", "replace", "--presentation", "refuse"];
    let (out3, output3) = write_cli(p, &doc, &format!("rt3-{stem}"), &refuse);
    match pin["refuse policy"].as_str().unwrap() {
        "written" => {
            write_ok(&output3, &format!("{stem} refuse"));
        }
        pinned => {
            let stderr = write_refused(&out3, &output3, &format!("{stem} refuse"));
            let presentation = stderr
                .split("; #")
                .filter(|b| b.contains(" (presentation) ") || b.contains(" (validation-property) "))
                .count();
            let n: usize = pinned
                .strip_prefix("refused: ")
                .and_then(|r| r.split(' ').next())
                .and_then(|n| n.parse().ok())
                .unwrap();
            if !stderr.contains("removal refused (Refuse)") || presentation != n {
                problems.push(format!(
                    "{stem}: policy refuse: {presentation} presentation blockers named, pinned \
                     {pinned:?}"
                ));
            }
        }
    }
    (problems, true)
}

/// spool_fits' complete reading, including legacy notes to migrate, and the STEP path.
fn spool_writable() -> (Json, String) {
    spool()
}

#[test]
fn shared_carrier_exemptions_use_structural_report_conditions() {
    for (case, condition, count) in [
        ("pmi_gaps", "shared-carrier-controlled-pair", 2),
        (
            "datum_targets_shared_support",
            "shared-carrier-datum-target-support",
            5,
        ),
    ] {
        let source = ap242().join(format!("write-presentation/{case}.step.gz"));
        let step = source.to_str().unwrap();
        let (doc, _) = ok(&["pmi", "read", step]);
        let (_, output) = write_cli(step, &doc, case, &["--mode", "replace"]);
        let (report, _) = write_ok(&output, case);
        let exemptions = report["geometry_carrier_exemptions"].as_array().unwrap();
        assert_eq!(exemptions.len(), count, "{case}");
        for exemption in exemptions {
            assert_eq!(exemption["condition"], condition, "{case}");
            assert_eq!(exemption["part"], 0, "{case}");
            assert_ne!(exemption["aspect"], exemption["feature"], "{case}");
        }
    }
}

/// The same write twice gives the same bytes and the same report; a `.gz` output is the same
/// text gzipped; the inch NIST fixtures' values keep their text through read → replace → read
/// (every inch value read back is a REAL token of the written file and was in the JSON written).
#[test]
fn pmi_write_is_deterministic_and_keeps_inch_values_as_stated() {
    let (doc, step) = spool_writable();
    let args = ["--mode", "replace"];
    let (out, o1) = write_cli(&step, &doc, "det", &args);
    let (_, s1) = write_ok(&o1, "det");
    let b1 = std::fs::read(&out).unwrap();
    let (out, o2) = write_cli(&step, &doc, "det", &args);
    let (_, s2) = write_ok(&o2, "det again");
    assert_eq!(std::fs::read(&out).unwrap(), b1, "the same bytes");
    assert_eq!(s1, s2, "the same report");
    let gz = scratch().join("det.out.step.gz");
    let json = scratch().join("det.json");
    let output = quiddity(&[
        "pmi",
        "write",
        &step,
        json.to_str().unwrap(),
        "-o",
        gz.to_str().unwrap(),
        "--mode",
        "replace",
    ]);
    write_ok(&output, "gz");
    assert_eq!(bytes(&gz), b1, "the gzipped output is the same text");

    let mut inch_files = 0;
    for path in fixtures() {
        let n = name(&path);
        let leaves_only = ["ftc_07", "stc_06", "stc_09"].iter().any(|s| n.contains(s));
        if !leaves_only {
            continue;
        }
        let p = path.to_str().unwrap();
        let (mut doc, _) = ok(&["pmi", "read", p]);
        for part in doc["parts"].as_array_mut().unwrap() {
            let pmi = part["pmi"].as_object_mut().unwrap();
            pmi.remove("attributes");
            // The writer refuses hole definitions (docs/step-ap242.md, Q-holes).
            pmi.remove("holes");
        }
        let (out, output) = write_cli(p, &doc, &format!("inch-{n}"), &args);
        write_ok(&output, &n);
        let (back, _) = ok(&["pmi", "read", out.to_str().unwrap()]);
        let text = String::from_utf8_lossy(&bytes(&out)).into_owned();
        let tokens = real_tokens(&text);
        let (mut was, mut now) = (Vec::new(), Vec::new());
        values(&doc["parts"], &mut was);
        values(&back["parts"], &mut now);
        let inch = |v: &[(&str, &Json)]| -> BTreeSet<String> {
            v.iter()
                .filter(|(_, u)| *u == "in")
                .map(|(v, _)| v.to_string())
                .collect()
        };
        let (was, now) = (inch(&was), inch(&now));
        assert!(!now.is_empty(), "{n}: inch values");
        assert_eq!(was, now, "{n}: the inch values' texts");
        for v in &now {
            assert!(tokens.contains(v.as_str()), "{n}: {v:?} is not in the file");
        }
        inch_files += 1;
    }
    assert_eq!(inch_files, 3);
}

/// Every refusal names its cause on stderr, exits 1, and creates nothing: a document bound to
/// another file or reader version, a remove naming PMI to write, presentation in the way under
/// `--presentation refuse` or invalid parameters; an existing file at the destination is left as it was.
#[test]
fn pmi_write_refusals_name_the_cause_and_create_nothing() {
    let (doc, step) = spool_writable();
    let mut other = doc.clone();
    other["binding"]["sha256"] = json!("0".repeat(64));
    let (out, o) = write_cli(&step, &other, "another-file", &[]);
    let err = write_refused(&out, &o, "another file");
    assert!(err.contains("binding.sha256"), "{err}");
    assert!(err.contains("numbered for another file"), "{err}");

    let mut other = doc.clone();
    other["binding"]["reader"] = json!("haecceity-pmi-read/0");
    let (out, o) = write_cli(&step, &other, "another-reader", &["--mode", "replace"]);
    let err = write_refused(&out, &o, "another reader");
    assert!(err.contains("binding.reader"), "{err}");

    let (out, o) = write_cli(&step, &doc, "remove-with-pmi", &["--mode", "remove"]);
    let err = write_refused(&out, &o, "remove with PMI");
    assert!(
        err.contains("remove names part 0 with PMI to write"),
        "{err}"
    );

    let refuse = ["--mode", "replace", "--presentation", "refuse"];
    let (out, o) = write_cli(&step, &doc, "presentation", &refuse);
    let err = write_refused(&out, &o, "presentation");
    assert!(err.contains("removal refused (Refuse)"), "{err}");
    assert!(
        err.contains("draughting_model_item_association (presentation) references removed"),
        "{err}"
    );

    let mut integer = doc.clone();
    integer["parts"][0]["pmi"]["dimensions"][0]["nominal"]["value"] = json!("62");
    let (out, result) = write_cli(&step, &integer, "integer-real", &["--mode", "replace"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let (reread, _) = ok(&["pmi", "read", out.to_str().unwrap()]);
    assert_eq!(
        reread["parts"][0]["pmi"]["dimensions"][0]["nominal"]["value"],
        "62."
    );

    let ctc02 = ap242().join("nist/nist_ctc_02_asme1_ap242-e2.stp.gz");
    let p = ctc02.to_str().unwrap();
    let (nist, _) = ok(&["pmi", "read", p]);
    let (out, o) = write_cli(p, &nist, "datum-targets", &["--mode", "replace"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let report: Json = serde_json::from_slice(&o.stdout).unwrap();
    let conflicts = report["practice_schema_conflicts"].as_array().unwrap();
    assert_eq!(conflicts.len(), 9);
    assert!(
        conflicts
            .iter()
            .all(|v| v["rule"] == "datum_target.WR5" && v["id"].as_u64().is_some())
    );
    let (read, _) = ok(&["pmi", "read", out.to_str().unwrap()]);
    assert_eq!(
        read["parts"][0]["pmi"]["datum_targets"],
        nist["parts"][0]["pmi"]["datum_targets"]
    );
    no_temporary_left(&out, "datum targets");

    // An existing destination stays as it was.
    let json = scratch().join("keep.json");
    std::fs::write(&json, other.to_string()).unwrap();
    let keep = scratch().join("keep.step");
    std::fs::write(&keep, b"untouched").unwrap();
    let o = quiddity(&[
        "pmi",
        "write",
        &step,
        json.to_str().unwrap(),
        "-o",
        keep.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(std::fs::read(&keep).unwrap(), b"untouched");
    no_temporary_left(&keep, "keep");
}

/// Add of a standard the part already states (NIST FTC-07's ASME Y14.41-2003) is that standard:
/// nothing is written, and the output is the input's text. Add of a second material (spool_fits
/// already names one) is accepted by the writer but fails the read-back (the reader finds two
/// material names): refused as not verifying, creating nothing.
#[test]
fn pmi_write_add_of_a_stated_standard_writes_nothing_and_a_second_material_does_not_verify() {
    let ftc07 = ap242().join("nist/nist_ftc_07_asme1_ap242-e2.stp.gz");
    let p = ftc07.to_str().unwrap();
    let (mut doc, _) = ok(&["pmi", "read", p]);
    let standards = doc["parts"][0]["pmi"]["standards"].clone();
    assert!(
        !standards.as_array().unwrap().is_empty(),
        "FTC-07 states a standard"
    );
    doc["parts"][0]["pmi"] = json!({ "standards": standards });
    let (out, o) = write_cli(p, &doc, "stated-standard", &[]);
    let (report, _) = write_ok(&o, "stated standard");
    assert_eq!(report["instances"]["added"], 0, "{report}");
    assert_eq!(bytes(&out), bytes(&ftc07), "the output is the input's text");

    let (mut doc, step) = spool_writable();
    assert!(doc["parts"][0]["pmi"]["material"].is_object());
    doc["parts"][0]["pmi"] = json!({ "material": { "id": "Steel" } });
    let (out, o) = write_cli(&step, &doc, "second-material", &[]);
    let err = write_refused(&out, &o, "second material");
    assert!(
        err.contains("not written: the output does not verify"),
        "{err}"
    );
    assert!(err.contains("material only in the second"), "{err}");
}

/// A destination that exists is replaced in place when the write verifies: through a symbolic
/// link to its target (the link stays a link), keeping the target's permissions.
#[cfg(unix)]
#[test]
fn pmi_write_replaces_an_existing_destination_through_a_link_keeping_its_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let (doc, step) = spool_writable();
    let dir = scratch().join("link");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("real")).unwrap();
    let target = dir.join("real/out.step");
    std::fs::write(&target, b"old").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
    let link = dir.join("out.step");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let json = dir.join("doc.json");
    std::fs::write(&json, doc.to_string()).unwrap();
    let o = quiddity(&[
        "pmi",
        "write",
        &step,
        json.to_str().unwrap(),
        "-o",
        link.to_str().unwrap(),
        "--mode",
        "replace",
    ]);
    write_ok(&o, "through a link");
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let m = std::fs::metadata(&target).unwrap();
    assert_eq!(m.permissions().mode() & 0o777, 0o640);
    assert_ne!(std::fs::read(&target).unwrap(), b"old");
    no_temporary_left(&target, "through a link");
}

/// Add onto a corpus part (AP214, no PMI): written as AP242 (decision 1), every item read back,
/// a datum feature symbol for A; then remove: the part reads back as it did at first. An AP203
/// file whose instances do not all validate against AP242 is refused, naming them.
#[test]
fn pmi_write_add_then_remove_returns_a_corpus_part_to_its_pmi() {
    let Some(corpus) = common::corpus_dir() else {
        assert!(
            std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
            "QUIDDITY_CORPUS_REQUIRED is set but the corpus was not found"
        );
        eprintln!("corpus not found; set QUIDDITY_CORPUS");
        return;
    };
    let path = corpus.join("cadgenbench/flanged_spool_132.step");
    let p = path.to_str().unwrap();
    let (original, _) = ok(&["pmi", "read", p]);
    let add = json!({
        "format": "quiddity-pmi",
        "version": pmi_json::VERSION,
        "binding": original["binding"],
        "parts": [{"part": 0, "pmi": {
            "features": [{"items": [{"face": 69}]}, {"items": [{"face": 55}]}],
            "datums": [{"label": "A", "feature": 0}],
            "dimensions": [
                {"kind": {"size": {"feature": 1, "kind": "diameter"}},
                 "nominal": {"value": "62.", "unit": "mm"},
                 "tolerance": {"fit": {"deviation": "H", "grade": "IT7"}}},
            ],
            "tolerances": [
                {"kind": "flatness", "target": {"feature": 0},
                 "magnitude": {"value": "0.02", "unit": "mm"}},
                {"kind": "perpendicularity", "target": {"feature": 1},
                 "magnitude": {"value": "0.05", "unit": "mm"},
                 "zone": {"form": "cylindrical or circular"},
                 "datums": [{"references": [{"datum": 0}]}]},
            ],
            "general": [{"class": {"text": "ISO 2768-mK"}}],
            "material": {"id": "Aluminium 6082-T6"},
        }}],
    });
    let (out, o) = write_cli(p, &add, "corpus-add", &[]);
    let (report, _) = write_ok(&o, "add");
    assert_eq!(report["mode"], "add");
    assert_eq!(
        report["file_schema"]["to"],
        "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 7 1 4 }"
    );
    assert_eq!(report["datum_symbols"], json!([{"part": 0, "labels": "A"}]));
    assert_eq!(report["repairs"], json!([]));
    let added = out.to_str().unwrap().to_string();
    let (back, _) = ok(&["pmi", "read", &added]);
    let d = read_back_differences(&add, &back, "add");
    assert!(d.is_empty(), "{}", d.join("\n"));

    let mut remove = back.clone();
    remove["parts"] = json!([{"part": 0, "pmi": {}}]);
    remove.as_object_mut().unwrap().remove("findings");
    let (out, o) = write_cli(&added, &remove, "corpus-remove", &["--mode", "remove"]);
    let (report, _) = write_ok(&o, "remove");
    assert_eq!(report["mode"], "remove");
    assert_eq!(report["instances"]["added"], 0);
    let (after, _) = ok(&["pmi", "read", out.to_str().unwrap()]);
    assert_eq!(after["parts"], original["parts"]);
    assert_eq!(after["findings"], original["findings"]);

    let ap203 = corpus.join("nist/nist_ctc_01_asme1_rd.stp");
    let p = ap203.to_str().unwrap();
    let (read, _) = ok(&["pmi", "read", p]);
    let mut doc = read.clone();
    doc["parts"] = json!([{"part": 0, "pmi": {"general": [{"class": {"text": "ISO 2768-m"}}]}}]);
    let (out, o) = write_cli(p, &doc, "edition-repaired", &[]);
    let (report, _) = write_ok(&o, "edition repaired");
    let repairs = report["repairs"].as_array().unwrap();
    assert_eq!(
        repairs
            .iter()
            .map(|r| (r["id"].as_u64().unwrap(), r["repair"].as_str().unwrap()))
            .collect::<Vec<_>>(),
        [(14, "R1"), (15, "R1")]
    );
    let before = haecceity::p21::Document::parse(std::fs::read(p).unwrap()).unwrap();
    let after = haecceity::p21::Document::parse(std::fs::read(&out).unwrap()).unwrap();
    for r in repairs {
        let id = r["id"].as_u64().unwrap();
        assert_eq!(
            r["original_record"].as_str().unwrap().as_bytes(),
            &before.bytes()[before.span(id).unwrap()]
        );
        assert_eq!(
            r["new_record"].as_str().unwrap().as_bytes(),
            &after.bytes()[after.span(id).unwrap()]
        );
    }
    let (back, _) = ok(&["pmi", "read", out.to_str().unwrap()]);
    assert!(read_back_differences(&doc, &back, "add").is_empty());

    // The edition refusal remains for anything outside the closed repair list (an approved
    // item in neither select: edition 4 approval_item takes any representation_item).
    let mut bad_bytes = std::fs::read(p).unwrap();
    let at = bad_bytes.windows(7).rposition(|w| w == b"ENDSEC;").unwrap();
    bad_bytes.splice(
        at..at,
        b"#900000000=DIMENSIONAL_EXPONENTS(0.,0.,0.,0.,0.,0.,0.);\n#900000001=CC_DESIGN_APPROVAL(#26,(#900000000));\n"
            .iter()
            .copied(),
    );
    let bad = scratch().join("bad-upgrade.step");
    std::fs::write(&bad, bad_bytes).unwrap();
    let p = bad.to_str().unwrap();
    let (read, _) = ok(&["pmi", "read", p]);
    doc["binding"] = read["binding"].clone();
    let (out, o) = write_cli(p, &doc, "edition", &[]);
    let err = write_refused(&out, &o, "edition");
    assert!(
        err.contains("cannot become AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF"),
        "{err}"
    );
    assert!(err.contains("instances violate it: #"), "{err}");
    assert!(err.contains("#900000001"), "{err}");
}

/// Before/after audit: pin every semantic field and finding, excluding format/binding
/// versions, over the supplied NIST directory and the committed semantic AP242 models.
#[test]
fn derived_geometry_does_not_regress_any_nist_pmi() {
    use quiddity::kernel::express;
    use quiddity::kernel::p21::RawEntity;
    let baseline: Json =
        serde_json::from_slice(&std::fs::read(ap242().join("derived/nist-baseline.json")).unwrap())
            .unwrap();
    let mut checked = 0;
    for row in baseline.as_array().unwrap() {
        let file = row["file"].as_str().unwrap();
        let path = if file.ends_with(".gz") {
            ap242().join("nist").join(file)
        } else {
            let Some(corpus) = common::corpus_dir() else {
                continue;
            };
            corpus.join("nist").join(file)
        };
        let b = bytes(&path);
        assert_eq!(
            Binding::of(&b).sha256,
            row["source_sha256"].as_str().unwrap(),
            "{file}"
        );
        let (json, _) = ok(&["pmi", "read", path.to_str().unwrap()]);
        let semantic = json!({"parts": json["parts"], "findings": json["findings"]});
        let canonical = serde_json::to_vec(&semantic).unwrap();
        assert_eq!(
            Binding::of(&canonical).sha256,
            row["semantic_sha256"].as_str().unwrap(),
            "{file}: resolved PMI or findings changed"
        );
        let doc = Document::parse(b).unwrap();
        let is_a = |id, parent| match doc.get(id).unwrap() {
            RawEntity::Simple { name, .. } => express::is_a(&name.to_ascii_lowercase(), parent),
            RawEntity::Complex { parts, .. } => parts
                .iter()
                .any(|p| express::is_a(&p.name.to_ascii_lowercase(), parent)),
        };
        let dimensions = doc
            .ids()
            .filter(|&id| is_a(id, "dimensional_size") || is_a(id, "dimensional_location"))
            .count();
        let tolerances = doc
            .ids()
            .filter(|&id| is_a(id, "geometric_tolerance"))
            .count();
        let derived = doc
            .ids()
            .filter(|&id| is_a(id, "derived_shape_aspect"))
            .count();
        let parts = json["parts"].as_array().unwrap();
        let read_count = |field| {
            parts
                .iter()
                .map(|p| p["pmi"][field].as_array().map_or(0, Vec::len))
                .sum::<usize>()
        };
        assert_eq!(
            dimensions,
            row["dimensions_stated"].as_u64().unwrap() as usize,
            "{file}"
        );
        assert_eq!(
            tolerances,
            row["tolerances_stated"].as_u64().unwrap() as usize,
            "{file}"
        );
        assert_eq!(
            derived,
            row["derived_aspects"].as_u64().unwrap() as usize,
            "{file}"
        );
        assert_eq!(
            dimensions - read_count("dimensions"),
            row["refused_dimensions"].as_u64().unwrap() as usize,
            "{file}"
        );
        assert_eq!(
            tolerances - read_count("tolerances"),
            row["refused_tolerances"].as_u64().unwrap() as usize,
            "{file}"
        );
        println!(
            "{file}: derived={derived}; dimensions {}/{dimensions}; tolerances {}/{tolerances}; refused {} -> {}",
            read_count("dimensions"),
            read_count("tolerances"),
            row["refused_dimensions"].as_u64().unwrap()
                + row["refused_tolerances"].as_u64().unwrap(),
            dimensions + tolerances - read_count("dimensions") - read_count("tolerances")
        );
        checked += 1;
    }
    assert!(checked >= 7, "the committed AP242 files are always checked");
}

#[test]
fn ap214_product_type_rewrites_are_reported() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("crates/haecceity/tests/fixtures/ap214_product_type.step");
    let step = path.to_str().unwrap();
    let (mut doc, _) = ok(&["pmi", "read", step]);
    doc["parts"] = json!([{"part": 0, "pmi": {
        "features": [{"items": [{"face": 3}]}],
        "tolerances": [{"kind": "flatness", "target": {"feature": 0},
            "magnitude": {"value": "0.05", "unit": "mm"}}],
        "material": {"id": "6061-T6"},
    }}]);
    let (out, output) = write_cli(step, &doc, "product-type", &[]);
    let (report, _) = write_ok(&output, "product-type");
    assert_eq!(
        report["edition_rewrites"],
        json!([{
            "id": 132, "from": "PRODUCT_TYPE", "to": "PRODUCT_RELATED_PRODUCT_CATEGORY",
        }])
    );
    assert_eq!(report["instances"]["replaced"], 1);
    let (back, _) = ok(&["pmi", "read", out.to_str().unwrap()]);
    assert!(read_back_differences(&doc, &back, "add").is_empty());
}
