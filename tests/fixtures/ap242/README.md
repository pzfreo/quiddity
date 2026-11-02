# AP242 PMI oracles

The fixtures the AP242 reader and writer (`docs/step-ap242.md`) are tested against, in the
design's order of authority: NIST's expected PMI (`nist/`), then OpenCascade's reading
(`occt/`) and draftwright's (`draftwright/`) as cross-checks, and specify-core's output
(`specify/`) as reader inputs only. Every capture records what its source says, as it says it;
none of them interprets or corrects.

| Directory | What | Made by |
|---|---|---|
| `nist/*.stp.gz` | 7 NIST MBE PMI test models (AP242 semantic PMI) | copied, gzipped (`mtime=0`) |
| `nist/<model>.expected.json` | NIST's expected PMI for each of them | `tools/nist_expected.py` |
| `occt/<file>.json.gz` | OpenCascade XCAF's reading of every NIST AP242 file and every `specify/` file | `tools/capture_pmi_occt.py` |
| `specify/<case>.*` | specify-core's output on corpus parts, with intent and answers | `tools/make_specify_inputs.py` |

## NIST models (`nist/`)

From `NIST-PMI-STEP-Files.zip` (NIST's MBE PMI test models; sha256 of the copy used
`1fb91bb8ff0fe02032b948fda0775bc74591cd0bebc0988347d32574e5884f90`), files dated as in its
February 2026 update. NIST says these are not error-free reference files.

| File | Edition (`FILE_SCHEMA`) | Units: drawing / STEP | Why it is here |
|---|---|---|---|
| `nist_ctc_01_asme1_ap242-e1.stp` | e1 `{… 442 1 1 4}` | none stated in the drawing text / mm | small; ±, limits, angle; datums B and C on half-cylinder pairs |
| `nist_ctc_02_asme1_ap242-e2.stp` | e2 `{… 442 3 1 4}` | none stated / mm | many tolerances and datums, datum targets |
| `nist_ftc_07_asme1_ap242-e2.stp` | e2 | `UNITS: INCHES` / inch | inch units |
| `nist_ftc_10_asme1_ap242-e2.stp` | e2 | `UNITS: MILLIMETERS` / mm | `LIMITS_AND_FITS('G6','hole','','')` ×3: not conforming (form variance holds the grade) |
| `nist_stc_10_asme1_ap242-e2.stp` | e2 | no drawing / mm | `LIMITS_AND_FITS('G','','6','')` ×3: conforming |
| `nist_stc_06_asme1_ap242-e3.stp` | e3 `{… 442 4 1 4}` | no drawing / geometry context mm, PMI measures inch | edition 3; mixed units |
| `nist_stc_09_asme1_ap242-e4.stp` | e4 `{… 442 7 1 4}` | no drawing / inch | edition 4 |

Total 3.2 MB gzipped. The STC models (based on the FTC models with less complex PMI) have no
drawing in the release.

### Expected PMI (`nist/<model>.expected.json`)

Obtained, not transcribed. The STEP File Analyzer and Viewer checks a NIST model's semantic PMI
against NIST's expected PMI (`sfa-nist.tcl`, `nistReadExpectedPMI`: columns A, the entity type,
and B, the PMI as SFA prints it, of the first sheet of `SFA-PMI-<model>.xlsx`, and the coverage
table `SFA-PMI-NIST-coverage.csv`). Both ship in `SFA-NIST-files.zip`, which is a member of the
freewrap archive inside `STEP-File-Analyzer.exe`, stored as one zlib stream:

| | sha256 |
|---|---|
| `SFA-5.51.zip` (usnistgov/SFA release) | `d7d06e2454c8cc9d6a41049b25d38eeb7822226660861af41e62613238d1a13c` |
| `STEP-File-Analyzer.exe` in it | `5bfa13823d5c104b2229c3d5dff4fd47f9f30cb17f1e9bbea78b749dc5104ba6` |
| `SFA-NIST-files.zip` (zlib stream at offset 4492140) | `5701884ac696f87bd5342d2bf099c15b01802b55d54829d0b355bc464175d1a1` |

