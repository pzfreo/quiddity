// Compare the actual projected segments, preserving every strip boundary.
// A hull would fill the empty space this layout change must keep available.
pub fn within_clearance(
    a: &[Vec<[f64; 3]>],
    b: &[Vec<[f64; 3]>],
    view: [f64; 3],
    clearance: f64,
) -> bool {
    use haecceity::geom;
    let x = geom::unit(geom::cross([0., 0., 1.], view)).unwrap();
    let y = geom::unit(geom::cross(view, x)).unwrap();
    let project = |p| [geom::dot(p, x), geom::dot(p, y)];
    let a: Vec<_> = a
        .iter()
        .flat_map(|line| line.windows(2))
        .map(|s| [project(s[0]), project(s[1])])
        .collect();
    let b: Vec<_> = b
        .iter()
        .flat_map(|line| line.windows(2))
        .map(|s| [project(s[0]), project(s[1])])
        .collect();
    let cross = |a: [f64; 2], b: [f64; 2]| a[0] * b[1] - a[1] * b[0];
    let sub = |a: [f64; 2], b: [f64; 2]| [a[0] - b[0], a[1] - b[1]];
    let point_distance = |p: [f64; 2], s: [[f64; 2]; 2]| {
        let d = sub(s[1], s[0]);
        let q = sub(p, s[0]);
        let length2 = d[0] * d[0] + d[1] * d[1];
        let t = if length2 > 0. {
            ((q[0] * d[0] + q[1] * d[1]) / length2).clamp(0., 1.)
        } else {
            0.
        };
        (q[0] - t * d[0]).hypot(q[1] - t * d[1])
    };
    a.iter().any(|a| {
        b.iter().any(|b| {
            if (0..2).any(|k| {
                a[0][k].min(a[1][k]) > b[0][k].max(b[1][k]) + clearance
                    || b[0][k].min(b[1][k]) > a[0][k].max(a[1][k]) + clearance
            }) {
                return false;
            }
            let u = sub(a[1], a[0]);
            let v = sub(b[1], b[0]);
            let q = sub(b[0], a[0]);
            let denominator = cross(u, v);
            if denominator.abs() > 1e-12 {
                let t = cross(q, v) / denominator;
                let s = cross(q, u) / denominator;
                if (0. ..=1.).contains(&t) && (0. ..=1.).contains(&s) {
                    return true;
                }
            }
            [
                point_distance(a[0], *b),
                point_distance(a[1], *b),
                point_distance(b[0], *a),
                point_distance(b[1], *a),
            ]
            .into_iter()
            .any(|d| d < clearance)
        })
    })
}
