//! The named WHERE and UNIQUE rule checks (`express_rules.rs`): every rule's label is the
//! schema's, every rule has a passing and a failing hand-made instance set
//! (`tests/fixtures/ap242/rules/{valid,invalid}.stp`, the expected violations in
//! `invalid.json`), and every NIST AP242 file's violations are pinned by file and rule with a
//! verdict (`known_nist_rule_violations.json`; `HAECCEITY_RULES_ACTUAL=<dir>` writes the
//! computed list there). The surface condition and presentation relationship rules are also
//! checked on the committed AP242 files (violations pinned with a verdict) and on minimal edits
//! of a written document.
//!
//! NIST files: `HAECCEITY_NIST_PMI` names their `NIST-PMI-STEP-Files` directory (else the AP242
//! track's download); skipped when absent unless `HAECCEITY_NIST_PMI_REQUIRED=1`.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use haecceity::express;
use haecceity::express_rules::{GLOBAL_RULES, RULES, RuleViolation, check_all};
use haecceity::p21::{Attribute, Document, RawEntity};
use serde_json::{Value, json};

fn fixtures() -> PathBuf {
    common::fixtures().join("ap242/rules")
}

fn load(name: &str) -> Document {
    Document::parse(std::fs::read(fixtures().join(name)).unwrap()).unwrap()
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

#[test]
fn rule_labels_are_the_schemas() {
    let mut seen = BTreeSet::new();
    for r in RULES {
        assert!(seen.insert(r.rule), "{} listed twice", r.rule);
        assert_eq!(r.rule, format!("{}.{}", r.entity, r.label));
        assert!(
            express::is_declared(r.entity),
            "{} is not declared",
            r.entity
        );
        if r.label.starts_with("WR") || r.label.starts_with("UR") {
            assert!(
                express::rules(r.entity).any(|l| l == r.label),
                "{} has no rule {}",
                r.entity,
                r.label
            );
        } else {
            // An INVERSE attribute's cardinality: the attribute is the entity's own.
            assert_eq!(r.rule, "datum_reference_compartment.owner");
        }
    }
    // The rules the writer relies on, by the design's list.
    let want = |e: &str, labels: &[&str]| {
        for l in labels {
            assert!(
                seen.contains(format!("{e}.{l}").as_str()),
                "{e}.{l} missing"
            );
        }
    };
    let wr = |n: usize| (1..=n).map(|i| format!("WR{i}")).collect::<Vec<_>>();
    want(
        "thread",
        &wr(16).iter().map(String::as_str).collect::<Vec<_>>(),
    );
    want(
        "turned_knurl",
        &wr(12).iter().map(String::as_str).collect::<Vec<_>>(),
    );
    want("default_tolerance_table", &["WR1", "WR2"]);
    want(
        "default_tolerance_table_cell",
        &["WR1", "WR2", "WR3", "WR4", "WR5"],
    );
    want("datum", &["WR1", "WR2", "WR3", "WR4", "UR1"]);
    want("datum_feature", &["WR1", "WR2"]);
    want("datum_target", &["WR1", "WR2", "WR3", "WR4", "WR5", "UR1"]);
    want("placed_datum_target_feature", &["WR1", "WR2", "WR3"]);
    want("datum_system", &["WR1", "UR1"]);
    want("geometric_tolerance", &["WR1", "WR3", "WR5"]);
    want("geometric_tolerance_with_datum_reference", &["WR1"]);
    for form in [
        "flatness_tolerance",
        "straightness_tolerance",
        "roundness_tolerance",
        "cylindricity_tolerance",
    ] {
        want(form, &["WR1"]);
    }
    want("geometric_tolerance_relationship", &["WR1", "WR2", "WR3"]);
    want("plus_minus_tolerance", &["UR1"]);
    want(
        "item_identified_representation_usage",
        &["UR1", "UR2", "WR1"],
    );
    want(
        "surface_texture_representation",
        &["WR1", "WR2", "WR3", "WR4", "WR5"],
    );
    want("general_property_association", &["WR1", "WR2"]);
    want(
        "mechanical_design_and_draughting_relationship",
        &["WR1", "WR2", "WR3"],
    );
}

/// The valid instance sets violate nothing, and hold an instance of every rule's entity.
#[test]
fn valid_instances_pass_every_rule() {
    let doc = load("valid.stp");
    let v = check_all(&doc);
    assert!(
        v.is_empty(),
        "{}",
        v.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );
    for r in RULES {
        assert!(
            doc.ids().any(|id| {
                let e = doc.get(id).unwrap();
                let names: Vec<String> = match e {
                    haecceity::p21::RawEntity::Simple { name, .. } => vec![name.clone()],
                    haecceity::p21::RawEntity::Complex { parts, .. } => {
                        parts.iter().map(|p| p.name.clone()).collect()
                    }
                };
                names.iter().any(|n| express::is_a(n, r.entity))
            }),
            "no {} in valid.stp, so {} has no passing case",
            r.entity,
            r.rule
        );
    }
}

/// Each invalid instance set violates the rules `invalid.json` names, and nothing else; every
/// rule has a failing case.
#[test]
fn invalid_instances_fail_their_rules() {
    let doc = load("invalid.stp");
    let expected: Value =
        serde_json::from_str(&std::fs::read_to_string(fixtures().join("invalid.json")).unwrap())
            .unwrap();
    let want: BTreeSet<(u64, String)> = expected["violations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            (
                v["id"].as_u64().unwrap(),
                v["rule"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let got: BTreeSet<(u64, String)> = check_all(&doc)
        .iter()
        .map(|v| (v.id, v.rule.to_string()))
        .collect();
    let all = check_all(&doc);
    let missing: Vec<_> = want.difference(&got).collect();
    let extra: Vec<String> = all
        .iter()
        .filter(|v| !want.contains(&(v.id, v.rule.to_string())))
        .map(ToString::to_string)
        .collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "missing {missing:?}\nunexpected:\n{}",
        extra.join("\n")
    );
    for r in RULES {
        assert!(
            want.iter().any(|(_, rule)| rule == r.rule),
            "{} has no failing case in invalid.stp",
            r.rule
        );
    }
}

#[test]
fn check_all_is_deterministic() {
    let doc = load("invalid.stp");
    assert_eq!(check_all(&doc), check_all(&load("invalid.stp")));
}

fn groups(v: &[RuleViolation]) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for x in v {
        *out.entry(x.rule.to_string()).or_insert(0) += 1;
    }
    out
}

const VERDICTS: [&str; 1] = ["file-wrong"];

/// Every NIST AP242 file's violations, by rule, pinned with a verdict and reason. A check
/// error is fixed in the check, not pinned: every pinned group is the file's.
#[test]
fn nist_rule_violations_are_pinned() {
    let Some(dir) = nist_dir() else { return };
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".stp") && n.contains("_ap242"))
        .collect();
    names.sort();
    assert!(!names.is_empty());
    let results = common::parallel::map(&names, |n| {
        let doc = Document::parse(std::fs::read(dir.join(n)).unwrap()).unwrap();
        let v = check_all(&doc);
        let examples: BTreeMap<String, String> = v
            .iter()
            .rev()
            .map(|x| (x.rule.to_string(), x.to_string()))
            .collect();
        (n.clone(), groups(&v), examples)
    });
    let pinned: Value = serde_json::from_str(
        &std::fs::read_to_string(fixtures().join("known_nist_rule_violations.json")).unwrap(),
    )
    .unwrap();
    let mut problems = Vec::new();
    let mut now = serde_json::Map::new();
    for (name, groups, examples) in &results {
        let p = &pinned["files"][name.as_str()];
        let mut entry = serde_json::Map::new();
        for (rule, count) in groups {
            let g = &p[rule.as_str()];
            if g["count"].as_u64() != Some(*count) {
                problems.push(format!("{name}: {rule} x{count}, pinned {}", g["count"]));
            }
            if !g["verdict"].as_str().is_some_and(|v| VERDICTS.contains(&v))
                || !g["reason"].as_str().is_some_and(|r| !r.is_empty())
            {
                problems.push(format!("{name}: {rule} has no verdict or reason"));
            }
            let mut e = json!({
                "count": count,
                "example": examples[rule],
            });
            for k in ["verdict", "reason"] {
                if let Some(v) = g.get(k) {
                    e[k] = v.clone();
                }
            }
            entry.insert(rule.clone(), e);
        }
        for (rule, _) in p.as_object().into_iter().flatten() {
            if !groups.contains_key(rule) {
                problems.push(format!("{name}: {rule} is pinned but not found"));
            }
        }
        now.insert(name.clone(), Value::Object(entry));
    }
    for name in pinned["files"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, _)| k)
    {
        if !now.contains_key(name) {
            problems.push(format!("{name} is pinned but not among the files"));
        }
    }
    if let Some(d) = std::env::var_os("HAECCEITY_RULES_ACTUAL") {
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            Path::new(&d).join("known_nist_rule_violations.json"),
            serde_json::to_string_pretty(&json!({"files": now})).unwrap() + "\n",
        )
        .unwrap();
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

