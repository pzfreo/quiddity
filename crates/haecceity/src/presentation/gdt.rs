//! Frame and characteristic geometry ported from pzfreo/draftwright aaa7a73,
//! src/dims/gdt.rs (Paul Fremantle; port authorized by the copyright owner for #137), without sheet planning or routing.
use super::cxf::{self, Face};
use super::{Callout as Laid, Text as GdtText};
type P2 = [f64; 2];
/// Capital height of the text in the CAD-font frames ([`cad_control_frame`],
/// [`cad_datum_frame`]), in millimetres: ISO 3098's standard 2.5 mm. Cell and box sizes follow
/// from it as a CAD system draws a native TOLERANCE: a frame is `2 * TEXT_HEIGHT` tall and each
/// cell is its ink plus `TEXT_HEIGHT`.
pub const TEXT_HEIGHT: f64 = 2.5;
/// The pen intended for frame text, in millimetres: ISO 3098 type B, one tenth of the text
/// height. Advisory: the CAD-font frames return their text as strokes and arcs, and a renderer
/// chooses how wide to draw them. Frame lines are the renderer's choice too.
pub const TEXT_PEN: f64 = TEXT_HEIGHT / 10.0;
const CAD_H: f64 = 2.0 * TEXT_HEIGHT;
const TEXT: f64 = 3.0;
const H: f64 = 2.0 * TEXT;
const PAD: f64 = 0.6 * TEXT;
const ZONE_R: f64 = 0.42 * TEXT;
const MATERIAL_R: f64 = 0.62 * TEXT;
fn text(s: &str, at: P2, size: f64) -> GdtText {
    GdtText {
        text: s.into(),
        at,
        size,
    }
}
fn circle(c: P2, r: f64) -> [f64; 5] {
    [c[0], c[1], r, 0.0, 360.0]
}

/// ISO 1101 symbol for the model's GD&T name: `straightness`, `flatness`,
/// `circularity`, `cylindricity`, `profile_line`, `profile_surface`, `angularity`,
/// `perpendicularity`, `parallelism`, `position`, `concentricity`, `coaxiality`,
/// `symmetry`, `circular_runout`, or `total_runout`.
/// Unknown names retain the historical total-runout fallback.
pub fn symbol(characteristic: &str) -> &'static str {
    match characteristic {
        "straightness" => "⏤",
        "flatness" => "⏥",
        "circularity" => "○",
        "cylindricity" => "⌭",
        "profile_line" => "⌒",
        "profile_surface" => "⌓",
        "angularity" => "∠",
        "perpendicularity" => "⟂",
        "parallelism" => "∥",
        "position" => "⌖",
        "concentricity" | "coaxiality" => "◎",
        "symmetry" => "⌯",
        "circular_runout" => "↗",
        _ => "⌰",
    }
}

