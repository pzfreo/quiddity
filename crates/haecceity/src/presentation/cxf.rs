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

/// Strokes, and whole circles as arcs (centre x/y, radius, 0, 360). Every other font arc is
/// already straight strokes.
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

/// A counter-clockwise arc: centre, radius, start angle and sweep in radians.
#[derive(Clone, Copy)]
struct Arc {
    c: P2,
    r: f64,
    start: f64,
    sweep: f64,
}

impl Arc {
    fn at(&self, t: f64) -> P2 {
        let a = self.start + self.sweep * t;
        [
            self.c[0] + self.r * libm::cos(a),
            self.c[1] + self.r * libm::sin(a),
        ]
    }

    /// Exact extent: the end points and the axis extremes the sweep passes through.
    fn bounds(&self, b: &mut Option<[f64; 4]>) {
        grow(b, self.at(0.));
        grow(b, self.at(1.));
        let (c, r) = (self.c, self.r);
        for (k, p) in [[r, 0.], [0., r], [-r, 0.], [0., -r]]
            .into_iter()
            .enumerate()
        {
            let axis = k as f64 * std::f64::consts::FRAC_PI_2;
            let turns = libm::ceil((self.start - axis) / std::f64::consts::TAU);
            if axis + std::f64::consts::TAU * turns <= self.start + self.sweep {
                grow(b, [c[0] + p[0], c[1] + p[1]]);
            }
        }
    }

    /// Straight strokes, 32 to a full turn: the font's arcs reach callers as lines, so no
    /// platform trigonometry decides a written coordinate.
    fn strokes(&self, out: &mut Vec<[P2; 2]>) {
        let n = (libm::ceil(32. * self.sweep.abs() / std::f64::consts::TAU) as usize).max(2);
        let mut prev = self.at(0.);
        for i in 1..=n {
            let next = self.at(i as f64 / n as f64);
            out.push([prev, next]);
            prev = next;
        }
    }
}

/// The arc a CAD polyline draws between two vertices. The bulge is the tangent of a quarter of
/// the included angle; positive runs counter-clockwise.
fn bulge_arc(p: P2, q: P2, bulge: f64) -> Arc {
    let sweep = 4. * libm::atan(bulge);
    let half = sweep.abs() / 2.;
    let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
    let d = libm::sqrt(dx * dx + dy * dy);
    let r = d / (2. * libm::sin(half));
    let off = r * libm::cos(half) * bulge.signum();
    let c = [
        (p[0] + q[0]) / 2. - off * dy / d,
        (p[1] + q[1]) / 2. + off * dx / d,
    ];
    let a1 = libm::atan2(p[1] - c[1], p[0] - c[0]);
    Arc {
        c,
        r,
        start: if sweep > 0. { a1 } else { a1 + sweep },
        sweep: sweep.abs(),
    }
}

#[derive(Default)]
struct Builder {
    glyph: Glyph,
    bounds: Option<[f64; 4]>,
}

impl Builder {
    fn line(&mut self, a: P2, b: P2) {
        grow(&mut self.bounds, a);
        grow(&mut self.bounds, b);
        self.glyph.ink.lines.push([a, b]);
    }
    fn arc(&mut self, arc: Arc) {
        arc.bounds(&mut self.bounds);
        arc.strokes(&mut self.glyph.ink.lines);
    }
    /// A whole circle stays an arc, from the font's own numbers.
    fn circle(&mut self, c: P2, r: f64) {
        grow(&mut self.bounds, [c[0] - r, c[1] - r]);
        grow(&mut self.bounds, [c[0] + r, c[1] + r]);
        self.glyph.ink.arcs.push([c[0], c[1], r, 0., 360.]);
    }
    fn finish(mut self) -> Glyph {
        self.glyph.bounds = self.bounds;
        self.glyph
    }
}

