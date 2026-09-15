# Architecture decision records

An ADR states a decision the package holds itself to and what enforces it. It is a statement
about the decision, not about the code: a record can be accepted while work remains to bring the
codebase to it, and that work belongs to an epic and its issues.

**A record is kept current, not appended to.** When a decision changes, the record is rewritten in
place to say what is true now; the history is `git log` on the file. A record whose decision is
replaced gets `Status: Superseded by NNNN` and stays in this directory so that citations of its
number in source and tests keep resolving. Numbers are never reused or renumbered.

## Core decisions

Read these in order to understand how the package is built. Together they are the architecture.

| ADR | Title | Status |
| --- | --- | --- |
| [0001](0001-standalone-geometry-only-apache-library.md) | Standalone geometry-only Apache library | Accepted |
| [0002](0002-uniform-deterministic-recogniser-contract.md) | Uniform deterministic recogniser contract | Accepted |
| [0003](0003-one-recognition-result-and-explicit-reconciliation.md) | One recognition result and explicit reconciliation | Accepted |
| [0004](0004-attributed-geometry-graph-and-residual-evidence.md) | Attributed geometry graph and residual evidence | Accepted |
| [0007](0007-recogniser-module-seams.md) | Internal recogniser module seams | Accepted |
| [0008](0008-length-tolerance-policy.md) | Length tolerance policy | Accepted |
| [0009](0009-filtering-belongs-to-a-recogniser.md) | Filtering belongs to a recogniser, not a shared reduction | Accepted |
| [0011](0011-explicit-part-relative-recognition-frame.md) | Pair local recognition with an explicit part frame | Accepted |
| [0005](0005-versioned-cross-repository-capability-contract.md) | Versioned cross-repository capability contract | Accepted |
| [0010](0010-narrow-public-geometry-facade.md) | Publish a narrow geometry facade; keep correspondence optional | Accepted |
| [0012](0012-bounded-recognition-explanations.md) | Publish bounded explanations beside recognition results | Accepted |
| [0014](0014-geometry-only-step-loading.md) | Load recognition inputs without STEP assembly metadata | Accepted |
| [0019](0019-unified-section-recess-json.md) | Unify constant-section recesses in one JSON geometry | Accepted |

## Family records

Each of these decides the geometric contract of one recogniser family or one published value.
Read the one for the family you are changing; none is needed to understand the architecture.

| ADR | Title | Status |
| --- | --- | --- |
| [0006](0006-explicit-step-ladder-z-span.md) | Explicit step-ladder Z-span boundary | Accepted |
| [0013](0013-public-blend-chain-recognition.md) | Publish complete blend chains separately from dimension-worthy Fillets | Accepted |
| [0016](0016-planar-passage-termination-planes.md) | Represent planar Passage terminations in the section frame | Accepted |
| [0018](0018-edge-open-polygonal-recess-profile.md) | Preserve an edge-open polygonal recess as an open profile | Accepted |
| [0020](0020-native-cylindrical-section-ends.md) | Native cylindrical SectionRecess ends | Accepted |
| [0021](0021-proved-interior-support-apertures.md) | Independently proved interior support apertures | Accepted |
| [0022](0022-observed-cylindrical-channel-ends.md) | Observed cylindrical channel terminations | Accepted |
| [0023](0023-observed-cylindrical-passage-ends.md) | Polygonal passages ending on an observed bore | Accepted |
| [0024](0024-observed-convex-plane-envelope-ends.md) | Observed convex two-plane passage ends | Accepted |
| [0025](0025-body-owned-planar-outer-profile-inspection.md) | Body-owned planar outer-profile inspection | Accepted |

Draftwright ADRs 0007, 0013, 0015 and 0017 are historical inputs, not normative records for this
project; `tests/test_published_prose.py` keeps runtime prose from citing them. Consumer-specific
requirements, annotation provenance, lint and placement remain owned by Draftwright.
