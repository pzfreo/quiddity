//! Rust effective-surface snapshots with unchanged material orientation, dependency and motion checks.

use super::common;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::PathBuf;

use quiddity::Part;
use quiddity::features::Context;
use quiddity::features::analytic_surfaces::{SurfaceKind, effective_fact};
use quiddity::features::effective_surfaces::{
    EffectiveFaces, SurfaceProvenance, SurfaceUse, cylinder_surface_dependency,
    physical_boundary_length, recovery_nominal, recovery_tolerance,
};
use quiddity::kernel::step::{Placement, read_step_file, read_step_file_placed};
use serde_json::{Value, json};

fn load_gz(name: &str) -> Value {
    let path = common::fixtures()
        .join("captured/effective_surfaces")
        .join(name);
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

fn kind_name(kind: SurfaceKind) -> &'static str {
    match kind {
        SurfaceKind::Plane => "plane",
        SurfaceKind::Cylinder => "cylinder",
        SurfaceKind::Cone => "cone",
        SurfaceKind::Sphere => "sphere",
    }
}

/// The port's answers for one face, shaped as the capture records Python's.
fn port_face(effective: &EffectiveFaces, part: &Part, face: usize) -> Value {
    let fact = match effective.fact(face) {
        Err(reason) => json!({"refused": reason.python_values()}),
        Ok(f) => json!({
            "kind": kind_name(f.kind()),
            "provenance": match f.provenance() {
                SurfaceProvenance::Native => "native",
                SurfaceProvenance::Recovered => "recovered",
            },
            "parameters": f.parameters(),
            "requested_tolerance": f.requested_tolerance,
        }),
    };
    let mut lengths = match (recovery_nominal(part, face), recovery_tolerance(part, face)) {
        (Some(n), Some(t)) => json!({"nominal": n, "tolerance": t}),
        _ => json!({"refused": true}),
    };
    // What the nominal is made of, for the message.
    lengths["area"] = json!(part.recognition_part().face_mass(face).map(|m| m[0]));
    lengths["perimeter"] = json!(physical_boundary_length(part, face));
    // Independent of Python: the face itself is oriented (a recovered primitive is not, but the
    // face's own surface and orientation still are), so its outward normal at each sample must
    // agree with the certified side.
    let orientation = match effective.surface_use(face, true) {
        Ok(SurfaceUse {
            material_side: Some(side),
            ..
        }) => json!(
            side.sample_points
                .iter()
                .zip(&side.outward_samples)
                .all(|(p, o)| part.outward_at(face, *p).is_some_and(|n| n
                    .iter()
                    .zip(o)
                    .map(|(a, b)| a * b)
                    .sum::<f64>()
                    > 0.5))
        ),
        _ => Value::Null,
    };
    let surface_use = match effective.surface_use(face, true) {
        Err(r) => json!({"refused": r.reason.python_value()}),
        Ok(u) => {
            let side = u.material_side.as_ref().expect("asked for a material side");
            json!({
                "kind": kind_name(u.surface.kind()),
                "native": u.surface.oriented(),
                "sign": side.candidate_outward_sign,
                "outward": side.outward,
                "probe_distance": side.probe_distance,
            })
        }
    };
    json!({"fact": fact, "nominal": lengths, "use": surface_use, "orientation": orientation})
}

/// A difference: (file, field) and the face, with what each side said.
type Difference = ((String, String), usize, String);

/// Checks the differences against the known list (entries for the given fields only): every
/// difference listed exactly once, every listed face differing.
fn check_known(found: Vec<Difference>, fields: &dyn Fn(&str) -> bool, files: &BTreeSet<String>) {
    check_reference(
        found,
        fields,
        files,
        "references/effective_surface_motion.json",
    );
}

