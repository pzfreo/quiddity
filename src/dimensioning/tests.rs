use super::*;
use crate::frames::{FrameGauge, PartFrame};
use crate::kernel::pmi;
fn input() -> Input {
    Input {
        frame: PartFrame {
            origin: [0.; 3],
            x: [1., 0., 0.],
            y: [0., 1., 0.],
            z: [0., 0., 1.],
            gauge: FrameGauge::Full,
        },
        bounds: [[0.; 3], [100., 60., 10.]],
        holes: vec![],
        patterns: vec![],
        planes: vec![],
        steps: StepLevels::default(),
        cartesian_envelope: true,
        deferred: vec![],
        datums: vec![],
        round_axes: vec![],
        specified: vec![],
        policy: Policy::default(),
    }
}
fn support(id: &str, at: Point, plane: bool) -> Support {
    Support {
        feature: id.into(),
        faces: vec![],
        body: None,
        at,
        plane,
    }
}
fn hole(n: usize, x: f64, y: f64) -> Hole {
    Hole {
        support: support(&format!("hole:{n}"), [x, y, 10.], false),
        direction: [0., 0., -1.],
        diameter: 6.6,
        depth: None,
        traversal: None,
        bottom: crate::features::holes::Bottom::Through,
        details: vec![],
    }
}
fn pattern(i: &Input, arrangement: Arrangement) -> Pattern {
    Pattern {
        members: i.holes.iter().map(|h| h.support.feature.clone()).collect(),
        arrangement,
    }
}
fn loc(id: &str, a: f64, b: f64, plane: bool) -> Required {
    Required {
        id: id.into(),
        kind: Kind::Location {
            from: support(&format!("{id}:a"), [a, 0., 0.], plane),
            to: support(&format!("{id}:b"), [b, 0., 0.], plane),
            axis: 0,
        },
        nominal: (b - a).abs(),
        count: 1,
        pattern: None,
        tolerance: Tolerance::Untoleranced,
        reference: None,
        covers: vec![id.into()],
    }
}
fn merged(
    rs: Vec<Required>,
    specified: &[&str],
    references: bool,
) -> (Vec<Required>, Vec<Conveyed>) {
    super::rules::test_consolidate(rs, specified, references)
}
fn size_count(rs: &[Required]) -> usize {
    rs.iter()
        .filter(|r| matches!(r.kind, Kind::Size { .. }))
        .count()
}
// Draftwright dims/holes.rs: coincidence, specified priority, stable tie break.
#[test]
fn raw_ends_and_tolerance_constrain_merging() {
    for (delta, tol, expected) in [
        (0.02, Tolerance::Untoleranced, 2),
        (1e-9, Tolerance::Untoleranced, 1),
        (0., Tolerance::Basic, 2),
        (0., Tolerance::General, 2),
        (0., Tolerance::Explicit("±0.1".into()), 2),
    ] {
        let a = loc("a", 0., 40., false);
        let mut b = loc("b", 0., 40. + delta, false);
        b.tolerance = tol;
        assert_eq!(merged(vec![a, b], &[], false).0.len(), expected);
    }
}
#[test]
fn specified_then_smaller_id_owns_coincident_claims() {
    let a = loc("a", 0., 40., false);
    let b = loc("b", 0., 40., false);
    for (specified, expected) in [(vec![], "a"), (vec!["b"], "b")] {
        let (rs, cs) = merged(vec![b.clone(), a.clone()], &specified, false);
        assert_eq!(rs[0].id, expected);
        assert_eq!(rs[0].covers, ["a", "b"]);
        assert_eq!(cs[0].by, [expected]);
    }
}
#[test]
fn basic_50_does_not_merge_with_general_50() {
    let mut a = loc("a", 0., 50., false);
    a.tolerance = Tolerance::Basic;
    let mut b = a.clone();
    b.id = "b".into();
    b.tolerance = Tolerance::General;
    assert_eq!(merged(vec![a, b], &[], false).0.len(), 2);
}
// bridge/holes.rs and dimension_rules: identical requirements, blind versus through.
#[test]
fn through_traversal_depth_is_not_a_requirement_but_blind_depth_is() {
    let mut i = input();
    i.holes = vec![hole(1, 15., 15.), hole(2, 45., 15.)];
    let r = plan(&i).0;
    assert_eq!(size_count(&r), 1);
    assert!(r.iter().any(|r| r.count == 2));
    i.holes[0].depth = Some(5.);
    i.holes[0].bottom = crate::features::holes::Bottom::Flat;
    assert_eq!(size_count(&plan(&i).0), 3);
    i.holes[1].depth = Some(8.);
    i.holes[1].bottom = crate::features::holes::Bottom::Flat;
    assert_eq!(size_count(&plan(&i).0), 4);
}
#[test]
fn compound_hole_preserves_every_quantity() {
    let mut i = input();
    let mut h = hole(1, 50., 30.);
    h.depth = Some(5.);
    for (role, nominal, kind) in [
        ("counterbore.diameter", 12., SizeKind::Diameter),
        ("counterbore.depth", 2., SizeKind::Length),
        ("spotface.diameter", 16., SizeKind::Diameter),
        ("spotface.depth", 1., SizeKind::Length),
        ("countersink.diameter", 20., SizeKind::Diameter),
        ("countersink.angle", 90., SizeKind::Angle),
    ] {
        h.details.push(Size {
            role: role.into(),
            nominal,
            kind,
        });
    }
    i.holes.push(h);
    let rs = plan(&i).0;
    assert_eq!(size_count(&rs), 8);
    let angle = rs
        .iter()
        .find(|r| matches!(&r.kind,Kind::Size {role,..} if role=="countersink.angle"))
        .unwrap();
    assert!(matches!(
        angle.dimension_kind(|_| pmi::FeatureId(0)),
        pmi::DimensionKind::Size {
            angle: Some(pmi::AngleSelection::Equal),
            ..
        }
    ));
}
// dimension_rules grid: one size, one centre, pitch for each varying coordinate.
#[test]
fn plate_has_eleven_requirements_and_conveys_member_locations() {
    let mut i = input();
    for x in [20., 50., 80.] {
        for y in [15., 45.] {
            i.holes.push(hole(i.holes.len(), x, y));
        }
    }
    i.patterns.push(pattern(&i, Arrangement::Grid));
    let mut h = hole(8, 50., 30.);
    h.diameter = 8.;
    i.holes.push(h);
    let mut h = hole(9, 95., 30.);
    h.diameter = 4.2;
    i.holes.push(h);
    let (rs, cs) = plan(&i);
    assert_eq!(rs.len(), 11, "{rs:#?}");
    assert!(rs.iter().any(|r| r.count == 6 && r.nominal == 6.6));
    assert_eq!(
        rs.iter()
            .filter(|r| matches!(r.pattern, Some(PatternRole::Pitch { .. })))
            .count(),
        2
    );
    assert!(
        rs.iter()
            .any(|r| r.pattern == Some(PatternRole::Pitch { intervals: 2 }) && r.nominal == 30.)
    );
    assert!(
        cs.iter()
            .all(|c| !c.by.is_empty() && c.by.iter().all(|id| rs.iter().any(|r| r.id == *id)))
    );
}
#[test]
fn grid_is_located_by_centre_and_line_by_first_member() {
    for arrangement in [Arrangement::Grid, Arrangement::Line] {
        let mut i = input();
        i.holes = vec![hole(1, 10., 20.), hole(2, 30., 20.), hole(3, 50., 20.)];
        i.patterns.push(pattern(&i, arrangement.clone()));
        let rs = plan(&i).0;
        let expected = if arrangement == Arrangement::Grid {
            PatternRole::Centre
        } else {
            PatternRole::FirstMember
        };
        assert!(rs.iter().any(|r| r.pattern == Some(expected.clone())
            && r.nominal
                == if arrangement == Arrangement::Grid {
                    30.
                } else {
                    10.
                }));
        assert!(rs.iter().any(|r|r.pattern==Some(PatternRole::Pitch {intervals:2}) && r.nominal==20.));
    }
}
#[test]
fn uneven_or_missing_lattice_members_are_dimensioned_singly() {
    for holes in [
        vec![hole(1, 10., 10.), hole(2, 20., 13.), hole(3, 35., 10.)],
        vec![hole(1, 10., 10.), hole(2, 20., 10.), hole(3, 10., 20.)],
    ] {
        let mut i = input();
        i.holes = holes;
        i.patterns.push(pattern(&i, Arrangement::Grid));
        let rs = plan(&i).0;
        assert!(!rs.iter().any(|r| r.pattern.is_some()));
        assert!(rs.iter().any(|r| r.count == 3));
    }
}
#[test]
fn oblique_coaxial_or_nonidentical_pattern_is_declined() {
    for mode in 0..4 {
        let mut i = input();
        i.holes = vec![hole(1, 10., 10.), hole(2, 30., 10.)];
        match mode {
            0 => {
                for h in &mut i.holes {
                    h.direction = [0., 0.6, 0.8];
                }
            }
            1 => {
                i.holes[1].support.at = i.holes[0].support.at;
                i.holes[1].support.at[2] += 10.;
            }
            2 => i.holes[1].diameter = 8.,
            _ => i.holes[1].depth = Some(5.),
        }
        i.patterns.push(pattern(&i, Arrangement::Line));
        assert!(!plan(&i).0.iter().any(|r| r.pattern.is_some()));
    }
}
fn circle(n: usize) -> Input {
    let mut i = input();
    for j in 0..n {
        let angle = j as f64 * std::f64::consts::TAU / n as f64;
        i.holes
            .push(hole(j, 50. + 20. * angle.cos(), 30. + 20. * angle.sin()));
    }
    i.patterns.push(pattern(
        &i,
        Arrangement::BoltCircle {
            centre: [50., 30., 10.],
            diameter: 40.,
        },
    ));
    i
}
#[test]
fn small_bolt_circle_requires_parallel_centre_corroboration() {
    for n in [3, 4, 5, 6] {
        let mut i = circle(n);
        assert_eq!(
            plan(&i)
                .0
                .iter()
                .any(|r| r.pattern == Some(PatternRole::Centre)),
            n >= 5
        );
        i.round_axes.push(([50.49, 30., 10.], [0., 0., 1.]));
        assert!(
            plan(&i)
                .0
                .iter()
                .any(|r| r.pattern == Some(PatternRole::Centre))
        );
        i.round_axes[0].1 = [1., 0., 0.];
        assert_eq!(
            plan(&i)
                .0
                .iter()
                .any(|r| r.pattern == Some(PatternRole::Centre)),
            n >= 5
        );
    }
    let mut i = circle(4);
    let mut h = hole(10, 50., 30.);
    h.diameter = 8.;
    i.holes.push(h);
    assert!(
        plan(&i)
            .0
            .iter()
            .any(|r| r.pattern == Some(PatternRole::Centre))
    );
}
#[test]
fn missing_duplicate_and_overlapping_members_do_not_claim_twice() {
    let mut i = input();
    i.holes = vec![hole(1, 10., 20.), hole(2, 30., 20.), hole(3, 50., 20.)];
    let mut p = pattern(&i, Arrangement::Line);
    p.members.push("missing".into());
    i.patterns.push(p);
    assert!(!plan(&i).0.iter().any(|r| r.pattern.is_some()));
    i.patterns.clear();
    i.patterns.push(pattern(&i, Arrangement::Line));
    let mut p = i.patterns[0].clone();
    p.members.pop();
    i.patterns.push(p);
    let rs = plan(&i).0;
    assert_eq!(size_count(&rs), 1);
    assert!(rs.iter().any(|r| r.count == 3));
}
// repeated_dimensions: one pitch for parallel rows on the same physical stations.
#[test]
fn parallel_rows_share_pitch_but_different_stations_do_not() {
    for offset in [0., 5.] {
        let mut i = input();
        for (j, (x, y)) in [
            (10., 10.),
            (30., 10.),
            (50., 10.),
            (10. + offset, 40.),
            (30. + offset, 40.),
            (50. + offset, 40.),
        ]
        .into_iter()
        .enumerate()
        {
            i.holes.push(hole(j, x, y));
        }
        for ids in [[0, 1, 2], [3, 4, 5]] {
            i.patterns.push(Pattern {
                members: ids
                    .iter()
                    .map(|&j| i.holes[j].support.feature.clone())
                    .collect(),
                arrangement: Arrangement::Line,
            });
        }
        assert_eq!(
            plan(&i)
                .0
                .iter()
                .filter(|r| matches!(r.pattern, Some(PatternRole::Pitch { .. })))
                .count(),
            if offset == 0. { 1 } else { 2 }
        );
    }
}
#[test]
fn face_level_and_axis_location_share_one_statement() {
    let a = loc("level", 0., 12.5, true);
    let b = loc("hole", 0., 12.5, false);
    let (rs, cs) = merged(vec![a, b], &["hole"], false);
    assert_eq!(rs.len(), 1);
    assert_eq!(rs[0].id, "hole");
    assert_eq!(cs[0].by, ["hole"]);
}
// lint/redundancy and h5_chains: physical station graphs, no closed driving chains.
#[test]
fn closed_plane_chain_conveys_last_interval_or_makes_a_reference() {
    for references in [false, true] {
        let (rs, cs) = merged(
            vec![
                loc("a", 0., 10., true),
                loc("b", 10., 30., true),
                loc("c", 0., 30., true),
            ],
            &[],
            references,
        );
        if references {
            assert_eq!(rs.len(), 3);
            assert_eq!(rs[2].reference.as_ref().unwrap(), &["a", "b"]);
        } else {
            assert_eq!(rs.len(), 2);
            assert_eq!(cs[0].by, ["a", "b"]);
        }
    }
}
#[test]
fn equal_values_different_planes_and_point_paths_are_not_determination() {
    let rs = vec![
        loc("a", 0., 10., true),
        loc("b", 20., 30., true),
        loc("c", 0., 30., true),
    ];
    assert_eq!(merged(rs, &[], false).0.len(), 3);
    let rs = vec![
        loc("a", 0., 10., false),
        loc("b", 10., 30., false),
        loc("c", 0., 30., true),
    ];
    assert_eq!(merged(rs, &[], false).0.len(), 3);
}
#[test]
fn no_toleranced_dimension_is_made_reference_and_specified_chain_is_retained() {
    for tol in [
        Tolerance::General,
        Tolerance::Basic,
        Tolerance::Explicit("±0.1".into()),
    ] {
        let rs = [
            loc("a", 0., 10., true),
            loc("b", 10., 30., true),
            loc("c", 0., 30., true),
        ]
        .map(|mut r| {
            r.tolerance = tol.clone();
            r
        })
        .to_vec();
        let (rs, _) = merged(rs, &["c"], true);
        assert!(rs.iter().all(|r| r.reference.is_none()));
        assert!(rs.iter().any(|r| r.id == "c"));
    }
}
#[test]
fn mixed_tolerance_chain_does_not_determine_a_basic_interval() {
    let mut c = loc("c", 0., 30., true);
    c.tolerance = Tolerance::Basic;
    assert_eq!(
        merged(
            vec![loc("a", 0., 10., true), loc("b", 10., 30., true), c],
            &[],
            true
        )
        .0
        .len(),
        3
    );
}
fn add_planes(i: &mut Input, axis: usize, stations: &[f64]) {
    for (j, &v) in stations.iter().enumerate() {
        let mut at = [0.; 3];
        at[axis] = v;
        i.planes.push(Plane {
            support: support(&format!("plane:{axis}:{j}"), at, true),
            axis,
            body: 0,
        });
    }
}
#[test]
fn explicit_step_heights_and_tied_shoulders_have_one_owner() {
    let mut i = input();
    for (axis, stations) in [
        (0, vec![0., 20., 80., 100.]),
        (1, vec![0., 30., 60.]),
        (2, vec![0., 4., 10.]),
    ] {
        add_planes(&mut i, axis, &stations);
    }
    i.steps.heights = vec![
        i.planes
            .iter()
            .find(|p| p.axis == 2 && p.support.at[2] == 4.)
            .unwrap()
            .support
            .clone(),
    ];
    i.steps.shoulders = i
        .planes
        .iter()
        .filter(|p| {
            p.axis < 2 && p.support.at[p.axis] > 0. && p.support.at[p.axis] < i.bounds[1][p.axis]
        })
        .cloned()
        .collect();
    let rs = plan(&i).0;
    assert_eq!(rs.len(), 7);
    for nominal in [100., 60., 10., 20., 80., 30., 4.] {
        assert!(rs.iter().any(|r| r.nominal == nominal));
    }
}
#[test]
fn datums_choose_label_order_explicit_origin_overrides_and_zero_is_omitted() {
    let mut i = input();
    add_planes(&mut i, 0, &[0., 20., 100.]);
    i.planes[0].support.faces = vec![0];
    i.planes[1].support.faces = vec![1];
    i.planes[2].support.faces = vec![2];
    i.datums = vec![
        Datum {
            label: "B".into(),
            faces: vec![0],
        },
        Datum {
            label: "A".into(),
            faces: vec![2],
        },
    ];
    i.holes.push(hole(1, 20., 30.));
    assert!(plan(&i).0.iter().any(|r| r.nominal == 80.));
    i.policy.origins[0] = Origin::Datum("B".into());
    assert!(plan(&i).0.iter().any(|r| r.nominal == 20.));
    i.policy.origins[0] = Origin::Maximum;
    assert!(plan(&i).0.iter().any(|r| r.nominal == 80.));
    i.policy.origins[0] = Origin::Minimum;
    i.holes[0].support.at[0] = 0.;
    assert!(!plan(&i).0.iter().any(|r| r.nominal == 0.));
}
#[test]
fn distinct_bodies_never_merge_equal_plane_spans() {
    let a = loc("a", 0., 10., true);
    let mut b = loc("b", 0., 10., true);
    if let Kind::Location { to, .. } = &mut b.kind {
        to.body = Some(1);
    }
    assert_eq!(merged(vec![a, b], &[], false).0.len(), 2);
}
#[test]
fn recognition_order_member_order_and_faces_do_not_change_output_or_ids() {
    let mut i = input();
    for x in [20., 50., 80.] {
        for y in [15., 45.] {
            i.holes.push(hole(i.holes.len(), x, y));
        }
    }
    i.patterns.push(pattern(&i, Arrangement::Grid));
    add_planes(&mut i, 0, &[0., 20., 80., 100.]);
    i.steps.axis = 0;
    i.steps.heights = i.planes[1..3].iter().map(|p| p.support.clone()).collect();
    i.steps.shoulders = vec![Plane {
        support: support("shoulder:40", [0., 40., 0.], true),
        axis: 1,
        body: 0,
    }];
    i.deferred = vec![
        Deferred {
            id: "deferred:pocket:42".into(),
            faces: vec![42],
            family: "pockets".into(),
        },
        Deferred {
            id: "deferred:slot:43".into(),
            faces: vec![43],
            family: "slots".into(),
        },
    ];
    let expected = plan(&i);
    i.holes.reverse();
    i.planes.reverse();
    i.patterns[0].members.reverse();
    i.datums.reverse();
    i.steps.heights.reverse();
    i.steps.shoulders.reverse();
    i.deferred.reverse();
    assert_eq!(expected, plan(&i));
    assert_eq!(expected, plan(&i));
}
#[test]
fn pmi_group_and_dimensions_use_consumers_feature_arena() {
    let mut i = input();
    i.holes = vec![hole(1, 10., 20.), hole(2, 30., 20.)];
    let p = pattern(&i, Arrangement::Line);
    assert!(
        matches!(p.pmi_feature(|s|pmi::FeatureId(usize::from(s=="hole:2"))),pmi::Feature::Group {kind:pmi::GroupKind::PatternOfFeatures,members} if members==[pmi::FeatureId(0),pmi::FeatureId(1)])
    );
    assert!(matches!(
        loc("a", 0., 10., false).dimension_kind(|_| pmi::FeatureId(5)),
        pmi::DimensionKind::Location {
            from: pmi::FeatureId(5),
            to: pmi::FeatureId(5),
            ..
        }
    ));
}
#[test]
fn specified_member_remains_an_authority_without_a_conveyed_self_reference() {
    let mut i = input();
    i.holes = vec![hole(1, 10., 20.), hole(2, 30., 20.)];
    i.patterns.push(pattern(&i, Arrangement::Line));
    let mut r = super::rules::test_member_location(&i);
    let generated = r.id.clone();
    r.id = "specified:first-member".into();
    r.tolerance = Tolerance::Basic;
    i.specified.push(r.clone());
    let (rs, cs) = plan(&i);
    assert!(rs.contains(&r));
    assert!(!rs.iter().any(|r| r.id == generated));
    assert_eq!(
        cs.iter().find(|c| c.id == generated).unwrap().by,
        [r.id.clone()]
    );
    assert!(
        cs.iter()
            .find(|c| c.id == generated)
            .unwrap()
            .why
            .contains("regardless of tolerance")
    );
    assert!(!cs.iter().any(|c| c.id == r.id));
}

