//! Requirement wording and ISO 1302 geometry ported from pzfreo/draftwright
//! aaa7a73: src/dims/gdt.rs::finish_symbol, src/title_block.rs::removal_marks,
//! src/pmi_lowering.rs::part_note_text, src/notes.rs (Paul Fremantle; #137).
//! Material/tolerance title cells become labelled rows in the model's notes block.
use super::{Callout, Text, font};
use crate::pmi::*;

fn width(s: &str, height: f64) -> f64 {
    let ink = font().text(s, height);
    ink.max[0] - ink.min[0]
}

/// The written property supplying a row; indices are in the submitted PartPmi only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    Material,
    General(usize),
    Texture(usize),
    Attribute(usize),
    Note(usize),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub source: Source,
    pub text: String,
}

/// Surface parameters in their stated units and decimal precision.
/// Micrometres are implicit in ISO 1302 notation; other units are explicit.
pub fn texture_parameters(t: &SurfaceTexture) -> Vec<String> {
    t.parameters
        .iter()
        .map(|p| {
            let unit = match &p.value.unit {
                LengthUnit::Micrometre => "",
                LengthUnit::Millimetre => " mm",
                LengthUnit::Centimetre => " cm",
                LengthUnit::Metre => " m",
                LengthUnit::Inch => " in",
                LengthUnit::Foot => " ft",
                LengthUnit::Other { name, .. } => {
                    return format!(
                        "{} {} {name}",
                        p.characteristic,
                        super::tolerance::number(&p.value.value)
                    );
                }
            };
            format!(
                "{} {}{unit}",
                p.characteristic,
                super::tolerance::number(&p.value.value)
            )
        })
        .collect()
}

/// Preserve process prose; qualify a bare coating or heat-treatment value (Draftwright).
pub fn part_note_text(kind: &str, value: &str) -> String {
    let text = value.trim();
    let title = match kind {
        "surface treatment" => "Coating",
        "heat treatment" => "Heat treatment",
        _ => return text.into(),
    };
    let process = text
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphabetic())
        .any(|word| {
            matches!(word, "plate" | "plated" | "plating")
                || [
                    "anodis", "anodiz", "paint", "coat", "harden", "temper", "nitrid", "treat",
                    "anneal", "carbur", "quench", "passivat", "oxid", "phosphat", "galvanis",
                    "galvaniz",
                ]
                .iter()
                .any(|stem| word.starts_with(stem))
        });
    if process {
        text.into()
    } else {
        format!("{title}: {text}")
    }
}

fn note_kind(kind: &str) -> &str {
    match kind {
        "general tolerance" | "general tolerances" => "general tolerance",
        "surface texture" | "surface finish" | "finish" => "surface finish",
        "edge condition" | "edges" => "edge condition",
        "surface treatment" | "coating" => "surface treatment",
        "heat treatment" => "heat treatment",
        "general note" | "semantic text" => "general note",
        _ if kind.starts_with("general note ") => "general note",
        _ => "other",
    }
}
fn default_finish(t: &SurfaceTexture) -> String {
    format!(
        "SURFACE FINISH {} UNLESS OTHERWISE STATED",
        texture_parameters(t).join(", ")
    )
}
fn prose_rows(p: &PartPmi, kind: &str) -> Vec<Row> {
    let wording = |text: &str| match kind {
        "general tolerance" => format!("GENERAL TOLERANCES {text}"),
        "surface finish" => format!(
            "SURFACE FINISH {text}{}",
            if text.to_ascii_uppercase().contains("UNLESS") {
                ""
            } else {
                " UNLESS OTHERWISE STATED"
            }
        ),
        "general note" | "other" => text.to_owned(),
        _ => part_note_text(kind, text),
    };
    let mut items = Vec::new();
    for (i, a) in p
        .attributes
        .iter()
        .enumerate()
        .filter(|(_, a)| a.on.is_none() && a.name == "semantic text")
    {
        let index = a.items.iter().find_map(|(k, v)| {
            if let AttributeValue::Integer(n) = v {
                (k == "note index" && *n > 0).then_some(*n)
            } else {
                None
            }
        });
        for (key, value) in &a.items {
            // Unknown attribute fields can carry opaque CAD-font symbols,
            // not prose notes. Keep those semantic fields and file graphics;
            // only explicitly submitted Notes use the catch-all category.
            if kind != "other"
                && note_kind(key) == kind
                && let AttributeValue::Text(text) = value
            {
                items.push((
                    index,
                    Row {
                        source: Source::Attribute(i),
                        text: wording(text),
                    },
                ));
            }
        }
    }
    for (i, note) in p
        .notes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.on.is_none() && note_kind(&n.kind) == kind)
    {
        items.push((
            None,
            Row {
                source: Source::Note(i),
                // Use the writer's explicit-unit parser; do not guess units for
                // prose. Native and normalized legacy writing then draw alike.
                text: if kind == "surface finish" {
                    crate::pmi::write::note_texture(note)
                        .map_or_else(|| wording(&note.text), |t| default_finish(&t))
                } else {
                    wording(&note.text)
                },
            },
        ));
    }
    if kind == "general note" {
        items.sort_by_key(|(index, row)| (index.is_none(), index.unwrap_or(0), row.source));
    }
    items.into_iter().map(|(_, row)| row).collect()
}

