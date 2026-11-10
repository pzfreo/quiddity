//! CAx-IF accuracy bands against independently authored STEP validation properties.
#[path = "../examples/gvp_support/mod.rs"]
mod support;
use serde_json::Value;
use std::path::Path;

fn fixtures() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}
fn corpus_dir() -> Option<std::path::PathBuf> {
    if std::env::var("QUIDDITY_CORPUS").is_ok_and(|v| v == "off") {
        return None;
    }
    let dir = std::env::var_os("QUIDDITY_CORPUS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../quiddity/tests/corpus")
        });
    dir.is_dir().then_some(dir)
}

const QUANTITIES: [&str; 4] = ["volume", "area", "independent_area", "centroid"];

/// Every committed file with distinct material validation properties, and its number of
/// checks per quantity (volume, area, independent area, centroid). Copies that only
/// differ in PMI (`ap242/write*/`, the fixtures root, `reexport/`, `specify/`) are left out.
const COMMITTED: [(&str, [usize; 4]); 9] = [
    ("ap242/nist/nist_ctc_01_asme1_ap242-e1.stp.gz", [1, 1, 0, 1]),
    ("ap242/nist/nist_ctc_02_asme1_ap242-e2.stp.gz", [2, 2, 3, 2]),
    ("ap242/nist/nist_ctc_03_asme1_ap242-e2.stp.gz", [2, 2, 0, 2]),
    ("ap242/nist/nist_ctc_04_asme1_ap242.stp.gz", [2, 2, 0, 2]),
    ("ap242/nist/nist_ftc_06_asme1_ap242-e2.stp.gz", [2, 2, 0, 2]),
    ("ap242/nist/nist_ftc_07_asme1_ap242-e2.stp.gz", [2, 2, 0, 2]),
    ("ap242/nist/nist_stc_06_asme1_ap242-e3.stp.gz", [1, 1, 0, 1]),
    ("ap242/nist/nist_stc_10_asme1_ap242-e2.stp.gz", [2, 2, 0, 2]),
    ("gvp/nist/nist_stc_08_asme1_ap242-e3.stp.gz", [2, 2, 2, 2]),
];

/// The surface-area scope of every product-level area on a part with independent sheets
/// (file, property, scope). Both scopes are within the band for CTC-04 and FTC-07, so a
/// changed scope rule would otherwise pass unnoticed.
const SCOPES: &[(&str, u64, &str)] = &[
    ("nist_ctc_02_asme1_ap242-e2.stp.gz", 22139, "solids"),
    ("nist_ctc_04_asme1_ap242.stp.gz", 18287, "solids"),
    ("nist_ftc_07_asme1_ap242-e2.stp.gz", 13668, "solids"),
    ("nist_stc_08_asme1_ap242-e3.stp.gz", 2609, "solids"),
    ("nist_ctc_02_asme1_rc.stp", 20807, "solids and sheets"),
    ("nist_ctc_05_asme1_rd.stp", 6664, "solids and sheets"),
];

