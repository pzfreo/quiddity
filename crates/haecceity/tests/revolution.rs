//! Issue #53. The truck fixtures are byte-for-byte copies of the master and 0.6.0
//! exports. The independent fixtures were written by hand: exact quadrant circles
//! about +X, and degree-one B-spline meridians swept by SURFACE_OF_REVOLUTION.
//! Shaft meridian (x,r): (0,0), (0,15), (40,15), (40,10), (100,10), (100,0).
//! Cone meridian: (0,0), (0,3), (4,0). Neither uses truck or an OCCT exporter.
//! revolution_line_shaft has the same exact quadrant topology but unbounded LINE
//! meridians: the first disk starts on the axis, and the annulus's LINE starts
//! at (40,-20,0) toward +Y, on the far side of the axis from its +Y trimming edges.
//! Exact pcurves specify that sheet for STEP readers; haecceity's freeform
//! integrator still projects the 3D boundary curves.
//!
//! The master shaft's wholly inward boundary is repaired and reported. The master
//! bore is mixed: its four bore faces already face outward. Proved inward normals
//! are now repaired individually when its unchanged wires form an outward boundary.
//! OCCT heals both on import. Pub's hole file repeats entity #539 (and others), with
//! different types. Its permissive OCCT import is not a valid reference solid.

use std::f64::consts::PI;
use std::path::Path;

use haecceity::geom::{self, Surface};
use haecceity::step::{IDENTITY, Placement, read_step_file_placed};

fn fixture(name: &str, placement: &Placement) -> haecceity::Part {
    read_step_file_placed(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name),
        placement,
    )
    .unwrap()
}

fn motion() -> Placement {
    let [x, y, z] = geom::unit([1.0, 2.0, 3.0]).unwrap();
    let (s, c) = 37f64.to_radians().sin_cos();
    let k = 1.0 - c;
    [
        [c + x * x * k, x * y * k - z * s, x * z * k + y * s, 123.456],
        [y * x * k + z * s, c + y * y * k, y * z * k - x * s, -78.9],
        [z * x * k - y * s, z * y * k + x * s, c + z * z * k, 41.3],
    ]
}

fn close(context: &str, got: f64, want: f64) {
    assert!(
        (got - want).abs() <= 1e-9 * want.abs(),
        "{context}: {got} vs {want}"
    );
}

#[test]
fn revolution_shafts_and_cone_have_analytic_mass_and_moments() {
    for (name, volume, area, centroid) in [
        (
            "truck_pub_shaft_only.step.gz",
            15000.0 * PI,
            2850.0 * PI,
            [40.0, 0.0, 0.0],
        ),
        (
            "revolution_shaft.step.gz",
            15000.0 * PI,
            2850.0 * PI,
            [40.0, 0.0, 0.0],
        ),
        (
            "revolution_line_shaft.step.gz",
            15000.0 * PI,
            2850.0 * PI,
            [40.0, 0.0, 0.0],
        ),
        (
            "revolution_cone.step.gz",
            12.0 * PI,
            24.0 * PI,
            [1.0, 0.0, 0.0],
        ),
    ] {
        for placement in [IDENTITY, motion()] {
            let part = fixture(name, &placement);
            assert!(part.unresolved_faces().is_empty());
            assert!(part.unresolved_edges().is_empty());
            assert!(part.solid_is_valid(0));
            assert!(part.faces.iter().all(|f| matches!(
                f.surface,
                Surface::Freeform {
                    kind: "REVOLUTION",
                    ..
                }
            )));
            let (v, a) = part.solid_mass(0).unwrap();
            close(&format!("{name} volume at {placement:?}"), v, volume);
            close(&format!("{name} solid area at {placement:?}"), a, area);
            let (v, c) = part.volume_centroid().unwrap();
            close(&format!("{name} volume at {placement:?}"), v, volume);
            let want = std::array::from_fn(|i| {
                geom::dot(
                    [placement[i][0], placement[i][1], placement[i][2]],
                    centroid,
                ) + placement[i][3]
            });
            assert!(
                geom::dist(c, want) < 1e-8,
                "{name} centroid: {c:?} vs {want:?}"
            );
            let mut sum = [0.0; 3];
            let mut face_area = 0.0;
            for i in 0..part.faces.len() {
                let m = part.face_moments(i).unwrap();
                face_area += m.area;
                sum = geom::add(sum, m.area_vector);
            }
            close(
                &format!("{name} summed face area at {placement:?}"),
                face_area,
                area,
            );
            assert!(
                geom::norm(sum) < 1e-9 * area,
                "{name} closed area vector: {sum:?}"
            );
        }
    }
}

