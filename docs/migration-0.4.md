# Migrating passage attribution to 0.4

Version 0.4 adds `SectionPassage` as the sole attributed `PASSAGES` record. It represents
principal and free-axis constant-section passages with a canonical frame, run interval, complete
section, and explicit open ends.

Writer-free `recognise_passages(part, ledger=None)` remains the compatibility API for legacy
principal-axis `Passage` values. Its values, ordering, and `to_dict()` contract are unchanged.
Passing any non-`None` `ClaimLedger` or `EvidenceWriter` now raises `PassageCompatibilityError`
before geometry discovery or ledger mutation. Attributed callers must migrate to:

```python
records = recognise_section_passages(part, ledger=ledger)
```

`RecognitionResult.section_passages` is the physical, counted result, and is now the only passage
output the aggregate carries. The `RecognitionResult.passages` compatibility projection has been
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
