//! The section-recess family's public views (`quiddity.result.recognise_section_recesses` and
//! `build_section_recess_document`): the aggregate's section-recess projection
//! (`_project_result`) over one completed, reconciled run.
//!
//! Recognition and reconciliation run once (`super::take_inventory`); the native section
//! recesses (`discover_section_recesses`) are discovered in the same run, as Python's registry
//! runs them among the physical families. The accepted records of the legacy recess families
//! are then projected into the unified view, in Python's order: native records, prismatic
//! pockets a native record does not already cover (by body and constituent faces), passages,
//! edge-open prismatic and circular recesses, rectangular and round-bottom blind slots, and
//! finally pockets and channels, which must prove a corner notch or an open channel from their
//! source faces. The first projection of each body-local face region is kept
//! (`_unique_section_recesses`); every accepted source candidate no kept record covers is a
//! [`SectionRecessRefusal`]; and the accepted pockets' derived patterns are re-expressed over
//! the kept records' section midpoints where they reconstruct them.
//!
//! A value that cannot be published (Python's `LegacySectionProjectionError`) drops only that
//! candidate, which then appears as a refusal; any other Python `ValueError` (a lost body, an
//! inconsistent record) refuses the whole projection with Python's message
//! ([`SectionRecessFamilyError`]), as it refuses the whole recognition there.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use super::Context;
use super::blend_view::face_area;
use super::channels::Channel;
use super::cylindrical_channels::{self, empty};
use super::edge_open_circular::EdgeOpenCircularPocket;
use super::edge_open_prismatic::EdgeOpenPrismaticRecess;
use super::effective_surfaces::{EffectiveFaces, arc_length};
use super::evidence::common_valid_solid;
use super::graph::{face_vertices, is_planar, normal};
use super::passages::{PassageFrame, PassageSectionVertex, SectionPassage};
use super::placed::{Grid, Placed};
use super::pockets::Pocket;
use super::prismatic_pockets::PrismaticPocket;
use super::recess_patterns::{PocketPattern, recognise_pocket_patterns};
use super::reconcile::{self, Outcome};
use super::rectangular_blind_slots::RectangularBlindSlot;
use super::round_bottom_slots::RoundBottomBlindSlot;
use super::section_passages::{end_slab, probe_prism};
use super::section_recess::{
    ClosedSectionProfile, EndSurface, OpenSectionProfile, PlanarEndSurface, SectionEnd,
    SectionProfile, SectionRecess, SectionRecessArray, SectionRecessBodyRef,
    SectionRecessClassification, SectionRecessDocument, SectionRecessEnds, SectionRecessError,
    SectionRecessEvidence, SectionRecessFaceRef, SectionRecessGeometry, SectionRecessGrid,
    SectionRecessPattern, SectionRecessRefusal, open_profile_material_side,
};
use super::section_recess_discovery::discover_section_recesses;
use super::section_recess_geometry::{
    cylindrical_channel_geometry, has_physical_planar_floor, polygonal_shape,
};
use super::sections::{LocalFrame, PlanarSection, SectionVertex, V2};
use super::support_patches::{covered_patch, polygon_face, ruled_prism};
use super::{Inventory, take_inventory};
use crate::kernel::brep::{Edge, Face, Loop, Part};
use crate::kernel::geom::{self, Curve, Frame, Surface, V3};
use crate::kernel::py;
use crate::kernel::sampling::sample_edge;
use crate::kernel::sweep::extrude_face;

/// The projection's refusal, with Python's `ValueError` message: the reconciliation's, a native
/// discovery's, or the projection's own (an accepted record that lost its body, a value Python
/// builds outside its publication guard that fails its checks).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionRecessFamilyError(pub String);

impl fmt::Display for SectionRecessFamilyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SectionRecessFamilyError {}

impl From<SectionRecessError> for SectionRecessFamilyError {
    fn from(e: SectionRecessError) -> Self {
        SectionRecessFamilyError(e.0.to_string())
    }
}

impl From<reconcile::ReconcileError> for SectionRecessFamilyError {
    fn from(e: reconcile::ReconcileError) -> Self {
        SectionRecessFamilyError(e.0)
    }
}

/// The aggregate's section-recess fields (`RecognitionResult.section_recesses`,
/// `section_recess_refusals`, `section_recess_patterns`).
#[derive(Clone, Debug, PartialEq)]
pub struct SectionRecessProjection {
    pub section_recesses: Vec<SectionRecess>,
    pub refusals: Vec<SectionRecessRefusal>,
    pub patterns: Vec<SectionRecessPattern>,
}

internal_entry! {
/// Every accepted unified constant-section recess in *part* (`recognise_section_recesses`).
pub fn recognise_section_recesses(
    part: &Part,
) -> Result<Vec<SectionRecess>, SectionRecessFamilyError> {
    Ok(section_recess_projection(part)?.section_recesses)
}
}

internal_entry! {
/// The accepted recesses as one deterministic document (`build_section_recess_document`):
/// occurrence indices dense within it, body and face references into the part's complete
/// rosters.
pub fn build_section_recess_document(
    part: &Part,
) -> Result<SectionRecessDocument, SectionRecessFamilyError> {
    let result = section_recess_projection(part)?;
    let occurrences = result
        .section_recesses
        .iter()
        .enumerate()
        .map(|(index, record)| reindexed(record, index))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SectionRecessDocument::new(
        4,
        "result",
        (0..part.solids.len())
            .map(|index| SectionRecessBodyRef { index })
            .collect(),
        (0..part.faces.len())
            .map(|index| SectionRecessFaceRef { index })
            .collect(),
        occurrences,
        result.refusals,
        result.patterns,
    )?)
}
}

internal_entry! {
/// One recognition and reconciliation of *part* and its section-recess projection, locally
/// degraded where Python's aggregate retries so (`features::take_inventory`).
pub fn section_recess_projection(
    part: &Part,
) -> Result<SectionRecessProjection, SectionRecessFamilyError> {
    take_inventory(part, |ctx, inventory| {
        section_recess_projection_in(ctx, &inventory)
    })?
}
}

/// The section-recess projection of a run the caller keeps: the native records discovered on
/// *ctx* and the projection of *inventory*, that run's recognition and reconciliation
/// (`super::recognise` reads its section recesses so, from its one inventory run).
pub(crate) fn section_recess_projection_in(
    ctx: &Context<'_>,
    inventory: &Inventory,
) -> Result<SectionRecessProjection, SectionRecessFamilyError> {
    let surfaces = EffectiveFaces::new(ctx);
    let native = discover_section_recesses(ctx, &surfaces)?
        .into_iter()
        .map(|o| o.record)
        .collect();
    project(ctx, &surfaces, inventory, native)
}

/// `dataclasses.replace(record, index=index)`, which validates the record again.
fn reindexed(record: &SectionRecess, index: usize) -> Result<SectionRecess, SectionRecessError> {
    SectionRecess::new(
        index,
        record.body(),
        record.geometry().clone(),
        record.classification().clone(),
        record.evidence().clone(),
    )
}

// --- the run's accepted records -----------------------------------------------------------

/// Which accepted record a candidate is: its family's field and its index among the family's
/// candidates (Python keys the corner projections by the record's identity).
type Source = (&'static str, usize);

/// One accepted candidate's evidence (`EvidenceIndex.defining_of` / `constituent_of`).
#[derive(Clone, Debug)]
struct Faces {
    source: Source,
    defining: BTreeSet<usize>,
    constituent: BTreeSet<usize>,
}

/// The accepted candidates of every family, by field, in order.
struct Accepted<'i> {
    inventory: &'i Inventory,
    rejected: BTreeMap<&'static str, BTreeSet<usize>>,
}

impl<'i> Accepted<'i> {
    fn new(inventory: &'i Inventory) -> Self {
        let mut rejected: BTreeMap<&'static str, BTreeSet<usize>> = BTreeMap::new();
        for d in &inventory.dispositions {
            if d.outcome == Outcome::Rejected
                && let Some(field) = d.candidate.family.field()
            {
                rejected.entry(field).or_default().insert(d.candidate.index);
            }
        }
        Accepted {
            inventory,
            rejected,
        }
    }

    /// The accepted records of the family in *field*, each with its evidence.
    fn of<'r, R>(&self, field: &'static str, records: &'r [R]) -> Vec<(&'r R, Faces)> {
        let defining = &self.inventory.physical.defining[field];
        let constituent = &self.inventory.constituent[field];
        let rejected = self.rejected.get(field);
        records
            .iter()
            .enumerate()
            .filter(|(i, _)| rejected.is_none_or(|r| !r.contains(i)))
            .map(|(i, record)| {
                (
                    record,
                    Faces {
                        source: (field, i),
                        defining: defining[i].iter().copied().collect(),
                        constituent: constituent[i].iter().copied().collect(),
                    },
                )
            })
            .collect()
    }

    /// Only the evidence of the family's accepted candidates.
    fn faces<R>(&self, field: &'static str, records: &[R]) -> Vec<Faces> {
        self.of(field, records)
            .into_iter()
            .map(|(_, f)| f)
            .collect()
    }
}

fn sorted(faces: &BTreeSet<usize>) -> Vec<usize> {
    faces.iter().copied().collect()
}

fn owner_of(part: &Part, faces: &BTreeSet<usize>) -> Option<usize> {
    common_valid_solid(part, &sorted(faces))
}

// --- projection errors ------------------------------------------------------------------------

/// Why a projector issued nothing: a value the publication guard refused (Python's
/// `LegacySectionProjectionError`, the candidate alone is dropped) or a refusal of the whole
/// projection.
#[derive(Clone, Debug)]
enum Refusal {
    Publication,
    Whole(String),
}

