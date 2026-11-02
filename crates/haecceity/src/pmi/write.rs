//! The AP242 semantic PMI writer (design: Architecture items 5 and 6, the Mapping table, the
//! Editions rule and the Anti-requirements table of `docs/step-ap242.md`).
//!
//! [`write`] maps the [`PartPmi`] of any number of parts of one file to one [`Edit`] of its
//! [`Document`]: [`Mode::Add`] writes the items beside what the parts already carry,
//! [`Mode::Replace`] removes what the reader consumed for each part (the removal plan of
//! `removal.rs`, seeded with the part's [`Provenance`](super::Provenance)) and writes the new
//! items, so that afterwards `read(part) == new`; [`Mode::Remove`] is a replace with nothing.
//! Every untouched byte of the file is kept ([`Document::apply`]).
//! On an AP214 upgrade gaining PMI, the closed identical-attribute subtype table in
//! `upgrade.rs` changes only entity keywords, recorded in [`WriteReport::edition_rewrites`].
//! Currently `PRODUCT_TYPE` becomes `PRODUCT_RELATED_PRODUCT_CATEGORY`, keeping its id,
//! name, description, products and all other record bytes. Other undeclared types block upgrades.
//!
//! **Typed emission.** Records are made only through the emitter below: measures only through
//! [`Emitter::length_measure`] and [`Emitter::angle_measure`] (and the measure representation
//! items built on them), which take a [`LengthUnitRef`] or [`AngleUnitRef`] found per value —
//! a unit instance of the document equal to the value's own unit, or one added — so a value
//! is written in its own unit with its own digits and nothing is converted; quantities that
//! require REAL refuse non-REAL text, never rewriting it. COUNT_MEASURE is NUMBER, so canonical
//! integer counts are written as INTEGER tokens. There is no function taking an untyped
//! number. Complex instances are made only through [`p21::Complex`] (leaves sorted), and only
//! when the set of types is not one entity's supertype closure (the internal mapping is used
//! otherwise). `tolerance_value` is built only from deviations ([`Bounds`]: signed offsets,
//! `upper > lower`), `limits_and_fits` only from an [`Iso286Class`] (two enums). Attributes
//! are placed by name through the schema table, so an instance cannot have too few or too many.
//!
//! **Practice/schema conflicts** (maintainer decision 2026-10-20): §6.6 target shapes are
//! written in their descriptions, and Figure 3 decimal-only cells are written without
//! invented ranges/tolerances. Only `datum_target.WR5` on targets this writer adds and
//! `default_tolerance_table_cell.WR3/WR4` on its decimal-only cells may be exempted. Actual
//! violations (rule and final instance id) are in [`WriteReport::practice_schema_conflicts`].
//! `express_rules` still reports every violation; no kept instance or other rule is exempt.
//!
//! **What is refused** (all refusals, [`WriteError::Refused`], naming the item): targets with
//! no establishing datum, nonpositive sizes, placement units unlike the part context, and
//! movable point/circle/rectangle/circular-curve parameter sets rejected by the non-exempt
//! `placed_datum_target_feature.WR3`; other invalid tolerance tables or inconsistent decimal
//! defaults; hole definitions; groups of unstated kind; dimensions without a nominal;
//! projected zones without an origin; unspecified/unsupported affected planes; invalid
//! thread/knurl parameter sets; density units without scale or dimensions M L^-3; and
//! values that are not Part 21 REALs.
//!
//! Tolerance relations (§6.9.9), tables (§4.1), density (material practice §4.2), relationship
//! targets (edition 4 SELECT, shared feature endpoints) and explicitly typed affected planes
//! (§6.9.2.1 Figures 53–54) use their standard entities. Legacy notes migrate via
//! [`standard_form`]: complete roughness requirements with stated micrometre units use ISO
//! 10303-1110; unitless wording stays semantic text; other wording uses
//! semantic text (§7.4), with every choice in [`WriteReport::note_migrations`]. Several notes
//! remain separate sets in input order; §7.4.4 offers no standardized ordering relationship.
//!
//! **Datum feature symbols** (decision 6): each datum feature of an added datum gets a minimal
//! symbol, derived from the model when written (`PartPlan::datum_symbols`): a tessellated
//! callout in an annotation plane of the part's draughting model, linked to the datum feature by
//! a `draughting_model_item_association`; replace and remove take it with its datum. The model is
//! not related to the part's shape representation (maintainer decision, 2026-10-09).
//!
//! **Usages** (maintainer decision, 2026-10-09): one `geometric_item_specific_usage` per face;
//! dimensions and tolerances reuse the feature's aspect directly (§5.1 / §6.5), including
//! datum features. A new datum on a kept carrier has independent per-face GISUs in
//! fresh representations (IIRU UR1/UR2), under the reported §5.1 preservation exception;
//! no kept carrier is promoted. A feature of several faces is composed of one member
//! shape aspect per face (`PartPlan::usages`). Part notes in words are 'semantic text' attribute sets (PMI practice
//! §7.4); surface texture is ISO 10303-1110's form (`PartPlan::surface_texture`).
//!
//! **Validation before returning.** The edit is applied; every added or replaced instance must
//! pass `express` validation against the table of the file's edition (the target edition for
//! an AP242 edition without a table, and for an AP214/AP203 file that gains PMI, whose every
//! instance must then pass, after only the closed R1 approval assignment and R2 conversion
//! unit dimensions repairs in `upgrade.rs`, each recorded in [`WriteReport::repairs`], plus
//! the keyword table above); and `express_rules::check_all` of the result (the WHERE, UNIQUE
//! and global rules it evaluates, `restrict_representation_for_surface_condition` among them)
//! must have no violation the original lacked except the exact registered conflicts above.
//! Violations the original already had are
//! reported. The plan's own refusals that cite a rule (thread WR1 and WR12, turned_knurl WR2)
//! are checks of the model before anything is emitted, naming every offending item at once
//! (`Refusal`); `express_rules` stays the check of what was written.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::model::*;
use super::standards::Iso2768;
use super::{Finding, FindingKind, PmiRead, ReadError};
use crate::express::{self, Edition, Violation};
use crate::express_rules::{self, RuleViolation};
use crate::p21::{self, Attribute, Complex, Document, Edit, NewId, P21Error, RawEntity, Record};
use crate::removal::{self, RemovalRefusal};
use crate::step::PartDefinition;

pub use crate::removal::PresentationPolicy;

#[path = "upgrade.rs"]
mod upgrade;

#[path = "authoring.rs"]
mod authoring;
pub(crate) use authoring::model_refusals;

type A = Attribute;

#[cfg(all(feature = "json-import", feature = "presentation"))]
#[path = "source_presentation.rs"]
mod source_presentation;

/// Maximum generated Part 21 string literal size, including the quotes. Long
/// descriptive values use ordered text chunks; generated indivisible strings are
/// refused. Carried source strings exceeding it produce [`LongStringWarning`]s.
pub const MAX_STRING_LITERAL_BYTES: usize = super::text::MAX_LITERAL_BYTES;

/// What [`write`] does with each named part's existing PMI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Append the items without changing any original entity bytes. Schema upgrades
    /// requiring source-entity repairs fail with [`WriteError::AddPreservation`].
    /// A datum label the part already has
    /// resolves to its existing datum (the same faces), or is refused (other faces).
    Add,
    /// Remove what the reader consumed for the part and write the items: afterwards
    /// `read(part)` equals them. Unconsumed PMI of the part is kept and reported.
    Replace,
    /// Replace with nothing; each part's [`PartPmi`] must be empty.
    Remove,
}

/// An item of a part's PMI, by its index in the [`PartPmi`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ItemRef {
    Standard(usize),
    DecimalPlaces,
    Feature(usize),
    DatumTarget(usize),
    Datum(usize),
    Dimension(usize),
    Tolerance(usize),
    ToleranceRelation(usize),
    General(usize),
    Thread(usize),
    Knurl(usize),
    Material,
    Note(usize),
    SurfaceTexture(usize),
    Attribute(usize),
    Hole(usize),
}

/// One item the writer will not write, and why.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Refusal {
    pub part: PartId,
    pub item: ItemRef,
    pub why: String,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "part {} {:?}: {}", self.part.0, self.item, self.why)
    }
}

/// Why nothing was written.
#[derive(Clone, Debug, PartialEq)]
pub enum WriteError {
    /// The parts named are not the document's, are named twice, or (remove) carry PMI.
    Parts(String),
    /// A part's PMI violates the model's invariants.
    Invalid {
        part: PartId,
        errors: Vec<ModelError>,
    },
    /// Items the writer does not write (all of them, every part).
    Refused(Vec<Refusal>),
    /// The file's PMI could not be read (replace and add need it).
    Read(ReadError),
    /// A replace or remove is blocked: kept instances reference what it removes.
    Removal(Box<RemovalRefusal>),
    /// The file's schema cannot carry the PMI: an AP214/AP203 file gaining PMI whose instances
    /// do not all validate against the target AP242 edition (decision 1).
    Edition {
        file_schema: String,
        violations: Vec<Violation>,
    },
    /// Instances the writer made that the schema rejects (the backstop; a writer defect).
    Schema(Vec<Violation>),
    /// Rule violations the edit introduces (the backstop).
    Rules(Vec<RuleViolation>),
    /// Part 21 could not be written.
    P21(P21Error),
    /// Add cannot preserve these source entities while performing a schema
    /// upgrade/repair. Nothing is applied; Replace is the explicit rewriting mode.
    AddPreservation(Vec<u64>),
    /// The writer's own invariant failed (a defect), described.
    Internal(String),
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let list = |f: &mut std::fmt::Formatter<'_>, v: &[String]| -> std::fmt::Result {
            for (i, s) in v.iter().enumerate() {
                write!(f, "{}{s}", if i == 0 { ": " } else { "; " })?;
            }
            Ok(())
        };
        match self {
            WriteError::Parts(s) => write!(f, "parts: {s}"),
            WriteError::AddPreservation(ids) => write!(
                f,
                "Add would change original entities {ids:?}; use Replace for schema upgrades requiring entity repairs"
            ),
            WriteError::Invalid { part, errors } => {
                write!(f, "part {} PMI is not valid", part.0)?;
                list(f, &errors.iter().map(|e| e.0.clone()).collect::<Vec<_>>())
            }
            WriteError::Refused(r) => {
                write!(f, "not written")?;
                list(f, &r.iter().map(ToString::to_string).collect::<Vec<_>>())
            }
            WriteError::Read(e) => write!(f, "{e}"),
            WriteError::Removal(r) => write!(f, "{r}"),
            WriteError::Edition {
                file_schema,
                violations,
            } => {
                write!(
                    f,
                    "{file_schema} cannot become {}: instances violate it",
                    express::TARGET_SCHEMA
                )?;
                list(
                    f,
                    &violations
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>(),
                )
            }
            WriteError::Schema(v) => {
                write!(f, "written instances violate the schema")?;
                list(f, &v.iter().map(ToString::to_string).collect::<Vec<_>>())
            }
            WriteError::Rules(v) => {
                write!(f, "the edit introduces rule violations")?;
                list(f, &v.iter().map(ToString::to_string).collect::<Vec<_>>())
            }
            WriteError::P21(e) => write!(f, "{e}"),
            WriteError::Internal(s) => write!(f, "writer defect: {s}"),
        }
    }
}

impl std::error::Error for WriteError {}

impl From<P21Error> for WriteError {
    fn from(e: P21Error) -> Self {
        WriteError::P21(e)
    }
}

/// What happened to the file's `FILE_SCHEMA`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchemaChange {
    /// Kept as it was.
    Kept(Vec<String>),
    /// An AP214/AP203 file gaining PMI, or an identified JSON write requiring a
    /// newer AP242 SELECT, now [`express::TARGET_SCHEMA`].
    Upgraded { from: Vec<String>, to: String },
}

/// The closed list of repairs allowed only on an AP203/AP214 upgrade that adds PMI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeRepairKind {
    /// R1: retain an approval's items using AP242's broader approval assignment.
    R1ApprovalAssignment,
    /// R2: make a conversion unit's dimensions explicit, from its factor's unit.
    R2ConversionUnitDimensions,
}

impl std::fmt::Display for UpgradeRepairKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::R1ApprovalAssignment => "R1 approval assignment",
            Self::R2ConversionUnitDimensions => "R2 conversion unit dimensions",
        })
    }
}

/// One repaired record, including its exact text (`#N` through `;`) before and after.
/// Recovering the original bytes of this record requires keeping this report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpgradeRepair {
    pub id: u64,
    pub repair: UpgradeRepairKind,
    pub original_record: String,
    pub new_record: String,
}

/// An edition upgrade's keyword-only rewrite; all other record bytes are preserved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditionRewrite {
    pub id: u64,
    pub from: &'static str,
    pub to: &'static str,
}

impl std::fmt::Display for UpgradeRepair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "#{} {}: {} -> {}",
            self.id, self.repair, self.original_record, self.new_record
        )
    }
}

/// A carried string exceeding the writer's interoperability bound. This is not
/// an ISO string-length violation and never makes an otherwise lossless write fail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LongStringWarning {
    /// STEP instance (`#N`) or header record (`FILE_NAME`, etc.).
    pub record: String,
    /// Encoded literal length, including quotes, escapes and doubled apostrophes.
    pub encoded_bytes: usize,
    /// Start of the literal in the output file.
    pub byte_offset: usize,
}
impl std::fmt::Display for LongStringWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: retained string of {} encoded bytes exceeds {}; some readers (including OpenCascade's scanner) may not read it; carried unchanged",
            self.record, self.encoded_bytes, MAX_STRING_LITERAL_BYTES
        )
    }
}

/// A narrow PMI §5.1 carrier-sharing exception, either an independent datum feature
/// preserving a kept carrier or a private endpoint disambiguating a functional SAR.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeometryCarrierExemption {
    pub part: PartId,
    /// Preservation: `kept-carrier-datum-feature`, `kept-carrier-datum-member`,
    /// `kept-carrier-datum-composite`, `kept-carrier-independent-feature`, or
    /// `kept-carrier-legacy-forwarder`. Functional endpoints use
    /// `shared-carrier-controlled-pair` / `shared-carrier-datum-target-support`.
    pub condition: &'static str,
    /// The independent aspect (or retained legacy alias), and its precise
    /// preserved geometric carrier. Multi-face datum members are reported per face.
    pub aspect: u64,
    pub feature: u64,
}

/// What a write did.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct WriteReport {
    pub schema: SchemaChange,
    /// Oversized carried literals, in output order; non-fatal scanner compatibility warnings.
    pub long_strings: Vec<LongStringWarning>,
    /// Source placement diagnostics; placed callouts are never moved.
    pub presentation_warnings: Vec<String>,
    /// R1/R2 upgrade repairs, in instance id order; empty whenever no upgrade occurs.
    pub repairs: Vec<UpgradeRepair>,
    /// Closed AP214 subtype-to-supertype keyword rewrites, ascending by instance id.
    pub edition_rewrites: Vec<EditionRewrite>,
    /// The `FILE_SCHEMA` of the table the written instances were validated against.
    pub validated_against: &'static str,
    /// Instances added.
    pub added: usize,
    /// Instances removed (replace, remove), ascending.
    pub removed: Vec<u64>,
    /// Kept instances rewritten by Replace/Remove, including removal from shared
    /// containers and retained producer text rechunking. Always empty in Add.
    pub rewritten: Vec<u64>,
    /// Presentation and validation properties removed with the replaced PMI.
    pub presentation_removed: Vec<(u64, String)>,
    /// Shared instances only removed ones referenced, kept unreferenced.
    pub left_unreferenced: Vec<(u64, String)>,
    /// Datums of [`Mode::Add`] that resolved to the part's existing datum: part, label, `#N`.
    pub reused_datums: Vec<(PartId, String, u64)>,
    /// Supplemental geometry items of the file used again rather than written twice.
    pub reused_geometry: Vec<u64>,
    /// Datum feature symbols written (decision 6): part and label(s), in order.
    pub datum_symbols: Vec<(PartId, String)>,
    /// Legacy note migrations: part, original note index, standard form and reason.
    pub note_migrations: Vec<(PartId, usize, String)>,
    /// Removed presentation roots, with their note index in the source reading and instance type.
    pub note_presentation_removed: Vec<(PartId, usize, u64, String)>,
    /// The file's `FILE_SCHEMA` when its AP242 edition has no table: the written instances were
    /// validated against [`WriteReport::validated_against`] (the target edition), so whether
    /// they are valid instances of the file's own edition is undetermined.
    pub edition_undetermined: Option<String>,
    /// The reader's findings about PMI of a replaced part that stays (it was not consumed).
    pub kept_unconsumed: Vec<Finding>,
    /// Schema violations of instances the edit did not make (the original's).
    pub preexisting_violations: Vec<Violation>,
    /// Rule violations the original already had and still has.
    pub preexisting_rule_violations: Vec<RuleViolation>,
    /// Actual violations of the named practice/schema conflicts on newly added instances
    /// (maintainer decision 2026-10-20). The rule engine itself remains unchanged.
    pub practice_schema_conflicts: Vec<RuleViolation>,
    /// Narrow §5.1 exceptions: independent additions preserving kept carriers, retained
    /// legacy forwarders, and private endpoints disambiguating shared-carrier SARs.
    pub geometry_carrier_exemptions: Vec<GeometryCarrierExemption>,
    /// Add keeps these source records instead of performing the named 0.4.5
    /// normalization: (original instance, normalization not performed).
    pub preserved_normalizations: Vec<(u64, &'static str)>,
}

fn occt_semantic_features(p: &PartPmi) -> BTreeSet<usize> {
    let mut todo = Vec::new();
    todo.extend(p.datums.iter().filter_map(|d| d.feature()));
    for d in &p.dimensions {
        match &d.kind {
            DimensionKind::Size { feature, path, .. } => {
                todo.push(*feature);
                todo.extend(path);
            }
            DimensionKind::Location { from, to, path, .. } => {
                todo.extend([*from, *to]);
                todo.extend(path);
            }
        }
    }
    todo.extend(p.tolerances.iter().filter_map(|t| match t.target {
        ToleranceTarget::Feature(f) => Some(f),
        _ => None,
    }));
    todo.extend(p.threads.iter().map(|t| t.feature));
    todo.extend(p.knurls.iter().map(|k| k.feature));
    todo.extend(p.attributes.iter().filter_map(|a| match a.on {
        Some(NoteOwner::Feature(f)) => Some(f),
        _ => None,
    }));
    todo.extend(p.surface_textures.iter().filter_map(|s| s.on));
    todo.extend(p.notes.iter().filter_map(|n| match n.on {
        Some(NoteOwner::Feature(f)) => Some(f),
        _ => None,
    }));
    let mut out = BTreeSet::new();
    while let Some(f) = todo.pop() {
        let c = f.0;
        if out.insert(c)
            && let Feature::Group { members, .. } | Feature::Derived { from: members, .. } =
                &p.features[c]
        {
            todo.extend(members);
        }
    }
    out
}

/// The standard requirements that a write of `p` represents. Legacy requirement notes
/// become semantic text (§7.4), or surface texture when their complete text states it.
/// An anonymous single decimal-only cell is the carrier for the part default; its canonical
/// model form is `decimal_places`, while named tables retain every stated cell item.
/// Callouts are presentation and are deliberately not part of this migration.
#[must_use]
pub fn standard_form(p: &PartPmi) -> PartPmi {
    let mut out = p.clone();
    out.general
        .retain(|g| !decimal_carrier(g, out.decimal_places));
    for n in out.notes.drain(..) {
        if let Some(t) = note_texture(&n) {
            out.surface_textures.push(t);
        } else {
            out.attributes.push(AttributeSet {
                name: "semantic text".into(),
                on: n.on,
                items: vec![(n.kind, AttributeValue::Text(n.text))],
                description: None,
                general_property: None,
            });
        }
    }
    out
}

pub(crate) fn decimal_carrier(g: &GeneralTolerance, places: Option<u8>) -> bool {
    matches!(g, GeneralTolerance::Table { name, cells } if name.is_empty() && places.is_some() && matches!(cells.as_slice(), [c] if c.decimal_default() == places))
}

pub(crate) fn note_texture(n: &Note) -> Option<SurfaceTexture> {
    if !["surface texture", "surface finish", "finish"].contains(&n.kind.as_str()) {
        return None;
    }
    let on = match n.on {
        None => None,
        Some(NoteOwner::Feature(f)) => Some(f),
        _ => return None,
    };
    let mut words = n.text.split_whitespace();
    let characteristic = words.next()?;
    if !["Ra", "Rq", "Rz", "Rt", "Rp", "Rv"].contains(&characteristic) {
        return None;
    }
    let mut value = words.next()?.to_string();
    if !value.contains('.') {
        if let Some(at) = value.find(['e', 'E']) {
            value.insert(at, '.');
        } else {
            value.push('.');
        }
    }
    value.make_ascii_uppercase();
    let value = Decimal::parse(&value).ok()?;
    if value.to_f64() <= 0. || real(&value).is_err() {
        return None;
    }
    // Without the part unit here an unqualified value could mean microinches.
    // Preserve its wording rather than guessing a unit.
    let rest: Vec<_> = words.collect();
    if rest != ["µm"] && rest != ["μm"] && rest != ["um"] {
        return None;
    }
    Some(SurfaceTexture {
        on,
        material_removal: MaterialRemoval::AnyProcessAllowed,
        parameters: vec![SurfaceTextureParameter {
            characteristic: characteristic.into(),
            value: Length {
                value,
                unit: LengthUnit::Micrometre,
            },
        }],
    })
}

fn migration_reason(n: &Note) -> String {
    let reason = if note_texture(n).is_some() {
        "surface texture (ISO 10303-1110): complete roughness value, ISO 1302 basic symbol"
    } else {
        "semantic text (§7.4): complete wording retained; units or a complete supported semantic parameter set and scope are not stated"
    };
    let mut reason = reason.to_string();
    for callout in &n.callouts {
        use std::fmt::Write as _;
        let _ = write!(
            reason,
            "; presentation omitted from migrated note: {:?} on {:?}",
            callout.name, callout.features
        );
    }
    reason
}

enum DensityUnit {
    Length(LengthUnit),
    Mass(Option<&'static str>),
}

fn density_unit(name: &str) -> Option<DensityUnit> {
    Some(match name {
        "gram" => DensityUnit::Mass(None),
        "kilogram" => DensityUnit::Mass(Some("kilo")),
        "milligram" => DensityUnit::Mass(Some("milli")),
        "metre" => DensityUnit::Length(LengthUnit::Metre),
        "centimetre" => DensityUnit::Length(LengthUnit::Centimetre),
        "millimetre" => DensityUnit::Length(LengthUnit::Millimetre),
        "micrometre" => DensityUnit::Length(LengthUnit::Micrometre),
        "inch" => DensityUnit::Length(LengthUnit::Inch),
        "foot" => DensityUnit::Length(LengthUnit::Foot),
        _ => return None,
    })
}

/// Writes `pmi` (one entry per part, any order; each part at most once) into `doc`, whose
/// `parts` are `read_part_definitions` of the same bytes. One [`Edit`] for all parts, made in
/// part order; deterministic.
pub fn write(
    doc: &Document,
    parts: &[PartDefinition],
    pmi: &[(PartId, PartPmi)],
    mode: Mode,
    policy: PresentationPolicy,
) -> Result<(Edit, WriteReport), WriteError> {
    write_inner(
        doc,
        parts,
        pmi,
        mode,
        policy,
        #[cfg(feature = "json-import")]
        &[],
        #[cfg(feature = "json-import")]
        None,
    )
}

/// Write using source callout positions keyed by the semantic ids produced by
/// `json::identify(standard_form(pmi), None)`. Existing `write` behavior is unchanged.
/// Source input requires the `presentation` feature; without it, nonempty input
/// is refused. All input is validated before an edit is returned. Saved views are not written.
#[cfg(feature = "json-import")]
pub fn write_with_presentation(
    doc: &Document,
    parts: &[PartDefinition],
    pmi: &[(PartId, PartPmi)],
    mode: Mode,
    policy: PresentationPolicy,
    presentation: &[(PartId, crate::json::presentation::Presentation)],
) -> Result<(Edit, WriteReport), WriteError> {
    write_inner(doc, parts, pmi, mode, policy, presentation, None)
}

/// The JSON path supplies item ids; the ordinary writer always supplies None.
#[cfg(feature = "json-import")]
pub(crate) fn write_identified(
    doc: &Document,
    parts: &[PartDefinition],
    pmi: &[(PartId, PartPmi)],
    ids: &BTreeMap<PartId, BTreeMap<String, Vec<String>>>,
    protected: &BTreeSet<u64>,
    policy: PresentationPolicy,
    presentation: &[(PartId, crate::json::presentation::Presentation)],
) -> Result<(Edit, WriteReport), WriteError> {
    write_inner(
        doc,
        parts,
        pmi,
        Mode::Replace,
        policy,
        presentation,
        Some((ids, protected)),
    )
}

#[cfg(feature = "json-import")]
type IdentityInput<'a> = (
    &'a BTreeMap<PartId, BTreeMap<String, Vec<String>>>,
    &'a BTreeSet<u64>,
);

fn write_inner(
    doc: &Document,
    parts: &[PartDefinition],
    pmi: &[(PartId, PartPmi)],
    mode: Mode,
    policy: PresentationPolicy,
    #[cfg(feature = "json-import")] presentation: &[(
        PartId,
        crate::json::presentation::Presentation,
    )],
    #[cfg(feature = "json-import")] identity: Option<IdentityInput<'_>>,
) -> Result<(Edit, WriteReport), WriteError> {
    #[cfg(feature = "json-import")]
    let (identifiers, protected) =
        identity.map_or((None, None), |(ids, kept)| (Some(ids), Some(kept)));
    #[cfg(all(feature = "json-import", not(feature = "presentation")))]
    if !presentation.is_empty() {
        return Err(WriteError::Parts(
            "source presentation requires the `presentation` feature".into(),
        ));
    }
    #[cfg(all(feature = "json-import", feature = "presentation"))]
    source_presentation::validate_parts(presentation, pmi, mode)?;
    // Validate the input before migration (in particular every note's owner).
    for (part, p) in pmi {
        p.validate().map_err(|errors| WriteError::Invalid {
            part: *part,
            errors,
        })?;
    }
    let normalized: Vec<_> = pmi
        .iter()
        .map(|(id, p)| {
            #[cfg(feature = "json-import")]
            if identifiers.is_some() {
                return (*id, p.clone());
            }
            (*id, standard_form(p))
        })
        .collect();
    let mut order: Vec<&(PartId, PartPmi)> = normalized.iter().collect();
    order.sort_by_key(|(p, _)| *p);
    for w in order.windows(2) {
        if w[0].0 == w[1].0 {
            return Err(WriteError::Parts(format!("part {} named twice", w[0].0.0)));
        }
    }
    for (p, part_pmi) in &order {
        if p.0 >= parts.len() {
            return Err(WriteError::Parts(format!(
                "part {} does not exist ({} parts)",
                p.0,
                parts.len()
            )));
        }
        if mode == Mode::Remove && part_pmi != &PartPmi::default() {
            return Err(WriteError::Parts(format!(
                "remove names part {} with PMI to write",
                p.0
            )));
        }
        part_pmi
            .validate()
            .map_err(|errors| WriteError::Invalid { part: *p, errors })?;
    }
    #[cfg(feature = "json-import")]
    let read = if identifiers.is_some() {
        super::read_native(doc, parts)
    } else {
        super::read(doc, parts)
    }
    .map_err(WriteError::Read)?;
    #[cfg(not(feature = "json-import"))]
    let read = super::read(doc, parts).map_err(WriteError::Read)?;

    // The file's edition and the table written instances are checked against.
    let schema = doc.file_schema()?;
    let first = schema.first().cloned().unwrap_or_default();
    let ap242 = first
        .trim()
        .to_ascii_uppercase()
        .starts_with("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF");
    // Edition 1 excludes dimensional_size and geometric_tolerance from
    // characterized_definition. JSON keeps their exact owner by using edition 4's
    // characterized_item SELECT (CAx-IF PMI v4.1 §§7.4.2, 10.1.2.1–2).
    #[cfg(feature = "json-import")]
    let json_upgrade = express::edition(&first)
        .is_some_and(|e| std::ptr::eq(e, &express::AP242_ED1))
        && (presentation.iter().any(|(_, p)| {
            p.items.values().flatten().any(|g| {
                g.plane_id.is_some() || !g.text_placements.is_empty() || g.leader.is_some()
            })
        }) || (identifiers.is_some()
            && pmi.iter().any(|(_, p)| {
                !p.tolerance_relations.is_empty()
                    || p.notes
                        .iter()
                        .filter_map(|n| n.on)
                        .chain(p.attributes.iter().filter_map(|a| a.on))
                        .any(|owner| {
                            matches!(owner, NoteOwner::Dimension(_) | NoteOwner::Tolerance(_))
                        })
            })));
    #[cfg(not(feature = "json-import"))]
    let json_upgrade = false;
    let edition: &'static Edition = if json_upgrade {
        express::TARGET
    } else if ap242 {
        express::edition(&first).unwrap_or(express::TARGET)
    } else {
        express::TARGET
    };

    // Every refusal of every part, before anything is made.
    let mut refusals = Vec::new();
    let mut plans = Vec::new();
    for (p, part_pmi) in &order {
        let plan = PartPlan::new(doc, &parts[p.0], *p, part_pmi, &read, mode)?;
        #[cfg(feature = "json-import")]
        let plan = {
            let mut plan = PartPlan {
                identifiers: identifiers.and_then(|ids| ids.get(p)),
                #[cfg(feature = "presentation")]
                source: source_presentation::prepare(
                    part_pmi,
                    identifiers.and_then(|ids| ids.get(p)),
                    presentation.iter().find(|(id, _)| id == p).map(|(_, p)| p),
                )?,
                ..plan
            };
            if plan.identifiers.is_some() {
                for role in plan.roles.values_mut() {
                    if matches!(role, Role::DatumFeatureOfSize(_)) {
                        *role = Role::DatumFeature;
                    }
                }
            }
            plan
        };
        let original = &pmi
            .iter()
            .find(|(id, _)| id == p)
            .ok_or_else(|| WriteError::Internal("a planned part is not in the input".into()))?
            .1;
        #[allow(
            clippy::expect_used,
            reason = "the standard form is made from this input: each of its migrated or \
                      normalized items has its original"
        )]
        refusals.extend(plan.refusals.iter().cloned().map(|mut refusal| {
            // Plans use the standard form, but public item indices name the caller's input.
            refusal.item = match refusal.item {
                ItemRef::General(i) => ItemRef::General(
                    original
                        .general
                        .iter()
                        .enumerate()
                        .filter(|(_, g)| !decimal_carrier(g, original.decimal_places))
                        .nth(i)
                        .expect("normalized general tolerance belongs to input")
                        .0,
                ),
                ItemRef::Attribute(i) if i >= original.attributes.len() => ItemRef::Note(
                    original
                        .notes
                        .iter()
                        .enumerate()
                        .filter(|(_, n)| note_texture(n).is_none())
                        .nth(i - original.attributes.len())
                        .expect("migrated attribute belongs to a note")
                        .0,
                ),
                ItemRef::SurfaceTexture(i) if i >= original.surface_textures.len() => {
                    ItemRef::Note(
                        original
                            .notes
                            .iter()
                            .enumerate()
                            .filter(|(_, n)| note_texture(n).is_some())
                            .nth(i - original.surface_textures.len())
                            .expect("migrated texture belongs to a note")
                            .0,
                    )
                }
                item => item,
            };
            refusal
        }));
        plans.push(plan);
    }
    if !refusals.is_empty() {
        refusals.sort();
        return Err(WriteError::Refused(refusals));
    }

    let mut edit = Edit::new();
    let mut report = WriteReport {
        schema: SchemaChange::Kept(schema.clone()),
        long_strings: Vec::new(),
        presentation_warnings: Vec::new(),
        repairs: Vec::new(),
        edition_rewrites: Vec::new(),
        validated_against: edition.file_schema,
        added: 0,
        removed: Vec::new(),
        rewritten: Vec::new(),
        presentation_removed: Vec::new(),
        note_presentation_removed: Vec::new(),
        left_unreferenced: Vec::new(),
        reused_datums: Vec::new(),
        reused_geometry: Vec::new(),
        datum_symbols: Vec::new(),
        note_migrations: pmi
            .iter()
            .flat_map(|(part, p)| {
                p.notes
                    .iter()
                    .enumerate()
                    .map(move |(i, n)| (*part, i, migration_reason(n)))
            })
            .collect(),
        edition_undetermined: (ap242 && express::edition(&first).is_none()).then(|| first.clone()),
        kept_unconsumed: Vec::new(),
        preexisting_violations: Vec::new(),
        preexisting_rule_violations: Vec::new(),
        practice_schema_conflicts: Vec::new(),
        geometry_carrier_exemptions: Vec::new(),
        preserved_normalizations: Vec::new(),
    };

    report
        .note_migrations
        .sort_by_key(|(part, note, _)| (*part, *note));

    // Replace and remove: one removal plan for every named part's consumed instances.
    let mut removed: BTreeSet<u64> = BTreeSet::new();
    if mode != Mode::Add {
        let mut seeds = BTreeSet::new();
        for (p, _) in &order {
            let prov = &read.provenance.parts[p.0];
            // Supplemental geometry is the part's geometry, not PMI: it is kept (and used
            // again by what is written, matched by value), never removed.
            let geometry: BTreeSet<u64> = prov.geometry.iter().flatten().copied().collect();
            seeds.extend(prov.all().into_iter().filter(|id| {
                #[cfg(feature = "json-import")]
                if protected.is_some_and(|ids| ids.contains(id)) {
                    return false;
                }
                !geometry.contains(id)
            }));
        }
        if !seeds.is_empty() {
            // A presentation model (draughting model, view) the removal would empty is the
            // caller's decision (removal.rs): under RemovePresentation the writer removes it
            // with the presentation it held, and lists it; under Refuse it is refused.
            let mut emptied: Vec<(u64, String)> = Vec::new();
            let plan = loop {
                match removal::plan(doc, &seeds, policy) {
                    Ok(plan) => break plan,
                    Err(r)
                        if policy == PresentationPolicy::RemovePresentation
                            && !r.emptied.is_empty()
                            && r.blockers.is_empty()
                            && r.missing.is_empty()
                            && r.infrastructure.is_empty()
                            && r.anchored.is_empty()
                            && r.emptied.iter().all(|e| !seeds.contains(&e.id)) =>
                    {
                        for e in &r.emptied {
                            seeds.insert(e.id);
                            emptied.push((e.id, e.entity.clone()));
                        }
                    }
                    Err(r) if r.emptied.is_empty() && !r.blockers.is_empty() => {
                        // Parts of a removed item's construct the reader does not list: its
                        // `id_attribute`s (§5.1 Figure 4) and edition 4 `uuid_attribute`s and, under RemovePresentation, the
                        // geometric validation properties of it (with their representation
                        // links), plus Specify's non-semantic writer ownership stamp.
                        // Other description attributes still block.
                        let extra: Vec<u64> = r
                            .blockers
                            .iter()
                            .map(|b| b.id)
                            .filter(|&b| {
                                is_a(doc, b, "id_attribute")
                                    || is_a(doc, b, "uuid_attribute")
                                    // Specify's ownership stamp carries no semantic value;
                                    // it belongs to the removed construct, like an item id.
                                    || (is_a(doc, b, "description_attribute")
                                        && attr_str(doc, b, "description_attribute", "attribute_value").as_deref()
                                            == Some("specify-core:written:v1"))
                                    || (policy == PresentationPolicy::RemovePresentation
                                        && validation_property(doc, b, &seeds))
                            })
                            .collect();
                        if extra.len() != r.blockers.len() {
                            return Err(WriteError::Removal(Box::new(r)));
                        }
                        for b in extra {
                            if is_a(doc, b, "property_definition") {
                                emptied.push((b, "property_definition".into()));
                            }
                            seeds.insert(b);
                        }
                    }
                    Err(r) => return Err(WriteError::Removal(Box::new(r))),
                }
            };
            #[cfg(feature = "json-import")]
            let plan = if let Some(protected) = protected {
                let mut plan = plan;
                // Removal can reach kept ink through an annotation plane or another
                // presentation container even after its associations were detached.
                // Its complete forward dependency graph must survive unchanged.
                plan.removed.retain(|id| !protected.contains(id));
                plan.rewritten.retain(|id, _| !protected.contains(id));
                plan.presentation_removed
                    .retain(|(id, _)| !protected.contains(id));
                plan
            } else {
                plan
            };
            plan.into_edit(&mut edit);
            removed = plan.removed.clone();
            report.removed = plan.removed.iter().copied().collect();
            report.rewritten = plan.rewritten.keys().copied().collect();
            report.presentation_removed = plan.presentation_removed.clone();
            report.presentation_removed.extend(emptied);
            #[cfg(feature = "json-import")]
            if protected.is_some() {
                report
                    .presentation_removed
                    .retain(|(id, _)| removed.contains(id));
            }
            report.presentation_removed.sort();
            report.left_unreferenced = plan.report.left_unreferenced.clone();
            for (part, _) in &order {
                for (note, callouts) in read.provenance.parts[part.0]
                    .note_callouts
                    .iter()
                    .enumerate()
                {
                    for &id in callouts {
                        if removed.contains(&id) {
                            let kind = removal::entity_name(doc.get(id).ok_or_else(|| {
                                WriteError::Internal(format!("callout #{id} is not in the file"))
                            })?);
                            report
                                .note_presentation_removed
                                .push((*part, note, id, kind.clone()));
                            if !report.presentation_removed.iter().any(|(i, _)| *i == id) {
                                report.presentation_removed.push((id, kind));
                            }
                        }
                    }
                }
            }
            report.presentation_removed.sort();
        }
        for (p, _) in &order {
            for f in &read.findings {
                if f.part == Some(*p)
                    && matches!(
                        f.kind,
                        FindingKind::Unconsumed
                            | FindingKind::Unsupported
                            | FindingKind::Unresolved
                            | FindingKind::Nonconformance
                    )
                    && f.ids.first().is_some_and(|id| !removed.contains(id))
                {
                    report.kept_unconsumed.push(f.clone());
                }
            }
        }
    }

    // Parse geometry once for the entire write, only when a new callout needs it. Parts
    // share the parsed STEP model; each B-rep and ray caster is still local to its part.
    let presentation_reader = plans
        .iter()
        .any(|p| !p.pmi.datums.is_empty() || needs_callouts(p.pmi))
        .then(|| crate::step::PartReader::read(doc.bytes()).ok())
        .flatten();
    // Emission, part by part.
    let mut em = Emitter::new(doc, edition, &mut edit, &removed);
    // Preserve the complete source document, including other selected parts and
    // incidental legacy normalization, for every Add write.
    em.preserve_kept = mode == Mode::Add;
    for plan in &plans {
        plan.emit(&mut em, &mut report, presentation_reader.as_ref())?;
    }
    repair_legacy_text(&mut em, &read, parts, &mut report)?;
    let emitted_dimensions = em
        .exponents
        .iter()
        .map(|(kind, r)| {
            let mut d = [0.; 7];
            if *kind == "length" {
                d[0] = 1.;
            }
            (d, r.a())
        })
        .collect();
    let added = em.count;
    let practice_exemptions = em.practice_exemptions.clone();
    let carrier_exemptions = em.carrier_exemptions.clone();
    report.preserved_normalizations = em.preserved_normalizations.iter().copied().collect();
    report.rewritten.extend(em.kept_rewrites.keys().copied());
    report.removed.extend(em.retired.iter().copied());
    report.removed.sort_unstable();
    report.removed.dedup();
    for (&id, record) in &em.kept_rewrites {
        if !em.retired.contains(&id) {
            em.edit.replace(id, record.clone());
        }
    }
    for &id in &em.retired {
        em.edit.remove(id);
    }
    report.rewritten.retain(|id| !em.retired.contains(id));
    report.rewritten.sort_unstable();
    report.rewritten.dedup();
    drop(em);
    report.added = added;

    // Decision 1: an AP214/AP203 file that gains PMI becomes AP242, if it can.
    let upgrade = added > 0 && (!ap242 || json_upgrade);
    let repaired = if upgrade {
        // Work on the original records; removal/presentation rewrites are never repaired.
        let excluded = removed
            .iter()
            .copied()
            .chain(report.rewritten.iter().copied())
            .collect();
        if first.to_ascii_uppercase().starts_with("AUTOMOTIVE_DESIGN") {
            report.edition_rewrites = upgrade::rewrite_entities(doc, &mut edit, &excluded);
        }
        let (repairs, added_dimensions) =
            upgrade::repair(doc, &mut edit, &excluded, emitted_dimensions)?;
        report.added += added_dimensions;
        repairs
    } else {
        Vec::new()
    };
    if upgrade {
        edit.set_file_schema(vec![express::TARGET_SCHEMA.to_string()]);
        report.schema = SchemaChange::Upgraded {
            from: schema.clone(),
            to: express::TARGET_SCHEMA.to_string(),
        };
    }

    // The backstop: apply, validate what was made, compare the rules.
    let written = doc.apply(&edit)?;
    report.geometry_carrier_exemptions = carrier_exemptions
        .into_iter()
        .map(
            |(part, condition, aspect, feature)| GeometryCarrierExemption {
                part,
                condition,
                aspect: match aspect {
                    R::Old(id) => id,
                    R::New(id) => written.ids[&id],
                },
                feature: match feature {
                    R::Old(id) => id,
                    R::New(id) => written.ids[&id],
                },
            },
        )
        .collect();
    report.long_strings = super::text::long_strings(&written.bytes);
    let after = Document::parse(written.bytes)?;
    for (id, repair) in repaired {
        report.repairs.push(UpgradeRepair {
            id,
            repair,
            original_record: upgrade::record_text(doc, id)?,
            new_record: upgrade::record_text(&after, id)?,
        });
    }
    let made: BTreeSet<u64> = written
        .ids
        .values()
        .copied()
        .chain(report.rewritten.iter().copied())
        .collect();
    let violations = edition.validate_document(after.graph());
    if upgrade && !violations.is_empty() {
        return Err(WriteError::Edition {
            file_schema: first,
            violations,
        });
    }
    // A rewritten container keeps the violations it had (its other items are the file's).
    let rewritten: BTreeSet<u64> = report.rewritten.iter().copied().collect();
    let had: BTreeSet<(u64, String)> = if rewritten.is_empty() {
        BTreeSet::new()
    } else {
        edition
            .validate_document(doc.graph())
            .into_iter()
            .filter(|v| rewritten.contains(&v.id))
            .map(|v| (v.id, violation_key(&v)))
            .collect()
    };
    let (bad, old): (Vec<Violation>, Vec<Violation>) = violations
        .into_iter()
        .partition(|v| made.contains(&v.id) && !had.contains(&(v.id, violation_key(v))));
    if !bad.is_empty() {
        return Err(WriteError::Schema(bad));
    }
    report.preexisting_violations = old;
    let before: BTreeSet<(u64, &'static str)> = express_rules::check_all(doc)
        .into_iter()
        .map(|v| (v.id, v.rule))
        .collect();
    let (new, kept): (Vec<RuleViolation>, Vec<RuleViolation>) = express_rules::check_all(&after)
        .into_iter()
        .partition(|v| !before.contains(&(v.id, v.rule)));
    // Resolve an allowlist registered at emission sites, never by population/type alone.
    // Only additions have NewIds: kept/replaced records cannot enter this exemption.
    let allowed: BTreeSet<_> = practice_exemptions
        .into_iter()
        .map(|(id, rule)| (written.ids[&id], rule))
        .collect();
    let (known, bad) = partition_practice_conflicts(new, &allowed);
    report.practice_schema_conflicts = known;
    if !bad.is_empty() {
        return Err(WriteError::Rules(bad));
    }
    report.preexisting_rule_violations = kept;
    if mode == Mode::Add {
        let changed: Vec<_> = doc
            .ids()
            .filter(|id| {
                doc.span(*id).map(|span| &doc.bytes()[span])
                    != after.span(*id).map(|span| &after.bytes()[span])
            })
            .collect();
        if !changed.is_empty() {
            return Err(WriteError::AddPreservation(changed));
        }
    }
    Ok((edit, report))
}