// ---------------------------------------------------------------------------------------------
// surface_texture_representation, general_property_association and
// mechanical_design_and_draughting_relationship on written and committed files
// ---------------------------------------------------------------------------------------------

/// The entities of the surface condition and presentation relationship rules.
const SURFACE_AND_DRAUGHTING: [&str; 3] = [
    "surface_texture_representation",
    "general_property_association",
    "mechanical_design_and_draughting_relationship",
];

fn text_of(path: &Path) -> String {
    use std::io::Read;
    let raw = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    if path.extension().is_some_and(|e| e == "gz") {
        let mut out = String::new();
        flate2::read::GzDecoder::new(raw.as_slice())
            .read_to_string(&mut out)
            .unwrap();
        out
    } else {
        String::from_utf8(raw).unwrap()
    }
}

/// Every committed AP242 file the writer, reader and removal suites use: the writer's output
/// (`write/`), specify-core's (`specify/`), the committed NIST models (`nist/`) and the
/// removal fixture.
fn committed_files() -> Vec<PathBuf> {
    let base = common::fixtures().join("ap242");
    let mut out = vec![base.join("removal/handmade.stp")];
    for dir in ["write", "specify", "nist"] {
        let mut names: Vec<PathBuf> = std::fs::read_dir(base.join(dir))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                let p = p.to_string_lossy();
                p.ends_with(".step.gz") || p.ends_with(".stp.gz")
            })
            .collect();
        names.sort();
        out.extend(names);
    }
    out
}

