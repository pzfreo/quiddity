//! Independent box-minus-polygon construction, with a step cutting across one mouth.
use quiddity::features::{self, Context, prismatic_through_cuts};
use quiddity::kernel::brep::{Part, Solid};
use quiddity::kernel::geom::{self, V3};

fn clip(poly: &[[f64; 2]], left: bool) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        let inside = |p: [f64; 2]| if left { p[0] < 0.5 } else { p[0] > 0.5 };
        if inside(a) {
            out.push(a);
        }
        if inside(a) != inside(b) {
            let t = (0.5 - a[0]) / (b[0] - a[0]);
            out.push([0.5, a[1] + t * (b[1] - a[1])]);
        }
    }
    out
}
fn area(p: &[[f64; 2]]) -> f64 {
    (0..p.len())
        .map(|i| {
            let a = p[i];
            let b = p[(i + 1) % p.len()];
            a[0] * b[1] - a[1] * b[0]
        })
        .sum::<f64>()
        .abs()
        / 2.
}

/// Stock -5..5 square, z=0..10; an n-sided through-hole; an edge step x>=0.5,z>=7.
/// A blind variant retains one millimetre of stock below the hole. No dataset geometry.
pub(crate) fn control(sides: usize, blind: bool) -> (Part, Vec<usize>, f64) {
    let poly: Vec<_> = (0..sides)
        .map(|i| {
            let t = std::f64::consts::TAU * i as f64 / sides as f64;
            [2. * t.cos(), 2. * t.sin()]
        })
        .collect();
    let left = clip(&poly, true);
    let right = clip(&poly, false);
    let ylo = left
        .iter()
        .filter(|p| (p[0] - 0.5).abs() < 1e-9)
        .map(|p| p[1])
        .fold(f64::INFINITY, f64::min);
    let yhi = left
        .iter()
        .filter(|p| (p[0] - 0.5).abs() < 1e-9)
        .map(|p| p[1])
        .fold(f64::NEG_INFINITY, f64::max);
    let chain = |mut p: Vec<[f64; 2]>, start: f64| {
        p.reverse();
        let i = p
            .iter()
            .position(|p| (p[0] - 0.5).abs() < 1e-9 && (p[1] - start).abs() < 1e-9)
            .unwrap();
        p.rotate_left(i);
        p
    };
    let mut upper = vec![[-5., -5.], [0.5, -5.]];
    upper.extend(chain(left.clone(), ylo));
    upper.extend([[0.5, 5.], [-5., 5.]]);
    let mut lower = vec![[0.5, -5.], [5., -5.], [5., 5.], [0.5, 5.]];
    lower.extend(chain(right.clone(), yhi));
    let mut rings: Vec<Vec<V3>> = vec![vec![
        [-5., -5., 0.],
        [-5., 5., 0.],
        [5., 5., 0.],
        [5., -5., 0.],
    ]];
    // Dummy cap becomes the bottom face's inner wire (or the blind floor).
    let floor = if blind { 1. } else { 0. };
    rings.push(poly.iter().map(|p| [p[0], p[1], floor]).collect());
    rings.push(vec![
        [-5., -5., 0.],
        [5., -5., 0.],
        [5., -5., 7.],
        [0.5, -5., 7.],
        [0.5, -5., 10.],
        [-5., -5., 10.],
    ]);
    rings.push(rings[2].iter().rev().map(|p| [p[0], 5., p[2]]).collect());
    rings.push(vec![
        [-5., -5., 0.],
        [-5., -5., 10.],
        [-5., 5., 10.],
        [-5., 5., 0.],
    ]);
    rings.push(vec![
        [5., -5., 0.],
        [5., 5., 0.],
        [5., 5., 7.],
        [5., -5., 7.],
    ]);
    rings.push(upper.iter().map(|p| [p[0], p[1], 10.]).collect());
    rings.push(lower.iter().map(|p| [p[0], p[1], 7.]).collect());
    for (a, b) in [(-5., ylo), (yhi, 5.)] {
        rings.push(vec![
            [0.5, a, 7.],
            [0.5, b, 7.],
            [0.5, b, 10.],
            [0.5, a, 10.],
        ]);
    }
    let start = rings.len();
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        let h = |p: [f64; 2]| if p[0] < 0.5 { 10. } else { 7. };
        let mut ring = vec![[a[0], a[1], floor], [a[0], a[1], h(a)]];
        if (a[0] < 0.5) != (b[0] < 0.5) {
            let t = (0.5 - a[0]) / (b[0] - a[0]);
            let y = a[1] + t * (b[1] - a[1]);
            ring.extend([[0.5, y, h(a)], [0.5, y, h(b)]]);
        }
        ring.extend([[b[0], b[1], h(b)], [b[0], b[1], floor]]);
        rings.push(ring);
    }
    let mut points = Vec::new();
    let faces: Vec<_> = rings
        .iter()
        .map(|r| {
            r.iter()
                .map(|&p| {
                    if let Some(i) = points.iter().position(|&q| geom::dist(q, p) < 1e-9) {
                        i
                    } else {
                        points.push(p);
                        points.len() - 1
                    }
                })
                .collect()
        })
        .collect();
    let shell = features::support_patches::polygon_part(&points, &faces, true).unwrap();
    let mut faces = shell.faces;
    if !blind {
        let hole = faces[1].loops[0].clone();
        faces[0].loops.push(hole);
        faces.remove(1);
    }
    let offset = usize::from(!blind);
    let walls = (start - offset..start + sides - offset).collect();
    let solid = Solid {
        faces: (0..faces.len()).collect(),
    };
    let part = Part::new(faces, shell.edges, vec![solid]);
    let expected = 865. - area(&left) * (10. - floor) - area(&right) * (7. - floor);
    (part, walls, expected)
}

