//! Kernel behaviour on real parts read from STEP.

mod common;

use std::path::Path;

use haecceity::classify::{Classifier, State};
use haecceity::read_step_file;

fn fixture(name: &str) -> haecceity::Part {
    read_step_file(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name),
    )
    .unwrap()
}

#[test]
fn classifier_sees_material_hole_and_outside() {
    // Box(30, 30, 20) - Cylinder(5, 20): a 30 × 30 × 20 block with a Ø10 through hole.
    let part = fixture("rejected_bored_box.step");
    let c = Classifier::new(&part);
    assert_eq!(c.classify([10.0, 10.0, 0.0]), State::In);
    assert_eq!(c.classify([0.0, 0.0, 0.0]), State::Out, "inside the hole");
    assert_eq!(c.classify([0.0, 0.0, 15.0]), State::Out, "above the block");
    assert_eq!(c.classify([20.0, 0.0, 0.0]), State::Out, "beside the block");
    // Just inside each kind of boundary.
    assert_eq!(c.classify([14.999, 0.0, 0.0]), State::In);
    assert_eq!(c.classify([5.001, 0.0, 0.0]), State::In);
    assert_eq!(c.classify([4.999, 0.0, 0.0]), State::Out);
    // On a face, as BRepClass3d reports within its tolerance.
    assert_eq!(c.classify([15.0, 3.0, 2.0]), State::On);
    assert_eq!(c.classify([5.0, 0.0, 0.0]), State::On);
    // On edges: a block corner, a block edge and the bore's rim.
    assert_eq!(c.classify([15.0, 15.0, 10.0]), State::On);
    assert_eq!(c.classify([15.0, 15.0, 3.0]), State::On);
    assert_eq!(c.classify([0.0, 5.0, 10.0]), State::On);
}

#[test]
fn reading_is_deterministic_and_topology_is_closed() {
    let a = fixture("turned_with_sphere.step");
    let b = fixture("turned_with_sphere.step");
    assert_eq!(a.faces.len(), b.faces.len());
    for s in 0..a.solids.len() {
        assert!(a.solid_is_valid(s));
    }
    for f in 0..a.faces.len() {
        assert_eq!(a.uv_bounds(f), b.uv_bounds(f));
        assert_eq!(a.neighbours(f), b.neighbours(f));
    }
}

#[test]
fn arcs_read_convex_rims_and_concave_pocket_corners() {
    use haecceity::brep::Arc;
    // Box(30, 30, 20) - Cylinder(5, 20): every edge, the bore rims included, is a convex wedge.
    let part = fixture("rejected_bored_box.step");
    for a in 0..part.faces.len() {
        for b in (0..part.faces.len()).filter(|&b| b != a) {
            if let Some(arc) = part.arc(a, b) {
                assert_eq!(arc, Arc::Convex, "faces {a} and {b}");
            }
        }
    }
    // A blind pocket: its floor meets its walls in concave edges, read the same from either side.
    let part = fixture("rejected_internal_pocket.step");
    let mut concave = 0;
    for a in 0..part.faces.len() {
        for b in (0..part.faces.len()).filter(|&b| b != a) {
            if let Some(arc) = part.arc(a, b) {
                assert_eq!(part.arc(b, a), Some(arc), "symmetric for faces {a} and {b}");
                concave += usize::from(arc == Arc::Concave);
            }
        }
    }
    assert!(
        concave >= 8,
        "the pocket floor and its four walls, both ways round"
    );
}

#[test]
fn arcs_at_closed_edges_ignore_their_recorded_direction() {
    use haecceity::brep::Arc;
    // The bore rims are full circles; flip every face's use of them and the read must not change.
    let mut part = fixture("rejected_bored_box.step");
    let closed: Vec<bool> = part.edges.iter().map(|e| e.is_closed()).collect();
    for face in &mut part.faces {
        for lp in &mut face.loops {
            for (e, forward) in &mut lp.edges {
                if closed[*e] {
                    *forward = !*forward;
                }
            }
        }
    }
    let mut rims = 0;
    for a in 0..part.faces.len() {
        for b in (0..part.faces.len()).filter(|&b| b != a) {
            if let Some(arc) = part.arc(a, b) {
                assert_eq!(arc, Arc::Convex, "faces {a} and {b}");
                rims += usize::from(part.shared_edges(a, b).iter().any(|&e| closed[e]));
            }
        }
    }
    assert!(rims >= 4, "both rims, both ways round");
}