#[test]
fn specified_tolerance_replaces_generated_equivalent_without_merging_other_features() {
    let mut i = input();
    i.holes = vec![hole(1, 10., 20.)];
    let generated = super::rules::test_member_location(&i);
    let mut specified = generated.clone();
    specified.id = "specified:basic-location".into();
    specified.tolerance = Tolerance::Basic;
    i.specified.push(specified.clone());
    let (rs, cs) = plan(&i);
    assert!(rs.contains(&specified));
    assert!(!rs.iter().any(|r| r.id == generated.id));
    let retired = cs.iter().find(|c| c.id == generated.id).unwrap();
    assert_eq!(retired.by, [specified.id]);
    assert!(retired.why.contains("regardless of tolerance"));
    // Equal scalar spans on a different feature remain generated general claims.
    i.holes[0].support.feature = "hole:other".into();
    assert!(
        plan(&i)
            .0
            .iter()
            .any(|r| r.nominal == 10. && r.tolerance == Tolerance::General)
    );
    // Sizes obey the same specified-wins rule.
    let mut size = plan(&i)
        .0
        .into_iter()
        .find(|r| matches!(r.kind, Kind::Size { .. }))
        .unwrap();
    let id = size.id.clone();
    size.id = "specified:basic-size".into();
    size.tolerance = Tolerance::Basic;
    i.specified.push(size.clone());
    let (rs, cs) = plan(&i);
    assert!(rs.contains(&size));
    assert!(!rs.iter().any(|r| r.id == id));
    assert_eq!(cs.iter().find(|c| c.id == id).unwrap().by, [size.id]);
}

