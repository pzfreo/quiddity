//! The versioned recognition document (review finding M5): one record per recognised feature,
//! with its family, Python's record class name, its defining faces and its record, for
//! draftwright to read in place of Python quiddity's document.
//!
//! It is shaped after Python's `build_recognition_document` features (`family`, `record_type`,
//! `record`, `defining_faces`), flattened to draftwright's bridge `Record` (`faces`,
//! `parameters`). Differences from Python's: hole patterns are records here (Python keeps them
//! under `derived`), with their members' faces; records carry no `dependents` or
//! `named_dimensions` (Python adds them when enriching its document); coordinates are the STEP
//! file's (Python's are in a working frame it reports); faces are the file's own indices
//! (Python's `caller_index`). The input's SHA256 identifies the exact source file bytes when
//! recognised from a file; in-memory recognition has no source digest.
//!
//! Version 2 (the maintainer's decision on review M8): the part is recognised in its own frame
//! and the records reported in the file's coordinates ([`crate::framed_records`]). `frame` says
//! where the records were found: the inferred frame (origin and axes in the file's coordinates,
//! and its gauge), the reason none could be inferred (the part was then recognised as placed in
//! the file, so its records depend on that placement), or `caller-space` for a part recognised in
//! memory without one. In version 6 every record is self-contained in file coordinates:
//! directions that cannot name a file axis use unit vectors, and their scalar coordinates have
//! an explicit record origin. The top-level frame is provenance only. Version 6 unifies
//! oriented families as well; see docs/recognition-6.md.

use std::collections::BTreeMap;

use serde::de::{Deserializer, MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::correspondence::Recognition;
use crate::features::Features;
pub use crate::features::evidence::{EvidenceStatus, RunMode};
use crate::features::gussets::GussetRibPattern;
use crate::features::hole_patterns::HolePattern;
use crate::features::oriented_slots::OrientedSlotPattern;
use crate::features::recess_patterns::{PocketPattern, SlotPattern};
use crate::features::section_recess::SectionRecessPattern;
use crate::frames::RecordFrame;

/// Version 6 unifies the plain and oriented slot, chamfer and slot-pattern namespaces.
pub const SCHEMA_VERSION: &str = "quiddity-rust/recognition/6";
/// Historical pre-thread contract, accepted on input but no longer emitted.
pub const LEGACY_SCHEMA_VERSION: &str = "quiddity-rust/recognition/3";

/// The contract emitted for complete recognition documents (all now carry accounting).
pub fn schema_version(_features: &Features) -> &'static str {
    SCHEMA_VERSION
}

/// Stable positive evidence codes; see docs/face-accounting.md.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BodyReason {
    StockBoxSide,
    StockBoxNotched,
    StockBoxSplit,
    PlateEdge,
    StockWall,
}

/// A named refusal record and its original reason code.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FaceRefusal {
    pub record_id: String,
    pub reason: String,
}

/// Exactly one group per face. Feature claims take precedence over refusals, then body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "group", rename_all = "snake_case")]
#[non_exhaustive]
pub enum FaceAccounting {
    Feature { record_ids: Vec<String> },
    Body { reason: BodyReason },
    Refused { refusals: Vec<FaceRefusal> },
    Unexplained,
}

/// One part's recognition, as draftwright reads it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RecognitionDocument {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failures: Vec<crate::features::FamilyFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complete: Option<bool>,
    pub schema_version: String,
    /// The recogniser: `quiddity-rust` and its crate version.
    pub package: Package,
    /// Coordinates in every record are the STEP file's (`file`).
    pub coordinate_space: String,
    /// Where the records were found: the part's own frame, or why none could be inferred.
    pub frame: RecordFrame,
    /// The part's face count: faces are indexed `0..face_count` in the file's order, as in
    /// the fingerprints.
    pub face_count: usize,
    /// In `Features`' family order, then each family's own.
    pub records: Vec<Record>,
    /// Array index is the file face index. None means an older, unaccounted document;
    /// every newly produced document contains exactly `face_count` entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face_accounting: Option<Vec<FaceAccounting>>,
    /// Lowercase SHA256 of input file bytes (compressed bytes for .gz), or null in memory.
    #[serde(default)]
    pub source_sha256: Option<String>,
    /// Strict inventory or its local-degradation retry; unknown only in older documents.
    #[serde(default)]
    pub run_mode: RunMode,
    /// Kernel input repairs, naming each affected solid; absent in older or unrepaired input.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_repairs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Package {
    pub name: String,
    pub version: String,
}

