//! Independent labelled accuracy, without Python quiddity output.
//! `cargo run --release --example mfinstseg_score -- DATASET IDS.txt OUT.json [WORKERS] [--mfcadpp]`
//! MFInstSeg DATASET has steps/ and labels/. For MFCAD++, DATASET is the STEP directory;
//! numeric face names supply semantic labels, and instance metrics are absent.
//! Use sorted STEP stems[::5] for the coverage_sweep selection (12,499 / 62,495 models).
//! Ground-truth or configuration errors fail the run. Read/panic failures with available
//! labels are explicitly reported and count as misses; zero denominators score zero.
//! With `MFINSTSEG_PER_MODEL=PATH`, also writes each model's nonzero per-class counts to PATH,
//! so two runs can be compared model by model.
// Internal diagnostic: use the exact engine implementation without exporting caller-space APIs.
#[path = "../src/engine.rs"]
mod engine;
pub use engine::*;
extern crate self as quiddity;
#[path = "support/mfinstseg.rs"]
mod mfinstseg;

use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() < 4 {
        return Err("usage: mfinstseg_score DATASET IDS.txt OUT.json [WORKERS] [--mfcadpp]".into());
    }
    let root = std::path::Path::new(&args[1]);
    let ids: Vec<_> = std::fs::read_to_string(&args[2])?
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    if ids.is_empty() || ids.iter().collect::<std::collections::BTreeSet<_>>().len() != ids.len() {
        return Err("ids must be nonempty and unique".into());
    }
    let workers: usize = args
        .get(4)
        .filter(|s| !s.starts_with("--"))
        .map_or(Ok(6), |s| s.parse())?;
    if !(1..=6).contains(&workers) {
        return Err("workers must be 1..=6".into());
    }
    let semantic_only = args.iter().any(|s| s == "--mfcadpp");
    let classes = mfinstseg::mapping();
    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            std::thread::Builder::new()
                .stack_size(256 << 20)
                .spawn_scoped(scope, || {
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(id) = ids.get(i) else {
                            break;
                        };
                        let result = mfinstseg::evaluate(root, id, semantic_only, &classes);
                        results.lock().unwrap().push((i, result));
                        if i.is_multiple_of(1000) {
                            eprintln!("{i}/{}", ids.len());
                        }
                    }
                })
                .unwrap();
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|(i, _)| *i);
    let mut scores = vec![mfinstseg::Score::default(); classes.len()];
    let mut accounting = mfinstseg::Accounting::default();
    let mut holes = mfinstseg::Counts::default();
    let mut any_feature = mfinstseg::Counts::default();
    let mut model_accounting = Vec::new();
    let mut failures = Vec::new();
    let mut errors = Vec::new();
    let mut refusals = Vec::new();
    let mut label_warnings = Vec::new();
    let mut instance_scored_models = 0;
    let mut per_model = Vec::new();
    for (i, result) in results {
        match result {
            Err(e) => errors.push(serde_json::json!({"id":ids[i],"error":e})),
            Ok(model) => {
                accounting.add(&model.accounting);
                holes.add(&model.holes);
                any_feature.add(&model.any_feature);
                model_accounting
                    .push(serde_json::json!({"id":model.id,"accounting":model.accounting}));
                instance_scored_models += usize::from(model.instances_available);
                if !model.label_warnings.is_empty() {
                    label_warnings
                        .push(serde_json::json!({"id":model.id,"warnings":model.label_warnings}));
                }
                for (total, s) in scores.iter_mut().zip(&model.scores) {
                    total.add(s);
                }
                let nonzero: Vec<_> = classes
                    .iter()
                    .zip(&model.scores)
                    .filter(|(_, s)| s.faces.predicted + s.faces.labelled > 0)
                    .map(|(c, s)| serde_json::json!({"class":c.name,"faces":s.faces,"instances":s.instances}))
                    .collect();
                per_model
                    .push(serde_json::json!({"id":model.id,"error":model.error,"classes":nonzero}));
                if let Some(e) = model.error {
                    failures.push(serde_json::json!({"id":model.id,"error":e}));
                }
                if let Some(e) = model.section_recess_refusal {
                    refusals.push(serde_json::json!({"id":model.id,"error":e}));
                }
            }
        }
    }
    let rows: Vec<_> = classes.iter().zip(&scores).map(|(c,s)| serde_json::json!({"id":c.id,"name":c.name,"covered":!c.families.is_empty(),"counts":s,"face_precision":s.faces.precision(),"face_recall":s.faces.recall(),"instance_precision":s.instances.as_ref().map(|s|s.precision()),"instance_recall":s.instances.as_ref().map(|s|s.recall())})).collect();
    let report = serde_json::json!({"schema":2,"models":ids.len(),"holes":holes,"any_feature":any_feature,"accounting":accounting,"model_accounting":model_accounting,"classes":rows,"recognition_failures":failures,"section_recess_refusals":refusals,"label_errors":errors,"label_warnings":label_warnings,"instance_scored_models":instance_scored_models,"face_scored_models":ids.len()-errors.len(),"mapping":serde_json::from_str::<serde_json::Value>(include_str!("../tests/fixtures/mfinstseg_mapping.json"))?});
    std::fs::write(&args[3], serde_json::to_string_pretty(&report)? + "\n")?;
    if let Some(path) = std::env::var_os("MFINSTSEG_PER_MODEL") {
        std::fs::write(path, serde_json::to_string(&per_model)? + "\n")?;
    }
    if !errors.is_empty() {
        return Err(format!("{} label errors; accuracy report incomplete", errors.len()).into());
    }
    if report["accounting"]["feature_as_body"].as_u64() != Some(0) {
        return Err("labelled feature faces classified body; see report".into());
    }
    Ok(())
}
