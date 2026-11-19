//! Chamfers on prismatic sections, for #301. The STEP files are from springcraft-ai's
//! brep-conformance 1.0.0 (MIT, OpenCascade 8.0.1 output); each file's case parameters are
//! its expected chamfer distances.
use super::common;
use quiddity::features::{Context, chamfers};
use quiddity::kernel::brep::Part;

fn part(name: &str) -> Part {
    quiddity::read_step_file(&common::fixtures().join("brep-conformance").join(name)).unwrap()
}

fn planar_chamfers(name: &str) -> Vec<(f64, f64, f64)> {
    chamfers::discover_verified(&Context::new(&part(name)), &Default::default())
        .unwrap()
        .into_iter()
        .filter(|o| !o.record.turned)
        .map(|o| (o.record.leg1, o.record.leg2, o.record.angle))
        .collect()
}

/// The flats of a hexagonal or polygonal prism lie between two walls that happen to be on
/// principal planes, but they are not chamfers of those walls: the walls run on into oblique
/// flats. The expected 120-degree vertex chamfers (3 x 0.524, 2 x 2.167 and 1.601 / 0.823 in
/// these files) are not claimed either, so no record is published, instead of legs such as
/// 2.517 / 1.453 at 30 degrees from the middle of a flat.
#[test]
fn polygon_flats_are_not_chamfers_of_each_other() {
    for name in [
        "prisms_hexagon_3_chamfers.step.gz",
        "prisms_hexagon_big_chamfers.step.gz",
        "uneven_octagon_one_chamfer.step.gz",
    ] {
        assert_eq!(planar_chamfers(name), [], "{name}");
    }
}

/// Chamfers of the corners of a rectilinear section keep their legs, including big ones that
/// leave only a short wall between two chamfers, and an L section with a concave corner
/// bevel beside the walls.
#[test]
fn chamfered_rectilinear_corners_keep_their_legs() {
    for (name, legs, count) in [
        ("prisms_box_3_chamfers.step.gz", 1.769, 3),
        ("prisms_box_4_chamfers.step.gz", 2.175, 4),
        ("prisms_l_section_chamfers.step.gz", 0.966, 4),
    ] {
        let found = planar_chamfers(name);
        assert_eq!(found.len(), count, "{name}: {found:?}");
        for (leg1, leg2, angle) in found {
            assert_eq!((leg1, leg2, angle), (legs, legs, 45.), "{name}");
        }
    }
}

/// An edge-break cone on a conical body's rim is a chamfer measured along the end plane and
/// along the body's cone (a base on a pointed cone, either end of a frustum), and the turned
/// body still owns the same cone face, so the two records are linked through it (#298).
#[test]
fn conical_body_rim_chamfers_are_published_and_linked_to_the_turned_body() {
    use quiddity::features;
    for (name, legs, count) in [
        ("rims_cone_apex_base_chamfer.step.gz", 0.69, 1),
        ("rims_frustum_one_chamfer.step.gz", 0.623, 1),
        ("rims_frustum_two_chamfers.step.gz", 0.651, 2),
        ("rims_cylinder_two_chamfers.step.gz", 0.799, 2),
    ] {
        let p = part(name);
        let f = features::recognise(&p);
        assert_eq!(f.chamfers.len(), count, "{name}: {:?}", f.chamfers);
        assert_eq!(f.turned_bodies.len(), 1, "{name}");
        for (i, c) in f.chamfers.iter().enumerate() {
            assert_eq!(
                (c.leg1, c.leg2, c.angle, c.turned),
                (legs, legs, 45., true),
                "{name}"
            );
            let face = f.defining["chamfers"][i][0];
            assert!(
                f.turned_bodies[0].profile.iter().any(|s| s.face == face),
                "{name}: the profile keeps the cone"
            );
        }
        let doc = quiddity::recognition::document(
            &quiddity::correspondence::recognise(&p),
            p.faces.len(),
        );
        let linked = doc
            .face_accounting
            .unwrap()
            .iter()
            .filter(|a| {
                matches!(a, quiddity::recognition::FaceAccounting::Feature { record_ids }
                    if record_ids.iter().any(|r| r.starts_with("chamfers/"))
                        && record_ids.contains(&"turned_bodies/0".into()))
            })
            .count();
        assert_eq!(linked, count, "{name}");
    }
}

