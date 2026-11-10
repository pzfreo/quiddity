//! `quiddity <file.step>`: recognise a STEP file's features and print them as JSON: each
//! family's records at the top level, their `fingerprints`, and the versioned recognition
//! `document` (`crate::recognition`: per feature its family, Python record type, defining
//! faces and record). The part is recognised in its own frame and the records reported in the
//! file's coordinates; nonprincipal directions use vectors and record-owned origins. The
//! document gives the recognition frame as provenance. A part with no frame (no plane or cylinder, no material)
//! is recognised as placed in the file, with a warning on stderr.
//!
//! `quiddity correspond <old> <new>`: match two revisions and print the correspondence as JSON.
//! Each revision is a recognition result written by `quiddity <file.step>`, or a STEP file
//! (recognised on the spot).
//!
//! `quiddity parts <file.step>`: the file's distinct parts (index, name, face and edge counts,
//! placements) and the binding (the STEP text's sha256 and the PMI reader version) that face
//! and edge numbers refer to.
//!
//! `quiddity pmi read <file.step> [--part N]`: the semantic PMI of every part (or part N) and
//! the reader's findings, in the versioned JSON form of `crate::pmi_json`.
//!
//! `quiddity pmi check <file.step> <pmi.json>`: decodes a PMI document and checks it against
//! the file (binding, part indices and names, face and edge numbers, the model's invariants);
//! prints the document in its canonical form, or refuses naming the JSON path.
//!
//! `quiddity pmi write <file.step> <pmi.json> -o <out.step> [--mode add|replace|remove]
//! [--presentation refuse|remove]`: writes the document's parts (all in one `pmi::write`) into
//! the file as AP242 semantic PMI — beside each part's PMI (`add`, the default), in place of it
//! (`replace`), or removing it (`remove`, each part's `pmi` empty) — keeping every other byte.
//! The document must be bound to this file and reader (`pmi read`, `pmi check`). Replace and
//! remove take the presentation of the PMI they remove with it (`--presentation remove`, the
//! default) or refuse naming it (`--presentation refuse`). The output is written to a temporary
//! file beside `out.step` (gzipped when its name ends in `.gz`), read back with `pmi::read` and
//! compared semantically with what was written by `pmi::verify` (add: the part's PMI before plus exactly the
//! items written; replace and remove: exactly the items written, and nothing the reader consumed
//! for the part survives; every other part as before; no finding the input did not have), and
//! renamed to `out.step` only then; prints the write's report as JSON. On any refusal nothing is
//! created.
//!
//! Exit status: 0 with the JSON on stdout; 1 with the error on stderr when a file cannot be
//! read (an unreadable, truncated, incomplete or unsupported STEP file is refused, never
//! recognised as empty), a PMI document is refused, or a write is refused or does not verify;
//! 2 with the usage on stderr for bad arguments. The command line is parsed by clap:
//! `-h`/`--help` (on any command) prints the help on stdout and exits 0, `--version` the version.
//!
//! `quiddity brep <file.step> -o <out.step>`: writes the part's B-rep as read (an inverted
//! shell repaired) to a new AP242 file (`haecceity::brep_write`) with its semantic PMI, and
//! prints what the write reports as JSON, with the kernel's volume of each solid. A file of one
//! part carrying PMI is written in that part's own frame, where its PMI is anchored; PMI items
//! not carried (refused by the writer, or of a file of several parts) are listed with why. This
//! is new output, not an edit of the file (`pmi write` edits files in place).
//! `--pmi <pmi.json>` writes the PMI document's part PMI in place of the file's: the document
//! must be bound to this file (`pmi read`, `pmi check`), the file must have one part, and the
//! document must name it.
//!
//! `quiddity serve`: recognition and correspondence as a JSON-lines service on stdin and stdout
//! (`crate::serve` has the protocol); exits 0 at the end of the input.
//!
//! `quiddity completions <SHELL>`: a completion script for bash, zsh, fish, powershell or
//! elvish, on stdout.

use std::path::{Path, PathBuf};

use clap::{Args, CommandFactory, Parser, Subcommand};

use crate::kernel::p21;
use crate::kernel::pmi::write::{Mode, PresentationPolicy};
use crate::kernel::pmi::{self, PartId, PartPmi, Snapshot};
use crate::kernel::step::{PartDefinition, read_part_definitions};
use crate::part_input;
use crate::pmi_json::{self, Binding};
use crate::serve::{self, Failure};

#[cfg(feature = "json-import")]
#[path = "bin/quiddity/pmi_interchange.rs"]
mod pmi_interchange;

const AFTER_HELP: &str =
    "`quiddity <file.step>` prints JSON: each family's records, their `fingerprints`, and the
