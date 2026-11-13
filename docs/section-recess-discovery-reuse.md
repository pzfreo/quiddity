# Reusing native section-recess discovery

Reconciliation and final publication call `section_recess_projection_in` with the
same `Context`, but different accepted inventories. Previously both calls repeated
`discover_section_recesses`, including the exact swept-volume proofs of native
floors. Those proofs read the immutable part and the ownership mode; they do not
read the inventory or reconciliation dispositions.

`Context` now retains the complete `Result<Vec<SectionRecess>, SectionRecessError>`
from native discovery. The slots are separate for strict and locally degraded
ownership because `common_valid_solid` can answer differently in those modes.
Each context borrows one part, so a transformed part or a new recognition run has
fresh slots. No records or face references cross that boundary.

The cache initializes by calling the existing discovery in its existing order.
There is no change to geometry probes, tolerances, publication rounding, record
numbering or missing-proof handling. Successful records are cloned for each
projection; discovery errors propagate with the same message. `OnceLock` stores
only a completed result, so a panic does not publish or retain a partial vector.

The inventory-dependent `project` function still runs on every call. It recomputes
accepted source families, duplicate-region handling, publication refusals and
patterns from that caller's current inventory. The surrounding family isolation
guard and failure ledger are unchanged. Caching the complete projection would be
incorrect because reconciliation can change that inventory.

Targeted tests compare uncached and cached native records, including serialized
bytes and positive pocket geometry. They check repeated reads, separate contexts
and both ownership modes. A second check changes accepted pocket inputs after the
first projection and requires the new patterns to match a fresh context.

These checks do not replace strict snapshots or whole-file acceptance. Performance
observations must identify the source, input and host load; a cache hit removes a
duplicate discovery, but does not establish a fixed whole-recognition speedup.

The source-refusal classification introduced by the ownership fix stays in the
inventory-dependent projection. Open-shell, owned-solid and mixed controls are
included in the exact discovery comparison; the existing ownership and projection
failure regressions run separately on this revision.