/// Submitted settings in block order: tolerance, finish, edges, coating, heat treatment,
/// free notes, material. Free notes retain their explicit index order (stable input
/// order when no index is stated). Draftwright keeps tolerance/finish in separate
/// sheet defaults; this is the combined model-space order.
pub fn general_notes(p: &PartPmi) -> Vec<Row> {
    let mut rows = Vec::new();
    for (i, g) in p.general.iter().enumerate() {
        if let GeneralTolerance::Class { text, .. } = g {
            rows.push(Row {
                source: Source::General(i),
                text: format!("GENERAL TOLERANCES {text}"),
            });
        }
    }
    rows.extend(prose_rows(p, "general tolerance"));
    for (i, t) in p
        .surface_textures
        .iter()
        .enumerate()
        .filter(|(_, t)| t.on.is_none())
    {
        rows.push(Row {
            source: Source::Texture(i),
            text: default_finish(t),
        });
    }
    rows.extend(prose_rows(p, "surface finish"));
    for kind in [
        "edge condition",
        "surface treatment",
        "heat treatment",
        "general note",
        "other",
    ] {
        rows.extend(prose_rows(p, kind));
    }
    if let Some(m) = &p.material {
        rows.push(Row {
            source: Source::Material,
            text: format!("MATERIAL: {}", m.id),
        });
    }
    rows
}

