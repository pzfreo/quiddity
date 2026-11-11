//! Polygonal through-cuts with an observed intact wall core and possibly interrupted ends.
//! The published prism is a nominal cut, not an assertion that its end planes are physical
//! mouth faces. Every defining wall is contained in its side, every core side is covered by
//! the original walls, and the entire prism through the owning solid's envelope is proved air.
use super::entry_treatments::material_at_most;
use super::evidence::{self, Occurrence, common_valid_solid};
use super::graph::{is_planar, normal};
use super::passages::{SectionPassage, section_passage_record};
use super::placed::{Grid, Placed};
use super::section_passages::{SectionRingProposal, components, ordered_cycle};
use super::sections::{BodyRefIssuer, LocalFrame, PlanarSection, SectionEnds, SectionOccurrence};
use super::support_patches::{covered_patch, ruled_prism};
use super::{Context, Features};
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::{self, Curve, V3};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
const EPS: f64 = 1e-6;
const ANG: f64 = 2e-8;

/// Constant section and observed wall extent of a proved through-cut. `core_interval` is
/// proof evidence: all sides there have complete original-face support. Elsewhere intersections
/// may remove wall patches. The void proof continues through the entire owner, beyond `cut`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct PrismaticThroughCut {
    pub cut: SectionPassage,
    pub core_interval: Placed<(f64, f64)>,
    pub extent: &'static str,
}

fn world(f: &LocalFrame, t: f64, p: [f64; 2]) -> V3 {
    geom::add(
        f.origin,
        geom::add(
            geom::scale(f.run, t),
            geom::add(geom::scale(f.u, p[0]), geom::scale(f.v, p[1])),
        ),
    )
}
fn prism(f: &LocalFrame, s: &PlanarSection, lo: f64, hi: f64) -> Option<Part> {
    let a: Vec<_> = s.boundary().iter().map(|v| world(f, lo, v.point)).collect();
    let b: Vec<_> = s.boundary().iter().map(|v| world(f, hi, v.point)).collect();
    ruled_prism(&a, &b)
}
fn projection_bounds(p: &Part, faces: &[usize], run: V3) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &face in faces {
        let (a, b) = p.face_projection_bounds(face, run)?;
        lo = lo.min(a);
        hi = hi.max(b);
    }
    (lo <= hi).then_some((lo, hi))
}

/// A conservative envelope suffices for the void proof; only the published wall
/// extent needs exact directional extrema. No whole-part transforms are needed.
fn owner_envelope(p: &Part, owner: usize, run: V3) -> Option<(f64, f64)> {
    let bounds = p.solid_bounds(owner);
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for bits in 0..8 {
        let point = [0, 1, 2].map(|i| {
            if bits & (1 << i) == 0 {
                bounds.min[i]
            } else {
                bounds.max[i]
            }
        });
        let t = geom::dot(point, run);
        if !t.is_finite() {
            return None;
        }
        lo = lo.min(t);
        hi = hi.max(t);
    }
    (lo <= hi).then_some((lo, hi))
}

/// One actual collinear junction, allowing a gap made by a crossing cut. Its source segments
/// are retained to find an intact core; no face's full axial span is assumed to be a seam.
fn junction(p: &Part, a: usize, b: usize, f: &LocalFrame) -> Option<[f64; 4]> {
    let mut points = Vec::new();
    for e in p.shared_edges(a, b) {
        let edge = &p.edges[e];
        let Curve::Line { dir, .. } = edge.curve else {
            return None;
        };
        if geom::norm(geom::cross(geom::unit(dir)?, f.run)) > ANG {
            return None;
        }
        points.extend([edge.start, edge.end]);
    }
    let first = *points.first()?;
    let u = geom::dot(first, f.u);
    let v = geom::dot(first, f.v);
    if points
        .iter()
        .any(|&p| (geom::dot(p, f.u) - u).hypot(geom::dot(p, f.v) - v) > EPS)
    {
        return None;
    }
    let ts: Vec<_> = points.iter().map(|&p| geom::dot(p, f.run)).collect();
    Some([
        u,
        v,
        ts.iter().copied().fold(f64::INFINITY, f64::min),
        ts.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    ])
}

