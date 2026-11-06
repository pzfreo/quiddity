# haecceity, nacre and solidcore compared (2026-11-06)

Two Rust B-rep kernels appeared in 2026 alongside haecceity:

- **nacre**: [elgar328/nacre](https://github.com/elgar328/nacre), crate `nacre` 0.0.1 (workspace 0.0.2-dev), MIT/Apache-2.0
- **solidcore**: [tangency-labs/solidcore](https://github.com/tangency-labs/solidcore), crate `solidcore` 1.0.1 (its README still says the API is 0.0.x and experimental), Apache-2.0

This note compares them with haecceity 0.4.7. The figures come from the default branch of each
repository on 2026-11-06 (nacre at 2026-11-04, solidcore at 2026-11-05).

## In one line each

| | Purpose | How it handles numbers |
|---|---|---|
| **haecceity** | Reads STEP files and answers questions about them: rays, classification, volumes, coverage, analytic recovery, PMI. It also writes AP242. | `f64`, with edge bands tuned on a test corpus to match what OpenCascade accepts (`OVERSHOOT_BANDS` and similar). |
| **nacre** | Builds models from code: sketch, extrude, boolean. Exact geometry is the source of truth. | Exact rationals and `a+b√c` values, with an `f64` filter that falls back to exact arithmetic. When a sign can't be proved, the operation is refused with a named reason. |
| **solidcore** | A full modelling kernel, ported from NVIDIA SMLib (usd-brep 1.0.1) and then extended. | A tolerant B-rep. Each vertex, edge and face carries its own tolerance, and constructions come with certificates. |

These are three different kinds of kernel. haecceity is a **reader and interrogator**. nacre and
solidcore are **modellers**. haecceity has none of their modelling operations: no booleans,
fillets or offsets. Its `sweep.rs` only extrudes a planar face bounded by lines and arcs. Neither of
the others comes close to haecceity on STEP input or PMI.

## Capability matrix

| | haecceity | nacre | solidcore |
|---|---|---|---|
| STEP import | AP203, AP214 and AP242, plain or gzip. Assembly instance selection, typed refusals, EXPRESS rule checks | **None** | AP203/AP214 B-rep subset, with healing and stitching on import (`step-import` and `step-brep`, `publish = false`) |
| STEP export | AP242 B-rep plus semantic PMI and graphical callouts | AP242 shape only, planes and cylinders (via `brep-to-step`) | `step-export` (unpublished) |
| PMI (dimensions, GD&T, datums, notes) | Read and write, plus a JSON interchange | Out of scope by design | Out of scope |
| Surfaces | Plane, cylinder, cone, sphere, torus; NURBS as `Freeform`; recovers analytic surfaces from freeform | Plane and cylinder. Spheres and cones are in progress (M6); general NURBS is a research track (M7) | All analytic surfaces, NURBS, and procedural surfaces (sweeps, rolling-ball blends) |
| Booleans | No | Planar solids are done (M5). Cylinder × plane works only with the plane perpendicular or parallel to the axis | Yes, through SSI, with an SSI corpus score |
| Fillets, chamfers, offset, shell | No | No | Yes |
| Point classification, rays | Yes, a core strength | Internal to the booleans | Yes (`query`) |
| Mass properties | Volume, area, centroid, face moments | Volume, area, centroid, bounding box | Yes |
| Tessellation and hidden lines | Mesh and HLR | Tessellation with origin tags | `solidcore-tess` |
| Healing | No (OpenCascade's seam insertion is a known gap, see `opencascade.md`) | Not applicable: no import | Yes (`heal`, `stitch`) |
| WASM | Yes (the `json_wasm_size` example) | Yes, with a browser playground | Yes, but needs a larger stack (no guard page) |
| Panic policy | Clippy warns on unwrap/expect/panic/unreachable; typed errors | Named refusals | Clippy *denies* unwrap/expect/panic/indexing |
| Validation oracle | Python and OpenCascade parity on a corpus | OCCT used only as a dev-time oracle (`nacre-oracle`) | Differential tests against the C++ SMLib to 1e-9 |

## Size and dependencies

| | Rust lines | Runtime dependencies |
|---|---|---|
| haecceity | ~106k, of which 46k is the generated `express_table.rs` | `step-io`, `i_overlay`, `spade`, `flate2`; optional `serde`, `cadmpeg-*` |
| nacre | ~170k, of which ~116k is in `nacre-ops`, much of it tests | `num-rational`, `num-bigint`, `astro-float`, `geometry-predicates`, optional `rayon` |
| solidcore | ~1.5M in the workspace: ~890k in the kernel crate, ~420k in difftest, ~120k in the FFI | `serde`, `thiserror` only |

solidcore's line count reflects a mechanical, unit-by-unit port of the C++ that keeps the upstream
naming. Its progress site lists units as green, partial or deferred. "Every module has a Rust
counterpart" does not mean every path is wired up.

## Maturity and community (2026-11-06)

- **nacre**: one author, started July 2026, 7 stars. Design docs are in Korean. It is deliberately
  narrow and grows one milestone at a time. Its philosophy is the most rigorous of the three:
  exactness first, refuse rather than approximate.
- **solidcore**: an organisation-run project with an open contributor and AI-agent workflow,
  published October 2026, 331 stars, and very active (it was pushed to today). The first consumer
  is SolidCraft.
- **haecceity**: in production under quiddity, with a CLI, Python wheels and a STEP and PMI test
  corpus. Its behaviour is pinned to what OpenCascade does on real files.

## What this means for quiddity

1. **Neither can replace haecceity as the reader.** quiddity needs to read arbitrary vendor STEP
   files, including freeform surfaces and the faults OpenCascade quietly heals, and AP242 PMI.
   nacre cannot import at all. Its exact model also doesn't fit `f64` file data: a file's
   coordinates are binary fractions, and its B-spline approximations of neighbouring faces leave
   cracks a few tenths of a millimetre wide. An exact kernel would have to refuse those files or
   heal them first. solidcore's importer covers B-rep geometry only (no PMI), and it isn't
   published as a crate.

2. **solidcore is the candidate for operations haecceity lacks.** The clearest gap is `sweep.rs`,
   whose probe solids give up on freeform edges and on arcs about an axis across the sweep. A real
   extrude plus boolean would close it. A second is OpenCascade-style seam insertion on import,
   which `docs/opencascade.md` lists as a known limitation and which solidcore's `heal` covers.
   Costs: a very large build, a different B-rep data model (haecceity's `Part` would need a
   converter), and an API its own README calls experimental. If this is tried, keep it behind an
   optional feature and use it only to build probe solids, not as the main reader.

3. **solidcore as a second oracle.** Its `step-import` is a clean-room AP203/AP214 reader with no
   dependencies. Running it on quiddity's corpus as a differential check (face, edge and loop
   counts, volumes) would give a parity source independent of OpenCascade, for little effort.

4. **Ideas worth taking from nacre, not code.** haecceity's corpus-tuned constants (for example
   `OVERSHOOT_BANDS` = 4, where the corpus needs at least 1.8 and breaks at 70) are where it is
   weakest. nacre's pattern of an `f64` filter, then exact escalation, then "proved or undecided",
   applies to sign decisions where both inputs are known exactly, such as the side of an analytic
   plane or cylinder a point lies on. It can't help where the uncertainty comes from the file
   itself. A cheap first step would be adaptive predicates, nacre's `geometry-predicates`
   dependency, in haecceity's planar orientation tests.

5. **Licensing is not an obstacle.** nacre is MIT/Apache-2.0 and solidcore is Apache-2.0 with an
   NVIDIA attribution NOTICE. Both are compatible with haecceity's Apache-2.0. Taking any
   solidcore code means carrying its NOTICE and ATTRIBUTION.

## Bottom line

haecceity is the most capable of the three at *reading and understanding* real STEP files with
PMI, which is quiddity's job. solidcore is far ahead at *making and changing* solids. nacre is the
most principled about exactness, but it is the earliest of the three and covers the least
geometry. They complement haecceity rather than compete with it. The one with near-term practical
value is solidcore, as an optional modelling and healing backend and as a differential oracle.
