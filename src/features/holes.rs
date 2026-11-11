//! Drilled-hole recognition (`quiddity.holes`).
//!
//! Coaxial internal cylinders are grouped into stacks — drill, optional counterbore and
//! optional spotface become one hole, and a bore interrupted by a crossing hole is recombined.
//! Each stack's ends are classified from the faces beyond them, which decides the opening, the
//! bottom and the depth.
//! After reconciliation, drill points and uniquely closed planar flat floors are added to
//! accepted claims. The flat-floor rule is shared with blind pockets; shared planes remain context.
//!
//! Python's default inventory retries with `local_degradation` set when this family's strict
//! evidence refuses on a part whose solids are not all valid; three corpus parts take it
//! (`tests/local_degradation.rs`). `discover_locally_degraded` is that run's evidence path: a
//! hole proving no one valid solid is skipped, not refused. The aggregate's inventory takes the
//! retry where `strict_refusal_retries` says Python's would (`super::inventory_in`).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::Context;
use super::countersinks::{self, CounterSink, HoleMouth, countersink_matches_hole};
use super::cylinders::{
    STACK_GAP_FRAC, Segment, axis_point_at, full_cylinders, line_key, merge_runs, segments,
    z_then_cross,
};
use super::evidence::{self, EvidenceError, Occurrence};
use super::placed::{Grid, Placed};
use super::policy;
use super::stacks::{End, classify_end, end_partners};
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::{self, COORD_FLOOR, Surface, V3};
use crate::kernel::py;

/// Two quantised diameters are one diameter within this proportion.
const SAME_DIAMETER_FRAC: f64 = 1e-4;
/// Smallest diameter the proportional test divides by.
const DIAMETER_FLOOR: f64 = 1e-9;
/// A spotface must be shallower than this fraction of its diameter and at most
/// SPOTFACE_MAX_DEPTH_MM deep. Both tests use the published, rounded measurements.
const SPOTFACE_MAX_RATIO: f64 = 0.2;
/// Absolute millimetre limit, like ADR 0008's other minimum-evidence thresholds (#118):
/// a spotface only cleans up a surface; deeper recesses are functional counterbores.
const SPOTFACE_MAX_DEPTH_MM: f64 = 1.0;

/// A cylindrical enlargement at a hole mouth. Depth is the cylindrical span,
/// excluding any conical mouth treatment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CounterBore {
    pub diameter: f64,
    pub depth: f64,
}

/// A small conical edge break at an exterior end of the hole stack.
/// Depths on the parent hole and its cylindrical enlargements exclude this cone.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MouthChamfer {
    /// Direction from the wide rim into the cylindrical mouth.
    pub axis: V3,
    /// Centre of the wide rim on the exterior support plane.
    pub location: Placed<V3>,
    /// Diameter of the cylinder this cone meets (not the through drill diameter).
    pub diameter: f64,
    /// Radial setback from the virtual sharp cylindrical mouth.
    pub size: f64,
    /// Axial span, to add to the cylinder depth for a top-face depth callout.
    pub depth: f64,
    /// Angle to the exterior support plane, in degrees (not a full cone angle).
    pub angle: f64,
    /// Cone faces, claimed by this nested treatment; excluded from the parent
    /// hole's defining faces and fingerprint to preserve its cylindrical anchor.
    pub faces: Vec<usize>,
}

/// What closes a hole's deep end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bottom {
    Through,
    Flat,
    DrillPoint,
    /// The adjacent geometry matches none of the others.
    Unknown,
}

/// A partial planar cap at an otherwise through mouth.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExitObstruction {
    /// Axis point at the obstructed mouth, in file coordinates.
    pub location: Placed<V3>,
    /// Faces covering part of the exit disk (consulted, not defining evidence).
    pub faces: Vec<usize>,
    /// Approximate union area of those faces inside the exit disk, divided by disk area.
    pub covered_fraction: f64,
}

/// A drilled hole: `axis` points from the opening into the hole, `location` is the axis point at
/// the opening; `diameter`/`depth` describe the bore (the narrowest segment), with `depth` from
/// the top of the bore to the hole's deep end. `bottom` is `"through"`, `"flat"`,
/// `"drill_point"` or `"unknown"`. For interchangeable mouths, axis/location are a
/// presentation end gauge. Identical counterbore, spotface and outer-step readings at both
/// mouths retain both sets of defining faces; differing readings are omitted. Identical countersinks
/// at opposite mouths retain both seats as context, presenting the seat at the gauge end.
/// With no proved attachment,
/// the gauge is on the bore itself. No material drilling-entry end is asserted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HoleRecord {
    /// Optional additive reference to the identical/superseding published feature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    pub axis: V3,
    pub location: Placed<V3>,
    pub diameter: f64,
    pub depth: f64,
    pub bottom: Bottom,
    pub cbore: Option<CounterBore>,
    pub spotface: Option<CounterBore>,
    /// Counterbore-class enlargements outside `cbore`, outermost first. Each depth
    /// is that cylinder's own mouth-to-floor span, not cumulative from the hole mouth.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outer_steps: Vec<CounterBore>,
    pub csink: Option<CounterSink>,
    /// Exterior edge breaks, independently located so both ends of a through hole can
    /// be described without turning its presentation gauge into a drilling-entry claim.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mouth_chamfers: Vec<MouthChamfer>,
    /// Partial caps on otherwise through mouths. Absent for unobstructed or blind holes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exit_obstructions: Vec<ExitObstruction>,
}

/// Options named as `recognise_holes`' keyword arguments.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HoleOptions {
    /// Compose recognised countersinks onto the holes they flare: Python's
    /// `csinks=recognise_countersinks(part)`, spelled `"csinks": "auto"`. Off by default, as
    /// there.
    #[serde(rename = "csinks", deserialize_with = "auto_flag")]
    pub with_countersinks: bool,
}

/// `null` → off, `"auto"` → on.
fn auto_flag<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    match Option::<String>::deserialize(d)?.as_deref() {
        None => Ok(false),
        Some("auto") => Ok(true),
        Some(other) => Err(serde::de::Error::custom(format!(
            "csinks must be \"auto\" or null, not {other:?}"
        ))),
    }
}

internal_entry! {
/// The evidence path: holes with their defining cylinder faces and the faces that close them,
/// published only if each lies in one valid solid and no countersink seats two holes.
pub fn discover_verified(
    ctx: &Context<'_>,
    csinks: &[Occurrence<CounterSink>],
) -> Result<Vec<Occurrence<HoleRecord>>, EvidenceError> {
    let found = discover(ctx, csinks);
    seats_once(csinks, &found)?;
    evidence::verified(ctx.part, found)
}
}

internal_entry! {
/// The evidence path as Python's locally degraded run takes it (`_discover_holes` on a graph
/// with `local_degradation` set): a hole whose cylinder and closing faces prove no one valid
/// solid is skipped instead of refusing every hole, and the countersink check is made over the
/// holes kept. Python's degraded run also admits a solid with at most three faulted faces,
/// skipping only the holes that touch them ([`evidence::locally_valid_solid`]). The seat face of a
/// composed countersink is proved with the hole, so a hole whose seat lies on another solid is
/// skipped, where Python refuses it (no captured call or corpus part has one).
pub fn discover_locally_degraded(
    ctx: &Context<'_>,
    csinks: &[Occurrence<CounterSink>],
) -> Result<Vec<Occurrence<HoleRecord>>, EvidenceError> {
    let mut found = discover(ctx, csinks);
    found.retain(|h| evidence::locally_proved(ctx.part, h));
    seats_once(csinks, &found)?;
    Ok(found)
}
}

/// Whether the aggregate retries locally degraded on these holes (`_take_inventory`): Python's
/// strict hole evidence refuses for want of one valid solid (countersinks seating one hole each,
/// [`discover_verified`]'s order), and the part's solids are not all valid
/// (`BRepCheck_Analyzer(solid).IsValid()`, so faulted faces count; a part with no solid
/// retries too).
pub(crate) fn strict_refusal_retries(
    part: &Part,
    csinks: &[Occurrence<CounterSink>],
    found: &[Occurrence<HoleRecord>],
) -> bool {
    let unproved = |h: &Occurrence<HoleRecord>| {
        let faces: Vec<usize> = h.defining.iter().chain(&h.context).copied().collect();
        h.defining.is_empty() || evidence::common_valid_solid(part, &faces).is_none()
    };
    let all_valid = !part.solids.is_empty()
        && (0..part.solids.len()).all(|s| part.solid_is_geometrically_valid(s));
    seats_once(csinks, found).is_ok() && found.iter().any(unproved) && !all_valid
}

/// No countersink may seat two holes.
fn seats_once(
    csinks: &[Occurrence<CounterSink>],
    found: &[Occurrence<HoleRecord>],
) -> Result<(), EvidenceError> {
    for cs in csinks {
        let seats = found
            .iter()
            .filter(|h| cs.defining.iter().all(|f| h.context.contains(f)))
            .count();
        if seats > 1 {
            return Err(EvidenceError::SharedEvidence);
        }
    }
    Ok(())
}

internal_entry! {
/// `recognise_holes`.
pub fn recognise_holes(part: &Part, opts: &HoleOptions) -> Vec<HoleRecord> {
    let ctx = Context::new(part);
    let csinks = if opts.with_countersinks {
        countersinks::discover(&ctx)
    } else {
        Vec::new()
    };
    discover(&ctx, &csinks)
        .into_iter()
        .map(|o| o.record)
        .collect()
}
}

