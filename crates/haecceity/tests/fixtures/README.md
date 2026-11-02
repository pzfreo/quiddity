# Fusion fixture

`generated_unit_cube.f3d` is copied unchanged from
[`cadmpeg/cadmpeg`](https://github.com/cadmpeg/cadmpeg), source path
`crates/cadmpeg-codec-f3d/tests/golden/fixtures/generated_unit_cube.f3d`.
It is an encoder-generated 10 × 10 × 10 mm cube, distributed under Apache-2.0
(see cadmpeg's root `LICENSE`). SHA-256:
`f582967230959a5bb58f9364e4203052adecd7dee0cd37abdfe7de64dd1c62ff`.

No user designs or Fusion Gallery files are included.

## Legacy PMI plate

`xcaf/plate-legacy-datum-location.step.gz` reuses the committed plate geometry,
trimmed before its material/presentation records, with synthetic PMI: datum A
identifies face #360; a linear dimension forwards through a faceless ordinary
aspect and plain SAR. It exercises legacy reading and migration to direct datum
linking without requiring the git-excluded cadgen/XCAF plate exports.

`xcaf/plate-legacy-datum-location.step.gz` also includes the older multi-face form:
a faceless `DIMENSION` composite duplicates a datum's two per-face members. It
includes a flatness tolerance on datum A and a position control referring to A/B.
`xcaf/plate-datum-features.step.gz` is its conformant counterpart; both dimensions
reference their datum features directly. The default CI matrix checks both files,
including datum-face dimension and tolerance targets, without excluded local plates.

`tests/fixtures/references/pmi_geometry_exemptions.json` pins the exact private
functional endpoint instances authorized for ambiguous shared-carrier SARs
(PMI §5.1), read as composition by both OCCT 7.9.3 XCAF and NIST SFA 5.41.
Keys name the structural condition; tuples are private aspect,
original carrier and functional association. The independent Part 21 checker
verifies those structures before comparing the per-file list. Cardinality of
face/item GISU ownership is never exempted.