versioned recognition `document`. `quiddity serve` answers JSON-lines requests on stdin
(`recognise`, `correspond`), one response line each on stdout.
With `--features f3d`, part inputs also accept .f3d (geometry only).

Exit status: 0 with the JSON on stdout; 1 with the error on stderr when a file cannot be read
or an operation is refused; 2 for wrong usage. `quiddity <command> --help` gives a command's
arguments.";

/// The command line. Values that were always checked here (`--part`, `--mode`,
/// `--presentation`) are kept as text and checked by [`main`], with their own messages; only the
/// shape of the command line is clap's.
#[derive(Parser)]
#[command(
    name = "quiddity",
    bin_name = "quiddity",
    version,
    about = "Manufacturing feature recognition from STEP B-rep models",
    after_help = AFTER_HELP,
    args_conflicts_with_subcommands = true,
    arg_required_else_help = true,
    disable_help_subcommand = true
)]
struct Cli {
    /// recognise the part's features and print them as JSON
    #[arg(value_name = "FILE.STEP", allow_hyphen_values = true)]
    file: Option<String>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// scan STEP product ranges and write only the requested manifest
    Manifest {
        #[arg(value_name = "FILE.STEP")]
        file: String,
        #[arg(short = 'o', long = "output", value_name = "MANIFEST.JSON")]
        out: String,
        /// directory for a temporary mapped index (deleted after scanning)
        #[arg(long, value_name = "DIR")]
        index_dir: Option<PathBuf>,
    },
    /// recognise selected products in their own coordinates through a manifest
    Recognise {
        #[arg(value_name = "FILE.STEP")]
        file: String,
        #[arg(long, value_name = "MANIFEST.JSON")]
        manifest: String,
        #[arg(long, value_name = "DIR")]
        out_dir: PathBuf,
        #[arg(long, value_delimiter = ',', value_name = "p0,p7,…")]
        parts: Option<Vec<String>>,
        /// also read and hash the entire source
        #[arg(long)]
        verify_source: bool,
        /// use size-only staleness checks (also skips common/header hashes)
        #[arg(long)]
        no_product_hashes: bool,
        /// parse the whole source for shared geometry; memory grows with the entire file
        #[arg(long)]
        fallback_whole_file: bool,
    },
    /// match two revisions' features and print the correspondence as JSON
    Correspond {
        /// the old revision: a recognition result written by `quiddity <file.step>`, or a STEP
        /// file
        #[arg(value_name = "OLD.JSON|OLD.STEP", allow_hyphen_values = true)]
        old: String,
        /// the new revision, likewise
        #[arg(value_name = "NEW.JSON|NEW.STEP", allow_hyphen_values = true)]
        new: String,
    },
    /// print the part's recognition evidence as JSON
    Evidence {
        #[arg(value_name = "FILE.STEP", allow_hyphen_values = true)]
        file: String,
        /// in the part's own frame, with the frame given
        #[arg(long)]
        framed: bool,
    },
    /// print the part's feature fingerprints as JSON
    Fingerprints {
        #[arg(value_name = "FILE.STEP", allow_hyphen_values = true)]
        file: String,
    },
    /// print this build's versioned capabilities as JSON
    Capabilities,
    /// answer JSON-lines requests (`recognise`, `correspond`) on stdin, one response line each on
    /// stdout
    Serve,
    /// list the file's distinct parts and the binding that face and edge numbers refer to
    Parts {
        #[arg(value_name = "FILE.STEP", allow_hyphen_values = true)]
        file: String,
    },
    /// read, check and write AP242 semantic PMI
    #[command(subcommand_required = true, arg_required_else_help = true)]
    Pmi {
        #[command(subcommand)]
        command: Pmi,
    },
    /// write the B-rep and its PMI to a new AP242 file
    Brep(BrepArgs),
    /// print a shell completion script to stdout
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

#[derive(Subcommand)]
enum Pmi {
    /// print the semantic PMI of every part (or part N) and the reader's findings
    Read {
        #[arg(value_name = "FILE.STEP", allow_hyphen_values = true)]
        file: String,
        /// only part N, as `quiddity parts` lists them
        #[arg(long, value_name = "N", allow_hyphen_values = true)]
        part: Option<String>,
    },
    /// check a PMI document against the file and print it in its canonical form
    Check {
        #[arg(value_name = "FILE.STEP", allow_hyphen_values = true)]
        file: String,
        #[arg(value_name = "PMI.JSON", allow_hyphen_values = true)]
        json: String,
    },
    /// write a PMI document's parts into the file as AP242 semantic PMI, verified by reading it
    /// back
    Write(WriteArgs),
    /// PMI interchange: the versioned semantic and presentation JSON schemas
    #[cfg(feature = "json-import")]
    #[command(subcommand_required = true, arg_required_else_help = true)]
    Json {
        #[command(subcommand)]
        command: pmi_interchange::JsonCommand,
    },
}

/// Prints *message* as a usage error of the command at *path*, with that command's usage; 2.
fn usage_error(path: &[&str], message: &str) -> u8 {
    let mut command = Cli::command();
    command.build();
    let mut sub = &mut command;
    for name in path {
        sub = sub
            .find_subcommand_mut(name)
            .expect("the command line has the command");
    }
    eprintln!("quiddity: {message}\n{}", sub.render_usage());
    2
}

/// Run the Rust command line on arguments after the program name.
pub fn main(args: &[String]) -> u8 {
    let argv = std::iter::once("quiddity").chain(args.iter().map(String::as_str));
    let cli = match Cli::try_parse_from(argv) {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            return u8::try_from(e.exit_code()).unwrap_or(2);
        }
    };
    let command = match (cli.file, cli.command) {
        (Some(path), _) => {
            return print(recognise(&path).map_err(|failure| failure.to_string()));
        }
        (None, Some(command)) => command,
        (None, None) => return usage_error(&[], "a STEP file or a command is needed"),
    };
    let result = match command {
        Command::Manifest {
            file,
            out,
            index_dir,
        } => {
            return finish(manifest_write(&file, &out, index_dir));
        }
        Command::Recognise {
            file,
            manifest,
            out_dir,
            parts,
            verify_source,
            no_product_hashes,
            fallback_whole_file,
        } => {
            return finish(manifest_recognise(
                &file,
                &manifest,
                &out_dir,
                parts.as_deref(),
                verify_source,
                no_product_hashes,
                fallback_whole_file,
            ));
        }
        Command::Serve => {
            return match serve::serve(std::io::stdin().lock(), std::io::stdout().lock()) {
                Ok(()) => 0,
                Err(error) => {
                    eprintln!("quiddity: serve: {error}");
                    1
                }
            };
        }
        Command::Completions { shell } => {
            clap_complete::generate(
                shell,
                &mut Cli::command(),
                "quiddity",
                &mut std::io::stdout(),
            );
            return 0;
        }
        Command::Capabilities => Ok(crate::file_api::capabilities()),
        Command::Fingerprints { file } => crate::file_api::fingerprints_file(&file),
        Command::Evidence { file, framed } => crate::file_api::evidence_file(&file, framed),
        Command::Correspond { old, new } => {
            correspond(&old, &new).map_err(|failure| failure.to_string())
        }
        Command::Parts { file } => parts(&file),
        Command::Brep(b) => brep(&b),
        #[cfg(feature = "json-import")]
        Command::Pmi {
            command: Pmi::Json { command },
        } => pmi_interchange::run(command),
        Command::Pmi {
            command: Pmi::Read { file, part },
        } => match part.map(|n| n.parse::<usize>().map_err(|_| n)).transpose() {
            Ok(part) => pmi_read(&file, part),
            Err(n) => {
                return usage_error(
                    &["pmi", "read"],
                    &format!("--part takes a part index, not {n:?}"),
                );
            }
        },
        Command::Pmi {
            command: Pmi::Check { file, json },
        } => pmi_check(&file, &json),
        Command::Pmi {
            command: Pmi::Write(w),
        } => match w.options() {
            Ok((mode, policy)) => pmi_write(&w, mode, policy),
            Err(e) => return usage_error(&["pmi", "write"], &e),
        },
    };
    print(result)
}

