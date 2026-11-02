//! The AP242 semantic PMI reader.

mod common;

use std::io::Read;
use std::path::Path;

use haecceity::p21::Document;
use haecceity::pmi::{self, PmiRead};
use haecceity::step::read_part_definitions;

fn file_bytes(path: &Path) -> Vec<u8> {
    let raw = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    if path.extension().is_some_and(|e| e == "gz") {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(raw.as_slice())
            .read_to_end(&mut out)
            .unwrap();
        out
    } else {
        raw
    }
}

fn read_bytes(bytes: Vec<u8>) -> PmiRead {
    read_doc(bytes).0
}

fn read_doc(bytes: Vec<u8>) -> (PmiRead, Document) {
    let parts = read_part_definitions(&bytes).expect("part definitions");
    let doc = Document::parse(bytes).expect("document");
    let r = pmi::read(&doc, &parts).expect("pmi");
    (r, doc)
}

/// The item's own instance among its provenance: the first of the given entity types.
fn root(doc: &Document, prov: &[u64], types: &[&str]) -> u64 {
    prov.iter()
        .copied()
        .find(|&id| {
            let names: Vec<String> = match doc.get(id).unwrap() {
                haecceity::p21::RawEntity::Simple { name, .. } => vec![name.to_ascii_lowercase()],
                haecceity::p21::RawEntity::Complex { parts, .. } => {
                    parts.iter().map(|p| p.name.to_ascii_lowercase()).collect()
                }
            };
            names
                .iter()
                .any(|n| types.iter().any(|t| haecceity::express::is_a(n, t)))
        })
        .unwrap_or(0)
}

/// Development aid: `PMI_DUMP=<file> cargo test --test pmi_read dump -- --ignored --nocapture`.
#[test]
#[ignore]
fn dump() {
    let path = std::env::var("PMI_DUMP").expect("PMI_DUMP");
    let r = read_bytes(file_bytes(Path::new(&path)));
    println!("{}", summary(&r));
}

fn v(x: &pmi::Value) -> String {
    let unit = match &x.quantity {
        pmi::Quantity::Length(l) => format!("{:?}", l.unit),
        pmi::Quantity::Angle(a) => format!("{:?}", a.unit),
    };
    match x.decimal_places {
        Some(p) => format!("{}{unit}/{p}", x.decimal()),
        None => format!("{}{unit}", x.decimal()),
    }
}

/// One line per item, for reading and pinning.
fn summary(r: &PmiRead) -> String {
    use std::fmt::Write;
    let mut o = String::new();
    for (pi, p) in r.parts.iter().enumerate() {
        let prov = &r.provenance.parts[pi];
        writeln!(
            o,
            "part {pi}: standards {:?} places {:?} material {:?}",
            p.standards, p.decimal_places, p.material
        )
        .unwrap();
        for (i, f) in p.features.iter().enumerate() {
            writeln!(
                o,
                "  F{i} {f:?} <- {:?}",
                &prov.features[i][..prov.features[i].len().min(3)]
            )
            .unwrap();
        }
        for (i, t) in p.datum_targets.iter().enumerate() {
            writeln!(
                o,
                "  T{i} {} {:?} on {:?} movable {:?} <- {:?}",
                t.number(),
                t.shape(),
                t.on(),
                t.movable().is_some(),
                prov.datum_targets[i].first()
            )
            .unwrap();
        }
        for (i, d) in p.datums.iter().enumerate() {
            writeln!(
                o,
                "  D{i} {} {:?} {:?} <- {:?}",
                d.label(),
                d.feature(),
                d.targets(),
                prov.datums[i].first()
            )
            .unwrap();
        }
        for (i, d) in p.dimensions.iter().enumerate() {
            let tol = match &d.tolerance {
                pmi::DimTolerance::Deviations(b) => format!("+{} {}", v(b.upper()), v(b.lower())),
                pmi::DimTolerance::Limits(b) => format!("limits {} {}", v(b.upper()), v(b.lower())),
                pmi::DimTolerance::Fit { class, limits } => format!(
                    "fit {class} {:?}",
                    limits.as_ref().map(|b| (v(b.upper()), v(b.lower())))
                ),
                t => format!("{t:?}"),
            };
            writeln!(
                o,
                "  N{i} {:?} {} {tol} q {:?} m {:?} p {:?} <- {:?}",
                d.kind,
                d.nominal.as_ref().map(v).unwrap_or_else(|| "-".into()),
                d.qualifier,
                d.modifiers,
                d.principle,
                prov.dimensions[i].first()
            )
            .unwrap();
        }
        for (i, t) in p.tolerances.iter().enumerate() {
            let ds = t.datums.as_ref().map(|s| {
                s.compartments()
                    .iter()
                    .map(|c| {
                        c.references
                            .iter()
                            .map(|r| format!("{}{:?}", p.datums[r.datum.0].label(), r.modifiers))
                            .collect::<Vec<_>>()
                            .join("-")
                            + &format!("{:?}", c.modifiers)
                    })
                    .collect::<Vec<_>>()
                    .join("|")
            });
            writeln!(o, "  G{i} {:?} {:?} {} zone {:?} mods {:?} ub {:?} max {:?} uneq {:?} datums {:?} aux {:?} desc {:?} <- {:?}", t.kind, t.target, t.magnitude.as_ref().map(v).unwrap_or_default(), t.zone, t.modifiers, t.unit_basis, t.maximum.as_ref().map(v), t.unequal.as_ref().map(v), ds, t.auxiliary, t.description, prov.tolerances[i].first()).unwrap();
        }
        for r in &p.tolerance_relations {
            writeln!(o, "  R {r:?}").unwrap();
        }
        for g in &p.general {
            writeln!(o, "  general {g:?}").unwrap();
        }
        for t in &p.threads {
            writeln!(o, "  thread {t:?}").unwrap();
        }
        for k in &p.knurls {
            writeln!(o, "  knurl {k:?}").unwrap();
        }
        for n in &p.notes {
            writeln!(o, "  note {n:?}").unwrap();
        }
        for a in &p.attributes {
            writeln!(o, "  attr {a:?}").unwrap();
        }
    }
    for f in &r.findings {
        writeln!(
            o,
            "finding {} {:?} {} {:?}: {}",
            f.kind.as_str(),
            f.part,
            f.entity,
            f.ids,
            f.detail
        )
        .unwrap();
    }
    writeln!(o, "{:?}", r.accounting).unwrap();
    o
}

// ---------------------------------------------------------------------------------------------
// NIST expected PMI (SFA notation) against the reader
// ---------------------------------------------------------------------------------------------

use std::collections::BTreeMap;

use haecceity::pmi::{
    DatumModifier, DimTolerance, DimensionKind, DimensionModifier, Feature, PartPmi, Qualifier,
    SimpleDatumModifier, SizeKind, TargetShape, ToleranceKind, ToleranceModifier, ToleranceTarget,
};
use serde_json::{Value as Json, json};

/// A number as shown: its value and, when the file states them (§5.4), its decimal places.
#[derive(Clone, Debug)]
struct Num {
    value: f64,
    places: Option<usize>,
}

impl PartialEq for Num {
    fn eq(&self, o: &Num) -> bool {
        let scale = self.value.abs().max(o.value.abs()).max(1e-300);
        (self.value - o.value).abs() <= 1e-12 * scale
    }
}

fn num(t: &str) -> Option<Num> {
    let t = t.trim().trim_end_matches('°');
    let v: f64 = t.parse().ok()?;
    let places = t.split_once('.').map_or(0, |(_, f)| f.len());
    Some(Num {
        value: v,
        places: Some(places),
    })
}

/// A read value as shown: rounded to its stated decimal places, else as stated. §5.4 Table 9
/// says the places truncate; NIST's drawings round (FTC-07's 0.5938 is shown .594) and, being
/// ASME, round a final 5 to even (§4.1.1; STC-06's 0.8125 is shown .812), as Rust's formatting
/// of these exactly representable values does.
fn shown(v: &pmi::Value) -> Num {
    let d = v.decimal().to_f64();
    match v.decimal_places {
        Some(p) => {
            let text = format!("{:.*}", usize::from(p), d);
            Num {
                value: text.parse().unwrap(),
                places: Some(usize::from(p)),
            }
        }
        None => Num {
            value: d,
            places: None,
        },
    }
}

#[derive(Clone, Debug, PartialEq)]
enum TolF {
    None,
    Basic,
    PlusMinus(Num),
    Dev(Num, Num),
    Limits(Num, Num),
    Fit(String),
}

/// A dimension as SFA shows it.
#[derive(Clone, Debug, PartialEq)]
struct DimF {
    sym: String,
    angle: bool,
    nominal: Option<Num>,
    tol: TolF,
    qualifier: Option<String>,
    reference: bool,
    statistical: bool,
}

/// A feature control frame as SFA shows it.
#[derive(Clone, Debug, PartialEq)]
struct GtolF {
    sym: String,
    all_around: bool,
    all_over: bool,
    zone: String,
    magnitude: Option<Num>,
    mods: Vec<String>,
    projected: Option<Num>,
    unequal: Option<Num>,
    maximum: Option<Num>,
    per_unit: Option<Num>,
    datums: Vec<String>,
    datum_feature: Option<String>,
    composite: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct TargetF {
    label: String,
    shape: String,
    size: Option<Num>,
    movable: bool,
}

fn strip_prefix_sym(t: &str) -> (String, &str) {
    for s in ["S⌀", "SR", "CR", "⌀", "R"] {
        if let Some(r) = t.strip_prefix(s)
            && r.starts_with(|c: char| c.is_ascii_digit() || c == '.')
        {
            return (s.to_string(), r);
        }
    }
    (String::new(), t)
}

/// SFA's dimension text: `[nX ]['('][symbol]value[°][-upper] [± v | +u -l | class | MAX | MIN
/// | <ST>][')']`. The repetition count nX is SFA's own inference from the associated faces
/// (`sfa-geom.tcl` reportAssocGeom counts cylindrical faces), not a semantic statement, and is
/// not compared.
fn parse_dim(text: &str) -> Option<DimF> {
    let mut toks: Vec<&str> = text.split_whitespace().collect();
    if toks
        .first()
        .is_some_and(|t| t.ends_with('X') && t[..t.len() - 1].parse::<u32>().is_ok())
    {
        toks.remove(0);
    }
    let mut reference = false;
    let first = toks.first()?.to_string();
    let mut main = first.as_str();
    if main.starts_with('(') {
        reference = true;
        main = main.trim_start_matches('(').trim_end_matches(')');
    }
    let (sym, rest) = strip_prefix_sym(main);
    let angle = rest.contains('°');
    let mut d = DimF {
        sym,
        angle,
        nominal: None,
        tol: TolF::None,
        qualifier: None,
        reference,
        statistical: false,
    };
    // Limits 'lower-upper'.
    if let Some(i) = rest[1..].find('-').map(|i| i + 1) {
        d.tol = TolF::Limits(num(&rest[..i])?, num(&rest[i + 1..])?);
    } else {
        d.nominal = Some(num(rest)?);
    }
    let rest: Vec<&str> = toks[1..].to_vec();
    let mut i = 0;
    let mut nums = Vec::new();
    while i < rest.len() {
        let t = rest[i].trim_end_matches(')');
        if t.ends_with(')') || rest[i].ends_with(')') {
            reference = true;
        }
        match t {
            "±" => {
                d.tol = TolF::PlusMinus(num(rest.get(i + 1)?)?);
                i += 1;
            }
            "MAX" => d.qualifier = Some("maximum".into()),
            "MIN" => d.qualifier = Some("minimum".into()),
            "<ST>" => d.statistical = true,
            t if num(t).is_some() => nums.push(num(t)?),
            t if t.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) => {
                d.tol = TolF::Fit(t.to_string());
            }
            _ => return None,
        }
        i += 1;
    }
    d.reference = reference;
    if nums.len() == 2 {
        d.tol = TolF::Dev(nums[0].clone(), nums[1].clone());
    } else if !nums.is_empty() {
        return None;
    }
    Some(d)
}

const MOD_SYMBOLS: [(&str, ToleranceModifier); 8] = [
    ("Ⓜ", ToleranceModifier::MaximumMaterialRequirement),
    ("Ⓛ", ToleranceModifier::LeastMaterialRequirement),
    ("Ⓕ", ToleranceModifier::FreeState),
    ("Ⓣ", ToleranceModifier::TangentPlane),
    ("<ST>", ToleranceModifier::StatisticalTolerance),
    ("CZ", ToleranceModifier::CommonZone),
    ("Ⓡ", ToleranceModifier::ReciprocityRequirement),
    ("ACS", ToleranceModifier::AnyCrossSection),
];

const TOL_SYMBOLS: [(&str, ToleranceKind); 15] = [
    ("⌖", ToleranceKind::Position),
    ("⌓", ToleranceKind::SurfaceProfile),
    ("⌒", ToleranceKind::LineProfile),
    ("▱", ToleranceKind::Flatness),
    ("⏊", ToleranceKind::Perpendicularity),
    ("⫽", ToleranceKind::Parallelism),
    ("∠", ToleranceKind::Angularity),
    ("⌭", ToleranceKind::Cylindricity),
    ("−", ToleranceKind::Straightness),
    ("○", ToleranceKind::Roundness),
    ("⌯", ToleranceKind::Symmetry),
    ("↗", ToleranceKind::CircularRunout),
    ("⌰", ToleranceKind::TotalRunout),
    ("◎", ToleranceKind::Concentricity),
    ("◎", ToleranceKind::Coaxiality),
];

/// SFA's tolerance text: optional dimension lines, the frame `[⌮ |][⭩◎ |] sym | tol | [max
/// |] datums…`, optional `▽ ⎹ [X]` datum feature lines and `(composite)`.
fn parse_gtol(text: &str) -> Option<(GtolF, Option<DimF>)> {
    let lines: Vec<&str> = text.lines().collect();
    let fi = lines.iter().position(|l| l.contains('|'))?;
    let dim = if fi > 0 {
        Some(parse_dim(&lines[..fi].join(" "))?)
    } else {
        None
    };
    let mut cells: Vec<&str> = lines[fi].split('|').map(str::trim).collect();
    let mut g = GtolF {
        sym: String::new(),
        all_around: false,
        all_over: false,
        zone: String::new(),
        magnitude: None,
        mods: Vec::new(),
        projected: None,
        unequal: None,
        maximum: None,
        per_unit: None,
        datums: Vec::new(),
        datum_feature: None,
        composite: false,
    };
    loop {
        match cells.first().copied() {
            Some("⌮") => g.all_around = true,
            Some("⭩◎") => g.all_over = true,
            _ => break,
        }
        cells.remove(0);
    }
    g.sym = cells.first()?.to_string();
    let toks: Vec<&str> = cells.get(1)?.split_whitespace().collect();
    let mut i = 0;
    while i < toks.len() {
        let t = toks[i];
        if g.magnitude.is_none() {
            let (z, r) = strip_prefix_sym(t);
            g.zone = z;
            g.magnitude = Some(num(r)?);
        } else if t == "Ⓟ" {
            g.projected = Some(num(toks.get(i + 1)?)?);
            i += 1;
        } else if t == "Ⓤ" {
            g.unequal = Some(num(toks.get(i + 1)?)?);
            i += 1;
        } else if t == "/" {
            let (_, r) = strip_prefix_sym(toks.get(i + 1)?);
            g.per_unit = Some(num(r)?);
            i += 1;
        } else {
            g.mods.push(t.to_string());
        }
        i += 1;
    }
    for c in &cells[2..] {
        if c.is_empty() {
            continue;
        }
        if let Some(m) = c.strip_suffix("MAX") {
            let (_, r) = strip_prefix_sym(m.trim());
            g.maximum = Some(num(r)?);
            continue;
        }
        g.datums.push(c.replace(' ', ""));
    }
    for l in &lines[fi + 1..] {
        let l = l.trim();
        if l == "(composite)" {
            g.composite = true;
        } else if let Some(x) = l.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
            g.datum_feature = Some(x.to_string());
        }
    }
    g.mods.sort();
    Some((g, dim))
}