#[test]
fn mass_keeps_the_prescribed_orientation() {
    for name in ["revolution_shaft.step.gz", "revolution_line_shaft.step.gz"] {
        let mut part = fixture(name, &IDENTITY);
        for face in &mut part.faces {
            face.reversed = !face.reversed;
        }
        let (volume, area) = part.solid_mass(0).unwrap();
        close(&format!("{name} reversed volume"), volume, -15000.0 * PI);
        close(&format!("{name} area"), area, 2850.0 * PI);
    }
}

#[test]
fn radial_line_disk_and_far_side_annulus_have_analytic_signed_flux() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/revolution_line_shaft.step.gz");
    let text =
        haecceity::step::decode_step_file_bytes(&path, std::fs::read(&path).unwrap()).unwrap();
    // The swept curves themselves must be LINEs, not merely the seam edges.
    let (model, _) = step_io::read(text.as_slice()).unwrap();
    assert_eq!(model.all::<step_io::model::SurfaceOfRevolution>().len(), 5);
    assert!(
        model
            .all::<step_io::model::SurfaceOfRevolution>()
            .all(|(_, s)| matches!(s.swept_curve, step_io::model::CurveRef::Line(_)))
    );
    let part = fixture("revolution_line_shaft.step.gz", &IDENTITY);
    for (faces, x, quarter_area, nx) in [
        (0..4, 0.0, 225.0 * PI / 4.0, -1.0),
        (8..12, 40.0, 125.0 * PI / 4.0, 1.0),
    ] {
        for f in faces {
            let [area, flux] = part.face_mass(f).unwrap();
            close(
                &format!("revolution_line_shaft.step.gz face {f} area"),
                area,
                quarter_area,
            );
            assert!((flux - x * nx * quarter_area).abs() < 1e-9 * quarter_area);
            let m = part.face_moments(f).unwrap();
            assert!((m.centroid[0] - x).abs() < 1e-9);
            assert!(geom::dist(m.area_vector, [nx * quarter_area, 0.0, 0.0]) < 1e-9 * quarter_area);
        }
    }
}

#[test]
fn truck_master_inward_shell_is_repaired_and_reported() {
    for placement in [IDENTITY, motion()] {
        let part = fixture("truck_master_shaft_only.step.gz", &placement);
        assert_eq!(
            part.input_repairs(),
            ["solid 0: inverted closed shell repaired"]
        );
        let (volume, area) = part.solid_mass(0).unwrap();
        // Exact geometry; OCCT's approximate pcurve projection gives 47118.969265311505.
        close(
            "truck_master_shaft_only.step.gz repaired volume",
            volume,
            15000.0 * PI,
        );
        close(
            "truck_master_shaft_only.step.gz repaired area",
            area,
            2850.0 * PI,
        );
        assert!(part.faces.iter().all(|f| !f.reversed));
    }
}

#[test]
fn truck_master_bore_repairs_only_proved_inward_normals() {
    use haecceity::classify::{Classifier, State};
    let part = fixture("truck_master_shaft_hole.step.gz", &IDENTITY);
    // Every material side is now proved before correcting the twenty inward normals;
    // the four outward bore normals and all boundary wires are preserved.
    assert_eq!(
        part.input_repairs(),
        ["solid 0: mixed face senses repaired (20 faces)"]
    );
    close(
        "truck_master_shaft_hole.step.gz repaired mixed volume",
        part.solid_mass(0).unwrap().0,
        46140.658347174205,
    );
    assert!(part.solid_is_valid(0));
    for (i, f) in part.faces.iter().enumerate() {
        assert_eq!(f.reversed, [15, 16, 21, 22].contains(&i), "face {i}");
    }
    let classifier = Classifier::for_solid(&part, 0);
    for (face, want) in [
        (0, (State::Out, State::In)),
        (15, (State::Out, State::In)),
        (16, (State::Out, State::In)),
        (21, (State::Out, State::In)),
        (22, (State::Out, State::In)),
    ] {
        let (p, (u, v)) = part.position_at(face, 0.37, 0.41).unwrap();
        let n = part.face_normal(face, u, v).unwrap();
        let sides = (
            classifier.classify(geom::add(p, geom::scale(n, 1e-3))),
            classifier.classify(geom::sub(p, geom::scale(n, 1e-3))),
        );
        assert_eq!(sides, want, "face {face}");
    }
}

