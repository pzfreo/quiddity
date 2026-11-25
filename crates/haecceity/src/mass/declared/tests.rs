use super::*;
const SQUARE: &str = include_str!("../../../tests/fixtures/master-trim-square.step");

fn entity(text: &str, id: usize, body: &str) -> String {
    let prefix = format!("#{id}=");
    let line = text.lines().find(|l| l.starts_with(&prefix)).unwrap();
    text.replace(line, &format!("{prefix}{body};"))
}

fn plane() -> String {
    entity(SQUARE, 108, "PLANE('',#11)")
}

#[test]
fn a_certified_trim_has_opposite_projections_from_opposite_sides() {
    let part = crate::step::read_step(SQUARE.as_bytes()).unwrap();
    let (up, centre) = part.face_projected_moments(0, [0., 0., 1.]).unwrap();
    let (down, other) = part.face_projected_moments(0, [0., 0., -1.]).unwrap();
    assert_eq!(
        part.cache[0].mass.get().unwrap().method,
        BoundaryMethod::CertifiedPatch
    );
    assert!((up.abs() - (1. - 0.0004 / 3.)).abs() < 1e-12);
    assert_eq!(up, -down);
    assert_eq!(centre, other);
}

#[test]
fn implicit_knot_families_keep_their_parameter_curve() {
    for kind in ["BEZIER_CURVE", "QUASI_UNIFORM_CURVE"] {
        let file = entity(
            &plane(),
            115,
            &format!("{kind}('',2,(#112,#113,#114),.UNSPECIFIED.,.F.,.F.)"),
        );
        let p = crate::step::read_step(file.as_bytes()).unwrap();
        assert!(
            (p.face_mass(0).unwrap()[0] - (1. - 0.0004 / 3.)).abs() < 1e-12,
            "{kind}"
        );
    }
    let file = entity(
        &plane(),
        127,
        "UNIFORM_CURVE('',1,(#124,#136),.UNSPECIFIED.,.F.,.F.)",
    );
    let p = crate::step::read_step(file.as_bytes()).unwrap();
    assert_eq!(p.measured_boundary(0).unwrap().edges.len(), 4);
    assert!((p.face_mass(0).unwrap()[0] - (1. - 0.0004 / 3.)).abs() < 1e-12);
}

#[test]
fn a_rounded_pcurve_endpoint_must_be_within_the_physical_coordinate_floor() {
    let outside = -3.7e-14;
    for (width, expected) in [(1., Some((0., 0.5))), (1e9, None)] {
        let text = entity(
            SQUARE,
            101,
            &format!("CARTESIAN_POINT('',({width:.1},0.,0.))"),
        );
        let text = entity(
            &text,
            102,
            &format!("CARTESIAN_POINT('',({width:.1},1.,0.))"),
        );
        let part = crate::step::read_step(text.as_bytes()).unwrap();
        let surface = &part.faces[0].surface;
        assert_eq!(
            on_carrier(surface, closed_spans(surface), [outside, 0.5, 0.]),
            expected
        );
    }
}

#[test]
fn the_decimal_end_of_the_carrier_band_is_inclusive() {
    let part = crate::step::read_step(SQUARE.as_bytes()).unwrap();
    let surface = &part.faces[0].surface;
    let spans = closed_spans(surface);
    let rounded: f64 = 1.000_000_000_001;
    assert_eq!(
        on_carrier(surface, spans, [rounded, 0.5, 0.]),
        Some((1., 0.5))
    );
    assert!(on_carrier(surface, spans, [rounded.next_up(), 0.5, 0.]).is_none());
}

#[test]
fn a_foreign_pcurve_cannot_be_clamped_to_its_carriers_boundary() {
    let text = SQUARE
        .replace("(0.5,0.0004)", "(0.5,-0.0004)")
        .replace("#21=OPEN_SHELL('',(#159))", "#201=PLANE('',#11);\n#202=ADVANCED_FACE('',(#158),#201,.T.);\n#21=OPEN_SHELL('',(#159,#202))");
    let part = crate::step::read_step(text.as_bytes()).unwrap();
    assert_eq!(part.faces.len(), 2);
    for f in 0..2 {
        assert!(part.measured_boundary(f).is_none());
        assert!((part.face_mass(f).unwrap()[0] - 1.).abs() < 1e-12);
    }
}

#[test]
fn master_on_another_surface_is_lifted_in_its_own_frame() {
    let extra = "#201=AXIS2_PLACEMENT_3D('',#12,#13,#121);\n#202=PLANE('',#201);\n#203=PCURVE('',#202,#116);\n#204=ADVANCED_FACE('',(#158),#202,.T.);\n";
    let mut text = plane().replace("#111=LINE", &format!("{extra}#111=LINE"));
    text = entity(&text, 113, "CARTESIAN_POINT('',(0.0004,-0.5))");
    text = entity(&text, 114, "CARTESIAN_POINT('',(0.,-1.))");
    text = entity(&text, 118, "SURFACE_CURVE('',#111,(#117,#203),.PCURVE_S2.)");
    let expected = 1. - 0.0004 / 3.;
    for has_carrier_face in [false, true] {
        let file = if has_carrier_face {
            entity(&text, 21, "OPEN_SHELL('',(#159,#204))")
        } else {
            text.clone()
        };
        let part = crate::step::read_step(file.as_bytes()).unwrap();
        for f in 0..part.faces.len() {
            let a = part.face_mass(f).unwrap()[0];
            assert!(
                (a - expected).abs() < 1e-11,
                "face {f}, carrier face {has_carrier_face}: {a}"
            );
        }
    }
    let off = text.replace(
        "#201=AXIS2_PLACEMENT_3D('',#12",
        "#205=CARTESIAN_POINT('',(0.,0.,0.01));\n#201=AXIS2_PLACEMENT_3D('',#205",
    );
    let rejected = crate::step::read_step(off.as_bytes()).unwrap();
    assert!(rejected.measured_boundary(0).is_none());
    assert!((rejected.face_mass(0).unwrap()[0] - 1.).abs() < 1e-12);
}

