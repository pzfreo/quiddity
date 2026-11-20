//! Conservative body evidence, evaluated only in the inferred part frame.
//! The complete rule and abstentions are specified in docs/face-accounting.md.
use std::collections::{BTreeMap, BTreeSet};

use crate::features::Features;
use crate::kernel::brep::Arc;

use crate::Part;
use crate::kernel::geom::{Bounds, Curve, Surface, V3};
use crate::recognition::BodyReason;

pub(crate) fn evidence(part: &Part, features: &Features) -> BTreeMap<usize, BodyReason> {
    let mut out = BTreeMap::new();
    let blends: BTreeSet<_> = ["fillets", "chamfers", "oriented_chamfers", "blends"]
        .iter()
        .filter_map(|family| features.defining.get(family))
        .flatten()
        .flatten()
        .copied()
        .collect();
    for (solid, shell) in part.solids.iter().enumerate() {
        let bounds = part.solid_bounds(solid);
        let tol = 1e-7 * bounds.max_extent().max(1.0);
        // Sampled freeform surfaces cannot establish a supporting plane. A native
        // surface's spline trim is safe only when its complete positive-weight
        // control hull is already enclosed by the analytic envelope.
        if shell.faces.iter().any(|&f| {
            !matches!(
                part.faces[f].surface,
                Surface::Plane { .. }
                    | Surface::Cylinder { .. }
                    | Surface::Cone { .. }
                    | Surface::Sphere { .. }
                    | Surface::Torus { .. }
            ) || part
                .face_edges(f)
                .iter()
                .any(|&e| !certified_curve(&part.edges[e].curve, bounds, tol))
        }) || !part.solid_is_geometrically_valid(solid)
        {
            continue;
        }
        if (0..3).any(|k| {
            !bounds.min[k].is_finite()
                || !bounds.max[k].is_finite()
                || bounds.max[k] - bounds.min[k] <= 100.0 * tol
        }) {
            continue;
        }
        for &f in &shell.faces {
            let face = &part.faces[f];
            if face.solid != Some(solid) || !matches!(face.surface, Surface::Plane { .. }) {
                continue;
            }
            let Some(normal) = part.face_normal(f, 0.0, 0.0) else {
                continue;
            };
            let Some(axis) = (0..3).find(|&k| {
                normal[k].abs() >= 1.0 - 1e-12
                    && (0..3).filter(|&j| j != k).all(|j| normal[j].abs() <= 1e-9)
            }) else {
                continue;
            };
            let at = if normal[axis] > 0.0 {
                bounds.max[axis]
            } else {
                bounds.min[axis]
            };
            let transverse: Vec<_> = (0..3).filter(|&j| j != axis).collect();
            // An entire rectangular box side is stronger evidence than merely reaching
            // the extrema: a notched side, boss cap or a chamfer cannot pass this test.
            let rectangle = face.loops.iter().any(|lp| {
                if lp.vertex.is_some() || lp.edges.len() != 4 {
                    return false;
                }
                let mut corners = Vec::new();
                for &(e, forward) in &lp.edges {
                    let edge = &part.edges[e];
                    if !matches!(edge.curve, Curve::Line { .. }) {
                        return false;
                    }
                    let corner = |p: [f64; 3]| -> Option<usize> {
                        if (p[axis] - at).abs() > tol {
                            return None;
                        }
                        let mut code = 0;
                        for (bit, &j) in transverse.iter().enumerate() {
                            if (p[j] - bounds.min[j]).abs() <= tol {
                            } else if (p[j] - bounds.max[j]).abs() <= tol {
                                code |= 1 << bit;
                            } else {
                                return None;
                            }
                        }
                        Some(code)
                    };
                    let (Some(start), Some(end)) = (corner(edge.start), corner(edge.end)) else {
                        return false;
                    };
                    if (start ^ end).count_ones() != 1 {
                        return false;
                    }
                    corners.push(if forward { (start, end) } else { (end, start) });
                }
                (0..4).all(|i| corners[i].1 == corners[(i + 1) % 4].0)
                    && (0..4).all(|code| corners.iter().filter(|c| c.0 == code).count() == 1)
            });
            if rectangle {
                out.insert(f, BodyReason::StockBoxSide);
            } else if let Some(reason) =
                broken_side(part, solid, f, normal, axis, bounds, tol, &blends)
            {
                out.insert(f, reason);
            }
        }
        let stock = out.clone();
        let plates = plate_perimeters(part, features, solid, tol);
        for &f in &shell.faces {
            if out.contains_key(&f) {
                continue;
            }
            if plate_edge(part, f, &plates, tol) {
                out.insert(f, BodyReason::PlateEdge);
            } else if stock_wall(part, f, &stock, &plates, bounds, tol) {
                out.insert(f, BodyReason::StockWall);
            }
        }
    }
    out
}

fn certified_curve(curve: &Curve, bounds: Bounds, tol: f64) -> bool {
    match curve {
        Curve::Line { .. } | Curve::Circle { .. } | Curve::Ellipse { .. } => true,
        Curve::Nurbs(n) => {
            n.weights().iter().all(|w| w.is_finite() && *w > 0.)
                && n.control_points().iter().all(|p| {
                    (0..3).all(|k| {
                        p[k].is_finite()
                            && p[k] >= bounds.min[k] - tol
                            && p[k] <= bounds.max[k] + tol
                    })
                })
        }
    }
}

