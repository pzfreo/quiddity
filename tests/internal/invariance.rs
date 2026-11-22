//! Accuracy, not parity: recognition must not depend on where the part sits. Every corpus part is
//! re-read under rigid motions that keep principal axes principal (a translation and right-angle
//! rotations), and every family must find the same features on the same faces. Python is not
//! consulted; differences it shares are still failures here. Explained exceptions live in
//! `tests/fixtures/references/invariance.json`, each with the most occurrences it may differ by
//! (`max_differing`), so a listed difference that grows still fails.

use super::common;
#[path = "../support/motion_breakdown.rs"]
mod motion_breakdown;
use motion_breakdown::Breakdown;
#[path = "../support/motion_sweep.rs"]
mod motion_sweep;
use super::slices;
use motion_sweep::{SWEEP_FAMILIES, compose, occurrences as sweep_occurrences, rotation};

use std::collections::BTreeMap;

use quiddity::Part;
use quiddity::correspondence::{self, Class, Correspondence, Fingerprints};
use quiddity::features::{self, levels};
use quiddity::frames;
use quiddity::kernel::step::{IDENTITY, Placement, read_step_file_placed};
use serde_json::Value;

/// A non-round translation (so rounding grids are not trivially aligned) and four rotations with
/// exact ±1/0 entries, so the placement itself adds no round-off. (A fifth, the axis cycle
/// without the translation, was dropped: `rot_zx_moved` is the same rotation moved, and every
/// difference listed under the cycle was listed under it too.)
const T: [f64; 3] = [123.456, -78.9, 41.3];
/// A named motion: rotation rows and translation.
type Motion = (&'static str, [[f64; 3]; 3], [f64; 3]);

const MOTIONS: [Motion; 5] = [
    (
        "translate",
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        T,
    ),
    (
        "rot_z90",
        [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
        [0.0; 3],
    ),
    (
        "rot_x90",
        [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]],
        [0.0; 3],
    ),
    (
        "rot_y180",
        [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]],
        [0.0; 3],
    ),
    (
        "rot_zx_moved",
        [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        T,
    ),
];

fn placement(r: &[[f64; 3]; 3], t: &[f64; 3]) -> Placement {
    [0, 1, 2].map(|i| [r[i][0], r[i][1], r[i][2], t[i]])
}

/// Each family's occurrences (`features::recognise`'s defining faces, under its field names, step
/// levels and risers among them), and face levels' faces, as a sorted list of sorted face sets:
/// what was found, and where, independent of the record's coordinates.
fn signature(part: &Part) -> BTreeMap<&'static str, Vec<Vec<usize>>> {
    signature_of(part, features::recognise(part))
}

/// [`signature`] from *features*, *part*'s recognition.
fn signature_of(
    part: &Part,
    features: features::Features,
) -> BTreeMap<&'static str, Vec<Vec<usize>>> {
    fn sorted(found: impl IntoIterator<Item = Vec<usize>>) -> Vec<Vec<usize>> {
        let mut out: Vec<Vec<usize>> = found
            .into_iter()
            .map(|mut f| {
                f.sort_unstable();
                f
            })
            .collect();
        out.sort();
        out
    }
    let mut out: BTreeMap<_, _> = features
        .defining
        .into_iter()
        .map(|(family, found)| (family, sorted(found)))
        .collect();
    let found = levels::face_levels_with_faces(part, &Default::default());
    out.insert("face_levels", sorted(found.into_iter().map(|(_, f)| f)));
    out
}

/// Families read along world Z by specification (`quiddity.levels`: the levels of a part's
/// horizontal planes, the step levels among them, and the risers between them), so compared only
/// under motions that keep the Z axis vertical (translate, rot_z90, rot_y180): under the others
/// the part's horizontal faces are other faces, in Python as here.
const WORLD_Z: [&str; 3] = ["face_levels", "risers", "step_levels"];

/// The listed exceptions for these motions whose file is *owned* (the slice's,
/// `tests/support/slices.rs`), keyed by (file, motion, family), with the most occurrences each
/// may differ by.
fn known_entries(
    motions: &[&str],
    owned: impl Fn(&str) -> bool,
) -> Vec<((String, String, String), usize)> {
    let known = common::load("references/invariance.json");
    let known = known.as_array().unwrap();
    common::check_verdicts("references/invariance.json", known);
    known
        .iter()
        .filter(|k| {
            motions.contains(&k["motion"].as_str().unwrap()) && owned(k["file"].as_str().unwrap())
        })
        .map(|k| {
            let s = |f: &str| k[f].as_str().unwrap().to_string();
            let most = k["max_differing"]
                .as_u64()
                .unwrap_or_else(|| panic!("an entry gives max_differing: {k}"));
            ((s("file"), s("motion"), s("family")), most as usize)
        })
        .collect()
}

