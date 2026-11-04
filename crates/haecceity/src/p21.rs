//! A lossless Part 21 (ISO 10303-21) document: the file's own bytes, every DATA instance's id,
//! parsed record and exact byte range, and edits (add, replace, remove instances; change
//! `FILE_SCHEMA` and append `FILE_DESCRIPTION` entries) written so untouched bytes are copied unchanged. With no edit, the
//! output is the input. It knows nothing about any schema or about PMI.
//!
//! Records are step-io's [`RawEntity`]. [`Attribute::String`] contains decoded Unicode
//! text; [`encode`] escapes backslashes and non-ASCII characters and doubles quotes.
//! [`decode`] and [`escape`] operate on encoded Part 21 string bodies, not attributes.
//!
//! Instance extents come from this module's scanner over the original bytes, from each
//! `#N` to its terminating `;` outside strings, binaries and comments. The ids must be
//! exactly step-io's, or the file is refused.
//! The scanner knows every section (HEADER, the edition 3 ANCHOR and REFERENCE sections, one or
//! more DATA sections), but step-io 0.3 reads a single DATA section, so a file with several is
//! refused with step-io's parse error; additions go before the last DATA section's `ENDSEC`.
//!
//! A complex instance's leaves are written in the external mapping's order, sorted by entity
//! name (ISO 10303-21 §12.2.5.2). [`Complex`] holds them sorted, so it cannot build an unsorted
//! one, and [`encode`] refuses a raw complex record whose parts are not in that order rather
//! than reorder them silently.
//!
//! Instances an [`Edit`] adds are referenced, before they have ids, through [`NewId::to_ref`]: a
//! provisional reference in the upper half of the id space (`#2^63 + n`), which [`Document::apply`]
//! replaces with the assigned `#N`. A file using ids that large is refused.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;

pub use step_io::parser::{Attribute, RawEntity, RawEntityPart};
use step_io::parser::{Graph, Span};

/// Preserve the duplicate-id diagnostic's source column: step-io 0.3 reports only
/// its line and the first definition's line. Recover the declaration from that line.
pub(super) fn parse_message(bytes: &[u8], error: &step_io::Error) -> String {
    if let step_io::Error::DuplicateEntityId { id, line, .. } = error {
        let column = scan(bytes)
            .ok()
            .and_then(|s| s.instances.into_iter().filter(|(n, _)| n == id).nth(1))
            .map(|(_, span)| {
                let line_start = bytes[..span.start]
                    .iter()
                    .rposition(|&b| b == b'\n')
                    .map_or(0, |i| i + 1);
                span.start - line_start + 1
            })
            .unwrap_or(1);
        return format!("parse error at line {line}, column {column}: duplicate entity #{id}");
    }
    error.to_string()
}

/// Why a document could not be read, an edit could not be applied, or a record or string could
/// not be written.
#[derive(Debug, Clone, PartialEq)]
pub enum P21Error {
    /// step-io or the extent scanner could not read the file.
    Parse(String),
    /// The extent scanner and step-io disagree about the file's instances.
    Mismatch(String),
    /// A record, string or real cannot be written as valid Part 21.
    Encode(String),
    /// A string body holds an escape that is malformed or not supported.
    Decode(String),
    /// A replacement or removal names an instance the file does not have.
    Missing(u64),
    /// An edit replaces or removes the same instance twice (or both), or sets `FILE_SCHEMA`
    /// twice (`None`).
    EditedTwice(Option<u64>),
    /// A written record references an instance that neither exists nor is added (`to` is `#N`
    /// or a provisional `new #n`); `from` is the referencing instance's id in the output.
    Dangling { from: u64, to: String },
    /// A removed instance is still referenced by instances that stay (kept, replaced or added),
    /// listed by their ids in the output.
    StillReferenced { id: u64, by: Vec<u64> },
    /// A removed instance is named by the file's ANCHOR section.
    Anchored { id: u64, name: String },
}

impl std::fmt::Display for P21Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            P21Error::Parse(s) => write!(f, "Part 21 parse error: {s}"),
            P21Error::Mismatch(s) => write!(f, "Part 21 instances disagree with step-io: {s}"),
            P21Error::Encode(s) => write!(f, "cannot write Part 21: {s}"),
            P21Error::Decode(s) => write!(f, "cannot decode Part 21 string: {s}"),
            P21Error::Missing(id) => write!(f, "no instance #{id} to edit"),
            P21Error::EditedTwice(Some(id)) => write!(f, "instance #{id} is edited twice"),
            P21Error::EditedTwice(None) => write!(f, "FILE_SCHEMA is set twice"),
            P21Error::Dangling { from, to } => {
                write!(
                    f,
                    "#{from} references {to}, which neither exists nor is added"
                )
            }
            P21Error::StillReferenced { id, by } => {
                let by: Vec<String> = by.iter().map(|b| format!("#{b}")).collect();
                write!(
                    f,
                    "#{id} is removed but still referenced by {}",
                    by.join(", ")
                )
            }
            P21Error::Anchored { id, name } => {
                write!(f, "#{id} is removed but named by anchor {name}")
            }
        }
    }
}

impl std::error::Error for P21Error {}

/// Provisional references live at and above this id; files may not use them.
const PROVISIONAL: u64 = 1 << 63;

/// An instance an [`Edit`] adds, before [`Document::apply`] numbers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NewId(u64);

impl NewId {
    /// The provisional reference to this instance, for use in records of the same edit.
    #[must_use]
    pub fn to_ref(self) -> Attribute {
        Attribute::EntityRef(PROVISIONAL | self.0)
    }

    /// Position among the edit's additions (0 for the first added).
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A record to write: step-io's [`RawEntity`] (its `id` and `span` are ignored; the edit gives
/// the id) and, optionally, the decimal text to write for some of its REAL values in place of
/// the shortest round-trip form, so that a value stated as `0.0030` is written `0.0030`.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub entity: RawEntity,
    stated: BTreeMap<u64, String>,
    real_texts: Vec<String>,
}

impl Record {
    #[must_use]
    pub fn new(entity: RawEntity) -> Self {
        Record {
            entity,
            stated: BTreeMap::new(),
            real_texts: Vec::new(),
        }
    }

