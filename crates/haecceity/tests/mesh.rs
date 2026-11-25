//! Face triangulation ([`Part::mesh`], [`Part::triangulate`]) over the corpus, at specify-core's
//! deflections (0.001 of the part's diagonal, 0.3 rad), checked against the exact geometry:
//!
//! - every face triangulates or is refused with a reason, and the refusals are the known ones;
//! - each face's mesh area is within 1% of its [`Part::face_moments`] area, or within what the
//!   boundary's chords may cut off or add (its length times the deflection);
//! - each closed shell whose faces all triangulate, none leaving out a folded triangle, is
//!   watertight: every mesh edge is used exactly once each way;
//! - triangles of analytic faces face outward (their normal agrees with the face's at the
//!   centroid), but for the known few;
//! - the surface lies within the chordal deflection of a converged analytic face's mesh at
//!   triangle centroids and at the midpoints of a sample of triangle sides (plus the edge
//!   samples' own chord tolerance, which the boundary inherits);
//! - two runs give the same mesh to the bit.
//!
//! What freeform faces do on these counts (inward triangles, deviation, faces stopped short of
//! the deflections) is printed, not asserted.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use haecceity::Part;
use haecceity::geom::{self, SurfaceType, V3};
use haecceity::mesh::{BoundaryPoint, FaceMesh, MeshRefusal};
use haecceity::read_step_file;
use haecceity::sampling::CHORD_TOLERANCE;

const DEFLECTION: f64 = 0.001;
const ANGULAR: f64 = 0.3;

/// Faces refused, each because its boundary crosses itself in parameter space even unthinned.
/// cgb202 face 399 has a neck that closes within the file's tolerance: its edges 1077 and 1078
/// pass within 15 µm of each other in space, and their samples cross on the surface (samples
/// 283 and 353 of the loop's 673, near (0.110, 0.425)), so no thinning keeps it open. 14052 is
/// the corpus's malformed file (README: its triangular pocket Python refuses on validity).
///
/// Nineteen faces refused before now mesh: on the tori (cgb207 faces 50, 54; cgb242 faces 422,
/// 426, 429, 430, and 10 and 46, which met no crossing only while their neighbours' refusals had
/// their edges thinned less) an edge meeting a parameter line tangentially strays past it within
/// tolerance, and its samples there are dropped (`straying` in mesh.rs,
/// `torus_edge_straying_past_its_top_circle_is_dropped`); on the B-splines (cgb202 faces 1034,
/// 1214; cgb207 face 222; cgb242 faces 483, 501, 505, 509, 513, 551, 552, 856, 863; cgb243 face
/// 543) the samples beside a collapsed side's apex had stalled on that side, off their edge
/// (`beside_collapsed` in uv.rs, `samples_beside_a_collapsed_side_invert_onto_their_edge`).
const KNOWN_REFUSALS: &[(&str, &[usize])] = &[
    ("cadgenbench_inputs/cgb202.step.gz", &[399]),
    ("inventory_refusal/14052.step.gz", &[1]),
];