fn same_diameter(a: f64, b: f64) -> bool {
    (a - b).abs() <= SAME_DIAMETER_FRAC * a.abs().max(b.abs()).max(DIAMETER_FLOOR)
}

/// A cone or torus spans the gap between *a*'s high end and *b*'s low end — the shoulder chamfer
/// or fillet that makes them steps of one hole, reached directly or one adjacency hop away
/// (`_shared_transition`).
fn shared_transition(part: &Part, a: &Segment, b: &Segment) -> bool {
    let a_partners = end_partners(part, a, a.s_hi);
    let b_partners = end_partners(part, b, b.s_lo);
    for (own, other) in [(&a_partners, &b_partners), (&b_partners, &a_partners)] {
        for &t in own {
            if !matches!(
                part.faces[t].surface,
                Surface::Cone { .. } | Surface::Torus { .. }
            ) {
                continue;
            }
            if other.contains(&t) || part.neighbours(t).iter().any(|n| other.contains(n)) {
                return true;
            }
        }
    }
    false
}

/// Recombine coaxial stacks that are one hole (`_merge_stacks`): equal bores either side of an
/// internal cylinder whose finite cylinder contains the gap (a crossing bore), or different
/// diameters bridged by a shoulder chamfer or fillet.
fn merge_stacks(part: &Part, stacks: Vec<Vec<Segment>>) -> Vec<Vec<Segment>> {
    let original: Vec<Segment> = stacks.iter().flatten().cloned().collect();
    let owner_of = |solid: usize, face: usize| {
        original
            .iter()
            .position(|s| s.solid == solid && s.faces.contains(&face))
    };
    let sources = |seg: &Segment| -> BTreeSet<usize> {
        let mut found = BTreeSet::new();
        for &face in &seg.faces {
            let own = owner_of(seg.solid, face);
            for other in part.neighbours(face) {
                if let Some(index) = owner_of(seg.solid, other)
                    && Some(index) != own
                {
                    found.insert(index);
                }
            }
        }
        found
    };
    let shares_interruption = |a: &Segment, b: &Segment| -> bool {
        let endpoints = [axis_point_at(a, a.s_hi), axis_point_at(b, b.s_lo)];
        let (sa, sb) = (sources(a), sources(b));
        sa.intersection(&sb).any(|&index| {
            let cut = &original[index];
            let tolerance = policy::length_tol(cut.diameter, STACK_GAP_FRAC);
            let radius = cut.diameter / 2.0;
            let radial_tolerance = policy::length_tol(radius, 5e-6);
            endpoints.iter().all(|&point| {
                let axial = py::dot(&point, &cut.direction);
                let centre = axis_point_at(cut, axial);
                cut.s_lo - tolerance <= axial
                    && axial <= cut.s_hi + tolerance
                    && py::dist(&point, &centre) < radius + radial_tolerance
            })
        })
    };
    let mut by_line: Vec<(Vec<f64>, Vec<Vec<Segment>>)> = Vec::new();
    for stack in stacks {
        let key = line_key(&stack[0]);
        match by_line.iter_mut().find(|(k, _)| *k == key) {
            Some((_, group)) => group.push(stack),
            None => by_line.push((key, vec![stack])),
        }
    }
    let mut merged = Vec::new();
    for (_, mut line) in by_line {
        let low = |st: &Vec<Segment>| st.iter().map(|s| s.s_lo).fold(f64::INFINITY, f64::min);
        line.sort_by(|a, b| py::order(low(a), low(b)));
        let mut stacks = line.into_iter();
        let mut cur = stacks.next().expect("a non-empty group");
        for nxt in stacks {
            let ai = end_segment(&cur, |s| s.s_hi);
            let bi = end_segment(&nxt, |s| -s.s_lo);
            let (a, b) = (&cur[ai], &nxt[bi]);
            let closed = |end: End| matches!(end, End::Flat | End::DrillPoint);
            if same_diameter(a.diameter, b.diameter)
                && !closed(classify_end(part, a, a.s_hi, true).0)
                && !closed(classify_end(part, b, b.s_lo, false).0)
                && shares_interruption(a, b)
            {
                let mut joined = a.clone();
                joined.s_hi = b.s_hi;
                joined.faces.extend(&b.faces);
                let mut next: Vec<Segment> = cur
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != ai)
                    .map(|(_, s)| s.clone())
                    .collect();
                next.push(joined);
                next.extend(
                    nxt.iter()
                        .enumerate()
                        .filter(|(i, _)| *i != bi)
                        .map(|(_, s)| s.clone()),
                );
                cur = next;
            } else if b.s_lo - a.s_hi
                <= policy::length_tol(a.diameter.max(b.diameter), STACK_GAP_FRAC)
                    + (a.diameter - b.diameter).abs()
                && shared_transition(part, a, b)
            {
                cur.extend(nxt);
            } else {
                merged.push(std::mem::replace(&mut cur, nxt));
            }
        }
        merged.push(cur);
    }
    merged
}

/// Which end is the opening. Mouth diameter, then the complete inward axial profile decide
/// intrinsically. An unresolved side is an end gauge: retain the bore and any identical
/// mouth-step readings, without asserting a material drilling-entry end.
struct Drilling {
    from_hi: bool,
    opening: usize,
    opening_s: f64,
    bottom: Bottom,
    terminal: Vec<usize>,
    presentation: bool,
    exit_obstructions: Vec<ExitObstruction>,
}

/// Exact transverse planar supports at a cylinder end. Requiring both wall planes
/// excludes crossing bores, sloping floors, lips, and unsupported end guesses.
fn end_planes(part: &Part, seg: &Segment, end: f64, hi: bool) -> Option<(Vec<usize>, Vec<usize>)> {
    let centre = axis_point_at(seg, end);
    let tol = policy::length_tol(seg.diameter, 1e-7);
    let mut exterior = Vec::new();
    let mut caps = Vec::new();
    for face in end_partners(part, seg, end) {
        let Surface::Plane { frame } = part.faces[face].surface else {
            return None;
        };
        let normal = part.face_normal(face, 0., 0.)?;
        let alignment = geom::dot(normal, seg.direction) * if hi { 1. } else { -1. };
        if alignment.abs() < 1. - 1e-7
            || geom::dot(geom::sub(centre, frame.origin), normal).abs() > tol
        {
            return None;
        }
        part.domain(face)?;
        if alignment > 0. {
            exterior.push(face);
        } else {
            caps.push(face);
        }
    }
    if exterior.is_empty() {
        return None;
    }
    Some((exterior, caps))
}

/// Resolve a wall exit with a genuine exterior rim and a partial planar cap.
/// Equal-area polar quadrature estimates the union (never the sum) of cap areas.
/// Open and covered witnesses are checked against the actual solid immediately
/// beyond the mouth; unsupported/ambiguous geometry retains Unknown.
fn partial_exit(part: &Part, seg: &Segment, end: f64, hi: bool) -> Option<ExitObstruction> {
    use crate::kernel::classify::{Classifier, State};
    let (exterior, caps) = end_planes(part, seg, end, hi)?;
    if caps.is_empty() {
        return None;
    }
    let Surface::Cylinder { frame, radius } = part.faces[seg.faces[0]].surface else {
        return None;
    };
    let centre = axis_point_at(seg, end);
    let contains = |face: usize, point| {
        let (u, v) = part.faces[face].surface.parameters(point, None)?;
        Some(part.domain(face)?.contains(u, v))
    };
    const RINGS: usize = 32;
    const SECTORS: usize = 128;
    let mut covered = 0;
    let mut faces = BTreeSet::new();
    let mut open_witness = None;
    let mut cap_witness = None;
    for ring in 0..RINGS {
        let r = radius * ((ring as f64 + 0.5) / RINGS as f64).sqrt();
        for sector in 0..SECTORS {
            let angle = std::f64::consts::TAU * (sector as f64 + 0.5) / SECTORS as f64;
            let (s, c) = angle.sin_cos();
            let point = geom::add(
                centre,
                geom::add(geom::scale(frame.x, r * c), geom::scale(frame.y, r * s)),
            );
            // Opposing coincident faces inside the disk are contradictory evidence.
            for &face in &exterior {
                if contains(face, point)? {
                    return None;
                }
            }
            let mut hit = false;
            for &face in &caps {
                if contains(face, point)? {
                    faces.insert(face);
                    hit = true;
                }
            }
            if hit {
                covered += 1;
                cap_witness.get_or_insert(point);
            } else {
                open_witness.get_or_insert(point);
            }
        }
    }
    let offset = geom::scale(seg.direction, if hi { 1e-4 } else { -1e-4 });
    part.solids.get(seg.solid)?;
    let classifier = Classifier::for_solid(part, seg.solid);
    if classifier.classify(geom::add(open_witness?, offset)) != State::Out
        || classifier.classify(geom::add(cap_witness?, offset)) != State::In
    {
        return None;
    }
    Some(ExitObstruction {
        location: Placed::new(centre, Grid::Figures(11)),
        faces: faces.into_iter().collect(),
        covered_fraction: policy::round_measurement(covered as f64 / (RINGS * SECTORS) as f64, 4),
    })
}