#[test]
fn pitch_endpoints_are_actual_members_with_only_their_own_faces() {
    for arrangement in [Arrangement::Line, Arrangement::Grid] {
        let mut i = input();
        for x in [10., 30., 50.] {
            for y in if arrangement == Arrangement::Grid {
                vec![15., 45.]
            } else {
                vec![15.]
            } {
                let mut h = hole(i.holes.len(), x, y);
                h.support.faces = vec![100 + i.holes.len()];
                i.holes.push(h);
            }
        }
        i.patterns.push(pattern(&i, arrangement));
        let rs = plan(&i).0;
        for r in rs
            .iter()
            .filter(|r| matches!(r.pattern, Some(PatternRole::Pitch { .. })))
        {
            let Kind::Location { from, to, axis } = &r.kind else {
                panic!("pitch must be a location")
            };
            for endpoint in [from, to] {
                let member = i
                    .holes
                    .iter()
                    .find(|h| h.support.feature == endpoint.feature)
                    .unwrap();
                assert_eq!(endpoint, &member.support);
                assert_eq!(endpoint.faces.len(), 1);
            }
            assert!((to.at[*axis] - from.at[*axis] - r.nominal).abs() < 1e-6);
        }
        i.holes.reverse();
        i.patterns[0].members.reverse();
        assert_eq!(rs, plan(&i).0);
    }
}

