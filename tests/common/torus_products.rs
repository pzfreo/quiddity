//! Several products made from the torus fixture, each with its own contexts.

/// `copies` products from the torus fixture, each with its own contexts and units. Only the
/// application context and protocol and the standard frame are shared.
pub fn torus_products(copies: u64) -> (Vec<u8>, u64) {
    use haecceity::p21::{Attribute, RawEntity};
    fn remap(a: &mut Attribute, offset: u64, shared: &[u64]) {
        match a {
            Attribute::EntityRef(id) if !shared.contains(id) => *id += offset,
            Attribute::List(v) => {
                for a in v {
                    remap(a, offset, shared)
                }
            }
            Attribute::Typed { value, .. } => remap(value, offset, shared),
            _ => {}
        }
    }
    let b = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/full_torus.step"),
    )
    .unwrap();
    let doc = haecceity::p21::Document::parse(b.clone()).unwrap();
    let stride = doc.max_id() + 1;
    let shared = [1, 2, 11, 12, 13, 14];
    let mut text = String::from_utf8(b).unwrap();
    let end = text.rfind("ENDSEC;").unwrap();
    let mut extra = String::new();
    for product in 1..copies {
        for (id, e) in doc.iter() {
            if shared.contains(&id) {
                continue;
            }
            let offset = product * stride;
            let mut e = e.clone();
            match &mut e {
                RawEntity::Simple { attributes, .. } => {
                    for a in attributes {
                        remap(a, offset, &shared)
                    }
                }
                RawEntity::Complex { parts, .. } => {
                    for a in parts.iter_mut().flat_map(|p| &mut p.attributes) {
                        remap(a, offset, &shared)
                    }
                }
            }
            extra.push_str(&format!(
                "#{}={};\n",
                id + offset,
                haecceity::p21::encode(&e).unwrap()
            ));
        }
    }
    text.insert_str(end, &extra);
    (text.into_bytes(), stride)
}