fn corpus_files() -> Vec<String> {
    common::load("corpus.json")["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["file"].as_str().unwrap().to_string())
        .collect()
}

/// A family whose occurrences differ under a motion: how many differ, and which.
struct Difference {
    family: &'static str,
    count: usize,
    text: String,
}

/// Every corpus part under every motion: one test per slice of the corpus
/// (`tests/support/slices.rs`). Each reading is recognised once, and serves two checks: the
/// families find the same features on the same faces (`references/invariance.json`), and the moved part
/// corresponds with its unmoved self, every feature and face carried to itself
/// (`references/correspondence.json`'s invariance entries; `tests/correspondence.rs`).
fn invariant_under_rigid_motion(k: usize, n: usize) {
    let Some(dir) = common::corpus_dir() else {
        assert!(
            std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
            "QUIDDITY_CORPUS_REQUIRED is set but the corpus was not found"
        );
        eprintln!("corpus not found; set QUIDDITY_CORPUS");
        return;
    };
    let names: Vec<&str> = MOTIONS.iter().map(|m| m.0).collect();
    let all = corpus_files();
    let known = known_entries(&names, |file| slices::owns(&all, k, n, file));
    let files = slices::slice(&all, k, n);
    // Every part unmoved and under every motion, all spread over every core at once (so a costly
    // part's readings run side by side); the differences come back in corpus order, motion by
    // motion.
    let readings: Vec<(usize, Option<&Motion>)> = (0..files.len())
        .flat_map(|i| {
            std::iter::once(None)
                .chain(MOTIONS.iter().map(Some))
                .map(move |m| (i, m))
        })
        .collect();
    let mut read = common::parallel::map(&readings, |&(i, motion)| {
        let at = motion.map_or(IDENTITY, |(_, r, t)| placement(r, t));
        // (The error as text: the kernel's error is not `Send`.)
        read_step_file_placed(&dir.join(&files[i]), &at)
            .map(|p| {
                let found = features::recognise(&p);
                let prints = correspondence::fingerprint(&p, &found);
                (p.faces.len(), signature_of(&p, found), prints)
            })
            .map_err(|e| e.to_string())
    })
    .into_iter();
    let mut found = Vec::new();
    let mut breakdown_problems = Vec::new();
    let (mut unmoved_prints, mut moved_prints) = (Vec::new(), Vec::new());
    for name in &files {
        let unmoved = read.next().unwrap();
        let moved: Vec<_> = read.by_ref().take(MOTIONS.len()).collect();
        // The corpus test reports read failures.
        let Ok((faces, expected, prints)) = unmoved else {
            unmoved_prints.push(None);
            moved_prints.push(Vec::new());
            continue;
        };
        let mut moved_prints_here = Vec::new();
        for ((motion, r, _), moved) in MOTIONS.iter().zip(moved) {
            let (moved_faces, got, got_prints) = moved.unwrap();
            assert_eq!(moved_faces, faces, "{name} {motion}");
            breakdown_problems.extend(check_breakdown(
                "corpus",
                name,
                motion,
                &Breakdown::compare(
                    &physical_occurrences(&expected),
                    &physical_occurrences(&got),
                ),
            ));
            let d = differences(&expected, &got, |family| {
                WORLD_Z.contains(&family) && r[2][2].abs() != 1.0
            });
            found.push((name, *motion, d));
            moved_prints_here.push(got_prints);
        }
        unmoved_prints.push(Some(prints));
        moved_prints.push(moved_prints_here);
    }
    let mut problems = unexplained(&known, found.into_iter());
    problems.extend(breakdown_problems);
    problems.extend(correspondence_problems(
        &all,
        k,
        n,
        &files,
        &unmoved_prints,
        &moved_prints,
    ));
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

sliced!(
    recognition_is_invariant_under_rigid_motion,
    super::invariant_under_rigid_motion,
    [
        0 => slice_0,
        1 => slice_1,
        2 => slice_2,
        3 => slice_3,
        4 => slice_4,
        5 => slice_5,
        6 => slice_6,
        7 => slice_7,
    ]
);

/// The families of *got* whose occurrences are not *expected*'s, except those *skip* names.
fn differences(
    expected: &BTreeMap<&'static str, Vec<Vec<usize>>>,
    got: &BTreeMap<&'static str, Vec<Vec<usize>>>,
    skip: impl Fn(&str) -> bool,
) -> Vec<Difference> {
    let mut out = Vec::new();
    for (&family, got) in got {
        if skip(family) {
            continue;
        }
        if *got != expected[family] {
            let differing = symmetric_difference(&expected[family], got);
            out.push(Difference {
                family,
                count: differing.len(),
                text: format!(
                    "{} occurrences, {} unmoved; differing: {differing:?}",
                    got.len(),
                    expected[family].len(),
                ),
            });
        }
    }
    out
}

/// The differences not explained by a listed exception, a listed exception exceeded, and every
/// listed exception no longer seen.
fn unexplained<'a>(
    known: &[((String, String, String), usize)],
    found: impl Iterator<Item = (&'a String, &'a str, Vec<Difference>)>,
) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen = Vec::new();
    for (name, motion, differences) in found {
        for d in differences {
            let key = (name.clone(), motion.to_string(), d.family.to_string());
            match known.iter().find(|k| k.0 == key) {
                Some((_, most)) => {
                    if d.count > *most {
                        problems.push(format!(
                            "{name} {motion} {} is listed with at most {most} differing, now {}: {}",
                            d.family, d.count, d.text
                        ));
                    }
                    seen.push(key);
                }
                None => problems.push(format!(
                    "{name} {motion} {}: {} differing, {}",
                    d.family, d.count, d.text
                )),
            }
        }
    }
    for (key, _) in known {
        if !seen.contains(key) {
            problems.push(format!("{key:?} is listed but now invariant: remove it"));
        }
    }
    problems
}

/// Review M8's generic rotation (37° about (1, 2, 3), then the translation `T`), under its name in
/// `references/invariance.json`. No principal axis stays principal, so caller-space recognition (which
/// reads world axes) is not expected to survive it; recognition in the part's own frame
/// (`quiddity::frames`) is.
const GENERIC: &str = "generic_framed";