/// cgb217's B-spline face 34: its edge 89 runs just outside the side v = 1 and dips 1.6e-3 mm
/// into the face, leaving the side 1.2e-5 before it crosses the knot line u = 0.25. The walk
/// must split at that corner, not only at the knot line, or it misses the foot point's step
/// off the side. tools/face_area_evidence.py gives 28.16983104 by Green's theorem and by
/// direct slices (tests/fixtures/references/face_areas.json).
#[test]
fn boundary_walk_splits_at_a_corner_beside_a_knot_line() {
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let part = read_step_file(&dir.join("cadgenbench_inputs/cgb217.step.gz")).unwrap();
    let [area, _] = part.face_mass(34).unwrap();
    assert!((area - 28.16983104).abs() < 1e-7, "{area}");
}

/// cgb242's B-spline face 726 has a side u = 1 collapsed to a point, where its edge 730 starts.
/// Under the motion below the walk's first node there lands at v = 1 instead of v = 0 (v is
/// undetermined on that side), so its swept parameter area lost the jump's ∮ u dv, the loop read
/// as clockwise and the face's area came out negative.
#[test]
fn a_jump_along_a_collapsed_side_leaves_the_area_unchanged() {
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let path = dir.join("cadgenbench_inputs/cgb242.step.gz");
    let moved: haecceity::step::Placement = [
        [0.0, 0.0, 1.0, 123.456],
        [1.0, 0.0, 0.0, -78.9],
        [0.0, 1.0, 0.0, 41.3],
    ];
    let area = |placement: &haecceity::step::Placement| {
        let part = haecceity::step::read_step_file_placed(&path, placement).unwrap();
        part.face_mass(726).unwrap()[0]
    };
    let (unmoved, moved) = (area(&haecceity::step::IDENTITY), area(&moved));
    // A separate strip integral on the three selected master trims of #84456
    // converges to 18.229224419 mm² at 65,536 panels. Keep the previous spatial
    // control by explicitly selecting CURVE_3D on those same three curves.
    assert!((unmoved - 18.229224419).abs() < 1e-7, "{unmoved}");
    let projected = common::with_spatial_edges(&path, &[34682, 44942, 84461].into());
    let projected = projected.face_mass(726).unwrap()[0];
    assert!((projected - 18.22923018).abs() < 1e-7, "{projected}");
    assert!(
        (moved - unmoved).abs() < 1e-9 * unmoved,
        "{unmoved} moved {moved}"
    );
}

/// Two quarter cylinders of radius 7 on axes 14 apart (x = ±7, y = 0) kiss along x = y = 0 with
/// opposite outward normals, as sm-hanger's faces 11 and 12 do; the first-order turn is
/// round-off there. Extruded 5 in z by OpenCascade from the profile (0,0) - arc to (7,7) -
/// (7,-5) - (-7,-5) - (-7,7) - arc to (0,0) (`notch`: the material fills all round the edge but
/// the zero-angle notch between the arcs, OpenCascade's classifier OUT at (0, 0.01, 2.5) and IN
/// 1e-3 to either side) and from the arcs closed by (7,7) - (-7,7) (`knife`: the material is the
/// notch).
#[test]
fn arcs_where_surfaces_kiss_read_the_second_order() {
    use haecceity::brep::Arc;
    use haecceity::geom::SurfaceType;
    for (name, expected) in [
        ("kissing_cylinders_notch.step", Arc::Concave),
        ("kissing_cylinders_knife.step", Arc::Convex),
    ] {
        let part = fixture(name);
        let cylinders: Vec<usize> = (0..part.faces.len())
            .filter(|&f| part.faces[f].surface.kind() == SurfaceType::Cylinder)
            .collect();
        let [a, b] = cylinders[..] else {
            panic!("{name}: two cylinders, not {cylinders:?}");
        };
        assert_eq!(part.arc(a, b), Some(expected), "{name}");
        assert_eq!(
            part.arc(b, a),
            Some(expected),
            "{name}, the other way round"
        );
    }
}