/// The characteristic's strokes about *c* (`_characteristic_edges`, s = 0.42 h). Coaxiality,
/// which draftwright's frame does not know, is drawn as concentricity's symbol, which ISO 1101
/// gives both.
pub fn characteristic(name: &str, c: P2, laid: &mut Laid) {
    let s = 0.42 * TEXT;
    let p = |x: f64, y: f64| [c[0] + x * s, c[1] + y * s];
    let mut st: Vec<[P2; 2]> = Vec::new();
    let mut arcs: Vec<[f64; 5]> = Vec::new();
    let barbs = |tip: P2, back: P2, size: f64| -> [[P2; 2]; 2] {
        let a = (back[1] - tip[1]).atan2(back[0] - tip[0]);
        [20f64, -20.0].map(|d| {
            let t = a + d.to_radians();
            [tip, [tip[0] + size * t.cos(), tip[1] + size * t.sin()]]
        })
    };
    match name {
        "straightness" => st.push([p(-1.0, 0.0), p(1.0, 0.0)]),
        "flatness" => {
            let q = [p(-1.0, -0.5), p(0.3, -0.5), p(1.0, 0.5), p(-0.3, 0.5)];
            for i in 0..4 {
                st.push([q[i], q[(i + 1) % 4]]);
            }
        }
        "circularity" => arcs.push(circle(c, s)),
        "cylindricity" => {
            arcs.push(circle(c, 0.55 * s));
            st.push([p(-1.0, -0.6), p(-0.55, 0.9)]);
            st.push([p(0.55, -0.9), p(1.0, 0.6)]);
        }
        "profile_line" => arcs.push([c[0], c[1], s, 0.0, 180.0]),
        "profile_surface" => {
            arcs.push([c[0], c[1], s, 0.0, 180.0]);
            st.push([p(-1.0, 0.0), p(1.0, 0.0)]);
        }
        "angularity" => {
            st.push([p(-1.0, -1.0), p(1.0, 1.0)]);
            st.push([p(-1.0, -1.0), p(1.0, -1.0)]);
        }
        "perpendicularity" => {
            st.push([p(0.0, -1.0), p(0.0, 1.0)]);
            st.push([p(-1.0, -1.0), p(1.0, -1.0)]);
        }
        "parallelism" => {
            st.push([p(-0.7, -1.0), p(0.1, 1.0)]);
            st.push([p(-0.1, -1.0), p(0.7, 1.0)]);
        }
        "position" => {
            arcs.push(circle(c, 0.6 * s));
            st.push([p(-1.0, 0.0), p(1.0, 0.0)]);
            st.push([p(0.0, -1.0), p(0.0, 1.0)]);
        }
        "concentricity" | "coaxiality" => {
            arcs.push(circle(c, s));
            arcs.push(circle(c, 0.5 * s));
        }
        "symmetry" => {
            st.push([p(0.0, -1.0), p(0.0, 1.0)]);
            st.push([p(-0.7, 0.45), p(0.7, 0.45)]);
            st.push([p(-0.7, -0.45), p(0.7, -0.45)]);
        }
        "circular_runout" => {
            st.push([p(-1.0, -1.0), p(1.0, 1.0)]);
            st.extend(barbs(p(1.0, 1.0), p(-1.0, -1.0), 0.5 * s));
        }
        _ => {
            for dx in [-0.25, 0.35] {
                let (a, b) = (p(-1.0 + dx, -1.0), p(0.4 + dx, 1.0));
                st.push([a, b]);
                st.extend(barbs(b, a, 0.45 * s));
            }
        }
    }
    laid.strokes.extend(st);
    laid.arcs.extend(arcs);
}

/// The gdt-font letter CAD uses for each characteristic symbol and modifier.
fn gdt_letter(symbol: char) -> Option<char> {
    Some(match symbol {
        '∠' => 'a',
        '⟂' => 'b',
        '⏥' => 'c',
        '⌓' => 'd',
        '○' => 'e',
        '∥' => 'f',
        '⌭' => 'g',
        '↗' => 'h',
        '⌯' => 'i',
        '⌖' => 'j',
        '⌒' => 'k',
        'Ⓛ' => 'l',
        'Ⓜ' => 'm',
        '⌀' => 'n',
        'Ⓟ' => 'p',
        '◎' => 'r',
        'Ⓢ' => 's',
        '⌰' => 't',
        '⏤' => 'u',
        _ => return None,
    })
}

