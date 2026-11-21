//! Step levels along every frame axis (#307 item 10). The fixtures are synthetic (build123d).
use std::collections::{BTreeMap, BTreeSet};

use super::common;
use quiddity::features::{Context, levels};

fn part(name: &str) -> quiddity::Part {
    quiddity::read_step_file(&common::fixtures().join(name)).unwrap()
}

/// Read in file coordinates (no frame), with no other family's claims: each block's top is a
/// level along z, each narrower block's +y side a level along y, the middle block's +x end a level
/// along x. A plane has one normal, so each face is in exactly one level; the riser of the z
/// steps (x = 50, the full width) stays a z riser and is no x level.
#[test]
fn steps_along_three_axes_each_face_in_one_level() {
    let p = part("multi_axis_steps.step.gz");
    let ctx = Context::new(&p);
    let z_levels = levels::discover_step_levels(&ctx);
    let z_risers = levels::discover_risers(&ctx, &z_levels);
    let taken: BTreeSet<usize> = z_levels
        .iter()
        .flat_map(|o| o.defining.iter().copied())
        .chain(z_risers.iter().flat_map(|o| o.defining.iter().copied()))
        .collect();
    let (other, risers) = levels::other_axes(&ctx, &BTreeSet::new(), &taken);
    let mut found: BTreeMap<(char, i64), Vec<usize>> = BTreeMap::new();
    for o in z_levels.iter().chain(&other) {
        found.insert(
            (o.record.height_axis, o.record.z.round() as i64),
            o.defining.clone(),
        );
    }
    let at = |f: usize, k: usize| p.face_bounds(f).min[k];
    let face = |k: usize, v: f64| {
        (0..p.faces.len())
            .filter(|&f| p.face_normal(f, 0., 0.).is_some_and(|n| n[k].abs() > 0.99))
            .filter(|&f| (at(f, k) - v).abs() < 1e-6)
            .collect::<Vec<_>>()
    };
    let want: BTreeMap<(char, i64), Vec<usize>> = [
        (('z', 10), face(2, 10.)),
        (('z', 20), face(2, 20.)),
        (('x', 100), face(0, 100.)),
        (('y', 20), face(1, 20.)),
        (('y', 40), face(1, 40.)),
    ]
    .into_iter()
    .collect();
    assert_eq!(found, want);
    let mut seen = BTreeSet::new();
    for faces in found.values() {
        for &f in faces {
            assert!(seen.insert(f), "face {f} in two levels");
        }
    }
    // The z riser (x = 50) is taken, so it is no x level, and the other walls are levels.
    assert_eq!(
        z_risers
            .iter()
            .map(|o| o.defining.clone())
            .collect::<Vec<_>>(),
        [face(0, 50.)]
    );
    assert!(risers.is_empty());
    // An x level's spans run along y then z, a y level's along z then x.
    let x = other.iter().find(|o| o.record.height_axis == 'x').unwrap();
    assert_eq!(
        (x.record.x_span, x.record.y_span),
        (Some((0., 40.)), Some((0., 20.)))
    );
    assert_eq!(levels::plane_axes(0), [1, 2, 0]);
    assert_eq!(levels::plane_axes(1), [2, 0, 1]);
    assert_eq!(levels::plane_axes(2), [0, 1, 2]);
}

/// A face another family claims is never a level along the frame's x or y. In its inferred
/// frame (z along the file's x) this part is a plate with a pocket and a pad, which claim the
/// tops and sides read as levels above; only the frame's z levels (the x = 50 and x = 100 ends,
/// co-owning as before) remain.
#[test]
fn claimed_faces_give_no_level_along_the_new_axes() {
    let p = part("multi_axis_steps.step.gz");
    let recognition = quiddity::correspondence::recognise(&p);
    let document = quiddity::recognition::document(&recognition, p.faces.len());
    let at_x = |v: f64| {
        (0..p.faces.len())
            .filter(|&f| p.face_normal(f, 0., 0.).is_some_and(|n| n[0].abs() > 0.99))
            .filter(|&f| (p.face_bounds(f).min[0] - v).abs() < 1e-6)
            .collect::<Vec<_>>()
    };
    let mut found: Vec<Vec<usize>> = document
        .records
        .iter()
        .filter(|r| r.family == "step_levels" || r.family == "risers")
        .map(|r| r.faces.clone())
        .collect();
    found.sort();
    let mut want = vec![at_x(50.), at_x(100.)];
    want.sort();
    assert_eq!(found, want);
}