#[test]
fn areas_and_volumes_are_exact() {
    use std::f64::consts::PI;
    let close = |got: Option<f64>, want: f64, what: &str| {
        let got = got.unwrap_or_else(|| panic!("{what}: none"));
        assert!(
            (got - want).abs() <= 1e-12 * want.abs(),
            "{what}: {got} vs {want} ({:e})",
            (got - want) / want
        );
    };
    // Box(30, 30, 20) - Cylinder(5, 20).
    let part = fixture("rejected_bored_box.step");
    close(
        part.solid_mass(0).map(|m| m.0),
        30.0 * 30.0 * 20.0 - PI * 25.0 * 20.0,
        "bored box volume",
    );
    close(
        part.solid_mass(0).map(|m| m.1),
        2.0 * (900.0 - 25.0 * PI) + 4.0 * 600.0 + 2.0 * PI * 5.0 * 20.0,
        "bored box area",
    );
    // Faces that test the boundary walk: a hemisphere bounded by a seam running up to its pole
    // and back (r = 8), a cap closed through a pole (r = 3: 2πr² of the cap), and a cylinder
    // stored as a closed B-spline surface (r = 4, h = 10) with its disc ends.
    let face = |name: &str, f: usize| fixture(name).face_mass(f).map(|m| m[0]);
    close(
        face("captured/9808e56042d8be45.step.gz", 3),
        128.0 * PI,
        "hemisphere",
    );
    close(
        face("turned_with_sphere.step", 9),
        18.0 * PI,
        "spherical cap",
    );
    let bspline = |got: Option<f64>, want: f64, what: &str| {
        let got = got.unwrap_or_else(|| panic!("{what}: none"));
        assert!((got - want).abs() <= 1e-9 * want, "{what}: {got} vs {want}");
    };
    let cylinder = "captured/14a917f282ad82d6.step.gz";
    bspline(face(cylinder, 0), 80.0 * PI, "B-spline cylinder");
    bspline(face(cylinder, 1), 16.0 * PI, "B-spline disc");
    // Torus(10, 2).
    let part = fixture("full_torus.step");
    close(
        part.solid_mass(0).map(|m| m.0),
        2.0 * PI * PI * 10.0 * 4.0,
        "torus volume",
    );
    close(
        part.solid_mass(0).map(|m| m.1),
        4.0 * PI * PI * 10.0 * 2.0,
        "torus area",
    );
    // A hemisphere (r = 12.7) bounded by one closed great circle through both poles, starting
    // 37° round from one, so a quadrature panel would straddle each pole.
    let part = fixture("hemisphere_one_edge.step");
    let r: f64 = 12.7;
    close(
        part.solid_mass(0).map(|m| m.0),
        2.0 / 3.0 * PI * r.powi(3),
        "hemisphere volume",
    );
    close(
        part.solid_mass(0).map(|m| m.1),
        3.0 * PI * r * r,
        "hemisphere area",
    );
    // Torus elbows with no seam edge, so the face is a band between two loops running round
    // v: Torus(20, 1) swept 200° (more than half a turn, so the nearer turn is the wrong side),
    // and Torus(7, 3) whose loops describe the 240° band rather than its 120° complement.
    for (name, big, small, sweep) in [
        ("torus_elbow_seamless.step", 20.0, 1.0, 200.0),
        ("torus_elbow_seamless_reversed_bound.step", 7.0, 3.0, 240.0),
    ] {
        let part = fixture(name);
        let turn = sweep / 360.0;
        close(
            part.solid_mass(0).map(|m| m.0),
            2.0 * PI * PI * big * small * small * turn,
            name,
        );
        close(
            part.solid_mass(0).map(|m| m.1),
            4.0 * PI * PI * big * small * turn + 2.0 * PI * small * small,
            name,
        );
    }
}

/// Drill points written as just their rim circle — a cone face with one loop, no seam and no
/// apex vertex loop — reach the apex, as OpenCascade completes them on import (it adds a seam
/// and a degenerate edge there). Each area is OpenCascade's `BRepGProp::SurfaceProperties`
/// (the file read by build123d's `import_step`, the face at this index). The files' degree is
/// 0.0174532925 rad, so the rim circle lies off the cone by about 1e-9 of its radius and the
/// two integrations agree to that, not to machine precision.
#[test]
fn drill_points_bounded_by_their_rim_alone_reach_the_apex() {
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    for (file, face, occ) in [
        ("nist/nist_ftc_07_asme1_rd.stp", 209, 36.946366733370226),
        ("nist/nist_ftc_10_asme1_rb.stp", 174, 27.717219757814156),
    ] {
        let part = read_step_file(&dir.join(file)).unwrap();
        let area = part.face_mass(face).expect("drill point has an area")[0];
        assert!(
            (area - occ).abs() <= 1e-8 * occ,
            "{file} face {face}: {area} vs OpenCascade's {occ}"
        );
        let apex = part.faces[face].surface.cone_apex().unwrap();
        let b = part.face_bounds(face);
        assert!(
            (0..3).all(|k| b.min[k] <= apex[k] + 1e-9 && apex[k] - 1e-9 <= b.max[k]),
            "{file} face {face}: bounds miss the apex"
        );
    }
}

