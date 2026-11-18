//! Analytic material slabs and refusal cases, independent of recognition snapshots.
use super::*;
use std::cell::RefCell;

#[test]
fn a_single_original_slab_is_complete_even_if_its_material_would_stop_a_prefix() {
    let calls = RefCell::new(Vec::new());
    let slab = |lo: f64, hi: f64| {
        calls.borrow_mut().push((lo, hi));
        Some([0.25 * (hi - lo), 0.75 * (hi - lo)])
    };
    assert_eq!(
        integrate_witness_slabs(&slab, (0.0, 1.0), vec![], &|v| v[0] > 0.0),
        Some(([0.25, 0.75], false))
    );
    assert_eq!(*calls.borrow(), vec![(0.0, 1.0)]);
}

#[test]
fn broad_original_slab_finds_material_beside_clustered_breaks() {
    // Unit cross-section, material only in z in [1/2, 1]. Thin lower slabs are air.
    let calls = RefCell::new(Vec::new());
    let breaks = vec![
        0.4, 0.41, 0.42, 0.43, 0.44, 0.45, 0.46, 0.47, 0.48, 0.49, 0.5,
    ];
    let slab = |lo: f64, hi: f64| {
        calls.borrow_mut().push((lo, hi));
        Some([
            if lo >= 0.5 { hi - lo } else { 0.0 },
            if hi <= 0.5 { hi - lo } else { 0.0 },
        ])
    };
    let result = integrate_witness_slabs(&slab, (0.0, 1.0), breaks, &|v| v[0] > 0.25);
    assert_eq!(result, Some(([0.5, 0.0], true)));
    assert_eq!(*calls.borrow(), vec![(0.5, 1.0)]);
}

#[test]
fn complete_sum_keeps_rounding_and_evaluates_each_original_slab_once() {
    // Adding the broad slab first would lose the two unit contributions to rounding.
    let calls = RefCell::new(Vec::new());
    let slab = |lo: f64, hi: f64| {
        calls.borrow_mut().push((lo, hi));
        Some([if lo >= 0.2 { 1e16 } else { 1.0 }, hi - lo])
    };
    let (value, short) =
        integrate_witness_slabs(&slab, (0.0, 1.0), vec![0.1, 0.2], &|_| false).unwrap();
    assert!(!short);
    assert_eq!(value[0].to_bits(), ((1.0f64 + 1.0) + 1e16).to_bits());
    let mut actual = calls.into_inner();
    assert_eq!(actual[0], (0.2, 1.0));
    actual.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(actual, vec![(0.0, 0.1), (0.1, 0.2), (0.2, 1.0)]);
}

#[test]
fn required_unknowns_refuse_completion_but_do_not_undo_a_sufficient_rejection() {
    let breaks: Vec<_> = (1..=25).map(|i| i as f64 / 50.0).collect();
    let slab = |lo: f64, hi: f64| (lo != 0.2).then_some([hi - lo, 0.0]);
    assert_eq!(
        integrate_witness_slabs(&slab, (0.0, 1.0), breaks.clone(), &|v| v[0] > 0.25),
        Some(([0.5, 0.0], true))
    );
    assert!(integrate_witness_slabs(&slab, (0.0, 1.0), breaks, &|v| v[0] > 1.0).is_none());
}

#[test]
fn equality_does_not_reject_and_equal_widths_start_at_the_first_slab() {
    let calls = RefCell::new(Vec::new());
    let slab = |lo: f64, hi: f64| {
        calls.borrow_mut().push((lo, hi));
        Some([if lo == 0.0 { 0.25 } else { 0.0 }, hi - lo])
    };
    assert_eq!(
        integrate_witness_slabs(&slab, (0.0, 1.0), vec![0.25, 0.5, 0.75], &|v| v[0] > 0.25),
        Some(([0.25, 1.0], false))
    );
    assert_eq!(calls.borrow()[0], (0.0, 0.25));
}
