//! The AP242 schema tables and the validator: the tables against their sources, hand-written
//! instances from the CAx-IF PMI practice, known-bad instances, every NIST AP242 test file
//! (own edition and upgraded to the target), and every corpus file upgraded to the target.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use haecceity::express::{
    self, AP242_ED1, AP242_ED4, Edition, Family, Kind, TARGET, TARGET_SCHEMA, Ty, TypeDef,
    Violation,
};
use serde_json::{Value, json};
use step_io::parser::{Graph, parse};

const HEADER: &str = "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\n\
    FILE_NAME('t','2026-10-08T00:00:00',(''),(''),'','','');\n\
    FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 7 1 4 }'));\n\
    ENDSEC;\nDATA;\n";

/// A product with a shape, a millimetre context and one plane in a shape representation: what
/// the hand-written PMI instances reference.
const CONTEXT: &str = "\
#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('p','p','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('design','',#4,#5);
#7=PRODUCT_DEFINITION_SHAPE('','',#6);
#8=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));
#9=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#8))REPRESENTATION_CONTEXT('',''));
#10=CARTESIAN_POINT('',(0.,0.,0.));
#11=DIRECTION('',(0.,0.,1.));
#12=DIRECTION('',(1.,0.,0.));
#13=AXIS2_PLACEMENT_3D('',#10,#11,#12);
#14=PLANE('',#13);
#15=SHAPE_REPRESENTATION('',(#13,#14),#9);
#16=SHAPE_DEFINITION_REPRESENTATION(#7,#15);
#17=PLANE('',#13);
";

fn graph(data: &str) -> Graph {
    let text = format!("{HEADER}{CONTEXT}{data}\nENDSEC;\nEND-ISO-10303-21;\n");
    parse(text.as_bytes()).unwrap()
}

fn fixtures() -> PathBuf {
    common::fixtures().join("ap242/express")
}

#[test]
fn target_is_edition_4_and_editions_are_identified_by_their_object_identifier() {
    assert_eq!(TARGET.file_schema, TARGET_SCHEMA);
    assert!(std::ptr::eq(TARGET, &AP242_ED4));
    for (schema, edition) in [
        (
            "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }",
            Some(&AP242_ED1),
        ),
        (
            "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF {1 0 10303 442 1 1 4 }",
            Some(&AP242_ED1),
        ),
        (
            "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF  {1 0 10303 442 7 1 4}",
            Some(&AP242_ED4),
        ),
        (
            "ap242_managed_model_based_3d_engineering_mim_lf { 1 0 10303 442 7 1 4 }",
            Some(&AP242_ED4),
        ),
        // Editions 2 and 3: no long form found, no table.
        (
            "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 3 1 4 }",
            None,
        ),
        (
            "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 4 1 4 }",
            None,
        ),
        ("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF", None),
        ("AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }", None),
        ("CONFIG_CONTROL_DESIGN", None),
    ] {
        assert_eq!(
            express::edition(schema).map(|e| e.file_schema),
            edition.map(|e| e.file_schema),
            "{schema}"
        );
    }
}

#[test]
fn tables_are_complete_sorted_and_closed() {
    for (ed, types, entities) in [(&AP242_ED1, 370, 1726), (&AP242_ED4, 528, 2407)] {
        assert_eq!(
            (ed.types.len(), ed.entities.len()),
            (types, entities),
            "{}",
            ed.file_schema
        );
        assert!(ed.types.windows(2).all(|w| w[0].name < w[1].name));
        assert!(ed.entities.windows(2).all(|w| w[0].name < w[1].name));
        let known = |n: &str| ed.entity(n).is_some() || ed.type_decl(n).is_some();
        fn names(ty: Ty, out: &mut Vec<&'static str>) {
            match ty {
                Ty::Named(n) => out.push(n),
                Ty::Aggregate(a) => names(a.of, out),
                _ => {}
            }
        }
        let mut used = Vec::new();
        for t in ed.types {
            match &t.def {
                TypeDef::Select(m) => used.extend(m.iter().copied()),
                TypeDef::Defined(ty) => names(*ty, &mut used),
                TypeDef::Enumeration(_) => {}
            }
        }
        for e in ed.entities {
            used.extend(e.supertypes.iter().copied());
            e.attributes.iter().for_each(|a| names(a.ty, &mut used));
            for r in e.redeclared {
                names(r.ty, &mut used);
                let owner = ed.entity(r.entity).unwrap();
                assert!(
                    owner.attributes.iter().any(|a| a.name == r.attribute),
                    "{}: {r:?}",
                    e.name
                );
                assert!(ed.is_a(e.name, r.entity), "{} redeclares {r:?}", e.name);
            }
        }
        for n in used {
            assert!(known(n), "{}: {n} is not declared", ed.file_schema);
        }
    }
}

