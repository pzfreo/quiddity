# Phase 0: solidcore bridge prototype (2026-11-06)

A throwaway prototype for phase 0 of [the AP242 strategy](../../docs/strategy-ap242-solidcore.md).
It answers one question: **can STEP face identity, and with it the PMI, survive a solidcore
modelling operation on a real AP242 file, using only the published crate?**

**Answer: yes.** It works on `solidcore = "=1.0.1"` from crates.io. There are two caveats, both
below: a trick needed for merged faces, and silent boolean failures on one part.

This is its own Cargo workspace, not a member of quiddity's, so quiddity's build and CI never
see it.

## Running it

```sh
cargo build --release
./target/release/tags copy hole|slot|merge                 # step A: tags through booleans
./target/release/bridge FILE.step[.gz] [--axis x|y|z] [--keep F]   # step B: a real file
./target/release/survey FILES...                            # which fixtures it can handle
python occt_reference.py FILE.step[.gz] AXIS CUT            # OpenCascade's answer (needs OCP)
```

`occt_reference.py` needs OpenCascade's Python bindings, for example
`pip install cadquery-ocp==7.8.1.1` in a venv.

## Step A: do face attributes survive solidcore booleans? (`src/bin/tags.rs`)

Each face of two primitives is tagged with a long attribute (its "STEP id"), then a boolean is
run.

| Case | What happened to the tags |
|---|---|
| Box minus a through cylinder | Every box face keeps its tag. The new hole wall carries the *cylinder's* side-face tag, so a new face's origin is known. |
| Box minus a slot across its top | The top face splits in two and **both pieces keep its tag**. Removed tool faces are gone. |
| Box plus a flush box (coplanar faces merge) | With one shared attribute id, **the merged face keeps one operand's tag and silently drops the other's**. |