/// Every corpus part recognised in its own frame (`frames::prepare_framed`), unmoved and under
/// the generic rotation: each family, face levels, step levels and risers included (in the frame
/// they are read along the frame's z), must find the same features on the same faces. Where a
/// part's frame is not full the two working frames may differ by the gauge's freedom (a sign or
/// interchange, a roll about the axis), and the listed exceptions say so.
///
/// One test per existing slice of the corpus.
fn framed_invariant_under_a_generic_rotation(k: usize, n: usize) {
    let Some(dir) = common::corpus_dir() else {
        assert!(
            std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
            "QUIDDITY_CORPUS_REQUIRED is set but the corpus was not found"
        );
        eprintln!("corpus not found; set QUIDDITY_CORPUS");
        return;
    };
    let all = corpus_files();
    let known = known_entries(&[GENERIC], |file| slices::owns(&all, k, n, file));
    let files = slices::slice(&all, k, n);
    let generic = rotation([1.0, 2.0, 3.0], 37.0, T);
    let readings: Vec<(usize, &Placement)> = (0..files.len())
        .flat_map(|i| [&IDENTITY, &generic].map(|at| (i, at)))
        .collect();
    let mut read = common::parallel::map(&readings, |&(i, at)| {
        frames::prepare_framed_file(&dir.join(&files[i]), at)
            .map(|f| (f.frame.gauge, signature(&f.part)))
            .map_err(|e| e.to_string())
    })
    .into_iter();
    let mut breakdown_problems = Vec::new();
    let found: Vec<_> = files
        .iter()
        .map(|name| {
            let mut next = || read.next().unwrap();
            // In the part's frame. A refused frame (no material, an unmeasured face) is a
            // difference only when the two placements disagree on it.
            match (next(), next()) {
                (Ok((_, unmoved)), Ok((_, moved))) => {
                    breakdown_problems.extend(check_breakdown(
                        "framed",
                        name,
                        GENERIC,
                        &Breakdown::compare(
                            &physical_occurrences(&unmoved),
                            &physical_occurrences(&moved),
                        ),
                    ));
                    differences(&unmoved, &moved, |_| false)
                }
                (Err(a), Err(b)) if a == b => Vec::new(),
                (a, b) => vec![Difference {
                    family: "frame",
                    count: 1,
                    text: format!("unmoved {:?}, moved {:?}", a.map(|f| f.0), b.map(|f| f.0)),
                }],
            }
        })
        .collect();
    let framed = files.iter().zip(found).map(|(name, d)| (name, GENERIC, d));
    let mut problems = unexplained(&known, framed);
    problems.extend(breakdown_problems);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

sliced!(
    framed_recognition_is_invariant_under_a_generic_rotation,
    super::framed_invariant_under_a_generic_rotation,
    [
        0 => slice_0,
        1 => slice_1,
        2 => slice_2,
        3 => slice_3,
        4 => slice_4,
        5 => slice_5,
        6 => slice_6,
        7 => slice_7,
    ]
);

fn symmetric_difference(a: &[Vec<usize>], b: &[Vec<usize>]) -> Vec<String> {
    let only = |x: &[Vec<usize>], y: &[Vec<usize>], tag: &str| {
        x.iter()
            .filter(|f| !y.contains(f))
            .map(|f| format!("{tag}{f:?}"))
            .collect::<Vec<_>>()
    };
    let mut out = only(a, b, "-");
    out.extend(only(b, a, "+"));
    out
}

/// Repeating radial profiles, which no corpus part has: every captured part Python finds one on,
/// under each motion and, in its own frame, under the generic rotation, must give the same
/// opposed faces with the same repeat and edge counts.
#[test]
fn repeating_radial_profiles_are_invariant_on_captured_parts() {
    let calls = common::load("captured/calls.json");
    let files: Vec<&str> = calls["motion_inputs"]["repeating_radial_profiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();
    assert_eq!(
        files.len(),
        15,
        "the original radial motion input set must stay complete"
    );
    let read = |part: &Part| {
        let found = features::recognise(part);
        let counts = found
            .repeating_radial_profiles
            .iter()
            .map(|r| (r.repeat_count, r.edge_count));
        let mut both: Vec<(Vec<usize>, (usize, usize))> =
            found.defining["repeating_radial_profiles"]
                .iter()
                .map(|f| {
                    let mut f = f.clone();
                    f.sort_unstable();
                    f
                })
                .zip(counts)
                .collect();
        both.sort();
        both
    };
    let generic = rotation([1.0, 2.0, 3.0], 37.0, T);
    for name in files {
        let path = common::fixtures().join("captured").join(name);
        let part = read_step_file_placed(&path, &IDENTITY).unwrap();
        let unmoved = read(&part);
        assert!(!unmoved.is_empty(), "{name}: no profile found");
        // The evidence path publishes them on a solid and refuses an open shell, as Python's
        // writer does (`test_open_shell_and_malformed_sampling_fail_closed`).
        let verified =
            features::repeating_profiles::discover_verified(&features::Context::new(&part));
        if part.solids.is_empty() {
            assert_eq!(
                verified,
                Err(features::EvidenceError::NoValidSolid),
                "{name}"
            );
        } else {
            assert_eq!(verified.map(|f| f.len()), Ok(unmoved.len()), "{name}");
        }
        for (motion, r, t) in &MOTIONS {
            let moved = read(&read_step_file_placed(&path, &placement(r, t)).unwrap());
            assert_eq!(moved, unmoved, "{name} {motion}");
        }
        // A refused frame (the open shell has no material) must be refused both ways.
        let framed = |p: &Placement| {
            frames::prepare_framed_file(&path, p)
                .map(|f| read(&f.part))
                .map_err(|e| e.to_string())
        };
        let unmoved = framed(&IDENTITY);
        assert!(
            unmoved.as_ref().map_or(true, |f| !f.is_empty()),
            "{name}: none framed"
        );
        assert_eq!(framed(&generic), unmoved, "{name} {GENERIC}");
    }
}

/// Correspondence's families read along world Z (`quiddity.levels`): [`WORLD_Z`] less face levels,
/// which are not a correspondence family.
const WORLD_Z_FEATURES: [&str; 2] = ["risers", "step_levels"];

fn faces_of<'a>(f: &'a Fingerprints, id: &str) -> &'a [usize] {
    &f.features.iter().find(|x| x.id == id).unwrap().faces
}

/// What a correspondence of a part with itself moved gets wrong: the old features not carried
/// to themselves (and new features, as `new:<id>`), the faces not carried to themselves, and a
/// description. Families whose recognition is listed as not invariant (`references/invariance.json`)
/// are left out, with the patterns made of them.
struct Problems {
    features: Vec<String>,
    faces: usize,
    text: Vec<String>,
}

fn invariance_problems(
    old: &Fingerprints,
    new: &Fingerprints,
    c: &Correspondence,
    skip: &[String],
) -> Problems {
    let skipped = |id: &str| {
        let family = id.split('/').next().unwrap();
        skip.iter().any(|s| {
            family == s
                || (s == "holes" && family == "hole_patterns")
                || (s == "slots" && family == "slot_patterns")
                || (s == "pockets" && family == "pocket_patterns")
                || (s == "oriented_slots" && family == "oriented_slot_patterns")
        })
    };
    let mut out = Problems {
        features: Vec::new(),
        faces: 0,
        text: Vec::new(),
    };
    for e in &c.features {
        if skipped(&e.old) {
            continue;
        }
        let ok = e.class == Class::Carried
            && e.new
                .as_deref()
                .is_some_and(|n| faces_of(new, n) == faces_of(old, &e.old));
        if !ok {
            out.features.push(e.old.clone());
            out.text.push(format!(
                "feature {} {:?} → {:?} {:?} {:?}",
                e.old, e.class, e.new, e.candidates, e.changes
            ));
        }
    }
    for id in &c.new_features {
        if !skipped(id) {
            out.features.push(format!("new:{id}"));
            out.text.push(format!("feature {id} new"));
        }
    }
    let faces: Vec<String> = c
        .faces
        .iter()
        .filter(|e| !(e.class == Class::Carried && e.new == Some(e.old)))
        .map(|e| format!("face {} {:?} → {:?} {:?}", e.old, e.class, e.new, e.changes))
        .collect();
    out.faces = faces.len();
    if !faces.is_empty() {
        out.text.push(format!(
            "{} faces not carried to themselves, e.g. {}",
            faces.len(),
            faces[0]
        ));
    }
    out
}

/// A listed invariance case: (file, motion), and the features and faces that differ.
type Case = (String, String);
type Listed = (Vec<String>, Faces);

