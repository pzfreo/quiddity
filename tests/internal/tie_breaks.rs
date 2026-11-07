//! ADR 0011: topology numbering and support parametrisation cannot establish machining roles.
use super::common;
use quiddity::features::{self, Context};
use quiddity::kernel::step::{IDENTITY, Placement, read_step_file_placed};

fn motions() -> Vec<Placement> {
    let mut out = vec![
        IDENTITY,
        [
            [1., 0., 0., 123.456],
            [0., 1., 0., -78.9],
            [0., 0., 1., 41.3],
        ],
    ];
    for (axis, degrees) in [(2, 30_f64), (0, 30.), (0, 90.), (1, 180.)] {
        let (s, c) = degrees.to_radians().sin_cos();
        let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
        let mut p = IDENTITY;
        p[a][a] = c;
        p[a][b] = -s;
        p[b][a] = s;
        p[b][b] = c;
        out.push(p);
    }
    out
}

fn bore_ends(h: &features::holes::HoleRecord) -> [[f64; 3]; 2] {
    let offset = h
        .cbore
        .iter()
        .chain(&h.spotface)
        .chain(&h.outer_steps)
        .map(|s| s.depth)
        .sum::<f64>();
    [offset, offset + h.depth]
        .map(|s| quiddity::kernel::geom::add(*h.location, quiddity::kernel::geom::scale(h.axis, s)))
}

#[test]
fn symmetric_double_mouth_keeps_the_bore_and_both_counterbores_under_motion() {
    let path = common::fixtures().join("ties/symmetric_double_mouth.step");
    let mut claims = None;
    for at in motions() {
        let part = read_step_file_placed(&path, &at).unwrap();
        let found = features::holes::discover(&Context::new(&part), &[]);
        assert_eq!(found.len(), 1, "{at:?}: {found:?}");
        let h = &found[0].record;
        assert_eq!(
            (h.diameter, h.depth, h.bottom),
            (6., 18., features::holes::Bottom::Through)
        );
        assert_eq!(
            h.cbore,
            Some(features::holes::CounterBore {
                diameter: 12.,
                depth: 6.
            })
        );
        assert!(h.spotface.is_none() && h.csink.is_none());
        let mut faces = found[0].defining.clone();
        faces.sort_unstable();
        assert_eq!(faces.len(), 3, "bore and both enlarged mouth cylinders");
        assert_eq!(claims.get_or_insert_with(|| faces.clone()), &faces);
        let ends = bore_ends(h);
        for z in [-9., 9.] {
            let expected = [0, 1, 2].map(|i| at[i][3] + z * at[i][2]);
            assert!(
                ends.iter()
                    .any(|p| quiddity::kernel::py::dist(p, &expected) < 1e-6)
            );
        }
    }
}

#[test]
fn equal_mouths_choose_by_inward_geometry_under_motion() {
    let path = common::fixtures().join("ties/asymmetric_double_mouth.step");
    let mut faces = None;
    for at in motions() {
        let part = read_step_file_placed(&path, &at).unwrap();
        let found = features::holes::discover(&Context::new(&part), &[]);
        assert_eq!(found.len(), 1, "{at:?}");
        let hole = &found[0].record;
        assert_eq!(hole.cbore.as_ref().unwrap().depth, 8.0);
        assert_eq!(hole.cbore.as_ref().unwrap().diameter, 12.0);
        assert_eq!(hole.diameter, 6.0);
        let expected = [
            at[0][3] + 15. * at[0][2],
            at[1][3] + 15. * at[1][2],
            at[2][3] + 15. * at[2][2],
        ];
        assert!(
            quiddity::kernel::py::dist(&hole.location[..], &expected) < 1e-6,
            "{hole:?}"
        );
        let mut got = found[0].defining.clone();
        got.sort_unstable();
        assert_eq!(faces.get_or_insert_with(|| got.clone()), &got);
    }
}

#[test]
fn symmetric_square_pocket_refuses_a_semantic_run_axis() {
    let path = common::fixtures().join("ties/square_pocket.step");
    // Principal-axis motions preserve this family's explicit axis-letter contract.
    for at in motions().into_iter().filter(|p| {
        p.iter()
            .flatten()
            .all(|v| v.abs() < 1e-12 || (v.abs() - 1.).abs() < 1e-12)
            || *p == motions()[1]
    }) {
        let part = read_step_file_placed(&path, &at).unwrap();
        let found = features::pockets::discover(&Context::new(&part));
        assert!(found.is_empty(), "{at:?}: {found:?}");
    }
}

