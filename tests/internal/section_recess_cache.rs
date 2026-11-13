//! Reusing geometry discovery must retain its bytes, mode and inventory independence.

use super::common;
use quiddity::features::Context;
use quiddity::features::effective_surfaces::EffectiveFaces;
use quiddity::features::evidence::locally_degraded;
use quiddity::features::section_recess_discovery::discover_section_recesses;
use quiddity::features::section_recess_family::section_recess_projection_in;
use quiddity::features::take_inventory;

#[test]
fn section_recess_cache_preserves_native_bytes_and_separates_modes() {
    let mut positive_records = 0;
    for file in [
        "golden_triangular_and_hex_pockets.step",
        "golden_blind_pockets_and_pocket_patterns.step",
        "two_solids.step",
        "unowned_recess_solid.step.gz",
        "unowned_recess_shell.step.gz",
        "unowned_recess_mixed.step.gz",
    ] {
        let part = quiddity::read_step_file(&common::fixtures().join(file)).unwrap();
        let ctx = Context::new(&part);
        let check = || {
            let surfaces = EffectiveFaces::new(&ctx);
            let direct = discover_section_recesses(&ctx, &surfaces)
                .map(|found| found.into_iter().map(|o| o.record).collect::<Vec<_>>());
            let first = ctx.native_section_recesses(&surfaces);
            let again = ctx.native_section_recesses(&EffectiveFaces::new(&ctx));
            assert!(std::ptr::eq(first, again), "{file}");
            assert_eq!(*first, direct, "{file}");
            if let Ok(records) = first {
                assert_eq!(
                    serde_json::to_vec(records).unwrap(),
                    serde_json::to_vec(direct.as_ref().unwrap()).unwrap(),
                    "{file}"
                );
            }
            first
        };
        let strict = check();
        positive_records += strict.as_ref().map_or(0, Vec::len);
        let degraded = locally_degraded(check);
        assert!(!std::ptr::eq(strict, degraded), "{file}");
        assert!(std::ptr::eq(strict, check()), "{file}");
        let another = Context::new(&part);
        assert!(!std::ptr::eq(
            strict,
            another.native_section_recesses(&EffectiveFaces::new(&another))
        ));
    }
    assert!(
        positive_records > 0,
        "the byte comparison must include positive geometry"
    );
}

#[test]
fn section_recess_cache_does_not_reuse_an_earlier_inventory_projection() {
    let part = quiddity::read_step_file(
        &common::fixtures().join("golden_blind_pockets_and_pocket_patterns.step"),
    )
    .unwrap();
    take_inventory(&part, |ctx, mut inventory| {
        let before = section_recess_projection_in(ctx, &inventory).unwrap();
        assert!(!before.patterns.is_empty());
        inventory.physical.pockets.clear();
        let after = section_recess_projection_in(ctx, &inventory).unwrap();
        let fresh = Context::new(&part);
        assert_eq!(
            after,
            section_recess_projection_in(&fresh, &inventory).unwrap()
        );
        assert!(after.patterns.is_empty());
        assert_ne!(before.patterns, after.patterns);
        assert_eq!(before.section_recesses, after.section_recesses);
    })
    .unwrap();
}
