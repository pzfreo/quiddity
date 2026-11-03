//! Closed, reported repairs for an edition upgrade gaining PMI. No general cleanup.

use super::{EditionRewrite, UpgradeRepairKind, WriteError, attr, attr_ref, is_a, leaf_names};
use crate::express::{self, Kind, TypeDef};
use crate::p21::{self, Attribute as A, Complex, Document, Edit, RawEntity};
use crate::pmi::Decimal;
use std::collections::BTreeSet;

type Dimensions = [f64; 7];
type Repairs = Vec<(u64, UpgradeRepairKind)>;

// Deliberate exceptions, never a general walk up the subtype graph. PRODUCT_TYPE has no
// explicit attributes of its own (OCCT's AP214 CD StepBasic_ProductType). Both carried
// AP242 EXPRESS tables (N8324 and N11521) give PRODUCT_RELATED_PRODUCT_CATEGORY exactly
// the inherited name: label, description: OPTIONAL text, products: SET [1:?] OF product.
// The schema comparison and source references are recorded in docs/step-ap242.md.
const AP214_SUBTYPES: &[(&str, &str)] = &[("PRODUCT_TYPE", "PRODUCT_RELATED_PRODUCT_CATEGORY")];

pub(super) fn rewrite_entities(
    doc: &Document,
    edit: &mut Edit,
    excluded: &BTreeSet<u64>,
) -> Vec<EditionRewrite> {
    let mut rewrites = Vec::new();
    for id in doc.ids().filter(|id| !excluded.contains(id)) {
        let Some(RawEntity::Simple { name, .. }) = doc.get(id) else {
            continue;
        };
        if let Some(&(from, to)) = AP214_SUBTYPES
            .iter()
            .find(|(from, _)| name.eq_ignore_ascii_case(from))
        {
            edit.rename_entity(id, to);
            rewrites.push(EditionRewrite { id, from, to });
        }
    }
    rewrites
}

/// Exact record text, never a lossy decoding of original bytes.
pub(super) fn record_text(doc: &Document, id: u64) -> Result<String, WriteError> {
    String::from_utf8(doc.bytes()[doc.span(id).ok_or(p21::P21Error::Missing(id))?].to_vec())
        .map_err(|_| {
            WriteError::P21(p21::P21Error::Encode(format!(
                "repair #{id}: original record is not UTF-8; cannot report its exact text"
            )))
        })
}

