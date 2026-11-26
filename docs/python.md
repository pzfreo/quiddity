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
| `recognise_with_evidence(step: str \| bytes)` | [Shared analysis](shared-analysis.md): complete recognition, evidence and an explicit evidence error from one shared analysis |
| `evidence(step, framed=False)` | [Evidence/1](evidence.md): family, record, defining/constituent faces, instance groups, hosts, pattern members and rejected candidates |
| `anchor(step, face: int)` | `{face, point: [x,y,z], uv: [u,v]}` from haecceity's proved trimmed-face anchor, in file coordinates |
| `anchors(step, faces=None)` | `anchor` for many faces from one parse: a JSON array of `{face, point, uv}` in the order asked (every face in index order when `faces` is omitted; each result equals the single `anchor` call's). A face with no provable point gets `point` and `uv` null and the reason in `refused`; an out-of-range index raises `ValueError` for the whole call. Use it instead of looping over `anchor`, which parses the file on every call |
| `fingerprints(step)` | The existing fingerprints/3 document |
| `correspond(old_step, new_step)` | The existing Rust revision correspondence document |
| `parts(step)` | Exactly `quiddity parts`: each distinct part's index, name, face and edge counts and placements, and the binding, including stdout's trailing newline |
| `solid_inertia(step, solid=0, point=None)` | Signed `volume` (mm³), `centroid` (mm), and symmetric `tensor` (mm⁵) for the zero-based solid, or JSON `null` if unavailable |
| `step_entities(step)` | Every DATA instance in file order, as compact JSON `[{id, types, refs}]`: `id` is the `#N`, `types` the upper-case entity name (each leaf of a complex instance, in written order) and `refs` the referenced `#N`s in attribute order, repeats kept |
| `capabilities()` | Versioned supported formats, inputs, operations and PMI import availability |
| `__version__` | the package version, e.g. `"0.5.5"` (a Python string) |

Every geometry argument accepts a string path or STEP bytes. A string is always a
path, never inline STEP text. Path inputs also accept the existing `.step.gz` /
`.stp.gz` reader. Byte inputs are uncompressed STEP. File and byte recognition of
the same uncompressed input have identical source digests and JSON. Geometry
work releases the GIL. Rust input/refusal messages become `ValueError`; wrong input
types raise `TypeError`, and negative face indices raise `OverflowError`.
Internal Rust panics become `quiddity.QuiddityError`, a `RuntimeError` subclass
caught by `except Exception`, with the panic message. This boundary covers
recognition, evidence, correspondence, manifests, PMI and the CLI.

Pass the keyword `k_factor=0.45` to `recognise`, `recognise_with_evidence` or
`evidence` to set the neutral-axis fraction for sheet-metal flat patterns.
The default (also used for `None`) is 0.5. Numeric values must be finite and in
(0, 1]. An out-of-range or non-finite value raises `ValueError`. Each development
reports the actual value in `flat_pattern.k_factor`.

Python `fingerprints` and `correspond` always read STEP inputs at K = 0.5 and
have no `k_factor` keyword. Compare revisions at one K. A change between
`checked` and `overlap` changes the `flat_pattern_status` fingerprint trait,
so correspondence can report the matched sheet as `adapted`. Where a development's
validity changes, K can also change flange adoption and the suppression of duplicate
slots, pockets and pads. For custom-K correspondence, recognise both revisions at
the same K and pass the two saved JSON results to `quiddity correspond`.

## Solid inertia

```python
centroidal = json.loads(quiddity.solid_inertia("part.step"))
about_origin = json.loads(quiddity.solid_inertia("part.step", point=(0, 0, 0)))
```

The tensor uses uniform density 1 and the STEP coordinate axes. Its diagonal
gives the moments about those axes, and off-diagonal entries are negated products
of inertia. The axes pass through the centroid by default. Pass a finite
three-coordinate point in mm to shift them by the parallel-axis theorem. The
reported centroid is always the centre of mass. Inward-facing void shells
subtract, and reversing all the shell orientations flips the sign of the tensor
and volume.

## Assembly manifests

`manifest(path, out=None)` returns a dict and writes only `out` when supplied.
`recognise_product(path, manifest, product_id)` returns a recognition dict;
`recognise_products(path, manifest, parts=None)` lazily yields `(id, dict)` in
manifest order, reading one product at a time and reusing one parsed Rust manifest. Paths accept `str` or `PathLike`;
the manifest argument accepts a dict or a JSON file path. These new functions
return dicts; the existing `recognise` continues to return JSON text. Scanning,
reading and recognition release the GIL. Unknown ids and refusals raise
`ValueError`; compressed product reads and shared geometry are refused.
Both product recognition functions also accept the `k_factor` keyword.

See [assembly manifests](assemblies.md) for format, staleness checks, placements,
limits and the CLI's opt-in whole-file fallback.

## build123d → AP242 PMI

A script that models a part in build123d can add semantic PMI to it directly: datums,
dimensions with tolerances, geometric tolerances and general tolerances, written as AP242
semantic PMI, which Draftwright's default `pmi first` mode reads as the authored intent. This plate
gets datum A on its bottom face, a 10 ± 0.2 mm thickness, 0.05 mm flatness on its top face
and ISO 2768-mK general tolerances (checked with quiddity 0.5.6 and build123d 0.13):

```python
from build123d import Axis, Box, export_step
from quiddity import pmi

# 1. Model the part and export plain STEP geometry.
plate = Box(80, 60, 10)
export_step(plate, "plate.step")

# 2. Find the faces to annotate. A face is anchored by its index in the file's face
#    order, which for a build123d export is plate.faces() order (check it; see below).
faces = list(plate.faces())
top = faces.index(plate.faces().sort_by(Axis.Z)[-1])
bottom = faces.index(plate.faces().sort_by(Axis.Z)[0])

def mm(value):
    return {"decimal_places": None, "quantity": {"length": {"unit": "unit/mm", "value": value}}}

# 3. Describe the PMI: datum A on the bottom, 10 ± 0.2 thickness, 0.05 flatness on top,
#    ISO 2768-mK general tolerances.
semantic = {
    "schema": "haecceity.pmi/1",
    "units": {"length": [{"id": "unit/mm", "value": "millimetre"}], "angle": [], "density": [], "other": []},
    "pmi": {
        "standards": [], "decimal_places": None,
        "features": [
            {"id": "bottom", "value": {"items": [{"face": bottom}]}},
            {"id": "top", "value": {"items": [{"face": top}]}},
        ],
        "datum_targets": [],
        "datums": [{"id": "datum/A", "value": {"feature": "bottom", "label": "A", "targets": []}}],
        "dimensions": [{"id": "thickness", "value": {
            "kind": {"location": {"kind": "linear distance", "from": "bottom", "to": "top",
                                  "directed": False, "angle": None, "path": None}},
            "modifiers": [], "principle": None, "qualifier": None,
            "nominal": mm("10"),
            "tolerance": {"deviations": {"lower": mm("-0.2"), "upper": mm("0.2")}}}}],
        "tolerances": [{"id": "top-flatness", "value": {
            "kind": "flatness_tolerance", "magnitude": mm("0.05"), "target": {"feature": "top"},
            "datums": None, "auxiliary": [], "description": None, "maximum": None,
            "modifiers": [], "unequal": None, "unit_basis": None, "zone": None}}],
        "tolerance_relations": [],
        "general": [{"id": "general", "value": {"class": {"text": "ISO 2768-mK",
                                                          "standard": {"linear": "m", "geometric": "K"}}}}],
        "threads": [], "knurls": [], "holes": [], "material": None, "notes": [],
        "surface_textures": [], "attributes": [], "geometry": [], "derived_geometry": [],
    },
}

# 4. Write AP242 with semantic PMI, and read it back.
pmi.validate(semantic)
step = pmi.write(open("plate.step", "rb").read(), semantic)
open("plate-pmi.step", "wb").write(step)
back = pmi.read(step)
assert [d["id"] for d in back["pmi"]["datums"]] == ["datum/A"]
```

Faces are anchored by index in the file's face order. For a build123d export that is
`plate.faces()` order, but quiddity does not control another exporter's order, so check the
mapping on each export as [the face-order trial](python-face-order.md) describes: for
example, confirm through `quiddity.evidence(...)["face_sources"][i]["entity"]` that each
anchored face is the plane you meant. A face can also be anchored by `{"name": ...}` when
the exporter writes face names into the STEP file; build123d 0.13 writes them empty.

The API is described in full in the next section.

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
| `pmi.read(step, *, part=None, findings=True)` | `(dict, findings)`: the same dict, and the reader's findings on that part or on no part |
| `pmi.presentation(step, *, part=None)` | File-owned/generated presentation dict |
| `pmi.validate(document)` | None on success; schema, references and the same geometry-independent authoring checks as import |
| `pmi.SCHEMA`, `pmi.PRESENTATION_SCHEMA` | Packaged JSON Schema dicts |

To keep warnings from a successful import, request the optional report:

```python
ap242_step, report = pmi.write(geometry_step, semantic, report=True)
assert report["differences"] == []
print(report["added"], report["read_findings"], report["writer_findings"])
```

To keep the reader's findings from a successful read (repairs, lenient reads, items not
read), pass `findings=True`; the document itself is unchanged, so it still compares equal to
other reads:

