# Geometric validation property fixtures

Inputs for `crates/haecceity/tests/gvp.rs`, which checks haecceity's volume, surface
area and centroid against each file's own geometric validation properties
(`docs/geometric-validation.md`). The test also reads every other committed fixture that
carries such properties; this directory holds only those added for it. They are kept
apart from `ap242/nist/` so that the PMI tests reading that directory are unaffected.

## NIST models (`nist/`)

From `NIST-PMI-STEP-Files.zip` in the `Release` directory of
[usnistgov/SFA](https://github.com/usnistgov/SFA) (NIST's MBE PMI test models,
February 2026 update; zip sha256
`1fb91bb8ff0fe02032b948fda0775bc74591cd0bebc0988347d32574e5884f90`, the same copy as
`ap242/nist/`). Stored unchanged, compressed with `gzip -9 -n`.

| File | SHA-256 of uncompressed file | Why it is here |
|---|---|---|
| `nist_stc_08_asme1_ap242-e3.stp.gz` | `64f24b914910ce8e20c9e4bd59637b4fd155715c7c7b51689466534ed7db1442` | wetted area on a part with an independent sheet |

Notice: these files are works of the US government (NIST) and are not subject to
copyright protection in the United States. The MBx-IF lists them as usable "without any
restrictions"; NIST's [disclaimer](https://www.nist.gov/disclaimer) applies. NIST says they
are not error-free reference files.

The zip's other GVP-bearing files are either already committed under `ap242/nist/` (with
identical hashes) or add no check: CTC-04 e2 measures the same and stores the same properties as
`ap242/nist/nist_ctc_04_asme1_ap242.stp.gz`; CTC-05 e1 and STC-07 carry no volume, area
or centroid; FTC-08 e4-tg is tessellated only, with no B-rep for haecceity to measure.
