# E5 current-main development baseline

Issue #369, 2026-09-06. Production source is main `8730db54803ff08103d6cbc12d41339008828679`
(Quiddity `0.2.2.dev0`). This refresh does not change recognition or taxonomy.

The complete published MFCAD++ test selection evaluates 2,493 of 2,500 models, with
the same seven documented invalid inputs. The canonical report records each input
hash, exact score counts, environment and runtime:
[`effectiveness-mfcadpp-2500-main-8730db5.json`](effectiveness-mfcadpp-2500-main-8730db5.json).
The taxonomy remains version 13 and recognition uses raw/caller coordinates.

| Family | Labelled faces | Covered faces | Coverage | Defining recall | Uncovered |
| --- | ---: | ---: | ---: | ---: | ---: |
| Triangular passage | 3,194 | 2,522 | 0.7896 | 0.7336 | 672 |
| Rectangular passage | 4,382 | 3,725 | 0.8501 | 0.6305 | 657 |
| Six-sided passage | 6,645 | 4,820 | 0.7254 | 0.6921 | 1,825 |
| Triangular pocket | 3,892 | 3,589 | 0.9221 | 0.6835 | 303 |
| Rectangular pocket | 4,895 | 4,618 | 0.9434 | 0.7140 | 277 |
| Six-sided pocket | 5,707 | 5,368 | 0.9406 | 0.7995 | 339 |
| Circular-end pocket | 4,536 | 4,164 | 0.9180 | 0.6920 | 372 |

Passages account for 3,154 uncovered faces versus 1,291 across these four pocket
families. These are all-family coverage residuals, **not** reference-only gap faces,
family-attributable recognition rates or guaranteed recoverable geometry. MFCAD++
does not supply native feature-instance labels; no instance-detection claim follows
from its connected same-label proxies.

## Next decision

Prioritise a new, corpus-independent polygonal-passage proof, led by six-sided
passages. Intact free-in-plane pockets have already shipped; repeating that
hypothesis is not justified by this residual. Preserve the same SectionRecess
geometry truth and exact same-run face/body authority. Discovery must not use labels
or infer missing walls merely because a class name suggests a polygon.

The first-500 pocket audit finds 30 triangular, 48 rectangular and 46 six-sided
uncovered faces. A geometry-first split-coplanar-mouth experiment recovers only
three new triangular faces, so it is a bounded later improvement rather than the
highest-value production increment. Separate small passage experiments test unique
mouth pairing and exact mixed line/circle mouths; their limited first-500 reach
does not establish a solution to the six-sided residual.

## Reproduction

```console
PYTHONPATH=src python tools/run_effectiveness_baseline.py mfcadpp \
  /path/to/MFCAD++_dataset/step/test \
  --dataset-version 'MFCAD++ published test split; DOI 10.17034/d1fec5a0-8c10-4630-b02e-b92dc81df823' \
  --taxonomy docs/benchmarks/effectiveness-taxonomy-v13.json \
  --limit 2500 --allow-invalid --canonical --workers 4 \
  --output /tmp/effectiveness-mfcadpp-2500-main-8730db5.json
```

Run on the pinned source. Checkpoint reuse was used only to publish the completed
authority-matched rows into the repository; no recognition was repeated and the
result is byte-identical to the initial completed report. Runtime values describe
this shared development host and are not a controlled performance comparison.
MFInstSeg was not read, run or inspected.
