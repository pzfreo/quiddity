//! Radial fins as `repeating_radial_profiles` (#307 item 10): fins that stop short of the
//! barrel's ends are one profile standing on the barrel between two interior levels, as full
//! length fins are on the end faces. The fixtures are synthetic (build123d).
use super::common;
use quiddity::features::{self, Context, repeating_profiles};

fn part(name: &str) -> quiddity::Part {
    quiddity::read_step_file(&common::fixtures().join(name)).unwrap()
}

#[test]
fn housing_fins_are_one_profile_claiming_every_fin_face() {
    let p = part("radial_fin_housing.step.gz");
    let f = features::recognise(&p);
    let [r] = &f.repeating_radial_profiles[..] else {
        panic!("{} profiles", f.repeating_radial_profiles.len());
    };
    assert_eq!(
        (r.axis.as_str(), r.repeat_count, r.edge_count),
        ("z", 64, 256)
    );
    assert_eq!(r.span, [-26.5, 12.0]);
    assert!(r.centre[0].abs() < 1e-9 && r.centre[1].abs() < 1e-9);
    // Per fin: one cap at each end, two sides and a tip.
    let found = repeating_profiles::discover(&Context::new(&p));
    assert_eq!(found[0].defining.len(), 64 * 5);
    // The four axis-aligned fins are members of the array, not pads of their own.
    assert!(f.pads.is_empty());
    // The barrel, rear wall, sleeve and standoffs are not claimed: the fin faces are all
    // planes, and no cylinder is among them.
    assert!(found[0].defining.iter().all(|&face| {
        matches!(
            p.faces[face].surface,
            quiddity::kernel::geom::Surface::Plane { .. }
        )
    }));
}

#[test]
fn full_length_and_short_fins_give_the_same_profile() {
    let a = features::recognise(&part("radial_fin_tube.step.gz"));
    let b = features::recognise(&part("radial_fin_short.step.gz"));
    let ([x], [y]) = (
        &a.repeating_radial_profiles[..],
        &b.repeating_radial_profiles[..],
    ) else {
        panic!("one profile each");
    };
    assert_eq!((x.repeat_count, x.edge_count), (24, 96));
    assert_eq!(x.sector_signature, y.sector_signature);
    assert_eq!(x.span, [0.0, 40.0]);
    assert_eq!(y.span, [6.0, 34.0]);
    assert!(b.pads.is_empty());
}

#[test]
fn ribs_walls_irregular_fins_and_notches_are_not_profiles() {
    for name in [
        "radial_fin_single.step.gz",
        "radial_fin_walls.step.gz",
        "radial_fin_irregular.step.gz",
        "radial_fin_notched.step.gz",
        // A polygon or star between two round sections: no one host under its caps.
        "radial_fin_hex.step.gz",
        "radial_fin_octagon.step.gz",
        "radial_fin_star.step.gz",
    ] {
        let f = features::recognise(&part(name));
        assert!(f.repeating_radial_profiles.is_empty(), "{name}");
    }
    // The irregular housing keeps its four pads: its fins are not an array.
    assert_eq!(
        features::recognise(&part("radial_fin_irregular.step.gz"))
            .pads
            .len(),
        4
    );
}

#[test]
fn a_gear_is_still_one_profile_on_its_end_faces() {
    let p =
        quiddity::read_step_file(&common::fixtures().join("golden_repeating_radial_profile.step"))
            .unwrap();
    let found = repeating_profiles::discover(&Context::new(&p));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].defining.len(), 2);
}