fn parse_target(text: &str) -> Option<TargetF> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let (size, rest) = if lines.len() == 2 {
        let (_, r) = strip_prefix_sym(lines[0]);
        (Some(num(r)?), lines[1])
    } else {
        (None, lines[0])
    };
    let mut toks = rest.split_whitespace();
    let label = toks.next()?.to_string();
    let mut shape = String::new();
    let mut movable = false;
    for t in toks {
        match t {
            "(movable)" => movable = true,
            t => shape = t.trim_matches(|c| c == '(' || c == ')').to_string(),
        }
    }
    if size.is_some() && shape.is_empty() {
        shape = "circle".into();
    }
    Some(TargetF {
        label,
        shape,
        size,
        movable,
    })
}

fn tol_symbol(k: ToleranceKind) -> &'static str {
    TOL_SYMBOLS
        .iter()
        .find(|(_, x)| *x == k)
        .map_or("?", |(s, _)| s)
}

fn datum_cell(p: &PartPmi, c: &pmi::Compartment) -> String {
    let mut parts = Vec::new();
    for r in &c.references {
        let mut s = p.datums[r.datum.0].label().to_string();
        for m in &r.modifiers {
            s.push_str(&datum_mod(m));
        }
        parts.push(s);
    }
    let mut s = parts.join("-");
    for m in &c.modifiers {
        s.push_str(&datum_mod(m));
    }
    s
}

fn datum_mod(m: &DatumModifier) -> String {
    match m {
        DatumModifier::Simple(SimpleDatumModifier::MaximumMaterialRequirement) => "Ⓜ".into(),
        DatumModifier::Simple(SimpleDatumModifier::LeastMaterialRequirement) => "Ⓛ".into(),
        DatumModifier::Simple(SimpleDatumModifier::FreeState) => "Ⓕ".into(),
        DatumModifier::Simple(s) => format!("[{s:?}]"),
        DatumModifier::WithValue { kind, value } => format!("[{kind:?} {}]", value.decimal()),
    }
}

fn model_dim(p: &PartPmi, d: &pmi::Dimension) -> DimF {
    let (sym, angle) = match &d.kind {
        DimensionKind::Size { kind, angle, .. } => (
            match kind {
                SizeKind::Diameter => "⌀",
                SizeKind::SphericalDiameter => "S⌀",
                SizeKind::Radius if d.modifiers.contains(&DimensionModifier::ControlledRadius) => {
                    "CR"
                }
                SizeKind::Radius => "R",
                SizeKind::SphericalRadius => "SR",
                _ => "",
            },
            angle.is_some(),
        ),
        DimensionKind::Location { angle, .. } => ("", angle.is_some()),
    };
    let _ = p;
    let mut nominal = d.nominal.as_ref().map(shown);
    let tol = match &d.tolerance {
        DimTolerance::None => TolF::None,
        DimTolerance::Basic => TolF::Basic,
        DimTolerance::Deviations(b) => {
            let (u, l) = (shown(b.upper()), shown(b.lower()));
            if b.upper().decimal().to_f64() == -b.lower().decimal().to_f64() {
                TolF::PlusMinus(u)
            } else {
                TolF::Dev(u, l)
            }
        }
        DimTolerance::Limits(b) => {
            nominal = None;
            TolF::Limits(shown(b.lower()), shown(b.upper()))
        }
        DimTolerance::Fit { class, .. } => TolF::Fit(class.to_string()),
    };
    DimF {
        sym: sym.into(),
        angle,
        nominal,
        tol,
        qualifier: match &d.qualifier {
            Some(Qualifier::Maximum) => Some("maximum".into()),
            Some(Qualifier::Minimum) => Some("minimum".into()),
            Some(Qualifier::Average) => Some("average".into()),
            Some(Qualifier::Other(o)) => Some(o.clone()),
            None => None,
        },
        reference: d.modifiers.contains(&DimensionModifier::Reference),
        statistical: d.modifiers.contains(&DimensionModifier::Statistical),
    }
}

/// The set of faces and edges a feature covers, for "same feature" by content.
fn cover(p: &PartPmi, f: pmi::FeatureId) -> Vec<pmi::Anchor> {
    let mut out = Vec::new();
    let mut stack = vec![f];
    while let Some(f) = stack.pop() {
        match &p.features[f.0] {
            Feature::Items(a) => out.extend(a.iter().copied()),
            Feature::Group { members, .. } => stack.extend(members.iter().copied()),
            Feature::Derived { from, .. } => stack.extend(from.iter().copied()),
        }
    }
    out.sort();
    out.dedup();
    out
}

fn target_feature(p: &PartPmi, t: &ToleranceTarget) -> Option<pmi::FeatureId> {
    match t {
        ToleranceTarget::Feature(f) => Some(*f),
        ToleranceTarget::Dimension(d) => match &p.dimensions[d.0].kind {
            DimensionKind::Size { feature, .. } => Some(*feature),
            DimensionKind::Location { .. } => None,
        },
        _ => None,
    }
}

fn model_gtol(p: &PartPmi, i: usize, t: &pmi::GeometricTolerance) -> GtolF {
    let feature = target_feature(p, &t.target);
    let all_around = matches!(
        feature.map(|f| &p.features[f.0]),
        Some(Feature::Group {
            kind: pmi::GroupKind::AllAround,
            ..
        })
    );
    let mut mods: Vec<String> = t
        .modifiers
        .iter()
        .map(|m| {
            MOD_SYMBOLS
                .iter()
                .find(|(_, x)| x == m)
                .map_or_else(|| format!("[{m:?}]"), |(s, _)| s.to_string())
        })
        .collect();
    mods.sort();
    let datum_feature = feature.and_then(|f| {
        let c = cover(p, f);
        p.datums
            .iter()
            .find(|d| d.feature().is_some_and(|df| cover(p, df) == c))
            .map(|d| d.label().to_string())
    });
    let composite = p
        .tolerance_relations
        .iter()
        .any(|r| r.kind == pmi::RelationKind::Composite && r.related.0 == i);
    GtolF {
        sym: tol_symbol(t.kind).into(),
        all_around,
        all_over: t.target == ToleranceTarget::WholePart,
        zone: match &t.zone {
            Some(z) if z.form.is_diametral() => "⌀".into(),
            Some(z) if z.form.is_spherical() => "S⌀".into(),
            _ => String::new(),
        },
        magnitude: t.magnitude.as_ref().map(shown),
        mods,
        projected: t
            .zone
            .as_ref()
            .and_then(|z| z.projected.as_ref())
            .map(|p| shown(&p.length)),
        unequal: t.unequal.as_ref().map(shown),
        maximum: t.maximum.as_ref().map(shown),
        per_unit: t.unit_basis.as_ref().map(|u| shown(&u.size)),
        datums: t
            .datums
            .as_ref()
            .map(|s| s.compartments().iter().map(|c| datum_cell(p, c)).collect())
            .unwrap_or_default(),
        datum_feature,
        composite,
    }
}

fn model_target(p: &PartPmi, t: &pmi::DatumTarget) -> TargetF {
    let label = p
        .datums
        .iter()
        .find(|d| {
            d.targets()
                .iter()
                .any(|x| std::ptr::eq(&p.datum_targets[x.0], t))
        })
        .map_or("?".to_string(), |d| d.label().to_string());
    let (shape, size) = match t.shape() {
        TargetShape::Point => ("point", None),
        TargetShape::Line { length } => ("line", Some(length)),
        TargetShape::Rectangle { length, .. } => ("rectangle", Some(length)),
        TargetShape::Circle { diameter } => ("circle", Some(diameter)),
        TargetShape::CircularCurve { diameter } => ("circular curve", Some(diameter)),
        TargetShape::Area(_) => ("area", None),
        TargetShape::Curve(_) => ("curve", None),
    };
    TargetF {
        label: format!("{label}{}", t.number()),
        shape: shape.into(),
        size: size.map(|l| {
            shown(&pmi::Value {
                quantity: pmi::Quantity::Length(l.clone()),
                decimal_places: None,
            })
        }),
        movable: t.movable().is_some(),
    }
}

/// One difference between NIST's expected PMI and the reader: its key (what it concerns) and a
/// description.
fn compare_nist(
    model: &str,
    expected: &Json,
    r: &PmiRead,
    doc: &Document,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let p = &r.parts[0];
    // Model facts.
    let mut dims: Vec<Option<DimF>> = p.dimensions.iter().map(|d| Some(model_dim(p, d))).collect();
    let mut gtols: Vec<Option<GtolF>> = p
        .tolerances
        .iter()
        .enumerate()
        .map(|(i, t)| Some(model_gtol(p, i, t)))
        .collect();
    let mut targets: Vec<Option<TargetF>> = p
        .datum_targets
        .iter()
        .map(|t| Some(model_target(p, t)))
        .collect();
    let prov = &r.provenance.parts[0];
    let dim_id = |i: usize| {
        root(
            doc,
            &prov.dimensions[i],
            &["dimensional_size", "dimensional_location"],
        )
    };
    let tol_id = |i: usize| root(doc, &prov.tolerances[i], &["geometric_tolerance"]);
    let mut systems: Vec<String> = Vec::new();
    for t in &p.tolerances {
        if let Some(s) = &t.datums {
            let k = s
                .compartments()
                .iter()
                .map(|c| datum_cell(p, c))
                .collect::<Vec<_>>()
                .join("|");
            if !systems.contains(&k) {
                systems.push(k);
            }
        }
    }
    let mut systems: Vec<Option<String>> = systems.into_iter().map(Some).collect();
    let text =
        |n: &Num, m: &Num, what: &str, row: u64, at: &str, out: &mut Vec<(String, String)>| {
            // Decimal places are compared where the file states them (a value format qualifier);
            // where it does not, SFA's padding is its own presentation.
            if m.places.is_some() && n.places != m.places {
                out.push((
                    format!("{model} row {row} {what} places"),
                    format!("expected {:?} places, read {:?} ({at})", n.places, m.places),
                ));
            }
        };
    for row in expected["rows"].as_array().unwrap() {
        let n = row["row"].as_u64().unwrap();
        let ty = row["cells"]["A"].as_str().unwrap();
        let b = row["cells"]["B"].as_str().unwrap();
        let key = format!("{model} row {n}");
        if ty == "datum_system" {
            let want = b.replace(' ', "");
            match systems
                .iter_mut()
                .find(|s| s.as_deref() == Some(want.as_str()))
            {
                Some(s) => *s = None,
                None => out.push((key, format!("datum system {b:?} not read"))),
            }
        } else if ty == "dimensional_characteristic_representation" {
            let Some(want) = parse_dim(b) else {
                out.push((key, format!("unparsed {b:?}")));
                continue;
            };
            match dims.iter().position(|d| d.as_ref() == Some(&want)) {
                Some(di) => {
                    let got = dims[di].take().unwrap();
                    let at = format!(" #{}", dim_id(di));
                    if let (Some(a), Some(b)) = (&want.nominal, &got.nominal) {
                        text(a, b, "nominal", n, &at, &mut out);
                    }
                    match (&want.tol, &got.tol) {
                        (TolF::PlusMinus(a), TolF::PlusMinus(b)) => {
                            text(a, b, "tolerance", n, &at, &mut out)
                        }
                        (TolF::Dev(a, c), TolF::Dev(b, d))
                        | (TolF::Limits(a, c), TolF::Limits(b, d)) => {
                            text(a, b, "upper", n, &at, &mut out);
                            text(c, d, "lower", n, &at, &mut out);
                        }
                        _ => {}
                    }
                }
                None => out.push((key, format!("dimension {b:?} not read ({want:?})"))),
            }
        } else if ty.contains("datum_target") {
            let Some(want) = parse_target(b) else {
                out.push((key, format!("unparsed {b:?}")));
                continue;
            };
            match targets.iter_mut().find(|t| t.as_ref() == Some(&want)) {
                Some(t) => *t = None,
                None => out.push((key, format!("datum target {b:?} not read ({want:?})"))),
            }
        } else if ty.contains("tolerance") {
            let Some((want, wdim)) = parse_gtol(b) else {
                out.push((key, format!("unparsed {b:?}")));
                continue;
            };
            let found = gtols.iter().position(|g| g.as_ref() == Some(&want));
            match found {
                Some(i) => {
                    let got = gtols[i].take().unwrap();
                    let at = format!(" #{}", tol_id(i));
                    if let (Some(a), Some(b)) = (&want.magnitude, &got.magnitude) {
                        text(a, b, "magnitude", n, &at, &mut out);
                    }
                    if let Some(wd) = wdim {
                        // The dimension SFA shows with the frame, which SFA associates by
                        // geometry: one equal to it on faces the toleranced feature shares.
                        let t = &p.tolerances[i];
                        let c = target_feature(p, &t.target)
                            .map(|f| cover(p, f))
                            .unwrap_or_default();
                        let ok = p.dimensions.iter().any(|d| {
                            let fs: Vec<pmi::FeatureId> = match &d.kind {
                                DimensionKind::Size { feature, .. } => vec![*feature],
                                DimensionKind::Location { from, to, .. } => vec![*from, *to],
                            };
                            model_dim(p, d) == wd
                                && fs
                                    .iter()
                                    .any(|f| cover(p, *f).iter().any(|a| c.contains(a)))
                        });
                        if !ok {
                            out.push((
                                format!("{key} dimension"),
                                format!("no dimension {wd:?} on the toleranced feature's faces"),
                            ));
                        }
                    }
                }
                None => out.push((key, format!("tolerance {b:?} not read ({want:?})"))),
            }
        } else {
            out.push((key, format!("row type {ty:?} not compared")));
        }
    }
    for (i, d) in dims.iter().enumerate() {
        if let Some(d) = d {
            out.push((
                format!("{model} extra dimension #{}", dim_id(i)),
                format!("{d:?}"),
            ));
        }
    }
    for (i, g) in gtols.iter().enumerate() {
        if let Some(g) = g {
            out.push((
                format!("{model} extra tolerance #{}", tol_id(i)),
                format!("{g:?}"),
            ));
        }
    }
    for (i, t) in targets.iter().enumerate() {
        if let Some(t) = t {
            let id = root(doc, &prov.datum_targets[i], &["datum_target"]);
            out.push((
                format!("{model} extra datum target #{id}"),
                format!("{t:?}"),
            ));
        }
    }
    for s in systems.into_iter().flatten() {
        out.push((format!("{model} extra datum system {s}"), String::new()));
    }
    out
}

/// Pinned differences of one list: `[{file?, key, verdict, reason}]`.
fn pinned(list: &str) -> BTreeMap<String, Json> {
    let path = common::fixtures().join("references/pmi.json");
    let all: Json = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let entries = all[list].as_array().cloned().unwrap_or_default();
    common::check_verdicts(&format!("references/pmi.json {list}"), &entries);
    let mut out = BTreeMap::new();
    for e in entries {
        let k = e["key"].as_str().unwrap().to_string();
        assert!(
            out.insert(k.clone(), e).is_none(),
            "references/pmi.json {list}: {k} listed twice"
        );
    }
    out
}

