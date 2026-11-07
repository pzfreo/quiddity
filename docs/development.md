# Developing quiddity-rust

How the test suite establishes correctness, how the regression snapshots are kept, and how
to run the full checks. For using the library, see [the README](../README.md).

## Expected results

The regression record is the **Rust snapshot** in `tests/fixtures/rust-snapshot/`.
Python is no longer consulted for expected results, fixes or regeneration. Python-written
STEP/PMI files, legacy notes, old fingerprints and recorded helper arguments remain test
inputs. Historical verdicts and their evidence are preserved in
[the parity archive](history/python-parity.md).

Independent geometry, label, invariance, inventory-floor and NIST checks remain separate
requirements. OCCT measurements recorded through Python wrappers remain independent
references (including booleans, volumes, areas and bounds). A snapshot records behavior; it does not establish geometric correctness.
Do not remove output to improve metrics, lower a label/inventory floor, loosen a tolerance,
or regenerate expectations merely to make a failure pass. Investigate against the independent
references, then review an intentional behavior change and its decompressed snapshot diff.

## Regression snapshots

Commit source and input changes before generating, so provenance identifies a reproducible
Rust commit. `provenance.json` records its full commit hash, input hashes, and every
compressed snapshot hash. The maintainer tags releases; this tool creates no tags.
CI's Linux shards record each snapshot they verify; one job then checks that every committed snapshot has a producer and that provenance matches. Regeneration requires an
explicit write flag supplied by the tool and is forbidden in CI.

```sh
export QUIDDITY_CORPUS=/path/to/pinned/corpus
export CARGO_BUILD_JOBS=6 RUST_TEST_THREADS=6
cargo run -p snapshot-tool --release --
cargo run -p snapshot-tool --release -- --diff BASE_REF
cargo run -p snapshot-tool --release -- --check
cargo fmt --all --check
cargo clippy --workspace --all-targets --release -- -D warnings
QUIDDITY_CORPUS_REQUIRED=1 cargo test --workspace --release
```

`--diff` decompresses the expectations for review; use `--file corpus.json.gz` to narrow it.
Review record counts, defining/constituent faces, refusals, dimensions, order, frame and PMI
changes, together with the source change and independent tests. Commit regenerated gzip files
and provenance in the same change. `--check` regenerates into a temporary directory and
requires byte-identical compressed output on HEAD. CI runs `--check --coverage-only` on what the shards recorded: the Rust
comparisons retain their original numeric bands across platforms, and the produced-file
inventory must match exactly, so orphaned snapshots fail. Normal tests only read expectations.

The corpus snapshots the public recognition document and fingerprints, plus Rust kernel
inventory, masses, UV bounds and arcs. Legacy per-function corpus replays are retained only
for raw passages and alternative hole/fillet/chamfer options absent from the default document. Other replays cover fillet manifests, frames,
effective surfaces, outer profiles, sections, patches, passages, geometry proofs,
section-recess records/documents/publication, face levels/risers, reconciliation,
recognition evidence, degradation, volume probes and semantic PMI. Inputs that a former capture
could not export remain documented as unavailable, rather than being invented.

Comparison bands are unchanged: record numbers use 1e-6 absolute/relative; manifest fillet
records compare exactly; frames use 1e-9 for axes and 1e-9 of part size for origins; PMI uses
1e-9; kernel mass uses 1e-9 relative and analytic face boxes keep their 5e-3 + 1e-6 band and existing exemptions; probes retain exact empty, 1e-9 full, and 1e-6 fraction comparisons. Section proposal
ordering still allows only the existing round-off ties. Strings, refusal outcomes, structure,
and array lengths must agree. Compressed files have a fixed gzip timestamp.

## Independent references by family

**F** is the separately maintained historical Rust inventory floor, with positive coverage
for every listed family (`tests/internal/inventory_floor.rs`, `tests/fixtures/inventory_floor.json`).
Its loss allowances retain their analytic/OpenCascade evidence. **L** is the independent
MFInstSeg/MFCAD++ class mapping and MFInstSeg ratchet (`tests/internal/mfinstseg.rs`,
`tests/fixtures/mfinstseg_mapping.json`). Label mappings are coarse and do not validate every
subtype. The corpus motion suite (`tests/internal/invariance.rs`) additionally checks the recognition
and correspondence families it exercises; motion alone cannot detect a missing feature.
No new independent references are claimed for legacy value-only helpers.