#[test]
fn swept_faces_are_closed_prisms_of_the_face() {
    use haecceity::geom::{Surface, scale};
    use haecceity::sweep::extrude_face;
    for name in [
        "golden_circular_blind_step.step",
        "golden_oblique_through_step.step",
        "edge_open_pocket_floor_hole.step",
    ] {
        let part = fixture(name);
        let mut swept = 0;
        for face in 0..part.faces.len() {
            let Surface::Plane { frame } = part.faces[face].surface else {
                continue;
            };
            let Some([area, _]) = part.face_mass(face) else {
                continue;
            };
            // Along the outward normal and against it, from a moved start.
            for sign in [2.5, -2.5] {
                let Some(prism) = extrude_face(&part, face, [0.5, -1.0, 2.0], scale(frame.z, sign))
                else {
                    continue;
                };
                swept += 1;
                assert!(prism.solid_is_valid(0), "{name} face {face}");
                let (volume, _) = prism.solid_mass(0).unwrap();
                assert!(
                    (volume - area * 2.5).abs() <= 1e-9 * volume,
                    "{name} face {face}: {volume} vs {}",
                    area * 2.5
                );
            }
        }
        assert!(swept > 0, "{name}: no face swept");
        if name == "edge_open_pocket_floor_hole.step" {
            // The pocket floor and the plate's bottom, each holed by a closed circle.
            let holed = (0..part.faces.len())
                .filter(|&f| {
                    part.faces[f].loops.len() > 1
                        && part
                            .face_edges(f)
                            .iter()
                            .any(|&e| part.edges[e].is_closed())
                        && extrude_face(&part, f, [0.0; 3], [0.0, 0.0, 1.0]).is_some()
                })
                .count();
            assert!(holed >= 2, "{name}: {holed} holed faces swept");
        }
    }
}

#[test]
fn prism_probes_measure_swept_regions() {
    use haecceity::geom::Bounds;
    use haecceity::volume::{Prism, PrismEdge, Probe, common_volume, probe_volume};
    // Box(30, 10, 40) at y = -5 less a U-section slot (10 wide, radius 3, flat 4) cut 20 deep
    // from z = 20 down to the cap at z = 0: open at y = 0.
    let part = fixture("captured/4e090fe361c883ac.step.gz");
    let solid = Classifier::new(&part);
    let straight = |pts: &[[f64; 2]]| PrismEdge {
        points: pts.iter().map(|p| [p[0], p[1], 0.0]).collect(),
        straight: true,
    };
    let rectangle = |x0: f64, x1: f64, y0: f64, y1: f64| {
        vec![
            straight(&[[x0, y0], [x1, y0]]),
            straight(&[[x1, y0], [x1, y1]]),
            straight(&[[x1, y1], [x0, y1]]),
            straight(&[[x0, y1], [x0, y0]]),
        ]
    };
    // A rectangle swept along z is the box with the same corners, wherever it lies.
    for (x0, x1, y0, y1, z0, z1) in [
        (-10.0, -6.0, -8.0, -2.0, -5.0, 5.0),
        (-8.0, 8.0, -6.0, 2.0, 5.0, 15.0),
        (-3.0, 3.0, -2.0, 1.0, -4.0, 12.0),
    ] {
        let prism = Probe::Prism(Prism {
            axis: 2,
            lo: z0,
            hi: z1,
            loops: vec![rectangle(x0, x1, y0, y1)],
        });
        let cube = Probe::Box(Bounds {
            min: [x0, y0, z0],
            max: [x1, y1, z1],
        });
        let (a, b) = (
            common_volume(&solid, &prism).unwrap(),
            common_volume(&solid, &cube).unwrap(),
        );
        assert!((a - b).abs() <= 1e-9 * b.max(1.0), "{a} vs {b}");
        assert!((probe_volume(&prism).unwrap() - probe_volume(&cube).unwrap()).abs() <= 1e-9);
    }
    // The slot's own section swept along the slot is empty; swept on beyond the cap it is full.
    let arc = |cx: f64, from: f64, to: f64| PrismEdge {
        points: (0..=64)
            .map(|i| {
                let t = from + (to - from) * i as f64 / 64.0;
                [cx + 3.0 * t.cos(), 3.0 * t.sin(), 0.0]
            })
            .collect(),
        straight: false,
    };
    let half_pi = std::f64::consts::FRAC_PI_2;
    let section = || {
        vec![
            straight(&[[-5.0, 0.0], [5.0, 0.0]]),
            arc(2.0, 0.0, -half_pi),
            straight(&[[2.0, -3.0], [-2.0, -3.0]]),
            arc(-2.0, -half_pi, -2.0 * half_pi),
        ]
    };
    let swept = |lo: f64, hi: f64| {
        Probe::Prism(Prism {
            axis: 2,
            lo,
            hi,
            loops: vec![section()],
        })
    };
    assert_eq!(common_volume(&solid, &swept(0.0, 20.0)), Some(0.0));
    let beyond = swept(-10.0, 0.0);
    let full = probe_volume(&beyond).unwrap();
    // Chords inside the arcs: 64 per quarter lose about 0.02 mm³ over the 10 mm.
    assert!(
        (full - 10.0 * (4.0 * 3.0 + 9.0 * half_pi)).abs() < 0.05,
        "{full}"
    );
    assert!((common_volume(&solid, &beyond).unwrap() - full).abs() <= 1e-9 * full);
}

