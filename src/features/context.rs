//! One recognition run over one part: the part plus the analysis several families share,
//! each computed on first use (`FaceGraph` and `SolidProperties` in the Python implementation).
//!
//! The rule for where a cache lives: facts about the topology (neighbours, validity, face
//! bounds, parameter-space domains) belong to [`Part`]; recognition analysis that more than one
//! family reads (the classifier, the cylinder inventory) belongs here.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use super::body::{BodyKey, body_signature, unambiguous_body_keys};
use super::cylinders::{CylinderEvidence, analyse_cylinders};
use super::modelled_threads::{ThreadAnalysis, analyse_modelled_threads};
use super::passages::{Found, PassageError, find_section_passages};
use super::section_recess::{SectionRecess, SectionRecessError};
use crate::kernel::brep::Part;
use crate::kernel::classify::Classifier;
use crate::kernel::geom::{Bounds, V3};
use crate::kernel::rays::RayCaster;
use crate::kernel::sweep::extrude_face;
use crate::kernel::volume::{Probe, common_volume, probe_volume};

/// A closed inward-facing boundary component of a solid, independent of air sampling.
pub(super) struct SealedVoidShell {
    pub solid: usize,
    pub faces: Vec<usize>,
    pub volume: f64,
}

/// Negative groups prove voids only when the largest boundary group proves an outward
/// outer skin. Inverted or ambiguous outer skins retain the recognizers' old behavior.
fn classify_shell_groups(solid: usize, groups: Vec<(Vec<usize>, f64)>) -> Vec<SealedVoidShell> {
    let Some(largest) = groups.iter().map(|(_, v)| v.abs()).max_by(f64::total_cmp) else {
        return Vec::new();
    };
    if groups.iter().any(|(_, v)| !v.is_finite())
        || groups.iter().any(|(_, v)| v.abs() == largest && *v <= 0.0)
    {
        return Vec::new();
    }
    groups
        .into_iter()
        .filter(|(_, volume)| *volume < 0.0)
        .map(|(faces, volume)| SealedVoidShell { solid, faces, volume: -volume })
        .collect()
}

pub struct Context<'a> {
    pub part: &'a Part,
    pub(super) floor_blends: Vec<super::recess_core::FloorBlendAnalysisCache>,
    bounds: OnceLock<Bounds>,
    classifier: OnceLock<Classifier<'a>>,
    solid_classifiers: Vec<OnceLock<Classifier<'a>>>,
    cylinders: OnceLock<Vec<CylinderEvidence>>,
    sealed_void_shells: OnceLock<Vec<SealedVoidShell>>,
    threads: OnceLock<ThreadAnalysis>,
    chamfers: OnceLock<Vec<super::evidence::Occurrence<super::chamfers::Chamfer>>>,
    oriented_chamfers: OnceLock<Vec<super::evidence::Occurrence<super::oriented_chamfers::OrientedChamfer>>>,
    fillets: OnceLock<Vec<super::evidence::Occurrence<super::fillets::Fillet>>>,
    bosses: OnceLock<super::bosses::Analysis>,
    edge_treatments: OnceLock<Vec<super::evidence::Occurrence<()>>>,
    skins: Vec<OnceLock<Option<super::plates::Skin>>>,
    surface_moments: Vec<OnceLock<Option<crate::kernel::mass::FaceMoments>>>,
    signatures: OnceLock<Vec<Option<BodyKey>>>,
    section_passages: OnceLock<Result<Vec<Found>, PassageError>>,
    native_section_recesses: [OnceLock<Result<Vec<SectionRecess>, SectionRecessError>>; 2],
}

impl<'a> Context<'a> {
internal_entry! {
    pub fn new(part: &'a Part) -> Self {
        Context {
            part,
            floor_blends: part.solids.iter().map(|_| Default::default()).collect(),
            bounds: OnceLock::new(),
            classifier: OnceLock::new(),
            solid_classifiers: part.solids.iter().map(|_| OnceLock::new()).collect(),
            cylinders: OnceLock::new(),
            sealed_void_shells: OnceLock::new(),
            threads: OnceLock::new(),
            chamfers: OnceLock::new(),
            oriented_chamfers: OnceLock::new(),
            fillets: OnceLock::new(),
            bosses: OnceLock::new(),
            edge_treatments: OnceLock::new(),
            skins: part.faces.iter().map(|_| OnceLock::new()).collect(),
            surface_moments: part.faces.iter().map(|_| OnceLock::new()).collect(),
            signatures: OnceLock::new(),
            section_passages: OnceLock::new(),
            native_section_recesses: std::array::from_fn(|_| OnceLock::new()),
        }
    }
}

