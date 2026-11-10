//! Synthetic geometry and product structure: no exporter or private file is used.
use haecceity::{
    express,
    p21::{Document, Edit},
    pmi::{
        self, Material, PartId, PartPmi,
        write::{Mode, PresentationPolicy, SchemaChange, UpgradeRepairKind, WriteError},
    },
    step::read_part_definitions,
};
use std::collections::BTreeMap;

#[derive(Default)]
struct Builder {
    records: Vec<String>,
}
impl Builder {
    fn add(&mut self, body: impl AsRef<str>) -> u64 {
        let id = self.records.len() as u64 + 1;
        self.records.push(format!("#{id}={};", body.as_ref()));
        id
    }
    fn document(&self, schema: &str) -> Document {
        Document::parse(
            format!(
                "ISO-10303-21;HEADER;FILE_DESCRIPTION(('synthetic tetrahedron'),'2;1');\n\
             FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('{schema}'));ENDSEC;DATA;\n{}\n\
             ENDSEC;END-ISO-10303-21;",
                self.records.join("\n")
            )
            .into_bytes(),
        )
        .unwrap()
    }
}
struct Fixture {
    builder: Builder,
    repairs: Vec<u64>,
    product: u64,
    optional: u64,
}
impl Fixture {
    fn new() -> Self {
        let mut b = Builder::default();
        let application = b.add("APPLICATION_CONTEXT('configuration controlled design')");
        let context = b.add(format!("PRODUCT_CONTEXT('',#{application},'mechanical')"));
        let product = b.add(format!(
            "PRODUCT('tetrahedron','tetrahedron',$,(#{context}))"
        ));
        let formation = b.add(format!("PRODUCT_DEFINITION_FORMATION('',$,#{product})"));
        let definition_context = b.add(format!(
            "PRODUCT_DEFINITION_CONTEXT('',#{application},'design')"
        ));
        let definition = b.add(format!(
            "PRODUCT_DEFINITION('design',$,#{formation},#{definition_context})"
        ));
        let shape = b.add(format!("PRODUCT_DEFINITION_SHAPE($,$,#{definition})"));
        let unit = b.add("(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.))");
        let geometric = b.add(format!("(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{unit})) REPRESENTATION_CONTEXT('',''))"));
        let coords: [[f64; 3]; 4] = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
        let mut points = Vec::new();
        let mut vertices = Vec::new();
        for [x, y, z] in coords {
            let point = b.add(format!("CARTESIAN_POINT('',({x:.1},{y:.1},{z:.1}))"));
            points.push(point);
            vertices.push(b.add(format!("VERTEX_POINT('',#{point})")));
        }
        let mut repairs = vec![shape];
        let mut edges = BTreeMap::new();
        for i in 0..4 {
            for j in i + 1..4 {
                let [x, y, z] = std::array::from_fn(|k| coords[j][k] - coords[i][k]);
                let direction = b.add(format!("DIRECTION('',({x:.1},{y:.1},{z:.1}))"));
                let vector = b.add(format!("VECTOR('',#{direction},1.0)"));
                let line = b.add(format!("LINE('',#{},#{vector})", points[i]));
                let name = if edges.is_empty() { "$" } else { "''" };
                let length = (x * x + y * y + z * z).sqrt();
                let curve = b.add(format!("TRIMMED_CURVE({name},#{line},(PARAMETER_VALUE(0.0)),(PARAMETER_VALUE({length:.16})),.T.,.PARAMETER.)"));
                if edges.is_empty() {
                    repairs.push(curve);
                }
                let edge = b.add(format!(
                    "EDGE_CURVE('',#{},#{},#{curve},.T.)",
                    vertices[i], vertices[j]
                ));
                edges.insert((i, j), edge);
            }
        }
        let mut faces = Vec::new();
        let mut optional = 0;
        // Outward cycles of the four faces, with shared edges.
        for [i, j, k] in [[0, 2, 1], [0, 1, 3], [1, 2, 3], [2, 0, 3]] {
            let mut oriented = Vec::new();
            for (a, c) in [(i, j), (j, k), (k, i)] {
                let (pair, sense) = if a < c { ((a, c), "T") } else { ((c, a), "F") };
                oriented.push(b.add(format!("ORIENTED_EDGE('',*,*,#{},.{sense}.)", edges[&pair])));
            }
            let loop_id = b.add(format!("EDGE_LOOP('',({}))", refs(&oriented)));
            let bound = b.add(format!("FACE_OUTER_BOUND('',#{loop_id},.T.)"));
            let u: [f64; 3] = std::array::from_fn(|n| coords[j][n] - coords[i][n]);
            let v: [f64; 3] = std::array::from_fn(|n| coords[k][n] - coords[i][n]);
            let normal = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let [x, y, z] = normal;
            let axis = b.add(format!("DIRECTION('',({x:.1},{y:.1},{z:.1}))"));
            let [x, y, z] = u;
            let along = b.add(format!("DIRECTION('',({x:.1},{y:.1},{z:.1}))"));
            let name = if faces.is_empty() { "$" } else { "''" };
            let placement = b.add(format!(
                "AXIS2_PLACEMENT_3D({name},#{},#{axis},#{along})",
                points[i]
            ));
            if faces.is_empty() {
                repairs.push(placement);
                optional = placement;
            }
            let plane = b.add(format!("PLANE('',#{placement})"));
            faces.push(b.add(format!("ADVANCED_FACE('',(#{bound}),#{plane},.T.)")));
        }
        let shell = b.add(format!("CLOSED_SHELL('',({}))", refs(&faces)));
        let solid = b.add(format!("MANIFOLD_SOLID_BREP('',#{shell})"));
        let representation = b.add(format!(
            "ADVANCED_BREP_SHAPE_REPRESENTATION('',(#{solid}),#{geometric})"
        ));
        b.add(format!(
            "SHAPE_DEFINITION_REPRESENTATION(#{shape},#{representation})"
        ));
        Self {
            builder: b,
            repairs,
            product,
            optional,
        }
    }
    fn doc(&self) -> Document {
        self.builder.document("CONFIG_CONTROL_DESIGN")
    }
}
fn refs(ids: &[u64]) -> String {
    ids.iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(",")
}
fn pmi() -> PartPmi {
    PartPmi {
        material: Some(Material {
            id: "synthetic material".into(),
            name: None,
            density: None,
        }),
        ..Default::default()
    }
}
fn write(doc: &Document, mode: Mode) -> Result<(Edit, pmi::write::WriteReport), WriteError> {
    write_pmi(doc, mode, pmi())
}
fn write_pmi(
    doc: &Document,
    mode: Mode,
    content: PartPmi,
) -> Result<(Edit, pmi::write::WriteReport), WriteError> {
    let parts = read_part_definitions(doc.bytes()).unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].faces.len(), 4);
    pmi::write::write(
        doc,
        &parts,
        &[(PartId(0), content)],
        mode,
        PresentationPolicy::RemovePresentation,
    )
}
fn record(doc: &Document, id: u64) -> &str {
    std::str::from_utf8(&doc.bytes()[doc.span(id).unwrap()]).unwrap()
}