#[test]
fn detached_native_spline_supplies_its_own_master() {
    let file = SQUARE.replace(
        "#159=ADVANCED_FACE('',(#158),#108",
        "#201=PLANE('',#11);\n#159=ADVANCED_FACE('',(#158),#201",
    );
    let bezier = entity(
        &file,
        108,
        "BEZIER_SURFACE('',1,1,((#100,#103),(#101,#102)),.UNSPECIFIED.,.F.,.F.,.F.)",
    );
    let rational = entity(
        &file,
        108,
        "(BOUNDED_SURFACE() B_SPLINE_SURFACE(1,1,((#100,#103),(#101,#102)),.UNSPECIFIED.,.F.,.F.,.F.) B_SPLINE_SURFACE_WITH_KNOTS((2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.) GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_SURFACE(((1.,1.),(1.,1.))) REPRESENTATION_ITEM('') SURFACE())",
    );
    let expected = 1. - 0.0004 / 3.;
    for input in [file, bezier, rational] {
        let part = crate::step::read_step(input.as_bytes()).unwrap();
        assert_eq!(part.measured_boundary(0).unwrap().edges.len(), 4);
        assert!((part.face_mass(0).unwrap()[0] - expected).abs() < 1e-11);
    }
}

#[test]
fn selected_circle_on_a_plane_keeps_its_own_parameter_interval() {
    let mut text = plane();
    let extra = "#201=CARTESIAN_POINT('',(0.5,0.5,0.));\n#202=AXIS2_PLACEMENT_3D('',#201,#13,#14);\n#203=CARTESIAN_POINT('',(0.5,0.5));\n#204=AXIS2_PLACEMENT_2D('',#203,$);\n";
    text = text.replace("#111=LINE", &format!("{extra}#111=LINE"));
    text = entity(&text, 100, "CARTESIAN_POINT('',(0.75,0.5,0.))");
    text = entity(&text, 111, "CIRCLE('',#202,0.25005)");
    text = entity(&text, 115, "CIRCLE('',#204,0.25)");
    text = entity(&text, 119, "EDGE_CURVE('',#104,#104,#118,.T.)");
    text = entity(&text, 157, "EDGE_LOOP('',(#120))");
    let p = crate::step::read_step(text.as_bytes()).unwrap();
    let a = p.face_mass(0).unwrap()[0];
    assert!((a - std::f64::consts::PI / 16.).abs() < 1e-12, "{a}");
    let m = p.face_moments(0).unwrap();
    assert!(geom::dist(m.centroid, [0.5, 0.5, 0.]) < 1e-12);
}