/// The violations of the rules declared by `entities`.
fn violations_of(doc: &Document, entities: &[&str]) -> Vec<String> {
    check_all(doc)
        .iter()
        .filter(|v| {
            v.rule
                .split('.')
                .next()
                .is_some_and(|e| entities.contains(&e))
        })
        .map(ToString::to_string)
        .collect()
}

/// The committed files' violations of the surface condition and presentation relationship
/// rules, by file, instance and rule. Verdict file-wrong, all of them: specify-core's
/// 'pmi-assist' user defined attributes (threads, knurls) associate a property definition
/// named for its content ('external thread', 'knurl') with `general_property('', 'user defined
/// attribute')`, and general_property_association WR2 requires the derived definition's name
/// to equal the general property's. NIST's files name the two alike ('Density', 'semantic
/// text'), as haecceity's writer does (its notes and surface textures pass, above and below).
const KNOWN_SPECIFY_CORE: [(&str, u64, &str); 8] = [
    (
        "assembly_plate_pin.step.gz",
        1553,
        "general_property_association.WR2",
    ),
    (
        "assembly_plate_pin.step.gz",
        1649,
        "general_property_association.WR2",
    ),
    (
        "bolt_thread_knurl.step.gz",
        470,
        "general_property_association.WR2",
    ),
    (
        "bolt_thread_knurl.step.gz",
        497,
        "general_property_association.WR2",
    ),
    (
        "string_post_tapped.step.gz",
        3185,
        "general_property_association.WR2",
    ),
    (
        "thumbwheel_thread_knurl.step.gz",
        717,
        "general_property_association.WR2",
    ),
    (
        "thumbwheel_thread_knurl.step.gz",
        744,
        "general_property_association.WR2",
    ),
    (
        "thumbwheel_thread_knurl.step.gz",
        775,
        "general_property_association.WR2",
    ),
];

