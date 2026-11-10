//! Audit import repairs over every STEP input, including read failures. Never writes inputs.
//! cargo run -p haecceity -j 6 --release --example repair_audit -- tests/fixtures "$QUIDDITY_CORPUS"
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
fn walk(path: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            walk(&entry?.path(), out)?;
        }
    } else if path.file_name().is_some_and(|s| {
        let s = s.to_string_lossy().to_ascii_lowercase();
        [".step", ".stp", ".step.gz", ".stp.gz"]
            .iter()
            .any(|e| s.ends_with(e))
    }) {
        out.push(path.to_owned());
    }
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut paths = Vec::new();
    for arg in std::env::args_os().skip(1) {
        walk(Path::new(&arg), &mut paths)?;
    }
    paths.sort();
    paths.dedup();
    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..6 {
            scope.spawn(|| loop {
                let i=next.fetch_add(1,Ordering::Relaxed);
                let Some(path)=paths.get(i) else {break};
                let row=match haecceity::read_step_file(path) {
                    Ok(part) => serde_json::json!({"file":path,"solids":part.solids.len(),
                        "valid_solids":(0..part.solids.len()).filter(|&s| part.solid_is_valid(s)).count(),
                        "repairs":part.input_repairs()}),
                    Err(error)=>serde_json::json!({"file":path,"error":error.to_string()}),
                };
                println!("{row}");
            });
        }
    });
    Ok(())
}
