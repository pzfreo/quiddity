//! For #134: treatments preserve supporting features; real dimension/footprint edits do not.
use super::common;
use quiddity::features::{Context, bosses, plates};
use quiddity::kernel::geom::{self, Surface};

fn part(name: &str) -> quiddity::Part {
    quiddity::read_step_file(&common::fixtures().join(name)).unwrap()
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-6, "{a} != {b}");
}

#[test]
fn gramil_end_chamfer_preserves_boss_height_and_plate_but_shortening_does_not() {
    for (name, height, lo, hi) in [
        ("old", 37.6, 33.6, 37.6),
        ("new", 37.6, 33.6, 37.6),
        ("shorter", 35.6, 31.6, 35.6),
    ] {
        let p = part(&format!("revisions/gramil_shaft.{name}.step.gz"));
        let ctx = Context::new(&p);
        let b = bosses::discover_verified(&ctx).unwrap();
        assert_eq!(b.len(), 1);
        near(b[0].record.height, height);
        let plates = plates::discover_verified(&ctx, &Default::default()).unwrap();
        assert_eq!(plates.len(), 1);
        near(*plates[0].record.lo, lo);
        near(*plates[0].record.hi, hi);
        near(plates[0].record.v, 0.190838914);
        // For #134: reusing a cone's projection integration in framed fingerprinting
        // must preserve the physical face area and centroid in file coordinates.
        let reading = quiddity::correspondence::recognise(&p);
        for (f, face) in p.faces.iter().enumerate() {
            if matches!(face.surface, Surface::Cone { .. }) {
                let measured = p.face_moments(f).unwrap();
                near(reading.fingerprints.faces[f].area.unwrap(), measured.area);
                near(
                    geom::dist(reading.fingerprints.faces[f].centroid, measured.centroid),
                    0.,
                );
            }
        }
    }
}

