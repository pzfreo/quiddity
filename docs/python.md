# Python quiddity 0.4.0

`quiddity` is now an abi3 wheel built from Rust with PyO3 and maturin. It supports
CPython 3.11 and newer, has no Python runtime dependencies, and requires neither
build123d nor OCP. The Rust binding crate is `crates/quiddity-py` (`publish = false`
for crates.io); its Python distribution and import name are both `quiddity`.

```python
import json
import quiddity

text = quiddity.recognise("part.step")
result = json.loads(text)
document = result["document"]  # quiddity-rust/recognition/6
for feature in document["records"]:
    print(feature["family"], feature["parameters"], feature["faces"])

with open("part.step", "rb") as stream:
    evidence = json.loads(quiddity.evidence(stream.read()))
```

| API | Result (JSON string) |
| --- | --- |
| `recognise(step: str \| bytes)` | Exactly the Rust CLI recognition envelope: family arrays, `fingerprints`, and the `/6` `document`, including stdout's trailing newline |
| `evidence(step, framed=False)` | [Evidence/1](evidence.md): family, record, defining/constituent faces, instance groups, hosts, pattern members and rejected candidates |
| `anchor(step, face: int)` | `{face, point: [x,y,z], uv: [u,v]}` from haecceity's proved trimmed-face anchor, in file coordinates |
| `fingerprints(step)` | The existing fingerprints/3 document |
| `correspond(old_step, new_step)` | The existing Rust revision correspondence document |
| `capabilities()` | Versioned supported formats, inputs, operations and PMI import availability |
| `__version__` | the package version, e.g. `"0.4.8"` (a Python string) |

Every geometry argument accepts a string path or STEP bytes. A string is always a
path, never inline STEP text. Path inputs also accept the existing `.step.gz` /
`.stp.gz` reader. Byte inputs are uncompressed STEP. File and byte recognition of
the same uncompressed input have identical source digests and JSON. Geometry
work releases the GIL. Rust input/refusal messages become `ValueError`; wrong input
types raise `TypeError`, and negative face indices raise `OverflowError`.

## XCAF/other producers → AP242 STEP

`quiddity.pmi` calls haecceity's `json-import` API directly in process. Geometry
accepts STEP bytes, a path string, or `os.PathLike` (paths may be gzipped). JSON
input accepts a dict or JSON string, retaining stated decimal strings and IDs.

```python
from pathlib import Path
from quiddity import pmi, PmiError

# The producer (for example xcaf-pmi) supplies these two in-memory values:
# geometry_step: bytes containing its STEP geometry, with the same face names
# semantic: dict in haecceity.pmi/1, with producer IDs and name/index face anchors
pmi.validate(semantic)
try:
    ap242_step = pmi.write(geometry_step, semantic)  # bytes; no temporary files
except PmiError as error:
    print(str(error), error.findings)
    raise

# Save only if the caller wants a file (or write bytes to a CLI's stdout.buffer).
Path("annotated.step").write_bytes(ap242_step)
assert pmi.read(ap242_step)["schema"] == "haecceity.pmi/1"
drawing = pmi.presentation(ap242_step)  # haecceity.presentation/1 dict
```

| API | Result |
| --- | --- |
| `pmi.write(geometry, document, *, part=None, report=False)` | AP242 STEP **bytes**, with generated callouts enabled |
| `pmi.write(geometry, document, *, part=None, report=True)` | `(bytes, report_dict)` with verification and warnings |
| `pmi.read(step, *, part=None, anchors="index")` | PMI **dict**, with face indices by default |
| `pmi.read(step, *, part=None, anchors="name")` | PMI dict with STEP face names; missing or ambiguous referenced names are refused |
| `pmi.presentation(step, *, part=None)` | File-owned/generated presentation dict |
| `pmi.validate(document)` | None on success; schema, references and the same geometry-independent authoring checks as import |
| `pmi.SCHEMA`, `pmi.PRESENTATION_SCHEMA` | Packaged JSON Schema dicts |

To keep warnings from a successful import, request the optional report:

```python
ap242_step, report = pmi.write(geometry_step, semantic, report=True)
assert report["differences"] == []
print(report["added"], report["read_findings"], report["writer_findings"])
```

The default return remains bytes. The report retains the JSON-import CLI's
`schema`, `part`, `added`, `differences` and `presentation_removed` fields; there
is no output filename for an in-memory operation. `read_findings` describes the
input snapshot, including repaired OCCT density measures; `output_findings`
describes the verified result. Each reader finding has `kind`, `part`, `ids`,
`entity` and `detail`. Refusals still raise `PmiError`, with either report setting.

