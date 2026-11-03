//! Semantic PMI: the model ([`model`]), ISO general tolerance tables ([`standards`]), the
//! AP242 reader ([`read()`]) and writer ([`write()`], [`mod@write`]), and the read-back check of
//! a write ([`verify()`]). The design is `docs/step-ap242.md`.
//!
//! [`read()`] maps a Part 21 document's semantic PMI to one [`PartPmi`] per distinct part of
//! the file, with [`Finding`]s for everything it did not read (or read through a
//! nonconformance) and the [`Provenance`] of every model item: the instance ids it consumed,
//! which a replace removes.
//!
//! With the default `presentation` feature, the writer uses `crate::presentation` for
//! dimension/tolerance content, datum lettering, face texture symbols and written part notes.
//! Styled tessellated geometric sets, annotation planes and semantic associations share one part
//! draughting model (CAx-IF PMI §7.3). See `docs/step-ap242.md`, Graphical callouts (#137).
//! Append preserves foreign callouts and symbol representations.

mod derived;
pub mod model;
mod read;
pub(crate) mod sites;
pub mod standards;
pub(crate) mod text;
mod unit_identity;
pub mod verify;
mod without;
pub mod write;

pub use model::*;
pub use read::read;
#[cfg(feature = "json-import")]
pub use read::read_native;
pub use verify::{Problem, Snapshot, Verification, verify};
pub use without::without;
pub(crate) use without::{Kept, without_kept};
pub use write::{Mode, write};

/// The result of reading a file's PMI.
#[derive(Clone, Debug)]
pub struct PmiRead {
    /// Own entity of each independently identified item, used by JSON presentation links.
    #[cfg(feature = "json")]
    pub(crate) json_carriers: std::collections::BTreeMap<(PartId, String, usize), u64>,
    /// One per part of `read_part_definitions`, in its order (`PartId` is the index).
    pub parts: Vec<PartPmi>,
    /// What was not read, or was read through a nonconformance, in a deterministic order.
    pub findings: Vec<Finding>,
    pub provenance: Provenance,
    pub identifiers: Identifiers,
    pub accounting: Accounting,
    /// Presented annotations in STEP instance order, including graphic-only callouts.
    pub callouts: Vec<GraphicalCallout>,
}

impl PartialEq for PmiRead {
    fn eq(&self, other: &Self) -> bool {
        self.parts == other.parts
            && self.findings == other.findings
            && self.provenance == other.provenance
            && self.identifiers == other.identifiers
            && self.accounting == other.accounting
            && self.callouts == other.callouts
    }
}
impl Eq for PmiRead {}

/// One presented annotation: a draughting callout, or an occurrence outside all callouts.
/// Annotation planes are containers; their elements are listed instead. Read-only evidence,
/// separate from the semantic model and its replace/consumption policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphicalCallout {
    /// The annotation's STEP instance id (without the `#`, like `Finding.ids`).
    pub id: u64,
    /// Its decoded representation-item name.
    pub name: String,
    /// Every DMIA on it or its contained annotations, ordered by association instance.
    /// Empty means graphic-only. An unresolved association is never graphic-only.
    pub links: Vec<CalloutLink>,
}

/// A `draughting_model_item_association` (including subtypes) and its semantic definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalloutLink {
    pub association: u64,
    /// The definition's STEP instance, retained even when it is not modelled.
    pub definition: u64,
    /// Model items named by the definition. Empty means unresolved; `entity` explains what
    /// was associated instead. Plain shape-aspect definitions resolve to their features only.
    pub targets: Vec<CalloutTarget>,
    /// The definition's entity type, lower case (complex leaves joined by `+`).
    pub entity: String,
}

/// A part-qualified reference into the returned semantic model.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CalloutTarget {
    pub part: PartId,
    pub item: PresentedItem,
}

/// The semantic item a graphical annotation presents. A datum feature names its `Feature`
/// and any `Datum` it establishes. Plain shape aspects name features only.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PresentedItem {
    Feature(FeatureId),
    Dimension(DimensionId),
    Tolerance(ToleranceId),
    Datum(DatumId),
    DatumTarget(DatumTargetId),
    Note(usize),
    Attribute(usize),
    SurfaceTexture(usize),
    General(usize),
    Thread(usize),
    Knurl(usize),
    Material,
}

#[cfg(feature = "json-import")]
impl PresentedItem {
    pub(crate) fn collection_index(&self) -> (&'static str, usize) {
        match *self {
            Self::Feature(id) => ("features", id.0),
            Self::Dimension(id) => ("dimensions", id.0),
            Self::Tolerance(id) => ("tolerances", id.0),
            Self::Datum(id) => ("datums", id.0),
            Self::DatumTarget(id) => ("datum_targets", id.0),
            Self::Note(i) => ("notes", i),
            Self::Attribute(i) => ("attributes", i),
            Self::SurfaceTexture(i) => ("surface_textures", i),
            Self::General(i) => ("general", i),
            Self::Thread(i) => ("threads", i),
            Self::Knurl(i) => ("knurls", i),
            Self::Material => ("material", 0),
        }
    }

