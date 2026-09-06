# Architecture decision records

These records extract the recognition-specific decisions from Draftwright without importing
its drawing/compiler policy. Accepted records are the contracts this package holds itself to,
whether proven in Draftwright or established here by measurement; proposed records describe the
next recognition architecture and require evidence before acceptance.

Acceptance is a statement about the decision, not about the code. A record can be accepted while
work remains to bring the codebase to it -- that work belongs to an epic and its issues, so that
the record stays a decision rather than a status report.

| ADR | Title | Status |
| --- | --- | --- |
| [0001](0001-standalone-geometry-only-apache-library.md) | Standalone geometry-only Apache library | Accepted |
| [0002](0002-uniform-deterministic-recogniser-contract.md) | Uniform deterministic recogniser contract | Accepted |
| [0003](0003-one-recognition-result-and-explicit-reconciliation.md) | One recognition result and explicit reconciliation | Accepted |
| [0004](0004-attributed-geometry-graph-and-residual-evidence.md) | Attributed geometry graph and residual evidence | Accepted |
| [0005](0005-versioned-cross-repository-capability-contract.md) | Versioned cross-repository capability contract | Accepted |
| [0006](0006-explicit-step-ladder-z-span.md) | Explicit step-ladder Z-span boundary | Accepted |
| [0007](0007-recogniser-module-seams.md) | Internal recogniser module seams | Accepted |
| [0008](0008-length-tolerance-policy.md) | Length tolerance policy | Accepted |
| [0009](0009-filtering-belongs-to-a-recogniser.md) | Filtering belongs to a recogniser, not a shared reduction | Accepted |
| [0010](0010-narrow-public-geometry-facade.md) | Publish a narrow geometry facade; keep correspondence optional | Accepted |
| [0011](0011-explicit-part-relative-recognition-frame.md) | Pair local recognition with an explicit part frame | Accepted |
| [0013](0013-public-blend-chain-recognition.md) | Publish complete blend chains separately from dimension-worthy Fillets | Accepted |
| [0012](0012-bounded-recognition-explanations.md) | Publish bounded explanations beside recognition results | Accepted |
| [0014](0014-geometry-only-step-loading.md) | Load recognition inputs without STEP assembly metadata | Accepted |
| [0016](0016-planar-passage-termination-planes.md) | Represent planar Passage terminations in the section frame | Accepted |
| [0018](0018-edge-open-polygonal-recess-profile.md) | Preserve an edge-open polygonal recess as an open profile | Accepted |
| [0019](0019-unified-section-recess-json.md) | Unify constant-section recesses in one JSON geometry | Accepted |
| [0023](0023-observed-cylindrical-passage-ends.md) | Polygonal passages ending on an observed bore | Accepted |

Draftwright ADRs 0007, 0013, 0015 and 0017 are historical inputs, not normative records for this
project. Consumer-specific requirements, annotation provenance, lint and placement remain owned
by Draftwright.