impl From<SectionRecessError> for Refusal {
    fn from(e: SectionRecessError) -> Self {
        Refusal::Whole(e.0.to_string())
    }
}

type Projected<T> = Result<T, Refusal>;

/// `_publication_value`: any refusal while building a published value refuses only it.
fn publication<T, E>(value: Result<T, E>) -> Projected<T> {
    value.map_err(|_| Refusal::Publication)
}

fn whole<T>(message: &str) -> Projected<T> {
    Err(Refusal::Whole(message.to_string()))
}

fn axis_index(axis: &str) -> Option<usize> {
    ["x", "y", "z"].iter().position(|&a| a == axis)
}

fn flat_end(condition: &str) -> Result<SectionEnd, SectionRecessError> {
    SectionEnd::new(
        condition,
        EndSurface::Planar(PlanarEndSurface::new("plane", [0.0, 0.0])?),
    )
}

// --- the projection ---------------------------------------------------------------------------

/// `_project_result`'s section-recess fields over one run's accepted inventory.
fn project(
    ctx: &Context<'_>,
    surfaces: &EffectiveFaces<'_, '_>,
    inventory: &Inventory,
    native: Vec<SectionRecess>,
) -> Result<SectionRecessProjection, SectionRecessFamilyError> {
    let part = ctx.part;
    let physical = &inventory.physical;
    let accepted = Accepted::new(inventory);
    let native_regions: BTreeSet<(usize, Vec<usize>)> = native
        .iter()
        .map(|r| (r.body(), r.evidence().constituent_faces().to_vec()))
        .collect();

    let mut uncovered = Vec::new();
    for (record, faces) in accepted.of("prismatic_pockets", &physical.prismatic_pockets) {
        let Some(owner) = owner_of(part, &faces.defining) else {
            return Err(SectionRecessFamilyError(
                "accepted section feature lost its body authority".into(),
            ));
        };
        if !native_regions.contains(&(owner, sorted(&faces.constituent))) {
            uncovered.push((record, faces));
        }
    }
    let prismatic = project_records(&uncovered, |r, f, i| prismatic_pocket_recess(part, r, f, i))?;

    let mut uniform = Vec::new();
    uniform.extend(project_records(
        &accepted.of("section_passages", &physical.section_passages),
        |r, f, i| section_passage_recess(part, r, f, i).map(Some),
    )?);
    uniform.extend(project_records(
        &accepted.of(
            "edge_open_prismatic_recesses",
            &physical.edge_open_prismatic_recesses,
        ),
        |r, f, i| edge_open_prismatic_recess(part, r, f, i).map(Some),
    )?);
    uniform.extend(project_records(
        &accepted.of(
            "edge_open_circular_pockets",
            &physical.edge_open_circular_pockets,
        ),
        |r, f, i| edge_open_circular_recess(part, r, f, i).map(Some),
    )?);
    uniform.extend(project_records(
        &accepted.of("rectangular_blind_slots", &physical.rectangular_blind_slots),
        |r, f, i| rectangular_blind_slot_recess(part, r, f, i).map(Some),
    )?);
    uniform.extend(project_records(
        &accepted.of(
            "round_bottom_blind_slots",
            &physical.round_bottom_blind_slots,
        ),
        |r, f, i| round_bottom_blind_slot_recess(part, r, f, i).map(Some),
    )?);

    let mut legacy: Vec<(LegacyRecess, Faces)> = accepted
        .of("pockets", &physical.pockets)
        .into_iter()
        .map(|(r, f)| (LegacyRecess::pocket(r), f))
        .collect();
    legacy.extend(
        accepted
            .of("channels", &physical.channels)
            .into_iter()
            .map(|(r, f)| (LegacyRecess::channel(r), f)),
    );
    let mut corner = Vec::new();
    let mut projected_regions: BTreeMap<Source, (usize, Vec<usize>)> = BTreeMap::new();
    for (index, (record, faces)) in legacy.iter().enumerate() {
        match corner_pocket_recess(ctx, surfaces, record, faces, index) {
            Err(Refusal::Publication) | Ok(None) => {}
            Err(Refusal::Whole(message)) => return Err(SectionRecessFamilyError(message)),
            Ok(Some(projected)) => {
                projected_regions.insert(
                    faces.source,
                    (
                        projected.body(),
                        projected.evidence().constituent_faces().to_vec(),
                    ),
                );
                corner.push(projected);
            }
        }
    }

    let section_recesses = unique_section_recesses(
        native
            .into_iter()
            .chain(prismatic)
            .chain(uniform)
            .chain(corner),
    )?;

    // `RECESS_SOURCE_FAMILIES` in registry order.
    let sources = [
        accepted.faces("channels", &physical.channels),
        accepted.faces("rectangular_blind_slots", &physical.rectangular_blind_slots),
        accepted.faces(
            "round_bottom_blind_slots",
            &physical.round_bottom_blind_slots,
        ),
        accepted.faces("pockets", &physical.pockets),
        accepted.faces("prismatic_pockets", &physical.prismatic_pockets),
        accepted.faces(
            "edge_open_circular_pockets",
            &physical.edge_open_circular_pockets,
        ),
        accepted.faces(
            "edge_open_prismatic_recesses",
            &physical.edge_open_prismatic_recesses,
        ),
        accepted.faces("section_passages", &physical.section_passages),
    ];
    let mut refusals: Vec<SectionRecessRefusal> = Vec::new();
    for faces in sources.iter().flatten() {
        if !matching_recesses(part, faces, &section_recesses, &projected_regions).is_empty() {
            continue;
        }
        let Some(owner) = owner_of(part, &faces.defining) else {
            return Err(SectionRecessFamilyError(
                "accepted recess candidate lost body authority".into(),
            ));
        };
        let refusal = SectionRecessRefusal::new(
            owner,
            "unsupported_support_geometry",
            SectionRecessEvidence::new(sorted(&faces.defining), sorted(&faces.constituent))?,
        )?;
        if !refusals.contains(&refusal) {
            refusals.push(refusal);
        }
    }

    let pockets = accepted.of("pockets", &physical.pockets);
    let accepted_pockets: Vec<_> = pockets.iter().map(|(r, _)| (*r).clone()).collect();
    let patterns = section_patterns(
        part,
        &recognise_pocket_patterns(&accepted_pockets),
        &pockets,
        &section_recesses,
        &projected_regions,
    )?;
    Ok(SectionRecessProjection {
        section_recesses,
        refusals,
        patterns,
    })
}

/// `_project_recess_records`: each accepted record through its projector, a publication refusal
/// dropping that record only.
fn project_records<R>(
    records: &[(&R, Faces)],
    projector: impl Fn(&R, &Faces, usize) -> Projected<Option<SectionRecess>>,
) -> Result<Vec<SectionRecess>, SectionRecessFamilyError> {
    let mut projected = Vec::new();
    for (index, (record, faces)) in records.iter().enumerate() {
        match projector(record, faces, index) {
            Ok(Some(value)) => projected.push(value),
            Ok(None) | Err(Refusal::Publication) => {}
            Err(Refusal::Whole(message)) => return Err(SectionRecessFamilyError(message)),
        }
    }
    Ok(projected)
}

/// `_unique_section_recesses`: the first projection of each body-local face region, numbered
/// in order.
fn unique_section_recesses(
    records: impl IntoIterator<Item = SectionRecess>,
) -> Result<Vec<SectionRecess>, SectionRecessError> {
    let mut unique: Vec<SectionRecess> = Vec::new();
    let mut seen: BTreeSet<(usize, Vec<usize>)> = BTreeSet::new();
    for record in records {
        if seen.insert((
            record.body(),
            record.evidence().constituent_faces().to_vec(),
        )) {
            let index = unique.len();
            unique.push(reindexed(&record, index)?);
        }
    }
    Ok(unique)
}

/// `_matching_recesses`: the kept records a candidate's region projects to. A corner-projected
/// pocket or channel matches its own projection's region (with its defining faces among the
/// record's); any other candidate a record whose constituent faces hold all its faces.
fn matching_recesses<'s>(
    part: &Part,
    faces: &Faces,
    recesses: &'s [SectionRecess],
    projected_regions: &BTreeMap<Source, (usize, Vec<usize>)>,
) -> Vec<&'s SectionRecess> {
    let Some(owner) = owner_of(part, &faces.defining) else {
        return Vec::new();
    };
    if let Some(region) = projected_regions.get(&faces.source) {
        return recesses
            .iter()
            .filter(|item| {
                item.body() == region.0
                    && item.evidence().constituent_faces() == region.1.as_slice()
                    && item.body() == owner
                    && faces
                        .defining
                        .iter()
                        .all(|f| item.evidence().defining_faces().contains(f))
            })
            .collect();
    }
    let indices: BTreeSet<usize> = faces.defining.union(&faces.constituent).copied().collect();
    recesses
        .iter()
        .filter(|item| {
            item.body() == owner
                && !indices.is_empty()
                && indices
                    .iter()
                    .all(|f| item.evidence().constituent_faces().contains(f))
        })
        .collect()
}

// --- the projectors ---------------------------------------------------------------------------

fn evidence(
    defining: &BTreeSet<usize>,
    constituent: &BTreeSet<usize>,
) -> Projected<SectionRecessEvidence> {
    Ok(SectionRecessEvidence::new(
        sorted(defining),
        sorted(constituent),
    )?)
}

