//! Synthetic hierarchy: two subassemblies, three single-solid leaves and two
//! occurrences of one three-solid product. Geometry comes from the public torus.
pub fn nested() -> Vec<u8> {
    use haecceity::p21::{self, Attribute, RawEntity};
    fn shift(a: &mut Attribute, offset: u64) {
        match a {
            Attribute::EntityRef(id) if (15..=72).contains(id) => *id += offset,
            Attribute::List(v) => v.iter_mut().for_each(|a| shift(a, offset)),
            Attribute::Typed { value, .. } => shift(value, offset),
            _ => {}
        }
    }
    let bytes = include_bytes!("../fixtures/full_torus.step");
    let doc = p21::Document::parse(bytes.to_vec()).unwrap();
    let mut text = String::from_utf8(bytes.to_vec()).unwrap();
    let mut extra = String::new();
    for offset in [100, 200, 300] {
        for id in 15..=72 {
            let mut e = doc.get(id).unwrap().clone();
            if let RawEntity::Simple { attributes, .. } = &mut e {
                attributes.iter_mut().for_each(|a| shift(a, offset));
            }
            extra.push_str(&format!("#{}={};\n", id + offset, p21::encode(&e).unwrap()));
        }
    }
    for (base, items) in [
        (1000, "#11,#115,#215,#315"),
        (2000, "#11"),
        (2010, "#11"),
        (2020, "#11"),
    ] {
        extra.push_str(&format!("#{base}=PRODUCT('p{base}','p{base}','',(#8));\n#{}=PRODUCT_DEFINITION_FORMATION('','',#{base});\n#{}=PRODUCT_DEFINITION('design','',#{},#9);\n#{}=PRODUCT_DEFINITION_SHAPE('','',#{});\n#{}=SHAPE_DEFINITION_REPRESENTATION(#{},#{});\n#{}=ADVANCED_BREP_SHAPE_REPRESENTATION('',({items}),#73);\n", base+1,base+2,base+1,base+3,base+2,base+4,base+3,base+5,base+5));
    }
    for (i, (parent, child)) in [
        (2002, 2012),
        (2002, 2022),
        (2012, 5),
        (2012, 5),
        (2012, 1002),
        (2022, 5),
        (2022, 1002),
    ]
    .into_iter()
    .enumerate()
    {
        extra.push_str(&format!(
            "#{}=NEXT_ASSEMBLY_USAGE_OCCURRENCE('{i}','','',#{parent},#{child},$);\n",
            3000 + i
        ));
    }
    text.insert_str(text.rfind("ENDSEC;").unwrap(), &extra);
    text.into_bytes()
}
