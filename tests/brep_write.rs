//! The B-rep writer's round trip (`haecceity::brep_write`, design: `docs/brep-writer.md`): a part
//! is read, written and read again. The part read back must be the part read first, bit for bit
//! (faces, edges and solids in the same order, the same geometry, so the same areas and
//! volumes), recognise to the same records and document, pass the EXPRESS schema and rule
//! checks, and write to the same bytes twice. A part the writer refuses must be refused by name.
//! A part moved after reading is written where it lies and reads back within round-off. PMI:
//! what is carried reads back as written, and everything else is named.
//!
//! The default tests run a fixed sample that covers every surface and curve kind and the special
//! cases (voids, surface models, assemblies, a repaired part, refusals, PMI with refusals), and
//! a few corpus parts when `QUIDDITY_CORPUS` names the corpus. The full sweeps (every fixture,
//! every captured fixture, every corpus part, every AP242 fixture's PMI; about 20 minutes of
//! CPU) are ignored by default:
//!
//! ```text
//! QUIDDITY_CORPUS=../quiddity-corpus cargo test --release --test brep_write -- --ignored
//! ```

#[path = "common/parallel.rs"]
mod parallel;

use std::io::Read;
use std::path::{Path, PathBuf};

use quiddity::kernel::brep_write::{WriteError, WriteOptions, write_step};
use quiddity::kernel::{self, pmi};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// The shared corpus, as the corpus tests find it (`QUIDDITY_CORPUS`, `off` to skip); its
/// absence fails when `QUIDDITY_CORPUS_REQUIRED` is set.
fn corpus_dir() -> Option<PathBuf> {
    let dir = if std::env::var("QUIDDITY_CORPUS").is_ok_and(|v| v == "off") {
        None
    } else {
        let dir = std::env::var("QUIDDITY_CORPUS")
            .map(PathBuf::from)
            .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../quiddity-corpus"));
        dir.is_dir().then_some(dir)
    };
    if dir.is_none() {
        assert!(
            std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
            "QUIDDITY_CORPUS_REQUIRED is set but the corpus was not found"
        );
        eprintln!("corpus not found; set QUIDDITY_CORPUS");
    }
    dir
}

