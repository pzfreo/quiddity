//! #307 (item 7): an edge round that follows a slot's rounded end is a fillet of its tube
//! radius; grooves, ridges and a whole bore's mouth ring are not.
use std::io::Read;
use std::path::Path;
use std::process::Command;

fn recognise(name: &str) -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let mut text = Vec::new();
    flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap())
        .read_to_end(&mut text)
        .unwrap();
    let step = std::env::temp_dir().join(format!(
        "quiddity-slot-end-{}-{name}.step",
        std::process::id()
    ));
    std::fs::write(&step, text).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_quiddity"))
        .arg(&step)
        .output()
        .expect("quiddity runs");
    std::fs::remove_file(&step).ok();
    assert!(out.status.success());
    serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()["document"].clone()
}

fn fillets(doc: &serde_json::Value) -> Vec<(Vec<u64>, f64, String)> {
    doc["records"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["family"] == "fillets")
        .map(|r| {
            let faces = r["faces"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f.as_u64().unwrap())
                .collect();
            let p = &r["parameters"];
            (
                faces,
                p["radius"].as_f64().unwrap(),
                p["side"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

/// The 100 x 60 x 8 plate with a 30 x 8 slot, every straight edge rounded R0.8 (38 faces):
/// 20 straight R0.8 rounds, four R0.8 corner rings and four R5 corners were already found;
/// the four tori at the slot's ends (faces 21, 22, 29, 30) are the new fillets.
#[test]
fn the_toroidal_rounds_at_a_slots_ends_are_fillets_of_the_tube_radius() {
    let found = fillets(&recognise("slot_end_rounds.step.gz"));
    assert_eq!(found.len(), 28, "{found:?}");
    for torus in [21, 22, 29, 30] {
        let (_, radius, side) = found
            .iter()
            .find(|f| f.0 == [torus])
            .unwrap_or_else(|| panic!("face {torus} is not a fillet: {found:?}"));
        assert!((radius - 0.8).abs() < 5e-4);
        assert_eq!(side, "convex");
    }
    assert_eq!(
        found.iter().filter(|f| (f.1 - 0.8).abs() < 5e-4).count(),
        24
    );
    assert_eq!(found.iter().filter(|f| (f.1 - 5.0).abs() < 5e-4).count(), 4);
}

#[test]
fn grooves_ridges_and_whole_bore_mouths_are_not_slot_end_fillets() {
    for name in ["round_hole_mouth", "bore_groove", "bore_ridge"] {
        let found = fillets(&recognise(&format!("{name}.step.gz")));
        assert!(found.is_empty(), "{name}: {found:?}");
    }
}
