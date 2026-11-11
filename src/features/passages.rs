//! Prismatic passages (`quiddity.passages`): a void running through the material, its walls a
//! closed ring of planar faces capped at neither end.
//!
//! Two entry points, as in Python. `recognise_section_passages` publishes `SectionPassage`
//! records from the section-ring proposals ([`super::section_passages`]): the section in a
//! canonical run-local frame, on any run direction, with the termination planes' gradients.
//! `recognise_passages` is the frozen pre-0.4 finder over principal-axis rings
//! ([`super::rings`]), kept for its legacy `Passage` value; each section passage that it also
//! found must reproduce that value, and a disagreement is refused, not resolved.
//!
//! Python raises `ValueError` on internal inconsistencies (competing roster matches, a changed
//! body, a serialisation that moves the geometry, an irreproducible legacy value): here those are
//! [`PassageError`] refusals, never a silent drop. Python's `ledger` argument to
//! `recognise_passages` is retired there (`PassageCompatibilityError`) and has no counterpart.
//! Python's `local_degradation` graph mode, which skips proposals without a common valid solid
//! on the evidence path, is not ported: the default inventory retries in it on three corpus
//! parts, and this family's check of the flag is reached on none of them (`tests/local_degradation.rs`).

use std::fmt;

use serde::{Deserialize, Serialize};

use super::Context;
use super::evidence::{Occurrence, common_valid_solid};
use super::passage_compat::{
    PassageCompatibilityView, canonical_section, compatibility_view, passage_from_view,
    principal_projection,
};
use super::placed::{Grid, Placed};
use super::rings::{centroid, rings};
use super::section_passages::{SectionRingProposal, section_ring_proposals};
use super::sections::{PlanarSection, SectionError, SectionVertex, V2};
use crate::kernel::brep::Part;
use crate::kernel::geom::{V3, cross};
use crate::kernel::py;

const SECTION_SERIALIZATION_LIMIT: f64 = 0.002;

/// A refusal, with the Python implementation's `ValueError` message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PassageError(pub &'static str);

impl fmt::Display for PassageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for PassageError {}

impl From<SectionError> for PassageError {
    fn from(e: SectionError) -> Self {
        PassageError(e.0)
    }
}

type Checked<T> = Result<T, PassageError>;

fn refuse<T>(message: &'static str) -> Checked<T> {
    Err(PassageError(message))
}

/// A legacy passage (`Passage`): the principal axis it runs along, its wall count, its length
/// and centre, and its cross-section corners in the two other axes, all to 3 decimals; the
/// corners anticlockwise from the least.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Passage {
    pub axis: String,
    pub sides: usize,
    pub length: f64,
    pub at: V3,
    pub section: Vec<V2>,
}

/// A section passage's placement frame (`PassageFrame`): origin to 3 decimals, directions to 6.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PassageFrame {
    pub origin: Placed<V3>,
    pub run: V3,
    pub u: V3,
    pub v: V3,
}

/// One section vertex (`PassageSectionVertex`): its point to 4 decimals and the bulge of the
/// edge to the next to 12.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PassageSectionVertex {
    pub point: V2,
    pub bulge: f64,
}

/// The canonical, origin-centred section (`PassageSection`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PassageSection {
    pub boundary: Vec<PassageSectionVertex>,
}

/// Both ends open, and each termination plane's gradient across the section (`PassageEnds`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PassageEnds {
    pub low_capped: bool,
    pub high_capped: bool,
    pub low_gradient: V2,
    pub high_gradient: V2,
}

/// A recognised section passage (`SectionPassage`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SectionPassage {
    pub frame: PassageFrame,
    pub run_interval: Placed<(f64, f64)>,
    pub section: PassageSection,
    pub ends: PassageEnds,
}

/// `_numbers`: finite values, negative zeros cleared.
fn numbers<const N: usize>(values: [f64; N], name: &'static str) -> Checked<[f64; N]> {
    if values.iter().any(|v| !v.is_finite()) {
        return refuse(name);
    }
    Ok(values.map(py::without_negative_zero))
}

/// `_serialized`: at most *digits* decimal places.
fn serialized(values: &[f64], digits: usize, message: &'static str) -> Checked<()> {
    if values.iter().any(|&v| v != py::round_to(v, digits)) {
        return refuse(message);
    }
    Ok(())
}

