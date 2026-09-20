# Recognition effectiveness baseline method

This is the reproducible evidence contract for Epic 0005 E0 ([#293](https://github.com/pzfreo/b123d-recognisers/issues/293)).
It measures the shipped aggregate recogniser against external annotations without making those
annotations recognition policy.

## Evidence authority

Each model is imported once and passed once through the package's frozen `InventoryProduct`.
Counts come from accepted physical candidates; defining-face comparisons come from the evidence
owned by those candidates; reconciliation drops come from final dispositions; supported residuals
come from the existing bounded diagnostics. The scorer never calls individual recognisers and does
not reconstruct candidate ownership from public record coordinates.

This follows ADR 0003's one-inventory and reconciliation authority and ADR 0004's distinction
between geometric evidence and corpus labels. A matching label is comparison evidence, not
permission to change ownership, tolerance, or public semantics.

Historical reports through E5d use
[`effectiveness-taxonomy-v1.json`](effectiveness-taxonomy-v1.json). E5f and later reports use
[`effectiveness-taxonomy-v2.json`](effectiveness-taxonomy-v2.json), which moves MFCAD++ class 21
from the former Fillet proxy to the dedicated Circular Blind Step family. Reports after the
class-7 scope audit use [`effectiveness-taxonomy-v3.json`](effectiveness-taxonomy-v3.json), which
marks MFCAD++/MFInstSeg `Circular through slot` unsupported: the audited corpus geometry is an exact
semicylindrical groove, outside the shipped two-opposed-wall `Slot` contract. Reports after the
rectangular class-6 audit use [`effectiveness-taxonomy-v4.json`](effectiveness-taxonomy-v4.json),
which marks `Rectangular through slot` partially supported: the corpus class mixes a dominant
open-ended three-wall edge slot with closed, free-axis and intersected variants, rather than one
geometry covered by the enclosed `Slot` record. Every published class is `supported`, `partial`,
`unsupported`, or `incomparable`; `Stock` is incomparable. A supported or partial class names all
package families that can legitimately report that geometry. This is deliberately many-to-many:
for example, a rectangular ring can be proposed by both Pocket and Prismatic Pocket machinery,
while aggregate reconciliation decides which occurrence survives.

The runner defaults to the latest checked-in mapping, currently
[`effectiveness-taxonomy-v13.json`](effectiveness-taxonomy-v13.json). Canonical commands still name
that file explicitly so a copied command cannot silently acquire a later mapping. Historical
reports retain their recorded mapping and are never reinterpreted through the current default.

`partial` preserves honest matched evidence and denominators when a corpus class contains a
geometrically supported subset but its label also covers materially different shapes outside the
mapped family contract. It qualifies interpretation; it does not turn out-of-contract faces into
false negatives or authorize a production predicate. Reports retain the full class numerator and
denominator so readers can see the mixed-class agreement rather than erasing genuine matches.

## Metrics and denominators

Every ratio is stored as `{numerator, denominator, value}`. `value` is `null`, not zero, when the
denominator is zero.

- **Physical records** count accepted candidates by stable package family ID.
- **Mapped dataset-class records** assign an accepted occurrence to the supported class that owns
  the largest number of its defining faces. A tie is `ambiguous`; no matching defining face is
  `unmapped`. This is a comparison projection and does not rename the package record.
- **Defining-face recall** is supported-class labelled faces claimed by a mapped accepted family
  divided by all faces carrying that class label.
- **Face coverage** is class-labelled faces present in the constituent evidence of any accepted
  physical candidate, regardless of which family owns that candidate, divided by all faces
  carrying the class label. Constituent evidence is exact physical membership retained by the
  accepted recogniser proof; it is not inferred from labels or adjacency and does not transfer
  ownership to the labelled class. Unmigrated families default exactly to defining evidence.
  Coverage must be read beside defining-face precision and recall: inventing broad membership
  would maximize coverage without producing truthful recognition.
- **Defining-face precision** uses the same matched faces over every defining face claimed by a
  family mapped to that class. A family mapped to several corpus classes therefore has a separate
  one-vs-class denominator for each; it is not a composite accuracy score.
- **Instance recall** is supported truth instances touched by at least one defining face of a
  mapped accepted candidate divided by supported truth instances. It measures detection, not exact
  instance localization. MFCAD++ has no instance relation, so its denominator is zero; MFInstSeg
  supplies the relation.
- **Taxonomy mismatch** counts defining-face occurrences whose supported label does not map to the
  accepted candidate family. Incomparable and unsupported labels do not become false positives.
- **Empty models**, reconciliation drops, bounded unsupported diagnostics, predicate observations,
  and runtime remain separate fields. They are not folded into accuracy.

MFCAD++ and MFInstSeg assign one semantic label per face. Shared and stock-contact faces can make a
correct multi-face occurrence look impure. The report preserves that mismatch instead of letting
the dataset overrule the reconciler.

Defining-face recall has a structural ceiling below 1.0 for classes whose annotations include
faces that the package deliberately does not consume as defining evidence. For example, opposed-
wall recess recognition defines an occurrence by its walls rather than claiming its floor, and
the class-11 O-ring audit in #360 found labelled geometry truthfully owned by Fillet. Face coverage
exposes exact accepted physical membership and accepted cross-family evidence in this gap; it does
not replace defining-face recall or erase genuinely untouched faces.

## Dataset adapters

### MFCAD++ development evidence

The adapter reads the published integer class from each STEP `ADVANCED_FACE` name. It then requires
the imported face count to match exactly. Selection is unique model ID in lexical order. The local
development archive used for E0 is expected at:

```text
/app/workspaces-codex/datasets/mfcadpp/MFCAD++_dataset/step/test
```

Run the deterministic first 500 test models:

```bash
uv run python tools/run_effectiveness_baseline.py \
  mfcadpp /app/workspaces-codex/datasets/mfcadpp/MFCAD++_dataset/step/test \
  --dataset-version published-test-split \
  --limit 500 \
  --workers 4 \
  --checkpoint-dir .cache/effectiveness/mfcadpp-500-current \
  --output docs/benchmarks/effectiveness-mfcadpp-500-0.5.0.json
```

Use this 500-model command on both the parent commit and the proposed commit during development,
with a different checkpoint directory and output filename for each commit. Compare their exact
score-vector fields; runtime fields are measurements and are expected to differ. Reserve the full
2,500 selection for a merge candidate:

```bash
uv run python tools/run_effectiveness_baseline.py \
  mfcadpp /app/workspaces-codex/datasets/mfcadpp/MFCAD++_dataset/step/test \
  --dataset-version published-test-split \
  --limit 2500 \
  --allow-invalid \
  --workers 0 \
  --checkpoint-dir .cache/effectiveness/mfcadpp-2500-COMMIT \
  --output docs/benchmarks/effectiveness-mfcadpp-2500-COMMIT.json
```

`--workers 0` uses every available CPU; the default is capped at four to avoid surprising memory
pressure. Workers use fresh processes because forking an initialized OCCT process is unsafe.
Completed model rows are written atomically and merged back into lexical model order, so worker
completion order never changes the report. Progress is emitted on stderr and does not enter the
canonical JSON.

A checkpoint is reusable only for the exact captured commit, importable-source digest, taxonomy,
dataset/version, lexical selection and limit, resolved input paths and hashes, recognition frame,
and invalid-model policy. Any mismatch, foreign row, malformed row, or missing authority manifest
is refused rather than ignored. Deleting the explicitly named checkpoint directory starts a fresh
run. The known MFCAD++-2,500 selection is refused before recognition unless `--allow-invalid` is
present, because its seven invalid IDs and policy are recorded below.

The runner imports production recognition code from the environment that launches it. When an
existing virtual environment is reused to replay another worktree, put that worktree's `src`
directory first on `PYTHONPATH` and verify `b123d_recognisers.__file__` before the long run. A
detached worktree alone does not override an editable install from another checkout. The runner's
captured source authority will refuse publication drift during a run, but it cannot infer that the
operator intended a different checkout from the one Python actually imported.

The frozen result and interpretation are in the
[`0.5.0 MFCAD++ baseline`](effectiveness-mfcadpp-500-0.5.0.md).

MFCAD++ is open development evidence. Models and labels may be inspected to diagnose omissions.
The report remains a false-negative detector and regression baseline, not an independent transfer
estimate.

### MFInstSeg transfer evidence

The adapter expects the original published layout:

```text
DATASET_ROOT/steps/MODEL_ID.step
DATASET_ROOT/labels/MODEL_ID.json
PARTITION_ROOT/{train,val,test}.txt
```

It validates exact face keys, semantic/bottom lengths, binary bottom labels, a symmetric square
instance matrix, disjoint equivalence classes, within-instance semantic consistency, duplicate
test rows, and IDs appearing in more than one split. Duplicate or cross-split IDs are disclosed
and excluded before lexical selection.

MFInstSeg is distributed through authenticated sources and is not vendored. Do not replace it with
MFCAD++ or generated fixtures. With the original files and upstream AAGNet partitions mounted, run
the complete published test selection:

```bash
uv run python tools/run_effectiveness_baseline.py \
  mfinstseg /absolute/path/to/MFInstSeg \
  --partition-root /absolute/path/to/AAGNet/MFInstseg_partition \
  --dataset-version published-original \
  --taxonomy docs/benchmarks/effectiveness-taxonomy-v13.json \
  --canonical \
  --workers 0 \
  --checkpoint-dir .cache/effectiveness/mfinstseg-9373-COMMIT \
  --output docs/benchmarks/effectiveness-mfinstseg-9373-COMMIT.json
```

Do not inspect individual MFInstSeg model geometry during baseline creation. If later work inspects
one, record its ID and affected class; that class is no longer described as independent transfer
evidence for the milestone.

The first canonical full-test report and its interpretation are the
[`f2cfe4f MFInstSeg baseline`](effectiveness-mfinstseg-9373-f2cfe4f.md). It records all five
cross-split exclusions before selection and evaluates all 9,373 remaining test IDs without an
invalid row.

## Failure and immutability policy

Missing roots, malformed labels, unknown classes, face-count mismatch, import failure, non-finite
runtime, or a malformed taxonomy fail closed. By default, any invalid selected model prevents an
output file. `--allow-invalid` exists only when a written benchmark policy names the expected
invalid cases; the report then preserves each invalid model and reason.

A long corpus run also freezes its source authority before importing the first model. By default,
the runner permits exploratory runs with tracked changes and records the package commit as
`<HEAD>+dirty.<tracked-diff-sha256>`. Pass `--canonical` for publishable evidence; that mode
requires tracked files to equal `HEAD` and records the unchanged commit hash. Both modes capture a
digest of importable Python source and the exact taxonomy bytes/hash before importing production
recognisers. Capture and verification sandwich the commit, tracked-diff fingerprint, source, and
taxonomy between repeated checks; production imports receive an immediate verification before
scoring, and the same verification runs before publication. A mid-run checkout, rebase, or tracked
edit therefore refuses the report instead of attaching post-run provenance to pre-run in-memory
code or mapping. Untracked output files remain compatible with atomic report creation. Resumable
checkpoints add the same authority plus the selection, input-source, frame and
invalid-policy hashes; completed rows are never reused under a merely similar run. Issue #405
identified this as a latent provenance race during a cross-version report
comparison. A recorded commit and taxonomy hash are necessary but not sufficient to establish that
the in-memory recogniser stayed at that authority throughout the run. The historical #404 artifact
has internally consistent taxonomy-v8 metadata, but a source-exact replay does not reproduce its
inventory or scores. It is retained with an explicit warning; a new source-pinned
parent/implementation pair is the comparison authority.

The face-to-label adapter boundary has a separate cross-process guard: an asymmetric STEP fixture
assigns distinct `ADVANCED_FACE` labels, imports it in independent Python processes, and requires
the same label/area/centroid/normal pairing. The source-exact replay and a fresh current-source run
produced exactly equal model evidence after removing only runtime and commit metadata. Together
these guard stable imported face pairing independently of the preventative source-authority check.
Score differences between taxonomy revisions are expected and must never be described as
same-authority non-reproducibility.

Effectiveness report format version 2 added per-class `covered_faces` evidence and the derived
`face_coverage` ratio over defining evidence. Version 3 changes that numerator to exact accepted
constituent evidence after #368; defining-face fields and their semantics are unchanged. Historical
reports remain immutable rather than being rewritten.
Reports are canonical JSON with sorted model IDs, exact package commit/version, runtime
environment, selection hash, taxonomy hash, per-model source hash, exact counts, and runtime
distribution. Never rewrite a historical report after changing scorer logic. Produce a new report
and explain which denominator changed.

## Separate non-corpus arms

The corpus report does not absorb the package's other evidence:

- semantic goldens: `uv run pytest -q tests/test_golden_parity.py`;
- real parts: `uv run pytest -q tests/test_nist_ctc_corpus.py tests/test_turned_real_part.py`;
- performance: the two commands in
  [`recognition-budget.md`](recognition-budget.md#running-it), compared on the same machine.

Keeping these arms separate prevents synthetic label agreement from masquerading as downstream or
real-part effectiveness.