#[test]
fn unpatterned_counted_sizes_have_multiple_elements_identity() {
    let mut i = input();
    i.holes = vec![hole(1, 10., 20.), hole(2, 31., 40.)];
    let rs = plan(&i).0;
    let size = rs
        .iter()
        .find(|r| matches!(r.kind, Kind::Size { .. }))
        .unwrap();
    assert!(
        matches!(&size.kind, Kind::Size { feature, .. } if feature == "multiple[hole:1,hole:2]")
    );
    assert_eq!(size.count, 2);
    assert!(!rs.iter().any(|r| r.id.contains("pattern[")));
}

#[test]
fn curved_envelope_side_has_one_identity_for_holes_and_overall() {
    for origin in [Origin::Minimum, Origin::Maximum] {
        let mut i = input(); // no principal plane at either curved side
        i.holes = vec![hole(1, 20., 30.)];
        i.policy.origins[0] = origin.clone();
        let rs = plan(&i).0;
        let endpoint = if origin == Origin::Maximum {
            "max"
        } else {
            "min"
        };
        let expected = format!("envelope:0:{endpoint}");
        let from_hole = rs
            .iter()
            .find_map(|r| match &r.kind {
                Kind::Location { from, to, axis: 0 } if to.feature == "hole:1" => Some(from),
                _ => None,
            })
            .unwrap();
        let from_overall = rs
            .iter()
            .find_map(|r| match &r.kind {
                Kind::Location { from, to, axis: 0 } if r.nominal == 100. => {
                    Some(if origin == Origin::Maximum { to } else { from })
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(from_hole.feature, expected);
        assert_eq!(from_hole, from_overall);
    }
}

#[test]
fn zero_length_paths_and_patterns_on_both_origins_never_convey_by_nothing() {
    let (rs, cs) = merged(vec![loc("zero", 0., 0., true)], &[], false);
    assert_eq!(rs.len(), 1);
    assert!(cs.is_empty());
    for arrangement in [Arrangement::Line, Arrangement::Grid] {
        let mut i = input();
        i.bounds[0] = [-100., -100., 0.];
        i.holes = if arrangement == Arrangement::Line {
            vec![hole(1, 0., 0.), hole(2, 10., 0.), hole(3, 20., 0.)]
        } else {
            vec![
                hole(1, -10., -10.),
                hole(2, -10., 10.),
                hole(3, 10., -10.),
                hole(4, 10., 10.),
            ]
        };
        for k in 0..2 {
            let mut s = support(&format!("datum:{k}"), [0.; 3], true);
            s.faces = vec![900 + k];
            i.planes.push(Plane {
                support: s,
                axis: k,
                body: 0,
            });
            i.datums.push(Datum {
                label: format!("D{k}"),
                faces: vec![900 + k],
            });
        }
        i.patterns.push(pattern(&i, arrangement));
        let (rs, cs) = plan(&i);
        assert!(!rs.iter().any(|r| matches!(
            r.pattern,
            Some(PatternRole::Centre | PatternRole::FirstMember)
        )));
        assert!(
            cs.iter()
                .all(|c| !c.by.is_empty() && c.by.iter().all(|id| rs.iter().any(|r| r.id == *id)))
        );
    }
}

#[test]
fn step_and_hole_origins_follow_top_datum_maximum_and_x_max_datum() {
    for choice in 0..3 {
        let mut i = input();
        add_planes(&mut i, 0, &[0., 25., 100.]);
        add_planes(&mut i, 2, &[0., 3., 10.]);
        for (j, p) in i.planes.iter_mut().enumerate() {
            p.support.faces = vec![j];
        }
        i.steps.heights = vec![i.planes[4].support.clone()];
        i.steps.shoulders = vec![i.planes[1].clone()];
        i.holes = vec![hole(1, 20., 30.)];
        // A hole along Y needs both X and Z locations.
        i.holes[0].direction = [0., 1., 0.];
        i.holes[0].support.at[2] = 2.;
        let axis = if choice == 2 { 0 } else { 2 };
        let top = i.planes[if axis == 0 { 2 } else { 5 }].support.clone();
        let nominal = |rs: &[Required], target: &str, axis| {
            rs.iter()
                .find_map(|r| match &r.kind {
                    Kind::Location { to, axis: k, .. } if to.feature == target && *k == axis => {
                        Some(r.nominal)
                    }
                    _ => None,
                })
                .unwrap()
        };
        let baseline = plan(&i).0;
        if choice == 1 {
            i.policy.origins[2] = Origin::Maximum;
        } else {
            i.datums.push(Datum {
                label: "A".into(),
                faces: top.faces,
            });
        }
        let rs = plan(&i).0;
        let target = if axis == 0 {
            &i.steps.shoulders[0].support.feature
        } else {
            &i.steps.heights[0].feature
        };
        assert_eq!(
            nominal(&baseline, "hole:1", axis),
            if axis == 0 { 20. } else { 2. }
        );
        assert_eq!(
            nominal(&rs, "hole:1", axis),
            if axis == 0 { 80. } else { 8. }
        );
        assert_eq!(
            nominal(&baseline, target, axis),
            if axis == 0 { 25. } else { 3. }
        );
        assert_eq!(nominal(&rs, target, axis), if axis == 0 { 75. } else { 7. });
        assert!(
            rs.iter()
                .filter_map(|r| match &r.kind {
                    Kind::Location { from, to, axis: k }
                        if *k == axis && (to.feature == "hole:1" || &to.feature == target) =>
                        Some(from.feature.as_str()),
                    _ => None,
                })
                .all(|id| id == top.feature)
        );
    }
}

#[test]
fn counted_step_ladder_reverses_at_maximum_and_expands_at_interior_origin() {
    let mut i = input();
    i.bounds[1][2] = 40.;
    add_planes(&mut i, 2, &[0., 10., 20., 30., 40.]);
    for (j, p) in i.planes.iter_mut().enumerate() {
        p.support.faces = vec![j];
    }
    i.steps.heights = i.planes[1..4].iter().map(|p| p.support.clone()).collect();
    i.policy.origins[2] = Origin::Maximum;
    let rs = plan(&i).0;
    let repeat = rs.iter().find(|r| r.count == 4).unwrap();
    assert_eq!(repeat.nominal, 10.);
    assert!(
        matches!(&repeat.kind, Kind::Location { from, to, .. } if from.at[2] == 40. && to.at[2] == 30.)
    );
    assert_eq!(rs.len(), 4); // counted repeats are not lint station paths in Draftwright
    i.datums.push(Datum {
        label: "A".into(),
        faces: vec![2],
    });
    i.policy.origins[2] = Origin::Automatic;
    let rs = plan(&i).0;
    assert!(rs.iter().all(|r| r.count == 1));
    assert!(!rs.iter().any(|r| r.nominal == 0.));
}

#[test]
fn slit_segments_count_one_drilling_but_wide_gaps_blind_and_enlarged_bores_do_not() {
    for (gap, blind, enlarged, expected) in [
        (1.2, false, false, 1),
        (48., false, false, 2),
        (1.2, true, false, 2),
        (1.2, false, true, 2),
    ] {
        let mut i = input();
        let mut a = hole(1, 0., 20.);
        a.traversal = Some(8.9);
        a.support.at[2] = 0.;
        a.direction = [0., 0., 1.];
        let mut b = a.clone();
        b.support.feature = "hole:2".into();
        b.support.at[2] = 8.9 + gap;
        for h in [&mut a, &mut b] {
            if blind {
                h.depth = Some(8.9);
                h.traversal = None;
            }
            if enlarged {
                h.details.push(Size {
                    role: "counterbore.diameter".into(),
                    nominal: 12.,
                    kind: SizeKind::Diameter,
                });
            }
        }
        i.holes = vec![a, b];
        let rs = plan(&i).0;
        assert!(
            rs.iter()
                .filter(|r| matches!(r.kind, Kind::Size { .. }))
                .all(|r| r.count == expected)
        );
    }
}
#[test]
fn adapter_keeps_face_identity_and_part_frame_for_real_recognition() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ties/square_hole_grid.step");
    let part = crate::read_step_file(&path).unwrap();
    let i = Input::from_recognition(&part, vec![], Policy::default()).unwrap();
    assert!(!i.holes.is_empty());
    assert!(
        i.holes
            .iter()
            .all(|h| !h.support.faces.is_empty() && h.support.feature.starts_with("hole:"))
    );
    assert_eq!(
        plan(&i),
        plan(&Input::from_recognition(&part, vec![], Policy::default()).unwrap())
    );
    for h in &i.holes {
        let at = i.frame.to_world(h.support.at);
        assert!(at.iter().all(|x| x.is_finite()));
    }
}

#[test]
fn principal_direction_normalizes_and_does_not_swallow_small_inclinations() {
    for (d, expected) in [
        ([0., 0., -1.], Some(2)),
        ([1e-9, 2., 0.], Some(1)),
        ([0., 0.01, 1.], None),
        ([0., 0.001, 1.], None),
        ([0.; 3], None),
    ] {
        assert_eq!(super::rules::principal(d), expected);
    }
}

#[test]
fn merged_point_survivor_retains_its_physical_plane_chain_ownership() {
    for references in [false, true] {
        let (rs, cs) = merged(
            vec![
                loc("a-hole", 0., 10., false),
                loc("a-plane", 0., 10., true),
                loc("b", 10., 30., true),
                loc("c", 0., 30., true),
            ],
            &[],
            references,
        );
        assert_eq!(rs[0].id, "a-hole");
        assert_eq!(rs[0].covers, ["a-hole", "a-plane"]);
        if references {
            assert_eq!(rs.len(), 3);
            assert_eq!(rs[2].reference.as_ref().unwrap(), &["a-hole", "b"]);
        } else {
            assert_eq!(rs.len(), 2);
            assert_eq!(cs.iter().find(|c| c.id == "c").unwrap().by, ["a-hole", "b"]);
        }
    }
}

#[test]
fn an_overall_extent_does_not_claim_to_locate_interior_steps() {
    let mut i = input();
    add_planes(&mut i, 0, &[0., 20., 80., 100.]);
    i.steps.axis = 0;
    i.steps.heights = i.planes[1..3].iter().map(|p| p.support.clone()).collect();
    let rs = plan(&i).0;
    let overall = rs.iter().find(|r| r.nominal == 100.).unwrap();
    assert_eq!(overall.covers, ["plane:0:0", "plane:0:3"]);
    for (nominal, feature) in [(20., "plane:0:1"), (80., "plane:0:2")] {
        assert_eq!(
            rs.iter().find(|r| r.nominal == nominal).unwrap().covers,
            [feature]
        );
    }
}

#[test]
fn pocket_floor_is_deferred_and_never_acquires_a_step_height() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ties/square_pocket.step");
    let part = crate::read_step_file(&path).unwrap();
    let i = Input::from_recognition(&part, vec![], Policy::default()).unwrap();
    let pockets: Vec<_> = i
        .deferred
        .iter()
        .filter(|d| d.family.contains("pocket") || d.family.contains("recess"))
        .collect();
    assert!(!pockets.is_empty());
    assert!(i.steps.heights.is_empty());
    let (rs, cs) = plan(&i);
    assert_eq!(rs.len(), 3); // envelope only
    assert!(pockets.iter().all(|d| {
        cs.iter()
            .any(|c| c.id == d.id && c.by.is_empty() && c.why.starts_with("deferred: owned by "))
    }));
}

#[test]
fn step_ladder_uses_base_heights_and_consolidates_equal_rises() {
    let mut i = input();
    i.bounds[1][2] = 40.;
    add_planes(&mut i, 2, &[0., 10., 20., 30., 40.]);
    i.steps.heights = i.planes[1..4].iter().map(|p| p.support.clone()).collect();
    let rs = plan(&i).0;
    let ladder: Vec<_> = rs.iter().filter(|r| r.count == 4).collect();
    assert_eq!(ladder.len(), 1);
    assert_eq!(ladder[0].nominal, 10.);
    assert_eq!(ladder[0].covers.len(), 3);
    assert_eq!(rs.len(), 4); // three envelope extents plus a counted rise
    i.steps.heights[1].at[2] = 23.;
    let rs = plan(&i).0;
    assert_eq!(rs.len(), 6);
    assert!(rs.iter().any(|r| r.nominal == 23. && r.count == 1));
}

#[test]
fn arbitrary_interior_planes_are_not_step_requirements() {
    let mut i = input();
    for k in 0..3 {
        let top = i.bounds[1][k];
        add_planes(&mut i, k, &[0., 2., top]);
    }
    assert_eq!(plan(&i).0.len(), 3);
}

#[test]
fn turned_body_envelope_is_explicitly_deferred() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/turned_body_disc.step");
    let part = crate::read_step_file(&path).unwrap();
    let i = Input::from_recognition(&part, vec![], Policy::default()).unwrap();
    assert!(!i.cartesian_envelope);
    let (rs, cs) = plan(&i);
    assert!(rs.iter().all(|r| matches!(r.kind, Kind::Size { .. })
        || r.covers.iter().all(|c| !c.starts_with("envelope:"))));
    assert_eq!(
        cs.iter()
            .filter(|c| c.id.starts_with("deferred:envelope:"))
            .count(),
        3
    );
}

