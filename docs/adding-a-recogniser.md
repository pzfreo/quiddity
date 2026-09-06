# Adding a recogniser

This is the shortest practical path for adding a feature recogniser without bypassing the
package's AAG evidence model or the framework introduced by epic 0003.

The central rule is simple:

> A recogniser discovers geometry and proposes records. Orchestration owns execution order,
> reconciliation owns conflicts, and projection owns the public aggregate.

Do not start by editing `result.py`. Start by defining the geometric claim the new family can
prove.

## 1. Write the supported geometry down first

Describe the smallest topology the recogniser will accept and the cases it will deliberately
reject. Use geometric facts, not corpus labels or model numbers.

For example:

- one cylindrical region with a principal axis;
- two hole-free planar side regions;
- complete concave shared seams between those regions;
- one opening at the source-solid envelope;
- an exact candidate volume containing no same-solid material.

State which faces are:

- **defining**: the feature record could not be established without them;
- **context**: inspected to prove a boundary, opening, material side, or exclusion, but not owned;
- **unrelated**: neither read nor claimed.

This distinction matters during reconciliation. Only defining faces may prove containment or
precedence. Empty defining evidence deliberately proves nothing. **Context is an analysis
category, not Candidate evidence**: the Candidate API records defining nodes only. The separate
Observation API records context for its one bounded failed-predicate diagnostic.

Also list adversaries before implementing: wrong curvature, incomplete seams, inner wires,
through instead of blind, material inside the proposed void, disconnected solids, split faces,
mirrors, traversal permutations, and relevant scale extremes.

This is the practical meaning of
[ADR 0004](adr/0004-attributed-geometry-graph-and-residual-evidence.md) and
[ADR 0009](adr/0009-filtering-belongs-to-a-recogniser.md): keep neutral graph facts total, and put
acceptance gates in the family that owns the decision.

## 2. Define a plain immutable record

Records are public geometry values, not OCP objects or handles into a recognition run.

```python
from dataclasses import dataclass

from quiddity._record import Record


@dataclass(frozen=True)
class ExampleFeature(Record):
    axis: str
    length: float
    width: float
    at: tuple[float, float, float]
```

Requirements:

- inherit `Record`;
- use a frozen dataclass;
- contain only JSON-serialisable values;
- describe geometry, not machining or drawing policy;
- round only when constructing the final public record, never while proving topology;
- preserve enough orientation and location to distinguish equal-sized occurrences.

Do not put `Face`, `Edge`, `Solid`, `FaceNode`, Candidate identity, or a consumer-specific concept
on the record. See [ADR 0001](adr/0001-standalone-geometry-only-apache-library.md),
[ADR 0002](adr/0002-uniform-deterministic-recogniser-contract.md), and
[ADR 0008](adr/0008-length-tolerance-policy.md).

## 3. Implement one public recogniser and one private discovery core

The public API remains independently useful:

```python
def recognise_example_features(
    part: Part,
    *,
    face_edges: FaceEdges | None = None,
    ledger: ClaimLedger | EvidenceWriter | None = None,
) -> list[ExampleFeature]:
    graph = ledger.graph if ledger is not None else FaceGraph(part, face_edges=face_edges)
    sink = None if ledger is None else ledger.sink
    return _discover_example_features(part, graph=graph, sink=sink)
```

The exact adapter depends on whether the family needs the compatibility `ledger=` sidecar. The
important private shape is narrower:

```python
def _discover_example_features(
    part: Part,
    *,
    graph: FaceGraph,
    sink: EvidenceSink | None,
) -> list[ExampleFeature]:
    proposals: list[tuple[ExampleFeature, tuple[FaceNode, ...]]] = []

    # Read graph facts, prove the family predicate, then append proposals.
    # Run the most expensive Boolean/material test only after cheap gates pass.

    proposals.sort(key=lambda item: (item[0].axis, item[0].at))
    if sink is not None:
        for record, defining in proposals:
            sink.propose(FamilyId.EXAMPLE_FEATURES, record, defining=defining)
    return [record for record, _defining in proposals]
```

Discovery may read the `Part` and neutral facts injected for that run. It may write proposals. It
must not read sibling claims, call a sibling `recognise_*` function, reconcile conflicts, or alter
its return depending on whether a writer was supplied.

New families should use `EvidenceSink.propose`, including when `defining=()` is deliberate. The
older `EvidenceWriter.add_defining` compatibility path requires at least one node and cannot state
empty evidence positively.

## 4. Understand the evidence lifecycle

One returned record occurrence follows this path:

```text
discover and optionally propose
        -> bind returned record occurrences to Candidates
        -> terminally freeze evidence
        -> assign exactly one disposition
        -> project accepted records
```