fn dot(a: V3, b: V3) -> f64 {
    py::dot(&a, &b)
}

impl PassageFrame {
    /// The frame, refused unless serialized, unit, orthogonal, right handed and canonical
    /// within the serialization tolerances (`PassageFrame.__post_init__`).
    pub fn new(origin: Placed<V3>, run: V3, u: V3, v: V3) -> Checked<Self> {
        let value = numbers(*origin, "origin must contain finite numbers")?;
        // A negative zero is cleared as Python's `_numbers` does; a published origin has none.
        let origin = if value == *origin {
            origin
        } else {
            Placed::exact(value)
        };
        let run = numbers(run, "run must contain finite numbers")?;
        let u = numbers(u, "u must contain finite numbers")?;
        let v = numbers(v, "v must contain finite numbers")?;
        serialized(&*origin, 3, "origin must use at most 3 decimal places")?;
        serialized(&run, 6, "run must use at most 6 decimal places")?;
        serialized(&u, 6, "u must use at most 6 decimal places")?;
        serialized(&v, 6, "v must use at most 6 decimal places")?;
        for d in [run, u, v] {
            // The extra 1e-12 only absorbs binary evaluation of the closed decimal boundary.
            if (dot(d, d).sqrt() - 1.0).abs() > 1e-6 + 1e-12 {
                return refuse("frame directions must be unit length");
            }
        }
        if [(run, u), (run, v), (u, v)]
            .iter()
            .any(|&(a, b)| dot(a, b).abs() > 2e-6)
        {
            return refuse("frame directions must be orthogonal");
        }
        let handed = cross(run, u);
        if (0..3).map(|i| (handed[i] - v[i]).abs()).fold(0.0, f64::max) > 3e-6 {
            return refuse("frame must be right handed");
        }
        let rounded = run.map(|c| py::round_to(c.abs(), 6));
        let peak = rounded.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let dominant = [2, 1, 0]
            .into_iter()
            .find(|&i| rounded[i] == peak)
            .expect("the peak is a component");
        if run[dominant] < -3e-6 {
            return refuse("frame run direction is not in the canonical gauge");
        }
        let normalized =
            py::unit(run).ok_or(PassageError("direction is nonfinite or degenerate"))?;
        let seed = [[0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]][dominant];
        let along = dot(seed, normalized);
        let expected_u = py::unit([0, 1, 2].map(|i| seed[i] - along * normalized[i]))
            .ok_or(PassageError("direction is nonfinite or degenerate"))?;
        let expected_v = cross(normalized, expected_u);
        if py::dist(&u, &expected_u) > 3e-6 || py::dist(&v, &expected_v) > 3e-6 {
            return refuse("frame in-plane basis is not canonical");
        }
        if dot(*origin, run).abs() > 8e-4 {
            return refuse("frame origin must be perpendicular to its run");
        }
        Ok(PassageFrame { origin, run, u, v })
    }
}

impl PassageSectionVertex {
    pub fn new(point: V2, bulge: f64) -> Checked<Self> {
        let point = numbers(point, "point must contain finite numbers")?;
        serialized(&point, 4, "point must use at most 4 decimal places")?;
        if !bulge.is_finite() {
            return refuse("bulge must be a finite number");
        }
        serialized(&[bulge], 12, "bulge must use at most 12 decimal places")?;
        Ok(PassageSectionVertex {
            point,
            bulge: py::without_negative_zero(bulge),
        })
    }
}

impl PassageSection {
    /// The section, refused unless it is already canonical and centred on the origin within
    /// 8e-4 (`PassageSection.__post_init__`).
    pub fn new(boundary: Vec<PassageSectionVertex>) -> Checked<Self> {
        let canonical = boundary
            .iter()
            .map(|v| SectionVertex::new(v.point, v.bulge))
            .collect::<Result<Vec<_>, _>>()
            .and_then(PlanarSection::new)
            .map_err(|_| PassageError("section boundary is invalid"))?;
        let same = canonical.boundary().len() == boundary.len()
            && canonical
                .boundary()
                .iter()
                .zip(&boundary)
                .all(|(c, v)| c.point == v.point && c.bulge == v.bulge);
        if !same || py::hypot(&canonical.centroid()) > 8e-4 {
            return refuse("section boundary must be canonical and origin-centred");
        }
        Ok(PassageSection { boundary })
    }
}