#[test]
fn convex_prism_probes_match_the_analytic_volume() {
    use haecceity::volume::{Prism, PrismEdge, Probe, common_volume, probe_volume};
    // Box(30, 30, 20) - Cylinder(5, 20): x, y in [-15, 15], z in [-10, 10], bored along z.
    let part = fixture("rejected_bored_box.step");
    let solid = Classifier::new(&part);
    // A gusset-like triangle, legs 14 from (6, 6), its hypotenuse oblique to every axis.
    let triangle = |pts: [[f64; 3]; 3]| {
        (0..3)
            .map(|i| PrismEdge {
                points: vec![pts[i], pts[(i + 1) % 3]],
                straight: true,
            })
            .collect::<Vec<_>>()
    };
    // Swept along z over [0, 15]: the block clips two corners (legs 5) and the top at z = 10,
    // so the material shares (98 - 2 × 12.5) × 10 = 730 of the probe's 98 × 15 = 1470.
    let along_z = Probe::Prism(Prism {
        axis: 2,
        lo: 0.0,
        hi: 15.0,
        loops: vec![triangle([
            [6.0, 6.0, 0.0],
            [20.0, 6.0, 0.0],
            [6.0, 20.0, 0.0],
        ])],
    });
    assert!((probe_volume(&along_z).unwrap() - 1470.0).abs() <= 1e-9);
    let got = common_volume(&solid, &along_z).unwrap();
    assert!((got - 730.0).abs() <= 1e-9 * 730.0, "{got}");
    // A triangle with legs 7 in (y, z) swept along x over [-5, 5], within the block and clear
    // of the bore (y ≥ 6 > 5): full, 24.5 × 10.
    let along_x = Probe::Prism(Prism {
        axis: 0,
        lo: -5.0,
        hi: 5.0,
        loops: vec![triangle([
            [0.0, 6.0, -8.0],
            [0.0, 13.0, -8.0],
            [0.0, 6.0, -1.0],
        ])],
    });
    let full = probe_volume(&along_x).unwrap();
    assert!((full - 245.0).abs() <= 1e-9);
    let got = common_volume(&solid, &along_x).unwrap();
    assert!((got - full).abs() <= 1e-9 * full, "{got}");
}

#[test]
fn an_edge_used_four_times_is_valid_while_the_shell_stays_one_set() {
    // A section-recess test part: a bore of radius 4 tangent inside the x = 25 wall, whose seam
    // is the wall's edge too (used by the wall, its neighbour and the seam's two sides), as
    // `BRepCheck_Analyzer` accepts it (`BRepCheck_Shell::Closed`: an edge on three or more face
    // sides faults only when the faces fall into more than one set joined by the other edges).
    let bore = fixture("captured/section_recesses/parts/349d4f2a44971104.step.gz");
    assert!(bore.solid_is_valid(0));
    // Two unit boxes meeting at one edge, sewn by OpenCascade into one shell and written as one
    // solid: `BRepCheck_InvalidMultiConnexity` before writing (OpenCascade's import heals the
    // file into two solids).
    let sewn = fixture("edge_sewn_boxes.step");
    assert_eq!(sewn.solids.len(), 1);
    assert!(!sewn.solid_is_valid(0));
}

