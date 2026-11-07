//! Phase 0, step B: a real AP242 file through solidcore with its PMI.
//!
//! 1. haecceity reads the file: the part's B-rep and its semantic PMI.
//! 2. Each face is rebuilt in solidcore (planes, cylinders and cones bounded by lines and
//!    circles), the faces are stitched into a solid, and each face is tagged with the STEP
//!    entity it came from.
//! 3. A box removes the half of the part above its middle in x (or y/z, `--axis`).
//! 4. Each PMI feature made of faces is reported: kept (all its faces survive, unsplit),
//!    split (a face of it is now several), partial (some faces gone) or orphaned (none left).
//!
//! The face build follows solidcore's own stitched STEP build (`crates/step-brep/src/lib.rs`,
//! `build_stitched`, Apache-2.0), cut down to the geometry this prototype supports.
//!
//! Usage: `bridge FILE.step[.gz] [--axis x|y|z] [--keep FRACTION]`

use std::collections::{BTreeMap, BTreeSet};

use haecceity::geom::{Curve as HCurve, Frame as HFrame, Surface as HSurface, V3};
use haecceity::p21::Document;
use haecceity::pmi::{self, Anchor, Feature};
use haecceity::step::{read_part, read_part_definitions};
use haecceity::Part;

use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::attribute::{Attribute, AttributeStore, AttributeValue, Behavior, ObjectKey, ids};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::heal::stitch::p1::api_stitch_into_solid;
use solidcore::mass::integrator::properties::{PropertyAttributes, brep_volume, face_area};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::primitive::api_primitives::create_box;
use solidcore::surface::cone::Cone;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::surface::torus::{Torus, TorusOptions};
use solidcore::topology::brep::p1::faces;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::{p3, p7};
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::trimming_tools;
use solidcore::{Brep, EntityRef, FaceId, Kernel, Point2, Point3, Vec3};

type Error = Box<dyn std::error::Error>;

/// Attribute id `STEP_BASE + n` on a face holds STEP entity `#n` (one id per entity: see
/// `tags.rs` for why a shared id loses tags when faces merge).
const STEP_BASE: u64 = ids::USER_1 + (1 << 40);

// ---------------------------------------------------------------------------------------------
// Small vector helpers on haecceity's [f64; 3]
// ---------------------------------------------------------------------------------------------

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn v(a: V3) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}
fn placement(f: &HFrame) -> Axis2Placement {
    Axis2Placement::new(v(f.origin), v(f.x), v(f.y))
}
/// Angle of `p` about `f`'s z axis from its x axis, in degrees.
fn angle_deg(f: &HFrame, p: V3) -> f64 {
    let d = sub(p, f.origin);
    dot(d, f.y).atan2(dot(d, f.x)).to_degrees()
}

/// A frame whose x axis points at the middle of the largest angular gap between `points`
/// about the frame's z axis, so a cylinder's or cone's seam does not cut through the face
/// (step-brep's `seam_frame`).
fn seam_frame(f: &HFrame, points: &[V3]) -> HFrame {
    let mut angles: Vec<f64> = points.iter().map(|&p| angle_deg(f, p).to_radians()).collect();
    if angles.is_empty() {
        return *f;
    }
    angles.sort_by(|a, b| a.total_cmp(b));
    let (mut largest, mut middle) = (0.0_f64, 0.0_f64);
    for (i, &a) in angles.iter().enumerate() {
        let next = angles.get(i + 1).copied().unwrap_or(angles[0] + std::f64::consts::TAU);
        let gap = (next - a).rem_euclid(std::f64::consts::TAU);
        if gap > largest {
            largest = gap;
            middle = a + gap * 0.5;
        }
    }
    let (s, c) = middle.sin_cos();
    let x = [
        f.x[0] * c + f.y[0] * s,
        f.x[1] * c + f.y[1] * s,
        f.x[2] * c + f.y[2] * s,
    ];
    let z = f.z;
    let y = [
        z[1] * x[2] - z[2] * x[1],
        z[2] * x[0] - z[0] * x[2],
        z[0] * x[1] - z[1] * x[0],
    ];
    HFrame { origin: f.origin, x, y, z }
}

// ---------------------------------------------------------------------------------------------
// haecceity → solidcore geometry
// ---------------------------------------------------------------------------------------------