#[test]
fn ap203_required_names_are_reported_and_pmi_reads_back() {
    let f = Fixture::new();
    let doc = f.doc();
    let original = express::TARGET.validate_document(doc.graph());
    assert_eq!(original.len(), 3, "{original:?}");
    assert_eq!(original.iter().map(|v| v.id).collect::<Vec<_>>(), f.repairs);
    let (edit, report) = write(&doc, Mode::Replace).unwrap();
    let after = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
    assert!(matches!(report.schema, SchemaChange::Upgraded { .. }));
    assert_eq!(after.file_schema().unwrap(), [express::TARGET_SCHEMA]);
    assert_eq!(report.repairs.len(), 3);
    for (repair, id) in report.repairs.iter().zip(&f.repairs) {
        assert_eq!(repair.id, *id);
        assert_eq!(repair.repair, UpgradeRepairKind::R4UnsetRequiredString);
        assert_eq!(repair.original_record, record(&doc, *id));
        assert_eq!(repair.new_record, record(&doc, *id).replacen('$', "''", 1));
        assert_eq!(record(&after, *id), repair.new_record);
    }
    for id in doc.ids().filter(|id| !f.repairs.contains(id)) {
        assert_eq!(record(&doc, id), record(&after, id), "#{id}");
    }
    assert!(express::TARGET.validate_document(after.graph()).is_empty());
    let parts = read_part_definitions(after.bytes()).unwrap();
    let read = pmi::read(&after, &parts).unwrap();
    assert_eq!(read.parts, vec![pmi()]);
}

