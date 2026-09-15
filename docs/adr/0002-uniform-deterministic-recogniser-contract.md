# ADR 0002 — Uniform deterministic recogniser contract

- **Status:** Accepted
- **Date:** 2026-08-15
- **Decider:** Paul Fremantle
- **Record form:** current state, rewritten 2026-09-15; absorbs ADR 0009 (filtering belongs to a
  recogniser). The per-issue amendments this file carried until then are in `git log` on it.

## Context

Recognition functions historically differed in naming, signatures, return shapes, dependency
handling and serialisation. Draftwright ADR 0013 established and mechanically tested a uniform
contract before extraction; this record is that contract as the package holds it today.

## Decision

A physical recogniser is a public function of one shape:

```python
recognise_<feature>(part, *, <tuning>, <injected evidence>, [<claim sidecar>]) -> list[RecordType]
```

A derived recogniser is a pure function of records already produced:

```python
recognise_hole_patterns(holes) -> list[PatternRecord]
```

**Records.** Every public record is a typed frozen dataclass containing only serialisable
geometry values, with a stable `to_dict()` projection. Schema changes are additive, are versioned
in the capability manifest ([ADR 0005](0005-versioned-cross-repository-capability-contract.md)),
and default so that hand-built earlier-schema records still construct. Empty
means confidently absent within the recogniser's documented supported domain; ambiguity or
unsupported topology is diagnostic output, not an empty-list alias.

**Determinism.** A recogniser is deterministic with respect to equivalent input geometry and its
configured tolerance. It never calls a sibling recogniser. Orchestration computes reusable
evidence once (cylinder inventory, face graph, face-edge memo) and injects it; a recogniser
called standalone derives what it was not given.

**Injected evidence is read; the claim sidecar is written.** Where a family has migrated onto the
evidence seam, the one mutable parameter is `ledger: ClaimLedger | EvidenceWriter | None = None`. During discovery a recogniser appends the
faces each record was established by and never reads back, so no family's output can depend on
which family ran first. On valid closed-solid input, passing a ledger changes nothing about the
return value; on open or ambiguous topology the public facade may still return records for
compatibility while the writer-enabled core refuses before publication. A ledger built from a
different part is refused, not silently ignored.

**One private core, one public facade.** The public function is a writer-free facade over a
private core that takes an optional writer, and the registry calls that core, or the public
function itself with `ledger=` where the family has no separate core. Across the
two calls parity means record type, value, order and `to_dict()`, not Python identity. Within a
writer-enabled run each Candidate retains the exact returned record occurrence, so equal-valued
occurrences stay distinct.

**Defining versus consulted evidence.** A record's claim names only the original faces that
establish it: the walls that set a slot's width, the bore patches of a hole, the six sides of a
polygonal boss. Floors, caps, probes, neighbours and stock the recogniser consulted are context
and are never claimed. Each family's defining set is pinned by its attribution tests
(`tests/test_*_attribution.py`) and summarised in [`capabilities.md`](../capabilities.md).

**Body-local occurrences.** Equal-valued records on separate solids are separate occurrences;
deduplication happens within one valid solid, never across solids. Body-owned records carry an
optional opaque `body_key` derived from the source solid's frame-local bounds, volume and area
so a consumer can correlate records of one body within one result. Separate solids with an
equal signature receive `None`; traversal order and kernel handles never break that tie. Keys
and scans come from the actual solids, never from a compound wrapper's own mass properties; the
original input is a fallback only when no solid exists.

**Spelling.** Public recognisers use British `recognise_`. Substrates returning evidence rather
than accepted features use precise verbs such as `analyse_cylinders`.

**Shared reductions are total; rejection is a gate.** A reduction used by more than one
recogniser returns one output per input element, with an attribute that does not apply left
absent rather than the element dropped. Rejecting a candidate is the recogniser's decision and
belongs where it can be named, counted and tested: a gate inside a family is visible, relaxable
and measurable; a filter inside a shared helper is invisible to every family that inherits it and
leaves nothing downstream to count. `FaceGraph` obeys this by construction, carrying every face
and computing attributes lazily, which is what makes totality affordable. Where a shared reduction
cannot be made total, the exclusion is documented on every family that inherits it in
[`capabilities.md`](../capabilities.md), naming the shared function.

**Passages.** `recognise_section_passages` is the physical entry point. `recognise_passages` is a
writer-free legacy projection and raises `PassageCompatibilityError` if handed a ledger, so there
is one Passage evidence authority.

## Enforced by

- `tests/test_recogniser_contract.py`: signature checks over the part-based recognisers and
  frozen, JSON-serialisable records with no build123d/OCP objects;
  `tests/test_capability_manifest.py`: return annotations against the manifest.
- Determinism tests: permuting kernel traversal order does not alter record order.
- Per-family claim tests (`tests/test_*_claims.py`): same records with and without a ledger;
  claims asserted against the geometry the faces have, not a captured count; foreign ledger refused.
- Mutation tests proving each injected dependency is used rather than recomputed.
- Per-family gate tests: a family that declines geometry does so in its own module, and the
  shared graph and surface readers expose every face or a closed refusal.

## Consequences

The contract favours predictable composition over one-off convenience. An aggregate that seems too
small for a list still receives a self-contained record, not a special return shape. The cost is
that every family pays the same facade-plus-core structure; the benefit is that
[ADR 0003](0003-one-recognition-result-and-explicit-reconciliation.md) can reconcile records it
did not produce.
