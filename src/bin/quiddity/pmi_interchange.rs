//! Opt-in CLI for the crate's versioned semantic and presentation JSON schemas.
use super::*;
use crate::kernel::json::{self as interchange, Anchors};
use serde_json::{Value, json};

/// `quiddity pmi json`'s commands.
#[derive(clap::Subcommand)]
pub(super) enum JsonCommand {
    /// validate a semantic PMI or presentation JSON document
    Validate {
        #[arg(value_name = "PMI-OR-PRESENTATION.JSON", allow_hyphen_values = true)]
        path: String,
    },
    /// print a part's semantic PMI as interchange JSON
    Export {
        #[arg(value_name = "FILE.STEP", allow_hyphen_values = true)]
        step: String,
        #[command(flatten)]
        part: PartArg,
    },
    /// print a part's PMI presentation as JSON
    Presentation {
        #[arg(value_name = "FILE.STEP", allow_hyphen_values = true)]
        step: String,
        #[command(flatten)]
        part: PartArg,
    },
    /// write interchange JSON into the STEP geometry as AP242 PMI, verified by reading it back
    Import {
        #[arg(value_name = "GEOMETRY.STEP", allow_hyphen_values = true)]
        step: String,
        #[arg(value_name = "PMI.JSON", allow_hyphen_values = true)]
        json: String,
        /// the output STEP file
        #[arg(short = 'o', value_name = "OUT.STEP", allow_hyphen_values = true)]
        out: String,
        #[command(flatten)]
        part: PartArg,
        /// a consumer-verified face namespace to bind the anchors to
        #[arg(
            long,
            value_name = "VERIFIED-FACE-NAMESPACE.JSON",
            allow_hyphen_values = true
        )]
        anchors: Option<String>,
        /// a presentation JSON document to draw the written PMI from
        #[arg(long, value_name = "FILE", allow_hyphen_values = true)]
        source_presentation: Option<String>,
    },
}

/// `--part N`, checked here.
#[derive(clap::Args)]
pub(super) struct PartArg {
    /// part N, as `quiddity parts` lists them (needed when the file has several)
    #[arg(long = "part", value_name = "N", allow_hyphen_values = true)]
    value: Option<String>,
}

impl PartArg {
    fn index(&self) -> Result<Option<usize>, String> {
        self.value
            .as_deref()
            .map(|v| v.parse().map_err(|_| "--part must be an index".to_string()))
            .transpose()
    }
}

