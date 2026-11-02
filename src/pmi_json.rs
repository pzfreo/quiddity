//! The versioned JSON form of haecceity's semantic PMI model ([`haecceity::pmi::PartPmi`]), the
//! process boundary specify-core and draftwright use (`quiddity pmi read`, `quiddity pmi check`;
//! design: `docs/step-ap242.md`, Architecture item 7).
//!
//! The conversion is written by hand over [`serde_json::Value`] rather than derived on mirror
//! types: the model's invariants live in its constructors (private fields), so decoding has to
//! call them anyway, and a hand-written decoder names the JSON path of every refusal and refuses
//! unknown fields and unknown terms without a second copy of every type.
//!
//! The form, by rule:
//!
//! - **Values** are `{"value": "<decimal text as stated>", "unit": <unit>}` (plus
//!   `"decimal_places"` for a [`Value`] that states them, §5.4). Never a JSON number, so `0.0030`
//!   stays `0.0030`. Length units are `"mm"`, `"um"`, `"cm"`, `"m"`, `"in"`, `"ft"` or
//!   `{"name": …, "metres": "<decimal>"}`; angle units `"rad"`, `"deg"` or `{"name": …, "radians":
//!   "<decimal>"}`. The unit says whether a value is a length or an angle.
//! - **Anchors** are `{"face": n}`, `{"edge": n}` (the part's face and edge indices in
//!   `step::read_part_definitions`' numbering, from 0) or `{"geometry": n}`.
//! - **References** between items are indices into the part's own lists (`features`,
//!   `datums`, …), as in the model.
//! - **Terms**: tolerance kinds by their ISO 1101 names (`"position"`, `"circular run-out"`);
//!   ISO 286 classes as `{"deviation": "H", "grade": "IT7"}`; schema enumerations as their
//!   EXPRESS value in lower case (`"maximum_material_requirement"`, `"all_over"`); strings the PMI
//!   practice defines as the practice writes them (`"diameter"`, `"linear distance"`,
//!   `"within a cylinder"`, `"multiple elements"`, `"envelope requirement"`). Where the model
//!   keeps a name the practice does not list (a size or location kind, a zone form, a qualifier),
//!   it is written `{"other": "<name>"}`, never as a bare string: a bare string must be a
//!   standard term, so a misspelt term is refused rather than kept as a free-text name.
//! - Optional fields and empty lists are omitted on output and may be omitted (or `null`) on
//!   input; every other field is required. Unknown fields are refused.
//! - No `#N` appears in the model's form; [`Finding`]s cite instance ids as file evidence.
//!
//! A document ([`Document`]) adds the format name and version and the binding (the STEP text's
//! sha256 and the reader version) that the anchors are numbered against.

use std::fmt;

use haecceity::pmi::standards::{GeometricClass, Iso2768, LinearClass};
use haecceity::pmi::*;
use haecceity::step::PartDefinition;
use serde_json::{Map, Value as Json, json};

/// The format name every PMI document carries.
pub const FORMAT: &str = "quiddity-pmi";
/// The format name of the parts listing (`quiddity parts`).
pub const PARTS_FORMAT: &str = "quiddity-parts";
/// The format version: bumped on any change to the JSON form.
pub const VERSION: u64 = 2;
/// The reader the anchors and items were produced by; bumped whenever `pmi::read`'s output for
/// a file can change, so a document read by another reader version is refused, not trusted.
pub const READER: &str = "haecceity-pmi-read/15";

/// Why a JSON document was refused: the JSON path and the reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError {
    pub path: String,
    pub message: String,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

impl std::error::Error for JsonError {}

type R<T> = Result<T, JsonError>;

fn fail<T>(path: &str, message: impl Into<String>) -> R<T> {
    Err(JsonError {
        path: path.to_string(),
        message: message.into(),
    })
}

/// What a document's anchors are numbered against: the STEP text and the reader.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    /// Lower-case hex sha256 of the STEP text (after gunzip for a `.gz` file).
    pub sha256: String,
    pub reader: String,
}

impl Binding {
    /// The binding of a STEP text under this reader.
    #[must_use]
    pub fn of(step: &[u8]) -> Binding {
        use sha2::Digest;
        let digest = sha2::Sha256::digest(step);
        Binding {
            sha256: digest.iter().map(|b| format!("{b:02x}")).collect(),
            reader: READER.to_string(),
        }
    }
}

/// One part's PMI in a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentPart {
    pub part: PartId,
    /// The product's name, as `read_part_definitions` gives it (informative; checked against
    /// the file when given).
    pub name: Option<String>,
    pub pmi: PartPmi,
}

/// A PMI document: the binding and the PMI of some parts of one file, and the reader's findings
/// (output only: read back, not interpreted).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub binding: Binding,
    pub parts: Vec<DocumentPart>,
    pub findings: Vec<Finding>,
}

// ---------------------------------------------------------------------------------------------
// Decoding helpers
// ---------------------------------------------------------------------------------------------

/// A JSON object being decoded: its path, and the keys it may have.
struct Obj<'a> {
    path: String,
    map: &'a Map<String, Json>,
}

fn object<'a>(v: &'a Json, path: &str, allowed: &[&str]) -> R<Obj<'a>> {
    let Some(map) = v.as_object() else {
        return fail(path, format!("expected an object, found {}", kind_of(v)));
    };
    for key in map.keys() {
        if !allowed.contains(&key.as_str()) {
            return fail(
                &field(path, key),
                format!("unknown field (expected one of {})", allowed.join(", ")),
            );
        }
    }
    Ok(Obj {
        path: path.to_string(),
        map,
    })
}

fn kind_of(v: &Json) -> &'static str {
    match v {
        Json::Null => "null",
        Json::Bool(_) => "a boolean",
        Json::Number(_) => "a number",
        Json::String(_) => "a string",
        Json::Array(_) => "an array",
        Json::Object(_) => "an object",
    }
}

fn field(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

fn index(path: &str, i: usize) -> String {
    format!("{path}[{i}]")
}

impl<'a> Obj<'a> {
    fn path(&self, key: &str) -> String {
        field(&self.path, key)
    }

    /// The field, `None` when absent or null.
    fn opt(&self, key: &str) -> Option<&'a Json> {
        self.map.get(key).filter(|v| !v.is_null())
    }

    fn req(&self, key: &str) -> R<&'a Json> {
        match self.opt(key) {
            Some(v) => Ok(v),
            None => fail(&self.path(key), "missing"),
        }
    }

    fn opt_with<T>(&self, key: &str, f: impl FnOnce(&Json, &str) -> R<T>) -> R<Option<T>> {
        self.opt(key).map(|v| f(v, &self.path(key))).transpose()
    }

    fn req_with<T>(&self, key: &str, f: impl FnOnce(&Json, &str) -> R<T>) -> R<T> {
        f(self.req(key)?, &self.path(key))
    }

    /// A list field; absent or null is empty.
    fn list<T>(&self, key: &str, f: impl Fn(&Json, &str) -> R<T>) -> R<Vec<T>> {
        match self.opt(key) {
            None => Ok(Vec::new()),
            Some(v) => list(v, &self.path(key), f),
        }
    }
}

fn list<T>(v: &Json, path: &str, f: impl Fn(&Json, &str) -> R<T>) -> R<Vec<T>> {
    let Some(items) = v.as_array() else {
        return fail(path, format!("expected an array, found {}", kind_of(v)));
    };
    items
        .iter()
        .enumerate()
        .map(|(i, x)| f(x, &index(path, i)))
        .collect()
}

fn string(v: &Json, path: &str) -> R<String> {
    match v.as_str() {
        Some(s) => Ok(s.to_string()),
        None => fail(path, format!("expected a string, found {}", kind_of(v))),
    }
}

fn boolean(v: &Json, path: &str) -> R<bool> {
    match v.as_bool() {
        Some(b) => Ok(b),
        None => fail(path, format!("expected a boolean, found {}", kind_of(v))),
    }
}

fn uint<T: TryFrom<u64>>(v: &Json, path: &str) -> R<T> {
    match v.as_u64().and_then(|n| T::try_from(n).ok()) {
        Some(n) => Ok(n),
        None => fail(
            path,
            format!("expected a non-negative integer in range, found {v}"),
        ),
    }
}

fn int(v: &Json, path: &str) -> R<i64> {
    match v.as_i64() {
        Some(n) => Ok(n),
        None => fail(path, format!("expected an integer, found {v}")),
    }
}

fn model<T>(r: Result<T, ModelError>, path: &str) -> R<T> {
    r.or_else(|e| fail(path, e.0))
}

fn decimal(v: &Json, path: &str) -> R<Decimal> {
    let s = string(v, path)?;
    model(Decimal::parse(&s), path)
}

/// An object with exactly one key, for enums that carry data: the key and its value.
fn tagged<'a>(v: &'a Json, path: &str, tags: &[&str]) -> R<(&'a str, &'a Json, String)> {
    let o = object(v, path, tags)?;
    let mut it = o.map.iter();
    match (it.next(), it.next()) {
        (Some((k, x)), None) => Ok((k.as_str(), x, field(path, k))),
        _ => fail(path, format!("expected exactly one of {}", tags.join(", "))),
    }
}

