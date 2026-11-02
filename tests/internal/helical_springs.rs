//! Structural geometry, publication and precedence checks; no output hashes.
use super::common;
use quiddity::Part;
use quiddity::features::{self, Context, helical_springs};
use quiddity::kernel::geom;

fn fixture(name: &str) -> std::path::PathBuf {
    common::fixtures()
        .join("helical_springs")
        .join(format!("{name}.step.gz"))
}
fn read(name: &str) -> Part {
    quiddity::read_step_file(&fixture(name)).unwrap()
}
fn close(a: f64, b: f64, t: f64) {
    assert!((a - b).abs() < t, "{a} != {b} ± {t}");
}

#[test]
fn segmented_wire_is_one_spring_and_no_wire_patch_callouts() {
    let part = read("segmented");
    let found = helical_springs::discover(&Context::new(&part));
    assert_eq!(found.len(), 1);
    let s = &found[0].record;
    close(s.wire_diameter, 1.5, 1e-6);
    close(s.mean_diameter, 24., 1e-5);
    close(s.outer_diameter, 25.5, 1e-5);
    close(s.inner_diameter, 22.5, 1e-5);
    close(s.pitch, 4., 1e-5);
    assert_eq!(s.hand, "right");
    assert_eq!(s.active_coils, None);
    assert!(s.active_coils_basis.contains("unknown"));
    assert!(s.total_coils > 3. && s.total_coils < 3.03);
    assert!(s.free_length > 13.5 && s.free_length < 13.6);
    assert_eq!(s.faces, (0..part.faces.len()).collect::<Vec<_>>());
    let aggregate = features::recognise(&part);
    assert_eq!(aggregate.helical_springs.len(), 1);
    for (family, records) in &aggregate.defining {
        if *family != "helical_springs" {
            assert!(records.is_empty(), "duplicate {family}: {records:?}");
        }
    }
    let evidence: serde_json::Value =
        serde_json::from_str(&quiddity::file_api::evidence(&part, false).unwrap()).unwrap();
    assert_eq!(
        evidence["features"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["family"] == "helical_springs")
            .count(),
        1
    );
    assert!(
        aggregate.holes.is_empty() && aggregate.bosses.is_empty() && aggregate.fillets.is_empty()
    );
}

#[test]
fn true_round_wire_sweep_has_measured_helix_and_document_contract() {
    let path = common::fixtures().join("modelled_threads/round_wire_spring.step.gz");
    let part = quiddity::read_step_file(&path).unwrap();
    let found = helical_springs::discover(&Context::new(&part));
    assert_eq!(found.len(), 1);
    let s = &found[0].record;
    close(s.wire_diameter, 1.5, 0.002);
    close(s.mean_diameter, 12., 0.002);
    close(s.pitch, 4., 0.002);
    close(s.total_coils, 3., 0.001);
    assert_eq!(s.hand, "right");
    assert!(s.free_length > 13.4 && s.free_length < 13.6);
    let output: serde_json::Value =
        serde_json::from_str(&quiddity::file_api::recognise_file(path.to_str().unwrap()).unwrap())
            .unwrap();
    let records = output["document"]["records"].as_array().unwrap();
    let springs: Vec<_> = records
        .iter()
        .filter(|r| r["family"] == "helical_springs")
        .collect();
    assert_eq!(springs.len(), 1);
    let r = springs[0];
    assert_eq!(r["record_type"], "HelicalSpring");
    assert_eq!(r["evidence_status"], "verified");
    assert_eq!(r["faces"].as_array().unwrap().len(), part.faces.len());
    assert!(r["parameters"]["active_coils"].is_null());
    assert!(output["helical_springs"].is_array());
    let fp = output["fingerprints"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["family"] == "helical_springs")
        .unwrap();
    assert!(fp["axis"].is_array());
    assert!(fp["sizes"]["pitch"].is_number());
}

#[test]
fn pins_knurls_threads_and_varying_pitch_are_not_springs() {
    for name in ["bolt_circle", "pin_row"] {
        assert!(
            helical_springs::discover(&Context::new(&read(name))).is_empty(),
            "{name}"
        );
    }
    for name in [
        "diamond_knurl",
        "left_two_start",
        "varying_pitch_sweep",
        "helical_groove",
    ] {
        let path = common::fixtures()
            .join("modelled_threads")
            .join(format!("{name}.step.gz"));
        let part = quiddity::read_step_file(&path).unwrap();
        assert!(
            helical_springs::discover(&Context::new(&part)).is_empty(),
            "{name}"
        );
    }
}

#[test]
fn axis_measurements_and_hand_survive_motion_and_left_hand_geometry() {
    let path = fixture("segmented");
    let original = quiddity::read_step_file(&path).unwrap();
    let angle = 0.47_f64;
    // The dominant axis reverses after rotation, exercising start/end reanchoring.
    let placement = [
        [-angle.cos(), 0., angle.sin(), 123.],
        [0., 1., 0., -47.],
        [-angle.sin(), 0., -angle.cos(), 91.],
    ];
    let moved = quiddity::kernel::step::read_step_file_placed(&path, &placement).unwrap();
    let actual = helical_springs::discover(&Context::new(&moved));
    assert_eq!(actual.len(), 1);
    let mapped = quiddity::framed_records::map_features(
        features::recognise(&original),
        &quiddity::framed_records::Rigid::from_placement(&placement),
    );
    let (a, b) = (&actual[0].record, &mapped.features.helical_springs[0]);
    close(geom::dist(a.axis.point, b.axis.point), 0., 0.001);
    close(geom::dist(a.axis.direction, b.axis.direction), 0., 1e-7);
    close(a.start, b.start, 0.001);
    close(a.end, b.end, 0.001);
    close(a.free_length, b.free_length, 0.001);
    close(a.pitch, b.pitch, 0.001);
    assert_eq!(a.hand, b.hand);
    let left = read("swept_left");
    let left = helical_springs::discover(&Context::new(&left));
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].record.hand, "left");
    close(left[0].record.pitch, 4., 1e-5);
}

#[test]
fn analytic_torus_biarcs_follow_one_helix() {
    let part = read("torus_chain");
    assert_eq!(
        part.faces
            .iter()
            .filter(|f| matches!(f.surface, geom::Surface::Torus { .. }))
            .count(),
        72
    );
    let found = helical_springs::discover(&Context::new(&part));
    assert_eq!(found.len(), 1);
    let s = &found[0].record;
    close(s.wire_diameter, 1.5, 0.001);
    close(s.mean_diameter, 24., 0.001);
    close(s.pitch, 4., 0.001);
    close(s.total_coils, 3., 0.001);
    assert_eq!(s.hand, "right");
    assert_eq!(s.end_type, "plain");
    assert_eq!(s.faces.len(), part.faces.len());
}
