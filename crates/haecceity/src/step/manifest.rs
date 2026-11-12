//! Product byte ranges for reading large STEP assemblies one distinct product at a time.
//!
//! [`scan`] uses bounded chunks, retaining structure rather than entity text. It requires a
//! replayable (`Read + Seek`) source for two forward passes, with one rewind between them.
//! Gzip input is scanned in uncompressed coordinates and marked compressed; product reads
//! refuse it. [`Manifest::verify_source`] is the explicit whole-source integrity check.
use super::*;
use crate::p21::{self, streaming};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};

mod index;
mod scan;
mod selected;
use index::{Index, Kind};
pub use scan::scan;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

pub const FORMAT: &str = "quiddity-manifest/2";
/// Independent of the per-product geometry limits. Counts definitions visited and assembly
/// occurrences with bodies emitted; callers may raise it for large assembly manifests.
pub const INSTANCE_LIMIT: usize = 1_000_000;
const CHUNK: usize = 8 * 1024 * 1024;
/// Bound a manifest-controlled allocation before reading a source header.
pub const MAX_HEADER_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ScanOptions {
    pub chunk_size: usize,
    /// Size-only fallback (`--no-product-hashes` in the future CLI).
    pub no_product_hashes: bool,
    /// Explicit directory for a temporary mapped extent/union-find index. No threshold.
    pub index_dir: Option<PathBuf>,
    pub instance_limit: usize,
}
impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            chunk_size: CHUNK,
            no_product_hashes: false,
            index_dir: None,
            instance_limit: INSTANCE_LIMIT,
        }
    }
}

impl ScanOptions {
    pub fn with_chunk_size(mut self, value: usize) -> Self {
        self.chunk_size = value;
        self
    }
    pub fn with_no_product_hashes(mut self, value: bool) -> Self {
        self.no_product_hashes = value;
        self
    }
    pub fn with_index_dir(mut self, value: Option<PathBuf>) -> Self {
        self.index_dir = value;
        self
    }
    pub fn with_instance_limit(mut self, value: usize) -> Self {
        self.instance_limit = value;
        self
    }
}

