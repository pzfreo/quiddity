//! CAx-IF PMI v4.1 §5.1: a face has one identifying aspect/usage across all GD&T.
use haecceity::{
    express,
    p21::{Attribute, Document, RawEntity},
};
use std::collections::{BTreeMap, BTreeSet};

pub fn is_a(doc: &Document, id: u64, ty: &str) -> bool {
    match doc.get(id) {
        Some(RawEntity::Simple { name, .. }) => express::is_a(name, ty),
        Some(RawEntity::Complex { parts, .. }) => parts.iter().any(|p| express::is_a(&p.name, ty)),
        None => false,
    }
}
pub fn attr<'a>(doc: &'a Document, id: u64, entity: &str, name: &str) -> Option<&'a Attribute> {
    match doc.get(id)? {
        RawEntity::Simple {
            name: ty,
            attributes,
            ..
        } => {
            let slots = express::TARGET.explicit_attributes(ty)?;
            attributes.get(
                slots
                    .iter()
                    .position(|a| a.entity == entity && a.name == name)?,
            )
        }
        RawEntity::Complex { parts, .. } => {
            let part = parts.iter().find(|p| p.name.eq_ignore_ascii_case(entity))?;
            let decl = express::TARGET.entity(entity)?;
            part.attributes
                .get(decl.attributes.iter().position(|a| a.name == name)?)
        }
    }
}
pub fn reference(doc: &Document, id: u64, entity: &str, name: &str) -> Option<u64> {
    match attr(doc, id, entity, name)? {
        Attribute::EntityRef(r) => Some(*r),
        _ => None,
    }
}
fn references(a: &Attribute, out: &mut Vec<u64>) {
    match a {
        Attribute::EntityRef(id) => out.push(*id),
        Attribute::List(items) => items.iter().for_each(|a| references(a, out)),
        Attribute::Typed { value, .. } => references(value, out),
        _ => {}
    }
}
/// Neutral unions can be reused without importing an unrelated specialized meaning.
pub fn neutral(doc: &Document, id: u64) -> bool {
    let allowed = |name: &str| {
        matches!(
            name.to_ascii_lowercase().as_str(),
            "shape_aspect" | "composite_shape_aspect"
        )
    };
    match doc.get(id) {
        Some(RawEntity::Simple { name, .. }) => allowed(name),
        Some(RawEntity::Complex { parts, .. }) => parts.iter().all(|p| allowed(&p.name)),
        None => false,
    }
}
fn composition(doc: &Document, id: u64) -> bool {
    matches!(doc.get(id),Some(RawEntity::Simple { name, .. }) if name.eq_ignore_ascii_case("shape_aspect_relationship"))
        && !["name", "description"].iter().any(|field| {
            attr(doc, id, "shape_aspect_relationship", field)
                == Some(&Attribute::String("affected plane association".into()))
        })
        && !doc.referrers(id).iter().any(|r| {
            reference(doc, *r, "geometric_tolerance", "toleranced_shape_aspect") == Some(id)
        })
}
/// All geometric/topological items identified by an aspect, through actual composition.
pub fn geometry(doc: &Document, aspect: u64) -> BTreeSet<u64> {
    let mut out = BTreeSet::new();
    let mut todo = vec![aspect];
    let mut seen = BTreeSet::new();
    while let Some(id) = todo.pop() {
        if !seen.insert(id) {
            continue;
        }
        for &r in doc.referrers(id) {
            if reference(doc, r, "item_identified_representation_usage", "definition") == Some(id)
                && !is_a(doc, r, "draughting_model_item_association")
            {
                let mut items = vec![];
                if let Some(a) = attr(
                    doc,
                    r,
                    "item_identified_representation_usage",
                    "identified_item",
                ) {
                    references(a, &mut items);
                }
                out.extend(items.into_iter().filter(|i| geometric(doc, *i)));
            }
            if composition(doc, r)
                && reference(doc, r, "shape_aspect_relationship", "relating_shape_aspect")
                    == Some(id)
                && let Some(member) =
                    reference(doc, r, "shape_aspect_relationship", "related_shape_aspect")
                && !is_a(doc, member, "datum")
                && !is_a(doc, member, "datum_target")
            {
                todo.push(member);
            }
        }
    }
    out
}
fn geometric(doc: &Document, id: u64) -> bool {
    is_a(doc, id, "geometric_representation_item")
        || is_a(doc, id, "topological_representation_item")
}
/// Resolve the feature scope as well as its geometry. Per-member/all-around scopes
/// with identical face sets are different features (§6.5.2); legacy links to a group
/// inherit that group's scope, just as the reader's compatibility mapping does.
fn signature(
    doc: &Document,
    id: u64,
    geometry: &BTreeMap<u64, BTreeSet<u64>>,
    cache: &mut BTreeMap<u64, String>,
    visiting: &mut BTreeSet<u64>,
) -> String {
    if let Some(s) = cache.get(&id) {
        return s.clone();
    }
    if !visiting.insert(id) {
        return format!("cycle:{id}");
    }
    let children: Vec<_> = doc
        .referrers(id)
        .iter()
        .copied()
        .filter(|r| {
            composition(doc, *r)
                && reference(
                    doc,
                    *r,
                    "shape_aspect_relationship",
                    "relating_shape_aspect",
                ) == Some(id)
        })
        .filter_map(|r| reference(doc, r, "shape_aspect_relationship", "related_shape_aspect"))
        .filter(|m| !is_a(doc, *m, "datum") && !is_a(doc, *m, "datum_target"))
        .collect();
    let direct = doc.referrers(id).iter().any(|r| {
        !is_a(doc, *r, "draughting_model_item_association")
            && reference(
                doc,
                *r,
                "item_identified_representation_usage",
                "definition",
            ) == Some(id)
    });
    let words = [
        attr(doc, id, "shape_aspect", "name"),
        attr(doc, id, "shape_aspect", "description"),
    ];
    let named = |name: &str| {
        words
            .iter()
            .any(|a| matches!(a, Some(Attribute::String(s)) if s == name))
    };
    let group = if is_a(doc, id, "all_around_shape_aspect") {
        Some("all-around")
    } else if is_a(doc, id, "between_shape_aspect") {
        Some("between")
    } else if is_a(doc, id, "composite_group_shape_aspect")
        || is_a(doc, id, "composite_shape_aspect")
            && (named("multiple elements") || named("pattern of features"))
    {
        Some(if named("pattern of features") {
            "pattern"
        } else {
            "multiple-elements"
        })
    } else {
        None
    };
    let mut child_keys: Vec<_> = children
        .iter()
        .map(|child| signature(doc, *child, geometry, cache, visiting))
        .collect();
    child_keys.sort();
    let s = if let Some(group) = group {
        format!("scope:{group}({child_keys:?})")
    } else if !direct && child_keys.len() == 1 && child_keys[0].starts_with("scope:") {
        child_keys[0].clone()
    } else if neutral(doc, id) || is_a(doc, id, "datum_feature") {
        format!("union:{:?}", geometry.get(&id))
    } else {
        format!(
            "special:{}:{:?}",
            match doc.get(id) {
                Some(RawEntity::Simple { name, .. }) => name.clone(),
                Some(RawEntity::Complex { parts, .. }) => parts
                    .iter()
                    .map(|p| p.name.clone())
                    .collect::<Vec<_>>()
                    .join("+"),
                None => String::new(),
            },
            geometry.get(&id)
        )
    };
    visiting.remove(&id);
    cache.insert(id, s.clone());
    s
}
/// Includes kept GISU/IIRU and SET items, all geometric items, redundant multi-member
/// wrappers, and the requirement to reference an exactly matching datum feature directly.
pub fn violations(doc: &Document) -> Vec<String> {
    let mut identified: BTreeMap<u64, BTreeSet<(u64, u64)>> = BTreeMap::new();
    let mut direct = BTreeSet::new();
    let mut members: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
    let mut targets = BTreeSet::new();
    let aspects: Vec<_> = doc
        .ids()
        .filter(|id| is_a(doc, *id, "shape_aspect"))
        .collect();
    let content: BTreeMap<_, _> = aspects.iter().map(|id| (*id, geometry(doc, *id))).collect();
    for id in doc.ids() {
        if is_a(doc, id, "item_identified_representation_usage")
            && !is_a(doc, id, "draughting_model_item_association")
            && let Some(owner) = reference(
                doc,
                id,
                "item_identified_representation_usage",
                "definition",
            )
            && is_a(doc, owner, "shape_aspect")
        {
            direct.insert(owner);
            let mut items = vec![];
            if let Some(a) = attr(
                doc,
                id,
                "item_identified_representation_usage",
                "identified_item",
            ) {
                references(a, &mut items);
            }
            for item in items.into_iter().filter(|i| geometric(doc, *i)) {
                identified.entry(item).or_default().insert((owner, id));
            }
        }
        if composition(doc, id)
            && let (Some(a), Some(b)) = (
                reference(
                    doc,
                    id,
                    "shape_aspect_relationship",
                    "relating_shape_aspect",
                ),
                reference(doc, id, "shape_aspect_relationship", "related_shape_aspect"),
            )
            && !is_a(doc, b, "datum")
            && !is_a(doc, b, "datum_target")
        {
            members.entry(a).or_default().insert(b);
        }
        if is_a(doc, id, "dimensional_location") {
            for field in ["relating_shape_aspect", "related_shape_aspect"] {
                targets.extend(reference(doc, id, "shape_aspect_relationship", field));
            }
        }
        for (ty, field) in [
            ("dimensional_size", "applies_to"),
            ("geometric_tolerance", "toleranced_shape_aspect"),
        ] {
            if is_a(doc, id, ty)
                && let Some(target) = reference(doc, id, ty, field)
            {
                targets.insert(target);
                if is_a(doc, target, "shape_aspect_relationship") {
                    for field in ["relating_shape_aspect", "related_shape_aspect"] {
                        targets.extend(reference(doc, target, "shape_aspect_relationship", field));
                    }
                }
            }
        }
    }
    let mut signatures = BTreeMap::new();
    for &id in &aspects {
        signature(doc, id, &content, &mut signatures, &mut BTreeSet::new());
    }
    let mut out = vec![];
    for (item, owners) in identified {
        if owners.len() > 1 {
            out.push(format!("item #{item} identified by {owners:?}"));
        }
    }
    for &id in &aspects {
        // Specialized scopes (pattern, all-around, zone, area, derivation, datum) have
        // their own meaning. Only interchangeable neutral gatherers are forwarders.
        let legacy =
            attr(doc, id, "shape_aspect", "name") == Some(&Attribute::String("DIMENSION".into()));
        if !(neutral(doc, id) || legacy)
            || is_a(doc, id, "datum_feature")
            || direct.contains(&id)
            || content[&id].is_empty()
        {
            continue;
        }
        if let Some(other) = aspects.iter().copied().find(|other| {
            *other != id
                && (neutral(doc, *other) || is_a(doc, *other, "datum_feature"))
                && signatures[other] == signatures[&id]
        }) {
            out.push(format!(
                "aspect #{id} is a faceless forwarding aspect duplicating #{other}"
            ));
        } else if members.get(&id).is_some_and(|m| m.len() == 1) {
            out.push(format!("aspect #{id} is a faceless forwarding aspect"));
        }
    }
    for target in targets {
        let Some(items) = content.get(&target) else {
            continue;
        };
        if is_a(doc, target, "datum_feature")
            || !items.iter().any(|id| is_a(doc, *id, "advanced_face"))
        {
            continue;
        }
        if let Some(datum) = aspects.iter().copied().find(|id| {
            is_a(doc, *id, "datum_feature")
                && *id != target
                && signatures[id] == signatures[&target]
        }) {
            out.push(format!(
                "endpoint #{target} must reference datum feature #{datum}"
            ));
        }
    }
    out
}