#[test]
fn inferred_pcurve_intervals_keep_the_spatial_arc_branch() {
    // A unit circle on a cylindrical carrier, from pi/2 to 2*pi in the positive sense.
    // The selected spline spans the whole circle at v=0, with a different knot origin.
    // Both ends of the spline lift to the endpoint at angle zero on the planar face.
    let mut text = plane().replace(
        "#108=PLANE",
        "#201=CYLINDRICAL_SURFACE('',#11,1.);\n#108=PLANE",
    );
    for (id, body) in [
        (100, "CARTESIAN_POINT('',(0.,1.,0.))"),
        (101, "CARTESIAN_POINT('',(1.,0.,0.))"),
        (111, "CIRCLE('',#11,1.)"),
        (114, "CARTESIAN_POINT('',(6.283185307179586,0.))"),
        (
            115,
            "B_SPLINE_CURVE_WITH_KNOTS('',1,(#112,#114),.UNSPECIFIED.,.F.,.F.,(2,2),(6.283185307179586,12.566370614359172),.PIECEWISE_BEZIER_KNOTS.)",
        ),
        (117, "PCURVE('',#201,#116)"),
        (121, "DIRECTION('',(-1.,1.,0.))"),
        (131, "EDGE_CURVE('',#105,#104,#123,.T.)"),
        (157, "EDGE_LOOP('',(#120,#132))"),
    ] {
        text = entity(&text, id, body);
    }
    // A line parameter in degrees cannot reuse the circle's radian interval.
    let degree_line = entity(&text, 115, "LINE('',#112,#203)").replace(
        "#33=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));",
        "#33=(CONVERSION_BASED_UNIT('degree',#204) NAMED_UNIT(#205) PLANE_ANGLE_UNIT());
#202=DIRECTION('',(1.,0.));
#203=VECTOR('',#202,1.);
#204=PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(0.017453292519943295),#206);
#205=DIMENSIONAL_EXPONENTS(0.,0.,0.,0.,0.,0.,0.);
#206=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));",
    );

    // Three rational quadratic quadrants trace the same arc. This spatial
    // spline has distinct endpoints and no periodic parameter.
    let spline = entity(
        &text,
        111,
        "(BOUNDED_CURVE() B_SPLINE_CURVE(2,(#100,#301,#302,#303,#304,#305,#101),.UNSPECIFIED.,.F.,.F.)
B_SPLINE_CURVE_WITH_KNOTS((3,2,2,3),(0.,2.,4.,6.),.UNSPECIFIED.) CURVE()
GEOMETRIC_REPRESENTATION_ITEM()
RATIONAL_B_SPLINE_CURVE((1.,0.7071067811865476,1.,0.7071067811865476,1.,0.7071067811865476,1.))
REPRESENTATION_ITEM(''))",
    )
    .replace(
        "#111=(BOUNDED_CURVE",
        "#301=CARTESIAN_POINT('',(-1.,1.,0.));
#302=CARTESIAN_POINT('',(-1.,0.,0.));
#303=CARTESIAN_POINT('',(-1.,-1.,0.));
#304=CARTESIAN_POINT('',(0.,-1.,0.));
#305=CARTESIAN_POINT('',(1.,-1.,0.));
#111=(BOUNDED_CURVE",
    );

    // Opposite circle axes make increasing parameters select opposite arcs.
    let opposite = entity(&text, 111, "CIRCLE('',#202,1.)").replace(
        "#201=CYLINDRICAL_SURFACE('',#11,1.);",
        "#201=PLANE('',#11);
#202=AXIS2_PLACEMENT_3D('',#12,#203,#14);
#203=DIRECTION('',(0.,0.,-1.));
#204=AXIS2_PLACEMENT_2D('',#112,$);",
    );
    let opposite = entity(&opposite, 115, "CIRCLE('',#204,1.)");

    // On this nearly full unit disc, the midpoint of the omitted arc is
    // within the file's 1e-3 mm uncertainty of both vertices.
    let gap = 0.1_f64.to_radians();
    let (y, x) = gap.sin_cos();
    let near_closed = entity(
        &text,
        100,
        &format!("CARTESIAN_POINT('',({x:.17},{y:.17},0.))"),
    );
    let near_closed = entity(
        &near_closed,
        121,
        &format!("DIRECTION('',({:.17},{y:.17},0.))", x - 1.),
    );
    let near_area = std::f64::consts::PI - 0.5 * (gap - y);
    let long = 3. * std::f64::consts::FRAC_PI_4 + 0.5;
    let short = std::f64::consts::FRAC_PI_4 - 0.5;
    let differences: Vec<_> = [
        ("bounded spline", text, long),
        ("line in degrees", degree_line, long),
        ("nonperiodic spatial spline", spline, long),
        ("opposite circle axes", opposite, short),
        ("near-closed arc", near_closed, near_area),
    ]
    .into_iter()
    .filter_map(|(name, source, expected)| {
        let part = crate::step::read_step(source.as_bytes()).unwrap();
        let area = part.face_moments(0).unwrap().area;
        ((area - expected).abs() >= 1e-12).then_some((name, area, expected))
    })
    .collect();
    assert!(differences.is_empty(), "{differences:?}");

    // Each rim of a 270-degree cylinder has a full-turn pcurve. The pcurves
    // bow axially only on the omitted quarter, so both possible intervals
    // produce closed loops with different areas.
    let mut patch = entity(
        SQUARE,
        108,
        "CYLINDRICAL_SURFACE('',#11,1.);
#201=CARTESIAN_POINT('',(0.,0.,1.));
#202=AXIS2_PLACEMENT_3D('',#201,#13,#14);
#203=CARTESIAN_POINT('',(0.,0.));
#204=CARTESIAN_POINT('',(0.,0.))",
    );
    for (id, body) in [
        (100, "CARTESIAN_POINT('',(0.,1.,0.))"),
        (101, "CARTESIAN_POINT('',(1.,0.,0.))"),
        (102, "CARTESIAN_POINT('',(1.,0.,1.))"),
        (103, "CARTESIAN_POINT('',(0.,1.,1.))"),
        (111, "CIRCLE('',#11,1.)"),
        (121, "DIRECTION('',(0.,0.,1.))"),
        (131, "EDGE_CURVE('',#105,#106,#123,.T.)"),
        (135, "CIRCLE('',#202,1.)"),
        (143, "EDGE_CURVE('',#106,#107,#142,.F.)"),
        (145, "DIRECTION('',(0.,0.,-1.))"),
        (155, "EDGE_CURVE('',#107,#104,#147,.T.)"),
    ] {
        patch = entity(&patch, id, body);
    }
    let areas = [(4e-5, 1e-7), (2e-3, 1e-3)].map(|(bow, tolerance)| {
        let mut file = entity(
            &patch,
            35,
            &format!("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE({tolerance:.9}),#32,'','')"),
        );
        for (id, [a, b, c, d], z) in [(115, [112, 113, 114, 124], 0.), (139, [136, 148, 203, 204], 1.)] {
            for (point, u, v) in [
                (a, 0., z),
                (b, std::f64::consts::FRAC_PI_4, z + bow),
                (c, std::f64::consts::FRAC_PI_2, z),
                (d, std::f64::consts::TAU, z),
            ] {
                file = entity(&file, point, &format!("CARTESIAN_POINT('',({u:.17},{v:.17}))"));
            }
            file = entity(
                &file,
                id,
                &format!("B_SPLINE_CURVE_WITH_KNOTS('',1,(#{a},#{b},#{c},#{d}),.UNSPECIFIED.,.F.,.F.,(2,1,1,2),(20.,22.,24.,30.),.UNSPECIFIED.)"),
            );
        }
        let part = crate::step::read_step(file.as_bytes()).unwrap();
        part.face_moments(0).unwrap().area
    });
    let area = 3. * std::f64::consts::FRAC_PI_2;
    assert!(areas.iter().all(|a| (a - area).abs() < 1e-12), "{areas:?}");
}

