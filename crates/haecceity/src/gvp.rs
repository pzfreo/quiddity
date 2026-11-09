//! CAx-IF geometric validation properties, in mm, mm² and mm³.
//!
//! Values retain their STEP owner and geometry selection. A shape aspect is not silently
//! promoted to its whole product. Definition values use definition coordinates; occurrence
//! values use the occurrence's representation coordinates (do not transform them twice).
use crate::p21::{Attribute as A, Document, RawEntity};

#[derive(Clone, Debug, PartialEq)]
pub struct ValidationProperty {
    pub property: u64,
    pub representation: u64,
    /// The original property_definition.definition, including shape aspects.
    pub target: u64,
    pub product_definition: Option<u64>,
    /// A next_assembly_usage_occurrence, when the property belongs to an instance.
    pub occurrence: Option<u64>,
    /// Explicit geometric items selected by an aspect's shape representation or GISU.
    pub items: Vec<u64>,
    pub volume: Option<f64>,
    pub area: Option<f64>,
    /// Material centroid only; curve/surface/independent-point centroids are excluded.
    pub centroid: Option<[f64; 3]>,
}

fn attrs<'a>(doc: &'a Document, id: u64, kind: &str) -> Option<&'a [A]> {
    match doc.get(id)? {
        RawEntity::Simple {
            name, attributes, ..
        } if name.eq_ignore_ascii_case(kind) => Some(attributes),
        RawEntity::Complex { parts, .. } => parts
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(kind))
            .map(|p| p.attributes.as_slice()),
        _ => None,
    }
}
fn reference(a: &A) -> Option<u64> {
    if let A::EntityRef(id) = a {
        Some(*id)
    } else {
        None
    }
}
fn number(a: &A) -> Option<f64> {
    match a {
        A::Real(x) => Some(*x),
        A::Integer(x) => Some(*x as f64),
        A::Typed { value, .. } => number(value),
        _ => None,
    }
}
fn text(a: &A) -> Option<&str> {
    match a {
        A::String(s) | A::Enum(s) => Some(s),
        _ => None,
    }
}
fn refs(a: &A) -> Vec<u64> {
    match a {
        A::List(xs) => xs.iter().filter_map(reference).collect(),
        _ => reference(a).into_iter().collect(),
    }
}
fn prefix(a: &A) -> Option<f64> {
    if matches!(a, A::Unset) {
        return Some(1.0);
    }
    Some(10f64.powi(match text(a)?.to_ascii_uppercase().as_str() {
        "EXA" => 18,
        "PETA" => 15,
        "TERA" => 12,
        "GIGA" => 9,
        "MEGA" => 6,
        "KILO" => 3,
        "HECTO" => 2,
        "DECA" => 1,
        "DECI" => -1,
        "CENTI" => -2,
        "MILLI" => -3,
        "MICRO" => -6,
        "NANO" => -9,
        "PICO" => -12,
        "FEMTO" => -15,
        "ATTO" => -18,
        _ => return None,
    }))
}
/// Dimensional unit scale to mm^dimension. Recursion is bounded, including corrupt cycles.
fn unit(doc: &Document, id: u64, depth: usize) -> Option<(f64, i32)> {
    if depth >= 32 {
        return None;
    }
    for kind in ["DERIVED_UNIT", "AREA_UNIT", "VOLUME_UNIT"] {
        if let Some(a) = attrs(doc, id, kind)
            && let Some(A::List(elements)) = a.first()
        {
            let (mut scale, mut dimension) = (1.0, 0.0);
            for element in elements {
                let e = attrs(doc, reference(element)?, "DERIVED_UNIT_ELEMENT")?;
                let (s, d) = unit(doc, reference(e.first()?)?, depth + 1)?;
                let exponent = number(e.get(1)?)?;
                scale *= s.powf(exponent);
                dimension += d as f64 * exponent;
            }
            if dimension.fract() != 0.0 {
                return None;
            }
            return Some((scale, dimension as i32));
        }
    }
    let dimension = if attrs(doc, id, "VOLUME_UNIT").is_some() {
        3
    } else if attrs(doc, id, "AREA_UNIT").is_some() {
        2
    } else if attrs(doc, id, "LENGTH_UNIT").is_some() {
        1
    } else {
        return None;
    };
    if let Some(a) = attrs(doc, id, "CONVERSION_BASED_UNIT") {
        let m = reference(a.last()?)?;
        let e = attrs(doc, m, "MEASURE_WITH_UNIT")
            .or_else(|| attrs(doc, m, "LENGTH_MEASURE_WITH_UNIT"))
            .or_else(|| attrs(doc, m, "AREA_MEASURE_WITH_UNIT"))
            .or_else(|| attrs(doc, m, "VOLUME_MEASURE_WITH_UNIT"))?;
        let (base, d) = unit(doc, reference(e.get(1)?)?, depth + 1)?;
        return (d == dimension).then_some((number(e.first()?)? * base, d));
    }
    let a = attrs(doc, id, "SI_UNIT")?;
    let base = match text(a.last()?)?.to_ascii_uppercase().as_str() {
        "METRE" if dimension == 1 => 1000.0,
        "SQUARE_METRE" if dimension == 2 => 1e6,
        "CUBIC_METRE" if dimension == 3 => 1e9,
        _ => return None,
    };
    // An SI prefix modifies the named SI unit; a derived mm³ instead uses exponent 3.
    Some((prefix(a.first()?)? * base, dimension))
}
fn context_length(doc: &Document, context: u64) -> Option<f64> {
    let a = attrs(doc, context, "GLOBAL_UNIT_ASSIGNED_CONTEXT")?;
    refs(a.first()?)
        .iter()
        .find_map(|&id| unit(doc, id, 0).filter(|(_, d)| *d == 1).map(|(s, _)| s))
}
fn owner(doc: &Document, target: u64) -> (Option<u64>, Option<u64>) {
    let mut id = target;
    for _ in 0..32 {
        if let Some(a) =
            attrs(doc, id, "SHAPE_ASPECT").or_else(|| attrs(doc, id, "PRODUCT_DEFINITION_SHAPE"))
        {
            if let Some(next) = a.get(2).and_then(reference) {
                id = next;
                continue;
            }
        }
        if let Some(a) = attrs(doc, id, "NEXT_ASSEMBLY_USAGE_OCCURRENCE") {
            return (a.get(4).and_then(reference), Some(id));
        }
        if attrs(doc, id, "PRODUCT_DEFINITION").is_some()
            || attrs(doc, id, "PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS").is_some()
        {
            return (Some(id), None);
        }
        break;
    }
    (None, None)
}

