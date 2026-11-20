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
