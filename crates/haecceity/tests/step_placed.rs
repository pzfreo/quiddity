//! Selected placed bodies keep whole-file source identity, geometry and topology (#111).
//! Inputs are existing STEP fixtures; the coincident-pin case changes only one placement.

mod common;

use std::collections::{HashMap, HashSet};

use haecceity::Part;
use haecceity::step::{StepError, StepFile, read_step};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(common::fixtures().join(name)).unwrap()
}

/// Compare with the full read by source identity, allowing only local index renumbering.
/// In particular, identical geometry in a dropped instance cannot satisfy a source match.
fn check_selection(whole: &Part, selected: &Part, keep: &[usize]) {
    let faces: Vec<_> = (0..whole.faces.len())
        .filter(|&f| keep.contains(&whole.face_source(f).unwrap().instance))
        .collect();
    let edges: Vec<_> = (0..whole.edges.len())
        .filter(|&e| keep.contains(&whole.edge_source(e).unwrap().instance))
        .collect();
    assert_eq!(selected.faces.len(), faces.len());
    assert_eq!(selected.edges.len(), edges.len());
    let mut vertices = HashMap::new();
    for (e, &original) in edges.iter().enumerate() {
        assert_eq!(selected.edge_source(e), whole.edge_source(original));
        let (a, b) = (&selected.edges[e], &whole.edges[original]);
        assert_eq!(a.start, b.start);
        assert_eq!(a.end, b.end);
        assert_eq!(a.same_sense, b.same_sense);
        assert_eq!(a.samples, b.samples);
        assert_eq!(format!("{:?}", a.curve), format!("{:?}", b.curve));
        for (new, old) in [(a.vertices.0, b.vertices.0), (a.vertices.1, b.vertices.1)] {
            if let Some(previous) = vertices.insert(new, old) {
                assert_eq!(previous, old, "shared vertex identity changed");
            }
        }
        assert_eq!(
            selected.unresolved_edges().contains(&e),
            whole.unresolved_edges().contains(&original)
        );
        let neighbours = |part: &Part, edge: usize| {
            part.edge_faces()[edge]
                .iter()
                .map(|&face| part.face_source(face).unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(neighbours(selected, e), neighbours(whole, original));
    }
    assert_eq!(
        vertices.values().collect::<HashSet<_>>().len(),
        vertices.len()
    );
    for (f, &original) in faces.iter().enumerate() {
        assert_eq!(selected.face_source(f), whole.face_source(original));
        let (a, b) = (&selected.faces[f], &whole.faces[original]);
        assert_eq!(a.reversed, b.reversed);
        assert_eq!(format!("{:?}", a.surface), format!("{:?}", b.surface));
        assert_eq!(a.loops.len(), b.loops.len());
        for (a, b) in a.loops.iter().zip(&b.loops) {
            assert_eq!(a.vertex, b.vertex);
            let sources = |part: &Part, edges: &[(usize, bool)]| {
                edges
                    .iter()
                    .map(|&(e, forward)| (part.edge_source(e).unwrap(), forward))
                    .collect::<Vec<_>>()
            };
            assert_eq!(sources(selected, &a.edges), sources(whole, &b.edges));
        }
        assert_eq!(a.pcurves.len(), b.pcurves.len());
        for ((ae, ac), (be, bc)) in a.pcurves.iter().zip(&b.pcurves) {
            assert_eq!(selected.edge_source(*ae), whole.edge_source(*be));
            assert_eq!(format!("{ac:?}"), format!("{bc:?}"));
        }
        assert_eq!(
            selected.unresolved_faces().contains(&f),
            whole.unresolved_faces().contains(&original)
        );
        match (selected.face_moments(f), whole.face_moments(original)) {
            (Some(a), Some(b)) => {
                assert!((a.area - b.area).abs() <= 1e-9 * b.area.abs().max(1.0));
                for k in 0..3 {
                    assert!((a.centroid[k] - b.centroid[k]).abs() < 1e-6);
                }
            }
            (None, None) => {}
            other => panic!("selected face moments differ: {other:?}"),
        }
        match (a.solid, b.solid) {
            (Some(new), Some(old)) => {
                assert_eq!(selected.solid_is_valid(new), whole.solid_is_valid(old));
                let sources = |part: &Part, solid: usize| {
                    part.solids[solid]
                        .faces
                        .iter()
                        .map(|&face| part.face_source(face).unwrap())
                        .collect::<Vec<_>>()
                };
                assert_eq!(sources(selected, new), sources(whole, old));
                assert!(selected.solids[new].faces.contains(&f));
            }
            (None, None) => {}
            other => panic!("solid ownership differs: {other:?}"),
        }
    }
    let kept_solids = whole
        .solids
        .iter()
        .filter(|solid| keep.contains(&whole.face_source(solid.faces[0]).unwrap().instance))
        .count();
    assert_eq!(selected.solids.len(), kept_solids);
    assert!(
        selected
            .unresolved_faces()
            .iter()
            .all(|&f| f < selected.faces.len())
    );
    assert!(
        selected
            .unresolved_edges()
            .iter()
            .all(|&e| e < selected.edges.len())
    );
}

#[test]
fn plate_and_one_coincident_pin_keep_the_chosen_instance() {
    let text = String::from_utf8(fixture("ap242/assembly/assembly.step")).unwrap();
    let from = "#1269 = ITEM_DEFINED_TRANSFORMATION('','',#11,#23);";
    assert_eq!(text.matches(from).count(), 1);
    let text = text.replace(from, "#1269 = ITEM_DEFINED_TRANSFORMATION('','',#11,#19);");
    let file = StepFile::read(text.as_bytes()).unwrap();
    let whole = read_step(text.as_bytes()).unwrap();
    assert_eq!(whole.solids.len(), 3);
    let pin = &file.parts[1];
    assert_eq!(pin.placements[0].placement, pin.placements[1].placement);
    assert_eq!(pin.placements[0].instances, [1]);
    assert_eq!(pin.placements[1].instances, [2]);
    // Keep the second copy so a dense renumbering (2 -> 1) would fail.
    for keep in [[0, 2], [0, 1]] {
        let selected = file.placed(&keep).unwrap();
        assert_eq!(selected.solids.len(), 2);
        assert_eq!(
            selected.faces.len(),
            file.parts[0].faces.len() + pin.faces.len()
        );
        assert!(
            selected
                .solids
                .iter()
                .enumerate()
                .all(|(s, _)| selected.solid_is_valid(s))
        );
        check_selection(&whole, &selected, &keep);
    }
}

#[test]
fn selection_preserves_rotated_and_nested_placements() {
    for name in ["ap242/assembly/assembly.step", "ap242/assembly/nested.step"] {
        let bytes = fixture(name);
        let file = StepFile::read(&bytes).unwrap();
        let whole = read_step(&bytes).unwrap();
        for keep in [&[2][..], &[0, 2], &[1, 2], &[0, 1, 2]] {
            check_selection(&whole, &file.placed(keep).unwrap(), keep);
        }
    }
}

#[test]
fn selection_is_a_set_and_does_not_change_other_reads() {
    let bytes = fixture("ap242/assembly/assembly.step");
    let file = StepFile::read(&bytes).unwrap();
    let whole = read_step(&bytes).unwrap();
    let own = file.part(1).unwrap();
    check_selection(&whole, &file.placed(&[2, 0, 2, 0]).unwrap(), &[0, 2]);
    check_selection(&whole, &file.placed(&[2, 1, 0]).unwrap(), &[0, 1, 2]);
    // Local part geometry stays unplaced, with its original first-placement sources.
    check_selection(&own, &file.part(1).unwrap(), &[1]);
    check_selection(&whole, &read_step(&bytes).unwrap(), &[0, 1, 2]);
}

#[test]
fn empty_or_unknown_instance_selection_is_refused() {
    let file = StepFile::read(&fixture("ap242/assembly/assembly.step")).unwrap();
    for keep in [&[][..], &[3], &[0, 3], &[usize::MAX]] {
        let err = file.placed(keep).unwrap_err();
        assert!(matches!(err, StepError::Unsupported(_)), "{err}");
        assert!(err.to_string().contains(if keep.is_empty() {
            "no placed body instances selected"
        } else {
            "out of range"
        }));
    }
    assert_eq!(
        file.placed(&[2]).unwrap().faces.len(),
        file.parts[1].faces.len()
    );
}

#[test]
fn unresolved_indices_are_rebuilt_for_a_later_selected_instance() {
    let mut text = String::from_utf8(fixture("ap242/assembly/assembly.step")).unwrap();
    // As in the existing broken fixtures: an unresolved offset surface and an open edge
    // whose stated hyperbola does not contain its endpoints. Both occur on each pin.
    for (from, to) in [
        (
            "#1236 = PLANE('',#1237);",
            "#1236 = OFFSET_SURFACE('',#1300,0.,.F.);\n#1300 = PLANE('',#1237);",
        ),
        (
            "#1022 = LINE('',#1023,#1024);",
            "#1022 = HYPERBOLA('',#995,10.,5.);",
        ),
    ] {
        assert_eq!(text.matches(from).count(), 1);
        text = text.replace(from, to);
    }
    let file = StepFile::read(text.as_bytes()).unwrap();
    let whole = read_step(text.as_bytes()).unwrap();
    let selected = file.placed(&[2]).unwrap();
    assert_eq!(whole.unresolved_faces().len(), 2);
    assert_eq!(selected.unresolved_faces().len(), 1);
    assert_eq!(whole.unresolved_edges().len(), 2);
    assert_eq!(selected.unresolved_edges().len(), 1);
    assert!(selected.unresolved_faces()[0] < whole.unresolved_faces()[1]);
    assert!(selected.unresolved_edges()[0] < whole.unresolved_edges()[1]);
    check_selection(&whole, &selected, &[2]);
}

#[test]
fn surface_models_and_unresolved_geometry_keep_their_sources() {
    for name in [
        "open_shell.step",
        "broken/unresolved_surface.step",
        "broken/unresolved_open_curve.step",
    ] {
        let bytes = fixture(name);
        let file = StepFile::read(&bytes).unwrap();
        let whole = read_step(&bytes).unwrap();
        let keep: HashSet<_> = (0..whole.faces.len())
            .map(|f| whole.face_source(f).unwrap().instance)
            .collect();
        let keep: Vec<_> = keep.into_iter().collect();
        assert!(!keep.is_empty());
        check_selection(&whole, &file.placed(&keep).unwrap(), &keep);
        if name.contains("unresolved_surface") {
            assert!(!whole.unresolved_faces().is_empty());
        } else if name.contains("unresolved_open_curve") {
            assert!(!whole.unresolved_edges().is_empty());
        } else {
            assert!(whole.solids.is_empty());
        }
    }
}