    /// Writes every REAL of the record whose value is exactly `decimal`'s with `decimal`'s own
    /// text. Refused when `decimal` is not a Part 21 REAL, when no REAL of the record has its
    /// value, or when another text is already stated for that value.
    pub fn state(mut self, decimal: &str) -> Result<Self, P21Error> {
        let value = parse_real_text(decimal)?;
        let bits = value.to_bits();
        let mut found = false;
        visit_attributes(&self.entity, &mut |a| {
            if matches!(a, Attribute::Real(v) if v.to_bits() == bits) {
                found = true;
            }
        });
        if !found {
            return Err(P21Error::Encode(format!(
                "stated real {decimal} is not a value of the record"
            )));
        }
        match self.stated.get(&bits) {
            Some(t) if t != decimal => Err(P21Error::Encode(format!(
                "real {value:e} stated both as {t} and as {decimal}"
            ))),
            _ => {
                self.stated.insert(bits, decimal.to_string());
                Ok(self)
            }
        }
    }

    /// State every REAL occurrence in depth-first attribute order. Unlike [`Self::state`],
    /// equal numeric values may have different text (for example `0.0` and `0.E0`).
    /// Each token must be a finite Part 21 REAL matching that occurrence exactly.
    pub fn state_reals(mut self, decimals: &[&str]) -> Result<Self, P21Error> {
        let mut values = Vec::new();
        visit_attributes(&self.entity, &mut |a| {
            if let Attribute::Real(v) = a {
                values.push(*v);
            }
        });
        if values.len() != decimals.len() {
            return Err(P21Error::Encode(
                "stated REAL occurrence count differs from record".into(),
            ));
        }
        for (value, text) in values.iter().zip(decimals) {
            if value.to_bits() != parse_real_text(text)?.to_bits() {
                return Err(P21Error::Encode(format!(
                    "stated REAL {text} differs from record occurrence"
                )));
            }
        }
        self.real_texts = decimals.iter().map(|s| (*s).to_string()).collect();
        Ok(self)
    }

    /// The record's Part 21 text (without `#N=` and `;`).
    pub fn encode(&self) -> Result<String, P21Error> {
        let mut out = String::new();
        encode_entity(&self.entity, &self.stated, &self.real_texts, &mut out)?;
        Ok(out)
    }
}

impl From<RawEntity> for Record {
    fn from(entity: RawEntity) -> Self {
        Record::new(entity)
    }
}

impl From<Complex> for Record {
    fn from(c: Complex) -> Self {
        Record::new(c.into_record())
    }
}

const NO_SPAN: Span = Span {
    start: 0,
    end: 0,
    line: 0,
    column: 0,
};

/// A simple record `NAME(attributes)` to add or write (id 0, no span).
#[must_use]
pub fn simple(name: &str, attributes: Vec<Attribute>) -> RawEntity {
    RawEntity::Simple {
        id: 0,
        name: name.to_string(),
        attributes,
        span: NO_SPAN,
    }
}

/// A complex record's leaves, held sorted by entity name: the only way this crate builds a
/// complex instance.
#[derive(Debug, Clone, PartialEq)]
pub struct Complex {
    leaves: Vec<RawEntityPart>,
}

impl Complex {
    /// The leaves in any order; refused when empty or when two leaves share a name.
    pub fn new(leaves: impl IntoIterator<Item = RawEntityPart>) -> Result<Self, P21Error> {
        let mut leaves: Vec<RawEntityPart> = leaves.into_iter().collect();
        if leaves.is_empty() {
            return Err(P21Error::Encode("a complex instance without leaves".into()));
        }
        leaves.sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
        if let Some(w) = leaves.windows(2).find(|w| w[0].name == w[1].name) {
            return Err(P21Error::Encode(format!(
                "a complex instance with two {} leaves",
                w[0].name
            )));
        }
        Ok(Complex { leaves })
    }

    /// The leaves, sorted by name.
    #[must_use]
    pub fn leaves(&self) -> &[RawEntityPart] {
        &self.leaves
    }

    /// The record (id 0, no span).
    #[must_use]
    pub fn into_record(self) -> RawEntity {
        RawEntity::Complex {
            id: 0,
            parts: self.leaves,
            span: NO_SPAN,
        }
    }
}

/// One leaf of a complex record.
#[must_use]
pub fn leaf(name: &str, attributes: Vec<Attribute>) -> RawEntityPart {
    RawEntityPart {
        name: name.to_string(),
        attributes,
    }
}

/// Changes to a [`Document`]: instances added (numbered on apply), replaced (same id, new
/// record) and removed, and a new `FILE_SCHEMA`.
#[derive(Debug, Clone, Default)]
pub struct Edit {
    adds: Vec<Record>,
    ops: Vec<Op>,
}

#[derive(Debug, Clone)]
enum Op {
    Replace(u64, Record),
    Rename(u64, String),
    Remove(u64),
    FileSchema(Vec<String>),
    FileDescription(String, Vec<String>),
}

impl Edit {
    #[must_use]
    pub fn new() -> Self {
        Edit::default()
    }

    /// Adds an instance; its records and others of the edit reference it by the returned id's
    /// [`NewId::to_ref`].
    pub fn add(&mut self, record: impl Into<Record>) -> NewId {
        self.adds.push(record.into());
        NewId(self.adds.len() as u64 - 1)
    }

    /// Replaces instance `#id`'s record, keeping its id and its place in the file.
    pub fn replace(&mut self, id: u64, record: impl Into<Record>) {
        self.ops.push(Op::Replace(id, record.into()));
    }

    /// Changes only a simple instance's entity keyword, preserving every other byte.
    /// Reserved for the PMI writer's closed edition rewrite table.
    pub(crate) fn rename_entity(&mut self, id: u64, name: &str) {
        self.ops.push(Op::Rename(id, name.to_string()));
    }

    /// Removes instance `#id`.
    pub fn remove(&mut self, id: u64) {
        self.ops.push(Op::Remove(id));
    }

    /// Sets the header's `FILE_SCHEMA` list (strings in step-io's convention, as
    /// [`Document::file_schema`] returns them).
    pub fn set_file_schema(&mut self, schemas: Vec<String>) {
        self.ops.push(Op::FileSchema(schemas));
    }

    /// Best-effort reference update: replace matching older references, or append
    /// when absent. Missing/malformed descriptions and unchanged bytes are retained.
    pub(crate) fn set_file_description_reference(&mut self, description: &str, prefixes: &[&str]) {
        self.ops.push(Op::FileDescription(
            description.into(),
            prefixes.iter().map(|s| (*s).into()).collect(),
        ));
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.adds.is_empty() && self.ops.is_empty()
    }
}

