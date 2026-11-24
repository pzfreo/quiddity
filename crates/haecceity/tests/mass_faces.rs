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
    // OCCT BRepGProp::SurfaceProperties via OCP 8.0.1.1, adaptive eps = 1e-10,
    // TopExp::MapShapes order.
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
    // OCCT BRepGProp::{Volume,Surface}Properties via OCP 8.0.1.1: default fixed-Gauss
    // overloads, with no adaptive eps argument (unlike the per-face references above).
    let occt_volume = 188312.81476384445;
    let occt_area = 48486.46282635298;
    relative(volume, occt_volume, 1e-6);
    relative(area, occt_area, 1e-6);
    // CAx-IF validation properties recorded in the STEP file, before export rounding.
    relative(volume, 188312.244641244, 1e-5);
    relative(area, 48486.2744665371, 1e-5);
    let (moment_volume, centre) = part.volume_centroid().unwrap();
    relative(moment_volume, occt_volume, 1e-6);
    let diagonal = geom::dist(part.bounds().min, part.bounds().max);
    for reference in [
        // OCCT BRepGProp::VolumeProperties via OCP 8.0.1.1, fixed Gauss, no eps argument.
        [84.00766564045023, 14.608746723660378, 37.49900523744887],
        // STEP CAx-IF geometric validation centroid, CARTESIAN_POINT #11538.
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
    // OCCT BRepGProp::SurfaceProperties via OCP 8.0.1.1, adaptive eps = 1e-10, face #1195.
    let area = 797.4852472692924;
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
    // OCCT BRepGProp::{Volume,Surface}Properties via OCP 8.0.1.1: default fixed-Gauss
    // overloads, with no adaptive eps argument. The centroid below is from the same capture.
    let occt_volume = 5135.058392239693;
    let occt_area = 5473.599136993476;
    relative(volume, occt_volume, 1e-5);
    relative(area, occt_area, 2e-5);
    let (moment_volume, centre) = part.volume_centroid().unwrap();
    relative(moment_volume, occt_volume, 1e-5);
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

fn collapsed_cylinder() -> haecceity::Part {
    let source = read_step_file(&common::fixtures().join("rejected_cylinder.step")).unwrap();
    let mut faces = source.faces;
    let geom::Surface::Cylinder { radius, .. } = &mut faces[0].surface else {
        panic!("cylinder side")
    };
    *radius = 0.0;
    // Import can already have queried the original side to prove shell orientation.
    haecceity::Part::new(faces, source.edges, source.solids)
}

#[test]
fn moments_require_every_mass_integral() {
    // The collapsed side has zero mass; the other two faces have nonzero flux and moments.
    let part = collapsed_cylinder();
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
                let cold = part.volume_centroid();
                if !part.solids.is_empty() {
                    assert_eq!(part.volume_centroid(), cold, "{name}: cache warm-up");
                }
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
        let part = collapsed_cylinder();
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

// Synthetic exact torus: R=10, r=2. Both bands span 4.2 > π and cross a seam.
// The rectangular UV hole lies 3.6..3.8 radians from the first rim: placing it nearest
// that rim chooses the wrong period; it must be placed nearest the band's midpoint.
fn torus_band_with_hole(winds_u: bool, reversed: bool) -> haecceity::Part {
    use geom::{Curve, Frame, Surface};
    use haecceity::brep::{Edge, Face, Loop};
    use std::f64::consts::TAU;

    let frame = Frame {
        origin: [0.0; 3],
        x: [1.0, 0.0, 0.0],
        y: [0.0, 1.0, 0.0],
        z: [0.0, 0.0, 1.0],
    };
    let mut edges = Vec::new();
    // Isoparametric curves of S(u,v)=((10+2cos v)cos u,(10+2cos v)sin u,2sin v).
    let mut arc = |vary_u: bool, fixed: f64, a: f64, b: f64, closed: bool| {
        let curve = if vary_u {
            Curve::Circle {
                frame: Frame {
                    origin: [0.0, 0.0, 2.0 * fixed.sin()],
                    ..frame
                },
                radius: 10.0 + 2.0 * fixed.cos(),
            }
        } else {
            let (s, c) = fixed.sin_cos();
            Curve::Circle {
                frame: Frame {
                    origin: [10.0 * c, 10.0 * s, 0.0],
                    x: [c, s, 0.0],
                    y: [0.0, 0.0, 1.0],
                    z: [s, -c, 0.0],
                },
                radius: 2.0,
            }
        };
        let start = curve.value(a);
        let end = if closed { start } else { curve.value(b) };
        let same_sense = b > a;
        let samples = haecceity::sampling::sample_edge(&curve, start, end, same_sense, closed);
        let e = edges.len();
        edges.push(Edge {
            curve,
            start,
            end,
            same_sense,
            samples,
            vertices: (2 * e, 2 * e + usize::from(!closed)),
        });
        (e, true)
    };
    let turn = if winds_u == reversed { -TAU } else { TAU };
    let low = arc(winds_u, 5.4, 0.0, turn, true);
    let high = arc(winds_u, 9.6, 0.0, -turn, true);
    let (u0, u1, v0, v1) = if winds_u {
        (TAU - 0.2, TAU + 0.3, 9.0, 9.2)
    } else {
        (9.0, 9.2, TAU - 0.2, TAU + 0.3)
    };
    let mut hole = vec![
        arc(true, v0, u0, u1, false),
        arc(false, u1, v0, v1, false),
        arc(true, v1, u1, u0, false),
        arc(false, u0, v1, v0, false),
    ];
    for (i, &(edge, _)) in hole.iter().enumerate() {
        edges[edge].vertices = (4 + i, 4 + (i + 1) % 4);
    }
    // A hole runs clockwise on a forward face, anticlockwise on a reversed face.
    if !reversed {
        hole.reverse();
        for (_, forward) in &mut hole {
            *forward = false;
        }
    }
    haecceity::Part::new(
        vec![Face {
            surface: Surface::Torus {
                frame,
                major: 10.0,
                minor: 2.0,
            },
            reversed,
            solid: None,
            pcurves: Vec::new(),
            loops: vec![
                Loop {
                    edges: vec![low],
                    vertex: None,
                },
                Loop {
                    edges: vec![high],
                    vertex: None,
                },
                Loop {
                    edges: hole,
                    vertex: None,
                },
            ],
        }],
        edges,
        Vec::new(),
    )
}

fn check_wide_torus_band(winds_u: bool) {
    use std::f64::consts::TAU;
    // ∫∫ r(R+r cos v) du dv, including the rectangular hole's exact area.
    let area =
        |du: f64, v0: f64, v1: f64| du * 2.0 * (10.0 * (v1 - v0) + 2.0 * (v1.sin() - v0.sin()));
    let expected = if winds_u {
        area(TAU, 5.4, 9.6) - area(0.5, 9.0, 9.2)
    } else {
        area(9.6 - 5.4, 0.0, TAU) - area(0.2, TAU - 0.2, TAU + 0.3)
    };
    for reversed in [false, true] {
        let part = torus_band_with_hole(winds_u, reversed);
        let loops = part.uv_loops(0).unwrap();
        for lp in &loops[..2] {
            assert_eq!((lp.winds_u, lp.winds_v), (winds_u, !winds_u));
        }
        assert!(!loops[2].winds_u && !loops[2].winds_v);
        relative(part.face_mass(0).unwrap()[0], expected, 1e-12);
        relative(part.face_moments(0).unwrap().area, expected, 1e-12);
        let domain = part.domain(0).unwrap();
        // (coordinate around the band, coordinate across it). Inside points include
        // the far end of the >π band, and points on either side of the winding seam.
        for (around, across, inside) in [
            (2.0, 5.6, true),
            (2.0, 9.4, true),
            (2.0, 4.0, false),
            (0.0, 9.1, false),
            (TAU + 0.1, 9.1, false),
            (TAU - 0.3, 9.1, true),
            (0.4, 9.1, true),
        ] {
            let (u, v) = if winds_u {
                (around, across)
            } else {
                (across, around)
            };
            for (du, dv) in [(0.0, 0.0), (TAU, -TAU)] {
                assert_eq!(
                    domain.contains(u + du, v + dv),
                    inside,
                    "winds_u={winds_u} reversed={reversed} ({u}, {v}) + ({du}, {dv})"
                );
            }
        }
    }
}

#[test]
fn wide_u_winding_torus_band_with_hole_and_reversed_face() {
    check_wide_torus_band(true);
}

#[test]
fn wide_v_winding_torus_band_with_hole_and_reversed_face() {
    check_wide_torus_band(false);
}

/// S(u,v) = (1e8, 1e8*u, v), u in [0,1e-7], v in [0,1]. The
/// 10-by-1 rectangle is regular despite its very different parameter scales.
#[test]
fn narrow_parameter_sheet_has_analytic_area() {
    let part = read_step_file(
        &common::fixtures().join("captured/effective_surfaces/parts/2fe0e2383b80a0b8.step.gz"),
    )
    .unwrap();
    assert!(part.solids.is_empty());
    // OCP 8.0.1.1, BRepGProp::SurfaceProperties, adaptive eps=1e-9; sole face.
    relative(part.face_mass(0).unwrap()[0], 10.0, 1e-6);
    relative(part.face_moments(0).unwrap().area, 10.0, 1e-6);
    relative(part.face_mass(0).unwrap()[1], 1e9, 1e-6);
}

/// Independent polygonal Green integrals on a cylinder, with explicit gap closure.
/// Refining these boundary samples converges independently of mass.rs's quadrature,
/// corner/knot splitting, primitive reference and loop placement.
fn cylinder_green(part: &haecceity::Part, face: usize, n: usize) -> (f64, f64) {
    let fc = &part.faces[face];
    let geom::Surface::Cylinder { radius, .. } = fc.surface else {
        panic!("cylinder")
    };
    assert_eq!(fc.loops.len(), 1);
    let mut points: Vec<(f64, f64)> = Vec::new();
    for &(e, forward) in &fc.loops[0].edges {
        let ed = &part.edges[e];
        let (mut a, mut b) = haecceity::sampling::edge_interval(
            &ed.curve,
            ed.start,
            ed.end,
            ed.same_sense,
            ed.is_closed(),
        );
        if !forward {
            std::mem::swap(&mut a, &mut b);
        }
        for k in 0..=n {
            let t = a + (b - a) * k as f64 / n as f64;
            let (mut u, v) = fc.surface.parameters(ed.curve.value(t), None).unwrap();
            if let Some(previous) = points.last() {
                u = geom::nearest_turn(u, previous.0);
            }
            points.push((u, v));
        }
    }
    points.push(points[0]);
    let mut h = 0.0;
    let mut g = 0.0;
    for w in points.windows(2) {
        h -= radius * (w[0].1 + w[1].1) / 2.0 * (w[1].0 - w[0].0);
        g += radius * (w[0].0 + w[1].0) / 2.0 * (w[1].1 - w[0].1);
    }
    (h.abs(), g.abs())
}

#[test]
fn varying_pitch_sweep_closes_open_uv_wires() {
    let part =
        read_step_file(&common::fixtures().join("modelled_threads/varying_pitch_sweep.step.gz"))
            .unwrap();
    // OCP 8.0.1.1, BRepGProp adaptive eps=1e-9, TopExp order. These
    // references integrate open pcurve wires: omitting gap closure in ∮u dv
    // reproduces OCCT; closing them makes both Green paths agree with ours.
    // Run: "$OCP_PY" -I tools/mass_fixture_evidence.py --freeform
    // The sweep's known OCCT difference comes from open pcurve wires; closed
    // projected-boundary integrals agree with ours instead of those references.
    for (face, _occt, closed) in [
        (4, 98.16765734609923, 98.17068665143155),
        (7, 49.9435965949216, 34.08103140997729),
        (8, 30.227164859519196, 30.228167268731845),
        (17, 152.8168886339292, 152.81565932170744),
        (20, 48.31801113428927, 32.47342792581202),
    ] {
        let mass = part.face_mass(face).unwrap();
        relative(mass[0], closed, 1e-9);
        relative(part.face_moments(face).unwrap().area, closed, 1e-9);
        if matches!(part.faces[face].surface, geom::Surface::Cylinder { .. }) {
            for n in [1024, 4096] {
                let (h, g) = cylinder_green(&part, face, n);
                relative(h, closed, 1e-6);
                relative(g, closed, 1e-6);
            }
        }
        // For spline faces4/17, --freeform projects the exact 3D curves and
        // converges to 98.1706866503/152.8156593289 at 10,000 boundary panels.
        // Their open pcurve integrals converge to the recorded OCCT references.
    }
    let (volume, area) = part.solid_mass(0).unwrap();
    // Whole-shape OCCT values retain the open-wire fluxes, so numerical
    // refinement cannot meet a 1e-6 comparison with that reference.
    // Independent UV strips on face 2 (#576), assembled from the trimmed
    // STEP master pcurves, give area 152.81565409318 mm². Substituting its
    // flux and first-moment tensors changes the master solid centroid by
    // 7.1e-13 mm. The projected boundary gives 152.8157192874 mm² there.
    relative(volume - 2535.1985709713267, -105.69257968032434, 1e-6);
    relative(area - 1531.3435624916838, -31.704345650956384, 1e-6);
    let (v, c) = part.volume_centroid().unwrap();
    relative(v, 2429.508722080353, 1e-9);
    assert!(
        geom::dist(
            c,
            [
                2.1062804706439307e-5,
                3.1714680413300244e-8,
                3.9991386278060883
            ]
        ) < 1e-9
    );
    // OCCT material centroid from the same capture (not a matching reference).
    let occt_c = [
        -0.02083624294153927,
        -2.384266550942814e-9,
        3.9673645946049643,
    ];
    assert!(geom::dist(c, occt_c) > 1e-3);
}

#[test]
fn truck_master_shaft_has_outward_mass_and_exact_caps() {
    let part = read_step_file(&common::fixtures().join("truck_master_shaft_hole.step.gz")).unwrap();
    assert_eq!(
        part.input_repairs(),
        ["solid 0: mixed face senses repaired (20 faces)"]
    );
    // OCP 8.0.1.1 BRepGProp adaptive eps=1e-9. Healing reorders this file:
    // our10,14,17,23 -> OCCT15,16,5,8; bore15,16,21,22 -> OCCT13,6,7,12.
    for (face, occt) in [
        (0, 942.4777960769379),
        (1, 942.4777960769379),
        (2, 942.477796076938),
        (3, 942.4777960769379),
        (10, 917.0395542364357),
        (14, 916.8916944960374),
        (17, 916.8370933615403),
        (23, 917.0506343679618),
        (15, 120.48219649981259),
        (16, 120.4776160661682),
        (21, 120.47672061765437),
        (22, 120.47331474425341),
    ] {
        relative(part.face_mass(face).unwrap()[0], occt, 1e-6);
    }
    // The exact circular 3D rims bound quarter disks. OCCT's approximate
    // pcurves give smaller caps; independent 3D-boundary integration converges
    // to these analytic values (tools/mass_fixture_evidence.py).
    for (faces, exact, _occt) in [
        (
            &[4, 5, 6, 7][..],
            225.0 * std::f64::consts::PI / 4.0,
            176.66168008902335,
        ),
        (
            &[11, 12, 13, 18][..],
            25.0 * std::f64::consts::PI,
            78.51630226178801,
        ),
    ] {
        for &face in faces {
            relative(part.face_mass(face).unwrap()[0], exact, 1e-12);
            relative(part.face_moments(face).unwrap().area, exact, 1e-12);
        }
    }
    // Whole OCCT volume=46135.47088917455, area=9333.051019799585.
    // The remaining 1.12e-4 OCCT volume gap comes from its approximate cap pcurves.
    // Off-surface bore intersection curves (up to 0.12 mm) make the shaft volume
    // intrinsically ambiguous by about 2e-4; analytic design intent is 46139.110 mm³.
    // These pins specify this quadrature's convention, not that geometric uncertainty.
    let (volume, area) = part.solid_mass(0).unwrap();
    relative(volume - 46135.47088917455, 5.187457999658829, 1e-6);
    relative(area - 9333.051019799585, 0.3052675857500873, 1e-6);
    let (v, c) = part.volume_centroid().unwrap();
    relative(v, 46140.3795561635, 1e-9);
    assert!(
        geom::dist(
            c,
            [
                39.36041247055186,
                -4.339035384208806e-5,
                -0.00011897503090313706
            ]
        ) < 1e-9
    );
    // OCCT centroid from that same capture; the different domain cannot meet
    // 1e-6 of the diagonal (the observed normalized distance is ~6.84e-6).
    let occt_c = [
        39.361152572240144,
        -4.2361593933366616e-5,
        -5.350120068459397e-5,
    ];
    assert!(geom::dist(c, occt_c) > 1e-6 * geom::dist(part.bounds().min, part.bounds().max));
}

/// Face moments and the solid's volume and centroid share one integration per face; neither
/// depends on which is asked first, and both keep the values the two separate integrations
/// gave (pinned from before they were shared, to round-off).
#[test]
fn face_moments_and_volume_centroid_share_one_integration() {
    let path = common::fixtures().join("ap242/nist/nist_stc_10_asme1_ap242-e2.stp.gz");
    let fresh = || read_step_file(&path).unwrap();
    let moments_first = fresh();
    let all: Vec<_> = (0..moments_first.faces.len())
        .map(|f| moments_first.face_moments(f))
        .collect();
    let centroid_first = fresh();
    let (volume, centre) = centroid_first.volume_centroid().unwrap();
    assert_eq!(moments_first.volume_centroid(), Some((volume, centre)));
    for (face, want) in all.iter().enumerate() {
        assert_eq!(centroid_first.face_moments(face), *want, "face {face}");
    }
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-12 * b.abs().max(1.0);
    assert!(close(volume, 188312.77072775364));
    for (a, b) in centre
        .iter()
        .zip([84.00764760460824, 14.608745222541721, 37.4990086816463])
    {
        assert!(close(*a, b), "{centre:?}");
    }
    let m = all[0].unwrap();
    assert!(close(m.area, 908.0473234821043));
    assert!(close(m.centroid[1], 15.946107488877757));
    assert!(close(m.second_moments[2][2], 392675.6831400961));
}

/// Whole spheres and closed splines with only seams have no material boundary.
/// Trimmed spheres must measure the correct side of each wire, including pole crossings.
#[test]
fn spherical_solid_measurements() {
    let dir = common::fixtures().join("brep-conformance");
    // Springrep 2.1.0 references. OCP 7.8 independently checked the whole
    // sphere, lens, box, split sphere and box with spindle caps.
    // For the stretched sphere, the volume and centre also follow from the affine
    // image of the unit ball. Integrating the cofactor of the affine map on the
    // unit sphere independently gives 20.704519167 mm².
    let cases = [
        (
            "whole_sphere",
            13.351809032195062,
            27.21724729010468,
            [-0.8499371627996487, 0.8735405104041485, -0.5747470776814828],
        ),
        (
            "sphere_lens",
            168.69167170610382,
            147.80770375090304,
            [2.099302740899602, 0.0, 0.0],
        ),
        (
            "thin_sphere_lens",
            1.1265494515087496e-9,
            0.0005090673797033365,
            [8.85186604602571, 0.0, 0.0],
        ),
        (
            "thin_sphere_cap",
            1.873084924608645e-9,
            0.000890187686229883,
            [0.0, 0.0, 8.416579384147722],
        ),
        (
            "spherical_box_face",
            553.5097156185955,
            432.0964256877111,
            [4.641539450109799; 3],
        ),
        (
            "spherical_box_with_spindle_caps",
            724.2581849459086,
            434.60950816900345,
            [3.285673925252643, 3.3354995618978527, 2.703472885135289],
        ),
        (
            "split_sphere",
            36.601431000072196,
            54.3796195809381,
            [-1.0872940416786603, 0.1700865678534731, 2.604665400579255],
        ),
        (
            "stretched_sphere",
            7.469846875521929,
            20.70451916683961,
            [-42.23533492325782, 47.10542502072179, 37.7400811722552],
        ),
    ];
    let mut failures = Vec::new();
    for (name, volume, area, centre) in cases {
        let path = dir.join(format!("{name}.step.gz"));
        let part = read_step_file(&path).unwrap();
        // Express volume, area, and centroid errors as lengths in mm.
        let tolerance = if name.starts_with("thin_") {
            1e-9
        } else {
            1e-7
        };
        let mut check = |label: &str, part: &haecceity::Part, centre, centre_tolerance| {
            assert_eq!(part.solids.len(), 1, "{name} {label}");
            let mass = part.solid_mass(0);
            let centroid = part.volume_centroid();
            if !mass.is_some_and(|(v, a)| {
                (v - volume).abs() <= tolerance * area
                    && (a - area).abs() <= tolerance * area.sqrt()
            }) || !centroid.is_some_and(|(v, c)| {
                (v - volume).abs() <= tolerance * area && geom::dist(c, centre) <= centre_tolerance
            }) {
                failures.push(format!(
                    "{name} {label}: mass {mass:?}, volume/centroid {centroid:?}"
                ));
            }
        };
        check("as written", &part, centre, tolerance);

        if name == "spherical_box_with_spindle_caps" {
            let bytes =
                haecceity::step::decode_step_file_bytes(&path, std::fs::read(&path).unwrap())
                    .unwrap();
            let step = std::str::from_utf8(&bytes).unwrap();
            let mut selected = step.to_owned();
            for (from, to) in [
                (
                    "#348=SURFACE_CURVE('',#205,(#341,#347),.CURVE_3D.);",
                    "#348=SURFACE_CURVE('',#205,(#341,#347),.PCURVE_S2.);",
                ),
                (
                    "#342=CARTESIAN_POINT('',(0.0,1.5707963267948966));",
                    "#342=CARTESIAN_POINT('',(0.0,1.5707963467948966));",
                ),
            ] {
                assert_eq!(selected.matches(from).count(), 1);
                selected = selected.replace(from, to);
            }
            let part = haecceity::read_step(selected.as_bytes()).unwrap();
            let face = (0..part.faces.len())
                .find(|&f| part.face_source(f).unwrap().entity == 2072)
                .unwrap();
            let geom::Surface::Torus { major, minor, .. } = part.faces[face].surface else {
                panic!("expected spindle cap");
            };
            // Full u sweep from the selected latitude to the collapsed pole.
            let rim: f64 = 1.5707963467948966;
            let pole = (-major / minor).acos();
            let expected = std::f64::consts::TAU
                * minor
                * (major * (pole - rim) + minor * (pole.sin() - rim.sin()));
            relative(part.face_mass(face).unwrap()[0], expected, 1e-12);
            relative(part.face_moments(face).unwrap().area, expected, 1e-12);
        }

        // Offset just the spatial rim circle by 1e-12 mm in its plane. Keep
        // the height and radius of the original cap and translate its centre.
        let (rim, moved, offset) = match name {
            "thin_sphere_cap" => (
                "#49=CARTESIAN_POINT('',(0.0,0.0,8.416576578619157));",
                "#49=CARTESIAN_POINT('',(1.E-12,0.0,8.416576578619157));",
                [1e-12, 0.0, 0.0],
            ),
            "thin_sphere_lens" => (
                "#29=CARTESIAN_POINT('',(8.851865945671033,0.0,0.0));",
                "#29=CARTESIAN_POINT('',(8.851865945671033,1.E-12,0.0));",
                [0.0, 1e-12, 0.0],
            ),
            _ => continue,
        };
        let bytes =
            haecceity::step::decode_step_file_bytes(&path, std::fs::read(&path).unwrap()).unwrap();
        let step = std::str::from_utf8(&bytes).unwrap();
        assert_eq!(step.matches(rim).count(), 1, "{name}");
        for (source, shift, centre_tolerance) in [
            (moved.to_owned(), offset, 1e-12),
            // Exceed the file's uncertainty and exercise the checked UV flux.
            (
                moved.replace("1.E-12", "1.E-6"),
                geom::scale(offset, 1e6),
                3e-12,
            ),
        ] {
            let perturbed = haecceity::read_step(step.replace(rim, &source).as_bytes()).unwrap();
            check(
                "with perturbed rim",
                &perturbed,
                geom::add(centre, shift),
                centre_tolerance,
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
