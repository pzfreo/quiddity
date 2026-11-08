//! Independent planar-solid construction: a pocket with a filled floor/wall corner.
use quiddity::features::{self, Context, internal_bevels};
use quiddity::kernel::brep::Part;
use quiddity::kernel::geom::{self, V3};

pub(crate) fn pocket(leg1: f64, leg2: f64, draft: f64) -> Part {
    pocket_section(&[[0., leg2], [leg1, 0.], [10., 0.], [10., 20.], [draft, 20.]])
}

fn pocket_section(section: &[[f64; 2]]) -> Part {
    let draft = section.last().unwrap()[0];
    let outer = [[-10., -10.], [30., -10.], [30., 20.], [-10., 20.]];
    let far = section
        .iter()
        .map(|p| p[0])
        .fold(f64::NEG_INFINITY, f64::max);
    let inner = [[draft, 0.], [far, 0.], [far, 10.], [draft, 10.]];
    let mut polygons = Vec::new();
    polygons.push(
        outer
            .iter()
            .rev()
            .map(|&[x, y]| [x, y, -10.])
            .collect::<Vec<_>>(),
    );
    for i in 0..4 {
        let j = (i + 1) % 4;
        let [x, y] = outer[i];
        let [u, v] = outer[j];
        polygons.push(vec![[x, y, -10.], [u, v, -10.], [u, v, 20.], [x, y, 20.]]);
        let [a, b] = inner[i];
        let [c, d] = inner[j];
        polygons.push(vec![[x, y, 20.], [u, v, 20.], [c, d, 20.], [a, b, 20.]]);
    }
    // Cavity section, counterclockwise in x,z. Reverse its walls toward the cavity.
    polygons.push(section.iter().map(|&[x, z]| [x, 10., z]).collect());
    polygons.push(section.iter().rev().map(|&[x, z]| [x, 0., z]).collect());
    for i in 0..section.len() {
        let j = (i + 1) % section.len();
        if section[i][1] == 20. && section[j][1] == 20. {
            continue;
        }
        let [x, z] = section[i];
        let [u, w] = section[j];
        polygons.push(vec![[x, 0., z], [u, 0., w], [u, 10., w], [x, 10., z]]);
    }
    let mut points = Vec::new();
    let faces = polygons
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
        .collect::<Vec<_>>();
    features::support_patches::polygon_part(&points, &faces, true).unwrap()
}

#[test]
fn filled_corner_has_true_legs_angle_and_only_the_strip_is_defining() {
    let part = pocket(3., 2., 0.);
    assert!(part.solid_is_geometrically_valid(0));
    assert!((part.solid_mass(0).unwrap().0 - 34030.).abs() < 1e-6);
    let found = internal_bevels::discover_verified(&Context::new(&part)).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    let bevel = &found[0];
    assert_eq!(bevel.defining, [11]);
    assert_eq!(bevel.context.len(), 4);
    let r = &bevel.record;
    assert_eq!((r.leg1, r.leg2, r.angle, r.length), (3., 2., 33.69, 10.));
    assert_eq!(*r.corner, [0., 5., 0.]);
    assert_eq!(*r.at, [1.5, 5., 1.]);
    assert_eq!(r.run, [0., 1., 0.]);
    assert_eq!(r.leg1_direction, [1., 0., 0.]);
    assert_eq!(r.leg2_direction, [0., 0., 1.]);
    let all = features::recognise(&part);
    assert_eq!(all.internal_bevels, vec![r.clone()]);
    let view = quiddity::build_recognition_evidence(&part).unwrap();
    assert!(
        view.features
            .iter()
            .any(|f| f.family == "internal_bevels" && f.defining == [11])
    );
}