    pub(crate) fn at_index(&self, i: usize) -> Self {
        match self {
            Self::Feature(_) => Self::Feature(FeatureId(i)),
            Self::Dimension(_) => Self::Dimension(DimensionId(i)),
            Self::Tolerance(_) => Self::Tolerance(ToleranceId(i)),
            Self::Datum(_) => Self::Datum(DatumId(i)),
            Self::DatumTarget(_) => Self::DatumTarget(DatumTargetId(i)),
            Self::Note(_) => Self::Note(i),
            Self::Attribute(_) => Self::Attribute(i),
            Self::SurfaceTexture(_) => Self::SurfaceTexture(i),
            Self::General(_) => Self::General(i),
            Self::Thread(_) => Self::Thread(i),
            Self::Knurl(_) => Self::Knurl(i),
            Self::Material => Self::Material,
        }
    }
}

/// Why a file's PMI could not be read at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The part definitions do not belong to this document (an id is not an instance of it).
    Parts(String),
    /// JSON transport metadata disagrees with the authoritative native STEP.
    #[cfg(feature = "json")]
    Transport(String),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Parts(s) => write!(f, "part definitions do not match the document: {s}"),
            #[cfg(feature = "json")]
            ReadError::Transport(s) => write!(f, "invalid PMI JSON transport metadata: {s}"),
        }
    }
}

impl std::error::Error for ReadError {}

/// A factual observation, something not read, or a stated interpretation.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Finding {
    pub kind: FindingKind,
    /// The part it concerns, when known.
    pub part: Option<PartId>,
    /// The subject instance first, then the others concerned.
    pub ids: Vec<u64>,
    /// The subject's entity type, lower case (a complex instance's leaves joined by `+`).
    pub entity: String,
    pub detail: String,
}

impl Finding {
    /// Factual observations survive replacement under fresh entity ids. Match their
    /// content and multiplicity, rather than their document-local provenance.
    #[must_use]
    pub fn preserved_fact(&self, before: &[Finding], after: &[Finding]) -> bool {
        if self.kind != FindingKind::Fact {
            return false;
        }
        let same = |f: &&Finding| {
            f.kind == self.kind
                && f.part == self.part
                && f.entity == self.entity
                && f.detail == self.detail
        };
        before.iter().filter(same).count() == after.iter().filter(same).count()
    }
}

/// The kinds of finding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FindingKind {
    /// A factual observation about a construct that is read and preserved.
    Fact,
    /// A construct the reader does not read (a form the practice does not define, or one the
    /// model does not hold yet); not consumed.
    Unsupported,
    /// A construct that violates the schema or the practice, read through as the detail says
    /// (consumed), or not read when it cannot be (not consumed).
    Nonconformance,
    /// A reference that does not resolve: a face or edge that is not the part's, an instance
    /// missing or of the wrong type; the item is not read. A retained datum feature
    /// resolving to no faces is also reported; the detail states which case applies.
    Unresolved,
    /// A measure whose unit cannot be resolved; its value is not used.
    Unitless,
    /// PMI on an assembly's product definition: out of scope, not attributed to any part.
    AssemblyPmi,
    /// PMI on an occurrence (`assembly_component_usage`, `specified_higher_usage_occurrence`
    /// paths): out of scope, not attributed to any part.
    OccurrencePmi,
    /// A construct whose practice was not available (material practice not obtained).
    UndeterminedPractice,
    /// Information the model does not hold, consumed with the item it belongs to (e.g.
    /// explicit geometry of a derived feature, CAD-system-specific aspects); the detail says
    /// what is dropped.
    NotModelled,
    /// Two statements of one thing that disagree; neither is guessed between.
    Conflict,
    /// A semantic-PMI instance reached by no part's PMI.
    Unconsumed,
}

impl FindingKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            FindingKind::Fact => "fact",
            FindingKind::Unsupported => "unsupported",
            FindingKind::Nonconformance => "nonconformance",
            FindingKind::Unresolved => "unresolved-reference",
            FindingKind::Unitless => "unitless-measure",
            FindingKind::AssemblyPmi => "assembly-pmi",
            FindingKind::OccurrencePmi => "occurrence-pmi",
            FindingKind::UndeterminedPractice => "undetermined-practice",
            FindingKind::NotModelled => "not-modelled",
            FindingKind::Conflict => "conflict",
            FindingKind::Unconsumed => "unconsumed",
        }
    }
}

/// Per part, per model item, the instance ids the reader consumed for it (ascending). Shared
/// instances (a datum system two tolerances use, a qualifier several values use) are listed
/// under each item that consumed them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Provenance {
    pub parts: Vec<PartProvenance>,
}

