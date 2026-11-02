//! The AP242 PMI writer: written PMI reads back as written, the original file's bytes are
//! kept, and every row of the design's Anti-requirements table has its test, named after it.

mod common;

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use haecceity::express;
use haecceity::p21::{Attribute, Document, RawEntity};
use haecceity::pmi::write::{
    ItemRef, Mode, PresentationPolicy, SchemaChange, WriteError, WriteReport, differences,
    differences_as_stated,
};
use haecceity::pmi::{self, *};
use haecceity::step::{PartDefinition, read_part_definitions};

fn file_bytes(path: &Path) -> Vec<u8> {
    let raw = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    if path.extension().is_some_and(|e| e == "gz") {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(raw.as_slice())
            .read_to_end(&mut out)
            .unwrap();
        out
    } else {
        raw
    }
}

struct File {
    parts: Vec<PartDefinition>,
    doc: Document,
}

fn open(path: &Path) -> File {
    let bytes = file_bytes(path);
    let parts = read_part_definitions(&bytes).expect("part definitions");
    let doc = Document::parse(bytes).expect("document");
    File { parts, doc }
}

fn fixture(rel: &str) -> File {
    open(&common::fixtures().join(rel))
}

/// A specify-core-written part, whose PMI each test replaces with its own.
fn spool() -> File {
    fixture("ap242/specify/spool_fits.step.gz")
}

fn gap_spool() -> File {
    let f = spool();
    let mut edit = haecceity::p21::Edit::new();
    edit.set_file_schema(vec![express::TARGET_SCHEMA.into()]);
    open_bytes(f.doc.apply(&edit).unwrap().bytes)
}

fn dec(t: &str) -> Decimal {
    Decimal::parse(t).unwrap()
}

fn mm(t: &str) -> Value {
    Value::length(dec(t), LengthUnit::Millimetre)
}

fn length(t: &str, unit: LengthUnit) -> Length {
    Length {
        value: dec(t),
        unit,
    }
}

fn faces(f: &[usize]) -> Feature {
    Feature::Items(f.iter().map(|&i| Anchor::Face(FaceIndex(i))).collect())
}

fn label(l: &str) -> DatumLabel {
    DatumLabel::new(l).unwrap()
}

fn datum(l: &str, f: usize) -> Datum {
    Datum::new(label(l), Some(FeatureId(f)), Vec::new()).unwrap()
}

fn system(datums: &[usize]) -> DatumSystem {
    DatumSystem::new(
        datums
            .iter()
            .map(|&d| Compartment {
                references: vec![DatumReference {
                    datum: DatumId(d),
                    modifiers: Vec::new(),
                }],
                modifiers: Vec::new(),
            })
            .collect(),
    )
    .unwrap()
}

fn tolerance(kind: ToleranceKind, target: ToleranceTarget, magnitude: &str) -> GeometricTolerance {
    GeometricTolerance {
        kind,
        target,
        magnitude: Some(mm(magnitude)),
        zone: None,
        modifiers: Vec::new(),
        unit_basis: None,
        maximum: None,
        unequal: None,
        datums: None,
        auxiliary: Vec::new(),
        description: None,
    }
}

fn size(feature: usize, nominal: &str, tolerance: DimTolerance) -> Dimension {
    Dimension {
        kind: DimensionKind::Size {
            feature: FeatureId(feature),
            kind: SizeKind::Diameter,
            path: None,
            angle: None,
        },
        nominal: Some(mm(nominal)),
        tolerance,
        qualifier: None,
        modifiers: Vec::new(),
        principle: None,
    }
}

fn bounds(upper: &str, lower: &str) -> Bounds {
    Bounds::new(mm(upper), mm(lower)).unwrap()
}

fn fit(class: &str) -> Iso286Class {
    let split = class.find(|c: char| c.is_ascii_digit()).unwrap();
    Iso286Class {
        deviation: FundamentalDeviation::parse(&class[..split]).unwrap(),
        grade: ToleranceGrade::parse(&class[split..]).unwrap(),
    }
}

struct Written {
    doc: Document,
    read: PmiRead,
    report: WriteReport,
    /// The original's largest id: added instances are above it.
    max: u64,
}

impl Written {
    /// The added instances.
    fn added(&self) -> Vec<u64> {
        self.doc.ids().filter(|&id| id > self.max).collect()
    }

    fn text(&self, id: u64) -> String {
        String::from_utf8_lossy(&self.doc.bytes()[self.doc.span(id).unwrap()]).into_owned()
    }

    fn names(&self, id: u64) -> Vec<String> {
        match self.doc.get(id).unwrap() {
            RawEntity::Simple { name, .. } => vec![name.to_ascii_lowercase()],
            RawEntity::Complex { parts, .. } => {
                parts.iter().map(|p| p.name.to_ascii_lowercase()).collect()
            }
        }
    }
}

fn write(f: &File, pmi: &[(PartId, PartPmi)], mode: Mode) -> Result<Written, WriteError> {
    let (edit, report) = pmi::write(
        &f.doc,
        &f.parts,
        pmi,
        mode,
        PresentationPolicy::RemovePresentation,
    )?;
    let bytes = f.doc.apply(&edit).unwrap().bytes;
    let parts = read_part_definitions(&bytes).unwrap();
    let doc = Document::parse(bytes).unwrap();
    let read = pmi::read(&doc, &parts).unwrap();
    Ok(Written {
        doc,
        read,
        report,
        max: f.doc.max_id(),
    })
}

/// Writes `p` as part 0's PMI (replacing what it had) and checks it reads back as written,
/// with no finding about what was written.
fn written(f: &File, p: &PartPmi) -> Written {
    let w = write(f, &[(PartId(0), p.clone())], Mode::Replace)
        .unwrap_or_else(|e| panic!("not written: {e}"));
    assert_reads_back(&w, PartId(0), p);
    w
}

/// `pmi::verify` of a replace of part 0's PMI with `p` (written into `f`, read back as `w`).
fn assert_verified(f: &File, w: &Written, p: &PartPmi) {
    assert_verified_as(f, w, p, Mode::Replace);
}

/// `pmi::verify` of a write of part 0's PMI `p` with `mode`.
fn assert_verified_as(f: &File, w: &Written, p: &PartPmi, mode: Mode) {
    let before = pmi::read(&f.doc, &f.parts).unwrap();
    let parts = read_part_definitions(w.doc.bytes()).unwrap();
    let v = pmi::verify(
        pmi::Snapshot {
            doc: &f.doc,
            defs: &f.parts,
            read: &before,
        },
        &f.doc,
        &[(PartId(0), p.clone())],
        mode,
        pmi::Snapshot {
            doc: &w.doc,
            defs: &parts,
            read: &w.read,
        },
    );
    assert!(v.is_ok(), "{:?}", v.into_result());
}

fn assert_reads_back(w: &Written, part: PartId, p: &PartPmi) {
    let d = differences_as_stated(&pmi::write::standard_form(p), &w.read.parts[part.0]);
    assert!(
        d.is_empty(),
        "read back differs:\n{}\nfindings: {:#?}",
        d.join("\n"),
        w.read
            .findings
            .iter()
            .filter(|f| f.ids.iter().any(|&id| id > w.max))
            .collect::<Vec<_>>()
    );
    let on_written: Vec<&Finding> = w
        .read
        .findings
        .iter()
        .filter(|f| f.ids.iter().any(|&id| id > w.max))
        .collect();
    assert!(
        on_written.is_empty(),
        "findings on written instances: {on_written:#?}"
    );
}

/// Four planar features, datums A–D on them.
fn four_datums() -> PartPmi {
    PartPmi {
        features: vec![faces(&[0]), faces(&[1]), faces(&[2]), faces(&[3])],
        datums: vec![datum("A", 0), datum("B", 1), datum("C", 2), datum("D", 3)],
        ..PartPmi::default()
    }
}

// ---------------------------------------------------------------------------------------------
// Anti-requirements, one test per row (design: Anti-requirements)
// ---------------------------------------------------------------------------------------------

/// Datum precedence on the datum (one label per letter per tolerance): A|B, B|C|A and D|B|C on
/// one part read back exactly, with one DATUM per letter in the file.
#[test]
fn datum_precedence_is_the_systems_not_the_datums() {
    let mut p = four_datums();
    p.features.push(faces(&[4]));
    for (sys, kind) in [
        (vec![0, 1], ToleranceKind::Perpendicularity),
        (vec![1, 2, 0], ToleranceKind::Parallelism),
        (vec![3, 1, 2], ToleranceKind::Position),
    ] {
        let mut t = tolerance(kind, ToleranceTarget::Feature(FeatureId(4)), "0.1");
        t.datums = Some(system(&sys));
        p.tolerances.push(t);
    }
    let w = written(&spool(), &p);
    for l in ["A", "B", "C", "D"] {
        let n = w
            .added()
            .into_iter()
            .filter(|&id| w.names(id) == ["datum"] && w.text(id).contains(&format!("'{l}')")))
            .count();
        assert_eq!(n, 1, "datum {l} written {n} times");
    }
    let systems: Vec<String> = w.read.parts[0]
        .tolerances
        .iter()
        .map(|t| {
            t.datums
                .as_ref()
                .unwrap()
                .compartments()
                .iter()
                .map(|c| {
                    w.read.parts[0].datums[c.references[0].datum.0]
                        .label()
                        .to_string()
                })
                .collect::<Vec<_>>()
                .join("|")
        })
        .collect();
    let mut systems = systems;
    systems.sort();
    assert_eq!(systems, ["A|B", "B|C|A", "D|B|C"]);
}

/// Lower deviation negated: +0.1/−0.05 and g6 (−0.009/−0.025) round trip, written as signed
/// offsets.
#[test]
fn lower_deviations_keep_their_sign() {
    let p = PartPmi {
        features: vec![faces(&[0]), faces(&[1])],
        dimensions: vec![
            size(0, "20.", DimTolerance::Deviations(bounds("0.1", "-0.05"))),
            size(
                1,
                "70.",
                DimTolerance::Deviations(bounds("-0.009", "-0.025")),
            ),
        ],
        ..PartPmi::default()
    };
    let w = written(&spool(), &p);
    let values: Vec<String> = w
        .added()
        .into_iter()
        .filter(|&id| w.names(id) == ["tolerance_value"])
        .map(|id| {
            let RawEntity::Simple { attributes, .. } = w.doc.get(id).unwrap() else {
                unreachable!()
            };
            attributes
                .iter()
                .map(|a| match a {
                    Attribute::EntityRef(m) => w.text(*m),
                    _ => panic!("tolerance_value bound is not a measure"),
                })
                .collect::<Vec<_>>()
                .join(" / ")
        })
        .collect();
    assert_eq!(values.len(), 2);
    assert!(
        values
            .iter()
            .any(|v| v.contains("LENGTH_MEASURE(-0.05)") && v.contains("LENGTH_MEASURE(0.1)")),
        "{values:?}"
    );
    assert!(
        values
            .iter()
            .any(|v| v.contains("LENGTH_MEASURE(-0.025)") && v.contains("LENGTH_MEASURE(-0.009)")),
        "{values:?}"
    );
}

/// Both deviations below nominal unreadable: g6 and f7 (−0.020/−0.041) are written and read.
#[test]
fn deviations_both_below_nominal_are_written_and_read() {
    let p = PartPmi {
        features: vec![faces(&[0]), faces(&[1])],
        dimensions: vec![
            size(
                0,
                "70.",
                DimTolerance::Deviations(bounds("-0.010", "-0.029")),
            ),
            size(
                1,
                "20.",
                DimTolerance::Deviations(bounds("-0.020", "-0.041")),
            ),
        ],
        ..PartPmi::default()
    };
    written(&spool(), &p);
}

/// ISO 286 grade written wrongly (fits as bare limits): Ø20 H7, Ø70 g6 and Ø20 H7
/// (20.000/20.021) read back as fits, `LIMITS_AND_FITS('H','hole','7','')`.
#[test]
fn iso286_fits_are_written_as_fits() {
    let p = PartPmi {
        features: vec![faces(&[0]), faces(&[1]), faces(&[2])],
        dimensions: vec![
            size(
                0,
                "20.",
                DimTolerance::Fit {
                    class: fit("H7"),
                    limits: None,
                },
            ),
            size(
                1,
                "70.",
                DimTolerance::Fit {
                    class: fit("g6"),
                    limits: None,
                },
            ),
            size(
                2,
                "20.",
                DimTolerance::Fit {
                    class: fit("H7"),
                    limits: Some(bounds("20.021", "20.000")),
                },
            ),
        ],
        ..PartPmi::default()
    };
    let w = written(&spool(), &p);
    let fits: Vec<String> = w
        .added()
        .into_iter()
        .filter(|&id| w.names(id) == ["limits_and_fits"])
        .map(|id| w.text(id))
        .collect();
    assert_eq!(fits.len(), 3);
    assert_eq!(
        fits.iter()
            .filter(|t| t.contains("('H','hole','7','')"))
            .count(),
        2,
        "{fits:?}"
    );
    assert_eq!(
        fits.iter()
            .filter(|t| t.contains("('g','shaft','6','')"))
            .count(),
        1,
        "{fits:?}"
    );
    assert!(
        w.read.parts[0]
            .dimensions
            .iter()
            .all(|d| matches!(d.tolerance, DimTolerance::Fit { .. }))
    );
}

/// Tolerance unit taken from the last dimension written (1000×): tolerances on a part with no
/// dimension carry their own unit; millimetre values written into an inch-context part (NIST
/// STC-06) keep their unit and their text.
#[test]
fn tolerance_units_are_each_values_own() {
    // No dimension at all: a flatness and a position in millimetres, a profile in inches.
    let mut p = four_datums();
    p.tolerances.push(tolerance(
        ToleranceKind::Flatness,
        ToleranceTarget::Feature(FeatureId(0)),
        "0.05",
    ));
    let mut inch = tolerance(
        ToleranceKind::SurfaceProfile,
        ToleranceTarget::Feature(FeatureId(1)),
        "0.002",
    );
    inch.magnitude = Some(Value::length(dec("0.002"), LengthUnit::Inch));
    p.tolerances.push(inch);
    written(&spool(), &p);

    // Millimetre values onto an inch part, beside its own PMI.
    let f = fixture("ap242/nist/nist_stc_06_asme1_ap242-e3.stp.gz");
    let free = free_faces(&f, 0, 2);
    let new = PartPmi {
        features: vec![faces(&[free[0]]), faces(&[free[1]])],
        dimensions: vec![size(
            1,
            "12.5",
            DimTolerance::Deviations(bounds("0.1", "-0.05")),
        )],
        tolerances: vec![tolerance(
            ToleranceKind::Flatness,
            ToleranceTarget::Feature(FeatureId(0)),
            "0.0030",
        )],
        ..PartPmi::default()
    };
    let w = write(&f, &[(PartId(0), new.clone())], Mode::Add).unwrap_or_else(|e| panic!("{e}"));
    let after = &w.read.parts[0];
    let flat = after
        .tolerances
        .iter()
        .find(|t| {
            t.kind == ToleranceKind::Flatness
                && t.magnitude
                    .as_ref()
                    .is_some_and(|m| m.decimal().as_str() == "0.0030")
        })
        .expect("the flatness reads back");
    assert_eq!(
        flat.magnitude.as_ref().unwrap().as_length().unwrap().unit,
        LengthUnit::Millimetre
    );
    let dim = after
        .dimensions
        .iter()
        .find(|d| {
            d.nominal
                .as_ref()
                .is_some_and(|n| n.decimal().as_str() == "12.5")
        })
        .expect("the dimension reads back");
    assert_eq!(
        dim.nominal.as_ref().unwrap().as_length().unwrap().unit,
        LengthUnit::Millimetre
    );
    assert_eq!(
        dim.tolerance,
        DimTolerance::Deviations(bounds("0.1", "-0.05"))
    );
}

/// Values changed by unit conversion: written in their own unit with their own digits (every
/// inch NIST file's values keep their text: `pmi_roundtrip.rs`); here stated texts that a
/// conversion or shortest-form printing would change.
#[test]
fn values_keep_their_stated_text() {
    let p = PartPmi {
        features: vec![faces(&[0]), faces(&[1])],
        dimensions: vec![Dimension {
            nominal: Some(Value::length(dec("0.3750"), LengthUnit::Inch)),
            tolerance: DimTolerance::Deviations(
                Bounds::new(
                    Value::length(dec("0.0030"), LengthUnit::Inch),
                    Value::length(dec("-0.0010"), LengthUnit::Inch),
                )
                .unwrap(),
            ),
            ..size(0, "1.", DimTolerance::None)
        }],
        tolerances: vec![tolerance(
            ToleranceKind::Flatness,
            ToleranceTarget::Feature(FeatureId(1)),
            "2.50E-02",
        )],
        ..PartPmi::default()
    };
    let w = written(&spool(), &p);
    let text: String = w.added().into_iter().map(|id| w.text(id)).collect();
    for t in ["(0.3750)", "(0.0030)", "(-0.0010)", "(2.50E-02)", "'INCH'"] {
        assert!(text.contains(t), "{t} not written");
    }
}

/// PMI that has been read cannot be written back: read → replace → read of a specify-core file
/// is the same PMI (every file: `pmi_roundtrip.rs`).
#[test]
fn pmi_read_is_written_back() {
    let f = spool();
    let r = pmi::read(&f.doc, &f.parts).unwrap();
    let mut p = r.parts[0].clone();
    p.notes.clear(); // decision 5: the text route is not written
    written(&f, &p);
}

/// Malformed RUNOUT_ZONE_DEFINITION: the validator rejects a two-attribute zone; the writer
/// writes three (zone, boundaries, orientation) only for a stated orientation.
#[test]
fn runout_zone_definitions_have_their_three_attributes() {
    let bad = haecceity::p21::simple(
        "RUNOUT_ZONE_DEFINITION",
        vec![Attribute::EntityRef(1), Attribute::List(Vec::new())],
    );
    let types = |id: u64| match id {
        1 => Some(vec!["TOLERANCE_ZONE".to_string()]),
        _ => Some(vec!["RUNOUT_ZONE_ORIENTATION".to_string()]),
    };
    let v = express::validate(&bad, &types);
    assert!(
        v.iter().any(|v| v.kind == express::Kind::AttributeCount),
        "{v:?}"
    );

    let mut p = four_datums();
    let mut t = tolerance(
        ToleranceKind::CircularRunout,
        ToleranceTarget::Feature(FeatureId(1)),
        "0.02",
    );
    t.datums = Some(system(&[0]));
    t.zone = Some(Zone {
        form: ZoneForm::Other(String::new()),
        projected: None,
        non_uniform: None,
        runout_angle: Some(Value::angle(dec("30."), AngleUnit::Degree)),
        affected_plane: None,
        affected_plane_kind: None,
    });
    p.tolerances.push(t);
    let mut plain = tolerance(
        ToleranceKind::TotalRunout,
        ToleranceTarget::Feature(FeatureId(2)),
        "0.03",
    );
    plain.datums = Some(system(&[0]));
    plain.zone = Some(Zone {
        form: ZoneForm::Other(String::new()),
        projected: None,
        non_uniform: None,
        runout_angle: None,
        affected_plane: None,
        affected_plane_kind: None,
    });
    p.tolerances.push(plain);
    let w = written(&spool(), &p);
    let zones: Vec<u64> = w
        .added()
        .into_iter()
        .filter(|&id| w.names(id) == ["runout_zone_definition"])
        .collect();
    assert_eq!(zones.len(), 1, "one stated orientation, one definition");
}

/// Untyped MEASURE_WITH_UNIT: the validator rejects an untyped value; nothing the writer makes
/// has one.
#[test]
fn untyped_measures_cannot_be_written() {
    let bad = haecceity::p21::simple(
        "LENGTH_MEASURE_WITH_UNIT",
        vec![Attribute::Real(0.5), Attribute::EntityRef(1)],
    );
    let types = |_| {
        Some(vec![
            "LENGTH_UNIT".into(),
            "NAMED_UNIT".into(),
            "SI_UNIT".into(),
        ])
    };
    let v = express::validate(&bad, &types);
    assert!(v.iter().any(|v| v.kind == express::Kind::Untyped), "{v:?}");

    let mut p = four_datums();
    p.dimensions.push(size(
        0,
        "20.",
        DimTolerance::Deviations(bounds("0.1", "-0.05")),
    ));
    let mut t = tolerance(
        ToleranceKind::Position,
        ToleranceTarget::Dimension(DimensionId(0)),
        "0.2",
    );
    t.datums = Some(system(&[1, 2]));
    t.modifiers = vec![ToleranceModifier::MaximumMaterialRequirement];
    p.tolerances.push(t);
    let w = written(&spool(), &p);
    let mut measures = 0;
    for id in w.added() {
        if let RawEntity::Simple {
            name, attributes, ..
        } = w.doc.get(id).unwrap()
            && name.ends_with("MEASURE_WITH_UNIT")
        {
            measures += 1;
            assert!(
                matches!(attributes[0], Attribute::Typed { .. }),
                "#{id} untyped"
            );
        }
        if let RawEntity::Complex { parts, .. } = w.doc.get(id).unwrap() {
            for leaf in parts.iter().filter(|l| l.name == "MEASURE_WITH_UNIT") {
                measures += 1;
                assert!(
                    matches!(leaf.attributes[0], Attribute::Typed { .. }),
                    "#{id} untyped"
                );
            }
        }
    }
    assert!(measures >= 4);
}

/// Complex entities where the simple form is expected: a perpendicularity with datums is a
/// simple PERPENDICULARITY_TOLERANCE (the validator rejects it as a complex instance); a
/// position with datums and a modifier is the complex instance it must be.
#[test]
fn complex_instances_only_where_required() {
    let bad = haecceity::p21::Complex::new([
        haecceity::p21::leaf(
            "GEOMETRIC_TOLERANCE",
            vec![
                Attribute::String(String::new()),
                Attribute::String(String::new()),
                Attribute::EntityRef(1),
                Attribute::EntityRef(2),
            ],
        ),
        haecceity::p21::leaf(
            "GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE",
            vec![Attribute::List(vec![Attribute::EntityRef(3)])],
        ),
        haecceity::p21::leaf("PERPENDICULARITY_TOLERANCE", vec![]),
    ])
    .unwrap()
    .into_record();
    let types = |id: u64| {
        Some(vec![
            match id {
                1 => "LENGTH_MEASURE_WITH_UNIT",
                2 => "SHAPE_ASPECT",
                _ => "DATUM_SYSTEM",
            }
            .to_string(),
        ])
    };
    let v = express::validate(&bad, &types);
    assert!(
        v.iter().any(|v| v.kind == express::Kind::InternalMapping),
        "{v:?}"
    );

    let mut p = four_datums();
    p.features.push(faces(&[4]));
    let mut perp = tolerance(
        ToleranceKind::Perpendicularity,
        ToleranceTarget::Feature(FeatureId(4)),
        "0.05",
    );
    perp.datums = Some(system(&[0]));
    let mut pos = tolerance(
        ToleranceKind::Position,
        ToleranceTarget::Feature(FeatureId(4)),
        "0.1",
    );
    pos.datums = Some(system(&[0, 1]));
    pos.modifiers = vec![ToleranceModifier::MaximumMaterialRequirement];
    p.tolerances = vec![perp, pos];
    let w = written(&spool(), &p);
    let kinds: BTreeSet<Vec<String>> = w
        .added()
        .into_iter()
        .map(|id| w.names(id))
        .filter(|n| {
            n.iter()
                .any(|x| x.ends_with("_tolerance") && x != "plus_minus_tolerance")
        })
        .collect();
    assert!(
        kinds.contains(&vec!["perpendicularity_tolerance".to_string()]),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&vec![
            "geometric_tolerance".to_string(),
            "geometric_tolerance_with_datum_reference".to_string(),
            "geometric_tolerance_with_modifiers".to_string(),
            "position_tolerance".to_string(),
        ]),
        "{kinds:?}"
    );
}

/// Datums imported only when referenced: a datum no tolerance references reads back.
#[test]
fn unreferenced_datums_are_written_and_read() {
    let p = PartPmi {
        features: vec![faces(&[0]), faces(&[5, 6])],
        datums: vec![datum("E", 0), datum("F", 1)],
        ..PartPmi::default()
    };
    written(&spool(), &p);
}

/// The references of instance `id`, in attribute order.
fn refs_of(doc: &Document, id: u64) -> Vec<u64> {
    let mut out = Vec::new();
    haecceity::p21::visit_attributes(doc.get(id).unwrap(), &mut |a| {
        if let Attribute::EntityRef(n) = a {
            out.push(*n);
        }
    });
    out
}

/// The datum feature symbols of the written file (decision 6): per
/// `draughting_model_item_association`, the label of the datum its datum feature establishes,
/// with its callout checked to be a tessellated 'datum' set in an annotation plane of a
/// draughting model that no `mechanical_design_and_draughting_relationship` relates to the
/// part's shape representation (left out, docs/step-ap242.md decisions of 2026-10-09).
fn datum_symbols(w: &Written) -> Vec<String> {
    let mut out = Vec::new();
    for id in w.doc.ids() {
        if w.names(id) != ["draughting_model_item_association"] {
            continue;
        }
        let [feature, model, callout] = refs_of(&w.doc, id)[..] else {
            panic!("#{id}: {}", w.text(id));
        };
        // This helper checks datum symbols only. Other PMI's complete graph is checked by
        // every_requirement_has_a_callout below, including associations to datum features.
        let occurrence = refs_of(&w.doc, callout)[0];
        let set = *refs_of(&w.doc, occurrence).last().unwrap();
        if !w.text(set).contains("TESSELLATED_GEOMETRIC_SET('datum'") {
            continue;
        }
        assert!(
            w.text(id)
                .contains("'PMI representation to presentation link'")
        );
        assert!(
            w.names(feature).iter().any(|n| n.contains("datum_feature")),
            "#{id} links {:?}",
            w.names(feature)
        );
        assert_eq!(w.names(model), ["draughting_model"]);
        assert_eq!(w.names(callout), ["draughting_callout"]);
        let [occurrence] = refs_of(&w.doc, callout)[..] else {
            panic!("{}", w.text(callout));
        };
        assert_eq!(w.names(occurrence), ["tessellated_annotation_occurrence"]);
        let set = *refs_of(&w.doc, occurrence).last().unwrap();
        assert!(
            w.text(set).contains("TESSELLATED_GEOMETRIC_SET('datum'"),
            "{}",
            w.text(set)
        );
        let plane = refs_of(&w.doc, model)
            .into_iter()
            .find(|&p| w.names(p) == ["annotation_plane"] && refs_of(&w.doc, p).contains(&callout))
            .expect("the callout's annotation plane is in the model");
        assert!(refs_of(&w.doc, plane).len() >= 3);
        assert!(
            !w.doc
                .referrers(model)
                .iter()
                .any(|&r| w.names(r) == ["mechanical_design_and_draughting_relationship"]),
            "the model is not related to the shape (maintainer decision 2026-10-09: \
             OpenCascade 7.9 crashes on that relationship)"
        );
        let datum = w
            .doc
            .referrers(feature)
            .iter()
            .filter(|&&r| w.names(r) == ["shape_aspect_relationship"])
            .map(|&r| refs_of(&w.doc, r)[1])
            .find(|&d| w.names(d) == ["datum"])
            .expect("the feature establishes a datum");
        let text = w.text(datum);
        let label = text.rsplit('\'').nth(1).unwrap().to_string();
        out.push(label);
    }
    out.sort();
    out
}

/// Datum feature symbols (decision 6): each added datum gets one, linked to its datum feature,
/// valid against the schema (the writer's own check) and not read as PMI; replace and remove
/// take the symbols with their datums, leaving none of what the first write added; a datum
/// resolved to the file's own (add) gets none.
#[test]
fn datum_feature_symbols_go_with_their_datums() {
    let f = spool();
    let w = written(&f, &four_datums());
    assert_eq!(datum_symbols(&w), ["A", "B", "C", "D"]);
    assert_eq!(
        w.report.datum_symbols,
        ["A", "B", "C", "D"].map(|l| (PartId(0), l.to_string()))
    );
    let again = File {
        parts: read_part_definitions(w.doc.bytes()).unwrap(),
        doc: Document::parse(w.doc.bytes().to_vec()).unwrap(),
    };
    // Replace with datum A only: the other three symbols go with their datums.
    let only_a = PartPmi {
        features: vec![faces(&[0])],
        datums: vec![datum("A", 0)],
        ..PartPmi::default()
    };
    let w2 = written(&again, &only_a);
    assert_eq!(datum_symbols(&w2), ["A"]);
    assert!(
        w2.doc
            .ids()
            .all(|id| id <= f.doc.max_id() || id > w.doc.max_id()),
        "an instance of the first write survives its replacement"
    );
    // Remove: nothing of the first write is left.
    let w3 = write(&again, &[(PartId(0), PartPmi::default())], Mode::Remove).unwrap();
    assert!(datum_symbols(&w3).is_empty());
    assert!(w3.doc.ids().all(|id| id <= f.doc.max_id()));
    // Under the Refuse policy the symbols are presentation like any other: a replace that
    // would orphan them is refused, naming them.
    match pmi::write(
        &again.doc,
        &again.parts,
        &[(PartId(0), PartPmi::default())],
        Mode::Remove,
        PresentationPolicy::Refuse,
    ) {
        Err(WriteError::Removal(r)) => assert!(
            r.blockers
                .iter()
                .any(|b| b.entity == "draughting_model_item_association"),
            "{r:?}"
        ),
        other => panic!("refused: {:?}", other.err()),
    }
    // Add with the file's datum A: reused, no symbol.
    let w4 = write(&again, &[(PartId(0), only_a)], Mode::Add).unwrap();
    assert!(w4.report.datum_symbols.is_empty());
    assert_eq!(datum_symbols(&w4).len(), 4);
}

/// A size and a tolerance on a feature of several faces (maintainer decision 2026-10-09): one
/// `geometric_item_specific_usage` per face, which OpenCascade reads, never an
/// `item_identified_representation_usage` of a `set_representation_item`. A shape aspect has
/// one usage per representation (UR2), so each face is a member shape aspect of its own,
/// shared by every feature on that face (UR1, §5.1), and the feature is composed of its
/// members; it reads back as the feature of those faces.
#[test]
fn a_feature_of_several_faces_has_a_usage_per_face() {
    let f = spool();
    let p = PartPmi {
        // Feature 1 shares face 10 with feature 2, which is a datum feature of its own.
        features: vec![faces(&[0]), faces(&[9, 10]), faces(&[10]), faces(&[5, 6])],
        datums: vec![datum("A", 0), datum("B", 2)],
        dimensions: vec![size(
            3,
            "20.",
            DimTolerance::Deviations(bounds("0.1", "-0.1")),
        )],
        tolerances: vec![{
            let mut t = tolerance(
                ToleranceKind::SurfaceProfile,
                ToleranceTarget::Feature(FeatureId(1)),
                "0.1",
            );
            t.datums = Some(system(&[0, 1]));
            t
        }],
        ..PartPmi::default()
    };
    let w = written(&f, &p);
    assert_verified(&f, &w, &p);
    for ids in [
        &w.read.provenance.parts[0].dimensions[0],
        &w.read.provenance.parts[0].tolerances[0],
    ] {
        let semantic = root(&w.doc, ids, &["dimensional_size", "geometric_tolerance"]);
        let aspect = refs_of(&w.doc, semantic)
            .into_iter()
            .find(|&id| w.names(id).iter().any(|n| express::is_a(n, "shape_aspect")))
            .unwrap();
        assert!(
            w.names(aspect)
                .iter()
                .any(|n| express::is_a(n, "composite_shape_aspect")),
            "{}",
            w.text(aspect)
        );
    }
    let added = w.added();
    for &id in &added {
        assert!(
            !w.text(id).contains("SET_REPRESENTATION_ITEM"),
            "{}",
            w.text(id)
        );
        assert!(
            w.names(id) != ["item_identified_representation_usage"],
            "{}",
            w.text(id)
        );
    }
    // Every usage identifies one face; each face is identified once.
    let usages: Vec<u64> = added
        .iter()
        .copied()
        .filter(|&id| w.names(id) == ["geometric_item_specific_usage"])
        .collect();
    let mut identified: Vec<u64> = usages.iter().map(|&u| refs_of(&w.doc, u)[2]).collect();
    let n = identified.len();
    identified.sort_unstable();
    identified.dedup();
    assert_eq!(identified.len(), n, "a face is identified twice");
    assert_eq!(n, 5, "faces 0, 5, 6, 9, 10: one usage each");
    let violations: Vec<_> = haecceity::express_rules::check_all(&w.doc)
        .into_iter()
        .filter(|v| v.id > w.max)
        .collect();
    assert!(violations.is_empty(), "{violations:#?}");
}

/// Add onto a file whose kept plain shape aspect identifies a feature's faces by one usage of
/// a `set_representation_item` (§6.5.1, as earlier writes did): reference that aspect
/// directly, and no face gains a second usage (item_identified_representation_usage UR1).
#[test]
fn add_reuses_an_aspect_of_exactly_the_features_faces() {
    let f = spool();
    let def = &f.parts[0];
    let (face9, face10) = (def.faces[9], def.faces[10]);
    let gisu = f
        .doc
        .ids()
        .find(|&id| {
            matches!(f.doc.get(id).unwrap(), RawEntity::Simple { name, .. }
                if name.eq_ignore_ascii_case("geometric_item_specific_usage"))
        })
        .unwrap();
    let rep = refs_of(&f.doc, gisu)[1];
    let (aspect, usage) = (f.doc.max_id() + 1, f.doc.max_id() + 2);
    let mut text = String::from_utf8(f.doc.bytes().to_vec()).unwrap();
    let at = text.rfind("ENDSEC;").unwrap();
    text.insert_str(
        at,
        &format!(
            "#{aspect}=SHAPE_ASPECT('','',#{},.T.);\n#{usage}=ITEM_IDENTIFIED_REPRESENTATION_USAGE('','',#{aspect},#{rep},SET_REPRESENTATION_ITEM((#{face9},#{face10})));\n",
            def.shape
        ),
    );
    let g = open_bytes(text.into_bytes());
    let add = PartPmi {
        features: vec![faces(&[9, 10])],
        tolerances: vec![tolerance(
            ToleranceKind::SurfaceProfile,
            ToleranceTarget::Feature(FeatureId(0)),
            "0.1",
        )],
        ..PartPmi::default()
    };
    let w = write(&g, &[(PartId(0), add)], Mode::Add).unwrap_or_else(|e| panic!("{e}"));
    let added = w.added();
    let compositions: Vec<u64> = added
        .iter()
        .copied()
        .filter(|&id| w.names(id) == ["shape_aspect_relationship"])
        .collect();
    assert!(
        compositions.is_empty(),
        "reuse the exact kept aspect directly"
    );
    let target = added
        .iter()
        .copied()
        .find(|&id| w.names(id) == ["surface_profile_tolerance"])
        .unwrap();
    assert_eq!(
        test_attribute(
            &w.doc,
            target,
            "geometric_tolerance",
            "toleranced_shape_aspect"
        ),
        Some(&Attribute::EntityRef(aspect))
    );
    assert!(
        added
            .iter()
            .all(|&id| w.names(id) != ["geometric_item_specific_usage"]),
        "no new usage of faces 9 and 10"
    );
    let t = w.read.parts[0]
        .tolerances
        .iter()
        .find(|t| t.kind == ToleranceKind::SurfaceProfile && t.datums.is_none())
        .expect("the added profile tolerance");
    let ToleranceTarget::Feature(fid) = t.target else {
        panic!("{t:?}");
    };
    assert_eq!(w.read.parts[0].features[fid.0], faces(&[9, 10]));
}