#[test]
fn recognition_keeps_the_unowned_step_height_in_a_mixed_part() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/golden_plates_pads_levels_and_slanted_steps.step");
    let part = crate::read_step_file(&path).unwrap();
    let i = Input::from_recognition(&part, vec![], Policy::default()).unwrap();
    assert_eq!(i.steps.heights.len(), 1);
    assert_eq!(i.steps.heights[0].faces, [14]);
    let height = i.steps.heights[0].at[2] - i.bounds[0][2];
    assert!((height - 16.).abs() < 1e-6);
    assert!(plan(&i).0.iter().any(
        |r| (r.nominal - height).abs() < 1e-6 && r.covers.contains(&i.steps.heights[0].feature)
    ));
    assert!(i.deferred.iter().any(|d| d.family == "pads"));
    assert!(
        i.deferred
            .iter()
            .any(|d| d.family.contains("pocket") || d.family.contains("recess"))
    );
}

#[test]
fn cylindrical_boss_caps_are_deferred_with_their_consulted_faces() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/revisions/boss_resized.old.step.gz");
    let part = crate::read_step_file(&path).unwrap();
    let i = Input::from_recognition(&part, vec![], Policy::default()).unwrap();
    let local = part.transformed(&i.frame.normalisation()).unwrap();
    let found = crate::features::recognise(&local);
    assert!(!found.bosses.is_empty());
    for (boss, defining) in found.bosses.iter().zip(&found.defining["bosses"]) {
        let axis = super::rules::principal(boss.axis).unwrap();
        let cap = local
            .faces
            .iter()
            .enumerate()
            .find_map(|(f, face)| {
                let crate::kernel::geom::Surface::Plane { frame } = &face.surface else {
                    return None;
                };
                (super::rules::principal(frame.z) == Some(axis)
                    && (frame.origin[axis] - boss.location[axis]).abs() < 1e-6)
                    .then_some(f)
            })
            .expect("the fixture's cylindrical boss has a planar cap");
        assert!(
            !defining.contains(&cap),
            "cap is consulted rather than defining"
        );
        assert!(
            i.deferred
                .iter()
                .any(|d| d.family == "bosses" && d.faces.contains(&cap))
        );
        assert!(i.steps.heights.iter().all(|s| !s.faces.contains(&cap)));
        assert!(
            i.steps
                .shoulders
                .iter()
                .all(|p| !p.support.faces.contains(&cap))
        );
    }
}

