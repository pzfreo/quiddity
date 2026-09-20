# MFInstSeg full-test effectiveness baseline at f2cfe4f

This is the canonical aggregate-only MFInstSeg transfer baseline required by Epic 0005 E0
([#293](https://github.com/pzfreo/quiddity/issues/293)). No individual model geometry, face,
candidate anatomy or refusal trace was inspected. The immutable machine-readable authority is
[`effectiveness-mfinstseg-9373-f2cfe4f.json`](effectiveness-mfinstseg-9373-f2cfe4f.json), SHA-256
`6edb3da42315790934fac342bc80ccd774c5d8bf1558b25213757169200dd1ea`.

## Authority and selection

- Package: Quiddity 0.3.2.dev0, clean scorer commit
  `f2cfe4f50700da59e5542bceceac0d68746141b1`.
- Mapping: format 1, taxonomy v13, SHA-256
  `bf03e2edd716b096c5c695df456e02856a0434acbbefd01ec3372c8475fad42e`.
- Dataset: authenticated original MFInstSeg `steps/` and `labels/` trees with the upstream AAGNet
  train/validation/test partitions.
- Selection: unique test ID in lexical order. The published test file has 9,375 unique rows; five
  IDs also occur in another split and are excluded, leaving 9,373. The selected-ID SHA-256 is
  `4c8f56fec3e5b980011d112061124bec9e31ec7afdc32b221939452e7fdb5b40`.
- Result: 9,373 selected, loaded and evaluated; zero invalid; one model with no accepted physical
  record.
- Environment: macOS 26.6.2 arm64, Python 3.14.7, build123d 0.11.1, OCP 7.9.3.1.
- Runtime: 1,858.68 aggregate model-seconds; median 0.183 s/model, p95 0.382 s/model, maximum
  2.907 s/model. Parallel wall time is not a per-model performance claim.

Exact reproduction command:

```bash
uv run python tools/run_effectiveness_baseline.py \
  mfinstseg /absolute/path/to/MFInstSeg \
  --partition-root /absolute/path/to/AAGNet/MFInstseg_partition \
  --dataset-version published-original \
  --taxonomy docs/benchmarks/effectiveness-taxonomy-v13.json \
  --canonical \
  --workers 0 \
  --checkpoint-dir .cache/effectiveness/mfinstseg-9373-f2cfe4f \
  --output docs/benchmarks/effectiveness-mfinstseg-9373-f2cfe4f.json
```

The report validates and hashes every selected STEP/label pair before recognition. Its checkpoint
authority binds the commit, importable source, exact taxonomy bytes, dataset/version, selected IDs
and sources, recognition frame and invalid policy. Publication rechecks that authority and creates
the report without overwriting existing evidence.

## Exact class results

Every percentage below retains its numerator and denominator in the canonical JSON. Precision is
one-vs-class for each mapped family, so a family mapped to several dataset classes contributes a
separate denominator to each row. The rows must not be summed into a blended score. `partial`,
`unsupported` and `incomparable` retain their declared taxonomy meanings.

| class | MFInstSeg name | status | defining precision | defining recall | face coverage | instance recall |
| ---: | --- | --- | ---: | ---: | ---: | ---: |
| 0 | Chamfer | supported | 1522/1893 (80.4%) | 1522/3567 (42.67%) | 2288/3567 (64.14%) | 1364/3014 (45.26%) |
| 1 | Through hole | supported | 3367/9809 (34.33%) | 3367/3438 (97.93%) | 3404/3438 (99.01%) | 3032/3060 (99.08%) |
| 2 | Triangular passage | supported | 8492/32972 (25.76%) | 8492/10406 (81.61%) | 9019/10406 (86.67%) | 2619/3126 (83.78%) |
| 3 | Rectangular passage | supported | 9263/33672 (27.51%) | 9263/13615 (68.04%) | 11901/13615 (87.41%) | 2144/3075 (69.72%) |
| 4 | 6-sided passage | supported | 15748/32972 (47.76%) | 15748/20172 (78.07%) | 16410/20172 (81.35%) | 2418/3049 (79.3%) |
| 5 | Triangular through slot | unsupported | 0/0 (—) | 0/1428 (0%) | 820/1428 (57.42%) | 0/642 (0%) |
| 6 | Rectangular through slot | partial | 891/2555 (34.87%) | 891/2138 (41.67%) | 1920/2138 (89.8%) | 441/627 (70.33%) |
| 7 | Circular through slot | unsupported | 0/0 (—) | 0/700 (0%) | 465/700 (66.43%) | 0/604 (0%) |
| 8 | Rectangular through step | supported | 2695/2696 (99.96%) | 2695/6859 (39.29%) | 5083/6859 (74.11%) | 1348/3186 (42.31%) |
| 9 | 2-sided through step | supported | 4911/4917 (99.88%) | 4911/10051 (48.86%) | 7359/10051 (73.22%) | 1637/3166 (51.71%) |
| 10 | Slanted through step | unsupported | 0/0 (—) | 0/6613 (0%) | 3478/6613 (52.59%) | 0/3056 (0%) |
| 11 | O-ring | supported | 3112/3113 (99.97%) | 3112/9723 (32.01%) | 9474/9723 (97.44%) | 3011/3102 (97.07%) |
| 12 | Blind hole | supported | 3165/9809 (32.27%) | 3165/6311 (50.15%) | 6152/6311 (97.48%) | 2982/3024 (98.61%) |
| 13 | Triangular pocket | supported | 8502/36785 (23.11%) | 8502/12108 (70.22%) | 11472/12108 (94.75%) | 2834/3007 (94.25%) |
| 14 | Rectangular pocket | supported | 11204/58115 (19.28%) | 11204/16082 (69.67%) | 14905/16082 (92.68%) | 2911/3167 (91.92%) |
| 15 | 6-sided pocket | supported | 16507/36785 (44.87%) | 16507/21050 (78.42%) | 19456/21050 (92.43%) | 2768/3018 (91.72%) |
| 16 | Circular end pocket | supported | 11025/26365 (41.82%) | 11025/15526 (71.01%) | 14489/15526 (93.32%) | 2854/3092 (92.3%) |
| 17 | Rectangular blind slot | supported | 1324/1376 (96.22%) | 1324/2454 (53.95%) | 1907/2454 (77.71%) | 331/599 (55.26%) |
| 18 | Vertical circular end blind slot | supported | 747/26275 (2.84%) | 747/2464 (30.32%) | 1347/2464 (54.67%) | 373/604 (61.75%) |
| 19 | Horizontal circular end blind slot | supported | 1829/1832 (99.84%) | 1829/2561 (71.42%) | 1923/2561 (75.09%) | 458/627 (73.05%) |
| 20 | Triangular blind step | supported | 2296/2389 (96.11%) | 2296/6670 (34.42%) | 5499/6670 (82.44%) | 2296/3194 (71.88%) |
| 21 | Circular blind step | supported | 4502/4504 (99.96%) | 4502/6487 (69.4%) | 5493/6487 (84.68%) | 2252/3148 (71.54%) |
| 22 | Rectangular blind step | supported | 7580/26275 (28.85%) | 7580/9875 (76.76%) | 8666/9875 (87.76%) | 2534/3175 (79.81%) |
| 23 | Round | supported | 2715/3428 (79.2%) | 2715/3415 (79.5%) | 2959/3415 (86.65%) | 2647/2925 (90.5%) |
| 24 | Stock | incomparable | 0/0 (—) | 0/61070 (0%) | 7062/61070 (11.56%) | 0/0 (—) |

## Interpretation

The exact baseline confirms that model-level silence is not the transfer problem: 9,372 of 9,373
models emit at least one physical record. Through and blind holes, O-rings, pockets and Round also
have high instance detection. Their lower defining-face recall shows that accepted occurrences
often claim a narrower evidence set than the dataset's single-assignment face annotation.

The sharpest supported-class gaps are more specific:

- Chamfer detects 45.26% of instances and claims 42.67% of defining faces.
- Rectangular through steps detect 42.31% of instances; two-sided through steps detect 51.71%.
- Rectangular and vertical circular-end blind slots detect 55.26% and 61.75% of instances.
- The partial rectangular-through-slot class detects 70.33% of instances but has 34.87% one-vs-
  class precision. Its 89.8% constituent coverage is evidence that other accepted physical
  records explain much of the labelled geometry; it does not authorize widening Slot semantics.

Low one-vs-class precision for broad passage, hole and pocket mappings is not by itself a false-
positive rate. Those families map to several mutually exclusive dataset labels, and each row's
denominator includes every defining face claimed by the mapped family. The report deliberately
preserves 47,178 taxonomy-mismatch defining-face occurrences and 46,179 unmapped accepted records
instead of forcing ambiguous package evidence into one corpus class.

Unsupported classes 5, 7 and 10 and incomparable Stock do not become production requirements.
Dataset labels may prioritize a geometry-first investigation, but they do not define ownership,
reconciliation or numerical tolerances. This baseline closes the missing evidence archive; it does
not itself request a recognition behavior change.