fn parse(src: &str) -> HashMap<char, Glyph> {
    let mut glyphs = HashMap::new();
    let mut current: Option<(char, Builder)> = None;
    for line in src.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            if let Some((c, g)) = current.take() {
                glyphs.insert(c, g.finish());
            }
            current = u32::from_str_radix(rest.split(']').next().unwrap_or(""), 16)
                .ok()
                .and_then(char::from_u32)
                .map(|c| (c, Builder::default()));
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
            ("L", 4) => g.line([n[0], n[1]], [n[2], n[3]]),
            ("A" | "AR", 5) => {
                // AR runs clockwise from the first angle to the second.
                let (a1, a2) = if cmd == "AR" {
                    (n[4], n[3])
                } else {
                    (n[3], n[4])
                };
                let a2 = if a2 < a1 { a2 + 360. } else { a2 };
                if a2 - a1 == 360. {
                    g.circle([n[0], n[1]], n[2]);
                } else {
                    g.arc(Arc {
                        c: [n[0], n[1]],
                        r: n[2],
                        start: a1.to_radians(),
                        sweep: (a2 - a1).to_radians(),
                    });
                }
            }
            ("PL" | "PLC", len) if len % 3 == 0 && len >= 6 => {
                let v: Vec<[f64; 3]> = n.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
                // Two vertices closed by half-turns the same way round are a circle.
                if cmd == "PLC" && v.len() == 2 && v[0][2].abs() == 1. && v[0][2] == v[1][2] {
                    let (dx, dy) = (v[1][0] - v[0][0], v[1][1] - v[0][1]);
                    g.circle(
                        [(v[0][0] + v[1][0]) / 2., (v[0][1] + v[1][1]) / 2.],
                        libm::sqrt(dx * dx + dy * dy) / 2.,
                    );
                    continue;
                }
                let mut pairs: Vec<_> = v.windows(2).map(|w| (w[0], w[1])).collect();
                if cmd == "PLC" {
                    pairs.push((v[v.len() - 1], v[0]));
                }
                for (a, b) in pairs {
                    if a[2].abs() < 1e-9 {
                        g.line([a[0], a[1]], [b[0], b[1]]);
                    } else {
                        g.arc(bulge_arc([a[0], a[1]], [b[0], b[1]], a[2]));
                    }
                }
            }
            _ => {}
        }
    }
    if let Some((c, g)) = current {
        glyphs.insert(c, g.finish());
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

#[cfg(test)]
mod tests {
    use super::*;

    // Font arcs are straight strokes computed with the pure-Rust `libm`, so these coordinates
    // are the same on every platform to the last bit. A platform `sin`, `cos`, `atan` or
    // `atan2` slipping back in would show here (and in the written reals) on another target.
    #[test]
    fn font_arc_points_are_bit_exact_on_every_platform() {
        let line = |face, text: &str, k: usize| lay(&[(face, text)]).ink.lines[k];
        assert_eq!(
            line(Face::Standard, "0", 1),
            [
                [0.6914539784081484, 7.8835767333813225],
                [0.39722755313694424, 7.211539120784248]
            ]
        );
        assert_eq!(
            line(Face::Standard, "0", 17),
            [
                [2.29165471322968, 0.03888030207627913],
                [2.4763128767583167, 0.10874644817924584]
            ]
        );
        assert_eq!(
            line(Face::Standard, "B", 5),
            [
                [4.297040000000001, 4.29704],
                [3.9333554144363427, 4.595509987346404]
            ]
        );
        assert_eq!(
            line(Face::Gdt, "d", 5),
            [
                [15.36396103067893, 6.363961030678928],
                [14.00013209717642, 7.483226510722907]
            ]
        );
    }

    #[test]
    fn only_whole_circles_stay_arcs_and_are_exact() {
        // The gdt `n` and `e` circles come from the font's numbers with no trigonometry.
        let n = lay(&[(Face::Gdt, "n")]);
        assert_eq!(n.ink.arcs, [[4.5, 4.5, 4.5, 0., 360.]]);
        let e = lay(&[(Face::Gdt, "e")]);
        assert_eq!(e.ink.arcs, [[7., 4.5, 7., 0., 360.]]);
        // A partial arc (the profile symbol's half circle) is strokes only.
        let d = lay(&[(Face::Gdt, "d")]);
        assert!(d.ink.arcs.is_empty() && d.ink.lines.len() > 2);
        // Segments scale with the sweep: 32 to a turn, so half a turn is 16.
        assert_eq!(d.ink.lines.len(), 1 + 16);
    }
}