fn check(path: &Path, failures: &mut Vec<String>, checks: &mut [usize; 4]) {
    let raw = support::bytes(path).unwrap();
    if !String::from_utf8_lossy(&raw)
        .to_ascii_lowercase()
        .contains("geometric validation property")
    {
        return;
    }
    let evidence: Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/gvp-occt.json")).unwrap();
    let key = path
        .strip_prefix(fixtures())
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"));
    let known = key.as_deref().and_then(|k| evidence["files"].get(k));
    let row = support::file(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    if let Some(known) = known {
        assert!(
            row["parts"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|p| p["gvp"].as_array().unwrap())
                .any(|p| p["property"] == known["property"]),
            "known reference error disappeared: reconsider evidence"
        );
    }
    for part in row["parts"].as_array().unwrap() {
        for prop in part["gvp"].as_array().unwrap() {
            if let Some(scope) = prop["ours"]["area_scope"].as_str()
                && !prop["reference"]["area"].is_null()
            {
                let file = path.file_name().unwrap().to_string_lossy();
                let property = prop["property"].as_u64().unwrap();
                let expected = SCOPES
                    .iter()
                    .find(|(f, p, _)| *f == file && *p == property)
                    .map(|s| s.2);
                if expected != Some(scope) {
                    failures.push(format!(
                        "{} GVP #{property}: area scope {scope}, expected {expected:?}",
                        path.display()
                    ));
                }
            }
            for (i, quantity) in QUANTITIES.iter().enumerate() {
                if prop["reference"][quantity].is_null() {
                    continue;
                }
                checks[i] += 1;
                let ours = &prop["ours"];
                let Some(diagonal) = part["ours"]["diagonal"].as_f64() else {
                    failures.push(format!(
                        "{} part {}: no bounding-box diagonal",
                        path.display(),
                        part["name"]
                    ));
                    continue;
                };
                let error = support::error(ours, &prop["reference"], quantity, diagonal);
                if let Some(known) = known.filter(|k| k["property"] == prop["property"]) {
                    assert_eq!(known["target"], prop["target"]);
                    assert_eq!(
                        prop["reference"][quantity], known["stored"][quantity],
                        "stored reference changed: reconsider exemption"
                    );
                    assert_eq!(
                        prop["faces"].as_array().unwrap().len(),
                        known["faces"].as_u64().unwrap() as usize
                    );
                    assert!(
                        support::error(&prop["reference"], &known["occt"], quantity, diagonal)
                            .is_some_and(|e| e > support::BAND),
                        "known error no longer disagrees with OCCT"
                    );
                    assert!(
                        error.is_some_and(|e| e > support::BAND),
                        "known error no longer disagrees with ours"
                    );
                    let independent = support::error(ours, &known["occt"], quantity, diagonal);
                    if !independent.is_some_and(|e| e <= support::BAND) {
                        failures.push(format!("{} GVP #{} {quantity}: ours={} OCCT={} normalized error={independent:?}; wrong-reference exemption requires agreement with OCCT",path.display(),prop["property"],ours[quantity],known["occt"][quantity]));
                    }
                } else if !error.is_some_and(|e| e <= support::BAND) {
                    failures.push(format!("{} part {} GVP #{} {quantity}: ours={} A={} normalized error={error:?}, faces={}",path.display(),part["name"],prop["property"],ours[quantity],prop["reference"][quantity],prop["faces"]));
                }
            }
        }
    }
    for p in row["unattached_gvp"].as_array().unwrap() {
        if QUANTITIES.iter().any(|q| p["reference"][q] != Value::Null) {
            failures.push(format!("{} unattached material GVP {p}", path.display()));
        }
    }
}
#[test]
fn committed_geometric_validation_properties() {
    let mut failures = Vec::new();
    for (name, expected) in COMMITTED {
        let mut checks = [0; 4];
        check(&fixtures().join(name), &mut failures, &mut checks);
        if checks != expected {
            failures.push(format!("{name}: checks {checks:?}, expected {expected:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
const CORPUS_CHECKS: [usize; 4] = [6, 8, 0, 6];

#[test]
fn corpus_geometric_validation_properties() {
    let Some(dir) = corpus_dir() else {
        assert!(
            std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
            "QUIDDITY_CORPUS_REQUIRED but no corpus"
        );
        return;
    };
    let mut paths = Vec::new();
    support::walk(&dir, &mut paths).unwrap();
    paths.sort();
    let (mut failures, mut checks) = (Vec::new(), [0; 4]);
    for path in paths {
        check(&path, &mut failures, &mut checks);
    }
    // The corpus is pinned (tests/corpus_files.rs), so its checks are too.
    if checks != CORPUS_CHECKS {
        failures.push(format!(
            "corpus checks {checks:?}, expected {CORPUS_CHECKS:?}"
        ));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
