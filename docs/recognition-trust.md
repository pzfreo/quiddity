# Recognition trust contract

The CLI (`quiddity part.step`) and `serve` (`recognise`) publish the same recognition
`document`. Trust metadata is additive to documents 3 and 4: the tag continues to
select record families, and existing record parameters, IDs, faces, ordering and
fingerprints keep their previous meaning. Readers should ignore unknown fields.

- `source_sha256` is a lowercase SHA256 of the exact input file bytes, including the
  gzip wrapper for `.gz`. The input is read once; hashing, initial parsing and any
  frame reread use that snapshot. It is `null` for geometry recognised in memory.
- `run_mode` is `strict` when the first inventory completes, or `locally-degraded`
  when the local-degradation retry supplies the inventory. It describes the
  completed run, independently of coordinate frame. Strict describes the discovery
  mode, not a claim that every published record is verified.
- `input_repairs`, when present, lists kernel repairs by zero-based solid index,
  also emitted as CLI and `serve` warnings. `solid N: inverted closed shell repaired`
  means a closed boundary with negative signed volume had inward material-side
  witnesses on every face; the kernel reversed its face senses and, where needed,
  coherently inward boundary wires before recognition.
  `solid N: mixed face senses repaired (M faces)` means only the M proved inward
  normals changed; already outward normals and all boundary wires were retained.
  Every valid solid's signed mass is checked before a material-side witness can
  skip repair, so a negative mixed shell is treated independently of face order.
  Every face must have three regular material-side witnesses at two offsets;
  the candidate must have coherent outward wires, valid closed topology and
  positive mass, and every candidate face must be re-proved outward with three
  witnesses at two offsets before any mutation. Insufficient evidence or mixed
  wires retain the original senses and signed volume. Ray-parity classification
  remains independent of those senses.
  Missing `input_repairs` means no repair was reported, including in older documents.
- Every `records[]` entry carries `evidence_status`: `verified` or `unproved`.
  Verified means the kernel's existing strict evidence ownership check succeeded:
  the record has defining faces and all defining and consulted faces belong to
  one geometrically valid solid. Geometric validity means the checks this kernel
  implements (resolved geometry, closed orientable topology and no faulted face
  loops); it is not an independent OpenCascade certificate, recognition confidence
  score, dimensional uncertainty or a claim about design intent. Family discovery
  and reconciliation still decide which candidates publish.

Local degradation can admit geometry that fails strict validity. Its records stay
`unproved` unless the strict check succeeds; evidence away from a bad face does
not certify the whole solid. Patterns inherit all their members' statuses,
including consulted faces absent from their fingerprints. Section recesses check
both their defining and constituent faces. Source refusals are always `unproved`:
they explain a failed claim and do not become verified features merely because
their source faces have a valid owner. Consumers must distinguish the
`SectionRecessRefusal`, `ModelledThreadRefusal` and `InteriorVoidRefusal` record types
(or use Rust's `Record::is_refusal()`) before applying an evidence-status policy. These
records explain why a source candidate could not publish unified recess or thread geometry,
or why a bounded-air candidate was not published as an interior void; they are not feature
claims whose evidence failed. A refusal alone does not imply
invalid geometry or invalidate verified features on the same part. Consumers that
require complete projection coverage may separately reject its recorded refusal
reason.

Warnings describe unresolved geometry, missing or invalid solids, a degraded retry,
and retained unproved feature record IDs. Source refusal records remain published
with their reasons and are excluded from this warning's unproved list and fallback.
A clean part with only source refusals emits no feature-evidence warning. A refusal
of the whole section-recess projection remains a separate, explicit warning. An unresolved face does not imply every feature
of every other solid was discarded. An open shell may retain geometric candidates;
its records are unproved because there is no valid solid authority. The default
continues to retain candidates. Consumers requiring strict refusal should opt in
by checking the feature records in the document (and treating absent trust fields
as unproved), before using geometry. Handle source refusal record types separately
under the coverage policy above. This additive contract introduces no new CLI refusal option.

The Rust document reader accepts older documents: missing digest becomes `null`,
missing mode becomes `unknown`, and missing per-record status becomes `unproved`.
New recognitions always supply `strict` or `locally-degraded`. A header-only or
empty STEP, a truncated entity, or a missing referenced `ADVANCED_FACE` is refused
by the existing kernel, without a recognition document. CLI exits 1 with the
kernel diagnostic; `serve` returns structured error code `broken_step` and keeps
serving subsequent requests.
