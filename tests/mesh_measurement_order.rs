//! A placed trim next to the file uncertainty can round to either side in caller space.
use haecceity::{Part, geom, step::read_step_placed};

#[test]
fn mesh_and_measurement_order_preserves_the_placed_boundary_and_recognition() {
    let square = include_str!("../crates/haecceity/tests/fixtures/master-trim-square.step");
    let placement = [
        [0.8, -0.6, 0., 100000.],
        [0.6, 0.8, 0., -100000.],
        [0., 0., 1., 5.],
    ];
    for (offset, selected) in [("0.00100099999999", true), ("0.00100100000001", false)] {
        let text = square
            .replace(
                "#112=CARTESIAN_POINT('',(0.,0.))",
                &format!("#112=CARTESIAN_POINT('',(0.,{offset}))"),
            )
            .replace(
                "#114=CARTESIAN_POINT('',(1.,0.))",
                &format!("#114=CARTESIAN_POINT('',(1.,{offset}))"),
            )
            .replace("(0.5,0.0004)", "(0.5,0.08)");
        let fresh = || read_step_placed(text.as_bytes(), &placement).unwrap();
        let measure = |part: &Part| {
            let moments = part.face_moments(0).unwrap();
            let length = part
                .measured_boundary_length(0, |e| {
                    let edge = &part.edges[e];
                    geom::dist(edge.start, edge.end)
                })
                .unwrap();
            (moments, length)
        };
        let a = fresh();
        let a_mesh = a.mesh(0.001, 0.3);
        let a_measure = measure(&a);
        let b = fresh();
        let b_measure = measure(&b);
        let b_mesh = b.mesh(0.001, 0.3);
        assert_eq!(a_measure, b_measure);
        assert_eq!(a_mesh, b_mesh);
        assert!(a_mesh[0].is_ok());
        assert_eq!(a_measure.1 > 4.001, selected);
        let recognise = |part| quiddity::file_api::recognise_part(part, "synthetic").unwrap();
        assert_eq!(recognise(&a), recognise(&b));
    }
}