impl PassageEnds {
    pub fn new(
        low_capped: bool,
        high_capped: bool,
        low_gradient: V2,
        high_gradient: V2,
    ) -> Checked<Self> {
        let low_gradient = numbers(low_gradient, "low_gradient must contain finite numbers")?;
        let high_gradient = numbers(high_gradient, "high_gradient must contain finite numbers")?;
        serialized(
            &low_gradient,
            6,
            "low_gradient must use at most 6 decimal places",
        )?;
        serialized(
            &high_gradient,
            6,
            "high_gradient must use at most 6 decimal places",
        )?;
        Ok(PassageEnds {
            low_capped,
            high_capped,
            low_gradient,
            high_gradient,
        })
    }
}

impl SectionPassage {
    /// The record, refused unless its interval increases, both ends are open, a sloped
    /// termination has a line-only section, and the termination planes do not cross the
    /// section (`SectionPassage.__post_init__`).
    pub fn new(
        frame: PassageFrame,
        run_interval: Placed<(f64, f64)>,
        section: PassageSection,
        ends: PassageEnds,
    ) -> Checked<Self> {
        let [lo, hi] = numbers(
            [run_interval.0, run_interval.1],
            "run_interval must contain finite numbers",
        )?;
        let run_interval = if (lo, hi) == *run_interval {
            run_interval
        } else {
            Placed::exact((lo, hi))
        };
        serialized(
            &[lo, hi],
            3,
            "run_interval must use at most 3 decimal places",
        )?;
        if hi - lo <= 1e-9 {
            return refuse("run_interval must be increasing");
        }
        if ends.low_capped || ends.high_capped {
            return refuse("SectionPassage must be open at both ends");
        }
        let sloped = ends.low_gradient != [0.0, 0.0] || ends.high_gradient != [0.0, 0.0];
        if sloped && section.boundary.iter().any(|v| v.bulge != 0.0) {
            return refuse("sloped passage terminations require a line-only section");
        }
        let (lg, hg) = (ends.low_gradient, ends.high_gradient);
        if section.boundary.iter().any(|v| {
            let [x, y] = v.point;
            lo + lg[0] * x + lg[1] * y >= hi + hg[0] * x + hg[1] * y - 1e-9
        }) {
            return refuse("passage termination planes must not cross the section");
        }
        Ok(SectionPassage {
            frame,
            run_interval,
            section,
            ends,
        })
    }
}

internal_entry! {
/// Recognise the legacy prismatic passages of *part* (`recognise_passages`): one per closed
/// ring capped at neither end, by axis then centre. Through slots are reported too (a closed
/// uncapped ring); Python's aggregate reconciles them, and neither does it here.
pub fn recognise_passages(part: &Part) -> Vec<Passage> {
    legacy_roster(&Context::new(part))
        .into_iter()
        .map(|(record, _)| record)
        .collect()
}
}

internal_entry! {
/// Recognise the section passages of *part* (`recognise_section_passages`), or the refusal
/// Python raises.
pub fn recognise_section_passages(part: &Part) -> Checked<Vec<SectionPassage>> {
    Ok(super::records(discover(&Context::new(part))?))
}
}

internal_entry! {
/// The section passages with their defining walls and consulted faces (the proposal's further
/// constituent faces: entry treatments or region), or a refusal.
pub fn discover(ctx: &Context<'_>) -> Checked<Vec<Occurrence<SectionPassage>>> {
    Ok(discover_section_passages(ctx)?
        .into_iter()
        .map(|found| {
            let mut defining = found.nodes;
            defining.sort_unstable();
            Occurrence {
                record: found.record,
                defining,
                context: found.constituent,
            }
        })
        .collect())
}
}

internal_entry! {
/// Each section passage's frozen compatibility view (Python's `passage_compatibility`), in
/// `discover`'s order: the slot grouping reconciliation reads ([`super::reconcile`]).
pub fn compatibility_views(ctx: &Context<'_>) -> Checked<Vec<PassageCompatibilityView>> {
    Ok(discover_section_passages(ctx)?
        .into_iter()
        .map(|found| found.compatibility)
        .collect())
}
}

