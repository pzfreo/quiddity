# quiddity vs Analysis Situs on MFInstSeg

This directory compares quiddity with Analysis Situs's open-source feature recognisers: drill holes, blends, cavities, convex hull and shafts. Both tools are scored face by face against MFInstSeg's labels.

The published results are in `docs/benchmarks/`:

| File | Contents |
|---|---|
| `mfinstseg-vs-analysis-situs-82bb3ba.html` | The report |
| `mfinstseg-vs-analysis-situs-82bb3ba.json` | Scores for both tools on the 9,375-model test split (`test_ids.txt`) |
| `mfinstseg-vs-analysis-situs-82bb3ba-full-corpus.json` | quiddity alone on all 62,495 models, scored with the earlier three-comparison version of `score.py` |
| `mfinstseg-vs-analysis-situs-alpha1.json` | October's alpha.1 run, used for the report's "Since October" section |

## Metrics

All four metrics are computed the same way for both tools:

| Metric | Meaning |
|---|---|
| Recognition | Labelled instances with at least half their faces claimed |
| Claimed faces | Labelled target faces claimed |
| False positives | Claimed faces outside the target classes, as a share of all claimed faces |
| False negatives | Labelled instances with no face claimed |

Other scoring rules:
- quiddity is scored on each record's defining faces.
- Body-level families (flats, plates, risers, step levels and similar) are not counted as feature claims.

## Running it

1. **Run quiddity.** Write one JSON line per model with every family's defining faces and the in-process CPU time:

   ```sh
   cargo run --release --example face_claims -- STEPS_DIR tools/as_compare/test_ids.txt qr.jsonl 4
   ```

2. **Build the Analysis Situs driver** against an Analysis Situs build (`246e2ab`), its Mobius dependency and OCCT 7.6.3. With `P` set to the OCCT/third-party prefix and `L` set to Analysis Situs's `build/lin64/gxx/lib`:

   ```sh
   INC=$(find src/src/asiAlgo src/src/asiActiveData -type d | sed 's/^/-I/' | tr '\n' ' ')
   OCCLIBS=$(ls $P/lib/libTK*.so | sed 's#.*/lib\(.*\)\.so#-l\1#' | tr '\n' ' ')
   g++ -std=c++17 -O2 -w as_recognise.cpp -o as_recognise $INC \
     -I$P/include/opencascade -I$P/include/eigen3 -I$P/include -I$MOBIUS/include \
     -L$L -lasiAlgo -lasiActiveData \
     -L$MOBIUS/lib -lmobiusCascade -lmobiusGeom -lmobiusBSpl -lmobiusPoly -lmobiusCore \
     -L$P/lib $OCCLIBS -Wl,-rpath,$L:$MOBIUS/lib:$P/lib
   ```

   Run this from the Analysis Situs checkout, so that `src/` resolves.

3. **Run Analysis Situs.** `as_recognise ID FILE.step` prints one JSON line per model with each recogniser's 0-based face indices (`TopExp::MapShapes` order, which matches quiddity's) and its timings:

   ```sh
   python3 -I run_as.py STEPS_DIR test_ids.txt as.jsonl 4 ./as_recognise
   ```

4. **Score and build the report:**

   ```sh
   python3 -I score.py LABELS_DIR qr.jsonl as.jsonl score.json
   python3 -I build_report.py score.json FULL_CORPUS_SCORE.json ALPHA1_SCORE.json \
     test_ids.txt qr.jsonl as.jsonl report.html
   ```

   A model either tool fails to read, or reads with a different face count from the labels, counts as claiming nothing.

## Reproducibility

From the stored sweep outputs:
- `score.py` reproduces `mfinstseg-vs-analysis-situs-82bb3ba.json` exactly.
- `build_report.py` reproduces the report; only the footer changed.
