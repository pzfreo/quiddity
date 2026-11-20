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