#[test]
fn ambiguity_draft_and_large_sloped_wall_are_refused() {
    // Tilting the adjacent wall destroys perpendicular corner evidence. A broad slope
    // consumes the supporting floor and wall rather than treating their internal edge.
    for part in [
        pocket(3., 2., 1.),
        pocket(9., 18., 0.),
        pocket(3., 0.01, 0.),
    ] {
        assert!(part.solid_is_geometrically_valid(0));
        assert!(
            internal_bevels::discover_verified(&Context::new(&part))
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn lookalikes_and_convex_chamfers_keep_their_families() {
    let fixtures = super::common::fixtures();
    for name in [
        "golden_plates_pads_levels_and_slanted_steps.step",
        "golden_oblique_through_step.step",
        "prismatic.step",
        "golden_counterbored_and_countersunk_holes.step",
        "golden_oriented_chamfer.step",
    ] {
        let part = quiddity::read_step_file(&fixtures.join(name)).unwrap();
        assert!(
            internal_bevels::discover_verified(&Context::new(&part))
                .unwrap()
                .is_empty(),
            "{name}"
        );
    }
    let part =
        quiddity::read_step_file(&fixtures.join("golden_chamfers_fillets_and_flats.step")).unwrap();
    let ctx = Context::new(&part);
    assert_eq!(
        features::chamfers::discover_verified(&ctx, &Default::default())
            .unwrap()
            .len(),
        4
    );
}

#[test]
fn rigid_motion_preserves_legs_faces_and_publication() {
    let part = pocket(3., 2., 0.);
    let (s, c) = 0.37_f64.sin_cos();
    let matrix = [[c, 0., s, 123.456], [0., 1., 0., -78.9], [-s, 0., c, 41.3]];
    let moved = part.transformed(&matrix).unwrap();
    for shape in [&part, &moved] {
        let recognized = quiddity::correspondence::recognise(shape);
        let doc = quiddity::recognition::document(&recognized, shape.faces.len());
        let records = doc
            .records
            .iter()
            .filter(|r| r.family == "internal_bevels")
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].faces, [11]);
        assert_eq!(records[0].parameters["leg1"], 3.);
        assert_eq!(records[0].parameters["leg2"], 2.);
        assert_eq!(records[0].parameters["angle"], 33.69);
    }
    let a = internal_bevels::discover_verified(&Context::new(&part)).unwrap();
    let b = internal_bevels::discover_verified(&Context::new(&moved)).unwrap();
    let motion = quiddity::framed_records::Rigid::from_placement(&matrix);
    let close = |a: V3, b: V3| assert!(geom::norm(geom::sub(a, b)) < 0.001);
    close(*b[0].record.corner, motion.point(*a[0].record.corner));
    close(
        b[0].record.leg1_direction,
        motion.direction(a[0].record.leg1_direction),
    );
}

#[test]
fn sloped_pocket_floor_and_drafted_wall_lack_corner_supports() {
    for section in [
        vec![[0., 2.], [10., 0.], [10., 20.], [0., 20.]],
        vec![[0., 0.], [10., 0.], [10., 20.], [2., 20.]],
    ] {
        let part = pocket_section(&section);
        assert!(part.solid_is_geometrically_valid(0));
        assert!(
            internal_bevels::discover_verified(&Context::new(&part))
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn an_air_void_inside_the_virtual_wedge_invalidates_the_whole_treatment() {
    let part = pocket(3., 2., 0.);
    let points = [
        [0.2, 4., 0.2],
        [0.6, 4., 0.2],
        [0.6, 6., 0.2],
        [0.2, 6., 0.2],
        [0.2, 4., 0.4],
        [0.6, 4., 0.4],
        [0.6, 6., 0.4],
        [0.2, 6., 0.4],
    ];
    // Normals face the enclosed air cavity.
    let faces = vec![
        vec![0, 1, 2, 3],
        vec![4, 7, 6, 5],
        vec![0, 4, 5, 1],
        vec![1, 5, 6, 2],
        vec![2, 6, 7, 3],
        vec![3, 7, 4, 0],
    ];
    let void = features::support_patches::polygon_part(&points, &faces, true).unwrap();
    let mut edges = part.edges.clone();
    let offset = edges.len();
    let vertex_offset = edges
        .iter()
        .flat_map(|e| [e.vertices.0, e.vertices.1])
        .max()
        .unwrap()
        + 1;
    edges.extend(void.edges.into_iter().map(|mut e| {
        e.vertices.0 += vertex_offset;
        e.vertices.1 += vertex_offset;
        e
    }));
    let mut faces = part.faces.clone();
    faces.extend(void.faces.into_iter().map(|mut f| {
        for lp in &mut f.loops {
            for (e, _) in &mut lp.edges {
                *e += offset;
            }
        }
        f
    }));
    let count = faces.len();
    let hollow = Part::new(
        faces,
        edges,
        vec![quiddity::kernel::brep::Solid {
            faces: (0..count).collect(),
        }],
    );
    assert!(hollow.solid_is_geometrically_valid(0));
    assert!((hollow.solid_mass(0).unwrap().0 - 34029.84).abs() < 1e-6);
    assert!(
        internal_bevels::discover_verified(&Context::new(&hollow))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn equal_leg_repro_is_a_concave_treatment_of_a_bounded_pocket_corner() {
    // Same four-concave-join geometry as #213, constructed with independent dimensions.
    let part = pocket(2., 2., 0.);
    assert!(part.solid_is_geometrically_valid(0));
    assert!((part.solid_mass(0).unwrap().0 - 34020.).abs() < 1e-6);
    let ctx = Context::new(&part);
    let found = internal_bevels::discover_verified(&ctx).unwrap();
    assert_eq!(found.len(), 1);
    let bevel = &found[0];
    assert_eq!(bevel.defining, [11]);
    assert_eq!(
        (bevel.record.leg1, bevel.record.leg2, bevel.record.angle),
        (2., 2., 45.)
    );
    assert_eq!(bevel.record.length, 10.);
    assert_eq!(*bevel.record.corner, [0., 5., 0.]);
    assert!(
        bevel
            .context
            .iter()
            .all(|&f| part.arc(11, f) == Some(quiddity::kernel::brep::Arc::Concave))
    );
    assert!(
        features::chamfers::discover_verified(&ctx, &Default::default())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn equal_legs_follow_intrinsic_support_area_in_rotated_exports() {
    let part = pocket(2., 2., 0.);
    let source = internal_bevels::discover_verified(&Context::new(&part)).unwrap();
    assert_eq!(source.len(), 1);
    // The taller support is intrinsically larger (180 versus 80 mm²).
    assert_eq!(source[0].record.leg1_direction, [0., 0., 1.]);
    let original = features::recognise(&part);
    let before = quiddity::correspondence::fingerprint::fingerprint(&part, &original);
    for angle in [0.37_f64, std::f64::consts::FRAC_PI_2, 2.1] {
        let (s, c) = angle.sin_cos();
        let (t, d) = 0.61_f64.sin_cos();
        let matrix = [
            [c, -s, 0., 17.],
            [d * s, d * c, -t, -29.],
            [t * s, t * c, d, 43.],
        ];
        let motion = quiddity::framed_records::Rigid::from_placement(&matrix);
        let moved = part.transformed(&matrix).unwrap();
        let found = internal_bevels::discover_verified(&Context::new(&moved)).unwrap();
        assert_eq!(found.len(), 1);
        let a = &source[0].record;
        let b = &found[0].record;
        for (got, want) in [
            (b.leg1_direction, motion.direction(a.leg1_direction)),
            (b.leg2_direction, motion.direction(a.leg2_direction)),
        ] {
            assert!(
                geom::norm(geom::sub(got, want)) < 2e-6,
                "{angle}: {got:?} != {want:?}"
            );
        }
        assert_eq!((b.leg1, b.leg2, b.angle), (2., 2., 45.));
        let after = quiddity::correspondence::fingerprint::fingerprint(
            &moved,
            &features::recognise(&moved),
        );
        let matched = quiddity::correspondence::correspond(&before, &after).unwrap();
        let entry = matched
            .features
            .iter()
            .find(|f| f.old.starts_with("internal_bevels/"))
            .unwrap();
        assert_eq!(
            entry.class,
            quiddity::correspondence::Class::Carried,
            "{angle}: {entry:?}"
        );
        assert!(entry.changes.is_empty());
    }
}

#[test]
fn symmetric_equal_support_legs_are_an_unordered_fingerprint_pair() {
    let part = pocket_section(&[[0., 2.], [2., 0.], [20., 0.], [20., 20.], [0., 20.]]);
    assert!(part.solid_is_geometrically_valid(0));
    let mut found = features::recognise(&part);
    assert_eq!(found.internal_bevels.len(), 1);
    let before = quiddity::correspondence::fingerprint::fingerprint(&part, &found);
    let r = &mut found.internal_bevels[0];
    assert_eq!((r.leg1, r.leg2), (2., 2.));
    std::mem::swap(&mut r.leg1_direction, &mut r.leg2_direction);
    let after = quiddity::correspondence::fingerprint::fingerprint(&part, &found);
    let record = |f: quiddity::correspondence::Fingerprints| {
        f.features
            .into_iter()
            .find(|r| r.family == "internal_bevels")
            .unwrap()
    };
    let first = record(before.clone());
    let second = record(after);
    assert_eq!(first, second);
    assert_eq!(first.sizes["leg1"], 2.);
    assert_eq!(first.sizes["leg2"], 2.);
    assert_eq!(first.sizes["length"], 10.);
    assert_eq!(first.sizes["angle"], 45.);
    let support_areas: Vec<_> = part
        .neighbours(11)
        .into_iter()
        .filter(|&f| part.face_normal(f, 0., 0.).unwrap()[1].abs() < 0.01)
        .map(|f| part.face_mass(f).unwrap()[0])
        .collect();
    assert_eq!(support_areas.len(), 2);
    assert!((support_areas[0] - support_areas[1]).abs() < 1e-6);
    // A rotated re-export may also reverse the two indistinguishable support slots.
    let matrix = [[0., 0., 1., 17.], [0., 1., 0., -29.], [-1., 0., 0., 43.]];
    let moved = part.transformed(&matrix).unwrap();
    let mut exported = features::recognise(&moved);
    assert_eq!(exported.internal_bevels.len(), 1);
    let r = &mut exported.internal_bevels[0];
    std::mem::swap(&mut r.leg1_direction, &mut r.leg2_direction);
    let rotated = quiddity::correspondence::fingerprint::fingerprint(&moved, &exported);
    let matched = quiddity::correspondence::correspond(&before, &rotated).unwrap();
    let entry = matched.features.iter().find(|f| f.old == first.id).unwrap();
    assert_eq!(
        entry.class,
        quiddity::correspondence::Class::Carried,
        "{entry:?}"
    );
    assert!(entry.changes.is_empty());
}

#[test]
fn step_angular_noise_keeps_the_actual_support_legs_and_slope_angle() {
    // A 0.159° wall departure is within AXIS_ZERO_COS, but far outside 1e-9.
    // The bevel and floor themselves have not moved: their slope remains atan2(2,3).
    let part = pocket(3., 2., 0.05);
    assert!(part.solid_is_geometrically_valid(0));
    let found = internal_bevels::discover_verified(&Context::new(&part)).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    let r = &found[0].record;
    assert_eq!((r.leg1, r.leg2, r.angle), (3.006, 2., 33.69));
    assert_eq!(*r.corner, [-0.006, 5., 0.]);
    // atan2 of the two lengths would misreport 33.64° on these real supports.
    assert_eq!(
        quiddity::kernel::py::round_to(r.leg2.atan2(r.leg1).to_degrees(), 2),
        33.64
    );
}
