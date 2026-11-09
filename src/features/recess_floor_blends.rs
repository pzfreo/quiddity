//! Closed rectangular cavities with analytic wall-to-floor rounds (#228).
//! A mouth inner wire and its entire bounded region prove closure independently
//! of the wall-pair floor-height heuristic. No open-end or shared-floor inference.
use super::*;
use crate::features::recess_records::FloorFillet;
use crate::kernel::geom::Surface;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= COORD_FLOOR
}

#[derive(Default)]
pub(super) struct Analysis {
    pub proposals: Vec<Proposal<Pocket>>,
    pub unsupported: Vec<BTreeSet<usize>>,
}

enum Reading {
    Supported(Box<Proposal<Pocket>>),
    Unsupported(BTreeSet<usize>),
}
impl Reading {
    fn region(&self) -> &BTreeSet<usize> {
        match self {
            Self::Supported(p) => &p.constituent,
            Self::Unsupported(r) => r,
        }
    }
}

pub(super) fn analyse(scan: &Scan<'_, '_>) -> Analysis {
    let part = scan.ctx.part;
    let mut out = Vec::new();
    for opening in &scan.faces {
        let Some(depth) = opening.axis else { continue };
        for wire in inner_wires(part, opening.node) {
            if let Some(p) = prove(scan, opening, depth, &wire) {
                out.push(p);
            }
        }
    }
    let overlaps: Vec<_> = out
        .iter()
        .enumerate()
        .map(|(i, p)| {
            out.iter()
                .enumerate()
                .any(|(j, q)| i != j && !p.region().is_disjoint(q.region()))
        })
        .collect();
    let mut analysis = Analysis::default();
    for (p, overlap) in out.into_iter().zip(overlaps) {
        if !overlap {
            match p {
                Reading::Supported(p) => analysis.proposals.push(*p),
                Reading::Unsupported(r) => analysis.unsupported.push(r),
            }
        }
    }
    analysis
}