/// The JSON on stdout (0), or the error on stderr (1).
fn print(result: Result<String, String>) -> u8 {
    match result {
        Ok(json) => {
            println!("{json}");
            0
        }
        Err(error) => {
            eprintln!("quiddity: {error}");
            1
        }
    }
}

/// Warnings, on stderr: geometry that did not resolve, a part frame that could not be inferred.
fn warn(warnings: Vec<String>) {
    for warning in warnings {
        eprintln!("quiddity: {warning}");
    }
}

fn recognise(path: &str) -> Result<String, Failure> {
    let (output, warning) = serve::recognise_file(path)?;
    warn(warning);
    Ok(serde_json::to_string_pretty(&output).expect("records serialise"))
}

fn correspond(old: &str, new: &str) -> Result<String, Failure> {
    let revision = |path| {
        let (fingerprints, warning) = serve::fingerprints_file(path)?;
        warn(warning);
        Ok::<_, Failure>(fingerprints)
    };
    let c = serve::correspond(&revision(old)?, &revision(new)?)?;
    Ok(serde_json::to_string_pretty(&c).expect("a correspondence serialises"))
}

/// Whether a file name says it is gzipped.
fn gzipped(path: &str) -> bool {
    path.to_lowercase().ends_with(".gz")
}

/// One source snapshot, with its STEP representation and original Fusion binding.
struct Input {
    step: Vec<u8>,
    fusion: Option<Vec<u8>>,
}