fn drilled_from(part: &Part, stack: &[Segment]) -> Drilling {
    let lo = end_segment(stack, |s| -s.s_lo);
    let hi = end_segment(stack, |s| s.s_hi);
    let (mut lo_state, lo_faces) = classify_end(part, &stack[lo], stack[lo].s_lo, false);
    let (mut hi_state, hi_faces) = classify_end(part, &stack[hi], stack[hi].s_hi, true);
    let mut exit_obstructions = Vec::new();
    // Coplanar exterior rim supports prove the local wall's two planes.
    // Resolve only otherwise undecided mixed exits, and publish only when both open.
    let lo_obstruction = if lo_state == End::Unknown
        && matches!(hi_state, End::Open | End::Unknown)
        && end_planes(part, &stack[hi], stack[hi].s_hi, true).is_some()
    {
        partial_exit(part, &stack[lo], stack[lo].s_lo, false)
    } else {
        None
    };
    let hi_obstruction = if hi_state == End::Unknown
        && matches!(lo_state, End::Open | End::Unknown)
        && end_planes(part, &stack[lo], stack[lo].s_lo, false).is_some()
    {
        partial_exit(part, &stack[hi], stack[hi].s_hi, true)
    } else {
        None
    };
    let both_open = (lo_state == End::Open || lo_obstruction.is_some())
        && (hi_state == End::Open || hi_obstruction.is_some());
    let lo_obstructed = lo_obstruction.is_some();
    let lo_obstruction = lo_obstruction.filter(|_| both_open);
    let hi_obstruction = hi_obstruction.filter(|_| both_open);
    if let Some(obstruction) = lo_obstruction {
        lo_state = End::Open;
        exit_obstructions.push(obstruction);
    }
    if let Some(obstruction) = hi_obstruction {
        hi_state = End::Open;
        exit_obstructions.push(obstruction);
    }
    let obstructed = !exit_obstructions.is_empty();
    let (from_hi, presentation) = match (lo_state == End::Open, hi_state == End::Open) {
        (true, false) => (false, false),
        (false, true) => (true, false),
        _ if obstructed && exit_obstructions.len() == 1 => (lo_obstructed, false),
        _ if lo == hi => (true, true),
        _ => match mouth_order(stack, lo, hi) {
            Some(side) => (side, false),
            None => (true, true),
        },
    };
    let gauge_faces = if presentation {
        let mut faces: Vec<_> = lo_faces.iter().chain(&hi_faces).copied().collect();
        faces.sort_unstable();
        faces.dedup();
        faces
    } else {
        vec![]
    };
    let (opening, opening_s, bottom, faces) = if from_hi {
        (hi, stack[hi].s_hi, lo_state, lo_faces)
    } else {
        (lo, stack[lo].s_lo, hi_state, hi_faces)
    };
    let bottom = match bottom {
        _ if presentation && lo_state != hi_state => Bottom::Unknown,
        End::Open => Bottom::Through,
        End::Flat => Bottom::Flat,
        End::DrillPoint => Bottom::DrillPoint,
        End::Unknown => Bottom::Unknown,
    };
    // Only a closing face is evidence of the bottom.
    let mut terminal = if matches!(bottom, Bottom::Flat | Bottom::DrillPoint) {
        if presentation { gauge_faces } else { faces }
    } else {
        vec![]
    };
    terminal.extend(exit_obstructions.iter().flat_map(|o| &o.faces));
    if obstructed {
        for (seg, end, hi) in [
            (&stack[lo], stack[lo].s_lo, false),
            (&stack[hi], stack[hi].s_hi, true),
        ] {
            if let Some((exterior, _)) = end_planes(part, seg, end, hi) {
                terminal.extend(exterior);
            }
        }
    }
    terminal.sort_unstable();
    terminal.dedup();
    Drilling {
        from_hi,
        opening,
        opening_s,
        bottom,
        terminal,
        presentation,
        exit_obstructions,
    }
}

/// Compare the two inward profiles using only distances and diameters. Source support signs,
/// face indices and the input order carry no machining-side evidence. Equal profiles leave
/// one-sided attribution unresolved, while retaining the bore and identical mouth steps.
fn mouth_order(stack: &[Segment], lo: usize, hi: usize) -> Option<bool> {
    let profile = |from_hi: bool| {
        let mut values: Vec<[f64; 3]> = stack
            .iter()
            .map(|s| {
                let (near, far) = if from_hi {
                    (stack[hi].s_hi - s.s_hi, stack[hi].s_hi - s.s_lo)
                } else {
                    (s.s_lo - stack[lo].s_lo, s.s_hi - stack[lo].s_lo)
                };
                [near, -s.diameter, -far]
            })
            .collect();
        values.sort_by(|a, b| py::tuple_order(a, b));
        values
    };
    let (low, high) = (profile(false), profile(true));
    for (a, b) in low.iter().zip(&high) {
        for (a, b) in a.iter().zip(b) {
            if (a - b).abs() > COORD_FLOOR {
                return Some(b < a);
            }
        }
    }
    None
}

/// The segment that forms one end of a stack: the one reaching furthest by *reach*, and of
/// those reaching it (within COORD_FLOOR) the widest, which is the mouth. Python takes the first
/// in stack order, which is face order, so turning the part over can pick a different segment.
fn end_segment(stack: &[Segment], reach: impl Fn(&Segment) -> f64) -> usize {
    let far = stack.iter().map(&reach).fold(f64::NEG_INFINITY, f64::max);
    let mut best: Option<usize> = None;
    for (i, s) in stack.iter().enumerate() {
        if far - reach(s) <= COORD_FLOOR && best.is_none_or(|b| s.diameter > stack[b].diameter) {
            best = Some(i);
        }
    }
    best.expect("a stack has segments")
}

/// The counterbore and spotface between the opening and the bore (`_near_side_steps`). Steps
/// must narrow inward (a wider one is a groove); lands of one diameter are unioned.
#[derive(Default)]
struct NearSideSteps {
    cbore: Option<CounterBore>,
    spotface: Option<CounterBore>,
    outer_steps: Vec<CounterBore>,
    faces: Vec<usize>,
}

fn near_side_steps(steps: &[Segment]) -> NearSideSteps {
    let mut spans: Vec<(f64, f64, f64, Vec<usize>)> = Vec::new(); // key, lo, hi, faces
    let mut min_d = f64::INFINITY;
    for step in steps {
        if step.diameter > min_d && !same_diameter(step.diameter, min_d) {
            continue;
        }
        min_d = step.diameter;
        let key = py::quantise(step.diameter, 4);
        match spans.iter_mut().find(|s| s.0 == key) {
            Some(s) => {
                s.1 = s.1.min(step.s_lo);
                s.2 = s.2.max(step.s_hi);
                s.3.extend(&step.faces);
            }
            None => spans.push((key, step.s_lo, step.s_hi, step.faces.clone())),
        }
    }
    let mut found = NearSideSteps::default();
    for (key, lo, hi, step_faces) in spans {
        let spec = CounterBore {
            diameter: key,
            depth: policy::round_measurement(hi - lo, 2),
        };
        if spec.depth <= SPOTFACE_MAX_DEPTH_MM && spec.depth < SPOTFACE_MAX_RATIO * spec.diameter {
            if found.spotface.is_none() {
                found.spotface = Some(spec);
                found.faces.extend(step_faces);
            }
        } else {
            found.outer_steps.push(spec);
            found.faces.extend(step_faces);
        }
    }
    // Spans are ordered from the opening towards the bore. Retain every counterbore
    // wall, reserving the singular `cbore` field for the innermost enlargement (#118).
    found.cbore = found.outer_steps.pop();
    found
}

/// Read a stack inward from either end, putting the widest level segment first.
fn ordered_from(stack: &[Segment], from_hi: bool) -> Vec<Segment> {
    let mut ordered = stack.to_vec();
    // Nearest the opening first: by the near end of each segment, which is s_hi drilling
    // down from the high end and s_lo drilling up from the low end. (Python sorts by s_hi
    // both ways, so overlapping segments order differently once the part is turned over.)
    // Segments starting level with each other put the wider first, as the mouth is outermost
    // (Python keeps them in face order). The sort is stable.
    ordered.sort_by(|a, b| {
        let near = if from_hi {
            py::order(a.s_hi, b.s_hi).reverse()
        } else {
            py::order(a.s_lo, b.s_lo)
        };
        let level = if from_hi {
            a.s_hi - b.s_hi
        } else {
            a.s_lo - b.s_lo
        };
        if level.abs() <= COORD_FLOOR {
            py::order(a.diameter, b.diameter).reverse()
        } else {
            near
        }
    });
    ordered
}

/// Depth from the top of the bore to the deep end (`_bore_depth`): the deep end is the whole
/// stack for a blind hole (a bottom relief groove counts) but only the bore for a through hole.
fn bore_depth(
    stack: &[Segment],
    bore: &Segment,
    through: bool,
    from_hi: bool,
) -> (f64, Vec<usize>) {
    let bore_segs: Vec<&Segment> = stack
        .iter()
        .filter(|s| same_diameter(s.diameter, bore.diameter))
        .collect();
    let deep: Vec<&Segment> = if through {
        bore_segs.clone()
    } else {
        stack.iter().collect()
    };
    let mut defining: Vec<&Segment> = bore_segs.clone();
    let depth = if from_hi {
        let deep_end = deep.iter().map(|s| s.s_lo).fold(f64::INFINITY, f64::min);
        if !through {
            defining.extend(deep.iter().filter(|s| s.s_lo == deep_end));
        }
        bore_segs
            .iter()
            .map(|s| s.s_hi)
            .fold(f64::NEG_INFINITY, f64::max)
            - deep_end
    } else {
        let deep_end = deep
            .iter()
            .map(|s| s.s_hi)
            .fold(f64::NEG_INFINITY, f64::max);
        if !through {
            defining.extend(deep.iter().filter(|s| s.s_hi == deep_end));
        }
        deep_end
            - bore_segs
                .iter()
                .map(|s| s.s_lo)
                .fold(f64::INFINITY, f64::min)
    };
    (
        depth,
        defining
            .iter()
            .flat_map(|s| s.faces.iter().copied())
            .collect(),
    )
}

