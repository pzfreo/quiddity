# quiddity vs Analysis Situs on MFInstSeg

This comparison scores defining-face unions against MFInstSeg labels. The historical results use **quiddity 0.4.8, revision `82bb3ba`, before #216 and #217**, and Analysis Situs `246e2ab` / OCCT 7.6.3. They are not current-main results. The separately labelled current rerun uses main `1b7dee670475531b7bd7d6d0bedefd096328815d` plus the fixed `face_claims` exporter at `1ebfcef7` on this branch (macOS, six workers, 8 November 2026). All 9,375 test models returned `ok`. The full corpus was not rerun; its scores remain historical.

| File in `docs/benchmarks/` | Contents |
|---|---|
| `mfinstseg-vs-analysis-situs-82bb3ba.html` | Report generated from committed aggregate scores |
| `mfinstseg-vs-analysis-situs-82bb3ba.json` | Historical 9,375-model test split, both tools |
| `mfinstseg-vs-analysis-situs-82bb3ba-full-corpus.json` | Historical 62,495-model quiddity run, earlier three-comparison scorer |
| `mfinstseg-vs-analysis-situs-alpha1.json` | Archival October alpha.1 result; different schema |
| `mfinstseg-vs-analysis-situs-main-1b7dee67.json` | Current quiddity test-split rerun; no fresh Analysis Situs run |

## Metrics and limits

**These numbers are not comparable with `tests/fixtures/mfinstseg_baseline.json`.** The repo scorer (`examples/support/mfinstseg.rs`, used by `tools/update_mfinstseg_baseline.py` and `tests/internal/mfinstseg.rs`) matches instances one to one at inclusive IoU ≥ 0.5. This comparison instead measures union face coverage: any selected claim can credit a labelled instance, and a prediction may credit multiple instances. It does not evaluate dimensions or instance identity.

| Metric | Meaning |
|---|---|
| Recognition | Labelled instances with at least half their faces claimed |
| Claimed faces | Labelled target faces claimed, as a share of labelled target faces |
| False positives | Claimed faces outside the target classes, as a share of all claimed faces |
| False negatives | Labelled instances with no face claimed; not the complement of recognition |

The repo's `any_feature` excludes only `step_levels` and `risers`. This comparison's “Any feature” excludes twelve families: `flats`, `freeform_surfaces`, `plates`, `risers`, `step_levels`, `thin_wall_bodies`, `sheet_metal_bodies`, `polygonal_stock`, `circular_face_patterns`, `repeating_radial_profiles`, `cylindrical_profiles`, `turned_steps`.

Current scoring skips records carrying `superseded_by`, but keeps `source` records, as #217 does. Original record indices are preserved. Class 1 selects hole `/bottom == through` (#216); class 12 selects `flat`, `drill_point` or `unknown`, matching the repo mapping. Pooled Holes unions these selectors. The class rows reuse Analysis Situs's untyped hole set. Historical results did not apply these rules. O-ring-labelled faces receive no special neutral treatment.

The old explanations about floors remaining context and O-ring duplicate holes being “by design” have been removed. #216 adds eligible flat floors; #217 marks duplicate groove holes and bosses. The historical quiddity Shafts entry erroneously used every feature family against an empty target. It has been removed: the missing raw sweep prevents a replacement historical count. Current scoring uses `turned_steps` only as a shaft proxy. There is no shaft class in MFInstSeg, so claims outside this taxonomy do not establish geometric errors.

Timing scopes differ: quiddity uses **calling-thread CPU**, while Analysis Situs uses **process CPU**, including any reader worker threads. Analysis Situs recognition includes AAG construction. Current quiddity recognition excludes JSON serialization; the old exporter included it. The workloads and clock scopes do not constitute a controlled speed comparison.

## Running it

The external corpus must provide `steps/ID.step` and `labels/ID.json`; it is not redistributed here.

```sh
cargo build --example face_claims --release
target/release/examples/face_claims /path/to/MFInstSeg/steps tools/as_compare/test_ids.txt /tmp/qr.jsonl 4
```

Each line includes defining faces, aligned per-record `superseded_by`, hole `bottom`, status and CPU times. Unsupported thread clocks fail with a visible diagnostic. The exporter uses the platform's `libc::CLOCK_THREAD_CPUTIME_ID`.

Analysis Situs is not installed locally and was not rebuilt or rerun for this correction. To run it elsewhere, build `as_recognise.cpp` against an existing Analysis Situs `246e2ab`, Mobius and OCCT 7.6.3 installation. With `P` the OCCT prefix, `MOBIUS` its prefix, and `L` Analysis Situs's `build/lin64/gxx/lib`, run from its checkout:

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

Failed or face-count-misaligned models count as claiming nothing. Equal face counts alone do not prove identical face ordering between STEP readers. Missing metadata fails rather than silently scoring an old quiddity sweep as current.

## Reproducibility

Historical raw sweeps are not committed. The historical aggregates cannot be reproduced from this branch, and the full-corpus JSON came from an earlier scorer whose exact revision is not recorded. Recreating those measurements requires the old raw sweeps, external dataset, original tool revisions and original scorer. No claim of exact score reproduction or October output identity is made.

The current test-split raw sweep is committed as `runs/main-1b7dee67-test-qr.jsonl.gz` (about 1 MB). It contains 2,637 superseded hole records and 2,637 superseded boss records; the scorer skips them. The current `turned_steps` shaft proxy claims zero faces. These observations can be checked in the compressed JSONL.

Rescore the current run using that committed input and external labels:

```sh
python3 -I tools/as_compare/score.py /path/to/MFInstSeg/labels \
  tools/as_compare/runs/main-1b7dee67-test-qr.jsonl.gz - /tmp/current-score.json \
  --quiddity-revision 1b7dee670475531b7bd7d6d0bedefd096328815d --exporter-revision 1ebfcef7
```

The aggregate records the compressed sweep and scorer SHA-256 digests. `labels_sha256` is SHA-256 of the concatenation, in sorted ID order, of `ID:SHA256(label_file_bytes)\n` in UTF-8. The external labels must match that fingerprint to recreate these scores. Fresh recognition also requires the STEP corpus and recorded recognizer/exporter revisions; CPU timings vary with the machine and run. No Analysis Situs raw sweep is available here.

The HTML can be regenerated from committed aggregates:

```sh
python3 -I tools/as_compare/build_report.py \
  docs/benchmarks/mfinstseg-vs-analysis-situs-82bb3ba.json \
  docs/benchmarks/mfinstseg-vs-analysis-situs-82bb3ba-full-corpus.json \
  docs/benchmarks/mfinstseg-vs-analysis-situs-main-1b7dee67.json \
  docs/benchmarks/mfinstseg-vs-analysis-situs-82bb3ba.html
```

Small **synthetic** inputs exercise the current scorer without redistributing dataset files:

```sh
python3 -I tools/as_compare/score.py tools/as_compare/fixtures/labels \
  tools/as_compare/fixtures/qr.jsonl tools/as_compare/fixtures/as.jsonl /tmp/fixture-score.json \
  --quiddity-revision synthetic
python3 -I tools/as_compare/test_score.py
```
