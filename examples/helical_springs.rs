//! Inspect spring measurements and whole-solid claims (optional distinct part index).
#[path = "../src/engine.rs"]
mod engine;
pub use engine::*;
extern crate self as quiddity;
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let part = if let Some(index) = args.get(1) {
        kernel::step::StepFile::read(&std::fs::read(&args[0]).expect("read"))
            .expect("STEP")
            .part(index.parse().expect("part index"))
            .expect("part")
    } else {
        quiddity::read_step_file(std::path::Path::new(&args[0])).expect("STEP")
    };
    println!(
        "{}",
        serde_json::to_string_pretty(
            &features::helical_springs::discover(&features::Context::new(&part))
                .iter()
                .map(|o| &o.record)
                .collect::<Vec<_>>()
        )
        .unwrap()
    );
}