```python
document, findings = pmi.read("part.step", findings=True)
```

They are the findings `quiddity pmi read --part N` lists for the selected part (those on
another part of the file are left out), each with `kind`, `part` (null for the file), `ids`,
`entity` and `detail`.

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
feature, body, refused, supporting (a boss's unclaimed cap) and unexplained are distinct groups. Do not treat
unproved/refusal records as permission to edit. Retain the consumer's existing
valid-solid, plain-through-hole and stale-source checks, then re-export and
re-recognise after each edit. Use `correspond` for revision matching.

Face index order is a contract: the reader's traversal order, `TopExp::MapShapes` for solids
(solids in assembly traversal order, each shell's faces as listed: ascending `ADVANCED_FACE` id
only for files written that way). `face_sources[i].entity` is the cross-check, and the route when
names are partial. `face_sources[i].instance` is the placed instance (on a single-part multi-solid
file, the solid index); it never restarts the face index, and a part placed repeatedly repeats its
entities per instance. See [evidence.md](evidence.md#face-index-order-is-a-contract).

The face-order trial and reproducible procedure are in
[the trial report](python-face-order.md). The trial concerns another exporter's (build123d/OCC) face order, which quiddity does not control, so
verify that mapping on each export. The trial verified a complete geometric bijection using unique anchor membership,
area and centroid agreement. Apply those checks to each export before issuing
edit handles. Names can also be joined through `face_sources[face].entity` (and edges through
`edge_sources[edge].entity`) when
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
