//! `pmi::verify`, the read-back check of a write (`quiddity pmi write` calls it): add reads back
//! as the part's PMI before plus exactly the items written (a feature or datum equal to one the
//! part has being that one), replace as exactly the items written with nothing the reader
//! consumed surviving, other parts and the findings as before; and what differs is named.

mod common;

use std::io::Read;

use haecceity::express;
use haecceity::p21::{Document, RawEntity};
use haecceity::pmi::write::{Mode, PresentationPolicy};
use haecceity::pmi::*;
use haecceity::step::{PartDefinition, read_part_definitions};

/// A file as read for verification.
struct File {
    doc: Document,
    defs: Vec<PartDefinition>,
    read: PmiRead,
}

impl File {
    fn from_bytes(bytes: Vec<u8>) -> File {
        let defs = read_part_definitions(&bytes).expect("part definitions");
        let doc = Document::parse(bytes).expect("document");
        let read = read(&doc, &defs).expect("PMI read");
        File { doc, defs, read }
    }

    fn fixture(rel: &str) -> File {
        let raw = std::fs::read(common::fixtures().join(rel)).unwrap();
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(raw.as_slice())
            .read_to_end(&mut bytes)
            .unwrap();
        File::from_bytes(bytes)
    }

    fn snapshot(&self) -> Snapshot<'_> {
        Snapshot::new(&self.doc, &self.defs, &self.read)
    }

    /// The file with `pmi` written by `mode`, read back.
    fn written(&self, pmi: &[(PartId, PartPmi)], mode: Mode) -> File {
        let (edit, _) = write(
            &self.doc,
            &self.defs,
            pmi,
            mode,
            PresentationPolicy::RemovePresentation,
        )
        .unwrap_or_else(|e| panic!("not written: {e}"));
        File::from_bytes(self.doc.apply(&edit).unwrap().bytes)
    }
}

fn verified(
    before: &File,
    written: &[(PartId, PartPmi)],
    mode: Mode,
    after: &File,
) -> Verification {
    verify(
        before.snapshot(),
        &before.doc,
        written,
        mode,
        after.snapshot(),
    )
}

fn faces(f: &[usize]) -> Feature {
    Feature::Items(f.iter().map(|&i| Anchor::Face(FaceIndex(i))).collect())
}

fn datum(l: &str, f: usize) -> Datum {
    Datum::new(DatumLabel::new(l).unwrap(), Some(FeatureId(f)), Vec::new()).unwrap()
}

fn tolerance(
    kind: ToleranceKind,
    feature: usize,
    magnitude: &str,
    datum: Option<usize>,
) -> GeometricTolerance {
    GeometricTolerance {
        kind,
        target: ToleranceTarget::Feature(FeatureId(feature)),
        magnitude: Some(Value::length(
            Decimal::parse(magnitude).unwrap(),
            LengthUnit::Millimetre,
        )),
        zone: None,
        modifiers: Vec::new(),
        unit_basis: None,
        maximum: None,
        unequal: None,
        datums: datum.map(|d| {
            DatumSystem::new(vec![Compartment {
                references: vec![DatumReference {
                    datum: DatumId(d),
                    modifiers: Vec::new(),
                }],
                modifiers: Vec::new(),
            }])
            .unwrap()
        }),
        auxiliary: Vec::new(),
        description: None,
    }
}

/// The first *n* faces of part *part* no PMI names.
fn free_faces(f: &File, part: usize, n: usize) -> Vec<usize> {
    let used = |face: u64| {
        f.doc.referrers(face).iter().any(|&r| {
            let names: Vec<String> = match f.doc.get(r).unwrap() {
                RawEntity::Simple { name, .. } => vec![name.to_ascii_lowercase()],
                RawEntity::Complex { parts, .. } => {
                    parts.iter().map(|p| p.name.to_ascii_lowercase()).collect()
                }
            };
            names
                .iter()
                .any(|n| express::is_a(n, "item_identified_representation_usage"))
        })
    };
    let out: Vec<usize> = (0..f.defs[part].faces.len())
        .filter(|&i| !used(f.defs[part].faces[i]))
        .take(n)
        .collect();
    assert_eq!(out.len(), n, "not enough free faces");
    out
}

