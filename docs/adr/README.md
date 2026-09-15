# Architecture decision records

An ADR states a decision the package holds itself to and what enforces it. It is a statement
about the decision, not about the code: a record can be accepted while work remains to bring the
codebase to it, and that work belongs to an epic and its issues.

**A record is kept current, not appended to.** When a decision changes, the record is rewritten in
place to say what is true now; the history is `git log` on the file. A record whose decision is
absorbed or replaced gets `Status: Superseded` with a pointer and stays in this directory so that
citations of its number in source and tests keep resolving. Numbers are never reused or renumbered.

**A record here is architecture.** A decision about one recogniser family is a
[family record](../families/README.md), not an ADR.

## The eight live records

Read them in this order. The first seven are how recognition is built; the eighth is what leaves
the package. Every commit is answerable to them.

| ADR | Decides | Status |
| --- | --- | --- |
| [0001](0001-standalone-geometry-only-apache-library.md) | What the library is: geometry in, records out, no consumer policy, no metadata | Accepted |
| [0002](0002-uniform-deterministic-recogniser-contract.md) | The shape of a recogniser: signature, records, determinism, claims, gates not filters | Accepted |
| [0003](0003-one-recognition-result-and-explicit-reconciliation.md) | One run, one result; discovery then named reconciliation | Accepted |
| [0004](0004-attributed-geometry-graph-and-residual-evidence.md) | The immutable face graph and the evidence layers above it | Accepted |
| [0007](0007-recogniser-module-seams.md) | Private modules, the enforced seam table, the layering | Accepted |
| [0008](0008-length-tolerance-policy.md) | Tolerances scale, thresholds do not, absolute constants are justified and bounded | Accepted |
| [0011](0011-explicit-part-relative-recognition-frame.md) | Recognition in an explicit part frame; families covariant with it | Accepted |
| [0005](0005-versioned-cross-repository-capability-contract.md) | The four published surfaces and how their contracts are versioned | Accepted |

## Superseded

Kept so that `ADR NNNN` citations resolve. Each stub says where its decision now lives.

| ADR | Was | Now in |
| --- | --- | --- |
| [0006](0006-explicit-step-ladder-z-span.md) | Explicit step-ladder Z-span boundary | [family record](../families/step-ladder-z-span.md) |
| [0009](0009-filtering-belongs-to-a-recogniser.md) | Filtering belongs to a recogniser | ADR 0002 |
| [0010](0010-narrow-public-geometry-facade.md) | Narrow geometry facade; correspondence optional | ADR 0005 |
| [0012](0012-bounded-recognition-explanations.md) | Bounded explanations | ADR 0005 |
| [0013](0013-public-blend-chain-recognition.md) | Public blend chains | [family record](../families/blend-chains.md) |
| [0014](0014-geometry-only-step-loading.md) | Geometry-only STEP loading | ADR 0001 |
| [0016](0016-planar-passage-termination-planes.md) | Planar Passage terminations | [family record](../families/planar-passage-ends.md) |
| [0018](0018-edge-open-polygonal-recess-profile.md) | Edge-open polygonal recess profile | [family record](../families/edge-open-polygonal-recess.md) |
| [0019](0019-unified-section-recess-json.md) | Unified section-recess JSON | [family record](../families/section-recess-json.md) |
| [0020](0020-native-cylindrical-section-ends.md) | Native cylindrical section ends | [family record](../families/cylindrical-section-ends.md) |
| [0021](0021-proved-interior-support-apertures.md) | Interior support apertures | [family record](../families/interior-support-apertures.md) |
| [0022](0022-observed-cylindrical-channel-ends.md) | Cylindrical channel terminations | [family record](../families/cylindrical-channel-ends.md) |
| [0023](0023-observed-cylindrical-passage-ends.md) | Passages ending on an observed bore | [family record](../families/cylindrical-passage-ends.md) |
| [0024](0024-observed-convex-plane-envelope-ends.md) | Convex two-plane passage ends | [family record](../families/plane-envelope-passage-ends.md) |
| [0025](0025-body-owned-planar-outer-profile-inspection.md) | Planar outer-profile inspection | [family record](../families/planar-outer-profile.md) |

Draftwright ADRs 0007, 0013, 0015 and 0017 are historical inputs, not normative records for this
project; `tests/test_published_prose.py` keeps runtime prose from citing them. Consumer-specific
requirements, annotation provenance, lint and placement remain owned by Draftwright.
