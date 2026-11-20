//! `flats` are planar truncations of round stock, not the sides of ribs, fins or tabs
//! standing on it. The `flats_*` fixtures are synthetic (build123d): an R20 shaft, 40 long.
use super::common;
use quiddity::features;

fn flats(name: &str) -> Vec<features::flats::Flat> {
    let p = quiddity::read_step_file(&common::fixtures().join(name)).unwrap();
    features::recognise(&p).flats
}

#[test]
fn fin_and_tab_sides_standing_on_a_cylinder_are_not_flats() {
    // Six 4-thick fins reaching R30: each side meets the shaft at its inner end but runs out
    // beyond the stock, and its chord lies wholly to one side of the nearest point.
    assert!(flats("flats_finned_shaft.step.gz").is_empty());
    // A tab whose tip stands only 0.3 beyond R20 (inside the 2.5 % end tolerance): the radius
    // cannot tell it from a flat, but the edge to the cylinder is concave, so it is a tab side.
    assert!(flats("flats_low_tab.step.gz").is_empty());
}

#[test]
fn a_key_flat_on_a_shaft_is_kept() {
    let f = flats("flats_key_flat.step.gz");
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].across, 32.); // 12 from the axis to the flat, plus the 20 radius
    assert_eq!(*f[0].stock_span, (0., 40.));
}

#[test]
fn a_double_d_keeps_both_flats() {
    let f = flats("spindle_flats.step");
    assert_eq!(f.len(), 2);
    assert!(f.iter().all(|flat| flat.across == 20.));
}

#[test]
fn a_flat_running_past_its_cylinder_keeps_its_value() {
    // 7 from the axis on an R10 shaft: across 17, whether the flat runs on into a collar's
    // chamfer or crosses a slightly larger land (which must not take the binding).
    for name in ["flats_chamfer_overrun.step.gz", "flats_over_land.step.gz"] {
        let f = flats(name);
        assert_eq!(f.len(), 1, "{name}");
        assert_eq!(f[0].across, 17., "{name}");
    }
}

#[test]
fn a_block_side_a_boss_pokes_through_is_not_a_flat() {
    // The boss cylinder exists beyond the side's plane (a real flat cuts it away), though the
    // boss's chord vertices sit at the ends of its own axial span.
    assert!(flats("flats_boss_through_block.step.gz").is_empty());
}