/// Every difference is pinned, and every pin is a current difference.
fn check_pinned(list: &str, diffs: &[(String, String)]) {
    let pins = pinned(list);
    let mut seen = std::collections::BTreeSet::new();
    let mut unexplained = Vec::new();
    for (k, d) in diffs {
        if !seen.insert(k.clone()) {
            unexplained.push(format!("{k} (twice): {d}"));
            continue;
        }
        if !pins.contains_key(k) {
            unexplained.push(format!("{k}: {d}"));
        }
    }
    let stale: Vec<&String> = pins.keys().filter(|k| !seen.contains(*k)).collect();
    if !unexplained.is_empty() || !stale.is_empty() {
        let dump: Vec<Json> = diffs
            .iter()
            .map(|(k, d)| json!({"key": k, "difference": d}))
            .collect();
        let _ = std::fs::write(
            std::env::temp_dir().join(format!("pmi_{list}_differences.json")),
            serde_json::to_string_pretty(&dump).unwrap(),
        );
    }
    assert!(
        unexplained.is_empty(),
        "{list}: {} unexplained differences:\n{}",
        unexplained.len(),
        unexplained.join("\n")
    );
    assert!(
        stale.is_empty(),
        "{list}: pinned but no longer different: {stale:?}"
    );
}

fn nist_fixture(model: &str) -> (Json, PmiRead, Document) {
    let dir = common::fixtures().join("ap242/nist");
    let expected: Json = serde_json::from_str(
        &std::fs::read_to_string(dir.join(format!("{model}.expected.json"))).unwrap(),
    )
    .unwrap();
    let file = expected["step_file"].as_str().unwrap();
    let (r, doc) = read_doc(file_bytes(&dir.join(format!("{file}.gz"))));
    (expected, r, doc)
}

const NIST_MODELS: [&str; 7] = [
    "nist_ctc_01",
    "nist_ctc_02",
    "nist_ftc_07",
    "nist_ftc_10",
    "nist_stc_06",
    "nist_stc_09",
    "nist_stc_10",
];

#[test]
fn committed_nist_datum_features_resolve_to_faces() {
    for model in NIST_MODELS {
        let (_, read, _) = nist_fixture(model);
        // CTC-02 A/B/C/K are established only by placed targets, with no datum feature.
        // Target placements are not faces and must not be silently treated as such.
        let expected = if model == "nist_ctc_02" {
            ["A", "B", "C", "K"]
                .map(|label| (pmi::PartId(0), label.to_string()))
                .into_iter()
                .collect()
        } else {
            Default::default()
        };
        assert_eq!(common::pmi_datums::unresolved(&read), expected, "{model}");
    }
}

#[test]
fn solid_edge_reexport_loses_exactly_datum_b_feature() {
    use std::collections::BTreeSet;

    for (file, missing) in [
        ("01-NIST-control-ctc01.stp.gz", BTreeSet::new()),
        (
            "09-NIST-reexported-by-SE.stp.gz",
            BTreeSet::from([(pmi::PartId(0), "B".to_string())]),
        ),
    ] {
        let read = read_bytes(file_bytes(
            &common::fixtures().join("ap242/reexport").join(file),
        ));
        assert_eq!(read.parts.len(), 1, "{file}");
        // The invalid B is omitted by the reader and must be named by its finding below.
        let labels: BTreeSet<_> = read.parts[0]
            .datums
            .iter()
            .map(|datum| datum.label().to_string())
            .collect();
        let expected_labels = if missing.is_empty() {
            BTreeSet::from(["A".into(), "B".into(), "C".into()])
        } else {
            BTreeSet::from(["A".into(), "C".into()])
        };
        assert_eq!(labels, expected_labels, "{file}");
        assert_eq!(common::pmi_datums::unresolved(&read), missing, "{file}");
        let datum_findings: Vec<_> = read
            .findings
            .iter()
            .filter(|f| f.kind == pmi::FindingKind::Unresolved && f.entity == "datum")
            .map(|f| (f.kind, f.detail.as_str()))
            .collect();
        let expected = if missing.is_empty() {
            vec![]
        } else {
            vec![(
                pmi::FindingKind::Unresolved,
                "datum B is not read: what establishes it does not resolve",
            )]
        };
        assert_eq!(datum_findings, expected, "{file}");
    }
}

/// Oracle 1: NIST's expected PMI. Every row is found with equal semantics (values to 1e-12
/// relative, decimal places as shown, datum frames in order, modifiers, classes, targets), every
/// read item is a row; each difference is pinned in `references/pmi.json` "nist" with its verdict.
#[test]
fn nist_expected_pmi() {
    let mut diffs = Vec::new();
    for m in NIST_MODELS {
        let (expected, r, doc) = nist_fixture(m);
        assert_eq!(r.parts.len(), 1, "{m}: one part");
        diffs.extend(compare_nist(m, &expected, &r, &doc));
    }
    check_pinned("nist", &diffs);
}

// ---------------------------------------------------------------------------------------------
// OpenCascade XCAF's reading against the reader
// ---------------------------------------------------------------------------------------------

fn load_gz_json(path: &Path) -> Json {
    serde_json::from_slice(&file_bytes(path)).unwrap()
}

/// A value OpenCascade gives against one the reader read: equal in millimetres (degrees for
/// angles), or equal only in the file's own unit (OpenCascade did not convert it).
fn occt_value(occt: f64, v: &pmi::Value) -> Option<&'static str> {
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0);
    let si = match &v.quantity {
        pmi::Quantity::Length(l) => l.mm(),
        pmi::Quantity::Angle(a) => a.rad().to_degrees(),
    };
    if close(occt, si) {
        None
    } else if close(occt, v.decimal().to_f64()) {
        Some("in the file's unit, not converted")
    } else {
        Some("different")
    }
}

fn face_cover(p: &PartPmi, f: pmi::FeatureId) -> Vec<usize> {
    let mut v: Vec<usize> = cover(p, f)
        .into_iter()
        .map(|a| match a {
            pmi::Anchor::Face(i) => i.0,
            pmi::Anchor::Edge(e) => usize::MAX - e.0,
            pmi::Anchor::Geometry(g) => usize::MAX / 2 - g.0,
        })
        .collect();
    v.sort_unstable();
    v
}

fn occt_dim_type(d: &pmi::Dimension) -> String {
    match &d.kind {
        DimensionKind::Size { angle: Some(_), .. } => "Size_Angular".into(),
        DimensionKind::Size { kind, .. } => {
            let camel: String = kind
                .name()
                .split(' ')
                .map(|w| {
                    let mut c = w.chars();
                    c.next()
                        .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                        .unwrap_or_default()
                })
                .collect();
            format!("Size_{camel}")
        }
        DimensionKind::Location { angle: Some(_), .. } => "Location_Angular".into(),
        DimensionKind::Location { kind, .. } => match kind {
            pmi::LocationKind::LinearDistance => "Location_LinearDistance".into(),
            pmi::LocationKind::CurvedDistance => "Location_CurvedDistance".into(),
            k => {
                let n = k.name().trim_start_matches("linear distance ");
                let w: Vec<&str> = n.split(' ').collect();
                let cap = |x: &str| {
                    match x {
                        "centre" => "Center",
                        "outer" => "Outer",
                        "inner" => "Inner",
                        o => o,
                    }
                    .to_string()
                };
                match w.as_slice() {
                    [a, b] => format!("Location_LinearDistance_From{}To{}", cap(a), cap(b)),
                    [a] => format!("Location_LinearDistance_{}", cap(a)),
                    _ => format!("Location_{}", k.name()),
                }
            }
        },
    }
}

fn occt_tol_type(k: ToleranceKind) -> &'static str {
    match k {
        ToleranceKind::Angularity => "Angularity",
        ToleranceKind::CircularRunout => "CircularRunout",
        ToleranceKind::Coaxiality => "Coaxiality",
        ToleranceKind::Concentricity => "Concentricity",
        ToleranceKind::Cylindricity => "Cylindricity",
        ToleranceKind::Flatness => "Flatness",
        ToleranceKind::LineProfile => "ProfileOfLine",
        ToleranceKind::Parallelism => "Parallelism",
        ToleranceKind::Perpendicularity => "Perpendicularity",
        ToleranceKind::Position => "Position",
        ToleranceKind::Roundness => "CircularityOrRoundness",
        ToleranceKind::Straightness => "Straightness",
        ToleranceKind::SurfaceProfile => "ProfileOfSurface",
        ToleranceKind::Symmetry => "Symmetry",
        ToleranceKind::TotalRunout => "TotalRunout",
    }
}

/// The differences between OpenCascade's reading of a dimension and one the reader read on the
/// same faces.
fn occt_dim_fields(o: &Json, p: &PartPmi, d: &pmi::Dimension) -> Vec<String> {
    let mut out = Vec::new();
    let t = o["type"].as_str().unwrap();
    if t != occt_dim_type(d) {
        out.push(format!("type {t} vs {}", occt_dim_type(d)));
    }
    let _ = p;
    let num = |k: &str| o[k].as_f64().unwrap_or(0.0);
    let cmp = |what: &str, occt: f64, v: &pmi::Value| {
        occt_value(occt, v)
            .map(|why| format!("{what} {occt} vs {} {} ({why})", v.decimal(), unit_name(v)))
    };
    match &d.tolerance {
        DimTolerance::Limits(b) => {
            if !o["range"].as_bool().unwrap_or(false) {
                out.push("not a range".into());
            }
            out.extend(cmp("lower_bound", num("lower_bound"), b.lower()));
            out.extend(cmp("upper_bound", num("upper_bound"), b.upper()));
        }
        DimTolerance::Deviations(b) => {
            if let Some(n) = &d.nominal {
                out.extend(cmp("value", num("value"), n));
            }
            if !o["plus_minus"].as_bool().unwrap_or(false) {
                out.push(format!(
                    "not plus/minus (range {} lower_bound {} upper_bound {})",
                    o["range"],
                    num("lower_bound"),
                    num("upper_bound")
                ));
            } else {
                out.extend(cmp("upper_tol", num("upper_tol"), b.upper()));
                let mut lower = b.lower().clone();
                // OpenCascade gives the lower deviation as a magnitude below nominal.
                let neg = format!("{}", -b.lower().decimal().to_f64());
                let neg = pmi::Decimal::parse(&neg).unwrap_or_else(|_| b.lower().decimal().clone());
                match &mut lower.quantity {
                    pmi::Quantity::Length(l) => l.value = neg,
                    pmi::Quantity::Angle(a) => a.value = neg,
                }
                out.extend(cmp("lower_tol", num("lower_tol"), &lower));
            }
        }
        DimTolerance::Fit { class, .. } => {
            if let Some(n) = &d.nominal {
                out.extend(cmp("value", num("value"), n));
            }
            if !o["class_of_tolerance"].as_bool().unwrap_or(false) {
                out.push(format!("class {class} not read as a class"));
            } else {
                let c = &o["class"];
                let theirs = format!(
                    "{}{}",
                    c["form_variance"].as_str().unwrap_or(""),
                    c["grade"].as_str().unwrap_or("").trim_start_matches("IT")
                );
                let hole = c["is_hole"].as_u64() == Some(1);
                let ours_hole = class.deviation.of == pmi::FitFeature::Hole;
                if !theirs.eq_ignore_ascii_case(&class.to_string()) || hole != ours_hole {
                    out.push(format!("class {c} vs {class}"));
                }
            }
        }
        DimTolerance::None | DimTolerance::Basic => {
            match &d.nominal {
                Some(n) => out.extend(cmp("value", num("value"), n)),
                // OpenCascade gives 0 where the file states no value.
                None if num("value") == 0.0 => {}
                None => out.push(format!("value {} vs none stated", num("value"))),
            }
            if o["plus_minus"].as_bool().unwrap_or(false) || o["range"].as_bool().unwrap_or(false) {
                out.push("toleranced in OpenCascade".into());
            }
        }
    }
    out
}

fn unit_name(v: &pmi::Value) -> String {
    match &v.quantity {
        pmi::Quantity::Length(l) => format!("{:?}", l.unit),
        pmi::Quantity::Angle(a) => format!("{:?}", a.unit),
    }
}

fn occt_tol_fields(o: &Json, p: &PartPmi, t: &pmi::GeometricTolerance) -> Vec<String> {
    let mut out = Vec::new();
    match &t.magnitude {
        Some(m) => {
            if let Some(why) = occt_value(o["value"].as_f64().unwrap_or(0.0), m) {
                out.push(format!("value {} vs {} ({why})", o["value"], m.decimal()));
            }
        }
        None => out.push(format!("value {} vs none", o["value"])),
    }
    let mm = o["material_modifier"].as_str().unwrap_or("None");
    let ours = if t
        .modifiers
        .contains(&ToleranceModifier::MaximumMaterialRequirement)
    {
        "M"
    } else if t
        .modifiers
        .contains(&ToleranceModifier::LeastMaterialRequirement)
    {
        "L"
    } else {
        "None"
    };
    if mm != ours {
        out.push(format!("material modifier {mm} vs {ours}"));
    }
    let tv = o["type_of_value"].as_str().unwrap_or("None");
    let zone = match &t.zone {
        Some(z) if z.form.is_diametral() => "Diameter",
        Some(z) if z.form.is_spherical() => "SphericalDiameter",
        _ => "None",
    };
    if tv != zone {
        out.push(format!("type of value {tv} vs {zone}"));
    }
    let projected = t.zone.as_ref().and_then(|z| z.projected.as_ref());
    match (o["zone_modifier"].as_str().unwrap_or("None"), projected) {
        ("Projected", Some(pz)) => {
            if let Some(why) = occt_value(o["zone_value"].as_f64().unwrap_or(0.0), &pz.length) {
                out.push(format!(
                    "projected length {} vs {} ({why})",
                    o["zone_value"],
                    pz.length.decimal()
                ));
            }
        }
        ("None", None) => {}
        (z, p) => out.push(format!("zone modifier {z} vs projected {}", p.is_some())),
    }
    let theirs: Vec<(String, u64)> = o["datums"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            (
                d["name"].as_str().unwrap().to_string(),
                d["position"].as_u64().unwrap(),
            )
        })
        .collect();
    let mut ours: Vec<(String, u64)> = Vec::new();
    if let Some(s) = &t.datums {
        for (i, c) in s.compartments().iter().enumerate() {
            for r in &c.references {
                ours.push((p.datums[r.datum.0].label().to_string(), i as u64 + 1));
            }
        }
    }
    if theirs != ours {
        out.push(format!("datums {theirs:?} vs {ours:?}"));
    }
    out
}

/// The part (index) and its anchors that OpenCascade's faces and edges are, by their
/// `advanced_face` / edge instances (OpenCascade's shapes need not be the file's product
/// definitions); faces are numbered, edges as `usize::MAX - index`.
fn occt_faces(
    j: &Json,
    parts: &[haecceity::step::PartDefinition],
    r: &PmiRead,
) -> Option<(usize, Vec<usize>)> {
    let mut part = None;
    let mut out = Vec::new();
    for f in j.as_array().unwrap() {
        let (n, edge) = match f["advanced_face"].as_str() {
            Some(a) => (a, false),
            None => (f["instance"].as_str()?, true),
        };
        let n: u64 = n.trim_start_matches('#').parse().ok()?;
        let (pi, i) = parts.iter().enumerate().find_map(|(pi, p)| {
            if edge {
                p.edge_index(n).map(|i| (pi, usize::MAX - i)).or_else(|| {
                    // An edge OpenCascade made of supplemental geometry the reader holds.
                    r.provenance.parts[pi]
                        .geometry_items
                        .iter()
                        .position(|&g| g == n)
                        .map(|g| (pi, usize::MAX / 2 - g))
                })
            } else {
                p.face_index(n).map(|i| (pi, i))
            }
        })?;
        if part.is_some_and(|q| q != pi) {
            return None;
        }
        part = Some(pi);
        out.push(i);
    }
    out.sort_unstable();
    out.dedup();
    Some((part?, out))
}