pub(super) fn repair(
    doc: &Document,
    edit: &mut Edit,
    excluded: &BTreeSet<u64>,
    emitted_dimensions: Vec<(Dimensions, A)>,
    repair_integer_literals: bool,
) -> Result<(Repairs, usize), WriteError> {
    let mut repairs = Vec::new();
    let mut added = 0;
    // Ascending ids choose the first equal existing dimensions deterministically. Provisional
    // references also let every repaired unit share one addition per distinct exponent set.
    let mut dimensions: Vec<(Dimensions, A)> = doc
        .ids()
        .filter(|id| !excluded.contains(id))
        .filter_map(|id| exponents(doc, id).map(|d| (d, A::EntityRef(id))))
        .collect();
    dimensions.extend(emitted_dimensions);
    for id in doc.ids().filter(|id| !excluded.contains(id)) {
        let Some(record) = doc.get(id) else {
            continue;
        };
        match record {
            RawEntity::Simple {
                name, attributes, ..
            } if repair_integer_literals
                && name.eq_ignore_ascii_case("integer_representation_item") =>
            {
                if let Some(A::Real(value)) = attr(doc, id, "literal_number", "the_value")
                    && value.is_finite()
                    && value.fract() == 0.
                    && value.abs() <= (1u64 << 53) as f64
                {
                    // The parser's f64 can round a fractional or out-of-range source
                    // token to an integer. Check the exact stated decimal as well.
                    let tokens = super::super::read::real_tokens(
                        &doc.bytes()[doc.span(id).ok_or(p21::P21Error::Missing(id))?],
                    );
                    let [text] = tokens.as_slice() else {
                        continue;
                    };
                    let stated = Decimal::parse(text).map_err(|e| WriteError::Internal(e.0))?;
                    let integer = *value as i64;
                    let exact = Decimal::parse(&integer.to_string())
                        .map_err(|e| WriteError::Internal(e.0))?;
                    if !stated.cmp_value(&exact).is_eq() {
                        continue;
                    }
                    let mut attributes = attributes.clone();
                    let slots = express::TARGET.explicit_attributes(name).ok_or_else(|| {
                        WriteError::Internal("integer_representation_item attributes absent".into())
                    })?;
                    if let Some(i) = slots
                        .iter()
                        .position(|a| a.entity == "literal_number" && a.name == "the_value")
                    {
                        attributes[i] = A::Integer(integer);
                        record_text(doc, id)?;
                        edit.replace(id, p21::simple(name, attributes));
                        repairs.push((id, UpgradeRepairKind::R3IntegerLiteral));
                    }
                }
            }
            RawEntity::Simple {
                name, attributes, ..
            } if name.eq_ignore_ascii_case("cc_design_approval") => {
                let Some(A::List(items)) = attr(doc, id, "cc_design_approval", "items") else {
                    continue; // The unchanged schema validator refuses malformed attributes.
                };
                let member = |item: &A, select| match item {
                    A::EntityRef(target) => in_select(doc, *target, select),
                    _ => false,
                };
                if items.iter().any(|a| !member(a, "approved_item"))
                    && items.iter().all(|a| member(a, "approval_item"))
                {
                    record_text(doc, id)?;
                    edit.replace(
                        id,
                        p21::simple("APPLIED_APPROVAL_ASSIGNMENT", attributes.clone()),
                    );
                    repairs.push((id, UpgradeRepairKind::R1ApprovalAssignment));
                }
            }
            RawEntity::Complex { parts, .. }
                if is_a(doc, id, "conversion_based_unit")
                    && matches!(attr(doc, id, "named_unit", "dimensions"), Some(A::Derived)) =>
            {
                let Some(d) = unit_dimensions(doc, id, &mut BTreeSet::new()) else {
                    let mut violations = express::TARGET.validate_document(doc.graph());
                    // Keep the schema refusal and its offending id, with the failed resolution
                    // named as well (even when the original has other upgrade violations).
                    for v in &mut violations {
                        if v.id == id && v.kind == Kind::Derived {
                            v.detail.push_str(
                                "; R2 cannot determine conversion factor unit dimensions",
                            );
                        }
                    }
                    return Err(WriteError::Edition {
                        file_schema: doc.file_schema()?.first().cloned().unwrap_or_default(),
                        violations,
                    });
                };
                record_text(doc, id)?;
                let reference = match dimensions.iter().find(|(existing, _)| *existing == d) {
                    Some((_, reference)) => reference.clone(),
                    None => {
                        let reference = edit
                            .add(p21::simple(
                                "DIMENSIONAL_EXPONENTS",
                                d.into_iter().map(A::Real).collect(),
                            ))
                            .to_ref();
                        dimensions.push((d, reference.clone()));
                        added += 1;
                        reference
                    }
                };
                let mut leaves = parts.clone();
                let named = leaves
                    .iter_mut()
                    .find(|p| p.name.eq_ignore_ascii_case("named_unit"))
                    .ok_or_else(|| {
                        WriteError::P21(p21::P21Error::Encode(format!(
                            "#{id} has no named_unit leaf for its dimensions"
                        )))
                    })?;
                named.attributes[0] = reference;
                edit.replace(id, Complex::new(leaves)?);
                repairs.push((id, UpgradeRepairKind::R2ConversionUnitDimensions));
            }
            _ => {}
        }
    }
    Ok((repairs, added))
}

fn in_select(doc: &Document, id: u64, select: &str) -> bool {
    let Some(TypeDef::Select(members)) = express::TARGET.type_decl(select).map(|t| &t.def) else {
        return false;
    };
    // These two selects contain entity types, checked through the target's supertype table.
    leaf_names(doc, id)
        .iter()
        .any(|n| members.iter().any(|m| express::TARGET.is_a(n, m)))
}

fn number(a: &A) -> Option<f64> {
    match a {
        A::Real(n) if n.is_finite() => Some(*n),
        A::Integer(n) => Some(*n as f64),
        _ => None,
    }
}

fn exponents(doc: &Document, id: u64) -> Option<Dimensions> {
    let RawEntity::Simple {
        name, attributes, ..
    } = doc.get(id)?
    else {
        return None;
    };
    if !name.eq_ignore_ascii_case("dimensional_exponents") || attributes.len() != 7 {
        return None;
    }
    let mut d = [0.; 7];
    for (out, a) in d.iter_mut().zip(attributes) {
        *out = number(a)?;
    }
    Some(d)
}

