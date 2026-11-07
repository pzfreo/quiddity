//! External-data accuracy ratchet. Run with QUIDDITY_MFINSTSEG=/path/to/MFInstSeg.
//! No dataset files are redistributed: permission to redistribute was not established.
//! `tools/update_mfinstseg_baseline.py` records improvements without allowing regressions.
#[path = "../../examples/support/mfinstseg.rs"]
mod scoring;

/// All seven PARABOLA-bearing files in the 62,495-model dataset. Open CASCADE 8.0
/// reads every solid as valid; these volumes use BRepGProp at Eps=1e-10.
/// The dataset stays external, just as it does for the label ratchet below.
#[test]
fn parabola_models_keep_every_face_and_edge() {
    let Some(root) = std::env::var_os("QUIDDITY_MFINSTSEG") else {
        eprintln!("external parabola regression skipped: set QUIDDITY_MFINSTSEG to enable");
        return;
    };
    for (id, faces, edges, volume) in [
        ("20221121_154648_21526", 34, 89, 45956.2418205826),
        ("20221121_154648_4306", 15, 41, 4973.484360445477),
        ("20221123_142528_10559", 38, 97, 28823.819620136084),
        ("20221123_142528_13568", 20, 48, 21052.923274488247),
        ("20221123_142528_19464", 30, 70, 147656.72812030592),
        ("20221124_154714_13230", 20, 49, 58543.0670631267),
        ("20221124_154714_16830", 12, 29, 223279.54767780998),
    ] {
        let path = Path::new(&root).join("steps").join(format!("{id}.step"));
        let part = quiddity::read_step_file(&path).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!((part.faces.len(), part.edges.len()), (faces, edges), "{id}");
        assert!(part.unresolved_faces().is_empty(), "{id}");
        assert!(part.unresolved_edges().is_empty(), "{id}");
        assert_eq!(part.solids.len(), 1, "{id}");
        assert!(part.solid_is_valid(0), "{id}");
        let actual = part.solid_mass(0).unwrap().0;
        assert!(
            (actual - volume).abs() < 1e-7 * volume,
            "{id}: {actual} vs {volume}"
        );
    }
}

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Deserialize)]
struct Model {
    id: String,
    step_sha256: String,
    labels_sha256: String,
    accounting: scoring::Accounting,
}
#[derive(Deserialize)]
struct Baseline {
    schema: usize,
    mapping_sha256: String,
    models: Vec<Model>,
    scores: Vec<scoring::Score>,
    accounting: scoring::Accounting,
}
fn hash(path: &Path) -> String {
    Sha256::digest(std::fs::read(path).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn accounting_baseline_is_complete_and_totals_match() {
    let baseline: Baseline =
        serde_json::from_str(include_str!("../fixtures/mfinstseg_baseline.json")).unwrap();
    assert_eq!(baseline.schema, 2);
    assert_eq!(baseline.models.len(), 200);
    let mut total = scoring::Accounting::default();
    for model in &baseline.models {
        assert_eq!(model.accounting.feature_as_body, 0, "{}", model.id);
        assert!(model.accounting.stock_unexplained <= model.accounting.unexplained);
        assert_eq!(model.accounting.stock_body, model.accounting.body);
        assert_eq!(
            model.accounting.body_reasons.values().sum::<u64>(),
            model.accounting.body
        );
        total.add(&model.accounting);
    }
    assert_eq!(total, baseline.accounting);
}

/// Exact rational comparison, with the scorer's zero-denominator convention.
fn no_drop(got: &scoring::Counts, baseline: &scoring::Counts) -> bool {
    fn ratio_ge(n: u64, d: u64, bn: u64, bd: u64) -> bool {
        let (n, d) = if d == 0 { (0, 1) } else { (n, d) };
        let (bn, bd) = if bd == 0 { (0, 1) } else { (bn, bd) };
        u128::from(n) * u128::from(bd) >= u128::from(bn) * u128::from(d)
    }
    ratio_ge(
        got.true_positive,
        got.predicted,
        baseline.true_positive,
        baseline.predicted,
    ) && ratio_ge(
        got.true_positive,
        got.labelled,
        baseline.true_positive,
        baseline.labelled,
    )
}

#[test]
fn rational_ratchet_detects_precision_and_recall_regressions() {
    use scoring::Counts;
    let base = Counts {
        true_positive: 3,
        predicted: 4,
        labelled: 5,
    };
    assert!(!no_drop(
        &Counts {
            true_positive: 3,
            predicted: 5,
            labelled: 5
        },
        &base
    ));
    assert!(!no_drop(
        &Counts {
            true_positive: 2,
            predicted: 2,
            labelled: 5
        },
        &base
    ));
    assert!(no_drop(
        &Counts {
            true_positive: 4,
            predicted: 5,
            labelled: 5
        },
        &base
    ));
    assert!(!no_drop(&Counts::default(), &base));
    assert!(no_drop(&base, &Counts::default()));
}

#[test]
fn independent_label_accuracy_ratchet() {
    let Some(root) = std::env::var_os("QUIDDITY_MFINSTSEG") else {
        eprintln!("external MFInstSeg ratchet skipped: set QUIDDITY_MFINSTSEG to enable");
        return;
    };
    let root = Path::new(&root);
    let baseline: Baseline =
        serde_json::from_str(include_str!("../fixtures/mfinstseg_baseline.json")).unwrap();
    assert_eq!(baseline.schema, 2);
    assert_eq!(baseline.models.len(), 200);
    assert_eq!(
        hash(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mfinstseg_mapping.json")),
        baseline.mapping_sha256,
        "mapping changed: review evidence and rebaseline explicitly"
    );
    let classes = scoring::mapping();
    assert_eq!(baseline.scores.len(), classes.len());
    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..6 {
            std::thread::Builder::new()
                .stack_size(256 << 20)
                .spawn_scoped(scope, || {
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(model) = baseline.models.get(i) else {
                            break;
                        };
                        assert_eq!(
                            hash(&root.join("steps").join(format!("{}.step", model.id))),
                            model.step_sha256,
                            "{} STEP changed",
                            model.id
                        );
                        assert_eq!(
                            hash(&root.join("labels").join(format!("{}.json", model.id))),
                            model.labels_sha256,
                            "{} labels changed",
                            model.id
                        );
                        results
                            .lock()
                            .unwrap()
                            .push(scoring::evaluate(root, &model.id, false, &classes).unwrap());
                    }
                })
                .unwrap();
        }
    });
    let mut scores = vec![scoring::Score::default(); classes.len()];
    let mut accounting = scoring::Accounting::default();
    for model in results.into_inner().unwrap() {
        let base = &baseline
            .models
            .iter()
            .find(|m| m.id == model.id)
            .unwrap()
            .accounting;
        assert_eq!(
            model.accounting.feature_as_body, 0,
            "{}: feature face hidden as body",
            model.id
        );
        assert!(
            model.accounting.stock_unexplained <= base.stock_unexplained,
            "{}: stock unexplained regressed: {} -> {}",
            model.id,
            base.stock_unexplained,
            model.accounting.stock_unexplained
        );
        accounting.add(&model.accounting);
        assert!(
            model.instances_available,
            "{}: incomplete instance labels",
            model.id
        );
        assert!(
            model.label_warnings.is_empty(),
            "{}: {:?}",
            model.id,
            model.label_warnings
        );
        assert!(model.error.is_none(), "{}: {:?}", model.id, model.error);
        assert!(
            model.section_recess_refusal.is_none(),
            "{}: {:?}",
            model.id,
            model.section_recess_refusal
        );
        for (total, s) in scores.iter_mut().zip(&model.scores) {
            total.add(s);
        }
    }
    assert_eq!(accounting.feature_as_body, 0);
    assert!(accounting.stock_unexplained <= baseline.accounting.stock_unexplained);
    eprintln!("face accounting: {accounting:?}");
    for ((c, got), base) in classes.iter().zip(&scores).zip(&baseline.scores) {
        assert_eq!(got.faces.labelled, base.faces.labelled);
        assert!(
            no_drop(&got.faces, &base.faces),
            "{} faces regressed: {:?} -> {:?}",
            c.name,
            base.faces,
            got.faces
        );
        let (got_i, base_i) = (
            got.instances.as_ref().unwrap(),
            base.instances.as_ref().unwrap(),
        );
        assert_eq!(got_i.labelled, base_i.labelled);
        assert!(
            no_drop(got_i, base_i),
            "{} instances regressed: {:?} -> {:?}",
            c.name,
            base_i,
            got_i
        );
        eprintln!(
            "{}: face P/R {:.6}/{:.6}, instance P/R {:.6}/{:.6}",
            c.name,
            got.faces.precision(),
            got.faces.recall(),
            got_i.precision(),
            got_i.recall()
        );
        if got != base {
            eprintln!(
                "{} counts changed without lowering a score; record improvements with tools/update_mfinstseg_baseline.py",
                c.name
            );
        }
    }
}