/// Read all geometric (not PMI) validation property representations. Unsupported units on
/// a requested quantity are errors, never guessed from the geometry's global unit.
/// Non-material properties remain in the result with empty material quantities.
pub fn read(doc: &Document) -> Result<Vec<ValidationProperty>, String> {
    let mut out = Vec::new();
    for link in doc.ids() {
        let Some(a) = attrs(doc, link, "PROPERTY_DEFINITION_REPRESENTATION") else {
            continue;
        };
        let Some(property) = a.first().and_then(reference) else {
            continue;
        };
        let Some(p) = attrs(doc, property, "PROPERTY_DEFINITION") else {
            continue;
        };
        if !p
            .first()
            .and_then(text)
            .is_some_and(|s| s.eq_ignore_ascii_case("geometric validation property"))
        {
            continue;
        }
        let parse = || -> Option<ValidationProperty> {
            let target = reference(p.get(2)?)?;
            let representation = reference(a.get(1)?)?;
            let r = attrs(doc, representation, "REPRESENTATION")?;
            let context = reference(r.get(2)?)?;
            let (product_definition, occurrence) = owner(doc, target);
            let mut v = ValidationProperty {
                property,
                representation,
                target,
                product_definition,
                occurrence,
                items: Vec::new(),
                volume: None,
                area: None,
                centroid: None,
            };
            for id in refs(r.get(1)?) {
                if let Some(m) = attrs(doc, id, "MEASURE_REPRESENTATION_ITEM") {
                    let name = text(m.first()?)?.to_ascii_lowercase();
                    let dimension = match name.as_str() {
                        "volume measure" => 3,
                        "surface area measure" | "wetted area measure" => 2,
                        _ => continue,
                    };
                    let value = number(m.get(1)?)?;
                    let scale = if let Some(u) = m.get(2).and_then(reference) {
                        let (s, d) = unit(doc, u, 0)?;
                        if d != dimension {
                            return None;
                        }
                        s
                    } else {
                        context_length(doc, context)?.powi(dimension)
                    };
                    let value = value * scale;
                    if !value.is_finite() || value < 0.0 {
                        return None;
                    }
                    let slot = if dimension == 3 {
                        &mut v.volume
                    } else {
                        &mut v.area
                    };
                    if slot.replace(value).is_some() {
                        return None;
                    }
                }
                if let Some(m) = attrs(doc, id, "CARTESIAN_POINT")
                    && text(m.first()?).is_some_and(|s| {
                        matches!(
                            s.to_ascii_lowercase().as_str(),
                            "centre point" | "centroid" | "center point"
                        )
                    })
                {
                    let A::List(xyz) = m.get(1)? else { return None };
                    if xyz.len() != 3 {
                        return None;
                    }
                    let s = context_length(doc, context)?;
                    let point = [
                        number(&xyz[0])? * s,
                        number(&xyz[1])? * s,
                        number(&xyz[2])? * s,
                    ];
                    if !point.iter().all(|x| x.is_finite()) || v.centroid.replace(point).is_some() {
                        return None;
                    }
                }
            }
            if attrs(doc, target, "SHAPE_ASPECT").is_some() {
                for id in doc.ids() {
                    if let Some(s) = attrs(doc, id, "SHAPE_DEFINITION_REPRESENTATION")
                        && let Some(pd) = s.first().and_then(reference)
                        && attrs(doc, pd, "PROPERTY_DEFINITION")
                            .and_then(|a| a.get(2))
                            .and_then(reference)
                            == Some(target)
                        && let Some(rep) = s.get(1).and_then(reference)
                        && let Some(a) = attrs(doc, rep, "SHAPE_REPRESENTATION")
                    {
                        v.items.extend(refs(a.get(1)?));
                    }
                    if let Some(s) = attrs(doc, id, "GEOMETRIC_ITEM_SPECIFIC_USAGE")
                        && s.get(2).and_then(reference) == Some(target)
                    {
                        v.items.extend(refs(s.get(4)?));
                    }
                }
                v.items.sort_unstable();
                v.items.dedup();
            }
            Some(v)
        };
        out.push(parse().ok_or_else(|| {
            format!("GVP #{property}: malformed property or unsupported/ambiguous units")
        })?);
    }
    out.sort_by_key(|v| (v.property, v.representation));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn document(data: &str) -> Document {
        Document::parse(format!("ISO-10303-21;HEADER;FILE_DESCRIPTION((''),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{data}ENDSEC;END-ISO-10303-21;").into_bytes()).unwrap()
    }
    fn model(length: &str) -> Document {
        document(&format!(
            r#"
#1=PRODUCT_DEFINITION('','',#90,#91);
#2=PRODUCT_DEFINITION_SHAPE('','',#1);
#3=PROPERTY_DEFINITION('geometric validation property','',#2);
#4=PROPERTY_DEFINITION_REPRESENTATION(#3,#5);
#5=REPRESENTATION('',(#6,#7,#8,#9),#10);
#6=MEASURE_REPRESENTATION_ITEM('volume measure',VOLUME_MEASURE(2.),#13);
#7=MEASURE_REPRESENTATION_ITEM('surface area measure',AREA_MEASURE(3.),#14);
#8=CARTESIAN_POINT('centre point',(1.,2.,3.));
#9=CARTESIAN_POINT('curve centre point',(99.,99.,99.));
#10=(GLOBAL_UNIT_ASSIGNED_CONTEXT((#11))REPRESENTATION_CONTEXT('',''));
#11={length};
#12=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.));
#13=DERIVED_UNIT((#15)); #14=DERIVED_UNIT((#16));
#15=DERIVED_UNIT_ELEMENT(#11,3.); #16=DERIVED_UNIT_ELEMENT(#11,2.);
#17=MEASURE_WITH_UNIT(LENGTH_MEASURE(0.0254),#12);
"#
        ))
    }
    #[test]
    fn mm_m_and_inch_with_dimensioned_units_and_part_owner() {
        for (unit, scale) in [
            ("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))", 1f64),
            ("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.))", 1000.),
            (
                "(CONVERSION_BASED_UNIT('inch',#17)LENGTH_UNIT()NAMED_UNIT(*))",
                25.4,
            ),
        ] {
            let doc = model(unit);
            let v = read(&doc).unwrap();
            let p = &v[0];
            assert_eq!(p.product_definition, Some(1));
            assert_eq!(p.target, 2);
            assert!((p.volume.unwrap() - 2. * scale.powi(3)).abs() < 1e-6);
            assert!((p.area.unwrap() - 3. * scale.powi(2)).abs() < 1e-6);
            for i in 0..3 {
                assert!((p.centroid.unwrap()[i] - (i + 1) as f64 * scale).abs() < 1e-9);
            }
        }
    }
    #[test]
    fn context_units_and_explicit_measure_units_are_independent() {
        let doc = model("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
        let text = String::from_utf8(doc.bytes().to_vec())
            .unwrap()
            .replace("VOLUME_MEASURE(2.),#13", "VOLUME_MEASURE(2.),#18")
            .replace(
                "#17=",
                "#18=(NAMED_UNIT(*)SI_UNIT(.MILLI.,.CUBIC_METRE.)VOLUME_UNIT());#17=",
            );
        // milli-cubic-metre is 10^6 mm³, unlike a cubic millimetre.
        assert_eq!(
            read(&Document::parse(text.into_bytes()).unwrap()).unwrap()[0].volume,
            Some(2e6)
        );
        let text = String::from_utf8(doc.bytes().to_vec())
            .unwrap()
            .replace("VOLUME_MEASURE(2.),#13", "VOLUME_MEASURE(2.)");
        assert_eq!(
            read(&Document::parse(text.into_bytes()).unwrap()).unwrap()[0].volume,
            Some(2.)
        );
    }
    #[test]
    fn assembly_and_two_instances_keep_distinct_owners() {
        let doc = document(
            r#"
#1=PRODUCT_DEFINITION('assembly','',#90,#91); #2=PRODUCT_DEFINITION('part','',#92,#91);
#3=NEXT_ASSEMBLY_USAGE_OCCURRENCE('left','',$,#1,#2,$);
#4=NEXT_ASSEMBLY_USAGE_OCCURRENCE('right','',$,#1,#2,$);
#5=PRODUCT_DEFINITION_SHAPE('','',#1); #6=PRODUCT_DEFINITION_SHAPE('','',#3); #7=PRODUCT_DEFINITION_SHAPE('','',#4);
#8=PROPERTY_DEFINITION('geometric validation property','assembly',#5);
#9=PROPERTY_DEFINITION('geometric validation property','left',#6);
#10=PROPERTY_DEFINITION('geometric validation property','right',#7);
#11=PROPERTY_DEFINITION_REPRESENTATION(#8,#20); #12=PROPERTY_DEFINITION_REPRESENTATION(#9,#21); #13=PROPERTY_DEFINITION_REPRESENTATION(#10,#22);
#20=REPRESENTATION('centroid',(#23),#26); #21=REPRESENTATION('centroid',(#24),#26); #22=REPRESENTATION('centroid',(#25),#26);
#23=CARTESIAN_POINT('centre point',(0.,0.,0.)); #24=CARTESIAN_POINT('centre point',(-10.,0.,0.)); #25=CARTESIAN_POINT('centre point',(10.,0.,0.));
#26=(GLOBAL_UNIT_ASSIGNED_CONTEXT((#27))REPRESENTATION_CONTEXT('',''));
#27=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));
"#,
        );
        let v = read(&doc).unwrap();
        assert_eq!(v.len(), 3);
        assert_eq!((v[0].product_definition, v[0].occurrence), (Some(1), None));
        assert_eq!(
            (v[1].product_definition, v[1].occurrence),
            (Some(2), Some(3))
        );
        assert_eq!(
            (v[2].product_definition, v[2].occurrence),
            (Some(2), Some(4))
        );
        assert_eq!(v[1].centroid, Some([-10., 0., 0.]));
        assert_eq!(v[2].centroid, Some([10., 0., 0.]));
    }
    #[test]
    fn shape_aspect_selection_is_not_the_whole_part() {
        let doc = document(
            r#"
#1=PRODUCT_DEFINITION('part','',#90,#91); #2=PRODUCT_DEFINITION_SHAPE('','',#1);
#3=SHAPE_ASPECT('solid','',#2,.T.); #4=PROPERTY_DEFINITION('geometric validation property','',#3);
#5=PROPERTY_DEFINITION_REPRESENTATION(#4,#6); #6=REPRESENTATION('',(),#99);
#7=PROPERTY_DEFINITION('','',#3); #8=SHAPE_DEFINITION_REPRESENTATION(#7,#9);
#9=SHAPE_REPRESENTATION('',(#10),#99); #10=MANIFOLD_SOLID_BREP('',#11);
"#,
        );
        let p = read(&doc).unwrap().remove(0);
        assert_eq!(p.target, 3);
        assert_eq!(p.product_definition, Some(1));
        assert_eq!(p.items, vec![10]);
    }
    #[test]
    fn refuses_missing_units_cycles_and_wrong_dimensions() {
        let doc = model("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
        for (a, b) in [
            ("VOLUME_MEASURE(2.),#13", "VOLUME_MEASURE(2.),#14"),
            (
                "DERIVED_UNIT_ELEMENT(#11,3.)",
                "DERIVED_UNIT_ELEMENT(#13,3.)",
            ),
            (
                "GLOBAL_UNIT_ASSIGNED_CONTEXT((#11))",
                "GLOBAL_UNIT_ASSIGNED_CONTEXT(())",
            ),
        ] {
            let raw = String::from_utf8(doc.bytes().to_vec())
                .unwrap()
                .replace(a, b);
            assert!(read(&Document::parse(raw.into_bytes()).unwrap()).is_err());
        }
    }
}