/// A surviving stock corner distinguishes a cut box outline from an isolated cap.
/// A lower, outward platform at the stock perimeter is evidence of a raised
/// feature; veto even when the platform is split into several faces by a rib.
#[allow(clippy::too_many_arguments)]
fn broken_side(
    part: &Part,
    solid: usize,
    face: usize,
    normal: V3,
    axis: usize,
    bounds: Bounds,
    tol: f64,
    blends: &BTreeSet<usize>,
) -> Option<BodyReason> {
    let at = if normal[axis] > 0.0 {
        bounds.max[axis]
    } else {
        bounds.min[axis]
    };
    let fb = part.face_bounds(face);
    if (fb.min[axis] - at).abs() > tol || (fb.max[axis] - at).abs() > tol {
        return None;
    }
    let transverse: Vec<_> = (0..3).filter(|&j| j != axis).collect();
    let stock_corner = part.face_edges(face).iter().any(|&e| {
        [part.edges[e].start, part.edges[e].end].iter().any(|p| {
            (p[axis] - at).abs() <= tol
                && transverse.iter().all(|&j| {
                    (p[j] - bounds.min[j]).abs() <= tol || (p[j] - bounds.max[j]).abs() <= tol
                })
        })
    });
    let blended = blended_corner(part, face, bounds, tol, blends);
    let rimmed = !stock_corner && !blended && enveloping_rim(part, face, axis, bounds, tol, blends);
    if !stock_corner && !blended && !rimmed {
        return None;
    }
    // A rim-only face has no surviving or replaced box corner; apply the broader veto.
    let stock_corner = stock_corner || rimmed;
    let mut platforms = Vec::new();
    let mut coplanar = 0;
    for &g in &part.solids[solid].faces {
        if !matches!(part.faces[g].surface, Surface::Plane { .. }) {
            continue;
        }
        let Some(n) = part.face_normal(g, 0.0, 0.0) else {
            continue;
        };
        if (0..3).any(|j| (n[j] - normal[j]).abs() > 1e-9) {
            continue;
        }
        let gb = part.face_bounds(g);
        if (gb.min[axis] - gb.max[axis]).abs() > tol {
            continue;
        }
        let level = 0.5 * (gb.min[axis] + gb.max[axis]);
        // A rimmed platform stops a blend short of the extremes: its own non-concave
        // blends count, so rounding the edges cannot hide it from the veto.
        let mut gb = gb;
        if rimmed {
            for h in part.neighbours(g) {
                if blends.contains(&h) && part.arc(g, h) != Some(Arc::Concave) {
                    gb.merge(&part.face_bounds(h));
                }
            }
        }
        // With a certified replaced envelope corner, an opposite block's inward
        // wall is not an underlying platform. Require an actual bridging face
        // incident to both planes. Keep the older surviving-corner veto intact.
        let bridges = stock_corner
            || part.neighbours(face).iter().any(|&h| {
                part.neighbours(g).contains(&h) && {
                    let hb = part.face_bounds(h);
                    hb.min[axis] <= level.min(at) + tol && hb.max[axis] >= level.max(at) - tol
                }
            });
        if (level - at).abs() <= tol {
            coplanar += 1;
        } else if normal[axis] * (level - at) < -tol && bridges {
            platforms.push((level, gb));
        }
    }
    platforms.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut i = 0;
    while i < platforms.len() {
        let mut b = platforms[i].1;
        let mut j = i + 1;
        while j < platforms.len() && platforms[j].0 - platforms[j - 1].0 <= tol {
            b.merge(&platforms[j].1);
            j += 1;
        }
        if transverse.iter().all(|&k| {
            (b.min[k] - bounds.min[k]).abs() <= tol || (b.max[k] - bounds.max[k]).abs() <= tol
        }) {
            return None;
        }
        i = j;
    }
    Some(if rimmed {
        BodyReason::StockEnvelope
    } else if coplanar > 1 {
        BodyReason::StockBoxSplit
    } else {
        BodyReason::StockBoxNotched
    })
}

/// A face on an outward envelope plane whose rim reaches an extreme of the solid in each
/// transverse axis, through an outward principal plane at that extreme or a recognised
/// non-concave fillet/chamfer/blend reaching it. Rounded outlines and edge rounds replace
/// every box corner with several faces, so none survives for the corner rules, yet the face
/// still bounds the stock like a notched side. At least one recognised blend must meet the
/// face: an unrecognised chamfer stays unproved, as for the corner rules. A boss or turned cap has no such planes or
/// blends at the envelope around it, so it abstains.
fn enveloping_rim(
    part: &Part,
    face: usize,
    axis: usize,
    bounds: Bounds,
    tol: f64,
    blends: &BTreeSet<usize>,
) -> bool {
    let transverse: Vec<_> = (0..3).filter(|&j| j != axis).collect();
    let mut reached = [[false; 2]; 3];
    let mut rounded = false;
    for g in part.neighbours(face) {
        if part.faces[g].solid != part.faces[face].solid {
            continue;
        }
        let gb = part.face_bounds(g);
        if let Some((k, n)) = principal_plane(part, g) {
            let side = usize::from(n[k] > 0.0);
            let at = if side == 1 {
                bounds.max[k]
            } else {
                bounds.min[k]
            };
            if k != axis
                && (gb.min[k] - at).abs() <= tol
                && (gb.max[k] - at).abs() <= tol
                && part.arc(face, g) == Some(Arc::Convex)
            {
                reached[k][side] = true;
            }
        } else if blends.contains(&g) && part.arc(face, g) != Some(Arc::Concave) {
            rounded = true;
            for &k in &transverse {
                reached[k][0] |= (gb.min[k] - bounds.min[k]).abs() <= tol;
                reached[k][1] |= (gb.max[k] - bounds.max[k]).abs() <= tol;
            }
        }
    }
    rounded && transverse.iter().all(|&k| reached[k][0] || reached[k][1])
}

