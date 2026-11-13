# MFInstSeg face coverage (quiddity, optionally Analysis Situs)

This comparison scores defining-face unions against MFInstSeg labels. The committed result is a quiddity run of release `v0.5.1` (`3a20dceb099f9780a5c18e1429d673ab6d5bca29`) on the 9,375-model test split: the `face_claims` exporter from that tag, release build, four workers, on a 4-vCPU Ubuntu 24.04 x86_64 DigitalOcean droplet with nothing else running, with rustc 1.99.1, on 13 November 2026. All 9,375 models returned `ok`. Its raw sweep, scorer and label fingerprints are recorded, so the scores can be recomputed from the repository plus the external labels. The earlier macOS run at `1b7dee67` stays in the repository for history.

No Analysis Situs numbers are reported. The Analysis Situs sweep on the same test split is committed, though, as `runs/as-246e2ab-test.jsonl.gz`. It came from Analysis Situs `246e2ab` with OCCT 7.6.3, run on Linux on 6 November 2026, and all 9,375 models returned `ok`. A later quiddity sweep can therefore be scored against it without rebuilding or rerunning Analysis Situs; see Reproducibility below. The tooling to rerun Analysis Situs is also kept below.

| File in `docs/benchmarks/` | Contents |
|---|---|
| `mfinstseg-v0.5.1-3a20dceb.json` | Aggregate scores for the current quiddity test-split run (v0.5.1) |
| `mfinstseg-v0.5.1-3a20dceb.html` | Report generated from that JSON |
| `mfinstseg-main-1b7dee67.json`, `.html` | The earlier macOS run, kept for history |

## Metrics and limits

**These numbers are not comparable with `tests/fixtures/mfinstseg_baseline.json`.** The repo scorer (`examples/support/mfinstseg.rs`, used by `tools/update_mfinstseg_baseline.py` and `tests/internal/mfinstseg.rs`) matches instances one to one at inclusive IoU ≥ 0.5. This comparison instead measures union face coverage: any selected claim can credit a labelled instance, and a prediction may credit multiple instances. It does not evaluate dimensions or instance identity.

| Metric | Meaning |
|---|---|
| Recognition | Labelled instances with at least half their faces claimed |
| Claimed faces | Labelled target faces claimed, as a share of labelled target faces |
| False positives | Claimed faces outside the target classes, as a share of all claimed faces |
| False negatives | Labelled instances with no face claimed; not the complement of recognition |

The repo's `any_feature` excludes only `step_levels` and `risers`. This comparison's “Any feature” excludes twelve families: `flats`, `freeform_surfaces`, `plates`, `risers`, `step_levels`, `thin_wall_bodies`, `sheet_metal_bodies`, `polygonal_stock`, `circular_face_patterns`, `repeating_radial_profiles`, `cylindrical_profiles`, `turned_steps`.

Current scoring skips records carrying `superseded_by`, but keeps `source` records, as #217 does. Original record indices are preserved. Class 1 selects hole `/bottom == through` (#216); class 12 selects `flat`, `drill_point` or `unknown`, matching the repo mapping. Pooled Holes unions these selectors. When Analysis Situs is scored, its untyped hole set is reused in both class rows. O-ring-labelled faces receive no special neutral treatment.

Comparison mappings: Holes uses quiddity `holes` versus Analysis Situs drill-hole faces; Rounds uses `blends` plus `fillets` versus Analysis Situs blends; Any feature uses the selected quiddity union versus Analysis Situs cavities. In Stock, quiddity is the complement of that selected union (an unclaimed-face proxy), while Analysis Situs uses convex-hull faces. Both Overall rows reuse quiddity's Any feature union. Analysis Situs's first Overall row unions holes, blends and cavities; the second adds faces off its successfully recognized hull. These unions do not assign predicted instance identities.

Shafts uses only `turned_steps` as a proxy. There is no shaft class in MFInstSeg, so claims outside this taxonomy do not establish geometric errors.

Timing is **calling-thread CPU time**, with read and recognition measured separately and JSON serialization excluded. Thread CPU time is a weaker measure than instructions retired: it varies with the machine, its load and the run. The figures describe one run and are not a speed comparison. Analysis Situs, when run, reports process CPU including any reader worker threads and AAG construction, so the two are not comparable either.

## Running it

The external corpus must provide `steps/ID.step` and `labels/ID.json`; it is not redistributed here.

```sh
cargo build --example face_claims --release
target/release/examples/face_claims /path/to/MFInstSeg/steps tools/as_compare/test_ids.txt /tmp/qr.jsonl 4
```

Each line includes defining faces, aligned per-record `superseded_by`, hole `bottom`, status and CPU times. Unsupported thread clocks fail with a visible diagnostic. The exporter uses the platform's `libc::CLOCK_THREAD_CPUTIME_ID`, so it runs on Linux and macOS; on Windows it compiles but exits with a diagnostic.

To score Analysis Situs as well, build `as_recognise.cpp` against an existing Analysis Situs `246e2ab`, Mobius and OCCT 7.6.3 installation. With `P` the OCCT prefix, `MOBIUS` its prefix, and `L` Analysis Situs's `build/lin64/gxx/lib`, run from its checkout:

