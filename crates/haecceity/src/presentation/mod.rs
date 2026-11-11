//! One owner of requirement notation and callout geometry in a local 2D frame.
//!
//! Ported from pzfreo/draftwright at aaa7a73: `src/dims/tolerance.rs`,
//! `src/dims/gdt.rs`, `src/pmi.rs` (qualifiers/limits/scope), and `src/text.rs` (Paul Fremantle; port authorized by the copyright owner for #137, same project family).
//! Sheet layout, leaders, lanes and drawn-precision rounding stay in
//! Draftwright. Values here retain requirement precision. Text uses public-domain Hershey
//! strokes plus explicit ISO symbols; no font parser or embedded outline font is needed.

use crate::pmi::*;

pub mod gdt;
pub mod linear;
pub mod notes;
pub mod placement;
pub mod text;
pub mod tolerance;

/// Text centred on its ink bounds, at a local point, with size in drawing units.
#[derive(Clone, Debug)]
pub struct Text {
    pub text: String,
    pub at: [f64; 2],
    pub size: f64,
}

/// Geometry and readable cells of one annotation, before placement in a model or sheet.
#[derive(Default, Clone, Debug)]
#[non_exhaustive]
pub struct Callout {
    pub strokes: Vec<[[f64; 2]; 2]>,
    /// Initial straight strokes which form frames (remaining strokes are symbols).
    pub frame_strokes: usize,
    /// Explicit symbol strokes/arcs also represented in readable text cells.
    pub text_symbols: bool,
    /// Local lower-left / upper-right corners, parallel to readable cells.
    pub cell_boxes: Vec<[[f64; 2]; 2]>,
    /// Centre x/y, radius, start/end angle in degrees.
    pub arcs: Vec<[f64; 5]>,
    pub letters: Vec<Text>,
    /// Text layer including symbols drawn geometrically rather than by the font.
    pub runs: Vec<Text>,
    pub cells: Vec<String>,
    pub size: [f64; 2],
    /// Draftwright's leader-bend scope symbol: one ring for all-around, two for all-over.
    pub leader_rings: u8,
}

/// Hershey ink width, in drawing units at the supplied capital height.
pub fn text_width(value: &str, height: f64) -> f64 {
    let ink = font().text(value, height);
    ink.max[0] - ink.min[0]
}

pub fn font() -> &'static text::Font {
    &text::Font
}

impl Callout {
    /// Full, readable notation, shared by presentation names and drawing clients.
    pub fn notation(&self) -> String {
        self.cells.join(" ")
    }

    /// Single-stroke text, frame lines and explicit symbol arcs; no filled text surfaces.
    pub fn polylines(&self) -> Vec<Vec<[f64; 2]>> {
        let mut lines: Vec<_> = self.strokes.iter().map(|s| s.to_vec()).collect();
        for &[x, y, r, a, b] in &self.arcs {
            lines.push(
                (0..=32)
                    .map(|i| {
                        let t = (a + (b - a) * i as f64 / 32.).to_radians();
                        [x + r * t.cos(), y + r * t.sin()]
                    })
                    .collect(),
            );
        }
        for t in &self.letters {
            for line in font().text(&t.text, t.size).lines {
                lines.push(
                    line.into_iter()
                        .map(|p| [p[0] + t.at[0], p[1] + t.at[1]])
                        .collect(),
                );
            }
        }
        lines
    }
}

fn decimal(v: &Value) -> String {
    tolerance::number(v.decimal())
}

// Convert only when the requirement actually uses different units, never to sheet precision.
fn in_units(v: &Value, nominal: &Value) -> Decimal {
    if v.quantity.cmp_same_unit(&nominal.quantity).is_some() {
        v.decimal().clone()
    } else {
        let scale = match &nominal.quantity {
            Quantity::Length(l) => l.unit.millimetres(),
            Quantity::Angle(a) => a.unit.radians(),
        };
        if scale.is_finite() && scale != 0.0 {
            Decimal::parse(&(v.si() / scale).to_string()).unwrap_or_else(|_| v.decimal().clone())
        } else {
            v.decimal().clone()
        }
    }
}