internal_entry! {
/// Like `discover`, every occurrence's faces checked to share one valid solid; for the
/// evidence comparison.
pub fn discover_verified(ctx: &Context<'_>) -> Result<Vec<Occurrence<SectionPassage>>, String> {
    let found = discover(ctx).map_err(|e| e.to_string())?;
    super::evidence::verified(ctx.part, found).map_err(|e| e.to_string())
}
}

internal_entry! {
/// Two legacy passages describe one closed section (`_same_legacy_passage_geometry`): the same
/// axis, sides, length and canonical section, and the same centre, or a centre whose differing
/// coordinates are opposite 3-decimal roundings of one half-quantum *exact_at* coordinate.
pub fn same_legacy_passage_geometry(
    left: Option<&Passage>,
    right: &Passage,
    exact_at: Option<V3>,
) -> bool {
    let Some(left) = left else {
        return false;
    };
    if (left.axis.as_str(), left.sides, left.length)
        != (right.axis.as_str(), right.sides, right.length)
        || canonical_section(&left.section) != canonical_section(&right.section)
    {
        return false;
    }
    if left.at == right.at {
        return true;
    }
    let Some(exact_at) = exact_at else {
        return false;
    };
    (0..3).all(|i| {
        let (first, second, source) = (left.at[i], right.at[i], exact_at[i]);
        if first == second {
            return true;
        }
        // Only opposite roundings of the same source half-grid tie are equivalent; this
        // numerical epsilon is not the occurrence displacement allowance.
        let (first_grid, second_grid) = (
            (first * 1000.0).round_ties_even(),
            (second * 1000.0).round_ties_even(),
        );
        first == first_grid / 1000.0
            && second == second_grid / 1000.0
            && (first_grid - second_grid).abs() == 1.0
            && py::isclose(source, (first_grid + second_grid) / 2000.0, 0.0, 1e-9)
    })
}
}

/// The proposal's section rounded, then canonicalised in the serialized domain
/// (`_serialized_passage_section`).
fn serialized_passage_section(section: &PlanarSection) -> Checked<PassageSection> {
    let projected = PlanarSection::new(
        section
            .boundary()
            .iter()
            .map(|v| {
                SectionVertex::new(
                    v.point.map(|c| py::round_to(c, 4)),
                    py::round_to(v.bulge, 12),
                )
            })
            .collect::<Result<_, _>>()?,
    )?;
    PassageSection::new(
        projected
            .boundary()
            .iter()
            .map(|v| PassageSectionVertex::new(v.point, v.bulge))
            .collect::<Checked<_>>()?,
    )
}

/// One proposal as its public serialized value (`_section_passage_record`), refused when the
/// serialization moves any section vertex at either end by more than 2e-3.
pub(crate) fn section_passage_record(proposal: &SectionRingProposal) -> Checked<SectionPassage> {
    let o = &proposal.occurrence;
    let f = o.frame();
    let (lo, hi) = o.run_interval();
    let record = SectionPassage::new(
        PassageFrame::new(
            Placed::new(f.origin, Grid::CleanDecimals(3)),
            f.run.map(|c| py::round_to(c, 6)),
            f.u.map(|c| py::round_to(c, 6)),
            f.v.map(|c| py::round_to(c, 6)),
        )?,
        Placed::new((lo, hi), Grid::CleanDecimals(3)),
        serialized_passage_section(o.section())?,
        PassageEnds::new(
            false,
            false,
            proposal.low_gradient.map(|c| py::round_to(c, 6)),
            proposal.high_gradient.map(|c| py::round_to(c, 6)),
        )?,
    )?;
    if section_projection_displacement(proposal, &record) > SECTION_SERIALIZATION_LIMIT {
        return refuse("section passage serialization exceeds the displacement bound");
    }
    Ok(record)
}