/// A fillet on the same rims is not a chamfer.
#[test]
fn conical_body_rim_fillets_are_not_chamfers() {
    for name in [
        "rims_frustum_fillet.step.gz",
        "rims_cone_apex_fillet.step.gz",
    ] {
        assert!(
            quiddity::features::recognise(&part(name))
                .chamfers
                .is_empty(),
            "{name}"
        );
    }
}

/// A hole's mouth edge break stays nested in the hole's `mouth_chamfers` (#244): size, depth,
/// angle and cone face are right for the brep-conformance hole-rim chamfers, and no standalone
/// chamfer is published for it (#298).
#[test]
fn hole_mouth_chamfers_stay_nested_with_the_right_values() {
    use quiddity::features;
    for (name, size) in [
        ("hole_rim_chamfer_a.step.gz", 1.0914),
        ("hole_rim_chamfer_b.step.gz", 0.542),
    ] {
        let f = features::recognise(&part(name));
        assert_eq!(f.holes.len(), 1, "{name}");
        let [m] = &f.holes[0].mouth_chamfers[..] else {
            panic!("{name}: {:?}", f.holes[0].mouth_chamfers)
        };
        assert!(
            (m.size - size).abs() < 5e-4 && (m.depth - size).abs() < 5e-4 && m.angle == 45.,
            "{name}: {m:?}"
        );
        assert_eq!(m.faces.len(), 1, "{name}");
        assert!(f.chamfers.is_empty(), "{name}");
    }
}

fn edge_break(name: &str) -> quiddity::kernel::brep::Part {
    quiddity::read_step_file(&common::fixtures().join("edge_breaks").join(name)).unwrap()
}

/// A chamfer beside a groove or a slanted step is still a chamfer: the strip it leaves runs on
/// into an oblique wall, but the section is no closed convex polygon (#301 review).
#[test]
fn chamfers_beside_grooves_and_steps_are_kept() {
    for name in ["vslot.step", "slantstep.step"] {
        let found: Vec<_> =
            chamfers::discover_verified(&Context::new(&edge_break(name)), &Default::default())
                .unwrap()
                .into_iter()
                .map(|o| (o.record.leg1, o.record.leg2, o.record.angle))
                .collect();
        assert!(found.contains(&(3., 3., 45.)), "{name}: {found:?}");
        assert_eq!(
            found.len(),
            if name == "vslot.step" { 1 } else { 2 },
            "{name}"
        );
    }
}

/// Two cones of comparable size (or equal ones) are a profile, not an edge break (#298 review).
#[test]
fn cone_to_cone_profiles_have_no_rim_chamfer() {
    for name in [
        "bicone.step",
        "diamond.step",
        "bicone12.step",
        "leadtaper30.step",
    ] {
        let found =
            chamfers::discover_verified(&Context::new(&edge_break(name)), &Default::default())
                .unwrap();
        assert!(
            found.is_empty(),
            "{name}: {:?}",
            found.iter().map(|o| &o.record).collect::<Vec<_>>()
        );
    }
}

/// A real C x 45 degree chamfer on a frustum's base rim is published with equal legs (#298).
#[test]
fn a_large_45_degree_cone_rim_chamfer_is_kept() {
    let found: Vec<_> = chamfers::discover_verified(
        &Context::new(&edge_break("realchamfer042.step")),
        &Default::default(),
    )
    .unwrap()
    .into_iter()
    .map(|o| {
        (
            o.record.leg1,
            o.record.leg2,
            o.record.angle,
            o.record.turned,
        )
    })
    .collect();
    assert_eq!(found, [(4.2, 4.2, 45., true)]);
}