The binding step is `_bind_physical`, which calls
`ledger.candidate_set_for(family, returned_records)`:

- an explicitly proposed record is matched by **object identity**, so return the same object that
  was passed to `sink.propose`; do not rebuild it with `replace()` or an equal constructor;
- a returned record that was never proposed still receives an empty-evidence Candidate;
- an empty-evidence Candidate still receives a disposition, but its empty set cannot prove
  containment or precedence;
- a proposed Candidate omitted from the returned list fails closed;
- two equal-valued records remain different Candidate occurrences.

This lets simple families return records without claims while keeping one complete physical
inventory. It also makes absence of a claim explicit at binding rather than treating the record as
outside reconciliation.

## 5. Use the AAG as evidence, not as a label matcher

`FaceGraph` is the run-local attributed adjacency graph (AAG). Its nodes represent all faces and
carry neutral facts such as surface type, bounds, normals, and edge relationships. Its arcs carry
adjacency and convexity/concavity information.

Use direct AAG facts when one kernel face is sufficient. Use a logical or generalised AAG (gAAG)
query when harmless STEP subdivision splits one geometric region across several faces. Existing
examples include coplanar regions, smooth regions, shared-edge coverage, and same-cylinder
normalisation.

A sound predicate normally has several reinforcing layers:

1. **Analytic geometry:** correct plane, cylinder, cone, radius, axis, or angle.
2. **Topology:** valid wire count, expected boundary kinds, and no unsupported holes or branches.
3. **AAG relations:** complete concave/convex seams to the expected logical neighbours.
4. **Context:** envelope, opening, material-side, or body ownership checks.
5. **Occupancy:** when topology cannot prove a void, an exact same-solid Boolean check performed
   last.

Do not accept from a bounding box alone. Do not merge same-radius arcs without proving the same
support circle and direction. Do not infer a missing wall from a cap unless the family contract
explicitly proves that representation.

When using tolerances, choose the smallest local nominal that controls the comparison through
`length_tol`. Keep dimensionless tolerances dimensionless. A minimum feature size is a deliberate
manufacturing threshold, not a tolerance, and needs separate justification under ADR 0008.

## 6. Add the family to the closed registry

Every physical aggregate family has one `FamilyId` and one ordered `PhysicalDefinition` in
`_registry.py`.

```python
class FamilyId(Enum):
    EXAMPLE_FEATURES = "example_features"


PhysicalDefinition(
    FamilyId.EXAMPLE_FEATURES,
    (ExampleFeature,),
    "example_features",
    "recognise_example_features",
    (),  # completed physical dependencies
    always,  # discovery applicability
    _example_adapter,  # aggregate discovery adapter
    Counted("example"),  # or NotCounted("reason")
    projected=always,  # public projection applicability
)
```

The registry owns orchestration metadata, not geometry:

- `dependencies` may name only earlier physical families whose completed records are required;
- the adapter receives only those declared inputs;
- `applicable` decides whether discovery runs from neutral `RecognitionContext` only;
- `projected` decides whether accepted records appear in the public aggregate;
- census participation is explicit;
- record types and result/public fields are checked independently.

Both applicability fields currently accept only the reviewed neutral predicates `always` and
`prismatic`. A new kind of context gate requires a framework design review and must be added to
the closed registry validator; an arbitrary lambda is rejected.

Do not import `_registry` from a family module. The dependency direction is registry → family.
See [ADR 0007](adr/0007-recogniser-module-seams.md).

## 7. Wire the public aggregate deliberately

Add the record tuple to `RecognitionResult` and map it in `_project_result` using `_records`:

```python
example_features: tuple[ExampleFeature, ...]

# In _project_result:
example_features = tuple(_records(accepted, FamilyId.EXAMPLE_FEATURES, ExampleFeature))
```

Then update the independent public surfaces:

- import and `__all__` in `quiddity/__init__.py`;
- the manual capability metadata in `tools/generate_capability_manifest.py`;
- `docs/capabilities.md` with the supported and excluded geometry;
- census binding, or an explicit `NotCounted` reason;
- snapshot/per-face tooling when it enumerates aggregate fields.

Regenerate, do not hand-edit, the committed capability manifest:

```bash
uv run python tools/generate_capability_manifest.py --write
```

Public exports, result fields, registry metadata, record annotations, capability metadata, census
bindings, and archive contents intentionally remain separate contracts. Tests compare them so a
forgotten integration step fails visibly. See
[ADR 0005](adr/0005-versioned-cross-repository-capability-contract.md).

## 8. Extend the semantic goldens deliberately

Adding a public `recognise_*` name or census key intentionally breaks every semantic parity
fixture until its snapshot shape is extended.