/// A newly established datum must use the already dimensioned feature's identity,
/// including when its several faces are gathered by a composite (§5.1 / §6.5).
#[test]
fn add_datum_specializes_the_kept_geometric_carrier() {
    for (face_set, complex) in [
        (&[9][..], false),
        (&[9, 10][..], false),
        (&[9, 10][..], true),
    ] {
        let f = spool();
        let first = PartPmi {
            features: vec![faces(face_set)],
            dimensions: vec![size(0, "10.", DimTolerance::Basic)],
            ..PartPmi::default()
        };
        let w = written(&f, &first);
        let first_dim = root(
            &w.doc,
            &w.read.provenance.parts[0].dimensions[0],
            &["dimensional_size"],
        );
        let owner =
            common::pmi_usage::reference(&w.doc, first_dim, "dimensional_size", "applies_to")
                .unwrap();
        let g = if complex {
            let RawEntity::Simple { attributes, .. } = w.doc.get(owner).unwrap() else {
                panic!("composite")
            };
            let mut edit = haecceity::p21::Edit::new();
            edit.replace(
                owner,
                haecceity::p21::Complex::new([
                    haecceity::p21::leaf("COMPOSITE_SHAPE_ASPECT", vec![]),
                    haecceity::p21::leaf("SHAPE_ASPECT", attributes.clone()),
                ])
                .unwrap()
                .into_record(),
            );
            open_bytes(w.doc.apply(&edit).unwrap().bytes)
        } else {
            open_bytes(w.doc.bytes().to_vec())
        };
        let add = PartPmi {
            features: vec![faces(face_set)],
            datums: vec![datum("Z", 0)],
            tolerances: vec![tolerance(
                ToleranceKind::SurfaceProfile,
                ToleranceTarget::Feature(FeatureId(0)),
                "0.1",
            )],
            ..PartPmi::default()
        };
        let w = write(&g, &[(PartId(0), add)], Mode::Add).unwrap();
        assert!(common::pmi_usage::is_a(&w.doc, owner, "datum_feature"));
        assert!(w.report.rewritten.contains(&owner));
        assert!(common::pmi_usage::violations(&w.doc).is_empty());
        assert!(new_violations(&g.doc, &w.doc).is_empty());
        let tol = w
            .added()
            .into_iter()
            .find(|id| common::pmi_usage::is_a(&w.doc, *id, "geometric_tolerance"))
            .unwrap();
        assert_eq!(
            common::pmi_usage::reference(
                &w.doc,
                tol,
                "geometric_tolerance",
                "toleranced_shape_aspect"
            ),
            Some(owner)
        );
    }
}

/// Part notes in words (coating, heat treatment, edges) as the PMI practice's editable text
/// (§7.4: 'semantic text', a user defined attribute; on the part, on its
/// `product_definition_shape`, Table 17).
fn part_notes() -> Vec<AttributeSet> {
    [
        ("coating", "Anodise to MIL-A-8625 Type II, black"),
        ("heat treatment", "None"),
        ("edges", "Break sharp edges 0.2 max"),
    ]
    .into_iter()
    .map(|(k, v)| AttributeSet {
        name: "semantic text".into(),
        on: None,
        items: vec![(k.into(), AttributeValue::Text(v.into()))],
        description: None,
        general_property: None,
    })
    .collect()
}

/// Ra 3.2 µm on the part (any process) and Ra 0.8 µm, Rz 4 µm on `faces` (material removal
/// required).
fn textures(faces: Option<FeatureId>) -> Vec<SurfaceTexture> {
    let um = |t| length(t, LengthUnit::Micrometre);
    let mut out = vec![SurfaceTexture {
        on: None,
        material_removal: MaterialRemoval::AnyProcessAllowed,
        parameters: vec![SurfaceTextureParameter {
            characteristic: "Ra".into(),
            value: um("3.2"),
        }],
    }];
    if let Some(f) = faces {
        out.push(SurfaceTexture {
            on: Some(f),
            material_removal: MaterialRemoval::Required,
            parameters: vec![
                SurfaceTextureParameter {
                    characteristic: "Ra".into(),
                    value: um("0.8"),
                },
                SurfaceTextureParameter {
                    characteristic: "Rz".into(),
                    value: um("4."),
                },
            ],
        });
    }
    out
}

/// `text` with `from` replaced by `to` in the line of instance `#id` (which holds it once).
fn edited(text: &str, id: u64, from: &str, to: &str) -> String {
    let tag = format!("#{id}=");
    let old = text.lines().find(|l| l.starts_with(&tag)).unwrap();
    assert_eq!(old.matches(from).count(), 1, "{from} in {old}");
    text.replacen(old, &old.replacen(from, to, 1), 1)
}

/// `text` with `line` appended to its data section.
fn appended(text: &str, line: &str) -> String {
    let at = text.rfind("ENDSEC;").unwrap();
    format!("{}{line}\n{}", &text[..at], &text[at..])
}

/// Notes and surface finish in standard forms (decision 5; maintainer decisions 2026-10-09,
/// specify-core's U3): part notes as 'semantic text' (PMI practice §7.4) and surface texture as
/// AP242's surface conditions (ISO 10303-1110: `Surface_texture`, its
/// `Standard_surface_texture_parameter`s in `surface_texture_representation`s). Both read back
/// as written; the instances satisfy `surface_texture_representation` WR1–WR5,
/// `general_property_association` WR1–WR2 and the global rule
/// `restrict_representation_for_surface_condition` (`express_rules::check_all`, each rule also
/// shown failing on an edit of the written file that breaks it), and the writer's own schema
/// and rule checks. WR5 with the association's WR2
/// makes the parameter 'surface_condition', not the mapping's 'surface texture parameter'
/// (docs/step-ap242.md, question 3). A parameter without its 'surface_condition' association
/// is reported, not read.
#[test]
fn notes_and_surface_textures_are_standard_forms() {
    let f = spool();
    let mut p = PartPmi {
        features: vec![faces(&[9, 10])],
        attributes: part_notes(),
        surface_textures: textures(Some(FeatureId(0))),
        ..PartPmi::default()
    };
    let w = written(&f, &p);
    assert_verified(&f, &w, &p);
    let pds = f.parts[0].shape;
    let name_of = |id: u64| w.text(id).split('\'').nth(1).unwrap_or("").to_string();
    // A general_property's name follows its id.
    let gp_name = |id: u64| w.text(id).split('\'').nth(3).unwrap_or("").to_string();
    let added = w.added();
    let of = |entity: &str| -> Vec<u64> {
        added
            .iter()
            .copied()
            .filter(|&id| w.names(id) == [entity])
            .collect()
    };
    // Notes: 'semantic text' on the part's shape.
    let notes: Vec<u64> = of("property_definition")
        .into_iter()
        .filter(|&id| name_of(id) == "semantic text")
        .collect();
    assert_eq!(notes.len(), 3);
    for n in notes {
        assert_eq!(refs_of(&w.doc, n), [pds], "{}", w.text(n));
    }
    // Surface textures: 'surface texture' with one representation, each parameter related to
    // it and represented by a surface_texture_representation of two items (the 'measuring
    // method' and the value), associated with a general property. The rules these forms meet
    // are express_rules', below.
    let pdrs = of("property_definition_representation");
    let rep_of = |pd: u64| -> u64 {
        let found: Vec<u64> = pdrs
            .iter()
            .filter(|&&r| refs_of(&w.doc, r)[0] == pd)
            .map(|&r| refs_of(&w.doc, r)[1])
            .collect();
        assert_eq!(found.len(), 1, "{}", w.text(pd));
        found[0]
    };
    let textures_: Vec<u64> = of("property_definition")
        .into_iter()
        .filter(|&id| name_of(id) == "surface texture")
        .collect();
    assert_eq!(textures_.len(), 2);
    for &t in &textures_ {
        rep_of(t);
    }
    let mut parameters = Vec::new();
    for r in of("property_definition_relationship") {
        assert_eq!(name_of(r), "surface texture parameter");
        let [relating, related] = refs_of(&w.doc, r)[..] else {
            panic!("{}", w.text(r));
        };
        assert!(textures_.contains(&relating));
        assert_eq!(name_of(related), "surface_condition");
        let rep = rep_of(related);
        assert_eq!(w.names(rep), ["surface_texture_representation"]);
        // The representation's references: its items, then its context.
        let mut items = refs_of(&w.doc, rep);
        items.pop();
        assert_eq!(items.len(), 2, "{}", w.text(rep));
        // The writer relates it to no 'measuring direction' (which WR4 would allow): it is
        // used by its property_definition_representation alone.
        assert!(
            w.doc
                .referrers(rep)
                .iter()
                .all(|&x| w.names(x) == ["property_definition_representation"])
        );
        parameters.push((related, rep, items));
    }
    assert_eq!(parameters.len(), 3);
    // The rules (express_rules::check_all): surface_texture_representation WR1–WR5,
    // general_property_association WR1–WR2 (the notes' associations and the parameters') and
    // the global rule restrict_representation_for_surface_condition hold on what the write
    // added; an edit of the written file breaking each is reported as that rule alone.
    let rules = |doc: &Document| -> BTreeSet<(u64, &'static str)> {
        haecceity::express_rules::check_all(doc)
            .into_iter()
            .map(|v| (v.id, v.rule))
            .collect()
    };
    let base = rules(&w.doc);
    let on_added: Vec<_> = base.iter().filter(|&&(id, _)| id > w.max).collect();
    assert!(on_added.is_empty(), "{on_added:?}");
    let text = String::from_utf8(w.doc.bytes().to_vec()).unwrap();
    let broken = |t: &str| -> BTreeSet<&'static str> {
        assert_ne!(t, text);
        rules(&Document::parse(t.as_bytes().to_vec()).unwrap())
            .difference(&base)
            .map(|&(_, r)| r)
            .collect()
    };
    let (param, rep, ref items) = parameters[0];
    let (method, value) = (items[0], items[1]);
    assert_eq!(name_of(method), "measuring method");
    let other_rep = parameters[1].1;
    let point = w
        .doc
        .ids()
        .find(|&id| w.names(id) == ["cartesian_point"])
        .unwrap();
    let gpa = of("general_property_association")
        .into_iter()
        .find(|&g| gp_name(refs_of(&w.doc, g)[0]) == "semantic text")
        .unwrap();
    let [gp, derived] = refs_of(&w.doc, gpa)[..] else {
        panic!("{}", w.text(gpa));
    };
    let next = w.doc.max_id() + 1;
    let items_ = format!("(#{method},#{value})");
    for (case, t, rule) in [
        (
            "a texture's representation named otherwise",
            text.replace(
                "REPRESENTATION('surface texture'",
                "REPRESENTATION('surface finish'",
            ),
            "restrict_representation_for_surface_condition.WR1",
        ),
        (
            "a point among the items",
            edited(
                &text,
                rep,
                &items_,
                &format!("(#{method},#{value},#{point})"),
            ),
            "surface_texture_representation.WR1",
        ),
        (
            "no 'measuring method'",
            edited(&text, method, "'measuring method'", "'measuring mode'"),
            "surface_texture_representation.WR2",
        ),
        (
            "no measure",
            edited(&text, rep, &items_, &format!("(#{method})")),
            "surface_texture_representation.WR3",
        ),
        (
            "related to another parameter's representation",
            appended(
                &text,
                &format!("#{next}=REPRESENTATION_RELATIONSHIP('','',#{rep},#{other_rep});"),
            ),
            "surface_texture_representation.WR4",
        ),
        (
            "a second representation of the parameter",
            appended(
                &text,
                &format!("#{next}=PROPERTY_DEFINITION_REPRESENTATION(#{param},#{rep});"),
            ),
            "surface_texture_representation.WR5",
        ),
        (
            "a second association of a definition",
            appended(
                &text,
                &format!("#{next}=GENERAL_PROPERTY_ASSOCIATION('',$,#{gp},#{derived});"),
            ),
            "general_property_association.WR1",
        ),
    ] {
        assert_eq!(broken(&t), BTreeSet::from([rule]), "{case}");
    }
    // A parameter named as the mapping names it (breaking the association's WR2) reads the
    // same.
    let mapped = text.replace(
        "PROPERTY_DEFINITION('surface_condition'",
        "PROPERTY_DEFINITION('surface texture parameter'",
    );
    assert_eq!(
        broken(&mapped),
        BTreeSet::from(["general_property_association.WR2"])
    );
    let r = pmi::read(&Document::parse(mapped.into_bytes()).unwrap(), &f.parts).unwrap();
    assert_eq!(r.parts[0].surface_textures, p.surface_textures);
    // A parameter whose general property is not 'surface_condition' breaks WR5 (and the
    // association's WR2: the names differ): reported, and its texture not read.
    let renamed = text.replace(
        "GENERAL_PROPERTY('','surface_condition',$)",
        "GENERAL_PROPERTY('','roughness',$)",
    );
    assert_eq!(
        broken(&renamed),
        BTreeSet::from([
            "general_property_association.WR2",
            "surface_texture_representation.WR5"
        ])
    );
    let r = pmi::read(&Document::parse(renamed.into_bytes()).unwrap(), &f.parts).unwrap();
    assert!(r.parts[0].surface_textures.is_empty());
    assert!(
        r.findings.iter().any(|x| x.detail.contains("WR5")),
        "{:?}",
        r.findings
    );
    // Remove: nothing of them is left.
    let again = File {
        parts: read_part_definitions(w.doc.bytes()).unwrap(),
        doc: Document::parse(w.doc.bytes().to_vec()).unwrap(),
    };
    let w2 = write(&again, &[(PartId(0), PartPmi::default())], Mode::Remove).unwrap();
    assert!(w2.read.parts[0].surface_textures.is_empty());
    assert!(w2.read.parts[0].attributes.is_empty());
    // The micrometre the first write added stays: the removal plan never removes units
    // (shared infrastructure, docs/step-ap242.md stage 2).
    let left: Vec<String> = w2
        .doc
        .ids()
        .filter(|&id| id > f.doc.max_id())
        .map(|id| w2.text(id))
        .collect();
    assert_eq!(
        left,
        ["(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MICRO.,.METRE.));"].map(|t| {
            let id = w2.doc.ids().find(|&id| id > f.doc.max_id()).unwrap_or(0);
            format!("#{id}={t}")
        })
    );
    // Integer-spelled decimals also have a valid REAL encoding.
    p.surface_textures[1].parameters[0].value = length("35", LengthUnit::Micrometre);
    write(&f, &[(PartId(0), p)], Mode::Replace).unwrap();
}

