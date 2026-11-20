//! A loaded, already queried part must transform like the STEP reader, including topology,
//! analytic parameters, NURBS, bounds, areas, moments and classification structures.
#[path = "common/parallel.rs"]
mod parallel;

use std::path::{Path, PathBuf};

use quiddity::Part;
use quiddity::kernel::brep::Pcurve;
use quiddity::kernel::geom::{Curve, Frame, Surface, V3};
use quiddity::kernel::step::{IDENTITY, Placement, read_step_file_placed};
use sha2::{Digest, Sha256};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files(&path, out);
        } else if path.to_str().is_some_and(|s| {
            s.ends_with(".step")
                || s.ends_with(".step.gz")
                || s.ends_with(".stp")
                || s.ends_with(".stp.gz")
        }) {
            out.push(path);
        }
    }
}

fn near(a: f64, b: f64, scale: f64) {
    assert!(
        a == b
            || (a.is_nan() && b.is_nan())
            || (a - b).abs() <= 1e-8 * scale.max(a.abs()).max(b.abs()).max(1.0),
        "{a} != {b} (scale {scale})"
    );
}
fn vector(a: V3, b: V3, scale: f64) {
    for i in 0..3 {
        near(a[i], b[i], scale);
    }
}
// Conic-to-NURBS conversion can change a weight or knot by one ULP when the
// reader derives the trimmed conic from placed endpoints instead of moving its poles.
fn scalars(a: &[f64], b: &[f64]) {
    assert_eq!(a.len(), b.len());
    for (&a, &b) in a.iter().zip(b) {
        near(a, b, 1.0);
    }
}
fn basis(a: &Frame, b: &Frame, scale: f64) {
    vector(a.origin, b.origin, scale);
    for (a, b) in [(a.x, b.x), (a.y, b.y), (a.z, b.z)] {
        vector(a, b, 1.0);
    }
}
fn surface(a: &Surface, b: &Surface, scale: f64) {
    assert_eq!(a.kind(), b.kind());
    match (a, b) {
        (Surface::Plane { frame: a }, Surface::Plane { frame: b }) => basis(a, b, scale),
        (
            Surface::Cylinder {
                frame: a,
                radius: r,
            },
            Surface::Cylinder {
                frame: b,
                radius: s,
            },
        )
        | (
            Surface::Sphere {
                frame: a,
                radius: r,
            },
            Surface::Sphere {
                frame: b,
                radius: s,
            },
        ) => {
            basis(a, b, scale);
            near(*r, *s, scale);
        }
        (
            Surface::Cone {
                frame: a,
                radius: r,
                semi_angle: t,
            },
            Surface::Cone {
                frame: b,
                radius: s,
                semi_angle: u,
            },
        ) => {
            basis(a, b, scale);
            near(*r, *s, scale);
            near(*t, *u, 1.0);
        }
        (
            Surface::Torus {
                frame: a,
                major: r,
                minor: t,
            },
            Surface::Torus {
                frame: b,
                major: s,
                minor: u,
            },
        ) => {
            basis(a, b, scale);
            near(*r, *s, scale);
            near(*t, *u, scale);
        }
        (
            Surface::Freeform {
                kind: a,
                surface: s,
            },
            Surface::Freeform {
                kind: b,
                surface: t,
            },
        ) => {
            assert_eq!(a, b);
            assert_eq!((s.degree_u, s.degree_v), (t.degree_u, t.degree_v));
            assert_eq!(s.weights, t.weights);
            assert_eq!(s.knots_u, t.knots_u);
            assert_eq!(s.knots_v, t.knots_v);
            assert_eq!(s.control_points.len(), t.control_points.len());
            for (a, b) in s.control_points.iter().zip(&t.control_points) {
                assert_eq!(a.len(), b.len());
                for (a, b) in a.iter().zip(b) {
                    vector(*a, *b, scale);
                }
            }
        }
        (Surface::Other { kind: a }, Surface::Other { kind: b }) => assert_eq!(a, b),
        _ => panic!("different surfaces"),
    }
}
fn compare(a: &Part, b: &Part) {
    assert_eq!(
        (a.faces.len(), a.edges.len(), a.solids.len()),
        (b.faces.len(), b.edges.len(), b.solids.len())
    );
    assert_eq!(a.input_repairs(), b.input_repairs());
    assert_eq!(a.unresolved_faces(), b.unresolved_faces());
    assert_eq!(a.unresolved_edges(), b.unresolved_edges());
    let scale = b.bounds().max_extent().max(1.0);
    for (i, (a_face, b_face)) in a.faces.iter().zip(&b.faces).enumerate() {
        assert_eq!(a.face_source(i), b.face_source(i));
        assert_eq!(a.face_edges(i), b.face_edges(i));
        assert_eq!(
            (a_face.reversed, a_face.solid),
            (b_face.reversed, b_face.solid)
        );
        surface(&a_face.surface, &b_face.surface, scale);
        assert_eq!(a_face.pcurves.len(), b_face.pcurves.len());
        for ((edge_a, a), (edge_b, b)) in a_face.pcurves.iter().zip(&b_face.pcurves) {
            assert_eq!(edge_a, edge_b);
            match (a, b) {
                (Pcurve::Poles(a), Pcurve::Poles(b)) => {
                    assert_eq!(a.len(), b.len());
                    for (a, b) in a.iter().zip(b) {
                        scalars(&[a.0, a.1], &[b.0, b.1]);
                    }
                }
                (Pcurve::Line { point: a, dir: c }, Pcurve::Line { point: b, dir: d }) => {
                    scalars(&[a.0, a.1, c.0, c.1], &[b.0, b.1, d.0, d.1]);
                }
                _ => panic!("face {i}: pcurve type differs"),
            }
        }
        assert_eq!(a_face.loops.len(), b_face.loops.len());
        for (a, b) in a_face.loops.iter().zip(&b_face.loops) {
            assert_eq!(a.edges, b.edges);
            assert_eq!(a.vertex.is_some(), b.vertex.is_some());
            if let (Some(a), Some(b)) = (a.vertex, b.vertex) {
                vector(a, b, scale);
            }
        }
        match (a.face_moments(i), b.face_moments(i)) {
            (Some(a), Some(b)) => {
                near(a.area, b.area, scale * scale);
                vector(a.centroid, b.centroid, scale);
                vector(a.area_vector, b.area_vector, scale * scale);
                for i in 0..3 {
                    vector(a.second_moments[i], b.second_moments[i], scale.powi(4));
                }
            }
            (None, None) => {}
            _ => panic!("face {i}: integration availability differs"),
        }
        let (x, y) = (a.face_bounds(i), b.face_bounds(i));
        vector(x.min, y.min, scale);
        vector(x.max, y.max, scale);
    }
    for (i, (a, b)) in a.edges.iter().zip(&b.edges).enumerate() {
        assert_eq!((a.vertices, a.same_sense), (b.vertices, b.same_sense));
        vector(a.start, b.start, scale);
        vector(a.end, b.end, scale);
        assert_eq!(
            a.samples.len(),
            b.samples.len(),
            "edge {i}: sampling density differs"
        );
        for (a, b) in a.samples.iter().zip(&b.samples) {
            vector(*a, *b, scale);
        }
        match (&a.curve, &b.curve) {
            (Curve::Line { origin: a, dir: c }, Curve::Line { origin: b, dir: d }) => {
                vector(*a, *b, scale);
                vector(*c, *d, 1.0);
            }
            (
                Curve::Circle {
                    frame: a,
                    radius: r,
                },
                Curve::Circle {
                    frame: b,
                    radius: s,
                },
            ) => {
                basis(a, b, scale);
                near(*r, *s, scale);
            }
            (
                Curve::Ellipse {
                    frame: a,
                    major: r,
                    minor: t,
                },
                Curve::Ellipse {
                    frame: b,
                    major: s,
                    minor: u,
                },
            ) => {
                basis(a, b, scale);
                near(*r, *s, scale);
                near(*t, *u, scale);
            }
            (Curve::Nurbs(a), Curve::Nurbs(b)) => {
                assert_eq!(a.degree(), b.degree());
                scalars(a.weights(), b.weights());
                scalars(a.knots(), b.knots());
                assert_eq!(a.is_closed(), b.is_closed());
                assert_eq!(a.control_points().len(), b.control_points().len());
                for (a, b) in a.control_points().iter().zip(b.control_points()) {
                    vector(*a, *b, scale);
                }
            }
            _ => panic!("edge {i}: curve type differs"),
        }
    }
    assert_eq!(a.edge_faces(), b.edge_faces());
    for (i, (x, y)) in a.solids.iter().zip(&b.solids).enumerate() {
        assert_eq!(x.faces, y.faces);
        match (a.solid_mass(i), b.solid_mass(i)) {
            (Some((v, e)), Some((w, f))) => {
                near(v, w, scale.powi(3));
                near(e, f, scale.powi(3));
            }
            (None, None) => {}
            _ => panic!("solid mass availability differs"),
        }
        assert_eq!(a.bad_faces(i), b.bad_faces(i));
        if a.solids.len() > 1 {
            // Isolate each solid's volume first moments; whole-part moments could hide
            // equal and opposite errors in different assembly instances.
            let isolated = |part: &Part| {
                Part::new(
                    part.faces.clone(),
                    part.edges.clone(),
                    vec![part.solids[i].clone()],
                )
            };
            match (isolated(a).volume_centroid(), isolated(b).volume_centroid()) {
                (Some((v, c)), Some((w, d))) => {
                    near(v, w, scale.powi(3));
                    vector(c, d, scale);
                }
                (None, None) => {}
                _ => panic!("solid {i}: first-moment availability differs"),
            }
        }
    }
    match (a.volume_centroid(), b.volume_centroid()) {
        (Some((v, c)), Some((w, d))) => {
            near(v, w, scale.powi(3));
            vector(c, d, scale);
        }
        (None, None) => {}
        _ => panic!("volume moments availability differs"),
    }
}

