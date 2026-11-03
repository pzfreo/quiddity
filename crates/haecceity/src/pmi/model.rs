//! The semantic PMI model: plain Rust data for the GD&T concepts the CAx-IF PMI practice
//! (v4.1, "§") and ISO 10303-242 represent, independent of Part 21. No type here holds a file
//! instance id; faces and edges are the part's indices ([`Anchor`]), and every other reference
//! is an index into the owning [`PartPmi`]'s vectors.
//!
//! Values are kept **as stated**: a [`Decimal`] is the decimal text a file or a consumer gives
//! (`0.0030` stays `0.0030`), in its own unit. Conversion to millimetres or radians is a view
//! ([`Length::mm`], [`Angle::rad`]), never storage.
//!
//! Invariants that concern one value are enforced by its constructor (fields private, read by
//! accessors): [`Decimal`], [`DatumLabel`], [`Datum`], [`DatumTarget`], [`DatumSystem`],
//! [`Bounds`]. Invariants across a part (every id resolves, labels unique, datum references
//! by tolerance kind, composite relations) are checked by [`PartPmi::validate`].

use std::cmp::Ordering;
use std::fmt;

/// Why a model value or a part's PMI is not valid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelError(pub String);

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ModelError {}

fn err<T>(why: impl Into<String>) -> Result<T, ModelError> {
    Err(ModelError(why.into()))
}

// ---------------------------------------------------------------------------------------------
// Values and units
// ---------------------------------------------------------------------------------------------

/// A decimal number as stated: optional sign, digits, optionally `.` and digits, optionally an
/// exponent (`E` or `e`, optional sign, digits). At least one digit precedes the point, as in a
/// Part 21 REAL (`35.`, `0.0030`, `2.54E1`); an integer (`35`) is a decimal without a point.
/// Equality (`==`) is textual: `0.5` and `0.50` are different statements of one value;
/// [`Decimal::cmp_value`] compares values exactly.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json-import", serde(try_from = "String"))]
pub struct Decimal(String);

impl Decimal {
    /// The decimal `text`, refused when it is not one.
    pub fn parse(text: &str) -> Result<Decimal, ModelError> {
        if parts(text).is_none() {
            return err(format!("{text:?} is not a decimal"));
        }
        Ok(Decimal(text.to_string()))
    }

    /// The text as stated.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The nearest `f64`.
    #[must_use]
    pub fn to_f64(&self) -> f64 {
        self.0.parse().unwrap_or(f64::NAN)
    }

    /// Digits after the decimal point as written (`0.0030` → 4, `35.` → 0, `35` → 0); `None`
    /// when an exponent is written.
    #[must_use]
    pub fn stated_places(&self) -> Option<usize> {
        let (_, int, frac, exp) = parts(&self.0)?;
        let _ = int;
        if exp.is_some() {
            return None;
        }
        Some(frac.len())
    }

    /// Exact comparison of the values (`0.5` equals `0.50` and `5.E-1`).
    #[must_use]
    pub fn cmp_value(&self, other: &Decimal) -> Ordering {
        exact(&self.0).cmp_exact(&exact(&other.0))
    }

    /// Whether the value is exactly zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        exact(&self.0).digits.is_empty()
    }

    /// Whether the value is below zero.
    #[must_use]
    pub fn is_negative(&self) -> bool {
        let e = exact(&self.0);
        e.negative && !e.digits.is_empty()
    }

    /// This value times `10^power`, exactly, written in plain or exponent form.
    #[must_use]
    pub fn scaled(&self, power: i64) -> Decimal {
        let mut e = exact(&self.0);
        if !e.digits.is_empty() {
            e.exp += power;
        }
        Decimal(e.text())
    }

    /// The exact product of two decimals.
    #[must_use]
    pub fn times(&self, other: &Decimal) -> Decimal {
        let (a, b) = (exact(&self.0), exact(&other.0));
        if a.digits.is_empty() || b.digits.is_empty() {
            return Decimal("0".to_string());
        }
        // Schoolbook multiplication of the digit strings.
        let x: Vec<u32> = a.digits.iter().rev().map(|&d| u32::from(d)).collect();
        let y: Vec<u32> = b.digits.iter().rev().map(|&d| u32::from(d)).collect();
        let mut acc = vec![0u32; x.len() + y.len()];
        for (i, &p) in x.iter().enumerate() {
            for (j, &q) in y.iter().enumerate() {
                acc[i + j] += p * q;
            }
        }
        let mut carry = 0;
        for d in &mut acc {
            let v = *d + carry;
            *d = v % 10;
            carry = v / 10;
        }
        while carry > 0 {
            acc.push(carry % 10);
            carry /= 10;
        }
        let digits: Vec<u8> = acc.iter().rev().map(|&d| d as u8).collect();
        let mut out = Exact {
            negative: a.negative != b.negative,
            digits,
            exp: a.exp + b.exp,
        };
        out.normalise();
        Decimal(out.text())
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Sign, integer digits, fraction digits and exponent digits (with sign) of a decimal text.
fn parts(t: &str) -> Option<(bool, &str, &str, Option<&str>)> {
    let b = t.as_bytes();
    let mut i = 0;
    let negative = match b.first() {
        Some(b'-') => {
            i = 1;
            true
        }
        Some(b'+') => {
            i = 1;
            false
        }
        _ => false,
    };
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == int_start {
        return None;
    }
    let int = &t[int_start..i];
    let mut frac = "";
    if b.get(i) == Some(&b'.') {
        let f = i + 1;
        i = f;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        frac = &t[f..i];
    }
    let mut exp = None;
    if matches!(b.get(i), Some(b'E' | b'e')) {
        let s = i + 1;
        i = s;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let d = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == d {
            return None;
        }
        exp = Some(&t[s..i]);
    }
    (i == b.len()).then_some((negative, int, frac, exp))
}

/// A decimal as `±digits × 10^exp` with no leading or trailing zero digits (zero: no digits).
struct Exact {
    negative: bool,
    digits: Vec<u8>,
    exp: i64,
}

fn exact(t: &str) -> Exact {
    #[allow(
        clippy::expect_used,
        reason = "a Decimal holds only text Decimal::parse accepted"
    )]
    let (negative, int, frac, e) = parts(t).expect("a validated decimal");
    let mut digits: Vec<u8> = int.bytes().chain(frac.bytes()).map(|c| c - b'0').collect();
    let exp = e.map_or(0, |e| e.parse::<i64>().unwrap_or(0)) - frac.len() as i64;
    let mut out = Exact {
        negative,
        digits: std::mem::take(&mut digits),
        exp,
    };
    out.normalise();
    out
}

impl Exact {
    fn normalise(&mut self) {
        let lead = self.digits.iter().take_while(|&&d| d == 0).count();
        self.digits.drain(..lead);
        while self.digits.last() == Some(&0) {
            self.digits.pop();
            self.exp += 1;
        }
        if self.digits.is_empty() {
            self.exp = 0;
            self.negative = false;
        }
    }

    fn cmp_exact(&self, o: &Exact) -> Ordering {
        let sign = |e: &Exact| {
            if e.digits.is_empty() {
                0
            } else if e.negative {
                -1
            } else {
                1
            }
        };
        let (s, t) = (sign(self), sign(o));
        if s != t || s == 0 {
            return s.cmp(&t);
        }
        // Magnitudes: the position of the leading digit, then the digits.
        let lead = |e: &Exact| e.digits.len() as i64 + e.exp;
        let magnitude = lead(self).cmp(&lead(o)).then_with(|| {
            let n = self.digits.len().max(o.digits.len());
            (0..n)
                .map(|i| {
                    (
                        self.digits.get(i).copied().unwrap_or(0),
                        o.digits.get(i).copied().unwrap_or(0),
                    )
                })
                .map(|(a, b)| a.cmp(&b))
                .find(|c| c.is_ne())
                .unwrap_or(Ordering::Equal)
        });
        if s < 0 {
            magnitude.reverse()
        } else {
            magnitude
        }
    }

    /// Plain notation when the exponent is small, otherwise `d.dddE±n`.
    fn text(&self) -> String {
        if self.digits.is_empty() {
            return "0.".to_string();
        }
        let sign = if self.negative { "-" } else { "" };
        let digits: String = self.digits.iter().map(|d| char::from(b'0' + d)).collect();
        let n = digits.len() as i64;
        if self.exp >= 0 && self.exp <= 12 {
            return format!("{sign}{digits}{}.", "0".repeat(self.exp as usize));
        }
        if self.exp < 0 && n + self.exp > 0 {
            let split = (n + self.exp) as usize;
            return format!("{sign}{}.{}", &digits[..split], &digits[split..]);
        }
        if self.exp < 0 && n + self.exp > -12 {
            return format!("{sign}0.{}{digits}", "0".repeat((-(n + self.exp)) as usize));
        }
        format!(
            "{sign}{}.{}E{}",
            &digits[..1],
            &digits[1..],
            n - 1 + self.exp
        )
    }
}

/// A unit of length.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum LengthUnit {
    Millimetre,
    Micrometre,
    Centimetre,
    Metre,
    Inch,
    Foot,
    /// Any other unit: its name and its size in metres, as stated (exactly derived from the
    /// file's conversion factor).
    Other {
        name: String,
        metres: Decimal,
    },
}

impl LengthUnit {
    /// The unit's size in millimetres.
    #[must_use]
    pub fn millimetres(&self) -> f64 {
        match self {
            LengthUnit::Millimetre => 1.0,
            LengthUnit::Micrometre => 1e-3,
            LengthUnit::Centimetre => 10.0,
            LengthUnit::Metre => 1000.0,
            LengthUnit::Inch => 25.4,
            LengthUnit::Foot => 304.8,
            LengthUnit::Other { metres, .. } => metres.to_f64() * 1000.0,
        }
    }

    /// The unit's size in metres, exactly.
    #[must_use]
    pub fn metres(&self) -> Decimal {
        let d = |t: &str| Decimal(t.to_string());
        match self {
            LengthUnit::Millimetre => d("0.001"),
            LengthUnit::Micrometre => d("0.000001"),
            LengthUnit::Centimetre => d("0.01"),
            LengthUnit::Metre => d("1."),
            LengthUnit::Inch => d("0.0254"),
            LengthUnit::Foot => d("0.3048"),
            LengthUnit::Other { metres, .. } => metres.clone(),
        }
    }

    /// The named unit whose size is exactly `metres`, else [`LengthUnit::Other`].
    #[must_use]
    pub fn from_metres(name: &str, metres: Decimal) -> LengthUnit {
        let known = [
            LengthUnit::Millimetre,
            LengthUnit::Micrometre,
            LengthUnit::Centimetre,
            LengthUnit::Metre,
            LengthUnit::Inch,
            LengthUnit::Foot,
        ];
        known
            .into_iter()
            .find(|u| u.metres().cmp_value(&metres).is_eq())
            .unwrap_or(LengthUnit::Other {
                name: name.to_string(),
                metres,
            })
    }
}

/// A unit of plane angle.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum AngleUnit {
    Radian,
    Degree,
    /// Any other unit: its name and its size in radians as stated.
    Other {
        name: String,
        radians: Decimal,
    },
}

impl AngleUnit {
    /// The unit's size in radians.
    #[must_use]
    pub fn radians(&self) -> f64 {
        match self {
            AngleUnit::Radian => 1.0,
            AngleUnit::Degree => std::f64::consts::PI / 180.0,
            AngleUnit::Other { radians, .. } => radians.to_f64(),
        }
    }
}

/// A length as stated.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct Length {
    pub value: Decimal,
    pub unit: LengthUnit,
}

impl Length {
    /// The length in millimetres, for comparison.
    #[must_use]
    pub fn mm(&self) -> f64 {
        self.value.to_f64() * self.unit.millimetres()
    }
}

/// An angle as stated.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct Angle {
    pub value: Decimal,
    pub unit: AngleUnit,
}

impl Angle {
    /// The angle in radians, for comparison.
    #[must_use]
    pub fn rad(&self) -> f64 {
        self.value.to_f64() * self.unit.radians()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum Quantity {
    Length(Length),
    Angle(Angle),
}

impl Quantity {
    /// The value as stated.
    #[must_use]
    pub fn decimal(&self) -> &Decimal {
        match self {
            Quantity::Length(l) => &l.value,
            Quantity::Angle(a) => &a.value,
        }
    }

    /// Millimetres or radians.
    #[must_use]
    pub fn si(&self) -> f64 {
        match self {
            Quantity::Length(l) => l.mm(),
            Quantity::Angle(a) => a.rad(),
        }
    }

    /// Whether both are lengths or both are angles.
    #[must_use]
    pub fn same_kind(&self, other: &Quantity) -> bool {
        matches!(
            (self, other),
            (Quantity::Length(_), Quantity::Length(_)) | (Quantity::Angle(_), Quantity::Angle(_))
        )
    }

    /// Exact comparison of two quantities of one kind in one unit; `None` across units or kinds.
    #[must_use]
    pub fn cmp_same_unit(&self, other: &Quantity) -> Option<Ordering> {
        match (self, other) {
            (Quantity::Length(a), Quantity::Length(b)) if a.unit == b.unit => {
                Some(a.value.cmp_value(&b.value))
            }
            (Quantity::Angle(a), Quantity::Angle(b)) if a.unit == b.unit => {
                Some(a.value.cmp_value(&b.value))
            }
            _ => None,
        }
    }
}

/// A stated value and the number of decimal places it is to be shown with, when stated
/// (`value_format_type_qualifier`, §5.4: the `y` of `NR2 x.y`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct Value {
    pub quantity: Quantity,
    pub decimal_places: Option<u8>,
}

impl Value {
    #[must_use]
    pub fn length(value: Decimal, unit: LengthUnit) -> Value {
        Value {
            quantity: Quantity::Length(Length { value, unit }),
            decimal_places: None,
        }
    }

    #[must_use]
    pub fn angle(value: Decimal, unit: AngleUnit) -> Value {
        Value {
            quantity: Quantity::Angle(Angle { value, unit }),
            decimal_places: None,
        }
    }

