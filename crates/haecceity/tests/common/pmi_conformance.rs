//! Structural CAx-IF PMI 4.1 checks, independent of writer byte snapshots and SFA.
use super::pmi_usage::{attr, is_a, reference};
use haecceity::p21::{Attribute as A, Document, RawEntity};

pub const PMI_RP: &str = "CAx-IF Rec.Pracs.---Representation and Presentation of Product Manufacturing Information (PMI)---4.1---2024-06-20";
const TYPES: &[&str] = &[
    "angularity",
    "circular runout",
    "circularity",
    "coaxiality",
    "concentricity",
    "cylindricity",
    "flatness",
    "parallelism",
    "perpendicularity",
    "position",
    "profile of line",
    "profile of surface",
    "roundness",
    "straightness",
    "symmetry",
    "total runout",
    "general tolerance",
    "linear dimension",
    "radial dimension",
    "diameter dimension",
    "angular dimension",
    "ordinate dimension",
    "curve dimension",
    "general dimension",
    "datum",
    "datum target",
    "note",
    "label",
    "surface roughness",
    "weld symbol",
    "general note",
];
fn refs(value: Option<&A>) -> Vec<u64> {
    match value {
        Some(A::List(items)) => items
            .iter()
            .filter_map(|a| match a {
                A::EntityRef(r) => Some(*r),
                _ => None,
            })
            .collect(),
        _ => vec![],
    }
}
pub fn issues(doc: &Document) -> Vec<String> {
    let mut issues = Vec::new();
    if !doc.header().iter().any(|h| {
        matches!(h, RawEntity::Simple { name, attributes, .. }
        if name == "FILE_DESCRIPTION" && matches!(attributes.first(), Some(A::List(entries))
            if entries.contains(&A::String(PMI_RP.into()))))
    }) {
        issues.push("§1.2: missing FILE_DESCRIPTION PMI RP reference".into());
    }
    let mut aco = false;
    let mut tao = false;
    for id in doc.ids() {
        for (occurrence, allowed) in [
            ("annotation_curve_occurrence", "geometric_curve_set"),
            ("annotation_fill_area_occurrence", "annotation_fill_area"),
            (
                "tessellated_annotation_occurrence",
                "tessellated_geometric_set",
            ),
            ("annotation_placeholder_occurrence", "geometric_set"),
        ] {
            if !is_a(doc, id, occurrence) {
                continue;
            }
            aco |= occurrence == "annotation_curve_occurrence";
            tao |= occurrence == "tessellated_annotation_occurrence";
            let item = reference(doc, id, "styled_item", "item");
            if !item.is_some_and(|r| is_a(doc, r, allowed)) {
                issues.push(format!(
                    "§7.2/8.1/8.2: #{id} {occurrence} item must be {allowed}"
                ));
            }
            if occurrence == "annotation_placeholder_occurrence" {
                let members = item
                    .map(|r| refs(attr(doc, r, "geometric_set", "elements")))
                    .unwrap_or_default();
                if !members.iter().any(|&r| is_a(doc, r, "axis2_placement_3d")) {
                    issues.push(format!(
                        "§7.2.2: #{id} placeholder missing direct axis2_placement_3d"
                    ));
                }
                if !doc
                    .referrers(id)
                    .iter()
                    .any(|&r| is_a(doc, r, "draughting_model_item_association_with_placeholder"))
                {
                    issues.push(format!(
                        "§7.2.3: #{id} placeholder missing semantic association"
                    ));
                }
                if matches!(attr(doc, id, "representation_item", "name"), Some(A::String(s)) if s.starts_with("haecceity placed text"))
                {
                    issues.push(format!(
                        "§7.2.2: #{id} placeholder has transport data as its name"
                    ));
                }
            }
        }
        if [
            "geometric_set",
            "annotation_fill_area",
            "tessellated_geometric_set",
        ]
        .iter()
        .any(|ty| is_a(doc, id, ty))
            && !matches!(attr(doc, id, "representation_item", "name"), Some(A::String(s)) if TYPES.contains(&s.as_str()))
        {
            issues.push(format!("§8.4: #{id} set name is not in Table 18"));
        }
        if is_a(doc, id, "draughting_callout") {
            let name = attr(doc, id, "representation_item", "name");
            for child in refs(attr(doc, id, "draughting_callout", "contents")) {
                if attr(doc, child, "representation_item", "name") != name {
                    issues.push(format!(
                        "§7.2.2/8.3: #{child} occurrence name differs from callout #{id}"
                    ));
                }
            }
        }
    }
    if aco && tao {
        issues.push("§8.3: mixed ACO and TAO presentation in one file".into());
    }
    issues
}
pub fn assert_conforms(doc: &Document) {
    assert_eq!(issues(doc), Vec::<String>::new());
}