/// The committed files (writer output, specify-core output, NIST models, the removal
/// fixture) violate `surface_texture_representation` WR1–WR5, `general_property_association`
/// WR1–WR2 and `mechanical_design_and_draughting_relationship` WR1–WR3 only as pinned
/// ([`KNOWN_SPECIFY_CORE`]): the writer's output not at all (`write/thumbwheel_notes_add` is
/// an add onto specify-core's thumbwheel and keeps its instances). Together they hold instances of
/// all three.
#[test]
fn committed_files_meet_the_surface_and_draughting_rules() {
    let files = committed_files();
    let results = common::parallel::map(&files, |p| {
        let text = text_of(p);
        let held: Vec<bool> = SURFACE_AND_DRAUGHTING
            .iter()
            .map(|e| text.contains(&format!("{}(", e.to_ascii_uppercase())))
            .collect();
        let doc = Document::parse(text.into_bytes()).unwrap();
        let v: BTreeSet<(u64, String)> = check_all(&doc)
            .into_iter()
            .filter(|v| {
                v.rule
                    .split('.')
                    .next()
                    .is_some_and(|e| SURFACE_AND_DRAUGHTING.contains(&e))
            })
            .map(|v| (v.id, v.rule.to_string()))
            .collect();
        (v, held)
    });
    let mut held = [false; 3];
    let mut got = BTreeSet::new();
    for (p, (v, h)) in files.iter().zip(&results) {
        for (i, x) in h.iter().enumerate() {
            held[i] |= x;
        }
        let dir = p.parent().unwrap().file_name().unwrap().to_string_lossy();
        let name = p.file_name().unwrap().to_string_lossy();
        for (id, rule) in v {
            got.insert((format!("{dir}/{name}"), *id, rule.clone()));
        }
    }
    let want: BTreeSet<(String, u64, String)> = KNOWN_SPECIFY_CORE
        .iter()
        .flat_map(|&(f, id, r)| {
            let kept = (f == "thumbwheel_thread_knurl.step.gz").then(|| {
                (
                    "write/thumbwheel_notes_add.step.gz".to_string(),
                    id,
                    r.to_string(),
                )
            });
            [Some((format!("specify/{f}"), id, r.to_string())), kept]
        })
        .flatten()
        .collect();
    assert_eq!(got, want);
    assert_eq!(held, [true; 3], "{SURFACE_AND_DRAUGHTING:?}");
}

/// A written document: the writer's every-kind output (notes and surface textures included).
fn every_kind() -> String {
    text_of(&common::fixtures().join("ap242/write/every_kind.step.gz"))
}

/// A line's instance id and what follows its `=` (`#10 = X(…)` and `#10=X(…)` alike).
fn head(line: &str) -> Option<(u64, &str)> {
    let (id, rest) = line.strip_prefix('#')?.split_once('=')?;
    Some((id.trim().parse().ok()?, rest.trim_start()))
}

/// The line of instance `#id`.
fn line(text: &str, id: u64) -> &str {
    text.lines()
        .find(|l| head(l).is_some_and(|(i, _)| i == id))
        .unwrap_or_else(|| panic!("no #{id}"))
}

/// The instances a line references, in order.
fn refs_in(line: &str) -> Vec<u64> {
    let body = &line[line.find('=').unwrap()..];
    body.split('#')
        .skip(1)
        .map(|s| {
            s.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .unwrap()
        })
        .collect()
}

/// The ids of the simple instances of `entity` (upper case), in order.
fn instances(text: &str, entity: &str) -> Vec<u64> {
    let tag = format!("{entity}(");
    text.lines()
        .filter_map(head)
        .filter(|(_, rest)| rest.starts_with(&tag))
        .map(|(id, _)| id)
        .collect()
}

/// `text` with `from` replaced by `to` in instance `#id` (which holds it once).
fn edit(text: &str, id: u64, from: &str, to: &str) -> String {
    let old = line(text, id);
    assert_eq!(old.matches(from).count(), 1, "{from} in {old}");
    text.replacen(old, &old.replacen(from, to, 1), 1)
}

/// `text` with `lines` appended to its data section.
fn append(text: &str, lines: &[String]) -> String {
    let at = text.rfind("ENDSEC;").unwrap();
    format!("{}{}\n{}", &text[..at], lines.join("\n"), &text[at..])
}

/// The (instance, rule) violations of `after` that `before` does not have.
fn new_violations(before: &str, after: &str) -> BTreeSet<(u64, String)> {
    let set = |t: &str| -> BTreeSet<(u64, String)> {
        check_all(&Document::parse(t.as_bytes().to_vec()).unwrap())
            .into_iter()
            .map(|v| (v.id, v.rule.to_string()))
            .collect()
    };
    let b = set(before);
    set(after).difference(&b).cloned().collect()
}

fn pairs(v: &[(u64, &str)]) -> BTreeSet<(u64, String)> {
    v.iter().map(|&(i, r)| (i, r.to_string())).collect()
}

