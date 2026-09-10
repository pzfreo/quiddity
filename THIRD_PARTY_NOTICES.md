# Third-party notices

`b123d-recognisers` declares build123d as a runtime dependency; it does not vendor or redistribute
the dependency source or native binaries in its wheel.

## build123d

- Project: <https://github.com/gumyr/build123d>
- Licence: Apache-2.0
- Copyright: the build123d contributors

build123d was originally derived from portions of CadQuery. Its distribution includes the
applicable Apache-2.0 notice.

## CadQuery OCP bindings

- Project: <https://github.com/CadQuery/OCP>
- Licence: Apache-2.0

build123d uses the CadQuery OCP Python bindings to access Open CASCADE Technology.

## Open CASCADE Technology

- Project: <https://github.com/Open-Cascade-SAS/OCCT>
- Licence: GNU LGPL version 2.1 with the Open CASCADE additional exception

OCCT remains under its own licence. Nothing in the Apache-2.0 licence for this project changes
the licence or redistribution conditions of OCCT or any other dependency. A distributor that
bundles dependency binaries must review and satisfy the corresponding binary-distribution terms.


## Vendored test corpora

Most of the STEP models under `tests/corpus` are third-party geometry redistributed here so
that this project's tests run against parts it did not author. They are **not** part of the
published wheel or source distribution — `pyproject.toml` excludes `/tests/corpus` from the
sdist and the wheel packages only `src/quiddity` — and they carry their own terms,
which the Apache-2.0 licence of this project does not alter.

`tests/corpus/gramel` is one exception: it is the author's own part, under this project's own licence, and needs
no third-party notice. It is vendored for the same reason the others are — the two third-party
corpora are milled prismatic parts and contain no turned geometry at all, so the turned
families had no real part behind them.

### Generated CADGenBench submissions (`tests/corpus/cadgenbench`)

These are another exception: two generated build123d outputs from the author's Apache-2.0
CADGenBench harness, included under this project's Apache-2.0 licence. They are not copies
of benchmark reference drawings or third-party reference STEP models. The directory README
records the submission identities and SHA-256 hashes. They are excluded from published
packages along with the rest of `tests/corpus`.

### NIST MBE PMI test cases (`tests/corpus/nist`)

- Source: <https://www.nist.gov/ctl/mbe-pmi-validation-and-conformance-testing-project/mbe-pmi-0>
- Files: ten of the eleven AP203 geometry-only variants — `ctc_01`–`ctc_05` and `ftc_06`–`ftc_10`.
  `ftc_11` is not vendored because no test reads it.
- Terms: NIST states the models "can be used without any restrictions" and requests
  acknowledgement, which this notice provides. NIST further states that such use "does not
  imply a recommendation or endorsement by NIST".

### MFCAD++ (`tests/corpus/mfcadpp`)

- Source: <https://doi.org/10.17034/d1fec5a0-8c10-4630-b02e-b92dc81df823>
- Licence: Creative Commons Attribution (**CC BY** — the source record states no version)
- Modifications: none. The files are byte-identical to the upstream distribution.
- Copyright: Queen's University Belfast and the dataset's authors
- Contents: a forty-model subset of the *test* split; `tests/corpus/mfcadpp/MANIFEST.json`
  records the rule that selected it

As CC BY requires, the dataset is attributed to its authors and its paper cited:

> Colligan AR, Robinson TR, Nolan DC, Hua Y, Cao W. *Hierarchical CADNet: Learning from B-Reps
> for Machining Feature Recognition.* Computer-Aided Design, 147:103226, 2022.
