# Consuming unified recess geometry

The 0.4.15 cutover replaces specialised pocket, recess, channel and passage outputs with
`SectionRecess`. Use `build_section_recess_document(part).to_dict()` for JSON schema 3, or
`RecognitionResult.section_recesses` when already running the aggregate. The builder runs
raw/caller-coordinate recognition once; it does not automatically frame the input.

## Geometry and classification

| Proved geometry | Profile | Run ends | Feature kind |
| --- | --- | --- | --- |
| Blind constant-section pocket | Closed | One capped | `pocket` |
| Through passage | Closed | Both open | `passage` |
| Edge-open recess or blind slot | Physical open chain | One capped | `edge_open_recess` |
| Three-support channel, including bounded partial supports | Physical U chain | Both open | `channel` |

Corner notches have a two-wall L chain and the `polygonal` section classification. Their opening
is not a diagonal wall or a floor outline. Channels have three physical support segments; the
fourth side is explicitly absent. No bounding box is promoted to a closed pocket.

A section point `(u, v)` at run coordinate `s` reconstructs as
`frame.origin + u * frame.u + v * frame.v + s * frame.run`.
Each end separates `condition` from its explicitly tagged `surface`.
For `surface.type == "plane"`, `surface.gradient` gives additional run displacement
relative to the corresponding centroid `run_interval` value.

For `surface.type == "cylinder"`, normalize its in-plane `axis_direction = (a,b)`.
With `axis_point = (cx,cy,cz)`, let `q = -b*(u-cx) + a*(v-cy)`.
The end coordinate is `cz ± sqrt(radius²-q²)`, using the explicit `positive` or
`negative` branch. The existing closed polygon supplies the domain. Radius and
axis point are in millimetres; the direction is dimensionless. The corresponding
run-interval value is the rounded intersection at `(u,v)=(0,0)`, not an envelope.
The producer proves valid branch and positive separation over the complete profile.

Schema 3 is an explicit breaking change: schema-2 `end.gradient` moves to
`end.surface.gradient`, and consumers must dispatch on the surface type. Do not
silently flatten an unfamiliar cylinder to the centroid plane. A pocket in
cylindrical stock may have a 6 × 24 footprint with 8–12 mm physical depth; label
depth as local, maximum or centroid depth, never as uniform. This requires no
build123d-specific value or consumer-side recognition. See ADR 0020.

A bulge belongs to the segment starting at that vertex. The last open-chain vertex has zero
bulge. The `opening` joins loose endpoints only to describe missing boundary.

Use provider-issued `feature_kind` and `section_shape` for conventions, and derive dimensions
from geometry. There are no duplicate width/depth/radius fields or build123d-specific JSON values.
CAM setup, accessibility and toolpaths remain consumer decisions. Non-constant sections and
islands are not admitted by this contract.

## Complete public cutover

The root exports and aggregate fields for `Pocket`, `PrismaticPocket`, `Channel`,
`Passage`, both edge-open families, both blind-slot families and old pocket
patterns are removed. Their `recognise_*` entrypoints are replaced by
`recognise_section_recesses`. Shared `PassageFrame`, `PassageSection` and
`PassageSectionVertex` geometry primitives remain. Unrelated `Slot` and `OrientedSlot`
families are unchanged.

`SectionPassage` and `PassageEnds` remain public **nested** contracts because
`OrientedSlot.source` still returns them. Both are available from the package root
and declared under `oriented-slots` in the manifest (schema version 2). Consumers
can validate their exact types without importing implementation modules. This
repairs the missing exports in Quiddity 0.2.0–0.2.2; it does not restore
`recognise_section_passages`, `RecognitionResult.section_passages` or a passage
census family, nor change the existing oriented-slot JSON.

`section_recess_patterns` contains `SectionRecessArray` or `SectionRecessGrid`, whose
`members` are occurrence indices, not embedded old Pocket values. Array direction is in result
coordinates. Grids provide perpendicular unit `row_direction` and `col_direction` in result
coordinates, plus center and pitches; no implicit world-XY angle is required. Only patterns with
one unambiguous geometric occurrence per member are published; this
does not introduce a new free-axis pattern detector.

The original unified cutover shipped in 0.4.15 at the maintainer's request. Despite
the patch version, that was a breaking, coordinated consumer change and an explicit
exception to the earlier 0.4.x compatibility promise. Schema 3 is a subsequent
Quiddity change, not part of 0.4.15. Consumers must migrate before adopting it;
neither change updates Draftwright automatically.

## Explicit refusals, not fabricated geometry

The two former summaries in `plates_pads_levels_and_slanted_steps` now have proved channel
geometry: the pad-overhang region has an 18-unit run, and the wall/step region a 50-unit run.
The proof intersects opposed source-wall extents and verifies the entire three support patches
against actual same-body faces. Holes and incomplete trims cannot masquerade as full support.
Material probes require the full section, both run-end openings and lateral opening to be empty.
The shorter support bounds the run; it need not span the stock envelope.

Some internal detectors still accept an extent summary without sufficient support for a constant
section. These become `section_recess_refusals` / document `refusals` with
`reason="unsupported_support_geometry"`, body index and source-face evidence. They are not
reconstructible occurrences and contain no alternate dimensional schema. This intentionally
retires the weak geometry claim while retaining its evidence. A bounded publication error in an
otherwise exact legacy section still raises `LegacySectionProjectionError`; it is not hidden as
an unsupported-support refusal.

## References, evidence and counts

Body, face and occurrence indices belong to one result. Retain the exact input and enumeration
to resolve them; never compare bare indices across imports, edits, healing or framing. Both
defining and constituent lists reference the same complete face roster. Geometry goldens do not
freeze kernel-dependent face ordinals.

The public census counts `section_recess` once from unified geometric occurrences; retired
detector-category census keys are gone. Refusals and patterns do not add to the occurrence count.

Python `RecognitionEvidence` uses the same unified records. Its feature references may also
resolve to a `SectionRecessRefusal`: distinguish that type before consuming geometry.
Associated-face coverage includes retained source evidence, including refusals. It means association,
not successful geometry reconstruction, machining completeness or recognition accuracy.

The private detector inventory retains its existing taxonomy for effectiveness scoring and frozen
historical goldens. It is not a consumer compatibility API. Output-schema changes must not
silently change benchmark denominators or double-count a projection as another discovery.
Consequently old detector counts and the new public geometric census are not interchangeable.

## Validation

Authored fixtures check reconstruction, orientation/signs, STEP round trips, ownership, support
and obstruction refusals. Migration tests account for accepted regions across authored goldens
and all 40 vendored MFCAD++ development models. An explicit refusal is accounted evidence, not a
claim of geometric equivalence. Historical detector goldens remain unchanged.

```console
uv run pytest -q -n 4 tests/test_section_recess_cutover.py tests/test_section_recess_migration.py
uv run python -m tools.audit_section_recess_migration
```

No MFInstSeg input is needed or inspected. A separate runner may later assess the frozen branch;
that is not a release or development gate.
