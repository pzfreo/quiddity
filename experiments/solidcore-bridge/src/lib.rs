//! Shared helpers for the phase 0 prototype.

use std::io::Read;

/// A STEP file's bytes, gunzipped when the name ends in `.gz`.
pub fn step_bytes(path: &std::path::Path) -> std::io::Result<Vec<u8>> {
    let raw = std::fs::read(path)?;
    if path.extension().is_some_and(|e| e == "gz") {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(raw.as_slice()).read_to_end(&mut out)?;
        Ok(out)
    } else {
        Ok(raw)
    }
}