/// Equal seats at the two ends of a through bore are an interchangeable pair, not
/// competing attributions at one mouth. Keep both as evidence and use the gauge end
/// only to present the one-seat field. Distinct seats at the same end still refuse.
fn symmetric_seats(matching: &[&Occurrence<CounterSink>], mouth: &HoleMouth) -> bool {
    if !mouth.through || matching.len() != 2 {
        return false;
    }
    let a = &matching[0].record;
    let b = &matching[1].record;
    let spec = |cs: &CounterSink| {
        [
            cs.major_diameter,
            cs.drill_diameter,
            cs.included_angle,
            cs.depth,
        ]
    };
    if spec(a)
        .iter()
        .zip(spec(b))
        .any(|(a, b)| (a - b).abs() > COORD_FLOOR)
    {
        return false;
    }
    let tol = policy::length_tol(mouth.diameter, 1e-4);
    let at_end = |cs: &CounterSink, far: bool| {
        let direction = if far { -1. } else { 1. };
        let end = geom::add(
            mouth.location,
            geom::scale(mouth.axis, if far { mouth.depth } else { 0. }),
        );
        let minor = geom::add(*cs.location, geom::scale(cs.axis, cs.depth));
        geom::unit(cs.axis).is_some_and(|axis| geom::dot(axis, mouth.axis) * direction > 1. - 1e-6)
            && geom::norm(geom::sub(minor, end)) <= tol
    };
    (at_end(a, false) && at_end(b, true)) || (at_end(b, false) && at_end(a, true))
}

/// Prove an edge break on an exterior end of the complete stack, never on an
/// intermediate shoulder. A full coaxial cone joins the cylinder's end circle to
/// an outward-facing transverse plane, with convex joins. The flare stays below
/// the countersink family's 1.5 radius ratio; both legs must be small relative to
/// the mouth radius. All tests use unrounded geometry and proportional tolerances.
fn mouth_chamfer(part: &Part, seg: &Segment, hi: bool) -> Option<(MouthChamfer, usize)> {
    let end = if hi { seg.s_hi } else { seg.s_lo };
    let centre = axis_point_at(seg, end);
    let outward = geom::scale(seg.direction, if hi { 1. } else { -1. });
    let radius = 0.5 * seg.diameter;
    let tol = policy::length_tol(seg.diameter, 1e-6);
    let mut found = Vec::new();
    for face in end_partners(part, seg, end) {
        let Surface::Cone { frame, .. } = part.faces[face].surface else {
            continue;
        };
        let Some(((minor, narrow), (major, wide), _)) = super::turned::cone_rims(part, face) else {
            continue;
        };
        let Some((u0, u1, _, _)) = part.uv_bounds(face) else {
            continue;
        };
        let delta = geom::sub(wide, narrow);
        let depth = geom::dot(delta, outward);
        let size = major - minor;
        // A symmetric band is identical whether measured from the axis or the
        // support plane. Exclude shallow tapers and nearly transverse lead-ins.
        let angle = depth.atan2(size).to_degrees();
        if (minor - radius).abs() > tol
            || geom::norm(geom::sub(narrow, centre)) > tol
            || geom::dot(frame.z, outward).abs() < 1. - 1e-6
            || geom::norm(geom::cross(geom::sub(frame.origin, centre), outward)) > tol
            || geom::norm(geom::cross(delta, outward)) > tol
            || u1 - u0 < std::f64::consts::TAU - 1e-6
            || size <= tol
            || depth <= tol
            || major >= countersinks::MIN_MAJOR_RATIO * minor
            || depth >= 0.5 * minor
            || !(15.0..=75.0).contains(&angle)
        {
            continue;
        }
        // Shared circle and convexity prove attachment, not just analytic coincidence.
        if !seg
            .faces
            .iter()
            .any(|&wall| part.arc(face, wall) == Some(Arc::Convex))
        {
            continue;
        }
        let exterior = part.neighbours(face).into_iter().find(|&support| {
            let Surface::Plane { frame } = part.faces[support].surface else {
                return false;
            };
            part.face_normal(support, 0., 0.).is_some_and(|normal| {
                geom::dot(normal, outward) > 1. - 1e-6
                    && geom::dot(geom::sub(wide, frame.origin), normal).abs() <= tol
            }) && part.arc(face, support) == Some(Arc::Convex)
                && part.shared_edges(face, support).iter().any(|&edge| {
                    let crate::kernel::geom::Curve::Circle { frame, radius } =
                        part.edges[edge].curve
                    else {
                        return false;
                    };
                    (radius - major).abs() <= tol
                        && geom::norm(geom::sub(frame.origin, wide)) <= tol
                })
        });
        if let Some(support) = exterior {
            found.push((
                MouthChamfer {
                    axis: geom::scale(outward, -1.).map(py::without_negative_zero),
                    location: Placed::new(wide, Grid::Figures(11)),
                    diameter: seg.diameter,
                    size: policy::round_measurement(size, 4),
                    depth: policy::round_measurement(depth, 4),
                    angle: py::round_to(angle, 2),
                    faces: vec![face],
                },
                support,
            ));
        }
    }
    if found.len() == 1 { found.pop() } else { None }
}

/// Coaxial internal cylinder stacks, shared by holes and their mouth treatments.
pub(super) fn hole_stacks(ctx: &Context<'_>) -> Vec<Vec<Segment>> {
    let part = ctx.part;
    let cyls = ctx.cylinders();
    let (z, cross) = z_then_cross(cyls);
    let mut internal: Vec<_> = full_cylinders(&z)
        .into_iter()
        .chain(full_cylinders(&cross))
        .filter(|c| !c.external)
        .collect();
    if internal.is_empty() {
        return Vec::new();
    }
    let enclosed: BTreeSet<_> = ctx
        .sealed_void_shells()
        .iter()
        .flat_map(|shell| shell.faces.iter().copied())
        .collect();
    internal.retain(|c| !enclosed.contains(&c.face));
    merge_stacks(part, merge_runs(&segments(&internal), line_key))
}

internal_entry! {
/// Holes in stack order, each with its defining cylinder faces; *csinks* are composed onto the
/// holes they flare.
pub fn discover(
    ctx: &Context<'_>,
    csinks: &[Occurrence<CounterSink>],
) -> Vec<Occurrence<HoleRecord>> {
    let part = ctx.part;
    let stacks = hole_stacks(ctx);
    let has_cones = part.faces.iter().any(|f| matches!(f.surface, Surface::Cone { .. }));
    let mut out = Vec::new();
    for stack in stacks {
        let d = stack[0].direction;
        let Drilling {
            from_hi,
            opening,
            opening_s,
            bottom,
            mut terminal,
            presentation,
            exit_obstructions,
        } = drilled_from(part, &stack);
        let ordered = ordered_from(&stack, from_hi);
        let bore_i = py::first_min(&ordered, |s| s.diameter);
        let bore = &ordered[bore_i];
        let NearSideSteps {
            mut cbore,
            mut spotface,
            mut outer_steps,
            faces: mut step_faces,
        } = near_side_steps(&ordered[..bore_i]);
        if presentation {
            let opposite = ordered_from(&stack, !from_hi);
            let other_bore_i = py::first_min(&opposite, |s| s.diameter);
            let other = near_side_steps(&opposite[..other_bore_i]);
            // An end gauge cannot choose between different published readings. Identical
            // readings retain their dimensions and claim both mouths, so reversal changes
            // only the representative end, never an attachment or its supporting evidence.
            if cbore == other.cbore && spotface == other.spotface && outer_steps == other.outer_steps {
                step_faces.extend(other.faces);
                step_faces.sort_unstable();
            } else {
                cbore = None;
                spotface = None;
                outer_steps.clear();
                step_faces.clear();
            }
        }
        let (depth, mut defining) = bore_depth(
            &stack,
            bore,
            bottom == Bottom::Through || presentation,
            from_hi,
        );
        defining.extend(step_faces);
        // A face counts once however many selections name it (Python resolves them to a set).
        let mut seen = std::collections::BTreeSet::new();
        defining.retain(|f| seen.insert(*f));
        let axis = if from_hi { geom::scale(d, -1.0) } else { d }.map(py::without_negative_zero);
        let location = if presentation && cbore.is_none() && spotface.is_none() {
            axis_point_at(bore, if from_hi { bore.s_hi } else { bore.s_lo })
        } else {
            axis_point_at(&stack[opening], opening_s)
        }
        ;
        let location = Placed::new(location, Grid::Figures(11));
        let mut record = HoleRecord {
            superseded_by: None,
            axis,
            location,
            diameter: bore.diameter,
            depth: policy::round_measurement(depth, 2),
            bottom,
            cbore,
            spotface,
            outer_steps,
            csink: None,
            mouth_chamfers: Vec::new(),
            exit_obstructions,
        };
        let mouth = HoleMouth {
            axis: record.axis,
            location: *record.location,
            diameter: record.diameter,
            depth: record.depth,
            through: record.bottom == Bottom::Through,
        };
        let mut matching: Vec<_> = csinks
            .iter()
            .filter(|cs| countersink_matches_hole(&cs.record, &mouth))
            .collect();
        let rank = |cs: &Occurrence<CounterSink>| {
            [
                cs.record.major_diameter,
                cs.record.depth,
                cs.record.included_angle,
            ]
        };
        matching.sort_by(|a, b| py::tuple_order(&rank(b), &rank(a)));
        let ambiguous_seat = matching.len() > 1
            && rank(matching[0])
                .iter()
                .zip(rank(matching[1]))
                .all(|(a, b)| (a - b).abs() <= COORD_FLOOR);
        // Prove the pair on the measured bore, before its depth is rounded for publication.
        let symmetric = presentation && symmetric_seats(&matching, &HoleMouth {
            location: *record.location.raw(),
            depth,
            ..mouth
        });
        if symmetric {
            // The representative end can change under motion, just as for a plain bore.
            // Both seats remain consulted evidence whichever representative is presented.
            for cs in &matching {
                terminal.extend(&cs.defining);
            }
            matching.sort_by(|a, b| {
                py::order(geom::dot(b.record.axis, axis), geom::dot(a.record.axis, axis))
            });
        }
        // Ambiguous attribution cannot remove the independently established bore.
        if let Some(cs) = matching.first().filter(|_| !ambiguous_seat || symmetric) {
            record.csink = Some(cs.record.clone());
            // The seat's face is consulted evidence: it must share the hole's solid.
            if !symmetric {
                terminal.extend(&cs.defining);
            }
        }
        for hi in [false, true].into_iter().filter(|_| has_cones) {
            let i = end_segment(&stack, |s| if hi { s.s_hi } else { -s.s_lo });
            if let Some((treatment, support)) = mouth_chamfer(part, &stack[i], hi) {
                terminal.extend(&treatment.faces);
                terminal.push(support);
                terminal.extend(stack[i].faces.iter().filter(|f| !defining.contains(f)));
                record.mouth_chamfers.push(treatment);
            }
        }
        out.push(Occurrence {
            record,
            defining,
            context: terminal,
        });
    }
    out
}
}