/// A local ISO 1101 frame from plain cells, independent of `PartPmi`.
/// `zone` is `diameter` or `spherical_diameter`; `material` is `M` or `L`.
/// Datums are already formatted compartments (for example `A-B`).
///
/// The frame is laid out as a CAD system lays out a native TOLERANCE, with the same fonts
/// (QCAD's public-domain `gdt` and `Standard`, from which this crate ports the glyphs) and the
/// same rule: text height [`TEXT_HEIGHT`], box height twice that, and every cell as wide as its
/// ink plus the text height, the ink starting half a text height inside the cell. A frame
/// drawn here and the one a CAD system draws from the same cells therefore have the same size.
/// Cells are the symbol, the tolerance (zone symbol, value, material modifier) and one per
/// datum compartment. A tolerance cell with no ink (no value) is kept, one text height wide, so
/// that readable cells keep their positions.
///
/// The text is geometry: `strokes` (after the first `frame_strokes`) and `arcs` hold every
/// glyph, and `letters` is empty. `runs`, `cells` and `cell_boxes` are the readable layer.
pub fn cad_control_frame(
    name: &str,
    tolerance: &str,
    zone: Option<&str>,
    material: Option<&str>,
    datums: &[String],
) -> Laid {
    let mut laid = Laid::default();
    let sym = symbol(name);
    let letter = |s: &str| {
        s.chars()
            .next()
            .and_then(gdt_letter)
            .unwrap_or('t')
            .to_string()
    };
    let sym_letter = letter(sym);
    let zone_letter = 'n'.to_string();
    let mut tolerance_parts = Vec::new();
    match zone {
        Some("diameter") => tolerance_parts.push((Face::Gdt, zone_letter.as_str())),
        Some(_) => {
            tolerance_parts.push((Face::Standard, "S"));
            tolerance_parts.push((Face::Gdt, zone_letter.as_str()));
        }
        None => {}
    }
    tolerance_parts.push((Face::Standard, tolerance));
    let material_letter = material.map(|m| if m == "M" { "m" } else { "l" });
    if let Some(m) = material_letter {
        tolerance_parts.push((Face::Gdt, m));
    }
    let mut cells = vec![
        cxf::lay(&[(Face::Gdt, &sym_letter)]),
        cxf::lay(&tolerance_parts),
    ];
    cells.extend(datums.iter().map(|d| cxf::lay(&[(Face::Standard, d)])));
    let mut xs = vec![0.0];
    for c in &cells {
        xs.push(xs[xs.len() - 1] + c.width(TEXT_HEIGHT) + TEXT_HEIGHT);
    }
    // Invariant: xs starts with the left edge and has one entry per cell after it.
    let w = xs[xs.len() - 1];
    let corners = [[0.0, 0.0], [w, 0.0], [w, CAD_H], [0.0, CAD_H]];
    for i in 0..4 {
        laid.strokes.push([corners[i], corners[(i + 1) % 4]]);
    }
    for x in &xs[1..xs.len() - 1] {
        laid.strokes.push([[*x, 0.0], [*x, CAD_H]]);
    }
    laid.frame_strokes = laid.strokes.len();
    laid.text_symbols = true;
    laid.cell_boxes = xs.windows(2).map(|x| [[x[0], 0.], [x[1], CAD_H]]).collect();
    // Text is centred on its capital height: the baseline is half a text height below the
    // middle of the box.
    for (x, c) in xs.iter().zip(&cells) {
        c.place(
            TEXT_HEIGHT,
            [x + TEXT_HEIGHT / 2.0, CAD_H / 2.0 - TEXT_HEIGHT / 2.0],
            &mut laid.strokes,
            &mut laid.arcs,
        );
    }
    let run = format!(
        "{}{tolerance}{}",
        match zone {
            Some("diameter") => "⌀",
            Some(_) => "S⌀",
            None => "",
        },
        match material {
            Some("M") => "Ⓜ",
            Some(_) => "Ⓛ",
            None => "",
        }
    );
    let mut texts = vec![sym.to_string(), run];
    texts.extend(datums.iter().cloned());
    for (t, b) in texts.iter().zip(&laid.cell_boxes) {
        laid.runs.push(text(
            t,
            [(b[0][0] + b[1][0]) / 2.0, CAD_H / 2.0],
            TEXT_HEIGHT,
        ));
    }
    laid.size = [w, CAD_H];
    laid.cells = texts;
    laid
}