/// The largest source-to-serialized movement of any section vertex at either end
/// (`_section_projection_displacement`); infinite when the rounded vertices do not pair with
/// the source's one to one.
fn section_projection_displacement(proposal: &SectionRingProposal, record: &SectionPassage) -> f64 {
    let o = &proposal.occurrence;
    let (f, interval) = (o.frame(), o.run_interval());
    let source = o.section().boundary();
    let projected: Vec<V2> = source
        .iter()
        .map(|v| v.point.map(|c| py::round_to(c, 4)))
        .collect();
    if (0..projected.len()).any(|i| projected[..i].contains(&projected[i])) {
        return f64::INFINITY;
    }
    let (rf, rends) = (&record.frame, &record.ends);
    let mut maximum: f64 = 0.0;
    for vertex in &record.section.boundary {
        let Some(at) = projected.iter().position(|p| *p == vertex.point) else {
            return f64::INFINITY;
        };
        let [sx, sy] = source[at].point;
        let [px, py_] = vertex.point;
        let source_ts = [
            interval.0 + proposal.low_gradient[0] * sx + proposal.low_gradient[1] * sy,
            interval.1 + proposal.high_gradient[0] * sx + proposal.high_gradient[1] * sy,
        ];
        let serialized_ts = [
            record.run_interval.0 + rends.low_gradient[0] * px + rends.low_gradient[1] * py_,
            record.run_interval.1 + rends.high_gradient[0] * px + rends.high_gradient[1] * py_,
        ];
        for (st, rt) in source_ts.into_iter().zip(serialized_ts) {
            let from: V3 =
                [0, 1, 2].map(|i| f.origin[i] + st * f.run[i] + sx * f.u[i] + sy * f.v[i]);
            let to: V3 =
                [0, 1, 2].map(|i| rf.origin[i] + rt * rf.run[i] + px * rf.u[i] + py_ * rf.v[i]);
            let squares = py::sum((0..3).map(|i| (from[i] - to[i]).powi(2)));
            maximum = maximum.max(squares.sqrt());
        }
    }
    maximum
}

/// The principal compatibility fact of a full-precision occurrence
/// (`_proposal_legacy_projection`): only for flat ends and a line-only section.
fn proposal_legacy_projection(
    proposal: &SectionRingProposal,
) -> Option<super::passage_compat::PrincipalProjection> {
    let o = &proposal.occurrence;
    if proposal.low_gradient != [0.0, 0.0]
        || proposal.high_gradient != [0.0, 0.0]
        || o.section().boundary().iter().any(|v| v.bulge != 0.0)
    {
        return None;
    }
    let f = o.frame();
    let points: Vec<V2> = o.section().boundary().iter().map(|v| v.point).collect();
    principal_projection(f.origin, f.run, f.u, f.v, o.run_interval(), &points)
}

internal_entry! {
/// The frozen pre-0.4 finder and its discovery order (`_legacy_roster`): every ring capped at
/// neither end as a legacy passage with its walls, by axis then centre.
pub fn legacy_roster(ctx: &Context<'_>) -> Vec<(Passage, Vec<usize>)> {
    let mut found: Vec<(Passage, Vec<usize>)> = rings(ctx)
        .into_iter()
        .filter(|ring| ring.caps() == (false, false))
        .map(|ring| {
            let others: Vec<usize> = (0..3).filter(|&a| a != ring.axis).collect();
            let middle = centroid(&ring.section);
            let mut at = [0.0; 3];
            at[ring.axis] = 0.5 * (ring.low + ring.high);
            at[others[0]] = middle[0];
            at[others[1]] = middle[1];
            let passage = Passage {
                axis: ["x", "y", "z"][ring.axis].into(),
                sides: ring.nodes.len(),
                length: py::round_to(ring.high - ring.low, 3),
                at: at.map(|c| py::round_to(c, 3)),
                section: ring
                    .section
                    .iter()
                    .map(|p| p.map(|c| py::round_to(c, 3)))
                    .collect(),
            };
            (passage, ring.nodes)
        })
        .collect();
    found.sort_by(|(a, _), (b, _)| a.axis.cmp(&b.axis).then(py::tuple_order(&a.at[..], &b.at[..])));
    found
}
}

/// A section passage before publication: its record, walls, solid and compatibility view.
#[derive(Clone)]
pub(super) struct Found {
    record: SectionPassage,
    nodes: Vec<usize>,
    /// The proposal's constituent faces beyond its walls (its entry treatments or region).
    constituent: Vec<usize>,
    solid: usize,
    compatibility: PassageCompatibilityView,
}

