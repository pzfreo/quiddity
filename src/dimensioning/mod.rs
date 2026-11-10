//! Requirement planning for prismatic holes, patterns and support-plane levels.
//!
//! Ported and adapted from pzfreo/draftwright (same copyright owner),
//! `src/dims/holes.rs` (wants, lattice, line, pitch, consolidation),
//! `src/bridge/holes.rs::established_with`, `src/dims/levels.rs` (ownership, shoulders, repeats),
//! the step-section ownership boundary in `src/dims/steps.rs`,
//! `src/dims/pmi_policy.rs` (datum origins), and `src/{dims,lint}/redundancy.rs`
//! (physical intervals and bounded station walks), origin/main 1c830977.
//! View selection, labels, layout and editor schemes are deliberately absent.
//! See `docs/dimensioning.md` for the input contract and slice boundary.

mod levels;
mod recognition;
mod rules;
#[cfg(test)]
mod tests;

use crate::kernel::pmi;
use serde::{Deserialize, Serialize};

/// Persistent within a part's face namespace; never a recognition vector index.
pub type Id = String;
pub type FeatureId = String;
pub type Point = [f64; 3];

/// A physical support in the input's part frame. Plane identities include all coplanar faces.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Support {
    pub feature: FeatureId,
    pub faces: Vec<usize>,
    pub body: Option<usize>,
    pub at: Point,
    pub plane: bool,
}

/// Hole specifications preserve every machined quantity, independently of printed callouts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hole {
    pub support: Support,
    pub direction: Point,
    pub diameter: f64,
    /// None for through holes: their material traversal length is not a size requirement.
    pub depth: Option<f64>,
    /// Through material segment length, used only to count drilling across narrow slits.
    pub traversal: Option<f64>,
    pub bottom: crate::features::holes::Bottom,
    pub details: Vec<Size>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Size {
    /// e.g. bore.diameter, counterbore.depth, countersink.angle.
    pub role: String,
    pub nominal: f64,
    pub kind: SizeKind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SizeKind {
    Diameter,
    Length,
    Angle,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pattern {
    pub members: Vec<FeatureId>,
    pub arrangement: Arrangement,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arrangement {
    Grid,
    Line,
    BoltCircle { centre: Point, diameter: f64 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plane {
    pub support: Support,
    pub axis: usize,
    /// Separate solids never acquire a common plane interval owner.
    pub body: usize,
}
/// Recognised step supports after feature ownership, in the part's height direction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StepLevels {
    pub axis: usize,
    pub heights: Vec<Support>,
    pub shoulders: Vec<Plane>,
}
impl Default for StepLevels {
    fn default() -> Self {
        Self {
            axis: 2,
            heights: vec![],
            shoulders: vec![],
        }
    }
}
/// An explicit boundary of this slice, anchored to physical faces where available.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deferred {
    pub id: Id,
    pub faces: Vec<usize>,
    pub family: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Datum {
    pub label: String,
    pub faces: Vec<usize>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    #[default]
    Automatic,
    Minimum,
    Maximum,
    Datum(String),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policy {
    pub origins: [Origin; 3],
    pub general_tolerance: bool,
    /// Keep determined, untoleranced intervals as references rather than conveying them away.
    pub references: bool,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            origins: std::array::from_fn(|_| Origin::Automatic),
            general_tolerance: true,
            references: false,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tolerance {
    Untoleranced,
    General,
    Basic,
    /// An opaque exact tolerance/precision contract supplied by the consumer.
    Explicit(String),
}

/// Typed input; all positions and axes are in `frame` coordinates, lengths in millimetres.
/// `specified` contains already stated requirements; these win over generated equivalents.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Input {
    pub frame: crate::frames::PartFrame,
    pub bounds: [Point; 2],
    pub holes: Vec<Hole>,
    pub patterns: Vec<Pattern>,
    /// All principal planes, including deferred ones, remain available to datum lookup.
    pub planes: Vec<Plane>,
    pub steps: StepLevels,
    /// False when the envelope belongs to a later-slice round/turned/stock owner.
    pub cartesian_envelope: bool,
    pub deferred: Vec<Deferred>,
    pub datums: Vec<Datum>,
    /// Round axes may corroborate small bolt circles; they are not dimensioned in slice 1.
    pub round_axes: Vec<(Point, Point)>,
    pub specified: Vec<Required>,
    pub policy: Policy,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Size {
        feature: FeatureId,
        role: String,
        kind: SizeKind,
    },
    Location {
        from: Support,
        to: Support,
        axis: usize,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternRole {
    FirstMember,
    Centre,
    Pitch { intervals: usize },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Required {
    pub id: Id,
    pub kind: Kind,
    pub nominal: f64,
    pub count: usize,
    pub pattern: Option<PatternRole>,
    pub tolerance: Tolerance,
    pub reference: Option<Vec<Id>>,
    /// All quantities stated by this requirement, including displaced coincident claims.
    pub covers: Vec<FeatureId>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conveyed {
    pub id: Id,
    /// Empty only for explicit later-slice deferrals (why starts with "deferred:").
    pub by: Vec<Id>,
    pub why: String,
}

impl Required {
    /// Lower into a consumer's PMI feature table. Stable planner ids are separate from the
    /// kernel's arena indices. Angle sizes use Equal selection; nominals remain in degrees.
    pub fn dimension_kind(
        &self,
        mut feature: impl FnMut(&str) -> pmi::FeatureId,
    ) -> pmi::DimensionKind {
        match &self.kind {
            Kind::Size {
                feature: id, kind, ..
            } => pmi::DimensionKind::Size {
                feature: feature(id),
                kind: if *kind == SizeKind::Diameter {
                    pmi::SizeKind::Diameter
                } else {
                    pmi::SizeKind::Other(
                        if *kind == SizeKind::Angle {
                            "angle"
                        } else {
                            "length"
                        }
                        .into(),
                    )
                },
                path: None,
                angle: (*kind == SizeKind::Angle).then_some(pmi::AngleSelection::Equal),
            },
            Kind::Location { from, to, .. } => pmi::DimensionKind::Location {
                from: feature(&from.feature),
                to: feature(&to.feature),
                kind: pmi::LocationKind::LinearDistance,
                path: None,
                directed: false,
                angle: None,
            },
        }
    }
}
impl Pattern {
    pub fn id(&self) -> FeatureId {
        rules::group_id(&self.members)
    }
    /// Populate the consumer's feature arena without leaking its indices into planner ids.
    pub fn pmi_feature(&self, mut member: impl FnMut(&str) -> pmi::FeatureId) -> pmi::Feature {
        let mut members = self.members.clone();
        members.sort();
        members.dedup();
        pmi::Feature::Group {
            members: members.iter().map(|m| member(m)).collect(),
            kind: pmi::GroupKind::PatternOfFeatures,
        }
    }
}

/// Deterministic, view- and wording-independent planning. No input or recognition is mutated.
pub fn plan(input: &Input) -> (Vec<Required>, Vec<Conveyed>) {
    rules::plan(input)
}