#[test]
fn crossing_equal_patterns_refuse_under_motion_and_member_permutation() {
    use features::holes::{Bottom, HoleRecord};
    for at in motions() {
        let mut holes: Vec<_> = [
            [0., 0., 0.],
            [10., 0., 0.],
            [20., 0., 0.],
            [0., 10., 0.],
            [0., 20., 0.],
        ]
        .map(|point| HoleRecord {
            superseded_by: None,
            axis: [at[0][2], at[1][2], at[2][2]],
            location: [0, 1, 2]
                .map(|i| at[i][3] + (0..3).map(|j| at[i][j] * point[j]).sum::<f64>())
                .into(),
            diameter: 2.,
            depth: 5.,
            bottom: Bottom::Through,
            cbore: None,
            spotface: None,
            outer_steps: Vec::new(),
            exit_obstructions: Vec::new(),
            csink: None,
        })
        .into();
        for _ in 0..holes.len() {
            assert!(
                features::hole_patterns::recognise_hole_patterns(&holes).is_empty(),
                "{at:?}"
            );
            holes.rotate_left(1);
        }
        holes.reverse();
        assert!(features::hole_patterns::recognise_hole_patterns(&holes).is_empty());
    }
}

#[test]
fn equidistant_support_planes_refuse_under_principal_motion() {
    let path = common::fixtures().join("ties/square_pocket.step");
    for at in [motions()[0], motions()[1], motions()[4], motions()[5]] {
        let part = read_step_file_placed(&path, &at).unwrap();
        // The intact bottom is bounded by four equally distant side planes.
        let face = (0..part.faces.len())
            .find(|&f| (part.face_mass(f).unwrap()[0] - 1600.).abs() < 1e-6)
            .unwrap();
        let (axis, _) = features::planes::axis_aligned_axis(&part, face).unwrap();
        let centre = part.face_bounds(face).centre();
        for legacy_flag in [false, true] {
            let got = features::planes::nearest_axis_aligned_planes(
                &part,
                face,
                centre,
                axis,
                legacy_flag,
            );
            assert!(got.is_empty(), "{at:?}: {got:?}");
        }
    }
}

#[test]
fn equally_supported_turning_axes_refuse_under_principal_motion() {
    let path = common::fixtures().join("ties/equal_axis_shafts.step");
    for at in [motions()[0], motions()[1], motions()[4], motions()[5]] {
        let part = read_step_file_placed(&path, &at).unwrap();
        let ctx = Context::new(&part);
        let supported = features::cylinders::full_cylinders(ctx.cylinders());
        let mut counts = std::collections::BTreeMap::new();
        for c in supported {
            *counts.entry(c.axis).or_insert(0) += 1;
        }
        assert_eq!(counts.len(), 2, "{counts:?}");
        assert_eq!(
            counts.values().next(),
            counts.values().next_back(),
            "{counts:?}"
        );
        assert!(features::turned_steps::discover(&ctx).is_empty(), "{at:?}");
    }
}

#[test]
fn equal_pitch_grid_uses_population_to_assign_rows_under_motion() {
    use features::hole_patterns::HolePattern;
    use features::holes::{Bottom, HoleRecord};
    for at in motions() {
        let mut holes: Vec<_> = (0..3)
            .flat_map(|x| (0..4).map(move |y| [10. * x as f64, 10. * y as f64, 0.]))
            .map(|p| HoleRecord {
                superseded_by: None,
                axis: [at[0][2], at[1][2], at[2][2]],
                location: [0, 1, 2]
                    .map(|i| at[i][3] + (0..3).map(|j| at[i][j] * p[j]).sum::<f64>())
                    .into(),
                diameter: 2.,
                depth: 5.,
                bottom: Bottom::Through,
                cbore: None,
                spotface: None,
                outer_steps: Vec::new(),
                exit_obstructions: Vec::new(),
                csink: None,
            })
            .collect();
        for _ in 0..holes.len() {
            let patterns = features::hole_patterns::recognise_hole_patterns(&holes);
            let [
                HolePattern::RectGrid {
                    rows,
                    cols,
                    row_pitch,
                    col_pitch,
                    ..
                },
            ] = patterns.as_slice()
            else {
                panic!("{at:?}: {patterns:?}")
            };
            assert_eq!((*rows, *cols, *row_pitch, *col_pitch), (3, 4, 10., 10.));
            holes.rotate_left(1);
        }
    }
}