    /// The decimal as stated.
    #[must_use]
    pub fn decimal(&self) -> &Decimal {
        self.quantity.decimal()
    }

    /// Millimetres or radians.
    #[must_use]
    pub fn si(&self) -> f64 {
        self.quantity.si()
    }

    #[must_use]
    pub fn as_length(&self) -> Option<&Length> {
        match &self.quantity {
            Quantity::Length(l) => Some(l),
            Quantity::Angle(_) => None,
        }
    }
}

/// A ratio as stated (`ratio_measure`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
pub struct Ratio(pub Decimal);

/// A count (`count_measure`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
pub struct Count(pub u32);

/// An upper and a lower value of one kind (both lengths or both angles), `upper > lower`, with
/// no sign assumed: deviations of g6 lie wholly below nominal, both negative (§5.2.3).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
#[cfg_attr(
    feature = "json-import",
    serde(try_from = "crate::json::invariants::BoundsInput")
)]
pub struct Bounds {
    upper: Value,
    lower: Value,
}

impl Bounds {
    /// Refused unless both are of one kind and `upper > lower` (compared exactly when in one
    /// unit, else in millimetres or radians).
    pub fn new(upper: Value, lower: Value) -> Result<Bounds, ModelError> {
        if !upper.quantity.same_kind(&lower.quantity) {
            return err("upper and lower bound are not of one kind");
        }
        let order = upper
            .quantity
            .cmp_same_unit(&lower.quantity)
            .unwrap_or_else(|| upper.si().total_cmp(&lower.si()));
        if order != Ordering::Greater {
            return err(format!(
                "upper bound {} is not greater than lower bound {}",
                upper.decimal(),
                lower.decimal()
            ));
        }
        Ok(Bounds { upper, lower })
    }

    #[must_use]
    pub fn upper(&self) -> &Value {
        &self.upper
    }

    #[must_use]
    pub fn lower(&self) -> &Value {
        &self.lower
    }
}

// ---------------------------------------------------------------------------------------------
// Ids and anchors
// ---------------------------------------------------------------------------------------------

macro_rules! id {
    ($($(#[$m:meta])* $name:ident),* $(,)?) => {$(
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[cfg_attr(feature = "json", derive(serde::Serialize))]
        #[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
        pub struct $name(pub usize);
    )*};
}

id!(
    /// A part: the index of a distinct part in `step::read_part_definitions`' list.
    PartId,
    /// A face of a part: its index in the part's face numbering.
    FaceIndex,
    /// An edge of a part: its index in the part's edge numbering.
    EdgeIndex,
    /// Index into [`PartPmi::features`].
    FeatureId,
    /// Index into [`PartPmi::datums`].
    DatumId,
    /// Index into [`PartPmi::datum_targets`].
    DatumTargetId,
    /// Index into [`PartPmi::dimensions`].
    DimensionId,
    /// Index into [`PartPmi::tolerances`].
    ToleranceId,
    /// Index into [`PartPmi::geometry`].
    GeometryId,
    /// Index into [`PartPmi::derived_geometry`].
    DerivedGeometryId,
);

/// What a feature is made of: a face or an edge of the part, or supplemental geometry (a
/// point, curve, surface or placement that is not the part's topology: a hole's axis, a target's
/// point, a datum plane; CAx-IF *Supplemental Geometry* practice, PMI practice §3.9.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum Anchor {
    Face(FaceIndex),
    Edge(EdgeIndex),
    Geometry(GeometryId),
}

/// Supplemental geometry, held by value: the geometric item as its schema entity and attribute
/// values (no instance ids), and the length unit of the representation it is given in.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct Geometry {
    pub item: GeometryItem,
    pub length_unit: Option<LengthUnit>,
}

/// One geometric item: its entity (one leaf, or the leaves of a complex instance in the
/// external mapping's order), each with its own attribute values.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct GeometryItem {
    pub leaves: Vec<(String, Vec<GeometryValue>)>,
}

impl GeometryItem {
    /// The entity type (lower case; complex leaves joined by `+`).
    #[must_use]
    pub fn entity(&self) -> String {
        self.leaves
            .iter()
            .map(|(n, _)| n.to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join("+")
    }
}

/// An attribute value of a geometric item, as stated.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum GeometryValue {
    Real(Decimal),
    Integer(i64),
    /// A string, decoded.
    Text(String),
    /// An enumeration, boolean or logical value, without the dots.
    Enum(String),
    Binary(String),
    Item(Box<GeometryItem>),
    List(Vec<GeometryValue>),
    Typed {
        type_name: String,
        value: Box<GeometryValue>,
    },
    Unset,
    Derived,
}

// ---------------------------------------------------------------------------------------------
// Features
// ---------------------------------------------------------------------------------------------

/// What PMI applies to: a shape aspect of the part (§3.8, §6.1).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum Feature {
    /// One feature made of one or more faces or edges (§5.1 Figure 5, §6.5.1).
    Items(Vec<Anchor>),
    /// Several features taken together (§6.4): each member individually (multiple elements,
    /// pattern of features) or as stated by the kind (all around, between).
    Group {
        members: Vec<FeatureId>,
        kind: GroupKind,
    },
    /// A derived feature (§5.1.4, Table 3) and the features it is derived from.
    Derived {
        kind: DerivedKind,
        from: Vec<FeatureId>,
        /// Computed geometry, absent when the derivation is refused (see findings).
        geometry: Option<DerivedGeometryId>,
    },
}

/// Geometry computed from a derived shape aspect's deriving faces. Coordinates are in the
/// part's local coordinate system, in millimetres; directions are unit vectors. These are
/// computed decimals, not source measure tokens. Face indices use the usual part numbering.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct DerivedGeometry {
    pub kind: DerivedGeometryKind,
    pub deriving_faces: Vec<FaceIndex>,
    /// A point on the axis or median plane.
    pub point: [Length; 3],
    /// Axis direction or median-plane normal (its sign has no significance).
    pub direction: Direction,
    /// Present only when all deriving faces belong to one cylinder.
    pub radius: Option<Length>,
    /// Explicit supplemental items that passed the geometric cross-check.
    pub supplemental: Vec<GeometryId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum DerivedGeometryKind {
    Axis,
    /// One face's own plane, or the median of two parallel deriving faces.
    MedianPlane,
}

/// How a group's members are taken (§6.4).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum GroupKind {
    /// `composite_group_shape_aspect` 'multiple elements' (§6.4).
    #[cfg_attr(feature = "json", serde(rename = "multiple elements"))]
    MultipleElements,
    /// `composite_group_shape_aspect` 'pattern of features' (§6.4.1).
    #[cfg_attr(feature = "json", serde(rename = "pattern of features"))]
    PatternOfFeatures,
    /// `all_around_shape_aspect` (§6.4.2).
    #[cfg_attr(feature = "json", serde(rename = "all_around_shape_aspect"))]
    AllAround,
    /// `between_shape_aspect` (§6.4.3).
    #[cfg_attr(feature = "json", serde(rename = "between_shape_aspect"))]
    Between,
    /// A `composite_group_shape_aspect` whose name and description state neither string: the
    /// file does not say which (a finding when read).
    Unstated,
}

/// The kind of a derived feature (§5.1.4, Table 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum DerivedKind {
    /// Plain `derived_shape_aspect`.
    #[cfg_attr(feature = "json", serde(rename = "derived_shape_aspect"))]
    Derived,
    Apex,
    CentreOfSymmetry,
    GeometricAlignment,
    PerpendicularTo,
    Extension,
    Tangent,
    ParallelOffset,
}

impl DerivedKind {
    /// The kind of an entity type name (lower case), if it is one.
    #[must_use]
    pub fn of_entity(name: &str) -> Option<DerivedKind> {
        Some(match name {
            "derived_shape_aspect" => DerivedKind::Derived,
            "apex" => DerivedKind::Apex,
            "centre_of_symmetry" => DerivedKind::CentreOfSymmetry,
            "geometric_alignment" => DerivedKind::GeometricAlignment,
            "perpendicular_to" => DerivedKind::PerpendicularTo,
            "extension" => DerivedKind::Extension,
            "tangent" => DerivedKind::Tangent,
            "parallel_offset" => DerivedKind::ParallelOffset,
            _ => return None,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Datums
// ---------------------------------------------------------------------------------------------

/// A datum's identification (`datum.identification`): non-empty, no white space ('A', 'AA').
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json-import", serde(try_from = "String"))]
pub struct DatumLabel(String);

impl DatumLabel {
    pub fn new(label: &str) -> Result<DatumLabel, ModelError> {
        if label.is_empty() || label.chars().any(char::is_whitespace) {
            return err(format!("{label:?} is not a datum label"));
        }
        Ok(DatumLabel(label.to_string()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DatumLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A datum (§6.5): its label and what establishes it, its datum feature and/or its datum
/// targets. It has no position: precedence exists only in a [`DatumSystem`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
#[cfg_attr(
    feature = "json-import",
    serde(try_from = "crate::json::invariants::DatumInput")
)]
pub struct Datum {
    label: DatumLabel,
    feature: Option<FeatureId>,
    targets: Vec<DatumTargetId>,
}

impl Datum {
    /// Refused when neither a feature nor a target establishes it (`datum` WR1) or a target is
    /// listed twice. At most one feature is the type's own guarantee (WR2).
    pub fn new(
        label: DatumLabel,
        feature: Option<FeatureId>,
        targets: Vec<DatumTargetId>,
    ) -> Result<Datum, ModelError> {
        if feature.is_none() && targets.is_empty() {
            return err(format!(
                "datum {label} is established by neither a feature nor a target"
            ));
        }
        let mut seen = targets.clone();
        seen.sort();
        if seen.windows(2).any(|w| w[0] == w[1]) {
            return err(format!("datum {label} lists a target twice"));
        }
        Ok(Datum {
            label,
            feature,
            targets,
        })
    }

    #[must_use]
    pub fn label(&self) -> &DatumLabel {
        &self.label
    }

    /// The datum feature: the feature itself (the writer writes it as the `datum_feature`).
    #[must_use]
    pub fn feature(&self) -> Option<FeatureId> {
        self.feature
    }

    #[must_use]
    pub fn targets(&self) -> &[DatumTargetId] {
        &self.targets
    }
}

/// A placement: origin and axes (`axis2_placement_3d`), the origin in the representation
/// context's length unit.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct Placement {
    pub origin: [Length; 3],
    /// The Z axis, when stated.
    pub axis: Option<Direction>,
    /// The X axis, when stated.
    pub ref_direction: Option<Direction>,
}

/// A direction's ratios as stated.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
pub struct Direction(pub [Decimal; 3]);

/// The shape of a datum target (§6.6.1 Table 10, §6.6.2 Table 11).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum TargetShape {
    Point,
    Line {
        length: Length,
    },
    Rectangle {
        length: Length,
        width: Length,
    },
    Circle {
        diameter: Length,
    },
    #[cfg_attr(feature = "json", serde(rename = "circular curve"))]
    CircularCurve {
        diameter: Length,
    },
    /// An explicit area: the feature that is the area (§6.6.2).
    Area(FeatureId),
    /// An explicit curve: the feature that is the curve (§6.6.2 Table 11).
    Curve(FeatureId),
}

/// A datum target (§6.6): 'A1' is datum A's target 1.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
#[cfg_attr(
    feature = "json-import",
    serde(try_from = "crate::json::invariants::TargetInput")
)]
pub struct DatumTarget {
    number: u32,
    shape: TargetShape,
    placement: Option<Placement>,
    movable: Option<Direction>,
    on: Option<FeatureId>,
}

impl DatumTarget {
    /// Refused when `number` is 0 (`datum_target` WR3), or a placed shape (point, line,
    /// rectangle, circle, circular curve) has no placement.
    pub fn new(
        number: u32,
        shape: TargetShape,
        placement: Option<Placement>,
        movable: Option<Direction>,
        on: Option<FeatureId>,
    ) -> Result<DatumTarget, ModelError> {
        if number == 0 {
            return err("a datum target's number is 0");
        }
        let placed = !matches!(shape, TargetShape::Area(_) | TargetShape::Curve(_));
        if placed && placement.is_none() {
            return err(format!(
                "placed datum target {number} ({shape:?}) has no placement"
            ));
        }
        Ok(DatumTarget {
            number,
            shape,
            placement,
            movable,
            on,
        })
    }

    #[must_use]
    pub fn number(&self) -> u32 {
        self.number
    }

    #[must_use]
    pub fn shape(&self) -> &TargetShape {
        &self.shape
    }

    /// Where a placed target is (§6.6.1).
    #[must_use]
    pub fn placement(&self) -> Option<&Placement> {
        self.placement.as_ref()
    }

    /// The direction a movable target moves in (§6.6.4).
    #[must_use]
    pub fn movable(&self) -> Option<&Direction> {
        self.movable.as_ref()
    }

    /// The feature the target lies on (`feature_for_datum_target_relationship`, §6.6.3).
    #[must_use]
    pub fn on(&self) -> Option<FeatureId> {
        self.on
    }
}

/// A modifier of a datum reference (§6.9.7 Table 15; schema `datum_reference_modifier`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum DatumModifier {
    Simple(SimpleDatumModifier),
    /// `datum_reference_modifier_with_value` (Figure 65).
    WithValue {
        kind: DatumModifierType,
        value: Value,
    },
}

/// Schema `simple_datum_reference_modifier`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum SimpleDatumModifier {
    #[cfg_attr(feature = "json", serde(rename = "free_state"))]
    FreeState,
    #[cfg_attr(feature = "json", serde(rename = "basic"))]
    Basic,
    #[cfg_attr(feature = "json", serde(rename = "translation"))]
    Translation,
    #[cfg_attr(feature = "json", serde(rename = "least_material_requirement"))]
    LeastMaterialRequirement,
    #[cfg_attr(feature = "json", serde(rename = "maximum_material_requirement"))]
    MaximumMaterialRequirement,
    #[cfg_attr(feature = "json", serde(rename = "point"))]
    Point,
    #[cfg_attr(feature = "json", serde(rename = "line"))]
    Line,
    #[cfg_attr(feature = "json", serde(rename = "plane"))]
    Plane,
    #[cfg_attr(feature = "json", serde(rename = "orientation"))]
    Orientation,
    #[cfg_attr(feature = "json", serde(rename = "any_cross_section"))]
    AnyCrossSection,
    #[cfg_attr(feature = "json", serde(rename = "any_longitudinal_section"))]
    AnyLongitudinalSection,
    #[cfg_attr(feature = "json", serde(rename = "contacting_feature"))]
    ContactingFeature,
    #[cfg_attr(feature = "json", serde(rename = "distance_variable"))]
    DistanceVariable,
    #[cfg_attr(feature = "json", serde(rename = "degree_of_freedom_constraint_x"))]
    DegreeOfFreedomConstraintX,
    #[cfg_attr(feature = "json", serde(rename = "degree_of_freedom_constraint_y"))]
    DegreeOfFreedomConstraintY,
    #[cfg_attr(feature = "json", serde(rename = "degree_of_freedom_constraint_z"))]
    DegreeOfFreedomConstraintZ,
    #[cfg_attr(feature = "json", serde(rename = "degree_of_freedom_constraint_u"))]
    DegreeOfFreedomConstraintU,
    #[cfg_attr(feature = "json", serde(rename = "degree_of_freedom_constraint_v"))]
    DegreeOfFreedomConstraintV,
    #[cfg_attr(feature = "json", serde(rename = "degree_of_freedom_constraint_w"))]
    DegreeOfFreedomConstraintW,
    #[cfg_attr(feature = "json", serde(rename = "minor_diameter"))]
    MinorDiameter,
    #[cfg_attr(feature = "json", serde(rename = "major_diameter"))]
    MajorDiameter,
    #[cfg_attr(feature = "json", serde(rename = "pitch_diameter"))]
    PitchDiameter,
}