#[test]
fn add_preservation_names_r4_and_unchanged_write_does_not_repair() {
    let f = Fixture::new();
    let doc = f.doc();
    let error = write(&doc, Mode::Add).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("R4 unset required string on entities")
    );
    let WriteError::AddPreservation {
        ids,
        repairs,
        edition_rewrites,
    } = error
    else {
        panic!()
    };
    assert_eq!(ids, f.repairs);
    assert_eq!(
        repairs,
        f.repairs
            .iter()
            .map(|id| (*id, UpgradeRepairKind::R4UnsetRequiredString))
            .collect::<Vec<_>>()
    );
    assert!(edition_rewrites.is_empty());
    let parts = read_part_definitions(doc.bytes()).unwrap();
    let (edit, report) = pmi::write::write(
        &doc,
        &parts,
        &[],
        Mode::Replace,
        PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    assert!(report.repairs.is_empty());
    assert_eq!(doc.apply(&edit).unwrap().bytes, doc.bytes());
}

#[test]
fn optional_unset_slots_are_preserved() {
    let mut f = Fixture::new();
    // Axis/ref_direction are OPTIONAL entity references. Property description and
    // product description are OPTIONAL text and already unset in the main fixture.
    let text = &mut f.builder.records[f.optional as usize - 1];
    let comma = text.find(',').unwrap();
    let point_end = text[comma + 1..].find(',').unwrap() + comma + 1;
    text.replace_range(point_end.., ",$,$);");
    let person = f.builder.add("PERSON('synthetic',$,$,$,$,$)");
    let doc = f.doc();
    let (edit, report) = write(&doc, Mode::Replace).unwrap();
    let after = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
    assert_eq!(report.repairs.len(), 3);
    assert_eq!(record(&doc, person), record(&after, person));
    assert_eq!(
        record(&after, f.optional),
        record(&doc, f.optional).replacen('$', "''", 1)
    );
    assert_eq!(record(&after, f.product), record(&doc, f.product));
    assert!(express::TARGET.validate_document(after.graph()).is_empty());
}

#[test]
fn non_string_and_other_violations_are_still_named() {
    let mut f = Fixture::new();
    let non_string = f.builder.add("VECTOR('',$,$)");
    let identifier = f.builder.add("PRODUCT($,'identity',$,())");
    let other = f.builder.add("DIRECTION('',(0.0,'wrong',1.0))");
    let doc = f.doc();
    let error = write(&doc, Mode::Replace).unwrap_err();
    let message = error.to_string();
    let WriteError::Edition { violations, .. } = error else {
        panic!("{message}")
    };
    for (id, attr) in [
        (non_string, "vector.orientation"),
        (non_string, "vector.magnitude"),
        (identifier, "product.id"),
        (other, "direction.direction_ratios"),
    ] {
        assert!(
            violations
                .iter()
                .any(|v| v.id == id && v.attribute.as_deref() == Some(attr)),
            "{violations:?}"
        );
        assert!(message.contains(&format!("#{id}")), "{message}");
    }
    assert!(
        violations.iter().all(|v| !f.repairs.contains(&v.id)),
        "{violations:?}"
    );
}