/// Our ordering property marks a JSON-written part. Its consumed descriptive PMI
/// (not foreign graphics or supplemental source geometry) was emitted by haecceity.
/// Explicit attribute representations are also our producer convention. Replace
/// repairs legacy literals in place; Add preserves and reports them instead.
fn repair_legacy_text(
    em: &mut Emitter<'_>,
    read: &PmiRead,
    parts: &[PartDefinition],
    report: &mut WriteReport,
) -> Result<(), WriteError> {
    let doc = em.doc;
    let mut owned = BTreeSet::new();
    let mut marked_parts = BTreeSet::new();
    for id in doc.ids() {
        if is_a(doc, id, "property_definition")
            && attr_str(doc, id, "property_definition", "name").as_deref()
                == Some("haecceity.pmi/1 ordering")
        {
            if let Some(owner) = attr_ref(doc, id, "property_definition", "definition") {
                marked_parts.extend(
                    parts
                        .iter()
                        .enumerate()
                        .filter(|(_, p)| p.shape == owner)
                        .map(|(i, _)| i),
                );
            }
            for &link in doc.referrers(id) {
                if is_a(doc, link, "property_definition_representation")
                    && let Some(rep) = attr_ref(
                        doc,
                        link,
                        "property_definition_representation",
                        "used_representation",
                    )
                {
                    owned.extend(attr_refs(doc, rep, "representation", "items"));
                }
            }
        }
        if is_a(doc, id, "representation")
            && attr_str(doc, id, "representation", "name").as_deref()
                == Some("haecceity.pmi/1 attribute")
        {
            owned.extend(attr_refs(doc, id, "representation", "items"));
        }
    }
    for i in marked_parts {
        let prov = &read.provenance.parts[i];
        let geometry: BTreeSet<_> = prov.geometry.iter().flatten().copied().collect();
        owned.extend(prov.all().into_iter().filter(|id| !geometry.contains(id)));
    }
    for id in owned {
        if em.removed.contains(&id) || report.rewritten.contains(&id) {
            continue;
        }
        let Some(RawEntity::Simple {
            name, attributes, ..
        }) = doc.get(id)
        else {
            continue;
        };
        if !name.eq_ignore_ascii_case("descriptive_representation_item") {
            continue;
        }
        let [A::String(name), A::String(text)] = attributes.as_slice() else {
            continue;
        };
        if super::text::literal_len(text) <= MAX_STRING_LITERAL_BYTES {
            continue;
        }
        if em.preserve_kept {
            em.preserved_normalizations
                .insert((id, "legacy-text-rechunking"));
            continue;
        }
        // The label is source text too; rechunking must not impose a new limit on it.
        let mut chunks = Vec::new();
        for chunk in super::text::split(text) {
            chunks.push(em.simple(
                "descriptive_representation_item",
                &[
                    ("name", s(super::text::CHUNK_NAME)),
                    ("description", s(chunk)),
                ],
            )?);
        }
        em.edit.replace(
            id,
            p21::simple(
                "COMPOUND_REPRESENTATION_ITEM",
                vec![s(name), typed("list_representation_item", refs(chunks))],
            ),
        );
        report.rewritten.push(id);
    }
    report.rewritten.sort_unstable();
    Ok(())
}

fn partition_practice_conflicts(
    violations: Vec<RuleViolation>,
    allowed: &BTreeSet<(u64, &'static str)>,
) -> (Vec<RuleViolation>, Vec<RuleViolation>) {
    violations
        .into_iter()
        .partition(|v| allowed.contains(&(v.id, v.rule)))
}

/// A violation without the positions of list elements (which a rewrite shifts).
fn violation_key(v: &Violation) -> String {
    let mut out = String::new();
    let mut chars = v.detail.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '[' && chars.peek().is_some_and(char::is_ascii_digit) {
            while chars.next().is_some_and(|x| x != ']') {}
            out.push_str("[]");
        } else {
            out.push(c);
        }
    }
    format!("{:?} {:?} {out}", v.kind, v.attribute)
}

// ---------------------------------------------------------------------------------------------
// Instance access (the document's own records)
// ---------------------------------------------------------------------------------------------

fn leaf_names(doc: &Document, id: u64) -> Vec<String> {
    match doc.get(id) {
        Some(RawEntity::Simple { name, .. }) => vec![name.to_ascii_lowercase()],
        Some(RawEntity::Complex { parts, .. }) => {
            parts.iter().map(|p| p.name.to_ascii_lowercase()).collect()
        }
        None => Vec::new(),
    }
}

fn is_a(doc: &Document, id: u64, ancestor: &str) -> bool {
    leaf_names(doc, id).iter().any(|n| {
        express::TARGET.is_a(n, ancestor)
            || (!express::TARGET.is_declared(n) && express::AP242_ED1.is_a(n, ancestor))
    })
}

/// Only neutral feature/union carriers are interchangeable with an ordinary feature.
fn neutral_aspect(doc: &Document, id: u64) -> bool {
    let names = leaf_names(doc, id);
    !names.is_empty()
        && names
            .iter()
            .all(|n| matches!(n.as_str(), "shape_aspect" | "composite_shape_aspect"))
}
fn reusable_aspect(doc: &Document, id: u64) -> bool {
    is_a(doc, id, "datum_feature")
        || (neutral_aspect(doc, id)
            && attr_str(doc, id, "shape_aspect", "name").as_deref() != Some("DIMENSION"))
}
// OCCT's complex composite/datum-feature records inherit ShapeAspect, not
// DatumFeature. They take collectShapeAspect's ordinary-SAR branch.
fn occt_stopping_record(record: &RawEntity) -> bool {
    matches!(record, RawEntity::Simple {name,..} if ["datum_feature", "datum_target", "placed_datum_target_feature", "derived_shape_aspect", "apex", "centre_of_symmetry", "geometric_alignment", "perpendicular_to", "extension", "tangent", "parallel_offset", "dimensional_size_with_datum_feature"].iter().any(|ty| name.eq_ignore_ascii_case(ty)))
}

fn geometric_composition(doc: &Document, id: u64) -> bool {
    leaf_names(doc, id) == ["shape_aspect_relationship"]
        && !["name", "description"].iter().any(|field| {
            attr_str(doc, id, "shape_aspect_relationship", field).as_deref()
                == Some("affected plane association")
        })
        && !doc.referrers(id).iter().any(|r| {
            attr_ref(doc, *r, "geometric_tolerance", "toleranced_shape_aspect") == Some(id)
        })
}

/// A neutral forwarding shell is not the geometric carrier, even if numbered first.
fn plain_forwarder(doc: &Document, id: u64) -> bool {
    if !neutral_aspect(doc, id) {
        return false;
    }
    let mut members = 0;
    for &r in doc.referrers(id) {
        if is_a(doc, r, "item_identified_representation_usage")
            && !is_a(doc, r, "draughting_model_item_association")
            && attr_ref(doc, r, "item_identified_representation_usage", "definition") == Some(id)
        {
            return false;
        }
        if geometric_composition(doc, r)
            && attr_ref(doc, r, "shape_aspect_relationship", "relating_shape_aspect") == Some(id)
            && attr_ref(doc, r, "shape_aspect_relationship", "related_shape_aspect")
                .is_some_and(|m| !is_a(doc, m, "datum") && !is_a(doc, m, "datum_target"))
        {
            members += 1;
        }
    }
    members == 1
}

/// Attribute `attr` declared by `entity` of instance `id`.
fn attr<'d>(doc: &'d Document, id: u64, entity: &str, attr: &str) -> Option<&'d Attribute> {
    match doc.get(id)? {
        RawEntity::Simple {
            name, attributes, ..
        } => {
            let slots = express::TARGET
                .explicit_attributes(name)
                .or_else(|| express::AP242_ED1.explicit_attributes(name))?;
            let i = slots
                .iter()
                .position(|s| s.entity == entity && s.name == attr)?;
            attributes.get(i)
        }
        RawEntity::Complex { parts, .. } => {
            let leaf = parts.iter().find(|p| p.name.eq_ignore_ascii_case(entity))?;
            let decl = express::TARGET
                .entity(entity)
                .or_else(|| express::AP242_ED1.entity(entity))?;
            let i = decl.attributes.iter().position(|a| a.name == attr)?;
            leaf.attributes.get(i)
        }
    }
}

fn attr_ref(doc: &Document, id: u64, entity: &str, a: &str) -> Option<u64> {
    match attr(doc, id, entity, a)? {
        Attribute::EntityRef(n) => Some(*n),
        _ => None,
    }
}

fn attr_refs(doc: &Document, id: u64, entity: &str, a: &str) -> Vec<u64> {
    let mut out = Vec::new();
    if let Some(v) = attr(doc, id, entity, a) {
        p21::visit_attributes(&p21::simple("X", vec![v.clone()]), &mut |x| {
            if let Attribute::EntityRef(n) = x {
                out.push(*n);
            }
        });
    }
    out
}

fn attr_str(doc: &Document, id: u64, entity: &str, a: &str) -> Option<String> {
    match attr(doc, id, entity, a)? {
        Attribute::String(s) => Some(s.clone()),
        _ => None,
    }
}

fn attr_enum<'d>(doc: &'d Document, id: u64, entity: &str, a: &str) -> Option<&'d str> {
    match attr(doc, id, entity, a)? {
        Attribute::Enum(s) => Some(s),
        _ => None,
    }
}

/// The shape representation (not a draughting model) that holds a supplemental geometry item,
/// the one a usage of it names (`shape_model`).
fn geometry_rep(doc: &Document, item: u64) -> Option<u64> {
    using_representations(doc, item)
        .into_iter()
        .find(|&r| is_a(doc, r, "shape_representation") && !is_a(doc, r, "draughting_model"))
}

/// A geometric validation property (or its representation link) of something removed: the
/// CAx-IF validation properties, which some files name freely ('length of #4654') but
/// describe as such.
fn validation_property(doc: &Document, id: u64, seeds: &BTreeSet<u64>) -> bool {
    let is_validation = |pd: u64| {
        is_a(doc, pd, "property_definition")
            && (attr_str(doc, pd, "property_definition", "description")
                .is_some_and(|d| d.ends_with("validation property"))
                || attr_str(doc, pd, "property_definition", "name")
                    .is_some_and(|n| express::VALIDATION_PROPERTY_NAMES.contains(&n.as_str())))
    };
    if is_a(doc, id, "property_definition_representation") {
        return attr_ref(doc, id, "property_definition_representation", "definition")
            .is_some_and(|pd| seeds.contains(&pd) && is_validation(pd));
    }
    is_validation(id)
        && attr_ref(doc, id, "property_definition", "definition")
            .is_some_and(|d| seeds.contains(&d))
}

/// The representations whose items hold `item`, directly or through representation items and
/// founded items (the schema's `using_representations`), ascending.
fn using_representations(doc: &Document, item: u64) -> Vec<u64> {
    let mut seen = BTreeSet::from([item]);
    let mut work = vec![item];
    while let Some(x) = work.pop() {
        for &r in doc.referrers(x) {
            if (is_a(doc, r, "representation_item") || is_a(doc, r, "founded_item"))
                && seen.insert(r)
            {
                work.push(r);
            }
        }
    }
    let mut out = BTreeSet::new();
    for &x in &seen {
        for &r in doc.referrers(x) {
            if is_a(doc, r, "representation")
                && attr_refs(doc, r, "representation", "items").contains(&x)
            {
                out.insert(r);
            }
        }
    }
    out.into_iter().collect()
}

/// A length or angle unit of the document, as the reader would read it (`si_unit` with its
/// prefix; `conversion_based_unit` by its factor).
#[derive(Clone, Debug, PartialEq)]
enum DocUnit {
    Length(LengthUnit),
    Angle(AngleUnit),
}

fn doc_unit(doc: &Document, id: u64, depth: usize) -> Option<DocUnit> {
    if depth > 4 {
        return None;
    }
    let names = leaf_names(doc, id);
    let has = |n: &str| names.iter().any(|x| x == n);
    if has("si_unit") {
        let prefix = attr_enum(doc, id, "si_unit", "prefix").map(str::to_ascii_uppercase);
        let name = attr_enum(doc, id, "si_unit", "name")?.to_ascii_uppercase();
        let power = match prefix.as_deref() {
            None => 0,
            Some(p) => si_prefix(p)?,
        };
        return match name.as_str() {
            "METRE" => {
                let full = format!(
                    "{}{}",
                    prefix
                        .as_deref()
                        .map_or(String::new(), str::to_ascii_lowercase),
                    "metre"
                );
                Some(DocUnit::Length(LengthUnit::from_metres(
                    &full,
                    Decimal::parse("1.").ok()?.scaled(power),
                )))
            }
            "RADIAN" if power == 0 => Some(DocUnit::Angle(AngleUnit::Radian)),
            _ => None,
        };
    }
    if has("conversion_based_unit") {
        let name = attr_str(doc, id, "conversion_based_unit", "name")?;
        let f = attr_ref(doc, id, "conversion_based_unit", "conversion_factor")?;
        let (value, unit) = measure_of(doc, f, depth + 1)?;
        return match unit {
            DocUnit::Length(u) => {
                let unit = LengthUnit::from_metres(&name, value.times(&u.metres()));
                Some(DocUnit::Length(unit))
            }
            DocUnit::Angle(a) => {
                let radians = value.to_f64() * a.radians();
                let degree = std::f64::consts::PI / 180.0;
                if (radians / degree - 1.0).abs() < 1e-9 {
                    Some(DocUnit::Angle(AngleUnit::Degree))
                } else if a == AngleUnit::Radian {
                    Some(DocUnit::Angle(AngleUnit::Other {
                        name,
                        radians: value,
                    }))
                } else {
                    None
                }
            }
        };
    }
    None
}

/// The value (as the file writes it) and unit of a measure with unit.
fn measure_of(doc: &Document, id: u64, depth: usize) -> Option<(Decimal, DocUnit)> {
    let v = attr(doc, id, "measure_with_unit", "value_component")?;
    let inner = match v {
        Attribute::Typed { value, .. } => &**value,
        v => v,
    };
    let text = match inner {
        Attribute::Real(r) => p21::encode_real(*r).ok()?,
        Attribute::Integer(i) => format!("{i}."),
        _ => return None,
    };
    let u = attr_ref(doc, id, "measure_with_unit", "unit_component")?;
    Some((Decimal::parse(&text).ok()?, doc_unit(doc, u, depth)?))
}

/// The name the reader gives a unit that is neither a length nor an angle (`Unit::Other`).
fn other_unit_name(doc: &Document, id: u64) -> Option<String> {
    let names = leaf_names(doc, id);
    let has = |n: &str| names.iter().any(|x| x == n);
    if has("si_unit") {
        let prefix = attr_enum(doc, id, "si_unit", "prefix").map(str::to_ascii_lowercase);
        let name = attr_enum(doc, id, "si_unit", "name")?.to_ascii_lowercase();
        if name == "metre" || name == "radian" {
            return None;
        }
        return Some(format!("{}{name}", prefix.unwrap_or_default()));
    }
    if has("conversion_based_unit") {
        if doc_unit(doc, id, 0).is_some() {
            return None;
        }
        return attr_str(doc, id, "conversion_based_unit", "name");
    }
    if has("context_dependent_unit") {
        return attr_str(doc, id, "context_dependent_unit", "name");
    }
    if has("derived_unit") || has("named_unit") {
        return Some(names.join("+"));
    }
    None
}

