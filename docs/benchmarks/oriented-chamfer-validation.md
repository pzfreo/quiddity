# Oriented chamfer validation (#780)

These reports use clean source commit `d3ee0e6bfe3f9656482ea998dfb112e69d179a46`,
taxonomy v15, and the original published datasets. They evaluated all selected models with zero
invalid results. The selected IDs and every model source hash match the
[#779 baseline](oblique-through-step-validation.md). Numerical libraries used one thread and the
runner used three model workers.

| Artifact | SHA-256 |
| --- | --- |
| [MFCAD++ effectiveness](effectiveness-mfcadpp-2500-oriented-chamfers-d3ee0e6.json) | `2765b1f08dfa4c57123022aa4dec3d1cc775f58ed54460ef5d8b9d9df2a33ccd` |
| [MFInstSeg effectiveness](effectiveness-mfinstseg-9373-oriented-chamfers-d3ee0e6.json) | `84026ebab4c4bb4f477a2e0909f63bf3dc83103f1d51763f1123a6343b3e6a2b` |

## Measured change

| Class-0 measure | #779 baseline | With oriented chamfers |
| --- | ---: | ---: |
| MFCAD++ physical records | 0 | 101 |
| MFCAD++ defining faces correct / claimed | 415 / 566 | 516 / 667 |
| MFCAD++ defining-face precision | 73.3% | 77.4% |
| MFCAD++ defining-face recall | 415 / 1,025 (40.5%) | 516 / 1,025 (50.3%) |
| MFCAD++ face coverage | 668 / 1,025 | 716 / 1,025 |
| MFInstSeg physical records | 0 | 378 |
| MFInstSeg defining faces correct / claimed | 1,522 / 1,893 | 1,900 / 2,271 |
| MFInstSeg defining-face precision | 80.4% | 83.7% |
| MFInstSeg defining-face recall | 1,522 / 3,567 (42.7%) | 1,900 / 3,567 (53.3%) |
| MFInstSeg instances recalled | 1,364 / 3,014 (45.3%) | 1,725 / 3,014 (57.2%) |
| MFInstSeg face coverage | 2,328 / 3,567 | 2,484 / 3,567 |

All 101 and 378 new defining-face claims respectively match class 0. Other classes' measured
summaries are unchanged. MFCAD++ has face labels but no native instance labels, so its record
count is not an instance-recall measure. MFInstSeg's instance recall uses native labels.

`OrientedChamfer` covers an original planar quadrilateral external bevel with two convex planar
support joins, a straight oblique run, and material-side proof of a removed corner. Clipped ends
are retained as individual support spans. Blind slants with triangular terminals, concave support
joins, internal bevels, gussets and webs remain outside this conservative subset. Taxonomy v15 maps
this family to class 0 only for scoring; corpus labels do not determine geometric eligibility.

The new public record family is a minor-release contract event under
[ADR 0005](../adr/0005-versioned-cross-repository-capability-contract.md). This validation does
not make a release.

## Reproduction

From the clean source commit above, substitute the installed original dataset paths:

```bash
OPENBLAS_NUM_THREADS=1 OMP_NUM_THREADS=1 MKL_NUM_THREADS=1 \
VECLIB_MAXIMUM_THREADS=1 NUMEXPR_NUM_THREADS=1 QUIDDITY_THREADS=1 \
  uv run python tools/run_effectiveness_baseline.py \
  mfcadpp /path/to/MFCAD++_dataset/step/test \
  --dataset-version 'MFCAD++ published test split; DOI 10.17034/d1fec5a0-8c10-4630-b02e-b92dc81df823' \
  --taxonomy docs/benchmarks/effectiveness-taxonomy-v15.json \
  --limit 2500 --allow-invalid --canonical --workers 3 \
  --output /tmp/quiddity-780-mfcadpp.json

OPENBLAS_NUM_THREADS=1 OMP_NUM_THREADS=1 MKL_NUM_THREADS=1 \
VECLIB_MAXIMUM_THREADS=1 NUMEXPR_NUM_THREADS=1 QUIDDITY_THREADS=1 \
  uv run python tools/run_effectiveness_baseline.py \
  mfinstseg /path/to/MFInstSeg \
  --partition-root /path/to/AAGNet/MFInstseg_partition \
  --dataset-version published-original \
  --taxonomy docs/benchmarks/effectiveness-taxonomy-v15.json \
  --canonical --workers 3 \
  --output /tmp/quiddity-780-mfinstseg.json
```
