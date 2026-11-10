//! JSON lines for tools/gvp.py; accepts STEP files or recursively scanned directories.
#[path = "gvp_support/mod.rs"]
#[allow(dead_code)]
mod support;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut paths = Vec::new();
    let mut jobs = 1usize;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--jobs" {
            jobs = args
                .next()
                .ok_or("--jobs requires a count")?
                .to_string_lossy()
                .parse()?;
            if !(1..=6).contains(&jobs) {
                return Err("--jobs must be between 1 and 6".into());
            }
        } else {
            support::walk(std::path::Path::new(&arg), &mut paths)?;
        }
    }
    paths.sort();
    paths.dedup();
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(path) = paths.get(i) else { break };
                    let row = match support::file(path) {
                        Ok(v) => v,
                        Err(e) => serde_json::json!({"file":path,"error":e.to_string()}),
                    };
                    println!("{row}");
                }
            });
        }
    });
    Ok(())
}
