//! Which neighbours meet a face along one of its wires (`quiddity._wire_seed`): traversal and
//! acceptance stay with the callers.

use std::collections::{BTreeMap, BTreeSet};

use super::graph::uniquely_paired_uses;
use crate::kernel::brep::Part;

internal_entry! {
/// The neighbours of *opening* that share an edge of *wire* with it through an exactly paired
/// occurrence (`wire_seed` over `FaceGraph.neighbours_by_occurrence_edge`).
///
/// Start from the queried edges, rather than comparing the whole opening with every neighbour
/// for each wire. Read all uses on the opening, including uses in other loops: seams and repeated
/// occurrences must satisfy the same unique opposite-use pairing as `shared_occurrences`.
/// Every qualifying neighbour owns an edge in both the opening and the wire, so its existing
/// edge-owner entry is sufficient to find it. The face set still ignores edge/use multiplicity.
pub fn wire_seed(part: &Part, opening: usize, wire: &[usize]) -> BTreeSet<usize> {
    let selected: BTreeSet<_> = wire.iter().copied().collect();
    let mut opening_uses: BTreeMap<usize, Vec<bool>> = BTreeMap::new();
    for lp in &part.faces[opening].loops {
        for &(edge, forward) in &lp.edges {
            if selected.contains(&edge) {
                opening_uses.entry(edge).or_default().push(forward);
            }
        }
    }
    let owners = part.edge_faces();
    let mut out = BTreeSet::new();
    for (edge, left) in opening_uses {
        for &other in &owners[edge] {
            if other == opening {
                continue;
            }
            let right: Vec<_> = part.faces[other]
                .loops
                .iter()
                .flat_map(|lp| &lp.edges)
                .filter(|(e, _)| *e == edge)
                .map(|(_, forward)| *forward)
                .collect();
            if uniquely_paired_uses(&left, &right) {
                out.insert(other);
            }
        }
    }
    out
}
}

internal_entry! {
/// The edges of each of the face's inner loops (`face.inner_wires()`): every loop but the outer
/// one. A face whose outer loop cannot be told has none read, so nothing is seeded from it.
pub fn inner_wires(part: &Part, face: usize) -> Vec<Vec<usize>> {
    let loops = &part.faces[face].loops;
    if loops.len() < 2 {
        return Vec::new();
    }
    let Some(outer) = part.outer_loop(face) else {
        return Vec::new();
    };
    loops
        .iter()
        .enumerate()
        .filter(|&(l, lp)| l != outer && !lp.edges.is_empty())
        .map(|(_, lp)| lp.edges.iter().map(|&(e, _)| e).collect())
        .collect()
}
}
