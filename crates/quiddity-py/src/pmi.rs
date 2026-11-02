//! In-memory PMI interchange. JSON validation and verified STEP writes belong to haecceity.
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use quiddity::kernel::{
    json::{self, Anchors},
    p21::Document,
    pmi::{self, PartId},
    step::{PartDefinition, decode_step_file_bytes, read_part_definitions},
};
use serde_json::{Value, json as value};

pyo3::create_exception!(quiddity, PmiError, PyValueError);

type Result<T> = std::result::Result<T, Failure>;

struct Failure {
    message: String,
    findings: Vec<Value>,
}
impl Failure {
    fn new(error: impl std::fmt::Display) -> Self {
        Self {
            message: error.to_string(),
            findings: Vec::new(),
        }
    }
    fn raised(self, py: Python<'_>) -> PyErr {
        let error = PmiError::new_err(self.message);
        let findings = py
            .import("json")
            .and_then(|m| m.call_method1("loads", (Value::Array(self.findings).to_string(),)));
        match findings.and_then(|findings| error.value(py).setattr("findings", findings)) {
            Ok(()) => error,
            Err(error) => error,
        }
    }
}

fn source(
    input: super::Input,
    selected: Option<usize>,
) -> Result<(Document, Vec<PartDefinition>, usize)> {
    let bytes = match input {
        super::Input::Path(path) => {
            let raw = std::fs::read(&path).map_err(Failure::new)?;
            decode_step_file_bytes(std::path::Path::new(&path), raw).map_err(Failure::new)?
        }
        super::Input::Bytes(bytes) => bytes,
    };
    let parts = read_part_definitions(&bytes).map_err(Failure::new)?;
    let part = match selected {
        Some(i) if i < parts.len() => i,
        Some(i) => {
            return Err(Failure::new(format!(
                "unknown part {i}; {} parts",
                parts.len()
            )));
        }
        None if parts.len() == 1 => 0,
        None => {
            return Err(Failure::new(format!(
                "file has {} parts; select part explicitly",
                parts.len()
            )));
        }
    };
    Ok((Document::parse(bytes).map_err(Failure::new)?, parts, part))
}

fn findings(read: &pmi::PmiRead) -> Vec<Value> {
    read.findings
        .iter()
        .map(|f| {
            value!({
                "kind": f.kind.as_str(), "part": f.part.map(|p| p.0), "ids": f.ids,
                "entity": f.entity, "detail": f.detail,
            })
        })
        .collect()
}

fn text(value: &Value) -> Result<String> {
    serde_json::to_string(value).map_err(Failure::new)
}

fn read(
    input: super::Input,
    selected: Option<usize>,
    names: bool,
    presentation: bool,
) -> Result<String> {
    let (doc, parts, part) = source(input, selected)?;
    let read = pmi::read(&doc, &parts).map_err(Failure::new)?;
    let result = (|| {
        let mut semantic = json::identify(
            read.parts[part].clone(),
            Some(&read.identifiers.parts[part]),
        )?;
        semantic.anchor_bindings.clear();
        if presentation {
            serde_json::to_value(json::presentation::read(
                &doc,
                &read,
                PartId(part),
                &semantic,
            )?)
            .map_err(json::Error::from)
        } else if names {
            json::to_value_with_face_names(&semantic, &Anchors::read(&doc, &parts[part]))
        } else {
            json::to_value(&semantic)
        }
    })();
    text(&result.map_err(|error| Failure {
        message: error.to_string(),
        findings: findings(&read),
    })?)
}

#[pyfunction]
#[pyo3(signature = (step, part=None, names=false, presentation=false))]
fn pmi_read(
    py: Python<'_>,
    step: &Bound<'_, PyAny>,
    part: Option<usize>,
    names: bool,
    presentation: bool,
) -> PyResult<String> {
    let input = super::Input::extract(step)?;
    py.detach(|| read(input, part, names, presentation))
        .map_err(|e| e.raised(py))
}

/// Preserve the canonical writer report and expose its warning-bearing collections as a list.
fn writer_findings(writer: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    for category in [
        "long_strings",
        "kept_unconsumed",
        "preexisting_violations",
        "preexisting_rule_violations",
        "practice_schema_conflicts",
        "repairs",
        "edition_rewrites",
        "left_unreferenced",
        "note_migrations",
        "note_presentation_removed",
    ] {
        if let Some(items) = writer[category].as_array() {
            for item in items {
                let mut finding = item.clone();
                finding["category"] = value!(category);
                out.push(finding);
            }
        }
    }
    if let Some(warnings) = writer["presentation_warnings"].as_array() {
        out.extend(
            warnings
                .iter()
                .map(|message| value!({"category": "presentation_warnings", "message": message})),
        );
    }
    if let Some(schema) = writer.get("edition_undetermined") {
        out.push(
            value!({"category": "edition_undetermined", "file_schema": schema,
            "validated_against": writer["validated_against"]}),
        );
    }
    out
}