fn prove(
    ctx: &Context<'_>,
    nodes: &[usize],
    adj: &BTreeMap<usize, BTreeSet<usize>>,
    lines: &BTreeMap<(usize, usize), [f64; 4]>,
    base: &LocalFrame,
) -> Option<Occurrence<PrismaticThroughCut>> {
    let p = ctx.part;
    let owner = common_valid_solid(p, nodes)?;
    let order = ordered_cycle(nodes, adj, lines).ok()?;
    let corners: Vec<_> = (0..order.len())
        .map(|i| {
            let a = order[i];
            let b = order[(i + 1) % order.len()];
            let l = lines[&(a.min(b), a.max(b))];
            [l[0], l[1]]
        })
        .collect();
    let raw = PlanarSection::polygon(&corners).ok()?;
    let centre = raw.centroid();
    // Each original face must face into this convex void, not bound an exterior material prism.
    let centre_world = world(base, 0., centre);
    for &wall in nodes {
        let crate::kernel::geom::Surface::Plane { frame } = p.faces[wall].surface else {
            return None;
        };
        if geom::dot(normal(p, wall)?, geom::sub(centre_world, frame.origin)) <= EPS {
            return None;
        }
    }
    let frame = LocalFrame::canonical(base.run, centre_world).ok()?;
    let section = raw.translated([-centre[0], -centre[1]]).ok()?;
    let (lo, hi) = projection_bounds(p, nodes, frame.run)?;
    let (body_lo, body_hi) = owner_envelope(p, owner, frame.run)?;
    let tool = prism(&frame, &section, body_lo - 4. * EPS, body_hi + 4. * EPS)?;
    // Claim whole source faces only if the nominal side contains them, including curved trims.
    for &w in nodes {
        let supports: Vec<_> = (2..tool.faces.len()).map(|i| (&tool, i)).collect();
        if !covered_patch((p, w), &supports) {
            return None;
        }
    }
    // Try intervals cut only at observed vertex stations. Coverage proves the entire open
    // slab, so holes, gaps and intermediate intersections cannot masquerade as an intact core.
    let mut stations: Vec<f64> = nodes
        .iter()
        .flat_map(|&w| super::graph::face_vertices(p, w))
        .map(|q| geom::dot(q, frame.run))
        .collect();
    stations.sort_by(f64::total_cmp);
    stations.dedup_by(|a, b| (*a - *b).abs() <= EPS);
    let mut core = None;
    for pair in stations.windows(2) {
        let (a, b) = (pair[0] + EPS, pair[1] - EPS);
        if b - a <= 2. * EPS {
            continue;
        }
        let Some(cell) = prism(&frame, &section, a, b) else {
            continue;
        };
        let supports: Vec<_> = nodes.iter().map(|&w| (p, w)).collect();
        if (2..cell.faces.len()).all(|i| covered_patch((&cell, i), &supports))
            && core.is_none_or(|(c, d): (f64, f64)| b - a > d - c)
        {
            core = Some((a, b));
        }
    }
    let core = core?;
    if !material_at_most(ctx, owner, &tool, 1e-9) {
        return None;
    }
    let mut issuer = BodyRefIssuer::default();
    let occurrence = SectionOccurrence::new(
        issuer.issue(None).ok()?,
        frame,
        (lo, hi),
        section,
        SectionEnds::new(false, false).ok()?,
    )
    .ok()?;
    let proposal = SectionRingProposal {
        occurrence,
        nodes: nodes.to_vec(),
        solid: owner,
        constituent: BTreeSet::new(),
        low_gradient: [0.; 2],
        high_gradient: [0.; 2],
    };
    let cut = section_passage_record(&proposal).ok()?;
    Some(Occurrence {
        record: PrismaticThroughCut {
            cut,
            core_interval: Placed::new(core, Grid::CleanDecimals(6)),
            extent: "observed_walls",
        },
        defining: nodes.to_vec(),
        context: vec![],
    })
}

internal_entry! {
pub fn discover(ctx:&Context<'_>)->Vec<Occurrence<PrismaticThroughCut>> {
    discover_uncovered(ctx,&BTreeSet::new())
}
}

/// Accepted subtractive owners take precedence on any shared defining face.
/// Context/evidence faces and body/additive families are deliberately excluded.
pub(crate) const CUT_OWNERS: &[&str] = &[
    "holes",
    "countersinks",
    "double_d_bores",
    "slots",
    "oriented_slots",
    "rectangular_blind_slots",
    "round_bottom_blind_slots",
    "semicircular_blind_slots",
    "section_passages",
    "section_recesses",
    "pockets",
    "prismatic_pockets",
    "edge_open_circular_pockets",
    "edge_open_prismatic_recesses",
    "channels",
    "v_channels",
    "rectangular_through_notches",
    "through_steps",
    "local_steps",
    "angled_steps",
    "paired_ramp_steps",
    "oblique_through_steps",
    "polygonal_oblique_through_steps",
    "circular_blind_steps",
    "grooves",
    "annular_grooves",
    "internal_bevels",
    "chamfers",
    "oriented_chamfers",
    "fillets",
    "blends",
    "interior_voids",
];

struct RunGroup {
    frame: LocalFrame,
    pairs: Vec<(usize, usize)>,
}

