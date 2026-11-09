//! Frame and characteristic geometry ported from pzfreo/draftwright aaa7a73,
//! src/dims/gdt.rs (Paul Fremantle; port authorized by the copyright owner for #137), without sheet planning or routing.
use super::{Callout as Laid, Text as GdtText};
type P2 = [f64; 2];
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

/// A local ISO 1101 frame from plain cells, independent of `PartPmi`.
/// `width` measures text ink at the supplied capital height in drawing units.
/// `zone` is `diameter` or `spherical_diameter`; `material` is `M` or `L`.
/// Datums are already formatted compartments (for example `A-B`).
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

/// Standalone text-layer coverage for every characteristic the frame renderer emits.
pub(super) fn glyph(c: char) -> Option<(f64, Vec<Vec<P2>>)> {
    let name = match c {
        '⏤' => "straightness",
        '⏥' => "flatness",
        '○' => "circularity",
        '⌭' => "cylindricity",
        '⌒' => "profile_line",
        '⌓' => "profile_surface",
        '∠' => "angularity",
        '⟂' => "perpendicularity",
        '∥' => "parallelism",
        '⌖' => "position",
        '◎' => "concentricity",
        '⌯' => "symmetry",
        '↗' => "circular_runout",
        '⌰' => "total_runout",
        _ => return None,
    };
    let mut laid = Laid::default();
    characteristic(name, [1.5, 1.5], &mut laid);
    let mut lines: Vec<_> = laid.strokes.iter().map(|s| s.to_vec()).collect();
    for [x, y, r, a, b] in laid.arcs {
        lines.push(
            (0..=32)
                .map(|i| {
                    let t = (a + (b - a) * i as f64 / 32.).to_radians();
                    [x + r * t.cos(), y + r * t.sin()]
                })
                .collect(),
        );
    }
    for p in lines.iter_mut().flatten() {
        p[0] *= 7.;
        p[1] *= 7.;
    }
    Some((24., lines))
}

#[cfg(test)]
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
