# MFInstSeg full-corpus coverage sweep

**This is not an effectiveness baseline.** It scores no defining faces, computes no precision or
recall, and does not use `tools/run_effectiveness_baseline.py`. It answers three narrower
questions across every model in MFInstSeg: does the model import, does recognition complete, and
does it emit anything. The frozen MFInstSeg transfer baseline that
[`effectiveness-baseline-method.md`](effectiveness-baseline-method.md) anticipates still does not
exist, and this file does not become it.

Canonical evidence:
[`mfinstseg-full-coverage-sweep-f1b02ab.json`](mfinstseg-full-coverage-sweep-f1b02ab.json).

## Provenance

- Package: `quiddity`, commit `f1b02ab`, working tree clean
- Corpus: MFInstSeg, **all 62,495 models**, lexical order, no sampling
- Mapping: [`effectiveness-taxonomy-v13.json`](effectiveness-taxonomy-v13.json)
- Entry point: `quiddity.feature_census`, one `import_step` per model
- Environment: macOS, Python 3.14, 16 worker processes, 1002 s wall

Per-model timings are deliberately absent. The run was parallel, so per-model wall clock measures
contention rather than cost. A separate sequential 2000-model sample at this commit gives a median
of 0.13 s/model, against 0.07 s/model at `b123d-recognisers` v0.4.11 on the same sample.

## What the evidence says

| | |
| --- | ---: |
| models | 62,495 |
| recognition completed | 62,476 (99.970%) |
| **raised instead of returning** | **19 (0.030%)** |
| emitted no record at all | 149 (0.238%) |
| total records | 337,138 |
| median records per labelled instance | 0.727 |
| models below one record per instance | 51,597 of 62,476 |

**Silence is not the problem.** 149 models in 62,495 produce nothing, and per-class silence never
exceeds 0.5%. The highest is `Slanted through step` at 82/17,565 (0.5%), a class the taxonomy marks
`unsupported` — so even there the models are being recognised through their other features. As a
false-negative detector at model granularity, this corpus is saturated.

**The deficit is in count, not coverage.** The median model emits 0.727 records per labelled
instance, and 82.6% of models emit fewer records than they have instances. This is not recall:
records and labelled instances are not 1:1 in either direction, patterns add records, one record
can span an instance, and MFInstSeg labels are known to be unreliable at feature intersections.
Turning this ratio into a per-class number requires defining-face attribution, which is exactly
what the effectiveness harness does and this sweep does not.

### Families emitted

| family | records | family | records |
| --- | ---: | --- | ---: |
| `section_recess` | 139,522 | `paired_ramp_step` | 11,126 |
| `hole` | 60,343 | `fillet` | 10,773 |
| `plate` | 26,475 | `through_step` | 8,703 |
| `boss` | 19,709 | `slot` | 4,030 |
| `angled_step` | 15,584 | `oriented_slot` | 1,117 |
| `circular_blind_step` | 15,074 | `countersink` | 563 |
| `chamfer` | 12,062 | `flat` | 170 |
| `blend` | 11,884 | `step` | 3 |

`step` at 3 records in 62,495 models is worth a look on its own: either the family is far narrower
than intended, or those three are misfires.

## The 19 failures

Every one is an internal invariant or projection failure escaping `build_recognition_result` as an
exception rather than becoming a refusal. The part is lost entirely, including features that were
recognised before the raise.

| n | error | message |
| ---: | --- | --- |
| 11 | `ValueError` | adjacent section edges meet away from their shared endpoint |
| 5 | `Standard_TypeMismatch` | `TopoDS::Face` |
| 1 | `ValueError` | section boundary must be canonical and origin-centred |
| 1 | `LegacySectionProjectionError` | legacy section projection refused: centre disagrees with published loop |
| 1 | `ValueError` | rich passage cannot reproduce its historical legacy value |

Three observations.

**The 11 are [#547](https://github.com/pzfreo/quiddity/issues/547)**, which was filed from a single
model found in a 2000-model sample. The corpus-wide count is 11.

**The `LegacySectionProjectionError` row is the sharpest evidence for that issue's second root
cause.** That type exists precisely to be a typed refusal, and it still propagates out of the
public aggregate. So the gap is not only that one construction path forgot a `try`; it is that
`build_recognition_result` has no boundary that converts a projection refusal into a
`SectionRecessRefusal` and continues.

**The 5 `Standard_TypeMismatch: TopoDS::Face` are a different defect** — an OCCT downcast of a
shape that is not a face, below the section machinery. They are not covered by #547 and want their
own investigation.

Fourteen of the nineteen are section or legacy-projection failures, so this is a concentrated
seam rather than scattered fragility.

## Per-class model coverage

Columns are: models whose labels contain the class, and how many of those emitted no record at all.
A silent count here is a property of the whole model, not of the class — it does not say the class
was recognised.

| cls | name | status | models | silent |
| ---: | --- | --- | ---: | ---: |
| 0 | Chamfer | supported | 17,412 | 41 |
| 1 | Through hole | supported | 17,535 | 1 |
| 2 | Triangular passage | supported | 17,700 | 10 |
| 3 | Rectangular passage | supported | 17,727 | 11 |
| 4 | 6-sided passage | supported | 17,515 | 18 |
| 5 | Triangular through slot | unsupported | 3,751 | 6 |
| 6 | Rectangular through slot | partial | 3,634 | 0 |
| 7 | Circular through slot | unsupported | 3,601 | 8 |
| 8 | Rectangular through step | supported | 17,705 | 35 |
| 9 | 2-sided through step | supported | 17,793 | 39 |
| 10 | Slanted through step | unsupported | 17,565 | 82 |
| 11 | O-ring | supported | 17,326 | 0 |
| 12 | Blind hole | supported | 17,334 | 1 |
| 13 | Triangular pocket | supported | 17,152 | 8 |
| 14 | Rectangular pocket | supported | 17,652 | 5 |
| 15 | 6-sided pocket | supported | 17,456 | 4 |
| 16 | Circular end pocket | supported | 17,468 | 7 |
| 17 | Rectangular blind slot | supported | 4,190 | 4 |
| 18 | Vertical circular end blind slot | supported | 4,119 | 11 |
| 19 | Horizontal circular end blind slot | supported | 4,087 | 7 |
| 20 | Triangular blind step | supported | 17,799 | 19 |
| 21 | Circular blind step | supported | 17,954 | 11 |
| 22 | Rectangular blind step | supported | 17,643 | 20 |
| 23 | Round | supported | 16,741 | 6 |
| 24 | Stock | incomparable | 62,475 | 149 |

## Limitations

- No defining-face attribution, so no precision, recall or per-class agreement.
- Model-level silence is a weak signal on a corpus where most models carry many classes.
- MFInstSeg labels are unreliable at feature intersections, which is where the record deficit
  would be expected to concentrate.
- The corpus is synthetic. Nothing here estimates behaviour on real parts.
- MFInstSeg is not vendored and its terms are not reviewed here; only derived counts are recorded.