/// Oracle 3: each item OpenCascade read (on faces of a part, matched by instance) against the
/// reader's item of that part on the same faces, and the reader's items OpenCascade did not
/// read.
fn compare_occt(
    name: &str,
    occt: &Json,
    r: &PmiRead,
    doc: &Document,
    parts: &[haecceity::step::PartDefinition],
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let all = |k: &str| -> Vec<Json> {
        occt["parts"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|p| p[k].as_array().unwrap().iter().cloned())
            .collect()
    };
    let (odims, otols, odatums) = (
        all("dimensions"),
        all("geometric_tolerances"),
        all("datums"),
    );
    for (pi, p) in r.parts.iter().enumerate() {
        let prov = &r.provenance.parts[pi];
        let dim_id = |i: usize| {
            root(
                doc,
                &prov.dimensions[i],
                &["dimensional_size", "dimensional_location"],
            )
        };
        let tol_id = |i: usize| root(doc, &prov.tolerances[i], &["geometric_tolerance"]);
        let on_part = |j: &Json| {
            occt_faces(j, parts, r)
                .filter(|(q, _)| *q == pi)
                .map(|(_, f)| f)
        };
        let mut dims_left: Vec<bool> = vec![true; p.dimensions.len()];
        for o in &odims {
            let t = o["type"].as_str().unwrap();
            let label = o["label"].as_str().unwrap();
            if t == "DimensionPresentation" {
                continue; // presentation only: OpenCascade's record of a callout, no semantics
            }
            let Some(f1) = on_part(&o["faces"]) else {
                continue;
            };
            let f2 = if o["faces2"].as_array().unwrap().is_empty() {
                Vec::new()
            } else {
                match on_part(&o["faces2"]) {
                    Some(f) => f,
                    None => continue,
                }
            };
            let candidates: Vec<usize> = (0..p.dimensions.len())
                .filter(|&i| match &p.dimensions[i].kind {
                    DimensionKind::Size { feature, .. } => {
                        f2.is_empty() && face_cover(p, *feature) == f1
                    }
                    DimensionKind::Location { from, to, .. } => {
                        let (a, b) = (face_cover(p, *from), face_cover(p, *to));
                        (a == f1 && b == f2) || (a == f2 && b == f1)
                    }
                })
                .collect();
            let mut best = candidates
                .iter()
                .filter(|&&i| dims_left[i])
                .min_by_key(|&&i| occt_dim_fields(o, p, &p.dimensions[i]).len())
                .copied();
            let mut subset = false;
            if best.is_none() && f2.is_empty() {
                // OpenCascade may attach a pattern's dimension to some of its faces only.
                best = (0..p.dimensions.len())
                    .filter(|&i| dims_left[i] && occt_dim_type(&p.dimensions[i]) == t)
                    .filter(|&i| match &p.dimensions[i].kind {
                        DimensionKind::Size { feature, .. } => {
                            let c = face_cover(p, *feature);
                            f1.iter().all(|x| c.contains(x))
                        }
                        DimensionKind::Location { .. } => false,
                    })
                    .min_by_key(|&i| occt_dim_fields(o, p, &p.dimensions[i]).len());
                subset = best.is_some();
            }
            match best {
                Some(i) => {
                    dims_left[i] = false;
                    let mut diff = occt_dim_fields(o, p, &p.dimensions[i]);
                    if subset {
                        let c = match &p.dimensions[i].kind {
                            DimensionKind::Size { feature, .. } => face_cover(p, *feature),
                            DimensionKind::Location { .. } => Vec::new(),
                        };
                        diff.insert(
                            0,
                            format!("on faces {f1:?}, a part of the dimensioned feature's {c:?}"),
                        );
                    }
                    if !diff.is_empty() {
                        out.push((
                            format!("{name} occt dimension {label}"),
                            format!("#{}: {}", dim_id(i), diff.join("; ")),
                        ));
                    }
                }
                None => out.push((
                    format!("{name} occt dimension {label}"),
                    format!(
                        "{t} {} on faces {f1:?} {f2:?}: no dimension read on those faces",
                        o["value"]
                    ),
                )),
            }
        }
        for (i, left) in dims_left.iter().enumerate() {
            if *left {
                out.push((
                    format!("{name} occt missing dimension #{}", dim_id(i)),
                    occt_dim_type(&p.dimensions[i]),
                ));
            }
        }
        let mut tols_left = vec![true; p.tolerances.len()];
        for o in &otols {
            let t = o["type"].as_str().unwrap();
            let label = o["label"].as_str().unwrap();
            let f = if o["faces"].as_array().unwrap().is_empty() {
                if pi != 0 {
                    continue;
                }
                Vec::new()
            } else {
                match on_part(&o["faces"]) {
                    Some(f) => f,
                    None => continue,
                }
            };
            let on_faces = |i: usize, exact: bool| -> bool {
                match target_feature(p, &p.tolerances[i].target) {
                    Some(x) => {
                        let c = face_cover(p, x);
                        !exact && !f.is_empty() && c != f && f.iter().all(|y| c.contains(y))
                    }
                    None => false,
                }
            };
            let mut best = (0..p.tolerances.len())
                .filter(|&i| {
                    tols_left[i]
                        && occt_tol_type(p.tolerances[i].kind) == t
                        && match &p.tolerances[i].target {
                            ToleranceTarget::WholePart => f.is_empty(),
                            tt => target_feature(p, tt).is_some_and(|x| face_cover(p, x) == f)
                                || matches!(tt, ToleranceTarget::Dimension(d) if match &p.dimensions[d.0].kind {
                                    DimensionKind::Location { from, to, .. } => {
                                        let mut c = face_cover(p, *from);
                                        c.extend(face_cover(p, *to));
                                        c.sort_unstable();
                                        c.dedup();
                                        c == f
                                    }
                                    _ => false,
                                }),
                        }
                })
                .min_by_key(|&i| occt_tol_fields(o, p, &p.tolerances[i]).len());
            let mut subset = false;
            if best.is_none() {
                best = (0..p.tolerances.len())
                    .filter(|&i| {
                        tols_left[i]
                            && occt_tol_type(p.tolerances[i].kind) == t
                            && on_faces(i, false)
                    })
                    .min_by_key(|&i| occt_tol_fields(o, p, &p.tolerances[i]).len());
                subset = best.is_some();
            }
            match best {
                Some(i) => {
                    tols_left[i] = false;
                    let mut diff = occt_tol_fields(o, p, &p.tolerances[i]);
                    if subset {
                        let c = target_feature(p, &p.tolerances[i].target)
                            .map(|x| face_cover(p, x))
                            .unwrap_or_default();
                        diff.insert(
                            0,
                            format!("on faces {f:?}, a part of the toleranced feature's {c:?}"),
                        );
                    }
                    if !diff.is_empty() {
                        out.push((
                            format!("{name} occt tolerance {label}"),
                            format!("#{}: {}", tol_id(i), diff.join("; ")),
                        ));
                    }
                }
                None => out.push((
                    format!("{name} occt tolerance {label}"),
                    format!(
                        "{t} {} on faces {f:?}: no tolerance read on those faces",
                        o["value"]
                    ),
                )),
            }
        }
        for (i, left) in tols_left.iter().enumerate() {
            if *left {
                out.push((
                    format!("{name} occt missing tolerance #{}", tol_id(i)),
                    format!("{:?}", p.tolerances[i].kind),
                ));
            }
        }
        // Datums by label and faces (OpenCascade keeps one per tolerance that cites it).
        let mut theirs: Vec<(String, Vec<usize>)> = odatums
            .iter()
            .filter(|d| !d["is_target"].as_bool().unwrap_or(false))
            .filter_map(|d| {
                Some((
                    d["name"].as_str().unwrap().to_string(),
                    on_part(&d["faces"])?,
                ))
            })
            .collect();
        theirs.sort();
        theirs.dedup();
        for (i, d) in p.datums.iter().enumerate() {
            let ours = d.feature().map(|f| face_cover(p, f)).unwrap_or_default();
            let key = (d.label().to_string(), ours.clone());
            if let Some(j) = theirs.iter().position(|x| *x == key) {
                theirs.remove(j);
            } else if d.feature().is_some() {
                let id = root(doc, &prov.datums[i], &["datum"]);
                out.push((
                    format!("{name} occt missing datum {} #{id}", d.label()),
                    format!("faces {ours:?}"),
                ));
            }
        }
        for (l, f) in theirs {
            out.push((
                format!("{name} occt datum {l} {f:?}"),
                "a datum the reader does not have on those faces".into(),
            ));
        }
    }
    // Items on faces of no part the reader has.
    for (k, list) in [("dimension", &odims), ("tolerance", &otols)] {
        for o in list.iter() {
            if o["type"] == "DimensionPresentation" || o["faces"].as_array().unwrap().is_empty() {
                continue;
            }
            if occt_faces(&o["faces"], parts, r).is_none() {
                out.push((
                    format!("{name} occt {k} {}", o["label"].as_str().unwrap()),
                    format!(
                        "{} on faces that are not one part's: {}",
                        o["type"], o["faces"]
                    ),
                ));
            }
        }
    }
    // Material.
    let materials: Vec<String> = occt["materials"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["name"] != "pmi-assist answers")
        .map(|m| m["name"].as_str().unwrap().to_string())
        .collect();
    let ours: Vec<String> = r
        .parts
        .iter()
        .filter_map(|p| p.material.as_ref().map(|m| m.id.clone()))
        .collect();
    for m in &materials {
        if !ours.contains(m) {
            out.push((format!("{name} occt material {m}"), "not read".into()));
        }
    }
    out
}

/// The OpenCascade captures there are files for: the committed NIST and specify-core files,
/// and every other NIST file when `HAECCEITY_NIST_PMI` names their directory.
fn occt_cases() -> Vec<(String, std::path::PathBuf)> {
    let dir = common::fixtures().join("ap242/occt");
    let mut out = Vec::new();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for n in names {
        let stem = n.trim_end_matches(".json.gz").to_string();
        let committed_nist = common::fixtures().join(format!("ap242/nist/{stem}.stp.gz"));
        let specify = common::fixtures().join(format!("ap242/specify/{stem}.step.gz"));
        if committed_nist.exists() {
            out.push((stem, committed_nist));
        } else if specify.exists() {
            out.push((stem, specify));
        } else if let Some(d) = nist_dir() {
            out.push((stem.clone(), d.join(format!("{stem}.stp"))));
        }
    }
    out
}

fn nist_dir() -> Option<std::path::PathBuf> {
    let d = std::env::var_os("HAECCEITY_NIST_PMI").map(std::path::PathBuf::from);
    match d {
        Some(d) if d.is_dir() => Some(d),
        _ => {
            assert!(
                std::env::var_os("HAECCEITY_NIST_PMI_REQUIRED").is_none_or(|v| v != "1"),
                "HAECCEITY_NIST_PMI_REQUIRED is set but HAECCEITY_NIST_PMI names no directory"
            );
            None
        }
    }
}

#[test]
fn opencascade_cross_check() {
    let mut diffs = Vec::new();
    let mut covered = std::collections::BTreeSet::new();
    for (stem, path) in occt_cases() {
        let occt = load_gz_json(&common::fixtures().join(format!("ap242/occt/{stem}.json.gz")));
        let bytes = file_bytes(&path);
        let Ok(parts) = read_part_definitions(&bytes) else {
            diffs.push((
                format!("{stem} occt part definitions"),
                "no B-rep part to anchor PMI to".into(),
            ));
            covered.insert(stem);
            continue;
        };
        let doc = Document::parse(bytes).unwrap();
        let r = pmi::read(&doc, &parts).unwrap();
        diffs.extend(compare_occt(&stem, &occt, &r, &doc, &parts));
        covered.insert(stem);
    }
    // Pins of files not checked in this run are not judged.
    let pins = pinned("occt");
    let judged: Vec<(String, String)> = diffs;
    let relevant: BTreeMap<String, Json> = pins
        .into_iter()
        .filter(|(k, _)| covered.iter().any(|c| k.starts_with(&format!("{c} "))))
        .collect();
    check_against("occt", &judged, &relevant);
}

/// [`check_pinned`] against a given subset of pins.
fn check_against(list: &str, diffs: &[(String, String)], pins: &BTreeMap<String, Json>) {
    let mut seen = std::collections::BTreeSet::new();
    let mut unexplained = Vec::new();
    for (k, d) in diffs {
        if !seen.insert(k.clone()) {
            unexplained.push(format!("{k} (twice): {d}"));
        } else if !pins.contains_key(k) {
            unexplained.push(format!("{k}: {d}"));
        }
    }
    let stale: Vec<&String> = pins.keys().filter(|k| !seen.contains(*k)).collect();
    if !unexplained.is_empty() || !stale.is_empty() {
        let dump: Vec<Json> = diffs
            .iter()
            .map(|(k, d)| json!({"key": k, "difference": d}))
            .collect();
        let _ = std::fs::write(
            std::env::temp_dir().join(format!("pmi_{list}_differences.json")),
            serde_json::to_string_pretty(&dump).unwrap(),
        );
    }
    assert!(
        unexplained.is_empty(),
        "{list}: {} unexplained differences:\n{}",
        unexplained.len(),
        unexplained.join("\n")
    );
    assert!(
        stale.is_empty(),
        "{list}: pinned but no longer different: {stale:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// specify-core-written files against the intent they were written from
// ---------------------------------------------------------------------------------------------

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

fn sorted_faces(j: &Json) -> Vec<usize> {
    let mut v: Vec<usize> = j
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap() as usize)
        .collect();
    v.sort_unstable();
    v
}

fn attr_text<'a>(a: &'a pmi::AttributeSet, k: &str) -> Option<&'a pmi::AttributeValue> {
    a.items.iter().find(|(n, _)| n == k).map(|(_, v)| v)
}

