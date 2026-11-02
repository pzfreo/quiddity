# quiddity-rust

Python users: [quiddity 0.4.0 wheel API and migration guide](docs/python.md).

Find the manufacturing features in a STEP model (holes, pockets, slots, bosses, steps,
fillets, patterns and turned profiles), with their measured sizes and positions. quiddity
also matches the features of two revisions of a part, and reads and writes AP242 PMI
(dimensions, tolerances, datums and notes).

It works from the B-rep geometry alone, with no CAD system, feature tree or OpenCascade.
Everything is pure Rust, and the same input always gives the same output.

Status: **0.4.5** (2026-11-02). See [CHANGELOG.md](CHANGELOG.md).

## Install

```sh
cargo install quiddity          # the `quiddity` command line
```

Add `--features f3d` to import Autodesk Fusion `.f3d` parts (visible solid geometry only, no PMI); see [Fusion input](docs/f3d.md). The feature is off by default in both crates.

```toml
[dependencies]
quiddity = "0.2"                # the library; the geometry kernel is the `haecceity` crate
```

## Command line

```text
quiddity <file.step>                                  recognise the part's features
quiddity correspond <old.json|old.step> <new.json|new.step>   match features across revisions
quiddity parts <file.step>                            list an assembly's parts
quiddity pmi read <file.step> [--part N]              the file's PMI as JSON
quiddity pmi check <file.step> <pmi.json>             validate a PMI document against the part
quiddity pmi write <file.step> <pmi.json> -o <out.step> [--mode add|replace|remove]
                   [--presentation refuse|remove]     write PMI into the file
quiddity brep <file.step> -o <out.step> [--pmi <pmi.json>]   the B-rep and PMI as a new AP242 file
quiddity serve                                        JSON-lines requests on stdin
quiddity --help
```

Every command prints JSON. Files may be `.step`, `.stp` or gzipped. The exit code is 0 for
success, 1 for an unreadable file or a refused operation, and 2 for wrong usage.

## Recognise a part

`quiddity part.step` prints the part's features. For a part with a counterbored hole, the
output includes:

```json
{
  "id": "holes/0",
  "family": "holes",
  "faces": [6, 8],
  "parameters": {
    "diameter": 8.0, "depth": 14.0, "bottom": "through",
    "cbore": { "diameter": 16.0, "depth": 6.0 },
    "axis": [0.0, 0.0, -1.0], "location": [-22.0, 0.0, 10.0]
  },
  "evidence_status": "verified"
}
```

That record is one entry of the output's `document`, the versioned form to build on
(`quiddity-rust/recognition/6`):

- **`records`**: one entry per feature. Each gives its family, the STEP faces it is made of,
  its measured parameters and whether its evidence is `verified` or `unproved`.
- **`face_accounting`**: what every face of the part is: part of a feature, part of the body
  (with the reason), or refused or unexplained.
- **`source_sha256`**: the input file's hash.

Every coordinate and direction is in the file's own coordinates. quiddity finds the part's
natural orientation itself, so a part rotated in its file is recognised the same way.

What each field means: [schema 6](docs/recognition-6.md), [face accounting](docs/face-accounting.md)
and [the trust contract](docs/recognition-trust.md), which says what `verified` does and
doesn't promise.

The same from Rust:

```rust
let part = quiddity::read_step_file(std::path::Path::new("part.step"))?;
let recognised = quiddity::correspondence::recognise(&part);
let document = quiddity::recognition::document(&recognised, part.faces.len());

for record in document.records.iter().filter(|r| r.family == "holes") {
    let p = &record.parameters;
    println!("{}: Ø{} × {} on faces {:?}", record.id, p["diameter"], p["depth"], record.faces);
}
```

### Families