#[test]
fn boss_rounds_include_their_axial_extent_even_below_the_fillet_reporting_floor() {
    for name in ["boss_end_round.step.gz", "boss_end_small_round.step.gz"] {
        let p = part(name);
        for motion in [
            [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
            [[0., 0., -1., 17.], [1., 0., 0., -23.], [0., -1., 0., 9.]],
        ] {
            let p = p.transformed(&motion).unwrap();
            let ctx = Context::new(&p);
            let b = bosses::discover_verified(&ctx).unwrap();
            assert_eq!(b.len(), 1);
            near(b[0].record.height, 20.);
            let end = motion.map(|r| r[2] * 24. + r[3]);
            near(geom::dist(b[0].record.location, end), 0.);
            assert!(
                b[0].context
                    .iter()
                    .any(|&f| matches!(p.faces[f].surface, Surface::Torus { .. }))
            );
        }
    }
    let p = part("boss_end_sphere.step.gz");
    let b = bosses::discover_verified(&Context::new(&p)).unwrap();
    near(b[0].record.height, 25.);
    near(b[0].record.location[2], 29.);
    // Two chamfers on the original analytic disc, #134: 6.5 mm of cylinder + 2 × 0.25.
    let p = part("turned_body_disc.step");
    near(bosses::discover(&Context::new(&p))[0].record.height, 7.);
}

#[test]
fn tangent_top_round_height_is_independent_of_the_remaining_shaft_length() {
    use quiddity::kernel::brep::Arc;
    for height in [13, 8, 6, 4] {
        let p = part(&format!("top_round_boss{height}.step.gz"));
        for motion in [
            [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
            [[0., 0., -1., 17.], [1., 0., 0., -23.], [0., -1., 0., 9.]],
        ] {
            let p = p.transformed(&motion).unwrap();
            let ctx = Context::new(&p);
            let found = bosses::discover_verified(&ctx).unwrap();
            assert_eq!(found.len(), 1);
            let boss = &found[0];
            near(boss.record.diameter, 65.);
            near(boss.record.height, f64::from(height));
            let end = motion.map(|r| r[2] * f64::from(height) + r[3]);
            near(geom::dist(boss.record.location, end), 0.);
            let torus = p
                .faces
                .iter()
                .position(|f| matches!(f.surface, Surface::Torus { .. }))
                .unwrap();
            assert!(boss.context.contains(&torus));
            assert!(
                boss.defining
                    .iter()
                    .all(|&f| p.arc(f, torus) == Some(Arc::Smooth))
            );
            assert!(boss.context.iter().any(|&f| {
                matches!(p.faces[f].surface, Surface::Plane { .. })
                    && p.neighbours(torus).contains(&f)
            }));
            // Fingerprint placement must include the round even with only 1 mm of shaft.
            let middle = motion.map(|r| r[2] * f64::from(height) / 2. + r[3]);
            near(
                geom::dist(bosses::midpoint(&ctx, &boss.record, &boss.defining), middle),
                0.,
            );
        }
    }
}

#[test]
fn top_chamfer_keeps_its_taper_bound_and_a_non_tangent_torus_is_not_absorbed() {
    use quiddity::kernel::brep::Arc;
    let p = part("top_round_boss8_chamfer.step.gz");
    let found = bosses::discover_verified(&Context::new(&p)).unwrap();
    assert_eq!(found.len(), 1);
    near(found[0].record.height, 8.);
    assert!(
        found[0]
            .context
            .iter()
            .any(|&f| matches!(p.faces[f].surface, Surface::Cone { .. }))
    );

    let p = part("top_round_non_tangent.step.gz");
    let found = bosses::discover_verified(&Context::new(&p)).unwrap();
    assert_eq!(found.len(), 1);
    near(found[0].record.height, 5.);
    let torus = p
        .faces
        .iter()
        .position(|f| matches!(f.surface, Surface::Torus { .. }))
        .unwrap();
    assert_eq!(p.arc(found[0].defining[0], torus), Some(Arc::Convex));
    assert!(!found[0].context.contains(&torus));
}

#[test]
fn projected_round_strips_restore_plate_coverage_and_centres() {
    let p = part("revisions/q134_plate.old.step.gz");
    let rounded = part("plate_edge_round.step.gz");
    let get = |p: &quiddity::Part| {
        plates::discover_verified(&Context::new(p), &Default::default())
            .unwrap()
            .into_iter()
            .find(|o| o.record.axis == 'z')
            .unwrap()
    };
    let (a, b) = (get(&p), get(&rounded));
    near(*a.record.lo, *b.record.lo);
    near(*a.record.hi, *b.record.hi);
    near(a.record.u, b.record.u);
    near(a.record.v, b.record.v);
    assert!(
        b.context
            .iter()
            .any(|&f| matches!(rounded.faces[f].surface, Surface::Cylinder { .. }))
    );
    // Orthogonal projection of the R1.5 quarter torus is the annulus R5..R3.5.
    let p = part("boss_end_round.step.gz");
    let torus = p
        .faces
        .iter()
        .position(|f| matches!(f.surface, Surface::Torus { .. }))
        .unwrap();
    let (a, c) = p.face_projected_moments(torus, [0., 0., 1.]).unwrap();
    near(a, std::f64::consts::PI * (25. - 3.5_f64.powi(2)));
    near(c[0], 0.);
    near(c[1], 0.);
}

#[test]
fn a_real_plate_thinning_or_removal_remains_visible() {
    for (name, thickness) in [("old", 4.), ("thinned", 2.)] {
        let p = part(&format!("revisions/q134_plate.{name}.step.gz"));
        let found = plates::discover_verified(&Context::new(&p), &Default::default()).unwrap();
        let plate = found.iter().find(|o| o.record.axis == 'z').unwrap();
        near(plate.record.thickness(), thickness);
    }
    let p = part("revisions/q134_plate.removed.step.gz");
    assert!(plates::discover(&Context::new(&p), &Default::default()).is_empty());
}

#[test]
fn audited_boss_ends_exclude_root_fillets_long_tapers_and_concave_shoulders() {
    let Some(root) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    // These imported R0.5 trims slightly overrun the analytic quarter circle. Keep
    // their constituent faces and measured end positions rather than dropping a patch.
    let p = quiddity::read_step_file(&root.join("cadgenbench_inputs/cgb242.step.gz")).unwrap();
    let found = bosses::discover_verified(&Context::new(&p)).unwrap();
    for (index, rounds, z) in [
        (0, vec![10, 11], 2.693086292598496),
        (1, vec![33, 426], 2.6930866426818474),
        (2, vec![45, 46], 2.6930862925984735),
    ] {
        assert!(rounds.iter().all(|f| found[index].context.contains(f)));
        assert!((found[index].record.location[2] - z).abs() < 1e-10);
        near(found[index].record.height, 13.);
    }
    // For #134: fingerprint face order must retrieve the same treated midpoint as
    // discovery's patch order, including a boss split into several cylinder patches.
    let p = quiddity::read_step_file(&root.join("cadgenbench_inputs/cgb243.step.gz")).unwrap();
    let ctx = Context::new(&p);
    for o in bosses::discover(&ctx) {
        let mut canonical = o.defining.clone();
        canonical.sort_unstable();
        near(
            geom::dist(
                bosses::midpoint(&ctx, &o.record, &o.defining),
                bosses::midpoint(&ctx, &o.record, &canonical),
            ),
            0.,
        );
    }
    let p =
        quiddity::read_step_file(&root.join("cadgenbench/threaded_connector_109.step")).unwrap();
    let found = bosses::discover(&Context::new(&p));
    let shaft = found.iter().find(|o| o.defining == [162]).unwrap();
    near(shaft.record.height, 199.67);
    assert!(
        !shaft.context.contains(&164),
        "#134: cone 164 joins cylinder 162 concavely"
    );
    let owners: Vec<_> = found.iter().filter(|o| o.context.contains(&164)).collect();
    assert_eq!(owners.len(), 1, "#134: one cone can extend only one boss");
    near(owners[0].record.diameter, 130.94);
    near(owners[0].record.height, 24.04);
    let r = quiddity::correspondence::caller_space_recognise(&p);
    for boss in r
        .fingerprints
        .features
        .iter()
        .filter(|f| f.family == "bosses")
    {
        let physical = boss
            .faces
            .iter()
            .flat_map(|&f| &r.fingerprints.faces[f].neighbours)
            .filter(|(g, _)| boss.faces.contains(g))
            .count();
        let contracted = boss
            .neighbourhood
            .iter()
            .filter(|s| s.contains(">in:"))
            .count();
        assert_eq!(contracted, physical, "#134: {}", boss.id);
    }
    let p = quiddity::read_step_file(&root.join("nist/nist_ftc_06_asme1_rd.stp")).unwrap();
    let fillets = quiddity::features::fillets::discover(&Context::new(&p), &Default::default());
    for face in [77, 78] {
        let round = fillets.iter().find(|o| o.defining.contains(&face)).unwrap();
        assert_eq!(
            round.record.side, "concave",
            "#134: FTC-06 root material side"
        );
    }
    let found = bosses::discover(&Context::new(&p));
    for index in [4, 6] {
        assert!(
            found[index]
                .context
                .iter()
                .all(|f| ![40, 45, 77, 78].contains(f)),
            "#134: neither a long taper nor a root round is an end treatment"
        );
    }
    let view = quiddity::evidence_view::build_recognition_evidence(&p).unwrap();
    for index in [4, 6] {
        let f = view
            .features
            .iter()
            .find(|f| f.family == "bosses" && f.index == index)
            .unwrap();
        assert!(
            f.hosts.is_empty(),
            "#134: a concave root round does not create a host"
        );
        near(view.result.bosses[index].height, 12.07);
    }
}