impl Input {
    fn binding_bytes(&self) -> &[u8] {
        self.fusion.as_deref().unwrap_or(&self.step)
    }
}

/// Geometry as STEP, gunzipped for STEP and converted for Fusion.
fn step_bytes(path: &str) -> Result<Input, String> {
    let raw = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    if part_input::is_f3d(path) {
        let step = part_input::fusion_step(&raw).map_err(|e| format!("{path}: {e}"))?;
        warn(vec![part_input::fusion_warning(path)]);
        return Ok(Input {
            step,
            fusion: Some(raw),
        });
    }
    if !gzipped(path) {
        return Ok(Input {
            step: raw,
            fusion: None,
        });
    }
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(&raw[..]), &mut out)
        .map_err(|e| format!("{path}: not gzip ({e})"))?;
    Ok(Input {
        step: out,
        fusion: None,
    })
}

/// A STEP file's text and its distinct parts.
fn part_definitions(path: &str) -> Result<(Input, Vec<PartDefinition>), String> {
    let bytes = step_bytes(path)?;
    let defs = read_part_definitions(&bytes.step).map_err(|e| format!("{path}: {e}"))?;
    Ok((bytes, defs))
}

fn parts(path: &str) -> Result<String, String> {
    let bytes = step_bytes(path)?;
    crate::file_api::parts(&bytes.step, bytes.binding_bytes()).map_err(|e| format!("{path}: {e}"))
}

fn pmi_read(path: &str, part: Option<usize>) -> Result<String, String> {
    let (bytes, defs) = part_definitions(path)?;
    if let Some(n) = part
        && n >= defs.len()
    {
        return Err(format!(
            "{path}: the file has {} parts (0 to {}); there is no part {n}",
            defs.len(),
            defs.len().saturating_sub(1)
        ));
    }
    let binding = Binding::of(bytes.binding_bytes());
    let doc = p21::Document::parse(bytes.step).map_err(|e| format!("{path}: {e}"))?;
    let read = crate::kernel::pmi::read(&doc, &defs).map_err(|e| format!("{path}: {e}"))?;
    let document = pmi_json::read_document(&read, &defs, binding, part);
    Ok(
        serde_json::to_string_pretty(&pmi_json::document_to_json(&document))
            .expect("JSON serialises"),
    )
}

fn pmi_check(path: &str, json: &str) -> Result<String, String> {
    let (bytes, defs) = part_definitions(path)?;
    let text = std::fs::read_to_string(json).map_err(|e| format!("{json}: {e}"))?;
    let value = pmi_json::parse(&text).map_err(|e| format!("{json}: not JSON ({e})"))?;
    let document = pmi_json::document_from_json(&value).map_err(|e| format!("{json}: {e}"))?;
    pmi_json::check_against(&document, bytes.binding_bytes(), &defs)
        .map_err(|e| format!("{json}: {e}"))?;
    Ok(
        serde_json::to_string_pretty(&pmi_json::document_to_json(&document))
            .expect("JSON serialises"),
    )
}

/// `brep`'s arguments.
#[derive(Args)]
struct BrepArgs {
    #[arg(value_name = "FILE.STEP")]
    step: String,
    /// the new AP242 file (gzipped when its name ends in .gz)
    #[arg(
        short = 'o',
        long = "output",
        value_name = "OUT.STEP",
        allow_hyphen_values = true
    )]
    out: String,
    /// write this PMI document's part PMI in place of the file's (bound to this file by `pmi
    /// read` or `pmi check`; the file must have one part)
    #[arg(long, value_name = "PMI.JSON", allow_hyphen_values = true)]
    pmi: Option<String>,
}

