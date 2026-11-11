//! Records with the faces that prove them, and the check that publishes them only when every
//! defining and consulted face belongs to one valid solid (`common_valid_solid`).

use std::cell::Cell;

use crate::kernel::brep::Part;
use serde::{Deserialize, Serialize};

/// The completed inventory's mode, retained after the retry's thread-local flag is restored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum RunMode {
    /// Missing from documents written before the trust contract.
    #[default]
    Unknown,
    Strict,
    LocallyDegraded,
}

/// Strict valid-solid evidence for a published record. Missing proof is never verification.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceStatus {
    Verified,
    #[default]
    Unproved,
}

internal_entry! {
/// Check the same defining-and-consulted-face ownership as [`verified`], always strictly.
/// Local degradation may admit candidates, but cannot upgrade invalid geometry's status.
pub fn strict_status(part: &Part, defining: &[usize], context: &[usize]) -> EvidenceStatus {
    if !defining.is_empty()
        && strict_valid_solid(part, defining.iter().chain(context).copied()).is_some()
    {
        EvidenceStatus::Verified
    } else {
        EvidenceStatus::Unproved
    }
}
}

/// The strict branch shared by the public evidence path and document statuses.
fn strict_valid_solid(part: &Part, mut faces: impl Iterator<Item = usize>) -> Option<usize> {
    let owner = part.faces.get(faces.next()?)?.solid?;
    (part.solid_is_geometrically_valid(owner)
        && faces.all(|f| {
            part.faces
                .get(f)
                .is_some_and(|face| face.solid == Some(owner))
        }))
    .then_some(owner)
}

/// A recognised record, the faces that define it, and the faces consulted to prove it.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Occurrence<R> {
    pub record: R,
    pub defining: Vec<usize>,
    pub context: Vec<usize>,
}

impl<R> Occurrence<R> {
    pub fn new(record: R, defining: Vec<usize>, context: Vec<usize>) -> Self {
        Self {
            record,
            defining,
            context,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EvidenceError {
    /// A defining face has no single valid solid shared with the rest of its evidence.
    NoValidSolid,
    /// Two occurrences claim one defining face, or one face both defines and is consulted.
    SharedEvidence,
    /// One turned profile key would identify two valid solids.
    AmbiguousProfile,
    /// Two obround cap clusters compete for one end of a slot or pocket, so which faces end it
    /// is undecided.
    CompetingCaps,
    /// A pocket found from opposed walls or obround caps has no floor faces to consult.
    MissingFloor,
}

impl std::fmt::Display for EvidenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvidenceError::NoValidSolid => {
                write!(f, "defining face has no unambiguous valid solid")
            }
            EvidenceError::SharedEvidence => {
                write!(f, "evidence faces are shared between occurrences")
            }
            EvidenceError::AmbiguousProfile => {
                write!(f, "turned profile key identifies multiple valid solids")
            }
            EvidenceError::CompetingCaps => {
                write!(f, "multiple obround cap clusters compete for one endpoint")
            }
            EvidenceError::MissingFloor => write!(f, "pocket floor faces are unavailable"),
        }
    }
}

impl std::error::Error for EvidenceError {}

thread_local! {
    /// Whether this thread is inside a locally degraded run (Python's
    /// `FaceGraph.local_degradation`, a property of the whole run's graph): set only by
    /// [`locally_degraded`], which the aggregate's retry (`super::recognise` and the other
    /// whole-inventory entry points) runs its second inventory under.
    static LOCAL_DEGRADATION: Cell<bool> = const { Cell::new(false) };
}

internal_entry! {
/// Whether the current run is locally degraded (`locally_degraded`, the aggregate's retry).
pub fn local_degradation() -> bool {
    LOCAL_DEGRADATION.with(Cell::get)
}
}

/// Run *f* as a locally degraded run: every [`common_valid_solid`] query inside it answers as
/// Python's degraded graph does ([`locally_valid_solid`]). The previous mode is restored
/// afterwards, also if *f* panics. Runs nest; recognition makes no threads of its own, so the
/// mode reaches all of the run.
pub(crate) fn locally_degraded<T>(f: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            LOCAL_DEGRADATION.with(|mode| mode.set(self.0));
        }
    }
    let _restore = Restore(LOCAL_DEGRADATION.with(|mode| mode.replace(true)));
    f()
}

