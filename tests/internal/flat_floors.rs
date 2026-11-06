//! Original boolean-cut controls, independent of the output snapshots.
use super::common;
use quiddity::features::{self, Context, holes, pockets};
use quiddity::kernel::geom::{self, Surface};
use quiddity::kernel::step::IDENTITY;

fn controls() -> quiddity::Part {
    quiddity::read_step_file(&common::fixtures().join("flat_floor_controls.step.gz")).unwrap()
}

#[test]
fn flat_floor_completion_patterns_follow_completed_members() {
    use quiddity::correspondence::fingerprint::{fingerprint, published_fingerprint};
    use quiddity::framed_records::{Rigid, map_features};
    use std::collections::BTreeSet;

    let part =
        quiddity::read_step_file(&common::fixtures().join("flat_floor_patterns.step.gz")).unwrap();
    let before = features::inventory(&part).unwrap().accepted();
    let after = features::recognise(&part);
    assert_eq!(before.holes.len(), 3);
    assert_eq!(before.pockets.len(), 3);
    assert_eq!(before.hole_patterns.len(), 1);
    assert_eq!(before.pocket_patterns.len(), 1);
    assert_eq!(
        serde_json::to_value(&before.hole_patterns).unwrap(),
        serde_json::to_value(&after.hole_patterns).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&before.pocket_patterns).unwrap(),
        serde_json::to_value(&after.pocket_patterns).unwrap()
    );
    for family in ["holes", "pockets"] {
        assert_eq!(before.defining[family].len(), after.defining[family].len());
        for (walls, completed) in before.defining[family].iter().zip(&after.defining[family]) {
            assert!(walls.iter().all(|f| completed.contains(f)));
            assert_eq!(completed.len(), walls.len() + 1);
        }
    }
    let old = fingerprint(&part, &before);
    let new = fingerprint(&part, &after);
    for (pattern, parent) in [("hole_patterns", "holes"), ("pocket_patterns", "pockets")] {
        let old = old.features.iter().find(|f| f.family == pattern).unwrap();
        let new = new.features.iter().find(|f| f.family == pattern).unwrap();
        let members: BTreeSet<_> = after.defining[parent].iter().flatten().copied().collect();
        assert_eq!(new.faces.iter().copied().collect::<BTreeSet<_>>(), members);
        assert!(old.faces.iter().all(|f| new.faces.contains(f)));
        assert_eq!(
            new.faces.len(),
            old.faces.len() + 3,
            "{pattern} gains all three member floors"
        );
        assert_eq!(old.sizes, new.sizes);
        assert_eq!(old.traits, new.traits);
    }
    // Publication must derive the same member union after identity or oblique motion.
    let (s, c) = 0.37_f64.sin_cos();
    for at in [
        IDENTITY,
        [[c, 0., s, 123.456], [0., 1., 0., -78.9], [-s, 0., c, 41.3]],
    ] {
        let motion = Rigid::from_placement(&at);
        let published = map_features(after.clone(), &motion).features;
        let fingerprints = published_fingerprint(&part, &published);
        for (pattern, parent) in [("hole_patterns", "holes"), ("pocket_patterns", "pockets")] {
            let members: BTreeSet<_> = published.defining[parent]
                .iter()
                .flatten()
                .copied()
                .collect();
            let pattern = fingerprints
                .features
                .iter()
                .find(|f| f.family == pattern)
                .unwrap();
            assert_eq!(
                pattern.faces.iter().copied().collect::<BTreeSet<_>>(),
                members
            );
        }
    }
}

fn floor_at(part: &quiddity::Part, x: f64) -> usize {
    let floors: Vec<_> = part
        .faces
        .iter()
        .enumerate()
        .filter_map(|(f, face)| {
            let b = part.face_bounds(f);
            (matches!(face.surface, Surface::Plane { .. })
                && (b.min[2] - 8.).abs() < 1e-7
                && (b.max[2] - 8.).abs() < 1e-7
                && b.min[0] < x
                && b.max[0] > x)
                .then_some(f)
        })
        .collect();
    assert_eq!(floors.len(), 1, "floor at x={x}: {floors:?}");
    floors[0]
}

#[test]
fn flat_blind_hole_claims_its_closed_floor() {
    let part = controls();
    let floor = floor_at(&part, 12.);
    let found = features::recognise(&part);
    let (record, faces) = found
        .holes
        .iter()
        .zip(&found.defining["holes"])
        .find(|(h, _)| h.diameter == 8.)
        .unwrap();
    assert_eq!(record.bottom, holes::Bottom::Flat);
    assert!(faces.contains(&floor));
    assert_eq!(faces.len(), 2);
    assert!(
        found.defining["step_levels"]
            .iter()
            .any(|faces| faces.contains(&floor)),
        "step-level evidence is retained alongside the owned floor"
    );
}

