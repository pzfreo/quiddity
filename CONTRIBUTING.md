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

## Public Rust API and releases

Before 1.0, any breaking public API change requires a **minor** version bump
(`0.4.x` → `0.5.0`); patch releases must remain source compatible. Adding fields
or variants to exhaustive public types, changing signatures, removing exports,
changing defaults, or moving an API behind a feature can break callers. State the
break and migration in CHANGELOG's `Breaking (Rust API)` section. Release tooling
changes the Cargo versions; do not disguise a break as a patch release.

Use `#[non_exhaustive]` on extensible results, reports, errors and options. Keep
construction possible through meaningful constructors/builders or `Default` plus
public field assignment. Closed mathematical and interchange value types may stay
exhaustive: extending them then requires a minor release. Preserve the default
feature surface and check supported feature-off builds too. See the maintained
[type inventory and migration examples](docs/rust-api.md).

CI pins cargo-semver-checks 0.51.0 and compares haecceity and quiddity with each
crate's latest stable crates.io version, with default features plus JSON. The lint
job also compiles supported feature-off builds.
Every run uses patch lints, even when Cargo versions await release. A non-empty
`### Breaking (Rust API)` subsection under `## Unreleased` is the sanctioned
breaking override. It must be accompanied by `Pending version: X.Y.Z` under
Unreleased, at least the next minor over each published baseline before 1.0
(at least the next major after 1.0), and no lower than the manifest version.
Document the actual break and caller migration there. Empty/commented sections
and historical releases grant no override. Without that section, API breaks fail
regardless of the manifest version. With it, completed API-lint failures are
reported and accepted for the declared pending release; build/tool failures fail.
This also applies while current = published. The release PR moves those entries under
`## X.Y.Z (date)`; while X.Y.Z is the manifest version and is not yet published, that
section's Breaking (Rust API) subsection is the override. CI also runs cargo-audit against Cargo.lock.

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