#[test]
fn extrusion_uses_vector_magnitude_and_native_profile_knots() {
    let mut text = entity(SQUARE, 108, "SURFACE_OF_LINEAR_EXTRUSION('',#201,#202)");
    let extra = "#201=B_SPLINE_CURVE_WITH_KNOTS('',1,(#100,#101),.UNSPECIFIED.,.F.,.F.,(2,2),(0.,1.),.PIECEWISE_BEZIER_KNOTS.);\n#202=VECTOR('',#121,2.);\n";
    text = text.replace(
        "#108=SURFACE_OF_LINEAR",
        &format!("{extra}#108=SURFACE_OF_LINEAR"),
    );
    text = entity(&text, 113, "CARTESIAN_POINT('',(0.5,0.0002))");
    text = entity(&text, 136, "CARTESIAN_POINT('',(1.,0.5))");
    text = entity(&text, 148, "CARTESIAN_POINT('',(0.,0.5))");
    let p = crate::step::read_step(text.as_bytes()).unwrap();
    assert!(matches!(
        &p.faces[0].surface,
        Surface::Freeform {
            kind: "EXTRUSION",
            ..
        }
    ));
    assert!(p.measured_boundary(0).is_some());
    let a = p.face_mass(0).unwrap()[0];
    assert!((a - (1. - 0.0004 / 3.)).abs() < 1e-12, "{a}");
}

#[test]
fn selected_area_centroid_and_projected_area_agree_on_mixed_loops() {
    for mixed in [false, true] {
        let mut text = plane();
        if mixed {
            for (id, curve, pcurve) in [(130, 123, 129), (142, 135, 141), (154, 147, 153)] {
                text = entity(
                    &text,
                    id,
                    &format!("SURFACE_CURVE('',#{curve},(#{pcurve}),.CURVE_3D.)"),
                );
            }
        }
        let p = crate::step::read_step(text.as_bytes()).unwrap();
        let expected = 1. - 0.0004 / 3.;
        let mass = p.face_mass(0).unwrap();
        assert!((mass[0] - expected).abs() < 1e-12, "{mass:?}");
        let m = p.face_moments(0).unwrap();
        assert_eq!(m.area, mass[0]);
        let (projected, centre) = p.face_projected_moments(0, [0., 0., 1.]).unwrap();
        assert!((projected.abs() - expected).abs() < 1e-12);
        assert!(geom::dist(centre, m.centroid) < 1e-12);
    }
}

#[test]
fn mixed_and_transferred_edges_measure_the_same_simple_region() {
    for (height, area, usable) in [("0.04", 1. - 0.04 / 3., true), ("2.4", 1., false)] {
        let local = plane().replace("(0.5,0.0004)", &format!("(0.5,{height})"));
        let spatial = entity(&local, 142, "SURFACE_CURVE('',#135,(#141),.CURVE_3D.)");
        let other_plane = local.replace(
            "#117=PCURVE('',#108",
            "#197=PLANE('',#11);\n#117=PCURVE('',#197",
        );
        // At height 2.4 the lower quadratic crosses the upper edge twice.
        // Its spatial alternate is the straight lower side of the unit square.
        for text in [local, spatial, other_plane] {
            let part = crate::step::read_step(text.as_bytes()).unwrap();
            assert_eq!(part.measured_boundary(0).is_some(), usable);
            assert!((part.face_mass(0).unwrap()[0] - area).abs() < 1e-12);
        }
    }
}

#[test]
fn a_declared_boundary_can_enclose_a_spatial_hole() {
    let mut extra = String::new();
    for (i, (x, y), line) in [
        (0, (0.25, 0.25), 110),
        (1, (0.75, 0.25), 122),
        (2, (0.75, 0.75), 134),
        (3, (0.25, 0.75), 146),
    ] {
        let (p, v, c, e, o, next) = (
            200 + i,
            204 + i,
            208 + i,
            212 + i,
            216 + i,
            204 + (i + 1) % 4,
        );
        extra += &format!(
            "#{p}=CARTESIAN_POINT('',({x},{y},0.));\n#{v}=VERTEX_POINT('',#{p});\n#{c}=LINE('',#{p},#{line});\n#{e}=EDGE_CURVE('',#{v},#{next},#{c},.T.);\n#{o}=ORIENTED_EDGE('',*,*,#{e},.T.);\n"
        );
    }
    extra += "#220=EDGE_LOOP('',(#216,#217,#218,#219));\n#221=FACE_BOUND('',#220,.F.);\n";
    // The outer pcurve bows inward while the explicit square hole stays fixed.
    let text = plane().replace("(0.5,0.0004)", "(0.5,0.04)").replace(
        "#159=ADVANCED_FACE('',(#158)",
        &(extra + "#159=ADVANCED_FACE('',(#221,#158)"),
    );
    let mislabeled = text
        .replace("#158=FACE_OUTER_BOUND", "#158=FACE_BOUND")
        .replace("#221=FACE_BOUND", "#221=FACE_OUTER_BOUND");
    for (data, marked) in [(text, 1), (mislabeled, 0)] {
        let p = crate::step::read_step(data.as_bytes()).unwrap();
        assert_eq!(p.outer_bounds.get(&0), Some(&marked));
        assert!((p.face_mass(0).unwrap()[0] - (0.75 - 0.04 / 3.)).abs() < 1e-12);
    }
}