/// Prefix for a size value (ISO 129-1:2018 §§7.2, 7.3.1 and 7.4).
/// Toroidal sizes use the ordinary circular radius/diameter symbols: these standards
/// define no torus-specific prefix. ISO 1101:2017 §8.4.2's major/minor identifiers
/// concern screw threads, not tori. See also ISO 14405-1:2025 §5.2.2 and Annex E.
/// A general curve length need not be a circular arc (§7.6); thickness does not
/// require the optional surface-leader notation `t=` (§7.16.1). Both stay unprefixed.
pub fn size_prefix(kind: &SizeKind) -> &'static str {
    match kind {
        SizeKind::Diameter
        | SizeKind::ToroidalMinorDiameter
        | SizeKind::ToroidalMajorDiameter
        | SizeKind::ToroidalHighMajorDiameter
        | SizeKind::ToroidalLowMajorDiameter => "⌀",
        SizeKind::SphericalDiameter => "S⌀",
        SizeKind::Radius
        | SizeKind::ToroidalMinorRadius
        | SizeKind::ToroidalMajorRadius
        | SizeKind::ToroidalHighMajorRadius
        | SizeKind::ToroidalLowMajorRadius => "R",
        SizeKind::SphericalRadius => "SR",
        SizeKind::CurveLength | SizeKind::Thickness | SizeKind::Other(_) => "",
    }
}

/// Text for a dimension. Limits remain limits; a fit remains its
/// ISO 286 class when no limits are stated. Zero deviations have no sign (ISO 129-1).
pub fn dimension_text(d: &Dimension) -> String {
    let (prefix, angular) = match &d.kind {
        DimensionKind::Size { kind, angle, .. } => (size_prefix(kind), angle.is_some()),
        DimensionKind::Location { angle, .. } => ("", angle.is_some()),
    };
    let suffix = if angular
        && d.nominal.as_ref().is_some_and(
            |n| matches!(&n.quantity, Quantity::Angle(a) if a.unit == AngleUnit::Degree),
        ) {
        "°"
    } else {
        ""
    };
    let mut text = format!(
        "{prefix}{}{suffix}",
        d.nominal.as_ref().map_or(String::new(), decimal)
    );
    match &d.tolerance {
        DimTolerance::Deviations(b) => {
            let value = |v: &Value| {
                d.nominal
                    .as_ref()
                    .map_or_else(|| v.decimal().clone(), |n| in_units(v, n))
            };
            let u = value(b.upper());
            let l = value(b.lower());
            let band = tolerance::band_text(&u, &l);
            text.push_str(&format!(" {band}"));
        }
        // Draftwright src/pmi.rs dimension_label: stated fit limits print as a range,
        // exactly like explicit limits; do not compute unstated limits from a class.
        DimTolerance::Limits(b)
        | DimTolerance::Fit {
            limits: Some(b), ..
        } => {
            text = format!(
                "{prefix}{}{suffix} - {prefix}{}{suffix}",
                decimal(b.lower()),
                decimal(b.upper())
            );
        }
        DimTolerance::Fit { class, .. } => text.push_str(&format!(" {class}")),
        _ => {}
    }
    // Ported from Draftwright aaa7a73 src/pmi.rs:1934–1941.
    if let Some(suffix) = d.qualifier.as_ref().and_then(|q| match q {
        Qualifier::Maximum => Some("MAX"),
        Qualifier::Minimum => Some("MIN"),
        Qualifier::Average => Some("AVG"),
        Qualifier::Other(_) => None,
    }) {
        text.push_str(&format!(" {suffix}"));
    }
    if d.modifiers.contains(&DimensionModifier::Reference) {
        text = format!("({text})");
    }
    text
}

pub fn dimension_type(d: &Dimension) -> &'static str {
    match &d.kind {
        DimensionKind::Size { angle: Some(_), .. }
        | DimensionKind::Location { angle: Some(_), .. } => "angular dimension",
        DimensionKind::Size {
            kind:
                SizeKind::Diameter
                | SizeKind::SphericalDiameter
                | SizeKind::ToroidalMinorDiameter
                | SizeKind::ToroidalMajorDiameter
                | SizeKind::ToroidalHighMajorDiameter
                | SizeKind::ToroidalLowMajorDiameter,
            ..
        } => "diameter dimension",
        DimensionKind::Size {
            kind:
                SizeKind::Radius
                | SizeKind::SphericalRadius
                | SizeKind::ToroidalMinorRadius
                | SizeKind::ToroidalMajorRadius
                | SizeKind::ToroidalHighMajorRadius
                | SizeKind::ToroidalLowMajorRadius,
            ..
        } => "radial dimension",
        _ => "linear dimension",
    }
}