/// An applied edit: the new file and the id each added instance was given.
#[derive(Debug, Clone, PartialEq)]
pub struct Written {
    pub bytes: Vec<u8>,
    pub ids: BTreeMap<NewId, u64>,
}

/// A Part 21 file kept byte for byte, with every DATA instance's record and extent.
#[derive(Debug)]
pub struct Document {
    bytes: Vec<u8>,
    graph: Graph,
    /// `#N` → its text from `#` through `;`.
    spans: HashMap<u64, Range<usize>>,
    /// Header entities' extents (keyword through `;`), in file order, as step-io lists them.
    header_spans: Vec<Range<usize>>,
    /// Each DATA section's `DATA` keyword start and its `ENDSEC` keyword start.
    data_sections: Vec<(usize, usize)>,
    referrers: HashMap<u64, Vec<u64>>,
    max_id: u64,
    newline: &'static str,
}

impl Document {
    /// Reads a file: step-io's records, this module's extents, and the check that both see the
    /// same instances.
    pub fn parse(bytes: Vec<u8>) -> Result<Document, P21Error> {
        let graph = step_io::parser::parse(bytes.as_slice())
            .map_err(|e| P21Error::Parse(parse_message(&bytes, &e)))?;
        let scan = scan(&bytes)?;

        if scan.header.len() != graph.header.len() {
            return Err(P21Error::Mismatch(format!(
                "{} header entities scanned, step-io has {}",
                scan.header.len(),
                graph.header.len()
            )));
        }
        for ((name, _), h) in scan.header.iter().zip(&graph.header) {
            if let RawEntity::Simple { name: n, .. } = h
                && n != name
            {
                return Err(P21Error::Mismatch(format!(
                    "header entity {name} scanned where step-io has {n}"
                )));
            }
        }
        let mut spans = HashMap::with_capacity(scan.instances.len());
        for (id, span) in &scan.instances {
            if spans.insert(*id, span.clone()).is_some() {
                return Err(P21Error::Mismatch(format!("#{id} scanned twice")));
            }
        }
        if spans.len() != graph.entities.len() {
            let extra = spans.keys().find(|id| !graph.entities.contains_key(id));
            let missing = graph.entities.keys().find(|id| !spans.contains_key(id));
            return Err(P21Error::Mismatch(format!(
                "{} instances scanned, step-io has {} (first scanned only: {extra:?}, first \
                 step-io only: {missing:?})",
                spans.len(),
                graph.entities.len()
            )));
        }
        if let Some(id) = graph.entities.keys().find(|id| !spans.contains_key(id)) {
            return Err(P21Error::Mismatch(format!("step-io's #{id} not scanned")));
        }

        let max_id = graph
            .entities
            .keys()
            .chain(graph.external_references.keys())
            .copied()
            .max()
            .unwrap_or(0);
        if max_id >= PROVISIONAL {
            return Err(P21Error::Parse(format!(
                "instance id #{max_id} is in the range reserved for provisional ids"
            )));
        }

        let mut referrers: HashMap<u64, Vec<u64>> = HashMap::new();
        let mut targets = Vec::new();
        for (&id, e) in &graph.entities {
            targets.clear();
            visit_attributes(e, &mut |a| {
                if let Attribute::EntityRef(t) = a {
                    targets.push(*t);
                }
            });
            targets.sort_unstable();
            targets.dedup();
            for &t in &targets {
                referrers.entry(t).or_default().push(id);
            }
        }

        let last_endsec = scan.data_sections.last().map(|s| s.1);
        let newline = line_ending(&bytes, last_endsec.unwrap_or(bytes.len()));
        Ok(Document {
            graph,
            spans,
            header_spans: scan.header.into_iter().map(|(_, r)| r).collect(),
            data_sections: scan.data_sections,
            referrers,
            max_id,
            newline,
            bytes,
        })
    }

    /// The file as read.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// step-io's graph of the file (header, instances, edition 3 references and anchors).
    #[must_use]
    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    /// Instance `#id`'s record.
    #[must_use]
    pub fn get(&self, id: u64) -> Option<&RawEntity> {
        self.graph.entities.get(&id)
    }

    /// Instance `#id`'s text, from `#` through its `;`.
    #[must_use]
    pub fn span(&self, id: u64) -> Option<Range<usize>> {
        self.spans.get(&id).cloned()
    }

