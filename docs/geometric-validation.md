# Geometric validation accuracy

`haecceity::gvp::read` reads a Part 21 `Document` and returns the file's geometric
validation properties (reference A). Each representation retains its property id,
original target, product definition, occurrence, and explicit shape-aspect geometry.
Values are normalized to mm³, mm² and mm. Length, area and volume units are resolved
independently, including SI prefixes, conversion-based and derived units. Unsupported
or ambiguous units are errors. PMI validation properties and curve/surface centroids
are not material properties. A shape aspect is never implicitly its entire product.
Definition properties are in definition coordinates; an occurrence retains its own
identity and representation coordinates.

Build and run the local comparison (no downloads):

```sh
cargo build -p haecceity -j 6 --release --example gvp
"$OCP_PY" tools/gvp.py --jobs 6 --rust target/release/examples/gvp \
  --json "$SCRATCH_OUT/gvp-evidence.json" \
  "$QUIDDITY_CORPUS" "$DW_EXAMPLES/steps/nist" tests/fixtures \
  > "$SCRATCH_OUT/gvp-table.txt"
```

To render an already captured measurement JSON again, use `--measurements FILE`
in place of `--rust` and paths. This retains the supplied Rust/A observations and
recomputes OCCT; it does not claim to remeasure ours.

`OCP_PY` is the Python executable in a venv with `cadquery-ocp-novtk`. The Python
reference uses OCCT `BRepGProp.VolumeProperties` and `SurfaceProperties`, adaptive
Gauss integration with requested relative tolerance 1e-9, exact surfaces, no
triangulation. It records the OCP version. XCAF's unique part traversal and
`TopExp.MapShapes` face order match `StepFile.parts` / `StepFile.part`'s documented
numbering contract; part and face count mismatches are reported rather than zipped
silently. OCCT sometimes gives independent sheets their own free XCAF roots; these
are regrouped only when the STEP text itself declares exactly one product definition.
The parser keeps `surface area measure` and `wetted area measure` apart as `AreaKind`.
For a product with both solids and independent sheets, wetted area excludes the sheets
and `independent surface area measure` is the sheets alone. Plain surface area follows
the file's schema: of the NIST files, the AP203 exports (CTC-02 rc, CTC-05 rd) include
the sheets and the AP242 exports (CTC-04, FTC-07) exclude them, with the same
representation structure, each matching its reference to better than 1e-5 while the
other scope is off by the sheets' 0.004–0.19%. Any other schema has no recorded
convention, so such an area is unavailable (a failure) rather than guessed. The scope
used is recorded as `area_scope`, and the table counts any disagreement with OCCT's.
Volume
and material centroid use solids only. A sheet-only part reports area, zero volume and no material centroid. Legacy
AP203 aspects explicitly described as "surface data associated with …" select the
independent sheets of that product when their representation lists only boundary
curves. This scope decision is recorded in the detailed JSON. Explicit geometry
selections are otherwise followed without promoting an aspect to its product. Values for distinct definitions use their
own coordinates, so repeated placements are not counted as additional definitions.
Occurrence and assembly properties that cannot be attached are printed explicitly.

Ours uses the existing `Part::solid_mass`, `shell_mass`, and `volume_centroid`:
Green's theorem boundary integration of exact curves and surfaces, adaptive
Gauss–Kronrod with relative target 1e-9 (and cancellation floors), and 16-point
Gauss–Legendre interior panels. This is quadrature, not tessellation. The material
centroid uses the existing zero-first-moment fixed point for slightly nonclosed
shells. Thus two kernels can disagree slightly on imperfect shells even with
converged integration. Missing face integrals remain unavailable, never zero.

All bands are fixed at 0.001: `abs(ours-reference)/abs(reference)` for volume and
area, and Euclidean centroid distance divided by the part's bounding-box diagonal.
The centroid band is scale invariant and gives a length analogue of the 0.1% bands.
For zero reference scalars, only exact zero passes. Counts distinguish unavailable
values, absent references, and numerical failures. Each GVP representation is a
separate check, including duplicate product/aspect representations. Two kinds of B difference
are known OCCT reference problems and counted as `KNOWN-REFERENCE`, not as failures of
ours: a part OCCT reads without a SOLID (it gives no volume or material centroid), and
`turned_body_disc.step`, where ours meets the exact value of the generated profile and
OCCT does not. The second applies only while ours stays within the band of the exact
value. The A–B rows
identify disagreements between independent references; they do not decide correctness
without comparing ours as well.

CI runs the `gvp` test binary in its nextest shards; locally, `cargo test -p haecceity --release -j 6 --test gvp`. It reads an explicit list of committed files
with distinct properties and pins each file's number of checks per quantity, the
corpus total, and the area scope of every product area on a part with independent
sheets, so a dropped property or a changed scope fails. Committed fixtures always run; corpus checks use
`QUIDDITY_CORPUS` and honor `QUIDDITY_CORPUS_REQUIRED`, like the existing suite.
No OCCT installation is required in CI. `tests/fixtures/gvp-occt.json` contains
independent evidence for a known wrong reference; the test requires both the
recorded disagreement and ours–OCCT agreement, so it cannot hide a geometry failure. Any known wrong-reference exemption must
contain independently captured OCCT evidence and still assert ours against that
reference with the same bands. Genuine haecceity failures must remain visible and
be fixed separately, without changing these bands.

Inputs retain their own licences: repository `NOTICE` and
`tests/fixtures/ap242/README.md`, the corpus `README.md` /
`THIRD_PARTY_NOTICES.md`, and dw-examples `steps/nist/README.md` record NIST sources
and unrestricted / US-government-work terms. No additional STEP files are needed.

Potential additional inputs (not fetched): NIST MBE PMI AP242 edition/revision files,
including `NIST-PMI-STEP-Files.zip` in usnistgov/SFA (US-government works per the
local notices), and CAx-IF public test-suite models at `cax-if.org` (check each
contributor's redistribution terms before vendoring; public access alone is not a
licence). Selection and any new redistribution are left to the maintainer.
