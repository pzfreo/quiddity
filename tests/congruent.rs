//! Geometric congruence (`correspondence::congruent`, `docs/correspondence.md`, "Congruence"):
//! two parts match only under a proper rigid motion proven face by face. Translated copies are
//! made here by rewriting a fixture's `CARTESIAN_POINT`s, so each copy is its own B-rep, as an
//! exporter writes repeated bought parts.

use std::io::Read;
use std::path::{Path, PathBuf};

use quiddity::Part;
use quiddity::correspondence::align::align;
use quiddity::correspondence::{Motion, congruent, fingerprint};
use quiddity::kernel::read_step;
use quiddity::kernel::step::read_step_file_placed;

fn fixture(name: &str) -> PathBuf {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    match name.strip_prefix("congruent/") {
        Some(own) => tests.join("congruent").join(own),
        None => tests.join("fixtures").join(name),
    }
}

fn text(name: &str) -> String {
    let bytes = std::fs::read(fixture(name)).unwrap();
    if name.ends_with(".gz") {
        let mut s = String::new();
        flate2::read::GzDecoder::new(&bytes[..])
            .read_to_string(&mut s)
            .unwrap();
        s
    } else {
        String::from_utf8(bytes).unwrap()
    }
}

/// *step* with every 3D `CARTESIAN_POINT` moved by *t*: a translated copy, written out as its own
/// B-rep (parameter-space points have two coordinates and stay as they are).
fn translated(step: &str, t: [f64; 3]) -> Part {
    const KEY: &str = "CARTESIAN_POINT(";
    let mut out = String::with_capacity(step.len());
    let mut rest = step;
    while let Some(i) = rest.find(KEY) {
        // The coordinate list: the entity's last parenthesised group.
        let start = i + KEY.len();
        let entity = &rest[start..start + rest[start..].find(';').unwrap()];
        let open = entity.rfind('(').unwrap();
        let close = open + entity[open..].find(')').unwrap();
        let values: Vec<f64> = entity[open + 1..close]
            .split(',')
            .map(|v| v.trim().parse().unwrap())
            .collect();
        out.push_str(&rest[..start + open + 1]);
        if values.len() == 3 {
            let moved: Vec<String> = (0..3).map(|k| format!("{:?}", values[k] + t[k])).collect();
            out.push_str(&moved.join(","));
        } else {
            out.push_str(&entity[open + 1..close]);
        }
        rest = &rest[start + close..];
    }
    out.push_str(rest);
    read(&out)
}

fn read(step: &str) -> Part {
    read_step(step.as_bytes()).unwrap()
}

fn translation(m: &Motion) -> Option<[f64; 3]> {
    (m.r == Motion::IDENTITY.r).then_some(m.t)
}

fn assert_bijection(pairs: &[(usize, usize)], n: usize) {
    let mut a: Vec<_> = pairs.iter().map(|p| p.0).collect();
    let mut b: Vec<_> = pairs.iter().map(|p| p.1).collect();
    a.sort_unstable();
    b.sort_unstable();
    assert_eq!(a, (0..n).collect::<Vec<_>>());
    assert_eq!(b, (0..n).collect::<Vec<_>>());
}

#[test]
fn identity_is_congruent_face_for_face() {
    let step = text(HEX);
    let (a, b) = (read(&step), read(&step));
    assert!(a.same_brep(&b));
    let (m, pairs) = congruent(&a, &b).expect("a part is congruent to itself");
    assert_eq!(m, Motion::IDENTITY);
    assert_eq!(
        pairs,
        (0..a.faces.len()).map(|i| (i, i)).collect::<Vec<_>>()
    );
}

/// Translated copies of a rotationally symmetric part: the fingerprint alignment returns a
/// symmetry of the fingerprints that is not one of the B-rep, so the proof needs the centroid
/// translation, tried first.
#[test]
fn translated_symmetric_copies_match_by_translation() {
    let step = text(HEX);
    let a = read(&step);
    for t in [[-67.0, 42.24, 0.0], [12.5, 42.72, 3.0]] {
        let b = translated(&step, t);
        assert!(!a.same_brep(&b));
        // The case this order exists for: align() turns the hexagon instead.
        let none = Default::default();
        let (aligned, _) = align(
            &fingerprint(&a, &none),
            &fingerprint(&b, &none),
            &Default::default(),
        );
        assert_ne!(
            aligned.r,
            Motion::IDENTITY.r,
            "align() now finds the translation itself"
        );
        let (m, pairs) = congruent(&a, &b).expect("translated copies are congruent");
        let got = translation(&m).expect("a pure translation");
        for i in 0..3 {
            assert!((got[i] - t[i]).abs() <= 1e-9, "{got:?} != {t:?}");
        }
        assert_bijection(&pairs, a.faces.len());
        // And back.
        assert!(congruent(&b, &a).is_some());
    }
}

/// A rotated and moved copy still matches, through the fingerprint alignment.
#[test]
fn rotated_copy_matches_by_alignment() {
    let a = read(&text(CHIRAL));
    let (c, s) = (0.6f64, 0.8f64);
    let placement = [[c, -s, 0.0, 20.0], [s, c, 0.0, -5.0], [0.0, 0.0, 1.0, 7.0]];
    let b = read_step_file_placed(&fixture(CHIRAL), &placement).unwrap();
    let (m, pairs) = congruent(&a, &b).expect("a rotated copy is congruent");
    assert!(
        (m.r[0][0] - c).abs() < 1e-9 && (m.r[1][0] - s).abs() < 1e-9,
        "{m:?}"
    );
    assert_bijection(&pairs, a.faces.len());
}

#[test]
fn mirror_image_does_not_match() {
    let a = read(&text(CHIRAL));
    let b = read(&text(MIRROR));
    // The mirror image is the same part in every intrinsic respect: only its handedness differs.
    assert_eq!(a.faces.len(), b.faces.len());
    let areas = |p: &Part| {
        let f = fingerprint(p, &Default::default());
        let mut v: Vec<f64> = f.faces.iter().map(|f| f.area.unwrap()).collect();
        v.sort_by(f64::total_cmp);
        v
    };
    for (x, y) in areas(&a).iter().zip(areas(&b)) {
        assert!((x - y).abs() <= 1e-7 * x.max(1.0), "{x} {y}");
    }
    assert!(congruent(&a, &b).is_none());
    assert!(congruent(&b, &a).is_none());
}

/// One feature moved: the same topology and nearly every face the same, but not congruent.
#[test]
fn one_changed_face_does_not_match() {
    let a = read(&text("revisions/hole_moved.old.step.gz"));
    let b = read(&text("revisions/hole_moved.new.step.gz"));
    assert_eq!(a.faces.len(), b.faces.len());
    assert_eq!(a.edges.len(), b.edges.len());
    assert!(congruent(&a, &b).is_none());
    assert!(congruent(&b, &a).is_none());
}

/// An M3 x 8 hex-head bolt built off the origin (`tools/make_congruent_fixture.py`).
const HEX: &str = "congruent/hex_bolt.step";
/// A chiral part, and its mirror image as Open CASCADE wrote it. Negating coordinates in the
/// file instead would leave left-handed frames, reversed circle parameters and loops, so not a
/// valid B-rep of the mirror image.
const CHIRAL: &str = "interrupted_corner.step";
const MIRROR: &str = "interrupted_corner_mirror.step";