/// The 3D curve of `edge`, bounded to the edge, running along the curve's own direction, and
/// whether that direction is start → end of the edge.
fn edge_curve(k: &Kernel, part: &Part, edge: usize) -> Result<(Curve, bool), Error> {
    let e = &part.edges[edge];
    match &e.curve {
        HCurve::Line { .. } => {
            let d = sub(e.end, e.start);
            let len = dot(d, d).sqrt();
            let line = Line::bounded(
                v(e.start),
                v(d),
                &Extent1d::from_min_max(0.0, len),
                &BoundedLineOptions { scale: 1.0, dim: 3, ..BoundedLineOptions::default() },
            );
            Ok((Curve::from(line), true))
        }
        HCurve::Circle { frame, radius } => {
            // Parameter (degrees) of the edge's start and end on the circle.
            let (a, b) = (angle_deg(frame, e.start), angle_deg(frame, e.end));
            let closed = e.vertices.0 == e.vertices.1;
            // Along the curve: from the start when same_sense, else from the end.
            let (low, mut high) = if e.same_sense { (a, b) } else { (b, a) };
            while high <= low + 1e-9 {
                high += 360.0;
            }
            if closed {
                high = low + 360.0;
            }
            let circle = Ellipse::create_canonical_circle(
                k,
                &placement(frame),
                *radius,
                Some(&Extent1d::from_min_max(low, high)),
                None,
            )?;
            Ok((Curve::from(circle), e.same_sense))
        }
        other => Err(format!("edge {edge}: unsupported curve {other:?}").into()),
    }
}

fn face_points(part: &Part, face: usize) -> Vec<V3> {
    let mut points = Vec::new();
    for l in &part.faces[face].loops {
        for &(edge, _) in &l.edges {
            points.extend_from_slice(&part.edges[edge].samples);
            points.push(part.edges[edge].start);
            points.push(part.edges[edge].end);
        }
    }
    points
}

/// The face's surface and the parameter window its boundary needs.
fn face_surface(k: &Kernel, part: &Part, face: usize) -> Result<(Surface, Extent2d), Error> {
    let points = face_points(part, face);
    let pad_box = |lo: (f64, f64), hi: (f64, f64)| {
        let pad = 0.05 * (hi.0 - lo.0).max(hi.1 - lo.1).max(1e-6) + 1e-6;
        Extent2d::from_points(
            Point2::new(lo.0 - pad, lo.1 - pad),
            Point2::new(hi.0 + pad, hi.1 + pad),
        )
    };
    let height_range = |f: &HFrame| {
        let hs = points.iter().map(|&p| dot(sub(p, f.origin), f.z));
        let (lo, hi) = hs.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), h| (a.min(h), b.max(h)));
        (lo, hi)
    };
    match &part.faces[face].surface {
        HSurface::Plane { frame } => {
            let s = Surface::from(Plane::create_canonical(&placement(frame))?);
            let us = points.iter().map(|&p| dot(sub(p, frame.origin), frame.x));
            let vs = points.iter().map(|&p| dot(sub(p, frame.origin), frame.y));
            let (ulo, uhi) = us.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(x), b.max(x)));
            let (vlo, vhi) = vs.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(x), b.max(x)));
            Ok((s, pad_box((ulo, vlo), (uhi, vhi))))
        }
        HSurface::Cylinder { frame, radius } => {
            let frame = seam_frame(frame, &points);
            let s = Surface::from(Cone::create_canonical_cylinder(k, &placement(&frame), *radius)?);
            let (lo, hi) = height_range(&frame);
            let u = s.natural_uv_domain_if_built().u_interval();
            let pad = 0.05 * (hi - lo).max(1e-6) + 1e-6;
            Ok((s, Extent2d::from_points(Point2::new(u.min(), lo - pad), Point2::new(u.max(), hi + pad))))
        }
        HSurface::Cone { frame, radius, semi_angle } => {
            let frame = seam_frame(frame, &points);
            let s = Surface::from(Cone::create_canonical_with_semi_angle(
                k,
                &placement(&frame),
                *radius,
                semi_angle.to_degrees(),
                None,
                false,
                false,
            )?);
            let (lo, hi) = height_range(&frame);
            let u = s.natural_uv_domain_if_built().u_interval();
            let pad = 0.05 * (hi - lo).max(1e-6) + 1e-6;
            Ok((s, Extent2d::from_points(Point2::new(u.min(), lo - pad), Point2::new(u.max(), hi + pad))))
        }
        HSurface::Torus { frame, major, minor } => {
            let s = Surface::from(Torus::create_canonical(
                k,
                &placement(frame),
                *major,
                *minor,
                TorusOptions::default(),
                None,
            )?);
            let uv = s.natural_uv_domain_with_nurb(k)?;
            Ok((s, uv))
        }
        other => Err(format!("face {face}: unsupported surface {:?}", other.kind()).into()),
    }
}

