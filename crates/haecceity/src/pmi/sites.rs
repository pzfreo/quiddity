//! Shared face-interior sites and the physical annotation height, including legacy datum symbols.
use crate::{
    Part,
    geom::{self, V3},
    pmi::*,
};

/// Capital height in millimetres: a small-part floor plus a fit-view proportion.
pub fn text_height(largest_mm: f64) -> f64 {
    0.5 + 0.018 * largest_mm.max(0.)
}

/// A point strictly within a trimmed face and its outward normal. Centroids may lie in
/// holes, so check the trim and fall back to deterministic interior parameter stations.
pub fn face_site(part: &Part, f: usize) -> Option<(V3, V3)> {
    let face = part.faces.get(f)?;
    let domain = part.domain(f)?;
    let (u0, u1, v0, v1) = part.uv_bounds(f)?;
    let du = (u1 - u0).abs().max(1e-6) * 1e-5;
    let dv = (v1 - v0).abs().max(1e-6) * 1e-5;
    let mut sites = Vec::new();
    if let Some(p) = part.face_centre(f)
        && let Some(uv) = face.surface.parameters(p, None)
    {
        sites.push(uv);
    }
    for a in [0.5, 0.25, 0.75, 0.125, 0.875, 0.375, 0.625] {
        for b in [0.5, 0.25, 0.75, 0.125, 0.875, 0.375, 0.625] {
            sites.push((u0 + a * (u1 - u0), v0 + b * (v1 - v0)));
        }
    }
    for (u, v) in sites {
        if interior_uv(domain, (du, dv), (u, v)) {
            let point = face.surface.value(u, v);
            let normal = face.surface.normal(u, v)?;
            if point.iter().all(|x| x.is_finite()) {
                return Some((
                    point,
                    geom::scale(normal, if face.reversed { -1. } else { 1. }),
                ));
            }
        }
    }
    None
}

fn interior_uv(domain: &crate::uv::FaceDomain, (du, dv): (f64, f64), (u, v): (f64, f64)) -> bool {
    [(u, v), (u - du, v), (u + du, v), (u, v - dv), (u, v + dv)]
        .iter()
        .all(|&(u, v)| domain.contains(u, v))
}

/// Alternative interior stations for an ISO surface symbol whose tip touches the face.
/// Prefer quarter stations so a finish need not share the datum's centroid anchor.
#[cfg(feature = "presentation")]
pub fn surface_sites(part: &Part, f: usize) -> Vec<(V3, V3)> {
    let Some(face) = part.faces.get(f) else {
        return Vec::new();
    };
    let Some(domain) = part.domain(f) else {
        return Vec::new();
    };
    let Some((u0, u1, v0, v1)) = part.uv_bounds(f) else {
        return Vec::new();
    };
    let margins = (
        (u1 - u0).abs().max(1e-6) * 1e-5,
        (v1 - v0).abs().max(1e-6) * 1e-5,
    );
    let mut sites = Vec::new();
    for a in [0.25, 0.75, 0.5, 0.125, 0.875, 0.375, 0.625] {
        for b in [0.5, 0.25, 0.75, 0.125, 0.875, 0.375, 0.625] {
            let (u, v) = (u0 + a * (u1 - u0), v0 + b * (v1 - v0));
            if !interior_uv(domain, margins, (u, v)) {
                continue;
            }
            let point = face.surface.value(u, v);
            if let Some(normal) = face.surface.normal(u, v)
                && point.iter().all(|x| x.is_finite())
            {
                sites.push((
                    point,
                    geom::scale(normal, if face.reversed { -1. } else { 1. }),
                ));
            }
        }
    }
    if let Some(site) = face_site(part, f) {
        sites.push(site);
    }
    sites
}

pub fn feature_site(part: &Part, p: &PartPmi, f: FeatureId) -> Option<(V3, V3, usize)> {
    fn faces(p: &PartPmi, f: FeatureId, depth: usize, out: &mut Vec<usize>) {
        if depth > p.features.len() {
            return;
        }
        match &p.features[f.0] {
            Feature::Items(items) => out.extend(items.iter().filter_map(|a| {
                if let Anchor::Face(f) = a {
                    Some(f.0)
                } else {
                    None
                }
            })),
            Feature::Group { members, .. } | Feature::Derived { from: members, .. } => {
                for f in members {
                    faces(p, *f, depth + 1, out);
                }
            }
        }
    }
    let mut fs = Vec::new();
    faces(p, f, 0, &mut fs);
    fs.sort_unstable();
    fs.dedup();
    fs.sort_by(|&a, &b| {
        part.face_bounds(b)
            .diagonal()
            .total_cmp(&part.face_bounds(a).diagonal())
            .then(a.cmp(&b))
    });
    fs.into_iter()
        .find_map(|f| face_site(part, f).map(|(p, n)| (p, n, f)))
}
