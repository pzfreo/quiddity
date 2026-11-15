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
fn a_tangent_end_round_belongs_to_its_boss_however_short_the_straight_wall() {
    // For specify-core #181: an R3 top round and cap stay in the Ø65 boss height whether the
    // straight wall (10, 5 or 3 mm) is longer or shorter than twice the round.
    for height in [13., 8., 6.] {
        let p = part(&format!("boss_top_round_h{height}.step.gz"));
        let b = bosses::discover_verified(&Context::new(&p)).unwrap();
        assert_eq!(b.len(), 1, "h{height}");
        near(b[0].record.diameter, 65.);
        near(b[0].record.height, height);
        near(geom::dist(b[0].record.location, [0., 0., height]), 0.);
        assert!(
            b[0].context
                .iter()
                .any(|&f| matches!(p.faces[f].surface, Surface::Torus { .. })),
            "h{height}: the round is the boss's end treatment"
        );
    }
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