/// How many faces a listed case leaves not carried to themselves. `faces` pins the count exactly
/// (the default). `faces_at_most` bounds it, for a case whose count depends on a float threshold:
/// a face is not carried when its kernel area moves with the placement by more than the freeform
/// tolerance, and whether a wrong area lands just over or just under it depends on the platform's
/// libm (cgb207 under `rot_zx_moved`: 2 faces on macOS, 1 on Linux). The features stay pinned
/// exactly either way. Linux (CI) is the reference platform for these pins: a bound is at least
/// the count CI gives, and no lower than the count seen on any other platform.
#[derive(Clone, Copy, PartialEq)]
enum Faces {
    Exactly(usize),
    AtMost(usize),
}

impl Faces {
    fn read(k: &Value) -> Option<Faces> {
        match (k["faces"].as_u64(), k["faces_at_most"].as_u64()) {
            (Some(n), None) => Some(Faces::Exactly(n as usize)),
            (None, Some(n)) => Some(Faces::AtMost(n as usize)),
            _ => None,
        }
    }

    fn admits(self, n: usize) -> bool {
        match self {
            Faces::Exactly(m) => n == m,
            Faces::AtMost(m) => n <= m,
        }
    }
}

#[test]
fn listed_face_counts_are_pinned_or_bounded() {
    let exact = Faces::read(&serde_json::json!({"faces": 2})).unwrap();
    let bound = Faces::read(&serde_json::json!({"faces_at_most": 2})).unwrap();
    assert!(exact.admits(2) && !exact.admits(1) && !exact.admits(3));
    assert!(bound.admits(2) && bound.admits(1) && !bound.admits(3));
    assert!(Faces::read(&serde_json::json!({"faces": 2, "faces_at_most": 2})).is_none());
    assert!(Faces::read(&serde_json::json!({})).is_none());
}

impl std::fmt::Display for Faces {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Faces::Exactly(n) => write!(f, "{n} faces"),
            Faces::AtMost(n) => write!(f, "at most {n} faces"),
        }
    }
}

/// One part moved: what its correspondence with the unmoved part gets wrong, its alignment, and
/// how much there was to carry.
struct Moved {
    found: Problems,
    alignment: String,
    features: usize,
    faces: usize,
}

fn known_correspondence() -> Vec<Value> {
    let known = common::load("references/correspondence.json");
    let known = known.as_array().unwrap().clone();
    common::check_verdicts("references/correspondence.json", &known);
    known
}

/// What correspondence gets wrong between each part of slice *k* of *n* (`files`) and itself
/// under each motion, from the readings' fingerprints (`unmoved[i]`, `None` where the part did
/// not read; `moved[i][m]` under `MOTIONS[m]`): every unlisted difference, every listed case
/// that differs otherwise or no longer differs, and every listed case not checked.
fn correspondence_problems(
    all: &[String],
    k: usize,
    n: usize,
    files: &[String],
    unmoved: &[Option<Fingerprints>],
    moved: &[Vec<Fingerprints>],
) -> Vec<String> {
    // Each listed case pins exactly which features and how many faces differ (or, for a
    // threshold-dependent count, at most how many), so any other difference in it still fails.
    let known: Vec<(Case, Listed)> = known_correspondence()
        .iter()
        .filter(|e| {
            e["test"] == "invariance" && slices::owns(all, k, n, e["file"].as_str().unwrap())
        })
        .map(|k| {
            let mut features: Vec<String> = k["features"]
                .as_array()
                .expect("an invariance entry lists its features")
                .iter()
                .map(|x| x.as_str().unwrap().to_string())
                .collect();
            features.sort();
            (
                (
                    k["file"].as_str().unwrap().to_string(),
                    k["motion"].as_str().unwrap().to_string(),
                ),
                (
                    features,
                    Faces::read(k).unwrap_or_else(|| {
                        panic!("an invariance entry gives one of faces and faces_at_most: {k}")
                    }),
                ),
            )
        })
        .collect();
    let invariance = common::load("references/invariance.json");
    let cases: Vec<(usize, usize, &Motion)> = (0..files.len())
        .filter(|&i| unmoved[i].is_some()) // the corpus test reports read failures
        .flat_map(|i| {
            MOTIONS
                .iter()
                .enumerate()
                .map(move |(m, motion)| (i, m, motion))
        })
        .collect();
    let checked = common::parallel::map(&cases, |&(i, m, (motion, r, _))| {
        let name = &files[i];
        let old = unmoved[i].as_ref().unwrap();
        let new = &moved[i][m];
        let c = correspondence::correspond(old, new).unwrap();
        // For #127, the six new R40 inside web fillets participate in the spool's
        // existing six-fold ambiguity. Preserve the exact 72-face bound below and
        // require these new records to be carried coherently with that face map.
        if name == "cadgenbench/flanged_spool_132.step"
            && matches!(*motion, "rot_z90" | "rot_zx_moved")
        {
            assert!(c.alignment.symmetric);
            assert_eq!(c.alignment.fold, Some(6));
            let rounds: Vec<_> = old
                .features
                .iter()
                .filter(|f| {
                    f.family == "fillets" && f.traits.get("side").is_some_and(|s| s == "concave")
                })
                .collect();
            // For #134, the two turned R10 root rounds now correctly report
            // concave too. Keep the six R40 web rounds and the exact face pins.
            assert_eq!(rounds.len(), 8);
            assert_eq!(
                rounds.iter().filter(|f| f.sizes["radius"] == 40.).count(),
                6
            );
            assert_eq!(
                rounds
                    .iter()
                    .filter(|f| f.sizes["radius"] == 10.
                        && f.traits.get("turned").is_some_and(|v| v == "true"))
                    .count(),
                2
            );
            // For #127 after #128, only the six convex R10 profiles remain residual.
            // They share this exact symmetry, without relaxing main's fillet or face pins.
            let profiles: Vec<_> = old
                .features
                .iter()
                .filter(|f| f.family == "cylindrical_profiles")
                .collect();
            assert_eq!(profiles.len(), 6);
            for profile in &profiles {
                assert_eq!(profile.sizes["radius"], 10.);
                assert_eq!(profile.traits["side"], "convex");
            }
            // The six derived R40 chains follow the same permutation as their fillet faces.
            let chains: Vec<_> = old
                .features
                .iter()
                .filter(|f| f.family == "fillet_chains" && f.sizes["radius"] == 40.)
                .collect();
            assert_eq!(chains.len(), 6);
            for round in rounds.into_iter().chain(profiles).chain(chains) {
                let carried = c.features.iter().find(|e| e.old == round.id).unwrap();
                assert_eq!(carried.class, Class::Carried);
                assert!(carried.changes.is_empty());
                let target = new
                    .features
                    .iter()
                    .find(|f| Some(&f.id) == carried.new.as_ref())
                    .unwrap();
                assert_eq!(target.traits, round.traits);
                if matches!(round.family.as_str(), "fillets" | "fillet_chains") {
                    assert_eq!(target.sizes, round.sizes);
                } else {
                    // Profile sweeps use the unchanged correspondence size comparison
                    // above; radius and the complete face permutation remain exact.
                    assert_eq!(target.sizes["radius"], round.sizes["radius"]);
                }
                let mut mapped: Vec<_> = round
                    .faces
                    .iter()
                    .map(|face| {
                        let entry = c.faces.iter().find(|e| e.old == *face).unwrap();
                        assert_eq!(entry.class, Class::Carried);
                        entry.new.unwrap()
                    })
                    .collect();
                mapped.sort_unstable();
                assert_eq!(mapped, target.faces);
            }
        }
        let mut skip: Vec<String> = invariance
            .as_array()
            .unwrap()
            .iter()
            .filter(|k| k["file"] == name.as_str() && k["motion"] == *motion)
            .map(|k| k["family"].as_str().unwrap().to_string())
            .collect();
        // Read along world Z by specification: where the motion does not keep Z vertical, the
        // moved part's levels and risers are other faces.
        if r[2][2].abs() != 1.0 {
            skip.extend(WORLD_Z_FEATURES.map(String::from));
        }
        Moved {
            found: invariance_problems(old, new, &c, &skip),
            alignment: format!(
                "aligned {}, symmetric {}, fold {:?}",
                c.alignment.found, c.alignment.symmetric, c.alignment.fold
            ),
            features: old.features.len(),
            faces: old.faces.len(),
        }
    });
    let mut problems = Vec::new();
    let mut seen = Vec::new();
    let (mut pairs, mut features, mut faces) = (0, 0, 0);
    for (&(i, _, (motion, _, _)), m) in cases.iter().zip(checked) {
        let name = &files[i];
        let found = m.found;
        pairs += 1;
        features += m.features;
        faces += m.faces;
        let key = (name.to_string(), motion.to_string());
        let mut got = found.features.clone();
        got.sort();
        if let Some((_, listed)) = known.iter().find(|k| k.0 == key) {
            if found.text.is_empty() {
                problems.push(format!("{key:?} is listed but now carried: remove it"));
            } else if got != listed.0 || !listed.1.admits(found.faces) {
                problems.push(format!(
                    "{key:?} is listed with features {:?} and {}, but now {got:?} and {} faces: {}",
                    listed.0,
                    listed.1,
                    found.faces,
                    found.text.join("; ")
                ));
            }
            seen.push(key);
            continue;
        }
        if !found.text.is_empty() {
            problems.push(format!(
                "{name} {motion} ({}): {} | features {got:?}, faces {}",
                m.alignment,
                found.text.join("; "),
                found.faces
            ));
        }
    }
    for (key, _) in &known {
        if !seen.contains(key) {
            problems.push(format!("{key:?} is listed but was not checked"));
        }
    }
    eprintln!("{pairs} moved parts corresponded: {features} features, {faces} faces");
    problems
}

