# Issue 485 authored correspondence corpus

The executable corpus is `tests/test_correspondence.py`. It is synthetic and authored from the
contract before implementation; no MFCAD++, MFInstSeg or external model label selected a matching
predicate or numerical bound.

| Revision case | Required outcome |
| --- | --- |
| Plain datum face, unrelated through hole added | same planar face resolves despite a new inner loop |
| Top and bottom datum face set, unrelated hole added | the complete set resolves as one subject |
| One hole resized at the same opening and axis | resolves to the resized hole |
| One uniquely specified hole moved | resolves without a distance threshold |
| Equal-spec hole added beside an unchanged hole | the exact occurrence wins |
| One old hole and two equal moved alternatives | ambiguous |
| Coincident equal bodies | equal face alternatives are ambiguous |
| Two old holes competing for one current hole | neither silently steals the occurrence |
| Removed hole | missing |
| Different caller lineage | incompatible |
| Modified or malformed payload | rejected before matching |

The corpus pins `analytic-v1`, not the public protocol's future ceiling. A later matcher adds its
own strategy-specific cases and keeps these receipts either resolvable by v1 or explicitly
incompatible. Existing recognition goldens and external-corpus outputs are expected to remain
byte-identical because correspondence is an optional projection over a completed evidence view.