Each `expected.json` holds every spreadsheet row verbatim (all columns, as SFA has them: e.g.
`⌀3.50 G6`, `⌖ | ⌀0.8 Ⓟ 3.2 | D | E | B`, `27.5 ± 1`, with SFA's own line breaks), the model's
coverage column verbatim, each spreadsheet's sha256, and `units`: the drawing's unit note
(`pdftotext` of `PDF/<model>_*.pdf`, null where the drawing text states none or there is no
drawing), the length units the STEP file's `GLOBAL_UNIT_ASSIGNED_CONTEXT`s name, and a count, by
unit, of the length units that `MEASURE_WITH_UNIT(value, #unit)` occurrences in the text
reference (a textual count, context for the reader, not a semantic reading). How SFA compares
(it normalises zeros and matches strings) is SFA's; the strings here are the data before that.

Because the expected PMI is NIST's own data, no transcription from the drawings was made and no
second transcription pass applies; there are no transcription errors to record. Where a reader
result differs from a row, the difference is to be resolved against the row, the drawing (CTC
and FTC models) and the file text, and recorded with its verdict.

## Solid Edge re-export pair (`reexport/`)

These are the maintainer-supplied NIST MBE PMI CTC-01 test model (US government work,
redistributed on the same basis as `nist/`) and its AP242 re-export made by the maintainer
in **Solid Edge 2026**. The re-export header states originating system `Solid Edge` and
preprocessor `ST-DEVELOPER v20`. Both inputs are stored unchanged, compressed with
`gzip.compress(..., compresslevel=9, mtime=0)`.

| File (under `reexport/`) | SHA-256 of uncompressed file |
|---|---|
| `01-NIST-control-ctc01.stp.gz` | `85a5752da05f53c456ca3a9e038c90358e1d5a3141d1f0d6e5f0970f2356e821` |
| `09-NIST-reexported-by-SE.stp.gz` | `1441bd2f98cea7ec6337b3bba7f1f50d8215d9063c1344c9b37ec02ecccb4e95` |

The control is the exact input used for this re-export. It shares its header with
`nist/nist_ctc_01_asme1_ap242-e1.stp.gz` but differs in entity layout and is **not
byte-identical** to that copy. The control's datums A, B and C resolve to faces; after the
re-export exactly B's datum feature resolves to no faces. Reader tests pin that loss by
label, including its unresolved diagnostic, so silently repairing or hiding it fails.
The reader omits the unestablished B from its valid semantic model and emits
`Unresolved` (`unresolved-reference`):
`datum B is not read: what establishes it does not resolve`.
The shared check includes that public finding as an unresolved datum as well as walking
the retained datums' feature graphs. Writer round trips independently check that resolved
datum features still reach faces.

The same check covers all seven committed `nist/` models. CTC-02 is the sole exception:
A, B, C and K are established only by placed datum targets (3 points, 4 points, 1 point,
and 1 circle respectively), with no datum feature. Their target support and placements
are retained; they are pinned as lacking datum-feature faces, rather than treating a
placement as a face. Every other datum in the committed NIST models resolves to a face.

## OpenCascade (`occt/`)

`tools/capture_pmi_occt.py`, specify-core's venv (OCP 7.9.3.1), `STEPCAFControl_Reader` with
GD&T, names and colours on, as specify-core's `load.py`. Per distinct part (specify-core's
numbering: `TopExp::MapShapes` face order on each part's own shape): dimensions, geometric
tolerances (with the datums XCAF links to each, their positions and modifiers), datums (with
datum target parameters and the tolerances that cite them) and materials, every value as
returned, and the faces each item references as `{part, face, advanced_face: "#N"}`. Items whose
faces are on no part are under `unattached`. Captured: every `*_ap242-*.stp` of the NIST release
(17, not only the 7 committed models) and every `specify/` file.

- `#N`: `TransferReader::EntityFromShapeResult` gives the entity; OpenCascade numbers entities
  by rank (file order), so the rank indexes the file's instances in order. The tool checks the
  instance count and that each face's instance is an `ADVANCED_FACE`, and stops otherwise.
- Class of tolerance (ISO 286): the OCP binding of `GetClassOfTolerance` cannot return its output
  arguments, so `class` comes from the object's `DumpJson` (`IsHole`, `FormVariance`, `Grade`).
- `known_misreads` in every capture lists what OpenCascade is known to misread
  (specify-core `writer.py`, `existing.py`). Values are not corrected: e.g. `spool_fits` reads
  Ø130 g6 as `value 65.0195, range, lower_bound 130.0, upper_bound 0.039`, and NIST magnitudes
  held in complex entities read as 0. Each difference from the haecceity reader gets a verdict.

## Rust PMI regression record

Python draftwright outputs and its capture tool have been retired. The same 14 STEP inputs
are listed in `../pmi_inputs.json`; `tests/pmi_snapshot.rs` records their complete Rust semantic
PMI documents and part bindings through `cargo run -p snapshot-tool --release --`. NIST, stated intent,
OpenCascade XCAF, analytic writer and round-trip reference checks remain independent.
Historical verdicts and evidence are in `docs/history/python-parity.md`.

## specify-core inputs (`specify/`)

**These are reader inputs only. They show how specify-core writes PMI through OpenCascade
(XCAF plus text it appends or transplants); they are not a reference for correct AP242.** They
carry the defects the design lists as anti-requirements (fits written as limits, OpenCascade's
complex and untyped forms where specify-core did not rewrite them, …).

`tools/make_specify_inputs.py` runs specify-core's CLI (`analyse`, then `write
--accept-defaults --intent`) with fixed answers. Per case: `<case>.step.gz` (the output),
`<case>.intent.json`, `<case>.answers.json`, `<case>.meta.json` (input and its sha256,
specify-core commit `cc341dd…`, specify-core's write report: what it wrote).

| Case | Input | Covers |
|---|---|---|
| `spool_fits` | corpus `cadgenbench/flanged_spool_132.step` | Ø62 H7 bore (datum B), Ø10 H11 clearance bolt circle with position 0.2 Ⓜ, Ø130 g6 and Ø70 f7 (both deviations below nominal), circular runout, flatness of A, perpendicularity of B, material, ISO 2768-m |
| `thumbwheel_thread_knurl` | corpus `gramel/GRM-03_thumbwheel_drive_screw.step` | tapped M2 hole with position, external M5 thread, diamond knurl, Ø10 f7 with runout |
| `bolt_thread_knurl` | corpus `gramel/GRM-05_depth_lock_bolt.step` | external M10 thread, straight knurl, Ø6 h6 with runout |
| `string_post_tapped` | corpus `gramel/string_post.step` | tapped M2 holes, clearance hole with position Ⓜ, basic location dimensions, flatness |
| `bracket_positions` | corpus `cadgenbench_inputs/cgb207.step` | datums A, B, C; flatness, two perpendicularities; two hole sets positioned Ø Ⓜ to the frame with basic location dimensions |
| `nist_ctc_01_merge` | NIST `nist_ctc_01_asme1_ap242-e1.stp` | specify-core adding to a part that already has PMI (its text transplant, `merge.py`) |
| `assembly_plate_pin` | two-part assembly (below) | both parts written at once, datum A on each part, tapped holes, clearance grid with position Ⓜ, basic locations, g6 with runout |

The assembly is the faces stream's `tests/fixtures/ap242/assembly/assembly.step` (a plate and a
pin placed twice; sha256 `ca5711668102f0e8bb9920469a46008e8a51d60e0bea7eb89a03089b0e464291`),
which was not yet committed when these inputs were made: a copy was used (`ASSEMBLY=`). If the
committed file differs, regenerate this case.

## Material practice

The PMI practice (lines 498 and 666 of `rec_pracs_pmi_v41.txt`) defers material to the CAx-IF
*Recommended Practices for Material Identification and Density*. Obtained:

- Release 2.1, July 12, 2005 (J. Boy, PROSTEP; P. Rosche, ATI/PDES), 17 pages,
  `RecPrac_MaterialDensity_v21.pdf`, sha256
  `11c81d9365236a835a70b9e0d223316d99a2da87b85a0b5dbbeff89d72e88448`, stored in the session
  scratch directory `ap242-caxif/` with its `pdftotext` text.
- Routes: a web search found `https://www.mbx-if.org/documents/RecPrac_MaterialDensity_v21.pdf`
  (404 now; `www.cax-if.org/documents/…` 403, `www.cax-if.de/documents/…` 404). The Wayback CDX
  for that path lists captures from 2010 to 2024 on cax-if.org and mbx-if.org, all with one
  digest (SHA-1 base32 `BMZQGVRRIIEEOHBOY7NBNTDYPMZX5E7T`); the copy fetched from
  `web.archive.org/web/20240419135511id_/https://www.mbx-if.org/documents/RecPrac_MaterialDensity_v21.pdf`
  has that SHA-1. (An earlier CDX search, by the mbx-if.org index, had found nothing.)
- Sections: §4 "Material and Density as General Property" (the approach the CAx-IF agreed for
  basic exchange): §4.1 material name and id — `property_definition('material property',
  'material name', <product_definition>)`, `representation('material name', …)` holding a
  `descriptive_representation_item(<material id>, <material name>)` (example
  `('AMS4928','Titanium 6-4')`), joined by `property_definition_representation`; optionally on a
  `shape_aspect` for a sub-shape; §4.2 density — `property_definition('material property',
  'density', …)`, `representation('density', …)` holding `measure_representation_item('density
  measure', POSITIVE_RATIO_MEASURE(v), <derived unit>)`. §5 "Detailed Material Identification"
  (material as a product, `make_from_usage_option`; density as general or material property),
  §6 Part 21 examples for AP214 and AP203. The document predates AP242; it names AP214 and AP203
  only.

## Regenerating

From the repository root (`S` is a directory holding the downloads):

```
# NIST fixtures: gzip the 7 files above from NIST-PMI-STEP-Files (gzip.compress, level 9, mtime=0)
python3 -I tools/nist_expected.py S/SFA-5.51.zip S/NIST-PMI-STEP-Files
QUIDDITY_CORPUS=../quiddity/tests/corpus NIST_PMI=S/NIST-PMI-STEP-Files ASSEMBLY=<assembly.step> \
  SPECIFY_CORE=../specify-core python3 tools/make_specify_inputs.py      # --check to compare
../specify-core/.venv/bin/python tools/capture_pmi_occt.py S/NIST-PMI-STEP-Files
```

Order matters: the OpenCascade and draftwright captures read `specify/` and `nist/`.

Verified on 2026-10-08: rerunning each tool reproduces its fixtures byte for byte, except the
STEP header `FILE_NAME` time stamp of the `specify/` outputs, which OpenCascade sets when it
writes (`make_specify_inputs.py --check` masks exactly that field and found no other
difference).

## Specify 0.1.4 inspection identifiers

`identifiers/specify-0.1.4-plate.step.gz` was written by **specify-core-rust v0.1.4** from
Draftwright editor's **Plate** sample, the maintainer's own model. Copied unchanged from
`i265/tests/fixtures/samples/specify-0.1.4-plate.step.gz` supplied for this change.
Compressed SHA-256: `983cc8b2b4b6e8b107a65e0053bc078fef81c2a5f00a66dcdb4aac2c31219e14`.
It contains 34 numbered requirements: 3 datums, 26 dimensions, 3 geometric tolerances,
1 part-default surface texture and 1 internal thread. `pmi_identifiers.rs` pins every
carrier/value and checks a lossless writer append. OCP 7.9.3.1 / XCAF successfully
transfers and safely walks this file via `tools/check_pmi_occt.py::check` (26 dimensions,
3 tolerances, 9 datum labels including references); identifier assertions use the actual
STEP records, since XCAF does not expose these ids or surface textures.
