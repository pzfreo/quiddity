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

/// An MFCAD++ part whose section has several oblique sides: the big slanted face stays the
/// angled step it is published as, and is no chamfer candidate (#301).
#[test]
fn corpus_polygon_side_is_no_chamfer_but_stays_an_angled_step() {
    use quiddity::features;
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let p = quiddity::read_step_file(&dir.join("mfcadpp/10138.step")).unwrap();
    assert!(
        chamfers::discover_verified(&Context::new(&p), &Default::default())
            .unwrap()
            .is_empty()
    );
    let f = features::recognise(&p);
    assert!(f.chamfers.is_empty());
    assert!(
        f.angled_steps
            .iter()
            .any(|s| (s.leg1, s.leg2) == (18.575, 12.942)),
        "{:?}",
        f.angled_steps
    );
}