    /// Every DATA instance's id, ascending.
    pub fn ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.graph.entities.keys().copied()
    }

    /// The largest id in use (DATA instances and edition 3 references), 0 for none.
    #[must_use]
    pub fn max_id(&self) -> u64 {
        self.max_id
    }

    /// The header entities (`FILE_DESCRIPTION`, `FILE_NAME`, `FILE_SCHEMA`, …), in file order.
    #[must_use]
    pub fn header(&self) -> &[RawEntity] {
        &self.graph.header
    }

    /// The header's `FILE_SCHEMA` strings, in step-io's string convention. Refused when the
    /// header has no `FILE_SCHEMA`, or one that is not a single list of strings.
    pub fn file_schema(&self) -> Result<Vec<String>, P21Error> {
        let bad = |why: &str| Err(P21Error::Parse(format!("FILE_SCHEMA {why}")));
        let mut schemas = self.graph.header.iter().filter_map(|h| match h {
            RawEntity::Simple {
                name, attributes, ..
            } if name == "FILE_SCHEMA" => Some(attributes),
            _ => None,
        });
        let Some(attributes) = schemas.next() else {
            return bad("is missing");
        };
        if schemas.next().is_some() {
            return bad("appears more than once");
        }
        let [Attribute::List(items)] = attributes.as_slice() else {
            return bad("is not a single list");
        };
        items
            .iter()
            .map(|a| match a {
                Attribute::String(s) => Ok(s.clone()),
                other => Err(P21Error::Parse(format!(
                    "FILE_SCHEMA holds {other:?}, not a string"
                ))),
            })
            .collect()
    }

    /// The ids of the instances whose records reference `#id`, ascending.
    #[must_use]
    pub fn referrers(&self, id: u64) -> &[u64] {
        self.referrers.get(&id).map_or(&[], Vec::as_slice)
    }

    /// The line ending additions are written with: the file's own, or where it mixes `\r\n` and
    /// `\n`, the one before the last DATA section's `ENDSEC`.
    #[must_use]
    pub fn newline(&self) -> &'static str {
        self.newline
    }

    /// The edited file. Untouched bytes are copied exactly; a replaced instance is written in
    /// its own extent with its own id; a removed instance's extent is dropped, with its line
    /// when it is alone on its line(s); additions go one per line before the `ENDSEC` of the
    /// last DATA section, numbered from [`Self::max_id`] + 1 in the order added. Refused, with
    /// nothing written, when a reference would dangle or a removed instance is still referenced.
    pub fn apply(&self, edit: &Edit) -> Result<Written, P21Error> {
        let mut replaced: BTreeMap<u64, &Record> = BTreeMap::new();
        let mut renamed = BTreeMap::new();
        let mut removed: BTreeSet<u64> = BTreeSet::new();
        let mut schema: Option<&Vec<String>> = None;
        let mut descriptions = Vec::new();
        for op in &edit.ops {
            match op {
                Op::Replace(id, _) | Op::Rename(id, _) | Op::Remove(id) => {
                    if !self.graph.entities.contains_key(id) {
                        return Err(P21Error::Missing(*id));
                    }
                    if replaced.contains_key(id) || renamed.contains_key(id) || removed.contains(id)
                    {
                        return Err(P21Error::EditedTwice(Some(*id)));
                    }
                    match op {
                        Op::Replace(_, r) => {
                            replaced.insert(*id, r);
                        }
                        Op::Rename(_, name) => {
                            if !matches!(self.get(*id), Some(RawEntity::Simple { .. }))
                                || name.is_empty()
                                || !name.as_bytes()[0].is_ascii_alphabetic()
                                || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
                            {
                                return Err(P21Error::Encode(format!(
                                    "invalid entity rename #{id}"
                                )));
                            }
                            renamed.insert(*id, name);
                        }
                        _ => {
                            removed.insert(*id);
                        }
                    }
                }
                Op::FileDescription(s, prefixes) => {
                    descriptions.push((s.as_str(), prefixes.as_slice()))
                }
                Op::FileSchema(s) => {
                    if schema.replace(s).is_some() {
                        return Err(P21Error::EditedTwice(None));
                    }
                }
            }
        }

        let first_new = self.max_id + 1;
        let ids: BTreeMap<NewId, u64> = (0..edit.adds.len() as u64)
            .map(|i| (NewId(i), first_new + i))
            .collect();
        let resolve = |from: u64, record: &Record| -> Result<Record, P21Error> {
            let mut out = record.clone();
            let mut err = None;
            visit_attributes_mut(&mut out.entity, &mut |a| {
                let Attribute::EntityRef(t) = a else { return };
                let provisional = *t >= PROVISIONAL;
                let target = if provisional {
                    match ids.get(&NewId(*t - PROVISIONAL)) {
                        Some(&n) => n,
                        None => {
                            err.get_or_insert(P21Error::Dangling {
                                from,
                                to: format!("new #{}", *t - PROVISIONAL),
                            });
                            return;
                        }
                    }
                } else {
                    *t
                };
                *a = Attribute::EntityRef(target);
                if removed.contains(&target) {
                    err.get_or_insert(P21Error::StillReferenced {
                        id: target,
                        by: vec![from],
                    });
                } else if !provisional
                    && !self.graph.entities.contains_key(&target)
                    && !self.graph.external_references.contains_key(&target)
                {
                    // A literal number for an instance this edit adds is refused too: only
                    // provisional references (`NewId::to_ref`) name additions.
                    err.get_or_insert(P21Error::Dangling {
                        from,
                        to: format!("#{target}"),
                    });
                }
            });
            match err {
                Some(e) => Err(e),
                None => Ok(out),
            }
        };

        // The records written, references resolved; then each removed instance's referrers
        // among the instances that stay unchanged (a replaced or added one was checked above).
        let mut written: Vec<(u64, Record)> = Vec::new();
        for (&id, r) in &replaced {
            written.push((id, resolve(id, r)?));
        }
        for (i, r) in edit.adds.iter().enumerate() {
            let id = first_new + i as u64;
            written.push((id, resolve(id, r)?));
        }
        for &id in &removed {
            let by: Vec<u64> = self
                .referrers(id)
                .iter()
                .copied()
                .filter(|r| !removed.contains(r) && !replaced.contains_key(r))
                .collect();
            if !by.is_empty() {
                return Err(P21Error::StillReferenced { id, by });
            }
            if let Some((name, _)) = self.graph.anchors.iter().find(|(_, a)| *a == id) {
                return Err(P21Error::Anchored {
                    id,
                    name: name.clone(),
                });
            }
        }
        let mut texts: BTreeMap<u64, String> = BTreeMap::new();
        for (id, r) in &written {
            texts.insert(*id, r.encode()?);
        }

        // Patches over the original bytes: (range, text), applied in order.
        let mut patches: Vec<(Range<usize>, String)> = Vec::new();
        for (&id, name) in &renamed {
            let span = &self.spans[&id];
            let b = &self.bytes;
            let mut pos = span.start + 1; // #, then the instance number
            while b[pos].is_ascii_digit() {
                pos += 1;
            }
            pos = skip_blank(b, pos).ok_or_else(|| P21Error::Encode("rename comment".into()))?;
            if b[pos] != b'=' {
                return Err(P21Error::Encode(format!("rename #{id}: expected '='")));
            }
            let start =
                skip_blank(b, pos + 1).ok_or_else(|| P21Error::Encode("rename comment".into()))?;
            let mut end = start;
            while b[end].is_ascii_alphanumeric() || b[end] == b'_' {
                end += 1;
            }
            patches.push((start..end, (*name).clone()));
        }
        for &id in replaced.keys() {
            patches.push((self.spans[&id].clone(), format!("#{id}={};", texts[&id])));
        }
        // Removed instances separated only by spaces and tabs are removed as one run, so a line
        // they share goes with them.
        let mut runs: Vec<Range<usize>> = Vec::new();
        let mut spans: Vec<Range<usize>> =
            removed.iter().map(|id| self.spans[id].clone()).collect();
        spans.sort_by_key(|r| r.start);
        for span in spans {
            match runs.last_mut() {
                Some(run)
                    if self.bytes[run.end..span.start]
                        .iter()
                        .all(|c| matches!(c, b' ' | b'\t')) =>
                {
                    run.end = span.end;
                }
                _ => runs.push(span),
            }
        }
        for run in &runs {
            patches.push((self.removal_extent(run), String::new()));
        }
        if let Some(schemas) = schema {
            let at = self
                .graph
                .header
                .iter()
                .position(|h| matches!(h, RawEntity::Simple { name, .. } if name == "FILE_SCHEMA"))
                .ok_or_else(|| P21Error::Parse("no FILE_SCHEMA".into()))?;
            let list = Attribute::List(schemas.iter().cloned().map(Attribute::String).collect());
            let text = format!("FILE_SCHEMA({});", encode_attribute_text(&list)?);
            patches.push((self.header_spans[at].clone(), text));
        }
        let description_at = self.graph.header.iter().position(
            |h| matches!(h, RawEntity::Simple { name, .. } if name == "FILE_DESCRIPTION"),
        );
        let updated = file_description_attributes(
            description_at.map(|at| &self.graph.header[at]),
            &descriptions,
        );
        if let (Some(at), Some(attributes)) = (description_at, updated) {
            let args = attributes
                .iter()
                .map(encode_attribute_text)
                .collect::<Result<Vec<_>, _>>()?;
            patches.push((
                self.header_spans[at].clone(),
                format!("FILE_DESCRIPTION({});", args.join(",")),
            ));
        }
        if !edit.adds.is_empty() {
            let Some(&(_, endsec)) = self.data_sections.last() else {
                return Err(P21Error::Parse("no DATA section to add to".into()));
            };
            let nl = self.newline;
            let line_start = line_start(&self.bytes, endsec);
            let (at, mut text) = match line_start {
                Some(s) => (s, String::new()),
                None => (endsec, nl.to_string()),
            };
            for id in first_new..first_new + edit.adds.len() as u64 {
                text.push_str(&format!("#{id}={};{nl}", texts[&id]));
            }
            patches.push((at..at, text));
        }
        patches.sort_by_key(|(r, _)| (r.start, r.end));

        let mut bytes = Vec::with_capacity(self.bytes.len() + 128 * edit.adds.len());
        let mut at = 0;
        for (range, text) in &patches {
            debug_assert!(range.start >= at, "overlapping patches");
            bytes.extend_from_slice(&self.bytes[at..range.start]);
            bytes.extend_from_slice(text.as_bytes());
            at = range.end;
        }
        bytes.extend_from_slice(&self.bytes[at..]);
        Ok(Written { bytes, ids })
    }

    /// What removing the instance(s) at `span` drops: its whole line(s) with the line break that
    /// follows when nothing but spaces and tabs shares them, else the span alone.
    fn removal_extent(&self, span: &Range<usize>) -> Range<usize> {
        let b = &self.bytes;
        let Some(start) = line_start(b, span.start) else {
            return span.clone();
        };
        let mut end = span.end;
        while end < b.len() && matches!(b[end], b' ' | b'\t') {
            end += 1;
        }
        if b[end..].starts_with(b"\r\n") {
            end += 2;
        } else if b[end..].starts_with(b"\n") {
            end += 1;
        } else if end < b.len() {
            return span.clone();
        }
        start..end
    }
}

