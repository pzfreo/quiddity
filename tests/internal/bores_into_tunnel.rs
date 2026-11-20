//! Bores that run out into a curved tunnel are not flat-bottomed (quiddity#307).
use super::common;
use quiddity::features::{Context, holes};
use quiddity::framed_records::Rigid;
use quiddity::kernel::geom;
use quiddity::kernel::step::IDENTITY;

#[test]
fn bores_opening_into_a_tunnel_have_no_floor_but_real_floors_beside_it_stay() {
    let base =
        quiddity::read_step_file(&common::fixtures().join("bores_into_tunnel.step.gz")).unwrap();
    assert!(base.solid_is_geometrically_valid(0));
    let (s, c) = 0.37_f64.sin_cos();
    for placement in [
        IDENTITY,
        [[c, 0., s, 123.456], [0., 1., 0., -78.9], [-s, 0., c, 41.3]],
    ] {
        let motion = Rigid::from_placement(&placement);
        let part = base.transformed(&placement).unwrap();
        let found = holes::discover_verified(&Context::new(&part), &[]).unwrap();
        // The tunnel itself plus six bores.
        assert_eq!(found.len(), 7);
        // (a point on the bore axis, diameter, expected bottom)
        for (point, diameter, bottom) in [
            ([-90., 0., 30.], 16., holes::Bottom::Through), // ends in air at the tunnel axis
            ([-10., 0., 30.], 16., holes::Bottom::Through), // floor disc only a crescent
            ([30., 0., 30.], 9., holes::Bottom::Flat),
            ([70., 0., 54.], 9., holes::Bottom::Flat), // 2 mm above the tunnel crown
            ([110., 0., 30.], 9., holes::Bottom::Flat), // floor 28 mm short of the tunnel
        ] {
            let on_axis = motion.point(point);
            let hole = found
                .iter()
                .map(|o| &o.record)
                .find(|h| {
                    h.diameter == diameter
                        && geom::norm(geom::cross(geom::sub(*h.location.raw(), on_axis), h.axis))
                            < 1e-4
                })
                .unwrap_or_else(|| panic!("no d{diameter} bore through {point:?}"));
            assert_eq!(hole.bottom, bottom, "bore through {point:?}");
        }
        // Aimed 14 mm off the tunnel axis, the exit edge is too long to be read as an end:
        // the bore is not given a floor.
        let skew = motion.point([-50., -14., 30.]);
        let hole = found
            .iter()
            .map(|o| &o.record)
            .find(|h| {
                h.diameter == 16.
                    && geom::norm(geom::cross(geom::sub(*h.location.raw(), skew), h.axis)) < 1e-4
            })
            .unwrap();
        assert_ne!(hole.bottom, holes::Bottom::Flat);
    }
}

fn blind_hole_bottom(fixture: &str) -> holes::Bottom {
    let part = quiddity::read_step_file(&common::fixtures().join(fixture)).unwrap();
    let found = holes::discover_verified(&Context::new(&part), &[]).unwrap();
    found
        .iter()
        .map(|o| &o.record)
        .find(|h| h.diameter == 20. && h.axis == [0., 0., -1.])
        .expect("the d20 blind hole")
        .bottom
}

#[test]
fn a_floor_crossing_the_axis_stays_flat_and_one_missing_there_is_through() {
    // Two R8 passages at the floor's height leave a 6-wide strip of floor across the axis,
    // with material on the axis just past it.
    assert_eq!(
        blind_hole_bottom("blind_hole_floor_strip_between_passages.step.gz"),
        holes::Bottom::Flat
    );
    // A thin R3 cross hole crosses the axis under the floor: air on the axis, but 72% of
    // the floor remains, so the blind hole keeps its floor.
    assert_eq!(
        blind_hole_bottom("blind_hole_floor_under_small_cross_hole.step.gz"),
        holes::Bottom::Flat
    );
    // An R10 tunnel removes the floor under the axis and 30% of it: air on the axis past the end.
    assert_eq!(
        blind_hole_bottom("blind_hole_floor_removed_under_axis.step.gz"),
        holes::Bottom::Through
    );
}