#[test]
fn contradictory_equal_distance_end_supports_report_unknown_under_motion() {
    use features::cylinders::Segment;
    use features::stacks::{End, classify_end};
    use quiddity::kernel::brep::{Edge, Face, Loop, Part};
    use quiddity::kernel::geom::{Curve, Frame, Surface};
    for at in motions() {
        let point = |p: [f64; 3]| {
            [0, 1, 2].map(|i| at[i][3] + (0..3).map(|j| at[i][j] * p[j]).sum::<f64>())
        };
        let dir = |p: [f64; 3]| [0, 1, 2].map(|i| (0..3).map(|j| at[i][j] * p[j]).sum::<f64>());
        let frame = Frame {
            origin: point([0.; 3]),
            x: dir([1., 0., 0.]),
            y: dir([0., 1., 0.]),
            z: dir([0., 0., 1.]),
        };
        let edge = Edge {
            curve: Curve::Circle { frame, radius: 2. },
            start: point([2., 0., 0.]),
            end: point([2., 0., 0.]),
            vertices: (0, 0),
            same_sense: true,
            samples: vec![point([2., 0., 0.]), point([-2., 0., 0.])],
        };
        for reverse in [false, true] {
            let face = |surface, reversed| Face {
                surface,
                reversed,
                loops: vec![Loop {
                    edges: vec![(0, true)],
                    vertex: None,
                }],
                solid: None,
                pcurves: vec![],
            };
            let part = Part::new(
                vec![
                    face(Surface::Cylinder { frame, radius: 2. }, false),
                    face(Surface::Plane { frame }, reverse),
                    face(Surface::Plane { frame }, !reverse),
                ],
                vec![edge.clone()],
                vec![],
            );
            let s = quiddity::kernel::geom::dot(frame.origin, frame.z);
            let segment = Segment {
                solid: 0,
                diameter: 4.,
                axis: 2,
                axis_point: frame.origin,
                direction: frame.z,
                s_lo: s - 1.,
                s_hi: s,
                external: false,
                faces: vec![0],
            };
            assert_eq!(
                classify_end(&part, &segment, s, true),
                (End::Unknown, vec![])
            );
        }
    }
}

#[test]
fn tied_countersink_attribution_keeps_the_bore_under_motion_and_seat_permutation() {
    use features::countersinks::CounterSink;
    use features::evidence::Occurrence;
    let path = common::fixtures().join("golden_simple_through_hole.step");
    for at in motions() {
        let part = read_step_file_placed(&path, &at).unwrap();
        let ctx = Context::new(&part);
        let holes = features::holes::discover(&ctx, &[]);
        let hole = &holes[0].record;
        let cs = CounterSink {
            axis: hole.axis,
            location: ([0, 1, 2].map(|i| hole.location[i] - hole.axis[i])).into(),
            major_diameter: hole.diameter + 2.,
            drill_diameter: hole.diameter,
            included_angle: 90.,
            depth: 1.,
        };
        let mut seats = vec![
            Occurrence {
                record: cs.clone(),
                defining: vec![1],
                context: vec![],
            },
            Occurrence {
                record: cs,
                defining: vec![7],
                context: vec![],
            },
        ];
        let found = features::holes::discover(&ctx, &seats);
        assert_eq!(found.len(), 1);
        assert!(found[0].record.csink.is_none());
        assert_eq!(found[0].defining, holes[0].defining);
        seats.reverse();
        let found = features::holes::discover(&ctx, &seats);
        assert_eq!(found.len(), 1);
        assert!(found[0].record.csink.is_none());
        assert_eq!(found[0].defining, holes[0].defining);
    }
}