fn check_reference(
    found: Vec<Difference>,
    fields: &dyn Fn(&str) -> bool,
    files: &BTreeSet<String>,
    reference: &str,
) {
    let known = common::load(reference);
    let entries: Vec<&Value> = known
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| fields(e["field"].as_str().unwrap()))
        .collect();
    common::check_verdicts(
        reference,
        &entries.iter().map(|e| (*e).clone()).collect::<Vec<_>>(),
    );
    let mut listed: BTreeMap<(String, String), BTreeSet<usize>> = BTreeMap::new();
    let mut problems = Vec::new();
    for e in &entries {
        let key = (
            e["file"].as_str().unwrap().to_string(),
            e["field"].as_str().unwrap().to_string(),
        );
        let faces = listed.entry(key.clone()).or_default();
        for f in e["faces"].as_array().unwrap() {
            let f = f.as_u64().unwrap() as usize;
            if !faces.insert(f) {
                problems.push(format!("{key:?} face {f} is listed twice"));
            }
        }
    }
    let mut unlisted: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
    let mut seen: BTreeMap<(String, String), BTreeSet<usize>> = BTreeMap::new();
    for (key, face, message) in found {
        seen.entry(key.clone()).or_default().insert(face);
        if !listed.get(&key).is_some_and(|l| l.contains(&face)) {
            problems.push(format!("{} {} face {face}: {message}", key.0, key.1));
            unlisted.entry(key).or_default().push(face);
        }
    }
    for (key, faces) in &listed {
        if !files.contains(&key.0) {
            continue;
        }
        let stale: Vec<usize> = faces
            .iter()
            .filter(|f| !seen.get(key).is_some_and(|s| s.contains(f)))
            .copied()
            .collect();
        if !stale.is_empty() {
            problems.push(format!("{} {}: stale faces {stale:?}", key.0, key.1));
        }
    }
    if !unlisted.is_empty() {
        let needed: Vec<Value> = unlisted
            .iter()
            .map(|((file, field), faces)| {
                json!({"file": file, "field": field, "faces": faces,
                       "verdict": "undetermined", "reason": "TODO"})
            })
            .collect();
        problems.push(format!(
            "entries needed:\n{}",
            serde_json::to_string_pretty(&needed).unwrap()
        ));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

struct Run<'v> {
    source: &'v str,
    file: &'v str,
    faces: &'v [Value],
    partial: bool,
}

fn runs(captured: &Value) -> Vec<Run<'_>> {
    captured["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| Run {
            source: r["source"].as_str().unwrap(),
            file: r["file"].as_str().unwrap(),
            // A part Python refused, or whose answering died, keeps the faces it answered.
            faces: r["faces"].as_array().map_or(&[][..], Vec::as_slice),
            partial: r["partial"] == true,
        })
        .collect()
}

fn path(run: &Run, corpus: &Option<PathBuf>) -> Option<PathBuf> {
    Some(match run.source {
        "test" => common::fixtures()
            .join("captured/effective_surfaces")
            .join(run.file),
        "fixture" => common::fixtures().join(run.file),
        _ => corpus.as_ref()?.join(run.file),
    })
}

fn corpus() -> Option<PathBuf> {
    let corpus = common::corpus_dir();
    if corpus.is_none() {
        assert!(
            std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
            "QUIDDITY_CORPUS_REQUIRED is set but the corpus was not found"
        );
    }
    corpus
}

const FIELDS: [&str; 4] = ["fact", "nominal", "use", "orientation"];

