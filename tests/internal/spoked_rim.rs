//! The inner wall of a spoked rim is not a counterbore of the hub bore (quiddity#307 item 4).
use super::common;
use quiddity::features::{Context, holes};

#[test]
fn a_rim_wall_round_the_hub_is_no_step_but_real_counterbores_stay() {
    let part = quiddity::read_step_file(&common::fixtures().join("spoked_rim.step.gz")).unwrap();
    let found = holes::discover_verified(&Context::new(&part), &[]).unwrap();
    let at = |x: f64| {
        let mut on_x: Vec<_> = found
            .iter()
            .map(|o| &o.record)
            .filter(|h| h.diameter == 16. && (h.location.raw()[0] - x).abs() < 1.)
            .collect();
        assert_eq!(on_x.len(), 1, "one d16 hole at x={x}");
        on_x.remove(0)
    };
    // The flexure: d16 through the hub, the d52 rim wall is nothing of the hole's.
    let flexure = at(0.);
    assert_eq!(flexure.bottom, holes::Bottom::Through);
    assert!(flexure.cbore.is_none() && flexure.outer_steps.is_empty());
    assert_eq!(flexure.depth, 2.5);
    // Spoked wheel hub with a real d20 counterbore, alone or with a cross hole.
    for x in [80., 160.] {
        let hole = at(x);
        let cbore = hole.cbore.as_ref().expect("the hub counterbore");
        assert_eq!((cbore.diameter, cbore.depth), (20., 1.5), "x={x}");
        assert!(hole.outer_steps.is_empty(), "x={x}");
        assert_eq!(hole.depth, 1.0, "x={x}");
    }
    // A plain stepped disc keeps its counterbore.
    let plain = at(240.);
    let cbore = plain.cbore.as_ref().expect("the counterbore");
    assert_eq!((cbore.diameter, cbore.depth), (28., 1.5));
    // The d3 cross hole is still found.
    assert!(found.iter().any(|o| o.record.diameter == 3.));
    // Nothing is published for the rim walls.
    assert!(found.iter().all(|o| o.record.diameter < 40.));
}

#[test]
fn collared_counterbores_with_a_blended_or_split_floor_are_kept() {
    let part =
        quiddity::read_step_file(&common::fixtures().join("collared_floor_variants.step.gz"))
            .unwrap();
    let found = holes::discover_verified(&Context::new(&part), &[]).unwrap();
    // (x, counterbore depth): wall-to-floor R1 fillet, 1 mm chamfer, faces split in halves.
    for (x, depth) in [(0., 3.), (100., 3.), (200., 4.)] {
        let hole = found
            .iter()
            .map(|o| &o.record)
            .find(|h| h.diameter == 10. && (h.location.raw()[0] - x).abs() < 1.)
            .unwrap_or_else(|| panic!("the d10 hole at x={x}"));
        let cbore = hole
            .cbore
            .as_ref()
            .unwrap_or_else(|| panic!("cbore at x={x}"));
        assert_eq!((cbore.diameter, cbore.depth), (40., depth), "x={x}");
    }
}