/// The written surface texture parameters pass `surface_texture_representation` WR1–WR5 and
/// the written associations (notes', parameters') `general_property_association` WR1–WR2;
/// each rule fails on a minimal edit of the written document, with exactly the violations
/// named.
#[test]
fn surface_texture_rules_on_a_written_document() {
    let text = every_kind();
    let doc = Document::parse(text.clone().into_bytes()).unwrap();
    let pass = violations_of(&doc, &SURFACE_AND_DRAUGHTING[..2]);
    assert!(pass.is_empty(), "{}", pass.join("\n"));
    let strs = instances(&text, "SURFACE_TEXTURE_REPRESENTATION");
    assert_eq!(strs.len(), 3);
    let (s, s2) = (strs[0], strs[1]);
    // Its items (the 'measuring method', the value), then its context.
    let [d, m, ctx] = refs_in(line(&text, s))[..] else {
        panic!("{}", line(&text, s));
    };
    assert!(line(&text, d).contains("'measuring method'"));
    let pdr = instances(&text, "PROPERTY_DEFINITION_REPRESENTATION")
        .into_iter()
        .find(|&r| refs_in(line(&text, r))[1] == s)
        .unwrap();
    let pd = refs_in(line(&text, pdr))[0];
    let gpa = instances(&text, "GENERAL_PROPERTY_ASSOCIATION")
        .into_iter()
        .find(|&g| refs_in(line(&text, g))[1] == pd)
        .unwrap();
    let gp = refs_in(line(&text, gpa))[0];
    let next = doc.max_id() + 1;
    let point = instances(&text, "CARTESIAN_POINT")[0];

    // WR1: a point among the items.
    let t = edit(
        &text,
        s,
        &format!("(#{d},#{m})"),
        &format!("(#{d},#{m},#{point})"),
    );
    assert_eq!(
        new_violations(&text, &t),
        pairs(&[(s, "surface_texture_representation.WR1")])
    );
    // WR2: the descriptive item not named 'measuring method'.
    let t = edit(&text, d, "'measuring method'", "'measuring mode'");
    assert_eq!(
        new_violations(&text, &t),
        pairs(&[(s, "surface_texture_representation.WR2")])
    );
    // WR3: no measure.
    let t = edit(&text, s, &format!("(#{d},#{m})"), &format!("(#{d})"));
    assert_eq!(
        new_violations(&text, &t),
        pairs(&[(s, "surface_texture_representation.WR3")])
    );
    // WR4: related to a representation not named 'measuring direction' (the next parameter's,
    // which as a rep_2 violates it too); related to one so named, it passes.
    let t = append(
        &text,
        &[format!(
            "#{next}=REPRESENTATION_RELATIONSHIP('','',#{s},#{s2});"
        )],
    );
    assert_eq!(
        new_violations(&text, &t),
        pairs(&[
            (s, "surface_texture_representation.WR4"),
            (s2, "surface_texture_representation.WR4"),
        ])
    );
    let t = append(
        &text,
        &[
            format!("#{next}=REPRESENTATION('measuring direction',(#{point}),#{ctx});"),
            format!(
                "#{}=REPRESENTATION_RELATIONSHIP('','',#{s},#{next});",
                next + 1
            ),
        ],
    );
    assert!(new_violations(&text, &t).is_empty());
    // WR5: a second property_definition_representation of it.
    let t = append(
        &text,
        &[format!(
            "#{next}=PROPERTY_DEFINITION_REPRESENTATION(#{pd},#{s});"
        )],
    );
    assert_eq!(
        new_violations(&text, &t),
        pairs(&[(s, "surface_texture_representation.WR5")])
    );
    // WR5 also when its general property is not 'surface_condition', which breaks the
    // association's WR2 too (the names then differ).
    let t = edit(&text, gp, "'surface_condition'", "'roughness'");
    assert_eq!(
        new_violations(&text, &t),
        pairs(&[
            (s, "surface_texture_representation.WR5"),
            (gpa, "general_property_association.WR2"),
        ])
    );
    // The parameter named as the mapping names it (docs/step-ap242.md question 3, open):
    // 'surface texture parameter' breaks the association's WR2.
    let t = edit(
        &text,
        pd,
        "'surface_condition'",
        "'surface texture parameter'",
    );
    assert_eq!(
        new_violations(&text, &t),
        pairs(&[(gpa, "general_property_association.WR2")])
    );
    // general_property_association WR1: a second association of a note's definition.
    let note = instances(&text, "GENERAL_PROPERTY_ASSOCIATION")
        .into_iter()
        .find(|&g| line(&text, refs_in(line(&text, g))[0]).contains("'semantic text'"))
        .unwrap();
    let [base, derived] = refs_in(line(&text, note))[..] else {
        panic!("{}", line(&text, note));
    };
    let t = append(
        &text,
        &[format!(
            "#{next}=GENERAL_PROPERTY_ASSOCIATION('',$,#{base},#{derived});"
        )],
    );
    assert_eq!(
        new_violations(&text, &t),
        pairs(&[
            (note, "general_property_association.WR1"),
            (next, "general_property_association.WR1"),
        ])
    );
    // general_property_association WR2: the note's definition renamed.
    let t = edit(&text, derived, "'semantic text'", "'note'");
    assert_eq!(
        new_violations(&text, &t),
        pairs(&[(note, "general_property_association.WR2")])
    );
}