/// Schema `datum_reference_modifier_type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum DatumModifierType {
    CircularOrCylindrical,
    Spherical,
    Distance,
    Projected,
}

/// A reference to one datum in a compartment, with its own modifiers.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct DatumReference {
    pub datum: DatumId,
    pub modifiers: Vec<DatumModifier>,
}

/// One compartment of a datum system: one datum, or several of equal weight (a common datum,
/// A–B, §6.9.8), and the modifiers of the compartment as a whole.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct Compartment {
    pub references: Vec<DatumReference>,
    pub modifiers: Vec<DatumModifier>,
}

/// A datum reference frame: primary, secondary, tertiary compartments in order (§6.9.7). A
/// value: two equal systems are one system.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json-import", serde(try_from = "Vec<Compartment>"))]
pub struct DatumSystem(Vec<Compartment>);

impl DatumSystem {
    /// Refused unless it has 1 to 3 compartments (`datum_system.constituents` LIST [1:3]), each
    /// with at least one reference.
    pub fn new(compartments: Vec<Compartment>) -> Result<DatumSystem, ModelError> {
        if compartments.is_empty() || compartments.len() > 3 {
            return err(format!(
                "a datum system has {} compartments (1 to 3)",
                compartments.len()
            ));
        }
        if compartments.iter().any(|c| c.references.is_empty()) {
            return err("a datum system compartment references no datum");
        }
        Ok(DatumSystem(compartments))
    }

    #[must_use]
    pub fn compartments(&self) -> &[Compartment] {
        &self.0
    }
}

// ---------------------------------------------------------------------------------------------
// Dimensions
// ---------------------------------------------------------------------------------------------

/// A dimension (§5).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct Dimension {
    pub kind: DimensionKind,
    /// A length, or an angle for an angular dimension. `None` only where a file states no
    /// nominal value; read and written without inventing one.
    pub nominal: Option<Value>,
    pub tolerance: DimTolerance,
    /// Maximum, minimum, average (§5.2.2 Table 6).
    pub qualifier: Option<Qualifier>,
    /// Type 1 'auxiliary' and the type 2 modifiers (§5.3 Tables 7–8); 'theoretical' is
    /// [`DimTolerance::Basic`].
    pub modifiers: Vec<DimensionModifier>,
    /// The tolerance principle stated by `shape_dimension_representation.name` (§5.2.1
    /// Table 5); `None` for '' (the standard's default, §4).
    pub principle: Option<Principle>,
}

/// What a dimension measures.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum DimensionKind {
    /// `dimensional_size` (§5.1.5), `angular_size` (§5.1.6, with `angle`), with path (§5.1.7).
    Size {
        feature: FeatureId,
        kind: SizeKind,
        path: Option<FeatureId>,
        angle: Option<AngleSelection>,
    },
    /// `dimensional_location` (§5.1.1), `angular_location` (§5.1.2, with `angle`), with path,
    /// directed (from `from` to `to`).
    Location {
        from: FeatureId,
        to: FeatureId,
        kind: LocationKind,
        path: Option<FeatureId>,
        directed: bool,
        angle: Option<AngleSelection>,
    },
}

/// Schema `angle_relator`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum AngleSelection {
    Equal,
    Large,
    Small,
}

/// `dimensional_size.name` (§5.1.5 Table 4).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum SizeKind {
    #[cfg_attr(feature = "json", serde(rename = "curve length"))]
    CurveLength,
    #[cfg_attr(feature = "json", serde(rename = "diameter"))]
    Diameter,
    #[cfg_attr(feature = "json", serde(rename = "spherical diameter"))]
    SphericalDiameter,
    #[cfg_attr(feature = "json", serde(rename = "radius"))]
    Radius,
    #[cfg_attr(feature = "json", serde(rename = "spherical radius"))]
    SphericalRadius,
    #[cfg_attr(feature = "json", serde(rename = "toroidal minor diameter"))]
    ToroidalMinorDiameter,
    #[cfg_attr(feature = "json", serde(rename = "toroidal major diameter"))]
    ToroidalMajorDiameter,
    #[cfg_attr(feature = "json", serde(rename = "toroidal minor radius"))]
    ToroidalMinorRadius,
    #[cfg_attr(feature = "json", serde(rename = "toroidal major radius"))]
    ToroidalMajorRadius,
    #[cfg_attr(feature = "json", serde(rename = "toroidal high major diameter"))]
    ToroidalHighMajorDiameter,
    #[cfg_attr(feature = "json", serde(rename = "toroidal low major diameter"))]
    ToroidalLowMajorDiameter,
    #[cfg_attr(feature = "json", serde(rename = "toroidal high major radius"))]
    ToroidalHighMajorRadius,
    #[cfg_attr(feature = "json", serde(rename = "toroidal low major radius"))]
    ToroidalLowMajorRadius,
    #[cfg_attr(feature = "json", serde(rename = "thickness"))]
    Thickness,
    /// A name outside Table 4, as stated (angular sizes name theirs freely, e.g. 'angle').
    Other(String),
}

impl SizeKind {
    pub const TABLE: [(&'static str, SizeKind); 14] = [
        ("curve length", SizeKind::CurveLength),
        ("diameter", SizeKind::Diameter),
        ("spherical diameter", SizeKind::SphericalDiameter),
        ("radius", SizeKind::Radius),
        ("spherical radius", SizeKind::SphericalRadius),
        ("toroidal minor diameter", SizeKind::ToroidalMinorDiameter),
        ("toroidal major diameter", SizeKind::ToroidalMajorDiameter),
        ("toroidal minor radius", SizeKind::ToroidalMinorRadius),
        ("toroidal major radius", SizeKind::ToroidalMajorRadius),
        (
            "toroidal high major diameter",
            SizeKind::ToroidalHighMajorDiameter,
        ),
        (
            "toroidal low major diameter",
            SizeKind::ToroidalLowMajorDiameter,
        ),
        (
            "toroidal high major radius",
            SizeKind::ToroidalHighMajorRadius,
        ),
        (
            "toroidal low major radius",
            SizeKind::ToroidalLowMajorRadius,
        ),
        ("thickness", SizeKind::Thickness),
    ];

    /// The kind a `dimensional_size.name` states.
    #[must_use]
    pub fn from_name(name: &str) -> SizeKind {
        Self::TABLE
            .iter()
            .find(|(n, _)| *n == name)
            .map_or_else(|| SizeKind::Other(name.to_string()), |(_, k)| k.clone())
    }

    /// The name Table 4 gives it.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            SizeKind::Other(n) => n,
            k => Self::TABLE
                .iter()
                .find(|(_, t)| t == k)
                .map_or("", |(n, _)| n),
        }
    }
}

/// `dimensional_location.name` (§5.1.1 Tables 1–2).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum LocationKind {
    #[cfg_attr(feature = "json", serde(rename = "curved distance"))]
    CurvedDistance,
    #[cfg_attr(feature = "json", serde(rename = "linear distance"))]
    LinearDistance,
    #[cfg_attr(feature = "json", serde(rename = "linear distance centre outer"))]
    LinearDistanceCentreOuter,
    #[cfg_attr(feature = "json", serde(rename = "linear distance centre inner"))]
    LinearDistanceCentreInner,
    #[cfg_attr(feature = "json", serde(rename = "linear distance outer centre"))]
    LinearDistanceOuterCentre,
    #[cfg_attr(feature = "json", serde(rename = "linear distance outer outer"))]
    LinearDistanceOuterOuter,
    #[cfg_attr(feature = "json", serde(rename = "linear distance outer inner"))]
    LinearDistanceOuterInner,
    #[cfg_attr(feature = "json", serde(rename = "linear distance inner centre"))]
    LinearDistanceInnerCentre,
    #[cfg_attr(feature = "json", serde(rename = "linear distance inner outer"))]
    LinearDistanceInnerOuter,
    #[cfg_attr(feature = "json", serde(rename = "linear distance inner inner"))]
    LinearDistanceInnerInner,
    #[cfg_attr(feature = "json", serde(rename = "linear distance centre"))]
    LinearDistanceCentre,
    #[cfg_attr(feature = "json", serde(rename = "linear distance inner"))]
    LinearDistanceInner,
    #[cfg_attr(feature = "json", serde(rename = "linear distance outer"))]
    LinearDistanceOuter,
    /// A name outside Tables 1–2, as stated (angular locations name theirs freely).
    Other(String),
}

impl LocationKind {
    pub const TABLE: [(&'static str, LocationKind); 13] = [
        ("curved distance", LocationKind::CurvedDistance),
        ("linear distance", LocationKind::LinearDistance),
        (
            "linear distance centre outer",
            LocationKind::LinearDistanceCentreOuter,
        ),
        (
            "linear distance centre inner",
            LocationKind::LinearDistanceCentreInner,
        ),
        (
            "linear distance outer centre",
            LocationKind::LinearDistanceOuterCentre,
        ),
        (
            "linear distance outer outer",
            LocationKind::LinearDistanceOuterOuter,
        ),
        (
            "linear distance outer inner",
            LocationKind::LinearDistanceOuterInner,
        ),
        (
            "linear distance inner centre",
            LocationKind::LinearDistanceInnerCentre,
        ),
        (
            "linear distance inner outer",
            LocationKind::LinearDistanceInnerOuter,
        ),
        (
            "linear distance inner inner",
            LocationKind::LinearDistanceInnerInner,
        ),
        ("linear distance centre", LocationKind::LinearDistanceCentre),
        ("linear distance inner", LocationKind::LinearDistanceInner),
        ("linear distance outer", LocationKind::LinearDistanceOuter),
    ];

    #[must_use]
    pub fn from_name(name: &str) -> LocationKind {
        Self::TABLE
            .iter()
            .find(|(n, _)| *n == name)
            .map_or_else(|| LocationKind::Other(name.to_string()), |(_, k)| k.clone())
    }

    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            LocationKind::Other(n) => n,
            k => Self::TABLE
                .iter()
                .find(|(_, t)| t == k)
                .map_or("", |(n, _)| n),
        }
    }
}

/// How a dimension is toleranced (§5.2).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum DimTolerance {
    /// A nominal value only.
    None,
    /// Theoretically exact: basic (ASME), theoretical (ISO) (§5.3 Table 7).
    Basic,
    /// Signed offsets from nominal, `upper > lower` (§5.2.3).
    Deviations(Bounds),
    /// A value range beside the nominal (§5.2.4).
    Limits(Bounds),
    /// An ISO 286 tolerance class (§5.2.5), optionally with the limits stated beside it, as in
    /// Ø20 H7 (20.000/20.021).
    Fit {
        class: Iso286Class,
        limits: Option<Bounds>,
    },
}

/// The tolerance principle of a dimension (§5.2.1 Table 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum Principle {
    Independency,
    #[cfg_attr(feature = "json", serde(rename = "envelope requirement"))]
    EnvelopeRequirement,
}

/// A value qualifier (`type_qualifier.name`, §5.2.2 Table 6).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum Qualifier {
    Maximum,
    Minimum,
    Average,
    /// Another string, as stated: the practice restricts none.
    Other(String),
}

impl Qualifier {
    #[must_use]
    pub fn from_name(name: &str) -> Qualifier {
        match name {
            "maximum" => Qualifier::Maximum,
            "minimum" => Qualifier::Minimum,
            "average" => Qualifier::Average,
            other => Qualifier::Other(other.to_string()),
        }
    }
}

