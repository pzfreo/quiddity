//! Portable identities for anonymous units. References are expanded by content, never by
//! document-local id. The encoding retains named-unit kinds, SI prefixes, conversion
//! factors, dimensions and derived-unit exponents. Derived-unit SETs are sorted by content.
use super::model::Decimal;
use crate::express;
use crate::p21::{self, Attribute as A, Document, RawEntity};

pub(super) const PREFIX: &str = "step-unit:";
const CHILD: &str = "UNIT_ENTITY";
const NUMBER: &str = "UNIT_REAL";

pub(super) struct UnitRecord {
    pub(super) entity: RawEntity,
    pub(super) numbers: Vec<Decimal>,
}

fn canonical_number(text: &str) -> Option<String> {
    let mut text = Decimal::parse(text).ok()?.scaled(0).as_str().to_owned();
    if !text.contains(['.', 'E', 'e']) {
        text.push('.');
    }
    Some(text)
}

fn allowed(record: &RawEntity) -> bool {
    let names: Vec<&str> = match record {
        RawEntity::Simple { name, .. } => vec![name],
        RawEntity::Complex { parts, .. } => parts.iter().map(|p| p.name.as_str()).collect(),
    };
    names.iter().all(|name| {
        [
            "named_unit",
            "derived_unit",
            "derived_unit_element",
            "measure_with_unit",
            "dimensional_exponents",
        ]
        .iter()
        .any(|base| express::TARGET.is_a(name, base))
    })
}

pub(super) fn identity(doc: &Document, id: u64) -> Option<String> {
    fn expand(doc: &Document, id: u64, path: &mut Vec<u64>) -> Option<String> {
        if path.len() >= 32 || path.contains(&id) {
            return None;
        }
        let mut record = doc.get(id)?.clone();
        if !allowed(&record) {
            return None;
        }
        path.push(id);
        let real_texts = super::read::real_tokens(&doc.bytes()[doc.span(id)?]);
        let mut real_texts = real_texts.iter();
        let mut valid = true;
        p21::visit_attributes_mut(&mut record, &mut |a| {
            // Keep exact decimal contents, including digits beyond f64 precision.
            // REAL slots also accept INTEGER tokens; canonicalize equal numeric values.
            let number = match a {
                A::Real(_) => real_texts.next().and_then(|s| canonical_number(s)),
                A::Integer(value) => canonical_number(&value.to_string()),
                _ => None,
            };
            if matches!(a, A::Real(_) | A::Integer(_)) {
                if let Some(text) = number {
                    *a = A::Typed {
                        type_name: NUMBER.into(),
                        value: Box::new(A::String(text)),
                    };
                } else {
                    valid = false;
                }
            }
            if let A::EntityRef(target) = a {
                if let Some(text) = expand(doc, *target, path) {
                    *a = A::Typed {
                        type_name: CHILD.into(),
                        value: Box::new(A::String(text)),
                    };
                } else {
                    valid = false;
                }
            }
        });
        path.pop();
        if !valid {
            return None;
        }
        // elements is a SET: different instance numbering/order gives the same identity.
        if let RawEntity::Simple {
            name, attributes, ..
        } = &mut record
            && name.eq_ignore_ascii_case("derived_unit")
            && let Some(A::List(elements)) = attributes.first_mut()
        {
            elements.sort_by_key(|a| {
                p21::encode(&p21::simple("element", vec![a.clone()])).unwrap_or_default()
            });
        }
        p21::encode(&record).ok()
    }
    Some(format!("{PREFIX}{}", expand(doc, id, &mut Vec::new())?))
}

