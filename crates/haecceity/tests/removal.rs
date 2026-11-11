//! The document-level removal plan (`removal.rs`) with both presentation policies, on NIST's
//! AP242 PMI test models and on hand-made files.
//!
//! The seeds stand in for the PMI reader's provenance (the writer stage re-tests with the real
//! one): the semantic-PMI-family instances inversely reachable from a part's
//! `product_definition_shape` or `product_definition` through semantic-PMI-family instances.
//!
//! NIST files: `HAECCEITY_NIST_PMI` names their `NIST-PMI-STEP-Files` directory (else the AP242
//! track's download); skipped when absent unless `HAECCEITY_NIST_PMI_REQUIRED=1`. The pinned
//! counts are `tests/fixtures/ap242/removal/nist_{refuse,remove}.json`;
//! `HAECCEITY_REMOVAL_ACTUAL=<dir>` writes the computed ones there.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use haecceity::express::Family;
use haecceity::p21::{Attribute, Document, Edit, RawEntity};
use haecceity::removal::{self, PresentationPolicy, RemovalPlan, entity_name, instance_family};
use haecceity::step::read_part_definitions;
use serde_json::{Value, json};

fn fixtures() -> PathBuf {
    common::fixtures().join("ap242/removal")
}

/// NIST's MBE PMI test models: `HAECCEITY_NIST_PMI`, else the AP242 track's download.
fn nist_dir() -> Option<PathBuf> {
    let dir = std::env::var("HAECCEITY_NIST_PMI")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(
                "/private/tmp/claude-501/-Users-paul-repos-quiddity-rust/\
                 98a3d09f-22d9-418c-b8db-3b48db0cb346/scratchpad/ap242-nist/NIST-PMI-STEP-Files",
            )
        });
    if dir.is_dir() {
        Some(dir)
    } else {
        assert!(
            std::env::var_os("HAECCEITY_NIST_PMI_REQUIRED").is_none(),
            "NIST PMI files required but {} is missing",
            dir.display()
        );
        None
    }
}

/// The NIST AP242 files, sorted by name.
fn nist_files() -> Option<Vec<PathBuf>> {
    let dir = nist_dir()?;
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".stp") && n.contains("_ap242"))
        })
        .collect();
    out.sort();
    assert!(!out.is_empty(), "no AP242 files in {}", dir.display());
    Some(out)
}

fn name(p: &Path) -> String {
    p.file_name().unwrap().to_string_lossy().into_owned()
}

/// The stand-in seeds: semantic-PMI-family instances inversely reachable from the parts'
/// `product_definition_shape`s and `product_definition`s through semantic-PMI-family instances,
/// with what the CAx-IF practices attach to each of them, which a reader therefore consumes
/// with it: its `id_attribute` (the derived `id`, PMI practice §5.1, Figure 4) and
/// `uuid_attribute`s, and the `property_definition`s defined on it that are not validation
/// properties ('semantic text' notes, §7.4; datum target parameters, §6.6.2; user defined
/// attributes, UDA practice) with their `property_definition_representation`s, and, inversely
/// again, what is built on those (a UDA's `general_property_association`, their ids).
fn seeds(doc: &Document, roots: &[u64]) -> BTreeSet<u64> {
    let is = |id: u64, t: &str| {
        doc.get(id)
            .is_some_and(|e| haecceity::express::is_a(&entity_name(e), t))
    };
    let attribute = |id: u64, i: usize| match doc.get(id) {
        Some(RawEntity::Simple { attributes, .. }) => match attributes.get(i) {
            Some(Attribute::EntityRef(t)) => Some(*t),
            _ => None,
        },
        _ => None,
    };
    let mut out = BTreeSet::new();
    let mut work = roots.to_vec();
    while let Some(x) = work.pop() {
        let root = roots.contains(&x);
        for &r in doc.referrers(x) {
            let family = instance_family(doc, r);
            let attached = !root
                && family == Family::Other
                && (is(r, "id_attribute")
                    || is(r, "uuid_attribute")
                    || (is(r, "property_definition") && attribute(r, 2) == Some(x))
                    || (is(r, "property_definition_representation")
                        && is(x, "property_definition")
                        && attribute(r, 0) == Some(x)));
            if (family == Family::SemanticPmi || attached) && out.insert(r) {
                work.push(r);
            }
        }
    }
    out
}