/// A dimension modifier: Table 7's 'auxiliary' and Table 8 (§5.3).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum DimensionModifier {
    /// Reference (ASME) / auxiliary (ISO), Table 7.
    Reference,
    #[cfg_attr(feature = "json", serde(rename = "controlled radius"))]
    ControlledRadius,
    #[cfg_attr(feature = "json", serde(rename = "square"))]
    Square,
    #[cfg_attr(feature = "json", serde(rename = "statistical"))]
    Statistical,
    #[cfg_attr(feature = "json", serde(rename = "continuous feature"))]
    ContinuousFeature,
    #[cfg_attr(feature = "json", serde(rename = "two point size"))]
    TwoPointSize,
    #[cfg_attr(feature = "json", serde(rename = "local size defined by a sphere"))]
    LocalSizeDefinedBySphere,
    #[cfg_attr(feature = "json", serde(rename = "least squares association criteria"))]
    LeastSquaresAssociationCriteria,
    #[cfg_attr(
        feature = "json",
        serde(rename = "maximum inscribed association criteria")
    )]
    MaximumInscribedAssociationCriteria,
    #[cfg_attr(
        feature = "json",
        serde(rename = "minimum circumscribed association criteria")
    )]
    MinimumCircumscribedAssociationCriteria,
    #[cfg_attr(
        feature = "json",
        serde(rename = "circumference diameter calculated size")
    )]
    CircumferenceDiameterCalculatedSize,
    #[cfg_attr(feature = "json", serde(rename = "area diameter calculated size"))]
    AreaDiameterCalculatedSize,
    #[cfg_attr(feature = "json", serde(rename = "volume diameter calculated size"))]
    VolumeDiameterCalculatedSize,
    #[cfg_attr(feature = "json", serde(rename = "maximum rank order size"))]
    MaximumRankOrderSize,
    #[cfg_attr(feature = "json", serde(rename = "minimum rank order size"))]
    MinimumRankOrderSize,
    #[cfg_attr(feature = "json", serde(rename = "average rank order size"))]
    AverageRankOrderSize,
    #[cfg_attr(feature = "json", serde(rename = "median rank order size"))]
    MedianRankOrderSize,
    #[cfg_attr(feature = "json", serde(rename = "mid range rank order size"))]
    MidRangeRankOrderSize,
    #[cfg_attr(feature = "json", serde(rename = "range rank order size"))]
    RangeRankOrderSize,
    #[cfg_attr(feature = "json", serde(rename = "any part of the feature"))]
    AnyPartOfTheFeature,
    #[cfg_attr(feature = "json", serde(rename = "any cross section"))]
    AnyCrossSection,
    #[cfg_attr(feature = "json", serde(rename = "specific fixed cross section"))]
    SpecificFixedCrossSection,
    #[cfg_attr(feature = "json", serde(rename = "common tolerance"))]
    CommonTolerance,
    #[cfg_attr(feature = "json", serde(rename = "free state condition"))]
    FreeStateCondition,
    #[cfg_attr(feature = "json", serde(rename = "united feature of size"))]
    UnitedFeatureOfSize,
}

impl DimensionModifier {
    /// Table 8's strings.
    pub const TABLE: [(&'static str, DimensionModifier); 24] = [
        ("controlled radius", DimensionModifier::ControlledRadius),
        ("square", DimensionModifier::Square),
        ("statistical", DimensionModifier::Statistical),
        ("continuous feature", DimensionModifier::ContinuousFeature),
        ("two point size", DimensionModifier::TwoPointSize),
        (
            "local size defined by a sphere",
            DimensionModifier::LocalSizeDefinedBySphere,
        ),
        (
            "least squares association criteria",
            DimensionModifier::LeastSquaresAssociationCriteria,
        ),
        (
            "maximum inscribed association criteria",
            DimensionModifier::MaximumInscribedAssociationCriteria,
        ),
        (
            "minimum circumscribed association criteria",
            DimensionModifier::MinimumCircumscribedAssociationCriteria,
        ),
        (
            "circumference diameter calculated size",
            DimensionModifier::CircumferenceDiameterCalculatedSize,
        ),
        (
            "area diameter calculated size",
            DimensionModifier::AreaDiameterCalculatedSize,
        ),
        (
            "volume diameter calculated size",
            DimensionModifier::VolumeDiameterCalculatedSize,
        ),
        (
            "maximum rank order size",
            DimensionModifier::MaximumRankOrderSize,
        ),
        (
            "minimum rank order size",
            DimensionModifier::MinimumRankOrderSize,
        ),
        (
            "average rank order size",
            DimensionModifier::AverageRankOrderSize,
        ),
        (
            "median rank order size",
            DimensionModifier::MedianRankOrderSize,
        ),
        (
            "mid range rank order size",
            DimensionModifier::MidRangeRankOrderSize,
        ),
        (
            "range rank order size",
            DimensionModifier::RangeRankOrderSize,
        ),
        (
            "any part of the feature",
            DimensionModifier::AnyPartOfTheFeature,
        ),
        ("any cross section", DimensionModifier::AnyCrossSection),
        (
            "specific fixed cross section",
            DimensionModifier::SpecificFixedCrossSection,
        ),
        ("common tolerance", DimensionModifier::CommonTolerance),
        (
            "free state condition",
            DimensionModifier::FreeStateCondition,
        ),
        (
            "united feature of size",
            DimensionModifier::UnitedFeatureOfSize,
        ),
    ];

    /// The Table 8 modifier a description states.
    #[must_use]
    pub fn from_description(text: &str) -> Option<DimensionModifier> {
        Self::TABLE
            .iter()
            .find(|(n, _)| *n == text)
            .map(|(_, m)| m.clone())
    }
}

/// ISO 286 tolerance class: fundamental deviation and grade, e.g. H7, g6 (§5.2.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct Iso286Class {
    pub deviation: FundamentalDeviation,
    pub grade: ToleranceGrade,
}

impl fmt::Display for Iso286Class {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.deviation, self.grade.number())
    }
}

/// A fundamental deviation: the ISO 286 letter and whether it is a hole's (upper case) or a
/// shaft's (lower case). The 28 letters are the same for both.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct FundamentalDeviation {
    pub letter: DeviationLetter,
    pub of: FitFeature,
}

/// Hole (internal feature, upper case) or shaft (external feature, lower case).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum FitFeature {
    Hole,
    Shaft,
}

/// ISO 286-1 fundamental deviation letters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum DeviationLetter {
    #[cfg_attr(feature = "json", serde(rename = "a"))]
    A,
    #[cfg_attr(feature = "json", serde(rename = "b"))]
    B,
    #[cfg_attr(feature = "json", serde(rename = "c"))]
    C,
    #[cfg_attr(feature = "json", serde(rename = "cd"))]
    Cd,
    #[cfg_attr(feature = "json", serde(rename = "d"))]
    D,
    #[cfg_attr(feature = "json", serde(rename = "e"))]
    E,
    #[cfg_attr(feature = "json", serde(rename = "ef"))]
    Ef,
    #[cfg_attr(feature = "json", serde(rename = "f"))]
    F,
    #[cfg_attr(feature = "json", serde(rename = "fg"))]
    Fg,
    #[cfg_attr(feature = "json", serde(rename = "g"))]
    G,
    #[cfg_attr(feature = "json", serde(rename = "h"))]
    H,
    #[cfg_attr(feature = "json", serde(rename = "js"))]
    Js,
    #[cfg_attr(feature = "json", serde(rename = "j"))]
    J,
    #[cfg_attr(feature = "json", serde(rename = "k"))]
    K,
    #[cfg_attr(feature = "json", serde(rename = "m"))]
    M,
    #[cfg_attr(feature = "json", serde(rename = "n"))]
    N,
    #[cfg_attr(feature = "json", serde(rename = "p"))]
    P,
    #[cfg_attr(feature = "json", serde(rename = "r"))]
    R,
    #[cfg_attr(feature = "json", serde(rename = "s"))]
    S,
    #[cfg_attr(feature = "json", serde(rename = "t"))]
    T,
    #[cfg_attr(feature = "json", serde(rename = "u"))]
    U,
    #[cfg_attr(feature = "json", serde(rename = "v"))]
    V,
    #[cfg_attr(feature = "json", serde(rename = "x"))]
    X,
    #[cfg_attr(feature = "json", serde(rename = "y"))]
    Y,
    #[cfg_attr(feature = "json", serde(rename = "z"))]
    Z,
    #[cfg_attr(feature = "json", serde(rename = "za"))]
    Za,
    #[cfg_attr(feature = "json", serde(rename = "zb"))]
    Zb,
    #[cfg_attr(feature = "json", serde(rename = "zc"))]
    Zc,
}

impl DeviationLetter {
    pub const ALL: [(&'static str, DeviationLetter); 28] = [
        ("A", DeviationLetter::A),
        ("B", DeviationLetter::B),
        ("C", DeviationLetter::C),
        ("CD", DeviationLetter::Cd),
        ("D", DeviationLetter::D),
        ("E", DeviationLetter::E),
        ("EF", DeviationLetter::Ef),
        ("F", DeviationLetter::F),
        ("FG", DeviationLetter::Fg),
        ("G", DeviationLetter::G),
        ("H", DeviationLetter::H),
        ("JS", DeviationLetter::Js),
        ("J", DeviationLetter::J),
        ("K", DeviationLetter::K),
        ("M", DeviationLetter::M),
        ("N", DeviationLetter::N),
        ("P", DeviationLetter::P),
        ("R", DeviationLetter::R),
        ("S", DeviationLetter::S),
        ("T", DeviationLetter::T),
        ("U", DeviationLetter::U),
        ("V", DeviationLetter::V),
        ("X", DeviationLetter::X),
        ("Y", DeviationLetter::Y),
        ("Z", DeviationLetter::Z),
        ("ZA", DeviationLetter::Za),
        ("ZB", DeviationLetter::Zb),
        ("ZC", DeviationLetter::Zc),
    ];
}

impl FundamentalDeviation {
    /// `A`…`ZC` (hole) or `a`…`zc` (shaft); mixed case is refused.
    pub fn parse(text: &str) -> Result<FundamentalDeviation, ModelError> {
        let of = if text.chars().all(|c| c.is_ascii_uppercase()) {
            FitFeature::Hole
        } else if text.chars().all(|c| c.is_ascii_lowercase()) {
            FitFeature::Shaft
        } else {
            return err(format!("{text:?} is not an ISO 286 fundamental deviation"));
        };
        let upper = text.to_ascii_uppercase();
        DeviationLetter::ALL
            .iter()
            .find(|(n, _)| *n == upper)
            .map(|&(_, letter)| FundamentalDeviation { letter, of })
            .ok_or_else(|| ModelError(format!("{text:?} is not an ISO 286 fundamental deviation")))
    }
}

impl fmt::Display for FundamentalDeviation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = DeviationLetter::ALL
            .iter()
            .find(|(_, l)| *l == self.letter)
            .map_or("", |(n, _)| n);
        match self.of {
            FitFeature::Hole => f.write_str(name),
            FitFeature::Shaft => f.write_str(&name.to_ascii_lowercase()),
        }
    }
}

/// ISO 286-1 standard tolerance grades IT01, IT0, IT1 … IT18.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum ToleranceGrade {
    #[cfg_attr(feature = "json", serde(rename = "01"))]
    It01,
    #[cfg_attr(feature = "json", serde(rename = "0"))]
    It0,
    #[cfg_attr(feature = "json", serde(rename = "1"))]
    It1,
    #[cfg_attr(feature = "json", serde(rename = "2"))]
    It2,
    #[cfg_attr(feature = "json", serde(rename = "3"))]
    It3,
    #[cfg_attr(feature = "json", serde(rename = "4"))]
    It4,
    #[cfg_attr(feature = "json", serde(rename = "5"))]
    It5,
    #[cfg_attr(feature = "json", serde(rename = "6"))]
    It6,
    #[cfg_attr(feature = "json", serde(rename = "7"))]
    It7,
    #[cfg_attr(feature = "json", serde(rename = "8"))]
    It8,
    #[cfg_attr(feature = "json", serde(rename = "9"))]
    It9,
    #[cfg_attr(feature = "json", serde(rename = "10"))]
    It10,
    #[cfg_attr(feature = "json", serde(rename = "11"))]
    It11,
    #[cfg_attr(feature = "json", serde(rename = "12"))]
    It12,
    #[cfg_attr(feature = "json", serde(rename = "13"))]
    It13,
    #[cfg_attr(feature = "json", serde(rename = "14"))]
    It14,
    #[cfg_attr(feature = "json", serde(rename = "15"))]
    It15,
    #[cfg_attr(feature = "json", serde(rename = "16"))]
    It16,
    #[cfg_attr(feature = "json", serde(rename = "17"))]
    It17,
    #[cfg_attr(feature = "json", serde(rename = "18"))]
    It18,
}

impl ToleranceGrade {
    const ALL: [(&'static str, ToleranceGrade); 20] = [
        ("01", ToleranceGrade::It01),
        ("0", ToleranceGrade::It0),
        ("1", ToleranceGrade::It1),
        ("2", ToleranceGrade::It2),
        ("3", ToleranceGrade::It3),
        ("4", ToleranceGrade::It4),
        ("5", ToleranceGrade::It5),
        ("6", ToleranceGrade::It6),
        ("7", ToleranceGrade::It7),
        ("8", ToleranceGrade::It8),
        ("9", ToleranceGrade::It9),
        ("10", ToleranceGrade::It10),
        ("11", ToleranceGrade::It11),
        ("12", ToleranceGrade::It12),
        ("13", ToleranceGrade::It13),
        ("14", ToleranceGrade::It14),
        ("15", ToleranceGrade::It15),
        ("16", ToleranceGrade::It16),
        ("17", ToleranceGrade::It17),
        ("18", ToleranceGrade::It18),
    ];

    /// `01`, `0`, `1` … `18`, optionally prefixed `IT`.
    pub fn parse(text: &str) -> Result<ToleranceGrade, ModelError> {
        let n = text.strip_prefix("IT").unwrap_or(text);
        Self::ALL
            .iter()
            .find(|(t, _)| *t == n)
            .map(|&(_, g)| g)
            .ok_or_else(|| ModelError(format!("{text:?} is not an ISO 286 tolerance grade")))
    }

    /// The grade's number as ISO 286 writes it after the letter (`7` for IT7, `01` for IT01).
    #[must_use]
    pub fn number(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(_, g)| *g == self)
            .map_or("", |(t, _)| t)
    }
}

// ---------------------------------------------------------------------------------------------
// Geometric tolerances
// ---------------------------------------------------------------------------------------------