pub(super) fn run(command: JsonCommand) -> Result<String, String> {
    let result = match command {
        JsonCommand::Validate { path } => {
            let value = read_json(&path)?;
            if value["schema"] == "haecceity.presentation/1" {
                interchange::presentation::validate(&value)
            } else {
                interchange::validate(&value)
            }
            .map_err(|e| e.to_string())?;
            Ok(json!({"valid":true,"schema":value["schema"]}))
        }
        JsonCommand::Export { step, part } => export(&step, &part, true),
        JsonCommand::Presentation { step, part } => export(&step, &part, false),
        JsonCommand::Import {
            step,
            json: json_path,
            out,
            part,
            anchors: anchors_path,
            source_presentation,
        } => {
            if part_input::is_f3d(&step) {
                return Err("json import requires original STEP geometry".into());
            }
            let part = part.index()?;
            let out = out.as_str();
            let step = step.as_str();
            let (bytes, defs) = part_definitions(step)?;
            let part = choose_part(part, &defs)?;
            let doc = p21::Document::parse(bytes.step).map_err(|e| e.to_string())?;
            let mut anchors = Anchors::read(&doc, &defs[part]);
            if let Some(path) = anchors_path {
                let support = read_json(&path)?;
                let schema = support["schema"]
                    .as_u64()
                    .and_then(|v| u8::try_from(v).ok())
                    .ok_or("anchors.schema must be 1 or 2")?;
                let faces = support["faces"]
                    .as_str()
                    .ok_or("anchors.faces must be a consumer-verified fingerprint")?;
                anchors
                    .bind_support(schema, faces)
                    .map_err(|e| e.to_string())?;
            }
            let semantic = interchange::from_value(&read_json(&json_path)?, &anchors)
                .map_err(|e| e.to_string())?;
            let expected = semantic.pmi.clone();
            let source = source_presentation
                .map(|path| {
                    interchange::presentation::from_value(read_json(&path)?)
                        .map(|p| (PartId(part), p))
                        .map_err(|e| format!("{path}: {e}"))
                })
                .transpose()?
                .into_iter()
                .collect::<Vec<_>>();
            let (output, report) = interchange::write_with_presentation(
                &doc,
                &defs,
                &[(PartId(part), semantic)],
                PresentationPolicy::RemovePresentation,
                &source,
            )
            .map_err(|e| e.to_string())?;
            let after = p21::Document::parse(output.clone()).map_err(|e| e.to_string())?;
            let after_defs = read_part_definitions(&output).map_err(|e| e.to_string())?;
            let after_read = pmi::read(&after, &after_defs).map_err(|e| e.to_string())?;
            let differences = interchange::identity_differences(&expected, &after_read.parts[part]);
            if !differences.is_empty() {
                return Err(format!("output does not verify: {differences:?}"));
            }
            atomic_write(out, &output)?;
            let mut result = json!({"schema":"haecceity.pmi/1","part":part,"output":out,"added":report.added,"differences":differences,"presentation_removed":report.presentation_removed});
            if !report.presentation_warnings.is_empty() {
                result["presentation_warnings"] = json!(report.presentation_warnings);
            }
            Ok(result)
        }
    }?;
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

/// `export` (the part's semantic PMI) or `presentation` (its presentation).
fn export(path: &str, part: &PartArg, semantic_only: bool) -> Result<Value, String> {
    let part = part.index()?;
    let (bytes, defs) = part_definitions(path)?;
    let part = choose_part(part, &defs)?;
    let doc = p21::Document::parse(bytes.step).map_err(|e| e.to_string())?;
    let read = pmi::read(&doc, &defs).map_err(|e| e.to_string())?;
    let semantic = interchange::identify(
        read.parts[part].clone(),
        Some(&read.identifiers.parts[part]),
    )
    .map_err(|e| e.to_string())?;
    if semantic_only {
        interchange::to_value(&semantic).map_err(|e| e.to_string())
    } else {
        serde_json::to_value(
            interchange::presentation::read(&doc, &read, PartId(part), &semantic)
                .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }
}
fn choose_part(selected: Option<usize>, defs: &[PartDefinition]) -> Result<usize, String> {
    match selected {
        Some(i) if i < defs.len() => Ok(i),
        Some(i) => Err(format!("unknown part {i}; {} parts", defs.len())),
        None if defs.len() == 1 => Ok(0),
        None => Err(format!(
            "file has {} parts; select one with --part N",
            defs.len()
        )),
    }
}
fn read_json(path: &str) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    // Match the existing CLI's duplicate-property refusal, not serde_json's last wins.
    interchange::parse(&text).map_err(|e| format!("{path}: {e}"))
}
fn atomic_write(out: &str, bytes: &[u8]) -> Result<(), String> {
    let target = match std::fs::symlink_metadata(out) {
        Ok(m) if m.file_type().is_symlink() => {
            std::fs::canonicalize(out).map_err(|e| e.to_string())?
        }
        _ => PathBuf::from(out),
    };
    let name = target
        .file_name()
        .ok_or("output must name a file")?
        .to_string_lossy();
    let tmp = target.with_file_name(format!(".{name}.{}.json.tmp", std::process::id()));
    let result = (|| {
        let file_bytes = if gzipped(out) {
            let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            std::io::Write::write_all(&mut enc, bytes).map_err(|e| e.to_string())?;
            enc.finish().map_err(|e| e.to_string())?
        } else {
            bytes.to_vec()
        };
        std::fs::write(&tmp, file_bytes).map_err(|e| e.to_string())?;
        if let Ok(metadata) = std::fs::metadata(&target) {
            std::fs::set_permissions(&tmp, metadata.permissions()).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, &target).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}