/// A term from a table of standard terms.
fn term<T: Copy>(table: &[(&'static str, T)], v: &Json, path: &str) -> R<T> {
    let s = string(v, path)?;
    match table.iter().find(|(n, _)| *n == s) {
        Some(&(_, t)) => Ok(t),
        None => fail(
            path,
            format!(
                "unknown term {s:?} (expected one of {})",
                table.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
            ),
        ),
    }
}

/// A term from a table of standard terms, or `{"other": name}` for a name outside the table
/// (the model's `Other` variants). A bare string outside the table is refused, so a misspelt
/// standard term cannot pass as a free-text name; an `other` naming a standard term is refused
/// too, so each value has one form.
fn open_term<T: Clone>(
    table: &[(&'static str, T)],
    other: impl FnOnce(String) -> T,
    v: &Json,
    path: &str,
) -> R<T> {
    if let Some(s) = v.as_str() {
        return match table.iter().find(|(n, _)| *n == s) {
            Some((_, t)) => Ok(t.clone()),
            None => fail(
                path,
                format!(
                    "unknown term {s:?} (expected one of {}, or {{\"other\": <name>}} for a name \
                     the practice does not list)",
                    table.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
                ),
            ),
        };
    }
    let (_, x, p) = tagged(v, path, &["other"])?;
    let s = string(x, &p)?;
    if table.iter().any(|(n, _)| *n == s) {
        return fail(
            &p,
            format!("{s:?} is a standard term; write it as a bare string"),
        );
    }
    Ok(other(s))
}

/// The JSON of an open term: the bare name, or `{"other": name}` for an `Other` variant.
fn open_term_json(name: &str, is_other: bool) -> Json {
    if is_other {
        one("other", json!(name))
    } else {
        json!(name)
    }
}

const QUALIFIERS: [(&str, Qualifier); 3] = [
    ("maximum", Qualifier::Maximum),
    ("minimum", Qualifier::Minimum),
    ("average", Qualifier::Average),
];

fn name_of<T: Copy + PartialEq>(table: &[(&'static str, T)], t: T) -> &'static str {
    table
        .iter()
        .find(|(_, x)| *x == t)
        .map(|(n, _)| *n)
        .expect("every variant is in its table")
}

/// A schema enumeration value in lower case (the model's tables are upper case).
fn schema_enum<T: Copy>(table: &[(&'static str, T)], v: &Json, path: &str) -> R<T> {
    let s = string(v, path)?;
    let found = (s == s.to_ascii_lowercase())
        .then(|| table.iter().find(|(n, _)| n.eq_ignore_ascii_case(&s)))
        .flatten();
    match found {
        Some(&(_, t)) => Ok(t),
        None => fail(
            path,
            format!(
                "unknown term {s:?} (expected one of {})",
                table
                    .iter()
                    .map(|(n, _)| n.to_ascii_lowercase())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ),
    }
}

fn schema_name<T: Copy + PartialEq>(table: &[(&'static str, T)], t: T) -> String {
    name_of(table, t).to_ascii_lowercase()
}

/// Builds an output object, leaving out `None` and empty lists.
struct Out(Map<String, Json>);

impl Out {
    fn new() -> Out {
        Out(Map::new())
    }

    fn put(mut self, key: &str, v: Json) -> Out {
        self.0.insert(key.to_string(), v);
        self
    }

    fn opt(self, key: &str, v: Option<Json>) -> Out {
        match v {
            Some(v) => self.put(key, v),
            None => self,
        }
    }

    fn list(self, key: &str, v: Vec<Json>) -> Out {
        if v.is_empty() {
            self
        } else {
            self.put(key, Json::Array(v))
        }
    }

    fn done(self) -> Json {
        Json::Object(self.0)
    }
}

fn one(key: &str, v: Json) -> Json {
    Out::new().put(key, v).done()
}

// ---------------------------------------------------------------------------------------------
// Tables of terms
// ---------------------------------------------------------------------------------------------

const LENGTH_UNITS: [(&str, LengthUnitTag); 6] = [
    ("mm", LengthUnitTag::Millimetre),
    ("um", LengthUnitTag::Micrometre),
    ("cm", LengthUnitTag::Centimetre),
    ("m", LengthUnitTag::Metre),
    ("in", LengthUnitTag::Inch),
    ("ft", LengthUnitTag::Foot),
];

#[derive(Clone, Copy, PartialEq)]
enum LengthUnitTag {
    Millimetre,
    Micrometre,
    Centimetre,
    Metre,
    Inch,
    Foot,
}

const ANGLE_UNITS: [(&str, bool); 2] = [("rad", true), ("deg", false)];

/// ISO 1101:2017 Table 2's names of the geometrical characteristics.
const TOLERANCE_KINDS: [(&str, ToleranceKind); 15] = [
    ("straightness", ToleranceKind::Straightness),
    ("flatness", ToleranceKind::Flatness),
    ("roundness", ToleranceKind::Roundness),
    ("cylindricity", ToleranceKind::Cylindricity),
    ("line profile", ToleranceKind::LineProfile),
    ("surface profile", ToleranceKind::SurfaceProfile),
    ("parallelism", ToleranceKind::Parallelism),
    ("perpendicularity", ToleranceKind::Perpendicularity),
    ("angularity", ToleranceKind::Angularity),
    ("position", ToleranceKind::Position),
    ("concentricity", ToleranceKind::Concentricity),
    ("coaxiality", ToleranceKind::Coaxiality),
    ("symmetry", ToleranceKind::Symmetry),
    ("circular run-out", ToleranceKind::CircularRunout),
    ("total run-out", ToleranceKind::TotalRunout),
];

const GROUP_KINDS: [(&str, GroupKindTag); 5] = [
    ("multiple elements", GroupKindTag::MultipleElements),
    ("pattern of features", GroupKindTag::PatternOfFeatures),
    ("all around", GroupKindTag::AllAround),
    ("between", GroupKindTag::Between),
    ("unstated", GroupKindTag::Unstated),
];

#[derive(Clone, Copy, PartialEq)]
enum GroupKindTag {
    MultipleElements,
    PatternOfFeatures,
    AllAround,
    Between,
    Unstated,
}

/// The schema entity of each derived kind (§5.1.4 Table 3).
const DERIVED_KINDS: [(&str, DerivedKind); 8] = [
    ("derived_shape_aspect", DerivedKind::Derived),
    ("apex", DerivedKind::Apex),
    ("centre_of_symmetry", DerivedKind::CentreOfSymmetry),
    ("geometric_alignment", DerivedKind::GeometricAlignment),
    ("perpendicular_to", DerivedKind::PerpendicularTo),
    ("extension", DerivedKind::Extension),
    ("tangent", DerivedKind::Tangent),
    ("parallel_offset", DerivedKind::ParallelOffset),
];

/// Schema `datum_reference_modifier_type`.
const MODIFIER_TYPES: [(&str, DatumModifierType); 4] = [
    (
        "CIRCULAR_OR_CYLINDRICAL",
        DatumModifierType::CircularOrCylindrical,
    ),
    ("SPHERICAL", DatumModifierType::Spherical),
    ("DISTANCE", DatumModifierType::Distance),
    ("PROJECTED", DatumModifierType::Projected),
];

/// Schema `angle_relator`.
const ANGLE_SELECTIONS: [(&str, AngleSelection); 3] = [
    ("EQUAL", AngleSelection::Equal),
    ("LARGE", AngleSelection::Large),
    ("SMALL", AngleSelection::Small),
];

/// §5.2.1 Table 5.
const PRINCIPLES: [(&str, Principle); 2] = [
    ("independency", Principle::Independency),
    ("envelope requirement", Principle::EnvelopeRequirement),
];

/// Schema `area_unit_type`.
const AREA_SHAPES: [(&str, AreaShape); 5] = [
    ("CIRCULAR", AreaShape::Circular),
    ("SQUARE", AreaShape::Square),
    ("RECTANGULAR", AreaShape::Rectangular),
    ("CYLINDRICAL", AreaShape::Cylindrical),
    ("SPHERICAL", AreaShape::Spherical),
];

/// Schema `geometric_tolerance_auxiliary_classification_enum`.
const AUXILIARY: [(&str, AuxiliaryClassification); 2] = [
    ("ALL_OVER", AuxiliaryClassification::AllOver),
    (
        "UNLESS_OTHERWISE_SPECIFIED",
        AuxiliaryClassification::UnlessOtherwiseSpecified,
    ),
];

/// `geometric_tolerance_relationship.name` (§6.9.9).
const RELATION_KINDS: [(&str, RelationKind); 3] = [
    ("composite tolerance", RelationKind::Composite),
    ("precedence", RelationKind::Precedence),
    ("simultaneity", RelationKind::Simultaneity),
];

const STANDARD_BODIES: [(&str, StandardBody); 3] = [
    ("asme", StandardBody::Asme),
    ("iso", StandardBody::Iso),
    ("other", StandardBody::Other),
];

/// 'thread side' (`thread` WR10).
const THREAD_SIDES: [(&str, ThreadSide); 2] = [
    ("internal", ThreadSide::Internal),
    ("external", ThreadSide::External),
];

/// 'hand' (`thread` WR8, `turned_knurl` WR10).
const HANDS: [(&str, Hand); 2] = [("left", Hand::Left), ("right", Hand::Right)];

/// ISO 10303-1110's material removal conditions ('any process allowed', …).
const MATERIAL_REMOVALS: [(&str, MaterialRemoval); 3] = [
    ("any_process_allowed", MaterialRemoval::AnyProcessAllowed),
    ("material_removal_required", MaterialRemoval::Required),
    ("no_material_removal", MaterialRemoval::NotAllowed),
];

/// `turned_knurl.description` (WR1).
const KNURL_PATTERNS: [(&str, KnurlPattern); 3] = [
    ("diamond", KnurlPattern::Diamond),
    ("diagonal", KnurlPattern::Diagonal),
    ("straight", KnurlPattern::Straight),
];

/// ISO 2768-1 classes.
const LINEAR_CLASSES: [(&str, LinearClass); 4] = [
    ("f", LinearClass::Fine),
    ("m", LinearClass::Medium),
    ("c", LinearClass::Coarse),
    ("v", LinearClass::VeryCoarse),
];

/// ISO 2768-2 classes.
const GEOMETRIC_CLASSES: [(&str, GeometricClass); 3] = [
    ("H", GeometricClass::H),
    ("K", GeometricClass::K),
    ("L", GeometricClass::L),
];

const FINDING_KINDS: [FindingKind; 10] = [
    FindingKind::Unsupported,
    FindingKind::Nonconformance,
    FindingKind::Unresolved,
    FindingKind::Unitless,
    FindingKind::AssemblyPmi,
    FindingKind::OccurrencePmi,
    FindingKind::UndeterminedPractice,
    FindingKind::NotModelled,
    FindingKind::Conflict,
    FindingKind::Unconsumed,
];

// ---------------------------------------------------------------------------------------------
// Values and units
// ---------------------------------------------------------------------------------------------

fn length_unit_json(u: &LengthUnit) -> Json {
    let tag = match u {
        LengthUnit::Millimetre => LengthUnitTag::Millimetre,
        LengthUnit::Micrometre => LengthUnitTag::Micrometre,
        LengthUnit::Centimetre => LengthUnitTag::Centimetre,
        LengthUnit::Metre => LengthUnitTag::Metre,
        LengthUnit::Inch => LengthUnitTag::Inch,
        LengthUnit::Foot => LengthUnitTag::Foot,
        LengthUnit::Other { name, metres } => {
            return json!({"name": name, "metres": metres.as_str()});
        }
    };
    json!(name_of(&LENGTH_UNITS, tag))
}

fn angle_unit_json(u: &AngleUnit) -> Json {
    match u {
        AngleUnit::Radian => json!("rad"),
        AngleUnit::Degree => json!("deg"),
        AngleUnit::Other { name, radians } => json!({"name": name, "radians": radians.as_str()}),
    }
}

fn quantity_json(q: &Quantity) -> (Json, Json) {
    match q {
        Quantity::Length(l) => (json!(l.value.as_str()), length_unit_json(&l.unit)),
        Quantity::Angle(a) => (json!(a.value.as_str()), angle_unit_json(&a.unit)),
    }
}

fn value_json(v: &Value) -> Json {
    let (value, unit) = quantity_json(&v.quantity);
    Out::new()
        .put("value", value)
        .put("unit", unit)
        .opt("decimal_places", v.decimal_places.map(|p| json!(p)))
        .done()
}

fn length_json(l: &Length) -> Json {
    json!({"value": l.value.as_str(), "unit": length_unit_json(&l.unit)})
}

fn angle_json(a: &Angle) -> Json {
    json!({"value": a.value.as_str(), "unit": angle_unit_json(&a.unit)})
}

/// A unit: `Ok(length unit)` or `Err(angle unit)`.
fn unit_from(v: &Json, path: &str) -> R<Result<LengthUnit, AngleUnit>> {
    if let Some(s) = v.as_str() {
        if let Some(&(_, tag)) = LENGTH_UNITS.iter().find(|(n, _)| *n == s) {
            return Ok(Ok(match tag {
                LengthUnitTag::Millimetre => LengthUnit::Millimetre,
                LengthUnitTag::Micrometre => LengthUnit::Micrometre,
                LengthUnitTag::Centimetre => LengthUnit::Centimetre,
                LengthUnitTag::Metre => LengthUnit::Metre,
                LengthUnitTag::Inch => LengthUnit::Inch,
                LengthUnitTag::Foot => LengthUnit::Foot,
            }));
        }
        if let Some(&(_, radian)) = ANGLE_UNITS.iter().find(|(n, _)| *n == s) {
            return Ok(Err(if radian {
                AngleUnit::Radian
            } else {
                AngleUnit::Degree
            }));
        }
        return fail(
            path,
            format!("unknown unit {s:?} (expected mm, um, cm, m, in, ft, rad, deg or an object)"),
        );
    }
    let o = object(v, path, &["name", "metres", "radians"])?;
    let name = o.req_with("name", string)?;
    match (o.opt("metres"), o.opt("radians")) {
        (Some(_), None) => Ok(Ok(LengthUnit::Other {
            name,
            metres: o.req_with("metres", decimal)?,
        })),
        (None, Some(_)) => Ok(Err(AngleUnit::Other {
            name,
            radians: o.req_with("radians", decimal)?,
        })),
        _ => fail(path, "a unit object states exactly one of metres, radians"),
    }
}

fn quantity_from(o: &Obj) -> R<Quantity> {
    let value = o.req_with("value", decimal)?;
    Ok(match o.req_with("unit", unit_from)? {
        Ok(unit) => Quantity::Length(Length { value, unit }),
        Err(unit) => Quantity::Angle(Angle { value, unit }),
    })
}

fn value_from(v: &Json, path: &str) -> R<Value> {
    let o = object(v, path, &["value", "unit", "decimal_places"])?;
    Ok(Value {
        quantity: quantity_from(&o)?,
        decimal_places: o.opt_with("decimal_places", uint::<u8>)?,
    })
}

fn length_value_from(v: &Json, path: &str) -> R<Value> {
    let value = value_from(v, path)?;
    if !matches!(value.quantity, Quantity::Length(_)) {
        return fail(&field(path, "unit"), "expected a length unit");
    }
    Ok(value)
}

fn length_from(v: &Json, path: &str) -> R<Length> {
    let o = object(v, path, &["value", "unit"])?;
    match quantity_from(&o)? {
        Quantity::Length(l) => Ok(l),
        Quantity::Angle(_) => fail(&field(path, "unit"), "expected a length unit"),
    }
}

fn angle_from(v: &Json, path: &str) -> R<Angle> {
    let o = object(v, path, &["value", "unit"])?;
    match quantity_from(&o)? {
        Quantity::Angle(a) => Ok(a),
        Quantity::Length(_) => fail(&field(path, "unit"), "expected an angle unit"),
    }
}

fn bounds_json(b: &Bounds) -> Json {
    json!({"upper": value_json(b.upper()), "lower": value_json(b.lower())})
}

fn bounds_from(v: &Json, path: &str) -> R<Bounds> {
    let o = object(v, path, &["upper", "lower"])?;
    let upper = o.req_with("upper", value_from)?;
    let lower = o.req_with("lower", value_from)?;
    model(Bounds::new(upper, lower), path)
}

// ---------------------------------------------------------------------------------------------
// Anchors, geometry, features
// ---------------------------------------------------------------------------------------------

fn anchor_json(a: &Anchor) -> Json {
    match a {
        Anchor::Face(f) => one("face", json!(f.0)),
        Anchor::Edge(e) => one("edge", json!(e.0)),
        Anchor::Geometry(g) => one("geometry", json!(g.0)),
    }
}

fn anchor_from(v: &Json, path: &str) -> R<Anchor> {
    let (tag, x, p) = tagged(v, path, &["face", "edge", "geometry"])?;
    let n: usize = uint(x, &p)?;
    Ok(match tag {
        "face" => Anchor::Face(FaceIndex(n)),
        "edge" => Anchor::Edge(EdgeIndex(n)),
        _ => Anchor::Geometry(GeometryId(n)),
    })
}

fn geometry_item_json(g: &GeometryItem) -> Json {
    let leaves: Vec<Json> = g
        .leaves
        .iter()
        .map(|(name, values)| {
            json!({"entity": name, "attributes": values.iter().map(geometry_value_json).collect::<Vec<_>>()})
        })
        .collect();
    json!({ "leaves": leaves })
}

fn geometry_value_json(v: &GeometryValue) -> Json {
    match v {
        GeometryValue::Real(d) => one("real", json!(d.as_str())),
        GeometryValue::Integer(i) => one("integer", json!(i)),
        GeometryValue::Text(s) => one("text", json!(s)),
        GeometryValue::Enum(s) => one("enum", json!(s)),
        GeometryValue::Binary(s) => one("binary", json!(s)),
        GeometryValue::Item(g) => one("item", geometry_item_json(g)),
        GeometryValue::List(l) => one(
            "list",
            Json::Array(l.iter().map(geometry_value_json).collect()),
        ),
        GeometryValue::Typed { type_name, value } => one(
            "typed",
            json!({"type": type_name, "value": geometry_value_json(value)}),
        ),
        GeometryValue::Unset => json!("unset"),
        GeometryValue::Derived => json!("derived"),
    }
}

fn geometry_item_from(v: &Json, path: &str) -> R<GeometryItem> {
    let o = object(v, path, &["leaves"])?;
    let leaves = o.list("leaves", |x, p| {
        let l = object(x, p, &["entity", "attributes"])?;
        Ok((
            l.req_with("entity", string)?,
            l.list("attributes", geometry_value_from)?,
        ))
    })?;
    if leaves.is_empty() {
        return fail(
            &field(path, "leaves"),
            "a geometric item has at least one leaf",
        );
    }
    Ok(GeometryItem { leaves })
}

fn geometry_value_from(v: &Json, path: &str) -> R<GeometryValue> {
    match v.as_str() {
        Some("unset") => return Ok(GeometryValue::Unset),
        Some("derived") => return Ok(GeometryValue::Derived),
        Some(s) => {
            return fail(
                path,
                format!("unknown term {s:?} (expected unset, derived or an object)"),
            );
        }
        None => {}
    }
    let tags = [
        "real", "integer", "text", "enum", "binary", "item", "list", "typed",
    ];
    let (tag, x, p) = tagged(v, path, &tags)?;
    Ok(match tag {
        "real" => GeometryValue::Real(decimal(x, &p)?),
        "integer" => GeometryValue::Integer(int(x, &p)?),
        "text" => GeometryValue::Text(string(x, &p)?),
        "enum" => GeometryValue::Enum(string(x, &p)?),
        "binary" => GeometryValue::Binary(string(x, &p)?),
        "item" => GeometryValue::Item(Box::new(geometry_item_from(x, &p)?)),
        "list" => GeometryValue::List(list(x, &p, geometry_value_from)?),
        _ => {
            let o = object(x, &p, &["type", "value"])?;
            GeometryValue::Typed {
                type_name: o.req_with("type", string)?,
                value: Box::new(o.req_with("value", geometry_value_from)?),
            }
        }
    })
}

fn geometry_json(g: &Geometry) -> Json {
    Out::new()
        .put("item", geometry_item_json(&g.item))
        .opt("length_unit", g.length_unit.as_ref().map(length_unit_json))
        .done()
}

fn geometry_from(v: &Json, path: &str) -> R<Geometry> {
    let o = object(v, path, &["item", "length_unit"])?;
    Ok(Geometry {
        item: o.req_with("item", geometry_item_from)?,
        length_unit: o.opt_with("length_unit", |x, p| match unit_from(x, p)? {
            Ok(u) => Ok(u),
            Err(_) => fail(p, "expected a length unit"),
        })?,
    })
}

fn derived_geometry_json(g: &DerivedGeometry) -> Json {
    Out::new()
        .put(
            "kind",
            json!(match g.kind {
                DerivedGeometryKind::Axis => "axis",
                DerivedGeometryKind::MedianPlane => "median_plane",
            }),
        )
        .put("deriving_faces", json!(ids(&g.deriving_faces, |f| f.0)))
        .put(
            "point",
            Json::Array(g.point.iter().map(length_json).collect()),
        )
        .put("direction", direction_json(&g.direction))
        .opt("radius", g.radius.as_ref().map(length_json))
        .list("supplemental", ids(&g.supplemental, |s| s.0))
        .done()
}

fn derived_geometry_from(v: &Json, path: &str) -> R<DerivedGeometry> {
    let o = object(
        v,
        path,
        &[
            "kind",
            "deriving_faces",
            "point",
            "direction",
            "radius",
            "supplemental",
        ],
    )?;
    let point = o.req_with("point", |x, p| list(x, p, length_from))?;
    let point = <[Length; 3]>::try_from(point).map_err(|_| JsonError {
        path: o.path("point"),
        message: "a point has three coordinates".into(),
    })?;
    Ok(DerivedGeometry {
        kind: o.req_with("kind", |x, p| match string(x, p)?.as_str() {
            "axis" => Ok(DerivedGeometryKind::Axis),
            "median_plane" => Ok(DerivedGeometryKind::MedianPlane),
            s => fail(p, format!("unknown derived geometry kind {s:?}")),
        })?,
        deriving_faces: o.list("deriving_faces", |x, p| uint(x, p).map(FaceIndex))?,
        point,
        direction: o.req_with("direction", direction_from)?,
        radius: o.opt_with("radius", length_from)?,
        supplemental: o.list("supplemental", |x, p| uint(x, p).map(GeometryId))?,
    })
}

fn ids<T>(v: &[T], f: impl Fn(&T) -> usize) -> Vec<Json> {
    v.iter().map(|x| json!(f(x))).collect()
}

fn feature_json(f: &Feature) -> Json {
    match f {
        Feature::Items(items) => one(
            "items",
            Json::Array(items.iter().map(anchor_json).collect()),
        ),
        Feature::Group { members, kind } => {
            let tag = match kind {
                GroupKind::MultipleElements => GroupKindTag::MultipleElements,
                GroupKind::PatternOfFeatures => GroupKindTag::PatternOfFeatures,
                GroupKind::AllAround => GroupKindTag::AllAround,
                GroupKind::Between => GroupKindTag::Between,
                GroupKind::Unstated => GroupKindTag::Unstated,
            };
            one(
                "group",
                json!({"kind": name_of(&GROUP_KINDS, tag), "members": ids(members, |m| m.0)}),
            )
        }
        Feature::Derived {
            kind,
            from,
            geometry,
        } => one(
            "derived",
            Out::new()
                .put("kind", json!(name_of(&DERIVED_KINDS, *kind)))
                .put("from", json!(ids(from, |m| m.0)))
                .opt("geometry", geometry.map(|g| json!(g.0)))
                .done(),
        ),
    }
}

fn feature_id(v: &Json, path: &str) -> R<FeatureId> {
    uint(v, path).map(FeatureId)
}

fn feature_from(v: &Json, path: &str) -> R<Feature> {
    let (tag, x, p) = tagged(v, path, &["items", "group", "derived"])?;
    Ok(match tag {
        "items" => Feature::Items(list(x, &p, anchor_from)?),
        "group" => {
            let o = object(x, &p, &["kind", "members"])?;
            let kind = match o.req_with("kind", |v, p| term(&GROUP_KINDS, v, p))? {
                GroupKindTag::MultipleElements => GroupKind::MultipleElements,
                GroupKindTag::PatternOfFeatures => GroupKind::PatternOfFeatures,
                GroupKindTag::AllAround => GroupKind::AllAround,
                GroupKindTag::Between => GroupKind::Between,
                GroupKindTag::Unstated => GroupKind::Unstated,
            };
            Feature::Group {
                members: o.list("members", feature_id)?,
                kind,
            }
        }
        _ => {
            let o = object(x, &p, &["kind", "from", "geometry"])?;
            Feature::Derived {
                kind: o.req_with("kind", |v, p| term(&DERIVED_KINDS, v, p))?,
                from: o.list("from", feature_id)?,
                geometry: o.opt_with("geometry", |x, p| uint(x, p).map(DerivedGeometryId))?,
            }
        }
    })
}

// ---------------------------------------------------------------------------------------------
// Datums
// ---------------------------------------------------------------------------------------------

fn direction_json(d: &Direction) -> Json {
    Json::Array(d.0.iter().map(|x| json!(x.as_str())).collect())
}

fn direction_from(v: &Json, path: &str) -> R<Direction> {
    let xs = list(v, path, decimal)?;
    match <[Decimal; 3]>::try_from(xs) {
        Ok(a) => Ok(Direction(a)),
        Err(_) => fail(path, "a direction has three ratios"),
    }
}

fn placement_json(p: &Placement) -> Json {
    Out::new()
        .put(
            "origin",
            Json::Array(p.origin.iter().map(length_json).collect()),
        )
        .opt("axis", p.axis.as_ref().map(direction_json))
        .opt(
            "ref_direction",
            p.ref_direction.as_ref().map(direction_json),
        )
        .done()
}

fn placement_from(v: &Json, path: &str) -> R<Placement> {
    let o = object(v, path, &["origin", "axis", "ref_direction"])?;
    let origin = o.req_with("origin", |x, p| list(x, p, length_from))?;
    let Ok(origin) = <[Length; 3]>::try_from(origin) else {
        return fail(&o.path("origin"), "an origin has three coordinates");
    };
    Ok(Placement {
        origin,
        axis: o.opt_with("axis", direction_from)?,
        ref_direction: o.opt_with("ref_direction", direction_from)?,
    })
}

fn target_shape_json(s: &TargetShape) -> Json {
    match s {
        TargetShape::Point => json!("point"),
        TargetShape::Line { length } => one("line", json!({"length": length_json(length)})),
        TargetShape::Rectangle { length, width } => one(
            "rectangle",
            json!({"length": length_json(length), "width": length_json(width)}),
        ),
        TargetShape::Circle { diameter } => {
            one("circle", json!({"diameter": length_json(diameter)}))
        }
        TargetShape::CircularCurve { diameter } => {
            one("circular curve", json!({"diameter": length_json(diameter)}))
        }
        TargetShape::Area(f) => one("area", json!(f.0)),
        TargetShape::Curve(f) => one("curve", json!(f.0)),
    }
}

fn target_shape_from(v: &Json, path: &str) -> R<TargetShape> {
    match v.as_str() {
        Some("point") => return Ok(TargetShape::Point),
        Some(s) => {
            return fail(
                path,
                format!("unknown term {s:?} (expected point or an object)"),
            );
        }
        None => {}
    }
    let tags = [
        "line",
        "rectangle",
        "circle",
        "circular curve",
        "area",
        "curve",
    ];
    let (tag, x, p) = tagged(v, path, &tags)?;
    Ok(match tag {
        "line" => {
            let o = object(x, &p, &["length"])?;
            TargetShape::Line {
                length: o.req_with("length", length_from)?,
            }
        }
        "rectangle" => {
            let o = object(x, &p, &["length", "width"])?;
            TargetShape::Rectangle {
                length: o.req_with("length", length_from)?,
                width: o.req_with("width", length_from)?,
            }
        }
        "circle" | "circular curve" => {
            let o = object(x, &p, &["diameter"])?;
            let diameter = o.req_with("diameter", length_from)?;
            if tag == "circle" {
                TargetShape::Circle { diameter }
            } else {
                TargetShape::CircularCurve { diameter }
            }
        }
        "area" => TargetShape::Area(feature_id(x, &p)?),
        _ => TargetShape::Curve(feature_id(x, &p)?),
    })
}

fn datum_target_json(t: &DatumTarget) -> Json {
    Out::new()
        .put("number", json!(t.number()))
        .put("shape", target_shape_json(t.shape()))
        .opt("placement", t.placement().map(placement_json))
        .opt("movable", t.movable().map(direction_json))
        .opt("on", t.on().map(|f| json!(f.0)))
        .done()
}

fn datum_target_from(v: &Json, path: &str) -> R<DatumTarget> {
    let o = object(v, path, &["number", "shape", "placement", "movable", "on"])?;
    model(
        DatumTarget::new(
            o.req_with("number", uint::<u32>)?,
            o.req_with("shape", target_shape_from)?,
            o.opt_with("placement", placement_from)?,
            o.opt_with("movable", direction_from)?,
            o.opt_with("on", feature_id)?,
        ),
        path,
    )
}

fn datum_json(d: &Datum) -> Json {
    Out::new()
        .put("label", json!(d.label().as_str()))
        .opt("feature", d.feature().map(|f| json!(f.0)))
        .list("targets", ids(d.targets(), |t| t.0))
        .done()
}

fn datum_from(v: &Json, path: &str) -> R<Datum> {
    let o = object(v, path, &["label", "feature", "targets"])?;
    let label = o.req_with("label", |x, p| model(DatumLabel::new(&string(x, p)?), p))?;
    model(
        Datum::new(
            label,
            o.opt_with("feature", feature_id)?,
            o.list("targets", |x, p| uint(x, p).map(DatumTargetId))?,
        ),
        path,
    )
}

fn datum_modifier_json(m: &DatumModifier) -> Json {
    match m {
        DatumModifier::Simple(s) => json!(schema_name(&SimpleDatumModifier::ALL, *s)),
        DatumModifier::WithValue { kind, value } => json!({
            "kind": schema_name(&MODIFIER_TYPES, *kind),
            "value": value_json(value),
        }),
    }
}

fn datum_modifier_from(v: &Json, path: &str) -> R<DatumModifier> {
    if v.is_string() {
        return schema_enum(&SimpleDatumModifier::ALL, v, path).map(DatumModifier::Simple);
    }
    let o = object(v, path, &["kind", "value"])?;
    Ok(DatumModifier::WithValue {
        kind: o.req_with("kind", |x, p| schema_enum(&MODIFIER_TYPES, x, p))?,
        value: o.req_with("value", value_from)?,
    })
}

fn datum_system_json(s: &DatumSystem) -> Json {
    Json::Array(
        s.compartments()
            .iter()
            .map(|c| {
                Out::new()
                    .put(
                        "references",
                        Json::Array(
                            c.references
                                .iter()
                                .map(|r| {
                                    Out::new()
                                        .put("datum", json!(r.datum.0))
                                        .list(
                                            "modifiers",
                                            r.modifiers.iter().map(datum_modifier_json).collect(),
                                        )
                                        .done()
                                })
                                .collect(),
                        ),
                    )
                    .list(
                        "modifiers",
                        c.modifiers.iter().map(datum_modifier_json).collect(),
                    )
                    .done()
            })
            .collect(),
    )
}

fn datum_system_from(v: &Json, path: &str) -> R<DatumSystem> {
    let compartments = list(v, path, |x, p| {
        let o = object(x, p, &["references", "modifiers"])?;
        Ok(Compartment {
            references: o.req_with("references", |x, p| {
                list(x, p, |x, p| {
                    let r = object(x, p, &["datum", "modifiers"])?;
                    Ok(DatumReference {
                        datum: r.req_with("datum", |x, p| uint(x, p).map(DatumId))?,
                        modifiers: r.list("modifiers", datum_modifier_from)?,
                    })
                })
            })?,
            modifiers: o.list("modifiers", datum_modifier_from)?,
        })
    })?;
    model(DatumSystem::new(compartments), path)
}

// ---------------------------------------------------------------------------------------------
// Dimensions
// ---------------------------------------------------------------------------------------------

fn dimension_modifier_name(m: &DimensionModifier) -> &'static str {
    if *m == DimensionModifier::Reference {
        return "auxiliary";
    }
    DimensionModifier::TABLE
        .iter()
        .find(|(_, x)| x == m)
        .map(|(n, _)| *n)
        .expect("every Table 8 modifier is in the table")
}

fn dimension_modifier_from(v: &Json, path: &str) -> R<DimensionModifier> {
    let s = string(v, path)?;
    if s == "auxiliary" {
        return Ok(DimensionModifier::Reference);
    }
    match DimensionModifier::from_description(&s) {
        Some(m) => Ok(m),
        None => fail(
            path,
            format!(
                "unknown term {s:?} (expected auxiliary or one of {})",
                DimensionModifier::TABLE
                    .iter()
                    .map(|(n, _)| *n)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ),
    }
}

fn qualifier_json(q: &Qualifier) -> Json {
    match q {
        Qualifier::Other(s) => open_term_json(s, true),
        k => open_term_json(
            QUALIFIERS
                .iter()
                .find(|(_, x)| x == k)
                .map(|(n, _)| *n)
                .expect("every named qualifier is in QUALIFIERS"),
            false,
        ),
    }
}

fn dimension_kind_json(k: &DimensionKind) -> Json {
    match k {
        DimensionKind::Size {
            feature,
            kind,
            path,
            angle,
        } => one(
            "size",
            Out::new()
                .put("feature", json!(feature.0))
                .put(
                    "kind",
                    open_term_json(kind.name(), matches!(kind, SizeKind::Other(_))),
                )
                .opt("path", path.map(|f| json!(f.0)))
                .opt(
                    "angle",
                    angle.map(|a| json!(schema_name(&ANGLE_SELECTIONS, a))),
                )
                .done(),
        ),
        DimensionKind::Location {
            from,
            to,
            kind,
            path,
            directed,
            angle,
        } => one(
            "location",
            Out::new()
                .put("from", json!(from.0))
                .put("to", json!(to.0))
                .put(
                    "kind",
                    open_term_json(kind.name(), matches!(kind, LocationKind::Other(_))),
                )
                .opt("path", path.map(|f| json!(f.0)))
                .put("directed", json!(directed))
                .opt(
                    "angle",
                    angle.map(|a| json!(schema_name(&ANGLE_SELECTIONS, a))),
                )
                .done(),
        ),
    }
}

fn dimension_kind_from(v: &Json, path: &str) -> R<DimensionKind> {
    let (tag, x, p) = tagged(v, path, &["size", "location"])?;
    let angle = |o: &Obj| o.opt_with("angle", |x, p| schema_enum(&ANGLE_SELECTIONS, x, p));
    Ok(if tag == "size" {
        let o = object(x, &p, &["feature", "kind", "path", "angle"])?;
        DimensionKind::Size {
            feature: o.req_with("feature", feature_id)?,
            kind: o.req_with("kind", |x, p| {
                open_term(&SizeKind::TABLE, SizeKind::Other, x, p)
            })?,
            path: o.opt_with("path", feature_id)?,
            angle: angle(&o)?,
        }
    } else {
        let o = object(x, &p, &["from", "to", "kind", "path", "directed", "angle"])?;
        DimensionKind::Location {
            from: o.req_with("from", feature_id)?,
            to: o.req_with("to", feature_id)?,
            kind: o.req_with("kind", |x, p| {
                open_term(&LocationKind::TABLE, LocationKind::Other, x, p)
            })?,
            path: o.opt_with("path", feature_id)?,
            directed: o.req_with("directed", boolean)?,
            angle: angle(&o)?,
        }
    })
}

fn iso286_json(c: &Iso286Class) -> (Json, Json) {
    (
        json!(c.deviation.to_string()),
        json!(format!("IT{}", c.grade.number())),
    )
}

fn dim_tolerance_json(t: &DimTolerance) -> Json {
    match t {
        DimTolerance::None => json!("none"),
        DimTolerance::Basic => json!("basic"),
        DimTolerance::Deviations(b) => one("deviations", bounds_json(b)),
        DimTolerance::Limits(b) => one("limits", bounds_json(b)),
        DimTolerance::Fit { class, limits } => {
            let (deviation, grade) = iso286_json(class);
            one(
                "fit",
                Out::new()
                    .put("deviation", deviation)
                    .put("grade", grade)
                    .opt("limits", limits.as_ref().map(bounds_json))
                    .done(),
            )
        }
    }
}

fn dim_tolerance_from(v: &Json, path: &str) -> R<DimTolerance> {
    match v.as_str() {
        Some("none") => return Ok(DimTolerance::None),
        Some("basic") => return Ok(DimTolerance::Basic),
        Some(s) => {
            return fail(
                path,
                format!("unknown term {s:?} (expected none, basic or an object)"),
            );
        }
        None => {}
    }
    let (tag, x, p) = tagged(v, path, &["deviations", "limits", "fit"])?;
    Ok(match tag {
        "deviations" => DimTolerance::Deviations(bounds_from(x, &p)?),
        "limits" => DimTolerance::Limits(bounds_from(x, &p)?),
        _ => {
            let o = object(x, &p, &["deviation", "grade", "limits"])?;
            let deviation = o.req_with("deviation", |x, p| {
                model(FundamentalDeviation::parse(&string(x, p)?), p)
            })?;
            let grade = o.req_with("grade", |x, p| {
                let s = string(x, p)?;
                if !s.starts_with("IT") {
                    return fail(
                        p,
                        format!("{s:?} is not an ISO 286 grade (IT01, IT0 … IT18)"),
                    );
                }
                model(ToleranceGrade::parse(&s), p)
            })?;
            DimTolerance::Fit {
                class: Iso286Class { deviation, grade },
                limits: o.opt_with("limits", bounds_from)?,
            }
        }
    })
}

fn dimension_json(d: &Dimension) -> Json {
    Out::new()
        .put("kind", dimension_kind_json(&d.kind))
        .opt("nominal", d.nominal.as_ref().map(value_json))
        .put("tolerance", dim_tolerance_json(&d.tolerance))
        .opt("qualifier", d.qualifier.as_ref().map(qualifier_json))
        .list(
            "modifiers",
            d.modifiers
                .iter()
                .map(|m| json!(dimension_modifier_name(m)))
                .collect(),
        )
        .opt(
            "principle",
            d.principle.map(|p| json!(name_of(&PRINCIPLES, p))),
        )
        .done()
}

fn dimension_from(v: &Json, path: &str) -> R<Dimension> {
    let keys = [
        "kind",
        "nominal",
        "tolerance",
        "qualifier",
        "modifiers",
        "principle",
    ];
    let o = object(v, path, &keys)?;
    Ok(Dimension {
        kind: o.req_with("kind", dimension_kind_from)?,
        nominal: o.opt_with("nominal", value_from)?,
        tolerance: o.req_with("tolerance", dim_tolerance_from)?,
        qualifier: o.opt_with("qualifier", |x, p| {
            open_term(&QUALIFIERS, Qualifier::Other, x, p)
        })?,
        modifiers: o.list("modifiers", dimension_modifier_from)?,
        principle: o.opt_with("principle", |x, p| term(&PRINCIPLES, x, p))?,
    })
}

// ---------------------------------------------------------------------------------------------
// Geometric tolerances
// ---------------------------------------------------------------------------------------------

fn target_json(t: &ToleranceTarget) -> Json {
    match t {
        ToleranceTarget::Feature(f) => one("feature", json!(f.0)),
        ToleranceTarget::Dimension(d) => one("dimension", json!(d.0)),
        ToleranceTarget::Relation {
            relating,
            related,
            name,
        } => one(
            "relation",
            json!({"relating": relating.0, "related": related.0, "name": name}),
        ),
        ToleranceTarget::WholePart => json!("whole part"),
    }
}

fn target_from(v: &Json, path: &str) -> R<ToleranceTarget> {
    match v.as_str() {
        Some("whole part") => return Ok(ToleranceTarget::WholePart),
        Some(s) => {
            return fail(
                path,
                format!("unknown term {s:?} (expected whole part or an object)"),
            );
        }
        None => {}
    }
    let (tag, x, p) = tagged(v, path, &["feature", "dimension", "relation"])?;
    Ok(match tag {
        "feature" => ToleranceTarget::Feature(feature_id(x, &p)?),
        "dimension" => ToleranceTarget::Dimension(uint(x, &p).map(DimensionId)?),
        _ => {
            let o = object(x, &p, &["relating", "related", "name"])?;
            ToleranceTarget::Relation {
                relating: o.req_with("relating", feature_id)?,
                related: o.req_with("related", feature_id)?,
                name: o.req_with("name", string)?,
            }
        }
    })
}

fn zone_form_json(f: &ZoneForm) -> Json {
    match f {
        ZoneForm::Other(s) => open_term_json(s, true),
        k => open_term_json(
            ZoneForm::TABLE
                .iter()
                .find(|(_, x)| x == k)
                .map(|(n, _)| *n)
                .expect("every named form is in Table 13"),
            false,
        ),
    }
}

const AFFECTED_PLANE_KINDS: [(&str, AffectedPlaneKind); 3] = [
    ("intersection line", AffectedPlaneKind::IntersectionLine),
    ("intersection plane", AffectedPlaneKind::IntersectionPlane),
    ("orientation plane", AffectedPlaneKind::OrientationPlane),
];

fn zone_json(z: &Zone) -> Json {
    Out::new()
        .put("form", zone_form_json(&z.form))
        .opt(
            "projected",
            z.projected.as_ref().map(|p| {
                Out::new()
                    .opt("end", p.end.map(|f| json!(f.0)))
                    .put("length", value_json(&p.length))
                    .done()
            }),
        )
        // `Some(vec![])` and `None` differ in the model, so an empty list is written.
        .opt(
            "non_uniform",
            z.non_uniform.as_ref().map(|v| Json::Array(ids(v, |f| f.0))),
        )
        .opt("runout_angle", z.runout_angle.as_ref().map(value_json))
        .opt("affected_plane", z.affected_plane.map(|f| json!(f.0)))
        .opt(
            "affected_plane_kind",
            z.affected_plane_kind
                .map(|k| json!(name_of(&AFFECTED_PLANE_KINDS, k))),
        )
        .done()
}

fn zone_from(v: &Json, path: &str) -> R<Zone> {
    let keys = [
        "form",
        "projected",
        "non_uniform",
        "runout_angle",
        "affected_plane",
        "affected_plane_kind",
    ];
    let o = object(v, path, &keys)?;
    Ok(Zone {
        form: o.req_with("form", |x, p| {
            open_term(&ZoneForm::TABLE, ZoneForm::Other, x, p)
        })?,
        projected: o.opt_with("projected", |x, p| {
            let o = object(x, p, &["end", "length"])?;
            Ok(ProjectedZone {
                end: o.opt_with("end", feature_id)?,
                length: o.req_with("length", value_from)?,
            })
        })?,
        non_uniform: o.opt_with("non_uniform", |x, p| list(x, p, feature_id))?,
        runout_angle: o.opt_with("runout_angle", value_from)?,
        affected_plane: o.opt_with("affected_plane", feature_id)?,
        affected_plane_kind: o.opt_with("affected_plane_kind", |x, p| {
            term(&AFFECTED_PLANE_KINDS, x, p)
        })?,
    })
}

fn tolerance_json(t: &GeometricTolerance) -> Json {
    Out::new()
        .put("kind", json!(name_of(&TOLERANCE_KINDS, t.kind)))
        .put("target", target_json(&t.target))
        .opt("magnitude", t.magnitude.as_ref().map(value_json))
        .opt("zone", t.zone.as_ref().map(zone_json))
        .list(
            "modifiers",
            t.modifiers
                .iter()
                .map(|m| json!(schema_name(&ToleranceModifier::ALL, *m)))
                .collect(),
        )
        .opt(
            "unit_basis",
            t.unit_basis.as_ref().map(|u| {
                Out::new()
                    .put("size", value_json(&u.size))
                    .opt(
                        "area",
                        u.area.as_ref().map(|a| {
                            Out::new()
                                .put("shape", json!(schema_name(&AREA_SHAPES, a.shape)))
                                .opt("second", a.second.as_ref().map(value_json))
                                .done()
                        }),
                    )
                    .done()
            }),
        )
        .opt("maximum", t.maximum.as_ref().map(value_json))
        .opt("unequal", t.unequal.as_ref().map(value_json))
        .opt("datums", t.datums.as_ref().map(datum_system_json))
        .list(
            "auxiliary",
            t.auxiliary
                .iter()
                .map(|a| json!(schema_name(&AUXILIARY, *a)))
                .collect(),
        )
        .opt("description", t.description.as_ref().map(|d| json!(d)))
        .done()
}

fn tolerance_from(v: &Json, path: &str) -> R<GeometricTolerance> {
    let keys = [
        "kind",
        "target",
        "magnitude",
        "zone",
        "modifiers",
        "unit_basis",
        "maximum",
        "unequal",
        "datums",
        "auxiliary",
        "description",
    ];
    let o = object(v, path, &keys)?;
    Ok(GeometricTolerance {
        kind: o.req_with("kind", |x, p| term(&TOLERANCE_KINDS, x, p))?,
        target: o.req_with("target", target_from)?,
        magnitude: o.opt_with("magnitude", length_value_from)?,
        zone: o.opt_with("zone", zone_from)?,
        modifiers: o.list("modifiers", |x, p| {
            schema_enum(&ToleranceModifier::ALL, x, p)
        })?,
        unit_basis: o.opt_with("unit_basis", |x, p| {
            let o = object(x, p, &["size", "area"])?;
            Ok(UnitBasis {
                size: o.req_with("size", value_from)?,
                area: o.opt_with("area", |x, p| {
                    let o = object(x, p, &["shape", "second"])?;
                    Ok(UnitArea {
                        shape: o.req_with("shape", |x, p| schema_enum(&AREA_SHAPES, x, p))?,
                        second: o.opt_with("second", value_from)?,
                    })
                })?,
            })
        })?,
        maximum: o.opt_with("maximum", length_value_from)?,
        unequal: o.opt_with("unequal", length_value_from)?,
        datums: o.opt_with("datums", datum_system_from)?,
        auxiliary: o.list("auxiliary", |x, p| schema_enum(&AUXILIARY, x, p))?,
        description: o.opt_with("description", string)?,
    })
}

fn relation_json(r: &ToleranceRelation) -> Json {
    json!({
        "kind": name_of(&RELATION_KINDS, r.kind),
        "relating": r.relating.0,
        "related": r.related.0,
    })
}

fn relation_from(v: &Json, path: &str) -> R<ToleranceRelation> {
    let o = object(v, path, &["kind", "relating", "related"])?;
    Ok(ToleranceRelation {
        kind: o.req_with("kind", |x, p| term(&RELATION_KINDS, x, p))?,
        relating: o.req_with("relating", |x, p| uint(x, p).map(ToleranceId))?,
        related: o.req_with("related", |x, p| uint(x, p).map(ToleranceId))?,
    })
}

// ---------------------------------------------------------------------------------------------
// Part-level information
// ---------------------------------------------------------------------------------------------

fn standard_json(s: &Standard) -> Json {
    Out::new()
        .put("document", json!(s.document))
        .opt("edition", s.edition.as_ref().map(|e| json!(e)))
        .put("body", json!(name_of(&STANDARD_BODIES, s.body)))
        .done()
}

fn standard_from(v: &Json, path: &str) -> R<Standard> {
    let o = object(v, path, &["document", "edition", "body"])?;
    Ok(Standard {
        document: o.req_with("document", string)?,
        edition: o.opt_with("edition", string)?,
        body: o.req_with("body", |x, p| term(&STANDARD_BODIES, x, p))?,
    })
}

fn iso2768_json(s: &Iso2768) -> Json {
    Out::new()
        .put("linear", json!(name_of(&LINEAR_CLASSES, s.linear)))
        .opt(
            "geometric",
            s.geometric.map(|g| json!(name_of(&GEOMETRIC_CLASSES, g))),
        )
        .done()
}

fn general_json(g: &GeneralTolerance) -> Json {
    match g {
        GeneralTolerance::Class { text, standard } => one(
            "class",
            Out::new()
                .put("text", json!(text))
                .opt("standard", standard.as_ref().map(iso2768_json))
                .done(),
        ),
        GeneralTolerance::Table { name, cells } => one(
            "table",
            json!({
                "name": name,
                "cells": cells.iter().map(|c| json!({
                    "items": c.items.iter().map(|(n, v)| {
                        let (k, x) = match v {
                            CellValue::Value(v) => ("measure", value_json(v)),
                            CellValue::Count(d) => ("count", json!(d.as_str())),
                            CellValue::Text(s) => ("text", json!(s)),
                        };
                        Out::new().put("name", json!(n)).put(k, x).done()
                    }).collect::<Vec<_>>()
                })).collect::<Vec<_>>(),
            }),
        ),
    }
}

fn general_from(v: &Json, path: &str) -> R<GeneralTolerance> {
    let (tag, x, p) = tagged(v, path, &["class", "table"])?;
    if tag == "class" {
        let o = object(x, &p, &["text", "standard"])?;
        let text = o.req_with("text", string)?;
        let recognised = Iso2768::recognise(&text);
        // The standard is recognised from the text (decision 3); a stated one must agree.
        if let Some(stated) = o.opt_with("standard", |x, p| {
            let o = object(x, p, &["linear", "geometric"])?;
            Ok(Iso2768 {
                linear: o.req_with("linear", |x, p| term(&LINEAR_CLASSES, x, p))?,
                geometric: o.opt_with("geometric", |x, p| term(&GEOMETRIC_CLASSES, x, p))?,
            })
        })? && Some(stated) != recognised
        {
            return fail(
                &o.path("standard"),
                format!("does not agree with the classes the text {text:?} states"),
            );
        }
        return Ok(GeneralTolerance::Class {
            text,
            standard: recognised,
        });
    }
    let o = object(x, &p, &["name", "cells"])?;
    Ok(GeneralTolerance::Table {
        name: o.req_with("name", string)?,
        cells: o.list("cells", |x, p| {
            let c = object(x, p, &["items"])?;
            Ok(ToleranceCell {
                items: c.list("items", |x, p| {
                    let i = object(x, p, &["name", "measure", "count", "text"])?;
                    let name = i.req_with("name", string)?;
                    let value = match (i.opt("measure"), i.opt("count"), i.opt("text")) {
                        (Some(_), None, None) => {
                            CellValue::Value(i.req_with("measure", value_from)?)
                        }
                        (None, Some(_), None) => CellValue::Count(i.req_with("count", decimal)?),
                        (None, None, Some(_)) => CellValue::Text(i.req_with("text", string)?),
                        _ => {
                            return fail(
                                p,
                                "a cell item states exactly one of measure, count, text",
                            );
                        }
                    };
                    Ok((name, value))
                })?,
            })
        })?,
    })
}

fn thread_json(t: &Thread) -> Json {
    Out::new()
        .put("feature", json!(t.feature.0))
        .opt("partial_area", t.partial_area.map(|f| json!(f.0)))
        .put("side", json!(name_of(&THREAD_SIDES, t.side)))
        .put("major_diameter", length_json(&t.major_diameter))
        .opt("minor_diameter", t.minor_diameter.as_ref().map(length_json))
        .opt("pitch_diameter", t.pitch_diameter.as_ref().map(length_json))
        .put("number_of_threads", json!(t.number_of_threads.0.as_str()))
        .put("form", json!(t.form))
        .put("fit_class", json!(t.fit_class))
        .opt("fit_class_2", t.fit_class_2.as_ref().map(|s| json!(s)))
        .put("hand", json!(name_of(&HANDS, t.hand)))
        .opt("crest", t.crest.as_ref().map(length_json))
        .opt("qualifier", t.qualifier.as_ref().map(|s| json!(s)))
        .opt("nominal_size", t.nominal_size.as_ref().map(length_json))
        .opt("runout", t.runout.map(|f| json!(f.0)))
        .done()
}

fn thread_from(v: &Json, path: &str) -> R<Thread> {
    let keys = [
        "feature",
        "partial_area",
        "side",
        "major_diameter",
        "minor_diameter",
        "pitch_diameter",
        "number_of_threads",
        "form",
        "fit_class",
        "fit_class_2",
        "hand",
        "crest",
        "qualifier",
        "nominal_size",
        "runout",
    ];
    let o = object(v, path, &keys)?;
    Ok(Thread {
        feature: o.req_with("feature", feature_id)?,
        partial_area: o.opt_with("partial_area", feature_id)?,
        side: o.req_with("side", |x, p| term(&THREAD_SIDES, x, p))?,
        major_diameter: o.req_with("major_diameter", length_from)?,
        minor_diameter: o.opt_with("minor_diameter", length_from)?,
        pitch_diameter: o.opt_with("pitch_diameter", length_from)?,
        number_of_threads: Ratio(o.req_with("number_of_threads", decimal)?),
        form: o.req_with("form", string)?,
        fit_class: o.req_with("fit_class", string)?,
        fit_class_2: o.opt_with("fit_class_2", string)?,
        hand: o.req_with("hand", |x, p| term(&HANDS, x, p))?,
        crest: o.opt_with("crest", length_from)?,
        qualifier: o.opt_with("qualifier", string)?,
        nominal_size: o.opt_with("nominal_size", length_from)?,
        runout: o.opt_with("runout", feature_id)?,
    })
}

fn knurl_json(k: &Knurl) -> Json {
    Out::new()
        .put("feature", json!(k.feature.0))
        .put("pattern", json!(name_of(&KNURL_PATTERNS, k.pattern)))
        .put("major_diameter", length_json(&k.major_diameter))
        .put("nominal_diameter", length_json(&k.nominal_diameter))
        .put("diametral_pitch", length_json(&k.diametral_pitch))
        .opt("number_of_teeth", k.number_of_teeth.map(|c| json!(c.0)))
        .opt("tooth_depth", k.tooth_depth.as_ref().map(length_json))
        .opt("root_fillet", k.root_fillet.as_ref().map(length_json))
        .opt("helix_angle", k.helix_angle.as_ref().map(angle_json))
        .opt(
            "helix_hand",
            k.helix_hand.map(|h| json!(name_of(&HANDS, h))),
        )
        .done()
}

fn knurl_from(v: &Json, path: &str) -> R<Knurl> {
    let keys = [
        "feature",
        "pattern",
        "major_diameter",
        "nominal_diameter",
        "diametral_pitch",
        "number_of_teeth",
        "tooth_depth",
        "root_fillet",
        "helix_angle",
        "helix_hand",
    ];
    let o = object(v, path, &keys)?;
    Ok(Knurl {
        feature: o.req_with("feature", feature_id)?,
        pattern: o.req_with("pattern", |x, p| term(&KNURL_PATTERNS, x, p))?,
        major_diameter: o.req_with("major_diameter", length_from)?,
        nominal_diameter: o.req_with("nominal_diameter", length_from)?,
        diametral_pitch: o.req_with("diametral_pitch", length_from)?,
        number_of_teeth: o.opt_with("number_of_teeth", |x, p| uint(x, p).map(Count))?,
        tooth_depth: o.opt_with("tooth_depth", length_from)?,
        root_fillet: o.opt_with("root_fillet", length_from)?,
        helix_angle: o.opt_with("helix_angle", angle_from)?,
        helix_hand: o.opt_with("helix_hand", |x, p| term(&HANDS, x, p))?,
    })
}

fn hole_json(h: &Hole) -> Json {
    let d = &h.definition;
    let definition = Out::new()
        .put("name", json!(d.name))
        .put("diameter", length_json(&d.diameter))
        .opt(
            "diameter_tolerance",
            d.diameter_tolerance.as_ref().map(bounds_json),
        )
        .opt("depth", d.depth.as_ref().map(length_json))
        .opt(
            "depth_tolerance",
            d.depth_tolerance.as_ref().map(bounds_json),
        )
        .put("through", json!(d.through))
        .put("placement", placement_json(&d.placement))
        .done();
    Out::new()
        .put("feature", json!(h.feature.0))
        .put("definition", definition)
        .opt("placement", h.placement.as_ref().map(placement_json))
        .done()
}

fn hole_from(v: &Json, path: &str) -> R<Hole> {
    let o = object(v, path, &["feature", "definition", "placement"])?;
    let definition = o.req_with("definition", |x, p| {
        let keys = [
            "name",
            "diameter",
            "diameter_tolerance",
            "depth",
            "depth_tolerance",
            "through",
            "placement",
        ];
        let d = object(x, p, &keys)?;
        Ok(RoundHole {
            name: d.req_with("name", string)?,
            diameter: d.req_with("diameter", length_from)?,
            diameter_tolerance: d.opt_with("diameter_tolerance", bounds_from)?,
            depth: d.opt_with("depth", length_from)?,
            depth_tolerance: d.opt_with("depth_tolerance", bounds_from)?,
            through: d.req_with("through", boolean)?,
            placement: d.req_with("placement", placement_from)?,
        })
    })?;
    Ok(Hole {
        feature: o.req_with("feature", feature_id)?,
        definition,
        placement: o.opt_with("placement", placement_from)?,
    })
}

fn material_json(m: &Material) -> Json {
    Out::new()
        .put("id", json!(m.id))
        .opt("name", m.name.as_ref().map(|n| json!(n)))
        .opt(
            "density",
            m.density.as_ref().map(|d| {
                json!({
                    "value": d.value.as_str(),
                    "unit": d.unit.iter().map(|(u, e)| json!({"unit": u, "exponent": e.as_str()})).collect::<Vec<_>>(),
                })
            }),
        )
        .done()
}

fn material_from(v: &Json, path: &str) -> R<Material> {
    let o = object(v, path, &["id", "name", "density"])?;
    Ok(Material {
        id: o.req_with("id", string)?,
        name: o.opt_with("name", string)?,
        density: o.opt_with("density", |x, p| {
            let o = object(x, p, &["value", "unit"])?;
            Ok(Density {
                value: o.req_with("value", decimal)?,
                unit: o.list("unit", |x, p| {
                    let u = object(x, p, &["unit", "exponent"])?;
                    Ok((
                        u.req_with("unit", string)?,
                        u.req_with("exponent", decimal)?,
                    ))
                })?,
            })
        })?,
    })
}

fn owner_json(o: &NoteOwner) -> Json {
    match o {
        NoteOwner::Feature(f) => one("feature", json!(f.0)),
        NoteOwner::Dimension(d) => one("dimension", json!(d.0)),
        NoteOwner::Tolerance(t) => one("tolerance", json!(t.0)),
        NoteOwner::DatumTarget(t) => one("datum_target", json!(t.0)),
    }
}

fn owner_from(v: &Json, path: &str) -> R<NoteOwner> {
    let (tag, x, p) = tagged(
        v,
        path,
        &["feature", "dimension", "tolerance", "datum_target"],
    )?;
    let n: usize = uint(x, &p)?;
    Ok(match tag {
        "feature" => NoteOwner::Feature(FeatureId(n)),
        "dimension" => NoteOwner::Dimension(DimensionId(n)),
        "tolerance" => NoteOwner::Tolerance(ToleranceId(n)),
        _ => NoteOwner::DatumTarget(DatumTargetId(n)),
    })
}

fn note_json(n: &Note) -> Json {
    Out::new()
        .put("kind", json!(n.kind))
        .put("text", json!(n.text))
        .opt("on", n.on.as_ref().map(owner_json))
        .list(
            "callouts",
            n.callouts
                .iter()
                .map(|c| {
                    Out::new()
                        .put("name", json!(c.name))
                        .list("features", c.features.iter().map(|f| json!(f.0)).collect())
                        .done()
                })
                .collect(),
        )
        .done()
}

fn note_from(v: &Json, path: &str) -> R<Note> {
    let o = object(v, path, &["kind", "text", "on", "callouts"])?;
    Ok(Note {
        kind: o.req_with("kind", string)?,
        text: o.req_with("text", string)?,
        on: o.opt_with("on", owner_from)?,
        callouts: o.list("callouts", |x, p| {
            let c = object(x, p, &["name", "features"])?;
            Ok(Callout {
                name: c.req_with("name", string)?,
                features: c.list("features", feature_id)?,
            })
        })?,
    })
}

fn attribute_value_json(v: &AttributeValue) -> (&'static str, Json) {
    match v {
        AttributeValue::Text(s) => ("text", json!(s)),
        AttributeValue::Integer(i) => ("integer", json!(i)),
        AttributeValue::Real(d) => ("real", json!(d.as_str())),
        AttributeValue::Boolean(b) => ("boolean", json!(b)),
        AttributeValue::Measure(v) => ("measure", value_json(v)),
        AttributeValue::OtherMeasure {
            measure,
            value,
            unit,
        } => (
            "other_measure",
            json!({"measure": measure, "value": value.as_str(), "unit": unit}),
        ),
    }
}

fn attributes_json(a: &AttributeSet) -> Json {
    Out::new()
        .put("name", json!(a.name))
        .opt("on", a.on.as_ref().map(owner_json))
        .list(
            "items",
            a.items
                .iter()
                .map(|(n, v)| {
                    let (k, x) = attribute_value_json(v);
                    Out::new().put("name", json!(n)).put(k, x).done()
                })
                .collect(),
        )
        .opt("description", a.description.as_ref().map(|d| json!(d)))
        .opt(
            "general_property",
            a.general_property.as_ref().map(|g| {
                Out::new()
                    .put("id", json!(g.id))
                    .put("name", json!(g.name))
                    .opt("description", g.description.as_ref().map(|d| json!(d)))
                    .done()
            }),
        )
        .done()
}

fn attributes_from(v: &Json, path: &str) -> R<AttributeSet> {
    let o = object(
        v,
        path,
        &["name", "on", "items", "description", "general_property"],
    )?;
    let kinds = [
        "text",
        "integer",
        "real",
        "boolean",
        "measure",
        "other_measure",
    ];
    Ok(AttributeSet {
        name: o.req_with("name", string)?,
        on: o.opt_with("on", owner_from)?,
        items: o.list("items", |x, p| {
            let mut keys = vec!["name"];
            keys.extend(kinds);
            let i = object(x, p, &keys)?;
            let name = i.req_with("name", string)?;
            let stated: Vec<&str> = kinds
                .iter()
                .copied()
                .filter(|k| i.opt(k).is_some())
                .collect();
            let [kind] = stated[..] else {
                return fail(
                    p,
                    format!(
                        "an attribute item states exactly one of {}",
                        kinds.join(", ")
                    ),
                );
            };
            let value = match kind {
                "text" => AttributeValue::Text(i.req_with(kind, string)?),
                "integer" => AttributeValue::Integer(i.req_with(kind, int)?),
                "real" => AttributeValue::Real(i.req_with(kind, decimal)?),
                "boolean" => AttributeValue::Boolean(i.req_with(kind, boolean)?),
                "measure" => AttributeValue::Measure(i.req_with(kind, value_from)?),
                _ => i.req_with(kind, |x, p| {
                    let m = object(x, p, &["measure", "value", "unit"])?;
                    Ok(AttributeValue::OtherMeasure {
                        measure: m.req_with("measure", string)?,
                        value: m.req_with("value", decimal)?,
                        unit: m.req_with("unit", string)?,
                    })
                })?,
            };
            Ok((name, value))
        })?,
        description: o.opt_with("description", string)?,
        general_property: o.opt_with("general_property", |x, p| {
            let g = object(x, p, &["id", "name", "description"])?;
            Ok(GeneralProperty {
                id: g.req_with("id", string)?,
                name: g.req_with("name", string)?,
                description: g.opt_with("description", string)?,
            })
        })?,
    })
}

fn surface_texture_json(s: &SurfaceTexture) -> Json {
    Out::new()
        .opt("on", s.on.map(|f| json!(f.0)))
        .put(
            "material_removal",
            json!(name_of(&MATERIAL_REMOVALS, s.material_removal)),
        )
        .list(
            "parameters",
            s.parameters
                .iter()
                .map(
                    |p| json!({"characteristic": p.characteristic, "value": length_json(&p.value)}),
                )
                .collect(),
        )
        .done()
}

fn surface_texture_from(v: &Json, path: &str) -> R<SurfaceTexture> {
    let o = object(v, path, &["on", "material_removal", "parameters"])?;
    Ok(SurfaceTexture {
        on: o.opt_with("on", |x, p| uint(x, p).map(FeatureId))?,
        material_removal: o.req_with("material_removal", |x, p| term(&MATERIAL_REMOVALS, x, p))?,
        parameters: o.list("parameters", |x, p| {
            let q = object(x, p, &["characteristic", "value"])?;
            Ok(SurfaceTextureParameter {
                characteristic: q.req_with("characteristic", string)?,
                value: q.req_with("value", length_from)?,
            })
        })?,
    })
}

// ---------------------------------------------------------------------------------------------
// Parts, findings, documents
// ---------------------------------------------------------------------------------------------

/// The JSON form of one part's PMI.
#[must_use]
pub fn to_json(p: &PartPmi) -> Json {
    Out::new()
        .list("standards", p.standards.iter().map(standard_json).collect())
        .opt("decimal_places", p.decimal_places.map(|d| json!(d)))
        .list("features", p.features.iter().map(feature_json).collect())
        .list(
            "datum_targets",
            p.datum_targets.iter().map(datum_target_json).collect(),
        )
        .list("datums", p.datums.iter().map(datum_json).collect())
        .list(
            "dimensions",
            p.dimensions.iter().map(dimension_json).collect(),
        )
        .list(
            "tolerances",
            p.tolerances.iter().map(tolerance_json).collect(),
        )
        .list(
            "tolerance_relations",
            p.tolerance_relations.iter().map(relation_json).collect(),
        )
        .list("general", p.general.iter().map(general_json).collect())
        .list("threads", p.threads.iter().map(thread_json).collect())
        .list("knurls", p.knurls.iter().map(knurl_json).collect())
        .list("holes", p.holes.iter().map(hole_json).collect())
        .opt("material", p.material.as_ref().map(material_json))
        .list("notes", p.notes.iter().map(note_json).collect())
        .list(
            "surface_textures",
            p.surface_textures
                .iter()
                .map(surface_texture_json)
                .collect(),
        )
        .list(
            "attributes",
            p.attributes.iter().map(attributes_json).collect(),
        )
        .list("geometry", p.geometry.iter().map(geometry_json).collect())
        .list(
            "derived_geometry",
            p.derived_geometry
                .iter()
                .map(derived_geometry_json)
                .collect(),
        )
        .done()
}

/// One part's PMI from its JSON form at `path`, refused (naming the JSON path) on an unknown
/// field or term, a malformed value, or a violated invariant (the model's constructors and
/// [`PartPmi::validate`]).
pub fn from_json(v: &Json, path: &str) -> Result<PartPmi, JsonError> {
    let keys = [
        "standards",
        "decimal_places",
        "features",
        "datum_targets",
        "datums",
        "dimensions",
        "tolerances",
        "tolerance_relations",
        "general",
        "threads",
        "knurls",
        "holes",
        "material",
        "notes",
        "surface_textures",
        "attributes",
        "geometry",
        "derived_geometry",
    ];
    let o = object(v, path, &keys)?;
    let pmi = PartPmi {
        standards: o.list("standards", standard_from)?,
        decimal_places: o.opt_with("decimal_places", uint::<u8>)?,
        features: o.list("features", feature_from)?,
        datum_targets: o.list("datum_targets", datum_target_from)?,
        datums: o.list("datums", datum_from)?,
        dimensions: o.list("dimensions", dimension_from)?,
        tolerances: o.list("tolerances", tolerance_from)?,
        tolerance_relations: o.list("tolerance_relations", relation_from)?,
        general: o.list("general", general_from)?,
        threads: o.list("threads", thread_from)?,
        knurls: o.list("knurls", knurl_from)?,
        holes: o.list("holes", hole_from)?,
        material: o.opt_with("material", material_from)?,
        notes: o.list("notes", note_from)?,
        surface_textures: o.list("surface_textures", surface_texture_from)?,
        attributes: o.list("attributes", attributes_from)?,
        geometry: o.list("geometry", geometry_from)?,
        derived_geometry: o.list("derived_geometry", derived_geometry_from)?,
    };
    if let Err(errors) = pmi.validate() {
        let why: Vec<String> = errors.into_iter().map(|e| e.0).collect();
        return fail(path, why.join("; "));
    }
    Ok(pmi)
}

/// The JSON form of a finding (it cites instance ids as file evidence).
#[must_use]
pub fn finding_json(f: &Finding) -> Json {
    Out::new()
        .put("kind", json!(f.kind.as_str()))
        .opt("part", f.part.map(|p| json!(p.0)))
        .put("ids", json!(f.ids))
        .put("entity", json!(f.entity))
        .put("detail", json!(f.detail))
        .done()
}

fn finding_from(v: &Json, path: &str) -> R<Finding> {
    let o = object(v, path, &["kind", "part", "ids", "entity", "detail"])?;
    let kind = o.req_with("kind", |x, p| {
        let s = string(x, p)?;
        match FINDING_KINDS.iter().find(|k| k.as_str() == s) {
            Some(&k) => Ok(k),
            None => fail(p, format!("unknown finding kind {s:?}")),
        }
    })?;
    Ok(Finding {
        kind,
        part: o.opt_with("part", |x, p| uint(x, p).map(PartId))?,
        ids: o.list("ids", uint::<u64>)?,
        entity: o.req_with("entity", string)?,
        detail: o.req_with("detail", string)?,
    })
}

/// The JSON form of a document.
#[must_use]
pub fn document_to_json(d: &Document) -> Json {
    json!({
        "format": FORMAT,
        "version": VERSION,
        "binding": {"sha256": d.binding.sha256, "reader": d.binding.reader},
        "parts": d.parts.iter().map(|p| {
            Out::new()
                .put("part", json!(p.part.0))
                .opt("name", p.name.as_ref().map(|n| json!(n)))
                .put("pmi", to_json(&p.pmi))
                .done()
        }).collect::<Vec<_>>(),
        "findings": d.findings.iter().map(finding_json).collect::<Vec<_>>(),
    })
}

/// JSON text as a value, refusing an object that names a key twice (`serde_json` keeps the last
/// occurrence, which would accept a conflicting document without a word).
pub fn parse(text: &str) -> Result<Json, serde_json::Error> {
    use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

    struct Strict(Json);
    struct V;

    impl<'de> Visitor<'de> for V {
        type Value = Strict;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a JSON value")
        }
        fn visit_bool<E>(self, b: bool) -> Result<Strict, E> {
            Ok(Strict(Json::Bool(b)))
        }
        fn visit_i64<E>(self, n: i64) -> Result<Strict, E> {
            Ok(Strict(Json::from(n)))
        }
        fn visit_u64<E>(self, n: u64) -> Result<Strict, E> {
            Ok(Strict(Json::from(n)))
        }
        fn visit_f64<E>(self, n: f64) -> Result<Strict, E> {
            Ok(Strict(
                serde_json::Number::from_f64(n).map_or(Json::Null, Json::Number),
            ))
        }
        fn visit_str<E>(self, s: &str) -> Result<Strict, E> {
            Ok(Strict(Json::String(s.to_string())))
        }
        fn visit_string<E>(self, s: String) -> Result<Strict, E> {
            Ok(Strict(Json::String(s)))
        }
        fn visit_unit<E>(self) -> Result<Strict, E> {
            Ok(Strict(Json::Null))
        }
        fn visit_none<E>(self) -> Result<Strict, E> {
            Ok(Strict(Json::Null))
        }
        fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Strict, D::Error> {
            Strict::deserialize(d)
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Strict, A::Error> {
            let mut out = Vec::new();
            while let Some(Strict(x)) = seq.next_element()? {
                out.push(x);
            }
            Ok(Strict(Json::Array(out)))
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Strict, A::Error> {
            let mut out = Map::new();
            while let Some(k) = map.next_key::<String>()? {
                if out.contains_key(&k) {
                    return Err(de::Error::custom(format!("duplicate key {k:?}")));
                }
                let Strict(x) = map.next_value()?;
                out.insert(k, x);
            }
            Ok(Strict(Json::Object(out)))
        }
    }

    impl<'de> Deserialize<'de> for Strict {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Strict, D::Error> {
            d.deserialize_any(V)
        }
    }

    serde_json::from_str::<Strict>(text).map(|Strict(v)| v)
}

/// A document from its JSON form, refused on a different format or version, an unknown field
/// or term, a malformed value, a violated invariant, or a part listed twice.
pub fn document_from_json(v: &Json) -> Result<Document, JsonError> {
    let o = object(
        v,
        "",
        &["format", "version", "binding", "parts", "findings"],
    )?;
    let format = o.req_with("format", string)?;
    if format != FORMAT {
        return fail("format", format!("{format:?} is not {FORMAT:?}"));
    }
    let version: u64 = o.req_with("version", uint)?;
    if version != VERSION {
        return fail(
            "version",
            format!("version {version} is not supported (this is version {VERSION})"),
        );
    }
    let binding = o.req_with("binding", |x, p| {
        let b = object(x, p, &["sha256", "reader"])?;
        Ok(Binding {
            sha256: b.req_with("sha256", string)?,
            reader: b.req_with("reader", string)?,
        })
    })?;
    let parts = o.list("parts", |x, p| {
        let q = object(x, p, &["part", "name", "pmi"])?;
        Ok(DocumentPart {
            part: q.req_with("part", |x, p| uint(x, p).map(PartId))?,
            name: q.opt_with("name", string)?,
            pmi: q.req_with("pmi", from_json)?,
        })
    })?;
    for (i, p) in parts.iter().enumerate() {
        if parts[..i].iter().any(|q| q.part == p.part) {
            return fail(
                &format!("parts[{i}].part"),
                format!("part {} is listed twice", p.part.0),
            );
        }
    }
    Ok(Document {
        binding,
        parts,
        findings: o.list("findings", finding_from)?,
    })
}

/// Checks a document against the file it claims to be bound to: the binding (sha256 and reader
/// version), every part index, a stated part name, and every face and edge anchor against the
/// part's faces and edges.
pub fn check_against(d: &Document, step: &[u8], parts: &[PartDefinition]) -> Result<(), JsonError> {
    let here = Binding::of(step);
    if d.binding.sha256 != here.sha256 {
        return fail(
            "binding.sha256",
            format!(
                "{} is not the file's sha256 {}: the anchors are numbered for another file",
                d.binding.sha256, here.sha256
            ),
        );
    }
    if d.binding.reader != here.reader {
        return fail(
            "binding.reader",
            format!(
                "{:?} is not this reader ({:?})",
                d.binding.reader, here.reader
            ),
        );
    }
    for (i, p) in d.parts.iter().enumerate() {
        let path = format!("parts[{i}]");
        let Some(def) = parts.get(p.part.0) else {
            return fail(
                &field(&path, "part"),
                format!(
                    "the file has {} parts; there is no part {}",
                    parts.len(),
                    p.part.0
                ),
            );
        };
        if let Some(name) = &p.name
            && *name != def.name
        {
            return fail(
                &field(&path, "name"),
                format!("{name:?} is not part {}'s name {:?}", p.part.0, def.name),
            );
        }
        for (gi, g) in p.pmi.derived_geometry.iter().enumerate() {
            for (fi, f) in g.deriving_faces.iter().enumerate() {
                if f.0 >= def.faces.len() {
                    return fail(
                        &format!("{path}.pmi.derived_geometry[{gi}].deriving_faces[{fi}]"),
                        format!(
                            "part {} has {} faces; there is no face {}",
                            p.part.0,
                            def.faces.len(),
                            f.0
                        ),
                    );
                }
            }
        }
        for (fi, f) in p.pmi.features.iter().enumerate() {
            let Feature::Items(items) = f else { continue };
            for (ai, a) in items.iter().enumerate() {
                let at = format!("{path}.pmi.features[{fi}].items[{ai}]");
                match a {
                    Anchor::Face(n) if n.0 >= def.faces.len() => {
                        return fail(
                            &format!("{at}.face"),
                            format!(
                                "part {} has {} faces (0 to {}); there is no face {}",
                                p.part.0,
                                def.faces.len(),
                                def.faces.len().saturating_sub(1),
                                n.0
                            ),
                        );
                    }
                    Anchor::Edge(n) if n.0 >= def.edges.len() => {
                        return fail(
                            &format!("{at}.edge"),
                            format!(
                                "part {} has {} edges; there is no edge {}",
                                p.part.0,
                                def.edges.len(),
                                n.0
                            ),
                        );
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

/// The document of a file's PMI as read: every part (or only `only`), with the findings that
/// concern those parts or no part.
#[must_use]
pub fn read_document(
    read: &PmiRead,
    parts: &[PartDefinition],
    binding: Binding,
    only: Option<usize>,
) -> Document {
    let wanted = |i: usize| only.is_none_or(|n| n == i);
    Document {
        binding,
        parts: read
            .parts
            .iter()
            .enumerate()
            .filter(|(i, _)| wanted(*i))
            .map(|(i, pmi)| DocumentPart {
                part: PartId(i),
                name: parts.get(i).map(|d| d.name.clone()),
                pmi: pmi.clone(),
            })
            .collect(),
        findings: read
            .findings
            .iter()
            .filter(|f| f.part.is_none_or(|p| wanted(p.0)))
            .cloned()
            .collect(),
    }
}

/// The parts listing: per distinct part its index, name, face and edge counts and placements
/// (rows of `[R | t]`, `t` in millimetres), and the binding.
#[must_use]
pub fn parts_json(parts: &[PartDefinition], binding: &Binding) -> Json {
    json!({
        "format": PARTS_FORMAT,
        "version": VERSION,
        "binding": {"sha256": binding.sha256, "reader": binding.reader},
        "parts": parts.iter().enumerate().map(|(i, p)| json!({
            "part": i,
            "name": p.name,
            "faces": p.faces.len(),
            "edges": p.edges.len(),
            "placements": p.placements.iter().map(|q| json!(q.placement)).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// The format name of a write's report (`quiddity pmi write`).
pub const WRITE_FORMAT: &str = "quiddity-pmi-write";

/// The report of a verified write: the mode and presentation policy, the parts written, the
/// input's and the output's bindings, instance counts (added; replaced, i.e. kept presentation
/// containers rewritten without removed members; removed), the presentation removed by id and
/// type, the `FILE_SCHEMA` kept or changed, the datums of an add that resolved to the part's
/// own, the datum feature symbols written, the edition table the written instances were checked
/// against, the original's schema and rule violations (reported, not repaired), the reader's
/// findings about PMI of a replaced part that stays, and the findings of reading the output.
#[must_use]
pub fn write_report_json(
    mode: write::Mode,
    policy: write::PresentationPolicy,
    parts: &[PartId],
    input: &Binding,
    output: &Binding,
    report: &write::WriteReport,
    findings: &[Finding],
) -> Json {
    let mode = match mode {
        write::Mode::Add => "add",
        write::Mode::Replace => "replace",
        write::Mode::Remove => "remove",
    };
    let presentation = match policy {
        write::PresentationPolicy::Refuse => "refuse",
        write::PresentationPolicy::RemovePresentation => "remove",
    };
    let schema = match &report.schema {
        write::SchemaChange::Kept(s) => json!({"kept": s}),
        write::SchemaChange::Upgraded { from, to } => json!({"from": from, "to": to}),
    };
    let typed = |v: &[(u64, String)]| {
        v.iter()
            .map(|(id, entity)| json!({"id": id, "entity": entity}))
            .collect::<Vec<_>>()
    };
    Out::new()
        .put("format", json!(WRITE_FORMAT))
        .put("version", json!(VERSION))
        .put("mode", json!(mode))
        .put("presentation", json!(presentation))
        .put(
            "parts",
            json!(parts.iter().map(|p| p.0).collect::<Vec<_>>()),
        )
        .put(
            "input",
            json!({"sha256": input.sha256, "reader": input.reader}),
        )
        .put(
            "binding",
            json!({"sha256": output.sha256, "reader": output.reader}),
        )
        .put(
            "instances",
            json!({
                "added": report.added,
                "replaced": report.rewritten.len() + report.repairs.len() + report.edition_rewrites.len(),
                "removed": report.removed.len(),
            }),
        )
        .put(
            "presentation_removed",
            json!(typed(&report.presentation_removed)),
        )
        .opt("long_strings", (!report.long_strings.is_empty()).then(|| json!(
            report.long_strings.iter().map(|w| json!({"record": w.record, "encoded_bytes": w.encoded_bytes, "byte_offset": w.byte_offset, "message": w.to_string()})).collect::<Vec<_>>()
        )))
        .put("left_unreferenced", json!(typed(&report.left_unreferenced)))
        .put("file_schema", schema)
        .put(
            "repairs",
            json!(
                report
                    .repairs
                    .iter()
                    .map(|r| json!({
                        "id": r.id,
                        "repair": match r.repair {
                            write::UpgradeRepairKind::R1ApprovalAssignment => "R1",
                            write::UpgradeRepairKind::R2ConversionUnitDimensions => "R2",
                        },
                        "original_record": r.original_record,
                        "new_record": r.new_record,
                    }))
                    .collect::<Vec<_>>()
            ),
        )
        .opt(
            "edition_rewrites",
            (!report.edition_rewrites.is_empty()).then(|| json!(report.edition_rewrites.iter()
                .map(|r| json!({"id": r.id, "from": r.from, "to": r.to}))
                .collect::<Vec<_>>())),
        )
        .put("validated_against", json!(report.validated_against))
        .opt(
            "edition_undetermined",
            report.edition_undetermined.as_ref().map(|s| json!(s)),
        )
        .put(
            "reused_datums",
            json!(
                report
                    .reused_datums
                    .iter()
                    .map(|(p, label, id)| json!({"part": p.0, "label": label, "id": id}))
                    .collect::<Vec<_>>()
            ),
        )
        .put(
            "note_presentation_removed",
            json!(
                report
                    .note_presentation_removed
                    .iter()
                    .map(|(p, n, id, kind)| json!({"part":p.0,"note":n,"id":id,"entity":kind}))
                    .collect::<Vec<_>>()
            ),
        )
        .put(
            "note_migrations",
            json!(
                report
                    .note_migrations
                    .iter()
                    .map(|(p, i, form)| json!({"part": p.0, "note": i, "form": form}))
                    .collect::<Vec<_>>()
            ),
        )
        .put(
            "datum_symbols",
            json!(
                report
                    .datum_symbols
                    .iter()
                    .map(|(p, labels)| json!({"part": p.0, "labels": labels}))
                    .collect::<Vec<_>>()
            ),
        )
        .put(
            "preexisting_violations",
            json!(
                report
                    .preexisting_violations
                    .iter()
                    .map(|v| json!({"id": v.id, "entity": v.entity, "violation": v.to_string()}))
                    .collect::<Vec<_>>()
            ),
        )
        .opt("geometry_carrier_exemptions", (!report.geometry_carrier_exemptions.is_empty()).then(|| json!(report.geometry_carrier_exemptions.iter().map(|e| json!({"part": e.part.0, "condition": e.condition, "aspect": e.aspect, "feature": e.feature})).collect::<Vec<_>>())))
        .put("practice_schema_conflicts", json!(report.practice_schema_conflicts.iter().map(|v| json!({"id": v.id, "entity": v.entity, "rule": v.rule, "message": v.message})).collect::<Vec<_>>()))
        .put(
            "preexisting_rule_violations",
            json!(
                report
                    .preexisting_rule_violations
                    .iter()
                    .map(|v| json!({"id": v.id, "entity": v.entity, "rule": v.rule,
                                    "message": v.message}))
                    .collect::<Vec<_>>()
            ),
        )
        .put(
            "kept_unconsumed",
            json!(
                report
                    .kept_unconsumed
                    .iter()
                    .map(finding_json)
                    .collect::<Vec<_>>()
            ),
        )
        .put(
            "findings",
            json!(findings.iter().map(finding_json).collect::<Vec<_>>()),
        )
        .done()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mm(v: &str) -> Value {
        Value::length(Decimal::parse(v).unwrap(), LengthUnit::Millimetre)
    }

    #[test]
    fn values_keep_their_text_and_unit() {
        let v = Value {
            quantity: Quantity::Length(Length {
                value: Decimal::parse("0.0030").unwrap(),
                unit: LengthUnit::Inch,
            }),
            decimal_places: Some(4),
        };
        let j = value_json(&v);
        assert_eq!(
            j,
            json!({"value": "0.0030", "unit": "in", "decimal_places": 4})
        );
        assert_eq!(value_from(&j, "v").unwrap(), v);
        let other = Value::length(
            Decimal::parse("1").unwrap(),
            LengthUnit::Other {
                name: "furlong".into(),
                metres: Decimal::parse("201.168").unwrap(),
            },
        );
        assert_eq!(value_from(&value_json(&other), "v").unwrap(), other);
    }

    #[test]
    fn a_fit_is_a_deviation_and_a_grade() {
        let t = DimTolerance::Fit {
            class: Iso286Class {
                deviation: FundamentalDeviation::parse("g").unwrap(),
                grade: ToleranceGrade::It6,
            },
            limits: None,
        };
        let j = dim_tolerance_json(&t);
        assert_eq!(j, json!({"fit": {"deviation": "g", "grade": "IT6"}}));
        assert_eq!(dim_tolerance_from(&j, "t").unwrap(), t);
        let e = dim_tolerance_from(&json!({"fit": {"deviation": "g", "grade": "6"}}), "t");
        assert_eq!(e.unwrap_err().path, "t.fit.grade");
    }

    #[test]
    fn refusals_name_the_path() {
        let bad = json!({"deviations": {"upper": mm_json("-0.025"), "lower": mm_json("-0.009")}});
        let e = dim_tolerance_from(&bad, "d").unwrap_err();
        assert_eq!(e.path, "d.deviations");
        assert!(e.message.contains("not greater"), "{e}");
        let e = value_from(&json!({"value": 0.1, "unit": "mm"}), "v").unwrap_err();
        assert_eq!(e.path, "v.value");
        let e = value_from(&json!({"value": "0.1", "unit": "mm", "x": 1}), "v").unwrap_err();
        assert_eq!(e.path, "v.x");
        let e =
            tolerance_from(&json!({"kind": "runout", "target": "whole part"}), "t").unwrap_err();
        assert_eq!(e.path, "t.kind");
        let _ = mm("1");
    }

    fn mm_json(v: &str) -> Json {
        json!({"value": v, "unit": "mm"})
    }

    #[test]
    fn every_term_table_round_trips() {
        for &(_, k) in &TOLERANCE_KINDS {
            let j = json!(name_of(&TOLERANCE_KINDS, k));
            assert_eq!(term(&TOLERANCE_KINDS, &j, "k").unwrap(), k);
        }
        for &(_, m) in &ToleranceModifier::ALL {
            let j = json!(schema_name(&ToleranceModifier::ALL, m));
            assert_eq!(schema_enum(&ToleranceModifier::ALL, &j, "m").unwrap(), m);
        }
        for (_, m) in &DimensionModifier::TABLE {
            let j = json!(dimension_modifier_name(m));
            assert_eq!(&dimension_modifier_from(&j, "m").unwrap(), m);
        }
        assert_eq!(TOLERANCE_KINDS.len(), ToleranceKind::ALL.len());
        for k in FINDING_KINDS {
            let f = Finding {
                kind: k,
                part: None,
                ids: vec![1],
                entity: "x".into(),
                detail: String::new(),
            };
            assert_eq!(finding_from(&finding_json(&f), "f").unwrap(), f);
        }
    }

    fn d(t: &str) -> Decimal {
        Decimal::parse(t).unwrap()
    }

    fn len(t: &str, unit: LengthUnit) -> Length {
        Length { value: d(t), unit }
    }

    /// Every variant no file at hand carries (threads, knurls, tables, the rarer zone and
    /// modifier forms, densities, other units), and the others, round trip exactly.
    #[test]
    fn a_part_with_every_variant_round_trips() {
        let inch = || LengthUnit::Inch;
        let deg = AngleUnit::Degree;
        let other_len = LengthUnit::Other {
            name: "furlong".into(),
            metres: d("201.168"),
        };
        let other_angle = AngleUnit::Other {
            name: "grad".into(),
            radians: d("0.015707963267948967"),
        };
        let placement = Placement {
            origin: [len("1.", inch()), len("0.", inch()), len("-2.5", inch())],
            axis: Some(Direction([d("0."), d("0."), d("1.")])),
            ref_direction: None,
        };
        let target =
            |n, shape, placement| DatumTarget::new(n, shape, placement, None, None).unwrap();
        let item = GeometryItem {
            leaves: vec![(
                "CARTESIAN_POINT".into(),
                vec![
                    GeometryValue::Text("p".into()),
                    GeometryValue::List(vec![
                        GeometryValue::Real(d("1.5")),
                        GeometryValue::Integer(-3),
                    ]),
                    GeometryValue::Enum("T".into()),
                    GeometryValue::Binary("0F".into()),
                    GeometryValue::Item(Box::new(GeometryItem {
                        leaves: vec![("DIRECTION".into(), vec![GeometryValue::Unset])],
                    })),
                    GeometryValue::Typed {
                        type_name: "LENGTH_MEASURE".into(),
                        value: Box::new(GeometryValue::Real(d("2."))),
                    },
                    GeometryValue::Derived,
                ],
            )],
        };
        let p = PartPmi {
            standards: vec![Standard {
                document: "ISO 1101".into(),
                edition: Some("ISO 1101:2017".into()),
                body: StandardBody::Iso,
            }],
            decimal_places: Some(3),
            features: vec![
                Feature::Items(vec![Anchor::Face(FaceIndex(0)), Anchor::Edge(EdgeIndex(2))]),
                Feature::Items(vec![Anchor::Geometry(GeometryId(0))]),
                Feature::Group {
                    members: vec![FeatureId(0), FeatureId(1)],
                    kind: GroupKind::Between,
                },
                Feature::Derived {
                    kind: DerivedKind::CentreOfSymmetry,
                    geometry: None,
                    from: vec![FeatureId(0)],
                },
                Feature::Items(vec![Anchor::Face(FaceIndex(5))]),
            ],
            datum_targets: vec![
                target(1, TargetShape::Point, Some(placement.clone())),
                target(
                    2,
                    TargetShape::Line {
                        length: len("3", inch()),
                    },
                    Some(placement.clone()),
                ),
                target(
                    3,
                    TargetShape::Rectangle {
                        length: len("3", inch()),
                        width: len("1", inch()),
                    },
                    Some(placement.clone()),
                ),
                target(
                    4,
                    TargetShape::Circle {
                        diameter: len("0.5", inch()),
                    },
                    Some(placement.clone()),
                ),
                DatumTarget::new(
                    5,
                    TargetShape::CircularCurve {
                        diameter: len("0.5", inch()),
                    },
                    Some(placement.clone()),
                    Some(Direction([d("1."), d("0."), d("0.")])),
                    Some(FeatureId(0)),
                )
                .unwrap(),
                target(6, TargetShape::Area(FeatureId(4)), None),
                target(7, TargetShape::Curve(FeatureId(0)), None),
            ],
            datums: vec![
                Datum::new(DatumLabel::new("A").unwrap(), Some(FeatureId(0)), vec![]).unwrap(),
                Datum::new(
                    DatumLabel::new("B").unwrap(),
                    None,
                    (0..7).map(DatumTargetId).collect(),
                )
                .unwrap(),
            ],
            dimensions: vec![
                Dimension {
                    kind: DimensionKind::Size {
                        feature: FeatureId(4),
                        kind: SizeKind::Other("angle".into()),
                        path: Some(FeatureId(0)),
                        angle: Some(AngleSelection::Large),
                    },
                    nominal: Some(Value::angle(d("30"), deg.clone())),
                    tolerance: DimTolerance::Limits(
                        Bounds::new(
                            Value::angle(d("30.5"), deg.clone()),
                            Value::angle(d("29.5"), deg.clone()),
                        )
                        .unwrap(),
                    ),
                    qualifier: Some(Qualifier::Other("nominal".into())),
                    modifiers: vec![DimensionModifier::Reference, DimensionModifier::Square],
                    principle: Some(Principle::EnvelopeRequirement),
                },
                Dimension {
                    kind: DimensionKind::Location {
                        from: FeatureId(0),
                        to: FeatureId(4),
                        kind: LocationKind::LinearDistanceInnerOuter,
                        path: None,
                        directed: true,
                        angle: None,
                    },
                    nominal: Some(Value::length(d("20"), LengthUnit::Millimetre)),
                    tolerance: DimTolerance::Fit {
                        class: Iso286Class {
                            deviation: FundamentalDeviation::parse("JS").unwrap(),
                            grade: ToleranceGrade::It01,
                        },
                        limits: Some(Bounds::new(mm("20.021"), mm("20.000")).unwrap()),
                    },
                    qualifier: Some(Qualifier::Maximum),
                    modifiers: vec![],
                    principle: None,
                },
                Dimension {
                    kind: DimensionKind::Size {
                        feature: FeatureId(4),
                        kind: SizeKind::Thickness,
                        path: None,
                        angle: None,
                    },
                    nominal: None,
                    tolerance: DimTolerance::Basic,
                    qualifier: None,
                    modifiers: vec![],
                    principle: None,
                },
            ],
            tolerances: vec![
                GeometricTolerance {
                    kind: ToleranceKind::Position,
                    target: ToleranceTarget::Relation {
                        relating: FeatureId(0),
                        related: FeatureId(4),
                        name: "r".into(),
                    },
                    magnitude: Some(Value::length(d("1"), other_len.clone())),
                    zone: Some(Zone {
                        form: ZoneForm::Other(String::new()),
                        projected: Some(ProjectedZone {
                            end: None,
                            length: mm("10"),
                        }),
                        non_uniform: Some(vec![]),
                        runout_angle: Some(Value::angle(d("100"), other_angle)),
                        affected_plane: Some(FeatureId(0)),
                        affected_plane_kind: Some(AffectedPlaneKind::IntersectionPlane),
                    }),
                    modifiers: vec![ToleranceModifier::LeastMaterialRequirement],
                    unit_basis: Some(UnitBasis {
                        size: mm("25"),
                        area: Some(UnitArea {
                            shape: AreaShape::Rectangular,
                            second: Some(mm("50")),
                        }),
                    }),
                    maximum: Some(mm("0.5")),
                    unequal: Some(mm("-0.1")),
                    datums: Some(
                        DatumSystem::new(vec![Compartment {
                            references: vec![
                                DatumReference {
                                    datum: DatumId(0),
                                    modifiers: vec![DatumModifier::WithValue {
                                        kind: DatumModifierType::Projected,
                                        value: mm("5"),
                                    }],
                                },
                                DatumReference {
                                    datum: DatumId(1),
                                    modifiers: vec![],
                                },
                            ],
                            modifiers: vec![DatumModifier::Simple(
                                SimpleDatumModifier::DegreeOfFreedomConstraintW,
                            )],
                        }])
                        .unwrap(),
                    ),
                    auxiliary: vec![AuxiliaryClassification::UnlessOtherwiseSpecified],
                    description: Some("\u{d8} \"x\"".into()),
                },
                GeometricTolerance {
                    kind: ToleranceKind::Position,
                    target: ToleranceTarget::Relation {
                        relating: FeatureId(0),
                        related: FeatureId(4),
                        name: "r".into(),
                    },
                    magnitude: Some(mm("0.1")),
                    zone: None,
                    modifiers: vec![],
                    unit_basis: None,
                    maximum: None,
                    unequal: None,
                    datums: None,
                    auxiliary: vec![],
                    description: None,
                },
                GeometricTolerance {
                    kind: ToleranceKind::TotalRunout,
                    target: ToleranceTarget::Dimension(DimensionId(2)),
                    magnitude: None,
                    zone: None,
                    modifiers: vec![],
                    unit_basis: None,
                    maximum: None,
                    unequal: None,
                    datums: Some(
                        DatumSystem::new(vec![Compartment {
                            references: vec![DatumReference {
                                datum: DatumId(0),
                                modifiers: vec![],
                            }],
                            modifiers: vec![],
                        }])
                        .unwrap(),
                    ),
                    auxiliary: vec![],
                    description: None,
                },
            ],
            tolerance_relations: vec![ToleranceRelation {
                kind: RelationKind::Composite,
                relating: ToleranceId(0),
                related: ToleranceId(1),
            }],
            general: vec![
                GeneralTolerance::Class {
                    text: "per drawing".into(),
                    standard: None,
                },
                GeneralTolerance::Table {
                    name: "ISO 2768-1".into(),
                    cells: vec![ToleranceCell {
                        items: vec![
                            ("upper".into(), CellValue::Value(mm("0.1"))),
                            ("significant digits".into(), CellValue::Count(d("2"))),
                            ("class".into(), CellValue::Text("m".into())),
                        ],
                    }],
                },
            ],
            threads: vec![Thread {
                feature: FeatureId(4),
                partial_area: Some(FeatureId(0)),
                side: ThreadSide::Internal,
                major_diameter: len("6", LengthUnit::Millimetre),
                minor_diameter: Some(len("4.917", LengthUnit::Millimetre)),
                pitch_diameter: None,
                number_of_threads: Ratio(d("1.")),
                form: "M".into(),
                fit_class: "6H".into(),
                fit_class_2: Some("6g".into()),
                hand: Hand::Left,
                crest: None,
                qualifier: Some("coarse".into()),
                nominal_size: Some(len("6", LengthUnit::Millimetre)),
                runout: Some(FeatureId(0)),
            }],
            knurls: vec![Knurl {
                feature: FeatureId(4),
                pattern: KnurlPattern::Diagonal,
                major_diameter: len("20", LengthUnit::Millimetre),
                nominal_diameter: len("19.8", LengthUnit::Millimetre),
                diametral_pitch: len("0.8", LengthUnit::Millimetre),
                number_of_teeth: Some(Count(78)),
                tooth_depth: Some(len("0.3", LengthUnit::Millimetre)),
                root_fillet: None,
                helix_angle: Some(Angle {
                    value: d("30"),
                    unit: AngleUnit::Degree,
                }),
                helix_hand: Some(Hand::Right),
            }],
            holes: vec![Hole {
                feature: FeatureId(4),
                definition: RoundHole {
                    name: "Hole.2".into(),
                    diameter: len("0.25", inch()),
                    diameter_tolerance: Some(
                        Bounds::new(
                            Value::length(d("0.003"), inch()),
                            Value::length(d("-0.0"), inch()),
                        )
                        .unwrap(),
                    ),
                    depth: Some(len("0.5", inch())),
                    depth_tolerance: None,
                    through: false,
                    placement: placement.clone(),
                },
                placement: Some(placement.clone()),
            }],
            material: Some(Material {
                id: "AMS4928".into(),
                name: Some("Titanium 6-4".into()),
                density: Some(Density {
                    value: d("4.43"),
                    unit: vec![("gram".into(), d("1")), ("centimetre".into(), d("-3"))],
                }),
            }),
            notes: vec![Note {
                kind: "semantic text".into(),
                text: "#10-32 UNF".into(),
                on: Some(NoteOwner::DatumTarget(DatumTargetId(0))),
                callouts: vec![
                    Callout {
                        name: "Text.1".into(),
                        features: vec![FeatureId(0), FeatureId(4)],
                    },
                    Callout {
                        name: String::new(),
                        features: vec![],
                    },
                ],
            }],
            surface_textures: vec![SurfaceTexture {
                on: Some(FeatureId(0)),
                material_removal: MaterialRemoval::Required,
                parameters: vec![SurfaceTextureParameter {
                    characteristic: "Ra".into(),
                    value: len("0.8", LengthUnit::Micrometre),
                }],
            }],
            attributes: vec![AttributeSet {
                name: "specify-core".into(),
                on: Some(NoteOwner::Tolerance(ToleranceId(2))),
                items: vec![
                    ("a".into(), AttributeValue::Text("x".into())),
                    ("b".into(), AttributeValue::Integer(-7)),
                    ("c".into(), AttributeValue::Real(d("1.25E-3"))),
                    ("d".into(), AttributeValue::Boolean(false)),
                    ("e".into(), AttributeValue::Measure(mm("5.0"))),
                    (
                        "f".into(),
                        AttributeValue::OtherMeasure {
                            measure: "MASS_MEASURE".into(),
                            value: d("2"),
                            unit: "kilogram".into(),
                        },
                    ),
                ],
                description: Some("pmi-assist".into()),
                general_property: Some(GeneralProperty {
                    id: "g1".into(),
                    name: "user defined attribute".into(),
                    description: None,
                }),
            }],
            derived_geometry: Vec::new(),
            geometry: vec![Geometry {
                item,
                length_unit: Some(other_len),
            }],
        };
        p.validate().unwrap();
        let j = to_json(&p);
        assert_eq!(from_json(&j, "pmi").unwrap(), p);
        let text = serde_json::to_string(&j).unwrap();
        let back: Json = serde_json::from_str(&text).unwrap();
        assert_eq!(from_json(&back, "pmi").unwrap(), p);
        assert_eq!(PartPmi::default(), from_json(&json!({}), "pmi").unwrap());
        assert_eq!(to_json(&PartPmi::default()), json!({}));
    }
    #[test]
    fn derived_geometry_records_round_trip_and_validate_references() {
        let mut p = PartPmi {
            features: vec![
                Feature::Items(vec![Anchor::Face(FaceIndex(0))]),
                Feature::Derived {
                    kind: DerivedKind::Derived,
                    from: vec![FeatureId(0)],
                    geometry: Some(DerivedGeometryId(0)),
                },
            ],
            ..PartPmi::default()
        };
        let length = |n| Length {
            value: d(n),
            unit: LengthUnit::Millimetre,
        };
        p.derived_geometry.push(DerivedGeometry {
            kind: DerivedGeometryKind::Axis,
            deriving_faces: vec![FaceIndex(0)],
            point: [length("1"), length("2"), length("3")],
            direction: Direction([d("0"), d("0"), d("1")]),
            radius: Some(length("2")),
            supplemental: Vec::new(),
        });
        let j = to_json(&p);
        assert_eq!(j["features"][1]["derived"]["geometry"], 0);
        assert_eq!(j["derived_geometry"][0]["kind"], "axis");
        assert_eq!(from_json(&j, "pmi").unwrap(), p);
        p.derived_geometry[0].kind = DerivedGeometryKind::MedianPlane;
        p.derived_geometry[0].radius = None;
        // A single face's own plane uses the same record as a two-face median.
        let j = to_json(&p);
        assert_eq!(j["derived_geometry"][0]["kind"], "median_plane");
        assert_eq!(from_json(&j, "pmi").unwrap(), p);
        p.derived_geometry[0].deriving_faces.push(FaceIndex(1));
        let j = to_json(&p);
        assert_eq!(j["derived_geometry"][0]["kind"], "median_plane");
        assert_eq!(from_json(&j, "pmi").unwrap(), p);
        for (field, value) in [
            ("kind", json!("guessed")),
            ("direction", json!(["0", "0", "0"])),
            ("deriving_faces", json!([])),
            ("deriving_faces", json!([0, 1, 2])),
            ("supplemental", json!([9])),
            ("radius", length_json(&length("2"))),
        ] {
            let mut bad = j.clone();
            bad["derived_geometry"][0][field] = value;
            assert!(from_json(&bad, "pmi").is_err(), "{field}");
        }
        // Equal computed targets remain equal even when the geometry records have
        // different indices (as separate shape aspects in a file can have).
        p.derived_geometry.push(p.derived_geometry[0].clone());
        p.features.push(Feature::Derived {
            kind: DerivedKind::Derived,
            from: vec![FeatureId(0)],
            geometry: Some(DerivedGeometryId(1)),
        });
        assert!(p.same_target(
            &ToleranceTarget::Feature(FeatureId(1)),
            &ToleranceTarget::Feature(FeatureId(2))
        ));
        let mut bad = j;
        bad["features"][1]["derived"]["geometry"] = json!(9);
        assert!(
            from_json(&bad, "pmi")
                .unwrap_err()
                .message
                .contains("missing derived geometry")
        );
    }
}