/// A recognised feature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Record {
    /// `<family>/<index>`, the feature's fingerprint ID.
    pub id: String,
    /// The family's field in [`Features`], its serde key.
    pub family: String,
    /// Python's record class name ([`RECORD_TYPES`]).
    pub record_type: String,
    /// Core defining faces, ascending (a pattern's are its members'). These anchor
    /// the fingerprint; use `claimed_faces` for nested treatment claims as well.
    pub faces: Vec<usize>,
    /// The record as the family serialises it.
    pub parameters: Map<String, Value>,
    /// Strict valid-solid proof on defining and consulted faces, or on every pattern member.
    /// Refusals and records from older documents are unproved.
    #[serde(default)]
    pub evidence_status: EvidenceStatus,
}

impl Record {
    /// A caller-authored record starts unproved; attaching a record does not prove it.
    pub fn new(
        id: String,
        family: String,
        record_type: String,
        faces: Vec<usize>,
        parameters: Map<String, Value>,
    ) -> Self {
        Self {
            id,
            family,
            record_type,
            faces,
            parameters,
            evidence_status: EvidenceStatus::default(),
        }
    }

    /// All claimed faces, including proved hole-mouth treatments. The main `faces`
    /// remain the cylindrical feature's anchor; consulted support planes are not claims.
    pub fn claimed_faces(&self) -> impl Iterator<Item = usize> + '_ {
        let treatments = (self.family == "holes")
            .then(|| {
                self.parameters
                    .get("mouth_chamfers")
                    .and_then(Value::as_array)
            })
            .flatten()
            .into_iter()
            .flatten()
            .filter_map(|c| c.get("faces").and_then(Value::as_array))
            .flatten()
            .filter_map(Value::as_u64)
            .filter_map(|f| usize::try_from(f).ok());
        self.faces.iter().copied().chain(treatments)
    }

    /// A published explanation of a refused projection, rather than a feature claim.
    /// Consumers should inspect its type and reason separately from feature evidence status.
    pub fn is_refusal(&self) -> bool {
        matches!(
            self.record_type.as_str(),
            "SectionRecessRefusal"
                | "ModelledThreadRefusal"
                | "InteriorVoidRefusal"
                | "PadRefusal"
                | "PocketRefusal"
                | "PrismaticPocketRefusal"
                | "SectionPassageRefusal"
                | "OrientedSlotRefusal"
                | "ReconciliationRefusal"
                | "PolygonalBossRefusal"
        )
    }
}

