//! The aggregate's cross-family reconciliation decisions (`quiddity._reconcile`, applied in
//! `quiddity.result._reconcile_existing`): the named rules that decide between families
//! describing one physical region, after every family has proposed (ADR 0003).
//!
//! Two kinds of rule. *Precedence* rejects a record a more complete one already describes: the
//! recess families from their complete boundary claims, a chamfer that is an angled step's
//! slant, a fillet that is a circular blind step's wall, a blend chain whose faces are all
//! fillets', a hole that is part of a Double-D bore, a boss that is exactly a turned step, a
//! passage that is exactly an oriented slot, and a plate, boss or riser lying inside a
//! thin-wall body's skins. *Compatibility* accepts both a turned step and the groove its band
//! is, related to each other.
//!
//! Every rule reads a record's evidence by identity, never by position: a candidate is its
//! family and index, and its faces are the defining (or, for a pocket, constituent) faces its
//! own occurrence published. Python's module docstring explains why: pairing records with
//! claims by position survived a permutation that handed every record another's faces.
//!
//! `reconcile` makes the decisions; `super::inventory` makes them on one run's candidates and
//! `super::recognise` applies them, dropping every rejected record (Python's `accepted_set`).
//! Python's `ReconciliationResult.complete` gives every undecided candidate a default
//! acceptance; that is left out here, as is any decision on a family no rule reads.

use std::collections::BTreeSet;
use std::fmt;

use serde::Serialize;

use super::passage_compat::{PassageCompatibilityView, grouping_from_view};
use super::passages::PassageError;
use super::sections::V2;
use super::{Context, Defining, Features, Occurrence, levels, pockets};

/// A disposition's outcome (`Outcome`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Outcome {
    #[serde(rename = "accepted")]
    Accepted,
    #[serde(rename = "rejected")]
    Rejected,
}

impl Outcome {
    /// Python's value.
    pub fn value(self) -> &'static str {
        match self {
            Outcome::Accepted => "accepted",
            Outcome::Rejected => "rejected",
        }
    }
}

macro_rules! reasons {
    ($($name:ident = $value:literal: $outcome:ident, $subject:ident, $related:ident;)*) => {
        /// Why a candidate was decided (`ReasonCode`), with Python's values. The default
        /// acceptance (`default.accepted`) is never issued here.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        pub enum ReasonCode {
            $(#[serde(rename = $value)] $name,)*
        }

        impl ReasonCode {
            /// Every reason a rule issues.
            pub const ALL: &'static [ReasonCode] = &[$(ReasonCode::$name,)*];

            /// Python's value.
            pub fn value(self) -> &'static str {
                match self { $(ReasonCode::$name => $value,)* }
            }

            /// The outcome, subject family and related family the reason requires
            /// (`_REASON_SPEC`); every issued reason requires related candidates.
            pub fn spec(self) -> (Outcome, Family, Family) {
                match self {
                    $(ReasonCode::$name => (Outcome::$outcome, Family::$subject, Family::$related),)*
                }
            }
        }
    };
}

