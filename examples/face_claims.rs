//! Recognise every model in a list and write one JSON line per model with each family's defining
//! faces, record by record, for face-level scoring against dataset labels.
//! `cargo run --release --example face_claims -- STEPS_DIR IDS_FILE OUT.jsonl [WORKERS]`.
// Internal diagnostic: use the exact engine implementation without exporting caller-space APIs.
#[path = "../src/engine.rs"]
mod engine;
pub use engine::*;
extern crate self as quiddity;
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

#[repr(C)]
struct Timespec {
    sec: i64,
    nsec: i64,
}
unsafe extern "C" {
    fn clock_gettime(clock: i32, ts: *mut Timespec) -> i32;
}
/// CPU seconds consumed by the calling thread (Linux CLOCK_THREAD_CPUTIME_ID); recognition is
/// single-threaded, so this is the model's own cost, unaffected by the other workers.
fn thread_cpu() -> f64 {
    let mut ts = Timespec { sec: 0, nsec: 0 };
    assert_eq!(unsafe { clock_gettime(3, &mut ts) }, 0);
    ts.sec as f64 + ts.nsec as f64 * 1e-9
}

fn sweep(path: &std::path::Path) -> serde_json::Value {
    let c0 = thread_cpu();
    let part = match quiddity::read_step_file(path) {
        Ok(part) => part,
        Err(e) => return serde_json::json!({"status": "unreadable", "message": e.to_string()}),
    };
    let faces = part.faces.len();
    let c1 = thread_cpu();
    let mut line = match catch_unwind(AssertUnwindSafe(|| quiddity::features::recognise(&part))) {
        Ok(features) => serde_json::json!({
            "status": "ok",
            "faces": faces,
            "defining": features.defining,
        }),
        Err(payload) => {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            serde_json::json!({"status": "panicked", "faces": faces, "message": message})
        }
    };
    let c2 = thread_cpu();
    line["read_cpu_s"] = (c1 - c0).into();
    line["recognise_cpu_s"] = (c2 - c1).into();
    line
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let steps = std::path::PathBuf::from(&args[1]);
    let ids: Vec<String> = std::fs::read_to_string(&args[2])
        .expect("ids")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    let out = Mutex::new(std::fs::File::create(&args[3]).expect("output"));
    let workers = args.get(4).map_or(4, |w| w.parse().expect("workers"));
    std::panic::set_hook(Box::new(|_| {})); // panics are recorded, not printed
    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            std::thread::Builder::new()
                .stack_size(256 << 20)
                .spawn_scoped(scope, || {
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(id) = ids.get(i) else { break };
                        let start = Instant::now();
                        let mut line = sweep(&steps.join(format!("{id}.step")));
                        line["id"] = id.as_str().into();
                        line["seconds"] = start.elapsed().as_secs_f64().into();
                        let mut out = out.lock().unwrap();
                        writeln!(out, "{line}").unwrap();
                        out.flush().unwrap();
                        if i.is_multiple_of(1000) {
                            eprintln!("{i}/{}", ids.len());
                        }
                    }
                })
                .unwrap();
        }
    });
}