#[test]
fn annular_counterbore_floor_is_owned_by_the_hole_depth_not_the_ladder() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/golden_counterbored_and_countersunk_holes.step");
    let part = crate::read_step_file(&path).unwrap();
    let i = Input::from_recognition(&part, vec![], Policy::default()).unwrap();
    assert!(
        i.holes
            .iter()
            .any(|h| h.details.iter().any(|s| s.role == "counterbore.depth"))
    );
    assert!(i.steps.heights.is_empty());
    assert!(
        plan(&i)
            .0
            .iter()
            .any(|r| matches!(&r.kind,Kind::Size {role,..} if role=="counterbore.depth"))
    );
}

#[test]
fn oblique_counterbore_depth_starts_at_the_axis_entry_plane_intersection() {
    use crate::kernel::geom::{Surface, dot, sub};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ap242/nist/nist_ctc_02_asme1_ap242-e2.stp.gz");
    let part = crate::read_step_file(&path).unwrap();
    let i = Input::from_recognition(&part, vec![], Policy::default()).unwrap();
    let hole = i
        .holes
        .iter()
        .find(|h| h.support.feature == "hole:98.99.101.102")
        .unwrap();
    let depth = hole
        .details
        .iter()
        .find(|s| s.role == "counterbore.depth")
        .unwrap();
    assert_eq!(depth.nominal, 21.484724);
    assert_eq!(depth.kind, SizeKind::Length);
    let required = plan(&i)
        .0
        .into_iter()
        .find(|r| r.id == "dim:hole:98.99.101.102/counterbore.depth")
        .unwrap();
    assert_eq!(
        required.kind,
        Kind::Size {
            feature: hole.support.feature.clone(),
            role: "counterbore.depth".into(),
            kind: SizeKind::Length,
        }
    );
    assert_eq!(required.covers, std::slice::from_ref(&hole.support.feature));
    assert_eq!(required.count, 1);
    assert_eq!(required.nominal, depth.nominal);

    // Independent geometric construction in file coordinates: the entry is on
    // plane 619 and on the counterbore axis; its deep station is the wall's end.
    let Surface::Plane { frame: entry } = part.faces[619].surface else {
        panic!("entry plane")
    };
    let Surface::Cylinder {
        frame: wall,
        radius,
    } = part.faces[98].surface
    else {
        panic!("counterbore wall")
    };
    assert_eq!(radius, 57.5);
    let axis = sub(
        i.frame.to_world(std::array::from_fn(|k| {
            hole.support.at[k] + hole.direction[k]
        })),
        i.frame.to_world(hole.support.at),
    );
    let cos = dot(entry.z, axis);
    assert!(cos.abs() > 1e-9 && cos.abs() < 0.999999);
    assert!(
        part.face_edges(98)
            .iter()
            .any(|e| part.face_edges(619).contains(e))
    );
    let travel = dot(sub(entry.origin, wall.origin), entry.z) / cos;
    let point = std::array::from_fn(|k| wall.origin[k] + travel * axis[k]);
    assert!(dot(sub(point, entry.origin), entry.z).abs() < 1e-8);
    assert!((point[0] + 203.515275872).abs() < 1e-7);
    assert!((point[1] - 200.).abs() < 1e-7);
    assert!((point[2] + 185.).abs() < 1e-7);
    let end = [98, 99]
        .into_iter()
        .map(|f| part.face_projection_bounds(f, axis).unwrap().1)
        .max_by(f64::total_cmp)
        .unwrap();
    assert!((end - dot(point, axis) - required.nominal).abs() < 0.0000005);
}