fn unit_dimensions(doc: &Document, id: u64, active: &mut BTreeSet<u64>) -> Option<Dimensions> {
    // Cycles and excessive depth are indeterminate, never guessed from LENGTH_UNIT etc.
    if active.len() >= 128 || !active.insert(id) {
        return None;
    }
    let result = (|| {
        if is_a(doc, id, "si_unit") {
            let A::Enum(name) = attr(doc, id, "si_unit", "name")? else {
                return None;
            };
            return dimensions_for_si_unit(name);
        }
        if is_a(doc, id, "conversion_based_unit") {
            let factor = attr_ref(doc, id, "conversion_based_unit", "conversion_factor")?;
            let unit = attr_ref(doc, factor, "measure_with_unit", "unit_component")?;
            return unit_dimensions(doc, unit, active);
        }
        if is_a(doc, id, "derived_unit") {
            let A::List(elements) = attr(doc, id, "derived_unit", "elements")? else {
                return None;
            };
            if elements.is_empty() {
                return None;
            }
            let mut sum = [0.; 7];
            for element in elements {
                let A::EntityRef(element) = element else {
                    return None;
                };
                let unit = attr_ref(doc, *element, "derived_unit_element", "unit")?;
                let exponent = number(attr(doc, *element, "derived_unit_element", "exponent")?)?;
                let d = unit_dimensions(doc, unit, active)?;
                for (out, x) in sum.iter_mut().zip(d) {
                    *out += x * exponent;
                }
            }
            return sum.iter().all(|x| x.is_finite()).then_some(sum);
        }
        exponents(doc, attr_ref(doc, id, "named_unit", "dimensions")?)
    })();
    active.remove(&id);
    result
}

/// ISO 10303-41 `dimensions_for_si_unit`, as published in the target MIM long form
/// (express::TARGET.source_url). Prefixes change scale, never dimensions.
fn dimensions_for_si_unit(name: &str) -> Option<Dimensions> {
    let d = match name.to_ascii_lowercase().as_str() {
        "metre" => [1, 0, 0, 0, 0, 0, 0],
        "gram" => [0, 1, 0, 0, 0, 0, 0],
        "second" => [0, 0, 1, 0, 0, 0, 0],
        "ampere" => [0, 0, 0, 1, 0, 0, 0],
        "kelvin" | "degree_celsius" => [0, 0, 0, 0, 1, 0, 0],
        "mole" => [0, 0, 0, 0, 0, 1, 0],
        "candela" | "lumen" => [0, 0, 0, 0, 0, 0, 1],
        "radian" | "steradian" => [0, 0, 0, 0, 0, 0, 0],
        "hertz" | "becquerel" => [0, 0, -1, 0, 0, 0, 0],
        "newton" => [1, 1, -2, 0, 0, 0, 0],
        "pascal" => [-1, 1, -2, 0, 0, 0, 0],
        "joule" => [2, 1, -2, 0, 0, 0, 0],
        "watt" => [2, 1, -3, 0, 0, 0, 0],
        "coulomb" => [0, 0, 1, 1, 0, 0, 0],
        "volt" => [2, 1, -3, -1, 0, 0, 0],
        "farad" => [-2, -1, 4, 2, 0, 0, 0],
        "ohm" => [2, 1, -3, -2, 0, 0, 0],
        "siemens" => [-2, -1, 3, 2, 0, 0, 0],
        "weber" => [2, 1, -2, -1, 0, 0, 0],
        "tesla" => [0, 1, -2, -1, 0, 0, 0],
        "henry" => [2, 1, -2, -2, 0, 0, 0],
        "lux" => [-2, 0, 0, 0, 0, 0, 1],
        "gray" | "sievert" => [2, 0, -2, 0, 0, 0, 0],
        _ => return None,
    };
    Some(d.map(f64::from))
}

#[cfg(test)]
mod integer_tests {
    use super::*;

    #[test]
    fn upgrade_repairs_only_exact_integral_integer_items() {
        let bytes = b"ISO-10303-21;HEADER;FILE_DESCRIPTION((''),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;
#1=INTEGER_REPRESENTATION_ITEM('count',43.);
#2=INTEGER_REPRESENTATION_ITEM('fraction',1.5);
#3=INTEGER_REPRESENTATION_ITEM('large',18014398509481984.);
#4=INTEGER_REPRESENTATION_ITEM('rounded fraction',43.0000000000000001);
#5=INTEGER_REPRESENTATION_ITEM('above bound',9007199254740993.0);
#6=INTEGER_REPRESENTATION_ITEM('underflow',1.E-9999);
ENDSEC;END-ISO-10303-21;";
        let doc = Document::parse(bytes.to_vec()).unwrap();
        let mut unchanged = Edit::new();
        assert_eq!(
            repair(&doc, &mut unchanged, &BTreeSet::new(), vec![], false).unwrap(),
            (vec![], 0)
        );
        assert!(unchanged.is_empty());
        let mut edit = Edit::new();
        let (repairs, added) = repair(&doc, &mut edit, &BTreeSet::new(), vec![], true).unwrap();
        assert_eq!(repairs, vec![(1, UpgradeRepairKind::R3IntegerLiteral)]);
        assert_eq!(added, 0);
        let after = Document::parse(doc.apply(&edit).unwrap().bytes).unwrap();
        assert_eq!(
            attr(&after, 1, "literal_number", "the_value"),
            Some(&A::Integer(43))
        );
        for id in [2, 3, 4, 5, 6] {
            assert_eq!(
                doc.span(id).map(|s| &doc.bytes()[s]),
                after.span(id).map(|s| &after.bytes()[s])
            );
        }
    }
}