/// Oracle 5: the PMI read from each specify-core output against the intent's requirements and
/// datums (faces are numbered alike); differences pinned in `references/pmi.json` "specify".
fn compare_specify(case: &str, intent: &Json, p: &PartPmi, pi: usize) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let key = |what: String| format!("{case} part {pi} {what}");
    let mut used_dims = vec![false; p.dimensions.len()];
    let mut used_tols = vec![false; p.tolerances.len()];
    let mut used_datums = vec![false; p.datums.len()];
    for d in intent["datums"].as_array().unwrap() {
        let l = d["letter"].as_str().unwrap();
        let faces = sorted_faces(&d["faces"]);
        match p.datums.iter().position(|x| x.label().as_str() == l) {
            Some(i) => {
                used_datums[i] = true;
                let got = p.datums[i]
                    .feature()
                    .map(|f| face_cover(p, f))
                    .unwrap_or_default();
                if got != faces {
                    out.push((
                        key(format!("datum {l}")),
                        format!("faces {got:?}, intent {faces:?}"),
                    ));
                }
            }
            None => out.push((key(format!("datum {l}")), "not read".into())),
        }
    }
    for (ri, r) in intent["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let kind = r["kind"].as_str().unwrap();
        let faces = sorted_faces(&r["faces"]);
        let k = key(format!("requirement {ri} {kind} {faces:?}"));
        let num = |n: &str| r[n].as_f64();
        match kind {
            "size" => {
                let found = (0..p.dimensions.len()).find(|&i| {
                    !used_dims[i]
                        && matches!(&p.dimensions[i].kind, DimensionKind::Size { feature, .. } if face_cover(p, *feature) == faces)
                        && p.dimensions[i].nominal.as_ref().is_some_and(|n| close(n.si(), num("nominal").unwrap()))
                });
                let Some(i) = found else {
                    out.push((k, "no size dimension read on those faces".into()));
                    continue;
                };
                used_dims[i] = true;
                let d = &p.dimensions[i];
                match &d.tolerance {
                    DimTolerance::Deviations(b) => {
                        if !close(b.upper().si(), num("upper").unwrap())
                            || !close(b.lower().si(), num("lower").unwrap())
                        {
                            out.push((
                                k.clone(),
                                format!(
                                    "deviations {} {}, intent {} {}",
                                    b.upper().decimal(),
                                    b.lower().decimal(),
                                    r["upper"],
                                    r["lower"]
                                ),
                            ));
                        }
                        if let Some(f) = r["fit"].as_str() {
                            out.push((
                                format!("{k} fit"),
                                format!("{f} stated as deviations only, no limits_and_fits"),
                            ));
                        }
                    }
                    DimTolerance::Fit { class, .. } => {
                        if Some(class.to_string().as_str()) != r["fit"].as_str() {
                            out.push((k, format!("class {class}, intent {}", r["fit"])));
                        }
                    }
                    t => out.push((k, format!("tolerance {t:?}"))),
                }
            }
            "location" => {
                let reference = sorted_faces(&r["reference"]);
                let found = (0..p.dimensions.len()).find(|&i| {
                    !used_dims[i]
                        && match &p.dimensions[i].kind {
                            DimensionKind::Location { from, to, .. } => {
                                let (a, b) = (face_cover(p, *from), face_cover(p, *to));
                                (a == faces && b == reference) || (a == reference && b == faces)
                            }
                            _ => false,
                        }
                        && p.dimensions[i]
                            .nominal
                            .as_ref()
                            .is_some_and(|n| close(n.si(), num("distance").unwrap()))
                });
                let Some(i) = found else {
                    out.push((k, "no location dimension read between those faces".into()));
                    continue;
                };
                used_dims[i] = true;
                let basic = r["basic"].as_bool().unwrap_or(false);
                if basic != (p.dimensions[i].tolerance == DimTolerance::Basic) {
                    out.push((
                        k,
                        format!(
                            "tolerance {:?}, intent basic {basic}",
                            p.dimensions[i].tolerance
                        ),
                    ));
                }
            }
            "position" | "runout" | "flatness" | "perpendicularity" => {
                let want = match kind {
                    "position" => ToleranceKind::Position,
                    "runout" => ToleranceKind::CircularRunout,
                    "flatness" => ToleranceKind::Flatness,
                    _ => ToleranceKind::Perpendicularity,
                };
                let found = (0..p.tolerances.len()).find(|&i| {
                    !used_tols[i]
                        && p.tolerances[i].kind == want
                        && target_feature(p, &p.tolerances[i].target)
                            .is_some_and(|f| face_cover(p, f) == faces)
                });
                let Some(i) = found else {
                    out.push((k, "no such tolerance read on those faces".into()));
                    continue;
                };
                used_tols[i] = true;
                let t = &p.tolerances[i];
                let mut why = Vec::new();
                if !t
                    .magnitude
                    .as_ref()
                    .is_some_and(|m| close(m.si(), num("tolerance").unwrap()))
                {
                    why.push(format!(
                        "magnitude {:?}",
                        t.magnitude.as_ref().map(|m| m.decimal().to_string())
                    ));
                }
                let diametral = t.zone.as_ref().is_some_and(|z| z.form.is_diametral());
                if diametral != r["diametral"].as_bool().unwrap_or(false) {
                    why.push(format!("diametral {diametral}"));
                }
                let mmc = t
                    .modifiers
                    .contains(&ToleranceModifier::MaximumMaterialRequirement);
                if mmc != r["mmc"].as_bool().unwrap_or(false) {
                    why.push(format!("mmc {mmc}"));
                }
                let ours: Vec<String> = t
                    .datums
                    .as_ref()
                    .map(|s| s.compartments().iter().map(|c| datum_cell(p, c)).collect())
                    .unwrap_or_default();
                let theirs: Vec<String> = r["datums"]
                    .as_array()
                    .map(|a| a.iter().map(|x| x.as_str().unwrap().to_string()).collect())
                    .unwrap_or_default();
                if ours != theirs {
                    why.push(format!("datums {ours:?}, intent {theirs:?}"));
                }
                if !why.is_empty() {
                    out.push((k, why.join("; ")));
                }
            }
            "thread" | "knurl" => {
                let name = if kind == "knurl" {
                    "knurl".to_string()
                } else {
                    format!("{} thread", r["side"].as_str().unwrap())
                };
                let a = p.attributes.iter().find(|a| {
                    a.name == name
                        && matches!(a.on, Some(pmi::NoteOwner::Feature(f)) if face_cover(p, f) == faces)
                });
                match a {
                    None => out.push((
                        k.clone(),
                        format!("no {name:?} attribute set on those faces"),
                    )),
                    Some(a) => {
                        let text = |n: &str| match attr_text(a, n) {
                            Some(pmi::AttributeValue::Text(t)) => Some(t.clone()),
                            _ => None,
                        };
                        let length = |n: &str| match attr_text(a, n) {
                            Some(pmi::AttributeValue::Measure(v)) => Some(v.si()),
                            _ => None,
                        };
                        let mut why = Vec::new();
                        if kind == "thread" {
                            if text("designation").as_deref() != r["spec"].as_str() {
                                why.push(format!("designation {:?}", text("designation")));
                            }
                            if text("fit class").as_deref() != r["class"].as_str() {
                                why.push(format!("fit class {:?}", text("fit class")));
                            }
                            if !length("pitch").is_some_and(|x| close(x, num("pitch").unwrap())) {
                                why.push(format!("pitch {:?}", length("pitch")));
                            }
                        } else {
                            if text("pattern").as_deref() != r["pattern"].as_str() {
                                why.push(format!("pattern {:?}", text("pattern")));
                            }
                            if !length("diametral pitch")
                                .is_some_and(|x| close(x, num("pitch").unwrap()))
                            {
                                why.push(format!(
                                    "diametral pitch {:?}",
                                    length("diametral pitch")
                                ));
                            }
                        }
                        if !why.is_empty() {
                            out.push((k.clone(), why.join("; ")));
                        }
                    }
                }
                // specify-core's note of the requirement, on its faces (its text route).
                if !p.notes.iter().any(|n| {
                    n.kind == name
                        && matches!(n.on, Some(pmi::NoteOwner::Feature(f)) if face_cover(p, f) == faces)
                }) {
                    out.push((format!("{k} note"), format!("no {name:?} note on those faces")));
                }
                if (kind == "thread" && p.threads.is_empty())
                    || (kind == "knurl" && p.knurls.is_empty())
                {
                    out.push((format!("{k} entity"), format!("no {kind} entity: stated only as a user defined attribute set and a note")));
                }
            }
            other => out.push((k, format!("requirement kind {other:?} not compared"))),
        }
    }
    let part = &intent["part"];
    let material = p.material.as_ref().map(|m| m.id.clone());
    if material.as_deref() != part["material"].as_str() {
        out.push((
            key("material".into()),
            format!("{material:?}, intent {}", part["material"]),
        ));
    }
    let class = p.general.iter().find_map(|g| match g {
        pmi::GeneralTolerance::Class { text, .. } => Some(text.clone()),
        pmi::GeneralTolerance::Table { .. } => None,
    });
    if class.as_deref() != part["general_tolerance"].as_str() {
        out.push((
            key("general tolerance".into()),
            format!("{class:?}, intent {}", part["general_tolerance"]),
        ));
    }
    let note = |k: &str| p.notes.iter().find(|n| n.kind == k).map(|n| n.text.clone());
    if !note("surface texture")
        .is_some_and(|t| t.starts_with(part["surface_finish"].as_str().unwrap()))
    {
        out.push((
            key("surface finish".into()),
            format!("{:?}", note("surface texture")),
        ));
    }
    if note("edge condition").as_deref() != part["edges"].as_str() {
        out.push((key("edges".into()), format!("{:?}", note("edge condition"))));
    }
    for (i, u) in used_dims.iter().enumerate() {
        if !u {
            out.push((
                key(format!("extra dimension {i}")),
                format!("{:?}", model_dim(p, &p.dimensions[i])),
            ));
        }
    }
    for (i, u) in used_tols.iter().enumerate() {
        if !u {
            out.push((
                key(format!("extra tolerance {i}")),
                format!("{:?}", p.tolerances[i].kind),
            ));
        }
    }
    for (i, u) in used_datums.iter().enumerate() {
        if !u {
            out.push((
                key(format!("extra datum {}", p.datums[i].label())),
                String::new(),
            ));
        }
    }
    out
}

#[test]
fn specify_core_outputs() {
    let dir = common::fixtures().join("ap242/specify");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".step.gz"))
        .collect();
    names.sort();
    let mut diffs = Vec::new();
    for n in names {
        let case = n.trim_end_matches(".step.gz");
        let r = read_bytes(file_bytes(&dir.join(&n)));
        let intent: Json = serde_json::from_str(
            &std::fs::read_to_string(dir.join(format!("{case}.intent.json"))).unwrap(),
        )
        .unwrap();
        let intents = match intent {
            Json::Array(a) => a,
            one => vec![one],
        };
        for i in &intents {
            let pi = i["binding"]["part"].as_u64().unwrap() as usize;
            diffs.extend(compare_specify(case, i, &r.parts[pi], pi));
        }
    }
    check_pinned("specify", &diffs);
}

/// specify-core's notes on faces (specify-core-rust's U10): each thread, knurl or finish note
/// specify-core wrote is on the faces its aspect names, as specify-core's own reader
/// (`existing.notes`, Python) reads them; captured with specify-core's venv on these files:
/// `existing.notes(path, part=k)`. Part notes stay on the part, and every instance is consumed
/// (no finding names the notes' aspects).
#[test]
fn rust_snapshot_specify_notes_on_faces_are_anchored() {
    let dir = common::fixtures().join("ap242/specify");
    for (case, part) in [
        ("assembly_plate_pin", 0),
        ("assembly_plate_pin", 1),
        ("bolt_thread_knurl", 0),
        ("string_post_tapped", 0),
        ("thumbwheel_thread_knurl", 0),
        ("spool_fits", 0),
    ] {
        let r = read_bytes(file_bytes(&dir.join(format!("{case}.step.gz"))));
        let p = &r.parts[part];
        let mut got: Vec<(String, Vec<usize>)> = p
            .notes
            .iter()
            .filter_map(|n| match n.on {
                Some(pmi::NoteOwner::Feature(f)) => Some((n.kind.clone(), face_cover(p, f))),
                _ => None,
            })
            .collect();
        got.sort();
        common::snapshot::check_with(
            &common::fixtures(),
            &format!("notes/{case}/{part}"),
            &json!(got),
            &|a, b| a == b,
        );
        for n in &p.notes {
            if n.on.is_none() {
                assert!(
                    ["surface texture", "edge condition", "general tolerances"]
                        .contains(&n.kind.as_str()),
                    "{case} part {part}: {n:?} is not on its faces"
                );
            }
        }
        assert!(
            r.findings
                .iter()
                .all(|f| !f.detail.contains("specify-core's")),
            "{case}: {:#?}",
            r.findings
        );
    }

    // Two finishes on faces in specify-core's form (`requirements.append`: each note followed by
    // its aspect), each on its own faces, and an aspect of that form after them that no note
    // precedes: reported, its faces read as a feature.
    let r = with_pmi(
        "#2000=DESCRIPTIVE_REPRESENTATION_ITEM('surface finish','Ra 0.8');
#2001=REPRESENTATION('surface finish requirement',(#2000),#399);
#2002=PROPERTY_DEFINITION('manufacturing requirement','surface finish',#5);
#2003=PROPERTY_DEFINITION_REPRESENTATION(#2002,#2001);
#2004=SHAPE_ASPECT('surface finish','',#4,.T.);
#2005=GEOMETRIC_ITEM_SPECIFIC_USAGE('surface finish','',#2004,#10,#17);
#2010=DESCRIPTIVE_REPRESENTATION_ITEM('surface finish','Ra 1.6');
#2011=REPRESENTATION('surface finish requirement',(#2010),#399);
#2012=PROPERTY_DEFINITION('manufacturing requirement','surface finish',#5);
#2013=PROPERTY_DEFINITION_REPRESENTATION(#2012,#2011);
#2014=SHAPE_ASPECT('surface finish','',#4,.T.);
#2015=GEOMETRIC_ITEM_SPECIFIC_USAGE('surface finish','',#2014,#10,#105);
#2020=SHAPE_ASPECT('surface finish','',#4,.T.);
#2021=GEOMETRIC_ITEM_SPECIFIC_USAGE('surface finish','',#2020,#10,#105);",
    );
    let base = file_bytes(&dir.join("bolt_thread_knurl.step.gz"));
    let face105 = read_part_definitions(&base).unwrap()[0]
        .faces
        .iter()
        .position(|&f| f == 105)
        .unwrap();
    let p = &r.parts[0];
    let finishes: Vec<(String, Option<Vec<usize>>)> = p
        .notes
        .iter()
        .filter(|n| n.kind == "surface finish")
        .map(|n| {
            let faces = match n.on {
                Some(pmi::NoteOwner::Feature(f)) => Some(face_cover(p, f)),
                _ => None,
            };
            (n.text.clone(), faces)
        })
        .collect();
    assert_eq!(
        finishes,
        [
            ("Ra 0.8".to_string(), Some(vec![0])),
            ("Ra 1.6".to_string(), Some(vec![face105]))
        ]
    );
    let stray: Vec<&pmi::Finding> = r
        .findings
        .iter()
        .filter(|f| f.detail.contains("specify-core's"))
        .collect();
    assert_eq!(stray.len(), 1, "{:#?}", r.findings);
    assert_eq!(
        (stray[0].kind, stray[0].ids.as_slice()),
        (pmi::FindingKind::Unresolved, &[2020][..])
    );
}

// ---------------------------------------------------------------------------------------------
// Every file: completes, accounts for every semantic instance, findings pinned
// ---------------------------------------------------------------------------------------------

/// A finding's detail with ids, numbers and quoted text abstracted, for pinning by kind.
fn template(detail: &str) -> String {
    let mut out = String::new();
    let mut chars = detail.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push_str("\"…\"");
                for d in chars.by_ref() {
                    if d == '"' {
                        break;
                    }
                }
            }
            '#' | '0'..='9' => {
                out.push('N');
                while chars
                    .peek()
                    .is_some_and(|d| d.is_ascii_digit() || *d == '.')
                {
                    chars.next();
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// The files read by the accounting test: the committed NIST models and specify-core outputs,
/// the assembly fixture, and every NIST AP242 file when `HAECCEITY_NIST_PMI` names them.
fn all_files() -> Vec<(String, std::path::PathBuf)> {
    let f = common::fixtures();
    let mut out: Vec<(String, std::path::PathBuf)> = Vec::new();
    for (dir, ext) in [("ap242/nist", ".stp.gz"), ("ap242/specify", ".step.gz")] {
        let mut names: Vec<String> = std::fs::read_dir(f.join(dir))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(ext))
            .collect();
        names.sort();
        for n in names {
            out.push((n.trim_end_matches(ext).to_string(), f.join(dir).join(&n)));
        }
    }
    out.push(("assembly".into(), f.join("ap242/assembly/assembly.step")));
    if let Some(d) = nist_dir() {
        let mut names: Vec<String> = std::fs::read_dir(&d)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains("_ap242-") && n.ends_with(".stp"))
            .collect();
        names.sort();
        for n in names {
            let stem = n.trim_end_matches(".stp").to_string();
            if !out.iter().any(|(s, _)| *s == stem) {
                out.push((stem, d.join(n)));
            }
        }
    }
    out
}