/// Analytic faces with inward triangles, and how many: slivers beside a boundary side (a few
/// thousandths as high as they are long), where refinement inserted points hard by a side it
/// may not split (the side is shared with the neighbouring face) and the side's sag tilts the
/// sliver over. cgb203 face 55: two of 322 triangles, 2.1 and 3.2 mm long along edge 179.
///
/// The torus faces of cgb207 and cgb242 listed are the same in a cusp: where an edge meets the
/// tube's top circle (a boundary side along v = ±π/2) tangentially, the face between them
/// narrows to nothing, and refinement fills the cusp with points ever nearer the circle, whose
/// slivers stand across its sag (cgb242 face 420: two triangles 1.4 and 2.1 mm long at vertex 47,
/// within 1e-7 of the circle in v). cgb242 faces 420, 424, 428 and 431 had none while the faces
/// then refused beside them (422 to 430) had their shared edges thinned to the samples; cgb242
/// face 10 had eight then and has none now.
const KNOWN_INWARD: &[(&str, usize, usize)] = &[
    ("cadgenbench_inputs/cgb202.step.gz", 1324, 10),
    ("cadgenbench_inputs/cgb203.step", 55, 2),
    ("cadgenbench_inputs/cgb203.step", 56, 2),
    ("cadgenbench_inputs/cgb203.step", 57, 2),
    ("cadgenbench_inputs/cgb203.step", 58, 2),
    ("cadgenbench_inputs/cgb203.step", 59, 2),
    ("cadgenbench_inputs/cgb203.step", 60, 2),
    ("cadgenbench_inputs/cgb203.step", 61, 2),
    ("cadgenbench_inputs/cgb207.step", 50, 16),
    ("cadgenbench_inputs/cgb207.step", 54, 5),
    ("cadgenbench_inputs/cgb207.step", 73, 6),
    ("cadgenbench_inputs/cgb242.step.gz", 46, 4),
    ("cadgenbench_inputs/cgb242.step.gz", 420, 2),
    ("cadgenbench_inputs/cgb242.step.gz", 422, 3),
    ("cadgenbench_inputs/cgb242.step.gz", 424, 2),
    ("cadgenbench_inputs/cgb242.step.gz", 426, 3),
    ("cadgenbench_inputs/cgb242.step.gz", 428, 5),
    ("cadgenbench_inputs/cgb242.step.gz", 429, 3),
    ("cadgenbench_inputs/cgb242.step.gz", 430, 4),
    ("cadgenbench_inputs/cgb242.step.gz", 431, 2),
    ("cadgenbench_inputs/cgb242.step.gz", 621, 1),
    ("cadgenbench_inputs/cgb242.step.gz", 623, 1),
    ("cadgenbench_inputs/cgb243.step.gz", 159, 4),
];

fn deflection(part: &Part) -> f64 {
    let d = part.bounds().diagonal();
    DEFLECTION * if d > 0.0 && d.is_finite() { d } else { 1.0 }
}

fn analytic(kind: SurfaceType) -> bool {
    !matches!(kind, SurfaceType::Freeform | SurfaceType::Other)
}

/// A mesh vertex across the part: a point on its edges, or a face's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Vertex {
    Boundary(BoundaryPoint),
    Inner(usize, usize),
}

fn vertex(face: usize, mesh: &FaceMesh, k: usize) -> Vertex {
    match mesh.boundary[k] {
        Some(p) => Vertex::Boundary(p),
        None => Vertex::Inner(face, k),
    }
}

/// Whether every edge of the faces is used twice by them (a seam twice by one face).
fn closed_shell(part: &Part, faces: &[usize]) -> bool {
    let mut uses: BTreeMap<usize, usize> = BTreeMap::new();
    for &f in faces {
        for lp in &part.faces[f].loops {
            for &(e, _) in &lp.edges {
                *uses.entry(e).or_default() += 1;
            }
        }
    }
    !uses.is_empty() && uses.values().all(|&n| n == 2)
}

/// How many of the faces' directed mesh edges are not used exactly once, with their reverse
/// used exactly once.
fn unpaired_edges(faces: &[usize], meshes: &[Result<FaceMesh, MeshRefusal>]) -> usize {
    let mut directed: BTreeMap<(Vertex, Vertex), usize> = BTreeMap::new();
    for &f in faces {
        let mesh = meshes[f].as_ref().expect("all faces meshed");
        for t in &mesh.triangles {
            for i in 0..3 {
                let (a, b) = (vertex(f, mesh, t[i]), vertex(f, mesh, t[(i + 1) % 3]));
                *directed.entry((a, b)).or_default() += 1;
            }
        }
    }
    directed
        .iter()
        .filter(|&(&(a, b), &n)| n != 1 || directed.get(&(b, a)) != Some(&1))
        .count()
}

#[test]
fn square_mesh_distinguishes_a_selected_trim_from_the_spatial_fallback() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/master-trim-square.step");
    let selected = read_step_file(&path).unwrap();
    let spatial = common::with_spatial_edges(&path, &[118, 130, 142, 154].into_iter().collect());
    let invalid = std::fs::read_to_string(path)
        .unwrap()
        .replace(".PCURVE_S1.", ".PCURVE_S2.");
    let fallback = haecceity::read_step(invalid.as_bytes()).unwrap();

    let area = selected.triangulate(0, 1e-5, ANGULAR).unwrap().area();
    assert!((area - 0.999_866_666_7).abs() < 1e-5, "{area}");
    let spatial_mesh = spatial.triangulate(0, 1e-5, ANGULAR).unwrap();
    assert!((spatial_mesh.area() - 1.0).abs() < 1e-12);
    assert_eq!(
        fallback.triangulate(0, 1e-5, ANGULAR).unwrap(),
        spatial_mesh
    );
}