reasons! {
    PrismaticSupersededByPocket = "recess.prismatic_superseded_by_pocket":
        Rejected, PrismaticPockets, Pockets;
    PocketSupersededByRectangularBlindSlot = "recess.pocket_superseded_by_rectangular_blind_slot":
        Rejected, Pockets, RectangularBlindSlots;
    PocketSupersededByEdgeOpenCircularPocket =
        "recess.pocket_superseded_by_edge_open_circular_pocket":
        Rejected, Pockets, EdgeOpenCircularPockets;
    PocketSupersededByPassage = "recess.pocket_superseded_by_passage":
        Rejected, Pockets, Passages;
    PocketSupersededByPrismatic = "recess.pocket_superseded_by_prismatic":
        Rejected, Pockets, PrismaticPockets;
    SlotSupersededByPocket = "recess.slot_superseded_by_pocket": Rejected, Slots, Pockets;
    SlotSupersededByPrismatic = "recess.slot_superseded_by_prismatic":
        Rejected, Slots, PrismaticPockets;
    SlotSupersededByPassage = "recess.slot_superseded_by_passage": Rejected, Slots, Passages;
    PassageSupersededBySlot = "recess.passage_superseded_by_slot": Rejected, Passages, Slots;
    PassageSupersededByOrientedSlot = "recess.passage_superseded_by_oriented_slot":
        Rejected, Passages, OrientedSlots;
    ChamferSupersededByAngledStep = "bevel.chamfer_superseded_by_angled_step":
        Rejected, Chamfers, AngledSteps;
    FilletSupersededByCircularBlindStep = "blend.fillet_superseded_by_circular_blind_step":
        Rejected, Fillets, CircularBlindSteps;
    FilletSupersededByPocket = "blend.fillet_superseded_by_pocket":
        Rejected, Fillets, Pockets;
    BlendSupersededByPocket = "blend.chain_superseded_by_pocket":
        Rejected, Blends, Pockets;
    BlendSupersededByFillet = "blend.chain_superseded_by_fillet": Rejected, Blends, Fillets;
    HoleSupersededByDoubleDBore = "bore.hole_superseded_by_double_d_bore":
        Rejected, Holes, DoubleDBores;
    BossSupersededByTurnedStep = "turned.boss_superseded_by_step":
        Rejected, Bosses, TurnedSteps;
    PlateSupersededByThinWall = "wall.plate_superseded_by_body":
        Rejected, Plates, ThinWallBodies;
    BossSupersededByThinWall = "wall.boss_superseded_by_body":
        Rejected, Bosses, ThinWallBodies;
    RiserSupersededByThinWall = "wall.riser_superseded_by_body":
        Rejected, Risers, ThinWallBodies;
    TurnedStepGrooveCompatible = "turned.step_groove_compatible":
        Accepted, TurnedSteps, Grooves;
    GrooveTurnedStepCompatible = "turned.groove_step_compatible":
        Accepted, Grooves, TurnedSteps;
}

/// The physical families a rule reads (`FamilyId`), with Python's values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    AngledSteps,
    Blends,
    Bosses,
    Chamfers,
    CircularBlindSteps,
    DoubleDBores,
    EdgeOpenCircularPockets,
    Fillets,
    Grooves,
    Holes,
    OrientedSlots,
    Passages,
    Plates,
    Pockets,
    PrismaticPockets,
    RectangularBlindSlots,
    Risers,
    Slots,
    ThinWallBodies,
    TurnedSteps,
}

impl Family {
    /// Python's value (`FamilyId.value`).
    pub fn value(self) -> &'static str {
        match self {
            Family::AngledSteps => "angled_steps",
            Family::Blends => "blends",
            Family::Bosses => "bosses",
            Family::Chamfers => "chamfers",
            Family::CircularBlindSteps => "circular_blind_steps",
            Family::DoubleDBores => "double_d_bores",
            Family::EdgeOpenCircularPockets => "edge_open_circular_pockets",
            Family::Fillets => "fillets",
            Family::Grooves => "grooves",
            Family::Holes => "holes",
            Family::OrientedSlots => "oriented_slots",
            Family::Passages => "passages",
            Family::Plates => "plates",
            Family::Pockets => "pockets",
            Family::PrismaticPockets => "prismatic_pockets",
            Family::RectangularBlindSlots => "rectangular_blind_slots",
            Family::Risers => "risers",
            Family::Slots => "slots",
            Family::ThinWallBodies => "thin_wall_bodies",
            Family::TurnedSteps => "turned_steps",
        }
    }

    /// The [`Features`] field (and [`super::Defining`] key) holding the family's records;
    /// Python's passages are the port's section passages. Risers are not in [`Features`].
    pub(crate) fn field(self) -> Option<&'static str> {
        match self {
            Family::Passages => Some("section_passages"),
            Family::Risers => None,
            other => Some(other.value()),
        }
    }
}

/// One candidate: its family, its index in that family's records, and its defining faces.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Candidate {
    pub family: Family,
    pub index: usize,
    pub defining: Vec<usize>,
}

/// One decision on a candidate (`Disposition`): its outcome and reason, and the candidates it
/// lost to (or, for a compatible pair, is related to), in their family's order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Disposition {
    pub candidate: Candidate,
    pub outcome: Outcome,
    pub reason: ReasonCode,
    pub related: Vec<Candidate>,
}

