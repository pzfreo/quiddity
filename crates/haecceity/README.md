# haecceity

An exact B-rep reader and geometry query library in pure Rust, with no OpenCascade or other
native dependency. It reads, queries and writes B-reps, and does not model them (see
[Scope](#scope)). It is the geometry under [quiddity](https://crates.io/crates/quiddity), which
recognises manufacturing features. Use haecceity directly when you need the geometry itself.

- **STEP:** reads AP203, AP214 and AP242 files (plain or gzipped) into a B-rep `Part`:
  solids, faces, edges and their exact surfaces and curves. A malformed or unsupported file is
  refused with a typed error, never a panic.
- **Geometry:** rays and point classification, face parameter space, volumes, areas,
  centroids and face moments, shared volumes and face coverage, analytic surfaces recovered
  from freeform ones, meshing and hidden-line views.
- **PMI:** reads and writes semantic AP242 PMI (dimensions, geometric tolerances, datums and
  notes) and reports where a file departs from the recommended practices.
- **Writing:** writes a B-rep and its PMI to a new AP242 file.

```toml
[dependencies]
haecceity = "0.5"
```

## Scope

haecceity **reads, queries and writes** B-reps. **It does not model them.** This is a standing
decision, not a gap waiting to be filled.

**In scope:**

- **Reading** STEP (AP203, AP214, AP242) and Fusion `.f3d` into a `Part`, with each face and
  edge traceable to the file entity it came from.
- **Querying** the exact geometry: rays, point classification, volumes and areas, moments,
  coverage, overlap, anchors, analytic surfaces recovered from freeform ones, meshing and
  hidden-line views.
- **Semantic AP242 PMI**: reading, writing, verifying and editing a file's PMI while leaving the
  rest of it as it was.
- **Writing** a `Part` and its PMI as AP242.

**Out of scope, permanently:**

- **Modelling operations**: booleans, fillets and chamfers, offsets and shells, sweeps and lofts,
  healing or stitching that changes the topology it was given.
- **General surface-surface intersection**, the machinery those operations rest on.
- **Construction history, sketches, constraints and parametric editing.**

**What a request for an out-of-scope operation gets:** a product that needs to change geometry
takes it from a modelling kernel and gives the result back to haecceity to read. It is not added
here. The one construction haecceity makes, `sweep::extrude_face`, builds a probe solid for a
volume query. It refuses any edge it cannot sweep exactly, and it is not a modelling API: don't
extend it into one.

**`Part` is the interface.** quiddity, Specify and other users depend on `Part` and the queries
on it, not on how a file became a `Part`. If another reader ever matches haecceity's results on
the test files (faces, volumes, recognition output), it can be put underneath `Part` without
changing what is built on top. Face and edge identity by STEP entity is what makes that
possible: keep every face and edge traceable to its source.

**Why:** most of a kernel's cost and risk is in modelling, and the products built on haecceity
(feature recognition, and PMI from design intent) need none of it. The parts worth owning are
reading real files faithfully, answering geometric questions about them exactly, and AP242 PMI.

## Selected assembly bodies

`step::StepFile` parses a STEP file once. `file.part(index)` reads one part definition
in its own coordinates; `file.placed(&keep)` reads chosen body instances in their original
file coordinates. Supply the whole-file `Part::face_source(face).instance` numbers (or
`PartDefinition::placements[].instances`), not indices into `file.parts`.

For a plate and two coincident copies of a pin, a caller can retain the plate and either
pin. The library reads the caller's selection; it does not decide which bodies coincide.
Every retained face and edge keeps its original STEP entity and instance number, so a
reference to the dropped copy cannot accidentally attach to the retained copy. Local
face, edge, vertex and solid indices, including unresolved-geometry indices, are rebuilt.
`Source::instance` is therefore not necessarily the resulting solid's local index.

Selection order does not reorder the geometry, repeated numbers are ignored, and an empty
selection or any unknown number is refused with `StepError::Unsupported`. Repeated reads
reuse the parsed file and do not change previously returned parts. This API does not make
the initial `StepFile::read` a streaming or geometry-free operation.

## Presentation

The default `presentation` feature exposes `haecceity::presentation`: dimension/tolerance
notation, feature control frames, datum frames, and local 2D text/symbol strokes shared with
drawing clients. Every newly written dimension and tolerance gets a graphical callout;
face textures get ISO 1302 symbols and written part settings share a general-notes block.
Property links retain their semantic requirements, and notes clear all other callouts;
existing foreign PMI/presentation remains untouched on append. Disable default features for
semantic-only dimension/tolerance writing and the legacy datum lettering. The feature is on
by default so Specify's normal writer output carries visible PMI. Public-domain Hershey lettering and explicit
ISO symbols keep files small. Datum leaders start inside their faces and leave normally,
in planes containing the face normal. Text height is 0.5 mm + 1.8% of the largest extent;
frames use short feature-scaled offsets with a deterministic stagger. Multi-face aspects
are composite, and location references use non-datum aspects in both feature configurations. Straight dimensions use two
measured witnesses, extension lines and arrowed lines on shortest-first ladders. See the [AP242 writer structure](../../docs/step-ap242.md#graphical-callouts-137).

Drawing clients can use these public functions without constructing `PartPmi`:

```rust
use haecceity::presentation::{self, gdt, notes, tolerance};
let plex_width = |text: &str, height: f64| text.chars().count() as f64 * height * 0.6;
let frame = gdt::cad_control_frame("position", "0.05", Some("diameter"), Some("M"),
    &["A".into(), "B-C".into()]);
assert_eq!(frame.cells, ["⌖", "⌀0.05Ⓜ", "A", "B-C"]);
let label = presentation::label_with_width("⌀10 H7", &plex_width);
let lines = presentation::label_lines_with_width(&["⌀10 H7", "Ra 10.4"], &plex_width);
assert_eq!(lines.len(), 2);
assert_eq!(gdt::symbol("profile_surface"), "⌓");
assert_eq!(tolerance::HOLE_CLASSES, ["H", "G", "F"]);
```

`gdt::cad_control_frame(name: &str, tolerance: &str, zone: Option<&str>,
material: Option<&str>, datums: &[String]) -> Callout` accepts already formatted cells and
lays the frame out as CAD draws a native TOLERANCE: 2.5 mm text (`gdt::TEXT_HEIGHT`, intended
pen `gdt::TEXT_PEN`), a box twice as tall, each cell its ink plus the text height, with the
text as strokes and arcs from QCAD's public-domain `gdt` and `Standard` fonts. The older
`gdt::control_frame`, which measures with a caller's font, is deprecated. `gdt::symbol(name: &str) ->
&'static str` uses model names (`profile_line`, `profile_surface`, `circular_runout`,
`total_runout`, etc.; see its rustdoc). `gdt::characteristic(name, centre, &mut Callout)`
adds just the characteristic geometry. `label_with_width(value, width)` and
`notes::surface_texture_with_width(&SurfaceTexture, width)` size labels and ISO 1302
symbols with the same convention. `notes::removal_marks(MaterialRemoval, short_height)`
returns strokes and arcs about the symbol tip. Existing `label`, `surface_texture`
and `dimension` use `text_width(value, height)` (Hershey ink width); `tolerance` and `datum`
need no measuring function (their text is CAD-font geometry).
`Callout::letters` is the sheet-font layer (empty in GD&T frames); `runs` also includes
geometrically drawn symbols. `gdt::cad_frame_missing` lists characters a frame cannot draw.

`label_lines_with_width(lines: &[&str], width: &dyn Fn(&str, f64) -> f64) -> Vec<Callout>`
returns one independent callout per input line, in order. Each uses capital height 3,
callback ink width and Hershey ink height (including descenders), with text centred at
half its size and the ink's lower-left corner at the origin. There is no box padding,
line spacing or lane placement. Empty lines have zero ink height; an empty slice returns
no callouts. Pass `&presentation::text_width` for Hershey widths. Other font clients supply
their widths and own vertical metrics when placing/rendering their text.

`notes::texture_parameters` reads the model's stated `Decimal`, preserving trailing zeros
and exponents (apart from STEP's terminal decimal point). Micrometres are implicit in
ISO 1302 parameter notation; other stated units are explicit, e.g. `Ra 0.010400 mm`.
The same text supplies face symbols and general surface-finish notes. Other requirement
notation already uses `Decimal`; `dimension_text` only converts through floating point
when deviation and nominal units differ, to express the band in the nominal's unit.

`tolerance::{HOLE_CLASSES, SHAFT_CLASSES, GRADES}` are ordered arrays for discovery and
refusal messages; `MAX_NOMINAL_MM` is the inclusive 250 mm table limit. These describe the
supported subset, not every ISO 286 class.

Dimension strings use U+2300 `⌀` (Unicode's diameter sign), not U+00F8 `ø` (a letter).
ISO 129-1:2018 §§7.1–7.2 specifies the diameter symbol before the value; ISO 1101:2017
§8.2.2.1.1 uses it for circular/cylindrical tolerance zones. Neither standard prescribes
Unicode code points; this API chooses the semantically correct character. A single-line
asymmetric band is `+upper/lower`, including an unsigned zero (`+0.09/0`). This is the
explicit single-line form in ISO 129-1:2004 §6.2, Figure 29. ISO 129-1:2018 §6.2,
Figures 31–34 specifies stacked deviations (upper above lower) or an ISO code; §4.1.7
requires signs except for zero and decimal alignment when stacked. Drawing clients targeting
the 2018 presentation should stack the two deviations. `limits_term` already returns upper,
newline, lower; slash-separated dimension text is the unambiguous plain-text form, not a
claim that the 2018 edition specifies inline deviations. Sources:
[ISO 129-1:2018](https://www.iso.org/standard/64007.html),
[ISO 129-1:2004 §6.2](https://s538bda2d5ed2d98d.jimcontent.com/download/version/1588561433/module/9041319182/name/ISO%2520129_2004.pdf),
[ISO 1101:2017](https://www.iso.org/standard/66777.html).
Draftwright should replace `ø`/`Sø` with `⌀`/`S⌀` in dimension labels and retain the slash
for inline asymmetric deviations. Geometry may still use an existing font's slashed-ring glyph.

`size_prefix(kind: &SizeKind) -> &'static str` is the exhaustive prefix policy used by
`dimension_text`, including both ends of limits:

| `SizeKind` | Prefix | Basis |
| --- | --- | --- |
| `Diameter` | `⌀` | ISO 129-1:2018 §7.2 |
| `Radius` | `R` | ISO 129-1:2018 §7.3.1 |
| `SphericalDiameter`, `SphericalRadius` | `S⌀`, `SR` | ISO 129-1:2018 §7.4 |
| `ToroidalMinorDiameter`, `ToroidalMajorDiameter`, `ToroidalHighMajorDiameter`, `ToroidalLowMajorDiameter` | `⌀` | Circular diameter, §7.2 |
| `ToroidalMinorRadius`, `ToroidalMajorRadius`, `ToroidalHighMajorRadius`, `ToroidalLowMajorRadius` | `R` | Circular radius, §7.3.1 |
| `CurveLength`, `Thickness`, `Other(_)` | none | General curve/linear/custom dimension |

No torus-specific prefix is defined by these standards; major/minor/high/low remain
semantic distinctions and must be located by the drawing's witnesses or section views.
ISO 1101:2017 §8.4.2's major/minor/pitch identifiers concern screw threads, not toroidal
size prefixes; §8.2.2.1.1's `⌀`/`S⌀` describe tolerance-zone shape, not toroidal sizes.
[ISO 14405-1:2025 §5.2.2](https://standards.iteh.ai/catalog/standards/iso/cd39d8ad-9301-475e-9ead-e1c5ce3bff18/iso-14405-1-2025)
also distinguishes circular/cylindrical `⌀`, spherical `S⌀` and unprefixed parallel-plane
sizes; its Annex E addresses circular longitudinal sections of a torus, without a new
torus prefix. `CurveLength` does not establish a circular arc, so the arc symbol of
ISO 129-1 §7.6 is not automatically added. Thickness keeps ordinary dimension notation:
§7.16.1 permits `t=` with a surface leader, which needs a drawing-client placement choice.

For Draftwright #258 / PR #274: replace `pmi.rs::dimension_label`'s spherical-radius `R`
with `SR`, ensure spherical diameter uses `S⌀`, and handle all eight toroidal variants
with plain `R`/`⌀` as above (no `T`, major/minor or high/low prefix). Prefer delegating
requirement text to `dimension_text` and prefix lookup to `size_prefix`. Use
`notes::texture_parameters` for stated finish values instead of converting through mm;
consume `label_lines_with_width` for individual unpadded lines while retaining lane
layout. Existing drawn-precision decisions remain the drawing client's responsibility.

Enable the optional `f3d` feature for `haecceity::f3d::read_f3d_file`: visible Autodesk
Fusion solid geometry in millimetres, including placements, with no imported PMI.

```rust
let part = haecceity::step::read_step_file(std::path::Path::new("part.step"))?;
for solid in 0..part.solids.len() {
    if let Some((volume, area)) = part.solid_mass(solid) {
        println!("solid {solid}: volume {volume:.3}, area {area:.3}");
    }
}
```

*Haecceity* is "thisness": the particular thing itself, as *quiddity* is "whatness".

Licensed under Apache-2.0. Source and issues: <https://github.com/pzfreo/quiddity>.


Enable `json` for serialization-only `haecceity::json`: semantic
`haecceity.pmi/1`, id assignment and the `haecceity.presentation/1` companion.
It does not embed schemas or compile parsing/import code. Enable `json-import`
(which implies `json`) for schema constants, parsing, validation, `from_value`
and JSON → STEP writing with checked identity restoration. Both are independent
of `presentation`. See [the reference](docs/pmi-json.md), packaged `schema/`,
and the `json_wasm_size` example. Ordinary writer output is unchanged except
for Annex C-conformant gram/milligram definitions.

### Large STEP assemblies: the manifest reader

The default `manifest` feature provides `step::manifest::scan(reader, &ScanOptions::default())`
and serde `quiddity-manifest/1`. Disable default features for WASM consumers that do not need
filesystem manifests; serde, SHA-256, tempfile and mmap dependencies are optional. The source
implements `Read + Seek` solely for replay: the scan uses 8 MiB chunks and one rewind, followed
by one forward pass combining all placement fetches with range hashes. A gzip source is decoded
once per pass, without backwards decoded seeks. Source size and SHA-256 come from the first pass.
`p21::streaming::scan` accepts any `Read` and captures extents only; `scan_structure` opts into
borrowed keywords and references using reusable buffers. Lossless `Document::parse` retains its
original slice scanner and diagnostics.

Ranges are half-open `[start, end)` offsets into the uncompressed source. Each product has
coalesced ranges, entity and byte counts, SHA-256, and the ordered face entity IDs. Edge indices
come from the existing product reader; the manifest carries no edge index map. A normals-only
shell repair can change edge traversal order, so equivalence compares edge counts and sets.
The manifest also carries assemblies, occurrence paths and placements in millimetres, unowned
presentation/other ranges, and shared-geometry refusals. `read_product(&file, "p0")` reads the
header and common/product ranges in one forward pass, checks size and hashes, and uses the
existing part readers with exact ordered face checks. Headers are bounded to 16 MiB before a
manifest-controlled allocation. `verify_source(reader)` explicitly checks the whole source.
The common hash includes the original header bytes followed by the common range bytes.
Incompatible manifests report both reader versions; haecceity records only its own version.
Gzip sources can be scanned, but range reads refuse them.

Only contexts/units reached by product or assembly chains, or their shape representations,
go to `common`. Geometry-only contexts (including per-pcurve contexts) go into their product's
ranges, and a context used by several products is copied into each without unioning them.
Unreached contexts remain unowned. Frames and their point/direction dependencies are also copied
by indexed dependency links, without retaining parsed geometry frames. Only actual assembly
transform frames are fetched into the small placement document after scanning.

`ScanOptions::no_product_hashes` is the explicit size-only fallback. `index_dir` places the
16-byte extent/union entries in a temporary mapped file, cleaned up on success or refusal;
otherwise fixed-size RAM pages avoid reallocating and copying the full index. Dense IDs use a
paged 4-byte slot array, with a sparse hash table for outlying IDs. Forward references use inline
or recycled flat links. Class and adjacency flags share the extent entry (offsets are bounded
to 56 bits, well beyond practical STEP file sizes). File-order links are
needed only for unordered/sparse IDs. Ownership uses inline product tags and pooled shared-owner
links. Retained structure contains assembly records, shape/solid roots and shell face lists,
not faces, bounds, loops or oriented edges; neither index stores entity text. The lexer keeps its
current statement and chunk buffers. There is no automatic RAM/disk threshold.
The independent `instance_limit` defaults to 1,000,000, counting visited definitions and emitted
assembly occurrences with bodies separately (one placement for all bodies of the product); hierarchy depth remains bounded at 64. Per-product
reads retain the existing reader's limits.

Classification uses type names, including every leaf of complex entities, in this order:

- Context/unit boundary types: names ending in `CONTEXT` or `UNIT`, plus `DIMENSIONAL_EXPONENTS`,
  `DERIVED_UNIT_ELEMENT`, `CONVERSION_BASED_UNIT_WITH_OFFSET`,
  `MEASURE_WITH_UNIT`, `UNCERTAINTY_MEASURE_WITH_UNIT`, `APPLICATION_PROTOCOL_DEFINITION`.
- Assembly: `PRODUCT`, `PRODUCT_DEFINITION` (also `_WITH_ASSOCIATED_DOCUMENTS`),
  `PRODUCT_DEFINITION_FORMATION` (also `_WITH_SPECIFIED_SOURCE`), `PRODUCT_DEFINITION_SHAPE`,
  `SHAPE_DEFINITION_REPRESENTATION`, `NEXT_ASSEMBLY_USAGE_OCCURRENCE`,
  `CONTEXT_DEPENDENT_SHAPE_REPRESENTATION`, `ITEM_DEFINED_TRANSFORMATION`,
  `REPRESENTATION_RELATIONSHIP`, `REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION`,
  `AXIS2_PLACEMENT_3D`, `AXIS2_PLACEMENT_2D`. Placement frames and their point/direction
  dependencies are copied without joining products through a shared standard frame.
- Presentation/PMI: prefixes `STYLED_`, `COLOUR`, `COLOR`, `PRESENTATION_`, `SURFACE_STYLE`,
  `CURVE_STYLE`, `FILL_AREA_STYLE`, `POINT_STYLE`, `TEXT_STYLE`, `TEXT_LITERAL`, `COMPOSITE_TEXT`,
  `SURFACE_CONDITION`, `SURFACE_TEXTURE`, `ANNOTATION`, `DRAUGHTING`,
  `CAMERA`, `LIGHT_SOURCE`, `GEOMETRIC_TOLERANCE`, `DATUM`, `DIMENSIONAL_`, `DIMENSION_`,
  `SHAPE_ASPECT`, `TESSELLATED_ANNOTATION`; names containing `TOLERANCE`, `SHAPE_ASPECT`,
  `STYLE` or `SYMBOL`, or ending in `_FONT`;
  and `MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION`, `OVER_RIDING_STYLED_ITEM`,
  `INVISIBILITY`, `PRE_DEFINED_COLOUR`, `PRE_DEFINED_CURVE_FONT`, `GEOMETRIC_ITEM_SPECIFIC_USAGE`,
  `ITEM_IDENTIFIED_REPRESENTATION_USAGE`, `COMMON_DATUM`, `LIMITS_AND_FITS`.
- A plain `SHAPE_REPRESENTATION_RELATIONSHIP` (including its complex inherited leaves) is
  geometry; a relationship with a transformation is assembly.
- Geometry: everything else. Only references between geometry entities participate in
  union-find. A component claimed by multiple products records a refusal with a shared
  entity ID; callers use their existing whole-file fallback for those products.

The CLI and Python manifest interfaces are separate work.