`writer` is the full canonical Rust write report, including instance counts,
source/output bindings, schema upgrades, repairs, kept/unconsumed PMI, and
preexisting schema/rule or practice-conflict diagnostics. `writer_findings`
flattens its warning-bearing collections, retaining every original field and
adding `category` to identify the source collection (for example
`preexisting_violations`, `repairs`, or `kept_unconsumed`). This keeps input reader
repairs distinct from writer diagnostics and from findings in the output.

The optional canonical `writer["preserved_normalizations"]` collection is an array
of objects with exactly `id` (original STEP entity id, integer) and `normalization`
(string). For example, `{"id":1004,"normalization":"legacy-endpoint-retirement"}`
becomes `{"category":"preserved_normalizations","id":1004,"normalization":"legacy-endpoint-retirement"}`
in `writer_findings`. The names are `legacy-endpoint-retirement`,
`functional-sar-isolation`, `functional-sar-unwrapping`, `kept-usage-factoring`
and `legacy-text-rechunking`. The canonical key is omitted when empty, and produces
no findings. These diagnostics concern Add preservation; Python's current public
write API uses Replace, so it normally has none. The findings projection preserves
the shape and names of canonical Add reports without introducing a Python Add API.

The schemas are also installed as `quiddity/schema/haecceity.pmi-1.json` and
`quiddity/schema/haecceity.presentation-1.json`, byte-identical to haecceity's.
Other producers can load these files with their own JSON Schema validator.
Geometry-independent validation treats face anchors as opaque; existence and
uniqueness are checked against the actual STEP during `write`.

`write` uses lossless STEP editing and the same `write_preserving` importer as
`quiddity pmi json import`. It verifies model fields, IDs and units by reading
back before returning any bytes. Existing foreign graphics on unchanged surviving
requirements are retained under that import policy; the geometry is not rebuilt.
Multi-part files require `part=`. Face indices refer to the supplied part, and
name anchors must identify exactly one face in that geometry snapshot.

Refusals raise `quiddity.PmiError`, a `ValueError` subclass carrying haecceity's
reason text. Its `.findings` list retains available input-reader diagnostics as
`{kind, part, ids, entity, detail}` dicts; it is empty when no structured findings
are available (including schema/JSON parse failures). Reading does not promise
that every source item is writable: dimensions without nominal values, for
example, remain explicit authoring refusals. Nothing is silently dropped.

The provisional path/output/report PMI helpers from the first wheel draft were
replaced; they had no consumer in this repository beyond its tests and docs.
Recognition and the other wheel APIs retain their existing signatures and output.

`quiddity ...` and `python -m quiddity ...` execute the Rust CLI dispatcher,
including `serve`, `parts`, `correspond`, `brep`, `pmi` and `pmi json` commands.
New commands include `evidence <step> [--framed]`, `fingerprints <step>` and
`capabilities`. Run `quiddity --help` for the current syntax.

## Breaking changes from Python quiddity 0.3.11

- Live build123d/OCP inputs, per-family `recognise_*` functions, and exported
  dataclasses are removed. Export to STEP and call the file-based API.
- Results are JSON strings (decode with `json.loads`), replacing dataclasses,
  `RecognitionResult`, and format-3 dictionaries. Recognition/6 is inside the
  CLI envelope's `document` field.
- Recognition coordinates are in file space, rather than a local working frame.
  Axes can be unit vectors as well as principal-axis names. A record's own
  origin/basis fields define its measurements. The top-level frame is provenance.
  Explicitly framed evidence is the exception described in [evidence/1](evidence.md).
- Recognition/6 merges oriented families into plain ones (including slots,
  chamfers and slot patterns), publishes hole patterns as records, and adds
  families such as modelled threads and turned bodies. `derived`, `association`
  and `named_dimensions` are removed. See [recognition/6](recognition-6.md).
- Fingerprints/3 and `correspond` replace correspondence receipts and their
  run-local issuance/resolution API. See [correspondence](correspondence.md).
- Evidence and anchors identify faces by indices, replacing Face objects.
  OCP import, inspection and mapping helpers are removed.
- The CLI is the Rust CLI, with different subcommands and JSON output.
- Python 3.11 is the minimum supported version.

## Migrating build123d-mcp