/// Write the part's B-rep to *out* (gzipped when its name ends in `.gz`); the write's report.
fn brep(args: &BrepArgs) -> Result<String, String> {
    use crate::kernel::brep_write::{WriteOptions, write_step};
    let (path, out) = (args.step.as_str(), args.out.as_str());
    let bytes = step_bytes(path)?;
    let items = |p: &PartPmi| pmi::write::differences_as_stated(&PartPmi::default(), p).len();
    // A PMI document given with --pmi is the written part's PMI, in place of the file's: it must
    // be bound to this file (`pmi read`, `pmi check`) and name its only part.
    let given = match &args.pmi {
        None => None,
        Some(json) => {
            let defs = read_part_definitions(&bytes.step).map_err(|e| format!("{path}: {e}"))?;
            let text = std::fs::read_to_string(json).map_err(|e| format!("{json}: {e}"))?;
            let value = pmi_json::parse(&text).map_err(|e| format!("{json}: not JSON ({e})"))?;
            let document =
                pmi_json::document_from_json(&value).map_err(|e| format!("{json}: {e}"))?;
            pmi_json::check_against(&document, bytes.binding_bytes(), &defs)
                .map_err(|e| format!("{json}: {e}"))?;
            if defs.len() != 1 {
                return Err(format!(
                    "{json}: {path} has {} parts; a write carries one part's PMI",
                    defs.len()
                ));
            }
            let [part] = <[_; 1]>::try_from(document.parts)
                .map_err(|_| format!("{json}: must name the file's one part (part 0)"))?;
            let read = crate::kernel::step::read_part(&bytes.step, &defs[0])
                .map_err(|e| format!("{path}: {e}"))?;
            Some((read, part.pmi))
        }
    };
    // Without --pmi, the file's PMI: a file of one part is written with it, in the part's own
    // frame; the PMI of a file of several parts is listed as not carried.
    let (given_part, given_pmi) = given.map_or((None, None), |(p, m)| (Some(p), Some(m)));
    let read = match given_part {
        Some(_) => None,
        None => read_part_definitions(&bytes.step).ok().and_then(|defs| {
            let doc = p21::Document::parse(bytes.step.clone()).ok()?;
            let read = crate::kernel::pmi::read(&doc, &defs).ok()?;
            Some((defs, read))
        }),
    };
    let mut not_carried = Vec::new();
    let (part, pmi) = match (given_part, &read) {
        (Some(part), _) => (part, given_pmi.as_ref()),
        (None, Some((defs, read))) if defs.len() == 1 && items(&read.parts[0]) > 0 => (
            crate::kernel::step::read_part(&bytes.step, &defs[0])
                .map_err(|e| format!("{path}: {e}"))?,
            Some(&read.parts[0]),
        ),
        (None, _) => {
            if let Some((_, read)) = &read {
                for (k, p) in read.parts.iter().enumerate() {
                    if items(p) > 0 {
                        not_carried.push(serde_json::json!({
                            "part": k,
                            "items": items(p),
                            "why": "the file has several parts; a write carries one part's PMI",
                        }));
                    }
                }
            }
            (
                crate::kernel::read_step(&bytes.step).map_err(|e| format!("{path}: {e}"))?,
                None,
            )
        }
    };
    let file_name = Path::new(path)
        .file_name()
        .map_or_else(|| "part".to_owned(), |n| n.to_string_lossy().into_owned());
    let lower = file_name.to_lowercase();
    let stem = [".step.gz", ".stp.gz", ".step", ".stp", ".f3d", ".gz"]
        .iter()
        .find(|s| lower.ends_with(*s))
        .map_or(file_name.as_str(), |s| {
            &file_name[..file_name.len() - s.len()]
        });
    let (written, report) =
        write_step(&part, &WriteOptions { name: stem, pmi }).map_err(|e| format!("{path}: {e}"))?;
    let file_bytes = if gzipped(out) {
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut enc, &written).map_err(|e| e.to_string())?;
        enc.finish().map_err(|e| e.to_string())?
    } else {
        written
    };
    std::fs::write(out, file_bytes).map_err(|e| format!("{out}: {e}"))?;
    let pmi_json = serde_json::json!({
        "carried_items": report.pmi.as_ref().map_or(0, |c| items(&c.written)),
        "refused": report.pmi.as_ref().map_or_else(Vec::new, |c| c
            .refused
            .iter()
            .map(|r| serde_json::json!({"item": format!("{:?}", r.item), "why": r.why}))
            .collect()),
        "parts_not_carried": not_carried,
    });
    let mut json = serde_json::json!({
        "solids": report.solids,
        "void_shells": report.void_shells,
        "faces": report.faces,
        "surface_model_faces": report.surface_model_faces,
        "edges": report.edges,
        "vertices": report.vertices,
        "pcurves_written": report.pcurves_written,
        "pcurves_not_written": report.pcurves_not_written,
        "surface_kinds_changed": report
            .surface_kinds_changed
            .iter()
            .map(|(face, kind)| serde_json::json!({"face": face, "kind": kind}))
            .collect::<Vec<_>>(),
        "face_order_preserved": report.face_order_preserved,
        "edge_order_preserved": report.edge_order_preserved,
        "placed_components": report.placed_components,
        "placements_flattened": report.placements_flattened,
        "products_renamed": report.products_renamed,
        "repairs_written": report.repairs_written,
        "pmi": pmi_json,
        // The kernel's volume of each solid, for checking the file against other readers.
        "solid_volumes": (0..part.solids.len())
            .map(|s| part.solid_mass(s).map(|(v, _)| v))
            .collect::<Vec<_>>(),
    });
    if !report.long_strings.is_empty() {
        json["long_strings"] = serde_json::json!(report.long_strings.iter().map(|w|
            serde_json::json!({"record": w.record, "encoded_bytes": w.encoded_bytes, "byte_offset": w.byte_offset, "message": w.to_string()})
        ).collect::<Vec<_>>());
    }
    Ok(serde_json::to_string_pretty(&json).expect("JSON serialises"))
}