pub fn dimension(d: &Dimension) -> Callout {
    let value = dimension_text(d);
    let mut c = label(&value);
    if d.tolerance == DimTolerance::Basic {
        let [w, h] = c.size;
        c.frame_strokes = 4;
        c.strokes.extend([
            [[0., 0.], [w, 0.]],
            [[w, 0.], [w, h]],
            [[w, h], [0., h]],
            [[0., h], [0., 0.]],
        ]);
    }
    c
}

pub fn label(value: &str) -> Callout {
    label_with_width(value, &text_width)
}

/// A label using the caller's ink width at the requested capital height.
/// Coordinates, padding and text height are in the same drawing units as the width.
pub fn label_with_width(value: &str, width: &dyn Fn(&str, f64) -> f64) -> Callout {
    let size = [width(value, 3.0) + 3.6, 6.0];
    let text = Text {
        text: value.into(),
        at: [size[0] / 2.0, 3.0],
        size: 3.0,
    };
    Callout {
        letters: vec![text.clone()],
        runs: vec![text],
        cells: vec![value.into()],
        size,
        ..Default::default()
    }
}

/// Independent, unpadded lines, in input order, for a caller that owns lane layout.
/// Each callout has its own lower-left origin and no inter-line offset or box padding.
/// The width callback receives each line at capital height 3, in drawing units.
/// Height is the Hershey ink extent (including descenders); an empty line has zero
/// ink height. Text is centred at half the returned size. An empty slice returns no
/// callouts. Use one string per line, and `text_width` for the built-in font.
pub fn label_lines_with_width(lines: &[&str], width: &dyn Fn(&str, f64) -> f64) -> Vec<Callout> {
    lines
        .iter()
        .map(|line| {
            let ink = font().text(line, 3.0);
            let size = [width(line, 3.0), ink.max[1] - ink.min[1]];
            let text = Text {
                text: (*line).into(),
                at: [size[0] / 2.0, size[1] / 2.0],
                size: 3.0,
            };
            Callout {
                letters: vec![text.clone()],
                runs: vec![text],
                cells: vec![(*line).into()],
                size,
                ..Default::default()
            }
        })
        .collect()
}

pub fn tolerance_type(t: &GeometricTolerance) -> &'static str {
    match t.kind {
        ToleranceKind::Roundness => "circularity",
        ToleranceKind::LineProfile => "profile of line",
        ToleranceKind::SurfaceProfile => "profile of surface",
        ToleranceKind::CircularRunout => "circular runout",
        ToleranceKind::TotalRunout => "total runout",
        ToleranceKind::Angularity => "angularity",
        ToleranceKind::Coaxiality => "coaxiality",
        ToleranceKind::Concentricity => "concentricity",
        ToleranceKind::Cylindricity => "cylindricity",
        ToleranceKind::Flatness => "flatness",
        ToleranceKind::Parallelism => "parallelism",
        ToleranceKind::Perpendicularity => "perpendicularity",
        ToleranceKind::Position => "position",
        ToleranceKind::Straightness => "straightness",
        ToleranceKind::Symmetry => "symmetry",
    }
}

