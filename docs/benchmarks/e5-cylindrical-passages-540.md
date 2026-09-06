# E5 — Polygonal passages terminating on native cross-bores

Issue #540, PR #551; part of #369 and epic #290. Architecture: ADR 0023.

## Scope and physical evidence

Production commit `15cc295` recognises one connected convex polygonal wall ring
between an observed planar mouth and an observed native inward cylinder. Both
ends are open. The cylinder branch is negative at a high end and positive at a
low end. Opposite regions across one bore remain separate; cylinder context is
not published as a polygonal wall. No new JSON surface fields are introduced.

An authored 40 mm cube with a radius-8 Y-axis bore and a radius-3 polygonal X-axis
through-cut produces two independently supported regions. Triangle, rectangle
and hexagon per-region volumes are approximately 141.127130, 217.712177 and
283.374482 mm³. Tests cover scale, arbitrary rotation, STEP round trips, separate
bodies, a physical bridge obstructing only one side, partial-bore refusal,
unequal opposite mouths, empty-volume gates and candidate-local failure.
Consumers reconstruct from JSON alone under the existing 0.002 mm publication
displacement budget. The independent bore remains recognised.

The motivating open-development model is MFCAD++ `12354.step`, SHA256
`7b9f9723cf9483722a0e8769980eaa5e14d9a99e952e5609ff28b5fa0cd01bd2`.
Its original wall groups `(8,9,10,11,12,13)` and `(45,46,47,48,49,50)` now publish
as separate hexagonal passages. This example is not a full-corpus gain estimate.

## Scoring and validation boundary

Native SectionRecess discovery formerly emitted only pocket classifications.
The scorer now maps an explicit native `feature_kind="passage"` to the existing
`passages` taxonomy family; pocket mappings are unchanged. Taxonomy v13 itself
is unchanged. The production predicate receives no labels or expected side
counts. This is a detection and publication change, not a remapping of pockets
into passages to inflate a score.

Independent architecture, public validator, private proof, integration and
scoring reviews found no remaining substantive findings. Before the final
comparison, 51 contract checks, 100 combined cylindrical geometry/proof/public
regressions, 21 architecture tests, seven scoring tests, full mypy and Ruff passed.
Additional strict-domain and disconnected-cycle source tests also pass.

No MFInstSeg data or anatomy was used. The separately requested 0.2.3 release
does not contain this work and was not a prerequisite for it.

## Reproduction

Run from a clean checkout of production commit `15cc295` with the project
development environment and `PYTHONPATH=src`. The input is the published
MFCAD++ test split, not a holdout-selected subset:

```bash
python tools/run_effectiveness_baseline.py mfcadpp \
  /app/workspaces-codex/datasets/mfcadpp/MFCAD++_dataset/step/test \
  --dataset-version 'MFCAD++ published test split; DOI 10.17034/d1fec5a0-8c10-4630-b02e-b92dc81df823' \
  --taxonomy docs/benchmarks/effectiveness-taxonomy-v13.json \
  --limit 2500 --allow-invalid --canonical --workers 4 \
  --output /tmp/effectiveness-cylindrical-passages-15cc295.json
```

The comparison baseline is the completed #546 production run at `771e85f`,
report SHA256 `3a9fb59847fb72282ad39a25726497ba2d649cbc9f74495ddd1af6440f2c2878`.
Its final follow-up changed tests only. It evaluated 2,493 of 2,500 inputs, with
seven pre-existing invalid models: 12939, 13975, 14052, 14307, 18628, 22386,
22439. All report `Hole cylindrical evidence does not prove one valid solid`.
The selected-ID SHA256 is
`ad92768788d88e3c4e3866bc2a614e7a345fea7fc52463dfc9f0b9b9e850058e`;
taxonomy SHA256 is
`bf03e2edd716b096c5c695df456e02856a0434acbbefd01ec3372c8475fad42e`.

## Full comparison

The complete 2,500-input run evaluates 2,493 models with the same seven invalid
IDs and reasons. Selection, taxonomy, model source hashes and statuses match
the baseline. There are 35 changed scored models, with **no per-model/class
coverage or matched-defining loss**. Native SectionRecess records increase by 51.

| Label family | Newly covered faces | Additional matched defining faces |
| --- | ---: | ---: |
| Triangular passage | 68 | 72 |
| Rectangular passage | 22 | 40 |
| Six-sided passage | 100 | 102 |
| Chamfer | 1 | 0 |
| Total | **191** | **214** |

Passage defining claims increase by 215 faces: 214 match passage labels and one
is labelled chamfer. That is 214/215 (99.53%) label agreement for the added
claims, not proof of native instance recall. MFCAD++ supplies semantic labels,
not ground-truth occurrence instances. The single chamfer-labelled wall is
covered but does not count as a newly recognised chamfer occurrence.

Triangular passage coverage rises from 0.799937 to 0.821227, rectangular from
0.857143 to 0.862163, and six-sided from 0.747780 to 0.762829. Pocket coverage
is unchanged. Per-class defining precision uses the shared mapped passage
prediction denominator: triangular precision rises, while rectangular and
six-sided ratios decline slightly as valid claims for sibling passage labels
enter that denominator. This is not a claim of uniformly improved precision;
report numerator and denominator changes separately from the loss audit.

Report SHA256:
`c6799c0471626d0cc4203e82da8c107020d4b17402d42d9808bf113ece04121f`.
The canonical report was produced from clean production `15cc295`; subsequent
benchmark documentation and boundary-test additions do not change production
or scoring code. Required CI remains a separate merge gate.

## Remaining scope

This does not solve stepped/grooved stock ends, missing or branched wall cycles,
sloped opposite mouths, fitted/non-native cylindrical terminations or general
two-curved-end passages. The hidden-groove counterexample in #540 establishes
non-unique prior-stock removal history for that specific anatomy, not a blanket
claim that the full remaining residual is unsupported. #540 and #369 stay open.
