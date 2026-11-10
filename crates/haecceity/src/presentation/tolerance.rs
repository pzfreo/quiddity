//! Requirement notation ported from pzfreo/draftwright aaa7a73,
//! src/dims/tolerance.rs (Paul Fremantle; port authorized by the copyright owner for #137). Sheet precision and authoring stay there.
/// Supported hole letters in discovery/refusal order.
pub const HOLE_CLASSES: [&str; 3] = ["H", "G", "F"];
/// Supported shaft letters in discovery/refusal order.
pub const SHAFT_CLASSES: [&str; 7] = ["h", "g", "f", "js", "k", "n", "p"];
/// Supported ISO 286 grades, in increasing order.
pub const GRADES: [u32; 7] = [5, 6, 7, 8, 9, 10, 11];
/// Inclusive upper nominal diameter supported by the fit table, in millimetres.
pub const MAX_NOMINAL_MM: f64 = 250.;
/// An ISO 286 class from draftwright's table: its letters and grade, or why not.
pub fn fit(code: &str) -> Result<(&str, u32), String> {
    let letters = code.trim_end_matches(|c: char| c.is_ascii_digit());
    let grade = &code[letters.len()..];
    if letters.is_empty() || grade.is_empty() || !letters.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(format!(
            "{code:?} is not a fit: a fit is an ISO 286 class, letters and a grade (H7, g6)"
        ));
    }
    let grade: u32 = grade
        .parse()
        .map_err(|_| format!("{code:?} is not a fit"))?;
    if !GRADES.contains(&grade) {
        return Err(format!(
            "the fit {code:?} has grade IT{grade}, outside draftwright's table (IT5 to IT11)"
        ));
    }
    if !HOLE_CLASSES.contains(&letters) && !SHAFT_CLASSES.contains(&letters) {
        return Err(format!(
            "the fit {code:?} is not in draftwright's table (holes H, G, F; shafts h, g, f, js, k, n, p)"
        ));
    }
    Ok((letters, grade))
}

/// ISO 286 nominal size bands: the upper bound, mm, of each "over the previous, up to and
/// including this" step (draftwright's `fits._BANDS`).
const BANDS: [f64; 10] = [
    3.0,
    6.0,
    10.0,
    18.0,
    30.0,
    50.0,
    80.0,
    120.0,
    180.0,
    MAX_NOMINAL_MM,
];
/// Standard tolerance grades IT5 to IT11, µm, per band (ISO 286-1 Table 1; `fits._IT`).
const IT: [[i32; 10]; 7] = [
    [4, 5, 6, 8, 9, 11, 13, 15, 18, 20],
    [6, 8, 9, 11, 13, 16, 19, 22, 25, 29],
    [10, 12, 15, 18, 21, 25, 30, 35, 40, 46],
    [14, 18, 22, 27, 33, 39, 46, 54, 63, 72],
    [25, 30, 36, 43, 52, 62, 74, 87, 100, 115],
    [40, 48, 58, 70, 84, 100, 120, 140, 160, 185],
    [60, 75, 90, 110, 130, 160, 190, 220, 250, 290],
];
/// The shafts' fundamental upper deviation es, µm, per band, for the clearance letters (`fits._ES`);
/// the hole classes H, G and F mirror them (EI = -es).
const ES: [(&str, [i32; 10]); 3] = [
    ("h", [0; 10]),
    ("g", [-2, -4, -5, -6, -7, -9, -10, -12, -14, -15]),
    ("f", [-6, -10, -13, -16, -20, -25, -30, -36, -43, -50]),
];
/// The shafts' fundamental lower deviation ei, µm, per band, for the interference letters
/// (`fits._EI`).
const EI: [(&str, [i32; 10]); 3] = [
    ("k", [0, 1, 1, 1, 2, 2, 2, 3, 3, 4]),
    ("n", [4, 8, 10, 12, 15, 17, 20, 23, 27, 31]),
    ("p", [6, 12, 15, 18, 22, 26, 32, 37, 43, 50]),
];

/// A class's limit deviations at a nominal diameter, mm: `(lower, upper)`, as draftwright's
/// `fits.fit_deviation` gives them. None outside the table (a class or grade it does not hold,
/// a nominal not over 0 or above 250 mm).
pub fn deviations(code: &str, nominal: f64) -> Option<(f64, f64)> {
    let (letters, grade) = fit(code).ok()?;
    let band = BANDS.iter().position(|&b| nominal > 0.0 && nominal <= b)?;
    let it = IT[(grade - 5) as usize][band];
    let table =
        |t: &[(&str, [i32; 10])], l: &str| t.iter().find(|(k, _)| *k == l).map(|(_, v)| v[band]);
    let lower = letters.to_lowercase();
    let (lo, hi) = if HOLE_CLASSES.contains(&letters) {
        let ei = -table(&ES, &lower)?;
        (ei, ei + it)
    } else if letters == "js" {
        return Some((-f64::from(it) / 2000.0, f64::from(it) / 2000.0));
    } else if let Some(es) = table(&ES, letters) {
        (es - it, es)
    } else {
        // k's tabulated deviation holds for IT4 to IT7 only; ISO 286 makes it 0 for coarser
        // grades (`fits.fit_deviation`, #29).
        let ei = if letters == "k" && grade > 7 {
            0
        } else {
            table(&EI, letters)?
        };
        (ei, ei + it)
    };
    Some((f64::from(lo) / 1000.0, f64::from(hi) / 1000.0))
}