    /// The whole part's box.
    pub fn bounds(&self) -> &Bounds {
        self.bounds.get_or_init(|| self.part.bounds())
    }

    /// Convex edge treatments, including their supporting evidence, independent of the
    /// minimum radius used to report fillets. Shared by boss extent and plate coverage.
    pub(crate) fn edge_treatments(&self) -> &[super::evidence::Occurrence<()>] {
        self.edge_treatments
            .get_or_init(|| super::edge_treatments::discover(self))
    }

    pub(crate) fn reported_chamfers(&self) -> &[super::evidence::Occurrence<super::chamfers::Chamfer>] {
        self.chamfers.get_or_init(|| super::chamfers::discover(self, &Default::default()))
    }

    pub(crate) fn reported_oriented_chamfers(&self) -> &[super::evidence::Occurrence<super::oriented_chamfers::OrientedChamfer>] {
        self.oriented_chamfers.get_or_init(|| super::oriented_chamfers::discover(self, &Default::default()))
    }

    pub(crate) fn reported_fillets(&self) -> &[super::evidence::Occurrence<super::fillets::Fillet>] {
        self.fillets.get_or_init(|| super::fillets::discover(self, &Default::default()))
    }

    pub(super) fn boss_analysis(&self) -> &super::bosses::Analysis {
        self.bosses.get_or_init(|| super::bosses::analyse(self))
    }

    pub(super) fn skin(&self, face: usize) -> Option<&super::plates::Skin> {
        self.skins[face].get_or_init(|| super::plates::measure_skin(self, face)).as_ref()
    }

    /// Shared by the boss's body-relative origin and constant-Jacobian skin strips.
    pub(super) fn surface_moments(&self, face: usize) -> Option<crate::kernel::mass::FaceMoments> {
        *self.surface_moments[face].get_or_init(|| self.part.face_moments(face))
    }

    pub(crate) fn measured_surface_moments(&self, face: usize) -> Option<crate::kernel::mass::FaceMoments> {
        self.surface_moments[face].get().copied().flatten()
    }