#[test]
fn volume_probes_refuse_rather_than_read_unanswered_rays_as_air() {
    use haecceity::geom::{Bounds, Surface, SurfaceType};
    use haecceity::volume::{Probe, common_volume};
    // Box(30, 30, 20) - Cylinder(5, 20), with the bore's surface made one the rays cannot
    // intersect.
    let mut part = fixture("rejected_bored_box.step");
    let bore = (0..part.faces.len())
        .find(|&f| part.faces[f].surface.kind() == SurfaceType::Cylinder)
        .unwrap();
    part.faces[bore].surface = Surface::Other { kind: "test" };
    let solid = Classifier::new(&part);
    let probe = |min: [f64; 3], max: [f64; 3]| Probe::Box(Bounds { min, max });
    // Lines clear of the bore still answer: a 4 × 4 × 10 box in the material.
    let away = common_volume(&solid, &probe([8.0, 8.0, -5.0], [12.0, 12.0, 5.0])).unwrap();
    assert!((away - 160.0).abs() <= 1e-9 * 160.0, "{away}");
    // Lines through the bore cannot be resolved: no answer, rather than air where they fail.
    assert_eq!(
        common_volume(&solid, &probe([-8.0, -8.0, -5.0], [8.0, 8.0, 5.0])),
        None
    );
    // With every surface unresolvable, a box clear of every face is placed by its centre, which
    // the classifier leaves `Unknown`: no answer, not empty.
    let mut blind = fixture("rejected_bored_box.step");
    for face in &mut blind.faces {
        face.surface = Surface::Other { kind: "test" };
    }
    let solid = Classifier::new(&blind);
    assert_eq!(solid.classify([10.0, 10.0, 0.0]), State::Unknown);
    assert_eq!(
        common_volume(&solid, &probe([8.0, 8.0, -5.0], [12.0, 12.0, 5.0])),
        None
    );
}

/// Review M8's generic rotation (37° about (1, 2, 3)), then a non-round translation.
fn generic_placement() -> haecceity::step::Placement {
    let n = 14f64.sqrt();
    let [x, y, z] = [1.0 / n, 2.0 / n, 3.0 / n];
    let (s, c) = 37f64.to_radians().sin_cos();
    let t = 1.0 - c;
    [
        [t * x * x + c, t * x * y - s * z, t * x * z + s * y, 123.456],
        [t * x * y + s * z, t * y * y + c, t * y * z - s * x, -78.9],
        [t * x * z - s * y, t * y * z + s * x, t * z * z + c, 41.3],
    ]
}

/// A face's box is its trimmed face's, wherever the part sits: each coordinate range of the
/// moved face is the unmoved face's extent along the matching turned axis, to 1e-9 of the part.
/// cgb241 face 15 is a B-spline face whose surface runs far past it along its domain edge
/// u = u0, which the face's boundary follows (grid points there were in or out by round-off);
/// nist_ftc_06 faces 40 and 45 are cone frusta whose seam runs through the apex's parameter
/// (the apex was taken in one placement); threaded_connector_109 has many B-spline faces.
#[test]
fn face_boxes_move_with_the_part() {
    use haecceity::step::read_step_file_placed;
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let placement = generic_placement();
    for file in [
        "cadgenbench_inputs/cgb241.step",
        "nist/nist_ftc_06_asme1_rd.stp",
        "cadgenbench/threaded_connector_109.step",
    ] {
        let unmoved = read_step_file(&dir.join(file)).unwrap();
        let moved = read_step_file_placed(&dir.join(file), &placement).unwrap();
        let tol = 1e-9 * unmoved.bounds().diagonal();
        for face in 0..unmoved.faces.len() {
            let b = moved.face_bounds(face);
            for (k, row) in placement.iter().enumerate() {
                let (lo, hi) = unmoved.extent_along(&[face], [row[0], row[1], row[2]]);
                let (mlo, mhi) = (b.min[k] - row[3], b.max[k] - row[3]);
                assert!(
                    (mlo - lo).abs() <= tol && (mhi - hi).abs() <= tol,
                    "{file} face {face} axis {k}: moved {:?}, unmoved {:?}",
                    (mlo, mhi),
                    (lo, hi)
                );
            }
        }
    }
}

/// A face's box does not reach past the trimmed face: no further than a dense sample of the face
/// (its edges' samples and the points of a 200 × 200 grid its domain contains) goes, in either
/// placement. The cone frusta of nist_ftc_06 stop well short of their apex.
#[test]
fn face_boxes_stay_on_the_trimmed_face() {
    use haecceity::step::{IDENTITY, read_step_file_placed};
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    for (file, faces) in [
        ("cadgenbench_inputs/cgb241.step", &[15][..]),
        ("nist/nist_ftc_06_asme1_rd.stp", &[40, 45][..]),
    ] {
        for placement in [IDENTITY, generic_placement()] {
            let part = read_step_file_placed(&dir.join(file), &placement).unwrap();
            let tol = 1e-6 * part.bounds().diagonal();
            for &face in faces {
                let mut sample = haecceity::geom::Bounds::empty();
                for e in part.face_edges(face) {
                    part.edges[e].samples.iter().for_each(|&p| sample.add(p));
                }
                let (u0, u1, v0, v1) = part.uv_bounds(face).unwrap();
                let domain = part.domain(face).unwrap();
                let n = 200;
                for i in 0..=n {
                    for j in 0..=n {
                        let u = u0 + (u1 - u0) * i as f64 / n as f64;
                        let v = v0 + (v1 - v0) * j as f64 / n as f64;
                        if domain.contains(u, v) {
                            sample.add(part.faces[face].surface.value(u, v));
                        }
                    }
                }
                let b = part.face_bounds(face);
                for k in 0..3 {
                    assert!(
                        b.min[k] >= sample.min[k] - tol && b.max[k] <= sample.max[k] + tol,
                        "{file} face {face} axis {k}: box {:?}, face sample {:?}",
                        (b.min[k], b.max[k]),
                        (sample.min[k], sample.max[k])
                    );
                }
            }
        }
    }
}