/// What specify-core adds to its spool: datum A again (its existing feature, equal to the
/// part's), a free face, and a parallelism of that face to A.
fn spool_addition(f: &File) -> PartPmi {
    let old = &f.read.parts[0];
    let a = old
        .datums
        .iter()
        .find(|d| d.label().as_str() == "A")
        .expect("datum A");
    let fa = old.features[a.feature().unwrap().0].clone();
    PartPmi {
        features: vec![fa, faces(&free_faces(f, 0, 1))],
        datums: vec![datum("A", 0)],
        tolerances: vec![tolerance(ToleranceKind::Parallelism, 1, "0.04", Some(0))],
        ..PartPmi::default()
    }
}

/// Add: the spool reads back as before plus the new face and the parallelism; datum A and its
/// feature, equal to the part's own, are the part's and not expected twice.
#[test]
fn add_reads_back_as_before_plus_exactly_the_items_written() {
    let f = File::fixture("ap242/specify/spool_fits.step.gz");
    let add = vec![(PartId(0), spool_addition(&f))];
    let w = f.written(&add, Mode::Add);
    assert_eq!(verified(&f, &add, Mode::Add, &w), Verification::default());
    assert_eq!(
        w.read.parts[0].datums.len(),
        f.read.parts[0].datums.len(),
        "datum A is the part's"
    );

    // An item claimed written that was not: named as written but not read back.
    let mut claimed = add.clone();
    claimed[0]
        .1
        .tolerances
        .push(tolerance(ToleranceKind::Flatness, 1, "0.02", None));
    let v = verified(&f, &claimed, Mode::Add, &w);
    match &v.problems[..] {
        [Problem::Missing { part: 0, item }] => {
            assert!(item.starts_with("tolerance only in the second: "), "{item}");
        }
        other => panic!("{other:?}"),
    }
    let message = v.clone().into_result().unwrap_err();
    assert!(
        message.starts_with("part 0: written but not read back: tolerance only in the second: "),
        "{message}"
    );

    // An item read back that was not claimed written: named as not written.
    let mut fewer = add.clone();
    fewer[0].1.tolerances.clear();
    let v = verified(&f, &fewer, Mode::Add, &w);
    match &v.problems[..] {
        [Problem::Unexpected { part: 0, item }] => {
            assert!(item.starts_with("tolerance only in the second: "), "{item}");
            assert_eq!(
                v.problems[0].to_string(),
                format!("part 0: not written: {item}")
            );
        }
        other => panic!("{other:?}"),
    }

    // The part's own datum dropped from the file: lost, and the add's datum A is not there.
    let removed = f.written(&[(PartId(0), PartPmi::default())], Mode::Remove);
    let v = verified(&f, &add, Mode::Add, &removed);
    assert!(
        v.problems
            .iter()
            .any(|p| matches!(p, Problem::Lost { part: 0, difference } if difference.starts_with("datum only in the first: "))),
        "{:?}",
        v.problems
    );
    assert!(
        v.problems
            .iter()
            .any(|p| matches!(p, Problem::Missing { part: 0, .. })),
        "{:?}",
        v.problems
    );
}