/// Builds solid 0 of `part` in solidcore. Returns the B-rep and, for each solidcore face, the
/// haecceity face it was made from.
fn build(k: &mut Kernel, part: &Part) -> Result<(Brep, BTreeMap<FaceId, usize>), Error> {
    let mut brep = p3::new_brep(k, solidcore::geom::tol::USE_DEFAULT, solidcore::geom::tol::USE_DEFAULT)?;
    let mut made = BTreeMap::new();
    {
        let mut state = ArenaNotifyState::new(&brep);
        let mut env = state.env(None);
        let key_of = env.key_of;
        let this_face = |f: FaceId| key_of(EntityRef::Face(f));
        let _marks = Marks::default();
        for face in part.solids[0].faces.iter().copied() {
            let (surface, uv) = face_surface(k, part, face)?;
            let mut loops = Vec::new();
            let mut curves = Vec::new();
            let mut orientations = Vec::new();
            for l in &part.faces[face].loops {
                if l.vertex.is_some() {
                    continue;
                }
                loops.push(l.edges.len());
                for &(edge, forward) in &l.edges {
                    let (curve, along) = edge_curve(k, part, edge)?;
                    curves.push(Some(curve));
                    orientations.push(if forward == along { OrientType::Same } else { OrientType::Opposite });
                }
            }
            let input = p7::FaceCurvesInput {
                loops,
                curves_3d: Some(curves),
                uv_curves: None,
                orientations,
                loop_points: Vec::new(),
            };
            let orient = if part.faces[face].reversed { OrientType::Opposite } else { OrientType::Same };
            let new = p7::make_face_with_curves(k, &mut brep, None, input, surface, &uv, orient, None, &this_face, &mut env)
                .map_err(|e| format!("face {face}: {e}"))?;
            if let Some(f) = new.face {
                made.insert(f, face);
            }
        }
    }
    let stitch = api_stitch_into_solid(k, Some(&mut brep))?;
    println!(
        "built {} faces; stitched {} edges; solid: {}",
        made.len(),
        stitch.stitched_edges,
        stitch.produces_a_solid
    );
    Ok((brep, made))
}

/// Seams and UV trims, as step-brep finishes a stitched build. A face split at its seam
/// hands its source to the new pieces in `made`.
fn finish(k: &mut Kernel, brep: &mut Brep, made: &mut BTreeMap<FaceId, usize>) -> Result<(), Error> {
    for f in faces(brep, None)? {
        let pieces = trimming_tools::split_at_seams(k, brep, f)?;
        if let Some(&source) = made.get(&f) {
            for piece in pieces {
                made.insert(piece, source);
            }
        }
    }
    for f in faces(brep, None)? {
        if !trimming_tools::face_uv_curves_present(brep, f)? {
            trimming_tools::create_uv_trim_curves(k, brep, f)?;
        }
    }
    brep.refresh_cached_topology();
    Ok(())
}

fn tag(brep: &mut Brep, f: FaceId, entity: u64) -> Result<(), Error> {
    brep.add_attribute(
        EntityRef::Face(f),
        Attribute::long(STEP_BASE + entity, entity as i64, Behavior::Copy),
    )?;
    Ok(())
}

/// STEP entity → the solidcore faces that carry its tag.
fn tags(brep: &Brep) -> Result<BTreeMap<u64, Vec<FaceId>>, Error> {
    let mut out: BTreeMap<u64, Vec<FaceId>> = BTreeMap::new();
    for f in faces(brep, None)? {
        for (_, a) in brep.entity_attributes(EntityRef::Face(f)) {
            if let (true, AttributeValue::Long(n)) = (a.attribute_id() >= STEP_BASE, a.value()) {
                out.entry(*n as u64).or_default().push(f);
            }
        }
    }
    Ok(out)
}

fn measure_all(k: &Kernel, brep: &mut Brep, map: &BTreeMap<u64, Vec<FaceId>>) -> Result<BTreeMap<u64, (usize, f64)>, Error> {
    let mut out = BTreeMap::new();
    for (e, fs) in map {
        let mut area = 0.0;
        for &f in fs {
            area += face_area(k, brep, f, 1e-6)?;
        }
        out.insert(*e, (fs.len(), area));
    }
    Ok(out)
}

fn volume(k: &Kernel, brep: &Brep) -> Result<f64, Error> {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes { store: &store, key_of: &|_| ObjectKey(0) };
    let mut copy = brep.clone_standing_for_self()?;
    Ok(brep_volume(k, &mut copy, 1e-6, &attrs)?)
}

fn bounds(part: &Part) -> (V3, V3) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for e in &part.edges {
        for p in e.samples.iter().chain([&e.start, &e.end]) {
            for i in 0..3 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
    }
    (lo, hi)
}

