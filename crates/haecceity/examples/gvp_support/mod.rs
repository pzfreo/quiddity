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
    let mass = if whole {
        let volume = part
            .solids
            .iter()
            .enumerate()
            .try_fold(0.0, |v, (i, _)| part.solid_mass(i).map(|(w, _)| v + w));
        let area = faces
            .iter()
            .try_fold(0.0, |a, &f| part.face_mass(f).map(|m| a + m[0]));
        volume.zip(area)
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
    let unintegrated: Vec<_> = if mass.is_none() {
        faces
            .iter()
            .filter(|&&f| part.face_mass(f).is_none())
            .map(|&f| json!({"index":f,"entity":part.face_source(f).map(|s|s.entity)}))
            .collect()
    } else {
        Vec::new()
    };
    json!({"unintegrated_faces":unintegrated,"volume":mass.map(|m|m.0),"area":mass.map(|m|m.1),"centroid":centroid.map(|m|m.1),"diagonal":geom::dist(bounds.min,bounds.max),"unresolved_faces":part.unresolved_faces(),"unresolved_edges":part.unresolved_edges()})
}
/// Whether plain product surface area includes independent sheets, by FILE_SCHEMA.
fn includes_sheets(raw: &[u8]) -> Option<bool> {
    let text = String::from_utf8_lossy(&raw[..raw.len().min(4096)]).to_ascii_uppercase();
    let start = text.find("FILE_SCHEMA")?;
    let schema = &text[start..start + text[start..].find(';')?];
    if schema.contains("AP203") || schema.contains("CONFIG_CONTROL_DESIGN") {
        Some(true)
    } else if schema.contains("AP242") {
        Some(false)
    } else {
        None
    }
}
pub fn file(path: &Path) -> Result<Value, Box<dyn std::error::Error>> {
    let raw = bytes(path)?;
    let doc = Document::parse(raw.clone())?;
    let properties = gvp::read(&doc).map_err(std::io::Error::other)?;
    let file = StepFile::read(&raw)?;
    let schema_includes_sheets = includes_sheets(&raw);
    let mut parts = Vec::new();
    for (i, def) in file.parts.iter().enumerate() {
        let part = file.part(i)?;
        let solid_faces: Vec<_> = part
            .solids
            .iter()
            .flat_map(|s| s.faces.iter().copied())
            .collect();
        let whole_faces: Vec<_> = (0..part.faces.len()).collect();
        let whole_ours = measure(&part, &whole_faces, true);
        let mut cache = std::collections::HashMap::new();
        let mut validations = Vec::new();
        for p in properties.iter().filter(|p| {
            p.product_definition == Some(def.product_definition) && p.occurrence.is_none()
        }) {
            let whole = p.target == def.shape || p.target == def.product_definition;
            let mut faces = if whole {
                whole_faces.clone()
            } else {
                selected_faces(&doc, &p.items, &def.faces)
            };
            // These legacy AP203 exports put the boundary curves of independent sheets in
            // the aspect representation. Its explicit description scopes the property to
            // all surface data of the product, not to its closed material solids.
            let surface_data = matches!(doc.get(p.target),Some(p21::RawEntity::Simple {name,attributes,..}) if name=="SHAPE_ASPECT" && matches!(attributes.get(1),Some(p21::Attribute::String(description)) if description.to_ascii_lowercase().starts_with("surface data associated with ")));
            if faces.is_empty() && surface_data {
                faces = (0..part.faces.len())
                    .filter(|&f| part.faces[f].solid.is_none())
                    .collect();
            }
            let mut ours = if faces.is_empty() {
                Value::Null
            } else if whole || faces == whole_faces {
                whole_ours.clone()
            } else if faces == solid_faces {
                // The same material as the product, excluding its independent sheets.
                // Reuse the original part's moments rather than constructing new caches.
                let mut material = whole_ours.clone();
                material["area"] = json!(
                    faces
                        .iter()
                        .try_fold(0.0, |a, &f| part.face_mass(f).map(|m| a + m[0]))
                );
                material
            } else if p.volume.is_none() && p.centroid.is_none() {
                json!({"volume":null,"centroid":null,"area":faces.iter().try_fold(0.0,|a,&f|part.face_mass(f).map(|m|a+m[0])),"diagonal":whole_ours["diagonal"]})
            } else {
                cache
                    .entry(faces.clone())
                    .or_insert_with(|| measure(&part, &faces, false))
                    .clone()
            };
            // A product's surface area either includes its independent sheets or not.
            // Wetted area never does. For plain surface area the NIST exports follow their schema: AP203 includes
            // the sheets, AP242 excludes them, with the same representation structure.
            // Any other schema has no recorded convention, so the area is unavailable.
            let sheets = !solid_faces.is_empty() && solid_faces.len() < whole_faces.len();
            let sheet_faces: Vec<_> = faces
                .iter()
                .copied()
                .filter(|&f| part.faces[f].solid.is_none())
                .collect();
            if whole && sheets && !ours.is_null() {
                let scope = match p.area_kind {
                    Some(gvp::AreaKind::Wetted) => Some("solids"),
                    _ => match schema_includes_sheets {
                        Some(true) => Some("solids and sheets"),
                        Some(false) => Some("solids"),
                        None => None,
                    },
                };
                match scope {
                    Some("solids") => {
                        ours["area"] = json!(
                            solid_faces
                                .iter()
                                .try_fold(0.0, |a, &f| part.face_mass(f).map(|m| a + m[0]))
                        )
                    }
                    Some(_) => {}
                    None => ours["area"] = Value::Null,
                }
                ours["area_scope"] = json!(scope.unwrap_or("unknown for this schema"));
            }
            // Independent surface area is the selection's sheets alone.
            if p.independent_area.is_some() {
                if ours.is_null() {
                    ours = json!({"diagonal":whole_ours["diagonal"]});
                }
                ours["independent_area"] = json!(
                    sheet_faces
                        .iter()
                        .try_fold(0.0, |a, &f| part.face_mass(f).map(|m| a + m[0]))
                );
            }
            validations.push(json!({"area_kind":match p.area_kind {Some(gvp::AreaKind::Wetted)=>Some("wetted"),Some(gvp::AreaKind::Surface)=>Some("surface"),None=>None},"property":p.property,"target":p.target,"items":p.items,"faces":faces,"sheet_faces":sheet_faces,"reference":{"volume":p.volume,"area":p.area,"independent_area":p.independent_area,"centroid":p.centroid},"ours":ours,"association":if surface_data {"legacy surface-data aspect: independent sheets of owning product"} else {"explicit STEP owner/geometry"},"scope":if whole {"product"} else {"aspect"}}));
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
    let unattached:Vec<_>=properties.iter().filter(|p|!attached.contains(&p.property)).map(|p|json!({"area_kind":match p.area_kind {Some(gvp::AreaKind::Wetted)=>Some("wetted"),Some(gvp::AreaKind::Surface)=>Some("surface"),None=>None},"property":p.property,"target":p.target,"product_definition":p.product_definition,"occurrence":p.occurrence,"reference":{"volume":p.volume,"area":p.area,"independent_area":p.independent_area,"centroid":p.centroid}})).collect();
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