/// The ids behind each item of one [`PartPmi`], index for index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PartProvenance {
    #[cfg(feature = "json")]
    pub(crate) transport: Vec<u64>,
    pub standards: Vec<Vec<u64>>,
    pub decimal_places: Vec<u64>,
    pub features: Vec<Vec<u64>>,
    pub datum_targets: Vec<Vec<u64>>,
    pub datums: Vec<Vec<u64>>,
    pub dimensions: Vec<Vec<u64>>,
    pub tolerances: Vec<Vec<u64>>,
    pub tolerance_relations: Vec<Vec<u64>>,
    pub general: Vec<Vec<u64>>,
    pub threads: Vec<Vec<u64>>,
    pub knurls: Vec<Vec<u64>>,
    pub material: Vec<u64>,
    pub notes: Vec<Vec<u64>>,
    /// Presentation roots for each note; read only, excluded from consumed semantic ids.
    pub note_callouts: Vec<Vec<u64>>,
    pub surface_textures: Vec<Vec<u64>>,
    pub attributes: Vec<Vec<u64>>,
    /// Per supplemental geometry: every instance of its item tree.
    pub geometry: Vec<Vec<u64>>,
    /// Per supplemental geometry: its item's own instance.
    pub geometry_items: Vec<u64>,
}

/// The decoded `id_attribute.attribute_value` on each item's own entity, never inherited
/// from its support. Absent or ambiguous identifiers are `None`; duplicates are findings.
/// Kept separate from the semantic model, like [`Provenance`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Identifiers {
    pub parts: Vec<PartIdentifiers>,
}

/// Identifiers for one [`PartPmi`], index for index. Threads and knurls use their definition
/// `product_definition_shape`; property items use their own `property_definition`
/// (general tolerance tables use their representation).
/// Settings merged from several entities (material, standards, decimal places) have no
/// single requirement entity and are not indexed here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PartIdentifiers {
    /// Optional source identifier observations retained by the JSON transport, separate
    /// from its unique wire aliases. An empty map means the producer supplied none.
    #[cfg(feature = "json")]
    pub json_identifiers: Option<String>,
    /// JSON transport unit ids/order; default-feature builds have no JSON metadata.
    #[cfg(feature = "json")]
    pub json_units: Option<String>,
    /// Verified JSON wire topology forms; absent in default-feature builds.
    #[cfg(feature = "json")]
    pub json_anchors: Option<String>,
    #[cfg(feature = "json")]
    pub geometry: Vec<Option<String>>,
    #[cfg(feature = "json")]
    pub derived_geometry: Vec<Option<String>>,
    pub features: Vec<Option<String>>,
    pub datum_targets: Vec<Option<String>>,
    pub datums: Vec<Option<String>>,
    pub dimensions: Vec<Option<String>>,
    pub tolerances: Vec<Option<String>>,
    pub tolerance_relations: Vec<Option<String>>,
    pub general: Vec<Option<String>>,
    pub threads: Vec<Option<String>>,
    pub knurls: Vec<Option<String>>,
    pub notes: Vec<Option<String>>,
    pub surface_textures: Vec<Option<String>>,
    pub attributes: Vec<Option<String>>,
}

impl PartProvenance {
    /// Every id consumed for the part, ascending, once.
    #[must_use]
    pub fn all(&self) -> Vec<u64> {
        let mut out: Vec<u64> = self
            .decimal_places
            .iter()
            .chain(&self.material)
            .copied()
            .chain(
                [
                    &self.standards,
                    &self.features,
                    &self.datum_targets,
                    &self.datums,
                    &self.dimensions,
                    &self.tolerances,
                    &self.tolerance_relations,
                    &self.general,
                    &self.threads,
                    &self.knurls,
                    &self.notes,
                    &self.surface_textures,
                    &self.attributes,
                    &self.geometry,
                ]
                .into_iter()
                .flatten()
                .flatten()
                .copied(),
            )
            .collect();
        #[cfg(feature = "json")]
        out.extend(&self.transport);
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// How the file's instances were accounted for: every semantic-PMI instance is consumed by a
/// part or named by a finding.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Accounting {
    /// Instances of the semantic-PMI family (`express::Family::SemanticPmi`).
    pub semantic: usize,
    /// Of them, consumed by some part.
    pub consumed: usize,
    /// Of them, not consumed but named by a finding.
    pub reported: usize,
    /// Graphic PMI instances (including the annotations read in `PmiRead.callouts`): occurrences (including
    /// planes and tessellated occurrences), draughting callouts, and their models/views.
    /// Geometry styling, layers, colours and invisibility are excluded.
    pub presentation: usize,
    /// Validation property definitions (counted, not interpreted).
    pub validation: usize,
}