/// A class with its stacked deviations, encoded upper then lower with a newline.
pub fn with_limits(code: &str, (lower, upper): (f64, f64)) -> String {
    format!("{code} {}", limits_term((lower, upper)))
}

/// The stacked deviations alone, encoded upper then lower with a newline.
pub fn limits_term((lower, upper): (f64, f64)) -> String {
    format!("{}\n{}", deviation(upper), deviation(lower))
}

/// A limit deviation as a fit prints it (draftwright's `fits._fmt_dev`): signed, to the µm
/// digit, and an exact zero as `0`.
pub fn deviation(v: f64) -> String {
    if v.abs() < 5e-5 {
        return "0".into();
    }
    let s = format!("{v:+.4}");
    s.strip_suffix('0').map_or(s.clone(), str::to_string)
}

/// Signed deviations as Draftwright prints them, without rounding requirement digits.
pub fn band(upper: f64, lower: f64) -> String {
    let m = |v: f64| {
        let s = v.abs().to_string();
        if s.contains('.') { s } else { format!("{s}.0") }
    };
    let signed = |v: f64, sign: char| {
        if v == 0.0 {
            "0".to_string()
        } else {
            format!("{sign}{}", m(v))
        }
    };
    if upper > 0.0 && upper == -lower {
        format!("±{}", m(upper))
    } else {
        let up = if upper < 0.0 { '-' } else { '+' };
        let down = if lower > 0.0 { '+' } else { '-' };
        format!("{}/{}", signed(upper, up), signed(lower, down))
    }
}

/// The decimal-string form preserves authored trailing digits as well as unsigned zero.
pub fn band_text(upper: &crate::pmi::Decimal, lower: &crate::pmi::Decimal) -> String {
    use crate::pmi::Decimal;
    let printed = |s: &str| {
        // STEP's REAL spelling requires a point even with no fractional digits. Apply
        // the shared notation normalisation before Draftwright's .0 integer convention.
        let s = Decimal::parse(s).map_or_else(|_| s.to_owned(), |d| number(&d));
        if s.contains('.') || s.contains(['e', 'E']) {
            s
        } else {
            format!("{s}.0")
        }
    };
    let signed = |value: &Decimal| {
        let s = value.as_str();
        if value.is_zero() {
            "0".into()
        } else if s.starts_with('-') || s.starts_with('+') {
            printed(s)
        } else {
            format!("+{}", printed(s))
        }
    };
    if !upper.is_negative()
        && !upper.is_zero()
        && lower.is_negative()
        && Decimal::parse(lower.as_str().trim_start_matches('-'))
            .ok()
            .is_some_and(|abs| upper.cmp_value(&abs).is_eq())
    {
        format!("±{}", printed(upper.as_str().trim_start_matches('+')))
    } else {
        format!("{}/{}", signed(upper), signed(lower))
    }
}

/// A requirement decimal as notation: strip a STEP-required terminal decimal point, never
/// round digits or turn an exact requirement into a Rust float's Debug spelling.
pub fn number(d: &crate::pmi::Decimal) -> String {
    let s = d.as_str();
    if let Some((mantissa, exponent)) = s.split_once(['e', 'E']) {
        format!("{}E{exponent}", mantissa.trim_end_matches('.'))
    } else {
        s.trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imported_bands_print_as_draftwright_prints_them() {
        assert_eq!(band(0.2, -0.2), "±0.2");
        assert_eq!(band(0.09, 0.0), "+0.09/0");
        assert_eq!(band(0.0, -0.2), "0/-0.2");
        assert_eq!(band(0.0762, -0.0254), "+0.0762/-0.0254");
        assert_eq!(band(1.0, -1.0), "±1.0");
        assert_eq!(band(-0.014, -0.039), "-0.014/-0.039");
        assert_eq!(band(0.039, 0.014), "+0.039/+0.014");
    }

    #[test]
    fn fit_classes_have_iso_286_limits() {
        // ISO 286-2: ⌀10 H7 is +0.015/0, ⌀10 G7 +0.020/+0.005, ⌀30 F8 +0.053/+0.020; a
        // band's upper bound belongs to it (⌀3 H11 is +0.060/0).
        let at = |c, d| deviations(c, d).map(|(l, u)| (deviation(u), deviation(l)));
        assert_eq!(at("H7", 10.0), Some(("+0.015".into(), "0".into())));
        assert_eq!(at("G7", 10.0), Some(("+0.020".into(), "+0.005".into())));
        assert_eq!(at("F8", 30.0), Some(("+0.053".into(), "+0.020".into())));
        assert_eq!(at("H11", 3.0), Some(("+0.060".into(), "0".into())));
        assert_eq!(at("H7", 250.5), None);
        // Shafts (`fits.fit_deviation("g6", 20) == (-0.020, -0.007)`), js symmetric, k8 from 0.
        assert_eq!(at("g6", 20.0), Some(("-0.007".into(), "-0.020".into())));
        assert_eq!(at("js6", 20.0), Some(("+0.0065".into(), "-0.0065".into())));
        assert_eq!(at("k8", 20.0), Some(("+0.033".into(), "0".into())));
        assert_eq!(at("H4", 10.0), None);
    }

    #[test]
    fn fits_are_draftwrights_table() {
        assert_eq!(fit("H7"), Ok(("H", 7)));
        assert_eq!(fit("js6"), Ok(("js", 6)));
        for bad in ["H4", "H12", "K7", "N7", "x6", "7", "H", "H 7", "Hh7"] {
            assert!(fit(bad).is_err(), "{bad}");
        }
    }
}