#[test]
fn blind_hole_on_a_shared_pocket_floor_keeps_it_as_context() {
    let part = controls();
    let floor = floor_at(&part, 38.);
    let discovered = holes::discover_verified(&Context::new(&part), &[]).unwrap();
    let hole = discovered
        .iter()
        .find(|h| h.record.diameter == 10.)
        .unwrap();
    assert_eq!(hole.record.bottom, holes::Bottom::Flat);
    assert!(hole.context.contains(&floor));
    let found = features::recognise(&part);
    let (record, faces) = found
        .holes
        .iter()
        .zip(&found.defining["holes"])
        .find(|(h, _)| h.diameter == 10.)
        .unwrap();
    assert_eq!(record.bottom, holes::Bottom::Flat);
    assert!(!faces.contains(&floor));
    assert_eq!(faces, &hole.defining);
}

#[test]
fn closed_blind_pocket_claims_its_floor() {
    let part = controls();
    let floor = floor_at(&part, 74.);
    let found = features::recognise(&part);
    let claims: Vec<_> = ["pockets", "prismatic_pockets"]
        .into_iter()
        .flat_map(|family| {
            found.defining[family]
                .iter()
                .map(move |faces| (family, faces))
        })
        .filter(|(_, faces)| faces.contains(&floor))
        .collect();
    assert_eq!(
        claims.len(),
        1,
        "only the first individual record may own the floor"
    );
    assert_eq!(claims[0].0, "pockets", "pockets precede prismatic pockets");
    assert_eq!(claims[0].1.len(), 5);
}

#[test]
fn pocket_continuing_into_a_slot_does_not_claim_the_shared_floor() {
    let part = controls();
    let floor = floor_at(&part, 109.);
    let discovered = pockets::discover_verified(&Context::new(&part)).unwrap();
    assert!(
        discovered
            .iter()
            .any(|p| p.record.width == 18. && p.record.length == 28. && p.context.contains(&floor)),
        "the main pocket sharing its floor with the slot must be recognised"
    );
    let found = features::recognise(&part);
    assert!(
        found
            .pockets
            .iter()
            .any(|p| p.width == 18. && p.length == 28.)
    );
    for family in ["pockets", "prismatic_pockets"] {
        assert!(
            found.defining[family]
                .iter()
                .all(|faces| !faces.contains(&floor)),
            "{family}"
        );
    }
}

#[test]
fn flat_floor_first_owner_wins_even_for_identical_walls_under_motion() {
    let base = controls();
    let floor = floor_at(&base, 12.);
    let (s, c) = 0.37_f64.sin_cos();
    for at in [
        IDENTITY,
        [[c, 0., s, 123.456], [0., 1., 0., -78.9], [-s, 0., c, 41.3]],
    ] {
        let part = base.transformed(&at).unwrap();
        let hole = holes::discover_verified(&Context::new(&part), &[])
            .unwrap()
            .into_iter()
            .find(|h| h.record.diameter == 8.)
            .unwrap();
        let mut faces = hole.defining.clone();
        let mut occupied = [floor].into_iter().collect();
        let mut floor_owners = std::collections::BTreeSet::new();
        holes::claim_flat_floor(
            &part,
            hole.record.axis,
            &mut faces,
            &hole.context,
            &occupied,
            &mut floor_owners,
        );
        assert_eq!(faces, hole.defining);
        occupied.clear();
        holes::claim_flat_floor(
            &part,
            hole.record.axis,
            &mut faces,
            &hole.context,
            &occupied,
            &mut floor_owners,
        );
        assert!(faces.contains(&floor));
        assert_eq!(floor_owners.iter().copied().collect::<Vec<_>>(), [floor]);
        let mut duplicate = hole.defining.clone();
        holes::claim_flat_floor(
            &part,
            hole.record.axis,
            &mut duplicate,
            &hole.context,
            &occupied,
            &mut floor_owners,
        );
        assert_eq!(
            duplicate, hole.defining,
            "the first owner wins even for identical walls"
        );
        assert!(faces.contains(&floor), "the first owner retains the floor");
        let mut competing = hole.defining.clone();
        competing.push(
            (0..part.faces.len())
                .find(|&f| {
                    !faces.contains(&f)
                        && matches!(part.faces[f].surface, Surface::Plane { .. })
                        && quiddity::features::graph::normal(&part, f)
                            .is_some_and(|n| geom::dot(n, hole.record.axis).abs() < 1e-9)
                        && part.face_edges(f).into_iter().all(|e| {
                            [part.edges[e].start, part.edges[e].end].iter().all(|&p| {
                                let depth = geom::dot(
                                    geom::sub(p, *hole.record.location.raw()),
                                    hole.record.axis,
                                );
                                (-1e-7..=hole.record.depth + 1e-7).contains(&depth)
                            })
                        })
                })
                .unwrap(),
        );
        let original = competing.clone();
        holes::claim_flat_floor(
            &part,
            hole.record.axis,
            &mut competing,
            &hole.context,
            &occupied,
            &mut floor_owners,
        );
        assert!(!competing.contains(&floor));
        assert_eq!(competing, original);
        let n = quiddity::features::graph::normal(&part, floor).unwrap();
        assert!(geom::norm(geom::cross(n, hole.record.axis)) < 1e-9);
    }
}

