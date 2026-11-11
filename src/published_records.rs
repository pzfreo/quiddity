//! Self-contained publication of fields that cannot use the file's principal axes.
//!
//! Working records remain internal; consumers cannot accidentally read an unmerged typed family.
//! Consumers use `families()` or serialization for the published contract. Only records needing
//! an explicit measuring basis acquire coordinate fields; file-axis measuring fields retain
//! their representation. Merged families also expose their optional detail fields.
use std::collections::BTreeMap;
#[cfg(test)]
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, OnceLock};

use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use serde_json::{Value, json};

use crate::features::Features;
use crate::framed_records::Rigid;
use crate::kernel::geom::V3;

#[derive(Clone, Debug)]
pub struct PublishedFeatures {
    working: Features,
    records: OnceLock<BTreeMap<String, Value>>,
    pending: BTreeMap<String, Vec<String>>,
    source: Arc<Features>,
    motion: Rigid,
    reach: f64,
}

#[cfg(test)]
impl Deref for PublishedFeatures {
    type Target = Features;
    fn deref(&self) -> &Features {
        &self.working
    }
}
#[cfg(test)]
impl DerefMut for PublishedFeatures {
    fn deref_mut(&mut self) -> &mut Features {
        self.records.take();
        &mut self.working
    }
}

impl PublishedFeatures {
    pub(crate) fn measurement_source(&self) -> (&Features, &Rigid) {
        (&self.source, &self.motion)
    }

    pub(crate) fn working(&self) -> &Features {
        &self.working
    }

    pub(crate) fn new(
        working: Features,
        pending: BTreeMap<String, Vec<String>>,
        motion: Rigid,
        source: Arc<Features>,
        reach: f64,
    ) -> Self {
        Self {
            working,
            pending,
            records: OnceLock::new(),
            source,
            motion,
            reach,
        }
    }

    fn converted_records(&self, families: &[(String, Vec<Value>)]) -> BTreeMap<String, Value> {
        let mut records = BTreeMap::new();
        for (family, list) in families {
            for (n, record) in list.iter().enumerate() {
                let id = format!("{family}/{n}");
                let Some(paths) = self.pending.get(&id) else {
                    continue;
                };
                let mut record = record.clone();
                if family == "paired_ramp_steps" && paths.iter().any(|p| p == "half_widths") {
                    let i = axis_index(&json!(self.source.paired_ramp_steps[n].half_width_axis))
                        .expect("width axis");
                    let (d, sign) = direction(&self.motion, i, self.reach);
                    record["width_axis"] = d;
                    if sign < 0.
                        && let Some(pair) = record["half_widths"].as_array_mut()
                    {
                        pair.reverse();
                    }
                }
                convert(&mut record, paths, &self.motion, family, self.reach);
                records.insert(id, record);
            }
        }
        records
    }

    /// All document families, including refusals, in registry order. Each payload is usable
    /// without the document's provenance frame.
    pub fn families(&self) -> Vec<(String, Vec<Value>)> {
        let mut families = crate::recognition::working_families(&self.working);
        let records = self
            .records
            .get_or_init(|| self.converted_records(&families));
        for (family, list) in &mut families {
            for (n, record) in list.iter_mut().enumerate() {
                if let Some(published) = records.get(&format!("{family}/{n}")) {
                    *record = published.clone();
                }
            }
        }
        merge_families(&mut families);
        for (family, records) in &mut families {
            if family != "slot_patterns" {
                continue;
            }
            for record in records {
                for key in ["direction", "row_direction", "col_direction"] {
                    if record[key].is_array() {
                        let d =
                            serde_json::from_value(record[key].clone()).expect("pattern direction");
                        record[key] = json!(snap_direction(d, self.reach));
                    }
                }
            }
        }
        families
    }

    /// Read an exact intrinsic axis at fingerprint extraction, without constructing the
    /// serialized publication view. Already mapped vectors keep their established contract.
    pub(crate) fn fingerprint_axis(&self, id: &str, path: &str, value: &Value) -> Option<V3> {
        self.pending
            .get(id)?
            .iter()
            .any(|p| p == path)
            .then_some(())?;
        Some(column(&self.motion, axis_index(value)?))
    }
}