/// A datum's frame laid out about its centre: a box twice the text height on a side (wider
/// only if the letters need it, each side ink plus the text height), the letter centred, in
/// the same font and height as the control frame. Geometry only, like `cad_control_frame`.
pub fn cad_datum_frame(letter: &str) -> Laid {
    let ink = cxf::lay(&[(Face::Standard, letter)]);
    let w = CAD_H.max(ink.width(TEXT_HEIGHT) + TEXT_HEIGHT);
    let mut laid = Laid::default();
    let corners = [
        [-w / 2., -TEXT_HEIGHT],
        [w / 2., -TEXT_HEIGHT],
        [w / 2., TEXT_HEIGHT],
        [-w / 2., TEXT_HEIGHT],
    ];
    for i in 0..4 {
        laid.strokes.push([corners[i], corners[(i + 1) % 4]]);
    }
    laid.frame_strokes = 4;
    laid.text_symbols = true;
    laid.cell_boxes = vec![[[-w / 2., -TEXT_HEIGHT], [w / 2., TEXT_HEIGHT]]];
    ink.place(
        TEXT_HEIGHT,
        [-ink.width(TEXT_HEIGHT) / 2., -TEXT_HEIGHT / 2.],
        &mut laid.strokes,
        &mut laid.arcs,
    );
    laid.runs.push(text(letter, [0.0, 0.0], TEXT_HEIGHT));
    laid.cells = vec![letter.to_string()];
    laid.size = [w, CAD_H];
    laid
}

/// A local ISO 1101 frame from plain cells, independent of `PartPmi`.
/// `width` measures text ink at the supplied capital height in drawing units.
/// `zone` is `diameter` or `spherical_diameter`; `material` is `M` or `L`.
/// Datums are already formatted compartments (for example `A-B`).
#[deprecated(
    note = "lays out with the caller's font and fixed 6 mm cells, which differ from CAD's; use `cad_control_frame`"
)]
pub fn control_frame(
    name: &str,
    tolerance: &str,
    zone: Option<&str>,
    material: Option<&str>,
    datums: &[String],
    width: &dyn Fn(&str, f64) -> f64,
) -> Laid {
    let mut laid = Laid::default();
    let cy = H / 2.0;
    let spherical = zone == Some("spherical_diameter");
    let diameter = zone == Some("diameter");
    // `⌀` is drawn as a ring and a stroke; `S⌀` as the font's `Sø` before the value.
    let ink = if spherical {
        format!("Sø{tolerance}")
    } else {
        tolerance.to_string()
    };
    let tw = width(&ink, TEXT);
    let w_tol = PAD
        + tw
        + PAD
        + if diameter { 2.0 * ZONE_R + PAD } else { 0.0 }
        + if material.is_some() {
            2.0 * MATERIAL_R + PAD
        } else {
            0.0
        };
    let mut xs = vec![0.0, H, H + w_tol];
    for _ in datums {
        // Invariant: xs starts with the left edge and grows once per frame cell.
        xs.push(xs[xs.len() - 1] + H);
    }
    // Invariant: xs starts with the left edge and grows once per frame cell.
    let w = xs[xs.len() - 1];
    let corners = [[0.0, 0.0], [w, 0.0], [w, H], [0.0, H]];
    for i in 0..4 {
        laid.strokes.push([corners[i], corners[(i + 1) % 4]]);
    }
    for x in &xs[1..xs.len() - 1] {
        laid.strokes.push([[*x, 0.0], [*x, H]]);
    }
    laid.frame_strokes = laid.strokes.len();
    laid.text_symbols = true;
    laid.cell_boxes = xs.windows(2).map(|x| [[x[0], 0.], [x[1], H]]).collect();
    characteristic(name, [H / 2.0, cy], &mut laid);
    let mut cells = vec![symbol(name).to_string()];
    laid.runs.push(text(symbol(name), [H / 2.0, cy], TEXT));
    let mut x = H + PAD;
    if diameter {
        let c = [x + ZONE_R, cy];
        laid.arcs.push(circle(c, ZONE_R));
        laid.strokes.push([
            [c[0] + 0.9 * ZONE_R, cy - 0.9 * ZONE_R],
            [c[0] - 0.9 * ZONE_R, cy + 0.9 * ZONE_R],
        ]);
        x = c[0] + ZONE_R + PAD;
    }
    laid.letters.push(text(&ink, [x + tw / 2.0, cy], TEXT));
    x += tw + PAD;
    if let Some(m) = material {
        let c = [x + MATERIAL_R, cy];
        laid.arcs.push(circle(c, MATERIAL_R));
        laid.letters.push(text(m, c, 0.8 * TEXT));
    }
    let run = format!(
        "{}{tolerance}{}",
        match zone {
            Some("diameter") => "⌀",
            Some(_) => "S⌀",
            None => "",
        },
        match material {
            Some("M") => "Ⓜ",
            Some(_) => "Ⓛ",
            None => "",
        }
    );
    laid.runs.push(text(&run, [H + w_tol / 2.0, cy], TEXT));
    cells.push(run);
    for (i, d) in datums.iter().enumerate() {
        let c = [xs[2 + i] + H / 2.0, cy];
        laid.letters.push(text(d, c, TEXT));
        laid.runs.push(text(d, c, TEXT));
        cells.push(d.clone());
    }
    laid.size = [w, H];
    laid.cells = cells;
    laid
}