The cause of the merge loss is solidcore's default merge/coincident handler
(`AttributeStore::merge`, ported from SMLib's `SmAttribute::Merge`). It copies an attribute onto
the surviving face only if that face has no attribute with the same id. **The fix is one
attribute id per STEP entity** (`STEP_BASE + n` holds `#n`). With that, the merged face carries
both tags (`[100, 900]`). No custom handler or unpublished API is needed.

## Step B: a real AP242 file (`src/bin/bridge.rs`)

1. haecceity reads the file: `read_part` for the B-rep and `pmi::read` for the semantic PMI.
   PMI features name faces by haecceity face index, and `Part::face_source` gives each face's
   STEP entity.
2. Each face is rebuilt in solidcore. This follows solidcore's own stitched STEP build
   (`step-brep`'s `build_stitched`) using only public 1.0.1 APIs: `p7::make_face_with_curves`
   per face, `api_stitch_into_solid`, `split_at_seams` and UV trims. It supports planes,
   cylinders, cones and tori bounded by lines and circles.
3. Each face is tagged with its STEP entity, including the pieces `split_at_seams` makes (it
   returns them).
4. An oversized box removes everything beyond a cut plane.
5. For each PMI feature made of faces, the report compares each face's piece count and area
   before and after the cut: **kept**, **split**, **trimmed** (area reduced), **partial** (some
   faces gone) or **orphaned** (all its faces gone).

### Results

| File | Rebuild | Cut | solidcore volume | OpenCascade volume | PMI features |
|---|---|---|---|---|---|
| `write/thumbwheel_notes_add` (14 faces: planes, cylinders, cones; 8 face features) | closed solid, 345.898 = haecceity | x > 11.75 | 263.889 | 263.889 | 7 kept, 1 trimmed |
| | | y > 0 | 172.949 | 172.949 | 8 trimmed; every face's area exactly halved |
| | | z > −2 | 43.233 | 43.233 | 5 trimmed, 3 orphaned |
| | | x > 2.35 | 152.976 | not run | 2 kept, 3 trimmed, 3 orphaned |
| `write/every_kind` (92 faces including 4 tori; 10 face features) | closed solid, 248051.610 vs haecceity 248052.032 (0.0002%) | z > 70 | 106778.709 | 106778.807 | 10 orphaned |
| | | z > 112 | 181716.351 | 181716.525 | 10 orphaned |
| | | x > −26 | 65779.850 | 65779.859 | 6 orphaned, 4 trimmed |
| | | **y > 0** | **0.000 (8 faces)** | 124026.016 | **wrong: solidcore returned an empty body with no error** |
| | | **x > 0** | **0.000 (6 faces)** | 124026.016 | **wrong, as above** |

`spool_fits_intent` and `datum_targets` are the same 92-face part with different PMI, and
behave the same.

## What this establishes

- **Phase 0 gate passed.** Face identity survives solidcore booleans on real AP242 files,
  through the published 1.0.1 crate. Splits pass the tag to every piece. Merges keep both tags
  with per-entity attribute ids. New faces carry the tool face's tag. Faces that are cut away
  are detected, so the PMI that was on them can be reported as orphaned.
- **Our geometry can be rebuilt in solidcore from haecceity's `Part`.** Both test parts rebuild
  as closed solids with matching volume. The converter is about 200 lines because it follows
  solidcore's own build recipe.
- **The PMI status logic** (kept / trimmed / split / partial / orphaned) is the start of the
  reconcile rules that phase 4 needs.

## Caveats and open issues

1. **Silent wrong booleans, still present on solidcore main.** See
   [the follow-up below](#follow-up-solidcore-main-2026-11-06). It is not limited to cuts
   through the axis.
2. **One boolean error.** thumbwheel z > −1.6 (`--keep 0.2`) fails with `InvalidInput` in
   `Line::create_line_segment` ("start and end points coincide"). That's a clean refusal, but
   it's still a kernel limitation.
3. **The published crate lags.** 1.0.1 is about 4,000 commits behind solidcore's main. The
   APIs used here exist in both, but signatures may drift. Pin exactly.
4. **Not yet covered:** B-spline surfaces and curves, ellipses, spheres, vertex loops, edges
   (only faces are tagged), multi-solid parts, and writing AP242 back out (phase 4).
5. **The converter uses low-level APIs** (`topology::brep::p7`, `ArenaNotifyState`). These are
   public but read as internal. A friendlier construction API, or solidcore publishing its
   importer, would be the better long-term base.

## Follow-up: solidcore main (2026-11-06)

The prototype builds unchanged against solidcore's main branch (`513e6fc`, about 4,000 commits
after 1.0.1). Main improves the rebuild: `every_kind` now comes out at 248052.029 against
haecceity's 248052.032, and z > 70 matches OpenCascade exactly at 106778.807. **The silent
failures remain.**

To rule out our converter, [`upstream-repro/`](upstream-repro) uses only solidcore: its own
unpublished STEP importer (`step_brep::import_with_report`) and `boolean_difference` with a
2000-unit box. It depends on solidcore by git, pinned to `513e6fc`. The same failures occur, so
**the converter is not the cause**. A plain cylinder cut through its axis is correct, so the
problem needs real part geometry.

`every_kind` (92 faces) imported by solidcore, minus the half-space beyond each cut, compared
with OpenCascade 7.8 (`occt_reference.py`):

| Cut | solidcore main | OpenCascade | |
|---|---|---|---|
| y > −60 | 1581.081 | 1581.081 | ok |
| y > −45 | 11910.307 | 12470.023 | **wrong (−4.5%)** |
| y > −30, −20, −10, −5, 5, 10, 20, 30, 45 | **0.000** (2–8 faces) | 52621.9 … 235582.0 | **wrong: empty** (9 cuts) |
| y > 0, 0.001, 1 | **0.000** | 124026.0 (y > 0) | **wrong: empty** |
| y > 60 | 246470.893 | 246470.951 | ok |
| x > −45 | 11350.617 | 12844.667 | **wrong (−11.6%)** |
| x > −26 | 65779.852 | 65779.859 | ok |
| x > −20 | 248051.975 (*nothing removed*, 142 faces) | 81409.059 | **wrong** |
| x > −10, 0, 10, 20, 45 | **0.000** | 104278.7 … 235207.4 | **wrong: empty** |
| z > 10, 40, 70, 100 | matches | matches | ok |
| z > 130 | **609976.135** (*more than the whole part*) | 217496.677 | **wrong** |

**Every one of these returned `Ok`.** Of 26 cuts, 18 are wrong. Cuts across the part's axis
(z) are mostly right; cuts parallel to it (x, y) are mostly wrong. Those cuts meet the part's
cones, cylinders and tori along conics and quartics rather than circles. The 14-face thumbwheel
cuts correctly in x, y and z.

What this means for the plan:

- **Identity tracking is not the problem; boolean correctness is.** Tags behaved correctly in
  every cut that produced a body.
- **A bridge must never trust a boolean result.** At minimum it should check the result for
  validity, check that the volume lies between 0 and the original, and preferably compare the
  volume against a cheap independent estimate (haecceity's own volume probes could provide one).
  If a check fails, it refuses rather than reporting PMI as orphaned.
- **This is worth reporting to solidcore**, with `upstream-repro/` as the reproduction and
  `upstream-repro/ISSUE-DRAFT.md` as the issue text (not yet posted). Their difftest compares
  against SMLib, so a first question is whether SMLib's C++ gets these cuts right too.
- **The shareable test file** is `upstream-repro/flanged_spool.step`: `every_kind`'s geometry
  only, re-exported as AP214 by OpenCascade 7.8 (`to_ap214.py`), with no PMI, no Specify data
  and no corpus names. It gives the same solidcore and OpenCascade results as the original.
  `src/bin/strip.rs` (haecceity's `Mode::Remove`) was tried first, but it leaves the header's
  PMI-practice line, the AP242 schema and Specify's `pmi-assist answers` property in the file.
  The base geometry comes from the cadgenbench corpus (`flanged_spool_132`): check that
  dataset's terms before posting.

Run the repro (needs network for the git dependency; the first build takes about 5 minutes):

```sh
cd upstream-repro
cargo run --release -- flanged_spool.step y:-30,0,30
```