#[test]
fn dominant_component_sign_is_presentation_only_and_preserves_thread_claims_and_ids() {
    let path = common::fixtures().join("modelled_threads/left_two_start.step.gz");
    let part = read_step_file_placed(&path, &IDENTITY).unwrap();
    let original = features::recognise(&part);
    assert_eq!(original.modelled_threads.len(), 1);
    let before = quiddity::correspondence::fingerprint(&part, &original);
    for degrees in [45_f64, 135., 225., 315.] {
        let (s, c) = degrees.to_radians().sin_cos();
        let at = [[c, 0., s, 123.456], [0., 1., 0., -78.9], [-s, 0., c, 41.3]];
        let moved = read_step_file_placed(&path, &at).unwrap();
        let mapped = quiddity::framed_records::map_features(
            original.clone(),
            &quiddity::framed_records::Rigid::from_placement(&at),
        );
        let after = quiddity::correspondence::fingerprint(&moved, &mapped.features);
        let thread = |prints: &quiddity::correspondence::Fingerprints| {
            prints
                .features
                .iter()
                .filter(|f| f.family == "modelled_threads")
                .map(|f| {
                    (
                        f.id.clone(),
                        f.faces.clone(),
                        f.sizes.clone(),
                        f.traits.clone(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(thread(&before), thread(&after));
        let a = &original.modelled_threads[0];
        let b = &mapped.features.modelled_threads[0];
        assert_eq!(
            (a.hand, a.starts, a.pitch, a.length),
            (b.hand, b.starts, b.pitch, b.length)
        );
        // Recanonicalisation must swap/negate parameters rather than moving the physical ends.
        let ends = |t: &features::modelled_threads::ModelledThread| {
            [t.start, t.end].map(|s| {
                quiddity::kernel::geom::add(
                    t.axis.point,
                    quiddity::kernel::geom::scale(t.axis.direction, s),
                )
            })
        };
        let expected = ends(a)
            .map(|p| [0, 1, 2].map(|i| at[i][3] + (0..3).map(|j| at[i][j] * p[j]).sum::<f64>()));
        assert!(ends(b).iter().all(|p| {
            expected
                .iter()
                .any(|q| quiddity::kernel::py::dist(p, q) < 1e-9)
        }));
    }
}

#[test]
fn a_symmetric_internal_relief_keeps_its_undirected_through_bore_under_motion() {
    let path = common::fixtures().join("captured/ece751715f714026.step.gz");
    let mut faces = None;
    for at in motions() {
        let part = read_step_file_placed(&path, &at).unwrap();
        let found = features::holes::discover(&Context::new(&part), &[]);
        assert_eq!(found.len(), 1, "{at:?}");
        assert_eq!(found[0].record.depth, 20.);
        assert_eq!(found[0].record.diameter, 10.);
        assert!(found[0].record.cbore.is_none() && found[0].record.spotface.is_none());
        let mut got = found[0].defining.clone();
        got.sort_unstable();
        assert_eq!(faces.get_or_insert_with(|| got.clone()), &got);
    }
}

#[test]
fn bevel_uses_its_cutting_plane_to_resolve_equal_support_distances_under_motion() {
    let Some(dir) = common::corpus_dir() else {
        return;
    };
    let path = dir.join("mfcadpp_holdout/483.step");
    let part = read_step_file_placed(&path, &IDENTITY).unwrap();
    let found = features::angled_steps::discover(&Context::new(&part));
    assert_eq!(found.len(), 3);
    // The bevel (13) and its concave blind cap (17), which defines the cut since the
    // angled-step cap rule.
    let real = found.iter().find(|o| o.defining == [13, 17]).unwrap();
    // Independent OpenCascade plane equation: x=6.095095 at y=29.381039 is on the
    // bevel itself; the removed corner is the opposite x support, 10.137419.
    let bevel = features::bevel::classify_bevel(&part, 13).unwrap();
    let centre = bevel.bounds.centre();
    let planes =
        features::bevel::proved_bevel_planes(&Context::new(&part), 13, centre, bevel.edge_axis)
            .unwrap();
    let corner = [0, 1, 2].map(|i| {
        quiddity::kernel::py::round_to3(if i == bevel.edge_axis {
            centre[i]
        } else {
            planes[&i]
        })
    });
    assert_eq!(corner, [10.137, 29.381, 13.064]);
    assert_eq!(*real.record.corner.unwrap(), corner);
    let read = |at: &Placement| {
        let framed = quiddity::frames::prepare_framed_file(&path, at).unwrap();
        let mut found: Vec<_> = features::angled_steps::discover(&Context::new(&framed.part))
            .into_iter()
            .map(|o| o.defining)
            .collect();
        found.sort();
        found
    };
    let baseline = read(&IDENTITY);
    for at in motions() {
        assert_eq!(read(&at), baseline, "{at:?}");
    }
}

#[test]
fn symmetric_nonorthogonal_frame_directions_refuse_after_motion_and_face_permutation() {
    use quiddity::frames::{FrameRefusal, infer_part_frame};
    let path = common::fixtures().join("ties/regular_tetrahedron.step");
    for at in motions() {
        let part = read_step_file_placed(&path, &at).unwrap();
        assert_eq!(
            infer_part_frame(&part),
            Err(FrameRefusal::NoAnalyticDirection)
        );
        let n = part.faces.len();
        let mut faces = part.faces.clone();
        faces.reverse();
        let mut solids = part.solids.clone();
        for solid in &mut solids {
            for f in &mut solid.faces {
                *f = n - 1 - *f;
            }
        }
        let permuted = quiddity::Part::new(faces, part.edges.clone(), solids);
        assert_eq!(
            infer_part_frame(&permuted),
            Err(FrameRefusal::NoAnalyticDirection)
        );
    }
}

#[test]
fn a_square_recess_needs_a_covariant_body_relative_run_for_correspondence() {
    let Some(dir) = common::corpus_dir() else {
        return;
    };
    let path = dir.join("mfcadpp/10049.step");
    let read = |at: &Placement| {
        let part = read_step_file_placed(&path, at).unwrap();
        let found = features::recognise(&part);
        quiddity::correspondence::fingerprint(&part, &found)
            .features
            .into_iter()
            .filter(|f| f.family == "pockets")
            .map(|f| (f.faces, (f.axis.unwrap(), f.sizes)))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let baseline = read(&IDENTITY);
    assert_eq!(baseline.len(), 3);
    for at in [
        motions()[1],
        motions()[4],
        motions()[5],
        [
            [0., 0., 1., 123.456],
            [1., 0., 0., -78.9],
            [0., 1., 0., 41.3],
        ],
    ] {
        let moved = read(&at);
        assert_eq!(
            baseline.keys().collect::<Vec<_>>(),
            moved.keys().collect::<Vec<_>>()
        );
        for (faces, (axis, sizes)) in &baseline {
            let expected = [0, 1, 2].map(|i| (0..3).map(|j| at[i][j] * axis[j]).sum::<f64>());
            let (actual, got_sizes) = &moved[faces];
            assert!(
                (quiddity::kernel::py::dot(&expected, actual).abs() - 1.).abs() < 1e-9,
                "{at:?} {faces:?}"
            );
            assert_eq!(sizes, got_sizes);
        }
    }
}

#[test]
fn a_cube_refuses_competing_circular_pattern_axes_under_motion() {
    let path = common::fixtures().join("ties/cube.step");
    for at in motions() {
        let part = read_step_file_placed(&path, &at).unwrap();
        assert!(part.solid_is_valid(0));
        // Every one of the three fourfold axes has equal analytic face-area support.
        // The four side faces cannot silently become a different claimed pattern.
        let found = features::circular_face_patterns::discover(&Context::new(&part));
        assert!(found.is_empty(), "{at:?}: {found:?}");
    }
}

#[test]
fn a_symmetric_cylinder_keeps_its_boss_with_an_end_gauge_under_motion() {
    let path = common::fixtures().join("ties/symmetric_cylinder.step");
    for at in motions() {
        let part = read_step_file_placed(&path, &at).unwrap();
        let ctx = Context::new(&part);
        let cylinders: Vec<_> = ctx.cylinders().iter().filter(|c| c.external).collect();
        assert_eq!(cylinders.len(), 1);
        let found = features::bosses::discover(&ctx);
        assert_eq!(found.len(), 1, "{at:?}");
        let b = &found[0].record;
        assert_eq!((b.diameter, b.height), (10., 20.));
        let ends = [
            b.location,
            quiddity::kernel::geom::sub(
                b.location,
                quiddity::kernel::geom::scale(b.axis, b.height),
            ),
        ];
        for z in [-10., 10.] {
            let expected = [0, 1, 2].map(|i| at[i][3] + z * at[i][2]);
            assert!(
                ends.iter()
                    .any(|p| quiddity::kernel::py::dist(p, &expected) < 1e-6)
            );
        }
    }
}

#[test]
fn tied_boss_end_states_use_the_body_origin_after_motion() {
    let Some(dir) = common::corpus_dir() else {
        return;
    };
    let path = dir.join("nist/nist_ctc_05_asme1_rd.stp");
    let baseline = read_step_file_placed(&path, &IDENTITY).unwrap();
    let before = features::bosses::discover(&Context::new(&baseline));
    assert!(before.iter().any(|b| b.defining == [1, 182]));
    for at in motions() {
        let moved = read_step_file_placed(&path, &at).unwrap();
        let after = features::bosses::discover(&Context::new(&moved));
        assert_eq!(before.len(), after.len(), "{at:?}");
        for b in &before {
            let a = after.iter().find(|a| a.defining == b.defining).unwrap();
            assert_eq!(b.context, a.context, "{at:?}: {:?}", b.defining);
            for (i, row) in at.iter().enumerate() {
                let point = row[3] + (0..3).map(|j| row[j] * b.record.location[j]).sum::<f64>();
                let axis = (0..3).map(|j| row[j] * b.record.axis[j]).sum::<f64>();
                assert!((point - a.record.location[i]).abs() < 1e-6);
                assert!((axis - a.record.axis[i]).abs() < 1e-8);
            }
        }
    }
}

#[test]
fn a_radial_profile_keeps_its_stronger_axis_despite_tied_budget_cutoff() {
    let path = common::fixtures().join("golden_repeating_radial_profile.step");
    let mut claims = None;
    for at in motions() {
        let part = read_step_file_placed(&path, &at).unwrap();
        let found = features::circular_face_patterns::discover(&Context::new(&part));
        assert_eq!(found.len(), 1, "{at:?}");
        assert_eq!(found[0].record.count, 8);
        let mut faces = found[0].defining.clone();
        faces.sort_unstable();
        assert_eq!(claims.get_or_insert_with(|| faces.clone()), &faces);
    }
}

#[test]
fn symmetric_double_counterbores_preserve_ftc10_and_captured_inventory() {
    if let Some(dir) = common::corpus_dir() {
        let path = dir.join("nist/nist_ftc_10_asme1_rb.stp");
        let (part, framed, _) =
            quiddity::serve::recognise_step(path.to_str().unwrap(), &IDENTITY).unwrap();
        let got = features::recognise(&part);
        assert_eq!((got.holes.len(), got.hole_patterns.len()), (30, 7));
        assert_eq!(
            (
                framed.features.holes.len(),
                framed.features.hole_patterns.len()
            ),
            (30, 7)
        );
        let double_mouths = got.holes.iter().filter(|h| h.diameter == 7.).count();
        assert_eq!(double_mouths, 4);
        for at in motions() {
            let moved = read_step_file_placed(&path, &at).unwrap();
            let moved = features::recognise(&moved);
            assert_eq!((moved.holes.len(), moved.hole_patterns.len()), (30, 7));
            for (i, a) in got
                .holes
                .iter()
                .enumerate()
                .filter(|(_, h)| h.diameter == 7.)
            {
                let j = moved.defining["holes"]
                    .iter()
                    .position(|f| *f == got.defining["holes"][i])
                    .unwrap();
                let b = &moved.holes[j];
                assert_eq!(
                    (&a.cbore, a.diameter, a.depth, a.bottom),
                    (&b.cbore, b.diameter, b.depth, b.bottom)
                );
                for p in bore_ends(a) {
                    let expected =
                        [0, 1, 2].map(|i| at[i][3] + (0..3).map(|j| at[i][j] * p[j]).sum::<f64>());
                    assert!(
                        bore_ends(b)
                            .iter()
                            .any(|q| quiddity::kernel::py::dist(&expected, q) < 1e-6)
                    );
                }
            }
        }
        // The two API contexts may reverse the same end gauge. Compare the actual physical
        // endpoints and defining claims, rather than requiring a material drilling-entry end.
        for (i, a) in got
            .holes
            .iter()
            .enumerate()
            .filter(|(_, h)| h.diameter == 7.)
        {
            let j = framed.features.defining["holes"]
                .iter()
                .position(|f| *f == got.defining["holes"][i])
                .unwrap();
            let b = &framed.features.holes[j];
            assert_eq!(
                (a.diameter, a.depth, a.bottom),
                (b.diameter, b.depth, b.bottom)
            );
            assert_eq!(
                (&a.cbore, &a.spotface, &a.csink),
                (&b.cbore, &b.spotface, &b.csink)
            );
            assert_eq!(
                a.cbore,
                Some(features::holes::CounterBore {
                    diameter: 10.,
                    depth: 10.
                })
            );
            assert_eq!(got.defining["holes"][i].len(), 3, "bore and both mouths");
            assert!(bore_ends(a).iter().all(|p| {
                bore_ends(b)
                    .iter()
                    .any(|q| quiddity::kernel::py::dist(p, q) < 1e-6)
            }));
        }
    }
    let calls = common::load("captured/calls.json");
    let file = calls["calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["file"].as_str())
        .find(|f| f.starts_with("f1d2c1f2"))
        .unwrap();
    let mut claims = None;
    for at in motions() {
        let part =
            read_step_file_placed(&common::fixtures().join("captured").join(file), &at).unwrap();
        let found = features::holes::discover(&Context::new(&part), &[]);
        assert_eq!(found.len(), 1);
        let h = &found[0].record;
        assert_eq!(
            (h.diameter, h.depth, h.bottom),
            (10., 8., features::holes::Bottom::Through)
        );
        assert_eq!(
            h.cbore,
            Some(features::holes::CounterBore {
                diameter: 18.,
                depth: 6.
            })
        );
        assert!(h.spotface.is_none() && h.csink.is_none());
        let mut faces = found[0].defining.clone();
        faces.sort_unstable();
        assert_eq!(faces.len(), 3, "bore and both enlarged mouth cylinders");
        assert_eq!(claims.get_or_insert_with(|| faces.clone()), &faces);
        for z in [-4., 4.] {
            let expected = [0, 1, 2].map(|i| at[i][3] + z * at[i][2]);
            assert!(
                bore_ends(h)
                    .iter()
                    .any(|p| quiddity::kernel::py::dist(p, &expected) < 1e-6)
            );
        }
    }
}

fn claim_identity(part: &quiddity::Part, f: &features::Features) -> Vec<serde_json::Value> {
    quiddity::correspondence::fingerprint(part, f).features.iter().filter(|f| !matches!(f.family.as_str(), "step_levels" | "risers")).map(|f| serde_json::json!({"family":f.family,"id":f.id,"faces":f.faces,"sizes":f.sizes,"traits":f.traits})).collect()
}

fn assert_claim_identity(before: &[serde_json::Value], after: &[serde_json::Value]) {
    assert_eq!(before.len(), after.len());
    for (a, b) in before.iter().zip(after) {
        for field in ["family", "id", "faces", "traits"] {
            assert_eq!(a[field], b[field]);
        }
        // Surface-section lengths are recomputed from transformed endpoints; use the
        // existing numeric parity comparator for the resulting floating-point roundoff.
        assert!(
            common::same(&a["sizes"], &b["sizes"]),
            "{} vs {}",
            a["sizes"],
            b["sizes"]
        );
    }
}

#[test]
fn frame_gauge_mapping_preserves_claims_sizes_and_ids() {
    use quiddity::frames::{FrameGauge, infer_part_frame};
    let mut gauges = Vec::new();
    for file in ["ties/symmetric_cylinder.step", "ties/square_pocket.step"] {
        let path = common::fixtures().join(file);
        let part = read_step_file_placed(&path, &IDENTITY).unwrap();
        gauges.push(infer_part_frame(&part).unwrap().gauge);
        let original = features::recognise(&part);
        let before = claim_identity(&part, &original);
        assert!(!before.is_empty());
        for at in motions() {
            let moved = read_step_file_placed(&path, &at).unwrap();
            let mapped = quiddity::framed_records::map_features(
                original.clone(),
                &quiddity::framed_records::Rigid::from_placement(&at),
            );
            assert_claim_identity(&before, &claim_identity(&moved, &mapped.features));
        }
    }
    assert!(gauges.contains(&FrameGauge::Axial));
    assert!(gauges.contains(&FrameGauge::Orthogonal));
}

#[test]
fn square_lattice_angle_is_a_gauge_preserving_claims_sizes_and_ids() {
    let path = common::fixtures().join("ties/square_hole_grid.step");
    let part = read_step_file_placed(&path, &IDENTITY).unwrap();
    let mut original = features::recognise(&part);
    assert_eq!(original.holes.len(), 9);
    assert_eq!(original.hole_patterns.len(), 1);
    let claims: std::collections::BTreeSet<_> = original.defining["holes"].iter().collect();
    assert_eq!(
        claims.len(),
        9,
        "member permutations must be observable as changed IDs"
    );
    let before = claim_identity(&part, &original);
    // A 90-degree change of the square lattice representative claims the same members.
    let features::hole_patterns::HolePattern::RectGrid { angle, .. } =
        &mut original.hole_patterns[0]
    else {
        panic!("expected grid")
    };
    *angle += 90.;
    assert_eq!(before, claim_identity(&part, &original));
    for at in motions() {
        let moved = read_step_file_placed(&path, &at).unwrap();
        let mapped = quiddity::framed_records::map_features(
            original.clone(),
            &quiddity::framed_records::Rigid::from_placement(&at),
        );
        assert_claim_identity(&before, &claim_identity(&moved, &mapped.features));
    }
}

#[test]
fn a_five_hundred_hole_perforated_plate_has_one_grid() {
    use features::holes::{Bottom, HoleRecord};
    let mut holes: Vec<_> = (0..20)
        .flat_map(|x| {
            (0..25).map(move |y| HoleRecord {
                superseded_by: None,
                axis: [0., 0., -1.],
                location: ([x as f64 * 10., y as f64 * 10., 5.]).into(),
                diameter: 2.,
                depth: 10.,
                bottom: Bottom::Through,
                cbore: None,
                spotface: None,
                outer_steps: Vec::new(),
                exit_obstructions: Vec::new(),
                csink: None,
            })
        })
        .collect();
    for _ in 0..2 {
        let got = features::hole_patterns::recognise_hole_patterns(&holes);
        let [
            features::hole_patterns::HolePattern::RectGrid {
                rows,
                cols,
                holes: members,
                ..
            },
        ] = got.as_slice()
        else {
            panic!("{got:?}")
        };
        assert_eq!((*rows, *cols, members.len()), (20, 25, 500));
        holes.reverse();
    }
}

/// For #83: equal seats at opposite mouths are interchangeable end gauges. Their
/// evidence is the pair, independent of the representative end or discovery order.
#[test]
fn symmetric_countersink_seats_survive_motion_and_seat_permutation() {
    let path = common::fixtures().join("countersink_two_face.step.gz");
    let mut claims = None;
    for at in motions() {
        let part = read_step_file_placed(&path, &at).unwrap();
        let ctx = Context::new(&part);
        let mut seats = features::countersinks::discover_verified(&ctx).unwrap();
        assert_eq!(seats.len(), 2);
        for _ in 0..2 {
            let found = features::holes::discover_verified(&ctx, &seats).unwrap();
            assert_eq!(found.len(), 1);
            let hole = &found[0].record;
            let seat = hole.csink.as_ref().expect("symmetric seats retained");
            assert_eq!(
                (hole.diameter, seat.major_diameter, seat.depth),
                (6., 14., 4.)
            );
            assert!(
                quiddity::kernel::geom::dot(
                    hole.axis,
                    quiddity::kernel::geom::unit(seat.axis).unwrap()
                ) > 1. - 1e-6
            );
            let mut context = found[0].context.clone();
            context.sort_unstable();
            context.dedup();
            let mut expected: Vec<_> = seats
                .iter()
                .flat_map(|s| s.defining.iter().copied())
                .collect();
            expected.sort_unstable();
            expected.dedup();
            assert_eq!(context, expected);
            assert_eq!(claims.get_or_insert_with(|| context.clone()), &context);
            seats.reverse();
        }
    }
}
