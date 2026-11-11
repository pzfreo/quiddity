//! Edge-open blind slots with a semicylindrical floor and two tangent straight sides.
//! Complete native boundaries prove the section and the unique run/cap roles. Treated or
//! interrupted boundaries and competing source claims are refused.
use super::Context;
use super::evidence::{self, EvidenceStatus, Occurrence};
use super::graph::normal;
use super::regions::{edge_length, length_tolerance};
use super::v_channels::{exterior, wall};
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::{self, Curve, Surface, V3};
use serde::Serialize;
use std::collections::BTreeSet;
use std::f64::consts::PI;

/// A blind round-bottom slot, in millimetres. `axis` points from the open run end towards
/// the cap; `opening` points out of the lateral mouth. `at` is the section box's centre
/// halfway along the run. Section width is 2r, depth is r + straight_height.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct SemicircularBlindSlot {
    pub axis: V3,
    pub opening: V3,
    pub at: V3,
    pub length: f64,
    pub radius: f64,
    pub straight_height: f64,
}

fn other(part: &Part, edge: usize, face: usize) -> Option<usize> {
    let [a, b] = part.edge_faces()[edge][..] else {
        return None;
    };
    if a == face {
        Some(b)
    } else if b == face {
        Some(a)
    } else {
        None
    }
}

fn from_floor(part: &Part, floor: usize) -> Option<Occurrence<SemicircularBlindSlot>> {
    let f = &part.faces[floor];
    let Surface::Cylinder { frame, radius } = f.surface else {
        return None;
    };
    if !f.reversed || radius <= 0. || f.loops.len() != 1 || f.loops[0].edges.len() != 4 {
        return None;
    }
    let edges = part.face_edges(floor);
    if edges.len() != 4 {
        return None;
    }
    let mut rims = Vec::new();
    let mut rails = Vec::new();
    for edge in edges {
        let e = &part.edges[edge];
        match e.curve {
            Curve::Circle {
                frame: cf,
                radius: r,
            } if !e.is_closed()
                && geom::dot(cf.z, frame.z).abs() > 1. - 1e-9
                && (r - radius).abs() <= length_tolerance(&[radius])
                && (edge_length(part, edge) - PI * radius).abs()
                    <= length_tolerance(&[PI * radius])
                && (geom::dist(e.start, e.end) - 2. * radius).abs()
                    <= length_tolerance(&[radius]) =>
            {
                rims.push((edge, cf.origin));
            }
            Curve::Line { dir, .. } if geom::dot(geom::unit(dir)?, frame.z).abs() > 1. - 1e-9 => {
                rails.push(edge)
            }
            _ => return None,
        }
    }
    if rims.len() != 2 || rails.len() != 2 {
        return None;
    }
    let cap_candidates: Vec<_> = rims
        .iter()
        .filter_map(|&(e, center)| {
            let cap = other(part, e, floor)?;
            (matches!(part.faces[cap].surface, Surface::Plane { .. })
                && part.arc(floor, cap) == Some(Arc::Concave))
            .then_some((e, center, cap))
        })
        .collect();
    let [(cap_rim, cap_center, cap)] = cap_candidates[..] else {
        return None;
    };
    let &(mouth_rim, mouth_center) = rims.iter().find(|(e, _)| *e != cap_rim)?;
    let delta = geom::sub(cap_center, mouth_center);
    let length = geom::norm(delta);
    let axis = geom::unit(delta)?;
    let tolerance = length_tolerance(&[radius, length]);
    if length <= tolerance
        || geom::dot(axis, frame.z).abs() < 1. - 1e-9
        || geom::dot(normal(part, cap)?, axis) > -1. + 1e-9
    {
        return None;
    }
    let mouth = exterior(part, floor, mouth_rim, geom::scale(axis, -1.))?;
    let sides: [usize; 2] = [other(part, rails[0], floor)?, other(part, rails[1], floor)?];
    if sides[0] == sides[1]
        || sides
            .iter()
            .any(|&side| part.arc(floor, side) != Some(Arc::Smooth))
    {
        return None;
    }
    let walls = [
        wall(part, sides[0], rails[0], axis, tolerance)?,
        wall(part, sides[1], rails[1], axis, tolerance)?,
    ];
    if geom::dist(walls[0].offset, walls[1].offset) > tolerance {
        return None;
    }
    let straight_height = geom::norm(walls[0].offset);
    let opening = geom::unit(walls[0].offset)?;
    if straight_height <= tolerance || geom::dot(opening, axis).abs() > 1e-9 {
        return None;
    }
    for &(rim, center) in &rims {
        if geom::dist(
            geom::sub(center, part.edges[rim].midpoint()),
            geom::scale(opening, radius),
        ) > tolerance
        {
            return None;
        }
    }
    let mut context = vec![mouth];
    let mut cap_sides = Vec::new();
    for (side, wall) in sides.into_iter().zip(&walls) {
        context.push(exterior(part, side, wall.lip, opening)?);
        for &edge in &wall.ends {
            let e = &part.edges[edge];
            let station = geom::dot(geom::sub(e.midpoint(), mouth_center), axis);
            if station.abs() <= tolerance {
                context.push(exterior(part, side, edge, geom::scale(axis, -1.))?);
            } else if (station - length).abs() <= tolerance
                && other(part, edge, side) == Some(cap)
                && part.arc(side, cap) == Some(Arc::Concave)
            {
                cap_sides.push(edge);
            } else {
                return None;
            }
        }
    }
    let cap_edges = part.face_edges(cap);
    if part.faces[cap].loops.len() != 1 || cap_edges.len() != 4 || cap_sides.len() != 2 {
        return None;
    }
    let top: Vec<_> = cap_edges
        .into_iter()
        .filter(|e| *e != cap_rim && !cap_sides.contains(e))
        .collect();
    let [top] = top[..] else { return None };
    if !matches!(part.edges[top].curve, Curve::Line { .. })
        || (edge_length(part, top) - 2. * radius).abs() > tolerance
    {
        return None;
    }
    context.push(exterior(part, cap, top, opening)?);
    context.sort_unstable();
    context.dedup();
    let defining: Vec<_> = BTreeSet::from([floor, sides[0], sides[1], cap])
        .into_iter()
        .collect();
    if defining.len() != 4
        || context.iter().any(|f| defining.contains(f))
        || evidence::strict_status(part, &defining, &context) != EvidenceStatus::Verified
    {
        return None;
    }
    Some(Occurrence {
        record: SemicircularBlindSlot {
            axis,
            opening,
            at: geom::add(
                geom::scale(geom::add(cap_center, mouth_center), 0.5),
                geom::scale(opening, (straight_height - radius) / 2.),
            ),
            length,
            radius,
            straight_height,
        },
        defining,
        context,
    })
}

