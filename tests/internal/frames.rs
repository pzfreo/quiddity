//! Rust frame snapshots (axes 1e-9, origin 1e-9 of part size) and unchanged motion-equivariance references.

use super::common;

use std::path::PathBuf;

use quiddity::frames::{
    FrameGauge, FrameRefusal, FramedError, PartFrame, infer_part_frame, prepare_framed_step,
};
use quiddity::kernel::step::{IDENTITY, Placement, read_step_file_placed, read_step_placed};
use serde_json::Value;

/// The rotation by *degrees* about *axis* (Rodrigues), then *translation*.
fn motion(axis: [f64; 3], degrees: f64, translation: [f64; 3]) -> Placement {
    let n = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    let [x, y, z] = axis.map(|c| c / n);
    let (s, c) = degrees.to_radians().sin_cos();
    let t = 1.0 - c;
    [
        [
            t * x * x + c,
            t * x * y - s * z,
            t * x * z + s * y,
            translation[0],
        ],
        [
            t * x * y + s * z,
            t * y * y + c,
            t * y * z - s * x,
            translation[1],
        ],
        [
            t * x * z - s * y,
            t * y * z + s * x,
            t * z * z + c,
            translation[2],
        ],
    ]
}

fn captured() -> Value {
    common::load("captured/frames.json")
}

fn captured_motion(captured: &Value) -> Placement {
    let m = &captured["motion"];
    let v = |k: &str| -> [f64; 3] {
        let a = m[k].as_array().unwrap();
        [0, 1, 2].map(|i| a[i].as_f64().unwrap())
    };
    motion(
        v("axis"),
        m["angle_degrees"].as_f64().unwrap(),
        v("translation"),
    )
}

fn vector(v: &Value) -> [f64; 3] {
    [0, 1, 2].map(|i| v[i].as_f64().unwrap())
}

fn largest_difference(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).abs()).fold(0.0, f64::max)
}

fn apply(m: &Placement, p: [f64; 3], point: bool) -> [f64; 3] {
    let t = |i: usize| if point { m[i][3] } else { 0.0 };
    [0, 1, 2].map(|i| m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2] + t(i))
}

/// How the frame of the part under *motion* differs from the unmoved frame carried by it: the
/// same refusal or gauge, the origin to 1e-9 of *size*, and for a full gauge the axes to 1e-9
/// (in the other gauges the axes are representatives, which may turn differently).
fn equivariance(
    motion: &Placement,
    unmoved: &Result<PartFrame, FrameRefusal>,
    moved: &Result<PartFrame, FrameRefusal>,
    size: f64,
) -> (Vec<&'static str>, f64, String) {
    match (unmoved, moved) {
        (Err(a), Err(b)) if a == b => (vec![], 0.0, String::new()),
        (Ok(a), Ok(b)) => {
            let (mut fields, mut text) = (Vec::new(), Vec::new());
            if a.gauge != b.gauge {
                fields.push("gauge");
                text.push(format!("gauge {:?} unmoved, {:?} moved", a.gauge, b.gauge));
            }
            if a.gauge == FrameGauge::Full {
                for (name, p, q) in [("x", a.x, b.x), ("y", a.y, b.y), ("z", a.z, b.z)] {
                    let off = largest_difference(apply(motion, p, false), q);
                    if off > 1e-9 {
                        fields.push(name);
                        text.push(format!("{name} off {off:.1e}"));
                    }
                }
            }
            let off = largest_difference(apply(motion, a.origin, true), b.origin);
            if off > 1e-9 * size.max(1.0) {
                fields.push("origin");
                text.push(format!("origin off {off:.1e}"));
            }
            (fields, off, text.join("; "))
        }
        _ => (
            vec!["refusal"],
            0.0,
            format!(
                "unmoved {:?}, moved {:?}",
                unmoved.as_ref().map(|f| f.gauge),
                moved.as_ref().map(|f| f.gauge)
            ),
        ),
    }
}

