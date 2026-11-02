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
/// Includes kept records and both GISU and IIRU (including SET_REPRESENTATION_ITEM).
/// Genuine multi-member composites are allowed; one-link forwarding aliases are not.
pub fn violations(doc: &Document) -> Vec<String> {
    let mut faces: BTreeMap<u64, BTreeSet<(u64, u64)>> = BTreeMap::new();
    let mut direct = BTreeSet::new();
    let mut members: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
    let mut targets = BTreeSet::new();
    for id in doc.ids() {
        if is_a(doc, id, "item_identified_representation_usage") {
            let Some(owner) = reference(
                doc,
                id,
                "item_identified_representation_usage",
                "definition",
            ) else {
                continue;
            };
            if !is_a(doc, owner, "shape_aspect") {
                continue;
            }
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
            for face in items.into_iter().filter(|f| is_a(doc, *f, "advanced_face")) {
                faces.entry(face).or_default().insert((owner, id));
            }
        }
        if matches!(doc.get(id), Some(RawEntity::Simple { name, .. }) if name.eq_ignore_ascii_case("shape_aspect_relationship"))
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
        {
            members.entry(a).or_default().insert(b);
        }
        if is_a(doc, id, "dimensional_location") {
            for name in ["relating_shape_aspect", "related_shape_aspect"] {
                targets.extend(reference(doc, id, "shape_aspect_relationship", name));
            }
        }
        for (ty, name) in [
            ("dimensional_size", "applies_to"),
            ("geometric_tolerance", "toleranced_shape_aspect"),
        ] {
            if is_a(doc, id, ty) {
                targets.extend(reference(doc, id, ty, name));
            }
        }
    }
    let mut out = vec![];
    for (face, owners) in faces {
        if owners.len() > 1 {
            out.push(format!("face #{face} identified by {owners:?}"));
        }
    }
    for target in targets {
        if !direct.contains(&target)
            && members.get(&target).is_some_and(|m| m.len() == 1)
            && ![
                "derived_shape_aspect",
                "composite_group_shape_aspect",
                "all_around_shape_aspect",
                "between_shape_aspect",
            ]
            .iter()
            .any(|ty| is_a(doc, target, ty))
        {
            out.push(format!(
                "target #{target} is a faceless forwarding aspect: {:?}",
                members[&target]
            ));
        }
    }
    out
}