| Kind | Families |
|---|---|
| Holes | `holes` (through, blind, counterbored, countersunk, spotfaced), `countersinks`, `hole_patterns`, `double_d_bores`, `modelled_threads` |
| Pockets and slots | `pockets`, `prismatic_pockets`, `edge_open_circular_pockets`, `edge_open_prismatic_recesses`, `pocket_patterns`, `slots`, `rectangular_blind_slots`, `round_bottom_blind_slots`, `semicircular_blind_slots`, `slot_patterns`, `rectangular_through_notches` |
| Channels and grooves | `channels`, `v_channels`, `grooves`, `annular_grooves`, `face_grooves` |
| Steps and levels | `through_steps`, `angled_steps`, `oblique_through_steps`, `paired_ramp_steps`, `circular_blind_steps`, `step_levels`, `risers` |
| Raised features | `bosses`, `polygonal_bosses`, `pads`, `gusset_ribs`, `gusset_rib_patterns` |
| Edges | `fillets`, `blends`, `chamfers` |
| Turned parts | `turned_bodies`, `turned_steps`, `repeating_radial_profiles`, `circular_face_patterns` |
| Body | `plates`, `polygonal_stock`, `sheet_metal_bodies`, `thin_wall_bodies`, `flats`, `interior_voids`, `cylindrical_profiles`, `freeform_surfaces` |
| Sections | `section_passages`, `section_recesses` |

## Compare two revisions

`quiddity correspond old.step new.step` says what happened to each feature between two
revisions of a part: `carried` (unchanged), `adapted` (with what changed), `orphaned` (gone
in the new revision) or `ambiguous` (more than one candidate, so none is guessed), and which
features are new. It works when the part is moved or re-modelled, because it matches
revision-independent fingerprints instead of face numbers. If a hole moves, its entry
looks like this:

```json
{ "old": "holes/2", "new": "holes/2", "class": "adapted",
  "changes": { "position": { "old": [40.0, 45.0, 5.0], "new": [25.0, 45.0, 5.0] } } }
```

`correspond` also takes saved recognition JSON for either side. In Rust, use
`quiddity::correspond(&old.fingerprints, &new.fingerprints)`. See [correspondence](docs/correspondence.md).

## Read and write PMI

PMI JSON is bound to the exact STEP text it was read from. A write keeps everything else in
the file, and it checks the result by reading it back before replacing the destination. The
reader reports where a file departs from the AP242 recommended practices. See
[AP242 practice](docs/step-ap242.md) and [`src/pmi_json.rs`](src/pmi_json.rs).

## Assemblies and applications

- `quiddity parts file.step` lists the parts of an assembly; `pmi read --part N` reads one of them.
- `quiddity serve` keeps a process open for applications: one JSON request per line on
  stdin, one response line per request on stdout. For example,
  `{"id":1,"op":"recognise","step":"part.step"}`. The operations are `recognise` and `correspond`.

## Limits

- quiddity reports what it can prove from the geometry. A file that reads without errors
  isn't a promise that every feature was found: check `face_accounting` for faces left
  unexplained.
- Malformed solids, approximate boundaries and unsupported surfaces can give refusals or
  `unproved` records. Refusals are records too, so they are visible and not silently dropped.
- Known wrong results are listed, with evidence, in [known defects](docs/known-defects.md).
- How the STEP reader heals input to match OpenCascade: [notes](docs/opencascade.md).

## Crates

- `quiddity`: recognition, fingerprints, correspondence, the PMI JSON and the command line.
- `haecceity` (`crates/haecceity`): the geometry kernel. It reads STEP, builds the B-rep,
  measures faces and bodies, and reads and writes semantic PMI. It is re-exported as
  `quiddity::kernel`.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md), and [docs/development.md](docs/development.md) for
how the test suite and regression snapshots work. Licensed under [Apache-2.0](LICENSE).

## History

quiddity-rust began as a port of the Python quiddity library. It no longer depends on it,
and the archived comparison is in [docs/history](docs/history/python-parity.md).


With `--features json-import`, the command line also exposes haecceity's versioned PMI
interchange (see [the reference](docs/pmi-json.md)):

```sh
cargo run --release --features json-import -- pmi json export part.step > part.pmi.json
cargo run --release --features json-import -- pmi json validate part.pmi.json
cargo run --release --features json-import -- pmi json import part.step part.pmi.json -o written.step
cargo run --release --features json-import -- pmi json presentation part.step > part.presentation.json
```

Use `--part N` for a multipart file. Import accepts face names directly; for a
Specify support key, `--anchors namespace.json` supplies a consumer-verified
`{"schema":2,"faces":"<face fingerprint>"}` namespace. The schemas ship in
haecceity's `schema/` directory; Specify can package these as
`specify_core/schema/haecceity.{pmi,presentation}-1.json`. Presentation JSON is
read-only. The JSON path reports existing STEP writer refusals, including missing
nominals/geometry. Requirement IDs are verified on read-back; support features
retain the model's array indices.