/// Also checks that the port's frame moves with the part: listed as case `equivariance`.
#[test]
fn rust_snapshot_frames_and_equivariance() {
    let captured = captured();
    let generic = captured_motion(&captured);
    let corpus = common::corpus_dir();
    if corpus.is_none() {
        assert!(
            std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
            "QUIDDITY_CORPUS_REQUIRED is set but the corpus was not found"
        );
    }
    let known = common::load("references/frame_equivariance.json");
    let known = known.as_array().unwrap();
    common::check_verdicts("references/frame_equivariance.json", known);
    let parts: Vec<&Value> = captured["parts"].as_array().unwrap().iter().collect();
    let path = |entry: &Value| -> Option<PathBuf> {
        let file = entry["file"].as_str().unwrap();
        match entry["source"].as_str().unwrap() {
            "built" => Some(common::fixtures().join("captured/frames").join(file)),
            "fixture" => Some(common::fixtures().join(file)),
            _ => corpus.as_ref().map(|d| d.join(file)),
        }
    };
    let found = common::parallel::map(&parts, |entry| {
        let file = entry["file"].as_str().unwrap();
        let Some(path) = path(entry) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut frames = Vec::new();
        let mut snapshot = serde_json::Map::new();
        for (case, placement) in [("frame", IDENTITY), ("moved", generic)] {
            let part = read_step_file_placed(&path, &placement).unwrap();
            let size = part.bounds().diagonal();
            let frame = infer_part_frame(&part);
            let value = frame.as_ref().map_or_else(
                |r| serde_json::json!({"refused":r.as_str()}),
                |f| serde_json::to_value(f).unwrap(),
            );
            snapshot.insert(case.into(), value);
            frames.push((frame, size));
        }
        let equal = |a: &Value, b: &Value| {
            ["frame", "moved"].iter().enumerate().all(|(i, case)| {
                let (a, b) = (&a[case], &b[case]);
                if a.get("refused").is_some() || b.get("refused").is_some() {
                    return a == b;
                }
                a["gauge"] == b["gauge"]
                    && ["x", "y", "z"]
                        .iter()
                        .all(|k| largest_difference(vector(&a[k]), vector(&b[k])) <= 1e-9)
                    && largest_difference(vector(&a["origin"]), vector(&b["origin"]))
                        <= 1e-9 * frames[i].1.max(1.0)
            })
        };
        common::snapshot::check_with(
            &common::fixtures(),
            &format!("frames/{}/{}", entry["source"].as_str().unwrap(), file),
            &Value::Object(snapshot),
            &equal,
        );
        let (fields, off, text) = equivariance(&generic, &frames[0].0, &frames[1].0, frames[0].1);
        if !fields.is_empty() {
            out.push((file.to_string(), "equivariance", fields, off, text));
        }
        out
    });
    let mut problems = Vec::new();
    let mut seen = Vec::new();
    for (file, case, fields, off, text) in found.into_iter().flatten() {
        let entry = || {
            let mut e = serde_json::json!({"file": file, "case": case, "fields": fields});
            if fields.contains(&"origin") {
                e["origin_within"] =
                    serde_json::json!(format!("{:.0e}", off * 1.5).parse::<f64>().unwrap());
            }
            e
        };
        match known
            .iter()
            .position(|k| k["file"] == file.as_str() && k["case"] == case)
        {
            Some(i) => {
                seen.push(i);
                let k = &known[i];
                let listed: Vec<&str> = k["fields"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|f| f.as_str().unwrap())
                    .collect();
                let within = k["origin_within"].as_f64().unwrap_or(0.0);
                if listed != fields || (fields.contains(&"origin") && off > within) {
                    problems.push(format!("{file} {case} changed: {text}\n  now: {}", entry()));
                }
            }
            None => problems.push(format!("{file} {case}: {text}\n  needs: {}", entry())),
        }
    }
    for (i, k) in known.iter().enumerate() {
        let corpus_entry = parts
            .iter()
            .any(|p| p["file"] == k["file"] && p["source"] == "corpus");
        if !seen.contains(&i) && !(corpus_entry && corpus.is_none()) {
            problems.push(format!("{k} is listed but now agrees: remove it"));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Python's `test_frame_point_transforms_are_inverse`.
#[test]
fn frame_point_transforms_are_inverse() {
    let frame = PartFrame {
        origin: [10.0, 20.0, 30.0],
        x: [0.0, 1.0, 0.0],
        y: [0.0, 0.0, 1.0],
        z: [1.0, 0.0, 0.0],
        gauge: FrameGauge::Full,
    };
    assert_eq!(frame.to_local([14.0, 25.0, 36.0]), [5.0, 6.0, 4.0]);
    assert_eq!(frame.to_world([5.0, 6.0, 4.0]), [14.0, 25.0, 36.0]);
}

/// The working part is the caller's part in the frame's local coordinates, face for face and
/// edge for edge (Python's `test_framed_working_shape_uses_the_published_local_coordinates`),
/// for one part of each gauge, as read and under the generic motion.
#[test]
fn working_part_is_the_caller_part_in_local_coordinates() {
    let generic = captured_motion(&captured());
    let dir = common::fixtures().join("captured/frames");
    for (file, gauge) in [
        ("asymmetric_bored_moved.step", FrameGauge::Full),
        ("box_moved.step", FrameGauge::Orthogonal),
        ("cylinder_rot_x37.step", FrameGauge::Axial),
    ] {
        let bytes = std::fs::read(dir.join(file)).unwrap();
        for placement in [IDENTITY, generic] {
            let caller = read_step_placed(&bytes, &placement).unwrap();
            let framed = prepare_framed_step(&bytes, &placement).unwrap();
            assert_eq!(framed.frame.gauge, gauge, "{file}");
            assert_eq!(framed.frame, infer_part_frame(&caller).unwrap(), "{file}");
            assert_eq!(framed.part.edges.len(), caller.edges.len(), "{file}");
            for (local, edge) in framed.part.edges.iter().zip(&caller.edges) {
                for (p, q) in [(local.start, edge.start), (local.end, edge.end)] {
                    let want = framed.frame.to_local(q);
                    assert!(
                        (0..3).all(|i| (p[i] - want[i]).abs() < 1e-8),
                        "{file}: {p:?} against {want:?}"
                    );
                }
            }
        }
    }
}

/// Framed recognition finds the bore of the full-gauge part, in local coordinates, wherever
/// the caller placed it; the sphere is refused for want of a direction, as Python refuses it.
#[test]
fn framed_recognition_follows_the_part() {
    let generic = captured_motion(&captured());
    let dir = common::fixtures().join("captured/frames");
    let bytes = std::fs::read(dir.join("asymmetric_bored_moved.step")).unwrap();
    let records: Vec<String> = [IDENTITY, generic]
        .iter()
        .map(|placement| {
            let framed = prepare_framed_step(&bytes, placement).unwrap().recognise();
            assert_eq!(framed.features.holes.len(), 1);
            let hole = serde_json::to_value(&framed.features.holes).unwrap();
            format!("{:?}", framed.features.defining["holes"]) + &hole["axis"].to_string()
        })
        .collect();
    assert_eq!(records[0], records[1]);
    let sphere = std::fs::read(dir.join("sphere.step")).unwrap();
    match prepare_framed_step(&sphere, &IDENTITY) {
        Err(FramedError::Frame(r)) => assert_eq!(r, FrameRefusal::NoAnalyticDirection),
        other => panic!("sphere: {:?}", other.map(|f| f.frame)),
    }
}

/// The default recognition reports the working part's faces under the caller's indices
/// (`correspondence::recognise`): the in-memory transform must preserve face numbering.
/// On every corpus part with a frame, each working face has the caller face's surface
/// kind and edges, and each of those edges' ends is the caller's carried into the frame (to 1e-6
/// of the part's size).
#[test]
fn working_faces_are_the_caller_faces_on_every_corpus_part() {
    let Some(dir) = common::corpus_dir() else {
        assert!(
            std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
            "QUIDDITY_CORPUS_REQUIRED is set but the corpus was not found"
        );
        return;
    };
    let files: Vec<String> = common::load("corpus.json")["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["file"].as_str().unwrap().to_string())
        .collect();
    let (mut checked, mut problems) = (0, Vec::new());
    let found = common::parallel::map(&files, |name| {
        let path = dir.join(name);
        let Ok(caller) = read_step_file_placed(&path, &IDENTITY) else {
            return None;
        };
        let Ok(framed) = quiddity::frames::prepare_framed_file(&path, &IDENTITY) else {
            return None;
        };
        let size = caller
            .edges
            .iter()
            .flat_map(|e| e.start.iter().chain(&e.end))
            .fold(1.0f64, |m, c| m.max(c.abs()));
        let mut out = Vec::new();
        for (i, (local, face)) in framed.part.faces.iter().zip(&caller.faces).enumerate() {
            let edges = caller.face_edges(i);
            if local.surface.kind() != face.surface.kind() || framed.part.face_edges(i) != edges {
                out.push(format!("{name}: face {i} is another face"));
                continue;
            }
            for e in edges {
                let (l, c) = (&framed.part.edges[e], &caller.edges[e]);
                for (p, q) in [(l.start, c.start), (l.end, c.end)] {
                    if largest_difference(p, framed.frame.to_local(q)) > 1e-6 * size {
                        out.push(format!("{name}: face {i} edge {e} at {p:?}, not {q:?}"));
                    }
                }
            }
        }
        Some(out)
    });
    for out in found.into_iter().flatten() {
        checked += 1;
        problems.extend(out);
    }
    assert!(checked > 90, "only {checked} corpus parts had a frame");
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// #81: the AP242 minority angled faces must not choose roll, and a real placement must
/// carry the frame and the same fillet evidence. A 1° motion is deliberately inside the old
/// direction-class tolerance, so these copies must be transformed in memory, not re-exported.
#[test]
fn axis_majorities_and_intrinsic_frames_survive_sixty_and_one_degree_motions() {
    use quiddity::{correspondence, frames::RecordFrame};
    let corpus = common::corpus_dir();
    // For #120, tangent convex rounds at drafted wall angles and oblique runs are
    // fillets too. For #127, proved concave rounds and their caps join those retained
    // convex records; the complete face audit preserves every existing record.
    let mut cases = vec![
        (
            common::fixtures().join("nist_ctc_02_asme1_ap242-e2.stp.gz"),
            106,
            0,
            0,
        ),
        (
            common::fixtures().join("nist_ftc_07_asme1_ap242-e2.stp.gz"),
            // For #297, 16 internal R0.432 rounds (below the former 0.6 mm floor) join 63.
            79,
            4,
            5,
        ),
    ];
    if let Some(dir) = corpus {
        cases.extend([
            (dir.join("nist/nist_ctc_02_asme1_rc.stp"), 113, 0, 0),
            (dir.join("nist/nist_ftc_07_asme1_rd.stp"), 79, 4, 5),
        ]);
    }
    for (path, fillets, bosses, plates) in cases {
        let part = quiddity::read_step_file(&path).unwrap();
        let frame = infer_part_frame(&part).unwrap();
        assert!(
            [frame.x, frame.y, frame.z]
                .iter()
                .all(|a| a.iter().any(|c| c.abs() == 1.)),
            "{path:?}: {frame:?}"
        );
        let mut original_faces = None;
        for placement in [
            IDENTITY,
            motion([0., 0., 1.], 60., [12.3, -45.6, 78.9]),
            motion([0., 0., 1.], 1., [12.3, -45.6, 78.9]),
            motion([1., 0., 0.], 60., [12.3, -45.6, 78.9]),
            motion([1., 0., 0.], 1., [12.3, -45.6, 78.9]),
        ] {
            let moved = part.transformed(&placement).unwrap();
            let recognised = correspondence::recognise(&moved);
            let RecordFrame::Inferred(actual) = recognised.frame else {
                panic!("{path:?}: frame refused")
            };
            let (fields, _, why) = equivariance(
                &placement,
                &Ok(frame),
                &Ok(actual),
                part.bounds().diagonal(),
            );
            assert!(fields.is_empty(), "{path:?}: {why}");
            assert_eq!(
                recognised.features.fillets.len(),
                fillets,
                "{path:?}: {placement:?}"
            );
            assert_eq!(
                recognised.features.polygonal_bosses.len(),
                bosses,
                "{path:?}"
            );
            assert_eq!(recognised.features.plates.len(), plates, "{path:?}");
            let mut faces = recognised.features.defining["fillets"].clone();
            faces.sort();
            if let Some(original) = &original_faces {
                assert_eq!(&faces, original, "{path:?}");
            } else {
                original_faces = Some(faces);
            }
        }
        if path.file_name().unwrap() == "nist_ctc_02_asme1_ap242-e2.stp.gz" {
            let faces = original_faces.unwrap();
            for face in [
                391, 394, 396, 399, 475, 519, 522, 532, 542, 545, 548, 625, 626, 630, 631, 77, 78,
                79,
            ] {
                assert!(faces.contains(&vec![face]), "missing AP242 fillet {face}");
            }
            // rc's extra small fillet is its face 635, R1.092560827904 on a 2° cylinder.
            // That small feature is absent from this AP242 geometry, whose minimum cylinder
            // radius is 4.19 mm. Do not invent that record to match another edition.
            assert!(
                part.faces
                    .iter()
                    .filter_map(|f| match f.surface {
                        quiddity::kernel::geom::Surface::Cylinder { radius, .. } => Some(radius),
                        _ => None,
                    })
                    .all(|r| r > 4.18)
            );
        }
    }
}

#[test]
fn majority_frame_recovers_specify_fillets_and_parallel_support_chamfer() {
    use quiddity::{correspondence, frames::RecordFrame};
    let Some(dir) = common::corpus_dir() else {
        return;
    };
    for (file, family, count, faces) in [
        (
            "cadgenbench_inputs/cgb202.step.gz",
            "fillets",
            // For #127, 401 proved inside rounds join #120's 307 convex fillets.
            708,
            vec![
                1248, 1250, 1453, 1464, 1473, 1475, 1477, 1623, 1631, 2056, 2057,
            ],
        ),
        // For #127, 34 proved inside rounds join #120's forty convex fillets.
        // For #297, 29 R0.5 rounds (below the former 0.6 mm floor) join those 74.
        ("cadgenbench_inputs/cgb207.step", "fillets", 103, vec![176]),
        ("mfcadpp/10252.step", "oriented_chamfers", 1, vec![2]),
    ] {
        let part = quiddity::read_step_file(&dir.join(file)).unwrap();
        let recognised = correspondence::recognise(&part);
        let RecordFrame::Inferred(frame) = recognised.frame else {
            panic!("{file}: frame refused")
        };
        assert!(
            [frame.x, frame.y, frame.z]
                .iter()
                .all(|a| a.iter().any(|c| c.abs() == 1.)),
            "{file}"
        );
        let found = &recognised.features.defining[family];
        // #159 groups interrupted torus arcs; retain the original patch-coverage
        // requirement rather than reducing the historical feature floor.
        let patch_count = if family == "fillets" {
            super::inventory_floor::fillet_patch_count(&part, found)
        } else {
            found.len()
        };
        assert_eq!(patch_count, count, "{file}/{family}");
        for face in faces {
            assert!(
                found.contains(&vec![face]),
                "{file}: missing {family}:{face}"
            );
        }
        if file == "cadgenbench_inputs/cgb202.step.gz" {
            // For #127, all seventeen inward R3 cylinders #89 excluded now pass the
            // full inside-round proof. Pin their concave records (some carry spherical
            // caps); the eleven genuinely convex recovered faces remain above.
            for face in [
                29, 31, 46, 571, 657, 662, 806, 876, 890, 984, 998, 1069, 1087, 1092, 1104, 1225,
                1346,
            ] {
                assert_eq!(part.frame_points_outward(face), Some(false), "face {face}");
                let index = found.iter().position(|fs| fs.contains(&face)).unwrap();
                let fillet = &recognised.features.fillets[index];
                assert_eq!((fillet.radius, fillet.side), (3., "concave"), "face {face}");
            }
        }
        if family == "oriented_chamfers" {
            let chamfer = &recognised.features.oriented_chamfers[0];
            assert_eq!(
                (chamfer.length, chamfer.leg1, chamfer.leg2, chamfer.angle),
                (18.694, 0.54, 0.54, 45.)
            );
            assert_eq!(*chamfer.corner, [31.254, 12.205, 0.]);
        }
    }
}

#[test]
fn corpus_direction_sets_keep_their_winning_frames() {
    let Some(dir) = common::corpus_dir() else {
        return;
    };
    for (file, expected_y) in [
        (
            "cadgenbench_inputs/cgb203.step",
            [0.18196311701898504, 0.9833053564609191, 0.],
        ),
        // #91: cgb217's vote (98, the 45-degree rounds) settles Z; its roll goes to the
        // 13.07-degree set of its cross bores and largest walls (93 votes), whose box is 0.80
        // times the 45-degree box (the file axes' 96-vote box is 0.83 times it).
        (
            "cadgenbench_inputs/cgb217.step.gz",
            [-0.9741017810080629, -0.22610997376701458, 0.],
        ),
        // Distinct-surface maxima over every candidate basis: cgb203 5 > file 4,
        // connector 19 > file 17; no less-voted roll about their leading lines has a smaller
        // box. cgb241 (20 > file 18) keeps its file-axis outline: tests/internal/frames.rs
        // `tightest_roll_keeps_plates_and_sheets_on_their_outline`.
        ("cadgenbench_inputs/cgb241.step", [-1., 0., 0.]),
        (
            "cadgenbench/threaded_connector_109.step",
            [-0.45979133716192094, -0.8880269851028474, 0.],
        ),
    ] {
        let part = quiddity::read_step_file(&dir.join(file)).unwrap();
        let frame = infer_part_frame(&part).unwrap();
        assert!(
            largest_difference(frame.y, expected_y) < 1e-10,
            "{file}: {frame:?}"
        );
        let placement = motion([0., 0., 1.], 1., [12.3, -45.6, 78.9]);
        let moved = part.transformed(&placement).unwrap();
        let (fields, _, why) = equivariance(
            &placement,
            &Ok(frame),
            &infer_part_frame(&moved),
            part.bounds().diagonal(),
        );
        assert!(fields.is_empty(), "{file}: {why}");
    }
}

/// Equal complete direction sets must choose the file axes. Each box has six distinct planes; their four Z planes are shared
/// direction support, so both competing bases have exactly eight distinct surfaces.
#[test]
fn exact_surface_support_tie_prefers_file_axes() {
    use quiddity::kernel::{brep::Part, geom::Surface};
    let source =
        quiddity::read_step_file(&common::fixtures().join("captured/frames/box_rot_x30.step"))
            .unwrap();
    assert_eq!(source.faces.len(), 6);
    assert!(
        source
            .faces
            .iter()
            .all(|f| matches!(f.surface, Surface::Plane { .. }))
    );
    let first = source
        .transformed(&motion([1., 0., 0.], -30., [0.; 3]))
        .unwrap();
    let second = first
        .transformed(&motion([0., 0., 1.], 30., [100., 100., 100.]))
        .unwrap();
    let mut faces = first.faces.clone();
    let mut edges = first.edges.clone();
    let mut solids = first.solids.clone();
    let vertex_offset = edges
        .iter()
        .map(|e| e.vertices.0.max(e.vertices.1))
        .max()
        .unwrap()
        + 1;
    for mut face in second.faces.clone() {
        face.solid = face.solid.map(|i| i + solids.len());
        for wire in &mut face.loops {
            for (edge, _) in &mut wire.edges {
                *edge += edges.len();
            }
        }
        for (edge, _) in &mut face.pcurves {
            *edge += edges.len();
        }
        faces.push(face);
    }
    for mut solid in second.solids.clone() {
        for face in &mut solid.faces {
            *face += first.faces.len();
        }
        solids.push(solid);
    }
    for mut edge in second.edges.clone() {
        edge.vertices.0 += vertex_offset;
        edge.vertices.1 += vertex_offset;
        edges.push(edge);
    }
    let part = Part::new(faces, edges, solids);
    let frame = infer_part_frame(&part).unwrap();
    assert!(
        [frame.x, frame.y, frame.z]
            .iter()
            .all(|a| a.iter().any(|c| c.abs() == 1.)),
        "{frame:?}"
    );
}

/// The oblique channel's coplanar patches used to outvote the file axes 17:16 by face.
/// Its complete basis has only 10 distinct supports against 15 on the file axes. Pin
/// the real slot, its recess, the void and the angled step that the split-face vote lost.
#[test]
fn split_channel_surfaces_do_not_outvote_file_axes() {
    let Some(dir) = common::corpus_dir() else {
        return;
    };
    let part = quiddity::read_step_file(&dir.join("mfcadpp_regressions/11512.step")).unwrap();
    let frame = infer_part_frame(&part).unwrap();
    assert!(
        [frame.x, frame.y, frame.z]
            .iter()
            .all(|a| a.iter().any(|c| c.abs() == 1.)),
        "{frame:?}"
    );
    let recognised = quiddity::correspondence::recognise(&part);
    assert_eq!(recognised.features.plates.len(), 1);
    assert_eq!(recognised.features.risers.len(), 2);
    assert_eq!(recognised.features.chamfers.len(), 1);
    assert!(recognised.features.oriented_chamfers.is_empty());
    assert!(recognised.features.fillets.is_empty());
    // The former fillet was an inward-facing (concave) R1 cylinder, not an external round.
    assert_eq!(part.frame_points_outward(28), Some(false));
    // Face 16 is the angled cut's concave blind cap, perpendicular to the slant run.
    assert_eq!(part.arc(32, 16), Some(quiddity::kernel::brep::Arc::Concave));
    let slant = quiddity::features::bevel::classify_bevel(&part, 32).unwrap();
    let cap = quiddity::features::planes::axis_aligned_axis(&part, 16).unwrap();
    assert_eq!(cap.0, slant.edge_axis);
    assert_eq!(part.faces[16].solid, part.faces[32].solid);
    for (family, faces) in [
        ("rectangular_blind_slots", vec![18, 19, 20, 29]),
        ("section_recesses", vec![18, 19, 20, 29]),
        ("interior_voids", vec![13, 14]),
        ("angled_steps", vec![32, 16]),
    ] {
        assert!(
            recognised.features.defining[family].contains(&faces),
            "{family}: {faces:?}"
        );
    }
}

/// #91: three parallel 30-degree slots (14 distinct surfaces) outvote their plate (6), and a
/// 17-degree slot outvotes its two plates (10 against 8), but the vote only settles the leading
/// line (Z). About it, the plate's own roll has the smaller box around the material (0.53 and
/// 0.44 times the slots' box) and sets the frame. cgb241's sheet keeps its file-axis outline
/// (box 0.94 times that of its 27.4-degree bend walls, 18 votes against 20). The rule never
/// reads the file's axes: a generic motion carries every one of these frames exactly.
#[test]
fn tightest_roll_keeps_plates_and_sheets_on_their_outline() {
    let root = common::fixtures();
    let mut paths = vec![
        (root.join("golden_oriented_slots.step"), [0., 1., 0.]),
        (root.join("captured/3f89fd15d4993366.step.gz"), [0., 1., 0.]),
    ];
    if let Some(dir) = common::corpus_dir() {
        paths.push((dir.join("cadgenbench_inputs/cgb241.step"), [-1., 0., 0.]));
    }
    for (path, expected_y) in paths {
        let part = quiddity::read_step_file(&path).unwrap();
        let frame = infer_part_frame(&part).unwrap();
        assert_eq!(frame.x, [0., 0., 1.], "{path:?}");
        assert!(
            largest_difference(frame.y, expected_y) < 1e-12,
            "{path:?}: {frame:?}"
        );
        let placement = motion([1., 2., 3.], 37., [123.456, -78.9, 41.3]);
        let moved = part.transformed(&placement).unwrap();
        let moved_frame = infer_part_frame(&moved).unwrap();
        for (a, b) in [
            (frame.x, moved_frame.x),
            (frame.y, moved_frame.y),
            (frame.z, moved_frame.z),
        ] {
            let carried = apply(&placement, a, false);
            // An orthogonal gauge may reverse a representative axis; a full one may not.
            let along = carried.iter().zip(&b).map(|(p, q)| p * q).sum::<f64>();
            assert!(
                (along.abs() - 1.).abs() < 1e-9 && (frame.gauge != FrameGauge::Full || along > 0.),
                "{path:?}: {frame:?} -> {moved_frame:?}"
            );
        }
        assert_eq!(frame.gauge, moved_frame.gauge, "{path:?}");
    }
}