/// Python's record class names for each family's records, in [`Features`]' order: one, or one
/// per variant of an untagged family ([`record_type`] picks it).
pub const RECORD_TYPES: &[(&str, &[&str])] = &[
    ("helical_springs", &["HelicalSpring"]),
    ("fillets", &["Fillet"]),
    ("cylindrical_profiles", &["CylindricalProfile"]),
    ("chamfers", &["Chamfer"]),
    ("internal_bevels", &["InternalBevel"]),
    ("bosses", &["BossRecord"]),
    ("angled_steps", &["AngledStep"]),
    ("flats", &["Flat"]),
    ("paired_ramp_steps", &["PairedRampStep"]),
    ("circular_face_patterns", &["CircularFacePattern"]),
    ("oblique_through_steps", &["ObliqueThroughStep"]),
    (
        "polygonal_oblique_through_steps",
        &["PolygonalObliqueThroughStep"],
    ),
    ("circular_blind_steps", &["CircularBlindStep"]),
    ("holes", &["HoleRecord"]),
    ("countersinks", &["CounterSink"]),
    (
        "hole_patterns",
        &[
            "RectGrid",
            "RectangularHoleSet",
            "BoltCircle",
            "LinearArray",
        ],
    ),
    ("gusset_ribs", &["GussetRib"]),
    (
        "gusset_rib_patterns",
        &["GussetRibArray", "GussetRibMirrorPair"],
    ),
    ("thin_wall_bodies", &["ThinWallBody"]),
    ("interior_voids", &["InteriorVoid", "InteriorVoidRefusal"]),
    ("through_steps", &["ThroughStep"]),
    ("local_steps", &["LocalStep"]),
    ("turned_steps", &["TurnedStep"]),
    ("grooves", &["Groove"]),
    ("annular_grooves", &["AnnularGroove"]),
    ("face_grooves", &["FaceGroove"]),
    ("plates", &["Plate"]),
    ("turned_bodies", &["TurnedBody"]),
    ("round_bottom_blind_slots", &["RoundBottomBlindSlot"]),
    ("semicircular_blind_slots", &["SemicircularBlindSlot"]),
    ("rectangular_blind_slots", &["RectangularBlindSlot"]),
    ("double_d_bores", &["DoubleDBore"]),
    ("edge_open_circular_pockets", &["EdgeOpenCircularPocket"]),
    ("edge_open_prismatic_recesses", &["EdgeOpenPrismaticRecess"]),
    ("blends", &["Blend"]),
    ("sheet_metal_bodies", &["SheetMetalBody"]),
    ("slots", &["Slot"]),
    ("pockets", &["Pocket"]),
    ("channels", &["Channel"]),
    ("v_channels", &["VChannel"]),
    ("rectangular_through_notches", &["RectangularThroughNotch"]),
    ("slot_patterns", &["SlotGrid", "SlotArray"]),
    ("pocket_patterns", &["PocketGrid", "PocketArray"]),
    ("repeating_radial_profiles", &["RepeatingRadialProfile"]),
    ("freeform_surfaces", &["FreeformSurface"]),
    ("modelled_threads", &["ModelledThread"]),
    ("polygonal_bosses", &["PolygonalBoss"]),
    ("polygonal_stock", &["PolygonalStock"]),
    ("section_passages", &["SectionPassage"]),
    ("prismatic_through_cuts", &["PrismaticThroughCut"]),
    ("prismatic_pockets", &["PrismaticPocket"]),
    ("pads", &["RaisedPad"]),
    ("step_levels", &["FaceLevel"]),
    ("risers", &["RiserEvidence"]),
    (
        "section_recesses",
        &["SectionRecess", "SectionRecessRefusal"],
    ),
    (
        "section_recess_patterns",
        &["SectionRecessArray", "SectionRecessGrid"],
    ),
    ("modelled_thread_refusals", &["ModelledThreadRefusal"]),
];

/// Optional refusal families carried in `Features::discovery_refusals`, rather than
/// ordinary feature fields. They have record types but no feature fingerprint tables.
const DISCOVERY_REFUSAL_TYPES: &[(&str, &[&str])] = &[
    ("pad_refusals", &["PadRefusal"]),
    ("pocket_refusals", &["PocketRefusal"]),
    ("prismatic_pocket_refusals", &["PrismaticPocketRefusal"]),
    ("section_passage_refusals", &["SectionPassageRefusal"]),
    ("oriented_slot_refusals", &["OrientedSlotRefusal"]),
    ("reconciliation_refusals", &["ReconciliationRefusal"]),
    ("polygonal_boss_refusals", &["PolygonalBossRefusal"]),
];

/// The record class names a family's records can have.
pub fn record_types(family: &str) -> Option<&'static [&'static str]> {
    RECORD_TYPES
        .iter()
        .chain(DISCOVERY_REFUSAL_TYPES)
        .find(|(f, _)| *f == family)
        .map(|(_, types)| *types)
}