/// Half-open `[start, end)` byte offsets into the uncompressed source.
pub type ByteRange = [u64; 2];
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct ReaderVersion {
    pub haecceity: String,
}
impl Default for ReaderVersion {
    fn default() -> Self {
        Self {
            haecceity: env!("CARGO_PKG_VERSION").into(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct Source {
    pub size: u64,
    pub sha256: String,
    pub header: ByteRange,
    pub data: Vec<ByteRange>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub compressed: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct Ranges {
    pub ranges: Vec<ByteRange>,
    pub entities: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct Common {
    pub ranges: Vec<ByteRange>,
    pub entities: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct Product {
    pub id: String,
    pub product_definition: u64,
    pub shape: u64,
    pub name: String,
    pub product_id: String,
    pub ranges: Vec<ByteRange>,
    pub entities: u64,
    pub bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    pub faces: Vec<u64>,
    /// Number of distinct EDGE_CURVE entities; edge order is reader-owned.
    pub edge_count: u64,
}
/// One occurrence of a product, placing all its solids and surface models together.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct Instance {
    pub product: String,
    pub path: Vec<u64>,
    pub placement: Placement,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct Unowned {
    pub presentation: Ranges,
    pub other: Ranges,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct Refusal {
    pub products: Vec<String>,
    pub entity: u64,
    pub reason: String,
}
impl Refusal {
    pub fn new(products: Vec<String>, entity: u64, reason: String) -> Self {
        Self {
            products,
            entity,
            reason,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(try_from = "WireManifest")]
#[non_exhaustive]
pub struct Manifest {
    #[serde(skip)]
    product_index: HashMap<String, (usize, Option<usize>)>,
    pub format: String,
    pub source: Source,
    pub reader: ReaderVersion,
    pub common: Common,
    pub products: Vec<Product>,
    pub assemblies: Vec<Assembly>,
    pub instances: Vec<Instance>,
    pub unowned: Unowned,
    pub refusals: Vec<Refusal>,
}
#[derive(Deserialize)]
struct WireManifest {
    format: String,
    source: Source,
    reader: ReaderVersion,
    common: Common,
    products: Vec<Product>,
    assemblies: Vec<Assembly>,
    instances: Vec<Instance>,
    unowned: Unowned,
    refusals: Vec<Refusal>,
}
impl TryFrom<WireManifest> for Manifest {
    type Error = String;
    fn try_from(w: WireManifest) -> Result<Self, String> {
        let mut m = Self {
            product_index: HashMap::new(),
            format: w.format,
            source: w.source,
            reader: w.reader,
            common: w.common,
            products: w.products,
            assemblies: w.assemblies,
            instances: w.instances,
            unowned: w.unowned,
            refusals: w.refusals,
        };
        m.check_version().map_err(|e| e.to_string())?;
        m.reindex_products().map_err(|e| e.to_string())?;
        check_references(
            m.instances.iter().map(|i| i.product.as_str()),
            &m.refusals,
            |id| m.product_index.contains_key(id),
        )?;
        Ok(m)
    }
}
fn valid_product_id(id: &str) -> bool {
    id.starts_with('p') && id.len() > 1 && id[1..].bytes().all(|b| b.is_ascii_digit())
}
fn check_references<'a>(
    instances: impl IntoIterator<Item = &'a str>,
    refusals: &[Refusal],
    known: impl Fn(&str) -> bool,
) -> Result<(), String> {
    for id in instances {
        if !valid_product_id(id) {
            return Err(format!("invalid instance product id {id}"));
        }
        if !known(id) {
            return Err(format!("instance references unknown product {id}"));
        }
    }
    for r in refusals {
        for id in &r.products {
            if !valid_product_id(id) || !known(id) {
                return Err(format!("refusal references unknown product {id}"));
            }
        }
    }
    Ok(())
}
fn failure(why: impl ToString) -> StepError {
    StepError::Unsupported(why.to_string())
}
fn io(e: std::io::Error) -> StepError {
    StepError::Parse(e.to_string())
}
fn digest(h: Sha256) -> String {
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn is_common(n: &str) -> bool {
    n.ends_with("CONTEXT")
        || n.ends_with("UNIT")
        || n == "DIMENSIONAL_EXPONENTS"
        || n == "DERIVED_UNIT_ELEMENT"
        || n == "CONVERSION_BASED_UNIT_WITH_OFFSET"
        || n == "MEASURE_WITH_UNIT"
        || n == "UNCERTAINTY_MEASURE_WITH_UNIT"
        || n == "APPLICATION_PROTOCOL_DEFINITION"
}
fn is_assembly(n: &str) -> bool {
    matches!(
        n,
        "AXIS2_PLACEMENT_3D"
            | "AXIS2_PLACEMENT_2D"
            | "PRODUCT"
            | "PRODUCT_DEFINITION"
            | "PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS"
            | "PRODUCT_DEFINITION_FORMATION"
            | "PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE"
            | "PRODUCT_DEFINITION_SHAPE"
            | "SHAPE_DEFINITION_REPRESENTATION"
            | "NEXT_ASSEMBLY_USAGE_OCCURRENCE"
            | "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION"
            | "ITEM_DEFINED_TRANSFORMATION"
            | "REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION"
            | "REPRESENTATION_RELATIONSHIP"
    )
}
fn is_presentation(n: &str) -> bool {
    [
        "STYLED_",
        "COLOUR",
        "COLOR",
        "PRESENTATION_",
        "SURFACE_STYLE",
        "CURVE_STYLE",
        "FILL_AREA_STYLE",
        "POINT_STYLE",
        "TEXT_STYLE",
        "TEXT_LITERAL",
        "COMPOSITE_TEXT",
        "SURFACE_CONDITION",
        "SURFACE_TEXTURE",
        "ANNOTATION",
        "DRAUGHTING",
        "CAMERA",
        "LIGHT_SOURCE",
        "GEOMETRIC_TOLERANCE",
        "DATUM",
        "DIMENSIONAL_",
        "DIMENSION_",
        "SHAPE_ASPECT",
        "TESSELLATED_ANNOTATION",
    ]
    .iter()
    .any(|p| n.starts_with(p))
        || n.contains("TOLERANCE")
        || n.contains("SHAPE_ASPECT")
        || n.contains("STYLE")
        || n.contains("SYMBOL")
        || n.ends_with("_FONT")
        || matches!(
            n,
            "MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION"
                | "OVER_RIDING_STYLED_ITEM"
                | "INVISIBILITY"
                | "PRE_DEFINED_COLOUR"
                | "PRE_DEFINED_CURVE_FONT"
                | "GEOMETRIC_ITEM_SPECIFIC_USAGE"
                | "ITEM_IDENTIFIED_REPRESENTATION_USAGE"
                | "COMMON_DATUM"
                | "LIMITS_AND_FITS"
        )
}
const PRELUDE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION((''),'2;1');FILE_NAME('','','',(''),(''),'','');FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));ENDSEC;DATA;";
const END: &[u8] = b"\nENDSEC;END-ISO-10303-21;";
fn record(bytes: &[u8], id: u64) -> Result<RawEntity, StepError> {
    let mut b = PRELUDE.to_vec();
    b.extend_from_slice(bytes);
    b.extend_from_slice(END);
    step_io::parser::parse(b.as_slice())
        .map_err(|e| StepError::Parse(e.to_string()))?
        .entities
        .remove(&id)
        .ok_or_else(dangling)
}
fn attributes(e: &RawEntity) -> &[Attribute] {
    match e {
        RawEntity::Simple { attributes, .. } => attributes,
        RawEntity::Complex { parts, .. } => parts
            .iter()
            .find(|p| p.name == "REPRESENTATION")
            .map_or(&[], |p| p.attributes.as_slice()),
    }
}
fn name(e: &RawEntity) -> &str {
    match e {
        RawEntity::Simple { name, .. } => name,
        RawEntity::Complex { .. } => "COMPLEX",
    }
}
fn reference(a: Option<&Attribute>) -> Option<u64> {
    match a? {
        Attribute::EntityRef(n) => Some(*n),
        _ => None,
    }
}
fn references(e: &RawEntity) -> Vec<u64> {
    fn walk(a: &Attribute, out: &mut Vec<u64>) {
        match a {
            Attribute::EntityRef(n) => out.push(*n),
            Attribute::List(v) => {
                for a in v {
                    walk(a, out)
                }
            }
            Attribute::Typed { value, .. } => walk(value, out),
            _ => {}
        }
    }
    let mut out = Vec::new();
    match e {
        RawEntity::Simple { attributes, .. } => {
            for a in attributes {
                walk(a, &mut out)
            }
        }
        RawEntity::Complex { parts, .. } => {
            for a in parts.iter().flat_map(|p| &p.attributes) {
                walk(a, &mut out)
            }
        }
    }
    out
}
fn list(a: Option<&Attribute>) -> Vec<u64> {
    match a {
        Some(Attribute::List(v)) => v.iter().filter_map(|a| reference(Some(a))).collect(),
        _ => Vec::new(),
    }
}
fn placement_model(
    graph: &BTreeMap<u64, RawEntity>,
    index: &Index,
    extra: &[u8],
    placement_nodes: &HashSet<u32>,
) -> Result<(m::StepModel, step_io::parser::Graph, step_io::Report), StepError> {
    let mut b = PRELUDE.to_vec();
    for (&id, e) in graph {
        let mut e = e.clone();
        let prune = |name: &str, a: &mut Vec<Attribute>| {
            if matches!(name, "CLOSED_SHELL" | "OPEN_SHELL") {
                if let Some(item) = a.get_mut(1) {
                    *item = Attribute::List(Vec::new());
                }
            } else if (name.ends_with("SHAPE_REPRESENTATION") || name == "REPRESENTATION")
                && let Some(Attribute::List(items)) = a.get_mut(1)
            {
                items.retain(|a| {
                    reference(Some(a)).is_some_and(|n| {
                        graph.get(&n).is_some_and(|e| {
                            matches!(
                                self::name(e),
                                "MANIFOLD_SOLID_BREP"
                                    | "BREP_WITH_VOIDS"
                                    | "FACETED_BREP"
                                    | "SHELL_BASED_SURFACE_MODEL"
                                    | "AXIS2_PLACEMENT_3D"
                                    | "AXIS2_PLACEMENT_2D"
                            )
                        }) || index.lookup(n).is_some_and(|slot| {
                            index.kind(slot) == Kind::Frame && placement_nodes.contains(&slot)
                        })
                    })
                });
            }
        };
        match &mut e {
            RawEntity::Simple {
                name, attributes, ..
            } => prune(name, attributes),
            RawEntity::Complex { parts, .. } => {
                for p in parts {
                    prune(&p.name, &mut p.attributes)
                }
            }
        }
        b.extend_from_slice(format!("#{id}={};\n", p21::encode(&e).map_err(failure)?).as_bytes());
    }
    b.extend_from_slice(extra);
    b.extend_from_slice(END);
    let graph = step_io::parser::parse(b.as_slice()).map_err(failure)?;
    let (model, report) = step_io::read(b.as_slice()).map_err(failure)?;
    Ok((model, graph, report))
}
fn key_id(ids: &FileIds<'_>, key: m::EntityKey) -> Result<u64, StepError> {
    let (kind, index) = match key {
        m::EntityKey::ManifoldSolidBrep(i) => ("MANIFOLD_SOLID_BREP", i.0),
        m::EntityKey::BrepWithVoids(i) => ("BREP_WITH_VOIDS", i.0),
        m::EntityKey::ShellBasedSurfaceModel(i) => ("SHELL_BASED_SURFACE_MODEL", i.0),
        m::EntityKey::NextAssemblyUsageOccurrence(i) => ("NEXT_ASSEMBLY_USAGE_OCCURRENCE", i.0),
        _ => return ids.id(key),
    };
    ids.by_type
        .get(kind)
        .and_then(|v| v.get(index as usize))
        .copied()
        .ok_or_else(dangling)
}
impl Manifest {
    /// Rebuild lookups after changing product ids/order/membership or refusal membership/order.
    /// Scanning and deserialization build it automatically; editing hashes needs no rebuild.
    pub fn reindex_products(&mut self) -> Result<(), StepError> {
        let mut index = HashMap::with_capacity(self.products.len());
        for (i, product) in self.products.iter().enumerate() {
            if index.insert(product.id.clone(), (i, None)).is_some() {
                return Err(failure(format!("duplicate product id {}", product.id)));
            }
        }
        for (i, refusal) in self.refusals.iter().enumerate() {
            for id in &refusal.products {
                if let Some((_, first)) = index.get_mut(id) {
                    first.get_or_insert(i);
                }
            }
        }
        self.product_index = index;
        Ok(())
    }

    /// Constant-time lookup in the index built by scan/deserialization.
    pub fn product(&self, id: &str) -> Result<&Product, StepError> {
        let (i, _) = self
            .product_index
            .get(id)
            .ok_or_else(|| failure(format!("unknown product {id}")))?;
        self.products.get(*i).filter(|p| p.id == id).ok_or_else(|| {
            failure("product ids/order changed; call reindex_products before reading")
        })
    }

    fn check_version(&self) -> Result<(), StepError> {
        if self.format != FORMAT {
            return Err(failure(format!(
                "manifest format {} differs from reader format {FORMAT}; rescan the source",
                self.format
            )));
        }
        let current = ReaderVersion::default();
        if self.reader != current {
            return Err(failure(format!(
                "manifest reader haecceity {} differs from current haecceity {}; rescan the source",
                self.reader.haecceity, current.haecceity
            )));
        }
        Ok(())
    }
    /// One forward pass over the source's header and the selected common/product ranges.
    /// Hashes are checked before geometry is parsed. Per-read geometry limits remain those
    /// of `read_part_definitions` and `read_part`; manifest instance limits do not change them.
    pub fn read_product(&self, file: &File, id: &str) -> Result<Part, StepError> {
        self.check_version()?;
        let p = self.product(id)?;
        let refuse = |why: String| {
            failure(format!(
                "product {} (#{} {}): {why}",
                p.id, p.product_definition, p.name
            ))
        };
        if self.source.compressed {
            return Err(refuse("compressed source cannot be read by ranges; supply an uncompressed source and rescan".into()));
        }
        if let Some(i) = self.product_index.get(id).and_then(|(_, refusal)| *refusal) {
            let r = self.refusals.get(i).ok_or_else(|| {
                refuse("refusals changed; call reindex_products before reading".into())
            })?;
            return Err(refuse(r.reason.clone()));
        }
        if file.metadata().map_err(io)?.len() != self.source.size {
            return Err(refuse("source size mismatch".into()));
        }
        if self.source.header[1] > MAX_HEADER_BYTES {
            return Err(refuse(format!("header exceeds {MAX_HEADER_BYTES} bytes")));
        }
        let mut ranges: Vec<_> = self
            .common
            .ranges
            .iter()
            .map(|&r| (r, 0))
            .chain(p.ranges.iter().map(|&r| (r, 1)))
            .collect();
        ranges.sort_unstable();
        let mut end = self.source.header[1];
        if self.source.header[0] != 0 || end > self.source.size {
            return Err(refuse("invalid header range".into()));
        }
        for &(r, _) in &ranges {
            if r[0] < end || r[1] < r[0] || r[1] > self.source.size {
                return Err(refuse("invalid or overlapping ranges".into()));
            }
            end = r[1];
        }
        if p.bytes != p.ranges.iter().map(|r| r[1] - r[0]).sum::<u64>() {
            return Err(refuse("range byte count mismatch".into()));
        }
        let mut input = file;
        input.seek(SeekFrom::Start(0)).map_err(io)?;
        let header_len = usize::try_from(self.source.header[1])
            .map_err(|_| refuse("header is too large".into()))?;
        let mut bytes = vec![0; header_len];
        input
            .read_exact(&mut bytes)
            .map_err(|e| refuse(e.to_string()))?;
        if bytes.starts_with(&[31, 139]) {
            return Err(refuse("compressed source cannot be read by ranges".into()));
        }
        bytes.extend_from_slice(b"\nDATA;\n");
        let mut hashes = [Sha256::new(), Sha256::new()];
        hashes[0].update(&bytes[..header_len]);
        let mut buffer = vec![0; CHUNK];
        for (r, group) in ranges {
            input
                .seek(SeekFrom::Start(r[0]))
                .map_err(|e| refuse(e.to_string()))?;
            let mut left = r[1] - r[0];
            while left > 0 {
                let n = buffer.len().min(left as usize);
                input
                    .read_exact(&mut buffer[..n])
                    .map_err(|e| refuse(e.to_string()))?;
                hashes[group].update(&buffer[..n]);
                bytes.extend_from_slice(&buffer[..n]);
                left -= n as u64;
            }
            bytes.push(b'\n');
        }
        let [common, product] = hashes;
        for (label, want, got) in [
            ("common", &self.common.sha256, digest(common)),
            ("product", &p.sha256, digest(product)),
        ] {
            if want.as_ref().is_some_and(|h| *h != got) {
                return Err(refuse(format!("{label} sha256 mismatch")));
            }
        }
        bytes.extend_from_slice(END);
        let file = StepFile::read(&bytes).map_err(|e| refuse(e.to_string()))?;
        let def = file
            .parts
            .iter()
            .find(|d| d.product_definition == p.product_definition)
            .ok_or_else(|| refuse("definition missing from extracted document".into()))?;
        if def.faces != p.faces {
            return Err(refuse("face entity order differs from manifest".into()));
        }
        if def.edges.len() as u64 != p.edge_count {
            return Err(refuse("edge count differs from manifest".into()));
        }
        let part = own_part(&file.parsed, def).map_err(|e| refuse(e.to_string()))?;
        // Edge indices belong to the unchanged product reader. Normals-only shell repairs
        // can change traversal order; identity is checked by count and set, never by order.
        let got: HashSet<_> = (0..part.edges.len())
            .filter_map(|e| part.edge_source(e).map(|s| s.entity))
            .collect();
        let expected: HashSet<_> = def.edges.iter().copied().collect();
        if got.len() != def.edges.len() || got != expected {
            return Err(refuse(
                "edge entity count or set differs from product definition".into(),
            ));
        }
        Ok(part)
    }
    /// Full uncompressed source size and SHA-256 verification. Unlike product checks this
    /// catches changes in assembly, presentation, comments and other unowned bytes too.
    pub fn verify_source<R: Read + Seek>(&self, reader: R) -> Result<(), StepError> {
        self.check_version()?;
        scan::verify(self, reader)
    }
}