/// A recognised edge treatment must meet this face and replace an actual box
/// corner: two of its planar supports are outward box planes, and its run reaches
/// the third extreme. Mere proximity to a blend is insufficient.
fn blended_corner(
    part: &Part,
    face: usize,
    bounds: Bounds,
    tol: f64,
    blends: &BTreeSet<usize>,
) -> bool {
    part.neighbours(face).iter().any(|g| {
        if !blends.contains(g) || part.faces[*g].solid != part.faces[face].solid {
            return false;
        }
        let run = match part.faces[*g].surface {
            Surface::Cylinder { frame, .. } if part.frame_points_outward(*g) == Some(true) => {
                (0..3).find(|&k| frame.z[k].abs() >= 1. - 1e-12)
            }
            Surface::Plane { .. } => part.face_normal(*g, 0., 0.).and_then(|n| {
                let zeros: Vec<_> = (0..3).filter(|&k| n[k].abs() <= 1e-9).collect();
                (zeros.len() == 1).then(|| zeros[0])
            }),
            _ => None,
        };
        let Some(run) = run else {
            return false;
        };
        let mut axes = BTreeSet::new();
        for h in part.neighbours(*g) {
            let Some((axis, normal)) = principal_plane(part, h) else {
                continue;
            };
            if axis == run {
                continue;
            }
            if matches!(part.faces[*g].surface, Surface::Plane { .. })
                && part.arc(*g, h) != Some(Arc::Convex)
            {
                continue;
            }
            let hb = part.face_bounds(h);
            let at = if normal[axis] > 0. {
                bounds.max[axis]
            } else {
                bounds.min[axis]
            };
            if (hb.min[axis] - at).abs() <= tol && (hb.max[axis] - at).abs() <= tol {
                axes.insert(axis);
            }
        }
        if axes.len() < 2 {
            return false;
        }
        let gb = part.face_bounds(*g);
        (0..3).any(|k| {
            !axes.contains(&k)
                && ((gb.min[k] - bounds.min[k]).abs() <= tol
                    || (gb.max[k] - bounds.max[k]).abs() <= tol)
        })
    })
}

fn principal_plane(part: &Part, face: usize) -> Option<(usize, V3)> {
    if !matches!(part.faces[face].surface, Surface::Plane { .. }) {
        return None;
    }
    let normal = part.face_normal(face, 0., 0.)?;
    let axis = (0..3).find(|&k| {
        normal[k].abs() >= 1. - 1e-12 && (0..3).filter(|&j| j != k).all(|j| normal[j].abs() <= 1e-9)
    })?;
    Some((axis, normal))
}

struct PlatePerimeter {
    axis: usize,
    lo: f64,
    hi: f64,
    faces: Vec<usize>,
    walls: Vec<usize>,
}

fn plate_perimeters(
    part: &Part,
    features: &Features,
    solid: usize,
    tol: f64,
) -> Vec<PlatePerimeter> {
    features
        .plates
        .iter()
        .enumerate()
        .filter_map(|(i, plate)| {
            let faces = features.defining.get("plates")?.get(i)?;
            if faces.is_empty() || faces.iter().any(|&f| part.faces[f].solid != Some(solid)) {
                return None;
            }
            let axis = ['x', 'y', 'z'].iter().position(|&c| c == plate.axis)?;
            let lo = *plate.lo.raw();
            let hi = *plate.hi.raw();
            let walls = part.solids[solid]
                .faces
                .iter()
                .copied()
                .filter(|&f| {
                    let Some((k, _)) = principal_plane(part, f) else {
                        return false;
                    };
                    if k == axis {
                        return false;
                    }
                    // Only outer-loop adjacency at BOTH slab ends. Inner slot/pocket loops
                    // and walls ending in the slab cannot establish its perimeter.
                    [lo, hi].iter().all(|&level| {
                        faces.iter().any(|&g| {
                            let gb = part.face_bounds(g);
                            (gb.min[axis] - level).abs() <= tol
                                && (gb.max[axis] - level).abs() <= tol
                                && part
                                    .outer_edges(g)
                                    .iter()
                                    .any(|e| part.face_edges(f).contains(e))
                                && part.arc(f, g) == Some(Arc::Convex)
                        })
                    })
                })
                .collect();
            Some(PlatePerimeter {
                axis,
                lo,
                hi,
                faces: faces.clone(),
                walls,
            })
        })
        .collect()
}