/// `mechanical_design_and_draughting_relationship` WR1–WR3 on relationships added to a
/// written document: presentations related from the part's shape, or from their own kind,
/// pass; each related from the wrong kind of representation fails its rule, and only it.
#[test]
fn draughting_relationship_rules_on_a_written_document() {
    let text = every_kind();
    let doc = Document::parse(text.clone().into_bytes()).unwrap();
    let dm = instances(&text, "DRAUGHTING_MODEL")[0];
    let shape = instances(&text, "ADVANCED_BREP_SHAPE_REPRESENTATION")
        .into_iter()
        .chain(instances(&text, "SHAPE_REPRESENTATION"))
        .next()
        .unwrap();
    let ctx = *refs_in(line(&text, dm)).last().unwrap();
    let axis = instances(&text, "AXIS2_PLACEMENT_3D")[0];
    let (geometric, shaded) = (doc.max_id() + 1, doc.max_id() + 2);
    let r = doc.max_id() + 3;
    let with = |rel: &[(u64, u64)]| {
        let mut lines = vec![
            format!(
                "#{geometric}=MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION('',(#{axis}),#{ctx});"
            ),
            format!(
                "#{shaded}=MECHANICAL_DESIGN_SHADED_PRESENTATION_REPRESENTATION('',(#{axis}),#{ctx});"
            ),
        ];
        for (k, (rep_1, rep_2)) in (r..).zip(rel) {
            lines.push(format!(
                "#{k}=MECHANICAL_DESIGN_AND_DRAUGHTING_RELATIONSHIP('','',#{rep_1},#{rep_2});"
            ));
        }
        new_violations(&text, &append(&text, &lines))
    };
    assert!(
        with(&[
            (shape, dm),
            (shape, geometric),
            (shape, shaded),
            (geometric, geometric),
        ])
        .is_empty()
    );
    assert_eq!(
        with(&[(geometric, dm)]),
        pairs(&[(r, "mechanical_design_and_draughting_relationship.WR1")])
    );
    assert_eq!(
        with(&[(dm, geometric)]),
        pairs(&[(r, "mechanical_design_and_draughting_relationship.WR2")])
    );
    assert_eq!(
        with(&[(dm, shaded)]),
        pairs(&[(r, "mechanical_design_and_draughting_relationship.WR3")])
    );
}

// ---------------------------------------------------------------------------------------------
// Global rules
// ---------------------------------------------------------------------------------------------

const SURFACE_CONDITION_RULE: &str = "restrict_representation_for_surface_condition.WR1";

/// The global rules: each named once, `name.label`, FOR declared entities. The table holds no
/// global rule, so the names are the schema text's as the module quotes it.
#[test]
fn global_rule_labels() {
    let mut seen = BTreeSet::new();
    for r in GLOBAL_RULES {
        assert!(seen.insert(r.rule), "{} listed twice", r.rule);
        assert_eq!(r.rule, format!("{}.{}", r.name, r.label));
        for e in r.entities {
            assert!(express::is_declared(e), "{e} is not declared");
        }
    }
    assert!(seen.contains(SURFACE_CONDITION_RULE));
}

