//! Face areas against OpenCascade's: every corpus face's [`Part::face_mass`] area against
//! `face.area`, what Python's recognisers read (`BRepGProp::SurfaceProperties` with its fixed
//! Gauss rule, captured by `tools/capture_face_areas.py`), and against itself with the part
//! moved by a translation and a generic rotation.
//!
//! The port integrates eligible STEP master trims and otherwise projects the 3D edges.
//! OpenCascade integrates the pcurves on its imported face. Each difference and change under
//! motion must be listed in
//! `tests/fixtures/references/face_areas.json` with a verdict, pinned to the area the port gives
//! (`tools/known_face_areas.py` writes the list from the differences, the capture's further
//! OpenCascade references and the independent integrations of `tools/face_area_evidence.py`).
//! A file whose faces the reader does not walk in OpenCascade's order (a known inventory
//! divergence) is compared as one `order` entry instead of face by face.

mod common;

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use haecceity::Part;
use haecceity::geom::SurfaceType;
use haecceity::step::{IDENTITY, Placement, read_step_file_placed};
use serde_json::Value;

/// Agreement: relative, with a floor for faces too small for a relative test to mean anything.
const RELATIVE: f64 = 1e-6;
const FLOOR: f64 = 1e-9;
/// A placement must leave every area unchanged: each boundary panel is resolved to 1e-9 of its
/// own terms, and all but one corpus face agree to 1e-12 moved (cgb203 face 51, 4.9e-12).
/// Whether this threshold holds across platforms is open (docs/review-2026-10-08.md).
const PLACED: f64 = 1e-9;
/// A listed difference's pinned area has changed.
const PINNED: f64 = 1e-7;

/// A non-round translation and 37° about a direction off every axis and diagonal.
fn motion() -> Placement {
    let t = [123.456, -78.9, 41.3];
    let n = (1.0f64 + 4.0 + 9.0).sqrt();
    let (x, y, z) = (1.0 / n, 2.0 / n, 3.0 / n);
    let (s, c) = 37f64.to_radians().sin_cos();
    let k = 1.0 - c;
    [
        [c + x * x * k, x * y * k - z * s, x * z * k + y * s, t[0]],
        [y * x * k + z * s, c + y * y * k, y * z * k - x * s, t[1]],
        [z * x * k - y * s, z * y * k + x * s, c + z * z * k, t[2]],
    ]
}

/// A solid whose faces do not quite close (as most files' do not: edges stray from their
/// surfaces within tolerance) has the same volume wherever it is placed. The cylinder's top cap
/// is tilted by 0.01 rad off its rim circle, so the cap's area is the rim's foot points' ellipse
/// and the outward area vectors leave a residue of about 3 mm²: measured from the origin, a
/// translation by 1000 would move the volume by about a thousand.
#[test]
fn volume_ignores_placement_when_faces_do_not_close() {
    let text = std::fs::read_to_string(common::fixtures().join("rejected_cylinder.step")).unwrap();
    let tilted = "#46 = DIRECTION('',(0.0099998333,0.,0.99995));";
    let text = text.replace("#46 = DIRECTION('',(0.,0.,1.));", tilted);
    assert!(text.contains(tilted));
    let volume = |placement: &Placement| {
        let part = haecceity::step::read_step_placed(text.as_bytes(), placement).unwrap();
        part.solid_mass(0).unwrap().0
    };
    let mut shifted = IDENTITY;
    shifted[0][3] = 1000.0;
    shifted[1][3] = -700.0;
    let unmoved = volume(&IDENTITY);
    // πr²h, to within what the residue makes of a reference point about 10 from the axis.
    assert!(
        (unmoved - std::f64::consts::PI * 2000.0).abs() < 20.0,
        "{unmoved}"
    );
    for placement in [shifted, motion()] {
        let moved = volume(&placement);
        assert!(
            (moved - unmoved).abs() <= 1e-12 * unmoved,
            "{unmoved} moved {moved}"
        );
    }
}

