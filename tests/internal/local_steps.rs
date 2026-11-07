//! Original analytic boolean controls for local-run steps; no dataset inputs are shipped.
use super::common;
use quiddity::features::{self, Context, local_steps, through_steps};
use quiddity::framed_records::{Rigid, map_features};
use quiddity::kernel::geom::{self, V3};

fn control(name: &str) -> quiddity::Part {
    quiddity::read_step_file(&common::fixtures().join(format!("local_steps/{name}.step.gz")))
        .unwrap()
}

fn close(a: V3, b: V3) {
    assert!(geom::norm(geom::sub(a, b)) < 0.001, "{a:?} != {b:?}");
}

#[test]
fn local_run_has_two_defining_walls_and_proved_end_faces() {
    for (name, lo, hi, scopes) in [
        ("local", 0., 4., ["solid", "local"]),
        ("both_ends", 10., 20., ["local", "local"]),
    ] {
        let part = control(name);
        assert!(part.solid_is_geometrically_valid(0));
        let volume = part.solid_mass(0).unwrap().0;
        let expected = if name == "local" { 120360. } else { 120900. };
        assert!((volume - expected).abs() < 1e-6, "{name}: {volume}");
        let found = local_steps::discover_verified(&Context::new(&part)).unwrap();
        assert_eq!(found.len(), 1, "{name}: {found:?}");
        let step = &found[0];
        assert_eq!(step.defining.len(), 2);
        assert_eq!(step.record.run_direction, [0., 1., 0.]);
        close(*step.record.run_start, [25., lo, 68.]);
        close(*step.record.run_end, [25., hi, 68.]);
        assert_eq!(step.record.length, hi - lo);
        assert_eq!((step.record.height, step.record.depth), (2., 5.));
        assert_eq!(step.record.height_direction, [0., 0., 1.]);
        assert_eq!(step.record.depth_direction, [1., 0., 0.]);
        assert_eq!(step.record.ends.each_ref().map(|e| e.scope), scopes);
        for (end, station) in step.record.ends.iter().zip([lo, hi]) {
            assert_eq!(end.faces.len(), 1);
            for &f in &end.faces {
                assert!(step.context.contains(&f));
                assert!(!step.defining.contains(&f));
                let b = part.face_bounds(f);
                assert!((b.min[1] - station).abs() < 1e-6 && (b.max[1] - station).abs() < 1e-6);
                for &wall in &step.defining {
                    assert_eq!(part.arc(wall, f), Some(quiddity::kernel::brep::Arc::Convex));
                }
            }
        }
        let all = features::recognise(&part);
        assert_eq!(all.local_steps, vec![step.record.clone()]);
        assert_eq!(all.defining["local_steps"], vec![step.defining.clone()]);
        assert!(
            through_steps::discover_verified(&Context::new(&part))
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn ambiguous_oblique_end_and_concave_blind_wall_are_refused() {
    for name in ["oblique_end", "blind_end"] {
        let part = control(name);
        assert!(part.solid_is_geometrically_valid(0));
        assert!(
            local_steps::discover_verified(&Context::new(&part))
                .unwrap()
                .is_empty(),
            "{name}"
        );
    }
}

#[test]
fn full_length_step_stays_in_its_original_family() {
    let part = control("full");
    assert!(
        local_steps::discover_verified(&Context::new(&part))
            .unwrap()
            .is_empty()
    );
    let steps = through_steps::discover_verified(&Context::new(&part)).unwrap();
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].record.axis, 'y');
    assert_eq!(steps[0].record.length, 60.);
    assert_eq!(
        steps[0].record.section.map(|p| *p),
        [[25., 70.], [25., 68.], [30., 68.]]
    );
    assert_eq!(steps[0].record.endpoint_scopes, ["solid", "solid"]);
    let original =
        quiddity::read_step_file(&common::fixtures().join("golden_rectangular_through_step.step"))
            .unwrap();
    assert!(
        local_steps::discover_verified(&Context::new(&original))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn local_step_publication_and_fingerprints_follow_rigid_motion() {
    let part = control("local");
    let features = features::recognise(&part);
    let source = &features.local_steps[0];
    let (s, c) = 0.37_f64.sin_cos();
    let motion =
        Rigid::from_placement(&[[c, 0., s, 123.456], [0., 1., 0., -78.9], [-s, 0., c, 41.3]]);
    let mapped = map_features(features.clone(), &motion).features;
    let step = &mapped.local_steps[0];
    close(*step.run_start, motion.point(*source.run_start));
    close(*step.run_end, motion.point(*source.run_end));
    close(
        step.height_direction,
        motion.direction(source.height_direction),
    );
    close(
        step.depth_direction,
        motion.direction(source.depth_direction),
    );
    assert_eq!(step.ends, source.ends);
    assert_eq!((step.length, step.height, step.depth), (4., 2., 5.));
    // Recognition itself, including frame inference, must retain the same defining walls.
    let moved = part
        .transformed(&[[c, 0., s, 123.456], [0., 1., 0., -78.9], [-s, 0., c, 41.3]])
        .unwrap();
    for shape in [&part, &moved] {
        let recognized = quiddity::correspondence::recognise(shape);
        let doc = quiddity::recognition::document(&recognized, shape.faces.len());
        let locals: Vec<_> = doc
            .records
            .iter()
            .filter(|r| r.family == "local_steps")
            .collect();
        assert_eq!(locals.len(), 1);
        assert_eq!(locals[0].faces, features.defining["local_steps"][0]);
        assert_eq!(locals[0].parameters["length"], 4.);
    }
    let before = quiddity::correspondence::fingerprint::fingerprint(&part, &features);
    let after = quiddity::correspondence::fingerprint::published_fingerprint(&part, &mapped);
    let first = before
        .features
        .iter()
        .find(|f| f.family == "local_steps")
        .unwrap();
    let next = after
        .features
        .iter()
        .find(|f| f.family == "local_steps")
        .unwrap();
    assert_eq!(first.sizes, next.sizes);
    assert_eq!(first.faces, next.faces);
    assert_eq!(first.traits, next.traits);
}
