//! Scratch analysis (not part of the repo): why are some classes' false positives high and
//! recall low? Reuses the scorer's own mapping, selectors and matching (support/mfinstseg_scratch.rs
//! is examples/support/mfinstseg.rs plus `detail`).
#[path = "../src/engine.rs"]
mod engine;
pub use engine::*;
extern crate self as quiddity;
#[path = "support/mfinstseg_scratch.rs"]
mod mfinstseg;

use std::collections::BTreeMap;
use std::sync::{Mutex, atomic::{AtomicUsize, Ordering}};

type Tally = BTreeMap<String, u64>;
#[derive(Default, serde::Serialize)]
struct Out {
    // class id -> ...
    fp_total: BTreeMap<usize, u64>,
    fp_majority_label: BTreeMap<usize, Tally>,
    fp_shared: BTreeMap<usize, u64>,
    fp_shared_with: BTreeMap<usize, Tally>,
    fp_by_source_family: BTreeMap<usize, Tally>,
    miss_total: BTreeMap<usize, u64>,
    miss_top_family: BTreeMap<usize, Tally>,
    miss_unclaimed: BTreeMap<usize, u64>,
    miss_under_half: BTreeMap<usize, u64>,
    models: u64,
}
fn bump(m: &mut Tally, k: &str) { *m.entry(k.to_string()).or_default() += 1; }

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let root = std::path::Path::new(&args[1]);
    let ids: Vec<String> = std::fs::read_to_string(&args[2])?.lines().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect();
    let workers: usize = args[4].parse()?;
    let classes = mfinstseg::mapping();
    let names: Vec<String> = classes.iter().map(|c| c.name.clone()).collect();
    let fp_classes = [6usize, 16, 22, 14, 0, 12];
    let miss_classes = [5usize, 10, 0, 17, 7, 3];
    let next = AtomicUsize::new(0);
    let out = Mutex::new(Out::default());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            std::thread::Builder::new().stack_size(256 << 20).spawn_scoped(scope, || loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(id) = ids.get(i) else { break };
                let Ok(truth) = mfinstseg::Truth::read(root, id) else { continue };
                if truth.instances.is_none() { continue; }
                let Ok(bytes) = std::fs::read(root.join("steps").join(format!("{id}.step"))) else { continue };
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let part = quiddity::read_step(&bytes).ok()?;
                    if part.faces.len() != truth.seg.len() { return None; }
                    Some(quiddity::features::recognise(&part))
                }));
                let Ok(Some(features)) = r else { continue };
                let detail = mfinstseg::detail(&truth, &features, &classes);
                // face -> families that define it (unmarked records only, as the scorer)
                let mut claim: Vec<Vec<String>> = vec![vec![]; truth.seg.len()];
                for (fam, recs) in features.defining.iter() {
                    let mut seen = std::collections::BTreeSet::new();
                    for (idx, faces) in recs.iter().enumerate() {
                        let dup = match *fam {
                            "holes" => features.holes.get(idx).is_some_and(|r| r.superseded_by.is_some()),
                            "bosses" => features.bosses.get(idx).is_some_and(|r| r.superseded_by.is_some()),
                            _ => false,
                        };
                        if dup { continue; }
                        for &f in faces { if seen.insert(f) { claim[f].push(fam.to_string()); } }
                    }
                }
                // which (family,index) records are predictions of which classes
                let mut used_by: BTreeMap<(String, usize), Vec<usize>> = BTreeMap::new();
                for (c, d) in detail.iter().enumerate() {
                    for p in &d.preds { for s in &p.sources { used_by.entry(s.clone()).or_default().push(c); } }
                }
                let mut o = out.lock().unwrap();
                o.models += 1;
                for &c in &fp_classes {
                    for p in detail[c].preds.iter().filter(|p| !p.matched) {
                        *o.fp_total.entry(c).or_default() += 1;
                        let mut cnt: BTreeMap<usize, usize> = BTreeMap::new();
                        for &f in &p.faces { *cnt.entry(truth.seg[f]).or_default() += 1; }
                        let maj = cnt.iter().max_by_key(|(k, v)| (**v, std::cmp::Reverse(**k))).map(|(k, _)| *k).unwrap();
                        bump(o.fp_majority_label.entry(c).or_default(), &names[maj]);
                        for s in &p.sources { bump(o.fp_by_source_family.entry(c).or_default(), &s.0); }
                        let mut others: std::collections::BTreeSet<usize> = Default::default();
                        for s in &p.sources { for &oc in &used_by[s] { if oc != c { others.insert(oc); } } }
                        if !others.is_empty() {
                            *o.fp_shared.entry(c).or_default() += 1;
                            for oc in others { bump(o.fp_shared_with.entry(c).or_default(), &names[oc]); }
                        }
                    }
                }
                for &c in &miss_classes {
                    for g in &detail[c].missed_labelled {
                        *o.miss_total.entry(c).or_default() += 1;
                        let mut fam_cnt: BTreeMap<&str, usize> = BTreeMap::new();
                        let mut claimed = 0usize;
                        for &f in g {
                            if !claim[f].is_empty() { claimed += 1; }
                            for fam in &claim[f] { *fam_cnt.entry(fam.as_str()).or_default() += 1; }
                        }
                        if claimed == 0 { *o.miss_unclaimed.entry(c).or_default() += 1; bump(o.miss_top_family.entry(c).or_default(), "(nothing)"); continue; }
                        if 2 * claimed < g.len() { *o.miss_under_half.entry(c).or_default() += 1; }
                        let top = fam_cnt.iter().max_by_key(|(k, v)| (**v, std::cmp::Reverse(**k))).map(|(k, _)| *k).unwrap();
                        bump(o.miss_top_family.entry(c).or_default(), top);
                    }
                }
            }).unwrap();
        }
    });
    let o = out.into_inner().unwrap();
    std::fs::write(&args[3], serde_json::to_string_pretty(&o)?)?;
    Ok(())
}
