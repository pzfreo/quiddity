//! Original BREP_WITH_VOIDS geometry with open-hole controls, checked structurally.
use std::collections::BTreeSet;

use super::common;
use quiddity::features::{Context, hole_patterns, holes, interior_voids};
use quiddity::kernel::geom::Surface;
use quiddity::kernel::step::IDENTITY;
use quiddity::recognition::{self, FaceAccounting};

#[test]
fn sealed_cylinders_are_only_voids() {
    let base =
        quiddity::read_step_file(&common::fixtures().join("sealed_cylindrical_voids.step.gz"))
            .unwrap();
    assert_eq!(base.solids.len(), 1);
    assert!(base.solid_is_geometrically_valid(0));
    let (s, c) = 0.37_f64.sin_cos();
    for placement in [
        IDENTITY,
        [[c, 0., s, 123.456], [0., 1., 0., -78.9], [-s, 0., c, 41.3]],
        [[0., 0., -1., 11.1], [1., 0., 0., 22.2], [0., -1., 0., 33.3]],
    ] {
        let part = base.transformed(&placement).unwrap();
        let ctx = Context::new(&part);
        let voids = interior_voids::discover_verified(&ctx).unwrap();
        assert_eq!(voids.len(), 3);
        let mut cavity_faces: BTreeSet<usize> = BTreeSet::new();
        for void in &voids {
            assert!(void.record.openings.is_empty());
            assert_eq!(void.record.volume_method, "closed_shell");
            assert!((void.record.estimated_volume - 32. * std::f64::consts::PI).abs() < 1e-7);
            assert_eq!(void.record.grid_pitch, 0.);
            assert_eq!(
                (void.record.enclosed_samples, void.record.air_samples),
                (0, 0)
            );
            assert_eq!(void.defining.len(), 3); // cylinder and two closing disks
            assert!(part.shell_mass(&void.defining).unwrap().0 < 0.);
            assert_eq!(
                void.defining
                    .iter()
                    .filter(|&&face| matches!(
                        part.faces[face].surface,
                        Surface::Cylinder { radius, .. } if (radius - 2.).abs() < 1e-9
                    ))
                    .count(),
                1
            );
            cavity_faces.extend(&void.defining);
        }
        assert_eq!(cavity_faces.len(), 9);
        let found = holes::discover_verified(&ctx, &[]).unwrap();
        assert!(found.is_empty());
        // Record-only and locally degraded consumers use the same candidate filter.
        assert!(holes::recognise_holes(&part, &Default::default()).is_empty());
        assert!(
            holes::discover_locally_degraded(&ctx, &[])
                .unwrap()
                .is_empty()
        );
        let records: Vec<_> = found.iter().map(|h| h.record.clone()).collect();
        assert!(hole_patterns::recognise_hole_patterns(&records).is_empty());

        let recognised = quiddity::correspondence::recognise(&part);
        let doc = recognition::document(&recognised, part.faces.len());
        assert!(doc.failures.is_empty());
        let accounting = doc.face_accounting.as_ref().unwrap();
        assert_eq!(accounting.len(), part.faces.len());
        assert!(
            accounting
                .iter()
                .all(|group| !matches!(group, FaceAccounting::Unexplained))
        );
        for face in &cavity_faces {
            let owners: Vec<_> = doc
                .records
                .iter()
                .filter(|r| r.faces.contains(face))
                .collect();
            assert!(!owners.is_empty());
            assert!(
                owners.iter().all(|r| r.family == "interior_voids"),
                "{owners:?}"
            );
            let FaceAccounting::Feature { record_ids } = &accounting[*face] else {
                panic!("cavity face {face} not accounted to its interior void");
            };
            assert_eq!(
                record_ids,
                &owners.iter().map(|r| r.id.clone()).collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn exterior_connected_bores_remain_holes() {
    let base =
        quiddity::read_step_file(&common::fixtures().join("open_hole_controls.step.gz")).unwrap();
    assert_eq!(base.solids.len(), 1);
    assert!(base.solid_is_geometrically_valid(0));
    let (s, c) = 0.37_f64.sin_cos();
    for placement in [
        IDENTITY,
        [[c, 0., s, 123.456], [0., 1., 0., -78.9], [-s, 0., c, 41.3]],
        [[0., 0., -1., 11.1], [1., 0., 0., 22.2], [0., -1., 0., 33.3]],
    ] {
        let part = base.transformed(&placement).unwrap();
        let ctx = Context::new(&part);
        let found = holes::discover_verified(&ctx, &[]).unwrap();
        assert_eq!(found.len(), 3);
        for (diameter, bottom, depth) in [
            (5., holes::Bottom::Through, 24.),
            (6., holes::Bottom::Flat, 6.),
            (7., holes::Bottom::Flat, 6.), // opens into the exterior-connected pocket
        ] {
            let hole = found
                .iter()
                .find(|h| h.record.diameter == diameter)
                .unwrap();
            assert_eq!((hole.record.bottom, hole.record.depth), (bottom, depth));
        }
        let recognised = quiddity::correspondence::recognise(&part);
        let doc = recognition::document(&recognised, part.faces.len());
        assert!(doc.failures.is_empty());
        assert_eq!(
            doc.records.iter().filter(|r| r.family == "holes").count(),
            3
        );
    }
}

#[test]
fn mixed_block_accounts_for_every_sealed_shell_even_when_grid_sampling_misses_one() {
    let part =
        quiddity::read_step_file(&common::fixtures().join("mixed_sealed_voids.step.gz")).unwrap();
    let ctx = Context::new(&part);
    let voids = interior_voids::discover_verified(&ctx).unwrap();
    assert_eq!(voids.len(), 3);
    let cavity_faces: BTreeSet<_> = voids
        .iter()
        .flat_map(|v| v.defining.iter().copied())
        .collect();
    assert_eq!(cavity_faces.len(), 9);
    let holes = holes::discover_verified(&ctx, &[]).unwrap();
    assert_eq!(holes.len(), 3);
    assert!(
        holes
            .iter()
            .all(|h| h.defining.iter().all(|f| !cavity_faces.contains(f)))
    );
    let found = quiddity::correspondence::recognise(&part);
    let doc = recognition::document(&found, part.faces.len());
    assert_eq!(
        doc.records
            .iter()
            .filter(|r| r.family == "interior_voids")
            .count(),
        3
    );
    for face in cavity_faces {
        let FaceAccounting::Feature { record_ids } = &doc.face_accounting.as_ref().unwrap()[face]
        else {
            panic!("unaccounted sealed cavity face {face}");
        };
        assert_eq!(record_ids.len(), 1);
        assert!(record_ids[0].starts_with("interior_voids/"));
    }
}
