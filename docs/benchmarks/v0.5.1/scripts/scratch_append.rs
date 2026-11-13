
// ---- scratch additions (confusion analysis) ----
pub struct Pred {
    pub faces: Faces,
    pub sources: Vec<(String, usize)>,
    pub matched: bool,
}
pub struct ClassDetail {
    pub preds: Vec<Pred>,
    pub missed_labelled: Vec<Faces>,
}

/// Same augmenting-path matching as `matched`, but returns which predictions and which
/// labelled instances were matched.
fn matched_sets(predicted: &[Faces], labelled: &[Faces]) -> (Vec<bool>, Vec<bool>) {
    let edges: Vec<Vec<usize>> = predicted
        .iter()
        .map(|p| {
            labelled
                .iter()
                .enumerate()
                .filter_map(|(i, g)| {
                    let intersection = p.intersection(g).count();
                    let union = p.len() + g.len() - intersection;
                    (union > 0 && 2 * intersection >= union).then_some(i)
                })
                .collect()
        })
        .collect();
    fn augment(p: usize, edges: &[Vec<usize>], used: &mut [bool], owner: &mut [Option<usize>]) -> bool {
        for &g in &edges[p] {
            if used[g] {
                continue;
            }
            used[g] = true;
            if owner[g].is_none_or(|old| augment(old, edges, used, owner)) {
                owner[g] = Some(p);
                return true;
            }
        }
        false
    }
    let mut owner = vec![None; labelled.len()];
    let pm: Vec<bool> = (0..predicted.len())
        .map(|p| augment(p, &edges, &mut vec![false; labelled.len()], &mut owner))
        .collect();
    let lm: Vec<bool> = owner.iter().map(|o| o.is_some()).collect();
    (pm, lm)
}

/// The scorer's per-class instance predictions (same family loop, selectors, superseded
/// filter and face-set de-duplication as `score`), with their source records and matching.
pub fn detail(truth: &Truth, features: &quiddity::features::Features, classes: &[Class]) -> Vec<ClassDetail> {
    classes
        .iter()
        .map(|class| {
            let mut by_set: BTreeMap<Faces, Vec<(String, usize)>> = BTreeMap::new();
            for family in &class.families {
                let selectors = class.selectors.get(family);
                let records = selectors.map(|_| family_records(features, family));
                for (i, faces) in attributed_faces(features, family) {
                    let ok = selectors.is_none_or(|selectors| {
                        selectors.iter().all(|s| s.matches(&records.as_ref().unwrap()[i]))
                    });
                    if ok {
                        by_set
                            .entry(faces.iter().copied().collect())
                            .or_default()
                            .push((family.clone(), i));
                    }
                }
            }
            let sets: Vec<Faces> = by_set.keys().cloned().collect();
            let labelled: Vec<Faces> = truth.instances.as_ref().map(|g| g[class.id].clone()).unwrap_or_default();
            let (pm, lm) = matched_sets(&sets, &labelled);
            let preds = by_set
                .into_iter()
                .zip(pm)
                .map(|((faces, sources), matched)| Pred { faces, sources, matched })
                .collect();
            let missed_labelled = labelled.into_iter().zip(lm).filter(|(_, m)| !m).map(|(g, _)| g).collect();
            ClassDetail { preds, missed_labelled }
        })
        .collect()
}
