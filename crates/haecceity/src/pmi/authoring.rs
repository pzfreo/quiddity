//! Geometry-independent writer refusals, shared with JSON authoring validation.
use super::*;

pub(crate) fn model_refusals(id: PartId, p: &PartPmi) -> Vec<Refusal> {
    struct Check {
        id: PartId,
        refusals: Vec<Refusal>,
    }
    impl Check {
        fn refuse(&mut self, item: ItemRef, why: impl Into<String>) {
            self.refusals.push(Refusal {
                part: self.id,
                item,
                why: why.into(),
            });
        }
    }
    let mut check = Check {
        id,
        refusals: Vec::new(),
    };
    // Values: every decimal must be a Part 21 REAL.
    let bad_value =
        |vals: &[&Decimal]| -> Option<String> { vals.iter().find_map(|d| real(d).err()) };

    for (i, s) in p.standards.iter().enumerate() {
        let key = s.edition.clone().unwrap_or_else(|| s.document.clone());
        let body = if key.starts_with("ASME") || s.document.starts_with("ASME") {
            StandardBody::Asme
        } else if key.starts_with("ISO") || s.document.starts_with("ISO") {
            StandardBody::Iso
        } else {
            StandardBody::Other
        };
        if body != s.body || s.document.trim().is_empty() || s.document.trim() != s.document {
            check.refuse(
                    ItemRef::Standard(i),
                    format!(
                        "standard {:?} ({:?}): the body is what its document or edition names (§4), and the document is a non-empty name",
                        s.document, s.body
                    ),
                );
        }
    }
    let table_places: Vec<_> = p
        .general
        .iter()
        .filter_map(|g| match g {
            GeneralTolerance::Table { cells, .. } => Some(cells),
            _ => None,
        })
        .flatten()
        .flat_map(|c| &c.items)
        .filter_map(|(name, value)| match value {
            CellValue::Count(d) if name == "number of decimal places" => Some(d.to_f64()),
            _ => None,
        })
        .collect();
    if !table_places.is_empty()
        && p.decimal_places
            .is_none_or(|places| table_places.iter().any(|v| *v != f64::from(places)))
    {
        check.refuse(
            ItemRef::DecimalPlaces,
            "the part's decimal places disagree with its stated table cells",
        );
    }
    for (i, t) in p.datum_targets.iter().enumerate() {
        let mut values = Vec::new();
        match t.shape() {
            TargetShape::Line { length } => values.push(&length.value),
            TargetShape::Rectangle { length, width } => {
                values.extend([&length.value, &width.value])
            }
            TargetShape::Circle { diameter } | TargetShape::CircularCurve { diameter } => {
                values.push(&diameter.value)
            }
            _ => {}
        }
        if values.iter().any(|v| v.to_f64() <= 0.) {
            check.refuse(ItemRef::DatumTarget(i), "target sizes must be positive");
        }
        if let Some(placement) = t.placement() {
            values.extend(placement.origin.iter().map(|l| &l.value));
            for d in placement.axis.iter().chain(&placement.ref_direction) {
                values.extend(d.0.iter());
            }
        }
        if let Some(d) = t.movable() {
            values.extend(d.0.iter());
            if matches!(
                t.shape(),
                TargetShape::Point
                    | TargetShape::Rectangle { .. }
                    | TargetShape::Circle { .. }
                    | TargetShape::CircularCurve { .. }
            ) {
                check.refuse(ItemRef::DatumTarget(i), "movable direction adds an item forbidden by placed_datum_target_feature.WR3; this rule is not exempted");
            }
        }
        if let Some(why) = bad_value(&values) {
            check.refuse(ItemRef::DatumTarget(i), why);
        }
        if let TargetShape::Area(f) | TargetShape::Curve(f) = t.shape()
            && !explicit_target_geometry(p, *f, matches!(t.shape(), TargetShape::Area(_)))
        {
            check.refuse(ItemRef::DatumTarget(i), "explicit area/curve target requires stated face/curve geometry (§6.6.2); a different anchor kind cannot define that target");
        }
        if !p
            .datums
            .iter()
            .any(|d| d.targets().contains(&DatumTargetId(i)))
        {
            check.refuse(
                ItemRef::DatumTarget(i),
                "datum_target.WR1: target establishes no datum",
            );
        }
    }
    for i in 0..p.holes.len() {
        check.refuse(
                ItemRef::Hole(i),
                "hole definitions (basic_round_hole) are read but not written yet (design: question Q-holes)",
            );
    }
    for (i, g) in p.general.iter().enumerate() {
        match g {
            GeneralTolerance::Table { cells, .. } => {
                if cells.is_empty() {
                    check.refuse(
                        ItemRef::General(i),
                        "a default_tolerance_table requires at least one cell",
                    );
                }
                for c in cells {
                    let measure = |name: &str| {
                        c.items
                            .iter()
                            .filter(|(n, v)| n == name && !matches!(v, CellValue::Text(_)))
                            .count()
                            == 1
                    };
                    let texts: Vec<_> = c
                        .items
                        .iter()
                        .filter(|(_, v)| matches!(v, CellValue::Text(_)))
                        .collect();
                    if c.items.len() > 5
                        || (c.decimal_default().is_none()
                            && (!(measure("significant number of digits")
                                || (measure("lower limit") && measure("upper limit")))
                                || !(measure("plus minus tolerance value")
                                    || (measure("lower tolerance value")
                                        && measure("upper tolerance value")))))
                        || texts.len() > 1
                        || texts.iter().any(|(n, _)| n != "cell description")
                    {
                        check.refuse(ItemRef::General(i), "table cell violates default_tolerance_table_cell WR2–WR5: at most five items, stated range/digits and tolerance, at most one 'cell description'");
                    }
                    for (_, v) in &c.items {
                        let d = match v {
                            CellValue::Count(d) if count_integer(d).is_some() => None,
                            CellValue::Count(d) => Some(d),
                            CellValue::Value(v) => Some(v.decimal()),
                            _ => None,
                        };
                        if let Some(why) = d.and_then(|d| real(d).err()) {
                            check.refuse(ItemRef::General(i), why);
                        }
                    }
                }
            }
            GeneralTolerance::Class { text, standard } => {
                if Iso2768::recognise(text) != *standard || text.trim().is_empty() {
                    check.refuse(ItemRef::General(i), format!("tolerance class {text:?}: its ISO 2768 classes are those its text states"));
                }
            }
        }
    }
    if let Some(m) = &p.material {
        if let Some(d) = &m.density {
            let mut mass = 0.;
            let mut length = 0.;
            for (name, exponent) in &d.unit {
                match density_unit(name) {
                    Some(DensityUnit::Mass(_)) => mass += exponent.to_f64(),
                    Some(DensityUnit::Length(_)) => length += exponent.to_f64(),
                    None => check.refuse(
                        ItemRef::Material,
                        format!("density unit {name:?} has no stated mass or length scale"),
                    ),
                }
                if let Err(why) = real(exponent) {
                    check.refuse(ItemRef::Material, why);
                }
            }
            if mass != 1. || length != -3. {
                check.refuse(
                    ItemRef::Material,
                    "density must have dimensional exponents mass 1, length -3 (ISO 10303-41)",
                );
            }
            if let Err(why) = real(&d.value) {
                check.refuse(ItemRef::Material, why);
            }
            if d.value.to_f64() <= 0. {
                check.refuse(ItemRef::Material, "density must be positive");
            }
        }
        if m.id.is_empty() {
            check.refuse(ItemRef::Material, "material without its identification");
        }
    }

    // Dimensions.
    for (i, d) in p.dimensions.iter().enumerate() {
        let mut vals: Vec<&Decimal> = d.nominal.iter().map(Value::decimal).collect();
        match &d.tolerance {
            DimTolerance::Deviations(b) | DimTolerance::Limits(b) => {
                vals.extend([b.upper().decimal(), b.lower().decimal()]);
            }
            DimTolerance::Fit {
                limits: Some(b), ..
            } => {
                vals.extend([b.upper().decimal(), b.lower().decimal()]);
            }
            _ => {}
        }
        if let Some(why) = bad_value(&vals) {
            check.refuse(ItemRef::Dimension(i), why);
        }
    }

    // Tolerances.
    let mut by_content = BTreeMap::new();
    let canonical: Vec<_> = p
        .features
        .iter()
        .enumerate()
        .map(|(i, feature)| {
            if let Feature::Items(items) = feature {
                let mut key = items.clone();
                key.sort();
                key.dedup();
                *by_content.entry(key).or_insert(i)
            } else {
                i
            }
        })
        .collect();
    let mut plane_kinds = BTreeMap::new();
    for (i, t) in p.tolerances.iter().enumerate() {
        if let Some(z) = &t.zone {
            if z.affected_plane.is_none() && z.affected_plane_kind.is_some() {
                check.refuse(
                    ItemRef::Tolerance(i),
                    "affected plane kind has no affected plane feature",
                );
            }
            if let Some(f) = z.affected_plane {
                let valid = match (&p.features[f.0], z.affected_plane_kind) {
                    (Feature::Items(items), Some(kind)) if items.len() == 1 => {
                        matches!(items[0], Anchor::Geometry(g) if matches!(
                            (p.geometry[g.0].item.entity().as_str(), kind),
                            ("polyline", AffectedPlaneKind::IntersectionLine) |
                            ("axis2_placement_3d", AffectedPlaneKind::IntersectionPlane | AffectedPlaneKind::OrientationPlane)
                        ))
                    }
                    _ => false,
                };
                if !valid {
                    check.refuse(ItemRef::Tolerance(i), "affected plane requires an explicit intersection line, intersection plane or orientation plane kind and its matching polyline/axis2_placement_3d geometry (§6.9.2.1)");
                }
                if let Some(old) = plane_kinds.insert(canonical[f.0], z.affected_plane_kind)
                    && old != z.affected_plane_kind
                {
                    check.refuse(ItemRef::Tolerance(i), "one affected plane feature has conflicting variants; use separate features with distinct geometry");
                }
            }
        }
    }
    for (i, t) in p.tolerances.iter().enumerate() {
        if let Some(u) = &t.unit_basis {
            let lengths = std::iter::once(&u.size)
                .chain(u.area.as_ref().and_then(|a| a.second.as_ref()))
                .all(|v| matches!(v.quantity, Quantity::Length(_)));
            if !lengths {
                check.refuse(
                        ItemRef::Tolerance(i),
                        "a unit basis that is not a length (geometric_tolerance_with_defined_unit.unit_size is a length_measure_with_unit)",
                    );
            }
        }
        let mut vals: Vec<&Decimal> = Vec::new();
        vals.extend(t.magnitude.iter().map(Value::decimal));
        vals.extend(t.maximum.iter().map(Value::decimal));
        vals.extend(t.unequal.iter().map(Value::decimal));
        if let Some(z) = &t.zone {
            vals.extend(z.projected.iter().map(|p| p.length.decimal()));
            vals.extend(z.runout_angle.iter().map(Value::decimal));
        }
        if let Some(why) = bad_value(&vals) {
            check.refuse(ItemRef::Tolerance(i), why);
        }
    }

    // Threads and knurls: their parameter counts (thread WR1, turned_knurl WR2), refused
    // here per item, before emission; express_rules checks the written instances.
    for (i, t) in p.threads.iter().enumerate() {
        if t.partial_area.is_none() {
            check.refuse(
                ItemRef::Thread(i),
                "a thread needs its 'partial area occurrence' feature (thread WR12: exactly one)",
            );
        }
        let n = 6
            + usize::from(t.minor_diameter.is_some())
            + usize::from(t.pitch_diameter.is_some())
            + usize::from(t.fit_class_2.is_some())
            + usize::from(t.crest.is_some())
            + usize::from(t.qualifier.is_some())
            + usize::from(t.nominal_size.is_some());
        if !(8..=11).contains(&n) {
            check.refuse(
                ItemRef::Thread(i),
                format!("a thread with {n} parameters (thread WR1: 8 to 11)"),
            );
        }
        let mut vals = vec![&t.major_diameter.value, &t.number_of_threads.0];
        for l in [
            &t.minor_diameter,
            &t.pitch_diameter,
            &t.crest,
            &t.nominal_size,
        ]
        .into_iter()
        .flatten()
        {
            vals.push(&l.value);
        }
        if let Some(why) = bad_value(&vals) {
            check.refuse(ItemRef::Thread(i), why);
        }
    }
    for (i, k) in p.knurls.iter().enumerate() {
        let n = 3
            + usize::from(k.number_of_teeth.is_some())
            + usize::from(k.tooth_depth.is_some())
            + usize::from(k.root_fillet.is_some())
            + usize::from(k.helix_angle.is_some())
            + usize::from(k.helix_hand.is_some());
        if !(6..=9).contains(&n) {
            check.refuse(
                ItemRef::Knurl(i),
                format!("a knurl with {n} parameters (turned_knurl WR2: 6 to 9)"),
            );
        }
        let mut vals = vec![
            &k.major_diameter.value,
            &k.nominal_diameter.value,
            &k.diametral_pitch.value,
        ];
        for l in [&k.tooth_depth, &k.root_fillet].into_iter().flatten() {
            vals.push(&l.value);
        }
        vals.extend(k.helix_angle.iter().map(|a| &a.value));
        if let Some(why) = bad_value(&vals) {
            check.refuse(ItemRef::Knurl(i), why);
        }
    }

    for (i, a) in p.attributes.iter().enumerate() {
        for (key, v) in &a.items {
            let why = match v {
                    AttributeValue::Measure(m) if m.decimal_places.is_some() => Some(
                        "decimal places of an attribute value are not held by the UDA form"
                            .to_string(),
                    ),
                    AttributeValue::Measure(m) => real(m.decimal()).err(),
                    AttributeValue::Real(d) => real(d).err(),
                    // The reader looks for the value of a boolean_representation_item as its
                    // own attribute (it is boolean_literal's), so a written boolean would not
                    // read back.
                    AttributeValue::Boolean(_) => Some(
                        "a boolean value is not written: the reader does not read boolean_representation_item.the_value back".to_string(),
                    ),
                    AttributeValue::OtherMeasure { value, .. } => {
                        real(value).err()
                    }
                    _ => None,
                };
            if let Some(why) = why {
                check.refuse(ItemRef::Attribute(i), format!("{key:?}: {why}"));
            }
        }
    }
    for (i, texture) in p.surface_textures.iter().enumerate() {
        let values: Vec<_> = texture.parameters.iter().map(|x| &x.value.value).collect();
        if let Some(why) = bad_value(&values) {
            check.refuse(ItemRef::SurfaceTexture(i), why);
        }
    }
    for (i, feature) in p.features.iter().enumerate() {
        if matches!(
            feature,
            Feature::Group {
                kind: GroupKind::Unstated,
                ..
            }
        ) {
            check.refuse(ItemRef::Feature(i), "a group of unstated kind (§6.4 requires 'multiple elements' or 'pattern of features')");
        }
    }
    check.refusals
}

fn explicit_target_geometry(p: &PartPmi, f: FeatureId, area: bool) -> bool {
    match &p.features[f.0] {
        Feature::Items(items) => {
            !items.is_empty()
                && items.iter().all(|a| match a {
                    Anchor::Face(_) => area,
                    Anchor::Edge(_) => false,
                    Anchor::Geometry(g) => p.geometry[g.0].item.leaves.iter().any(|(entity, _)| {
                        express::TARGET.is_a(
                            &entity.to_ascii_lowercase(),
                            if area { "face" } else { "curve" },
                        )
                    }),
                })
        }
        Feature::Group { members, .. } => {
            !members.is_empty()
                && members
                    .iter()
                    .all(|f| explicit_target_geometry(p, *f, area))
        }
        Feature::Derived { .. } => false,
    }
}
