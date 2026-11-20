//! Face moments against OpenCascade's adaptive integration (upstream need U11 of
//! specify-core-rust): every face of the parts specify-core compares face by face, its
//! [`Part::face_moments`] area and centroid against `BRepGProp::SurfaceProperties` with eps 1e-9
//! (captured by `tools/capture_face_moments.py` into `tests/fixtures/face_moments.json.gz`).
//!
//! Agreement is specify-core's: the area within 1e-6 relative, each centroid coordinate within
//! 1e-6 of the larger of its magnitude and 1 mm. Every face outside it must be listed in
//! `tests/fixtures/references/face_moments.json` with a verdict, pinned to the port's numbers
//! (`tools/known_face_moments.py` writes it from the differences, the capture's rebuilt-pcurve
//! references, `tools/face_area_evidence.py`'s areas and the independent integration
//! [`face_moment_evidence`] runs on request).

mod common;

use std::collections::BTreeMap;
use std::io::Read;

use haecceity::Part;
use haecceity::geom::{Frame, Surface};
use haecceity::sampling::edge_interval;
use haecceity::step::read_step_file;
use serde_json::{Value, json};

const RELATIVE: f64 = 1e-6;
/// A listed difference's pinned area or centroid has changed.
const PINNED: f64 = 1e-7;

/// The three S1 pcurves of #17 delimit a triangle on the file's degree-(1,6)
/// spline. Tensor Gauss integration of the surface Jacobian, position and
/// centred tensor on that triangle gives these references at orders 8, 16
/// and 32. Both placements must retain the selected area and all its moments.
#[test]
fn tiny_declared_spline_triangle_keeps_its_moments_away_from_the_origin() {
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let part = read_step_file(&dir.join("cadgenbench/threaded_connector_109.step")).unwrap();
    assert_eq!(part.face_source(0).unwrap().entity, 17);
    let origin = [51.56, 0.04771, 367.61423];
    let placed = part
        .transformed(&[
            [1., 0., 0., -origin[0]],
            [0., 1., 0., -origin[1]],
            [0., 0., 1., -origin[2]],
        ])
        .unwrap();
    let centre = [51.559984959625815, 0.04771000100467721, 367.61422777543277];
    let tensor = [
        [9.47942887e-19, -7.74988935e-17, -1.51942339e-18],
        [-7.74988935e-17, 1.8015568032873e-14, 3.53208562e-16],
        [-1.51942339e-18, 3.53208562e-16, 6.92491561e-18],
    ];
    for (p, offset) in [(&part, [0.; 3]), (&placed, origin)] {
        let m = p.face_moments(0).expect("selected triangle has moments");
        assert!((m.area / 3.307124882796035e-8 - 1.).abs() < 1e-8);
        assert!((m.area / p.face_mass(0).unwrap()[0] - 1.).abs() < 1e-8);
        for (i, row) in tensor.iter().enumerate() {
            assert!((m.centroid[i] - (centre[i] - offset[i])).abs() < 1e-9);
            for (got, want) in m.second_moments[i].iter().zip(row) {
                assert!((got / want - 1.).abs() < 1e-6, "{got} vs {want}");
            }
        }
    }
}