/// The STEP files under *dir*, sorted, skipping the subdirectories named.
fn step_files(dir: &Path, skip: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if !skip.iter().any(|s| path.ends_with(s)) {
                    stack.push(path);
                }
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().to_lowercase();
            if [".step", ".stp", ".step.gz", ".stp.gz"]
                .iter()
                .any(|e| name.ends_with(e))
            {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn file_bytes(path: &Path) -> Vec<u8> {
    let raw = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    if path.extension().is_some_and(|e| e == "gz") {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(raw.as_slice())
            .read_to_end(&mut out)
            .unwrap();
        out
    } else {
        raw
    }
}

/// The recognition `quiddity <file.step>` prints (its `source_sha256` aside, which names the
/// file's bytes): the records and the document.
fn recognition(part: &kernel::Part, without_repairs: bool) -> String {
    let mut recognition = quiddity::correspondence::recognise(part);
    if without_repairs {
        recognition.input_repairs.clear();
    }
    let document = quiddity::recognition::document(&recognition, part.faces.len());
    let output = quiddity::serve::RecognitionOutput::new(recognition, document);
    serde_json::to_string(&output).expect("records serialise")
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Outcome {
    /// Read back as the same part, recognised the same.
    Same,
    /// Read back as the part reading repaired, recognised the same but for the repair's record:
    /// the file holds the repaired part, so reading it back repairs nothing.
    SameRepaired,
    /// Refused by the writer, naming what it cannot write.
    Refused,
    /// The input is not a readable part (the broken fixtures).
    Unreadable,
}

/// Read, write and read *path*; the outcome, or every problem found.
fn round_trip(path: &Path) -> Result<Outcome, Vec<String>> {
    let name = path.display().to_string();
    let Ok(part) = kernel::read_step(&file_bytes(path)) else {
        return Ok(Outcome::Unreadable);
    };
    let options = WriteOptions::new("round trip").with_pmi(None);
    let (bytes, report) = match write_step(&part, &options) {
        Ok(written) => written,
        Err(WriteError::Unsupported(why)) if !why.is_empty() => return Ok(Outcome::Refused),
        Err(e) => return Err(vec![format!("{name}: {e}")]),
    };
    let mut problems = Vec::new();
    match write_step(&part, &options) {
        Ok((again, _)) if again == bytes => {}
        _ => problems.push(format!("{name}: a second write differs")),
    }
    if !report.face_order_preserved || !report.edge_order_preserved {
        problems.push(format!("{name}: face or edge order not preserved"));
    }
    if report.placements_flattened || !report.surface_kinds_changed.is_empty() {
        problems.push(format!("{name}: geometry not written as read: {report:?}"));
    }
    if !report.pcurves_not_written.is_empty() {
        problems.push(format!(
            "{name}: pcurves not written: {:?}",
            report.pcurves_not_written
        ));
    }
    let back = match kernel::read_step(&bytes) {
        Ok(back) => back,
        Err(e) => return Err(vec![format!("{name}: the written file does not read: {e}")]),
    };
    if (part.faces.len(), part.edges.len(), part.solids.len())
        != (back.faces.len(), back.edges.len(), back.solids.len())
    {
        problems.push(format!(
            "{name}: {} faces, {} edges, {} solids read back as {}, {}, {}",
            part.faces.len(),
            part.edges.len(),
            part.solids.len(),
            back.faces.len(),
            back.edges.len(),
            back.solids.len()
        ));
        return Err(problems);
    }
    for f in 0..part.faces.len() {
        if format!("{:?}", part.faces[f]) != format!("{:?}", back.faces[f]) {
            problems.push(format!("{name}: face {f} reads back otherwise"));
        }
        let (a, b) = (part.face_mass(f), back.face_mass(f));
        if a.map(|m| m[0].to_bits()) != b.map(|m| m[0].to_bits()) {
            problems.push(format!("{name}: face {f} area {a:?} reads back as {b:?}"));
        }
    }
    for e in 0..part.edges.len() {
        if format!("{:?}", part.edges[e]) != format!("{:?}", back.edges[e]) {
            problems.push(format!("{name}: edge {e} reads back otherwise"));
        }
    }
    for s in 0..part.solids.len() {
        if part.solids[s].faces != back.solids[s].faces {
            problems.push(format!("{name}: solid {s}'s faces read back otherwise"));
        }
        let (a, b) = (part.solid_mass(s), back.solid_mass(s));
        if a.map(|m| m.0.to_bits()) != b.map(|m| m.0.to_bits()) {
            problems.push(format!(
                "{name}: solid {s} volume {a:?} reads back as {b:?}"
            ));
        }
    }
    // A part read through a placement is written in its own frame and placed as the file placed
    // it: moving both alike must agree too (recognition moves parts into their own frame).
    let at = [
        [0.6, -0.8, 0.0, 1.5],
        [0.8, 0.6, 0.0, -2.0],
        [0.0, 0.0, 1.0, 3.0],
    ];
    let (moved, moved_back) = (
        part.transformed(&at).unwrap(),
        back.transformed(&at).unwrap(),
    );
    if (0..part.faces.len())
        .any(|f| format!("{:?}", moved.faces[f]) != format!("{:?}", moved_back.faces[f]))
        || (0..part.edges.len())
            .any(|e| format!("{:?}", moved.edges[e]) != format!("{:?}", moved_back.edges[e]))
    {
        problems.push(format!(
            "{name}: moved, the part read back differs from the part"
        ));
    }
    let doc = kernel::p21::Document::parse(bytes).unwrap();
    for v in kernel::express::validate_document(doc.graph())
        .iter()
        .take(3)
    {
        problems.push(format!("{name}: EXPRESS: {v}"));
    }
    for v in kernel::express_rules::check_all(&doc).iter().take(3) {
        problems.push(format!(
            "{name}: rule {} on #{} {}: {}",
            v.rule, v.id, v.entity, v.message
        ));
    }
    let repaired = !part.input_repairs().is_empty();
    if repaired
        && (report.repairs_written != part.input_repairs() || !back.input_repairs().is_empty())
    {
        problems.push(format!(
            "{name}: the repair is not reported, or is made again"
        ));
    }
    if problems.is_empty() && recognition(&part, repaired) != recognition(&back, false) {
        problems.push(format!("{name}: recognises otherwise"));
    }
    if problems.is_empty() {
        Ok(if repaired {
            Outcome::SameRepaired
        } else {
            Outcome::Same
        })
    } else {
        Err(problems)
    }
}

/// Round-trip *files*: every problem, and the count of each outcome.
fn check(files: &[PathBuf]) -> std::collections::BTreeMap<String, usize> {
    let results = parallel::map(files, |p| round_trip(p));
    let mut counts = std::collections::BTreeMap::new();
    let mut problems = Vec::new();
    for r in results {
        match r {
            Ok(o) => *counts.entry(format!("{o:?}")).or_insert(0) += 1,
            Err(p) => {
                *counts.entry("Problem".to_owned()).or_insert(0) += 1;
                problems.extend(p);
            }
        }
    }
    eprintln!("{counts:?}");
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    counts
}

/// The sample: every surface and curve kind the writer writes, and its special cases.
const SAMPLE: [&str; 22] = [
    // Planes, cylinders, lines, circles; a part placed by an assembly.
    "golden_simple_through_hole.step",
    "golden_polygonal_stock.step",
    // Tori, placed circles; a full torus with seams.
    "golden_toroidal_blends_turned.step",
    "full_torus.step",
    "slanted_bead.step",
    "captured/8cd5c2e86c7a8c9c.step.gz",
    // Hyperbola and parabola edges.
    "hyperbola_cut_cone.step",
    "parabola_cut_cone.step",
    // Voids and vertex loops; ellipses; spheres; cones.
    "captured/c2c392ec476f9440.step.gz",
    "captured/be6b5f9eebea62e5.step.gz",
    "captured/frames/sphere.step",
    "captured/40574cca69345a01.step.gz",
    // Rational B-spline surfaces and curves.
    "captured/356850c7a85fab15.step.gz",
    // Surfaces of revolution (line, circle and B-spline profiles) and of extrusion.
    "revolution_cone.step.gz",
    "revolution_shaft.step.gz",
    "captured/outer_profiles/parts/fd1d7a692544e212.step.gz",
    // Surface models (open shells, a B-spline one).
    "open_shell.step",
    "late_turn.step",
    // A nested assembly; a part reading repairs (an inverted shell).
    "ap242/assembly/nested.step",
    "truck_master_shaft_only.step.gz",
    // Geometry that did not resolve: refused by name.
    "broken/unresolved_open_curve.step",
    "broken/unresolved_surface.step",
];

/// Small corpus parts for the default run: OCCT-written B-rep with B-spline pcurves, a NIST
/// file with a surface model and a coarse uncertainty, a GRAMEL part with B-spline faces.
const CORPUS_SAMPLE: [&str; 4] = [
    "mfcadpp/10038.step",
    "mfcadpp_holdout/118.step",
    "nist/nist_ctc_01_asme1_rd.stp",
    "gramel/string_post.step",
];

fn sample() -> Vec<PathBuf> {
    SAMPLE.iter().map(|f| fixtures().join(f)).collect()
}

fn corpus_sample(dir: &Path) -> Vec<PathBuf> {
    CORPUS_SAMPLE.iter().map(|f| dir.join(f)).collect()
}

#[test]
fn sample_parts_round_trip() {
    let counts = check(&sample());
    let expected: std::collections::BTreeMap<String, usize> = [
        ("Same".to_owned(), 19),
        ("SameRepaired".to_owned(), 1),
        ("Refused".to_owned(), 2),
    ]
    .into();
    assert_eq!(counts, expected);
}

#[test]
fn corpus_sample_round_trips() {
    let Some(dir) = corpus_dir() else { return };
    let counts = check(&corpus_sample(&dir));
    assert_eq!(counts.keys().collect::<Vec<_>>(), ["Same"], "{counts:?}");
}

#[test]
#[ignore = "full sweep, about 1 minute of CPU: run with --ignored"]
fn fixture_parts_round_trip() {
    let counts = check(&step_files(&fixtures(), &["captured"]));
    // The two broken fixtures with geometry that did not resolve are refused, by name; one
    // fixture's inverted shell is repaired on reading and written repaired.
    assert_eq!(counts.get("Refused"), Some(&2), "{counts:?}");
    assert_eq!(counts.get("SameRepaired"), Some(&1), "{counts:?}");
    assert!(counts.get("Same").is_some_and(|&n| n >= 160), "{counts:?}");
}

#[test]
#[ignore = "full sweep, about 10 minutes of CPU: run with --ignored"]
fn captured_fixture_parts_round_trip() {
    let counts = check(&step_files(&fixtures().join("captured"), &[]));
    assert_eq!(counts.keys().collect::<Vec<_>>(), ["Same"], "{counts:?}");
}

#[test]
#[ignore = "full sweep, about 4 minutes of CPU: run with --ignored and QUIDDITY_CORPUS"]
fn corpus_parts_round_trip() {
    let Some(dir) = corpus_dir() else { return };
    let counts = check(&step_files(&dir, &[]));
    assert_eq!(counts.keys().collect::<Vec<_>>(), ["Same"], "{counts:?}");
}

#[test]
fn unresolved_geometry_is_refused_by_name() {
    for (file, named) in [
        ("broken/unresolved_open_curve.step", "edge 1"),
        ("broken/unresolved_surface.step", "face 1"),
    ] {
        let part = kernel::read_step_file(&fixtures().join(file)).unwrap();
        let options = WriteOptions::new("refused").with_pmi(None);
        match write_step(&part, &options) {
            Err(WriteError::Unsupported(why)) => {
                assert!(why.iter().any(|w| w.starts_with(named)), "{file}: {why:?}")
            }
            other => panic!("{file}: not refused: {:?}", other.map(|(_, r)| r)),
        }
    }
}

/// Whether two JSON values agree, numbers to one part in 1e-6 (absolutely below 1e-6): what a
/// part read back an ulp away from where it was written may change.
fn close(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    use serde_json::Value as V;
    match (a, b) {
        (V::Number(x), V::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            (x - y).abs() <= 1e-6 * x.abs().max(y.abs()).max(1.0)
        }
        (V::Array(x), V::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| close(a, b))
        }
        (V::Object(x), V::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| close(v, w)))
        }
        _ => a == b,
    }
}

