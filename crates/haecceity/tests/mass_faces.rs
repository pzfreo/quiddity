//! Seamless periodic bands and refusal of moments from incomplete mass integrals.
mod common;

use haecceity::{geom, step::read_step_file};

fn relative(got: f64, want: f64, tolerance: f64) {
    assert!(
        (got - want).abs() <= tolerance * want.abs(),
        "{got} vs {want}: relative error {}",
        (got - want) / want
    );
}

#[test]
fn nist_stc_10_closed_spline_rims() {
    let part =
        read_step_file(&common::fixtures().join("ap242/nist/nist_stc_10_asme1_ap242-e2.stp.gz"))
            .unwrap();
    // OCCT BRepGProp::SurfaceProperties, adaptive eps = 1e-10, TopExp::MapShapes order.
    // The fixed Gauss rule is ~6e-6 off on the smaller cylinders. Besides the seven
    // missing cylinders, four adjoining faces previously returned incorrect positive areas.
    for (face, area) in [
        (60, 444.61006689318754),
        (61, 116.05045271003091),
        (62, 116.05045271003127),
        (63, 116.05045271003114),
        (66, 1199.5452590973964),
        (69, 1199.5452590973962),
        (75, 1199.545259097522),
        (93, 20.533688968100627),
        (96, 20.5338787312374),
        (99, 20.53379079232945),
        (100, 20.53379079232944),
    ] {
        let mass = part
            .face_mass(face)
            .unwrap_or_else(|| panic!("face {face}"));
        relative(mass[0], area, 1e-6);
        relative(part.face_moments(face).unwrap().area, area, 1e-6);
    }
    let (volume, area) = part.solid_mass(0).unwrap();
    relative(volume, 188312.81476384445, 1e-6);
    relative(area, 48486.46282635298, 1e-6);
    // CAx-IF validation properties recorded in the STEP file, before export rounding.
    relative(volume, 188312.244641244, 1e-5);
    relative(area, 48486.2744665371, 1e-5);
    let (moment_volume, centre) = part.volume_centroid().unwrap();
    relative(moment_volume, 188312.81476384445, 1e-6);
    let diagonal = geom::dist(part.bounds().min, part.bounds().max);
    for reference in [
        [84.00766564045023, 14.608746723660378, 37.49900523744887],
        [84.0076805122325, 14.6087293037364, 37.4990364132354],
    ] {
        assert!(geom::dist(centre, reference) < 1e-6 * diagonal);
    }
}

#[test]
fn nist_ftc_11_torus_band_across_minor_seam() {
    let part =
        read_step_file(&common::fixtures().join("ap242/nist/nist_ftc_11_asme1_ap242-e3.stp.gz"))
            .unwrap();
    let area = 797.4852472692924; // OCCT adaptive eps = 1e-10, face #1195.
    relative(part.face_mass(3).unwrap()[0], area, 1e-12);
    relative(part.face_moments(3).unwrap().area, area, 1e-12);
    let domain = part.domain(3).unwrap();
    // The region is v ∈ [3.47922…, 2π], rather than its complementary band.
    for u in [0.2, 2.0, 5.9] {
        assert!(domain.contains(u, 5.0));
        assert!(domain.contains(u, 5.0 - std::f64::consts::TAU));
        assert!(!domain.contains(u, 1.0));
    }
    let (volume, area) = part.solid_mass(0).unwrap();
    // Other small trimmed lettering faces retain existing area differences from OCCT.
    // The repaired torus itself agrees to round-off.
    relative(volume, 5135.058392239693, 1e-5);
    relative(area, 5473.599136993476, 2e-5);
    let (moment_volume, centre) = part.volume_centroid().unwrap();
    relative(moment_volume, 5135.058392239693, 1e-5);
    let diagonal = geom::dist(part.bounds().min, part.bounds().max);
    assert!(
        geom::dist(
            centre,
            [
                0.008266784986591528,
                0.0001239144703660064,
                -0.35038226066401973
            ]
        ) < 1e-6 * diagonal
    );
}

#[test]
fn moments_require_every_mass_integral() {
    let mut part = read_step_file(&common::fixtures().join("rejected_cylinder.step")).unwrap();
    // Collapse the side before any cached query. Its quadrature returns finite zero terms,
    // which face_mass rejects; the other two faces still have nonzero flux and moments.
    let geom::Surface::Cylinder { radius, .. } = &mut part.faces[0].surface else {
        panic!("cylinder side")
    };
    *radius = 0.0;
    assert!(part.face_mass(0).is_none());
    assert!(part.solid_mass(0).is_none());
    assert!(part.volume_centroid().is_none());
    assert!(part.face_moments(0).is_none());
    assert!(part.face_projected_moments(0, [1.0, 0.0, 0.0]).is_none());
}

/// Publishing mass from a moment pass preserves the standalone quadrature, including its
/// independent refinement. Exercise periodic bands, held sides, knots and near-end corners.
#[test]
fn mass_cache_is_independent_of_moment_query_order() {
    for name in [
        "ap242/nist/nist_stc_10_asme1_ap242-e2.stp.gz",
        "ap242/nist/nist_ftc_11_asme1_ap242-e3.stp.gz",
        "held_side_knot.step",
        "late_turn.step",
        "rotated_seam_band.step",
    ] {
        let path = common::fixtures().join(name);
        let standalone = read_step_file(&path).unwrap();
        let expected: Vec<_> = (0..standalone.faces.len())
            .map(|f| standalone.face_mass(f))
            .collect();
        for first in 0..3 {
            let part = read_step_file(&path).unwrap();
            if first == 0 {
                let _ = part.volume_centroid();
            } else {
                let faces: Vec<usize> = if name.contains("stc_10") {
                    vec![60, 61, 62, 63, 66, 69, 75, 93, 96, 99, 100]
                } else if name.contains("ftc_11") {
                    vec![3]
                } else {
                    (0..part.faces.len()).collect()
                };
                for face in faces {
                    if first == 1 {
                        let _ = part.face_moments(face);
                    } else {
                        let _ = part.face_projected_moments(face, [0.0, 0.0, 1.0]);
                    }
                }
            }
            for (face, want) in expected.iter().enumerate() {
                assert_eq!(
                    part.face_mass(face),
                    *want,
                    "{name} face {face}, first {first}"
                );
            }
        }
    }
}

#[test]
fn cold_moments_refuse_missing_mass_before_any_mass_query() {
    for first in 0..3 {
        let mut part = read_step_file(&common::fixtures().join("rejected_cylinder.step")).unwrap();
        let geom::Surface::Cylinder { radius, .. } = &mut part.faces[0].surface else {
            panic!("cylinder side")
        };
        *radius = 0.0;
        match first {
            0 => assert!(part.volume_centroid().is_none()),
            1 => assert!(part.face_moments(0).is_none()),
            _ => assert!(part.face_projected_moments(0, [1.0, 0.0, 0.0]).is_none()),
        }
        assert!(part.face_mass(0).is_none());
    }
}

#[test]
fn concurrent_moment_and_mass_queries_publish_the_same_mass() {
    let path = common::fixtures().join("held_side_knot.step");
    let standalone = read_step_file(&path).unwrap();
    let want = standalone.face_mass(0);
    let part = read_step_file(&path).unwrap();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let _ = part.volume_centroid();
        });
        scope.spawn(|| {
            let _ = part.face_moments(0);
        });
        scope.spawn(|| {
            let _ = part.face_projected_moments(0, [0.0, 0.0, 1.0]);
        });
        scope.spawn(|| {
            assert_eq!(part.face_mass(0), want);
        });
    });
    assert_eq!(part.face_mass(0), want);
}