#[test]
fn selected_trim_mesh_agrees_with_the_independent_cgb243_area() {
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let path = dir.join("cadgenbench_inputs/cgb243.step.gz");
    let part = read_step_file(&path).unwrap();
    let face = 543;
    assert_eq!(part.face_source(face).unwrap().entity, 60_132);
    // Independent Green integration of the four carrier pcurves gives
    // 0.481554752985 and 0.481554754720 mm² after Richardson extrapolation.
    // Two independent higher-order integrals give 0.4815547641 and 0.4815547643.
    let reference = 0.481_554_764_2;
    let exact = part.face_moments(face).unwrap().area;
    assert!((exact - reference).abs() < 1e-9);
    let d = deflection(&part);
    let single = part.triangulate(face, d, ANGULAR).unwrap();
    let all = part.mesh(d, ANGULAR);
    for mesh in [&single, all[face].as_ref().unwrap()] {
        let area = mesh.area();
        eprintln!(
            "#60132: {} triangles, {area:.12} mm², {:.6}% difference",
            mesh.triangles.len(),
            100.0 * (area / exact - 1.0).abs()
        );
        assert!((area / exact - 1.0).abs() <= 0.01);
    }
    // A denser uniform seed can introduce inward slivers on neighbouring
    // analytic faces even while #60132's area improves. Keep the original
    // corpus winding criterion alongside the independent area check.
    for (face, entity) in [(342, 50_870), (541, 60_095)] {
        assert_eq!(part.face_source(face).unwrap().entity, entity);
        let mesh = all[face].as_ref().unwrap();
        assert!(mesh.converged);
        assert_outward_mesh(&part, face, mesh, d);
    }
    // Rotating the part changes its axis-aligned bounds and hence the default
    // deflection. The 64-segment grid missed 1% at 37 degrees and the scoped
    // 128-segment grid missed it at 85 degrees around Z.
    let (s, c) = 37_f64.to_radians().sin_cos();
    let (s85, c85) = 85_f64.to_radians().sin_cos();
    for placement in [
        [[c, -s, 0., 0.], [s, c, 0., 0.], [0., 0., 1., 0.]],
        [[1., 0., 0., 123.456], [0., c, -s, -78.9], [0., s, c, 41.3]],
        [[c85, -s85, 0., 0.], [s85, c85, 0., 0.], [0., 0., 1., 0.]],
    ] {
        let placed = haecceity::step::read_step_file_placed(&path, &placement).unwrap();
        assert_eq!(placed.face_source(face).unwrap().entity, 60_132);
        let mesh = placed
            .triangulate(face, deflection(&placed), ANGULAR)
            .unwrap();
        assert!((mesh.area() / reference - 1.0).abs() <= 0.01);
    }
}

fn assert_outward_mesh(part: &Part, face: usize, mesh: &FaceMesh, d: f64) {
    for tri in &mesh.triangles {
        let points = tri.map(|i| mesh.points[i]);
        let uv = tri.map(|i| mesh.uv[i]);
        let hint = (
            uv.iter().map(|p| p.0).sum::<f64>() / 3.0,
            uv.iter().map(|p| p.1).sum::<f64>() / 3.0,
        );
        let cross = geom::cross(
            geom::sub(points[1], points[0]),
            geom::sub(points[2], points[0]),
        );
        let longest = (0..3)
            .map(|i| geom::dist(points[i], points[(i + 1) % 3]))
            .fold(0.0, f64::max);
        if geom::norm(cross) > 1e-6 * d * longest {
            let (_, (u, v)) = off_surface(part, face, mean(&points), hint).unwrap();
            let normal = part.face_normal(face, u, v).unwrap();
            assert!(
                geom::dot(cross, normal) > 0.0,
                "face {face}: inward triangle {tri:?}"
            );
        }
    }
}

