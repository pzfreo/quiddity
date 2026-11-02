//! Structural evidence from an original analytic fixture; no snapshot hashes.
use super::common;
use quiddity::features::{Context, Features, hole_patterns, holes};
use quiddity::framed_records::{Rigid, map_features};
use quiddity::kernel::geom::{self, Surface};
use quiddity::kernel::step::IDENTITY;

#[test]
fn partial_hole_exits_are_through_but_full_caps_and_drill_points_stay_blind() {
    let base =
        quiddity::read_step_file(&common::fixtures().join("partial_hole_exits.step.gz")).unwrap();
    assert_eq!(base.solids.len(), 1);
    assert!(base.solid_is_geometrically_valid(0));
    let (s, c) = 0.37_f64.sin_cos();
    for placement in [
        IDENTITY,
        [[c, 0., s, 123.456], [0., 1., 0., -78.9], [-s, 0., c, 41.3]],
        [[0., 0., -1., 11.1], [1., 0., 0., 22.2], [0., -1., 0., 33.3]],
    ] {
        let motion = Rigid::from_placement(&placement);
        let part = base.transformed(&placement).unwrap();
        let found = holes::discover_verified(&Context::new(&part), &[]).unwrap();
        assert_eq!(found.len(), 6);
        for (x, bottom, count, fraction, depth) in [
            (10., holes::Bottom::Through, 0, 0., 8.),
            (30., holes::Bottom::Through, 1, 0.292, 8.),
            (50., holes::Bottom::Through, 1, 0.5, 8.),
            (70., holes::Bottom::Flat, 0, 0., 8.),
            (90., holes::Bottom::DrillPoint, 0, 0., 6.),
            (110., holes::Bottom::Through, 2, 0.292, 8.),
        ] {
            let on_axis = motion.point([x, 0., 0.]);
            let axis = motion.direction([0., 0., 1.]);
            let occurrence = found
                .iter()
                .find(|o| {
                    geom::norm(geom::cross(
                        geom::sub(*o.record.location.raw(), on_axis),
                        axis,
                    )) < 1e-6
                })
                .unwrap();
            let h = &occurrence.record;
            assert_eq!(
                (h.diameter, h.depth, h.bottom),
                (6., depth, bottom),
                "x={x}"
            );
            assert_eq!(h.exit_obstructions.len(), count, "x={x}");
            for obstruction in &h.exit_obstructions {
                assert!((obstruction.covered_fraction - fraction).abs() < 0.01);
                assert!(!obstruction.faces.is_empty());
                assert!(
                    geom::norm(geom::cross(
                        geom::sub(*obstruction.location.raw(), on_axis),
                        axis
                    )) < 1e-6
                );
                let axial = geom::dot(geom::sub(*obstruction.location.raw(), on_axis), axis);
                assert!((axial - 8.).abs() < 1e-6 || (x == 110. && axial.abs() < 1e-6));
                for &face in &obstruction.faces {
                    assert!(matches!(part.faces[face].surface, Surface::Plane { .. }));
                    assert!(occurrence.context.contains(&face));
                    assert!(!occurrence.defining.contains(&face));
                    let normal = part.face_normal(face, 0., 0.).unwrap();
                    assert!(geom::dot(normal, axis).abs() > 0.999);
                }
            }
            let json = serde_json::to_value(h).unwrap();
            assert_eq!(json.get("exit_obstructions").is_some(), count > 0);
            assert_eq!(
                serde_json::from_value::<holes::HoleRecord>(json.clone()).unwrap(),
                *h
            );
            let mut legacy = json;
            legacy.as_object_mut().unwrap().remove("exit_obstructions");
            assert!(
                serde_json::from_value::<holes::HoleRecord>(legacy)
                    .unwrap()
                    .exit_obstructions
                    .is_empty()
            );
        }
    }
}

#[test]
fn partial_hole_warnings_follow_frame_mapping_and_pattern_members() {
    let part =
        quiddity::read_step_file(&common::fixtures().join("partial_hole_exits.step.gz")).unwrap();
    let found = holes::discover_verified(&Context::new(&part), &[]).unwrap();
    let hole = found
        .iter()
        .find(|h| h.record.location[0] == 30.)
        .unwrap()
        .record
        .clone();
    let holes: Vec<_> = (0..3)
        .map(|i| {
            let mut h = hole.clone();
            h.location = [30., i as f64 * 20., 0.].into();
            h.exit_obstructions[0].location = [30., i as f64 * 20., 8.].into();
            h.exit_obstructions[0].covered_fraction = 0.2 + i as f64 * 0.1;
            h
        })
        .collect();
    let patterns = hole_patterns::recognise_hole_patterns(&holes);
    assert_eq!(patterns.len(), 1, "warnings do not split machining specs");
    let motion =
        Rigid::from_placement(&[[0., 0., 1., 123.], [1., 0., 0., -45.], [0., 1., 0., 67.]]);
    let mapped = map_features(
        Features {
            holes: holes.clone(),
            hole_patterns: patterns,
            ..Features::default()
        },
        &motion,
    );
    let json = serde_json::to_value(&mapped.features).unwrap();
    for i in 0..3 {
        let expected = motion.point(*holes[i].exit_obstructions[0].location.raw());
        for value in [&json["holes"][i], &json["hole_patterns"][0]["holes"][i]] {
            let obstruction = &value["exit_obstructions"][0];
            assert_eq!(obstruction["location"], serde_json::json!(expected));
            assert_eq!(
                obstruction["faces"],
                serde_json::json!(hole.exit_obstructions[0].faces)
            );
            assert_eq!(
                obstruction["covered_fraction"],
                serde_json::json!(0.2 + i as f64 * 0.1)
            );
        }
    }
}

#[test]
fn partial_hole_exits_without_an_owning_solid_stay_unknown() {
    let base =
        quiddity::read_step_file(&common::fixtures().join("partial_hole_exits.step.gz")).unwrap();
    let mut faces = base.faces.clone();
    for face in &mut faces {
        face.solid = None;
    }
    let shell = quiddity::Part::new(faces, base.edges.clone(), Vec::new());
    let found = holes::recognise_holes(&shell, &holes::HoleOptions::default());
    for x in [30., 50., 110.] {
        let h = found.iter().find(|h| h.location[0] == x).unwrap();
        assert_eq!(h.bottom, holes::Bottom::Unknown);
        assert!(h.exit_obstructions.is_empty());
    }
}