/// Python's record class name for record *n* of *family*.
pub fn record_type(features: &Features, family: &str, n: usize) -> &'static str {
    match family {
        "oriented_slots" => "Slot",
        "oriented_chamfers" => "Chamfer",
        "section_recess_patterns" => match &features.section_recess_patterns[n] {
            SectionRecessPattern::Array(_) => "SectionRecessArray",
            SectionRecessPattern::Grid(_) => "SectionRecessGrid",
        },
        "interior_voids" => {
            if n < features.interior_voids.len() {
                "InteriorVoid"
            } else {
                "InteriorVoidRefusal"
            }
        }
        "section_recesses" => {
            if n < features.section_recesses.len() {
                "SectionRecess"
            } else {
                "SectionRecessRefusal"
            }
        }
        "hole_patterns" => match &features.hole_patterns[n] {
            HolePattern::RectGrid { .. } => "RectGrid",
            HolePattern::RectangularHoleSet { .. } => "RectangularHoleSet",
            HolePattern::BoltCircle { .. } => "BoltCircle",
            HolePattern::LinearArray { .. } => "LinearArray",
        },
        "gusset_rib_patterns" => match &features.gusset_rib_patterns[n] {
            GussetRibPattern::Array { .. } => "GussetRibArray",
            GussetRibPattern::MirrorPair { .. } => "GussetRibMirrorPair",
        },
        "slot_patterns" => match &features.slot_patterns[n] {
            SlotPattern::SlotGrid { .. } => "SlotGrid",
            SlotPattern::SlotArray { .. } => "SlotArray",
        },
        "pocket_patterns" => match &features.pocket_patterns[n] {
            PocketPattern::PocketGrid { .. } => "PocketGrid",
            PocketPattern::PocketArray { .. } => "PocketArray",
        },
        "oriented_slot_patterns" => match &features.oriented_slot_patterns[n] {
            OrientedSlotPattern::OrientedSlotGrid { .. } => "SlotGrid",
            OrientedSlotPattern::OrientedSlotArray { .. } => "SlotArray",
        },
        _ => match record_types(family) {
            Some([one]) => one,
            _ => panic!(
                "family {family} has no single record type: add (\"{family}\", \
                 &[\"<Python record class>\"]) to RECORD_TYPES in src/recognition.rs"
            ),
        },
    }
}

/// Each family's document records as JSON, in [`Features`]' field order, with section-recess
/// source refusals appended after occurrences (a `serde_json::Value` object
/// would sort the families by name).
pub fn families(
    features: &crate::published_records::PublishedFeatures,
) -> Vec<(String, Vec<Value>)> {
    features.families()
}

/// Working recogniser records before file-coordinate conversion and namespace merging.
pub(crate) fn working_families(features: &Features) -> Vec<(String, Vec<Value>)> {
    struct Ordered(Vec<(String, Vec<Value>)>);
    impl<'de> Deserialize<'de> for Ordered {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            struct V;
            impl<'de> Visitor<'de> for V {
                type Value = Ordered;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str("families of records")
                }
                fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Ordered, A::Error> {
                    let mut out = Vec::new();
                    while let Some(entry) = map.next_entry()? {
                        out.push(entry);
                    }
                    Ok(Ordered(out))
                }
            }
            d.deserialize_map(V)
        }
    }
    let text = serde_json::to_string(features).expect("records serialise");
    let mut families = serde_json::from_str::<Ordered>(&text)
        .expect("every family is a list of records")
        .0;
    families
        .iter_mut()
        .find(|(f, _)| f == "interior_voids")
        .unwrap()
        .1
        .extend(
            features
                .interior_void_refusals
                .iter()
                .map(|r| serde_json::to_value(r).expect("void refusal serialises")),
        );
    let (_, recesses) = families
        .iter_mut()
        .find(|(f, _)| f == "section_recesses")
        .unwrap();
    recesses.extend(
        features
            .section_recess_refusals
            .iter()
            .map(|r| serde_json::to_value(r).expect("refusal serialises")),
    );
    families.push((
        "section_recess_patterns".into(),
        features
            .section_recess_patterns
            .iter()
            .map(|r| serde_json::to_value(r).expect("pattern serialises"))
            .collect(),
    ));
    families.push((
        "modelled_thread_refusals".into(),
        features
            .modelled_thread_refusals
            .iter()
            .map(|r| serde_json::to_value(r).expect("thread refusal serialises"))
            .collect(),
    ));
    families.extend(
        features
            .discovery_refusals
            .iter()
            .filter(|(_, r)| !r.is_empty())
            .map(|(&family, refusals)| {
                (
                    family.to_owned(),
                    refusals
                        .iter()
                        .map(|r| serde_json::to_value(r).expect("discovery refusal serialises"))
                        .collect(),
                )
            }),
    );
    families
}