fn plate_edge(part: &Part, face: usize, plates: &[PlatePerimeter], tol: f64) -> bool {
    let Some((axis, normal)) = principal_plane(part, face) else {
        return false;
    };
    if part
        .neighbours(face)
        .iter()
        .any(|&g| part.arc(face, g) == Some(Arc::Concave))
    {
        return false;
    }
    let b = part.face_bounds(face);
    plates.iter().any(|p| {
        let run = (0..3).find(|&k| k != axis && k != p.axis).unwrap();
        p.walls.contains(&face) && (b.min[p.axis] - p.lo).abs() <= tol && (b.max[p.axis] - p.hi).abs() <= tol &&
            // A supporting perimeter wall, never a return into an edge notch.
            // Include taller walls meeting both ends when establishing support.
            p.walls.iter().all(|&g| {
                if principal_plane(part, g).is_none_or(|(k, _)| k != axis) { return true; }
                let gb = part.face_bounds(g);
                let at = if normal[axis] > 0. { gb.max[axis] } else { gb.min[axis] };
                normal[axis] * (at - b.min[axis]) <= tol
            }) &&
            // A plate recogniser can find a thin strip beside a step. Its own
            // perimeter alone is then insufficient: a parallel wall elsewhere
            // on the same run exposes the strip's edge as a recessed shoulder.
            part.solids[part.faces[face].solid.unwrap()].faces.iter().all(|&g| {
                if principal_plane(part, g).is_none_or(|(k, _)| k != axis) { return true; }
                let gb = part.face_bounds(g);
                gb.max[run].min(b.max[run]) - gb.min[run].max(b.min[run]) <= tol ||
                    normal[axis] * (gb.min[axis] - b.min[axis]) <= tol
            })
    })
}