| Family | Independent references |
|---|---|
| `angled_steps` | F; L |
| `blends` | F |
| `bosses` | F |
| `chamfers` | F; L; analytic rolled planes (`src/features/chamfers.rs`) |
| `channels` | F; L |
| `circular_blind_steps` | F; L; analytic supporting planes (`src/features/circular_blind_steps.rs`) |
| `circular_face_patterns` | F |
| `countersinks` | F |
| `double_d_bores` | F; analytic wall claims + motion (`tests/internal/evidence.rs`) |
| `edge_open_circular_pockets` | F; analytic floor/hole claims (`tests/internal/evidence.rs`) |
| `edge_open_prismatic_recesses` | F; analytic wall/floor claims (`tests/internal/evidence.rs`) |
| `fillets` | F; L; analytic rolled supports (`src/features/fillets.rs`) |
| `flats` | F |
| `freeform_surfaces` | F |
| `grooves` | F |
| `gusset_rib_patterns` | F |
| `gusset_ribs` | F; analytic cap/slant claims (`tests/internal/evidence.rs`) |
| `hole_patterns` | F |
| `holes` | F; L; analytic counterbores + motion (`tests/internal/tie_breaks.rs`) |
| `interior_voids` | F; analytic bounded-air fixtures (`src/features/interior_voids.rs`) |
| `modelled_threads` | F; analytic thread fixtures (`tests/internal/modelled_threads.rs`) |
| `oblique_through_steps` | F; L |
| `oriented_chamfers` | F |
| `oriented_slot_patterns` | F |
| `oriented_slots` | F; L |
| `pads` | F; analytic pad fixtures (`tests/internal/pads.rs`) |
| `paired_ramp_steps` | F; L |
| `plates` | F |
| `pocket_patterns` | F |
| `pockets` | F; L |
| `polygonal_bosses` | F |
| `polygonal_stock` | F |
| `prismatic_pockets` | F; L; analytic prism fixtures (`tests/internal/prismatic_pockets.rs`) |
| `rectangular_blind_slots` | F; L |
| `repeating_radial_profiles` | F |
| `risers` | F |
| `round_bottom_blind_slots` | F; L |
| `section_passages` | F; L |
| `section_recess_patterns` | F; member-face unions + motion (`tests/internal/section_recess_publication.rs`, `tests/internal/section_recess.rs`) |
| `section_recesses` | F; geometry publication and motion (`tests/internal/section_recess_publication.rs`, `tests/internal/section_recesses.rs`) |
| `sheet_metal_bodies` | F |
| `slot_patterns` | F |
| `slots` | F |
| `step_levels` | F |
| `thin_wall_bodies` | F |
| `through_steps` | F; L |
| `turned_steps` | F |
| face levels (legacy `recognise_face_levels`) | **snapshot only** |
| passages (legacy roster) | **snapshot only** |
| planar outer profiles | analytic directed supports, source-edge correspondence, motion (`tests/internal/outer_profiles.rs`) |
| effective surfaces | OCCT areas/perimeters and recovery lengths; material orientation, dependency consistency, motion (`tests/internal/effective_surfaces.rs`) |
| sections / cylindrical / end-surface geometry | OCCT clipped-cell/envelope volumes and face coverage; motion (`tests/internal/section_geometry.rs`, `tests/internal/section_recess_geometry.rs`, `tests/internal/section_recess_helpers.rs`); value-only helpers: **snapshot only** |
| face accounting | analytic stock/feature controls, motion (`src/body.rs`), complete golden/corpus partition (`tests/face_accounting.rs`), MFInstSeg stock/feature labels |
| frames | motion equivariance, inverse point transforms, caller/working-face identity (`tests/internal/frames.rs`) |
| correspondence | handwritten analytic revision intent (`tests/fixtures/revisions/expected.json`), motion (`tests/internal/correspondence.rs`, `tests/internal/invariance.rs`) |
| mass / face integrals | OpenCascade-measured corpus, body bounds/volume/area (`tests/occt_body_measurements.rs`), area and moment ledgers; analytic surfaces; live OCCT (`tests/internal/corpus.rs`, `crates/haecceity/tests/face_{areas,moments,integrals}.rs`, `tools/check_face_integrals.py`) |
| volume probes / face patches | OCCT common/probe volumes and exact face booleans (`crates/haecceity/tests/probes.rs`, `crates/haecceity/tests/patches.rs`, `tests/internal/sections.rs`), existing evidence verdicts |
| PMI read/write | NIST expectations, stated design intent, OCCT XCAF, analytic writer fixtures, semantic round trips (`crates/haecceity/tests/pmi_{read,write,roundtrip}.rs`, `tests/pmi_cli.rs`) |