impl Serialize for PublishedFeatures {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Preserve the old top-level family coverage and order, including omitted empty threads.
        let raw = self.families();
        let mut map = serializer.serialize_map(None)?;
        for (family, mut list) in raw {
            if ["section_recess_patterns", "modelled_thread_refusals"].contains(&family.as_str()) {
                continue;
            }
            if family == "interior_voids" {
                list.truncate(self.working.interior_voids.len());
            }
            if family == "section_recesses" {
                list.truncate(self.working.section_recesses.len());
            }
            map.serialize_entry(&family, &list)?;
        }
        map.end()
    }
}

/// The publication namespace; recognition and proofs retain their source families.
pub(crate) fn publication_family(family: &str) -> &str {
    match family {
        "oriented_slots" => "slots",
        "oriented_chamfers" => "chamfers",
        "oriented_slot_patterns" => "slot_patterns",
        _ => family,
    }
}

impl PublishedFeatures {
    /// Translate a working record ID once, appending oriented records after plain records.
    pub(crate) fn published_id(&self, family: &str, n: usize) -> String {
        let offset = match family {
            "oriented_slots" => self.working.slots.len(),
            "oriented_chamfers" => self.working.chamfers.len(),
            "oriented_slot_patterns" => self.working.slot_patterns.len(),
            _ => 0,
        };
        format!("{}/{}", publication_family(family), offset + n)
    }

    pub(crate) fn working_record<'a>(&self, family: &'a str, n: usize) -> (&'a str, usize) {
        let (oriented, count) = match family {
            "slots" => ("oriented_slots", self.working.slots.len()),
            "chamfers" => ("oriented_chamfers", self.working.chamfers.len()),
            "slot_patterns" => ("oriented_slot_patterns", self.working.slot_patterns.len()),
            _ => return (family, n),
        };
        if n < count {
            (family, n)
        } else {
            (oriented, n - count)
        }
    }
}

fn merge_families(families: &mut Vec<(String, Vec<Value>)>) {
    for (old, new) in [
        ("oriented_slots", "slots"),
        ("oriented_chamfers", "chamfers"),
        ("oriented_slot_patterns", "slot_patterns"),
    ] {
        let index = families
            .iter()
            .position(|(f, _)| f == old)
            .expect("working family");
        let (_, mut oriented) = families.remove(index);
        let (_, plain) = families
            .iter_mut()
            .find(|(f, _)| f == new)
            .expect("plain family");
        for record in plain.iter_mut() {
            match new {
                "slots" => {
                    record["source"] = Value::Null;
                }
                "chamfers" => {
                    for key in [
                        "leg1_direction",
                        "leg2_direction",
                        "length",
                        "support_spans",
                        "body_key",
                    ] {
                        record[key] = Value::Null;
                    }
                }
                "slot_patterns" => {
                    for member in record["slots"].as_array_mut().expect("slot members") {
                        member["source"] = Value::Null;
                    }
                }
                _ => unreachable!(),
            }
        }
        for record in &mut oriented {
            match new {
                "slots" => merge_slot(record),
                "chamfers" => {
                    let obj = record.as_object_mut().expect("chamfer");
                    let run = obj.remove("run").expect("run");
                    obj.insert("axis".into(), json!(measuring_axis(&run)));
                    obj.insert("turned".into(), json!(false));
                }
                "slot_patterns" => {
                    for member in record["slots"].as_array_mut().expect("slot members") {
                        merge_slot(member);
                    }
                }
                _ => unreachable!(),
            }
        }
        plain.extend(oriented);
        if new == "slot_patterns" {
            for pattern in plain {
                grid_directions(pattern);
            }
        }
    }
}

fn grid_directions(pattern: &mut Value) {
    let Some(angle) = pattern["angle"].as_f64() else {
        return;
    };
    let first = &pattern["slots"][0];
    let normal: V3 = if first["depth_axis"].is_null() {
        let w = axis_index(&first["width_axis"]).expect("width axis");
        let l = axis_index(&first["long_axis"]).expect("long axis");
        std::array::from_fn(|i| if i == 3 - w - l { 1. } else { 0. })
    } else if let Some(i) = axis_index(&first["depth_axis"]) {
        std::array::from_fn(|j| if i == j { 1. } else { 0. })
    } else {
        serde_json::from_value(first["depth_axis"].clone()).expect("depth vector")
    };
    let (u, v) = crate::features::pattern_geometry::plane_uv(normal);
    let (s, c) = angle.to_radians().sin_cos();
    pattern["col_direction"] = json!((0..3).map(|i| c * u[i] + s * v[i]).collect::<Vec<_>>());
    pattern["row_direction"] = json!((0..3).map(|i| -s * u[i] + c * v[i]).collect::<Vec<_>>());
}

