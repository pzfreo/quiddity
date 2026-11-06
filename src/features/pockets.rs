//! Bounded blind recesses (`quiddity._recess_features.recognise_pockets` and
//! `_discover_pockets`): the through-slot's wall pairs kept where a floor caps one depth end,
//! corner notches, and stubby obround pockets from their caps, recognised per solid.
//!
//! The seed defines a pocket from opposed walls, a corner notch's floor and both walls, or
//! obround cap clusters. A uniquely owned complete cavity adds its observed planar end walls
//! or the sides joining both cap clusters. The floor and remaining cavity faces are consulted
//! context during discovery. After reconciliation the shared floor rule in `holes` adds a
//! uniquely closed planar floor to the accepted claim. An intact rectangular floor can also
//! prove the end walls when an intersecting cut destroys the mouth wire; competing cavity or floor ownership never expands a claim.

use std::collections::BTreeSet;

use super::Context;
use super::evidence::{EvidenceError, Occurrence, common_valid_solid, locally_valid_solid};
use super::recess_core::pocket_proposals;
use super::recess_reduce::{Proposal, body_scoped};
use super::records;
use crate::kernel::brep::Part;

pub use super::recess_records::Pocket;

internal_entry! {
/// `recognise_pockets`.
pub fn recognise_pockets(part: &Part) -> Vec<Pocket> {
    records(discover(&Context::new(part)))
}
}

/// The occurrence and every face it consulted (`members`: defining, floors, inner region).
fn occurrence(proposal: Proposal<Pocket>) -> (Occurrence<Pocket>, Vec<usize>) {
    let defining = proposal.defining();
    let members: BTreeSet<usize> = defining
        .iter()
        .chain(&proposal.floors)
        .chain(&proposal.constituent)
        .copied()
        .collect();
    let found = Occurrence {
        context: members.difference(&defining).copied().collect(),
        defining: defining.into_iter().collect(),
        record: proposal.record.published(),
    };
    (found, members.into_iter().collect())
}

internal_entry! {
/// Every pocket with its faces, in (width, centre) order. Competing cap clusters skip the
/// ambiguous occurrence on this record path; the evidence path returns `CompetingCaps`.
pub fn discover(ctx: &Context<'_>) -> Vec<Occurrence<Pocket>> {
    body_scoped(ctx, |ctx, solid| pocket_proposals(ctx, solid, false))
        .unwrap_or_else(|_| unreachable!("only a strict scan refuses"))
        .into_iter()
        .map(|p| occurrence(p).0)
        .collect()
}
}

internal_entry! {
/// The evidence path (`_discover_pockets` with a writer): competing cap clusters refuse, as do
/// a wall- or cap-found pocket without floor faces, faces without one valid solid, and one
/// record given two face sets on one solid; a record found twice from the same faces is
/// published once.
pub fn discover_verified(ctx: &Context<'_>) -> Result<Vec<Occurrence<Pocket>>, EvidenceError> {
    verified(ctx, false)
}
}

internal_entry! {
/// The evidence path as Python's locally degraded run takes it (`_discover_pockets` on a graph
/// with `local_degradation` set): a pocket whose faces (defining, floors, inner region) prove no
/// one valid solid is skipped instead of refusing every pocket, and the duplicate check is made
/// over the pockets kept; every other refusal stands. Python's degraded run also admits a solid
/// with at most three faulted faces, skipping only the pockets that touch them
/// ([`locally_valid_solid`]).
pub fn discover_locally_degraded(
    ctx: &Context<'_>,
) -> Result<Vec<Occurrence<Pocket>>, EvidenceError> {
    verified(ctx, true)
}
}

/// The evidence path, skipping (when *degraded*) rather than refusing a pocket proving no one
/// valid solid.
fn verified(ctx: &Context<'_>, degraded: bool) -> Result<Vec<Occurrence<Pocket>>, EvidenceError> {
    let proposals = body_scoped(ctx, |ctx, solid| pocket_proposals(ctx, solid, true))
        .map_err(|_| EvidenceError::CompetingCaps)?;
    let mut pending: Vec<(Occurrence<Pocket>, Vec<usize>, usize)> = Vec::new();
    for proposal in proposals {
        if !proposal.record.edge_anchored && proposal.floors.is_empty() {
            return Err(EvidenceError::MissingFloor);
        }
        let (found, members) = occurrence(proposal);
        if found.defining.is_empty() {
            return Err(EvidenceError::NoValidSolid);
        }
        let proved = if degraded {
            locally_valid_solid(ctx.part, &members)
        } else {
            common_valid_solid(ctx.part, &members)
        };
        let Some(solid) = proved else {
            if degraded {
                continue;
            }
            return Err(EvidenceError::NoValidSolid);
        };
        match pending
            .iter()
            .find(|(other, _, other_solid)| other.record == found.record && *other_solid == solid)
        {
            Some((other, other_members, _)) => {
                if other.defining != found.defining || *other_members != members {
                    return Err(EvidenceError::SharedEvidence);
                }
            }
            None => pending.push((found, members, solid)),
        }
    }
    Ok(pending.into_iter().map(|(o, _, _)| o).collect())
}