/// Oracle 2: every file reads to completion; every semantic-PMI instance is consumed by a part
/// or named by a finding; the findings, by kind, entity type and detail, are those pinned in
/// `tests/fixtures/ap242/read/findings.json` with a reason.
#[test]
fn every_file_reads_and_accounts() {
    let pins: Json = serde_json::from_str(
        &std::fs::read_to_string(common::fixtures().join("ap242/read/findings.json")).unwrap(),
    )
    .unwrap();
    let reasons = pins["reasons"].as_object().unwrap();
    for (k, v) in reasons {
        assert!(
            v.as_str().is_some_and(|s| !s.is_empty()),
            "findings.json: reason for {k} is empty"
        );
    }
    let mut actual = serde_json::Map::new();
    let mut problems = Vec::new();
    for (stem, path) in all_files() {
        let bytes = file_bytes(&path);
        let parts = match read_part_definitions(&bytes) {
            Ok(p) => p,
            Err(e) => {
                // No part to anchor to: the PMI is read against no part, all of it reported.
                problems.push(format!(
                    "{stem}: part definitions refused ({e}); read with none"
                ));
                Vec::new()
            }
        };
        let doc = Document::parse(bytes).unwrap();
        let r = pmi::read(&doc, &parts).unwrap();
        // Accounting.
        let consumed: std::collections::BTreeSet<u64> =
            r.provenance.parts.iter().flat_map(|p| p.all()).collect();
        let named: std::collections::BTreeSet<u64> = r
            .findings
            .iter()
            .flat_map(|f| f.ids.iter().copied())
            .collect();
        let mut semantic = 0;
        for id in doc.ids() {
            let names: Vec<String> = match doc.get(id).unwrap() {
                haecceity::p21::RawEntity::Simple { name, .. } => vec![name.to_ascii_lowercase()],
                haecceity::p21::RawEntity::Complex { parts, .. } => {
                    parts.iter().map(|p| p.name.to_ascii_lowercase()).collect()
                }
            };
            let fams: Vec<_> = names
                .iter()
                .filter_map(|n| haecceity::express::family(n))
                .collect();
            let semantic_type = fams.contains(&haecceity::express::Family::SemanticPmi)
                && !fams.contains(&haecceity::express::Family::Presentation);
            if semantic_type {
                semantic += 1;
                assert!(
                    consumed.contains(&id) || named.contains(&id),
                    "{stem}: semantic instance #{id} ({names:?}) neither consumed nor reported"
                );
            }
        }
        assert_eq!(r.accounting.semantic, semantic, "{stem}: accounting count");
        assert_eq!(
            r.accounting.consumed + r.accounting.reported,
            r.accounting.semantic,
            "{stem}: accounting"
        );
        // Every part's model is valid.
        for (pi, p) in r.parts.iter().enumerate() {
            if let Err(e) = p.validate() {
                problems.push(format!("{stem} part {pi}: invalid model: {e:?}"));
            }
        }
        let mut counts: BTreeMap<String, u64> = BTreeMap::new();
        for f in &r.findings {
            *counts
                .entry(format!(
                    "{}|{}|{}",
                    f.kind.as_str(),
                    f.entity,
                    template(&f.detail)
                ))
                .or_default() += 1;
        }
        for k in counts.keys() {
            let reason_key = k.split('|').map(|x| x.to_string()).collect::<Vec<_>>();
            let rk = format!("{}|{}", reason_key[0], reason_key[2]);
            if !reasons.contains_key(&rk) {
                problems.push(format!("{stem}: no reason for {rk}"));
            }
        }
        actual.insert(stem.clone(), serde_json::to_value(counts).unwrap());
    }
    let pinned = pins["files"].as_object().unwrap();
    for (stem, counts) in &actual {
        match pinned.get(stem) {
            Some(p) if p == counts => {}
            Some(p) => problems.push(format!(
                "{stem}: findings changed:\n  pinned {p}\n  actual {counts}"
            )),
            None => problems.push(format!("{stem}: findings not pinned: {counts}")),
        }
    }
    let _ = std::fs::write(
        std::env::temp_dir().join("pmi_findings_actual.json"),
        serde_json::to_string_pretty(&Json::Object(actual)).unwrap(),
    );
    let problems: Vec<String> = problems
        .into_iter()
        .filter(|p| {
            !p.contains("part definitions refused")
                || !pinned.contains_key(p.split(':').next().unwrap())
        })
        .collect();
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

// ---------------------------------------------------------------------------------------------
// Values as stated, assemblies, determinism, the model's invariants, synthetic constructs
// ---------------------------------------------------------------------------------------------

/// Every length and angle value of a part, with what it is.
fn values(p: &PartPmi) -> Vec<(String, pmi::Quantity)> {
    let mut out = Vec::new();
    let mut v = |what: String, x: &pmi::Value| out.push((what, x.quantity.clone()));
    for (i, d) in p.dimensions.iter().enumerate() {
        if let Some(n) = &d.nominal {
            v(format!("dimension {i} nominal"), n);
        }
        match &d.tolerance {
            DimTolerance::Deviations(b)
            | DimTolerance::Limits(b)
            | DimTolerance::Fit {
                limits: Some(b), ..
            } => {
                v(format!("dimension {i} upper"), b.upper());
                v(format!("dimension {i} lower"), b.lower());
            }
            _ => {}
        }
    }
    for (i, t) in p.tolerances.iter().enumerate() {
        for (w, x) in [
            ("magnitude", &t.magnitude),
            ("maximum", &t.maximum),
            ("unequal", &t.unequal),
        ] {
            if let Some(x) = x {
                v(format!("tolerance {i} {w}"), x);
            }
        }
        if let Some(z) = &t.zone {
            if let Some(pz) = &z.projected {
                v(format!("tolerance {i} projected length"), &pz.length);
            }
            if let Some(a) = &z.runout_angle {
                v(format!("tolerance {i} runout angle"), a);
            }
        }
        if let Some(u) = &t.unit_basis {
            v(format!("tolerance {i} unit size"), &u.size);
        }
    }
    let mut lengths: Vec<(String, pmi::Quantity)> = Vec::new();
    for (i, t) in p.datum_targets.iter().enumerate() {
        let ls: Vec<&pmi::Length> = match t.shape() {
            TargetShape::Line { length } => vec![length],
            TargetShape::Rectangle { length, width } => vec![length, width],
            TargetShape::Circle { diameter } | TargetShape::CircularCurve { diameter } => {
                vec![diameter]
            }
            _ => vec![],
        };
        for l in ls {
            lengths.push((format!("target {i} size"), pmi::Quantity::Length(l.clone())));
        }
    }
    out.extend(lengths);
    out
}

/// The REAL tokens of instance texts.
fn reals_in(text: &str) -> Vec<String> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\'' => {
                i += 1;
                while i < b.len() && !(b[i] == b'\'' && b.get(i + 1) != Some(&b'\'')) {
                    if b[i] == b'\'' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'#' => {
                i += 1;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
            }
            c if c.is_ascii_digit()
                || ((c == b'-' || c == b'+') && b.get(i + 1).is_some_and(u8::is_ascii_digit)) =>
            {
                let s = i;
                i += 1;
                while i < b.len()
                    && (b[i].is_ascii_digit()
                        || b[i] == b'.'
                        || b[i] == b'E'
                        || b[i] == b'e'
                        || ((b[i] == b'-' || b[i] == b'+') && matches!(b[i - 1], b'E' | b'e')))
                {
                    i += 1;
                }
                let t = &text[s..i];
                if t.contains('.') {
                    out.push(t.to_string());
                }
            }
            _ => i += 1,
        }
    }
    out
}