/// The parts' `product_definition`s and `product_definition_shape`s, by
/// `read_part_definitions`; for a file it refuses (NIST FTC-08 edition 4 has tessellated
/// geometry only), every `product_definition` and the `product_definition_shape`s defined on
/// one.
fn part_roots(doc: &Document) -> Vec<u64> {
    if let Ok(parts) = read_part_definitions(doc.bytes()) {
        return parts
            .iter()
            .flat_map(|p| [p.product_definition, p.shape])
            .collect();
    }
    let is = |id: u64, t: &str| {
        doc.get(id)
            .is_some_and(|e| haecceity::express::is_a(&entity_name(e), t))
    };
    let definitions: Vec<u64> = doc
        .ids()
        .filter(|&id| is(id, "product_definition"))
        .collect();
    let shapes = doc.ids().filter(|&id| {
        is(id, "product_definition_shape")
            && matches!(doc.get(id), Some(RawEntity::Simple { attributes, .. })
                if matches!(attributes.get(2), Some(Attribute::EntityRef(d)) if definitions.contains(d)))
    });
    definitions.iter().copied().chain(shapes).collect()
}

fn counts<'a>(types: impl IntoIterator<Item = &'a str>) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for t in types {
        *out.entry(t.to_string()).or_insert(0) += 1;
    }
    out
}

fn write_actual(name: &str, value: &Value) {
    if let Some(dir) = std::env::var_os("HAECCEITY_REMOVAL_ACTUAL") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            Path::new(&dir).join(name),
            serde_json::to_string_pretty(value).unwrap() + "\n",
        )
        .unwrap();
    }
}

fn pinned(name: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(fixtures().join(name)).unwrap()).unwrap()
}

