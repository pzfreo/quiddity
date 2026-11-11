# Contributing to quiddity

Contributions are welcome: bug reports, fixes, features and documentation.

## Rust-first

The regression record is the Rust snapshot in `tests/fixtures/rust-snapshot/`. Python is
no longer consulted. Python-written STEP/PMI, old notes/fingerprints and helper arguments
remain inputs; they do not supply expected recognition results.

Use the independent references in [docs/development.md](docs/development.md#independent-references-by-family)
to establish correctness. Keep OpenCascade, analytic, label, motion, inventory-floor and NIST
checks and their bands intact. Do not discard output to improve a metric or lower a floor.

For an intentional output change, commit source/input changes, export `QUIDDITY_CORPUS`,
then run `cargo run -p snapshot-tool --release --`. Review
`cargo run -p snapshot-tool --release -- --diff BASE_REF` alongside the implementation and evidence;
commit the compressed expectations and provenance. The tool records the source Rust commit
as a full commit hash (an ancestor of HEAD). Releases are tagged by the maintainer. `cargo run -p snapshot-tool --release -- --check` must reproduce HEAD exactly.
Normal tests cannot accept a changed output. Do not hand-edit snapshots or restore a Python
oracle. Historical verdicts live in [the archive](docs/history/python-parity.md).

Public integration tests under `tests/` are discovered automatically. Register private-engine
unit tests in `tests/internal/mod.rs`; they can exercise caller-space algorithms without
exposing a public feature. The engine uses ordinary Rust modules, so `cargo fmt --all` covers it.

Before review, export `CARGO_BUILD_JOBS=6 RUST_TEST_THREADS=6` and run:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --release -- -D warnings
QUIDDITY_CORPUS_REQUIRED=1 cargo test --workspace --release
```

## Test files and data

Add only files you created yourself or whose licence allows redistribution, and record each
file's source and licence beside it (a fixture README or NOTICE entry). Never add customer,
confidential or NDA-covered files, or geometry extracted from them, to the repository, to
issues or to pull requests. For a bug seen on such a part, share a reduced synthetic part that
reproduces it instead.

## Developer Certificate of Origin

Contributions are accepted under the project's licence, Apache-2.0 (its section 5 covers
contributions). Each commit must be signed off to certify the
[Developer Certificate of Origin 1.1](https://developercertificate.org/): that you wrote the
change, or otherwise have the right to submit it under that licence. Sign off with
`git commit -s`, which adds a line with your real name and email:

```
Signed-off-by: Your Name <you@example.com>
```

A check on pull requests from forks confirms every commit is signed off. To sign off commits
already made, `git fetch upstream && git rebase --signoff upstream/main` (with `upstream` this repository) and force-push the branch.