/// Oracle 7: on every NIST file with inch units, every value read keeps the REAL token's text of
/// the file (an instance of its item's provenance), in inches: nothing is converted.
#[test]
fn inch_values_keep_their_text() {
    let mut files: Vec<(String, std::path::PathBuf)> = all_files()
        .into_iter()
        .filter(|(s, _)| s.starts_with("nist_"))
        .collect();
    files.dedup();
    let mut checked = 0;
    for (stem, path) in files {
        let bytes = file_bytes(&path);
        let Ok(parts) = read_part_definitions(&bytes) else {
            continue;
        };
        let doc = Document::parse(bytes).unwrap();
        let r = pmi::read(&doc, &parts).unwrap();
        for (pi, p) in r.parts.iter().enumerate() {
            let vals = values(p);
            if !vals.iter().any(
                |(_, q)| matches!(q, pmi::Quantity::Length(l) if l.unit == pmi::LengthUnit::Inch),
            ) {
                continue;
            }
            let prov = &r.provenance.parts[pi];
            let mut tokens = std::collections::BTreeSet::new();
            for id in prov.all() {
                let span = doc.span(id).unwrap();
                tokens.extend(reals_in(&String::from_utf8_lossy(&doc.bytes()[span])));
            }
            for (what, q) in vals {
                let text = q.decimal().as_str().to_string();
                assert!(
                    tokens.contains(&text),
                    "{stem}: {what} {text} is not a REAL token of its instances"
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "no inch values checked");
}

/// Oracle 6: the two-part assembly fixture (a part placed twice) and specify-core's output on it:
/// each part's PMI is on its own part, the twice-placed part is read once.
#[test]
fn assembly_parts_read_once_each_with_its_own_pmi() {
    let f = common::fixtures();
    let bytes = file_bytes(&f.join("ap242/assembly/assembly.step"));
    let parts = read_part_definitions(&bytes).unwrap();
    let doc = Document::parse(bytes).unwrap();
    let r = pmi::read(&doc, &parts).unwrap();
    assert_eq!(r.parts.len(), parts.len());
    assert!(
        parts.iter().any(|p| p.placements.len() == 2),
        "a part placed twice"
    );
    assert!(
        r.parts.iter().all(|p| p == &PartPmi::default()),
        "no PMI in the fixture"
    );

    let bytes = file_bytes(&f.join("ap242/specify/assembly_plate_pin.step.gz"));
    let parts = read_part_definitions(&bytes).unwrap();
    let doc = Document::parse(bytes).unwrap();
    let r = pmi::read(&doc, &parts).unwrap();
    assert_eq!(r.parts.len(), 2, "plate and pin, the pin placed twice");
    assert!(parts.iter().any(|p| p.placements.len() == 2));
    let labels = |p: &PartPmi| {
        p.datums
            .iter()
            .map(|d| d.label().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(labels(&r.parts[0]), ["A", "B", "C"]);
    assert_eq!(
        labels(&r.parts[1]),
        ["A", "B"],
        "datum A on each part, its own"
    );
    for (pi, p) in r.parts.iter().enumerate() {
        p.validate().unwrap();
        for f in &p.features {
            if let Feature::Items(a) = f {
                for x in a {
                    if let pmi::Anchor::Face(i) = x {
                        assert!(
                            i.0 < parts[pi].faces.len(),
                            "part {pi}: face {} is not the part's",
                            i.0
                        );
                    }
                }
            }
        }
        // Every instance consumed for the part hangs off the part's own shape or definition.
        for id in r.provenance.parts[pi].all() {
            if let Some(haecceity::p21::RawEntity::Simple {
                name, attributes, ..
            }) = doc.get(id)
                && name == "SHAPE_ASPECT"
            {
                assert_eq!(
                    attributes[2],
                    haecceity::p21::Attribute::EntityRef(parts[pi].shape)
                );
            }
        }
    }
    assert!(
        r.findings
            .iter()
            .all(|f| f.kind == pmi::FindingKind::NotModelled),
        "{:?}",
        r.findings
    );
}

/// Oracle 8: reading is deterministic (twice, each with fresh hash seeds, identical output).
#[test]
fn reading_is_deterministic() {
    for name in [
        "nist/nist_ftc_10_asme1_ap242-e2.stp.gz",
        "specify/assembly_plate_pin.step.gz",
    ] {
        let bytes = file_bytes(&common::fixtures().join("ap242").join(name));
        let a = format!("{:?}", read_bytes(bytes.clone()));
        let b = format!("{:?}", read_bytes(bytes));
        assert_eq!(a, b, "{name}");
    }
}

/// The datum requirement of each tolerance kind is the schema's: subtypes of
/// `geometric_tolerance_with_datum_reference` require datums; kinds with a WR1 (which is
/// `NOT (… GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE IN TYPEOF(SELF))` for exactly these four) forbid
/// them; the rest (position, the profiles: no rule, not a subtype) take them optionally.
#[test]
fn datum_requirements_follow_the_schema() {
    for (entity, kind) in ToleranceKind::ALL {
        let subtype = haecceity::express::is_a(entity, "geometric_tolerance_with_datum_reference");
        let rules: Vec<&str> = haecceity::express::rules(entity).collect();
        let want = if subtype {
            pmi::DatumRequirement::Required
        } else if rules == ["WR1"] {
            pmi::DatumRequirement::Forbidden
        } else {
            assert!(rules.is_empty(), "{entity}: rules {rules:?}");
            pmi::DatumRequirement::Optional
        };
        assert_eq!(kind.datums(), want, "{entity}");
        assert!(
            haecceity::express::is_a(entity, "geometric_tolerance"),
            "{entity}"
        );
    }
}

#[test]
fn model_invariants() {
    use pmi::*;
    let d = |t: &str| Decimal::parse(t).unwrap();
    assert!(Decimal::parse("0.0030").is_ok());
    assert!(Decimal::parse("35.").is_ok());
    assert!(Decimal::parse("2.54E1").is_ok());
    assert!(Decimal::parse(".5").is_err());
    assert!(Decimal::parse("1,5").is_err());
    assert_eq!(d("0.0030").as_str(), "0.0030");
    assert!(d("0.5").cmp_value(&d("5.E-1")).is_eq());
    assert!(d("-0.009").cmp_value(&d("-0.025")).is_gt());
    assert_eq!(
        d("25.4").times(&d("0.001")).cmp_value(&d("0.0254")),
        std::cmp::Ordering::Equal
    );
    assert_eq!(
        LengthUnit::from_metres("INCH", d("0.0254")),
        LengthUnit::Inch
    );
    // Bounds: upper > lower, no sign assumed (g6: both below nominal).
    let mm = |t: &str| Value::length(d(t), LengthUnit::Millimetre);
    let g6 = Bounds::new(mm("-0.009"), mm("-0.025")).unwrap();
    assert_eq!(g6.upper().decimal().as_str(), "-0.009");
    assert!(Bounds::new(mm("-0.025"), mm("-0.009")).is_err());
    assert!(Bounds::new(mm("0.1"), Value::angle(d("0."), AngleUnit::Degree)).is_err());
    // Mixed units compare by size: 0.001 in > 0.02 mm.
    assert!(Bounds::new(Value::length(d("0.001"), LengthUnit::Inch), mm("0.02")).is_ok());
    // ISO 286 classes: two enums, case gives hole or shaft.
    let h7 = Iso286Class {
        deviation: FundamentalDeviation::parse("H").unwrap(),
        grade: ToleranceGrade::parse("7").unwrap(),
    };
    assert_eq!(h7.to_string(), "H7");
    assert_eq!(
        FundamentalDeviation::parse("js").unwrap().of,
        FitFeature::Shaft
    );
    assert!(FundamentalDeviation::parse("Js").is_err());
    assert!(FundamentalDeviation::parse("I").is_err());
    assert_eq!(ToleranceGrade::parse("IT01").unwrap(), ToleranceGrade::It01);
    assert!(ToleranceGrade::parse("19").is_err());
    // Datums: established by a feature or targets, never neither; targets numbered > 0.
    let a = DatumLabel::new("A").unwrap();
    assert!(Datum::new(a.clone(), None, vec![]).is_err());
    assert!(Datum::new(a.clone(), Some(FeatureId(0)), vec![]).is_ok());
    assert!(Datum::new(a.clone(), None, vec![DatumTargetId(0), DatumTargetId(0)]).is_err());
    assert!(DatumLabel::new("").is_err());
    assert!(DatumTarget::new(0, TargetShape::Area(FeatureId(0)), None, None, None).is_err());
    assert!(DatumTarget::new(1, TargetShape::Point, None, None, None).is_err());
    assert!(DatumSystem::new(vec![]).is_err());
    let comp = |d: usize| Compartment {
        references: vec![DatumReference {
            datum: DatumId(d),
            modifiers: vec![],
        }],
        modifiers: vec![],
    };
    assert!(DatumSystem::new(vec![comp(0), comp(0), comp(0), comp(0)]).is_err());
    // Part-level: references resolve, one datum per label, datum requirements, composites.
    let face = Feature::Items(vec![Anchor::Face(FaceIndex(0))]);
    let mut p = PartPmi {
        features: vec![face.clone(), face],
        datums: vec![Datum::new(a.clone(), Some(FeatureId(0)), vec![]).unwrap()],
        ..PartPmi::default()
    };
    p.validate().unwrap();
    p.datums
        .push(Datum::new(a, Some(FeatureId(1)), vec![]).unwrap());
    assert!(p.validate().is_err(), "a label twice on one part");
    p.datums.pop();
    let tol = |kind, datums: Option<DatumSystem>| GeometricTolerance {
        kind,
        target: ToleranceTarget::Feature(FeatureId(1)),
        magnitude: Some(mm("0.1")),
        zone: None,
        modifiers: vec![],
        unit_basis: None,
        maximum: None,
        unequal: None,
        datums,
        auxiliary: vec![],
        description: None,
    };
    p.tolerances = vec![tol(ToleranceKind::Perpendicularity, None)];
    assert!(p.validate().is_err(), "perpendicularity requires datums");
    p.tolerances = vec![tol(
        ToleranceKind::Flatness,
        Some(DatumSystem::new(vec![comp(0)]).unwrap()),
    )];
    assert!(p.validate().is_err(), "flatness takes no datums");
    p.tolerances = vec![
        tol(ToleranceKind::Position, None),
        tol(ToleranceKind::Flatness, None),
    ];
    p.tolerance_relations = vec![ToleranceRelation {
        kind: RelationKind::Composite,
        relating: ToleranceId(0),
        related: ToleranceId(1),
    }];
    assert!(
        p.validate().is_err(),
        "a composite of position and flatness"
    );
    p.tolerances[1] = tol(ToleranceKind::Position, None);
    p.validate().unwrap();
    p.features
        .push(Feature::Items(vec![Anchor::Geometry(GeometryId(0))]));
    assert!(p.validate().is_err(), "missing geometry");
}

#[test]
fn iso_2768_classes_and_table() {
    use pmi::standards::*;
    let c = Iso2768::recognise("ISO 2768-mK").unwrap();
    assert_eq!(
        (c.linear, c.geometric),
        (LinearClass::Medium, Some(GeometricClass::K))
    );
    assert_eq!(
        Iso2768::recognise("ISO 2768-f").unwrap().linear,
        LinearClass::Fine
    );
    assert!(Iso2768::recognise("ISO 2768-x").is_none());
    assert!(Iso2768::recognise("DIN 7168-m").is_none());
    // ISO 2768-1:1989 Table 1.
    let dev = |c: LinearClass, n: f64| c.linear_deviation(n).map(|d| d.as_str().to_string());
    assert_eq!(dev(LinearClass::Medium, 50.0).as_deref(), Some("0.3"));
    assert_eq!(dev(LinearClass::Medium, 6.0).as_deref(), Some("0.1"));
    assert_eq!(dev(LinearClass::Medium, 6.01).as_deref(), Some("0.2"));
    assert_eq!(dev(LinearClass::Fine, 0.5).as_deref(), Some("0.05"));
    assert_eq!(dev(LinearClass::Fine, 3000.0), None);
    assert_eq!(dev(LinearClass::VeryCoarse, 2.0), None);
    assert_eq!(dev(LinearClass::Coarse, 4000.0).as_deref(), Some("4"));
    assert_eq!(dev(LinearClass::Medium, 0.4), None);
}

/// The small specify-core output the synthetic constructs are added to: its part definition is
/// #5, its shape #4, its shape representation #10 in context #399 (millimetre #400, radian
/// #401); #17 and #105 are faces of its solid.
fn with_pmi(extra: &str) -> PmiRead {
    let base = file_bytes(&common::fixtures().join("ap242/specify/bolt_thread_knurl.step.gz"));
    let text = String::from_utf8(base).unwrap();
    let at = text.rfind("ENDSEC;").unwrap();
    let out = format!("{}{extra}\n{}", &text[..at], &text[at..]);
    read_bytes(out.into_bytes())
}

#[test]
fn reads_threads_and_knurls_by_their_schema_parameters() {
    let r = with_pmi(
        "#1000=THREAD('M10x1.5','thread');
#1001=PRODUCT_DEFINITION_SHAPE('','',#1000);
#1002=SHAPE_ASPECT('','',#1001,.T.);
#1003=SHAPE_ASPECT('thread face','',#4,.T.);
#1004=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#1003,#10,#17);
#1005=SHAPE_DEFINING_RELATIONSHIP('','applied shape',#1003,#1002);
#1006=PROPERTY_DEFINITION('thread','',#1000);
#1010=DESCRIPTIVE_REPRESENTATION_ITEM('thread side','external');
#1011=(LENGTH_MEASURE_WITH_UNIT()MEASURE_REPRESENTATION_ITEM()MEASURE_WITH_UNIT(LENGTH_MEASURE(10.),#400)REPRESENTATION_ITEM('major diameter'));
#1012=DIMENSIONAL_EXPONENTS(0.,0.,0.,0.,0.,0.,0.);
#1013=(NAMED_UNIT(#1012)RATIO_UNIT());
#1014=(MEASURE_REPRESENTATION_ITEM()MEASURE_WITH_UNIT(RATIO_MEASURE(1.),#1013)RATIO_MEASURE_WITH_UNIT()REPRESENTATION_ITEM('number of threads'));
#1015=DESCRIPTIVE_REPRESENTATION_ITEM('form','ISO metric');
#1016=DESCRIPTIVE_REPRESENTATION_ITEM('fit class','6g');
#1017=DESCRIPTIVE_REPRESENTATION_ITEM('hand','right');
#1018=(LENGTH_MEASURE_WITH_UNIT()MEASURE_REPRESENTATION_ITEM()MEASURE_WITH_UNIT(LENGTH_MEASURE(9.026),#400)REPRESENTATION_ITEM('pitch diameter'));
#1019=SHAPE_REPRESENTATION_WITH_PARAMETERS('',(#1010,#1011,#1014,#1015,#1016,#1017,#1018,#11),#399);
#1020=PROPERTY_DEFINITION_REPRESENTATION(#1006,#1019);
#1100=TURNED_KNURL('k','diamond');
#1101=PRODUCT_DEFINITION_SHAPE('','',#1100);
#1102=SHAPE_ASPECT('','',#1101,.T.);
#1103=SHAPE_ASPECT('knurl face','',#4,.T.);
#1104=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#1103,#10,#105);
#1105=SHAPE_DEFINING_RELATIONSHIP('','applied shape',#1103,#1102);
#1106=PROPERTY_DEFINITION('knurl','',#1100);
#1110=(LENGTH_MEASURE_WITH_UNIT()MEASURE_REPRESENTATION_ITEM()MEASURE_WITH_UNIT(LENGTH_MEASURE(6.25),#400)REPRESENTATION_ITEM('major diameter'));
#1111=(LENGTH_MEASURE_WITH_UNIT()MEASURE_REPRESENTATION_ITEM()MEASURE_WITH_UNIT(LENGTH_MEASURE(6.),#400)REPRESENTATION_ITEM('nominal diameter'));
#1112=(LENGTH_MEASURE_WITH_UNIT()MEASURE_REPRESENTATION_ITEM()MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),#400)REPRESENTATION_ITEM('diametral pitch'));
#1113=(MEASURE_REPRESENTATION_ITEM()MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(0.5235987756),#401)PLANE_ANGLE_MEASURE_WITH_UNIT()REPRESENTATION_ITEM('helix angle'));
#1114=(MEASURE_REPRESENTATION_ITEM()MEASURE_WITH_UNIT(COUNT_MEASURE(60.),#1013)REPRESENTATION_ITEM('number of teeth'));
#1115=SHAPE_REPRESENTATION_WITH_PARAMETERS('',(#1110,#1111,#1112,#1113,#1114,#11),#399);
#1116=PROPERTY_DEFINITION_REPRESENTATION(#1106,#1115);
#1120=ID_ATTRIBUTE('thread-01',#1001);
#1121=ID_ATTRIBUTE('knurl-01',#1101);
#1122=ID_ATTRIBUTE('support-only',#1103);",
    );
    let p = &r.parts[0];
    assert_eq!(p.threads.len(), 1, "{:?}", r.findings);
    let t = &p.threads[0];
    assert_eq!(t.side, pmi::ThreadSide::External);
    assert_eq!(t.major_diameter.value.as_str(), "10.");
    assert_eq!(t.pitch_diameter.as_ref().unwrap().value.as_str(), "9.026");
    assert_eq!(t.number_of_threads.0.as_str(), "1.");
    assert_eq!(
        (t.form.as_str(), t.fit_class.as_str(), t.hand),
        ("ISO metric", "6g", pmi::Hand::Right)
    );
    assert_eq!(
        p.features[t.feature.0],
        Feature::Items(vec![pmi::Anchor::Face(pmi::FaceIndex(0))])
    );
    assert_eq!(p.knurls.len(), 1);
    assert_eq!(r.identifiers.parts[0].threads, [Some("thread-01".into())]);
    assert_eq!(r.identifiers.parts[0].knurls, [Some("knurl-01".into())]);
    assert!(r.provenance.parts[0].threads[0].contains(&1120));
    assert!(r.provenance.parts[0].knurls[0].contains(&1121));
    let k = &p.knurls[0];
    assert_eq!(k.pattern, pmi::KnurlPattern::Diamond);
    assert_eq!(k.number_of_teeth, Some(pmi::Count(60)));
    assert!((k.helix_angle.as_ref().unwrap().rad() - 30f64.to_radians()).abs() < 1e-9);
    let prov = &r.provenance.parts[0];
    for id in [1000, 1001, 1002, 1005, 1006, 1019, 1020] {
        assert!(
            prov.threads[0].contains(&id),
            "#{id} in the thread's provenance"
        );
    }
    // The parameter placement #11 is not a thread parameter: reported, not guessed.
    assert!(
        r.findings
            .iter()
            .any(|f| f.ids[0] == 1000 && f.kind == pmi::FindingKind::Nonconformance),
        "{:?}",
        r.findings
    );
    p.validate().unwrap();
}

#[test]
fn reads_general_tolerance_table_decimal_places_material_density_and_standard() {
    let records = "#1200=DIMENSIONAL_EXPONENTS(0.,0.,0.,0.,0.,0.,0.);
#1201=NAMED_UNIT(#1200);
#1202=(MEASURE_REPRESENTATION_ITEM()MEASURE_WITH_UNIT(COUNT_MEASURE(3.),#1201)REPRESENTATION_ITEM('number of decimal places'));
#1203=DEFAULT_TOLERANCE_TABLE_CELL('',SET_REPRESENTATION_ITEM((#1202)));
#1204=REPRESENTATION_CONTEXT('','default setting');
#1205=DEFAULT_TOLERANCE_TABLE('',(#1203),#1204);
#1206=REPRESENTATION_RELATIONSHIP('general tolerance definition','',#1205,#514);
#1300=PROPERTY_DEFINITION('material property','density',#5);
#1301=(MASS_UNIT()NAMED_UNIT(*)SI_UNIT($,.GRAM.));
#1302=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.CENTI.,.METRE.));
#1303=DERIVED_UNIT_ELEMENT(#1301,1.);
#1304=DERIVED_UNIT_ELEMENT(#1302,-3.);
#1305=DERIVED_UNIT((#1303,#1304));
#1306=MEASURE_REPRESENTATION_ITEM('density measure',POSITIVE_RATIO_MEASURE(7.85),#1305);
#1307=REPRESENTATION('density',(#1306),#399);
#1308=PROPERTY_DEFINITION_REPRESENTATION(#1300,#1307);
#1400=APPLICATION_CONTEXT('geometrical dimensioning and tolerancing representation');
#1401=PRODUCT_DEFINITION_CONTEXT('',#1400,'design');
#1402=PRODUCT_DEFINITION_CONTEXT_ROLE('additional context','');
#1403=PRODUCT_DEFINITION_CONTEXT_ASSOCIATION(#5,#1401,#1402);
#1404=DOCUMENT_TYPE('configuration controlled document version');
#1405=DOCUMENT('ISO 1101','Geometrical tolerancing','',#1404);
#1406=APPLIED_DOCUMENT_REFERENCE(#1405,'',(#1401));
#1407=OBJECT_ROLE('mandatory','');
#1408=ROLE_ASSOCIATION(#1407,#1406);
#1410=ID_ATTRIBUTE('class-id',#515);
#1411=ID_ATTRIBUTE('table-id',#1205);";
    let r = with_pmi(records);
    let p = &r.parts[0];
    // Reader /11 projects Figure 3's anonymous single decimal-only carrier to the
    // part default, retaining its provenance. Named tables retain the stated cell and digits.
    // specify-core's own 'default tolerances' (#513-#515) still carries the class.
    assert!(
        matches!(&p.general[0], pmi::GeneralTolerance::Class { text, standard: Some(_) } if text == "ISO 2768-m"),
        "{:?}",
        p.general
    );
    assert_eq!(p.general.len(), 1);
    let named = with_pmi(&records.replace(
        "DEFAULT_TOLERANCE_TABLE('',",
        "DEFAULT_TOLERANCE_TABLE('precision',",
    ));
    assert_eq!(named.parts[0].general.len(), 2);
    assert_eq!(r.identifiers.parts[0].general, [Some("class-id".into())]);
    assert_eq!(
        named.identifiers.parts[0].general,
        [Some("class-id".into()), Some("table-id".into())]
    );
    match &named.parts[0].general[1] {
        pmi::GeneralTolerance::Table { name, cells } => {
            assert_eq!(name, "precision");
            assert_eq!(cells.len(), 1);
            assert_eq!(cells[0].items.len(), 1);
            assert_eq!(cells[0].items[0].0, "number of decimal places");
            assert!(matches!(&cells[0].items[0].1, pmi::CellValue::Count(v) if v.as_str() == "3."));
        }
        g => panic!("{g:?}"),
    }
    assert_eq!(named.parts[0].decimal_places, Some(3));
    for id in [1203, 1205, 1206] {
        assert!(named.provenance.parts[0].general[1].contains(&id));
    }
    assert_eq!(p.decimal_places, Some(3));
    let m = p.material.as_ref().unwrap();
    assert_eq!(m.id, "Steel C45");
    let d = m.density.as_ref().unwrap();
    assert_eq!(d.value.as_str(), "7.85");
    let unit: Vec<(String, String)> = d
        .unit
        .iter()
        .map(|(n, e)| (n.clone(), e.as_str().to_string()))
        .collect();
    assert_eq!(
        unit,
        [
            ("gram".to_string(), "1.".to_string()),
            ("centimetre".to_string(), "-3.".to_string())
        ]
    );
    assert_eq!(p.standards.len(), 1);
    assert_eq!(p.standards[0].document, "ISO 1101");
    assert_eq!(
        p.standards[0].default_principle(),
        Some(pmi::Principle::Independency)
    );
    for id in [1203, 1205, 1206] {
        assert!(r.provenance.parts[0].decimal_places.contains(&id));
    }
    for id in [1403, 1406, 1408] {
        assert!(r.provenance.parts[0].standards[0].contains(&id));
    }
}

#[test]
fn datum_features_without_faces_are_reported() {
    let read = with_pmi(
        "#1500=DATUM_FEATURE('','',#4,.T.);
#1501=DATUM('',$,#4,.F.,'EMPTY');
#1502=SHAPE_ASPECT_RELATIONSHIP('',$,#1500,#1501);
#1510=DATUM_FEATURE('','',#4,.T.);
#1511=DATUM('',$,#4,.F.,'EDGE');
#1512=SHAPE_ASPECT_RELATIONSHIP('',$,#1510,#1511);
#1513=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#1510,#10,#21);",
    );
    assert_eq!(
        common::pmi_datums::unresolved(&read),
        ["EMPTY", "EDGE"]
            .map(|label| (pmi::PartId(0), label.to_string()))
            .into_iter()
            .collect()
    );
    for (label, datum, feature) in [("EMPTY", 1501, 1500), ("EDGE", 1511, 1510)] {
        let findings: Vec<_> = read.findings.iter().filter(|f| f.ids[0] == datum).collect();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, pmi::FindingKind::Unresolved);
        assert_eq!(findings[0].ids, [datum, feature]);
        assert_eq!(
            findings[0].detail,
            format!("datum {label} feature resolves to no faces")
        );
    }
}

#[test]
fn unreferenced_datums_are_read_and_bad_references_are_findings() {
    let r = with_pmi(
        "#1500=DATUM_FEATURE('','',#4,.T.);
#1501=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#1500,#10,#105);
#1502=DATUM('',$,#4,.F.,'Z');
#1503=SHAPE_ASPECT_RELATIONSHIP('',$,#1500,#1502);
#1510=SHAPE_ASPECT('','',#4,.T.);
#1511=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#1510,#10,#17);
#1512=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.1),#12);
#1513=FLATNESS_TOLERANCE('','',#1512,#1510);
#1520=SHAPE_ASPECT('','',#4,.T.);
#1521=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#1520,#10,#21);
#1522=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.2),#400);
#1523=FLATNESS_TOLERANCE('','',#1522,#1520);",
    );
    let p = &r.parts[0];
    // Datum Z is cited by no tolerance; it is read with its feature (not only when referenced).
    let z = p.datums.iter().find(|d| d.label().as_str() == "Z").unwrap();
    assert!(z.feature().is_some());
    // A magnitude whose unit is a cartesian point: unitless, the tolerance not read.
    assert!(
        r.findings
            .iter()
            .any(|f| f.kind == pmi::FindingKind::Unitless && f.ids[0] == 1512),
        "{:?}",
        r.findings
    );
    // Edge #21 is an edge of the part: read; the flatness on it is read.
    assert!(
        p.tolerances
            .iter()
            .any(|t| t.magnitude.as_ref().unwrap().decimal().as_str() == "0.2")
    );
    assert!(
        !p.tolerances
            .iter()
            .any(|t| t.magnitude.as_ref().unwrap().decimal().as_str() == "0.1")
    );
    for f in &r.findings {
        if f.ids.contains(&1513) || f.ids.contains(&1512) {
            assert_ne!(f.kind, pmi::FindingKind::Unconsumed, "{f:?}");
        }
    }
}

/// PMI on the assembly's own product definition shape, and on an occurrence (a
/// `next_assembly_usage_occurrence`'s shape), is out of scope: each is a finding of its own kind,
/// with no part, and no part consumes any of it. The assembly fixture: assembly definition #5
/// (shape #4), plate occurrence #968 (shape #967), millimetre #28.
#[test]
fn assembly_and_occurrence_pmi_are_findings_not_part_pmi() {
    let f = common::fixtures();
    let base = file_bytes(&f.join("ap242/assembly/assembly.step"));
    let text = String::from_utf8(base).unwrap();
    let at = text.rfind("ENDSEC;").unwrap();
    let extra = "#2000=SHAPE_ASPECT('top','',#4,.T.);
#2001=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.1),#28);
#2002=FLATNESS_TOLERANCE('flat','',#2001,#2000);
#2010=SHAPE_ASPECT('plate face','',#967,.T.);
#2011=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.2),#28);
#2012=FLATNESS_TOLERANCE('flat','',#2011,#2010);
#2013=DIMENSIONAL_SIZE(#2010,'thickness');
";
    let bytes = format!("{}{extra}{}", &text[..at], &text[at..]).into_bytes();
    let (r, _) = read_doc(bytes);
    assert!(
        r.parts.iter().all(|p| p == &PartPmi::default()),
        "no part holds assembly or occurrence PMI"
    );
    for p in &r.provenance.parts {
        for id in 2000..=2013 {
            assert!(!p.all().contains(&id), "#{id} consumed for a part");
        }
    }
    let kind_of = |id: u64| {
        r.findings
            .iter()
            .find(|f| f.ids[0] == id)
            .unwrap_or_else(|| panic!("no finding for #{id}: {:?}", r.findings))
    };
    for id in [2000, 2002] {
        let f = kind_of(id);
        assert_eq!(
            (f.kind, f.part),
            (pmi::FindingKind::AssemblyPmi, None),
            "{f:?}"
        );
    }
    for id in [2010, 2012, 2013] {
        let f = kind_of(id);
        assert_eq!(
            (f.kind, f.part),
            (pmi::FindingKind::OccurrencePmi, None),
            "{f:?}"
        );
    }
}

/// A count is read as the whole number it states (`1.2E1` is 12) and a fraction is refused, not
/// truncated; a measure whose type and unit disagree is a nonconformance, not "unitless".
#[test]
fn counts_are_whole_and_measure_errors_are_typed() {
    let knurl = |teeth: &str| {
        with_pmi(&format!(
            "#1100=TURNED_KNURL('k','straight');
#1101=PRODUCT_DEFINITION_SHAPE('','',#1100);
#1102=SHAPE_ASPECT('','',#1101,.T.);
#1103=SHAPE_ASPECT('knurl face','',#4,.T.);
#1104=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#1103,#10,#105);
#1105=SHAPE_DEFINING_RELATIONSHIP('','applied shape',#1103,#1102);
#1106=PROPERTY_DEFINITION('knurl','',#1100);
#1110=(LENGTH_MEASURE_WITH_UNIT()MEASURE_REPRESENTATION_ITEM()MEASURE_WITH_UNIT(LENGTH_MEASURE(6.25),#400)REPRESENTATION_ITEM('major diameter'));
#1111=(LENGTH_MEASURE_WITH_UNIT()MEASURE_REPRESENTATION_ITEM()MEASURE_WITH_UNIT(LENGTH_MEASURE(6.),#400)REPRESENTATION_ITEM('nominal diameter'));
#1112=(LENGTH_MEASURE_WITH_UNIT()MEASURE_REPRESENTATION_ITEM()MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),#400)REPRESENTATION_ITEM('diametral pitch'));
#1012=DIMENSIONAL_EXPONENTS(0.,0.,0.,0.,0.,0.,0.);
#1013=(NAMED_UNIT(#1012)RATIO_UNIT());
#1114=(MEASURE_REPRESENTATION_ITEM()MEASURE_WITH_UNIT(COUNT_MEASURE({teeth}),#1013)REPRESENTATION_ITEM('number of teeth'));
#1115=SHAPE_REPRESENTATION_WITH_PARAMETERS('',(#1110,#1111,#1112,#1114),#399);
#1116=PROPERTY_DEFINITION_REPRESENTATION(#1106,#1115);"
        ))
    };
    let r = knurl("1.2E1");
    assert_eq!(
        r.parts[0].knurls[0].number_of_teeth,
        Some(pmi::Count(12)),
        "{:?}",
        r.findings
    );
    let r = knurl("12.7");
    assert!(r.parts[0].knurls.is_empty());
    assert!(
        r.findings
            .iter()
            .any(|f| f.ids[0] == 1100 && f.detail.contains("not a whole count")),
        "{:?}",
        r.findings
    );

    // A flatness magnitude typed as a length in the radian unit #401.
    let r = with_pmi(
        "#1520=SHAPE_ASPECT('','',#4,.T.);
#1521=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#1520,#10,#17);
#1522=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.2),#401);
#1523=FLATNESS_TOLERANCE('','',#1522,#1520);",
    );
    let f = r.findings.iter().find(|f| f.ids[0] == 1522).unwrap();
    assert_eq!(f.kind, pmi::FindingKind::Nonconformance, "{f:?}");
}

/// NIST STC-09 (AP242 edition 4): every BASIC_ROUND_HOLE_OCCURRENCE is read with its
/// definition (#8367 'Hole.2': Ø0.25 in -0/+0.003, through) and the placement its mapped item
/// gives it (#8373), as the file states them, and no finding says a hole definition is not
/// held. Each diameter dimension on hole occurrences (the NIST and OpenCascade cross-checks
/// check those against their oracles) states the nominal and deviations of the holes' own
/// definition.
#[test]
fn reads_hole_occurrences_with_their_definition() {
    use pmi::*;
    let (_, r, doc) = nist_fixture("nist_stc_09");
    let p = &r.parts[0];
    let prov = &r.provenance.parts[0];
    assert_eq!(p.holes.len(), 25, "25 BASIC_ROUND_HOLE_OCCURRENCEs");
    let left: Vec<&Finding> = r
        .findings
        .iter()
        .filter(|f| f.entity == "basic_round_hole_occurrence")
        .collect();
    assert!(left.is_empty(), "{left:?}");
    let occurrence = |h: &Hole| {
        root(
            &doc,
            &prov.features[h.feature.0],
            &["basic_round_hole_occurrence"],
        )
    };
    let d = |s: &str| Decimal::parse(s).unwrap();
    let inch = |s: &str| Length {
        value: d(s),
        unit: LengthUnit::Inch,
    };
    let hole2 = p.holes.iter().find(|h| occurrence(h) == 8369).unwrap();
    let def = &hole2.definition;
    assert_eq!(def.name, "Hole.2");
    assert_eq!(def.diameter, inch("0.25"));
    let tol = def.diameter_tolerance.as_ref().unwrap();
    assert_eq!(tol.lower().as_length(), Some(&inch("-0.0")));
    assert_eq!(tol.upper().as_length(), Some(&inch("0.003")));
    assert!(def.through && def.depth.is_none() && def.depth_tolerance.is_none());
    assert_eq!(
        def.placement,
        Placement {
            origin: [inch("0."), inch("0."), inch("0.")],
            axis: Some(Direction([d("0."), d("0."), d("1.")])),
            ref_direction: Some(Direction([d("1."), d("0."), d("0.")])),
        }
    );
    assert_eq!(
        hole2.placement,
        Some(Placement {
            origin: [inch("0.75"), inch("0.1196"), inch("-5.1")],
            axis: Some(Direction([d("0.0"), d("-1.0"), d("0.0")])),
            ref_direction: None,
        })
    );
    // Its occurrences share it; the definition's id is in each one's provenance only.
    let mut shared: Vec<u64> = p
        .holes
        .iter()
        .filter(|h| h.definition == *def)
        .map(occurrence)
        .collect();
    shared.sort_unstable();
    assert_eq!(shared, [8369, 8378, 8387, 8617, 8626, 8635]);
    for h in &p.holes {
        assert_eq!(
            prov.features[h.feature.0].contains(&8367),
            h.definition == *def
        );
    }
    // Every diameter dimension on hole occurrences (directly or as a group of them) states
    // its holes' definition.
    let holes_of = |f: FeatureId| -> Vec<&Hole> {
        let members = match &p.features[f.0] {
            Feature::Group { members, .. } => members.clone(),
            _ => vec![f],
        };
        p.holes
            .iter()
            .filter(|h| members.contains(&h.feature))
            .collect()
    };
    let mut checked = 0;
    for dim in &p.dimensions {
        let DimensionKind::Size {
            feature,
            kind: SizeKind::Diameter,
            ..
        } = &dim.kind
        else {
            continue;
        };
        let hs = holes_of(*feature);
        if hs.is_empty() {
            continue;
        }
        let nominal = dim.nominal.as_ref().unwrap();
        let DimTolerance::Deviations(b) = &dim.tolerance else {
            panic!("{dim:?}");
        };
        for h in hs {
            let def = &h.definition;
            assert_eq!(nominal.as_length(), Some(&def.diameter), "{}", def.name);
            let t = def.diameter_tolerance.as_ref().unwrap();
            assert_eq!(b.lower().si(), t.lower().si(), "{}", def.name);
            assert_eq!(b.upper().si(), t.upper().si(), "{}", def.name);
        }
        checked += 1;
    }
    assert!(checked >= 8, "{checked} diameter dimensions on holes");
    // The writer does not write hole definitions: each is refused by name, never dropped
    // (docs/step-ap242.md, Q-holes).
    let dir = common::fixtures().join("ap242/nist");
    let bytes = file_bytes(&dir.join("nist_stc_09_asme1_ap242-e4.stp.gz"));
    let parts = read_part_definitions(&bytes).expect("part definitions");
    use pmi::write::{ItemRef, PresentationPolicy, WriteError};
    let pmi = [(PartId(0), p.clone())];
    match pmi::write::write(
        &doc,
        &parts,
        &pmi,
        Mode::Replace,
        PresentationPolicy::RemovePresentation,
    ) {
        Err(WriteError::Refused(list)) => {
            for i in 0..p.holes.len() {
                assert!(
                    list.iter().any(|r| r.item == ItemRef::Hole(i)),
                    "hole {i} not refused: {list:?}"
                );
            }
        }
        other => panic!("holes not refused: {:?}", other.err()),
    }
}

/// What is not held of a hole definition keeps the occurrence's finding, naming it: here a
/// property of definition #8405 ('Hole.3', four occurrences) added to STC-09; the other
/// definitions' occurrences are still held.
#[test]
fn hole_definition_not_held_is_reported() {
    let dir = common::fixtures().join("ap242/nist");
    let text =
        String::from_utf8(file_bytes(&dir.join("nist_stc_09_asme1_ap242-e4.stp.gz"))).unwrap();
    let data = text.find("DATA;").unwrap();
    let at = data + text[data..].find("ENDSEC;").unwrap();
    let edited = format!(
        "{}#999999=PROPERTY_DEFINITION('extra','',#8405);\n{}",
        &text[..at],
        &text[at..]
    );
    let r = read_bytes(edited.into_bytes());
    let mut occ: Vec<u64> = r
        .findings
        .iter()
        .filter(|f| f.entity == "basic_round_hole_occurrence")
        .inspect(|f| {
            assert_eq!(f.kind, pmi::FindingKind::NotModelled, "{f:?}");
            assert!(
                f.detail.contains(
                    "what refers to the definition is not held: property_definition #999999"
                ),
                "{f:?}"
            );
            assert!(f.ids.contains(&8405), "{f:?}");
        })
        .map(|f| f.ids[0])
        .collect();
    occ.sort_unstable();
    assert_eq!(occ, [8406, 8415, 8424, 8433]);
    assert_eq!(r.parts[0].holes.len(), 21);
}