/// Quantisation is only a lookup accelerator. Neighbouring cells, both signs,
/// and the original angular test decide equivalence, including cell boundaries.
fn run_group(
    groups: &mut Vec<RunGroup>,
    bins: &mut BTreeMap<[i64; 3], Vec<usize>>,
    frame: LocalFrame,
) -> usize {
    let key = |r: V3| r.map(|v| (v / ANG).floor() as i64);
    let mut matched = None;
    for sign in [1., -1.] {
        let k = key(geom::scale(frame.run, sign));
        for x in -1..=1 {
            for y in -1..=1 {
                for z in -1..=1 {
                    if let Some(indices) = bins.get(&[k[0] + x, k[1] + y, k[2] + z]) {
                        for &i in indices {
                            if geom::norm(geom::cross(groups[i].frame.run, frame.run)) < ANG {
                                matched = Some(matched.map_or(i, |old: usize| old.min(i)));
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(i) = matched {
        return i;
    }
    let i = groups.len();
    bins.entry(key(frame.run)).or_default().push(i);
    groups.push(RunGroup {
        frame,
        pairs: Vec::new(),
    });
    i
}

fn discover_uncovered(
    ctx: &Context<'_>,
    occupied: &BTreeSet<usize>,
) -> Vec<Occurrence<PrismaticThroughCut>> {
    let p = ctx.part;
    let normals: Vec<_> = (0..p.faces.len())
        .map(|f| if is_planar(p, f) { normal(p, f) } else { None })
        .collect();
    let mut groups: Vec<RunGroup> = Vec::new();
    let mut bins = BTreeMap::new();
    for a in 0..p.faces.len() {
        let Some(na) = normals[a] else {
            continue;
        };
        for b in p.neighbours(a) {
            if b <= a || p.arc(a, b) != Some(Arc::Concave) {
                continue;
            }
            let Some(nb) = normals[b] else {
                continue;
            };
            let Some(run) = geom::unit(geom::cross(na, nb)) else {
                continue;
            };
            let Ok(frame) = LocalFrame::canonical(run, [0.; 3]) else {
                continue;
            };
            let i = run_group(&mut groups, &mut bins, frame);
            groups[i].pairs.push((a, b));
        }
    }
    groups.sort_by(|a, b| crate::kernel::py::tuple_order(&a.frame.run, &b.frame.run));
    let mut found = Vec::new();
    let mut seen = BTreeSet::new();
    for group in groups {
        let base = group.frame;
        let mut adj: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        let mut lines = BTreeMap::new();
        for (a, b) in group.pairs {
            if normals[a].is_none_or(|n| geom::dot(n, base.run).abs() > ANG)
                || normals[b].is_none_or(|n| geom::dot(n, base.run).abs() > ANG)
            {
                continue;
            }
            let Some(line) = junction(p, a, b, &base) else {
                continue;
            };
            adj.entry(a).or_default().insert(b);
            adj.entry(b).or_default().insert(a);
            lines.insert((a, b), line);
        }
        let nodes = adj.keys().copied().collect();
        for component in components(&nodes, &adj) {
            if component.len() < 3
                || component.iter().any(|w| adj[w].len() != 2)
                || seen.contains(&component)
                || component.iter().any(|w| occupied.contains(w))
            {
                continue;
            }
            if let Some(o) = prove(ctx, &component, &adj, &lines, &base) {
                seen.insert(component);
                found.push(o);
            }
        }
    }
    found.sort_by(|a, b| a.defining.cmp(&b.defining));
    found
}

pub(crate) fn append(ctx: &Context<'_>, features: &mut Features) {
    let occupied = CUT_OWNERS
        .iter()
        .flat_map(|family| {
            features
                .defining
                .get(family)
                .into_iter()
                .flatten()
                .flatten()
                .copied()
        })
        .collect();
    let found = discover_uncovered(ctx, &occupied);
    if found.is_empty() {
        return;
    }
    features.defining.insert(
        "prismatic_through_cuts",
        found.iter().map(|o| o.defining.clone()).collect(),
    );
    features.evidence_status.insert(
        "prismatic_through_cuts",
        found
            .iter()
            .map(|o| evidence::strict_status(ctx.part, &o.defining, &o.context))
            .collect(),
    );
    features.prismatic_through_cuts = found.into_iter().map(|o| o.record).collect();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_buckets_preserve_tolerance_across_cells_and_signs() {
        let mut groups = Vec::new();
        let mut bins = BTreeMap::new();
        let frame = |x| LocalFrame::canonical([x, 0., 1.], [0.; 3]).unwrap();
        let a = run_group(&mut groups, &mut bins, frame(0.99 * ANG));
        let b = run_group(&mut groups, &mut bins, frame(1.01 * ANG));
        assert_eq!(a, b, "nearby runs on different quantisation cells");
        let mut opposite = frame(1.01 * ANG);
        opposite.run = geom::scale(opposite.run, -1.);
        opposite.v = geom::scale(opposite.v, -1.);
        assert_eq!(a, run_group(&mut groups, &mut bins, opposite));
        assert_ne!(a, run_group(&mut groups, &mut bins, frame(4. * ANG)));
        assert_eq!(groups.len(), 2);
    }
}