fn prove(scan: &Scan<'_, '_>, opening: &Face, depth: usize, wire: &[usize]) -> Option<Reading> {
    let part = scan.ctx.part;
    let g = &scan.graph;
    let seed = wire_seed(part, opening.node, wire);
    let sign = if opening.normal[depth] > 0. { 1 } else { -1 };
    let mouth = center(&opening.bb, depth);
    let planar: BTreeSet<_> = seed.iter().copied().filter(|&f| g.is_planar(f)).collect();
    if planar.len() != 4 || !matches!(seed.len(), 4 | 8) || wire.len() != seed.len() {
        return None;
    }
    // Reject holes and nonrectangular mouths before walking their whole region.
    let region = bounded_inner_region(g, opening.node, &seed);
    let members: Vec<_> = region.iter().copied().chain([opening.node]).collect();
    if common_valid_solid(part, &members) != Some(scan.solid) {
        return None;
    }
    let mut bounds = part.face_bounds(*seed.first()?);
    for &f in &seed {
        bounds.merge(&part.face_bounds(f));
    }
    let axes: Vec<_> = (0..3).filter(|&a| a != depth).collect();
    let sizes = axes
        .iter()
        .map(|&a| bounds.max[a] - bounds.min[a])
        .collect::<Vec<_>>();
    if close(sizes[0], sizes[1]) {
        return None;
    } // no arbitrary run on a square
    let (width, long) = if sizes[0] < sizes[1] {
        (axes[0], axes[1])
    } else {
        (axes[1], axes[0])
    };
    let mut sides = BTreeSet::new();
    for &f in &planar {
        let normal = graph::normal(part, f)?;
        let axis = super::super::recess_faces::dominant_axis(normal)?;
        if axis == depth {
            return None;
        }
        let b = part.face_bounds(f);
        let low = normal[axis] > 0.;
        let at = if low {
            bounds.min[axis]
        } else {
            bounds.max[axis]
        };
        if !close(b.min[axis], at)
            || !close(b.max[axis], at)
            || !close(if sign > 0 { b.max[depth] } else { b.min[depth] }, mouth)
            || !sides.insert((axis, low))
        {
            return None;
        }
    }
    // There must be exactly one intact planar floor, facing the opening. Every
    // other face below the mouth must be a proved floor transition or a mouth wall.
    let floors: Vec<_> = region
        .difference(&seed)
        .copied()
        .filter(|&f| g.is_planar(f))
        .collect();
    let [floor] = floors[..] else { return None };
    let fb = part.face_bounds(floor);
    let floor_at = center(&fb, depth);
    let floor_normal = graph::normal(part, floor)?;
    if floor_normal[depth] * f64::from(sign) < 1. - super::super::policy::AXIS_ZERO_COS
        || !close(fb.min[depth], fb.max[depth])
        || (mouth - floor_at) * f64::from(sign) <= COORD_FLOOR
        || part.faces[floor].loops.len() != 1
    {
        return None;
    }
    let transitions: BTreeSet<_> = region
        .difference(&seed)
        .copied()
        .filter(|&f| f != floor)
        .collect();
    if transitions.is_empty() {
        return None;
    } // sharp floors retain the existing path
    let floor_neighbours = g.neighbours(floor);
    let mut fillets = Vec::new();
    let mut unsupported = false;
    for &f in &transitions {
        let sphere = matches!(part.faces[f].surface, Surface::Sphere { .. });
        if !sphere && (!floor_neighbours.contains(&f) || g.arc(floor, f) != Some(Arc::Smooth)) {
            return None;
        }
        let walls: Vec<_> = g
            .neighbours(f)
            .iter()
            .copied()
            .filter(|w| seed.contains(w) && g.arc(f, *w) == Some(Arc::Smooth))
            .collect();
        let [wall] = walls[..] else { return None };
        if g.arc(wall, f) != Some(Arc::Smooth) {
            return None;
        }
        let radius = match transition_radius(scan, f, wall, depth, floor_at, sign) {
            Some(r) => r,
            None => {
                // STEP may encode a deep rolling corner as a nonanalytic
                // revolution. Its complete closed floor is still observed, but
                // its height is not a certified radius and is never published.
                if !matches!(
                    part.faces[f].surface,
                    Surface::Freeform {
                        kind: "REVOLUTION",
                        ..
                    }
                ) || !matches!(part.faces[wall].surface, Surface::Cylinder { .. })
                {
                    return None;
                }
                unsupported = true;
                let span = g.span(f, depth);
                span.1 - span.0
            }
        };
        if sphere {
            let Surface::Sphere { frame, .. } = part.faces[f].surface else {
                unreachable!()
            };
            let mut contact = frame.origin;
            contact[depth] = floor_at;
            if !graph::face_vertices(part, floor)
                .iter()
                .any(|&p| crate::kernel::geom::dist(p, contact) <= COORD_FLOOR)
            {
                return None;
            }
            let springs: Vec<_> = g
                .neighbours(f)
                .iter()
                .copied()
                .filter(|n| floor_neighbours.contains(n))
                .collect();
            if springs.len() != 2 || springs.iter().any(|&n| {
                g.arc(f, n) != Some(Arc::Smooth) || !matches!(part.faces[n].surface, Surface::Cylinder { radius: r, .. } if close(r, radius))
            }) { return None; }
        }
        if round_measurement(radius, 3) <= 0. {
            return None;
        }
        let b = part.face_bounds(f);
        let lo = floor_at.min(floor_at + f64::from(sign) * radius);
        let hi = floor_at.max(floor_at + f64::from(sign) * radius);
        if !close(b.min[depth], lo) || !close(b.max[depth], hi) {
            return None;
        }
        fillets.push(FloorFillet {
            face: f,
            wall_face: wall,
            radius,
        });
    }
    // Every floor edge is paired with either a sharp mouth wall or one transition;
    // holes, islands, open ends and extra lower faces are deliberately refused.
    let boundary = &part.faces[floor].loops[0];
    if boundary.edges.len() != floor_neighbours.len() {
        return None;
    }
    for &f in floor_neighbours.iter() {
        if transitions.contains(&f) {
            continue;
        }
        if !seed.contains(&f) || !g.is_planar(f) || g.arc(floor, f) != Some(Arc::Concave) {
            return None;
        }
    }
    for &f in &region {
        if g.neighbours(f)
            .iter()
            .any(|n| *n != opening.node && !region.contains(n))
        {
            return None;
        }
        let b = part.face_bounds(f);
        if b.min[depth] < floor_at.min(mouth) - COORD_FLOOR
            || b.max[depth] > floor_at.max(mouth) + COORD_FLOOR
        {
            return None;
        }
    }
    // A wall has exactly one bottom role, fully spanning its run. This excludes
    // partial-height shelves and split/ambiguous treatments.
    for &wall in &seed {
        let treatments: Vec<_> = fillets.iter().filter(|f| f.wall_face == wall).collect();
        let bottom = match treatments.as_slice() {
            [] if floor_neighbours.contains(&wall) => floor_at,
            [f] => {
                let b = part.face_bounds(f.face);
                let w = part.face_bounds(wall);
                for a in 0..3 {
                    if a != depth {
                        // Cylinders run along one footprint axis; corner tori
                        // share both footprint bounds with their vertical corner.
                        if let Surface::Cylinder { frame, .. } = part.faces[f.face].surface {
                            if frame.z[a].abs() > 1. - 1e-9
                                && (!close(b.min[a], w.min[a]) || !close(b.max[a], w.max[a]))
                            {
                                return None;
                            }
                        } else if !close(b.min[a], w.min[a]) || !close(b.max[a], w.max[a]) {
                            return None;
                        }
                    }
                }
                floor_at + f64::from(sign) * f.radius
            }
            _ => return None,
        };
        let b = part.face_bounds(wall);
        if !close(if sign > 0 { b.min[depth] } else { b.max[depth] }, bottom) {
            return None;
        }
    }
    let width_walls: Vec<_> = planar
        .iter()
        .copied()
        .filter(|&f| graph::normal(part, f).is_some_and(|n| n[width].abs() > 1. - 1e-9))
        .collect();
    let lo = width_walls
        .iter()
        .map(|&f| g.span(f, long).0)
        .fold(f64::NEG_INFINITY, f64::max);
    let hi = width_walls
        .iter()
        .map(|&f| g.span(f, long).1)
        .fold(f64::INFINITY, f64::min);
    let d_lo = seed
        .iter()
        .map(|&f| g.span(f, depth).0)
        .fold(f64::NEG_INFINITY, f64::max);
    let d_hi = seed
        .iter()
        .map(|&f| g.span(f, depth).1)
        .fold(f64::INFINITY, f64::min);
    let mut record = Pocket {
        width_axis: AXES[width],
        long_axis: AXES[long],
        width: round_measurement(bounds.max[width] - bounds.min[width], 2),
        length: round_measurement(hi - lo, 2),
        depth: round_measurement((mouth - floor_at).abs(), 2),
        w_center: ((bounds.min[width] + bounds.max[width]) / 2.).into(),
        lo: lo.into(),
        hi: hi.into(),
        d_lo: d_lo.into(),
        d_hi: d_hi.into(),
        open_sign: sign,
        edge_anchored: false,
        rectangular_boundary: false,
        floor_radius: None,
        floor_fillets: Vec::new(),
        body_key: Some(Vec::new().into()),
        end_radius: None,
        corner_radius: None,
    };
    if seed.len() == 8 {
        // Depth-axis r5 cylinders are vertical wall corners, not floor rounds.
        record = with_proved_corner_radius(record, &scan.cylinders);
        record.corner_radius?;
    }
    if !close(record.lo(), bounds.min[long]) || !close(record.hi(), bounds.max[long]) {
        return None;
    }
    if unsupported {
        return Some(Reading::Unsupported(region));
    }
    record.d_lo = floor_at.min(mouth).into();
    record.d_hi = floor_at.max(mouth).into();
    if floor_neighbours.iter().all(|f| transitions.contains(f))
        && fillets.iter().all(|f| close(f.radius, fillets[0].radius))
    {
        record.floor_radius = Some(fillets[0].radius);
    }
    record.floor_radius = record.floor_radius.map(|r| round_measurement(r, 3));
    for f in &mut fillets {
        f.radius = round_measurement(f.radius, 3);
    }
    record.floor_fillets = fillets;
    Some(Reading::Supported(Box::new(Proposal {
        record,
        planar,
        caps: vec![transitions],
        floors: BTreeSet::from([floor]),
        constituent: region,
    })))
}

