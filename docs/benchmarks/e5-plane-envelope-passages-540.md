# E5 — Polygonal passages through observed convex two-plane roofs

Issue #540, PR #553; part of #369 and epic #290. Architecture: ADR0024.

## Scope

Production `8c5c7d3` recognises a complete original polygonal wall cycle between
one planar mouth and an observed continuous convex two-plane roof. Both ends
are open. Original roof faces share a convex ridge, collectively support every
wall, and belong to the same solid. Exact clipped-cell wall equality and empty
cell/opening probes are required. No label or expected side count enters the
production predicate.

The kernel-free public `plane_envelope` end carries exactly two affine plane
terms and their minimum (high end) or maximum (low end). Public validation
requires both terms active over positive-area patches and strict separation
from the opposite end. Whole-occurrence serialization error remains bounded by
0.002 mm. Roof faces are contextual authority, not claimed passage walls.

Authored tests cover triangular, rectangular and hexagonal sections, scale,
run reversal, arbitrary transforms, distinct slopes and ridge positions,
compound ownership, STEP round trips and independent JSON reconstruction of
removed volume. Negatives cover material bridges, concave valleys, discontinuous
steps, lateral breakouts and excessive projection error. The final source/public
test file passes 33 cases; independent substantive review is clear. The last
four cases and ADR status correction change no production or scoring code.

## Reproduction and baseline

Run from a clean checkout of `8c5c7d3` with the development environment:

```bash
PYTHONPATH=src:. python tools/run_effectiveness_baseline.py mfcadpp \
  /app/workspaces-codex/datasets/mfcadpp/MFCAD++_dataset/step/test \
  --dataset-version 'MFCAD++ published test split; DOI 10.17034/d1fec5a0-8c10-4630-b02e-b92dc81df823' \
  --taxonomy docs/benchmarks/effectiveness-taxonomy-v13.json \
  --limit 2500 --allow-invalid --canonical --workers 2 \
  --output /tmp/effectiveness-plane-envelope-8c5c7d3.json
```

Baseline: #552 source `23f5fc9`, report SHA256
`20c78ce539d12cf57f18601df3e8ce0eee396442708d6f69348768b62ffd3896`.
It has identical scored results to the validated #551 baseline.

Selected-ID SHA256:
`ad92768788d88e3c4e3866bc2a614e7a345fea7fc52463dfc9f0b9b9e850058e`.
Taxonomy v13 SHA256:
`bf03e2edd716b096c5c695df456e02856a0434acbbefd01ec3372c8475fad42e`.

## Full 2,500-input comparison

Both runs evaluate 2,493 models. Selection, taxonomy, model source hashes and
statuses match. The same seven models remain invalid: 12939, 13975, 14052,
14307, 18628, 22386 and 22439, all with
`Hole cylindrical evidence does not prove one valid solid`.

There are 34 changed scored models and **no per-model/class coverage or
matched-defining losses**. Native SectionRecess records increase by 35.

| Label family | Additional covered faces | Additional matched defining faces |
| --- | ---: | ---: |
| Triangular passage | 34 | 36 |
| Rectangular passage | 32 | 48 |
| Six-sided passage | 55 | 66 |
| Chamfer | 1 | 0 |
| Total | **122** | **150** |

Passage defining claims increase by 151: 150 match passage labels and one is
labelled chamfer (99.34% added-claim label agreement). This is not native
instance recall; MFCAD++ supplies semantic labels rather than occurrence
instances. The chamfer-labelled face does not represent a newly detected
chamfer occurrence. Pocket coverage is unchanged.

Passage coverage rises from 0.821227 to 0.831872 (triangular), 0.862163 to
0.869466 (rectangular), and 0.762829 to 0.771106 (six-sided). Shared mapped
prediction denominators also increase: triangular and six-sided per-class
defining precision decline slightly, while rectangular improves. Do not claim
uniformly improved precision from the added-claim label agreement.

Report SHA256:
`b179594d136d8dafb9771dd59464c44bb79eaa6e21ae6be196ae56fe535f6cf4`.
The source-pinned full run completed in 2,133.2 seconds with two workers and
other validation work sharing the host. This is reproducibility context, not a
performance comparison. Required CI remains a separate final-head merge gate.

## Remaining scope

The original first-500 cohort of 40 untouched six-sided same-label proxies now
has 11 touched proxies / 66 faces, versus 10 / 60 after #551. The remaining
cohort has 18 all-planar-context proxies / 115 faces and 11 curved-context
proxies / 60 faces. Neighbour type does not prove termination anatomy; these
bounded diagnostics are not full-corpus extrapolations or native instances.

This contract does not resolve hidden-groove prior-stock ambiguity, partial
transverse bores, incomplete/branched wall cycles, two envelope ends, sloped
opposite mouths or arbitrary piecewise roofs. #540 and #369 remain open.
Existing interior circular-aperture support proofs cannot directly explain
wall-splitting partial bores: they require complete interior wires and complete
cylindrical rings. Do not remove those invariants to force the new cases through.

No MFInstSeg data, anatomy, refusal-stage aggregates or iterative feedback was
used. No new taxonomy, performance project or release is part of this increment.