/// A refusal, with Python's message: the inputs do not describe one run, or two rules decided
/// one candidate (`ReconciliationResult.complete` raises there, refusing the whole inventory).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconcileError(pub String);

impl fmt::Display for ReconcileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ReconcileError {}

impl From<PassageError> for ReconcileError {
    fn from(error: PassageError) -> Self {
        ReconcileError(error.to_string())
    }
}

/// What the rules read beyond [`Features`]' records and defining faces: each pocket's
/// constituent faces (its walls, floors and inner region), each section passage's compatibility
/// view, and the risers' defining faces. Python's aggregate runs risers as a physical family;
/// the port's `super::recognise` calls them on their own, so they are found here with the
/// aggregate's options (`_discover_risers`: `min_area_frac` 0.15, the default tolerance; the
/// body levels it attaches change no record's faces).
#[derive(Clone, Debug)]
pub struct Evidence {
    pub pocket_constituent: Vec<Vec<usize>>,
    pub pocket_floor_fillets: Vec<Vec<usize>>,
    pub passage_compatibility: Vec<PassageCompatibilityView>,
    pub risers: Vec<Vec<usize>>,
}

impl Evidence {
    internal_entry! {
        /// The run's evidence on *ctx*'s part, or Python's refusal of its passages.
        pub fn discover(ctx: &Context<'_>) -> Result<Evidence, ReconcileError> {
            Self::with_pockets(ctx, &pockets::discover(ctx))
        }
    }

    internal_entry! {
        /// [`Evidence::discover`], given the run's pocket occurrences.
        pub fn with_pockets(
            ctx: &Context<'_>,
            pockets: &[Occurrence<pockets::Pocket>],
        ) -> Result<Evidence, ReconcileError> {
            Ok(Self::with_passage_views(ctx, pockets, super::passages::compatibility_views(ctx)?))
        }
    }

    pub(crate) fn with_passage_views(
        ctx: &Context<'_>,
        pockets: &[Occurrence<pockets::Pocket>],
        passage_compatibility: Vec<PassageCompatibilityView>,
    ) -> Evidence {
        Evidence {
            pocket_floor_fillets: pockets
                .iter()
                .map(|o| o.record.floor_fillets.iter().map(|f| f.face).collect())
                .collect(),
            pocket_constituent: pockets
                .iter()
                .map(|o| {
                    let mut faces: Vec<usize> =
                        o.defining.iter().chain(&o.context).copied().collect();
                    faces.sort_unstable();
                    faces.dedup();
                    faces
                })
                .collect(),
            passage_compatibility,
            risers: levels::risers_with_faces(ctx.part, &levels::RiserOptions::default())
                .into_iter()
                .map(|(_, faces)| faces)
                .collect(),
        }
    }
}

type Faces = BTreeSet<usize>;

/// One family's candidates, each with its defining faces as a set.
struct Group {
    family: Family,
    faces: Vec<Faces>,
}

impl Group {
    fn candidate(&self, index: usize) -> Candidate {
        Candidate {
            family: self.family,
            index,
            defining: self.faces[index].iter().copied().collect(),
        }
    }

    fn len(&self) -> usize {
        self.faces.len()
    }
}

/// The decisions so far, in rule order ([`completed`] refuses a candidate decided twice).
#[derive(Default)]
struct Decisions(Vec<Disposition>);

impl Decisions {
    fn push(
        &mut self,
        subject: &Group,
        index: usize,
        reason: ReasonCode,
        winners: &Group,
        by: Vec<usize>,
    ) {
        let (outcome, _, _) = reason.spec();
        self.0.push(Disposition {
            candidate: subject.candidate(index),
            outcome,
            reason,
            related: by.into_iter().map(|i| winners.candidate(i)).collect(),
        });
    }

    fn decided(&self, family: Family, index: usize) -> bool {
        self.0
            .iter()
            .any(|d| d.candidate.family == family && d.candidate.index == index)
    }
}

internal_entry! {
/// Every decision Python's aggregate makes on *features*' records, in `_reconcile_existing`'s
/// rule order, or Python's refusal.
pub fn reconcile(
    features: &Features,
    evidence: &Evidence,
) -> Result<Vec<Disposition>, ReconcileError> {
    let sides: Vec<usize> = features.prismatic_pockets.iter().map(|p| p.sides).collect();
    reconcile_faces(&features.defining, &sides, evidence)
}
}