/// Integral knot panels must not stagger the opposite sides of a narrow ribbon
/// when the part's incident faces share an edge sampling plan.
#[test]
fn whole_part_mesh_preserves_narrow_selected_ribbons() {
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let part = read_step_file(&dir.join("cadgenbench/threaded_connector_109.step")).unwrap();
    let d = deflection(&part);
    let meshes = part.mesh(d, ANGULAR);
    for (face, entity) in [
        (55, 9366),
        (112, 16858),
        (148, 20421),
        (136, 20024),
        (142, 20284),
        (141, 20250),
    ] {
        assert_eq!(part.face_source(face).unwrap().entity, entity);
        let mesh = meshes[face].as_ref().unwrap();
        let area = part.face_moments(face).unwrap().area;
        assert!(mesh.converged, "face {face} did not converge");
        assert!(
            (mesh.area() / area - 1.0).abs() <= 0.01,
            "face {face}: {} against {area}",
            mesh.area()
        );
        assert_eq!(mesh.folded_dropped.0, 0, "face {face} folded");
    }
    // These two distinct, already inaccurate faces gained inward triangles
    // with 192 starting segments. Check orientation independently of area.
    for (face, entity) in [(16, 4301), (138, 20128)] {
        assert_eq!(part.face_source(face).unwrap().entity, entity);
        assert_outward_mesh(&part, face, meshes[face].as_ref().unwrap(), d);
    }
}

#[test]
fn a_selected_knot_near_a_step_vertex_has_one_shared_mesh_endpoint() {
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let part = read_step_file(&dir.join("cadgenbench_inputs/cgb245.step")).unwrap();
    // A selected spline sample near the end of STEP edge #28519 must not
    // introduce a second vertex at the cylinder and plane's shared endpoint.
    let faces = &part.solids[0].faces;
    assert!(closed_shell(&part, faces));
    let meshes = part.mesh(deflection(&part), ANGULAR);
    assert_eq!(unpaired_edges(faces, &meshes), 0);
}

#[test]
fn a_selected_curve_crossing_a_closed_spline_chart_keeps_the_shell_watertight() {
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let part = read_step_file(&dir.join("gramel/string_post.step")).unwrap();
    // Edge #634 selects the cylinder pcurve. Its samples also bound the
    // adjoining closed spline #1020, whose inverse crosses the chart seam.
    let edge = (0..part.edges.len())
        .find(|&e| part.edge_source(e).is_some_and(|s| s.entity == 634))
        .unwrap();
    let spline = (0..part.faces.len())
        .find(|&f| part.face_source(f).is_some_and(|s| s.entity == 1668))
        .unwrap();
    let incident = &part.edge_faces()[edge];
    assert_eq!(incident.len(), 2);
    assert!(incident.contains(&spline));
    let faces = &part.solids[0].faces;
    assert!(closed_shell(&part, faces));
    let meshes = part.mesh(deflection(&part), ANGULAR);
    assert_eq!(unpaired_edges(faces, &meshes), 0);
    // Boundary IDs alone could miss a crack if each face placed its own point.
    let mut shared = BTreeMap::new();
    for &face in faces {
        let mesh = meshes[face].as_ref().unwrap();
        if incident.contains(&face) {
            assert!(
                mesh.boundary.iter().any(
                    |key| matches!(key, Some(BoundaryPoint::Edge { edge: e, .. }) if *e == edge)
                )
            );
        }
        for (&key, &point) in mesh.boundary.iter().zip(&mesh.points) {
            if let Some(key) = key {
                let point = point.map(f64::to_bits);
                assert_eq!(*shared.entry(key).or_insert(point), point);
            }
        }
    }
}

fn mean(points: &[V3]) -> V3 {
    let sum = points.iter().fold([0.0; 3], |s, p| geom::add(s, *p));
    geom::scale(sum, 1.0 / points.len() as f64)
}

/// The distance from `p` to the face's surface and its foot's parameters, found from `hint`.
fn off_surface(part: &Part, face: usize, p: V3, hint: (f64, f64)) -> Option<(f64, (f64, f64))> {
    let surface = &part.faces[face].surface;
    let (u, v) = surface.parameters(p, Some(hint))?;
    Some((geom::dist(surface.value(u, v), p), (u, v)))
}