/// `restrict_representation_for_surface_condition` on a written document (its surface
/// textures pass) and on minimal edits of it: a property definition named for any of the six
/// surface conditions with a representation named otherwise fails, reported on the
/// `property_definition_representation`; named alike they pass. Any other name (case
/// included), an unset name and a definition that is no `property_definition` are UNKNOWN:
/// they pass. Subtypes count on both sides (a `product_definition_shape` represented through a
/// `shape_definition_representation`).
#[test]
fn surface_condition_rule_on_a_written_document() {
    let text = every_kind();
    let doc = Document::parse(text.clone().into_bytes()).unwrap();
    let pass: Vec<String> = check_all(&doc)
        .iter()
        .filter(|v| v.rule == SURFACE_CONDITION_RULE)
        .map(ToString::to_string)
        .collect();
    assert!(pass.is_empty(), "{}", pass.join("\n"));
    let pd = instances(&text, "PROPERTY_DEFINITION")
        .into_iter()
        .find(|&p| line(&text, p).contains("'surface texture'"))
        .unwrap();
    let pdr = instances(&text, "PROPERTY_DEFINITION_REPRESENTATION")
        .into_iter()
        .find(|&r| refs_in(line(&text, r))[0] == pd)
        .unwrap();
    let rep = refs_in(line(&text, pdr))[1];
    assert!(line(&text, rep).contains("'surface texture'"));
    let fails = pairs(&[(pdr, SURFACE_CONDITION_RULE)]);
    for name in [
        "visual appearance",
        "tactile appearance",
        "contact ratio",
        "hardness",
        "treatment result",
    ] {
        let renamed = edit(&text, pd, "'surface texture'", &format!("'{name}'"));
        assert_eq!(new_violations(&text, &renamed), fails, "{name}");
        let both = edit(&renamed, rep, "'surface texture'", &format!("'{name}'"));
        assert!(new_violations(&text, &both).is_empty(), "{name}");
    }
    let t = edit(&text, rep, "'surface texture'", "'surface finish'");
    assert_eq!(new_violations(&text, &t), fails);
    // UNKNOWN: another name, a name differing in case, an unset name on either side.
    for (id, to) in [
        (pd, "'roughness'"),
        (pd, "'Surface Texture'"),
        (pd, "$"),
        (rep, "$"),
    ] {
        let t = edit(&text, id, "'surface texture'", to);
        assert!(new_violations(&text, &t).is_empty(), "#{id} named {to}");
    }
    // A shape aspect named 'surface texture' is not a property_definition: UNKNOWN. A
    // product_definition_shape so named, represented through a
    // shape_definition_representation, is one.
    let next = doc.max_id() + 1;
    let point = instances(&text, "CARTESIAN_POINT")[0];
    let ctx = *refs_in(line(&text, rep)).last().unwrap();
    let owner = refs_in(line(&text, pd))[0];
    let product = instances(&text, "PRODUCT_DEFINITION")[0];
    let other = format!("#{next}=REPRESENTATION('other',(#{point}),#{ctx});");
    let t = append(
        &text,
        &[
            other.clone(),
            format!(
                "#{}=SHAPE_ASPECT('surface texture','',#{owner},.F.);",
                next + 1
            ),
            format!(
                "#{}=PROPERTY_DEFINITION_REPRESENTATION(#{},#{next});",
                next + 2,
                next + 1
            ),
        ],
    );
    assert!(new_violations(&text, &t).is_empty());
    let t = append(
        &text,
        &[
            other,
            format!(
                "#{}=PRODUCT_DEFINITION_SHAPE('surface texture','',#{product});",
                next + 1
            ),
            format!(
                "#{}=SHAPE_DEFINITION_REPRESENTATION(#{},#{next});",
                next + 2,
                next + 1
            ),
        ],
    );
    assert_eq!(
        new_violations(&text, &t),
        pairs(&[(next + 2, SURFACE_CONDITION_RULE)])
    );
}

/// The committed files (writer output, specify-core output, NIST models, the removal fixture)
/// meet every global rule, and some hold surface textures for it to apply to.
#[test]
fn committed_files_meet_the_global_rules() {
    let files = committed_files();
    let results = common::parallel::map(&files, |p| {
        let text = text_of(p);
        let held = text.contains("PROPERTY_DEFINITION('surface texture'");
        let doc = Document::parse(text.into_bytes()).unwrap();
        let v: Vec<String> = check_all(&doc)
            .into_iter()
            .filter(|v| GLOBAL_RULES.iter().any(|g| g.rule == v.rule))
            .map(|v| format!("{}: {v}", p.display()))
            .collect();
        (v, held)
    });
    let v: Vec<&String> = results.iter().flat_map(|(v, _)| v).collect();
    assert!(v.is_empty(), "{v:#?}");
    assert!(results.iter().any(|&(_, h)| h));
}