/// A face whose edge runs just outside its B-spline surface's side u = 0 and crosses a knot
/// line there: the foot points are held on the side, where the second partials jump at the
/// knot. `held_side_knot.step`: S(u, v) = (10u, Y(v) + 5u, Z(v)), (Y, Z) quadratic with a knot
/// at v = 1/2, the face's region [0, 1/2] × [0.2, 0.8] with its u = 0 side drawn 0.01 outside
/// (and 0.05 along y, so the knot crossing is not at a knot of the edge's own curve). Its area
/// is ½ ∫ |(10, 5, 0) × (0, Y', Z')| dv, here by Simpson's rule on each polynomial piece. A
/// split placed where the inversion's own parameter (not the foot point's) crosses the knot, or
/// a difference straddling it, left 8e-10 of the area out (1e-8 on cgb242 face 546).
#[test]
fn held_side_crossing_a_knot_line() {
    // C'(v): linear from (12, 16) to (8, −6) on [0, ½], back to (12, 16) on [½, 1].
    let (q0, q1) = ([12.0, 16.0], [8.0, -6.0]);
    let speed = |v: f64| {
        let s = if v < 0.5 { 2.0 * v } else { 2.0 - 2.0 * v };
        let (y, z) = (q0[0] + (q1[0] - q0[0]) * s, q0[1] + (q1[1] - q0[1]) * s);
        0.5 * (100.0 * y * y + 125.0 * z * z).sqrt()
    };
    let simpson = |a: f64, b: f64| {
        let n = 20_000;
        let h = (b - a) / n as f64;
        let inner: f64 = (1..n)
            .map(|k| speed(a + k as f64 * h) * if k % 2 == 1 { 4.0 } else { 2.0 })
            .sum();
        h / 3.0 * (speed(a) + inner + speed(b))
    };
    let want = simpson(0.2, 0.5) + simpson(0.5, 0.8);
    let text = std::fs::read(common::fixtures().join("held_side_knot.step")).unwrap();
    for placement in [IDENTITY, motion()] {
        let part = haecceity::step::read_step_placed(&text, &placement).unwrap();
        let got = part.face_mass(0).unwrap()[0];
        assert!((got - want).abs() <= 1e-12 * want, "{got} vs {want}");
    }
}

/// A face whose own seam is not its closed B-spline surface's seam (cgb217 face 29): in
/// `rotated_seam_band.step` a rational B-spline cylinder (r 4, closed at u = 0 on +x) carries
/// the band z ∈ [−3, 3] with its seam edge at 45°, so both circles cross the surface's seam
/// mid-edge. The walk unwraps across it: area 2π·4·6, flux of the position vector r × area.
#[test]
fn rotated_seam_on_a_closed_bspline_surface() {
    let want = 48.0 * std::f64::consts::PI;
    let text = std::fs::read(common::fixtures().join("rotated_seam_band.step")).unwrap();
    for placement in [IDENTITY, motion()] {
        let part = haecceity::step::read_step_placed(&text, &placement).unwrap();
        let [area, flux] = part.face_mass(0).expect("integrated");
        assert!((area - want).abs() <= 1e-12 * want, "{area} vs {want}");
        if placement == IDENTITY {
            assert!((flux - 4.0 * want).abs() <= 1e-12 * want, "{flux}");
        }
        // The trimming domain unwraps the same way: the band is every u for v in [0.2, 0.8]
        // (z from −3 to 3 of −5 to 5), on either side of both seams.
        let domain = part.domain(0).unwrap();
        for u in [0.0, 0.05, 0.124, 0.126, 0.5, 0.95, 1.0] {
            assert!(domain.contains(u, 0.5), "u {u}");
            assert!(
                !domain.contains(u, 0.1) && !domain.contains(u, 0.9),
                "u {u}"
            );
        }
    }
}