#[test]
fn ap242_in_its_own_edition_keeps_defects_as_preexisting_violations() {
    let f = Fixture::new();
    for schema in [express::AP242_ED1.file_schema, express::TARGET_SCHEMA] {
        for mode in [Mode::Add, Mode::Replace] {
            let doc = f.builder.document(schema);
            // A face datum needs no general Notes view or edition upgrade.
            let content = PartPmi {
                features: vec![pmi::Feature::Items(vec![pmi::Anchor::Face(
                    pmi::FaceIndex(0),
                )])],
                datums: vec![
                    pmi::Datum::new(
                        pmi::DatumLabel::new("Z").unwrap(),
                        Some(pmi::FeatureId(0)),
                        vec![],
                    )
                    .unwrap(),
                ],
                ..Default::default()
            };
            let (edit, report) = write_pmi(&doc, mode, content).unwrap();
            assert!(report.repairs.is_empty());
            assert_eq!(
                report
                    .preexisting_violations
                    .iter()
                    .map(|v| v.id)
                    .collect::<Vec<_>>(),
                f.repairs
            );
            let after = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
            assert_eq!(after.file_schema().unwrap(), [schema]);
            for id in doc.ids() {
                assert_eq!(record(&after, id), record(&doc, id));
            }
        }
    }
}

#[cfg(feature = "presentation")]
#[test]
fn ap242_edition_upgrade_repairs_r4_alongside_r3() {
    let mut f = Fixture::new();
    let integer = f.builder.add("INTEGER_REPRESENTATION_ITEM($,43.0)");
    let doc = f.builder.document(express::AP242_ED1.file_schema);
    // Material writes an eligible Notes view, which requires edition 4.
    let (edit, report) = write(&doc, Mode::Replace).unwrap();
    assert!(matches!(report.schema, SchemaChange::Upgraded { .. }));
    assert_eq!(
        report
            .repairs
            .iter()
            .map(|r| (r.id, r.repair))
            .collect::<Vec<_>>(),
        f.repairs
            .iter()
            .map(|id| (*id, UpgradeRepairKind::R4UnsetRequiredString))
            .chain([
                (integer, UpgradeRepairKind::R3IntegerLiteral),
                (integer, UpgradeRepairKind::R4UnsetRequiredString)
            ])
            .collect::<Vec<_>>()
    );
    let after = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
    assert_eq!(after.file_schema().unwrap(), [express::TARGET_SCHEMA]);
    assert_eq!(
        record(&after, integer),
        format!("#{integer}=INTEGER_REPRESENTATION_ITEM('',43);")
    );
    assert!(express::TARGET.validate_document(after.graph()).is_empty());
    for repair in &report.repairs {
        assert_eq!(repair.original_record, record(&doc, repair.id));
        assert_eq!(repair.new_record, record(&after, repair.id));
    }
}

