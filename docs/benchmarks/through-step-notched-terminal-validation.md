# Notched-terminal ThroughStep validation at 7647df2

The source-pinned reports below evaluate the change in issue #777 after the #781 inventory
recovery was merged. Both datasets use taxonomy v13, SHA-256
`bf03e2edd716b096c5c695df456e02856a0434acbbefd01ec3372c8475fad42e`, and
Quiddity `0.4.0.dev0` at commit `7647df2b7013511edc3ee714e40afc5402894c91`.
Recognition ran on macOS arm64, Python 3.14.7, build123d 0.11.1 and OCP 7.9.3.1, with
four worker processes and one numerical thread per worker.

## MFCAD++ development evidence

The canonical [2,500-model effectiveness report](effectiveness-mfcadpp-2500-notched-7647df2.json)
has SHA-256 `3ec4b6262d5e6b88c69262184ceae4fdaba19b0a8fe60b9e5b05ed3b6ea900bc`.
The [class-8 component and residual audit](through-step-residual-audit-mfcadpp-2500-7647df2.json)
has SHA-256 `8278c8e32d09312922b8e647a150d387241055eb1012732d1b5cc5d964621829`.
The first 2,500 unique test IDs in lexical order have selection SHA-256
`ad92768788d88e3c4e3866bc2a614e7a345fea7fc52463dfc9f0b9b9e850058e`.
All 2,500 were evaluated, with zero invalid report rows under the documented
`--allow-invalid` policy.

| Class-8 measure | #781-only source `0ec738b` | Notched-terminal source `7647df2` |
| --- | ---: | ---: |
| ThroughStep records | 410 | 571 |
| Correct / claimed defining faces | 821/821 | 1,143/1,143 |
| Defining-face recall | 821/2,045 (40.15%) | 1,143/2,045 (55.89%) |
| Non-native connected-component recall | 410/859 (47.73%) | 569/859 (66.24%) |
| All-family face coverage on class 8 | 1,619/2,045 | 1,706/2,045 |

The #781-only comparison was run canonically from commit
`0ec738bd54739db0fdffee11acc4373337c31957`; its full report has SHA-256
`7c0cc7df01402a2f3a1f55dbc300828e00fdfa69abe199f1bbabe4a4cffa2ade`.
The matched selection, taxonomy and input hashes prevent a denominator drift in this comparison.
The earlier #777 projection used 858 components and 2,040 labelled faces because it excluded
model 14052. The #781 recovery evaluates that model and adds one unproved component with five
labelled faces; it contributes no ThroughStep claim. Other physical-family counts, other mapped
class metrics, and the taxonomy-mismatch count are unchanged between these two MFCAD++ runs.

The 569 recalled proxies include **55 partial components**. Those components contain 246 labelled
faces, of which ThroughStep claims cover 114. The other 132 faces remain uncovered by this family.
Across all class-8 faces, ThroughStep defining claims cover 1,143/2,045 (55.89%); component recall
must not be read as complete face recall. The audit records the claimed and total face indices for
each partial component, and marks model 14052's component `unproven_solid`.

## MFInstSeg transfer evidence

The canonical [9,373-model transfer report](effectiveness-mfinstseg-9373-notched-7647df2.json)
has SHA-256 `46e422830f4f29d3c270167b4b93d3d59f6b6030016a5ff659f615ae598d689c`.
Its selected-ID SHA-256 is
`4c8f56fec3e5b980011d112061124bec9e31ec7afdc32b221939452e7fdb5b40`.
The report evaluates all 9,373 original published test IDs after the same five cross-split
exclusions as the [frozen baseline](effectiveness-mfinstseg-9373-f2cfe4f.md), with zero invalid
rows. No individual MFInstSeg model geometry was inspected for this change.

| Class-8 measure | Frozen `f2cfe4f` | Notched-terminal `7647df2` |
| --- | ---: | ---: |
| Instance recall | 1,348/3,186 (42.31%) | 1,711/3,186 (53.70%) |
| Correct / claimed defining faces | 2,695/2,696 | 3,423/3,424 |
| Defining-face recall | 2,695/6,859 (39.29%) | 3,423/6,859 (49.91%) |
| All-family face coverage on class 8 | 5,083/6,859 | 5,277/6,859 |

The new claims add 728 correct defining faces and no new incorrect defining face. All other
classes' defining-face metrics and instance recall remain unchanged. A second canonical run from
the #777-only commit `83a0092` had the same class summaries as this combined-source run, so #781
did not change the transfer outcome.

## Geometric and audit boundary

The recogniser still requires a complete concave seam over the source-solid run, both walls on
the stock envelope, one same-solid owner, a terminal region at each run end joined convexly to
both walls, no third co-spanning concave wall, and an exactly empty removed prism. Only the
terminal region's full-section bounding-box coverage check was removed. Authored tests cover
step, edge-open pocket and hole notches; rotation, scale, STEP and face-order changes; and mostly
removed terminals, caps, obstructed prisms and third-wall controls. The audit schema is now
version 2 so partial components and unproved solid ownership are reported rather than raised.

## Reproduction

Run from the clean source commit named above with the original published datasets. Use a fresh
output and checkpoint path for each run; canonical reports never overwrite an earlier artifact.

```bash
OPENBLAS_NUM_THREADS=1 OMP_NUM_THREADS=1 QUIDDITY_THREADS=1 \
  uv run python tools/run_effectiveness_baseline.py \
  mfcadpp /path/to/MFCAD++_dataset/step/test \
  --dataset-version 'MFCAD++ published test split; DOI 10.17034/d1fec5a0-8c10-4630-b02e-b92dc81df823' \
  --taxonomy docs/benchmarks/effectiveness-taxonomy-v13.json \
  --limit 2500 --allow-invalid --canonical --workers 4 \
  --checkpoint-dir .cache/effectiveness/mfcadpp-2500-7647df2 \
  --output /tmp/mfcadpp-2500-7647df2.json

OPENBLAS_NUM_THREADS=1 OMP_NUM_THREADS=1 QUIDDITY_THREADS=2 \
  uv run python tools/audit_mfcadpp_through_steps.py \
  /path/to/MFCAD++_dataset/step/test --limit 2500 \
  --output /tmp/through-step-audit-2500-7647df2.json

OPENBLAS_NUM_THREADS=1 OMP_NUM_THREADS=1 QUIDDITY_THREADS=1 \
  uv run python tools/run_effectiveness_baseline.py \
  mfinstseg /path/to/MFInstSeg \
  --partition-root /path/to/AAGNet/MFInstseg_partition \
  --dataset-version published-original \
  --taxonomy docs/benchmarks/effectiveness-taxonomy-v13.json \
  --canonical --workers 4 \
  --checkpoint-dir .cache/effectiveness/mfinstseg-9373-7647df2 \
  --output /tmp/mfinstseg-9373-7647df2.json
```
