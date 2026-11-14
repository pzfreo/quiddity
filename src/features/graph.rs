//! Face-graph readings the recognisers share (`quiddity._adjacency.FaceGraph`, and the build123d
//! face queries the families make directly): a face's planarity, normal and box span, its
//! vertices, the neighbours a set of faces has in common, the shared-edge occurrences between
//! two faces, and a set of faces walked as one open chain.

use super::planes::{normalized, unit_plane_normal};
use crate::kernel::brep::Part;
use crate::kernel::geom::{Surface, V3};

internal_entry! {
/// Whether the face is a native plane (`FaceGraph.is_planar`).
pub fn is_planar(part: &Part, face: usize) -> bool {
    matches!(part.faces[face].surface, Surface::Plane { .. })
}
}

internal_entry! {
/// The face's unit normal as `FaceGraph.normal` reads it (`face.normal_at()`): at the centre of
/// its parameter range, flipped on a reversed face. `None` for a face without one there.
pub fn normal(part: &Part, face: usize) -> Option<V3> {
    let n = if is_planar(part, face) {
        unit_plane_normal(part, face)?
    } else {
        let (u0, u1, v0, v1) = part.uv_bounds(face)?;
        let (u, v) = (u0 + 0.5 * (u1 - u0), v0 + 0.5 * (v1 - v0));
        normalized(part.face_normal(face, u, v)?)?
    };
    n.iter().all(|c| c.is_finite()).then_some(n)
}
}

internal_entry! {
/// The face's box as `(low, high)` along *axis* (`FaceGraph.bounds(node)[axis]`).
pub fn span(part: &Part, face: usize, axis: usize) -> (f64, f64) {
    let b = part.face_bounds(face);
    (b.min[axis], b.max[axis])
}
}

internal_entry! {
/// The face's distinct vertex points (`face.vertices()`), each edge read in its stored
/// direction, edges in [`Part::face_edges`] order. The oblique through-step reading: it uses
/// the points as a set (their depths, each point's offset).
pub fn face_vertices(part: &Part, face: usize) -> Vec<V3> {
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for e in part.face_edges(face) {
        let edge = &part.edges[e];
        for (v, p) in [(edge.vertices.0, edge.start), (edge.vertices.1, edge.end)] {
            if !seen.contains(&v) {
                seen.push(v);
                out.push(p);
            }
        }
    }
    out
}
}

internal_entry! {
/// The face's distinct vertex points (`face.vertices()`) in boundary order: every loop walked
/// in its own direction, each edge's ends as the walk meets them. The gusset reading: its
/// order reaches the record (the first square corner found, the centroid's sum). It names the
/// same points as [`face_vertices`], possibly in another order; the two are kept apart so
/// neither family's output moves.
pub fn face_vertices_along_loops(part: &Part, face: usize) -> Vec<V3> {
    let mut ids = Vec::new();
    let mut points = Vec::new();
    for lp in &part.faces[face].loops {
        for &(e, forward) in &lp.edges {
            let edge = &part.edges[e];
            let ends = [(edge.vertices.0, edge.start), (edge.vertices.1, edge.end)];
            let ends = if forward { ends } else { [ends[1], ends[0]] };
            for (id, p) in ends {
                if !ids.contains(&id) {
                    ids.push(id);
                    points.push(p);
                }
            }
        }
    }
    points
}
}

internal_entry! {
/// The neighbours every one of *faces* shares, in the first face's neighbour order.
pub fn common_neighbours(part: &Part, faces: &[usize]) -> Vec<usize> {
    let Some((&first, rest)) = faces.split_first() else {
        return Vec::new();
    };
    let others: Vec<Vec<usize>> = rest.iter().map(|&f| part.neighbours(f)).collect();
    part.neighbours(first)
        .into_iter()
        .filter(|n| others.iter().all(|o| o.contains(n)))
        .collect()
}
}

internal_entry! {
/// The edges two faces share whose uses pair up uniquely in opposite directions
/// (`FaceGraph.shared_occurrences`): an edge read a different number of times by the two
/// faces, or without one opposite partner per use, has no traversal-independent pairing and is
/// left out.
pub fn shared_occurrences(part: &Part, a: usize, b: usize) -> Vec<usize> {
    if a == b {
        return Vec::new();
    }
    let uses = |face: usize, edge: usize| -> Vec<bool> {
        part.faces[face]
            .loops
            .iter()
            .flat_map(|l| &l.edges)
            .filter(|(e, _)| *e == edge)
            .map(|(_, forward)| *forward)
            .collect()
    };
    let mut out = Vec::new();
    for edge in part.shared_edges(a, b) {
        let (left, right) = (uses(a, edge), uses(b, edge));
        if uniquely_paired_uses(&left, &right) {
            // One occurrence per pair: as many as the edge has uses on either side.
            out.extend(std::iter::repeat_n(edge, left.len()));
        }
    }
    out
}
}

/// Whether every use has exactly one opposite partner on the other face, in both directions.
pub(super) fn uniquely_paired_uses(left: &[bool], right: &[bool]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .all(|l| right.iter().filter(|r| *r != l).count() == 1)
        && right
            .iter()
            .all(|r| left.iter().filter(|l| *l != r).count() == 1)
}

internal_entry! {
/// *nodes* as one open chain from its lower-numbered end, two nodes linked where they are
/// neighbours and *joined* holds (`edge_open_prismatic_recesses._ordered_open_chain`, every
/// neighbour; `edge_open_circular_recesses._ordered_chain`, smooth arcs only). `None` unless
/// exactly two nodes have one link, none has more than two, and the walk from the lower end
/// reaches every node. *nodes* may come in any order.
pub fn ordered_chain(
    part: &Part,
    nodes: &[usize],
    joined: impl Fn(usize, usize) -> bool,
) -> Option<Vec<usize>> {
    let adjacent: Vec<Vec<usize>> = nodes
        .iter()
        .map(|&node| {
            part.neighbours(node)
                .into_iter()
                .filter(|&o| nodes.contains(&o) && joined(node, o))
                .collect()
        })
        .collect();
    let of = |node: usize| &adjacent[nodes.iter().position(|&n| n == node).expect("member")];
    let ends: Vec<usize> = nodes
        .iter()
        .copied()
        .filter(|&n| of(n).len() == 1)
        .collect();
    if ends.len() != 2 || nodes.iter().any(|&n| !matches!(of(n).len(), 1 | 2)) {
        return None;
    }
    let mut ordered = vec![*ends.iter().min().expect("two ends")];
    while ordered.len() < nodes.len() {
        let choices: Vec<usize> = of(*ordered.last().expect("started"))
            .iter()
            .copied()
            .filter(|n| !ordered.contains(n))
            .collect();
        let &[next] = choices.as_slice() else {
            return None;
        };
        ordered.push(next);
    }
    let mut a = ordered.clone();
    let mut b = nodes.to_vec();
    a.sort_unstable();
    b.sort_unstable();
    (a == b).then_some(ordered)
}
}
