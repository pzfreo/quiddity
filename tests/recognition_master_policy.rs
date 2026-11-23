//! The reverse-policy reference was captured on Apple silicon from the detached
//! ba425f16 probe before splitting recognition from the kernel's mass API.
#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use std::io::Read;
use std::path::Path;
use std::process::Command;

use flate2::read::MultiGzDecoder;
use sha2::{Digest, Sha256};

#[test]
fn recognition_matches_the_reverse_probe_byte_for_byte() {
    let Some(corpus) = std::env::var_os("QUIDDITY_CORPUS") else {
        assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
        return;
    };
    let work = tempfile::tempdir().unwrap();
    for (name, expected) in [
        (
            "cgb202",
            "a1a0d8d3004061d2ade2b4b697fea410d73c00cc88401118abf8cd9a62888df8",
        ),
        (
            "cgb243",
            "0c9ae8db90946320a1f31f56662c6bb15499cbc6133d9c0e314d4768c02239fa",
        ),
    ] {
        let file = std::fs::File::open(
            Path::new(&corpus).join(format!("cadgenbench_inputs/{name}.step.gz")),
        )
        .unwrap();
        let mut bytes = Vec::new();
        MultiGzDecoder::new(file).read_to_end(&mut bytes).unwrap();
        let path = work.path().join(format!("{name}.step"));
        std::fs::write(&path, bytes).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_quiddity"))
            .arg(path)
            .output()
            .unwrap();
        assert!(output.status.success(), "{name}: {:?}", output.stderr);
        assert_eq!(
            format!("{:x}", Sha256::digest(&output.stdout)),
            expected,
            "{name}"
        );
    }
}