/// `pmi write`'s arguments.
#[derive(Args)]
struct WriteArgs {
    #[arg(value_name = "FILE.STEP")]
    step: String,
    #[arg(value_name = "PMI.JSON")]
    json: String,
    /// the output file (gzipped when its name ends in .gz), written only when it verifies
    #[arg(
        short = 'o',
        long = "output",
        value_name = "OUT.STEP",
        allow_hyphen_values = true
    )]
    out: String,
    /// add (the default) beside each part's PMI, replace it, or remove it
    #[arg(long, value_name = "add|replace|remove", allow_hyphen_values = true)]
    mode: Option<String>,
    /// for replace and remove: remove the presentation of the PMI removed (the default), or
    /// refuse naming it
    #[arg(long, value_name = "refuse|remove", allow_hyphen_values = true)]
    presentation: Option<String>,
    /// a presentation JSON document to draw the written PMI from
    #[cfg(feature = "json-import")]
    #[arg(long, value_name = "FILE", allow_hyphen_values = true)]
    source_presentation: Option<String>,
}

impl WriteArgs {
    /// The mode and presentation policy; a usage error names what is wrong.
    fn options(&self) -> Result<(Mode, PresentationPolicy), String> {
        let mode = match self.mode.as_deref() {
            None | Some("add") => Mode::Add,
            Some("replace") => Mode::Replace,
            Some("remove") => Mode::Remove,
            Some(v) => return Err(format!("--mode takes add, replace or remove, not {v:?}")),
        };
        let policy = match self.presentation.as_deref() {
            None => None,
            Some("refuse") => Some(PresentationPolicy::Refuse),
            Some("remove") => Some(PresentationPolicy::RemovePresentation),
            Some(v) => return Err(format!("--presentation takes refuse or remove, not {v:?}")),
        };
        if mode == Mode::Add && policy.is_some() {
            return Err("--presentation applies to replace and remove, not add".into());
        }
        Ok((
            mode,
            policy.unwrap_or(PresentationPolicy::RemovePresentation),
        ))
    }
}