/// The length of the face's boundary, along its edges' samples.
fn perimeter(part: &Part, face: usize) -> f64 {
    part.faces[face]
        .loops
        .iter()
        .flat_map(|lp| &lp.edges)
        .map(|&(e, _)| {
            part.edges[e]
                .samples
                .windows(2)
                .map(|w| geom::dist(w[0], w[1]))
                .sum::<f64>()
        })
        .sum()
}

#[derive(Default)]
struct Report {
    files: usize,
    unread: Vec<String>,
    faces: usize,
    triangles: usize,
    /// (file, face) → reason, and the count per surface type.
    refused: BTreeMap<(String, usize), String>,
    refused_kinds: BTreeMap<String, usize>,
    unconverged: Vec<String>,
    folded: Vec<String>,
    /// Relative area difference per face with a moments area.
    areas: Vec<f64>,
    area_failures: Vec<String>,
    shells: usize,
    open_shells: usize,
    folded_shells: Vec<String>,
    leaky: Vec<String>,
    /// (file, face) → inward triangles, on analytic faces.
    inward_analytic: BTreeMap<(String, usize), usize>,
    inward_freeform: Vec<String>,
    deviation_analytic: Vec<String>,
    deviation_other: Vec<String>,
    worst_deviation: f64,
    nondeterministic: Vec<String>,
    seconds: f64,
}

impl Report {
    fn absorb(&mut self, r: Report) {
        self.files += r.files;
        self.unread.extend(r.unread);
        self.faces += r.faces;
        self.triangles += r.triangles;
        self.refused.extend(r.refused);
        for (k, n) in r.refused_kinds {
            *self.refused_kinds.entry(k).or_default() += n;
        }
        self.unconverged.extend(r.unconverged);
        self.folded.extend(r.folded);
        self.areas.extend(r.areas);
        self.area_failures.extend(r.area_failures);
        self.shells += r.shells;
        self.open_shells += r.open_shells;
        self.folded_shells.extend(r.folded_shells);
        self.leaky.extend(r.leaky);
        self.inward_analytic.extend(r.inward_analytic);
        self.inward_freeform.extend(r.inward_freeform);
        self.deviation_analytic.extend(r.deviation_analytic);
        self.deviation_other.extend(r.deviation_other);
        self.worst_deviation = self.worst_deviation.max(r.worst_deviation);
        self.nondeterministic.extend(r.nondeterministic);
        self.seconds += r.seconds;
    }
}