/// The reported #211 remnant's two walls; the perpendicular secondary-cut end is context.
#[test]
fn local_step_repro_keeps_the_labelled_defining_faces() {
    let Some(root) = std::env::var_os("QUIDDITY_MFINSTSEG") else {
        return;
    };
    let part =
        quiddity::read_step_file(&Path::new(&root).join("steps/20221121_154647_1.step")).unwrap();
    let found = quiddity::features::local_steps::discover_verified(
        &quiddity::features::Context::new(&part),
    )
    .unwrap();
    let step = found
        .iter()
        .find(|s| s.defining == [7, 8])
        .expect("#211 walls 7–8");
    assert_eq!(step.record.length, 4.061);
    assert_eq!((step.record.height, step.record.depth), (2., 5.423));
    assert_eq!(step.record.ends[1].scope, "local");
    assert_eq!(step.record.ends[1].faces, [23]);
    assert!(step.context.contains(&23));
    let accepted = quiddity::features::recognise(&part);
    assert!(accepted.defining["local_steps"].contains(&vec![7, 8]));
    let published = quiddity::correspondence::recognise(&part);
    let document = quiddity::recognition::document(&published, part.faces.len());
    assert!(
        document
            .records
            .iter()
            .any(|r| r.family == "local_steps" && r.faces == [7, 8])
    );
}