fn pmi_write(w: &WriteArgs, mode: Mode, policy: PresentationPolicy) -> Result<String, String> {
    if part_input::is_f3d(&w.step) {
        #[cfg(not(feature = "f3d"))]
        part_input::fusion_step(&[]).map_err(|e| format!("{}: {e}", w.step))?;
        return Err(format!(
            "{}: pmi write requires original STEP text; use `brep --pmi` to create a new AP242 STEP file from Fusion geometry",
            w.step
        ));
    }
    let (bytes, defs) = part_definitions(&w.step)?;
    let json = &w.json;
    let text = std::fs::read_to_string(json).map_err(|e| format!("{json}: {e}"))?;
    let value = pmi_json::parse(&text).map_err(|e| format!("{json}: not JSON ({e})"))?;
    let document = pmi_json::document_from_json(&value).map_err(|e| format!("{json}: {e}"))?;
    pmi_json::check_against(&document, bytes.binding_bytes(), &defs)
        .map_err(|e| format!("{json}: {e}"))?;
    if document.parts.is_empty() {
        return Err(format!("{json}: names no part; there is nothing to write"));
    }
    let input = Binding::of(bytes.binding_bytes());
    let step = &w.step;
    let doc = p21::Document::parse(bytes.step).map_err(|e| format!("{step}: {e}"))?;
    let before = crate::kernel::pmi::read(&doc, &defs).map_err(|e| format!("{step}: {e}"))?;
    // A standard is a set member, not an item: adding one the part already states (or stating
    // one twice) is that standard, so it is not written again.
    let pmi: Vec<(PartId, PartPmi)> = document
        .parts
        .iter()
        .map(|p| {
            let mut pmi = p.pmi.clone();
            if mode == Mode::Add {
                let mut stated = before
                    .parts
                    .get(p.part.0)
                    .map(|old| old.standards.clone())
                    .unwrap_or_default();
                pmi.standards.retain(|s| {
                    let new = !stated.contains(s);
                    if new {
                        stated.push(s.clone());
                    }
                    new
                });
            }
            (p.part, pmi)
        })
        .collect();
    #[cfg(feature = "json-import")]
    let source = if let Some(path) = &w.source_presentation {
        if pmi.len() != 1 {
            return Err("--source-presentation requires exactly one selected part".into());
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let value = crate::kernel::json::parse(&text).map_err(|e| format!("{path}: {e}"))?;
        let presentation = crate::kernel::json::presentation::from_value(value)
            .map_err(|e| format!("{path}: {e}"))?;
        vec![(pmi[0].0, presentation)]
    } else {
        Vec::new()
    };
    #[cfg(feature = "json-import")]
    let result = crate::kernel::pmi::write::write_with_presentation(
        &doc, &defs, &pmi, mode, policy, &source,
    );
    #[cfg(not(feature = "json-import"))]
    let result = crate::kernel::pmi::write(&doc, &defs, &pmi, mode, policy);
    let (edit, report) = result.map_err(|e| format!("{step}: write refused: {e}"))?;
    let written = doc
        .apply(&edit)
        .map_err(|e| format!("{step}: write refused: {e}"))?;

    // Written beside the destination, read back from there, and renamed only when it verifies.
    let out = &w.out;
    // An existing destination is replaced in place: through a symbolic link to its target, and
    // keeping its permissions.
    let dest = Path::new(out);
    let target: PathBuf = match std::fs::symlink_metadata(dest) {
        Ok(m) if m.file_type().is_symlink() => {
            std::fs::canonicalize(dest).map_err(|e| format!("{out}: {e}"))?
        }
        _ => dest.to_path_buf(),
    };
    let name = target
        .file_name()
        .ok_or_else(|| format!("{out}: not a file name"))?
        .to_string_lossy();
    let tmp: PathBuf = target.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let verified = (|| -> Result<(Binding, Vec<crate::kernel::pmi::Finding>), String> {
        let gz = gzipped(out);
        let file_bytes = if gz {
            let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            std::io::Write::write_all(&mut enc, &written.bytes).map_err(|e| e.to_string())?;
            enc.finish().map_err(|e| e.to_string())?
        } else {
            written.bytes.clone()
        };
        std::fs::write(&tmp, &file_bytes).map_err(|e| format!("{out}: {e}"))?;
        let raw = std::fs::read(&tmp).map_err(|e| format!("{out}: {e}"))?;
        let back = if gz {
            let mut v = Vec::new();
            std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(&raw[..]), &mut v)
                .map_err(|e| format!("{out}: {e}"))?;
            v
        } else {
            raw
        };
        if back != written.bytes {
            return Err("the file on disk is not what was written".into());
        }
        let defs1 = read_part_definitions(&back).map_err(|e| e.to_string())?;
        let doc1 = p21::Document::parse(back).map_err(|e| e.to_string())?;
        let after = crate::kernel::pmi::read(&doc1, &defs1).map_err(|e| e.to_string())?;
        let snapshot = |doc, defs, read| Snapshot { doc, defs, read };
        pmi::verify(
            snapshot(&doc, &defs, &before),
            &doc,
            &pmi,
            mode,
            snapshot(&doc1, &defs1, &after),
        )
        .into_result()?;
        Ok((Binding::of(doc1.bytes()), after.findings))
    })();
    let (output, findings) = match verified {
        Ok(v) => v,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!(
                "{out}: not written: the output does not verify: {e}"
            ));
        }
    };
    if let Ok(m) = std::fs::metadata(&target)
        && let Err(e) = std::fs::set_permissions(&tmp, m.permissions())
    {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{out}: {e}"));
    }
    if let Err(e) = std::fs::rename(&tmp, &target) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{out}: {e}"));
    }
    let mut parts: Vec<PartId> = pmi.iter().map(|(p, _)| *p).collect();
    parts.sort();
    let json =
        pmi_json::write_report_json(mode, policy, &parts, &input, &output, &report, &findings);
    Ok(serde_json::to_string_pretty(&json).expect("JSON serialises"))
}

/// File-producing commands have no stdout payload.
fn finish(result: Result<(), String>) -> u8 {
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("quiddity: {error}");
            1
        }
    }
}

fn manifest_write(path: &str, out: &str, index_dir: Option<PathBuf>) -> Result<(), String> {
    let options = crate::kernel::step::manifest::ScanOptions {
        index_dir,
        ..Default::default()
    };
    let manifest = crate::file_api::manifest(path, &options)?;
    std::fs::write(out, crate::file_api::json(&manifest)?).map_err(|e| format!("{out}: {e}"))
}

#[derive(serde::Serialize)]
struct ProductRefusal {
    product: String,
    reason: String,
    /// True only when a shared-geometry range refusal was recovered by an explicit fallback.
    recovered: bool,
}

