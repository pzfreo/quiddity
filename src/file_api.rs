//! File/byte entry points shared by the Python wheel and additive CLI commands.
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};

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

/// A single STEP snapshot, inventory and recess projection, with both existing documents.
///
/// Evidence automatically uses part coordinates when a frame is inferred, and file
/// coordinates otherwise. There is no independent `framed` argument; use [`evidence`]
/// to select a fixed coordinate mode. Recognition document parameters remain in file
/// coordinates. A refused evidence view is retained as an explicit error beside the
/// recognition; it must never be mistaken for an empty view.
pub fn recognise_with_evidence_bytes(bytes: Vec<u8>) -> Result<String, String> {
    recognise_with_evidence_snapshot("<bytes>", bytes)
}

/// Read a STEP file once, including gzip inputs, before analysing both views.
///
/// Evidence automatically uses inferred part coordinates, falling back to file
/// coordinates when the frame is refused. Like [`recognise_with_evidence_bytes`],
/// this function has no independent `framed` argument; use [`evidence`] for an explicit
/// coordinate mode. Recognition document parameters remain in file coordinates.
pub fn recognise_with_evidence_file(path: &str) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    recognise_with_evidence_snapshot(path, bytes)
}

fn recognise_with_evidence_snapshot(path: &str, bytes: Vec<u8>) -> Result<String, String> {
    let digest = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let step_bytes = step::decode_step_file_bytes(std::path::Path::new(path), bytes)
        .map_err(|e| format!("{path}: {e}"))?;
    let part = step::read_step(&step_bytes).map_err(|e| format!("{path}: {e}"))?;
    let crate::correspondence::RecognitionWithEvidence {
        mut recognition,
        evidence,
        ..
    } = crate::correspondence::recognise_with_evidence(&part);
    recognition.source_sha256 = Some(digest);
    let document = crate::recognition::document(&recognition, part.faces.len());
    let (evidence, evidence_error) = match evidence {
        Ok(document) => (Some(document), None),
        Err(error) => (None, Some(error.to_string())),
    };
    json(&json!({
        "schema_version": "quiddity-rust/analysis/1",
        "recognition": serve::RecognitionOutput { recognition, document },
        "evidence": evidence,
        "evidence_error": evidence_error,
    }))
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
        "analysis": "quiddity-rust/analysis/1",
        "fingerprints": crate::correspondence::fingerprint::FINGERPRINT_VERSION,
        "inputs": ["step-path", "step-bytes"],
        "functions": ["recognise", "evidence", "recognise_with_evidence", "anchor", "fingerprints", "correspond", "capabilities"],
        "pmi_json_import": cfg!(feature = "json-import"),
    })).expect("capabilities serialise")
}

/// Recognition of an already read part in its own coordinates, bound to the original source.
/// Uses the same envelope and serialization as whole-file recognition.
pub fn recognise_part(part: &Part, source_sha256: &str) -> Result<String, String> {
    recognise_part_with_warnings(part, source_sha256, "<part>").map(|(json, _)| json)
}

/// The standard part envelope and the same diagnostics as whole-file recognition.
pub fn recognise_part_with_warnings(
    part: &Part,
    source_sha256: &str,
    label: &str,
) -> Result<(String, Vec<String>), String> {
    let mut recognition = crate::correspondence::recognise(part);
    recognition.source_sha256 = Some(source_sha256.to_owned());
    let document = crate::recognition::document(&recognition, part.faces.len());
    let warnings = serve::recognition_warnings(label, part, &recognition, &document);
    let output = serve::RecognitionOutput {
        recognition,
        document,
    };
    Ok((json(&output)?, warnings))
}

/// Scan a replayable STEP file (including gzip) without writing any sidecars.
#[cfg(feature = "manifest")]
pub fn manifest(
    path: &str,
    options: &step::manifest::ScanOptions,
) -> Result<step::manifest::Manifest, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    step::manifest::scan(file, options).map_err(|e| format!("{path}: {e}"))
}

/// Read just one product's ranges and recognise it using the whole-file JSON contract.
/// Shared-geometry refusals are returned; this never silently parses the whole source.
#[cfg(feature = "manifest")]
pub fn recognise_product(
    path: &str,
    manifest: &step::manifest::Manifest,
    id: &str,
) -> Result<String, String> {
    recognise_product_with_warnings(path, manifest, id).map(|(json, _)| json)
}

/// Product recognition and diagnostics, shared by the CLI and the dict-returning Python API.
#[cfg(feature = "manifest")]
pub fn recognise_product_with_warnings(
    path: &str,
    manifest: &step::manifest::Manifest,
    id: &str,
) -> Result<(String, Vec<String>), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let part = manifest
        .read_product(&file, id)
        .map_err(|e| format!("{path}: {e}"))?;
    recognise_part_with_warnings(&part, &manifest.source.sha256, path)
}

/// Validate a worker's selection before any output is written. Results follow manifest order.
#[cfg(feature = "manifest")]
pub fn product_ids(
    manifest: &step::manifest::Manifest,
    parts: Option<&[String]>,
) -> Result<Vec<String>, String> {
    let mut known = std::collections::HashSet::with_capacity(manifest.products.len());
    for p in &manifest.products {
        if !p.id.starts_with('p')
            || p.id.len() < 2
            || !p.id[1..].bytes().all(|b| b.is_ascii_digit())
            || !known.insert(p.id.as_str())
        {
            return Err(format!("invalid or duplicate manifest product id {}", p.id));
        }
    }
    if let Some(parts) = parts {
        let unknown: Vec<_> = parts
            .iter()
            .filter(|id| !known.contains(id.as_str()))
            .map(String::as_str)
            .collect();
        if !unknown.is_empty() {
            return Err(format!(
                "unknown manifest product ids: {}",
                unknown.join(", ")
            ));
        }
    }
    let selected = parts.map(|parts| {
        parts
            .iter()
            .map(String::as_str)
            .collect::<std::collections::HashSet<_>>()
    });
    Ok(manifest
        .products
        .iter()
        .filter(|p| {
            selected
                .as_ref()
                .is_none_or(|selected| selected.contains(p.id.as_str()))
        })
        .map(|p| p.id.clone())
        .collect())
}