/// Existing explicit directions may have the recogniser's six-decimal precision. When
/// promoted into the common measurement-axis contract they must be unit length; detailed
/// source geometry and other existing vector fields keep their original precision.
fn measuring_axis(value: &Value) -> V3 {
    let d: V3 = serde_json::from_value(value.clone()).expect("measuring direction");
    crate::kernel::geom::unit(d).expect("issued nonzero measuring direction")
}

/// Project the already proved profile into the slot's measuring basis. No geometry is
/// recognised here; the detailed passage and its original run stations remain intact.
fn merge_slot(record: &mut Value) {
    let source = record["source"].clone();
    let vector = |v: &Value| -> V3 { serde_json::from_value(v.clone()).expect("vector") };
    let u = vector(&source["frame"]["u"]);
    let v = vector(&source["frame"]["v"]);
    let w = measuring_axis(&record["width_direction"]);
    let l = measuring_axis(&record["long_direction"]);
    let project = |axis: V3| -> (f64, f64) {
        source["section"]["boundary"]
            .as_array()
            .expect("profile")
            .iter()
            .map(|vertex| {
                let a = vertex["point"][0].as_f64().expect("coordinate");
                let b = vertex["point"][1].as_f64().expect("coordinate");
                (0..3).map(|i| (a * u[i] + b * v[i]) * axis[i]).sum::<f64>()
            })
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| {
                (lo.min(x), hi.max(x))
            })
    };
    let (w_lo, w_hi) = project(w);
    let (lo, hi) = project(l);
    let obj = record.as_object_mut().expect("slot");
    obj.remove("width_direction");
    obj.remove("long_direction");
    obj.remove("center");
    obj.insert("width_axis".into(), json!(w));
    obj.insert("long_axis".into(), json!(l));
    obj.insert(
        "depth_axis".into(),
        json!(measuring_axis(&source["frame"]["run"])),
    );
    obj.insert("origin".into(), source["frame"]["origin"].clone());
    obj.insert("w_center".into(), json!((w_lo + w_hi) / 2.));
    obj.insert("lo".into(), json!(lo));
    obj.insert("hi".into(), json!(hi));
    obj.insert("d_lo".into(), source["run_interval"][0].clone());
    obj.insert("d_hi".into(), source["run_interval"][1].clone());
    obj.insert("end_radius".into(), Value::Null);
    obj.insert("corner_radius".into(), Value::Null);
}

/// Input to a subsequent rigid mapping. Retaining the recognition source lets motions compose
/// without decoding vector records back into frame letters or applying publication rounding twice.
pub struct MappingSource {
    pub(crate) features: Features,
    pub(crate) motion: Rigid,
    pub(crate) reach: f64,
}
const IDENTITY: Rigid = Rigid {
    rotation: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
    translation: [0.; 3],
};
impl From<Features> for MappingSource {
    fn from(features: Features) -> Self {
        Self {
            features,
            motion: IDENTITY,
            reach: f64::INFINITY,
        }
    }
}
impl From<PublishedFeatures> for MappingSource {
    fn from(features: PublishedFeatures) -> Self {
        Self {
            features: (*features.source).clone(),
            motion: features.motion,
            reach: features.reach,
        }
    }
}
impl From<Features> for PublishedFeatures {
    fn from(features: Features) -> Self {
        Self::new(
            features.clone(),
            BTreeMap::new(),
            IDENTITY,
            Arc::new(features),
            f64::INFINITY,
        )
    }
}

fn axis_index(v: &Value) -> Option<usize> {
    match v.as_str()? {
        "x" => Some(0),
        "y" => Some(1),
        "z" => Some(2),
        _ => None,
    }
}
fn column(m: &Rigid, i: usize) -> V3 {
    std::array::from_fn(|w| m.rotation[w][i])
}