fn si_prefix(p: &str) -> Option<i64> {
    Some(match p {
        "EXA" => 18,
        "PETA" => 15,
        "TERA" => 12,
        "GIGA" => 9,
        "MEGA" => 6,
        "KILO" => 3,
        "HECTO" => 2,
        "DECA" => 1,
        "DECI" => -1,
        "CENTI" => -2,
        "MILLI" => -3,
        "MICRO" => -6,
        "NANO" => -9,
        "PICO" => -12,
        "FEMTO" => -15,
        "ATTO" => -18,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------------------------
// The typed emission layer
// ---------------------------------------------------------------------------------------------

/// An instance a record references: one of the file's, or one the edit adds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InstanceRef {
    Old(u64),
    New(NewId),
}

impl InstanceRef {
    fn a(self) -> A {
        match self {
            InstanceRef::Old(n) => A::EntityRef(n),
            InstanceRef::New(n) => n.to_ref(),
        }
    }
}

type R = InstanceRef;

/// A length unit instance, found or added for one value's unit: the only unit a
/// `length_measure_with_unit` can be made with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LengthUnitRef(R);

/// A plane angle unit instance, likewise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AngleUnitRef(R);

fn s(text: &str) -> A {
    A::String(text.to_owned())
}

fn en(v: &str) -> A {
    A::Enum(v.to_ascii_uppercase())
}

fn boolean(v: bool) -> A {
    A::Enum(if v { "T" } else { "F" }.into())
}

fn typed(t: &str, v: A) -> A {
    A::Typed {
        type_name: t.to_ascii_uppercase(),
        value: Box::new(v),
    }
}

fn refs(v: impl IntoIterator<Item = R>) -> A {
    A::List(v.into_iter().map(R::a).collect())
}

/// Encode a decimal as a legal REAL without changing its exact value. JSON transport
/// retains spellings that Part 21 cannot express (integer mantissas and lower-case e).
pub(crate) fn real_text(d: &Decimal) -> Result<String, String> {
    let mut text = d.as_str().to_ascii_uppercase();
    if !text.contains('.') {
        let at = text.find('E').unwrap_or(text.len());
        text.insert(at, '.');
    }
    p21::encode_real_text(&text).map_err(|e| e.to_string())
}
fn real(d: &Decimal) -> Result<A, String> {
    real_text(d)?;
    Ok(A::Real(d.to_f64()))
}

// COUNT_MEASURE is NUMBER, unlike lengths/angles/ratios (REAL). Preserve canonical
// integer counts read from Part 21 as integers; do not add a decimal point to them.
fn count_integer(d: &Decimal) -> Option<i64> {
    let i = d.as_str().parse::<i64>().ok()?;
    (i.to_string() == d.as_str()).then_some(i)
}

/// The emitter: the only way the writer makes records.
pub struct Emitter<'a> {
    doc: &'a Document,
    edition: &'static Edition,
    edit: &'a mut Edit,
    /// Instances the edit removes: never referenced by what it adds.
    removed: &'a BTreeSet<u64>,
    /// Units found or added, by their unit.
    length_units: Vec<(LengthUnit, LengthUnitRef)>,
    angle_units: Vec<(AngleUnit, AngleUnitRef)>,
    other_units: BTreeMap<String, R>,
    exponents: BTreeMap<&'static str, R>,
    count: usize,
    practice_exemptions: BTreeSet<(NewId, &'static str)>,
    // Source representation membership is immutable during emission. Reuse the
    // graph walk across usage planning, exact-carrier lookup and final emission.
    shape_representations: BTreeMap<u64, u64>,
    occt_stopping: BTreeSet<R>,
    direct_gisu: BTreeSet<R>,
    carrier_exemptions: Vec<(PartId, &'static str, R, R)>,
    preserved_normalizations: BTreeSet<(u64, &'static str)>,
    preserve_kept: bool,
    kept_rewrites: BTreeMap<u64, RawEntity>,
    retired: BTreeSet<u64>,
}

impl<'a> Emitter<'a> {
    fn new(
        doc: &'a Document,
        edition: &'static Edition,
        edit: &'a mut Edit,
        removed: &'a BTreeSet<u64>,
    ) -> Self {
        Emitter {
            doc,
            edition,
            edit,
            removed,
            length_units: Vec::new(),
            angle_units: Vec::new(),
            other_units: BTreeMap::new(),
            exponents: BTreeMap::new(),
            count: 0,
            practice_exemptions: BTreeSet::new(),
            shape_representations: BTreeMap::new(),
            occt_stopping: BTreeSet::new(),
            direct_gisu: BTreeSet::new(),
            carrier_exemptions: Vec::new(),
            preserved_normalizations: BTreeSet::new(),
            preserve_kept: false,
            kept_rewrites: BTreeMap::new(),
            retired: BTreeSet::new(),
        }
    }

    fn practice_conflict(&mut self, instance: R, rules: &[&'static str]) {
        #[allow(
            clippy::unreachable,
            reason = "callers pass only instances the writer adds"
        )]
        let R::New(id) = instance else {
            unreachable!("only new instances may be exempted");
        };
        self.practice_exemptions
            .extend(rules.iter().map(|&rule| (id, rule)));
    }

    /// The provisional reference of the next instance this emitter adds (`#2^63 + n`, as
    /// `p21` documents), for a record that references itself.
    #[cfg(not(feature = "presentation"))]
    fn next_ref(&self) -> A {
        A::EntityRef((1u64 << 63) | self.count as u64)
    }

    fn push(&mut self, e: RawEntity, stated: &[&Decimal]) -> Result<R, WriteError> {
        // Supplemental geometry can supply raw records without going through instance.
        let mut too_long = None;
        p21::visit_attributes(&e, &mut |a| {
            if let A::String(text) = a {
                let size = super::text::literal_len(text);
                if size > MAX_STRING_LITERAL_BYTES {
                    too_long = Some(size);
                }
            }
        });
        if let Some(size) = too_long {
            return Err(P21Error::Encode(format!(
                "generated record {} has an indivisible string of {size} encoded bytes (limit {MAX_STRING_LITERAL_BYTES}); cannot split this schema attribute losslessly",
                self.count,
            )).into());
        }
        let occt_stopping = occt_stopping_record(&e);
        let mut rec = Record::new(e);
        if !stated.is_empty() {
            // All REALs in attribute order, including equal values with different text.
            let texts = stated
                .iter()
                .map(|d| real_text(d).map_err(WriteError::Internal))
                .collect::<Result<Vec<_>, _>>()?;
            rec = rec.state_reals(&texts.iter().map(String::as_str).collect::<Vec<_>>())?;
        }
        let id = self.edit.add(rec);
        if id.index() != self.count {
            return Err(WriteError::Internal(format!(
                "addition {} numbered {}",
                self.count,
                id.index()
            )));
        }
        self.count += 1;
        if occt_stopping {
            self.occt_stopping.insert(R::New(id));
        }
        Ok(R::New(id))
    }

    /// The attribute values of one entity's own or inherited slots, in order, from `vals`
    /// keyed by `(declaring entity, attribute)` (or `("", attribute)` for any entity):
    /// unset when not given and optional, derived when redeclared so, refused when missing.
    fn place(
        entity: &str,
        slots: &[(&'static str, &'static str, bool, bool)],
        vals: &[((&str, &str), A)],
    ) -> Result<Vec<A>, WriteError> {
        let mut used = vec![false; vals.len()];
        let mut out = Vec::new();
        for &(decl, name, optional, derived) in slots {
            if derived {
                out.push(A::Derived);
                continue;
            }
            let found = vals.iter().enumerate().position(|(i, ((e, n), _))| {
                !used[i] && *n == name && (e.is_empty() || *e == decl)
            });
            match found {
                Some(i) => {
                    used[i] = true;
                    out.push(vals[i].1.clone());
                }
                None if optional => out.push(A::Unset),
                None => {
                    return Err(WriteError::Internal(format!(
                        "{entity}: no value for {decl}.{name}"
                    )));
                }
            }
        }
        if let Some(i) = used.iter().position(|u| !u) {
            return Err(WriteError::Internal(format!(
                "{entity}: {:?} is not one of its attributes",
                vals[i].0
            )));
        }
        Ok(out)
    }

    /// An instance of the types `types` (each with all its supertypes implied): a simple record
    /// when one of them is a subtype of all the others (internal mapping), otherwise a complex
    /// record of the supertype closure (external mapping, leaves sorted by [`Complex`]).
    fn instance(
        &mut self,
        types: &[&str],
        vals: &[((&str, &str), A)],
        stated: &[&Decimal],
    ) -> Result<R, WriteError> {
        // Every descriptive payload (JSON ordering, notes, UDA, material and
        // parameter text) passes here, including callers outside text_item.
        if types == ["descriptive_representation_item"]
            && let Some((_, A::String(text))) = vals.iter().find(|((_, n), _)| *n == "description")
            && super::text::literal_len(text) > MAX_STRING_LITERAL_BYTES
        {
            let name = vals
                .iter()
                .find(|((_, n), _)| *n == "name")
                .map(|(_, a)| a.clone())
                .unwrap_or_else(|| s(""));
            let mut items = Vec::new();
            for chunk in super::text::split(text) {
                items.push(self.simple(
                    "descriptive_representation_item",
                    &[
                        ("name", s(super::text::CHUNK_NAME)),
                        ("description", s(chunk)),
                    ],
                )?);
            }
            return self.simple(
                "compound_representation_item",
                &[
                    ("name", name),
                    (
                        "item_element",
                        typed("list_representation_item", refs(items)),
                    ),
                ],
            );
        }
        let record = self.entity_record(types, vals)?;
        self.push(record, stated)
    }

    /// The typed mapping for an added record.
    fn entity_record(
        &self,
        types: &[&str],
        vals: &[((&str, &str), A)],
    ) -> Result<RawEntity, WriteError> {
        let ed = self.edition;
        let closure = |n: &str| -> BTreeSet<&'static str> {
            let mut out = BTreeSet::new();
            let mut stack: Vec<&'static str> = ed.entity(n).map(|e| e.name).into_iter().collect();
            while let Some(x) = stack.pop() {
                if out.insert(x)
                    && let Some(e) = ed.entity(x)
                {
                    stack.extend(e.supertypes.iter().copied());
                }
            }
            out
        };
        for t in types {
            if ed.entity(t).is_none() {
                return Err(WriteError::Internal(format!(
                    "{t} is not an entity of {}",
                    ed.file_schema
                )));
            }
        }
        let all: BTreeSet<&'static str> = types.iter().flat_map(|t| closure(t)).collect();
        if let Some(top) = types.iter().find(|t| closure(t) == all) {
            let slots: Vec<_> = ed
                .explicit_attributes(top)
                .unwrap_or_default()
                .into_iter()
                .map(|s| (s.entity, s.name, s.optional, s.derived))
                .collect();
            let attributes = Self::place(top, &slots, vals)?;
            return Ok(p21::simple(&top.to_ascii_uppercase(), attributes));
        }
        // Complex: each leaf its own attributes; one redeclared as derived by any leaf is `*`.
        let mut leaves = Vec::new();
        let mut remaining: Vec<((&str, &str), A)> = vals.to_vec();
        for leaf in &all {
            let decl = ed.entity(leaf).ok_or_else(|| {
                WriteError::Internal(format!("{leaf} is not in the EXPRESS table"))
            })?;
            let slots: Vec<_> = decl
                .attributes
                .iter()
                .map(|a| {
                    let derived = all.iter().filter_map(|x| ed.entity(x)).any(|x| {
                        x.redeclared
                            .iter()
                            .any(|r| r.entity == *leaf && r.attribute == a.name && r.derived)
                    });
                    (decl.name, a.name, a.optional, derived)
                })
                .collect();
            let mine: Vec<((&str, &str), A)> = remaining
                .iter()
                .filter(|((e, n), _)| {
                    (*e == *leaf || e.is_empty()) && decl.attributes.iter().any(|a| a.name == *n)
                })
                .cloned()
                .collect();
            remaining.retain(|((e, n), _)| {
                !((*e == *leaf || e.is_empty()) && decl.attributes.iter().any(|a| a.name == *n))
            });
            leaves.push(p21::leaf(
                &leaf.to_ascii_uppercase(),
                Self::place(leaf, &slots, &mine)?,
            ));
        }
        if let Some(((e, n), _)) = remaining.first() {
            return Err(WriteError::Internal(format!(
                "{types:?}: {e}.{n} is not one of its attributes"
            )));
        }
        let complex = Complex::new(leaves)?;
        Ok(complex.into_record())
    }

    /// A simple instance of `entity`, attributes by name.
    fn simple(&mut self, entity: &str, vals: &[(&str, A)]) -> Result<R, WriteError> {
        let v: Vec<((&str, &str), A)> = vals.iter().map(|(n, a)| (("", *n), a.clone())).collect();
        self.instance(&[entity], &v, &[])
    }

    fn reference(&self, id: u64) -> Result<R, WriteError> {
        if self.removed.contains(&id) {
            return Err(WriteError::Internal(format!(
                "#{id} is removed by the edit but referenced by what it adds"
            )));
        }
        Ok(R::Old(id))
    }

    // --- units -----------------------------------------------------------------------------

    fn dimensional_exponents(&mut self, kind: &'static str) -> Result<R, WriteError> {
        if let Some(r) = self.exponents.get(kind) {
            return Ok(*r);
        }
        let length = if kind == "length" { 1.0 } else { 0.0 };
        let mass = if kind == "mass" { 1.0 } else { 0.0 };
        let r = self.simple(
            "dimensional_exponents",
            &[
                ("length_exponent", A::Real(length)),
                ("mass_exponent", A::Real(mass)),
                ("time_exponent", A::Real(0.0)),
                ("electric_current_exponent", A::Real(0.0)),
                ("thermodynamic_temperature_exponent", A::Real(0.0)),
                ("amount_of_substance_exponent", A::Real(0.0)),
                ("luminous_intensity_exponent", A::Real(0.0)),
            ],
        )?;
        self.exponents.insert(kind, r);
        Ok(r)
    }

    /// UDA v1.8 Annex C.1/C.4.2: only kilogram is an SI mass unit;
    /// gram and milligram are conversion-based named units, preserving the density value.
    fn mass_unit(&mut self, prefix: Option<&str>) -> Result<R, WriteError> {
        let kilogram = self.instance(
            &["mass_unit", "si_unit"],
            &[
                (("si_unit", "prefix"), en("kilo")),
                (("si_unit", "name"), en("gram")),
            ],
            &[],
        )?;
        if prefix == Some("kilo") {
            return Ok(kilogram);
        }
        let (name, factor) = if prefix == Some("milli") {
            ("milligram", "0.000001")
        } else {
            ("gram", "0.001")
        };
        let factor = Decimal::parse(factor).map_err(|e| WriteError::Internal(e.0))?;
        let measure = self.instance(
            &["mass_measure_with_unit"],
            &[
                (
                    ("", "value_component"),
                    typed("mass_measure", real(&factor).map_err(WriteError::Internal)?),
                ),
                (("", "unit_component"), kilogram.a()),
            ],
            &[&factor],
        )?;
        let dimensions = self.dimensional_exponents("mass")?;
        self.instance(
            &["conversion_based_unit", "mass_unit"],
            &[
                (("conversion_based_unit", "name"), s(name)),
                (("conversion_based_unit", "conversion_factor"), measure.a()),
                (("named_unit", "dimensions"), dimensions.a()),
            ],
            &[],
        )
    }

    /// A unit instance equal to `unit`: one of `context`'s units, else one added (SI with its
    /// prefix, or a conversion based unit with its exact factor).
    pub fn length_unit(
        &mut self,
        context: u64,
        unit: &LengthUnit,
    ) -> Result<LengthUnitRef, WriteError> {
        if let Some((_, r)) = self.length_units.iter().find(|(u, _)| same_length(u, unit)) {
            return Ok(*r);
        }
        let found = attr_refs(self.doc, context, "global_unit_assigned_context", "units")
            .into_iter()
            .find(|&u| {
                matches!(doc_unit(self.doc, u, 0), Some(DocUnit::Length(l)) if same_length(&l, unit))
            });
        let r = match found {
            Some(u) => LengthUnitRef(self.reference(u)?),
            None => {
                let si = |prefix: Option<&str>| {
                    vec![
                        (("si_unit", "prefix"), prefix.map_or(A::Unset, en)),
                        (("si_unit", "name"), en("metre")),
                    ]
                };
                let r = match unit {
                    LengthUnit::Millimetre => {
                        self.instance(&["length_unit", "si_unit"], &si(Some("milli")), &[])?
                    }
                    LengthUnit::Micrometre => {
                        self.instance(&["length_unit", "si_unit"], &si(Some("micro")), &[])?
                    }
                    LengthUnit::Centimetre => {
                        self.instance(&["length_unit", "si_unit"], &si(Some("centi")), &[])?
                    }
                    LengthUnit::Metre => {
                        self.instance(&["length_unit", "si_unit"], &si(None), &[])?
                    }
                    LengthUnit::Inch | LengthUnit::Foot | LengthUnit::Other { .. } => {
                        let (name, factor, base) = match unit {
                            LengthUnit::Inch => {
                                ("INCH".to_string(), "25.4", LengthUnit::Millimetre)
                            }
                            LengthUnit::Foot => {
                                ("FOOT".to_string(), "304.8", LengthUnit::Millimetre)
                            }
                            LengthUnit::Other { name, metres } => {
                                (name.clone(), metres.as_str(), LengthUnit::Metre)
                            }
                            #[allow(clippy::unreachable, reason = "the base units match above")]
                            _ => unreachable!("matched above"),
                        };
                        let factor =
                            Decimal::parse(factor).map_err(|e| WriteError::Internal(e.0))?;
                        let base_unit = base;
                        let base = self.length_unit(context, &base_unit)?;
                        let f = self.length_measure(
                            &Length {
                                value: factor,
                                unit: base_unit,
                            },
                            base,
                        )?;
                        let dims = self.dimensional_exponents("length")?;
                        self.instance(
                            &["conversion_based_unit", "length_unit"],
                            &[
                                (("named_unit", "dimensions"), dims.a()),
                                (("conversion_based_unit", "name"), s(&name)),
                                (("conversion_based_unit", "conversion_factor"), f.a()),
                            ],
                            &[],
                        )?
                    }
                };
                LengthUnitRef(r)
            }
        };
        self.length_units.push((unit.clone(), r));
        Ok(r)
    }

    /// A plane angle unit instance equal to `unit`, found in `context` or added.
    pub fn angle_unit(
        &mut self,
        context: u64,
        unit: &AngleUnit,
    ) -> Result<AngleUnitRef, WriteError> {
        if let Some((_, r)) = self.angle_units.iter().find(|(u, _)| same_angle(u, unit)) {
            return Ok(*r);
        }
        let found = attr_refs(self.doc, context, "global_unit_assigned_context", "units")
            .into_iter()
            .find(|&u| {
                matches!(doc_unit(self.doc, u, 0), Some(DocUnit::Angle(a)) if same_angle(&a, unit))
            });
        let r = match found {
            Some(u) => AngleUnitRef(self.reference(u)?),
            None => {
                let r = match unit {
                    AngleUnit::Radian => self.instance(
                        &["plane_angle_unit", "si_unit"],
                        &[
                            (("si_unit", "prefix"), A::Unset),
                            (("si_unit", "name"), en("radian")),
                        ],
                        &[],
                    )?,
                    AngleUnit::Degree | AngleUnit::Other { .. } => {
                        let (name, factor) = match unit {
                            AngleUnit::Degree => (
                                "DEGREE".to_string(),
                                Decimal::parse(&p21::encode_real(std::f64::consts::PI / 180.0)?)
                                    .map_err(|e| WriteError::Internal(e.0))?,
                            ),
                            AngleUnit::Other { name, radians } => (name.clone(), radians.clone()),
                            #[allow(clippy::unreachable, reason = "radians match above")]
                            AngleUnit::Radian => unreachable!("matched above"),
                        };
                        let base = self.angle_unit(context, &AngleUnit::Radian)?;
                        let f = self.angle_measure(
                            &Angle {
                                value: factor,
                                unit: AngleUnit::Radian,
                            },
                            base,
                        )?;
                        let dims = self.dimensional_exponents("none")?;
                        self.instance(
                            &["conversion_based_unit", "plane_angle_unit"],
                            &[
                                (("named_unit", "dimensions"), dims.a()),
                                (("conversion_based_unit", "name"), s(&name)),
                                (("conversion_based_unit", "conversion_factor"), f.a()),
                            ],
                            &[],
                        )?
                    }
                };
                AngleUnitRef(r)
            }
        };
        self.angle_units.push((unit.clone(), r));
        Ok(r)
    }

    /// A dimensionless unit: a ratio unit of the context, else one added.
    fn ratio_unit(&mut self, context: u64) -> Result<R, WriteError> {
        if let Some(r) = self.other_units.get("ratio") {
            return Ok(*r);
        }
        let found = attr_refs(self.doc, context, "global_unit_assigned_context", "units")
            .into_iter()
            .find(|&u| is_a(self.doc, u, "ratio_unit"));
        let r = match found {
            Some(u) => self.reference(u)?,
            None => {
                // A ratio unit with a name of its own (context dependent): the reader resolves
                // a named unit by its leaves, and a lone RATIO_UNIT's internal mapping names
                // none it knows.
                let dims = self.dimensional_exponents("none")?;
                self.instance(
                    &["context_dependent_unit", "ratio_unit"],
                    &[
                        (("named_unit", "dimensions"), dims.a()),
                        (("context_dependent_unit", "name"), s("ratio")),
                    ],
                    &[],
                )?
            }
        };
        self.other_units.insert("ratio".into(), r);
        Ok(r)
    }

    /// The unit of a count (a context dependent unit 'count': ISO 10303-41 has no count unit).
    fn count_unit(&mut self) -> Result<R, WriteError> {
        if let Some(r) = self.other_units.get("count") {
            return Ok(*r);
        }
        let dims = self.dimensional_exponents("none")?;
        let r = self.instance(
            &["context_dependent_unit"],
            &[
                (("named_unit", "dimensions"), dims.a()),
                (("context_dependent_unit", "name"), s("count")),
            ],
            &[],
        )?;
        self.other_units.insert("count".into(), r);
        Ok(r)
    }

    // --- measures --------------------------------------------------------------------------

    /// `LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(v), #u)`, `v` written as stated.
    pub fn length_measure(&mut self, l: &Length, unit: LengthUnitRef) -> Result<R, WriteError> {
        let v = real(&l.value).map_err(WriteError::Internal)?;
        self.instance(
            &["length_measure_with_unit"],
            &[
                (("", "value_component"), typed("length_measure", v)),
                (("", "unit_component"), unit.0.a()),
            ],
            &[&l.value],
        )
    }

    /// `PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(v), #u)`, `v` written as stated.
    pub fn angle_measure(&mut self, a: &Angle, unit: AngleUnitRef) -> Result<R, WriteError> {
        let v = real(&a.value).map_err(WriteError::Internal)?;
        self.instance(
            &["plane_angle_measure_with_unit"],
            &[
                (("", "value_component"), typed("plane_angle_measure", v)),
                (("", "unit_component"), unit.0.a()),
            ],
            &[&a.value],
        )
    }

    /// A value as a measure with unit, its §5.4 decimal places by a `measure_qualification`.
    fn value(&mut self, context: u64, v: &Value) -> Result<R, WriteError> {
        let m = match &v.quantity {
            Quantity::Length(l) => {
                let u = self.length_unit(context, &l.unit)?;
                self.length_measure(l, u)?
            }
            Quantity::Angle(a) => {
                let u = self.angle_unit(context, &a.unit)?;
                self.angle_measure(a, u)?
            }
        };
        if let Some(p) = v.decimal_places {
            let q = self.format_qualifier(v.decimal(), p)?;
            self.simple(
                "measure_qualification",
                &[
                    ("name", s("")),
                    ("description", s("")),
                    ("qualified_measure", m.a()),
                    ("qualifiers", refs([q])),
                ],
            )?;
        }
        Ok(m)
    }

    /// `VALUE_FORMAT_TYPE_QUALIFIER('NR2 x.y')` (§5.4): `x` the value's integer digits, `y`
    /// its decimal places.
    fn format_qualifier(&mut self, d: &Decimal, places: u8) -> Result<R, WriteError> {
        let int = d
            .as_str()
            .trim_start_matches(['+', '-'])
            .split(['.', 'E', 'e'])
            .next()
            .unwrap_or("")
            .trim_start_matches('0')
            .len()
            .max(1);
        self.simple(
            "value_format_type_qualifier",
            &[("format_type", s(&format!("NR2 {int}.{places}")))],
        )
    }

    /// A named measure representation item of a length, angle, ratio or count, with
    /// qualifiers: the typed leaf (`LENGTH_MEASURE_WITH_UNIT`, …) is part of the instance.
    fn measure_item(
        &mut self,
        name: &str,
        m: MeasureItem<'_>,
        context: u64,
        mut qualifiers: Vec<R>,
        places: Option<u8>,
    ) -> Result<R, WriteError> {
        let (leaf, value_type, decimal, unit) = match m {
            MeasureItem::Length(l) => (
                Some("length_measure_with_unit"),
                "length_measure",
                &l.value,
                self.length_unit(context, &l.unit)?.0,
            ),
            MeasureItem::Angle(a) => (
                Some("plane_angle_measure_with_unit"),
                "plane_angle_measure",
                &a.value,
                self.angle_unit(context, &a.unit)?.0,
            ),
            MeasureItem::Ratio(r) => (
                Some("ratio_measure_with_unit"),
                "ratio_measure",
                r,
                self.ratio_unit(context)?,
            ),
            MeasureItem::Count(c) => (None, "count_measure", c, self.count_unit()?),
        };
        if let Some(p) = places {
            qualifiers.push(self.format_qualifier(decimal, p)?);
        }
        let integer = (value_type == "count_measure")
            .then(|| count_integer(decimal))
            .flatten();
        let v = match integer {
            Some(i) => A::Integer(i),
            None => real(decimal).map_err(WriteError::Internal)?,
        };
        let mut types = vec!["measure_representation_item"];
        types.extend(leaf);
        if !qualifiers.is_empty() {
            types.push("qualified_representation_item");
        }
        let mut vals = vec![
            (("", "name"), s(name)),
            (("", "value_component"), typed(value_type, v)),
            (("", "unit_component"), unit.a()),
        ];
        if !qualifiers.is_empty() {
            vals.push((("", "qualifiers"), refs(qualifiers)));
        }
        let stated = if integer.is_some() {
            Vec::new()
        } else {
            vec![decimal]
        };
        self.instance(&types, &vals, &stated)
    }

    /// `TOLERANCE_VALUE(lower_bound, upper_bound)` from deviations: signed offsets from the
    /// nominal, `upper > lower` (the only way a tolerance value is made).
    fn tolerance_value(&mut self, context: u64, deviations: &Bounds) -> Result<R, WriteError> {
        let lower = self.value(context, deviations.lower())?;
        let upper = self.value(context, deviations.upper())?;
        self.simple(
            "tolerance_value",
            &[("lower_bound", lower.a()), ("upper_bound", upper.a())],
        )
    }

    /// `LIMITS_AND_FITS(form_variance, zone_variance, grade, source)` from an ISO 286 class
    /// (§5.2.5): the deviation's letters, 'hole' or 'shaft' by their case, the grade's number.
    fn limits_and_fits(&mut self, class: &Iso286Class) -> Result<R, WriteError> {
        let zone = match class.deviation.of {
            FitFeature::Hole => "hole",
            FitFeature::Shaft => "shaft",
        };
        self.simple(
            "limits_and_fits",
            &[
                ("form_variance", s(&class.deviation.to_string())),
                ("zone_variance", s(zone)),
                ("grade", s(class.grade.number())),
                ("source", s("")),
            ],
        )
    }
}

fn same_length(a: &LengthUnit, b: &LengthUnit) -> bool {
    match (a, b) {
        (LengthUnit::Other { name: n, metres: m }, LengthUnit::Other { name: o, metres: p }) => {
            n == o && m.cmp_value(p).is_eq()
        }
        _ => a == b,
    }
}

fn same_angle(a: &AngleUnit, b: &AngleUnit) -> bool {
    match (a, b) {
        (
            AngleUnit::Other {
                name: n,
                radians: m,
            },
            AngleUnit::Other {
                name: o,
                radians: p,
            },
        ) => n == o && m.cmp_value(p).is_eq(),
        _ => a == b,
    }
}

/// What a measure representation item holds.
#[derive(Clone, Copy)]
enum MeasureItem<'v> {
    Length(&'v Length),
    Angle(&'v Angle),
    Ratio(&'v Decimal),
    Count(&'v Decimal),
}

fn measure_of_value(v: &Value) -> MeasureItem<'_> {
    match &v.quantity {
        Quantity::Length(l) => MeasureItem::Length(l),
        Quantity::Angle(a) => MeasureItem::Angle(a),
    }
}

// ---------------------------------------------------------------------------------------------
// One part: the plan (checks, roles, reuse) and its emission
// ---------------------------------------------------------------------------------------------

/// A feature's role, which decides its entity type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Role {
    Plain,
    DatumFeature,
    /// The datum feature is also the one size dimension on it (§6.5.3).
    DatumFeatureOfSize(usize),
    AppliedArea,
    ThreadRunout,
}

struct PartPlan<'a> {
    #[cfg(all(feature = "json-import", feature = "presentation"))]
    source: source_presentation::Placements,
    #[cfg(feature = "json-import")]
    identifiers: Option<&'a BTreeMap<String, Vec<String>>>,
    doc: &'a Document,
    def: &'a PartDefinition,
    id: PartId,
    pmi: &'a PartPmi,
    mode: Mode,
    refusals: Vec<Refusal>,
    /// The part's shape representation and its context.
    shape_rep: u64,
    context: u64,
    /// Feature → the feature written for it (features of equal content are one instance).
    canonical: Vec<usize>,
    roles: BTreeMap<usize, Role>,
    /// Features that are also an applied area or a thread runout beside their own role: a
    /// second instance of that type.
    twins: BTreeSet<(usize, Role)>,
    /// Features something refers to (other than as an area or a runout).
    referenced: BTreeSet<usize>,
    /// Datums resolved to the part's existing datum (add): datum → (`#datum`, `#datum_feature`).
    existing_datums: BTreeMap<usize, (u64, Option<u64>)>,
    /// Geometry → an existing supplemental geometry item with the same value.
    existing_geometry: BTreeMap<usize, u64>,
    kept_groups: BTreeMap<u64, String>,
    kept_occt_carriers: BTreeSet<u64>,
    /// Existing datum system names on the part (kept ones).
    system_names: BTreeSet<String>,
}

impl<'a> PartPlan<'a> {
    #[cfg(feature = "json-import")]
    fn identify(
        &self,
        em: &mut Emitter<'_>,
        collection: &str,
        index: usize,
        carrier: R,
    ) -> Result<(), WriteError> {
        let Some(ids) = self.identifiers else {
            return Ok(());
        };
        let id = ids
            .get(collection)
            .and_then(|v| v.get(index))
            .ok_or_else(|| WriteError::Internal(format!("missing JSON id {collection}/{index}")))?;
        if collection == "tolerance_relations" {
            // geometric_tolerance_relationship is not in id_attribute_select.
            // AP242 ed4 identification_item explicitly includes it.
            let role = em.simple(
                "identification_role",
                &[
                    ("name", s("haecceity.pmi/1 item id")),
                    ("description", A::Unset),
                ],
            )?;
            em.simple(
                "applied_identification_assignment",
                &[
                    ("assigned_id", s(id)),
                    ("role", role.a()),
                    ("items", refs([carrier])),
                ],
            )?;
        } else {
            em.simple(
                "id_attribute",
                &[("attribute_value", s(id)), ("identified_item", carrier.a())],
            )?;
        }
        Ok(())
    }

    fn new(
        doc: &'a Document,
        def: &'a PartDefinition,
        id: PartId,
        pmi: &'a PartPmi,
        read: &PmiRead,
        mode: Mode,
    ) -> Result<Self, WriteError> {
        let shape_rep = doc
            .referrers(def.shape)
            .iter()
            .copied()
            .filter(|&r| is_a(doc, r, "shape_definition_representation"))
            .filter_map(|r| {
                attr_ref(
                    doc,
                    r,
                    "property_definition_representation",
                    "used_representation",
                )
            })
            .min()
            .ok_or_else(|| {
                WriteError::Parts(format!(
                    "part {} has no shape representation for #{}",
                    id.0, def.shape
                ))
            })?;
        let context =
            attr_ref(doc, shape_rep, "representation", "context_of_items").ok_or_else(|| {
                WriteError::Parts(format!("shape representation #{shape_rep} has no context"))
            })?;
        let mut plan = PartPlan {
            #[cfg(feature = "json-import")]
            identifiers: None,
            #[cfg(all(feature = "json-import", feature = "presentation"))]
            source: BTreeMap::new(),
            doc,
            def,
            id,
            pmi,
            mode,
            refusals: Vec::new(),
            shape_rep,
            context,
            canonical: (0..pmi.features.len()).collect(),
            roles: BTreeMap::new(),
            twins: BTreeSet::new(),
            referenced: BTreeSet::new(),
            existing_datums: BTreeMap::new(),
            existing_geometry: BTreeMap::new(),
            kept_groups: BTreeMap::new(),
            kept_occt_carriers: BTreeSet::new(),
            system_names: BTreeSet::new(),
        };
        plan.check(read);
        Ok(plan)
    }

    fn refuse(&mut self, item: ItemRef, why: impl Into<String>) {
        self.refusals.push(Refusal {
            part: self.id,
            item,
            why: why.into(),
        });
    }

    fn context_length_unit(&self) -> Option<LengthUnit> {
        attr_refs(
            self.doc,
            self.context,
            "global_unit_assigned_context",
            "units",
        )
        .into_iter()
        .find_map(|u| match doc_unit(self.doc, u, 0) {
            Some(DocUnit::Length(l)) => Some(l),
            _ => None,
        })
    }

    /// Every check that can refuse an item, and the roles and reuse decisions.
    fn check(&mut self, read: &PmiRead) {
        let p = self.pmi;
        let existing = &read.parts[self.id.0];
        let prov = &read.provenance.parts[self.id.0];
        if self.mode == Mode::Add {
            for i in occt_semantic_features(existing) {
                self.kept_occt_carriers
                    .extend(prov.features[i].iter().copied().filter(|id| {
                        is_a(self.doc, *id, "shape_aspect") && !is_a(self.doc, *id, "datum")
                    }));
            }
            let canonical_existing = Keys {
                p: existing,
                stated: true,
            };
            for (i, feature) in existing.features.iter().enumerate() {
                if matches!(feature, Feature::Group { .. }) {
                    for &id in &prov.features[i] {
                        if is_a(self.doc, id, "shape_aspect") && !is_a(self.doc, id, "datum") {
                            self.kept_groups
                                .insert(id, canonical_existing.feature(FeatureId(i)));
                        }
                    }
                }
            }
        }

        self.refusals.extend(model_refusals(self.id, p));
        for (i, t) in p.datum_targets.iter().enumerate() {
            if let Some(placement) = t.placement() {
                let unit = self.context_length_unit();
                if placement
                    .origin
                    .iter()
                    .any(|v| Some(&v.unit) != unit.as_ref())
                {
                    self.refuse(ItemRef::DatumTarget(i), "target placement coordinates must use the part context length unit (§6.6.1); no coordinate conversion is guessed");
                }
            }
        }
        // Features: anchors on the part, geometry, groups.
        let ctx_unit = self.context_length_unit();
        let existing_geometry: Vec<(&Geometry, u64)> = existing
            .geometry
            .iter()
            .zip(&prov.geometry_items)
            .map(|(g, &id)| (g, id))
            .collect();
        for (gi, g) in p.geometry.iter().enumerate() {
            if let Some(&(_, id)) = existing_geometry
                .iter()
                .find(|(e, id)| *e == g && geometry_rep(self.doc, *id).is_some())
            {
                self.existing_geometry.insert(gi, id);
            }
        }
        for (i, f) in p.features.iter().enumerate() {
            match f {
                Feature::Items(items) => {
                    for a in items {
                        match *a {
                            Anchor::Face(x) if x.0 >= self.def.faces.len() => self.refuse(
                                ItemRef::Feature(i),
                                format!(
                                    "face {} is not a face of the part ({} faces)",
                                    x.0,
                                    self.def.faces.len()
                                ),
                            ),
                            Anchor::Edge(x) if x.0 >= self.def.edges.len() => self.refuse(
                                ItemRef::Feature(i),
                                format!(
                                    "edge {} is not an edge of the part ({} edges)",
                                    x.0,
                                    self.def.edges.len()
                                ),
                            ),
                            Anchor::Geometry(g) if !self.existing_geometry.contains_key(&g.0) => {
                                let geo = &p.geometry[g.0];
                                if geo.length_unit.is_some() && geo.length_unit != ctx_unit {
                                    self.refuse(
                                        ItemRef::Feature(i),
                                        format!(
                                            "supplemental geometry {} is in {:?}, the part's context in {:?}",
                                            g.0, geo.length_unit, ctx_unit
                                        ),
                                    );
                                }
                                if let Some(why) = geometry_values(&geo.item).err() {
                                    self.refuse(ItemRef::Feature(i), why);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Feature::Derived {
                    geometry: Some(g), ..
                } => {
                    let g = &p.derived_geometry[g.0];
                    for face in &g.deriving_faces {
                        if face.0 >= self.def.faces.len() {
                            self.refuse(
                                ItemRef::Feature(i),
                                format!("derived face {} is not a face of the part", face.0),
                            );
                        }
                    }
                    for supplemental in &g.supplemental {
                        if !self.existing_geometry.contains_key(&supplemental.0) {
                            let geo = &p.geometry[supplemental.0];
                            if geo.length_unit.is_some() && geo.length_unit != ctx_unit {
                                self.refuse(ItemRef::Feature(i), "derived supplemental geometry has a different length unit from the part context");
                            }
                            if let Err(why) = geometry_values(&geo.item) {
                                self.refuse(ItemRef::Feature(i), why);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Features of equal content are one instance (one usage per item and representation,
        // item_identified_representation_usage UR1).
        let mut by_content: BTreeMap<Vec<Anchor>, usize> = BTreeMap::new();
        for (i, f) in p.features.iter().enumerate() {
            if let Feature::Items(items) = f {
                let mut key = items.clone();
                key.sort();
                key.dedup();
                let first = *by_content.entry(key).or_insert(i);
                self.canonical[i] = first;
            }
        }

        // Equal scoped groups also share their semantic carrier (including a datum
        // feature); sharing only their leaves would leave duplicate datum endpoints.
        let keys = Keys { p, stated: true };
        let mut groups = BTreeMap::new();
        for (i, feature) in p.features.iter().enumerate() {
            if matches!(feature, Feature::Group { .. }) {
                self.canonical[i] = *groups.entry(keys.feature(FeatureId(i))).or_insert(i);
            }
        }

        // Roles.
        let mut roles: BTreeMap<usize, BTreeSet<Role>> = BTreeMap::new();
        for d in &p.datums {
            if let Some(f) = d.feature() {
                roles
                    .entry(self.canonical[f.0])
                    .or_default()
                    .insert(Role::DatumFeature);
            }
        }
        for t in &p.threads {
            if let Some(f) = t.partial_area {
                roles
                    .entry(self.canonical[f.0])
                    .or_default()
                    .insert(Role::AppliedArea);
            }
            if let Some(f) = t.runout {
                roles
                    .entry(self.canonical[f.0])
                    .or_default()
                    .insert(Role::ThreadRunout);
            }
        }
        for k in &p.knurls {
            roles
                .entry(self.canonical[k.feature.0])
                .or_default()
                .insert(Role::AppliedArea);
        }
        let twin_role = |r: &Role| matches!(r, Role::AppliedArea | Role::ThreadRunout);
        for (f, rs) in &roles {
            // A datum on a group whose members it applies to individually is the group and
            // the datum feature in one complex instance (§6.5.2); no other group or derived
            // feature takes a role.
            let group_datum = rs.len() == 1
                && rs.contains(&Role::DatumFeature)
                && matches!(
                    p.features[*f],
                    Feature::Group {
                        kind: GroupKind::MultipleElements | GroupKind::PatternOfFeatures,
                        ..
                    }
                );
            if !matches!(p.features[*f], Feature::Items(_)) && !group_datum {
                self.refuse(
                    ItemRef::Feature(*f),
                    format!("a group or derived feature used as {rs:?}"),
                );
            }
        }
        // Features something refers to other than as a thread's or knurl's area or a thread's
        // runout.
        let mut referenced: BTreeSet<usize> = BTreeSet::new();
        {
            let mut add = |f: FeatureId| {
                referenced.insert(self.canonical[f.0]);
            };
            for d in &p.datums {
                d.feature().into_iter().for_each(&mut add);
            }
            for d in &p.dimensions {
                match &d.kind {
                    DimensionKind::Size { feature, path, .. } => {
                        add(*feature);
                        path.iter().copied().for_each(&mut add);
                    }
                    DimensionKind::Location { from, to, path, .. } => {
                        add(*from);
                        add(*to);
                        path.iter().copied().for_each(&mut add);
                    }
                }
            }
            for t in &p.tolerances {
                match t.target {
                    ToleranceTarget::Feature(f) => add(f),
                    ToleranceTarget::Relation {
                        relating, related, ..
                    } => {
                        add(relating);
                        add(related);
                    }
                    _ => {}
                }
                if let Some(z) = &t.zone {
                    z.projected.iter().filter_map(|x| x.end).for_each(&mut add);
                    z.non_uniform.iter().flatten().copied().for_each(&mut add);
                    z.affected_plane.into_iter().for_each(&mut add);
                }
            }
            for f in &p.features {
                if let Feature::Group { members, .. } | Feature::Derived { from: members, .. } = f {
                    members.iter().copied().for_each(&mut add);
                }
            }
            for th in &p.threads {
                add(th.feature);
            }
            for k in &p.knurls {
                add(k.feature);
            }
            for a in &p.attributes {
                if let Some(NoteOwner::Feature(f)) = a.on {
                    add(f);
                }
            }
            for s in &p.surface_textures {
                s.on.into_iter().for_each(&mut add);
            }
        }
        for (f, rs) in roles {
            // A thread's or knurl's area and a thread's runout are shape aspects of their own
            // types (applied_area, thread_runout; thread WR12, WR16). Where the same faces are
            // also a feature in their own right (the threaded faces, a datum feature), that type
            // is a second instance composed of the feature (§6.5.2, UR1), so that what states
            // the feature keeps the plain shape aspect or datum feature.
            let primary = rs.iter().find(|r| !twin_role(r)).copied();
            let mut twins: Vec<Role> = rs.iter().filter(|r| twin_role(r)).copied().collect();
            let primary = match primary {
                Some(r) => r,
                None if referenced.contains(&f) => Role::Plain,
                None => twins.remove(0),
            };
            self.roles.insert(f, primary);
            for r in twins {
                self.twins.insert((f, r));
            }
        }
        self.referenced = referenced;
        // A datum feature with exactly one size dimension on it, nothing else stated of it
        // as a feature, is that dimension (§6.5.3).
        let datum_features: Vec<usize> = self
            .roles
            .iter()
            .filter(|(_, r)| **r == Role::DatumFeature)
            .map(|(f, _)| *f)
            .collect();
        for f in datum_features {
            let sizes: Vec<usize> = p
                .dimensions
                .iter()
                .enumerate()
                .filter(|(_, d)| matches!(&d.kind, DimensionKind::Size { feature, path: None, angle: None, .. } if self.canonical[feature.0] == f))
                .map(|(i, _)| i)
                .collect();
            let stated_as_feature = p.tolerances.iter().any(
                |t| matches!(t.target, ToleranceTarget::Feature(x) if self.canonical[x.0] == f),
            ) || p
                .attributes
                .iter()
                .any(|a| matches!(a.on, Some(NoteOwner::Feature(x)) if self.canonical[x.0] == f))
                || p.surface_textures
                    .iter()
                    .any(|s| matches!(s.on, Some(x) if self.canonical[x.0] == f));
            if let [d] = sizes.as_slice()
                && !stated_as_feature
            {
                self.roles.insert(f, Role::DatumFeatureOfSize(*d));
            }
        }

        // Datums: one per label per part; add resolves an existing label.
        if self.mode == Mode::Add {
            let wanted = Keys { p, stated: true };
            let kept = Keys {
                p: existing,
                stated: true,
            };
            for (i, d) in p.datums.iter().enumerate() {
                if let Some(f) = d.feature()
                    && let Some(other) = existing.datums.iter().find(|other| {
                        other.label() != d.label()
                            && other
                                .feature()
                                .is_some_and(|g| kept.feature(g) == wanted.feature(f))
                    })
                {
                    self.refuse(ItemRef::Datum(i), format!("datum feature already establishes {} (datum_feature.WR1 permits exactly one datum)",other.label()));
                }
            }
        }
        for (i, d) in p.datums.iter().enumerate() {
            if self.mode != Mode::Add {
                continue;
            }
            let Some(e) = existing.datums.iter().position(|x| x.label() == d.label()) else {
                continue;
            };
            let old = &existing.datums[e];
            let same = match (old.feature(), d.feature()) {
                (Some(a), Some(b)) => {
                    let set = |f: &Feature| match f {
                        Feature::Items(items) => {
                            let mut v = items.clone();
                            v.sort();
                            v.dedup();
                            Some(v)
                        }
                        _ => None,
                    };
                    old.targets().is_empty()
                        && set(&existing.features[a.0]).is_some()
                        && set(&existing.features[a.0]) == set(&p.features[b.0])
                }
                _ => false,
            };
            if !same {
                self.refuse(
                    ItemRef::Datum(i),
                    format!("the part already has datum {} on other faces (one datum per label per part)", d.label()),
                );
                continue;
            }
            let datum = prov.datums[e]
                .iter()
                .copied()
                .find(|&x| is_a(self.doc, x, "datum"));
            let feature = datum.and_then(|dx| {
                self.doc.referrers(dx).iter().copied().find_map(|r| {
                    (is_a(self.doc, r, "shape_aspect_relationship")
                        && attr_ref(
                            self.doc,
                            r,
                            "shape_aspect_relationship",
                            "related_shape_aspect",
                        ) == Some(dx))
                    .then(|| {
                        attr_ref(
                            self.doc,
                            r,
                            "shape_aspect_relationship",
                            "relating_shape_aspect",
                        )
                    })
                    .flatten()
                    .filter(|&f| is_a(self.doc, f, "datum_feature"))
                })
            });
            match (datum, feature) {
                (Some(dx), Some(fx)) => {
                    self.existing_datums.insert(i, (dx, Some(fx)));
                }
                _ => self.refuse(
                    ItemRef::Datum(i),
                    format!(
                        "the part's datum {} could not be resolved to its instances",
                        d.label()
                    ),
                ),
            }
        }
        if self.mode == Mode::Add {
            for &r in self.doc.referrers(self.def.shape) {
                if is_a(self.doc, r, "datum_system")
                    && let Some(n) = attr_str(self.doc, r, "shape_aspect", "name")
                {
                    self.system_names.insert(n);
                }
            }
        }

        for (i, t) in p.tolerances.iter().enumerate() {
            let schema = self.doc.file_schema().unwrap_or_default();
            let edition = schema
                .first()
                .and_then(|name| express::edition(name))
                .unwrap_or(express::TARGET);
            let relation_allowed = edition.type_decl("geometric_tolerance_target").is_some_and(|t| matches!(t.def, express::TypeDef::Select(members) if members.contains(&"shape_aspect_relationship")));
            if matches!(t.target, ToleranceTarget::Relation { .. }) && !relation_allowed {
                self.refuse(ItemRef::Tolerance(i), "this AP242 edition's geometric_tolerance_target SELECT excludes shape_aspect_relationship; edition 4 supports it");
            }
        }

        // Requirements on an unwritable feature must name their own refusal as well.
        let refused_features: BTreeMap<_, _> = self
            .refusals
            .iter()
            .filter_map(|r| match r.item {
                ItemRef::Feature(f) => Some((f, r.why.clone())),
                _ => None,
            })
            .collect();
        // Attributes.
        for (i, a) in p.attributes.iter().enumerate() {
            if let Some(NoteOwner::Feature(f)) = a.on
                && let Some(why) = refused_features.get(&f.0)
            {
                self.refuse(
                    ItemRef::Attribute(i),
                    format!("owner feature {} is refused: {why}", f.0),
                );
            }
            for (key, value) in &a.items {
                if let AttributeValue::OtherMeasure { unit, .. } = value
                    && !unit.is_empty()
                    && let Err(why) = self.other_unit(unit)
                {
                    self.refuse(ItemRef::Attribute(i), format!("{key:?}: {why}"));
                }
            }
        }

        // Surface textures.
        for (i, s) in p.surface_textures.iter().enumerate() {
            if let Some(f) = s.on
                && let Some(why) = refused_features.get(&f.0)
            {
                self.refuse(
                    ItemRef::SurfaceTexture(i),
                    format!("owner feature {} is refused: {why}", f.0),
                );
            }
        }
    }

    /// The document's unit the reader names `name` (an attribute's measure of another kind):
    /// exactly one distinct instance, else refused.
    fn other_unit(&self, name: &str) -> Result<u64, String> {
        let found: Vec<u64> = self
            .doc
            .ids()
            .filter(|&id| is_a(self.doc, id, "named_unit") || is_a(self.doc, id, "derived_unit"))
            .filter(|&id| other_unit_name(self.doc, id).as_deref() == Some(name))
            .collect();
        match found.as_slice() {
            [u] => Ok(*u),
            [] => Err(format!("no unit of the file is {name:?}")),
            _ => Err(format!(
                "{} units of the file are named {name:?}; which one is not stated",
                found.len()
            )),
        }
    }

    /// The representation of the part that holds a face or edge (the B-rep's).
    fn rep_of(&self, em: &mut Emitter<'_>, item: u64) -> Result<u64, WriteError> {
        if let Some(&rep) = em.shape_representations.get(&item) {
            return Ok(rep);
        }
        let rep = using_representations(self.doc, item)
            .into_iter()
            .find(|&r| is_a(self.doc, r, "shape_representation"))
            .ok_or_else(|| {
                WriteError::Internal(format!("#{item} is in no shape representation"))
            })?;
        em.shape_representations.insert(item, rep);
        Ok(rep)
    }

    // --- emission --------------------------------------------------------------------------

    fn emit(
        &self,
        em: &mut Emitter<'_>,
        report: &mut WriteReport,
        presentation_reader: Option<&crate::step::PartReader>,
    ) -> Result<(), WriteError> {
        let mut st = Emitted::default();
        let p = self.pmi;
        let pds = em.reference(self.def.shape)?;
        // Replace consumes the selected part's modelled wrappers in its removal
        // plan. Add reports its retained legacy endpoints; scanning the
        // whole source graph here on every Replace needlessly revisits the B-rep.
        if self.mode == Mode::Add {
            self.preserve_legacy_endpoints(em)?;
        }
        let pd = em.reference(self.def.product_definition)?;

        for s in &p.standards {
            self.standard(em, pd, s)?;
        }

        // Supplemental geometry not already in the file: its items, then one constructive
        // geometry representation of the part holding them.
        let mut new_items: Vec<(usize, R)> = Vec::new();
        let mut used_geometry: BTreeSet<usize> = BTreeSet::new();
        for f in &p.features {
            if let Feature::Items(items) = f {
                for a in items {
                    if let Anchor::Geometry(g) = a {
                        used_geometry.insert(g.0);
                    }
                }
            }
        }
        for g in &p.derived_geometry {
            used_geometry.extend(g.supplemental.iter().map(|g| g.0));
        }
        #[cfg(feature = "json-import")]
        if self.identifiers.is_some() {
            used_geometry.extend(0..p.geometry.len());
        }
        let mut memo: HashMap<GeometryItem, R> = HashMap::new();
        for &g in &used_geometry {
            if let Some(&old) = self.existing_geometry.get(&g) {
                st.geometry.insert(g, (em.reference(old)?, None));
                report.reused_geometry.push(old);
            } else {
                let r = geometry_item(em, &p.geometry[g].item, &mut memo)?;
                new_items.push((g, r));
            }
        }
        if !new_items.is_empty() {
            let cgr = em.simple(
                "constructive_geometry_representation",
                &[
                    ("name", s("supplemental geometry")),
                    ("items", {
                        let items: Vec<_> = new_items.iter().map(|(_, r)| *r).collect();
                        #[cfg(feature = "json-import")]
                        let items = if self.identifiers.is_some() {
                            let mut seen = BTreeSet::new();
                            items.into_iter().filter(|r| seen.insert(*r)).collect()
                        } else {
                            items
                        };
                        refs(items)
                    }),
                    ("context_of_items", em.reference(self.context)?.a()),
                ],
            )?;
            em.simple(
                "constructive_geometry_representation_relationship",
                &[
                    ("name", s("supplemental geometry")),
                    ("description", s("")),
                    ("rep_1", em.reference(self.shape_rep)?.a()),
                    ("rep_2", cgr.a()),
                ],
            )?;
            for (g, r) in new_items {
                st.geometry.insert(g, (r, Some(cgr)));
            }
        }

        for (i, t) in p.datum_targets.iter().enumerate() {
            let explicit = matches!(t.shape(), TargetShape::Area(_) | TargetShape::Curve(_));
            let mut types = vec![if explicit {
                "datum_target"
            } else {
                "placed_datum_target_feature"
            }];
            if let TargetShape::Area(f) | TargetShape::Curve(f) = t.shape()
                && matches!(&p.features[self.canonical[f.0]], Feature::Items(items)
                    if items.iter().copied().collect::<BTreeSet<_>>().len() > 1)
            {
                types.push("composite_shape_aspect");
            }
            let target = em.instance(
                &types,
                &[
                    (("shape_aspect", "name"), s("")),
                    (
                        ("shape_aspect", "description"),
                        s(target_shape_name(t.shape())),
                    ),
                    (("shape_aspect", "of_shape"), pds.a()),
                    (("shape_aspect", "product_definitional"), boolean(true)),
                    (("datum_target", "target_id"), s(&t.number().to_string())),
                ],
                &[],
            )?;
            em.practice_conflict(target, &["datum_target.WR5"]);
            st.targets.insert(i, target);
            if let TargetShape::Area(f) | TargetShape::Curve(f) = t.shape() {
                let c = self.canonical[f.0];
                let uses = p.datum_targets.iter().filter(|x| matches!(x.shape(), TargetShape::Area(g) | TargetShape::Curve(g) if self.canonical[g.0] == c)).count();
                if uses == 1
                    && !self.referenced.contains(&c)
                    && !p
                        .datum_targets
                        .iter()
                        .any(|t| t.on().is_some_and(|f| self.canonical[f.0] == c))
                    && !self.roles.contains_key(&c)
                    && matches!(p.features[c], Feature::Items(_))
                {
                    st.features.insert(c, target);
                }
            }
        }

        // Features resolved to an existing datum feature write no usage.
        let reused: BTreeSet<usize> = self
            .existing_datums
            .iter()
            .filter(|(_, (_, fx))| fx.is_some())
            .filter_map(|(d, _)| p.datums[*d].feature())
            .map(|f| self.canonical[f.0])
            .collect();
        let mut keys: BTreeMap<(R, Vec<R>), usize> = BTreeMap::new();
        let mut done = BTreeSet::new();
        for (i, f) in p.features.iter().enumerate() {
            let c = self.canonical[i];
            if let Feature::Items(items) = f
                && done.insert(c)
                && !reused.contains(&c)
            {
                for k in self.usage_keys(em, &st, items)? {
                    *keys.entry(k).or_insert(0) += 1;
                }
            }
        }
        for &(c, _) in &self.twins {
            if let Feature::Items(items) = &p.features[c] {
                for k in self.usage_keys(em, &st, items)? {
                    *keys.entry(k).or_insert(0) += 1;
                }
            }
        }
        for (i, t) in p.datum_targets.iter().enumerate() {
            if let TargetShape::Area(f) | TargetShape::Curve(f) = t.shape() {
                let c = self.canonical[f.0];
                if st.features.get(&c) != st.targets.get(&i)
                    && let Feature::Items(items) = &p.features[c]
                {
                    for k in self.usage_keys(em, &st, items)? {
                        *keys.entry(k).or_insert(0) += 1;
                    }
                }
            }
        }
        let shared: BTreeSet<(R, Vec<R>)> = keys
            .iter()
            .filter(|(_, n)| **n > 1)
            .map(|(k, _)| k.clone())
            .collect();
        let mut done = BTreeSet::new();
        for (i, f) in p.features.iter().enumerate() {
            let c = self.canonical[i];
            if let Feature::Items(items) = f
                && done.insert(c)
                && !reused.contains(&c)
                && let [k] = self.usage_keys(em, &st, items)?.as_slice()
                && shared.contains(k)
            {
                // A plain datum needs its own GISU for OCCT and Solid Edge typing.
                // Other partially overlapping features compose it,
                // rather than making the datum a one-component composite.
                if self.is_datum_aspect(c) {
                    st.owners.insert(k.clone(), c);
                } else {
                    st.owners.entry(k.clone()).or_insert(c);
                }
            }
        }
        st.shared_keys = keys
            .into_iter()
            .filter(|(_, n)| *n > 1)
            .map(|(k, _)| k)
            .collect();
        for i in 0..p.features.len() {
            self.feature(em, &mut st, pds, i)?;
        }
        // Resolve the graph of carriers reached by other semantic PMI only when an
        // outgoing controlled/support SAR might be read as composition by XCAF or SFA.
        if self.mode == Mode::Add
            || p.tolerances
                .iter()
                .any(|t| matches!(t.target, ToleranceTarget::Relation { .. }))
            || p.datum_targets.iter().any(|t| t.on().is_some())
        {
            st.occt_protected.extend(
                self.kept_occt_carriers
                    .iter()
                    .copied()
                    .filter(|id| !em.removed.contains(id) && !em.retired.contains(id))
                    .map(R::Old),
            );
            for i in self.occt_protected_features() {
                st.occt_protected.insert(st.features[&self.canonical[i]]);
                if let Feature::Items(items) = &p.features[i]
                    && items.len() > 1
                {
                    for key in self.usage_keys(em, &st, items)? {
                        if let Some(&member) = st.shared.get(&key) {
                            st.occt_protected.insert(member);
                        }
                    }
                }
            }
        }
        for (i, t) in p.datum_targets.iter().enumerate() {
            self.datum_target(em, &mut st, pds, i, t)?;
        }
        // Decision 6: a datum feature symbol per datum feature of an added datum.
        let mut symbols: Vec<(R, Vec<String>, usize)> = Vec::new();
        for (i, d) in p.datums.iter().enumerate() {
            if let Some(&(dx, _)) = self.existing_datums.get(&i) {
                st.datums.insert(i, em.reference(dx)?);
                report
                    .reused_datums
                    .push((self.id, d.label().as_str().to_string(), dx));
                continue;
            }
            let datum = em.simple(
                "datum",
                &[
                    ("name", s("")),
                    ("description", s("")),
                    ("of_shape", pds.a()),
                    ("product_definitional", boolean(false)),
                    ("identification", s(d.label().as_str())),
                ],
            )?;
            if let Some(f) = d.feature() {
                let fr = self.feature(em, &mut st, pds, f.0)?;
                em.simple(
                    "shape_aspect_relationship",
                    &[
                        ("name", s("")),
                        ("description", s("")),
                        ("relating_shape_aspect", fr.a()),
                        ("related_shape_aspect", datum.a()),
                    ],
                )?;
                match symbols.iter_mut().find(|(r, _, _)| *r == fr) {
                    Some((_, labels, _)) => labels.push(d.label().as_str().to_string()),
                    None => symbols.push((fr, vec![d.label().as_str().to_string()], f.0)),
                }
            }
            for t in d.targets() {
                em.simple(
                    "shape_aspect_relationship",
                    &[
                        ("name", s("")),
                        ("description", s("datum target")),
                        ("relating_shape_aspect", st.targets[&t.0].a()),
                        ("related_shape_aspect", datum.a()),
                    ],
                )?;
            }
            st.datums.insert(i, datum);
        }
        #[cfg(not(feature = "presentation"))]
        self.datum_symbols(em, &symbols, &st, report, presentation_reader)?;
        for i in 0..p.dimensions.len() {
            self.dimension(em, &mut st, pds, i)?;
        }
        let mut names = self.system_names.clone();
        for (i, t) in p.tolerances.iter().enumerate() {
            let r = self.tolerance(em, &mut st, pds, t, &mut names)?;
            st.tolerances.insert(i, r);
        }
        let tolerance_relations = p.tolerance_relations.iter();
        #[cfg(feature = "json-import")]
        let tolerance_relations = tolerance_relations.enumerate();
        #[cfg(not(feature = "json-import"))]
        let tolerance_relations = tolerance_relations.map(|item| ((), item));
        for (_i, r) in tolerance_relations {
            let name = match r.kind {
                RelationKind::Composite => "composite",
                RelationKind::Precedence => "precedence",
                RelationKind::Simultaneity => "simultaneity",
            };
            let _carrier = em.simple(
                "geometric_tolerance_relationship",
                &[
                    ("name", s(name)),
                    ("description", s("")),
                    (
                        "relating_geometric_tolerance",
                        st.tolerances[&r.relating.0].a(),
                    ),
                    (
                        "related_geometric_tolerance",
                        st.tolerances[&r.related.0].a(),
                    ),
                ],
            )?;
            #[cfg(feature = "json-import")]
            self.identify(em, "tolerance_relations", _i, _carrier)?;
        }
        let general = p.general.iter();
        #[cfg(any(feature = "presentation", feature = "json-import"))]
        let general = general.enumerate();
        #[cfg(not(any(feature = "presentation", feature = "json-import")))]
        let general = general.map(|item| ((), item));
        for (_i, g) in general {
            if let GeneralTolerance::Table { name, cells } = g {
                #[cfg(feature = "json-import")]
                let _table = self.tolerance_table(em, pds, name, cells)?;
                #[cfg(not(feature = "json-import"))]
                self.tolerance_table(em, pds, name, cells)?;
                #[cfg(feature = "json-import")]
                self.identify(em, "general", _i, _table)?;
            }
            if let GeneralTolerance::Class { text, .. } = g {
                let item = em.simple(
                    "descriptive_representation_item",
                    &[("name", s("tolerance class")), ("description", s(text))],
                )?;
                let _property = self.property(
                    em,
                    pds,
                    "default tolerances",
                    "",
                    "default tolerances",
                    &[item],
                )?;
                #[cfg(feature = "json-import")]
                self.identify(em, "general", _i, _property)?;
                #[cfg(feature = "presentation")]
                st.properties
                    .insert(crate::presentation::notes::Source::General(_i), _property);
            }
        }
        if let Some(places) = p.decimal_places
            && !p.general.iter().any(|g| matches!(g, GeneralTolerance::Table { cells, .. } if cells.iter().any(|c| c.items.iter().any(|(n,_)| n == "number of decimal places")))) {
            self.tolerance_table(em, pds, "", &[ToleranceCell { items: vec![("number of decimal places".into(), CellValue::Count(Decimal::parse(&places.to_string()).map_err(|e| WriteError::Internal(e.0))?))] }])?;
        }
        let threads = p.threads.iter().enumerate();
        for (_i, t) in threads {
            self.thread(em, &mut st, pds, t, _i)?;
        }
        let knurls = p.knurls.iter();
        #[cfg(feature = "json-import")]
        let knurls = knurls.enumerate();
        #[cfg(not(feature = "json-import"))]
        let knurls = knurls.map(|item| ((), item));
        for (_i, k) in knurls {
            self.knurl(
                em,
                &mut st,
                pds,
                k,
                #[cfg(feature = "json-import")]
                _i,
            )?;
        }
        if let Some(m) = &p.material {
            let item = em.simple(
                "descriptive_representation_item",
                &[
                    ("name", s(&m.id)),
                    ("description", s(m.name.as_deref().unwrap_or(""))),
                ],
            )?;
            let _property = self.property(
                em,
                pd,
                "material property",
                "material name",
                "material name",
                &[item],
            )?;
            #[cfg(feature = "presentation")]
            st.properties
                .insert(crate::presentation::notes::Source::Material, _property);
        }
        if let Some(d) = p.material.as_ref().and_then(|m| m.density.as_ref()) {
            let mut elements = Vec::new();
            for (name, exponent) in &d.unit {
                let unit = match density_unit(name)
                    .ok_or_else(|| WriteError::Internal(format!("{name} is not a density unit")))?
                {
                    DensityUnit::Length(l) => em.length_unit(self.context, &l)?.0,
                    DensityUnit::Mass(prefix) => em.mass_unit(prefix)?,
                };
                let e = real(exponent).map_err(WriteError::Internal)?;
                elements.push(em.instance(
                    &["derived_unit_element"],
                    &[(("", "unit"), unit.a()), (("", "exponent"), e)],
                    &[exponent],
                )?);
            }
            let unit = em.simple("derived_unit", &[("elements", refs(elements))])?;
            let item = em.instance(
                &["measure_representation_item"],
                &[
                    (("", "name"), s("density measure")),
                    (
                        ("", "value_component"),
                        typed(
                            "positive_ratio_measure",
                            real(&d.value).map_err(WriteError::Internal)?,
                        ),
                    ),
                    (("", "unit_component"), unit.a()),
                ],
                &[&d.value],
            )?;
            self.property(em, pd, "material property", "density", "density", &[item])?;
        }
        let attributes = p.attributes.iter();
        #[cfg(any(feature = "presentation", feature = "json-import"))]
        let attributes = attributes.enumerate();
        #[cfg(not(any(feature = "presentation", feature = "json-import")))]
        let attributes = attributes.map(|item| ((), item));
        for (_i, a) in attributes {
            let _property = self.attribute_set(em, &mut st, pd, pds, a)?;
            #[cfg(feature = "json-import")]
            self.identify(em, "attributes", _i, _property)?;
            #[cfg(feature = "presentation")]
            st.properties
                .insert(crate::presentation::notes::Source::Attribute(_i), _property);
        }
        let surface_textures = p.surface_textures.iter();
        #[cfg(any(feature = "presentation", feature = "json-import"))]
        let surface_textures = surface_textures.enumerate();
        #[cfg(not(any(feature = "presentation", feature = "json-import")))]
        let surface_textures = surface_textures.map(|item| ((), item));
        for (_i, t) in surface_textures {
            let _property = self.surface_texture(em, &mut st, pds, t)?;
            #[cfg(feature = "json-import")]
            self.identify(em, "surface_textures", _i, _property)?;
            #[cfg(feature = "presentation")]
            st.properties
                .insert(crate::presentation::notes::Source::Texture(_i), _property);
        }
        #[cfg(feature = "json-import")]
        if let Some(ids) = self.identifiers {
            for (i, note) in p.notes.iter().enumerate() {
                let owner = match note.on {
                    None => pds,
                    Some(NoteOwner::Feature(f)) => self.feature(em, &mut st, pds, f.0)?,
                    Some(NoteOwner::Dimension(d)) => self.dimension(em, &mut st, pds, d.0)?,
                    Some(NoteOwner::Tolerance(t)) => st.tolerances[&t.0],
                    Some(NoteOwner::DatumTarget(t)) => st.targets[&t.0],
                };
                let text = Self::text_item(em, "note text", &note.text)?;
                let property = self.property(
                    em,
                    owner,
                    "manufacturing requirement",
                    &note.kind,
                    "",
                    &[text],
                )?;
                self.identify(em, "notes", i, property)?;
                #[cfg(feature = "presentation")]
                st.properties
                    .insert(crate::presentation::notes::Source::Note(i), property);
            }
            if p != &PartPmi::default()
                || crate::json::transport::has_units(ids)
                    .map_err(|e| WriteError::Internal(e.to_string()))?
            {
                let layout = crate::json::transport::layout(p, ids)
                    .map_err(|e| WriteError::Internal(e.to_string()))?;
                let item = Self::text_item(em, "ordering", &layout)?;
                self.property(
                    em,
                    pds,
                    "haecceity.pmi/1 ordering",
                    "Transport ordering and reference aliases; native PMI remains authoritative",
                    "",
                    &[item],
                )?;
            }
            for (collection, refs) in [
                ("datum_targets", &st.targets),
                ("datums", &st.datums),
                ("dimensions", &st.dimensions),
                ("tolerances", &st.tolerances),
            ] {
                for (&i, &carrier) in refs {
                    self.identify(em, collection, i, carrier)?;
                }
            }
        }
        #[cfg(feature = "presentation")]
        self.datum_symbols(em, &symbols, &st, report, presentation_reader)?;
        Ok(())
    }

    fn datum_target(
        &self,
        em: &mut Emitter<'_>,
        st: &mut Emitted,
        pds: R,
        i: usize,
        t: &DatumTarget,
    ) -> Result<(), WriteError> {
        let target = st.targets[&i];
        if let TargetShape::Area(f) | TargetShape::Curve(f) = t.shape() {
            let c = self.canonical[f.0];
            if let Feature::Items(items) = &self.pmi.features[c] {
                let own = (st.features.get(&c) == Some(&target)).then_some(c);
                self.usages(em, st, pds, target, own, items)?;
            } else {
                let member = self.feature(em, st, pds, f.0)?;
                em.simple(
                    "shape_aspect_relationship",
                    &[
                        ("name", s("")),
                        ("description", s("")),
                        ("relating_shape_aspect", target.a()),
                        ("related_shape_aspect", member.a()),
                    ],
                )?;
            }
        }
        let mut parameters = Vec::new();
        if let Some(p) = t.placement() {
            let coords = p
                .origin
                .iter()
                .map(|l| real(&l.value).map_err(WriteError::Internal))
                .collect::<Result<Vec<_>, _>>()?;
            let point = self.stated_simple(
                em,
                "cartesian_point",
                &[("name", s("")), ("coordinates", A::List(coords))],
                &p.origin.iter().map(|l| &l.value).collect::<Vec<_>>(),
            )?;
            let mut direction = |d: &Direction, name: &str| -> Result<R, WriteError> {
                let ratios =
                    d.0.iter()
                        .map(|v| real(v).map_err(WriteError::Internal))
                        .collect::<Result<Vec<_>, _>>()?;
                self.stated_simple(
                    em,
                    "direction",
                    &[("name", s(name)), ("direction_ratios", A::List(ratios))],
                    &d.0.iter().collect::<Vec<_>>(),
                )
            };
            let axis = p
                .axis
                .as_ref()
                .map(|d| direction(d, ""))
                .transpose()?
                .map_or(A::Unset, R::a);
            let x = p
                .ref_direction
                .as_ref()
                .map(|d| direction(d, ""))
                .transpose()?
                .map_or(A::Unset, R::a);
            parameters.push(em.simple(
                "axis2_placement_3d",
                &[
                    ("name", s("orientation")),
                    ("location", point.a()),
                    ("axis", axis),
                    ("ref_direction", x),
                ],
            )?);
        }
        for (name, length) in target_sizes(t.shape()) {
            parameters.push(em.measure_item(
                name,
                MeasureItem::Length(length),
                self.context,
                Vec::new(),
                None,
            )?);
        }
        if let Some(d) = t.movable() {
            let ratios =
                d.0.iter()
                    .map(|v| real(v).map_err(WriteError::Internal))
                    .collect::<Result<Vec<_>, _>>()?;
            parameters.push(self.stated_simple(
                em,
                "direction",
                &[
                    ("name", s("movable direction")),
                    ("direction_ratios", A::List(ratios)),
                ],
                &d.0.iter().collect::<Vec<_>>(),
            )?);
        }
        if !parameters.is_empty() {
            let prop = em.simple(
                "property_definition",
                &[
                    ("name", s("")),
                    ("description", A::Unset),
                    ("definition", target.a()),
                ],
            )?;
            let rep = em.simple(
                "shape_representation_with_parameters",
                &[
                    ("name", s("")),
                    ("items", refs(parameters)),
                    ("context_of_items", em.reference(self.context)?.a()),
                ],
            )?;
            em.simple(
                "shape_definition_representation",
                &[("definition", prop.a()), ("used_representation", rep.a())],
            )?;
        }
        if let Some(f) = t.on() {
            let carrier = self.feature(em, st, pds, f.0)?;
            let isolate = self.occt_needs_isolation(em, st, carrier)
                && self.occt_has_direct_gisu(em, carrier);
            let feature = self.relationship_endpoint(
                em,
                st,
                pds,
                f,
                isolate.then_some("shared-carrier-datum-target-support"),
            )?;
            em.simple(
                "feature_for_datum_target_relationship",
                &[
                    ("name", s("")),
                    ("description", A::Unset),
                    ("relating_shape_aspect", feature.a()),
                    ("related_shape_aspect", target.a()),
                ],
            )?;
        }
        Ok(())
    }

    fn tolerance_table(
        &self,
        em: &mut Emitter<'_>,
        pds: R,
        name: &str,
        cells: &[ToleranceCell],
    ) -> Result<TableIdentity, WriteError> {
        let context = em.simple(
            "representation_context",
            &[
                ("context_identifier", s("")),
                ("context_type", s("default setting")),
            ],
        )?;
        let mut items = Vec::new();
        for c in cells {
            let mut elements = Vec::new();
            for (name, v) in &c.items {
                elements.push(match v {
                    CellValue::Text(t) => Self::text_item(em, name, t)?,
                    CellValue::Count(d) => em.measure_item(
                        name,
                        MeasureItem::Count(d),
                        self.context,
                        Vec::new(),
                        None,
                    )?,
                    CellValue::Value(v) => em.measure_item(
                        name,
                        measure_of_value(v),
                        self.context,
                        Vec::new(),
                        v.decimal_places,
                    )?,
                });
            }
            let cell = em.simple(
                "default_tolerance_table_cell",
                &[
                    ("name", s("")),
                    (
                        "item_element",
                        typed("set_representation_item", refs(elements)),
                    ),
                ],
            )?;
            if c.decimal_default().is_some() {
                em.practice_conflict(
                    cell,
                    &[
                        "default_tolerance_table_cell.WR3",
                        "default_tolerance_table_cell.WR4",
                    ],
                );
            }
            items.push(cell);
        }
        let table = em.simple(
            "default_tolerance_table",
            &[
                ("name", s(name)),
                ("items", refs(items)),
                ("context_of_items", context.a()),
            ],
        )?;
        let class = Self::text_item(em, "tolerance class", "")?;
        let rep = em.simple(
            "representation",
            &[
                ("name", s("default tolerances")),
                ("items", refs([class])),
                ("context_of_items", context.a()),
            ],
        )?;
        let prop = em.simple(
            "property_definition",
            &[
                ("name", s("default tolerances")),
                ("description", s("")),
                ("definition", pds.a()),
            ],
        )?;
        em.simple(
            "property_definition_representation",
            &[("definition", prop.a()), ("used_representation", rep.a())],
        )?;
        em.simple(
            "representation_relationship",
            &[
                ("name", s("general tolerance definition")),
                ("description", s("")),
                ("rep_1", table.a()),
                ("rep_2", rep.a()),
            ],
        )?;
        #[cfg(feature = "json-import")]
        {
            Ok(table)
        }
        #[cfg(not(feature = "json-import"))]
        {
            Ok(())
        }
    }

    /// A point of feature `f` in the part's coordinates: of its first face, edge or
    /// supplemental geometry item (a group's or derived feature's first member's).
    fn feature_point(&self, f: usize) -> Option<[f64; 3]> {
        match &self.pmi.features[f] {
            Feature::Items(items) => items.iter().find_map(|a| match *a {
                Anchor::Face(x) => first_point(self.doc, self.def.faces[x.0]),
                Anchor::Edge(x) => first_point(self.doc, self.def.edges[x.0]),
                Anchor::Geometry(g) => geometry_point(&self.pmi.geometry[g.0].item),
            }),
            Feature::Group { members, .. } | Feature::Derived { from: members, .. } => {
                members.iter().find_map(|m| self.feature_point(m.0))
            }
        }
    }

    /// The axis of the feature's first elementary surface of revolution, in part coordinates.
    #[cfg(feature = "presentation")]
    fn feature_axis(&self, f: usize) -> Option<[f64; 3]> {
        match &self.pmi.features[f] {
            Feature::Items(items) => items.iter().find_map(|a| {
                let Anchor::Face(face) = a else { return None };
                let surface = attr_ref(
                    self.doc,
                    self.def.faces[face.0],
                    "face_surface",
                    "face_geometry",
                )?;
                if !["cylindrical_surface", "conical_surface", "toroidal_surface"]
                    .iter()
                    .any(|t| is_a(self.doc, surface, t))
                {
                    return None;
                }
                let pos = attr_ref(self.doc, surface, "elementary_surface", "position")?;
                let axis = attr_ref(self.doc, pos, "axis2_placement_3d", "axis")?;
                let A::List(values) = attr(self.doc, axis, "direction", "direction_ratios")? else {
                    return None;
                };
                let mut d = [0.; 3];
                for (i, v) in values.iter().take(3).enumerate() {
                    d[i] = match v {
                        A::Real(x) => *x,
                        A::Integer(x) => *x as f64,
                        _ => return None,
                    };
                }
                Some(d)
            }),
            Feature::Group { members, .. } | Feature::Derived { from: members, .. } => {
                members.iter().find_map(|f| self.feature_axis(f.0))
            }
        }
    }

    /// Decision 6: for each datum feature of an added datum, a minimal datum feature symbol
    /// (the label boxed, a stem to a triangle on the feature), derived from the semantics:
    /// one tessellated callout per feature (PMI practice §8.2, the set named for its PMI type,
    /// 'datum'), each in its own annotation plane (§9.1), the planes in one draughting model of
    /// the part related to its shape representation, each callout linked to its datum feature
    /// by a `draughting_model_item_association` ('PMI representation to presentation link',
    /// §7.3). Replace and remove take the symbol with its datum (removal plan, policy
    /// `RemovePresentation`).
    fn datum_symbols(
        &self,
        em: &mut Emitter<'_>,
        symbols: &[(R, Vec<String>, usize)],
        _st: &Emitted,
        report: &mut WriteReport,
        presentation_reader: Option<&crate::step::PartReader>,
    ) -> Result<(), WriteError> {
        if symbols.is_empty() && !needs_callouts(self.pmi) && {
            #[cfg(all(feature = "json-import", feature = "presentation"))]
            {
                self.source.is_empty()
            }
            #[cfg(not(all(feature = "json-import", feature = "presentation")))]
            {
                true
            }
        } {
            return Ok(());
        }
        let presentation_part = presentation_reader.and_then(|r| r.part(self.def).ok());
        let factor = self.context_length_unit().map_or(1., |u| u.millimetres());
        let mut tips = Vec::new();
        for (_, labels, f) in symbols {
            tips.push(self.feature_point(*f).ok_or_else(|| {
                WriteError::Internal(format!(
                    "datum {}: its feature has no point for its symbol",
                    labels.join(",")
                ))
            })?);
        }
        let mut points = vertices(self.doc, &self.def.faces);
        points.extend(tips.iter().copied());
        #[cfg(not(feature = "presentation"))]
        let mut layout = SymbolLayout::new(&points, factor);
        #[cfg(feature = "presentation")]
        let mut local_layout = {
            let mut bounds = crate::geom::Bounds::empty();
            // The B-rep includes circular-arc and surface extrema, not just seam vertices.
            if let Some(part) = &presentation_part {
                let b = part.bounds();
                bounds.add(b.min.map(|x| x / factor));
                bounds.add(b.max.map(|x| x / factor));
            }
            if !bounds.max_extent().is_finite() {
                for &p in &points {
                    bounds.add(p);
                }
            }
            crate::presentation::placement::Layout::with_unit(
                bounds,
                self.context_length_unit().map_or(1., |u| u.millimetres()),
            )
        };
        #[cfg(feature = "presentation")]
        let measurements = presentation_part
            .as_ref()
            .map(|part| crate::presentation::linear::measurements(part, self.def, self.pmi));
        #[cfg(all(feature = "json-import", feature = "presentation"))]
        let source_height = local_layout.text_height() * factor;
        #[cfg(all(feature = "json-import", feature = "presentation"))]
        let placed = self.source_graphics(symbols, _st, factor, source_height, report)?;
        #[cfg(all(feature = "json-import", feature = "presentation"))]
        let is_placed = |definition: R| placed.iter().any(|g| g.0.contains(&definition));
        #[cfg(all(not(feature = "json-import"), feature = "presentation"))]
        let is_placed = |_: R| false;
        #[cfg(all(feature = "json-import", feature = "presentation"))]
        for (_, _, _, plane, lines) in &placed {
            local_layout.reserve_ink(lines, plane[1], plane[2]);
        }
        let mut graphics = Vec::new();
        #[cfg(feature = "presentation")]
        let mut note_pieces: BTreeMap<R, Vec<Vec<[f64; 3]>>> = BTreeMap::new();
        for ((feature, labels, _feature_index), tip) in symbols.iter().zip(&tips) {
            let label = labels.join(",");
            #[cfg(all(feature = "json-import", feature = "presentation"))]
            if let Some(g) = placed.iter().find(|g| g.0.contains(feature)) {
                graphics.push(g.clone());
                report.datum_symbols.push((self.id, label));
                continue;
            }
            #[cfg(not(feature = "presentation"))]
            let (plane, lines) = if let Some((point, normal, _)) =
                presentation_part.as_ref().and_then(|part| {
                    super::sites::feature_site(part, self.pmi, FeatureId(*_feature_index))
                }) {
                layout.draw_on_face(point.map(|x| x / factor), normal, &label)
            } else {
                layout.draw(*tip, &label)
            };
            #[cfg(feature = "presentation")]
            let (plane, lines) = {
                let c = crate::presentation::datum(&label);
                let p = if let Some((point, normal, _)) =
                    presentation_part.as_ref().and_then(|part| {
                        crate::presentation::placement::feature_site(
                            part,
                            self.pmi,
                            FeatureId(*_feature_index),
                        )
                    }) {
                    local_layout.on_face(point.map(|x| x / factor), normal, &c)
                } else {
                    local_layout.place(*tip, self.feature_axis(*_feature_index), &c)
                };
                ([p.origin, p.normal, p.along], p.lines)
            };
            graphics.push((
                vec![*feature],
                "datum",
                format!("Datum {label}"),
                plane,
                lines,
            ));
            report.datum_symbols.push((self.id, label));
        }
        #[cfg(feature = "presentation")]
        {
            use crate::presentation;
            let feature_of_dimension = |d: &Dimension| match d.kind {
                DimensionKind::Size { feature, .. } => feature,
                DimensionKind::Location { from, .. } => from,
            };
            for (i, d) in self.pmi.dimensions.iter().enumerate() {
                #[cfg(feature = "json-import")]
                if let Some(g) = placed.iter().find(|g| g.0.contains(&_st.dimensions[&i])) {
                    graphics.push(g.clone());
                    continue;
                }
                let f = feature_of_dimension(d);
                let tip = self
                    .feature_point(f.0)
                    .unwrap_or(points.first().copied().unwrap_or([0.; 3]));
                let c = presentation::dimension(d);
                let name = c.notation();
                let p = if measurements.as_ref().is_some_and(|ms| ms[i].is_some()) {
                    // The measured ladder below places it. Do not reserve a discarded
                    // bare-number slot that would push the control frames away.
                    crate::presentation::placement::Placed {
                        anchor: tip,
                        text_origin: tip,
                        text_scale: 1.,
                        origin: tip,
                        normal: [0., 0., 1.],
                        along: [1., 0., 0.],
                        lines: Vec::new(),
                    }
                } else {
                    local_layout.place(tip, self.feature_axis(f.0), &c)
                };
                let (plane, lines) = ([p.origin, p.normal, p.along], p.lines);
                graphics.push((
                    vec![_st.dimensions[&i]],
                    presentation::dimension_type(d),
                    name,
                    plane,
                    lines,
                ));
            }
            for (i, t) in self.pmi.tolerances.iter().enumerate() {
                #[cfg(feature = "json-import")]
                if let Some(g) = placed.iter().find(|g| g.0.contains(&_st.tolerances[&i])) {
                    graphics.push(g.clone());
                    continue;
                }
                let f = match &t.target {
                    ToleranceTarget::Feature(f) => Some(*f),
                    ToleranceTarget::Dimension(d) => {
                        Some(feature_of_dimension(&self.pmi.dimensions[d.0]))
                    }
                    ToleranceTarget::Relation { relating, .. } => Some(*relating),
                    ToleranceTarget::WholePart => None,
                };
                let tip = f
                    .and_then(|f| self.feature_point(f.0))
                    .unwrap_or(points.first().copied().unwrap_or([0.; 3]));
                let c = presentation::tolerance(t, self.pmi);
                let name = c.notation();
                let site = f.and_then(|f| {
                    presentation_part.as_ref().and_then(|part| {
                        crate::presentation::placement::feature_site(part, self.pmi, f)
                    })
                });
                let p = if let Some((point, normal, face)) = site {
                    let part = presentation_part.as_ref().ok_or_else(|| {
                        WriteError::Internal("missing annotation geometry".into())
                    })?;
                    let extent = part.face_bounds(face).max_extent() / factor;
                    if matches!(part.faces[face].surface, crate::geom::Surface::Plane { .. }) {
                        local_layout.on_feature(point.map(|x| x / factor), normal, &c, extent)
                    } else if let Some(axis) = f
                        .and_then(|f| self.feature_axis(f.0))
                        .and_then(crate::geom::unit)
                    {
                        let centre = part.bounds().centre().map(|x| x / factor);
                        let axis = crate::geom::scale(
                            axis,
                            if crate::geom::dot(crate::geom::sub(tip, centre), axis) < 0. {
                                -1.
                            } else {
                                1.
                            },
                        );
                        local_layout.on_axis(tip, axis, &c, extent)
                    } else {
                        local_layout.place(tip, None, &c)
                    }
                } else {
                    local_layout.place(tip, f.and_then(|f| self.feature_axis(f.0)), &c)
                };
                let (plane, lines) = ([p.origin, p.normal, p.along], p.lines);
                graphics.push((
                    vec![_st.tolerances[&i]],
                    presentation::tolerance_type(t),
                    name,
                    plane,
                    lines,
                ));
            }
        }
        #[cfg(feature = "presentation")]
        if !self.pmi.threads.is_empty() {
            use crate::{geom, presentation};
            // Repack only thread-bearing features. Keep thread-free serialization
            // identical; an old independent frame must not choose another plane.
            let mut features = Vec::<FeatureId>::new();
            for t in &self.pmi.threads {
                if !features.iter().any(|&f| {
                    self.pmi.same_target(
                        &ToleranceTarget::Feature(f),
                        &ToleranceTarget::Feature(t.feature),
                    )
                }) {
                    features.push(t.feature);
                }
            }
            let key = |f| {
                features.iter().copied().find(|&other| {
                    self.pmi.same_target(
                        &ToleranceTarget::Feature(f),
                        &ToleranceTarget::Feature(other),
                    )
                })
            };
            let mut stacks = BTreeMap::<usize, Vec<(usize, presentation::Callout)>>::new();
            for (i, d) in self.pmi.dimensions.iter().enumerate() {
                if is_placed(_st.dimensions[&i])
                    || measurements.as_ref().is_some_and(|m| m[i].is_some())
                {
                    continue;
                }
                if let DimensionKind::Size { feature, .. } = d.kind
                    && let Some(f) = key(feature)
                {
                    stacks
                        .entry(f.0)
                        .or_default()
                        .push((symbols.len() + i, presentation::dimension(d)));
                }
            }
            for (i, t) in self.pmi.threads.iter().enumerate() {
                #[cfg(feature = "json-import")]
                if let Some(g) = placed.iter().find(|g| g.0.contains(&_st.threads[&i])) {
                    graphics.push(g.clone());
                    continue;
                }
                let c = presentation::thread(t, self.pmi);
                let index = graphics.len();
                graphics.push((
                    vec![_st.threads[&i]],
                    "thread",
                    c.notation(),
                    [[0.; 3]; 3],
                    Vec::new(),
                ));
                let f = key(t.feature)
                    .ok_or_else(|| WriteError::Internal("thread stack feature missing".into()))?;
                stacks.entry(f.0).or_default().push((index, c));
            }
            for (i, t) in self.pmi.tolerances.iter().enumerate() {
                if is_placed(_st.tolerances[&i]) {
                    continue;
                }
                let feature = match t.target {
                    ToleranceTarget::Feature(f) | ToleranceTarget::Relation { relating: f, .. } => {
                        Some(f)
                    }
                    ToleranceTarget::Dimension(d) => match self.pmi.dimensions[d.0].kind {
                        DimensionKind::Size { feature, .. } => Some(feature),
                        _ => None,
                    },
                    ToleranceTarget::WholePart => None,
                };
                if let Some(f) = feature.and_then(key) {
                    stacks.entry(f.0).or_default().push((
                        symbols.len() + self.pmi.dimensions.len() + i,
                        presentation::tolerance(t, self.pmi),
                    ));
                }
            }
            let stacked: BTreeSet<_> = stacks.values().flatten().map(|(i, _)| *i).collect();
            let mut bounds = geom::Bounds::empty();
            if let Some(part) = &presentation_part {
                let b = part.bounds();
                bounds.add(b.min.map(|x| x / factor));
                bounds.add(b.max.map(|x| x / factor));
            } else {
                for &p in &points {
                    bounds.add(p);
                }
            }
            let mut packed = presentation::placement::Layout::with_unit(bounds, factor);
            #[cfg(feature = "json-import")]
            for (_, _, _, plane, lines) in &placed {
                packed.reserve_ink(lines, plane[1], plane[2]);
            }
            for (i, (_, _, _, plane, lines)) in graphics.iter().enumerate() {
                if !stacked.contains(&i) {
                    packed.reserve_ink(&lines[..lines.len().saturating_sub(1)], plane[1], plane[2]);
                }
            }
            for (feature, stack) in stacks {
                let site = presentation_part.as_ref().and_then(|part| {
                    presentation::placement::feature_site(part, self.pmi, FeatureId(feature)).map(
                        |(point, normal, face)| {
                            (point, normal, part.face_bounds(face).max_extent())
                        },
                    )
                });
                let positions = if let Some((point, normal, extent)) = site {
                    let callouts: Vec<_> = stack.iter().map(|(_, c)| c).collect();
                    if stack.len() == 1 {
                        vec![packed.on_label(
                            point.map(|x| x / factor),
                            normal,
                            callouts[0],
                            extent / factor,
                        )]
                    } else {
                        packed.stack(
                            point.map(|x| x / factor),
                            normal,
                            &callouts,
                            extent / factor,
                        )
                    }
                } else {
                    stack
                        .iter()
                        .map(|(_, c)| {
                            packed.place(
                                self.feature_point(feature).unwrap_or([0.; 3]),
                                self.feature_axis(feature),
                                c,
                            )
                        })
                        .collect()
                };
                for ((i, _), p) in stack.into_iter().zip(positions) {
                    graphics[i].3 = [p.origin, p.normal, p.along];
                    graphics[i].4 = p.lines;
                }
            }
            local_layout = packed;
        }
        // The measured ladder already avoids annotations. Include placed notes
        // and finishes before that pass, even though generated ones follow it.
        #[cfg(all(feature = "presentation", feature = "json-import"))]
        for g in &placed {
            if !graphics.iter().any(|old| old.0 == g.0) {
                graphics.push(g.clone());
            }
        }
        #[cfg(feature = "presentation")]
        if let Some(part) = &presentation_part {
            use crate::presentation::{self, linear};
            let measurements = measurements
                .as_ref()
                .ok_or_else(|| WriteError::Internal("missing linear measurements".into()))?;
            let mut ladder = linear::Ladder::compact(part.bounds());
            for (i, (_, _, _, plane, lines)) in graphics.iter().enumerate() {
                if i < symbols.len()
                    || i >= symbols.len() + self.pmi.dimensions.len()
                    || is_placed(graphics[i].0[0])
                    || measurements[i - symbols.len()].is_none()
                {
                    let mut b = crate::geom::Bounds::empty();
                    for p in lines.iter().take(lines.len().saturating_sub(1)).flatten() {
                        b.add(p.map(|x| x * factor));
                    }
                    if i >= symbols.len() && i < symbols.len() + self.pmi.dimensions.len() {
                        ladder.reserve_dimension(b, plane[1], plane[2]);
                    } else {
                        ladder.reserve_annotation(b, plane[1]);
                    }
                }
            }
            let mut order: Vec<_> = measurements
                .iter()
                .enumerate()
                .filter(|(i, _)| !is_placed(_st.dimensions[i]))
                .filter_map(|(i, m)| m.as_ref().map(|m| (i, m)))
                .collect();
            order.sort_by(|a, b| a.1.length().total_cmp(&b.1.length()).then(a.0.cmp(&b.0)));
            for (i, m) in order {
                let p = ladder
                    .draw(m, &presentation::dimension(&self.pmi.dimensions[i]))
                    .placed;
                let g = &mut graphics[symbols.len() + i];
                g.3 = [p.origin.map(|x| x / factor), p.normal, p.along];
                g.4 = p
                    .lines
                    .into_iter()
                    .map(|l| l.into_iter().map(|p| p.map(|x| x / factor)).collect())
                    .collect();
            }
        }
        #[cfg(feature = "presentation")]
        {
            use crate::presentation::{self, notes};
            // Reserve the finished graphics, including extension lines, before placing new ink.
            for (_, _, _, _, lines) in &graphics {
                local_layout.reserve(lines);
            }
            for (i, t) in self
                .pmi
                .surface_textures
                .iter()
                .enumerate()
                .filter(|(_, t)| t.on.is_some())
            {
                if is_placed(_st.properties[&notes::Source::Texture(i)]) {
                    continue;
                }
                let f =
                    t.on.ok_or_else(|| WriteError::Internal("missing texture feature".into()))?;
                let part = presentation_part
                    .as_ref()
                    .ok_or_else(|| WriteError::Internal("texture geometry unavailable".into()))?;
                let (_, _, face) = presentation::placement::feature_site(part, self.pmi, f)
                    .ok_or_else(|| {
                        WriteError::Internal("surface texture has no interior face site".into())
                    })?;
                let sites = presentation::placement::surface_sites(part, face)
                    .into_iter()
                    .map(|(point, normal)| (point.map(|x| x / factor), normal))
                    .collect::<Vec<_>>();
                let c = notes::surface_texture(t);
                let p = local_layout.surface(&sites, &c).ok_or_else(|| {
                    WriteError::Internal("surface texture has no interior tip site".into())
                })?;
                graphics.push((
                    vec![_st.properties[&notes::Source::Texture(i)]],
                    "surface roughness",
                    c.notation(),
                    [p.origin, p.normal, p.along],
                    p.lines,
                ));
            }
            let rows: Vec<_> = notes::general_notes(self.pmi)
                .into_iter()
                .filter(|r| !is_placed(_st.properties[&r.source]))
                .collect();
            if !rows.is_empty() {
                let c = notes::notes_block(&rows);
                // Appended annotations stay byte-identical, but still occupy space.
                local_layout.reserve_notes(&kept_notes_points(self.doc, em.removed));
                let p = local_layout.notes(&c);
                let scale = local_layout.text_height() / 3.;
                for (i, text) in c.letters.iter().enumerate() {
                    let row = &rows[i.saturating_sub(1)];
                    let lines = presentation::font().text(&text.text, text.size).lines;
                    note_pieces
                        .entry(_st.properties[&row.source])
                        .or_default()
                        .extend(lines.into_iter().map(|line| {
                            line.into_iter()
                                .map(|q| {
                                    [
                                        p.origin[0] + (q[0] + text.at[0]) * scale,
                                        p.origin[1],
                                        p.origin[2] + (q[1] + text.at[1]) * scale,
                                    ]
                                })
                                .collect()
                        }));
                }
                let definitions: BTreeSet<_> =
                    rows.iter().map(|r| _st.properties[&r.source]).collect();
                graphics.push((
                    definitions.into_iter().collect(),
                    "note",
                    c.cells.join("\n"),
                    [p.origin, p.normal, p.along],
                    p.lines,
                ));
            }
        }
        #[cfg(all(feature = "json-import", feature = "presentation"))]
        source_presentation::overlap_warnings(&graphics, &placed, report);
        let context = em.reference(self.context)?;
        let font = em.simple(
            "draughting_pre_defined_curve_font",
            &[("name", s("continuous"))],
        )?;
        let black = em.simple("draughting_pre_defined_colour", &[("name", s("black"))])?;
        #[cfg(not(feature = "presentation"))]
        let width = (layout.height / 14. * 1e6).round() / 1e6;
        #[cfg(feature = "presentation")]
        let width = ((local_layout.text_height() / 14. * 1e6).round() / 1e6).max(1e-6);
        let curve = em.simple(
            "curve_style",
            &[
                ("name", s("")),
                ("curve_font", font.a()),
                (
                    "curve_width",
                    typed("positive_length_measure", A::Real(width)),
                ),
                ("curve_colour", black.a()),
            ],
        )?;
        let line_style = em.simple(
            "presentation_style_assignment",
            &[("styles", refs([curve]))],
        )?;
        // The annotation plane's style, as NIST's test files give it.
        let colour = em.simple("colour", &[])?;
        let fill_colour = em.simple(
            "fill_area_style_colour",
            &[("name", s("")), ("fill_colour", colour.a())],
        )?;
        let fill = em.simple(
            "fill_area_style",
            &[("name", s("")), ("fill_styles", refs([fill_colour]))],
        )?;
        let plane_style =
            em.simple("presentation_style_assignment", &[("styles", refs([fill]))])?;
        let point = |em: &mut Emitter<'_>, p: [f64; 3]| {
            em.simple(
                "cartesian_point",
                &[
                    ("name", s("")),
                    ("coordinates", A::List(p.map(A::Real).to_vec())),
                ],
            )
        };
        let direction = |em: &mut Emitter<'_>, d: [f64; 3]| {
            em.simple(
                "direction",
                &[
                    ("name", s("")),
                    ("direction_ratios", A::List(d.map(A::Real).to_vec())),
                ],
            )
        };
        let mut planes = Vec::new();
        let mut callouts = Vec::new();
        for (definition, annotation_type, name, [origin, normal, along], lines) in graphics {
            // Solid Edge uses these names in its tree. Keep the full note wording
            // with the drawn curves, independently of the short annotation title.
            let title = if annotation_type == "note" {
                "General notes"
            } else {
                &name
            };
            #[cfg(feature = "presentation")]
            if let Some(missing) = name
                .chars()
                .find(|&c| !crate::presentation::font().supports(c))
            {
                return Err(WriteError::Internal(format!("missing PMI glyph {missing}")));
            }
            let mut occurrences = Vec::new();
            for &definition in &definition {
                #[cfg(feature = "presentation")]
                let source_lines = if annotation_type == "note" {
                    &note_pieces[&definition]
                } else {
                    &lines
                };
                #[cfg(not(feature = "presentation"))]
                let source_lines = &lines;
                #[cfg(all(feature = "json-import", feature = "presentation"))]
                let source_lines = if self
                    .source_for_definitions(symbols, _st, &[definition])
                    .is_some_and(|p| p.has_leader())
                {
                    &source_lines[..source_lines.len().saturating_sub(1)]
                } else {
                    source_lines.as_slice()
                };
                // Blank note rows still occupy their measured height in the block,
                // but AP242 tessellated_curve_set requires nonempty line strips.
                #[cfg(feature = "presentation")]
                if source_lines.is_empty() {
                    continue;
                }
                let coordinates: Vec<A> = source_lines
                    .iter()
                    .flatten()
                    .map(|p| A::List(p.map(A::Real).to_vec()))
                    .collect();
                let mut strips = Vec::new();
                let mut first = 1i64;
                for l in source_lines {
                    strips.push(A::List(
                        (0..l.len() as i64).map(|k| A::Integer(first + k)).collect(),
                    ));
                    first += l.len() as i64;
                }
                let list = em.simple(
                    "coordinates_list",
                    &[
                        ("name", s("")),
                        ("npoints", A::Integer(coordinates.len() as i64)),
                        ("position_coords", A::List(coordinates)),
                    ],
                )?;
                let curves = em.simple(
                    "tessellated_curve_set",
                    &[
                        (
                            "name",
                            s(
                                if annotation_type == "note"
                                    && super::text::literal_len(&name) <= MAX_STRING_LITERAL_BYTES
                                {
                                    &name
                                } else if annotation_type == "note" {
                                    // This generated label duplicates the semantic text. The
                                    // full wording remains in bounded descriptive items and
                                    // in the drawn curves; a schema label cannot be chunked.
                                    "General notes"
                                } else {
                                    annotation_type
                                },
                            ),
                        ),
                        ("coordinates", list.a()),
                        ("line_strips", A::List(strips)),
                    ],
                )?;
                let set = em.simple(
                    "tessellated_geometric_set",
                    &[("name", s(annotation_type)), ("children", refs([curves]))],
                )?;
                let occurrence = em.simple(
                    "tessellated_annotation_occurrence",
                    &[
                        ("name", s(title)),
                        ("styles", refs([line_style])),
                        ("item", set.a()),
                    ],
                )?;
                occurrences.push(occurrence);
                if annotation_type == "note" {
                    callouts.push((vec![definition], occurrence, annotation_type));
                }
            }
            #[cfg(all(feature = "json-import", feature = "presentation"))]
            if let Some(source) = self.source_for_definitions(symbols, _st, &definition) {
                occurrences.extend(source.emit_metadata(em, line_style, factor, source_height)?);
            }
            let callout = em.simple(
                "draughting_callout",
                &[("name", s(title)), ("contents", refs(occurrences))],
            )?;
            let (o, n, a) = (
                point(em, origin)?,
                direction(em, normal)?,
                direction(em, along)?,
            );
            let placement = em.simple(
                "axis2_placement_3d",
                &[
                    ("name", s("")),
                    ("location", o.a()),
                    ("axis", n.a()),
                    ("ref_direction", a.a()),
                ],
            )?;
            let plane = em.simple("plane", &[("name", s(title)), ("position", placement.a())])?;
            let annotation_plane = em.simple(
                "annotation_plane",
                &[
                    ("name", s(title)),
                    ("styles", refs([plane_style])),
                    ("item", plane.a()),
                    ("elements", refs([callout])),
                ],
            )?;
            planes.push(annotation_plane);
            if annotation_type != "note" {
                callouts.push((definition, callout, annotation_type));
            }
        }
        let model = em.simple(
            "draughting_model",
            &[
                (
                    "name",
                    s(if cfg!(feature = "presentation") {
                        "PMI callouts"
                    } else {
                        "datum feature symbols"
                    }),
                ),
                ("items", refs(planes)),
                ("context_of_items", context.a()),
            ],
        )?;
        // No mechanical_design_and_draughting_relationship relates the model to the part's
        // shape representation (maintainer decision, 2026-10-09): specify-core's OpenCascade
        // loader crashed on every file with one (docs/step-ap242.md gives the cause); the
        // associations below link each symbol.
        // CAx-IF PMI §7.3: exactly the semantic link in the part draughting model.
        // SFA 5.41 rejects feature links and symbol representations here; removing them
        // leaves Solid Edge 2026's typed imports and symbolic graphics identical.
        for (definitions, callout, _annotation_type) in callouts {
            for definition in definitions {
                em.simple(
                    "draughting_model_item_association",
                    &[
                        ("name", s("PMI representation to presentation link")),
                        ("description", {
                            #[cfg(feature = "json-import")]
                            if self.identifiers.is_some() {
                                s("haecceity.generated/1")
                            } else {
                                s("")
                            }
                            #[cfg(not(feature = "json-import"))]
                            s("")
                        }),
                        ("definition", definition.a()),
                        ("used_representation", model.a()),
                        ("identified_item", callout.a()),
                    ],
                )?;
            }
        }
        Ok(())
    }

    /// Preserve each stated REAL on the JSON path; retain historical default bytes.
    fn stated_simple(
        &self,
        em: &mut Emitter<'_>,
        entity: &str,
        vals: &[(&str, A)],
        _stated: &[&Decimal],
    ) -> Result<R, WriteError> {
        #[cfg(feature = "json-import")]
        if self.identifiers.is_some() {
            let vals: Vec<_> = vals
                .iter()
                .map(|(name, value)| (("", *name), value.clone()))
                .collect();
            return em.instance(&[entity], &vals, _stated);
        }
        em.simple(entity, vals)
    }

    /// `property_definition(name, description, owner)` with one representation of `items`.
    fn property(
        &self,
        em: &mut Emitter<'_>,
        owner: R,
        name: &str,
        description: &str,
        rep_name: &str,
        items: &[R],
    ) -> Result<R, WriteError> {
        self.property_optional(em, owner, name, Some(description), rep_name, items)
    }

    #[allow(clippy::too_many_arguments)]
    fn property_optional(
        &self,
        em: &mut Emitter<'_>,
        owner: R,
        name: &str,
        description: Option<&str>,
        rep_name: &str,
        items: &[R],
    ) -> Result<R, WriteError> {
        let pdef = em.simple(
            "property_definition",
            &[
                ("name", s(name)),
                ("description", description.map_or(A::Unset, s)),
                ("definition", owner.a()),
            ],
        )?;
        let rep = em.simple(
            "representation",
            &[
                ("name", s(rep_name)),
                ("items", refs(items.iter().copied())),
                ("context_of_items", em.reference(self.context)?.a()),
            ],
        )?;
        em.simple(
            "property_definition_representation",
            &[("definition", pdef.a()), ("used_representation", rep.a())],
        )?;
        Ok(pdef)
    }

    fn standard(&self, em: &mut Emitter<'_>, pd: R, s_: &Standard) -> Result<(), WriteError> {
        let app = em.simple(
            "application_context",
            &[(
                "application",
                s("geometrical dimensioning and tolerancing representation"),
            )],
        )?;
        let ctx = em.simple(
            "product_definition_context",
            &[
                ("name", s("")),
                ("frame_of_reference", app.a()),
                ("life_cycle_stage", s("design")),
            ],
        )?;
        let role = em.simple(
            "product_definition_context_role",
            &[("name", s("additional context")), ("description", s(""))],
        )?;
        em.simple(
            "product_definition_context_association",
            &[
                ("definition", pd.a()),
                ("frame_of_reference", ctx.a()),
                ("role", role.a()),
            ],
        )?;
        let kind = em.simple("document_type", &[("product_data_type", s("standard"))])?;
        let doc = em.simple(
            "document",
            &[
                ("id", s(&s_.document)),
                ("name", s("")),
                ("description", s("")),
                ("kind", kind.a()),
            ],
        )?;
        let adr = em.simple(
            "applied_document_reference",
            &[
                ("assigned_document", doc.a()),
                ("source", s("")),
                ("items", refs([ctx])),
            ],
        )?;
        let orole = em.simple(
            "object_role",
            &[("name", s("mandatory")), ("description", s(""))],
        )?;
        em.simple(
            "role_association",
            &[("role", orole.a()), ("item_with_role", adr.a())],
        )?;
        if let Some(edition) = &s_.edition {
            let pmi_app = em.simple(
                "application_context",
                &[("application", s("product and manufacturing information"))],
            )?;
            let pctx = em.simple(
                "product_context",
                &[
                    ("name", s("")),
                    ("frame_of_reference", pmi_app.a()),
                    ("discipline_type", s("mechanical")),
                ],
            )?;
            let product = em.simple(
                "product",
                &[
                    ("id", s(&s_.document)),
                    ("name", s("")),
                    ("description", s("")),
                    ("frame_of_reference", refs([pctx])),
                ],
            )?;
            em.simple(
                "product_related_product_category",
                &[
                    ("name", s("document")),
                    ("description", s("")),
                    ("products", refs([product])),
                ],
            )?;
            let pdf = em.simple(
                "product_definition_formation",
                &[
                    ("id", s(edition)),
                    ("description", s("")),
                    ("of_product", product.a()),
                ],
            )?;
            em.simple(
                "document_product_equivalence",
                &[
                    ("name", s("equivalence")),
                    ("description", s("")),
                    ("relating_document", doc.a()),
                    ("related_product", pdf.a()),
                ],
            )?;
        }
        Ok(())
    }

    fn datum_carrier_exemption(&self, em: &mut Emitter<'_>, aspect: R, feature: R) {
        if let R::New(id) = aspect {
            let entry = (self.id, "kept-carrier-datum-feature", R::New(id), feature);
            if !em.carrier_exemptions.contains(&entry) {
                em.carrier_exemptions.push(entry);
            }
        }
    }

    /// The instance of feature `i` (memoised; features of equal content share one).
    fn feature(
        &self,
        em: &mut Emitter<'_>,
        st: &mut Emitted,
        pds: R,
        i: usize,
    ) -> Result<R, WriteError> {
        let c = self.canonical[i];
        if let Some(&r) = st.features.get(&c) {
            return Ok(r);
        }
        // A datum of add resolved to the part's existing datum: its datum feature.
        for (d, &(_, fx)) in &self.existing_datums {
            if let (Some(f), Some(fx)) = (self.pmi.datums[*d].feature(), fx)
                && self.canonical[f.0] == c
            {
                let r = em.reference(fx)?;
                st.features.insert(c, r);
                return Ok(r);
            }
        }
        let description = self
            .pmi
            .tolerances
            .iter()
            .filter_map(|t| t.zone.as_ref())
            .find(|z| z.affected_plane.is_some_and(|f| self.canonical[f.0] == c))
            .and_then(|z| z.affected_plane_kind)
            .map_or("", AffectedPlaneKind::description);
        let base = |pd: bool| {
            vec![
                (("shape_aspect", "name"), s("")),
                (("shape_aspect", "description"), s(description)),
                (("shape_aspect", "of_shape"), pds.a()),
                (("shape_aspect", "product_definitional"), boolean(pd)),
            ]
        };
        let r = match &self.pmi.features[c] {
            Feature::Items(items) => {
                let role = self.roles.get(&c).copied().unwrap_or(Role::Plain);
                // §6.5.3 is a semantic dimension as well as a datum feature. Keep
                // that combined record over a kept geometry carrier: promoting the
                // carrier itself would turn its existing feature controls into
                // dimension controls on readback.
                let combined_size =
                    !cfg!(feature = "presentation") && matches!(role, Role::DatumFeatureOfSize(_));
                // CAx-IF PMI v4.1 §5.1 / §6.5: reuse an exact kept carrier, including
                // datum features, instead of a faceless forwarding aspect and SAR.
                let keys = self.usage_keys(em, st, items)?;
                if let Some((rep, _)) = keys.first()
                    && keys.iter().all(|(r, _)| r == rep)
                {
                    let all: Vec<_> = keys
                        .iter()
                        .flat_map(|(_, items)| items.iter().copied())
                        .collect();
                    if let Some(owner) = self.existing_owner(em, *rep, &all, None)? {
                        let compatible = !combined_size
                            && match (role, owner) {
                                (Role::Plain, _) => !self.kept_needs_independence(em, owner),
                                (Role::DatumFeature | Role::DatumFeatureOfSize(_), R::Old(id))
                                    if is_a(self.doc, id, "datum_feature")
                                        && attr(
                                            self.doc,
                                            id,
                                            "shape_aspect",
                                            "product_definitional",
                                        ) == Some(&boolean(true)) =>
                                {
                                    true
                                }
                                _ => false,
                            };
                        if compatible {
                            st.features.insert(c, owner);
                            return Ok(owner);
                        }
                        if self.is_datum_aspect(c) && !combined_size {
                            let types = if items.len() > 1 {
                                vec!["datum_feature", "composite_shape_aspect"]
                            } else {
                                vec!["datum_feature"]
                            };
                            let r = em.instance(&types, &base(true), &[])?;
                            self.kept_datum_usages(em, st, pds, r, &keys, Some(owner))?;
                            st.features.insert(c, r);
                            return Ok(r);
                        }
                    }
                }
                // A specialized aspect is not an interchangeable feature. Share its
                // geometry through neutral carriers, preserving the specialized scope.
                for (rep, items) in &keys {
                    if self.is_datum_aspect(c) && self.has_kept_usage(em, *rep, items[0]) {
                        continue;
                    }
                    self.kept_item_carrier(
                        em,
                        st,
                        pds,
                        *rep,
                        &items[0],
                        keys.len() == 1 && self.is_datum_aspect(c) && !combined_size,
                    )?;
                }
                if self.is_datum_aspect(c)
                    && !combined_size
                    && keys.len() == 1
                    && let Some(&carrier) = st.shared.get(&keys[0])
                    && st.datum_carriers.contains(&carrier)
                {
                    st.features.insert(c, carrier);
                    return Ok(carrier);
                }
                if role == Role::Plain && keys.len() == 1 {
                    let key = &keys[0];
                    let carrier = if let Some(&carrier) = st.shared.get(key) {
                        carrier
                    } else if let Some(&owner) = st.owners.get(key).filter(|owner| **owner != c) {
                        self.feature(em, st, pds, owner)?
                    } else {
                        let mut attributes = base(true);
                        if !self.referenced.contains(&c) && st.shared_keys.contains(key) {
                            attributes[0].1 = s("haecceity.feature/1");
                        }
                        let carrier = em.instance(&["shape_aspect"], &attributes, &[])?;
                        Self::usage(em, carrier, key.0, key.1[0])?;
                        st.shared.insert(key.clone(), carrier);
                        carrier
                    };
                    st.features.insert(c, carrier);
                    return Ok(carrier);
                }
                // Composite means at least two distinct geometric components. A shared
                // single-face datum remains plain and owns the face's GISU.
                let composite = items.iter().copied().collect::<BTreeSet<_>>().len() > 1;
                let types = |ty| {
                    let mut types = vec![ty];
                    if composite {
                        types.push("composite_shape_aspect");
                    }
                    types
                };
                let r = match role {
                    Role::Plain => em.instance(&types("shape_aspect"), &base(true), &[])?,
                    Role::DatumFeature => em.instance(&types("datum_feature"), &base(true), &[])?,
                    Role::AppliedArea => em.instance(&types("applied_area"), &base(true), &[])?,
                    Role::ThreadRunout => em.instance(&types("thread_runout"), &base(true), &[])?,
                    // Two independently placed callouts need distinct semantic definitions
                    // in one draughting model (IIRU UR2). Keep the datum feature and write
                    // its size as a separate dimensional_size applying to that same feature.
                    #[cfg(feature = "presentation")]
                    Role::DatumFeatureOfSize(_) => {
                        em.instance(&types("datum_feature"), &base(true), &[])?
                    }
                    #[cfg(not(feature = "presentation"))]
                    Role::DatumFeatureOfSize(d) => {
                        let DimensionKind::Size { kind, .. } = &self.pmi.dimensions[d].kind else {
                            return Err(WriteError::Internal(
                                "a size dimension that is not".into(),
                            ));
                        };
                        let me = em.next_ref();
                        let mut vals = base(true);
                        vals.push((("dimensional_size", "applies_to"), me));
                        vals.push((("dimensional_size", "name"), s(kind.name())));
                        let r =
                            em.instance(&types("dimensional_size_with_datum_feature"), &vals, &[])?;
                        st.dimensions.insert(d, r);
                        r
                    }
                };
                st.features.insert(c, r);
                if self.is_datum_aspect(c)
                    && keys
                        .iter()
                        .any(|(rep, items)| self.has_kept_usage(em, *rep, items[0]))
                {
                    self.kept_datum_usages(em, st, pds, r, &keys, None)?;
                } else {
                    self.usages(em, st, pds, r, Some(c), items)?;
                }
                r
            }
            Feature::Group { members, kind } => {
                if !self.kept_groups.is_empty() {
                    let wanted = Keys {
                        p: self.pmi,
                        stated: true,
                    }
                    .feature(FeatureId(c));
                    let mut owners: Vec<_> = self
                        .kept_groups
                        .iter()
                        .filter(|(id, key)| {
                            **key == wanted
                                && !plain_forwarder(self.doc, **id)
                                && !em.removed.contains(id)
                                && !em.retired.contains(id)
                                && attr_str(self.doc, **id, "shape_aspect", "name").as_deref()
                                    != Some("DIMENSION")
                        })
                        .map(|(id, _)| (!is_a(self.doc, *id, "datum_feature"), *id))
                        .collect();
                    owners.sort_unstable();
                    if let Some((_, id)) = owners.first()
                        && (!self.is_datum_aspect(c)
                            || (is_a(self.doc, *id, "datum_feature")
                                && attr(self.doc, *id, "shape_aspect", "product_definitional")
                                    == Some(&boolean(true))))
                    {
                        let carrier = em.reference(*id)?;
                        st.features.insert(c, carrier);
                        return Ok(carrier);
                    }
                }
                let (ty, desc) = match kind {
                    GroupKind::MultipleElements => {
                        ("composite_group_shape_aspect", "multiple elements")
                    }
                    GroupKind::PatternOfFeatures => {
                        ("composite_group_shape_aspect", "pattern of features")
                    }
                    GroupKind::AllAround => ("all_around_shape_aspect", ""),
                    GroupKind::Between => ("between_shape_aspect", ""),
                    GroupKind::Unstated => {
                        return Err(WriteError::Internal(
                            "an unstated group passed the checks".into(),
                        ));
                    }
                };
                let mut vals = base(true);
                vals[1].1 = s(desc);
                let mut ms = Vec::new();
                for m in members {
                    ms.push(self.feature(em, st, pds, m.0)?);
                }
                let r = if self.roles.get(&c) == Some(&Role::DatumFeature) {
                    em.instance(&[ty, "datum_feature"], &vals, &[])?
                } else {
                    em.instance(&[ty], &vals, &[])?
                };
                if self.is_datum_aspect(c) {
                    let wanted = Keys {
                        p: self.pmi,
                        stated: true,
                    }
                    .feature(FeatureId(c));
                    if let Some((&id, _)) = self
                        .kept_groups
                        .iter()
                        .find(|(id, key)| **key == wanted && !em.removed.contains(id))
                    {
                        self.datum_carrier_exemption(em, r, em.reference(id)?);
                    }
                }
                for m in ms {
                    em.simple(
                        "shape_aspect_relationship",
                        &[
                            ("name", s("")),
                            ("description", s("")),
                            ("relating_shape_aspect", r.a()),
                            ("related_shape_aspect", m.a()),
                        ],
                    )?;
                }
                st.features.insert(c, r);
                r
            }
            Feature::Derived { kind, from, .. } => {
                let ty = match kind {
                    DerivedKind::Derived => "derived_shape_aspect",
                    DerivedKind::Apex => "apex",
                    DerivedKind::CentreOfSymmetry => "centre_of_symmetry",
                    DerivedKind::GeometricAlignment => "geometric_alignment",
                    DerivedKind::PerpendicularTo => "perpendicular_to",
                    DerivedKind::Extension => "extension",
                    DerivedKind::Tangent => "tangent",
                    DerivedKind::ParallelOffset => "parallel_offset",
                };
                let mut bases = Vec::new();
                for f in from {
                    bases.push(self.feature(em, st, pds, f.0)?);
                }
                let r = em.instance(&[ty], &base(false), &[])?;
                for b in bases {
                    em.simple(
                        "shape_aspect_deriving_relationship",
                        &[
                            ("name", s("")),
                            ("description", s("")),
                            ("relating_shape_aspect", r.a()),
                            ("related_shape_aspect", b.a()),
                        ],
                    )?;
                }
                if let Feature::Derived {
                    geometry: Some(g), ..
                } = &self.pmi.features[c]
                {
                    let items: Vec<Anchor> = self.pmi.derived_geometry[g.0]
                        .supplemental
                        .iter()
                        .copied()
                        .map(Anchor::Geometry)
                        .collect();
                    self.usages(em, st, pds, r, None, &items)?;
                }
                st.features.insert(c, r);
                r
            }
        };
        Ok(r)
    }

    fn is_datum_aspect(&self, c: usize) -> bool {
        matches!(
            self.roles.get(&c),
            Some(Role::DatumFeature | Role::DatumFeatureOfSize(_))
        )
    }

    /// The instance of feature `f` in `role`: the feature's own, or its second instance of
    /// that type (an applied area or thread runout on faces that are also another feature).
    fn role_instance(
        &self,
        em: &mut Emitter<'_>,
        st: &mut Emitted,
        pds: R,
        f: FeatureId,
        role: Role,
    ) -> Result<R, WriteError> {
        let c = self.canonical[f.0];
        if !self.twins.contains(&(c, role)) {
            return self.feature(em, st, pds, f.0);
        }
        if let Some(&r) = st.twins.get(&(c, role)) {
            return Ok(r);
        }
        let Feature::Items(items) = &self.pmi.features[c] else {
            return Err(WriteError::Internal(
                "a twin of a feature that is not items".into(),
            ));
        };
        let ty = if role == Role::AppliedArea {
            "applied_area"
        } else {
            "thread_runout"
        };
        let mut types = vec![ty];
        if items.iter().copied().collect::<BTreeSet<_>>().len() > 1 {
            types.push("composite_shape_aspect");
        }
        let r = em.instance(
            &types,
            &[
                (("shape_aspect", "name"), s("")),
                (("shape_aspect", "description"), s("")),
                (("shape_aspect", "of_shape"), pds.a()),
                (("shape_aspect", "product_definitional"), boolean(true)),
            ],
            &[],
        )?;
        st.twins.insert((c, role), r);
        self.usages(em, st, pds, r, None, items)?;
        Ok(r)
    }

    /// The (representation, item) keys of a feature's usages: one per item, in model order
    /// (one `geometric_item_specific_usage` per face, maintainer decision 2026-10-09).
    fn usage_keys(
        &self,
        em: &mut Emitter<'_>,
        st: &Emitted,
        items: &[Anchor],
    ) -> Result<Vec<(R, Vec<R>)>, WriteError> {
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        for a in items {
            if !seen.insert(*a) {
                continue;
            }
            let (item, rep) = match *a {
                Anchor::Face(f) => {
                    let id = self.def.faces[f.0];
                    {
                        let rep = self.rep_of(em, id)?;
                        (em.reference(id)?, em.reference(rep)?)
                    }
                }
                Anchor::Edge(e) => {
                    let id = self.def.edges[e.0];
                    {
                        let rep = self.rep_of(em, id)?;
                        (em.reference(id)?, em.reference(rep)?)
                    }
                }
                Anchor::Geometry(g) => {
                    let (r, rep) = st.geometry[&g.0];
                    let rep = match (rep, r) {
                        (Some(rep), _) => rep,
                        (None, R::Old(id)) => {
                            em.reference(geometry_rep(self.doc, id).ok_or_else(|| {
                                WriteError::Internal(format!(
                                    "geometry #{id} is in no representation"
                                ))
                            })?)?
                        }
                        (None, R::New(_)) => {
                            return Err(WriteError::Internal(
                                "new geometry without its representation".into(),
                            ));
                        }
                    };
                    (r, rep)
                }
            };
            out.push((rep, vec![item]));
        }
        Ok(out)
    }

    /// The usage of `item` in `rep` defining `definition`: a `geometric_item_specific_usage`.
    fn usage(em: &mut Emitter<'_>, definition: R, rep: R, item: R) -> Result<R, WriteError> {
        em.direct_gisu.insert(definition);
        em.simple(
            "geometric_item_specific_usage",
            &[
                ("name", s("")),
                ("description", s("")),
                ("definition", definition.a()),
                ("used_representation", rep.a()),
                ("identified_item", item.a()),
            ],
        )
    }

    fn kept_usage_owners(&self, em: &Emitter<'_>, rep: R, item: R) -> Vec<u64> {
        let (R::Old(rep), R::Old(item)) = (rep, item) else {
            return Vec::new();
        };
        self.doc
            .referrers(item)
            .iter()
            .copied()
            .filter(|u| {
                !em.removed.contains(u)
                    && !em.retired.contains(u)
                    && is_a(self.doc, *u, "item_identified_representation_usage")
                    && !is_a(self.doc, *u, "draughting_model_item_association")
                    && attr_ref(
                        self.doc,
                        *u,
                        "item_identified_representation_usage",
                        "used_representation",
                    ) == Some(rep)
            })
            .filter_map(|u| {
                attr_ref(
                    self.doc,
                    u,
                    "item_identified_representation_usage",
                    "definition",
                )
            })
            .filter(|owner| {
                attr_ref(self.doc, *owner, "shape_aspect", "of_shape") == Some(self.def.shape)
            })
            .collect()
    }

    fn kept_face_carrier(
        &self,
        em: &Emitter<'_>,
        rep: R,
        item: R,
    ) -> Result<Option<R>, WriteError> {
        if let Some(owner) = self.existing_owner(em, rep, &[item], None)? {
            return Ok(Some(owner));
        }
        let mut owners = self.kept_usage_owners(em, rep, item);
        owners.sort_by_key(|id| (!reusable_aspect(self.doc, *id), *id));
        owners.first().map(|id| em.reference(*id)).transpose()
    }

    fn has_kept_usage(&self, em: &Emitter<'_>, rep: R, item: R) -> bool {
        !self.kept_usage_owners(em, rep, item).is_empty()
    }

    /// Functional outgoing SARs make OCCT collect unrelated geometry from a
    /// neutral carrier. Isolate the new requirement, never edit the kept SAR.
    fn kept_functional_sars(&self, owner: R) -> Vec<u64> {
        let R::Old(owner) = owner else {
            return Vec::new();
        };
        self.doc
            .referrers(owner)
            .iter()
            .copied()
            .filter(|r| {
                attr_ref(
                    self.doc,
                    *r,
                    "shape_aspect_relationship",
                    "relating_shape_aspect",
                ) == Some(owner)
                    && (is_a(self.doc, *r, "feature_for_datum_target_relationship")
                        || self.doc.referrers(*r).iter().any(|t| {
                            attr_ref(
                                self.doc,
                                *t,
                                "geometric_tolerance",
                                "toleranced_shape_aspect",
                            ) == Some(*r)
                        }))
            })
            .collect()
    }

    fn kept_needs_independence(&self, em: &Emitter<'_>, owner: R) -> bool {
        em.preserve_kept
            && !self.occt_record_stops_at(em, owner)
            && !self.kept_functional_sars(owner).is_empty()
    }

    fn independent_kept_usage(
        &self,
        em: &mut Emitter<'_>,
        member: R,
        item: R,
        carrier: R,
        condition: &'static str,
    ) -> Result<(), WriteError> {
        let own_rep = em.simple(
            "shape_representation",
            &[
                ("name", s("")),
                ("items", refs([item])),
                ("context_of_items", em.reference(self.context)?.a()),
            ],
        )?;
        Self::usage(em, member, own_rep, item)?;
        // The kept carrier gains only an incoming-use backlink to the same
        // geometry/subset. The new aspect has no outgoing path through a kept
        // functional SAR, which would confuse OCCT's ordinary aspect traversal.
        em.simple(
            "shape_aspect_relationship",
            &[
                ("name", s("")),
                ("description", s("")),
                ("relating_shape_aspect", carrier.a()),
                ("related_shape_aspect", member.a()),
            ],
        )?;
        em.carrier_exemptions
            .push((self.id, condition, member, carrier));
        for sar in self.kept_functional_sars(carrier) {
            em.preserved_normalizations
                .insert((sar, "functional-sar-isolation"));
        }
        Ok(())
    }

    /// Each kept face gets an independent aspect/usage; untouched faces use the
    /// normal shared-carrier path. Reports name the actual per-face aspect and
    /// one deterministically selected source carrier, not the multi-face root.
    fn kept_datum_usages(
        &self,
        em: &mut Emitter<'_>,
        st: &mut Emitted,
        pds: R,
        feature: R,
        keys: &[(R, Vec<R>)],
        carrier: Option<R>,
    ) -> Result<(), WriteError> {
        if keys.len() > 1
            && let Some(carrier) = carrier
        {
            em.carrier_exemptions
                .push((self.id, "kept-carrier-datum-composite", feature, carrier));
        }
        for (rep, face_items) in keys {
            let item = face_items[0];
            if let Some(carrier) = self.kept_face_carrier(em, *rep, item)? {
                let member = if keys.len() == 1 {
                    feature
                } else {
                    let member = em.simple(
                        "shape_aspect",
                        &[
                            ("name", s("")),
                            ("description", s("")),
                            ("of_shape", pds.a()),
                            ("product_definitional", boolean(true)),
                        ],
                    )?;
                    em.simple(
                        "shape_aspect_relationship",
                        &[
                            ("name", s("")),
                            ("description", s("")),
                            ("relating_shape_aspect", feature.a()),
                            ("related_shape_aspect", member.a()),
                        ],
                    )?;
                    member
                };
                let condition = if keys.len() == 1 {
                    "kept-carrier-datum-feature"
                } else {
                    "kept-carrier-datum-member"
                };
                self.independent_kept_usage(em, member, item, carrier, condition)?;
            } else {
                let key = (*rep, face_items.clone());
                let member = if keys.len() == 1 {
                    feature
                } else if let Some(&member) = st.shared.get(&key) {
                    member
                } else if let Some(&owner) = st.owners.get(&key) {
                    self.feature(em, st, pds, owner)?
                } else {
                    let member = em.simple(
                        "shape_aspect",
                        &[
                            ("name", s("")),
                            ("description", s("")),
                            ("of_shape", pds.a()),
                            ("product_definitional", boolean(true)),
                        ],
                    )?;
                    Self::usage(em, member, *rep, item)?;
                    st.shared.insert(key, member);
                    member
                };
                if member == feature {
                    Self::usage(em, member, *rep, item)?;
                } else {
                    em.simple(
                        "shape_aspect_relationship",
                        &[
                            ("name", s("")),
                            ("description", s("")),
                            ("relating_shape_aspect", feature.a()),
                            ("related_shape_aspect", member.a()),
                        ],
                    )?;
                }
            }
        }
        Ok(())
    }

    /// The usages of a feature's items: one `geometric_item_specific_usage` per item
    /// (maintainer decision 2026-10-09, which OpenCascade reads; the PMI practice's §5.1
    /// Figure 5 alternative, one `item_identified_representation_usage` of a
    /// `set_representation_item`, it drops). An aspect has one usage per representation
    /// (item_identified_representation_usage UR2), so a feature of several items in one
    /// representation identifies each through a member shape aspect of that one item, which
    /// it is composed of (`shape_aspect_relationship`, §5.1: one shape aspect and usage per
    /// face, shared by every feature on that face); the reader reads the composition as the
    /// feature's items. One item is identified once per representation (UR1): an item several
    /// features share, or that an existing shape aspect of the file already identifies, is
    /// such a member too, owned by a referenced feature of that item alone where there is one.
    /// A kept plain shape aspect of the file that identifies exactly all the feature's items
    /// in one representation by one usage (as earlier writes and §6.5.1 identify several faces)
    /// is the feature's one member instead.
    fn usages(
        &self,
        em: &mut Emitter<'_>,
        st: &mut Emitted,
        pds: R,
        feature: R,
        own: Option<usize>,
        items: &[Anchor],
    ) -> Result<(), WriteError> {
        let keys = self.usage_keys(em, st, items)?;
        let several = keys.len() > 1;
        for (rep, items) in keys {
            let [item] = items[..] else {
                return Err(WriteError::Internal("a usage key of several items".into()));
            };
            let key = (rep, items);
            let member = if let Some(&m) = st.shared.get(&key) {
                Some(m)
            } else if let Some(m) = self.kept_item_carrier(em, st, pds, rep, &item, false)? {
                // A face the file already identifies keeps its one usage, whichever new feature
                // would own it: every feature on it is composed of the file's aspect (UR1).
                st.shared.insert(key.clone(), m);
                Some(m)
            } else if st.shared_keys.contains(&key)
                && own == st.owners.get(&key).copied()
                && own.is_some()
            {
                // This feature owns the shared item: the others are composed of it.
                Self::usage(em, feature, rep, item)?;
                st.shared.insert(key, feature);
                continue;
            } else if let Some(&o) = st.owners.get(&key) {
                Some(self.feature(em, st, pds, o)?)
            } else if st.shared_keys.contains(&key) || several {
                let m = em.simple(
                    "shape_aspect",
                    &[
                        ("name", s("")),
                        ("description", s("")),
                        ("of_shape", pds.a()),
                        ("product_definitional", boolean(true)),
                    ],
                )?;
                Self::usage(em, m, rep, item)?;
                st.shared.insert(key, m);
                Some(m)
            } else {
                None
            };
            match member {
                Some(m) => {
                    em.simple(
                        "shape_aspect_relationship",
                        &[
                            ("name", s("")),
                            ("description", s("")),
                            ("relating_shape_aspect", feature.a()),
                            ("related_shape_aspect", m.a()),
                        ],
                    )?;
                }
                None => {
                    Self::usage(em, feature, rep, item)?;
                }
            }
        }
        Ok(())
    }

    /// Neutralize a kept usage only when no interchangeable aspect already owns the
    /// requested item. Semantic scopes keep their identity and compose the neutral leaves.
    fn kept_item_carrier(
        &self,
        em: &mut Emitter<'_>,
        st: &mut Emitted,
        pds: R,
        rep: R,
        item: &R,
        datum: bool,
    ) -> Result<Option<R>, WriteError> {
        let key = (rep, vec![*item]);
        if let Some(&r) = st.shared.get(&key) {
            return Ok(Some(r));
        }
        if let Some(r) = self.existing_owner(em, rep, &[*item], None)?
            && !self.kept_needs_independence(em, r)
        {
            st.shared.insert(key, r);
            return Ok(Some(r));
        }
        if em.preserve_kept
            && let Some(owner) = self.kept_face_carrier(em, rep, *item)?
        {
            let leaf = em.simple(
                "shape_aspect",
                &[
                    ("name", s("")),
                    ("description", s("")),
                    ("of_shape", pds.a()),
                    ("product_definitional", boolean(true)),
                ],
            )?;
            self.independent_kept_usage(
                em,
                leaf,
                *item,
                owner,
                "kept-carrier-independent-feature",
            )?;
            st.shared.insert(key, leaf);
            return Ok(Some(leaf));
        }
        let (R::Old(rep_id), R::Old(item_id)) = (rep, *item) else {
            return Ok(None);
        };
        for &usage in self.doc.referrers(item_id) {
            if em.removed.contains(&usage)
                || em.retired.contains(&usage)
                || em.kept_rewrites.contains_key(&usage)
                || !is_a(self.doc, usage, "item_identified_representation_usage")
                || is_a(self.doc, usage, "draughting_model_item_association")
                || attr_ref(
                    self.doc,
                    usage,
                    "item_identified_representation_usage",
                    "used_representation",
                ) != Some(rep_id)
            {
                continue;
            }
            let Some(owner) = attr_ref(
                self.doc,
                usage,
                "item_identified_representation_usage",
                "definition",
            ) else {
                continue;
            };
            if attr_ref(self.doc, owner, "shape_aspect", "of_shape") != Some(self.def.shape) {
                continue;
            }
            let items = attr_refs(
                self.doc,
                usage,
                "item_identified_representation_usage",
                "identified_item",
            );
            for (n, id) in items.into_iter().enumerate() {
                let is_datum = datum && id == item_id;
                let leaf = em.simple(
                    if is_datum {
                        "datum_feature"
                    } else {
                        "shape_aspect"
                    },
                    &[
                        ("name", s("")),
                        ("description", s("")),
                        ("of_shape", pds.a()),
                        ("product_definitional", boolean(true)),
                    ],
                )?;
                if n == 0 {
                    em.direct_gisu.insert(leaf);
                    em.kept_rewrites.insert(
                        usage,
                        p21::simple(
                            "GEOMETRIC_ITEM_SPECIFIC_USAGE",
                            vec![
                                attr(
                                    self.doc,
                                    usage,
                                    "item_identified_representation_usage",
                                    "name",
                                )
                                .cloned()
                                .unwrap_or_else(|| s("")),
                                attr(
                                    self.doc,
                                    usage,
                                    "item_identified_representation_usage",
                                    "description",
                                )
                                .cloned()
                                .unwrap_or_else(|| s("")),
                                leaf.a(),
                                rep.a(),
                                em.reference(id)?.a(),
                            ],
                        ),
                    );
                } else {
                    Self::usage(em, leaf, rep, em.reference(id)?)?;
                }
                em.simple(
                    "shape_aspect_relationship",
                    &[
                        ("name", s("")),
                        ("description", s("")),
                        ("relating_shape_aspect", em.reference(owner)?.a()),
                        ("related_shape_aspect", leaf.a()),
                    ],
                )?;
                st.shared.insert((rep, vec![em.reference(id)?]), leaf);
                if is_datum {
                    st.datum_carriers.insert(leaf);
                }
            }
            return Ok(st.shared.get(&key).copied());
        }
        Ok(None)
    }

    /// Preserve legacy endpoint wrappers and report the normalization Add forgoes.
    /// The source alias and its dimensions retain their original identities.
    fn preserve_legacy_endpoints(&self, em: &mut Emitter<'_>) -> Result<(), WriteError> {
        for alias in self.doc.ids() {
            if em.removed.contains(&alias)
                || em.retired.contains(&alias)
                || is_a(self.doc, alias, "datum_feature")
                || attr_str(self.doc, alias, "shape_aspect", "name").as_deref() != Some("DIMENSION")
                || attr_ref(self.doc, alias, "shape_aspect", "of_shape") != Some(self.def.shape)
            {
                continue;
            }
            let mut todo = vec![alias];
            let mut visited = BTreeSet::new();
            let mut by_rep: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
            let mut relations = Vec::new();
            let mut direct = false;
            while let Some(aspect) = todo.pop() {
                if !visited.insert(aspect) {
                    continue;
                }
                for &r in self.doc.referrers(aspect) {
                    if em.removed.contains(&r) {
                        continue;
                    }
                    if is_a(self.doc, r, "item_identified_representation_usage")
                        && !is_a(self.doc, r, "draughting_model_item_association")
                        && attr_ref(
                            self.doc,
                            r,
                            "item_identified_representation_usage",
                            "definition",
                        ) == Some(aspect)
                        && let Some(rep) = attr_ref(
                            self.doc,
                            r,
                            "item_identified_representation_usage",
                            "used_representation",
                        )
                    {
                        direct |= aspect == alias;
                        by_rep.entry(rep).or_default().extend(attr_refs(
                            self.doc,
                            r,
                            "item_identified_representation_usage",
                            "identified_item",
                        ));
                    }
                    if geometric_composition(self.doc, r)
                        && attr_ref(
                            self.doc,
                            r,
                            "shape_aspect_relationship",
                            "relating_shape_aspect",
                        ) == Some(aspect)
                        && let Some(member) = attr_ref(
                            self.doc,
                            r,
                            "shape_aspect_relationship",
                            "related_shape_aspect",
                        )
                        && !is_a(self.doc, member, "datum")
                    {
                        if aspect == alias {
                            relations.push(r);
                        }
                        todo.push(member);
                    }
                }
            }
            if direct || by_rep.len() != 1 {
                continue;
            }
            let (rep, items) = by_rep
                .into_iter()
                .next()
                .ok_or_else(|| WriteError::Internal("missing legacy geometry".into()))?;
            let items: Vec<_> = items.into_iter().map(R::Old).collect();
            let Some(R::Old(owner)) = self.existing_owner(
                em,
                R::Old(rep),
                &items,
                self.kept_groups.get(&alias).map(String::as_str),
            )?
            else {
                continue;
            };
            if !neutral_aspect(self.doc, alias) && !is_a(self.doc, owner, "datum_feature") {
                continue;
            }
            em.preserved_normalizations
                .insert((alias, "legacy-endpoint-retirement"));
            em.carrier_exemptions.push((
                self.id,
                "kept-carrier-legacy-forwarder",
                R::Old(alias),
                R::Old(owner),
            ));
        }
        Ok(())
    }

    /// A kept aspect whose complete geometric content is exactly `items` in `rep`.
    /// Walk upward from the first item's GISU/IIRU, then compare whole sets: a proper
    /// subset of a datum must never alias its parent (§6.5.1). Composition is allowed,
    /// but datum-establishment links and dimensional locations are not composition.
    fn existing_owner(
        &self,
        em: &Emitter<'_>,
        rep: R,
        items: &[R],
        group_key: Option<&str>,
    ) -> Result<Option<R>, WriteError> {
        let (R::Old(rep), Some(R::Old(first))) = (rep, items.first().copied()) else {
            return Ok(None);
        };
        let want: BTreeSet<u64> = items
            .iter()
            .filter_map(|r| match r {
                R::Old(id) => Some(*id),
                R::New(_) => None,
            })
            .collect();
        if want.len() != items.len() {
            return Ok(None);
        }
        let doc = self.doc;
        let mut pending: Vec<u64> = doc
            .referrers(first)
            .iter()
            .copied()
            .filter(|u| {
                !em.removed.contains(u) && is_a(doc, *u, "item_identified_representation_usage")
            })
            .filter_map(|u| attr_ref(doc, u, "item_identified_representation_usage", "definition"))
            .collect();
        let mut seen = BTreeSet::new();
        let mut matches = Vec::new();
        while let Some(owner) = pending.pop() {
            if !seen.insert(owner)
                || em.removed.contains(&owner)
                || em.retired.contains(&owner)
                || !is_a(doc, owner, "shape_aspect")
                || is_a(doc, owner, "datum")
                || is_a(doc, owner, "datum_target")
                || attr_ref(doc, owner, "shape_aspect", "of_shape") != Some(self.def.shape)
            {
                continue;
            }
            let mut got = BTreeSet::new();
            let mut todo = vec![owner];
            let mut visited = BTreeSet::new();
            let mut valid = true;
            while let Some(aspect) = todo.pop() {
                if !visited.insert(aspect) {
                    continue;
                }
                for &r in doc.referrers(aspect) {
                    if em.removed.contains(&r) {
                        continue;
                    }
                    if is_a(doc, r, "item_identified_representation_usage")
                        && !is_a(doc, r, "draughting_model_item_association")
                        && attr_ref(doc, r, "item_identified_representation_usage", "definition")
                            == Some(aspect)
                    {
                        let usage_rep = attr_ref(
                            doc,
                            r,
                            "item_identified_representation_usage",
                            "used_representation",
                        );
                        // A previous append-only write may have attached this
                        // same face in an independent representation. Reuse its
                        // carrier on reopen when the part context still agrees.
                        valid &= usage_rep == Some(rep)
                            || usage_rep.is_some_and(|other| {
                                is_a(doc, other, "shape_representation")
                                    && attr_ref(doc, other, "representation", "context_of_items")
                                        == attr_ref(doc, rep, "representation", "context_of_items")
                            });
                        got.extend(attr_refs(
                            doc,
                            r,
                            "item_identified_representation_usage",
                            "identified_item",
                        ));
                    }
                    if geometric_composition(doc, r)
                        && attr_ref(doc, r, "shape_aspect_relationship", "relating_shape_aspect")
                            == Some(aspect)
                        && let Some(member) =
                            attr_ref(doc, r, "shape_aspect_relationship", "related_shape_aspect")
                        && !is_a(doc, member, "datum")
                    {
                        todo.push(member);
                    }
                }
            }
            let compatible = if let Some(key) = group_key {
                self.kept_groups.get(&owner).is_some_and(|old| old == key)
                    && !plain_forwarder(doc, owner)
                    && attr_str(doc, owner, "shape_aspect", "name").as_deref() != Some("DIMENSION")
            } else {
                reusable_aspect(doc, owner)
                    && !plain_forwarder(doc, owner)
                    && !self.kept_groups.contains_key(&owner)
            };
            if valid && got == want && compatible {
                matches.push((!is_a(doc, owner, "datum_feature"), owner));
            }
            for &r in doc.referrers(owner) {
                if !em.removed.contains(&r)
                    && geometric_composition(doc, r)
                    && attr_ref(doc, r, "shape_aspect_relationship", "related_shape_aspect")
                        == Some(owner)
                    && let Some(parent) =
                        attr_ref(doc, r, "shape_aspect_relationship", "relating_shape_aspect")
                {
                    pending.push(parent);
                }
            }
        }
        matches.sort_unstable();
        matches.first().map(|(_, id)| em.reference(*id)).transpose()
    }

    fn dimension(
        &self,
        em: &mut Emitter<'_>,
        st: &mut Emitted,
        pds: R,
        i: usize,
    ) -> Result<R, WriteError> {
        let d = &self.pmi.dimensions[i];
        if let Some((&f, _)) = self
            .roles
            .iter()
            .find(|(_, r)| **r == Role::DatumFeatureOfSize(i))
        {
            self.feature(em, st, pds, f)?;
        }
        let dim = match st.dimensions.get(&i) {
            Some(&r) => r,
            None => {
                let sel = |a: &Option<AngleSelection>| {
                    a.map(|a| {
                        en(match a {
                            AngleSelection::Equal => "equal",
                            AngleSelection::Large => "large",
                            AngleSelection::Small => "small",
                        })
                    })
                };
                let r = match &d.kind {
                    DimensionKind::Size {
                        feature,
                        kind,
                        path,
                        angle,
                    } => {
                        let f = self.feature(em, st, pds, feature.0)?;
                        let p = match path {
                            Some(p) => Some(self.feature(em, st, pds, p.0)?),
                            None => None,
                        };
                        let mut types = vec!["dimensional_size"];
                        let mut vals = vec![
                            (("dimensional_size", "applies_to"), f.a()),
                            (("dimensional_size", "name"), s(kind.name())),
                        ];
                        if let Some(a) = sel(angle) {
                            types.push("angular_size");
                            vals.push((("angular_size", "angle_selection"), a));
                        }
                        if let Some(p) = p {
                            types.push("dimensional_size_with_path");
                            vals.push((("dimensional_size_with_path", "path"), p.a()));
                        }
                        em.instance(&types, &vals, &[])?
                    }
                    DimensionKind::Location {
                        from,
                        to,
                        kind,
                        path,
                        directed,
                        angle,
                    } => {
                        let a = self.feature(em, st, pds, from.0)?;
                        let b = self.feature(em, st, pds, to.0)?;
                        let p = match path {
                            Some(p) => Some(self.feature(em, st, pds, p.0)?),
                            None => None,
                        };
                        let mut types = vec!["dimensional_location"];
                        let mut vals = vec![
                            (("shape_aspect_relationship", "name"), s(kind.name())),
                            (("shape_aspect_relationship", "description"), s("")),
                            (
                                ("shape_aspect_relationship", "relating_shape_aspect"),
                                a.a(),
                            ),
                            (("shape_aspect_relationship", "related_shape_aspect"), b.a()),
                        ];
                        if let Some(x) = sel(angle) {
                            types.push("angular_location");
                            vals.push((("angular_location", "angle_selection"), x));
                        }
                        if let Some(p) = p {
                            types.push("dimensional_location_with_path");
                            vals.push((("dimensional_location_with_path", "path"), p.a()));
                        }
                        if *directed {
                            types.push("directed_dimensional_location");
                        }
                        em.instance(&types, &vals, &[])?
                    }
                };
                st.dimensions.insert(i, r);
                r
            }
        };
        if st.dimension_values.contains(&i) {
            return Ok(dim);
        }
        st.dimension_values.insert(i);

        // The value representation (§5.2).
        let mut items = Vec::new();
        if let Some(n) = &d.nominal {
            let mut q = Vec::new();
            if let Some(qual) = &d.qualifier {
                let name = match qual {
                    Qualifier::Maximum => "maximum",
                    Qualifier::Minimum => "minimum",
                    Qualifier::Average => "average",
                    Qualifier::Other(o) => o.as_str(),
                };
                q.push(em.simple("type_qualifier", &[("name", s(name))])?);
            }
            items.push(em.measure_item(
                "nominal value",
                measure_of_value(n),
                self.context,
                q,
                n.decimal_places,
            )?);
        }
        let limits = match &d.tolerance {
            DimTolerance::Limits(b) => Some(b),
            DimTolerance::Fit { limits, .. } => limits.as_ref(),
            _ => None,
        };
        if let Some(b) = limits {
            items.push(em.measure_item(
                "upper limit",
                measure_of_value(b.upper()),
                self.context,
                Vec::new(),
                b.upper().decimal_places,
            )?);
            items.push(em.measure_item(
                "lower limit",
                measure_of_value(b.lower()),
                self.context,
                Vec::new(),
                b.lower().decimal_places,
            )?);
        }
        if d.tolerance == DimTolerance::Basic {
            items.push(em.simple(
                "descriptive_representation_item",
                &[
                    ("name", s("dimensional note")),
                    ("description", s("theoretical")),
                ],
            )?);
        }
        if d.modifiers.contains(&DimensionModifier::Reference) {
            items.push(em.simple(
                "descriptive_representation_item",
                &[
                    ("name", s("dimensional note")),
                    ("description", s("auxiliary")),
                ],
            )?);
        }
        let mut type2 = Vec::new();
        for m in &d.modifiers {
            if let Some((text, _)) = DimensionModifier::TABLE.iter().find(|(_, x)| x == m) {
                type2.push(em.simple(
                    "descriptive_representation_item",
                    &[("name", s("dimensional note")), ("description", s(text))],
                )?);
            }
        }
        if !type2.is_empty() {
            items.push(em.simple(
                "compound_representation_item",
                &[
                    ("name", s("modifiers")),
                    (
                        "item_element",
                        typed("set_representation_item", refs(type2)),
                    ),
                ],
            )?);
        }
        let principle = match d.principle {
            None => "",
            Some(Principle::Independency) => "independency",
            Some(Principle::EnvelopeRequirement) => "envelope requirement",
        };
        let sdr = em.simple(
            "shape_dimension_representation",
            &[
                ("name", s(principle)),
                ("items", refs(items)),
                ("context_of_items", em.reference(self.context)?.a()),
            ],
        )?;
        em.simple(
            "dimensional_characteristic_representation",
            &[("dimension", dim.a()), ("representation", sdr.a())],
        )?;
        let range = match &d.tolerance {
            DimTolerance::Deviations(b) => Some(em.tolerance_value(self.context, b)?),
            DimTolerance::Fit { class, .. } => Some(em.limits_and_fits(class)?),
            _ => None,
        };
        if let Some(range) = range {
            em.simple(
                "plus_minus_tolerance",
                &[("range", range.a()), ("toleranced_dimension", dim.a())],
            )?;
        }
        Ok(dim)
    }

    /// Semantic feature roots that readers must still be able to collect independently.
    /// Controlled-pair endpoints and datum-target supports are deliberately not roots:
    /// without another characteristic, a direct outgoing SAR needs no exception.
    fn occt_protected_features(&self) -> BTreeSet<usize> {
        occt_semantic_features(self.pmi)
            .into_iter()
            .map(|i| self.canonical[i])
            .collect()
    }

    fn occt_record_stops_at(&self, em: &Emitter<'_>, carrier: R) -> bool {
        match carrier {
            R::New(_) => em.occt_stopping.contains(&carrier),
            R::Old(id) => em
                .kept_rewrites
                .get(&id)
                .or_else(|| self.doc.get(id))
                .is_some_and(occt_stopping_record),
        }
    }

    fn occt_has_direct_gisu(&self, em: &Emitter<'_>, carrier: R) -> bool {
        if em.direct_gisu.contains(&carrier) {
            return true;
        }
        let R::Old(id) = carrier else {
            return false;
        };
        self.doc.referrers(id).iter().any(|r| {
            if em.removed.contains(r)
                || em.retired.contains(r)
                || !is_a(self.doc, *r, "geometric_item_specific_usage")
            {
                return false;
            }
            if let Some(RawEntity::Simple {
                name, attributes, ..
            }) = em.kept_rewrites.get(r)
            {
                name.eq_ignore_ascii_case("geometric_item_specific_usage")
                    && attributes.get(2) == Some(&carrier.a())
            } else {
                attr_ref(
                    self.doc,
                    *r,
                    "item_identified_representation_usage",
                    "definition",
                ) == Some(id)
            }
        })
    }

    fn occt_needs_isolation(&self, em: &Emitter<'_>, st: &Emitted, carrier: R) -> bool {
        st.occt_protected.contains(&carrier) && !self.occt_record_stops_at(em, carrier)
    }

    /// Isolate only shared, non-datum controlled-pair/support endpoints. The carrier
    /// keeps its one GISU; the exception is a private aspect plus one composition SAR.
    fn relationship_endpoint(
        &self,
        em: &mut Emitter<'_>,
        st: &mut Emitted,
        pds: R,
        f: FeatureId,
        condition: Option<&'static str>,
    ) -> Result<R, WriteError> {
        let member = self.feature(em, st, pds, f.0)?;
        if condition.is_none() || self.occt_record_stops_at(em, member) {
            return Ok(member);
        }
        self.isolate_carrier(
            em,
            pds,
            member,
            condition
                .ok_or_else(|| WriteError::Internal("missing shared-carrier condition".into()))?,
        )
    }

    fn isolate_carrier(
        &self,
        em: &mut Emitter<'_>,
        pds: R,
        member: R,
        condition: &'static str,
    ) -> Result<R, WriteError> {
        let endpoint = em.simple(
            "shape_aspect",
            &[
                ("name", s("")),
                ("description", s("")),
                ("of_shape", pds.a()),
                ("product_definitional", boolean(true)),
            ],
        )?;
        em.simple(
            "shape_aspect_relationship",
            &[
                ("name", s("")),
                ("description", s("")),
                ("relating_shape_aspect", endpoint.a()),
                ("related_shape_aspect", member.a()),
            ],
        )?;
        if let R::New(id) = endpoint {
            em.carrier_exemptions
                .push((self.id, condition, R::New(id), member));
        }
        Ok(endpoint)
    }

    fn tolerance(
        &self,
        em: &mut Emitter<'_>,
        st: &mut Emitted,
        pds: R,
        t: &GeometricTolerance,
        names: &mut BTreeSet<String>,
    ) -> Result<R, WriteError> {
        let target = match &t.target {
            ToleranceTarget::Feature(f) => self.feature(em, st, pds, f.0)?,
            ToleranceTarget::Dimension(d) => self.dimension(em, st, pds, d.0)?,
            ToleranceTarget::WholePart => pds,
            ToleranceTarget::Relation {
                relating,
                related,
                name,
            } => {
                let carrier = self.feature(em, st, pds, relating.0)?;
                let condition = self
                    .occt_needs_isolation(em, st, carrier)
                    .then_some("shared-carrier-controlled-pair");
                let a = self.relationship_endpoint(em, st, pds, *relating, condition)?;
                let b = self.relationship_endpoint(em, st, pds, *related, condition)?;
                em.simple(
                    "shape_aspect_relationship",
                    &[
                        ("name", s(name)),
                        ("description", s("")),
                        ("relating_shape_aspect", a.a()),
                        ("related_shape_aspect", b.a()),
                    ],
                )?
            }
        };
        st.tolerance_targets.insert(st.tolerances.len(), target);
        let system = match &t.datums {
            Some(ds) => Some(self.datum_system(em, st, pds, ds, names)?),
            None => None,
        };
        let mut types = vec![t.kind.entity()];
        let mut vals = vec![
            (("geometric_tolerance", "name"), s("")),
            (
                ("geometric_tolerance", "description"),
                s(t.description.as_deref().unwrap_or("")),
            ),
            (
                ("geometric_tolerance", "toleranced_shape_aspect"),
                target.a(),
            ),
        ];
        if let Some(m) = &t.magnitude {
            vals.push((
                ("geometric_tolerance", "magnitude"),
                em.value(self.context, m)?.a(),
            ));
        }
        if let Some(ds) = system {
            types.push("geometric_tolerance_with_datum_reference");
            vals.push((
                ("geometric_tolerance_with_datum_reference", "datum_system"),
                refs([ds]),
            ));
        }
        if !t.modifiers.is_empty() || t.maximum.is_some() {
            types.push("geometric_tolerance_with_modifiers");
            vals.push((
                ("geometric_tolerance_with_modifiers", "modifiers"),
                A::List(t.modifiers.iter().map(|m| en(m.as_enum())).collect()),
            ));
        }
        if let Some(m) = &t.maximum {
            types.push("geometric_tolerance_with_maximum_tolerance");
            vals.push((
                (
                    "geometric_tolerance_with_maximum_tolerance",
                    "maximum_upper_tolerance",
                ),
                em.value(self.context, m)?.a(),
            ));
        }
        if let Some(u) = &t.unit_basis {
            types.push("geometric_tolerance_with_defined_unit");
            vals.push((
                ("geometric_tolerance_with_defined_unit", "unit_size"),
                em.value(self.context, &u.size)?.a(),
            ));
            if let Some(area) = &u.area {
                types.push("geometric_tolerance_with_defined_area_unit");
                let shape = match area.shape {
                    AreaShape::Circular => "circular",
                    AreaShape::Square => "square",
                    AreaShape::Rectangular => "rectangular",
                    AreaShape::Cylindrical => "cylindrical",
                    AreaShape::Spherical => "spherical",
                };
                vals.push((
                    ("geometric_tolerance_with_defined_area_unit", "area_type"),
                    en(shape),
                ));
                if let Some(second) = &area.second {
                    vals.push((
                        (
                            "geometric_tolerance_with_defined_area_unit",
                            "second_unit_size",
                        ),
                        em.value(self.context, second)?.a(),
                    ));
                }
            }
        }
        if let Some(u) = &t.unequal {
            types.push("unequally_disposed_geometric_tolerance");
            vals.push((
                ("unequally_disposed_geometric_tolerance", "displacement"),
                em.value(self.context, u)?.a(),
            ));
        }
        let tol = em.instance(&types, &vals, &[])?;
        if let Some(z) = &t.zone {
            let form = em.simple(
                "tolerance_zone_form",
                &[("name", s(zone_form_name(&z.form)))],
            )?;
            let zone = em.simple(
                "tolerance_zone",
                &[
                    ("name", s("")),
                    ("description", s("")),
                    ("of_shape", pds.a()),
                    ("product_definitional", boolean(false)),
                    ("defining_tolerance", refs([tol])),
                    ("form", form.a()),
                ],
            )?;
            if let Some(f) = z.affected_plane {
                let plane = self.feature(em, st, pds, f.0)?;
                em.simple(
                    "shape_aspect_relationship",
                    &[
                        ("name", s("affected plane association")),
                        ("description", s("affected plane association")),
                        ("relating_shape_aspect", zone.a()),
                        ("related_shape_aspect", plane.a()),
                    ],
                )?;
            }
            if let Some(pz) = &z.projected {
                let end = match pz.end {
                    Some(f) => self.feature(em, st, pds, f.0)?,
                    None => {
                        return Err(WriteError::Internal(
                            "a projected zone without its end passed the checks".into(),
                        ));
                    }
                };
                let len = em.value(self.context, &pz.length)?;
                em.simple(
                    "projected_zone_definition",
                    &[
                        ("zone", zone.a()),
                        ("boundaries", A::List(Vec::new())),
                        ("projection_end", end.a()),
                        ("projected_length", len.a()),
                    ],
                )?;
            }
            if let Some(bs) = &z.non_uniform {
                let mut fs = Vec::new();
                for f in bs {
                    fs.push(self.feature(em, st, pds, f.0)?);
                }
                em.simple(
                    "non_uniform_zone_definition",
                    &[("zone", zone.a()), ("boundaries", refs(fs))],
                )?;
            }
            if let Some(a) = &z.runout_angle {
                let angle = em.value(self.context, a)?;
                let o = em.simple("runout_zone_orientation", &[("angle", angle.a())])?;
                em.simple(
                    "runout_zone_definition",
                    &[
                        ("zone", zone.a()),
                        ("boundaries", A::List(Vec::new())),
                        ("orientation", o.a()),
                    ],
                )?;
            }
        }
        for a in &t.auxiliary {
            let v = match a {
                AuxiliaryClassification::AllOver => "all_over",
                AuxiliaryClassification::UnlessOtherwiseSpecified => "unless_otherwise_specified",
            };
            em.simple(
                "geometric_tolerance_auxiliary_classification",
                &[("attribute_value", en(v)), ("described_item", tol.a())],
            )?;
        }
        Ok(tol)
    }

    /// One `datum_system` per distinct system of the part (a value, §6.9.7), its compartments
    /// its own.
    fn datum_system(
        &self,
        em: &mut Emitter<'_>,
        st: &mut Emitted,
        pds: R,
        ds: &DatumSystem,
        names: &mut BTreeSet<String>,
    ) -> Result<R, WriteError> {
        let key = format!("{ds:?}");
        if let Some(&r) = st.systems.get(&key) {
            return Ok(r);
        }
        let label = |d: DatumId| self.pmi.datums[d.0].label().as_str().to_string();
        let base_name = ds
            .compartments()
            .iter()
            .map(|c| {
                c.references
                    .iter()
                    .map(|r| label(r.datum))
                    .collect::<Vec<_>>()
                    .join("-")
            })
            .collect::<Vec<_>>()
            .join("|");
        let mut name = base_name.clone();
        let mut n = 2;
        while names.contains(&name) {
            name = format!("{base_name} ({n})");
            n += 1;
        }
        names.insert(name.clone());
        let mut compartments = Vec::new();
        for c in ds.compartments() {
            let shape = || -> Vec<((&'static str, &'static str), A)> {
                vec![
                    (("shape_aspect", "name"), s("")),
                    (("shape_aspect", "description"), s("")),
                    (("shape_aspect", "of_shape"), pds.a()),
                    (("shape_aspect", "product_definitional"), boolean(false)),
                ]
            };
            let (base, modifiers) = match c.references.as_slice() {
                [one] => {
                    // One datum: its own modifiers and the compartment's are the same thing.
                    let mut m = c.modifiers.clone();
                    m.extend(one.modifiers.iter().cloned());
                    (st.datums[&one.datum.0].a(), m)
                }
                many => {
                    let mut elements = Vec::new();
                    for r in many {
                        let mut vals = shape();
                        vals.push((
                            ("general_datum_reference", "base"),
                            st.datums[&r.datum.0].a(),
                        ));
                        if !r.modifiers.is_empty() {
                            let m = self.datum_modifiers(em, &r.modifiers)?;
                            vals.push((("general_datum_reference", "modifiers"), m));
                        }
                        elements.push(em.instance(&["datum_reference_element"], &vals, &[])?);
                    }
                    (
                        typed("common_datum_list", refs(elements)),
                        c.modifiers.clone(),
                    )
                }
            };
            let mut vals = shape();
            vals.push((("general_datum_reference", "base"), base));
            if !modifiers.is_empty() {
                let m = self.datum_modifiers(em, &modifiers)?;
                vals.push((("general_datum_reference", "modifiers"), m));
            }
            compartments.push(em.instance(&["datum_reference_compartment"], &vals, &[])?);
        }
        let r = em.simple(
            "datum_system",
            &[
                ("name", s(&name)),
                ("description", s("")),
                ("of_shape", pds.a()),
                ("product_definitional", boolean(false)),
                ("constituents", refs(compartments)),
            ],
        )?;
        st.systems.insert(key, r);
        Ok(r)
    }

    fn datum_modifiers(&self, em: &mut Emitter<'_>, ms: &[DatumModifier]) -> Result<A, WriteError> {
        let mut out = Vec::new();
        for m in ms {
            out.push(match m {
                DatumModifier::Simple(x) => {
                    let name = SimpleDatumModifier::ALL
                        .iter()
                        .find(|(_, v)| v == x)
                        .map_or("", |(n, _)| n);
                    typed("simple_datum_reference_modifier", en(name))
                }
                DatumModifier::WithValue { kind, value } => {
                    let k = match kind {
                        DatumModifierType::CircularOrCylindrical => "circular_or_cylindrical",
                        DatumModifierType::Spherical => "spherical",
                        DatumModifierType::Distance => "distance",
                        DatumModifierType::Projected => "projected",
                    };
                    let v = em.value(self.context, value)?;
                    em.simple(
                        "datum_reference_modifier_with_value",
                        &[("modifier_type", en(k)), ("modifier_value", v.a())],
                    )?
                    .a()
                }
            });
        }
        Ok(A::List(out))
    }

    /// The parameters of a thread or knurl: a `shape_representation_with_parameters` named
    /// items, as their WHERE rules name them.
    fn parameters(
        &self,
        em: &mut Emitter<'_>,
        definition: R,
        items: Vec<R>,
    ) -> Result<(), WriteError> {
        let pdef = em.simple(
            "property_definition",
            &[
                ("name", s("")),
                ("description", s("")),
                ("definition", definition.a()),
            ],
        )?;
        let rep = em.simple(
            "shape_representation_with_parameters",
            &[
                ("name", s("")),
                ("items", refs(items)),
                ("context_of_items", em.reference(self.context)?.a()),
            ],
        )?;
        em.simple(
            "property_definition_representation",
            &[("definition", pdef.a()), ("used_representation", rep.a())],
        )?;
        Ok(())
    }

    fn text_item(em: &mut Emitter<'_>, name: &str, text: &str) -> Result<R, WriteError> {
        em.simple(
            "descriptive_representation_item",
            &[("name", s(name)), ("description", s(text))],
        )
    }

    /// The feature definition's own shape and its 'applied shape' occurrence (thread WR13).
    fn applied(
        &self,
        em: &mut Emitter<'_>,
        st: &mut Emitted,
        pds: R,
        definition: R,
        feature: FeatureId,
    ) -> Result<R, WriteError> {
        let shape = em.simple(
            "product_definition_shape",
            &[
                ("name", s("")),
                ("description", s("")),
                ("definition", definition.a()),
            ],
        )?;
        let f = self.feature(em, st, pds, feature.0)?;
        let occ = em.simple(
            "shape_aspect",
            &[
                ("name", s("")),
                ("description", s("applied shape")),
                ("of_shape", shape.a()),
                ("product_definitional", boolean(false)),
            ],
        )?;
        em.simple(
            "shape_defining_relationship",
            &[
                ("name", s("")),
                ("description", s("applied shape")),
                ("relating_shape_aspect", f.a()),
                ("related_shape_aspect", occ.a()),
            ],
        )?;
        Ok(shape)
    }

    /// A shape aspect of the definition's shape described `description`, and, when `from` is
    /// given, the `shape_defining_relationship` described `usage` from it.
    fn definition_aspect(
        &self,
        em: &mut Emitter<'_>,
        st: &mut Emitted,
        pds: R,
        shape: R,
        (description, usage): (&str, &str),
        from: Option<R>,
    ) -> Result<(), WriteError> {
        let occ = em.simple(
            "shape_aspect",
            &[
                ("name", s("")),
                ("description", s(description)),
                ("of_shape", shape.a()),
                ("product_definitional", boolean(false)),
            ],
        )?;
        let _ = (st, pds);
        if let Some(f) = from {
            em.simple(
                "shape_defining_relationship",
                &[
                    ("name", s("")),
                    ("description", s(usage)),
                    ("relating_shape_aspect", f.a()),
                    ("related_shape_aspect", occ.a()),
                ],
            )?;
        }
        Ok(())
    }

    fn thread(
        &self,
        em: &mut Emitter<'_>,
        st: &mut Emitted,
        pds: R,
        t: &Thread,
        index: usize,
    ) -> Result<(), WriteError> {
        let def = em.simple("thread", &[("name", s("")), ("description", s(""))])?;
        let shape = self.applied(em, st, pds, def, t.feature)?;
        #[cfg(feature = "presentation")]
        st.threads.insert(index, shape);
        #[cfg(not(any(feature = "presentation", feature = "json-import")))]
        let _ = index;
        #[cfg(feature = "json-import")]
        self.identify(em, "threads", index, shape)?;
        let area = match t.partial_area {
            Some(f) => Some(self.role_instance(em, st, pds, f, Role::AppliedArea)?),
            None => None,
        };
        self.definition_aspect(
            em,
            st,
            pds,
            shape,
            ("partial area occurrence", "applied area usage"),
            area,
        )?;
        let runout = match t.runout {
            Some(f) => Some(self.role_instance(em, st, pds, f, Role::ThreadRunout)?),
            None => None,
        };
        self.definition_aspect(
            em,
            st,
            pds,
            shape,
            ("thread runout", "thread runout usage"),
            runout,
        )?;
        let ctx = self.context;
        let hand = |h: Hand| match h {
            Hand::Left => "left",
            Hand::Right => "right",
        };
        let mut items = vec![
            Self::text_item(
                em,
                "thread side",
                match t.side {
                    ThreadSide::Internal => "internal",
                    ThreadSide::External => "external",
                },
            )?,
            em.measure_item(
                "major diameter",
                MeasureItem::Length(&t.major_diameter),
                ctx,
                Vec::new(),
                None,
            )?,
        ];
        if let Some(l) = &t.minor_diameter {
            items.push(em.measure_item(
                "minor diameter",
                MeasureItem::Length(l),
                ctx,
                Vec::new(),
                None,
            )?);
        }
        if let Some(l) = &t.pitch_diameter {
            items.push(em.measure_item(
                "pitch diameter",
                MeasureItem::Length(l),
                ctx,
                Vec::new(),
                None,
            )?);
        }
        items.push(em.measure_item(
            "number of threads",
            MeasureItem::Ratio(&t.number_of_threads.0),
            ctx,
            Vec::new(),
            None,
        )?);
        items.push(Self::text_item(em, "form", &t.form)?);
        items.push(Self::text_item(em, "fit class", &t.fit_class)?);
        if let Some(c) = &t.fit_class_2 {
            items.push(Self::text_item(em, "fit class 2", c)?);
        }
        items.push(Self::text_item(em, "hand", hand(t.hand))?);
        if let Some(l) = &t.crest {
            items.push(em.measure_item("crest", MeasureItem::Length(l), ctx, Vec::new(), None)?);
        }
        if let Some(q) = &t.qualifier {
            items.push(Self::text_item(em, "qualifier", q)?);
        }
        if let Some(l) = &t.nominal_size {
            items.push(em.measure_item(
                "nominal size",
                MeasureItem::Length(l),
                ctx,
                Vec::new(),
                None,
            )?);
        }
        self.parameters(em, def, items)
    }

    fn knurl(
        &self,
        em: &mut Emitter<'_>,
        st: &mut Emitted,
        pds: R,
        k: &Knurl,
        #[cfg(feature = "json-import")] index: usize,
    ) -> Result<(), WriteError> {
        let pattern = match k.pattern {
            KnurlPattern::Diamond => "diamond",
            KnurlPattern::Diagonal => "diagonal",
            KnurlPattern::Straight => "straight",
        };
        let def = em.simple(
            "turned_knurl",
            &[("name", s("")), ("description", s(pattern))],
        )?;
        let shape = self.applied(em, st, pds, def, k.feature)?;
        #[cfg(feature = "json-import")]
        self.identify(em, "knurls", index, shape)?;
        let area = self.role_instance(em, st, pds, k.feature, Role::AppliedArea)?;
        // turned_knurl WR11 (as thread WR12): exactly one 'partial area occurrence' with one
        // 'applied area usage' from an applied_area: the knurled faces themselves (the model
        // holds no other area for a knurl).
        self.definition_aspect(
            em,
            st,
            pds,
            shape,
            ("partial area occurrence", "applied area usage"),
            Some(area),
        )?;
        let ctx = self.context;
        let mut items = vec![
            em.measure_item(
                "major diameter",
                MeasureItem::Length(&k.major_diameter),
                ctx,
                Vec::new(),
                None,
            )?,
            em.measure_item(
                "nominal diameter",
                MeasureItem::Length(&k.nominal_diameter),
                ctx,
                Vec::new(),
                None,
            )?,
            em.measure_item(
                "diametral pitch",
                MeasureItem::Length(&k.diametral_pitch),
                ctx,
                Vec::new(),
                None,
            )?,
        ];
        let teeth;
        if let Some(c) = k.number_of_teeth {
            teeth = Decimal::parse(&format!("{}.", c.0)).map_err(|e| WriteError::Internal(e.0))?;
            items.push(em.measure_item(
                "number of teeth",
                MeasureItem::Count(&teeth),
                ctx,
                Vec::new(),
                None,
            )?);
        }
        if let Some(l) = &k.tooth_depth {
            items.push(em.measure_item(
                "tooth depth",
                MeasureItem::Length(l),
                ctx,
                Vec::new(),
                None,
            )?);
        }
        if let Some(l) = &k.root_fillet {
            items.push(em.measure_item(
                "root fillet",
                MeasureItem::Length(l),
                ctx,
                Vec::new(),
                None,
            )?);
        }
        if let Some(a) = &k.helix_angle {
            items.push(em.measure_item(
                "helix angle",
                MeasureItem::Angle(a),
                ctx,
                Vec::new(),
                None,
            )?);
        }
        if let Some(h) = k.helix_hand {
            items.push(Self::text_item(
                em,
                "helix hand",
                match h {
                    Hand::Left => "left",
                    Hand::Right => "right",
                },
            )?);
        }
        self.parameters(em, def, items)
    }

    /// A user defined attribute (UDA practice §5–§7): `general_property` associated with the
    /// property definition, its values the items of its representation.
    fn attribute_set(
        &self,
        em: &mut Emitter<'_>,
        st: &mut Emitted,
        pd: R,
        pds: R,
        a: &AttributeSet,
    ) -> Result<R, WriteError> {
        let owner = match a.on {
            // Editable note text on the part is on its shape (PMI practice §7.4.3, Table 17:
            // 'on part' is property_definition.definition = product_definition_shape).
            None if a.name == "semantic text" => pds,
            None => pd,
            Some(NoteOwner::Feature(f)) => self.feature(em, st, pds, f.0)?,
            Some(NoteOwner::Dimension(d)) => self.dimension(em, st, pds, d.0)?,
            Some(NoteOwner::Tolerance(t)) => st.tolerances[&t.0],
            Some(NoteOwner::DatumTarget(t)) => st.targets[&t.0],
        };
        let mut items = Vec::new();
        for (key, v) in &a.items {
            items.push(match v {
                AttributeValue::Text(t) => Self::text_item(em, key, t)?,
                AttributeValue::Integer(i) => em.simple(
                    "integer_representation_item",
                    &[("name", s(key)), ("the_value", A::Integer(*i))],
                )?,
                AttributeValue::Real(d) => {
                    let v = real(d).map_err(WriteError::Internal)?;
                    em.instance(
                        &["real_representation_item"],
                        &[(("", "name"), s(key)), (("", "the_value"), v)],
                        &[d],
                    )?
                }
                AttributeValue::Boolean(b) => em.simple(
                    "boolean_representation_item",
                    &[("name", s(key)), ("the_value", boolean(*b))],
                )?,
                AttributeValue::Measure(m) => {
                    em.measure_item(key, measure_of_value(m), self.context, Vec::new(), None)?
                }
                AttributeValue::OtherMeasure {
                    measure,
                    value,
                    unit,
                } => {
                    let v = typed(measure, real(value).map_err(WriteError::Internal)?);
                    if unit.is_empty() {
                        em.instance(
                            &["value_representation_item"],
                            &[(("", "name"), s(key)), (("", "value_component"), v)],
                            &[value],
                        )?
                    } else {
                        let u = self.other_unit(unit).map_err(WriteError::Internal)?;
                        let u = em.reference(u)?;
                        em.instance(
                            &["measure_representation_item"],
                            &[
                                (("", "name"), s(key)),
                                (("", "value_component"), v),
                                (("", "unit_component"), u.a()),
                            ],
                            &[value],
                        )?
                    }
                }
            });
        }
        #[cfg(feature = "json-import")]
        if self.identifiers.is_some() {
            let pdef = self.property_optional(
                em,
                owner,
                &a.name,
                a.description.as_deref(),
                "haecceity.pmi/1 attribute",
                &items,
            )?;
            if let Some(g) = &a.general_property {
                let gp = em.simple(
                    "general_property",
                    &[
                        ("id", s(&g.id)),
                        ("name", s(&g.name)),
                        (
                            "description",
                            g.description.as_ref().map_or(A::Unset, |d| s(d)),
                        ),
                    ],
                )?;
                em.simple(
                    "general_property_association",
                    &[
                        ("name", s("")),
                        ("description", A::Unset),
                        ("base_definition", gp.a()),
                        ("derived_definition", pdef.a()),
                    ],
                )?;
            }
            return Ok(pdef);
        }
        let gp = em.simple(
            "general_property",
            &[
                ("id", s("")),
                ("name", s(&a.name)),
                ("description", A::Unset),
            ],
        )?;
        let pdef = self.property(em, owner, &a.name, "user defined attribute", "", &items)?;
        em.simple(
            "general_property_association",
            &[
                ("name", s("")),
                ("description", A::Unset),
                ("base_definition", gp.a()),
                ("derived_definition", pdef.a()),
            ],
        )?;
        Ok(pdef)
    }

    /// A surface texture as ISO 10303-1110 maps it (the PMI practice has no section on it):
    /// the `Surface_texture` is `property_definition('surface texture', '', <faces' shape
    /// aspect or the part's shape>)` with a `representation('surface texture')` (the global
    /// rule restrict_representation_for_surface_condition: the names agree) holding the
    /// 'material removal condition'; each `Standard_surface_texture_parameter` is a
    /// `property_definition` on the same owner, related to it by
    /// `property_definition_relationship('surface texture parameter')`, with a
    /// `surface_texture_representation` of the characteristic ('measuring method', WR2) and its
    /// value (a length measure item, WR1, WR3), and associated with a `general_property`
    /// 'surface_condition' (WR5). The parameter is named 'surface_condition', not the mapping's
    /// 'surface texture parameter': general_property_association WR2 requires the derived and
    /// base names to agree, so with WR5 no other name is schema-valid (docs/step-ap242.md,
    /// question 3).
    fn surface_texture(
        &self,
        em: &mut Emitter<'_>,
        st: &mut Emitted,
        pds: R,
        t: &SurfaceTexture,
    ) -> Result<R, WriteError> {
        let owner = match t.on {
            None => pds,
            Some(f) => self.feature(em, st, pds, f.0)?,
        };
        let removal = Self::text_item(em, "material removal condition", t.material_removal.term())?;
        let texture = self.property(
            em,
            owner,
            "surface texture",
            "",
            "surface texture",
            &[removal],
        )?;
        for p in &t.parameters {
            let method = Self::text_item(em, "measuring method", &p.characteristic)?;
            let value = em.measure_item(
                "characteristic value",
                MeasureItem::Length(&p.value),
                self.context,
                Vec::new(),
                None,
            )?;
            let parameter = em.simple(
                "property_definition",
                &[
                    ("name", s("surface_condition")),
                    ("description", s("")),
                    ("definition", owner.a()),
                ],
            )?;
            let rep = em.simple(
                "surface_texture_representation",
                &[
                    ("name", s("surface texture parameter")),
                    ("items", refs([method, value])),
                    ("context_of_items", em.reference(self.context)?.a()),
                ],
            )?;
            em.simple(
                "property_definition_representation",
                &[
                    ("definition", parameter.a()),
                    ("used_representation", rep.a()),
                ],
            )?;
            em.simple(
                "property_definition_relationship",
                &[
                    ("name", s("surface texture parameter")),
                    ("description", s("")),
                    ("relating_property_definition", texture.a()),
                    ("related_property_definition", parameter.a()),
                ],
            )?;
            let condition = em.simple(
                "general_property",
                &[
                    ("id", s("")),
                    ("name", s("surface_condition")),
                    ("description", A::Unset),
                ],
            )?;
            em.simple(
                "general_property_association",
                &[
                    ("name", s("")),
                    ("description", A::Unset),
                    ("base_definition", condition.a()),
                    ("derived_definition", parameter.a()),
                ],
            )?;
        }
        Ok(texture)
    }
}

/// Tessellated/polyline ink of kept general notes, including repositioned tessellated sets.
/// Read only: reserve foreign presentation without moving or restating it.
#[cfg(feature = "presentation")]
fn kept_notes_points(doc: &Document, removed: &BTreeSet<u64>) -> Vec<Vec<[f64; 3]>> {
    use crate::geom;
    let mut out = Vec::new();
    let roots = doc.ids().filter(|id| {
        !removed.contains(id)
            && is_a(doc, *id, "draughting_callout")
            && attr_str(doc, *id, "representation_item", "name").as_deref() == Some("General notes")
    });
    for root in roots {
        let identity = ([0.; 3], [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]);
        let mut stack = vec![(root, identity)];
        let mut seen = BTreeSet::new();
        while let Some((id, mut frame)) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            let transform = |frame: ([f64; 3], [[f64; 3]; 3]), p: [f64; 3]| {
                (0..3).fold(frame.0, |q, k| geom::add(q, geom::scale(frame.1[k], p[k])))
            };
            let location = attr_ref(doc, id, "repositioned_tessellated_item", "location");
            if let Some(location) = location {
                let origin = attr_ref(doc, location, "placement", "location")
                    .and_then(|p| point_of(doc, p))
                    .unwrap_or([0.; 3]);
                let direction = |field, default| {
                    attr_ref(doc, location, "axis2_placement_3d", field)
                        .and_then(|d| {
                            let A::List(v) = attr(doc, d, "direction", "direction_ratios")? else {
                                return None;
                            };
                            let mut xyz = [0.; 3];
                            for (i, a) in v.iter().take(3).enumerate() {
                                xyz[i] = match a {
                                    A::Real(x) => *x,
                                    A::Integer(x) => *x as f64,
                                    _ => return None,
                                };
                            }
                            geom::unit(xyz)
                        })
                        .unwrap_or(default)
                };
                let z = direction("axis", [0., 0., 1.]);
                let x = direction("ref_direction", [1., 0., 0.]);
                let y = geom::unit(geom::cross(z, x)).unwrap_or([0., 1., 0.]);
                let x = geom::cross(y, z);
                let basis = [x, y, z].map(|v| geom::sub(transform(frame, v), frame.0));
                frame = (transform(frame, origin), basis);
            }
            if let Some(p) = point_of(doc, id) {
                out.push(vec![transform(frame, p)]);
            }
            // Full conics conservatively enclose any trimmed annotation arcs.
            if let Some(position) = attr_ref(doc, id, "conic", "position") {
                let radius = ["radius", "semi_axis_1", "semi_axis_2"]
                    .iter()
                    .filter_map(|field| {
                        match attr(
                            doc,
                            id,
                            if *field == "radius" {
                                "circle"
                            } else {
                                "ellipse"
                            },
                            field,
                        )? {
                            A::Real(r) => Some(*r),
                            A::Integer(r) => Some(*r as f64),
                            _ => None,
                        }
                    })
                    .fold(0., f64::max);
                if let Some(centre) =
                    attr_ref(doc, position, "placement", "location").and_then(|p| point_of(doc, p))
                {
                    let centre = transform(frame, centre);
                    out.push(vec![
                        geom::sub(centre, [radius; 3]),
                        geom::add(centre, [radius; 3]),
                    ]);
                }
            }
            if let Some(A::List(points)) = attr(doc, id, "coordinates_list", "position_coords") {
                let points = points
                    .iter()
                    .filter_map(|p| {
                        let A::List(v) = p else { return None };
                        let mut xyz = [0.; 3];
                        for (i, a) in v.iter().take(3).enumerate() {
                            xyz[i] = match a {
                                A::Real(x) => *x,
                                A::Integer(x) => *x as f64,
                                _ => return None,
                            };
                        }
                        Some(transform(frame, xyz))
                    })
                    .collect();
                out.push(points);
            }
            if let Some(e) = doc.get(id) {
                p21::visit_attributes(e, &mut |a| {
                    if let A::EntityRef(next) = a
                        && Some(*next) != location
                    {
                        stack.push((*next, frame));
                    }
                });
            }
        }
    }
    out
}

fn needs_callouts(p: &PartPmi) -> bool {
    #[cfg(feature = "presentation")]
    {
        !p.dimensions.is_empty()
            || !p.threads.is_empty()
            || !p.tolerances.is_empty()
            || p.surface_textures.iter().any(|t| t.on.is_some())
            || !crate::presentation::notes::general_notes(p).is_empty()
    }
    #[cfg(not(feature = "presentation"))]
    {
        let _ = p;
        false
    }
}

// Preserve the ordinary writer's return type when JSON identity emission is disabled.
#[cfg(feature = "json-import")]
type TableIdentity = R;
#[cfg(not(feature = "json-import"))]
type TableIdentity = ();

/// What a part's emission has made so far.
#[derive(Default)]
struct Emitted {
    occt_protected: BTreeSet<R>,
    #[cfg(feature = "presentation")]
    properties: BTreeMap<crate::presentation::notes::Source, R>,
    /// Canonical feature → instance.
    features: BTreeMap<usize, R>,
    /// Geometry → its item and, for a new one, the representation holding it.
    geometry: BTreeMap<usize, (R, Option<R>)>,
    datums: BTreeMap<usize, R>,
    targets: BTreeMap<usize, R>,
    dimensions: BTreeMap<usize, R>,
    dimension_values: BTreeSet<usize>,
    tolerances: BTreeMap<usize, R>,
    #[cfg(feature = "presentation")]
    threads: BTreeMap<usize, R>,
    tolerance_targets: BTreeMap<usize, R>,
    systems: BTreeMap<String, R>,
    /// Usage keys (representation, items) several features need: each one shape aspect.
    shared_keys: BTreeSet<(R, Vec<R>)>,
    shared: BTreeMap<(R, Vec<R>), R>,
    datum_carriers: BTreeSet<R>,
    /// Per shared key, the feature that owns it (a referenced feature of exactly those items).
    owners: BTreeMap<(R, Vec<R>), usize>,
    twins: BTreeMap<(usize, Role), R>,
}

fn target_shape_name(shape: &TargetShape) -> &'static str {
    match shape {
        TargetShape::Point => "point",
        TargetShape::Line { .. } => "line",
        TargetShape::Rectangle { .. } => "rectangle",
        TargetShape::Circle { .. } => "circle",
        TargetShape::CircularCurve { .. } => "circular curve",
        TargetShape::Area(_) => "area",
        TargetShape::Curve(_) => "curve",
    }
}
fn target_sizes(shape: &TargetShape) -> Vec<(&'static str, &Length)> {
    match shape {
        TargetShape::Line { length } => vec![("target length", length)],
        TargetShape::Rectangle { length, width } => {
            vec![("target length", length), ("target width", width)]
        }
        TargetShape::Circle { diameter } | TargetShape::CircularCurve { diameter } => {
            vec![("target diameter", diameter)]
        }
        _ => Vec::new(),
    }
}

fn zone_form_name(f: &ZoneForm) -> &str {
    match f {
        ZoneForm::Other(o) => o,
        f => ZoneForm::TABLE
            .iter()
            .find(|(_, x)| x == f)
            .map_or("", |(n, _)| n),
    }
}

/// Every REAL of a geometry item is a Part 21 REAL.
fn geometry_values(g: &GeometryItem) -> Result<(), String> {
    fn value(v: &GeometryValue) -> Result<(), String> {
        match v {
            GeometryValue::Real(d) => real(d).map(|_| ()),
            GeometryValue::Item(i) => geometry_values(i),
            GeometryValue::List(l) => l.iter().try_for_each(value),
            GeometryValue::Typed { value: v, .. } => value(v),
            _ => Ok(()),
        }
    }
    g.leaves
        .iter()
        .try_for_each(|(_, vals)| vals.iter().try_for_each(value))
}

/// A supplemental geometry item, by value: its sub-items first (equal ones once).
fn geometry_item(
    em: &mut Emitter<'_>,
    g: &GeometryItem,
    memo: &mut HashMap<GeometryItem, R>,
) -> Result<R, WriteError> {
    if let Some(&r) = memo.get(g) {
        return Ok(r);
    }
    fn value(
        em: &mut Emitter<'_>,
        v: &GeometryValue,
        memo: &mut HashMap<GeometryItem, R>,
        stated: &mut Vec<Decimal>,
    ) -> Result<A, WriteError> {
        Ok(match v {
            GeometryValue::Real(d) => {
                stated.push(d.clone());
                real(d).map_err(WriteError::Internal)?
            }
            GeometryValue::Integer(i) => A::Integer(*i),
            GeometryValue::Text(t) => s(t),
            GeometryValue::Enum(e) => A::Enum(e.clone()),
            GeometryValue::Binary(b) => A::Binary(b.clone()),
            GeometryValue::Item(i) => geometry_item(em, i, memo)?.a(),
            GeometryValue::List(l) => {
                let mut out = Vec::new();
                for x in l {
                    out.push(value(em, x, memo, stated)?);
                }
                A::List(out)
            }
            GeometryValue::Typed {
                type_name,
                value: v,
            } => A::Typed {
                type_name: type_name.clone(),
                value: Box::new(value(em, v, memo, stated)?),
            },
            GeometryValue::Unset => A::Unset,
            GeometryValue::Derived => A::Derived,
        })
    }
    let mut stated = Vec::new();
    let mut leaves = Vec::new();
    for (name, vals) in &g.leaves {
        let mut attrs = Vec::new();
        for v in vals {
            attrs.push(value(em, v, memo, &mut stated)?);
        }
        leaves.push((name.clone(), attrs));
    }
    let record = match leaves.as_slice() {
        [(name, attrs)] => p21::simple(name, attrs.clone()),
        _ => Complex::new(leaves.into_iter().map(|(n, a)| p21::leaf(&n, a)))?.into_record(),
    };
    let refs: Vec<&Decimal> = stated.iter().collect();
    let r = em.push(record, &refs)?;
    memo.insert(g.clone(), r);
    Ok(r)
}

// ---------------------------------------------------------------------------------------------
// Datum feature symbols (decision 6): presentation derived from the semantics
// ---------------------------------------------------------------------------------------------

#[cfg(not(feature = "presentation"))]
use crate::stroke_font::lettering;

/// The coordinates of a `cartesian_point` instance.
fn point_of(doc: &Document, id: u64) -> Option<[f64; 3]> {
    if !leaf_names(doc, id).iter().any(|n| n == "cartesian_point") {
        return None;
    }
    let Some(Attribute::List(c)) = attr(doc, id, "cartesian_point", "coordinates") else {
        return None;
    };
    let mut p = [0.0; 3];
    for (k, v) in c.iter().take(3).enumerate() {
        p[k] = match v {
            Attribute::Real(x) => *x,
            Attribute::Integer(i) => *i as f64,
            _ => return None,
        };
    }
    Some(p)
}

/// The first point under `id`, depth first in attribute order: for a face, a vertex of its
/// first bound (bounds come before the surface), for an edge its start vertex, for a geometric
/// item its location.
fn first_point(doc: &Document, id: u64) -> Option<[f64; 3]> {
    let mut stack = vec![id];
    let mut seen = BTreeSet::new();
    while let Some(x) = stack.pop() {
        if !seen.insert(x) {
            continue;
        }
        if let Some(p) = point_of(doc, x) {
            return Some(p);
        }
        let mut next = Vec::new();
        if let Some(e) = doc.get(x) {
            p21::visit_attributes(e, &mut |a| {
                if let Attribute::EntityRef(n) = a {
                    next.push(*n);
                }
            });
        }
        stack.extend(next.into_iter().rev());
    }
    None
}

/// The vertices of `faces`: the points of their `vertex_point`s, reached through their bounds
/// (curves and surfaces are not entered).
fn vertices(doc: &Document, faces: &[u64]) -> Vec<[f64; 3]> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut stack: Vec<u64> = faces
        .iter()
        .flat_map(|&f| attr_refs(doc, f, "face", "bounds"))
        .collect();
    while let Some(x) = stack.pop() {
        if !seen.insert(x) {
            continue;
        }
        if leaf_names(doc, x).iter().any(|n| n == "vertex_point") {
            if let Some(p) =
                attr_ref(doc, x, "vertex_point", "vertex_geometry").and_then(|p| point_of(doc, p))
            {
                out.push(p);
            }
            continue;
        }
        if is_a(doc, x, "curve") || is_a(doc, x, "surface") || is_a(doc, x, "point") {
            continue;
        }
        if let Some(e) = doc.get(x) {
            p21::visit_attributes(e, &mut |a| {
                if let Attribute::EntityRef(n) = a {
                    stack.push(*n);
                }
            });
        }
    }
    out
}

/// The first `cartesian_point` of a supplemental geometry item, by value.
pub(crate) fn geometry_point(g: &GeometryItem) -> Option<[f64; 3]> {
    fn value(v: &GeometryValue) -> Option<[f64; 3]> {
        match v {
            GeometryValue::Item(i) => geometry_point(i),
            GeometryValue::List(l) => l.iter().find_map(value),
            GeometryValue::Typed { value: v, .. } => value(v),
            _ => None,
        }
    }
    for (name, vals) in &g.leaves {
        if name.eq_ignore_ascii_case("cartesian_point")
            && let Some(GeometryValue::List(c)) = vals.get(1)
        {
            let mut p = [0.0; 3];
            for (k, v) in c.iter().take(3).enumerate() {
                p[k] = match v {
                    GeometryValue::Real(d) => d.to_f64(),
                    GeometryValue::Integer(i) => *i as f64,
                    _ => return None,
                };
            }
            return Some(p);
        }
    }
    g.leaves
        .iter()
        .find_map(|(_, vals)| vals.iter().find_map(value))
}

/// Where datum feature symbols go (as specify-core lays them out): standing above the part in
/// the plane of its two longest extents, each in the plane through its feature's point, side
/// by side, sized to the part.
#[cfg(not(feature = "presentation"))]
struct SymbolLayout {
    hi: [f64; 3],
    /// Axes of the longest, second longest and shortest extent.
    u: usize,
    v: usize,
    height: f64,
    /// The right edge of the last symbol.
    right: f64,
}

#[cfg(not(feature = "presentation"))]
impl SymbolLayout {
    fn new(points: &[[f64; 3]], mm_per_unit: f64) -> SymbolLayout {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in points {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let extent: Vec<f64> = (0..3).map(|k| (hi[k] - lo[k]).max(0.0)).collect();
        let mut axes = [0usize, 1, 2];
        axes.sort_by(|a, b| extent[*b].total_cmp(&extent[*a]).then(a.cmp(b)));
        let diagonal = extent.iter().map(|e| e * e).sum::<f64>().sqrt();
        let diagonal = if diagonal > 0.0 { diagonal } else { 30.0 };
        SymbolLayout {
            hi,
            u: axes[0],
            v: axes[1],
            height: super::sites::text_height(
                extent.iter().copied().fold(0., f64::max) * mm_per_unit,
            ) / mm_per_unit,
            right: lo[axes[0]] - diagonal,
        }
    }

    /// The same measured datum placement rule in the feature-off lettering path.
    #[allow(clippy::type_complexity)]
    fn draw_on_face(
        &mut self,
        tip: [f64; 3],
        outward: [f64; 3],
        label: &str,
    ) -> ([[f64; 3]; 3], Vec<Vec<[f64; 3]>>) {
        use crate::geom;
        let axis = if outward[0].abs() < 0.8 {
            [1., 0., 0.]
        } else {
            [0., 1., 0.]
        };
        let along = geom::unit(geom::sub(
            axis,
            geom::scale(outward, geom::dot(axis, outward)),
        ))
        .unwrap_or([1., 0., 0.]);
        let normal = geom::unit(geom::cross(along, outward)).unwrap_or([0., 0., 1.]);
        let h = self.height;
        let (strokes, width) = lettering(label, h);
        let size = h;
        let frame = width.max(size) + h;
        let bottom = 0.6 * h;
        let at = |x: f64, y: f64| {
            geom::add(
                tip,
                geom::add(geom::scale(along, x), geom::scale(outward, y)),
            )
        };
        let mut lines = vec![vec![
            at(-frame / 2., bottom),
            at(frame / 2., bottom),
            at(frame / 2., bottom + size + h),
            at(-frame / 2., bottom + size + h),
            at(-frame / 2., bottom),
        ]];
        for stroke in strokes {
            lines.push(
                stroke
                    .into_iter()
                    .map(|(x, y)| at(x - width / 2., bottom + h / 2. + y))
                    .collect(),
            );
        }
        lines.push(vec![
            at(-0.2 * h, 0.3 * h),
            tip,
            at(0.2 * h, 0.3 * h),
            at(-0.2 * h, 0.3 * h),
        ]);
        lines.push(vec![tip, at(0., bottom)]);
        let round = |p: [f64; 3]| p.map(|x| (x * 1e6).round() / 1e6 + 0.);
        let lines = lines
            .into_iter()
            .map(|l| l.into_iter().map(round).collect())
            .collect();
        ([round(at(-frame / 2., bottom)), normal, along], lines)
    }

    /// The symbol's plane (origin, normal, direction in it) and its polylines: the label boxed,
    /// a stem down to a triangle whose apex is `tip`.
    #[allow(clippy::type_complexity)]
    fn draw(&mut self, tip: [f64; 3], label: &str) -> ([[f64; 3]; 3], Vec<Vec<[f64; 3]>>) {
        let (u, v, h) = (self.u, self.v, self.height);
        let at = |a: f64, b: f64| {
            let mut p = tip;
            p[u] = a;
            p[v] = b;
            p
        };
        #[cfg(not(feature = "presentation"))]
        let (mut lines, frame, left, bottom) = {
            let (strokes, width) = lettering(label, h);
            let size = 1.6 * h;
            let frame = size.max(width + 0.6 * h);
            let left = (tip[u] - frame / 2.0).max(self.right + size / 2.0);
            self.right = left + frame;
            let bottom = self.hi[v].max(tip[v]) + size;
            let mut lines = vec![vec![
                at(left, bottom),
                at(left + frame, bottom),
                at(left + frame, bottom + size),
                at(left, bottom + size),
                at(left, bottom),
            ]];
            let (a, b) = (left + (frame - width) / 2.0, bottom + (size - h) / 2.0);
            for s in &strokes {
                lines.push(s.iter().map(|(x, y)| at(a + x, b + y)).collect());
            }
            (lines, frame, left, bottom)
        };
        let half = h / 2.0;
        let (tu, tv) = (tip[u], tip[v]);
        let triangle = vec![
            at(tu - half, tv + half),
            at(tu + half, tv + half),
            at(tu, tv),
            at(tu - half, tv + half),
        ];
        lines.push(vec![at(left + frame / 2.0, bottom), at(tu, tv + half)]);
        lines.push(triangle);
        let mut normal = [0.0; 3];
        // e_u × e_v, toward the viewer.
        let w = 3 - u - v;
        normal[w] = if (u + 1) % 3 == v { 1.0 } else { -1.0 };
        let mut along = [0.0; 3];
        along[u] = 1.0;
        let round = |p: [f64; 3]| p.map(|x| (x * 1e6).round() / 1e6 + 0.0);
        let lines = lines
            .into_iter()
            .map(|l| l.into_iter().map(round).collect())
            .collect();
        ([round(at(left, bottom)), normal, along], lines)
    }
}

// ---------------------------------------------------------------------------------------------
// Semantic comparison
// ---------------------------------------------------------------------------------------------

/// The differences between two parts' PMI as meaning, not as indices: every reference is
/// resolved to what it names (a feature to its faces, edges and geometry; a datum to its label
/// and feature), collections are compared as sets (features of equal content are one feature;
/// modifiers in any order), and a one-datum compartment's own modifiers are the compartment's.
/// Values are compared as quantities (millimetres or radians, to 1e-12 relative) with their
/// decimal places; [`differences_as_stated`] compares their text and unit too. Empty when the
/// two mean the same.
#[must_use]
pub fn differences(a: &PartPmi, b: &PartPmi) -> Vec<String> {
    compare(a, b, false)
}

/// [`differences`], with every value compared as stated: its decimal text and its unit (a
/// round trip writes each value's own digits in its own unit). Integer mantissas and
/// exponent case are compared in their required REAL encoding; unit factors compare
/// exactly after unit arithmetic. Legal REAL precision remains significant. JSON's
/// `identity_differences` separately compares original transport spellings.
#[must_use]
pub fn differences_as_stated(a: &PartPmi, b: &PartPmi) -> Vec<String> {
    compare(a, b, true)
}

fn compare(a: &PartPmi, b: &PartPmi, stated: bool) -> Vec<String> {
    let ka = Canonical::of(a, stated);
    let kb = Canonical::of(b, stated);
    let mut out = Vec::new();
    for (name, x, y) in [
        ("standard", &ka.standards, &kb.standards),
        ("decimal places", &ka.decimal_places, &kb.decimal_places),
        ("feature", &ka.features, &kb.features),
        ("datum target", &ka.datum_targets, &kb.datum_targets),
        ("datum", &ka.datums, &kb.datums),
        ("dimension", &ka.dimensions, &kb.dimensions),
        ("tolerance", &ka.tolerances, &kb.tolerances),
        ("tolerance relation", &ka.relations, &kb.relations),
        ("general tolerance", &ka.general, &kb.general),
        ("thread", &ka.threads, &kb.threads),
        ("knurl", &ka.knurls, &kb.knurls),
        ("material", &ka.material, &kb.material),
        ("note", &ka.notes, &kb.notes),
        (
            "surface texture",
            &ka.surface_textures,
            &kb.surface_textures,
        ),
        ("attribute", &ka.attributes, &kb.attributes),
    ] {
        let mut ys = y.clone();
        for k in x {
            match ys.iter().position(|o| o == k) {
                Some(i) => {
                    ys.remove(i);
                }
                None => out.push(format!("{name} only in the first: {k}")),
            }
        }
        for k in ys {
            out.push(format!("{name} only in the second: {k}"));
        }
    }
    out
}

/// A part's PMI with every index resolved, each collection sorted.
struct Canonical {
    standards: Vec<String>,
    decimal_places: Vec<String>,
    features: Vec<String>,
    datum_targets: Vec<String>,
    datums: Vec<String>,
    dimensions: Vec<String>,
    tolerances: Vec<String>,
    relations: Vec<String>,
    general: Vec<String>,
    threads: Vec<String>,
    knurls: Vec<String>,
    material: Vec<String>,
    notes: Vec<String>,
    surface_textures: Vec<String>,
    attributes: Vec<String>,
}

impl Canonical {
    fn of(p: &PartPmi, stated: bool) -> Canonical {
        let k = Keys { p, stated };
        let sorted = |mut v: Vec<String>| {
            v.sort();
            v
        };
        let mut features = sorted(p.features.iter().map(|f| k.feature_content(f)).collect());
        features.dedup();
        let ol = |l: &Option<Length>| l.as_ref().map(|l| k.length(l));
        Canonical {
            standards: sorted(p.standards.iter().map(|s| format!("{s:?}")).collect()),
            decimal_places: p.decimal_places.iter().map(|d| d.to_string()).collect(),
            features,
            datum_targets: sorted((0..p.datum_targets.len()).map(|i| k.target(i)).collect()),
            datums: sorted((0..p.datums.len()).map(|i| k.datum(i)).collect()),
            dimensions: sorted((0..p.dimensions.len()).map(|i| k.dimension(i)).collect()),
            tolerances: sorted((0..p.tolerances.len()).map(|i| k.tolerance(i)).collect()),
            relations: sorted(
                p.tolerance_relations
                    .iter()
                    .map(|r| {
                        format!(
                            "{:?} {} -> {}",
                            r.kind,
                            k.tolerance(r.relating.0),
                            k.tolerance(r.related.0)
                        )
                    })
                    .collect(),
            ),
            general: sorted(p.general.iter().map(|g| k.general(g)).collect()),
            threads: sorted(
                p.threads
                    .iter()
                    .map(|t| {
                        format!(
                            "{:?} major {} minor {:?} pitch diameter {:?} threads {} form {:?} fit {:?} {:?} {:?} crest {:?} qualifier {:?} nominal {:?} on {} area {:?} runout {:?}",
                            t.side,
                            k.length(&t.major_diameter),
                            ol(&t.minor_diameter),
                            ol(&t.pitch_diameter),
                            k.decimal(&t.number_of_threads.0),
                            t.form,
                            t.fit_class,
                            t.fit_class_2,
                            t.hand,
                            ol(&t.crest),
                            t.qualifier,
                            ol(&t.nominal_size),
                            k.feature(t.feature),
                            t.partial_area.map(|f| k.feature(f)),
                            t.runout.map(|f| k.feature(f))
                        )
                    })
                    .collect(),
            ),
            knurls: sorted(
                p.knurls
                    .iter()
                    .map(|x| {
                        format!(
                            "{:?} major {} nominal {} pitch {} teeth {:?} depth {:?} fillet {:?} helix {:?} {:?} on {}",
                            x.pattern,
                            k.length(&x.major_diameter),
                            k.length(&x.nominal_diameter),
                            k.length(&x.diametral_pitch),
                            x.number_of_teeth,
                            ol(&x.tooth_depth),
                            ol(&x.root_fillet),
                            x.helix_angle.as_ref().map(|a| k.angle(a)),
                            x.helix_hand,
                            k.feature(x.feature)
                        )
                    })
                    .collect(),
            ),
            material: p.material.iter().map(|m| {
                let mut m = m.clone();
                if let Some(density) = &mut m.density {
                    density.value = Keys::encoded(&density.value);
                    for (_, exponent) in &mut density.unit { *exponent = Keys::encoded(exponent); }
                }
                format!("{m:?}")
            }).collect(),
            notes: sorted(
                p.notes
                    .iter()
                    .map(|n| format!("{:?} {:?} on {}", n.kind, n.text, k.owner(n.on)))
                    .collect(),
            ),
            surface_textures: sorted(
                p.surface_textures
                    .iter()
                    .map(|s| {
                        let ps: Vec<String> = s
                            .parameters
                            .iter()
                            .map(|x| format!("{:?} {}", x.characteristic, k.length(&x.value)))
                            .collect();
                        format!(
                            "{:?} [{}] on {}",
                            s.material_removal,
                            ps.join(", "),
                            k.owner(s.on.map(NoteOwner::Feature))
                        )
                    })
                    .collect(),
            ),
            attributes: sorted(
                p.attributes
                    .iter()
                    .map(|a| {
                        let items: Vec<String> = a
                            .items
                            .iter()
                            .map(|(n, v)| {
                                let v = match v {
                                    AttributeValue::Measure(m) => k.value(m),
                                    AttributeValue::Real(d) => k.decimal(d),
                                    AttributeValue::OtherMeasure {
                                        measure,
                                        value,
                                        unit,
                                    } => format!("{measure} {} {unit:?}", k.decimal(value)),
                                    v => format!("{v:?}"),
                                };
                                format!("{n:?}={v}")
                            })
                            .collect();
                        format!("{:?} [{}] on {}", a.name, items.join(", "), k.owner(a.on))
                    })
                    .collect(),
            ),
        }
    }
}

struct Keys<'p> {
    p: &'p PartPmi,
    /// Values as stated (text and unit), else as quantities.
    stated: bool,
}

impl Keys<'_> {
    // Native REALs cannot retain integer mantissas or lower-case e. Comparison
    // accepts exactly the shared emitter's spelling, retaining legal precision.
    fn encoded(d: &Decimal) -> Decimal {
        real_text(d)
            .ok()
            .and_then(|text| Decimal::parse(&text).ok())
            .unwrap_or_else(|| d.clone())
    }
    fn direction(d: &Direction) -> Direction {
        Direction(d.0.clone().map(|d| Self::encoded(&d)))
    }
    fn length_unit(unit: &LengthUnit) -> LengthUnit {
        match unit {
            LengthUnit::Other { name, metres } => LengthUnit::Other {
                name: name.clone(),
                metres: metres.scaled(0),
            },
            _ => unit.clone(),
        }
    }
    fn length_copy(l: &Length) -> Length {
        Length {
            value: Self::encoded(&l.value),
            unit: Self::length_unit(&l.unit),
        }
    }
    fn geometry(&self, id: usize) -> Option<Geometry> {
        fn item(geometry: &mut GeometryItem) {
            fn value(v: &mut GeometryValue) {
                match v {
                    GeometryValue::Real(d) => *d = Keys::encoded(d),
                    GeometryValue::Item(i) => item(i),
                    GeometryValue::List(a) => a.iter_mut().for_each(value),
                    GeometryValue::Typed { value: v, .. } => value(v),
                    _ => {}
                }
            }
            for (_, values) in &mut geometry.leaves {
                values.iter_mut().for_each(value);
            }
        }
        self.p.geometry.get(id).cloned().map(|mut g| {
            g.length_unit = g.length_unit.as_ref().map(Self::length_unit);
            item(&mut g.item);
            g
        })
    }
    fn decimal(&self, d: &Decimal) -> String {
        if self.stated {
            Self::encoded(d).to_string()
        } else {
            format!("{:.11e}", d.to_f64())
        }
    }

    fn length(&self, l: &Length) -> String {
        if self.stated {
            format!("{}{:?}", self.decimal(&l.value), Self::length_unit(&l.unit))
        } else {
            format!("{:.11e} mm", l.mm())
        }
    }

    fn angle(&self, a: &Angle) -> String {
        if self.stated {
            let unit = match &a.unit {
                AngleUnit::Other { name, radians } => AngleUnit::Other {
                    name: name.clone(),
                    radians: radians.scaled(0),
                },
                unit => unit.clone(),
            };
            format!("{}{:?}", self.decimal(&a.value), unit)
        } else {
            format!("{:.11e} rad", a.rad())
        }
    }

    fn value(&self, v: &Value) -> String {
        let q = match &v.quantity {
            Quantity::Length(l) => self.length(l),
            Quantity::Angle(a) => self.angle(a),
        };
        match v.decimal_places {
            Some(p) => format!("{q}/{p}"),
            None => q,
        }
    }

    fn ov(&self, v: &Option<Value>) -> String {
        v.as_ref()
            .map_or_else(|| "-".to_string(), |v| self.value(v))
    }

    fn bounds(&self, b: &Bounds) -> String {
        format!("{} {}", self.value(b.upper()), self.value(b.lower()))
    }

    fn general(&self, g: &GeneralTolerance) -> String {
        match g {
            GeneralTolerance::Class { text, standard } => format!("class {text:?} {standard:?}"),
            GeneralTolerance::Table { name, cells } => {
                let cells: Vec<String> = cells
                    .iter()
                    .map(|c| {
                        c.items
                            .iter()
                            .map(|(n, v)| {
                                let v = match v {
                                    CellValue::Value(x) => self.value(x),
                                    CellValue::Count(d)
                                        if self.stated && count_integer(d).is_some() =>
                                    {
                                        d.to_string()
                                    }
                                    CellValue::Count(d) => self.decimal(d),
                                    CellValue::Text(t) => format!("{t:?}"),
                                };
                                format!("{n:?}={v}")
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .collect();
                format!("table {name:?} [{}]", cells.join("; "))
            }
        }
    }

    fn feature(&self, f: FeatureId) -> String {
        self.p.features.get(f.0).map_or_else(
            || format!("missing feature {}", f.0),
            |x| self.feature_content(x),
        )
    }

    fn feature_content(&self, f: &Feature) -> String {
        match f {
            Feature::Items(items) => {
                let mut v: Vec<String> = items
                    .iter()
                    .map(|a| match a {
                        Anchor::Face(x) => format!("face {}", x.0),
                        Anchor::Edge(x) => format!("edge {}", x.0),
                        Anchor::Geometry(g) => format!("geometry {:?}", self.geometry(g.0)),
                    })
                    .collect();
                v.sort();
                v.dedup();
                format!("[{}]", v.join(", "))
            }
            Feature::Group { members, kind } => {
                let mut v: Vec<String> = members.iter().map(|m| self.feature(*m)).collect();
                v.sort();
                format!("{kind:?}({})", v.join(", "))
            }
            Feature::Derived {
                kind,
                from,
                geometry,
            } => {
                let mut v: Vec<String> = from.iter().map(|m| self.feature(*m)).collect();
                v.sort();
                let computed = geometry.map(|g| {
                    let g = &self.p.derived_geometry[g.0];
                    let supplemental: Vec<_> = g
                        .supplemental
                        .iter()
                        .filter_map(|s| self.geometry(s.0))
                        .collect();
                    format!(
                        "{:?} {:?} {:?} {:?} {:?} {:?}",
                        g.kind,
                        g.deriving_faces,
                        g.point.clone().map(|l| Self::length_copy(&l)),
                        Self::direction(&g.direction),
                        g.radius.as_ref().map(Self::length_copy),
                        supplemental
                    )
                });
                format!("{kind:?}<-({}) geometry {computed:?}", v.join(", "))
            }
        }
    }

    fn target(&self, i: usize) -> String {
        let t = &self.p.datum_targets[i];
        let shape = match t.shape() {
            TargetShape::Area(f) => format!("Area({})", self.feature(*f)),
            TargetShape::Curve(f) => format!("Curve({})", self.feature(*f)),
            TargetShape::Point => "Point".into(),
            TargetShape::Line { length } => format!("Line {}", self.length(length)),
            TargetShape::Rectangle { length, width } => {
                format!("Rectangle {} {}", self.length(length), self.length(width))
            }
            TargetShape::Circle { diameter } => format!("Circle {}", self.length(diameter)),
            TargetShape::CircularCurve { diameter } => {
                format!("CircularCurve {}", self.length(diameter))
            }
        };
        let placement = t.placement().map(|pl| {
            format!(
                "at {} {} {} axis {:?} ref {:?}",
                self.length(&pl.origin[0]),
                self.length(&pl.origin[1]),
                self.length(&pl.origin[2]),
                pl.axis.as_ref().map(Self::direction),
                pl.ref_direction.as_ref().map(Self::direction)
            )
        });
        format!(
            "{} {shape} {placement:?} {:?} on {:?}",
            t.number(),
            t.movable().map(Self::direction),
            t.on().map(|f| self.feature(f))
        )
    }

    fn datum(&self, i: usize) -> String {
        let d = &self.p.datums[i];
        let mut ts: Vec<String> = d.targets().iter().map(|t| self.target(t.0)).collect();
        ts.sort();
        format!(
            "{} feature {:?} targets [{}]",
            d.label(),
            d.feature().map(|f| self.feature(f)),
            ts.join("; ")
        )
    }

    fn label(&self, d: DatumId) -> String {
        self.p.datums.get(d.0).map_or_else(
            || format!("missing datum {}", d.0),
            |x| x.label().to_string(),
        )
    }

    fn modifiers(&self, ms: &[DatumModifier]) -> String {
        let mut v: Vec<String> = ms
            .iter()
            .map(|m| match m {
                DatumModifier::Simple(s) => format!("{s:?}"),
                DatumModifier::WithValue { kind, value } => {
                    format!("{kind:?} {}", self.value(value))
                }
            })
            .collect();
        v.sort();
        v.join(",")
    }

    fn system(&self, ds: &DatumSystem) -> String {
        ds.compartments()
            .iter()
            .map(|c| match c.references.as_slice() {
                [one] => {
                    let mut m = c.modifiers.clone();
                    m.extend(one.modifiers.iter().cloned());
                    format!("{}[{}]", self.label(one.datum), self.modifiers(&m))
                }
                many => {
                    let mut v: Vec<String> = many
                        .iter()
                        .map(|r| {
                            format!("{}[{}]", self.label(r.datum), self.modifiers(&r.modifiers))
                        })
                        .collect();
                    v.sort();
                    format!("({})[{}]", v.join("-"), self.modifiers(&c.modifiers))
                }
            })
            .collect::<Vec<_>>()
            .join("|")
    }

    fn dimension(&self, i: usize) -> String {
        let d = &self.p.dimensions[i];
        let kind = match &d.kind {
            DimensionKind::Size {
                feature,
                kind,
                path,
                angle,
            } => format!(
                "size {kind:?} of {} path {:?} angle {angle:?}",
                self.feature(*feature),
                path.map(|p| self.feature(p))
            ),
            DimensionKind::Location {
                from,
                to,
                kind,
                path,
                directed,
                angle,
            } => format!(
                "location {kind:?} from {} to {} path {:?} directed {directed} angle {angle:?}",
                self.feature(*from),
                self.feature(*to),
                path.map(|p| self.feature(p))
            ),
        };
        let tolerance = match &d.tolerance {
            DimTolerance::None => "none".to_string(),
            DimTolerance::Basic => "basic".to_string(),
            DimTolerance::Deviations(b) => format!("deviations {}", self.bounds(b)),
            DimTolerance::Limits(b) => format!("limits {}", self.bounds(b)),
            DimTolerance::Fit { class, limits } => format!(
                "fit {class} {}",
                limits.as_ref().map_or_else(String::new, |b| self.bounds(b))
            ),
        };
        let mut m: Vec<String> = d.modifiers.iter().map(|x| format!("{x:?}")).collect();
        m.sort();
        format!(
            "{kind} nominal {} {tolerance} qualifier {:?} modifiers {m:?} principle {:?}",
            self.ov(&d.nominal),
            d.qualifier,
            d.principle
        )
    }

    fn tolerance(&self, i: usize) -> String {
        let Some(t) = self.p.tolerances.get(i) else {
            return format!("missing tolerance {i}");
        };
        let target = match &t.target {
            ToleranceTarget::Feature(f) => format!("feature {}", self.feature(*f)),
            ToleranceTarget::Dimension(d) => format!("dimension {}", self.dimension(d.0)),
            ToleranceTarget::Relation {
                relating,
                related,
                name,
            } => format!(
                "relation {name:?} {} -> {}",
                self.feature(*relating),
                self.feature(*related)
            ),
            ToleranceTarget::WholePart => "whole part".into(),
        };
        let zone = t.zone.as_ref().map(|z| {
            let mut nu: Option<Vec<String>> = z
                .non_uniform
                .as_ref()
                .map(|v| v.iter().map(|f| self.feature(*f)).collect());
            if let Some(v) = &mut nu {
                v.sort();
            }
            format!(
                "{:?} projected {:?} non-uniform {nu:?} runout {} affected {:?}",
                z.form,
                z.projected
                    .as_ref()
                    .map(|p| (p.end.map(|f| self.feature(f)), self.value(&p.length))),
                self.ov(&z.runout_angle),
                z.affected_plane
                    .map(|f| (self.feature(f), z.affected_plane_kind))
            )
        });
        let unit = t.unit_basis.as_ref().map(|u| {
            format!(
                "{} {:?}",
                self.value(&u.size),
                u.area.as_ref().map(|a| (a.shape, self.ov(&a.second)))
            )
        });
        let mut mods: Vec<String> = t.modifiers.iter().map(|m| format!("{m:?}")).collect();
        mods.sort();
        let mut aux: Vec<String> = t.auxiliary.iter().map(|m| format!("{m:?}")).collect();
        aux.sort();
        format!(
            "{:?} on {target} magnitude {} zone {zone:?} modifiers {mods:?} unit {unit:?} maximum {} unequal {} datums {:?} auxiliary {aux:?} description {:?}",
            t.kind,
            self.ov(&t.magnitude),
            self.ov(&t.maximum),
            self.ov(&t.unequal),
            t.datums.as_ref().map(|d| self.system(d)),
            t.description
        )
    }

    fn owner(&self, o: Option<NoteOwner>) -> String {
        match o {
            None => "part".into(),
            Some(NoteOwner::Feature(f)) => format!("feature {}", self.feature(f)),
            Some(NoteOwner::Dimension(d)) => format!("dimension {}", self.dimension(d.0)),
            Some(NoteOwner::Tolerance(t)) => format!("tolerance {}", self.tolerance(t.0)),
            Some(NoteOwner::DatumTarget(t)) => format!("datum target {}", self.target(t.0)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complex_leaves_are_sorted_whatever_the_order_given() {
        // The only way to make a complex record sorts its leaves: the external mapping's
        // order cannot be broken through this layer.
        let c = Complex::new([
            p21::leaf("REPRESENTATION_ITEM", vec![s("x")]),
            p21::leaf("LENGTH_MEASURE_WITH_UNIT", vec![]),
            p21::leaf("MEASURE_REPRESENTATION_ITEM", vec![]),
        ])
        .unwrap();
        let names: Vec<&str> = c.leaves().iter().map(|l| l.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "LENGTH_MEASURE_WITH_UNIT",
                "MEASURE_REPRESENTATION_ITEM",
                "REPRESENTATION_ITEM"
            ]
        );
    }

    #[test]
    fn decimal_mantissas_and_exponents_are_encoded_as_part21_reals() {
        for (stated, encoded) in [("35", "35."), ("1.e-3", "1.E-3"), ("2E1", "2.E1")] {
            assert_eq!(
                real_text(&Decimal::parse(stated).unwrap()).unwrap(),
                encoded
            );
        }
        for t in ["35.", "0.0030", "-2.54E1", "1.E-07"] {
            assert!(real(&Decimal::parse(t).unwrap()).is_ok(), "{t}");
        }
    }

    #[test]
    fn stated_verification_accepts_real_encoding_but_retains_legal_precision() {
        let model = |text| PartPmi {
            attributes: vec![AttributeSet {
                name: "REAL".into(),
                on: None,
                description: None,
                general_property: None,
                items: vec![(
                    "value".into(),
                    AttributeValue::Real(Decimal::parse(text).unwrap()),
                )],
            }],
            ..PartPmi::default()
        };
        for (input, native) in [("42", "42."), ("2e1", "2.E1"), ("1.e-3", "1.E-3")] {
            assert!(differences_as_stated(&model(input), &model(native)).is_empty());
        }
        assert!(!differences_as_stated(&model("42.0"), &model("42.")).is_empty());
    }

    #[test]
    fn deviations_cannot_be_given_as_a_magnitude() {
        // A lower deviation exists only as a signed value of `Bounds`, whose upper is above its
        // lower: "0.05 below" is -0.05, never a magnitude with an implied sign.
        let mm = |t: &str| Value::length(Decimal::parse(t).unwrap(), LengthUnit::Millimetre);
        assert!(Bounds::new(mm("0.1"), mm("0.1")).is_err());
        assert!(Bounds::new(mm("-0.025"), mm("-0.009")).is_err());
        let g6 = Bounds::new(mm("-0.009"), mm("-0.025")).unwrap();
        assert_eq!(g6.lower().decimal().as_str(), "-0.025");
    }

    #[test]
    fn zone_form_names_are_table_13s() {
        assert_eq!(
            zone_form_name(&ZoneForm::WithinACylinder),
            "within a cylinder"
        );
        assert_eq!(zone_form_name(&ZoneForm::Other(String::new())), "");
    }
    #[test]
    fn practice_exemptions_do_not_extend_to_other_rules_or_instances() {
        let violations = [
            (1, "datum_target.WR5"),
            (1, "datum_target.WR3"),
            (1, "datum_target.WR4"),
            (2, "datum_target.WR5"),
            (3, "default_tolerance_table_cell.WR3"),
            (3, "default_tolerance_table_cell.WR5"),
            (4, "default_tolerance_table_cell.WR3"),
        ]
        .into_iter()
        .map(|(id, rule)| RuleViolation {
            id,
            rule,
            entity: "test".into(),
            message: "raw violation".into(),
        })
        .collect();
        let allowed = BTreeSet::from([
            (1, "datum_target.WR5"),
            (3, "default_tolerance_table_cell.WR3"),
            (3, "default_tolerance_table_cell.WR4"),
        ]);
        let (known, bad) = partition_practice_conflicts(violations, &allowed);
        assert_eq!(
            known.iter().map(|v| (v.id, v.rule)).collect::<Vec<_>>(),
            [
                (1, "datum_target.WR5"),
                (3, "default_tolerance_table_cell.WR3")
            ]
        );
        assert_eq!(
            bad.iter().map(|v| (v.id, v.rule)).collect::<Vec<_>>(),
            [
                (1, "datum_target.WR3"),
                (1, "datum_target.WR4"),
                (2, "datum_target.WR5"),
                (3, "default_tolerance_table_cell.WR5"),
                (4, "default_tolerance_table_cell.WR3")
            ]
        );
    }
}
