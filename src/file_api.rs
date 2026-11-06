//! File/byte entry points shared by the Python wheel and additive CLI commands.
use serde::Serialize;
use serde_json::json;

use crate::{
    Part, evidence_view,
    kernel::{
        p21::{self, Attribute, RawEntity},
        step,
    },
    pmi_json, serve,
};

/// Pretty JSON with the CLI's terminating newline.
pub fn json<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string_pretty(value)
        .map(|text| format!("{text}\n"))
        .map_err(|e| e.to_string())
}

/// Recognition on a snapshot of STEP bytes, with the same digest and output as a file.
pub fn recognise_bytes(bytes: Vec<u8>) -> Result<String, String> {
    let (output, _) = serve::recognise_bytes("<bytes>", bytes).map_err(|e| e.to_string())?;
    json(&output)
}

/// File recognition, identical to stdout from `quiddity <path>`.
pub fn recognise_file(path: &str) -> Result<String, String> {
    let (output, _) = serve::recognise_file(path).map_err(|e| e.to_string())?;
    json(&output)
}

/// The evidence of a loaded part.
pub fn evidence(part: &Part, framed: bool) -> Result<String, String> {
    json(&evidence_view::document(part, framed).map_err(|e| e.to_string())?)
}

/// Evidence from a STEP path (gzip is supported by the existing STEP reader).
pub fn evidence_file(path: &str, framed: bool) -> Result<String, String> {
    evidence(
        &step::read_step_file(std::path::Path::new(path)).map_err(|e| e.to_string())?,
        framed,
    )
    .map(|s| s.trim_end_matches('\n').to_owned())
}

/// A proved point on a face; out-of-range indices are refused before entering the kernel.
pub fn anchor(part: &Part, face: usize) -> Result<String, String> {
    if face >= part.faces.len() {
        return Err(format!(
            "face {face}: out of range ({} faces)",
            part.faces.len()
        ));
    }
    let anchor = part.face_anchor(face).map_err(|e| e.to_string())?;
    json(&json!({"face": face, "point": anchor.point, "uv": anchor.uv}))
}

/// Existing fingerprints/3 from a STEP file.
pub fn fingerprints_file(path: &str) -> Result<String, String> {
    let (output, _) = serve::recognise_file(path).map_err(|e| e.to_string())?;
    serde_json::to_string_pretty(&output.recognition.fingerprints).map_err(|e| e.to_string())
}

/// `quiddity parts`: the distinct parts of the STEP text *step* (index, name, face and edge
/// counts, placements) and the binding of *binding*, the bytes the caller read (the same as
/// *step* unless converted, as from Fusion). Pretty JSON without the CLI's trailing newline.
pub fn parts(step: &[u8], binding: &[u8]) -> Result<String, String> {
    let defs = step::read_part_definitions(step).map_err(|e| e.to_string())?;
    let json = pmi_json::parts_json(&defs, &pmi_json::Binding::of(binding));
    serde_json::to_string_pretty(&json).map_err(|e| e.to_string())
}

/// Every DATA instance of the Part 21 text *step*, in file order, as compact JSON: its `id`
/// (`#N`), its `types` (the upper-case entity name, or each leaf of a complex instance in the
/// order written) and the instances it `refs`erences, in attribute order with repeats.
pub fn step_entities(step: Vec<u8>) -> Result<String, String> {
    let doc = p21::Document::parse(step).map_err(|e| e.to_string())?;
    let mut ids: Vec<u64> = doc.ids().collect();
    ids.sort_by_key(|&id| doc.span(id).map(|span| span.start));
    let rows: Vec<_> = ids
        .into_iter()
        .filter_map(|id| {
            let entity = doc.get(id)?;
            let types: Vec<&str> = match entity {
                RawEntity::Simple { name, .. } => vec![name],
                RawEntity::Complex { parts, .. } => parts.iter().map(|p| p.name.as_str()).collect(),
            };
            let mut refs = Vec::new();
            p21::visit_attributes(entity, &mut |a| {
                if let Attribute::EntityRef(target) = a {
                    refs.push(*target);
                }
            });
            Some(json!({"id": id, "types": types, "refs": refs}))
        })
        .collect();
    serde_json::to_string(&rows).map_err(|e| e.to_string())
}

/// Versioned capabilities, with no Python or OpenCascade dependency.
pub fn capabilities() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_version": "quiddity-rust/capabilities/1",
        "package": {"name": "quiddity-rust", "version": env!("CARGO_PKG_VERSION")},
        "recognition": crate::recognition::SCHEMA_VERSION,
        "evidence": "quiddity-rust/evidence/1",
        "fingerprints": crate::correspondence::fingerprint::FINGERPRINT_VERSION,
        "inputs": ["step-path", "step-bytes"],
        "functions": ["recognise", "evidence", "anchor", "fingerprints", "correspond", "capabilities"],
        "pmi_json_import": cfg!(feature = "json-import"),
    })).expect("capabilities serialise")
}
