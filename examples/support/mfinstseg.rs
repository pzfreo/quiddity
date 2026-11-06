//! Independent dataset evaluation shared by the example and the external-data ratchet.
use std::collections::{BTreeMap, BTreeSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use serde::{Deserialize, Serialize};

pub type Faces = BTreeSet<usize>;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Class {
    pub id: usize,
    pub name: String,
    pub families: Vec<String>,
    /// Per-family predicates over published record fields, independent of labels.
    #[serde(default)]
    pub selectors: BTreeMap<String, Vec<Selector>>,
}

/// A JSON pointer into a published record, with the geometric justification in the mapping.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Selector {
    pub field: String,
    pub evidence: String,
    #[serde(flatten)]
    pub predicate: Predicate,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Predicate {
    Equals {
        value: serde_json::Value,
    },
    OneOf {
        values: Vec<serde_json::Value>,
    },
    Length {
        value: usize,
    },
    NonNull,
    All {
        selector: Box<Selector>,
    },
    /// Exclude only another proved canonical profile; altered profiles stay ambiguous.
    CompatiblePolygon {
        shape: CanonicalPolygon,
        decimals: u32,
    },
    LipBevelledBlindSlot {
        coordinate_decimals: u32,
        interval_decimals: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalPolygon {
    Triangle,
    Rectangle,
    RegularHexagon,
}

/// Classify only complete canonical polygons at their published coordinate precision.
/// A treatment, intersection, arc or unresolved edge leaves the old correspondence intact.
fn canonical_polygon(value: &serde_json::Value, decimals: u32) -> Option<CanonicalPolygon> {
    assert!(decimals <= 12, "published coordinate precision");
    let vertices = value.as_array().expect("published polygon vertices");
    let n = vertices.len();
    if ![3, 4, 6].contains(&n) {
        return None;
    }
    let points: Option<Vec<[f64; 2]>> = vertices
        .iter()
        .map(|v| {
            let point = if v.is_object() {
                if v["bulge"].as_f64()? != 0.0 {
                    return None;
                }
                &v["point"]
            } else {
                v
            };
            let p = point.as_array()?;
            (p.len() == 2).then_some([p[0].as_f64()?, p[1].as_f64()?])
        })
        .collect();
    let points = points?;
    let edges: Vec<[f64; 2]> = (0..n)
        .map(|i| std::array::from_fn(|j| points[(i + 1) % n][j] - points[i][j]))
        .collect();
    let lengths: Vec<f64> = edges.iter().map(|v| v[0].hypot(v[1])).collect();
    // Each coordinate is rounded to this half-grid. Difference-vector error is at most
    // one grid per component; dot/cross bounds follow from those endpoint errors.
    let q = 0.5 * 10_f64.powi(-(decimals as i32));
    if lengths.iter().any(|&length| length <= 4.0 * q) {
        return None;
    }
    let bounds: Vec<f64> = (0..n)
        .map(|i| {
            let (a, b) = (edges[i], edges[(i + 1) % n]);
            2.0 * q * (a[0].abs() + a[1].abs() + b[0].abs() + b[1].abs()) + 8.0 * q * q
        })
        .collect();
    let crosses: Vec<f64> = (0..n)
        .map(|i| {
            let (a, b) = (edges[i], edges[(i + 1) % n]);
            a[0] * b[1] - a[1] * b[0]
        })
        .collect();
    if !crosses.iter().zip(&bounds).all(|(c, b)| *c > *b)
        && !crosses.iter().zip(&bounds).all(|(c, b)| *c < -*b)
    {
        return None;
    }
    if n == 3 {
        return Some(CanonicalPolygon::Triangle);
    }
    let dots: Vec<f64> = (0..n)
        .map(|i| {
            let (a, b) = (edges[i], edges[(i + 1) % n]);
            a[0] * b[0] + a[1] * b[1]
        })
        .collect();
    if n == 4 && dots.iter().zip(&bounds).all(|(d, b)| d.abs() <= *b) {
        return Some(CanonicalPolygon::Rectangle);
    }
    if n == 6 {
        let min = lengths.iter().copied().fold(f64::INFINITY, f64::min);
        let max = lengths.iter().copied().fold(0.0, f64::max);
        let equal_edges = max - min <= 4.0 * 2_f64.sqrt() * q;
        let sixty_degree_turns = (0..n)
            .all(|i| (dots[i] - 0.5 * lengths[i] * lengths[(i + 1) % n]).abs() <= 2.0 * bounds[i]);
        if equal_edges && sixty_degree_turns {
            return Some(CanonicalPolygon::RegularHexagon);
        }
    }
    None
}

/// A proved open section with one flared lip bevel, two parallel sides and a flat floor.
/// Its run must have a unique slot role: strictly longer than both width and depth.
fn lip_bevelled_blind_slot(
    value: &serde_json::Value,
    coordinate_decimals: u32,
    interval_decimals: u32,
) -> bool {
    assert!(coordinate_decimals <= 12 && interval_decimals <= 12);
    let ends: Option<Vec<&str>> = ["low", "high"]
        .iter()
        .map(|end| {
            let end = &value["ends"][end];
            let gradient = end["surface"]["gradient"].as_array()?;
            if end["surface"]["type"] != "plane"
                || gradient.len() != 2
                || gradient.iter().any(|v| v.as_f64() != Some(0.0))
            {
                return None;
            }
            end["condition"].as_str()
        })
        .collect();
    if !matches!(
        ends.as_deref(),
        Some(["open", "capped"] | ["capped", "open"])
    ) || value["profile"]["closure"] != "open"
    {
        return false;
    }
    let Some(boundary) = value["profile"]["boundary"].as_array() else {
        return false;
    };
    if boundary.len() != 5 || boundary.iter().any(|v| v["bulge"].as_f64() != Some(0.0)) {
        return false;
    }
    let points: Option<Vec<[f64; 2]>> = boundary
        .iter()
        .map(|v| Some([v["point"][0].as_f64()?, v["point"][1].as_f64()?]))
        .collect();
    let Some(points) = points else {
        return false;
    };
    let delta: [f64; 2] = std::array::from_fn(|i| points[4][i] - points[0][i]);
    let span = delta[0].hypot(delta[1]);
    let q = 0.5 * 10_f64.powi(-(coordinate_decimals as i32));
    if span <= 8.0 * q {
        return false;
    }
    let width_direction = delta.map(|v| v / span);
    let depth_direction = [-width_direction[1], width_direction[0]];
    let radius = points
        .iter()
        .map(|p| (p[0] - points[0][0]).hypot(p[1] - points[0][1]))
        .fold(0.0, f64::max);
    // Endpoint rounding affects both the projected points and their opening-line basis.
    let tolerance = 8.0 * q * (1.0 + radius / span);
    let mut coordinates: Vec<[f64; 2]> = points
        .iter()
        .map(|p| {
            [width_direction, depth_direction]
                .map(|d| (p[0] - points[0][0]) * d[0] + (p[1] - points[0][1]) * d[1])
        })
        .collect();
    if coordinates.iter().map(|p| p[1]).sum::<f64>() < 0.0 {
        coordinates.iter_mut().for_each(|p| p[1] = -p[1]);
    }
    let Some(low) = value["run_interval"][0].as_f64() else {
        return false;
    };
    let Some(high) = value["run_interval"][1].as_f64() else {
        return false;
    };
    let length = high - low;
    let role_tolerance = tolerance + 10_f64.powi(-(interval_decimals as i32));
    let matches = |p: &[[f64; 2]]| {
        let [a, bevel, side, floor, opposite] = p else {
            return false;
        };
        let near = |a: f64, b: f64| (a - b).abs() <= tolerance;
        let depth = side[1];
        let width = floor[0] - side[0];
        bevel[0] > a[0] + tolerance
            && bevel[1] > tolerance
            && bevel[1] < depth - tolerance
            && near(bevel[0], side[0])
            && near(side[1], floor[1])
            && near(floor[0], opposite[0])
            && near(opposite[1], 0.0)
            && width > tolerance
            && length > width + role_tolerance
            && length > depth + role_tolerance
    };
    matches(&coordinates)
        || matches(
            &coordinates
                .iter()
                .rev()
                .map(|p| [span - p[0], p[1]])
                .collect::<Vec<_>>(),
        )
}

impl Selector {
    fn matches(&self, record: &serde_json::Value) -> bool {
        let value = record
            .pointer(&self.field)
            .expect("mapping field is published");
        match &self.predicate {
            Predicate::Equals { value: expected } => value == expected,
            Predicate::OneOf { values } => values.contains(value),
            Predicate::Length { value: n } => value.as_array().expect("array field").len() == *n,
            Predicate::NonNull => !value.is_null(),
            Predicate::All { selector } => value
                .as_array()
                .expect("array field")
                .iter()
                .all(|element| selector.matches(element)),
            Predicate::CompatiblePolygon { shape, decimals } => {
                canonical_polygon(value, *decimals).is_none_or(|found| found == *shape)
            }
            Predicate::LipBevelledBlindSlot {
                coordinate_decimals,
                interval_decimals,
            } => lip_bevelled_blind_slot(value, *coordinate_decimals, *interval_decimals),
        }
    }
}

/// The same serialized fields readers receive; no topology, labels or model identity here.
fn family_records(features: &quiddity::features::Features, family: &str) -> serde_json::Value {
    match family {
        "holes" => serde_json::to_value(&features.holes),
        "blends" => serde_json::to_value(&features.blends),
        "section_recesses" => serde_json::to_value(&features.section_recesses),
        "section_passages" => serde_json::to_value(&features.section_passages),
        "prismatic_pockets" => serde_json::to_value(&features.prismatic_pockets),
        "pockets" => serde_json::to_value(&features.pockets),
        _ => panic!("selectors unsupported for family {family}"),
    }
    .expect("published records serialize")
}

pub fn mapping() -> Vec<Class> {
    #[derive(Deserialize)]
    struct Mapping {
        classes: Vec<Class>,
    }
    let mapping: Mapping =
        serde_json::from_str(include_str!("../../tests/fixtures/mfinstseg_mapping.json")).unwrap();
    assert_eq!(mapping.classes.len(), 25);
    for (i, c) in mapping.classes.iter().enumerate() {
        assert_eq!(c.id, i);
        for (family, selectors) in &c.selectors {
            assert!(
                c.families.contains(family),
                "selector family must be mapped"
            );
            assert!(!selectors.is_empty());
            fn validate(selector: &Selector) {
                assert!(!selector.evidence.trim().is_empty());
                assert!(selector.field.starts_with('/'));
                if let Predicate::All { selector } = &selector.predicate {
                    validate(selector);
                }
                if let Predicate::CompatiblePolygon { decimals, .. } = &selector.predicate {
                    assert!(*decimals <= 12);
                }
                if let Predicate::LipBevelledBlindSlot {
                    coordinate_decimals,
                    interval_decimals,
                } = &selector.predicate
                {
                    assert!(*coordinate_decimals <= 12 && *interval_decimals <= 12);
                }
            }
            selectors.iter().for_each(validate);
        }
    }
    mapping.classes
}

/// Zero denominators score zero, never vacuous perfect precision or recall.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Counts {
    pub true_positive: u64,
    pub predicted: u64,
    pub labelled: u64,
}
impl Counts {
    pub fn precision(&self) -> f64 {
        ratio(self.true_positive, self.predicted)
    }
    pub fn recall(&self) -> f64 {
        ratio(self.true_positive, self.labelled)
    }
    fn add(&mut self, other: &Self) {
        self.true_positive += other.true_positive;
        self.predicted += other.predicted;
        self.labelled += other.labelled;
    }
}
fn ratio(n: u64, d: u64) -> f64 {
    if d == 0 { 0.0 } else { n as f64 / d as f64 }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Score {
    pub faces: Counts,
    /// None for semantic-only MFCAD++ labels: instances cannot be inferred from adjacency.
    pub instances: Option<Counts>,
}
impl Score {
    pub fn add(&mut self, other: &Self) {
        self.faces.add(&other.faces);
        if let Some(other) = &other.instances {
            self.instances.get_or_insert_default().add(other);
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ModelScore {
    pub id: String,
    pub scores: Vec<Score>,
    /// Read failures/panics remain in denominators as missed labelled faces and instances.
    pub error: Option<String>,
    pub section_recess_refusal: Option<String>,
    pub label_warnings: Vec<String>,
    pub instances_available: bool,
    pub accounting: Accounting,
}

/// Measured on the supported inferred-frame recognition document. Labels never select rules.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Accounting {
    pub feature_as_body: u64,
    pub stock_unexplained: u64,
    #[serde(default)]
    pub stock_body: u64,
    #[serde(default)]
    pub body_reasons: BTreeMap<quiddity::recognition::BodyReason, u64>,
    pub feature: u64,
    /// Non-stock labelled faces claimed by any record in the supported document.
    #[serde(default)]
    pub labelled_feature_claimed: u64,
    pub body: u64,
    pub refused: u64,
    pub unexplained: u64,
}
impl Accounting {
    pub fn add(&mut self, other: &Self) {
        self.feature_as_body += other.feature_as_body;
        self.stock_unexplained += other.stock_unexplained;
        self.stock_body += other.stock_body;
        for (&reason, &count) in &other.body_reasons {
            *self.body_reasons.entry(reason).or_default() += count;
        }
        self.feature += other.feature;
        self.labelled_feature_claimed += other.labelled_feature_claimed;
        self.body += other.body;
        self.refused += other.refused;
        self.unexplained += other.unexplained;
    }
    fn measure(groups: &[quiddity::recognition::FaceAccounting], seg: &[usize]) -> Self {
        use quiddity::recognition::FaceAccounting;
        assert_eq!(groups.len(), seg.len());
        let mut out = Self::default();
        for (group, &label) in groups.iter().zip(seg) {
            match group {
                FaceAccounting::Feature { .. } => {
                    out.feature += 1;
                    out.labelled_feature_claimed += u64::from(label != 24);
                }
                FaceAccounting::Body { reason } => {
                    out.body += 1;
                    out.stock_body += u64::from(label == 24);
                    *out.body_reasons.entry(*reason).or_default() += 1;
                    out.feature_as_body += u64::from(label != 24);
                }
                FaceAccounting::Refused { .. } => out.refused += 1,
                FaceAccounting::Unexplained => {
                    out.unexplained += 1;
                    out.stock_unexplained += u64::from(label == 24);
                }
            }
        }
        out
    }
}

#[derive(Deserialize)]
struct Labels {
    seg: BTreeMap<String, usize>,
    inst: Vec<Vec<u8>>,
}

#[derive(Debug)]
pub struct Truth {
    pub seg: Vec<usize>,
    pub instances: Option<Vec<Vec<Faces>>>,
    pub warnings: Vec<String>,
}
impl Truth {
    pub fn read(root: &Path, id: &str) -> Result<Self, String> {
        let bytes = std::fs::read(root.join("labels").join(format!("{id}.json")))
            .map_err(|e| e.to_string())?;
        let mut rows: Vec<(String, Labels)> =
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if rows.len() != 1 {
            return Err(format!("{id}: expected one label record"));
        }
        let mut warnings = Vec::new();
        let declared = &rows[0].0;
        if declared != id {
            if declared.is_empty()
                || !declared
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_')
            {
                return Err(format!("{id}: invalid label identity"));
            }
            // A few published files are renamed copies. Accept only provably identical
            // DATA records and byte-identical labels, never just equal face counts.
            fn records(path: &Path) -> Result<BTreeMap<u64, String>, String> {
                use quiddity::kernel::p21::{Document, encode};
                let doc = Document::parse(std::fs::read(path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                doc.graph()
                    .entities
                    .iter()
                    .map(|(&id, e)| encode(e).map(|s| (id, s)).map_err(|e| e.to_string()))
                    .collect()
            }
            let original_labels =
                std::fs::read(root.join("labels").join(format!("{declared}.json")))
                    .map_err(|e| e.to_string())?;
            if original_labels != bytes
                || records(&root.join("steps").join(format!("{id}.step")))?
                    != records(&root.join("steps").join(format!("{declared}.step")))?
            {
                return Err(format!(
                    "{id}: label identity differs and is not an identical STEP/label duplicate"
                ));
            }
            warnings.push(format!(
                "label identity {declared}: verified identical STEP DATA records and label bytes"
            ));
        }
        let labels = rows.pop().unwrap().1;
        Self::parse(labels, warnings, id)
    }

    fn parse(labels: Labels, mut warnings: Vec<String>, id: &str) -> Result<Self, String> {
        let seg: Vec<_> = (0..labels.seg.len())
            .map(|i| {
                labels
                    .seg
                    .get(&i.to_string())
                    .copied()
                    .ok_or_else(|| format!("{id}: non-contiguous face labels"))
            })
            .collect::<Result<_, _>>()?;
        if seg.iter().any(|&c| c >= 25) {
            return Err(format!("{id}: unknown class"));
        }
        let n = seg.len();
        if labels.inst.len() != n
            || labels
                .inst
                .iter()
                .any(|r| r.len() != n || r.iter().any(|&v| v > 1))
        {
            return Err(format!("{id}: invalid instance matrix dimensions/values"));
        }
        let mut instances = vec![Vec::new(); 25];
        let rows: Vec<Faces> = labels
            .inst
            .iter()
            .map(|r| {
                r.iter()
                    .enumerate()
                    .filter_map(|(i, &x)| (x == 1).then_some(i))
                    .collect()
            })
            .collect();
        let mut seen = BTreeSet::new();
        let mut unassigned = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            if row.is_empty() {
                if seg[i] != 24 {
                    unassigned.push(i);
                }
                continue;
            }
            if !row.contains(&i)
                || row.iter().any(|&j| rows[j] != *row || seg[j] != seg[i])
                || seg[i] == 24
            {
                return Err(format!(
                    "{id}: instance is not a same-class equivalence clique at face {i}"
                ));
            }
            if seen.insert(row.clone()) {
                instances[seg[i]].push(row.clone());
            }
        }
        let instances = if unassigned.is_empty() {
            Some(instances)
        } else {
            warnings.push(format!("semantic feature faces {unassigned:?} have no instance annotation; face metrics retained, instance metrics unavailable for this model"));
            None
        };
        Ok(Self {
            seg,
            instances,
            warnings,
        })
    }

    /// AAGNet's MFCAD++ extractor reads the numeric STEP representation-item name.
    /// Bind it through the Rust face's source entity, rather than guessing STEP text order.
    pub fn mfcadpp(part: &quiddity::Part, bytes: Vec<u8>) -> Result<Self, String> {
        use quiddity::kernel::p21::{Attribute, Document, RawEntity};
        let doc = Document::parse(bytes).map_err(|e| e.to_string())?;
        let mut seg = Vec::new();
        for i in 0..part.faces.len() {
            let source = part.face_source(i).ok_or("missing face source")?;
            let e = doc.get(source.entity).ok_or("missing source entity")?;
            let RawEntity::Simple {
                name: kind,
                attributes,
                ..
            } = e
            else {
                return Err("complex face entity".into());
            };
            if kind != "ADVANCED_FACE" && kind != "FACE_SURFACE" {
                return Err("source is not a face".into());
            }
            let Some(Attribute::String(name)) = attributes.first() else {
                return Err("missing face name".into());
            };
            let c: usize = name
                .parse()
                .map_err(|_| format!("face {i}: non-numeric class {name:?}"))?;
            if c >= 25 {
                return Err(format!("face {i}: unknown class {c}"));
            }
            seg.push(c);
        }
        Ok(Self {
            seg,
            instances: None,
            warnings: Vec::new(),
        })
    }
}

/// Maximum-cardinality one-to-one matching at inclusive face-set IoU >= 0.5.
/// An augmenting path avoids the order dependence and undercounting of greedy matching.
pub fn matched(predicted: &[Faces], labelled: &[Faces]) -> u64 {
    let edges: Vec<Vec<usize>> = predicted
        .iter()
        .map(|p| {
            labelled
                .iter()
                .enumerate()
                .filter_map(|(i, g)| {
                    let intersection = p.intersection(g).count();
                    let union = p.len() + g.len() - intersection;
                    (union > 0 && 2 * intersection >= union).then_some(i)
                })
                .collect()
        })
        .collect();
    fn augment(
        p: usize,
        edges: &[Vec<usize>],
        used: &mut [bool],
        owner: &mut [Option<usize>],
    ) -> bool {
        for &g in &edges[p] {
            if used[g] {
                continue;
            }
            used[g] = true;
            if owner[g].is_none_or(|old| augment(old, edges, used, owner)) {
                owner[g] = Some(p);
                return true;
            }
        }
        false
    }
    let mut owner = vec![None; labelled.len()];
    (0..predicted.len())
        .filter(|&p| augment(p, &edges, &mut vec![false; labelled.len()], &mut owner))
        .count() as u64
}

pub fn score(
    truth: &Truth,
    features: &quiddity::features::Features,
    classes: &[Class],
) -> Vec<Score> {
    classes
        .iter()
        .map(|class| {
            let labelled: Faces = truth
                .seg
                .iter()
                .enumerate()
                .filter_map(|(i, &c)| (c == class.id).then_some(i))
                .collect();
            // Same physical face set under two mapped family names is one instance prediction.
            let instances: BTreeSet<Faces> = class
                .families
                .iter()
                .flat_map(|family| {
                    let selectors = class.selectors.get(family);
                    let records = selectors.map(|_| family_records(features, family));
                    let defining = features.defining.get(family.as_str());
                    if let Some(records) = &records {
                        assert_eq!(
                            records.as_array().unwrap().len(),
                            defining.map_or(0, Vec::len),
                            "records and defining faces must have the same indices"
                        );
                    }
                    defining
                        .into_iter()
                        .flatten()
                        .enumerate()
                        .filter_map(move |(i, faces)| {
                            selectors
                                .is_none_or(|selectors| {
                                    selectors.iter().all(|selector| {
                                        selector.matches(&records.as_ref().unwrap()[i])
                                    })
                                })
                                .then_some(faces)
                        })
                })
                .map(|fs| fs.iter().copied().collect())
                .collect();
            let predicted: Faces = instances.iter().flatten().copied().collect();
            assert!(
                predicted.iter().all(|&i| i < truth.seg.len()),
                "out-of-range predicted face"
            );
            let face_counts = Counts {
                true_positive: predicted.intersection(&labelled).count() as u64,
                predicted: predicted.len() as u64,
                labelled: labelled.len() as u64,
            };
            let instance_counts = truth.instances.as_ref().map(|g| Counts {
                true_positive: matched(
                    &instances.iter().cloned().collect::<Vec<_>>(),
                    &g[class.id],
                ),
                predicted: instances.len() as u64,
                labelled: g[class.id].len() as u64,
            });
            Score {
                faces: face_counts,
                instances: instance_counts,
            }
        })
        .collect()
}

pub fn evaluate(
    root: &Path,
    id: &str,
    mfcadpp: bool,
    classes: &[Class],
) -> Result<ModelScore, String> {
    let mut truth = if mfcadpp {
        None
    } else {
        Some(Truth::read(root, id)?)
    };
    let path = if mfcadpp {
        root.join(format!("{id}.step"))
    } else {
        root.join("steps").join(format!("{id}.step"))
    };
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<_, String> {
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let part = quiddity::read_step(&bytes).map_err(|e| e.to_string())?;
        if mfcadpp {
            truth = Some(Truth::mfcadpp(&part, bytes)?);
        }
        if part.faces.len() != truth.as_ref().unwrap().seg.len() {
            return Err("STEP/label face count mismatch".into());
        }
        // Keep the historical caller-space feature scorer unchanged. Accounting uses
        // the supported default route, including frame refusal and document precedence.
        let features = quiddity::features::recognise(&part);
        let recognition = quiddity::correspondence::recognise(&part);
        let doc = quiddity::recognition::document(&recognition, part.faces.len());
        let accounting = Accounting::measure(
            doc.face_accounting.as_ref().unwrap(),
            &truth.as_ref().unwrap().seg,
        );
        Ok((features, accounting))
    }));
    let (features, accounting, error) = match result {
        Ok(Ok((f, a))) => (Some(f), Some(a), None),
        Ok(Err(e)) => (None, None, Some(e)),
        Err(e) => (
            None,
            None,
            Some(
                e.downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "recognition panic".into()),
            ),
        ),
    };
    // Without labels no accuracy denominator is possible: propagate, never silently skip.
    let truth =
        truth.ok_or_else(|| format!("{id}: {}", error.as_deref().unwrap_or("missing labels")))?;
    let empty = quiddity::features::Features::default();
    Ok(ModelScore {
        id: id.into(),
        // Failed recognition is fully unexplained, never silently absent from totals.
        accounting: accounting.unwrap_or_else(|| Accounting {
            unexplained: truth.seg.len() as u64,
            stock_unexplained: truth.seg.iter().filter(|&&c| c == 24).count() as u64,
            ..Accounting::default()
        }),
        scores: score(&truth, features.as_ref().unwrap_or(&empty), classes),
        error,
        section_recess_refusal: features
            .and_then(|f| f.section_recess_refusal.map(|e| e.to_string())),
        label_warnings: truth.warnings,
        instances_available: truth.instances.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bevelled_slot_geometry(points: Vec<[f64; 2]>, length: f64) -> serde_json::Value {
        serde_json::json!({
            "profile": {"closure":"open", "boundary":points.into_iter().map(|p|
                serde_json::json!({"point":p,"bulge":0.0})).collect::<Vec<_>>()},
            "ends":{
                "low":{"condition":"capped","surface":{"type":"plane","gradient":[0.,0.]}},
                "high":{"condition":"open","surface":{"type":"plane","gradient":[0.,0.]}}
            },
            "run_interval":[0.,length]
        })
    }

    #[test]
    fn lip_bevelled_slot_requires_a_unique_long_run_after_section_motion() {
        let points = [[0., 0.], [1., 1.], [1., 4.], [6., 4.], [6., 0.]];
        for scale in [0.1, 1., 100.] {
            for angle in [0_f64, 0.72, 2.34] {
                for reverse in [false, true] {
                    let mut moved: Vec<[f64; 2]> = points
                        .iter()
                        .map(|p| {
                            let p = [
                                12.3 + scale * (angle.cos() * p[0] - angle.sin() * p[1]),
                                -2.1 + scale * (angle.sin() * p[0] + angle.cos() * p[1]),
                            ];
                            p.map(|x| (x * 1e4).round() / 1e4)
                        })
                        .collect();
                    if reverse {
                        moved.reverse();
                    }
                    let mut geometry = bevelled_slot_geometry(moved, 20. * scale);
                    assert!(lip_bevelled_blind_slot(&geometry, 4, 3));
                    geometry["run_interval"] = serde_json::json!([0., 5. * scale]);
                    assert!(!lip_bevelled_blind_slot(&geometry, 4, 3));
                    geometry["run_interval"] = serde_json::json!([0., 4. * scale]);
                    assert!(!lip_bevelled_blind_slot(&geometry, 4, 3));
                }
            }
        }
    }

    #[test]
    fn lip_bevelled_slot_refuses_other_ends_curves_and_nonrectangular_sides() {
        let points = vec![[0., 0.], [1., 1.], [1., 4.], [6., 4.], [6., 0.]];
        let original = bevelled_slot_geometry(points, 20.);
        for (path, replacement) in [
            ("/ends/low/condition", serde_json::json!("open")),
            ("/ends/high/condition", serde_json::json!("capped")),
            ("/ends/low/surface/gradient", serde_json::json!([0.1, 0.])),
            ("/profile/closure", serde_json::json!("closed")),
            ("/profile/boundary/0/bulge", serde_json::json!(0.2)),
            ("/profile/boundary/1/point", serde_json::json!([-1., 1.])),
            ("/profile/boundary/1/point", serde_json::json!([1., 4.])),
            ("/profile/boundary/2/point", serde_json::json!([1.2, 4.])),
            ("/profile/boundary/3/point", serde_json::json!([6., 4.2])),
        ] {
            let mut geometry = original.clone();
            *geometry.pointer_mut(path).unwrap() = replacement;
            assert!(!lip_bevelled_blind_slot(&geometry, 4, 3), "{path}");
        }
    }

    #[test]
    fn canonical_polygons_survive_pose_scale_winding_and_publication_rounding() {
        let triangle = vec![[0., 0.], [3., 0.], [1., 2.]];
        let rectangle = vec![[0., 0.], [3., 0.], [3., 2.], [0., 2.]];
        let hexagon: Vec<[f64; 2]> = (0..6)
            .map(|i| {
                let angle = i as f64 * std::f64::consts::TAU / 6.;
                [angle.cos(), angle.sin()]
            })
            .collect();
        for (shape, points) in [
            (CanonicalPolygon::Triangle, triangle),
            (CanonicalPolygon::Rectangle, rectangle),
            (CanonicalPolygon::RegularHexagon, hexagon),
        ] {
            for decimals in [3, 4] {
                let grid = 10_f64.powi(decimals as i32);
                for scale in [0.1, 1., 200.] {
                    for angle in [0_f64, 0.72, 2.34] {
                        for winding in [false, true] {
                            let mut moved: Vec<[f64; 2]> = points
                                .iter()
                                .map(|p| {
                                    let p = [
                                        123.456 + scale * (angle.cos() * p[0] - angle.sin() * p[1]),
                                        -32.1 + scale * (angle.sin() * p[0] + angle.cos() * p[1]),
                                    ];
                                    p.map(|x| (x * grid).round() / grid)
                                })
                                .collect();
                            if winding {
                                moved.reverse();
                            }
                            assert_eq!(
                                canonical_polygon(&serde_json::json!(moved), decimals),
                                Some(shape),
                                "{shape:?} at precision {decimals}, scale {scale}, angle {angle}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn altered_profiles_and_unresolved_edges_stay_ambiguous() {
        for points in [
            // A rectangle with a clipped corner.
            vec![[0., 0.], [4., 0.], [4., 3.], [3., 4.], [0., 4.]],
            // An extra collinear triangle vertex cannot prove a rectangle.
            vec![[0., 0.], [4., 0.], [3., 1.], [0., 4.]],
            // The concave section of two intersecting rectangular cuts.
            vec![[0., 0.], [4., 0.], [4., 2.], [2., 2.], [2., 4.], [0., 4.]],
            // A convex, irregular six-sided section is not a regular hexagon.
            vec![[0., 0.], [3., 0.], [4., 1.], [3., 3.], [0., 3.], [-1., 1.]],
            // A collapsed or nearly collinear edge cannot decide a subtype.
            vec![[0., 0.], [1., 0.], [1., 0.]],
            vec![[0., 0.], [4., 0.], [2., 0.0001]],
        ] {
            assert_eq!(canonical_polygon(&serde_json::json!(points), 4), None);
        }
        let curved = serde_json::json!([
            {"point":[0.,0.],"bulge":1.},
            {"point":[2.,0.],"bulge":0.},
            {"point":[0.,2.],"bulge":0.}
        ]);
        assert_eq!(canonical_polygon(&curved, 4), None);
    }

    #[test]
    fn compatible_polygon_excludes_only_a_different_complete_profile() {
        let selector: Selector = serde_json::from_value(serde_json::json!({
            "field":"/section", "op":"compatible_polygon", "shape":"triangle", "decimals":3,
            "evidence":"Exclude another complete canonical profile; altered profiles are ambiguous."
        }))
        .unwrap();
        assert!(selector.matches(&serde_json::json!({"section":[[0.,0.],[3.,0.],[1.,2.]]})));
        assert!(
            !selector.matches(&serde_json::json!({"section":[[0.,0.],[3.,0.],[3.,2.],[0.,2.]]}))
        );
        assert!(
            selector.matches(&serde_json::json!({"section":[[0.,0.],[4.,0.],[3.,1.],[0.,4.]]}))
        );
    }

    #[test]
    fn selectors_use_only_published_properties_and_preserve_record_indices() {
        let selector: Selector = serde_json::from_value(serde_json::json!({
            "field":"/classification/feature_kind", "op":"equals", "value":"channel",
            "evidence":"An open profile with both run ends open is a channel."
        }))
        .unwrap();
        assert!(
            selector.matches(&serde_json::json!({"classification":{"feature_kind":"channel"}}))
        );
        assert!(
            !selector.matches(&serde_json::json!({"classification":{"feature_kind":"passage"}}))
        );
        let mut features = quiddity::features::Features::default();
        use quiddity::features::holes::{Bottom, HoleRecord};
        features.holes = [Bottom::Flat, Bottom::Through]
            .into_iter()
            .map(|bottom| HoleRecord {
                axis: [0., 0., 1.],
                location: [0., 0., 0.].into(),
                diameter: 2.,
                depth: 3.,
                bottom,
                cbore: None,
                spotface: None,
                outer_steps: Vec::new(),
                exit_obstructions: Vec::new(),
                csink: None,
            })
            .collect();
        features.defining.insert("holes", vec![vec![0], vec![1]]);
        let class = Class {
            id: 0,
            name: "test".into(),
            families: vec!["holes".into()],
            selectors: BTreeMap::from([(
                "holes".into(),
                vec![serde_json::from_value(serde_json::json!({
                "field":"/bottom", "op":"equals", "value":"through", "evidence":"Two open ends."
            })).unwrap()],
            )]),
        };
        // Swapping truth labels changes the score, never the selected prediction or its index.
        for seg in [vec![0, 1], vec![1, 0]] {
            let truth = Truth {
                seg,
                instances: None,
                warnings: Vec::new(),
            };
            let scores = score(&truth, &features, std::slice::from_ref(&class));
            assert_eq!(scores[0].faces.predicted, 1);
            assert_eq!(scores[0].faces.true_positive, u64::from(truth.seg[1] == 0));
        }
    }

    #[test]
    fn accounting_measures_all_feature_labels_and_only_explicit_stock() {
        use quiddity::recognition::{BodyReason, FaceAccounting};
        let groups = vec![
            FaceAccounting::Body {
                reason: BodyReason::StockBoxSide,
            },
            FaceAccounting::Body {
                reason: BodyReason::StockBoxSide,
            },
            FaceAccounting::Unexplained,
            FaceAccounting::Unexplained,
            FaceAccounting::Feature {
                record_ids: vec!["holes/0".into()],
            },
            FaceAccounting::Feature {
                record_ids: vec!["plates/0".into()],
            },
        ];
        let counts = Accounting::measure(&groups, &[23, 24, 24, 0, 12, 24]);
        assert_eq!(counts.feature, 2);
        assert_eq!(counts.labelled_feature_claimed, 1);
        assert_eq!(counts.feature_as_body, 1);
        assert_eq!(counts.stock_unexplained, 1);
        assert_eq!(counts.stock_body, 1);
        assert_eq!(counts.body_reasons[&BodyReason::StockBoxSide], 2);
        assert_eq!(counts.body, 2);
        assert_eq!(counts.unexplained, 2);
    }
    fn faces(fs: &[usize]) -> Faces {
        fs.iter().copied().collect()
    }
    #[test]
    fn incomplete_instances_retain_semantics_but_invalid_cliques_fail() {
        let partial = Labels {
            seg: BTreeMap::from([("0".into(), 0), ("1".into(), 0)]),
            inst: vec![vec![1, 0], vec![0, 0]],
        };
        let truth = Truth::parse(partial, Vec::new(), "partial").unwrap();
        assert_eq!(truth.seg, vec![0, 0]);
        assert!(truth.instances.is_none());
        assert_eq!(truth.warnings.len(), 1);
        let invalid = Labels {
            seg: BTreeMap::from([("0".into(), 0), ("1".into(), 0)]),
            inst: vec![vec![1, 1], vec![0, 1]],
        };
        assert!(Truth::parse(invalid, Vec::new(), "invalid").is_err());
    }
    #[test]
    fn inclusive_iou_and_maximum_matching() {
        assert_eq!(
            matched(&[faces(&[0, 1]), faces(&[0])], &[faces(&[0]), faces(&[1])]),
            2
        );
        assert_eq!(matched(&[faces(&[0, 1, 2])], &[faces(&[0])]), 0);
        assert_eq!(matched(&[Faces::new()], &[Faces::new()]), 0);
        assert_eq!(matched(&[faces(&[0]), faces(&[0])], &[faces(&[0])]), 1);
    }
    #[test]
    fn empty_face_claim_is_a_false_positive_instance() {
        let classes = vec![Class {
            id: 0,
            name: "test".into(),
            families: vec!["holes".into()],
            selectors: BTreeMap::new(),
        }];
        let truth = Truth {
            seg: vec![0],
            instances: Some(vec![vec![faces(&[0])]]),
            warnings: Vec::new(),
        };
        let defining = BTreeMap::from([("holes", vec![vec![]])]);
        let s = score(
            &truth,
            &quiddity::features::Features {
                defining,
                ..Default::default()
            },
            &classes,
        );
        assert_eq!(
            s[0].instances,
            Some(Counts {
                true_positive: 0,
                predicted: 1,
                labelled: 1
            })
        );
    }
    #[test]
    fn defining_only_deduplicated_predictions_do_not_consult_labels() {
        let classes = vec![Class {
            id: 0,
            name: "test".into(),
            families: vec!["holes".into(), "pockets".into()],
            selectors: BTreeMap::new(),
        }];
        let truth = Truth {
            seg: vec![0, 1, 0],
            instances: Some(vec![vec![faces(&[0, 2])]]),
            warnings: Vec::new(),
        };
        let defining = BTreeMap::from([("holes", vec![vec![0, 1]]), ("pockets", vec![vec![0, 1]])]);
        let s = score(
            &truth,
            &quiddity::features::Features {
                defining,
                ..Default::default()
            },
            &classes,
        );
        assert_eq!(
            s[0].faces,
            Counts {
                true_positive: 1,
                predicted: 2,
                labelled: 2
            }
        );
        assert_eq!(
            s[0].instances,
            Some(Counts {
                true_positive: 0,
                predicted: 1,
                labelled: 1
            })
        );
        assert_eq!(Counts::default().precision(), 0.0);
        assert_eq!(Counts::default().recall(), 0.0);
    }
}