#[test]
fn quarter_revolution_ends_at_a_collapsed_pole() {
    // Rotate r = 1 - z through pi/2. Its area is (pi/2) times the integral
    // of sqrt(2) (1 - z) from zero to one, including the collapsed upper end.
    let mut text = entity(SQUARE, 108, "SURFACE_OF_REVOLUTION('',#201,#202)");
    let extra = "#201=B_SPLINE_CURVE_WITH_KNOTS('',1,(#100,#102),.UNSPECIFIED.,.F.,.F.,(2,2),(0.,1.),.PIECEWISE_BEZIER_KNOTS.);\n#202=AXIS1_PLACEMENT('',#12,#13);\n#203=DIRECTION('',(1.,0.));\n#204=VECTOR('',#203,1.);\n";
    text = text.replace(
        "#108=SURFACE_OF_REVOLUTION",
        &format!("{extra}#108=SURFACE_OF_REVOLUTION"),
    );
    for (id, body) in [
        (100, "CARTESIAN_POINT('',(1.,0.,0.))"),
        (101, "CARTESIAN_POINT('',(0.,1.,0.))"),
        (102, "CARTESIAN_POINT('',(0.,0.,1.))"),
        (111, "CIRCLE('',#11,1.)"),
        (112, "CARTESIAN_POINT('',(-6.283185307179586,0.))"),
        (115, "LINE('',#112,#204)"),
        (121, "DIRECTION('',(0.,-1.,1.))"),
        (124, "CARTESIAN_POINT('',(1.5707963267948966,0.))"),
        (133, "DIRECTION('',(1.,0.,-1.))"),
        (136, "CARTESIAN_POINT('',(0.,1.))"),
        (137, "DIRECTION('',(0.,-1.))"),
        (143, "EDGE_CURVE('',#106,#104,#142,.T.)"),
        (157, "EDGE_LOOP('',(#120,#132,#144))"),
    ] {
        text = entity(&text, id, body);
    }
    let p = crate::step::read_step(text.as_bytes()).unwrap();
    let area = p.face_mass(0).unwrap()[0];
    assert!(p.measured_boundary(0).is_some());
    assert!(
        (area - std::f64::consts::FRAC_PI_4 * std::f64::consts::SQRT_2).abs() < 1e-10,
        "{area}"
    );
}

#[test]
fn cylinder_crossing_the_seam_uses_its_declared_curve() {
    // One radian around a unit cylinder and one millimetre in height:
    // the area element is exactly du dv in these coordinates.
    let mut text = entity(SQUARE, 108, "CYLINDRICAL_SURFACE('',#11,1.)");
    for (id, theta, z) in [
        (100, -0.5_f64, 0),
        (101, 0.5, 0),
        (102, 0.5, 1),
        (103, -0.5, 1),
    ] {
        text = entity(
            &text,
            id,
            &format!(
                "CARTESIAN_POINT('',({:.17},{:.17},{z}.))",
                theta.cos(),
                theta.sin()
            ),
        );
    }
    for (id, x, y) in [
        (112, -0.5, 0.),
        (113, 0., 0.0004),
        (114, 0.5, 0.),
        (124, 0.5, 0.),
        (136, 0.5, 1.),
        (148, -0.5, 1.),
    ] {
        text = entity(&text, id, &format!("CARTESIAN_POINT('',({x:.16},{y:.16}))"));
    }
    text = entity(&text, 111, "CIRCLE('',#11,1.)");
    text = entity(&text, 135, "CIRCLE('',#202,1.)");
    text = entity(&text, 143, "EDGE_CURVE('',#106,#107,#142,.F.)");
    text = entity(&text, 121, "DIRECTION('',(0.,0.,1.))");
    text = entity(&text, 145, "DIRECTION('',(0.,0.,-1.))");
    text=text.replace("#108=CYLINDRICAL", "#201=CARTESIAN_POINT('',(0.,0.,1.));\n#202=AXIS2_PLACEMENT_3D('',#201,#13,#14);\n#108=CYLINDRICAL");
    let mut swept = entity(&text, 108, "SURFACE_OF_REVOLUTION('',#207,#206)");
    swept=swept.replace("#108=SURFACE_OF_REVOLUTION","#203=CARTESIAN_POINT('',(1.,0.,0.));\n#205=VECTOR('',#13,1.);\n#206=AXIS1_PLACEMENT('',#12,#13);\n#207=LINE('',#203,#205);\n#108=SURFACE_OF_REVOLUTION");
    // The top spatial circle and the bowed pcurve now cross on both sides of longitude zero.
    let crossed = entity(&text, 113, "CARTESIAN_POINT('',(0.,2.4))");
    let crossed = entity(&crossed, 142, "SURFACE_CURVE('',#135,(#141),.CURVE_3D.)");
    let part = crate::step::read_step(crossed.as_bytes()).unwrap();
    assert!(part.measured_boundary(0).is_none());
    assert!((part.face_mass(0).unwrap()[0] - 1.).abs() < 1e-11);

    for source in [text, swept] {
        let p = crate::step::read_step(source.as_bytes()).unwrap();
        let area = p.face_mass(0).unwrap()[0];
        assert!(
            (area - (1. - 0.0004 / 3.)).abs() < 1e-11,
            "{:?} {area}",
            p.faces[0].surface.kind()
        );
        assert_eq!(area, p.face_moments(0).unwrap().area);
    }
}

#[test]
fn an_unreadable_selected_slot_rejects_the_entire_loop() {
    let text = entity(&plane(), 130, "SURFACE_CURVE('',#123,(#129),.PCURVE_S2.)");
    let part = crate::step::read_step(text.as_bytes()).unwrap();
    assert!(part.measured_boundary(0).is_none());
    assert!((part.face_mass(0).unwrap()[0] - 1.).abs() < 1e-12);
}