#[test]
fn perpendicular_entries_keep_their_recognised_depths() {
    for fixture in [
        "golden_counterbored_and_countersunk_holes.step",
        "flat_floor_controls.step.gz",
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture);
        let part = crate::read_step_file(&path).unwrap();
        let i = Input::from_recognition(&part, vec![], Policy::default()).unwrap();
        let local = part.transformed(&i.frame.normalisation()).unwrap();
        let found = crate::features::recognise(&local);
        assert!(!i.holes.is_empty());
        assert_eq!(i.holes.len(), found.holes.len());
        for (hole, recognised) in i.holes.iter().zip(&found.holes) {
            assert_eq!(
                hole.depth,
                (recognised.bottom != crate::features::holes::Bottom::Through)
                    .then_some(recognised.depth)
            );
            if let Some(cbore) = &recognised.cbore {
                let depth = hole
                    .details
                    .iter()
                    .find(|s| s.role == "counterbore.depth")
                    .unwrap();
                assert_eq!(depth.nominal, 6.);
                assert_eq!(depth.nominal, cbore.depth);
            }
        }
        let depths: Vec<_> = plan(&i)
            .0
            .into_iter()
            .filter(|r| matches!(&r.kind, Kind::Size { role, .. } if role.ends_with(".depth")))
            .map(|r| r.nominal)
            .collect();
        assert_eq!(
            depths,
            if fixture.starts_with("golden_") {
                vec![6.]
            } else {
                vec![8., 8.]
            }
        );
    }
}