/// The analytic 0.4999 mm face-band fixture has a non-grid published thickness (8.4999, its
/// bands' outer planes): rounding its absolute ends before taking thickness would change the
/// dimension under this sub-micron-grid shift.
#[test]
fn plate_thickness_survives_a_fractional_publication_grid_translation() {
    let path = common::fixtures().join("captured/6d359acf631cc258.step.gz");
    let opts = features::plates::PlateOptions {
        max_thick_frac: 1.1,
        ..Default::default()
    };
    let read = |at: &Placement| {
        let part = read_step_file_placed(&path, at).unwrap();
        features::plates::discover(&features::Context::new(&part), &opts)
            .into_iter()
            .map(|o| {
                let mut faces = o.defining;
                faces.sort_unstable();
                (faces, o.record.thickness())
            })
            .collect::<BTreeMap<_, _>>()
    };
    let before = read(&IDENTITY);
    assert!(!before.is_empty());
    assert!(
        before
            .values()
            .any(|t| (t - (t * 1000.0).round() / 1000.0).abs() > 1e-7)
    );
    let at = [
        [1.0, 0.0, 0.0, 0.00051],
        [0.0, 1.0, 0.0, 0.00051],
        [0.0, 0.0, 1.0, 0.00051],
    ];
    let after = read(&at);
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    for (faces, thickness) in before {
        assert!((thickness - after[&faces]).abs() < 1e-12);
    }
}

/// The reference census excludes world-Z construction levels and risers. Keep their existing
/// comparison above, but never match them against physical features as alleged reclassifications.
/// Fillet aggregates also have their own comparison and reuse existing physical occurrences.
fn physical_occurrences(
    signature: &BTreeMap<&str, Vec<Vec<usize>>>,
) -> Vec<motion_breakdown::Occurrence> {
    motion_breakdown::occurrences(signature)
        .into_iter()
        .filter(|o| {
            !WORLD_Z.contains(&o.family.as_str())
                && !quiddity::features::is_fillet_aggregate(&o.family)
        })
        .collect()
}

/// Regression limits captured at 5fb1706. Missing entries mean zero; improvements are allowed.
fn check_breakdown(suite: &str, file: &str, motion: &str, got: &Breakdown) -> Vec<String> {
    eprintln!(
        "BREAKDOWN {}",
        serde_json::json!({"suite":suite,"file":file,"motion":motion,"counts":got})
    );
    let limits = common::load("motion_breakdown_limits.json");
    let limit: Breakdown = limits
        .get(suite)
        .and_then(|v| v.get(file))
        .and_then(|v| v.get(motion))
        .map(|v| serde_json::from_value(v.clone()).expect("valid breakdown regression limit"))
        .unwrap_or_default();
    let problems = got.regressions(&limit);
    if problems.is_empty() {
        Vec::new()
    } else {
        vec![format!(
            "{suite} {file} {motion}: {}; per-family breakdown {got:?}",
            problems.join("; ")
        )]
    }
}

fn golden_paths() -> Vec<std::path::PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(common::fixtures())
        .unwrap()
        .map(|p| p.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("golden_")
                && p.extension().is_some_and(|e| e == "step")
        })
        .collect();
    paths.sort();
    assert_eq!(paths.len(), 34);
    paths
}