/// The table is what the generator makes from the recorded sources. Set
/// `HAECCEITY_AP242_EXPRESS` to a directory holding `242_n8324_mim_lf.exp` and
/// `242_mim_lf.exp` (the URLs in the table) to regenerate and compare byte for byte.
#[test]
fn table_regenerates_from_its_sources() {
    let table =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/express_table.rs"))
            .unwrap();
    for ed in [&AP242_ED1, &AP242_ED4] {
        assert!(table.contains(&format!("sha256 {}", ed.source_sha256)));
        assert!(ed.source_url.contains("/stepcode/stepcode/"));
    }
    let Some(dir) = std::env::var_os("HAECCEITY_AP242_EXPRESS").map(PathBuf::from) else {
        eprintln!("HAECCEITY_AP242_EXPRESS not set; the table is not regenerated");
        return;
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = std::process::Command::new("python3")
        .arg("-I")
        .arg(root.join("tools/express_table.py"))
        .arg(dir.join("242_n8324_mim_lf.exp"))
        .arg(dir.join("242_mim_lf.exp"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stdout == table.as_bytes(),
        "the generated table differs from express_table.rs"
    );
}

#[test]
fn attributes_come_in_part_21_order_with_redeclarations() {
    let names = |e: &str| -> Vec<String> {
        express::explicit_attributes(e)
            .unwrap()
            .iter()
            .map(|s| {
                format!(
                    "{}.{}{}",
                    s.entity,
                    s.name,
                    if s.derived { "*" } else { "" }
                )
            })
            .collect()
    };
    assert_eq!(
        names("DATUM"),
        [
            "shape_aspect.name",
            "shape_aspect.description",
            "shape_aspect.of_shape",
            "shape_aspect.product_definitional",
            "datum.identification"
        ]
    );
    assert_eq!(
        names("position_tolerance"),
        [
            "geometric_tolerance.name",
            "geometric_tolerance.description",
            "geometric_tolerance.magnitude",
            "geometric_tolerance.toleranced_shape_aspect"
        ]
    );
    // si_unit redeclares named_unit.dimensions as derived: written `*`.
    assert_eq!(
        names("si_unit"),
        ["named_unit.dimensions*", "si_unit.prefix", "si_unit.name"]
    );
    // geometric_item_specific_usage narrows three inherited attributes.
    let gisu = express::explicit_attributes("geometric_item_specific_usage").unwrap();
    assert_eq!(gisu.len(), 5);
    assert!(
        gisu[2]
            .redeclared
            .iter()
            .any(|t| matches!(t, Ty::Named("geometric_item_specific_usage_select")))
    );
    assert!(express::explicit_attributes("no_such_entity").is_none());
}

#[test]
fn subtypes_and_rule_labels() {
    assert!(express::is_a(
        "PERPENDICULARITY_TOLERANCE",
        "geometric_tolerance"
    ));
    assert!(express::is_a(
        "perpendicularity_tolerance",
        "geometric_tolerance_with_datum_reference"
    ));
    assert!(!express::is_a(
        "position_tolerance",
        "geometric_tolerance_with_datum_reference"
    ));
    assert!(express::is_a("datum", "datum"));
    assert!(!express::is_a("not_an_entity", "datum"));
    assert!(express::is_declared("DATUM_SYSTEM"));
    assert!(!express::is_declared("length_measure"));
    assert_eq!(
        express::rules("datum").collect::<Vec<_>>(),
        ["UR1", "WR1", "WR2", "WR3", "WR4"]
    );
    assert_eq!(express::rules("thread").count(), 16);
    assert_eq!(express::rules("turned_knurl").count(), 12);
    assert_eq!(
        express::rules("geometric_tolerance_relationship").collect::<Vec<_>>(),
        ["WR1", "WR2", "WR3"]
    );
}

/// The PMI practice's instances (§6.4 'multiple elements', §6.5.1 datum and datum feature,
/// §6.9.1 position tolerance, §5.2.5 limits and fits), referencing a consistent product, shape
/// and context: the practice's examples elide or reuse their referenced ids.
#[test]
fn practice_instances_validate_clean() {
    let g = graph(
        "\
#20=SHAPE_ASPECT('',$,#7,.T.);
#21=GEOMETRIC_ITEM_SPECIFIC_USAGE('','single feature',#20,#15,#14);
#22=SHAPE_ASPECT('',$,#7,.T.);
#23=GEOMETRIC_ITEM_SPECIFIC_USAGE('','single feature',#22,#15,#17);
#24=COMPOSITE_GROUP_SHAPE_ASPECT('','multiple elements',#7,.T.);
#25=SHAPE_ASPECT_RELATIONSHIP('',$,#24,#20);
#26=SHAPE_ASPECT_RELATIONSHIP('',$,#24,#22);
#30=DATUM('',$,#7,.F.,'B');
#31=DATUM_FEATURE('',$,#7,.T.);
#32=SHAPE_ASPECT_RELATIONSHIP('',$,#31,#30);
#33=GEOMETRIC_ITEM_SPECIFIC_USAGE('','datum feature',#31,#15,#14);
#40=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.1),#8);
#41=POSITION_TOLERANCE('',$,#40,#20);
#50=DIMENSIONAL_SIZE(#20,'diameter');
#51=LIMITS_AND_FITS('G','','6','');
#52=PLUS_MINUS_TOLERANCE(#51,#50);
#53=(GEOMETRIC_TOLERANCE('',$,#40,#22)GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE((#60))POSITION_TOLERANCE());
#60=DATUM_SYSTEM('',$,#7,.F.,(#61));
#61=DATUM_REFERENCE_COMPARTMENT('',$,#7,.F.,#30,$);
",
    );
    let v = express::validate_document(&g);
    assert!(
        v.is_empty(),
        "{}",
        v.iter().map(|v| format!("\n{v}")).collect::<String>()
    );
}

fn kinds(data: &str) -> Vec<(u64, Kind, Option<String>)> {
    let v = express::validate_document(&graph(data));
    v.iter()
        .map(|v| (v.id, v.kind, v.attribute.clone()))
        .collect()
}

#[test]
fn known_bad_instances_are_caught() {
    // A runout zone definition with two attributes of three (OpenCascade writes it so).
    assert_eq!(
        kinds(
            "#20=TOLERANCE_ZONE('',$,#7,.T.,(#25),#21);\n#25=FLATNESS_TOLERANCE('',$,#26,#27);\n\
               #26=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.1),#8);\n#27=SHAPE_ASPECT('',$,#7,.T.);\n#21=TOLERANCE_ZONE_FORM('cylindrical');\n\
               #22=PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(0.),#8);\n\
               #23=RUNOUT_ZONE_ORIENTATION(#22);\n#24=RUNOUT_ZONE_DEFINITION(#20,#23);"
        ),
        [(24, Kind::AttributeCount, None)]
    );
    // An untyped value in a measure_with_unit (its value_component is the SELECT measure_value).
    assert_eq!(
        kinds("#20=MEASURE_WITH_UNIT(0.075,#8);"),
        [(
            20,
            Kind::Untyped,
            Some("measure_with_unit.value_component".into())
        )]
    );
    // A complex instance that is the supertype closure of perpendicularity_tolerance.
    assert_eq!(
        kinds("#20=SHAPE_ASPECT('',$,#7,.T.);\n#21=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.1),#8);\n\
               #22=(GEOMETRIC_TOLERANCE('',$,#21,#20)GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE(())\
               PERPENDICULARITY_TOLERANCE());")
        .iter()
        .map(|k| k.1)
        .collect::<Vec<_>>(),
        // and an empty datum system, which SET [1:?] forbids.
        [Kind::InternalMapping, Kind::Bound]
    );
    // Complex parts out of order.
    assert_eq!(
        kinds("#20=(NAMED_UNIT(*)LENGTH_UNIT()SI_UNIT(.MILLI.,.METRE.));"),
        [(20, Kind::PartOrder, None)]
    );
    // A missing supertype part.
    assert_eq!(
        kinds("#20=(LENGTH_UNIT()SI_UNIT(.MILLI.,.METRE.));")[0].1,
        Kind::MissingPart
    );
    // Two subtypes ONEOF excludes.
    assert!(
        kinds("#20=(FLATNESS_TOLERANCE()GEOMETRIC_TOLERANCE('',$,#21,#22)POSITION_TOLERANCE());\n\
               #21=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.1),#8);\n#22=SHAPE_ASPECT('',$,#7,.T.);")
        .iter()
        .any(|k| k.1 == Kind::Combination)
    );
    // An ABSTRACT entity on its own.
    assert_eq!(
        kinds("#20=GEOMETRIC_TOLERANCE('',$,$,#21);\n#21=SHAPE_ASPECT('',$,#7,.T.);"),
        [(20, Kind::Combination, None)]
    );
    // A wrong enumeration value.
    assert_eq!(
        kinds("#20=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.INCH.));"),
        [(20, Kind::Enumeration, Some("si_unit.name".into()))]
    );
    // A missing non-optional attribute.
    assert_eq!(
        kinds("#20=SHAPE_ASPECT('',$,$,.T.);"),
        [(20, Kind::Unset, Some("shape_aspect.of_shape".into()))]
    );
    // A reference to an instance of the wrong type.
    assert_eq!(
        kinds("#20=SHAPE_ASPECT('',$,#6,.T.);"),
        [(20, Kind::Reference, Some("shape_aspect.of_shape".into()))]
    );
    // `*` where nothing is derived, a value where it is, and a dangling reference.
    assert_eq!(
        kinds(
            "#20=SHAPE_ASPECT(*,$,#7,.T.);\n#21=(LENGTH_UNIT()NAMED_UNIT(#99)SI_UNIT(.MILLI.,.METRE.));\n\
               #22=SHAPE_ASPECT('',$,#98,.T.);"
        ),
        [
            (20, Kind::Derived, Some("shape_aspect.name".into())),
            (21, Kind::Derived, Some("named_unit.dimensions".into())),
            (22, Kind::Dangling, Some("shape_aspect.of_shape".into())),
        ]
    );
    // Names the edition does not declare, a typed parameter where no SELECT is, a type not in
    // the SELECT, the wrong value kind and a wrong count.
    assert_eq!(
        kinds(
            "#20=NOT_AN_ENTITY('');\n#21=LENGTH_MEASURE(1.);\n#22=DIRECTION('',(LENGTH_MEASURE(1.),0.,0.));\n\
               #23=MEASURE_WITH_UNIT(LABEL('x'),#8);\n#24=DIRECTION('',(1,0.,0.));\n#25=DIRECTION('');"
        ),
        [
            (20, Kind::Undeclared, None),
            (21, Kind::Undeclared, None),
            (22, Kind::Typed, Some("direction.direction_ratios".into())),
            (
                23,
                Kind::Typed,
                Some("measure_with_unit.value_component".into())
            ),
            (
                24,
                Kind::ValueType,
                Some("direction.direction_ratios".into())
            ),
            (25, Kind::AttributeCount, None),
        ]
    );
}

#[test]
fn families_follow_the_rules() {
    for (root, _) in express::FAMILY_RULES {
        assert!(
            TARGET.is_declared(root),
            "{root} is not an entity type of the target"
        );
    }
    for e in TARGET.entities {
        assert_ne!(
            TARGET.family(e.name),
            Some(Family::ValidationProperty),
            "{}",
            e.name
        );
    }
    for (name, family) in [
        ("datum", Family::SemanticPmi),
        ("dimensional_location", Family::SemanticPmi),
        ("directed_dimensional_location", Family::SemanticPmi),
        ("geometric_item_specific_usage", Family::SemanticPmi),
        ("draughting_model_item_association", Family::Presentation),
        ("tessellated_annotation_occurrence", Family::Presentation),
        ("styled_item", Family::Presentation),
        ("thread", Family::SemanticPmi),
        ("boss", Family::Other),
        ("symbol_representation", Family::Other),
        ("advanced_face", Family::Other),
        ("property_definition", Family::Other),
    ] {
        assert_eq!(TARGET.family(name), Some(family), "{name}");
    }
    assert_eq!(
        TARGET.instance_family("PROPERTY_DEFINITION", Some("pmi validation property")),
        Some(Family::ValidationProperty)
    );
    assert_eq!(
        TARGET.instance_family("product_definition_shape", Some("")),
        Some(Family::Other)
    );
    assert_eq!(TARGET.family("not_an_entity"), None);
}

/// Every entity type in a NIST file has its pinned family, under the target edition.
#[test]
fn nist_entity_types_have_their_pinned_family() {
    let pinned: BTreeMap<String, String> =
        serde_json::from_str(&std::fs::read_to_string(fixtures().join("families.json")).unwrap())
            .unwrap();
    let family = |n: &str| {
        TARGET
            .family(n)
            .map_or("undeclared", Family::as_str)
            .to_string()
    };
    let changed: Vec<String> = pinned
        .iter()
        .filter(|(n, f)| family(n) != **f)
        .map(|(n, f)| format!("{n}: pinned {f}, now {}", family(n)))
        .collect();
    assert!(
        changed.is_empty(),
        "families changed:\n{}",
        changed.join("\n")
    );
    let Some(files) = nist_files() else { return };
    let mut names = BTreeSet::new();
    for (_, g) in &files {
        for e in g.entities.values() {
            match e {
                step_io::parser::RawEntity::Simple { name, .. } => {
                    names.insert(name.to_ascii_lowercase());
                }
                step_io::parser::RawEntity::Complex { parts, .. } => {
                    names.extend(parts.iter().map(|p| p.name.to_ascii_lowercase()));
                }
            }
        }
    }
    let unpinned: Vec<String> = names
        .iter()
        .filter(|n| !pinned.contains_key(*n))
        .map(|n| format!("\"{n}\": \"{}\",", family(n)))
        .collect();
    let stale: Vec<&String> = pinned.keys().filter(|n| !names.contains(*n)).collect();
    assert!(
        unpinned.is_empty() && stale.is_empty(),
        "unpinned:\n{}\nstale: {stale:?}",
        unpinned.join("\n")
    );
}

const VERDICTS: [&str; 3] = ["file-wrong", "edition-difference", "undetermined"];

/// Violations grouped by entity, kind and attribute, with counts.
fn groups(violations: &[Violation]) -> BTreeMap<(String, String, String), u64> {
    let mut out = BTreeMap::new();
    for v in violations {
        let key = (
            v.entity.clone(),
            v.kind.as_str().to_string(),
            v.attribute.clone().unwrap_or_default(),
        );
        *out.entry(key).or_insert(0) += 1;
    }
    out
}

/// Compare computed groups with a pinned list; returns the problems and the list as it now is
/// (pinned reasons and verdicts kept, new groups without).
fn compare(
    what: &str,
    computed: &BTreeMap<(String, String, String), u64>,
    pinned: &Value,
    problems: &mut Vec<String>,
) -> Value {
    let mut by_key = BTreeMap::new();
    for g in pinned.as_array().into_iter().flatten() {
        let key = (
            g["entity"].as_str().unwrap_or_default().to_string(),
            g["kind"].as_str().unwrap_or_default().to_string(),
            g["attribute"].as_str().unwrap_or_default().to_string(),
        );
        if !g["reason"].as_str().is_some_and(|r| !r.is_empty())
            || !g["verdict"].as_str().is_some_and(|v| VERDICTS.contains(&v))
        {
            problems.push(format!(
                "{what}: {key:?} has no reason or no verdict ({VERDICTS:?})"
            ));
        }
        by_key.insert(key, g.clone());
    }
    let mut now = Vec::new();
    for (key, &count) in computed {
        let mut g = by_key.remove(key).unwrap_or_else(|| {
            problems.push(format!("{what}: unpinned {key:?} x{count}"));
            json!({"entity": key.0, "kind": key.1, "attribute": key.2, "reason": "", "verdict": ""})
        });
        if g["count"].as_u64() != Some(count) {
            if g["count"].is_u64() {
                problems.push(format!(
                    "{what}: {key:?} pinned x{}, now x{count}",
                    g["count"]
                ));
            }
            g["count"] = json!(count);
        }
        now.push(g);
    }
    for key in by_key.keys() {
        problems.push(format!("{what}: pinned {key:?} no longer occurs"));
    }
    Value::Array(now)
}

/// Write the lists as they now are to `HAECCEITY_EXPRESS_ACTUAL` (a directory), for review.
fn write_actual(name: &str, value: &Value) {
    if let Some(dir) = std::env::var_os("HAECCEITY_EXPRESS_ACTUAL") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            Path::new(&dir).join(name),
            serde_json::to_string_pretty(value).unwrap() + "\n",
        )
        .unwrap();
    }
}

