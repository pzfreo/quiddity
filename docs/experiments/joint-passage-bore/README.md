# Deferred joint passage/bore feasibility

This archive is **not production recognition code**. See ADR0025 and #540.
Its source-only prototype reaches one model and eight previously uncovered
six-sided passage wall faces in the 2,500-input MFCAD++ development split.
Seven baseline-invalid models are explicitly skipped. This is an opportunity
audit, not a public-path effectiveness comparison or an instance-recall claim.

The narrow two-plane-roof/internal-bore contract is feasible but deferred for
low measured reach. It is not declared impossible. Revisit for concrete consumer
demand or a materially larger recurring opportunity, not merely because the
prototype exists. Broader joint-support combinations were not measured here.

Independent architecture review supported direct joint-boundary proof and the
existing base-support interpretation. Its internal-only opening and unrelated
coplanar-feature concerns were incorporated into the prototype and ADR. The
22-case authored suite passed, as did a subsequently added disconnected/coaxial
bore negative. Production proof/projector reviews, public tests and a full
before/after production comparison have **not** been completed.

## Reproduction

Use production commit `83989a5` (same recognition source as the pinned #553
corpus head `8c5c7d3`) with its development dependencies. Copy the three archived
`.py.txt` files to a scratch directory, removing only the `.txt` suffix. They
are stored as text to keep this deferred prototype out of normal test discovery
and production tooling. The source prototype is preserved byte-for-byte;
the archived scan wrapper accepts explicit baseline and dataset locations.

From the repository root, with `PYTHONPATH=src:/absolute/path/to/scratch`:

```bash
python -m pytest /absolute/path/to/scratch/test_source_joint_passage_bore.py -q --no-cov
python /absolute/path/to/scratch/scan_joint_passage_bore.py \
  --limit 2500 \
  --baseline /absolute/path/to/effectiveness-plane-envelope-8c5c7d3.json \
  --dataset /absolute/path/to/MFCAD++_dataset/step/test
```

Generate that baseline using the command in
`docs/benchmarks/e5-plane-envelope-passages-540.md`. Its report SHA256 is
`b179594d136d8dafb9771dd59464c44bb79eaa6e21ae6be196ae56fe535f6cf4`.
The archived opportunity JSON records the exact selected model IDs, candidate
walls and labels. Labels enter only after geometry-only discovery; baseline
coverage is queried only for candidates that were actually found.

Prototype SHA256:
`368ca7ae62968690f690bd25fdff934163674f97b03fff2e10427df9a3ef1391`.
No MFInstSeg data or feedback is used.