/// A datum's frame laid out about its centre.
#[deprecated(note = "use `cad_datum_frame`")]
pub fn datum_frame(letter: &str) -> Laid {
    let h = TEXT;
    let corners = [[-h, -h], [h, -h], [h, h], [-h, h]];
    let mut laid = Laid::default();
    for i in 0..4 {
        laid.strokes.push([corners[i], corners[(i + 1) % 4]]);
    }
    laid.frame_strokes = 4;
    laid.cell_boxes = vec![[[-h, -h], [h, h]]];
    laid.letters.push(text(letter, [0.0, 0.0], h));
    laid.runs.push(text(letter, [0.0, 0.0], h));
    laid.cells = vec![letter.to_string()];
    laid.size = [H, H];
    laid
}

/// Standalone text-layer coverage for every characteristic the frame renderer emits: the
/// `gdt` font's glyph, centred and sized to fill most of the 24-unit cell.
pub(super) fn glyph(c: char) -> Option<(f64, Vec<Vec<P2>>)> {
    let letter = gdt_letter(c).filter(|l| !matches!(l, 'l' | 'm' | 'n' | 'p' | 's'))?;
    let (ink, [x0, y0, x1, y1]) = cxf::symbol(letter)?;
    let k = 17.6 / (x1 - x0).max(y1 - y0);
    let at = |p: P2| {
        [
            10.5 + (p[0] - (x0 + x1) / 2.) * k,
            10.5 + (p[1] - (y0 + y1) / 2.) * k,
        ]
    };
    let mut lines: Vec<Vec<P2>> = ink
        .lines
        .iter()
        .map(|l| l.iter().map(|&p| at(p)).collect())
        .collect();
    for &[x, y, r, a, b] in &ink.arcs {
        lines.push(
            (0..=32)
                .map(|i| {
                    let t = (a + (b - a) * i as f64 / 32.).to_radians();
                    at([x + r * t.cos(), y + r * t.sin()])
                })
                .collect(),
        );
    }
    Some((24., lines))
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;

    // Content assertions ported from Draftwright tests/spec_gdt.rs. Authoring and placement
    // checks stay there; the same requirements here are already semantic PMI.
    #[test]
    fn position_frame_and_datum_state_the_same_cells_as_draftwright() {
        let frame = control_frame(
            "position",
            "0.1",
            Some("diameter"),
            None,
            &["A".into()],
            &super::super::text_width,
        );
        assert_eq!(frame.cells.join(" | "), "⌖ | ⌀0.1 | A");
        assert_eq!(
            frame
                .runs
                .iter()
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>(),
            ["⌖", "⌀0.1", "A"]
        );
        assert_eq!(datum_frame("A").runs[0].text, "A");
        assert_eq!(frame.size[1], 6.0);
        assert_eq!(frame.arcs.len(), 2); // Position ring and diameter ring.
        assert!(frame.strokes.len() >= 9); // Box, cells, crosshair and diameter stroke.
    }
    #[test]
    fn a_flatness_control_on_a_face_draws_its_frame() {
        let frame = control_frame(
            "flatness",
            "0.05",
            None,
            None,
            &[],
            &super::super::text_width,
        );
        assert_eq!(frame.cells.join(" | "), "⏥ | 0.05");
        assert_eq!(
            frame
                .runs
                .iter()
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>(),
            ["⏥", "0.05"]
        );
        assert_eq!(frame.strokes.len(), 9); // Box, separator, four-sided flatness symbol.
    }
    #[test]
    fn spherical_zone_and_material_condition_have_real_ink() {
        let frame = control_frame(
            "position",
            "0.0254",
            Some("spherical_diameter"),
            Some("M"),
            &["A".into(), "B".into()],
            &super::super::text_width,
        );
        assert_eq!(frame.cells, ["⌖", "S⌀0.0254Ⓜ", "A", "B"]);
        assert_eq!(frame.letters[0].text, "Sø0.0254");
        assert_eq!(frame.letters[1].text, "M");
        assert_eq!(frame.arcs.len(), 2); // Position and material rings.
    }
}