/// The start of `at`'s line when only spaces and tabs precede `at` on it.
fn line_start(b: &[u8], at: usize) -> Option<usize> {
    let mut s = at;
    while s > 0 && matches!(b[s - 1], b' ' | b'\t') {
        s -= 1;
    }
    (s == 0 || b[s - 1] == b'\n').then_some(s)
}

/// The file's line ending: `\r\n` or `\n` if it uses only one, else the one before `before`.
fn line_ending(b: &[u8], before: usize) -> &'static str {
    let (mut crlf, mut lf) = (0usize, 0usize);
    for (i, &c) in b.iter().enumerate() {
        if c == b'\n' {
            if i > 0 && b[i - 1] == b'\r' {
                crlf += 1;
            } else {
                lf += 1;
            }
        }
    }
    match (crlf, lf) {
        (0, _) => "\n",
        (_, 0) => "\r\n",
        _ => match b[..before].iter().rposition(|&c| c == b'\n') {
            Some(i) if i > 0 && b[i - 1] == b'\r' => "\r\n",
            _ => "\n",
        },
    }
}

// ---------------------------------------------------------------------------------------------
// Extent scanning
// ---------------------------------------------------------------------------------------------

struct Scan {
    header: Vec<(String, Range<usize>)>,
    instances: Vec<(u64, Range<usize>)>,
    data_sections: Vec<(usize, usize)>,
}

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Start,
    Top,
    Header,
    Data,
    Ed3,
    End,
}

/// Splits the file into statements (each ending at a `;` outside strings, binaries and
/// comments) and records the header entities, the DATA sections and each instance's extent.
fn scan(b: &[u8]) -> Result<Scan, P21Error> {
    let err = |at: usize, what: &str| {
        let line = b[..at.min(b.len())].iter().filter(|&&c| c == b'\n').count() + 1;
        P21Error::Parse(format!("line {line}: {what}"))
    };
    let mut out = Scan {
        header: Vec::new(),
        instances: Vec::new(),
        data_sections: Vec::new(),
    };
    let mut section = Section::Start;
    let mut data_start = 0;
    let mut pos = 0;
    loop {
        pos = skip_blank(b, pos).ok_or_else(|| err(pos, "unterminated comment"))?;
        if pos >= b.len() {
            break;
        }
        let start = pos;
        if section == Section::End {
            return Err(err(pos, "text after END-ISO-10303-21"));
        }
        let first = if b[pos] == b'#' {
            pos += 1;
            let digits = pos;
            while pos < b.len() && b[pos].is_ascii_digit() {
                pos += 1;
            }
            let id = std::str::from_utf8(&b[digits..pos])
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or_else(|| err(start, "malformed instance id"))?;
            Statement::Instance(id)
        } else if b[pos] == b'<' && section == Section::Ed3 {
            // An ANCHOR entry, `<name> = #N;` (or another value): checked by step-io.
            Statement::Anchor
        } else {
            while pos < b.len() && (b[pos].is_ascii_alphanumeric() || b"_-".contains(&b[pos])) {
                pos += 1;
            }
            if pos == start {
                return Err(err(start, "expected a keyword or #N"));
            }
            Statement::Word(String::from_utf8_lossy(&b[start..pos]).to_ascii_uppercase())
        };
        let end = statement_end(b, pos, section == Section::Ed3)
            .ok_or_else(|| err(start, "statement without its terminating ';'"))?;
        let word = match &first {
            Statement::Word(w) => w.as_str(),
            Statement::Instance(_) | Statement::Anchor => "",
        };
        match (section, &first) {
            (Section::Start, _) if word == "ISO-10303-21" => section = Section::Top,
            (Section::Start, _) => return Err(err(start, "expected ISO-10303-21;")),
            (Section::Top, _) => match word {
                "HEADER" => section = Section::Header,
                "DATA" => {
                    section = Section::Data;
                    data_start = start;
                }
                "ANCHOR" | "REFERENCE" => section = Section::Ed3,
                "END-ISO-10303-21" => section = Section::End,
                "SIGNATURE" => return Err(err(start, "SIGNATURE sections are not supported")),
                _ => return Err(err(start, "expected a section")),
            },
            (Section::Header | Section::Data | Section::Ed3, _) if word == "ENDSEC" => {
                if section == Section::Data {
                    out.data_sections.push((data_start, start));
                }
                section = Section::Top;
            }
            (Section::Header, Statement::Word(w)) => out.header.push((w.clone(), start..end)),
            (Section::Data, Statement::Instance(id)) => out.instances.push((*id, start..end)),
            (Section::Ed3, _) => {}
            _ => return Err(err(start, "unexpected statement")),
        }
        pos = end;
    }
    if section != Section::End {
        return Err(err(b.len(), "missing END-ISO-10303-21;"));
    }
    Ok(out)
}