fn check_file(dir: &std::path::Path, file: &str, out: &mut Report) {
    out.files += 1;
    let part = match read_step_file(&dir.join(file)) {
        Ok(part) => part,
        Err(e) => {
            out.unread.push(format!("{file}: {e}"));
            return;
        }
    };
    let deflection = deflection(&part);
    let start = Instant::now();
    let meshes = part.mesh(deflection, ANGULAR);
    out.seconds += start.elapsed().as_secs_f64();
    assert_eq!(meshes.len(), part.faces.len());
    for (face, result) in meshes.iter().enumerate() {
        out.faces += 1;
        let kind = part.faces[face].surface.kind();
        let name = format!("{file} face {face} ({kind:?})");
        let mesh = match result {
            Ok(mesh) => mesh,
            Err(why) => {
                assert!(!why.reason.is_empty(), "{name}: refused without a reason");
                out.refused
                    .insert((file.to_owned(), face), why.reason.clone());
                *out.refused_kinds.entry(format!("{kind:?}")).or_default() += 1;
                continue;
            }
        };
        assert!(!mesh.triangles.is_empty(), "{name}: no triangles");
        out.triangles += mesh.triangles.len();
        if !mesh.converged {
            out.unconverged.push(name.clone());
        }
        if mesh.folded_dropped.0 > 0 {
            out.folded.push(format!(
                "{name}: {} triangles, {:.3e} mm2",
                mesh.folded_dropped.0, mesh.folded_dropped.1
            ));
        }
        if let Some(m) = part.face_moments(face)
            && m.area > 0.0
        {
            let got = mesh.area();
            out.areas.push((got - m.area) / m.area);
            if (got - m.area).abs() > 0.01 * m.area
                && (got - m.area).abs() > perimeter(&part, face) * deflection
            {
                out.area_failures
                    .push(format!("{name}: mesh {got:.6} moments {:.6}", m.area));
            }
        }
        let (mut inward, mut worst) = (0, 0.0_f64);
        for (t, tri) in mesh.triangles.iter().enumerate() {
            let corners = tri.map(|k| mesh.points[k]);
            let uv = tri.map(|k| mesh.uv[k]);
            let hint = (
                (uv[0].0 + uv[1].0 + uv[2].0) / 3.0,
                (uv[0].1 + uv[1].1 + uv[2].1) / 3.0,
            );
            let doubled = geom::cross(
                geom::sub(corners[1], corners[0]),
                geom::sub(corners[2], corners[0]),
            );
            let longest = (0..3)
                .map(|i| geom::dist(corners[i], corners[(i + 1) % 3]))
                .fold(0.0, f64::max);
            if let Some((d, (u, v))) = off_surface(&part, face, mean(&corners), hint) {
                worst = worst.max(d);
                // A triangle under a millionth of the deflection high has no direction.
                if geom::norm(doubled) > 1e-6 * deflection * longest
                    && part
                        .face_normal(face, u, v)
                        .is_some_and(|n| geom::dot(n, doubled) <= 0.0)
                {
                    inward += 1;
                }
            }
            if t % 5 == 0 {
                for i in 0..3 {
                    let j = (i + 1) % 3;
                    let hint = ((uv[i].0 + uv[j].0) / 2.0, (uv[i].1 + uv[j].1) / 2.0);
                    if let Some((d, _)) =
                        off_surface(&part, face, mean(&[corners[i], corners[j]]), hint)
                    {
                        worst = worst.max(d);
                    }
                }
            }
        }
        if inward > 0 {
            if analytic(kind) {
                out.inward_analytic.insert((file.to_owned(), face), inward);
            } else {
                out.inward_freeform
                    .push(format!("{name}: {inward} of {}", mesh.triangles.len()));
            }
        }
        out.worst_deviation = out.worst_deviation.max(worst / deflection);
        if worst > deflection + CHORD_TOLERANCE {
            let line = format!("{name}: {:.3} deflections", worst / deflection);
            if analytic(kind) && mesh.converged {
                out.deviation_analytic.push(line);
            } else {
                out.deviation_other.push(line);
            }
        }
    }
    for (s, solid) in part.solids.iter().enumerate() {
        if !closed_shell(&part, &solid.faces) {
            out.open_shells += 1;
            continue;
        }
        if solid.faces.iter().any(|&f| meshes[f].is_err()) {
            continue;
        }
        if solid
            .faces
            .iter()
            .any(|&f| meshes[f].as_ref().is_ok_and(|m| m.folded_dropped.0 > 0))
        {
            out.folded_shells.push(format!("{file} solid {s}"));
            continue;
        }
        out.shells += 1;
        let unpaired = unpaired_edges(&solid.faces, &meshes);
        if unpaired > 0 {
            out.leaky
                .push(format!("{file} solid {s}: {unpaired} unpaired mesh edges"));
        }
    }
    if part.mesh(deflection, ANGULAR) != meshes {
        out.nondeterministic.push(file.to_owned());
    }
}

