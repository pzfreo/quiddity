//! The AP242 semantic PMI reader (design: Architecture item 4).
//!
//! It walks the Part 21 records inversely from each part: the roots of a part's PMI are the
//! instances that reference its `product_definition_shape` (shape aspects by `of_shape`,
//! tolerances on the whole part, property definitions) or its `product_definition` (property
//! constructs, the dimensioning standard). From the shape aspects it follows usages to faces
//! and edges (resolved to the part's indices), and inverse references to what is built on them:
//! datums on datum features and targets, dimensions, tolerances, zones, relationships,
//! properties. Every item records the instance ids it consumed ([`Provenance`]).
//!
//! Accounting: every semantic-PMI-family instance of the file is consumed by a part or named
//! by a [`Finding`]; what no part reaches is classified (assembly PMI, occurrence PMI, or
//! unconsumed) at the end. Nothing is guessed: a value whose unit cannot be resolved, a
//! reference that does not resolve, a construct in a form the reader does not know, is a
//! finding and the item it belongs to is not read.
//!
//! Values keep the REAL token's text from the file ([`Decimal`]), read from the instance's
//! bytes, and their own unit entity's unit.

mod derived;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::model::*;
use super::standards::Iso2768;
use super::{
    Accounting, CalloutLink, CalloutTarget, Finding, FindingKind, GraphicalCallout, Identifiers,
    PartIdentifiers, PartProvenance, PmiRead, PresentedItem, Provenance, ReadError,
};
use crate::express::{self, Edition, Family};
use crate::p21::{Attribute, Document, RawEntity, visit_attributes};
use crate::step::PartDefinition;

/// Reads the semantic PMI of every part of `doc`. `parts` are `read_part_definitions` of the
/// same bytes.
pub fn read(doc: &Document, parts: &[PartDefinition]) -> Result<PmiRead, ReadError> {
    read_impl(
        doc,
        parts,
        #[cfg(feature = "json-import")]
        true,
    )
}

/// Read authoritative native AP242 values and ids without applying JSON transport
/// order/aliases. Useful for auditing or recovering stale ordering metadata.
#[cfg(feature = "json-import")]
pub fn read_native(doc: &Document, parts: &[PartDefinition]) -> Result<PmiRead, ReadError> {
    read_impl(doc, parts, false)
}

fn read_impl(
    doc: &Document,
    parts: &[PartDefinition],
    #[cfg(feature = "json-import")] restore: bool,
) -> Result<PmiRead, ReadError> {
    for p in parts {
        for id in [p.product_definition, p.shape] {
            if doc.get(id).is_none() {
                return Err(ReadError::Parts(format!(
                    "part {} names #{id}, which the document does not have",
                    p.name
                )));
            }
        }
    }
    let mut r = Reader::new(doc, parts);
    let mut out_parts = Vec::with_capacity(parts.len());
    let mut provenance = Provenance::default();
    let mut identifiers = Identifiers::default();
    for (i, part) in parts.iter().enumerate() {
        let mut st = PartState::new(PartId(i), part);
        r.read_part(&mut st);
        super::text::extend_provenance(doc, &mut st.prov);
        r.callout_aliases(&st);
        let PartState {
            pmi,
            prov,
            identifiers: part_identifiers,
            ..
        } = st;
        out_parts.push(pmi);
        provenance.parts.push(prov);
        identifiers.parts.push(part_identifiers);
    }
    r.check_identifiers();
    let accounting = r.account(&provenance);
    let callouts = r.read_callouts();
    let saved_views = r.read_saved_views();
    let mut findings = std::mem::take(&mut r.findings);
    findings.sort();
    findings.dedup();
    #[allow(unused_mut)]
    let mut result = PmiRead {
        parts: out_parts,
        findings,
        provenance,
        identifiers,
        accounting,
        callouts,
        saved_views,
        #[cfg(feature = "json")]
        json_carriers: r.json_carriers,
    };
    #[cfg(feature = "json-import")]
    if restore {
        crate::json::transport::restore(doc, &mut result)
            .map_err(|e| ReadError::Transport(e.to_string()))?;
    }
    Ok(result)
}

// ---------------------------------------------------------------------------------------------
// Instance access
// ---------------------------------------------------------------------------------------------

/// A unit's kind and size.
#[derive(Clone, Debug)]
enum Unit {
    Length(LengthUnit),
    Angle(AngleUnit),
    /// Any other unit (solid angle, mass, ratio, derived): its name.
    Other(String),
}

/// A measure read from a `measure_with_unit`.
#[derive(Clone, Debug)]
enum Measured {
    Length(Length),
    Angle(Angle),
    Ratio(Decimal),
    Count(Decimal),
    Other {
        measure: String,
        value: Decimal,
        unit: String,
    },
}

/// Why a measure or unit could not be read, with the kind of finding it is: `Unitless` when
/// there is no unit to read (missing, dangling, not a unit), `Nonconformance` when what is
/// stated is invalid or contradicts itself (a value whose type and unit disagree, a conversion
/// whose name contradicts its factor, an unknown prefix, a value that is not a number).
#[derive(Clone, Debug)]
struct MeasureError {
    kind: FindingKind,
    why: String,
}

impl MeasureError {
    fn unitless(why: String) -> Self {
        MeasureError {
            kind: FindingKind::Unitless,
            why,
        }
    }

    fn nonconformance(why: String) -> Self {
        MeasureError {
            kind: FindingKind::Nonconformance,
            why,
        }
    }
}

struct Reader<'a> {
    /// A source identifier may be projected several ways, but distinct carriers sharing
    /// it must be reported without losing their original characteristic numbers.
    source_identifiers: BTreeMap<(PartId, String), BTreeSet<u64>>,
    #[cfg(feature = "json")]
    json_carriers: BTreeMap<(PartId, String, usize), u64>,
    callout_targets: HashMap<u64, Vec<CalloutTarget>>,
    graphic_items: BTreeSet<u64>,
    doc: &'a Document,
    parts: &'a [PartDefinition],
    edition: &'static Edition,
    /// `product_definition_shape` → part index.
    shape_part: HashMap<u64, usize>,
    findings: Vec<Finding>,
    units: HashMap<u64, Result<Unit, MeasureError>>,
    slots: HashMap<String, Vec<(&'static str, &'static str)>>,
}

/// One part's reading.
struct PartState<'a> {
    id: PartId,
    part: &'a PartDefinition,
    pmi: PartPmi,
    prov: PartProvenance,
    identifiers: PartIdentifiers,
    /// Shape aspect → its feature (None: tried and failed, or in progress).
    features: HashMap<u64, Option<FeatureId>>,
    /// Shape aspect → its content when read without registering (members of a composite).
    contents: HashMap<u64, Option<(Feature, Vec<u64>)>>,
    dimensions: HashMap<u64, Option<DimensionId>>,
    tolerances: HashMap<u64, Option<ToleranceId>>,
    datums: HashMap<u64, DatumId>,
    targets: HashMap<u64, Option<DatumTargetId>>,
    /// Instances read as parts of other items (members of composites, datum feature aliases).
    absorbed: BTreeSet<u64>,
    /// Supplemental geometry item → its geometry.
    geometry: HashMap<u64, GeometryId>,
    /// Hole occurrence → its definition and placement, until its feature is registered.
    holes: HashMap<u64, (RoundHole, Option<Placement>)>,
    /// Shape aspects being read, one inside another (groups, derivations, aliases).
    nesting: usize,
}

/// Shape aspects nested deeper than this (groups, derivations, aliases, one inside another)
/// are not read: each level is several frames of the reader's stack, about 19 KiB unoptimised,
/// and a 1 MiB (WASM-like) stack must hold them (#52). Files nest two or three.
const FEATURE_NESTING: usize = 16;

impl<'a> PartState<'a> {
    fn new(id: PartId, part: &'a PartDefinition) -> Self {
        PartState {
            id,
            part,
            pmi: PartPmi::default(),
            prov: PartProvenance::default(),
            identifiers: PartIdentifiers::default(),
            features: HashMap::new(),
            contents: HashMap::new(),
            dimensions: HashMap::new(),
            tolerances: HashMap::new(),
            datums: HashMap::new(),
            targets: HashMap::new(),
            absorbed: BTreeSet::new(),
            geometry: HashMap::new(),
            holes: HashMap::new(),
            nesting: 0,
        }
    }
}

