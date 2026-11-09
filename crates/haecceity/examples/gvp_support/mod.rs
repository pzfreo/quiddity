//! Shared measurement policy for the example and CI; no reference exceptions here.
use haecceity::{
    Part, geom, gvp,
    p21::{self, Document},
    step::StepFile,
};
use serde_json::{Value, json};
use std::{collections::HashSet, io::Read, path::Path};

pub const BAND: f64 = 0.001;

pub fn bytes(path: &Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let raw = std::fs::read(path)?;
    if raw.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(raw.as_slice()).read_to_end(&mut out)?;
        Ok(out)
    } else {
        Ok(raw)
    }
}
pub fn walk(path: &Path, out: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            walk(&entry?.path(), out)?;
        }
    } else if path.file_name().is_some_and(|s| {
        let s = s.to_string_lossy().to_ascii_lowercase();
        [".stp", ".step", ".stp.gz", ".step.gz"]
            .iter()
            .any(|e| s.ends_with(e))
    }) {
        out.push(path.to_owned());
    }
    Ok(())
}
/// A geometric selection is followed only down its reference graph, never up to its owner.
fn selected_faces(doc: &Document, items: &[u64], sources: &[u64]) -> Vec<usize> {
    let mut seen = HashSet::new();
    let mut todo = items.to_vec();
    while let Some(id) = todo.pop() {
        if seen.insert(id)
            && let Some(e) = doc.get(id)
            && !sources.contains(&id)
        {
            p21::visit_attributes(e, &mut |a| {
                if let p21::Attribute::EntityRef(r) = a {
                    todo.push(*r)
                }
            });
        }
    }
    sources
        .iter()
        .enumerate()
        .filter_map(|(i, id)| seen.contains(id).then_some(i))
        .collect()
}
fn measure(part: &Part, faces: &[usize], whole: bool) -> Value {
    let mass = if whole && part.solids.is_empty() {
        part.shell_mass(faces).map(|(_, a)| (0.0, a))
    } else if whole {
        part.solids
            .iter()
            .enumerate()
            .try_fold((0.0, 0.0), |(v, a), (i, _)| {
                part.solid_mass(i).map(|(w, b)| (v + w, a + b))
            })
    } else {
        part.shell_mass(faces)
    };
    // Use the existing material-centroid API on a geometry-preserving solid selection.
    let centroid = if whole {
        part.volume_centroid()
    } else {
        Part::new(
            part.faces.clone(),
            part.edges.clone(),
            vec![haecceity::brep::Solid {
                faces: faces.to_vec(),
            }],
        )
        .volume_centroid()
    };
    let bounds = part.bounds();
    json!({"volume":mass.map(|m|m.0),"area":mass.map(|m|m.1),"centroid":centroid.map(|m|m.1),"diagonal":geom::dist(bounds.min,bounds.max),"unresolved_faces":part.unresolved_faces(),"unresolved_edges":part.unresolved_edges()})
}
pub fn file(path: &Path) -> Result<Value, Box<dyn std::error::Error>> {
    let raw = bytes(path)?;
    let doc = Document::parse(raw.clone())?;
    let properties = gvp::read(&doc).map_err(std::io::Error::other)?;
    let file = StepFile::read(&raw)?;
    let mut parts = Vec::new();
    for (i, def) in file.parts.iter().enumerate() {
        let part = file.part(i)?;
        let solid_faces: Vec<_> = part
            .solids
            .iter()
            .flat_map(|s| s.faces.iter().copied())
            .collect();
        let solid_faces = if solid_faces.is_empty() {
            (0..part.faces.len()).collect()
        } else {
            solid_faces
        };
        let whole_ours = measure(&part, &solid_faces, true);
        let mut cache = std::collections::HashMap::new();
        let mut validations = Vec::new();
        for p in properties.iter().filter(|p| {
            p.product_definition == Some(def.product_definition) && p.occurrence.is_none()
        }) {
            let whole = p.target == def.shape || p.target == def.product_definition;
            let faces = if whole {
                solid_faces.clone()
            } else {
                selected_faces(&doc, &p.items, &def.faces)
            };
            let ours = if faces.is_empty() {
                Value::Null
            } else if whole || faces == solid_faces {
                whole_ours.clone()
            } else {
                cache
                    .entry(faces.clone())
                    .or_insert_with(|| measure(&part, &faces, false))
                    .clone()
            };
            validations.push(json!({"property":p.property,"target":p.target,"items":p.items,"faces":faces,"reference":{"volume":p.volume,"area":p.area,"centroid":p.centroid},"ours":ours,"scope":if whole {"product"} else {"aspect"}}));
        }
        parts.push(json!({"part":i,"name":def.name,"product_definition":def.product_definition,"faces":def.faces,"placements":def.placements.iter().map(|p|p.placement).collect::<Vec<_>>(),"ours":whole_ours,"gvp":validations}));
    }
    let attached: HashSet<_> = parts
        .iter()
        .flat_map(|p| {
            p["gvp"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| p["property"].as_u64())
        })
        .collect();
    let unattached:Vec<_>=properties.iter().filter(|p|!attached.contains(&p.property)).map(|p|json!({"property":p.property,"target":p.target,"product_definition":p.product_definition,"occurrence":p.occurrence,"reference":{"volume":p.volume,"area":p.area,"centroid":p.centroid}})).collect();
    Ok(
        json!({"file":path,"parts":parts,"unattached_gvp":unattached,"geometric_properties":properties.len()}),
    )
}
pub fn relative(ours: f64, reference: f64) -> f64 {
    if reference == 0.0 {
        if ours == 0.0 { 0.0 } else { f64::INFINITY }
    } else {
        (ours - reference).abs() / reference.abs()
    }
}
pub fn error(ours: &Value, reference: &Value, quantity: &str, diagonal: f64) -> Option<f64> {
    if quantity == "centroid" {
        let a = ours[quantity].as_array()?;
        let b = reference[quantity].as_array()?;
        let mut sum = 0.0;
        for i in 0..3 {
            sum += (a[i].as_f64()? - b[i].as_f64()?).powi(2);
        }
        Some(sum.sqrt() / diagonal)
    } else {
        Some(relative(
            ours[quantity].as_f64()?,
            reference[quantity].as_f64()?,
        ))
    }
}