internal_entry! {
/// Complete semicircular blind slots on strictly valid solids, without overlapping claims.
pub fn discover(ctx:&Context<'_>)->Vec<Occurrence<SemicircularBlindSlot>> {
    let found:Vec<_>=(0..ctx.part.faces.len()).filter_map(|f|from_floor(ctx.part,f)).collect();
    found.iter().filter(|o|!found.iter().any(|p|!std::ptr::eq(*o,p)
        && p.defining.iter().any(|f|o.defining.contains(f)))).cloned().collect()
}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::brep::{Edge, Face, Loop, Solid};
    use crate::kernel::geom::Frame;
    use crate::kernel::sampling::sample_edge;
    use crate::kernel::step::IDENTITY;
    use std::collections::BTreeMap;

    fn stock() -> Part {
        let points = [
            [0., 0., 0.],
            [30., 0., 0.],
            [30., 20., 0.],
            [0., 20., 0.],
            [0., 0., 20.],
            [30., 0., 20.],
            [30., 20., 20.],
            [0., 20., 20.],
            [10., 20., 0.],
            [10., 15., 0.],
            [20., 15., 0.],
            [20., 20., 0.],
            [10., 20., 12.],
            [10., 15., 12.],
            [20., 15., 12.],
            [20., 20., 12.],
        ];
        let mut edges = Vec::new();
        let mut ids = BTreeMap::new();
        let mut faces = Vec::new();
        let circle_frame = |z| Frame {
            origin: [15., 15., z],
            x: [1., 0., 0.],
            y: [0., 1., 0.],
            z: [0., 0., 1.],
        };
        let mut add = |vertices: &[usize], n: V3, cylinder: bool| {
            let mut boundary = Vec::new();
            for i in 0..vertices.len() {
                let (a, b) = (vertices[i], vertices[(i + 1) % vertices.len()]);
                let key = (a.min(b), a.max(b));
                let (edge, forward) = if let Some(&(e, first)) = ids.get(&key) {
                    (e, a == first)
                } else {
                    let e = edges.len();
                    let (start, end) = (points[a], points[b]);
                    let (curve, same_sense) = if key == (9, 10) || key == (13, 14) {
                        (
                            Curve::Circle {
                                frame: circle_frame(start[2]),
                                radius: 5.,
                            },
                            a == key.0,
                        )
                    } else {
                        (
                            Curve::Line {
                                origin: start,
                                dir: geom::sub(end, start),
                            },
                            true,
                        )
                    };
                    edges.push(Edge {
                        samples: sample_edge(&curve, start, end, same_sense, false),
                        curve,
                        start,
                        end,
                        vertices: (a, b),
                        same_sense,
                    });
                    ids.insert(key, (e, a));
                    (e, true)
                };
                boundary.push((edge, forward));
            }
            let surface = if cylinder {
                Surface::Cylinder {
                    frame: circle_frame(0.),
                    radius: 5.,
                }
            } else {
                let x = geom::default_x_axis(n);
                Surface::Plane {
                    frame: Frame {
                        origin: points[vertices[0]],
                        x,
                        y: geom::cross(n, x),
                        z: n,
                    },
                }
            };
            faces.push(Face {
                surface,
                reversed: cylinder,
                loops: vec![Loop {
                    edges: boundary,
                    vertex: None,
                }],
                solid: Some(0),
                pcurves: Vec::new(),
            });
        };
        add(&[0, 3, 8, 9, 10, 11, 2, 1], [0., 0., -1.], false);
        add(&[4, 5, 6, 7], [0., 0., 1.], false);
        add(&[0, 4, 7, 3], [-1., 0., 0.], false);
        add(&[1, 2, 6, 5], [1., 0., 0.], false);
        add(&[0, 1, 5, 4], [0., -1., 0.], false);
        add(&[3, 7, 6, 2, 11, 15, 12, 8], [0., 1., 0.], false);
        add(&[12, 15, 14, 13], [0., 0., -1.], false);
        add(&[10, 9, 13, 14], [0.; 3], true);
        add(&[8, 12, 13, 9], [1., 0., 0.], false);
        add(&[11, 10, 14, 15], [-1., 0., 0.], false);
        Part::new(
            faces,
            edges,
            vec![Solid {
                faces: (0..10).collect(),
            }],
        )
    }

    #[test]
    fn semicircular_blind_slot_has_unique_run_and_measured_profile_after_motion() {
        let base = stock();
        assert!(base.solid_is_geometrically_valid(0));
        let (s, c) = 0.37_f64.sin_cos();
        for motion in [
            IDENTITY,
            [[c, 0., s, 123.456], [0., 1., 0., -78.9], [-s, 0., c, 41.3]],
        ] {
            let part = base.transformed(&motion).unwrap();
            let found = discover(&Context::new(&part));
            assert_eq!(found.len(), 1);
            let o = &found[0];
            let r = &o.record;
            assert_eq!(o.defining, [6, 7, 8, 9]);
            assert_eq!(r.radius, 5.);
            assert!((r.straight_height - 5.).abs() < 1e-9);
            assert!((r.length - 12.).abs() < 1e-9);
            let recognition = crate::correspondence::recognise(&part);
            let doc = crate::recognition::document(&recognition, part.faces.len());
            let records: Vec<_> = doc
                .records
                .iter()
                .filter(|r| r.family == "semicircular_blind_slots")
                .collect();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].evidence_status, EvidenceStatus::Verified);
            let at: V3 = serde_json::from_value(records[0].parameters["at"].clone()).unwrap();
            assert!(geom::dist(at, r.at) < 1e-9);
        }
    }

    #[test]
    fn interrupted_profile_wrong_material_and_missing_cap_refuse() {
        for fault in 0..3 {
            let mut part = stock();
            match fault {
                0 => {
                    part.faces[8].loops[0].edges.pop();
                }
                1 => part.faces[7].reversed = false,
                _ => part.faces[6].solid = None,
            }
            assert!(discover(&Context::new(&part)).is_empty(), "fault {fault}");
        }
    }
}
