# Quiddity CLI and recognition document

Available from Quiddity 0.2.1:

```bash
quiddity part.step
quiddity part.step -o recognition.json
quiddity --capabilities -o capabilities.json
quiddity --version
```

No `recognise` verb is needed. A STEP file or `--capabilities` is required, not both.
JSON goes to stdout unless `-o` is given. Diagnostics, including native reader messages,
go to stderr. Existing output files are replaced only after successful processing and
serialization. Output cannot be the input STEP file. The old `quiddity-capabilities`
command remains unchanged, including its `--format-version` option.

Exit status: 0 means processing succeeded (even if no features were recognised); 1 means
input, recognition, serialization or output failure; 2 means invalid command usage.
Frame/evidence refusal is a processing failure, never an implicit fallback to raw coordinates.

## Shared Python document builder

```python
from quiddity import import_step_geometry
from quiddity.document import build_recognition_document

document = build_recognition_document(import_step_geometry("part.step"))
```

The CLI is a transport adapter over this builder, not a second recogniser. It runs the ordinary
framed evidence lifecycle once, with the Python default `rotational=False`. It does not infer
application classification. Python callers can explicitly supply `rotational=True`.
STEP loading is geometry-only; assembly metadata is not retained.

The envelope is `format: quiddity-recognition`, `format_version: 1`. Existing feature `to_dict()`
records are embedded unchanged, not translated to a new geometry schema. It contains:

- `package`: distribution name and installed version.
- `coordinate_space: local`, `frame`: caller-space origin, orthonormal x/y/z directions and
  gauge. Map local points back as `origin + x*u + y*v + z*w` (ADR 0011).
- `bodies`, `faces`: rosters in the exact local working shape enumeration. A face carries
  `index`, `caller_index` in the input shape's face roster, and all matching `body_indices`.
  Multiple owners are not silently collapsed; no owner is represented by an empty list.
- `features`: accepted physical evidence in provider order, including bounded geometry refusals.
  Each entry has an envelope `index`, `family`, `record_type`, unchanged `record`, and sorted
  `defining_faces` / `constituent_faces` indices into `faces`.
- `derived`: existing hole, slot, oriented-slot and section-recess patterns, plus turned profiles.
- `association`: face-count and surface-area totals, associated/unassociated partitions, ratios,
  per-family union contributions and the `unassociated_faces` list.

Envelope feature indices are distinct from indices embedded in SectionRecess records.
SectionRecess pattern references target the embedded recess occurrence indices, **not** the
all-family envelope feature indices. Refusals retain their explicit `SectionRecessRefusal`
record type and reason; they are evidence, not reconstructible recess geometry (ADR 0019).

References are document-local: they are not STEP entity numbers, random identifiers or durable
IDs across imports, transformations or versions. The existing recess-only JSON builder remains
unchanged. This shared envelope adds transport around existing public records and evidence.

Association is **not accuracy or recall**, nor a percentage of file bytes or material volume.
It counts accepted constituent evidence. Ratios are null for zero denominators; per-family
contributions can overlap and must not be summed. Unassociated faces can include intentional
stock/background, not just missed features. Defining evidence and constituent evidence remain
separate. No holdout corpus, labels or taxonomy mapping participates in CLI recognition.