/// Replace: the part reads back as exactly the items written and nothing the reader consumed
/// for it survives; the instances the edit's base no longer had do not count.
#[test]
fn replace_reads_back_as_exactly_the_items_written() {
    let f = File::fixture("ap242/specify/spool_fits.step.gz");
    let new = vec![(PartId(0), spool_addition(&f))];
    let w = f.written(&new, Mode::Replace);
    assert_eq!(
        verified(&f, &new, Mode::Replace, &w),
        Verification::default()
    );

    // Another PMI claimed written: each difference named.
    let mut other = new.clone();
    other[0].1.tolerances[0].kind = ToleranceKind::Perpendicularity;
    let v = verified(&f, &other, Mode::Replace, &w);
    assert!(!v.problems.is_empty());
    assert!(
        v.problems
            .iter()
            .all(|p| matches!(p, Problem::Differs { part: 0, .. })),
        "{:?}",
        v.problems
    );

    // The original claimed to be the replaced file: every consumed instance survives (its
    // supplemental geometry excepted), each named.
    let consumed = &f.read.provenance.parts[0];
    let geometry: Vec<u64> = consumed.geometry.iter().flatten().copied().collect();
    let expected: Vec<u64> = consumed
        .all()
        .into_iter()
        .filter(|id| !geometry.contains(id))
        .collect();
    assert!(!expected.is_empty());
    let v = verified(&f, &new, Mode::Replace, &f);
    let survivors: Vec<u64> = v
        .problems
        .iter()
        .filter_map(|p| match p {
            Problem::Survives { part: 0, id } => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(survivors, expected);
    assert_eq!(
        Problem::Survives { part: 0, id: 7 }.to_string(),
        "part 0: #7, which the reader consumed, survives"
    );

    // With the replaced file as the edit's base, the consumed instances it no longer has are
    // not survivors, whatever the file read back holds under their ids.
    let v = verify(f.snapshot(), &w.doc, &new, Mode::Replace, f.snapshot());
    let still: Vec<u64> = v
        .problems
        .iter()
        .filter_map(|p| match p {
            Problem::Survives { id, .. } => Some(*id),
            _ => None,
        })
        .collect();
    let kept: Vec<u64> = expected
        .iter()
        .copied()
        .filter(|&id| {
            w.doc.span(id).map(|s| &w.doc.bytes()[s]) == f.doc.span(id).map(|s| &f.doc.bytes()[s])
        })
        .collect();
    assert_eq!(still, kept);
    assert!(still.len() < expected.len());
}

/// Other parts: a part not written must read back as before; the parts' count and identity,
/// and the findings, are checked too.
#[test]
fn other_parts_and_findings_are_as_before() {
    let f = File::fixture("ap242/specify/assembly_plate_pin.step.gz");
    assert!(f.defs.len() >= 2, "an assembly of two parts");
    let free = free_faces(&f, 1, 1);
    let add = vec![(
        PartId(1),
        PartPmi {
            features: vec![faces(&free)],
            tolerances: vec![tolerance(ToleranceKind::Flatness, 0, "0.01", None)],
            ..PartPmi::default()
        },
    )];
    let w = f.written(&add, Mode::Add);
    assert_eq!(verified(&f, &add, Mode::Add, &w), Verification::default());

    // Claimed written to part 0 instead: part 1 changed though not written.
    let elsewhere = vec![(PartId(0), add[0].1.clone())];
    let v = verified(&f, &elsewhere, Mode::Add, &w);
    assert!(
        v.problems
            .iter()
            .any(|p| matches!(p, Problem::NotWrittenPart { part: 1, .. })),
        "{:?}",
        v.problems
    );
    assert!(
        v.problems
            .iter()
            .any(|p| matches!(p, Problem::Missing { part: 0, .. })),
        "{:?}",
        v.problems
    );

    // A finding the input did not have.
    let mut read = w.read.clone();
    let finding = Finding::new(
        FindingKind::Unsupported,
        Some(PartId(1)),
        vec![1],
        "shape_aspect".into(),
        "made up".into(),
    );
    read.findings.push(finding.clone());
    let after = Snapshot::new(&w.doc, &w.defs, &read);
    let v = verify(f.snapshot(), &f.doc, &add, Mode::Add, after);
    assert_eq!(v.problems, [Problem::NewFinding(finding)]);
    assert_eq!(
        v.into_result().unwrap_err(),
        "a finding the input did not have: unsupported [1] shape_aspect: made up"
    );

    // A part gone: only the count is reported.
    let after = Snapshot::new(&w.doc, &w.defs[..1], &w.read);
    let v = verify(f.snapshot(), &f.doc, &add, Mode::Add, after);
    assert_eq!(
        v.problems,
        [Problem::PartCount {
            before: f.defs.len(),
            after: 1
        }]
    );
    assert_eq!(
        v.problems[0].to_string(),
        format!("1 parts read back, not {}", f.defs.len())
    );
}