OpenCascade and analytic ledgers now live in `tests/fixtures/references/`; their executable
checks and tolerances remain. Direct OCCT captures under `ap242/occt` and `ap242/written/occt`
are independent measurements, not Python recognition expectations. The read-only live
cross-checks (`tools/check_face_integrals.py`, `tools/check_pmi_occt.py`) remain available.

## Labelled accuracy

`examples/mfinstseg_score.rs` compiles the same private Rust engine as the library and
retains the original caller-space evaluation semantics and committed class mapping. No feature flag is required:

```sh
cargo run --release --example mfinstseg_score -- DATASET IDS.txt OUT.json 6
QUIDDITY_MFINSTSEG=/path/to/MFInstSeg cargo test --release --lib regression_tests::mfinstseg
```

MFInstSeg has `steps/` and `labels/`; for MFCAD++ pass its STEP directory and append
`--mfcadpp`. Only defining faces count. Identical face sets are deduplicated; instance
matching is one-to-one at IoU >= 0.5; empty denominators score zero. Incomplete instance
labels retain semantic metrics. Input failures remain misses. Dataset files stay external;
use `tools/update_mfinstseg_baseline.py` only for non-regressing improvements.

## Corpus

The 100-file corpus is pinned by path and SHA-256 in `tests/fixtures/corpus.json`. CI checks
out only those input files at its recorded input-repository revision; it does not run Python
recognition or capture steps. Export `QUIDDITY_CORPUS` locally. CI requires it; developer runs
without a corpus may skip corpus work. The full release suite must include it. Additional
NIST AP242 files can be checked by exporting `HAECCEITY_NIST_PMI` and
`HAECCEITY_NIST_PMI_REQUIRED=1`; committed NIST fixtures always run.

## Releases

The maintainer tags a release and publishes it on GitHub; nobody else does, and no session
publishes to crates.io or PyPI itself. A published release is the approval for both registries:

- **PyPI:** `python.yml` builds, tests and publishes the wheels and sdist (see
  [Python](python.md)).
- **crates.io:** `crates.yml` checks out the tag, refuses unless the tag, `quiddity`'s version
  and `haecceity`'s version agree, then publishes `haecceity` and `quiddity` in that order
  (Cargo waits until `haecceity` is in the index). A crate whose version is already on
  crates.io is skipped, so a rerun publishes only what is missing. The token is the repository
  secret `CARGO_REGISTRY_TOKEN`, a crates.io token scoped to publish-update for `quiddity` and
  `haecceity`.
- **Backfill:** a manual run of `crates.yml` with `tag` (e.g. `v0.4.8`) publishes an earlier
  release's crates from its tag. It needs the maintainer's word each time.

Pull requests that touch either crate run `cargo publish --dry-run -p haecceity -p quiddity`,
so a packaging break shows before a release.

## Layout

- `src/`: recognition, frames, feature records, fingerprints, correspondence and CLI.
- `crates/haecceity/`: STEP/Part 21, geometry, validity, integrals, drawings and semantic PMI.
- `tests/`: automatically discovered public integration tests.
- `tests/internal/`: private geometry/reference and snapshot unit tests, registered in `mod.rs`.
- `tests/fixtures/captured/`: preserved legacy test inputs and query arguments.
- `tests/fixtures/references/`: independent measured ledgers and motion exceptions.
- `tests/fixtures/rust-snapshot/`: Rust-owned expectations and provenance.
- `cargo run -p snapshot-tool --release --`: the single snapshot regeneration/check/review tool.
- `docs/history/python-parity.md`: archived verdicts, evidence and source commits.

Kernel limitations and historical findings remain documented in the source and reference
ledgers. Approximate boundaries, malformed solids, unsupported surfaces and ambiguous
geometry can yield explicit refusals or unproved claims. A successful parse is not a guarantee
that every family is recognised. Refer to the trust contract before consuming such claims.

See [Notes on matching OpenCascade](opencascade.md) for reader healing and boundary rules.