Export the in-session shape once to STEP bytes (or a temporary STEP path), retain
those bytes for the run, then call `recognise` for inventory and `evidence` for
edit handles. `evidence(..., framed=False)` corresponds to the old caller-space
evidence; `framed=True` corresponds to part-relative evidence. Evidence preserves
`bottom`, `cbore`, `spotface`, `csink`, `diameter`, `axis`, `location`, and `depth`
in hole records. Preserve null stack fields and handle axis vectors.

Build targets from `evidence["features"]`: retain `family`, `index`, `record`,
`defining_faces` and `constituent_faces`, and resolve indices through a verified
STEP-to-live-face mapping. Keep pattern aggregates read-only as before. Replace
the old association report with the recognition document's `face_accounting`;
feature, body, refused and unexplained are distinct groups. Do not treat
unproved/refusal records as permission to edit. Retain the consumer's existing
valid-solid, plain-through-hole and stale-source checks, then re-export and
re-recognise after each edit. Use `correspond` for revision matching.

The face-order trial and reproducible procedure are in
[the trial report](python-face-order.md). Do not assume an exporter version's
observed ordering is a durable contract. The trial verified a complete geometric bijection using unique anchor membership,
area and centroid agreement. Apply those checks to each export before issuing
edit handles. Names can also be joined through `face_sources[face].entity` when
the writer supplies a complete name map, but the fillet-heavy trial retained
only 23 of 27 names. Never fill missing names by assumption or resolve an
ambiguous geometric match by nearest centroid.

## Migrating b123d-decompiler (document 3 → recognition/6)

| Python format-3 field | New field or action |
| --- | --- |
| `frame` | `result["document"]["frame"]`; provenance only; records already in file space |
| `features` | `result["document"]["records"]` |
| `features[].index` | Parse the family-local suffix of `records[].id` (`family/index`) |
| `features[].family` | `records[].family` (apply oriented-family merge migration) |
| `features[].record` | `records[].parameters` |
| `features[].constituent_faces` | Use the matching evidence feature after verifying family/record/defining faces; `records[].faces` are defining faces only |
| `derived.turned_profiles` | No direct field. Use `turned_bodies`, `turned_steps`, `cylindrical_profiles` and groove records to rebuild the consumer's machining profile |
| `association` | `document.face_accounting` (one group per face) and evidence faces; the old association graph has no direct replacement |
| `named_dimensions` / `dependents` | Reconstruct in the consumer if needed |

Remove the old `import_step_geometry` and `build_recognition_document` imports.
The consumer can import STEP itself for reconstruction, but must verify that
its imported face map matches the exact file the recogniser read. Stop applying
`to_local` to record coordinates. If the emitter requires a local working part,
transform both the part and every relevant record using the provenance frame,
including points, direction vectors and record-owned bases. Framed evidence can
help with legacy reconstruction, but its family registry differs from `/6`.

## Development and release gate

```sh
maturin build --release -m crates/quiddity-py/Cargo.toml
python -m pip install target/wheels/quiddity-*.whl
python -m pytest tests/python -q
# Optional: export QUIDDITY_CORPUS=/path/to/tests/corpus
```

`.github/workflows/python.yml` builds abi3 wheels for Linux x86_64/aarch64,
macOS arm64/x86_64 and Windows x64, plus an sdist. Each wheel runs the smoke
suite on CPython 3.11; Linux x86_64 also tests a newer CPython. A separate Linux
job installs the sdist and runs the same smoke suite. The corpus test
skips without `QUIDDITY_CORPUS` and fails if required corpus inputs are missing.
**A published GitHub release publishes to PyPI.** After all builds and tests pass, the
release's wheels and sdist go to PyPI through the `pypi` environment (main and `v*` tags only)
and PyPI trusted publishing. This was the maintainer's decision: a GitHub release is the
approval. The job refuses to publish unless the release tag, the wheel's version
(`crates/quiddity-py/pyproject.toml`) and the crates' version (`Cargo.toml`) all agree, so
crates.io and PyPI releases share one version from 0.4.0 on. A manual run on `main` with
`publish=true` is a fallback. Pushes and pull requests never publish.

### Helical springs

Recognition JSON includes the optional `helical_springs` array and `HelicalSpring`
document records, using the same schema-6 file-space measurements as the CLI.
See [the spring record contract](recognition-6.md#helical-coil-springs). Read
`active_coils` as nullable and preserve `active_coils_basis`; unstated spring rate
is not inferred. There is no separate Python record-class registration.