/// A foot point that leaves a held side between an edge's end and the quadrature node beside
/// it (cgb203 face 3, 2.3e-4 of its area). `late_turn.step`: the flat B-spline square
/// S(u, v) = (10u, 10v, 0) carries (−1, 2), (0.001, 8), (5, 8), (5, 2), whose first side runs
/// outside u = 0 and enters the square 1e-3 of its length before its end; the region its foot
/// points bound is that quadrilateral clipped to x ≥ 0.
#[test]
fn foot_point_turning_next_to_an_edge_end() {
    let y = 2.0 + 6.0 / 1.001;
    let clipped = [(0.0, 2.0), (0.0, y), (0.001, 8.0), (5.0, 8.0), (5.0, 2.0)];
    let want = 0.5
        * (0..5)
            .map(|k| {
                let (a, b) = (clipped[k], clipped[(k + 1) % 5]);
                a.0 * b.1 - b.0 * a.1
            })
            .sum::<f64>()
            .abs();
    let text = std::fs::read(common::fixtures().join("late_turn.step")).unwrap();
    for placement in [IDENTITY, motion()] {
        let part = haecceity::step::read_step_placed(&text, &placement).unwrap();
        let got = part.face_mass(0).unwrap()[0];
        assert!((got - want).abs() <= 1e-12 * want, "{got} vs {want}");
    }
}

