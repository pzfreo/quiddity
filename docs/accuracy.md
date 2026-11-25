# How accurate are quiddity's volume, surface area and centre of gravity?

**Summary.** On 86,410 test parts with an independent reference, quiddity's result is within 10⁻⁷ mm of the reference on 99.71% and within 10⁻³ mm on 99.92%. Where it detects that a number can't be trusted it says "unavailable"; the misses in the table include cases it did not detect.

## What the STEP kernel is, and is not

quiddity measures through *haecceity* [1], its STEP kernel: an exact boundary-representation (B-rep) reader and geometry-query library in Rust, with no OpenCascade or other native dependency. It reads STEP files (AP203, AP214 and AP242) and keeps each face and edge as the exact analytic or spline surface or curve the file describes, not as triangles. From that it answers geometric questions: volume, surface area, centre of gravity, moments of inertia, point classification, ray hits, sections, meshes and hidden-line views. It also reads and writes AP242 product and manufacturing information (PMI).

It is **not** a modelling kernel. It has no booleans, fillets, chamfers, offsets, shells, sweeps or lofts, no sketches or construction history, and it does not repair or restitch geometry in ways that change a file's topology. Modelling kernels such as springrep [2] and OpenCascade [4] build the solids; haecceity reads them. It is also not a mesh tool: every number on this page comes from the exact surfaces, not from tessellations, and where it detects that a face or its boundary can't be trusted, haecceity refuses rather than guesses. Accuracy here therefore means how closely the volume, area and centre of gravity read from a STEP file agree with what an independent tool measures from the same file.

## Measure of difference

For each solid, ε is the largest of three lengths in mm: the volume difference divided by the surface area, ΔV/A; the area difference divided by √A, ΔA/√A; and the distance between the two centres of gravity, |Δc|. A volume error ΔV is the same as the whole surface being offset by ΔV/A, so ε reads as a length. For scale: ε = 10⁻⁷ mm is about the width of an atom, and ε = 10⁻³ mm is about 1/70 of the width of a hair.

## Results, measured against independent references

| Source | Parts | Reference | ε ≤ 10⁻⁷ mm | % | 10⁻⁷ < ε ≤ 10⁻³ mm | % | ε > 10⁻³ mm or no answer | % | The misses |
|---|---:|---|---:|---:|---:|---:|---:|---:|---|
| Exact formulas (plain shapes and closed-form intersections) | 705 | mathematics | 694 | 98.44% | 11 | 1.56% | 0 | 0.00% | 11 nearly touching boxes, ε ≤ 2·10⁻⁶ mm; springrep's value is identical to ours |
| brep-conformance [3] test solids | 3,210 | springrep [2], reading its own STEP files back | 3,200 | 99.69% | 9 | 0.28% | 1 | 0.03% | 9 spline slabs and lofts, ε ≤ 5·10⁻⁶ mm; 1 loft that springrep reloads wrongly (ours matches an exact integration of the surface) |
| BenDFM [5] bent sheet metal | 20,000 | volumes recorded by the dataset's authors | 20,000 | 100.00% | 0 | 0.00% | 0 | 0.00% | none |
| MFInstSeg [6] machined parts | 62,495 | OpenCascade [4], accurate method | 62,265 | 99.63% | 159 | 0.25% | 71 | 0.11% | 52 with ε > 10⁻³ mm and 19 with no answer: zero-volume, reversed or degenerate shells |
| **Total** | **86,410** | | **86,159** | **99.71%** | **179** | **0.21%** | **72** | **0.08%** | |

Two sets use a different measure, so they are separate:

| Check | Count | Measure | Within | % | Misses | % | What the misses are |
|---|---:|---|---:|---:|---:|---:|---|
| NIST [7] values stored by CAD makers [8] | 73 checks (12 files) | relative error ≤ 10⁻³ | 70 | 95.89% | 3 | 4.11% | all one file whose stored values are zero |
| Moments of inertia against springrep [2] | 3,155 single-solid | relative error ≤ 10⁻⁷ | 3,154 | 99.97% | 1 | 0.03% | the same loft; the 54 multi-solid files also return tensors |

The 0.5.1 benchmark ([docs/benchmarks/v0.5.1-measurements.md](benchmarks/v0.5.1-measurements.md)) ran 18 NIST files and 106 checks, 103 within 0.1%. This run covers the 12 files that store values among those we ran; the same file, STC-06, accounts for the misses in both.

## When tools disagree

No tool was always right, quiddity included.

- **quiddity:** four faults found and fixed. Spherical faces were refused or wrong (#351). A recent change broke 74 machined parts (#355). Smooth spline fillets were off by up to 3·10⁻⁷ mm (#357). Inertia was wrong on shallow domes (#359).
- **springrep:** it reloads two smooth lofts wrongly (springrep issue #99). A direct integration of the exact surface agrees with quiddity.
- **OpenCascade:** its default volume method is wrong on spline shapes, and even its accurate method (`BRepGProp::VolumePropertiesGK`) misreads 97 test solids, giving them zero area.

Where a tool could show its working by exact calculation, the table says who was right.

## When to double-check a number

- **Zero-volume or reversed (inside-out) shells.** You get 0, or a negative volume, as the file states.
- **Spherical domes with radius far larger than the part.** The centre of gravity can be about 2·10⁻⁴ mm off, and inertia is refused.
- **A few spline faces** (height-field slabs): the area can be 5·10⁻⁶ mm off.
- **Files with small gaps between faces**, a few thousandths of a mm: other tools may differ slightly from us.

If you find a wrong number, please open an issue.

## References

1. haecceity, the STEP kernel in the quiddity repository (`crates/haecceity`): https://github.com/pzfreo/quiddity, https://crates.io/crates/haecceity. Measured in November 2026 on the code of main `08f6175e`.
2. springrep, an exact B-rep modelling kernel in Rust (MIT OR Apache-2.0), versions 2.1.0 and 2.1.1: https://github.com/springcraft-ai/springrep, https://crates.io/crates/springrep.
3. brep-conformance 2.0.1, a conformance suite for exact B-rep kernels (MIT), Springcraft: https://springcraft-ai.github.io/brep-conformance/, https://github.com/springcraft-ai/brep-conformance.
4. Open CASCADE Technology (OCCT), versions 7.8.1 and 8.0.1 through the CadQuery OCP bindings, using `BRepGProp::VolumePropertiesGK` and `BRepGProp::SurfaceProperties`: https://dev.opencascade.org, https://github.com/CadQuery/OCP (LGPL-2.1 with exception).
5. BenDFM: Ballegeer and Benoit, arXiv:2603.13102; code (MIT) https://github.com/UGent-CVAMO/bendfm; data (GPL-3.0) https://zenodo.org/records/18622958.
6. MFInstSeg, from AAGNet: Wu et al., Robotics and Computer-Integrated Manufacturing, https://doi.org/10.1016/j.rcim.2023.102661; dataset links at https://github.com/whjdark/AAGNet. The dataset's own licence is not stated there.
7. NIST MBE PMI test cases: https://www.nist.gov/el/systems-integration-division-73400/mbe-pmi-validation-and-conformance-testing.
8. CAx-IF, Recommended Practices for Geometric and Assembly Validation Properties, v4.6: https://www.mbx-if.org/documents/rec_prac_gvp_v46.pdf.