enum Statement {
    Instance(u64),
    Anchor,
    Word(String),
}

/// Past whitespace and comments from `pos`; `None` for an unterminated comment.
fn skip_blank(b: &[u8], mut pos: usize) -> Option<usize> {
    loop {
        while pos < b.len() && b[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if b[pos..].starts_with(b"/*") {
            pos = comment_end(b, pos + 2)?;
        } else {
            return Some(pos);
        }
    }
}

fn comment_end(b: &[u8], from: usize) -> Option<usize> {
    b[from..]
        .windows(2)
        .position(|w| w == b"*/")
        .map(|i| from + i + 2)
}

/// The position just past the `;` ending the statement that continues at `pos`.
fn statement_end(b: &[u8], mut pos: usize, anchors: bool) -> Option<usize> {
    while pos < b.len() {
        match b[pos] {
            b';' => return Some(pos + 1),
            b'\'' => {
                pos += 1;
                loop {
                    let q = pos + b[pos..].iter().position(|&c| c == b'\'')?;
                    if b.get(q + 1) == Some(&b'\'') {
                        pos = q + 2;
                    } else {
                        pos = q + 1;
                        break;
                    }
                }
            }
            b'"' => pos += 1 + b[pos + 1..].iter().position(|&c| c == b'"')? + 1,
            b'<' if anchors => pos += 1 + b[pos + 1..].iter().position(|&c| c == b'>')? + 1,
            b'/' if b.get(pos + 1) == Some(&b'*') => pos = comment_end(b, pos + 2)?,
            _ => pos += 1,
        }
    }
    None
}

// The reference is advisory; absent or malformed headers never prevent a DATA
// edit. Returning None also preserves every original header byte on a no-op.
fn file_description_attributes(
    header: Option<&RawEntity>,
    descriptions: &[(&str, &[String])],
) -> Option<Vec<Attribute>> {
    let RawEntity::Simple { attributes, .. } = header? else {
        return None;
    };
    let Attribute::List(entries) = attributes.first()? else {
        return None;
    };
    let mut updated = entries.clone();
    for (description, prefixes) in descriptions {
        let mut seen = false;
        updated = updated.into_iter().filter_map(|entry| {
            if matches!(&entry, Attribute::String(s) if s == description || prefixes.iter().any(|p| s.starts_with(p))) {
                if seen { return None; }
                seen = true;
                Some(Attribute::String((*description).into()))
            } else { Some(entry) }
        }).collect();
        if !seen {
            updated.push(Attribute::String((*description).into()));
        }
    }
    if &updated == entries {
        return None;
    }
    let mut attributes = attributes.clone();
    attributes[0] = Attribute::List(updated);
    Some(attributes)
}

// ---------------------------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------------------------

/// The Part 21 text of a simple (`NAME(…)`) or complex (`(A(…)B(…))`) record, without `#N=` and
/// `;`. Refused for a complex record whose parts are not sorted by name, names that are not
/// Part 21 keywords, non-finite reals, and strings that are not valid bodies (see [`escape`]).
pub fn encode(record: &RawEntity) -> Result<String, P21Error> {
    let mut out = String::new();
    encode_entity(record, &BTreeMap::new(), &[], &mut out)?;
    Ok(out)
}

fn encode_entity(
    e: &RawEntity,
    stated: &BTreeMap<u64, String>,
    occurrences: &[String],
    out: &mut String,
) -> Result<(), P21Error> {
    let mut text = RealText {
        stated,
        occurrences: occurrences.iter(),
    };
    match e {
        RawEntity::Simple {
            name, attributes, ..
        } => encode_part(name, attributes, &mut text, out),
        RawEntity::Complex { parts, .. } => {
            if parts.is_empty() {
                return Err(P21Error::Encode("a complex instance without leaves".into()));
            }
            if let Some(w) = parts
                .windows(2)
                .find(|w| w[0].name.as_bytes() >= w[1].name.as_bytes())
            {
                return Err(P21Error::Encode(format!(
                    "complex instance leaves not in the external mapping's order: {} before {}",
                    w[0].name, w[1].name
                )));
            }
            out.push('(');
            for p in parts {
                encode_part(&p.name, &p.attributes, &mut text, out)?;
            }
            out.push(')');
            Ok(())
        }
    }
}

struct RealText<'a> {
    stated: &'a BTreeMap<u64, String>,
    occurrences: std::slice::Iter<'a, String>,
}

fn encode_part(
    name: &str,
    attributes: &[Attribute],
    text: &mut RealText<'_>,
    out: &mut String,
) -> Result<(), P21Error> {
    check_keyword(name)?;
    out.push_str(name);
    out.push('(');
    for (i, a) in attributes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        encode_attribute(a, text, out)?;
    }
    out.push(')');
    Ok(())
}