#[test]
fn recognition_keeps_agreeing_trims_out_of_the_master_integrator() {
    let part = crate::read_step(SQUARE.as_bytes()).unwrap();
    let recognition = part.recognition_part();
    assert!(part.master_deviations.get().is_none());
    assert!(recognition.master_deviations.get().is_none());

    MASTER_INTEGRATIONS.with(|calls| calls.set(0));
    let projected = recognition.face_moments(0).unwrap().area;
    assert!((projected - 1.).abs() < 1e-12);
    assert_eq!(MASTER_INTEGRATIONS.with(|calls| calls.get()), 0);
    assert!(recognition.master_deviations.get().is_some());
    assert!(part.cache[0].mass.get().is_none());

    // The checks also count against the face budget after the edges are cached.
    let report = |p: &Part| {
        p.with_integration_budget(u64::MAX)
            .integration_budget()
            .unwrap()
            .work[0]
    };
    let spatial = crate::read_step(SQUARE.replace(".PCURVE_S1.", ".CURVE_3D.").as_bytes()).unwrap();
    let edge_work: u64 = recognition
        .master_deviations
        .get()
        .unwrap()
        .iter()
        .filter_map(OnceLock::get)
        .map(|e| e.work_for_test())
        .sum();
    assert!(edge_work > 0);
    assert_eq!(report(recognition), report(&spatial) + edge_work);
    let bounded = recognition.with_integration_budget(1);
    assert_eq!(bounded.integration_budget().unwrap().work, [2]);
    assert_eq!(bounded.integration_budget().unwrap().refused, [0]);
    assert_eq!(MASTER_INTEGRATIONS.with(|calls| calls.get()), 0);

    let master = part.face_moments(0).unwrap().area;
    assert!((master - (1. - 0.0004 / 3.)).abs() < 1e-12);
    assert_eq!(MASTER_INTEGRATIONS.with(|calls| calls.get()), 1);
    assert_eq!(part.face_mass(0).unwrap()[0], master);
    assert_eq!(recognition.face_mass(0).unwrap()[0], projected);
    assert_eq!(MASTER_INTEGRATIONS.with(|calls| calls.get()), 1);

    let mut at = crate::step::IDENTITY;
    at[0][3] = 5.;
    let moved = part.transformed(&at).unwrap();
    assert_eq!(moved.recognition_part().face_mass(0).unwrap()[0], projected);
    assert_eq!(moved.face_mass(0).unwrap()[0], master);
}

#[test]
fn mesh_keeps_curvature_between_a_cylinders_matching_endpoints() {
    let frame = geom::Frame {
        origin: [0.; 3],
        x: [1., 0., 0.],
        y: [0., 1., 0.],
        z: [0., 0., 1.],
    };
    let surface = Surface::Cylinder { frame, radius: 1. };
    let edge = crate::brep::Edge {
        curve: Curve::Line {
            origin: [1., 0., 0.],
            dir: [0., 0., 1.],
        },
        start: [1., 0., 0.],
        end: [1., 0., 1.],
        vertices: (0, 1),
        same_sense: true,
        samples: vec![[1., 0., 0.], [1., 0., 1.]],
    };
    for turns in [0., std::f64::consts::TAU] {
        let curve = ParamCurve {
            source: Arc::new(Curve::Line {
                origin: [0.; 3],
                dir: [turns, 1., 0.],
            }),
            axes: [Axis::ID; 2],
            transfer: None,
        };
        let spatial = edge_interval(
            &edge.curve,
            edge.start,
            edge.end,
            edge.same_sense,
            edge.is_closed(),
        );
        let trim = resolve(
            (&surface, closed_spans(&surface)),
            (&edge, spatial),
            curve,
            geom::COORD_FLOOR,
            true,
            [None; 2],
            Vec::new(),
        )
        .unwrap();
        let samples = trim.samples(&surface, &edge).unwrap();
        assert_eq!(samples.points.first(), Some(&edge.start));
        assert_eq!(samples.points.last(), Some(&edge.end));
        if turns == 0. {
            assert_eq!(samples.points, edge.samples);
        } else {
            // The helix has the same endpoints as the generator. Its interior
            // must still wind around the cylinder rather than become a chord.
            assert!(samples.points.iter().any(|p| p[0] < -0.99));
        }
    }
}

#[test]
fn mesh_rejects_a_foreign_circle_touching_the_receiver_only_at_its_ends() {
    let frame = geom::Frame {
        origin: [0.; 3],
        x: [1., 0., 0.],
        y: [0., 1., 0.],
        z: [0., 0., 1.],
    };
    let receiver = Surface::Plane {
        frame: geom::Frame {
            origin: [1., 0., 0.],
            x: [0., 1., 0.],
            y: [0., 0., 1.],
            z: [1., 0., 0.],
        },
    };
    let edge = crate::brep::Edge {
        curve: Curve::Circle { frame, radius: 1. },
        start: [1., 0., 0.],
        end: [1., 0., 0.],
        vertices: (0, 0),
        same_sense: true,
        samples: vec![[1., 0., 0.], [1., 0., 0.]],
    };
    let curve = ParamCurve {
        source: Arc::new(Curve::Line {
            origin: [0.; 3],
            dir: [1., 0., 0.],
        }),
        axes: [Axis::ID; 2],
        transfer: Some(Box::new(Transfer {
            carrier: Arc::new(Surface::Cylinder { frame, radius: 1. }),
            face: Arc::new(receiver.clone()),
            carrier_spans: (f64::INFINITY, f64::INFINITY),
            face_spans: (f64::INFINITY, f64::INFINITY),
            face_collapsed: (None, None),
            placement: crate::step::IDENTITY,
        })),
    };
    let witness = ParamCurve {
        source: Arc::new(Curve::Line {
            origin: [0.; 3],
            dir: [0.; 3],
        }),
        axes: [Axis::ID; 2],
        transfer: None,
    };
    let spatial = edge_interval(
        &edge.curve,
        edge.start,
        edge.end,
        edge.same_sense,
        edge.is_closed(),
    );
    assert!(
        resolve(
            (&receiver, closed_spans(&receiver)),
            (&edge, spatial),
            curve,
            geom::COORD_FLOOR,
            true,
            [None; 2],
            vec![witness]
        )
        .is_none()
    );
}

