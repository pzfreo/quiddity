//! Wire-local adjacency preserves the original occurrence pairing and face-set contract.
//! The synthetic incidence graphs below were created for these tests under Apache-2.0;
//! they contain no imported or derived customer geometry and make no solid-validity claim.

use super::common;
use quiddity::features::graph::shared_occurrences;
use quiddity::features::wire_seed::wire_seed;
use quiddity::kernel::brep::{Edge, Face, Loop, Part};
use quiddity::kernel::geom::{Curve, Frame, Surface};
use std::collections::BTreeSet;

fn reference(part: &Part, opening: usize, wire: &[usize]) -> BTreeSet<usize> {
    part.neighbours(opening)
        .into_iter()
        .filter(|&other| {
            shared_occurrences(part, opening, other)
                .iter()
                .any(|edge| wire.contains(edge))
        })
        .collect()
}

fn topology(face_loops: Vec<Vec<Vec<(usize, bool)>>>, edge_count: usize) -> Part {
    let frame = Frame {
        origin: [0.; 3],
        x: [1., 0., 0.],
        y: [0., 1., 0.],
        z: [0., 0., 1.],
    };
    let faces = face_loops
        .into_iter()
        .map(|loops| Face {
            surface: Surface::Plane { frame },
            reversed: false,
            loops: loops
                .into_iter()
                .map(|edges| Loop {
                    edges,
                    vertex: None,
                })
                .collect(),
            solid: None,
            pcurves: vec![],
        })
        .collect();
    let edges = (0..edge_count)
        .map(|i| {
            let start = [i as f64, 0., 0.];
            let end = [i as f64, 1., 0.];
            Edge {
                curve: Curve::Line {
                    origin: start,
                    dir: [0., 1., 0.],
                },
                start,
                end,
                vertices: (2 * i, 2 * i + 1),
                same_sense: true,
                samples: vec![start, end],
            }
        })
        .collect();
    Part::new(faces, edges, vec![])
}

#[test]
fn occurrence_pairing_keeps_seams_and_refusals() {
    let uses: Vec<Vec<bool>> = (0..=4)
        .flat_map(|n| (0..1 << n).map(move |bits| (0..n).map(|i| bits & (1 << i) != 0).collect()))
        .collect();
    for left in &uses {
        for right in &uses {
            // A single opposite use or one use in each direction has a unique pairing.
            // Three or more boolean uses necessarily have duplicate opposite partners.
            let paired = matches!(
                (left.as_slice(), right.as_slice()),
                ([false], [true])
                    | ([true], [false])
                    | ([false, true] | [true, false], [false, true] | [true, false])
            );
            let expected = if paired {
                BTreeSet::from([1])
            } else {
                BTreeSet::new()
            };
            // Split the opening's uses across loops; all uses of an edge must be read.
            let part = topology(
                vec![
                    left.iter().map(|&forward| vec![(0, forward)]).collect(),
                    vec![right.iter().map(|&forward| (0, forward)).collect()],
                ],
                1,
            );
            for wire in [vec![0], vec![0, 0], vec![usize::MAX, 0]] {
                assert_eq!(wire_seed(&part, 0, &wire), expected, "{left:?} / {right:?}");
                assert_eq!(reference(&part, 0, &wire), expected);
            }
            assert!(wire_seed(&part, 0, &[]).is_empty());
            assert!(wire_seed(&part, 0, &[usize::MAX]).is_empty());
        }
    }
}

#[test]
fn owners_are_paired_individually_and_foreign_edges_do_not_seed() {
    let part = topology(
        vec![
            vec![vec![(0, true), (1, false)]],
            vec![vec![(0, false), (1, true), (2, true)]],
            vec![vec![(0, false), (2, false)]],
            vec![vec![(0, true)]],
            vec![vec![(0, false), (0, false)]],
        ],
        3,
    );
    for (wire, expected) in [
        (vec![0], BTreeSet::from([1, 2])),
        (vec![1], BTreeSet::from([1])),
        (vec![1, 0, 1], BTreeSet::from([1, 2])),
        (vec![2, usize::MAX], BTreeSet::new()),
    ] {
        assert_eq!(wire_seed(&part, 0, &wire), expected);
        assert_eq!(reference(&part, 0, &wire), expected);
    }
}

#[test]
fn public_fixture_loops_match_full_neighbour_scan() {
    for name in [
        "semicircular_blind_slot.step",
        "golden_straight_and_obround_slots.step",
        "golden_rectangular_blind_slot.step",
        "golden_triangular_and_hex_pockets.step",
    ] {
        let part = quiddity::read_step_file(&common::fixtures().join(name)).unwrap();
        for face in 0..part.faces.len() {
            for lp in &part.faces[face].loops {
                let wire: Vec<_> = lp.edges.iter().map(|e| e.0).collect();
                assert_eq!(wire_seed(&part, face, &wire), reference(&part, face, &wire));
            }
        }
    }
}

fn many_wires(count: usize) -> Part {
    let mut faces = vec![
        (0..count)
            .map(|i| vec![(2 * i, true), (2 * i + 1, true)])
            .collect(),
    ];
    for edge in 0..2 * count {
        faces.push(vec![vec![(edge, false)]]);
    }
    topology(faces, 2 * count)
}

#[test]
fn dense_opening_returns_only_each_wires_two_owners() {
    let count = 1024;
    let part = many_wires(count);
    for i in 0..count {
        let wire = [2 * i, 2 * i + 1];
        assert_eq!(
            wire_seed(&part, 0, &wire),
            BTreeSet::from([2 * i + 1, 2 * i + 2])
        );
    }
}

#[test]
#[ignore = "Manual timing comparison on an independently constructed dense opening"]
fn dense_opening_timing() {
    use std::hint::black_box;
    use std::time::Instant;

    let count = 1024;
    let part = many_wires(count);
    let _ = part.edge_faces();
    let mut reference_seconds = 0.;
    let mut indexed_seconds = 0.;
    for i in [0, count / 2, count - 1] {
        let wire = [2 * i, 2 * i + 1];
        let start = Instant::now();
        let expected = black_box(reference(&part, 0, &wire));
        reference_seconds += start.elapsed().as_secs_f64();
        let start = Instant::now();
        let actual = black_box(wire_seed(&part, 0, &wire));
        indexed_seconds += start.elapsed().as_secs_f64();
        assert_eq!(actual, expected);
        assert_eq!(actual, BTreeSet::from([2 * i + 1, 2 * i + 2]));
    }
    let start = Instant::now();
    for i in 0..count {
        let actual = black_box(wire_seed(&part, 0, &[2 * i, 2 * i + 1]));
        assert_eq!(actual, BTreeSet::from([2 * i + 1, 2 * i + 2]));
    }
    let all_indexed_seconds = start.elapsed().as_secs_f64();
    eprintln!(
        "WIRE_SEED_BENCHMARK wires={count} neighbours={} samples=3 reference_seconds={reference_seconds:.9} indexed_seconds={indexed_seconds:.9} all_indexed_seconds={all_indexed_seconds:.9}",
        2 * count
    );
}
