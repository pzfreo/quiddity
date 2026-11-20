//! `fins`: thin walls standing on a host surface. The `fins_*` fixtures are synthetic
//! (build123d, `tools/make_fin_fixtures.py`); `stock_envelope_housing` is a finned housing.
use super::common;
use quiddity::features::{self, Features};

fn recognise(name: &str) -> Features {
    let p = quiddity::read_step_file(&common::fixtures().join(name)).unwrap();
    features::recognise(&p)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 2e-3
}

#[test]
fn a_radial_fin_array_on_a_housing_is_sixteen_fins() {
    let f = recognise("stock_envelope_housing.step.gz");
    assert_eq!(f.fins.len(), 16);
    let mut faces = Vec::new();
    for fin in &f.fins {
        assert_eq!(fin.thickness, 2.);
        assert_eq!(fin.length, 54.);
        // 30 - sqrt(30^2 - 1) = 0.017 short of 12: the root meets the tube off the axis line.
        assert!(close(fin.height, 12.017), "{}", fin.height);
        assert_eq!(fin.run, [0., 0., 1.]);
        assert_eq!(fin.host, "mixed"); // the tube and the flange
        assert_eq!((fin.side_faces.len(), fin.tip_faces.len()), (2, 1));
        // Radial: the rise is perpendicular to the normal and to the run.
        assert!(
            fin.rise[2] == 0.
                && (fin.rise[0] * fin.normal[0] + fin.rise[1] * fin.normal[1]).abs() < 1e-5
        );
        faces.extend(fin.side_faces.iter().chain(&fin.tip_faces).copied());
    }
    faces.sort_unstable();
    faces.dedup();
    assert_eq!(faces.len(), 48);
    // The fins claim their sides and tips, and nothing else: no host and no housing top (23).
    let claimed: Vec<usize> = f.defining["fins"].iter().flatten().copied().collect();
    assert_eq!(claimed.len(), 48);
    assert!(!claimed.contains(&23));
    for fin in &f.fins {
        assert!(fin.host_faces.iter().all(|h| !claimed.contains(h)));
    }
}

#[test]
fn a_half_round_tip_rib_is_one_fin_that_leaves_the_tip_to_its_claimant() {
    let f = recognise("fins_round_tip_rib.step.gz");
    assert_eq!(f.fins.len(), 1);
    let fin = &f.fins[0];
    assert_eq!((fin.thickness, fin.length, fin.height), (2., 80., 10.)); // the R1 tip counts
    assert_eq!((fin.host, fin.tip_faces.len()), ("plane", 3));
    // The tip cylinder is a blend's: the fin does not claim it.
    assert_eq!(f.blends.len(), 1);
    let blend: Vec<usize> = f.defining["blends"].iter().flatten().copied().collect();
    let claimed = &f.defining["fins"][0];
    assert!(
        blend
            .iter()
            .all(|b| fin.tip_faces.contains(b) && !claimed.contains(b))
    );
    assert_eq!(claimed.len(), 4);
}

#[test]
fn a_gusset_is_the_gusset_records_and_a_free_triangular_rib_is_a_fin() {
    let f = recognise("fins_gusset_plate.step.gz");
    assert_eq!(f.gusset_ribs.len(), 1);
    assert_eq!(f.fins.len(), 1);
    let fin = &f.fins[0];
    assert_eq!((fin.thickness, fin.height, fin.length), (3., 20., 25.));
    let gusset: Vec<usize> = f.defining["gusset_ribs"]
        .iter()
        .flatten()
        .copied()
        .collect();
    assert!(f.defining["fins"][0].iter().all(|x| !gusset.contains(x)));
}

#[test]
fn a_rib_another_family_claims_is_not_published_twice() {
    // `pads` claim straight ribs on a plate, one rib or three.
    for name in ["fins_flat_rib.step.gz", "fins_ribbed_plate.step.gz"] {
        let f = recognise(name);
        assert!(!f.pads.is_empty(), "{name}");
        assert!(f.fins.is_empty(), "{name}");
    }
}

#[test]
fn walls_that_are_not_thin_fins_are_not_fins() {
    for name in [
        "fins_boss_pad.step.gz",      // a boss and a pad on a plate
        "fins_pocket.step.gz",        // the wall of a pocket
        "fins_thick_step.step.gz",    // a 12-thick bar: not thin
        "fins_sheet_flange.step.gz",  // a sharp sheet flange: no host, sides unlike
        "fins_sheet_bend.step.gz",    // a bent flange: tangent bend
        "flats_low_tab.step.gz",      // a 4-thick, 1.3-high tab on a shaft
        "flats_finned_shaft.step.gz", // 4-thick, 10-high fins: thickness over a quarter of height
        "stock_envelope_plate.step.gz",
        "stock_envelope_step_allround.step.gz",
        "stock_envelope_pad_allround.step.gz",
        "stock_envelope_rib_allround.step.gz",
    ] {
        assert!(recognise(name).fins.is_empty(), "{name}");
    }
}
