//! Original polyhedron controls, assembled with the repository's planar-solid generator.
use quiddity::features::{self, Context, oblique_through_steps, polygonal_oblique_through_steps};
use quiddity::kernel::brep::Part;
use quiddity::kernel::geom::{norm, sub};

/// A slanted corner cut in a block, with a secondary cut taking the lower half of
/// its exterior lip. The principal wall has six corners; its oblique neighbour is
/// intact, ending at a local convex lip (y=30) inside the stock envelope (y=50).
/// No external dataset coordinates or files are used.
pub(crate) fn polygonal_corner(skew_lip: bool) -> Part {
    corner(skew_lip, false)
}

fn corner(skew_lip: bool, tapered: bool) -> Part {
    corner_with_shift(skew_lip, tapered, 2.0)
}

fn corner_with_shift(skew_lip: bool, tapered: bool, seam_shift: f64) -> Part {
    let k = if skew_lip { 3.0 } else { 2.0 };
    let polygons = vec![
        vec![
            [2., 10., 0.],
            [5., 10., 0.],
            [5., 30., 0.],
            [15., 30., 0.],
            [15., 50., 0.],
            [20., 50., 0.],
            [20., 0., 0.],
            [2., 0., 0.],
        ],
        vec![
            [0., 0., 40.],
            [20., 0., 40.],
            [20., 50., 40.],
            [15., 50., 40.],
            [15., 30., 40.],
            [7., 30., 40.],
            [7., 10., 40.],
            [0., 10., 40.],
        ],
        vec![
            [2., 0., 0.],
            [20., 0., 0.],
            [20., 0., 40.],
            [0., 0., 40.],
            [0., 0., 20.],
            [2., 0., 20.],
        ],
        vec![
            [20., 0., 0.],
            [20., 50., 0.],
            [20., 50., 40.],
            [20., 0., 40.],
        ],
        vec![
            [15., 30., 0.],
            [5., 30., 0.],
            [7., 30., 40.],
            [15., 30., 40.],
        ],
        vec![[5., 30., 0.], [5., 10., 0.], [7., 10., 40.], [7., 30., 40.]],
        vec![
            [5., 10., 0.],
            [k, 10., 0.],
            [2., 10., 20.],
            [0., 10., 20.],
            [0., 10., 40.],
            [7., 10., 40.],
        ],
        vec![[2., 0., 0.], [2., 0., 20.], [2., 10., 20.], [k, 10., 0.]],
        vec![[0., 0., 20.], [0., 0., 40.], [0., 10., 40.], [0., 10., 20.]],
        vec![[0., 0., 20.], [0., 10., 20.], [2., 10., 20.], [2., 0., 20.]],
        vec![
            [20., 50., 0.],
            [15., 50., 0.],
            [15., 50., 40.],
            [20., 50., 40.],
        ],
        vec![
            [15., 50., 0.],
            [15., 30., 0.],
            [15., 30., 40.],
            [15., 50., 40.],
        ],
    ];
    // The negative uses a sloped outside edge; maintain a closed planar solid.
    let mut polygons = polygons;
    let wall_end = 5.0 + seam_shift;
    let right = 20.0_f64.max(wall_end + 10.0);
    for ring in &mut polygons {
        for point in ring {
            point[0] = match point[0] {
                7.0 => wall_end,
                15.0 => right - 5.0,
                20.0 => right,
                x => x,
            };
        }
    }
    if skew_lip {
        polygons[0][0] = [k, 10., 0.];
        polygons[0][7] = [k, 0., 0.];
        polygons[2][0] = [k, 0., 0.];
        polygons[7][0] = [k, 0., 0.];
    }
    if tapered {
        for ring in &mut polygons {
            for p in ring {
                if p[1] == 30.0 {
                    p[1] += 0.1 * p[2];
                }
            }
        }
    }
    let mut points = Vec::new();
    let faces: Vec<Vec<usize>> = polygons
        .iter()
        .map(|ring| {
            ring.iter()
                .map(|&p| {
                    if let Some(i) = points.iter().position(|q| *q == p) {
                        i
                    } else {
                        points.push(p);
                        points.len() - 1
                    }
                })
                .collect()
        })
        .collect();
    features::support_patches::polygon_part(&points, &faces, true).expect("original polygon solid")
}