pub fn tolerance(t: &GeometricTolerance, p: &PartPmi) -> Callout {
    let name = match t.kind {
        ToleranceKind::LineProfile => "profile_line",
        ToleranceKind::SurfaceProfile => "profile_surface",
        ToleranceKind::CircularRunout => "circular_runout",
        ToleranceKind::TotalRunout => "total_runout",
        _ => tolerance_type(t),
    };
    let zone = t.zone.as_ref().and_then(|z| match z.form {
        ZoneForm::WithinACircle | ZoneForm::WithinACylinder | ZoneForm::CylindricalOrCircular => {
            Some("diameter")
        }
        ZoneForm::Spherical | ZoneForm::WithinASphere => Some("spherical_diameter"),
        _ => None,
    });
    let material = if t
        .modifiers
        .contains(&ToleranceModifier::MaximumMaterialRequirement)
    {
        Some("M")
    } else if t
        .modifiers
        .contains(&ToleranceModifier::LeastMaterialRequirement)
    {
        Some("L")
    } else {
        None
    };
    let datums: Vec<String> = t
        .datums
        .iter()
        .flat_map(|d| d.compartments())
        .map(|c| {
            c.references
                .iter()
                .map(|r| p.datums[r.datum.0].label().to_string())
                .collect::<Vec<_>>()
                .join("-")
        })
        .collect();
    let mut c = gdt::control_frame(
        name,
        &t.magnitude.as_ref().map_or(String::new(), decimal),
        zone,
        material,
        &datums,
        &text_width,
    );
    // Draftwright pmi.rs support()/tolerance() and dims/gdt.rs leader scope rings.
    fn around(p: &PartPmi, f: FeatureId, depth: usize) -> bool {
        if depth > p.features.len() {
            return false;
        }
        match &p.features[f.0] {
            Feature::Group {
                kind: GroupKind::AllAround,
                ..
            } => true,
            Feature::Group { members, .. } | Feature::Derived { from: members, .. } => {
                members.iter().any(|f| around(p, *f, depth + 1))
            }
            Feature::Items(_) => false,
        }
    }
    c.leader_rings = if t.target == ToleranceTarget::WholePart
        || t.auxiliary.contains(&AuxiliaryClassification::AllOver)
    {
        2
    } else {
        let features = match &t.target {
            ToleranceTarget::Feature(f) => vec![*f],
            ToleranceTarget::Relation {
                relating, related, ..
            } => vec![*relating, *related],
            ToleranceTarget::Dimension(d) => match p.dimensions[d.0].kind {
                DimensionKind::Size { feature, .. } => vec![feature],
                DimensionKind::Location { from, to, .. } => vec![from, to],
            },
            ToleranceTarget::WholePart => vec![],
        };
        u8::from(features.iter().any(|f| around(p, *f, 0)))
    };
    c
}

/// A boxed datum letter about the local origin (Draftwright's datum frame).
pub fn datum(letter: &str) -> Callout {
    gdt::datum_frame(letter)
}

/// A thread's stated designation, class and manufacturing depth. Never infer a
/// pitch or a depth from the tap drill; AP242 threads need not state either.
pub fn thread(t: &Thread, p: &PartPmi) -> Callout {
    let attributes = p
        .attributes
        .iter()
        .filter(|a| {
            a.name == "thread manufacturing"
                && matches!(a.on, Some(NoteOwner::Feature(f)) if p.same_target(
                &ToleranceTarget::Feature(f), &ToleranceTarget::Feature(t.feature)))
        })
        .flat_map(|a| &a.items)
        .collect::<Vec<_>>();
    let measure = |name: &str| {
        attributes.iter().find_map(|(key, value)| {
            if key == name
                && let AttributeValue::Measure(v) = value
            {
                Some(decimal(v))
            } else {
                None
            }
        })
    };
    let mut value = t
        .qualifier
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}{}", t.form, tolerance::number(&t.major_diameter.value)))
        .replace('x', "×");
    if !value.contains('×')
        && let Some(pitch) = measure("pitch")
    {
        value.push_str(&format!("×{pitch}"));
    }
    if !t.fit_class.is_empty() {
        value.push_str(&format!("-{}", t.fit_class));
    }
    if let Some(second) = t.fit_class_2.as_deref().filter(|s| !s.is_empty()) {
        value.push_str(if t.fit_class.is_empty() { "-" } else { "/" });
        value.push_str(second);
    }
    if t.hand == Hand::Left {
        value.push_str(" LH");
    }
    if attributes.iter().any(|(key, v)| {
        key == "through"
            && (matches!(v, AttributeValue::Text(s) if s == "true")
                || matches!(v, AttributeValue::Boolean(true)))
    }) {
        value.push_str(" THRU");
    } else if let Some(depth) = measure("minimum full thread").or_else(|| measure("thread length"))
    {
        value.push_str(&format!(" ↧ {depth}"));
    }
    label(&value)
}
