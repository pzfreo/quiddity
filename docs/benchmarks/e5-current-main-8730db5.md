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

### Partial-outlet hypothesis boundary

A one-complete-mouth passage experiment initially used the largest wall extent as
the far end. Independent ADR review rejected it: subtracting a half-width stock
step from a rectangular through-hole leaves one wall ending before the others.
An empty prism and two empty end slabs still pass, but the resulting closed
section fabricates support above that short wall. ADR 0016 rejects this envelope;
ADR 0019 does not permit silently varying physical support.

Two stricter first-500 diagnostics each add **zero** all-family covered faces:
requiring complete original wall-face support over the whole proposed prism, and
requiring the entire far polygon boundary on actual coplanar exterior stock
patches. The latter accepts an authored split-outlet positive and rejects the
stepped-stock negative. These are ceilings for the tested hypotheses, not proof
that every remaining passage is unsupported. No passage envelope change is
promoted. A broader interrupted or nonplanar termination needs its own geometric
diagnosis and an explicit truthful contract, not looser extent tolerances.

### Selected bounded increment: split coplanar pocket mouths

The geometry-first prototype completed the full 2,500 selection. It adds seven
native candidates, comprising 39 constituent faces; all 39 have the corresponding
polygonal-pocket labels. Of these, 14 faces were absent from all accepted baseline
evidence: seven triangular and seven six-sided pocket faces. This measures the
candidate opportunity, not yet a full after-change aggregate score: final ledger
arbitration and regression checks remain necessary.

| Model | Added candidate constituents | Previously uncovered constituents |
| --- | --- | --- |
| 10046 | 6, 7, 24, 25 | 7, 24, 25 |
| 13326 | 8, 9, 12, 25, 26 | — |
| 14240 | 35, 37, 39, 43 | 35, 37, 39, 43 |
| 14368 | 19, 20, 21, 31, 32 | — |
| 15902 | 10, 11, 12, 15, 19, 21, 22 | — |
| 22569 | 57, 58, 59, 60, 61, 62, 64 | 57, 58, 59, 60, 61, 62, 64 |
| 22898 | 10, 11, 12, 13, 14, 21, 22 | — |

These are MFCAD++ development-input indices, not persistent public face IDs. Labels
were read only after geometric candidate construction. The production change
retains the exact existing floor/section/volume proofs, requires mouth context for
every wall and one owner for all consulted faces, and excludes stock patches from
feature evidence. Authored tests cover split/unsplit equivalence, rigid placement,
scale, missing context, stepped-stock refusal and public evidence publication.
The modest gain does not close #369 or establish a solution for its larger passage
residual.

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
