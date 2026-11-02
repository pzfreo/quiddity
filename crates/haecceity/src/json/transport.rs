//! AP242 transport metadata for order and alias bindings. Values remain in native PMI.
//! Every binding is checked against independently decoded native semantic content.
use super::*;
use crate::pmi::{PartId, PmiRead};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: String,
    key: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    references: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    sequences: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    anchors: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    callouts: Option<Value>,
}
type Entries = BTreeMap<String, Vec<Entry>>;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Layout {
    #[serde(rename = "v", default = "legacy_version")]
    version: u8,
    items: Entries,
    /// Source identifier observations, distinct from native unique wire aliases.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    identifiers: BTreeMap<String, Vec<Option<String>>>,
    #[serde(default = "units::empty", skip_serializing_if = "empty_units")]
    units: Value,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    spellings: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    wire_anchors: Vec<AnchorBinding>,
}
impl std::ops::Deref for Layout {
    type Target = Entries;
    fn deref(&self) -> &Entries {
        &self.items
    }
}

fn legacy_version() -> u8 {
    1
}
fn empty_units(value: &Value) -> bool {
    ["length", "angle", "density", "other"]
        .iter()
        .all(|k| value[*k].as_array().is_some_and(Vec::is_empty))
}
impl Layout {
    fn entries(&self, collection: &str) -> &[Entry] {
        self.items
            .get(collection)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
}
/// Preserve tokens lost by REAL encoding or exact unit-factor arithmetic.
pub(super) fn walk_reals(
    raw: &mut Value,
    f: &mut impl FnMut(&str, &mut Value, bool) -> Result<()>,
) -> Result<()> {
    schema::walk_path(
        raw,
        &model_schema(),
        pmi_schema(),
        "",
        &mut |kind, value, path| {
            if kind == "Decimal" {
                f(path, value, false)?;
            }
            if kind == "DensityUnit" {
                for (i, pair) in value.as_array_mut().into_iter().flatten().enumerate() {
                    f(&format!("{path}/{i}/1"), &mut pair[1], false)?;
                }
            }
            for (unit_kind, factor) in [("LengthUnit", "metres"), ("AngleUnit", "radians")] {
                if kind == unit_kind
                    && let Some(scale) = value.pointer_mut(&format!("/other/{factor}"))
                {
                    f(&format!("{path}/other/{factor}"), scale, true)?;
                }
            }
            Ok(())
        },
    )
}
fn normalize_reals(raw: &mut Value, normalize_units: bool) -> Result<BTreeMap<String, String>> {
    let mut spellings = BTreeMap::new();
    walk_reals(raw, &mut |path, value, unit_scale| {
        if unit_scale && !normalize_units {
            return Ok(());
        }
        let stated = value
            .as_str()
            .ok_or_else(|| Error("invalid decimal".into()))?;
        if path.ends_with("/count") && stated.parse::<i64>().is_ok_and(|n| n.to_string() == stated)
        {
            return Ok(());
        }
        let decimal = pmi::Decimal::parse(stated).map_err(|e| Error(e.0))?;
        let encoded = pmi::write::real_text(&if unit_scale {
            decimal.scaled(0)
        } else {
            decimal
        })
        .map_err(Error)?;
        if encoded != stated {
            spellings.insert(path.to_string(), stated.to_string());
            *value = json!(encoded);
        }
        Ok(())
    })?;
    Ok(spellings)
}

fn key(raw: &Value, collection: &str, index: usize) -> Result<String> {
    let mut value = identity::content(
        raw,
        collection,
        index,
        &Anchors::default(),
        &mut BTreeSet::new(),
    )?;
    fn sort_set(array: &mut Vec<Value>) -> Result<()> {
        let mut values = std::mem::take(array)
            .into_iter()
            .map(|v| identity::canonical(&v).map(|key| (key, v)))
            .collect::<Result<Vec<_>>>()?;
        values.sort_by(|a, b| a.0.cmp(&b.0));
        values.dedup_by(|a, b| a.0 == b.0);
        *array = values.into_iter().map(|(_, v)| v).collect();
        Ok(())
    }
    fn sets(value: &mut Value) -> Result<()> {
        match value {
            Value::Object(object) => {
                for child in object.values_mut() {
                    sets(child)?;
                }
                if object.len() == 1 {
                    if let Some(array) = object.get_mut("items").and_then(Value::as_array_mut)
                        && array.iter().all(|a| {
                            a.as_object().is_some_and(|o| {
                                o.len() == 1
                                    && ["face", "edge", "geometry"]
                                        .iter()
                                        .any(|k| o.contains_key(*k))
                            })
                        })
                    {
                        sort_set(array)?;
                    }
                    for (variant, field) in [("group", "members"), ("derived", "from")] {
                        if let Some(array) = object
                            .get_mut(variant)
                            .and_then(|v| v.get_mut(field))
                            .and_then(Value::as_array_mut)
                        {
                            sort_set(array)?;
                        }
                    }
                }
            }
            Value::Array(array) => {
                for child in array {
                    sets(child)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    // Only structural support sets are normalized, including their occurrences
    // inside semantic requirements. UDA cell order and semantic values stay exact.
    sets(&mut value)?;
    Ok(identity::digest(&identity::canonical(&value)?))
}
fn collect(
    value: &Value,
    path: &str,
    out: &mut BTreeMap<String, Value>,
    sequences: &mut BTreeMap<String, Value>,
) {
    if let Some(reference) = value.get("$reference") {
        out.insert(path.into(), reference.clone());
        return;
    }
    match value {
        Value::Object(o) => {
            for (k, v) in o {
                collect(
                    v,
                    &format!("{path}/{}", k.replace('~', "~0").replace('/', "~1")),
                    out,
                    sequences,
                );
            }
        }
        Value::Array(a) => {
            if !a.is_empty() && a.iter().all(|v| v.get("$reference").is_some()) {
                sequences.insert(path.into(), value.clone());
            }
            for (i, v) in a.iter().enumerate() {
                collect(v, &format!("{path}/{i}"), out, sequences);
            }
        }
        _ => {}
    }
}
pub(crate) fn layout(pmi: &PartPmi, ids: &BTreeMap<String, Vec<String>>) -> Result<String> {
    let mut raw = serde_json::to_value(pmi)?;
    let spellings = normalize_reals(&mut raw, true)?;
    let mut linked = raw.clone();
    schema::walk(
        &mut linked,
        &model_schema(),
        pmi_schema(),
        &mut |kind, value| {
            if let Some(c) = reference_collection(kind) {
                let i = value
                    .as_u64()
                    .ok_or_else(|| Error(format!("invalid {kind}")))?
                    as usize;
                *value = json!({"$reference":[COLLECTIONS.iter().position(|&name| name == c),i]});
            }
            Ok(())
        },
    )?;
    let mut layout = Entries::new();
    for &c in COLLECTIONS {
        let mut entries = vec![];
        for (i, id) in ids[c].iter().enumerate() {
            let mut references = BTreeMap::new();
            let mut sequences = BTreeMap::new();
            collect(&linked[c][i], "", &mut references, &mut sequences);
            entries.push(Entry {
                id: id.clone(),
                key: key(&raw, c, i)?,
                references,
                sequences,
                callouts: if c == "notes"
                    && linked[c][i]["callouts"]
                        .as_array()
                        .is_some_and(|a| !a.is_empty())
                {
                    Some(linked[c][i]["callouts"].clone())
                } else {
                    None
                },
                anchors: if c == "features" {
                    linked[c][i]
                        .get("items")
                        .filter(|a| {
                            // A singleton native face/edge resolves its entire wire support.
                            !a.as_array().is_some_and(|a| {
                                a.len() == 1
                                    && (a[0]["face"].is_number() || a[0]["edge"].is_number())
                            })
                        })
                        .cloned()
                } else {
                    None
                },
            });
        }
        if !entries.is_empty() {
            layout.insert(c.into(), entries);
        }
    }
    let units = ids
        .get("__units")
        .and_then(|v| v.first())
        .map(|s| serde_json::from_str(s))
        .transpose()?
        .unwrap_or_else(super::units::empty);
    Ok(serde_json::to_string(&Layout {
        version: 2,
        items: layout,
        identifiers: ids
            .get("__identifiers")
            .and_then(|v| v.first())
            .map(|s| serde_json::from_str(s))
            .transpose()?
            .unwrap_or_default(),
        units,
        spellings,
        wire_anchors: ids
            .get("__anchors")
            .and_then(|v| v.first())
            .map(|s| serde_json::from_str(s))
            .transpose()?
            .unwrap_or_default(),
    })?)
}
fn ids_mut<'a>(ids: &'a mut PartIdentifiers, c: &str) -> &'a mut Vec<Option<String>> {
    match c {
        "features" => &mut ids.features,
        "geometry" => &mut ids.geometry,
        "derived_geometry" => &mut ids.derived_geometry,
        "datum_targets" => &mut ids.datum_targets,
        "datums" => &mut ids.datums,
        "dimensions" => &mut ids.dimensions,
        "tolerances" => &mut ids.tolerances,
        "tolerance_relations" => &mut ids.tolerance_relations,
        "general" => &mut ids.general,
        "threads" => &mut ids.threads,
        "knurls" => &mut ids.knurls,
        "notes" => &mut ids.notes,
        "surface_textures" => &mut ids.surface_textures,
        _ => &mut ids.attributes,
    }
}
fn provenance_mut<'a>(p: &'a mut pmi::PartProvenance, c: &str) -> Option<&'a mut Vec<Vec<u64>>> {
    Some(match c {
        "features" => &mut p.features,
        "geometry" => &mut p.geometry,
        "datum_targets" => &mut p.datum_targets,
        "datums" => &mut p.datums,
        "dimensions" => &mut p.dimensions,
        "tolerances" => &mut p.tolerances,
        "tolerance_relations" => &mut p.tolerance_relations,
        "general" => &mut p.general,
        "threads" => &mut p.threads,
        "knurls" => &mut p.knurls,
        "notes" => &mut p.notes,
        "surface_textures" => &mut p.surface_textures,
        "attributes" => &mut p.attributes,
        _ => return None,
    })
}
fn reference_binding<'a>(
    value: &'a Value,
    layout: &'a Layout,
) -> Result<(&'a str, usize, &'a str)> {
    let (collection, index) = if let Some(pair) = value.as_array() {
        let c = pair
            .first()
            .and_then(Value::as_u64)
            .and_then(|i| COLLECTIONS.get(i as usize))
            .ok_or_else(|| Error("invalid ordering reference collection".into()))?;
        let i = pair
            .get(1)
            .and_then(Value::as_u64)
            .ok_or_else(|| Error("invalid ordering reference index".into()))?
            as usize;
        if pair.len() != 2 {
            return Err(Error("invalid ordering reference pair".into()));
        }
        (*c, i)
    } else {
        // Read the verbose producer convention emitted before #142 round 3.
        let c = value["collection"]
            .as_str()
            .ok_or_else(|| Error("ordering binding missing collection".into()))?;
        let id = value["id"]
            .as_str()
            .ok_or_else(|| Error("ordering binding missing id".into()))?;
        let i = layout
            .entries(c)
            .iter()
            .position(|e| e.id == id)
            .ok_or_else(|| Error(format!("unknown ordering {c} id {id:?}")))?;
        (c, i)
    };
    let id = layout
        .entries(collection)
        .get(index)
        .ok_or_else(|| Error("ordering reference out of range".into()))?;
    Ok((collection, index, &id.id))
}
fn binding(value: &Value, layout: &Layout) -> Result<Value> {
    Ok(json!(reference_binding(value, layout)?.1))
}