#[test]
fn required_text_defined_aliases_complex_slots_and_exact_decimals() {
    let mut f = Fixture::new();
    let text = f.builder.add("DESCRIPTIVE_REPRESENTATION_ITEM($,$)");
    // Neither a number nor a SELECT is defaulted. A label reached via a
    // defined alias is repaired even when its attribute isn't called name.
    let font = f.builder.add("PRE_DEFINED_TEXT_FONT('synthetic')");
    let alias = f.builder.add(format!(
        "TEXT_LITERAL('','literal',#{},$,.RIGHT.,#{font})",
        f.optional
    ));
    let decimal = f
        .builder
        .add("CARTESIAN_POINT($,(0.123456789012345678901,0.0,0.E0))");
    let dimensions = f
        .builder
        .add("DIMENSIONAL_EXPONENTS(1.0,0.0,0.0,0.0,0.0,0.0,0.0)");
    let unit = f
        .builder
        .add("(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.))");
    let factor = f.builder.add(format!(
        "LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#{unit})"
    ));
    let complex = f.builder.add(format!(
        "(CONVERSION_BASED_UNIT($,#{factor}) LENGTH_UNIT() NAMED_UNIT(*))"
    ));
    let doc = f.doc();
    let (edit, report) = write(&doc, Mode::Replace).unwrap();
    let after = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
    let expected = f
        .repairs
        .iter()
        .map(|id| (*id, UpgradeRepairKind::R4UnsetRequiredString))
        .chain([text, alias, decimal].map(|id| (id, UpgradeRepairKind::R4UnsetRequiredString)))
        .chain([
            (complex, UpgradeRepairKind::R2ConversionUnitDimensions),
            (complex, UpgradeRepairKind::R4UnsetRequiredString),
        ])
        .collect::<Vec<_>>();
    assert_eq!(
        report
            .repairs
            .iter()
            .map(|r| (r.id, r.repair))
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        record(&after, text),
        format!("#{text}=DESCRIPTIVE_REPRESENTATION_ITEM('','');")
    );
    assert_eq!(
        record(&after, alias),
        record(&doc, alias).replace('$', "''")
    );
    assert_eq!(
        record(&after, decimal),
        record(&doc, decimal).replace('$', "''")
    );
    assert_eq!(
        record(&after, complex),
        format!(
            "#{complex}=(CONVERSION_BASED_UNIT('',#{factor})LENGTH_UNIT()NAMED_UNIT(#{dimensions}));"
        )
    );
    for repair in &report.repairs {
        assert_eq!(repair.original_record, record(&doc, repair.id));
        assert_eq!(repair.new_record, record(&after, repair.id));
    }
    assert!(express::TARGET.validate_document(after.graph()).is_empty());
}

#[test]
fn ap214_keyword_rewrite_and_r4_share_one_record_edit() {
    let mut f = Fixture::new();
    let category = f.builder.add(format!("PRODUCT_TYPE($,$,(#{}))", f.product));
    let doc = f.builder.document("AUTOMOTIVE_DESIGN");
    let (edit, report) = write(&doc, Mode::Replace).unwrap();
    let after = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
    assert_eq!(report.edition_rewrites.len(), 1);
    assert_eq!(report.edition_rewrites[0].id, category);
    let repair = report.repairs.last().unwrap();
    assert_eq!(repair.id, category);
    assert_eq!(repair.repair, UpgradeRepairKind::R4UnsetRequiredString);
    assert_eq!(repair.original_record, record(&doc, category));
    assert_eq!(
        repair.new_record,
        format!(
            "#{category}=PRODUCT_RELATED_PRODUCT_CATEGORY('',$,(#{}));",
            f.product
        )
    );
    assert_eq!(record(&after, category), repair.new_record);
    assert!(express::TARGET.validate_document(after.graph()).is_empty());
}

#[test]
fn derived_slots_and_other_string_types_are_not_defaulted() {
    let mut f = Fixture::new();
    let font = f.builder.add("PRE_DEFINED_TEXT_FONT('synthetic')");
    let string = f.builder.add(format!(
        "TEXT_LITERAL('',$,#{},'',.RIGHT.,#{font})",
        f.optional
    ));
    let derived = f.builder.add(format!(
        "CHARACTERIZED_REPRESENTATION('',(#{}),#9,$,$)",
        f.optional
    ));
    let doc = f.doc();
    let WriteError::Edition { violations, .. } = write(&doc, Mode::Replace).unwrap_err() else {
        panic!()
    };
    assert_eq!(
        violations
            .iter()
            .map(|v| (v.id, v.attribute.as_deref(), v.kind))
            .collect::<Vec<_>>(),
        vec![
            (string, Some("text_literal.literal"), express::Kind::Unset),
            (
                derived,
                Some("characterized_object.name"),
                express::Kind::Derived
            ),
            (
                derived,
                Some("characterized_object.description"),
                express::Kind::Derived
            ),
        ]
    );
}