/// A geometric tolerance: one feature control frame (§6.7–§6.9).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct GeometricTolerance {
    pub kind: ToleranceKind,
    pub target: ToleranceTarget,
    /// Schema: OPTIONAL `length_measure_with_unit`.
    pub magnitude: Option<Value>,
    pub zone: Option<Zone>,
    /// Ⓜ Ⓛ Ⓕ Ⓣ Ⓟ … (§6.9.3 Table 14), in the file's order.
    pub modifiers: Vec<ToleranceModifier>,
    /// Unit basis (§6.9.6).
    pub unit_basis: Option<UnitBasis>,
    /// Maximum value (§6.9.5).
    pub maximum: Option<Value>,
    /// Unequally disposed displacement (§6.9.4).
    pub unequal: Option<Value>,
    pub datums: Option<DatumSystem>,
    /// `geometric_tolerance_auxiliary_classification`: all over, unless otherwise specified.
    pub auxiliary: Vec<AuxiliaryClassification>,
    /// Text associated with the tolerance otherwise not covered (`description`, §6.9 note),
    /// decoded; `None` when unset or empty.
    pub description: Option<String>,
}

/// The tolerance types (§6.8 Table 12).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum ToleranceKind {
    #[cfg_attr(feature = "json", serde(rename = "angularity_tolerance"))]
    Angularity,
    #[cfg_attr(feature = "json", serde(rename = "circular_runout_tolerance"))]
    CircularRunout,
    #[cfg_attr(feature = "json", serde(rename = "coaxiality_tolerance"))]
    Coaxiality,
    #[cfg_attr(feature = "json", serde(rename = "concentricity_tolerance"))]
    Concentricity,
    #[cfg_attr(feature = "json", serde(rename = "cylindricity_tolerance"))]
    Cylindricity,
    #[cfg_attr(feature = "json", serde(rename = "flatness_tolerance"))]
    Flatness,
    #[cfg_attr(feature = "json", serde(rename = "line_profile_tolerance"))]
    LineProfile,
    #[cfg_attr(feature = "json", serde(rename = "parallelism_tolerance"))]
    Parallelism,
    #[cfg_attr(feature = "json", serde(rename = "perpendicularity_tolerance"))]
    Perpendicularity,
    #[cfg_attr(feature = "json", serde(rename = "position_tolerance"))]
    Position,
    #[cfg_attr(feature = "json", serde(rename = "roundness_tolerance"))]
    Roundness,
    #[cfg_attr(feature = "json", serde(rename = "straightness_tolerance"))]
    Straightness,
    #[cfg_attr(feature = "json", serde(rename = "surface_profile_tolerance"))]
    SurfaceProfile,
    #[cfg_attr(feature = "json", serde(rename = "symmetry_tolerance"))]
    Symmetry,
    #[cfg_attr(feature = "json", serde(rename = "total_runout_tolerance"))]
    TotalRunout,
}

/// Whether a tolerance kind requires, allows or forbids datum references.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum DatumRequirement {
    Required,
    Optional,
    Forbidden,
}

impl ToleranceKind {
    pub const ALL: [(&'static str, ToleranceKind); 15] = [
        ("angularity_tolerance", ToleranceKind::Angularity),
        ("circular_runout_tolerance", ToleranceKind::CircularRunout),
        ("coaxiality_tolerance", ToleranceKind::Coaxiality),
        ("concentricity_tolerance", ToleranceKind::Concentricity),
        ("cylindricity_tolerance", ToleranceKind::Cylindricity),
        ("flatness_tolerance", ToleranceKind::Flatness),
        ("line_profile_tolerance", ToleranceKind::LineProfile),
        ("parallelism_tolerance", ToleranceKind::Parallelism),
        (
            "perpendicularity_tolerance",
            ToleranceKind::Perpendicularity,
        ),
        ("position_tolerance", ToleranceKind::Position),
        ("roundness_tolerance", ToleranceKind::Roundness),
        ("straightness_tolerance", ToleranceKind::Straightness),
        ("surface_profile_tolerance", ToleranceKind::SurfaceProfile),
        ("symmetry_tolerance", ToleranceKind::Symmetry),
        ("total_runout_tolerance", ToleranceKind::TotalRunout),
    ];

    /// The kind of an entity type name (lower case).
    #[must_use]
    pub fn of_entity(name: &str) -> Option<ToleranceKind> {
        Self::ALL.iter().find(|(n, _)| *n == name).map(|&(_, k)| k)
    }

    /// The entity type of the kind.
    #[must_use]
    pub fn entity(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(_, k)| *k == self)
            .map_or("", |(n, _)| n)
    }

    /// Datum references by kind, from the schema's subtype structure: the subtypes of
    /// `geometric_tolerance_with_datum_reference` require them; straightness, flatness,
    /// roundness and cylindricity forbid them (their WR1); position and the profiles take them
    /// optionally, by the complex form.
    #[must_use]
    pub fn datums(self) -> DatumRequirement {
        use ToleranceKind as K;
        match self {
            K::Angularity
            | K::Perpendicularity
            | K::Parallelism
            | K::CircularRunout
            | K::TotalRunout
            | K::Symmetry
            | K::Concentricity
            | K::Coaxiality => DatumRequirement::Required,
            K::Position | K::LineProfile | K::SurfaceProfile => DatumRequirement::Optional,
            K::Straightness | K::Flatness | K::Roundness | K::Cylindricity => {
                DatumRequirement::Forbidden
            }
        }
    }
}

/// What a geometric tolerance applies to (schema `geometric_tolerance_target`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum ToleranceTarget {
    /// A shape aspect (§6.1).
    Feature(FeatureId),
    /// A dimensional size or location: a feature of size (§6.2).
    Dimension(DimensionId),
    /// A `shape_aspect_relationship` that is not a dimensional location, with its name.
    Relation {
        relating: FeatureId,
        related: FeatureId,
        name: String,
    },
    /// The `product_definition_shape`: the whole part, "applies to all surfaces" (§6.3).
    WholePart,
}

/// The tolerance zone (§6.9.2).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct Zone {
    /// `tolerance_zone_form.name` (Table 13).
    pub form: ZoneForm,
    /// Projected tolerance zone (§6.9.2.2).
    pub projected: Option<ProjectedZone>,
    /// Non-uniform zone and its boundaries (§6.9.2.3).
    pub non_uniform: Option<Vec<FeatureId>>,
    /// Runout zone orientation angle (`runout_zone_definition`, Figure 51).
    pub runout_angle: Option<Value>,
    /// The affected plane feature (§6.9.2.1).
    pub affected_plane: Option<FeatureId>,
    /// Explicit variant; unspecified legacy planes are read but refused when writing.
    pub affected_plane_kind: Option<AffectedPlaneKind>,
}

/// Affected plane variants in §6.9.2.1 Figures 53–54.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum AffectedPlaneKind {
    IntersectionLine,
    IntersectionPlane,
    OrientationPlane,
}
impl AffectedPlaneKind {
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::IntersectionLine => "representative line element",
            Self::IntersectionPlane => "representative plane element",
            Self::OrientationPlane => "",
        }
    }
}

/// `projected_zone_definition`: the projection's origin feature and length.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct ProjectedZone {
    /// The feature the projection starts from; `None` where a file's projection end names no
    /// geometry (read through, reported).
    pub end: Option<FeatureId>,
    pub length: Value,
}

/// `tolerance_zone_form.name` (§6.9.2 Table 13).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum ZoneForm {
    #[cfg_attr(feature = "json", serde(rename = "within a circle"))]
    WithinACircle,
    #[cfg_attr(feature = "json", serde(rename = "within a cylinder"))]
    WithinACylinder,
    #[cfg_attr(feature = "json", serde(rename = "cylindrical or circular"))]
    CylindricalOrCircular,
    #[cfg_attr(feature = "json", serde(rename = "spherical"))]
    Spherical,
    #[cfg_attr(feature = "json", serde(rename = "within a sphere"))]
    WithinASphere,
    #[cfg_attr(feature = "json", serde(rename = "between two concentric circles"))]
    BetweenTwoConcentricCircles,
    #[cfg_attr(feature = "json", serde(rename = "between two equidistant curves"))]
    BetweenTwoEquidistantCurves,
    #[cfg_attr(feature = "json", serde(rename = "between two coaxial cylinders"))]
    BetweenTwoCoaxialCylinders,
    #[cfg_attr(feature = "json", serde(rename = "between two equidistant surfaces"))]
    BetweenTwoEquidistantSurfaces,
    #[cfg_attr(feature = "json", serde(rename = "non uniform"))]
    NonUniform,
    #[cfg_attr(
        feature = "json",
        serde(rename = "between two parallel circles on a conical surface")
    )]
    BetweenTwoParallelCirclesOnAConicalSurface,
    #[cfg_attr(
        feature = "json",
        serde(rename = "between two parallel circles of the same diameter")
    )]
    BetweenTwoParallelCirclesOfTheSameDiameter,
    #[cfg_attr(
        feature = "json",
        serde(rename = "between two equidistant complex lines or two parallel straight lines")
    )]
    BetweenTwoEquidistantComplexLinesOrTwoParallelStraightLines,
    #[cfg_attr(
        feature = "json",
        serde(
            rename = "between two non-equidistant complex lines or two non-parallel straight lines"
        )
    )]
    BetweenTwoNonEquidistantComplexLinesOrTwoNonParallelStraightLines,
    #[cfg_attr(feature = "json", serde(rename = "within a cone"))]
    WithinACone,
    #[cfg_attr(feature = "json", serde(rename = "within a single complex surface"))]
    WithinASingleComplexSurface,
    #[cfg_attr(
        feature = "json",
        serde(rename = "between two equidistant complex surfaces or two parallel planes")
    )]
    BetweenTwoEquidistantComplexSurfacesOrTwoParallelPlanes,
    #[cfg_attr(
        feature = "json",
        serde(rename = "between two non-equidistant complex surfaces or two non-parallel planes")
    )]
    BetweenTwoNonEquidistantComplexSurfacesOrTwoNonParallelPlanes,
    #[cfg_attr(feature = "json", serde(rename = "unknown"))]
    Unknown,
    /// Another string or '' (the practice allows them), as stated.
    Other(String),
}

impl ZoneForm {
    pub const TABLE: [(&'static str, ZoneForm); 19] = [
        ("within a circle", ZoneForm::WithinACircle),
        ("within a cylinder", ZoneForm::WithinACylinder),
        ("cylindrical or circular", ZoneForm::CylindricalOrCircular),
        ("spherical", ZoneForm::Spherical),
        ("within a sphere", ZoneForm::WithinASphere),
        (
            "between two concentric circles",
            ZoneForm::BetweenTwoConcentricCircles,
        ),
        (
            "between two equidistant curves",
            ZoneForm::BetweenTwoEquidistantCurves,
        ),
        (
            "between two coaxial cylinders",
            ZoneForm::BetweenTwoCoaxialCylinders,
        ),
        (
            "between two equidistant surfaces",
            ZoneForm::BetweenTwoEquidistantSurfaces,
        ),
        ("non uniform", ZoneForm::NonUniform),
        (
            "between two parallel circles on a conical surface",
            ZoneForm::BetweenTwoParallelCirclesOnAConicalSurface,
        ),
        (
            "between two parallel circles of the same diameter",
            ZoneForm::BetweenTwoParallelCirclesOfTheSameDiameter,
        ),
        (
            "between two equidistant complex lines or two parallel straight lines",
            ZoneForm::BetweenTwoEquidistantComplexLinesOrTwoParallelStraightLines,
        ),
        (
            "between two non-equidistant complex lines or two non-parallel straight lines",
            ZoneForm::BetweenTwoNonEquidistantComplexLinesOrTwoNonParallelStraightLines,
        ),
        ("within a cone", ZoneForm::WithinACone),
        (
            "within a single complex surface",
            ZoneForm::WithinASingleComplexSurface,
        ),
        (
            "between two equidistant complex surfaces or two parallel planes",
            ZoneForm::BetweenTwoEquidistantComplexSurfacesOrTwoParallelPlanes,
        ),
        (
            "between two non-equidistant complex surfaces or two non-parallel planes",
            ZoneForm::BetweenTwoNonEquidistantComplexSurfacesOrTwoNonParallelPlanes,
        ),
        ("unknown", ZoneForm::Unknown),
    ];

    #[must_use]
    pub fn from_name(name: &str) -> ZoneForm {
        Self::TABLE
            .iter()
            .find(|(n, _)| *n == name)
            .map_or_else(|| ZoneForm::Other(name.to_string()), |(_, k)| k.clone())
    }

    /// Whether the form is diametral (Ø, Table 13).
    #[must_use]
    pub fn is_diametral(&self) -> bool {
        matches!(
            self,
            ZoneForm::WithinACircle | ZoneForm::WithinACylinder | ZoneForm::CylindricalOrCircular
        )
    }

    /// Whether the form is spherical (SØ, Table 13).
    #[must_use]
    pub fn is_spherical(&self) -> bool {
        matches!(self, ZoneForm::Spherical | ZoneForm::WithinASphere)
    }
}

/// Schema `geometric_tolerance_modifier` (§6.9.3 Table 14), edition 4's values (edition 1's
/// are a subset).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum ToleranceModifier {
    #[cfg_attr(feature = "json", serde(rename = "any_cross_section"))]
    AnyCrossSection,
    #[cfg_attr(feature = "json", serde(rename = "associated_least_square_feature"))]
    AssociatedLeastSquareFeature,
    #[cfg_attr(
        feature = "json",
        serde(rename = "associated_maximum_inscribed_feature")
    )]
    AssociatedMaximumInscribedFeature,
    #[cfg_attr(
        feature = "json",
        serde(rename = "associated_minimum_inscribed_feature")
    )]
    AssociatedMinimumInscribedFeature,
    #[cfg_attr(feature = "json", serde(rename = "associated_minmax_feature"))]
    AssociatedMinmaxFeature,
    #[cfg_attr(feature = "json", serde(rename = "associated_tangent_feature"))]
    AssociatedTangentFeature,
    #[cfg_attr(feature = "json", serde(rename = "circle_a"))]
    CircleA,
    #[cfg_attr(feature = "json", serde(rename = "common_zone"))]
    CommonZone,
    #[cfg_attr(feature = "json", serde(rename = "continuous_features"))]
    ContinuousFeatures,
    #[cfg_attr(feature = "json", serde(rename = "derived_feature"))]
    DerivedFeature,
    #[cfg_attr(feature = "json", serde(rename = "each_element"))]
    EachElement,
    #[cfg_attr(feature = "json", serde(rename = "each_radial_element"))]
    EachRadialElement,
    #[cfg_attr(feature = "json", serde(rename = "free_state"))]
    FreeState,
    #[cfg_attr(feature = "json", serde(rename = "individually"))]
    Individually,
    #[cfg_attr(feature = "json", serde(rename = "least_material_requirement"))]
    LeastMaterialRequirement,
    #[cfg_attr(feature = "json", serde(rename = "line_element"))]
    LineElement,
    #[cfg_attr(feature = "json", serde(rename = "major_diameter"))]
    MajorDiameter,
    #[cfg_attr(feature = "json", serde(rename = "maximum_material_requirement"))]
    MaximumMaterialRequirement,
    #[cfg_attr(feature = "json", serde(rename = "minor_diameter"))]
    MinorDiameter,
    #[cfg_attr(feature = "json", serde(rename = "not_convex"))]
    NotConvex,
    #[cfg_attr(feature = "json", serde(rename = "offset_zone"))]
    OffsetZone,
    #[cfg_attr(feature = "json", serde(rename = "peak_height"))]
    PeakHeight,
    #[cfg_attr(feature = "json", serde(rename = "pitch_diameter"))]
    PitchDiameter,
    #[cfg_attr(feature = "json", serde(rename = "reciprocity_requirement"))]
    ReciprocityRequirement,
    #[cfg_attr(
        feature = "json",
        serde(rename = "reference_least_square_feature_with_external_material_constraint")
    )]
    ReferenceLeastSquareFeatureWithExternalMaterialConstraint,
    #[cfg_attr(
        feature = "json",
        serde(rename = "reference_least_square_feature_with_internal_material_constraint")
    )]
    ReferenceLeastSquareFeatureWithInternalMaterialConstraint,
    #[cfg_attr(
        feature = "json",
        serde(rename = "reference_least_square_feature_without_constraint")
    )]
    ReferenceLeastSquareFeatureWithoutConstraint,
    #[cfg_attr(
        feature = "json",
        serde(rename = "reference_maximum_inscribed_feature")
    )]
    ReferenceMaximumInscribedFeature,
    #[cfg_attr(
        feature = "json",
        serde(rename = "reference_minimax_feature_with_external_material_constraint")
    )]
    ReferenceMinimaxFeatureWithExternalMaterialConstraint,
    #[cfg_attr(
        feature = "json",
        serde(rename = "reference_minimax_feature_with_internal_material_constraint")
    )]
    ReferenceMinimaxFeatureWithInternalMaterialConstraint,
    #[cfg_attr(
        feature = "json",
        serde(rename = "reference_minimax_feature_without_constraint")
    )]
    ReferenceMinimaxFeatureWithoutConstraint,
    #[cfg_attr(
        feature = "json",
        serde(rename = "reference_minimum_circumscribed_feature")
    )]
    ReferenceMinimumCircumscribedFeature,
    #[cfg_attr(feature = "json", serde(rename = "separate_requirement"))]
    SeparateRequirement,
    #[cfg_attr(feature = "json", serde(rename = "separate_zones"))]
    SeparateZones,
    #[cfg_attr(feature = "json", serde(rename = "standard_deviation"))]
    StandardDeviation,
    #[cfg_attr(feature = "json", serde(rename = "statistical_tolerance"))]
    StatisticalTolerance,
    #[cfg_attr(feature = "json", serde(rename = "stock"))]
    Stock,
    #[cfg_attr(feature = "json", serde(rename = "tangent_plane"))]
    TangentPlane,
    #[cfg_attr(feature = "json", serde(rename = "total_range_deviations"))]
    TotalRangeDeviations,
    #[cfg_attr(feature = "json", serde(rename = "united_feature"))]
    UnitedFeature,
    #[cfg_attr(
        feature = "json",
        serde(rename = "unspecified_angular_tolerance_zone_offset")
    )]
    UnspecifiedAngularToleranceZoneOffset,
    #[cfg_attr(
        feature = "json",
        serde(rename = "unspecified_linear_tolerance_zone_offset")
    )]
    UnspecifiedLinearToleranceZoneOffset,
    #[cfg_attr(feature = "json", serde(rename = "valley_depth"))]
    ValleyDepth,
    #[cfg_attr(feature = "json", serde(rename = "variable_angle"))]
    VariableAngle,
}