/// `_section_passage_recess`: one accepted passage, its two planar open ends as published.
fn section_passage_recess(
    part: &Part,
    record: &SectionPassage,
    faces: &Faces,
    index: usize,
) -> Projected<SectionRecess> {
    let Some(owner) = owner_of(part, &faces.defining) else {
        return whole("accepted section passage lost its body authority");
    };
    let boundary = &record.section.boundary;
    let section_shape = if boundary.iter().all(|v| v.bulge == 0.0) {
        polygonal_shape(&boundary.iter().map(|v| v.point).collect::<Vec<_>>())
    } else {
        "general"
    };
    let ends = SectionRecessEnds::new(
        SectionEnd::new(
            "open",
            EndSurface::Planar(PlanarEndSurface::new("plane", record.ends.low_gradient)?),
        )?,
        SectionEnd::new(
            "open",
            EndSurface::Planar(PlanarEndSurface::new("plane", record.ends.high_gradient)?),
        )?,
    )?;
    let profile = publication(ClosedSectionProfile::new("closed", boundary.clone()))?;
    let geometry = publication(SectionRecessGeometry::new(
        "section_recess",
        record.frame.clone(),
        record.run_interval,
        SectionProfile::Closed(profile),
        ends,
    ))?;
    Ok(SectionRecess::new(
        index,
        owner,
        geometry,
        SectionRecessClassification::new("passage", section_shape)?,
        evidence(&faces.defining, &faces.constituent)?,
    )?)
}

/// `_prismatic_pocket_recess`: one accepted prismatic pocket at the legacy publication grid,
/// only where an observed plane caps it at its published floor.
fn prismatic_pocket_recess(
    part: &Part,
    record: &PrismaticPocket,
    faces: &Faces,
    index: usize,
) -> Projected<Option<SectionRecess>> {
    let Some(owner) = owner_of(part, &faces.defining) else {
        return whole("accepted prismatic pocket lost its body authority");
    };
    let geometry = publication(legacy_section_geometry(record).ok_or(()))?;
    let interval = geometry.run_interval();
    let published_floor = if record.open_sign == 1 {
        interval.0
    } else {
        interval.1
    };
    if !has_physical_planar_floor(
        part,
        &faces.defining,
        &faces.constituent,
        &record.axis,
        record.open_sign,
        published_floor,
    ) {
        return Ok(None);
    }
    let boundary = geometry.profile().boundary();
    let mut section_shape = polygonal_shape(&boundary.iter().map(|v| v.point).collect::<Vec<_>>());
    if boundary.len() != record.section.len() {
        section_shape = "polygonal";
    }
    Ok(Some(SectionRecess::new(
        index,
        owner,
        geometry,
        SectionRecessClassification::new("pocket", section_shape)?,
        evidence(&faces.defining, &faces.constituent)?,
    )?))
}

/// `_legacy_section_recess`: a projected geometry with the record's own evidence.
fn legacy_section_recess(
    part: &Part,
    faces: &Faces,
    index: usize,
    geometry: SectionRecessGeometry,
    feature_kind: &str,
    section_shape: &str,
) -> Projected<SectionRecess> {
    let Some(owner) = owner_of(part, &faces.defining) else {
        return whole("accepted legacy section recess lost its body authority");
    };
    Ok(SectionRecess::new(
        index,
        owner,
        geometry,
        SectionRecessClassification::new(feature_kind, section_shape)?,
        evidence(&faces.defining, &faces.constituent)?,
    )?)
}

/// What the corner and open-channel proofs read of a pocket or channel.
#[derive(Clone, Copy, Debug)]
struct LegacyRecess {
    width_axis: char,
    long_axis: char,
    open_sign: i32,
    /// `Some(edge_anchored)` for a pocket, `None` for a channel.
    edge_anchored: Option<bool>,
}

impl LegacyRecess {
    fn pocket(p: &Pocket) -> Self {
        LegacyRecess {
            width_axis: p.width_axis,
            long_axis: p.long_axis,
            open_sign: p.open_sign,
            edge_anchored: Some(p.edge_anchored),
        }
    }

    fn channel(c: &Channel) -> Self {
        LegacyRecess {
            width_axis: c.width_axis,
            long_axis: c.long_axis,
            open_sign: c.open_sign,
            edge_anchored: None,
        }
    }

    fn depth_axis(&self) -> char {
        ['x', 'y', 'z']
            .into_iter()
            .find(|&a| a != self.width_axis && a != self.long_axis)
            .expect("three axes")
    }
}

/// `_corner_pocket_recess`: a pocket or channel as an open channel (planar, or ending on a
/// cylinder) or, for an edge-anchored pocket, a corner notch, each proved from its source faces;
/// `None` where nothing is proved.
fn corner_pocket_recess(
    ctx: &Context<'_>,
    surfaces: &EffectiveFaces<'_, '_>,
    record: &LegacyRecess,
    faces: &Faces,
    index: usize,
) -> Projected<Option<SectionRecess>> {
    let part = ctx.part;
    if record.edge_anchored != Some(true) {
        let Some(channel) =
            prove_open_channel(ctx, surfaces, &faces.defining, &faces.constituent, record)
        else {
            let Some(curved) = cylindrical_channels::prove(
                ctx,
                surfaces,
                &faces.defining,
                &faces.constituent,
                &record.long_axis.to_string(),
                &record.width_axis.to_string(),
                record.open_sign,
            ) else {
                return Ok(None);
            };
            // Source anatomy can be valid while its serialized geometry exceeds the public
            // error budget. Refuse this candidate only.
            let Ok(geometry) = cylindrical_channel_geometry(&curved) else {
                return Ok(None);
            };
            let supports: BTreeSet<usize> = curved.supports.iter().copied().collect();
            return Ok(Some(SectionRecess::new(
                index,
                curved.owner,
                geometry,
                SectionRecessClassification::new("channel", "rectangular")?,
                evidence(&faces.defining, &supports)?,
            )?));
        };
        let boundary = proof_vertices(&channel.boundary)?;
        let geometry = publication(principal_open_geometry(
            &channel.axis,
            channel.run_interval,
            1,
            &boundary,
            None,
        ))?;
        let geometry = SectionRecessGeometry::new(
            "section_recess",
            geometry.frame().clone(),
            geometry.placed_run_interval(),
            geometry.profile().clone(),
            SectionRecessEnds::new(flat_end("open")?, flat_end("open")?)?,
        )?;
        let projected =
            legacy_section_recess(part, faces, index, geometry, "channel", "rectangular")?;
        let mut constituent: BTreeSet<usize> = projected
            .evidence()
            .constituent_faces()
            .iter()
            .copied()
            .collect();
        constituent.extend(channel.aperture_faces.iter().copied());
        return Ok(Some(SectionRecess::new(
            projected.index(),
            projected.body(),
            projected.geometry().clone(),
            projected.classification().clone(),
            SectionRecessEvidence::new(
                projected.evidence().defining_faces().to_vec(),
                sorted(&constituent),
            )?,
        )?));
    }
    let depth_axis = record.depth_axis().to_string();
    let Some(proof) = prove_corner_section(ctx, &faces.defining, &depth_axis) else {
        return Ok(None);
    };
    let boundary = proof_vertices(&proof.boundary)?;
    let geometry = publication(principal_open_geometry(
        &depth_axis,
        proof.run_interval,
        proof.open_sign,
        &boundary,
        None,
    ))?;
    Ok(Some(legacy_section_recess(
        part,
        faces,
        index,
        geometry,
        "edge_open_recess",
        "polygonal",
    )?))
}

/// Private proof vertices: keep world coordinates exact until the local frame is chosen.
fn proof_vertices(points: &[V2]) -> Projected<Vec<SectionVertex>> {
    points
        .iter()
        .map(|&p| SectionVertex::new(p, 0.0).map_err(|e| Refusal::Whole(e.0.to_string())))
        .collect()
}

/// `_principal_local_point`: transverse offsets of a principal run in `LocalFrame.principal`
/// coordinates, on the four-decimal grid.
fn principal_local_point(axis: char, offsets: &[(char, f64)]) -> V2 {
    let transverse = match axis {
        'x' => ['y', 'z'],
        'y' => ['z', 'x'],
        _ => ['x', 'y'],
    };
    let get = |key: char| {
        offsets
            .iter()
            .rev()
            .find(|(k, _)| *k == key)
            .map_or(0.0, |(_, v)| *v)
    };
    [
        py::round_to(get(transverse[0]), 4),
        py::round_to(get(transverse[1]), 4),
    ]
}

/// The published run interval of a slot centred at *at* along *axis*.
fn slot_interval(axis: char, at: V3, length: f64) -> Placed<(f64, f64)> {
    let along = at[usize::from(axis as u8 - b'x')];
    Placed::new(
        (along - length / 2.0, along + length / 2.0),
        Grid::CleanDecimals(3),
    )
}

/// `_rectangular_blind_slot_recess`.
fn rectangular_blind_slot_recess(
    part: &Part,
    record: &RectangularBlindSlot,
    faces: &Faces,
    index: usize,
) -> Projected<SectionRecess> {
    let (half_width, half_depth) = (record.width / 2.0, record.depth / 2.0);
    let opening = f64::from(record.depth_sign) * half_depth;
    let floor = -opening;
    let chain = [
        (-half_width, opening),
        (-half_width, floor),
        (half_width, floor),
        (half_width, opening),
    ]
    .iter()
    .map(|&(width, depth)| {
        SectionVertex::new(
            principal_local_point(
                record.axis,
                &[(record.width_axis, width), (record.depth_axis, depth)],
            ),
            0.0,
        )
        .map_err(|e| Refusal::Whole(e.0.to_string()))
    })
    .collect::<Projected<Vec<_>>>()?;
    let geometry = publication(principal_open_geometry(
        &record.axis.to_string(),
        slot_interval(record.axis, *record.at, record.length),
        record.open_sign,
        &chain,
        Some(*record.at),
    ))?;
    legacy_section_recess(
        part,
        faces,
        index,
        geometry,
        "edge_open_recess",
        "rectangular",
    )
}

