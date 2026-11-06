<!-- Draft for https://github.com/tangency-labs/solidcore/issues — not posted.
     Before posting: decide how to share the STEP file (see the note at the end of the
     conversation), and attach it or link it where marked. -->

**Title:** `boolean_difference` returns wrong results with `Ok` on an imported revolved part (empty, unchanged, or larger than the input)

## Summary

Subtracting a half-space box from an imported 92-face revolved part (a flanged spool: planes,
cylinders, cones and four tori) returns a wrong body in 18 of 26 cuts, **with `Ok` every time**.
Depending on where the cut is, the result is:

- empty (volume 0, 2 to 8 faces);
- unchanged (nothing removed);
- a few percent too small; or
- larger than the input part.

Cuts across the part's axis are mostly right. Cuts parallel to it are mostly wrong. The volumes
below were checked against OpenCascade 7.8.

This is on `main` at `513e6fc`. The same failures happen on the published `solidcore 1.0.1`.

## Reproduction

The repro uses only solidcore crates: `step_brep::import_with_report` to read the file, then
`boolean_difference` with a 2000-unit box covering everything beyond the cut plane.

<!-- attach: repro crate (Cargo.toml + src/main.rs, ~90 lines) and the STEP file -->

```toml
[dependencies]
solidcore  = { git = "https://github.com/tangency-labs/solidcore", rev = "513e6fcfcd049d41e9487599880479de4add63f2" }
step-brep  = { git = "https://github.com/tangency-labs/solidcore", rev = "513e6fcfcd049d41e9487599880479de4add63f2" }
step-import = { git = "https://github.com/tangency-labs/solidcore", rev = "513e6fcfcd049d41e9487599880479de4add63f2" }
flate2 = "1"
```

```rust
/// Removes everything with coordinate `axis` > `at`.
fn cut(k: &mut Kernel, b: Brep, axis: usize, at: f64) -> Result<Brep, SolidError> {
    let mut o = [-1000.0; 3];
    let mut s = [2000.0; 3];
    o[axis] = at;
    s[axis] = 1000.0 - at;
    let tool = create_box(k, Point3::new(o[0], o[1], o[2]), s[0], s[1], s[2])?;
    let mut r = boolean_difference(k, b, tool)?;
    r.refresh_cached_topology();
    Ok(r)
}

// body = step_brep::import_with_report(&bytes, &step_import::Limits::default())?.bodies[0]
// volume: mass::integrator::properties::brep_volume(k, &mut copy, 1e-6, &attrs)
```

```sh
cargo run --release -- spool.step.gz y:-30,0,10
# step-brep import: 1 bodies, 0 skipped
# imported part: cut y > -30: volume 248051.975 -> 0.000, faces 92 -> 6
# imported part: cut y > 0: volume 248051.975 -> 0.000, faces 92 -> 7
# imported part: cut y > 10: volume 248051.975 -> 0.000, faces 92 -> 2
```

The import itself looks right. The imported body's volume is 248051.975, and OpenCascade gives
248052.032 for the same file.

## Results

Part volume 248052 mm³. Each row is the part minus the half-space beyond the cut. Reference
values come from OpenCascade 7.8 (`BRepAlgoAPI_Cut` with an equivalent box,
`BRepGProp::VolumeProperties`).

| Cut | solidcore (`513e6fc`) | OpenCascade | |
|---|---|---|---|
| y > −60 | 1581.081 | 1581.081 | ok |
| y > −45 | 11910.307 | 12470.023 | wrong (−4.5%) |
| y > −30 | 0.000 | 52621.938 | **empty** |
| y > −20 | 0.000 | 80739.501 | **empty** |
| y > −10 | 0.000 | 103106.170 | **empty** |
| y > −5 | 0.000 | 114227.334 | **empty** |
| y > 0 | 0.000 | 124026.016 | **empty** |
| y > 5 | 0.000 | 133824.697 | **empty** |
| y > 10 | 0.000 | 144945.862 | **empty** |
| y > 20 | 0.000 | 167312.531 | **empty** |
| y > 30 | 0.000 | 195430.103 | **empty** |
| y > 45 | 0.000 | 235582.009 | **empty** |
| y > 60 | 246470.893 | 246470.951 | ok |
| x > −45 | 11350.617 | 12844.667 | wrong (−11.6%) |
| x > −26 | 65779.852 | 65779.859 | ok |
| x > −20 | 248051.975 (142 faces) | 81409.059 | **nothing removed** |
| x > −10 | 0.000 | 104278.694 | **empty** |
| x > 0 | 0.000 | 124026.016 | **empty** |
| x > 10 | 0.000 | 143773.336 | **empty** |
| x > 20 | 0.000 | 166642.998 | **empty** |
| x > 45 | 0.000 | 235207.366 | **empty** |
| z > 10 | 56533.488 | 56533.488 | ok |
| z > 40 | 81897.393 | 81897.393 | ok |
| z > 70 | 106778.807 | 106778.807 | ok |
| z > 100 | 131689.772 | 131689.772 | ok |
| z > 130 | 609976.135 | 217496.677 | **larger than the input part** |

The part's axis is z. Cuts in x and y meet its cones and tori along conics and quartic curves,
not circles.

Controls that give correct results:

- `create_cylinder` (r 10, h 20) cut at x > 0, y > 0 and x > 3: 3141.593, 3141.593 and
  4323.341, all correct.
- A 14-face part of planes, cylinders and cones, imported the same way and cut in x, y and z:
  all match OpenCascade.

## Expected

Either the correct body, or an `Err`. The main problem is that wrong results come back as `Ok`:
a caller has no way to tell them apart from good ones without an independent check.

## Questions

1. Does SMLib's C++ get these cuts right? We haven't run the difftest harness. If it does, this
   is a port divergence; if not, it's a defect inherited from SMLib.
2. Is there a validity check a caller should run on a boolean result (the `solidcore-ops` check,
   or something in the kernel)? Would it catch these bodies?

## Environment

- Linux x86_64, rustc 1.99.1, `--release`
- solidcore `main` @ `513e6fc` (also reproduced on crates.io `solidcore 1.0.1`)
- Reference: OpenCascade 7.8.1 via `cadquery-ocp==7.8.1.1`

## Context

We found this while prototyping a crate that carries AP242 PMI (dimensions and tolerances on
faces) through solidcore operations, using face attributes. Attribute propagation through the
booleans worked well in every case that produced a valid body. This issue is only about the
boolean results.

---
_Generated by [Claude Code](https://claude.ai/code)_