#[test]
fn rust_snapshot_effective_surfaces() {
    let captured = load_gz("faces.json.gz");
    let runs = runs(&captured);
    let corpus = corpus();
    let files: BTreeSet<String> = runs
        .iter()
        .filter(|r| path(r, &corpus).is_some())
        .map(|r| r.file.to_string())
        .collect();
    let results = common::parallel::map(&runs, |run| {
        let Some(path) = path(run, &corpus) else {
            return (0, Vec::new());
        };
        let part = read_step_file(&path).unwrap_or_else(|e| panic!("{}: {e}", run.file));
        if !run.partial {
            assert_eq!(
                part.faces.len(),
                run.faces.len(),
                "{}: face count",
                run.file
            );
        }
        let ctx = Context::new(&part);
        let effective = EffectiveFaces::new(&ctx);
        let out = Vec::new();
        let mut snapshots = Vec::new();
        for captured in run.faces {
            let face = captured["face"].as_u64().unwrap() as usize;
            let port = port_face(&effective, &part, face);
            // The fact is the one the ported families already read.
            assert_eq!(
                effective.fact(face).as_ref().ok().map(|f| &f.analytic),
                effective_fact(&part, face).as_ref(),
                "{} face {face}",
                run.file
            );
            // `cylinder_surface_dependency` certifies exactly the recovered faces.
            let dependency = cylinder_surface_dependency(&effective, face);
            let recovered = effective
                .fact(face)
                .as_ref()
                .is_ok_and(|f| f.provenance() == SurfaceProvenance::Recovered);
            assert_eq!(
                dependency.as_ref().map(|u| u.material_side.is_some()),
                effective
                    .surface_use(face, recovered)
                    .as_ref()
                    .map(|u| u.material_side.is_some()),
            );
            assert_ne!(port["orientation"], false, "{} face {face}", run.file);
            snapshots.push(json!({"face":face,"surface":port}));
        }
        common::snapshot::check_with(
            &common::fixtures(),
            &format!("effective_surfaces/{}/{}", run.source, run.file),
            &json!(snapshots),
            &|a, b| common::snapshot::same(&without_curved_outward(a), &without_curved_outward(b)),
        );
        (run.faces.len(), out)
    });
    // The orientation check is the port's own, not a captured answer.
    let calls: usize = results.iter().map(|r| r.0).sum::<usize>() * (FIELDS.len() - 1);
    let found: Vec<Difference> = results.into_iter().flat_map(|r| r.1).collect();
    eprintln!(
        "effective surfaces: {calls} captured answers, {} differ",
        found.len()
    );
    check_known(found, &|f| FIELDS.contains(&f), &files);
}

/// A snapshot without the `outward` of a curved surface's side: a cylinder's is the radial
/// direction at its first sample, which is presentation (which sample comes first differs
/// between platforms), so only a plane's normal is compared, as the parity test did. The
/// side itself stays compared through `sign` and `probe_distance`.
fn without_curved_outward(snapshot: &Value) -> Value {
    let mut snapshot = snapshot.clone();
    for face in snapshot.as_array_mut().into_iter().flatten() {
        let surface_use = &mut face["surface"]["use"];
        if surface_use["kind"] != "plane"
            && let Some(u) = surface_use.as_object_mut()
        {
            u.remove("outward");
        }
    }
    snapshot
}