/// One datum per name per document (assemblies): datum A on each part of the two-part
/// assembly, each on its own faces; replacing one part's PMI leaves the other's PMI and every
/// byte of its instances untouched. The AP214 file becomes AP242 (decision 1) only because
/// every instance validates against the target edition.
#[test]
fn datums_belong_to_their_part() {
    let f = fixture("ap242/assembly/assembly.step");
    assert_eq!(f.parts.len(), 2);
    let a = |faces_: &[usize], m: &str| PartPmi {
        features: vec![faces(faces_), faces(&[faces_[0] + 1])],
        datums: vec![datum("A", 0)],
        tolerances: vec![{
            let mut t = tolerance(
                ToleranceKind::Perpendicularity,
                ToleranceTarget::Feature(FeatureId(1)),
                m,
            );
            t.datums = Some(system(&[0]));
            t
        }],
        ..PartPmi::default()
    };
    let (p0, p1) = (a(&[0], "0.05"), a(&[1], "0.07"));
    let w = write(
        &f,
        &[(PartId(1), p1.clone()), (PartId(0), p0.clone())],
        Mode::Replace,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(matches!(w.report.schema, SchemaChange::Upgraded { .. }));
    assert!(w.report.repairs.is_empty(), "valid AP214 needs no repair");
    assert_eq!(w.doc.file_schema().unwrap(), [express::TARGET_SCHEMA]);
    assert_reads_back(&w, PartId(0), &p0);
    assert_reads_back(&w, PartId(1), &p1);
    let datums = w
        .added()
        .into_iter()
        .filter(|&id| w.names(id) == ["datum"])
        .count();
    assert_eq!(datums, 2, "datum A once per part");

    // Replace part 0 only.
    let bytes = w.doc.bytes().to_vec();
    let g = open_bytes(bytes);
    let p0b = a(&[2], "0.01");
    let w2 =
        write(&g, &[(PartId(0), p0b.clone())], Mode::Replace).unwrap_or_else(|e| panic!("{e}"));
    assert_reads_back(&w2, PartId(0), &p0b);
    assert!(differences_as_stated(&p1, &w2.read.parts[1]).is_empty());
    let r = pmi::read(&g.doc, &g.parts).unwrap();
    for id in r.provenance.parts[1].all() {
        assert_eq!(
            g.doc.bytes()[g.doc.span(id).unwrap()],
            w2.doc.bytes()[w2.doc.span(id).unwrap()],
            "part 1's #{id} changed"
        );
    }
    assert_eq!(
        w2.report.schema,
        SchemaChange::Kept(vec![express::TARGET_SCHEMA.to_string()])
    );
}

/// The draftwright-editor's assembly fixture (a plate, and a pin placed twice, part 1) and
/// specify-core's intent for the pin of the same parts (`assembly_plate_pin.intent.json`,
/// binding part 1: datums, a thread, a position, a fit, a runout and part notes).
fn editor_pin() -> (File, PartPmi) {
    let f = fixture("ap242/assembly/editor_assembly.step");
    let intents: Json = serde_json::from_str(
        &std::fs::read_to_string(
            common::fixtures().join("ap242/specify/assembly_plate_pin.intent.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let pin = intents
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["binding"]["part"] == 1)
        .unwrap();
    assert_eq!(pin["binding"]["face_count"], 7);
    (f, intent_pmi(pin).0)
}

/// One part of an assembly written alone (assemblies slice 1): the pin's PMI, replacing part 1
/// only, reads back on part 1 and nothing is read on part 0; every instance of the original,
/// part 0's shape among them, keeps its bytes. OpenCascade's reading of the result
/// (`write/editor_assembly_pin_alone`, checked in `opencascade_reads_the_written_files` and
/// `opencascade_puts_a_parts_pmi_on_its_prototype`) puts it on the pin's own label, so both
/// placements show it.
#[test]
fn one_part_of_an_assembly_is_written_alone() {
    let (f, p) = editor_pin();
    assert_eq!(f.parts.len(), 2);
    assert!(
        pmi::read(&f.doc, &f.parts)
            .unwrap()
            .parts
            .iter()
            .all(|q| differences_as_stated(&PartPmi::default(), q).is_empty()),
        "the fixture has no PMI"
    );
    let w = write(&f, &[(PartId(1), p.clone())], Mode::Replace).unwrap_or_else(|e| panic!("{e}"));
    assert!(matches!(w.report.schema, SchemaChange::Upgraded { .. }));
    assert!(w.report.repairs.is_empty());
    assert_reads_back(&w, PartId(1), &p);
    let d = differences_as_stated(&PartPmi::default(), &w.read.parts[0]);
    assert!(d.is_empty(), "PMI read on part 0: {}", d.join("; "));
    assert!(w.read.provenance.parts[0].all().is_empty());
    // Part 0's shape: its product definition, its product_definition_shape and everything its
    // faces reach, and with them every other original instance, byte for byte.
    let plate = &f.parts[0];
    let mut reach: Vec<u64> = [plate.product_definition, plate.shape]
        .into_iter()
        .chain(plate.faces.iter().copied())
        .collect();
    let mut seen = BTreeSet::new();
    while let Some(id) = reach.pop() {
        if seen.insert(id) {
            reach.extend(refs_of(&f.doc, id));
        }
    }
    assert!(seen.len() > plate.faces.len() * 10, "{}", seen.len());
    let same =
        |id: u64| f.doc.bytes()[f.doc.span(id).unwrap()] == w.doc.bytes()[w.doc.span(id).unwrap()];
    for &id in &seen {
        assert!(same(id), "part 0's #{id} changed");
    }
    let changed: Vec<u64> = f.doc.ids().filter(|&id| !same(id)).collect();
    assert!(
        changed.is_empty(),
        "original instances changed: {changed:?}"
    );
}

fn open_bytes(bytes: Vec<u8>) -> File {
    let parts = read_part_definitions(&bytes).unwrap();
    let doc = Document::parse(bytes).unwrap();
    File { parts, doc }
}

/// Threads, knurls and the general tolerance appended as raw text: written as `thread`,
/// `turned_knurl` and the 'default tolerances' class through the emission layer, their
/// WHERE rules checked, read back from those entities. A `default_tolerance_table` (an ISO
/// 2768-1 or an ASME-style one) is read, not written (decision 3): refused by name.
#[test]
fn threads_knurls_and_general_tolerances_are_standard_entities() {
    let p = PartPmi {
        features: vec![faces(&[0]), faces(&[1]), faces(&[2])],
        threads: vec![Thread {
            feature: FeatureId(0),
            partial_area: Some(FeatureId(0)),
            side: ThreadSide::Internal,
            major_diameter: length("8.", LengthUnit::Millimetre),
            minor_diameter: Some(length("6.647", LengthUnit::Millimetre)),
            pitch_diameter: Some(length("7.188", LengthUnit::Millimetre)),
            number_of_threads: Ratio(dec("1.")),
            form: "M".into(),
            fit_class: "6H".into(),
            fit_class_2: None,
            hand: Hand::Right,
            crest: None,
            qualifier: None,
            nominal_size: None,
            runout: Some(FeatureId(1)),
        }],
        knurls: vec![Knurl {
            feature: FeatureId(2),
            pattern: KnurlPattern::Diamond,
            major_diameter: length("20.", LengthUnit::Millimetre),
            nominal_diameter: length("19.8", LengthUnit::Millimetre),
            diametral_pitch: length("0.8", LengthUnit::Millimetre),
            number_of_teeth: Some(Count(78)),
            tooth_depth: Some(length("0.4", LengthUnit::Millimetre)),
            root_fillet: None,
            helix_angle: Some(Angle {
                value: dec("30."),
                unit: AngleUnit::Degree,
            }),
            helix_hand: None,
        }],
        general: vec![GeneralTolerance::Class {
            text: "ISO 2768-mK".into(),
            standard: pmi::standards::Iso2768::recognise("ISO 2768-mK"),
        }],
        ..PartPmi::default()
    };
    let w = written(&spool(), &p);
    let names: BTreeSet<Vec<String>> = w.added().into_iter().map(|id| w.names(id)).collect();
    for e in ["thread", "turned_knurl", "applied_area", "thread_runout"] {
        assert!(names.contains(&vec![e.to_string()]), "no {e}");
    }

    for table in [
        GeneralTolerance::Table {
            name: "ISO 2768-1 m".into(),
            cells: Vec::new(),
        },
        GeneralTolerance::Table {
            name: "ASME Y14.5 2 places".into(),
            cells: Vec::new(),
        },
    ] {
        let p = PartPmi {
            general: vec![table],
            ..PartPmi::default()
        };
        match write(&spool(), &[(PartId(0), p)], Mode::Replace) {
            Err(WriteError::Refused(r)) => {
                assert_eq!(r.len(), 1);
                assert_eq!(r[0].item, ItemRef::General(0));
                assert!(r[0].why.contains("at least one cell"), "{}", r[0].why);
            }
            other => panic!("a table is refused: {:?}", other.err()),
        }
    }
}

fn thread_on(feature: usize, side: ThreadSide, runout: Option<usize>) -> Thread {
    Thread {
        feature: FeatureId(feature),
        partial_area: Some(FeatureId(feature)),
        side,
        major_diameter: length("6.", LengthUnit::Millimetre),
        minor_diameter: Some(length("4.917", LengthUnit::Millimetre)),
        pitch_diameter: Some(length("5.35", LengthUnit::Millimetre)),
        number_of_threads: Ratio(dec("1.")),
        form: "M".into(),
        fit_class: if side == ThreadSide::Internal {
            "6H"
        } else {
            "6g"
        }
        .into(),
        fit_class_2: None,
        hand: Hand::Right,
        crest: None,
        qualifier: None,
        nominal_size: None,
        runout: runout.map(FeatureId),
    }
}

/// The schema violations (`express::validate_document`) and rule violations
/// (`express_rules::check_all`) of `after` that `before` does not have.
fn new_violations(before: &Document, after: &Document) -> Vec<String> {
    let schema = express::validate_document(before.graph());
    let rules = haecceity::express_rules::check_all(before);
    let mut out: Vec<String> = express::validate_document(after.graph())
        .into_iter()
        .filter(|v| !schema.contains(v))
        .map(|v| v.to_string())
        .collect();
    out.extend(
        haecceity::express_rules::check_all(after)
            .into_iter()
            .filter(|v| !rules.contains(v))
            .map(|v| v.to_string()),
    );
    out
}

/// The `product_definition_shape`s of feature definitions (a thread's or knurl's, thread WR13).
fn feature_definition_shapes(doc: &Document) -> Vec<u64> {
    doc.ids()
        .filter(|&id| match doc.get(id).unwrap() {
            RawEntity::Simple {
                name, attributes, ..
            } if name.eq_ignore_ascii_case("product_definition_shape") => {
                matches!(attributes.get(2), Some(Attribute::EntityRef(d))
                    if matches!(doc.get(*d).unwrap(), RawEntity::Simple { name, .. }
                        if express::is_a(&name.to_ascii_lowercase(), "characterized_object")))
            }
            _ => false,
        })
        .collect()
}

/// specify-core-rust's U14 PMI on the spool's first four free faces: an internal thread (no
/// runout); an external thread with a runout and a straight knurl; and other PMI (a flatness)
/// to replace either with.
struct ThreadSets {
    internal: PartPmi,
    external_and_knurl: PartPmi,
    other: PartPmi,
}

fn thread_sets(f: &File) -> ThreadSets {
    let free = free_faces(f, 0, 4);
    ThreadSets {
        internal: PartPmi {
            features: vec![faces(&[free[0]])],
            threads: vec![thread_on(0, ThreadSide::Internal, None)],
            ..PartPmi::default()
        },
        external_and_knurl: PartPmi {
            features: vec![faces(&[free[0]]), faces(&[free[1]]), faces(&[free[2]])],
            threads: vec![thread_on(0, ThreadSide::External, Some(1))],
            knurls: vec![Knurl {
                feature: FeatureId(2),
                pattern: KnurlPattern::Straight,
                major_diameter: length("20.", LengthUnit::Millimetre),
                nominal_diameter: length("19.8", LengthUnit::Millimetre),
                diametral_pitch: length("0.8", LengthUnit::Millimetre),
                number_of_teeth: Some(Count(78)),
                tooth_depth: Some(length("0.4", LengthUnit::Millimetre)),
                root_fillet: Some(length("0.1", LengthUnit::Millimetre)),
                helix_angle: None,
                helix_hand: None,
            }],
            ..PartPmi::default()
        },
        other: PartPmi {
            features: vec![faces(&[free[3]])],
            tolerances: vec![tolerance(
                ToleranceKind::Flatness,
                ToleranceTarget::Feature(FeatureId(0)),
                "0.05",
            )],
            ..PartPmi::default()
        },
    }
}

/// A part with a thread haecceity wrote is written again (specify-core-rust's U14): read back
/// with nothing unconsumed (thread WR16's bare 'thread runout' aspect is the thread's), then
/// replaced with other PMI, replaced with the other thread set, and removed. Each write takes
/// the feature definition's `product_definition_shape` with its thread, is schema- and
/// rule-valid, and verifies (`pmi::verify`: exactly the items written, no consumed instance
/// surviving).
#[test]
fn a_written_thread_is_replaced_and_removed() {
    let f = spool();
    let s = thread_sets(&f);
    for (case, p, swap) in [
        ("internal", &s.internal, &s.external_and_knurl),
        ("external and knurl", &s.external_and_knurl, &s.internal),
    ] {
        let w = written(&f, p);
        assert_verified(&f, &w, p);
        assert!(
            w.read
                .findings
                .iter()
                .all(|x| x.kind != FindingKind::Unconsumed),
            "{case}: {:#?}",
            w.read.findings
        );
        let v = new_violations(&f.doc, &w.doc);
        assert!(v.is_empty(), "{case}: {v:#?}");
        let shapes = feature_definition_shapes(&w.doc);
        assert_eq!(shapes.len(), p.threads.len() + p.knurls.len(), "{case}");

        let g = open_bytes(w.doc.bytes().to_vec());
        for (mode, q) in [
            (Mode::Replace, &s.other),
            (Mode::Replace, swap),
            (Mode::Remove, &PartPmi::default()),
        ] {
            let w2 = write(&g, &[(PartId(0), q.clone())], mode)
                .unwrap_or_else(|e| panic!("{case} {mode:?}: {e}"));
            assert_reads_back(&w2, PartId(0), q);
            assert_verified_as(&g, &w2, q, mode);
            let v = new_violations(&g.doc, &w2.doc);
            assert!(v.is_empty(), "{case} {mode:?}: {v:#?}");
            for &id in &shapes {
                assert!(w2.doc.get(id).is_none(), "{case} {mode:?}: #{id} kept");
            }
            assert_eq!(
                feature_definition_shapes(&w2.doc).len(),
                q.threads.len() + q.knurls.len(),
                "{case} {mode:?}"
            );
        }
    }
}

/// The removal plan takes a feature definition's `product_definition_shape` only with what
/// uses it (`product_definition_shape` WR1 and UR1: the shape of a characterized object is that
/// object's), and still refuses a part's own shape as shared infrastructure and a feature
/// definition's shape that kept instances reference.
#[test]
fn removal_takes_a_feature_definitions_shape_only_with_its_feature() {
    use haecceity::removal::{self, PresentationPolicy as Policy};
    let f = spool();
    let free = free_faces(&f, 0, 1);
    let p = PartPmi {
        features: vec![faces(&[free[0]])],
        threads: vec![thread_on(0, ThreadSide::Internal, None)],
        ..PartPmi::default()
    };
    let w = written(&f, &p);
    let g = open_bytes(w.doc.bytes().to_vec());
    let [shape] = feature_definition_shapes(&g.doc)[..] else {
        panic!("one feature definition shape")
    };
    let thread: BTreeSet<u64> = w.read.provenance.parts[0].threads[0]
        .iter()
        .copied()
        .collect();
    assert!(
        thread.contains(&shape),
        "the reader consumes it with its thread"
    );
    for policy in [Policy::Refuse, Policy::RemovePresentation] {
        let result = removal::plan(&g.doc, &thread, policy);
        if cfg!(feature = "presentation") && policy == Policy::Refuse {
            // Threads now own presentation too: Refuse must protect their callout.
            assert!(
                result.is_err(),
                "thread callout requires RemovePresentation"
            );
        } else {
            let plan = if cfg!(feature = "presentation") {
                // Generic removal preserves independently named models. With the
                // last thread annotation gone, the caller explicitly removes its
                // now-empty generated model too (the existing container contract).
                let refusal = result.unwrap_err();
                assert!(
                    refusal
                        .emptied
                        .iter()
                        .any(|e| e.attribute == "representation.items")
                );
                let mut seeds = thread.clone();
                seeds.extend(
                    w.added()
                        .into_iter()
                        .filter(|&id| w.names(id) == ["draughting_model"]),
                );
                removal::plan(&g.doc, &seeds, policy).unwrap()
            } else {
                result.unwrap_or_else(|e| panic!("{policy:?}: {e}"))
            };
            assert!(plan.removed.contains(&shape), "{policy:?}");
        }

        // Alone, the thread's aspects (kept) still reference it.
        let alone = removal::plan(&g.doc, &BTreeSet::from([shape]), policy).unwrap_err();
        assert!(alone.infrastructure.is_empty(), "{policy:?}: {alone}");
        assert!(!alone.blockers.is_empty(), "{policy:?}: {alone}");

        // The part's own shape stays shared infrastructure.
        let own = g.parts[0].shape;
        let refused = removal::plan(&g.doc, &BTreeSet::from([own]), policy).unwrap_err();
        assert_eq!(
            refused.infrastructure,
            [(own, "product_definition_shape".to_string())],
            "{policy:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Editing: add, replace, remove; refusals; determinism
// ---------------------------------------------------------------------------------------------

/// Faces of `part` no usage of the file identifies (so new features take them without
/// sharing an item with the file's own), the first `n`.
fn free_faces(f: &File, part: usize, n: usize) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, &face) in f.parts[part].faces.iter().enumerate() {
        let used = f.doc.referrers(face).iter().any(|&r| {
            let names: Vec<String> = match f.doc.get(r).unwrap() {
                RawEntity::Simple { name, .. } => vec![name.to_ascii_lowercase()],
                RawEntity::Complex { parts, .. } => {
                    parts.iter().map(|p| p.name.to_ascii_lowercase()).collect()
                }
            };
            names
                .iter()
                .any(|n| express::is_a(n, "item_identified_representation_usage"))
        });
        if !used {
            out.push(i);
            if out.len() == n {
                break;
            }
        }
    }
    assert_eq!(out.len(), n, "not enough free faces");
    out
}

/// A set of PMI to add beside a file's own: a datum on a free face, a fit, a perpendicularity
/// to the new datum.
fn addition(f: &File, part: usize, existing: &PartPmi) -> PartPmi {
    let free = free_faces(f, part, 2);
    let mut l = "Z".to_string();
    while existing.datums.iter().any(|d| d.label().as_str() == l) {
        l.push('Z');
    }
    let mut perp = tolerance(
        ToleranceKind::Perpendicularity,
        ToleranceTarget::Dimension(DimensionId(0)),
        "0.05",
    );
    perp.datums = Some(system(&[0]));
    PartPmi {
        features: vec![faces(&[free[0]]), faces(&[free[1]])],
        datums: vec![datum(&l, 0)],
        dimensions: vec![size(
            1,
            "10.",
            DimTolerance::Fit {
                class: fit("H7"),
                limits: None,
            },
        )],
        tolerances: vec![perp],
        ..PartPmi::default()
    }
}

/// Byte preservation: PMI added onto every NIST AP242 file and every specify-core input
/// leaves every original instance byte for byte, the original PMI reads back unchanged (the
/// new items besides), and FILE_SCHEMA as it was.
#[test]
fn add_keeps_every_original_byte() {
    let mut files: Vec<PathBuf> = std::fs::read_dir(common::fixtures().join("ap242/specify"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".step.gz"))
        .collect();
    // The committed NIST models, and the rest of NIST's set when HAECCEITY_NIST_PMI names it.
    let committed: Vec<PathBuf> = std::fs::read_dir(common::fixtures().join("ap242/nist"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".stp.gz"))
        .collect();
    let name = |p: &Path| p.file_name().unwrap().to_string_lossy().replace(".gz", "");
    let committed_names: BTreeSet<String> = committed.iter().map(|p| name(p)).collect();
    files.extend(committed);
    files.extend(
        nist_files()
            .into_iter()
            .filter(|p| !committed_names.contains(&name(p))),
    );
    files.sort();
    let mut problems = Vec::new();
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let bytes = file_bytes(&path);
        let Ok(parts) = read_part_definitions(&bytes) else {
            continue;
        };
        let f = File {
            parts,
            doc: Document::parse(bytes).unwrap(),
        };
        let r0 = pmi::read(&f.doc, &f.parts).unwrap();
        let add = addition(&f, 0, &r0.parts[0]);
        let w = match write(&f, &[(PartId(0), add.clone())], Mode::Add) {
            Ok(w) => w,
            Err(e) => {
                problems.push(format!("{name}: {e}"));
                continue;
            }
        };
        for id in f.doc.ids() {
            if f.doc.bytes()[f.doc.span(id).unwrap()] != w.doc.bytes()[w.doc.span(id).unwrap()] {
                problems.push(format!("{name}: #{id} changed"));
            }
        }
        if w.doc.file_schema() != f.doc.file_schema() {
            problems.push(format!("{name}: FILE_SCHEMA changed"));
        }
        for (i, p) in r0.parts.iter().enumerate() {
            let d: Vec<String> = differences_as_stated(p, &w.read.parts[i])
                .into_iter()
                .filter(|d| d.contains("only in the first"))
                .collect();
            if !d.is_empty() {
                problems.push(format!(
                    "{name} part {i}: original PMI lost: {}",
                    d.join("; ")
                ));
            }
        }
        // What was added reads back: the original's items plus exactly these.
        let mut expect = r0.parts[0].clone();
        merge(&mut expect, &add);
        let d = differences_as_stated(&expect, &w.read.parts[0]);
        if !d.is_empty() {
            problems.push(format!("{name}: added PMI differs: {}", d.join("; ")));
        }
        if w.read
            .findings
            .iter()
            .any(|x| x.ids.iter().any(|&id| id > w.max))
        {
            problems.push(format!("{name}: findings on added instances"));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// `b`'s items appended to `a` (indices shifted).
fn merge(a: &mut PartPmi, b: &PartPmi) {
    let (nf, nd, nn) = (a.features.len(), a.datums.len(), a.dimensions.len());
    let f = |x: FeatureId| FeatureId(x.0 + nf);
    a.features.extend(b.features.iter().cloned());
    for d in &b.datums {
        a.datums
            .push(Datum::new(d.label().clone(), d.feature().map(f), Vec::new()).unwrap());
    }
    for d in &b.dimensions {
        let mut d = d.clone();
        if let DimensionKind::Size { feature, .. } = &mut d.kind {
            *feature = f(*feature);
        }
        a.dimensions.push(d);
    }
    for t in &b.tolerances {
        let mut t = t.clone();
        t.target = match t.target {
            ToleranceTarget::Feature(x) => ToleranceTarget::Feature(f(x)),
            ToleranceTarget::Dimension(x) => ToleranceTarget::Dimension(DimensionId(x.0 + nn)),
            other => other,
        };
        if let Some(s) = &t.datums {
            t.datums = Some(
                DatumSystem::new(
                    s.compartments()
                        .iter()
                        .map(|c| Compartment {
                            references: c
                                .references
                                .iter()
                                .map(|r| DatumReference {
                                    datum: DatumId(r.datum.0 + nd),
                                    modifiers: r.modifiers.clone(),
                                })
                                .collect(),
                            modifiers: c.modifiers.clone(),
                        })
                        .collect(),
                )
                .unwrap(),
            );
        }
        a.tolerances.push(t);
    }
}

/// Add resolves a label the part already has to its datum (same faces); other faces are
/// refused by name.
#[test]
fn add_resolves_an_existing_datum_label() {
    let f = spool();
    let r0 = pmi::read(&f.doc, &f.parts).unwrap();
    let old = &r0.parts[0];
    let a = old
        .datums
        .iter()
        .find(|d| d.label().as_str() == "A")
        .expect("datum A");
    let fa = old.features[a.feature().unwrap().0].clone();
    let free = free_faces(&f, 0, 1);
    let mut t = tolerance(
        ToleranceKind::Parallelism,
        ToleranceTarget::Feature(FeatureId(1)),
        "0.04",
    );
    t.datums = Some(system(&[0]));
    let add = PartPmi {
        features: vec![fa, faces(&free)],
        datums: vec![datum("A", 0)],
        tolerances: vec![t],
        ..PartPmi::default()
    };
    let w = write(&f, &[(PartId(0), add)], Mode::Add).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(w.report.reused_datums.len(), 1);
    assert!(w.added().into_iter().all(|id| w.names(id) != ["datum"]));
    let after = &w.read.parts[0];
    assert_eq!(after.datums.len(), old.datums.len());
    let par = after
        .tolerances
        .iter()
        .find(|t| t.kind == ToleranceKind::Parallelism)
        .unwrap();
    let ds = par.datums.as_ref().unwrap();
    assert_eq!(
        after.datums[ds.compartments()[0].references[0].datum.0]
            .label()
            .as_str(),
        "A"
    );

    let other = PartPmi {
        features: vec![faces(&free)],
        datums: vec![datum("A", 0)],
        ..PartPmi::default()
    };
    match write(&f, &[(PartId(0), other)], Mode::Add) {
        Err(WriteError::Refused(r)) => {
            assert_eq!(r[0].item, ItemRef::Datum(0));
            assert!(r[0].why.contains("other faces"), "{}", r[0].why);
        }
        other => panic!("refused: {:?}", other.err()),
    }
}

/// Remove on a specify-core-written file removes exactly its PMI: nothing is read afterwards,
/// every instance the reader did not consume stays byte for byte (presentation containers
/// rewritten, listed), and what the reader consumed is gone.
#[test]
fn remove_takes_exactly_the_parts_pmi() {
    for name in ["spool_fits", "bolt_thread_knurl", "assembly_plate_pin"] {
        let f = fixture(&format!("ap242/specify/{name}.step.gz"));
        let r0 = pmi::read(&f.doc, &f.parts).unwrap();
        let all: Vec<(PartId, PartPmi)> = (0..f.parts.len())
            .map(|i| (PartId(i), PartPmi::default()))
            .collect();
        let w = write(&f, &all, Mode::Remove).unwrap_or_else(|e| panic!("{name}: {e}"));
        for (i, p) in w.read.parts.iter().enumerate() {
            assert_eq!(p, &PartPmi::default(), "{name} part {i} keeps PMI");
        }
        let consumed: BTreeSet<u64> = r0.provenance.parts.iter().flat_map(|p| p.all()).collect();
        let removed: BTreeSet<u64> = w.report.removed.iter().copied().collect();
        assert!(
            consumed.is_subset(&removed),
            "{name}: consumed instances survive"
        );
        let rewritten: BTreeSet<u64> = w.report.rewritten.iter().copied().collect();
        for id in f.doc.ids() {
            if removed.contains(&id) || rewritten.contains(&id) {
                assert!(w.doc.get(id).is_none() || rewritten.contains(&id));
                continue;
            }
            assert_eq!(
                f.doc.bytes()[f.doc.span(id).unwrap()],
                w.doc.bytes()[w.doc.span(id).unwrap()],
                "{name}: #{id} changed"
            );
        }
        // Everything removed beyond the consumed PMI is presentation or the PMI's own parts.
        for id in removed.difference(&consumed) {
            let fam = haecceity::removal::instance_family(&f.doc, *id);
            assert_ne!(
                fam,
                express::Family::SemanticPmi,
                "{name}: unconsumed PMI #{id} removed"
            );
        }
        assert!(w.added().is_empty());
    }
}

/// Every refusal of every part at once, each naming its item.
#[test]
fn refusals_name_every_item() {
    let f = spool();
    let p = PartPmi {
        features: vec![
            faces(&[0]),
            Feature::Group {
                members: vec![FeatureId(0)],
                kind: GroupKind::Unstated,
            },
        ],
        datum_targets: vec![
            DatumTarget::new(
                1,
                TargetShape::Line {
                    length: length("-1.", LengthUnit::Millimetre),
                },
                Some(Placement {
                    origin: std::array::from_fn(|_| length("0.", LengthUnit::Millimetre)),
                    axis: None,
                    ref_direction: None,
                }),
                None,
                None,
            )
            .unwrap(),
        ],
        datums: vec![Datum::new(label("A"), None, vec![DatumTargetId(0)]).unwrap()],
        notes: vec![Note {
            kind: "surface texture".into(),
            text: "Ra 1.6".into(),
            on: None,
            callouts: Vec::new(),
        }],
        material: Some(Material {
            id: "steel".into(),
            name: None,
            density: Some(Density {
                value: dec("7.85"),
                unit: vec![("gram".into(), dec("1."))],
            }),
        }),
        decimal_places: Some(4),
        general: vec![GeneralTolerance::Table {
            name: "precision".into(),
            cells: vec![common::pmi_targets::decimal_cell()],
        }],
        dimensions: vec![Dimension {
            nominal: Some(Value::length(dec("35"), LengthUnit::Millimetre)),
            ..size(0, "1.", DimTolerance::None)
        }],
        ..PartPmi::default()
    };
    match write(&f, &[(PartId(0), p)], Mode::Replace) {
        Err(WriteError::Refused(r)) => {
            let items: BTreeSet<ItemRef> = r.iter().map(|x| x.item).collect();
            for i in [
                ItemRef::Feature(1),
                ItemRef::DatumTarget(0),
                ItemRef::Material,
                ItemRef::DecimalPlaces,
            ] {
                assert!(items.contains(&i), "{i:?} not refused: {r:#?}");
            }
        }
        other => panic!("refused: {:?}", other.err()),
    }
}

/// Determinism: the same input gives the same bytes (each write's maps hash with fresh seeds).
#[test]
fn writes_are_deterministic() {
    let f = fixture("ap242/nist/nist_stc_06_asme1_ap242-e3.stp.gz");
    let r = pmi::read(&f.doc, &f.parts).unwrap();
    let mut p = r.parts[0].clone();
    p.tolerance_relations.clear();
    let run = || {
        let (edit, _) = pmi::write(
            &f.doc,
            &f.parts,
            &[(PartId(0), p.clone())],
            Mode::Replace,
            PresentationPolicy::RemovePresentation,
        )
        .unwrap();
        f.doc.apply(&edit).unwrap().bytes
    };
    let a = run();
    let b = run();
    assert!(a == b, "two writes differ");
}

/// NIST's MBE PMI test models: the `.stp` files of the directory `HAECCEITY_NIST_PMI` names
/// (NIST's `NIST-PMI-STEP-Files`); none when it is not set, unless
/// `HAECCEITY_NIST_PMI_REQUIRED` is.
fn nist_files() -> Vec<PathBuf> {
    let Some(dir) = std::env::var_os("HAECCEITY_NIST_PMI").map(PathBuf::from) else {
        assert!(
            std::env::var_os("HAECCEITY_NIST_PMI_REQUIRED").is_none(),
            "HAECCEITY_NIST_PMI_REQUIRED is set but HAECCEITY_NIST_PMI is not"
        );
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "stp"))
        .collect();
    out.sort();
    out
}

// ---------------------------------------------------------------------------------------------
// Corpus: specify-core's intents written onto the original files, compared with what
// specify-core wrote
// ---------------------------------------------------------------------------------------------

type Json = serde_json::Value;

fn num(v: &Json) -> Decimal {
    // JSON numbers as their shortest decimal, written with a point (a Part 21 REAL).
    let t = v.to_string();
    let t = if t.contains('.') || t.contains('e') || t.contains('E') {
        t
    } else {
        format!("{t}.")
    };
    Decimal::parse(&t).unwrap()
}

/// specify-core's intent for one part as a [`PartPmi`] (millimetres, specify-core's face
/// numbering), and what of it has no standard form (refused here, compared nowhere).
fn intent_pmi(intent: &Json) -> (PartPmi, Vec<String>) {
    let mut p = PartPmi::default();
    let mut not_written = Vec::new();
    let feature = |p: &mut PartPmi, faces_: &Json| -> FeatureId {
        let f = faces(
            &faces_
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_u64().unwrap() as usize)
                .collect::<Vec<_>>(),
        );
        if let Some(i) = p.features.iter().position(|x| *x == f) {
            return FeatureId(i);
        }
        p.features.push(f);
        FeatureId(p.features.len() - 1)
    };
    let mut labels: Vec<String> = Vec::new();
    for d in intent["datums"].as_array().unwrap() {
        let f = feature(&mut p, &d["faces"]);
        let l = d["letter"].as_str().unwrap();
        labels.push(l.to_string());
        p.datums
            .push(Datum::new(label(l), Some(f), Vec::new()).unwrap());
    }
    let datums = |r: &Json| -> Option<DatumSystem> {
        let ls = r["datums"].as_array()?;
        Some(system(
            &ls.iter()
                .map(|l| {
                    labels
                        .iter()
                        .position(|x| x == l.as_str().unwrap())
                        .unwrap()
                })
                .collect::<Vec<_>>(),
        ))
    };
    let mm_n = |v: &Json| Value::length(num(v), LengthUnit::Millimetre);
    for r in intent["requirements"].as_array().unwrap() {
        let kind = r["kind"].as_str().unwrap();
        match kind {
            "size" => {
                let f = feature(&mut p, &r["faces"]);
                let tolerance = match r["fit"].as_str() {
                    Some(c) => DimTolerance::Fit {
                        class: fit(c),
                        limits: None,
                    },
                    None if r["upper"].is_number() => DimTolerance::Deviations(
                        Bounds::new(mm_n(&r["upper"]), mm_n(&r["lower"])).unwrap(),
                    ),
                    None => DimTolerance::None,
                };
                p.dimensions.push(Dimension {
                    nominal: Some(mm_n(&r["nominal"])),
                    ..size(f.0, "1.", tolerance)
                });
            }
            "location" => {
                let to = feature(&mut p, &r["faces"]);
                let from = feature(&mut p, &r["reference"]);
                p.dimensions.push(Dimension {
                    kind: DimensionKind::Location {
                        from,
                        to,
                        kind: LocationKind::LinearDistance,
                        path: None,
                        directed: false,
                        angle: None,
                    },
                    nominal: Some(mm_n(&r["distance"])),
                    tolerance: if r["basic"].as_bool() == Some(true) {
                        DimTolerance::Basic
                    } else {
                        DimTolerance::None
                    },
                    qualifier: None,
                    modifiers: Vec::new(),
                    principle: None,
                });
            }
            "position" | "runout" | "flatness" | "perpendicularity" | "parallelism" | "profile" => {
                let f = feature(&mut p, &r["faces"]);
                let k = match kind {
                    "position" => ToleranceKind::Position,
                    "runout" => ToleranceKind::CircularRunout,
                    "flatness" => ToleranceKind::Flatness,
                    "perpendicularity" => ToleranceKind::Perpendicularity,
                    "parallelism" => ToleranceKind::Parallelism,
                    _ => ToleranceKind::SurfaceProfile,
                };
                let mut t = tolerance(k, ToleranceTarget::Feature(f), "1.");
                t.magnitude = Some(mm_n(&r["tolerance"]));
                t.datums = datums(r);
                if r["diametral"].as_bool() == Some(true) {
                    t.zone = Some(Zone {
                        form: ZoneForm::CylindricalOrCircular,
                        projected: None,
                        non_uniform: None,
                        runout_angle: None,
                        affected_plane: None,
                        affected_plane_kind: None,
                    });
                }
                if r["mmc"].as_bool() == Some(true) {
                    t.modifiers
                        .push(ToleranceModifier::MaximumMaterialRequirement);
                }
                p.tolerances.push(t);
            }
            "thread" => {
                let f = feature(&mut p, &r["faces"]);
                let designation = r["designation"].as_str().unwrap();
                let major = num(&r.get("nominal").cloned().unwrap_or_else(|| {
                    serde_json::from_str(designation.trim_start_matches('M')).unwrap()
                }));
                p.threads.push(Thread {
                    feature: f,
                    partial_area: Some(f),
                    side: if r["side"] == "internal" {
                        ThreadSide::Internal
                    } else {
                        ThreadSide::External
                    },
                    major_diameter: Length {
                        value: major.clone(),
                        unit: LengthUnit::Millimetre,
                    },
                    minor_diameter: None,
                    pitch_diameter: None,
                    number_of_threads: Ratio(dec("1.")),
                    form: "M".into(),
                    fit_class: r["class"].as_str().unwrap().into(),
                    fit_class_2: None,
                    hand: if r["hand"] == "LH" {
                        Hand::Left
                    } else {
                        Hand::Right
                    },
                    crest: None,
                    qualifier: Some(r["spec"].as_str().unwrap().into()),
                    nominal_size: Some(Length {
                        value: major,
                        unit: LengthUnit::Millimetre,
                    }),
                    runout: None,
                });
                // Pitch, tapping drill and depths are not thread semantics (design: Threads
                // and knurls): a user defined attribute set on the thread's feature.
                let mut items = vec![(
                    "pitch".to_string(),
                    AttributeValue::Measure(mm_n(&r["pitch"])),
                )];
                for (k, n) in [
                    ("drill_diameter", "tapping drill diameter"),
                    ("drill_depth", "tapping drill depth"),
                    ("full_thread", "full thread depth"),
                    ("length", "thread length"),
                ] {
                    if r[k].is_number() {
                        items.push((n.to_string(), AttributeValue::Measure(mm_n(&r[k]))));
                    }
                }
                if r["through"].is_boolean() {
                    not_written.push(format!(
                        "thread on {}: 'through' is a boolean, which the writer refuses (the reader cannot read it back)",
                        r["faces"]
                    ));
                }
                p.attributes.push(AttributeSet {
                    name: "thread manufacturing".into(),
                    on: Some(NoteOwner::Feature(f)),
                    items,
                    description: None,
                    general_property: None,
                });
            }
            other => not_written.push(format!(
                "{other} on {}: the intent lacks parameters its standard entity requires",
                r["faces"]
            )),
        }
    }
    let part = &intent["part"];
    if let Some(m) = part["material"].as_str() {
        p.material = Some(Material {
            id: m.into(),
            name: None,
            density: None,
        });
    }
    if let Some(g) = part["general_tolerance"].as_str() {
        p.general.push(GeneralTolerance::Class {
            text: g.into(),
            standard: pmi::standards::Iso2768::recognise(g),
        });
    }
    for k in ["surface_finish", "edges"] {
        if part[k].is_string() {
            not_written.push(format!("{k}: no standard form in the model (decision 5)"));
        }
    }
    (p, not_written)
}

/// The original file of a specify-core case.
fn original(input: &str) -> Option<PathBuf> {
    let (kind, rel) = input.split_once(':').unwrap();
    match kind {
        "corpus" => {
            let dir = common::corpus_dir();
            if dir.is_none() {
                assert!(
                    std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
                    "QUIDDITY_CORPUS_REQUIRED is set but the corpus is missing"
                );
            }
            dir.map(|d| d.join(rel))
        }
        "assembly" => Some(common::fixtures().join("ap242/assembly").join(rel)),
        "nist" => Some(
            common::fixtures()
                .join("ap242/nist")
                .join(format!("{rel}.gz")),
        ),
        _ => panic!("{input}"),
    }
}

/// For every specify-core case: its intent written onto the ORIGINAL file (AP214 files become
/// AP242, decision 1, each pinned upgradable in corpus_upgrade.json) reads back as written;
/// compared with what specify-core wrote, every difference matches a pinned pattern with a
/// verdict (`references/pmi_write.json` "corpus").
#[test]
fn specify_intents_written_onto_the_original_files() {
    let pins = load_pins();
    let patterns = pins["corpus"]["patterns"].as_array().unwrap();
    common::check_verdicts("references/pmi_write.json corpus", patterns);
    let mut problems = Vec::new();
    let mut actual = serde_json::Map::new();
    let dir = common::fixtures().join("ap242/specify");
    let mut cases: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter_map(|n| n.strip_suffix(".meta.json").map(str::to_string))
        .collect();
    cases.sort();
    for case in cases {
        let meta: Json = serde_json::from_str(
            &std::fs::read_to_string(dir.join(format!("{case}.meta.json"))).unwrap(),
        )
        .unwrap();
        let Some(path) = original(meta["input"].as_str().unwrap()) else {
            continue;
        };
        let intents: Json = serde_json::from_str(
            &std::fs::read_to_string(dir.join(format!("{case}.intent.json"))).unwrap(),
        )
        .unwrap();
        let intents: Vec<Json> = match intents {
            Json::Array(a) => a,
            one => vec![one],
        };
        let f = open(&path);
        let spec = fixture(&format!("ap242/specify/{case}.step.gz"));
        let spec_read = pmi::read(&spec.doc, &spec.parts).unwrap();
        let mut pmi_ = Vec::new();
        let mut entry = serde_json::Map::new();
        for intent in &intents {
            let part = intent["binding"]["part"].as_u64().unwrap() as usize;
            let (p, not_written) = intent_pmi(intent);
            for n in not_written {
                entry
                    .entry("not written")
                    .or_insert(Json::Array(Vec::new()))
                    .as_array_mut()
                    .unwrap()
                    .push(format!("part {part}: {n}").into());
            }
            pmi_.push((PartId(part), p));
        }
        let w = match write(&f, &pmi_, Mode::Add) {
            Ok(w) => w,
            Err(e) => {
                problems.push(format!("{case}: {e}"));
                continue;
            }
        };
        entry.insert("schema".into(), format!("{:?}", w.report.schema).into());
        let mut diffs: Vec<String> = Vec::new();
        let before = pmi::read(&f.doc, &f.parts).unwrap();
        for (part, p) in &pmi_ {
            if before.parts[part.0] == PartPmi::default() {
                assert_reads_back(&w, *part, p);
            } else {
                // Beside the file's own PMI (NIST CTC-01): what was written is all there.
                let lost: Vec<String> = differences_as_stated(p, &w.read.parts[part.0])
                    .into_iter()
                    .filter(|d| d.contains("only in the first"))
                    .collect();
                assert!(lost.is_empty(), "{case}: {lost:?}");
            }
            // Compared by meaning; and as stated only for values in metres (specify-core's
            // tolerance unit), which are equal in meaning.
            let stated = differences_as_stated(&spec_read.parts[part.0], &w.read.parts[part.0])
                .into_iter()
                .filter(|d| d.contains("Metre"));
            for d in differences(&spec_read.parts[part.0], &w.read.parts[part.0])
                .into_iter()
                .chain(stated)
            {
                let matched = patterns.iter().find(|pt| {
                    pt["match"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|m| d.contains(m.as_str().unwrap()))
                });
                match matched {
                    Some(pt) => {
                        diffs.push(format!("part {}: {}", part.0, pt["key"].as_str().unwrap()))
                    }
                    None => problems.push(format!(
                        "{case} part {}: unexplained difference: {d}",
                        part.0
                    )),
                }
            }
        }
        let mut counts: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for d in diffs {
            *counts.entry(d).or_insert(0) += 1;
        }
        entry.insert("differences".into(), serde_json::to_value(counts).unwrap());
        actual.insert(case, Json::Object(entry));
    }
    let pinned = pins["corpus"]["cases"].as_object().unwrap();
    for (case, got) in &actual {
        if pinned.get(case) != Some(got) {
            problems.push(format!(
                "corpus {case}:\n  pinned {:?}\n  actual {got}",
                pinned.get(case)
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

fn load_pins() -> Json {
    serde_json::from_str(
        &std::fs::read_to_string(common::fixtures().join(if cfg!(feature = "presentation") {
            "references/pmi_write-presentation.json"
        } else {
            "references/pmi_write.json"
        }))
        .unwrap(),
    )
    .unwrap()
}

// ---------------------------------------------------------------------------------------------
// OpenCascade XCAF's reading of written files (adapted from the reader's oracle in pmi_read.rs)
// ---------------------------------------------------------------------------------------------

/// The item's own instance among its provenance: the first of the given entity types.
fn root(doc: &Document, prov: &[u64], types: &[&str]) -> u64 {
    prov.iter()
        .copied()
        .find(|&id| {
            let names: Vec<String> = match doc.get(id).unwrap() {
                haecceity::p21::RawEntity::Simple { name, .. } => vec![name.to_ascii_lowercase()],
                haecceity::p21::RawEntity::Complex { parts, .. } => {
                    parts.iter().map(|p| p.name.to_ascii_lowercase()).collect()
                }
            };
            names
                .iter()
                .any(|n| types.iter().any(|t| haecceity::express::is_a(n, t)))
        })
        .unwrap_or(0)
}

fn cover(p: &PartPmi, f: pmi::FeatureId) -> Vec<pmi::Anchor> {
    let mut out = Vec::new();
    let mut stack = vec![f];
    while let Some(f) = stack.pop() {
        match &p.features[f.0] {
            Feature::Items(a) => out.extend(a.iter().copied()),
            Feature::Group { members, .. } => stack.extend(members.iter().copied()),
            Feature::Derived { from, .. } => stack.extend(from.iter().copied()),
        }
    }
    out.sort();
    out.dedup();
    out
}

fn target_feature(p: &PartPmi, t: &ToleranceTarget) -> Option<pmi::FeatureId> {
    match t {
        ToleranceTarget::Feature(f) => Some(*f),
        ToleranceTarget::Dimension(d) => match &p.dimensions[d.0].kind {
            DimensionKind::Size { feature, .. } => Some(*feature),
            DimensionKind::Location { .. } => None,
        },
        _ => None,
    }
}

/// A value OpenCascade gives against one the reader read: equal in millimetres (degrees for
/// angles), or equal only in the file's own unit (OpenCascade did not convert it).
fn occt_value(occt: f64, v: &pmi::Value) -> Option<&'static str> {
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0);
    let si = match &v.quantity {
        pmi::Quantity::Length(l) => l.mm(),
        pmi::Quantity::Angle(a) => a.rad().to_degrees(),
    };
    if close(occt, si) {
        None
    } else if close(occt, v.decimal().to_f64()) {
        Some("in the file's unit, not converted")
    } else {
        Some("different")
    }
}

fn face_cover(p: &PartPmi, f: pmi::FeatureId) -> Vec<usize> {
    let mut v: Vec<usize> = cover(p, f)
        .into_iter()
        .map(|a| match a {
            pmi::Anchor::Face(i) => i.0,
            pmi::Anchor::Edge(e) => usize::MAX - e.0,
            pmi::Anchor::Geometry(g) => usize::MAX / 2 - g.0,
        })
        .collect();
    v.sort_unstable();
    v
}

fn occt_dim_type(d: &pmi::Dimension) -> String {
    match &d.kind {
        DimensionKind::Size { angle: Some(_), .. } => "Size_Angular".into(),
        DimensionKind::Size { kind, .. } => {
            let camel: String = kind
                .name()
                .split(' ')
                .map(|w| {
                    let mut c = w.chars();
                    c.next()
                        .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                        .unwrap_or_default()
                })
                .collect();
            format!("Size_{camel}")
        }
        DimensionKind::Location { angle: Some(_), .. } => "Location_Angular".into(),
        DimensionKind::Location { kind, .. } => match kind {
            pmi::LocationKind::LinearDistance => "Location_LinearDistance".into(),
            pmi::LocationKind::CurvedDistance => "Location_CurvedDistance".into(),
            k => {
                let n = k.name().trim_start_matches("linear distance ");
                let w: Vec<&str> = n.split(' ').collect();
                let cap = |x: &str| {
                    match x {
                        "centre" => "Center",
                        "outer" => "Outer",
                        "inner" => "Inner",
                        o => o,
                    }
                    .to_string()
                };
                match w.as_slice() {
                    [a, b] => format!("Location_LinearDistance_From{}To{}", cap(a), cap(b)),
                    [a] => format!("Location_LinearDistance_{}", cap(a)),
                    _ => format!("Location_{}", k.name()),
                }
            }
        },
    }
}

fn occt_tol_type(k: ToleranceKind) -> &'static str {
    match k {
        ToleranceKind::Angularity => "Angularity",
        ToleranceKind::CircularRunout => "CircularRunout",
        ToleranceKind::Coaxiality => "Coaxiality",
        ToleranceKind::Concentricity => "Concentricity",
        ToleranceKind::Cylindricity => "Cylindricity",
        ToleranceKind::Flatness => "Flatness",
        ToleranceKind::LineProfile => "ProfileOfLine",
        ToleranceKind::Parallelism => "Parallelism",
        ToleranceKind::Perpendicularity => "Perpendicularity",
        ToleranceKind::Position => "Position",
        ToleranceKind::Roundness => "CircularityOrRoundness",
        ToleranceKind::Straightness => "Straightness",
        ToleranceKind::SurfaceProfile => "ProfileOfSurface",
        ToleranceKind::Symmetry => "Symmetry",
        ToleranceKind::TotalRunout => "TotalRunout",
    }
}

/// The differences between OpenCascade's reading of a dimension and one the reader read on the
/// same faces.
fn occt_dim_fields(o: &Json, p: &PartPmi, d: &pmi::Dimension) -> Vec<String> {
    let mut out = Vec::new();
    let t = o["type"].as_str().unwrap();
    if t != occt_dim_type(d) {
        out.push(format!("type {t} vs {}", occt_dim_type(d)));
    }
    let _ = p;
    let num = |k: &str| o[k].as_f64().unwrap_or(0.0);
    let cmp = |what: &str, occt: f64, v: &pmi::Value| {
        occt_value(occt, v)
            .map(|why| format!("{what} {occt} vs {} {} ({why})", v.decimal(), unit_name(v)))
    };
    match &d.tolerance {
        DimTolerance::Limits(b) => {
            if !o["range"].as_bool().unwrap_or(false) {
                out.push("not a range".into());
            }
            out.extend(cmp("lower_bound", num("lower_bound"), b.lower()));
            out.extend(cmp("upper_bound", num("upper_bound"), b.upper()));
        }
        DimTolerance::Deviations(b) => {
            if let Some(n) = &d.nominal {
                out.extend(cmp("value", num("value"), n));
            }
            if !o["plus_minus"].as_bool().unwrap_or(false) {
                out.push(format!(
                    "not plus/minus (range {} lower_bound {} upper_bound {})",
                    o["range"],
                    num("lower_bound"),
                    num("upper_bound")
                ));
            } else {
                out.extend(cmp("upper_tol", num("upper_tol"), b.upper()));
                let mut lower = b.lower().clone();
                // OpenCascade gives the lower deviation as a magnitude below nominal.
                let neg = format!("{}", -b.lower().decimal().to_f64());
                let neg = pmi::Decimal::parse(&neg).unwrap_or_else(|_| b.lower().decimal().clone());
                match &mut lower.quantity {
                    pmi::Quantity::Length(l) => l.value = neg,
                    pmi::Quantity::Angle(a) => a.value = neg,
                }
                out.extend(cmp("lower_tol", num("lower_tol"), &lower));
            }
        }
        DimTolerance::Fit { class, .. } => {
            if let Some(n) = &d.nominal {
                out.extend(cmp("value", num("value"), n));
            }
            if !o["class_of_tolerance"].as_bool().unwrap_or(false) {
                out.push(format!("class {class} not read as a class"));
            } else {
                let c = &o["class"];
                let theirs = format!(
                    "{}{}",
                    c["form_variance"].as_str().unwrap_or(""),
                    c["grade"].as_str().unwrap_or("").trim_start_matches("IT")
                );
                let hole = c["is_hole"].as_u64() == Some(1);
                let ours_hole = class.deviation.of == pmi::FitFeature::Hole;
                if !theirs.eq_ignore_ascii_case(&class.to_string()) || hole != ours_hole {
                    out.push(format!("class {c} vs {class}"));
                }
            }
        }
        DimTolerance::None | DimTolerance::Basic => {
            match &d.nominal {
                Some(n) => out.extend(cmp("value", num("value"), n)),
                // OpenCascade gives 0 where the file states no value.
                None if num("value") == 0.0 => {}
                None => out.push(format!("value {} vs none stated", num("value"))),
            }
            if o["plus_minus"].as_bool().unwrap_or(false) || o["range"].as_bool().unwrap_or(false) {
                out.push("toleranced in OpenCascade".into());
            }
        }
    }
    out
}

fn unit_name(v: &pmi::Value) -> String {
    match &v.quantity {
        pmi::Quantity::Length(l) => format!("{:?}", l.unit),
        pmi::Quantity::Angle(a) => format!("{:?}", a.unit),
    }
}

fn occt_tol_fields(o: &Json, p: &PartPmi, t: &pmi::GeometricTolerance) -> Vec<String> {
    let mut out = Vec::new();
    match &t.magnitude {
        Some(m) => {
            if let Some(why) = occt_value(o["value"].as_f64().unwrap_or(0.0), m) {
                out.push(format!("value {} vs {} ({why})", o["value"], m.decimal()));
            }
        }
        None => out.push(format!("value {} vs none", o["value"])),
    }
    let mm = o["material_modifier"].as_str().unwrap_or("None");
    let ours = if t
        .modifiers
        .contains(&ToleranceModifier::MaximumMaterialRequirement)
    {
        "M"
    } else if t
        .modifiers
        .contains(&ToleranceModifier::LeastMaterialRequirement)
    {
        "L"
    } else {
        "None"
    };
    if mm != ours {
        out.push(format!("material modifier {mm} vs {ours}"));
    }
    let tv = o["type_of_value"].as_str().unwrap_or("None");
    let zone = match &t.zone {
        Some(z) if z.form.is_diametral() => "Diameter",
        Some(z) if z.form.is_spherical() => "SphericalDiameter",
        _ => "None",
    };
    if tv != zone {
        out.push(format!("type of value {tv} vs {zone}"));
    }
    let projected = t.zone.as_ref().and_then(|z| z.projected.as_ref());
    match (o["zone_modifier"].as_str().unwrap_or("None"), projected) {
        ("Projected", Some(pz)) => {
            if let Some(why) = occt_value(o["zone_value"].as_f64().unwrap_or(0.0), &pz.length) {
                out.push(format!(
                    "projected length {} vs {} ({why})",
                    o["zone_value"],
                    pz.length.decimal()
                ));
            }
        }
        ("None", None) => {}
        (z, p) => out.push(format!("zone modifier {z} vs projected {}", p.is_some())),
    }
    let theirs: Vec<(String, u64)> = o["datums"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            (
                d["name"].as_str().unwrap().to_string(),
                d["position"].as_u64().unwrap(),
            )
        })
        .collect();
    let mut ours: Vec<(String, u64)> = Vec::new();
    if let Some(s) = &t.datums {
        for (i, c) in s.compartments().iter().enumerate() {
            for r in &c.references {
                ours.push((p.datums[r.datum.0].label().to_string(), i as u64 + 1));
            }
        }
    }
    if theirs != ours {
        out.push(format!("datums {theirs:?} vs {ours:?}"));
    }
    out
}

/// The part (index) and its anchors that OpenCascade's faces and edges are, by their
/// `advanced_face` / edge instances (OpenCascade's shapes need not be the file's product
/// definitions); faces are numbered, edges as `usize::MAX - index`.
fn occt_faces(
    j: &Json,
    parts: &[haecceity::step::PartDefinition],
    r: &PmiRead,
) -> Option<(usize, Vec<usize>)> {
    let mut part = None;
    let mut out = Vec::new();
    for f in j.as_array().unwrap() {
        let (n, edge) = match f["advanced_face"].as_str() {
            Some(a) => (a, false),
            None => (f["instance"].as_str()?, true),
        };
        let n: u64 = n.trim_start_matches('#').parse().ok()?;
        let (pi, i) = parts.iter().enumerate().find_map(|(pi, p)| {
            if edge {
                p.edge_index(n).map(|i| (pi, usize::MAX - i)).or_else(|| {
                    // An edge OpenCascade made of supplemental geometry the reader holds.
                    r.provenance.parts[pi]
                        .geometry_items
                        .iter()
                        .position(|&g| g == n)
                        .map(|g| (pi, usize::MAX / 2 - g))
                })
            } else {
                p.face_index(n).map(|i| (pi, i))
            }
        })?;
        if part.is_some_and(|q| q != pi) {
            return None;
        }
        part = Some(pi);
        out.push(i);
    }
    out.sort_unstable();
    out.dedup();
    Some((part?, out))
}

/// Oracle 3: each item OpenCascade read (on faces of a part, matched by instance) against the
/// reader's item of that part on the same faces, and the reader's items OpenCascade did not
/// read.
fn compare_occt(
    name: &str,
    occt: &Json,
    r: &PmiRead,
    doc: &Document,
    parts: &[haecceity::step::PartDefinition],
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let all = |k: &str| -> Vec<Json> {
        occt["parts"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|p| p[k].as_array().unwrap().iter().cloned())
            .collect()
    };
    let (odims, otols, odatums) = (
        all("dimensions"),
        all("geometric_tolerances"),
        all("datums"),
    );
    for (pi, p) in r.parts.iter().enumerate() {
        let prov = &r.provenance.parts[pi];
        let dim_id = |i: usize| {
            root(
                doc,
                &prov.dimensions[i],
                &["dimensional_size", "dimensional_location"],
            )
        };
        let tol_id = |i: usize| root(doc, &prov.tolerances[i], &["geometric_tolerance"]);
        let on_part = |j: &Json| {
            occt_faces(j, parts, r)
                .filter(|(q, _)| *q == pi)
                .map(|(_, f)| f)
        };
        let mut dims_left: Vec<bool> = vec![true; p.dimensions.len()];
        let mut callouts: Vec<Vec<usize>> = Vec::new();
        for o in &odims {
            let t = o["type"].as_str().unwrap();
            let label = o["label"].as_str().unwrap();
            if t == "DimensionPresentation" {
                // Presentation only: OpenCascade's record of a callout, no semantics; its faces
                // are compared with the notes' below.
                callouts.extend(on_part(&o["faces"]));
                continue;
            }
            let Some(f1) = on_part(&o["faces"]) else {
                continue;
            };
            let f2 = if o["faces2"].as_array().unwrap().is_empty() {
                Vec::new()
            } else {
                match on_part(&o["faces2"]) {
                    Some(f) => f,
                    None => continue,
                }
            };
            let candidates: Vec<usize> = (0..p.dimensions.len())
                .filter(|&i| match &p.dimensions[i].kind {
                    DimensionKind::Size { feature, .. } => {
                        f2.is_empty() && face_cover(p, *feature) == f1
                    }
                    DimensionKind::Location { from, to, .. } => {
                        let (a, b) = (face_cover(p, *from), face_cover(p, *to));
                        (a == f1 && b == f2) || (a == f2 && b == f1)
                    }
                })
                .collect();
            let mut best = candidates
                .iter()
                .filter(|&&i| dims_left[i])
                .min_by_key(|&&i| occt_dim_fields(o, p, &p.dimensions[i]).len())
                .copied();
            let mut subset = false;
            if best.is_none() && f2.is_empty() {
                // OpenCascade may attach a pattern's dimension to some of its faces only.
                best = (0..p.dimensions.len())
                    .filter(|&i| dims_left[i] && occt_dim_type(&p.dimensions[i]) == t)
                    .filter(|&i| match &p.dimensions[i].kind {
                        DimensionKind::Size { feature, .. } => {
                            let c = face_cover(p, *feature);
                            f1.iter().all(|x| c.contains(x))
                        }
                        DimensionKind::Location { .. } => false,
                    })
                    .min_by_key(|&i| occt_dim_fields(o, p, &p.dimensions[i]).len());
                subset = best.is_some();
            }
            match best {
                Some(i) => {
                    dims_left[i] = false;
                    let mut diff = occt_dim_fields(o, p, &p.dimensions[i]);
                    if subset {
                        let c = match &p.dimensions[i].kind {
                            DimensionKind::Size { feature, .. } => face_cover(p, *feature),
                            DimensionKind::Location { .. } => Vec::new(),
                        };
                        diff.insert(
                            0,
                            format!("on faces {f1:?}, a part of the dimensioned feature's {c:?}"),
                        );
                    }
                    if !diff.is_empty() {
                        out.push((
                            format!("{name} occt dimension {label}"),
                            format!("#{}: {}", dim_id(i), diff.join("; ")),
                        ));
                    }
                }
                None => out.push((
                    format!("{name} occt dimension {label}"),
                    format!(
                        "{t} {} on faces {f1:?} {f2:?}: no dimension read on those faces",
                        o["value"]
                    ),
                )),
            }
        }
        for (i, left) in dims_left.iter().enumerate() {
            if *left {
                out.push((
                    format!("{name} occt missing dimension #{}", dim_id(i)),
                    occt_dim_type(&p.dimensions[i]),
                ));
            }
        }
        // specify-core's notes on faces (U10): OpenCascade links each note's callout to the
        // faces of the aspect the reader puts the note on.
        for n in &p.notes {
            let Some(NoteOwner::Feature(x)) = n.on else {
                continue;
            };
            let c = face_cover(p, x);
            match callouts.iter().position(|f| *f == c) {
                Some(j) => {
                    callouts.remove(j);
                }
                None => out.push((
                    format!("{name} occt note {:?} {c:?}", n.kind),
                    "no callout OpenCascade links to those faces".into(),
                )),
            }
        }
        for c in callouts {
            out.push((
                format!("{name} occt callout {c:?}"),
                "on faces where the reader has no note".into(),
            ));
        }
        let mut tols_left = vec![true; p.tolerances.len()];
        for o in &otols {
            let t = o["type"].as_str().unwrap();
            let label = o["label"].as_str().unwrap();
            let f = if o["faces"].as_array().unwrap().is_empty() {
                if pi != 0 {
                    continue;
                }
                Vec::new()
            } else {
                match on_part(&o["faces"]) {
                    Some(f) => f,
                    None => continue,
                }
            };
            let on_faces = |i: usize, exact: bool| -> bool {
                match target_feature(p, &p.tolerances[i].target) {
                    Some(x) => {
                        let c = face_cover(p, x);
                        !exact && !f.is_empty() && c != f && f.iter().all(|y| c.contains(y))
                    }
                    None => false,
                }
            };
            let mut best = (0..p.tolerances.len())
                .filter(|&i| {
                    tols_left[i]
                        && occt_tol_type(p.tolerances[i].kind) == t
                        && match &p.tolerances[i].target {
                            ToleranceTarget::WholePart => f.is_empty(),
                            tt => target_feature(p, tt).is_some_and(|x| face_cover(p, x) == f)
                                || matches!(tt, ToleranceTarget::Dimension(d) if match &p.dimensions[d.0].kind {
                                    DimensionKind::Location { from, to, .. } => {
                                        let mut c = face_cover(p, *from);
                                        c.extend(face_cover(p, *to));
                                        c.sort_unstable();
                                        c.dedup();
                                        c == f
                                    }
                                    _ => false,
                                }),
                        }
                })
                .min_by_key(|&i| occt_tol_fields(o, p, &p.tolerances[i]).len());
            let mut subset = false;
            if best.is_none() {
                best = (0..p.tolerances.len())
                    .filter(|&i| {
                        tols_left[i]
                            && occt_tol_type(p.tolerances[i].kind) == t
                            && on_faces(i, false)
                    })
                    .min_by_key(|&i| occt_tol_fields(o, p, &p.tolerances[i]).len());
                subset = best.is_some();
            }
            match best {
                Some(i) => {
                    tols_left[i] = false;
                    let mut diff = occt_tol_fields(o, p, &p.tolerances[i]);
                    if subset {
                        let c = target_feature(p, &p.tolerances[i].target)
                            .map(|x| face_cover(p, x))
                            .unwrap_or_default();
                        diff.insert(
                            0,
                            format!("on faces {f:?}, a part of the toleranced feature's {c:?}"),
                        );
                    }
                    if !diff.is_empty() {
                        out.push((
                            format!("{name} occt tolerance {label}"),
                            format!("#{}: {}", tol_id(i), diff.join("; ")),
                        ));
                    }
                }
                None => out.push((
                    format!("{name} occt tolerance {label}"),
                    format!(
                        "{t} {} on faces {f:?}: no tolerance read on those faces",
                        o["value"]
                    ),
                )),
            }
        }
        for (i, left) in tols_left.iter().enumerate() {
            if *left {
                out.push((
                    format!("{name} occt missing tolerance #{}", tol_id(i)),
                    format!("{:?}", p.tolerances[i].kind),
                ));
            }
        }
        // Target objects: number/type, sizes, placement and stated support feature.
        for (i, t) in p.datum_targets.iter().enumerate() {
            let label = p
                .datums
                .iter()
                .find(|d| d.targets().contains(&DatumTargetId(i)))
                .unwrap()
                .label()
                .as_str();
            let key = format!("{name} occt part {pi} datum target {label}{}", t.number());
            let Some(o) = odatums
                .iter()
                .chain(occt["unattached"]["datums"].as_array().unwrap().iter())
                .find(|o| {
                    o["is_target"] == true
                        && o["name"].as_str() == Some(label)
                        && o["target"]["number"].as_u64() == Some(u64::from(t.number()))
                })
            else {
                out.push((key, format!("not read: {:?}", t.shape())));
                continue;
            };
            let target = &o["target"];
            let expected_type = match t.shape() {
                TargetShape::Point => "Point",
                TargetShape::Line { .. } => "Line",
                TargetShape::Rectangle { .. } => "Rectangle",
                TargetShape::Circle { .. } => "Circle",
                TargetShape::CircularCurve { .. } => "CircularCurve",
                TargetShape::Area(_) => "Area",
                TargetShape::Curve(_) => "Curve",
            };
            let mut detail = Vec::new();
            if target["type"].as_str() != Some(expected_type) {
                detail.push(format!("type {} vs {expected_type}", target["type"]));
            }
            let close = |a: f64, b: f64| (a - b).abs() <= 1e-8 * a.abs().max(b.abs()).max(1.);
            for (field, value) in match t.shape() {
                TargetShape::Line { length } => vec![("length", length.mm())],
                TargetShape::Rectangle { length, width } => {
                    vec![("length", length.mm()), ("width", width.mm())]
                }
                TargetShape::Circle { diameter } | TargetShape::CircularCurve { diameter } => {
                    vec![("length", diameter.mm())]
                }
                _ => vec![],
            } {
                if !target[field].as_f64().is_some_and(|v| close(v, value)) {
                    detail.push(format!("{field} {} vs {value} mm", target[field]));
                }
            }
            if let Some(placement) = t.placement() {
                let vector = |actual: &Json, expected: [f64; 3]| {
                    actual.as_array().is_some_and(|v| {
                        v.len() == 3
                            && v.iter()
                                .zip(expected)
                                .all(|(a, b)| a.as_f64().is_some_and(|a| close(a, b)))
                    })
                };
                if !vector(
                    &target["axis"][0],
                    placement.origin.each_ref().map(|l| l.mm()),
                ) {
                    detail.push(format!(
                        "origin {} vs {:?}",
                        target["axis"][0],
                        placement.origin.each_ref().map(|l| l.mm())
                    ));
                }
                let normalized = |d: &Direction| {
                    let v = d.0.each_ref().map(|x| x.to_f64());
                    let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
                    v.map(|x| x / norm)
                };
                if let Some(z) = &placement.axis
                    && !vector(&target["axis"][1], normalized(z))
                {
                    detail.push(format!("axis {} vs {:?}", target["axis"][1], normalized(z)));
                }
                if let Some(x) = &placement.ref_direction
                    && !vector(&target["ref_direction"], normalized(x))
                {
                    detail.push(format!(
                        "reference direction {} vs {:?}",
                        target["ref_direction"],
                        normalized(x)
                    ));
                }
            }
            if let Some(f) = t.on() {
                let mut expected: Vec<_> = cover(p, f)
                    .iter()
                    .map(|a| match *a {
                        Anchor::Face(f) => parts[pi].faces[f.0],
                        Anchor::Edge(e) => parts[pi].edges[e.0],
                        Anchor::Geometry(g) => prov.geometry_items[g.0],
                    })
                    .collect();
                let mut actual: Vec<_> = o["faces"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|f| {
                        ["advanced_face", "edge_curve", "instance"]
                            .iter()
                            .find_map(|k| f[*k].as_str())
                            .and_then(|s| s.strip_prefix('#'))
                            .and_then(|s| s.parse::<u64>().ok())
                    })
                    .collect();
                expected.sort_unstable();
                expected.dedup();
                actual.sort_unstable();
                actual.dedup();
                if expected != actual {
                    detail.push(format!("support instances {actual:?} vs {expected:?}"));
                }
            }
            if !detail.is_empty() {
                out.push((key, detail.join("; ")));
            }
        }
        // Datums by label and faces (OpenCascade keeps one per tolerance that cites it).
        let mut theirs: Vec<(String, Vec<usize>)> = odatums
            .iter()
            .filter(|d| !d["is_target"].as_bool().unwrap_or(false))
            .filter_map(|d| {
                Some((
                    d["name"].as_str().unwrap().to_string(),
                    on_part(&d["faces"])?,
                ))
            })
            .collect();
        theirs.sort();
        theirs.dedup();
        for (i, d) in p.datums.iter().enumerate() {
            let ours = d.feature().map(|f| face_cover(p, f)).unwrap_or_default();
            let key = (d.label().to_string(), ours.clone());
            if let Some(j) = theirs.iter().position(|x| *x == key) {
                theirs.remove(j);
            } else if d.feature().is_some() {
                let id = root(doc, &prov.datums[i], &["datum"]);
                out.push((
                    format!("{name} occt missing datum {} #{id}", d.label()),
                    format!("faces {ours:?}"),
                ));
            }
        }
        for (l, f) in theirs {
            out.push((
                format!("{name} occt datum {l} {f:?}"),
                "a datum the reader does not have on those faces".into(),
            ));
        }
    }
    // Items on faces of no part the reader has.
    for (k, list) in [("dimension", &odims), ("tolerance", &otols)] {
        for o in list.iter() {
            if o["type"] == "DimensionPresentation" || o["faces"].as_array().unwrap().is_empty() {
                continue;
            }
            if occt_faces(&o["faces"], parts, r).is_none() {
                out.push((
                    format!("{name} occt {k} {}", o["label"].as_str().unwrap()),
                    format!(
                        "{} on faces that are not one part's: {}",
                        o["type"], o["faces"]
                    ),
                ));
            }
        }
    }
    // Notes on the part ('semantic text', §7.4): XCAF's metadata of the part's label, which
    // holds one string per property name; and what it holds that the reader does not.
    let metadata: Vec<&Json> = occt["metadata"].as_array().unwrap().iter().collect();
    for (pi, p) in r.parts.iter().enumerate() {
        let strings = metadata
            .iter()
            .find(|m| m["part"].as_u64() == Some(pi as u64))
            .map(|m| m["strings"].clone())
            .unwrap_or(Json::Null);
        let mut ours: Vec<String> = Vec::new();
        for a in p.attributes.iter().filter(|a| a.on.is_none()) {
            for (item, v) in &a.items {
                let AttributeValue::Text(text) = v else {
                    continue;
                };
                ours.push(text.clone());
                if strings[&a.name].as_str() != Some(text.as_str()) {
                    out.push((
                        format!("{name} occt part {pi} {} {item:?}", a.name),
                        format!(
                            "not in XCAF's metadata, which holds {:?} = {}",
                            a.name, strings[&a.name]
                        ),
                    ));
                }
            }
        }
        if let Json::Object(m) = &strings {
            for (k, v) in m {
                if !ours.iter().any(|t| Some(t.as_str()) == v.as_str()) {
                    out.push((
                        format!("{name} occt part {pi} metadata {k:?}"),
                        format!("{v} is no text the reader has on the part"),
                    ));
                }
            }
        }
        for (i, a) in p
            .attributes
            .iter()
            .enumerate()
            .filter(|(_, a)| a.name == "semantic text" && a.on.is_some())
        {
            for (_, value) in &a.items {
                if let AttributeValue::Text(text) = value
                    && !metadata
                        .iter()
                        .any(|m| m["strings"]["semantic text"].as_str() == Some(text))
                {
                    out.push((
                        format!("{name} occt part {pi} feature semantic text {i}"),
                        format!("not in XCAF metadata: {text:?}"),
                    ));
                }
            }
        }
        // Surface texture: XCAF has none.
        for (i, s) in p.surface_textures.iter().enumerate() {
            out.push((
                format!("{name} occt part {pi} surface texture {i}"),
                format!(
                    "not read: {} {}",
                    s.material_removal.term(),
                    s.parameters
                        .iter()
                        .map(|x| format!(
                            "{} {}{:?}",
                            x.characteristic, x.value.value, x.value.unit
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ));
        }
    }
    // XCAF's tolerance objects have no frame relations or general-table API.
    for (pi, p) in r.parts.iter().enumerate() {
        if p.decimal_places.is_some()
            && !p
                .general
                .iter()
                .any(|g| matches!(g, GeneralTolerance::Table { .. }))
        {
            out.push((
                format!("{name} occt part {pi} decimal places"),
                "XCAF exposes no part decimal default".into(),
            ));
        }
        for (i, t) in p.tolerances.iter().enumerate() {
            if t.zone.as_ref().and_then(|z| z.affected_plane).is_some()
                && !otols.iter().any(|o| {
                    o["affected_plane"].is_string()
                        && t.magnitude.as_ref().is_some_and(|m| {
                            occt_value(o["value"].as_f64().unwrap_or(0.), m).is_none()
                        })
                })
            {
                out.push((
                    format!("{name} occt part {pi} affected plane {i}"),
                    "XCAF exposes no affected plane on the tolerance".into(),
                ));
            }
        }
        for (i, rel) in p.tolerance_relations.iter().enumerate() {
            out.push((
                format!("{name} occt part {pi} tolerance relation {i}"),
                format!(
                    "{:?}: XCAF exposes the individual tolerances, no relation",
                    rel.kind
                ),
            ));
        }
        for (i, g) in p.general.iter().enumerate() {
            if let GeneralTolerance::Table { .. } = g {
                out.push((
                    format!("{name} occt part {pi} default table {i}"),
                    "XCAF exposes no general tolerance table or part decimal default".into(),
                ));
            }
        }
        if let Some(m) = &p.material
            && let Some(d) = &m.density
        {
            let mut expected = d.value.to_f64();
            for (unit, power) in &d.unit {
                let scale: f64 = match unit.as_str() {
                    "kilogram" => 1000.,
                    "gram" => 1.,
                    "metre" => 100.,
                    "centimetre" => 1.,
                    _ => panic!("add expected density scale for {unit}"),
                };
                expected *= scale.powf(power.to_f64());
            }
            let material = occt["materials"]
                .as_array()
                .unwrap()
                .iter()
                .find(|o| o["name"].as_str() == Some(&m.id));
            let actual = material.and_then(|o| o["density"].as_f64());
            if actual.is_none_or(|a| (a - expected).abs() > 1e-10 * expected.abs()) {
                out.push((
                    format!("{name} occt part {pi} density"),
                    format!(
                        "XCAF {:?} g/cm³ vs {expected} g/cm³ from {} {:?}",
                        actual, d.value, d.unit
                    ),
                ));
            }
        }
    }
    // Material.
    let materials: Vec<String> = occt["materials"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["name"] != "pmi-assist answers")
        .map(|m| m["name"].as_str().unwrap().to_string())
        .collect();
    let ours: Vec<String> = r
        .parts
        .iter()
        .filter_map(|p| p.material.as_ref().map(|m| m.id.clone()))
        .collect();
    for m in &materials {
        if !ours.contains(m) {
            out.push((format!("{name} occt material {m}"), "not read".into()));
        }
    }
    out
}

/// The written files OpenCascade is asked to read (`tools/check_pmi_occt.py`): one with every
/// kind of item the writer makes, on a specify-core part; a written thread replaced and removed
/// (U14); an add beside specify-core's notes on faces (U10); and specify-core's intents written
/// onto their original corpus files. `None` for a case whose input is missing (the corpus).
fn written_cases() -> Vec<(String, Option<Vec<u8>>)> {
    let mut out = Vec::new();
    out.push((
        "pmi_gaps".into(),
        Some(
            written(&gap_spool(), &common::pmi_gaps::every_kind())
                .doc
                .bytes()
                .to_vec(),
        ),
    ));
    let mut gram = common::pmi_gaps::every_kind();
    gram.material.as_mut().unwrap().density = Some(Density {
        value: dec("7.85"),
        unit: vec![
            ("gram".into(), dec("1.")),
            ("centimetre".into(), dec("-3.")),
        ],
    });
    out.push((
        "pmi_gaps_gram".into(),
        Some(written(&gap_spool(), &gram).doc.bytes().to_vec()),
    ));
    // Every kind of item, replacing a specify-core part's PMI.
    let f = spool();
    let mut p = four_datums();
    p.features.extend([
        faces(&[4]),
        faces(&[5]),
        faces(&[6]),
        faces(&[7]),
        faces(&[8]),
        faces(&[9, 10]),
    ]);
    p.dimensions = vec![
        size(4, "20.", DimTolerance::Deviations(bounds("0.1", "-0.05"))),
        size(
            5,
            "70.",
            DimTolerance::Fit {
                class: fit("g6"),
                limits: None,
            },
        ),
        size(
            6,
            "20.",
            DimTolerance::Fit {
                class: fit("H7"),
                limits: Some(bounds("20.021", "20.000")),
            },
        ),
        size(7, "12.", DimTolerance::Limits(bounds("12.1", "11.9"))),
        Dimension {
            kind: DimensionKind::Location {
                from: FeatureId(0),
                to: FeatureId(8),
                kind: LocationKind::LinearDistance,
                path: None,
                directed: false,
                angle: None,
            },
            nominal: Some(mm("30.")),
            tolerance: DimTolerance::Basic,
            qualifier: None,
            modifiers: Vec::new(),
            principle: None,
        },
    ];
    let zone = |form: ZoneForm| Zone {
        form,
        projected: None,
        non_uniform: None,
        runout_angle: None,
        affected_plane: None,
        affected_plane_kind: None,
    };
    let mut pos = tolerance(
        ToleranceKind::Position,
        ToleranceTarget::Dimension(DimensionId(0)),
        "0.2",
    );
    pos.datums = Some(system(&[0, 1, 2]));
    pos.modifiers = vec![ToleranceModifier::MaximumMaterialRequirement];
    pos.zone = Some(zone(ZoneForm::CylindricalOrCircular));
    let mut perp = tolerance(
        ToleranceKind::Perpendicularity,
        ToleranceTarget::Feature(FeatureId(1)),
        "0.05",
    );
    perp.datums = Some(system(&[0]));
    let mut par = tolerance(
        ToleranceKind::Parallelism,
        ToleranceTarget::Feature(FeatureId(3)),
        "0.04",
    );
    par.datums = Some(system(&[3, 1, 2]));
    let mut run = tolerance(
        ToleranceKind::CircularRunout,
        ToleranceTarget::Feature(FeatureId(5)),
        "0.02",
    );
    run.datums = Some(system(&[0, 1]));
    let mut total = tolerance(
        ToleranceKind::TotalRunout,
        ToleranceTarget::Feature(FeatureId(6)),
        "0.03",
    );
    total.datums = Some(system(&[0]));
    let mut prof = tolerance(
        ToleranceKind::SurfaceProfile,
        ToleranceTarget::Feature(FeatureId(9)),
        "0.1",
    );
    prof.datums = Some(system(&[0, 1]));
    p.tolerances = vec![
        tolerance(
            ToleranceKind::Flatness,
            ToleranceTarget::Feature(FeatureId(0)),
            "0.02",
        ),
        pos,
        perp,
        par,
        run,
        total,
        prof,
    ];
    p.general = vec![GeneralTolerance::Class {
        text: "ISO 2768-mK".into(),
        standard: pmi::standards::Iso2768::recognise("ISO 2768-mK"),
    }];
    p.material = Some(Material {
        id: "Aluminium 6082-T6".into(),
        name: None,
        density: None,
    });
    p.attributes = vec![AttributeSet {
        name: "inspection".into(),
        on: Some(NoteOwner::Feature(FeatureId(4))),
        items: vec![("gauge".into(), AttributeValue::Text("plug".into()))],
        description: None,
        general_property: None,
    }];
    p.attributes.extend(part_notes());
    p.surface_textures = textures(Some(FeatureId(9)));
    let (edit, _) = pmi::write(
        &f.doc,
        &f.parts,
        &[(PartId(0), p)],
        Mode::Replace,
        PresentationPolicy::RemovePresentation,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    out.push((
        "every_kind".to_string(),
        Some(f.doc.apply(&edit).unwrap().bytes),
    ));

    let f = gap_spool();
    for (name, p) in [
        (
            "datum_targets",
            common::pmi_targets::separate_support(common::pmi_targets::every_kind()),
        ),
        (
            "datum_targets_shared_support",
            common::pmi_targets::every_kind(),
        ),
        (
            "decimal_default",
            PartPmi {
                decimal_places: Some(3),
                ..PartPmi::default()
            },
        ),
    ] {
        out.push((name.into(), Some(written(&f, &p).doc.bytes().to_vec())));
    }
    for (name, source) in [
        ("nist_ctc_02_targets", "nist_ctc_02_asme1_ap242-e2"),
        ("nist_ftc_10_targets", "nist_ftc_10_asme1_ap242-e2"),
    ] {
        let f = fixture(&format!("ap242/nist/{source}.stp.gz"));
        let read = pmi::read(&f.doc, &f.parts).unwrap();
        let p =
            common::pmi_targets::with_occt_probes(common::pmi_targets::from_read(&read.parts[0]));
        out.push((name.into(), Some(written(&f, &p).doc.bytes().to_vec())));
    }

    // A thread haecceity wrote, written again (U14, `a_written_thread_is_replaced_and_removed`):
    // the internal thread replaced by the external thread and knurl, and those removed.
    let f = spool();
    let s = thread_sets(&f);
    for (name, first, mode, second) in [
        (
            "thread_replaced",
            &s.internal,
            Mode::Replace,
            &s.external_and_knurl,
        ),
        (
            "thread_removed",
            &s.external_and_knurl,
            Mode::Remove,
            &PartPmi::default(),
        ),
    ] {
        let g = open_bytes(written(&f, first).doc.bytes().to_vec());
        let w = write(&g, &[(PartId(0), second.clone())], mode)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_reads_back(&w, PartId(0), second);
        out.push((name.to_string(), Some(w.doc.bytes().to_vec())));
    }

    // specify-core's notes on faces (U10, `specify_core_notes_on_faces_are_anchored`): its
    // thumbwheel, whose external thread, internal thread and knurl notes the reader puts on
    // their faces, with a flatness added beside them.
    let dir = common::fixtures().join("ap242/specify");
    let f = open(&dir.join("thumbwheel_thread_knurl.step.gz"));
    let free = free_faces(&f, 0, 1);
    let flat = PartPmi {
        features: vec![faces(&[free[0]])],
        tolerances: vec![tolerance(
            ToleranceKind::Flatness,
            ToleranceTarget::Feature(FeatureId(0)),
            "0.05",
        )],
        ..PartPmi::default()
    };
    let w = write(&f, &[(PartId(0), flat)], Mode::Add).unwrap_or_else(|e| panic!("{e}"));
    let anchored = w.read.parts[0]
        .notes
        .iter()
        .filter(|n| n.on.is_some())
        .count();
    assert_eq!(anchored, 3, "{:#?}", w.read.parts[0].notes);
    out.push((
        "thumbwheel_notes_add".to_string(),
        Some(w.doc.bytes().to_vec()),
    ));

    // One part of an assembly written alone (`one_part_of_an_assembly_is_written_alone`).
    let (f, p) = editor_pin();
    let (edit, _) = pmi::write(
        &f.doc,
        &f.parts,
        &[(PartId(1), p)],
        Mode::Replace,
        PresentationPolicy::RemovePresentation,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    out.push((
        "editor_assembly_pin_alone".to_string(),
        Some(f.doc.apply(&edit).unwrap().bytes),
    ));

    // specify-core's intents onto the original files.
    for case in [
        "assembly_plate_pin",
        "spool_fits",
        "string_post_tapped",
        "thumbwheel_thread_knurl",
    ] {
        let meta: Json = serde_json::from_str(
            &std::fs::read_to_string(dir.join(format!("{case}.meta.json"))).unwrap(),
        )
        .unwrap();
        let Some(path) = original(meta["input"].as_str().unwrap()) else {
            out.push((format!("{case}_intent"), None));
            continue;
        };
        let intents: Json = serde_json::from_str(
            &std::fs::read_to_string(dir.join(format!("{case}.intent.json"))).unwrap(),
        )
        .unwrap();
        let intents: Vec<Json> = match intents {
            Json::Array(a) => a,
            one => vec![one],
        };
        let f = open(&path);
        let pmi_: Vec<(PartId, PartPmi)> = intents
            .iter()
            .map(|i| {
                (
                    PartId(i["binding"]["part"].as_u64().unwrap() as usize),
                    intent_pmi(i).0,
                )
            })
            .collect();
        let (edit, _) = pmi::write(
            &f.doc,
            &f.parts,
            &pmi_,
            Mode::Add,
            PresentationPolicy::RemovePresentation,
        )
        .unwrap_or_else(|e| panic!("{case}: {e}"));
        out.push((
            format!("{case}_intent"),
            Some(f.doc.apply(&edit).unwrap().bytes),
        ));
    }
    out
}

fn write_dir() -> PathBuf {
    common::fixtures().join(if cfg!(feature = "presentation") {
        "ap242/write-presentation"
    } else {
        "ap242/write"
    })
}

/// Development aid: writes the written cases to `tests/fixtures/ap242/write/`
/// (`cargo test --release --test pmi_write export_written_files -- --ignored`), then
/// `tools/check_pmi_occt.py` captures OpenCascade's reading of them.
#[test]
#[ignore]
fn export_written_files() {
    std::fs::create_dir_all(write_dir()).unwrap();
    for (name, bytes) in written_cases() {
        let bytes = bytes.unwrap_or_else(|| panic!("{name}: input missing"));
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        std::io::Write::write_all(&mut gz, &bytes).unwrap();
        std::fs::write(
            write_dir().join(format!("{name}.step.gz")),
            gz.finish().unwrap(),
        )
        .unwrap();
    }
}

/// The committed written files are what the writer writes now (so OpenCascade's captures of
/// them are of the current writer's output); re-export and re-capture when this fails.
#[test]
fn written_files_are_the_writers_current_output() {
    for (name, bytes) in written_cases() {
        let Some(bytes) = bytes else { continue };
        let committed = file_bytes(&write_dir().join(format!("{name}.step.gz")));
        assert!(
            committed == bytes,
            "{name}: the writer's output changed; run export_written_files and tools/check_pmi_occt.py"
        );
        assert_composite_components(&Document::parse(bytes).unwrap());
    }
}

/// Oracle: OpenCascade XCAF's reading of each written file against haecceity's (equal to
/// what was written, checked above); every difference pinned with a verdict
/// (`references/pmi_write.json` "occt"); and every datum's symbol read as its presentation.
#[test]
fn opencascade_reads_the_written_files() {
    let pins = load_pins();
    let pinned = pins["occt"].as_array().unwrap();
    common::check_verdicts("references/pmi_write.json occt", pinned);
    let mut problems = Vec::new();
    let mut actual = Vec::new();
    let mut names: Vec<String> = std::fs::read_dir(write_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter_map(|n| n.strip_suffix(".occt.json.gz").map(str::to_string))
        .collect();
    names.sort();
    assert!(
        !names.is_empty(),
        "no OpenCascade captures of written files"
    );
    for name in names {
        let capture: Json = serde_json::from_slice(&file_bytes(
            &write_dir().join(format!("{name}.occt.json.gz")),
        ))
        .unwrap();
        let path = write_dir().join(format!("{name}.step.gz"));
        assert_eq!(
            capture["sha256"].as_str(),
            Some(common::sha256_hex(&std::fs::read(&path).unwrap()).as_str()),
            "{name}: the capture is not of the committed file"
        );
        // specify-core's load.load_all opens it (it crashed on the datum feature symbols'
        // mechanical_design_and_draughting_relationship, no longer written), and XCAF reads it
        // and every shape label's name.
        let unavailable = pins["loader_unavailable"]
            .as_object()
            .and_then(|p| p.get(&name));
        let loader_ok = capture["loads"] == true
            || unavailable.is_some_and(|pin| {
                pin["status"] == capture["loads"]
                    && pin["verdict"] == "not-applicable"
                    && pin["reason"].is_string()
            });
        if !loader_ok || capture["reads"] != true {
            problems.push(format!(
                "{name}: OpenCascade does not open it: loads {} reads {}",
                capture["loads"], capture["reads"]
            ));
            continue;
        }
        let f = open(&path);
        let r = pmi::read(&f.doc, &f.parts).unwrap();
        // Decision 6: datum feature symbols on actual datum features. Target-only datums
        // have target presentation, not a fabricated datum feature.
        let datums = capture["parts"]
            .as_array()
            .unwrap()
            .iter()
            .chain([&capture["unattached"]])
            .flat_map(|p| p["datums"].as_array().unwrap());
        for d in datums {
            if d["is_target"] == true
                || r.parts.iter().flat_map(|p| &p.datums).any(|ours| {
                    ours.feature().is_none()
                        && !ours.targets().is_empty()
                        && Some(ours.label().as_str()) == d["name"].as_str()
                })
            {
                continue;
            }
            let pres = &d["presentation"];
            let want = format!("Datum {}", d["name"].as_str().unwrap_or(""));
            if pres["name"].as_str() != Some(want.as_str())
                || pres["edges"].as_u64().unwrap_or(0) == 0
                || pres["plane"] != true
            {
                problems.push(format!(
                    "{name}: OpenCascade reads datum {} without its symbol: {pres}",
                    d["name"]
                ));
            }
        }
        for (key, detail) in compare_occt(&name, &capture, &r, &f.doc, &f.parts) {
            match pinned.iter().find(|e| e["key"] == key.as_str()) {
                Some(e) if e["detail"] == detail.as_str() => {}
                Some(e) => problems.push(format!(
                    "{key}: changed: pinned {} actual {detail}",
                    e["detail"]
                )),
                None => problems.push(format!("{key}: not pinned: {detail}")),
            }
            actual.push(serde_json::json!({"key": key, "detail": detail}));
        }
    }
    for e in pinned {
        if !actual.iter().any(|a| a["key"] == e["key"]) {
            problems.push(format!("{}: pinned but not found", e["key"]));
        }
    }
    if let Some(path) = std::env::var_os("HAECCEITY_PMI_OCCT_ACTUAL") {
        std::fs::write(path, serde_json::to_vec_pretty(&actual).unwrap()).unwrap();
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// OpenCascade XCAF on one part of an assembly written alone
/// (`write/editor_assembly_pin_alone`, from `one_part_of_an_assembly_is_written_alone`): every
/// item it reads is on the pin's own prototype label (a direct child of the shape tool's
/// label, not a component of the assembly, so both placements show it), on the pin's faces as
/// haecceity numbers them; the plate's label has nothing. Feature-on output additionally
/// carries symbolic notes/thread CommonLabels (no semantic dimension is unattached).
#[test]
fn opencascade_puts_a_parts_pmi_on_its_prototype() {
    let name = "editor_assembly_pin_alone";
    let capture: Json = serde_json::from_slice(&file_bytes(
        &write_dir().join(format!("{name}.occt.json.gz")),
    ))
    .unwrap();
    let f = open(&write_dir().join(format!("{name}.step.gz")));
    let pins = load_pins();
    let unavailable = &pins["loader_unavailable"][name];
    assert!(
        capture["loads"] == true
            || (unavailable["status"] == capture["loads"]
                && unavailable["verdict"] == "not-applicable"
                && unavailable["reason"].is_string())
    );
    assert_eq!(capture["reads"], true);
    let parts = capture["parts"].as_array().unwrap();
    let names: Vec<&str> = parts.iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["plate", "pin"]);
    const KINDS: [&str; 3] = ["dimensions", "geometric_tolerances", "datums"];
    for k in KINDS {
        assert!(parts[0][k].as_array().unwrap().is_empty(), "plate has {k}");
        if k != "dimensions" {
            assert!(
                capture["unattached"][k].as_array().unwrap().is_empty(),
                "unattached {k}"
            );
        }
    }
    let graphic_notes = capture["unattached"]["dimensions"].as_array().unwrap();
    #[cfg(not(feature = "presentation"))]
    assert!(graphic_notes.is_empty());
    #[cfg(feature = "presentation")]
    {
        // XCAF exposes the notes block and the new thread callout as symbolic
        // CommonLabels. The semantic counts/face/prototype assertions stay exact.
        assert_eq!(graphic_notes.len(), 2);
        for note in graphic_notes {
            assert_eq!(note["type"], "CommonLabel");
            assert_eq!(note["semantic_name"], "DGT:Common_label");
            assert_eq!(note["value"], 0.0);
            for key in ["faces", "faces2", "parts", "values", "modifiers"] {
                assert!(
                    note[key].as_array().unwrap().is_empty(),
                    "symbolic label {key}"
                );
            }
        }
        let reading = pmi::read(&f.doc, &f.parts).unwrap();
        assert_eq!(reading.parts[1].threads.len(), 1);
        assert!(reading.parts[0].material.is_none());
        assert!(reading.parts[0].general.is_empty());
        assert_eq!(reading.parts[1].material.as_ref().unwrap().id, "Steel C45");
        assert!(
            matches!(&reading.parts[1].general[..], [GeneralTolerance::Class{text,..}] if text=="ISO 2768-f")
        );
        let blocks:Vec<_>=f.doc.ids().filter(|&id| matches!(f.doc.get(id),Some(RawEntity::Simple{name,..}) if name.eq_ignore_ascii_case("draughting_callout")) && test_attribute(&f.doc,id,"representation_item","name")==Some(&Attribute::String("General notes".into()))).collect();
        assert_eq!(blocks.len(), 1);
        assert_eq!(refs_of(&f.doc, blocks[0]).len(), 2);
        assert_general_notes_names(
            &f.doc,
            blocks[0],
            "GENERAL NOTES\n1  GENERAL TOLERANCES ISO 2768-f\n2  MATERIAL: Steel C45",
        );
    }
    let pin = &parts[1];
    let label = pin["label"].as_str().unwrap();
    assert_eq!(
        label.split(':').count(),
        4,
        "the pin's label {label} is not a prototype"
    );
    assert_eq!(pin["faces"], 7);
    let counts: Vec<usize> = KINDS
        .iter()
        .map(|k| pin[k].as_array().unwrap().len())
        .collect();
    assert_eq!(counts, [1, 2, 4], "dimensions, tolerances, datums");
    let def = &f.parts[1];
    for k in KINDS {
        for item in pin[k].as_array().unwrap() {
            for face in item["faces"].as_array().unwrap() {
                assert_eq!(face["part"], 1, "{k} {item}");
                let n: u64 = face["advanced_face"].as_str().unwrap()[1..]
                    .parse()
                    .unwrap();
                assert_eq!(
                    def.face_index(n),
                    face["face"].as_u64().map(|i| i as usize),
                    "{k}: {face}"
                );
            }
        }
    }
}

/// These six AP203 files (the corpus's copies of NIST's) have only the two closed upgrade
/// defects. Adding a datum must upgrade them and read it back; an empty add must preserve even
/// their defective bytes.
#[test]
fn nist_ap203_upgrade_repairs() {
    let Some(root) = common::corpus_dir() else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let mut failures = Vec::new();
    use haecceity::pmi::write::UpgradeRepairKind::*;
    for (name, repair, ids) in [
        (
            "nist_ctc_01_asme1_rd.stp",
            R1ApprovalAssignment,
            vec![14, 15],
        ),
        (
            "nist_ctc_02_asme1_rc.stp",
            R2ConversionUnitDimensions,
            vec![20750],
        ),
        (
            "nist_ctc_03_asme1_rc.stp",
            R1ApprovalAssignment,
            vec![14, 15],
        ),
        (
            "nist_ctc_05_asme1_rd.stp",
            R2ConversionUnitDimensions,
            vec![6596, 6599, 6625, 6634, 6651, 6660, 6669],
        ),
        (
            "nist_ftc_06_asme1_rd.stp",
            R1ApprovalAssignment,
            vec![14, 15],
        ),
        (
            "nist_ftc_09_asme1_rd.stp",
            R2ConversionUnitDimensions,
            vec![5778, 5781, 5805, 5814, 5827, 5836],
        ),
    ] {
        let f = open(&root.join("nist").join(name));
        let empty = write(&f, &[(PartId(0), PartPmi::default())], Mode::Add).unwrap();
        assert_eq!(empty.doc.bytes(), f.doc.bytes(), "{name}: empty add");
        assert!(empty.report.repairs.is_empty());
        let p = PartPmi {
            features: vec![faces(&[0])],
            datums: vec![datum("Z", 0)],
            ..PartPmi::default()
        };
        let w = match write(&f, &[(PartId(0), p.clone())], Mode::Add) {
            Ok(w) => w,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        assert_eq!(w.doc.file_schema().unwrap(), [express::TARGET_SCHEMA]);
        assert_reads_back(&w, PartId(0), &p);
        assert_eq!(
            w.report
                .repairs
                .iter()
                .map(|r| (r.id, r.repair))
                .collect::<Vec<_>>(),
            ids.into_iter().map(|id| (id, repair)).collect::<Vec<_>>(),
            "{name}"
        );
        assert_repair_bytes(&f.doc, &w.doc, &w.report);
        assert!(express::TARGET.validate_document(w.doc.graph()).is_empty());
        assert!(new_violations(&f.doc, &w.doc).is_empty());
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn ap214_with_records(records: &str) -> File {
    let f = fixture("ap242/assembly/assembly.step");
    let mut bytes = f.doc.bytes().to_vec();
    let end = bytes.windows(7).rposition(|w| w == b"ENDSEC;").unwrap();
    bytes.splice(end..end, records.bytes());
    open_bytes(bytes)
}

fn upgrade_datum() -> PartPmi {
    PartPmi {
        features: vec![faces(&[0])],
        datums: vec![datum("Z", 0)],
        ..PartPmi::default()
    }
}

#[test]
fn upgrade_refuses_approval_items_outside_both_selects() {
    // Edition 4's approval_item takes any representation_item, so the item is an entity in
    // neither select and with no supertype in either.
    let f = ap214_with_records(
        "#900000000=APPROVAL_STATUS('approved');\n\
         #900000001=APPROVAL(#900000000,'');\n\
         #900000002=DIMENSIONAL_EXPONENTS(0.,0.,0.,0.,0.,0.,0.);\n\
         #900000003=CC_DESIGN_APPROVAL(#900000001,(#900000002));\n",
    );
    assert_eq!(
        write(&f, &[(PartId(0), PartPmi::default())], Mode::Add)
            .unwrap()
            .doc
            .bytes(),
        f.doc.bytes()
    );
    match write(&f, &[(PartId(0), upgrade_datum())], Mode::Add) {
        Err(WriteError::Edition { violations, .. }) => {
            assert!(
                violations.iter().any(|v| v.id == 900000003
                    && v.attribute.as_deref() == Some("cc_design_approval.items"))
            );
        }
        other => panic!("not refused: {:?}", other.err()),
    }
}

#[test]
fn conversion_dimensions_reuse_and_follow_conversion_and_derived_units() {
    let f = ap214_with_records(
        "#900000000=DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.);\n\
         #900000001=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n\
         #900000002=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#900000001);\n\
         #900000003=(CONVERSION_BASED_UNIT('INCH',#900000002) LENGTH_UNIT() NAMED_UNIT(*));\n\
         #900000004=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n\
         #900000005=PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(0.0174532925199433),#900000004);\n\
         #900000006=(CONVERSION_BASED_UNIT('DEGREE',#900000005) NAMED_UNIT(*) PLANE_ANGLE_UNIT());\n\
         #900000007=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(12.),#900000003);\n\
         #900000008=(CONVERSION_BASED_UNIT('FOOT',#900000007) LENGTH_UNIT() NAMED_UNIT(*));\n\
         #900000009=DERIVED_UNIT_ELEMENT(#900000001,2.);\n\
         #900000010=DERIVED_UNIT_ELEMENT(#900000001,-1.);\n\
         #900000011=DERIVED_UNIT((#900000009,#900000010));\n\
         #900000012=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),#900000011);\n\
         #900000013=(CONVERSION_BASED_UNIT('DERIVED LENGTH',#900000012) LENGTH_UNIT() NAMED_UNIT(*));\n\
         #900000014=(CONVERSION_BASED_UNIT('SECOND ANGLE UNIT',#900000005) NAMED_UNIT(*) PLANE_ANGLE_UNIT());\n\
         #900000015=(CONVERSION_BASED_UNIT('EXPLICIT INCH',#900000002) LENGTH_UNIT() NAMED_UNIT(#900000000));\n",
    );
    let empty = write(&f, &[(PartId(0), PartPmi::default())], Mode::Add).unwrap();
    assert!(empty.report.repairs.is_empty());
    assert_eq!(empty.doc.bytes(), f.doc.bytes());
    let mut p = upgrade_datum();
    // The repair must also share exponents the PMI emitter just added, using its provisional id.
    p.attributes.push(AttributeSet {
        name: "inspection angle".into(),
        on: None,
        items: vec![(
            "angle".into(),
            AttributeValue::Measure(Value::angle(dec("30."), AngleUnit::Degree)),
        )],
        description: None,
        general_property: None,
    });
    let w = write(&f, &[(PartId(0), p.clone())], Mode::Add).unwrap();
    assert_reads_back(&w, PartId(0), &p);
    assert_eq!(
        w.added()
            .iter()
            .filter(|&&id| w.names(id) == ["dimensional_exponents"])
            .count(),
        1
    );
    assert_eq!(
        w.report
            .repairs
            .iter()
            .map(|r| (r.id, r.repair))
            .collect::<Vec<_>>(),
        [900000003, 900000006, 900000008, 900000013, 900000014].map(|id| (
            id,
            haecceity::pmi::write::UpgradeRepairKind::R2ConversionUnitDimensions
        ))
    );
    for (id, dimensions) in [
        (900000003, [1., 0., 0., 0., 0., 0., 0.]),
        (900000006, [0.; 7]),
        (900000014, [0.; 7]),
        (900000008, [1., 0., 0., 0., 0., 0., 0.]),
        (900000013, [1., 0., 0., 0., 0., 0., 0.]),
    ] {
        let RawEntity::Complex { parts, .. } = w.doc.get(id).unwrap() else {
            panic!()
        };
        let named = parts.iter().find(|p| p.name == "NAMED_UNIT").unwrap();
        let Attribute::EntityRef(dim_id) = named.attributes[0] else {
            panic!()
        };
        if id != 900000006 && id != 900000014 {
            assert_eq!(dim_id, 900000000, "reuse length dimensions");
        }
        let RawEntity::Simple {
            name, attributes, ..
        } = w.doc.get(dim_id).unwrap()
        else {
            panic!()
        };
        assert_eq!(name, "DIMENSIONAL_EXPONENTS");
        assert_eq!(attributes, &dimensions.map(Attribute::Real));
        let RawEntity::Complex {
            parts: original, ..
        } = f.doc.get(id).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            parts
                .iter()
                .filter(|p| p.name != "NAMED_UNIT")
                .collect::<Vec<_>>(),
            original
                .iter()
                .filter(|p| p.name != "NAMED_UNIT")
                .collect::<Vec<_>>()
        );
        assert!(parts.windows(2).all(|p| p[0].name < p[1].name));
    }
    assert_repair_bytes(&f.doc, &w.doc, &w.report);
}

#[test]
fn conversion_dimensions_that_cannot_be_determined_are_refused() {
    for unit in ["#900000000", "#900000002"] {
        let f = ap214_with_records(&format!(
            "#900000000=DIRECTION('',(1.,0.,0.));\n\
             #900000001=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),{unit});\n\
             #900000002=(CONVERSION_BASED_UNIT('UNKNOWN',#900000001) LENGTH_UNIT() NAMED_UNIT(*));\n"
        ));
        let error = write(&f, &[(PartId(0), upgrade_datum())], Mode::Add)
            .err()
            .unwrap();
        assert!(matches!(error, WriteError::Edition { .. }));
        assert!(error.to_string().contains("#900000002"));
        assert!(error.to_string().contains("cannot determine"));
    }
}

/// Check the report's exact records and all intervening original bytes (including comments
/// and whitespace); only FILE_SCHEMA, repaired records and appended instances may differ.
fn assert_repair_bytes(before: &Document, after: &Document, report: &WriteReport) {
    let repaired: BTreeSet<u64> = report.repairs.iter().map(|r| r.id).collect();
    for id in before.ids() {
        if !repaired.contains(&id) {
            assert_eq!(
                before.bytes()[before.span(id).unwrap()],
                after.bytes()[after.span(id).unwrap()],
                "untouched #{id}"
            );
        }
    }
    for r in &report.repairs {
        assert_eq!(
            r.original_record.as_bytes(),
            &before.bytes()[before.span(r.id).unwrap()]
        );
        assert_eq!(
            r.new_record.as_bytes(),
            &after.bytes()[after.span(r.id).unwrap()]
        );
        assert!(r.to_string().contains(&format!("#{}", r.id)));
    }
    // Reconstruct the output directly from original bytes plus the reported replacements,
    // the schema replacement and exactly the added records. This also verifies non-record bytes.
    let schema_span = |doc: &Document| {
        let start = doc
            .bytes()
            .windows(11)
            .position(|w| w == b"FILE_SCHEMA")
            .unwrap();
        let end = start
            + doc.bytes()[start..]
                .iter()
                .position(|&b| b == b';')
                .unwrap()
            + 1;
        start..end
    };
    let mut replacements: Vec<_> = report
        .repairs
        .iter()
        .map(|r| (before.span(r.id).unwrap(), r.new_record.as_bytes().to_vec()))
        .collect();
    replacements.push((
        schema_span(before),
        after.bytes()[schema_span(after)].to_vec(),
    ));
    replacements.sort_by_key(|(span, _)| span.start);
    let mut expected = before.bytes().to_vec();
    for (span, text) in replacements.into_iter().rev() {
        expected.splice(span, text);
    }
    let new_ids: Vec<_> = after.ids().filter(|&id| id > before.max_id()).collect();
    assert_eq!(report.added, new_ids.len());
    let insertion = expected.windows(7).rposition(|w| w == b"ENDSEC;").unwrap();
    let first = after.span(*new_ids.first().unwrap()).unwrap().start;
    let end = after
        .bytes()
        .windows(7)
        .rposition(|w| w == b"ENDSEC;")
        .unwrap();
    expected.splice(
        insertion..insertion,
        after.bytes()[first..end].iter().copied(),
    );
    assert_eq!(expected, after.bytes(), "all untouched bytes");
}

#[test]
fn gaps_write_standard_entities_and_migrate_notes() {
    let f = gap_spool();
    let p = common::pmi_gaps::every_kind();
    let w = written(&f, &p);
    assert_verified(&f, &w, &p);
    assert_eq!(w.report.note_migrations.len(), 7);
    assert!(w.report.note_migrations[5].2.starts_with("surface texture"));
    assert!(w.report.note_migrations[6].2.starts_with("semantic text"));
    assert!(!String::from_utf8_lossy(w.doc.bytes()).contains("manufacturing requirement"));
    let types: BTreeSet<_> = w.added().into_iter().flat_map(|id| w.names(id)).collect();
    for name in [
        "geometric_tolerance_relationship",
        "default_tolerance_table",
        "default_tolerance_table_cell",
        "derived_unit",
        "derived_unit_element",
        "surface_texture_representation",
        "general_property_association",
        "projected_zone_definition",
    ] {
        assert!(types.contains(name), "{name}");
    }
    let made: BTreeSet<_> = w.added().into_iter().collect();
    assert!(
        express::TARGET
            .validate_document(w.doc.graph())
            .iter()
            .all(|v| !made.contains(&v.id))
    );
    assert!(
        haecceity::express_rules::check_all(&w.doc)
            .iter()
            .all(|v| !made.contains(&v.id))
    );
}

#[test]
fn five_numbered_part_notes_keep_order_on_replace_and_resume() {
    let all = pmi::write::standard_form(&common::pmi_gaps::every_kind());
    let notes: Vec<_> = all
        .attributes
        .into_iter()
        .filter(|a| a.on.is_none())
        .collect();
    let text = |p: &PartPmi| {
        p.attributes
            .iter()
            .filter(|a| a.name == "semantic text" && a.on.is_none())
            .flat_map(|a| &a.items)
            .map(|(_, v)| v.clone())
            .collect::<Vec<_>>()
    };
    let p = PartPmi {
        attributes: notes[..3].to_vec(),
        ..PartPmi::default()
    };
    let first = written(&spool(), &p);
    let f = open_bytes(first.doc.bytes().to_vec());
    let rest = PartPmi {
        attributes: notes[3..].to_vec(),
        ..PartPmi::default()
    };
    let resumed = write(&f, &[(PartId(0), rest.clone())], Mode::Add).unwrap();
    assert_verified_as(&f, &resumed, &rest, Mode::Add);
    let expected = PartPmi {
        attributes: notes,
        ..PartPmi::default()
    };
    assert_eq!(text(&resumed.read.parts[0]), text(&expected));
    let replaced = written(
        &open_bytes(resumed.doc.bytes().to_vec()),
        &resumed.read.parts[0],
    );
    assert_eq!(text(&replaced.read.parts[0]), text(&expected));
    let note_owners: Vec<_> = replaced
        .added()
        .into_iter()
        .filter(|&id| {
            replaced
                .text(id)
                .contains("PROPERTY_DEFINITION('semantic text'")
        })
        .collect();
    assert_eq!(note_owners.len(), 5);
}

#[test]
fn python_requirements_replace_as_standard_requirements() {
    for name in [
        "assembly_plate_pin",
        "bolt_thread_knurl",
        "bracket_positions",
        "spool_fits",
        "string_post_tapped",
        "thumbwheel_thread_knurl",
        "nist_ctc_01_merge",
    ] {
        let f = fixture(&format!("ap242/specify/{name}.step.gz"));
        let old = pmi::read(&f.doc, &f.parts).unwrap();
        let requests: Vec<_> = old
            .parts
            .iter()
            .enumerate()
            .map(|(i, p)| {
                (
                    PartId(i),
                    PartPmi {
                        features: p.features.clone(),
                        geometry: p.geometry.clone(),
                        derived_geometry: p.derived_geometry.clone(),
                        notes: p.notes.clone(),
                        ..PartPmi::default()
                    },
                )
            })
            .collect();
        let w = write(&f, &requests, Mode::Replace).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            !String::from_utf8_lossy(w.doc.bytes()).contains("manufacturing requirement"),
            "{name}"
        );
        assert_eq!(
            w.report.note_migrations.len(),
            old.parts.iter().map(|p| p.notes.len()).sum::<usize>()
        );
        let expected_callouts: Vec<_> = old
            .provenance
            .parts
            .iter()
            .enumerate()
            .flat_map(|(part, prov)| {
                prov.note_callouts
                    .iter()
                    .enumerate()
                    .flat_map(move |(note, ids)| {
                        ids.iter().map(move |&id| (PartId(part), note, id))
                    })
            })
            .collect();
        if matches!(name, "assembly_plate_pin" | "bolt_thread_knurl") {
            assert!(
                !expected_callouts.is_empty(),
                "{name} must exercise real Python presentation"
            );
        }
        for (part, note, id) in &expected_callouts {
            assert!(
                w.doc.get(*id).is_none(),
                "{name}: old callout #{id} survived"
            );
            assert!(
                w.report.presentation_removed.iter().any(|(i, _)| i == id),
                "{name}: unreported #{id}"
            );
            assert!(
                w.report
                    .note_presentation_removed
                    .iter()
                    .any(|(p, n, i, _)| p == part && n == note && i == id),
                "{name}: #{id} lost its note association"
            );
        }
        assert_eq!(
            w.report.note_presentation_removed.len(),
            expected_callouts.len()
        );
        for (part, p) in &requests {
            assert_reads_back(&w, *part, p);
        }
        let again: Vec<_> = w
            .read
            .parts
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, p)| (PartId(i), p))
            .collect();
        let second = write(&open_bytes(w.doc.bytes().to_vec()), &again, Mode::Replace).unwrap();
        for (part, p) in &again {
            assert_reads_back(&second, *part, p);
        }
    }
}

#[test]
fn relation_targets_still_require_edition_four() {
    let mut p = common::pmi_gaps::every_kind();
    p.material = None;
    let Err(WriteError::Refused(r)) = write(&spool(), &[(PartId(0), p)], Mode::Replace) else {
        panic!("edition 1 excludes relation targets");
    };
    assert!(
        r.iter()
            .any(|r| r.item == ItemRef::Tolerance(2) && r.why.contains("edition 4"))
    );
}

#[test]
fn tolerance_table_counts_are_numbers_and_values_keep_qualifiers() {
    let mut p = common::pmi_gaps::every_kind();
    let GeneralTolerance::Table { cells, .. } = &mut p.general[0] else {
        unreachable!()
    };
    cells[0].items[3].1 = CellValue::Count(dec("3")); // schema COUNT_MEASURE = NUMBER
    let CellValue::Value(v) = &mut cells[0].items[1].1 else {
        unreachable!()
    };
    v.decimal_places = Some(2);
    let w = written(&gap_spool(), &p);
    assert_verified(&gap_spool(), &w, &p);
    assert!(
        w.added()
            .iter()
            .any(|&id| w.text(id).contains("COUNT_MEASURE(3)"))
    );
}

#[test]
fn a_refused_migrated_note_names_the_original_input_item() {
    let note = |kind: &str, text: &str, feature| Note {
        kind: kind.into(),
        text: text.into(),
        on: Some(NoteOwner::Feature(FeatureId(feature))),
        callouts: Vec::new(),
    };
    let mut p = PartPmi {
        features: vec![
            faces(&[0]),
            Feature::Group {
                members: vec![FeatureId(0)],
                kind: GroupKind::Unstated,
            },
        ],
        attributes: vec![AttributeSet {
            name: "existing attribute".into(),
            on: Some(NoteOwner::Feature(FeatureId(0))),
            items: vec![(
                "instruction".into(),
                AttributeValue::Text("Keep clean.".into()),
            )],
            description: None,
            general_property: None,
        }],
        notes: vec![
            note("surface finish", "Ra 1.6 µm", 0),
            note("instruction", "Keep clean.", 0),
            note("instruction", "Protect this group.", 1),
            note("surface finish", "Ra 3.2 µm", 1),
        ],
        ..PartPmi::default()
    };
    // Offset both destination lists, and interleave migrations to exercise both filters.
    p.surface_textures
        .push(pmi::write::standard_form(&p).surface_textures[0].clone());
    let Err(WriteError::Refused(r)) = write(&gap_spool(), &[(PartId(0), p)], Mode::Replace) else {
        panic!("an unstated group and the notes on it must be refused");
    };
    assert_eq!(
        r.iter().map(|r| r.item).collect::<BTreeSet<_>>(),
        BTreeSet::from([ItemRef::Feature(1), ItemRef::Note(2), ItemRef::Note(3)])
    );
    for refusal in r.iter().filter(|r| matches!(r.item, ItemRef::Note(_))) {
        assert!(refusal.why.contains("owner feature 1 is refused"));
        assert!(refusal.why.contains("unstated kind"));
    }
}

#[test]
fn a_legacy_target_note_migrates_to_semantic_text() {
    let p = PartPmi {
        features: vec![faces(&[0])],
        datum_targets: vec![
            DatumTarget::new(1, TargetShape::Area(FeatureId(0)), None, None, None).unwrap(),
        ],
        datums: vec![Datum::new(label("A"), None, vec![DatumTargetId(0)]).unwrap()],
        notes: vec![Note {
            kind: "target note".into(),
            text: "Keep this target clean.".into(),
            on: Some(NoteOwner::DatumTarget(DatumTargetId(0))),
            callouts: Vec::new(),
        }],
        ..PartPmi::default()
    };
    let f = gap_spool();
    let w = written(&f, &p);
    assert_verified(&f, &w, &p);
    assert_eq!(w.report.note_migrations.len(), 1);
    assert!(w.report.note_migrations[0].2.starts_with("semantic text"));
    assert_eq!(w.report.practice_schema_conflicts.len(), 1);
}

#[test]
fn legacy_finish_numbers_migrate_without_refusing_the_note() {
    for (text, textures) in [("Ra 3e0 µm", 1), ("Ra 1e9999 µm", 0), ("Ra 32", 0)] {
        let p = PartPmi {
            features: vec![faces(&[0])],
            notes: vec![Note {
                kind: "surface finish".into(),
                text: text.into(),
                on: Some(NoteOwner::Feature(FeatureId(0))),
                callouts: Vec::new(),
            }],
            ..PartPmi::default()
        };
        let w = written(&gap_spool(), &p);
        assert_eq!(w.read.parts[0].surface_textures.len(), textures);
        assert_eq!(w.report.note_migrations.len(), 1);
        if textures == 0 {
            assert!(w.read.parts[0].attributes.iter().any(|a| {
                a.items
                    .iter()
                    .any(|(_, v)| *v == AttributeValue::Text(text.into()))
            }));
        }
    }
}

#[test]
fn tolerance_relationship_cycles_are_refused_by_name() {
    let mut p = common::pmi_gaps::every_kind();
    // Opposite directions with different names must still be acyclic.
    p.tolerance_relations.push(ToleranceRelation {
        kind: RelationKind::Precedence,
        relating: ToleranceId(1),
        related: ToleranceId(0),
    });
    let Err(WriteError::Invalid { errors, .. }) =
        write(&gap_spool(), &[(PartId(0), p)], Mode::Replace)
    else {
        panic!("cycle must be refused");
    };
    assert!(
        errors
            .iter()
            .any(|e| e.0.contains("geometric_tolerance_relationship.WR3"))
    );
}

#[test]
fn affected_plane_variants_and_ambiguous_refusal() {
    for kind in [
        AffectedPlaneKind::IntersectionPlane,
        AffectedPlaneKind::OrientationPlane,
        AffectedPlaneKind::IntersectionLine,
    ] {
        let mut p = common::pmi_gaps::every_kind();
        p.tolerances[0].zone.as_mut().unwrap().affected_plane_kind = Some(kind);
        let description = match kind {
            AffectedPlaneKind::IntersectionPlane => "representative plane element",
            AffectedPlaneKind::OrientationPlane => "",
            AffectedPlaneKind::IntersectionLine => {
                let point = p.geometry[0].item.leaves[0].1[1].clone();
                let mut second = point.clone();
                if let GeometryValue::Item(item) = &mut second
                    && let GeometryValue::List(coords) = &mut item.leaves[0].1[1]
                {
                    coords[0] = GeometryValue::Real(dec("10."));
                }
                p.geometry[0].item = GeometryItem {
                    leaves: vec![(
                        "POLYLINE".into(),
                        vec![
                            GeometryValue::Text("".into()),
                            GeometryValue::List(vec![point, second]),
                        ],
                    )],
                };
                "representative line element"
            }
        };
        let f = gap_spool();
        let w = written(&f, &p);
        assert_verified(&f, &w, &p);
        let z = w.read.parts[0].tolerances[0].zone.as_ref().unwrap();
        assert_eq!(z.affected_plane_kind, Some(kind));
        let aspects: Vec<_> = w
            .added()
            .into_iter()
            .filter(|&id| {
                w.names(id) == ["shape_aspect"] && w.text(id).contains(&format!("'{description}'"))
            })
            .collect();
        assert!(!aspects.is_empty());
    }
    let mut p = common::pmi_gaps::every_kind();
    p.tolerances[0].zone.as_mut().unwrap().affected_plane_kind = None;
    let Err(WriteError::Refused(r)) = write(&gap_spool(), &[(PartId(0), p)], Mode::Replace) else {
        panic!("ambiguous plane");
    };
    assert!(
        r.iter()
            .any(|r| r.item == ItemRef::Tolerance(0) && r.why.contains("explicit intersection"))
    );
}

#[test]
fn isolated_relationship_endpoints_keep_occt_tolerances_on_their_faces() {
    for name in ["pmi_gaps", "pmi_gaps_gram"] {
        let capture: Json = serde_json::from_slice(&file_bytes(
            &write_dir().join(format!("{name}.occt.json.gz")),
        ))
        .unwrap();
        let tol: Vec<_> = capture["parts"]
            .as_array()
            .unwrap()
            .iter()
            .chain([&capture["unattached"]])
            .flat_map(|p| p["geometric_tolerances"].as_array().unwrap())
            .collect();
        for value in [0.2, 0.1] {
            let t = tol
                .iter()
                .find(|t| t["value"].as_f64() == Some(value))
                .expect("ordinary Position tolerance read by OCCT");
            assert_eq!(
                t["faces"],
                serde_json::json!([{"advanced_face":"#17", "face":0, "part":0}]),
                "{name}: {t}"
            );
        }
    }
}

#[test]
fn unitless_roughness_on_an_inch_part_keeps_its_words() {
    let f = fixture("ap242/nist/nist_stc_06_asme1_ap242-e3.stp.gz");
    let p = PartPmi {
        features: vec![faces(&[0])],
        notes: vec![Note {
            kind: "surface finish".into(),
            text: "Ra 32".into(),
            on: Some(NoteOwner::Feature(FeatureId(0))),
            callouts: Vec::new(),
        }],
        ..PartPmi::default()
    };
    let w = written(&f, &p);
    assert!(w.read.parts[0].surface_textures.is_empty());
    assert!(w.report.note_migrations[0].2.starts_with("semantic text"));
    assert!(w.report.note_migrations[0].2.contains("units"));
    assert_eq!(
        w.read.parts[0].attributes[0].items[0].1,
        AttributeValue::Text("Ra 32".into())
    );
}

#[test]
fn practice_targets_and_decimal_cells_have_only_reported_conflicts() {
    let p = common::pmi_targets::every_kind();
    let f = gap_spool();
    let w = written(&f, &p);
    assert_verified(&f, &w, &p);
    let conflicts = &w.report.practice_schema_conflicts;
    assert_eq!(
        conflicts
            .iter()
            .filter(|v| v.rule == "datum_target.WR5")
            .count(),
        7
    );
    assert_eq!(
        conflicts
            .iter()
            .filter(|v| v.rule.starts_with("default_tolerance_table_cell."))
            .count(),
        2
    );
    let made: BTreeSet<_> = w.added().into_iter().collect();
    assert!(conflicts.iter().all(|v| made.contains(&v.id)));
    assert!(
        express::validate_document(w.doc.graph())
            .iter()
            .all(|v| !made.contains(&v.id))
    );
    let raw: Vec<_> = haecceity::express_rules::check_all(&w.doc)
        .into_iter()
        .filter(|v| made.contains(&v.id))
        .collect();
    assert_eq!(
        raw, *conflicts,
        "the unmodified rule engine must still report exactly the conflicts"
    );
    let target = conflicts
        .iter()
        .find(|v| v.rule == "datum_target.WR5")
        .unwrap()
        .id;
    let mut edit = haecceity::p21::Edit::new();
    let mut raw = w.doc.get(target).unwrap().clone();
    let RawEntity::Simple {
        attributes: attrs, ..
    } = &mut raw
    else {
        panic!("target");
    };
    attrs[0] = haecceity::p21::Attribute::String("A1".into());
    edit.replace(target, raw);
    let changed = Document::parse(w.doc.apply(&edit).unwrap().bytes).unwrap();
    let rules: BTreeSet<_> = haecceity::express_rules::check_all(&changed)
        .into_iter()
        .filter(|v| v.id == target)
        .map(|v| v.rule)
        .collect();
    assert_eq!(
        rules,
        BTreeSet::from(["datum_target.WR4", "datum_target.WR5"])
    );
    // WR4 is not exempted: only the shape-description conflict WR5 is authorized.
    // The exemption cannot swallow another rule on the very same target.
    let mut raw = changed.get(target).unwrap().clone();
    let RawEntity::Simple {
        attributes: attrs, ..
    } = &mut raw
    else {
        panic!("target");
    };
    attrs[4] = haecceity::p21::Attribute::String("0".into());
    let mut edit = haecceity::p21::Edit::new();
    edit.replace(target, raw);
    let changed = Document::parse(changed.apply(&edit).unwrap().bytes).unwrap();
    assert!(
        haecceity::express_rules::check_all(&changed)
            .iter()
            .any(|v| v.id == target && v.rule == "datum_target.WR3")
    );
}

#[test]
fn decimal_only_default_and_named_cell_roundtrip() {
    let f = gap_spool();
    for p in [
        PartPmi {
            decimal_places: Some(3),
            ..PartPmi::default()
        },
        PartPmi {
            decimal_places: Some(3),
            general: vec![GeneralTolerance::Table {
                name: "precision".into(),
                cells: vec![common::pmi_targets::decimal_cell()],
            }],
            ..PartPmi::default()
        },
    ] {
        let w = written(&f, &p);
        assert_verified(&f, &w, &p);
        assert_eq!(w.read.parts[0], p);
        assert_eq!(
            w.report
                .practice_schema_conflicts
                .iter()
                .map(|v| v.rule)
                .collect::<Vec<_>>(),
            [
                "default_tolerance_table_cell.WR3",
                "default_tolerance_table_cell.WR4"
            ]
        );
        let read_f = open_bytes(w.doc.bytes().to_vec());
        let again = written(&read_f, &w.read.parts[0]);
        assert_eq!(again.read.parts[0], p);
    }
}

#[test]
fn only_authorized_target_rules_are_exempted() {
    let mut p = common::pmi_targets::every_kind();
    let t = &p.datum_targets[0];
    p.datum_targets[0] = DatumTarget::new(
        t.number(),
        t.shape().clone(),
        t.placement().cloned(),
        Some(Direction([dec("1."), dec("0."), dec("0.")])),
        t.on(),
    )
    .unwrap();
    let Err(WriteError::Refused(r)) = write(&gap_spool(), &[(PartId(0), p)], Mode::Replace) else {
        panic!("non-exempt placed-target WR3");
    };
    assert!(
        r.iter().any(|r| r.item == ItemRef::DatumTarget(0)
            && r.why.contains("placed_datum_target_feature.WR3"))
    );
}

#[test]
fn anonymous_decimal_carriers_keep_original_refusal_indices() {
    let p = PartPmi {
        decimal_places: Some(3),
        general: vec![
            GeneralTolerance::Table {
                name: "".into(),
                cells: vec![common::pmi_targets::decimal_cell()],
            },
            GeneralTolerance::Table {
                name: "invalid".into(),
                cells: vec![],
            },
        ],
        ..PartPmi::default()
    };
    let Err(WriteError::Refused(r)) = write(&spool(), &[(PartId(0), p)], Mode::Replace) else {
        panic!("invalid second table");
    };
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].item, ItemRef::General(1));
}

#[test]
fn movable_lines_and_real_datum_features_keep_their_standard_forms() {
    let mut p = common::pmi_targets::every_kind();
    let t = &p.datum_targets[1];
    p.datum_targets[1] = DatumTarget::new(
        t.number(),
        t.shape().clone(),
        t.placement().cloned(),
        Some(Direction([dec("1."), dec("0."), dec("0.")])),
        t.on(),
    )
    .unwrap();
    let targets = p.datums[0].targets().to_vec();
    p.datums[0] = Datum::new(label("A"), Some(FeatureId(0)), targets).unwrap();
    let f = gap_spool();
    let w = written(&f, &p);
    assert_verified(&f, &w, &p);
    assert_eq!(w.report.datum_symbols, [(PartId(0), "A".into())]);
    let mut invalid = common::pmi_targets::every_kind();
    invalid.datum_targets[6] =
        DatumTarget::new(7, TargetShape::Curve(FeatureId(0)), None, None, None).unwrap();
    let Err(WriteError::Refused(r)) = write(&f, &[(PartId(0), invalid)], Mode::Replace) else {
        panic!("a face is not curve geometry");
    };
    assert!(
        r.iter()
            .any(|r| r.item == ItemRef::DatumTarget(6) && r.why.contains("§6.6.2"))
    );
}

#[test]
fn occt_reads_supported_targets_without_moving_the_controlled_feature() {
    for (name, wanted) in [
        ("datum_targets", 4),
        ("datum_targets_shared_support", 4),
        ("nist_ctc_02_targets", 9),
    ] {
        let capture: Json = serde_json::from_slice(&file_bytes(
            &write_dir().join(format!("{name}.occt.json.gz")),
        ))
        .unwrap();
        assert_eq!(capture["reads"], true);
        let targets: Vec<_> = capture["parts"]
            .as_array()
            .unwrap()
            .iter()
            .chain([&capture["unattached"]])
            .flat_map(|p| p["datums"].as_array().unwrap())
            .filter(|d| d["is_target"] == true)
            .collect();
        assert_eq!(targets.len(), wanted, "{name}");
        assert!(targets.iter().all(|d| d["target"]["has_params"] == true));
        if name.starts_with("datum_targets") {
            let tol = &capture["parts"][0]["geometric_tolerances"][0];
            assert_eq!(tol["value"], 0.05);
            assert_eq!(
                tol["faces"],
                serde_json::json!([{"advanced_face":"#17","face":0,"part":0}])
            );
            for d in targets {
                if name == "datum_targets" {
                    assert_eq!(
                        d["faces"],
                        serde_json::json!([{"advanced_face":"#129","face":1,"part":0}])
                    );
                }
            }
        }
    }
}

/// PMI added (or written again) on faces a location dimension of the file is measured from
/// shares that feature's usage, as on any other face: a `dimensional_location` relating the
/// feature is not a composition of it. With a toleranced location or one with its nominal alone
/// (under the general tolerance, specify-core-rust#63) alike, no face is identified twice
/// (item_identified_representation_usage.UR1).
#[test]
fn a_location_dimensions_reference_feature_is_shared() {
    let f = spool();
    let pmi = |tolerance: DimTolerance| PartPmi {
        features: vec![faces(&[0]), faces(&[1])],
        dimensions: vec![
            size(0, "8.", tolerance.clone()),
            Dimension {
                kind: DimensionKind::Location {
                    from: FeatureId(1),
                    to: FeatureId(0),
                    kind: LocationKind::LinearDistance,
                    path: None,
                    directed: false,
                    angle: None,
                },
                nominal: Some(mm("10.")),
                tolerance,
                qualifier: None,
                modifiers: Vec::new(),
                principle: None,
            },
        ],
        ..PartPmi::default()
    };
    let other = PartPmi {
        features: vec![faces(&[0]), faces(&[1])],
        dimensions: vec![size(
            0,
            "8.",
            DimTolerance::Deviations(bounds("0.1", "-0.1")),
        )],
        tolerances: vec![tolerance(
            ToleranceKind::Flatness,
            ToleranceTarget::Feature(FeatureId(1)),
            "0.02",
        )],
        ..PartPmi::default()
    };
    for (case, first) in [
        (
            "toleranced",
            pmi(DimTolerance::Deviations(bounds("0.3", "-0.3"))),
        ),
        ("nominal only", pmi(DimTolerance::None)),
    ] {
        let w = write(&f, &[(PartId(0), first.clone())], Mode::Replace).unwrap();
        assert_reads_back(&w, PartId(0), &first);
        let g = open_bytes(w.doc.bytes().to_vec());
        for mode in [Mode::Add, Mode::Replace] {
            let w2 = write(&g, &[(PartId(0), other.clone())], mode)
                .unwrap_or_else(|e| panic!("{case} {mode:?}: {e}"));
            let v = new_violations(&g.doc, &w2.doc);
            assert!(v.is_empty(), "{case} {mode:?}: {v:#?}");
        }
    }
}

#[cfg(feature = "presentation")]
#[test]
fn every_requirement_has_a_callout() {
    use haecceity::presentation;
    let f = spool();
    let mut p = four_datums();
    p.dimensions = vec![
        size(0, "10.0", DimTolerance::Deviations(bounds("0.0762", "0.0"))),
        size(
            1,
            "20.0",
            DimTolerance::Fit {
                class: fit("H7"),
                limits: None,
            },
        ),
        size(2, "12.0", DimTolerance::Limits(bounds("12.1", "11.9"))),
        Dimension {
            kind: DimensionKind::Location {
                from: FeatureId(0),
                to: FeatureId(1),
                kind: LocationKind::LinearDistance,
                path: None,
                directed: false,
                angle: None,
            },
            nominal: Some(mm("30.0")),
            tolerance: DimTolerance::Basic,
            qualifier: None,
            modifiers: vec![],
            principle: None,
        },
    ];
    for (_, kind) in ToleranceKind::ALL {
        let mut t = tolerance(kind, ToleranceTarget::Feature(FeatureId(0)), "0.0254");
        if kind.datums() == DatumRequirement::Required {
            t.datums = Some(system(&[0, 1]));
        }
        p.tolerances.push(t);
    }
    // Also cover a semantic dimension as the tolerance's target, shared target features,
    // and the ringed material and cylindrical zone symbols.
    p.tolerances[9].target = ToleranceTarget::Dimension(DimensionId(0));
    p.tolerances[9].zone = Some(Zone {
        form: ZoneForm::WithinACylinder,
        projected: None,
        non_uniform: None,
        runout_angle: None,
        affected_plane: None,
        affected_plane_kind: None,
    });
    p.tolerances[9]
        .modifiers
        .push(ToleranceModifier::MaximumMaterialRequirement);
    p.tolerances[9].datums = Some(system(&[0, 1, 2]));
    let w = written(&f, &p);
    assert_verified(&f, &w, &p);
    assert!(new_violations(&f.doc, &w.doc).is_empty());
    let of = |name: &str| {
        w.added()
            .into_iter()
            .filter(|&id| w.names(id).iter().any(|n| express::is_a(n, name)))
            .collect::<Vec<_>>()
    };
    assert!(of("symbol_representation").is_empty());
    assert!(of("dimensional_size_with_datum_feature").is_empty());
    // A size on a datum feature must not introduce a second usage of the same definition.
    let links = of("draughting_model_item_association");
    let definitions: BTreeSet<_> = links.iter().map(|&id| refs_of(&w.doc, id)[0]).collect();
    assert_eq!(definitions.len(), links.len());
    let callouts = of("draughting_callout");
    assert_eq!(
        callouts.len(),
        p.datums.len() + p.dimensions.len() + p.tolerances.len()
    );
    assert_eq!(of("annotation_plane").len(), callouts.len());
    assert_eq!(of("draughting_model").len(), 1);
    assert_eq!(
        of("draughting_model_item_association").len(),
        callouts.len()
    );
    let expected: Vec<_> = p
        .dimensions
        .iter()
        .map(|d| {
            (
                presentation::dimension_type(d),
                presentation::dimension(d).notation(),
            )
        })
        .chain(p.tolerances.iter().map(|t| {
            (
                presentation::tolerance_type(t),
                presentation::tolerance(t, &p).notation(),
            )
        }))
        .collect();
    for (kind, text) in expected {
        let callout = *callouts.iter().find(|&&id| {
            let occurrence=refs_of(&w.doc,id)[0];
            let set=*refs_of(&w.doc,occurrence).last().unwrap();
            matches!(w.doc.get(id),Some(RawEntity::Simple { attributes,.. }) if attributes[0] == Attribute::String(text.clone())) &&
                matches!(w.doc.get(set),Some(RawEntity::Simple { attributes,.. }) if attributes[0] == Attribute::String(kind.into()))
        }).unwrap_or_else(|| panic!("missing {kind}: {text}"));
        let occurrence = refs_of(&w.doc, callout)[0];
        assert_eq!(w.names(occurrence), ["tessellated_annotation_occurrence"]);
        let occurrence_refs = refs_of(&w.doc, occurrence);
        assert_eq!(
            w.names(occurrence_refs[0]),
            ["presentation_style_assignment"]
        );
        let set = *occurrence_refs.last().unwrap();
        assert!(
            w.text(set)
                .starts_with(&format!("TESSELLATED_GEOMETRIC_SET('{kind}'"))
                || w.text(set)
                    .contains(&format!("=TESSELLATED_GEOMETRIC_SET('{kind}'"))
        );
        let curves = refs_of(&w.doc, set)[0];
        assert_eq!(w.names(curves), ["tessellated_curve_set"]);
        let coordinates = refs_of(&w.doc, curves)[0];
        // Real text and symbols, rather than one placeholder segment.
        assert!(w.text(coordinates).len() > 200);
        let links: Vec<_> = of("draughting_model_item_association")
            .into_iter()
            .filter(|&id| refs_of(&w.doc, id).last() == Some(&callout))
            .collect();
        assert_eq!(links.len(), 1, "{text}");
        let a = refs_of(&w.doc, links[0]);
        assert_eq!(w.names(a[1]), ["draughting_model"]);
        assert_eq!(a[1], of("draughting_model")[0]);
        assert_eq!(
            test_attribute(
                &w.doc,
                links[0],
                "item_identified_representation_usage",
                "name"
            ),
            Some(&Attribute::String(
                "PMI representation to presentation link".into()
            ))
        );
        assert!(
            w.names(a[0])
                .iter()
                .any(|n| express::is_a(n, "dimensional_size")
                    || express::is_a(n, "dimensional_location")
                    || express::is_a(n, "geometric_tolerance"))
        );
        let plane = of("annotation_plane")
            .into_iter()
            .find(|&id| refs_of(&w.doc, id).contains(&callout))
            .unwrap();
        assert!(refs_of(&w.doc, of("draughting_model")[0]).contains(&plane));
    }
    let before = haecceity::step::read_step(f.doc.bytes()).unwrap();
    let after = haecceity::step::read_step(w.doc.bytes()).unwrap();
    assert_eq!(before.solids.len(), after.solids.len());
    for i in 0..before.solids.len() {
        assert_eq!(before.bad_faces(i), after.bad_faces(i));
        assert_eq!(before.solid_is_valid(i), after.solid_is_valid(i));
    }
}

fn test_attribute<'d>(
    doc: &'d Document,
    id: u64,
    entity: &str,
    name: &str,
) -> Option<&'d Attribute> {
    match doc.get(id)? {
        RawEntity::Simple {
            name: kind,
            attributes,
            ..
        } => {
            let slots = express::TARGET.explicit_attributes(kind)?;
            attributes.get(
                slots
                    .iter()
                    .position(|a| a.entity == entity && a.name == name)?,
            )
        }
        RawEntity::Complex { parts, .. } => {
            let part = parts.iter().find(|p| p.name.eq_ignore_ascii_case(entity))?;
            let decl = express::TARGET.entity(entity)?;
            part.attributes
                .get(decl.attributes.iter().position(|a| a.name == name)?)
        }
    }
}

#[test]
fn an_empty_add_is_byte_identical_even_with_foreign_presentation() {
    for file in [
        "ap242/nist/nist_ctc_01_asme1_ap242-e1.stp.gz",
        "ap242/specify/spool_fits.step.gz",
        "golden_oriented_slots.step",
    ] {
        let f = fixture(file);
        let w = write(&f, &[(PartId(0), PartPmi::default())], Mode::Add).unwrap();
        assert_eq!(f.doc.bytes(), w.doc.bytes(), "{file}");
    }
}

#[cfg(feature = "presentation")]
#[test]
fn semantic_callouts_follow_replace_and_remove_without_auxiliary_representations() {
    let f = spool();
    let mut p = four_datums();
    p.dimensions.push(size(
        0,
        "20.0",
        DimTolerance::Deviations(bounds("0.1", "0.0")),
    ));
    p.tolerances.push(tolerance(
        ToleranceKind::Flatness,
        ToleranceTarget::Feature(FeatureId(0)),
        "0.02",
    ));
    let first = written(&f, &p);
    let g = open_bytes(first.doc.bytes().to_vec());
    for (mode, next) in [
        (Mode::Remove, PartPmi::default()),
        (Mode::Replace, p.clone()),
    ] {
        let after = write(&g, &[(PartId(0), next.clone())], mode).unwrap();
        assert_verified_as(&g, &after, &next, mode);
        assert!(new_violations(&g.doc, &after.doc).is_empty());
        for id in first.added() {
            if first
                .names(id)
                .iter()
                .any(|n| express::TARGET.family(n) == Some(express::Family::Presentation))
            {
                assert!(
                    after.doc.get(id).is_none(),
                    "{mode:?}: stale presentation {}",
                    first.text(id)
                );
            }
        }
    }
}

#[cfg(feature = "presentation")]
fn sample_placement(doc: &Document, bounds: haecceity::geom::Bounds, expected: usize) {
    let number = |a: &Attribute| match a {
        Attribute::Real(x) => *x,
        Attribute::Integer(x) => *x as f64,
        _ => panic!("not a coordinate: {a:?}"),
    };
    let xyz = |a: &Attribute| {
        let Attribute::List(v) = a else {
            panic!("not a point")
        };
        assert_eq!(v.len(), 3);
        [number(&v[0]), number(&v[1]), number(&v[2])]
    };
    let distance = |a: [f64; 3], b: [f64; 3]| {
        a.into_iter()
            .zip(b)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f64>()
            .sqrt()
    };
    let mut count = 0;
    let mut largest: f64 = 0.;
    for callout in doc.ids().filter(|&id| matches!(doc.get(id), Some(RawEntity::Simple {name,..}) if name.eq_ignore_ascii_case("draughting_callout"))) {
        count += 1;
        let occurrence = refs_of(doc, callout)[0];
        let set = *refs_of(doc, occurrence).last().unwrap();
        if test_attribute(doc,set,"representation_item","name") == Some(&Attribute::String("linear dimension".into())) || test_attribute(doc,set,"representation_item","name") == Some(&Attribute::String("note".into())) { continue; }
        let curves = refs_of(doc, set)[0];
        let coords = refs_of(doc, curves)[0];
        let Some(Attribute::List(points)) = test_attribute(doc, coords, "coordinates_list", "position_coords") else { panic!("coordinates") };
        let points: Vec<_> = points.iter().map(xyz).collect();
        let Some(Attribute::List(strips)) = test_attribute(doc, curves, "tessellated_curve_set", "line_strips") else { panic!("strips") };
        let Attribute::List(leader) = strips.last().unwrap() else { panic!("leader") };
        assert!(leader.len() >= 2, "a leader must have a feature foot and attachment");
        let anchor = points[number(&leader[0]) as usize - 1];
        for k in 0..3 {
            assert!(anchor[k] >= bounds.min[k]-1e-5 && anchor[k] <= bounds.max[k]+1e-5, "anchor outside part: {anchor:?}, {bounds:?}");
        }
        let plane = doc.ids().find(|&id| matches!(doc.get(id),Some(RawEntity::Simple{name,..}) if name.eq_ignore_ascii_case("annotation_plane")) && refs_of(doc,id).contains(&callout)).unwrap();
        let geometric_plane = refs_of(doc, plane)[1];
        let placement = refs_of(doc, geometric_plane)[0];
        let origin_id = refs_of(doc, placement)[0];
        let origin = xyz(test_attribute(doc,origin_id,"cartesian_point","coordinates").unwrap());
        let mut farthest = distance(anchor, origin);
        for &point in &points { farthest = farthest.max(distance(anchor,point)); }
        let ratio = farthest / bounds.max_extent();
        assert!(ratio < 0.5, "callout #{callout} extends {ratio:.3} part lengths from feature {anchor:?}; origin {origin:?}");
        largest = largest.max(ratio);
        let name = test_attribute(doc, callout,"representation_item","name").unwrap();
        println!("{name:?}: anchor={anchor:?} plane={origin:?} max={ratio:.4}x");
    }
    assert_eq!(count, expected);
    println!(
        "bbox={bounds:?}; {count} callouts; datum/diameter/frame maximum feature distance={largest:.5}x"
    );
}

#[cfg(feature = "presentation")]
#[test]
fn all_three_specify_samples_have_real_linear_dimensions_and_local_callouts() {
    for (name, dims, tols, count, with_settings) in [
        ("spindle", 9, 2, 13, 14),
        ("plate", 21, 2, 26, 27),
        ("corners-flatness", 12, 2, 17, 18),
    ] {
        println!("sample {name}");
        let f = fixture(&format!("ap242/presentation-placement/{name}.step.gz"));
        let read = pmi::read(&f.doc, &f.parts).unwrap();
        let p = &read.parts[0];
        assert_eq!(p.dimensions.len(), dims);
        assert_eq!(p.tolerances.len(), tols);
        let bounds = haecceity::step::read_step(f.doc.bytes()).unwrap().bounds();
        sample_placement(&f.doc, bounds, count);
        let w = written(&f, p);
        assert_verified(&f, &w, p);
        assert!(new_violations(&f.doc, &w.doc).is_empty());
        sample_placement(&w.doc, bounds, with_settings + p.threads.len());
        assert_notes_clear(&w.doc);
        sample_linear_dimensions(&w.doc, &w.read, &f.parts[0]);
        sample_datums_and_frames(&w.doc, &w.read, &f.parts[0]);
        assert_frames_and_diameters_clear(&w.doc);
        if let Some(dir) = std::env::var_os("HAECCEITY_PMI_SAMPLE_DIR") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(Path::new(&dir).join(format!("{name}.step")), w.doc.bytes()).unwrap();
        }
    }
}

/// Keep the measured faces, circular edges and datum references: B is a two-face
/// planar datum unused by GD&T; A is a one-face cylinder used by both controls.
/// Round 4 changes the aggregate's entity type and location endpoints, not those facts.
#[cfg(feature = "presentation")]
#[test]
fn spindle_datums_have_complete_links_and_circular_edges() {
    let f = fixture("ap242/presentation-placement/spindle.step.gz");
    let read = pmi::read(&f.doc, &f.parts).unwrap();
    let p = &read.parts[0];
    let w = written(&f, p);
    assert_eq!(datum_symbols(&w), ["A", "B"]);
    let is_a = |id, kind| w.names(id).iter().any(|n| express::is_a(n, kind));
    let mut b_planes = Vec::new();
    for (label, face_count, surface_kind, references) in
        [("A", 1, "cylindrical_surface", 2), ("B", 2, "plane", 0)]
    {
        let index = w.read.parts[0]
            .datums
            .iter()
            .position(|d| d.label().as_str() == label)
            .unwrap();
        let datum = &w.read.parts[0].datums[index];
        let Feature::Items(items) = &w.read.parts[0].features[datum.feature().unwrap().0] else {
            panic!("face datum")
        };
        assert_eq!(items.len(), face_count);
        for item in items {
            let Anchor::Face(face) = item else {
                panic!("face")
            };
            let face = f.parts[0].faces[face.0];
            let Attribute::EntityRef(surface) =
                test_attribute(&w.doc, face, "face_surface", "face_geometry").unwrap()
            else {
                panic!("surface")
            };
            assert!(is_a(*surface, surface_kind));
            if label == "B" {
                let Attribute::EntityRef(pos) =
                    test_attribute(&w.doc, *surface, "elementary_surface", "position").unwrap()
                else {
                    panic!("plane position")
                };
                let ids = refs_of(&w.doc, *pos);
                // Plane origins and normal directions (reference directions may differ).
                b_planes.push((
                    test_attribute(&w.doc, ids[0], "cartesian_point", "coordinates").unwrap(),
                    test_attribute(&w.doc, ids[1], "direction", "direction_ratios").unwrap(),
                ));
            }
            let mut visited = BTreeSet::new();
            let mut stack = vec![face];
            while let Some(id) = stack.pop() {
                if visited.insert(id) {
                    stack.extend(refs_of(&w.doc, id));
                }
            }
            assert!(
                visited.iter().any(|&id| is_a(id, "circle")),
                "{label}: face #{face} has no circular edge"
            );
        }
        let used = w.read.parts[0]
            .tolerances
            .iter()
            .filter(|t| {
                t.datums
                    .iter()
                    .flat_map(|s| s.compartments())
                    .flat_map(|c| &c.references)
                    .any(|r| r.datum.0 == index)
            })
            .count();
        assert_eq!(used, references);
    }
    assert_eq!(b_planes.len(), 2);
    assert_eq!(
        b_planes[0], b_planes[1],
        "B's member faces must be coplanar"
    );
    for text in ["3", "5", "⌀3.3 H11", "⌖ ⌀0.2 A", "⌖ ⌀0.3Ⓜ A"] {
        assert!(
            w.doc.ids().any(|id| is_a(id, "draughting_callout")
                && test_attribute(&w.doc, id, "representation_item", "name")
                    == Some(&Attribute::String(text.into()))),
            "missing full notation {text}"
        );
    }
}

fn graphic_lines(doc: &Document, callout: u64) -> Vec<Vec<haecceity::geom::V3>> {
    refs_of(doc, callout)
        .into_iter()
        .flat_map(|occurrence| occurrence_lines(doc, occurrence))
        .collect()
}
fn occurrence_lines(doc: &Document, occurrence: u64) -> Vec<Vec<haecceity::geom::V3>> {
    let set = *refs_of(doc, occurrence).last().unwrap();
    let curve = refs_of(doc, set)[0];
    let coords = refs_of(doc, curve)[0];
    let number = |a: &Attribute| match a {
        Attribute::Real(x) => *x,
        Attribute::Integer(x) => *x as f64,
        _ => panic!("coordinate {a:?}"),
    };
    let Some(Attribute::List(points)) =
        test_attribute(doc, coords, "coordinates_list", "position_coords")
    else {
        panic!("points")
    };
    let points: Vec<[f64; 3]> = points
        .iter()
        .map(|a| {
            let Attribute::List(p) = a else {
                panic!("point")
            };
            [number(&p[0]), number(&p[1]), number(&p[2])]
        })
        .collect();
    let Some(Attribute::List(lines)) =
        test_attribute(doc, curve, "tessellated_curve_set", "line_strips")
    else {
        panic!("lines")
    };
    lines
        .iter()
        .map(|a| {
            let Attribute::List(ids) = a else {
                panic!("strip")
            };
            ids.iter().map(|i| points[number(i) as usize - 1]).collect()
        })
        .collect()
}
fn annotation_basis(doc: &Document, callout: u64) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let plane=doc.ids().find(|&id|matches!(doc.get(id),Some(RawEntity::Simple{name,..}) if name.eq_ignore_ascii_case("annotation_plane")) && refs_of(doc,id).contains(&callout)).unwrap();
    let geometric = refs_of(doc, plane)[1];
    let pos = refs_of(doc, geometric)[0];
    let ids = refs_of(doc, pos);
    let xyz = |id, kind, field| {
        let Some(Attribute::List(a)) = test_attribute(doc, id, kind, field) else {
            panic!("vector")
        };
        let n = |i| match a[i] {
            Attribute::Real(x) => x,
            Attribute::Integer(x) => x as f64,
            _ => panic!("number"),
        };
        [n(0), n(1), n(2)]
    };
    (
        xyz(ids[0], "cartesian_point", "coordinates"),
        xyz(ids[1], "direction", "direction_ratios"),
        xyz(ids[2], "direction", "direction_ratios"),
    )
}
fn sample_datums_and_frames(doc: &Document, read: &PmiRead, def: &PartDefinition) {
    use haecceity::geom;
    let part = haecceity::step::read_part(doc.bytes(), def).unwrap();
    let p = &read.parts[0];
    let largest = part.bounds().max_extent();
    let is = |id, kind| match doc.get(id).unwrap() {
        RawEntity::Simple { name, .. } => express::is_a(name, kind),
        RawEntity::Complex { parts, .. } => parts.iter().any(|p| express::is_a(&p.name, kind)),
    };
    fn faces(p: &PartPmi, f: FeatureId, depth: usize, out: &mut Vec<usize>) {
        assert!(depth <= p.features.len());
        match &p.features[f.0] {
            Feature::Items(items) => out.extend(items.iter().filter_map(|a| {
                if let Anchor::Face(f) = a {
                    Some(f.0)
                } else {
                    None
                }
            })),
            Feature::Group { members, .. } | Feature::Derived { from: members, .. } => {
                for f in members {
                    faces(p, *f, depth + 1, out);
                }
            }
        }
    }
    for (i, d) in p.datums.iter().enumerate() {
        let name = format!("Datum {}", d.label());
        let callout = doc
            .ids()
            .find(|&id| {
                is(id, "draughting_callout")
                    && test_attribute(doc, id, "representation_item", "name")
                        == Some(&Attribute::String(name.clone()))
            })
            .unwrap();
        let lines = graphic_lines(doc, callout);
        let leader = lines.last().unwrap();
        let foot = leader[0];
        let (_, plane_normal, _) = annotation_basis(doc, callout);
        let mut fs = Vec::new();
        faces(p, d.feature().unwrap(), 0, &mut fs);
        let face = fs
            .iter()
            .copied()
            .find(|&f| {
                let surface = &part.faces[f].surface;
                surface.parameters(foot, None).is_some_and(|(u, v)| {
                    let (u0, u1, v0, v1) = part.uv_bounds(f).unwrap();
                    let du = (u1 - u0).abs().max(1e-6) * 1e-5;
                    let dv = (v1 - v0).abs().max(1e-6) * 1e-5;
                    geom::dist(surface.value(u, v), foot) < 2e-6
                        && [(u, v), (u - du, v), (u + du, v), (u, v - dv), (u, v + dv)]
                            .iter()
                            .all(|&(u, v)| part.domain(f).unwrap().contains(u, v))
                })
            })
            .unwrap_or_else(|| {
                panic!("datum {i} {name} foot {foot:?} is not interior to any of its faces {fs:?}")
            });
        let surface = &part.faces[face].surface;
        let (u, v) = surface.parameters(foot, None).unwrap();
        let n = geom::scale(
            surface.normal(u, v).unwrap(),
            if part.faces[face].reversed { -1. } else { 1. },
        );
        assert!(
            geom::dot(n, plane_normal).abs() < 1e-8,
            "{name}: annotation plane must contain the face normal"
        );
        let stem = geom::unit(geom::sub(leader[1], foot)).unwrap();
        assert!(
            geom::dot(stem, n) > 1. - 1e-8,
            "{name}: leader must leave along the outward face normal"
        );
        for lp in &part.faces[face].loops {
            for &(edge, _) in &lp.edges {
                let e = &part.edges[edge];
                assert!(
                    geom::dist(foot, e.curve.value(e.curve.parameter(foot))) > 1e-5,
                    "{name}: foot on a boundary curve"
                );
            }
        }
        println!(
            "{name}: interior face {face}, anchor={foot:?}, face normal={n:?}, annotation normal={plane_normal:?}"
        );
    }
    for callout in doc.ids().filter(|&id| is(id, "draughting_callout")) {
        let occurrence = refs_of(doc, callout)[0];
        let set = *refs_of(doc, occurrence).last().unwrap();
        let kind = test_attribute(doc, set, "representation_item", "name").unwrap();
        if kind == &Attribute::String("datum".into())
            || kind == &Attribute::String("linear dimension".into())
            || kind == &Attribute::String("diameter dimension".into())
            || kind == &Attribute::String("note".into())
            || kind == &Attribute::String("surface roughness".into())
            || kind == &Attribute::String("thread".into())
        {
            continue;
        }
        let lines = graphic_lines(doc, callout);
        let leader = lines.last().unwrap();
        let reach = leader
            .iter()
            .map(|q| geom::dist(*q, leader[0]))
            .fold(0., f64::max);
        let association = doc
            .referrers(callout)
            .iter()
            .copied()
            .find(|&id| is(id, "draughting_model_item_association"))
            .unwrap();
        let root = refs_of(doc, association)[0];
        let index = read.provenance.parts[0]
            .tolerances
            .iter()
            .position(|ids| ids.contains(&root))
            .unwrap();
        let mut fs = Vec::new();
        match p.tolerances[index].target {
            ToleranceTarget::Feature(f) => faces(p, f, 0, &mut fs),
            ToleranceTarget::Dimension(d) => match p.dimensions[d.0].kind {
                DimensionKind::Size { feature, .. } => faces(p, feature, 0, &mut fs),
                DimensionKind::Location { from, to, .. } => {
                    faces(p, from, 0, &mut fs);
                    faces(p, to, 0, &mut fs);
                }
            },
            ToleranceTarget::Relation {
                relating, related, ..
            } => {
                faces(p, relating, 0, &mut fs);
                faces(p, related, 0, &mut fs);
            }
            ToleranceTarget::WholePart => fs.extend(0..part.faces.len()),
        }
        let mut feature_bounds = geom::Bounds::empty();
        for f in fs {
            feature_bounds.merge(&part.face_bounds(f));
        }
        let extent = feature_bounds.max_extent();
        assert!(
            reach <= 0.25 * extent,
            "frame #{callout}: leader {reach} exceeds 0.25 * feature extent {extent}"
        );
        println!(
            "frame #{callout} leader={reach:.6}, feature extent={extent:.6} ({:.6} part lengths)",
            reach / largest
        );
    }
}
#[cfg(feature = "presentation")]
fn measured_foot(
    part: &haecceity::Part,
    def: &PartDefinition,
    p: &PartPmi,
    f: FeatureId,
    q: [f64; 3],
) -> bool {
    use haecceity::geom::{self, Surface};
    match &p.features[f.0] {
        Feature::Items(items) => items.iter().any(|a| match a {
            Anchor::Face(face) => match part.faces[face.0].surface {
                Surface::Plane { frame } => {
                    let length = part.bounds().max_extent();
                    let caster = haecceity::rays::RayCaster::for_faces(part, vec![face.0]);
                    let origin = geom::sub(q, geom::scale(frame.z, length));
                    caster
                        .hits_on(&[face.0], origin, frame.z, 2. * length)
                        .is_some_and(|hits| {
                            hits.iter().any(|hit| {
                                geom::dist(q, geom::add(origin, geom::scale(frame.z, hit.t)))
                                    < 1e-7 * length
                            })
                        })
                }
                Surface::Cylinder { frame, .. }
                | Surface::Cone { frame, .. }
                | Surface::Torus { frame, .. } => {
                    let b = part.face_bounds(face.0);
                    let t = geom::dot(geom::sub(q, frame.origin), frame.z);
                    let axial: Vec<_> = [b.min[0], b.max[0]]
                        .into_iter()
                        .flat_map(|x| {
                            [b.min[1], b.max[1]].into_iter().flat_map(move |y| {
                                [b.min[2], b.max[2]].into_iter().map(move |z| {
                                    geom::dot(geom::sub([x, y, z], frame.origin), frame.z)
                                })
                            })
                        })
                        .collect();
                    let lo = axial.iter().copied().fold(f64::INFINITY, f64::min);
                    let hi = axial.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                    geom::norm(geom::cross(geom::sub(q, frame.origin), frame.z)) < 1e-7
                        && ((t - lo).abs() < 1e-7 || (t - hi).abs() < 1e-7)
                }
                _ => false,
            },
            Anchor::Edge(e) => {
                let entity = def.edges[e.0];
                let i = (0..part.edges.len())
                    .find(|&i| part.edge_source(i).is_some_and(|s| s.entity == entity))
                    .unwrap();
                let edge = &part.edges[i];
                // Witnesses are endpoints or the trimmed midpoint, not a point anywhere
                // on the infinite supporting curve. Check both independently.
                geom::dist(edge.curve.value(edge.curve.parameter(q)), q) < 1e-7
                    && [
                        edge.start,
                        edge.curve.value(edge.curve.parameter(edge.midpoint())),
                        edge.end,
                    ]
                    .iter()
                    .any(|p| geom::dist(*p, q) < 1e-7)
            }
            Anchor::Geometry(_) => false,
        }),
        Feature::Group { members, .. } | Feature::Derived { from: members, .. } => {
            members.iter().any(|f| measured_foot(part, def, p, *f, q))
        }
    }
}
#[cfg(feature = "presentation")]
fn sample_linear_dimensions(doc: &Document, read: &PmiRead, def: &PartDefinition) {
    use haecceity::{
        geom::{self, Bounds},
        presentation::linear,
    };
    let part = haecceity::step::read_part(doc.bytes(), def).unwrap();
    let bounds = part.bounds();
    let length = bounds.max_extent();
    let gap = (0.5 + 0.018 * length) / 3.;
    let p = &read.parts[0];
    let fixed:Vec<_>=doc.ids().filter_map(|id| {
        if !matches!(doc.get(id),Some(RawEntity::Simple{name,..})if name.eq_ignore_ascii_case("draughting_callout")){return None;}
        let occurrence=refs_of(doc,id)[0];let set=*refs_of(doc,occurrence).last().unwrap();
        if test_attribute(doc,set,"representation_item","name")==Some(&Attribute::String("linear dimension".into())) {return None;}
        let lines=graphic_lines(doc,id);let mut b=Bounds::empty();for p in lines.iter().take(lines.len()-1).flatten(){b.add(*p);}
        Some((id,b,annotation_basis(doc,id).1))
    }).collect();
    let mut labels = Vec::new();
    let mut projected = Vec::new();
    let mut ladder = Vec::new();
    let mut linear_count = 0;
    for (i, d) in p.dimensions.iter().enumerate() {
        let root = *read.provenance.parts[0].dimensions[i]
            .iter()
            .find(|&&id| match doc.get(id) {
                Some(RawEntity::Simple { name, .. }) => {
                    express::is_a(name, "dimensional_size")
                        || express::is_a(name, "dimensional_location")
                }
                Some(RawEntity::Complex { parts, .. }) => parts.iter().any(|p| {
                    express::is_a(&p.name, "dimensional_size")
                        || express::is_a(&p.name, "dimensional_location")
                }),
                _ => false,
            })
            .unwrap();
        let association=doc.ids().find(|&id|matches!(doc.get(id),Some(RawEntity::Simple{name,..}) if name.eq_ignore_ascii_case("draughting_model_item_association")) && refs_of(doc,id)[0]==root).unwrap();
        let callout = *refs_of(doc, association).last().unwrap();
        let lines = graphic_lines(doc, callout);
        let (origin, n, u) = annotation_basis(doc, callout);
        let v = geom::unit(geom::sub(
            *lines[0].last().unwrap(),
            lines[0][lines[0].len() - 2],
        ))
        .unwrap();
        let mut text = Bounds::empty();
        if !linear::is_linear(d) {
            for &q in lines.iter().take(lines.len() - 1).flatten() {
                text.add(q);
            }
            let up = geom::cross(n, u);
            let centre = text.centre();
            let extent = |axis| {
                (0..8)
                    .map(|mask| {
                        geom::dot(
                            std::array::from_fn(|k| {
                                if mask & (1 << k) == 0 {
                                    text.min[k]
                                } else {
                                    text.max[k]
                                }
                            }),
                            axis,
                        )
                    })
                    .fold([f64::INFINITY, f64::NEG_INFINITY], |r, t| {
                        [r[0].min(t), r[1].max(t)]
                    })
            };
            let x = extent(u);
            let y = extent(up);
            let polygon = [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)]
                .map(|(a, b)| {
                    geom::add(
                        centre,
                        geom::add(
                            geom::scale(u, a * (x[1] - x[0]) / 2.),
                            geom::scale(up, b * (y[1] - y[0]) / 2.),
                        ),
                    )
                })
                .to_vec();
            projected.push((callout, n, [polygon.clone(), polygon]));
            labels.push((callout, text, n));
            continue;
        }
        linear_count += 1;
        assert!(
            lines.len() > 10,
            "dimension must have extensions, split line, two triangular arrows and text"
        );
        let feet = [
            geom::sub(lines[0][0], geom::scale(v, gap)),
            geom::sub(lines[1][0], geom::scale(v, gap)),
        ];
        let ends = [lines[2][0], *lines[3].last().unwrap()];
        for (other, b, normal) in &fixed {
            if geom::dot(n, *normal).abs() > 1. - 1e-8 {
                let axes: Vec<_> = (0..3).filter(|&k| n[k].abs() < 0.5).collect();
                if axes.len() == 2 {
                    assert!(
                        !axes
                            .iter()
                            .all(|&k| ends[0][k].min(ends[1][k]) < b.max[k] - 1e-8
                                && ends[0][k].max(ends[1][k]) > b.min[k] + 1e-8),
                        "dimension #{callout} crosses retained annotation #{other}"
                    );
                }
            }
        }
        let span = geom::sub(ends[1], ends[0]);
        let nominal = d.nominal.as_ref().unwrap().si().abs();
        assert!(
            (geom::norm(span) - nominal).abs() <= 1e-6 * nominal,
            "D{i}: span {:?} nominal {nominal}",
            ends
        );
        assert!(geom::norm(geom::cross(span, u)) < 1e-8 * length);
        let (from, to) = match d.kind {
            DimensionKind::Location { from, to, .. } => (from, to),
            DimensionKind::Size { feature, .. } => (feature, feature),
        };
        assert!(
            measured_foot(&part, def, p, from, feet[0]),
            "D{i} first foot {:?} misses {from:?}",
            feet[0]
        );
        assert!(
            measured_foot(&part, def, p, to, feet[1]),
            "D{i} second foot {:?} misses {to:?}",
            feet[1]
        );
        // A plane fixes the measured direction independently of the renderer's chosen basis.
        if let Feature::Items(items) = &p.features[from.0] {
            for a in items {
                if let Anchor::Face(face) = a
                    && let geom::Surface::Plane { frame } = part.faces[face.0].surface
                {
                    assert!(geom::norm(geom::cross(frame.z, span)) < 1e-8 * length);
                }
            }
        }
        for (j, foot) in feet.iter().enumerate() {
            assert!(
                geom::norm(geom::cross(
                    geom::sub(*lines[j].last().unwrap(), lines[j][lines[j].len() - 2]),
                    v
                )) < 1e-8 * length
            );
            assert!(
                (geom::dot(*foot, u) - geom::dot(ends[j], u)).abs() < 1e-7 * length
                    || (geom::dot(*foot, u) - geom::dot(ends[1 - j], u)).abs() < 1e-7 * length
            );
            assert!(
                geom::dot(*lines[j].last().unwrap(), v) > geom::dot(ends[0], v),
                "extension overshoots dimension line"
            );
        }
        for q in lines.iter().skip(10).flatten() {
            text.add(*q);
        }
        let centre = text.centre();
        assert!(
            geom::dist(centre, origin) < 1e-7 * length,
            "text centred at annotation origin"
        );
        assert!(
            geom::norm(geom::cross(geom::sub(centre, ends[0]), u)) < 1e-7 * length,
            "text is on dimension line"
        );
        let station = geom::dot(geom::sub(centre, ends[0]), u);
        assert!(
            station > 0. && station < geom::norm(span),
            "text between arrows"
        );
        for head in [&lines[4..7], &lines[7..10]] {
            assert_eq!(head[0][0], head[2][1]);
            assert_eq!(head[0][1], head[1][0]);
            assert_eq!(head[1][1], head[2][0]);
        }
        let edge = (0..8)
            .map(|mask| {
                geom::dot(
                    std::array::from_fn(|k| {
                        if mask & (1 << k) == 0 {
                            bounds.min[k]
                        } else {
                            bounds.max[k]
                        }
                    }),
                    v,
                )
            })
            .fold(f64::NEG_INFINITY, f64::max);
        let offset = geom::dot(ends[0], v) - edge;
        assert!(offset > 0.);
        ladder.push((nominal, i, u, v, offset, geom::dot(origin, n)));
        let rect = |width: f64, height: f64| {
            [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)]
                .map(|(x, y)| {
                    geom::add(
                        origin,
                        geom::add(
                            geom::scale(u, x * width / 2.),
                            geom::scale(v, y * height / 2.),
                        ),
                    )
                })
                .to_vec()
        };
        let ink: Vec<_> = lines.iter().skip(10).flatten().copied().collect();
        let extent = |axis| {
            let stations: Vec<_> = ink
                .iter()
                .map(|q| geom::dot(geom::sub(*q, origin), axis))
                .collect();
            stations.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                - stations.iter().copied().fold(f64::INFINITY, f64::min)
        };
        projected.push((
            callout,
            n,
            [rect(extent(u), extent(v)), rect(nominal, gap * 0.3)],
        ));
        labels.push((callout, text, n));
        println!(
            "linear D{i} {}: feet={feet:?} dimension={ends:?} text={centre:?} offset={offset:.6} gap={gap:.6} plane_normal={n:?}",
            haecceity::presentation::dimension_text(d)
        );
    }
    assert_eq!(
        linear_count,
        p.dimensions.iter().filter(|d| linear::is_linear(d)).count()
    );
    // Actual serialized ink and dimension spans are checked in all seven distinct
    // principal/isometric views, including different plane groups. An edge-on plane
    // has zero-area text and is not a legible view (a planar ladder necessarily collapses).
    let mut worst = (f64::INFINITY, 0, 0, [0.; 3]);
    for view in linear::VIEWS {
        for (i, (a, n, polygons)) in projected.iter().enumerate() {
            for (b, m, others) in &projected[..i] {
                if geom::dot(*n, view).abs() < 1e-8 || geom::dot(*m, view).abs() < 1e-8 {
                    continue;
                }
                for x in polygons {
                    for y in others {
                        let clearance = linear::projected_clearance(x, y, view);
                        assert!(
                            clearance > 1e-8,
                            "projected dimension text/line overlap #{a}, #{b}, view {view:?}, clearance {clearance}"
                        );
                        if clearance < worst.0 {
                            worst = (clearance, *a, *b, view);
                        }
                    }
                }
            }
        }
    }
    // Other dimension notation still has its explicit face-on projection check.
    for (i, (a, x, n)) in labels.iter().enumerate() {
        for (b, y, m) in &labels[..i] {
            if geom::dot(*n, *m).abs() > 1. - 1e-8 {
                let axes: Vec<_> = (0..3).filter(|&k| n[k].abs() < 0.5).collect();
                if axes.len() == 2 {
                    assert!(
                        !axes
                            .iter()
                            .all(|&k| x.min[k] < y.max[k] - 1e-8 && x.max[k] > y.min[k] + 1e-8),
                        "projected text boxes overlap #{a}, #{b}"
                    );
                }
            }
        }
    }
    println!(
        "principal/isometric worst projected text/line clearance={:.9} mm, callouts #{} / #{}, view {:?}; zero overlaps in readable views",
        worst.0, worst.1, worst.2, worst.3
    );
    ladder.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    for (i, a) in ladder.iter().enumerate() {
        for b in &ladder[..i] {
            if geom::dot(a.2, b.2) > 1. - 1e-8 && geom::dot(a.3, b.3) > 1. - 1e-8 {
                assert!(
                    (a.5 - b.5).abs() < 1e-8,
                    "parallel dimensions on the same side must share a plane: {a:?}, {b:?}"
                );
                assert!(
                    a.4 > b.4 + 1e-8,
                    "shorter dimension must be nearer part: {a:?}, {b:?}"
                );
            }
        }
    }
    println!(
        "{linear_count} real linear dimensions; {} dimension text boxes disjoint",
        labels.len()
    );
}

#[cfg(feature = "presentation")]
#[test]
fn linear_sizes_use_opposed_faces_and_axis_locations_use_both_centrelines() {
    let f = fixture("ap242/presentation-placement/plate.step.gz");
    let mut p = pmi::read(&f.doc, &f.parts).unwrap().parts.remove(0);
    let width = p.features.len();
    p.features.push(faces(&[2, 4]));
    let mut thickness = size(
        width,
        "10.",
        DimTolerance::Deviations(bounds("0.1", "-0.1")),
    );
    if let DimensionKind::Size { kind, .. } = &mut thickness.kind {
        *kind = SizeKind::Thickness;
    }
    let between_axes = Dimension {
        kind: DimensionKind::Location {
            from: FeatureId(5),
            to: FeatureId(6),
            kind: LocationKind::LinearDistance,
            path: None,
            directed: false,
            angle: None,
        },
        nominal: Some(mm("30.")),
        tolerance: DimTolerance::Basic,
        qualifier: None,
        modifiers: vec![],
        principle: None,
    };
    p.dimensions = vec![thickness, between_axes];
    let w = written(&f, &p);
    assert_verified(&f, &w, &p);
    assert!(new_violations(&f.doc, &w.doc).is_empty());
    sample_linear_dimensions(&w.doc, &w.read, &f.parts[0]);
}

#[cfg(feature = "presentation")]
#[test]
fn nist_edge_witnesses_follow_semantic_entities_not_loop_order() {
    use haecceity::{
        geom,
        presentation::{self, linear},
    };
    let f = fixture("ap242/specify/nist_ctc_01_merge.step.gz");
    let mut edge_feet = 0;
    let mut reordered = 0;
    for def in &f.parts {
        let part = haecceity::step::read_part(f.doc.bytes(), def).unwrap();
        reordered += def
            .edges
            .iter()
            .enumerate()
            .filter(|(i, id)| part.edge_source(*i).is_none_or(|s| s.entity != **id))
            .count();
        // The merged fixture has no direct edge-to-edge linear requirements. Exercise
        // every semantic edge as an extension target on its real geometry, including
        // all 70 whose B-rep order differs; choose an independent second edge.
        let midpoint = |id: u64| {
            let i = (0..part.edges.len())
                .find(|&i| part.edge_source(i).is_some_and(|s| s.entity == id))
                .unwrap();
            part.edges[i]
                .curve
                .value(part.edges[i].curve.parameter(part.edges[i].midpoint()))
        };
        let mut p = PartPmi {
            features: (0..def.edges.len())
                .map(|i| Feature::Items(vec![Anchor::Edge(EdgeIndex(i))]))
                .collect(),
            ..PartPmi::default()
        };
        for (i, &id) in def.edges.iter().enumerate() {
            let a = midpoint(id);
            let j = def
                .edges
                .iter()
                .position(|&id| geom::dist(a, midpoint(id)) > 1e-6)
                .unwrap();
            p.dimensions.push(Dimension {
                kind: DimensionKind::Location {
                    from: FeatureId(i),
                    to: FeatureId(j),
                    kind: LocationKind::LinearDistance,
                    path: None,
                    directed: false,
                    angle: None,
                },
                nominal: Some(mm(&geom::dist(a, midpoint(def.edges[j])).to_string())),
                tolerance: DimTolerance::None,
                qualifier: None,
                modifiers: vec![],
                principle: None,
            });
        }
        let measurements = linear::measurements(&part, def, &p);
        for (d, m) in p.dimensions.iter().zip(measurements) {
            let m = m.expect("every edge-anchored dimension must resolve");
            let features = match d.kind {
                DimensionKind::Location { from, to, .. } => [from, to],
                DimensionKind::Size { feature, .. } => [feature; 2],
            };
            let drawn = linear::Ladder::new(part.bounds()).draw(&m, &presentation::dimension(d));
            for (i, feature) in features.into_iter().enumerate() {
                if matches!(&p.features[feature.0], Feature::Items(items) if items.iter().any(|a| matches!(a,Anchor::Edge(_))))
                {
                    let line = &drawn.placed.lines[i];
                    let v =
                        geom::unit(geom::sub(*line.last().unwrap(), line[line.len() - 2])).unwrap();
                    let foot = geom::sub(line[0], geom::scale(v, drawn.gap));
                    assert!(
                        measured_foot(&part, def, &p, feature, foot),
                        "semantic edge missed: {feature:?} {foot:?}"
                    );
                    edge_feet += 1;
                }
            }
        }
    }
    assert_eq!(
        reordered, 70,
        "the fixture must exercise reversed loop edge order"
    );
    assert_eq!(
        edge_feet, 636,
        "both extension feet on all 318 semantic edges"
    );
    println!("{edge_feet} semantic edge extension feet checked; {reordered} reordered edges");
}

#[cfg(feature = "presentation")]
#[test]
fn read_back_deviations_use_notation_numbers() {
    let f = spool();
    let mut p = PartPmi {
        features: vec![faces(&[0])],
        ..PartPmi::default()
    };
    for (upper, lower) in [("1.", "-1."), ("0.5", "-1."), ("1.E-2", "0.")] {
        p.dimensions.push(size(
            0,
            "10.",
            DimTolerance::Deviations(bounds(upper, lower)),
        ));
    }
    let w = written(&f, &p);
    for (d, expected) in
        w.read.parts[0]
            .dimensions
            .iter()
            .zip(["⌀10 ±1.0", "⌀10 +0.5/-1.0", "⌀10 +1E-2/0"])
    {
        assert_eq!(haecceity::presentation::dimension(d).notation(), expected);
        assert!(
            w.added()
                .into_iter()
                .any(|id| w.names(id) == ["draughting_callout"]
                    && test_attribute(&w.doc, id, "representation_item", "name")
                        == Some(&Attribute::String(expected.into())))
        );
    }
}

#[cfg(feature = "presentation")]
#[test]
fn scope_symbols_survive_the_written_entity_graph() {
    use haecceity::{geom, presentation};
    let f = gap_spool();
    for (target, auxiliary, rings) in [
        (ToleranceTarget::Feature(FeatureId(1)), vec![], 1),
        (
            ToleranceTarget::Feature(FeatureId(0)),
            vec![AuxiliaryClassification::AllOver],
            2,
        ),
        (ToleranceTarget::WholePart, vec![], 2),
    ] {
        let mut p = PartPmi {
            features: vec![
                faces(&[0]),
                Feature::Group {
                    kind: GroupKind::AllAround,
                    members: vec![FeatureId(0)],
                },
            ],
            ..PartPmi::default()
        };
        let mut t = tolerance(ToleranceKind::SurfaceProfile, target, "0.1");
        t.auxiliary = auxiliary;
        p.tolerances.push(t);
        let w = written(&f, &p);
        assert_verified(&f, &w, &p);
        assert!(new_violations(&f.doc, &w.doc).is_empty());
        let callout = w
            .added()
            .into_iter()
            .find(|&id| w.names(id) == ["draughting_callout"])
            .unwrap();
        let lines = graphic_lines(&w.doc, callout);
        let local = presentation::tolerance(&p.tolerances[0], &p);
        assert_eq!(lines.len(), local.polylines().len() + rings + 1);
        let leader = lines.last().unwrap();
        assert_eq!(leader.len(), 3);
        for ring in &lines[lines.len() - 1 - rings..lines.len() - 1] {
            assert_eq!(ring.len(), 33);
            assert_eq!(ring.first(), ring.last());
            let centre = geom::scale(geom::add(ring[0], ring[16]), 0.5);
            assert!(
                geom::dist(centre, leader[1]) < 2e-6,
                "ring centred on leader bend"
            );
        }
    }
}

/// SFA §6.3–6.5 counts geometric components, not the datum-establishment relation.
fn assert_composite_components(doc: &Document) {
    for id in doc.ids() {
        let composite = match doc.get(id) {
            Some(RawEntity::Simple { name, .. }) => express::is_a(name, "composite_shape_aspect"),
            Some(RawEntity::Complex { parts, .. }) => parts
                .iter()
                .any(|p| express::is_a(&p.name, "composite_shape_aspect")),
            _ => false,
        };
        if !composite {
            continue;
        }
        let components: BTreeSet<_> = doc.referrers(id).iter().copied().filter(|&r| {
            matches!(doc.get(r), Some(RawEntity::Simple { name, .. }) if name.eq_ignore_ascii_case("shape_aspect_relationship"))
                && test_attribute(doc, r, "shape_aspect_relationship", "relating_shape_aspect") == Some(&Attribute::EntityRef(id))
        }).filter_map(|r| {
            let Some(Attribute::EntityRef(member)) = test_attribute(doc, r, "shape_aspect_relationship", "related_shape_aspect") else { return None; };
            let datum = match doc.get(*member) {
                Some(RawEntity::Simple { name, .. }) => express::is_a(name, "datum"),
                Some(RawEntity::Complex { parts, .. }) => parts.iter().any(|p| express::is_a(&p.name, "datum")),
                _ => false,
            };
            (!datum).then_some(*member)
        }).collect();
        assert!(
            components.len() >= 2,
            "composite #{id} needs at least two geometric components, got {components:?}"
        );
    }
}

#[test]
fn all_written_goldens_have_at_least_two_components_per_composite() {
    // Current writer output is checked by written_files_are_the_writers_current_output;
    // reuse that export pass, rather than write every case a second time for this guard.
    for entry in std::fs::read_dir(write_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.to_string_lossy().ends_with(".step.gz") {
            assert_composite_components(&Document::parse(file_bytes(&path)).unwrap());
        }
    }
}

/// The measured semantic rules apply with and without graphical dimension writing.
#[test]
fn composite_features_and_locations_reuse_their_geometry_carriers() {
    for (name, count) in [("spindle", 2), ("plate", 3), ("corners-flatness", 3)] {
        let f = fixture(&format!("ap242/presentation-placement/{name}.step.gz"));
        let old = pmi::read(&f.doc, &f.parts).unwrap();
        let p = &old.parts[0];
        let w = written(&f, p);
        assert_verified(&f, &w, p);
        assert!(new_violations(&f.doc, &w.doc).is_empty());
        let is = |id, kind| w.names(id).iter().any(|n| express::is_a(n, kind));
        fn geometry(doc: &Document, id: u64, depth: usize, faces: &mut BTreeSet<u64>) {
            assert!(depth < 100, "cyclic composition");
            for &r in doc.referrers(id) {
                if test_attribute(doc, r, "item_identified_representation_usage", "definition")
                    == Some(&Attribute::EntityRef(id))
                    && matches!(doc.get(r),Some(RawEntity::Simple{name,..})if name.eq_ignore_ascii_case("geometric_item_specific_usage"))
                    && let Some(Attribute::EntityRef(face)) = test_attribute(
                        doc,
                        r,
                        "item_identified_representation_usage",
                        "identified_item",
                    )
                {
                    faces.insert(*face);
                }
                if matches!(doc.get(r),Some(RawEntity::Simple{name,..})if name.eq_ignore_ascii_case("shape_aspect_relationship"))
                    && test_attribute(doc, r, "shape_aspect_relationship", "relating_shape_aspect")
                        == Some(&Attribute::EntityRef(id))
                {
                    let Some(Attribute::EntityRef(m)) =
                        test_attribute(doc, r, "shape_aspect_relationship", "related_shape_aspect")
                    else {
                        panic!("component")
                    };
                    geometry(doc, *m, depth + 1, faces);
                }
            }
        }
        let mut composites = 0;
        let mut locations = 0;
        for id in w.added() {
            if is(id, "shape_aspect") {
                let mut face_set = BTreeSet::new();
                geometry(&w.doc, id, 0, &mut face_set);
                face_set.retain(|id| f.parts[0].faces.contains(id));
                if face_set.len() == 1 {
                    assert!(
                        !is(id, "composite_shape_aspect"),
                        "a single face cannot be composite: {}",
                        w.text(id)
                    );
                }
                if is(id, "datum_feature") && face_set.len() == 1 {
                    assert!(
                        !is(id, "composite_shape_aspect"),
                        "single-face datum must be plain: {}",
                        w.text(id)
                    );
                    assert!(
                        w.doc.referrers(id).iter().any(|&r| {
                            w.names(r) == ["geometric_item_specific_usage"]
                                && test_attribute(
                                    &w.doc,
                                    r,
                                    "item_identified_representation_usage",
                                    "definition",
                                ) == Some(&Attribute::EntityRef(id))
                        }),
                        "single-face datum needs its own face GISU: {}",
                        w.text(id)
                    );
                }
                if face_set.len() > 1 {
                    assert!(
                        is(id, "composite_shape_aspect"),
                        "{name}: multi-face {} must be composite",
                        w.text(id)
                    );
                    assert!(w.doc.referrers(id).iter().any(|&r| test_attribute(
                        &w.doc,
                        r,
                        "shape_aspect_relationship",
                        "relating_shape_aspect"
                    ) == Some(&Attribute::EntityRef(
                        id
                    ))));
                    composites += 1;
                }
            }
            if is(id, "dimensional_location") {
                locations += 1;
                for field in ["relating_shape_aspect", "related_shape_aspect"] {
                    let Some(Attribute::EntityRef(a)) =
                        test_attribute(&w.doc, id, "shape_aspect_relationship", field)
                    else {
                        panic!("location endpoint")
                    };
                    assert!(is(*a, "shape_aspect"));
                    let endpoint_geometry = common::pmi_usage::geometry(&w.doc, *a);
                    for datum in w.doc.ids().filter(|d| {
                        is(*d, "datum_feature") && !is(*d, "composite_group_shape_aspect")
                    }) {
                        if !endpoint_geometry.is_empty()
                            && (common::pmi_usage::neutral(&w.doc, *a) || is(*a, "datum_feature"))
                            && common::pmi_usage::geometry(&w.doc, datum) == endpoint_geometry
                        {
                            assert_eq!(
                                *a, datum,
                                "{name}: datum-face endpoint must be the datum feature"
                            );
                        }
                    }
                    assert_ne!(
                        test_attribute(&w.doc, *a, "shape_aspect", "name"),
                        Some(&Attribute::String("DIMENSION".into()))
                    );
                }
            }
        }
        assert!(
            composites > 0 && locations > 0,
            "sample must exercise both multi-face aspects and location endpoints"
        );
        assert_composite_components(&w.doc);
        assert_eq!(w.read.parts[0].datums.len(), count);
        sample_datums_and_frames(&w.doc, &w.read, &f.parts[0]);
        #[cfg(feature = "presentation")]
        assert_frames_and_diameters_clear(&w.doc);
    }
}

#[test]
fn a_multi_face_area_target_is_composite_with_per_face_usages() {
    let f = gap_spool();
    let p = PartPmi {
        features: vec![faces(&[0, 1])],
        datum_targets: vec![
            DatumTarget::new(1, TargetShape::Area(FeatureId(0)), None, None, None).unwrap(),
        ],
        datums: vec![Datum::new(label("A"), None, vec![DatumTargetId(0)]).unwrap()],
        ..PartPmi::default()
    };
    let w = written(&f, &p);
    assert_verified(&f, &w, &p);
    // The existing CAx-IF datum_target.WR5 conflict is reported for any area
    // target; combining it with CSA must add no further schema/rule finding.
    let findings = new_violations(&f.doc, &w.doc);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("datum_target.WR5"), "{findings:?}");
    let target = w
        .added()
        .into_iter()
        .find(|&id| w.names(id).iter().any(|n| express::is_a(n, "datum_target")))
        .unwrap();
    assert!(
        w.names(target)
            .iter()
            .any(|n| express::is_a(n, "composite_shape_aspect"))
    );
    let components: Vec<_> = w
        .doc
        .referrers(target)
        .iter()
        .copied()
        .filter(|&id| {
            test_attribute(
                &w.doc,
                id,
                "shape_aspect_relationship",
                "relating_shape_aspect",
            ) == Some(&Attribute::EntityRef(target))
                && test_attribute(&w.doc, id, "shape_aspect_relationship", "name")
                    == Some(&Attribute::String(String::new()))
        })
        .collect();
    // Two geometry components, plus the relationship establishing the datum.
    assert_eq!(components.len(), 3);
    assert_eq!(
        w.added()
            .into_iter()
            .filter(|&id| w.names(id) == ["geometric_item_specific_usage"])
            .count(),
        2
    );
}

#[test]
fn a_location_on_a_datum_group_shares_its_datum_members_geometry_once() {
    let f = spool();
    let mut d = size(0, "10.", DimTolerance::Basic);
    d.kind = DimensionKind::Location {
        from: FeatureId(2),
        to: FeatureId(3),
        kind: LocationKind::LinearDistance,
        path: None,
        directed: false,
        angle: None,
    };
    let p = PartPmi {
        features: vec![
            faces(&[0]),
            faces(&[1]),
            Feature::Group {
                members: vec![FeatureId(0), FeatureId(1)],
                kind: GroupKind::PatternOfFeatures,
            },
            faces(&[2]),
        ],
        datums: vec![datum("A", 0), datum("B", 1), datum("C", 2)],
        dimensions: vec![d],
        ..PartPmi::default()
    };
    let w = written(&f, &p);
    assert_verified(&f, &w, &p);
    assert!(new_violations(&f.doc, &w.doc).is_empty());
    let loc = root(
        &w.doc,
        &w.read.provenance.parts[0].dimensions[0],
        &["dimensional_location"],
    );
    let from = common::pmi_usage::reference(
        &w.doc,
        loc,
        "shape_aspect_relationship",
        "relating_shape_aspect",
    )
    .unwrap();
    assert!(common::pmi_usage::is_a(&w.doc, from, "datum_feature"));
    assert_eq!(
        common::pmi_usage::geometry(&w.doc, from),
        [f.parts[0].faces[0], f.parts[0].faces[1]]
            .into_iter()
            .collect()
    );
    let to = common::pmi_usage::reference(
        &w.doc,
        loc,
        "shape_aspect_relationship",
        "related_shape_aspect",
    )
    .unwrap();
    assert!(common::pmi_usage::neutral(&w.doc, to));
    assert!(!common::pmi_usage::is_a(&w.doc, to, "datum_feature"));
    assert_eq!(
        common::pmi_usage::geometry(&w.doc, to),
        [f.parts[0].faces[2]].into_iter().collect()
    );
    assert!(
        w.doc
            .referrers(to)
            .iter()
            .any(|r| common::pmi_usage::reference(
                &w.doc,
                *r,
                "item_identified_representation_usage",
                "definition"
            ) == Some(to))
    );
    assert!(common::pmi_usage::violations(&w.doc).is_empty());
}

#[test]
fn add_locations_reuse_foreign_datum_features_directly() {
    let f = fixture("ap242/presentation-placement/spindle.step.gz");
    let before = pmi::read(&f.doc, &f.parts).unwrap();
    let p = &before.parts[0];
    let b = p
        .datums
        .iter()
        .find(|d| d.label().as_str() == "B")
        .unwrap()
        .feature()
        .unwrap();
    let d = p
        .dimensions
        .iter()
        .find(|d| matches!(d.kind, DimensionKind::Location { from, .. } if from == b))
        .unwrap()
        .clone();
    let addition = PartPmi {
        features: p.features.clone(),
        datums: p.datums.clone(),
        dimensions: vec![d],
        ..PartPmi::default()
    };
    let w = write(&f, &[(PartId(0), addition.clone())], Mode::Add).unwrap();
    assert_verified_as(&f, &w, &addition, Mode::Add);
    for id in f.doc.ids() {
        assert_eq!(
            &f.doc.bytes()[f.doc.span(id).unwrap()],
            &w.doc.bytes()[w.doc.span(id).unwrap()],
            "foreign entity #{id} changed"
        );
    }
    assert_eq!(f.doc.file_schema(), w.doc.file_schema());
    assert!(new_violations(&f.doc, &w.doc).is_empty());
    let loc = w
        .added()
        .into_iter()
        .find(|&id| w.names(id) == ["dimensional_location"])
        .unwrap();
    let Some(Attribute::EntityRef(a)) = test_attribute(
        &w.doc,
        loc,
        "shape_aspect_relationship",
        "relating_shape_aspect",
    ) else {
        panic!("endpoint")
    };
    assert!(
        w.names(*a)
            .iter()
            .any(|n| express::is_a(n, "datum_feature"))
    );
    assert!(
        *a <= f.doc.max_id(),
        "the location must reference the kept datum"
    );
    let components: Vec<_> = w
        .doc
        .referrers(*a)
        .iter()
        .copied()
        .filter(|&r| {
            w.names(r) == ["shape_aspect_relationship"]
                && test_attribute(
                    &w.doc,
                    r,
                    "shape_aspect_relationship",
                    "relating_shape_aspect",
                ) == Some(&Attribute::EntityRef(*a))
        })
        .filter(|&r| {
            common::pmi_usage::reference(
                &w.doc,
                r,
                "shape_aspect_relationship",
                "related_shape_aspect",
            )
            .is_some_and(|member| !common::pmi_usage::is_a(&w.doc, member, "datum"))
        })
        .collect();
    assert_eq!(components.len(), 2, "reuse both original face carriers");
}

#[cfg(feature = "presentation")]
fn settings_pmi() -> PartPmi {
    let note = |kind: &str, text: &str, index| AttributeSet {
        name: "semantic text".into(),
        on: None,
        description: None,
        general_property: None,
        items: vec![
            (kind.into(), AttributeValue::Text(text.into())),
            ("note index".into(), AttributeValue::Integer(index)),
        ],
    };
    PartPmi {
        features: vec![faces(&[0])],
        datums: vec![datum("A", 0)],
        material: Some(Material {
            id: "6082-T6".into(),
            name: None,
            density: None,
        }),
        general: vec![GeneralTolerance::Class {
            text: "ISO 2768-m".into(),
            standard: Some(pmi::standards::Iso2768::recognise("ISO 2768-m").unwrap()),
        }],
        surface_textures: textures(Some(FeatureId(0))),
        attributes: vec![
            note("general note", "Second note", 2),
            note("heat treatment", "T6", 0),
            note("surface treatment", "Anodise black", 0),
            note("edge condition", "Break sharp edges 0.2 max", 0),
            note("general note", "First note", 1),
        ],
        ..PartPmi::default()
    }
}

#[cfg(feature = "presentation")]
#[test]
fn written_settings_have_one_notes_block_and_face_texture_symbols_with_valid_links() {
    use haecceity::{geom, presentation};
    let f = gap_spool();
    let p = settings_pmi();
    let w = written(&f, &p);
    assert_verified(&f, &w, &p);
    assert_reads_back(&w, PartId(0), &p);
    assert!(new_violations(&f.doc, &w.doc).is_empty());
    let callouts: Vec<_> = w
        .added()
        .into_iter()
        .filter(|&id| w.names(id) == ["draughting_callout"])
        .collect();
    assert_eq!(callouts.len(), 3, "datum, face texture, general notes");
    let note = *callouts
        .iter()
        .find(|&&id| {
            test_attribute(&w.doc, id, "representation_item", "name")
                == Some(&Attribute::String("General notes".into()))
        })
        .unwrap();
    let expected = presentation::notes::notes_block(&presentation::notes::general_notes(&p))
        .cells
        .join("\n");
    assert_general_notes_names(&w.doc, note, &expected);
    let occurrences = refs_of(&w.doc, note);
    assert_eq!(
        occurrences.len(),
        8,
        "one occurrence per property; header shares the first"
    );
    let rows = presentation::notes::general_notes(&p);
    let block = presentation::notes::notes_block(&rows);
    let provenance = &w.read.provenance.parts[0];
    let source_property = |source| {
        use presentation::notes::Source;
        let ids = match source {
            Source::Material => &provenance.material,
            Source::General(i) => &provenance.general[i],
            Source::Texture(i) => &provenance.surface_textures[i],
            Source::Note(i) => &provenance.notes[i],
            Source::Attribute(i) => {
                assert_eq!(w.read.parts[0].attributes[i].items, p.attributes[i].items);
                &provenance.attributes[i]
            }
        };
        root(&w.doc, ids, &["property_definition"])
    };
    let (origin, normal, along) = annotation_basis(&w.doc, note);
    let up = geom::cross(normal, along);
    let physical = haecceity::step::read_part(f.doc.bytes(), &f.parts[0]).unwrap();
    let scale = presentation::placement::text_height(physical.bounds().max_extent()) / 3.;
    let mut properties = BTreeSet::new();
    for occurrence in occurrences {
        let set = *refs_of(&w.doc, occurrence).last().unwrap();
        assert_eq!(
            test_attribute(&w.doc, set, "representation_item", "name"),
            Some(&Attribute::String("note".into()))
        );
        // Each property links its own occurrence, keeping IIRU UR1 and UR2 satisfied.
        let links: Vec<_> = w
            .added()
            .into_iter()
            .filter(|&id| {
                w.names(id) == ["draughting_model_item_association"]
                    && refs_of(&w.doc, id).last() == Some(&occurrence)
            })
            .collect();
        assert_eq!(links.len(), 1);
        let ids = refs_of(&w.doc, links[0]);
        assert_eq!(w.names(ids[0]), ["property_definition"]);
        assert_eq!(w.names(ids[1]), ["draughting_model"]);
        assert_eq!(
            test_attribute(
                &w.doc,
                links[0],
                "item_identified_representation_usage",
                "name"
            ),
            Some(&Attribute::String(
                "PMI representation to presentation link".into()
            ))
        );
        let source = rows
            .iter()
            .map(|r| r.source)
            .find(|&source| source_property(source) == ids[0])
            .expect("link to a stated property");
        let mut expected_lines = Vec::new();
        for (i, text) in block.letters.iter().enumerate() {
            if rows[i.saturating_sub(1)].source != source {
                continue;
            }
            expected_lines.extend(
                presentation::font()
                    .text(&text.text, text.size)
                    .lines
                    .into_iter()
                    .map(|line| {
                        line.into_iter()
                            .map(|q| {
                                geom::add(
                                    origin,
                                    geom::add(
                                        geom::scale(along, (q[0] + text.at[0]) * scale),
                                        geom::scale(up, (q[1] + text.at[1]) * scale),
                                    ),
                                )
                            })
                            .collect::<Vec<_>>()
                    }),
            );
        }
        let actual_lines = occurrence_lines(&w.doc, occurrence);
        assert_eq!(
            actual_lines.len(),
            expected_lines.len(),
            "strokes for {source:?}"
        );
        for (actual, expected) in actual_lines.iter().zip(expected_lines) {
            assert_eq!(actual.len(), expected.len());
            assert!(
                actual
                    .iter()
                    .zip(expected)
                    .all(|(a, b)| geom::dist(*a, b) < 1e-9),
                "the drawn row must state its associated property {source:?}"
            );
        }
        properties.insert(ids[0]);
    }
    assert_eq!(properties.len(), 8);
    // Notes form a compact column beside the part in their front plane.
    assert_notes_clear(&w.doc);
    let texture = *callouts
        .iter()
        .find(|&&id| w.text(id).contains("Ra 0.8 Rz 4"))
        .unwrap();
    let links: Vec<_> = w
        .added()
        .into_iter()
        .filter(|&id| {
            w.names(id) == ["draughting_model_item_association"]
                && refs_of(&w.doc, id).last() == Some(&texture)
        })
        .collect();
    assert_eq!(links.len(), 1);
    let ids = refs_of(&w.doc, links[0]);
    assert_eq!(w.names(ids[0]), ["property_definition"]);
    assert_eq!(
        test_attribute(
            &w.doc,
            links[0],
            "item_identified_representation_usage",
            "name"
        ),
        Some(&Attribute::String(
            "PMI representation to presentation link".into()
        ))
    );
    assert_texture_tip(&w.doc, &p, &f.parts[0], texture, FeatureId(0));
    let set = *refs_of(&w.doc, refs_of(&w.doc, texture)[0]).last().unwrap();
    assert_eq!(
        test_attribute(&w.doc, set, "representation_item", "name"),
        Some(&Attribute::String("surface roughness".into()))
    );
}

#[cfg(feature = "presentation")]
#[test]
fn absent_settings_add_no_notes_and_append_does_not_restate_foreign_settings() {
    let f = gap_spool();
    let original = written(&f, &settings_pmi());
    let carried = open_bytes(original.doc.bytes().to_vec());
    let no_settings = PartPmi::default();
    let unchanged = write(&carried, &[(PartId(0), no_settings)], Mode::Add).unwrap();
    assert_eq!(unchanged.doc.bytes(), carried.doc.bytes());
    let new = PartPmi {
        attributes: vec![AttributeSet {
            name: "semantic text".into(),
            on: None,
            description: None,
            general_property: None,
            items: vec![(
                "general note".into(),
                AttributeValue::Text("Added only".into()),
            )],
        }],
        ..PartPmi::default()
    };
    let w = write(&carried, &[(PartId(0), new.clone())], Mode::Add).unwrap();
    assert_verified_as(&carried, &w, &new, Mode::Add);
    for id in carried.doc.ids() {
        assert_eq!(
            &carried.doc.bytes()[carried.doc.span(id).unwrap()],
            &w.doc.bytes()[w.doc.span(id).unwrap()],
            "foreign entity #{id}"
        );
    }
    assert_notes_clear(&w.doc);
    let new_notes: Vec<_> = w
        .added()
        .into_iter()
        .filter(|&id| w.names(id) == ["draughting_callout"])
        .collect();
    assert_eq!(new_notes.len(), 1);
    assert_general_notes_names(&w.doc, new_notes[0], "GENERAL NOTES\n1  Added only");
    let blank = written(&f, &PartPmi::default());
    assert!(
        blank
            .added()
            .iter()
            .all(|&id| blank.names(id) != ["draughting_callout"])
    );
}

// The tree title is short; the complete, unmodified wording stays with the drawn strokes.
#[cfg(feature = "presentation")]
fn assert_general_notes_names(doc: &Document, callout: u64, expected: &str) {
    let assert_title = |id| {
        assert_eq!(
            test_attribute(doc, id, "representation_item", "name"),
            Some(&Attribute::String("General notes".into()))
        )
    };
    assert_title(callout);
    let plane = doc
        .ids()
        .find(|&id| {
            matches!(doc.get(id),
        Some(RawEntity::Simple{name,..}) if name.eq_ignore_ascii_case("annotation_plane"))
                && refs_of(doc, id).contains(&callout)
        })
        .unwrap();
    assert_title(plane);
    assert_title(refs_of(doc, plane)[1]);
    let occurrences = refs_of(doc, callout);
    assert!(!occurrences.is_empty());
    for occurrence in occurrences {
        assert_title(occurrence);
        let set = *refs_of(doc, occurrence).last().unwrap();
        assert_eq!(
            test_attribute(doc, set, "representation_item", "name"),
            Some(&Attribute::String("note".into()))
        );
        let curves = refs_of(doc, set);
        assert_eq!(curves.len(), 1);
        assert_eq!(
            test_attribute(doc, curves[0], "representation_item", "name"),
            Some(&Attribute::String(expected.into()))
        );
        assert!(!occurrence_lines(doc, occurrence).is_empty());
    }
}

#[cfg(feature = "presentation")]
fn assert_notes_clear(doc: &Document) {
    use haecceity::{geom, presentation::placement};
    // Notes are readable in their own front plane. Requiring clearance in all
    // seven projections forced the old search arbitrarily far from the part.
    let defs = read_part_definitions(doc.bytes()).unwrap();
    let part = haecceity::step::read_part(doc.bytes(), &defs[0]).unwrap();
    let bounds = part.bounds();
    let gap = 2. * placement::text_height(bounds.max_extent());
    let mut notes = Vec::new();
    for id in doc.ids().filter(|&id| {
        test_attribute(doc, id, "representation_item", "name")
            == Some(&Attribute::String("General notes".into()))
            && matches!(doc.get(id), Some(RawEntity::Simple { name, .. }) if name.eq_ignore_ascii_case("draughting_callout"))
    }) {
        let mut ink = geom::Bounds::empty();
        for p in graphic_lines(doc, id).into_iter().flatten() { ink.add(p); }
        assert!((ink.min[0] - bounds.max[0] - gap).abs() < 1e-6);
        assert!(ink.max[2] <= bounds.min[2] - gap + 1e-6);
        assert!((ink.min[1] - bounds.centre()[1]).abs() < 1e-6);
        for other in &notes {
            let other: &geom::Bounds = other;
            assert!(other.min[2] >= ink.max[2] + gap - 1e-6
                || ink.min[2] >= other.max[2] + gap - 1e-6);
        }
        notes.push(ink);
    }
}

#[cfg(feature = "presentation")]
#[test]
fn full_notes_and_face_finishes_clear_the_three_sample_parts_without_moving_dimensions() {
    use haecceity::presentation;
    for name in ["spindle", "plate", "corners-flatness"] {
        let f = fixture(&format!("ap242/presentation-placement/{name}.step.gz"));
        let read = pmi::read(&f.doc, &f.parts).unwrap();
        let mut p = read.parts[0].clone();
        let settings = settings_pmi();
        p.material = settings.material;
        p.general = settings.general;
        p.attributes = settings.attributes;
        let mut textures = settings.surface_textures;
        let feature = p.datums[0].feature().unwrap();
        textures[1].on = Some(feature);
        p.surface_textures = textures;
        let w = written(&f, &p);
        assert_verified(&f, &w, &p);
        assert_reads_back(&w, PartId(0), &p);
        assert!(new_violations(&f.doc, &w.doc).is_empty());
        let blocks: Vec<_> = w
            .added()
            .into_iter()
            .filter(|&id| {
                w.names(id) == ["draughting_callout"]
                    && test_attribute(&w.doc, id, "representation_item", "name")
                        == Some(&Attribute::String("General notes".into()))
            })
            .collect();
        assert_eq!(blocks.len(), 1, "{name}: one complete notes block");
        let expected = presentation::notes::notes_block(&presentation::notes::general_notes(&p))
            .cells
            .join("\n");
        assert_general_notes_names(&w.doc, blocks[0], &expected);
        assert_notes_clear(&w.doc);
        sample_linear_dimensions(&w.doc, &w.read, &f.parts[0]);
        sample_datums_and_frames(&w.doc, &w.read, &f.parts[0]);
        assert_frames_and_diameters_clear(&w.doc);
        let textures: Vec<_> = w
            .added()
            .into_iter()
            .filter(|&id| {
                w.names(id) == ["draughting_callout"]
                    && test_attribute(&w.doc, id, "representation_item", "name")
                        == Some(&Attribute::String("Ra 0.8 Rz 4".into()))
            })
            .collect();
        assert_eq!(textures.len(), 1, "{name}: one surface texture symbol");
        assert_texture_tip(&w.doc, &p, &f.parts[0], textures[0], feature);
    }
}

#[cfg(feature = "presentation")]
fn assert_texture_tip(
    doc: &Document,
    p: &PartPmi,
    def: &PartDefinition,
    callout: u64,
    feature: FeatureId,
) {
    use haecceity::{geom, presentation};
    let part = haecceity::step::read_part(doc.bytes(), def).unwrap();
    let (_, _, face) = presentation::placement::feature_site(&part, p, feature).unwrap();
    let lines = graphic_lines(doc, callout);
    let tip = lines[0][1];
    assert_eq!(tip, lines[1][0], "both ISO legs meet on the face");
    let surface = &part.faces[face].surface;
    let (u, v) = surface.parameters(tip, None).unwrap();
    assert!(geom::dist(surface.value(u, v), tip) < 2e-6);
    let (u0, u1, v0, v1) = part.uv_bounds(face).unwrap();
    let du = (u1 - u0).abs().max(1e-6) * 1e-5;
    let dv = (v1 - v0).abs().max(1e-6) * 1e-5;
    assert!(
        [(u, v), (u - du, v), (u + du, v), (u, v - dv), (u, v + dv)]
            .iter()
            .all(|&(u, v)| part.domain(face).unwrap().contains(u, v)),
        "ISO symbol tip is strictly inside the semantic face"
    );
    let normal = geom::scale(
        surface.normal(u, v).unwrap(),
        if part.faces[face].reversed { -1. } else { 1. },
    );
    let (origin, plane_normal, along) = annotation_basis(doc, callout);
    assert!(
        geom::dist(origin, tip) < 1e-9,
        "annotation plane is anchored at the symbol tip"
    );
    assert!(geom::dot(normal, plane_normal).abs() < 1e-6);
    assert!(
        geom::dot(normal, geom::cross(plane_normal, along)) > 1. - 1e-6,
        "symbol rises outward from the face"
    );
}

// Native STEP may renumber support aspects and reorder properties without changing meaning.
#[test]
fn differences_accept_native_feature_aliases_and_attribute_canonicalization() {
    let p = PartPmi {
        features: vec![
            Feature::Items(vec![Anchor::Face(FaceIndex(0))]),
            Feature::Items(vec![Anchor::Face(FaceIndex(1))]),
        ],
        attributes: vec![
            AttributeSet {
                name: "a".into(),
                on: Some(NoteOwner::Feature(FeatureId(0))),
                items: vec![("value".into(), AttributeValue::Text("x".into()))],
                description: None,
                general_property: None,
            },
            AttributeSet {
                name: "b".into(),
                on: Some(NoteOwner::Feature(FeatureId(1))),
                items: vec![("value".into(), AttributeValue::Text("y".into()))],
                description: None,
                general_property: None,
            },
        ],
        ..PartPmi::default()
    };
    let mut canonicalized = p.clone();
    canonicalized.features.swap(0, 1);
    canonicalized.attributes[0].on = Some(NoteOwner::Feature(FeatureId(1)));
    canonicalized.attributes[1].on = Some(NoteOwner::Feature(FeatureId(0)));
    canonicalized.attributes.swap(0, 1);
    canonicalized.attributes[0].description = Some("user defined attribute".into());
    // Equal-content support aliases are also accepted by the original comparator.
    canonicalized
        .features
        .push(canonicalized.features[1].clone());
    canonicalized.attributes[1].on = Some(NoteOwner::Feature(FeatureId(2)));
    assert!(differences(&p, &canonicalized).is_empty());
    assert!(differences_as_stated(&p, &canonicalized).is_empty());
    #[cfg(feature = "json-import")]
    assert!(!haecceity::json::identity_differences(&p, &canonicalized).is_empty());
}

#[cfg(feature = "presentation")]
#[test]
fn blank_notes_emit_no_empty_curves_and_keep_row_spacing() {
    use haecceity::presentation::notes;
    let f = gap_spool();
    let mut p = PartPmi::default();
    for (i, text) in ["First", "", " \t ", "Middle\n\n \t\nLast\n", "End"]
        .iter()
        .enumerate()
    {
        p.attributes.push(AttributeSet {
            name: "semantic text".into(),
            on: None,
            description: None,
            general_property: None,
            items: vec![
                ("general note".into(), AttributeValue::Text((*text).into())),
                ("note index".into(), AttributeValue::Integer(i as i64 + 1)),
            ],
        });
    }
    let block = notes::notes_block(&notes::general_notes(&p));
    assert_eq!(block.letters.len(), 6);
    assert!(block.letters[1].at[1] - block.letters[4].at[1] > 3. * 4.8);
    let w = written(&f, &p);
    assert_verified(&f, &w, &p);
    assert_reads_back(&w, PartId(0), &p);
    assert!(new_violations(&f.doc, &w.doc).is_empty());
    let mut curves = 0;
    for id in w
        .added()
        .into_iter()
        .filter(|&id| w.names(id) == ["tessellated_curve_set"])
    {
        curves += 1;
        let Some(Attribute::List(strips)) =
            test_attribute(&w.doc, id, "tessellated_curve_set", "line_strips")
        else {
            panic!("strips")
        };
        assert!(!strips.is_empty(), "curve #{id}");
        assert!(
            strips
                .iter()
                .all(|s| matches!(s, Attribute::List(points) if points.len() >= 2))
        );
    }
    assert_eq!(
        curves, 3,
        "blank requirements have no tessellated occurrences"
    );
}

#[cfg(feature = "presentation")]
fn assert_frames_and_diameters_clear(doc: &Document) {
    use haecceity::{geom, presentation::linear};
    let mut ink = Vec::new();
    for id in doc.ids().filter(|&id| matches!(doc.get(id), Some(RawEntity::Simple { name, .. }) if name.eq_ignore_ascii_case("draughting_callout"))) {
        let occurrence = refs_of(doc, id)[0];
        let set = *refs_of(doc, occurrence).last().unwrap();
        let Some(Attribute::String(kind)) = test_attribute(doc, set, "representation_item", "name") else { panic!("kind") };
        if matches!(kind.as_str(), "linear dimension" | "datum" | "note" | "surface roughness") { continue; }
        let mut lines = graphic_lines(doc, id);
        let leader = lines.pop().unwrap();
        let (_, normal, along) = annotation_basis(doc, id);
        let up = geom::cross(normal, along);
        let interval = |axis| lines.iter().flatten().map(|p| geom::dot(*p, axis)).fold([f64::INFINITY, f64::NEG_INFINITY], |r, t| [r[0].min(t), r[1].max(t)]);
        let x = interval(along); let y = interval(up);
        // Independent oriented ink rectangle from serialized strokes, not layout metadata.
        let depth = geom::dot(lines[0][0], normal);
        let polygon: Vec<_> = [(x[0], y[0]), (x[1], y[0]), (x[1], y[1]), (x[0], y[1])].into_iter().map(|(x,y)| geom::add(geom::scale(normal, depth), geom::add(geom::scale(along, x), geom::scale(up, y)))).collect();
        ink.push((id, polygon, leader, normal, lines));
    }
    fn clearance(polygon: &[geom::V3], other: &[geom::V3], view: geom::V3) -> f64 {
        let mut clearance = linear::projected_clearance(polygon, other, view);
        let n = geom::unit(view).unwrap();
        for axis in [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]] {
            let Some(axis) = geom::unit(geom::sub(axis, geom::scale(n, geom::dot(axis, n)))) else {
                continue;
            };
            let interval = |p: &[geom::V3]| {
                p.iter()
                    .map(|p| geom::dot(*p, axis))
                    .fold([f64::INFINITY, f64::NEG_INFINITY], |r, t| {
                        [r[0].min(t), r[1].max(t)]
                    })
            };
            let x = interval(polygon);
            let y = interval(other);
            clearance = clearance.max((x[0] - y[1]).max(y[0] - x[1]));
        }
        clearance
    }
    let mut worst = (f64::INFINITY, 0, 0, [0.; 3]);
    for (i, (a, polygon, leader, normal, strokes)) in ink.iter().enumerate() {
        for (b, other, other_leader, other_normal, other_strokes) in &ink[..i] {
            for view in linear::VIEWS {
                // Proper crossings of actual ink, including both leaders, in every
                // view. Collinear collapsed edge-on strokes and shared attachment
                // endpoints are not transverse crossings.
                let n = geom::unit(view).unwrap();
                for x in strokes
                    .iter()
                    .chain(std::iter::once(leader))
                    .flat_map(|l| l.windows(2))
                {
                    for y in other_strokes
                        .iter()
                        .chain(std::iter::once(other_leader))
                        .flat_map(|l| l.windows(2))
                    {
                        let dx = geom::sub(x[1], x[0]);
                        let dy = geom::sub(y[1], y[0]);
                        let delta = geom::sub(y[0], x[0]);
                        let cross = |u, v| geom::dot(n, geom::cross(u, v));
                        let den = cross(dx, dy);
                        if den.abs() < 1e-10 {
                            continue;
                        }
                        let t = cross(delta, dy) / den;
                        let u = cross(delta, dx) / den;
                        assert!(
                            !(t > 1e-7 && t < 1. - 1e-7 && u > 1e-7 && u < 1. - 1e-7),
                            "frame/diameter crossing #{a} / #{b}, view {view:?}, {x:?} / {y:?}"
                        );
                    }
                }
                if geom::dot(*normal, view).abs() < 1e-8
                    || geom::dot(*other_normal, view).abs() < 1e-8
                {
                    continue;
                }
                for (x, y) in std::iter::once((polygon.as_slice(), other.as_slice()))
                    .chain(other_leader.windows(2).map(|l| (polygon.as_slice(), l)))
                    .chain(leader.windows(2).map(|l| (l, other.as_slice())))
                {
                    let gap = clearance(x, y, view);
                    assert!(
                        gap > 1e-8,
                        "frame/diameter #{a} / #{b} view {view:?}: {gap}, {x:?} / {y:?}"
                    );
                    if gap < worst.0 {
                        worst = (gap, *a, *b, view);
                    }
                }
            }
        }
    }
    println!(
        "frame/diameter clearance {:.9} mm, #{} / #{}, view {:?}",
        worst.0, worst.1, worst.2, worst.3
    );
}

#[test]
fn invalid_density_units_report_the_dimensional_reason_and_old_grams_still_read() {
    let raw = include_bytes!("fixtures/q142/invalid-density.step.gz");
    let mut bytes = vec![];
    flate2::read::GzDecoder::new(&raw[..])
        .read_to_end(&mut bytes)
        .unwrap();
    let invalid = String::from_utf8(bytes.clone())
        .unwrap()
        .replace("(#505,3.)", "(#505,4.)");
    let f = open_bytes(invalid.into_bytes());
    let read = pmi::read(&f.doc, &f.parts).unwrap();
    assert!(read.parts[0].material.as_ref().unwrap().density.is_none());
    assert!(
        read.findings
            .iter()
            .any(|f| f.detail.contains("not mass / length cubed"))
    );
    let text = String::from_utf8(bytes)
        .unwrap()
        .replace("(#505,3.)", "(#505,1.)")
        .replace("(#506,2.)", "(#506,-3.)");
    let f = open_bytes(text.into_bytes());
    let read = pmi::read(&f.doc, &f.parts).unwrap();
    let density = read.parts[0]
        .material
        .as_ref()
        .unwrap()
        .density
        .as_ref()
        .unwrap();
    assert_eq!(
        density.unit,
        vec![
            ("gram".into(), dec("1.")),
            ("centimetre".into(), dec("-3."))
        ]
    );
    let written = write(&f, &[(PartId(0), read.parts[0].clone())], Mode::Replace).unwrap();
    let text = String::from_utf8_lossy(written.doc.bytes());
    assert!(text.contains("CONVERSION_BASED_UNIT('gram',"));
    assert!(text.contains("SI_UNIT(.KILO.,.GRAM.)"));
    assert_eq!(written.read.parts[0].material, read.parts[0].material);
    let custom = text.replace(
        "CONVERSION_BASED_UNIT('gram',",
        "CONVERSION_BASED_UNIT('custom mass',",
    );
    let custom = open_bytes(custom.into_bytes());
    let read = pmi::read(&custom.doc, &custom.parts).unwrap();
    assert_eq!(
        read.parts[0]
            .material
            .as_ref()
            .unwrap()
            .density
            .as_ref()
            .unwrap()
            .unit[0]
            .0,
        "custom mass"
    );
    assert!(
        !read
            .findings
            .iter()
            .any(|f| f.detail.contains("not mass / length cubed"))
    );
}

/// #142 Round 4: the actual OCCT 8 producer file has both the hard-coded
/// mass^3/length^2 bug and an untyped REAL select. Neither is used by the written density.
#[test]
fn occt_density_plate_reads_with_findings_and_writes_practice_form() {
    let raw = include_bytes!("fixtures/q142/occt-density-plate.step.gz");
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(&raw[..])
        .read_to_end(&mut bytes)
        .unwrap();
    let f = open_bytes(bytes);
    let read = pmi::read(&f.doc, &f.parts).unwrap();
    let p = &read.parts[0];
    let material = p.material.as_ref().unwrap();
    assert_eq!(material.id, "Alu_G6061_T6");
    assert_eq!(
        material.density,
        Some(Density {
            value: dec("2.7"),
            unit: vec![
                ("gram".into(), dec("1.")),
                ("centimetre".into(), dec("-3."))
            ],
        })
    );
    for detail in [
        "OCCT writes mass^3 · length^2",
        "density value untyped (OCCT)",
    ] {
        assert!(
            read.findings
                .iter()
                .any(|f| f.detail.contains(detail) && f.ids.contains(&725)),
            "{:#?}",
            read.findings
        );
    }
    let w = written(&f, p);
    assert_verified(&f, &w, p);
    assert!(express::TARGET.validate_document(w.doc.graph()).is_empty());
    assert!(haecceity::express_rules::check_all(&w.doc).is_empty());
    let text = String::from_utf8_lossy(w.doc.bytes());
    assert!(text.contains("'density measure',POSITIVE_RATIO_MEASURE(2.7)"));
    assert!(text.contains("CONVERSION_BASED_UNIT('gram',"));
    assert!(text.contains("SI_UNIT(.KILO.,.GRAM.)"));
    // Source named units may be retained as unreferenced source instances;
    // the new density must use the conversion-based unit above.
    assert!(!text.contains("MEASURE_REPRESENTATION_ITEM('density',2.7,"));
    let measure = w
        .added()
        .into_iter()
        .find(|&id| {
            matches!(test_attribute(&w.doc, id, "representation_item", "name"),
            Some(Attribute::String(name)) if name == "density measure")
        })
        .unwrap();
    let Some(Attribute::EntityRef(unit)) =
        test_attribute(&w.doc, measure, "measure_with_unit", "unit_component")
    else {
        panic!("density has no derived unit");
    };
    let elements = refs_of(&w.doc, *unit);
    assert_eq!(elements.len(), 2);
    for (element, expected) in elements.iter().zip([1., -3.]) {
        assert_eq!(
            test_attribute(&w.doc, *element, "derived_unit_element", "exponent"),
            Some(&Attribute::Real(expected))
        );
        let Some(Attribute::EntityRef(named)) =
            test_attribute(&w.doc, *element, "derived_unit_element", "unit")
        else {
            panic!("missing base unit");
        };
        if expected == 1. {
            assert!(w.text(*named).contains("CONVERSION_BASED_UNIT('gram',"));
        } else {
            assert!(w.text(*named).contains("SI_UNIT(.CENTI.,.METRE.)"));
        }
    }
    assert!(
        !w.read.findings.iter().any(|f| f.detail.contains("density")),
        "{:#?}",
        w.read.findings
    );
}

#[test]
fn occt_density_repair_requires_exactly_two_mass_and_length_factors() {
    let raw = include_bytes!("fixtures/q142/invalid-density.step.gz");
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(&raw[..])
        .read_to_end(&mut bytes)
        .unwrap();
    let text = String::from_utf8(bytes).unwrap();
    // The Round 3 reproducer is a typed version of OCCT's unit bug.
    // A reversed derived-unit SET must work as well.
    for input in [
        text.clone(),
        text.replace("((#507,#508))", "((#508,#507))"),
        text.replace("(#505,3.)", "(#505,3.00)")
            .replace("(#506,2.)", "(#506,2.E0)"),
    ] {
        let f = open_bytes(input.into_bytes());
        let read = pmi::read(&f.doc, &f.parts).unwrap();
        let d = read.parts[0]
            .material
            .as_ref()
            .unwrap()
            .density
            .as_ref()
            .unwrap();
        assert_eq!(d.value, dec("2.7"));
        assert!(d.unit.contains(&("gram".into(), dec("1."))));
        assert!(d.unit.contains(&("centimetre".into(), dec("-3."))));
        assert_eq!(
            read.findings
                .iter()
                .filter(|f| f.detail.contains("OCCT writes"))
                .count(),
            1
        );
        assert!(!read.findings.iter().any(|f| f.detail.contains("untyped")));
    }
    for input in [
        text.replace("(#505,3.)", "(#505,1.)"),
        text.replace("(#506,2.)", "(#506,3.)"),
        // Different exact decimals must not qualify through f64 rounding.
        text.replace("(#505,3.)", "(#505,3.00000000000000000001)"),
        text.replace("(#506,2.)", "(#506,2.00000000000000000001)"),
        // Same aggregate mass^3/length^2 dimensions, but three factors: refuse.
        text.replace("(#505,3.)", "(#505,1.5)")
            .replace("((#507,#508))", "((#507,#508,#513))")
            .replace(
                "ENDSEC;\nEND-ISO",
                "#513=DERIVED_UNIT_ELEMENT(#505,1.5);\nENDSEC;\nEND-ISO",
            ),
    ] {
        assert_ne!(input, text);
        let f = open_bytes(input.into_bytes());
        let read = pmi::read(&f.doc, &f.parts).unwrap();
        assert!(read.parts[0].material.as_ref().unwrap().density.is_none());
        assert!(
            read.findings
                .iter()
                .any(|f| f.detail.contains("not mass / length cubed"))
        );
        assert!(
            !read
                .findings
                .iter()
                .any(|f| f.detail.contains("OCCT writes"))
        );
    }
}

/// The only closed subtype rewrite has the same explicit attribute layout in both carried
/// EXPRESS schemas (including inherited attributes, optionality, types and aggregate bounds).
#[test]
fn ap214_product_type_attribute_contract() {
    use express::{AggregateKind, Ty};
    for edition in express::EDITIONS {
        assert!(!edition.is_declared("product_type"));
        let slots = edition
            .explicit_attributes("product_related_product_category")
            .unwrap();
        assert_eq!(slots.len(), 3);
        for (slot, name, owner, optional) in [
            (&slots[0], "name", "product_category", false),
            (&slots[1], "description", "product_category", true),
            (
                &slots[2],
                "products",
                "product_related_product_category",
                false,
            ),
        ] {
            assert_eq!(
                (slot.name, slot.entity, slot.optional),
                (name, owner, optional)
            );
            assert!(!slot.derived && slot.redeclared.is_empty());
        }
        assert!(matches!(slots[0].ty, Ty::Named("label")));
        assert!(matches!(slots[1].ty, Ty::Named("text")));
        let Ty::Aggregate(products) = slots[2].ty else {
            panic!("products is a SET")
        };
        assert_eq!(products.kind, AggregateKind::Set);
        assert_eq!((products.lower, products.upper), (Some(1), None));
        assert!(!products.optional && !products.unique);
        assert!(matches!(products.of, Ty::Named("product")));
    }
}

fn product_type_pmi() -> PartPmi {
    PartPmi {
        // Face 3 of the original analytic disc is its upper annular plane.
        features: vec![faces(&[3])],
        tolerances: vec![tolerance(
            ToleranceKind::Flatness,
            ToleranceTarget::Feature(FeatureId(0)),
            "0.05",
        )],
        material: Some(Material {
            id: "6061-T6".into(),
            name: None,
            density: None,
        }),
        ..PartPmi::default()
    }
}

#[test]
fn ap214_product_type_keyword_only_and_readback() {
    use haecceity::pmi::write::EditionRewrite;
    let f =
        open(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ap214_product_type.step"));
    let empty = write(&f, &[(PartId(0), PartPmi::default())], Mode::Add).unwrap();
    assert_eq!(empty.doc.bytes(), f.doc.bytes());
    assert!(empty.report.edition_rewrites.is_empty());
    let p = product_type_pmi();
    let w = write(&f, &[(PartId(0), p.clone())], Mode::Add).unwrap();
    assert_reads_back(&w, PartId(0), &p);
    assert_verified_as(&f, &w, &p, Mode::Add);
    assert_eq!(
        w.report.edition_rewrites,
        [EditionRewrite {
            id: 132,
            from: "PRODUCT_TYPE",
            to: "PRODUCT_RELATED_PRODUCT_CATEGORY",
        }]
    );
    assert!(w.report.repairs.is_empty());
    for id in f.doc.ids() {
        let before = &f.doc.bytes()[f.doc.span(id).unwrap()];
        let after = &w.doc.bytes()[w.doc.span(id).unwrap()];
        if id == 132 {
            assert_eq!(
                after,
                String::from_utf8(before.to_vec())
                    .unwrap()
                    .replace("PrOdUcT_TyPe", "PRODUCT_RELATED_PRODUCT_CATEGORY")
                    .as_bytes()
            );
        } else {
            assert_eq!(before, after, "#{id} changed");
        }
    }
    // Reverse only the schema edit, remove additions, and reverse the keyword: all bytes,
    // including whitespace/comments between records, return exactly to the input.
    let mut undo = haecceity::p21::Edit::new();
    undo.set_file_schema(f.doc.file_schema().unwrap());
    for id in w.doc.ids().filter(|id| *id > f.doc.max_id()) {
        undo.remove(id);
    }
    let bytes = w.doc.apply(&undo).unwrap().bytes;
    let restored = String::from_utf8(bytes).unwrap().replacen(
        "PRODUCT_RELATED_PRODUCT_CATEGORY /* category */",
        "PrOdUcT_TyPe /* category */",
        1,
    );
    assert_eq!(restored.as_bytes(), f.doc.bytes());

    // Neither AP242 nor AP203 qualifies for an AP214 rewrite. AP242 keeps its preexisting
    // undeclared instance (as before); AP203 must pass a complete upgrade and refuses.
    for schema in [express::TARGET_SCHEMA, "CONFIG_CONTROL_DESIGN"] {
        let mut edit = haecceity::p21::Edit::new();
        edit.set_file_schema(vec![schema.into()]);
        let other = open_bytes(f.doc.apply(&edit).unwrap().bytes);
        let result = write(&other, &[(PartId(0), p.clone())], Mode::Add);
        if schema == express::TARGET_SCHEMA {
            let w = result.unwrap();
            assert!(w.report.edition_rewrites.is_empty());
            assert_eq!(
                &w.doc.bytes()[w.doc.span(132).unwrap()],
                &other.doc.bytes()[other.doc.span(132).unwrap()]
            );
        } else {
            assert!(matches!(result, Err(WriteError::Edition { .. })));
        }
    }
}

#[test]
fn ap214_non_table_entity_still_refuses() {
    // AP214's APPLICATION_CONTEXT_RELATIONSHIP is absent from AP242. No guessed mapping.
    let f = ap214_with_records("#900000000=APPLICATION_CONTEXT_RELATIONSHIP('',$,#2,#2);\n");
    let e = write(&f, &[(PartId(0), upgrade_datum())], Mode::Add)
        .err()
        .expect("undeclared entity must refuse");
    assert!(e.to_string().contains("#900000000 application_context_relationship undeclared: application_context_relationship is not declared in AP242_"), "{e}");
    assert!(matches!(e, WriteError::Edition { .. }));
}

#[test]
fn ap214_extra_attributes_are_not_dropped() {
    let f =
        open(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ap214_product_type.step"));
    let Some(RawEntity::Simple { attributes, .. }) = f.doc.get(132) else {
        panic!("simple")
    };
    let mut attributes = attributes.clone();
    attributes.push(Attribute::Unset);
    let mut edit = haecceity::p21::Edit::new();
    edit.replace(132, haecceity::p21::simple("PRODUCT_TYPE", attributes));
    let f = open_bytes(f.doc.apply(&edit).unwrap().bytes);
    let e = write(&f, &[(PartId(0), product_type_pmi())], Mode::Add)
        .err()
        .expect("extra attribute must refuse");
    let WriteError::Edition { violations, .. } = e else {
        panic!("{e}")
    };
    assert!(
        violations
            .iter()
            .any(|v| v.id == 132 && v.kind == express::Kind::AttributeCount)
    );
}

#[test]
fn add_plain_requirements_do_not_inherit_specialized_aspect_meaning() {
    use common::pmi_usage::{geometry, is_a, neutral, reference, violations};
    for kind in [
        "APPLIED_AREA",
        "THREAD_RUNOUT",
        "COMPOSITE_GROUP_SHAPE_ASPECT",
        "ALL_AROUND_SHAPE_ASPECT",
        "BETWEEN_SHAPE_ASPECT",
        "COMPOSITE_UNIT_SHAPE_ASPECT",
        "TOLERANCE_ZONE",
    ] {
        for count in [1, 2] {
            if kind == "COMPOSITE_UNIT_SHAPE_ASPECT" && count == 1 {
                continue; // A unit composite needs multiple constituents.
            }
            let input = spool();
            let base = PartPmi {
                features: vec![faces(&[0])],
                tolerances: vec![tolerance(
                    ToleranceKind::Flatness,
                    ToleranceTarget::Feature(FeatureId(0)),
                    "0.1",
                )],
                ..PartPmi::default()
            };
            let clean = written(&input, &base);
            let f = open_bytes(clean.doc.bytes().to_vec());
            let free = free_faces(&f, 0, 2);
            let def = &f.parts[0];
            let old = pmi::read(&f.doc, &f.parts).unwrap();
            let tol = root(
                &f.doc,
                &old.provenance.parts[0].tolerances[0],
                &["geometric_tolerance"],
            );
            let gisu = f
                .doc
                .ids()
                .find(|id| is_a(&f.doc, *id, "geometric_item_specific_usage"))
                .unwrap();
            let rep = reference(
                &f.doc,
                gisu,
                "item_identified_representation_usage",
                "used_representation",
            )
            .unwrap();
            let mut edit = haecceity::p21::Edit::new();
            let form = edit.add(haecceity::p21::simple(
                "TOLERANCE_ZONE_FORM",
                vec![Attribute::String("between two parallel planes".into())],
            ));
            let base = vec![
                Attribute::String("kept specialized scope".into()),
                Attribute::String(
                    if kind == "COMPOSITE_GROUP_SHAPE_ASPECT" {
                        "pattern of features"
                    } else {
                        ""
                    }
                    .into(),
                ),
                Attribute::EntityRef(def.shape),
                Attribute::Enum("T".into()),
            ];
            let record = if kind == "TOLERANCE_ZONE" {
                let mut attrs = base.clone();
                attrs.extend([
                    Attribute::List(vec![Attribute::EntityRef(tol)]),
                    form.to_ref(),
                ]);
                haecceity::p21::simple(kind, attrs)
            } else {
                haecceity::p21::simple(kind, base.clone())
            };
            let owner = edit.add(record);
            for (i, face) in free[..count].iter().enumerate() {
                let leaf = if count == 1 {
                    owner
                } else {
                    let leaf = edit.add(haecceity::p21::simple(
                        "SHAPE_ASPECT",
                        vec![
                            Attribute::String(format!("member {i}")),
                            Attribute::String("".into()),
                            Attribute::EntityRef(def.shape),
                            Attribute::Enum("T".into()),
                        ],
                    ));
                    edit.add(haecceity::p21::simple(
                        "SHAPE_ASPECT_RELATIONSHIP",
                        vec![
                            Attribute::String("".into()),
                            Attribute::String("".into()),
                            owner.to_ref(),
                            leaf.to_ref(),
                        ],
                    ));
                    leaf
                };
                edit.add(haecceity::p21::simple(
                    "GEOMETRIC_ITEM_SPECIFIC_USAGE",
                    vec![
                        Attribute::String("".into()),
                        Attribute::String("".into()),
                        leaf.to_ref(),
                        Attribute::EntityRef(rep),
                        Attribute::EntityRef(def.faces[*face]),
                    ],
                ));
            }
            let g = open_bytes(f.doc.apply(&edit).unwrap().bytes);
            let owner = g
                .doc
                .ids()
                .find(|id| {
                    test_attribute(&g.doc, *id, "shape_aspect", "name")
                        == Some(&Attribute::String("kept specialized scope".into()))
                })
                .unwrap();
            let before = geometry(&g.doc, owner);
            let add = PartPmi {
                features: vec![faces(&free[..count])],
                dimensions: vec![size(0, "10.", DimTolerance::Basic)],
                tolerances: vec![tolerance(
                    ToleranceKind::Flatness,
                    ToleranceTarget::Feature(FeatureId(0)),
                    "0.1",
                )],
                ..PartPmi::default()
            };
            let w = write(&g, &[(PartId(0), add)], Mode::Add)
                .unwrap_or_else(|e| panic!("{kind} {count}: {e}"));
            assert_eq!(
                geometry(&w.doc, owner),
                before,
                "{kind}: scope geometry changed"
            );
            assert_eq!(
                w.names(owner),
                g.doc
                    .get(owner)
                    .map(|e| match e {
                        RawEntity::Simple { name, .. } => vec![name.to_ascii_lowercase()],
                        RawEntity::Complex { parts, .. } =>
                            parts.iter().map(|p| p.name.to_ascii_lowercase()).collect(),
                    })
                    .unwrap()
            );
            for id in w.added() {
                let target = if is_a(&w.doc, id, "dimensional_size") {
                    reference(&w.doc, id, "dimensional_size", "applies_to")
                } else if is_a(&w.doc, id, "geometric_tolerance") {
                    reference(&w.doc, id, "geometric_tolerance", "toleranced_shape_aspect")
                } else {
                    None
                };
                if let Some(target) = target {
                    assert_ne!(target, owner, "{kind}");
                    assert!(neutral(&w.doc, target), "{kind}");
                    assert_eq!(geometry(&w.doc, target), before);
                }
            }
            assert!(
                violations(&w.doc).is_empty(),
                "{kind} {count}: {:?}",
                violations(&w.doc)
            );
        }
    }
}

#[test]
fn add_on_legacy_plate_prefers_datums_and_retires_forwarders() {
    use common::pmi_usage::{geometry, is_a, reference, violations};
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/xcaf/plate-legacy-datum-location.step.gz");
    let f = open(&path);
    let old = pmi::read(&f.doc, &f.parts).unwrap();
    let b = old.parts[0]
        .datums
        .iter()
        .find(|d| d.label().as_str() == "B")
        .unwrap()
        .feature()
        .unwrap();
    let feature = old.parts[0].features[b.0].clone();
    let conflict = PartPmi {
        features: vec![feature.clone()],
        datums: vec![datum("Z", 0)],
        ..PartPmi::default()
    };
    assert!(matches!(
        write(&f, &[(PartId(0), conflict)], Mode::Add),
        Err(WriteError::Refused(_))
    ));
    // A legacy carrier awaiting its datum assignment must be preferred to its
    // DIMENSION wrapper. Remove the old establishment first, respecting WR1.
    let mut edit = haecceity::p21::Edit::new();
    for id in [1107, 1108, 1121] {
        edit.remove(id);
    }
    edit.replace(
        1122,
        haecceity::p21::simple(
            "DATUM_SYSTEM",
            vec![
                Attribute::String("A".into()),
                Attribute::String("".into()),
                Attribute::EntityRef(26),
                Attribute::Enum("F".into()),
                Attribute::List(vec![Attribute::EntityRef(1120)]),
            ],
        ),
    );
    let f = open_bytes(f.doc.apply(&edit).unwrap().bytes);
    let p = PartPmi {
        features: vec![feature],
        datums: vec![datum("Z", 0)],
        dimensions: vec![size(0, "30.", DimTolerance::Basic)],
        tolerances: vec![tolerance(
            ToleranceKind::Flatness,
            ToleranceTarget::Feature(FeatureId(0)),
            "0.1",
        )],
        ..PartPmi::default()
    };
    let w = write(&f, &[(PartId(0), p)], Mode::Add).unwrap();
    assert!(is_a(&w.doc, 1100, "datum_feature"));
    assert!(
        w.doc.get(1109).is_none(),
        "never promote the old multi-member forwarder"
    );
    assert!(w.doc.get(1004).is_none());
    for loc in [1008, 1112] {
        let owner = reference(
            &w.doc,
            loc,
            "shape_aspect_relationship",
            "relating_shape_aspect",
        )
        .unwrap();
        assert_eq!(owner, if loc == 1008 { 1000 } else { 1100 });
    }
    for id in w
        .added()
        .into_iter()
        .filter(|id| is_a(&w.doc, *id, "dimensional_size"))
    {
        assert_eq!(
            reference(&w.doc, id, "dimensional_size", "applies_to"),
            Some(1100)
        );
    }
    assert_eq!(geometry(&w.doc, 1100), geometry(&f.doc, 1100));
    assert!(violations(&w.doc).is_empty(), "{:?}", violations(&w.doc));
}