fn recognition_bytes(part: &Part) -> Result<Vec<u8>, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let recognition = quiddity::correspondence::recognise(part);
        let document = quiddity::recognition::document(&recognition, part.faces.len());
        serde_json::to_vec(&(recognition, document)).unwrap()
    }))
    .map_err(|e| {
        e.downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_else(|| "non-string recognition panic".into())
    })
}

/// Corpus parts whose recognition is also compared: a costly one and the two whose frame choice
/// is closest (#91). The fixtures are all compared; every corpus part's recognition is pinned by
/// the corpus snapshot and by `the_default_document_moves_with_the_part`.
const RECOGNISED_CORPUS: [&str; 3] = ["cgb203.step", "cgb217.step.gz", "cgb241.step"];

#[test]
fn loaded_transform_matches_reader() {
    let mut paths = Vec::new();
    files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"),
        &mut paths,
    );
    if let Some(dir) = std::env::var_os("QUIDDITY_CORPUS").filter(|s| s != "off") {
        files(Path::new(&dir), &mut paths);
    }
    // The captured query inputs kept from the Python port add files, not geometry kinds.
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    paths.retain(|path| !path.starts_with(fixtures.join("captured")));
    // One reading per distinct file (some parts are committed in several places), largest
    // first: the costliest parts take minutes each, so starting them last leaves threads idle.
    let mut seen = std::collections::BTreeSet::new();
    let mut paths: Vec<(u64, PathBuf)> = paths
        .into_iter()
        .filter_map(|path| {
            let bytes = std::fs::read(&path).unwrap();
            seen.insert(Sha256::digest(&bytes).to_vec())
                .then_some((bytes.len() as u64, path))
        })
        .collect();
    paths.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let paths: Vec<PathBuf> = paths.into_iter().map(|(_, path)| path).collect();
    let (s, c) = 0.37_f64.sin_cos();
    let (t, d) = 0.61_f64.sin_cos();
    let oblique: Placement = [
        [c, -s, 0.0, 17.0],
        [d * s, d * c, -t, -29.0],
        [t * s, t * c, d, 41.0],
    ];
    let done = std::sync::atomic::AtomicUsize::new(0);
    let failed = std::sync::atomic::AtomicBool::new(false);
    let checked = parallel::map(&paths, |path| {
        if failed.load(std::sync::atomic::Ordering::Relaxed) {
            return false;
        }
        eprintln!("transform equivalence: begin {}", path.display());
        let Ok(part) = read_step_file_placed(path, &IDENTITY) else {
            return false;
        };
        // Force geometry-dependent caches before copying and transforming the loaded part.
        for i in 0..part.faces.len() {
            let _ = part.face_moments(i);
            let _ = part.face_bounds(i);
            let _ = part.recovered(i);
        }
        let _ = part.volume_centroid();
        let _ = part.edge_faces();
        for i in 0..part.solids.len() {
            let _ = part.bad_faces(i);
        }
        eprintln!("transform equivalence: caches warmed {}", path.display());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // Two real motions, which also check the copied caches: an exact quarter turn and a
            // generic one.
            for at in [
                [
                    [0.0, -1.0, 0.0, 11.0],
                    [1.0, 0.0, 0.0, 23.0],
                    [0.0, 0.0, 1.0, -37.0],
                ],
                oblique,
            ] {
                let moved = part.transformed(&at).unwrap();
                let read = read_step_file_placed(path, &at).unwrap();
                compare(&moved, &read);
                eprintln!(
                    "transform equivalence: geometry checked {} at {at:?}",
                    path.display()
                );
                // Recognition exercises production's transformed normalisation.
                let named = path
                    .file_name()
                    .is_some_and(|name| RECOGNISED_CORPUS.iter().any(|n| name == *n));
                if !path.starts_with(&fixtures) && !named {
                    continue;
                }
                let (a, b) = (recognition_bytes(&moved), recognition_bytes(&read));
                match (&a, &b) {
                    (Ok(a), Ok(b)) if a != b => {
                        let offset = a
                            .iter()
                            .zip(b)
                            .position(|(a, b)| a != b)
                            .unwrap_or(a.len().min(b.len()));
                        panic!(
                            "recognition differs under {at:?} at byte {offset}: {:?} vs {:?}",
                            String::from_utf8_lossy(
                                &a[offset.saturating_sub(40)..a.len().min(offset + 80)]
                            ),
                            String::from_utf8_lossy(
                                &b[offset.saturating_sub(40)..b.len().min(offset + 80)]
                            )
                        );
                    }
                    (Ok(_), Ok(_)) => {}
                    _ => assert_eq!(a, b, "recognition refusal differs"),
                }
                eprintln!(
                    "transform equivalence: recognition checked {} at {at:?}",
                    path.display()
                );
            }
        }));
        if result.is_err() {
            failed.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        assert!(result.is_ok(), "{}", path.display());
        let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        if n.is_multiple_of(100) {
            eprintln!(
                "transform equivalence: {n}/{} readable parts checked",
                paths.len()
            );
        }
        true
    })
    .into_iter()
    .filter(|v| *v)
    .count();
    eprintln!(
        "rigid transform: {checked} distinct readable fixture/corpus parts, two placements each"
    );
    assert!(checked > 100);
}