const IDENTITY: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// A named motion: rotation rows and translation.
type Motion = (&'static str, [[f64; 3]; 3], [f64; 3]);

/// Two of `tests/invariance.rs`'s rigid motions: one moving every axis and the origin, and one
/// reversing two axes.
const MOTIONS: [Motion; 2] = [
    (
        "rot_zx_moved",
        [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        [123.456, -78.9, 41.3],
    ),
    (
        "rot_y180",
        [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]],
        [0.0; 3],
    ),
];

/// What does not depend on placement: the fact's kind, provenance, radius or semi-angle's size, or
/// its refusal; the nominal; the certificate's probe distance and outward side, or its refusal.
/// The canonical candidate normal (dominant component non-negative) turns with the axes, and so
/// does a cone's signed semi-angle: a plane's side is its outward normal turned back by the
/// motion's rotation *r*, a cylinder's the sign (whether its outward normal points away from the
/// axis).
fn placement_free(
    effective: &EffectiveFaces,
    part: &Part,
    face: usize,
    r: &[[f64; 3]; 3],
) -> [Value; 3] {
    let fact = match effective.fact(face) {
        Err(reason) => json!({"refused": reason.python_values()}),
        Ok(f) => json!({
            "kind": kind_name(f.kind()),
            "native": f.oriented(),
            "size": match f.kind() {
                SurfaceKind::Plane => Value::Null,
                SurfaceKind::Cylinder | SurfaceKind::Cone => json!(f.parameters()[6].abs()),
                SurfaceKind::Sphere => json!(f.parameters()[3]),
            },
        }),
    };
    let nominal = json!(recovery_nominal(part, face));
    let surface_use = match effective.surface_use(face, true) {
        Err(r) => json!(r.reason.python_value()),
        Ok(u) => {
            let side = u.material_side.as_ref().unwrap();
            let o = side.outward;
            let outward = match u.surface.kind() {
                SurfaceKind::Plane => {
                    json!([0, 1, 2].map(|j| (0..3).map(|i| r[i][j] * o[i]).sum::<f64>()))
                }
                _ => json!(side.candidate_outward_sign),
            };
            json!([outward, side.probe_distance])
        }
    };
    [fact, nominal, surface_use]
}

#[test]
fn effective_surfaces_are_placement_independent() {
    let Some(dir) = corpus() else { return };
    let names: Vec<String> = common::load("corpus.json")["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["file"].as_str().unwrap().to_string())
        .collect();
    // Every part unmoved and under each motion, all spread over every core at once (so a costly
    // part's readings run side by side); compared part by part, motion by motion.
    let readings: Vec<(usize, Option<&Motion>)> = (0..names.len())
        .flat_map(|i| {
            std::iter::once(None)
                .chain(MOTIONS.iter().map(Some))
                .map(move |m| (i, m))
        })
        .collect();
    let mut read_all = common::parallel::map(&readings, |&(i, motion)| {
        let path = dir.join(&names[i]);
        let (part, r) = match motion {
            None => (read_step_file(&path).unwrap(), IDENTITY),
            Some((_, r, t)) => {
                let placement: Placement = [0, 1, 2].map(|i| [r[i][0], r[i][1], r[i][2], t[i]]);
                (read_step_file_placed(&path, &placement).unwrap(), *r)
            }
        };
        let ctx = Context::new(&part);
        let effective = EffectiveFaces::new(&ctx);
        (0..part.faces.len())
            .map(|f| placement_free(&effective, &part, f, &r))
            .collect::<Vec<[Value; 3]>>()
    })
    .into_iter();
    let results: Vec<Vec<Difference>> = names
        .iter()
        .map(|name| {
            let want = read_all.next().unwrap();
            let mut out = Vec::new();
            for (motion, _, _) in MOTIONS {
                let moved = read_all.next().unwrap();
                for (face, want) in want.iter().enumerate() {
                    let got = &moved[face];
                    for (i, field) in FIELDS[..3].iter().enumerate() {
                        if !common::same(&got[i], &want[i]) {
                            out.push((
                                (name.clone(), format!("{motion}:{field}")),
                                face,
                                format!("moved {}, unmoved {}", got[i], want[i]),
                            ));
                        }
                    }
                }
            }
            out
        })
        .collect();
    let found: Vec<Difference> = results.into_iter().flatten().collect();
    let files: BTreeSet<String> = names.into_iter().collect();
    eprintln!("effective surfaces under motion: {} differ", found.len());
    check_known(found, &|f| f.contains(':'), &files);
}

/// The controlling length from OCCT's measured area and physical boundary length, with the
/// original 1e-6 band and exact, evidence-backed face exceptions. Adaptive area and precisely
/// integrated perimeter stay beside the fixed-rule measurements as independent evidence.
#[test]
fn recovery_lengths_match_opencascade_measurements() {
    let captured = load_gz("faces.json.gz");
    let corpus = corpus();
    let runs = runs(&captured);
    let files = runs
        .iter()
        .filter(|r| path(r, &corpus).is_some())
        .map(|r| r.file.to_owned())
        .collect();
    let found = common::parallel::map(&runs, |run| {
        let Some(path) = path(run, &corpus) else {
            return Vec::new();
        };
        let part = read_step_file(&path).unwrap();
        run.faces
            .iter()
            .filter_map(|f| {
                let face = f["face"].as_u64().unwrap() as usize;
                let area = f["area"].as_f64().unwrap();
                let perimeter = f["perimeter"].as_f64().unwrap();
                let expected = if area > 0.0
                    && area.is_finite()
                    && perimeter >= 0.0
                    && perimeter.is_finite()
                {
                    Some(if perimeter > 0.0 {
                        area.sqrt().min(2.0 * area / perimeter)
                    } else {
                        area.sqrt()
                    })
                } else {
                    None
                };
                let got = recovery_nominal(&part, face);
                (!common::same(&json!(got), &json!(expected))).then(|| {
                    (
                        (run.file.to_owned(), "nominal".into()),
                        face,
                        format!("Rust {got:?}; OCCT measurements {f}"),
                    )
                })
            })
            .collect::<Vec<_>>()
    })
    .into_iter()
    .flatten()
    .collect();
    check_reference(
        found,
        &|field| field == "nominal",
        &files,
        "references/effective_surface_measurements.json",
    );
}