fn check_keyword(name: &str) -> Result<(), P21Error> {
    let b = name.as_bytes();
    let ok = !b.is_empty()
        && (b[0].is_ascii_uppercase() || b[0] == b'_')
        && b.iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_');
    if ok && !matches!(name, "HEADER" | "DATA" | "ENDSEC") {
        Ok(())
    } else {
        Err(P21Error::Encode(format!(
            "{name:?} is not an upper-case Part 21 keyword"
        )))
    }
}

fn encode_attribute_text(a: &Attribute) -> Result<String, P21Error> {
    let mut out = String::new();
    encode_attribute(
        a,
        &mut RealText {
            stated: &BTreeMap::new(),
            occurrences: [].iter(),
        },
        &mut out,
    )?;
    Ok(out)
}

fn encode_attribute(
    a: &Attribute,
    text: &mut RealText<'_>,
    out: &mut String,
) -> Result<(), P21Error> {
    match a {
        Attribute::Integer(i) => out.push_str(&i.to_string()),
        Attribute::Real(v) => match text
            .occurrences
            .next()
            .or_else(|| text.stated.get(&v.to_bits()))
        {
            Some(t) => out.push_str(t),
            None => out.push_str(&encode_real(*v)?),
        },
        Attribute::String(s) => {
            out.push('\'');
            out.push_str(&escape(s).replace('\'', "''"));
            out.push('\'');
        }
        Attribute::Enum(s) => {
            let b = s.as_bytes();
            let ok = !b.is_empty()
                && (b[0].is_ascii_alphabetic() || b[0] == b'_')
                && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_');
            if !ok {
                return Err(P21Error::Encode(format!(
                    "{s:?} is not an enumeration value"
                )));
            }
            out.push('.');
            out.push_str(s);
            out.push('.');
        }
        Attribute::Binary(s) => {
            let b = s.as_bytes();
            let ok = !b.is_empty()
                && (b'0'..=b'3').contains(&b[0])
                && b[1..].iter().all(|c| c.is_ascii_hexdigit());
            if !ok {
                return Err(P21Error::Encode(format!("{s:?} is not a binary value")));
            }
            out.push('"');
            out.push_str(s);
            out.push('"');
        }
        Attribute::EntityRef(id) => {
            out.push('#');
            out.push_str(&id.to_string());
        }
        Attribute::Unset => out.push('$'),
        Attribute::Derived => out.push('*'),
        Attribute::List(items) => {
            out.push('(');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                encode_attribute(item, text, out)?;
            }
            out.push(')');
        }
        Attribute::Typed { type_name, value } => {
            check_keyword(type_name)?;
            out.push_str(type_name);
            out.push('(');
            encode_attribute(value, text, out)?;
            out.push(')');
        }
    }
    Ok(())
}

/// The shortest decimal that reads back as `v`, written as a Part 21 REAL: always with a `.`,
/// an exponent as `E` with its sign and at least two digits (`0.1`, `123456789.`, `1.E-07`,
/// `-2.5E+300`, `-0.`). Refused for NaN and infinities, which Part 21 cannot write.
pub fn encode_real(v: f64) -> Result<String, P21Error> {
    if !v.is_finite() {
        return Err(P21Error::Encode(format!("{v} is not a finite real")));
    }
    // `{:e}` gives the shortest round-trip digits: "-1.2345e-7".
    let sci = format!("{v:e}");
    let unformatted = || P21Error::Encode(format!("{v} has no exponent form {sci:?}"));
    let (mantissa, exp) = sci.split_once('e').ok_or_else(unformatted)?;
    let exp: i32 = exp.parse().map_err(|_| unformatted())?;
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(m) => ("-", m),
        None => ("", mantissa),
    };
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    Ok(if (-5..=15).contains(&exp) {
        if exp >= 0 {
            let int_len = (exp + 1) as usize;
            if digits.len() <= int_len {
                format!("{sign}{digits}{}.", "0".repeat(int_len - digits.len()))
            } else {
                format!("{sign}{}.{}", &digits[..int_len], &digits[int_len..])
            }
        } else {
            format!("{sign}0.{}{digits}", "0".repeat((-exp - 1) as usize))
        }
    } else {
        let esign = if exp < 0 { '-' } else { '+' };
        format!(
            "{sign}{}.{}E{esign}{:02}",
            &digits[..1],
            &digits[1..],
            exp.abs()
        )
    })
}

/// A decimal already stated, written verbatim: refused unless it is a Part 21 REAL
/// (`[+-]digits.[digits][E[+-]digits]`) with a finite value.
pub fn encode_real_text(decimal: &str) -> Result<String, P21Error> {
    parse_real_text(decimal)?;
    Ok(decimal.to_string())
}

fn parse_real_text(decimal: &str) -> Result<f64, P21Error> {
    let bad = || P21Error::Encode(format!("{decimal:?} is not a Part 21 REAL"));
    let b = decimal.as_bytes();
    let mut i = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    let int = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == int || b.get(i) != Some(&b'.') {
        return Err(bad());
    }
    i += 1;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i < b.len() {
        if b[i] != b'E' {
            return Err(bad());
        }
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let e = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == e || i != b.len() {
            return Err(bad());
        }
    }
    decimal
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(bad)
}

// ---------------------------------------------------------------------------------------------
// Strings
// ---------------------------------------------------------------------------------------------

/// `text` as an encoded Part 21 string body: `\` as `\\`, characters outside
/// printable ASCII as `\X2\HHHH…\X0\` (UTF-16 code units; `\X4\HHHHHHHH…\X0\` for a run
/// containing a character beyond the BMP), every other character as itself. A `'` stays single
/// here; a quoted Part 21 literal also doubles it. [`encode`] handles decoded attributes.
#[must_use]
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if (' '..='~').contains(&c) {
            if c == '\\' {
                out.push_str("\\\\");
            } else {
                out.push(c);
            }
            i += 1;
            continue;
        }
        let run_end = (i..chars.len())
            .find(|&j| (' '..='~').contains(&chars[j]))
            .unwrap_or(chars.len());
        let run = &chars[i..run_end];
        if run.iter().any(|&c| u32::from(c) > 0xFFFF) {
            out.push_str("\\X4\\");
            for &c in run {
                out.push_str(&format!("{:08X}", u32::from(c)));
            }
        } else {
            out.push_str("\\X2\\");
            for &c in run {
                out.push_str(&format!("{:04X}", u32::from(c)));
            }
        }
        out.push_str("\\X0\\");
        i = run_end;
    }
    out
}

