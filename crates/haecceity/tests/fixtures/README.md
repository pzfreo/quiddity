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
