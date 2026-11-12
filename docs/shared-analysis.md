# Shared recognition and evidence analysis

The Python entry point uses the same native exception boundary as `recognise` and
`evidence`: an unexpected uncaught native panic raises `QuiddityError`, an
`Exception` subclass, while ordinary invalid inputs raise `ValueError`. Family
failures already isolated by the engine remain in the returned failure ledger.

When both recognition results and face evidence are needed, `recognise_with_evidence`
shares STEP reading, frame inference, working geometry, the candidate inventory and
section-recess projection, returning both complete documents. The existing `recognise`
and `evidence` APIs and their respective outputs remain unchanged.

## Python API

```python
import json
import quiddity

analysis = json.loads(quiddity.recognise_with_evidence("part.step"))
recognition = analysis["recognition"]
if analysis["evidence_error"] is not None:
    raise RuntimeError(analysis["evidence_error"])
evidence = analysis["evidence"]
```

The argument is a `str` file path or STEP `bytes`. The result is a newline-terminated JSON string:

| Field | Content |
| --- | --- |
| `schema_version` | `quiddity-rust/analysis/1` |
| `recognition` | The complete original `recognise(step)` result, including features, fingerprints, frame and `document` |
| `evidence` | The complete original evidence document, or `null` when evidence is refused |
| `evidence_error` | The original evidence refusal text, or `null` on success |

File paths support `.gz`. Recognition's `source_sha256` hashes the original input bytes,
including the compressed bytes of compressed files, as in the existing recognition API.
Reading, decompression or STEP parsing failures raise `ValueError`; unsupported argument
types raise `TypeError`. The input is read once, so both documents derive from the same
STEP content.

## Coordinates, categories and error semantics

Recognition document parameters remain in file coordinates. When a part frame can be
inferred, joint evidence is equivalent to `evidence(step, framed=True)` and uses part
coordinates. Otherwise it is equivalent to `framed=False` and uses file coordinates.
The joint API has no independent `framed` argument. Consumers requiring fixed file
coordinates can continue using the existing evidence API. Face/edge indices and
`face_sources`/`edge_sources` retain the original STEP identities and are not renumbered
by the coordinate transformation.

Each document retains its own categories, members, face ownership, candidates, refusals
and proof status; they are not collapsed into one feature list. Recognition publication
post-passes can add face claims or withdraw feature families, so the shared implementation
retains the pre-publication inventory before producing recognition and evidence separately.
Evidence constituent-face discovery, recognition fingerprints and serialization of both
documents still run.

An evidence failure preserves recognition and is explicitly returned as `evidence: null`
with `evidence_error`. Callers must check this error and continue to respect each document's
`complete`, `failures` and proof status. A successful API return alone does not establish
solid evidence for every geometric conclusion.

A returned section-recess projection error is recorded in the shared failure ledger.
Both calls retain independent evidence with `complete: false`, just as the separate
recognition and evidence APIs do. The joint envelope keeps that partial evidence and
does not turn this family-local failure into an `evidence: null` refusal. Input,
reconciliation and document-identity errors retain their existing fatal boundaries.

The normal path builds one candidate inventory. Strict-to-local-degradation retries and
rediscovery required after recognition publication failures remain intact. One shared
call does not mean that discovery runs only once for every possible input.

## Rust APIs and regression tests

- `correspondence::recognise_with_evidence(&Part)` returns a `#[non_exhaustive]`
  `RecognitionWithEvidence { recognition, evidence }`, where `evidence` is a
  `Result<EvidenceDocument, EvidenceViewError>`.
- `file_api::recognise_with_evidence_file(&str)` and
  `file_api::recognise_with_evidence_bytes(Vec<u8>)` return the JSON envelope above.
- `capabilities()` lists the new API in `functions` and declares the envelope version in `analysis`.

The Rust `shared_analysis` tests compare the separate and joint APIs field by field,
including moved geometry, source identities, refused frames, local degradation and fault
injection. Trace assertions check that normal-path candidate discovery and consumer
section-recess projection each run once. `tests/python/test_shared_analysis.py` checks
the public binding, type declaration, path/bytes inputs, digests and error semantics.
These tests supplement the existing Rust snapshots and independent geometry, label,
motion-invariance, inventory-floor and NIST checks. They do not replace those checks,
change snapshot expectations or loosen comparison tolerances.

## Instruction-count benchmark

The [Windows benchmark report](https://github.com/xu9299/quiddity-rust/blob/aa309fcb473629fe799f7915fb5503162214763e/tools/shared_analysis/benchmark-windows-msvc.md)
records calibrated DynamoRIO software instruction counts for separate and joint
calls, with complete-output equality checks. The optional harness and raw results
remain in that fixed fork revision. No benchmark tooling or runtime dependency is
added to this repository.
