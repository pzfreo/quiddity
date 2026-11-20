//! Face index order is a contract: face *i* of a read part is `face_source(i)`, and the order is
//! the reader's traversal (docs/evidence.md): placed solid instances in assembly traversal order,
//! each its outer shell's faces then its void shells' in `CLOSED_SHELL` order, then the shells of
//! surface models. For solids it is OpenCascade's `TopExp::MapShapes` order, except that a face
//! shared by two shells is listed twice.
//!
//! That is the ascending `ADVANCED_FACE` file order whenever the file writes its solids and each
//! shell's faces in ascending instance order (every exporter output in the corpus does), and only
//! then. These tests pin both: the equality on single- and multi-solid fixtures, and the
//! traversal order where it differs from file order (`two_solids_reversed.step`). `instance` is
//! the placed instance (a solid's index in `Part::solids`); a part placed more than once repeats
//! its `ADVANCED_FACE` entities per instance without restarting the face index.
//!
//! The order was also checked against OpenCascade directly (face area and centroid, face by face,
//! over `TopExp_Explorer` with `IsSame` de-duplication) on every fixture below;
//! `tests/face_sources.rs` checks it against the `TopExp::MapShapes` capture of the corpus and
//! NIST files.

mod common;

use haecceity::Part;
use haecceity::step::read_step;

fn text(name: &str) -> String {
    std::fs::read_to_string(common::fixtures().join(name)).unwrap()
}

/// The `#N` of every `ADVANCED_FACE` in the file, in file order.
fn advanced_faces(step: &str) -> Vec<u64> {
    step.lines()
        .filter_map(|l| {
            let (id, rest) = l.strip_prefix('#')?.split_once('=')?;
            rest.trim_start()
                .starts_with("ADVANCED_FACE(")
                .then(|| id.trim().parse().unwrap())
        })
        .collect()
}

/// `(entity, instance)` of every face, in index order.
fn sources(part: &Part) -> Vec<(u64, usize)> {
    (0..part.faces.len())
        .map(|i| {
            let s = part.face_source(i).expect("a file read records every face");
            (s.entity, s.instance)
        })
        .collect()
}

fn read(name: &str) -> (Vec<u64>, Part) {
    let step = text(name);
    let part = read_step(step.as_bytes()).unwrap();
    (advanced_faces(&step), part)
}

#[test]
fn single_solid_index_is_advanced_face_file_order() {
    let (file, part) = read("filleted_box_one_edge.step");
    assert_eq!(part.solids.len(), 1);
    let got = sources(&part);
    assert_eq!(got.iter().map(|s| s.0).collect::<Vec<_>>(), file);
    assert!(got.iter().all(|s| s.1 == 0));
}

#[test]
fn multi_solid_index_is_file_order_when_the_file_lists_solids_in_order() {
    let (file, part) = read("two_solids.step");
    assert_eq!(part.solids.len(), 2);
    let got = sources(&part);
    assert_eq!(got.iter().map(|s| s.0).collect::<Vec<_>>(), file);
    // `instance` is the solid's index in `Part::solids`; the face index does not restart.
    for (solid, s) in part.solids.iter().enumerate() {
        assert!(!s.faces.is_empty());
        assert!(s.faces.iter().all(|&f| got[f].1 == solid));
    }
    assert_eq!(part.solids[1].faces[0], part.solids[0].faces.len());
}

#[test]
fn index_follows_traversal_not_entity_ids_when_the_file_lists_solids_out_of_order() {
    // The representation lists the solid whose faces come later in the file first.
    let (file, part) = read("two_solids_reversed.step");
    let got = sources(&part);
    let entities: Vec<u64> = got.iter().map(|s| s.0).collect();
    assert_ne!(entities, file, "traversal order, not ascending entity ids");
    let (late, early) = file.split_at(file.len() / 2);
    let expected: Vec<u64> = early.iter().chain(late).copied().collect();
    // The later-numbered solid is traversed first, so it is instance 0.
    assert_eq!(entities, expected);
    let instances: Vec<usize> = got.iter().map(|s| s.1).collect();
    assert_eq!(instances[0], 0);
    assert_eq!(*instances.last().unwrap(), 1);
    assert!(instances.windows(2).all(|w| w[0] <= w[1]));
    // Within a solid the order is the shell's own list, here ascending.
    for solid in &part.solids {
        let e: Vec<u64> = solid.faces.iter().map(|&f| got[f].0).collect();
        assert!(e.windows(2).all(|w| w[0] < w[1]));
    }
}

#[test]
fn a_part_placed_repeatedly_repeats_entities_per_instance() {
    let (file, part) = read("golden_traversal_order.step");
    let got = sources(&part);
    assert_eq!(part.solids.len(), 3);
    assert_eq!(got.len(), 3 * file.len());
    for (instance, chunk) in got.chunks(file.len()).enumerate() {
        assert_eq!(chunk.iter().map(|s| s.0).collect::<Vec<_>>(), file);
        assert!(chunk.iter().all(|s| s.1 == instance));
    }
}
