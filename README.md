# quiddity

Find the manufacturing features in a STEP model: holes, pockets, slots, bosses, steps,
fillets, patterns, springs and turned profiles, with their measured sizes and positions.
quiddity also matches the features of two revisions of a part, and reads and writes AP242
PMI (dimensions, tolerances, datums, notes and their callouts).

It works from the B-rep geometry alone, with no CAD system, feature tree or OpenCascade.
It is pure Rust, and the same input always gives the same output. It is available as a
command line, a Rust library and a Python package.

Status: **0.4.9** (2026-11-07). See [CHANGELOG.md](CHANGELOG.md).

## Install

| | |
|---|---|
| Command line | `cargo install quiddity` |
| Rust library | `quiddity = "0.4"` in `[dependencies]` (the geometry is the `haecceity` crate, a B-rep reader and query library) |
| Python 3.11+ | `pip install quiddity` (an abi3 wheel with no Python dependencies; see [Python](docs/python.md)) |

Optional Cargo features (all off by default):
- `json-import`: the versioned PMI JSON interchange (`pmi json …` commands).
- `f3d`: Autodesk Fusion `.f3d` input (visible solid geometry only, no PMI; see [Fusion input](docs/f3d.md)).

The command line is the default `cli` feature; a library build that does not want clap (WASM,
say) can use `default-features = false, features = ["presentation"]`.

## Quick start

```sh
quiddity part.step                      # the part's features, as JSON
quiddity correspond old.step new.step   # what changed between two revisions
quiddity pmi read part.step             # the file's PMI, as JSON
```

```python
import json, quiddity

document = json.loads(quiddity.recognise("part.step"))["document"]
for feature in document["records"]:
    print(feature["family"], feature["parameters"], feature["faces"])
```

```rust
let part = quiddity::read_step_file(std::path::Path::new("part.step"))?;
let recognised = quiddity::correspondence::recognise(&part);
let document = quiddity::recognition::document(&recognised, part.faces.len());

for record in document.records.iter().filter(|r| r.family == "holes") {
    let p = &record.parameters;
    println!("{}: Ø{} × {} on faces {:?}", record.id, p["diameter"], p["depth"], record.faces);
}
```

## Recognise a part

For a part with a counterbored hole, `quiddity part.step` includes:

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
(`quiddity-rust/recognition/6`; the identifier keeps its original name):

- **`records`**: one entry per feature. Each gives its family, the STEP faces it is made of,
  its measured parameters, and whether its evidence is `verified` or `unproved`.
- **`face_accounting`**: what every face of the part is: part of a feature, part of the body
  (with the reason), or refused or unexplained.
- **`source_sha256`**: the input file's hash.

Every coordinate and direction is in the file's own coordinates. quiddity finds the part's
natural orientation itself, so a part rotated in its file is recognised the same way.

What each field means: [schema 6](docs/recognition-6.md), [face accounting](docs/face-accounting.md),
and [the trust contract](docs/recognition-trust.md), which says what `verified` does and
doesn't promise.

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
| Springs | `helical_springs` |
| Body | `plates`, `polygonal_stock`, `sheet_metal_bodies`, `thin_wall_bodies`, `flats`, `interior_voids`, `cylindrical_profiles`, `freeform_surfaces` |
| Sections | `section_passages`, `section_recesses` |

## Compare two revisions

`quiddity correspond old.step new.step` says what happened to each feature between two
revisions: `carried` (unchanged), `adapted` (with what changed), `orphaned` (gone in the
new revision) or `ambiguous` (more than one candidate, so none is guessed), and which
features are new. It works when the part is moved or re-modelled, because it matches
revision-independent fingerprints rather than face numbers. If a hole moves:

```json
{ "old": "holes/2", "new": "holes/2", "class": "adapted",
  "changes": { "position": { "old": [40.0, 45.0, 5.0], "new": [25.0, 45.0, 5.0] } } }
```

Either side can also be saved recognition JSON. In Rust, use
`quiddity::correspond(&old.fingerprints, &new.fingerprints)`. See [correspondence](docs/correspondence.md).

## Read and write PMI

quiddity reads semantic AP242 PMI and writes it back into the same file. PMI JSON is bound
to the exact STEP text it was read from; a write keeps everything else in the file, and
checks the result by reading it back before replacing the destination.