/// Which analytic surface a B-spline face is recovered as does not depend on where the part
/// sits. threaded_connector_109 face 125 (next to a boss and a turned step) was a plane unmoved
/// only: the recovery grid's outer lines run along the face's boundary, where containment is
/// round-off, and a cone fitted to the plane's near-identical normals came within tolerance in
/// one placement.
#[test]
fn surface_recovery_moves_with_the_part() {
    use haecceity::geom::{Surface, SurfaceType};
    use haecceity::step::read_step_file_placed;
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let rot_zx_moved = [
        [0.0, 0.0, 1.0, 123.456],
        [1.0, 0.0, 0.0, -78.9],
        [0.0, 1.0, 0.0, 41.3],
    ];
    let file = "cadgenbench/threaded_connector_109.step";
    let unmoved = read_step_file(&dir.join(file)).unwrap();
    assert_eq!(
        unmoved.recovered(125).map(|s| s.kind()),
        Some(SurfaceType::Plane)
    );
    for placement in [rot_zx_moved, generic_placement()] {
        let moved = read_step_file_placed(&dir.join(file), &placement).unwrap();
        for face in 0..unmoved.faces.len() {
            if !matches!(unmoved.faces[face].surface, Surface::Freeform { .. }) {
                continue;
            }
            assert_eq!(
                unmoved.recovered(face).map(|s| s.kind()),
                moved.recovered(face).map(|s| s.kind()),
                "{file} face {face}"
            );
        }
    }
}

/// A face's parameter range is the range its boundary edges reach on its surface, each value
/// from tools/kernel_evidence.py uv (2001 samples per edge, extremes refined by Brent):
/// cone faces running to their apex, along an edge (cgb202 1262) or to a vertex loop (cgb243
/// 21, 104); a cylinder whose file pcurve stops 0.0107 short of its B-spline edge (cgb202
/// 1535) and one whose edge bulges between samples (1721); B-spline faces whose shared vertex
/// stands 0.02 mm off their edges' ends (1428, 1448); sphere patches whose edges stop a degree
/// from the pole (nist_ftc_07 49, 51); a cone pcurve running to the apex at another u (cgb243
/// 19); a B-spline edge whose samples missed its end (nist_ctc_05 79, cgb242 558); and a
/// B-spline face whose surface collapses at a corner of it, where v is arbitrary (cgb242 289,
/// to its samples' 2.4e-6). A pcurve constant in v fixes the edge's v (cgb217 106: the bore's
/// B-spline rim strays 0.0187 above the plane its pcurve, v = 24.5, gives).
#[test]
fn uv_bounds_are_the_range_of_the_boundary_edges() {
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    const PI: f64 = std::f64::consts::PI;
    // (file, face, side (u0, u1, v0, v1 as 0..4), value, tolerance)
    let cases: [(&str, usize, usize, f64, f64); 14] = [
        (
            "cadgenbench_inputs/cgb202.step.gz",
            1262,
            2,
            -4.958191942,
            1e-8,
        ),
        (
            "cadgenbench_inputs/cgb202.step.gz",
            1535,
            3,
            -115.0652464,
            1e-6,
        ),
        (
            "cadgenbench_inputs/cgb202.step.gz",
            1721,
            3,
            67.731421158,
            1e-8,
        ),
        (
            "cadgenbench_inputs/cgb202.step.gz",
            1428,
            2,
            0.00597859658,
            1e-7,
        ),
        (
            "cadgenbench_inputs/cgb202.step.gz",
            1448,
            0,
            0.005941117131,
            1e-9,
        ),
        (
            "cadgenbench_inputs/cgb243.step.gz",
            21,
            2,
            -1.458291747,
            1e-8,
        ),
        (
            "cadgenbench_inputs/cgb243.step.gz",
            104,
            2,
            -2.916583493,
            1e-8,
        ),
        ("cadgenbench_inputs/cgb243.step.gz", 19, 0, PI, 1e-9),
        ("nist/nist_ftc_07_asme1_rd.stp", 49, 3, 1.553343034, 1e-8),
        ("nist/nist_ftc_07_asme1_rd.stp", 51, 2, -1.553343034, 1e-8),
        ("nist/nist_ctc_05_asme1_rd.stp", 79, 1, 0.9030627005, 1e-9),
        (
            "cadgenbench_inputs/cgb242.step.gz",
            289,
            2,
            0.0058682224,
            3e-6,
        ),
        (
            "cadgenbench_inputs/cgb242.step.gz",
            558,
            0,
            -0.0001523131,
            1e-9,
        ),
        ("cadgenbench_inputs/cgb217.step.gz", 106, 3, 24.5, 1e-9),
    ];
    let mut parts = std::collections::BTreeMap::new();
    for (file, face, side, want, tol) in cases {
        let part = parts
            .entry(file)
            .or_insert_with(|| read_step_file(&dir.join(file)).unwrap());
        let (u0, u1, v0, v1) = part.uv_bounds(face).unwrap();
        let got = [u0, u1, v0, v1][side];
        assert!(
            (got - want).abs() <= tol,
            "{file} face {face} side {side}: {got} vs {want}"
        );
    }
}

