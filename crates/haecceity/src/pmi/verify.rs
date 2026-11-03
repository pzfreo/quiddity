//! Read-back verification of a PMI write: [`verify()`] compares a file read back after
//! [`write()`](super::write()) with the file it was written into and the PMI written, and says
//! exactly what differs ([`Problem`]), not only whether anything does.

use std::collections::BTreeSet;
use std::fmt;

use super::write::{Mode, differences_as_stated};
use super::{Finding, PartId, PartPmi, PmiRead};
use crate::p21::Document;
use crate::step::PartDefinition;

/// One file as read for verification: its document, its parts and its PMI read from them.
#[derive(Clone, Copy)]
pub struct Snapshot<'a> {
    pub doc: &'a Document,
    pub defs: &'a [PartDefinition],
    pub read: &'a PmiRead,
}

/// One way the file read back differs from what the write should have made of it. Differences
/// are named as [`differences_as_stated`] names them ("tolerance only in the second: …"); the
/// `Display` form is `quiddity pmi write`'s message for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The file read back has another number of parts (nothing else is compared then).
    PartCount { before: usize, after: usize },
    /// A part's name, face count or edge count changed.
    PartChanged { part: usize },
    /// A part not written reads back differently.
    NotWrittenPart { part: usize, difference: String },
    /// Add: something the part had before is gone or changed.
    Lost { part: usize, difference: String },
    /// Add: an item read back that was neither there before nor written.
    Unexpected { part: usize, item: String },
    /// Add: an item written that does not read back.
    Missing { part: usize, item: String },
    /// Replace or remove: the part's PMI read back differs from what was written.
    Differs { part: usize, difference: String },
    /// Replace or remove: an instance the reader consumed for the part's PMI before is still in
    /// the file (its supplemental geometry excepted, which is the part's).
    Survives { part: usize, id: u64 },
    /// A finding the file read back has and the input did not.
    NewFinding(Finding),
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Problem::PartCount { before, after } => {
                write!(f, "{after} parts read back, not {before}")
            }
            Problem::PartChanged { part } => {
                write!(f, "part {part}'s name, faces or edges changed")
            }
            Problem::NotWrittenPart { part, difference } => {
                write!(f, "part {part} (not written): {difference}")
            }
            Problem::Lost { part, difference } => write!(f, "part {part}: lost: {difference}"),
            Problem::Unexpected { part, item } => write!(f, "part {part}: not written: {item}"),
            Problem::Missing { part, item } => {
                write!(f, "part {part}: written but not read back: {item}")
            }
            Problem::Differs { part, difference } => write!(f, "part {part}: {difference}"),
            Problem::Survives { part, id } => {
                write!(f, "part {part}: #{id}, which the reader consumed, survives")
            }
            Problem::NewFinding(finding) => write!(
                f,
                "a finding the input did not have: {} {:?} {}: {}",
                finding.kind.as_str(),
                finding.ids,
                finding.entity,
                finding.detail
            ),
        }
    }
}

/// What [`verify()`] found: empty when the file read back is what the write should have made.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Verification {
    pub problems: Vec<Problem>,
}

impl Verification {
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.problems.is_empty()
    }

    /// `Ok`, or every problem's message joined by `"; "`.
    pub fn into_result(self) -> Result<(), String> {
        if self.problems.is_empty() {
            return Ok(());
        }
        Err(self
            .problems
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; "))
    }
}

/// The item keys of a part's PMI as [`differences_as_stated`] names them ("tolerance only in
/// the second: …"): every reference resolved, values as stated.
fn item_keys(p: &PartPmi) -> Vec<String> {
    differences_as_stated(&PartPmi::default(), p)
}

