# Migrating passage attribution to 0.4

Version 0.4 adds `SectionPassage` as the sole attributed `PASSAGES` record. It represents
principal and free-axis constant-section passages with a canonical frame, run interval, complete
section, and explicit open ends.

Writer-free `recognise_passages(part, ledger=None)` remains the compatibility API for legacy
principal-axis `Passage` values. Its values, ordering, and `to_dict()` contract are unchanged.
Passing any non-`None` `ClaimLedger` or `EvidenceWriter` now raises `PassageCompatibilityError`
before geometry discovery or ledger mutation. Attributed callers must migrate to:

```python
from quiddity.passages import recognise_section_passages

records = recognise_section_passages(part)
```

That is writer-free, per ADR 0002, and returns records only.

**Attribution does not arrive under a passages family, and often does not arrive as a passage at
all.** Two things happen between standalone discovery and a run:

1. There is no `passages` family in the evidence projection. What a run publishes is a
   `SectionRecess`, the unified constant-section record.
2. A run *reconciles*. A passage that describes the same void as an oriented slot is dropped in
   favour of the slot, because both are true of the ring and only one should be counted.

Measured over the golden corpus, `recognise_section_passages` finds **11** passages across four
fixtures; a run publishes **one** of them with
`classification.feature_kind == "passage"`. The other ten are the same voids, published under
the family that won reconciliation.

So an attributed caller migrating off `recognise_passages(..., ledger=...)` should not filter
for a passages family, and should not filter the projection for `feature_kind == "passage"`
either: both find nothing for most shapes and raise no error. Look for the void under whichever
family the run accepted, or use `recognise_section_passages(part)` standalone when the question
really is "what rings are there", independent of how a complete interpretation resolves them.

`SectionPassage` is now the only passage record a run carries, and
`quiddity.passages.recognise_section_passages` returns it standalone -- the function is not
root-exported, though the record type is. The family is declared `NotCounted`: a run counts it
once through the unified section-recess projection, which is also where `RecognitionResult`
exposes it, as `section_recesses` rather than a passages field of its own. The `RecognitionResult.passages` compatibility projection has been
removed: it owned no Candidate or evidence, was not counted separately, and was only ever a stable
subsequence of the standalone legacy result rather than a promise that every historical legacy
record had a rich equivalent. Writer-free `recognise_passages` still returns legacy records
directly for callers that want them.

This intentionally narrows aggregate compatibility before 1.0. On the checked-in
`10060.step` regression, standalone legacy output remains the two-element `(X, Z)` sequence: the
partial-span X false positive has no rich occurrence, and the truthful Z occurrence is the one
`recognise_section_passages` reports. Consequently `feature_census` reports one Passage instead of
the historical two; all other census keys and the exact Slot, Pocket and Prismatic Pocket
dispositions remain unchanged on that part.

The package capability manifest is format 2. Each recogniser declares whether it is a physical
authority, compatibility projection, or derived API, and each counted family names its
authoritative aggregate output. Consumers must explicitly support format 2 before accepting the
0.4 package range. The existing `passages` family identifier and historical `introduced_in` value
do not change; all five new records use schema version 1.
