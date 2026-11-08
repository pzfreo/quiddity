//! Compare geometric measurements rather than a basis's gauge or a body key's box orientation.
use quiddity::kernel::geom::{V3, dot};
use serde_json::{Value, json};
fn vector(v: &Value) -> Option<V3> {
    match v.as_str() {
        Some("x") => Some([1., 0., 0.]),
        Some("y") => Some([0., 1., 0.]),
        Some("z") => Some([0., 0., 1.]),
        _ => serde_json::from_value(v.clone()).ok(),
    }
}
fn point(v: &Value) -> V3 {
    serde_json::from_value(v.clone()).unwrap()
}
fn add(o: V3, d: V3, s: f64) -> V3 {
    std::array::from_fn(|i| o[i] + d[i] * s)
}
fn positive(d: V3) -> (V3, f64) {
    // For #127: axes are already compared modulo sign. At a 45-degree tie,
    // round-off must not choose opposite representatives after a motion. Select
    // the last tied component, preserving every component and the comparison bands.
    let largest = d.iter().map(|v| v.abs()).fold(0., f64::max);
    let w = d
        .iter()
        .rposition(|v| largest - v.abs() <= 8. * f64::EPSILON)
        .unwrap();
    let s = 1_f64.copysign(d[w]);
    (d.map(|v| v * s), s)
}
fn plane(d: V3, o: V3, s: f64) -> Value {
    let (n, sign) = positive(d);
    json!({"normal":n,"offset":(dot(o,d)+s)*sign})
}
fn order(v: &mut [Value]) {
    fn key(v: &Value) -> String {
        match v {
            Value::Number(n) => {
                format!("{:024.6}", (n.as_f64().unwrap() * 1e6).round() / 1e6 + 0.0)
            }
            Value::Array(a) => a.iter().map(key).collect::<Vec<_>>().join(","),
            Value::Object(o) => o
                .iter()
                .map(|(k, v)| format!("{k}:{}", key(v)))
                .collect::<Vec<_>>()
                .join(","),
            _ => v.to_string(),
        }
    }
    v.sort_by_cached_key(key);
}

