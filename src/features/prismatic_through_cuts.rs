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
fn projection_bounds(p: &Part, faces: &[usize], run: V3) -> (f64, f64) {
    // Measure curved trim extrema in a run-aligned frame. Projecting a world AABB
    // would make the published extent depend on the part's rigid placement.
    let axis = (0..3).find(|&i| run[i].abs() == 1. && (0..3).all(|j| i == j || run[j] == 0.));
    let rotated;
    let (p, run) = if axis.is_none() {
        let f = LocalFrame::canonical(run, [0.; 3]).expect("unit run");
        let matrix = [
            [f.u[0], f.u[1], f.u[2], 0.],
            [f.v[0], f.v[1], f.v[2], 0.],
            [run[0], run[1], run[2], 0.],
        ];
        rotated = p.transformed(&matrix).expect("rigid frame");
        (&rotated, [0., 0., 1.])
    } else {
        (p, run)
    };
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &face in faces {
        let b = p.face_bounds(face);
        for bits in 0..8 {
            let q = [0, 1, 2].map(|i| {
                if bits & (1 << i) == 0 {
                    b.min[i]
                } else {
                    b.max[i]
                }
            });
            let t = geom::dot(q, run);
            lo = lo.min(t);
            hi = hi.max(t);
        }
    }
    (lo, hi)
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
    if nodes.iter().any(|&w| {
        let crate::kernel::geom::Surface::Plane { frame } = p.faces[w].surface else {
            return true;
        };
        geom::dot(normal(p, w).unwrap(), geom::sub(centre_world, frame.origin)) <= EPS
    }) {
        return None;
    }
    let frame = LocalFrame::canonical(base.run, centre_world).ok()?;
    let section = raw.translated([-centre[0], -centre[1]]).ok()?;
    let (lo, hi) = projection_bounds(p, nodes, frame.run);
    let body_faces = &p.solids[owner].faces;
    let (body_lo, body_hi) = projection_bounds(p, body_faces, frame.run);
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
    discover_uncovered(ctx,&[])
}
}

fn discover_uncovered(
    ctx: &Context<'_>,
    covered: &[Vec<usize>],
) -> Vec<Occurrence<PrismaticThroughCut>> {
    let p = ctx.part;
    let mut runs: Vec<V3> = Vec::new();
    let mut pairs = Vec::new();
    for a in 0..p.faces.len() {
        if !is_planar(p, a) {
            continue;
        }
        for b in p.neighbours(a) {
            if b <= a || !is_planar(p, b) || p.arc(a, b) != Some(Arc::Concave) {
                continue;
            }
            let Some(run) = geom::unit(geom::cross(normal(p, a).unwrap(), normal(p, b).unwrap()))
            else {
                continue;
            };
            let Ok(f) = LocalFrame::canonical(run, [0.; 3]) else {
                continue;
            };
            if !runs
                .iter()
                .any(|&r| geom::norm(geom::cross(r, f.run)) < ANG)
            {
                runs.push(f.run);
            }
            pairs.push((a, b));
        }
    }
    runs.sort_by(|a, b| crate::kernel::py::tuple_order(a, b));
    let mut found = Vec::new();
    let mut seen = BTreeSet::new();
    for run in runs {
        let Ok(base) = LocalFrame::canonical(run, [0.; 3]) else {
            continue;
        };
        let mut adj: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        let mut lines = BTreeMap::new();
        for &(a, b) in &pairs {
            if normal(p, a).is_none_or(|n| geom::dot(n, run).abs() > ANG)
                || normal(p, b).is_none_or(|n| geom::dot(n, run).abs() > ANG)
            {
                continue;
            }
            let Some(l) = junction(p, a, b, &base) else {
                continue;
            };
            adj.entry(a).or_default().insert(b);
            adj.entry(b).or_default().insert(a);
            lines.insert((a, b), l);
        }
        let nodes = adj.keys().copied().collect();
        for component in components(&nodes, &adj) {
            if component.len() < 3
                || component.iter().any(|w| adj[w].len() != 2)
                || seen.contains(&component)
            {
                continue;
            }
            if covered
                .iter()
                .any(|old| component.iter().all(|w| old.contains(w)))
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
    let covered: Vec<_> = ["section_passages", "section_recesses", "oriented_slots"]
        .into_iter()
        .flat_map(|family| features.defining.get(family).into_iter().flatten().cloned())
        .collect();
    let found = discover_uncovered(ctx, &covered);
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
