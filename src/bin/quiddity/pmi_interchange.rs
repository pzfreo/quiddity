//! Opt-in CLI for the crate's versioned semantic and presentation JSON schemas.
use super::*;
use crate::kernel::json::{self as interchange, Anchors};
use serde_json::{Value, json};

pub(super) fn run(args: &[String]) -> Result<String, String> {
    let (command, rest) = args.split_first().ok_or("missing json command")?;
    let result = match command.as_str() {
        "validate" => {
            let [path] = rest else {
                return Err("usage: quiddity pmi json validate <pmi-or-presentation.json>".into());
            };
            let value = read_json(path)?;
            if value["schema"] == "haecceity.presentation/1" {
                interchange::presentation::validate(&value)
            } else {
                interchange::validate(&value)
            }
            .map_err(|e| e.to_string())?;
            Ok(json!({"valid":true,"schema":value["schema"]}))
        }
        "export" | "presentation" => {
            let (path, options) = rest.split_first().ok_or("missing STEP input")?;
            let opts = options_parse(options, false)?;
            let (bytes, defs) = part_definitions(path)?;
            let part = choose_part(opts.part, &defs)?;
            let doc = p21::Document::parse(bytes.step).map_err(|e| e.to_string())?;
            let read = pmi::read(&doc, &defs).map_err(|e| e.to_string())?;
            let semantic = interchange::identify(
                read.parts[part].clone(),
                Some(&read.identifiers.parts[part]),
            )
            .map_err(|e| e.to_string())?;
            if command == "export" {
                interchange::to_value(&semantic).map_err(|e| e.to_string())
            } else {
                serde_json::to_value(
                    interchange::presentation::read(&doc, &read, PartId(part), &semantic)
                        .map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())
            }
        }
        "import" => {
            let [step, json_path, options @ ..] = rest else {
                return Err("usage: quiddity pmi json import <geometry.step> <pmi.json> -o <out.step> [--part N] [--anchors <anchors.json>] [--source-presentation FILE]".into());
            };
            if part_input::is_f3d(step) {
                return Err("json import requires original STEP geometry".into());
            }
            let opts = options_parse(options, true)?;
            let out = opts.out.ok_or("import requires -o <out.step>")?;
            let (bytes, defs) = part_definitions(step)?;
            let part = choose_part(opts.part, &defs)?;
            let doc = p21::Document::parse(bytes.step).map_err(|e| e.to_string())?;
            let mut anchors = Anchors::read(&doc, &defs[part]);
            if let Some(path) = opts.anchors {
                let support = read_json(path)?;
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
            let semantic = interchange::from_value(&read_json(json_path)?, &anchors)
                .map_err(|e| e.to_string())?;
            let expected = semantic.pmi.clone();
            let source = opts
                .source_presentation
                .map(|path| {
                    interchange::presentation::from_value(read_json(path)?)
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
        _ => Err(format!("unknown PMI JSON command {command:?}")),
    }?;
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}
struct Options<'a> {
    part: Option<usize>,
    out: Option<&'a str>,
    anchors: Option<&'a str>,
    source_presentation: Option<&'a str>,
}
fn options_parse(args: &[String], import: bool) -> Result<Options<'_>, String> {
    let mut opts = Options {
        part: None,
        out: None,
        anchors: None,
        source_presentation: None,
    };
    let mut rest = args;
    while let [flag, value, tail @ ..] = rest {
        match flag.as_str() {
            "--part" if opts.part.is_none() => {
                opts.part = Some(value.parse().map_err(|_| "--part must be an index")?)
            }
            "-o" if import && opts.out.is_none() => opts.out = Some(value),
            "--anchors" if import && opts.anchors.is_none() => opts.anchors = Some(value),
            "--source-presentation" if import && opts.source_presentation.is_none() => {
                opts.source_presentation = Some(value)
            }
            _ => return Err(format!("unknown or repeated option {flag:?}")),
        };
        rest = tail;
    }
    if !rest.is_empty() {
        return Err(format!("option {:?} needs a value", rest[0]));
    }
    Ok(opts)
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