/// Move *path*'s part by *at*, write it, read it back: the problems found. The part read back
/// must have the moved part's counts, its areas and volumes within 1e-9, and, when *recognise*,
/// its recognition within 1e-6.
fn moved_round_trip(path: &Path, at: &kernel::step::Placement, recognise: bool) -> Vec<String> {
    let name = path.display().to_string();
    let Ok(part) = kernel::read_step(&file_bytes(path)) else {
        return vec![format!("{name}: unreadable")];
    };
    let moved = part.transformed(at).unwrap();
    let options = WriteOptions::new("moved").with_pmi(None);
    let (bytes, report) = match write_step(&moved, &options) {
        Ok(w) => w,
        Err(e) => return vec![format!("{name}: {e}")],
    };
    let mut problems = Vec::new();
    if !report.placements_flattened {
        problems.push(format!(
            "{name}: a moved part not reported as written where it lies"
        ));
    }
    let back = match kernel::read_step(&bytes) {
        Ok(b) => b,
        Err(e) => return vec![format!("{name}: the written file does not read: {e}")],
    };
    if (moved.faces.len(), moved.edges.len(), moved.solids.len())
        != (back.faces.len(), back.edges.len(), back.solids.len())
    {
        return vec![format!("{name}: counts differ")];
    }
    let near = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0);
    for f in 0..moved.faces.len() {
        if let (Some(a), Some(b)) = (moved.face_mass(f), back.face_mass(f))
            && !near(a[0], b[0])
        {
            problems.push(format!(
                "{name}: face {f} area {} reads back as {}",
                a[0], b[0]
            ));
        }
    }
    for s in 0..moved.solids.len() {
        if let (Some(a), Some(b)) = (moved.solid_mass(s), back.solid_mass(s))
            && !near(a.0, b.0)
        {
            problems.push(format!(
                "{name}: solid {s} volume {} reads back as {}",
                a.0, b.0
            ));
        }
    }
    if !recognise {
        return problems;
    }
    // The file holds a repaired part repaired: the repair's record aside.
    let repaired = !moved.input_repairs().is_empty();
    let doc = |p: &kernel::Part, r: bool| -> serde_json::Value {
        serde_json::from_str(&recognition(p, r)).unwrap()
    };
    let (a, b) = (doc(&moved, repaired), doc(&back, false));
    if !close(&a, &b) {
        problems.push(format!("{name}: recognises otherwise beyond round-off"));
    }
    problems
}