fn same_nodes(a: &[usize], b: &[usize]) -> bool {
    a.len() == b.len() && a.iter().all(|n| b.contains(n))
}

/// The run's section passages before publication, found once per run (`Context`).
fn discover_section_passages(ctx: &Context<'_>) -> Checked<Vec<Found>> {
    ctx.section_passages().clone()
}

/// `_discover_section_passages`: each proposal serialized, matched to the legacy roster by its
/// walls (whose legacy value it must reproduce), duplicates of one wall set merged, sorted by
/// run, interval and origin.
pub(super) fn find_section_passages(ctx: &Context<'_>) -> Checked<Vec<Found>> {
    let part = ctx.part;
    let proposals = section_ring_proposals(ctx)?;
    if proposals.is_empty() {
        return Ok(Vec::new());
    }
    let roster = legacy_roster(ctx);
    for (i, (_, nodes)) in roster.iter().enumerate() {
        if roster[..i]
            .iter()
            .any(|(_, other)| same_nodes(nodes, other))
        {
            return refuse("legacy passage roster has competing defining-node matches");
        }
    }
    let mut found: Vec<Found> = Vec::new();
    for proposal in &proposals {
        let projection = proposal_legacy_projection(proposal);
        let full_precision = match &projection {
            Some(p) => Some(passage_from_view(&compatibility_view(
                Some(p),
                true,
                Some(0),
            )?)?),
            None => None,
        };
        let record = section_passage_record(proposal)?;
        if common_valid_solid(part, &proposal.nodes) != Some(proposal.solid) {
            return refuse("section passage body authority changed before issuance");
        }
        // Python's `body_adapter.validate` re-checks the occurrence's run-owned body: the
        // proposal's occurrence was validated against its issuer when it was built and cannot
        // change since.
        let historical = roster
            .iter()
            .enumerate()
            .find(|(_, (_, nodes))| same_nodes(nodes, &proposal.nodes));
        let compatibility = match historical {
            Some((ordinal, (legacy, _))) => {
                // Compatibility is an issuance-time fact from the full-precision occurrence: an
                // odd number of millimetre quanta has a half-quantum midpoint (10060.step), so
                // it cannot always be re-derived from the three-decimal span. The frozen finder
                // is the authority for its legacy value.
                let o = &proposal.occurrence;
                let mut exact_at = o.frame().origin;
                let axis = ["x", "y", "z"]
                    .iter()
                    .position(|a| *a == legacy.axis)
                    .expect("an axis letter");
                exact_at[axis] = py::sum([o.run_interval().0, o.run_interval().1]) / 2.0;
                if !same_legacy_passage_geometry(full_precision.as_ref(), legacy, Some(exact_at)) {
                    return refuse("rich passage cannot reproduce its historical legacy value");
                }
                PassageCompatibilityView::new(
                    Some(["x", "y", "z"][axis]),
                    Some(legacy.section.clone()),
                    Some(legacy.sides),
                    Some(legacy.length),
                    Some(legacy.at),
                    Some(ordinal),
                    true,
                )?
            }
            None => compatibility_view(projection.as_ref(), false, None)?,
        };
        let mut duplicate = false;
        for other in &found {
            if proposal.solid == other.solid && same_nodes(&proposal.nodes, &other.nodes) {
                if record != other.record || compatibility != other.compatibility {
                    return refuse("one passage defining set produced competing records");
                }
                duplicate = true;
                break;
            }
            if compatibility.eligible()
                && other.compatibility.eligible()
                && compatibility.legacy_ordinal() == other.compatibility.legacy_ordinal()
            {
                return refuse("one legacy passage occurrence matched multiple rich proposals");
            }
        }
        if !duplicate {
            found.push(Found {
                record,
                nodes: proposal.nodes.clone(),
                constituent: proposal
                    .constituent
                    .iter()
                    .copied()
                    .filter(|f| !proposal.nodes.contains(f))
                    .collect(),
                solid: proposal.solid,
                compatibility,
            });
        }
    }
    let key = |f: &Found| {
        let r = &f.record;
        let mut k = r.frame.run.to_vec();
        k.extend([r.run_interval.0, r.run_interval.1]);
        k.extend(*r.frame.origin);
        k
    };
    found.sort_by(|a, b| py::tuple_order(&key(a), &key(b)));
    for (at, a) in found.iter().enumerate() {
        for b in &found[at + 1..] {
            if a.record == b.record && a.solid == b.solid && !same_nodes(&a.nodes, &b.nodes) {
                return refuse("equal section passage proposals compete on one solid");
            }
        }
    }
    Ok(found)
}

