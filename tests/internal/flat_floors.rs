//! Original boolean-cut controls, independent of the output snapshots.
use super::common;
use quiddity::features::{self, Context, holes, pockets};
use quiddity::kernel::geom::{self, Surface};
use quiddity::kernel::step::IDENTITY;

fn controls() -> quiddity::Part {
    quiddity::read_step_file(&common::fixtures().join("flat_floor_controls.step.gz")).unwrap()
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
        .flat_map(|family| found.defining[family].iter())
        .filter(|faces| faces.contains(&floor))
        .collect();
    assert!(!claims.is_empty());
    for faces in &claims {
        assert_eq!(faces.len(), 5);
        assert_eq!(
            faces, &claims[0],
            "equivalent descriptions share the same closed floor"
        );
    }
}

#[test]
fn pocket_continuing_into_a_slot_does_not_claim_the_shared_floor() {
    let part = controls();
    let floor = floor_at(&part, 109.);
    let discovered = pockets::discover_verified(&Context::new(&part)).unwrap();
    assert!(
        discovered.iter().any(|p| p.context.contains(&floor)),
        "the shared floor must be consulted by a recognised pocket"
    );
    let found = features::recognise(&part);
    assert!(!found.pockets.is_empty());
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
fn existing_owner_and_rigid_motion_preserve_floor_ownership() {
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
        let mut floor_owners = std::collections::BTreeMap::new();
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
        assert_eq!(floor_owners.keys().copied().collect::<Vec<_>>(), [floor]);
        let mut duplicate = hole.defining.clone();
        holes::claim_flat_floor(
            &part,
            hole.record.axis,
            &mut duplicate,
            &hole.context,
            &occupied,
            &mut floor_owners,
        );
        assert_eq!(duplicate, faces);
        let mut competing = hole.defining.clone();
        competing.push(
            (0..part.faces.len())
                .find(|f| *f != floor && !faces.contains(f))
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
        assert_eq!(competing.len(), original.len());
        let n = quiddity::features::graph::normal(&part, floor).unwrap();
        assert!(geom::norm(geom::cross(n, hole.record.axis)) < 1e-9);
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
        ("20221121_154647_1007", "pockets", &[19, 20, 21, 22, 26][..]),
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