impl ToleranceModifier {
    pub const ALL: [(&'static str, ToleranceModifier); 44] = [
        ("ANY_CROSS_SECTION", ToleranceModifier::AnyCrossSection),
        (
            "ASSOCIATED_LEAST_SQUARE_FEATURE",
            ToleranceModifier::AssociatedLeastSquareFeature,
        ),
        (
            "ASSOCIATED_MAXIMUM_INSCRIBED_FEATURE",
            ToleranceModifier::AssociatedMaximumInscribedFeature,
        ),
        (
            "ASSOCIATED_MINIMUM_INSCRIBED_FEATURE",
            ToleranceModifier::AssociatedMinimumInscribedFeature,
        ),
        (
            "ASSOCIATED_MINMAX_FEATURE",
            ToleranceModifier::AssociatedMinmaxFeature,
        ),
        (
            "ASSOCIATED_TANGENT_FEATURE",
            ToleranceModifier::AssociatedTangentFeature,
        ),
        ("CIRCLE_A", ToleranceModifier::CircleA),
        ("COMMON_ZONE", ToleranceModifier::CommonZone),
        ("CONTINUOUS_FEATURES", ToleranceModifier::ContinuousFeatures),
        ("DERIVED_FEATURE", ToleranceModifier::DerivedFeature),
        ("EACH_ELEMENT", ToleranceModifier::EachElement),
        ("EACH_RADIAL_ELEMENT", ToleranceModifier::EachRadialElement),
        ("FREE_STATE", ToleranceModifier::FreeState),
        ("INDIVIDUALLY", ToleranceModifier::Individually),
        (
            "LEAST_MATERIAL_REQUIREMENT",
            ToleranceModifier::LeastMaterialRequirement,
        ),
        ("LINE_ELEMENT", ToleranceModifier::LineElement),
        ("MAJOR_DIAMETER", ToleranceModifier::MajorDiameter),
        (
            "MAXIMUM_MATERIAL_REQUIREMENT",
            ToleranceModifier::MaximumMaterialRequirement,
        ),
        ("MINOR_DIAMETER", ToleranceModifier::MinorDiameter),
        ("NOT_CONVEX", ToleranceModifier::NotConvex),
        ("OFFSET_ZONE", ToleranceModifier::OffsetZone),
        ("PEAK_HEIGHT", ToleranceModifier::PeakHeight),
        ("PITCH_DIAMETER", ToleranceModifier::PitchDiameter),
        (
            "RECIPROCITY_REQUIREMENT",
            ToleranceModifier::ReciprocityRequirement,
        ),
        (
            "REFERENCE_LEAST_SQUARE_FEATURE_WITH_EXTERNAL_MATERIAL_CONSTRAINT",
            ToleranceModifier::ReferenceLeastSquareFeatureWithExternalMaterialConstraint,
        ),
        (
            "REFERENCE_LEAST_SQUARE_FEATURE_WITH_INTERNAL_MATERIAL_CONSTRAINT",
            ToleranceModifier::ReferenceLeastSquareFeatureWithInternalMaterialConstraint,
        ),
        (
            "REFERENCE_LEAST_SQUARE_FEATURE_WITHOUT_CONSTRAINT",
            ToleranceModifier::ReferenceLeastSquareFeatureWithoutConstraint,
        ),
        (
            "REFERENCE_MAXIMUM_INSCRIBED_FEATURE",
            ToleranceModifier::ReferenceMaximumInscribedFeature,
        ),
        (
            "REFERENCE_MINIMAX_FEATURE_WITH_EXTERNAL_MATERIAL_CONSTRAINT",
            ToleranceModifier::ReferenceMinimaxFeatureWithExternalMaterialConstraint,
        ),
        (
            "REFERENCE_MINIMAX_FEATURE_WITH_INTERNAL_MATERIAL_CONSTRAINT",
            ToleranceModifier::ReferenceMinimaxFeatureWithInternalMaterialConstraint,
        ),
        (
            "REFERENCE_MINIMAX_FEATURE_WITHOUT_CONSTRAINT",
            ToleranceModifier::ReferenceMinimaxFeatureWithoutConstraint,
        ),
        (
            "REFERENCE_MINIMUM_CIRCUMSCRIBED_FEATURE",
            ToleranceModifier::ReferenceMinimumCircumscribedFeature,
        ),
        (
            "SEPARATE_REQUIREMENT",
            ToleranceModifier::SeparateRequirement,
        ),
        ("SEPARATE_ZONES", ToleranceModifier::SeparateZones),
        ("STANDARD_DEVIATION", ToleranceModifier::StandardDeviation),
        (
            "STATISTICAL_TOLERANCE",
            ToleranceModifier::StatisticalTolerance,
        ),
        ("STOCK", ToleranceModifier::Stock),
        ("TANGENT_PLANE", ToleranceModifier::TangentPlane),
        (
            "TOTAL_RANGE_DEVIATIONS",
            ToleranceModifier::TotalRangeDeviations,
        ),
        ("UNITED_FEATURE", ToleranceModifier::UnitedFeature),
        (
            "UNSPECIFIED_ANGULAR_TOLERANCE_ZONE_OFFSET",
            ToleranceModifier::UnspecifiedAngularToleranceZoneOffset,
        ),
        (
            "UNSPECIFIED_LINEAR_TOLERANCE_ZONE_OFFSET",
            ToleranceModifier::UnspecifiedLinearToleranceZoneOffset,
        ),
        ("VALLEY_DEPTH", ToleranceModifier::ValleyDepth),
        ("VARIABLE_ANGLE", ToleranceModifier::VariableAngle),
    ];

    /// The modifier an enumeration value (without dots, any case) names.
    #[must_use]
    pub fn from_enum(value: &str) -> Option<ToleranceModifier> {
        let v = value.to_ascii_uppercase();
        Self::ALL.iter().find(|(n, _)| *n == v).map(|&(_, m)| m)
    }

    /// The enumeration value (upper case, no dots).
    #[must_use]
    pub fn as_enum(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(_, m)| *m == self)
            .map_or("", |(n, _)| n)
    }
}

impl SimpleDatumModifier {
    pub const ALL: [(&'static str, SimpleDatumModifier); 22] = [
        ("FREE_STATE", SimpleDatumModifier::FreeState),
        ("BASIC", SimpleDatumModifier::Basic),
        ("TRANSLATION", SimpleDatumModifier::Translation),
        (
            "LEAST_MATERIAL_REQUIREMENT",
            SimpleDatumModifier::LeastMaterialRequirement,
        ),
        (
            "MAXIMUM_MATERIAL_REQUIREMENT",
            SimpleDatumModifier::MaximumMaterialRequirement,
        ),
        ("POINT", SimpleDatumModifier::Point),
        ("LINE", SimpleDatumModifier::Line),
        ("PLANE", SimpleDatumModifier::Plane),
        ("ORIENTATION", SimpleDatumModifier::Orientation),
        ("ANY_CROSS_SECTION", SimpleDatumModifier::AnyCrossSection),
        (
            "ANY_LONGITUDINAL_SECTION",
            SimpleDatumModifier::AnyLongitudinalSection,
        ),
        ("CONTACTING_FEATURE", SimpleDatumModifier::ContactingFeature),
        ("DISTANCE_VARIABLE", SimpleDatumModifier::DistanceVariable),
        (
            "DEGREE_OF_FREEDOM_CONSTRAINT_X",
            SimpleDatumModifier::DegreeOfFreedomConstraintX,
        ),
        (
            "DEGREE_OF_FREEDOM_CONSTRAINT_Y",
            SimpleDatumModifier::DegreeOfFreedomConstraintY,
        ),
        (
            "DEGREE_OF_FREEDOM_CONSTRAINT_Z",
            SimpleDatumModifier::DegreeOfFreedomConstraintZ,
        ),
        (
            "DEGREE_OF_FREEDOM_CONSTRAINT_U",
            SimpleDatumModifier::DegreeOfFreedomConstraintU,
        ),
        (
            "DEGREE_OF_FREEDOM_CONSTRAINT_V",
            SimpleDatumModifier::DegreeOfFreedomConstraintV,
        ),
        (
            "DEGREE_OF_FREEDOM_CONSTRAINT_W",
            SimpleDatumModifier::DegreeOfFreedomConstraintW,
        ),
        ("MINOR_DIAMETER", SimpleDatumModifier::MinorDiameter),
        ("MAJOR_DIAMETER", SimpleDatumModifier::MajorDiameter),
        ("PITCH_DIAMETER", SimpleDatumModifier::PitchDiameter),
    ];

    #[must_use]
    pub fn from_enum(value: &str) -> Option<SimpleDatumModifier> {
        let v = value.to_ascii_uppercase();
        Self::ALL.iter().find(|(n, _)| *n == v).map(|&(_, m)| m)
    }
}

impl DatumModifierType {
    #[must_use]
    pub fn from_enum(value: &str) -> Option<DatumModifierType> {
        Some(match value.to_ascii_uppercase().as_str() {
            "CIRCULAR_OR_CYLINDRICAL" => DatumModifierType::CircularOrCylindrical,
            "SPHERICAL" => DatumModifierType::Spherical,
            "DISTANCE" => DatumModifierType::Distance,
            "PROJECTED" => DatumModifierType::Projected,
            _ => return None,
        })
    }
}

/// Unit basis (§6.9.6): per unit length, or per unit area of a stated shape.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct UnitBasis {
    pub size: Value,
    pub area: Option<UnitArea>,
}

/// `geometric_tolerance_with_defined_area_unit`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct UnitArea {
    pub shape: AreaShape,
    pub second: Option<Value>,
}

/// Schema `area_unit_type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum AreaShape {
    Circular,
    Square,
    Rectangular,
    Cylindrical,
    Spherical,
}

/// Schema `geometric_tolerance_auxiliary_classification_enum`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum AuxiliaryClassification {
    AllOver,
    UnlessOtherwiseSpecified,
}

