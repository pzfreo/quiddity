//! Round-bottomed (toroidal) grooves: O-ring glands and ball grooves (quiddity#307, item 5).
use super::common;
use quiddity::features::{self, round_grooves::RoundGrooveHost};
use quiddity::framed_records::Rigid;
use quiddity::kernel::step::IDENTITY;

fn load(name: &str) -> quiddity::kernel::brep::Part {
    quiddity::read_step_file(&common::fixtures().join(name)).unwrap()
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-6, "{a} != {b}");
}

/// A torus face of the part, as its index.
fn torus_faces(part: &quiddity::kernel::brep::Part) -> Vec<usize> {
    (0..part.faces.len())
        .filter(|&f| {
            matches!(
                part.faces[f].surface,
                quiddity::kernel::geom::Surface::Torus { .. }
            )
        })
        .collect()
}

#[test]
fn an_o_ring_gland_on_a_shaft_is_a_round_groove_in_any_placement() {
    let base = load("round_groove_gland_shaft.step.gz");
    let (s, c) = 0.61_f64.sin_cos();
    for placement in [
        IDENTITY,
        [[c, 0., s, 12.5], [0., 1., 0., -7.25], [-s, 0., c, 31.]],
    ] {
        let motion = Rigid::from_placement(&placement);
        let part = base.transformed(&placement).unwrap();
        let found = features::recognise(&part);
        let [g] = &found.round_grooves[..] else {
            panic!("{:?}", found.round_grooves)
        };
        assert_eq!(g.host, RoundGrooveHost::Shaft);
        close(g.ring_diameter, 57.);
        close(g.bottom_radius, 1.3);
        // The mouth meets the R29 shaft where the torus is 0.5 above its centre circle.
        close(g.width, 2. * (1.3_f64 * 1.3 - 0.5 * 0.5).sqrt());
        close(g.depth, 29. - (28.5 - 1.3));
        close(g.host_diameter.unwrap(), 58.);
        assert_eq!(g.faces, torus_faces(&part));
        let on_axis = motion.point([0., 0., 15.]);
        for (c, e) in g.center.iter().zip(on_axis) {
            close(*c, e);
        }
        // The lands are consulted, never defining.
        assert_eq!(g.host_faces.len(), 2);
        assert!(g.host_faces.iter().all(|f| !g.faces.contains(f)));
    }
}

#[test]
fn a_ball_groove_in_a_bore_is_one_record_whatever_the_bore_records_are() {
    // The race: tube r13.6..15.0, z -3.5..1.5, minus a torus R13.0 r1.3 at z=-1.
    let part = load("round_groove_ball_race.step.gz");
    let found = features::recognise(&part);
    let [g] = &found.round_grooves[..] else {
        panic!("{:?}", found.round_grooves)
    };
    assert_eq!(g.host, RoundGrooveHost::Bore);
    close(g.ring_diameter, 26.);
    close(g.bottom_radius, 1.3);
    close(g.width, 2. * (1.3_f64 * 1.3 - 0.6 * 0.6).sqrt());
    close(g.depth, 13.0 + 1.3 - 13.6);
    close(g.host_diameter.unwrap(), 27.2);
    assert_eq!(g.faces, torus_faces(&part));
    // The two bore lands stay with the hole records: the groove claims only the torus.
    let claimed = |family: &str| -> Vec<usize> {
        found
            .defining
            .get(family)
            .map(|r| r.iter().flatten().copied().collect())
            .unwrap_or_default()
    };
    assert!(claimed("holes").iter().all(|f| !g.faces.contains(f)));
    assert!(g.host_faces.iter().all(|f| !g.faces.contains(f)));
    assert_eq!(claimed("round_grooves"), g.faces);
}

#[test]
fn face_o_ring_glands_are_round_grooves_of_the_face_host() {
    let part = load("round_groove_face_glands.step.gz");
    let found = features::recognise(&part);
    assert_eq!(found.round_grooves.len(), 2);
    for (g, ring) in found.round_grooves.iter().zip([16.4, 13.448]) {
        assert_eq!(g.host, RoundGrooveHost::Face);
        close(g.ring_diameter, ring);
        close(g.bottom_radius, 0.7);
        // The gland is centred on the face plane: a half section.
        close(g.width, 1.4);
        close(g.depth, 0.7);
        assert!(g.host_diameter.is_none());
        assert_eq!(g.axis, [0., 0., 1.]);
    }
}

#[test]
fn a_filleted_step_and_a_bead_are_not_grooves() {
    // A tangent R3 fillet and a convex R2 bead are torus faces, but neither is cut into a host.
    let part = load("round_groove_controls.step.gz");
    assert!(!torus_faces(&part).is_empty());
    assert!(features::recognise(&part).round_grooves.is_empty());
}
