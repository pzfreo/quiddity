# Evidence document, version 1

`quiddity evidence part.step` and Python `quiddity.evidence(step)` emit
`quiddity-rust/evidence/1`. This is the owned JSON form of the root crate's
`RecognitionEvidence`, available to Rust through `evidence_view::document(&part, framed)`.
It is additive: recognition/6 and fingerprints/3 are unchanged.

The top-level fields are:

| Field | Meaning |
| --- | --- |
| `schema_version` | `quiddity-rust/evidence/1` |
| `coordinate_space` | `file` by default; `part` with `--framed` / `framed=True` |
| `frame` | null by default; inferred origin, x/y/z axes and gauge in file coordinates when framed |
| `face_count` | Number of input faces; all face indices are zero based |
| `face_sources` | Array indexed by face: `{entity, instance}` identifies the original STEP `#entity` and its placed instance, or null for geometry without source identity |
| `edge_sources` | Array indexed by edge (haecceity's edge numbering, as PMI edge anchors and `quiddity parts` count them): `{entity, instance}` identifies the original STEP `EDGE_CURVE` `#entity` and its placed instance, or null |
| `features` | Accepted evidence features and section-recess refusal records |
| `candidates` | Rejected candidates and the transitive closure of their reconciliation links |
| `rejected_candidates` | Positions in `candidates` |
| `failures` | Optional failed/blocked family entries, including returned section-projection errors |
| `complete` | `false` only when `failures` is present; otherwise omitted |

Each feature has `index` (within its family), `family`, `record` (the Rust
record serialized as JSON), `defining_faces`, `constituent_faces`,
`instance_faces` (ordered groups of constituent faces), `host_faces`, and
`members` (positions in `features`, for hole-pattern members). Face lists
are sorted and unique. Empty instance/host/member lists mean no such evidence
is available. A face is an integer, never an OCP or build123d object.

Each candidate has `family`, family-local `index`, `outcome`, optional `reason`,
`defining`, `constituent`, and `related` (positions in `candidates`). A null
reason denotes default acceptance. These candidate field names follow the
existing Rust evidence view.

The evidence view retains its own physical-family registry and section-recess
aggregation: recess source families are represented by `section_recesses`,
`passages` names section passages, and oriented families retain their names.
It is not a one-to-one enumeration of recognition/6 records. Evidence IDs and
order belong to this run; never join the two documents by array position or
assume the same family-local index after oriented-family merging.

Default evidence recognises directly in file coordinates, as the old caller-space
evidence API did. Framed evidence transforms the loaded geometry into an inferred
part frame before recognition; records then use that local frame. Both preserve
exactly the input face numbering. Framed requests refuse when the frame cannot
be inferred. This differs from recognition/6, whose records always use file
coordinates and whose frame is provenance only.

For edit operations, retain the exact exported bytes and a verified mapping
from `face_sources` (and `edge_sources`) to the live shape's faces (and edges). Check every index and invalidate
all handles when the source changes. STEP entity IDs are scoped to one export;
`instance` disambiguates repeated geometry. An entity ID alone is not a durable
revision identity. Use fingerprints and correspondence between revisions.

## Incomplete evidence (For #161)

Evidence discovery, accepted-member pattern derivation and section-recess projection use
always-on family isolation. The Python wheel returns this same document. For example,
a failed pocket discovery adds `"failures": [{"family": "pockets", "stage": "discover",
"message": "example panic"}, ...]` and `"complete": false`; dependent patterns and the
section-recess projection have `blocked_by` entries instead of a message. Entries are
sorted by family/stage and blocker lists alphabetically. Names identify the internal
recognition family (`section_passages`, even though evidence records call it `passages`).
Treat absent records in those families as unevaluated. Failed families contribute no
candidates or face claims. Both fields are omitted on successful runs, preserving bytes.

The dependency, payload, retryable-cache and panic-hook policies are described in
[recognition/6](recognition-6.md#incomplete-recognition-for-161). STEP reading, context
construction, reconciliation, evidence document assembly and validation remain fatal.
WASM builds abort on panic, so this has no effect in the browser.

## Recess sources without body authority

A STEP file may contain both solids and open surface shells. A prismatic candidate
on such a shell can be retained as `unproved` in recognition/6, but cannot authorize
a material section recess. All eight recess source families check the union of their
defining and consulted faces before projection. A candidate without a common valid
solid produces a `section_recess_source_refusals` record. This intentionally also
refuses a solid-owned candidate that consults a face on another solid, or a face
excluded by local degradation, even when its defining faces alone have valid ownership:

```json
{"reason": "missing_common_valid_solid", "faces": [10, 11, 12, 13]}
```

The refusal has no invented body index. Its sorted face union is carried in both
evidence face lists and resolves through the unchanged `face_sources` roster.
Recognition publishes the same payload as `SectionRecessSourceRefusal`, with
`unproved` status; existing physical candidates keep their measurements and status.
The standalone section-recess document includes optional `source_refusals` when
nonempty. Independent recesses, including others from the same source family, remain
available. A completed evaluation with an explicit candidate refusal is not a failed
family and does not set `complete: false`.

If the section projection itself returns an error, both recognition and evidence
instead record `section_recesses / section_recess_projection` in `failures` and mark
the result incomplete, including `complete: false` in recognition/6. This is a
deliberate partial-result contract, not a successful empty evaluation. Independent
families survive. Input parsing, source identity
and document validation errors still fail the call. No sewing, solidification, input
repair or change to the recognition/6, evidence/1 or section-recess/4 schema is involved.
