use super::*;
use std::collections::VecDeque;
const NONE: u32 = 0;
const COMMON: u32 = 1;
const MULTI: u32 = 1 << 31;

// Private entities use an inline product tag. Only genuinely shared ownership uses the
// pooled links and membership table, so adding thousands of products to a standard frame
// neither allocates a Vec per entity nor materializes quadratic prefix sets.
struct Group {
    head: u32,
    len: u32,
    sealed: bool,
}
struct Member {
    product: u32,
    next: u32,
}
#[derive(Default)]
struct Owners {
    labels: Vec<u32>,
    groups: Vec<Group>,
    links: Vec<Member>,
    membership: HashSet<(u32, u32)>,
}
impl Owners {
    fn new(n: usize) -> Self {
        Self {
            labels: vec![0; n],
            ..Self::default()
        }
    }
    fn product(p: usize) -> u32 {
        p as u32 + 2
    }
    fn each(&self, tag: u32, mut f: impl FnMut(usize)) {
        if tag & MULTI == 0 {
            if tag >= 2 {
                f((tag - 2) as usize);
            }
            return;
        }
        let mut link = self.groups[(tag & !MULTI) as usize].head;
        while link != u32::MAX {
            let m = &self.links[link as usize];
            f(m.product as usize);
            link = m.next;
        }
    }
    fn members(&self, tag: u32) -> Vec<usize> {
        let mut out = Vec::new();
        self.each(tag, |p| out.push(p));
        out.sort_unstable();
        out
    }
    fn insert(&mut self, group: u32, p: u32) {
        if self.membership.insert((group, p)) {
            let g = &mut self.groups[group as usize];
            let link = self.links.len() as u32;
            self.links.push(Member {
                product: p,
                next: g.head,
            });
            g.head = link;
            g.len += 1;
        }
    }
    fn new_group(&mut self, tag: u32) -> u32 {
        let g = self.groups.len() as u32;
        self.groups.push(Group {
            head: u32::MAX,
            len: 0,
            sealed: false,
        });
        if tag & MULTI == 0 {
            if tag >= 2 {
                self.insert(g, tag - 2);
            }
        } else {
            let mut link = self.groups[(tag & !MULTI) as usize].head;
            while link != u32::MAX {
                let m = &self.links[link as usize];
                let (p, next) = (m.product, m.next);
                self.insert(g, p);
                link = next;
            }
        }
        g
    }
    fn combine(&mut self, a: u32, b: u32) -> u32 {
        if a == COMMON || b == COMMON {
            return COMMON;
        }
        if a == b || b == NONE {
            return a;
        }
        if a == NONE {
            return if b & MULTI == 0 {
                b
            } else {
                MULTI | self.new_group(b)
            };
        }
        let mut target = if a & MULTI == 0 {
            None
        } else {
            Some(a & !MULTI)
        };
        let mut link = if b & MULTI != 0 {
            self.groups[(b & !MULTI) as usize].head
        } else {
            u32::MAX
        };
        let mut single = (b & MULTI == 0).then_some(b - 2);
        loop {
            let p = if let Some(p) = single.take() {
                p
            } else if link != u32::MAX {
                let m = &self.links[link as usize];
                link = m.next;
                m.product
            } else {
                break;
            };
            if target.is_some_and(|g| self.membership.contains(&(g, p)))
                || (target.is_none() && a - 2 == p)
            {
                continue;
            }
            let g = match target {
                Some(g) if !self.groups[g as usize].sealed => g,
                _ => self.new_group(a),
            };
            target = Some(g);
            self.insert(g, p);
        }
        target.map_or(a, |g| MULTI | g)
    }
    fn add(&mut self, slot: u32, tag: u32) -> bool {
        let old = self.labels[slot as usize];
        let len = if old & MULTI != 0 {
            self.groups[(old & !MULTI) as usize].len
        } else {
            0
        };
        let new = self.combine(old, tag);
        self.labels[slot as usize] = new;
        old != new || (new & MULTI != 0 && self.groups[(new & !MULTI) as usize].len != len)
    }
}
struct Queue {
    nodes: VecDeque<u32>,
    queued: Vec<u64>,
}
impl Queue {
    fn new(n: usize) -> Self {
        Self {
            nodes: VecDeque::new(),
            queued: vec![0; n.div_ceil(64)],
        }
    }
    fn push_back(&mut self, n: u32) {
        let word = &mut self.queued[n as usize / 64];
        let mask = 1u64 << (n % 64);
        if *word & mask == 0 {
            *word |= mask;
            self.nodes.push_back(n);
        }
    }
    fn pop_front(&mut self) -> Option<u32> {
        let n = self.nodes.pop_front()?;
        self.queued[n as usize / 64] &= !(1u64 << (n % 64));
        Some(n)
    }
}
fn outgoing(edges: &[(u32, u32)], slot: u32) -> &[(u32, u32)] {
    let a = edges.partition_point(|e| e.0 < slot);
    let b = edges.partition_point(|e| e.0 <= slot);
    &edges[a..b]
}
fn ids_of_kind(
    index: &Index,
    ids: impl IntoIterator<Item = u64>,
    accept: impl Fn(Kind) -> bool,
) -> Vec<u32> {
    ids.into_iter()
        .filter_map(|id| index.lookup(id))
        .filter(|&slot| accept(index.kind(slot)))
        .collect()
}