/// A moved part (the writer's purpose: output that is ours) is written where it lies, reported
/// as such. Under a right-angle rotation and a non-round translation (the motions the invariance
/// tests use, which add no round-off of their own) it reads back with the same counts, areas and
/// volumes within 1e-9 and the same recognition within 1e-6. Under a general rotation the moved
/// part's frames are orthonormal only to round-off, and normalising them on reading moves them by
/// an ulp: the counts, areas and volumes are checked (recognition thresholds may then fall either
/// side, as they may for the moved part itself).
#[test]
fn moved_parts_round_trip_within_round_off() {
    let mut files: Vec<PathBuf> = sample()
        .into_iter()
        .filter(|p| !p.to_string_lossy().contains("broken/"))
        .collect();
    if let Some(dir) = corpus_dir() {
        files.extend(corpus_sample(&dir));
    }
    let right_angle = [
        [0.0, -1.0, 0.0, 12.5],
        [0.0, 0.0, -1.0, -7.25],
        [1.0, 0.0, 0.0, 3.0],
    ];
    let (c, s) = (0.6_f64, 0.8_f64);
    let general = [
        [c, -s * 0.6, s * 0.8, 12.5],
        [s, c * 0.6, -c * 0.8, -7.25],
        [0.0, 0.8, 0.6, 3.0],
    ];
    let problems: Vec<String> = parallel::map(&files, |p| {
        let mut out = moved_round_trip(p, &right_angle, true);
        out.extend(moved_round_trip(p, &general, false));
        out
    })
    .into_iter()
    .flatten()
    .collect();
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// A file's parts with their PMI: what is carried reads back as written (in the PMI writer's
/// standard form), and the source's items not carried are exactly those named as refused (with
/// what refers to them). Counts parts carried whole and with refusals named.
fn pmi_check(files: &[PathBuf]) -> (usize, usize) {
    let mut problems = Vec::new();
    let (mut carried_whole, mut with_refusals) = (0, 0);
    for path in files {
        let bytes = file_bytes(path);
        let Ok(defs) = kernel::step::read_part_definitions(&bytes) else {
            continue;
        };
        let name = path.display().to_string();
        let doc = kernel::p21::Document::parse(bytes.clone()).unwrap();
        let read = pmi::read(&doc, &defs).unwrap();
        for (k, def) in defs.iter().enumerate() {
            let part = kernel::step::read_part(&bytes, def).unwrap();
            let source = &read.parts[k];
            let options = WriteOptions::new(&def.name).with_pmi(Some(source));
            let (written, report) = match write_step(&part, &options) {
                Ok(w) => w,
                Err(e) => {
                    problems.push(format!("{name} part {k}: {e}"));
                    continue;
                }
            };
            let carried = report.pmi.expect("PMI was given");
            let refused: Vec<_> = carried.refused.iter().map(|r| r.item).collect();
            let mut expected = pmi::without(source, &refused);
            expected.holes.clear();
            if !pmi::write::differences_as_stated(&carried.written, &expected).is_empty() {
                problems.push(format!(
                    "{name} part {k}: what was written is not the source less what was named"
                ));
            }
            if carried.refused.iter().any(|r| r.why.is_empty()) {
                problems.push(format!("{name} part {k}: a refusal without a reason"));
            }
            let defs1 = kernel::step::read_part_definitions(&written).unwrap();
            let doc1 = kernel::p21::Document::parse(written).unwrap();
            let back = pmi::read(&doc1, &defs1).unwrap();
            for d in pmi::write::differences_as_stated(
                &pmi::write::standard_form(&carried.written),
                &back.parts[0],
            ) {
                problems.push(format!("{name} part {k}: {d}"));
            }
            if carried.refused.is_empty() {
                carried_whole += 1;
            } else {
                with_refusals += 1;
            }
        }
    }
    eprintln!("PMI carried whole: {carried_whole}; with refusals named: {with_refusals}");
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    (carried_whole, with_refusals)
}

/// PMI over a sample: every written kind, an assembly's two parts, CTC-01 including
/// its dimensions without a value, and the named GRM-03 carry failure.
#[test]
fn sample_pmi_is_carried_or_refused_by_name() {
    let files: Vec<PathBuf> = [
        "ap242/write/every_kind.step.gz",
        "ap242/write/datum_targets.step.gz",
        "ap242/specify/bolt_thread_knurl.step.gz",
        "ap242/assembly/assembly.step",
        "ap242/nist/nist_ctc_01_asme1_ap242-e1.stp.gz",
        "ap242/derived/GRM-03_thumbwheel_drive_screw_AP242_PMI.step.gz",
    ]
    .iter()
    .map(|f| fixtures().join(f))
    .collect();
    let (whole, refused) = pmi_check(&files);
    assert!(
        whole >= 5 && refused == 1,
        "{whole} whole, {refused} with refusals"
    );
}

#[test]
#[ignore = "full sweep, about 1 minute of CPU: run with --ignored"]
fn pmi_is_carried_or_refused_by_name() {
    let (whole, _) = pmi_check(&step_files(&fixtures().join("ap242"), &[]));
    assert!(whole > 20, "{whole}");
}

/// The source's product names are kept: an assembly's and its components', and a part's.
#[test]
fn product_names_are_kept() {
    for (file, names) in [
        (
            "ap242/write/editor_assembly_pin_alone.step.gz",
            &["assembly", "plate", "pin"][..],
        ),
        ("golden_simple_through_hole.step", &[][..]),
    ] {
        let path = fixtures().join(file);
        let source = String::from_utf8_lossy(&file_bytes(&path)).into_owned();
        let part = kernel::read_step(&file_bytes(&path)).unwrap();
        let options = WriteOptions::new("unused").with_pmi(None);
        let (bytes, report) = write_step(&part, &options).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            report.products_renamed.is_empty(),
            "{file}: {:?}",
            report.products_renamed
        );
        assert!(!text.contains("unused"), "{file}");
        for name in names {
            assert!(
                text.contains(&format!("PRODUCT('{name}','{name}'")),
                "{file}: {name}"
            );
        }
        // Every product name written is one of the source's.
        for line in text.lines().filter(|l| l.contains("= PRODUCT(")) {
            let name = line.split('\'').nth(3).unwrap();
            assert!(source.contains(&format!("'{name}'")), "{file}: {name}");
        }
    }
}

#[test]
fn programmatic_face_without_bounds_is_refused_by_name() {
    let part = kernel::read_step_file(&fixtures().join("golden_simple_through_hole.step")).unwrap();
    let mut faces = part.faces.clone();
    faces[0].loops.clear();
    let part = kernel::Part::new(faces, part.edges.clone(), part.solids.clone());
    let error = write_step(&part, &WriteOptions::new("boundless").with_pmi(None)).unwrap_err();
    assert!(error.to_string().contains("face 0: no bounds"), "{error}");
}
