//! For #127: procedural analytic rounds and the pinned CTC-05 pocket treatments.
use super::common;
use quiddity::features::{Context, fillets};
use quiddity::kernel::brep::{Edge, Face, Loop, Part, Solid};
use quiddity::kernel::geom::{self, Curve, Frame, Surface, V3};

const XYZ: [V3; 3] = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];

#[derive(Default)]
struct Builder {
    faces: Vec<Face>,
    edges: Vec<Edge>,
    points: Vec<V3>,
}
impl Builder {
    fn vertex(&mut self, p: V3) -> usize {
        if let Some(i) = self
            .points
            .iter()
            .position(|&q| geom::norm(geom::sub(p, q)) < 1e-9)
        {
            i
        } else {
            self.points.push(p);
            self.points.len() - 1
        }
    }
    // Each boundary segment is straight, or a minor circular arc about its centre.
    fn patch(
        &mut self,
        surface: Surface,
        inward: bool,
        solid: usize,
        points: &[V3],
        centres: &[Option<V3>],
    ) {
        let mut uses = Vec::new();
        let mut walk = Vec::new();
        for i in 0..points.len() {
            let (p, q) = (points[i], points[(i + 1) % points.len()]);
            let (a, b) = (self.vertex(p), self.vertex(q));
            let existing = self
                .edges
                .iter()
                .position(|e| e.vertices == (a, b) || e.vertices == (b, a));
            let e = existing.unwrap_or_else(|| {
                let (curve, samples) = if let Some(c) = centres[i] {
                    let x = geom::unit(geom::sub(p, c)).unwrap();
                    let end = geom::unit(geom::sub(q, c)).unwrap();
                    let z = geom::unit(geom::cross(x, end)).unwrap();
                    let frame = Frame {
                        origin: c,
                        x,
                        y: geom::cross(z, x),
                        z,
                    };
                    let radius = geom::norm(geom::sub(p, c));
                    let angle = geom::norm(geom::cross(x, end)).atan2(geom::dot(x, end));
                    let curve = Curve::Circle { frame, radius };
                    let samples = (0..=32)
                        .map(|j| curve.value(angle * j as f64 / 32.))
                        .collect();
                    (curve, samples)
                } else {
                    (
                        Curve::Line {
                            origin: p,
                            dir: geom::sub(q, p),
                        },
                        vec![p, q],
                    )
                };
                self.edges.push(Edge {
                    curve,
                    start: p,
                    end: q,
                    vertices: (a, b),
                    same_sense: true,
                    samples,
                });
                self.edges.len() - 1
            });
            let forward = self.edges[e].vertices == (a, b);
            let mut samples = self.edges[e].samples.clone();
            if !forward {
                samples.reverse();
            }
            walk.extend(samples);
            uses.push((e, forward));
        }
        let centre = geom::scale(
            walk.iter().copied().fold([0.; 3], geom::add),
            1. / walk.len() as f64,
        );
        let normal = match surface {
            Surface::Plane { frame } => frame.z,
            Surface::Cylinder { frame, .. } => {
                let d = geom::sub(centre, frame.origin);
                geom::sub(d, geom::scale(frame.z, geom::dot(d, frame.z)))
            }
            Surface::Sphere { frame, .. } => geom::sub(centre, frame.origin),
            _ => unreachable!(),
        };
        let area = (0..walk.len())
            .map(|i| {
                geom::cross(
                    geom::sub(walk[i], centre),
                    geom::sub(walk[(i + 1) % walk.len()], centre),
                )
            })
            .fold([0.; 3], geom::add);
        if (geom::dot(area, normal) < 0.) != inward {
            uses.reverse();
            for (_, forward) in &mut uses {
                *forward = !*forward;
            }
        }
        self.faces.push(Face {
            surface,
            reversed: inward,
            solid: Some(solid),
            loops: vec![Loop {
                edges: uses,
                vertex: None,
            }],
            pcurves: vec![],
        });
    }
    fn planes(&mut self, origin: V3, half: f64, radius: f64, inward: bool, solid: usize) {
        for (a, &axis) in XYZ.iter().enumerate() {
            for sign in [-1., 1.] {
                let (b, c) = ((a + 1) % 3, (a + 2) % 3);
                let mut centre = origin;
                centre[a] += sign * (half + radius);
                let points: Vec<_> = [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)]
                    .into_iter()
                    .map(|(s, t)| {
                        let mut p = centre;
                        p[b] += s * half;
                        p[c] += t * half;
                        p
                    })
                    .collect();
                self.patch(
                    Surface::Plane {
                        frame: Frame {
                            origin: centre,
                            x: XYZ[b],
                            y: geom::scale(XYZ[c], sign),
                            z: geom::scale(axis, sign),
                        },
                    },
                    inward,
                    solid,
                    &points,
                    &[None; 4],
                );
            }
        }
    }
    fn rounded_box(&mut self, origin: V3, inward: bool, solid: usize) {
        let (h, r) = (2., 1.);
        self.planes(origin, h, r, inward, solid);
        for (a, &axis) in XYZ.iter().enumerate() {
            for sb in [-1., 1.] {
                for sc in [-1., 1.] {
                    let (b, c) = ((a + 1) % 3, (a + 2) % 3);
                    let mut centre = origin;
                    centre[b] += sb * h;
                    centre[c] += sc * h;
                    let x = geom::scale(XYZ[b], sb);
                    let y = geom::scale(XYZ[c], sc);
                    let frame = Frame {
                        origin: centre,
                        x,
                        y,
                        z: geom::cross(x, y),
                    };
                    let lo = geom::sub(centre, geom::scale(axis, h));
                    let hi = geom::add(centre, geom::scale(axis, h));
                    self.patch(
                        Surface::Cylinder { frame, radius: r },
                        inward,
                        solid,
                        &[
                            geom::add(lo, x),
                            geom::add(hi, x),
                            geom::add(hi, y),
                            geom::add(lo, y),
                        ],
                        &[None, Some(hi), None, Some(lo)],
                    );
                }
            }
        }
        for sx in [-1., 1.] {
            for sy in [-1., 1.] {
                for sz in [-1., 1.] {
                    let signs = [sx, sy, sz];
                    let centre = geom::add(origin, signs.map(|s| s * h));
                    let points: Vec<_> = (0..3)
                        .map(|a| geom::add(centre, geom::scale(XYZ[a], signs[a] * r)))
                        .collect();
                    self.patch(
                        Surface::Sphere {
                            frame: Frame {
                                origin: centre,
                                x: XYZ[0],
                                y: XYZ[1],
                                z: XYZ[2],
                            },
                            radius: r,
                        },
                        inward,
                        solid,
                        &points,
                        &[Some(centre); 3],
                    );
                }
            }
        }
    }
    fn drafted_notch(&mut self, solid: usize) -> usize {
        let theta = 82_f64.to_radians();
        let (s, c) = theta.sin_cos();
        let origin = [60., 0., 0.];
        let vertices = [
            [c, s, 0.],
            [1., 0., 0.],
            [1., -5., 0.],
            [10., -5., 0.],
            [10., 10., 0.],
            [-10., 10., 0.],
            [c - 5. * s, s + 5. * c, 0.],
        ];
        let low: Vec<_> = vertices.into_iter().map(|p| geom::add(origin, p)).collect();
        let high: Vec<_> = low.iter().map(|&p| geom::add(p, [0., 0., 4.])).collect();
        for (points, z) in [(&low, -1.), (&high, 1.)] {
            let mut centres = vec![None; low.len()];
            centres[0] = Some([60., 0., if z < 0. { 0. } else { 4. }]);
            self.patch(
                Surface::Plane {
                    frame: Frame {
                        origin: points[0],
                        x: XYZ[0],
                        y: geom::scale(XYZ[1], z),
                        z: geom::scale(XYZ[2], z),
                    },
                },
                false,
                solid,
                points,
                &centres,
            );
        }
        let round = self.faces.len();
        for i in 0..low.len() {
            let j = (i + 1) % low.len();
            let points = [low[i], low[j], high[j], high[i]];
            if i == 0 {
                self.patch(
                    Surface::Cylinder {
                        frame: Frame {
                            origin,
                            x: XYZ[0],
                            y: XYZ[1],
                            z: XYZ[2],
                        },
                        radius: 1.,
                    },
                    true,
                    solid,
                    &points,
                    &[Some(origin), None, Some([60., 0., 4.]), None],
                );
            } else {
                let x = geom::unit(geom::sub(low[j], low[i])).unwrap();
                let z = geom::cross(x, XYZ[2]);
                self.patch(
                    Surface::Plane {
                        frame: Frame {
                            origin: low[i],
                            x,
                            y: XYZ[2],
                            z,
                        },
                    },
                    false,
                    solid,
                    &points,
                    &[None; 4],
                );
            }
        }
        round
    }
    fn part(self) -> Part {
        let count = self.faces.iter().map(|f| f.solid.unwrap()).max().unwrap() + 1;
        let solids = (0..count)
            .map(|s| Solid {
                faces: (0..self.faces.len())
                    .filter(|&f| self.faces[f].solid == Some(s))
                    .collect(),
            })
            .collect();
        Part::new(self.faces, self.edges, solids)
    }
}

