# Native cylindrical pocket ends — #541 / PR #544

## Result

The authored Draftwright reproduction now publishes one rectangular SectionRecess
with a truthful cylindrical mouth, four original defining walls and five original
constituent faces. Cylindrical stock is consulted context, not constituent evidence.
The 6 × 24 footprint has physical depth 8–12 mm; it is not a uniform-depth prism.

The full MFCAD++ development comparison adds **no measured recognition gain** on
this selection and introduces no regression. This is a consumer capability repair.

| Check | Result |
| --- | --- |
| Baseline production commit | `154182c8328ece2f8a261558fade46b2562d1eed` (main-equivalent PR #543) |
| Evaluated implementation | `88cd359d9ee97b05a964df7e1fd5c67b597199cb` |
| Selection | 2,500 selected; 2,493 evaluated |
| Invalid inputs | Same seven IDs and reasons |
| Changed per-model results | 0 |
| Per-model/class coverage or matched-defining losses | 0 |
| Summary/count/reconciliation changes | None |
| Selection SHA-256 | `ad92768788d88e3c4e3866bc2a614e7a345fea7fc52463dfc9f0b9b9e850058e` |
| Output SHA-256 | `fca9c7aa283ce604decd9603c92cda11e70caec62f3a8dd6f256cc6e35cd6eb2` |

Mapping, selected source-model hashes and invalid-input reasons were compared, not
assumed equal. The unchanged invalid IDs are 12939, 13975, 14052, 14307, 18628,
22386 and 22439 (hole cylindrical evidence without one valid owning solid).

## Reproduction

Run in a clean, isolated checkout at the evaluated implementation, using the same
Python 3.12 / build123d 0.11.1 environment as the baseline:

```sh
PYTHONPATH=src python tools/run_effectiveness_baseline.py mfcadpp MFCADPP_TEST_ROOT \
  --dataset-version 'MFCAD++ published test split; DOI 10.17034/d1fec5a0-8c10-4630-b02e-b92dc81df823' \
  --taxonomy docs/benchmarks/effectiveness-taxonomy-v13.json \
  --limit 2500 --allow-invalid --canonical --workers 4 \
  --checkpoint-dir NEW_CHECKPOINT_DIRECTORY --output cylindrical-pocket-report.json
```

The authority-bound run completed successfully. Timing on this shared host is not
performance evidence. The stored report is
`effectiveness-mfcadpp-2500-cylindrical-88cd359.json`; its hash identifies this
particular report, including provenance.

## Other gates

Authored tests exercise source proofs, positive/negative branches, floor and wall
support, publication bounds, scale, offset, rigid placement, compound ownership,
STEP transport and independent reconstruction from emitted JSON. Whole-branch
independent review and focused closure review report no remaining blocker.
Required CI is still a separate merge gate; this document does not assert it has
finished. No MFInstSeg inputs or diagnostics were used.