/// Claim an already-consulted flat floor after reconciliation, for holes and pockets alike.
/// `direction` points into the recess. Every boundary edge must have exactly one other
/// incident face, an own wall at the deepest end. A larger plane, an interrupted floor,
/// a non-manifold edge or another accepted owner's claim cannot become this floor.
/// The first individual owner wins, even when a later record has identical walls.
pub(crate) fn claim_flat_floor(
    part: &Part,
    direction: V3,
    faces: &mut Vec<usize>,
    constituent: &[usize],
    occupied: &BTreeSet<usize>,
    floor_owners: &mut BTreeSet<usize>,
) {
    let spans: Vec<_> = faces
        .iter()
        .map(|&face| {
            let span = part
                .face_edges(face)
                .into_iter()
                .flat_map(|e| {
                    [
                        part.edges[e].start,
                        part.edges[e].midpoint(),
                        part.edges[e].end,
                    ]
                })
                .map(|p| geom::dot(p, direction))
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), s| {
                    (lo.min(s), hi.max(s))
                });
            (face, span)
        })
        .collect();
    let (start, end) = spans.iter().fold(
        (f64::INFINITY, f64::NEG_INFINITY),
        |(lo, hi), (_, (a, b))| (lo.min(*a), hi.max(*b)),
    );
    // Relative to the recess size, not its placement in file coordinates.
    let tol = 1e-7 * (end - start).max(1.);
    // Some corner-notch seeds already define a floor. A coplanar floor piece is
    // never a wall closing another floor piece, even if the seed claims both.
    let mut walls: Vec<_> = spans
        .into_iter()
        .filter_map(|(face, (lo, hi))| (hi - lo > tol).then_some(face))
        .collect();
    walls.sort_unstable();
    walls.dedup();
    let floors: Vec<_> = constituent
        .iter()
        .copied()
        .filter(|f| !faces.contains(f) && !occupied.contains(f) && !floor_owners.contains(f))
        .filter(|&f| {
            if !super::planes::effective_plane(part, f)
                .is_some_and(|(n, _)| geom::norm(geom::cross(n, direction)) <= 1e-7)
            {
                return false;
            }
            let edges = part.face_edges(f);
            !edges.is_empty()
                && edges.iter().all(|&e| {
                    let adjacent = &part.edge_faces()[e];
                    adjacent.len() == 2
                        && adjacent.iter().all(|g| *g == f || walls.contains(g))
                        && [
                            part.edges[e].start,
                            part.edges[e].midpoint(),
                            part.edges[e].end,
                        ]
                        .iter()
                        .all(|&p| (geom::dot(p, direction) - end).abs() <= tol)
                })
        })
        .collect();
    if floors.is_empty() {
        return;
    }
    floor_owners.extend(floors.iter().copied());
    faces.extend(floors);
    faces.sort_unstable();
    faces.dedup();
}

