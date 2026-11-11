//! Cheap deterministic smoke fuzzing. Each input runs in a killable subprocess so a
//! hang fails this test rather than leaving a detached thread running in CI.
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CASE_PATH: &str = "QUIDDITY_MUTATION_CASE";

#[test]
fn mutation_case() {
    let Some(path) = std::env::var_os(CASE_PATH) else {
        return;
    };
    let bytes = std::fs::read(path).unwrap();
    // Call the library directly: serve::respond would catch and hide a panic.
    match quiddity::serve::recognise_bytes("mutation.step", bytes) {
        Ok(_) => (),
        Err(error) => assert_eq!(error.code, "broken_step", "{error:?}"),
    }
}

#[test]
fn boundless_faces_are_refused() {
    for name in [
        "golden_simple_through_hole_no_bounds.step",
        "kissing_cylinders_knife_no_bounds.step",
    ] {
        let bytes = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/broken")
                .join(name),
        )
        .unwrap();
        let error = quiddity::serve::recognise_bytes(name, bytes).err().unwrap();
        assert_eq!(error.code, "broken_step");
        assert!(
            error.message.contains("face #17 has no readable bounds"),
            "{error}"
        );
    }
}

/// Reference and numeric token spans, excluding entity declarations,
/// strings, comments and the STEP header. Replacements always change one token.
fn tokens(text: &str) -> Vec<(usize, usize, bool)> {
    let mut spans = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if line.starts_with('#')
            && let Some(eq) = line.find('=')
        {
            let b = line.as_bytes();
            let mut i = eq + 1;
            while i < b.len() {
                if b[i] == b'\'' {
                    i += 1;
                    while i < b.len() {
                        if b[i] == b'\'' {
                            i += 1;
                            if i >= b.len() || b[i] != b'\'' {
                                break;
                            }
                        }
                        i += 1;
                    }
                } else if b[i] == b'#'
                    || b[i].is_ascii_digit()
                    || b[i] == b'-'
                    || (b[i] == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit))
                {
                    let start = i;
                    let reference = b[i] == b'#';
                    i += 1;
                    while i < b.len()
                        && (b[i].is_ascii_digit() || (!reference && b".Ee+-".contains(&b[i])))
                    {
                        i += 1;
                    }
                    spans.push((offset + start, offset + i, reference));
                } else {
                    i += 1;
                }
            }
        }
        offset += line.len();
    }
    spans
}

#[test]
fn single_token_mutations_do_not_panic_or_hang() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("case.step");
    let log = dir.path().join("case.log");
    let exe = std::env::current_exe().unwrap();
    let mut seed = 0x7175_6964_6469_7479_u64;
    for name in [
        "golden_simple_through_hole.step",
        "kissing_cylinders_knife.step",
        "turned_body_disc.step",
        "golden_straight_and_obround_slots.step",
    ] {
        let text = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(name),
        )
        .unwrap();
        let spans = tokens(&text);
        assert!(!spans.is_empty());
        for case in 0..64 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let &(start, end, reference) = &spans[seed as usize % spans.len()];
            let choices = if reference {
                ["#0", "#42", "#17", "#999999"]
            } else {
                ["0.", "-1.", "1.E30", "1.E-30"]
            };
            let mut replacement = choices[case % choices.len()];
            if replacement == &text[start..end] {
                replacement = choices[(case + 1) % choices.len()];
            }
            let mut mutated = text.clone();
            mutated.replace_range(start..end, replacement);
            std::fs::write(&input, mutated).unwrap();
            let output = std::fs::File::create(&log).unwrap();
            let mut child = Command::new(&exe)
                .args(["--exact", "mutation_case", "--nocapture"])
                .env(CASE_PATH, &input)
                .stdout(Stdio::from(output.try_clone().unwrap()))
                .stderr(Stdio::from(output))
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    assert!(
                        status.success(),
                        "{name} case {case}, token {} -> {replacement}:\n{}",
                        &text[start..end],
                        std::fs::read_to_string(&log).unwrap()
                    );
                    break;
                }
                if Instant::now() >= deadline {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!(
                        "{name} case {case}, token {} -> {replacement}: exceeded 10s",
                        &text[start..end]
                    );
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}
