# E5 public Blend chain validation

Issue #414 publishes the existing label-independent `BlendCollapseIndex` as a conservative
physical `Blend` family. One occurrence is one complete same-solid, same-radius native cylindrical
rolling-ball chain. `Fillet` retains precedence where its exact defining-face union covers the
chain. [the blend-chain family record](../families/blend-chains.md) records the public contract.

## Evidence protocol

- Authored geometry supplies positive convex chains, an explicit public concave exclusion,
  small-radius and free-axis examples, compounds, rigid transforms, uniform scaling,
  traversal-order invariance, and sharp/full-cylinder negatives.
- The package golden `small_convex_blends` proves the supported public result, census, and archive
  surface end to end.
- MFCAD++ is open development evidence. Discovery and ownership do not read its labels; taxonomy
  v9 adds `blends` beside `fillets` only for class 23, `Round`.
- MFInstSeg remains sealed for this development increment. No individual model, miss, instance,
  face, or class decomposition is inspected or used to shape the implementation.
- Analysis Situs comparison is deferred until the reviewed implementation is merged to `main`.

## Complete-corpus invalid-model policy

The standard lexical first 500 MFCAD++ models contain only one Round face, so the effectiveness
gate uses the complete lexical 2,500-model test split. A clean fail-closed diagnostic found that
all 2,500 STEP files import, but aggregate recognition rejects these seven models before scoring.
The diagnostic implementation commit is
`7be23930bab485dfab52e886e8e137865ba18086` (tree
`ad45310b83fa2ccf9fce3d1bcf50a394d5bfa200`):

`12939`, `13975`, `14052`, `14307`, `18628`, `22386`, and `22439`.

Every rejection is the same pre-existing Hole ownership invariant:
`Hole cylindrical evidence does not prove one valid solid`. The label-independent diagnostic ran
`_take_inventory` over every selected model and did not inspect semantic labels. These seven cases
are retained as named `invalid` rows and excluded from every evaluated-face denominator; they are
not converted into Blend misses. A change in this exact ID/reason set invalidates the policy and
must be investigated before accepting a later report.

The canonical report command is:

```bash
uv run python tools/run_effectiveness_baseline.py \
  mfcadpp /app/workspaces-codex/datasets/mfcadpp/MFCAD++_dataset/step/test \
  --dataset-version \
  'MFCAD++ published test split; DOI 10.17034/d1fec5a0-8c10-4630-b02e-b92dc81df823' \
  --taxonomy docs/benchmarks/effectiveness-taxonomy-v9.json \
  --allow-invalid \
  --output docs/benchmarks/effectiveness-mfcadpp-2500-blends-<commit>.json
```

## Complete-corpus result

Taxonomy files are only the versioned translation from benchmark labels to public recogniser
families; they do not change recognition geometry. The parent uses v8, where class 23 `Round` maps
to `fillets`. The candidate uses v9, where the same class maps to `fillets` and `blends`. A new
taxonomy version is justified here because that translation changed; no additional checkpoint is
needed merely for review or merge.

The exact same 2,500 selected IDs and source hashes were evaluated on the parent and candidate.
Both reports contain the same seven invalid ID/reason rows listed above. After removing provenance,
ordinary timing, class-23 scoring, and the new `blends` counter, every per-model result is
unchanged.

| Measurement | Parent `56b8bd9` | Convex candidate `142edf7` | Change |
| --- | ---: | ---: | ---: |
| Evaluated models | 2,493 | 2,493 | 0 |
| Physical Blend occurrences | 0 | 5 | +5 |
| Round defining-face recall | 0/13 (0.00%) | 5/13 (38.46%) | +5 faces |
| Round face coverage | 5/13 (38.46%) | 10/13 (76.92%) | +5 faces |
| Round mapped defining precision | 0/219 (0.00%) | 5/224 (2.23%) | +5 matches and +5 candidates |

The precision denominator includes both mapped Fillet and Blend records; the five new Blend
occurrences are the five new defining-face matches. Runtime is recorded only as descriptive report
metadata, not as a performance claim or acceptance gate: median model time was 0.675 seconds on
the parent and 0.678 seconds on the candidate (p95 1.300 and 1.292 seconds respectively).

Checked-in reports:

- `effectiveness-mfcadpp-2500-blends-parent-56b8bd9.json` — taxonomy v8, SHA-256
  `bdcdaf0a9e8663d20ac51aef91759c311a2b7d30cad7f1afdebbb6ea9e4102c9`;
- `effectiveness-mfcadpp-2500-blends-convex-142edf7.json` — taxonomy v9, SHA-256
  `a9f984c6d06bcbaec9f38f2c9d4ffcec89eb6c35a6f9cb3ff7db7509b9722e57`; and
- `effectiveness-mfcadpp-2500-blends-broad-fea506d.json` — rejected broad-scope evidence,
  SHA-256 `c6b1ed104426f5cc8c38fa7b42a83e7b92c898dd03aca7c6fde9a84107873650`.

The rejected broad candidate published 1,832 Blend occurrences, of which 1,827 were concave
chains with no Round label. It reached 6/13 Round defining faces rather than 5/13, but its sole
additional match was a concave chain already owned as Pocket constituent evidence. That weak
ownership boundary and duplication did not justify a public concave contract. The broad report is
retained to make that decision auditable, not as the accepted effectiveness result.

All three reports pass the closed report-schema and denominator validator. The independent review,
final-diff ADR conformance, and merge authority are recorded in the pull request; Analysis Situs
and aggregate-only MFInstSeg transfer checks remain post-merge gates against the exact merged
`main` commit.