#[test]
fn counted_repeats_are_not_exact_station_edges_in_draftwright_lint() {
    let mut repeat = loc("a-repeat", 0., 10., true);
    repeat.count = 4;
    let (rs, cs) = merged(
        vec![
            repeat,
            loc("b-tail", 10., 40., true),
            loc("c-overall", 0., 40., true),
        ],
        &[],
        true,
    );
    assert_eq!(rs.len(), 3);
    assert!(rs.iter().all(|r| r.reference.is_none()));
    assert!(cs.is_empty());
}

#[test]
fn inclined_hex_recess_walls_are_not_missing_step_levels_10038() {
    let Some(corpus) = std::env::var_os("QUIDDITY_CORPUS") else {
        return;
    };
    let path = std::path::PathBuf::from(corpus).join("mfcadpp/10038.step");
    let part = crate::read_step_file(&path).unwrap();
    let i = Input::from_recognition(&part, vec![], Policy::default()).unwrap();
    assert_eq!(i.steps.heights.len(), 1);
    assert_eq!(i.steps.heights[0].faces, [4]);
    let height = i.steps.heights[0].at[2] - i.bounds[0][2];
    assert!((height - 7.167971980549).abs() < 1e-6);
    for face in [45, 48] {
        let d = i
            .deferred
            .iter()
            .find(|d| d.family == "section_recesses" && d.faces.contains(&face))
            .unwrap();
        assert!(plan(&i).1.iter().any(|c| c.id == d.id && c.by.is_empty()));
        assert!(i.steps.heights.iter().all(|s| !s.faces.contains(&face)));
    }
}