fn manifest_recognise(
    path: &str,
    manifest_path: &str,
    out: &Path,
    parts: Option<&[String]>,
    verify_source: bool,
    no_product_hashes: bool,
    fallback: bool,
) -> Result<(), String> {
    use crate::kernel::step::{StepFile, manifest::Manifest};
    let bytes = std::fs::read(manifest_path).map_err(|e| format!("{manifest_path}: {e}"))?;
    let mut manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|e| format!("{manifest_path}: {e}"))?;
    let ids = crate::file_api::product_ids(&manifest, parts)?;
    let file = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    // Check compression even for an empty selection; never implicitly unpack the source.
    let mut magic = [0; 2];
    let mut input = &file;
    let n = std::io::Read::read(&mut input, &mut magic).map_err(|e| e.to_string())?;
    if manifest.source.compressed || (n == 2 && magic == [31, 139]) {
        return Err(format!(
            "{path}: compressed source cannot be read by ranges; supply an uncompressed source and rescan"
        ));
    }
    if file.metadata().map_err(|e| e.to_string())?.len() != manifest.source.size {
        return Err(format!("{path}: source size mismatch"));
    }
    if verify_source {
        manifest
            .verify_source(&file)
            .map_err(|e| format!("{path}: {e}"))?;
    }
    if no_product_hashes {
        manifest.common.sha256 = None;
        for p in &mut manifest.products {
            p.sha256 = None;
        }
    }
    std::fs::create_dir_all(out.join("products")).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut whole = None;
    let mut refusals = Vec::new();
    for id in &ids {
        let shared = manifest
            .refusals
            .iter()
            .filter(|r| r.products.contains(id))
            .map(|r| format!("{} (shared entity #{})", r.reason, r.entity))
            .collect::<Vec<_>>();
        let mut recovered = false;
        let result = if shared.is_empty() {
            crate::file_api::recognise_product(path, &manifest, id)
        } else if fallback {
            if whole.is_none() {
                whole = Some((|| -> Result<StepFile, String> {
                    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
                    // Bind the exact snapshot passed to the whole-file parser. Hash failures
                    // are refusals too, never an invitation to recognise stale geometry.
                    manifest
                        .verify_source(std::io::Cursor::new(&bytes))
                        .map_err(|e| format!("{path}: {e}"))?;
                    StepFile::read(&bytes).map_err(|e| format!("{path}: {e}"))
                })());
            }
            let product = manifest
                .products
                .iter()
                .find(|p| &p.id == id)
                .expect("selected id");
            whole
                .as_ref()
                .expect("fallback attempted")
                .as_ref()
                .map_err(|e| format!("product {id}: whole-file fallback failed: {e}"))
                .and_then(|whole| {
                    let index = whole
                        .parts
                        .iter()
                        .position(|p| p.product_definition == product.product_definition)
                        .ok_or_else(|| {
                            format!("product {id}: definition missing from whole source")
                        })?;
                    let definition = &whole.parts[index];
                    if definition.faces != product.faces
                        || definition.edges.len() as u64 != product.edge_count
                    {
                        return Err(format!(
                            "product {id}: face order or edge count differs from manifest"
                        ));
                    }
                    whole.part(index).map_err(|e| format!("product {id}: {e}"))
                })
                .and_then(|part| crate::file_api::recognise_part(&part, &manifest.source.sha256))
                .inspect(|_| recovered = true)
        } else {
            Err(format!(
                "product {id}: {}; retry with --fallback-whole-file (parses the entire source)",
                shared.join("; ")
            ))
        };
        let dest = out.join("products").join(format!("{id}.json"));
        match result {
            Ok(json) => {
                std::fs::write(&dest, json).map_err(|e| format!("{}: {e}", dest.display()))?;
                if recovered {
                    refusals.push(ProductRefusal {
                        product: id.clone(),
                        reason: shared.join("; "),
                        recovered: true,
                    });
                }
            }
            Err(reason) => {
                // A rerun must never leave an old success for a product now refused.
                match std::fs::remove_file(&dest) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(format!("{}: {e}", dest.display())),
                }
                eprintln!("quiddity: {reason}");
                refusals.push(ProductRefusal {
                    product: id.clone(),
                    reason,
                    recovered: false,
                });
            }
        }
    }
    let assembly = serde_json::json!({
        "source": manifest.source,
        "instances": manifest.instances.iter().filter(|i| ids.contains(&i.product)).collect::<Vec<_>>(),
    });
    for (name, json) in [
        ("assembly.json", crate::file_api::json(&assembly)?),
        ("refusals.json", crate::file_api::json(&refusals)?),
    ] {
        let dest = out.join(name);
        std::fs::write(&dest, json).map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    if refusals.iter().any(|r| !r.recovered) {
        return Err(format!(
            "product recognition refused; see {}",
            out.join("refusals.json").display()
        ));
    }
    Ok(())
}
