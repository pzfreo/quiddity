//! QCAD CXF stroke fonts for GD&T frames: the `Standard` face for text and the `gdt` face for
//! the ISO 1101 symbols, laid out the way a CAD system lays out a native TOLERANCE.
//!
//! `fonts/Standard.cxf` and `fonts/gdt.cxf` are copied unchanged from QCAD 3 (author Andrew
//! Mustun, RibbonSoft). QCAD's licence file states that its CXF fonts in the `fonts` directory
//! are released as public domain, with all copyright waived. See `NOTICE`.
//!
//! Font units: capital height is 9, glyphs are separated by 3 units (a glyph's greatest ink x
//! plus 3 is its advance) and a space advances 6.75. A string's ink runs from its leftmost ink
//! x to its rightmost ink x. Callers scale by `height / 9`.
use std::collections::HashMap;
use std::sync::OnceLock;

type P2 = [f64; 2];

const SPACING: f64 = 3.0;
const WORD_SPACING: f64 = 6.75;
/// Capital height of both faces, in font units.
pub(super) const CAP: f64 = 9.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Face {
    Standard,
    Gdt,
}

/// Strokes and arcs (centre x/y, radius, start/end degrees counter-clockwise).
#[derive(Default, Clone, Debug)]
pub(super) struct Ink {
    pub lines: Vec<[P2; 2]>,
    pub arcs: Vec<[f64; 5]>,
}

#[derive(Default, Clone, Debug)]
struct Glyph {
    ink: Ink,
    /// `[min x, min y, max x, max y]` of the ink; `None` when the glyph has none.
    bounds: Option<[f64; 4]>,
}

fn grow(b: &mut Option<[f64; 4]>, p: P2) {
    let b = b.get_or_insert([p[0], p[1], p[0], p[1]]);
    b[0] = b[0].min(p[0]);
    b[1] = b[1].min(p[1]);
    b[2] = b[2].max(p[0]);
    b[3] = b[3].max(p[1]);
}

fn arc_bounds(b: &mut Option<[f64; 4]>, [cx, cy, r, a1, a2]: [f64; 5]) {
    for a in [a1, a2] {
        let t = a.to_radians();
        grow(b, [cx + r * t.cos(), cy + r * t.sin()]);
    }
    // Axis extremes inside the sweep.
    for (k, p) in [[r, 0.], [0., r], [-r, 0.], [0., -r]]
        .into_iter()
        .enumerate()
    {
        let axis = 90. * k as f64;
        let n = ((a1 - axis) / 360.).ceil();
        if axis + 360. * n <= a2 {
            grow(b, [cx + p[0], cy + p[1]]);
        }
    }
}

/// The arc a CAD polyline draws between two vertices. The bulge is the tangent of a quarter of
/// the included angle; positive runs counter-clockwise.
fn bulge_arc(p: P2, q: P2, bulge: f64) -> [f64; 5] {
    let sweep = 4. * bulge.atan();
    let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
    let d = dx.hypot(dy);
    let r = d / (2. * (sweep.abs() / 2.).sin());
    let off = r * (sweep.abs() / 2.).cos() * bulge.signum();
    let c = [
        (p[0] + q[0]) / 2. - off * dy / d,
        (p[1] + q[1]) / 2. + off * dx / d,
    ];
    let a1 = (p[1] - c[1]).atan2(p[0] - c[0]).to_degrees();
    let (s, e) = if sweep > 0. {
        (a1, a1 + sweep.to_degrees())
    } else {
        (a1 + sweep.to_degrees(), a1)
    };
    let shift = 360. * (s / 360.).floor();
    [c[0], c[1], r, s - shift, e - shift]
}

fn finish(mut g: Glyph) -> Glyph {
    // A closed pair of half-turns about one centre is the circle it draws.
    if let [a, b] = g.ink.arcs[..]
        && (a[..3].iter().zip(&b[..3])).all(|(x, y)| (x - y).abs() < 1e-9)
        && ((a[4] - a[3]) + (b[4] - b[3]) - 360.).abs() < 1e-9
        // Complementary: each half starts where the other ends.
        && (a[3] - b[4]).rem_euclid(360.).min((b[4] - a[3]).rem_euclid(360.)) < 1e-9
        && (b[3] - a[4]).rem_euclid(360.).min((a[4] - b[3]).rem_euclid(360.)) < 1e-9
    {
        g.ink.arcs = vec![[a[0], a[1], a[2], 0., 360.]];
    }
    let mut bounds = None;
    for l in &g.ink.lines {
        l.iter().for_each(|&p| grow(&mut bounds, p));
    }
    for &a in &g.ink.arcs {
        arc_bounds(&mut bounds, a);
    }
    g.bounds = bounds;
    g
}