/// Two fixtures without the corpus: a whole torus (two seam edges, both periodic directions)
/// and a filleted plate with holes. Every face meshes outward, its area within 1% of its
/// moments area; the solid is watertight; a face meshed alone is the face in the whole part's
/// mesh (neither needs an edge thinned less); and a deflection that is not a length is refused.
#[test]
fn fixture_faces_mesh_watertight_alone_as_in_the_part() {
    for name in ["full_torus.step", "filleted_plate_with_holes.step"] {
        let part = read_step_file(&common::fixtures().join(name)).unwrap();
        let deflection = deflection(&part);
        let meshes = part.mesh(deflection, ANGULAR);
        for (face, mesh) in meshes.iter().enumerate() {
            let mesh = mesh
                .as_ref()
                .unwrap_or_else(|e| panic!("{name} face {face}: {e}"));
            let exact = part.face_moments(face).unwrap().area;
            assert!(
                (mesh.area() - exact).abs() <= 0.01 * exact,
                "{name} face {face}: {} against {exact}",
                mesh.area()
            );
            for tri in &mesh.triangles {
                let corners = tri.map(|k| mesh.points[k]);
                let uv = tri.map(|k| mesh.uv[k]);
                let hint = (
                    (uv[0].0 + uv[1].0 + uv[2].0) / 3.0,
                    (uv[0].1 + uv[1].1 + uv[2].1) / 3.0,
                );
                let (_, (u, v)) = off_surface(&part, face, mean(&corners), hint).unwrap();
                let doubled = geom::cross(
                    geom::sub(corners[1], corners[0]),
                    geom::sub(corners[2], corners[0]),
                );
                assert!(geom::dot(part.face_normal(face, u, v).unwrap(), doubled) > 0.0);
            }
            let alone = part.triangulate(face, deflection, ANGULAR).unwrap();
            assert!(&alone == mesh, "{name} face {face}: alone differs");
        }
        for solid in &part.solids {
            assert!(closed_shell(&part, &solid.faces), "{name}");
            assert_eq!(unpaired_edges(&solid.faces, &meshes), 0, "{name}");
        }
    }
    let part = read_step_file(&common::fixtures().join("full_torus.step")).unwrap();
    let refusal = part.triangulate(0, 0.0, ANGULAR).unwrap_err();
    assert_eq!(refusal.reason, "deflection 0 is not a positive length");
    assert!(part.mesh(f64::NAN, ANGULAR).iter().all(|m| m.is_err()));
}

