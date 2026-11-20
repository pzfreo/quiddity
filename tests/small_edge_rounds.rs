//! #297: every edge round is reported at its own radius, however small, including where
//! the rounds of an all-edges break meet at corner spheres.
use std::path::Path;
use std::process::Command;

/// The fillet records of a recognised fixture, as (axis, radius, side).
fn fillets(name: &str) -> Vec<(String, f64, String)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/brep-conformance")
        .join(name);
    let out = Command::new(env!("CARGO_BIN_EXE_quiddity"))
        .arg(path)
        .output()
        .expect("quiddity runs");
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    json["document"]["records"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["family"] == "fillets")
        .map(|r| {
            let p = &r["parameters"];
            (
                p["axis"].as_str().unwrap().to_string(),
                p["radius"].as_f64().unwrap(),
                p["side"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

/// A box (or prism) with all 12 edges rounded: four rounds on each of the three axes.
fn assert_twelve_rounds(name: &str, radius: f64) {
    let found = fillets(name);
    assert_eq!(found.len(), 12, "{name}: {found:?}");
    for axis in ["x", "y", "z"] {
        assert_eq!(
            found.iter().filter(|f| f.0 == axis).count(),
            4,
            "{name} {axis}"
        );
    }
    for (axis, r, side) in &found {
        // Reported to 3 decimals; the STEP's own radius is the oracle.
        assert!((r - radius).abs() <= 5e-4, "{name} {axis}: {r} vs {radius}");
        assert_eq!(side, "convex");
    }
}

/// OpenCascade 8.0.1 box, 26 faces: R0.337687 on all 12 edges (below the old 0.6 mm floor).
#[test]
fn a_box_with_every_edge_rounded_at_a_third_of_a_millimetre() {
    assert_twelve_rounds("rounded-boxes-dd2233577a7b962b.step.gz", 0.337687463199);
}

/// OpenCascade 8.0.1 prism, 26 faces: R0.558518 on all 12 edges (below the old floor).
#[test]
fn a_prism_with_every_edge_rounded_at_half_a_millimetre() {
    assert_twelve_rounds("rounded-prisms-2e6ad17301305f19.step.gz", 0.558518312407);
}