/// `_round_bottom_blind_slot_recess`: its two quarter arcs as bulges.
fn round_bottom_blind_slot_recess(
    part: &Part,
    record: &RoundBottomBlindSlot,
    faces: &Faces,
    index: usize,
) -> Projected<SectionRecess> {
    let half_width = (record.flat_width + 2.0 * record.radius) / 2.0;
    let half_flat = record.flat_width / 2.0;
    let half_depth = record.radius / 2.0;
    let opening = f64::from(record.depth_sign) * half_depth;
    let floor = -opening;
    let point = |width: f64, depth: f64| {
        principal_local_point(
            record.axis,
            &[(record.width_axis, width), (record.depth_axis, depth)],
        )
    };
    let width_vector = point(1.0, 0.0);
    let depth_vector = point(0.0, 1.0);
    let determinant = width_vector[0] * depth_vector[1] - width_vector[1] * depth_vector[0];
    let orientation = if determinant > 0.0 { 1.0 } else { -1.0 };
    let arc_bulge = py::round_to(
        (f64::from(record.depth_sign) * orientation * std::f64::consts::PI / 8.0).tan(),
        12,
    );
    let chain = [
        (point(-half_width, opening), arc_bulge),
        (point(-half_flat, floor), 0.0),
        (point(half_flat, floor), arc_bulge),
        (point(half_width, opening), 0.0),
    ]
    .iter()
    .map(|&(p, bulge)| SectionVertex::new(p, bulge).map_err(|e| Refusal::Whole(e.0.to_string())))
    .collect::<Projected<Vec<_>>>()?;
    let geometry = publication(principal_open_geometry(
        &record.axis.to_string(),
        slot_interval(record.axis, *record.at, record.length),
        record.open_sign,
        &chain,
        Some(*record.at),
    ))?;
    legacy_section_recess(part, faces, index, geometry, "edge_open_recess", "general")
}

/// `_edge_open_prismatic_recess`: its wall chain as published.
fn edge_open_prismatic_recess(
    part: &Part,
    record: &EdgeOpenPrismaticRecess,
    faces: &Faces,
    index: usize,
) -> Projected<SectionRecess> {
    let Some(owner) = owner_of(part, &faces.defining) else {
        return whole("accepted edge-open prismatic recess lost its body authority");
    };
    let boundary = record
        .section
        .wall_chain
        .iter()
        .map(|&p| SectionVertex::new(*p, 0.0).map_err(|e| Refusal::Whole(e.0.to_string())))
        .collect::<Projected<Vec<_>>>()?;
    let geometry = publication(principal_open_geometry(
        &record.axis.to_string(),
        record.run_interval.map(|[lo, hi]| (lo, hi)),
        record.open_sign,
        &boundary,
        None,
    ))?;
    let section_shape = if boundary.len() == 4 {
        polygonal_shape(&boundary.iter().map(|v| v.point).collect::<Vec<_>>())
    } else {
        "polygonal"
    };
    Ok(SectionRecess::new(
        index,
        owner,
        geometry,
        SectionRecessClassification::new("edge_open_recess", section_shape)?,
        evidence(&faces.defining, &faces.constituent)?,
    )?)
}

/// `_edge_open_circular_recess`: its line and arc segments as bulged vertices.
fn edge_open_circular_recess(
    part: &Part,
    record: &EdgeOpenCircularPocket,
    faces: &Faces,
    index: usize,
) -> Projected<SectionRecess> {
    let Some(owner) = owner_of(part, &faces.defining) else {
        return whole("accepted edge-open circular recess lost its body authority");
    };
    let segments = &record.section.segments;
    let mut vertices = Vec::new();
    for segment in segments {
        let bulge = if segment.kind == "line" {
            0.0
        } else {
            let Some(sweep) = segment.sweep else {
                return whole("arc segment requires center, radius and sweep");
            };
            py::round_to((sweep / 4.0).tan(), 12)
        };
        vertices.push(
            SectionVertex::new(*segment.start, bulge)
                .map_err(|e| Refusal::Whole(e.0.to_string()))?,
        );
    }
    let Some(last) = segments.last() else {
        return whole("an open circular section requires four physical segments");
    };
    vertices.push(SectionVertex::new(*last.end, 0.0).map_err(|e| Refusal::Whole(e.0.to_string()))?);
    let geometry = publication(principal_open_geometry(
        &record.axis.to_string(),
        record.run_interval.map(|[lo, hi]| (lo, hi)),
        record.open_sign,
        &vertices,
        None,
    ))?;
    Ok(SectionRecess::new(
        index,
        owner,
        geometry,
        SectionRecessClassification::new("edge_open_recess", "obround")?,
        evidence(&faces.defining, &faces.constituent)?,
    )?)
}

// --- published geometry -----------------------------------------------------------------------

/// Python's tuple order of two vertex chains: vertex by vertex, each by point then bulge.
fn chain_less(a: &[PassageSectionVertex], b: &[PassageSectionVertex]) -> bool {
    let flat = |s: &[PassageSectionVertex]| -> Vec<f64> {
        s.iter()
            .flat_map(|v| [v.point[0], v.point[1], v.bulge])
            .collect()
    };
    py::tuple_order(&flat(a), &flat(b)) == std::cmp::Ordering::Less
}

/// `_canonical_open_profile`: the chain in whichever direction orders first, its opening from
/// its end to its start.
fn canonical_open_profile(
    boundary: Vec<PassageSectionVertex>,
) -> Result<OpenSectionProfile, SectionRecessError> {
    let n = boundary.len();
    let reversed: Vec<PassageSectionVertex> = (0..n)
        .map(|index| PassageSectionVertex {
            point: boundary[n - 1 - index].point,
            bulge: if index < n - 1 {
                py::without_negative_zero(-boundary[n - 2 - index].bulge)
            } else {
                0.0
            },
        })
        .collect();
    let canonical = if chain_less(&reversed, &boundary) {
        reversed
    } else {
        boundary
    };
    let side = open_profile_material_side(&canonical)?;
    let opening = [canonical[n - 1].point, canonical[0].point];
    OpenSectionProfile::new("open", canonical, opening, Some(side))
}