#[test]
fn interrupted_triangle_rectangle_and_hexagon_are_proved_without_stock_claims() {
    for n in [3, 4, 6] {
        let (p, walls, volume) = control(n, false);
        assert!(
            p.solid_is_geometrically_valid(0),
            "{n}: bad {:?}",
            p.bad_faces(0)
        );
        assert!((p.solid_mass(0).unwrap().0 - volume).abs() < 1e-7);
        let found = prismatic_through_cuts::discover(&Context::new(&p));
        assert_eq!(found.len(), 1, "{n}: {found:?}");
        assert_eq!(found[0].defining, walls);
        assert_eq!(found[0].record.cut.section.boundary.len(), n);
        assert!(
            (found[0].record.cut.run_interval.1 - found[0].record.cut.run_interval.0 - 10.).abs()
                < 1e-6
        );
        let all = features::recognise(&p);
        assert_eq!(all.defining["prismatic_through_cuts"], vec![walls]);
        assert_eq!(all.prismatic_through_cuts[0].extent, "observed_walls");
    }
}

#[test]
fn an_intact_wall_ring_with_a_floor_is_not_a_through_cut() {
    for n in [3, 4, 6] {
        let (p, _, volume) = control(n, true);
        assert!(p.solid_is_geometrically_valid(0));
        assert!((p.solid_mass(0).unwrap().0 - volume).abs() < 1e-7);
        assert!(prismatic_through_cuts::discover(&Context::new(&p)).is_empty());
    }
}

#[test]
fn cut_geometry_claims_and_fingerprint_follow_a_generic_rigid_motion() {
    let (p, walls, _) = control(6, false);
    let (s, c) = 0.37_f64.sin_cos();
    let matrix = [
        [c, 0., s, 123.456],
        [s * s, c, -s * c, -78.9],
        [-s * c, s, c * c, 41.3],
    ];
    let moved = p.transformed(&matrix).unwrap();
    let direct = prismatic_through_cuts::discover(&Context::new(&moved));
    assert_eq!(direct.len(), 1);
    assert_eq!(direct[0].defining, walls);
    assert!(
        (direct[0].record.cut.run_interval.1 - direct[0].record.cut.run_interval.0 - 10.).abs()
            < 1e-6
    );
    let a = quiddity::correspondence::recognise(&p);
    let b = quiddity::correspondence::recognise(&moved);
    let records = |r: &quiddity::correspondence::Recognition| {
        quiddity::recognition::document(r, p.faces.len())
            .records
            .into_iter()
            .filter(|r| r.family == "prismatic_through_cuts")
            .collect::<Vec<_>>()
    };
    let ar = records(&a);
    let br = records(&b);
    assert_eq!(ar.len(), 1);
    assert_eq!(br.len(), 1);
    assert_eq!(ar[0].faces, walls);
    assert_eq!(br[0].faces, walls);
    let af = a
        .fingerprints
        .features
        .iter()
        .find(|f| f.family == "prismatic_through_cuts")
        .unwrap();
    let bf = b
        .fingerprints
        .features
        .iter()
        .find(|f| f.family == af.family)
        .unwrap();
    assert_eq!(af.sizes, bf.sizes);
    assert_eq!(af.faces, bf.faces);
}