#[test]
fn procedural_drafted_inside_round_spherical_corners_and_unchanged_convex_rounds() {
    let mut b = Builder::default();
    b.planes([0.; 3], 8., 0., false, 0);
    b.rounded_box([0.; 3], true, 0);
    b.rounded_box([30., 0., 0.], false, 1);
    let drafted = b.drafted_notch(2);
    let part = b.part();
    for s in 0..3 {
        assert!(
            part.solid_is_geometrically_valid(s),
            "solid {s}: {:?}",
            part.bad_faces(s)
        );
    }
    let rounds = fillets::discover_verified(&Context::new(&part), &Default::default()).unwrap();
    let inside: Vec<_> = rounds
        .iter()
        .filter(|o| o.record.side == "concave")
        .collect();
    let outside: Vec<_> = rounds
        .iter()
        .filter(|o| o.record.side == "convex")
        .collect();
    assert_eq!(inside.len(), 13);
    assert_eq!(outside.len(), 12);
    assert!(inside.iter().any(|o| o.defining.contains(&drafted)));
    let caps: Vec<_> = inside
        .iter()
        .flat_map(|o| &o.defining)
        .filter(|&&f| matches!(part.faces[f].surface, Surface::Sphere { .. }))
        .collect();
    assert_eq!(caps.len(), 8);
    // Moving the complete analytic solid must preserve the physical round/cap sets.
    let (sn, cs) = 31_f64.to_radians().sin_cos();
    let moved = part
        .transformed(&[[cs, 0., sn, 13.], [0., 1., 0., -7.], [-sn, 0., cs, 2.]])
        .unwrap();
    let signature = |p: &Part| {
        let mut result: Vec<_> = fillets::discover_verified(&Context::new(p), &Default::default())
            .unwrap()
            .into_iter()
            .map(|o| (o.defining, o.record.side, o.record.radius))
            .collect();
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    };
    assert_eq!(signature(&part), signature(&moved));
    // Inexact radius, centre or material side must remove the cap without losing its runs.
    let cap = **caps.first().unwrap();
    for fault in 0..3 {
        let mut faces = part.faces.clone();
        let Surface::Sphere {
            mut frame,
            mut radius,
        } = faces[cap].surface
        else {
            unreachable!()
        };
        match fault {
            0 => radius += 0.01,
            1 => frame.origin[0] += 0.01,
            _ => faces[cap].reversed = !faces[cap].reversed,
        }
        faces[cap].surface = Surface::Sphere { frame, radius };
        let damaged = Part::new(faces, part.edges.clone(), part.solids.clone());
        let candidates = fillets::discover(&Context::new(&damaged), &Default::default());
        assert!(candidates.iter().all(|o| !o.defining.contains(&cap)));
        assert_eq!(candidates.len(), rounds.len());
    }
    let convex = outside.iter().find(|o| o.defining == vec![38]).unwrap();
    assert_eq!(
        serde_json::to_value(&convex.record).unwrap(),
        serde_json::json!({
            "axis":"x", "radius":1., "at":[30.,-2.707,-2.707], "turned":false, "side":"convex"
        })
    );
    for o in outside {
        assert_eq!(o.defining.len(), 1);
        assert_eq!(o.record.radius, 1.);
        assert!(!o.record.turned);
    }
}

