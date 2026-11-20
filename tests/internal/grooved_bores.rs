//! A bore with an internal groove is one hole, and the groove is not its floor (quiddity#307).
use super::common;
use quiddity::features::{Context, holes};
use quiddity::kernel::step::IDENTITY;

/// (diameter, bottom, depth) of every hole of a fixture, in a placement, sorted.
fn holes_of(fixture: &str, placement: &[[f64; 4]; 3]) -> Vec<(f64, holes::Bottom, f64)> {
    let base =
        quiddity::read_step_file(&common::fixtures().join(format!("{fixture}.step.gz"))).unwrap();
    let part = base.transformed(placement).unwrap();
    let mut found: Vec<_> = holes::discover_verified(&Context::new(&part), &[])
        .unwrap()
        .iter()
        .map(|o| (o.record.diameter, o.record.bottom, o.record.depth))
        .collect();
    found.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.2.total_cmp(&b.2)));
    found
}

fn placements() -> [[[f64; 4]; 3]; 2] {
    let (s, c) = 0.37_f64.sin_cos();
    [
        IDENTITY,
        [[c, 0., s, 123.456], [0., 1., 0., -78.9], [-s, 0., c, 41.3]],
    ]
}

fn assert_holes(fixture: &str, expected: &[(f64, holes::Bottom, f64)]) {
    for placement in placements() {
        let found = holes_of(fixture, &placement);
        assert_eq!(found.len(), expected.len(), "{fixture}: {found:?}");
        for (got, want) in found.iter().zip(expected) {
            assert_eq!((got.0, got.1), (want.0, want.1), "{fixture}: {found:?}");
            assert!((got.2 - want.2).abs() < 1e-6, "{fixture}: {found:?}");
        }
    }
}

#[test]
fn a_ball_groove_between_two_lands_is_one_through_bore() {
    // d27.2, 5 mm long: two 1.35 mm lands either side of a toroidal ball groove.
    assert_holes("grooved_bore_race", &[(27.2, holes::Bottom::Through, 5.0)]);
}

#[test]
fn a_v_groove_between_two_lands_is_one_through_bore() {
    // d83, 8 mm long: lands z 21..23 and 27..29 either side of a 90 degree V groove.
    assert_holes("grooved_bore_ring", &[(83.0, holes::Bottom::Through, 8.0)]);
}

#[test]
fn lands_separated_by_material_inside_the_bore_stay_separate_holes() {
    // A solid web across the bore: two blind holes back to back, each ending on the web.
    assert_holes(
        "grooved_bore_web",
        &[
            (27.2, holes::Bottom::Flat, 2.0),
            (27.2, holes::Bottom::Flat, 2.0),
        ],
    );
    // The same web across the ball groove: still two holes, and the groove is not a floor.
    for placement in placements() {
        let found = holes_of("grooved_bore_webgroove", &placement);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            found
                .iter()
                .all(|h| h.0 == 27.2 && h.1 != holes::Bottom::Through)
        );
    }
}

#[test]
fn lands_of_different_diameter_are_never_merged() {
    // A plain shoulder: one stepped hole.
    assert_holes("grooved_bore_cbore", &[(27.2, holes::Bottom::Through, 3.5)]);
    // A ball groove between a d27.2 and a d28.0 land: two holes, neither given a floor.
    for placement in placements() {
        let found = holes_of("grooved_bore_steps", &placement);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(
            found.iter().map(|h| h.0).collect::<Vec<_>>(),
            vec![27.2, 28.0]
        );
        assert!(found.iter().all(|h| h.1 != holes::Bottom::Flat));
    }
}

#[test]
fn a_groove_wider_than_a_quarter_of_the_bore_is_not_merged() {
    // A torus R13.0 r4.0 (8 mm wide, against a quarter diameter of 6.8): two holes, and no
    // floor claimed on the groove.
    for placement in placements() {
        let found = holes_of("grooved_bore_wide", &placement);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            found
                .iter()
                .all(|h| h.0 == 27.2 && h.1 != holes::Bottom::Flat)
        );
    }
}

#[test]
fn a_torus_gallery_whose_major_radius_reaches_the_bore_keeps_its_through_ends() {
    // R8.0 r3.5 on a d16 bore: the end already read as open before the groove rule.
    assert_holes(
        "grooved_bore_valve",
        &[
            (16.0, holes::Bottom::Through, 16.5),
            (16.0, holes::Bottom::Through, 16.5),
        ],
    );
}

#[test]
fn a_slot_open_to_the_outside_is_not_an_internal_groove_at_any_wall_thickness() {
    // A 1 mm sawn slot across a d10 bore leaves a 0.7 mm bridge: two holes whatever the
    // outer radius.
    for fixture in ["grooved_bore_sawn_6_5", "grooved_bore_sawn_7_4"] {
        assert_holes(
            fixture,
            &[
                (10.0, holes::Bottom::Through, 9.5),
                (10.0, holes::Bottom::Through, 9.5),
            ],
        );
    }
}

#[test]
fn a_cross_hole_leaves_the_bore_through_and_whole() {
    assert_holes(
        "grooved_bore_cross",
        &[
            (3.0, holes::Bottom::Through, 60.0),
            (27.2, holes::Bottom::Through, 5.0),
        ],
    );
}
