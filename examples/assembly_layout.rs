//! Generate reproducible assembly layouts and streaming scale fixtures.
#[path = "support/assembly_layout.rs"]
mod engine;
use clap::Parser;
use std::{fs::File, io::BufWriter, path::PathBuf};
#[derive(Parser)]
struct Args {
    source: PathBuf,
    #[arg(short, long)]
    output: PathBuf,
    #[arg(long, default_value = "contiguous")]
    layout: engine::Layout,
    #[arg(long, default_value_t = 4)]
    runs: usize,
    #[arg(long, default_value_t = 400)]
    seed: u64,
    #[arg(long)]
    scale: bool,
    #[arg(long, default_value_t = 1)]
    distinct: usize,
    #[arg(long, default_value_t = 1)]
    instances: usize,
    #[arg(long, default_value_t = 0)]
    target_bytes: u64,
    /// Defaults to OUTPUT.expected.json.
    #[arg(long)]
    sidecar: Option<PathBuf>,
}
fn main() -> engine::Result<()> {
    let args = Args::parse();
    if args.source == args.output {
        return Err("source and output must differ".into());
    }
    let bytes = std::fs::read(&args.source)?;
    let options = engine::Options {
        layout: args.layout,
        runs: args.runs,
        seed: args.seed,
    };
    let output = BufWriter::new(File::create(&args.output)?);
    if args.scale {
        let sidecar = args.sidecar.unwrap_or_else(|| {
            let mut p = args.output.as_os_str().to_owned();
            p.push(".expected.json");
            PathBuf::from(p)
        });
        engine::scale(
            &bytes,
            options,
            engine::Scale {
                distinct: args.distinct,
                instances: args.instances,
                target_bytes: args.target_bytes,
            },
            output,
            BufWriter::new(File::create(sidecar)?),
        )
    } else {
        engine::rewrite(&bytes, options, output)
    }
}