/// Face/feature-owned prose gets its own leadered callout rather than a block row.
/// Typed writes migrate Notes to semantic-text attributes; JSON writes retain Notes.
pub(crate) fn feature_notes(p: &PartPmi) -> Vec<(Row, FeatureId)> {
    let mut rows = Vec::new();
    for (i, a) in p.attributes.iter().enumerate() {
        if a.name == "semantic text"
            && let Some(NoteOwner::Feature(feature)) = a.on
        {
            let text = a
                .items
                .iter()
                .filter_map(|(_, value)| match value {
                    AttributeValue::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            rows.push((
                Row {
                    source: Source::Attribute(i),
                    text,
                },
                feature,
            ));
        }
    }
    for (i, note) in p.notes.iter().enumerate() {
        if let Some(NoteOwner::Feature(feature)) = note.on {
            rows.push((
                Row {
                    source: Source::Note(i),
                    text: note.text.clone(),
                },
                feature,
            ));
        }
    }
    rows
}

/// Left-aligned rows at the same capital height as dimensions, headed as Draftwright's table.
pub fn notes_block(rows: &[Row]) -> Callout {
    if rows.is_empty() {
        return Callout::default();
    }
    let lines: Vec<_> = std::iter::once("GENERAL NOTES".to_owned())
        .chain(rows.iter().enumerate().map(|(index, r)| {
            let number = index + 1;
            // Draftwright notes.rs::rows: first line numbered, continuations indented.
            let first_ink = r.text.split('\n').position(|line| !line.trim().is_empty());
            r.text
                .split('\n')
                .enumerate()
                .map(|(i, line)| {
                    if line.trim().is_empty() {
                        line.to_string()
                    } else if Some(i) == first_ink {
                        format!("{number}  {line}")
                    } else {
                        format!("   {line}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        }))
        .collect();
    let mut c = Callout::default();
    let height = 3.;
    let block_width = lines.iter().map(|l| width(l, height)).fold(0., f64::max);
    let heights: Vec<_> = lines
        .iter()
        .map(|l| {
            let ink = font().text(l, height);
            let mut extent = (ink.max[1] - ink.min[1]).max(height);
            if l.split('\n').any(|line| line.trim().is_empty()) {
                extent = extent.max(height + l.matches('\n').count() as f64 * 1.6 * height);
            }
            extent
        })
        .collect();
    c.size = [
        block_width + 3.6,
        heights.iter().sum::<f64>() + (lines.len() - 1) as f64 * 0.6 * height + 3.6,
    ];
    let mut top = c.size[1] - 1.8;
    for (line, row_height) in lines.iter().zip(heights) {
        // Font::text centres on ink, so restore the authored position of leading
        // and trailing blank baselines inside the space reserved for this row.
        let leading = line.split('\n').take_while(|s| s.trim().is_empty()).count();
        let trailing = line
            .rsplit('\n')
            .take_while(|s| s.trim().is_empty())
            .count();
        let blank_shift = (trailing as f64 - leading as f64) * 0.8 * height;
        let text = Text {
            text: line.clone(),
            at: [
                1.8 + width(line, height) / 2.,
                top - row_height / 2. + blank_shift,
            ],
            size: height,
        };
        c.letters.push(text.clone());
        c.runs.push(text);
        top -= row_height + 0.6 * height;
    }
    c.cells = lines;
    c
}

/// Draftwright's two 60-degree legs, parameter bar and removal mark, in text-height units.
pub fn surface_texture(t: &SurfaceTexture) -> Callout {
    surface_texture_with_width(t, &width)
}

/// ISO 1302 symbol sized with the caller's text ink width at a capital height.
pub fn surface_texture_with_width(t: &SurfaceTexture, width: &dyn Fn(&str, f64) -> f64) -> Callout {
    let lines = texture_parameters(t);
    let text = 3.0_f64;
    let gap = 0.3 * text;
    let cot = 1. / 3f64.sqrt();
    let short = 1.4 * text;
    let long = (3. * text).max(lines.len() as f64 * (text + gap));
    let tip = [gap + short * cot, 0.];
    let at = |p: [f64; 2]| [p[0] + tip[0], p[1]];
    let top = at([long * cot, long]);
    let tw = lines
        .iter()
        .map(|l| width(l, text))
        .fold(0.5 * text, f64::max);
    let left = top[0] + gap;
    let end = left + tw + gap;
    let mut c = Callout {
        strokes: vec![
            [at([-short * cot, short]), tip],
            [tip, top],
            [top, [end, long]],
        ],
        size: [end, long],
        cells: lines.clone(),
        ..Callout::default()
    };
    let (strokes, arcs) = removal_marks(t.material_removal, short);
    c.strokes.extend(strokes.into_iter().map(|s| s.map(at)));
    c.arcs.extend(arcs.into_iter().map(|[x, y, r, a, b]| {
        let [x, y] = at([x, y]);
        [x, y, r, a, b]
    }));
    for (i, line) in lines.iter().enumerate() {
        let t = Text {
            text: line.clone(),
            at: [
                left + tw / 2.,
                long - gap - text / 2. - i as f64 * (text + gap),
            ],
            size: text,
        };
        c.letters.push(t.clone());
        c.runs.push(t);
    }
    c
}

/// ISO 1302 removal condition about a symbol tip at the origin.
/// `short` is the short leg's height. Returns line segments and circular arcs
/// (`[cx, cy, radius, start_degrees, end_degrees]`) in the same drawing units.
pub fn removal_marks(removal: MaterialRemoval, short: f64) -> (Vec<[[f64; 2]; 2]>, Vec<[f64; 5]>) {
    let cot = 1. / 3f64.sqrt();
    match removal {
        MaterialRemoval::Required => (
            vec![[[-short * cot, short], [short * cot, short]]],
            Vec::new(),
        ),
        MaterialRemoval::NotAllowed => (
            Vec::new(),
            vec![[0., 2. * short / 3., short / 3., 0., 360.]],
        ),
        MaterialRemoval::AnyProcessAllowed => (Vec::new(), Vec::new()),
    }
}