#[test]
fn independent_inverted_shell_is_repaired_but_negative_mixed_shell_is_not() {
    use haecceity::classify::{Classifier, State};
    for name in ["revolution_shaft.step.gz", "revolution_line_shaft.step.gz"] {
        let original = fixture(name, &IDENTITY);
        assert!(original.input_repairs().is_empty());
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name);
        let text =
            haecceity::step::decode_step_file_bytes(&path, std::fs::read(&path).unwrap()).unwrap();
        let text = String::from_utf8(text).unwrap();
        for mixed in [false, true] {
            // Change only the independent file's face senses, exercising the import path
            // without changing the fixture bytes or relying on truck's exporter.
            let mut face_count = 0;
            let inverted = text
                .lines()
                .map(|line| {
                    if line.contains(" = ADVANCED_FACE(") {
                        let i = face_count;
                        face_count += 1;
                        assert!(line.ends_with(",.T.);"));
                        if !mixed || i != 0 {
                            return line.replace(",.T.);", ",.F.);");
                        }
                    }
                    line.to_owned()
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert_eq!(face_count, original.faces.len());
            let imported = haecceity::read_step(inverted.as_bytes()).unwrap();
            let mut faces = original.faces.clone();
            for (i, face) in faces.iter_mut().enumerate() {
                if !mixed || i != 0 {
                    face.reversed = !face.reversed;
                }
            }
            let raw = haecceity::Part::new(faces, original.edges.clone(), original.solids.clone());
            let before = raw.solid_mass(0).unwrap();
            assert!(before.0 < 0.0);
            let repaired = raw.repair_inverted_shells();
            assert_eq!(imported.input_repairs(), repaired.input_repairs());
            assert_eq!(imported.solid_mass(0), repaired.solid_mass(0));
            if mixed {
                assert!(repaired.input_repairs().is_empty());
                assert_eq!(repaired.solid_mass(0).unwrap(), before);
            } else {
                assert_eq!(
                    repaired.input_repairs(),
                    ["solid 0: inverted closed shell repaired"]
                );
                close(
                    &format!("{name} repaired volume"),
                    repaired.solid_mass(0).unwrap().0,
                    15000.0 * PI,
                );
                close(
                    &format!("{name} repaired centroid volume"),
                    repaired.volume_centroid().unwrap().0,
                    15000.0 * PI,
                );
                for f in 0..original.faces.len() {
                    assert_eq!(repaired.faces[f].reversed, original.faces[f].reversed);
                    assert!(
                        geom::dist(
                            repaired.face_moments(f).unwrap().area_vector,
                            original.face_moments(f).unwrap().area_vector
                        ) < 1e-8
                    );
                }
                let classifier = Classifier::for_solid(&repaired, 0);
                assert_eq!(classifier.classify([20.0, 0.0, 0.0]), State::In);
                assert_eq!(classifier.classify([20.0, 20.0, 0.0]), State::Out);
                assert_eq!(repaired.repair_inverted_shells().input_repairs().len(), 1);
            }
        }
    }
}

#[test]
fn malformed_truck_pub_hole_is_refused() {
    let error = read_step_file_placed(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/broken/truck_pub_shaft_hole.step.gz"),
        &IDENTITY,
    )
    .expect_err("duplicate STEP entity IDs must not be silently overwritten");
    assert!(
        error.to_string().contains("duplicate entity #539"),
        "{error}"
    );
}

// Model last-ulp differences in step-io's trigonometric NURBS conversion, rebuilding
// its caches too. Before the geometric retry, rotated shaft trial 0 gave 49974.59424876291.
#[test]
fn revolution_control_net_ulp_perturbations() {
    use haecceity::nurbs::NurbsSurface;
    for (name, volume, area, cx) in [
        (
            "truck_pub_shaft_only.step.gz",
            15000.0 * PI,
            2850.0 * PI,
            40.0,
        ),
        ("revolution_shaft.step.gz", 15000.0 * PI, 2850.0 * PI, 40.0),
        (
            "revolution_line_shaft.step.gz",
            15000.0 * PI,
            2850.0 * PI,
            40.0,
        ),
        ("revolution_cone.step.gz", 12.0 * PI, 24.0 * PI, 1.0),
    ] {
        for placement in [IDENTITY, motion()] {
            let original = fixture(name, &placement);
            for trial in 0..32u64 {
                let mut bits = trial + 1;
                let mut faces = original.faces.clone();
                for face in &mut faces {
                    if let Surface::Freeform { surface, .. } = &mut face.surface {
                        let mut poles = surface.control_points.clone();
                        for p in poles.iter_mut().flatten() {
                            for x in p {
                                bits ^= bits << 13;
                                bits ^= bits >> 7;
                                bits ^= bits << 17;
                                let shift = (bits % 17) as i32 - 8;
                                for _ in 0..shift.abs() {
                                    *x = if shift > 0 {
                                        x.next_up()
                                    } else {
                                        x.next_down()
                                    };
                                }
                            }
                        }
                        **surface = NurbsSurface::new(
                            surface.degree_u,
                            surface.degree_v,
                            poles,
                            surface.weights.clone(),
                            surface.knots_u.clone(),
                            surface.knots_v.clone(),
                        )
                        .unwrap();
                    }
                }
                let part =
                    haecceity::Part::new(faces, original.edges.clone(), original.solids.clone());
                let context = format!("{name} control-net trial {trial} pose {placement:?}");
                let (v, a) = part
                    .solid_mass(0)
                    .unwrap_or_else(|| panic!("{context} solid mass unresolved"));
                close(&format!("{context} volume"), v, volume);
                close(&format!("{context} area"), a, area);
                let (v, c) = part
                    .volume_centroid()
                    .unwrap_or_else(|| panic!("{context} centroid unresolved"));
                close(&format!("{context} centroid volume"), v, volume);
                let want = [0, 1, 2].map(|i| placement[i][0] * cx + placement[i][3]);
                assert!(
                    geom::dist(c, want) < 1e-9,
                    "{context} centroid {c:?} vs {want:?}"
                );
            }
        }
    }
}

#[test]
fn radial_line_sheet_choice_tolerates_axis_coordinate_roundoff() {
    let name = "revolution_line_shaft.step.gz";
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name);
    let bytes =
        haecceity::step::decode_step_file_bytes(&path, std::fs::read(&path).unwrap()).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    for shift in [-8i32, -4, -2, -1, 0, 1, 2, 4, 8] {
        let mut changed = 0;
        let text = text
            .lines()
            .map(|line| {
                if line.contains("CARTESIAN_POINT") {
                    let (prefix, coords) = line.rsplit_once(",(").unwrap();
                    let coords = coords.strip_suffix("));").unwrap();
                    let mut p: Vec<f64> = coords
                        .split(',')
                        .map(|x| x.trim().parse().unwrap())
                        .collect();
                    if p.len() == 3 {
                        p[1] += 1.0; // translate the whole geometry, leaving 2D pcurves alone
                        if line.starts_with("#72 =") {
                            // This LINE's origin lies on the axis. Nudge its translated y=1
                            // either way by a few ulps, rather than testing subnormal zero.
                            for _ in 0..shift.abs() {
                                p[1] = if shift > 0 {
                                    p[1].next_up()
                                } else {
                                    p[1].next_down()
                                };
                            }
                            changed += 1;
                        }
                        return format!("{prefix},({:.17E},{:.17E},{:.17E}));", p[0], p[1], p[2]);
                    }
                }
                line.to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(changed, 1);
        let part = haecceity::read_step(text.as_bytes()).unwrap();
        let context = format!("{name} axis origin {shift} ulps");
        let (v, a) = part
            .solid_mass(0)
            .unwrap_or_else(|| panic!("{context} solid mass unresolved"));
        close(&format!("{context} volume"), v, 15000.0 * PI);
        close(&format!("{context} area"), a, 2850.0 * PI);
        let (_, c) = part.volume_centroid().unwrap();
        assert!(
            geom::dist(c, [40., 1., 0.]) < 1e-9,
            "{context} centroid {c:?}"
        );
    }
}

#[test]
fn fully_reversed_shell_restores_wires_validity_and_convexity() {
    let name = "captured/frames/box_moved.step";
    let original = fixture(name, &IDENTITY);
    assert_eq!(original.faces.len(), 6);
    assert!(
        original
            .faces
            .iter()
            .all(|f| matches!(f.surface, Surface::Plane { .. }))
    );
    assert!(original.solid_is_geometrically_valid(0));
    // This box is rigidly moved, including a rotation. Its three mutually orthogonal
    // incident edge vectors give the analytic volume independently of surface flux.
    let vertex = original.edges[0].vertices.0;
    let sides: Vec<_> = original
        .edges
        .iter()
        .filter_map(|e| {
            if e.vertices.0 == vertex {
                Some(geom::sub(e.end, e.start))
            } else if e.vertices.1 == vertex {
                Some(geom::sub(e.start, e.end))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(sides.len(), 3);
    for i in 0..3 {
        for j in i + 1..3 {
            assert!(
                geom::dot(sides[i], sides[j]).abs()
                    < 1e-9 * geom::norm(sides[i]) * geom::norm(sides[j])
            );
        }
    }
    let volume = geom::dot(sides[0], geom::cross(sides[1], sides[2])).abs();
    close(
        &format!("{name} analytic box volume"),
        original.solid_mass(0).unwrap().0,
        volume,
    );
    let mut faces = original.faces.clone();
    for face in &mut faces {
        face.reversed = !face.reversed;
        for lp in &mut face.loops {
            lp.edges.reverse();
            for (_, forward) in &mut lp.edges {
                *forward = !*forward;
            }
        }
    }
    let raw = haecceity::Part::new(faces, original.edges.clone(), original.solids.clone());
    assert!(raw.solid_is_valid(0));
    close(
        &format!("{name} fully reversed volume"),
        raw.solid_mass(0).unwrap().0,
        -volume,
    );
    let repaired = raw.repair_inverted_shells();
    assert_eq!(
        repaired.input_repairs(),
        ["solid 0: inverted closed shell repaired"]
    );
    close(
        &format!("{name} repaired volume"),
        repaired.solid_mass(0).unwrap().0,
        volume,
    );
    assert!(
        repaired.solid_is_geometrically_valid(0),
        "{name} repaired bad faces: {:?}",
        repaired.bad_faces(0)
    );
    for f in 0..original.faces.len() {
        for n in original.neighbours(f) {
            assert_eq!(
                repaired.arc(f, n),
                original.arc(f, n),
                "{name} convexity at {f}/{n}"
            );
        }
    }
}

/// The mixed-normal fallback never guesses a missing material side or reverses
/// mixed/inward wires piecemeal. These inputs retain their original face senses.
#[test]
fn mixed_normal_repair_refuses_unproved_faces_and_inward_wires() {
    let repaired = fixture("truck_master_shaft_hole.step.gz", &IDENTITY);
    for variant in 0..3 {
        let mut faces = repaired.faces.clone();
        for (i, face) in faces.iter_mut().enumerate() {
            if ![15, 16, 21, 22].contains(&i) {
                face.reversed = !face.reversed;
            }
            if variant == 1 {
                for lp in &mut face.loops {
                    lp.edges.reverse();
                    for (_, forward) in &mut lp.edges {
                        *forward = !*forward;
                    }
                }
            }
        }
        if variant == 0 {
            faces[15].surface = Surface::Other { kind: "unproved" };
        } else if variant == 2 {
            faces[15].loops[0].edges.pop();
        }
        let senses: Vec<_> = faces.iter().map(|f| f.reversed).collect();
        let part = haecceity::Part::new(faces, repaired.edges.clone(), repaired.solids.clone())
            .repair_inverted_shells();
        assert!(part.input_repairs().is_empty(), "variant {variant}");
        assert_eq!(
            part.faces.iter().map(|f| f.reversed).collect::<Vec<_>>(),
            senses
        );
    }
}