There are two different sources of golden data:

- `tools/capture_draftwright_goldens.py` captures the pinned historical Draftwright baseline;
- families originated in this package do not exist in that baseline and cannot be recaptured from
  it.

For a package-originated family:

1. add its recogniser name to the explicit package-originated list in
   `tools/recognition_snapshot.py`;
2. add its `individual` result and aggregate field to every `tests/golden/*/expected.json`;
3. if it is `Counted`, add its census key to every expected census object;
4. add a dedicated fixture whose geometry positively proves the new record when the existing
   fixtures do not;
5. generate the new values from the reviewed package implementation, but preserve all existing
   pinned Draftwright fields byte-for-byte;
6. review the JSON diff: existing values must not move unless the PR explicitly declares a
   compatibility change.

Do **not** overwrite the corpus with `capture_draftwright_goldens.py` to make a new package family
pass; the pinned source cannot emit it. `tests/test_golden_parity.py` uses exact equality precisely
so a missing integration or an unrelated movement is visible.

## 9. Add reconciliation only for a real overlap

Discovery proposes every locally valid interpretation. A recogniser must not suppress itself
because another family may win later.

If the new family overlaps an existing one:

1. write a geometry counterexample showing both proposals are locally valid;
2. define the narrow identity/evidence relationship that chooses the winner;
3. add a named `ReasonCode` and reason specification;
4. implement the rule in `_reconcile.py` over completed `CandidateSet`s and frozen
   `EvidenceIndex`;
5. relate the losing Candidate to the actual winning Candidate identities;
6. keep empty evidence from proving containment;
7. test equal-valued but identity-distinct candidates and reversed input order.

Reconciliation receives no `Part`, mutable writer, or discovery function. Every physical proposal
must finish with exactly one accepted or rejected disposition. Do not add precedence merely because
two labels coexist in a corpus. See
[ADR 0003](adr/0003-one-recognition-result-and-explicit-reconciliation.md).

## 10. Keep derived recognisers separate

A pattern or other record computed only from accepted records is derived, not physical. Give it a
`DerivedId` and `DerivedDefinition`; do not issue Candidates or dispositions for it.

```python
DerivedDefinition(
    DerivedId.EXAMPLE_PATTERNS,
    (ExamplePattern,),
    "example_patterns",
    "recognise_example_patterns",
    (FamilyId.EXAMPLE_FEATURES,),
    _derive_example_patterns,
    Counted("example_pattern"),
)
```

The derived function receives accepted records only and must not inspect the Part or rediscover
geometry.

## 11. Test the predicate, integration, and failure boundary

At minimum add:

- one direct positive and the same positive through `build_recognition_result`;
- all principal axes, relevant signs/quadrants, mirrors, translation, and traversal permutation;
- harmless coplanar/smooth/analytic face subdivision, including combined subdivisions;
- a STEP round-trip fixture;
- scale tests appropriate to the predicate;
- every defining negative from the written contract;
- same-solid and compound-body ownership cases;
- exact defining-face claims and no stock/context claims;
- identical records with different Candidate identities when reconciliation is involved;
- public signature, frozen record, JSON, export, registry, census, capability, and golden coverage;
- a performance measurement when adding a scan, Boolean, or broad candidate loop.

For a genuinely new predicate, follow the project review gate: development evidence first, two
independent accepts, then reveal and pin the authorised holdout. Never tune a predicate after
looking at the holdout; once revealed, that holdout is spent.

Useful commands:

```bash
uv run pytest -q --no-cov tests/test_example_features.py
uv run pytest -q
uv run ruff check src tests tools
uv run mypy src
git diff --check
uv run python tools/generate_capability_manifest.py --check
uv run python tools/benchmark_recognition.py \
  --implementation package --workload composite --iterations 5
```

## Review checklist

- [ ] The supported geometry and exclusions are written before implementation.
- [ ] The record is immutable, serialisable, and geometry-only.
- [ ] The family owns its gates; shared reductions do not silently filter faces.
- [ ] AAG/gAAG seams and context ownership are complete, not bounding-box guesses.
- [ ] Expensive Booleans run only after cheap gates.
- [ ] Discovery is write-only and does not call sibling recognisers.
- [ ] Defining claims contain all and only the faces establishing the record.
- [ ] Registry dependencies, discovery applicability, projection applicability, and census policy
  are explicit.
- [ ] Reconciliation is identity-safe, phase-pure, and justified by a real overlap.
- [ ] Derived records consume accepted records only.
- [ ] Public exports, aggregate, capabilities, census, tooling, and goldens agree.
- [ ] Adversaries, subdivision, orientation, compound, determinism, and performance gates pass.