internal_entry! {
/// The one valid solid every face belongs to (`FaceGraph.common_valid_solid`). Valid as
/// Python's `solid.is_valid` (`BRepCheck_Analyzer`) reads it: sound topology and no faulted face
/// (`Part::solid_is_geometrically_valid`). Inside a locally degraded run ([`local_degradation`])
/// it is [`locally_valid_solid`], as Python's graph answers in that mode.
pub fn common_valid_solid(part: &Part, faces: &[usize]) -> Option<usize> {
    if local_degradation() {
        return locally_valid_solid(part, faces);
    }
    strict_valid_solid(part, faces.iter().copied())
}
}

/// Most faulted faces a locally degraded run admits a solid with.
const MAX_BAD_FACES: usize = 3;

internal_entry! {
/// [`common_valid_solid`] as Python's locally degraded run proves it (`local_degradation`): a
/// solid with faulted faces is also admitted when its topology is sound, it has one shell and a
/// positive volume and at most three faulted faces, but then no face may be a faulted one or
/// share an edge with one.
pub fn locally_valid_solid(part: &Part, faces: &[usize]) -> Option<usize> {
    let owner = part.faces[*faces.first()?].solid?;
    if !faces.iter().all(|&f| part.faces[f].solid == Some(owner)) || !part.solid_is_valid(owner) {
        return None;
    }
    let bad = part.bad_faces(owner);
    if bad.is_empty() {
        return Some(owner);
    }
    let admitted = bad.len() <= MAX_BAD_FACES
        && part.shell_count(owner) == 1
        && part
            .solid_mass(owner)
            .is_some_and(|(volume, _)| volume > 0.0);
    let unsafe_face =
        |f: usize| bad.contains(&f) || part.neighbours(f).iter().any(|n| bad.contains(n));
    (admitted && !faces.iter().any(|&f| unsafe_face(f))).then_some(owner)
}
}

internal_entry! {
/// Whether a locally degraded run proves *o*: it has defining faces, and its defining and
/// consulted faces together prove one solid as [`locally_valid_solid`] does. The holes', pockets'
/// and fillets' check on a degraded graph (`_discover_holes`, `_discover_pockets`,
/// `_discover_fillets`), each skipping the occurrences it fails.
pub fn locally_proved<R>(part: &Part, o: &Occurrence<R>) -> bool {
    let faces: Vec<usize> = o.defining.iter().chain(&o.context).copied().collect();
    !o.defining.is_empty() && locally_valid_solid(part, &faces).is_some()
}
}

internal_entry! {
/// All occurrences, or none: each one's faces must share one valid solid.
pub fn verified<R>(
    part: &Part,
    found: Vec<Occurrence<R>>,
) -> Result<Vec<Occurrence<R>>, EvidenceError> {
    for o in &found {
        let faces: Vec<usize> = o.defining.iter().chain(&o.context).copied().collect();
        if o.defining.is_empty() || common_valid_solid(part, &faces).is_none() {
            return Err(EvidenceError::NoValidSolid);
        }
    }
    Ok(found)
}
}

#[cfg(test)]
mod trust_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn strict_status_checks_consulted_faces_and_never_relaxes_in_a_retry() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rejected_cylinder.step");
        let mut part = crate::read_step_file(&path).unwrap();
        assert_eq!(strict_status(&part, &[0], &[1]), EvidenceStatus::Verified);
        // A consulted face without the defining face's solid invalidates the claim.
        part.faces[1].solid = None;
        assert_eq!(strict_status(&part, &[0], &[1]), EvidenceStatus::Unproved);
        assert_eq!(strict_status(&part, &[], &[0]), EvidenceStatus::Unproved);
        assert_eq!(
            strict_status(&part, &[usize::MAX], &[]),
            EvidenceStatus::Unproved
        );
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/broken/unresolved_surface.step");
        let part = crate::read_step_file(&path).unwrap();
        locally_degraded(|| {
            assert!(local_degradation());
            assert_eq!(strict_status(&part, &[0], &[]), EvidenceStatus::Unproved);
        });
        assert!(!local_degradation());
    }
}