/// A relationship between two tolerances (schema `geometric_tolerance_relationship`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct ToleranceRelation {
    pub kind: RelationKind,
    /// The upper frame of a composite.
    pub relating: ToleranceId,
    /// The lower frame of a composite.
    pub related: ToleranceId,
}

/// `geometric_tolerance_relationship.name` (§6.9.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum RelationKind {
    Composite,
    Precedence,
    Simultaneity,
}

// ---------------------------------------------------------------------------------------------
// Part-level information
// ---------------------------------------------------------------------------------------------

/// The dimensioning standard the part is toleranced to (§4 Figure 1).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct Standard {
    /// The standard's document (`document.id`, else the `product.id` it is equivalent to),
    /// e.g. 'ASME Y14.5', 'ISO 1101'.
    pub document: String,
    /// The edition (`product_definition_formation.id`), e.g. 'ASME Y14.5-2009', when stated.
    pub edition: Option<String>,
    pub body: StandardBody,
}

/// Whose standard: the default tolerance principle follows from it (§4 note, Table 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum StandardBody {
    Asme,
    Iso,
    Other,
}

impl Standard {
    /// The principle the standard assumes unless stated: envelope for ASME, independence for
    /// ISO (§4 note).
    #[must_use]
    pub fn default_principle(&self) -> Option<Principle> {
        match self.body {
            StandardBody::Asme => Some(Principle::EnvelopeRequirement),
            StandardBody::Iso => Some(Principle::Independency),
            StandardBody::Other => None,
        }
    }
}

/// A general (default) tolerance (maintainer decision 3).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum GeneralTolerance {
    /// A tolerance class as stated ('tolerance class' item of the 'default tolerances'
    /// representation), and the ISO 2768 classes recognised in it.
    Class {
        text: String,
        standard: Option<crate::pmi::standards::Iso2768>,
    },
    /// A `default_tolerance_table` (§4.1), with its cells as stated.
    Table {
        name: String,
        cells: Vec<ToleranceCell>,
    },
}

/// One `default_tolerance_table_cell`: its items by name, as stated.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct ToleranceCell {
    pub items: Vec<(String, CellValue)>,
}

impl ToleranceCell {
    /// A Figure 3 decimal-only default cell (no range or tolerance is invented).
    pub(crate) fn decimal_default(&self) -> Option<u8> {
        let [(name, CellValue::Count(value))] = self.items.as_slice() else {
            return None;
        };
        let v = value.to_f64();
        (name == "number of decimal places"
            && v.is_finite()
            && v.fract() == 0.
            && (0. ..=255.).contains(&v))
        .then_some(v as u8)
    }
}

/// The value of a table cell item.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum CellValue {
    Value(Value),
    Count(Decimal),
    Text(String),
}

/// A thread, exactly its schema parameters (`thread` WR1–WR16).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct Thread {
    /// 'applied shape' (WR13).
    pub feature: FeatureId,
    /// 'partial area occurrence' (WR12).
    pub partial_area: Option<FeatureId>,
    /// 'thread side' (WR10).
    pub side: ThreadSide,
    /// 'major diameter' (WR2).
    pub major_diameter: Length,
    /// 'minor diameter' (WR3).
    pub minor_diameter: Option<Length>,
    /// 'pitch diameter' (WR4).
    pub pitch_diameter: Option<Length>,
    /// 'number of threads', a `ratio_measure_with_unit` (WR5). What it counts is not settled:
    /// ISO 10303-242's definition text is not available here, its EXPRESS states only the
    /// name and the type, and no file at hand carries a `thread` (none of the 17 NIST AP242
    /// files, none of specify-core's). A ratio suggests threads per unit length rather than a
    /// count of starts, but that is an inference, so the value is kept as stated and nothing
    /// (no pitch) is derived from it.
    pub number_of_threads: Ratio,
    /// 'form' (WR7).
    pub form: String,
    /// 'fit class' (WR6).
    pub fit_class: String,
    /// 'fit class 2' (WR14).
    pub fit_class_2: Option<String>,
    /// 'hand' (WR8).
    pub hand: Hand,
    /// 'crest' (WR11).
    pub crest: Option<Length>,
    /// 'qualifier' (WR9).
    pub qualifier: Option<String>,
    /// 'nominal size' (WR15).
    pub nominal_size: Option<Length>,
    /// 'thread runout' (WR16).
    pub runout: Option<FeatureId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum ThreadSide {
    Internal,
    External,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum Hand {
    Left,
    Right,
}

/// A turned knurl, exactly its schema parameters (`turned_knurl` WR1–WR12).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct Knurl {
    pub feature: FeatureId,
    /// `description`: diamond, diagonal, straight (WR1).
    pub pattern: KnurlPattern,
    pub major_diameter: Length,
    pub nominal_diameter: Length,
    pub diametral_pitch: Length,
    pub number_of_teeth: Option<Count>,
    pub tooth_depth: Option<Length>,
    pub root_fillet: Option<Length>,
    /// Required for diamond and diagonal (WR9).
    pub helix_angle: Option<Angle>,
    /// Required for diagonal (WR10).
    pub helix_hand: Option<Hand>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum KnurlPattern {
    Diamond,
    Diagonal,
    Straight,
}

/// A hole occurrence's feature definition and placement (AP242 edition 4: a
/// `basic_round_hole_occurrence` of a `basic_round_hole`, whose faces are [`Hole::feature`]).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct Hole {
    /// The occurrence: its faces.
    pub feature: FeatureId,
    pub definition: RoundHole,
    /// The occurrence's placement: the target its `mapped_item` maps the definition's
    /// placement (the mapping origin) onto, in the length unit of the representation the
    /// occurrence identifies it in. `None` when the occurrence states no mapped item.
    pub placement: Option<Placement>,
}

/// A `basic_round_hole`, exactly its schema attributes (WR1–WR8). A definition is shared by
/// its occurrences; each [`Hole`] holds it by value.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct RoundHole {
    /// The definition's name (`characterized_object.name`: 'Hole.2').
    pub name: String,
    pub diameter: Length,
    /// Deviations from the diameter (a `tolerance_value`).
    pub diameter_tolerance: Option<Bounds>,
    /// Required unless through (WR7).
    pub depth: Option<Length>,
    pub depth_tolerance: Option<Bounds>,
    pub through: bool,
    /// The definition's own placement (its `shape_representation`'s one `axis2_placement_3d`,
    /// WR1–WR2), in that representation's length unit.
    pub placement: Placement,
}

/// The part's material (CAx-IF *Material Identification and Density* §4.1–4.2).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct Material {
    /// The material identification (`descriptive_representation_item.name`, §4.1:
    /// 'AMS4928'); specify-core's files state the material's name here.
    pub id: String,
    /// The material name (`descriptive_representation_item.description`, §4.1: 'Titanium
    /// 6-4'); `None` when empty.
    pub name: Option<String>,
    /// The density when stated (§4.2), with its own unit.
    pub density: Option<Density>,
}

/// A density as stated: the value and its derived unit's elements (unit and exponent).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct Density {
    pub value: Decimal,
    /// The unit, as `(unit name, exponent)` pairs of the `derived_unit`, e.g.
    /// `[("gram", 1), ("centimetre", -3)]`.
    pub unit: Vec<(String, Decimal)>,
}

/// A note: descriptive text of a requirement, on the part or on a feature.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct Note {
    /// What the note is (the property's name, e.g. 'semantic text' for editable text, §7.4,
    /// or 'manufacturing requirement' with its kind).
    pub kind: String,
    /// The text, decoded.
    pub text: String,
    pub on: Option<NoteOwner>,
    /// The callouts that present it (§7.3), in the order of their instances. Read only:
    /// `pmi::write` migrates the requirement to a standard form and removes replaced presentation.
    pub callouts: Vec<Callout>,
}

/// A presentation of a note: a `draughting_callout` (or an annotation occurrence) that a
/// `draughting_model_item_association` relates to the note (PMI practice §7.3, the link from
/// semantic PMI to its presentation): to the note's property definition, or to a shape aspect
/// that is the note's alone (the aspect it is on, which nothing refers to but its usages, its
/// presentation and the note: specify-core's notes on faces).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct Callout {
    /// The callout's name (e.g. 'External thread requirement'), decoded.
    pub name: String,
    /// The features of the part whose shape aspects an association relates the callout to,
    /// ascending: the faces it is attached to. Empty when it is related only to the note's
    /// property definition.
    pub features: Vec<FeatureId>,
}

/// What a note or attribute set is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum NoteOwner {
    Feature(FeatureId),
    Dimension(DimensionId),
    Tolerance(ToleranceId),
    DatumTarget(DatumTargetId),
}

/// A surface texture requirement (ISO 1302; ASME Y14.36), in AP242's surface conditions
/// (ISO 10303-1110 `Surface_texture` with its `Standard_surface_texture_parameter`s; the PMI
/// practice has no section on it). Part notes in words (coating, heat treatment, edges) are
/// not this: they are 'semantic text' attribute sets (PMI practice §7.4).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct SurfaceTexture {
    /// The faces it applies to; `None` for the part as a whole (a default for its surfaces).
    pub on: Option<FeatureId>,
    /// Whether material removal is allowed, required or not allowed (ISO 1302's symbol).
    pub material_removal: MaterialRemoval,
    /// Its parameters, in order; at least one.
    pub parameters: Vec<SurfaceTextureParameter>,
}

/// ISO 1302's material removal condition (ISO 10303-1110
/// `surface_texture_material_removal_condition_enumeration`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum MaterialRemoval {
    /// 'any process allowed': the basic symbol.
    #[cfg_attr(feature = "json", serde(rename = "any process allowed"))]
    AnyProcessAllowed,
    /// 'material removal required'.
    #[cfg_attr(feature = "json", serde(rename = "material removal required"))]
    Required,
    /// 'no material removal'.
    #[cfg_attr(feature = "json", serde(rename = "no material removal"))]
    NotAllowed,
}

impl MaterialRemoval {
    /// The term the file states (ISO 10303-1110's mapping of `material_removal_condition`).
    #[must_use]
    pub fn term(self) -> &'static str {
        match self {
            MaterialRemoval::AnyProcessAllowed => "any process allowed",
            MaterialRemoval::Required => "material removal required",
            MaterialRemoval::NotAllowed => "no material removal",
        }
    }

    #[must_use]
    pub fn from_term(t: &str) -> Option<MaterialRemoval> {
        [
            MaterialRemoval::AnyProcessAllowed,
            MaterialRemoval::Required,
            MaterialRemoval::NotAllowed,
        ]
        .into_iter()
        .find(|m| m.term() == t)
    }
}

/// One surface texture parameter: its characteristic and the value required of it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct SurfaceTextureParameter {
    /// The characteristic as ISO 1302 names it ('Ra', 'Rz', 'Wt', …; ISO 4287, ISO 12085,
    /// ISO 13565), as stated.
    pub characteristic: String,
    /// The value with its own unit (a length; e.g. 3.2 µm).
    pub value: Length,
}

/// A user defined attribute set (UDA practice §5–7): the attribute's name and its values.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct AttributeSet {
    /// The attribute's name (`property_definition.name`).
    pub name: String,
    /// What it is on: `None` for the part.
    pub on: Option<NoteOwner>,
    /// The values by item name, in the representation's order.
    pub items: Vec<(String, AttributeValue)>,
    /// The property definition's description (specify-core's structured requirements:
    /// 'pmi-assist'); `None` when unset or empty. Read only: `pmi::write` writes 'user defined
    /// attribute', and its comparison does not carry this.
    pub description: Option<String>,
    /// The `general_property` it is associated with (UDA practice §5,
    /// `general_property_association`). `None` when none is stated (editable note text written
    /// without one), or when what is stated is not one property (several associations,
    /// `general_property_association` WR1, or one naming no `general_property`: a finding).
    /// Read only, as `description`: `pmi::write` names its own after the attribute.
    pub general_property: Option<GeneralProperty>,
}

/// A `general_property` (ISO 10303-41): the kind of property a property definition is.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct GeneralProperty {
    /// `general_property.id`, decoded.
    pub id: String,
    /// `general_property.name`: 'user defined attribute' in specify-core's files, the
    /// attribute's own name where `general_property_association` WR2 is kept.
    pub name: String,
    /// `general_property.description`; `None` when unset.
    pub description: Option<String>,
}

/// One value of a user defined attribute (UDA practice §7).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum AttributeValue {
    Text(String),
    Integer(i64),
    Real(Decimal),
    Boolean(bool),
    Measure(Value),
    /// A measure of a kind other than length or plane angle: its type name, value and unit
    /// name, as stated. Anonymous units use their source entity reference (`#id`),
    /// resolved against the original STEP document when writing.
    OtherMeasure {
        measure: String,
        value: Decimal,
        unit: String,
    },
}

/// The PMI of one product definition of the file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[cfg_attr(feature = "json", serde(deny_unknown_fields))]
pub struct PartPmi {
    /// The standards applied to the part's GD&T (§4 Figure 1), in the file's order: a file may
    /// apply several (NIST STC-07: ASME Y14.5, Y14.41M, ANSI B4.2, Y14.36M).
    pub standards: Vec<Standard>,
    /// The default number of decimal places (§4.1).
    pub decimal_places: Option<u8>,
    pub features: Vec<Feature>,
    pub datum_targets: Vec<DatumTarget>,
    pub datums: Vec<Datum>,
    pub dimensions: Vec<Dimension>,
    pub tolerances: Vec<GeometricTolerance>,
    pub tolerance_relations: Vec<ToleranceRelation>,
    pub general: Vec<GeneralTolerance>,
    pub threads: Vec<Thread>,
    pub knurls: Vec<Knurl>,
    /// Hole occurrences' definitions. Read only: the writer neither writes nor refuses them
    /// (`docs/step-ap242.md`, Q-holes).
    pub holes: Vec<Hole>,
    pub material: Option<Material>,
    pub notes: Vec<Note>,
    pub surface_textures: Vec<SurfaceTexture>,
    pub attributes: Vec<AttributeSet>,
    /// Supplemental geometry that features are made of ([`Anchor::Geometry`]).
    pub geometry: Vec<Geometry>,
    /// Axes and median planes computed from deriving faces.
    pub derived_geometry: Vec<DerivedGeometry>,
}