#[test]
fn ctc05_inside_rounds_and_spherical_corners_are_claimed() {
    let Some(dir) = common::corpus_dir() else {
        return;
    };
    let part = quiddity::read_step_file(&dir.join("nist/nist_ctc_05_asme1_rd.stp")).unwrap();
    let rounds = fillets::discover_verified(&Context::new(&part), &Default::default()).unwrap();
    let inside: Vec<_> = rounds
        .iter()
        // For #134, turned root rounds now also report concave. These #127
        // expectations describe the original prismatic proofs and spherical caps.
        .filter(|o| o.record.side == "concave" && !o.record.turned)
        .collect();
    assert_eq!(inside.len(), 16);
    assert!(
        rounds
            .iter()
            .any(|o| o.record.turned && o.record.side == "concave"),
        "#134: CTC-05 turned root rounds have concave material side"
    );
    assert_eq!(
        inside.iter().filter(|o| o.record.radius == 6.35).count(),
        15
    );
    assert!(!inside.iter().any(|o| o.record.radius == 19.05));
    let caps: Vec<_> = inside
        .iter()
        .flat_map(|o| &o.defining)
        .filter(|&&f| matches!(part.faces[f].surface, Surface::Sphere { .. }))
        .collect();
    assert_eq!(caps.len(), 4);
    let claimed: std::collections::BTreeSet<_> = inside
        .iter()
        .flat_map(|o| o.defining.iter().copied())
        .collect();
    for f in [11, 71, 72, 74, 75, 76, 172, 173, 175, 176, 179, 181] {
        assert!(claimed.contains(&f), "unclaimed inside treatment {f}");
    }
    // For #127: the R19.05 ends belong to existing pocket wall pairs and sweep 180°.
    // They remain full-round slot ends, beyond the unchanged 135° edge-round limit.
    for (face, walls) in [(5, [4, 6]), (184, [183, 185])] {
        let (u0, u1, _, _) = part.uv_bounds(face).unwrap();
        assert!((u1 - u0 - std::f64::consts::PI).abs() < 1e-12);
        assert!(!claimed.contains(&face));
        let pockets = quiddity::features::pockets::discover(&Context::new(&part));
        assert!(pockets.iter().any(|p| p.defining == walls));
    }
    // Exact axis/support geometry must still be proved independently of STEP's tilted
    // spring lines. A grossly displaced cylindrical wall is not a tangent support.
    // Preserve the file precision in fresh copies: Part::new would discard it and
    // could reject the already imperfect spring lines before testing the wall fault.
    let copied = part.transformed(&quiddity::kernel::step::IDENTITY).unwrap();
    let control = fillets::discover(&Context::new(&copied), &Default::default());
    for face in [11, 76] {
        assert!(control.iter().any(|o| o.defining.contains(&face)));
    }
    let mut damaged = part.transformed(&quiddity::kernel::step::IDENTITY).unwrap();
    assert_eq!(damaged.file_uncertainty(), part.file_uncertainty());
    let Surface::Cylinder { frame, radius } = damaged.faces[10].surface else {
        unreachable!()
    };
    damaged.faces[10].surface = Surface::Cylinder {
        frame,
        radius: radius + 1.,
    };
    assert!(
        fillets::discover(&Context::new(&damaged), &Default::default())
            .iter()
            .all(|o| !o.defining.contains(&11) && !o.defining.contains(&76))
    );
    // A nearby, distinct parallel support remains competing even when its offset
    // falls inside the file's larger tangency precision (#120, #127).
    let mut competing = part.transformed(&quiddity::kernel::step::IDENTITY).unwrap();
    let Surface::Cylinder { frame: run, radius } = competing.faces[11].surface else {
        unreachable!()
    };
    let at = fillets::fillet_anchor(&competing, 11).unwrap();
    assert!(fillets::curved_wall_corner(&competing, 11, at, run.z, run.origin, radius).is_some());
    let mut wall = competing.faces[13].clone();
    let Surface::Plane { mut frame } = wall.surface else {
        unreachable!()
    };
    let offset = 0.01;
    assert!(offset < part.file_uncertainty().unwrap());
    frame.origin = geom::add(frame.origin, geom::scale(frame.z, offset));
    wall.surface = Surface::Plane { frame };
    // Use the terminal neighbour's incidence; query the local support proof directly,
    // so an intentionally inconsistent solid cannot make the negative test vacuous.
    competing.faces[17].surface = wall.surface;
    competing.faces[17].reversed = wall.reversed;
    assert!(fillets::curved_wall_corner(&competing, 11, at, run.z, run.origin, radius).is_none());
    // #205 adds closed bore floor 207 to the claim: it was unexplained only in the
    // framed reading. Prove it independently; retain every round/profile assertion.
    let Surface::Plane { frame: floor } = part.faces[207].surface else {
        panic!("the added bore floor must be planar")
    };
    let Surface::Cylinder { frame: wall, .. } = part.faces[205].surface else {
        panic!("the floor must terminate the cylindrical bore")
    };
    assert!(geom::norm(geom::cross(floor.z, wall.z)) < 1e-9);
    assert!(!part.face_edges(207).is_empty());
    for edge in part.face_edges(207) {
        let neighbours = &part.edge_faces()[edge];
        assert_eq!(neighbours.len(), 2);
        assert!(neighbours.iter().all(|f| [205, 206, 207].contains(f)));
    }
    for (reading, unexplained) in [
        (quiddity::correspondence::recognise(&part), 30),
        (quiddity::correspondence::caller_space_recognise(&part), 20),
    ] {
        let d = quiddity::recognition::document(&reading, part.faces.len());
        assert!(d.records.iter().any(|r| {
            r.family == "holes" && r.faces == [205, 206, 207] && r.parameters["bottom"] == "flat"
        }));
        // For #127, a projected pocket's complete claim reserves its treatment 147.
        // The existing convex fillet 151 keeps its established recess co-ownership.
        for (face, round_family) in [(147, "blends"), (151, "fillets")] {
            assert!(
                d.records
                    .iter()
                    .any(|r| r.family == round_family && r.faces.contains(&face))
            );
            assert!(
                d.records
                    .iter()
                    .any(|r| r.family == "section_recesses" && r.faces.contains(&face))
            );
        }
        assert!(
            !d.records
                .iter()
                .any(|r| r.family == "fillets" && r.faces.contains(&147))
        );
        for face in [11, 71, 72, 74, 75, 76, 172, 173, 175, 176, 179, 181] {
            assert!(
                d.records
                    .iter()
                    .any(|r| r.family == "fillets" && r.faces.contains(&face))
            );
        }
        // For #127 after #128, five formerly unexplained convex cylinders are
        // profiles: four non-tangent R203.2 surfaces and the R127 surface 87.
        // Keep every round claim above and pin the exact five-face accounting change.
        let profile_faces: std::collections::BTreeSet<_> = d
            .records
            .iter()
            .filter(|r| r.family == "cylindrical_profiles")
            .flat_map(|r| r.faces.iter().copied())
            .collect();
        assert_eq!(profile_faces, [14, 15, 87, 168, 169].into_iter().collect());
        assert_eq!(
            d.face_accounting
                .unwrap()
                .iter()
                .filter(|f| matches!(f, quiddity::recognition::FaceAccounting::Unexplained))
                .count(),
            unexplained
        );
    }
}