struct Plan {
    product: Product,
    sdrs: Vec<u64>,
    representations: Vec<u64>,
}
fn plans(
    graph: &BTreeMap<u64, RawEntity>,
    index: &Index,
) -> Result<(Vec<Plan>, HashSet<u64>), StepError> {
    let mut pd_to_pds: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    let mut pds_to_sdr: HashMap<u64, Vec<(u64, u64)>> = HashMap::new();
    let mut bridges: HashMap<u64, Vec<(u64, u64)>> = HashMap::new();
    for (&id, e) in graph {
        let a = attributes(e);
        match name(e) {
            "PRODUCT_DEFINITION_SHAPE" => {
                if let Some(pd) = reference(a.get(2)) {
                    pd_to_pds.entry(pd).or_default().push(id);
                }
            }
            "SHAPE_DEFINITION_REPRESENTATION" => {
                if let (Some(pds), Some(rep)) = (reference(a.first()), reference(a.get(1))) {
                    pds_to_sdr.entry(pds).or_default().push((id, rep));
                }
            }
            "SHAPE_REPRESENTATION_RELATIONSHIP" => {
                if let (Some(left), Some(right)) = (reference(a.get(2)), reference(a.get(3))) {
                    bridges.entry(left).or_default().push((id, right));
                    bridges.entry(right).or_default().push((id, left));
                }
            }
            _ => {}
        }
    }
    let mut result = Vec::new();
    let mut common_reps = HashSet::new();
    for (pd, shapes) in pd_to_pds {
        if !graph.get(&pd).is_some_and(|e| {
            matches!(
                name(e),
                "PRODUCT_DEFINITION" | "PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS"
            )
        }) {
            continue;
        }
        let represented: Vec<_> = shapes
            .into_iter()
            .filter(|s| pds_to_sdr.contains_key(s))
            .collect();
        let mut roots = Vec::new();
        let mut sdrs = Vec::new();
        for shape in &represented {
            if let Some(sdr) = pds_to_sdr.get(shape) {
                for &(id, rep) in sdr {
                    sdrs.push(id);
                    roots.push(rep);
                }
            }
        }
        let mut visited = HashSet::new();
        let mut items = Vec::new();
        let mut stack: Vec<_> = roots.iter().rev().copied().collect();
        while let Some(rep) = stack.pop() {
            if !visited.insert(rep) {
                continue;
            }
            if let Some(e) = graph.get(&rep) {
                items.extend(list(attributes(e).get(1)));
            }
            if let Some(next) = bridges.get(&rep) {
                stack.extend(next.iter().rev().map(|&(_, rep)| rep));
            }
        }
        common_reps.extend(visited);
        let mut seen = HashSet::new();
        let mut solids: Vec<_> = items
            .iter()
            .copied()
            .filter(|id| {
                index
                    .lookup(*id)
                    .is_some_and(|s| matches!(index.kind(s), Kind::Solid | Kind::Voids))
            })
            .filter(|id| seen.insert(*id))
            .collect();
        let mut surfaces: Vec<_> = items
            .into_iter()
            .filter(|id| {
                index
                    .lookup(*id)
                    .is_some_and(|s| index.kind(s) == Kind::SurfaceModel)
            })
            .collect();
        surfaces.sort_unstable();
        surfaces.dedup();
        solids.extend(surfaces);
        if solids.is_empty() {
            continue;
        }
        let [shape] = represented.as_slice() else {
            return Err(failure(format!(
                "part #{pd}: not exactly one represented product definition shape"
            )));
        };
        let faces = face_list(graph, &solids)?;
        let p = result.len();
        result.push(Plan {
            product: Product {
                id: format!("p{p}"),
                product_definition: pd,
                shape: *shape,
                name: super::super::product_name(graph, pd),
                product_id: super::super::product_id(graph, pd),
                ranges: Vec::new(),
                entities: 0,
                bytes: 0,
                sha256: None,
                faces,
                edge_count: 0,
            },
            sdrs,
            representations: roots,
        });
    }
    // CDSR relationships can use an occurrence representation which is not itself the
    // SDR of a product. Its context still belongs to the assembly chain, and must be
    // available to the unchanged placement reader. Ignore unrelated representations.
    let mut assembly_reps = Vec::new();
    for e in graph
        .values()
        .filter(|e| name(e) == "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION")
    {
        let a = attributes(e);
        let linked = reference(a.get(1))
            .and_then(|pds| graph.get(&pds))
            .and_then(|pds| reference(attributes(pds).get(2)))
            .and_then(|occurrence| graph.get(&occurrence))
            .is_some_and(|e| name(e) == "NEXT_ASSEMBLY_USAGE_OCCURRENCE");
        if linked && let Some(relation) = reference(a.first()) {
            assembly_reps.push(relation);
        }
    }
    let mut seen = HashSet::new();
    while let Some(id) = assembly_reps.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(slot) = index.lookup(id) else {
            continue;
        };
        if index.kind(slot) == Kind::Representation {
            common_reps.insert(id);
            if let Some(next) = bridges.get(&id) {
                assembly_reps.extend(next.iter().map(|&(_, rep)| rep));
            }
        } else if index.kind(slot) == Kind::Assembly
            && let Some(e) = graph.get(&id)
        {
            assembly_reps.extend(references(e).into_iter().filter(|id| {
                index.lookup(*id).is_some_and(|slot| {
                    matches!(index.kind(slot), Kind::Assembly | Kind::Representation)
                })
            }));
        }
    }
    Ok((result, common_reps))
}
fn face_list(graph: &BTreeMap<u64, RawEntity>, solids: &[u64]) -> Result<Vec<u64>, StepError> {
    let get = |id| {
        graph
            .get(&id)
            .ok_or_else(|| failure(format!("missing topology root #{id}")))
    };
    let mut shells = Vec::new();
    for &id in solids {
        let e = get(id)?;
        let a = attributes(e);
        if name(e) == "SHELL_BASED_SURFACE_MODEL" {
            shells.extend(list(a.get(1)));
        } else {
            if let Some(shell) = reference(a.get(1)) {
                shells.push(shell);
            }
            for void in list(a.get(2)) {
                if let Some(shell) = reference(attributes(get(void)?).get(2)) {
                    shells.push(shell);
                }
            }
        }
    }
    let mut faces = Vec::new();
    for shell in shells {
        faces.extend(list(attributes(get(shell)?).get(1)));
    }
    Ok(faces)
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
fn compressed<R: Read + Seek>(reader: &mut R) -> Result<bool, StepError> {
    reader.seek(SeekFrom::Start(0)).map_err(io)?;
    let mut b = [0; 2];
    let mut n = 0;
    while n < 2 {
        let k = reader.read(&mut b[n..]).map_err(io)?;
        if k == 0 {
            break;
        }
        n += k;
    }
    reader.seek(SeekFrom::Start(0)).map_err(io)?;
    Ok(n == 2 && b == [31, 139])
}
fn decoded<'a, R: Read + 'a>(reader: &'a mut R, gzip: bool) -> Box<dyn Read + 'a> {
    if gzip {
        Box::new(flate2::read::MultiGzDecoder::new(reader))
    } else {
        Box::new(reader)
    }
}

/// Two forward passes separated by one rewind. `Read + Seek` is needed only to replay the
/// original source; gzip is decoded exactly once per pass, with no backwards decoded seeks.
pub fn scan<R: Read + Seek>(mut reader: R, options: &ScanOptions) -> Result<Manifest, StepError> {
    let gzip = compressed(&mut reader)?;
    let mut index = Index::new(options)?;
    let mut graph = BTreeMap::new();
    let mut header_end = 0;
    let mut data = Vec::new();
    let (size, sha256) = {
        let mut source = Hashed {
            inner: decoded(&mut reader, gzip),
            hash: Sha256::new(),
            size: 0,
        };
        streaming::scan_structure(&mut source, options.chunk_size, |event| {
            match event {
                streaming::Event::Instance(i, b) => {
                    let slot = index
                        .add(i)
                        .map_err(|e| p21::P21Error::Parse(e.to_string()))?;
                    if index.kind(slot).retained() {
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
        (source.size, digest(source.hash))
    };
    if header_end > MAX_HEADER_BYTES {
        return Err(StepError::Limit(format!(
            "manifest header exceeds {MAX_HEADER_BYTES} bytes"
        )));
    }
    index.finish()?;
    let (plans, common_reps) = plans(&graph, &index)?;
    if plans.len() >= (MULTI - 2) as usize {
        return Err(StepError::Limit(
            "manifest has too many distinct products".into(),
        ));
    }
    let mut owners = Owners::new(index.len());
    // Common is a forward closure of contexts reached by the product/assembly skeleton or
    // shape representation. Per-pcurve contexts are deliberately not seeded here.
    let mut queue = Queue::new(index.len());
    for (&id, e) in &graph {
        if !common_reps.contains(&id)
            && !index
                .lookup(id)
                .is_some_and(|slot| index.kind(slot) == Kind::Assembly)
        {
            continue;
        }
        for slot in ids_of_kind(&index, references(e), Kind::context) {
            if owners.add(slot, COMMON) {
                queue.push_back(slot);
            }
        }
    }
    while let Some(slot) = queue.pop_front() {
        for &(_, target) in outgoing(&index.boundary, slot) {
            if index.kind(target).context() && owners.add(target, COMMON) {
                queue.push_back(target);
            }
        }
    }
    // Protocol declarations point to the application context, rather than vice versa.
    for &(from, to) in &index.boundary {
        if index.kind(from) == Kind::Protocol && owners.labels[to as usize] == COMMON {
            owners.add(from, COMMON);
        }
    }
    let mut roots: HashMap<u32, u32> = HashMap::new();
    for (p, plan) in plans.iter().enumerate() {
        for &rep in &plan.representations {
            let slot = index.lookup(rep).ok_or_else(dangling)?;
            let root = index.root(slot);
            let tag = owners.combine(
                roots.get(&root).copied().unwrap_or_default(),
                Owners::product(p),
            );
            roots.insert(root, tag);
        }
    }
    for group in &mut owners.groups {
        group.sealed = true;
    }
    for i in 0..index.len() as u32 {
        if index.kind(i).geometry() {
            let root = index.root(i);
            owners.labels[i as usize] = roots.get(&root).copied().unwrap_or_default();
        }
    }
    for (p, plan) in plans.iter().enumerate() {
        for &id in &plan.sdrs {
            let slot = index.lookup(id).ok_or_else(dangling)?;
            if owners.add(slot, Owners::product(p)) {
                queue.push_back(slot);
            }
        }
    }
    // Every geometry-to-boundary dependency is one flat pair. Copying a frame's point does
    // not claim the geometry component that might also happen to use that point.
    for &(from, to) in &index.boundary {
        let tag = owners.labels[from as usize];
        if tag != NONE && owners.add(to, tag) {
            queue.push_back(to);
        }
    }
    while let Some(slot) = queue.pop_front() {
        let tag = owners.labels[slot as usize];
        for &(_, target) in outgoing(&index.boundary, slot) {
            if owners.add(target, tag) {
                queue.push_back(target);
            }
        }
    }
    let mut products: Vec<_> = plans.iter().map(|p| p.product.clone()).collect();
    let mut common = Common::default();
    let mut unowned = Unowned::default();
    let mut previous = vec![usize::MAX; products.len() + 3];
    for (ordinal, slot) in index.file_slots().enumerate() {
        let tag = owners.labels[slot as usize];
        let range = index.range(slot);
        let adjacent = index.adjacent(slot);
        if tag == COMMON {
            push_range(
                &mut common.ranges,
                range,
                ordinal,
                adjacent,
                &mut previous[0],
            );
            common.entities += 1;
        } else if tag == NONE {
            let (group, out) = if index.kind(slot) == Kind::Presentation {
                (products.len() + 1, &mut unowned.presentation)
            } else {
                (products.len() + 2, &mut unowned.other)
            };
            push_range(
                &mut out.ranges,
                range,
                ordinal,
                adjacent,
                &mut previous[group],
            );
            out.entities += 1;
        } else if tag & MULTI == 0 {
            let p = (tag - 2) as usize;
            push_range(
                &mut products[p].ranges,
                range,
                ordinal,
                adjacent,
                &mut previous[p + 1],
            );
            products[p].entities += 1;
        } else {
            owners.each(tag, |p| {
                push_range(
                    &mut products[p].ranges,
                    range,
                    ordinal,
                    adjacent,
                    &mut previous[p + 1],
                );
                products[p].entities += 1;
            });
        }
    }
    for p in &mut products {
        p.bytes = p.ranges.iter().map(|r| r[1] - r[0]).sum();
        let mut stack: Vec<_> = p.faces.iter().filter_map(|id| index.lookup(*id)).collect();
        let mut seen = HashSet::new();
        while let Some(slot) = stack.pop() {
            if !seen.insert(slot) {
                continue;
            }
            if index.kind(slot) == Kind::Edge {
                p.edge_count += 1;
            } else if index.kind(slot) == Kind::Topology {
                stack.extend(outgoing(&index.boundary, slot).iter().map(|e| e.1));
            }
        }
    }
    // Only the skeleton's placement frames (not all geometric surface frames), and common
    // contexts/units, need bytes for the small placement document.
    let mut fetch_set = HashSet::new();
    for (slot, &tag) in owners.labels.iter().enumerate() {
        if tag == COMMON {
            fetch_set.insert(slot as u32);
        }
    }
    for e in graph.values() {
        let seeds = if name(e) == "ITEM_DEFINED_TRANSFORMATION" {
            references(e)
        } else {
            Vec::new()
        };
        for slot in ids_of_kind(&index, seeds, |k| k == Kind::Frame) {
            if fetch_set.insert(slot) {
                queue.push_back(slot);
            }
        }
    }
    while let Some(slot) = queue.pop_front() {
        for &(_, to) in outgoing(&index.boundary, slot) {
            if fetch_set.insert(to) {
                queue.push_back(to);
            }
        }
    }
    let shared: HashMap<_, _> = roots
        .iter()
        .filter(|(_, t)| **t & MULTI != 0)
        .map(|(&root, &tag)| (root, tag))
        .collect();
    let mut fetches = Vec::new();
    for slot in index.file_slots() {
        let for_assembly = fetch_set.contains(&slot);
        let for_shared = index.kind(slot).geometry() && shared.contains_key(&index.root_read(slot));
        if for_assembly || for_shared {
            fetches.push(Fetch {
                range: index.range(slot),
                slot,
                assembly: for_assembly,
                shared: for_shared,
            });
        }
    }
    // Release the per-entity ownership and boundary data before the byte/hash walk.
    let shared_sets = shared
        .iter()
        .map(|(&root, &tag)| (root, owners.members(tag)))
        .collect::<Vec<_>>();
    drop(owners);
    drop(queue);
    index.boundary = Vec::new();
    reader.seek(SeekFrom::Start(0)).map_err(io)?;
    let Fetched {
        placement_bytes: extra,
        shared_edges,
    } = second_pass(
        decoded(&mut reader, gzip),
        header_end,
        &mut common,
        &mut products,
        &fetches,
        &index,
        options.no_product_hashes,
    )?;
    let assemblies = super::super::assemblies(&graph);
    let (model, placement_graph, report) = placement_model(&graph, &index, &extra, &fetch_set)?;
    drop(extra);
    let PlacedProducts {
        products,
        instances,
        renumber: old_to_new,
    } = place_products(&model, &placement_graph, &report, products, options)?;
    let refusals = refusals(&mut index, &plans, &shared_sets, &shared_edges, &old_to_new)?;
    let mut manifest = Manifest {
        product_index: HashMap::new(),
        format: FORMAT.into(),
        source: Source {
            size,
            sha256,
            header: [0, header_end],
            data,
            compressed: gzip,
        },
        reader: ReaderVersion::default(),
        common,
        products,
        assemblies,
        instances,
        unowned,
        refusals,
    };
    manifest.reindex_products()?;
    Ok(manifest)
}
fn push_range(
    ranges: &mut Vec<ByteRange>,
    range: ByteRange,
    ordinal: usize,
    adjacent: bool,
    previous: &mut usize,
) {
    if adjacent
        && previous.checked_add(1) == Some(ordinal)
        && let Some(last) = ranges.last_mut()
    {
        last[1] = range[1];
    } else {
        ranges.push(range);
    }
    *previous = ordinal;
}

struct Fetch {
    range: ByteRange,
    slot: u32,
    assembly: bool,
    shared: bool,
}
#[derive(Clone, Copy)]
enum Action {
    Hash(u32, bool),
    Fetch(usize, bool),
}
struct Fetched {
    placement_bytes: Vec<u8>,
    shared_edges: Vec<(u32, u32)>,
}
fn second_pass(
    mut reader: impl Read,
    header_end: u64,
    common: &mut Common,
    products: &mut [Product],
    fetches: &[Fetch],
    index: &Index,
    no_hashes: bool,
) -> Result<Fetched, StepError> {
    let mut events = Vec::new();
    if !no_hashes {
        for r in std::iter::once(&[0, header_end]).chain(common.ranges.iter()) {
            events.push((r[0], Action::Hash(0, true)));
            events.push((r[1], Action::Hash(0, false)));
        }
        for (p, product) in products.iter().enumerate() {
            for r in &product.ranges {
                events.push((r[0], Action::Hash(p as u32 + 1, true)));
                events.push((r[1], Action::Hash(p as u32 + 1, false)));
            }
        }
    }
    for (i, f) in fetches.iter().enumerate() {
        events.push((f.range[0], Action::Fetch(i, true)));
        events.push((f.range[1], Action::Fetch(i, false)));
    }
    events.sort_by_key(|e| e.0);
    let mut hashes = if no_hashes {
        Vec::new()
    } else {
        vec![Sha256::new(); products.len() + 1]
    };
    let mut active = HashSet::new();
    let mut current_fetch = None;
    let mut text = Vec::new();
    let mut extra = Vec::new();
    let mut edges = Vec::new();
    let mut at = 0;
    let mut buffer = vec![0; CHUNK];
    for (end, action) in events {
        let mut left = end - at;
        if active.is_empty() && current_fetch.is_none() {
            let copied =
                std::io::copy(&mut reader.by_ref().take(left), &mut std::io::sink()).map_err(io)?;
            if copied != left {
                return Err(failure("source truncated in forward hash/fetch pass"));
            }
        } else {
            while left > 0 {
                let n = buffer.len().min(left as usize);
                reader.read_exact(&mut buffer[..n]).map_err(io)?;
                for &h in &active {
                    hashes[h as usize].update(&buffer[..n]);
                }
                if current_fetch.is_some() {
                    text.extend_from_slice(&buffer[..n]);
                }
                left -= n as u64;
            }
        }
        at = end;
        match action {
            Action::Hash(h, true) => {
                active.insert(h);
            }
            Action::Hash(h, false) => {
                active.remove(&h);
            }
            Action::Fetch(i, true) => {
                current_fetch = Some(i);
                text.clear();
            }
            Action::Fetch(i, false) => {
                let f = &fetches[i];
                if f.assembly {
                    extra.extend_from_slice(&text);
                    extra.push(b'\n');
                }
                if f.shared {
                    let end = text[1..]
                        .iter()
                        .position(|b| !b.is_ascii_digit())
                        .map_or(text.len(), |p| p + 1);
                    let id = std::str::from_utf8(&text[1..end])
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .ok_or_else(dangling)?;
                    for reference in references(&record(&text, id)?) {
                        if let Some(to) = index.lookup(reference)
                            && index.kind(to).geometry()
                        {
                            edges.push((f.slot, to));
                        }
                    }
                }
                current_fetch = None;
            }
        }
    }
    if !no_hashes {
        let mut h = hashes.into_iter();
        common.sha256 = h.next().map(digest);
        for (p, h) in products.iter_mut().zip(h) {
            p.sha256 = Some(digest(h));
        }
    }
    edges.sort_unstable();
    Ok(Fetched {
        placement_bytes: extra,
        shared_edges: edges,
    })
}

struct PlacementWalk<'a, 'b> {
    ids: &'a FileIds<'b>,
    product_index: HashMap<u64, usize>,
    seen: Vec<bool>,
    order: Vec<usize>,
    instances: Vec<Instance>,
    visited: usize,
    limit: usize,
    to_mm: f64,
}
impl PlacementWalk<'_, '_> {
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
        let definition = self.ids.id(def.key())?;
        let bodies = def.solids().count() + def.surface_models().count();
        if bodies > 0 {
            let &p = self
                .product_index
                .get(&definition)
                .ok_or_else(|| failure(format!("product #{definition} has no topology roots")))?;
            if !self.seen[p] {
                self.seen[p] = true;
                self.order.push(p);
            }
            for _ in 0..bodies {
                if self.instances.len() >= self.limit {
                    return Err(StepError::Limit(format!(
                        "manifest instance limit {}",
                        self.limit
                    )));
                }
                self.instances.push(Instance {
                    product: format!("p{p}"),
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
struct PlacedProducts {
    products: Vec<Product>,
    instances: Vec<Instance>,
    renumber: Vec<String>,
}
fn place_products(
    model: &m::StepModel,
    graph: &step_io::parser::Graph,
    report: &step_io::Report,
    products: Vec<Product>,
    options: &ScanOptions,
) -> Result<PlacedProducts, StepError> {
    let mut ids = FileIds::new(graph, model, report);
    let dropped: HashSet<_> = report.dropped.iter().map(|(id, _)| *id).collect();
    ids.by_type.insert(
        "NEXT_ASSEMBLY_USAGE_OCCURRENCE",
        graph
            .entities
            .iter()
            .filter_map(|(&id, e)| {
                (name(e) == "NEXT_ASSEMBLY_USAGE_OCCURRENCE" && !dropped.contains(&id))
                    .then_some(id)
            })
            .collect(),
    );
    let scene = model.scene();
    let units = scene.units();
    let mut walk = PlacementWalk {
        ids: &ids,
        product_index: products
            .iter()
            .enumerate()
            .map(|(i, p)| (p.product_definition, i))
            .collect(),
        seen: vec![false; products.len()],
        order: Vec::new(),
        instances: Vec::new(),
        visited: 0,
        limit: options.instance_limit,
        to_mm: units.length.as_ref().map_or(1.0, |u| u.to_si * 1000.0),
    };
    for root in scene.root_definitions() {
        walk.visit(root, IDENTITY, &mut Vec::new())?;
    }
    if walk.order.len() != products.len() {
        let missing = walk.seen.iter().position(|seen| !*seen).unwrap_or_default();
        return Err(failure(format!(
            "product #{} is not reachable from an assembly root (or has dropped dependencies)",
            products[missing].product_definition
        )));
    }
    let assemblies = super::super::assemblies(&graph.entities);
    let mut wrapper_names = HashMap::new();
    for a in assemblies {
        if let [c] = a.components.as_slice()
            && !a.name.is_empty()
        {
            wrapper_names.entry(c.definition).or_insert(a.name);
        }
    }
    let mut renumber = vec![String::new(); products.len()];
    for (next, &old) in walk.order.iter().enumerate() {
        renumber[old] = format!("p{next}");
    }
    // IDs in instances are provisional only until the retained hierarchy gives the existing
    // reader's visit order. Hashes and ranges move with the product, without rescanning.
    let provisional: HashMap<_, _> = renumber
        .iter()
        .enumerate()
        .map(|(old, new)| (format!("p{old}"), new.clone()))
        .collect();
    for instance in &mut walk.instances {
        if let Some(new) = provisional.get(&instance.product) {
            instance.product = new.clone();
        }
    }
    let mut products: Vec<_> = products.into_iter().map(Some).collect();
    let mut out = Vec::with_capacity(products.len());
    for old in walk.order {
        let mut p = products[old].take().ok_or_else(dangling)?;
        p.id = renumber[old].clone();
        if let Some(name) = wrapper_names.get(&p.product_definition) {
            p.name = name.clone();
        }
        out.push(p);
    }
    Ok(PlacedProducts {
        products: out,
        instances: walk.instances,
        renumber,
    })
}
fn refusals(
    index: &mut Index,
    plans: &[Plan],
    shared: &[(u32, Vec<usize>)],
    edges: &[(u32, u32)],
    renumber: &[String],
) -> Result<Vec<Refusal>, StepError> {
    let mut out = Vec::new();
    for (root, products) in shared {
        let mut reached = HashMap::new();
        let mut witness = None;
        for &p in products {
            let mut stack = ids_of_kind(
                index,
                plans[p].representations.iter().copied(),
                Kind::geometry,
            );
            let mut seen = HashSet::new();
            while let Some(slot) = stack.pop() {
                if !seen.insert(slot) {
                    continue;
                }
                if reached
                    .insert(slot, p)
                    .is_some_and(|previous| previous != p)
                {
                    witness = Some(slot);
                    break;
                }
                stack.extend(outgoing(edges, slot).iter().map(|e| e.1));
            }
            if witness.is_some() {
                break;
            }
        }
        let entity = index.id(witness.unwrap_or(*root)).ok_or_else(dangling)?;
        out.push(Refusal {
            products: products.iter().map(|&p| renumber[p].clone()).collect(),
            entity,
            reason: format!("shared geometry entity #{entity}; use the whole-file reader"),
        });
    }
    out.sort_by_key(|r| r.entity);
    Ok(out)
}

pub(super) fn verify<R: Read + Seek>(m: &Manifest, mut reader: R) -> Result<(), StepError> {
    let gzip = compressed(&mut reader)?;
    let mut reader = decoded(&mut reader, gzip);
    let mut hash = Sha256::new();
    let mut size = 0;
    let mut b = vec![0; CHUNK];
    loop {
        let n = reader.read(&mut b).map_err(io)?;
        if n == 0 {
            break;
        }
        hash.update(&b[..n]);
        size += n as u64;
    }
    if size != m.source.size || digest(hash) != m.source.sha256 {
        return Err(failure("manifest source size or sha256 mismatch"));
    }
    Ok(())
}