fn unmark(value: &mut Value, layout: &Layout) -> Result<()> {
    if let Some(reference) = value.get("$reference") {
        *value = binding(reference, layout)?;
        return Ok(());
    }
    match value {
        Value::Object(o) => {
            for v in o.values_mut() {
                unmark(v, layout)?;
            }
        }
        Value::Array(a) => {
            for v in a {
                unmark(v, layout)?;
            }
        }
        _ => {}
    }
    Ok(())
}
pub(crate) fn restore(doc: &Document, read: &mut PmiRead) -> Result<()> {
    for part in 0..read.parts.len() {
        let items: Vec<_> = read.provenance.parts[part]
            .transport
            .iter()
            .filter_map(|id| {
                let name = match doc.get(*id)? {
                    crate::p21::RawEntity::Simple { attributes, .. } => attributes.first(),
                    _ => None,
                };
                (name == Some(&Attribute::String("ordering".into()))).then_some(*id)
            })
            .map(|id| crate::pmi::text::read(doc, id)
                .ok_or_else(|| Error(format!("invalid PMI ordering text at #{id}; expected a descriptive item or marked ordered text chunks"))))
            .collect::<Result<_>>()?;
        if items.is_empty() {
            continue;
        }
        if items.len() != 1 {
            return Err(Error("multiple PMI ordering records on a part".into()));
        }
        let layout: Layout = serde_json::from_value(parse(&items[0])?)?;
        if layout.keys().any(|c| !COLLECTIONS.contains(&c.as_str())) {
            return Err(Error("unknown PMI ordering collection".into()));
        }
        if ![1, 2].contains(&layout.version) {
            return Err(Error("unknown ordering convention version".into()));
        }
        for (c, identifiers) in &layout.identifiers {
            if !COLLECTIONS.contains(&c.as_str()) || identifiers.len() != layout.entries(c).len() {
                return Err(Error(format!(
                    "invalid source identifier observations for {c}"
                )));
            }
        }
        let mut seen = BTreeSet::new();
        super::units::validate(&layout.units, &mut seen)?;
        let mut raw = serde_json::to_value(&read.parts[part])?;
        normalize_reals(&mut raw, layout.version == 2)?;
        let mut restored = raw.clone();
        let mut maps = BTreeMap::<String, Vec<usize>>::new();
        for &c in COLLECTIONS {
            let old = raw[c]
                .as_array()
                .ok_or_else(|| Error(format!("missing {c}")))?;
            let mut values = vec![];
            let mut order = vec![];
            for e in layout.entries(c) {
                if e.id.is_empty() || !seen.insert(e.id.clone()) {
                    return Err(Error(format!("duplicate ordering id {:?}", e.id)));
                }
                let index = if ["features", "geometry", "derived_geometry"].contains(&c) {
                    (0..old.len()).find(|&i| key(&raw, c, i).is_ok_and(|k| k == e.key))
                } else {
                    file_ids(Some(&read.identifiers.parts[part]), c)
                        .iter()
                        .position(|id| id.as_deref() == Some(&e.id))
                };
                let i = index.ok_or_else(|| {
                    Error(format!("ordering id {:?} not found in native {c}", e.id))
                })?;
                if key(&raw, c, i)? != e.key {
                    return Err(Error(format!(
                        "ordering id {:?}: native content changed in {c}",
                        e.id
                    )));
                }
                // Verify independently identified reference targets before restoring any
                // projection/order bindings. Equal-valued twins still have distinct ids.
                for (path, reference) in &e.references {
                    let (target, _, expected) = reference_binding(reference, &layout)?;
                    if ["features", "geometry", "derived_geometry"].contains(&target) {
                        continue;
                    }
                    let index = old[i]
                        .pointer(path)
                        .and_then(Value::as_u64)
                        .ok_or_else(|| {
                            Error(format!(
                                "ordering id {:?}: native reference {path} is absent",
                                e.id
                            ))
                        })? as usize;
                    let actual = file_ids(Some(&read.identifiers.parts[part]), target)
                        .get(index)
                        .and_then(Option::as_deref);
                    if actual != Some(expected) {
                        return Err(Error(format!(
                            "ordering id {:?}: native reference {path} selects {target} id {actual:?}, expected {}",
                            e.id, expected
                        )));
                    }
                }
                let mut value = old[i].clone();
                if let Some(anchors) = &e.anchors {
                    value["items"] = anchors.clone();
                    unmark(&mut value["items"], &layout)?;
                }
                if c == "notes" {
                    value["callouts"] = e.callouts.clone().unwrap_or_else(|| json!([]));
                    unmark(&mut value["callouts"], &layout)?;
                }
                for (path, sequence) in &e.sequences {
                    let target = value.pointer_mut(path).ok_or_else(|| {
                        Error(format!("ordering reference sequence {c}{path} is absent"))
                    })?;
                    *target = sequence.clone();
                    unmark(target, &layout)?;
                }
                for (path, reference) in &e.references {
                    let target = value.pointer_mut(path).ok_or_else(|| {
                        Error(format!("ordering reference path {c}{path} is absent"))
                    })?;
                    *target = binding(reference, &layout)?;
                }
                values.push(value);
                order.push(i);
            }
            if !["features", "geometry", "derived_geometry"].contains(&c) {
                for i in 0..old.len() {
                    if order.contains(&i) {
                        continue;
                    }
                    let precision_helper = c == "general"
                        && file_ids(Some(&read.identifiers.parts[part]), c)
                            .get(i)
                            .is_none_or(Option::is_none)
                        && crate::pmi::write::decimal_carrier(
                            &read.parts[part].general[i],
                            read.parts[part].decimal_places,
                        );
                    if !precision_helper {
                        return Err(Error(format!(
                            "ordering record omits native {c} item {i}; use pmi::read_native to recover stale metadata"
                        )));
                    }
                }
            }
            restored[c] = Value::Array(values);
            maps.insert(c.into(), order);
        }
        // Reference reindexing/aliases are checked against native, resolved semantic content.
        for &c in COLLECTIONS {
            for (i, e) in layout.entries(c).iter().enumerate() {
                if key(&restored, c, i)? != e.key {
                    return Err(Error(format!(
                        "ordering id {:?} native content changed in {c}",
                        e.id
                    )));
                }
            }
        }
        let mut decimal_paths = BTreeSet::new();
        walk_reals(&mut restored, &mut |path, _, _| {
            decimal_paths.insert(path.to_string());
            Ok(())
        })?;
        for (path, stated) in &layout.spellings {
            if !decimal_paths.contains(path) {
                return Err(Error(format!("spelling {path} is not a decimal")));
            }
            let target = restored
                .pointer_mut(path)
                .ok_or_else(|| Error(format!("stale decimal spelling {path}")))?;
            let stated_decimal = pmi::Decimal::parse(stated).map_err(|e| Error(e.0))?;
            let native_decimal = target
                .as_str()
                .and_then(|s| pmi::Decimal::parse(s).ok())
                .ok_or_else(|| Error("native decimal spelling is absent".into()))?;
            if stated_decimal.cmp_value(&native_decimal) != std::cmp::Ordering::Equal {
                return Err(Error(format!(
                    "decimal spelling {path} changes native value"
                )));
            }
            *target = json!(stated);
        }
        let pmi: PartPmi = serde_json::from_value(restored)?;
        pmi.validate()
            .map_err(|e| Error(format!("invalid ordered PMI: {e:?}")))?;
        let old_carriers = read.json_carriers.clone();
        for &c in COLLECTIONS {
            let order = &maps[c];
            if let Some(prov) = provenance_mut(&mut read.provenance.parts[part], c) {
                let old = prov.clone();
                *prov = order
                    .iter()
                    .map(|&i| {
                        // Legacy faceless endpoint aspects can duplicate a datum's
                        // geometric content. Ordering projects both to one feature;
                        // retain every consumed carrier/SAR so Replace can remove them.
                        if c == "features" {
                            let mut ids: Vec<_> = old
                                .iter()
                                .enumerate()
                                .filter(|(j, _)| raw[c][*j] == raw[c][i])
                                .flat_map(|(_, ids)| ids.iter().copied())
                                .collect();
                            ids.sort_unstable();
                            ids.dedup();
                            ids
                        } else {
                            old.get(i).cloned().unwrap_or_default()
                        }
                    })
                    .collect();
            }
            if c == "notes" {
                let old = read.provenance.parts[part].note_callouts.clone();
                read.provenance.parts[part].note_callouts = order
                    .iter()
                    .map(|&i| old.get(i).cloned().unwrap_or_default())
                    .collect();
            }
            *ids_mut(&mut read.identifiers.parts[part], c) = layout
                .entries(c)
                .iter()
                .map(|e| Some(e.id.clone()))
                .collect();
            read.json_carriers
                .retain(|(p, collection, _), _| *p != PartId(part) || collection != c);
            for (i, &old) in order.iter().enumerate() {
                if let Some(carrier) = old_carriers.get(&(PartId(part), c.into(), old)) {
                    read.json_carriers
                        .insert((PartId(part), c.into(), i), *carrier);
                }
            }
        }
        // Callout targets use the returned model's indices, including restored ordering
        // and aliases (one native feature can project to several caller feature ids).
        for callout in &mut read.callouts {
            for link in &mut callout.links {
                link.targets = link
                    .targets
                    .iter()
                    .flat_map(|target| {
                        if target.part != PartId(part) {
                            return vec![target.clone()];
                        }
                        let (collection, old) = target.item.collection_index();
                        let Some(order) = maps.get(collection) else {
                            return vec![target.clone()];
                        };
                        order
                            .iter()
                            .enumerate()
                            .filter(|&(_, &native)| native == old)
                            .map(|(new, _)| crate::pmi::CalloutTarget {
                                part: target.part,
                                item: target.item.at_index(new),
                            })
                            .collect()
                    })
                    .collect();
                link.targets.sort();
                link.targets.dedup();
            }
        }
        read.identifiers.parts[part].json_identifiers =
            Some(serde_json::to_string(&layout.identifiers)?);
        read.identifiers.parts[part].json_units = Some(serde_json::to_string(&layout.units)?);
        read.identifiers.parts[part].json_anchors =
            Some(serde_json::to_string(&layout.wire_anchors)?);
        read.parts[part] = pmi;
    }
    Ok(())
}

pub(crate) fn has_units(ids: &BTreeMap<String, Vec<String>>) -> Result<bool> {
    let units: Value = ids
        .get("__units")
        .and_then(|v| v.first())
        .map(|s| serde_json::from_str(s))
        .transpose()?
        .unwrap_or_else(super::units::empty);
    Ok(["length", "angle", "density", "other"].iter().any(|kind| {
        units[*kind]
            .as_array()
            .is_some_and(|units| !units.is_empty())
    }))
}