impl PartPmi {
    /// Whether two tolerance targets are one: equal, or features of equal content (a file may
    /// state the same faces twice as two shape aspects).
    #[must_use]
    pub fn same_target(&self, a: &ToleranceTarget, b: &ToleranceTarget) -> bool {
        match (a, b) {
            (ToleranceTarget::Feature(x), ToleranceTarget::Feature(y)) => {
                x == y
                    || (x.0 < self.features.len()
                        && y.0 < self.features.len()
                        && match (&self.features[x.0], &self.features[y.0]) {
                            (
                                Feature::Derived {
                                    kind: ak,
                                    from: af,
                                    geometry: Some(ag),
                                },
                                Feature::Derived {
                                    kind: bk,
                                    from: bf,
                                    geometry: Some(bg),
                                },
                            ) => {
                                ak == bk
                                    && af == bf
                                    && match (
                                        self.derived_geometry.get(ag.0),
                                        self.derived_geometry.get(bg.0),
                                    ) {
                                        (Some(a), Some(b)) => {
                                            a.kind == b.kind
                                                && a.deriving_faces == b.deriving_faces
                                                && a.point == b.point
                                                && a.direction == b.direction
                                                && a.radius == b.radius
                                        }
                                        _ => false,
                                    }
                            }
                            (a, b) => a == b,
                        })
            }
            _ => a == b,
        }
    }

    /// Every cross-reference and part-level invariant, each violation described.
    pub fn validate(&self) -> Result<(), Vec<ModelError>> {
        let mut out = Vec::new();
        let mut bad = |why: String| out.push(ModelError(why));
        let feature = |f: FeatureId| f.0 < self.features.len();
        let nf = self.features.len();
        for (i, f) in self.features.iter().enumerate() {
            if let Feature::Items(items) = f {
                for a in items {
                    if let Anchor::Geometry(g) = a
                        && g.0 >= self.geometry.len()
                    {
                        bad(format!("feature {i} names missing geometry {}", g.0));
                    }
                }
            }
            match f {
                Feature::Items(items) if items.is_empty() => {
                    bad(format!("feature {i} has no items"));
                }
                Feature::Group { members, .. } | Feature::Derived { from: members, .. } => {
                    for m in members {
                        if m.0 >= nf {
                            bad(format!("feature {i} names missing feature {}", m.0));
                        } else if m.0 == i {
                            bad(format!("feature {i} is its own member"));
                        }
                    }
                }
                Feature::Items(_) => {}
            }
        }
        for (i, f) in self.features.iter().enumerate() {
            if let Feature::Derived {
                geometry: Some(g), ..
            } = f
                && g.0 >= self.derived_geometry.len()
            {
                bad(format!(
                    "feature {i} names missing derived geometry {}",
                    g.0
                ));
            }
        }
        for (i, g) in self.derived_geometry.iter().enumerate() {
            if g.deriving_faces.is_empty() {
                bad(format!("derived geometry {i} has no deriving faces"));
            }
            if g.kind == DerivedGeometryKind::MedianPlane
                && !matches!(g.deriving_faces.len(), 1 | 2)
            {
                bad(format!(
                    "derived geometry {i} plane requires one or two deriving faces"
                ));
            }
            if g.point.iter().any(|p| !p.mm().is_finite()) {
                bad(format!("derived geometry {i} has a non-finite point"));
            }
            let d = g.direction.0.each_ref().map(Decimal::to_f64);
            if d.iter().any(|v| !v.is_finite()) || (crate::geom::norm(d) - 1.0).abs() > 1e-9 {
                bad(format!(
                    "derived geometry {i} direction is not a unit vector"
                ));
            }
            if let Some(r) = &g.radius
                && (g.kind != DerivedGeometryKind::Axis || !r.mm().is_finite() || r.mm() <= 0.0)
            {
                bad(format!(
                    "derived geometry {i} has an invalid cylinder radius"
                ));
            }
            for s in &g.supplemental {
                if s.0 >= self.geometry.len() {
                    bad(format!(
                        "derived geometry {i} names missing geometry {}",
                        s.0
                    ));
                }
            }
        }
        for (i, t) in self.datum_targets.iter().enumerate() {
            let mut fs: Vec<FeatureId> = t.on.into_iter().collect();
            if let TargetShape::Area(f) | TargetShape::Curve(f) = t.shape {
                fs.push(f);
            }
            for f in fs {
                if !feature(f) {
                    bad(format!("datum target {i} names missing feature {}", f.0));
                }
            }
        }
        let mut labels: Vec<&DatumLabel> = Vec::new();
        let mut owner = vec![None; self.datum_targets.len()];
        for (i, d) in self.datums.iter().enumerate() {
            if labels.contains(&&d.label) {
                bad(format!("datum label {} is used twice", d.label));
            }
            labels.push(&d.label);
            if let Some(f) = d.feature
                && !feature(f)
            {
                bad(format!("datum {} names missing feature {}", d.label, f.0));
            }
            let mut numbers = Vec::new();
            for t in &d.targets {
                match self.datum_targets.get(t.0) {
                    None => bad(format!("datum {} names missing target {}", d.label, t.0)),
                    Some(target) => {
                        if numbers.contains(&target.number) {
                            bad(format!(
                                "datum {} has two targets numbered {}",
                                d.label, target.number
                            ));
                        }
                        numbers.push(target.number);
                        if let Some(other) = owner[t.0].replace(i) {
                            bad(format!(
                                "datum target {} belongs to datums {} and {}",
                                t.0, self.datums[other].label, d.label
                            ));
                        }
                    }
                }
            }
        }
        for (i, d) in self.dimensions.iter().enumerate() {
            match &d.kind {
                DimensionKind::Size {
                    feature: f, path, ..
                } => {
                    for f in std::iter::once(f).chain(path.iter()) {
                        if !feature(*f) {
                            bad(format!("dimension {i} names missing feature {}", f.0));
                        }
                    }
                }
                DimensionKind::Location { from, to, path, .. } => {
                    for f in [from, to].into_iter().chain(path.iter()) {
                        if !feature(*f) {
                            bad(format!("dimension {i} names missing feature {}", f.0));
                        }
                    }
                }
            }
            let angular = match &d.kind {
                DimensionKind::Size { angle, .. } | DimensionKind::Location { angle, .. } => {
                    angle.is_some()
                }
            };
            let bounds = match &d.tolerance {
                DimTolerance::Deviations(b) | DimTolerance::Limits(b) => Some(b),
                DimTolerance::Fit { limits, .. } => limits.as_ref(),
                _ => None,
            };
            let reference = d
                .nominal
                .as_ref()
                .map(|v| &v.quantity)
                .or_else(|| bounds.map(|b| &b.upper.quantity));
            let nominal_angle = matches!(reference, Some(Quantity::Angle(_)));
            if reference.is_some() && angular != nominal_angle {
                bad(format!(
                    "dimension {i}: an {} dimension with an {} value",
                    if angular { "angular" } else { "linear" },
                    if nominal_angle { "angle" } else { "length" }
                ));
            }
            if let (Some(b), Some(n)) = (bounds, &d.nominal)
                && !b.upper.quantity.same_kind(&n.quantity)
            {
                bad(format!(
                    "dimension {i}: bounds and nominal of different kinds"
                ));
            }
            if matches!(
                d.tolerance,
                DimTolerance::Deviations(_) | DimTolerance::Fit { .. }
            ) && d.nominal.is_none()
            {
                bad(format!(
                    "dimension {i}: deviations or a class without a nominal"
                ));
            }
            if matches!(d.tolerance, DimTolerance::Fit { .. }) && nominal_angle {
                bad(format!("dimension {i}: an ISO 286 class on an angle"));
            }
        }
        for (i, t) in self.tolerances.iter().enumerate() {
            match &t.target {
                ToleranceTarget::Feature(f) if !feature(*f) => {
                    bad(format!("tolerance {i} names missing feature {}", f.0));
                }
                ToleranceTarget::Dimension(d) if d.0 >= self.dimensions.len() => {
                    bad(format!("tolerance {i} names missing dimension {}", d.0));
                }
                ToleranceTarget::Relation {
                    relating, related, ..
                } => {
                    for f in [relating, related] {
                        if !feature(*f) {
                            bad(format!("tolerance {i} names missing feature {}", f.0));
                        }
                    }
                }
                _ => {}
            }
            match (t.kind.datums(), &t.datums) {
                (DatumRequirement::Required, None) => {
                    bad(format!("tolerance {i}: {:?} requires datums", t.kind));
                }
                (DatumRequirement::Forbidden, Some(_)) => {
                    bad(format!("tolerance {i}: {:?} takes no datums", t.kind));
                }
                _ => {}
            }
            if let Some(ds) = &t.datums {
                for c in &ds.0 {
                    for r in &c.references {
                        if r.datum.0 >= self.datums.len() {
                            bad(format!("tolerance {i} names missing datum {}", r.datum.0));
                        }
                    }
                }
            }
            if let Some(z) = &t.zone {
                let fs = z
                    .projected
                    .iter()
                    .filter_map(|p| p.end)
                    .chain(z.non_uniform.iter().flatten().copied())
                    .chain(z.affected_plane);
                for f in fs {
                    if !feature(f) {
                        bad(format!(
                            "tolerance {i}'s zone names missing feature {}",
                            f.0
                        ));
                    }
                }
            }
            for v in t.magnitude.iter().chain(&t.maximum).chain(&t.unequal) {
                if !matches!(v.quantity, Quantity::Length(_)) {
                    bad(format!("tolerance {i}: a magnitude that is not a length"));
                }
            }
        }
        let mut composite = vec![0usize; self.tolerances.len()];
        for (i, r) in self.tolerance_relations.iter().enumerate() {
            let (Some(a), Some(b)) = (
                self.tolerances.get(r.relating.0),
                self.tolerances.get(r.related.0),
            ) else {
                bad(format!("relation {i} names a missing tolerance"));
                continue;
            };
            if r.relating == r.related {
                bad(format!("relation {i} relates a tolerance to itself"));
            }
            let mut pending = vec![r.related.0];
            let mut seen = std::collections::BTreeSet::new();
            while let Some(node) = pending.pop() {
                if node == r.relating.0 {
                    bad(format!(
                        "relation {i} is cyclic (geometric_tolerance_relationship.WR3)"
                    ));
                    break;
                }
                if seen.insert(node) {
                    pending.extend(
                        self.tolerance_relations
                            .iter()
                            .filter(|e| e.relating.0 == node)
                            .map(|e| e.related.0),
                    );
                }
            }
            if r.kind == RelationKind::Composite {
                let kinds = [
                    ToleranceKind::Position,
                    ToleranceKind::LineProfile,
                    ToleranceKind::SurfaceProfile,
                ];
                if a.kind != b.kind || !kinds.contains(&a.kind) {
                    bad(format!(
                        "composite relation {i} relates {:?} and {:?} (geometric_tolerance_relationship WR1)",
                        a.kind, b.kind
                    ));
                }
                if !self.same_target(&a.target, &b.target) {
                    bad(format!(
                        "composite relation {i} relates tolerances on different targets (WR2)"
                    ));
                }
                composite[r.relating.0] += 1;
            }
        }
        for (i, n) in composite.iter().enumerate() {
            if *n > 1 {
                bad(format!(
                    "tolerance {i} relates {n} composite relations (geometric_tolerance WR5)"
                ));
            }
        }
        for (i, th) in self.threads.iter().enumerate() {
            for f in std::iter::once(th.feature)
                .chain(th.partial_area)
                .chain(th.runout)
            {
                if !feature(f) {
                    bad(format!("thread {i} names missing feature {}", f.0));
                }
            }
        }
        for (i, h) in self.holes.iter().enumerate() {
            if !feature(h.feature) {
                bad(format!("hole {i} names missing feature {}", h.feature.0));
            }
        }
        for (i, k) in self.knurls.iter().enumerate() {
            if !feature(k.feature) {
                bad(format!("knurl {i} names missing feature {}", k.feature.0));
            }
            match k.pattern {
                KnurlPattern::Diamond | KnurlPattern::Diagonal if k.helix_angle.is_none() => {
                    bad(format!(
                        "knurl {i}: a {:?} knurl needs a helix angle (WR9)",
                        k.pattern
                    ));
                }
                KnurlPattern::Diagonal if k.helix_hand.is_none() => {
                    bad(format!(
                        "knurl {i}: a diagonal knurl needs a helix hand (WR10)"
                    ));
                }
                _ => {}
            }
        }
        for (i, s) in self.surface_textures.iter().enumerate() {
            if let Some(f) = s.on
                && !feature(f)
            {
                bad(format!("surface texture {i} names missing feature {}", f.0));
            }
            if s.parameters.is_empty() {
                bad(format!("surface texture {i} has no parameter"));
            }
            for p in &s.parameters {
                if p.characteristic.trim().is_empty() || p.characteristic.trim() != p.characteristic
                {
                    bad(format!(
                        "surface texture {i}: the characteristic {:?} is not a name",
                        p.characteristic
                    ));
                }
            }
        }
        for (i, n) in self.notes.iter().enumerate() {
            for f in n.callouts.iter().flat_map(|c| &c.features) {
                if !feature(*f) {
                    bad(format!("note {i}'s callout names missing feature {}", f.0));
                }
            }
        }
        let owners = self
            .notes
            .iter()
            .filter_map(|n| n.on)
            .chain(self.attributes.iter().filter_map(|a| a.on));
        for o in owners {
            let ok = match o {
                NoteOwner::Feature(f) => feature(f),
                NoteOwner::Dimension(d) => d.0 < self.dimensions.len(),
                NoteOwner::Tolerance(t) => t.0 < self.tolerances.len(),
                NoteOwner::DatumTarget(t) => t.0 < self.datum_targets.len(),
            };
            if !ok {
                bad(format!("a note or attribute is on missing {o:?}"));
            }
        }
        if out.is_empty() { Ok(()) } else { Err(out) }
    }
}