#[test]
fn six_corner_step_preserves_outline_and_measures_the_complete_walls() {
    let part = polygonal_corner(false);
    assert!(part.solid_is_geometrically_valid(0));
    assert!((part.solid_mass(0).unwrap().0 - 22800.0).abs() < 1e-6);
    assert_eq!(part.solid_bounds(0).max[1], 50.0);
    let found = polygonal_oblique_through_steps::discover_verified(&Context::new(&part)).unwrap();
    assert_eq!(found.len(), 1);
    let step = &found[0];
    assert_eq!(step.defining, vec![6, 5]);
    assert_eq!(step.context, vec![0, 1, 4, 7, 8, 9]);
    assert_eq!(step.record.floor_outline.len(), 6);
    assert_eq!(step.record.length, 40.05);
    assert_eq!(step.record.depth, 20.);
    assert_eq!(step.record.through_axis, [0., 0., 1.]);
    assert_eq!(*step.record.at, [6., 10., 20.]);
    assert!((step.record.slant_degrees - 0.05_f64.atan().to_degrees()).abs() < 1e-6);
    assert!(norm(sub(step.record.run, [0.049938, 0., 0.998752])) < 1e-6);
    assert!(oblique_through_steps::discover(&Context::new(&part)).is_empty());
    let all = features::recognise(&part);
    assert_eq!(
        all.polygonal_oblique_through_steps,
        vec![step.record.clone()]
    );
    assert_eq!(
        all.defining["polygonal_oblique_through_steps"],
        vec![vec![6, 5]]
    );
    // Six published corners reconstruct the actual six-sided boundary, not its hull.
    let r = &step.record;
    let start =
        quiddity::kernel::geom::sub(*r.at, quiddity::kernel::geom::scale(r.run, r.length / 2.0));
    let expected = [
        [5., 10., 0.],
        [2., 10., 0.],
        [2., 10., 20.],
        [0., 10., 20.],
        [0., 10., 40.],
        [7., 10., 40.],
    ];
    for corner in &r.floor_outline {
        let point = quiddity::kernel::geom::add(
            start,
            quiddity::kernel::geom::add(
                quiddity::kernel::geom::scale(r.run, corner[0]),
                quiddity::kernel::geom::scale(r.across_direction, corner[1]),
            ),
        );
        assert!(expected.iter().any(|&v| norm(sub(v, point)) < 0.002));
    }
}

#[test]
fn skew_polygon_boundary_is_refused() {
    let part = polygonal_corner(true);
    assert!(part.solid_is_valid(0));
    assert!(polygonal_oblique_through_steps::discover(&Context::new(&part)).is_empty());
}

#[test]
fn four_corner_step_stays_exclusively_in_the_existing_family() {
    let part = quiddity::read_step_file(
        &super::common::fixtures().join("golden_oblique_through_step.step"),
    )
    .unwrap();
    let evidence = quiddity::evidence_view::build_recognition_evidence(&part).unwrap();
    assert!(
        !evidence
            .features
            .iter()
            .any(|f| f.family == "polygonal_oblique_through_steps")
    );
    let old = oblique_through_steps::discover_verified(&Context::new(&part)).unwrap();
    assert_eq!(old.len(), 1);
    assert!(polygonal_oblique_through_steps::discover(&Context::new(&part)).is_empty());
    let all = features::recognise(&part);
    assert_eq!(all.oblique_through_steps, vec![old[0].record.clone()]);
    assert!(all.polygonal_oblique_through_steps.is_empty());
}