/// Source scope for a whole-family refusal. If proposal construction itself refused, its
/// planar inputs are the evidence under consideration; no particular wall claim was issued.
pub(crate) fn proposal_faces(ctx: &Context<'_>) -> Vec<usize> {
    match section_ring_proposals(ctx) {
        Ok(proposals) => proposals
            .iter()
            .flat_map(|p| p.nodes.iter().chain(&p.constituent))
            .copied()
            .collect(),
        Err(_) => (0..ctx.part.faces.len())
            .filter(|&f| super::graph::is_planar(ctx.part, f))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy(at: V3) -> Passage {
        Passage {
            axis: "z".into(),
            sides: 4,
            length: 33.245,
            at,
            section: vec![[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]],
        }
    }

    #[test]
    fn only_opposite_roundings_of_one_half_quantum_tie_are_one_centre() {
        let (a, b) = (legacy([0.0, 0.0, 16.622]), legacy([0.0, 0.0, 16.623]));
        assert!(same_legacy_passage_geometry(Some(&a), &a, None));
        assert!(!same_legacy_passage_geometry(Some(&a), &b, None));
        assert!(same_legacy_passage_geometry(
            Some(&a),
            &b,
            Some([0.0, 0.0, 16.6225])
        ));
        assert!(!same_legacy_passage_geometry(
            Some(&a),
            &b,
            Some([0.0, 0.0, 16.6226])
        ));
        let c = legacy([0.0, 0.0, 16.624]);
        assert!(!same_legacy_passage_geometry(
            Some(&a),
            &c,
            Some([0.0, 0.0, 16.623])
        ));
        assert!(!same_legacy_passage_geometry(None, &a, None));
        let mut turned = a.clone();
        turned.section.rotate_left(1);
        assert!(same_legacy_passage_geometry(Some(&a), &turned, None));
    }

    #[test]
    fn a_record_refuses_capped_ends_and_crossing_terminations() {
        let frame = PassageFrame::new(
            [0.0; 3].into(),
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        )
        .unwrap();
        let section = PassageSection::new(
            [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]
                .into_iter()
                .map(|p| PassageSectionVertex::new(p, 0.0).unwrap())
                .collect(),
        )
        .unwrap();
        let open = PassageEnds::new(false, false, [0.0; 2], [0.0; 2]).unwrap();
        assert!(
            SectionPassage::new(
                frame.clone(),
                (0.0, 1.0).into(),
                section.clone(),
                open.clone()
            )
            .is_ok()
        );
        assert_eq!(
            SectionPassage::new(frame.clone(), (1.0, 1.0).into(), section.clone(), open)
                .unwrap_err(),
            PassageError("run_interval must be increasing")
        );
        let capped = PassageEnds::new(true, false, [0.0; 2], [0.0; 2]).unwrap();
        assert!(
            SectionPassage::new(frame.clone(), (0.0, 1.0).into(), section.clone(), capped).is_err()
        );
        let steep = PassageEnds::new(false, false, [1.0, 0.0], [0.0; 2]).unwrap();
        assert_eq!(
            SectionPassage::new(frame, (0.0, 1.0).into(), section, steep).unwrap_err(),
            PassageError("passage termination planes must not cross the section")
        );
        assert!(
            PassageFrame::new(
                [0.0; 3].into(),
                [0.0, 0.0, -1.0],
                [1.0, 0.0, 0.0],
                [0.0, -1.0, 0.0]
            )
            .is_err()
        );
        assert!(
            PassageSection::new(
                [[1.0, -1.0], [1.0, 1.0], [-1.0, 1.0], [-1.0, -1.0]]
                    .into_iter()
                    .map(|p| PassageSectionVertex::new(p, 0.0).unwrap())
                    .collect()
            )
            .is_err()
        );
    }
}