/// Checks the file read back (`after`) against the file written into (`before`) and the PMI
/// `written` with `mode`: each written part reads back as written (replace, remove) or as its
/// PMI before plus exactly the items written (add; a feature or datum equal to one the part
/// has is that one), and for replace and remove nothing the reader consumed for it survives
/// (except supplemental geometry and auxiliary support still used by unread PMI); every other part reads as
/// before; the parts keep their names and face and edge counts; and no finding is new.
///
/// `base` is the document the edit was applied to: `before.doc`, unless the caller edited a
/// copy of it (say with some PMI already cleared). A consumed instance survives when `base`
/// still has it as `before.doc` states it and `after` has it, so an instance the edit's base no
/// longer had, whose id what the edit added may reuse, does not count.
///
/// In add mode `written` is what was handed to the writer: a standard the part already states
/// must not be in it (the reader reads the part's standards as a set, so a repeated one reads
/// back once).
#[must_use]
pub fn verify(
    before: Snapshot<'_>,
    base: &Document,
    written: &[(PartId, PartPmi)],
    mode: Mode,
    after: Snapshot<'_>,
) -> Verification {
    let mut problems: Vec<Problem> = Vec::new();
    let (defs, after_defs) = (before.defs, after.defs);
    if after_defs.len() != defs.len() {
        problems.push(Problem::PartCount {
            before: defs.len(),
            after: after_defs.len(),
        });
        return Verification { problems };
    }
    for (part, (d0, d1)) in defs.iter().zip(after_defs).enumerate() {
        if d0.name != d1.name
            || d0.faces.len() != d1.faces.len()
            || d0.edges.len() != d1.edges.len()
        {
            problems.push(Problem::PartChanged { part });
        }
    }
    fn text(doc: &Document, id: u64) -> Option<&[u8]> {
        doc.span(id).map(|s| &doc.bytes()[s])
    }
    let consumed = written
        .iter()
        .flat_map(|(part, _)| before.read.provenance.parts[part.0].all())
        .collect();
    let shared = super::write::shared_dependencies(before.doc, &consumed);
    for part in 0..defs.len() {
        let old = &before.read.parts[part];
        let read = &after.read.parts[part];
        match written.iter().find(|(p, _)| p.0 == part) {
            None => problems.extend(
                differences_as_stated(old, read)
                    .into_iter()
                    .map(|difference| Problem::NotWrittenPart { part, difference }),
            ),
            Some((_, new)) if mode == Mode::Add => {
                let shared: BTreeSet<String> = item_keys(old)
                    .into_iter()
                    .filter(|k| {
                        k.starts_with("feature only in the second: ")
                            || k.starts_with("datum only in the second: ")
                    })
                    .collect();
                let mut expected: Vec<String> = item_keys(&super::write::standard_form(new))
                    .into_iter()
                    .filter(|k| !shared.contains(k))
                    .collect();
                for d in differences_as_stated(old, read) {
                    if !d.contains(" only in the second: ") {
                        problems.push(Problem::Lost {
                            part,
                            difference: d,
                        });
                    } else if let Some(at) = expected.iter().position(|k| *k == d) {
                        expected.remove(at);
                    } else {
                        problems.push(Problem::Unexpected { part, item: d });
                    }
                }
                problems.extend(
                    expected
                        .into_iter()
                        .map(|item| Problem::Missing { part, item }),
                );
            }
            Some((_, new)) => {
                problems.extend(
                    differences_as_stated(&super::write::standard_form(new), read)
                        .into_iter()
                        .map(|difference| Problem::Differs { part, difference }),
                );
                let prov = &before.read.provenance.parts[part];
                let geometry: BTreeSet<u64> = prov.geometry.iter().flatten().copied().collect();
                for id in prov.all() {
                    if !geometry.contains(&id)
                        && !shared.contains(&id)
                        && text(base, id) == text(before.doc, id)
                        && after.doc.get(id).is_some()
                    {
                        problems.push(Problem::Survives { part, id });
                    }
                }
            }
        }
    }
    for f in &after.read.findings {
        if !before.read.findings.contains(f)
            && !(f.kind == super::FindingKind::Unconsumed
                && f.ids.first().is_some_and(|id| shared.contains(id)))
        {
            problems.push(Problem::NewFinding(f.clone()));
        }
    }
    Verification { problems }
}
