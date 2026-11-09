//! Product byte ranges for reading large STEP assemblies one distinct product at a time.
//!
//! [`scan`] uses bounded chunks, retaining structure rather than entity text. It requires a
//! replayable (`Read + Seek`) source for dependency lookups and the optional range-hash pass.
//! Gzip input is scanned in uncompressed coordinates and marked compressed; product reads
//! refuse it. [`Manifest::verify_source`] is the explicit whole-source integrity check.
use super::*;
use crate::p21::{self, streaming};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

pub const FORMAT: &str = "quiddity-manifest/1";
/// Independent of the per-product geometry limits. Counts definitions visited and solid or
/// surface-model instances emitted; callers may raise it for large assembly manifests.
pub const INSTANCE_LIMIT: usize = 1_000_000;
const CHUNK: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug)]
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

/// Half-open `[start, end)` byte offsets into the uncompressed source.
pub type ByteRange = [u64; 2];
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReaderVersion {
    pub haecceity: String,
    pub quiddity: String,
}
impl Default for ReaderVersion {
    fn default() -> Self {
        Self {
            haecceity: env!("CARGO_PKG_VERSION").into(),
            quiddity: env!("CARGO_PKG_VERSION").into(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Source {
    pub size: u64,
    pub sha256: String,
    pub header: ByteRange,
    pub data: Vec<ByteRange>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub compressed: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Ranges {
    pub ranges: Vec<ByteRange>,
    pub entities: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Common {
    pub ranges: Vec<ByteRange>,
    pub entities: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
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
    pub edges: Vec<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Instance {
    pub product: String,
    pub path: Vec<u64>,
    pub placement: Placement,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Unowned {
    pub presentation: Ranges,
    pub other: Ranges,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Refusal {
    pub products: Vec<String>,
    pub entity: u64,
    pub reason: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(try_from = "WireManifest")]
pub struct Manifest {
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
        let m = Self {
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
        Ok(m)
    }
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

/// Classification is by external-mapping type names, never by an entity's parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Common,
    Assembly,
    Presentation,
    Geometry,
}
fn class(types: &[String]) -> Class {
    if types.iter().any(|n| is_common(n)) {
        Class::Common
    } else if types
        .iter()
        .any(|n| n == "SHAPE_REPRESENTATION_RELATIONSHIP")
        && !types
            .iter()
            .any(|n| n == "REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION")
    {
        Class::Geometry
    } else if types.iter().any(|n| is_assembly(n)) {
        Class::Assembly
    } else if types.iter().any(|n| is_presentation(n)) {
        Class::Presentation
    } else {
        Class::Geometry
    }
}
fn is_common(n: &str) -> bool {
    n.ends_with("CONTEXT")
        || n.ends_with("UNIT")
        || n == "DIMENSIONAL_EXPONENTS"
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
fn topology(n: &str) -> bool {
    n.ends_with("SHAPE_REPRESENTATION")
        || matches!(
            n,
            "REPRESENTATION"
                | "SHAPE_REPRESENTATION_RELATIONSHIP"
                | "MANIFOLD_SOLID_BREP"
                | "BREP_WITH_VOIDS"
                | "FACETED_BREP"
                | "SHELL_BASED_SURFACE_MODEL"
                | "CLOSED_SHELL"
                | "OPEN_SHELL"
                | "ORIENTED_CLOSED_SHELL"
                | "ADVANCED_FACE"
                | "FACE_SURFACE"
                | "FACE_BOUND"
                | "FACE_OUTER_BOUND"
                | "EDGE_LOOP"
                | "ORIENTED_EDGE"
        )
}

// The extent/union-find entry is exactly 16 bytes. IDs use a sparse lookup, independently of
// their largest value. Mapped mode puts the same entries in a private temporary file; mappings
// are dropped before the file is removed, including on error.
enum Storage {
    Memory(Vec<[u8; 16]>),
    Mapped {
        map: memmap2::MmapMut,
        file: tempfile::NamedTempFile,
        len: usize,
    },
}
impl Storage {
    fn new(dir: Option<&PathBuf>) -> Result<Self, StepError> {
        if let Some(dir) = dir {
            let file = tempfile::NamedTempFile::new_in(dir).map_err(io)?;
            file.as_file().set_len(16 * 4096).map_err(io)?;
            // SAFETY: this newly created private file is owned by Storage. No external handle
            // is exposed, and the old map is replaced before any subsequent access after growth.
            let map = unsafe { memmap2::MmapMut::map_mut(file.as_file()) }.map_err(io)?;
            Ok(Self::Mapped { map, file, len: 0 })
        } else {
            Ok(Self::Memory(Vec::new()))
        }
    }
    fn push(&mut self, entry: [u8; 16]) -> Result<(), StepError> {
        match self {
            Self::Memory(v) => v.push(entry),
            Self::Mapped { map, file, len } => {
                if (*len + 1) * 16 > map.len() {
                    file.as_file().set_len((map.len() * 2) as u64).map_err(io)?;
                    // SAFETY: as above; only growth, with no references into the previous map.
                    *map = unsafe { memmap2::MmapMut::map_mut(file.as_file()) }.map_err(io)?;
                }
                map[*len * 16..(*len + 1) * 16].copy_from_slice(&entry);
                *len += 1;
            }
        }
        Ok(())
    }
    fn get(&self, i: u32) -> [u8; 16] {
        match self {
            Self::Memory(v) => v[i as usize],
            Self::Mapped { map, .. } => {
                let mut out = [0; 16];
                out.copy_from_slice(&map[i as usize * 16..(i as usize + 1) * 16]);
                out
            }
        }
    }
    fn set(&mut self, i: u32, e: [u8; 16]) {
        match self {
            Self::Memory(v) => v[i as usize] = e,
            Self::Mapped { map, .. } => {
                map[i as usize * 16..(i as usize + 1) * 16].copy_from_slice(&e)
            }
        }
    }
}
struct Index {
    entries: Storage,
    lookup: HashMap<u64, u32>,
    ids: Vec<u64>,
    classes: Vec<Class>,
    /// Whether this entity and the preceding entity are separated only by trivia.
    adjacent: Vec<bool>,
    pending: HashMap<u64, Vec<u32>>,
    dependencies: Vec<(u32, u64)>,
}
impl Index {
    fn new(options: &ScanOptions) -> Result<Self, StepError> {
        Ok(Self {
            entries: Storage::new(options.index_dir.as_ref())?,
            lookup: HashMap::new(),
            ids: Vec::new(),
            classes: Vec::new(),
            adjacent: Vec::new(),
            pending: HashMap::new(),
            dependencies: Vec::new(),
        })
    }
    fn parent(&self, i: u32) -> u32 {
        let e = self.entries.get(i);
        u32::from_le_bytes([e[12], e[13], e[14], e[15]])
    }
    fn root(&mut self, mut i: u32) -> u32 {
        while self.parent(i) != i {
            let p = self.parent(i);
            let root = self.parent(p);
            let mut e = self.entries.get(i);
            e[12..].copy_from_slice(&root.to_le_bytes());
            self.entries.set(i, e);
            i = p;
        }
        i
    }
    fn union(&mut self, a: u32, b: u32) {
        let a = self.root(a);
        let b = self.root(b);
        if a != b {
            let mut e = self.entries.get(a);
            e[12..].copy_from_slice(&b.to_le_bytes());
            self.entries.set(a, e);
        }
    }
    fn range(&self, i: u32) -> ByteRange {
        let e = self.entries.get(i);
        let start = u64::from_le_bytes(e[..8].try_into().unwrap_or_default());
        let len = u32::from_le_bytes(e[8..12].try_into().unwrap_or_default());
        [start, start + u64::from(len)]
    }
    fn add(&mut self, item: &streaming::Instance) -> Result<(), StepError> {
        let i = u32::try_from(self.ids.len())
            .map_err(|_| StepError::Limit("more than u32 entities".into()))?;
        if self.lookup.insert(item.id, i).is_some() {
            return Err(failure(format!("duplicate entity #{}", item.id)));
        }
        let mut entry = [0; 16];
        entry[..8].copy_from_slice(&item.offset.to_le_bytes());
        entry[8..12].copy_from_slice(&item.length.to_le_bytes());
        entry[12..].copy_from_slice(&i.to_le_bytes());
        self.entries.push(entry)?;
        let c = class(&item.types);
        self.ids.push(item.id);
        self.classes.push(c);
        self.adjacent
            .push(i > 0 && self.range(i - 1)[1] == item.gap_start);
        if let Some(waiting) = self.pending.remove(&item.id) {
            for from in waiting {
                if c == Class::Geometry {
                    self.union(from, i);
                } else if c == Class::Assembly {
                    self.dependencies.push((from, item.id));
                }
            }
        }
        if c == Class::Geometry {
            for id in &item.references {
                if let Some(&target) = self.lookup.get(id) {
                    if self.classes[target as usize] == Class::Geometry {
                        self.union(i, target);
                    } else if self.classes[target as usize] == Class::Assembly {
                        self.dependencies.push((i, *id));
                    }
                } else {
                    self.pending.entry(*id).or_default().push(i);
                }
            }
        }
        Ok(())
    }
}

// Gzip offsets are uncompressed. Replaying a backwards seek restarts the decoder; plain files
// seek directly. Product reads deliberately do not use this adapter.
enum Replay<R: Read + Seek> {
    Plain(R),
    Gzip {
        decoder: Option<flate2::read::GzDecoder<R>>,
        at: u64,
    },
}
impl<R: Read + Seek> Replay<R> {
    fn new(mut r: R) -> Result<Self, StepError> {
        r.seek(SeekFrom::Start(0)).map_err(io)?;
        let mut magic = [0; 2];
        let mut count = 0;
        while count < magic.len() {
            let n = r.read(&mut magic[count..]).map_err(io)?;
            if n == 0 {
                break;
            }
            count += n;
        }
        r.seek(SeekFrom::Start(0)).map_err(io)?;
        Ok(if count == 2 && magic == [31, 139] {
            Self::Gzip {
                decoder: Some(flate2::read::GzDecoder::new(r)),
                at: 0,
            }
        } else {
            Self::Plain(r)
        })
    }
}
impl<R: Read + Seek> Read for Replay<R> {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(r) => r.read(b),
            Self::Gzip { decoder, at } => {
                let n = decoder
                    .as_mut()
                    .ok_or_else(|| std::io::Error::other("gzip replay unavailable"))?
                    .read(b)?;
                *at += n as u64;
                Ok(n)
            }
        }
    }
}
impl<R: Read + Seek> Seek for Replay<R> {
    fn seek(&mut self, p: SeekFrom) -> std::io::Result<u64> {
        match self {
            Self::Plain(r) => r.seek(p),
            Self::Gzip { decoder, at } => {
                let SeekFrom::Start(target) = p else {
                    return Err(std::io::Error::other(
                        "gzip replay needs an absolute offset",
                    ));
                };
                if target < *at {
                    let old = decoder
                        .take()
                        .ok_or_else(|| std::io::Error::other("gzip replay unavailable"))?;
                    let mut r = old.into_inner();
                    r.seek(SeekFrom::Start(0))?;
                    *decoder = Some(flate2::read::GzDecoder::new(r));
                    *at = 0;
                }
                let count = std::io::copy(
                    &mut decoder
                        .as_mut()
                        .ok_or_else(|| std::io::Error::other("gzip replay unavailable"))?
                        .take(target - *at),
                    &mut std::io::sink(),
                )?;
                *at += count;
                Ok(*at)
            }
        }
    }
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
fn fetch<R: Read + Seek>(source: &mut R, index: &Index, id: u64) -> Result<RawEntity, StepError> {
    let &slot = index
        .lookup
        .get(&id)
        .ok_or_else(|| failure(format!("missing dependency #{id}")))?;
    let [start, end] = index.range(slot);
    source.seek(SeekFrom::Start(start)).map_err(io)?;
    let mut b = vec![0; (end - start) as usize];
    source.read_exact(&mut b).map_err(io)?;
    record(&b, id)
}

struct Hashed<R> {
    inner: R,
    hash: Sha256,
    size: u64,
}
impl<R: Read> Read for Hashed<R> {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(b)?;
        self.hash.update(&b[..n]);
        self.size += n as u64;
        Ok(n)
    }
}

/// Scan a replayable source. The source digest is calculated during the statement pass;
/// range hashes use a second forward walk. Only selected placement dependencies are sought
/// between those walks. Source and index files are never implicitly created in the workspace.
pub fn scan<R: Read + Seek>(reader: R, options: &ScanOptions) -> Result<Manifest, StepError> {
    let mut source = Replay::new(reader)?;
    let compressed = matches!(source, Replay::Gzip { .. });
    let mut index = Index::new(options)?;
    let mut graph = BTreeMap::new();
    let mut axes = HashSet::new();
    let mut header_end = 0;
    let mut data = Vec::new();
    let mut hashed = Hashed {
        inner: &mut source,
        hash: Sha256::new(),
        size: 0,
    };
    streaming::scan(&mut hashed, options.chunk_size, |event| {
        match event {
            streaming::Event::Instance(i, b) => {
                index
                    .add(&i)
                    .map_err(|e| p21::P21Error::Parse(e.to_string()))?;
                if i.types.iter().any(|n| n.starts_with("AXIS2_PLACEMENT")) {
                    axes.insert(i.id);
                }
                if (matches!(class(&i.types), Class::Common | Class::Assembly)
                    && !i.types.iter().any(|n| n.starts_with("AXIS2_PLACEMENT")))
                    || i.types.iter().any(|n| topology(n))
                {
                    graph.insert(
                        i.id,
                        record(b, i.id).map_err(|e| p21::P21Error::Parse(e.to_string()))?,
                    );
                }
            }
            streaming::Event::HeaderEnd(end) => header_end = end,
            streaming::Event::Data(start, end) => data.push([start, end]),
            streaming::Event::Header(..) => {}
        }
        Ok(())
    })
    .map_err(|e| StepError::Parse(e.to_string()))?;
    let size = hashed.size;
    let sha256 = digest(hashed.hash);
    if !index.pending.is_empty() {
        return Err(failure(format!(
            "missing indexed references: {:?}",
            index.pending.keys().take(8).collect::<Vec<_>>()
        )));
    }
    index.pending.shrink_to_fit();
    // Only the placement frames used by representations or assembly transformations are
    // fetched. Ordinary surface frames, points, curves and splines are never retained.
    let mut stack: Vec<u64> = graph
        .values()
        .filter(|e| name(e) == "ITEM_DEFINED_TRANSFORMATION")
        .flat_map(references)
        .filter(|id| axes.contains(id))
        .collect();
    for e in graph.values() {
        for id in list(attributes(e).get(1)) {
            if axes.contains(&id) {
                stack.push(id);
            }
        }
    }
    while let Some(id) = stack.pop() {
        if graph.contains_key(&id) {
            continue;
        }
        let e = fetch(&mut source, &index, id)?;
        stack.extend(references(&e));
        graph.insert(id, e);
    }
    let assemblies = super::assemblies(&graph);
    let (placement_model, placement_graph, report) = placement_model(&graph)?;
    let mut ids = FileIds::new(&placement_graph, &placement_model, &report);
    let dropped: HashSet<_> = report.dropped.iter().map(|(id, _)| *id).collect();
    for kind in [
        "MANIFOLD_SOLID_BREP",
        "BREP_WITH_VOIDS",
        "SHELL_BASED_SURFACE_MODEL",
        "NEXT_ASSEMBLY_USAGE_OCCURRENCE",
    ] {
        ids.by_type.insert(
            kind,
            placement_graph
                .entities
                .iter()
                .filter_map(|(&id, e)| (name(e) == kind && !dropped.contains(&id)).then_some(id))
                .collect(),
        );
    }
    let scene = placement_model.scene();
    let units = scene.units();
    let to_mm = units.length.as_ref().map_or(1.0, |u| u.to_si * 1000.0);
    let mut traversal = Traversal {
        graph: &graph,
        ids: &mut ids,
        products: Vec::new(),
        instances: Vec::new(),
        visited: 0,
        limit: options.instance_limit,
        to_mm,
        assemblies: &assemblies,
    };
    for root in scene.root_definitions() {
        traversal.visit(root, IDENTITY, &mut Vec::new())?;
    }
    let mut products = traversal.products;
    let instances = traversal.instances;
    let mut roots: HashMap<u32, Vec<usize>> = HashMap::new();
    let mut seeds = vec![Vec::new(); products.len()];
    let mut chain: HashMap<u64, Vec<usize>> = HashMap::new();
    for (p, product) in products.iter().enumerate() {
        let mut own = Vec::new();
        for (&sdr, e) in &graph {
            if name(e) == "SHAPE_DEFINITION_REPRESENTATION"
                && reference(attributes(e).first()) == Some(product.shape)
            {
                own.push(sdr);
                if let Some(rep) = reference(attributes(e).get(1)) {
                    seeds[p].push(rep);
                    let &slot = index.lookup.get(&rep).ok_or_else(dangling)?;
                    let root = index.root(slot);
                    let owners = roots.entry(root).or_default();
                    if !owners.contains(&p) {
                        owners.push(p);
                    }
                }
            }
        }
        // Product identity chain, excluding the representation's geometry.
        let mut seen = HashSet::new();
        while let Some(id) = own.pop() {
            if !seen.insert(id) {
                continue;
            }
            let &slot = index
                .lookup
                .get(&id)
                .ok_or_else(|| failure(format!("missing dependency #{id}")))?;
            if matches!(
                index.classes[slot as usize],
                Class::Common | Class::Geometry
            ) {
                continue;
            }
            chain.entry(id).or_default().push(p);
            if let Some(e) = graph.get(&id) {
                own.extend(references(e));
            }
        }
    }
    // Frames form an assembly-class boundary. Their point/direction dependencies are copied
    // with a product without unioning unrelated products through a shared standard frame.
    let mut boundary: HashMap<u32, Vec<u64>> = HashMap::new();
    for (from, id) in std::mem::take(&mut index.dependencies) {
        let root = index.root(from);
        boundary.entry(root).or_default().push(id);
    }
    for (&root, owners) in &roots {
        let mut stack = boundary.get(&root).cloned().unwrap_or_default();
        let mut seen = HashSet::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            let &slot = index
                .lookup
                .get(&id)
                .ok_or_else(|| failure(format!("missing frame dependency #{id}")))?;
            if index.classes[slot as usize] == Class::Common {
                continue;
            }
            if let std::collections::btree_map::Entry::Vacant(entry) = graph.entry(id) {
                entry.insert(fetch(&mut source, &index, id)?);
            }
            if let Some(e) = graph.get(&id) {
                stack.extend(references(e));
            }
            let assigned = chain.entry(id).or_default();
            for &p in owners {
                if !assigned.contains(&p) {
                    assigned.push(p);
                }
            }
        }
    }
    let mut refusals = Vec::new();
    let mut shared: Vec<_> = roots
        .iter()
        .filter(|(_, owners)| owners.len() > 1)
        .map(|(&root, owners)| (root, owners.clone()))
        .collect();
    shared.sort_by_key(|(r, _)| index.ids[*r as usize]);
    for (root, owners) in shared {
        let entity = shared_witness(&mut source, &index, &seeds, &owners)?
            .unwrap_or(index.ids[root as usize]);
        refusals.push(Refusal {
            products: owners.iter().map(|&p| products[p].id.clone()).collect(),
            entity,
            reason: format!("shared geometry entity #{entity}; use the whole-file reader"),
        });
    }
    let mut common = Common::default();
    let mut unowned = Unowned::default();
    let mut previous_common = None;
    let mut previous_presentation = None;
    let mut previous_other = None;
    let mut previous_products = vec![None; products.len()];
    for i in 0..index.ids.len() {
        let slot = i as u32;
        let id = index.ids[i];
        let range = index.range(slot);
        match index.classes[i] {
            Class::Common => {
                push_range(
                    &mut common.ranges,
                    range,
                    slot,
                    index.adjacent[i],
                    &mut previous_common,
                );
                common.entities += 1;
            }
            Class::Presentation => {
                push_range(
                    &mut unowned.presentation.ranges,
                    range,
                    slot,
                    index.adjacent[i],
                    &mut previous_presentation,
                );
                unowned.presentation.entities += 1;
            }
            c => {
                let mut owners = chain.get(&id).cloned().unwrap_or_default();
                if c == Class::Geometry {
                    let root = index.root(slot);
                    if let Some(more) = roots.get(&root) {
                        for &p in more {
                            if !owners.contains(&p) {
                                owners.push(p);
                            }
                        }
                    }
                }
                if !owners.is_empty() {
                    for p in owners {
                        push_range(
                            &mut products[p].ranges,
                            range,
                            slot,
                            index.adjacent[i],
                            &mut previous_products[p],
                        );
                        products[p].entities += 1;
                    }
                } else {
                    push_range(
                        &mut unowned.other.ranges,
                        range,
                        slot,
                        index.adjacent[i],
                        &mut previous_other,
                    );
                    unowned.other.entities += 1;
                }
            }
        }
    }
    for p in &mut products {
        p.bytes = p.ranges.iter().map(|r| r[1] - r[0]).sum();
    }
    let mut manifest = Manifest {
        format: FORMAT.into(),
        source: Source {
            size,
            sha256,
            header: [0, header_end],
            data,
            compressed,
        },
        reader: ReaderVersion::default(),
        common,
        products,
        assemblies,
        instances,
        unowned,
        refusals,
    };
    if !options.no_product_hashes {
        manifest.hash_ranges(&mut source)?;
    }
    Ok(manifest)
}
// A union-find root need not itself be reachable from both representations. Report an
// actual shared dependency, rather than the arbitrary representative chosen by union-find.
fn shared_witness<R: Read + Seek>(
    source: &mut R,
    index: &Index,
    seeds: &[Vec<u64>],
    owners: &[usize],
) -> Result<Option<u64>, StepError> {
    let mut reached = HashMap::new();
    let mut refs: HashMap<u64, Vec<u64>> = HashMap::new();
    for &p in owners {
        let mut stack = seeds[p].clone();
        let mut seen = HashSet::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            let &slot = index.lookup.get(&id).ok_or_else(dangling)?;
            if index.classes[slot as usize] != Class::Geometry {
                continue;
            }
            if reached.insert(id, p).is_some_and(|previous| previous != p) {
                return Ok(Some(id));
            }
            if let std::collections::hash_map::Entry::Vacant(entry) = refs.entry(id) {
                entry.insert(references(&fetch(source, index, id)?));
            }
            if let Some(next) = refs.get(&id) {
                stack.extend(next);
            }
        }
    }
    Ok(None)
}

fn push_range(
    ranges: &mut Vec<ByteRange>,
    range: ByteRange,
    slot: u32,
    adjacent: bool,
    previous: &mut Option<u32>,
) {
    if adjacent
        && previous.is_some_and(|p| p + 1 == slot)
        && let Some(last) = ranges.last_mut()
    {
        last[1] = range[1];
    } else {
        ranges.push(range);
    }
    *previous = Some(slot);
}

// Strip only topology below the solid roots from the model used for placements. This is not
// a geometry read: shells are empty, and no surface, curve or B-spline enters step-io.
fn placement_model(
    graph: &BTreeMap<u64, RawEntity>,
) -> Result<(m::StepModel, step_io::parser::Graph, step_io::Report), StepError> {
    let mut b = PRELUDE.to_vec();
    for (&id, e) in graph {
        let n = name(e);
        if matches!(
            n,
            "ADVANCED_FACE"
                | "FACE_SURFACE"
                | "FACE_BOUND"
                | "FACE_OUTER_BOUND"
                | "EDGE_LOOP"
                | "ORIENTED_EDGE"
        ) {
            continue;
        }
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
struct Traversal<'a, 'b> {
    graph: &'a BTreeMap<u64, RawEntity>,
    ids: &'a mut FileIds<'b>,
    products: Vec<Product>,
    instances: Vec<Instance>,
    visited: usize,
    limit: usize,
    to_mm: f64,
    assemblies: &'a [Assembly],
}
impl Traversal<'_, '_> {
    fn visit(
        &mut self,
        def: step_io::scene::ProductDef<'_>,
        placement: Placement,
        path: &mut Vec<u64>,
    ) -> Result<(), StepError> {
        if path.len() > limits::ASSEMBLY_DEPTH {
            return Err(StepError::Limit(
                "manifest assembly depth (or cycle)".into(),
            ));
        }
        self.visited += 1;
        if self.visited > self.limit {
            return Err(StepError::Limit(format!(
                "manifest instance limit {}",
                self.limit
            )));
        }
        let pd = self.ids.id(def.key())?;
        let mut solids = def
            .solids()
            .map(|s| key_id(self.ids, s.key()))
            .collect::<Result<Vec<_>, _>>()?;
        let mut surfaces = def
            .surface_models()
            .map(|s| key_id(self.ids, s.key()))
            .collect::<Result<Vec<_>, _>>()?;
        surfaces.sort_unstable();
        solids.extend(surfaces);
        if !solids.is_empty() {
            let p = if let Some(p) = self
                .products
                .iter()
                .position(|p| p.product_definition == pd)
            {
                p
            } else {
                let shape = self
                    .graph
                    .iter()
                    .filter_map(|(&id, e)| {
                        (name(e) == "PRODUCT_DEFINITION_SHAPE"
                            && reference(attributes(e).get(2)) == Some(pd))
                        .then_some(id)
                    })
                    .filter(|&id| {
                        self.graph.values().any(|e| {
                            name(e) == "SHAPE_DEFINITION_REPRESENTATION"
                                && reference(attributes(e).first()) == Some(id)
                        })
                    })
                    .collect::<Vec<_>>();
                let [shape] = shape.as_slice() else {
                    return Err(failure(format!(
                        "part #{pd}: not exactly one represented shape"
                    )));
                };
                let (faces, edges) = topology_lists(self.graph, &solids)?;
                let mut name = super::product_name(self.graph, pd);
                if let Some(a) = self.assemblies.iter().find(|a| {
                    matches!(a.components[..],[c] if c.definition==pd) && !a.name.is_empty()
                }) {
                    name = a.name.clone();
                }
                let p = self.products.len();
                self.products.push(Product {
                    id: format!("p{p}"),
                    product_definition: pd,
                    shape: *shape,
                    name,
                    product_id: super::product_id(self.graph, pd),
                    ranges: Vec::new(),
                    entities: 0,
                    bytes: 0,
                    sha256: None,
                    faces,
                    edges,
                });
                p
            };
            for _ in solids {
                if self.instances.len() >= self.limit {
                    return Err(StepError::Limit(format!(
                        "manifest instance limit {}",
                        self.limit
                    )));
                }
                self.instances.push(Instance {
                    product: self.products[p].id.clone(),
                    path: path.clone(),
                    placement,
                });
            }
        }
        for occurrence in def.occurrences() {
            let Some(child) = occurrence.definition() else {
                continue;
            };
            let local = occurrence
                .transform()
                .and_then(|t| t.matrix())
                .map_or(IDENTITY, |m| {
                    [0, 1, 2].map(|i| [m[i][0], m[i][1], m[i][2], m[i][3] * self.to_mm])
                });
            path.push(key_id(self.ids, occurrence.key())?);
            self.visit(child, compose(&placement, &local), path)?;
            path.pop();
        }
        Ok(())
    }
}
fn topology_lists(
    graph: &BTreeMap<u64, RawEntity>,
    solids: &[u64],
) -> Result<(Vec<u64>, Vec<u64>), StepError> {
    let get = |id| {
        graph
            .get(&id)
            .ok_or_else(|| failure(format!("missing topology #{id}")))
    };
    let mut shells = Vec::new();
    for &id in solids {
        let e = get(id)?;
        let a = attributes(e);
        match name(e) {
            "SHELL_BASED_SURFACE_MODEL" => shells.extend(list(a.get(1))),
            _ => {
                if let Some(shell) = reference(a.get(1)) {
                    shells.push(shell);
                }
                for oriented in list(a.get(2)) {
                    if let Some(shell) = reference(attributes(get(oriented)?).get(2)) {
                        shells.push(shell);
                    }
                }
            }
        }
    }
    let mut faces = Vec::new();
    let mut edges = Vec::new();
    let mut seen = HashSet::new();
    for shell in shells {
        for face in list(attributes(get(shell)?).get(1)) {
            let a = attributes(get(face)?);
            faces.push(face);
            let same = logical(a.get(3))
                .ok_or_else(|| failure(format!("face #{face} has no same_sense")))?;
            for bound in list(a.get(1)) {
                let b = attributes(get(bound)?);
                let Some(loop_id) = reference(b.get(1)) else {
                    return Err(dangling());
                };
                let Some(e) = graph.get(&loop_id) else {
                    continue;
                }; // VERTEX_LOOP has no edges.
                let mut oriented = list(attributes(e).get(1));
                if logical(b.get(2)).ok_or_else(dangling)? != same {
                    oriented.reverse();
                }
                for oe in oriented {
                    if let Some(edge) = reference(attributes(get(oe)?).get(3))
                        && seen.insert(edge)
                    {
                        edges.push(edge);
                    }
                }
            }
        }
    }
    Ok((faces, edges))
}

impl Manifest {
    fn check_version(&self) -> Result<(), StepError> {
        if self.format != FORMAT || self.reader != ReaderVersion::default() {
            return Err(failure(
                "manifest format or reader version differs; rescan the source",
            ));
        }
        Ok(())
    }
    fn hash_ranges<R: Read + Seek>(&mut self, source: &mut R) -> Result<(), StepError> {
        let mut events = Vec::new();
        for r in &self.common.ranges {
            events.push((r[0], 0, true));
            events.push((r[1], 0, false));
        }
        for (i, p) in self.products.iter().enumerate() {
            for r in &p.ranges {
                events.push((r[0], i + 1, true));
                events.push((r[1], i + 1, false));
            }
        }
        events.sort_unstable();
        let mut hashes = vec![Sha256::new(); self.products.len() + 1];
        let mut active: HashSet<usize> = HashSet::new();
        let mut previous = 0;
        let mut buffer = vec![0; CHUNK];
        for (at, group, start) in events {
            if at > previous && !active.is_empty() {
                source.seek(SeekFrom::Start(previous)).map_err(io)?;
                let mut left = at - previous;
                while left > 0 {
                    let n = buffer.len().min(left as usize);
                    source.read_exact(&mut buffer[..n]).map_err(io)?;
                    for &i in &active {
                        hashes[i].update(&buffer[..n]);
                    }
                    left -= n as u64;
                }
            }
            if start {
                active.insert(group);
            } else {
                active.remove(&group);
            }
            previous = at;
        }
        let mut hashes = hashes.into_iter();
        self.common.sha256 = hashes.next().map(digest);
        for (p, h) in self.products.iter_mut().zip(hashes) {
            p.sha256 = Some(digest(h));
        }
        Ok(())
    }
    /// One forward pass over the source's header and the selected common/product ranges.
    /// Hashes are checked before geometry is parsed. Per-read geometry limits remain those
    /// of `read_part_definitions` and `read_part`; manifest instance limits do not change them.
    pub fn read_product(&self, file: &File, id: &str) -> Result<Part, StepError> {
        self.check_version()?;
        let p = self
            .products
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| failure(format!("unknown product {id}")))?;
        let refuse = |why: String| {
            failure(format!(
                "product {} (#{} {}): {why}",
                p.id, p.product_definition, p.name
            ))
        };
        if self.source.compressed {
            return Err(refuse("compressed source cannot be read by ranges; supply an uncompressed source and rescan".into()));
        }
        if let Some(r) = self.refusals.iter().find(|r| r.products.contains(&p.id)) {
            return Err(refuse(r.reason.clone()));
        }
        if file.metadata().map_err(io)?.len() != self.source.size {
            return Err(refuse("source size mismatch".into()));
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
        let defs = read_part_definitions(&bytes).map_err(|e| refuse(e.to_string()))?;
        let def = defs
            .iter()
            .find(|d| d.product_definition == p.product_definition)
            .ok_or_else(|| refuse("definition missing from extracted document".into()))?;
        if def.faces != p.faces || def.edges != p.edges {
            return Err(refuse(
                "face or edge entity order differs from manifest".into(),
            ));
        }
        read_part(&bytes, def).map_err(|e| refuse(e.to_string()))
    }
    /// Full uncompressed source size and SHA-256 verification. Unlike product checks this
    /// catches changes in assembly, presentation, comments and other unowned bytes too.
    pub fn verify_source<R: Read + Seek>(&self, reader: R) -> Result<(), StepError> {
        self.check_version()?;
        let mut source = Replay::new(reader)?;
        let mut hash = Sha256::new();
        let mut size = 0;
        let mut b = vec![0; CHUNK];
        loop {
            let n = source.read(&mut b).map_err(io)?;
            if n == 0 {
                break;
            }
            hash.update(&b[..n]);
            size += n as u64;
        }
        if size != self.source.size || digest(hash) != self.source.sha256 {
            return Err(failure("manifest source size or sha256 mismatch"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn index_retains_structure_without_entity_text_and_handles_sparse_ids() {
        let mut index = Index::new(&ScanOptions::default()).unwrap();
        let text=b"ISO-10303-21;DATA;#1=THING('original entity text',#18446744073709551615);#18446744073709551615=THING('other text');ENDSEC;END-ISO-10303-21;";
        streaming::scan(text.as_slice(), 1, |e| {
            if let streaming::Event::Instance(i, _) = e {
                index.add(&i).unwrap();
            }
            Ok(())
        })
        .unwrap();
        let Storage::Memory(entries) = &index.entries else {
            panic!("memory index")
        };
        assert_eq!(entries.len(), 2);
        assert_eq!(std::mem::size_of_val(&entries[0]), 16);
        assert!(
            !entries
                .iter()
                .flat_map(|e| e.iter())
                .copied()
                .collect::<Vec<_>>()
                .windows(b"original entity text".len())
                .any(|w| w == b"original entity text")
        );
        assert!(index.pending.is_empty());
        assert_eq!(index.root(0), index.root(1));
        assert_eq!(index.ids, [1, u64::MAX]);
    }
    #[test]
    fn mapped_growth_and_cleanup_on_refusal() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut index = Index::new(&ScanOptions {
                index_dir: Some(dir.path().into()),
                ..ScanOptions::default()
            })
            .unwrap();
            for id in 0..5000 {
                index
                    .add(&streaming::Instance {
                        id,
                        offset: id * 17,
                        length: 16,
                        gap_start: 0,
                        types: vec!["POINT".into()],
                        references: Vec::new(),
                    })
                    .unwrap();
            }
            assert_eq!(index.range(4999), [4999 * 17, 4999 * 17 + 16]);
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        assert!(
            scan(
                std::io::Cursor::new(b"bad STEP"),
                &ScanOptions {
                    index_dir: Some(dir.path().into()),
                    ..ScanOptions::default()
                }
            )
            .is_err()
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
