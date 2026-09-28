# Oblique through-step validation (#779)

The two reports below were produced from clean source commit
`38c857ee1a0e4772c930c1987facd38bd3840e4c` and taxonomy v14 on the original published
datasets. They evaluated all selected models with zero invalid results. Selected IDs and every
model's source hashes match the [#778 baseline](through-step-local-boundary-validation.md).
Numerical libraries used one thread and the runner used three model workers.

| Artifact | SHA-256 |
| --- | --- |
| [MFCAD++ effectiveness](effectiveness-mfcadpp-2500-oblique-through-steps-38c857.json) | `a697a56834ceda5f452e8cddd65897b7b31aa309ebddf4615e77a93e58f63cc1` |
| [MFInstSeg effectiveness](effectiveness-mfinstseg-9373-oblique-through-steps-38c857.json) | `85e0df492dd469c762e85a93e2408292a5ff420d55f40422a7922fdbe60a291e` |

## Measured change

| Class-10 measure | #778 baseline | With oblique steps |
| --- | ---: | ---: |
| MFCAD++ physical records | 0 | 156 |
| MFCAD++ defining faces correct / claimed | 0 / 0 | 312 / 312 |
| MFCAD++ defining-face recall | 0 / 1,997 | 312 / 1,997 |
| MFCAD++ face coverage | 1,100 / 1,997 | 1,204 / 1,997 |
| MFInstSeg physical records | 0 | 641 |
| MFInstSeg instances recalled | 0 / 3,056 | 641 / 3,056 (20.98%) |
| MFInstSeg defining faces correct / claimed | 0 / 0 | 1,282 / 1,282 |
| MFInstSeg defining-face recall | 0 / 6,613 | 1,282 / 6,613 |
| MFInstSeg face coverage | 3,478 / 6,613 | 3,929 / 6,613 |

The new defining-face precision is 100% on both corpora. Other classes' measured class summaries
are unchanged. MFCAD++ supplies face labels but no native instance labels, so its record count is
not an instance-recall measure. MFInstSeg uses native instances for the 641 / 3,056 result.

The accepted subset has one principal quadrilateral wall, one oblique rectangular wall and a
complete straight concave seam. Both ends must reach the source-solid envelope with convex
same-solid terminal closure. The opposite principal-wall edge must remain principal, and the
removed wall sweep must be empty. This excludes the triangular blind-step lookalikes that share
the basic two-plane motif. Split and curved walls, local seam ends and multi-face class-10
components remain outside this record's proved scope. Taxonomy v14 maps only this physical family
to class 10; a dataset label never overrides the geometric gates.

Adding the public `ObliqueThroughStep` family is a minor-release contract event under
[ADR 0005](../adr/0005-versioned-cross-repository-capability-contract.md). No release is made by
this validation.

## Reproduction

From the clean source commit above, substitute the installed original dataset paths:

```bash
OPENBLAS_NUM_THREADS=1 OMP_NUM_THREADS=1 MKL_NUM_THREADS=1 \
VECLIB_MAXIMUM_THREADS=1 NUMEXPR_NUM_THREADS=1 QUIDDITY_THREADS=1 \
  uv run python tools/run_effectiveness_baseline.py \
  mfcadpp /path/to/MFCAD++_dataset/step/test \
  --dataset-version 'MFCAD++ published test split; DOI 10.17034/d1fec5a0-8c10-4630-b02e-b92dc81df823' \
  --taxonomy docs/benchmarks/effectiveness-taxonomy-v14.json \
  --limit 2500 --allow-invalid --canonical --workers 3 \
  --output /tmp/quiddity-779-mfcadpp.json

OPENBLAS_NUM_THREADS=1 OMP_NUM_THREADS=1 MKL_NUM_THREADS=1 \
VECLIB_MAXIMUM_THREADS=1 NUMEXPR_NUM_THREADS=1 QUIDDITY_THREADS=1 \
  uv run python tools/run_effectiveness_baseline.py \
  mfinstseg /path/to/MFInstSeg \
  --partition-root /path/to/AAGNet/MFInstseg_partition \
  --dataset-version published-original \
  --taxonomy docs/benchmarks/effectiveness-taxonomy-v14.json \
  --canonical --workers 3 \
  --output /tmp/quiddity-779-mfinstseg.json
```