/// The document of *recognition*, a part of *face_count* faces: each record with its
/// fingerprint's faces (or source evidence's defining faces for a refusal).
pub fn document(recognition: &Recognition, face_count: usize) -> RecognitionDocument {
    let working = recognition.features.working();
    let faces: BTreeMap<&str, &Vec<usize>> = recognition
        .fingerprints
        .features
        .iter()
        .map(|f| (f.id.as_str(), &f.faces))
        .collect();
    let mut records = Vec::new();
    let families = recognition.features.families();
    for (family, list) in &families {
        for (n, record) in list.iter().enumerate() {
            let id = format!("{family}/{n}");
            let Value::Object(parameters) = record else {
                panic!("{id} is not a record");
            };
            // Refusals describe source evidence, not recognised geometry to fingerprint.
            let faces = if let Some(refusals) = working.discovery_refusals.get(family.as_str()) {
                refusals[n].faces.clone()
            } else if family == "interior_voids" && n >= working.interior_voids.len() {
                working.interior_void_refusals[n - working.interior_voids.len()]
                    .candidate
                    .void_faces
                    .clone()
            } else if family == "modelled_thread_refusals" {
                working.modelled_thread_refusals[n].faces.clone()
            } else if family == "section_recess_patterns" {
                let mut faces: Vec<_> = working.section_recess_patterns[n]
                    .members()
                    .iter()
                    .flat_map(|&m| {
                        working.section_recesses[m]
                            .evidence()
                            .defining_faces()
                            .iter()
                            .copied()
                    })
                    .collect();
                faces.sort_unstable();
                faces.dedup();
                faces
            } else if family == "section_recesses" && n >= working.section_recesses.len() {
                working.section_recess_refusals[n - working.section_recesses.len()]
                    .evidence()
                    .defining_faces()
                    .to_vec()
            } else {
                faces
                    .get(id.as_str())
                    .unwrap_or_else(|| panic!("{id} has no fingerprint"))
                    .to_vec()
            };
            let (working_family, working_n) = recognition.features.working_record(family, n);
            records.push(Record {
                record_type: record_type(working, working_family, working_n).into(),
                faces,
                id,
                family: family.clone(),
                evidence_status: record_status(
                    &recognition.features,
                    family,
                    n,
                    parameters,
                    &families,
                ),
                parameters: parameters.clone(),
            });
        }
    }
    let face_accounting = account_faces(&records, &recognition.body_evidence, face_count);
    RecognitionDocument {
        failures: working.failures.clone(),
        complete: (!working.failures.is_empty()).then_some(false),
        schema_version: schema_version(working).into(),
        package: Package {
            name: "quiddity-rust".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        },
        coordinate_space: "file".into(),
        frame: recognition.frame.clone(),
        face_count,
        face_accounting: Some(face_accounting),
        records,
        source_sha256: recognition.source_sha256.clone(),
        run_mode: working.run_mode,
        input_repairs: recognition.input_repairs.clone(),
    }
}

/// Derived records inherit every member's proof, including consulted faces absent from the
/// fingerprint. Embedded members are resolved in the same order as fingerprint::derived_faces.
fn record_status(
    features: &crate::published_records::PublishedFeatures,
    family: &str,
    n: usize,
    parameters: &Map<String, Value>,
    families: &[(String, Vec<Value>)],
) -> EvidenceStatus {
    let status = |family: &str, n: usize| {
        let (family, n) = features.working_record(family, n);
        features
            .working()
            .evidence_status
            .get(family)
            .and_then(|s| s.get(n))
            .copied()
            .unwrap_or_default()
    };
    if family == "section_recess_patterns" {
        let members = features.working().section_recess_patterns[n].members();
        return if !members.is_empty()
            && members
                .iter()
                .all(|&m| status("section_recesses", m) == EvidenceStatus::Verified)
        {
            EvidenceStatus::Verified
        } else {
            EvidenceStatus::Unproved
        };
    }
    if let Some((field, base)) =
        crate::correspondence::fingerprint::family(family).and_then(|f| f.members)
    {
        let Some(members) = parameters.get(field).and_then(Value::as_array) else {
            return EvidenceStatus::Unproved;
        };
        let Some((_, pool)) = families.iter().find(|(f, _)| f == base) else {
            return EvidenceStatus::Unproved;
        };
        let mut taken = vec![false; pool.len()];
        return if !members.is_empty()
            && members.iter().all(|member| {
                let Some(k) = (0..pool.len()).find(|&k| {
                    !taken[k]
                        && crate::correspondence::fingerprint::same_member(base, &pool[k], member)
                }) else {
                    return false;
                };
                taken[k] = true;
                status(base, k) == EvidenceStatus::Verified
            }) {
            EvidenceStatus::Verified
        } else {
            EvidenceStatus::Unproved
        };
    }
    status(family, n)
}

