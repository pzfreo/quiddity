//! Keep foreign presentation by rebinding unchanged semantic identities after replacement.
use super::*;
use crate::p21::{Edit, RawEntity};
use presentation::{attr, is, reference};

pub(super) struct Kept {
    part: pmi::PartId,
    collection: String,
    index: usize,
    definition: u64,
    record: RawEntity,
    source_id: u64,
    model: u64,
    item: u64,
    context: u64,
}
pub(super) fn detach(
    doc: &Document,
    parts: &[PartDefinition],
    requested: &[(pmi::PartId, Semantic)],
) -> Result<(Document, Vec<Kept>, BTreeSet<u64>)> {
    let read = match pmi::read(doc, parts) {
        Ok(read) => read,
        Err(pmi::ReadError::Transport(_)) => {
            pmi::read_native(doc, parts).map_err(|e| Error(e.to_string()))?
        }
        Err(e) => return Err(Error(e.to_string())),
    };
    let mut kept = vec![];
    let mut edit = Edit::new();
    let mut detached = BTreeSet::new();
    for id in doc
        .ids()
        .filter(|&id| is(doc, id, "draughting_model_item_association"))
    {
        if matches!(attr(doc,id,"item_identified_representation_usage","description"),Some(Attribute::String(s)) if s=="haecceity.generated/1")
        {
            continue;
        }
        let Some(definition) = reference(attr(
            doc,
            id,
            "item_identified_representation_usage",
            "definition",
        )) else {
            continue;
        };
        for ((part, c, index), _) in read
            .json_carriers
            .iter()
            .filter(|(_, carrier)| **carrier == definition)
        {
            let Some((_, new)) = requested.iter().find(|(p, _)| p == part) else {
                continue;
            };
            let before = identify(
                read.parts[part.0].clone(),
                Some(&read.identifiers.parts[part.0]),
            )?;
            let Some(old_id) = before.ids.get(c).and_then(|ids| ids.get(*index)) else {
                continue;
            };
            let Some(new_index) = new
                .ids
                .get(c)
                .and_then(|ids| ids.iter().position(|id| id == old_id))
            else {
                continue;
            };
            let old_raw = serde_json::to_value(&before.pmi)?;
            let new_raw = serde_json::to_value(&new.pmi)?;
            // A note-only support association presents the note's text, rather than
            // an independent support annotation. Do not keep obsolete note text merely
            // because its geometric support survives.
            if c == "features" {
                let annotation = reference(attr(
                    doc,
                    id,
                    "item_identified_representation_usage",
                    "identified_item",
                ));
                let notes: Vec<_> = read.provenance.parts[part.0]
                    .note_callouts
                    .iter()
                    .enumerate()
                    .filter(|(_, roots)| annotation.is_some_and(|a| roots.contains(&a)))
                    .map(|(i, _)| i)
                    .collect();
                let mut unchanged_note = false;
                for &note in &notes {
                    if let Some(index) = before.ids["notes"].get(note).and_then(|id| {
                        new.ids["notes"]
                            .iter()
                            .position(|candidate| candidate == id)
                    }) && identity::content(
                        &old_raw,
                        "notes",
                        note,
                        &Anchors::default(),
                        &mut BTreeSet::new(),
                    )? == identity::content(
                        &new_raw,
                        "notes",
                        index,
                        &Anchors::default(),
                        &mut BTreeSet::new(),
                    )? {
                        unchanged_note = true;
                        break;
                    }
                }
                if !notes.is_empty() && !unchanged_note {
                    continue;
                }
            }
            if identity::content(
                &old_raw,
                c,
                *index,
                &Anchors::default(),
                &mut BTreeSet::new(),
            )? != identity::content(
                &new_raw,
                c,
                new_index,
                &Anchors::default(),
                &mut BTreeSet::new(),
            )? {
                continue;
            }
            let record = doc
                .get(id)
                .cloned()
                .ok_or_else(|| Error(format!("missing association #{id}")))?;
            let model = reference(attr(
                doc,
                id,
                "item_identified_representation_usage",
                "used_representation",
            ))
            .ok_or_else(|| Error(format!("#{id}: presentation has no representation")))?;
            let item = reference(attr(
                doc,
                id,
                "item_identified_representation_usage",
                "identified_item",
            ))
            .ok_or_else(|| Error(format!("#{id}: presentation has no item")))?;
            let context = reference(attr(doc, model, "representation", "context_of_items"))
                .ok_or_else(|| Error(format!("#{model}: presentation has no context")))?;
            if detached.insert(id) {
                edit.remove(id);
            }
            kept.push(Kept {
                part: *part,
                collection: c.clone(),
                index: new_index,
                definition,
                record,
                source_id: id,
                model,
                item,
                context,
            });
        }
    }
    let mut protected = BTreeSet::new();
    let semantic_carriers: BTreeSet<_> = read.json_carriers.values().copied().collect();
    for k in &kept {
        let mut stack = vec![k.item];
        while let Some(id) = stack.pop() {
            if (id != k.item && semantic_carriers.contains(&id)) || !protected.insert(id) {
                continue;
            }
            if let Some(record) = doc.get(id) {
                crate::p21::visit_attributes(record, &mut |a| {
                    if let Attribute::EntityRef(child) = a {
                        stack.push(*child)
                    }
                });
            }
        }
    }
    let bytes = doc.apply(&edit).map_err(|e| Error(e.to_string()))?.bytes;
    Ok((
        Document::parse(bytes).map_err(|e| Error(e.to_string()))?,
        kept,
        protected,
    ))
}
fn replace(a: &mut Attribute, old: u64, new: &Attribute) {
    match a {
        Attribute::EntityRef(id) if *id == old => *a = new.clone(),
        Attribute::List(a) => {
            for x in a {
                replace(x, old, new)
            }
        }
        Attribute::Typed { value, .. } => replace(value, old, new),
        _ => {}
    }
}
pub(super) fn attach(
    doc: &Document,
    read: &pmi::PmiRead,
    kept: Vec<Kept>,
) -> Result<(Vec<u8>, usize)> {
    let mut edit = Edit::new();
    let mut attached = BTreeSet::new();
    let mut added = 0;
    let mut names: BTreeSet<_> = doc
        .ids()
        .filter(|id| is(doc, *id, "draughting_model"))
        .filter_map(|id| match attr(doc, id, "representation", "name") {
            Some(Attribute::String(name)) => Some(name.clone()),
            _ => None,
        })
        .collect();
    let mut serial = 1;
    for mut k in kept {
        let new = *read
            .json_carriers
            .get(&(k.part, k.collection.clone(), k.index))
            .ok_or_else(|| {
                Error(format!(
                    "surviving presentation lost {} item {}",
                    k.collection, k.index
                ))
            })?;
        if !attached.insert((k.source_id, new)) {
            continue;
        }
        // A source can attach several definitions/placeholders to one callout, and
        // support aliases can split during typed emission. Separate linking models
        // keep UR1/UR2 valid while reusing all original drawn graphics and placements.
        let name = loop {
            let candidate = if serial == 1 {
                "As Drawn".into()
            } else {
                format!("As Drawn ({serial})")
            };
            serial += 1;
            if names.insert(candidate.clone()) {
                break candidate;
            }
        };
        let model = edit.add(crate::p21::simple(
            "DRAUGHTING_MODEL",
            vec![
                Attribute::String(name),
                Attribute::List(vec![Attribute::EntityRef(k.item)]),
                Attribute::EntityRef(k.context),
            ],
        ));
        match &mut k.record {
            RawEntity::Simple { attributes, .. } => {
                for a in attributes {
                    replace(a, k.definition, &Attribute::EntityRef(new));
                    replace(a, k.model, &model.to_ref());
                }
            }
            RawEntity::Complex { parts, .. } => {
                for part in parts {
                    for a in &mut part.attributes {
                        replace(a, k.definition, &Attribute::EntityRef(new));
                        replace(a, k.model, &model.to_ref());
                    }
                }
            }
        }
        edit.add(k.record);
        added += 2;
    }
    Ok((
        doc.apply(&edit).map_err(|e| Error(e.to_string()))?.bytes,
        added,
    ))
}