fn file_schema(g: &Graph) -> String {
    g.schema.raw.join(",")
}

/// The NIST AP242 test files (`HAECCEITY_NIST_PMI`: the `NIST-PMI-STEP-Files` directory),
/// parsed; `None` when absent and not required.
fn nist_files() -> Option<Vec<(String, Graph)>> {
    let Some(dir) = std::env::var_os("HAECCEITY_NIST_PMI").map(PathBuf::from) else {
        assert!(
            std::env::var_os("HAECCEITY_NIST_PMI_REQUIRED").is_none(),
            "HAECCEITY_NIST_PMI_REQUIRED is set but HAECCEITY_NIST_PMI is not"
        );
        eprintln!("HAECCEITY_NIST_PMI not set; NIST files skipped");
        return None;
    };
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".stp") && n.contains("_ap242"))
        .collect();
    names.sort();
    assert!(!names.is_empty(), "no AP242 files in {}", dir.display());
    Some(common::parallel::map(&names, |n| {
        (
            n.clone(),
            parse(std::fs::read(dir.join(n)).unwrap().as_slice()).unwrap(),
        )
    }))
}

#[test]
fn nist_files_violations_are_pinned() {
    let Some(files) = nist_files() else { return };
    let pinned: Value = serde_json::from_str(
        &std::fs::read_to_string(fixtures().join("known_nist_violations.json")).unwrap(),
    )
    .unwrap();
    let results = common::parallel::map(&files, |(name, g)| {
        let schema = file_schema(g);
        let own = express::edition(&schema).map(|ed| groups(&ed.validate_document(g)));
        let upgrade = (express::edition(&schema).map(|e| e.file_schema) != Some(TARGET_SCHEMA))
            .then(|| groups(&TARGET.validate_document(g)));
        (name.clone(), schema, own, upgrade)
    });
    let mut problems = Vec::new();
    let mut now = serde_json::Map::new();
    for (name, schema, own, upgrade) in results {
        let p = &pinned["files"][name.as_str()];
        if p["file_schema"].as_str() != Some(schema.as_str()) {
            problems.push(format!(
                "{name}: FILE_SCHEMA {schema}, pinned {}",
                p["file_schema"]
            ));
        }
        let edition = express::edition(&schema).map(|e| e.file_schema);
        if p["edition"].as_str() != edition {
            problems.push(format!(
                "{name}: edition table {edition:?}, pinned {}",
                p["edition"]
            ));
        }
        let mut entry = json!({"file_schema": schema, "edition": edition});
        entry["own"] = match &own {
            Some(own) => compare(&format!("{name} own"), own, &p["own"], &mut problems),
            None => {
                if !p["own"].is_null() {
                    problems.push(format!(
                        "{name}: no table for its edition, but own violations are pinned"
                    ));
                }
                Value::Null
            }
        };
        entry["upgrade"] = match &upgrade {
            Some(up) => compare(&format!("{name} upgrade"), up, &p["upgrade"], &mut problems),
            None => Value::Null,
        };
        now.insert(name.clone(), entry);
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
    write_actual("known_nist_violations.json", &json!({"files": now}));
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Every corpus file (AP214 and AP203), validated against the target as if its FILE_SCHEMA
/// were changed to it: with no repair, with the closed reported repairs, or refused.
/// The original violations retain their verdicts and reasons even when repaired.
/// Replace performs repairs; Add must refuse exactly the entities needing changes.
#[test]
fn corpus_upgrades_are_pinned() {
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let corpus = common::load("corpus.json");
    let names: Vec<String> = corpus["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["file"].as_str().unwrap().to_string())
        .filter(|n| {
            [".step", ".stp", ".step.gz", ".stp.gz"]
                .iter()
                .any(|x| n.to_ascii_lowercase().ends_with(x))
        })
        .collect();
    assert_eq!(names.len(), 100);
    let pinned: Value = serde_json::from_str(
        &std::fs::read_to_string(fixtures().join("corpus_upgrade.json")).unwrap(),
    )
    .unwrap();
    let results = common::parallel::map(&names, |n| {
        let path = dir.join(n);
        let mut bytes = std::fs::read(&path).unwrap();
        if n.ends_with(".gz") {
            let mut out = Vec::new();
            flate2::read::GzDecoder::new(&bytes[..])
                .read_to_end(&mut out)
                .unwrap();
            bytes = out;
        }
        let doc = haecceity::p21::Document::parse(bytes).unwrap();
        let original = groups(&TARGET.validate_document(doc.graph()));
        // No repair is needed when the original already passes. Exercise the actual writer
        // for every original that fails; its all-instance and no-new-rule checks are the verdict.
        let (upgradable, repairs) = if original.is_empty() {
            (true, Vec::new())
        } else {
            use haecceity::pmi::{self, *};
            let parts = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
            let pmi = PartPmi {
                features: vec![Feature::Items(vec![Anchor::Face(FaceIndex(0))])],
                datums: vec![
                    Datum::new(
                        DatumLabel::new("Z").unwrap(),
                        Some(FeatureId(0)),
                        Vec::new(),
                    )
                    .unwrap(),
                ],
                ..PartPmi::default()
            };
            let requested = [(PartId(0), pmi)];
            match pmi::write(
                &doc,
                &parts,
                &requested,
                pmi::Mode::Replace,
                pmi::write::PresentationPolicy::RemovePresentation,
            ) {
                Ok((_, report)) => {
                    let mut changed: Vec<_> = report
                        .repairs
                        .iter()
                        .map(|r| r.id)
                        .chain(report.edition_rewrites.iter().map(|r| r.id))
                        .collect();
                    changed.sort_unstable();
                    changed.dedup();
                    assert!(!changed.is_empty(), "{n}: upgrade requires source repairs");
                    assert!(
                        matches!(
                            pmi::write(&doc, &parts, &requested, pmi::Mode::Add,
                                pmi::write::PresentationPolicy::RemovePresentation),
                            Err(pmi::write::WriteError::AddPreservation { ids, .. }) if ids == changed
                        ),
                        "{n}: Add must refuse exactly {changed:?}"
                    );
                    (true, report.repairs.iter().map(|r| json!({
                        "id": r.id,
                        "repair": match r.repair {
                            pmi::write::UpgradeRepairKind::R1ApprovalAssignment => "R1",
                            pmi::write::UpgradeRepairKind::R2ConversionUnitDimensions => "R2",
                        }
                    })).collect())
                }
                Err(_) => (false, Vec::new()),
            }
        };
        (file_schema(doc.graph()), original, upgradable, repairs)
    });
    let mut problems = Vec::new();
    let mut now = serde_json::Map::new();
    for (name, (schema, up, upgradable, repairs)) in names.iter().zip(results) {
        let p = &pinned["files"][name.as_str()];
        if p["file_schema"].as_str() != Some(schema.as_str()) {
            problems.push(format!(
                "{name}: FILE_SCHEMA {schema}, pinned {}",
                p["file_schema"]
            ));
        }
        if p["upgradable"].as_bool() != Some(upgradable) {
            problems.push(format!(
                "{name}: upgradable {}, pinned {}",
                upgradable, p["upgradable"]
            ));
        }
        let outcome = if !upgradable {
            "refused"
        } else if repairs.is_empty() {
            "without-repairs"
        } else {
            "with-repairs"
        };
        if p["repairs"] != json!(repairs) || p["outcome"] != json!(outcome) {
            problems.push(format!(
                "{name}: outcome {outcome}, repairs {repairs:?}, pinned {p}"
            ));
        }
        let groups = compare(name, &up, &p["violations"], &mut problems);
        now.insert(
            name.clone(),
            json!({"file_schema": schema, "upgradable": upgradable, "violations": groups,
                   "outcome": outcome, "repairs": repairs}),
        );
    }
    assert_eq!(
        now.values().filter(|p| p["upgradable"] == true).count(),
        100
    );
    assert_eq!(
        now.values()
            .filter(|p| p["outcome"] == "with-repairs")
            .count(),
        6
    );
    write_actual("corpus_upgrade.json", &json!({"files": now}));
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn every_edition_validates_its_own_hand_instances() {
    // The same instances are valid in edition 1: none of what they use changed.
    let g = graph(
        "#20=SHAPE_ASPECT('',$,#7,.T.);\n#21=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.1),#8);\n#22=FLATNESS_TOLERANCE('',$,#21,#20);",
    );
    for ed in [&AP242_ED1, &AP242_ED4] {
        let ed: &Edition = ed;
        let v = ed.validate_document(&g);
        assert!(v.is_empty(), "{}: {v:?}", ed.file_schema);
    }
}

/// The kinds of the violations of the one DATA record of `data`, its references' types given.
fn record_kinds(data: &str, targets: &[(u64, &str)]) -> Vec<(Kind, Option<String>)> {
    let text = format!("{HEADER}{data}\nENDSEC;\nEND-ISO-10303-21;\n");
    let g = parse(text.as_bytes()).unwrap();
    let record = g.entities.values().next().unwrap();
    let types = |id: u64| {
        targets
            .iter()
            .find(|t| t.0 == id)
            .map(|t| vec![t.1.to_string()])
    };
    TARGET
        .validate(record, &types)
        .into_iter()
        .map(|v| (v.kind, v.attribute))
        .collect()
}

#[test]
fn redeclarations_narrowing_a_select_keep_its_typed_encoding() {
    // compound_item_definition (a SELECT of two defined aggregates) narrowed to
    // list_representation_item: the value is still a typed parameter, and must be that type.
    let items = [(1, "representation_item"), (2, "representation_item")];
    assert_eq!(
        record_kinds(
            "#20=ROW_REPRESENTATION_ITEM('',LIST_REPRESENTATION_ITEM((#1,#2)));",
            &items
        ),
        []
    );
    let at = Some("compound_representation_item.item_element".to_string());
    assert_eq!(
        record_kinds(
            "#20=ROW_REPRESENTATION_ITEM('',SET_REPRESENTATION_ITEM((#1,#2)));",
            &items
        ),
        [(Kind::Typed, at.clone())]
    );
    assert_eq!(
        record_kinds("#20=ROW_REPRESENTATION_ITEM('',(#1,#2));", &items),
        [(Kind::Untyped, at)]
    );
    // measure_value narrowed to positive_length_measure: a positive length measure (or a type
    // defined from one), typed; a wider length_measure or an untyped value is a violation.
    let criterion = [(21, "non_manifold_at_triangle_vertex")];
    let entity = "TSDQ_POSITIVE_LENGTH_MEASURE_FOR_NON_MANIFOLD_AT_TRIANGLE_VERTEX";
    assert_eq!(
        record_kinds(
            &format!("#20={entity}('',#21,POSITIVE_LENGTH_MEASURE(0.1));"),
            &criterion
        ),
        []
    );
    let at = Some("a3m_data_quality_criterion_specific_applied_value.applied_value".to_string());
    assert_eq!(
        record_kinds(
            &format!("#20={entity}('',#21,LENGTH_MEASURE(0.1));"),
            &criterion
        ),
        [(Kind::Typed, at.clone())]
    );
    assert_eq!(
        record_kinds(&format!("#20={entity}('',#21,0.1);"), &criterion),
        [(Kind::Untyped, at)]
    );
}

#[test]
fn a_select_member_renaming_a_select_contributes_its_entity_types() {
    // experience_item has the member experience_type_classification_item = classification_item,
    // a SELECT that includes product_definition: a plain reference to one is valid, and one
    // to an application_context, in none of its SELECTs, is not.
    let targets = [
        (30, "experience"),
        (31, "experience_role"),
        (6, "product_definition"),
        (7, "application_context"),
    ];
    assert_eq!(
        record_kinds(
            "#20=APPLIED_EXPERIENCE_ASSIGNMENT('','',$,#30,#31,(#6));",
            &targets
        ),
        []
    );
    assert_eq!(
        record_kinds(
            "#20=APPLIED_EXPERIENCE_ASSIGNMENT('','',$,#30,#31,(#7));",
            &targets
        ),
        [(
            Kind::Reference,
            Some("applied_experience_assignment.items".into())
        )]
    );
}