fn parse(src: &str) -> HashMap<char, Glyph> {
    let mut glyphs = HashMap::new();
    let mut current: Option<(char, Glyph)> = None;
    for line in src.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            if let Some((c, g)) = current.take() {
                glyphs.insert(c, finish(g));
            }
            current = u32::from_str_radix(rest.split(']').next().unwrap_or(""), 16)
                .ok()
                .and_then(char::from_u32)
                .map(|c| (c, Glyph::default()));
            continue;
        }
        let Some((_, g)) = current.as_mut() else {
            continue;
        };
        let (cmd, args) = line.split_once(' ').unwrap_or((line, ""));
        let n: Vec<f64> = args
            .split(',')
            .filter_map(|v| v.trim().parse().ok())
            .collect();
        match (cmd, n.len()) {
            ("L", 4) => g.ink.lines.push([[n[0], n[1]], [n[2], n[3]]]),
            ("A" | "AR", 5) => {
                // AR runs clockwise from the first angle to the second.
                let (a1, a2) = if cmd == "AR" {
                    (n[4], n[3])
                } else {
                    (n[3], n[4])
                };
                let a2 = if a2 < a1 { a2 + 360. } else { a2 };
                g.ink.arcs.push([n[0], n[1], n[2], a1, a2]);
            }
            ("PL" | "PLC", len) if len % 3 == 0 && len >= 6 => {
                let v: Vec<[f64; 3]> = n.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
                let mut pairs: Vec<_> = v.windows(2).map(|w| (w[0], w[1])).collect();
                if cmd == "PLC" {
                    pairs.push((v[v.len() - 1], v[0]));
                }
                for (a, b) in pairs {
                    if a[2].abs() < 1e-9 {
                        g.ink.lines.push([[a[0], a[1]], [b[0], b[1]]]);
                    } else {
                        g.ink.arcs.push(bulge_arc([a[0], a[1]], [b[0], b[1]], a[2]));
                    }
                }
            }
            _ => {}
        }
    }
    if let Some((c, g)) = current {
        glyphs.insert(c, finish(g));
    }
    glyphs
}

fn glyphs(face: Face) -> &'static HashMap<char, Glyph> {
    static STANDARD: OnceLock<HashMap<char, Glyph>> = OnceLock::new();
    static GDT: OnceLock<HashMap<char, Glyph>> = OnceLock::new();
    match face {
        Face::Standard => STANDARD.get_or_init(|| parse(include_str!("fonts/Standard.cxf"))),
        Face::Gdt => GDT.get_or_init(|| parse(include_str!("fonts/gdt.cxf"))),
    }
}

fn lookup(face: Face, c: char) -> Option<&'static Glyph> {
    // The gdt font keeps its last symbol at U+0080 rather than at `z`.
    let c = if face == Face::Gdt && c == 'z' {
        '\u{80}'
    } else {
        c
    };
    glyphs(face).get(&c).filter(|g| g.bounds.is_some())
}

/// A laid-out string in font units: ink from its start, and its horizontal ink extent.
#[derive(Default, Clone, Debug)]
pub(super) struct Laid {
    pub ink: Ink,
    /// Leftmost and rightmost ink x; both zero for a string with no ink.
    pub x: [f64; 2],
    /// Characters the face lacks, in order. They add no ink and no advance.
    pub missing: Vec<char>,
}

impl Laid {
    /// Ink width in drawing units at capital height `height`.
    pub fn width(&self, height: f64) -> f64 {
        (self.x[1] - self.x[0]) * height / CAP
    }

    /// Append the ink at capital height `height`, its leftmost ink at `origin[0]` and its
    /// baseline at `origin[1]`.
    pub fn place(
        &self,
        height: f64,
        origin: P2,
        strokes: &mut Vec<[P2; 2]>,
        arcs: &mut Vec<[f64; 5]>,
    ) {
        let k = height / CAP;
        let at = |p: P2| [(p[0] - self.x[0]) * k + origin[0], p[1] * k + origin[1]];
        strokes.extend(self.ink.lines.iter().map(|l| [at(l[0]), at(l[1])]));
        arcs.extend(self.ink.arcs.iter().map(|a| {
            let c = at([a[0], a[1]]);
            [c[0], c[1], a[2] * k, a[3], a[4]]
        }));
    }
}

/// Lay out the parts glyph by glyph as CAD does. Characters a face lacks are recorded in
/// `missing`, never substituted.
pub(super) fn lay(parts: &[(Face, &str)]) -> Laid {
    let mut out = Laid::default();
    let (mut pen, mut lo, mut hi) = (0.0_f64, f64::INFINITY, f64::NEG_INFINITY);
    for &(face, text) in parts {
        for c in text.chars() {
            if c == ' ' {
                pen += WORD_SPACING;
                continue;
            }
            let Some(g) = lookup(face, c) else {
                out.missing.push(c);
                continue;
            };
            let b = g.bounds.unwrap_or_default();
            lo = lo.min(pen + b[0]);
            hi = hi.max(pen + b[2]);
            out.ink
                .lines
                .extend(g.ink.lines.iter().map(|l| l.map(|p| [p[0] + pen, p[1]])));
            out.ink.arcs.extend(
                g.ink
                    .arcs
                    .iter()
                    .map(|a| [a[0] + pen, a[1], a[2], a[3], a[4]]),
            );
            pen += b[2] + SPACING;
        }
    }
    if lo <= hi {
        out.x = [lo, hi];
    }
    out
}

/// One gdt glyph's ink and `[min x, min y, max x, max y]`, in font units.
pub(super) fn symbol(letter: char) -> Option<(&'static Ink, [f64; 4])> {
    let g = lookup(Face::Gdt, letter)?;
    Some((&g.ink, g.bounds?))
}
