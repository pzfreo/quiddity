//! Central moments and rigid motion, without a Python-output target.
mod common;

use haecceity::geom::{self, Surface};
use haecceity::step::{IDENTITY, Placement, read_step_file_placed, read_step_placed};

fn motion(translate: bool) -> Placement {
    let n = geom::unit([1.0, 2.0, 3.0]).unwrap();
    let (s, c) = 37f64.to_radians().sin_cos();
    let k = 1.0 - c;
    let [x, y, z] = n;
    let t = if translate {
        [123.456, -78.9, 41.3]
    } else {
        [0.0; 3]
    };
    [
        [c + x * x * k, x * y * k - z * s, x * z * k + y * s, t[0]],
        [y * x * k + z * s, c + y * y * k, y * z * k - x * s, t[1]],
        [z * x * k - y * s, z * y * k + x * s, c + z * z * k, t[2]],
    ]
}

fn rotate(p: [f64; 3], m: &Placement) -> [f64; 3] {
    std::array::from_fn(|i| (0..3).map(|j| m[i][j] * p[j]).sum())
}

fn tensor(q: [[f64; 3]; 3], m: &Placement) -> [[f64; 3]; 3] {
    std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            (0..3)
                .flat_map(|k| (0..3).map(move |l| m[i][k] * q[k][l] * m[j][l]))
                .sum()
        })
    })
}

/// Green integration of the four STEP-selected pcurves on their own carriers
/// gives the same area at 12, 24 and 48 Gauss nodes per knot span. One endpoint
/// on the adjacent rational spline has v = -3.67598317687e-14 at its lower knot.
/// On sphere #151680 another carrier ends at u = 1.000000000001 for an upper
/// knot of 1. Its three radially projected pcurves enclose 3.54881920497864 mm²
/// by an independent spherical solid-angle integral in two coordinate frames.
#[test]
fn rounded_foreign_spline_endpoints_keep_their_face_trims_under_motion() {
    let Some(corpus) = common::corpus_dir() else {
        return;
    };
    let path = corpus.join("cadgenbench_inputs/cgb202.step.gz");
    let area = 25.67657558531589;
    let centre = [112.71217493348577, -159.73551391711212, -94.228115143539];
    let translation = [
        [1., 0., 0., 123.456],
        [0., 1., 0., -78.9],
        [0., 0., 1., 41.3],
    ];
    for (first, m) in [IDENTITY, translation, motion(false), motion(true)]
        .into_iter()
        .enumerate()
    {
        let part = read_step_file_placed(&path, &m).unwrap();
        let f = 684;
        assert_eq!(part.face_source(f).unwrap().entity, 104_265);
        let got = part.face_moments(f).unwrap();
        let placed = geom::add(rotate(centre, &m), m.map(|row| row[3]));
        assert!((got.area - area).abs() < 1e-9);
        assert!((part.face_mass(f).unwrap()[0] - area).abs() < 1e-9);
        assert!(geom::dist(got.centroid, placed) < 1e-8);

        let sphere = 1031;
        let sphere_area = 3.54881920497864;
        assert_eq!(part.face_source(sphere).unwrap().entity, 151_680);
        if first % 2 == 1 {
            assert!((part.face_moments(sphere).unwrap().area - sphere_area).abs() < 1e-9);
        }
        assert!((part.face_mass(sphere).unwrap()[0] - sphere_area).abs() < 1e-9);
        assert!((part.face_moments(sphere).unwrap().area - sphere_area).abs() < 1e-9);
    }
    let spatial = common::with_spatial_edges(
        &path,
        &[26, 104_128, 104_271, 104_293].into_iter().collect(),
    );
    assert!((spatial.face_mass(684).unwrap()[0] - 25.67737528249655).abs() < 1e-9);
}