#[test]
fn refuses_reflection_scale_shear_and_nonfinite_placements() {
    let part = Part::new(vec![], vec![], vec![]);
    for at in [
        [
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
        [
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 0.5, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
        [
            [1.0, 0.1, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
        [
            [1.0, 0.0, 0.0, f64::NAN],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
    ] {
        assert!(part.transformed(&at).is_err());
        assert!(
            quiddity::kernel::step::read_step_placed(b"", &at)
                .unwrap_err()
                .to_string()
                .contains("proper rotation")
        );
    }
}

#[test]
fn constructed_geometry_keeps_line_parameters_and_indirect_frames() {
    use quiddity::kernel::brep::{Edge, Face, Loop};
    let frame = Frame {
        origin: [1.0, 2.0, 3.0],
        x: [1.0, 0.0, 0.0],
        y: [0.0, -1.0, 0.0],
        z: [0.0, 0.0, 1.0],
    };
    let part = Part::new(
        vec![Face {
            surface: Surface::Plane { frame },
            reversed: false,
            loops: vec![Loop {
                edges: vec![(0, true)],
                vertex: Some([1.0, 2.0, 3.0]),
            }],
            solid: None,
            pcurves: vec![],
        }],
        vec![Edge {
            curve: Curve::Line {
                origin: [1.0, 2.0, 3.0],
                dir: [2.0, 0.0, 0.0],
            },
            start: [1.0, 2.0, 3.0],
            end: [5.0, 2.0, 3.0],
            vertices: (0, 1),
            same_sense: true,
            samples: vec![[1.0, 2.0, 3.0], [5.0, 2.0, 3.0]],
        }],
        vec![],
    );
    let moved = part
        .transformed(&[
            [0.0, -1.0, 0.0, 11.0],
            [1.0, 0.0, 0.0, 23.0],
            [0.0, 0.0, 1.0, -37.0],
        ])
        .unwrap();
    assert_eq!(moved.edges[0].curve.value(0.25), [9.0, 24.5, -34.0]);
    assert_eq!(moved.edges[0].vertices, (0, 1));
    let Surface::Plane { frame } = &moved.faces[0].surface else {
        panic!("lost plane");
    };
    assert!(!frame.direct());
    assert_eq!(frame.y, [1.0, 0.0, 0.0]);
    assert_eq!(moved.faces[0].loops[0].vertex, Some([9.0, 24.0, -34.0]));
}

#[test]
fn composed_and_preplaced_transforms_match_one_composed_read() {
    let (s, c) = 0.37_f64.sin_cos();
    let (t, d) = 0.61_f64.sin_cos();
    let first = [[c, -s, 0., 17.], [s, c, 0., -29.], [0., 0., 1., 41.]];
    let second = [[1., 0., 0., -11.], [0., d, -t, 23.], [0., t, d, -37.]];
    let mut composed = [[0.; 4]; 3];
    for (i, row) in composed.iter_mut().enumerate() {
        for (j, value) in row.iter_mut().enumerate() {
            *value = (0..3).map(|k| second[i][k] * first[k][j]).sum::<f64>();
        }
        row[3] += second[i][3];
    }
    for name in [
        "large_block_small_fillets.step",
        "cast_bearing_unit.step.gz",
        "hyperbola_cut_cone.step",
        "ap242/assembly/nested.step",
        "revolution_cone.step.gz",
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let read = read_step_file_placed(&path, &composed).unwrap();
        let original = read_step_file_placed(&path, &IDENTITY).unwrap();
        let twice = original
            .transformed(&first)
            .unwrap()
            .transformed(&second)
            .unwrap();
        let placed = read_step_file_placed(&path, &first)
            .unwrap()
            .transformed(&second)
            .unwrap();
        for part in [twice, placed] {
            compare(&part, &read);
            assert_eq!(recognition_bytes(&part), recognition_bytes(&read), "{name}");
        }
    }
}
