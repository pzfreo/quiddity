# quiddity

Find the manufacturing features in a STEP model: holes, pockets, slots, bosses, steps,
fillets, patterns, springs and turned profiles, with their measured sizes and positions.
quiddity also matches the features of two revisions of a part, and reads and writes AP242
PMI (dimensions, tolerances, datums, notes and their callouts).

It works from the B-rep geometry alone, with no CAD system, feature tree or OpenCascade.
It is pure Rust, and the same input always gives the same output. It is available as a
command line, a Rust library and a Python package.

![Eight public parts turning while quiddity's recognised features light up family by family: holes and counterbores, teeth, grooves, sheet-metal bends and flanges, pockets, slots, chamfers, fillets and V-channels](docs/images/showcase.gif)

*Features recognised by quiddity on eight public parts. Colours and counts are quiddity's own
output. Parts: NIST MBE PMI test cases; FreeCAD library parts by Rbgo Web, Francisco and
hasecilu ([CC BY 3.0](https://creativecommons.org/licenses/by/3.0), renders adapted); text-to-cad
(© Thompson Labs LLC, MIT); build123d / Gumyr / Too Tall Toby (Apache-2.0). Rendered with
draftwright-editor's `npm run showcase:render`.*

![Automatically recognised features of NIST CTC-01: two views, each face coloured by the feature quiddity recognises it as: holes, slots, notches, V-channels, the hexagonal boss, chamfers and fillets](docs/images/ctc01-features.png)

*Each face of NIST CTC-01 (AP242) coloured by the feature it belongs to; grey faces belong
to no feature shown here.*

Status: **0.5.1** (2026-11-13). See [CHANGELOG.md](CHANGELOG.md).

The [geometric validation accuracy tool](docs/geometric-validation.md) compares exact
volume, area and centroid against STEP validation properties and independent OCCT measurements.

## Runs on

| Target | Get it | Tested in CI |
|---|---|---|
| Python 3.11+ | `pip install quiddity`: one abi3 wheel, no Python dependencies | Wheels for Linux x86_64 and aarch64 (manylinux 2_28), macOS arm64 and x86_64, and Windows x64 are built and their Python tests run on each, with Python 3.11; also on Linux x86_64 with 3.15 |
| Command line | `uv tool install quiddity`, `pipx install quiddity` or `cargo install quiddity` | Run by the Python tests on all five wheel platforms, and by the Rust tests on Linux and macOS |
| Rust library | `quiddity = "0.5"` or `haecceity = "0.5"` | Full Rust test suite on Linux and macOS (no Rust test run on Windows) |
| Browser (WebAssembly) | `haecceity` and `quiddity` (`default-features = false, features = ["presentation"]`) build for `wasm32-unknown-unknown`; the draftwright-editor engines built on this kernel run in the browser | Not built for WebAssembly in CI; CI checks the library without its command line on Linux. Both crates were built for `wasm32-unknown-unknown` by hand at 0.5.1 |

## Install

| | |
|---|---|
| Command line | `uv tool install quiddity` (or `pipx install quiddity`): the Python package's `quiddity` command, with no Rust toolchain needed; upgrade with `uv tool upgrade quiddity`. Or `cargo install quiddity`. |
| Rust library | `quiddity = "0.5"` in `[dependencies]` (the geometry is the `haecceity` crate, a B-rep reader and query library) |
| Python 3.11+ | `pip install quiddity` (an abi3 wheel with no Python dependencies; see [Python](docs/python.md)) |

Every [release](https://github.com/pzfreo/quiddity/releases) is published to PyPI and
crates.io, so `uv tool`, `pipx`, `pip`, `cargo install` and `quiddity = "0.5"` all get the
latest one.

Cargo features: `cli` (the command line) and `presentation` (drawing PMI callouts) are on by
default. A library build that does not want clap (WASM, say) can use
`default-features = false, features = ["presentation"]`. `manifest` enables the range-based
assembly APIs and is included by `cli`. Off by default:
- `json-import`: the versioned PMI JSON interchange (`pmi json …` commands).
- `f3d`: Autodesk Fusion `.f3d` input (visible solid geometry only, no PMI; see [Fusion input](docs/f3d.md)).

Read large assemblies one product at a time with `quiddity manifest` and
`quiddity recognise --manifest`; see [assembly manifests](docs/assemblies.md)
for range checks, placements, fallback and Python APIs.

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

When both complete recognition and face evidence are needed,
[`recognise_with_evidence`](docs/shared-analysis.md) shares their analysis while
preserving each document's categories, coordinates and error semantics.

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

## Accuracy and speed

Measured with quiddity 0.5.1 on a 4-vCPU Ubuntu 24.04 x86_64 cloud VM, 13 November 2026. The full report is
[`docs/benchmarks/mfinstseg-v0.5.1-3a20dceb.html`](docs/benchmarks/mfinstseg-v0.5.1-3a20dceb.html); commands,
provenance and the raw data are in [`docs/benchmarks/v0.5.1-measurements.md`](docs/benchmarks/v0.5.1-measurements.md).

**MFInstSeg test split (9,375 models).** All 9,375 read and recognised without error. Scored as face coverage
against the dataset's labels (a labelled instance is recognised when at least half its faces are claimed):

| Class | Recognition | Claimed faces | False positives |
|---|---:|---:|---:|
| Holes | 98.5% | 95.0% | 6.2% |
| Rounds | 89.3% | 79.5% | 0.2% |
| Any feature | 84.3% | 83.1% | 0.4% |

Per model, read takes a median 13 ms (95th percentile 23 ms) and recognition 73 ms (95th percentile 242 ms),
as calling-thread CPU time. The report also has the stricter one-to-one matching (instance IoU of at least 0.5)
that the repository's regression baseline uses, per class: precision and recall are 0.86 and 0.77 over the 24
mapped classes.

**Robustness.** On a fixed 10% sample of all 62,495 MFInstSeg models (every tenth in lexical order, 6,250 models):
0 read failures, 0 recognition failures and 0 panics.

**Geometry accuracy on known parts.** haecceity's volume, area and centroid, with tolerance 0.1% (the centroid
distance is relative to the bounding-box diagonal). Against the validation properties stored in 18 NIST STEP
files, 103 of 106 checks are within tolerance, the largest in-tolerance error being 0.009%; the other three are
one file (NIST STC-06) whose stored properties are zero, where we agree with OpenCascade. Against OpenCascade
over 2,868 files (9,120 checks), 9,110 are within tolerance, 8 are known OpenCascade reference problems, and 2
differ (one part: 0.35% volume, 0.16% area).

**From Python.** Compared with the last pure-Python quiddity (0.3.12, built on OpenCascade) on the same machine,
on 10 NIST parts and 200 MFInstSeg models, 0.5.1 recognises a part a median 10.7 times faster (median 0.096 s
against 1.085 s per part), imports in 0.02 s instead of about 6 s, peaks at a median 32 MB instead of 418 MB, and
installs as one 25 MB package instead of 59 packages and 876 MB. The two recognise different feature sets, so the
speed-up is between the two products, not between two implementations of one algorithm.

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
quiddity evidence <file.step> [--framed]              the recognition evidence for each feature
quiddity fingerprints <file.step>                     the features' fingerprints
quiddity capabilities                                 this build's versioned capabilities
quiddity parts <file.step>                            list an assembly's parts
quiddity manifest <file.step> -o <manifest.json>       scan product ranges
quiddity recognise <file.step> --manifest <manifest.json> --out-dir <dir> [--parts p0,p7]
                                                       recognise products and write results
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

Recognition and inspection commands print JSON; `manifest` and `recognise --manifest`
write their requested files. `completions`, help and version print text. STEP inputs may be
`.step`, `.stp` or gzipped; manifest product reads require an uncompressed source.
The exit code is 0 for success, 1 for an unreadable file or a refused operation,
and 2 for wrong usage.

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