#[test]
fn tolerance_relationship_wr3_checks_cycles_across_names() {
    let text = String::from_utf8({
        use std::io::Read;
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(
            std::fs::File::open(common::fixtures().join("ap242/write/pmi_gaps.step.gz")).unwrap(),
        )
        .read_to_end(&mut bytes)
        .unwrap();
        bytes
    })
    .unwrap();
    let relations = instances(&text, "GEOMETRIC_TOLERANCE_RELATIONSHIP");
    let [a, b] = refs_in(line(&text, relations[0]))[..] else {
        panic!("two tolerance references");
    };
    let id = 999999;
    let cycled = append(
        &text,
        &[format!(
            "#{id}=GEOMETRIC_TOLERANCE_RELATIONSHIP('precedence',$,#{b},#{a});"
        )],
    );
    let doc = Document::parse(cycled.into_bytes()).unwrap();
    assert!(
        check_all(&doc)
            .iter()
            .any(|v| v.id == id && v.rule == "geometric_tolerance_relationship.WR3")
    );
    let valid = Document::parse(text.into_bytes()).unwrap();
    assert!(
        !check_all(&valid)
            .iter()
            .any(|v| v.rule == "geometric_tolerance_relationship.WR3")
    );
}

#[test]
fn default_saved_view_rules_check_both_names_descriptions_and_uniqueness() {
    let base = every_kind();
    let base_doc = Document::parse(base.as_bytes().to_vec()).unwrap();
    let n = base_doc.max_id() + 1;
    let axis = instances(&base, "AXIS2_PLACEMENT_3D")[0];
    let shape = instances(&base, "PRODUCT_DEFINITION_SHAPE")[0];
    let dm = instances(&base, "DRAUGHTING_MODEL")[0];
    let context = *refs_in(line(&base, dm)).last().unwrap();
    let text = append(
        &base,
        &[
            format!("#{n}=CARTESIAN_POINT('',(-10.,-10.));"),
            format!("#{}=AXIS2_PLACEMENT_2D('',#{n},$);", n + 1),
            format!("#{}=PLANAR_BOX('',20.,20.,#{});", n + 2, n + 1),
            format!("#{}=CARTESIAN_POINT('',(0.,0.,0.));", n + 3),
            format!(
                "#{}=VIEW_VOLUME(.PARALLEL.,#{},40.,0.,.F.,80.,.F.,.T.,#{});",
                n + 4,
                n + 3,
                n + 2
            ),
            format!("#{}=CAMERA_MODEL_D3('Notes',#{axis},#{});", n + 5, n + 4),
            format!(
                "#{}=DRAUGHTING_MODEL('Notes',(#{}),#{context});",
                n + 6,
                n + 5
            ),
            format!(
                "#{}=DEFAULT_MODEL_GEOMETRIC_VIEW('Notes',$,#{},#{},'Notes',$,#{shape},*);",
                n + 7,
                n + 5,
                n + 6
            ),
        ],
    );
    let doc = Document::parse(text.as_bytes().to_vec()).unwrap();
    assert!(new_violations(&base, &text).is_empty());
    let view = doc
        .ids()
        .find(|&id| common::pmi_usage::is_a(&doc, id, "default_model_geometric_view"))
        .unwrap();
    let RawEntity::Simple {
        name, attributes, ..
    } = doc.get(view).unwrap()
    else {
        panic!("internal mapping")
    };
    let slots = express::TARGET.explicit_attributes(name).unwrap();
    for (entity, field, value, want) in [
        (
            "shape_aspect",
            "name",
            Attribute::String("Different".into()),
            "default_model_geometric_view.WR1",
        ),
        (
            "characterized_object",
            "description",
            Attribute::String("".into()),
            "default_model_geometric_view.WR2",
        ),
        (
            "shape_aspect",
            "description",
            Attribute::String("".into()),
            "default_model_geometric_view.WR3",
        ),
    ] {
        let mut values = attributes.clone();
        let i = slots
            .iter()
            .position(|s| s.entity == entity && s.name == field)
            .unwrap();
        values[i] = value;
        let mut edit = haecceity::p21::Edit::new();
        edit.replace(view, haecceity::p21::simple(name, values));
        let changed = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
        let violations: Vec<_> = check_all(&changed)
            .into_iter()
            .filter(|v| v.id == view)
            .collect();
        assert_eq!(
            violations.iter().map(|v| v.rule).collect::<Vec<_>>(),
            vec![want]
        );
    }
    let mut edit = haecceity::p21::Edit::new();
    edit.add(haecceity::p21::Record::new(haecceity::p21::simple(
        name,
        attributes.clone(),
    )));
    let changed = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
    assert!(
        check_all(&changed)
            .iter()
            .any(|v| v.rule == "default_model_geometric_view.UR1")
    );
}