/// The faces of *file*'s published step levels, or `None` without the corpus (refused when the
/// corpus is required).
fn level_faces(file: &str) -> Option<BTreeSet<usize>> {
    let Some(dir) = common::corpus_dir() else {
        assert!(
            std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
            "QUIDDITY_CORPUS_REQUIRED is set but the corpus was not found"
        );
        return None;
    };
    let p = quiddity::read_step_file(&dir.join(file)).unwrap();
    let recognition = quiddity::correspondence::recognise(&p);
    let document = quiddity::recognition::document(&recognition, p.faces.len());
    Some(
        document
            .records
            .iter()
            .filter(|r| r.family == "step_levels")
            .flat_map(|r| r.faces.iter().copied())
            .collect(),
    )
}

/// ctc_02's drafted walls (tilted 2-5 degrees, spanning 6.1 along the frame's z) are no level,
/// while its flat shelves 540 and 390 apart, facing each other across the part, stay levels:
/// that gap is longer than the faces are wide, so they are not one channel's walls.
#[test]
fn drafted_walls_are_no_level_and_wide_facing_shelves_are() {
    let Some(faces) = level_faces("nist/nist_ctc_02_asme1_rc.stp") else {
        return;
    };
    for f in [205, 323, 367] {
        assert!(!faces.contains(&f), "drafted face {f} is a level");
    }
    for f in [214, 358, 188, 313] {
        assert!(faces.contains(&f), "shelf {f} is no level");
    }
}

/// ctc_04's 25-wide slots: each wall faces the other across the slot, fully overlapping, so
/// neither is a level along the frame's y.
#[test]
fn slot_walls_are_no_level() {
    let Some(faces) = level_faces("nist/nist_ctc_04_asme1_rd.stp") else {
        return;
    };
    for f in [154, 156, 67, 162] {
        assert!(!faces.contains(&f), "slot wall {f} is a level");
    }
}

/// Step levels and risers in `quiddity evidence` are the recognition document's, part by part
/// over the corpus: framed evidence against the published (framed) document, file-coordinate
/// evidence against the caller-space recognition of the same part.
#[test]
fn evidence_step_levels_and_risers_match_the_documents() {
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
    let sorted = |mut faces: Vec<Vec<usize>>| {
        for f in &mut faces {
            f.sort_unstable();
        }
        faces.sort();
        faces
    };
    let differ = common::parallel::map(&files, |file| {
        let p = quiddity::read_step_file(&dir.join(file)).unwrap();
        let mut differ = Vec::new();
        for framed in [true, false] {
            let recognition = if framed {
                quiddity::correspondence::recognise(&p)
            } else {
                quiddity::correspondence::caller_space_recognise(&p)
            };
            let document = quiddity::recognition::document(&recognition, p.faces.len());
            let evidence = quiddity::evidence_view::document(&p, framed).unwrap();
            for family in ["step_levels", "risers"] {
                let published = sorted(
                    document
                        .records
                        .iter()
                        .filter(|r| r.family == family)
                        .map(|r| r.faces.clone())
                        .collect(),
                );
                let found = sorted(
                    evidence
                        .features
                        .iter()
                        .filter(|f| f.family == family)
                        .map(|f| f.defining_faces.clone())
                        .collect(),
                );
                if found != published {
                    differ.push(format!("{file} {family} framed {framed}"));
                }
            }
        }
        differ
    });
    let differ: Vec<String> = differ.into_iter().flatten().collect();
    let parts: std::collections::BTreeSet<&str> = differ
        .iter()
        .map(|d| d.split(' ').next().unwrap())
        .collect();
    assert!(
        differ.is_empty(),
        "{} parts differ: {differ:?}",
        parts.len()
    );
}