    /// The point classifier for the whole part.
    pub fn classifier(&self) -> &Classifier<'a> {
        self.classifier.get_or_init(|| Classifier::new(self.part))
    }

    /// The point classifier for one solid alone.
    pub fn solid_classifier(&self, solid: usize) -> &Classifier<'a> {
        self.solid_classifiers[solid].get_or_init(|| Classifier::for_solid(self.part, solid))
    }

    /// Rays against one solid's faces (`IntCurvesFace_ShapeIntersector` loaded with the solid).
    pub fn solid_rays(&self, solid: usize) -> &RayCaster<'a> {
        self.solid_classifier(solid).rays()
    }

    /// Each solid's body key (`unambiguous_body_keys`).
    pub fn body_keys(&self, require_valid_solid: bool) -> Vec<Option<BodyKey>> {
        let signatures = self.signatures.get_or_init(|| {
            (0..self.part.solids.len())
                .map(|s| body_signature(self.part, s))
                .collect()
        });
        unambiguous_body_keys(self.part, signatures, require_valid_solid)
    }

    /// Every native cylindrical face (`analyse_cylinders`), in solid then face order.
    pub fn cylinders(&self) -> &[CylinderEvidence] {
        self.cylinders.get_or_init(|| analyse_cylinders(self.part))
    }

    /// Shared sealed-air proof for holes and interior voids: closed edge-connected shells
    /// with inward orientation (negative signed volume). Mouths into open pockets remain on
    /// the outer shell. The largest-|volume| group must be outward-facing. This does not
    /// depend on face order or a grid's ability to resolve air.
    pub(super) fn sealed_void_shells(&self) -> &[SealedVoidShell] {
        self.sealed_void_shells.get_or_init(|| {
            let part = self.part;
            let mut enclosed = Vec::new();
            for (solid, body) in part.solids.iter().enumerate() {
                let mut remaining: BTreeSet<_> = body.faces.iter().copied().collect();
                let mut shells = Vec::new();
                while let Some(seed) = remaining.pop_first() {
                    let mut shell = vec![seed];
                    let mut pending = vec![seed];
                    while let Some(face) = pending.pop() {
                        for other in part.neighbours(face) {
                            if remaining.remove(&other) {
                                shell.push(other);
                                pending.push(other);
                            }
                        }
                    }
                    shell.sort_unstable();
                    shells.push(shell);
                }
                if shells.len() < 2 || !part.solid_is_valid(solid) {
                    continue;
                }
                // An unmeasured group could be the outer skin; it cannot be omitted from
                // the orientation proof by considering only the groups whose mass resolved.
                let Some(groups) = shells.into_iter().map(|faces| {
                    part.shell_mass(&faces).map(|(volume, _)| (faces, volume))
                }).collect::<Option<Vec<_>>>() else {
                    continue;
                };
                enclosed.extend(classify_shell_groups(solid, groups));
            }
            enclosed
        })
    }

    /// Helical proofs shared by the thread inventory and groove/root disambiguation.
    pub(super) fn modelled_threads(&self) -> &ThreadAnalysis {
        self.threads
            .get_or_init(|| analyse_modelled_threads(self.part))
    }

    /// The section passages before publication, or their refusal: found once, as the
    /// passages and the reconciliation's compatibility views both read them.
    pub(super) fn section_passages(&self) -> &Result<Vec<Found>, PassageError> {
        self.section_passages
            .get_or_init(|| find_section_passages(self))
    }

    /// Geometry-only records shared by reconciliation and publication. Strict and locally
    /// degraded ownership proofs occupy separate slots; neither the accepted inventory nor
    /// its projection is cached. Each context borrows one immutable part; a new run or
    /// transformed part starts fresh. Discovery order and complete errors are retained;
    /// a panicking initializer leaves the slot empty for retry.
    pub(crate) fn native_section_recesses(
        &self,
        surfaces: &super::effective_surfaces::EffectiveFaces<'_, '_>,
    ) -> &Result<Vec<SectionRecess>, SectionRecessError> {
        let mode = usize::from(super::evidence::local_degradation());
        self.native_section_recesses[mode].get_or_init(|| {
            super::section_recess_discovery::discover_section_recesses(self, surfaces)
                .map(|found| found.into_iter().map(|occurrence| occurrence.record).collect())
        })
    }

    /// The volume *face* moved by *offset* and swept along *sweep* shares with one solid
    /// (`probe_volume` of a `Solid.extrude` of the face), or `None` when the face cannot be
    /// swept exactly (see [`extrude_face`]) or the volume probe cannot answer.
    pub fn swept_face_volume(
        &self,
        solid: usize,
        face: usize,
        offset: V3,
        sweep: V3,
    ) -> Option<f64> {
        let prism = extrude_face(self.part, face, offset, sweep)?;
        let probe = Probe::Solid(RayCaster::for_solid(&prism, 0));
        common_volume(self.solid_classifier(solid), &probe)
    }

    /// The fraction of that swept face's own volume one solid's material fills
    /// (`material_fraction` of a `Solid.extrude` of the face), or `None` when the face cannot be
    /// swept exactly or sweeps to no volume (Python divides by zero there and refuses), or the
    /// volume probe cannot answer.
    pub fn swept_face_fraction(
        &self,
        solid: usize,
        face: usize,
        offset: V3,
        sweep: V3,
    ) -> Option<f64> {
        let prism = extrude_face(self.part, face, offset, sweep)?;
        let probe = Probe::Solid(RayCaster::for_solid(&prism, 0));
        let whole = probe_volume(&probe).filter(|&w| w > 0.0)?;
        Some(common_volume(self.solid_classifier(solid), &probe)? / whole)
    }
}

#[cfg(test)]
mod shell_group_tests {
    use super::*;

    #[test]
    fn an_inverted_outer_group_keeps_its_exterior_hole_candidates() {
        // Faces 7 and 8 are exterior hole walls on the outer group. Even with an
        // additional negative group, an inverted outer skin supplies no sealed-air proof.
        for outer in [-100.0, -8.0] {
            for groups in [
                vec![(vec![2, 7, 8], outer), (vec![10], 8.0), (vec![11], -2.0)],
                vec![(vec![11], -2.0), (vec![10], 8.0), (vec![2, 7, 8], outer)],
            ] {
                let sealed = classify_shell_groups(0, groups);
                assert!(sealed.is_empty());
                let kept: Vec<_> = [7, 8].into_iter().filter(|f|
                    !sealed.iter().any(|shell| shell.faces.contains(f))
                ).collect();
                assert_eq!(kept, [7, 8]);
            }
        }
        let sealed = classify_shell_groups(0, vec![(vec![2, 7, 8], 100.0), (vec![11], -2.0)]);
        assert_eq!(sealed.len(), 1);
        assert_eq!(sealed[0].faces, [11]);
        assert_eq!(sealed[0].volume, 2.0);
    }
}