#[test]
fn flat_floor_evidence_and_body_coowners_are_explicit_and_exhaustive() {
    use std::collections::{BTreeMap, BTreeSet};
    let part = controls();
    let floor = floor_at(&part, 12.);
    let hole = holes::discover_verified(&Context::new(&part), &[])
        .unwrap()
        .into_iter()
        .find(|h| h.record.diameter == 8.)
        .unwrap();
    let coowners = [
        "step_levels",
        "risers",
        "thin_wall_bodies",
        "sheet_metal_bodies",
        "plates",
        "polygonal_stock",
        "turned_bodies",
    ];
    let families: BTreeSet<_> = quiddity::recognition::RECORD_TYPES
        .iter()
        .map(|(f, _)| *f)
        .chain(coowners)
        .chain(["unknown_family"])
        .collect();
    for family in families {
        let defining = BTreeMap::from([(family, vec![vec![floor]])]);
        let occupied = features::floor_claims_occupied(&defining);
        let mut faces = hole.defining.clone();
        holes::claim_flat_floor(
            &part,
            hole.record.axis,
            &mut faces,
            &hole.context,
            &occupied,
            &mut BTreeSet::new(),
        );
        assert_eq!(
            faces.contains(&floor),
            coowners.contains(&family),
            "{family}"
        );
        assert_eq!(
            defining[family],
            [vec![floor]],
            "existing evidence is retained"
        );
    }
}

#[test]
fn mfinstseg_flat_floor_spots() {
    let Some(root) = std::env::var_os("QUIDDITY_MFINSTSEG") else {
        return;
    };
    let root = std::path::Path::new(&root).join("steps");
    for (id, floor) in [
        ("20221121_154647_100", 13),
        ("20221121_154647_0", 39),
        ("20221121_154647_10", 40),
    ] {
        let part = quiddity::read_step_file(&root.join(format!("{id}.step"))).unwrap();
        let found = features::recognise(&part);
        let owners: Vec<_> = found
            .holes
            .iter()
            .zip(&found.defining["holes"])
            .filter(|(_, faces)| faces.contains(&floor))
            .collect();
        assert_eq!(owners.len(), 1, "{id}: floor {floor}");
        assert_eq!(owners[0].0.bottom, holes::Bottom::Flat);
    }
    // A whole-body plate description is nonexclusive, like level evidence.
    let part = quiddity::read_step_file(&root.join("20221121_154648_10415.step")).unwrap();
    let before = features::inventory(&part).unwrap().accepted();
    assert!(before.defining["plates"].iter().any(|f| f.contains(&28)));
    let found = features::recognise(&part);
    assert_eq!(found.defining["plates"], before.defining["plates"]);
    assert!(found.defining["holes"].iter().any(|f| f.contains(&28)));
    // Accepted wall sets in the stride-five coverage sample, one per pocket class.
    for (id, family, faces) in [
        (
            "20221121_154647_1034",
            "prismatic_pockets",
            &[7, 8, 9, 18][..],
        ),
        ("20221121_154647_0", "pockets", &[26, 27, 28, 29, 38][..]),
        (
            "20221121_154647_0",
            "prismatic_pockets",
            &[32, 33, 34, 35, 36, 37, 40][..],
        ),
        ("20221121_154647_1034", "pockets", &[10, 11, 12, 13, 19][..]),
    ] {
        let part = quiddity::read_step_file(&root.join(format!("{id}.step"))).unwrap();
        let found = features::recognise(&part);
        assert!(
            found.defining[family]
                .iter()
                .any(|own| faces.iter().all(|f| own.contains(f))),
            "{id}: {family} does not claim {faces:?}"
        );
    }
}