/// Postorder records whose local references are one-based indices into this vector.
/// Only the closed unit graph is accepted; external references cannot enter the writer.
pub(super) fn records(identity: &str) -> Option<Vec<UnitRecord>> {
    fn parse(text: &str, depth: usize, out: &mut Vec<UnitRecord>) -> Option<u64> {
        if depth >= 32 {
            return None;
        }
        let bytes = format!(
            "ISO-10303-21;HEADER;FILE_DESCRIPTION((''),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;#1={text};ENDSEC;END-ISO-10303-21;"
        );
        let doc = Document::parse(bytes.into_bytes()).ok()?;
        if doc.ids().count() != 1 {
            return None;
        }
        let mut record = doc.get(1)?.clone();
        if !allowed(&record) {
            return None;
        }
        let mut valid = true;
        let real_texts = super::read::real_tokens(&doc.bytes()[doc.span(1)?]);
        let mut real_texts = real_texts.iter();
        let mut numbers = Vec::new();
        p21::visit_attributes_mut(&mut record, &mut |a| {
            if matches!(a, A::Real(_)) {
                if let Some(number) = real_texts.next().and_then(|s| Decimal::parse(s).ok()) {
                    numbers.push(number);
                } else {
                    valid = false;
                }
            }
            if let A::Typed { type_name, value } = a
                && type_name.eq_ignore_ascii_case(NUMBER)
            {
                if let A::String(text) = value.as_ref()
                    && let Ok(number) = Decimal::parse(text)
                    && number.to_f64().is_finite()
                {
                    *a = A::Real(number.to_f64());
                    numbers.push(number);
                } else {
                    valid = false;
                }
            }
            if let A::EntityRef(_) = a {
                valid = false;
            }
            if let A::Typed { type_name, value } = a
                && type_name.eq_ignore_ascii_case(CHILD)
            {
                if let A::String(text) = value.as_ref()
                    && let Some(id) = parse(text, depth + 1, out)
                {
                    *a = A::EntityRef(id);
                } else {
                    valid = false;
                }
            }
        });
        if !valid {
            return None;
        }
        out.push(UnitRecord {
            entity: record,
            numbers,
        });
        Some(out.len() as u64)
    }
    let mut out = Vec::new();
    parse(identity.strip_prefix(PREFIX)?, 0, &mut out)?;
    let root = &out.last()?.entity;
    let unit = |name: &str| {
        express::TARGET.is_a(name, "named_unit") || express::TARGET.is_a(name, "derived_unit")
    };
    let is_unit = match root {
        RawEntity::Simple { name, .. } => unit(name),
        RawEntity::Complex { parts, .. } => parts.iter().any(|part| unit(&part.name)),
    };
    is_unit.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn doc(records: &str) -> Document {
        Document::parse(format!("ISO-10303-21;HEADER;FILE_DESCRIPTION((''),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;").into_bytes()).unwrap()
    }
    #[test]
    fn identity_uses_contents_including_kind_prefix_and_exponent() {
        let a = doc(
            "#1=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));#2=(MASS_UNIT()NAMED_UNIT(*)SI_UNIT(.KILO.,.GRAM.));#3=DERIVED_UNIT_ELEMENT(#1,-3.);#4=DERIVED_UNIT_ELEMENT(#2,1.);#5=DERIVED_UNIT((#3,#4));",
        );
        let b = doc(
            "#1=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.));#20=(MASS_UNIT()NAMED_UNIT(*)SI_UNIT(.KILO.,.GRAM.));#10=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));#30=DERIVED_UNIT_ELEMENT(#20,1);#40=DERIVED_UNIT_ELEMENT(#10,-3);#50=DERIVED_UNIT((#30,#40));#60=DERIVED_UNIT_ELEMENT(#1,-3.);#70=DERIVED_UNIT((#30,#60));#80=DERIVED_UNIT_ELEMENT(#10,-2.);#90=DERIVED_UNIT((#30,#80));",
        );
        let identity = identity(&a, 5).unwrap();
        assert_eq!(Some(identity.clone()), super::identity(&b, 50));
        assert_ne!(Some(identity.clone()), super::identity(&b, 70));
        assert_ne!(Some(identity.clone()), super::identity(&b, 90));
        let records = records(&identity).unwrap();
        let text: String = records
            .iter()
            .enumerate()
            .map(|(i, r)| {
                format!(
                    "#{}={};",
                    i + 1,
                    p21::Record::new(r.entity.clone())
                        .state_reals(&r.numbers.iter().map(Decimal::as_str).collect::<Vec<_>>())
                        .unwrap()
                        .encode()
                        .unwrap()
                )
            })
            .collect();
        let restored = doc(&text);
        assert_eq!(
            Some(identity),
            super::identity(&restored, records.len() as u64)
        );
        assert!(super::records("step-unit:DERIVED_UNIT((#123))").is_none());
        assert!(super::records("#5").is_none());
        let precise = |exponent| {
            doc(&format!(
                "#1=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.));#2=DERIVED_UNIT_ELEMENT(#1,{exponent});#3=DERIVED_UNIT((#2));"
            ))
        };
        let x = super::identity(&precise("1.000000000000000001"), 3).unwrap();
        let y = super::identity(&precise("1.000000000000000002"), 3).unwrap();
        assert_ne!(
            x, y,
            "distinct exact exponents must never bind the same unit"
        );
        assert!(super::records(&x).unwrap().iter().any(|r| {
            r.numbers
                .iter()
                .any(|n| n.as_str() == "1.000000000000000001")
        }));
    }
}
