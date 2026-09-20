# E5 current passage/pocket decision

Issue [#369](https://github.com/pzfreo/quiddity/issues/369), under epic
[#290](https://github.com/pzfreo/quiddity/issues/290), asked for a fresh implementation-baseline
decision after the intervening correctness work. This is a negative production result: the current
development evidence does not expose a substantial intact passage or pocket family behind one
bounded gate. It refreshes the evidence and hardens the audit, but does not weaken recognition or
change a public record.

## Authority and reproduction

The baseline is raw-coordinate recognition at clean main
`d156aab8cb37ae23f1bdaddf62981340af1af459` (Quiddity 0.3.2.dev0), taxonomy v13, against the
published MFCAD++ test split. The lexical first 2,500 model IDs have selection SHA-256
`ad92768788d88e3c4e3866bc2a614e7a345fea7fc52463dfc9f0b9b9e850058e`. Exactly 2,493 models
evaluate; the same seven documented inputs remain invalid for the same reason.

```console
uv run python tools/run_effectiveness_baseline.py mfcadpp \
  /path/to/MFCAD++_dataset/step/test \
  --dataset-version \
  'MFCAD++ published test split; DOI 10.17034/d1fec5a0-8c10-4630-b02e-b92dc81df823' \
  --taxonomy docs/benchmarks/effectiveness-taxonomy-v13.json \
  --limit 2500 --allow-invalid --canonical --workers 0 \
  --checkpoint-dir /tmp/effectiveness-mfcadpp-2500-d156aab8 \
  --output docs/benchmarks/effectiveness-mfcadpp-2500-current-main-d156aab8.json
```

The passage component reports use audit commit `9a68898`. Version 3 of the audit now requires an
explicit invalid-input policy for the canonical 2,500 selection, records only the exact known
model/reason combinations, and fails if that set drifts. Run it once for each class ID 2, 3 and 4:

```console
uv run python tools/audit_mfcadpp_section_passage_gaps.py \
  /path/to/MFCAD++_dataset/step/test \
  --class-id CLASS --limit 2500 --allow-invalid \
  --output docs/benchmarks/effectiveness-mfcadpp-2500-section-passage-classCLASS-9a68898.json
```

The polygonal-pocket report was generated at `d156aab8` with:

```console
uv run python tools/audit_mfcadpp_polygonal_pocket_residuals.py \
  /path/to/MFCAD++_dataset/step/test \
  --limit 2500 --workers 64 --allow-invalid \
  --output docs/benchmarks/mfcadpp-polygonal-pocket-residuals-d156aab8.json
```

Immutable artifacts and SHA-256 digests:

| Artifact | SHA-256 |
| --- | --- |
| [`effectiveness-mfcadpp-2500-current-main-d156aab8.json`](effectiveness-mfcadpp-2500-current-main-d156aab8.json) | `0a72f7849247c05a2fe4d20df247ae031140c6731d658ac8505fc92d2cdccda5` |
| [`effectiveness-mfcadpp-2500-section-passage-class2-9a68898.json`](effectiveness-mfcadpp-2500-section-passage-class2-9a68898.json) | `ea2ceb43be74e7fbbc69657bcadf76877b057eb66c62486e66f656d7c2f624f4` |
| [`effectiveness-mfcadpp-2500-section-passage-class3-9a68898.json`](effectiveness-mfcadpp-2500-section-passage-class3-9a68898.json) | `c9c3bb880df8b917f347e100086dbfab7370af3d47618604bb1683ec3e6a701b` |
| [`effectiveness-mfcadpp-2500-section-passage-class4-9a68898.json`](effectiveness-mfcadpp-2500-section-passage-class4-9a68898.json) | `706f8ae14aa7444238a56ee654c773f02a5b0dbd6d273228eaaf168424fb0a4c` |
| [`mfcadpp-polygonal-pocket-residuals-d156aab8.json`](mfcadpp-polygonal-pocket-residuals-d156aab8.json) | `21396e65575faa2f8ef2e989b3c885ea0c9088ae5ab87d77d49122127e718927` |

MFCAD++ supplies face labels but no native occurrence relation. Every count below is therefore
either an exact face count or a same-label, shared-edge **component proxy** count. Labels select
geometry to describe only after production candidates have been constructed; they do not enter a
recognition predicate.

## Refreshed score vector

The historical comparison point is the last pre-increment full report at `8730db5`. Intervening
bounded work reduced uncovered passage faces by 515 and uncovered pocket faces by 37. Those gains
do not turn the remaining totals into proposed occurrences.

| MFCAD++ class | Historical covered | Current covered | Current uncovered |
| --- | ---: | ---: | ---: |
| 2 — triangular passage | 2,522 / 3,194 | 2,660 / 3,194 | 534 |
| 3 — rectangular passage | 3,725 / 4,382 | 3,810 / 4,382 | 572 |
| 4 — six-sided passage | 4,820 / 6,645 | 5,112 / 6,645 | 1,533 |
| **Passage total** | **11,067 / 14,221** | **11,582 / 14,221** | **2,639** |
| 13 — triangular pocket | 3,589 / 3,892 | 3,596 / 3,892 | 296 |
| 14 — rectangular pocket | 4,618 / 4,895 | 4,618 / 4,895 | 277 |
| 15 — six-sided pocket | 5,368 / 5,707 | 5,375 / 5,707 | 332 |
| 16 — circular-end pocket | 4,164 / 4,536 | 4,187 / 4,536 | 349 |
| **Pocket total** | **17,739 / 19,030** | **17,776 / 19,030** | **1,254** |

These are all-family constituent coverage totals. They are not native instance recall, an
AS-only gap, or a promise that every uncovered labelled face belongs in one package occurrence.

## Passage residual

Of the 2,639 uncovered passage faces, 1,606 are in 324 component proxies untouched by every
accepted family. Applying the unchanged production wall-ring proofs gives:

| Class | Untouched proxies / faces | Not one simple cycle | Unequal wall intervals | Nonplanar/non-wall |
| --- | ---: | ---: | ---: | ---: |
| Triangular | 112 / 343 | 89 / 270 | 21 / 64 | 2 / 9 |
| Rectangular | 43 / 177 | 38 / 153 | 5 / 24 | 0 / 0 |
| Six-sided | 169 / 1,086 | 148 / 889 | 13 / 67 | 8 / 130 |
| **Total** | **324 / 1,606** | **275 / 1,312** | **39 / 155** | **10 / 139** |

No untouched proxy reaches the existing complete ring proof. The dominant 275-proxy population is
the already-audited interrupted/branched topology, now measured over the complete development
selection rather than the old first 500. Joining its fragments would infer missing section
boundaries or split a labelled component without independent occurrence authority.

The 39 unequal-interval proxies are not one tolerance cluster: they include common-low,
common-high and neither-end-common spans, with both planar and cylindrical surrounding context.
The prior authored stepped-stock counterexample proves that truncating them to a common core or a
maximum envelope can respectively discard physical occurrence geometry or fill real exterior air.
The ten nonplanar/non-wall proxies do not establish a supported polygonal wall contract.

## Pocket residual

The exact polygonal-pocket audit accounts for all 905 uncovered faces in classes 13–15:

| Class | Untouched proxies / faces | Missing faces in partial proxies | Incomplete first-gate result |
| --- | ---: | ---: | --- |
| Triangular | 58 / 204 | 92 | 78 cycle, 6 cap, 3 recognisable |
| Rectangular | 13 / 51 | 226 | 107 cycle, 8 recognisable |
| Six-sided | 39 / 218 | 114 | 55 cycle, 4 cap |
| **Total** | **110 / 473** | **432** | **240 cycle, 10 cap, 11 recognisable** |

Of the 110 wholly untouched proxies, 109 fail the simple-cycle proof and one is an inaccessible
two-cap cavity. The eleven recognisable rows are all partially covered membership cases, not new
untouched detection: together they leave 36 faces unpublished. Reviving the deferred #473
membership audit for that bounded subset would not satisfy this issue's requested substantial
detection increment.

The remaining class-16 total follows the already-delivered coordinate-free SectionRecess work;
the old oriented-pocket prototype is not unfinished production scope. No new recurring circular-end
contract was selected from the residual total.

## Decision

Close the resumed #369 passage/pocket selection without a production predicate change. Current
main has no substantial intact supported family concealed by one presentation gate, and the
remaining high-count buckets require one of the contracts already rejected by authored
counterexamples: invented wall closure, a common-core occurrence, hidden prior-stock support, or
label-directed component splitting.

Future passage/pocket work needs new consumer-backed geometry that independently proves its
physical boundary. A concrete supplied part or a materially larger geometry-first opportunity can
justify a new issue; headline residual counts alone cannot. #473 remains a separately deferred
membership audit, and the one-model bore/roof composition experiment remains below its recorded
promotion threshold.

No MFInstSeg model, face, anatomy or refusal detail was read or used. No release, taxonomy change,
public schema change or performance claim is part of this decision.
