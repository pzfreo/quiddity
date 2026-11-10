#![cfg(feature = "manifest")]
use haecceity::step::manifest::{self, Manifest, ScanOptions};
use std::io::{BufReader, Cursor};
fn manifest() -> Manifest {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/ap242/assembly/assembly.step");
    manifest::scan(std::fs::File::open(path).unwrap(), &ScanOptions::default()).unwrap()
}
fn load(bytes: &[u8], ids: &[String]) -> Result<Manifest, haecceity::step::StepError> {
    Manifest::read_selected(BufReader::new(bytes), ids, |_| Ok(()))
}
#[test]
fn projection_streams_full_instances_and_reads_identical_geometry() {
    let m = manifest();
    let ids = vec![m.products[1].id.clone()];
    let mut instances = Vec::new();
    let bytes = serde_json::to_vec(&m).unwrap();
    let selected = Manifest::read_selected(BufReader::new(bytes.as_slice()), &ids, |i| {
        instances.push(i);
        Ok(())
    })
    .unwrap();
    assert_eq!(selected.products, vec![m.products[1].clone()]);
    assert_eq!(instances, m.instances);
    assert!(selected.instances.is_empty() && selected.assemblies.is_empty());
    assert_eq!(selected.common, m.common);
    assert_eq!(selected.source, m.source);
    assert_eq!(selected.refusals, m.refusals);
    let file = std::fs::File::open(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/ap242/assembly/assembly.step"),
    )
    .unwrap();
    let a = m.read_product(&file, &ids[0]).unwrap();
    let b = selected.read_product(&file, &ids[0]).unwrap();
    assert_eq!(a.faces.len(), b.faces.len());
    for i in 0..a.faces.len() {
        assert_eq!(a.face_source(i), b.face_source(i));
    }
    assert!(load(&bytes, &[]).unwrap().products.is_empty());
}
#[test]
fn reordered_fields_skip_unselected_bodies_but_validate_all_ids() {
    let m = manifest();
    let ids = vec![m.products[0].id.clone()];
    let mut v = serde_json::to_value(&m).unwrap(); // sorted keys put instances before products and faces before id
    let bytes = serde_json::to_vec(&v).unwrap();
    assert_eq!(
        load(&bytes, &ids).unwrap().products,
        vec![m.products[0].clone()]
    );
    // Canonical id-first unselected products must not deserialize their bodies.
    let mut text = String::from_utf8(serde_json::to_vec(&m).unwrap()).unwrap();
    let product = &m.products[1];
    let old = serde_json::to_string(product).unwrap();
    text = text.replace(
        &old,
        &format!(
            "{{\"id\":\"{}\",\"faces\":\"deliberately not a Product\"}}",
            product.id
        ),
    );
    assert_eq!(
        load(text.as_bytes(), &ids).unwrap().products,
        vec![m.products[0].clone()]
    );
    for id in ["../bad", &m.products[0].id] {
        v["products"][1]["id"] = id.into();
        assert!(
            load(&serde_json::to_vec(&v).unwrap(), &ids)
                .unwrap_err()
                .to_string()
                .contains("invalid or duplicate")
        );
    }
}
#[test]
fn refuses_unknown_ids_versions_trailing_data_and_callback_errors() {
    let m = manifest();
    let mut bytes = serde_json::to_vec(&m).unwrap();
    assert!(
        load(&bytes, &["p999".into(), "p998".into()])
            .unwrap_err()
            .to_string()
            .contains("p999, p998")
    );
    let mut v = serde_json::to_value(&m).unwrap();
    v["reader"]["haecceity"] = "wrong".into();
    assert!(
        load(&serde_json::to_vec(&v).unwrap(), &[])
            .unwrap_err()
            .to_string()
            .contains("reader")
    );
    assert!(
        Manifest::read_selected(Cursor::new(&bytes), &[], |_| Err(
            haecceity::step::StepError::Unsupported("callback failed".into())
        ))
        .unwrap_err()
        .to_string()
        .contains("callback failed")
    );
    bytes.extend_from_slice(b" {}");
    assert!(load(&bytes, &[]).is_err());
    let mut v = serde_json::to_value(&m).unwrap();
    v["instances"][0]["product"] = "p999".into();
    assert!(
        load(&serde_json::to_vec(&v).unwrap(), &[])
            .unwrap_err()
            .to_string()
            .contains("unknown product p999")
    );
}

#[test]
fn full_and_selected_loads_validate_instance_and_refusal_references() {
    let mut m = manifest();
    let ids = vec![m.products[0].id.clone()];
    m.refusals.push(manifest::Refusal {
        products: m.products.iter().map(|p| p.id.clone()).collect(),
        entity: m.products[1].shape,
        reason: "valid references, including an unselected product".into(),
    });
    m.reindex_products().unwrap();
    let bytes = serde_json::to_vec(&m).unwrap();
    assert_eq!(serde_json::from_slice::<Manifest>(&bytes).unwrap(), m);
    assert_eq!(load(&bytes, &ids).unwrap().refusals, m.refusals);

    for id in ["p999", "../invalid"] {
        for instance in [true, false] {
            let mut corrupt = serde_json::to_value(&m).unwrap();
            let expected = if instance {
                corrupt["instances"][0]["product"] = id.into();
                if id == "p999" {
                    format!("instance references unknown product {id}")
                } else {
                    format!("invalid instance product id {id}")
                }
            } else {
                corrupt["refusals"][0]["products"][0] = id.into();
                format!("refusal references unknown product {id}")
            };
            let bytes = serde_json::to_vec(&corrupt).unwrap();
            let full = serde_json::from_slice::<Manifest>(&bytes).unwrap_err();
            assert_eq!(full.classify(), serde_json::error::Category::Data);
            assert!(full.to_string().contains(&expected), "{full}");
            let selected = load(&bytes, &ids).unwrap_err();
            assert!(matches!(
                selected,
                haecceity::step::StepError::Unsupported(_)
            ));
            assert!(selected.to_string().contains(&expected), "{selected}");
        }
    }
}
