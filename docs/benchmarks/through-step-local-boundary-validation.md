# Through-step local-boundary validation (#778)

The three source-pinned artifacts below were produced from clean commit
`643b764c7a10e75e5791b6e6b7a262ab9fe91972`, taxonomy v13, and the original published
datasets. The before reports are the merged #777 artifacts. Selected IDs and each selected
model's STEP and label hashes match between before and after; all 2,500 MFCAD++ and 9,373
MFInstSeg models evaluated with zero invalid results. Numerical libraries used one thread and
the runner used three model workers.

| Artifact | SHA-256 |
| --- | --- |
| [MFCAD++ effectiveness](effectiveness-mfcadpp-2500-local-boundaries-643b764.json) | `3888b06039a194a89be163b5721cee0f68a9de918399d8fa9824f2f9afc974c5` |
| [MFInstSeg effectiveness](effectiveness-mfinstseg-9373-local-boundaries-643b764.json) | `ade50a1345b5da4bb3398196212718372130437ea3b57de0ed630df803f45bcd` |
| [MFCAD++ residual audit](through-step-residual-audit-mfcadpp-2500-local-boundaries-643b764.json) | `6597fd9c1368b9c497365112999967ecf16e9ed101732f598aa5fe6ed09c6b96` |

## Measured change

| Metric | Before (#777) | With local boundary |
| --- | ---: | ---: |
| MFCAD++ class-8 defining faces correct / claimed | 1,143 / 1,143 | 1,259 / 1,259 |
| MFCAD++ class-8 defining recall | 1,143 / 2,045 | 1,259 / 2,045 |
| MFCAD++ through-step records | 571 | 629 |
| MFCAD++ recalled class-8 connected-component proxies | 569 / 859 | 614 / 859 |
| MFInstSeg class-8 instances recalled | 1,711 / 3,186 | 1,875 / 3,186 |
| MFInstSeg class-8 defining faces correct / claimed | 3,423 / 3,424 | 3,752 / 3,757 |
| MFInstSeg class-8 defining recall | 3,423 / 6,859 | 3,752 / 6,859 |

Across MFCAD++ the 116 additional claimed class-8 defining faces are all correct, in 46 models;
other classes' measured defining metrics and instance recall are unchanged. The component count
is a connected-component proxy, not a native instance label; 66 components are partial. The
residual audit reconciles 1,259 claimed and 786 unclaimed labelled faces exactly. The
shortened-wall samples 10118 and 10505 still fail because their concave seams end inside the
solid. Thus the 218-component upper bound in the issue is not an expected recovery count.

Across MFInstSeg, 333 additional class-8 defining faces are claimed: 329 correct and four
incorrect. All four incorrect faces are in model `20221121_154648_11330`, labelled as a class-6
channel. Its two local L-shaped corner pairs satisfy the same-solid, complete-run and empty-prism
proof, while the wider slanted channel context is not currently a recognised Channel occurrence.
The class-6 face-coverage count rises by two on this model; all other classes' defining metrics
and instance recall are unchanged. An attempted broader same-plane return exclusion also rejected
three correctly labelled MFInstSeg through steps and was not retained. This measured overlap is
a remaining taxonomy limitation, rather than a reason to infer an unsupported drafting history
from identical local geometry.

## Reproduction

Run from the clean source commit above, substituting the installed original dataset paths:

```bash
OPENBLAS_NUM_THREADS=1 OMP_NUM_THREADS=1 QUIDDITY_THREADS=1 \
  uv run python tools/run_effectiveness_baseline.py \
  mfcadpp /path/to/MFCAD++_dataset/step/test \
  --dataset-version 'MFCAD++ published test split; DOI 10.17034/d1fec5a0-8c10-4630-b02e-b92dc81df823' \
  --taxonomy docs/benchmarks/effectiveness-taxonomy-v13.json \
  --limit 2500 --allow-invalid --canonical --workers 3 \
  --checkpoint-dir /tmp/quiddity-778-mfcadpp-checkpoints \
  --output /tmp/quiddity-778-mfcadpp.json

OPENBLAS_NUM_THREADS=1 OMP_NUM_THREADS=1 QUIDDITY_THREADS=1 \
  uv run python tools/run_effectiveness_baseline.py \
  mfinstseg /path/to/MFInstSeg \
  --partition-root /path/to/AAGNet/MFInstseg_partition \
  --dataset-version published-original \
  --taxonomy docs/benchmarks/effectiveness-taxonomy-v13.json \
  --canonical --workers 3 \
  --checkpoint-dir /tmp/quiddity-778-mfinstseg-checkpoints \
  --output /tmp/quiddity-778-mfinstseg.json

OPENBLAS_NUM_THREADS=1 OMP_NUM_THREADS=1 QUIDDITY_THREADS=1 \
  uv run python tools/audit_mfcadpp_through_steps.py \
  /path/to/MFCAD++_dataset/step/test --limit 2500 \
  --output /tmp/quiddity-778-residual.json
```
