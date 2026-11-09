//! JSON lines for tools/gvp.py; accepts STEP files or recursively scanned directories.
#[path = "gvp_support/mod.rs"]
#[allow(dead_code)]
mod support;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut paths = Vec::new();
    for arg in std::env::args_os().skip(1) {
        support::walk(std::path::Path::new(&arg), &mut paths)?;
    }
    paths.sort();
    paths.dedup();
    for path in paths {
        let row = match support::file(&path) {
            Ok(v) => v,
            Err(e) => serde_json::json!({"file":path,"error":e.to_string()}),
        };
        println!("{row}");
    }
    Ok(())
}