#[test]
fn hemisphere_and_disk_have_analytic_central_moments() {
    let text = std::fs::read(common::fixtures().join("hemisphere_one_edge.step")).unwrap();
    let r: f64 = 12.7;
    let pi = std::f64::consts::PI;
    for m in [IDENTITY, motion(true)] {
        let part = read_step_placed(&text, &m).unwrap();
        for f in 0..part.faces.len() {
            let got = part.face_moments(f).unwrap();
            let sphere = matches!(part.faces[f].surface, Surface::Sphere { .. });
            let area = if sphere { 2.0 } else { 1.0 } * pi * r * r;
            let centroid = if sphere {
                [0.0, -r / 2.0, 0.0]
            } else {
                [0.0; 3]
            };
            // Hemisphere about its centroid: tangential r²/3, normal r²/12.
            // A planar disk: tangential r²/4, zero normal.
            let q = if sphere {
                [
                    [area * r * r / 3.0, 0.0, 0.0],
                    [0.0, area * r * r / 12.0, 0.0],
                    [0.0, 0.0, area * r * r / 3.0],
                ]
            } else {
                [
                    [area * r * r / 4.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0],
                    [0.0, 0.0, area * r * r / 4.0],
                ]
            };
            let centre = geom::add(rotate(centroid, &m), [m[0][3], m[1][3], m[2][3]]);
            let q = tensor(q, &m);
            assert!((got.area - area).abs() < 1e-10 * area, "face {f}: {got:?}");
            assert!(
                geom::dist(got.centroid, centre) < 1e-10 * r,
                "face {f}: {got:?} expected {centre:?}"
            );
            for (row, want) in got.second_moments.iter().zip(q) {
                for (value, want) in row.iter().zip(want) {
                    assert!(
                        (value - want).abs() < 1e-10 * area * r * r,
                        "face {f}: {got:?} expected {q:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn whole_sphere_has_isotropic_second_moments() {
    let text = std::fs::read(common::fixtures().join("hemisphere_one_edge.step")).unwrap();
    for m in [IDENTITY, motion(true)] {
        let mut part = read_step_placed(&text, &m).unwrap();
        let Surface::Sphere { frame, radius } = part.faces[0].surface else {
            panic!("sphere")
        };
        let pole = geom::add(frame.origin, geom::scale(frame.z, radius));
        part.faces[0].loops[0].edges.clear();
        part.faces[0].loops[0].vertex = Some(pole);
        let got = part.face_moments(0).unwrap();
        let area = 4.0 * std::f64::consts::PI * radius * radius;
        assert_eq!(got.area, area);
        assert_eq!(got.centroid, frame.origin);
        for i in 0..3 {
            for j in 0..3 {
                let want = if i == j {
                    area * radius * radius / 3.0
                } else {
                    0.0
                };
                assert!((got.second_moments[i][j] - want).abs() < 1e-12 * area);
            }
        }
    }
}

#[test]
fn every_corpus_face_moves_its_moments_rigidly() {
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let corpus = common::load("corpus.json");
    let files = corpus["files"].as_array().unwrap();
    let problems = common::parallel::map(files, |entry| {
        let file = entry["file"].as_str().unwrap();
        let part = read_step_file_placed(&dir.join(file), &IDENTITY).unwrap();
        let mut shift = IDENTITY;
        for (row, t) in shift.iter_mut().zip([123.456, -78.9, 41.3]) {
            row[3] = t;
        }
        let original: Vec<_> = (0..part.faces.len())
            .map(|f| part.face_moments(f))
            .collect();
        let mut errors = Vec::new();
        for m in [shift, motion(false)] {
            let moved = read_step_file_placed(&dir.join(file), &m).unwrap();
            for (f, first) in original.iter().enumerate() {
                let (Some(a), Some(b)) = (first, moved.face_moments(f)) else {
                    errors.push(format!("{file} face {f}: missing moments"));
                    continue;
                };
                let length = geom::dist(part.face_bounds(f).min, part.face_bounds(f).max).max(1.0);
                let centre = geom::add(rotate(a.centroid, &m), [m[0][3], m[1][3], m[2][3]]);
                let q = tensor(a.second_moments, &m);
                let qscale = (0..3).map(|i| a.second_moments[i][i]).sum::<f64>().abs();
                let mass = part.face_mass(f).unwrap();
                let moved_mass = moved.face_mass(f).unwrap();
                let sense = if part.faces[f].reversed { -1.0 } else { 1.0 };
                let flux = mass[1]
                    + sense * geom::dot(rotate(a.area_vector, &m), [m[0][3], m[1][3], m[2][3]]);
                if (a.area - b.area).abs() > 1e-9 * a.area + 1e-9
                    || geom::dist(centre, b.centroid) > 1e-6 * length
                    || (0..3).any(|i| {
                        (0..3).any(|j| {
                            (q[i][j] - b.second_moments[i][j]).abs() > 1e-6 * qscale + 1e-9
                        })
                    })
                    || (mass[0] - moved_mass[0]).abs() > 1e-9 * mass[0] + 1e-9
                    || (flux - moved_mass[1]).abs() > 1e-6 * a.area * length + 1e-8
                {
                    errors.push(format!("{file} face {f}: rigid motion failed: {a:?} -> {b:?}; expected centroid {centre:?}, second {q:?}, flux {flux} got {}",moved_mass[1]));
                }
            }
        }
        errors
    });
    let errors: Vec<_> = problems.into_iter().flatten().collect();
    assert!(
        errors.is_empty(),
        "{} failures:\n{}",
        errors.len(),
        errors.join("\n")
    );
}

/// Independent tensor-product Simpson integration of the analytic trimming rectangle of
/// held_side_knot.step. Integration along u is exact (S is linear in u); along v it is
/// split at the derivative discontinuity. This exercises B-spline moments at a held side.
#[test]
fn bspline_held_side_has_independently_integrated_second_moments() {
    let text = std::fs::read(common::fixtures().join("held_side_knot.step")).unwrap();
    let part = read_step_placed(&text, &IDENTITY).unwrap();
    let surface = &part.faces[0].surface;
    let mut sums = [0.0; 10];
    for (a, b) in [(0.2, 0.5), (0.5, 0.8)] {
        let n = 4000;
        let h = (b - a) / n as f64;
        for i in 0..=n {
            // The knot is continuous in position and its area density; either partial at
            // the shared end is suitable for Simpson's rule.
            let v = a + h * i as f64;
            let wv = if i == 0 || i == n {
                1.0
            } else if i % 2 == 0 {
                2.0
            } else {
                4.0
            };
            for (u, wu) in [(0.0, 1.0), (0.25, 4.0), (0.5, 1.0)] {
                let p = surface.value(u, v);
                let (su, sv) = surface.partials(u, v);
                let weight = geom::norm(geom::cross(su, sv)) * wv * wu * h / 3.0 * 0.5 / 6.0;
                let [x, y, z] = p;
                let values = [1.0, x, y, z, x * x, y * y, z * z, x * y, x * z, y * z];
                for (sum, value) in sums.iter_mut().zip(values) {
                    *sum += weight * value;
                }
            }
        }
    }
    let area = sums[0];
    let centre = [sums[1] / area, sums[2] / area, sums[3] / area];
    let raw = [
        [sums[4], sums[7], sums[8]],
        [sums[7], sums[5], sums[9]],
        [sums[8], sums[9], sums[6]],
    ];
    let q =
        std::array::from_fn(|i| std::array::from_fn(|j| raw[i][j] - area * centre[i] * centre[j]));
    for m in [IDENTITY, motion(true)] {
        let placed = read_step_placed(&text, &m).unwrap();
        let got = placed.face_moments(0).unwrap();
        let q = tensor(q, &m);
        let centre = geom::add(rotate(centre, &m), [m[0][3], m[1][3], m[2][3]]);
        assert!(
            (got.area - area).abs() < 1e-8 * area,
            "{got:?} expected {area}"
        );
        assert!(geom::dist(got.centroid, centre) < 1e-8);
        for (row, want) in got.second_moments.iter().zip(q) {
            for (value, want) in row.iter().zip(want) {
                assert!((value - want).abs() < 1e-8 * area, "{got:?} expected {q:?}");
            }
        }
    }
}

/// The spatial controls use the 3D boundary on which their reference was measured.
/// OCCT evaluation over the 3D boundary, with 16 and 32 Lobatto panels per span, independently
/// confirms every entry to 1.2e-12 of the tensor trace (the ledger records each error). This
/// regression runs in CI without adding a Python capture or requiring an OCCT installation.
#[test]
fn second_moments_match_independently_verified_boundaries() {
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let known = common::load("references/face_integrals.json");
    let rows = known.as_array().unwrap();
    common::check_verdicts("references/face_integrals.json", rows);
    let mut groups = std::collections::BTreeMap::<String, Vec<_>>::new();
    for row in rows {
        groups
            .entry(row["file"].as_str().unwrap().to_string())
            .or_default()
            .push(row);
    }
    let groups: Vec<_> = groups.into_iter().collect();
    let _ = common::parallel::map(&groups, |(file, rows)| {
        let part = read_step_file_placed(&dir.join(file), &IDENTITY).unwrap();
        let edges: std::collections::BTreeSet<u64> = rows
            .iter()
            .filter_map(|row| row["spatial_control_edges"].as_array())
            .flatten()
            .map(|edge| edge.as_u64().unwrap())
            .collect();
        let spatial =
            (!edges.is_empty()).then(|| common::with_spatial_edges(&dir.join(file), &edges));
        for row in rows {
            let face = row["face"].as_u64().unwrap() as usize;
            let m = part
                .face_moments(face)
                .expect("independently verified integral");
            let check = |m: haecceity::mass::FaceMoments, pin: &serde_json::Value| {
                let area = pin["area"].as_f64().unwrap();
                assert!((m.area - area).abs() < 1e-7 * area, "{file} face {face}");
                let trace: f64 = (0..3)
                    .map(|i| pin["second_moments"][i][i].as_f64().unwrap())
                    .sum();
                for (i, &c) in m.centroid.iter().enumerate() {
                    let want = pin["centroid"][i].as_f64().unwrap();
                    assert!(
                        (c - want).abs() < 1e-7 * want.abs().max(1.0),
                        "{file} face {face}: centroid {m:?}"
                    );
                }
                for (i, values) in m.second_moments.iter().enumerate() {
                    for (j, &value) in values.iter().enumerate() {
                        let want = pin["second_moments"][i][j].as_f64().unwrap();
                        assert!(
                            (value - want).abs() < 1e-7 * trace,
                            "{file} face {face}: second moments {m:?}"
                        );
                    }
                }
            };
            if row["spatial_control_edges"].is_array() {
                check(m, &row["step_master"]);
                check(
                    spatial.as_ref().unwrap().face_moments(face).unwrap(),
                    &row["rust"],
                );
            } else {
                check(m, &row["rust"]);
            }
        }
    });
}

/// The area of the collapsed-tip sliver (face 483) is finite, but its quadratic flux terms
/// about the solid's centre used to amplify round-off enough to refuse the entire centroid
/// after translation. The material frame must remain measurable in all these placements.
#[test]
fn solid_centroid_flux_moments_stay_measured_under_motion() {
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let path = dir.join("cadgenbench_inputs/cgb242.step.gz");
    let part = read_step_file_placed(&path, &IDENTITY).unwrap();
    let (volume, centre) = part.volume_centroid().expect("unmoved centroid");
    let bounds = part.bounds();
    let length = geom::dist(bounds.min, bounds.max);
    let mut shift = IDENTITY;
    for (row, t) in shift.iter_mut().zip([123.456, -78.9, 41.3]) {
        row[3] = t;
    }
    let cycle = [
        [0.0, 0.0, 1.0, 123.456],
        [1.0, 0.0, 0.0, -78.9],
        [0.0, 1.0, 0.0, 41.3],
    ];
    for m in [shift, cycle, motion(true)] {
        let placed = read_step_file_placed(&path, &m).unwrap();
        let (moved_volume, moved_centre) = placed.volume_centroid().expect("moved centroid");
        let want = geom::add(rotate(centre, &m), [m[0][3], m[1][3], m[2][3]]);
        assert!(
            (moved_volume - volume).abs() <= 1e-9 * volume.abs(),
            "{moved_volume} vs {volume}"
        );
        assert!(
            geom::dist(want, moved_centre) <= 1e-9 * length,
            "{moved_centre:?} vs {want:?}"
        );
    }
}