/// The 34 pinned golden STEP parts are the Python sweep's fixtures exported at the corpus pin.
/// Raw inventory is intentional: this measures frame coupling, independently of the framed test.
#[test]
fn golden_sweep_matches_python_motion_definitions() {
    let paths = golden_paths();
    let motions = [
        ("Z30", rotation([0.0, 0.0, 1.0], 30.0, [0.0; 3])),
        ("X30", rotation([1.0, 0.0, 0.0], 30.0, [0.0; 3])),
        ("X90", rotation([1.0, 0.0, 0.0], 90.0, [0.0; 3])),
        ("translate", placement(&MOTIONS[0].1, &T)),
    ];
    let results = common::parallel::map(&paths, |path| {
        let part = read_step_file_placed(path, &IDENTITY).unwrap();
        let baseline = sweep_occurrences(&part);
        motions
            .iter()
            .map(|(name, at)| {
                let moved = read_step_file_placed(path, at).unwrap();
                assert_eq!(part.faces.len(), moved.faces.len());
                for i in 0..part.faces.len() {
                    assert_eq!(part.faces[i].surface.kind(), moved.faces[i].surface.kind());
                    assert_eq!(part.face_edges(i).len(), moved.face_edges(i).len());
                    let a = part.face_moments(i).unwrap();
                    let b = moved.face_moments(i).unwrap();
                    assert!(
                        (a.area - b.area).abs() < 1e-5,
                        "{} {name} face {i} area",
                        path.display()
                    );
                    let restored = [0, 1, 2].map(|d| {
                        (0..3)
                            .map(|k| at[k][d] * (b.centroid[k] - at[k][3]))
                            .sum::<f64>()
                    });
                    assert!(
                        quiddity::kernel::py::dist(&a.centroid, &restored) < 1e-5,
                        "{} {name} face {i} correspondence",
                        path.display()
                    );
                }
                (
                    *name,
                    Breakdown::compare(&baseline, &sweep_occurrences(&moved)),
                )
            })
            .collect::<Vec<_>>()
    });
    let mut totals: BTreeMap<&str, Breakdown> = BTreeMap::new();
    let mut problems = Vec::new();
    for (path, results) in paths.iter().zip(results) {
        let file = path.file_name().unwrap().to_str().unwrap();
        for (motion, counts) in results {
            problems.extend(check_breakdown("golden", file, motion, &counts));
            totals.entry(motion).or_default().add(&counts);
        }
    }
    for (motion, counts) in totals {
        eprintln!("TOTAL {motion} {}", serde_json::to_string(&counts).unwrap());
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Rolls of the transverse support planes on each possible principal run axis.
fn principal_edge_rolls() -> Vec<Placement> {
    [
        IDENTITY,
        placement(&MOTIONS[2].1, &[0.; 3]),
        placement(&MOTIONS[4].1, &[0.; 3]),
    ]
    .into_iter()
    .flat_map(|carrier| {
        [-30., -17., 17., 30.].map(|angle| {
            let roll = rotation([0., 0., 1.], angle, [0.; 3]);
            [0, 1, 2].map(|i| {
                [0, 1, 2, 3].map(|j| {
                    if j == 3 {
                        T[i]
                    } else {
                        (0..3).map(|k| carrier[i][k] * roll[k][j]).sum()
                    }
                })
            })
        })
    })
    .collect()
}

/// Rolling the supporting walls keeps the same chamfers, legs, anchors and virtual corners.
#[test]
fn principal_chamfers_follow_rolled_supports() {
    use features::{Context, chamfers};
    let path = common::fixtures().join("golden_chamfers_fillets_and_flats.step");
    let read = |at: &Placement| {
        let part = read_step_file_placed(&path, at).unwrap();
        chamfers::discover_verified(&Context::new(&part), &Default::default())
            .unwrap()
            .into_iter()
            .map(|o| (o.defining, o.record))
            .collect::<BTreeMap<_, _>>()
    };
    let baseline = read(&IDENTITY);
    assert_eq!(baseline.len(), 4);
    for at in principal_edge_rolls() {
        let moved = read(&at);
        assert_eq!(
            baseline.keys().collect::<Vec<_>>(),
            moved.keys().collect::<Vec<_>>(),
            "{at:?}"
        );
        let transform = |p: [f64; 3]| {
            [0, 1, 2].map(|i| at[i][3] + (0..3).map(|j| at[i][j] * p[j]).sum::<f64>())
        };
        for (faces, before) in &baseline {
            let after = &moved[faces];
            assert_eq!(
                (before.leg1, before.leg2, before.angle),
                (after.leg1, after.leg2, after.angle)
            );
            let (before_corner, after_corner) = (before.corner.unwrap(), after.corner.unwrap());
            assert!(
                quiddity::kernel::py::dist(&transform(*before_corner), &after_corner[..]) < 0.002
            );
            assert!(quiddity::kernel::py::dist(&transform(*before.at), &after.at[..]) < 0.002);
        }
    }
}

/// Rolling the tangent planes keeps the same fillets, radii and centreline anchors.
#[test]
fn principal_fillets_follow_rolled_supports() {
    use features::{Context, fillets};
    let path = common::fixtures().join("golden_chamfers_fillets_and_flats.step");
    let read = |at: &Placement| {
        let part = read_step_file_placed(&path, at).unwrap();
        fillets::discover_verified(&Context::new(&part), &Default::default())
            .unwrap()
            .into_iter()
            .map(|o| (o.defining, o.record))
            .collect::<BTreeMap<_, _>>()
    };
    let baseline = read(&IDENTITY);
    assert_eq!(baseline.len(), 4);
    for at in principal_edge_rolls() {
        let moved = read(&at);
        assert_eq!(
            baseline.keys().collect::<Vec<_>>(),
            moved.keys().collect::<Vec<_>>(),
            "{at:?}"
        );
        let transform = |p: [f64; 3]| {
            [0, 1, 2].map(|i| at[i][3] + (0..3).map(|j| at[i][j] * p[j]).sum::<f64>())
        };
        for (faces, before) in &baseline {
            let after = &moved[faces];
            assert_eq!(before.radius, after.radius);
            assert!(quiddity::kernel::py::dist(&transform(*before.at), &after.at[..]) < 0.002);
        }
    }
}
/// Exercise the supported loaded-part route directly, including record mapping and fingerprint
/// construction. The caller-space parity checks above retain their original semantics.
#[test]
fn golden_public_framed_route_survives_oblique_poses() {
    let z = rotation([0., 0., 1.], 17., [0.; 3]);
    let x = rotation([1., 0., 0.], 23., [0.; 3]);
    let compound = compose(x, z);
    let motions = [
        ("Z30", rotation([0., 0., 1.], 30., [0.; 3])),
        ("X30", rotation([1., 0., 0.], 30., [0.; 3])),
        ("Z17X23", compound),
    ];
    let paths = golden_paths();
    let results = common::parallel::map(&paths, |path| {
        let read = |at: &Placement| read_step_file_placed(path, at);
        let census = |at: &Placement| {
            let recognition = correspondence::recognise(&read(at).unwrap());
            assert!(matches!(
                recognition.frame,
                frames::RecordFrame::Inferred(_)
            ));
            SWEEP_FAMILIES
                .iter()
                .flat_map(|&(family, field)| {
                    recognition
                        .fingerprints
                        .features
                        .iter()
                        .filter(move |f| f.family == field)
                        .map(move |f| motion_breakdown::Occurrence {
                            family: family.into(),
                            faces: f.faces.clone(),
                        })
                })
                .collect::<Vec<_>>()
        };
        let baseline = census(&IDENTITY);
        motions
            .iter()
            .map(|(motion, at)| (*motion, Breakdown::compare(&baseline, &census(at))))
            .collect::<Vec<_>>()
    });
    let mut problems = Vec::new();
    let mut totals = BTreeMap::<_, Breakdown>::new();
    for (path, results) in paths.iter().zip(results) {
        for (motion, counts) in results {
            problems.extend(check_breakdown(
                "golden_public_framed",
                path.file_name().unwrap().to_str().unwrap(),
                motion,
                &counts,
            ));
            totals.entry(motion).or_default().add(&counts);
        }
    }
    for (motion, counts) in totals {
        assert!(counts.baseline >= 112);
        eprintln!(
            "PUBLIC FRAMED TOTAL {motion} {}",
            serde_json::to_string(&counts).unwrap()
        );
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// OpenCascade independently proves four R2 convex quarter cylinders with tangent supports
/// rolled 45 degrees about Z. The specific fillets replace only duplicate accepted blends.
#[test]
fn rolled_boss_rounds_are_specific_fillets() {
    let path = common::fixtures().join("captured/60275841a02f03b9.step.gz");
    let mut baseline = None;
    for angle in [0., -30., 17., 30.] {
        let part = read_step_file_placed(&path, &rotation([0., 0., 1.], angle, T)).unwrap();
        let inventory = features::inventory(&part).unwrap();
        let physical_blends = inventory.physical.defining["blends"].clone();
        let accepted = inventory.accepted();
        assert_eq!(physical_blends.len(), 4);
        assert_eq!(accepted.fillets.len(), 4);
        assert!(accepted.blends.is_empty());
        let mut faces = accepted.defining["fillets"].clone();
        faces.sort();
        let mut blends = physical_blends.clone();
        blends.sort();
        assert_eq!(faces, blends);
        assert!(
            accepted
                .fillets
                .iter()
                .all(|r| r.radius == 2. && r.axis == quiddity::FilletAxis::Principal('z'))
        );
        if let Some(expected) = &baseline {
            assert_eq!(&faces, expected);
        } else {
            baseline = Some(faces);
        }
    }
}

/// The circular corner cut's three section points already represent a roll of its side
/// planes. The cylinder run and terminal stay principal; side-plane axis letters are needless.
#[test]
fn circular_steps_follow_rolled_supports() {
    use features::{Context, circular_blind_steps};
    let path = common::fixtures().join("golden_circular_blind_step.step");
    let read = |at: &Placement| {
        let part = read_step_file_placed(&path, at).unwrap();
        circular_blind_steps::discover_verified(&Context::new(&part)).unwrap()
    };
    let baseline = read(&IDENTITY);
    assert_eq!(baseline.len(), 1);
    let before = &baseline[0].record;
    assert_eq!((before.radius, before.length), (4., 25.));
    for carrier in [
        IDENTITY,
        placement(&MOTIONS[4].1, &[0.; 3]),
        [[0., 1., 0., 0.], [0., 0., 1., 0.], [1., 0., 0., 0.]],
    ] {
        for angle in [-30., -17., 17., 30.] {
            let roll = rotation([1., 0., 0.], angle, [0.; 3]);
            let at = [0, 1, 2].map(|i| {
                [0, 1, 2, 3].map(|j| {
                    if j == 3 {
                        T[i]
                    } else {
                        (0..3).map(|k| carrier[i][k] * roll[k][j]).sum()
                    }
                })
            });
            let moved = read(&at);
            assert_eq!(moved.len(), 1, "{at:?}");
            assert_eq!(baseline[0].defining, moved[0].defining);
            let after = &moved[0].record;
            assert_eq!((before.radius, before.length), (after.radius, after.length));
            let axis = (0..3).find(|&i| at[i][0].abs() > 0.999).unwrap();
            assert_eq!(after.axis, ['x', 'y', 'z'][axis]);
            let transform = |p: [f64; 3]| {
                [0, 1, 2].map(|i| at[i][3] + (0..3).map(|j| at[i][j] * p[j]).sum::<f64>())
            };
            // Circular-step coordinates use six significant figures, not six decimals.
            for i in 0..2 {
                assert_eq!(
                    transform(*before.centreline[i]).map(quiddity::kernel::py::quantise6),
                    *after.centreline[i]
                );
            }
            let transverse: Vec<_> = (0..3).filter(|&i| i != axis).collect();
            for (index, p) in before.section.iter().enumerate() {
                let point = transform([before.centreline[0][0], p[0], p[1]]);
                let point = [point[transverse[0]], point[transverse[1]]]
                    .map(quiddity::kernel::py::quantise6);
                if index == 1 {
                    assert_eq!(point, *after.section[1]);
                } else {
                    assert!([0, 2].iter().any(|&j| point == *after.section[j]));
                }
            }
        }
    }
}

/// Recognition does not depend on the order of a compound's children (#73). `two_channels_ordered`
/// and `two_channels_reversed` hold the same two channelled blocks (z −6..24 and z 54..84) as two
/// occurrences of one solid, in opposite orders: `_reversed` is draftwright-rust's fixture, and
/// `_ordered` is it with the two occurrences' placements swapped. Both give one channel per body,
/// and every record is the same apart from body order: face ids swap halves (both bodies have the
/// same face count), so record ids, indices and bodies follow, and plate offsets may differ by
/// round-off. `two_solids_reversed` is `two_solids` with its two distinct solids listed in the
/// opposite order, and gives the same records in the same way.
/// `two_channels` is draftwright-rust's forward-order file. Its writer kept only the z 54..84
/// occurrence (one NAUO, one solid; OpenCascade's reader also finds one solid), so it has one
/// body and one channel: the 1 against 2 reported in #73 is the file, not recognition.
#[test]
fn compound_child_order_does_not_change_recognition() {
    let fixture = |name: &str| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let (output, _) = quiddity::serve::recognise_file(&path.to_string_lossy())
            .unwrap_or_else(|e| panic!("{name}: {}", e.message));
        let edge_count = quiddity::read_step_file(&path).unwrap().edges.len() as u64;
        (serde_json::to_value(&output).unwrap(), edge_count)
    };
    // `body_key` is `body_signature`'s bounds min (x, y, z), max (x, y, z), volume, area.
    let z_extent = |record: &Value| {
        let key = &record["parameters"]["body_key"];
        (key[2].as_f64().unwrap(), key[5].as_f64().unwrap())
    };
    // Records by family, without what follows body order, with faces mapped by `face`.
    fn records(
        document: &Value,
        face: impl Fn(u64) -> u64,
        edge: impl Fn(u64) -> u64,
    ) -> BTreeMap<String, Vec<Value>> {
        let records = document["records"].as_array().unwrap();
        let remap = |v: &Value, map: &dyn Fn(u64) -> u64| -> Vec<u64> {
            let mut ids = v
                .as_array()
                .unwrap()
                .iter()
                .map(|f| map(f.as_u64().unwrap()))
                .collect::<Vec<_>>();
            ids.sort_unstable();
            ids
        };
        let members = |family: &str, refs: &Value| -> Value {
            let by_index = records
                .iter()
                .filter(|r| r["family"] == family)
                .collect::<Vec<_>>();
            let mut members: Vec<_> = refs
                .as_array()
                .unwrap()
                .iter()
                .map(|i| remap(&by_index[i.as_u64().unwrap() as usize]["faces"], &face))
                .collect();
            members.sort_unstable();
            serde_json::json!(members)
        };
        let mut out: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for record in records {
            let family = record["family"].as_str().unwrap().to_owned();
            let mut record = record.clone();
            let fields = record.as_object_mut().unwrap();
            fields.remove("id");
            let faces = fields["faces"].as_array().unwrap();
            fields["faces"] = faces.iter().map(|f| face(f.as_u64().unwrap())).collect();
            let parameters = fields["parameters"].as_object_mut().unwrap();
            parameters.remove("index");
            parameters.remove("body");
            if matches!(
                family.as_str(),
                "fillets" | "fillet_chains" | "fillet_groups"
            ) {
                fields["faces"] = serde_json::json!(remap(&fields["faces"], &|f| f));
                let parameters = fields["parameters"].as_object_mut().unwrap();
                parameters.insert(
                    "faces".into(),
                    serde_json::json!(remap(&parameters["faces"], &face)),
                );
                if let Some(chains) = parameters.get_mut("chains") {
                    *chains = members("fillet_chains", chains);
                }
                if let Some(fillets) = parameters.get_mut("fillets") {
                    *fillets = members("fillets", fillets);
                }
                if let Some(blend_faces) = parameters.get_mut("blend_faces") {
                    *blend_faces = serde_json::json!(remap(blend_faces, &face));
                }
                if let Some(edges) = parameters.get_mut("edges") {
                    *edges = serde_json::json!(remap(edges, &edge));
                }
            }
            let parameters = fields["parameters"].as_object_mut().unwrap();
            if let Some(evidence) = parameters
                .get_mut("evidence")
                .and_then(Value::as_object_mut)
            {
                for faces in evidence.values_mut() {
                    let mapped = faces.as_array().unwrap().iter();
                    *faces = mapped.map(|f| face(f.as_u64().unwrap())).collect();
                }
            }
            out.entry(family).or_default().push(record);
        }
        for list in out.values_mut() {
            list.sort_by_key(|r| r["faces"].to_string());
        }
        out
    }
    fn same(a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => {
                (x.as_f64().unwrap() - y.as_f64().unwrap()).abs() < 1e-9
            }
            (Value::Array(x), Value::Array(y)) => {
                x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q))
            }
            (Value::Object(x), Value::Object(y)) => {
                x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
            }
            _ => a == b,
        }
    }

    // Every record is the same in both orders apart from body order. Each pair holds two bodies
    // of equal face count, so the face ids of one order are the other's with halves swapped.
    let same_records = |(forward, edges): &(Value, u64), (reversed, other_edges): &(Value, u64)| {
        let n = forward["document"]["face_count"].as_u64().unwrap();
        assert_eq!(edges, other_edges);
        assert_eq!(edges % 2, 0, "two bodies of equal edge count");
        assert_eq!(reversed["document"]["face_count"].as_u64(), Some(n));
        assert_eq!(n % 2, 0, "two bodies of equal face count");
        let (a, b) = (
            records(
                &forward["document"],
                |f| (f + n / 2) % n,
                |e| (e + edges / 2) % edges,
            ),
            records(&reversed["document"], |f| f, |e| e),
        );
        assert_eq!(
            a.keys().collect::<Vec<_>>(),
            b.keys().collect::<Vec<_>>(),
            "families"
        );
        for (family, list) in &a {
            assert_eq!(list.len(), b[family].len(), "{family}");
            for (x, y) in list.iter().zip(&b[family]) {
                assert!(same(x, y), "{family}:\n{x}\n{y}");
            }
        }
    };

    let ordered = fixture("two_channels_ordered.step");
    let reversed = fixture("two_channels_reversed.step");
    for (output, _) in [&ordered, &reversed] {
        assert_eq!(output["document"]["face_count"], 20);
        let mut channels: Vec<_> = output["channels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| (c["d_lo"].as_f64().unwrap(), c["d_hi"].as_f64().unwrap()))
            .collect();
        channels.sort_by(|p, q| p.partial_cmp(q).unwrap());
        assert_eq!(
            channels,
            [(6.0, 24.0), (66.0, 84.0)],
            "one channel per body"
        );
    }
    same_records(&ordered, &reversed);
    // Two distinct solids rather than two occurrences of one product.
    same_records(
        &fixture("two_solids.step"),
        &fixture("two_solids_reversed.step"),
    );
    // The channels' records name the body they lie on, in either order.
    for (output, _) in [&ordered, &reversed] {
        for record in output["document"]["records"].as_array().unwrap() {
            if record["family"] == "channels" {
                let (lo, hi) = z_extent(record);
                let depth = record["parameters"]["d_lo"].as_f64().unwrap();
                assert!(lo < depth && depth < hi, "{record}");
            }
        }
    }

    let (single, _) = fixture("two_channels.step");
    assert_eq!(single["document"]["face_count"], 10);
    let channels = single["channels"].as_array().unwrap();
    assert_eq!(channels.len(), 1);
    assert_eq!(
        (channels[0]["d_lo"].as_f64(), channels[0]["d_hi"].as_f64()),
        (Some(66.0), Some(84.0))
    );
}