fn ids(v: impl IntoIterator<Item = u64>) -> Vec<u64> {
    let mut v: Vec<u64> = v.into_iter().collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// The entity references in an attribute, depth first.
fn refs_in(a: &Attribute, out: &mut Vec<u64>) {
    match a {
        Attribute::EntityRef(n) => out.push(*n),
        Attribute::List(l) => l.iter().for_each(|x| refs_in(x, out)),
        Attribute::Typed { value, .. } => refs_in(value, out),
        _ => {}
    }
}

fn refs(a: Option<&Attribute>) -> Vec<u64> {
    let mut out = Vec::new();
    if let Some(a) = a {
        refs_in(a, &mut out);
    }
    out
}

fn ref1(a: Option<&Attribute>) -> Option<u64> {
    match a? {
        Attribute::EntityRef(n) => Some(*n),
        _ => None,
    }
}

fn enum_value(a: Option<&Attribute>) -> Option<&str> {
    match a? {
        Attribute::Enum(s) => Some(s.as_str()),
        _ => None,
    }
}

/// The REAL tokens of an instance's text, in order.
pub(super) fn real_tokens(text: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    let n = text.len();
    while i < n {
        let c = text[i];
        match c {
            b'\'' => {
                i += 1;
                while i < n {
                    if text[i] == b'\'' {
                        if i + 1 < n && text[i + 1] == b'\'' {
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'"' => {
                i += 1;
                while i < n && text[i] != b'"' {
                    i += 1;
                }
                i += 1;
            }
            b'/' if i + 1 < n && text[i + 1] == b'*' => {
                i += 2;
                while i + 1 < n && !(text[i] == b'*' && text[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
            }
            b'#' => {
                i += 1;
                while i < n && text[i].is_ascii_digit() {
                    i += 1;
                }
            }
            b'.' if i + 1 < n && (text[i + 1].is_ascii_alphabetic() || text[i + 1] == b'_') => {
                i += 1;
                while i < n && text[i] != b'.' {
                    i += 1;
                }
                i += 1;
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                while i < n && (text[i].is_ascii_alphanumeric() || text[i] == b'_') {
                    i += 1;
                }
            }
            c if c.is_ascii_digit()
                || ((c == b'-' || c == b'+') && i + 1 < n && text[i + 1].is_ascii_digit()) =>
            {
                let start = i;
                i += 1;
                while i < n && text[i].is_ascii_digit() {
                    i += 1;
                }
                let mut real = false;
                if i < n && text[i] == b'.' {
                    real = true;
                    i += 1;
                    while i < n && text[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                if i < n && (text[i] == b'E' || text[i] == b'e') {
                    let save = i;
                    i += 1;
                    if i < n && (text[i] == b'+' || text[i] == b'-') {
                        i += 1;
                    }
                    if i < n && text[i].is_ascii_digit() {
                        while i < n && text[i].is_ascii_digit() {
                            i += 1;
                        }
                    } else {
                        i = save;
                    }
                }
                if real {
                    out.push(String::from_utf8_lossy(&text[start..i]).into_owned());
                }
            }
            _ => i += 1,
        }
    }
    out
}

fn leaf_names(e: &RawEntity) -> Vec<String> {
    match e {
        RawEntity::Simple { name, .. } => vec![name.to_ascii_lowercase()],
        RawEntity::Complex { parts, .. } => {
            parts.iter().map(|p| p.name.to_ascii_lowercase()).collect()
        }
    }
}

impl<'a> Reader<'a> {
    fn new(doc: &'a Document, parts: &'a [PartDefinition]) -> Self {
        let edition = doc
            .file_schema()
            .ok()
            .and_then(|s| s.first().and_then(|s| express::edition(s)))
            .unwrap_or(express::TARGET);
        let shape_part = parts
            .iter()
            .enumerate()
            .map(|(i, p)| (p.shape, i))
            .collect();
        Reader {
            #[cfg(feature = "json")]
            json_carriers: BTreeMap::new(),
            source_identifiers: BTreeMap::new(),
            callout_targets: HashMap::new(),
            graphic_items: BTreeSet::new(),
            doc,
            parts,
            edition,
            shape_part,
            findings: Vec::new(),
            units: HashMap::new(),
            slots: HashMap::new(),
        }
    }

    // --- types and attributes -------------------------------------------------------------

    fn names(&self, id: u64) -> Vec<String> {
        self.doc.get(id).map(leaf_names).unwrap_or_default()
    }

    fn entity_label(&self, id: u64) -> String {
        let mut n = self.names(id);
        n.sort();
        if n.is_empty() {
            "missing".to_string()
        } else {
            n.join("+")
        }
    }

    fn is_a_name(&self, name: &str, ancestor: &str) -> bool {
        self.edition.is_a(name, ancestor)
            || (!self.edition.is_declared(name) && express::TARGET.is_a(name, ancestor))
            || (!self.edition.is_declared(name)
                && !express::TARGET.is_declared(name)
                && express::AP242_ED1.is_a(name, ancestor))
    }

    fn is(&self, id: u64, ancestor: &str) -> bool {
        self.names(id).iter().any(|n| self.is_a_name(n, ancestor))
    }

    /// The (declaring entity, attribute) of each explicit attribute of a simple instance.
    fn slots_of(&mut self, name: &str) -> &Vec<(&'static str, &'static str)> {
        if !self.slots.contains_key(name) {
            let slots = self
                .edition
                .explicit_attributes(name)
                .or_else(|| express::TARGET.explicit_attributes(name))
                .or_else(|| express::AP242_ED1.explicit_attributes(name))
                .unwrap_or_default();
            let v = slots.into_iter().map(|s| (s.entity, s.name)).collect();
            self.slots.insert(name.to_string(), v);
        }
        &self.slots[name]
    }

    /// Attribute `attr` declared by `entity` (both lower case) of instance `id`.
    fn get(&mut self, id: u64, entity: &str, attr: &str) -> Option<&'a Attribute> {
        let e = self.doc.get(id)?;
        match e {
            RawEntity::Simple {
                name, attributes, ..
            } => {
                let lname = name.to_ascii_lowercase();
                let i = self
                    .slots_of(&lname)
                    .iter()
                    .position(|&(en, an)| en == entity && an == attr)?;
                attributes.get(i)
            }
            RawEntity::Complex { parts, .. } => {
                let leaf = parts.iter().find(|p| p.name.eq_ignore_ascii_case(entity))?;
                let decl = self
                    .edition
                    .entity(entity)
                    .or_else(|| express::TARGET.entity(entity))
                    .or_else(|| express::AP242_ED1.entity(entity))?;
                let i = decl.attributes.iter().position(|a| a.name == attr)?;
                leaf.attributes.get(i)
            }
        }
    }

    fn get_ref(&mut self, id: u64, entity: &str, attr: &str) -> Option<u64> {
        ref1(self.get(id, entity, attr))
    }

    fn get_refs(&mut self, id: u64, entity: &str, attr: &str) -> Vec<u64> {
        refs(self.get(id, entity, attr))
    }

    /// A string attribute, decoded; `None` when unset.
    fn get_str(&mut self, id: u64, entity: &str, attr: &str) -> Option<String> {
        match self.get(id, entity, attr)? {
            Attribute::String(s) => Some(s.clone()),
            _ => None,
        }
    }

    // A chunked descriptive value remains a compound in EXPRESS; only text consumers
    // interpret our marked, ordered list. Do not change schema subtype tests.
    fn is_text(&self, id: u64) -> bool {
        self.is(id, "descriptive_representation_item")
            || super::text::chunks(self.doc, id).is_some()
    }

    fn text_of(&self, id: u64) -> Option<String> {
        super::text::read(self.doc, id)
    }

    fn name_of(&mut self, id: u64) -> String {
        for (e, a) in [
            ("representation_item", "name"),
            ("shape_aspect", "name"),
            ("representation", "name"),
            ("property_definition", "name"),
            ("shape_aspect_relationship", "name"),
        ] {
            if self.is(id, e)
                && let Some(s) = self.get_str(id, e, a)
            {
                return s;
            }
        }
        String::new()
    }

    /// The instances referencing `id` that are of type `ancestor` and reference it through
    /// `entity.attr` (directly or within an aggregate), ascending.
    fn referrers_by(&mut self, id: u64, ancestor: &str, entity: &str, attr: &str) -> Vec<u64> {
        let candidates: Vec<u64> = self.doc.referrers(id).to_vec();
        candidates
            .into_iter()
            .filter(|&r| self.is(r, ancestor) && self.get_refs(r, entity, attr).contains(&id))
            .collect()
    }

    fn family(&mut self, id: u64) -> Family {
        let names = self.names(id);
        let mut fams = Vec::new();
        for n in &names {
            let f = if self.is_a_name(n, "property_definition") {
                let pd_name = self.get_str(id, "property_definition", "name");
                self.edition
                    .instance_family(n, pd_name.as_deref())
                    .or_else(|| express::TARGET.instance_family(n, pd_name.as_deref()))
            } else {
                self.edition
                    .family(n)
                    .or_else(|| express::TARGET.family(n))
                    .or_else(|| express::AP242_ED1.family(n))
            };
            fams.push(f.unwrap_or(Family::Other));
        }
        for f in [
            Family::Presentation,
            Family::ValidationProperty,
            Family::SemanticPmi,
        ] {
            if fams.contains(&f) {
                return f;
            }
        }
        Family::Other
    }

    fn find(
        &mut self,
        kind: FindingKind,
        part: Option<PartId>,
        subject: u64,
        others: &[u64],
        detail: impl Into<String>,
    ) {
        let mut v = vec![subject];
        v.extend(others.iter().copied().filter(|&o| o != subject));
        let entity = self.entity_label(subject);
        self.findings.push(Finding {
            kind,
            part,
            ids: v,
            entity,
            detail: detail.into(),
        });
    }

    /// `id_attribute`s naming `id` (§5.1 Figure 4): part of the item's construct.
    fn id_attributes(&mut self, id: u64) -> Vec<u64> {
        let mut out = self.referrers_by(id, "id_attribute", "id_attribute", "identified_item");
        out.extend(self.json_identifications(id));
        out
    }

    fn json_identifications(&mut self, id: u64) -> Vec<u64> {
        self.referrers_by(
            id,
            "applied_identification_assignment",
            "applied_identification_assignment",
            "items",
        )
        .into_iter()
        .filter(|&r| {
            self.get_ref(r, "identification_assignment", "role")
                .and_then(|role| self.get_str(role, "identification_role", "name"))
                .as_deref()
                == Some("haecceity.pmi/1 item id")
        })
        .collect()
    }

    fn item_identifier(
        &mut self,
        part: PartId,
        _collection: &str,
        _index: usize,
        id: u64,
    ) -> Option<String> {
        #[cfg(feature = "json")]
        self.json_carriers
            .insert((part, _collection.to_string(), _index), id);
        let item = match _collection {
            "features" => PresentedItem::Feature(FeatureId(_index)),
            "dimensions" => PresentedItem::Dimension(DimensionId(_index)),
            "tolerances" => PresentedItem::Tolerance(ToleranceId(_index)),
            "datums" => PresentedItem::Datum(DatumId(_index)),
            "datum_targets" => PresentedItem::DatumTarget(DatumTargetId(_index)),
            "notes" => PresentedItem::Note(_index),
            "attributes" => PresentedItem::Attribute(_index),
            "surface_textures" => PresentedItem::SurfaceTexture(_index),
            "general" => PresentedItem::General(_index),
            "threads" => PresentedItem::Thread(_index),
            "knurls" => PresentedItem::Knurl(_index),
            _ => return self.identifier(part, id),
        };
        self.callout_targets
            .entry(id)
            .or_default()
            .push(CalloutTarget { part, item });
        self.identifier(part, id)
    }

    fn identifier(&mut self, part: PartId, id: u64) -> Option<String> {
        let attributes = self.id_attributes(id);
        match attributes.as_slice() {
            [] => None,
            [attribute] => {
                let value = self.get_str(*attribute, "id_attribute", "attribute_value");
                let value = value.or_else(|| {
                    self.get_str(*attribute, "identification_assignment", "assigned_id")
                });
                if value.is_none() {
                    self.find(
                        FindingKind::Nonconformance,
                        Some(part),
                        id,
                        &attributes,
                        "id_attribute has no string attribute_value; identifier not read",
                    );
                }
                if let Some(value) = &value {
                    self.source_identifiers
                        .entry((part, value.clone()))
                        .or_default()
                        .insert(id);
                }
                value
            }
            _ => {
                self.find(
                    FindingKind::Nonconformance,
                    Some(part),
                    id,
                    &attributes,
                    "more than one id_attribute on the same entity; identifier not read",
                );
                None
            }
        }
    }

    fn check_identifiers(&mut self) {
        for ((part, value), owners) in std::mem::take(&mut self.source_identifiers) {
            if !value.is_empty() && owners.len() < 2 {
                continue;
            }
            let mut evidence: Vec<_> = owners.iter().copied().collect();
            for &owner in &owners {
                evidence.extend(self.id_attributes(owner));
            }
            if let Some(&owner) = owners.first() {
                self.find(
                    FindingKind::Nonconformance,
                    Some(part),
                    owner,
                    &evidence,
                    format!(
                        "{} source identifier {value:?}; retained as identifier metadata, with content-derived JSON ids for empty or repeated item ids",
                        if value.is_empty() { "empty" } else { "duplicate" }
                    ),
                );
            }
        }
    }

    // --- values ---------------------------------------------------------------------------

    /// The text of the REAL `target` (an attribute of instance `id`) as the file writes it.
    fn real_text(&self, id: u64, target: &Attribute) -> Option<String> {
        if let Attribute::Integer(i) = target {
            return Some(i.to_string());
        }
        let e = self.doc.get(id)?;
        let mut n = 0usize;
        let mut found = None;
        let mut count = 0usize;
        visit_attributes(e, &mut |a| {
            if let Attribute::Real(_) = a {
                if std::ptr::eq(a, target) {
                    found = Some(n);
                }
                n += 1;
                count += 1;
            }
        });
        let span = self.doc.span(id)?;
        let tokens = real_tokens(&self.doc.bytes()[span]);
        if tokens.len() != count {
            return None;
        }
        tokens.get(found?).cloned()
    }

    fn unit(&mut self, id: u64) -> Result<Unit, MeasureError> {
        if let Some(u) = self.units.get(&id) {
            return u.clone();
        }
        self.units.insert(
            id,
            Err(MeasureError::unitless(format!(
                "unit #{id} refers to itself"
            ))),
        );
        let u = self.resolve_unit(id);
        self.units.insert(id, u.clone());
        u
    }

    fn resolve_unit(&mut self, id: u64) -> Result<Unit, MeasureError> {
        let names = self.names(id);
        if names.is_empty() {
            return Err(MeasureError::unitless(format!("unit #{id} does not exist")));
        }
        let has = |n: &str| names.iter().any(|x| x == n);
        if has("si_unit") {
            let prefix = enum_value(self.get(id, "si_unit", "prefix")).map(str::to_ascii_uppercase);
            let name = enum_value(self.get(id, "si_unit", "name"))
                .map(str::to_ascii_uppercase)
                .ok_or_else(|| MeasureError::unitless(format!("si_unit #{id} has no name")))?;
            let power = match prefix.as_deref() {
                None => 0,
                Some(p) => match si_prefix(p) {
                    Some(e) => e,
                    None => {
                        return Err(MeasureError::nonconformance(format!(
                            "si_unit #{id} has unknown prefix {p}"
                        )));
                    }
                },
            };
            let full = format!(
                "{}{}",
                prefix
                    .as_deref()
                    .map_or(String::new(), str::to_ascii_lowercase),
                name.to_ascii_lowercase()
            );
            let one = Decimal::parse("1.")
                .map_err(|e| MeasureError::nonconformance(format!("unit scale: {e:?}")))?;
            return Ok(match name.as_str() {
                "METRE" => Unit::Length(LengthUnit::from_metres(&full, one.scaled(power))),
                "RADIAN" if power == 0 => Unit::Angle(AngleUnit::Radian),
                "RADIAN" => Unit::Angle(AngleUnit::Other {
                    name: full,
                    radians: one.scaled(power),
                }),
                _ => Unit::Other(full),
            });
        }
        if has("conversion_based_unit") {
            let name = self
                .get_str(id, "conversion_based_unit", "name")
                .unwrap_or_default();
            let factor = self
                .get_ref(id, "conversion_based_unit", "conversion_factor")
                .ok_or_else(|| {
                    MeasureError::unitless(format!(
                        "conversion_based_unit #{id} has no conversion factor"
                    ))
                })?;
            let m = self.measure(factor)?;
            let lname = name.to_ascii_lowercase();
            return match m {
                Measured::Length(l) => {
                    let unit = LengthUnit::from_metres(&name, l.value.times(&l.unit.metres()));
                    let expected = match lname.as_str() {
                        "inch" => Some(LengthUnit::Inch),
                        "millimetre" | "millimeter" => Some(LengthUnit::Millimetre),
                        "foot" => Some(LengthUnit::Foot),
                        "metre" | "meter" => Some(LengthUnit::Metre),
                        _ => None,
                    };
                    if let Some(e) = expected
                        && e != unit
                    {
                        return Err(MeasureError::nonconformance(format!(
                            "conversion_based_unit #{id} is named {name:?} but its factor is {} {:?}",
                            l.value, l.unit
                        )));
                    }
                    Ok(Unit::Length(unit))
                }
                Measured::Angle(a) => {
                    let radians = a.rad();
                    let degree = std::f64::consts::PI / 180.0;
                    let unit = if (radians / degree - 1.0).abs() < 1e-9 {
                        AngleUnit::Degree
                    } else if a.unit == AngleUnit::Radian {
                        AngleUnit::Other {
                            name: name.clone(),
                            radians: a.value.clone(),
                        }
                    } else {
                        return Err(MeasureError::nonconformance(format!(
                            "conversion_based_unit #{id}: an angle factor in a unit other than radian"
                        )));
                    };
                    if lname == "degree" && unit != AngleUnit::Degree {
                        return Err(MeasureError::nonconformance(format!(
                            "conversion_based_unit #{id} is named {name:?} but its factor is {} rad",
                            a.value
                        )));
                    }
                    Ok(Unit::Angle(unit))
                }
                _ => Ok(Unit::Other(name)),
            };
        }
        if has("derived_unit") || has("context_dependent_unit") || has("named_unit") {
            let name = if has("context_dependent_unit") {
                self.get_str(id, "context_dependent_unit", "name")
                    .unwrap_or_default()
            } else {
                super::unit_identity::identity(self.doc, id).ok_or_else(|| {
                    MeasureError::unitless(format!("unit #{id} has no portable content identity"))
                })?
            };
            return Ok(Unit::Other(name));
        }
        Err(MeasureError::unitless(format!(
            "#{id} ({}) is not a unit",
            names.join("+")
        )))
    }

    /// The measure of a `measure_with_unit` instance (simple or a complex with that leaf).
    fn measure(&mut self, id: u64) -> Result<Measured, MeasureError> {
        if !self.is(id, "measure_with_unit") {
            return Err(MeasureError::nonconformance(format!(
                "#{id} ({}) is not a measure",
                self.entity_label(id)
            )));
        }
        let value = self
            .get(id, "measure_with_unit", "value_component")
            .ok_or_else(|| MeasureError::nonconformance(format!("measure #{id} has no value")))?;
        let (type_name, inner) = match value {
            Attribute::Typed { type_name, value } => {
                (Some(type_name.to_ascii_lowercase()), &**value)
            }
            v => (None, v),
        };
        let text = match inner {
            Attribute::Real(_) | Attribute::Integer(_) => {
                self.real_text(id, inner).ok_or_else(|| {
                    MeasureError::nonconformance(format!(
                        "measure #{id}: the value's text could not be read"
                    ))
                })?
            }
            other => {
                return Err(MeasureError::nonconformance(format!(
                    "measure #{id}: value {other:?} is not a number"
                )));
            }
        };
        let decimal = Decimal::parse(&text)
            .map_err(|e| MeasureError::nonconformance(format!("measure #{id}: {e}")))?;
        let unit_id = self
            .get_ref(id, "measure_with_unit", "unit_component")
            .ok_or_else(|| MeasureError::unitless(format!("measure #{id} has no unit instance")))?;
        let unit = self.unit(unit_id)?;
        let tn = type_name.as_deref().unwrap_or("");
        match tn {
            "ratio_measure" | "positive_ratio_measure" => return Ok(Measured::Ratio(decimal)),
            "count_measure" => return Ok(Measured::Count(decimal)),
            _ => {}
        }
        let length_type = matches!(
            tn,
            "length_measure" | "positive_length_measure" | "non_negative_length_measure"
        );
        let angle_type = matches!(tn, "plane_angle_measure" | "positive_plane_angle_measure");
        match unit {
            Unit::Length(u) if length_type || tn.is_empty() => Ok(Measured::Length(Length {
                value: decimal,
                unit: u,
            })),
            Unit::Angle(u) if angle_type || tn.is_empty() => Ok(Measured::Angle(Angle {
                value: decimal,
                unit: u,
            })),
            Unit::Other(u) if !length_type && !angle_type => Ok(Measured::Other {
                measure: tn.to_string(),
                value: decimal,
                unit: u,
            }),
            u => Err(MeasureError::nonconformance(format!(
                "measure #{id}: a {} value in unit {u:?}",
                if tn.is_empty() { "untyped" } else { tn }
            ))),
        }
    }

    /// Whether a measure's value is written untyped (a nonconformance: the schema's SELECT
    /// needs the defined type).
    fn untyped(&mut self, id: u64) -> bool {
        !matches!(
            self.get(id, "measure_with_unit", "value_component"),
            Some(Attribute::Typed { .. })
        )
    }

    /// A length or angle value of measure `id` with its §5.4 decimal places from the
    /// qualifiers of the instance itself (a qualified measure representation item) and of
    /// `measure_qualification`s naming it. Findings for anything wrong; the ids read go to
    /// `consumed`.
    fn value(
        &mut self,
        st: &PartState,
        id: u64,
        want: Want,
        consumed: &mut Vec<u64>,
    ) -> Option<Value> {
        let m = match self.measure(id) {
            Ok(m) => m,
            Err(e) => {
                self.find(e.kind, Some(st.id), id, &[], e.why);
                return None;
            }
        };
        if self.untyped(id) {
            self.find(
                FindingKind::Nonconformance,
                Some(st.id),
                id,
                &[],
                "value written untyped (the measure_value SELECT needs its defined type); read as its unit's quantity",
            );
        }
        let quantity = match (m, want) {
            (Measured::Length(l), Want::Length | Want::Any) => Quantity::Length(l),
            (Measured::Angle(a), Want::Angle | Want::Any) => Quantity::Angle(a),
            (m, _) => {
                self.find(
                    FindingKind::Nonconformance,
                    Some(st.id),
                    id,
                    &[],
                    format!("expected a {want:?} value, the measure is {m:?}"),
                );
                return None;
            }
        };
        consumed.push(id);
        let mut places = None;
        let mut qualifiers = self.get_refs(id, "qualified_representation_item", "qualifiers");
        for mq in self.referrers_by(
            id,
            "measure_qualification",
            "measure_qualification",
            "qualified_measure",
        ) {
            consumed.push(mq);
            for q in self.get_refs(mq, "measure_qualification", "qualifiers") {
                if self.is(q, "value_format_type_qualifier") {
                    qualifiers.push(q);
                } else {
                    // A measure_qualification's other qualifiers (NIST CTC-05:
                    // TYPE_QUALIFIER('designed') on a dimension's deviations) say something
                    // the model's value does not hold.
                    let label = self.entity_label(q);
                    let name = self.get_str(q, "type_qualifier", "name");
                    self.find(
                        FindingKind::NotModelled,
                        Some(st.id),
                        q,
                        &[mq, id],
                        format!(
                            "qualifier {label}{} on a value (through a measure_qualification) is not held; the value is read without it",
                            name.map_or(String::new(), |n| format!(" {n:?}"))
                        ),
                    );
                }
            }
        }
        for q in qualifiers {
            if self.is(q, "value_format_type_qualifier") {
                consumed.push(q);
                let f = self
                    .get_str(q, "value_format_type_qualifier", "format_type")
                    .unwrap_or_default();
                match decimal_places(&f) {
                    Some(p) => places = Some(p),
                    None => self.find(
                        FindingKind::Nonconformance,
                        Some(st.id),
                        q,
                        &[id],
                        format!("format {f:?} is not NR2 x.y (§5.4); decimal places not read"),
                    ),
                }
            }
        }
        Some(Value {
            quantity,
            decimal_places: places,
        })
    }

    // --- per part -------------------------------------------------------------------------

    fn read_part(&mut self, st: &mut PartState) {
        let pds = st.part.shape;
        let pd = st.part.product_definition;
        let mut aspects: Vec<u64> = self
            .doc
            .referrers(pds)
            .iter()
            .copied()
            .filter(|&r| self.is(r, "shape_aspect") && !self.is(r, "model_geometric_view"))
            .collect();
        aspects.sort_unstable();

        // Datum targets and datums first: every one is read whether referenced or not.
        let datums: Vec<u64> = aspects
            .iter()
            .copied()
            .filter(|&a| self.is(a, "datum"))
            .collect();
        self.read_datums(st, &datums);
        for &a in &aspects {
            if self.is(a, "datum_target") && !st.targets.contains_key(&a) {
                self.find(
                    FindingKind::Nonconformance,
                    Some(st.id),
                    a,
                    &[],
                    "datum target establishes no datum of the part (datum_target WR1); not read",
                );
            }
        }

        // Dimensions on the part's shape aspects.
        let mut dims = BTreeSet::new();
        for &a in &aspects {
            if self.is(a, "dimensional_size") {
                dims.insert(a);
            }
            for r in self.referrers_by(a, "dimensional_size", "dimensional_size", "applies_to") {
                dims.insert(r);
            }
            for r in self.referrers_by(
                a,
                "dimensional_location",
                "shape_aspect_relationship",
                "relating_shape_aspect",
            ) {
                dims.insert(r);
            }
        }
        for d in dims {
            self.dimension(st, d);
        }

        // Tolerances on the part, its shape aspects, dimensions and relationships.
        let mut tols = BTreeSet::new();
        let mut targets: Vec<u64> = vec![pds];
        targets.extend(&aspects);
        targets.extend(st.dimensions.keys().copied());
        for &a in &aspects {
            for r in self.referrers_by(
                a,
                "shape_aspect_relationship",
                "shape_aspect_relationship",
                "relating_shape_aspect",
            ) {
                targets.push(r);
            }
        }
        for t in targets {
            for r in self.referrers_by(
                t,
                "geometric_tolerance",
                "geometric_tolerance",
                "toleranced_shape_aspect",
            ) {
                tols.insert(r);
            }
        }
        for t in &tols {
            self.tolerance(st, *t);
        }
        for t in &tols {
            self.relations(st, *t);
        }

        // Threads and knurls applied to the part's shape aspects.
        self.features_definitions(st, &aspects);

        // Part properties: standard, general tolerances, material, notes, attributes.
        self.standard(st, pd);
        let mut owners: Vec<(u64, Option<NoteOwner>)> = vec![(pd, None), (pds, None)];
        let mut aspect_owners: Vec<u64> = aspects.clone();
        aspect_owners.sort_unstable();
        for a in aspect_owners {
            owners.push((a, None));
        }
        let dim_ids: Vec<(u64, DimensionId)> = st
            .dimensions
            .iter()
            .filter_map(|(&k, v)| v.map(|d| (k, d)))
            .collect();
        let tol_ids: Vec<(u64, ToleranceId)> = st
            .tolerances
            .iter()
            .filter_map(|(&k, v)| v.map(|d| (k, d)))
            .collect();
        let mut seen_owner = BTreeSet::new();
        let mut list: Vec<(u64, Option<NoteOwner>)> = Vec::new();
        for (o, _) in owners {
            if seen_owner.insert(o) {
                list.push((o, None));
            }
        }
        for (o, d) in dim_ids {
            if seen_owner.insert(o) {
                list.push((o, Some(NoteOwner::Dimension(d))));
            } else if let Some(e) = list.iter_mut().find(|e| e.0 == o) {
                e.1 = Some(NoteOwner::Dimension(d));
            }
        }
        for (o, t) in tol_ids {
            if seen_owner.insert(o) {
                list.push((o, Some(NoteOwner::Tolerance(t))));
            }
        }
        list.sort_by_key(|e| e.0);
        for (owner, as_item) in list {
            self.properties(st, owner, as_item);
        }

        // Shape aspects nothing above read: features in their own right.
        for &a in &aspects {
            // Preserve an explicitly authored independent feature even when its one
            // geometry carrier is also used as a composition member (§5.1).
            if !st.features.contains_key(&a)
                && matches!(self.get(a, "shape_aspect", "name"), Some(Attribute::String(name)) if name == "haecceity.feature/1")
            {
                self.feature(st, a);
                continue;
            }
            if st.features.contains_key(&a)
                || st.absorbed.contains(&a)
                || st.targets.contains_key(&a)
                || st.datums.contains_key(&a)
                || st.dimensions.contains_key(&a)
            {
                continue;
            }
            if self.is(a, "datum")
                || self.is(a, "datum_target")
                || self.is(a, "datum_system")
                || self.is(a, "general_datum_reference")
                || self.is(a, "tolerance_zone")
            {
                continue; // reported by accounting when no item consumed them
            }
            if self.consumed_elsewhere(st, a) {
                continue;
            }
            self.feature(st, a);
        }

        // Hole occurrences read only as members of another feature: their faces are that
        // feature's, and no feature of their own carries the definition.
        let mut absorbed_holes: Vec<u64> = st.holes.keys().copied().collect();
        absorbed_holes.sort_unstable();
        for occ in absorbed_holes {
            st.holes.remove(&occ);
            self.find(
                FindingKind::NotModelled,
                Some(st.id),
                occ,
                &[],
                "the occurrence is read only as a member of another feature; its feature definition (basic_round_hole) is not held there",
            );
        }

        #[cfg(feature = "json")]
        if !st.prov.transport.is_empty() {
            // The versioned JSON document can also declare supplemental geometry
            // with no feature anchor. Read its native constructive representation.
            for link in self.referrers_by(
                st.part.shape,
                "shape_definition_representation",
                "property_definition_representation",
                "definition",
            ) {
                if let Some(shape) = self.get_ref(
                    link,
                    "property_definition_representation",
                    "used_representation",
                ) {
                    for relationship in self.referrers_by(
                        shape,
                        "constructive_geometry_representation_relationship",
                        "representation_relationship",
                        "rep_1",
                    ) {
                        if let Some(rep) =
                            self.get_ref(relationship, "representation_relationship", "rep_2")
                            && self.is(rep, "constructive_geometry_representation")
                        {
                            for item in self.get_refs(rep, "representation", "items") {
                                if let Err(why) = self.geometry(st, item, Some(rep)) {
                                    self.find(
                                        FindingKind::Unresolved,
                                        Some(st.id),
                                        item,
                                        &[rep],
                                        why,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        self.notes_on_faces(st, &aspects);
        self.note_callouts(st);
    }

    /// specify-core's notes on faces (its `requirements.append`, Python): a thread, knurl or
    /// finish is a part-level 'manufacturing requirement' note of that kind, written
    /// immediately followed by a plain `shape_aspect` named as the kind whose
    /// `geometric_item_specific_usage`s, named alike, identify the faces; nothing semantic
    /// refers to that aspect (a presentation link does). Each such aspect, read above as a
    /// feature in its own right, anchors the note of its kind written last before it; a note
    /// no aspect follows (a part note) stays on the part. An aspect of that form with no
    /// unanchored note of its kind before it is reported: which note it is for is not stated.
    fn notes_on_faces(&mut self, st: &mut PartState, aspects: &[u64]) {
        const KINDS: [&str; 5] = [
            "internal thread",
            "external thread",
            "knurl",
            "surface finish",
            "surface texture",
        ];
        let part = Some(st.id);
        for kind in KINDS {
            // (id, Ok(note index) | Err(aspect)), in file order.
            let mut events: Vec<(u64, Result<usize, u64>)> = Vec::new();
            for (i, note) in st.pmi.notes.iter().enumerate() {
                if note.kind != kind || note.on.is_some() {
                    continue;
                }
                let pdef = st.prov.notes[i].iter().copied().find(|&id| {
                    self.is(id, "property_definition")
                        && self.get_str(id, "property_definition", "name").as_deref()
                            == Some("manufacturing requirement")
                });
                if let Some(pdef) = pdef {
                    events.push((pdef, Ok(i)));
                }
            }
            for &a in aspects {
                if self.names(a) != ["shape_aspect"]
                    || self.get_str(a, "shape_aspect", "name").as_deref() != Some(kind)
                {
                    continue;
                }
                let usages: Vec<u64> = self.usages(a).into_iter().map(|(u, _, _)| u).collect();
                let named = !usages.is_empty()
                    && usages.iter().all(|&u| {
                        self.names(u) == ["geometric_item_specific_usage"]
                            && self
                                .get_str(u, "item_identified_representation_usage", "name")
                                .as_deref()
                                == Some(kind)
                    });
                let only_presented = self.doc.referrers(a).to_vec().into_iter().all(|r| {
                    usages.contains(&r) || self.is(r, "draughting_model_item_association")
                });
                if named && only_presented && st.features.get(&a).copied().flatten().is_some() {
                    events.push((a, Err(a)));
                }
            }
            events.sort_by_key(|e| e.0);
            let mut last: Option<usize> = None;
            for (_, e) in events {
                match e {
                    Ok(note) => last = Some(note),
                    Err(a) => match last.take() {
                        Some(note) => {
                            // Only aspects with a held feature are queued above.
                            if let Some(f) = st.features.get(&a).copied().flatten() {
                                st.pmi.notes[note].on = Some(NoteOwner::Feature(f));
                            }
                        }
                        None => self.find(
                            FindingKind::Unresolved,
                            part,
                            a,
                            &[],
                            format!(
                                "specify-core's {kind:?} aspect follows no {kind:?} note; which note it is for is not stated (its faces are read as a feature)"
                            ),
                        ),
                    },
                }
            }
        }
    }

    /// The callouts presenting each note (§7.3): the draughting callouts and annotation
    /// occurrences a `draughting_model_item_association` relates to the note's property
    /// definition, or to a shape aspect of the feature it is on that is the note's alone
    /// (nothing refers to the aspect but its usages, presentation associations and the note's
    /// property definition: specify-core's notes on faces). A callout's features are those of
    /// the part's shape aspects its associations relate it to (another part's aspect is that
    /// part's presentation). What cannot be held is reported: an associated item that is no
    /// callout or annotation occurrence, an aspect of the part that is no feature. Presentation
    /// is not consumed: it stays counted, and a replace takes it by its removal policy.
    fn note_callouts(&mut self, st: &mut PartState) {
        const DMIA: &str = "draughting_model_item_association";
        const IIRU: &str = "item_identified_representation_usage";
        let part = Some(st.id);
        for i in 0..st.pmi.notes.len() {
            let Some(pdef) = st.prov.notes[i]
                .iter()
                .copied()
                .find(|&id| self.is(id, "property_definition"))
            else {
                st.prov.note_callouts.push(Vec::new());
                continue;
            };
            let mut subjects = vec![pdef];
            if let Some(NoteOwner::Feature(f)) = st.pmi.notes[i].on {
                let mut aspects: Vec<u64> = st
                    .features
                    .iter()
                    .filter(|(_, v)| **v == Some(f))
                    .map(|(&a, _)| a)
                    .collect();
                aspects.sort_unstable();
                for a in aspects {
                    let usages: Vec<u64> = self.usages(a).into_iter().map(|(u, _, _)| u).collect();
                    let own = self
                        .doc
                        .referrers(a)
                        .to_vec()
                        .into_iter()
                        .all(|r| r == pdef || usages.contains(&r) || self.is(r, DMIA));
                    if own {
                        subjects.push(a);
                    }
                }
            }
            let mut callouts = BTreeSet::new();
            for s in subjects {
                for assoc in self.presentation_associations(s, "definition") {
                    for item in self.get_refs(assoc, IIRU, "identified_item") {
                        if self.is(item, "draughting_callout")
                            || self.is(item, "annotation_occurrence")
                        {
                            callouts.insert(item);
                        } else {
                            let what = self.entity_label(item);
                            self.find(
                                FindingKind::NotModelled,
                                part,
                                assoc,
                                &[item, s],
                                format!("a note's presentation association names a {what}, which is no callout or annotation occurrence; not held as the note's callout"),
                            );
                        }
                    }
                }
            }
            st.prov
                .note_callouts
                .push(callouts.iter().copied().collect());
            let mut held = Vec::new();
            for c in callouts {
                let mut features = BTreeSet::new();
                for assoc in self.presentation_associations(c, "identified_item") {
                    let Some(d) = self.get_ref(assoc, IIRU, "definition") else {
                        continue;
                    };
                    if !self.is(d, "shape_aspect")
                        || self.get_ref(d, "shape_aspect", "of_shape") != Some(st.part.shape)
                    {
                        continue;
                    }
                    match st.features.get(&d).copied().flatten() {
                        Some(f) => {
                            features.insert(f);
                        }
                        None => self.find(
                            FindingKind::NotModelled,
                            part,
                            assoc,
                            &[c, d],
                            "a note's callout is associated with a shape aspect of the part that is not read as a feature; that association is not held",
                        ),
                    }
                }
                held.push(Callout {
                    name: self.name_of(c),
                    features: features.into_iter().collect(),
                });
            }
            st.pmi.notes[i].callouts = held;
        }
    }

    /// Whether `a` was consumed by an item of the part already (a zone, a target's area, …).
    fn consumed_elsewhere(&self, st: &PartState, a: u64) -> bool {
        st.prov.all().binary_search(&a).is_ok()
    }

    // --- features -------------------------------------------------------------------------

    /// The usages (`item_identified_representation_usage`, not presentation) defining `id`
    /// and their identified items.
    fn usages(&mut self, id: u64) -> Vec<(u64, Option<u64>, Vec<u64>)> {
        let mut out = Vec::new();
        for u in self.referrers_by(
            id,
            "item_identified_representation_usage",
            "item_identified_representation_usage",
            "definition",
        ) {
            if self.family(u) == Family::Presentation {
                continue;
            }
            let items = self.get_refs(u, "item_identified_representation_usage", "identified_item");
            let rep = self.get_ref(
                u,
                "item_identified_representation_usage",
                "used_representation",
            );
            out.push((u, rep, items));
        }
        out
    }

    /// The shape aspects `id` is related to as relating shape aspect by plain composition
    /// relationships (not dimensions, derivations, target or definition relationships), with
    /// the relationship ids.
    fn members(&mut self, id: u64) -> Vec<(u64, u64)> {
        let mut out = Vec::new();
        for r in self.referrers_by(
            id,
            "shape_aspect_relationship",
            "shape_aspect_relationship",
            "relating_shape_aspect",
        ) {
            if [
                "dimensional_location",
                "feature_for_datum_target_relationship",
                "shape_aspect_deriving_relationship",
                "shape_defining_relationship",
            ]
            .iter()
            .any(|t| self.is(r, t))
            {
                continue;
            }
            // A tolerance's relation target and a zone's affected-plane link do not
            // compose the relating feature of the related one.
            if self
                .get_str(r, "shape_aspect_relationship", "name")
                .as_deref()
                == Some("affected plane association")
                || self
                    .get_str(r, "shape_aspect_relationship", "description")
                    .as_deref()
                    == Some("affected plane association")
                || !self
                    .referrers_by(
                        r,
                        "geometric_tolerance",
                        "geometric_tolerance",
                        "toleranced_shape_aspect",
                    )
                    .is_empty()
            {
                continue;
            }
            let Some(m) = self.get_ref(r, "shape_aspect_relationship", "related_shape_aspect")
            else {
                continue;
            };
            if self.is(m, "datum") || self.is(m, "datum_target") {
                continue;
            }
            out.push((r, m));
        }
        out
    }

    /// A plain constituent of one indivisible datum feature (§6.5.1–6.5.2).
    /// The datum feature is SAR.relating_shape_aspect, its constituent is related.
    /// SAR names/descriptions are not discriminators ("datum feature" is one exporter label).
    /// A group datum applies to each member separately and is not a union alias.
    /// Every constituent is checked against the datum's whole content before aliasing.
    fn datum_feature_parent(&mut self, id: u64) -> Option<u64> {
        if self.names(id) != ["shape_aspect"] {
            return None;
        }
        let mut parents = BTreeSet::new();
        for r in self.referrers_by(
            id,
            "shape_aspect_relationship",
            "shape_aspect_relationship",
            "related_shape_aspect",
        ) {
            if let Some(parent) =
                self.get_ref(r, "shape_aspect_relationship", "relating_shape_aspect")
                && self.is(parent, "datum_feature")
                && !self.is(parent, "composite_group_shape_aspect")
                && !self.is(parent, "all_around_shape_aspect")
                && !self.is(parent, "between_shape_aspect")
                && self.members(parent).contains(&(r, id))
            {
                parents.insert(parent);
            }
        }
        if parents.len() == 1 {
            parents.into_iter().next()
        } else {
            None
        }
    }

    /// A constituent names the whole datum feature only when their face sets agree.
    /// A land of a multi-constituent datum keeps its own feature, even when it has no
    /// other composition parent. Content is memoised, so faces are still collected once.
    fn datum_constituent_alias(&mut self, st: &mut PartState, id: u64, feature: FeatureId) -> bool {
        matches!(st.pmi.features[feature.0], Feature::Items(_))
            && self
                .content(st, id)
                .is_some_and(|(content, _)| content == st.pmi.features[feature.0])
    }

    /// Whether shape aspect `id` may be read one level deeper; a finding when not.
    fn nest(&mut self, st: &mut PartState, id: u64) -> bool {
        if st.nesting >= FEATURE_NESTING {
            self.find(
                FindingKind::Unsupported,
                Some(st.id),
                id,
                &[],
                format!(
                    "shape aspects nested more than {FEATURE_NESTING} deep (groups, derivations or aliases); not read"
                ),
            );
            return false;
        }
        st.nesting += 1;
        true
    }

    /// The feature of shape aspect `id`, registered in the part (memoised).
    fn feature(&mut self, st: &mut PartState, id: u64) -> Option<FeatureId> {
        if st.features.contains_key(&id) || !self.nest(st, id) {
            return st.features.get(&id).copied().flatten();
        }
        let r = self.feature_at(st, id);
        st.nesting -= 1;
        r
    }

    fn feature_at(&mut self, st: &mut PartState, id: u64) -> Option<FeatureId> {
        if let Some(r) = st.features.get(&id) {
            return *r;
        }
        // Resolve the datum first, even when another datum or requirement names a child
        // before its parent. Registering the parent can already register this child.
        let mut constituent_content = None;
        if let Some(parent) = self.datum_feature_parent(id)
            && let Some(feature) = self.feature(st, parent)
        {
            if let Some(r) = st.features.get(&id) {
                return *r;
            }
            if self.datum_constituent_alias(st, id, feature) {
                st.features.insert(id, Some(feature));
                st.absorbed.insert(id);
                return Some(feature);
            }
            constituent_content = self.content(st, id);
        }
        st.features.insert(id, None);
        // Composition has already read a plain constituent's content. Reuse it when
        // registering its own identity rather than collecting its faces a second time.
        let built = match constituent_content {
            Some((f, prov)) => Built::Feature(f, prov),
            None => self.build_feature(st, id),
        };
        let r = match built {
            Built::Feature(f, prov) => {
                let fid = FeatureId(st.pmi.features.len());
                st.pmi.features.push(f);
                st.prov.features.push(ids(prov));
                st.identifiers.features.push(self.item_identifier(
                    st.id,
                    "features",
                    st.pmi.features.len() - 1,
                    id,
                ));
                if let Some((definition, placement)) = st.holes.remove(&id) {
                    st.pmi.holes.push(Hole {
                        feature: fid,
                        definition,
                        placement,
                    });
                }
                Some(fid)
            }
            Built::Alias(member, prov) => {
                if st.holes.remove(&id).is_some() {
                    self.find(
                        FindingKind::NotModelled,
                        Some(st.id),
                        id,
                        &[],
                        "the occurrence is read as the feature of its one member; its feature definition (basic_round_hole) is not held there",
                    );
                }
                let fid = self.feature(st, member);
                if let Some(f) = fid {
                    let p = &mut st.prov.features[f.0];
                    p.extend(prov);
                    *p = ids(std::mem::take(p));
                    st.absorbed.insert(id);
                }
                fid
            }
            Built::None => None,
        };
        // A hole whose feature failed is reported by that failure.
        st.holes.remove(&id);
        st.features.insert(id, r);
        if let Some(feature) = r
            && self.is(id, "datum_feature")
            && matches!(st.pmi.features[feature.0], Feature::Items(_))
        {
            // Preserve identities even for constituents named only by presentation.
            // Whole-set constituents alias the datum; proper subsets register separately.
            let constituents = st.prov.features[feature.0].clone();
            for aspect in constituents {
                if self.datum_feature_parent(aspect) == Some(id) {
                    if self.datum_constituent_alias(st, aspect, feature) {
                        st.features.insert(aspect, Some(feature));
                        st.absorbed.insert(aspect);
                    } else if !self
                        .presentation_associations(aspect, "definition")
                        .is_empty()
                    {
                        // Unnamed support aspects remain absorbed. A presentation naming
                        // a proper subset needs its own feature, just like a requirement.
                        self.feature(st, aspect);
                    }
                }
            }
        }
        r
    }

    /// The content of shape aspect `id` without registering it (a member of a composite).
    fn content(&mut self, st: &mut PartState, id: u64) -> Option<(Feature, Vec<u64>)> {
        if st.contents.contains_key(&id) || !self.nest(st, id) {
            return st.contents.get(&id).cloned().flatten();
        }
        let c = self.content_at(st, id);
        st.nesting -= 1;
        c
    }

    fn content_at(&mut self, st: &mut PartState, id: u64) -> Option<(Feature, Vec<u64>)> {
        if let Some(c) = st.contents.get(&id) {
            return c.clone();
        }
        st.contents.insert(id, None);
        let c = match self.build_feature(st, id) {
            Built::Feature(f, p) => Some((f, p)),
            Built::Alias(m, p) => self.content(st, m).map(|(f, mut q)| {
                q.extend(p);
                (f, q)
            }),
            Built::None => {
                st.holes.remove(&id);
                None
            }
        };
        st.contents.insert(id, c.clone());
        c
    }

    fn build_feature(&mut self, st: &mut PartState, id: u64) -> Built {
        let part = Some(st.id);
        if !self.is(id, "shape_aspect") {
            self.find(
                FindingKind::Unresolved,
                part,
                id,
                &[],
                "expected a shape aspect",
            );
            return Built::None;
        }
        let of_shape = self.get_ref(id, "shape_aspect", "of_shape");
        if of_shape != Some(st.part.shape) {
            self.find(
                FindingKind::Unresolved,
                part,
                id,
                &of_shape.into_iter().collect::<Vec<_>>(),
                "shape aspect of another product definition shape referenced from this part's PMI",
            );
            return Built::None;
        }
        let description = self.get_str(id, "shape_aspect", "description");
        if description.as_deref() == Some("CATIA Geometric Set") {
            self.find(
                FindingKind::NotModelled,
                part,
                id,
                &[],
                "CATIA Geometric Set: CATIA-specific, to be ignored by other systems (UDA practice §6.3)",
            );
            return Built::None;
        }
        let names = self.names(id);
        let occurrence = self.is(id, "shape_aspect_occurrence");
        let mut prov = vec![id];
        prov.extend(self.id_attributes(id));

        // Usages and their anchors.
        let mut anchors: Vec<Anchor> = Vec::new();
        let mut unresolved: Vec<u64> = Vec::new();
        let mut placed_definition = Vec::new();
        let derived_shape = names.iter().any(|n| DerivedKind::of_entity(n).is_some());
        let mut geometry_failures = Vec::new();
        for (u, rep, items) in self.usages(id) {
            prov.push(u);
            for item in items {
                match self.anchor(st, item) {
                    Some(a) => anchors.push(a),
                    None if occurrence && self.is(item, "mapped_item") => {
                        placed_definition.push((item, rep));
                    }
                    None if !derived_shape && self.is_supplemental(item) => {
                        match self.geometry(st, item, rep) {
                            Ok(g) => anchors.push(Anchor::Geometry(g)),
                            Err(why) => {
                                geometry_failures.push(why);
                                unresolved.push(item);
                            }
                        }
                    }
                    None => unresolved.push(item),
                }
            }
        }
        if occurrence {
            // An occurrence of a feature definition (edition 4 holes): its faces are the
            // feature; a basic_round_hole's parameters and placement are held as its Hole,
            // any other definition is not.
            let definition = self.get_ref(id, "shape_aspect_occurrence", "definition");
            let held = match definition {
                Some(d) if self.is(d, "basic_round_hole") => {
                    let mut c = Vec::new();
                    self.round_hole(st, d, &placed_definition, &mut c)
                        .map(|h| (h, c))
                }
                Some(d) => Err(format!(
                    "{} is not a basic_round_hole, the one definition held",
                    self.entity_label(d)
                )),
                None => Err("the occurrence has no definition".to_string()),
            };
            let why = match held {
                Ok((h, c)) => {
                    prov.extend(c);
                    st.holes.insert(id, h);
                    None
                }
                Err(why) => Some(why),
            };
            if let Some(why) = why {
                let mut named: Vec<u64> = definition.into_iter().collect();
                named.extend(placed_definition.iter().map(|&(m, _)| m));
                // The definition's own semantic parts (a hole's TOLERANCE_VALUE) go with it.
                if let Some(d) = definition
                    && let Some(e) = self.doc.get(d)
                {
                    let mut fw = Vec::new();
                    visit_attributes(e, &mut |a| {
                        if let Attribute::EntityRef(n) = a {
                            fw.push(*n);
                        }
                    });
                    for n in fw {
                        if self.family(n) == Family::SemanticPmi {
                            named.push(n);
                        }
                    }
                }
                let def_type = definition.map(|d| self.entity_label(d)).unwrap_or_default();
                self.find(
                    FindingKind::NotModelled,
                    part,
                    id,
                    &named,
                    format!("the feature definition of the occurrence ({def_type}: its parameters, tolerances and placement) is not held: {why}; the occurrence's faces are read as the feature"),
                );
            }
        }

        // Derived features.
        let derived = names
            .iter()
            .filter_map(|n| DerivedKind::of_entity(n))
            .max_by_key(|k| *k != DerivedKind::Derived);
        if let Some(kind) = derived {
            let mut from = Vec::new();
            for r in self.referrers_by(
                id,
                "shape_aspect_deriving_relationship",
                "shape_aspect_relationship",
                "relating_shape_aspect",
            ) {
                let Some(base) =
                    self.get_ref(r, "shape_aspect_relationship", "related_shape_aspect")
                else {
                    continue;
                };
                match self.feature(st, base) {
                    Some(f) => {
                        from.push(f);
                        prov.push(r);
                    }
                    None => {
                        self.find(
                            FindingKind::Unresolved,
                            part,
                            id,
                            &[r, base],
                            "derived feature's base does not resolve; not read",
                        );
                        return Built::None;
                    }
                }
            }
            if from.is_empty() {
                self.find(
                    FindingKind::Nonconformance,
                    part,
                    id,
                    &[],
                    "derived shape aspect without a shape_aspect_deriving_relationship (§5.1.4): no deriving faces; explicit geometry alone cannot establish a derivation; not read",
                );
                return Built::None;
            }
            if kind == DerivedKind::Derived {
                let geometry = self.derived_geometry(st, id, &from);
                return Built::Feature(
                    Feature::Derived {
                        kind,
                        from,
                        geometry,
                    },
                    prov,
                );
            }
            if !anchors.is_empty() || !unresolved.is_empty() {
                let mut items: Vec<u64> = unresolved.clone();
                items.sort_unstable();
                self.find(
                    FindingKind::NotModelled,
                    part,
                    id,
                    &items,
                    "explicit geometry of the derived feature (supplemental geometry) is not held; the derivation is",
                );
            }
            return Built::Feature(
                Feature::Derived {
                    kind,
                    from,
                    geometry: None,
                },
                prov,
            );
        }

        if !unresolved.is_empty() {
            let types: Vec<String> = unresolved.iter().map(|&i| self.entity_label(i)).collect();
            let mut detail = format!(
                "items that are not faces, edges or geometry the reader holds: {}; feature not read",
                types.join(", ")
            );
            if !geometry_failures.is_empty() {
                detail.push_str(&format!(" ({})", geometry_failures.join("; ")));
            }
            self.find(FindingKind::Unresolved, part, id, &unresolved, detail);
            return Built::None;
        }

        let members = self.members(id);
        let group_kind = if names.iter().any(|n| n == "all_around_shape_aspect") {
            Some(GroupKind::AllAround)
        } else if names.iter().any(|n| n == "between_shape_aspect") {
            Some(GroupKind::Between)
        } else if names.iter().any(|n| n == "composite_shape_aspect")
            && !self.is(id, "composite_group_shape_aspect")
            && [
                self.get_str(id, "shape_aspect", "name").unwrap_or_default(),
                description.clone().unwrap_or_default(),
            ]
            .iter()
            .any(|t| t == "multiple elements" || t == "pattern of features")
        {
            let pattern = [
                self.get_str(id, "shape_aspect", "name").unwrap_or_default(),
                description.clone().unwrap_or_default(),
            ]
            .iter()
            .any(|t| t == "pattern of features");
            self.find(
                FindingKind::Nonconformance,
                part,
                id,
                &[],
                "a composite_shape_aspect stating 'multiple elements' or 'pattern of features' (§6.4: composite_group_shape_aspect); read as that group",
            );
            Some(if pattern {
                GroupKind::PatternOfFeatures
            } else {
                GroupKind::MultipleElements
            })
        } else if self.is(id, "composite_group_shape_aspect") {
            let name = self.get_str(id, "shape_aspect", "name").unwrap_or_default();
            let d = description.clone().unwrap_or_default();
            Some(
                if name == "pattern of features" || d == "pattern of features" {
                    GroupKind::PatternOfFeatures
                } else if name == "multiple elements" || d == "multiple elements" {
                    GroupKind::MultipleElements
                } else {
                    self.find(
                        FindingKind::Nonconformance,
                        part,
                        id,
                        &[],
                        "composite_group_shape_aspect names neither 'multiple elements' nor 'pattern of features' (§6.4); read as a group of unstated kind",
                    );
                    GroupKind::Unstated
                },
            )
        } else {
            None
        };

        if let Some(kind) = group_kind {
            if !anchors.is_empty() {
                self.find(
                    FindingKind::Unsupported,
                    part,
                    id,
                    &[],
                    "a group with geometry of its own besides its members; not read",
                );
                return Built::None;
            }
            let mut fs = Vec::new();
            for (r, m) in members {
                match self.feature(st, m) {
                    Some(f) => {
                        fs.push(f);
                        prov.push(r);
                    }
                    None => {
                        self.find(
                            FindingKind::Unresolved,
                            part,
                            id,
                            &[r, m],
                            "a member of the group does not resolve; group not read",
                        );
                        return Built::None;
                    }
                }
            }
            if fs.is_empty() {
                self.find(
                    FindingKind::Nonconformance,
                    part,
                    id,
                    &[],
                    "a group without members; not read",
                );
                return Built::None;
            }
            return Built::Feature(Feature::Group { members: fs, kind }, prov);
        }

        if members.is_empty() {
            if anchors.is_empty() {
                self.find(
                    FindingKind::Nonconformance,
                    part,
                    id,
                    &[],
                    "shape aspect identifies no face or edge; not read",
                );
                return Built::None;
            }
            return Built::Feature(Feature::Items(dedup_anchors(anchors)), prov);
        }

        // A feature made of members: one feature of several items (§5.1 Figure 5, a datum
        // feature on a whole pattern, §6.5.2), or the pre-4.0.6 datum feature form whose one
        // member is the group (§6.5.2).
        if anchors.is_empty() && members.len() == 1 {
            let (r, m) = members[0];
            let member_is_composite = self.is(m, "composite_group_shape_aspect")
                || self.is(m, "all_around_shape_aspect")
                || self.is(m, "between_shape_aspect");
            if member_is_composite {
                prov.push(r);
                return Built::Alias(m, prov);
            }
        }
        for (r, m) in members {
            match self.content(st, m) {
                Some((Feature::Items(items), p)) => {
                    anchors.extend(items);
                    prov.push(r);
                    prov.extend(p);
                    st.absorbed.insert(m);
                }
                Some(_) => {
                    self.find(
                        FindingKind::Unsupported,
                        part,
                        id,
                        &[r, m],
                        "a single feature whose member is a group or derived feature; not read",
                    );
                    return Built::None;
                }
                None => {
                    self.find(
                        FindingKind::Unresolved,
                        part,
                        id,
                        &[r, m],
                        "a member of the feature does not resolve; not read",
                    );
                    return Built::None;
                }
            }
        }
        Built::Feature(Feature::Items(dedup_anchors(anchors)), prov)
    }

    /// A `basic_round_hole` definition `def` and the placement of its occurrence, from the
    /// occurrence's mapped items (with the representations they are identified in), or why
    /// it is not held. The ids read go to `consumed` (on success only).
    fn round_hole(
        &mut self,
        st: &PartState,
        def: u64,
        placed: &[(u64, Option<u64>)],
        consumed: &mut Vec<u64>,
    ) -> Result<(RoundHole, Option<Placement>), String> {
        const E: &str = "basic_round_hole";
        let mut c = vec![def];
        // What else refers to the definition (a round_hole_bottom_condition, a property of
        // it) is not held.
        let others: Vec<u64> = self
            .doc
            .referrers(def)
            .to_vec()
            .into_iter()
            .filter(|&r| {
                !(self.is(r, "shape_aspect_occurrence")
                    && self.get_ref(r, "shape_aspect_occurrence", "definition") == Some(def))
            })
            .collect();
        if !others.is_empty() {
            let labels: Vec<String> = others
                .iter()
                .map(|&r| format!("{} #{r}", self.entity_label(r)))
                .collect();
            return Err(format!(
                "what refers to the definition is not held: {}",
                labels.join(", ")
            ));
        }
        let name = self
            .get_str(def, "characterized_object", "name")
            .unwrap_or_default();
        let length = |r: &mut Self, attr: &str, c: &mut Vec<u64>| -> Result<_, String> {
            let Some(m) = r.get_ref(def, E, attr) else {
                return Ok(None);
            };
            match r.measure(m) {
                Ok(Measured::Length(l)) => {
                    c.push(m);
                    Ok(Some(l))
                }
                Ok(other) => Err(format!("its {attr} #{m} is not a length ({other:?})")),
                Err(e) => Err(format!("its {attr} #{m} is not read ({})", e.why)),
            }
        };
        let diameter = length(self, "diameter", &mut c)?.ok_or("it has no diameter")?;
        let depth = length(self, "depth", &mut c)?;
        let bounds = |r: &mut Self, attr: &str, c: &mut Vec<u64>| -> Result<_, String> {
            let Some(t) = r.get_ref(def, E, attr) else {
                return Ok(None);
            };
            if !r.is(t, "tolerance_value") {
                return Err(format!("its {attr} is a {}", r.entity_label(t)));
            }
            let (Some(lo), Some(hi)) = (
                r.get_ref(t, "tolerance_value", "lower_bound"),
                r.get_ref(t, "tolerance_value", "upper_bound"),
            ) else {
                return Err(format!("its {attr} #{t} lacks a bound"));
            };
            let mut v = Vec::new();
            let (Some(lo_v), Some(hi_v)) = (
                r.value(st, lo, Want::Length, &mut v),
                r.value(st, hi, Want::Length, &mut v),
            ) else {
                return Err(format!(
                    "its {attr} #{t} is not read (its finding says why)"
                ));
            };
            let b = Bounds::new(hi_v, lo_v)
                .map_err(|e| format!("its {attr} #{t}: {} (tolerance_value WR1)", e.0))?;
            c.push(t);
            c.extend(v);
            Ok(Some(b))
        };
        let diameter_tolerance = bounds(self, "diameter_tolerance", &mut c)?;
        let depth_tolerance = bounds(self, "depth_tolerance", &mut c)?;
        let through = match enum_value(self.get(def, E, "through_hole")) {
            Some("T") => true,
            Some("F") => false,
            v => return Err(format!("its through_hole {v:?} is not a boolean")),
        };
        // WR1, WR2: the placement is the representation's one item, an axis2_placement_3d.
        let rep = self
            .get_ref(def, E, "placement")
            .ok_or("it has no placement")?;
        let axis = match self.get_refs(rep, "representation", "items")[..] {
            [a] if self.is(a, "axis2_placement_3d") => a,
            _ => {
                return Err(format!(
                    "its placement #{rep} is not one axis2_placement_3d (WR1, WR2)"
                ));
            }
        };
        let unit = self.context_length_unit(rep);
        let placement = self
            .placement(axis, unit)
            .map_err(|e| format!("its placement #{axis}: {e}"))?;
        c.extend([rep, axis]);
        // The occurrence's placement: the one mapped item mapping the definition's placement.
        let located = match placed {
            [] => None,
            [(mi, used)] => {
                let map = self.get_ref(*mi, "mapped_item", "mapping_source");
                let origin =
                    map.and_then(|m| self.get_ref(m, "representation_map", "mapping_origin"));
                let mapped = map
                    .and_then(|m| self.get_ref(m, "representation_map", "mapped_representation"));
                if origin != Some(axis) || mapped != Some(rep) {
                    return Err(format!(
                        "the occurrence's mapped item #{mi} does not map the definition's placement"
                    ));
                }
                let target = self
                    .get_ref(*mi, "mapped_item", "mapping_target")
                    .filter(|&t| self.is(t, "axis2_placement_3d"))
                    .ok_or_else(|| {
                        format!("the occurrence's mapped item #{mi} targets no axis2_placement_3d")
                    })?;
                let unit = used.and_then(|u| self.context_length_unit(u));
                let p = self
                    .placement(target, unit)
                    .map_err(|e| format!("the occurrence's placement #{target}: {e}"))?;
                c.extend([*mi]);
                c.extend(map);
                Some(p)
            }
            _ => {
                return Err(format!(
                    "the occurrence states {} mapped items; which places it is not stated",
                    placed.len()
                ));
            }
        };
        if through == depth.is_some() {
            self.find(
                FindingKind::Nonconformance,
                Some(st.id),
                def,
                &[],
                "a hole that is through and has a depth, or neither (basic_round_hole WR7); read as stated",
            );
        }
        consumed.extend(c);
        Ok((
            RoundHole {
                name,
                diameter,
                diameter_tolerance,
                depth,
                depth_tolerance,
                through,
                placement,
            },
            located,
        ))
    }

    /// Whether `item` is geometry that is not part topology: points, curves, surfaces,
    /// placements and sets of them (supplemental geometry), as opposed to faces, edges,
    /// shells, solids and mapped items.
    fn is_supplemental(&mut self, item: u64) -> bool {
        let topo = [
            "topological_representation_item",
            "solid_model",
            "mapped_item",
            "shell_based_surface_model",
            "face_based_surface_model",
        ];
        if topo.iter().any(|t| self.is(item, t)) {
            return false;
        }
        [
            "point",
            "curve",
            "surface",
            "placement",
            "geometric_set",
            "geometric_curve_set",
            "direction",
        ]
        .iter()
        .any(|t| self.is(item, t))
    }

    /// Supplemental geometry item `item` of representation `rep`, held by value (memoised).
    fn geometry(
        &mut self,
        st: &mut PartState,
        item: u64,
        rep: Option<u64>,
    ) -> Result<GeometryId, String> {
        if let Some(&g) = st.geometry.get(&item) {
            return Ok(g);
        }
        let mut ids = Vec::new();
        let tree = self.geometry_item(item, 0, &mut ids)?;
        ids.extend(self.id_attributes(item));
        let length_unit = rep.and_then(|r| self.context_length_unit(r));
        let g = GeometryId(st.pmi.geometry.len());
        st.pmi.geometry.push(Geometry {
            item: tree,
            length_unit,
        });
        st.prov.geometry.push(self::ids(ids));
        st.prov.geometry_items.push(item);
        st.geometry.insert(item, g);
        let _identifier = self.identifier(st.id, item);
        #[cfg(feature = "json")]
        {
            self.json_carriers
                .insert((st.id, "geometry".into(), g.0), item);
            st.identifiers.geometry.push(_identifier);
        }
        Ok(g)
    }

    fn geometry_item(
        &mut self,
        id: u64,
        depth: usize,
        ids: &mut Vec<u64>,
    ) -> Result<GeometryItem, String> {
        if depth > 64 {
            return Err(format!("geometry #{id} nests too deep"));
        }
        if !(self.is(id, "geometric_representation_item") || self.is(id, "founded_item")) {
            return Err(format!("#{id} ({}) is not geometry", self.entity_label(id)));
        }
        ids.push(id);
        let e = self
            .doc
            .get(id)
            .ok_or_else(|| format!("#{id} does not exist"))?;
        let leaves: Vec<(String, &'a [Attribute])> = match e {
            RawEntity::Simple {
                name, attributes, ..
            } => vec![(name.clone(), attributes.as_slice())],
            RawEntity::Complex { parts, .. } => parts
                .iter()
                .map(|p| (p.name.clone(), p.attributes.as_slice()))
                .collect(),
        };
        let mut out = Vec::new();
        for (name, attrs) in leaves {
            let mut values = Vec::new();
            for a in attrs {
                values.push(self.geometry_value(id, a, depth, ids)?);
            }
            out.push((name, values));
        }
        Ok(GeometryItem { leaves: out })
    }

    fn geometry_value(
        &mut self,
        owner: u64,
        a: &'a Attribute,
        depth: usize,
        ids: &mut Vec<u64>,
    ) -> Result<GeometryValue, String> {
        Ok(match a {
            Attribute::Integer(i) => GeometryValue::Integer(*i),
            Attribute::Real(_) => {
                let t = self
                    .real_text(owner, a)
                    .ok_or_else(|| format!("#{owner}: a real's text could not be read"))?;
                GeometryValue::Real(Decimal::parse(&t).map_err(|e| e.0)?)
            }
            Attribute::String(s) => GeometryValue::Text(s.clone()),
            Attribute::Enum(s) => GeometryValue::Enum(s.clone()),
            Attribute::Binary(s) => GeometryValue::Binary(s.clone()),
            Attribute::EntityRef(n) => {
                GeometryValue::Item(Box::new(self.geometry_item(*n, depth + 1, ids)?))
            }
            Attribute::Unset => GeometryValue::Unset,
            Attribute::Derived => GeometryValue::Derived,
            Attribute::List(l) => {
                let mut v = Vec::new();
                for x in l {
                    v.push(self.geometry_value(owner, x, depth, ids)?);
                }
                GeometryValue::List(v)
            }
            Attribute::Typed { type_name, value } => GeometryValue::Typed {
                type_name: type_name.clone(),
                value: Box::new(self.geometry_value(owner, value, depth, ids)?),
            },
        })
    }

    /// A face or edge of the part, by its `#N`.
    fn anchor(&self, st: &PartState, item: u64) -> Option<Anchor> {
        if let Some(f) = st.part.face_index(item) {
            return Some(Anchor::Face(FaceIndex(f)));
        }
        st.part.edge_index(item).map(|e| Anchor::Edge(EdgeIndex(e)))
    }

    // --- datums ---------------------------------------------------------------------------

    fn read_datums(&mut self, st: &mut PartState, datums: &[u64]) {
        let part = Some(st.id);
        for &d in datums {
            let label_text = self
                .get_str(d, "datum", "identification")
                .unwrap_or_default();
            if self.is(d, "common_datum") {
                self.find(
                    FindingKind::Unsupported,
                    part,
                    d,
                    &[],
                    "common_datum (a datum established by datums) is not read",
                );
                continue;
            }
            let label = match DatumLabel::new(&label_text) {
                Ok(l) => l,
                Err(e) => {
                    self.find(FindingKind::Nonconformance, part, d, &[], e.0);
                    continue;
                }
            };
            let mut prov = vec![d];
            prov.extend(self.id_attributes(d));
            let mut feature = None;
            let mut targets = Vec::new();
            let mut failed = false;
            let rels = self.referrers_by(
                d,
                "shape_aspect_relationship",
                "shape_aspect_relationship",
                "related_shape_aspect",
            );
            for r in rels {
                let Some(src) =
                    self.get_ref(r, "shape_aspect_relationship", "relating_shape_aspect")
                else {
                    continue;
                };
                if self.is(src, "datum_target") {
                    match self.datum_target(st, src) {
                        Some(t) => {
                            targets.push(t);
                            prov.push(r);
                        }
                        None => failed = true,
                    }
                } else if self.is(src, "datum_feature") {
                    if feature.is_some() {
                        self.find(
                            FindingKind::Nonconformance,
                            part,
                            d,
                            &[src],
                            "datum established by more than one datum feature (datum WR2); not read",
                        );
                        failed = true;
                        continue;
                    }
                    if !st.features.contains_key(&src) {
                        self.datum_feature_on_datum(st, d, src);
                    }
                    match self.feature(st, src) {
                        Some(f) => {
                            if !feature_has_face(&st.pmi, f) {
                                self.find(
                                    FindingKind::Unresolved,
                                    part,
                                    d,
                                    &[src],
                                    format!("datum {label} feature resolves to no faces"),
                                );
                            }
                            feature = Some(f);
                            prov.push(r);
                        }
                        None => failed = true,
                    }
                } else {
                    self.find(
                        FindingKind::Unsupported,
                        part,
                        r,
                        &[d, src],
                        "relationship to a datum from something that is neither a datum feature nor a datum target",
                    );
                }
            }
            if failed || (feature.is_none() && targets.is_empty()) {
                self.find(
                    FindingKind::Unresolved,
                    part,
                    d,
                    &[],
                    format!("datum {label} is not read: what establishes it does not resolve"),
                );
                continue;
            }
            // The datum's own usages: its theoretical geometry (§6.5.1 example: 'datum plane'),
            // not held; or, where the datum feature had none, read as the feature's faces.
            for (u, _, items) in self.usages(d) {
                prov.push(u);
                if !st.absorbed.contains(&u) {
                    self.find(
                        FindingKind::NotModelled,
                        part,
                        d,
                        &[u],
                        format!("the datum's own geometry ({}) is not held (the datum is established by its feature or targets)", items.iter().map(|&i| self.entity_label(i)).collect::<Vec<_>>().join(", ")),
                    );
                }
            }
            // One datum per label per part: a second instance of the label (datum UR1) is the
            // same datum when it is established by the same feature content.
            if let Some((&other, &existing)) = st
                .datums
                .iter()
                .find(|&(_, di)| st.pmi.datums[di.0].label() == &label)
            {
                let same_feature = match (st.pmi.datums[existing.0].feature(), feature) {
                    (Some(a), Some(b)) => st.pmi.features[a.0] == st.pmi.features[b.0],
                    (None, None) => true,
                    _ => false,
                };
                if same_feature {
                    let old = &st.pmi.datums[existing.0];
                    let mut ts = old.targets().to_vec();
                    for t in &targets {
                        if !ts.contains(t) {
                            ts.push(*t);
                        }
                    }
                    let f = old.feature();
                    #[allow(
                        clippy::expect_used,
                        reason = "the old datum was established, and this keeps its feature and adds targets"
                    )]
                    let datum = Datum::new(label.clone(), f, ts).expect("datum stays established");
                    st.pmi.datums[existing.0] = datum;
                    let p = &mut st.prov.datums[existing.0];
                    p.extend(prov);
                    *p = ids(std::mem::take(p));
                    let identifier = self.identifier(st.id, d);
                    if identifier.is_some() || st.identifiers.datums[existing.0].is_some() {
                        st.identifiers.datums[existing.0] = None;
                        self.find(FindingKind::Nonconformance, part, d, &[other],
                            "merged datum has more than one requirement entity; identifier not read");
                    }
                    st.datums.insert(d, existing);
                    self.find(
                        FindingKind::Nonconformance,
                        part,
                        d,
                        &[other],
                        format!(
                            "datum {label} stated twice on one part (datum UR1); read as one datum (same feature)"
                        ),
                    );
                } else {
                    self.find(
                        FindingKind::Conflict,
                        part,
                        d,
                        &[other],
                        format!("datum {label} stated twice on one part with different features; the second is not read"),
                    );
                }
                continue;
            }
            match Datum::new(label, feature, targets) {
                Ok(datum) => {
                    let id = DatumId(st.pmi.datums.len());
                    st.pmi.datums.push(datum);
                    st.prov.datums.push(ids(prov));
                    st.identifiers.datums.push(self.item_identifier(
                        st.id,
                        "datums",
                        st.pmi.datums.len() - 1,
                        d,
                    ));
                    st.datums.insert(d, id);
                }
                Err(e) => self.find(FindingKind::Nonconformance, part, d, &[], e.0),
            }
        }
    }

    /// A datum feature that identifies nothing itself, of a datum whose own usages identify
    /// faces or edges of the part (NIST CTC-02 datum D): the faces are read as the datum
    /// feature's, with a nonconformance finding (the practice puts them on the datum feature,
    /// §6.5.1 Figure 36).
    fn datum_feature_on_datum(&mut self, st: &mut PartState, datum: u64, feature: u64) {
        if !self.usages(feature).is_empty() || !self.members(feature).is_empty() {
            return;
        }
        let usages = self.usages(datum);
        if usages.is_empty() {
            return;
        }
        let mut anchors = Vec::new();
        let mut prov = vec![feature];
        prov.extend(self.id_attributes(feature));
        for (u, _, items) in &usages {
            for &i in items {
                match self.anchor(st, i) {
                    Some(a) => anchors.push(a),
                    None => return,
                }
            }
            prov.push(*u);
            st.absorbed.insert(*u);
        }
        self.find(
            FindingKind::Nonconformance,
            Some(st.id),
            feature,
            &usages.iter().map(|u| u.0).collect::<Vec<_>>(),
            "the datum feature identifies no face; its datum's usage does (§6.5.1 puts it on the datum feature): read as the datum feature's faces",
        );
        let fid = FeatureId(st.pmi.features.len());
        st.pmi.features.push(Feature::Items(dedup_anchors(anchors)));
        st.prov.features.push(ids(prov));
        st.identifiers.features.push(self.item_identifier(
            st.id,
            "features",
            st.pmi.features.len() - 1,
            feature,
        ));
        st.features.insert(feature, Some(fid));
    }

    fn datum_target(&mut self, st: &mut PartState, id: u64) -> Option<DatumTargetId> {
        if let Some(t) = st.targets.get(&id) {
            return *t;
        }
        st.targets.insert(id, None);
        let r = self.build_target(st, id);
        st.targets.insert(id, r);
        if r.is_none() {
            self.not_read(st, id);
        }
        r
    }

    fn build_target(&mut self, st: &mut PartState, id: u64) -> Option<DatumTargetId> {
        let part = Some(st.id);
        let number_text = self
            .get_str(id, "datum_target", "target_id")
            .unwrap_or_default();
        let Ok(number) = number_text.trim().parse::<u32>() else {
            self.find(
                FindingKind::Nonconformance,
                part,
                id,
                &[],
                format!("target_id {number_text:?} is not a positive number (datum_target WR3); not read"),
            );
            return None;
        };
        let description = self
            .get_str(id, "shape_aspect", "description")
            .unwrap_or_default();
        let mut prov = vec![id];
        prov.extend(self.id_attributes(id));
        let placed = self.is(id, "placed_datum_target_feature");

        // Parameters: property_definition → shape representation with parameters (§6.6.1).
        let mut placement = None;
        let mut movable = None;
        let mut lengths: BTreeMap<String, Length> = BTreeMap::new();
        let pds_ = self.referrers_by(
            id,
            "property_definition",
            "property_definition",
            "definition",
        );
        for pdef in pds_ {
            if self.family(pdef) == Family::ValidationProperty {
                continue;
            }
            let reps = self.property_representations(pdef);
            if reps.is_empty() {
                // An empty property definition carries nothing (NIST files write one per
                // explicit target).
                prov.push(pdef);
                continue;
            }
            let mut used = false;
            for (pdr, rep) in reps {
                if !self.is(rep, "shape_representation_with_parameters") {
                    continue;
                }
                used = true;
                prov.extend([pdef, pdr, rep]);
                let ctx_unit = self.context_length_unit(rep);
                for item in self.get_refs(rep, "representation", "items") {
                    let name = self.name_of(item);
                    if self.is(item, "axis2_placement_3d") && name == "orientation" {
                        match self.placement(item, ctx_unit.clone()) {
                            Ok(p) => placement = Some(p),
                            Err(why) => {
                                self.find(FindingKind::Unresolved, part, id, &[item], why);
                                return None;
                            }
                        }
                    } else if self.is(item, "direction") && name == "movable direction" {
                        match self.direction(item) {
                            Some(d) => movable = Some(d),
                            None => {
                                self.find(
                                    FindingKind::Nonconformance,
                                    part,
                                    id,
                                    &[item],
                                    "movable direction without three ratios; target not read",
                                );
                                return None;
                            }
                        }
                    } else if self.is(item, "measure_representation_item") {
                        let mut c = Vec::new();
                        let v = self.value(st, item, Want::Length, &mut c)?;
                        prov.extend(c);
                        if let Quantity::Length(l) = v.quantity {
                            lengths.insert(name, l);
                        }
                    } else {
                        self.find(
                            FindingKind::Unsupported,
                            part,
                            id,
                            &[item],
                            format!("datum target parameter {name:?} is not one §6.6 defines"),
                        );
                    }
                }
            }
            if !used
                && self.get_str(pdef, "property_definition", "name").as_deref()
                    != Some("semantic text")
                && self
                    .referrers_by(
                        pdef,
                        "general_property_association",
                        "general_property_association",
                        "derived_definition",
                    )
                    .is_empty()
            {
                self.find(
                    FindingKind::Unsupported,
                    part,
                    pdef,
                    &[id],
                    "property of a datum target that is not its shape representation with parameters",
                );
            }
        }
        let mut length = |n: &str| lengths.remove(n);
        let shape = match (placed, description.as_str()) {
            (true, "point") => TargetShape::Point,
            (true, "line") => match length("target length") {
                Some(l) => TargetShape::Line { length: l },
                None => return self.missing_param(st, id, "target length"),
            },
            (true, "rectangle") => match (length("target length"), length("target width")) {
                (Some(l), Some(w)) => TargetShape::Rectangle {
                    length: l,
                    width: w,
                },
                _ => return self.missing_param(st, id, "target length and target width"),
            },
            (true, "circle") => match length("target diameter") {
                Some(d) => TargetShape::Circle { diameter: d },
                None => return self.missing_param(st, id, "target diameter"),
            },
            (true, "circular curve") => match length("target diameter") {
                Some(d) => TargetShape::CircularCurve { diameter: d },
                None => return self.missing_param(st, id, "target diameter"),
            },
            (false, "area") | (false, "curve") => {
                let f = self.feature(st, id)?;
                if description == "area" {
                    TargetShape::Area(f)
                } else {
                    TargetShape::Curve(f)
                }
            }
            _ => {
                self.find(
                    FindingKind::Nonconformance,
                    part,
                    id,
                    &[],
                    format!(
                        "datum target description {description:?} is not one §6.6 defines for {}; not read",
                        if placed { "a placed target" } else { "an explicit target" }
                    ),
                );
                return None;
            }
        };
        if !lengths.is_empty() {
            let names: Vec<&String> = lengths.keys().collect();
            self.find(
                FindingKind::Nonconformance,
                part,
                id,
                &[],
                format!("parameters {names:?} do not belong to a {description} target; ignored"),
            );
        }
        // The feature the target lies on (§6.6.3).
        let mut on = None;
        for r in self.referrers_by(
            id,
            "feature_for_datum_target_relationship",
            "shape_aspect_relationship",
            "related_shape_aspect",
        ) {
            let Some(f) = self.get_ref(r, "shape_aspect_relationship", "relating_shape_aspect")
            else {
                continue;
            };
            if on.is_some() {
                self.find(
                    FindingKind::Nonconformance,
                    part,
                    r,
                    &[id],
                    "a second feature_for_datum_target_relationship for one target (UR1); ignored",
                );
                continue;
            }
            match self.feature(st, f) {
                Some(fid) => {
                    on = Some(fid);
                    prov.push(r);
                }
                None => {
                    self.find(
                        FindingKind::Unresolved,
                        part,
                        id,
                        &[r, f],
                        "the target's feature does not resolve; target not read",
                    );
                    return None;
                }
            }
        }
        match DatumTarget::new(number, shape, placement, movable, on) {
            Ok(t) => {
                let tid = DatumTargetId(st.pmi.datum_targets.len());
                st.pmi.datum_targets.push(t);
                st.prov.datum_targets.push(ids(prov));
                st.identifiers.datum_targets.push(self.item_identifier(
                    st.id,
                    "datum_targets",
                    st.pmi.datum_targets.len() - 1,
                    id,
                ));
                Some(tid)
            }
            Err(e) => {
                self.find(FindingKind::Nonconformance, part, id, &[], e.0);
                None
            }
        }
    }

    fn missing_param<T>(&mut self, st: &PartState, id: u64, what: &str) -> Option<T> {
        self.find(
            FindingKind::Nonconformance,
            Some(st.id),
            id,
            &[],
            format!("placed datum target without its {what} (§6.6.1 Table 10); not read"),
        );
        None
    }

    /// The length unit of a representation's context (`global_unit_assigned_context`).
    fn context_length_unit(&mut self, rep: u64) -> Option<LengthUnit> {
        let ctx = self.get_ref(rep, "representation", "context_of_items")?;
        for u in self.get_refs(ctx, "global_unit_assigned_context", "units") {
            if let Ok(Unit::Length(l)) = self.unit(u) {
                return Some(l);
            }
        }
        None
    }

    fn placement(&mut self, id: u64, unit: Option<LengthUnit>) -> Result<Placement, String> {
        let unit = unit.ok_or("the placement's context has no length unit")?;
        let point = self
            .get_ref(id, "placement", "location")
            .ok_or("placement without a location")?;
        let c = match self.get(point, "cartesian_point", "coordinates") {
            Some(Attribute::List(l)) if l.len() == 3 => l,
            _ => return Err(format!("location #{point} is not a 3D cartesian point")),
        };
        let mut origin = Vec::new();
        for a in c {
            let t = self
                .real_text(point, a)
                .ok_or_else(|| format!("coordinate of #{point} is not a number"))?;
            origin.push(Length {
                value: Decimal::parse(&t).map_err(|e| e.0)?,
                unit: unit.clone(),
            });
        }
        let origin: [Length; 3] = origin.try_into().map_err(|_| "three coordinates")?;
        let axis = match self.get_ref(id, "axis2_placement_3d", "axis") {
            Some(a) => Some(self.direction(a).ok_or("axis is not a 3D direction")?),
            None => None,
        };
        let ref_direction = match self.get_ref(id, "axis2_placement_3d", "ref_direction") {
            Some(a) => Some(
                self.direction(a)
                    .ok_or("ref_direction is not a 3D direction")?,
            ),
            None => None,
        };
        Ok(Placement {
            origin,
            axis,
            ref_direction,
        })
    }

    fn direction(&mut self, id: u64) -> Option<Direction> {
        let l = match self.get(id, "direction", "direction_ratios")? {
            Attribute::List(l) if l.len() == 3 => l,
            _ => return None,
        };
        let mut out = Vec::new();
        for a in l {
            out.push(Decimal::parse(&self.real_text(id, a)?).ok()?);
        }
        Some(Direction(out.try_into().ok()?))
    }

    /// The (property_definition_representation, representation) pairs of a property.
    fn property_representations(&mut self, pdef: u64) -> Vec<(u64, u64)> {
        let mut out = Vec::new();
        for pdr in self.referrers_by(
            pdef,
            "property_definition_representation",
            "property_definition_representation",
            "definition",
        ) {
            if let Some(rep) = self.get_ref(
                pdr,
                "property_definition_representation",
                "used_representation",
            ) {
                out.push((pdr, rep));
            }
        }
        out
    }

    // --- dimensions -----------------------------------------------------------------------

    fn dimension(&mut self, st: &mut PartState, id: u64) -> Option<DimensionId> {
        if let Some(d) = st.dimensions.get(&id) {
            return *d;
        }
        st.dimensions.insert(id, None);
        let r = self.build_dimension(st, id);
        st.dimensions.insert(id, r);
        if r.is_none() {
            self.not_read(st, id);
        }
        r
    }

    /// An item not read whose reason is a finding on one of its parts (a measure without a
    /// unit, …): the item itself is named too, so that what is not read is said.
    fn not_read(&mut self, st: &PartState, id: u64) {
        if !self.findings.iter().any(|f| f.ids.contains(&id)) {
            self.find(
                FindingKind::Unresolved,
                Some(st.id),
                id,
                &[],
                "not read: a value or reference of it does not resolve (see the findings on its parts)",
            );
        }
    }

    fn build_dimension(&mut self, st: &mut PartState, id: u64) -> Option<DimensionId> {
        let part = Some(st.id);
        let mut prov = vec![id];
        prov.extend(self.id_attributes(id));
        let angle = |s: Option<&str>| match s.map(str::to_ascii_uppercase).as_deref() {
            Some("EQUAL") => Some(AngleSelection::Equal),
            Some("LARGE") => Some(AngleSelection::Large),
            Some("SMALL") => Some(AngleSelection::Small),
            _ => None,
        };
        let kind = if self.is(id, "dimensional_size") {
            let name = self
                .get_str(id, "dimensional_size", "name")
                .unwrap_or_default();
            let target = self.get_ref(id, "dimensional_size", "applies_to")?;
            let feature = self.feature(st, target);
            let Some(feature) = feature else {
                self.find(
                    FindingKind::Unresolved,
                    part,
                    id,
                    &[target],
                    "the dimensioned feature does not resolve; dimension not read",
                );
                return None;
            };
            let angular = self.is(id, "angular_size");
            let sel = if angular {
                let s = enum_value(self.get(id, "angular_size", "angle_selection"));
                angle(s)
            } else {
                None
            };
            if angular && sel.is_none() {
                self.find(
                    FindingKind::Nonconformance,
                    part,
                    id,
                    &[],
                    "angular size without a valid angle_selection; not read",
                );
                return None;
            }
            let path = self.dimension_path(st, id, "dimensional_size_with_path")?;
            let kind = SizeKind::from_name(&name);
            if kind == SizeKind::Diameter
                && let Feature::Derived {
                    geometry: Some(g), ..
                } = &st.pmi.features[feature.0]
                && st.pmi.derived_geometry[g.0].radius.is_none()
            {
                self.find(FindingKind::NotModelled, part, id, &[target],
                    "diameter target has no cylinder radius: deriving faces must belong to one cylinder; semantic value retained");
            }
            if !angular && matches!(kind, SizeKind::Other(_)) {
                self.find(
                    FindingKind::Nonconformance,
                    part,
                    id,
                    &[],
                    format!(
                        "dimensional_size name {name:?} is not in Table 4 (§5.1.5); kept as stated"
                    ),
                );
            }
            DimensionKind::Size {
                feature,
                kind,
                path,
                angle: sel,
            }
        } else {
            let name = self
                .get_str(id, "shape_aspect_relationship", "name")
                .unwrap_or_default();
            let a = self.get_ref(id, "shape_aspect_relationship", "relating_shape_aspect")?;
            let b = self.get_ref(id, "shape_aspect_relationship", "related_shape_aspect")?;
            let (Some(from), Some(to)) = (self.feature(st, a), self.feature(st, b)) else {
                self.find(
                    FindingKind::Unresolved,
                    part,
                    id,
                    &[a, b],
                    "a dimensioned feature does not resolve; dimension not read",
                );
                return None;
            };
            let angular = self.is(id, "angular_location");
            let sel = if angular {
                let s = enum_value(self.get(id, "angular_location", "angle_selection"));
                angle(s)
            } else {
                None
            };
            if angular && sel.is_none() {
                self.find(
                    FindingKind::Nonconformance,
                    part,
                    id,
                    &[],
                    "angular location without a valid angle_selection; not read",
                );
                return None;
            }
            let path = self.dimension_path(st, id, "dimensional_location_with_path")?;
            let kind = LocationKind::from_name(&name);
            if !angular && kind == LocationKind::LinearDistance {
                self.check_derived_plane_distance(st, id, from, to, &[a, b]);
            }
            if !angular && matches!(kind, LocationKind::Other(_)) {
                self.find(
                    FindingKind::Nonconformance,
                    part,
                    id,
                    &[],
                    format!("dimensional_location name {name:?} is not in Tables 1-2 (§5.1.1); kept as stated"),
                );
            }
            DimensionKind::Location {
                from,
                to,
                kind,
                path,
                directed: self.is(id, "directed_dimensional_location"),
                angle: sel,
            }
        };
        let angular = matches!(
            kind,
            DimensionKind::Size { angle: Some(_), .. }
                | DimensionKind::Location { angle: Some(_), .. }
        );
        let want = if angular { Want::Angle } else { Want::Length };

        // The value representation (§5.2).
        let dcrs = self.referrers_by(
            id,
            "dimensional_characteristic_representation",
            "dimensional_characteristic_representation",
            "dimension",
        );
        if dcrs.len() > 1 {
            self.find(
                FindingKind::Conflict,
                part,
                id,
                &dcrs,
                format!(
                    "{} dimensional_characteristic_representations for one dimension; not read",
                    dcrs.len()
                ),
            );
            return None;
        }
        let sdr = match dcrs.first() {
            Some(&dcr) => {
                prov.push(dcr);
                let sdr = self.get_ref(
                    dcr,
                    "dimensional_characteristic_representation",
                    "representation",
                )?;
                prov.push(sdr);
                Some(sdr)
            }
            None => None,
        };
        let principle = match sdr
            .and_then(|sdr| self.get_str(sdr, "representation", "name"))
            .as_deref()
        {
            None | Some("") => None,
            Some("independency") => Some(Principle::Independency),
            Some("envelope requirement") => Some(Principle::EnvelopeRequirement),
            Some(other) => {
                self.find(
                    FindingKind::Nonconformance,
                    part,
                    sdr.unwrap_or(id),
                    &[id],
                    format!("shape_dimension_representation name {other:?} is not in Table 5; no principle read"),
                );
                None
            }
        };
        let mut nominal = None;
        let mut upper = None;
        let mut lower = None;
        let mut limit_items = Vec::new();
        let mut reversed = Vec::new();
        let mut qualifier = None;
        let mut basic = false;
        let mut modifiers = Vec::new();
        let items = sdr
            .map(|sdr| self.get_refs(sdr, "representation", "items"))
            .unwrap_or_default();
        for item in items {
            let name = self.name_of(item);
            if self.is(item, "descriptive_representation_item")
                && name.is_empty()
                && self.text_of(item).as_deref() == Some("")
                && (principle.is_some() || self.is(item, "qualified_representation_item"))
            {
                // A value-less representation can hold a principle/qualifier using an
                // empty descriptive carrier. It states no nominal value.
                prov.push(item);
                for q in self.get_refs(item, "qualified_representation_item", "qualifiers") {
                    if self.is(q, "type_qualifier") {
                        qualifier = Some(Qualifier::from_name(
                            &self
                                .get_str(q, "type_qualifier", "name")
                                .unwrap_or_default(),
                        ));
                        prov.push(q);
                    }
                }
            } else if self.is(item, "measure_representation_item") {
                let mut c = Vec::new();
                let v = self.value(st, item, want, &mut c)?;
                prov.extend(c);
                prov.extend(self.id_attributes(item));
                if matches!(name.as_str(), "upper limit" | "lower limit") {
                    limit_items.push(item);
                }
                let slot = match name.as_str() {
                    "nominal value" => &mut nominal,
                    "upper limit" => &mut upper,
                    "lower limit" => &mut lower,
                    other => {
                        self.find(
                            FindingKind::Nonconformance,
                            part,
                            item,
                            &[id],
                            format!("dimension value item {other:?} is none of 'nominal value', 'upper limit', 'lower limit' (§5.2); dimension not read"),
                        );
                        return None;
                    }
                };
                if slot.replace(v).is_some() {
                    self.find(
                        FindingKind::Conflict,
                        part,
                        item,
                        &[id],
                        format!("two {name:?} items; dimension not read"),
                    );
                    return None;
                }
                if name == "nominal value" {
                    for q in self.get_refs(item, "qualified_representation_item", "qualifiers") {
                        if self.is(q, "type_qualifier") {
                            let n = self
                                .get_str(q, "type_qualifier", "name")
                                .unwrap_or_default();
                            qualifier = Some(Qualifier::from_name(&n));
                            prov.push(q);
                        }
                    }
                }
            } else if self.is_text(item) && name == "dimensional note" {
                let d = self.text_of(item).unwrap_or_default();
                match d.as_str() {
                    "theoretical" => basic = true,
                    "auxiliary" => modifiers.push(DimensionModifier::Reference),
                    other if DimensionModifier::from_description(other).is_some() => {
                        self.find(
                            FindingKind::Nonconformance,
                            part,
                            item,
                            &[id],
                            format!("a Table 8 modifier {other:?} written as a type 1 'dimensional note' (§5.3 puts it in a 'modifiers' compound item); read as that modifier"),
                        );
                        modifiers.extend(DimensionModifier::from_description(other));
                    }
                    other => {
                        self.find(
                            FindingKind::Nonconformance,
                            part,
                            item,
                            &[id],
                            format!(
                                "dimensional note {other:?} is not in Table 7; dimension not read"
                            ),
                        );
                        return None;
                    }
                }
                prov.push(item);
            } else if self.is(item, "compound_representation_item")
                && (name == "modifiers" || self.modifier_items(item))
            {
                if name != "modifiers" {
                    self.find(
                        FindingKind::Nonconformance,
                        part,
                        item,
                        &[id],
                        format!("type 2 dimension modifiers in a compound item named {name:?} (§5.3: 'modifiers'); read as the modifiers"),
                    );
                }
                prov.push(item);
                let mut list = Vec::new();
                for li in self.get_refs(item, "compound_representation_item", "item_element") {
                    prov.push(li);
                    if self.is(li, "list_representation_item")
                        || self.is(li, "set_representation_item")
                        || self.is(li, "compound_representation_item")
                    {
                        let inner =
                            self.get_refs(li, "compound_representation_item", "item_element");
                        list.extend(inner);
                    } else {
                        list.push(li);
                    }
                }
                for m in list {
                    prov.push(m);
                    let d = self.text_of(m).unwrap_or_default();
                    match DimensionModifier::from_description(&d) {
                        Some(x) => modifiers.push(x),
                        None => {
                            self.find(
                                FindingKind::Nonconformance,
                                part,
                                m,
                                &[id],
                                format!(
                                    "dimension modifier {d:?} is not in Table 8; dimension not read"
                                ),
                            );
                            return None;
                        }
                    }
                }
            } else {
                self.find(
                    FindingKind::Unsupported,
                    part,
                    item,
                    &[id],
                    format!(
                        "dimension item {name:?} of a kind §5 does not define; dimension not read"
                    ),
                );
                return None;
            }
        }

        // Plus/minus tolerance (§5.2.3, §5.2.5).
        let pmts = self.referrers_by(
            id,
            "plus_minus_tolerance",
            "plus_minus_tolerance",
            "toleranced_dimension",
        );
        if pmts.len() > 1 {
            self.find(
                FindingKind::Conflict,
                part,
                id,
                &pmts,
                "several plus_minus_tolerances (UR1); dimension not read",
            );
            return None;
        }
        let mut deviations = None;
        let mut class = None;
        if let Some(&pmt) = pmts.first() {
            prov.push(pmt);
            let range = self.get_ref(pmt, "plus_minus_tolerance", "range")?;
            prov.push(range);
            if self.is(range, "tolerance_value") {
                let lo = self.get_ref(range, "tolerance_value", "lower_bound")?;
                let hi = self.get_ref(range, "tolerance_value", "upper_bound")?;
                let mut c = Vec::new();
                let lo_v = self.value(st, lo, want, &mut c)?;
                let hi_v = self.value(st, hi, want, &mut c)?;
                prov.extend(c);
                match dimension_bounds(hi_v, lo_v, None) {
                    Ok((b, swapped)) => {
                        if swapped {
                            reversed.push((range, vec![id, pmt, hi, lo],
                                "plus/minus deviations were stated reversed; upper and lower bounds swapped (tolerance_value WR1)"));
                        }
                        deviations = Some(b);
                    }
                    Err(e) => {
                        self.find(
                            FindingKind::Nonconformance,
                            part,
                            range,
                            &[id],
                            format!("{} (tolerance_value WR1); dimension not read", e.0),
                        );
                        return None;
                    }
                }
            } else if self.is(range, "limits_and_fits") {
                class = Some(self.fit(st, range, id)?);
            } else {
                self.find(
                    FindingKind::Unsupported,
                    part,
                    range,
                    &[id],
                    "plus_minus_tolerance range of an unknown kind; dimension not read",
                );
                return None;
            }
        }
        let limits = match (upper, lower) {
            (Some(u), Some(l)) => {
                // Without a nominal, a reversed absolute interval cannot be checked for a
                // clean reversal. Keep the existing strict refusal in that malformed case.
                let bounds = if let Some(nominal) = nominal.as_ref() {
                    dimension_bounds(u, l, Some(nominal))
                } else {
                    Bounds::new(u, l).map(|bounds| (bounds, false))
                };
                match bounds {
                    Ok((b, swapped)) => {
                        if swapped {
                            let mut evidence: Vec<_> = sdr.into_iter().collect();
                            evidence.extend(limit_items);
                            reversed.push((id, evidence,
                            "dimension limits were stated reversed; upper and lower limits swapped"));
                        }
                        Some(b)
                    }
                    Err(e) => {
                        self.find(
                            FindingKind::Nonconformance,
                            part,
                            id,
                            &sdr.into_iter().collect::<Vec<_>>(),
                            format!("{}; dimension not read", e.0),
                        );
                        return None;
                    }
                }
            }
            (None, None) => None,
            _ => {
                self.find(
                    FindingKind::Nonconformance,
                    part,
                    id,
                    &sdr.into_iter().collect::<Vec<_>>(),
                    "one limit without the other (§5.2.4); dimension not read",
                );
                return None;
            }
        };
        let tolerance = match (basic, deviations, limits, class) {
            (false, None, None, None) => DimTolerance::None,
            (true, None, None, None) => DimTolerance::Basic,
            (false, Some(d), None, None) => DimTolerance::Deviations(d),
            (false, None, Some(l), None) => DimTolerance::Limits(l),
            (false, None, limits, Some(class)) => DimTolerance::Fit { class, limits },
            _ => {
                self.find(
                    FindingKind::Conflict,
                    part,
                    id,
                    &sdr.into_iter().collect::<Vec<_>>(),
                    "the dimension states more than one of theoretical, deviations, limits (other than a fit's stated limits); not read",
                );
                return None;
            }
        };
        if nominal.is_none()
            && matches!(
                tolerance,
                DimTolerance::Deviations(_) | DimTolerance::Fit { .. }
            )
        {
            self.find(
                FindingKind::Nonconformance,
                part,
                id,
                &[],
                "deviations or a tolerance class without a nominal value to apply them to; dimension not read",
            );
            return None;
        }
        for (subject, evidence, detail) in reversed {
            self.find(
                FindingKind::Nonconformance,
                part,
                subject,
                &evidence,
                detail,
            );
        }
        let did = DimensionId(st.pmi.dimensions.len());
        st.pmi.dimensions.push(Dimension {
            kind,
            nominal,
            tolerance,
            qualifier,
            modifiers,
            principle,
        });
        st.prov.dimensions.push(ids(prov));
        st.identifiers.dimensions.push(self.item_identifier(
            st.id,
            "dimensions",
            st.pmi.dimensions.len() - 1,
            id,
        ));
        Some(did)
    }

    /// Whether a compound item holds only descriptive items with Table 8 modifier strings (files
    /// name the compound 'dimensional note' or 'dimension modifiers type 2' for 'modifiers').
    fn modifier_items(&mut self, item: u64) -> bool {
        let mut list = Vec::new();
        for li in self.get_refs(item, "compound_representation_item", "item_element") {
            if self.is(li, "compound_representation_item") {
                list.extend(self.get_refs(li, "compound_representation_item", "item_element"));
            } else {
                list.push(li);
            }
        }
        !list.is_empty()
            && list.iter().all(|&m| {
                self.is_text(m)
                    && self
                        .text_of(m)
                        .is_some_and(|d| DimensionModifier::from_description(&d).is_some())
            })
    }

    /// The path feature of a dimension with path (`Some(None)` when it has none; `None` when
    /// it does not resolve).
    fn dimension_path(
        &mut self,
        st: &mut PartState,
        id: u64,
        entity: &str,
    ) -> Option<Option<FeatureId>> {
        if !self.is(id, entity) {
            return Some(None);
        }
        let p = self.get_ref(id, entity, "path")?;
        match self.feature(st, p) {
            Some(f) => Some(Some(f)),
            None => {
                self.find(
                    FindingKind::Unresolved,
                    Some(st.id),
                    id,
                    &[p],
                    "the dimension's path does not resolve; not read",
                );
                None
            }
        }
    }

    /// An ISO 286 class from `limits_and_fits` (§5.2.5).
    fn fit(&mut self, st: &PartState, lf: u64, dim: u64) -> Option<Iso286Class> {
        let part = Some(st.id);
        let form = self
            .get_str(lf, "limits_and_fits", "form_variance")
            .unwrap_or_default();
        let zone = self
            .get_str(lf, "limits_and_fits", "zone_variance")
            .unwrap_or_default();
        let grade = self
            .get_str(lf, "limits_and_fits", "grade")
            .unwrap_or_default();
        let (letters, grade_text, read_through) = if grade.is_empty() {
            // The grade written into form_variance (NIST FTC-10: 'G6', grade ''): split at the
            // first digit.
            let split = form.find(|c: char| c.is_ascii_digit());
            match split {
                Some(i) if i > 0 => (form[..i].to_string(), form[i..].to_string(), true),
                _ => {
                    self.find(
                        FindingKind::Nonconformance,
                        part,
                        lf,
                        &[dim],
                        format!(
                            "limits_and_fits without a grade (form {form:?}); dimension not read"
                        ),
                    );
                    return None;
                }
            }
        } else {
            (form.clone(), grade.clone(), false)
        };
        let deviation = FundamentalDeviation::parse(&letters);
        let g = ToleranceGrade::parse(&grade_text);
        let (Ok(deviation), Ok(g)) = (deviation, g) else {
            self.find(
                FindingKind::Nonconformance,
                part,
                lf,
                &[dim],
                format!("limits_and_fits ({form:?}, {zone:?}, {grade:?}) is not an ISO 286 class; dimension not read"),
            );
            return None;
        };
        if read_through {
            self.find(
                FindingKind::Nonconformance,
                part,
                lf,
                &[dim],
                format!(
                    "limits_and_fits form_variance {form:?} holds the grade and grade is '' (§5.2.5: form_variance is the deviation, grade the grade); read as {deviation} and grade {grade_text}"
                ),
            );
        }
        let zone_of = match zone.as_str() {
            "hole" => Some(FitFeature::Hole),
            "shaft" => Some(FitFeature::Shaft),
            "" => None,
            other => {
                self.find(
                    FindingKind::Nonconformance,
                    part,
                    lf,
                    &[dim],
                    format!("zone_variance {other:?} is neither 'hole' nor 'shaft' (§5.2.5); ignored, the letter's case decides"),
                );
                None
            }
        };
        // ISO 286-1 defines the class by its letters: capitals are holes, lower case shafts, and
        // the fundamental deviation's value depends on that case. zone_variance only restates
        // it, so a zone_variance that disagrees is a nonconformance, not a conflict between two
        // authorities: the letters are read as stated.
        if let Some(z) = zone_of
            && z != deviation.of
        {
            self.find(
                FindingKind::Nonconformance,
                part,
                lf,
                &[dim],
                format!(
                    "zone_variance {zone:?} contradicts deviation {deviation}; the letter's case (ISO 286) decides"
                ),
            );
        }
        Some(Iso286Class {
            deviation,
            grade: g,
        })
    }

    // --- tolerances -----------------------------------------------------------------------

    fn tolerance(&mut self, st: &mut PartState, id: u64) -> Option<ToleranceId> {
        if let Some(t) = st.tolerances.get(&id) {
            return *t;
        }
        st.tolerances.insert(id, None);
        let r = self.build_tolerance(st, id);
        st.tolerances.insert(id, r);
        if r.is_none() {
            self.not_read(st, id);
        }
        r
    }

    fn build_tolerance(&mut self, st: &mut PartState, id: u64) -> Option<ToleranceId> {
        let part = Some(st.id);
        let names = self.names(id);
        let mut prov = vec![id];
        prov.extend(self.id_attributes(id));
        let kinds: Vec<ToleranceKind> = names
            .iter()
            .filter_map(|n| {
                ToleranceKind::ALL
                    .iter()
                    .find(|(e, _)| self.is_a_name(n, e))
                    .map(|&(_, k)| k)
            })
            .collect();
        let kind = match kinds.as_slice() {
            [k] => *k,
            _ => {
                self.find(
                    FindingKind::Unsupported,
                    part,
                    id,
                    &[],
                    "a geometric tolerance that is not exactly one of the Table 12 types; not read",
                );
                return None;
            }
        };
        if names.iter().any(|n| n == "modified_geometric_tolerance") {
            // Edition 1's form: read below with the modifiers.
        }

        // Target (§6.1–§6.3).
        let t = self.get_ref(id, "geometric_tolerance", "toleranced_shape_aspect")?;
        let target = if t == st.part.shape {
            ToleranceTarget::WholePart
        } else if self.is(t, "dimensional_size") || self.is(t, "dimensional_location") {
            match self.dimension(st, t) {
                Some(d) => ToleranceTarget::Dimension(d),
                None => {
                    self.find(
                        FindingKind::Unresolved,
                        part,
                        id,
                        &[t],
                        "the toleranced dimension was not read; tolerance not read",
                    );
                    return None;
                }
            }
        } else if self.is(t, "shape_aspect") {
            match self.feature(st, t) {
                Some(f) => ToleranceTarget::Feature(f),
                None => {
                    self.find(
                        FindingKind::Unresolved,
                        part,
                        id,
                        &[t],
                        "the toleranced feature was not read; tolerance not read",
                    );
                    return None;
                }
            }
        } else if self.is(t, "shape_aspect_relationship") {
            let a = self.get_ref(t, "shape_aspect_relationship", "relating_shape_aspect")?;
            let b = self.get_ref(t, "shape_aspect_relationship", "related_shape_aspect")?;
            let name = self
                .get_str(t, "shape_aspect_relationship", "name")
                .unwrap_or_default();
            match (self.feature(st, a), self.feature(st, b)) {
                (Some(relating), Some(related)) => {
                    prov.push(t);
                    ToleranceTarget::Relation {
                        relating,
                        related,
                        name,
                    }
                }
                _ => {
                    self.find(
                        FindingKind::Unresolved,
                        part,
                        id,
                        &[t, a, b],
                        "a feature of the toleranced relationship was not read; tolerance not read",
                    );
                    return None;
                }
            }
        } else {
            self.find(
                FindingKind::Unresolved,
                part,
                id,
                &[t],
                "the tolerance's target is not a geometric_tolerance_target; not read",
            );
            return None;
        };

        let mut c = Vec::new();
        let magnitude = match self.get_ref(id, "geometric_tolerance", "magnitude") {
            Some(m) => Some(self.value(st, m, Want::Length, &mut c)?),
            None => None,
        };

        // Modifiers (§6.9.3), edition 1's modified_geometric_tolerance too.
        let mut modifiers = Vec::new();
        if let Some(Attribute::List(l)) =
            self.get(id, "geometric_tolerance_with_modifiers", "modifiers")
        {
            for m in l {
                match m {
                    Attribute::Enum(v) => {
                        match ToleranceModifier::from_enum(v) {
                            Some(x) => modifiers.push(x),
                            None => {
                                self.find(
                                FindingKind::Nonconformance,
                                part,
                                id,
                                &[],
                                format!("unknown geometric_tolerance_modifier .{v}.; tolerance not read"),
                            );
                                return None;
                            }
                        }
                    }
                    other => {
                        self.find(
                            FindingKind::Nonconformance,
                            part,
                            id,
                            &[],
                            format!(
                                "modifier {other:?} is not an enumeration value; tolerance not read"
                            ),
                        );
                        return None;
                    }
                }
            }
        }
        if let Some(v) = enum_value(self.get(id, "modified_geometric_tolerance", "modifier")) {
            let m = match v.to_ascii_uppercase().as_str() {
                "MAXIMUM_MATERIAL_CONDITION" => Some(ToleranceModifier::MaximumMaterialRequirement),
                "LEAST_MATERIAL_CONDITION" => Some(ToleranceModifier::LeastMaterialRequirement),
                "REGARDLESS_OF_FEATURE_SIZE" => {
                    // RFS is what a tolerance without a material modifier means (ISO 8015
                    // independency; ASME Y14.5-2009 Rule #2), so the model has no modifier for
                    // it; the file stated it, so it is reported, not dropped silently.
                    self.find(
                        FindingKind::NotModelled,
                        part,
                        id,
                        &[],
                        "limit_condition .REGARDLESS_OF_FEATURE_SIZE. is not held (it is the meaning of no material modifier); read without one",
                    );
                    None
                }
                _ => {
                    self.find(
                        FindingKind::Nonconformance,
                        part,
                        id,
                        &[],
                        format!("unknown limit_condition .{v}.; tolerance not read"),
                    );
                    return None;
                }
            };
            modifiers.extend(m);
        }

        // Unit basis (§6.9.6), maximum (§6.9.5), unequal disposition (§6.9.4).
        let unit_basis = if self.is(id, "geometric_tolerance_with_defined_unit") {
            let u = self.get_ref(id, "geometric_tolerance_with_defined_unit", "unit_size")?;
            let size = self.value(st, u, Want::Any, &mut c)?;
            let area = if self.is(id, "geometric_tolerance_with_defined_area_unit") {
                let a = enum_value(self.get(
                    id,
                    "geometric_tolerance_with_defined_area_unit",
                    "area_type",
                ))
                .map(str::to_ascii_uppercase);
                let shape = match a.as_deref() {
                    Some("CIRCULAR") => AreaShape::Circular,
                    Some("SQUARE") => AreaShape::Square,
                    Some("RECTANGULAR") => AreaShape::Rectangular,
                    Some("CYLINDRICAL") => AreaShape::Cylindrical,
                    Some("SPHERICAL") => AreaShape::Spherical,
                    _ => {
                        self.find(
                            FindingKind::Nonconformance,
                            part,
                            id,
                            &[],
                            "unknown area_type; tolerance not read",
                        );
                        return None;
                    }
                };
                let second = match self.get_ref(
                    id,
                    "geometric_tolerance_with_defined_area_unit",
                    "second_unit_size",
                ) {
                    Some(s) => Some(self.value(st, s, Want::Any, &mut c)?),
                    None => None,
                };
                Some(UnitArea { shape, second })
            } else {
                None
            };
            Some(UnitBasis { size, area })
        } else {
            None
        };
        let maximum = match self.get_ref(
            id,
            "geometric_tolerance_with_maximum_tolerance",
            "maximum_upper_tolerance",
        ) {
            Some(m) => Some(self.value(st, m, Want::Length, &mut c)?),
            None => None,
        };
        let unequal =
            match self.get_ref(id, "unequally_disposed_geometric_tolerance", "displacement") {
                Some(m) => Some(self.value(st, m, Want::Length, &mut c)?),
                None => None,
            };
        prov.extend(c);

        // Datum reference (§6.9.7–§6.9.8).
        let datums = if self.is(id, "geometric_tolerance_with_datum_reference") {
            let systems = self.get_refs(
                id,
                "geometric_tolerance_with_datum_reference",
                "datum_system",
            );
            Some(self.datum_system(st, id, &systems, &mut prov)?)
        } else {
            None
        };
        match (kind.datums(), &datums) {
            (DatumRequirement::Required, None) => {
                self.find(
                    FindingKind::Nonconformance,
                    part,
                    id,
                    &[],
                    format!("{kind:?} without datums (it is a geometric_tolerance_with_datum_reference subtype); not read"),
                );
                return None;
            }
            (DatumRequirement::Forbidden, Some(_)) => {
                self.find(
                    FindingKind::Nonconformance,
                    part,
                    id,
                    &[],
                    format!("{kind:?} with datums (its WR1 forbids them); not read"),
                );
                return None;
            }
            _ => {}
        }

        // Zone (§6.9.2).
        let zone = self.zone(st, id, kind, &mut prov)?;

        let mut auxiliary = Vec::new();
        for a in self.referrers_by(
            id,
            "geometric_tolerance_auxiliary_classification",
            "geometric_tolerance_auxiliary_classification",
            "described_item",
        ) {
            let v = enum_value(self.get(
                a,
                "geometric_tolerance_auxiliary_classification",
                "attribute_value",
            ))
            .map(str::to_ascii_uppercase);
            match v.as_deref() {
                Some("ALL_OVER") => auxiliary.push(AuxiliaryClassification::AllOver),
                Some("UNLESS_OTHERWISE_SPECIFIED") => {
                    auxiliary.push(AuxiliaryClassification::UnlessOtherwiseSpecified);
                }
                _ => {
                    self.find(
                        FindingKind::Nonconformance,
                        part,
                        a,
                        &[id],
                        "unknown auxiliary classification; ignored",
                    );
                    continue;
                }
            }
            prov.push(a);
        }
        let description = self
            .get_str(id, "geometric_tolerance", "description")
            .filter(|d| !d.is_empty());

        let tid = ToleranceId(st.pmi.tolerances.len());
        st.pmi.tolerances.push(GeometricTolerance {
            kind,
            target,
            magnitude,
            zone,
            modifiers,
            unit_basis,
            maximum,
            unequal,
            datums,
            auxiliary,
            description,
        });
        st.prov.tolerances.push(ids(prov));
        st.identifiers.tolerances.push(self.item_identifier(
            st.id,
            "tolerances",
            st.pmi.tolerances.len() - 1,
            id,
        ));
        Some(tid)
    }

    /// The datum system of a tolerance: one `datum_system`, or edition 1's `datum_reference`s
    /// by precedence.
    fn datum_system(
        &mut self,
        st: &mut PartState,
        tol: u64,
        systems: &[u64],
        prov: &mut Vec<u64>,
    ) -> Option<DatumSystem> {
        let part = Some(st.id);
        if let [ds] = systems
            && self.is(*ds, "datum_system")
        {
            prov.push(*ds);
            prov.extend(self.id_attributes(*ds));
            // The datum reference frame's axis system (§6.9.7 Figure 64), given by a usage of
            // the datum system: not held.
            for (u, _, items) in self.usages(*ds) {
                prov.push(u);
                let mut named = vec![u];
                named.extend(items);
                self.find(
                    FindingKind::NotModelled,
                    part,
                    *ds,
                    &named,
                    "the datum reference frame's axis system (§6.9.7 Figure 64) is not held",
                );
            }
            let mut compartments = Vec::new();
            for c in self.get_refs(*ds, "datum_system", "constituents") {
                prov.push(c);
                let base = self.get(c, "general_datum_reference", "base");
                let references = self.datum_base(st, tol, c, base, prov)?;
                let modifiers = self.datum_modifiers(st, tol, c, prov)?;
                compartments.push(Compartment {
                    references,
                    modifiers,
                });
            }
            return match DatumSystem::new(compartments) {
                Ok(s) => Some(s),
                Err(e) => {
                    self.find(FindingKind::Nonconformance, part, *ds, &[tol], e.0);
                    None
                }
            };
        }
        if !systems.is_empty() && systems.iter().all(|&s| self.is(s, "datum_reference")) {
            let mut by_precedence: Vec<(i64, u64)> = Vec::new();
            for &s in systems {
                let p = match self.get(s, "datum_reference", "precedence") {
                    Some(Attribute::Integer(p)) => *p,
                    _ => {
                        self.find(
                            FindingKind::Nonconformance,
                            part,
                            s,
                            &[tol],
                            "datum_reference without an integer precedence; tolerance not read",
                        );
                        return None;
                    }
                };
                by_precedence.push((p, s));
            }
            by_precedence.sort();
            let mut compartments = Vec::new();
            for (_, s) in by_precedence {
                prov.push(s);
                let d = self.get_ref(s, "datum_reference", "referenced_datum")?;
                let Some(&datum) = st.datums.get(&d) else {
                    self.find(
                        FindingKind::Unresolved,
                        part,
                        tol,
                        &[s, d],
                        "referenced datum is not a datum of the part; tolerance not read",
                    );
                    return None;
                };
                compartments.push(Compartment {
                    references: vec![DatumReference {
                        datum,
                        modifiers: Vec::new(),
                    }],
                    modifiers: Vec::new(),
                });
            }
            return match DatumSystem::new(compartments) {
                Ok(s) => Some(s),
                Err(e) => {
                    self.find(FindingKind::Nonconformance, part, tol, systems, e.0);
                    None
                }
            };
        }
        self.find(
            FindingKind::Nonconformance,
            part,
            tol,
            systems,
            "a datum reference that is neither one datum_system (geometric_tolerance_with_datum_reference WR1) nor datum_references; tolerance not read",
        );
        None
    }

    /// The datums of a compartment or element's `base`: a datum, or a common datum list of
    /// elements (§6.9.8).
    fn datum_base(
        &mut self,
        st: &mut PartState,
        tol: u64,
        owner: u64,
        base: Option<&'a Attribute>,
        prov: &mut Vec<u64>,
    ) -> Option<Vec<DatumReference>> {
        let part = Some(st.id);
        match base {
            Some(Attribute::EntityRef(d)) => {
                let Some(&datum) = st.datums.get(d) else {
                    self.find(
                        FindingKind::Unresolved,
                        part,
                        tol,
                        &[owner, *d],
                        "the referenced datum is not a datum read for the part; tolerance not read",
                    );
                    return None;
                };
                Some(vec![DatumReference {
                    datum,
                    modifiers: Vec::new(),
                }])
            }
            Some(Attribute::Typed { value, .. })
                if matches!(value.as_ref(), Attribute::List(_)) =>
            {
                let Attribute::List(elements) = value.as_ref() else {
                    return None;
                };
                self.common_datum(st, tol, elements, prov)
            }
            Some(Attribute::List(elements)) => self.common_datum(st, tol, elements, prov),
            other => {
                self.find(
                    FindingKind::Nonconformance,
                    part,
                    owner,
                    &[tol],
                    format!("datum reference base {other:?} is neither a datum nor a common datum list; tolerance not read"),
                );
                None
            }
        }
    }

    /// The elements of a common datum list (§6.9.8), each with its own modifiers.
    fn common_datum(
        &mut self,
        st: &mut PartState,
        tol: u64,
        elements: &'a [Attribute],
        prov: &mut Vec<u64>,
    ) -> Option<Vec<DatumReference>> {
        let mut out = Vec::new();
        for e in elements {
            let Attribute::EntityRef(e) = e else {
                return None;
            };
            prov.push(*e);
            let inner = self.get(*e, "general_datum_reference", "base");
            if !matches!(inner, Some(Attribute::EntityRef(_))) {
                self.find(
                    FindingKind::Unsupported,
                    Some(st.id),
                    *e,
                    &[tol],
                    "a nested common datum list is not read",
                );
                return None;
            }
            let mut r = self.datum_base(st, tol, *e, inner, prov)?;
            let m = self.datum_modifiers(st, tol, *e, prov)?;
            r[0].modifiers = m;
            out.extend(r);
        }
        Some(out)
    }

    fn datum_modifiers(
        &mut self,
        st: &mut PartState,
        tol: u64,
        owner: u64,
        prov: &mut Vec<u64>,
    ) -> Option<Vec<DatumModifier>> {
        let part = Some(st.id);
        let mut out = Vec::new();
        let list = match self.get(owner, "general_datum_reference", "modifiers") {
            None | Some(Attribute::Unset) => return Some(out),
            Some(Attribute::List(l)) => l,
            Some(other) => {
                self.find(
                    FindingKind::Nonconformance,
                    part,
                    owner,
                    &[tol],
                    format!(
                        "datum reference modifiers {other:?} are not a set; tolerance not read"
                    ),
                );
                return None;
            }
        };
        for m in list {
            match m {
                Attribute::Typed { type_name, value }
                    if type_name.eq_ignore_ascii_case("simple_datum_reference_modifier") =>
                {
                    let Attribute::Enum(v) = value.as_ref() else {
                        return None;
                    };
                    match SimpleDatumModifier::from_enum(v) {
                        Some(x) => out.push(DatumModifier::Simple(x)),
                        None => {
                            self.find(
                                FindingKind::Nonconformance,
                                part,
                                owner,
                                &[tol],
                                format!("unknown simple datum reference modifier .{v}.; tolerance not read"),
                            );
                            return None;
                        }
                    }
                }
                Attribute::EntityRef(w) if self.is(*w, "datum_reference_modifier_with_value") => {
                    prov.push(*w);
                    let t = enum_value(self.get(
                        *w,
                        "datum_reference_modifier_with_value",
                        "modifier_type",
                    ))
                    .and_then(DatumModifierType::from_enum);
                    let v =
                        self.get_ref(*w, "datum_reference_modifier_with_value", "modifier_value");
                    let (Some(kind), Some(v)) = (t, v) else {
                        self.find(
                            FindingKind::Nonconformance,
                            part,
                            *w,
                            &[tol],
                            "malformed datum_reference_modifier_with_value; tolerance not read",
                        );
                        return None;
                    };
                    let mut c = Vec::new();
                    let value = self.value(st, v, Want::Length, &mut c)?;
                    prov.extend(c);
                    out.push(DatumModifier::WithValue { kind, value });
                }
                other => {
                    self.find(
                        FindingKind::Nonconformance,
                        part,
                        owner,
                        &[tol],
                        format!("datum reference modifier {other:?} is not a datum_reference_modifier; tolerance not read"),
                    );
                    return None;
                }
            }
        }
        Some(out)
    }

    fn zone(
        &mut self,
        st: &mut PartState,
        tol: u64,
        kind: ToleranceKind,
        prov: &mut Vec<u64>,
    ) -> Option<Option<Zone>> {
        let part = Some(st.id);
        let zones = self.referrers_by(
            tol,
            "tolerance_zone",
            "tolerance_zone",
            "defining_tolerance",
        );
        let zone = match zones.as_slice() {
            [] => return Some(None),
            [z] => *z,
            _ => {
                self.find(
                    FindingKind::Conflict,
                    part,
                    tol,
                    &zones,
                    "several tolerance zones for one tolerance; tolerance not read",
                );
                return None;
            }
        };
        prov.push(zone);
        prov.extend(self.id_attributes(zone));
        let form_id = self.get_ref(zone, "tolerance_zone", "form")?;
        prov.push(form_id);
        let form_name = self
            .get_str(form_id, "tolerance_zone_form", "name")
            .unwrap_or_default();
        let form = ZoneForm::from_name(&form_name);
        let mut out = Zone {
            form,
            projected: None,
            non_uniform: None,
            runout_angle: None,
            affected_plane: None,
            affected_plane_kind: None,
        };
        for def in self.referrers_by(
            zone,
            "tolerance_zone_definition",
            "tolerance_zone_definition",
            "zone",
        ) {
            prov.push(def);
            let boundaries = self.get_refs(def, "tolerance_zone_definition", "boundaries");
            if self.is(def, "projected_zone_definition") {
                let end = self.get_ref(def, "projected_zone_definition", "projection_end")?;
                let len = self.get_ref(def, "projected_zone_definition", "projected_length")?;
                let end_f = if self.usages(end).is_empty() && self.members(end).is_empty() {
                    // A mandatory shape-aspect carrier can state no origin geometry.
                    // Its absence is held directly by ProjectedZone::end.
                    prov.push(end);
                    prov.extend(self.id_attributes(end));
                    st.absorbed.insert(end);
                    self.find(
                        FindingKind::Fact,
                        part,
                        def,
                        &[end],
                        "projected zone states a projection-end shape aspect with no geometry",
                    );
                    None
                } else {
                    match self.feature(st, end) {
                        Some(f) => Some(f),
                        None => {
                            self.find(
                                FindingKind::Unresolved,
                                part,
                                tol,
                                &[def, end],
                                "the projection end does not resolve; tolerance not read",
                            );
                            return None;
                        }
                    }
                };
                let mut c = Vec::new();
                let length = self.value(st, len, Want::Length, &mut c)?;
                prov.extend(c);
                out.projected = Some(ProjectedZone { end: end_f, length });
                if !boundaries.is_empty() {
                    self.find(
                        FindingKind::NotModelled,
                        part,
                        def,
                        &boundaries,
                        "projected zone boundaries (not used, §6.9.2.2) are not held",
                    );
                }
            } else if self.is(def, "non_uniform_zone_definition") {
                let mut fs = Vec::new();
                for b in boundaries {
                    match self.feature(st, b) {
                        Some(f) => fs.push(f),
                        None => {
                            self.find(
                                FindingKind::Unresolved,
                                part,
                                tol,
                                &[def, b],
                                "a non-uniform zone boundary does not resolve; tolerance not read",
                            );
                            return None;
                        }
                    }
                }
                out.non_uniform = Some(fs);
            } else if self.is(def, "runout_zone_definition") {
                let o = self.get_ref(def, "runout_zone_definition", "orientation");
                let angle = o.and_then(|o| {
                    prov.push(o);
                    self.get_ref(o, "runout_zone_orientation", "angle")
                });
                let Some(a) = angle else {
                    // OCCT sometimes emits a runout definition for a position zone, with
                    // its orientation missing. The explicit zone form remains authoritative;
                    // never invent an angle or attach an orphan orientation from elsewhere.
                    let reason = if matches!(
                        kind,
                        ToleranceKind::CircularRunout | ToleranceKind::TotalRunout
                    ) {
                        Some("the runout tolerance's missing orientation cannot be recovered")
                    } else if !boundaries.is_empty() {
                        Some("the stated runout boundaries need the missing orientation")
                    } else if !(out.form.is_diametral()
                        || out.form.is_spherical()
                        || matches!(out.form,
                            ZoneForm::BetweenTwoConcentricCircles
                            | ZoneForm::BetweenTwoEquidistantCurves
                            | ZoneForm::BetweenTwoCoaxialCylinders
                            | ZoneForm::BetweenTwoEquidistantSurfaces
                            | ZoneForm::BetweenTwoEquidistantComplexLinesOrTwoParallelStraightLines
                            | ZoneForm::BetweenTwoEquidistantComplexSurfacesOrTwoParallelPlanes))
                    {
                        Some(
                            "the stated zone form does not establish an orientation-independent zone",
                        )
                    } else {
                        None
                    };
                    let recovered = reason.is_none();
                    self.find(
                        FindingKind::Nonconformance,
                        part,
                        def,
                        &[tol],
                        reason.map_or_else(
                            || "runout_zone_definition without an orientation angle on a non-runout tolerance; read the explicitly stated zone form without a runout angle".to_string(),
                            |reason| format!("runout_zone_definition without an orientation angle (Figure 51); {reason}; tolerance not read")),
                    );
                    if recovered {
                        continue;
                    }
                    return None;
                };
                let mut c = Vec::new();
                let v = self.value(st, a, Want::Angle, &mut c)?;
                prov.extend(c);
                out.runout_angle = Some(v);
            } else {
                self.find(
                    FindingKind::Unsupported,
                    part,
                    def,
                    &[tol],
                    "a tolerance zone definition of an unknown kind; tolerance not read",
                );
                return None;
            }
        }
        for r in self.referrers_by(
            zone,
            "shape_aspect_relationship",
            "shape_aspect_relationship",
            "relating_shape_aspect",
        ) {
            if self
                .get_str(r, "shape_aspect_relationship", "name")
                .as_deref()
                == Some("affected plane association")
                || self
                    .get_str(r, "shape_aspect_relationship", "description")
                    .as_deref()
                    == Some("affected plane association")
            {
                let f = self.get_ref(r, "shape_aspect_relationship", "related_shape_aspect")?;
                out.affected_plane = Some(self.feature(st, f)?);
                out.affected_plane_kind = match self
                    .get_str(f, "shape_aspect", "description")
                    .as_deref()
                {
                    Some("representative line element") => {
                        Some(AffectedPlaneKind::IntersectionLine)
                    }
                    Some("representative plane element") => {
                        Some(AffectedPlaneKind::IntersectionPlane)
                    }
                    // Figure 54 has an undescribed aspect and an axis2 placement.
                    Some("") | None if matches!(&st.pmi.features[out.affected_plane?.0], Feature::Items(items) if matches!(items.as_slice(), [Anchor::Geometry(g)] if st.pmi.geometry[g.0].item.entity() == "axis2_placement_3d")) => {
                        Some(AffectedPlaneKind::OrientationPlane)
                    }
                    _ => None,
                };
                prov.push(r);
            }
        }
        Some(Some(out))
    }

    fn relations(&mut self, st: &mut PartState, tol: u64) {
        let part = Some(st.id);
        let Some(Some(relating)) = st.tolerances.get(&tol).copied() else {
            return;
        };
        for r in self.referrers_by(
            tol,
            "geometric_tolerance_relationship",
            "geometric_tolerance_relationship",
            "relating_geometric_tolerance",
        ) {
            let name = self
                .get_str(r, "geometric_tolerance_relationship", "name")
                .unwrap_or_default();
            let kind = match name.as_str() {
                "composite" | "composite tolerance" => RelationKind::Composite,
                "precedence" => RelationKind::Precedence,
                "simultaneity" => RelationKind::Simultaneity,
                other => {
                    self.find(
                        FindingKind::Nonconformance,
                        part,
                        r,
                        &[tol],
                        format!("tolerance relationship name {other:?} is none of composite, precedence, simultaneity (§6.9.9); not read"),
                    );
                    continue;
                }
            };
            let Some(other) = self.get_ref(
                r,
                "geometric_tolerance_relationship",
                "related_geometric_tolerance",
            ) else {
                continue;
            };
            let Some(Some(related)) = st.tolerances.get(&other).copied() else {
                self.find(
                    FindingKind::Unresolved,
                    part,
                    r,
                    &[tol, other],
                    "the related tolerance was not read for this part; relationship not read",
                );
                continue;
            };
            if kind == RelationKind::Composite {
                let (a, b) = (
                    &st.pmi.tolerances[relating.0],
                    &st.pmi.tolerances[related.0],
                );
                let kinds = [
                    ToleranceKind::Position,
                    ToleranceKind::LineProfile,
                    ToleranceKind::SurfaceProfile,
                ];
                let why = if a.kind != b.kind || !kinds.contains(&a.kind) {
                    Some(
                        "relates tolerances that are not both position, line or surface profile (geometric_tolerance_relationship WR1)",
                    )
                } else if !st.pmi.same_target(&a.target, &b.target) {
                    Some(
                        "relates tolerances on different targets (geometric_tolerance_relationship WR2)",
                    )
                } else if st
                    .pmi
                    .tolerance_relations
                    .iter()
                    .any(|x| x.kind == RelationKind::Composite && x.relating == relating)
                {
                    Some(
                        "is a second composite relationship of one tolerance (geometric_tolerance WR5)",
                    )
                } else {
                    None
                };
                if let Some(why) = why {
                    self.find(
                        FindingKind::Nonconformance,
                        part,
                        r,
                        &[tol, other],
                        format!("the composite relationship {why}; not read"),
                    );
                    continue;
                }
            }
            let mut prov = vec![r];
            prov.extend(self.id_attributes(r));
            st.pmi.tolerance_relations.push(ToleranceRelation {
                kind,
                relating,
                related,
            });
            st.prov.tolerance_relations.push(ids(prov));
            st.identifiers
                .tolerance_relations
                .push(self.item_identifier(
                    st.id,
                    "tolerance_relations",
                    st.pmi.tolerance_relations.len() - 1,
                    r,
                ));
        }
    }

    // --- threads and knurls ---------------------------------------------------------------

    /// `thread` and `turned_knurl` definitions applied to the part's shape aspects ('applied
    /// shape', thread WR13, knurl likewise).
    fn features_definitions(&mut self, st: &mut PartState, aspects: &[u64]) {
        let part = Some(st.id);
        for &a in aspects {
            for sdr in self.referrers_by(
                a,
                "shape_defining_relationship",
                "shape_aspect_relationship",
                "relating_shape_aspect",
            ) {
                let desc = self
                    .get_str(sdr, "shape_aspect_relationship", "description")
                    .unwrap_or_default();
                if desc != "applied shape" {
                    continue;
                }
                let Some(occ) =
                    self.get_ref(sdr, "shape_aspect_relationship", "related_shape_aspect")
                else {
                    continue;
                };
                let Some(def_shape) = self.get_ref(occ, "shape_aspect", "of_shape") else {
                    continue;
                };
                let Some(def) = self.get_ref(def_shape, "property_definition", "definition") else {
                    continue;
                };
                let is_thread = self.is(def, "thread");
                let is_knurl = self.is(def, "turned_knurl");
                if !is_thread && !is_knurl {
                    continue;
                }
                let Some(feature) = self.feature(st, a) else {
                    self.find(
                        FindingKind::Unresolved,
                        part,
                        def,
                        &[a],
                        "the applied shape does not resolve; not read",
                    );
                    continue;
                };
                let mut prov = vec![def, def_shape, occ, sdr];
                prov.extend(self.id_attributes(def_shape));
                // Parameters.
                let mut params: BTreeMap<String, (u64, ParamValue)> = BTreeMap::new();
                let mut bad = false;
                for pdef in self.referrers_by(
                    def,
                    "property_definition",
                    "property_definition",
                    "definition",
                ) {
                    if pdef == def_shape {
                        continue;
                    }
                    for (pdr, rep) in self.property_representations(pdef) {
                        if !self.is(rep, "shape_representation_with_parameters") {
                            continue;
                        }
                        prov.extend([pdef, pdr, rep]);
                        for item in self.get_refs(rep, "representation", "items") {
                            let name = self.name_of(item);
                            let v = if self.is_text(item) {
                                ParamValue::Text(self.text_of(item).unwrap_or_default())
                            } else if !self.is(item, "measure_with_unit") {
                                self.find(
                                    FindingKind::Nonconformance,
                                    part,
                                    def,
                                    &[item],
                                    format!(
                                        "{} in the parameters is neither a descriptive nor a measure item; ignored",
                                        self.entity_label(item)
                                    ),
                                );
                                continue;
                            } else {
                                match self.measure(item) {
                                    Ok(m) => ParamValue::Measure(m),
                                    Err(e) => {
                                        self.find(e.kind, part, item, &[def], e.why);
                                        bad = true;
                                        continue;
                                    }
                                }
                            };
                            prov.push(item);
                            if params.insert(name.clone(), (item, v)).is_some() {
                                self.find(
                                    FindingKind::Conflict,
                                    part,
                                    item,
                                    &[def],
                                    format!("parameter {name:?} stated twice; not read"),
                                );
                                bad = true;
                            }
                        }
                    }
                }
                // Other shape aspects of the definition: partial area, runout.
                let mut partial_area = None;
                let mut runout = None;
                let occs: Vec<u64> = self
                    .doc
                    .referrers(def_shape)
                    .iter()
                    .copied()
                    .filter(|&o| o != occ && self.is(o, "shape_aspect"))
                    .collect();
                for o in occs {
                    let d = self
                        .get_str(o, "shape_aspect", "description")
                        .unwrap_or_default();
                    let usage = match d.as_str() {
                        "partial area occurrence" => "applied area usage",
                        "thread runout" => "thread runout usage",
                        _ => continue,
                    };
                    let usages: Vec<u64> = self
                        .referrers_by(
                            o,
                            "shape_defining_relationship",
                            "shape_aspect_relationship",
                            "related_shape_aspect",
                        )
                        .into_iter()
                        .filter(|&r| {
                            self.get_str(r, "shape_aspect_relationship", "description")
                                .as_deref()
                                == Some(usage)
                        })
                        .collect();
                    // thread WR16 requires the 'thread runout' aspect and allows it no usage:
                    // a thread with no runout stated still has it, and it is the thread's.
                    if usages.is_empty() && is_thread && d == "thread runout" {
                        prov.push(o);
                    }
                    for r in usages {
                        let Some(src) =
                            self.get_ref(r, "shape_aspect_relationship", "relating_shape_aspect")
                        else {
                            continue;
                        };
                        match self.feature(st, src) {
                            Some(f) => {
                                prov.extend([o, r]);
                                if d == "thread runout" {
                                    runout = Some(f);
                                } else {
                                    partial_area = Some(f);
                                }
                            }
                            None => bad = true,
                        }
                    }
                }
                if bad {
                    self.find(
                        FindingKind::Unresolved,
                        part,
                        def,
                        &[],
                        "the thread or knurl has parameters or features that do not resolve; not read",
                    );
                    continue;
                }
                let built = if is_thread {
                    thread_of(feature, partial_area, runout, &mut params).map(Item::Thread)
                } else {
                    let pattern = self
                        .get_str(def, "characterized_object", "description")
                        .unwrap_or_default();
                    knurl_of(feature, &pattern, &mut params).map(Item::Knurl)
                };
                match built {
                    Ok(item) => {
                        if !params.is_empty() {
                            let names: Vec<&String> = params.keys().collect();
                            self.find(
                                FindingKind::Nonconformance,
                                part,
                                def,
                                &[],
                                format!("parameters {names:?} are not the schema's; ignored"),
                            );
                        }
                        match item {
                            Item::Thread(t) => {
                                st.pmi.threads.push(t);
                                st.prov.threads.push(ids(prov));
                                st.identifiers.threads.push(self.item_identifier(
                                    st.id,
                                    "threads",
                                    st.pmi.threads.len() - 1,
                                    def_shape,
                                ));
                            }
                            Item::Knurl(k) => {
                                st.pmi.knurls.push(k);
                                st.prov.knurls.push(ids(prov));
                                st.identifiers.knurls.push(self.item_identifier(
                                    st.id,
                                    "knurls",
                                    st.pmi.knurls.len() - 1,
                                    def_shape,
                                ));
                            }
                        }
                    }
                    Err(why) => self.find(FindingKind::Nonconformance, part, def, &[], why),
                }
            }
        }
    }

    // --- part properties ------------------------------------------------------------------

    /// The dimensioning standard (§4 Figure 1): an `applied_document_reference` of a
    /// `product_definition_context` that is associated with the part's product definition and
    /// whose application context is 'geometrical dimensioning and tolerancing
    /// representation'. The modelling standard (Figure 2, 'model based 3d annotation
    /// presentation') is presentation and not read.
    fn standard(&mut self, st: &mut PartState, pd: u64) {
        let part = Some(st.id);
        for pdca in self.referrers_by(
            pd,
            "product_definition_context_association",
            "product_definition_context_association",
            "definition",
        ) {
            let Some(ctx) = self.get_ref(
                pdca,
                "product_definition_context_association",
                "frame_of_reference",
            ) else {
                continue;
            };
            let app = self.get_ref(ctx, "application_context_element", "frame_of_reference");
            let app_name = app
                .and_then(|a| self.get_str(a, "application_context", "application"))
                .unwrap_or_default();
            if app_name != "geometrical dimensioning and tolerancing representation" {
                continue;
            }
            for adr in self.referrers_by(
                ctx,
                "applied_document_reference",
                "applied_document_reference",
                "items",
            ) {
                let Some(doc) = self.get_ref(adr, "document_reference", "assigned_document") else {
                    continue;
                };
                let mut prov = vec![pdca, adr, doc];
                prov.extend(self.get_ref(pdca, "product_definition_context_association", "role"));
                prov.extend(self.get_ref(doc, "document", "kind"));
                for ra in self.referrers_by(
                    adr,
                    "role_association",
                    "role_association",
                    "item_with_role",
                ) {
                    prov.push(ra);
                    prov.extend(self.get_ref(ra, "role_association", "role"));
                }
                let doc_id = self.get_str(doc, "document", "id").unwrap_or_default();
                let mut edition = None;
                let mut product_id = None;
                for dpe in self.referrers_by(
                    doc,
                    "document_product_equivalence",
                    "document_product_association",
                    "relating_document",
                ) {
                    prov.push(dpe);
                    if let Some(rp) =
                        self.get_ref(dpe, "document_product_association", "related_product")
                    {
                        if self.is(rp, "product_definition_formation") {
                            edition = self
                                .get_str(rp, "product_definition_formation", "id")
                                .filter(|s| !s.trim().is_empty());
                            if let Some(p) =
                                self.get_ref(rp, "product_definition_formation", "of_product")
                            {
                                product_id = self.get_str(p, "product", "id");
                            }
                        } else if self.is(rp, "product") {
                            product_id = self.get_str(rp, "product", "id");
                        }
                    }
                }
                let document = if doc_id.trim().is_empty() {
                    product_id.clone().unwrap_or_default()
                } else {
                    doc_id
                };
                let document = document.trim().to_string();
                let key = edition.clone().unwrap_or_else(|| document.clone());
                let body = if key.starts_with("ASME") || document.starts_with("ASME") {
                    StandardBody::Asme
                } else if key.starts_with("ISO") || document.starts_with("ISO") {
                    StandardBody::Iso
                } else {
                    StandardBody::Other
                };
                if document.is_empty() && edition.is_none() {
                    self.find(
                        FindingKind::Nonconformance,
                        part,
                        adr,
                        &[doc],
                        "dimensioning standard reference names no document; not read",
                    );
                    continue;
                }
                let s = Standard {
                    document,
                    edition,
                    body,
                };
                if let Some(i) = st.pmi.standards.iter().position(|x| x == &s) {
                    let p = &mut st.prov.standards[i];
                    p.extend(prov);
                    *p = ids(std::mem::take(p));
                    continue;
                }
                st.pmi.standards.push(s);
                st.prov.standards.push(ids(prov));
            }
        }
    }

    /// The property definitions on `owner` (the part's product definition or shape, a shape
    /// aspect, a dimension, a tolerance): general tolerances, material, notes, attributes.
    fn properties(&mut self, st: &mut PartState, owner: u64, as_item: Option<NoteOwner>) {
        let part = Some(st.id);
        let pdefs = self.referrers_by(
            owner,
            "property_definition",
            "property_definition",
            "definition",
        );
        for pdef in pdefs {
            if self.is(pdef, "product_definition_shape") {
                continue; // the shape itself, not a property of it
            }
            if self.family(pdef) == Family::ValidationProperty {
                continue;
            }
            if self.consumed_elsewhere(st, pdef) {
                continue;
            }
            let name = self
                .get_str(pdef, "property_definition", "name")
                .unwrap_or_default();
            let description = self
                .get_str(pdef, "property_definition", "description")
                .unwrap_or_default();
            let gpas = self.referrers_by(
                pdef,
                "general_property_association",
                "general_property_association",
                "derived_definition",
            );
            // The thing it is on.
            let on = if owner == st.part.product_definition || owner == st.part.shape {
                Some(None)
            } else if let Some(o) = as_item {
                Some(Some(o))
            } else if self.is(owner, "datum_target") {
                st.targets
                    .get(&owner)
                    .copied()
                    .flatten()
                    .map(|t| Some(NoteOwner::DatumTarget(t)))
            } else if self.is(owner, "shape_aspect") {
                self.feature(st, owner).map(|f| Some(NoteOwner::Feature(f)))
            } else {
                None
            };
            // A property on a shape aspect that identifies nothing (NIST CTC-02's general notes):
            // the aspect names no face, so the property is read as on the part, reported.
            let on = match on {
                None if self.is(owner, "shape_aspect")
                    && !self.is(owner, "datum_target")
                    && self.usages(owner).is_empty()
                    && self.members(owner).is_empty() =>
                {
                    self.find(
                        FindingKind::Nonconformance,
                        part,
                        pdef,
                        &[owner],
                        "a property on a shape aspect that identifies no face or edge; read as on the part",
                    );
                    Some(None)
                }
                on => on,
            };
            let Some(on) = on else {
                let held = self
                    .get_str(owner, "shape_aspect", "description")
                    .as_deref()
                    != Some("CATIA Geometric Set");
                self.find(
                    if held {
                        FindingKind::Unresolved
                    } else {
                        FindingKind::NotModelled
                    },
                    part,
                    pdef,
                    &[owner],
                    if held {
                        "the item the property is on was not read; property not read"
                    } else {
                        "property of a CATIA geometric set, which the model does not hold; not read"
                    },
                );
                continue;
            };
            // A surface texture parameter (ISO 10303-1110) is read with its surface texture:
            // named 'surface texture parameter' as the mapping has it, or 'surface_condition'
            // as `pmi::write` names it (general_property_association WR2 with
            // surface_texture_representation WR5).
            let mapped = name == "surface texture parameter";
            if mapped || name == "surface_condition" {
                if self.has_texture(pdef) {
                    continue;
                }
                if mapped {
                    self.find(
                        FindingKind::Unsupported,
                        part,
                        pdef,
                        &[owner],
                        "a surface texture parameter of no surface texture (ISO 10303-1110: related from its 'surface texture' by a 'surface texture parameter' property_definition_relationship); not read",
                    );
                    continue;
                }
            }
            #[cfg(feature = "json")]
            if name == "haecceity.pmi/1 ordering" {
                st.prov.transport.push(pdef);
                for (link, rep) in self.property_representations(pdef) {
                    st.prov.transport.extend([link, rep]);
                    st.prov
                        .transport
                        .extend(self.get_refs(rep, "representation", "items"));
                }
                continue;
            }
            // User defined attributes (UDA practice §5: associated with a general_property),
            // and editable note text (PMI practice §7.4, UDA practice §6.4.1: 'semantic text',
            // which some files write without the general_property).
            if !gpas.is_empty()
                || name == "semantic text"
                || self
                    .property_representations(pdef)
                    .iter()
                    .any(|(_, rep)| self.name_of(*rep) == "haecceity.pmi/1 attribute")
            {
                self.attribute_set(st, pdef, &name, on, &gpas);
                continue;
            }
            match (name.as_str(), description.as_str(), on) {
                ("surface texture", _, _) => self.surface_texture(st, pdef, on),
                ("default tolerances", _, None) => self.general_tolerance(st, pdef),
                ("material property", _, None) => self.material(st, pdef, &description),
                ("manufacturing requirement", _, _) => self.note(st, pdef, &description, on),
                _ => {
                    let reps = self.property_representations(pdef);
                    if reps.is_empty() && name.is_empty() {
                        continue; // an empty property carries nothing
                    }
                    self.find(
                        FindingKind::NotModelled,
                        part,
                        pdef,
                        &[owner],
                        format!("property {name:?} ({description:?}) is not a construct the reader knows; not read"),
                    );
                }
            }
        }
    }

    /// A user defined attribute (UDA practice §5–§7): the property definition associated with
    /// a `general_property`, its values the items of its representations.
    fn attribute_set(
        &mut self,
        st: &mut PartState,
        pdef: u64,
        name: &str,
        on: Option<NoteOwner>,
        gpas: &[u64],
    ) {
        let part = Some(st.id);
        let mut prov = vec![pdef];
        prov.extend(self.id_attributes(pdef));
        for &g in gpas {
            prov.push(g);
            if let Some(gp) = self.get_ref(g, "general_property_association", "base_definition") {
                prov.push(gp);
            }
        }
        let mut items = Vec::new();
        for (pdr, rep) in self.property_representations(pdef) {
            prov.extend([pdr, rep]);
            for item in self.get_refs(rep, "representation", "items") {
                let key = self.name_of(item);
                let v = if self.is_text(item) {
                    AttributeValue::Text(self.text_of(item).unwrap_or_default())
                } else if self.is(item, "integer_representation_item")
                    || self.is(item, "real_representation_item")
                {
                    let a = self.get(item, "literal_number", "the_value");
                    match a {
                        Some(Attribute::Integer(i)) => AttributeValue::Integer(*i),
                        Some(Attribute::Real(r))
                            if self.is(item, "integer_representation_item") =>
                        {
                            if r.fract() == 0.0 {
                                AttributeValue::Integer(*r as i64)
                            } else {
                                self.find(
                                    FindingKind::Nonconformance,
                                    part,
                                    item,
                                    &[pdef],
                                    "integer item with a fractional value; attribute not read",
                                );
                                return;
                            }
                        }
                        Some(r @ Attribute::Real(_)) => match self
                            .real_text(item, r)
                            .and_then(|t| Decimal::parse(&t).ok())
                        {
                            Some(d) => AttributeValue::Real(d),
                            None => return,
                        },
                        _ => {
                            self.find(
                                FindingKind::Nonconformance,
                                part,
                                item,
                                &[pdef],
                                "number item without a number; attribute not read",
                            );
                            return;
                        }
                    }
                } else if self.is(item, "boolean_representation_item") {
                    match enum_value(self.get(item, "boolean_representation_item", "the_value")) {
                        Some("T") => AttributeValue::Boolean(true),
                        Some("F") => AttributeValue::Boolean(false),
                        _ => {
                            self.find(
                                FindingKind::Nonconformance,
                                part,
                                item,
                                &[pdef],
                                "boolean item without .T. or .F.; attribute not read",
                            );
                            return;
                        }
                    }
                } else if self.is(item, "measure_with_unit") {
                    match self.measure(item) {
                        Ok(Measured::Length(l)) => AttributeValue::Measure(Value {
                            quantity: Quantity::Length(l),
                            decimal_places: None,
                        }),
                        Ok(Measured::Angle(a)) => AttributeValue::Measure(Value {
                            quantity: Quantity::Angle(a),
                            decimal_places: None,
                        }),
                        Ok(Measured::Ratio(d)) => AttributeValue::OtherMeasure {
                            measure: "ratio_measure".into(),
                            value: d,
                            unit: String::new(),
                        },
                        Ok(Measured::Count(d)) => AttributeValue::OtherMeasure {
                            measure: "count_measure".into(),
                            value: d,
                            unit: String::new(),
                        },
                        Ok(Measured::Other {
                            measure,
                            value,
                            unit,
                        }) => AttributeValue::OtherMeasure {
                            measure,
                            value,
                            unit,
                        },
                        Err(e) => {
                            self.find(e.kind, part, item, &[pdef], e.why);
                            return;
                        }
                    }
                } else if self.is(item, "value_representation_item") {
                    match self.get(item, "value_representation_item", "value_component") {
                        Some(Attribute::Typed { type_name, value }) => {
                            let text = self.real_text(item, value).unwrap_or_default();
                            match Decimal::parse(&text) {
                                Ok(d) => AttributeValue::OtherMeasure {
                                    measure: type_name.to_ascii_lowercase(),
                                    value: d,
                                    unit: String::new(),
                                },
                                Err(_) => {
                                    self.find(
                                        FindingKind::Nonconformance,
                                        part,
                                        item,
                                        &[pdef],
                                        "value item without a number; attribute not read",
                                    );
                                    return;
                                }
                            }
                        }
                        _ => {
                            self.find(
                                FindingKind::Nonconformance,
                                part,
                                item,
                                &[pdef],
                                "value item without a typed value; attribute not read",
                            );
                            return;
                        }
                    }
                } else {
                    self.find(
                        FindingKind::Unsupported,
                        part,
                        item,
                        &[pdef],
                        format!("attribute value of type {} (UDA practice §7 defines descriptive, value and measure items); attribute not read", self.entity_label(item)),
                    );
                    return;
                };
                prov.push(item);
                items.push((key, v));
            }
        }
        let marked = self
            .property_representations(pdef)
            .iter()
            .any(|(_, rep)| self.name_of(*rep) == "haecceity.pmi/1 attribute");
        let description = self
            .get_str(pdef, "property_definition", "description")
            .filter(|d| marked || !d.is_empty());
        let general_property = self.general_property(part, pdef, gpas);
        st.pmi.attributes.push(AttributeSet {
            name: name.to_string(),
            on,
            items,
            description,
            general_property,
        });
        st.prov.attributes.push(ids(prov));
        st.identifiers.attributes.push(self.item_identifier(
            st.id,
            "attributes",
            st.pmi.attributes.len() - 1,
            pdef,
        ));
    }

    /// The `general_property` an attribute's property definition is associated with by its
    /// associations `gpas`: one, naming a `general_property`; otherwise none, reported
    /// (several: `general_property_association` WR1; a base that is not one: unresolved).
    fn general_property(
        &mut self,
        part: Option<PartId>,
        pdef: u64,
        gpas: &[u64],
    ) -> Option<GeneralProperty> {
        let g = match gpas {
            [] => return None,
            [g] => *g,
            _ => {
                self.find(
                    FindingKind::Nonconformance,
                    part,
                    pdef,
                    gpas,
                    format!("a user defined attribute derived by {} general property associations (general_property_association WR1: exactly 1); its general property is not held", gpas.len()),
                );
                return None;
            }
        };
        match self.get_ref(g, "general_property_association", "base_definition") {
            Some(gp) if self.is(gp, "general_property") => Some(GeneralProperty {
                id: self
                    .get_str(gp, "general_property", "id")
                    .unwrap_or_default(),
                name: self
                    .get_str(gp, "general_property", "name")
                    .unwrap_or_default(),
                description: self.get_str(gp, "general_property", "description"),
            }),
            gp => {
                self.find(
                    FindingKind::Unresolved,
                    part,
                    g,
                    &[pdef],
                    format!(
                        "general property association's base definition {} is not a general_property; the attribute's general property is not held",
                        gp.map_or("(unset)".to_string(), |gp| format!("#{gp}"))
                    ),
                );
                None
            }
        }
    }

    /// The general tolerance (decision 3): 'tolerance class' items of the 'default
    /// tolerances' representation, and a `default_tolerance_table` related to it (§4.1).
    fn general_tolerance(&mut self, st: &mut PartState, pdef: u64) {
        let part = Some(st.id);
        let reps = self.property_representations(pdef);
        if reps.is_empty() {
            self.find(
                FindingKind::Nonconformance,
                part,
                pdef,
                &[],
                "'default tolerances' property without a representation; not read",
            );
            return;
        }
        let mut class_count = 0;
        for (_, rep) in &reps {
            for item in self.get_refs(*rep, "representation", "items") {
                if self.is_text(item)
                    && self.name_of(item) == "tolerance class"
                    && self
                        .text_of(item)
                        .is_some_and(|text| !text.trim().is_empty())
                {
                    class_count += 1;
                }
            }
        }
        let mut class_twins = BTreeMap::<String, usize>::new();
        for (pdr, rep) in reps {
            let mut base = vec![pdef, pdr, rep];
            base.extend(self.id_attributes(pdef));
            let mut read_any = false;
            for item in self.get_refs(rep, "representation", "items") {
                let n = self.name_of(item);
                if self.is_text(item) && n == "tolerance class" {
                    let text = self.text_of(item).unwrap_or_default();
                    if text.trim().is_empty() {
                        continue; // §4.1 Figure 3 writes '' when only the table applies
                    }
                    let standard = Iso2768::recognise(&text);
                    let twin = class_twins.entry(text.clone()).or_default();
                    *twin += 1;
                    let identifier = self
                        .item_identifier(st.id, "general", st.pmi.general.len(), pdef)
                        .map(|id| {
                            if class_count <= 1 || id.is_empty() {
                                id
                            } else {
                                let qualifier = text
                                    .as_bytes()
                                    .iter()
                                    .map(|byte| format!("{byte:02x}"))
                                    .collect::<String>();
                                let base = format!("{id}/tolerance-class/{qualifier}");
                                if *twin == 1 {
                                    base
                                } else {
                                    format!("{base}~{twin}")
                                }
                            }
                        });
                    st.pmi
                        .general
                        .push(GeneralTolerance::Class { text, standard });
                    let mut p = base.clone();
                    p.push(item);
                    st.prov.general.push(ids(p));
                    st.identifiers.general.push(identifier);
                    read_any = true;
                } else {
                    self.find(
                        FindingKind::Unsupported,
                        part,
                        item,
                        &[pdef],
                        format!("default tolerances item {n:?} is not 'tolerance class'; not read"),
                    );
                }
            }
            for rr in self.referrers_by(
                rep,
                "representation_relationship",
                "representation_relationship",
                "rep_2",
            ) {
                let rname = self
                    .get_str(rr, "representation_relationship", "name")
                    .unwrap_or_default();
                let Some(table) = self.get_ref(rr, "representation_relationship", "rep_1") else {
                    continue;
                };
                if rname != "general tolerance definition"
                    || !self.is(table, "default_tolerance_table")
                {
                    continue;
                }
                let mut p = base.clone();
                p.extend([rr, table]);
                p.extend(self.id_attributes(table));
                let tname = self
                    .get_str(table, "representation", "name")
                    .unwrap_or_default();
                let mut cells = Vec::new();
                let mut places = None;
                for cell in self.get_refs(table, "representation", "items") {
                    p.push(cell);
                    let mut items = Vec::new();
                    for ci in self.get_refs(cell, "compound_representation_item", "item_element") {
                        p.push(ci);
                        let n = self.name_of(ci);
                        let v = if self.is_text(ci) {
                            CellValue::Text(self.text_of(ci).unwrap_or_default())
                        } else {
                            match self.measure(ci) {
                                Ok(Measured::Count(d)) => {
                                    if n == "number of decimal places" {
                                        places = whole(&d);
                                        if places.is_none() {
                                            self.find(
                                                FindingKind::Nonconformance,
                                                part,
                                                ci,
                                                &[table],
                                                format!("'number of decimal places' {d} is not a whole count; the cell is kept as stated, the part's decimal places not set"),
                                            );
                                        }
                                    }
                                    CellValue::Count(d)
                                }
                                Ok(Measured::Length(_) | Measured::Angle(_)) => {
                                    let Some(value) = self.value(st, ci, Want::Any, &mut p) else {
                                        return;
                                    };
                                    CellValue::Value(value)
                                }
                                Ok(other) => {
                                    self.find(
                                        FindingKind::Unsupported,
                                        part,
                                        ci,
                                        &[table],
                                        format!("table cell value {other:?}; table not read"),
                                    );
                                    return;
                                }
                                Err(e) => {
                                    self.find(e.kind, part, ci, &[table], e.why);
                                    return;
                                }
                            }
                        };
                        items.push((n, v));
                    }
                    cells.push(ToleranceCell { items });
                }
                if let Some(pl) = places {
                    st.pmi.decimal_places = Some(pl);
                    st.prov.decimal_places = ids(p.clone());
                }
                if !(tname.is_empty()
                    && matches!(cells.as_slice(), [c] if c.decimal_default().is_some()))
                {
                    st.pmi
                        .general
                        .push(GeneralTolerance::Table { name: tname, cells });
                    st.prov.general.push(ids(p));
                    st.identifiers.general.push(self.item_identifier(
                        st.id,
                        "general",
                        st.pmi.general.len() - 1,
                        table,
                    ));
                }
                read_any = true;
            }
            if !read_any {
                self.find(
                    FindingKind::Nonconformance,
                    part,
                    pdef,
                    &[rep],
                    "'default tolerances' states neither a tolerance class nor a table; not read",
                );
            }
        }
    }

    /// Material (CAx-IF *Material Identification and Density* §4.1 name, §4.2 density).
    fn material(&mut self, st: &mut PartState, pdef: u64, description: &str) {
        let part = Some(st.id);
        let reps = self.property_representations(pdef);
        let mut m = st.pmi.material.clone().unwrap_or(Material {
            id: String::new(),
            name: None,
            density: None,
        });
        let mut prov = vec![pdef];
        match description {
            "material name" => {
                let mut found = None;
                for (pdr, rep) in reps {
                    for item in self.get_refs(rep, "representation", "items") {
                        if self.is_text(item) {
                            if found.is_some() {
                                self.find(
                                    FindingKind::Conflict,
                                    part,
                                    item,
                                    &[pdef],
                                    "several material names; not read",
                                );
                                return;
                            }
                            let id = self
                                .get_str(item, "representation_item", "name")
                                .unwrap_or_default();
                            let name = self.text_of(item).filter(|s| !s.is_empty());
                            // Preserve absence of a separate name when the writer
                            // supplies the ID as nonempty text for other consumers.
                            let identifier_fallback =
                                self.get_str(rep, "representation", "name").as_deref()
                                    == Some(super::MATERIAL_ID_FALLBACK_REP)
                                    && name.as_deref() == Some(id.as_str());
                            found = Some((id, if identifier_fallback { None } else { name }));
                            prov.extend([pdr, rep, item]);
                        }
                    }
                }
                let Some((id, name)) = found else {
                    self.find(
                        FindingKind::Nonconformance,
                        part,
                        pdef,
                        &[],
                        "'material name' property without a descriptive item (§4.1); not read",
                    );
                    return;
                };
                if !m.id.is_empty() {
                    self.find(
                        FindingKind::Conflict,
                        part,
                        pdef,
                        &[],
                        "a second material name on the part; not read",
                    );
                    return;
                }
                m.id = id;
                m.name = name;
            }
            "density" => {
                let mut found = None;
                for (pdr, rep) in reps {
                    for item in self.get_refs(rep, "representation", "items") {
                        if !self.is(item, "measure_representation_item") {
                            continue;
                        }
                        let component = self.get(item, "measure_with_unit", "value_component");
                        let untyped = matches!(component, Some(Attribute::Real(_)));
                        let value = match component {
                            Some(Attribute::Typed { value, .. }) => Some(value.as_ref()),
                            Some(value @ Attribute::Real(_)) => Some(value),
                            _ => None,
                        }
                        .and_then(|value| self.real_text(item, value))
                        .and_then(|text| Decimal::parse(&text).ok());
                        let unit = self.get_ref(item, "measure_with_unit", "unit_component");
                        let (Some(value), Some(unit)) = (value, unit) else {
                            self.find(
                                FindingKind::Unitless,
                                part,
                                item,
                                &[pdef],
                                "density without a value or unit (§4.2); not read",
                            );
                            return;
                        };
                        let Some(mut elements) = self.derived_unit(unit) else {
                            self.find(
                                FindingKind::Unitless,
                                part,
                                item,
                                &[pdef, unit],
                                "density unit is not a derived unit of named units (§4.2); not read",
                            );
                            return;
                        };
                        if self.derived_dimensions(unit) != Some([-3., 1., 0., 0., 0., 0., 0.]) {
                            if let Some(exponents) = self.occt_density_exponents(unit) {
                                for ((_, exponent), corrected) in elements.iter_mut().zip(exponents)
                                {
                                    *exponent = corrected;
                                }
                                self.find(FindingKind::Nonconformance, part, item, &[pdef, unit],
                                    "density unit read as mass / length cubed (OCCT writes mass^3 · length^2); base units and value retained");
                            } else {
                                self.find(FindingKind::Nonconformance, part, item, &[pdef, unit],
                                    "density derived unit is not mass / length cubed (mass 1, length -3); not read");
                                return;
                            }
                        }
                        if untyped {
                            self.find(FindingKind::Nonconformance, part, item, &[pdef],
                                "density value untyped (OCCT); read the bare REAL as the density value");
                        }
                        found = Some(Density {
                            value,
                            unit: elements,
                        });
                        prov.extend([pdr, rep, item]);
                    }
                }
                let Some(d) = found else {
                    self.find(
                        FindingKind::Nonconformance,
                        part,
                        pdef,
                        &[],
                        "'density' property without a measure item (§4.2); not read",
                    );
                    return;
                };
                m.density = Some(d);
            }
            other => {
                self.find(
                    FindingKind::NotModelled,
                    part,
                    pdef,
                    &[],
                    format!("material property {other:?} is neither 'material name' nor 'density'; not read"),
                );
                return;
            }
        }
        st.pmi.material = Some(m);
        st.prov.material.extend(prov);
        st.prov.material = ids(std::mem::take(&mut st.prov.material));
    }

    /// Inspect dimensions, not unit names: custom conversion-based mass/length
    /// units remain readable even when the writer does not know their scale name.
    fn derived_dimensions(&mut self, unit: u64) -> Option<[f64; 7]> {
        let kinds = [
            "length_unit",
            "mass_unit",
            "time_unit",
            "electric_current_unit",
            "thermodynamic_temperature_unit",
            "amount_of_substance_unit",
            "luminous_intensity_unit",
        ];
        let slots = [
            "length_exponent",
            "mass_exponent",
            "time_exponent",
            "electric_current_exponent",
            "thermodynamic_temperature_exponent",
            "amount_of_substance_exponent",
            "luminous_intensity_exponent",
        ];
        let number = |value: Option<&Attribute>| match value {
            Some(Attribute::Real(n)) => Some(*n),
            Some(Attribute::Integer(n)) => Some(*n as f64),
            _ => None,
        };
        let mut sum = [0.; 7];
        for element in self.get_refs(unit, "derived_unit", "elements") {
            let exponent = number(self.get(element, "derived_unit_element", "exponent"))?;
            let named = self.get_ref(element, "derived_unit_element", "unit")?;
            if let Some(dimensions) = self.get_ref(named, "named_unit", "dimensions") {
                for (i, slot) in slots.iter().enumerate() {
                    let power = number(self.get(dimensions, "dimensional_exponents", slot))?;
                    sum[i] += power * exponent;
                }
            } else if let Some(i) = kinds.iter().position(|kind| self.is(named, kind)) {
                sum[i] += exponent;
            } else if !self.is(named, "plane_angle_unit") && !self.is(named, "solid_angle_unit") {
                return None;
            }
        }
        Some(sum)
    }

    /// Only OCCT's two-factor mass^3 · length^2 material density is repairable.
    /// Do not accept additional factors, duplicate kinds or other wrong exponents.
    fn occt_density_exponents(&mut self, unit: u64) -> Option<Vec<Decimal>> {
        if self.derived_dimensions(unit) != Some([2., 3., 0., 0., 0., 0., 0.]) {
            return None;
        }
        let elements = self.get_refs(unit, "derived_unit", "elements");
        if elements.len() != 2 {
            return None;
        }
        let mut mass = false;
        let mut length = false;
        let mut corrected = Vec::new();
        for element in elements {
            let named = self.get_ref(element, "derived_unit_element", "unit")?;
            let exponent = match self.get(element, "derived_unit_element", "exponent")? {
                value @ (Attribute::Real(_) | Attribute::Integer(_)) => {
                    Decimal::parse(&self.real_text(element, value)?).ok()?
                }
                _ => return None,
            };
            let (expected, value) = if self.is(named, "mass_unit") && !mass {
                mass = true;
                ("3.", "1.")
            } else if self.is(named, "length_unit") && !length {
                length = true;
                ("2.", "-3.")
            } else {
                return None;
            };
            if !exponent.cmp_value(&Decimal::parse(expected).ok()?).is_eq() {
                return None;
            }
            corrected.push(Decimal::parse(value).ok()?);
        }
        (mass && length).then_some(corrected)
    }

    fn derived_unit(&mut self, unit: u64) -> Option<Vec<(String, Decimal)>> {
        if !self.is(unit, "derived_unit") {
            return None;
        }
        let mut out = Vec::new();
        for e in self.get_refs(unit, "derived_unit", "elements") {
            let u = self.get_ref(e, "derived_unit_element", "unit")?;
            let exp = match self.get(e, "derived_unit_element", "exponent")? {
                a @ (Attribute::Real(_) | Attribute::Integer(_)) => {
                    Decimal::parse(&self.real_text(e, a)?).ok()?
                }
                _ => return None,
            };
            let name = match self.unit(u) {
                Ok(Unit::Length(l)) => format!("{l:?}").to_ascii_lowercase(),
                Ok(Unit::Angle(a)) => format!("{a:?}").to_ascii_lowercase(),
                Ok(Unit::Other(n)) => n,
                Err(_) => return None,
            };
            out.push((name, exp));
        }
        Some(out)
    }

    /// specify-core's 'manufacturing requirement' property: a descriptive item's text.
    fn note(&mut self, st: &mut PartState, pdef: u64, kind: &str, on: Option<NoteOwner>) {
        let part = Some(st.id);
        let mut prov = vec![pdef];
        prov.extend(self.id_attributes(pdef));
        let mut texts = Vec::new();
        for (pdr, rep) in self.property_representations(pdef) {
            prov.extend([pdr, rep]);
            for item in self.get_refs(rep, "representation", "items") {
                if self.is_text(item) {
                    texts.push(self.text_of(item).unwrap_or_default());
                    prov.push(item);
                } else {
                    self.find(
                        FindingKind::Unsupported,
                        part,
                        item,
                        &[pdef],
                        "requirement item that is not descriptive; note not read",
                    );
                    return;
                }
            }
        }
        let [text] = texts.as_slice() else {
            self.find(
                FindingKind::Nonconformance,
                part,
                pdef,
                &[],
                format!(
                    "requirement with {} descriptive items (one expected); not read",
                    texts.len()
                ),
            );
            return;
        };
        st.pmi.notes.push(Note {
            kind: kind.to_string(),
            text: text.clone(),
            on,
            callouts: Vec::new(),
        });
        st.prov.notes.push(ids(prov));
        st.identifiers.notes.push(self.item_identifier(
            st.id,
            "notes",
            st.pmi.notes.len() - 1,
            pdef,
        ));
    }

    /// Whether a 'surface texture parameter' is related from a 'surface texture' property
    /// definition (ISO 10303-1110, `Surface_texture.parameters`), with which it is read.
    fn has_texture(&mut self, parameter: u64) -> bool {
        let rels = self.referrers_by(
            parameter,
            "property_definition_relationship",
            "property_definition_relationship",
            "related_property_definition",
        );
        for r in rels {
            if self.name_of_relationship(r) != "surface texture parameter" {
                continue;
            }
            let texture = self.get_ref(
                r,
                "property_definition_relationship",
                "relating_property_definition",
            );
            if let Some(t) = texture
                && self.get_str(t, "property_definition", "name").as_deref()
                    == Some("surface texture")
            {
                return true;
            }
        }
        false
    }

    fn name_of_relationship(&mut self, r: u64) -> String {
        self.get_str(r, "property_definition_relationship", "name")
            .unwrap_or_default()
    }

    /// A surface texture (ISO 10303-1110 `Surface_texture` and its
    /// `Standard_surface_texture_parameter`s, as `pmi::write` writes them): the 'material
    /// removal condition' of its representation and, per related 'surface texture parameter',
    /// the characteristic ('measuring method') and the length value of its
    /// `surface_texture_representation`. What the model does not hold (the texture's
    /// direction, manufacturing method, machining allowance; a parameter's evaluation length,
    /// filters, value range, other measures) is reported and the texture is not read.
    fn surface_texture(&mut self, st: &mut PartState, pdef: u64, on: Option<NoteOwner>) {
        let part = Some(st.id);
        let on = match on {
            None => None,
            Some(NoteOwner::Feature(f)) => Some(f),
            Some(o) => {
                self.find(
                    FindingKind::Unsupported,
                    part,
                    pdef,
                    &[],
                    format!("a surface texture on {o:?} (the model holds it on faces or the part); not read"),
                );
                return;
            }
        };
        let mut prov = vec![pdef];
        prov.extend(self.id_attributes(pdef));
        let mut removal = None;
        for (pdr, rep) in self.property_representations(pdef) {
            prov.extend([pdr, rep]);
            for item in self.get_refs(rep, "representation", "items") {
                let name = self.name_of(item);
                if !self.is_text(item) || name != "material removal condition" {
                    self.find(
                        FindingKind::Unsupported,
                        part,
                        item,
                        &[pdef],
                        format!("surface texture item {name:?} is not held by the model; texture not read"),
                    );
                    return;
                }
                let term = self.text_of(item).unwrap_or_default();
                match (MaterialRemoval::from_term(&term), removal) {
                    (Some(m), None) => removal = Some(m),
                    (Some(_), Some(_)) => {
                        self.find(
                            FindingKind::Conflict,
                            part,
                            item,
                            &[pdef],
                            "a second material removal condition; texture not read",
                        );
                        return;
                    }
                    (None, _) => {
                        self.find(
                            FindingKind::Nonconformance,
                            part,
                            item,
                            &[pdef],
                            format!("material removal condition {term:?} is none of ISO 10303-1110's terms; texture not read"),
                        );
                        return;
                    }
                }
                prov.push(item);
            }
        }
        let Some(material_removal) = removal else {
            self.find(
                FindingKind::Nonconformance,
                part,
                pdef,
                &[],
                "a surface texture without its material removal condition (mandatory, ISO 10303-1110); not read",
            );
            return;
        };
        let mut parameters = Vec::new();
        let rels = self.referrers_by(
            pdef,
            "property_definition_relationship",
            "property_definition_relationship",
            "relating_property_definition",
        );
        for r in rels {
            if self.name_of_relationship(r) != "surface texture parameter" {
                continue;
            }
            let Some(p) = self.get_ref(
                r,
                "property_definition_relationship",
                "related_property_definition",
            ) else {
                continue;
            };
            let pname = self
                .get_str(p, "property_definition", "name")
                .unwrap_or_default();
            if pname != "surface texture parameter" && pname != "surface_condition" {
                self.find(
                    FindingKind::Nonconformance,
                    part,
                    p,
                    &[pdef],
                    format!("a surface texture parameter named {pname:?} (ISO 10303-1110: 'surface texture parameter', or 'surface_condition' for general_property_association WR2); texture not read"),
                );
                return;
            }
            prov.extend([r, p]);
            prov.extend(self.id_attributes(p));
            // WR5 of its representation: exactly one association with the general property
            // 'surface_condition'.
            let gpas = self.referrers_by(
                p,
                "general_property_association",
                "general_property_association",
                "derived_definition",
            );
            let gp = match gpas[..] {
                [g] => self
                    .get_ref(g, "general_property_association", "base_definition")
                    .filter(|&gp| {
                        self.get_str(gp, "general_property", "name").as_deref()
                            == Some("surface_condition")
                    })
                    .map(|gp| (g, gp)),
                _ => None,
            };
            let Some((g, gp)) = gp else {
                self.find(
                    FindingKind::Nonconformance,
                    part,
                    p,
                    &[pdef],
                    "a surface texture parameter without exactly one association with the general property 'surface_condition' (surface_texture_representation WR5); texture not read",
                );
                return;
            };
            prov.extend([g, gp]);
            let mut characteristic = None;
            let mut value = None;
            for (pdr, rep) in self.property_representations(p) {
                prov.extend([pdr, rep]);
                for item in self.get_refs(rep, "representation", "items") {
                    let name = self.name_of(item);
                    if self.is_text(item) && name == "measuring method" && characteristic.is_none()
                    {
                        characteristic = self.text_of(item);
                    } else if self.is(item, "measure_representation_item")
                        && !self.is(item, "qualified_representation_item")
                        && name == "characteristic value"
                        && value.is_none()
                    {
                        match self.measure(item) {
                            Ok(Measured::Length(l)) => value = Some(l),
                            Ok(m) => {
                                self.find(
                                    FindingKind::Unsupported,
                                    part,
                                    item,
                                    &[pdef, p],
                                    format!("a surface texture value that is not a length ({m:?}); texture not read"),
                                );
                                return;
                            }
                            Err(e) => {
                                self.find(e.kind, part, item, &[pdef, p], e.why);
                                return;
                            }
                        }
                    } else {
                        self.find(
                            FindingKind::Unsupported,
                            part,
                            item,
                            &[pdef, p],
                            format!("surface texture parameter item {name:?} is not held by the model; texture not read"),
                        );
                        return;
                    }
                    prov.push(item);
                }
            }
            let (Some(characteristic), Some(value)) = (characteristic, value) else {
                self.find(
                    FindingKind::Nonconformance,
                    part,
                    p,
                    &[pdef],
                    "a surface texture parameter without its characteristic ('measuring method') or its value; texture not read",
                );
                return;
            };
            parameters.push(SurfaceTextureParameter {
                characteristic,
                value,
            });
        }
        if parameters.is_empty() {
            self.find(
                FindingKind::Unsupported,
                part,
                pdef,
                &[],
                "a surface texture without parameters (the model holds a texture by its parameters); not read",
            );
            return;
        }
        st.pmi.surface_textures.push(SurfaceTexture {
            on,
            material_removal,
            parameters,
        });
        st.prov.surface_textures.push(ids(prov));
        st.identifiers.surface_textures.push(self.item_identifier(
            st.id,
            "surface_textures",
            st.pmi.surface_textures.len() - 1,
            pdef,
        ));
    }

    fn presentation_associations(&mut self, id: u64, attribute: &str) -> Vec<u64> {
        self.referrers_by(
            id,
            "draughting_model_item_association",
            "item_identified_representation_usage",
            attribute,
        )
    }

    /// Include aliases absorbed by composite features and merged datums, without interpreting
    /// every consumed support instance as an independently presented semantic item.
    fn callout_aliases(&mut self, st: &PartState) {
        for (&id, feature) in &st.features {
            if let Some(feature) = feature {
                self.callout_targets
                    .entry(id)
                    .or_default()
                    .push(CalloutTarget {
                        part: st.id,
                        item: PresentedItem::Feature(*feature),
                    });
                if self.is(id, "datum_feature") {
                    for (i, datum) in st.pmi.datums.iter().enumerate() {
                        if datum.feature() == Some(*feature) {
                            self.callout_targets
                                .entry(id)
                                .or_default()
                                .push(CalloutTarget {
                                    part: st.id,
                                    item: PresentedItem::Datum(DatumId(i)),
                                });
                        }
                    }
                }
            }
        }
        // A property definition may state several general requirements; none has that
        // definition as its unique identifier carrier. Material is a part-level singleton.
        for (i, prov) in st.prov.general.iter().enumerate() {
            for &id in prov {
                if self.is(id, "property_definition") {
                    self.callout_targets
                        .entry(id)
                        .or_default()
                        .push(CalloutTarget {
                            part: st.id,
                            item: PresentedItem::General(i),
                        });
                }
            }
        }
        for &id in &st.prov.material {
            if self.is(id, "property_definition") && st.pmi.material.is_some() {
                self.callout_targets
                    .entry(id)
                    .or_default()
                    .push(CalloutTarget {
                        part: st.id,
                        item: PresentedItem::Material,
                    });
            }
        }
        for (&id, &datum) in &st.datums {
            self.callout_targets
                .entry(id)
                .or_default()
                .push(CalloutTarget {
                    part: st.id,
                    item: PresentedItem::Datum(datum),
                });
        }
    }

    /// Only presentation content, never styles, geometry, or a shared context.
    fn annotation_children(&mut self, id: u64) -> Vec<u64> {
        if self.is(id, "draughting_callout") {
            self.get_refs(id, "draughting_callout", "contents")
        } else if self.is(id, "annotation_plane") {
            self.get_refs(id, "annotation_plane", "elements")
        } else if self.is(id, "styled_item") {
            self.get_refs(id, "styled_item", "item")
                .into_iter()
                .filter(|child| self.graphic_items.contains(child))
                .collect()
        } else {
            Vec::new()
        }
    }

    fn read_callouts(&mut self) -> Vec<GraphicalCallout> {
        let graphics: Vec<_> = self.graphic_items.iter().copied().collect();
        let mut contained = BTreeSet::new();
        let mut trees = BTreeMap::new();
        let mut planes: HashMap<u64, Vec<u64>> = HashMap::new();
        for &id in &graphics {
            if self.is(id, "annotation_plane") {
                for child in self.annotation_children(id) {
                    planes.entry(child).or_default().push(id);
                }
            }
            if !self.is(id, "draughting_callout") {
                continue;
            }
            let mut seen = BTreeSet::from([id]);
            let mut pending = self.annotation_children(id);
            while let Some(child) = pending.pop() {
                if seen.insert(child) {
                    contained.insert(child);
                    pending.extend(self.annotation_children(child));
                }
            }
            trees.insert(id, seen);
        }
        let mut out = Vec::new();
        for id in graphics {
            if self.is(id, "annotation_plane")
                || (contained.contains(&id) && !self.is(id, "draughting_callout"))
            {
                continue;
            }
            let mut members = trees.remove(&id).unwrap_or_else(|| BTreeSet::from([id]));
            // An association can identify the holding plane rather than its element.
            let mut pending: Vec<_> = members.iter().copied().collect();
            while let Some(child) = pending.pop() {
                for &plane in planes.get(&child).into_iter().flatten() {
                    if members.insert(plane) {
                        pending.push(plane);
                    }
                }
            }
            let associations: BTreeSet<_> = members
                .into_iter()
                .flat_map(|member| self.presentation_associations(member, "identified_item"))
                .collect();
            let mut links = Vec::new();
            for association in associations {
                // A malformed/dangling definition still remains an unresolved link.
                let definition = self
                    .get_ref(
                        association,
                        "item_identified_representation_usage",
                        "definition",
                    )
                    .unwrap_or(0);
                let mut targets = self
                    .callout_targets
                    .get(&definition)
                    .cloned()
                    .unwrap_or_default();
                targets.sort();
                targets.dedup();
                links.push(CalloutLink {
                    association,
                    definition,
                    targets,
                    entity: self.entity_label(definition),
                });
            }
            out.push(GraphicalCallout {
                id,
                name: self.name_of(id),
                links,
            });
        }
        out
    }

    fn read_saved_views(&mut self) -> Vec<super::SavedView> {
        let mut views = Vec::new();
        let ids: Vec<_> = self.doc.ids().collect();
        for id in ids {
            if !self.is(id, "model_geometric_view") {
                continue;
            }
            let Some(camera) = self.get_ref(id, "characterized_item_within_representation", "item")
            else {
                continue;
            };
            let Some(representation) =
                self.get_ref(id, "characterized_item_within_representation", "rep")
            else {
                continue;
            };
            let name = self
                .get_str(id, "characterized_object", "name")
                .filter(|s| !s.is_empty())
                .or_else(|| self.get_str(camera, "representation_item", "name"))
                .unwrap_or_default();
            views.push(super::SavedView {
                id,
                name,
                camera,
                representation,
                contents: self.get_refs(representation, "representation", "items"),
                is_default: self.is(id, "default_model_geometric_view"),
            });
        }
        // Pre-edition-2 files can carry a camera without model_geometric_view.
        for rep in self.doc.ids().collect::<Vec<_>>() {
            if !self.is(rep, "draughting_model") {
                continue;
            }
            let contents = self.get_refs(rep, "representation", "items");
            for &camera in &contents {
                if !self.is(camera, "camera_model")
                    || views
                        .iter()
                        .any(|v| v.camera == camera && v.representation == rep)
                {
                    continue;
                }
                let name = self
                    .get_str(camera, "representation_item", "name")
                    .filter(|s| !s.is_empty())
                    .or_else(|| self.get_str(rep, "representation", "name"))
                    .unwrap_or_default();
                views.push(super::SavedView {
                    id: camera,
                    name,
                    camera,
                    representation: rep,
                    contents: contents.clone(),
                    is_default: false,
                });
            }
        }
        views.sort_by_key(|v| v.id);
        views
    }

    // --- accounting -----------------------------------------------------------------------

    /// Graphic annotations, plus the models/views that actually carry them. The EXPRESS
    /// presentation family is deliberately broader (it also drives safe removal), so it
    /// cannot answer whether a file contains graphic PMI. Walk only content links: never
    /// follow styles, colours, layers, invisibility, or a representation's shared context.
    fn graphic_presentation(&mut self) -> BTreeSet<u64> {
        let all: Vec<u64> = self.doc.ids().collect();
        let mut graphic = BTreeSet::new();
        let mut containers = BTreeSet::new();
        let mut parents: HashMap<u64, Vec<u64>> = HashMap::new();
        for id in all {
            if self.is(id, "annotation_occurrence") || self.is(id, "draughting_callout") {
                graphic.insert(id);
                self.graphic_items.insert(id);
            }
            let children = if self.is(id, "draughting_model")
                || self.is(id, "presentation_representation")
                || self.is(
                    id,
                    "mechanical_design_geometric_presentation_representation",
                ) {
                containers.insert(id);
                self.get_refs(id, "representation", "items")
            } else if self.is(id, "model_geometric_view") {
                containers.insert(id);
                self.get_refs(id, "characterized_item_within_representation", "rep")
            } else if self.is(id, "mapped_item") {
                self.get_refs(id, "mapped_item", "mapping_source")
            } else if self.is(id, "representation_map") {
                self.get_refs(id, "representation_map", "mapped_representation")
            } else if self.is(id, "styled_item") {
                self.get_refs(id, "styled_item", "item")
            } else if self.is(id, "compound_representation_item") {
                self.get_refs(id, "compound_representation_item", "item_element")
            } else {
                Vec::new()
            };
            for child in children {
                parents.entry(child).or_default().push(id);
            }
        }
        let mut reached = graphic.clone();
        let mut pending: Vec<u64> = graphic.iter().copied().collect();
        while let Some(child) = pending.pop() {
            for &parent in parents.get(&child).into_iter().flatten() {
                if reached.insert(parent) {
                    if containers.contains(&parent) {
                        graphic.insert(parent);
                    }
                    pending.push(parent);
                }
            }
        }
        graphic
    }

    fn account(&mut self, provenance: &Provenance) -> Accounting {
        let mut consumed: BTreeSet<u64> = BTreeSet::new();
        for p in &provenance.parts {
            consumed.extend(p.all());
        }
        let mut acc = Accounting {
            presentation: self.graphic_presentation().len(),
            ..Accounting::default()
        };
        let all: Vec<u64> = self.doc.ids().collect();
        let mut unconsumed = Vec::new();
        for id in all {
            match self.family(id) {
                Family::Presentation => {}
                Family::ValidationProperty => acc.validation += 1,
                Family::SemanticPmi => {
                    acc.semantic += 1;
                    if consumed.contains(&id) {
                        acc.consumed += 1;
                    } else {
                        unconsumed.push(id);
                    }
                }
                Family::Other => {}
            }
        }
        let named: BTreeSet<u64> = self
            .findings
            .iter()
            .flat_map(|f| f.ids.iter().copied())
            .collect();
        // PMI on assemblies and occurrences first, so that what belongs to it is classed with it.
        let mut rest = Vec::new();
        for &id in &unconsumed {
            if named.contains(&id) {
                continue;
            }
            let (kind, part, detail) = self.classify_unconsumed(id);
            if matches!(kind, FindingKind::AssemblyPmi | FindingKind::OccurrencePmi) {
                self.find(kind, part, id, &[], detail);
            } else {
                rest.push((id, kind, part, detail));
            }
        }
        let named: BTreeSet<u64> = self
            .findings
            .iter()
            .flat_map(|f| f.ids.iter().copied())
            .collect();
        for (id, kind, part, detail) in rest {
            if let Some((owner, opart, okind)) = self.unread_owner(id, &named) {
                let label = self.entity_label(owner);
                self.find(
                    okind,
                    opart,
                    id,
                    &[owner],
                    format!("part of {label} #{owner}, which is not read (its finding says why)"),
                );
            } else {
                self.find(kind, part, id, &[], detail);
            }
        }
        // Counted from the findings as made, not assumed: an unconsumed instance no finding
        // names leaves `consumed + reported` short of `semantic`.
        let named: BTreeSet<u64> = self
            .findings
            .iter()
            .flat_map(|f| f.ids.iter().copied())
            .collect();
        acc.reported = unconsumed.iter().filter(|id| named.contains(id)).count();
        acc
    }

    /// An instance within two references (either way) of `id` that a finding names: the item
    /// `id` belongs to, which was not read; with that finding's part and kind.
    fn unread_owner(
        &mut self,
        id: u64,
        named: &BTreeSet<u64>,
    ) -> Option<(u64, Option<PartId>, FindingKind)> {
        let neighbours = |r: &Self, x: u64| -> Vec<u64> {
            let mut out: Vec<u64> = r.doc.referrers(x).to_vec();
            if let Some(e) = r.doc.get(x) {
                visit_attributes(e, &mut |a| {
                    if let Attribute::EntityRef(n) = a {
                        out.push(*n);
                    }
                });
            }
            out
        };
        let mut frontier = vec![id];
        let mut seen = BTreeSet::from([id]);
        for _ in 0..2 {
            let mut next = Vec::new();
            for x in frontier {
                for n in neighbours(self, x) {
                    if !seen.insert(n)
                        || self.is(n, "named_unit")
                        || self.is(n, "representation_context")
                        || self.is(n, "product_definition_shape")
                    {
                        continue;
                    }
                    if named.contains(&n) {
                        let f = self.findings.iter().find(|f| f.ids.contains(&n))?;
                        let kind = match f.kind {
                            FindingKind::AssemblyPmi | FindingKind::OccurrencePmi => f.kind,
                            _ => FindingKind::Unconsumed,
                        };
                        return Some((n, f.part, kind));
                    }
                    next.push(n);
                }
            }
            frontier = next;
        }
        None
    }

    /// Whether a product definition has components (it relates a `next_assembly_usage_occurrence`).
    fn is_assembly(&mut self, pd: u64) -> bool {
        !self
            .referrers_by(
                pd,
                "assembly_component_usage",
                "product_definition_relationship",
                "relating_product_definition",
            )
            .is_empty()
    }

    /// Why a semantic instance no part consumed: PMI on an assembly or an occurrence, or not
    /// reached.
    fn classify_unconsumed(&mut self, id: u64) -> (FindingKind, Option<PartId>, String) {
        // The product definition shape the instance hangs off, through a few typical links.
        let mut frontier = vec![id];
        let mut seen = BTreeSet::new();
        for _ in 0..4 {
            let mut next = Vec::new();
            for x in frontier {
                if !seen.insert(x) {
                    continue;
                }
                if self.is(x, "product_definition_shape") {
                    if let Some(&p) = self.shape_part.get(&x) {
                        return (
                            FindingKind::Unconsumed,
                            Some(PartId(p)),
                            "on the part but not reached by any PMI item the reader reads".into(),
                        );
                    }
                    let def = self.get_ref(x, "property_definition", "definition");
                    return match def {
                        Some(d) if self.is(d, "product_definition_relationship") => (
                            FindingKind::OccurrencePmi,
                            None,
                            format!("PMI on an occurrence ({}): out of scope", self.entity_label(d)),
                        ),
                        Some(d) if self.is(d, "product_definition") && self.is_assembly(d) => (
                            FindingKind::AssemblyPmi,
                            None,
                            "PMI on a product definition that is not a part (an assembly): out of scope"
                                .into(),
                        ),
                        Some(d) if self.is(d, "product_definition") => (
                            FindingKind::Unconsumed,
                            None,
                            "on a product definition that is not one of the parts read (no B-rep shape to anchor to)"
                                .into(),
                        ),
                        _ => (
                            FindingKind::Unconsumed,
                            None,
                            "on a product definition shape of nothing the reader knows".into(),
                        ),
                    };
                }
                if self.is(x, "product_definition") {
                    if let Some(p) = self.parts.iter().position(|p| p.product_definition == x) {
                        return (
                            FindingKind::Unconsumed,
                            Some(PartId(p)),
                            "on the part but not reached by any PMI item the reader reads".into(),
                        );
                    }
                    return if self.is_assembly(x) {
                        (
                            FindingKind::AssemblyPmi,
                            None,
                            "PMI on a product definition that is not a part (an assembly): out of scope"
                                .into(),
                        )
                    } else {
                        (
                            FindingKind::Unconsumed,
                            None,
                            "on a product definition that is not one of the parts read (no B-rep shape to anchor to)"
                                .into(),
                        )
                    };
                }
                if let Some(e) = self.doc.get(x) {
                    let mut rs = Vec::new();
                    visit_attributes(e, &mut |a| {
                        if let Attribute::EntityRef(n) = a {
                            rs.push(*n);
                        }
                    });
                    next.extend(rs);
                }
            }
            frontier = next;
        }
        (
            FindingKind::Unconsumed,
            None,
            "not reached from any part's PMI".into(),
        )
    }
}

enum Built {
    Feature(Feature, Vec<u64>),
    /// The shape aspect is the same feature as this other one (pre-4.0.6 datum feature form),
    /// with its own ids.
    Alias(u64, Vec<u64>),
    None,
}

/// Follow groups and derivations without mistaking an edge or supplemental geometry for a
/// datum feature's face. Keep the datum in the public result so the loss stays inspectable.
fn feature_has_face(pmi: &PartPmi, feature: FeatureId) -> bool {
    let mut pending = vec![feature];
    let mut seen = BTreeSet::new();
    while let Some(f) = pending.pop() {
        if !seen.insert(f) {
            continue;
        }
        match &pmi.features[f.0] {
            Feature::Items(items) => {
                if items.iter().any(|a| matches!(a, Anchor::Face(_))) {
                    return true;
                }
            }
            Feature::Group { members, .. } | Feature::Derived { from: members, .. } => {
                pending.extend(members);
            }
        }
    }
    false
}

#[derive(Clone, Copy, Debug)]
enum Want {
    Length,
    Angle,
    Any,
}

/// A count stated as a decimal (`count_measure` is a NUMBER) as the whole number it is: `60.`,
/// `6.E1`; `None` for a fraction, a negative or an out-of-range value, never truncated.
fn whole<T: TryFrom<u64>>(d: &Decimal) -> Option<T> {
    let x: f64 = d.as_str().parse().ok()?;
    if x.is_finite() && x >= 0.0 && x.fract() == 0.0 && x <= u64::MAX as f64 {
        T::try_from(x as u64).ok()
    } else {
        None
    }
}

/// Import repair only: the model's Bounds constructor remains strict. Deviations are
/// signed offsets (§5.2.3), so a nominal is checked only for absolute limits (§5.2.4).
fn dimension_bounds(
    upper: Value,
    lower: Value,
    nominal: Option<&Value>,
) -> Result<(Bounds, bool), ModelError> {
    let compare = |a: &Quantity, b: &Quantity| {
        a.cmp_same_unit(b)
            .unwrap_or_else(|| a.si().total_cmp(&b.si()))
    };
    let swapped = upper.quantity.same_kind(&lower.quantity)
        && compare(&upper.quantity, &lower.quantity).is_lt();
    if !swapped {
        return Bounds::new(upper, lower).map(|bounds| (bounds, false));
    }
    if let Some(nominal) = nominal
        && (!nominal.quantity.same_kind(&upper.quantity)
            || compare(&nominal.quantity, &upper.quantity).is_lt()
            || compare(&nominal.quantity, &lower.quantity).is_gt())
    {
        return Err(ModelError(format!(
            "nominal {} is outside the reversed limits {}..{}; limits not swapped",
            nominal.decimal(),
            upper.decimal(),
            lower.decimal()
        )));
    }
    Bounds::new(lower, upper).map(|bounds| (bounds, true))
}

fn dedup_anchors(mut a: Vec<Anchor>) -> Vec<Anchor> {
    let mut seen = BTreeSet::new();
    a.retain(|x| seen.insert(*x));
    a
}

fn si_prefix(p: &str) -> Option<i64> {
    Some(match p {
        "EXA" => 18,
        "PETA" => 15,
        "TERA" => 12,
        "GIGA" => 9,
        "MEGA" => 6,
        "KILO" => 3,
        "HECTO" => 2,
        "DECA" => 1,
        "DECI" => -1,
        "CENTI" => -2,
        "MILLI" => -3,
        "MICRO" => -6,
        "NANO" => -9,
        "PICO" => -12,
        "FEMTO" => -15,
        "ATTO" => -18,
        _ => return None,
    })
}

/// The `y` of a §5.4 format `NR2 x.y`, `NR2S x.y`, `NR2..x.y`, `NR2S..x.y`.
fn decimal_places(format: &str) -> Option<u8> {
    let rest = format.trim().strip_prefix("NR2")?;
    let rest = rest.strip_prefix('S').unwrap_or(rest);
    let rest = rest.strip_prefix("..").unwrap_or(rest).trim();
    let (x, y) = rest.split_once('.')?;
    x.trim().parse::<u8>().ok()?;
    y.trim().parse::<u8>().ok()
}

enum ParamValue {
    Text(String),
    Measure(Measured),
}

enum Item {
    Thread(Thread),
    Knurl(Knurl),
}

fn take_length(
    p: &mut BTreeMap<String, (u64, ParamValue)>,
    n: &str,
) -> Result<Option<Length>, String> {
    match p.remove(n) {
        None => Ok(None),
        Some((_, ParamValue::Measure(Measured::Length(l)))) => Ok(Some(l)),
        Some(_) => Err(format!("parameter {n:?} is not a length")),
    }
}

fn take_text(
    p: &mut BTreeMap<String, (u64, ParamValue)>,
    n: &str,
) -> Result<Option<String>, String> {
    match p.remove(n) {
        None => Ok(None),
        Some((_, ParamValue::Text(t))) => Ok(Some(t)),
        Some(_) => Err(format!("parameter {n:?} is not descriptive")),
    }
}

fn hand(t: &str) -> Result<Hand, String> {
    match t {
        "left" => Ok(Hand::Left),
        "right" => Ok(Hand::Right),
        o => Err(format!("hand {o:?} is neither 'left' nor 'right'")),
    }
}

fn thread_of(
    feature: FeatureId,
    partial_area: Option<FeatureId>,
    runout: Option<FeatureId>,
    p: &mut BTreeMap<String, (u64, ParamValue)>,
) -> Result<Thread, String> {
    let need = |o: Option<String>, n: &str| {
        o.ok_or_else(|| format!("thread without its {n:?} (thread WR)"))
    };
    let side = match need(take_text(p, "thread side")?, "thread side")?.as_str() {
        "internal" => ThreadSide::Internal,
        "external" => ThreadSide::External,
        o => return Err(format!("thread side {o:?} (WR10)")),
    };
    let major =
        take_length(p, "major diameter")?.ok_or("thread without its 'major diameter' (WR2)")?;
    let number = match p.remove("number of threads") {
        Some((_, ParamValue::Measure(Measured::Ratio(d)))) => Ratio(d),
        Some(_) => return Err("'number of threads' is not a ratio_measure_with_unit (WR5)".into()),
        None => return Err("thread without its 'number of threads' (WR5)".into()),
    };
    Ok(Thread {
        feature,
        partial_area,
        side,
        major_diameter: major,
        minor_diameter: take_length(p, "minor diameter")?,
        pitch_diameter: take_length(p, "pitch diameter")?,
        number_of_threads: number,
        form: need(take_text(p, "form")?, "form")?,
        fit_class: need(take_text(p, "fit class")?, "fit class")?,
        fit_class_2: take_text(p, "fit class 2")?,
        hand: hand(&need(take_text(p, "hand")?, "hand")?)?,
        crest: take_length(p, "crest")?,
        qualifier: take_text(p, "qualifier")?,
        nominal_size: take_length(p, "nominal size")?,
        runout,
    })
}

fn knurl_of(
    feature: FeatureId,
    pattern: &str,
    p: &mut BTreeMap<String, (u64, ParamValue)>,
) -> Result<Knurl, String> {
    let pattern = match pattern {
        "diamond" => KnurlPattern::Diamond,
        "diagonal" => KnurlPattern::Diagonal,
        "straight" => KnurlPattern::Straight,
        o => return Err(format!("knurl description {o:?} (turned_knurl WR1)")),
    };
    let need = |o: Option<Length>, n: &str| o.ok_or_else(|| format!("knurl without its {n:?}"));
    let number_of_teeth = match p.remove("number of teeth") {
        None => None,
        Some((_, ParamValue::Measure(Measured::Count(d)))) => {
            Some(Count(whole(&d).ok_or_else(|| {
                format!("'number of teeth' {} is not a whole count", d)
            })?))
        }
        Some(_) => return Err("'number of teeth' is not a count_measure (WR3)".into()),
    };
    let helix_angle = match p.remove("helix angle") {
        None => None,
        Some((_, ParamValue::Measure(Measured::Angle(a)))) => Some(a),
        Some(_) => return Err("'helix angle' is not a plane angle".into()),
    };
    let helix_hand = match take_text(p, "helix hand")? {
        None => None,
        Some(t) => Some(hand(&t)?),
    };
    let k = Knurl {
        feature,
        pattern,
        major_diameter: need(take_length(p, "major diameter")?, "major diameter")?,
        nominal_diameter: need(take_length(p, "nominal diameter")?, "nominal diameter")?,
        diametral_pitch: need(take_length(p, "diametral pitch")?, "diametral pitch")?,
        number_of_teeth,
        tooth_depth: take_length(p, "tooth depth")?,
        root_fillet: take_length(p, "root fillet")?,
        helix_angle,
        helix_hand,
    };
    match k.pattern {
        KnurlPattern::Diamond | KnurlPattern::Diagonal if k.helix_angle.is_none() => {
            Err("a diamond or diagonal knurl without its 'helix angle' (WR9)".into())
        }
        KnurlPattern::Diagonal if k.helix_hand.is_none() => {
            Err("a diagonal knurl without its 'helix hand' (WR10)".into())
        }
        _ => Ok(k),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_tokens_skip_strings_names_and_refs() {
        let t = b"#12=(LENGTH_MEASURE_WITH_UNIT()MEASURE_WITH_UNIT(LENGTH_MEASURE(0.0030),#4)REPRESENTATION_ITEM('a 1.5 ''x'' 2.')AXIS2_PLACEMENT_3D('',#1,.T.,-2.54E1,3,1.e-3));";
        assert_eq!(real_tokens(t), ["0.0030", "-2.54E1", "1.e-3"]);
    }

    #[test]
    fn decimal_places_of_formats() {
        assert_eq!(decimal_places("NR2 1.3"), Some(3));
        assert_eq!(decimal_places("NR2S 0.3"), Some(3));
        assert_eq!(decimal_places("NR2..3.2"), Some(2));
        assert_eq!(decimal_places("NR2S..3.3"), Some(3));
        assert_eq!(decimal_places("NR1 3"), None);
    }
}