#[test]
fn analytic_midpoint_bounds_preserve_adaptive_samples() {
    // Reproduce the original midpoint refinement independently, including its
    // exact parameter arithmetic and endpoint pinning, on every analytic kind.
    let reference = |trim: &EdgeTrim, surface: &Surface, edge: &crate::brep::Edge| {
        let nodes = trim.nodes().unwrap();
        let mut out = vec![(nodes[0], edge.start)];
        for w in nodes.windows(2) {
            let mut pending = vec![(
                w[0],
                trim.point(surface, w[0]),
                w[1],
                trim.point(surface, w[1]),
                0,
            )];
            while let Some((a, p, b, q, depth)) = pending.pop() {
                let t = 0.5 * (a + b);
                let m = trim.point(surface, t);
                if depth < 14
                    && geom::dist(m, geom::scale(geom::add(p, q), 0.5))
                        > crate::sampling::CHORD_TOLERANCE
                {
                    pending.push((t, m, b, q, depth + 1));
                    pending.push((a, p, t, m, depth + 1));
                } else {
                    out.push((b, q));
                }
            }
        }
        out.last_mut().unwrap().1 = edge.end;
        out
    };
    let mut compared = 0;
    let mut settled = 0;
    for placement in [0., 1000.] {
        for distortion in [0., 0.2] {
            let frame = geom::Frame {
                origin: [placement, -0.5 * placement, 0.25 * placement],
                x: [1. + distortion, 0.1, 0.],
                y: [0.2, 1., distortion],
                z: [0., -0.1, 1.],
            };
            let surfaces = [
                Surface::Plane { frame },
                Surface::Cylinder { frame, radius: 2. },
                Surface::Cone {
                    frame,
                    radius: 2.,
                    semi_angle: 0.3,
                },
                Surface::Sphere { frame, radius: 2. },
                Surface::Torus {
                    frame,
                    major: 3.,
                    minor: 0.7,
                },
            ];
            let curve_frame = geom::Frame {
                origin: [0.2, -0.3, 0.],
                ..frame
            };
            let curves = [
                Curve::Line {
                    origin: [0.1, -0.2, 0.],
                    dir: [1.3, -0.7, 0.],
                },
                Curve::Circle {
                    frame: curve_frame,
                    radius: 0.7,
                },
                Curve::Ellipse {
                    frame: curve_frame,
                    major: 1.2,
                    minor: 0.4,
                },
            ];
            for surface in &surfaces {
                for source in &curves {
                    for interval in [(-2.3, 1.7), (1.7, -2.3), (0., 0.025)] {
                        for transferred in [false, true] {
                            let curve = ParamCurve {
                                source: Arc::new(source.clone()),
                                axes: [
                                    Axis::Linear {
                                        scale: -0.7,
                                        offset: 0.3,
                                    },
                                    Axis::Linear {
                                        scale: 1.1,
                                        offset: -0.2,
                                    },
                                ],
                                transfer: transferred.then(|| {
                                    Box::new(Transfer {
                                        carrier: Arc::new(surface.clone()),
                                        face: Arc::new(Surface::Plane { frame }),
                                        carrier_spans: closed_spans(surface),
                                        face_spans: (f64::INFINITY, f64::INFINITY),
                                        face_collapsed: (None, None),
                                        placement: crate::step::IDENTITY,
                                    })
                                }),
                            };
                            assert!(EdgeTrim::midpoint_bound(&curve, surface, interval).is_some());
                            if EdgeTrim::affine_spatial_size(&curve, surface, interval).is_some() {
                                continue;
                            }
                            let trim = EdgeTrim {
                                curve,
                                spans: closed_spans(surface),
                                interval,
                                mesh: true,
                                uniform_mesh_stations: false,
                                witnesses: Vec::new(),
                                samples: OnceLock::new(),
                                nodes: OnceLock::new(),
                                length: OnceLock::new(),
                            };
                            let edge = crate::brep::Edge {
                                curve: source.clone(),
                                start: trim.point(surface, interval.0),
                                end: trim.point(surface, interval.1),
                                vertices: (0, 1),
                                same_sense: interval.0 < interval.1,
                                samples: Vec::new(),
                            };
                            let expected = reference(&trim, surface, &edge);
                            let actual = trim.samples(surface, &edge).unwrap();
                            assert_eq!(
                                actual.ts.iter().map(|t| t.to_bits()).collect::<Vec<_>>(),
                                expected.iter().map(|x| x.0.to_bits()).collect::<Vec<_>>()
                            );
                            assert_eq!(
                                actual
                                    .points
                                    .iter()
                                    .map(|p| p.map(f64::to_bits))
                                    .collect::<Vec<_>>(),
                                expected
                                    .iter()
                                    .map(|x| x.1.map(f64::to_bits))
                                    .collect::<Vec<_>>()
                            );
                            let (curvature, rounding) =
                                EdgeTrim::midpoint_bound(&trim.curve, surface, interval).unwrap();
                            for w in actual.ts.windows(2) {
                                if curvature * (w[1] - w[0]) * (w[1] - w[0]) / 8. + rounding
                                    < 0.99 * crate::sampling::CHORD_TOLERANCE
                                {
                                    let m = trim.point(surface, 0.5 * (w[0] + w[1]));
                                    let chord = geom::scale(
                                        geom::add(
                                            trim.point(surface, w[0]),
                                            trim.point(surface, w[1]),
                                        ),
                                        0.5,
                                    );
                                    assert!(
                                        geom::dist(m, chord) <= crate::sampling::CHORD_TOLERANCE
                                    );
                                    settled += 1;
                                }
                            }
                            compared += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(compared > 300);
    assert!(settled > 1000);
}

#[test]
fn analytic_midpoint_bounds_decline_unresolved_or_unbounded_inputs() {
    let frame = geom::Frame {
        origin: [0.; 3],
        x: [1., 0., 0.],
        y: [0., 1., 0.],
        z: [0., 0., 1.],
    };
    let surface = Surface::Cylinder { frame, radius: 1. };
    let mut curve = ParamCurve {
        source: Arc::new(Curve::Line {
            origin: [0.; 3],
            dir: [1., 1., 0.],
        }),
        axes: [Axis::ID; 2],
        transfer: None,
    };
    for interval in [(f64::NAN, 1.), (0., f64::INFINITY), (0., 1e200)] {
        assert!(EdgeTrim::midpoint_bound(&curve, &surface, interval).is_none());
    }
    assert!(
        EdgeTrim::midpoint_bound(&curve, &Surface::Other { kind: "unresolved" }, (0., 1.))
            .is_none()
    );
    curve.axes[0] = Axis::Angle { radians: 1. };
    assert!(EdgeTrim::midpoint_bound(&curve, &surface, (0., 1.)).is_none());
    curve.axes[0] = Axis::ID;
    for bad in [f64::NAN, f64::INFINITY, 1e100] {
        curve.source = Arc::new(Curve::Line {
            origin: [bad, 0., 0.],
            dir: [1., 1., 0.],
        });
        assert!(EdgeTrim::midpoint_bound(&curve, &surface, (0., 1.)).is_none());
    }
}

#[test]
fn selected_curve_pool_preserves_operand_units_and_part_identity() {
    let make = |p: Vec<V3>, w, k, degree| {
        Arc::new(Curve::Nurbs(
            crate::nurbs::NurbsCurve::new(degree, p, w, k).unwrap(),
        ))
    };
    let p = vec![[0., 0., 0.], [1., 0.5, 0.]];
    let w = vec![1., 1.];
    let k = vec![0., 0., 1., 1.];
    let source = || make(p.clone(), w.clone(), k.clone(), 1);
    let pool = SelectedCurvePool::default();
    let scales = [1_f64.to_bits(); 2];
    let shared = pool.intern(source(), scales);
    assert!(Arc::ptr_eq(&shared, &pool.intern(source(), scales)));
    assert!(!Arc::ptr_eq(
        &shared,
        &SelectedCurvePool::default().intern(source(), scales)
    ));
    for units in [
        [2_f64.to_bits(), scales[1]],
        [scales[0], 1_f64.next_up().to_bits()],
    ] {
        assert!(!Arc::ptr_eq(&shared, &pool.intern(source(), units)));
    }
    let mut moved = p.clone();
    moved[0][0] = 0_f64.next_up();
    let mut signed = p.clone();
    signed[0][0] = -0.;
    let mut weights = w.clone();
    weights[0] = 1_f64.next_up();
    let mut knots = k.clone();
    knots[0] = -f64::EPSILON;
    for different in [
        make(moved, w.clone(), k.clone(), 1),
        make(signed, w.clone(), k.clone(), 1),
        make(p.clone(), weights, k.clone(), 1),
        make(p.clone(), w.clone(), knots, 1),
        make(
            vec![p[0], [0.5, 0.25, 0.], p[1]],
            vec![1.; 3],
            vec![0., 0., 0., 1., 1., 1.],
            2,
        ),
    ] {
        assert!(!Arc::ptr_eq(&shared, &pool.intern(different, scales)));
    }
    for t in [-0.25, 0., 0.1, 0.5, 0.9, 1., 1.25] {
        let independent = source();
        let target = [t, 0.4 * t, 0.2];
        assert_eq!(
            shared.parameter(target).to_bits(),
            independent.parameter(target).to_bits()
        );
        assert_eq!(
            shared.value(t).map(f64::to_bits),
            independent.value(t).map(f64::to_bits)
        );
        assert_eq!(
            shared.derivative(t).map(f64::to_bits),
            independent.derivative(t).map(f64::to_bits)
        );
    }
}

#[test]
fn selected_curve_pool_keeps_concurrent_inverse_queries_independent() {
    let pool = SelectedCurvePool::default();
    let source = || {
        Arc::new(Curve::Nurbs(
            crate::nurbs::NurbsCurve::new(
                2,
                vec![[0., 0., 0.], [0.5, 0.75, 0.], [1., 0., 0.]],
                vec![1., 0.8, 1.],
                vec![0., 0., 0., 1., 1., 1.],
            )
            .unwrap(),
        ))
    };
    let results = std::thread::scope(|scope| {
        (0..8)
            .map(|i| {
                let source = &source;
                let pool = &pool;
                scope.spawn(move || {
                    let curve = pool.intern(source(), [1_f64.to_bits(); 2]);
                    let target = [i as f64 / 7., 0.1, 0.];
                    assert_eq!(
                        curve.parameter(target).to_bits(),
                        source().parameter(target).to_bits()
                    );
                    curve
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(results.iter().all(|curve| Arc::ptr_eq(curve, &results[0])));
}

#[test]
fn selected_curve_pool_poisoning_only_disables_reuse() {
    let pool = SelectedCurvePool::default();
    let curve = Arc::new(Curve::Nurbs(
        crate::nurbs::NurbsCurve::new(
            1,
            vec![[0.; 3], [1., 0., 0.]],
            vec![1.; 2],
            vec![0., 0., 1., 1.],
        )
        .unwrap(),
    ));
    let failure = std::panic::catch_unwind(|| {
        let _guard = pool.0.lock().unwrap();
        panic!("interrupt a cache insertion");
    });
    assert!(failure.is_err());
    let reused = pool.intern(curve.clone(), [1_f64.to_bits(); 2]);
    assert!(Arc::ptr_eq(&curve, &reused));
    assert_eq!(reused.parameter([0.5, 0., 0.]), 0.5);
}