/// Polygonal floor with a local convex opening lip; neither is approximated by stock bounds.
#[test]
fn polygonal_slanted_step_1002_keeps_its_six_corners_and_defining_faces() {
    let Some(root) = std::env::var_os("QUIDDITY_MFINSTSEG") else {
        return;
    };
    let path = Path::new(&root).join("steps/20221121_154647_1002.step");
    let part = quiddity::read_step_file(&path).unwrap();
    let found = quiddity::features::polygonal_oblique_through_steps::discover_verified(
        &quiddity::features::Context::new(&part),
    )
    .unwrap();
    assert_eq!(
        found.len(),
        1,
        "the other polygonal pair [3,4] is a triangular blind step"
    );
    let step = &found[0];
    assert_eq!(step.defining, vec![20, 21]);
    assert_eq!(step.context, vec![6, 11, 14, 18, 19, 22]);
    assert_eq!(step.record.floor_outline.len(), 6);
    assert_eq!(step.record.length, 88.169);
    assert_eq!(step.record.depth, 30.02);
    assert_eq!(step.record.slant_degrees, 3.100929);
    assert_eq!(step.record.through_axis, [0., 0., 1.]);
    assert_eq!(step.record.run, [0.054095, 0., -0.998536]);
    let all = quiddity::features::recognise(&part);
    assert_eq!(
        all.polygonal_oblique_through_steps,
        vec![step.record.clone()]
    );
    assert_eq!(
        all.defining["polygonal_oblique_through_steps"],
        vec![vec![20, 21]]
    );
}

#[test]
fn internal_bevel_1005_has_the_labelled_strip_and_measured_support_legs() {
    let Some(root) = std::env::var_os("QUIDDITY_MFINSTSEG") else {
        return;
    };
    let part = quiddity::read_step_file(&Path::new(&root).join("steps/20221121_154647_1005.step"))
        .unwrap();
    let found = quiddity::features::internal_bevels::discover_verified(
        &quiddity::features::Context::new(&part),
    )
    .unwrap();
    eprintln!("internal repro: {found:?}");
    let bevel = found
        .iter()
        .find(|b| b.defining == [2])
        .expect("#213 internal bevel face 2");
    assert_eq!(bevel.context, vec![0, 1, 4, 6]);
    assert_eq!(
        (bevel.record.leg1, bevel.record.leg2, bevel.record.angle),
        (0.574, 0.574, 45.)
    );
    assert_eq!(bevel.record.length, 5.639);
    assert_eq!(*bevel.record.corner, [35.732, 62.383, 9.641]);
    let all = quiddity::features::recognise(&part);
    assert!(all.defining["internal_bevels"].contains(&vec![2]));
    let published = quiddity::correspondence::recognise(&part);
    let document = quiddity::recognition::document(&published, part.faces.len());
    assert!(
        document
            .records
            .iter()
            .any(|r| r.family == "internal_bevels" && r.faces == [2])
    );
}

/// Audit a lexical label sample covering #208 plus the fixed ratchet.
/// Dataset labels remain external; a class-0 prediction must really be a chamfer face.
#[test]
fn internal_bevel_additions_have_independent_chamfer_labels() {
    let Some(root) = std::env::var_os("QUIDDITY_MFINSTSEG") else {
        return;
    };
    let root = Path::new(&root);
    let mut ids: std::collections::BTreeSet<String> =
        serde_json::from_str::<Baseline>(include_str!("../fixtures/mfinstseg_baseline.json"))
            .unwrap()
            .models
            .into_iter()
            .map(|m| m.id)
            .collect();
    // Cover the investigation IDs (through lexical model 1117), with a small
    // independent extension; do not choose candidates by their bevel labels.
    let mut labels = std::fs::read_dir(root.join("labels"))
        .unwrap()
        .map(|p| p.unwrap().path())
        .collect::<Vec<_>>();
    labels.sort();
    for path in labels.into_iter().take(1200) {
        ids.insert(path.file_stem().unwrap().to_str().unwrap().to_owned());
    }
    let mut additions = 0;
    for id in ids {
        let part =
            quiddity::read_step_file(&root.join("steps").join(format!("{id}.step"))).unwrap();
        let found = quiddity::features::internal_bevels::discover_verified(
            &quiddity::features::Context::new(&part),
        )
        .unwrap();
        if found.is_empty() {
            continue;
        }
        let labels: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join("labels").join(format!("{id}.json"))).unwrap(),
        )
        .unwrap();
        for bevel in found {
            for face in &bevel.defining {
                assert_eq!(labels[0][1]["seg"][face.to_string()], 0, "{id}, {bevel:?}");
            }
            additions += 1;
            eprintln!(
                "internal bevel addition: {id} {:?} {:?}",
                bevel.defining, bevel.record
            );
        }
    }
    assert!(additions > 0);
}