fn captured() -> Value {
    let path = common::fixtures().join("face_moments.json.gz");
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

/// Whether `got` = [area, cx, cy, cz] agrees with `want` to `tolerance`.
fn agrees(got: &[f64; 4], want: &[f64], tolerance: f64) -> bool {
    (got[0] - want[0]).abs() <= tolerance * want[0].abs()
        && (1..4).all(|k| (got[k] - want[k]).abs() <= tolerance * want[k].abs().max(1.0))
}

fn moments(part: &Part, face: usize) -> Option<[f64; 4]> {
    part.face_moments(face)
        .map(|m| [m.area, m.centroid[0], m.centroid[1], m.centroid[2]])
}

/// cgb217 face 29 (`#2586`, a B-spline surface carrying a band whose own seam is not its closed
/// surface's seam) had no moments at specify-core-rust's pin (a16d7af; the walk across the
/// surface's seam came with f0e2104). OpenCascade (OCP 7.9.3.1 in the quiddity venv, quiddity
/// 6d544e06; `face.area` 49.9175719 by `tools/capture_face_areas.py`, the rest by
/// `tools/capture_face_moments.py`): adaptive (eps 1e-9) area
/// 50.08480883968193 and centroid (6.0767381143363774, 36.80801853921258, 16.382107652711074);
/// with its pcurves projected again from the edges 49.92034789571351 and (6.077270785263104,
/// 36.80819503002866, 16.382049132275753). OpenCascade integrates the region the pcurves bound,
/// approximations of the edges (the three readings differ by up to 0.17 mm², within the 1.15 mm²
/// strip the edges' tolerances allow the boundary); the port integrates the region the 3D edges
/// bound, which `tools/face_area_evidence.py`'s independent Green's-theorem and slice
/// integrations both put at 49.92958114 (`references/face_areas.json`: rust-correct).
#[test]
fn cgb217_face_29_has_moments() {
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let part = read_step_file(&dir.join("cadgenbench_inputs/cgb217.step.gz")).unwrap();
    assert_eq!(part.face_source(29).map(|s| s.entity), Some(2586));
    let m = part.face_moments(29).expect("cgb217 face 29 has moments");
    assert!((m.area - 49.92958114).abs() <= 1e-7 * 49.93, "{}", m.area);
    let occ_fine = [
        50.08480883968193,
        6.0767381143363774,
        36.80801853921258,
        16.382107652711074,
    ];
    let occ_rebuilt = [
        49.92034789571351,
        6.077270785263104,
        36.80819503002866,
        16.382049132275753,
    ];
    let band = 1.1457192213352627;
    for occ in [occ_fine, occ_rebuilt] {
        assert!((m.area - occ[0]).abs() <= band, "{} vs {}", m.area, occ[0]);
        // OpenCascade's extra 0.16 mm² (or 0.009 mm²) moves its centroid this far at most.
        for k in 0..3 {
            assert!(
                (m.centroid[k] - occ[k + 1]).abs() <= 2e-3,
                "{m:?} vs {occ:?}"
            );
        }
    }
}

/// An analytic surface as p(u, v) = o + ρ(v)(cos u X + sin u Y) + h(v) Z with area density
/// w(v), or a plane (p = o + uX + vY, w = 1): what [`green_moments`] integrates in closed form
/// along u.
enum Revolved {
    Plane(Frame),
    Round {
        frame: Frame,
        rho: Box<dyn Fn(f64) -> f64>,
        h: Box<dyn Fn(f64) -> f64>,
        w: Box<dyn Fn(f64) -> f64>,
    },
}

fn revolved(surface: &Surface) -> Option<Revolved> {
    Some(match *surface {
        Surface::Plane { frame } => Revolved::Plane(frame),
        Surface::Cylinder { frame, radius } => Revolved::Round {
            frame,
            rho: Box::new(move |_| radius),
            h: Box::new(|v| v),
            w: Box::new(move |_| radius.abs()),
        },
        Surface::Cone {
            frame,
            radius,
            semi_angle,
        } => {
            let (s, c) = semi_angle.sin_cos();
            Revolved::Round {
                frame,
                rho: Box::new(move |v| radius + v * s),
                h: Box::new(move |v| v * c),
                w: Box::new(move |v| (radius + v * s).abs()),
            }
        }
        Surface::Sphere { frame, radius } => Revolved::Round {
            frame,
            rho: Box::new(move |v| radius * v.cos()),
            h: Box::new(move |v| radius * v.sin()),
            w: Box::new(move |v| radius * radius * v.cos().abs()),
        },
        Surface::Torus {
            frame,
            major,
            minor,
        } => Revolved::Round {
            frame,
            rho: Box::new(move |v| major + minor * v.cos()),
            h: Box::new(move |v| minor * v.sin()),
            w: Box::new(move |v| (minor * (major + minor * v.cos())).abs()),
        },
        _ => return None,
    })
}

/// ∫ from a to b of `f` (each component), by 20-point Gauss–Legendre on pieces at most 0.05
/// long.
fn line_integral<const N: usize>(a: f64, b: f64, f: impl Fn(f64) -> [f64; N]) -> [f64; N] {
    // Nodes and weights of the 20-point rule on [-1, 1], from the 10 positive ones.
    const X: [f64; 10] = [
        0.076_526_521_133_497_33,
        0.227_785_851_141_645_08,
        0.373_706_088_715_419_56,
        0.510_867_001_950_827_1,
        0.636_053_680_726_515,
        0.746_331_906_460_150_8,
        0.839_116_971_822_218_8,
        0.912_234_428_251_326,
        0.963_971_927_277_913_8,
        0.993_128_599_185_094_9,
    ];
    const W: [f64; 10] = [
        0.152_753_387_130_725_85,
        0.149_172_986_472_603_75,
        0.142_096_109_318_382_05,
        0.131_688_638_449_176_63,
        0.118_194_531_961_518_42,
        0.101_930_119_817_240_44,
        0.083_276_741_576_704_75,
        0.062_672_048_334_109_06,
        0.040_601_429_800_386_94,
        0.017_614_007_139_152_12,
    ];
    let pieces = (((b - a).abs() / 0.05).ceil() as usize).max(1);
    let mut sum = [0.0; N];
    for k in 0..pieces {
        let (lo, hi) = (
            a + (b - a) * k as f64 / pieces as f64,
            a + (b - a) * (k + 1) as f64 / pieces as f64,
        );
        let (mid, half) = (0.5 * (lo + hi), 0.5 * (hi - lo));
        for i in 0..10 {
            for s in [-1.0, 1.0] {
                let y = f(mid + s * half * X[i]);
                for n in 0..N {
                    sum[n] += W[i] * half * y[n];
                }
            }
        }
    }
    sum
}

/// An integration of the face independent of `mass.rs`'s boundary walk, for the analytic
/// surfaces: [area, cx, cy, cz] by Green's theorem along each loop's edge curves sampled at
/// `n` + 1 points each (curve parameters evenly spaced, each sample's parameters by the
/// surface's closed-form inversion, consecutive samples joined straight in parameter space),
/// the inner integral in closed form along u (∮ G dv, G = ∫ from 0 to u of w·(1, p) du) or,
/// where a loop runs round u, numerically along v from a pole or apex if the face meets one
/// (−∮ H du, H = ∫ w·(1, p) dv), each straight piece by Simpson's rule. `None` on freeform
/// surfaces, where loops run round both parameters, or where a loop does not close in parameter
/// space after unwrapping (a pole the walk cannot cross).
fn green_moments(part: &Part, face: usize, n: usize) -> Option<[f64; 4]> {
    let surface = &part.faces[face].surface;
    let kind = revolved(surface)?;
    let (pu, pv) = surface.periodic();
    let turn = std::f64::consts::TAU;
    let mut loops: Vec<Vec<(f64, f64)>> = Vec::new();
    let (mut winds_u, mut winds_v) = (false, false);
    for lp in &part.faces[face].loops {
        if lp.edges.is_empty() {
            continue;
        }
        let mut points: Vec<(f64, f64)> = Vec::new();
        for &(e, forward) in &lp.edges {
            let ed = &part.edges[e];
            let (mut t0, mut t1) =
                edge_interval(&ed.curve, ed.start, ed.end, ed.same_sense, ed.is_closed());
            if !forward {
                std::mem::swap(&mut t0, &mut t1);
            }
            for i in 0..=n {
                let t = t0 + (t1 - t0) * i as f64 / n as f64;
                let (mut u, mut v) = surface.parameters(ed.curve.value(t), None)?;
                if let Some(&(lu, lv)) = points.last() {
                    if pu {
                        u = haecceity::geom::nearest_turn(u, lu);
                    }
                    if pv {
                        v = haecceity::geom::nearest_turn(v, lv);
                    }
                }
                points.push((u, v));
            }
        }
        let (first, last) = (points[0], points[points.len() - 1]);
        let (du, dv) = (last.0 - first.0, last.1 - first.1);
        let whole = |d: f64, periodic: bool| {
            if d.abs() < 1e-6 {
                Some(false)
            } else if periodic && (d.abs() - turn).abs() < 1e-6 {
                Some(true)
            } else {
                None
            }
        };
        winds_u |= whole(du, pu)?;
        winds_v |= whole(dv, pv)?;
        loops.push(points);
    }
    if winds_u && winds_v {
        return None;
    }
    // The integrand's components: w and w·p, from p's parts along o, X and Y (or cos u X +
    // sin u Y), and Z.
    let mut sum = [0.0; 4];
    match &kind {
        Revolved::Plane(f) => {
            // w = 1, p = o + uX + vY: G = (u, u·o + u²/2 X + uv Y).
            let g = |u: f64, v: f64| -> [f64; 4] {
                std::array::from_fn(|k| {
                    if k == 0 {
                        u
                    } else {
                        u * f.origin[k - 1] + 0.5 * u * u * f.x[k - 1] + u * v * f.y[k - 1]
                    }
                })
            };
            for points in &loops {
                for w in points.windows(2) {
                    let (a, b) = (w[0], w[1]);
                    let m = (0.5 * (a.0 + b.0), 0.5 * (a.1 + b.1));
                    let (ga, gm, gb) = (g(a.0, a.1), g(m.0, m.1), g(b.0, b.1));
                    for k in 0..4 {
                        sum[k] += (ga[k] + 4.0 * gm[k] + gb[k]) / 6.0 * (b.1 - a.1);
                    }
                }
            }
        }
        Revolved::Round { frame, rho, h, w } => {
            let point = |u: f64, v: f64| -> [f64; 3] {
                std::array::from_fn(|k| {
                    frame.origin[k]
                        + rho(v) * (u.cos() * frame.x[k] + u.sin() * frame.y[k])
                        + h(v) * frame.z[k]
                })
            };
            // The formula's point must be the surface's.
            let (u0, v0) = loops[0][0];
            let (q, s) = (point(u0, v0), surface.value(u0, v0));
            if (0..3).any(|k| (q[k] - s[k]).abs() > 1e-9 * (1.0 + s[k].abs())) {
                return None;
            }
            if winds_u {
                // H(u, v) = A(v) o + B(v)(cos u X + sin u Y) + C(v) Z with A = ∫ w, B = ∫ wρ,
                // C = ∫ wh from v_ref: a pole or apex the face meets, where H must vanish, else
                // the lowest v.
                let vs = loops.iter().flatten().map(|p| p.1);
                let low = vs.clone().fold(f64::INFINITY, f64::min);
                let reference = loops
                    .iter()
                    .flatten()
                    .find(|p| rho(p.1).abs() < 1e-9)
                    .map_or(low, |p| p.1);
                let hv = |u: f64, v: f64| -> [f64; 4] {
                    let [a, b, c] =
                        line_integral(reference, v, |t| [w(t), w(t) * rho(t), w(t) * h(t)]);
                    std::array::from_fn(|k| {
                        if k == 0 {
                            a
                        } else {
                            let k = k - 1;
                            a * frame.origin[k]
                                + b * (u.cos() * frame.x[k] + u.sin() * frame.y[k])
                                + c * frame.z[k]
                        }
                    })
                };
                for points in &loops {
                    for p in points.windows(2) {
                        let (a, b) = (p[0], p[1]);
                        let m = (0.5 * (a.0 + b.0), 0.5 * (a.1 + b.1));
                        let (ha, hm, hb) = (hv(a.0, a.1), hv(m.0, m.1), hv(b.0, b.1));
                        for k in 0..4 {
                            sum[k] -= (ha[k] + 4.0 * hm[k] + hb[k]) / 6.0 * (b.0 - a.0);
                        }
                    }
                }
            } else {
                // G(u, v) = w(v)·(u, u o + ρ(v)(sin u X + (1 − cos u) Y) + u h(v) Z).
                let gv = |u: f64, v: f64| -> [f64; 4] {
                    let (wv, r, hh) = (w(v), rho(v), h(v));
                    std::array::from_fn(|k| {
                        if k == 0 {
                            wv * u
                        } else {
                            let k = k - 1;
                            wv * (u * frame.origin[k]
                                + r * (u.sin() * frame.x[k] + (1.0 - u.cos()) * frame.y[k])
                                + u * hh * frame.z[k])
                        }
                    })
                };
                for points in &loops {
                    for p in points.windows(2) {
                        let (a, b) = (p[0], p[1]);
                        let m = (0.5 * (a.0 + b.0), 0.5 * (a.1 + b.1));
                        let (ga, gm, gb) = (gv(a.0, a.1), gv(m.0, m.1), gv(b.0, b.1));
                        for k in 0..4 {
                            sum[k] += (ga[k] + 4.0 * gm[k] + gb[k]) / 6.0 * (b.1 - a.1);
                        }
                    }
                }
            }
        }
    }
    // Each loop is walked as the file orients it (holes against the outer loop), so the total
    // is the area up to the parameterisation's sense. (A file whose loops are misoriented gets a
    // wrong answer here: the evidence then disagrees and the face stays undetermined.)
    let sign = sum[0].signum();
    let area = sum[0] * sign;
    (area > 0.0).then(|| {
        [
            area,
            sum[1] * sign / area,
            sum[2] * sign / area,
            sum[3] * sign / area,
        ]
    })
}

/// [`green_moments`] for a B-spline surface: the samples' parameters by the surface's inversion
/// (seeded from the sample before), and the inner integral G = ∫ from the domain's lower u of
/// w·(1, p) du numerically (20-point Gauss–Legendre on pieces at most 0.05 long within each knot
/// span). `None` where a loop does not close in parameter space or two consecutive samples are
/// more than a tenth of the domain apart (a loop crossing a closed surface's seam).
fn freeform_green_moments(part: &Part, face: usize, n: usize) -> Option<[f64; 4]> {
    let surface = &part.faces[face].surface;
    let Surface::Freeform { surface: s, .. } = surface else {
        return None;
    };
    let (u0, u1, v0, v1) = s.domain();
    let mut knots: Vec<f64> = s
        .knots_u
        .iter()
        .copied()
        .filter(|&k| k > u0 && k < u1)
        .collect();
    knots.dedup();
    let mut loops: Vec<Vec<(f64, f64)>> = Vec::new();
    for lp in &part.faces[face].loops {
        if lp.edges.is_empty() {
            continue;
        }
        let mut points: Vec<(f64, f64)> = Vec::new();
        for &(e, forward) in &lp.edges {
            let ed = &part.edges[e];
            let (mut t0, mut t1) =
                edge_interval(&ed.curve, ed.start, ed.end, ed.same_sense, ed.is_closed());
            if !forward {
                std::mem::swap(&mut t0, &mut t1);
            }
            for i in 0..=n {
                let t = t0 + (t1 - t0) * i as f64 / n as f64;
                let uv = s.invert(ed.curve.value(t), points.last().copied());
                if let Some(&l) = points.last()
                    && ((uv.0 - l.0).abs() > 0.1 * (u1 - u0)
                        || (uv.1 - l.1).abs() > 0.1 * (v1 - v0))
                {
                    return None;
                }
                points.push(uv);
            }
        }
        let (first, last) = (points[0], points[points.len() - 1]);
        if (last.0 - first.0).abs() > 1e-6 * (u1 - u0)
            || (last.1 - first.1).abs() > 1e-6 * (v1 - v0)
        {
            return None;
        }
        loops.push(points);
    }
    let f = |u: f64, v: f64| -> [f64; 4] {
        let (p, su, sv) = s.value_and_partials(u, v);
        let w = haecceity::geom::norm(haecceity::geom::cross(su, sv));
        [w, w * p[0], w * p[1], w * p[2]]
    };
    let g = |u: f64, v: f64| -> [f64; 4] {
        let mut cuts = vec![u0];
        cuts.extend(knots.iter().copied().filter(|&k| k < u));
        cuts.push(u);
        let mut sum = [0.0; 4];
        for w in cuts.windows(2) {
            let y = line_integral(w[0], w[1], |x| f(x, v));
            for k in 0..4 {
                sum[k] += y[k];
            }
        }
        sum
    };
    let mut sum = [0.0; 4];
    for points in &loops {
        for p in points.windows(2) {
            let (a, b) = (p[0], p[1]);
            if b.1 == a.1 {
                continue;
            }
            let m = (0.5 * (a.0 + b.0), 0.5 * (a.1 + b.1));
            let (ga, gm, gb) = (g(a.0, a.1), g(m.0, m.1), g(b.0, b.1));
            for k in 0..4 {
                sum[k] += (ga[k] + 4.0 * gm[k] + gb[k]) / 6.0 * (b.1 - a.1);
            }
        }
    }
    let sign = sum[0].signum();
    let area = sum[0] * sign;
    (area > 0.0).then(|| {
        [
            area,
            sum[1] * sign / area,
            sum[2] * sign / area,
            sum[3] * sign / area,
        ]
    })
}

/// [`green_moments`] with 2000 and 4000 samples per edge (B-spline faces 1000 and 2000),
/// extrapolated (the chords' error
/// falls with the square of the spacing): the estimate and, as its error, the largest change the
/// extrapolation made, scaled as an area or centroid is compared.
fn green_evidence(part: &Part, face: usize) -> Option<([f64; 4], f64)> {
    let moments = |n: usize| match part.faces[face].surface {
        Surface::Freeform { .. } => freeform_green_moments(part, face, n / 2),
        _ => green_moments(part, face, n),
    };
    let coarse = moments(2000)?;
    let fine = moments(4000)?;
    let best: [f64; 4] = std::array::from_fn(|k| fine[k] + (fine[k] - coarse[k]) / 3.0);
    let error = (0..4)
        .map(|k| {
            let scale = if k == 0 {
                best[0].abs()
            } else {
                best[k].abs().max(1.0)
            };
            (best[k] - fine[k]).abs() / scale
        })
        .fold(0.0, f64::max);
    Some((best, error))
}

/// One face outside the agreement: the port's numbers and a report.
type Found = (Option<[f64; 4]>, String);

/// Face `f` of a captured file: `None` where it agrees, else the port's numbers and a report.
fn check_face(part: &Part, entry: &Value, f: usize) -> Option<Found> {
    let want: Vec<f64> = entry["fine"][f]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect();
    let got = moments(part, f);
    if got.is_some_and(|g| agrees(&g, &want, RELATIVE)) {
        return None;
    }
    let report = format!(
        "{} rust {got:?} occ adaptive {want:?} (pcurves rebuilt {}, band {})",
        entry["types"][f], entry["fine_3d"][f], entry["band"][f]
    );
    Some((got, report))
}

#[test]
fn face_moments_match_opencascade_adaptive() {
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let captured = captured();
    let entries = captured["files"].as_array().unwrap();
    let known = common::load("references/face_moments.json");
    let known = known.as_array().unwrap();
    common::check_verdicts("references/face_moments.json", known);
    // The parts read in parallel, then their faces integrated in parallel.
    let parts: Vec<Part> = common::parallel::map(entries, |entry| {
        let file = entry["file"].as_str().unwrap();
        let part = read_step_file(&dir.join(file)).unwrap();
        assert_eq!(
            part.faces.len(),
            entry["fine"].as_array().unwrap().len(),
            "{file}: face count"
        );
        part
    });
    let jobs: Vec<(usize, usize)> = parts
        .iter()
        .enumerate()
        .flat_map(|(i, p)| (0..p.faces.len()).map(move |f| (i, f)))
        .collect();
    let outcomes = common::parallel::map(&jobs, |&(i, f)| check_face(&parts[i], &entries[i], f));
    let faces = jobs.len();
    let found: BTreeMap<(String, u64), Found> = jobs
        .iter()
        .zip(outcomes)
        .filter_map(|(&(i, f), found)| {
            let file = entries[i]["file"].as_str().unwrap().to_string();
            found.map(|x| ((file, f as u64), x))
        })
        .collect();
    eprintln!("{} of {faces} faces outside 1e-6", found.len());
    if let Some(dump) = std::env::var_os("FACE_MOMENTS_DUMP") {
        let rows: Vec<Value> = found
            .iter()
            .map(|((file, face), (rust, report))| {
                json!({"file": file, "face": face, "rust": rust, "report": report})
            })
            .collect();
        std::fs::write(dump, serde_json::to_string_pretty(&rows).unwrap()).unwrap();
    }
    let mut problems = Vec::new();
    let mut listed = Vec::new();
    for k in known {
        let key = (
            k["file"].as_str().unwrap().to_string(),
            k["face"].as_u64().unwrap(),
        );
        let pinned: Option<Vec<f64>> = k["rust"]
            .as_array()
            .map(|a| a.iter().map(|x| x.as_f64().unwrap()).collect());
        if let Some(ids) = k["spatial_control_edges"].as_array() {
            // This independent Green integral followed the 3D edges. Check
            // its original numbers under that selection. The file's own master
            // must pass the ordinary OpenCascade check as an unlisted face.
            let edges = ids.iter().map(|id| id.as_u64().unwrap()).collect();
            let spatial = common::with_spatial_edges(&dir.join(&key.0), &edges);
            let got = moments(&spatial, key.1 as usize);
            if !got
                .zip(pinned.as_deref())
                .is_some_and(|(g, p)| agrees(&g, p, PINNED))
            {
                problems.push(format!(
                    "{key:?} spatial control was {pinned:?}, now {got:?}"
                ));
            }
            continue;
        }
        match found.get(&key) {
            None => problems.push(format!("{key:?} is listed but now agrees: remove it")),
            Some((got, report)) => {
                let same = match (got, &pinned) {
                    (Some(g), Some(p)) => agrees(g, p, PINNED),
                    (None, None) => true,
                    _ => false,
                };
                if !same {
                    problems.push(format!(
                        "{key:?} is listed at {pinned:?} but changed: {report}"
                    ));
                }
            }
        }
        listed.push(key);
    }
    for (key, (_, report)) in &found {
        if !listed.contains(key) {
            problems.push(format!("{} face {}: {report}", key.0, key.1));
        }
    }
    assert!(
        problems.is_empty(),
        "{} problems:\n{}",
        problems.len(),
        problems.join("\n")
    );
}

/// The independent evidence for the faces a `FACE_MOMENTS_DUMP` lists, written to
/// `FACE_MOMENTS_EVIDENCE` for `tools/known_face_moments.py`: per face, [`green_evidence`]'s
/// moments and error estimate (`null` where it gives none), and the diagonal of the face's box.
/// Run on request: `cargo test --release -p haecceity --test face_moments -- --ignored face_moment_evidence`.
#[test]
#[ignore]
fn face_moment_evidence() {
    let dir = common::corpus_dir().expect("the corpus");
    let dump = std::env::var_os("FACE_MOMENTS_DUMP").expect("FACE_MOMENTS_DUMP");
    let out = std::env::var_os("FACE_MOMENTS_EVIDENCE").expect("FACE_MOMENTS_EVIDENCE");
    let rows: Value = serde_json::from_str(&std::fs::read_to_string(dump).unwrap()).unwrap();
    let mut by_file: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for r in rows.as_array().unwrap() {
        by_file
            .entry(r["file"].as_str().unwrap().to_string())
            .or_default()
            .push(r["face"].as_u64().unwrap() as usize);
    }
    let parts: BTreeMap<&String, Part> = by_file
        .keys()
        .map(|f| (f, read_step_file(&dir.join(f)).unwrap()))
        .collect();
    let jobs: Vec<(&String, usize)> = by_file
        .iter()
        .flat_map(|(f, faces)| faces.iter().map(move |&i| (f, i)))
        .collect();
    let found = common::parallel::map(&jobs, |&(file, face)| {
        let part = &parts[file];
        let e = green_evidence(part, face);
        let b = part.face_bounds(face);
        let value = json!({
            "green": e.map(|e| e.0),
            "error": e.map(|e| e.1),
            "diagonal": haecceity::geom::dist(b.min, b.max),
        });
        (file.clone(), face, value)
    });
    let map: serde_json::Map<String, Value> = found
        .into_iter()
        .map(|(f, i, v)| (format!("{f}#{i}"), v))
        .collect();
    std::fs::write(out, serde_json::to_string_pretty(&map).unwrap()).unwrap();
}