/// `_principal_open_geometry`: an open wall chain across a principal run, in the run's
/// `LocalFrame.principal` coordinates about its box centre (or, given a *placement*, already
/// in them about it), capped at the end away from *open_sign*.
fn principal_open_geometry(
    axis: &str,
    run_interval: Placed<(f64, f64)>,
    open_sign: i32,
    boundary: &[SectionVertex],
    placement: Option<V3>,
) -> Result<SectionRecessGeometry, SectionRecessError> {
    let Some(index) = axis_index(axis) else {
        return Err(SectionRecessError("substring not found"));
    };
    let transverse: Vec<usize> = (0..3).filter(|&i| i != index).collect();
    let points: Vec<V2> = boundary.iter().map(|v| v.point).collect();
    let center2: V2 = [0, 1].map(|i| {
        let low = points.iter().map(|p| p[i]).fold(f64::INFINITY, f64::min);
        let high = points
            .iter()
            .map(|p| p[i])
            .fold(f64::NEG_INFINITY, f64::max);
        0.5 * (low + high)
    });
    let mut center3 = [0.0; 3];
    center3[transverse[0]] = center2[0];
    center3[transverse[1]] = center2[1];
    if let Some(at) = placement {
        center3 = at;
    }
    let frame = LocalFrame::principal(axis, center3)?;
    let order = if axis == "y" { [1, 0] } else { [0, 1] };
    let local = boundary
        .iter()
        .map(|vertex| match placement {
            Some(_) => PassageSectionVertex::new(vertex.point, vertex.bulge),
            None => PassageSectionVertex::new(
                order.map(|i| super::policy::round_measurement(vertex.point[i] - center2[i], 4)),
                if axis == "y" {
                    -vertex.bulge
                } else {
                    vertex.bulge
                },
            ),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let frame = PassageFrame::new(
        Placed::new(frame.origin, Grid::CleanDecimals(3)),
        frame.run,
        frame.u,
        frame.v,
    )?;
    let (low, high) = if open_sign == -1 {
        ("open", "capped")
    } else {
        ("capped", "open")
    };
    SectionRecessGeometry::new(
        "section_recess",
        frame,
        run_interval,
        SectionProfile::Open(canonical_open_profile(local)?),
        SectionRecessEnds::new(flat_end(low)?, flat_end(high)?)?,
    )
}

/// Python's `round(x)`: to the nearest integer, ties to even.
fn round_int(value: f64) -> i64 {
    value.round_ties_even() as i64
}

/// `_normalise_published_section`: a published three-decimal loop on its integer grid, with
/// repeated and backtracking vertices removed, refused where that collapses it, leaves a
/// repeated vertex or moves a published vertex more than the occurrence tolerance.
fn normalise_published_section(section: &[V2]) -> Option<PlanarSection> {
    if section
        .iter()
        .any(|p| !p[0].is_finite() || !p[1].is_finite())
    {
        return None;
    }
    let mut vertices: Vec<[i64; 2]> = section
        .iter()
        .map(|p| [round_int(p[0] * 1000.0), round_int(p[1] * 1000.0)])
        .collect();
    let mut changed = true;
    while changed && vertices.len() >= 3 {
        changed = false;
        let n = vertices.len();
        for index in 0..n {
            let point = vertices[index];
            let previous = vertices[(index + n - 1) % n];
            let following = vertices[(index + 1) % n];
            let incoming = [point[0] - previous[0], point[1] - previous[1]];
            let outgoing = [following[0] - point[0], following[1] - point[1]];
            let backtrack = incoming[0] * outgoing[1] == incoming[1] * outgoing[0]
                && incoming[0] * outgoing[0] + incoming[1] * outgoing[1] < 0;
            if point == previous || point == following || backtrack {
                vertices.remove(index);
                changed = true;
                break;
            }
        }
    }
    let distinct: BTreeSet<[i64; 2]> = vertices.iter().copied().collect();
    if distinct.len() < 3 || distinct.len() != vertices.len() {
        return None;
    }
    let normalized = PlanarSection::new(
        vertices
            .iter()
            .map(|p| SectionVertex::new([p[0] as f64 / 1000.0, p[1] as f64 / 1000.0], 0.0))
            .collect::<Result<Vec<_>, _>>()
            .ok()?,
    )
    .ok()?;
    let boundary = normalized.boundary();
    for original in section {
        let mut nearest = f64::INFINITY;
        for (index, vertex) in boundary.iter().enumerate() {
            let start = vertex.point;
            let end = boundary[(index + 1) % boundary.len()].point;
            let direction = [end[0] - start[0], end[1] - start[1]];
            let fraction = (((original[0] - start[0]) * direction[0]
                + (original[1] - start[1]) * direction[1])
                / (direction[0].powi(2) + direction[1].powi(2)))
            .clamp(0.0, 1.0);
            let foot = [
                start[0] + fraction * direction[0],
                start[1] + fraction * direction[1],
            ];
            nearest = nearest.min(py::dist(original, &foot));
        }
        if nearest > OCCURRENCE_TOL {
            return None;
        }
    }
    Some(normalized)
}

/// `_sections._OCCURRENCE_TOL`: the whole-occurrence publication displacement allowance.
const OCCURRENCE_TOL: f64 = 2e-3;

/// `legacy_section_geometry` for a prismatic pocket: its published grid geometry without an
/// exact-centroid intermediate. `None` is Python's `LegacySectionProjectionError` (the message
/// is never published: the candidate becomes a refusal).
fn legacy_section_geometry(record: &PrismaticPocket) -> Option<SectionRecessGeometry> {
    let index = axis_index(&record.axis)?;
    let span = record.depth;
    if !span.is_finite() || span <= 0.0 || record.at.iter().any(|v| !v.is_finite()) {
        return None;
    }
    if record.sides != record.section.len() || record.sides < 3 {
        return None;
    }
    if !matches!(record.open_sign, -1 | 1) {
        return None;
    }
    let raw = normalise_published_section(&record.section_points())?;
    let transverse: Vec<usize> = (0..3).filter(|&i| i != index).collect();
    let centroid = raw.centroid();
    if (0..2).any(|i| (centroid[i] - record.at[transverse[i]]).abs() > 0.0008) {
        return None;
    }
    let grid = centroid.map(|v| round_int(v * 1000.0));
    let (mut origin, mut centre) = ([0.0; 3], [0.0; 3]);
    for i in 0..2 {
        origin[transverse[i]] = grid[i] as f64 / 1000.0;
        centre[transverse[i]] = centroid[i];
    }
    let frame = LocalFrame::principal(&record.axis, origin).ok()?;
    let order = if record.axis == "y" { [1, 0] } else { [0, 1] };
    let local = PlanarSection::new(
        raw.boundary()
            .iter()
            .map(|vertex| {
                SectionVertex::new(
                    order.map(|i| (round_int(vertex.point[i] * 1000.0) - grid[i]) as f64 / 1000.0),
                    0.0,
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .ok()?,
    )
    .ok()?;
    let along = record.at[index];
    let interval = Placed::new(
        (along - span / 2.0, along + span / 2.0),
        Grid::CleanDecimals(3),
    );
    let boundary = local
        .boundary()
        .iter()
        .map(|v| PassageSectionVertex::new(v.point, 0.0))
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let ends = SectionRecessEnds::new(
        flat_end(if record.open_sign == 1 {
            "capped"
        } else {
            "open"
        })
        .ok()?,
        flat_end(if record.open_sign == -1 {
            "capped"
        } else {
            "open"
        })
        .ok()?,
    )
    .ok()?;
    SectionRecessGeometry::new(
        "section_recess",
        PassageFrame::new(
            Placed::from_parts(frame.origin, centre, Grid::CleanDecimals(3)),
            frame.run,
            frame.u,
            frame.v,
        )
        .ok()?,
        interval,
        SectionProfile::Closed(ClosedSectionProfile::new("closed", boundary).ok()?),
        ends,
    )
    .ok()
}

// --- source-face proofs -----------------------------------------------------------------------

/// A face's vertex box (`_bounds`): low and high along each axis.
fn vertex_bounds(part: &Part, face: usize) -> [(f64, f64); 3] {
    let points = face_vertices(part, face);
    [0, 1, 2].map(|i| {
        points
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                (lo.min(p[i]), hi.max(p[i]))
            })
    })
}

/// A proved corner notch (`CornerSectionProof`).
struct CornerSectionProof {
    run_interval: Placed<(f64, f64)>,
    open_sign: i32,
    boundary: Vec<V2>,
}

/// `prove_corner_section`: three mutually adjacent rectangular faces, one per principal normal,
/// whose two walls face into the removed quadrant from a floor across *axis*, the walls
/// meeting a planar exterior mouth, and the whole floor rectangle swept to the mouth and just
/// past it holding no material. Only the two physical wall segments are returned.
fn prove_corner_section(
    ctx: &Context<'_>,
    nodes: &BTreeSet<usize>,
    axis: &str,
) -> Option<CornerSectionProof> {
    let part = ctx.part;
    let run = axis_index(axis)?;
    if nodes.len() != 3 {
        return None;
    }
    let list = sorted(nodes);
    let owner = common_valid_solid(part, &list)?;
    for (i, &first) in list.iter().enumerate() {
        let neighbours = part.neighbours(first);
        if list[i + 1..].iter().any(|s| !neighbours.contains(s)) {
            return None;
        }
    }
    // One rectangular planar face per principal normal, bounded by physical straight edges.
    let mut rectangles: [Option<[(f64, f64); 3]>; 3] = [None; 3];
    let mut normals = [0.0; 3];
    let mut by_axis = [0usize; 3];
    for &node in &list {
        if !is_planar(part, node) {
            return None;
        }
        let n = normal(part, node)?;
        let fixed = (0..3).fold(
            0,
            |best, i| if n[i].abs() > n[best].abs() { i } else { best },
        );
        if n[fixed].abs() < 1.0 - 1e-8 || rectangles[fixed].is_some() {
            return None;
        }
        let edges = part.face_edges(node);
        let points = face_vertices(part, node);
        if edges.len() != 4
            || points.len() != 4
            || edges
                .iter()
                .any(|&e| !matches!(part.edges[e].curve, Curve::Line { .. }))
        {
            return None;
        }
        let bounds = vertex_bounds(part, node);
        let varying: Vec<usize> = (0..3).filter(|&i| i != fixed).collect();
        let largest = bounds
            .iter()
            .map(|(lo, hi)| hi - lo)
            .fold(f64::NEG_INFINITY, f64::max);
        let tolerance = f64::max(1e-7, largest * 1e-7);
        if bounds[fixed].1 - bounds[fixed].0 > tolerance {
            return None;
        }
        if varying
            .iter()
            .any(|&i| bounds[i].1 - bounds[i].0 <= tolerance)
        {
            return None;
        }
        // Four lines alone can bound a trapezoid. Require each actual corner of the rectangle.
        let pick = |(lo, hi): (f64, f64), k: usize| if k == 0 { lo } else { hi };
        for a in 0..2 {
            for b in 0..2 {
                let target = [pick(bounds[varying[0]], a), pick(bounds[varying[1]], b)];
                if !points
                    .iter()
                    .any(|p| (0..2).all(|j| (p[varying[j]] - target[j]).abs() <= tolerance))
                {
                    return None;
                }
            }
        }
        let area = (bounds[varying[0]].1 - bounds[varying[0]].0)
            * (bounds[varying[1]].1 - bounds[varying[1]].0);
        if (face_area(part, node) - area).abs() > area * 1e-6 {
            return None;
        }
        rectangles[fixed] = Some(bounds);
        normals[fixed] = n[fixed];
        by_axis[fixed] = node;
    }
    let rectangles = rectangles.map(|r| r.expect("three distinct normals"));
    let others: Vec<usize> = (0..3).filter(|&i| i != run).collect();
    let (u, v) = (others[0], others[1]);
    let (floor, first, second) = (rectangles[run], rectangles[u], rectangles[v]);
    let largest = rectangles
        .iter()
        .flat_map(|b| b.iter().map(|(lo, hi)| hi - lo))
        .fold(f64::NEG_INFINITY, f64::max);
    let tolerance = f64::max(1e-7, largest * 1e-7);
    let same_span = |l: (f64, f64), r: (f64, f64)| {
        (l.0 - r.0).abs() <= tolerance && (l.1 - r.1).abs() <= tolerance
    };
    if !(same_span(first[run], second[run])
        && same_span(first[v], floor[v])
        && same_span(second[u], floor[u]))
    {
        return None;
    }
    let (low, high) = first[run];
    let sign = if normals[run] > 0.0 { 1 } else { -1 };
    if (floor[run].0 - if sign == 1 { low } else { high }).abs() > tolerance {
        return None;
    }
    let mouth = if sign == 1 { high } else { low };
    for wall_axis in [u, v] {
        let open = part.neighbours(by_axis[wall_axis]).into_iter().any(|nb| {
            is_planar(part, nb)
                && normal(part, nb).is_some_and(|n| n[run] * f64::from(sign) > 1.0 - 1e-8)
                && face_vertices(part, nb)
                    .iter()
                    .all(|p| (p[run] - mouth).abs() <= tolerance)
        });
        if !open {
            return None; // no planar exterior mouth: a cap or treatment is not proved open
        }
    }
    // Each wall's outward normal must point INTO the removed quadrant. This rejects a boss
    // corner even though its three rectangular faces have the same incidence and extents.
    let pick = |(lo, hi): (f64, f64), low_end: bool| if low_end { lo } else { hi };
    let (first_at, second_at) = (first[u].0, second[v].0);
    if (first_at - pick(floor[u], normals[u] > 0.0)).abs() > tolerance {
        return None;
    }
    if (second_at - pick(floor[v], normals[v] > 0.0)).abs() > tolerance {
        return None;
    }
    let far_u = pick(floor[u], normals[u] <= 0.0);
    let far_v = pick(floor[v], normals[v] <= 0.0);
    // The three faces alone do not exclude a suspended same-body obstruction. Sweep the
    // independently proved whole floor rectangle, not a diagonal closure of the L chain.
    let centre = floor.map(|(lo, hi)| (lo + hi) / 2.0);
    let frame = LocalFrame::principal(axis, centre).ok()?;
    let local_axes = if axis == "y" { [v, u] } else { [u, v] };
    let [half_u, half_v] = local_axes.map(|i| (floor[i].1 - floor[i].0) / 2.0);
    let section = PlanarSection::polygon(&[
        [-half_u, -half_v],
        [half_u, -half_v],
        [half_u, half_v],
        [-half_u, half_v],
    ])
    .ok()?;
    let thickness = f64::max(
        f64::max(2e-5, f64::max(1.0, high - low) * 1e-4),
        (half_u * half_u + half_v * half_v).sqrt() * 1e-4,
    );
    if !empty(
        ctx,
        owner,
        probe_prism(&frame, (low, high), &section).as_ref(),
    ) || !empty(
        ctx,
        owner,
        end_slab(&frame, mouth, f64::from(sign), thickness, &section).as_ref(),
    ) {
        return None;
    }
    Some(CornerSectionProof {
        run_interval: Placed::new((low, high), Grid::CleanDecimals(3)),
        open_sign: sign,
        boundary: vec![[first_at, far_v], [first_at, second_at], [far_u, second_at]],
    })
}

/// A proved two-ended open channel (`OpenChannelProof`).
struct OpenChannelProof {
    axis: String,
    run_interval: Placed<(f64, f64)>,
    boundary: Vec<V2>,
    aperture_faces: Vec<usize>,
}

/// `_supports`: whether the constituent planes facing *sign* along *axis* at *at* cover the
/// rectangle of *bounds* across it, completely or with individually proved native circular
/// apertures; the apertures' cylinders are added to *aperture_faces*.
#[allow(clippy::too_many_arguments)]
fn supports(
    ctx: &Context<'_>,
    surfaces: &EffectiveFaces<'_, '_>,
    nodes: &BTreeSet<usize>,
    bounds: &[(f64, f64); 3],
    axis: usize,
    at: f64,
    sign: i32,
    aperture_faces: &mut BTreeSet<usize>,
) -> bool {
    let part = ctx.part;
    let transverse: Vec<usize> = (0..3).filter(|&i| i != axis).collect();
    let pick = |(lo, hi): (f64, f64), k: usize| if k == 0 { lo } else { hi };
    let corners: Vec<V3> = [(0, 0), (0, 1), (1, 1), (1, 0)]
        .iter()
        .map(|&(a, b)| {
            let mut xyz = [0.0; 3];
            xyz[axis] = at;
            xyz[transverse[0]] = pick(bounds[transverse[0]], a);
            xyz[transverse[1]] = pick(bounds[transverse[1]], b);
            xyz
        })
        .collect();
    let Some(patch) = polygon_face(&corners) else {
        return false;
    };
    let mut found = Vec::new();
    for &node in nodes {
        if !is_planar(part, node) {
            continue;
        }
        let Some(n) = normal(part, node) else {
            continue;
        };
        if n[axis] * f64::from(sign) < 1.0 - 1e-8 {
            continue;
        }
        let (lo, hi) = vertex_bounds(part, node)[axis];
        if f64::max((lo - at).abs(), (hi - at).abs()) > 1e-6 {
            continue;
        }
        found.push(node);
    }
    let sources: Vec<(&Part, usize)> = found.iter().map(|&n| (part, n)).collect();
    if covered_patch((&patch, 0), &sources) {
        return true;
    }
    let apertures: Vec<(Part, usize)> = found
        .iter()
        .flat_map(|&node| support_apertures(ctx, surfaces, node, &patch))
        .collect();
    let mut explained = sources;
    explained.extend(apertures.iter().map(|(disk, _)| (disk, 0)));
    if !covered_patch((&patch, 0), &explained) {
        return false;
    }
    aperture_faces.extend(apertures.iter().map(|&(_, cylinder)| cylinder));
    true
}

/// The least distance from a disk (*centre*, *radius*, in the plane of *edges*) to *edges*,
/// each read by its samples: 0 where an edge enters the disk.
fn disk_distance(part: &Part, edges: &[usize], centre: V3, radius: f64) -> f64 {
    let mut nearest = f64::INFINITY;
    for &e in edges {
        for pair in part.edges[e].samples.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let ab = geom::sub(b, a);
            let length = geom::dot(ab, ab);
            let t = if length > 0.0 {
                (geom::dot(geom::sub(centre, a), ab) / length).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let foot = geom::add(a, geom::scale(ab, t));
            nearest = nearest.min((geom::dist(foot, centre) - radius).max(0.0));
        }
    }
    nearest
}

/// A one-face part: the planar disk bounded by the closed circle *curve* through *start*
/// (`Face(wire)`).
fn disk_part(curve: &Curve, start: V3) -> Option<Part> {
    let &Curve::Circle { frame, .. } = curve else {
        return None;
    };
    let edge = Edge {
        samples: sample_edge(curve, start, start, true, true),
        curve: curve.clone(),
        start,
        end: start,
        vertices: (0, 0),
        same_sense: true,
    };
    let face = Face {
        surface: Surface::Plane { frame },
        reversed: false,
        loops: vec![Loop {
            edges: vec![(0, true)],
            vertex: None,
        }],
        solid: None,
        pcurves: Vec::new(),
    };
    Some(Part::new(vec![face], vec![edge], Vec::new()))
}

/// `proved_support_apertures` (`_aperture` per inner wire): each circle a planar *support*
/// is holed by strictly inside *patch*, explained by the native bore through it; the disk
/// stands in for the missing support, with the bore's face.
fn support_apertures(
    ctx: &Context<'_>,
    surfaces: &EffectiveFaces<'_, '_>,
    support: usize,
    patch: &Part,
) -> Vec<(Part, usize)> {
    let part = ctx.part;
    if !is_planar(part, support) {
        return Vec::new();
    }
    let (Some(support_normal), Some(outer)) = (normal(part, support), part.outer_loop(support))
    else {
        return Vec::new();
    };
    let outer_edges = part.outer_edges(support);
    let patch_edges = patch.face_edges(0);
    let mut results = Vec::new();
    for (index, wire) in part.faces[support].loops.iter().enumerate() {
        if index == outer || wire.edges.len() != 1 {
            continue;
        }
        let edge = wire.edges[0].0;
        let e = &part.edges[edge];
        let Curve::Circle { frame, radius } = e.curve else {
            continue;
        };
        let Some(disk) = disk_part(&e.curve, e.start) else {
            continue;
        };
        if disk_distance(part, &outer_edges, frame.origin, radius) <= 1e-6
            || disk_distance(patch, &patch_edges, frame.origin, radius) <= 1e-6
            || !covered_patch((&disk, 0), &[(patch, 0)])
        {
            continue;
        }
        let proved = part.neighbours(support).into_iter().find(|&neighbour| {
            part.shared_edges(support, neighbour).contains(&edge)
                && aperture_bore(ctx, surfaces, support, support_normal, neighbour)
                && (face_area(&disk, 0) - std::f64::consts::PI * radius.powi(2)).abs() <= 1e-6
        });
        if let Some(cylinder) = proved {
            results.push((disk, cylinder));
        }
    }
    results
}

/// Whether *neighbour* is the native bore explaining an aperture in *support* (`_aperture`'s
/// cylinder checks): concave and along the support's normal, its two full rings each meeting
/// one planar face, owned with them by one valid solid, and the cylinder between its rings
/// exactly its face and empty of material.
fn aperture_bore(
    ctx: &Context<'_>,
    surfaces: &EffectiveFaces<'_, '_>,
    support: usize,
    support_normal: V3,
    neighbour: usize,
) -> bool {
    let part = ctx.part;
    let Some((centre, axis, radius)) = cylindrical_channels::native_bore(ctx, surfaces, neighbour)
    else {
        return false;
    };
    if (geom::dot(support_normal, axis).abs() - 1.0).abs() > 1e-8 {
        return false;
    }
    let rings: Vec<usize> = part
        .face_edges(neighbour)
        .into_iter()
        .filter(|&e| matches!(part.edges[e].curve, Curve::Circle { .. }))
        .collect();
    let full = 2.0 * std::f64::consts::PI * radius;
    if rings.len() != 2
        || rings
            .iter()
            .any(|&e| (arc_length(part, e) - full).abs() > 1e-6)
    {
        return false;
    }
    let mut faces = vec![support, neighbour];
    for &ring in &rings {
        let planar: Vec<usize> = part
            .neighbours(neighbour)
            .into_iter()
            .filter(|&n| is_planar(part, n) && part.shared_edges(neighbour, n).contains(&ring))
            .collect();
        if planar.len() != 1 {
            return false;
        }
        faces.push(planar[0]);
    }
    let Some(owner) = common_valid_solid(part, &faces) else {
        return false;
    };
    let mut ends: Vec<f64> = rings
        .iter()
        .filter_map(|&e| match part.edges[e].curve {
            Curve::Circle { frame, .. } => Some(geom::dot(geom::sub(frame.origin, centre), axis)),
            _ => None,
        })
        .collect();
    ends.sort_by(f64::total_cmp);
    if ends[1] - ends[0] <= 1e-6 {
        return false;
    }
    let base = geom::add(centre, geom::scale(axis, ends[0]));
    let Some(cell) = cylinder_cell(base, axis, radius, ends[1] - ends[0]) else {
        return false;
    };
    // The cell's side, as OpenCascade builds it: one face with a seam (the swept cell's own
    // band has none, which the cover reads as no area).
    let Some(side) = band_face(base, axis, radius, ends[1] - ends[0]) else {
        return false;
    };
    // The kernel's cover puts each loop of a cylindrical support on the patch's turn on its
    // own, so it can miss a hole in the bore's face (mfcadpp/10138 face 28: 299.137 of the
    // band's 299.705 mm², read as covering it). A face covers the whole band only if it has
    // no edge but its two rings and its seams; that is checked first.
    let uses = |e: usize| {
        part.faces[neighbour]
            .loops
            .iter()
            .flat_map(|lp| &lp.edges)
            .filter(|(x, _)| *x == e)
            .count()
    };
    if part
        .face_edges(neighbour)
        .into_iter()
        .any(|e| !rings.contains(&e) && uses(e) != 2)
        || !covered_patch((part, neighbour), &[(&side, 0)])
        || !covered_patch((&side, 0), &[(part, neighbour)])
    {
        return false;
    }
    empty(ctx, owner, Some(&cell))
}

/// The frame of a cylinder from *base* along the unit *axis*, its seam on a fixed side.
fn axis_frame(base: V3, axis: V3) -> Option<Frame> {
    let seed = if axis[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let x = geom::unit(geom::sub(seed, geom::scale(axis, geom::dot(seed, axis))))?;
    Some(Frame {
        origin: base,
        x,
        y: geom::cross(axis, x),
        z: axis,
    })
}

/// A one-face part: the cylindrical band of *radius* from *base* along the unit *axis* for
/// *height*, bounded by its two rings and joined by one seam walked both ways.
fn band_face(base: V3, axis: V3, radius: f64, height: f64) -> Option<Part> {
    let low = axis_frame(base, axis)?;
    let high = Frame {
        origin: geom::add(base, geom::scale(axis, height)),
        ..low
    };
    let at = |f: &Frame| geom::add(f.origin, geom::scale(f.x, radius));
    let ring = |f: Frame, vertex: usize| {
        let curve = Curve::Circle { frame: f, radius };
        let start = at(&f);
        Edge {
            samples: sample_edge(&curve, start, start, true, true),
            curve,
            start,
            end: start,
            vertices: (vertex, vertex),
            same_sense: true,
        }
    };
    let (bottom, top) = (at(&low), at(&high));
    let seam = Edge {
        curve: Curve::Line {
            origin: bottom,
            dir: geom::sub(top, bottom),
        },
        start: bottom,
        end: top,
        vertices: (0, 1),
        same_sense: true,
        samples: vec![bottom, top],
    };
    let face = Face {
        surface: Surface::Cylinder { frame: low, radius },
        reversed: false,
        loops: vec![Loop {
            edges: vec![(0, true), (2, true), (1, false), (2, false)],
            vertex: None,
        }],
        solid: None,
        pcurves: Vec::new(),
    };
    Some(Part::new(
        vec![face],
        vec![ring(low, 0), ring(high, 1), seam],
        Vec::new(),
    ))
}

/// The solid cylinder of *radius* from *base* along the unit *axis* for *height*
/// (`Solid.make_cylinder`).
fn cylinder_cell(base: V3, axis: V3, radius: f64, height: f64) -> Option<Part> {
    let frame = axis_frame(base, axis)?;
    let disk = disk_part(
        &Curve::Circle { frame, radius },
        geom::add(base, geom::scale(frame.x, radius)),
    )?;
    extrude_face(&disk, 0, [0.0; 3], geom::scale(axis, height))
}

/// `prove_open_channel`: two opposed width walls and the floor completely supported by the
/// candidate's constituent planes, and the rectangle between them, both run ends and the
/// lateral mouth holding no material; a support may be pierced by proved native circular
/// apertures, whose bores join the proof's faces.
fn prove_open_channel(
    ctx: &Context<'_>,
    surfaces: &EffectiveFaces<'_, '_>,
    defining: &BTreeSet<usize>,
    constituent: &BTreeSet<usize>,
    record: &LegacyRecess,
) -> Option<OpenChannelProof> {
    let part = ctx.part;
    if record.edge_anchored == Some(true) {
        return None;
    }
    let owner = owner_of(part, constituent)?;
    let index = |a: char| usize::from(a as u8 - b'x');
    let (w, d, run) = (
        index(record.width_axis),
        index(record.depth_axis()),
        index(record.long_axis),
    );
    let mut walls: [Option<[(f64, f64); 3]>; 2] = [None, None];
    for &node in defining {
        if !is_planar(part, node) {
            continue;
        }
        let Some(n) = normal(part, node) else {
            continue;
        };
        if n[w].abs() < 1.0 - 1e-8 {
            continue;
        }
        let slot = &mut walls[usize::from(n[w] <= 0.0)];
        if slot.is_some() {
            return None; // ambiguous opposed-wall authority
        }
        *slot = Some(vertex_bounds(part, node));
    }
    let (Some(positive), Some(negative)) = (walls[0], walls[1]) else {
        return None;
    };
    let mut bounds: [(f64, f64); 3] = [0, 1, 2].map(|i| {
        (
            positive[i].0.max(negative[i].0),
            positive[i].1.min(negative[i].1),
        )
    });
    bounds[w] = (
        (positive[w].0 + positive[w].1) / 2.0,
        (negative[w].0 + negative[w].1) / 2.0,
    );
    if bounds.iter().any(|(lo, hi)| hi - lo <= 1e-6) {
        return None;
    }
    let open_sign = record.open_sign;
    let floor = if open_sign == 1 {
        bounds[d].0
    } else {
        bounds[d].1
    };
    let mouth = if open_sign == 1 {
        bounds[d].1
    } else {
        bounds[d].0
    };
    let mut aperture_faces: BTreeSet<usize> = BTreeSet::new();
    if ![
        (w, bounds[w].0, 1),
        (w, bounds[w].1, -1),
        (d, floor, open_sign),
    ]
    .iter()
    .all(|&(axis, at, sign)| {
        supports(
            ctx,
            surfaces,
            constituent,
            &bounds,
            axis,
            at,
            sign,
            &mut aperture_faces,
        )
    }) {
        return None;
    }
    let mut all: BTreeSet<usize> = constituent.clone();
    all.extend(aperture_faces.iter().copied());
    if owner_of(part, &all) != Some(owner) {
        return None;
    }
    let centre = bounds.map(|(lo, hi)| (lo + hi) / 2.0);
    let frame = LocalFrame::principal(&record.long_axis.to_string(), centre).ok()?;
    let local_axes = [[1, 2], [2, 0], [0, 1]][run];
    let [half_u, half_v] = local_axes.map(|i| (bounds[i].1 - bounds[i].0) / 2.0);
    let section = PlanarSection::polygon(&[
        [-half_u, -half_v],
        [half_u, -half_v],
        [half_u, half_v],
        [-half_u, half_v],
    ])
    .ok()?;
    let span = bounds[run];
    let thickness = f64::max(
        2e-5,
        [1.0, span.1 - span.0, half_u, half_v]
            .into_iter()
            .fold(f64::NEG_INFINITY, f64::max)
            * 1e-4,
    );
    if !empty(ctx, owner, probe_prism(&frame, span, &section).as_ref()) {
        return None;
    }
    for (end, sign) in [(span.0, -1.0), (span.1, 1.0)] {
        if !empty(
            ctx,
            owner,
            end_slab(&frame, end, sign, thickness, &section).as_ref(),
        ) {
            return None;
        }
    }
    // The lateral opening is physical absence too, not a fourth unobserved support wall.
    let mut lateral = bounds;
    let (a, b) = (
        mouth + f64::from(open_sign) * 1e-6,
        mouth + f64::from(open_sign) * thickness,
    );
    lateral[d] = (a.min(b), a.max(b));
    let corner = |x: usize, y: usize, z: usize| {
        let pick = |(lo, hi): (f64, f64), k: usize| if k == 0 { lo } else { hi };
        [
            pick(lateral[0], x),
            pick(lateral[1], y),
            pick(lateral[2], z),
        ]
    };
    let probe = ruled_prism(
        &[
            corner(0, 0, 0),
            corner(1, 0, 0),
            corner(1, 1, 0),
            corner(0, 1, 0),
        ],
        &[
            corner(0, 0, 1),
            corner(1, 0, 1),
            corner(1, 1, 1),
            corner(0, 1, 1),
        ],
    );
    if !empty(ctx, owner, probe.as_ref()) {
        return None;
    }
    let transverse: Vec<usize> = (0..3).filter(|&i| i != run).collect();
    let boundary = [
        (bounds[w].0, mouth),
        (bounds[w].0, floor),
        (bounds[w].1, floor),
        (bounds[w].1, mouth),
    ]
    .iter()
    .map(|&(width, depth)| {
        let mut xyz = [0.0; 3];
        xyz[w] = width;
        xyz[d] = depth;
        [xyz[transverse[0]], xyz[transverse[1]]]
    })
    .collect();
    Some(OpenChannelProof {
        axis: record.long_axis.to_string(),
        run_interval: Placed::new(span, Grid::CleanDecimals(3)),
        boundary,
        aperture_faces: aperture_faces.into_iter().collect(),
    })
}

// --- patterns ---------------------------------------------------------------------------------

/// `_section_midpoint`: the published section-centroid run line at the midpoint of its ends.
fn section_midpoint(record: &SectionRecess) -> V3 {
    let geometry = record.geometry();
    let (low, high) = geometry.run_interval();
    let along = py::fsum([low, high]) / 2.0;
    let frame = geometry.frame();
    [0, 1, 2].map(|i| frame.origin[i] + along * frame.run[i])
}

/// `_geometry.plane_axes`: the stable right-handed in-plane basis across a principal axis.
fn plane_axes(axis: char) -> (V3, V3) {
    match axis {
        'x' => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        'y' => ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        _ => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    }
}

/// `_project_section_pattern`: a derived pocket lattice re-expressed over its members' public
/// section midpoints, or `None` where they are on more than one body or do not reconstruct it
/// within the publication allowance.
fn project_section_pattern(
    pattern: &PocketPattern,
    occurrences: &[&SectionRecess],
) -> Result<Option<SectionRecessPattern>, SectionRecessError> {
    let bodies: BTreeSet<usize> = occurrences.iter().map(|r| r.body()).collect();
    if bodies.len() != 1 {
        return Ok(None);
    }
    let points: Vec<V3> = occurrences.iter().map(|r| section_midpoint(r)).collect();
    let count = points.len() as f64;
    let center: V3 = [0, 1, 2].map(|i| py::fsum(points.iter().map(|p| p[i])) / count);
    let mut pairs: Vec<(usize, V3)> = occurrences
        .iter()
        .map(|r| r.index())
        .zip(points.iter().copied())
        .collect();
    let step = |pairs: &[(usize, V3)], offsets: &[f64]| -> V3 {
        let denominator = py::fsum(offsets.iter().map(|o| o * o));
        [0, 1, 2].map(|i| {
            py::fsum(
                offsets
                    .iter()
                    .zip(pairs)
                    .map(|(o, (_, p))| o * (p[i] - center[i])),
            ) / denominator
        })
    };
    let n = pairs.len();
    let half = (n as f64 - 1.0) / 2.0;
    let (projected, expected): (SectionRecessPattern, Vec<V3>) = match pattern {
        PocketPattern::PocketArray { direction, .. } => {
            let mut direction = *direction;
            if direction
                .iter()
                .find(|v| v.abs() > 1e-9)
                .is_some_and(|&v| v < 0.0)
            {
                direction = direction.map(|v| -v);
            }
            let key = |p: &V3| py::sum((0..3).map(|i| p[i] * direction[i]));
            pairs.sort_by(|a, b| py::order(key(&a.1), key(&b.1)));
            let offsets: Vec<f64> = (0..n).map(|at| at as f64 - half).collect();
            let vector = step(&pairs, &offsets);
            let pitch = py::hypot(&vector);
            if pitch == 0.0 {
                return Ok(None);
            }
            let direction = vector.map(|v| v / pitch);
            let expected = (0..n)
                .map(|at| [0, 1, 2].map(|i| center[i] + (at as f64 - half) * pitch * direction[i]))
                .collect();
            (
                SectionRecessPattern::Array(SectionRecessArray::new(
                    pairs.iter().map(|(m, _)| *m).collect(),
                    pitch,
                    direction,
                )?),
                expected,
            )
        }
        PocketPattern::PocketGrid {
            pockets,
            rows,
            cols,
            row_pitch,
            col_pitch,
            angle,
            ..
        } => {
            let (rows, cols) = (*rows, *cols);
            let depth_axis = LegacyRecess::pocket(&pockets[0]).depth_axis();
            let (u, v) = plane_axes(depth_axis);
            let radians = angle.to_radians();
            let (cosine, sine) = (radians.cos(), radians.sin());
            let col_direction: V3 = [0, 1, 2].map(|i| cosine * u[i] + sine * v[i]);
            let row_direction: V3 = [0, 1, 2].map(|i| -sine * u[i] + cosine * v[i]);
            let row_half = (rows as f64 - 1.0) / 2.0;
            let col_half = (cols as f64 - 1.0) / 2.0;
            // Legacy grid members are column-major. Publish an explicit row-major roster,
            // matching the reconstructed cells, regardless of the source detector's ordering.
            let cell = |p: &V3| {
                let delta: V3 = [0, 1, 2].map(|i| p[i] - center[i]);
                (
                    round_int(
                        py::sum((0..3).map(|i| delta[i] * row_direction[i])) / row_pitch + row_half,
                    ),
                    round_int(
                        py::sum((0..3).map(|i| delta[i] * col_direction[i])) / col_pitch + col_half,
                    ),
                )
            };
            pairs.sort_by_key(|pair| cell(&pair.1));
            // Legacy pitch/angle fields are rounded. Use them only to assign cells; derive the
            // published lattice from the accepted midpoint coordinates.
            let row_offsets: Vec<f64> = (0..n).map(|at| (at / cols) as f64 - row_half).collect();
            let col_offsets: Vec<f64> = (0..n).map(|at| (at % cols) as f64 - col_half).collect();
            let row_step = step(&pairs, &row_offsets);
            let col_step = step(&pairs, &col_offsets);
            let col_pitch = py::hypot(&col_step);
            if col_pitch == 0.0 {
                return Ok(None);
            }
            let col_direction = col_step.map(|v| v / col_pitch);
            let skew = py::fsum((0..3).map(|i| row_step[i] * col_direction[i]));
            let row_step: V3 = [0, 1, 2].map(|i| row_step[i] - skew * col_direction[i]);
            let row_pitch = py::hypot(&row_step);
            if row_pitch == 0.0 {
                return Ok(None);
            }
            let row_direction = row_step.map(|v| v / row_pitch);
            let mut expected = Vec::new();
            for row in 0..rows {
                for col in 0..cols {
                    expected.push([0, 1, 2].map(|i| {
                        center[i]
                            + (row as f64 - row_half) * row_pitch * row_direction[i]
                            + (col as f64 - col_half) * col_pitch * col_direction[i]
                    }));
                }
            }
            (
                SectionRecessPattern::Grid(SectionRecessGrid::new(
                    pairs.iter().map(|(m, _)| *m).collect(),
                    rows,
                    cols,
                    row_pitch,
                    col_pitch,
                    row_direction,
                    col_direction,
                    center,
                )?),
                expected,
            )
        }
    };
    // Existing whole-occurrence publication displacement allowance, not a fitted recognition
    // tolerance: the pattern must reconstruct the public member centres.
    if pairs
        .iter()
        .zip(&expected)
        .any(|((_, point), target)| py::dist(point, target) > 0.002)
    {
        return Ok(None);
    }
    Ok(Some(projected))
}

/// `_section_patterns`: each derived pocket pattern whose members each project to exactly one
/// distinct kept record, then the largest first, one pattern per member.
fn section_patterns(
    part: &Part,
    patterns: &[PocketPattern],
    pockets: &[(&Pocket, Faces)],
    recesses: &[SectionRecess],
    projected_regions: &BTreeMap<Source, (usize, Vec<usize>)>,
) -> Result<Vec<SectionRecessPattern>, SectionRecessFamilyError> {
    let mut result: Vec<SectionRecessPattern> = Vec::new();
    for pattern in patterns {
        let members = match pattern {
            PocketPattern::PocketArray { pockets, .. }
            | PocketPattern::PocketGrid { pockets, .. } => pockets,
        };
        let mut occurrences = Vec::new();
        for member in members {
            // The derived pattern holds the accepted records themselves in Python; here equal
            // copies of them.
            let Some((_, faces)) = pockets.iter().find(|(p, _)| *p == member) else {
                return Err(SectionRecessFamilyError(
                    "pocket pattern member is not an accepted pocket".into(),
                ));
            };
            let matches = matching_recesses(part, faces, recesses, projected_regions);
            if matches.len() != 1 {
                occurrences.clear();
                break; // a pattern cannot refer to unproved or ambiguously projected geometry
            }
            occurrences.push(matches[0]);
        }
        if occurrences.len() != members.len() {
            continue;
        }
        let indices: BTreeSet<usize> = occurrences.iter().map(|r| r.index()).collect();
        if indices.len() != occurrences.len() {
            continue;
        }
        if let Some(projected) = project_section_pattern(pattern, &occurrences)?
            && !result.contains(&projected)
        {
            result.push(projected);
        }
    }
    // Distinct legacy specification groups can resolve to the same public region. Apply the
    // largest-first, one-pattern-per-member policy after that join.
    result.sort_by_key(|p| std::cmp::Reverse(p.members().len()));
    let mut allocated = Vec::new();
    let mut used: BTreeSet<usize> = BTreeSet::new();
    for pattern in result {
        if pattern.members().iter().all(|m| !used.contains(m)) {
            used.extend(pattern.members().iter().copied());
            allocated.push(pattern);
        }
    }
    Ok(allocated)
}
