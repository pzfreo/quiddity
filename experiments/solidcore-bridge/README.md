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

1. **Silent wrong booleans.** On `every_kind`, a cut plane through the part's axis
   (x = 0 or y = 0) returns an empty body without an error. Those planes contain the seams of
   many revolved faces. A bridge must check results, for example volume against an expectation
   or solidcore's own validity check, and refuse them. Before reporting it upstream, check
   whether it still happens on solidcore's main branch, which is about 4,000 commits ahead of
   1.0.1.
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