#[cfg(test)]
mod cad_tests {
    use super::*;

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }
    /// Frame size and cell widths, in the order symbol, tolerance, datums.
    fn widths(frame: &Laid) -> Vec<f64> {
        frame.cell_boxes.iter().map(|b| b[1][0] - b[0][0]).collect()
    }
    fn close(got: &[f64], want: &[f64]) {
        assert_eq!(got.len(), want.len(), "{got:?} vs {want:?}");
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() < 0.01, "{got:?} vs {want:?}");
        }
    }
    /// Ink strictly after the frame lines: `strokes` then `arcs`, as points.
    fn ink_points(frame: &Laid) -> Vec<P2> {
        let mut pts: Vec<P2> = frame.strokes[frame.frame_strokes..]
            .iter()
            .flatten()
            .copied()
            .collect();
        for &[x, y, r, a1, a2] in &frame.arcs {
            let at = |a: f64| [x + r * a.to_radians().cos(), y + r * a.to_radians().sin()];
            pts.extend([at(a1), at(a2)]);
            for axis in [0., 90., 180., 270.] {
                if axis + 360. * ((a1 - axis) / 360.).ceil() <= a2 {
                    pts.push(at(axis));
                }
            }
        }
        pts
    }
    fn inside(frame: &Laid, cell: usize, pts: &[P2]) -> bool {
        let b = frame.cell_boxes[cell];
        pts.iter().all(|p| {
            p[0] >= b[0][0] - 1e-9
                && p[0] <= b[1][0] + 1e-9
                && p[1] >= b[0][1] - 1e-9
                && p[1] <= b[1][1] + 1e-9
        })
    }

    // Content assertions ported from Draftwright tests/spec_gdt.rs. Authoring and placement
    // checks stay there; the same requirements here are already semantic PMI.
    #[test]
    fn position_frame_and_datum_state_the_same_cells_as_draftwright() {
        let frame = cad_control_frame("position", "0.1", Some("diameter"), None, &["A".into()]);
        assert_eq!(frame.cells.join(" | "), "⌖ | ⌀0.1 | A");
        assert_eq!(
            frame
                .runs
                .iter()
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>(),
            ["⌖", "⌀0.1", "A"]
        );
        assert_eq!(cad_datum_frame("A").runs[0].text, "A");
        assert_eq!(frame.size[1], 5.0);
        assert_eq!(frame.arcs.iter().filter(|a| a[4] - a[3] == 360.).count(), 2); // Position and diameter rings.
        assert!(frame.strokes.len() >= 9); // Box, cells, crosshair and diameter stroke.
    }
    #[test]
    fn a_flatness_control_on_a_face_draws_its_frame() {
        let frame = cad_control_frame("flatness", "0.05", None, None, &[]);
        assert_eq!(frame.cells.join(" | "), "⏥ | 0.05");
        assert_eq!(
            frame
                .runs
                .iter()
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>(),
            ["⏥", "0.05"]
        );
        assert_eq!(frame.frame_strokes, 5); // Box and one separator.
        assert!(frame.strokes.len() > 9); // Plus the four-sided symbol and the digits.
        assert_eq!(frame.cell_boxes.len(), 2); // A frame with no datums has two cells.
        assert!(frame.letters.is_empty());
    }
    #[test]
    fn spherical_zone_and_material_condition_have_real_ink() {
        let frame = cad_control_frame(
            "position",
            "0.0254",
            Some("spherical_diameter"),
            Some("M"),
            &["A".into(), "B".into()],
        );
        assert_eq!(frame.cells, ["⌖", "S⌀0.0254Ⓜ", "A", "B"]);
        assert!(frame.letters.is_empty());
        // Position, diameter and material rings are whole circles.
        assert!(frame.arcs.iter().filter(|a| a[4] - a[3] == 360.).count() >= 3);
    }

    // Widths QCAD 3.33 gives the same native TOLERANCE text at 3.0 mm, times 2.5 / 3.
    #[test]
    fn frame_widths_equal_those_a_cad_system_draws() {
        let ds = |d: &[&str]| strings(d);
        let cases: [(
            &str,
            &str,
            Option<&str>,
            Option<&str>,
            Vec<String>,
            f64,
            &[f64],
        ); 5] = [
            (
                "position",
                "1.016",
                Some("diameter"),
                Some("M"),
                ds(&["A", "B", "C"]),
                35.139,
                &[6.25, 17.222, 4.167, 3.889, 3.611],
            ),
            (
                "flatness",
                "0.254",
                None,
                None,
                ds(&[]),
                18.194,
                &[7.639, 10.556],
            ),
            (
                "profile_surface",
                "0.5",
                None,
                None,
                ds(&["A"]),
                18.056,
                &[7.5, 6.389, 4.167],
            ),
            (
                "perpendicularity",
                "0.3048",
                None,
                None,
                ds(&["A", "B"]),
                28.056,
                &[7.5, 12.5, 4.167, 3.889],
            ),
            (
                "position",
                "0.635",
                Some("spherical_diameter"),
                None,
                ds(&["D", "B", "C"]),
                33.472,
                &[6.25, 15.833, 3.889, 3.889, 3.611],
            ),
        ];
        for (name, tol, zone, material, datums, total, cells) in cases {
            let frame = cad_control_frame(name, tol, zone, material, &datums);
            close(&widths(&frame), cells);
            close(&[frame.size[0]], &[total]);
            assert_eq!(frame.size[1], 5.0);
            assert!(frame.letters.is_empty());
        }
    }
    #[test]
    fn every_characteristic_and_modifier_draws_ink_inside_its_cell() {
        for name in [
            "straightness",
            "flatness",
            "circularity",
            "cylindricity",
            "profile_line",
            "profile_surface",
            "angularity",
            "perpendicularity",
            "parallelism",
            "position",
            "concentricity",
            "coaxiality",
            "symmetry",
            "circular_runout",
            "total_runout",
        ] {
            let frame = cad_control_frame(name, "0.1", None, None, &["A".into()]);
            // Symbol ink is whatever lies in the first cell; the value's ink lies in the next.
            let symbol_ink: Vec<P2> = ink_points(&frame)
                .into_iter()
                .filter(|p| p[0] < frame.cell_boxes[0][1][0])
                .collect();
            assert!(!symbol_ink.is_empty(), "{name}");
            assert!(inside(&frame, 0, &symbol_ink), "{name}");
        }
        // Zone and modifier symbols, alone in the tolerance cell with a value.
        for (zone, material) in [
            (Some("diameter"), None),
            (Some("spherical_diameter"), None),
            (None, Some("M")),
            (None, Some("L")),
        ] {
            let plain = cad_control_frame("flatness", "0.1", None, None, &[]);
            let frame = cad_control_frame("flatness", "0.1", zone, material, &[]);
            let (p, f) = (ink_points(&plain).len(), ink_points(&frame).len());
            assert!(f > p, "{zone:?} {material:?} adds ink");
            let tol_ink: Vec<P2> = ink_points(&frame)
                .into_iter()
                .filter(|p| p[0] >= frame.cell_boxes[1][0][0])
                .collect();
            assert!(inside(&frame, 1, &tol_ink), "{zone:?} {material:?}");
            // CAD's padding: ink starts half a text height in and ends half before the edge.
            let lo = tol_ink.iter().map(|p| p[0]).fold(f64::MAX, f64::min);
            let hi = tol_ink.iter().map(|p| p[0]).fold(f64::MIN, f64::max);
            let b = frame.cell_boxes[1];
            assert!((lo - b[0][0] - 1.25).abs() < 0.01, "{zone:?} {material:?}");
            assert!((b[1][0] - hi - 1.25).abs() < 0.01, "{zone:?} {material:?}");
        }
    }
    #[test]
    fn text_is_centred_on_its_capital_height_in_the_box() {
        let frame = cad_control_frame("flatness", "0.05", None, None, &[]);
        let digits: Vec<P2> = ink_points(&frame)
            .into_iter()
            .filter(|p| p[0] >= frame.cell_boxes[1][0][0])
            .collect();
        let lo = digits.iter().map(|p| p[1]).fold(f64::MAX, f64::min);
        let hi = digits.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
        assert!(
            (lo - 1.25).abs() < 0.01 && (hi - 3.75).abs() < 0.01,
            "{lo} {hi}"
        );
    }
    #[test]
    fn composite_datums_stay_one_cell() {
        let frame = cad_control_frame("position", "0.1", None, None, &["A-B".into(), "C".into()]);
        assert_eq!(frame.cells, ["⌖", "0.1", "A-B", "C"]);
        assert_eq!(frame.cell_boxes.len(), 4);
        // "A-B" at 2.5 mm: ink 17.5 font units * 2.5 / 9 plus 2.5.
        let ab = widths(&frame)[2];
        let single = widths(&cad_control_frame(
            "position",
            "0.1",
            None,
            None,
            &["A".into()],
        ))[2];
        assert!(ab > single + 2.0, "{ab} {single}");
        let ink: Vec<P2> = ink_points(&frame)
            .into_iter()
            .filter(|p| {
                let b = frame.cell_boxes[2];
                p[0] >= b[0][0] && p[0] <= b[1][0]
            })
            .collect();
        assert!(inside(&frame, 2, &ink));
    }
    #[test]
    fn datum_frame_is_a_square_with_the_letter_inside() {
        let d = cad_datum_frame("A");
        assert_eq!(d.size, [5.0, 5.0]);
        assert_eq!(d.cell_boxes, [[[-2.5, -2.5], [2.5, 2.5]]]);
        assert!(d.letters.is_empty());
        let ink: Vec<P2> = d.strokes[d.frame_strokes..]
            .iter()
            .flatten()
            .copied()
            .collect();
        assert!(!ink.is_empty());
        assert!(ink.iter().all(|p| p[0].abs() < 2.5 && p[1].abs() < 2.5));
        let (lo, hi) = ink
            .iter()
            .fold((f64::MAX, f64::MIN), |(l, h), p| (l.min(p[1]), h.max(p[1])));
        assert!((lo + 1.25).abs() < 0.01 && (hi - 1.25).abs() < 0.01);
    }
    #[test]
    fn text_height_and_pen_are_the_iso_3098_values() {
        assert_eq!((TEXT_HEIGHT, TEXT_PEN), (2.5, 0.25));
    }
    #[test]
    fn text_layer_glyphs_come_from_the_gdt_font() {
        for c in "⏤⏥○⌭⌒⌓∠⟂∥⌖◎⌯↗⌰".chars() {
            let (advance, lines) = glyph(c).unwrap();
            assert_eq!(advance, 24.);
            assert!(!lines.is_empty());
            assert!(
                lines
                    .iter()
                    .flatten()
                    .all(|p| p[0] > 0. && p[0] < 24. && p[1] > 0. && p[1] < 21.)
            );
        }
        assert!(glyph('x').is_none());
    }
}