#[test]
fn corpus_faces_mesh_watertight_outward_and_within_deflection() {
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let corpus = common::load("corpus.json");
    let files: Vec<String> = corpus["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["file"].as_str().unwrap().to_owned())
        .collect();
    let mut all = Report::default();
    for r in common::parallel::map(&files, |file| {
        let mut r = Report::default();
        check_file(&dir, file, &mut r);
        r
    }) {
        all.absorb(r);
    }

    println!(
        "{} files ({} unread), {} faces, {} triangles; meshing took {:.1} s summed over threads",
        all.files,
        all.unread.len(),
        all.faces,
        all.triangles,
        all.seconds
    );
    println!("refused by surface type: {:?}", all.refused_kinds);
    for ((file, face), why) in &all.refused {
        println!("  {file} face {face}: {why}");
    }
    let mut areas: Vec<f64> = all.areas.iter().map(|a| a.abs()).collect();
    areas.sort_by(f64::total_cmp);
    let n = areas.len();
    let at = |q: f64| areas[((n - 1) as f64 * q) as usize];
    println!(
        "area against face_moments: {} of {n} within 1%; |difference| median {:.2e}, p90 {:.2e}, \
         p99 {:.2e}, max {:.2e}; {} beyond both 1% and perimeter x deflection",
        areas.iter().filter(|&&a| a <= 0.01).count(),
        at(0.5),
        at(0.9),
        at(0.99),
        at(1.0),
        all.area_failures.len()
    );
    println!(
        "closed shells: {} checked, {} skipped for a folded triangle left out {:?}, {} open",
        all.shells,
        all.folded_shells.len(),
        all.folded_shells,
        all.open_shells
    );
    println!(
        "{} faces stopped short of the deflections; {} with folded triangles left out; {} \
         freeform faces with inward triangles; {} freeform or stopped faces beyond the \
         deflection; worst deviation {:.3} deflections",
        all.unconverged.len(),
        all.folded.len(),
        all.inward_freeform.len(),
        all.deviation_other.len(),
        all.worst_deviation
    );

    for ((file, face), n) in &all.inward_analytic {
        println!("inward triangles on analytic faces: {file} face {face}: {n}");
    }
    for (what, list) in [
        ("not watertight", &all.leaky),
        (
            "beyond the deflection on analytic faces",
            &all.deviation_analytic,
        ),
        ("area beyond both bounds", &all.area_failures),
    ] {
        for line in list {
            println!("{what}: {line}");
        }
    }

    assert!(all.unread.is_empty(), "unread: {:#?}", all.unread);
    let known: BTreeSet<(String, usize)> = KNOWN_REFUSALS
        .iter()
        .flat_map(|(file, faces)| faces.iter().map(|&f| ((*file).to_owned(), f)))
        .collect();
    let refused: BTreeSet<(String, usize)> = all.refused.keys().cloned().collect();
    assert_eq!(refused, known, "refusals changed: {:#?}", all.refused);
    assert!(
        all.area_failures.is_empty(),
        "areas: {:#?}",
        all.area_failures
    );
    assert!(all.leaky.is_empty(), "not watertight: {:#?}", all.leaky);
    let known: BTreeMap<(String, usize), usize> = KNOWN_INWARD
        .iter()
        .map(|&(file, face, n)| ((file.to_owned(), face), n))
        .collect();
    assert_eq!(
        all.inward_analytic, known,
        "inward triangles on analytic faces changed"
    );
    assert!(
        all.deviation_analytic.is_empty(),
        "beyond the deflection: {:#?}",
        all.deviation_analytic
    );
    assert!(
        all.nondeterministic.is_empty(),
        "two runs differ: {:?}",
        all.nondeterministic
    );
}

/// A corpus face meshed with the whole part, its area within 1% of its moments area.
fn meshed(part: &Part, face: usize, name: &str) -> FaceMesh {
    let mesh = part.mesh(deflection(part), ANGULAR).swap_remove(face);
    let mesh = mesh.unwrap_or_else(|e| panic!("{name} face {face}: {e}"));
    let exact = part.face_moments(face).unwrap().area;
    assert!(
        (mesh.area() - exact).abs() <= 0.01 * exact,
        "{name} face {face}: {} against {exact}",
        mesh.area()
    );
    mesh
}

/// cgb207 face 50 and cgb242 face 422 are torus faces whose edge meets the tube's top circle
/// (their side along v = π/2, or -π/2) tangentially and runs on past it within the file's
/// tolerance (to v = 1.5719 and -1.5726), crossing the side in parameter space. The samples past
/// it are dropped, the faces mesh, and no mesh vertex lies past the circle.
#[test]
fn torus_edge_straying_past_its_top_circle_is_dropped() {
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let half = std::f64::consts::FRAC_PI_2;
    for (file, face, line, sign) in [
        ("cadgenbench_inputs/cgb207.step", 50, half, 1.0),
        ("cadgenbench_inputs/cgb242.step.gz", 422, -half, -1.0),
    ] {
        let part = read_step_file(&dir.join(file)).unwrap();
        let past = |v: f64, by: f64| sign * (v - line) > by;
        let lp = &part.uv_loops(face).unwrap()[0];
        assert!(
            lp.points.iter().any(|p| past(p.1, 1e-4)),
            "{file} face {face}: no sample strays past v = {line}"
        );
        let mesh = meshed(&part, face, file);
        assert!(
            mesh.uv.iter().all(|p| !past(p.1, 1e-9)),
            "{file} face {face}: a mesh vertex lies past v = {line}"
        );
    }
}

/// cgb202 face 1034 and cgb243 face 543 are B-spline faces with a side collapsed to a point,
/// their boundary running through it. The samples after it were inverted from the apex's
/// parameters, where the search stalls on the side, and stopped there 0.04 to 0.1 mm from their
/// edge, so the loop jumped across the face. Now every loop point lies within 0.01 mm of its edge
/// sample (3 µm at most, the edges standing off the surface within their tolerance), and the
/// faces mesh.
#[test]
fn samples_beside_a_collapsed_side_invert_onto_their_edge() {
    let Some(dir) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    for (file, face) in [
        ("cadgenbench_inputs/cgb202.step.gz", 1034),
        ("cadgenbench_inputs/cgb243.step.gz", 543),
    ] {
        let part = read_step_file(&dir.join(file)).unwrap();
        let f = &part.faces[face];
        let mut worst = 0.0_f64;
        for (li, lp) in part.uv_loops(face).unwrap().iter().enumerate() {
            for (k, source) in lp.sources.iter().enumerate() {
                let Some((place, sample)) = *source else {
                    continue;
                };
                let edge = f.loops[li].edges[place].0;
                let (u, v) = lp.points[k];
                let off = geom::dist(f.surface.value(u, v), part.edges[edge].samples[sample]);
                assert!(
                    off <= 0.01,
                    "{file} face {face}: edge {edge} sample {sample} lies {off:.3e} mm off"
                );
                worst = worst.max(off);
            }
        }
        println!("{file} face {face}: loop points at most {worst:.2e} mm off their samples");
        meshed(&part, face, file);
    }
}
