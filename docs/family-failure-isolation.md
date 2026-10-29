# Optional detector failure isolation

Proposal for [#806](https://github.com/pzfreo/quiddity/issues/806). Maintainer review is pending.

The report and source-face evidence routes accept `continue_on_error=True`. For example:

```python
from quiddity.evidence import build_recognition_evidence

view = build_recognition_evidence(part, continue_on_error=True)
if not view.report.execution_complete:
    for failure in view.report.failures:
        print(
            failure.family,
            failure.stage,
            failure.exception_type,
            failure.message,
            failure.blocked_by,
        )

# Keep the report with these results; an empty failed family is not proven absent.
for feature in view.features:
    record = view.record(feature)
    original_faces = [view.face(ref) for ref in view.constituent_faces(feature)]
```

The same keyword is supported by raw/compatibility/framed report builders, the framed evidence
builder, and `PreparedFramedPart.recognise_report()` / `.recognise_evidence()`. It is independent
of `local_degradation`, which concerns invalid source geometry and keeps its existing proof rules.

Result-only builders, census, document and CLI routes remain strict. Their return contracts cannot
carry the new execution-failure report. No dependency versions, geometry predicates, feature
schemas, or default recognition outputs are changed.

## What the report means

`RecognitionReport.failures` is an immutable tuple of `RecognitionFailure` values, in physical
registry order followed by derived registry order. Each value contains:

| Field | Meaning |
| --- | --- |
| `family` | Physical detector ID or derived-pattern ID |
| `stage` | `discovery` or `derivation` |
| `exception_type` | Qualified top-level exception class, or `None` for a dependency skip |
| `message` | Top-level exception message, which may be empty; `None` for a dependency skip |
| `blocked_by` | Unavailable direct inputs in declared dependency order; empty for an exception |

No exception object, traceback frame, source geometry or private candidate identity is retained.
For physical detectors, `FamilyExplanation.evaluation` distinguishes `failed` and
`dependency-failed` from `evaluated` and `not-applicable`. Failed/skipped families have zero
published candidate counts, but those counts are not an evaluated absence. Derived failures and
skips are listed in `failures`; the physical-family explanation roster is unchanged.

`execution_complete` means only that no detector failed or was dependency-skipped. It is not an
accuracy or geometric completeness claim: explanation coverage remains bounded. Reconciliation
runs on surviving candidates; a missing competitor may affect which interpretations survive.
Consumers must retain the failure report and explicitly decide whether a partial inventory is
usable for their task. In particular, it must not become a clean completeness/census result.

## Recovery boundary

Only calls to an applicable physical definition's discovery function or a derived definition's
derivation function are recoverable. A failed physical family loses all of its unfinished
candidates, defining/constituent indexes, legacy claims and observations. Other families keep
their original run identities; neither geometry setup nor successful detectors are rerun.
Required physical dependents and derived patterns are skipped, transitively. Classification-gated
families retain `not-applicable` rather than being reported as failed dependencies.

Shared setup, applicability/input validation, output validation, family completion, final evidence
validation, reconciliation, diagnostics and result/evidence projection remain outside recovery.
Errors crossing the evidence-issuance and restricted-input authority layers also remain fatal,
even when raised from inside a detector. Their exception type and traceback are preserved.
`MemoryError`, `OSError` (including `TimeoutError`), `RecursionError`, `SystemError`, future
cancellation, OCCT `Standard_OutOfMemory`, and control-flow `BaseException`s propagate. Resource
or control failures in exception causes, contexts or groups also propagate. The existing bounded
retry for unproved hole ownership remains intact. Native crashes and unrecognized native resource
errors are not made recoverable by this Python boundary.

Exact defining/constituent evidence and solid attribution are still validated before publication.
Framed evidence retains exact caller-face mapping. If mapping is refused after a partial inventory,
the refusal carries no bare result, since that surface cannot attach its failure report. A complete
inventory retains the existing result-preserving mapping-refusal behavior.

## Evidence and scope

`tests/test_family_failure_isolation.py` uses the valid 16-face annular-pocket construction from
[#805](https://github.com/pzfreo/quiddity/issues/805): its pocket detector raises `StopIteration`,
while its independent pad is recoverable with exact original faces. Tests cover strict and
opt-in raw/framed/prepared routes, unfinished proposal and observation removal, dependency skips,
derived failure, fatal exceptions, immutable diagnostics and unchanged successful results.
The existing invalid-input tests also cover preservation of the ownership fallback.

The fixture is authored from boxes and cylinders; no production/customer geometry is included.
The opt-in successful path adds one bounded dependency check per definition and no geometric scans.
Failure cleanup traverses the existing evidence indexes; it does not copy/re-run the entire
inventory. No new recognition family or downstream CAD/drawing semantics are introduced.

Under ADR 0005 this additive optional Python report contract should be reviewed as a minor-release
change. No release/version bump is included in the proposal. Downstream adoption is deferred until
the contract is agreed and a stable version is published and independently tested. IR, DSL,
generated-code and drawing integration are not applicable to this orchestration-only proposal.