/// Extend a completed inventory's record claim with its already-consulted drill
/// point, after reconciliation. Mouth seats remain context, not tip faces.
pub(crate) fn claim_drill_point(
    part: &Part,
    record: &HoleRecord,
    faces: &mut Vec<usize>,
    constituent: &[usize],
) {
    if record.bottom != Bottom::DrillPoint {
        return;
    }
    let axis = record.axis;
    let tol = 1e-7 * record.diameter.max(record.depth).max(1.);
    let end = faces
        .iter()
        .flat_map(|&f| part.face_edges(f))
        .flat_map(|e| [part.edges[e].start, part.edges[e].end])
        .map(|p| geom::dot(p, axis))
        .fold(f64::NEG_INFINITY, f64::max);
    let tips: Vec<_> = constituent
        .iter()
        .copied()
        .filter(|&f| {
            if record.mouth_chamfers.iter().any(|c| c.faces.contains(&f)) {
                return false;
            }
            let Some(apex) = part.faces[f].surface.cone_apex() else {
                return false;
            };
            let delta = geom::sub(apex, *record.location.raw());
            if geom::norm(geom::cross(delta, axis)) > tol || geom::dot(apex, axis) <= end + tol {
                return false;
            }
            faces.iter().any(|&g| {
                let shared = part.shared_edges(f, g);
                !shared.is_empty()
                    && shared.iter().all(|&e| {
                        [
                            part.edges[e].start,
                            part.edges[e].midpoint(),
                            part.edges[e].end,
                        ]
                        .iter()
                        .all(|&p| (geom::dot(p, axis) - end).abs() <= tol)
                    })
            })
        })
        .collect();
    faces.extend(tips);
    faces.sort_unstable();
    faces.dedup();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::brep::{Edge, Face, Loop, Solid};
    use crate::kernel::geom::{Curve, Frame};
    use crate::kernel::step::IDENTITY;
    use std::f64::consts::TAU;

    #[test]
    fn spotface_depth_and_ratio_boundaries_use_published_measurements() {
        for (diameter, raw_depth, published_depth, is_spotface) in [
            (16.4, 1.0, 1.0, true),
            (16.4, 1.01, 1.01, false),
            (40., 0.5, 0.5, true),
            (4., 0.9, 0.9, false),
            (4., 0.8, 0.8, false), // Ratio equality is excluded.
            (4., 0.79, 0.79, true),
            (16.4, 1.004, 1.0, true),
            (16.4, 1.006, 1.01, false),
            (4., 0.796, 0.8, false), // Ratio also reads rounded depth.
            (4., 0.794, 0.79, true),
        ] {
            let step = Segment {
                solid: 0,
                diameter,
                axis: 2,
                axis_point: [0.; 3],
                direction: [0., 0., 1.],
                s_lo: 0.,
                s_hi: raw_depth,
                external: false,
                faces: vec![7],
            };
            let NearSideSteps {
                cbore,
                spotface,
                outer_steps,
                faces,
            } = near_side_steps(&[step]);
            assert!(outer_steps.is_empty());
            let spec = Some(CounterBore {
                diameter,
                depth: published_depth,
            });
            assert_eq!(spotface, if is_spotface { spec.clone() } else { None });
            assert_eq!(cbore, if is_spotface { None } else { spec });
            assert_eq!(faces, [7]);
        }
    }

    /// Independently built turned ring: diameter 10 through, diameter 16.4 x 1.4
    /// recess at each end, with diameter 11.2 collars rising 0.7 from each floor.
    /// Only these analytic dimensions are shared with the #118 example; no STEP is read.
    fn collared_recesses() -> Part {
        // Walk the material boundary, starting up the outside then into the top mouth.
        let rings = [
            (9.5, 0.),
            (9.5, 29.),
            (8.2, 29.),
            (8.2, 27.6),
            (5.6, 27.6),
            (5.6, 28.3),
            (5., 28.3),
            (5., 0.7),
            (5.6, 0.7),
            (5.6, 1.4),
            (8.2, 1.4),
            (8.2, 0.),
        ];
        turned_ring(&rings)
    }

    /// Close a hand-written radial/axial profile with analytic circle and seam edges.
    fn turned_ring(rings: &[(f64, f64)]) -> Part {
        let frame = |z| Frame {
            origin: [0., 0., z],
            x: [1., 0., 0.],
            y: [0., 1., 0.],
            z: [0., 0., 1.],
        };
        let mut edges: Vec<_> = rings
            .iter()
            .enumerate()
            .map(|(i, &(radius, z))| {
                let curve = Curve::Circle {
                    frame: frame(z),
                    radius,
                };
                Edge {
                    start: curve.value(0.),
                    end: curve.value(0.),
                    samples: (0..=32)
                        .map(|j| curve.value(TAU * j as f64 / 32.))
                        .collect(),
                    curve,
                    vertices: (i, i),
                    same_sense: true,
                }
            })
            .collect();
        let mut faces = Vec::new();
        for (i, &(radius, z)) in rings.iter().enumerate() {
            let j = (i + 1) % rings.len();
            let (next_radius, next_z) = rings[j];
            let (surface, reversed, boundaries) = if radius == next_radius {
                let (bottom, top) = if z < next_z { (i, j) } else { (j, i) };
                let seam = edges.len();
                let (start, end) = (edges[bottom].start, edges[top].start);
                edges.push(Edge {
                    curve: Curve::Line {
                        origin: start,
                        dir: [0., 0., 1.],
                    },
                    start,
                    end,
                    vertices: (bottom, top),
                    same_sense: true,
                    samples: vec![start, end],
                });
                (
                    Surface::Cylinder {
                        frame: frame(0.),
                        radius,
                    },
                    z > next_z,
                    vec![vec![
                        (bottom, z < next_z),
                        (seam, true),
                        (top, z > next_z),
                        (seam, false),
                    ]],
                )
            } else if z != next_z {
                let (bottom, top) = if z < next_z { (i, j) } else { (j, i) };
                let (low_r, low_z) = rings[bottom];
                let (high_r, high_z) = rings[top];
                let seam = edges.len();
                let (start, end) = (edges[bottom].start, edges[top].start);
                edges.push(Edge {
                    curve: Curve::Line {
                        origin: start,
                        dir: geom::unit(geom::sub(end, start)).unwrap(),
                    },
                    start,
                    end,
                    vertices: (bottom, top),
                    same_sense: true,
                    samples: vec![start, end],
                });
                (
                    Surface::Cone {
                        frame: frame(low_z),
                        radius: low_r,
                        semi_angle: ((high_r - low_r) / (high_z - low_z)).atan(),
                    },
                    z > next_z,
                    vec![vec![
                        (bottom, true),
                        (seam, true),
                        (top, false),
                        (seam, false),
                    ]],
                )
            } else {
                assert_eq!(z, next_z);
                let reversed = radius < next_radius;
                (
                    Surface::Plane { frame: frame(z) },
                    reversed,
                    vec![vec![(i, true)], vec![(j, false)]],
                )
            };
            faces.push(Face {
                surface,
                reversed,
                solid: Some(0),
                pcurves: Vec::new(),
                loops: boundaries
                    .into_iter()
                    .map(|edges| Loop {
                        edges,
                        vertex: None,
                    })
                    .collect(),
            });
        }
        Part::new(
            faces,
            edges,
            vec![Solid {
                faces: (0..rings.len()).collect(),
            }],
        )
    }

    /// A turned plate section with an optional boss and counterbore. The radial
    /// cross-section is analytic test data, independent of STEP import and CAD kernels.
    fn chamfered_hole(
        boss: bool,
        counterbore: bool,
        size: f64,
        axial: f64,
        floor: bool,
        scale: f64,
    ) -> Part {
        let mut rings = vec![(15., -5.), (15., 0.)];
        let top = if boss { 2. } else { 0. };
        if boss {
            rings.extend([(4., 0.), (4., top)]);
        }
        if counterbore {
            if floor {
                rings.extend([
                    (2.5, top),
                    (2.5, top - 2.25 + axial),
                    (2.5 - size, top - 2.25),
                    (1.65, top - 2.25),
                ]);
            } else {
                rings.extend([
                    (2.5 + size, top),
                    (2.5, top - axial),
                    (2.5, top - 2.25),
                    (1.65, top - 2.25),
                ]);
            }
        } else {
            rings.extend([(1.65 + size, top), (1.65, top - axial)]);
        }
        rings.push((1.65, -5.));
        turned_ring(
            &rings
                .into_iter()
                .map(|(r, z)| (r * scale, z * scale))
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn mouth_chamfers_are_geometric_invariant_hole_details() {
        let (s, c) = 0.63_f64.sin_cos();
        for (boss, cbore, axial) in [
            (true, true, 0.25),
            (true, true, 0.4),
            (false, true, 0.25),
            (false, false, 0.25),
        ] {
            for scale in [0.1, 1., 10.] {
                let base = chamfered_hole(boss, cbore, 0.25, axial, false, scale);
                assert!(base.solid_is_geometrically_valid(0));
                for rotation in [
                    IDENTITY,
                    [
                        [c, -s, 0., 123.],
                        [s * c, c * c, -s, -37.],
                        [s * s, c * s, c, 19.],
                    ],
                ] {
                    let part = base.transformed(&rotation).unwrap();
                    let found = discover_verified(&Context::new(&part), &[]).unwrap();
                    assert_eq!(found.len(), 1);
                    let h = &found[0].record;
                    let core: Vec<_> = part
                        .faces
                        .iter()
                        .enumerate()
                        .filter_map(|(i, f)| {
                            (f.reversed && matches!(f.surface, Surface::Cylinder { .. }))
                                .then_some(i)
                        })
                        .collect();
                    let mut claimed = found[0].defining.clone();
                    claimed.sort_unstable();
                    assert_eq!(
                        claimed, core,
                        "treatments must preserve cylindrical hole anchors"
                    );
                    assert_eq!(
                        h.mouth_chamfers.len(),
                        1,
                        "boss={boss} cbore={cbore} axial={axial} scale={scale}"
                    );
                    let ch = &h.mouth_chamfers[0];
                    assert!((ch.size - 0.25 * scale).abs() < 1e-8);
                    assert!((ch.depth - axial * scale).abs() < 1e-8);
                    assert_eq!(ch.angle, py::round_to(axial.atan2(0.25).to_degrees(), 2));
                    assert_eq!(ch.diameter, if cbore { 5. * scale } else { 3.3 * scale });
                    assert!(ch.faces.iter().all(|f| !found[0].defining.contains(f) && found[0].context.contains(f)));
                    assert!(countersinks::discover(&Context::new(&part)).is_empty());
                    if cbore {
                        assert_eq!(
                            h.cbore.as_ref().unwrap().depth,
                            policy::round_measurement((2.25 - axial) * scale, 2)
                        );
                    }
                    let published = crate::correspondence::recognise(&part);
                    let doc = crate::recognition::document(&published, part.faces.len());
                    let hole = doc.records.iter().find(|r| r.family == "holes").unwrap();
                    assert!(ch.faces.iter().all(|f| !hole.faces.contains(f)));
                    for face in &ch.faces {
                        assert!(hole.claimed_faces().any(|f| f == *face));
                        // A whole turned-body record may already co-own this analytic
                        // ring. Preserve every existing core owner and add the hole.
                        let owners = doc
                            .records
                            .iter()
                            .filter(|r| r.faces.contains(face) || r.id == hole.id)
                            .map(|r| r.id.clone())
                            .collect();
                        assert_eq!(
                            doc.face_accounting.as_ref().unwrap()[*face],
                            crate::recognition::FaceAccounting::Feature { record_ids: owners }
                        );
                    }
                    let position: V3 = serde_json::from_value(
                        hole.parameters["mouth_chamfers"][0]["location"].clone(),
                    )
                    .unwrap();
                    assert!(geom::norm(geom::sub(position, *ch.location)) < 1e-7);
                    let expected = [0, 1, 2].map(|i| {
                        rotation[i][2] * if boss { 2. * scale } else { 0. } + rotation[i][3]
                    });
                    assert!(geom::norm(geom::sub(position, expected)) < 1e-7);
                    let fp = published
                        .fingerprints
                        .features
                        .iter()
                        .find(|f| f.family == "holes")
                        .unwrap();
                    assert_eq!(fp.faces, hole.faces);
                    assert!(!fp.sizes.keys().any(|key| key.starts_with("mouth_chamfers")));
                    let encoded = serde_json::to_value(h).unwrap();
                    assert_eq!(serde_json::from_value::<HoleRecord>(encoded).unwrap(), *h);
                }
            }
        }
    }

    #[test]
    fn both_plain_hole_ends_retain_their_edge_breaks_and_legacy_default() {
        let part = turned_ring(&[
            (10., 0.),
            (10., 5.),
            (2.25, 5.),
            (2., 4.75),
            (2., 0.25),
            (2.25, 0.),
        ]);
        let holes = discover_verified(&Context::new(&part), &[]).unwrap();
        assert_eq!(holes.len(), 1);
        let h = &holes[0].record;
        assert_eq!(h.mouth_chamfers.len(), 2);
        assert_eq!(h.depth, 4.5);
        assert!(geom::dot(h.mouth_chamfers[0].axis, h.mouth_chamfers[1].axis) < -0.999);
        assert!(
            h.mouth_chamfers
                .iter()
                .flat_map(|c| &c.faces)
                .all(|f| !holes[0].defining.contains(f) && holes[0].context.contains(f))
        );
        let mut legacy = serde_json::to_value(h).unwrap();
        legacy.as_object_mut().unwrap().remove("mouth_chamfers");
        let decoded: HoleRecord = serde_json::from_value(legacy.clone()).unwrap();
        assert!(decoded.mouth_chamfers.is_empty());
        assert_eq!(serde_json::to_value(decoded).unwrap(), legacy);
    }

    #[test]
    fn floor_bevel_is_not_a_mouth_and_screw_seat_stays_a_countersink() {
        let (s, c) = 0.37_f64.sin_cos();
        for scale in [0.1, 1., 10.] {
            for motion in [
                IDENTITY,
                [[c, 0., s, 123.], [0., 1., 0., -37.], [-s, 0., c, 19.]],
            ] {
                for is_seat in [false, true] {
                    let base = if is_seat {
                        chamfered_hole(false, false, 1., 1., false, scale)
                    } else {
                        chamfered_hole(true, true, 0.25, 0.25, true, scale)
                    };
                    let part = base.transformed(&motion).unwrap();
                    let ctx = Context::new(&part);
                    let cs = countersinks::discover(&ctx);
                    assert_eq!(cs.len(), usize::from(is_seat));
                    let holes = discover_verified(&ctx, &cs).unwrap();
                    assert_eq!(holes.len(), 1);
                    assert!(holes[0].record.mouth_chamfers.is_empty());
                    assert_eq!(holes[0].record.csink.is_some(), is_seat);
                    assert_eq!(holes[0].record.cbore.is_some(), !is_seat);
                }
            }
        }
    }

    #[test]
    fn convex_inner_counterbore_step_is_not_an_exterior_mouth() {
        let (s, c) = 0.37_f64.sin_cos();
        for scale in [0.1, 1., 10.] {
            // Convex edge break on the THROUGH-bore lip at the counterbore floor,
            // in addition to the outer floor/wall bevel tested above.
            let rings = [
                (15., -5.),
                (15., 0.),
                (4., 0.),
                (4., 2.),
                (2.5, 2.),
                (2.5, -0.25),
                (1.9, -0.25),
                (1.65, -0.5),
                (1.65, -5.),
            ];
            let base = turned_ring(&rings.map(|(r, z)| (r * scale, z * scale)));
            assert!(base.solid_is_geometrically_valid(0));
            for motion in [
                IDENTITY,
                [[c, 0., s, 123.], [0., 1., 0., -37.], [-s, 0., c, 19.]],
            ] {
                let part = base.transformed(&motion).unwrap();
                let ctx = Context::new(&part);
                let stacks = hole_stacks(&ctx);
                assert_eq!(stacks.len(), 1);
                let bore = &stacks[0][py::first_min(&stacks[0], |s| s.diameter)];
                // It meets the local coaxial-cone/convex-plane tests. Only its
                // position inside the complete stack excludes it as a mouth.
                assert!(
                    [false, true]
                        .into_iter()
                        .any(|hi| mouth_chamfer(&part, bore, hi).is_some())
                );
                let holes = discover_verified(&ctx, &[]).unwrap();
                assert_eq!(holes.len(), 1);
                assert!(holes[0].record.cbore.is_some());
                assert!(holes[0].record.mouth_chamfers.is_empty());
                assert!(countersinks::discover(&ctx).is_empty());
            }
        }
    }

    #[test]
    fn mouth_chamfer_requires_coaxial_surface_and_convex_exterior_support() {
        let base = chamfered_hole(false, true, 0.25, 0.25, false, 1.);
        let cone = base
            .faces
            .iter()
            .position(|f| matches!(f.surface, Surface::Cone { .. }))
            .unwrap();
        let top = base
            .neighbours(cone)
            .into_iter()
            .find(|&f| matches!(base.faces[f].surface, Surface::Plane { .. }))
            .unwrap();
        for off_axis in [false, true] {
            let mut faces = base.faces.clone();
            if off_axis {
                let Surface::Cone { frame, .. } = &mut faces[cone].surface else {
                    unreachable!()
                };
                frame.origin[0] += 0.1;
            } else {
                faces[top].reversed = !faces[top].reversed;
            }
            let part = Part::new(faces, base.edges.clone(), base.solids.clone());
            let holes = discover(&Context::new(&part), &[]);
            assert!(holes.iter().all(|h| h.record.mouth_chamfers.is_empty()));
        }
    }

    #[test]
    fn shallow_and_steep_lead_ins_are_not_mouth_chamfers() {
        let (s, c) = 0.37_f64.sin_cos();
        for scale in [0.1, 1., 10.] {
            // Cover 5° and 86° measured from either the axis or the top plane.
            for degrees in [4_f64, 5., 85., 86.] {
                let (size, depth) = if degrees < 45. {
                    (0.2, 0.2 * degrees.to_radians().tan())
                } else {
                    (0.2 / degrees.to_radians().tan(), 0.2)
                };
                let base = chamfered_hole(false, true, size, depth, false, scale);
                assert!(base.solid_is_geometrically_valid(0));
                for motion in [
                    IDENTITY,
                    [[c, 0., s, 123.], [0., 1., 0., -37.], [-s, 0., c, 19.]],
                ] {
                    let part = base.transformed(&motion).unwrap();
                    let ctx = Context::new(&part);
                    assert!(countersinks::discover(&ctx).is_empty());
                    let holes = discover_verified(&ctx, &[]).unwrap();
                    assert_eq!(holes.len(), 1);
                    assert!(
                        holes[0].record.mouth_chamfers.is_empty(),
                        "angle={degrees} scale={scale}"
                    );
                    assert!(holes[0].record.cbore.is_some());
                }
            }
        }
    }

    #[test]
    fn ambiguous_half_millimetre_edge_on_two_millimetre_bore_keeps_the_seat_boundary() {
        for scale in [0.1, 1., 10.] {
            let part = turned_ring(
                &[(5., 0.), (5., 4.), (1.5, 4.), (1., 3.5), (1., 0.)]
                    .map(|(r, z)| (r * scale, z * scale)),
            );
            let ctx = Context::new(&part);
            let seats = countersinks::discover(&ctx);
            assert_eq!(seats.len(), 1);
            let holes = discover_verified(&ctx, &seats).unwrap();
            assert_eq!(holes.len(), 1);
            assert!(holes[0].record.mouth_chamfers.is_empty());
            let seat = holes[0].record.csink.as_ref().unwrap();
            assert_eq!(
                seat.major_diameter,
                policy::round_measurement(3. * scale, 2)
            );
            assert_eq!(seat.included_angle, 90.);
        }
    }

    /// Split just the cone and its two circular rims into complementary half
    /// patches. Adjacent cylinder/plane loops retain both shared rim arcs, and
    /// the cone patches share the original seam plus an independently built pi seam.
    fn split_mouth_cone(base: &Part) -> Part {
        let mut faces = base.faces.clone();
        let mut edges = base.edges.clone();
        let cone = faces
            .iter()
            .position(|f| matches!(f.surface, Surface::Cone { .. }))
            .unwrap();
        let loop_edges = faces[cone].loops[0].edges.clone();
        let (low, seam, high) = (loop_edges[0].0, loop_edges[1].0, loop_edges[2].0);
        let mut halves = Vec::new();
        let next_vertex = edges
            .iter()
            .flat_map(|e| [e.vertices.0, e.vertices.1])
            .max()
            .unwrap()
            + 1;
        for (pi_vertex, rim) in (next_vertex..).zip([low, high]) {
            let original = edges[rim].clone();
            let mut second = original.clone();
            edges[rim].end = original.curve.value(std::f64::consts::PI);
            edges[rim].vertices.1 = pi_vertex;
            edges[rim].samples = (0..=16)
                .map(|i| original.curve.value(std::f64::consts::PI * i as f64 / 16.))
                .collect();
            second.start = edges[rim].end;
            second.vertices.0 = pi_vertex;
            second.samples = (0..=16)
                .map(|i| {
                    original
                        .curve
                        .value(std::f64::consts::PI * (1. + i as f64 / 16.))
                })
                .collect();
            let other = edges.len();
            edges.push(second);
            halves.push(other);
            for (face_index, face) in faces.iter_mut().enumerate() {
                if face_index == cone {
                    continue;
                }
                for boundary in &mut face.loops {
                    boundary.edges = boundary
                        .edges
                        .iter()
                        .flat_map(|&(e, forward)| {
                            if e != rim {
                                vec![(e, forward)]
                            } else if forward {
                                vec![(rim, true), (other, true)]
                            } else {
                                vec![(other, false), (rim, false)]
                            }
                        })
                        .collect();
                }
            }
        }
        let pi_seam = edges.len();
        let start = edges[low].end;
        let end = edges[high].end;
        edges.push(Edge {
            curve: Curve::Line {
                origin: start,
                dir: geom::unit(geom::sub(end, start)).unwrap(),
            },
            start,
            end,
            vertices: (edges[low].vertices.1, edges[high].vertices.1),
            same_sense: true,
            samples: vec![start, end],
        });
        let mut other = faces[cone].clone();
        // Closed circles in the original helper are direction-exempt; these
        // open arcs must run opposite their cylinder/plane neighbours.
        faces[cone].loops[0].edges =
            vec![(low, false), (seam, true), (high, true), (pi_seam, false)];
        other.loops[0].edges = vec![
            (halves[0], false),
            (pi_seam, true),
            (halves[1], true),
            (seam, false),
        ];
        let other_face = faces.len();
        faces.push(other);
        let mut solids = base.solids.clone();
        solids[0].faces.push(other_face);
        Part::new(faces, edges, solids)
    }

    #[test]
    fn two_half_cone_patches_preserve_the_bore_but_abstain_from_a_mouth_claim() {
        let (s, c) = 0.37_f64.sin_cos();
        for scale in [0.1, 1., 10.] {
            let base = split_mouth_cone(&chamfered_hole(false, true, 0.25, 0.25, false, scale));
            assert!(base.solid_is_geometrically_valid(0));
            for motion in [
                IDENTITY,
                [[c, 0., s, 123.], [0., 1., 0., -37.], [-s, 0., c, 19.]],
            ] {
                let part = base.transformed(&motion).unwrap();
                let cones: Vec<_> = part
                    .faces
                    .iter()
                    .enumerate()
                    .filter_map(|(i, f)| matches!(f.surface, Surface::Cone { .. }).then_some(i))
                    .collect();
                assert_eq!(cones.len(), 2);
                for &cone in &cones {
                    let (lo, hi, _, _) = part.uv_bounds(cone).unwrap();
                    assert!((hi - lo - std::f64::consts::PI).abs() < 1e-8);
                }
                let holes = discover_verified(&Context::new(&part), &[]).unwrap();
                assert_eq!(holes.len(), 1);
                assert_eq!(holes[0].record.cbore.as_ref().unwrap().diameter, 5. * scale);
                assert!(holes[0].record.mouth_chamfers.is_empty());
                assert!(cones.iter().all(|f| !holes[0].defining.contains(f)));
            }
        }
    }

    #[test]
    fn collared_recesses_are_counterbores_and_collars_stay_bosses() {
        let base = collared_recesses();
        assert!(base.solid_is_geometrically_valid(0));
        let (s, c) = 0.37_f64.sin_cos();
        for motion in [
            IDENTITY,
            [[c, 0., s, 123.456], [0., 1., 0., -78.9], [-s, 0., c, 41.3]],
        ] {
            let part = base.transformed(&motion).unwrap();
            let found = discover(&Context::new(&part), &[]);
            assert_eq!(found.len(), 1);
            let hole = &found[0].record;
            assert_eq!(
                (hole.diameter, hole.depth, hole.bottom),
                (10., 27.6, Bottom::Through)
            );
            assert_eq!(
                hole.cbore,
                Some(CounterBore {
                    diameter: 16.4,
                    depth: 1.4
                })
            );
            assert!(hole.spotface.is_none());
            let mut faces = found[0].defining.clone();
            faces.sort_unstable();
            assert_eq!(faces, [2, 6, 10], "bore and both recess walls");
            let recognition = crate::correspondence::recognise(&part);
            let doc = crate::recognition::document(&recognition, part.faces.len());
            let holes: Vec<_> = doc.records.iter().filter(|r| r.family == "holes").collect();
            assert_eq!(holes.len(), 1);
            assert_eq!(
                holes[0].parameters["cbore"],
                serde_json::json!({"diameter":16.4,"depth":1.4})
            );
            assert!(holes[0].parameters["spotface"].is_null());
            let collars: Vec<_> = doc
                .records
                .iter()
                .filter(|r| r.family == "bosses" && r.parameters["diameter"] == 11.2)
                .collect();
            assert_eq!(collars.len(), 2);
            for collar in collars {
                assert_eq!(collar.parameters["height"], 0.7);
            }
        }
    }

    #[test]
    fn two_step_holes_keep_the_inner_counterbore_and_publish_the_outer_classification() {
        let (s, c) = 0.37_f64.sin_cos();
        for outer_depth in [5., 0.5] {
            let floor = 30. - outer_depth;
            let base = turned_ring(&[
                (40., 0.),
                (40., 30.),
                (30., 30.),
                (30., floor),
                (9., floor),
                (9., floor - 6.),
                (5.05, floor - 6.),
                (5.05, 0.),
            ]);
            assert!(base.solid_is_geometrically_valid(0));
            for motion in [
                IDENTITY,
                [[c, 0., s, 123.456], [0., 1., 0., -78.9], [-s, 0., c, 41.3]],
            ] {
                let part = base.transformed(&motion).unwrap();
                let found = discover(&Context::new(&part), &[]);
                assert_eq!(found.len(), 1);
                let hole = &found[0].record;
                assert_eq!(
                    hole.cbore,
                    Some(CounterBore {
                        diameter: 18.,
                        depth: 6.
                    })
                );
                let outer = CounterBore {
                    diameter: 60.,
                    depth: outer_depth,
                };
                if outer_depth == 5. {
                    assert_eq!(hole.outer_steps, [outer]);
                    assert!(hole.spotface.is_none());
                } else {
                    assert!(hole.outer_steps.is_empty());
                    assert_eq!(hole.spotface, Some(outer));
                }
                let mut faces = found[0].defining.clone();
                faces.sort_unstable();
                assert_eq!(
                    faces,
                    [2, 4, 6],
                    "every cylindrical step remains defining evidence"
                );
                let recognition = crate::correspondence::recognise(&part);
                let doc = crate::recognition::document(&recognition, part.faces.len());
                let record = doc.records.iter().find(|r| r.id == "holes/0").unwrap();
                assert_eq!(
                    record.parameters["cbore"],
                    serde_json::json!({"diameter":18.,"depth":6.})
                );
                assert_eq!(record.faces, [2, 4, 6]);
                let fp = recognition
                    .fingerprints
                    .features
                    .iter()
                    .find(|f| f.id == "holes/0")
                    .unwrap();
                assert_eq!(fp.sizes["cbore.depth"], 6.);
                assert_eq!(fp.sizes["cbore.diameter"], 18.);
                if outer_depth == 5. {
                    assert_eq!(
                        record.parameters["outer_steps"],
                        serde_json::json!([{"diameter":60.,"depth":5.}])
                    );
                    assert_eq!(fp.sizes["outer_steps.count"], 1.);
                    assert_eq!(fp.sizes["outer_steps.0.diameter"], 60.);
                    assert_eq!(fp.sizes["outer_steps.0.depth"], 5.);
                } else {
                    assert!(record.parameters.get("outer_steps").is_none());
                    assert!(!fp.sizes.keys().any(|k| k.starts_with("outer_steps")));
                }
                let encoded = serde_json::to_value(hole).unwrap();
                let decoded: HoleRecord = serde_json::from_value(encoded.clone()).unwrap();
                assert_eq!(*hole, decoded);
                let mut legacy = encoded;
                legacy.as_object_mut().unwrap().remove("outer_steps");
                let decoded: HoleRecord = serde_json::from_value(legacy).unwrap();
                assert!(
                    decoded.outer_steps.is_empty(),
                    "old records default to no outer steps"
                );
                assert!(
                    serde_json::to_value(decoded)
                        .unwrap()
                        .get("outer_steps")
                        .is_none()
                );
            }
        }
    }

    #[test]
    fn multiple_outer_steps_keep_mouth_order_and_fingerprint_diameter_depth_pairs() {
        let part = turned_ring(&[
            (45., 0.),
            (45., 40.),
            (35., 40.),
            (35., 35.),
            (25., 35.),
            (25., 32.),
            (9., 32.),
            (9., 26.),
            (5., 26.),
            (5., 0.),
        ]);
        assert!(part.solid_is_geometrically_valid(0));
        let found = discover(&Context::new(&part), &[]);
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].record.cbore,
            Some(CounterBore {
                diameter: 18.,
                depth: 6.
            })
        );
        assert_eq!(
            found[0].record.outer_steps,
            [
                CounterBore {
                    diameter: 70.,
                    depth: 5.
                },
                CounterBore {
                    diameter: 50.,
                    depth: 3.
                }
            ]
        );
        let recognition = crate::correspondence::recognise(&part);
        let fp = recognition
            .fingerprints
            .features
            .iter()
            .find(|f| f.id == "holes/0")
            .unwrap();
        assert_eq!(fp.sizes["outer_steps.count"], 2.);
        assert_eq!(fp.sizes["outer_steps.0.diameter"], 70.);
        assert_eq!(fp.sizes["outer_steps.0.depth"], 5.);
        assert_eq!(fp.sizes["outer_steps.1.diameter"], 50.);
        assert_eq!(fp.sizes["outer_steps.1.depth"], 3.);

        let mut holes: Vec<_> = [0., 100., 200.]
            .into_iter()
            .map(|x| {
                let mut hole = found[0].record.clone();
                hole.location = [x, 0., 40.].into();
                hole
            })
            .collect();
        let patterns = super::super::hole_patterns::recognise_hole_patterns(&holes);
        assert_eq!(
            patterns.len(),
            1,
            "equal outer steps can form a hole pattern"
        );
        let published = serde_json::to_value(&patterns[0]).unwrap();
        assert_eq!(
            published["holes"][0]["outer_steps"],
            serde_json::json!([
                {"diameter":70.,"depth":5.}, {"diameter":50.,"depth":3.}
            ])
        );
        holes[1].outer_steps[0].depth = 4.;
        assert!(
            super::super::hole_patterns::recognise_hole_patterns(&holes).is_empty(),
            "different outer-step specs must not be grouped just because the cbore matches"
        );
    }
}