/// Applies a plan and checks the result: it applies and re-parses, the seed search finds
/// nothing, nothing kept references a removed id, every instance neither removed nor rewritten
/// is byte-identical, and the parts' faces are unchanged. Returns the output.
fn check_applied(label: &str, doc: &Document, plan: &RemovalPlan, problems: &mut Vec<String>) {
    let mut edit = Edit::new();
    plan.into_edit(&mut edit);
    let written = match doc.apply(&edit) {
        Ok(w) => w,
        Err(e) => {
            problems.push(format!("{label}: the edit does not apply: {e}"));
            return;
        }
    };
    let out = match Document::parse(written.bytes.clone()) {
        Ok(d) => d,
        Err(e) => {
            problems.push(format!("{label}: the output does not re-parse: {e}"));
            return;
        }
    };
    let roots = part_roots(&out);
    let again = seeds(&out, &roots);
    if !again.is_empty() {
        problems.push(format!(
            "{label}: the seed search still finds {} instances, e.g. #{}",
            again.len(),
            again.first().unwrap()
        ));
    }
    for id in out.ids() {
        let e = out.get(id).unwrap();
        let mut dangling = Vec::new();
        haecceity::p21::visit_attributes(e, &mut |a| {
            if let Attribute::EntityRef(t) = a
                && plan.removed.contains(t)
            {
                dangling.push(*t);
            }
        });
        if !dangling.is_empty() {
            problems.push(format!(
                "{label}: kept #{id} {} ({}) references removed {dangling:?}",
                entity_name(e),
                instance_family(&out, id).as_str()
            ));
        }
        if plan.removed.contains(&id) {
            problems.push(format!("{label}: removed #{id} is still in the output"));
        } else if !plan.rewritten.contains_key(&id) {
            let a = &doc.bytes()[doc.span(id).unwrap()];
            let b = &written.bytes[out.span(id).unwrap()];
            if a != b {
                problems.push(format!("{label}: kept #{id} is not byte-identical"));
            }
        }
    }
    // Completeness: an item a removed draughting model item association presented, which no
    // kept association presents, is removed.
    let dmia = |e: &RawEntity| {
        haecceity::express::is_a(&entity_name(e), "draughting_model_item_association")
    };
    for id in doc.ids() {
        let e = doc.get(id).unwrap();
        if !dmia(e) || !plan.removed.contains(&id) {
            continue;
        }
        let RawEntity::Simple { attributes, .. } = e else {
            continue;
        };
        let Some(Attribute::EntityRef(item)) = attributes.get(4) else {
            continue;
        };
        let kept_association = doc
            .referrers(*item)
            .iter()
            .any(|&r| !plan.removed.contains(&r) && dmia(doc.get(r).unwrap()));
        if !kept_association && !plan.removed.contains(item) {
            problems.push(format!(
                "{label}: #{item} {}, presented by removed #{id} only, is kept",
                entity_name(doc.get(*item).unwrap())
            ));
        }
    }
    for id in doc.ids() {
        if !plan.removed.contains(&id) && out.get(id).is_none() {
            problems.push(format!("{label}: kept #{id} is missing from the output"));
        }
    }
    // The parts, their faces and edges, as read before and after (or the same refusal).
    let key = |bytes: &[u8]| {
        read_part_definitions(bytes)
            .map(|ps| {
                ps.iter()
                    .map(|p| {
                        (
                            p.product_definition,
                            p.shape,
                            p.name.clone(),
                            p.faces.clone(),
                            p.edges.clone(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .map_err(|e| e.to_string())
    };
    if key(doc.bytes()) != key(&written.bytes) {
        problems.push(format!("{label}: the parts or their faces changed"));
    }
}

struct NistResult {
    name: String,
    seeds: usize,
    refuse: Value,
    remove: Value,
    problems: Vec<String>,
}

fn nist_one(path: &Path) -> NistResult {
    let name = name(path);
    let bytes = std::fs::read(path).unwrap();
    let doc = Document::parse(bytes.clone()).unwrap();
    let seeds = seeds(&doc, &part_roots(&doc));
    let mut problems = Vec::new();

    let refuse = match removal::plan(&doc, &seeds, PresentationPolicy::Refuse) {
        Ok(_) => {
            problems.push(format!("{name}: Refuse planned a removal"));
            Value::Null
        }
        Err(r) => {
            let by: BTreeMap<String, u64> = counts(r.blockers.iter().map(|b| b.entity.as_str()));
            if !r
                .blockers
                .iter()
                .any(|b| b.entity.starts_with("draughting_model_item_association"))
            {
                problems.push(format!(
                    "{name}: Refuse names no draughting model item association"
                ));
            }
            let families = counts(r.blockers.iter().map(|b| b.family.as_str()));
            json!({"blockers": by, "families": families})
        }
    };
    let remove = match removal::plan(&doc, &seeds, PresentationPolicy::RemovePresentation) {
        Ok(plan) => {
            check_applied(&name, &doc, &plan, &mut problems);
            let again =
                removal::plan(&doc, &seeds, PresentationPolicy::RemovePresentation).unwrap();
            if again != plan {
                problems.push(format!("{name}: the plan is not deterministic"));
            }
            let rewritten = counts(
                plan.rewritten
                    .values()
                    .map(entity_name)
                    .collect::<Vec<_>>()
                    .iter()
                    .map(String::as_str),
            );
            json!({
                "removed": plan.report.removed_by_type,
                "rewritten": rewritten,
                "presentation_removed": plan.presentation_removed.len(),
                "left_unreferenced": counts(plan.report.left_unreferenced.iter().map(|(_, t)| t.as_str())),
            })
        }
        Err(r) => json!({"refused": r.to_string()}),
    };
    NistResult {
        name,
        seeds: seeds.len(),
        refuse,
        remove,
        problems,
    }
}

#[test]
fn nist_removal_plans_are_pinned() {
    let Some(files) = nist_files() else { return };
    let results = common::parallel::map(&files, |p| nist_one(p));
    let refuse_pinned = pinned("nist_refuse.json");
    let remove_pinned = pinned("nist_remove.json");
    let mut problems = Vec::new();
    let mut refuse_now = serde_json::Map::new();
    let mut remove_now = serde_json::Map::new();
    for r in &results {
        problems.extend(r.problems.iter().cloned());
        let mut refuse = r.refuse.clone();
        refuse["seeds"] = json!(r.seeds);
        if refuse_pinned["files"][&r.name] != refuse {
            problems.push(format!(
                "{}: Refuse gives {refuse}, pinned {}",
                r.name, refuse_pinned["files"][&r.name]
            ));
        }
        // A refusal is pinned with the reason it is right.
        let mut remove_pin = remove_pinned["files"][&r.name].clone();
        let reason = remove_pin.as_object_mut().and_then(|o| o.remove("reason"));
        if r.remove.get("refused").is_some()
            && !reason
                .as_ref()
                .is_some_and(|x| x.as_str().is_some_and(|x| !x.is_empty()))
        {
            problems.push(format!("{}: the pinned refusal has no reason", r.name));
        }
        if remove_pin != r.remove {
            problems.push(format!(
                "{}: RemovePresentation gives {}, pinned {}",
                r.name, r.remove, remove_pinned["files"][&r.name]
            ));
        }
        refuse_now.insert(r.name.clone(), refuse);
        let mut remove = r.remove.clone();
        if let Some(reason) = remove_pinned["files"][&r.name].get("reason") {
            remove["reason"] = reason.clone();
        }
        remove_now.insert(r.name.clone(), remove);
    }
    for (pinned, now) in [(&refuse_pinned, &refuse_now), (&remove_pinned, &remove_now)] {
        for k in pinned["files"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, _)| k)
        {
            if !now.contains_key(k) {
                problems.push(format!("{k} is pinned but not among the files"));
            }
        }
    }
    write_actual("nist_refuse.json", &json!({"files": refuse_now}));
    write_actual("nist_remove.json", &json!({"files": remove_now}));
    // The three files the brief names must be among those checked.
    for f in [
        "nist_ctc_02_asme1_ap242-e2.stp",
        "nist_ftc_06_asme1_ap242-e2.stp",
        "nist_stc_10_asme1_ap242-e2.stp",
    ] {
        assert!(results.iter().any(|r| r.name == f), "{f} not found");
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

// ---------------------------------------------------------------------------------------------
// Hand-made files
// ---------------------------------------------------------------------------------------------

/// `handmade.stp`, with `edit` applied to its text.
fn handmade(edit: impl Fn(String) -> String) -> Document {
    let text = std::fs::read_to_string(fixtures().join("handmade.stp")).unwrap();
    Document::parse(edit(text).into_bytes()).unwrap()
}

/// The hand-made file's stand-in seeds: its shape aspect, the usage binding it to geometry, its
/// size and the size's representation.
fn handmade_seeds(doc: &Document) -> BTreeSet<u64> {
    assert!(seeds(doc, &[6, 7]).is_superset(&BTreeSet::from([20, 21, 22, 26])));
    BTreeSet::from([20, 21, 22, 26])
}

fn ids(v: &[(u64, String)]) -> Vec<u64> {
    v.iter().map(|(id, _)| *id).collect()
}

#[test]
fn refuse_names_the_presentation_that_references_removed_pmi() {
    let doc = handmade(|t| t);
    let r = removal::plan(&doc, &handmade_seeds(&doc), PresentationPolicy::Refuse).unwrap_err();
    assert_eq!(
        r.blockers,
        vec![removal::Blocker::new(
            53,
            "draughting_model_item_association".into(),
            Family::Presentation,
            vec![22]
        )]
    );
    assert!(
        r.to_string()
            .contains("#53 draughting_model_item_association")
    );
}

#[test]
fn remove_presentation_removes_the_callout_and_rewrites_its_model() {
    let doc = handmade(|t| t);
    let seeds = handmade_seeds(&doc);
    let plan = removal::plan(&doc, &seeds, PresentationPolicy::RemovePresentation).unwrap();
    // The seeds and what only they reference; the association, the callout it presents, its
    // occurrence and geometry, and the annotation plane that held only the callout, with its
    // plane. Never the shared style (#33, also on the B-rep's point), the B-rep's point, the
    // shape representation, the unit or the context.
    assert_eq!(
        plan.removed,
        BTreeSet::from([
            20, 21, 22, 24, 25, 26, 30, 31, 32, 34, 35, 53, 55, 56, 57, 58
        ])
    );
    assert_eq!(
        ids(&plan.presentation_removed),
        vec![30, 31, 32, 34, 35, 53, 55, 56, 57, 58]
    );
    assert_eq!(plan.rewritten.keys().copied().collect::<Vec<_>>(), vec![50]);
    assert_eq!(
        plan.report.rewrites,
        vec![removal::Rewrite {
            id: 50,
            entity: "draughting_model".into(),
            attribute: "representation.items".into(),
            dropped: vec![30, 55],
        }]
    );
    // The model only the removed association referenced stays, reported.
    assert_eq!(
        plan.report.left_unreferenced,
        vec![(50, "draughting_model".to_string())]
    );

    let mut problems = Vec::new();
    check_applied("handmade", &doc, &plan, &mut problems);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    let mut edit = Edit::new();
    plan.into_edit(&mut edit);
    let out = String::from_utf8(doc.apply(&edit).unwrap().bytes).unwrap();
    assert!(
        out.contains("#50=DRAUGHTING_MODEL('pmi',(#51),#9);"),
        "{out}"
    );
    assert!(out.contains("#33=PRESENTATION_STYLE_ASSIGNMENT((#36));"));
    assert!(out.contains("#39=STYLED_ITEM('',(#33),#10);"));
}

#[test]
fn a_kept_other_instance_referencing_a_seed_is_refused_by_name() {
    let doc = handmade(|t| {
        t.replace(
            "ENDSEC;\nEND-ISO",
            "#60=ID_ATTRIBUTE('h1',#20);\nENDSEC;\nEND-ISO",
        )
    });
    for policy in [
        PresentationPolicy::Refuse,
        PresentationPolicy::RemovePresentation,
    ] {
        let r = removal::plan(&doc, &handmade_seeds(&doc), policy).unwrap_err();
        let other: Vec<_> = r
            .blockers
            .iter()
            .filter(|b| b.family == Family::Other)
            .collect();
        assert_eq!(other.len(), 1, "{r}");
        assert_eq!(
            (other[0].id, other[0].entity.as_str()),
            (60, "id_attribute")
        );
        assert_eq!(other[0].references, vec![20]);
        assert!(
            r.to_string()
                .contains("#60 id_attribute (other) references removed #20")
        );
    }
}

#[test]
fn a_model_that_would_be_emptied_is_refused() {
    let doc = handmade(|t| {
        t.replace(
            "DRAUGHTING_MODEL('pmi',(#30,#51,#55),#9)",
            "DRAUGHTING_MODEL('pmi',(#30,#55),#9)",
        )
        .replace("#51=AXIS2_PLACEMENT_3D('',#52,$,$);\n", "")
    });
    let r = removal::plan(
        &doc,
        &handmade_seeds(&doc),
        PresentationPolicy::RemovePresentation,
    )
    .unwrap_err();
    assert!(r.blockers.is_empty());
    assert_eq!(
        r.emptied,
        vec![removal::Emptied::new(
            50,
            "draughting_model".into(),
            "representation.items".into(),
            1
        )]
    );
    assert!(
        r.to_string()
            .contains("#50 draughting_model would empty representation.items")
    );
}

#[test]
fn a_group_keeps_its_other_members() {
    // The annotation plane also holds a callout of PMI that stays: it is rewritten, not removed.
    let doc = handmade(|t| {
        t.replace(
            "#55=ANNOTATION_PLANE('',(#33),#56,(#30));",
            "#55=ANNOTATION_PLANE('',(#33),#56,(#30,#59));",
        )
        .replace(
            "ENDSEC;\nEND-ISO",
            "#59=DRAUGHTING_CALLOUT('',(#51));\nENDSEC;\nEND-ISO",
        )
    });
    let plan = removal::plan(
        &doc,
        &handmade_seeds(&doc),
        PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    assert!(!plan.removed.contains(&55) && !plan.removed.contains(&56));
    assert_eq!(
        plan.rewritten.keys().copied().collect::<Vec<_>>(),
        vec![50, 55]
    );
    let mut edit = Edit::new();
    plan.into_edit(&mut edit);
    let out = String::from_utf8(doc.apply(&edit).unwrap().bytes).unwrap();
    assert!(
        out.contains("#55=ANNOTATION_PLANE('',(#33),#56,(#59));"),
        "{out}"
    );
    assert!(
        out.contains("#50=DRAUGHTING_MODEL('pmi',(#51,#55),#9);"),
        "{out}"
    );
}

#[test]
fn seeds_that_are_missing_or_infrastructure_are_refused() {
    let doc = handmade(|t| t);
    let r = removal::plan(
        &doc,
        &BTreeSet::from([8, 20, 99]),
        PresentationPolicy::Refuse,
    )
    .unwrap_err();
    assert_eq!(r.missing, vec![99]);
    assert_eq!(
        r.infrastructure,
        vec![(8, "(length_unit named_unit si_unit)".to_string())]
    );
    for infrastructure in [1, 6, 7, 9, 10, 11] {
        let r = removal::plan(
            &doc,
            &BTreeSet::from([infrastructure]),
            PresentationPolicy::RemovePresentation,
        )
        .unwrap_err();
        assert_eq!(ids(&r.infrastructure), vec![infrastructure]);
    }
}

#[test]
fn plans_are_deterministic() {
    let doc = handmade(|t| t);
    let seeds = handmade_seeds(&doc);
    let a = removal::plan(&doc, &seeds, PresentationPolicy::RemovePresentation).unwrap();
    let b = removal::plan(
        &handmade(|t| t),
        &seeds,
        PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    assert_eq!(a, b);
    let bytes = |p: &RemovalPlan| {
        let mut edit = Edit::new();
        p.into_edit(&mut edit);
        doc.apply(&edit).unwrap().bytes
    };
    assert_eq!(bytes(&a), bytes(&b));
}

#[test]
fn a_foreign_symbol_representation_is_not_generic_pmi_presentation() {
    let doc = handmade(|t| {
        t.replace(
            "ENDSEC;\nEND-ISO",
            "#90=SYMBOL_REPRESENTATION('foreign symbol',(#30),#9);\nENDSEC;\nEND-ISO",
        )
    });
    assert_eq!(instance_family(&doc, 90), Family::Other);
    let plan = removal::plan(
        &doc,
        &handmade_seeds(&doc),
        PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let mut edit = Edit::new();
    plan.into_edit(&mut edit);
    let after = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
    // The foreign symbol holds its callout: neither the representation nor any of its
    // shared content may be removed or rewritten as generic PMI presentation.
    for id in [90, 30, 31, 32, 33, 34, 35, 36, 37] {
        assert_eq!(
            &doc.bytes()[doc.span(id).unwrap()],
            &after.bytes()[after.span(id).unwrap()],
            "foreign symbol content #{id}"
        );
        assert!(!plan.removed.contains(&id));
    }
}