fn import_report(
    doc: &Document,
    output: &[u8],
    selected: usize,
    expected: &pmi::PartPmi,
    before: &pmi::PmiRead,
    report: &pmi::write::WriteReport,
) -> json::Result<Value> {
    let after_doc = Document::parse(output.to_vec()).map_err(|e| json::Error(e.to_string()))?;
    let after_parts = read_part_definitions(output).map_err(|e| json::Error(e.to_string()))?;
    let after = pmi::read(&after_doc, &after_parts).map_err(|e| json::Error(e.to_string()))?;
    let differences = json::identity_differences(expected, &after.parts[selected]);
    if !differences.is_empty() {
        return Err(json::Error(format!(
            "output does not verify: {differences:?}"
        )));
    }
    let writer = quiddity::pmi_json::write_report_json(
        pmi::Mode::Replace,
        pmi::write::PresentationPolicy::RemovePresentation,
        &[PartId(selected)],
        &quiddity::pmi_json::Binding::of(doc.bytes()),
        &quiddity::pmi_json::Binding::of(output),
        report,
        &after.findings,
    );
    Ok(value!({
        "schema": "haecceity.pmi/1", "part": selected, "added": report.added,
        "differences": differences, "presentation_removed": report.presentation_removed,
        "read_findings": findings(before), "output_findings": findings(&after),
        "writer_findings": writer_findings(&writer), "writer": writer,
    }))
}

#[pyfunction]
#[pyo3(signature = (geometry, document, part=None, report=false, presentation=None))]
fn pmi_write<'py>(
    py: Python<'py>,
    geometry: &Bound<'_, PyAny>,
    document: String,
    part: Option<usize>,
    report: bool,
    presentation: Option<String>,
) -> PyResult<(Bound<'py, PyBytes>, Option<String>)> {
    let input = super::Input::extract(geometry)?;
    let (output, details) = py
        .detach(|| {
            let value = json::parse(&document).map_err(Failure::new)?;
            let (doc, parts, selected) = source(input, part)?;
            let read = pmi::read(&doc, &parts).map_err(Failure::new)?;
            let written = (|| {
                // Validation shares geometry-independent writer refusals with the CLI; decoding
                // resolves topology against this exact geometry snapshot before writing.
                json::validate(&value)?;
                let semantic = json::from_value(&value, &Anchors::read(&doc, &parts[selected]))?;
                let requested = [(PartId(selected), semantic)];
                // write_preserving already verifies exact model, requirement ids and units by
                // reading the candidate back. Presentation is enabled by the dependency defaults.
                let presentation = presentation
                    .as_deref()
                    .map(|text| {
                        let value = json::parse(text)?;
                        Ok::<_, json::Error>((
                            PartId(selected),
                            json::presentation::from_value(value)?,
                        ))
                    })
                    .transpose()?
                    .into_iter()
                    .collect::<Vec<_>>();
                let (output, written) = json::write_with_presentation(
                    &doc,
                    &parts,
                    &requested,
                    pmi::write::PresentationPolicy::RemovePresentation,
                    &presentation,
                )?;
                let details = if report {
                    Some(serde_json::to_string(&import_report(
                        &doc,
                        &output,
                        selected,
                        &requested[0].1.pmi,
                        &read,
                        &written,
                    )?)?)
                } else {
                    None
                };
                Ok::<_, json::Error>((output, details))
            })();
            written.map_err(|error| Failure {
                message: error.to_string(),
                findings: findings(&read),
            })
        })
        .map_err(|e| e.raised(py))?;
    Ok((PyBytes::new(py, &output), details))
}

#[pyfunction]
fn pmi_validate(py: Python<'_>, document: String) -> PyResult<()> {
    py.detach(|| {
        let value = json::parse(&document)?;
        if value["schema"] == "haecceity.presentation/1" {
            json::presentation::validate(&value)
        } else {
            json::validate(&value)
        }
    })
    .map_err(|e| Failure::new(e).raised(py))
}

pub(super) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("PmiError", m.py().get_type::<PmiError>())?;
    m.add_function(wrap_pyfunction!(pmi_read, m)?)?;
    m.add_function(wrap_pyfunction!(pmi_write, m)?)?;
    m.add_function(wrap_pyfunction!(pmi_validate, m)?)?;
    m.add("PMI_SCHEMA", json::PMI_SCHEMA)?;
    m.add("PRESENTATION_SCHEMA", json::PRESENTATION_SCHEMA)?;
    Ok(())
}
