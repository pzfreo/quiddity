//! Duplicate a part sharing only one geometry point (the reader acceptance fixture).
pub fn source(b: Vec<u8>, shared: u64) -> String {
    use haecceity::p21::{Attribute, RawEntity};
    fn remap(a: &mut Attribute, offset: u64, shared: u64) {
        match a {
            Attribute::EntityRef(id) if *id != shared => *id += offset,
            Attribute::List(v) => {
                for a in v {
                    remap(a, offset, shared)
                }
            }
            Attribute::Typed { value, .. } => remap(value, offset, shared),
            _ => {}
        }
    }
    let mut text = String::from_utf8(b.clone()).unwrap();
    let doc = haecceity::p21::Document::parse(b).unwrap();
    let offset = doc.graph().entities.keys().max().unwrap() + 1;
    // Duplicate an otherwise independent part. Only the vertex's CARTESIAN_POINT #23 is
    // shared; every other geometry, placement, product and context gets a fresh number.
    let mut extra = String::new();
    for (&id, e) in &doc.graph().entities {
        if id == shared {
            continue;
        }
        let mut e = e.clone();
        match &mut e {
            RawEntity::Simple { attributes, .. } => {
                for a in attributes {
                    remap(a, offset, shared)
                }
            }
            RawEntity::Complex { parts, .. } => {
                for a in parts.iter_mut().flat_map(|p| &mut p.attributes) {
                    remap(a, offset, shared)
                }
            }
        }
        extra.push_str(&format!(
            "#{}={};\n",
            id + offset,
            haecceity::p21::encode(&e).unwrap()
        ));
    }
    let end = text.rfind("ENDSEC;").unwrap();
    text.insert_str(end, &extra);
    text
}