fn captured() -> Value {
    let path = common::fixtures().join("face_areas.json.gz");
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

/// A difference: the port's area, moved (for a placement difference), and a report.
type Found = (Option<f64>, Option<f64>, String);

/// The first face whose kind is not the one OpenCascade's traversal has there: faces the reader
/// does not walk in OpenCascade's order (tests/corpus.rs checks the order fully, with its
/// known divergences; kinds suffice to keep a reordered file's areas from being paired here).
fn misaligned(part: &Part, inventory: &[Value]) -> Option<String> {
    for (i, want) in inventory.iter().enumerate() {
        let got = match part.faces[i].surface.kind() {
            SurfaceType::Plane => "PLANE",
            SurfaceType::Cylinder => "CYLINDER",
            SurfaceType::Cone => "CONE",
            SurfaceType::Sphere => "SPHERE",
            SurfaceType::Torus => "TORUS",
            SurfaceType::Freeform | SurfaceType::Other => "OTHER",
        };
        let kind = want["type"].as_str().unwrap();
        let analytic = ["PLANE", "CYLINDER", "CONE", "SPHERE", "TORUS"].contains(&kind);
        if got != kind && (got != "OTHER" || analytic) {
            return Some(format!("face {i} is {got}, OpenCascade's is {kind}"));
        }
    }
    None
}

/// What one file's faces gave: differences by (file, face, check) with the port's area and a
/// report, and how many faces had a negative area.
#[derive(Default)]
struct Outcome {
    found: BTreeMap<(String, u64, String), Found>,
    negative: usize,
}

/// The relative change an area makes under motion, when both are there.
fn change(a: Option<f64>, b: Option<f64>) -> String {
    match (a, b) {
        (Some(a), Some(b)) => format!(
            " (relative change {:.1e})",
            (a - b).abs() / a.abs().max(b.abs())
        ),
        _ => String::new(),
    }
}

/// Every face of one file: against OpenCascade's `face.area`, and moved against unmoved.
fn check_file(dir: &Path, entry: &Value, inventory: &[Value], out: &mut Outcome) {
    let file = entry["file"].as_str().unwrap();
    let path = dir.join(file);
    let part = read_step_file_placed(&path, &IDENTITY).unwrap();
    let moved = read_step_file_placed(&path, &motion()).unwrap();
    let column = |name: &str| -> Vec<Option<f64>> {
        entry[name]
            .as_array()
            .unwrap()
            .iter()
            .map(Value::as_f64)
            .collect()
    };
    let (occ, fine, rebuilt) = (column("area"), column("area_fine"), column("area_3d"));
    let types = entry["types"].as_array().unwrap();
    assert_eq!(part.faces.len(), occ.len(), "{file}: face count");
    let area = |p: &Part, f: usize| p.face_mass(f).map(|m| m[0]);
    let order = misaligned(&part, inventory);
    if let Some(report) = &order {
        let areas: Vec<_> = (0..part.faces.len()).map(|f| area(&part, f)).collect();
        let key = (file.to_string(), 0, "order".to_string());
        out.found
            .insert(key, (None, None, format!("{report}; areas {areas:?}")));
    }
    for f in 0..occ.len() {
        let got = area(&part, f);
        let key = |check: &str| (file.to_string(), f as u64, check.to_string());
        if got.is_some_and(|g| g < 0.0) {
            out.negative += 1;
        }
        let near = matches!((got, occ[f]), (Some(g), Some(w)) if (g - w).abs() <= RELATIVE * w.abs() + FLOOR);
        if !near && order.is_none() {
            let report = format!(
                "{} rust {got:?} occ {:?} (adaptive {:?}, pcurves rebuilt {:?})",
                types[f], occ[f], fine[f], rebuilt[f]
            );
            out.found.insert(key("occ"), (got, None, report));
        }
        let placed = area(&moved, f);
        let same = match (got, placed) {
            (Some(a), Some(b)) => (a - b).abs() <= PLACED * a.abs().max(b.abs()) + FLOOR,
            (None, None) => true,
            _ => false,
        };
        if !same {
            out.found.insert(
                key("placement"),
                (
                    got,
                    placed,
                    format!(
                        "{} rust {got:?} moved {placed:?}{}",
                        types[f],
                        change(got, placed)
                    ),
                ),
            );
        }
    }
}

#[test]
fn face_areas_match_opencascade_and_ignore_placement() {
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let captured = captured();
    let entries = captured["files"].as_array().unwrap();
    let corpus = common::load("corpus.json");
    let inventories: BTreeMap<&str, &[Value]> = corpus["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["file"].as_str().unwrap(),
                e["inventory"].as_array().unwrap().as_slice(),
            )
        })
        .collect();
    let known = common::load("references/face_areas.json");
    let known = known.as_array().unwrap();
    common::check_verdicts("references/face_areas.json", known);
    // The parts are independent; read and integrate them across the machine's cores.
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(
            std::env::var("RUST_TEST_THREADS")
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(6)
                .max(1),
        );
    let outcomes: Vec<Outcome> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|w| {
                let (dir, inventories) = (&dir, &inventories);
                scope.spawn(move || {
                    let mut out = Outcome::default();
                    for entry in entries.iter().skip(w).step_by(threads) {
                        let inventory = inventories[entry["file"].as_str().unwrap()];
                        check_file(dir, entry, inventory, &mut out);
                    }
                    out
                })
            })
            .collect();
        workers.into_iter().map(|w| w.join().unwrap()).collect()
    });
    let mut found = BTreeMap::new();
    let mut negative = 0;
    for out in outcomes {
        found.extend(out.found);
        negative += out.negative;
    }
    eprintln!("{} differences", found.len());
    // For tools/known_face_areas.py, which writes the list from them and the evidence.
    if let Some(dump) = std::env::var_os("FACE_AREAS_DUMP") {
        let rows: Vec<Value> = found
            .iter()
            .map(|((file, face, check), (rust, moved, report))| {
                serde_json::json!({"file": file, "face": face, "check": check, "rust": rust, "moved": moved, "report": report})
            })
            .collect();
        std::fs::write(dump, serde_json::to_string_pretty(&rows).unwrap()).unwrap();
    }
    let mut problems = Vec::new();
    if negative > 0 {
        problems.push(format!("{negative} faces have a negative area"));
    }
    let mut listed = Vec::new();
    for k in known {
        let key = (
            k["file"].as_str().unwrap().to_string(),
            k["face"].as_u64().unwrap(),
            k["check"].as_str().unwrap().to_string(),
        );
        let pinned = (k["rust"].as_f64(), k["moved"].as_f64());
        let same = |got: Option<f64>, pin: Option<f64>| match (got, pin) {
            (Some(g), Some(p)) => (g - p).abs() <= PINNED * p.abs() + FLOOR,
            (None, None) => true,
            _ => false,
        };
        match found.get(&key) {
            None => problems.push(format!("{key:?} is listed but now agrees: remove it")),
            Some((got, moved, report)) => {
                if !same(*got, pinned.0) || !same(*moved, pinned.1) {
                    problems.push(format!(
                        "{key:?} is listed at {pinned:?} but changed: {report}"
                    ));
                }
            }
        }
        listed.push(key);
    }
    for (key, (_, _, report)) in &found {
        if !listed.contains(key) {
            problems.push(format!("{} face {} {}: {report}", key.0, key.1, key.2));
        }
    }
    assert!(
        problems.is_empty(),
        "{} problems:\n{}",
        problems.len(),
        problems.join("\n")
    );
}