- **Modes:** `replace` re-authors the part's PMI; `add` adds to it and keeps every existing
  entity byte for byte (it refuses, naming the entities, if that isn't possible); `remove`
  takes PMI out.
- **Callouts:** the writer draws the visible callouts (frames, dimensions, datums, notes)
  in tessellated strokes. It lays them out itself, or uses the positions a source supplies
  (`--source-presentation`, Python `presentation=`), drawing only what the source didn't place.
- **Practice:** the writer follows the CAx-IF PMI Recommended Practice, and the reader
  reports where a file departs from it. See [AP242 practice](docs/step-ap242.md).

```sh
quiddity pmi read part.step [--part N] > part.pmi.json
quiddity pmi check part.step part.pmi.json
quiddity pmi write part.step part.pmi.json -o out.step [--mode add|replace|remove]
```

With `--features json-import`, the versioned interchange ([reference](docs/pmi-json.md)) is
also available, for producers such as XCAF converters:

```sh
quiddity pmi json export part.step > part.pmi.json
quiddity pmi json validate part.pmi.json
quiddity pmi json import part.step part.pmi.json -o written.step [--source-presentation callouts.json]
quiddity pmi json presentation part.step > part.presentation.json
```

The schemas ship in haecceity's `schema/` directory. From Python, `quiddity.pmi` does the
same in process (`pmi.validate`, `pmi.write`); see [Python](docs/python.md).

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
quiddity completions <SHELL>                          a shell completion script
quiddity --help
```

`quiddity <command> --help` gives a command's arguments.

Every command prints JSON. Files may be `.step`, `.stp` or gzipped. The exit code is 0 for
success, 1 for an unreadable file or a refused operation, and 2 for wrong usage.

`quiddity serve` keeps one process open for applications: one JSON request per line on
stdin, one response line per request on stdout, e.g. `{"id":1,"op":"recognise","step":"part.step"}`.
The operations are `recognise` and `correspond`.

**Shell completion.** `quiddity completions SHELL` prints a completion script for `bash`,
`zsh`, `fish`, `powershell` or `elvish`:

```sh
mkdir -p ~/.local/share/bash-completion/completions && quiddity completions bash > ~/.local/share/bash-completion/completions/quiddity
mkdir -p ~/.zfunc && quiddity completions zsh > ~/.zfunc/_quiddity
# zsh: add `fpath=(~/.zfunc $fpath)` before `compinit` in ~/.zshrc, then restart zsh
mkdir -p ~/.config/fish/completions && quiddity completions fish > ~/.config/fish/completions/quiddity.fish
```

## Limits

- quiddity reports what it can prove from the geometry. A file that reads without errors
  isn't a promise that every feature was found: check `face_accounting` for faces left
  unexplained.
- Malformed solids, approximate boundaries and unsupported surfaces can give refusals or
  `unproved` records. Refusals are records too, so nothing is silently dropped.
- Known wrong results are listed, with evidence, in [known defects](docs/known-defects.md).
- How the STEP reader heals input to match OpenCascade: [notes](docs/opencascade.md).

## Packages

| Package | What it is |
|---|---|
| `quiddity` (crate) | recognition, fingerprints, correspondence, the PMI JSON and the command line |
| `haecceity` (crate, `crates/haecceity`) | the geometry: STEP reading, the B-rep, measurement and queries, and semantic PMI reading and writing (it does not model; see its README); re-exported as `quiddity::kernel` |
| `quiddity` (Python, `crates/quiddity-py`) | the Python wheel over both |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md), and [docs/development.md](docs/development.md) for
how the test suite and regression snapshots work. Contributions are taken under
[Apache-2.0](LICENSE), with each commit signed off (`git commit -s`) under the
[Developer Certificate of Origin](CONTRIBUTING.md#developer-certificate-of-origin).

## History

This repository was called quiddity-rust until 2026-11-03; identifiers such as
`quiddity-rust/recognition/6` keep that name. It began as a port of the Python quiddity
library (now [pzfreo/quiddity-python](https://github.com/pzfreo/quiddity-python)), which it no
longer depends on; the archived comparison is in [docs/history](docs/history/python-parity.md).