internal_entry! {
/// `reconcile` on its inputs alone: each family's defining faces under its [`Features`] field
/// name, and each prismatic pocket's side count (the one record value a rule reads).
pub fn reconcile_faces(
    defining: &Defining,
    prismatic_sides: &[usize],
    evidence: &Evidence,
) -> Result<Vec<Disposition>, ReconcileError> {
    let group = |family: Family| -> Result<Group, ReconcileError> {
        let faces: Vec<Faces> = match family.field() {
            Some(field) => defining
                .get(field)
                .ok_or_else(|| ReconcileError(format!("{field} has no defining evidence")))?
                .iter()
                .map(|f| f.iter().copied().collect())
                .collect(),
            None => evidence
                .risers
                .iter()
                .map(|f| f.iter().copied().collect())
                .collect(),
        };
        Ok(Group { family, faces })
    };
    let slots = group(Family::Slots)?;
    let pockets = group(Family::Pockets)?;
    let prismatic = group(Family::PrismaticPockets)?;
    let passages = group(Family::Passages)?;
    let blind_slots = group(Family::RectangularBlindSlots)?;
    let open_circular = group(Family::EdgeOpenCircularPockets)?;
    if prismatic.len() != prismatic_sides.len()
        || evidence.pocket_constituent.len() != pockets.len()
        || evidence.pocket_floor_fillets.len() != pockets.len()
        || evidence.passage_compatibility.len() != passages.len()
    {
        return Err(ReconcileError(
            "candidate evidence does not match its records".into(),
        ));
    }
    for (constituent, defining) in evidence.pocket_constituent.iter().zip(&pockets.faces) {
        if !defining.iter().all(|f| constituent.contains(f)) {
            return Err(ReconcileError(
                "defining evidence must be a subset of constituent evidence".into(),
            ));
        }
    }
    for (treatments, defining) in evidence.pocket_floor_fillets.iter().zip(&pockets.faces) {
        if !treatments.iter().all(|f| defining.contains(f)) {
            return Err(ReconcileError("floor treatments must be defining pocket faces".into()));
        }
    }
    let mut decisions = Decisions::default();
    recesses(
        &mut decisions,
        &slots,
        &pockets,
        &prismatic,
        &passages,
        &blind_slots,
        &open_circular,
        prismatic_sides,
        evidence,
    );

    // A chamfer that names an angled step's slant at all (`reconcile_bevel_candidates`).
    let (chamfers, steps) = (group(Family::Chamfers)?, group(Family::AngledSteps)?);
    for (i, faces) in chamfers.faces.iter().enumerate() {
        let by = winners(&steps, |step| !faces.is_disjoint(step));
        if !by.is_empty() {
            decisions.push(
                &chamfers,
                i,
                ReasonCode::ChamferSupersededByAngledStep,
                &steps,
                by,
            );
        }
    }

    // Proved pocket floor treatments and circular blind steps own their fillets;
    // those fillets win no blend. Pocket wall corners are not floor treatments.
    let (fillets, circular) = (group(Family::Fillets)?, group(Family::CircularBlindSteps)?);
    let mut rejected_fillets = BTreeSet::new();
    for (i, faces) in fillets.faces.iter().enumerate() {
        let by: Vec<_> = (0..pockets.len()).filter(|&j| {
            !decisions.decided(Family::Pockets, j) && !faces.is_empty() && faces.iter().all(|f| evidence.pocket_floor_fillets[j].contains(f))
        }).collect();
        if !by.is_empty() {
            rejected_fillets.insert(i);
            decisions.push(&fillets, i, ReasonCode::FilletSupersededByPocket, &pockets, by);
            continue;
        }
        let by = winners(&circular, |step| !faces.is_empty() && faces.is_subset(step));
        if !by.is_empty() {
            rejected_fillets.insert(i);
            decisions.push(
                &fillets,
                i,
                ReasonCode::FilletSupersededByCircularBlindStep,
                &circular,
                by,
            );
        }
    }

    // A blend chain every face of which is a surviving fillet's (`reconcile_blend_candidates`).
    let blends = group(Family::Blends)?;
    for (i, faces) in blends.faces.iter().enumerate() {
        let by: Vec<_> = (0..pockets.len()).filter(|&j| {
            !decisions.decided(Family::Pockets, j) && !faces.is_empty() && faces.iter().all(|f| evidence.pocket_floor_fillets[j].contains(f))
        }).collect();
        if !by.is_empty() {
            decisions.push(&blends, i, ReasonCode::BlendSupersededByPocket, &pockets, by);
            continue;
        }
        let by: Vec<usize> = (0..fillets.len())
            .filter(|j| !rejected_fillets.contains(j) && !faces.is_disjoint(&fillets.faces[*j]))
            .collect();
        let covered: Faces = by
            .iter()
            .flat_map(|&j| fillets.faces[j].iter().copied())
            .collect();
        if !faces.is_empty() && faces.is_subset(&covered) {
            decisions.push(
                &blends,
                i,
                ReasonCode::BlendSupersededByFillet,
                &fillets,
                by,
            );
        }
    }

    // A hole whose faces are a strict part of a Double-D bore's
    // (`reconcile_profiled_bore_candidates`).
    let (holes, bores) = (group(Family::Holes)?, group(Family::DoubleDBores)?);
    for (i, faces) in holes.faces.iter().enumerate() {
        let by = winners(&bores, |bore| {
            !faces.is_empty() && faces.is_subset(bore) && faces.len() < bore.len()
        });
        if !by.is_empty() {
            decisions.push(
                &holes,
                i,
                ReasonCode::HoleSupersededByDoubleDBore,
                &bores,
                by,
            );
        }
    }

    // A boss whose faces are exactly a turned step's (`reconcile_boss_turned_step_candidates`).
    let (bosses, turned) = (group(Family::Bosses)?, group(Family::TurnedSteps)?);
    for (i, faces) in bosses.faces.iter().enumerate() {
        let by = winners(&turned, |step| !faces.is_empty() && faces == step);
        if !by.is_empty() {
            decisions.push(
                &bosses,
                i,
                ReasonCode::BossSupersededByTurnedStep,
                &turned,
                by,
            );
        }
    }

    // A turned step and each groove whose floor lies in it, both accepted and related
    // (`reconcile_step_groove_candidates`): the steps first, then the grooves.
    let grooves = group(Family::Grooves)?;
    let related = |step: usize, groove: usize| {
        let floor = &grooves.faces[groove];
        !floor.is_empty() && floor.is_subset(&turned.faces[step])
    };
    for step in 0..turned.len() {
        let by: Vec<usize> = (0..grooves.len()).filter(|&g| related(step, g)).collect();
        if !by.is_empty() {
            decisions.push(
                &turned,
                step,
                ReasonCode::TurnedStepGrooveCompatible,
                &grooves,
                by,
            );
        }
    }
    for groove in 0..grooves.len() {
        let by: Vec<usize> = (0..turned.len()).filter(|&s| related(s, groove)).collect();
        if !by.is_empty() {
            decisions.push(
                &grooves,
                groove,
                ReasonCode::GrooveTurnedStepCompatible,
                &turned,
                by,
            );
        }
    }

    // A passage whose walls are exactly an oriented slot's
    // (`reconcile_oriented_slot_passages`).
    let oriented = group(Family::OrientedSlots)?;
    for (i, walls) in passages.faces.iter().enumerate() {
        let by = winners(&oriented, |slot| !walls.is_empty() && walls == slot);
        if !by.is_empty() {
            decisions.push(
                &passages,
                i,
                ReasonCode::PassageSupersededByOrientedSlot,
                &oriented,
                by,
            );
        }
    }

    // A plate, boss or riser inside a thin-wall body's skins (`reconcile_thin_wall_candidates`),
    // unless an earlier rule has already decided it.
    let walls = group(Family::ThinWallBodies)?;
    let (plates, risers) = (group(Family::Plates)?, group(Family::Risers)?);
    let mut thin = Decisions::default();
    for (subject, reason) in [
        (&plates, ReasonCode::PlateSupersededByThinWall),
        (&bosses, ReasonCode::BossSupersededByThinWall),
        (&risers, ReasonCode::RiserSupersededByThinWall),
    ] {
        for (i, faces) in subject.faces.iter().enumerate() {
            let by = winners(&walls, |wall| !faces.is_empty() && faces.is_subset(wall));
            if !by.is_empty() && !decisions.decided(subject.family, i) {
                thin.push(subject, i, reason, &walls, by);
            }
        }
    }
    decisions.0.extend(thin.0);
    completed(decisions.0)
}
}