fn main() -> Result<(), Error> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: bridge FILE.step[.gz] [--axis x|y|z] [--keep FRACTION]")?;
    let (mut axis, mut keep) = (0usize, 0.5f64);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--axis" => axis = match args.next().as_deref() { Some("y") => 1, Some("z") => 2, _ => 0 },
            "--keep" => keep = args.next().ok_or("--keep needs a value")?.parse()?,
            other => return Err(format!("unknown argument {other}").into()),
        }
    }

    // 1. haecceity: geometry and PMI.
    let bytes = solidcore_bridge_prototype::step_bytes(std::path::Path::new(&path))?;
    let defs = read_part_definitions(&bytes)?;
    let part = read_part(&bytes, &defs[0])?;
    let doc = Document::parse(bytes.clone())?;
    let read = pmi::read(&doc, &defs)?;
    let pmi = &read.parts[0];
    let entity_of = |face: usize| part.face_source(face).map(|s| s.entity);
    println!("{path}: {} faces, {} edges, {} PMI features", part.faces.len(), part.edges.len(), pmi.features.len());
    if let Some((vol, _)) = part.solid_mass(0) {
        println!("haecceity volume {vol:.3}");
    }

    // 2. solidcore: build, tag, finish.
    let mut k = Kernel::default();
    let (mut brep, mut made) = build(&mut k, &part)?;
    finish(&mut k, &mut brep, &mut made)?;
    for (&f, &face) in &made {
        if let Some(entity) = entity_of(face) {
            tag(&mut brep, f, entity)?;
        }
    }
    let before = tags(&brep)?;
    let before_m = measure_all(&k, &mut brep, &before)?;
    let untagged = faces(&brep, None)?.len() - before.values().map(Vec::len).sum::<usize>();
    println!(
        "solidcore: {} faces, {} tagged entities ({untagged} untagged faces), volume {:.3}",
        faces(&brep, None)?.len(),
        before.len(),
        volume(&k, &brep)?
    );

    // 3. Cut: remove everything above `keep` of the way along `axis`.
    let (lo, hi) = bounds(&part);
    let margin = 10.0 + (0..3).map(|i| hi[i] - lo[i]).fold(0.0, f64::max);
    let cut_at = lo[axis] + keep * (hi[axis] - lo[axis]);
    let mut origin = [lo[0] - margin, lo[1] - margin, lo[2] - margin];
    let mut size = [hi[0] - lo[0] + 2.0 * margin, hi[1] - lo[1] + 2.0 * margin, hi[2] - lo[2] + 2.0 * margin];
    origin[axis] = cut_at;
    size[axis] = hi[axis] + margin - cut_at;
    let tool = create_box(&mut k, Point3::new(origin[0], origin[1], origin[2]), size[0], size[1], size[2])?;
    let axis_name = ["x", "y", "z"][axis];
    let mut result = boolean_difference(&mut k, brep, tool)?;
    result.refresh_cached_topology();
    let after = tags(&result)?;
    let tagged_after = after.values().map(Vec::len).sum::<usize>();
    println!(
        "after removing {axis_name} > {cut_at:.3}: {} faces ({tagged_after} tagged, {} entities), volume {:.3}",
        faces(&result, None)?.len(),
        after.len(),
        volume(&k, &result)?
    );

    // 4. What happened to each PMI feature made of faces: per STEP face, its pieces and area
    //    before and after the cut.
    let after_m = measure_all(&k, &mut result, &after)?;
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, feature) in pmi.features.iter().enumerate() {
        let Feature::Items(items) = feature else { continue };
        let entities: BTreeSet<u64> = items
            .iter()
            .filter_map(|a| match a {
                Anchor::Face(f) => entity_of(f.0),
                _ => None,
            })
            .collect();
        if entities.is_empty() {
            continue;
        }
        let mut states = Vec::new();
        let mut detail = Vec::new();
        for e in &entities {
            let (n0, a0) = before_m.get(e).copied().unwrap_or((0, 0.0));
            let state = match after_m.get(e) {
                None => "gone",
                Some(&(n1, a1)) if a1 < a0 * (1.0 - 1e-6) => {
                    detail.push(format!("#{e}: {n0}→{n1} pieces, area {a0:.2}→{a1:.2}"));
                    "trimmed"
                }
                Some(&(n1, _)) if n1 > n0 => "split",
                Some(_) => "kept",
            };
            if state == "gone" {
                detail.push(format!("#{e}: gone (area {a0:.2})"));
            } else if state != "trimmed" {
                detail.push(format!("#{e}: {state}"));
            }
            states.push(state);
        }
        let status = if states.iter().all(|s| *s == "gone") {
            "orphaned"
        } else if states.contains(&"gone") {
            "partial"
        } else if states.contains(&"trimmed") {
            "trimmed"
        } else if states.contains(&"split") {
            "split"
        } else {
            "kept"
        };
        *counts.entry(status).or_default() += 1;
        println!("  feature {i:>2}  {status:<8}  {}", detail.join("; "));
    }
    println!("summary: {counts:?}");
    Ok(())
}