fn stock_wall(
    part: &Part,
    face: usize,
    stock: &BTreeMap<usize, BodyReason>,
    plates: &[PlatePerimeter],
    envelope: Bounds,
    tol: f64,
) -> bool {
    if plates.is_empty() {
        return false;
    }
    let Some((axis, normal)) = principal_plane(part, face) else {
        return false;
    };
    if part
        .neighbours(face)
        .iter()
        .any(|&g| part.arc(face, g) == Some(Arc::Concave))
    {
        return false;
    }
    let b = part.face_bounds(face);
    let plate_faces: BTreeSet<_> = plates.iter().flat_map(|p| &p.faces).copied().collect();
    let neighbours = part.neighbours(face);
    let at = if normal[axis] > 0. {
        envelope.max[axis]
    } else {
        envelope.min[axis]
    };
    let on_envelope = (b.min[axis] - at).abs() <= tol && (b.max[axis] - at).abs() <= tol;
    plates.iter().any(|p| {
        if !(b.min[p.axis] < p.lo - tol || b.max[p.axis] > p.hi + tol) {
            return false;
        }
        // The entire native face on an outward envelope plane supplies an
        // independent supporting-plane proof. Interior feature walls have no
        // such proof, regardless of how many convex stock contacts they have.
        let contact = |g| {
            part.arc(face, g) == Some(Arc::Convex)
                && part
                    .shared_edges(face, g)
                    .iter()
                    .any(|&e| matches!(part.edges[e].curve, Curve::Line { .. }))
        };
        if on_envelope
            && neighbours.iter().any(|&g| {
                stock.contains_key(&g)
                    && !plate_faces.contains(&g)
                    && contact(g)
                    && neighbours.iter().any(|&h| {
                        h != g && (stock.contains_key(&h) || plate_faces.contains(&h)) && contact(h)
                    })
            })
        {
            return true;
        }
        // An interior wall needs stronger evidence than convex stock contacts.
        // Parallel cut ends cannot qualify, even beside a recognised plate.
        if axis == p.axis {
            return false;
        }
        let run = (0..3).find(|&k| k != axis && k != p.axis).unwrap();
        // Require two perpendicular, complete stock boundary edges: one along
        // the wall's run, the other across its height. Two opposite stock end
        // caps can bound an arbitrary through-cut wall and prove nothing here.
        [p.axis, run].iter().all(|&support_axis| {
            let span = if support_axis == p.axis { run } else { p.axis };
            neighbours.iter().any(|&g| {
                stock.contains_key(&g)
                    && !plate_faces.contains(&g)
                    && principal_plane(part, g).is_some_and(|(k, _)| k == support_axis)
                    && part.arc(face, g) == Some(Arc::Convex)
                    && part.shared_edges(face, g).iter().any(|&e| {
                        let edge = &part.edges[e];
                        matches!(edge.curve, Curve::Line { .. })
                            && (edge.start[span].min(edge.end[span]) - b.min[span]).abs() <= tol
                            && (edge.start[span].max(edge.end[span]) - b.max[span]).abs() <= tol
                    })
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(part: &Part) -> BTreeMap<usize, BodyReason> {
        super::evidence(part, &Features::default())
    }
    fn stock_wall(
        part: &Part,
        face: usize,
        stock: &BTreeMap<usize, BodyReason>,
        plates: &[PlatePerimeter],
        tol: f64,
    ) -> bool {
        super::stock_wall(part, face, stock, plates, part.solid_bounds(0), tol)
    }
    use crate::frames;
    use crate::kernel::step::Placement;
    use std::path::{Path, PathBuf};

    fn profile(points: &[[f64; 2]]) -> Part {
        profile_with_arcs(points, &[])
    }

    fn profile_with_arcs(points: &[[f64; 2]], arcs: &[(usize, [f64; 2])]) -> Part {
        use crate::kernel::brep::{Edge, Face, Loop};
        use crate::kernel::geom::{Frame, sub};
        let vertices: Vec<V3> = points.iter().map(|p| [p[0], 0.0, p[1]]).collect();
        let mut edges: Vec<Edge> = (0..vertices.len())
            .map(|i| {
                let j = (i + 1) % vertices.len();
                Edge {
                    curve: Curve::Line {
                        origin: vertices[i],
                        dir: sub(vertices[j], vertices[i]),
                    },
                    start: vertices[i],
                    end: vertices[j],
                    vertices: (i, j),
                    same_sense: true,
                    samples: vec![vertices[i], vertices[j]],
                }
            })
            .collect();
        for &(e, centre) in arcs {
            let origin = [centre[0], 0., centre[1]];
            edges[e].curve = Curve::Circle {
                frame: Frame {
                    origin,
                    x: [1., 0., 0.],
                    y: [0., 0., 1.],
                    z: [0., -1., 0.],
                },
                radius: crate::kernel::geom::dist(origin, edges[e].start),
            };
            edges[e].same_sense = false;
        }
        let source = Part::new(
            vec![Face {
                surface: Surface::Plane {
                    frame: Frame {
                        origin: [0.0; 3],
                        x: [1.0, 0.0, 0.0],
                        y: [0.0, 0.0, 1.0],
                        z: [0.0, -1.0, 0.0],
                    },
                },
                reversed: false,
                loops: vec![Loop {
                    edges: (0..vertices.len()).map(|e| (e, true)).collect(),
                    vertex: None,
                }],
                solid: None,
                pcurves: vec![],
            }],
            edges,
            vec![],
        );
        crate::kernel::sweep::extrude_face(&source, 0, [0.0; 3], [0.0, -10.0, 0.0]).unwrap()
    }

    #[test]
    fn unrecognised_edge_pad_and_rib_caps_abstain_but_cut_stock_sides_qualify() {
        // Direct geometry evidence: no recognition records exist to mask a mistake.
        for points in [
            vec![[0., 0.], [10., 0.], [10., 2.], [3., 2.], [3., 5.], [0., 5.]],
            vec![
                [0., 0.],
                [10., 0.],
                [10., 2.],
                [6., 2.],
                [6., 5.],
                [4., 5.],
                [4., 2.],
                [0., 2.],
            ],
        ] {
            let part = profile(&points);
            assert!(part.solid_is_geometrically_valid(0));
            let body = evidence(&part);
            let caps: Vec<_> = (0..part.faces.len())
                .filter(|&f| {
                    let b = part.face_bounds(f);
                    (b.min[2] - 5.0).abs() < 1e-9 && (b.max[2] - 5.0).abs() < 1e-9
                })
                .collect();
            assert!(!caps.is_empty());
            for f in caps {
                assert!(!body.contains_key(&f), "raised cap {f}");
            }
            assert!(body.values().any(|&r| r == BodyReason::StockBoxNotched));
        }
        let slot = profile(&[
            [0., 0.],
            [10., 0.],
            [10., 5.],
            [7., 5.],
            [7., 2.],
            [3., 2.],
            [3., 5.],
            [0., 5.],
        ]);
        assert!(slot.solid_is_geometrically_valid(0));
        assert_eq!(
            evidence(&slot)
                .values()
                .filter(|&&r| r == BodyReason::StockBoxSplit)
                .count(),
            2
        );
    }

    fn slab(part: &Part, axis: usize, lo: f64, hi: f64) -> Features {
        use crate::features::placed::{Grid, Placed};
        let faces: Vec<_> = (0..part.faces.len())
            .filter(|&f| {
                let b = part.face_bounds(f);
                principal_plane(part, f).is_some_and(|(k, _)| k == axis)
                    && (b.max[axis] - b.min[axis]).abs() < 1e-9
                    && ((b.min[axis] - lo).abs() < 1e-9 || (b.min[axis] - hi).abs() < 1e-9)
            })
            .collect();
        assert!(faces.len() >= 2);
        let mut features = Features::default();
        features.plates.push(crate::features::plates::Plate {
            axis: ['x', 'y', 'z'][axis],
            lo: Placed::new(lo, Grid::Decimals(6)),
            hi: Placed::new(hi, Grid::Decimals(6)),
            u: 0.,
            v: 0.,
            body_key: None,
        });
        features.defining.insert("plates", vec![faces]);
        features
    }

    #[test]
    fn plate_edges_reject_notch_returns_and_inward_walls() {
        let part = profile(&[
            [0., 0.],
            [20., 0.],
            [20., 6.],
            [15., 6.],
            [15., 3.],
            [5., 3.],
            [5., 6.],
            [0., 6.],
        ]);
        let features = slab(&part, 1, -10., 0.);
        let plates = plate_perimeters(&part, &features, 0, 1e-6);
        let mut positive = 0;
        let mut negative = 0;
        for f in 0..part.faces.len() {
            let Some((k, _)) = principal_plane(&part, f) else {
                continue;
            };
            if k == 1 {
                continue;
            }
            let b = part.face_bounds(f);
            let notch = (k == 2 && (b.min[2] - 3.).abs() < 1e-9)
                || (k == 0 && ((b.min[0] - 5.).abs() < 1e-9 || (b.min[0] - 15.).abs() < 1e-9));
            assert_eq!(plate_edge(&part, f, &plates, 1e-6), !notch, "face {f}");
            if notch {
                negative += 1;
            } else {
                positive += 1;
            }
        }
        assert_eq!((positive, negative), (5, 3));
        // Inward orientation cannot become plate skin even on the same support plane.
        let f = plates[0].walls[0];
        let mut inward = Part::new(part.faces.clone(), part.edges.clone(), part.solids.clone());
        inward.faces[f].reversed = !inward.faces[f].reversed;
        let inward_plates = plate_perimeters(&inward, &features, 0, 1e-6);
        assert!(!plate_edge(&inward, f, &inward_plates, 1e-6));
    }

    #[test]
    fn plate_edge_rejects_a_step_shoulder_beside_a_thin_strip() {
        let part = profile(&[
            [0., 0.],
            [10., 0.],
            [10., 2.],
            [2., 2.],
            [2., 6.],
            [20., 6.],
            [20., 10.],
            [0., 10.],
        ]);
        let features = slab(&part, 2, 0., 2.);
        let plates = plate_perimeters(&part, &features, 0, 1e-6);
        let shoulder = (0..part.faces.len())
            .find(|&f| {
                let b = part.face_bounds(f);
                principal_plane(&part, f).is_some_and(|(k, _)| k == 0)
                    && (b.min[0] - 10.).abs() < 1e-9
            })
            .unwrap();
        assert!(plates[0].walls.contains(&shoulder));
        assert!(
            part.neighbours(shoulder)
                .iter()
                .all(|&g| part.arc(shoulder, g) == Some(Arc::Convex))
        );
        assert!(!plate_edge(&part, shoulder, &plates, 1e-6));
    }

    #[test]
    fn stock_wall_envelope_skin_can_use_two_stock_end_caps() {
        let part = profile(&[
            [0., 0.],
            [20., 0.],
            [20., 6.],
            [15., 6.],
            [15., 2.],
            [5., 2.],
            [5., 6.],
            [0., 6.],
        ]);
        let features = slab(&part, 2, 0., 2.);
        let plates = plate_perimeters(&part, &features, 0, 1e-6);
        let stock: BTreeMap<_, _> = evidence(&part)
            .into_iter()
            .filter(|&(f, _)| principal_plane(&part, f).is_some_and(|(k, _)| k == 1))
            .collect();
        let outside = (0..part.faces.len())
            .find(|&f| {
                principal_plane(&part, f).is_some_and(|(k, n)| k == 0 && n[0] < 0.)
                    && part.face_bounds(f).min[0].abs() < 1e-9
            })
            .unwrap();
        assert_eq!(stock.len(), 2);
        assert!(stock_wall(&part, outside, &stock, &plates, 1e-6));
        assert!(!stock_wall(&part, outside, &BTreeMap::new(), &plates, 1e-6));
    }

    #[test]
    fn stock_wall_requires_thick_block_anchors_not_a_cut_through_plate() {
        let part = profile_with_arcs(
            &[
                [0., 0.],
                [5., 0.],
                [5., 3.],
                [6., 4.],
                [14., 4.],
                [15., 3.],
                [15., 0.],
                [20., 0.],
                [20., 6.],
                [0., 6.],
            ],
            &[(2, [6., 3.]), (4, [14., 3.])],
        );
        assert!(part.solid_is_geometrically_valid(0));
        let features = slab(&part, 2, 4., 6.);
        let plates = plate_perimeters(&part, &features, 0, 1e-6);
        let stock = evidence(&part);
        let walls: Vec<_> = (0..part.faces.len())
            .filter(|&f| {
                let b = part.face_bounds(f);
                principal_plane(&part, f).is_some_and(|(k, _)| k == 0)
                    && ((b.min[0] - 5.).abs() < 1e-9 || (b.min[0] - 15.).abs() < 1e-9)
            })
            .collect();
        assert_eq!(walls.len(), 2);
        for &f in &walls {
            assert!(stock_wall(&part, f, &stock, &plates, 1e-6));
            assert!(!stock_wall(&part, f, &BTreeMap::new(), &plates, 1e-6));
        }
        // A second, perpendicular slab does not turn a parallel cut end face
        // into a wall. It can have convex stock contacts on three sides.
        let unrelated = slab(&part, 0, 0., 5.);
        let unrelated_plates = plate_perimeters(&part, &unrelated, 0, 1e-6);
        for &f in &walls {
            assert!(!stock_wall(&part, f, &stock, &unrelated_plates, 1e-6));
        }

        // A slanted through-step's principal wall can borrow the two stock end
        // caps. Neither cap establishes a stock edge along the wall's run.
        let step = profile_with_arcs(
            &[
                [0., 0.],
                [5., 1.],
                [5., 3.],
                [6., 4.],
                [14., 4.],
                [15., 3.],
                [15., 0.],
                [20., 0.],
                [20., 6.],
                [0., 6.],
            ],
            &[(2, [6., 3.]), (4, [14., 3.])],
        );
        let features = slab(&step, 2, 4., 6.);
        let plates = plate_perimeters(&step, &features, 0, 1e-6);
        let wall = (0..step.faces.len())
            .find(|&f| {
                let b = step.face_bounds(f);
                principal_plane(&step, f).is_some_and(|(k, _)| k == 0)
                    && (b.min[0] - 5.).abs() < 1e-9
            })
            .unwrap();
        assert!(
            step.neighbours(wall)
                .iter()
                .all(|&g| step.arc(wall, g) != Some(Arc::Concave))
        );
        let step_stock = evidence(&step);
        let stock_caps: Vec<_> = step
            .neighbours(wall)
            .into_iter()
            .filter(|g| step_stock.contains_key(g) && step.arc(wall, *g) == Some(Arc::Convex))
            .collect();
        assert_eq!(stock_caps.len(), 2);
        assert!(
            stock_caps
                .iter()
                .all(|&g| principal_plane(&step, g).unwrap().0 == 1)
        );
        assert!(!stock_wall(&step, wall, &step_stock, &plates, 1e-6));
        let sharp = profile(&[
            [0., 0.],
            [5., 0.],
            [5., 4.],
            [15., 4.],
            [15., 0.],
            [20., 0.],
            [20., 6.],
            [0., 6.],
        ]);
        let sharp_features = slab(&sharp, 2, 4., 6.);
        let sharp_plates = plate_perimeters(&sharp, &sharp_features, 0, 1e-6);
        for f in 0..sharp.faces.len() {
            if sharp
                .neighbours(f)
                .iter()
                .any(|&g| sharp.arc(f, g) == Some(Arc::Concave))
            {
                assert!(!stock_wall(
                    &sharp,
                    f,
                    &evidence(&sharp),
                    &sharp_plates,
                    1e-6
                ));
            }
        }
        // The same U outline is a waist cut through a plate when the plate axis
        // is the extrusion direction. No recursive/plate-only stock anchors.
        let thin = slab(&part, 1, -10., 0.);
        let thin_plates = plate_perimeters(&part, &thin, 0, 1e-6);
        for f in 0..part.faces.len() {
            assert!(!stock_wall(&part, f, &stock, &thin_plates, 1e-6));
        }
        let notch = profile(&[
            [0., 0.],
            [20., 0.],
            [20., 6.],
            [15., 6.],
            [15., 3.],
            [5., 3.],
            [5., 6.],
            [0., 6.],
        ]);
        let features = slab(&notch, 1, -10., 0.);
        let plates = plate_perimeters(&notch, &features, 0, 1e-6);
        let stock = evidence(&notch);
        for f in 0..notch.faces.len() {
            assert!(!stock_wall(&notch, f, &stock, &plates, 1e-6));
        }
    }

    #[test]
    fn chamfered_profile_supplies_missing_box_corners_only_when_recognised() {
        let part = profile(&[
            [0., 1.],
            [1., 0.],
            [19., 0.],
            [20., 1.],
            [20., 5.],
            [19., 6.],
            [1., 6.],
            [0., 5.],
        ]);
        assert!(evidence(&part).is_empty());
        let features = crate::features::recognise(&part);
        assert_eq!(features.chamfers.len(), 4);
        let body = super::evidence(&part, &features);
        assert_eq!(body.len(), 6);
        assert!(body.values().all(|&r| r == BodyReason::StockBoxNotched));
        for &f in body.keys() {
            assert!(principal_plane(&part, f).is_some());
        }
    }

    #[test]
    fn spline_trims_require_an_enclosed_control_hull() {
        use crate::kernel::nurbs::NurbsCurve;
        let bounds = Bounds {
            min: [0.; 3],
            max: [1.; 3],
        };
        let curve = |middle| {
            Curve::Nurbs(
                NurbsCurve::new(
                    2,
                    vec![[0., 0., 0.], middle, [1., 1., 1.]],
                    vec![1.; 3],
                    vec![0., 0., 0., 1., 1., 1.],
                )
                .unwrap(),
            )
        };
        assert!(certified_curve(&curve([0.5; 3]), bounds, 1e-9));
        assert!(!certified_curve(&curve([0.5, 1.5, 0.5]), bounds, 1e-9));
        assert!(!certified_curve(&curve([f64::NAN, 0.5, 0.5]), bounds, 1e-9));
    }

    #[test]
    fn replaced_corner_requires_recognised_meeting_edge_treatment() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/filleted_box_one_edge.step");
        let part = crate::read_step_file(&path).unwrap();
        let features = crate::features::recognise(&part);
        let blends: BTreeSet<_> = features.defining["fillets"]
            .iter()
            .flatten()
            .copied()
            .collect();
        assert!(!blends.is_empty());
        let bounds = part.solid_bounds(0);
        let mut met = 0;
        let mut remote = 0;
        for f in 0..part.faces.len() {
            if !matches!(part.faces[f].surface, Surface::Plane { .. }) {
                continue;
            }
            assert!(!blended_corner(&part, f, bounds, 1e-6, &BTreeSet::new()));
            let touches = part.neighbours(f).iter().any(|g| blends.contains(g));
            assert_eq!(blended_corner(&part, f, bounds, 1e-6, &blends), touches);
            if touches {
                met += 1;
            } else {
                remote += 1;
            }
        }
        assert!(met > 0 && remote > 0);
    }

    fn normalized_evidence(part: &Part) -> BTreeMap<usize, BodyReason> {
        frames::infer_part_frame(part)
            .ok()
            .and_then(|f| part.transformed(&f.normalisation()).ok())
            .map_or_else(BTreeMap::new, |p| evidence(&p))
    }

    fn rotation(axis: usize, degrees: f64, translation: [f64; 3]) -> Placement {
        let (s, c) = degrees.to_radians().sin_cos();
        let (i, j) = ((axis + 1) % 3, (axis + 2) % 3);
        let mut p = crate::kernel::step::IDENTITY;
        p[i][i] = c;
        p[j][j] = c;
        p[i][j] = -s;
        p[j][i] = s;
        for k in 0..3 {
            p[k][3] = translation[k];
        }
        p
    }

    #[test]
    fn body_evidence_survives_z30_x30_and_z17_x23_on_goldens_and_corpus() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let mut paths: Vec<_> = std::fs::read_dir(&fixtures)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("golden_")
                    && p.extension().is_some_and(|e| e == "step")
            })
            .collect();
        if let Some(root) = std::env::var_os("QUIDDITY_CORPUS").filter(|r| r != "off") {
            let corpus: serde_json::Value =
                serde_json::from_str(include_str!("../tests/fixtures/corpus.json")).unwrap();
            paths.extend(
                corpus["files"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| PathBuf::from(&root).join(e["file"].as_str().unwrap())),
            );
        } else {
            assert!(
                std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
                "QUIDDITY_CORPUS required"
            );
        }
        let t = [137.0, -83.0, 49.0];
        let z17 = rotation(2, 17.0, [0.0; 3]);
        let x23 = rotation(0, 23.0, t);
        let composed = std::array::from_fn(|i| {
            std::array::from_fn(|j| {
                (0..3).map(|k| x23[i][k] * z17[k][j]).sum::<f64>()
                    + if j == 3 { x23[i][3] } else { 0.0 }
            })
        });
        let motions = [
            ("Z30", rotation(2, 30.0, t)),
            ("X30", rotation(0, 30.0, t)),
            ("Z17+X23", composed),
        ];
        let mut body_count = 0;
        for path in paths {
            let part = crate::read_step_file(&path).unwrap();
            let expected = normalized_evidence(&part);
            body_count += expected.len();
            for (name, motion) in &motions {
                let moved = part.transformed(motion).unwrap();
                assert_eq!(
                    normalized_evidence(&moved),
                    expected,
                    "{} {name}",
                    path.display()
                );
            }
        }
        assert!(body_count > 0, "invariance must exercise positive evidence");
    }

    #[test]
    fn hole_skin_is_body_but_pocket_floors_and_partial_boss_caps_are_not() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let hole =
            crate::read_step_file(&fixtures.join("golden_simple_through_hole.step")).unwrap();
        let body = normalized_evidence(&hole);
        // This fixture also has a boss rising above the block. Its cap must
        // abstain independently of whether the boss recognizer publishes a record.
        assert_eq!(
            body.keys().copied().collect::<Vec<_>>(),
            vec![0, 1, 3, 4, 5]
        );
        assert!(!body.contains_key(&8));
        assert!(
            body.keys()
                .all(|&f| matches!(hole.faces[f].surface, Surface::Plane { .. }))
        );
        for name in [
            "golden_blind_pockets_and_pocket_patterns.step",
            "golden_plates_pads_levels_and_slanted_steps.step",
        ] {
            let part = crate::read_step_file(&fixtures.join(name)).unwrap();
            let frame = frames::infer_part_frame(&part).unwrap();
            let part = part.transformed(&frame.normalisation()).unwrap();
            let body = evidence(&part);
            let mut excluded = 0;
            for (f, face) in part.faces.iter().enumerate() {
                if !matches!(face.surface, Surface::Plane { .. }) {
                    continue;
                }
                let solid = face.solid.unwrap();
                let b = part.solid_bounds(solid);
                let fb = part.face_bounds(f);
                // Interior planes remain unexplained without feature claims.
                let on_extreme = (0..3).any(|k| {
                    (fb.max[k] - fb.min[k]).abs() < 1e-6
                        && ((fb.min[k] - b.min[k]).abs() < 1e-6
                            || (fb.min[k] - b.max[k]).abs() < 1e-6)
                });
                if !on_extreme {
                    assert!(!body.contains_key(&f), "{name} face {f}");
                    excluded += 1;
                }
            }
            assert!(excluded > 0, "{name} must exercise negative evidence");
        }
        let shell = Part::new(hole.faces.clone(), hole.edges.clone(), vec![]);
        assert!(evidence(&shell).is_empty());
    }
}