/// The indices of *group*'s candidates whose faces satisfy *wins*, in order.
fn winners(group: &Group, wins: impl Fn(&Faces) -> bool) -> Vec<usize> {
    (0..group.len())
        .filter(|&i| wins(&group.faces[i]))
        .collect()
}

/// Passages pooled under one slot grouping key (axis and section): their indices and walls.
type Pool<'a> = ((&'static str, &'a [V2]), Vec<usize>, Faces);

/// The recess families' cascading precedence (`reconcile_recess_candidates`).
#[allow(clippy::too_many_arguments)]
fn recesses(
    decisions: &mut Decisions,
    slots: &Group,
    pockets: &Group,
    prismatic: &Group,
    passages: &Group,
    blind_slots: &Group,
    open_circular: &Group,
    prismatic_sides: &[usize],
    evidence: &Evidence,
) {
    // Keep the historical sharp-floor precedence. Floor-treated pockets have
    // additional defining faces and retire their ring only after acceptance below.
    let mut accepted_prismatic = Vec::new();
    for (i, ring) in prismatic.faces.iter().enumerate() {
        let by = winners(pockets, |walls| {
            prismatic_sides[i] == 4 && !walls.is_empty() && walls.is_subset(ring)
        })
        .into_iter()
        .filter(|&j| evidence.pocket_floor_fillets[j].is_empty())
        .collect::<Vec<_>>();
        if by.is_empty() {
            accepted_prismatic.push(i);
        } else {
            decisions.push(
                prismatic,
                i,
                ReasonCode::PrismaticSupersededByPocket,
                pockets,
                by,
            );
        }
    }
    let nonrect_prismatic: Vec<usize> = accepted_prismatic
        .iter()
        .copied()
        .filter(|&i| prismatic_sides[i] != 4)
        .collect();

    // A pocket yields, in turn, to an edge-open circular pocket, a rectangular blind slot (by
    // its constituent faces), a passage or a non-rectangular ring containing it.
    let mut accepted_pockets = Vec::new();
    for (i, walls) in pockets.faces.iter().enumerate() {
        if walls.is_empty() {
            accepted_pockets.push(i);
            continue;
        }
        let members: Faces = evidence.pocket_constituent[i].iter().copied().collect();
        let contains = |other: &Faces| walls.is_subset(other);
        let candidates: [(&Group, Vec<usize>, ReasonCode); 4] = [
            (
                open_circular,
                winners(open_circular, contains),
                ReasonCode::PocketSupersededByEdgeOpenCircularPocket,
            ),
            (
                blind_slots,
                winners(blind_slots, |slot| {
                    !members.is_empty() && members.is_subset(slot)
                }),
                ReasonCode::PocketSupersededByRectangularBlindSlot,
            ),
            (
                passages,
                winners(passages, contains),
                ReasonCode::PocketSupersededByPassage,
            ),
            (
                prismatic,
                nonrect_prismatic
                    .iter()
                    .copied()
                    .filter(|&r| contains(&prismatic.faces[r]))
                    .collect(),
                ReasonCode::PocketSupersededByPrismatic,
            ),
        ];
        match candidates.into_iter().find(|(_, by, _)| !by.is_empty()) {
            Some((group, by, reason)) => decisions.push(pockets, i, reason, group, by),
            None => accepted_pockets.push(i),
        }
    }

    // A complete floor-treated pocket defines more than its planar wall seed.
    // Its four-sided ring is subordinate only when that pocket actually survives.
    accepted_prismatic.retain(|&i| {
        let ring = &prismatic.faces[i];
        let by: Vec<_> = accepted_pockets
            .iter()
            .copied()
            .filter(|&j| {
                let walls = &pockets.faces[j];
                prismatic_sides[i] == 4
                    && !ring.is_empty()
                    && !evidence.pocket_floor_fillets[j].is_empty()
                    && ring.is_subset(walls)
            })
            .collect();
        if by.is_empty() {
            true
        } else {
            decisions.push(
                prismatic,
                i,
                ReasonCode::PrismaticSupersededByPocket,
                pockets,
                by,
            );
            false
        }
    });

    // Non-rectangular passages' walls pooled by their exact (axis, section) grouping key, in
    // first-seen order; four-sided passages are tested on their own below.
    let groupings: Vec<Option<(&'static str, &[V2], usize)>> = evidence
        .passage_compatibility
        .iter()
        .map(grouping_from_view)
        .collect();
    let mut pooled: Vec<Pool> = Vec::new();
    for (i, grouping) in groupings.iter().enumerate() {
        let Some((axis, section, sides)) = *grouping else {
            continue;
        };
        if sides == 4 {
            continue;
        }
        let at = match pooled.iter().position(|(key, ..)| *key == (axis, section)) {
            Some(at) => at,
            None => {
                pooled.push(((axis, section), Vec::new(), Faces::new()));
                pooled.len() - 1
            }
        };
        pooled[at].1.push(i);
        pooled[at].2.extend(passages.faces[i].iter().copied());
    }

    // A slot yields, in turn, to an accepted pocket, an accepted ring, or every passage of each
    // pooled grouping containing its walls.
    let mut accepted_slots = Vec::new();
    for (i, walls) in slots.faces.iter().enumerate() {
        if walls.is_empty() {
            accepted_slots.push(i);
            continue;
        }
        let in_accepted = |group: &Group, accepted: &[usize]| -> Vec<usize> {
            accepted
                .iter()
                .copied()
                .filter(|&j| walls.is_subset(&group.faces[j]))
                .collect()
        };
        let mut by_passages: Vec<usize> = pooled
            .iter()
            .filter(|(_, _, faces)| walls.is_subset(faces))
            .flat_map(|(_, members, _)| members.iter().copied())
            .collect();
        // Python lists the winners pool by pool; the disposition orders them by candidate.
        by_passages.sort_unstable();
        by_passages.dedup();
        let candidates: [(&Group, Vec<usize>, ReasonCode); 3] = [
            (
                pockets,
                in_accepted(pockets, &accepted_pockets),
                ReasonCode::SlotSupersededByPocket,
            ),
            (
                prismatic,
                in_accepted(prismatic, &accepted_prismatic),
                ReasonCode::SlotSupersededByPrismatic,
            ),
            (passages, by_passages, ReasonCode::SlotSupersededByPassage),
        ];
        match candidates.into_iter().find(|(_, by, _)| !by.is_empty()) {
            Some((group, by, reason)) => decisions.push(slots, i, reason, group, by),
            None => accepted_slots.push(i),
        }
    }

    // A four-sided passage yields to the accepted slot that dimensions it.
    for (i, grouping) in groupings.iter().enumerate() {
        if !matches!(grouping, Some((_, _, 4))) {
            continue;
        }
        let by: Vec<usize> = accepted_slots
            .iter()
            .copied()
            .filter(|&s| !slots.faces[s].is_empty() && slots.faces[s].is_subset(&passages.faces[i]))
            .collect();
        if !by.is_empty() {
            decisions.push(passages, i, ReasonCode::PassageSupersededBySlot, slots, by);
        }
    }
}

/// The decisions, refused as `ReconciliationResult.complete` refuses them: one candidate
/// decided twice, or a reason on the wrong families or outcome.
fn completed(decisions: Vec<Disposition>) -> Result<Vec<Disposition>, ReconcileError> {
    for (at, d) in decisions.iter().enumerate() {
        let (outcome, subject, related) = d.reason.spec();
        if decisions[..at].iter().any(|e| {
            e.candidate.family == d.candidate.family && e.candidate.index == d.candidate.index
        }) {
            return Err(ReconcileError(
                "physical candidate received more than one disposition".into(),
            ));
        }
        if d.outcome != outcome || d.candidate.family != subject {
            return Err(ReconcileError(
                "disposition reason does not match its candidate".into(),
            ));
        }
        if d.related.is_empty() || d.related.iter().any(|r| r.family != related) {
            return Err(ReconcileError(
                "disposition reason has invalid related candidates".into(),
            ));
        }
    }
    Ok(decisions)
}