/// Decode an encoded Part 21 string body (not an [`Attribute::String`]):
/// `\\`, `\X\HH` (ISO 8859-1), `\X2\…\X0\` (UTF-16), `\X4\…\X0\` (UCS-4), and `\S\c` under the
/// default code page `\PA\` (ISO 8859-1). Other code pages (`\PB\` … `\PI\`) are refused as
/// unsupported; malformed escapes are refused. Line breaks (`\r`, `\n`) are print control, not
/// part of the value: files break long strings across lines, even inside an escape, and they are
/// dropped. Characters outside ASCII that a file carries raw are kept as they are.
pub fn decode(body: &str) -> Result<String, P21Error> {
    decode_with(body, false)
}

fn decode_with(body: &str, any_page: bool) -> Result<String, P21Error> {
    let bad = |what: &str| P21Error::Decode(format!("{what} in {body:?}"));
    let hex_run = |rest: &str, group: usize, what: &str| -> Result<(Vec<u32>, usize), P21Error> {
        let end = rest
            .find("\\X0\\")
            .ok_or_else(|| bad(&format!("unterminated {what}")))?;
        let hex = &rest[..end];
        if !hex.len().is_multiple_of(group) || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(bad(&format!("malformed {what} run")));
        }
        let values = (0..hex.len())
            .step_by(group)
            .map(|k| u32::from_str_radix(&hex[k..k + group], 16))
            .collect::<Result<_, _>>()
            .map_err(|_| bad(&format!("malformed {what} run")))?;
        Ok((values, end + 4))
    };
    let unbroken: String = body.chars().filter(|c| !matches!(c, '\r' | '\n')).collect();
    let mut out = String::with_capacity(body.len());
    let mut s = unbroken.as_str();
    while let Some(i) = s.find('\\') {
        out.push_str(&s[..i]);
        s = &s[i..];
        if let Some(rest) = s.strip_prefix("\\\\") {
            out.push('\\');
            s = rest;
        } else if let Some(rest) = s.strip_prefix("\\X\\") {
            let h = rest
                .get(..2)
                .filter(|h| h.bytes().all(|c| c.is_ascii_hexdigit()))
                .ok_or_else(|| bad("malformed \\X\\ escape"))?;
            let byte = u8::from_str_radix(h, 16).map_err(|_| bad("malformed \\X\\ escape"))?;
            out.push(char::from(byte));
            s = &rest[2..];
        } else if let Some(rest) = s.strip_prefix("\\X2\\") {
            let (units, used) = hex_run(rest, 4, "\\X2\\")?;
            let units: Vec<u16> = units.into_iter().map(|u| u as u16).collect();
            out.push_str(&String::from_utf16(&units).map_err(|_| bad("invalid UTF-16"))?);
            s = &rest[used..];
        } else if let Some(rest) = s.strip_prefix("\\X4\\") {
            let (values, used) = hex_run(rest, 8, "\\X4\\")?;
            for v in values {
                out.push(char::from_u32(v).ok_or_else(|| bad("invalid \\X4\\ character"))?);
            }
            s = &rest[used..];
        } else if let Some(rest) = s.strip_prefix("\\S\\") {
            let c = rest
                .chars()
                .next()
                .filter(|c| (' '..='~').contains(c))
                .ok_or_else(|| bad("\\S\\ without a printable character"))?;
            out.push(char::from(c as u8 + 0x80));
            s = &rest[1..];
        } else if let Some(rest) = s.strip_prefix("\\P") {
            match rest.as_bytes() {
                [b'A', b'\\', ..] => {}
                [b'B'..=b'I', b'\\', ..] if any_page => {}
                [b'B'..=b'I', b'\\', ..] => return Err(bad("unsupported code page switch")),
                _ => return Err(bad("malformed code page switch")),
            }
            s = &rest[2..];
        } else {
            return Err(bad("unknown escape"));
        }
    }
    out.push_str(s);
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Attribute traversal
// ---------------------------------------------------------------------------------------------

/// Calls `f` on every attribute of the record, nested ones included, depth first.
pub fn visit_attributes(e: &RawEntity, f: &mut impl FnMut(&Attribute)) {
    fn walk(a: &Attribute, f: &mut impl FnMut(&Attribute)) {
        f(a);
        match a {
            Attribute::List(items) => items.iter().for_each(|i| walk(i, f)),
            Attribute::Typed { value, .. } => walk(value, f),
            _ => {}
        }
    }
    match e {
        RawEntity::Simple { attributes, .. } => attributes.iter().for_each(|a| walk(a, f)),
        RawEntity::Complex { parts, .. } => parts
            .iter()
            .flat_map(|p| &p.attributes)
            .for_each(|a| walk(a, f)),
    }
}

pub(crate) fn visit_attributes_mut(e: &mut RawEntity, f: &mut impl FnMut(&mut Attribute)) {
    fn walk(a: &mut Attribute, f: &mut impl FnMut(&mut Attribute)) {
        f(a);
        match a {
            Attribute::List(items) => items.iter_mut().for_each(|i| walk(i, f)),
            Attribute::Typed { value, .. } => walk(value, f),
            _ => {}
        }
    }
    match e {
        RawEntity::Simple { attributes, .. } => attributes.iter_mut().for_each(|a| walk(a, f)),
        RawEntity::Complex { parts, .. } => parts
            .iter_mut()
            .flat_map(|p| &mut p.attributes)
            .for_each(|a| walk(a, f)),
    }
}

#[cfg(test)]
mod description_reference_tests {
    use super::*;
    #[test]
    fn absent_or_non_list_description_skips_reference() {
        let prefixes = vec!["reference---".into()];
        let requests = [("reference---current", prefixes.as_slice())];
        // step-io requires FILE_DESCRIPTION when parsing a complete file. The
        // edit-layer fallback nevertheless accepts a missing header explicitly.
        assert_eq!(file_description_attributes(None, &requests), None);
        let malformed = simple(
            "FILE_DESCRIPTION",
            vec![Attribute::String("not a list".into())],
        );
        assert_eq!(
            file_description_attributes(Some(&malformed), &requests),
            None
        );
    }
}