/// The truck shaft's end caps are revolved B-spline quarters: each a whole closed (not
/// periodic) revolution, u in [0, 4], trimmed to one quarter u in [0, 1], its centre collapsed
/// at v = 1. The radial edge on u = 0 also lies on u = 4, and past the skipped centre round-off
/// picked which: moved by a right angle, face 19 read u in [0, 4] and its interior probes all
/// fell outside the face (written and read back, face 14 did instead). Every quarter spans its
/// own quarter, unmoved and moved.
#[test]
fn closed_bspline_seam_edges_take_their_loops_side() {
    let part = fixture("truck_master_shaft_only.step.gz");
    let right_angle: haecceity::step::Placement = [
        [0.0, -1.0, 0.0, 12.5],
        [0.0, 0.0, -1.0, -7.25],
        [1.0, 0.0, 0.0, 3.0],
    ];
    let moved = part.transformed(&right_angle).unwrap();
    for (name, p) in [("unmoved", &part), ("moved", &moved)] {
        for face in [4, 9, 14, 19] {
            let (u0, u1, _, _) = p.uv_bounds(face).unwrap();
            assert!(
                u0.abs() < 1e-9 && (u1 - 1.0).abs() < 1e-9,
                "{name} face {face}: u in [{u0}, {u1}]"
            );
        }
    }
}

/// A face's box reaches its edges' and its surface's extremes between their samples: cgb207's
/// y min lies on B-spline edge 452 (-41.1839642 at 4001 samples; its own samples stopped
/// 2.7e-4 short) and its z min inside B-spline face 22 (OpenCascade's mesh node -32.4992456;
/// the 13 × 13 grid stopped 3.1e-5 short); cgb203 face 89 bulges to y 127.997 (OpenCascade's
/// box, to its three decimals) where the grid stopped at 127.936.
#[test]
fn face_boxes_reach_extremes_between_samples() {
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let part = read_step_file(&dir.join("cadgenbench_inputs/cgb207.step")).unwrap();
    let b = part.solid_bounds(0);
    assert!((b.min[1] + 41.1839642).abs() < 1e-6, "{:?}", b.min);
    assert!((b.min[2] + 32.4992456).abs() < 1e-6, "{:?}", b.min);
    let part = read_step_file(&dir.join("cadgenbench_inputs/cgb203.step")).unwrap();
    let max_y = part.face_bounds(89).max[1];
    assert!((max_y - 127.997).abs() < 5e-4, "{max_y}");
}

/// An analytic face's box is its surface's, not that of an edge straying from it within its
/// tolerance: cgb202's torus face 1870 (minor radius 5 about y = -102.5) reaches no lower than
/// y = -107.5, though its B-spline edge 1077 dips to -107.512205; nist_ctc_03's plane face 8 lies
/// at z = 76.2, though its edge 88 runs 4.6e-6 above it.
#[test]
fn analytic_face_boxes_stay_on_their_surface() {
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let part = read_step_file(&dir.join("cadgenbench_inputs/cgb202.step.gz")).unwrap();
    let min_y = part.face_bounds(1870).min[1];
    assert!((min_y + 107.5).abs() < 1e-9, "{min_y}");
    let part = read_step_file(&dir.join("nist/nist_ctc_03_asme1_rc.stp")).unwrap();
    let max_z = part.face_bounds(8).max[2];
    assert!((max_z - 76.2).abs() < 1e-9, "{max_z}");
}
