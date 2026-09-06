# Geometric failure boundaries — issue #520

Audit baseline: `4ddfd65`, epic #514. This is a bounded inspection of the reported proof paths,
not a claim that every kernel operation has been standardized.

## Findings and changes

| Boundary | Evidence | Disposition |
| --- | --- | --- |
| Round-bottom planar region construction | `Face` caught every `Exception`; an injected assertion became ordinary absence. `Wire.combine` propagated the equivalent kernel construction failure. | Catch `Standard_Failure`, `RuntimeError`, and `ValueError` at these construction calls. Attribute/type/assertion errors propagate. Construct and validate a region face only once. |
| Double-D opening and declared-tool prism proofs | Broad catch masked programming errors; scalar-only `.volume` access could also mask fragmented boolean results as missed recognition. | Use shared boolean-volume normalization; narrow both catches to the same expected construction/kernel classes. |
| Section-passage `_void_and_open` | Explicit construction/arithmetic exceptions already delimit an existing geometric proof. | Retained; no evidence here justifies changing its full acceptance policy. |
| Adjacency ownership and differential queries | Broad catches wrap source validity, normal evaluation or analytic continuation, with explicit unproved ownership/side outcomes. | Retained in this increment. Their kernel/input domain differs from constructing a proved planar region; replacing every catch mechanically would not establish better semantics. |

OCCT's installed `Standard_Failure` derives directly from `Exception`, not `RuntimeError`; it
must be named explicitly when narrowing kernel catches. Python `ValueError` is also used by
build123d for non-closed face wires and failed construction. These classes are caught at bounded
proof operations, not installed as a package-wide error suppressor.

## Validation and interpretation

Injected kernel construction errors refuse the candidate. Injected assertion, attribute and type
errors remain visible. Authored Double-D controls cover empty and fragmented boolean results,
including nonempty material refusal. Existing round-bottom and Double-D geometry tests retain
positive/negative recognition controls; architecture tests enforce the shared-helper edge.

No diagnostic claims to enumerate all unsupported geometry. Existing `None`/empty candidate
results remain bounded refusals, not proof of feature absence. This change adds no second scan,
new exception framework, corpus inspection or global error-policy rewrite. Additional exception
changes should require a concrete failing proof boundary and an injected-error regression.