#[test]
fn housing_rims_are_body_and_the_rest_is_left_unexplained_for_stated_reasons() {
    use quiddity::recognition::{BodyReason, FaceAccounting};
    let p = part("radial_fin_housing.step.gz");
    let recognition = quiddity::correspondence::recognise(&p);
    let groups = quiddity::recognition::document(&recognition, p.faces.len())
        .face_accounting
        .unwrap();
    let plane_at = |z: f64, lo: f64, hi: f64| -> Vec<usize> {
        (0..p.faces.len())
            .filter(|&f| {
                matches!(
                    p.faces[f].surface,
                    quiddity::kernel::geom::Surface::Plane { .. }
                )
            })
            .filter(|&f| {
                let b = p.face_bounds(f);
                let width = b.max[0] - b.min[0];
                (b.min[2] - z).abs() < 1e-6
                    && (b.max[2] - z).abs() < 1e-6
                    && (lo..=hi).contains(&width)
            })
            .collect()
    };
    // The barrel's rims: the bar's end planes.
    for z in [-31.0, 29.0] {
        let rim = plane_at(z, 99.0, 101.0);
        assert_eq!(rim.len(), 1);
        assert_eq!(
            groups[rim[0]],
            FaceAccounting::Body {
                reason: BodyReason::StockEnvelope
            }
        );
    }
    // Standoff tops and the sleeve's end are boss caps (consulted, not claimed by the boss
    // record), and the rear recess's floor ring carries islands no family describes.
    let unexplained: Vec<usize> = (0..p.faces.len())
        .filter(|&f| groups[f] == FaceAccounting::Unexplained)
        .collect();
    assert_eq!(unexplained.len(), 7);
    for (z, lo, hi) in [(-28.8, 5.9, 6.1), (-24.4, 4.9, 5.1)] {
        let found = plane_at(z, lo, hi);
        assert!(!found.is_empty(), "no plane at z={z}");
        for f in found {
            assert!(unexplained.contains(&f), "face {f} at z={z}");
        }
    }
    // The rear wall's faces, the shoulder between the d26 and d30 bores and the sleeve's
    // shoulder are step levels along the barrel's axis (#307 item 10: the frame's z is across
    // it, so these are levels along the frame's x), each in exactly one level; the fin sides,
    // which the fins record claims, are in none.
    let document = quiddity::recognition::document(&recognition, p.faces.len());
    let levels: Vec<_> = document
        .records
        .iter()
        .filter(|r| r.family == "step_levels")
        .collect();
    assert_eq!(levels.len(), 3);
    for level in &levels {
        assert_eq!(level.parameters["height_axis"], "z");
    }
    for (z, lo, hi) in [
        (1.5, 55.0, 57.0),
        (-18.0, 29.0, 31.0),
        (-22.0, 91.0, 93.0),
        (-18.0, 91.0, 93.0),
    ] {
        let found = plane_at(z, lo, hi);
        assert!(!found.is_empty(), "no plane at z={z}");
        for f in found {
            let owners: Vec<_> = levels.iter().filter(|l| l.faces.contains(&f)).collect();
            assert_eq!(owners.len(), 1, "face {f} at z={z}");
        }
    }
    let fins = &recognition.features.working().defining["repeating_radial_profiles"][0];
    assert!(
        levels
            .iter()
            .all(|l| l.faces.iter().all(|f| !fins.contains(f)))
    );
    // The rear recess's cylinder wall (r46, z -31..-22) is unexplained as well.
    let wall: Vec<usize> = (0..p.faces.len())
        .filter(|&f| {
            let b = p.face_bounds(f);
            matches!(
                p.faces[f].surface,
                quiddity::kernel::geom::Surface::Cylinder { .. }
            ) && (b.min[2] + 31.0).abs() < 1e-6
                && (b.max[2] + 22.0).abs() < 1e-6
                && (b.max[0] - 46.0).abs() < 1e-6
        })
        .collect();
    assert_eq!(wall.len(), 1);
    assert_eq!(groups[wall[0]], FaceAccounting::Unexplained);
}

/// `quiddity evidence`, framed and in file coordinates, reports the housing's step levels and
/// risers on the faces the recognition document publishes: the three levels along the barrel's
/// axis, none on the fin sides or fin ends.
#[test]
fn housing_evidence_lists_the_published_step_levels() {
    let p = part("radial_fin_housing.step.gz");
    let recognition = quiddity::correspondence::recognise(&p);
    let document = quiddity::recognition::document(&recognition, p.faces.len());
    let published = |family: &str| -> Vec<Vec<usize>> {
        let mut found: Vec<Vec<usize>> = document
            .records
            .iter()
            .filter(|r| r.family == family)
            .map(|r| {
                let mut faces = r.faces.clone();
                faces.sort_unstable();
                faces
            })
            .collect();
        found.sort();
        found
    };
    assert_eq!(published("step_levels").len(), 3);
    for framed in [true, false] {
        let evidence = quiddity::evidence_view::document(&p, framed).unwrap();
        for family in ["step_levels", "risers"] {
            let mut found: Vec<Vec<usize>> = evidence
                .features
                .iter()
                .filter(|f| f.family == family)
                .map(|f| f.defining_faces.clone())
                .collect();
            found.sort();
            assert_eq!(found, published(family), "{family}, framed {framed}");
        }
    }
}