#[test]
fn polygonal_record_and_evidence_follow_rigid_motion() {
    use quiddity::framed_records::{Rigid, map_features};
    let part = polygonal_corner(false);
    let features = features::recognise(&part);
    let source = &features.polygonal_oblique_through_steps[0];
    let (s, c) = 0.37_f64.sin_cos();
    let matrix = [[c, 0., s, 123.456], [0., 1., 0., -78.9], [-s, 0., c, 41.3]];
    let motion = Rigid::from_placement(&matrix);
    let mapped = map_features(features.clone(), &motion).features;
    let step = &mapped.polygonal_oblique_through_steps[0];
    assert!(norm(sub(*step.at, motion.point(*source.at))) < 0.001);
    for (a, b) in [
        (step.run, motion.direction(source.run)),
        (step.through_axis, motion.direction(source.through_axis)),
        (
            step.depth_direction,
            motion.direction(source.depth_direction),
        ),
        (
            step.across_direction,
            motion.direction(source.across_direction),
        ),
    ] {
        assert!(norm(sub(a, b)) < 1e-6);
    }
    assert_eq!(step.floor_outline, source.floor_outline);
    assert_eq!(
        (step.length, step.depth, step.slant_degrees),
        (source.length, source.depth, source.slant_degrees)
    );
    let before = quiddity::correspondence::fingerprint::fingerprint(&part, &features);
    let after = quiddity::correspondence::fingerprint::published_fingerprint(&part, &mapped);
    let first = before
        .features
        .iter()
        .find(|f| f.family == "polygonal_oblique_through_steps")
        .unwrap();
    let next = after
        .features
        .iter()
        .find(|f| f.family == first.family)
        .unwrap();
    assert_eq!(first.sizes, next.sizes);
    assert_eq!(first.faces, next.faces);
    let evidence = quiddity::evidence_view::build_recognition_evidence(&part).unwrap();
    let proof = evidence
        .features
        .iter()
        .find(|f| f.family == first.family)
        .unwrap();
    assert_eq!(proof.defining, vec![5, 6]);
    assert_eq!(proof.constituent, proof.defining);
    // The framed recognition entry point must retain the same defining walls.
    let moved = part.transformed(&matrix).unwrap();
    for shape in [&part, &moved] {
        let recognized = quiddity::correspondence::recognise(shape);
        let doc = quiddity::recognition::document(&recognized, shape.faces.len());
        let records: Vec<_> = doc
            .records
            .iter()
            .filter(|r| r.family == first.family)
            .collect();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].faces, vec![5, 6]);
        assert_eq!(records[0].parameters["length"], 40.05);
        assert_eq!(records[0].parameters["depth"], 20.);
        assert!(records[0].parameters.get("floor_outline").is_some());
        assert!(records[0].parameters.get("wall_outline").is_none());
        assert!(records[0].parameters.get("through_axis").is_some());
    }
}

#[test]
fn tapered_oblique_wall_does_not_supply_one_step_height() {
    let part = corner(false, true);
    assert!(part.solid_is_geometrically_valid(0));
    assert!(polygonal_oblique_through_steps::discover(&Context::new(&part)).is_empty());
}

#[test]
fn sixty_degree_seam_is_measured_against_the_x_through_axis() {
    // Expand the stock to keep both seam endpoints inside its transverse bounds.
    // Then rotate the original z-through control into an x-through step.
    let part = corner_with_shift(false, false, 40.0 * 3.0_f64.sqrt())
        .transformed(&[[0., 0., 1., 0.], [0., 1., 0., 0.], [-1., 0., 0., 0.]])
        .unwrap();
    assert!(part.solid_is_geometrically_valid(0));
    let found = polygonal_oblique_through_steps::discover_verified(&Context::new(&part)).unwrap();
    assert_eq!(found.len(), 1);
    let step = &found[0];
    assert_eq!(step.defining, vec![6, 5]);
    assert_eq!(step.record.through_axis, [1., 0., 0.]);
    assert_eq!(step.record.slant_degrees, 60.0);
    assert_eq!(step.record.length, 80.0);
    assert_eq!(step.record.depth, 20.0);
    assert_eq!(step.record.floor_outline.len(), 6);
    assert!(norm(sub(step.record.run, [0.5, 0., -3.0_f64.sqrt() / 2.0])) < 1e-6);
    let payload = serde_json::to_value(&step.record).unwrap();
    assert!(payload.get("wall_outline").is_none());
    assert_eq!(payload["floor_outline"].as_array().unwrap().len(), 6);
    assert_eq!(payload["through_axis"], serde_json::json!([1., 0., 0.]));
}
