//! Step 3 of `correspond`: matching faces (`docs/correspondence.md`, "Match the faces"). Seeds
//! come from faces that coincide after alignment (one surface, overlapping) and from the faces of
//! matched features; matches then propagate across the attributed adjacency graph, a face
//! matching a neighbour of a matched face's partner across an edge of the same kind. What no
//! seed reaches is assigned last, family by family of surface type, under the same margin rule
//! as the features.

// Matrix code reads clearest indexed.
#![allow(clippy::needless_range_loop)]

use std::collections::{BTreeMap, HashMap};

use serde_json::json;

use super::{Change, Class, Ctx, Entry, Outcome, assign, multiset_changes, multiset_distance};
use super::{same_value, sizes_distance};
use crate::kernel::geom;

/// The surfaces the kernel integrates exactly; the rest are freeform.
const ANALYTIC: [&str; 5] = ["plane", "cylinder", "cone", "sphere", "torus"];

/// Remaining faces of one surface type beyond which the last, global assignment is not tried
/// (its cost grows with the cube): they are left unmatched.
const GLOBAL_LIMIT: usize = 400;

impl Ctx<'_> {
    /// The cost of old face *a* being new face *b*; `None` if their surface types differ.
    fn face_cost(&self, a: usize, b: usize) -> Option<f64> {
        let (fa, fb) = (&self.old.faces[a], &self.new.faces[b]);
        if fa.surface != fb.surface {
            return None;
        }
        let (sum, n) = sizes_distance(&fa.parameters, &fb.parameters);
        let parameters = if n == 0 { 0.0 } else { sum / n as f64 };
        let area = match (fa.area, fb.area) {
            (Some(x), Some(y)) => ((x - y).abs() / x.max(y).max(1e-12)).min(1.0),
            _ => 0.5,
        };
        let adjacency = multiset_distance(&fa.adjacency, &fb.adjacency);
        let direction = match ((fa.normal, fb.normal), (fa.axis, fb.axis)) {
            ((Some(x), Some(y)), _) => 0.5 * (1.0 - self.agreement(x, y, false)),
            (_, (Some(x), Some(y))) => 1.0 - self.agreement(x, y, true),
            _ => 0.0,
        };
        let position = self.displacement(self.gap(fa.centroid, fb.centroid));
        let placement = 0.5 * direction + 0.5 * position;
        Some(
            self.th.semantics_weight * (parameters + area)
                + self.th.neighbourhood_weight * adjacency
                + self.placement_weight() * placement,
        )
    }

    /// Whether old face *a* and new face *b* lie on one surface after alignment and overlap
    /// (estimated: areas within the overlap ratio, centroids within the rest of it of the smaller
    /// face's size).
    fn coincide(&self, a: usize, b: usize) -> bool {
        let (fa, fb) = (&self.old.faces[a], &self.new.faces[b]);
        // About an axisymmetric alignment's line nothing off it can be placed exactly.
        if self.axis.is_some() {
            return false;
        }
        let (tol, cos) = (self.tol(), self.th.angle_tol.cos());
        if fa.surface != fb.surface
            || fa.parameters.len() != fb.parameters.len()
            || !fa.parameters.iter().all(|(k, x)| {
                fb.parameters
                    .get(k)
                    .is_some_and(|y| same_value(*x, *y, self.th.same_rel))
            })
        {
            return false;
        }
        let (Some(xa), Some(xb)) = (fa.area, fb.area) else {
            return false;
        };
        if xa.min(xb) < self.th.overlap * xa.max(xb) {
            return false;
        }
        let ca = self.motion.point(fa.centroid);
        if geom::dist(ca, fb.centroid) > (1.0 - self.th.overlap) * xa.min(xb).sqrt() + tol {
            return false;
        }
        let parallel = |line: bool| match (fa.axis, fb.axis) {
            (Some(x), Some(y)) => {
                let c = geom::dot(self.motion.dir(x), y);
                (if line { c.abs() } else { c }) >= cos
            }
            _ => false,
        };
        let support = || match (fa.support, fb.support) {
            (Some(p), Some(q)) => Some((self.motion.point(p), q)),
            _ => None,
        };
        match fa.surface.as_str() {
            "plane" => {
                parallel(false)
                    && fb
                        .axis
                        .is_some_and(|n| geom::dot(geom::sub(ca, fb.centroid), n).abs() <= tol)
            }
            "cylinder" => {
                parallel(true)
                    && support().is_some_and(|(p, q)| {
                        let axis = fb.axis.unwrap_or([0.0; 3]);
                        let d = geom::sub(p, q);
                        geom::norm(geom::sub(d, geom::scale(axis, geom::dot(d, axis)))) <= tol
                    })
            }
            "cone" | "torus" => {
                parallel(true) && support().is_some_and(|(p, q)| geom::dist(p, q) <= tol)
            }
            "sphere" => support().is_some_and(|(p, q)| geom::dist(p, q) <= tol),
            _ => false,
        }
    }

    fn face_changes(&self, a: usize, b: usize) -> BTreeMap<String, Change> {
        let (fa, fb) = (&self.old.faces[a], &self.new.faces[b]);
        let mut out = BTreeMap::new();
        for (k, x) in &fa.parameters {
            let y = fb.parameters.get(k);
            if !y.is_some_and(|y| same_value(*x, *y, self.th.same_rel)) {
                out.insert(
                    format!("parameters.{k}"),
                    Change {
                        old: json!(x),
                        new: json!(y),
                    },
                );
            }
        }
        let freeform = !ANALYTIC.contains(&fa.surface.as_str());
        let rel = if freeform {
            self.th.freeform_rel
        } else {
            self.th.same_rel
        };
        let same_area = match (fa.area, fb.area) {
            (Some(x), Some(y)) => same_value(x, y, rel),
            (None, None) => true,
            _ => false,
        };
        if !same_area {
            out.insert(
                "area".into(),
                Change {
                    old: json!(fa.area.map(super::round)),
                    new: json!(fb.area.map(super::round)),
                },
            );
        }
        if fa.adjacency != fb.adjacency {
            let (removed, added) = multiset_changes(&fa.adjacency, &fb.adjacency);
            out.insert(
                "adjacency".into(),
                Change {
                    old: json!(removed),
                    new: json!(added),
                },
            );
        }
        if self.aligned {
            let line = fa.normal.is_none() || fb.normal.is_none();
            let dir = |f: &super::FaceFingerprint| if line { f.axis } else { f.normal };
            let mut placed = BTreeMap::new();
            self.placement_changes(
                &mut placed,
                (dir(fa), Some(fa.centroid)),
                (dir(fb), Some(fb.centroid)),
                line,
                if freeform {
                    self.th.freeform_rel
                } else {
                    self.th.angle_tol
                },
                false,
            );
            for (k, v) in placed {
                let k = match k.as_str() {
                    "position" => "centroid".to_string(),
                    _ => k,
                };
                out.insert(k, v);
            }
        }
        out
    }

    pub(super) fn match_faces(
        &self,
        features: &[Entry<String>],
        questions: &mut Vec<String>,
    ) -> (Vec<Entry<usize>>, Vec<usize>) {
        let (n, m) = (self.old.faces.len(), self.new.faces.len());
        let mut fwd: Vec<Option<usize>> = vec![None; n];
        let mut back: Vec<Option<usize>> = vec![None; m];
        let link =
            |fwd: &mut Vec<Option<usize>>, back: &mut Vec<Option<usize>>, a: usize, b: usize| {
                if fwd[a].is_none() && back[b].is_none() {
                    fwd[a] = Some(b);
                    back[b] = Some(a);
                }
            };

        // Seeds: faces that coincide, one to one.
        if self.aligned {
            let mut by_old: Vec<Vec<usize>> = vec![Vec::new(); n];
            let mut per_new = vec![0usize; m];
            for (a, list) in by_old.iter_mut().enumerate() {
                for b in 0..m {
                    if self.coincide(a, b) {
                        list.push(b);
                        per_new[b] += 1;
                    }
                }
            }
            for (a, list) in by_old.iter().enumerate() {
                if let [b] = list[..]
                    && per_new[b] == 1
                {
                    link(&mut fwd, &mut back, a, b);
                }
            }
        }

        // Seeds: the faces of matched features, assigned within each pair.
        let new_index: HashMap<&str, usize> = self
            .new
            .features
            .iter()
            .enumerate()
            .map(|(j, f)| (f.id.as_str(), j))
            .collect();
        for (i, e) in features.iter().enumerate() {
            let (Class::Carried | Class::Adapted, Some(id)) = (e.class, &e.new) else {
                continue;
            };
            let mut a_faces: Vec<usize> = self.old.features[i]
                .faces
                .iter()
                .copied()
                .filter(|&a| fwd[a].is_none())
                .collect();
            let mut b_faces: Vec<usize> = self.new.features[new_index[id.as_str()]]
                .faces
                .iter()
                .copied()
                .filter(|&b| back[b].is_none())
                .collect();
            // A matched plate establishes its two real supporting skins (#134). An edge
            // treatment may remove most of one skin's area and replace its adjacency, so
            // area-based global coincidence/cost is no longer a sound seed for that skin.
            // Seed only a mutually unique, equally oriented supporting plane within this
            // matched feature; keep the actual area/centroid/adjacency changes below.
            if self.aligned && self.axis.is_none() && self.old.features[i].family == "plates" {
                let same_plane = |a: usize, b: usize| {
                    let (fa, fb) = (&self.old.faces[a], &self.new.faces[b]);
                    fa.surface == "plane"
                        && fb.surface == "plane"
                        && matches!((fa.axis, fb.axis), (Some(x), Some(y))
                            if geom::dot(self.motion.dir(x), y) >= self.th.angle_tol.cos()
                            && geom::dot(geom::sub(self.motion.point(fa.centroid), fb.centroid), y).abs() <= self.tol())
                };
                for &a in &a_faces {
                    let bs: Vec<_> = b_faces
                        .iter()
                        .copied()
                        .filter(|&b| same_plane(a, b))
                        .collect();
                    if let [b] = bs[..]
                        && a_faces.iter().filter(|&&a2| same_plane(a2, b)).count() == 1
                    {
                        link(&mut fwd, &mut back, a, b);
                    }
                }
                a_faces.retain(|&a| fwd[a].is_none());
                b_faces.retain(|&b| back[b].is_none());
            }
            if a_faces.is_empty() || b_faces.is_empty() {
                continue;
            }
            let costs: Vec<Vec<f64>> = a_faces
                .iter()
                .map(|&a| {
                    b_faces
                        .iter()
                        .map(|&b| self.face_cost(a, b).unwrap_or(assign::FORBIDDEN))
                        .collect()
                })
                .collect();
            for (r, outcome) in self.resolve(&costs, b_faces.len()).into_iter().enumerate() {
                if let Outcome::Matched(c, _) = outcome {
                    link(&mut fwd, &mut back, a_faces[r], b_faces[c]);
                }
            }
        }

        self.propagate(&mut fwd, &mut back);

        // What the seeds did not reach: assigned by surface type, then propagated again.
        let mut ambiguous: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        let mut kinds: Vec<&String> = self.old.faces.iter().map(|f| &f.surface).collect();
        kinds.sort();
        kinds.dedup();
        for kind in kinds {
            let olds: Vec<usize> = (0..n)
                .filter(|&a| fwd[a].is_none() && &self.old.faces[a].surface == kind)
                .collect();
            let news: Vec<usize> = (0..m)
                .filter(|&b| back[b].is_none() && &self.new.faces[b].surface == kind)
                .collect();
            if olds.is_empty()
                || news.is_empty()
                || olds.len() > GLOBAL_LIMIT
                || news.len() > GLOBAL_LIMIT
            {
                continue;
            }
            let costs: Vec<Vec<f64>> = olds
                .iter()
                .map(|&a| {
                    news.iter()
                        .map(|&b| self.face_cost(a, b).unwrap_or(assign::FORBIDDEN))
                        .collect()
                })
                .collect();
            for (r, outcome) in self.resolve(&costs, news.len()).into_iter().enumerate() {
                match outcome {
                    Outcome::Matched(c, _) => link(&mut fwd, &mut back, olds[r], news[c]),
                    Outcome::Ambiguous(cs) => {
                        ambiguous.insert(olds[r], cs.into_iter().map(|c| news[c]).collect());
                    }
                    Outcome::Unmatched => {}
                }
            }
        }
        self.propagate(&mut fwd, &mut back);

        let mut entries = Vec::with_capacity(n);
        let mut candidate_of = vec![false; m];
        for a in 0..n {
            let entry = match fwd[a] {
                Some(b) => {
                    let changes = self.face_changes(a, b);
                    Entry {
                        old: a,
                        class: if changes.is_empty() {
                            Class::Carried
                        } else {
                            Class::Adapted
                        },
                        new: Some(b),
                        candidates: Vec::new(),
                        changes,
                        cost: self.face_cost(a, b).map(super::round),
                    }
                }
                None => match ambiguous.get(&a) {
                    Some(cs) if cs.iter().any(|&b| back[b].is_none()) => {
                        let cs: Vec<usize> =
                            cs.iter().copied().filter(|&b| back[b].is_none()).collect();
                        for &b in &cs {
                            candidate_of[b] = true;
                        }
                        questions.push(format!(
                            "Which of new faces {cs:?} is old face {a}? They are alike within the margin."
                        ));
                        Entry {
                            old: a,
                            class: Class::Ambiguous,
                            new: None,
                            candidates: cs,
                            changes: BTreeMap::new(),
                            cost: None,
                        }
                    }
                    _ => Entry {
                        old: a,
                        class: Class::Orphaned,
                        new: None,
                        candidates: Vec::new(),
                        changes: BTreeMap::new(),
                        cost: None,
                    },
                },
            };
            entries.push(entry);
        }
        let new_faces = (0..m)
            .filter(|&b| back[b].is_none() && !candidate_of[b])
            .collect();
        (entries, new_faces)
    }

    /// Spreads matches across the adjacency graph until none is clear: an unmatched old face
    /// next to a matched one pairs with an unmatched new face next to its partner, across an
    /// edge of the same kind, when each is the other's cheapest such candidate by the margin.
    fn propagate(&self, fwd: &mut [Option<usize>], back: &mut [Option<usize>]) {
        loop {
            // Each unmatched old face's candidates: the faces next to its matched neighbours'
            // partners, each with how many of those neighbours it is next to (across an edge of
            // the same kind).
            let mut support: BTreeMap<usize, BTreeMap<usize, usize>> = BTreeMap::new();
            let mut matched_neighbours: BTreeMap<usize, usize> = BTreeMap::new();
            for (a, fa) in self.old.faces.iter().enumerate() {
                if fwd[a].is_some() {
                    continue;
                }
                for (x, arc) in &fa.neighbours {
                    let Some(x2) = fwd[*x] else {
                        continue;
                    };
                    *matched_neighbours.entry(a).or_default() += 1;
                    for (b, arc2) in &self.new.faces[x2].neighbours {
                        if back[*b].is_none() && arc2 == arc {
                            *support.entry(a).or_default().entry(*b).or_default() += 1;
                        }
                    }
                }
            }
            // The cost of each: the face cost, plus the fraction of the old face's matched
            // neighbours whose partners the new face is not next to.
            let mut options: BTreeMap<usize, BTreeMap<usize, f64>> = BTreeMap::new();
            for (a, bs) in &support {
                let total = matched_neighbours[a] as f64;
                for (b, n) in bs {
                    if let Some(c) = self.face_cost(*a, *b) {
                        let c = c + (1.0 - *n as f64 / total);
                        options.entry(*a).or_default().insert(*b, c);
                    }
                }
            }
            let mut by_new: BTreeMap<usize, Vec<(f64, usize)>> = BTreeMap::new();
            for (a, opts) in &options {
                for (b, c) in opts {
                    by_new.entry(*b).or_default().push((*c, *a));
                }
            }
            let best = |list: &mut Vec<(f64, usize)>| -> Option<(f64, usize, f64)> {
                list.sort_by(|p, q| p.0.total_cmp(&q.0).then(p.1.cmp(&q.1)));
                let (c, k) = *list.first()?;
                let next = list.get(1).map_or(f64::INFINITY, |p| p.0);
                Some((c, k, next))
            };
            let mut accepted: Vec<(usize, usize)> = Vec::new();
            for (a, opts) in &options {
                let mut list: Vec<(f64, usize)> = opts.iter().map(|(b, c)| (*c, *b)).collect();
                let Some((c, b, next)) = best(&mut list) else {
                    continue;
                };
                if c > self.th.cutoff || next < c + self.th.margin {
                    continue;
                }
                let Some((c2, a2, next2)) = best(by_new.get_mut(&b).unwrap()) else {
                    continue;
                };
                if a2 == *a && c2 == c && next2 >= c + self.th.margin {
                    accepted.push((*a, b));
                }
            }
            if accepted.is_empty() {
                return;
            }
            for (a, b) in accepted {
                fwd[a] = Some(b);
                back[b] = Some(a);
            }
        }
    }
}
