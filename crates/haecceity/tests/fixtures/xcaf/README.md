# Outside-producer PMI fixture

Provided with the #142 fix-round-1 brief: xcaf-pmi converted cadgen’s
`pmi_plate.xbf` into `ours.pmi.json`, imported onto `pmi_plate.geom.step`.
The semantic data matches cadgen’s own AP242 on the same faces.

The committed JSON migrates the draft’s index references to the settled v1
id-reference contract; it retains caller requirement IDs and OCCT face-name
anchors. The geometry is the supplied STEP, losslessly gzip-compressed.

`pmi_plate.source.pmi.json` and `pmi_plate.source.presentation.json` were extracted
read-only with xcaf-pmi on 2026-11-02 from cadgen-run's `pmi_plate.xbf` (job
41778893), using `--haecceity` and `--presentation`. They share the geometry in
`pmi_plate.geom.step.gz`. The companion supplies four dimensions, four tolerances
and three datums. xcaf-pmi omits the flag-note position; the source-placement test
adds a handmade flag at [25, 35, 5] with tip [5, 0, 5], without changing the source
fixture. The section A-A diameter uses its main item placement. No saved cameras
are written in phase 1.