/// A signed measuring vector becomes a positive file-axis letter only when exactly aligned
/// after #70's reach-bounded snap. Its coordinates are multiplied by the returned sign.
fn direction(m: &Rigid, i: usize, reach: f64) -> (Value, f64) {
    let d = column(m, i);
    let w = (0..3)
        .max_by(|&a, &b| d[a].abs().total_cmp(&d[b].abs()))
        .unwrap();
    let sign = 1_f64.copysign(d[w]);
    let squared_deviation = |d: V3| {
        let w = (0..3)
            .max_by(|&a, &b| d[a].abs().total_cmp(&d[b].abs()))
            .unwrap();
        let sign = 1_f64.copysign(d[w]);
        (0..3)
            .map(|k| (d[k] - if k == w { sign } else { 0. }).powi(2))
            .sum::<f64>()
    };
    let deviation = squared_deviation(d);
    // Bound the combined change of every snapped column, as #70 bounds the whole
    // rotation. Three individually small snaps must not together exceed the coordinate floor.
    let combined = (0..3)
        .map(|j| squared_deviation(column(m, j)))
        .filter(|&v| v == 0. || v.sqrt() * reach <= crate::kernel::geom::COORD_FLOOR)
        .sum::<f64>()
        .sqrt();
    if deviation == 0.
        || (deviation.sqrt() * reach <= crate::kernel::geom::COORD_FLOOR
            && combined * reach <= crate::kernel::geom::COORD_FLOOR)
    {
        (json!(["x", "y", "z"][w]), sign)
    } else {
        (json!(d), 1.)
    }
}
/// An explicit direction snapped only when its movement over the part is within
/// the same coordinate floor used for measuring bases. Preserve vector sense.
fn snap_direction(d: V3, reach: f64) -> V3 {
    let w = (0..3)
        .max_by(|&a, &b| d[a].abs().total_cmp(&d[b].abs()))
        .unwrap();
    let axis = std::array::from_fn(|i| if i == w { 1_f64.copysign(d[w]) } else { 0. });
    let deviation = (0..3).map(|i| (d[i] - axis[i]).powi(2)).sum::<f64>().sqrt();
    if deviation == 0. || deviation * reach <= crate::kernel::geom::COORD_FLOOR {
        axis
    } else {
        d
    }
}

fn scaled(v: &mut Value, sign: f64, interval: bool) {
    if sign == 1. {
        return;
    }
    match v {
        Value::Number(n) => *v = json!(-n.as_f64().expect("coordinate")),
        Value::Array(a) => {
            for v in a.iter_mut() {
                scaled(v, sign, false);
            }
            if interval {
                a.reverse();
            }
        }
        Value::Null => {}
        _ => panic!("invalid coordinate {v}"),
    }
}