```sh
INC=$(find src/src/asiAlgo src/src/asiActiveData -type d | sed 's/^/-I/' | tr '\n' ' ')
OCCLIBS=$(ls $P/lib/libTK*.so | sed 's#.*/lib\(.*\)\.so#-l\1#' | tr '\n' ' ')
g++ -std=c++17 -O2 -w /path/to/quiddity/tools/as_compare/as_recognise.cpp -o as_recognise $INC \
  -I$P/include/opencascade -I$P/include/eigen3 -I$P/include -I$MOBIUS/include \
  -L$L -lasiAlgo -lasiActiveData \
  -L$MOBIUS/lib -lmobiusCascade -lmobiusGeom -lmobiusBSpl -lmobiusPoly -lmobiusCore \
  -L$P/lib $OCCLIBS -Wl,-rpath,$L:$MOBIUS/lib:$P/lib
python3 -I /path/to/quiddity/tools/as_compare/run_as.py /path/to/MFInstSeg/steps \
  /path/to/quiddity/tools/as_compare/test_ids.txt /tmp/as.jsonl 4 ./as_recognise
```

Score both sweeps, or use `-` for an unavailable tool (omitted from the scores):

```sh
python3 -I tools/as_compare/score.py /path/to/MFInstSeg/labels /tmp/qr.jsonl /tmp/as.jsonl /tmp/scores.json \
  --quiddity-revision REVISION
```

Failed or face-count-misaligned models count as claiming nothing. A refused recognizer contributes no claims; in particular, a refused hull does not contribute off-hull faces. Equal face counts alone do not prove identical face ordering between STEP readers. Missing metadata fails rather than silently scoring an old quiddity sweep as current.

## Reproducibility

The current test-split raw sweep is committed as `runs/v0.5.1-3a20dceb-test-qr.jsonl.gz` (about 1.7 MB). It contains 2,637 superseded hole records and 2,637 superseded boss records; the scorer skips them. The current `turned_steps` shaft proxy claims zero faces. These observations can be checked in the compressed JSONL.

Rescore the current run using that committed input and external labels:

```sh
python3 -I tools/as_compare/score.py /path/to/MFInstSeg/labels \
  tools/as_compare/runs/v0.5.1-3a20dceb-test-qr.jsonl.gz - /tmp/current-score.json \
  --quiddity-revision 3a20dceb099f9780a5c18e1429d673ab6d5bca29 --exporter-revision 3a20dceb
```

To score quiddity against the committed Analysis Situs sweep, pass that sweep in place of `-`. The sweep is gzip-compressed JSONL, about 0.7 MB:

```sh
python3 -I tools/as_compare/score.py /path/to/MFInstSeg/labels \
  /tmp/qr.jsonl tools/as_compare/runs/as-246e2ab-test.jsonl.gz /tmp/scores.json \
  --quiddity-revision REVISION
```

The Analysis Situs output is deterministic: its holes, blends and cavities output matched an October 2026 run on all 9,375 models. Its CPU timings are from that one Linux run.

The aggregate records the compressed sweep and scorer SHA-256 digests. `labels_sha256` is SHA-256 of the concatenation, in sorted ID order, of `ID:SHA256(label_file_bytes)\n` in UTF-8. The external labels must match that fingerprint to recreate these scores. Fresh recognition also requires the STEP corpus and recorded recognizer/exporter revisions; CPU timings vary with the machine and run.
The HTML can be regenerated from the committed aggregate:

```sh
python3 -I tools/as_compare/build_report.py \
  docs/benchmarks/mfinstseg-v0.5.1-3a20dceb.json docs/benchmarks/mfinstseg-v0.5.1-3a20dceb.html \
  tools/as_compare/runs/v0.5.1-3a20dceb-test-qr.jsonl.gz
```

Small **synthetic** inputs exercise the current scorer without redistributing dataset files:

```sh
python3 -I tools/as_compare/score.py tools/as_compare/fixtures/labels \
  tools/as_compare/fixtures/qr.jsonl tools/as_compare/fixtures/as.jsonl /tmp/fixture-score.json \
  --quiddity-revision synthetic
python3 -I tools/as_compare/test_score.py
```

CI runs `test_score.py` in the lint job.

## MFCAD++ semantic scoring

`face_claims` includes `face_sources`, the source STEP entity for each caller face.
Run `python3 -I tools/as_compare/score_mfcadpp.py STEPS QR.jsonl OUT.json
--quiddity-revision REV` to bind the numeric `ADVANCED_FACE` names through those
IDs. For readable imports, a missing, duplicate or invalid binding is an error,
never an assumed file order. An unreadable import claims nothing: its STEP label
multiset still supplies class denominators without assigning caller-face indices.
Missing or malformed label input is reported as unavailable/unreadable; no face
count is invented. Provenance hashes the adapter and the shared metric separately.
The adapter calls `score.py` for every comparison and selector; it adds a missed-face
ranking with unclaimed/body-only counts and overlapping body-family counts.
MFCAD++ names have no instance IDs: instance metrics are unavailable except for
stock's existing one-body comparison. No feature instances are inferred from labels.

Development samples use every tenth nonempty ID (10th, 20th, ...) of `train.txt`;
validation uses the same rule on `val.txt`. Develop on MFCAD++; use MFInstSeg
mainly to check generalisation.
Synthetic binding/metric tests: `python3 -I tools/as_compare/test_score_mfcadpp.py`.