fn transition_radius(
    scan: &Scan<'_, '_>,
    face: usize,
    wall: usize,
    depth: usize,
    floor: f64,
    sign: i32,
) -> Option<f64> {
    let part = scan.ctx.part;
    let aligned = |v: [f64; 3], a: usize| v[a].abs() > 1. - 1e-9;
    match (
        part.faces[face].surface.clone(),
        part.faces[wall].surface.clone(),
    ) {
        (Surface::Cylinder { frame, radius }, Surface::Plane { .. }) => {
            let normal = graph::normal(part, wall)?;
            let axis = super::super::recess_faces::dominant_axis(normal)?;
            if axis == depth {
                return None;
            }
            let run = 3 - axis - depth;
            let at = center(&part.face_bounds(wall), axis);
            (axis != depth
                && radius > 0.
                && aligned(frame.z, run)
                && part.frame_points_outward(face) == Some(false)
                && close(frame.origin[depth], floor + f64::from(sign) * radius)
                && close(frame.origin[axis], at + normal[axis] * radius))
            .then_some(radius)
        }
        (
            Surface::Torus {
                frame,
                major,
                minor,
            },
            Surface::Cylinder { frame: w, radius },
        ) => (minor > 0.
            && major > 0.
            && aligned(frame.z, depth)
            && aligned(w.z, depth)
            && part.frame_points_outward(wall) == Some(false)
            && close(major + minor, radius)
            && close(frame.origin[depth], floor + f64::from(sign) * minor)
            && (0..3)
                .filter(|&a| a != depth)
                .all(|a| close(frame.origin[a], w.origin[a])))
        .then_some(minor),
        (Surface::Sphere { frame, radius: r }, Surface::Cylinder { frame: w, radius }) => (r > 0.
            && close(r, radius)
            && aligned(w.z, depth)
            && part.frame_points_outward(face) == Some(false)
            && part.frame_points_outward(wall) == Some(false)
            && close(frame.origin[depth], floor + f64::from(sign) * r)
            && (0..3)
                .filter(|&a| a != depth)
                .all(|a| close(frame.origin[a], w.origin[a])))
        .then_some(r),
        _ => None,
    }
}
