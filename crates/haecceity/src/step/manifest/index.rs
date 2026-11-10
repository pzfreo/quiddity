//! Compact extent/union index. Dense IDs use a u32 slot array, with a hash table only for
//! outlying/sparse IDs. Unknown targets hold their first waiting union inline in the unused
//! extent fields; additional waiting links live in one recycled flat arena, never Vecs/maps
//! per entity. An unknown context/frame is therefore never accidentally unioned as geometry.
use super::*;
const PRESENT: u8 = 64;
const ADJACENT: u8 = 128;
const OFFSET_MASK: u64 = (1u64 << 56) - 1;
const NONE: u32 = u32::MAX;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Kind {
    Unknown,
    Geometry,
    Context,
    Assembly,
    Presentation,
    Frame,
    Point,
    Direction,
    Representation,
    Solid,
    Voids,
    SurfaceModel,
    Shell,
    OrientedShell,
    Protocol,
    Relation,
    Edge,
    Topology,
}
impl Kind {
    pub fn geometry(self) -> bool {
        matches!(
            self,
            Self::Geometry
                | Self::Point
                | Self::Direction
                | Self::Representation
                | Self::Solid
                | Self::Voids
                | Self::SurfaceModel
                | Self::Shell
                | Self::OrientedShell
                | Self::Relation
                | Self::Edge
                | Self::Topology
        )
    }
    pub fn retained(self) -> bool {
        matches!(
            self,
            Self::Assembly
                | Self::Representation
                | Self::Solid
                | Self::Voids
                | Self::SurfaceModel
                | Self::Shell
                | Self::OrientedShell
                | Self::Relation
        )
    }
    pub fn context(self) -> bool {
        matches!(self, Self::Context | Self::Protocol)
    }
    pub fn classify(types: streaming::Types<'_>) -> Self {
        let mut result = Self::Geometry;
        let mut relationship = false;
        let mut transform = false;
        for raw in types.iter() {
            let mut folded = [0u8; 128];
            let n = if raw.len() <= folded.len() {
                for (to, from) in folded.iter_mut().zip(raw.bytes()) {
                    *to = from.to_ascii_uppercase();
                }
                std::str::from_utf8(&folded[..raw.len()]).unwrap_or_default()
            } else {
                raw
            };
            let kind = match n {
                "APPLICATION_PROTOCOL_DEFINITION" => Self::Protocol,
                "AXIS2_PLACEMENT_3D" | "AXIS2_PLACEMENT_2D" => Self::Frame,
                "EDGE_CURVE" => Self::Edge,
                "ADVANCED_FACE" | "FACE_SURFACE" | "FACE_BOUND" | "FACE_OUTER_BOUND"
                | "EDGE_LOOP" | "ORIENTED_EDGE" => Self::Topology,
                "CARTESIAN_POINT" => Self::Point,
                "DIRECTION" => Self::Direction,
                "MANIFOLD_SOLID_BREP" | "FACETED_BREP" => Self::Solid,
                "BREP_WITH_VOIDS" => Self::Voids,
                "SHELL_BASED_SURFACE_MODEL" => Self::SurfaceModel,
                "CLOSED_SHELL" | "OPEN_SHELL" => Self::Shell,
                "ORIENTED_CLOSED_SHELL" => Self::OrientedShell,
                "SHAPE_REPRESENTATION_RELATIONSHIP" => {
                    relationship = true;
                    Self::Geometry
                }
                "REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION" => {
                    transform = true;
                    Self::Assembly
                }
                _ if is_common(n) => Self::Context,
                _ if is_assembly(n) => Self::Assembly,
                _ if is_presentation(n) => Self::Presentation,
                _ if n.ends_with("SHAPE_REPRESENTATION") => Self::Representation,
                _ => Self::Geometry,
            };
            // External mapping inherited leaves may occur in any order. Common, assembly
            // and presentation take precedence over the ordinary geometry inheritance leaves.
            let priority = |k| match k {
                Self::Context | Self::Protocol => 4,
                Self::Assembly | Self::Frame => 3,
                Self::Presentation => 2,
                Self::Geometry => 0,
                _ => 1,
            };
            if priority(kind) > priority(result) {
                result = kind;
            }
        }
        if relationship && !transform && !result.context() {
            Self::Relation
        } else {
            result
        }
    }
}
#[derive(Clone, Copy, Default)]
#[repr(C)]
struct Entry {
    offset: u64,
    length: u32,
    parent: u32,
}
impl Entry {
    fn flags(self) -> u8 {
        (self.offset >> 56) as u8
    }
    fn present(self) -> bool {
        self.flags() & PRESENT != 0
    }
    fn kind(self) -> Kind {
        match self.flags() & 63 {
            1 => Kind::Geometry,
            2 => Kind::Context,
            3 => Kind::Assembly,
            4 => Kind::Presentation,
            5 => Kind::Frame,
            6 => Kind::Point,
            7 => Kind::Direction,
            8 => Kind::Representation,
            9 => Kind::Solid,
            10 => Kind::Voids,
            11 => Kind::SurfaceModel,
            12 => Kind::Shell,
            13 => Kind::OrientedShell,
            14 => Kind::Protocol,
            15 => Kind::Relation,
            16 => Kind::Edge,
            17 => Kind::Topology,
            _ => Kind::Unknown,
        }
    }
    fn bytes(self) -> [u8; 16] {
        let mut b = [0; 16];
        b[..8].copy_from_slice(&self.offset.to_le_bytes());
        b[8..12].copy_from_slice(&self.length.to_le_bytes());
        b[12..].copy_from_slice(&self.parent.to_le_bytes());
        b
    }
    fn read(b: &[u8]) -> Self {
        let mut a = [0; 8];
        a.copy_from_slice(&b[..8]);
        let mut l = [0; 4];
        l.copy_from_slice(&b[8..12]);
        let mut p = [0; 4];
        p.copy_from_slice(&b[12..16]);
        Self {
            offset: u64::from_le_bytes(a),
            length: u32::from_le_bytes(l),
            parent: u32::from_le_bytes(p),
        }
    }
}
enum Storage {
    Memory {
        pages: Vec<Box<[Entry]>>,
        len: usize,
    },
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
            file.as_file().set_len(65536).map_err(io)?;
            // SAFETY: the private temporary file is owned here, never shrunk, and no reference
            // into a mapping survives growth. The mapping drops before the file is unlinked.
            let map = unsafe { memmap2::MmapMut::map_mut(file.as_file()) }.map_err(io)?;
            Ok(Self::Mapped { map, file, len: 0 })
        } else {
            Ok(Self::Memory {
                pages: Vec::new(),
                len: 0,
            })
        }
    }
    fn len(&self) -> usize {
        match self {
            Self::Memory { len, .. } => *len,
            Self::Mapped { len, .. } => *len,
        }
    }
    fn push(&mut self, e: Entry) -> Result<(), StepError> {
        match self {
            Self::Memory { pages, len } => {
                if *len % 4096 == 0 {
                    pages.push(vec![Entry::default(); 4096].into_boxed_slice());
                }
                pages[*len / 4096][*len % 4096] = e;
                *len += 1;
            }
            Self::Mapped { map, file, len } => {
                if (*len + 1) * 16 > map.len() {
                    file.as_file().set_len((map.len() * 2) as u64).map_err(io)?;
                    // SAFETY: only growth of the same privately owned file, as above.
                    *map = unsafe { memmap2::MmapMut::map_mut(file.as_file()) }.map_err(io)?;
                }
                map[*len * 16..(*len + 1) * 16].copy_from_slice(&e.bytes());
                *len += 1;
            }
        }
        Ok(())
    }
    fn get(&self, i: u32) -> Entry {
        match self {
            Self::Memory { pages, .. } => pages[i as usize / 4096][i as usize % 4096],
            Self::Mapped { map, .. } => Entry::read(&map[i as usize * 16..i as usize * 16 + 16]),
        }
    }
    fn set(&mut self, i: u32, e: Entry) {
        match self {
            Self::Memory { pages, .. } => pages[i as usize / 4096][i as usize % 4096] = e,
            Self::Mapped { map, .. } => {
                map[i as usize * 16..i as usize * 16 + 16].copy_from_slice(&e.bytes())
            }
        }
    }
}
struct Dense {
    pages: Vec<Box<[u32]>>,
}
impl Dense {
    fn len(&self) -> usize {
        self.pages.len() * 4096
    }
    fn get(&self, id: usize) -> Option<u32> {
        self.pages.get(id / 4096).map(|p| p[id % 4096])
    }
    fn set(&mut self, id: usize, slot: u32) {
        self.pages[id / 4096][id % 4096] = slot;
    }
    fn grow(&mut self, needed: usize) {
        self.pages.resize_with(needed.div_ceil(4096), || {
            vec![NONE; 4096].into_boxed_slice()
        });
    }
}
struct Link {
    source: u32,
    next: u32,
}
pub(super) struct Index {
    entries: Storage,
    dense: Dense,
    sparse: HashMap<u64, u32>,
    order: Option<Vec<u32>>,
    count: usize,
    last_id: u64,
    last_slot: Option<u32>,
    links: Vec<Link>,
    free: u32,
    pub boundary: Vec<(u32, u32)>,
}
pub(super) enum Slots<'a> {
    Explicit(std::iter::Copied<std::slice::Iter<'a, u32>>),
    Dense { index: &'a Index, at: usize },
}
impl Iterator for Slots<'_> {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        match self {
            Self::Explicit(i) => i.next(),
            Self::Dense { index, at } => {
                while *at < index.dense.len() {
                    let i = *at;
                    *at += 1;
                    if let Some(slot) = index.dense.get(i)
                        && slot != NONE
                    {
                        return Some(slot);
                    }
                }
                None
            }
        }
    }
}
impl Index {
    pub fn new(options: &ScanOptions) -> Result<Self, StepError> {
        Ok(Self {
            entries: Storage::new(options.index_dir.as_ref())?,
            dense: Dense { pages: Vec::new() },
            sparse: HashMap::new(),
            order: None,
            count: 0,
            last_id: 0,
            last_slot: None,
            links: Vec::new(),
            free: NONE,
            boundary: Vec::new(),
        })
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn file_slots(&self) -> Slots<'_> {
        if let Some(order) = &self.order {
            Slots::Explicit(order.iter().copied())
        } else {
            Slots::Dense { index: self, at: 0 }
        }
    }
    pub fn root_read(&self, slot: u32) -> u32 {
        self.entries.get(slot).parent
    }
    pub fn kind(&self, i: u32) -> Kind {
        self.entries.get(i).kind()
    }
    pub fn present(&self, i: u32) -> bool {
        self.entries.get(i).present()
    }
    pub fn adjacent(&self, i: u32) -> bool {
        self.entries.get(i).flags() & ADJACENT != 0
    }
    pub fn range(&self, i: u32) -> ByteRange {
        let e = self.entries.get(i);
        let start = e.offset & OFFSET_MASK;
        [start, start + u64::from(e.length)]
    }
    pub fn lookup(&self, id: u64) -> Option<u32> {
        usize::try_from(id)
            .ok()
            .and_then(|id| self.dense.get(id))
            .filter(|&s| s != NONE)
            .or_else(|| self.sparse.get(&id).copied())
    }
    pub fn id(&self, slot: u32) -> Option<u64> {
        (0..self.dense.len())
            .find(|&id| self.dense.get(id) == Some(slot))
            .map(|i| i as u64)
            .or_else(|| {
                self.sparse
                    .iter()
                    .find_map(|(&id, &s)| (s == slot).then_some(id))
            })
    }
    fn slot(&mut self, id: u64) -> Result<u32, StepError> {
        if let Some(slot) = self.lookup(id) {
            return Ok(slot);
        }
        let budget = self.entries.len().saturating_mul(2).saturating_add(4096);
        if let Ok(id) = usize::try_from(id)
            && id < budget
            && id >= self.dense.len()
        {
            let old_len = self.dense.len();
            self.dense.grow(id + 1);
            if !self.sparse.is_empty() {
                for id in old_len..self.dense.len() {
                    if let Some(slot) = self.sparse.remove(&(id as u64)) {
                        self.dense.set(id, slot);
                    }
                }
            }
            if self.sparse.is_empty() {
                self.sparse = HashMap::new();
            }
        }
        if self.len() >= u32::MAX as usize {
            return Err(StepError::Limit("manifest index exceeds u32 slots".into()));
        }
        let slot = u32::try_from(self.len())
            .map_err(|_| StepError::Limit("manifest index exceeds u32 slots".into()))?;
        self.entries.push(Entry::default())?;
        if id < self.dense.len() as u64 {
            self.dense.set(id as usize, slot);
        } else {
            self.sparse.insert(id, slot);
        }
        Ok(slot)
    }
    pub fn root(&mut self, mut i: u32) -> u32 {
        while self.entries.get(i).parent != i {
            let p = self.entries.get(i).parent;
            let root = self.entries.get(p).parent;
            let mut e = self.entries.get(i);
            e.parent = root;
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
            e.parent = b;
            self.entries.set(a, e);
        }
    }
    fn waiting(&mut self, target: u32, source: u32) {
        let mut e = self.entries.get(target);
        if e.length == 0 {
            e.length = source + 1;
        } else {
            let link = Link {
                source,
                next: e.parent,
            };
            let head = if self.free == NONE {
                self.links.push(link);
                self.links.len() as u32
            } else {
                let i = self.free;
                self.free = self.links[i as usize].next;
                self.links[i as usize] = link;
                i + 1
            };
            e.parent = head;
        }
        self.entries.set(target, e);
    }
    fn connect(&mut self, source: u32, target: u32) {
        let k = self.kind(target);
        if k.geometry() {
            self.union(source, target);
        } else if k != Kind::Presentation {
            self.boundary.push((source, target));
        }
    }
    pub fn add(&mut self, i: streaming::Instance<'_>) -> Result<u32, StepError> {
        if i.offset > OFFSET_MASK {
            return Err(StepError::Limit(
                "manifest byte offsets exceed 56 bits".into(),
            ));
        }
        let slot = self.slot(i.id)?;
        let old = self.entries.get(slot);
        if old.present() {
            return Err(failure(format!("duplicate entity #{}", i.id)));
        }
        let kind = Kind::classify(i.types);
        let adjacent = self
            .last_slot
            .is_some_and(|p| self.range(p)[1] == i.gap_start);
        if self.order.is_none() && self.count > 0 && i.id < self.last_id {
            let mut previous: Vec<_> = (0..self.len() as u32)
                .filter(|&s| self.present(s))
                .collect();
            previous.sort_unstable_by_key(|&s| self.range(s)[0]);
            self.order = Some(previous);
        }
        let flags = kind as u8 | PRESENT | if adjacent { ADJACENT } else { 0 };
        self.entries.set(
            slot,
            Entry {
                offset: i.offset | (u64::from(flags) << 56),
                length: i.length,
                parent: slot,
            },
        );
        if let Some(order) = &mut self.order {
            order.push(slot);
        }
        self.count += 1;
        self.last_id = i.id;
        self.last_slot = Some(slot);
        if old.length != 0 {
            self.connect(old.length - 1, slot);
        }
        let mut link = old.parent;
        while link != 0 {
            let n = link - 1;
            let source = self.links[n as usize].source;
            link = self.links[n as usize].next;
            self.links[n as usize].next = self.free;
            self.free = n;
            self.connect(source, slot);
        }
        if kind.geometry() {
            for &id in i.references {
                let target = self.slot(id)?;
                // Preserve only topology links for the face-reachable edge count. Ordinary
                // geometry unions can include unused edges sharing a point or curve.
                if kind == Kind::Topology {
                    self.boundary.push((slot, target));
                }
                if self.present(target) {
                    self.connect(slot, target);
                } else {
                    self.waiting(target, slot);
                }
            }
        } else if kind != Kind::Presentation {
            for &id in i.references {
                let target = self.slot(id)?;
                self.boundary.push((slot, target));
            }
        }
        Ok(slot)
    }
    pub fn finish(&mut self) -> Result<(), StepError> {
        // Unknown references in unowned metadata must not silently become geometry. They are
        // rejected here, as missing dependencies cannot be copied into a product document.
        if self.count != self.len() {
            let missing = (0..self.len() as u32).find(|&s| !self.present(s));
            return Err(failure(format!(
                "missing referenced entity #{}",
                missing.and_then(|s| self.id(s)).unwrap_or_default()
            )));
        }
        self.links = Vec::new();
        self.free = NONE;
        if self.sparse.is_empty() {
            self.sparse = HashMap::new();
        }
        if self.order.is_none() && !self.sparse.is_empty() {
            let mut order: Vec<_> = (0..self.len() as u32).collect();
            order.sort_unstable_by_key(|&s| self.range(s)[0]);
            self.order = Some(order);
        }
        for slot in 0..self.len() as u32 {
            if self.kind(slot).geometry() {
                let root = self.root(slot);
                let mut e = self.entries.get(slot);
                e.parent = root;
                self.entries.set(slot, e);
            }
        }
        self.boundary.sort_unstable();
        self.boundary.dedup();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn index_retains_only_compact_structure_with_dense_or_sparse_ids() {
        let text=b"ISO-10303-21;DATA;#1=THING('original entity text',#18446744073709551615);#18446744073709551615=THING('other text');ENDSEC;END-ISO-10303-21;";
        let mut index = Index::new(&ScanOptions::default()).unwrap();
        streaming::scan_structure(text.as_slice(), 1, |e| {
            if let streaming::Event::Instance(i, _) = e {
                index.add(i).unwrap();
            }
            Ok(())
        })
        .unwrap();
        index.finish().unwrap();
        assert_eq!(std::mem::size_of::<Entry>(), 16);
        assert_eq!(index.len(), 2);
        assert_eq!(
            index.root(index.lookup(1).unwrap()),
            index.root(index.lookup(u64::MAX).unwrap())
        );
        assert!(index.links.is_empty());
        assert!(index.sparse.contains_key(&u64::MAX));
        let bytes: Vec<_> = (0..index.len() as u32)
            .flat_map(|s| index.entries.get(s).bytes())
            .collect();
        assert!(
            !bytes
                .windows(b"original entity text".len())
                .any(|w| w == b"original entity text")
        );
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
            let mut text = b"ISO-10303-21;DATA;".to_vec();
            for i in 1..5001 {
                text.extend_from_slice(format!("#{i}=THING();").as_bytes());
            }
            text.extend_from_slice(b"ENDSEC;END-ISO-10303-21;");
            streaming::scan_structure(text.as_slice(), 7, |e| {
                if let streaming::Event::Instance(i, _) = e {
                    index.add(i).unwrap();
                }
                Ok(())
            })
            .unwrap();
            index.finish().unwrap();
            assert_eq!(index.len(), 5000);
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
            assert!(index.order.is_none());
            assert!(index.sparse.is_empty());
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
    #[test]
    fn dense_forward_references_have_no_per_entity_heap_objects() {
        let mut text = b"ISO-10303-21;DATA;".to_vec();
        for i in 1..10001 {
            text.extend_from_slice(format!("#{i}=THING(#{},#10001);", i + 1).as_bytes());
        }
        text.extend_from_slice(b"#10001=THING();ENDSEC;END-ISO-10303-21;");
        let mut index = Index::new(&ScanOptions::default()).unwrap();
        streaming::scan_structure(text.as_slice(), 4096, |e| {
            if let streaming::Event::Instance(i, _) = e {
                index.add(i).unwrap();
            }
            Ok(())
        })
        .unwrap();
        index.finish().unwrap();
        assert!(index.sparse.is_empty());
        assert!(index.order.is_none());
        assert!(index.links.is_empty());
        assert!(index.boundary.is_empty());
        let structural = index.len() * 16 + index.dense.len() * 4;
        assert!(
            structural <= index.len() * 24 + 4096 * 4,
            "dense extent/union entries and paged ID lookup only"
        );
    }
}

#[cfg(test)]
mod external_budget_tests {
    use super::*;
    #[test]
    fn measured_dense_files_have_compact_state_below_32_bytes_per_entity() {
        let Some(paths) = std::env::var_os("HAECCEITY_MANIFEST_MEMORY_INPUTS") else {
            return;
        };
        for path in std::env::split_paths(&paths) {
            let mut index = Index::new(&ScanOptions::default()).unwrap();
            streaming::scan_structure(File::open(&path).unwrap(), CHUNK, |event| {
                if let streaming::Event::Instance(i, _) = event {
                    index.add(i).unwrap();
                }
                Ok(())
            })
            .unwrap();
            let links = index.links.capacity() * std::mem::size_of::<Link>();
            index.finish().unwrap();
            assert!(index.sparse.is_empty());
            assert!(index.order.is_none());
            let entries = match &index.entries {
                Storage::Memory { pages, .. } => pages.len() * 4096 * 16,
                _ => unreachable!(),
            };
            let arrays = entries + index.dense.len() * 4;
            let boundaries = index.boundary.capacity() * 8;
            // Two phase peaks: first-pass growth (including a boundary reallocation and
            // recycled forward links), or ownership labels, bit queue and a conservative
            // queue slot reservation. Assembly/topology records and fixed I/O buffers are
            // intentionally outside this per-entity index budget.
            let scan_peak = arrays + boundaries + boundaries / 2 + links;
            let own_peak = arrays
                + boundaries
                + index.len() * 4
                + index.len().div_ceil(64) * 8
                + index.boundary.len().next_power_of_two() * 4;
            let peak = scan_peak.max(own_peak);
            eprintln!(
                "{}: {} entities, compact-state upper bound {} bytes ({:.2} B/entity)",
                path.display(),
                index.len(),
                peak,
                peak as f64 / index.len() as f64
            );
            assert!(
                peak <= index.len() * 32 + 65536,
                "{}: {peak} bytes for {} entities",
                path.display(),
                index.len()
            );
        }
    }
}