/// Partition core defining faces and proved nested treatment claims.
fn account_faces(
    records: &[Record],
    body_evidence: &BTreeMap<usize, BodyReason>,
    face_count: usize,
) -> Vec<FaceAccounting> {
    let mut by_face: Vec<Vec<&Record>> = vec![Vec::new(); face_count];
    for record in records {
        let mut claimed: Vec<_> = record.claimed_faces().collect();
        claimed.sort_unstable();
        claimed.dedup();
        for face in claimed {
            if let Some(owners) = by_face.get_mut(face) {
                owners.push(record);
            }
        }
    }
    by_face
        .into_iter()
        .enumerate()
        .map(|(face, owners)| {
            let record_ids: Vec<_> = owners
                .iter()
                .filter(|r| !r.is_refusal())
                .map(|r| r.id.clone())
                .collect();
            if !record_ids.is_empty() {
                return FaceAccounting::Feature { record_ids };
            }
            let refusals: Vec<_> = owners
                .iter()
                .filter(|r| r.is_refusal())
                .map(|r| FaceRefusal {
                    record_id: r.id.clone(),
                    reason: r.parameters["reason"]
                        .as_str()
                        .expect("named refusal has a reason")
                        .into(),
                })
                .collect();
            if !refusals.is_empty() {
                return FaceAccounting::Refused { refusals };
            }
            body_evidence
                .get(&face)
                .map_or(FaceAccounting::Unexplained, |&reason| {
                    FaceAccounting::Body { reason }
                })
        })
        .collect()
}

#[cfg(test)]
mod accounting_tests {
    use super::*;

    #[test]
    fn nested_mouth_claims_preserve_core_anchors_and_exclude_support_planes() {
        let record=Record {
            id:"holes/0".into(),family:"holes".into(),record_type:"HoleRecord".into(),
            faces:vec![3,10],evidence_status:EvidenceStatus::Verified,
            parameters:serde_json::from_value(serde_json::json!({
                "mouth_chamfers":[{"faces":[1,1],"location":[0,0,2],"size":0.25,"depth":0.25,"angle":45}],
                "exit_obstructions":[{"faces":[5]}]
            })).unwrap(),
        };
        assert_eq!(record.faces.iter().min(), Some(&3));
        assert!(record.claimed_faces().any(|f| f == 1));
        assert!(!record.claimed_faces().any(|f| f == 5));
        let groups = account_faces(&[record], &BTreeMap::new(), 11);
        assert_eq!(
            groups[1],
            FaceAccounting::Feature {
                record_ids: vec!["holes/0".into()]
            }
        );
        assert_eq!(groups[5], FaceAccounting::Unexplained);
        assert_eq!(groups[3], groups[1]);
        assert_eq!(groups[10], groups[1]);
    }

    #[test]
    fn overlapping_claims_refusals_and_body_have_explicit_precedence() {
        let record = |id: &str, refusal: bool, faces: Vec<usize>| Record {
            id: id.into(),
            family: "test".into(),
            record_type: if refusal {
                "SectionRecessRefusal"
            } else {
                "HoleRecord"
            }
            .into(),
            faces,
            parameters: serde_json::from_value(
                serde_json::json!({"reason":"unsupported_support_geometry"}),
            )
            .unwrap(),
            evidence_status: EvidenceStatus::Unproved,
        };
        let records = vec![
            record("feature/0", false, vec![0]),
            record("feature/1", false, vec![0]),
            record("refusal/0", true, vec![0, 1]),
            record("refusal/1", true, vec![1]),
        ];
        let body = (0..3).map(|f| (f, BodyReason::StockBoxSide)).collect();
        let groups = account_faces(&records, &body, 4);
        assert_eq!(
            groups[0],
            FaceAccounting::Feature {
                record_ids: vec!["feature/0".into(), "feature/1".into()]
            }
        );
        let FaceAccounting::Refused { refusals } = &groups[1] else {
            panic!("refusal lost")
        };
        assert_eq!(
            refusals
                .iter()
                .map(|r| r.record_id.as_str())
                .collect::<Vec<_>>(),
            ["refusal/0", "refusal/1"]
        );
        assert!(
            refusals
                .iter()
                .all(|r| r.reason == "unsupported_support_geometry")
        );
        assert_eq!(
            groups[2],
            FaceAccounting::Body {
                reason: BodyReason::StockBoxSide
            }
        );
        assert_eq!(groups[3], FaceAccounting::Unexplained);
        assert!(account_faces(&[], &BTreeMap::new(), 0).is_empty());
    }
}