pub fn canonical(v: &Value, family: &str) -> Value {
    let Value::Object(raw) = v else {
        return v.clone();
    };
    let mut r = raw.clone();
    let o = raw.get("origin").map(point).unwrap_or([0.; 3]);
    let axis = raw.get("axis").and_then(vector);
    // Boxes are a geometry-derived body identity, not a feature's outline. Their independent
    // coordinate reconstruction is checked on every published box by vector_publication.
    for key in ["body_key", "body_bounds"] {
        if let Some(b) = r.remove(key)
            && key == "body_key"
            && !b.is_null()
        {
            let mass = if b.is_array() {
                json!([b[6], b[7]])
            } else {
                json!([b["volume"], b["area"]])
            };
            r.insert(key.into(), mass);
        }
    }
    // For #123: compare the meridian's actual file-coordinate stations and radii.
    // Axis sense may reverse; preserve face/kind/side and every arc measurement.
    if family == "turned_bodies" {
        let d = axis.unwrap();
        let origin = point(&raw["axis_point"]);
        let station =
            |p: &Value| json!({"point":add(origin,d,p[0].as_f64().unwrap()),"radius":p[1]});
        let mut segments = Vec::new();
        for p in raw["profile"].as_array().unwrap() {
            let mut p = p.clone();
            let mut ends = vec![station(&p["start"]), station(&p["end"])];
            order(&mut ends);
            p["start"] = ends[0].clone();
            p["end"] = ends[1].clone();
            if !p["arc_center"].is_null() {
                let c = &p["arc_center"];
                let cs = c[0].as_f64().unwrap();
                let cr = c[1].as_f64().unwrap();
                let start = &raw["profile"][segments.len()]["start"];
                let theta =
                    (start[1].as_f64().unwrap() - cr).atan2(start[0].as_f64().unwrap() - cs);
                let middle = theta + p["arc_sweep"].as_f64().unwrap() / 2.;
                let radius = p["arc_radius"].as_f64().unwrap();
                p["arc_midpoint"] = station(&json!([
                    cs + radius * middle.cos(),
                    cr + radius * middle.sin()
                ]));
                p["arc_center"] = station(&p["arc_center"]);
            }
            if let Some(sweep) = p["arc_sweep"].as_f64() {
                p["arc_sweep"] = json!(sweep.abs());
            }
            segments.push(p);
        }
        order(&mut segments);
        r.insert("profile".into(), json!(segments));
        let mut bores = Vec::new();
        for b in raw["bores"].as_array().unwrap() {
            let mut ends: Vec<_> = b["span"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| json!(add(origin, d, s.as_f64().unwrap())))
                .collect();
            order(&mut ends);
            bores.push(json!({"diameter":b["diameter"],"ends":ends}));
        }
        order(&mut bores);
        r.insert("bores".into(), json!(bores));
        if !raw["profile_key"].is_null() {
            r.insert(
                "profile_key".into(),
                canonical(&raw["profile_key"], "profile"),
            );
        }
    }
    if let Some(p) = r.get_mut("profile") {
        *p = canonical(p, "profile");
    }
    if family == "profile"
        && let Some(d) = axis
        && let Some(p) = raw.get("axis_origin")
    {
        let p = point(p);
        r.insert("axis_origin".into(), json!(add(p, d, -dot(p, d))));
    }
    if family == "flats"
        && let Some(d) = axis
    {
        let basis: Vec<V3> = raw
            .get("section_axes")
            .map(|v| {
                v.as_array()
                    .unwrap()
                    .iter()
                    .map(|v| vector(v).unwrap())
                    .collect()
            })
            .unwrap_or_else(|| {
                (0..3)
                    .map(|i| {
                        let mut a = [0.; 3];
                        a[i] = 1.;
                        a
                    })
                    .filter(|a| dot(*a, d).abs() < 0.5)
                    .collect()
            });
        let p = raw["axis_line"].as_array().unwrap();
        let q = add(
            add(o, basis[0], p[0].as_f64().unwrap()),
            basis[1],
            p[1].as_f64().unwrap(),
        );
        let run = vector(&raw["axis_direction"]).unwrap();
        let relative: V3 = std::array::from_fn(|i| q[i] - o[i]);
        let q = add(q, d, -dot(relative, run) / dot(d, run));
        r.insert("axis_line".into(), json!(add(q, run, -dot(q, run))));
        r.remove("section_axes");
        r.remove("origin");
    }
    if [
        "turned_steps",
        "plates",
        "polygonal_stock",
        "polygonal_bosses",
        "repeating_radial_profiles",
    ]
    .contains(&family)
        && let Some(d) = axis
    {
        for (lo, hi) in [("lo", "hi"), ("base", "top")] {
            if let (Some(a), Some(b)) = (raw.get(lo), raw.get(hi)) {
                let mut planes = vec![
                    plane(d, o, a.as_f64().unwrap()),
                    plane(d, o, b.as_f64().unwrap()),
                ];
                order(&mut planes);
                r.remove(lo);
                r.remove(hi);
                r.insert("stations".into(), json!(planes));
            }
        }
        r.remove("origin");
    }
    if family == "plates"
        && let Some(d) = axis
    {
        let axes: Vec<V3> = raw
            .get("section_axes")
            .map(|v| {
                v.as_array()
                    .unwrap()
                    .iter()
                    .map(|v| vector(v).unwrap())
                    .collect()
            })
            .unwrap_or_else(|| {
                (0..3)
                    .map(|i| {
                        let mut a = [0.; 3];
                        a[i] = 1.;
                        a
                    })
                    .filter(|a| dot(*a, d).abs() < 0.5)
                    .collect()
            });
        let q = add(
            add(o, axes[0], raw["u"].as_f64().unwrap()),
            axes[1],
            raw["v"].as_f64().unwrap(),
        );
        r.insert("transverse_center".into(), json!(add(q, d, -dot(q, d))));
        for k in ["u", "v", "section_axes"] {
            r.remove(k);
        }
    }
    if family == "risers" {
        for (key, a) in [("positions", "axis"), ("other_positions", "other_axis")] {
            if let Some(values) = raw.get(key).and_then(Value::as_array) {
                let d = vector(&raw[a]).unwrap();
                let mut planes: Vec<_> = values
                    .iter()
                    .map(|v| plane(d, o, v.as_f64().unwrap()))
                    .collect();
                order(&mut planes);
                r.insert(key.into(), json!(planes));
            }
        }
        let h = raw
            .get("height_axis")
            .and_then(vector)
            .unwrap_or([0., 0., 1.]);
        let mut ends:Vec<_>=[("z_lo","lo_at_envelope"),("z_hi","hi_at_envelope")].into_iter().map(|(s,flag)|json!({"plane":plane(h,o,raw[s].as_f64().unwrap()),"envelope":raw[flag]})).collect();
        order(&mut ends);
        for k in [
            "z_lo",
            "z_hi",
            "lo_at_envelope",
            "hi_at_envelope",
            "origin",
            "height_axis",
        ] {
            r.remove(k);
        }
        r.insert("height_ends".into(), json!(ends));
    }
    if family == "step_levels" {
        let h = raw
            .get("height_axis")
            .and_then(vector)
            .unwrap_or([0., 0., 1.]);
        let x = raw.get("x_axis").and_then(vector).unwrap_or([1., 0., 0.]);
        let y = raw.get("y_axis").and_then(vector).unwrap_or([0., 1., 0.]);
        let z = raw["z"].as_f64().unwrap();
        let mut corners = Vec::new();
        if let (Some(xs), Some(ys)) = (raw["x_span"].as_array(), raw["y_span"].as_array()) {
            for a in xs {
                for b in ys {
                    corners.push(json!(add(
                        add(add(o, h, z), x, a.as_f64().unwrap()),
                        y,
                        b.as_f64().unwrap()
                    )));
                }
            }
        }
        order(&mut corners);
        for k in [
            "z",
            "x_span",
            "y_span",
            "origin",
            "height_axis",
            "x_axis",
            "y_axis",
        ] {
            r.remove(k);
        }
        r.insert("plane".into(), plane(h, o, z));
        r.insert("corners".into(), json!(corners));
    }
    if let Some(levels) = r.get_mut("body_levels").and_then(Value::as_array_mut) {
        for level in levels.iter_mut() {
            *level = canonical(level, "step_levels");
        }
        order(levels);
    }
    if family == "paired_ramp_steps"
        && let Some(d) = raw.get("width_axis").and_then(vector)
    {
        let (d, sign) = positive(d);
        r.insert("width_axis".into(), json!(d));
        if sign < 0.
            && let Some(pair) = r.get_mut("half_widths").and_then(Value::as_array_mut)
        {
            pair.reverse();
        }
    }
    if matches!(
        family,
        "oblique_through_steps" | "polygonal_oblique_through_steps"
    ) {
        let field = if family == "polygonal_oblique_through_steps" {
            r.insert(
                "through_axis".into(),
                json!(positive(vector(&raw["through_axis"]).unwrap()).0),
            );
            "floor_outline"
        } else {
            "wall_outline"
        };
        let run = vector(&raw["run"]).unwrap();
        let across = vector(&raw["across_direction"]).unwrap();
        let start = add(
            point(&raw["at"]),
            run,
            -0.5 * raw["length"].as_f64().unwrap(),
        );
        let corners: Vec<_> = raw[field]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                json!(add(
                    add(start, run, p[0].as_f64().unwrap()),
                    across,
                    p[1].as_f64().unwrap()
                ))
            })
            .collect();
        r.insert(field.into(), json!(corners));
        r.insert("run".into(), json!(positive(run).0));
    }
    if family == "circular_blind_steps"
        && let Some(section) = r.get_mut("section").and_then(Value::as_array_mut)
        && section.len() == 3
    {
        // The arc endpoints are unordered geometry. Preserve the centre and the directed
        // blind-to-opening centreline while comparing both endpoint coordinates exactly.
        let mut ends = [section[0].clone(), section[2].clone()];
        order(&mut ends);
        section[0] = ends[0].clone();
        section[2] = ends[1].clone();
    }
    if let Some(d) = axis {
        r.insert("axis".into(), json!(positive(d).0));
    }
    if let Some(d) = raw.get("other_axis").and_then(vector) {
        r.insert("other_axis".into(), json!(positive(d).0));
    }
    Value::Object(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circular() -> Value {
        json!({"axis":"z", "radius":4.0, "length":10.0,
            "centreline":[[0.0,0.0,0.0],[0.0,0.0,10.0]],
            "section":[[-4.0,0.0],[0.0,0.0],[0.0,4.0]]})
    }

    #[test]
    fn unsigned_axis_ties_preserve_every_component_and_other_measurements() {
        let q = std::f64::consts::FRAC_1_SQRT_2;
        let base =
            json!({"axis":[-q,0.,q],"radius":3.,"at":[1.,2.,3.],"side":"concave","turned":false});
        for delta in [-f64::EPSILON, 0., f64::EPSILON] {
            for sign in [-1., 1.] {
                let d = [sign * (-q - delta), 0., sign * (q - delta)];
                let mut r = base.clone();
                r["axis"] = json!(d);
                let normal = canonical(&r, "fillets");
                let axis: V3 = point(&normal["axis"]);
                assert!(axis[0] < 0. && axis[2] > 0.);
                for (actual, original) in axis.iter().zip(d) {
                    assert_eq!(actual.abs(), original.abs());
                }
                for key in ["radius", "at", "side", "turned"] {
                    assert_eq!(normal[key], base[key]);
                }
            }
        }
        let expected = canonical(&base, "fillets");
        for (key, value) in [
            ("axis", json!([0., 0., 1.])),
            ("radius", json!(4.)),
            ("at", json!([2., 2., 3.])),
            ("side", json!("convex")),
        ] {
            let mut changed = base.clone();
            changed[key] = value;
            assert_ne!(canonical(&changed, "fillets"), expected);
        }
    }

    #[test]
    fn circular_endpoint_permutation_preserves_the_cut() {
        let base = circular();
        let mut reversed = base.clone();
        reversed["section"].as_array_mut().unwrap().swap(0, 2);
        assert_eq!(
            canonical(&base, "circular_blind_steps"),
            canonical(&reversed, "circular_blind_steps")
        );
    }

    #[test]
    fn circular_geometry_and_blind_open_roles_remain_observable() {
        let base = circular();
        let expected = canonical(&base, "circular_blind_steps");
        for (field, value) in [
            ("radius", json!(5.0)),
            ("length", json!(11.0)),
            ("section", json!([[-3.0, 0.0], [0.0, 0.0], [0.0, 4.0]])),
            ("section", json!([[-4.0, 0.0], [1.0, 0.0], [0.0, 4.0]])),
            ("centreline", json!([[0.0, 0.0, 10.0], [0.0, 0.0, 0.0]])),
        ] {
            let mut changed = base.clone();
            changed[field] = value;
            assert_ne!(
                canonical(&changed, "circular_blind_steps"),
                expected,
                "{field}"
            );
        }
    }
}
