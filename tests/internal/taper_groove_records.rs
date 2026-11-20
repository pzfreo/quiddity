//! Tapers and V-grooves as records of their own (#307 item 6), on original turned parts
//! made by `tools/make_taper_groove_fixtures.py`.
use super::common;
use quiddity::features::{self, Features};
use serde_json::Value;

fn read(name: &str) -> Features {
    let part =
        quiddity::read_step_file(&common::fixtures().join(format!("{name}.step.gz"))).unwrap();
    features::recognise(&part)
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-6, "{a} != {b}");
}
fn angle(half_opening: f64, adjacent: f64) -> f64 {
    2. * half_opening.atan2(adjacent).to_degrees()
}

#[test]
fn a_morse_like_shaft_taper_has_both_diameters_length_and_angle() {
    let f = read("taper_morse");
    assert!(f.v_grooves.is_empty());
    let [t] = &f.tapers[..] else {
        panic!("{:?}", f.tapers)
    };
    assert_eq!(t.side, "external");
    near(t.large_diameter, 20.);
    near(t.small_diameter, 12.5);
    near(t.length, 60.);
    near(t.included_angle, angle(3.75, 60.));
    near(t.axis[2], 1.);
    near(t.axis_point[2], 30.);
    // The turned body's profile is unchanged and holds the cone this record names.
    let body = &f.turned_bodies[t.turned_body];
    assert!(
        body.profile
            .iter()
            .any(|s| s.kind == "cone" && s.face == t.faces[0])
    );
    assert_eq!(body.profile.len(), 5);
}

#[test]
fn a_bare_frustum_between_two_end_planes_is_a_taper() {
    let f = read("taper_shank");
    let [t] = &f.tapers[..] else {
        panic!("{:?}", f.tapers)
    };
    near(t.large_diameter, 40.);
    near(t.small_diameter, 24.);
    near(t.length, 16.);
    near(t.included_angle, angle(8., 16.));
    // From the large end to the small one: against +z, as the part is built.
    near(t.axis[2], -1.);
}

#[test]
fn an_internal_taper_pin_hole_is_an_internal_taper() {
    let f = read("taper_bore_pin");
    let [t] = &f.tapers[..] else {
        panic!("{:?}", f.tapers)
    };
    assert_eq!(t.side, "internal");
    near(t.large_diameter, 16.);
    near(t.small_diameter, 10.);
    near(t.length, 40.);
    near(t.included_angle, angle(3., 40.));
    assert!(f.v_grooves.is_empty());
}

#[test]
fn an_external_v_groove_has_width_depth_and_angle_and_no_taper() {
    let f = read("vgroove_external");
    let [g] = &f.v_grooves[..] else {
        panic!("{:?}", f.v_grooves)
    };
    assert_eq!(g.side, "external");
    near(g.width, 4.);
    near(g.depth, 2.);
    near(g.included_angle, 90.);
    near(g.mouth_diameter, 50.);
    near(g.bottom_diameter, 46.);
    near(g.axis[2], 1.);
    assert_eq!(g.faces.len(), 2);
    assert!(
        f.tapers.is_empty(),
        "the flanks are not tapers: {:?}",
        f.tapers
    );
}

#[test]
fn an_internal_v_groove_in_a_ring_bore_claims_only_its_flanks() {
    let f = read("vgroove_internal");
    let [g] = &f.v_grooves[..] else {
        panic!("{:?}", f.v_grooves)
    };
    assert_eq!(g.side, "internal");
    let half = 5.65 / 2f64.sqrt();
    near(g.width, 2. * (half - 2.));
    near(g.depth, half - 2.);
    near(g.included_angle, 90.);
    near(g.mouth_diameter, 83.);
    assert!(f.tapers.is_empty());
    // No face that a hole or other record defines is also claimed here.
    for (family, records) in &f.defining {
        if matches!(*family, "v_grooves" | "turned_bodies") {
            continue;
        }
        for faces in records {
            assert!(
                faces.iter().all(|face| !g.faces.contains(face)),
                "{family} shares a face with the V groove"
            );
        }
    }
}

#[test]
fn cones_that_other_records_define_are_left_to_them() {
    // A V groove with a tiny flat is already `grooves` plus two `chamfers`; a blind taper socket
    // and a closed chamber are holes with countersinks. None gains a second claim.
    for (name, family) in [
        ("vgroove_flat", "grooves"),
        ("taper_bore_socket", "holes"),
        ("taper_bore_chamber", "holes"),
    ] {
        let f = read(name);
        assert!(f.tapers.is_empty() && f.v_grooves.is_empty(), "{name}");
        assert!(!f.defining[family].is_empty(), "{name}");
    }
}

#[test]
fn edge_breaks_countersinks_drill_points_trapezoids_and_ridges_publish_neither() {
    for name in [
        "taper_control_chamfer",
        "taper_control_chamfer_narrow",
        "taper_control_countersink",
        "taper_control_drill_point",
        "taper_control_trapezoid",
        "taper_control_ridge",
        "taper_control_wide_frustum",
        "taper_control_tiny_step",
        "taper_control_web",
    ] {
        let f = read(name);
        assert!(f.tapers.is_empty(), "{name}: {:?}", f.tapers);
        assert!(f.v_grooves.is_empty(), "{name}: {:?}", f.v_grooves);
    }
}

#[test]
fn the_records_are_published_with_their_family_and_type() {
    let part =
        quiddity::read_step_file(&common::fixtures().join("vgroove_external.step.gz")).unwrap();
    let reading = quiddity::correspondence::recognise(&part);
    let doc = quiddity::recognition::document(&reading, part.faces.len());
    let [r] = &doc
        .records
        .iter()
        .filter(|r| r.family == "v_grooves")
        .collect::<Vec<_>>()[..]
    else {
        panic!()
    };
    assert_eq!(r.record_type, "VGroove");
    let json = serde_json::to_value(r).unwrap();
    let text = json.to_string();
    assert!(text.contains("included_angle"), "{text}");
    let _: Value = json;
}
