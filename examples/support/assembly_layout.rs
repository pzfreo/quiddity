//! Shared layout/scale engine. Only source-sized templates are retained; generated
//! entities and sidecar rows are encoded directly to buffered writers.
use haecceity::{
    p21::{
        self, Attribute, Document, RawEntity,
        streaming::{self, Event},
    },
    step::manifest::{self, ScanOptions},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    error::Error,
    io::{Cursor, Read, Seek, SeekFrom, Write},
};

pub type Result<T> = std::result::Result<T, Box<dyn Error>>;
#[derive(Clone, Copy, Debug)]
pub enum Layout {
    Contiguous,
    Runs,
    Shuffled,
}
impl std::str::FromStr for Layout {
    type Err = &'static str;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "contiguous" => Ok(Self::Contiguous),
            "runs" => Ok(Self::Runs),
            "shuffled" => Ok(Self::Shuffled),
            _ => Err("expected contiguous, runs or shuffled"),
        }
    }
}
#[derive(Clone, Copy)]
pub struct Options {
    pub layout: Layout,
    pub runs: usize,
    pub seed: u64,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            layout: Layout::Contiguous,
            runs: 4,
            seed: 0x190,
        }
    }
}
fn random(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state
}
fn shuffle<T>(v: &mut [T], seed: u64) {
    let mut state = seed;
    for i in (1..v.len()).rev() {
        v.swap(i, random(&mut state) as usize % (i + 1));
    }
}
/// Reorder exact statement bytes, preserving ids and all inter-statement trivia.
pub fn rewrite(bytes: &[u8], options: Options, mut out: impl Write) -> Result<()> {
    if options.runs == 0 {
        return Err("--runs must be positive".into());
    }
    let m = manifest::scan(Cursor::new(bytes), &ScanOptions::default())?;
    let mut rows = Vec::new();
    streaming::scan(bytes, 8192, |e| {
        if let Event::Instance(i, _) = e {
            rows.push((i.offset as usize, i.length as usize));
        }
        Ok(())
    })?;
    let first = rows.first().ok_or("source has no entities")?.0;
    let end = rows.last().map(|(a, b)| a + b).unwrap();
    let mut buckets = vec![Vec::new(); m.products.len() + 1];
    for (i, &(at, _)) in rows.iter().enumerate() {
        let owner = m
            .products
            .iter()
            .position(|p| {
                p.ranges
                    .iter()
                    .any(|r| r[0] <= at as u64 && (at as u64) < r[1])
            })
            .map_or(0, |p| p + 1);
        buckets[owner].push(i);
    }
    let mut order = Vec::with_capacity(rows.len());
    match options.layout {
        Layout::Contiguous => order.extend(buckets.iter().flatten().copied()),
        Layout::Runs => {
            // Seed changes product scheduling, never the order within a product.
            let mut schedule: Vec<_> = (0..buckets.len()).collect();
            shuffle(&mut schedule, options.seed);
            for run in 0..options.runs {
                for &b in &schedule {
                    let v = &buckets[b];
                    order.extend_from_slice(
                        &v[run * v.len() / options.runs..(run + 1) * v.len() / options.runs],
                    );
                }
            }
        }
        Layout::Shuffled => {
            order.extend(0..rows.len());
            shuffle(&mut order, options.seed);
        }
    }
    out.write_all(&bytes[..first])?;
    for i in order {
        let (at, n) = rows[i];
        let start = if i == 0 {
            at
        } else {
            rows[i - 1].0 + rows[i - 1].1
        };
        out.write_all(&bytes[start..at + n])?;
        out.write_all(b"\n")?;
    }
    out.write_all(&bytes[end..])?;
    out.flush()?;
    Ok(())
}
#[allow(dead_code)] // Used by the two integration-test binaries.
pub fn layouts(bytes: &[u8]) -> Vec<Vec<u8>> {
    [Layout::Contiguous, Layout::Runs, Layout::Shuffled]
        .into_iter()
        .map(|layout| {
            let mut out = Vec::new();
            rewrite(
                bytes,
                Options {
                    layout,
                    ..Options::default()
                },
                &mut out,
            )
            .unwrap();
            out
        })
        .collect()
}
fn attrs(e: &RawEntity) -> &[Attribute] {
    match e {
        RawEntity::Simple { attributes, .. } => attributes,
        _ => &[],
    }
}
fn named(e: &RawEntity, n: &str) -> bool {
    matches!(e, RawEntity::Simple { name, .. } if name == n)
}
fn reference(a: &Attribute) -> Option<u64> {
    if let Attribute::EntityRef(n) = a {
        Some(*n)
    } else {
        None
    }
}
fn remap(e: &mut RawEntity, offset: u64, owned: &HashSet<u64>, suffix: &str) {
    fn walk(a: &mut Attribute, offset: u64, owned: &HashSet<u64>) {
        match a {
            Attribute::EntityRef(n) if owned.contains(n) => *n += offset,
            Attribute::List(v) => {
                for a in v {
                    walk(a, offset, owned);
                }
            }
            Attribute::Typed { value, .. } => walk(value, offset, owned),
            _ => {}
        }
    }
    match e {
        RawEntity::Simple {
            name, attributes, ..
        } => {
            for a in attributes.iter_mut() {
                walk(a, offset, owned);
            }
            if name == "PRODUCT"
                && let Some(Attribute::String(s)) = attributes.first_mut()
            {
                s.push_str(suffix);
            }
        }
        RawEntity::Complex { parts, .. } => {
            for a in parts.iter_mut().flat_map(|p| &mut p.attributes) {
                walk(a, offset, owned);
            }
        }
    }
}
struct Template {
    rows: Vec<(u64, RawEntity)>,
    owned: HashSet<u64>,
    pd: u64,
    rep: u64,
    faces: usize,
    entities: u64,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ExpectedProduct {
    pub product_definition: u64,
    pub faces: usize,
    pub entities: u64,
}
#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct Expected {
    pub products: usize,
    pub instances: usize,
    pub entities: u64,
    pub bytes: u64,
    pub per_product: Vec<ExpectedProduct>,
}
#[derive(Clone, Copy)]
pub struct Scale {
    pub distinct: usize,
    pub instances: usize,
    pub target_bytes: u64,
}
struct Count<W> {
    out: W,
    bytes: u64,
}
impl<W: Write> Write for Count<W> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        let n = self.out.write(b)?;
        self.bytes += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
}
/// N and M are minima. Whole product copies are added until the byte target is
/// reached, each with an occurrence. No padding or unused geometry fills the file.
/// Sidecar rows stream too: memory depends on the source, never the byte target.
fn scale_contiguous(
    bytes: &[u8],
    options: Options,
    scale: Scale,
    out: impl Write,
    mut sidecar: impl Write,
    mut groups: impl Write,
) -> Result<()> {
    if scale.distinct == 0 || options.runs == 0 {
        return Err("--distinct and --runs must be positive".into());
    }
    let m = manifest::scan(Cursor::new(bytes), &ScanOptions::default())?;
    if !m.refusals.is_empty() {
        return Err("scale source has shared geometry refusals".into());
    }
    let doc = Document::parse(bytes.to_vec())?;
    let stride = doc.max_id();
    // Keep generated assembly metadata in a disjoint sparse region, leaving
    // copied source ids offset by exactly k * source.max_id().
    let base = 1u64 << 60;
    if stride >= base {
        return Err("source ids overlap the synthetic assembly id region".into());
    }
    let mut templates = Vec::new();
    for p in &m.products {
        let rows: Vec<_> = doc
            .ids()
            .filter(|id| {
                let at = doc.span(*id).unwrap().start as u64;
                p.ranges.iter().any(|r| r[0] <= at && at < r[1])
            })
            .map(|id| (id, doc.get(id).unwrap().clone()))
            .collect();
        let rep = rows
            .iter()
            .find_map(|(_, e)| {
                if named(e, "SHAPE_DEFINITION_REPRESENTATION") {
                    attrs(e).get(1).and_then(reference)
                } else {
                    None
                }
            })
            .ok_or("product has no shape representation")?;
        templates.push(Template {
            owned: rows.iter().map(|r| r.0).collect(),
            rows,
            pd: p.product_definition,
            rep,
            faces: p.faces.len(),
            entities: p.entities,
        });
    }
    if templates.is_empty() {
        return Err("source has no products".into());
    }
    let mut common = Vec::new();
    for id in doc.ids() {
        let at = doc.span(id).unwrap().start as u64;
        if m.common.ranges.iter().any(|r| r[0] <= at && at < r[1]) {
            common.push((id, doc.get(id).unwrap().clone()));
        }
    }
    let context = |n| {
        common
            .iter()
            .find_map(|(id, e)| named(e, n).then_some(*id))
            .ok_or_else(|| format!("missing {n}"))
    };
    let pc = context("PRODUCT_CONTEXT")?;
    let dc = context("PRODUCT_DEFINITION_CONTEXT")?;
    let rc = doc
        .graph()
        .entities
        .iter()
        .find_map(|(id, e)| match e {
            RawEntity::Complex { parts, .. }
                if parts
                    .iter()
                    .any(|p| p.name == "GEOMETRIC_REPRESENTATION_CONTEXT") =>
            {
                Some(*id)
            }
            _ => None,
        })
        .ok_or("missing geometry context")?;
    let rootpd = base + 3;
    let rootrep = base + 6;
    let axis = base + 7;
    let root = format!(
        "#{}=PRODUCT('scale-root','scale-root','',(#{pc}));\n#{}=PRODUCT_DEFINITION_FORMATION('','',#{});\n#{rootpd}=PRODUCT_DEFINITION('design','',#{},#{dc});\n#{}=PRODUCT_DEFINITION_SHAPE('','',#{rootpd});\n#{}=SHAPE_DEFINITION_REPRESENTATION(#{},#{rootrep});\n#{rootrep}=SHAPE_REPRESENTATION('',(#{axis}),#{rc});\n#{axis}=AXIS2_PLACEMENT_3D('',#{},#{},#{});\n#{}=CARTESIAN_POINT('',(0.,0.,0.));\n#{}=DIRECTION('',(0.,0.,1.));\n#{}=DIRECTION('',(1.,0.,0.));\n",
        base + 1,
        base + 2,
        base + 1,
        base + 2,
        base + 4,
        base + 5,
        base + 4,
        base + 8,
        base + 9,
        base + 10,
        base + 8,
        base + 9,
        base + 10
    );
    let mut out = Count { out, bytes: 0 };
    out.write_all(&bytes[..m.source.header[1] as usize])?;
    out.write_all(b"\nDATA;\n")?;
    for (id, e) in &common {
        writeln!(out, "#{id}={};", p21::encode(e)?)?;
    }
    out.write_all(root.as_bytes())?;
    let mut products = 0usize;
    let mut instances = 0usize;
    let mut entities = common.len() as u64 + 10;
    group(&mut groups, 0, entities)?;
    sidecar.write_all(b"{\"per_product\":[")?;
    // Each product is emitted before the next is constructed. Runs/shuffled scale
    // output uses a source-sized batch, so even very large targets stay bounded.
    while products < scale.distinct
        || out.bytes + (b"ENDSEC;\nEND-ISO-10303-21;\n".len() as u64) < scale.target_bytes
    {
        let t = &templates[products % templates.len()];
        let offset = (products as u64 + 1)
            .checked_mul(stride)
            .ok_or("id overflow")?;
        if offset.checked_add(stride).is_none_or(|end| end >= base) {
            return Err("id overflow".into());
        }
        for i in 0..t.rows.len() {
            let (id, e) = &t.rows[i];
            let mut e = e.clone();
            remap(&mut e, offset, &t.owned, &format!("-copy-{}", products + 1));
            writeln!(out, "#{}={};", id + offset, p21::encode(&e)?)?;
        }
        if products > 0 {
            sidecar.write_all(b",")?;
        }
        serde_json::to_writer(
            &mut sidecar,
            &ExpectedProduct {
                product_definition: t.pd + offset,
                faces: t.faces,
                entities: t.entities,
            },
        )?;
        group(&mut groups, entities, t.rows.len() as u64 + 7)?;
        entities += t.rows.len() as u64;
        occurrence(
            &mut out,
            base + 11 + instances as u64 * 7,
            rootpd,
            rootrep,
            axis,
            t.pd + offset,
            t.rep + offset,
            instances,
        )?;
        entities += 7;
        instances += 1;
        products += 1;
    }
    while instances < scale.instances {
        let copy = instances % products;
        let t = &templates[copy % templates.len()];
        let offset = (copy as u64 + 1) * stride;
        let id = base + 11 + instances as u64 * 7;
        occurrence(
            &mut out,
            id,
            rootpd,
            rootrep,
            axis,
            t.pd + offset,
            t.rep + offset,
            instances,
        )?;
        group(&mut groups, entities, 7)?;
        entities += 7;
        instances += 1;
    }
    out.write_all(b"ENDSEC;\nEND-ISO-10303-21;\n")?;
    out.flush()?;
    writeln!(
        sidecar,
        "],\"products\":{products},\"instances\":{instances},\"entities\":{entities},\"bytes\":{}}}",
        out.bytes
    )?;
    sidecar.flush()?;
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn occurrence(
    out: &mut impl Write,
    id: u64,
    rootpd: u64,
    rootrep: u64,
    axis: u64,
    pd: u64,
    rep: u64,
    i: usize,
) -> Result<()> {
    let x = (i % 100) as f64 * 1000.;
    let y = (i / 100) as f64 * 1000.;
    let x = p21::encode_real(x)?;
    let y = p21::encode_real(y)?;
    writeln!(
        out,
        "#{id}=NEXT_ASSEMBLY_USAGE_OCCURRENCE('{i}','copy-{i}','',#{rootpd},#{pd},$);\n#{}=PRODUCT_DEFINITION_SHAPE('','',#{id});\n#{}=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{},#{});\n#{}=(REPRESENTATION_RELATIONSHIP('','',#{rep},#{rootrep}) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{}) SHAPE_REPRESENTATION_RELATIONSHIP());\n#{}=ITEM_DEFINED_TRANSFORMATION('','',#{axis},#{});\n#{}=AXIS2_PLACEMENT_3D('',#{},$,$);\n#{}=CARTESIAN_POINT('',({x},{y},0.));",
        id + 1,
        id + 2,
        id + 3,
        id + 1,
        id + 3,
        id + 4,
        id + 4,
        id + 5,
        id + 5,
        id + 6,
        id + 6
    )?;
    Ok(())
}

fn group(out: &mut impl Write, start: u64, count: u64) -> Result<()> {
    out.write_all(&start.to_le_bytes())?;
    out.write_all(&count.to_le_bytes())?;
    Ok(())
}
fn pair(file: &mut std::fs::File, i: u64) -> Result<(u64, u64)> {
    file.seek(SeekFrom::Start(i.checked_mul(16).ok_or("index overflow")?))?;
    let mut b = [0; 16];
    file.read_exact(&mut b)?;
    Ok((
        u64::from_le_bytes(b[..8].try_into()?),
        u64::from_le_bytes(b[8..].try_into()?),
    ))
}
fn permutation(n: u64, seed: u64) -> (u64, u64) {
    fn gcd(mut a: u64, mut b: u64) -> u64 {
        while b != 0 {
            let r = a % b;
            a = b;
            b = r;
        }
        a
    }
    let mut state = seed;
    let mut a = random(&mut state) % n;
    while gcd(a, n) != 1 {
        a = (a + 1) % n;
    }
    (a, random(&mut state) % n)
}
/// Contiguous output goes directly to the caller. Other layouts spool statements
/// and 16-byte extent/group records to temporary files, then seek/copy in the
/// requested order. Neither the statement index nor the product schedule lives
/// in RAM. Shuffling is a seeded affine bijection of *all* entity positions.
pub fn scale(
    bytes: &[u8],
    options: Options,
    settings: Scale,
    mut out: impl Write,
    sidecar: impl Write,
) -> Result<()> {
    if matches!(options.layout, Layout::Contiguous) {
        return scale_contiguous(bytes, options, settings, out, sidecar, std::io::sink());
    }
    let mut spool = tempfile::tempfile()?;
    let mut groups = tempfile::tempfile()?;
    scale_contiguous(
        bytes,
        options,
        settings,
        std::io::BufWriter::new(&mut spool),
        sidecar,
        &mut groups,
    )?;
    spool.rewind()?;
    let mut index = tempfile::tempfile()?;
    let mut rows = 0;
    let mut first = 0;
    let mut end = 0;
    {
        let mut buffered = std::io::BufWriter::new(&mut index);
        streaming::scan(&mut spool, 8192, |event| {
            if let Event::Instance(i, _) = event {
                if rows == 0 {
                    first = i.offset;
                }
                end = i.offset + i.length as u64 + 1;
                group(&mut buffered, i.offset, i.length as u64 + 1)
                    .map_err(|e| p21::P21Error::Encode(e.to_string()))?;
                rows += 1;
            }
            Ok(())
        })?;
        buffered.flush()?;
    }
    spool.rewind()?;
    std::io::copy(&mut (&mut spool).take(first), &mut out)?;
    let mut copy = |i| -> Result<()> {
        let (at, len) = pair(&mut index, i)?;
        spool.seek(SeekFrom::Start(at))?;
        std::io::copy(&mut (&mut spool).take(len), &mut out)?;
        Ok(())
    };
    match options.layout {
        Layout::Shuffled => {
            let (a, b) = permutation(rows, options.seed);
            for i in 0..rows {
                copy(((a as u128 * i as u128 + b as u128) % rows as u128) as u64)?;
            }
        }
        Layout::Runs => {
            let n = groups.metadata()?.len() / 16;
            let (a, b) = permutation(n, options.seed);
            for run in 0..options.runs as u64 {
                for g in 0..n {
                    let g = ((a as u128 * g as u128 + b as u128) % n as u128) as u64;
                    let (start, len) = pair(&mut groups, g)?;
                    for i in start + run * len / options.runs as u64
                        ..start + (run + 1) * len / options.runs as u64
                    {
                        copy(i)?;
                    }
                }
            }
        }
        Layout::Contiguous => unreachable!(),
    }
    spool.seek(SeekFrom::Start(end))?;
    std::io::copy(&mut spool, &mut out)?;
    out.flush()?;
    Ok(())
}