fn convert(record: &mut Value, paths: &[String], m: &Rigid, family: &str, reach: f64) {
    // Walk nested members with the same contract; no record asks its reader for a part frame.
    let mut nested: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut direct = Vec::new();
    for path in paths {
        if let Some((head, tail)) = path.split_once('.') {
            nested.entry(head.into()).or_default().push(tail.into());
        } else {
            direct.push(path.as_str());
        }
    }
    for (head, tails) in nested {
        if let Some(name) = head.strip_suffix("[]") {
            if let Some(members) = record.get_mut(name).and_then(Value::as_array_mut) {
                for member in members {
                    convert(
                        member,
                        &tails,
                        m,
                        if name == "body_levels" {
                            "step_levels"
                        } else {
                            name
                        },
                        reach,
                    );
                }
            }
        } else if let Some(member) = record.get_mut(&head) {
            convert(member, &tails, m, &head, reach);
        }
    }
    if direct.is_empty() {
        return;
    }
    let raw = record.clone();
    let obj = record.as_object_mut().expect("record object");
    let has = |p: &str| direct.contains(&p);
    let axis = raw.get("axis").and_then(axis_index);
    let width = raw.get("width_axis").and_then(axis_index);
    let long = raw.get("long_axis").and_then(axis_index);
    let depth = raw
        .get("depth_axis")
        .and_then(axis_index)
        .or_else(|| width.zip(long).map(|(w, l)| 3 - w - l));
    let mut uses_origin = false;
    for &key in &direct {
        assert!(
            [
                "axis",
                "width_axis",
                "long_axis",
                "depth_axis",
                "thickness_axis",
                "other_axis",
                "lo",
                "hi",
                "base",
                "top",
                "span",
                "run_interval",
                "thickness_bounds",
                "w_center",
                "d_lo",
                "d_hi",
                "z",
                "z_lo",
                "z_hi",
                "x_span",
                "y_span",
                "x0",
                "x1",
                "y0",
                "y1",
                "z0",
                "z1",
                "positions",
                "other_positions",
                "body_key",
                "body_bounds",
                "axis_origin",
                "axis_line",
                "supports",
                "directions",
                "legs",
                "mirror_plane",
                "section",
                "endpoint_scopes",
                "half_widths",
                "open_sign",
                "depth_sign",
                "lo_at_envelope",
                "hi_at_envelope",
                "u",
                "v",
                "direction",
                "row_direction",
                "col_direction",
                "center"
            ]
            .contains(&key),
            "{family}.{key}: missing publication conversion"
        );
        if let Some(v) = raw.get(key)
            && let Some(i) = axis_index(v)
        {
            obj.insert(key.into(), direction(m, i, reach).0);
        }
        let coordinate_axis = match key {
            "lo" | "hi" | "base" | "top" | "span" | "run_interval" | "thickness_bounds" => long
                .or(axis)
                .or_else(|| raw.get("thickness_axis").and_then(axis_index)),
            "w_center" => width,
            "d_lo" | "d_hi" => depth,
            "z" | "z_lo" | "z_hi" => Some(2),
            "x_span" | "x0" | "x1" => Some(0),
            "y_span" | "y0" | "y1" => Some(1),
            "z0" | "z1" => Some(2),
            "positions" => axis,
            "other_positions" => raw.get("other_axis").and_then(axis_index),
            _ => None,
        };
        if let Some(i) = coordinate_axis {
            uses_origin = true;
            if let Some(v) = obj.get_mut(key) {
                scaled(
                    v,
                    direction(m, i, reach).1,
                    [
                        "span",
                        "run_interval",
                        "thickness_bounds",
                        "x_span",
                        "y_span",
                        "positions",
                        "other_positions",
                    ]
                    .contains(&key),
                );
            }
        }
        if ["body_key", "body_bounds"].contains(&key)
            && let Some(bounds) = raw.get(key).and_then(Value::as_array)
        {
            let interleaved = key == "body_bounds";
            let intervals: Vec<Value> = (0..3)
                .map(|i| {
                    let (a, b) = if interleaved {
                        (2 * i, 2 * i + 1)
                    } else {
                        (i, i + 3)
                    };
                    let mut pair = json!([bounds[a], bounds[b]]);
                    scaled(&mut pair, direction(m, i, reach).1, true);
                    pair
                })
                .collect();
            let mut box_value = json!({"origin":m.translation,"axes":(0..3).map(|i| direction(m,i,reach).0).collect::<Vec<_>>(),"bounds":intervals});
            if key == "body_key" {
                box_value["volume"] = bounds[6].clone();
                box_value["area"] = bounds[7].clone();
            }
            obj.insert(key.into(), box_value);
        }
        if key == "axis_origin"
            && let Some(p) = raw.get(key).and_then(Value::as_array)
        {
            let p = std::array::from_fn(|i| p[i].as_f64().expect("point"));
            obj.insert(key.into(), json!(m.point(p)));
        }
    }
    if has("supports") {
        uses_origin = true;
        let mut supports = raw["supports"].as_array().expect("two supports").clone();
        let mut signs = Vec::new();
        for support in &mut supports {
            let pair = support.as_array_mut().expect("support plane");
            let i = axis_index(&pair[0]).expect("support axis");
            let (d, s) = direction(m, i, reach);
            pair[0] = d;
            scaled(&mut pair[1], s, false);
            signs.push(s);
        }
        obj.insert("supports".into(), json!(supports));
        if let Some(directions) = obj.get_mut("directions").and_then(Value::as_array_mut) {
            for (v, s) in directions.iter_mut().zip(signs) {
                scaled(v, s, false);
            }
        }
    }
    if has("mirror_plane") {
        uses_origin = true;
        let mut pair = raw["mirror_plane"]
            .as_array()
            .expect("mirror plane")
            .clone();
        let i = axis_index(&pair[0]).expect("mirror axis");
        let (d, s) = direction(m, i, reach);
        pair[0] = d;
        scaled(&mut pair[1], s, false);
        obj.insert("mirror_plane".into(), json!(pair));
    }
    for key in ["direction", "row_direction", "col_direction", "center"] {
        if has(key)
            && let Some(p) = raw.get(key).and_then(Value::as_array)
            && p.len() == 3
        {
            let v = std::array::from_fn(|i| p[i].as_f64().expect("vector"));
            obj.insert(
                key.into(),
                json!(if key == "center" {
                    m.point(v)
                } else {
                    m.direction(v)
                }),
            );
        }
    }
    // Order interval endpoints and associated end flags in the published vector's sense.
    for (lo, hi, i) in [
        ("lo", "hi", long.or(axis)),
        ("base", "top", axis),
        ("d_lo", "d_hi", depth),
        ("z_lo", "z_hi", Some(2)),
        ("x0", "x1", Some(0)),
        ("y0", "y1", Some(1)),
        ("z0", "z1", Some(2)),
    ] {
        if has(lo) && has(hi) && i.is_some_and(|i| direction(m, i, reach).1 < 0.) {
            let a = obj[lo].clone();
            let b = obj[hi].clone();
            obj.insert(lo.into(), b);
            obj.insert(hi.into(), a);
            if lo == "z_lo" {
                let a = obj["lo_at_envelope"].clone();
                let b = obj["hi_at_envelope"].clone();
                obj.insert("lo_at_envelope".into(), b);
                obj.insert("hi_at_envelope".into(), a);
                if let Some(levels) = obj.get_mut("body_levels").and_then(Value::as_array_mut) {
                    levels.reverse();
                }
            }
        }
    }
    for (key, i) in [
        ("open_sign", depth.or(axis)),
        ("depth_sign", depth),
        ("direction", axis),
    ] {
        if has(key)
            && let Some(i) = i
            && let Some(v) = obj.get_mut(key)
        {
            scaled(v, direction(m, i, reach).1, false);
        }
    }
    if uses_origin {
        obj.insert("origin".into(), json!(m.translation));
    }
    if width.is_some() && long.is_some() && has("d_lo") {
        obj.insert("depth_axis".into(), direction(m, depth.unwrap(), reach).0);
    }
    if family == "step_levels" || family == "risers" {
        if has("z") || has("z_lo") {
            obj.insert("height_axis".into(), direction(m, 2, reach).0);
        }
        if has("x_span") {
            obj.insert("x_axis".into(), direction(m, 0, reach).0);
        }
        if has("y_span") {
            obj.insert("y_axis".into(), direction(m, 1, reach).0);
        }
    }
    if let Some(i) = axis {
        let across: Vec<_> = (0..3).filter(|&k| k != i).collect();
        if has("section") || has("axis_line") || has("u") || has("v") {
            for key in ["section", "axis_line"] {
                if has(key)
                    && let Some(v) = obj.get_mut(key)
                {
                    across_coordinates(
                        v,
                        [
                            direction(m, across[0], reach).1,
                            direction(m, across[1], reach).1,
                        ],
                    );
                }
            }
            obj.insert(
                "section_axes".into(),
                json!(
                    across
                        .iter()
                        .map(|&k| direction(m, k, reach).0)
                        .collect::<Vec<_>>()
                ),
            );
            obj.insert("origin".into(), json!(m.translation));
            for (key, k) in [("u", across[0]), ("v", across[1])] {
                if has(key)
                    && let Some(v) = obj.get_mut(key)
                {
                    scaled(v, direction(m, k, reach).1, false);
                }
            }
        }
    }
    if ["pads"].contains(&family) {
        obj.insert(
            "basis_axes".into(),
            json!((0..3).map(|i| direction(m, i, reach).0).collect::<Vec<_>>()),
        );
    }
}

// Re-express section points and traversal sense when a snapped basis direction reverses.
fn across_coordinates(v: &mut Value, signs: [f64; 2]) {
    match v {
        Value::Array(a) if a.len() == 2 && a.iter().all(Value::is_number) => {
            for (v, s) in a.iter_mut().zip(signs) {
                scaled(v, s, false);
            }
        }
        Value::Array(a) => {
            for v in a {
                across_coordinates(v, signs);
            }
        }
        Value::Object(o) => {
            for (k, v) in o {
                if ["sweep", "bulge"].contains(&k.as_str()) {
                    scaled(v, signs[0] * signs[1], false);
                } else {
                    across_coordinates(v, signs);
                }
            }
        }
        _ => {}
    }
}
