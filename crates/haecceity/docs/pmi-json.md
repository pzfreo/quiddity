# PMI JSON reference

## Standards check — 2026-10-30 (issue #142, rule 6 / agreement g)

[ISO 10303-28:2007](https://www.iso.org/standard/40646.html) specifies XML
representations of EXPRESS schemas and data, including EXPRESS-to-XSD mapping;
it is not a JSON binding. Its XML exchange structure is an entity graph.

AP242 edition 4 includes the edition 5 domain model,
[ISO/TS 10303-4442:2025](https://www.iso.org/standard/91590.html), whose published
scope explicitly names the domain-object XML Schema.
The [ISO domain-model XSD location](https://standards.iso.org/iso/ts/10303/-4442/ed-5/tech/xml-schema/domain_model/)
and [MBx-IF ed4 Domain Model XML practice](https://www.mbx-if.org/home/wp-content/uploads/2025/10/rec_prac_ap242xml_assy_struct_v4.0.pdf)
identify XML as the implemented domain-model binding. The
[AP242 ed4 project](https://atlas.afnet.fr/en/iso-step-ap242-edition-4/)
also lists web services using JSON. That is evidence of JSON work, not enough to
claim an available normative JSON encoding of all EXPRESS or semantic PMI.

Related ISO JSON does exist: [ISO/TS 10303-4443:2022](https://www.iso.org/standard/83491.html)
includes a domain-object OpenAPI JSON Schema for AP243/MoSSEC. That is a
published, domain-specific API model; it supplies neither a generic EXPRESS
instance encoding nor AP242 PMI objects to reuse here. It must not be mistaken
for the AP242 -4442 XML domain model.

A public [experimental EXPRESS JSON encoding, v1.0.0.0, 2026-03-10](https://spatialcompute3d.com/blogs/express_json/express_json.html)
exists. It maps Part 21 entity instances and SELECTs to JSON arrays. It calls
itself experimental and is not an ISO publication or an identified ISO draft.
Searches of ISO's public catalogue, the AP242 project and MBx-IF publications
found no published or publicly accessible draft ISO JSON binding with rules that
could govern this mapping. This is a bounded finding, not a claim that no
unpublished work exists. In particular, ed4 JSON web-service work must not be
confused with a published Part 28 JSON replacement.

We therefore follow the existing AP242 EXPRESS semantic model and
[CAx-IF PMI Representation and Presentation v4.1, 2024-06-20](https://www.mbx-if.org/home/wp-content/uploads/2024/06/rec_pracs_pmi_v41.pdf).
This is an application-level semantic JSON mapping, not ISO Part 28 conformance
or a Part 21 entity-graph serialization. It retains stated decimal tokens,
AP242 names and the practice's groupings. Supplemental geometry alone retains
its typed geometry tree because that is how `PartPmi` represents it.

## Read-only callout evidence

`pmi::PmiRead.callouts` is file-level evidence, beside findings, provenance and
accounting. Each `GraphicalCallout` has its numeric STEP `id`, decoded `name`, and
`links`. Every `CalloutLink` retains the DMIA `association`, its `definition` STEP
id and `entity` type, and resolved `targets` (`CalloutTarget { part, item }`).
`PresentedItem` uses the returned part model's typed ids (indices for notes,
attributes and property collections). An empty target list is an unresolved
association, not graphic-only; an empty link list means graphic-only.

The list includes every draughting callout and annotation occurrence outside a
callout, including tessellated and annotation-plane-held occurrences. Planes
are containers, not additional callouts. Associations on contained occurrences
and holding planes are included. Plain shape aspects resolve to features; datum features also name the datums they
establish. Unsupported
or refused semantic definitions retain their entity type instead of disappearing.
`Note.callouts` and the presentation accounting count keep their existing meaning.

`PmiRead` itself is not serialized by the JSON API. `haecceity.pmi/1` serializes
`PartPmi` through `Semantic`, and `quiddity-pmi` exports a semantic projection;
neither includes file-level provenance or accounting. Consequently this read-only
list does not change those wire formats or their mirrored Python schemas.
The existing As Drawn presentation companion is unchanged. With `json-import`,
transport restoration reindexes the callout targets alongside semantic items,
including one-to-many feature aliases.

## Shipping and wire conventions

Canonical files are `crates/haecceity/docs/pmi-json.md` and
`crates/haecceity/schema/haecceity.{pmi,presentation}-1.json`; root `docs/` and
`schema/` links expose them to repository consumers. Cargo packages include the
canonical files. Specify's Python package can copy the two schemas into
`specify_core/schema/` as package data and expose the reference in its documentation;
that separate repository is not modified here. Rust exposes the schema texts as
constants under the opt-in `json-import` feature, independent of `presentation`.

`haecceity/json` is serialization only: `identify`, `identify_with_anchors`,
`to_value`, `to_string`, and As Drawn/generated presentation export. It uses a
small generated typed traversal, with no embedded schema, schema engine, JSON
parser, model deserializer, importer, or transport-order restoration. Native STEP
exports use index anchors; supplied typed semantic values retain their ids.
`haecceity/json-import` implies `json` and adds parsing, schema constants,
validation, `from_value`/`from_str`, writing and checked transport restoration.
This is the feature for lossless re-import of a JSON producer's custom aliases.
The CLI requires `json-import` for the complete command set. At both crate levels,
`json` enables export only, and `json-import` implies `json`. `presentation` stays separate.
`examples/json_wasm_size.rs` provides a reproducible WASM size probe.

A document is `{ "schema": "haecceity.pmi/1", "units": { "length": [...],
"angle": [...], "density": [...], "other": [...] }, "pmi": { ... } }`. The machine-readable schemas use JSON
Schema 2020-12. Every model field is explicit, including null optionals and empty
arrays. Unknown properties are errors. Every item in `features`, `geometry`, `derived_geometry`, and the semantic
requirement collections has `{ "id": "caller string", "value": <typed value> }`.
An optional sibling `"identifier"` carries the decoded source file identifier /
characteristic number (`ID_ATTRIBUTE.attribute_value`). It is present whenever the
reader has an unambiguous source identifier, including a duplicated or empty string.
It is an observation, not a reference key: consumers must continue to resolve links
using the unique, nonempty `id`. The value retains its original spelling and is not
restricted to numeric characteristic numbers. Ambiguous multiple identifiers remain
unavailable and are reported by the native reader.

`identifier` is an additive field in `haecceity.pmi/1`; closed-object consumers must
update their schema before accepting new exports. Documents without it remain valid.
`Semantic.identifiers` keeps these observations separately from `Semantic.ids` and
from model values. JSON decoding and transport preserve both the observations and
their absence: transport-managed wire aliases are not invented characteristic numbers.
All references between items are nonempty **id strings**, never array indices:
`FeatureId`, `DatumId`, `DatumTargetId`, `DimensionId`, `ToleranceId`,
`GeometryId` and `DerivedGeometryId` resolve within their named collection.
Unknown ids and references to the wrong collection are refused with a reason.
Array order is retained; adding an unrelated item requires no renumbering.
Topology indices (`FaceIndex` and `EdgeIndex`) are not item references. Unit definitions also have `{id,value}` wrappers;
all unit references are id strings. A face fingerprint anchor can name several support
faces; every support face must resolve, and the anchor expands into those faces.
One dimension or tolerance is one semantic object, with its content together.

Source requirement IDs come from `ID_ATTRIBUTE`. Empty or repeated file identifiers
never abort native JSON export: the first nonempty requirement identifier in the fixed
collection order (`geometry`, `derived_geometry`, `datum_targets`, `datums`, `dimensions`,
`tolerances`, `tolerance_relations`, `general`, `threads`, `knurls`, `notes`,
`surface_textures`, `attributes`), then model order, retains the wire id. Later items
use their content-derived ids. Native model order is deterministic; source observations
are retained for every item. The native `PmiRead.findings` reports empty or duplicated
source identifiers with their carrier and identifier-attribute STEP instances and value.
Features have a separate support namespace; repeated or empty feature identifiers also
fall back to content ids. Duplicate wire ids in caller-authored JSON still refuse import,
because their references cannot be resolved unambiguously.

Structural support projections
of a shared STEP carrier have their own `features:` namespace. Missing IDs are
content-derived: canonical model content recursively substitutes referenced
item content and verified fingerprint keys (when supplied) for indices; a
specified FNV-1a 128-bit digest names the collection and content. The digest
input is UTF-8 JSON with recursively sorted object keys, no insignificant
whitespace and arrays retained in order; no Unicode or decimal-text
normalisation is applied. `Note.callouts` are presentation observations and are
excluded from semantic identity. This is independent of serde's map-order features. Only identical
twins acquire `~2`, `~3`, … suffixes. Different content with a hash collision is
refused. Reading an unrelated extra item cannot shift existing IDs. Without a
verified fingerprint namespace, topology indices are used as the explicit
support identity; no incompatible fingerprint algorithm is invented here.
The companion presentation includes the sorted complete `semantic_ids` set;
clients must compare it with the PMI document before using its links.

The schemas use permanent identifier URNs `urn:haecceity:schema:pmi:1` and
`urn:haecceity:schema:presentation:1`; they do not promise an unhosted web URL.

Enums with no payload are strings; enums with payload are single-key objects.
Keys and enum strings use AP242 entity/enumeration or CAx-IF names wherever the
model supplies a name table. Other model alternatives use lower snake case.
For example a diameter size kind is `"diameter"`, a tolerance kind is
`"position_tolerance"`, and a dimension's kind is
`{"size":{"feature":"support:bore","kind":"diameter","path":null,"angle":null}}`.
The `$defs` enumerate every alternative and field without relying on this prose.

Decimals are strings, including directions, geometry real values and ratios.
Integer mantissas and either exponent case are accepted. When a STEP REAL is
required, the writer adds a decimal point and uppercases `e` as needed (e.g.
`-3` → `-3.`, `2e1` → `2.E1`), without changing the exact value. JSON transport
retains and checks the original spelling, including derived-unit exponents.
Already legal REAL spellings stay verbatim. COUNT_MEASURE still permits native
integer tokens. Non-finite/unrepresentable REALs are refused by validation and import.
`Length.unit`, `Angle.unit` and `Geometry.length_unit` are id references into the
appropriate **document-level** unit table by id; mixed units retain their original
values without conversion. Unit definitions include named SI/conversion units
and `other` with the stated exact metres/radians factor. `Density.unit` references `units.density` by id, whose definitions retain the model's
derived factors (base-unit names and decimal exponents).
Both writers use [CAx-IF UDA v1.8 Annex C.1 and C.4.2](https://www.mbx-if.org/home/wp-content/uploads/2024/05/rec_prac_user_def_attributes_v18.pdf):
SI mass is kilogram; gram is a conversion-based mass unit with factor `0.001 kg`
(and milligram `0.000001 kg`). This follows the explicit g/cm³ example in C.4.2,
leaving density values, unit names and JSON statements unchanged. Reading also
accepts older unprefixed SI gram. For material density only, the reader repairs
OCCT's exact two-factor mass³ × length² unit to mass/length³, retaining the value
and base-unit names, and reports a nonconformance finding. A bare REAL density
value is accepted with a separate `density value untyped (OCCT)` finding. Other
non-density units remain omitted with an explicit finding; extra factors or
other wrong exponents do not qualify for the repair. Rewriting uses Material
Identification and Density R2.1 §4.2.4: `density measure`, typed
`POSITIVE_RATIO_MEASURE`, exponents `1.` and `-3.`, and the conversion-based mass
unit above. This native-read repair does not allow authors to import JSON with
wrong density dimensions. As in ordinary replacement writes, unused source unit
records remain shared infrastructure; the rewritten density references newly
emitted conformant units. Other measured attribute units reference `units.other` by id, preserving their source unit names. Every unit therefore
lives at document level, including units the model holds only by name.

`FaceIndex` accepts an integer, `{"name":"OCCT face name"}`, or
`{"fingerprint":"support key"}`. Indices use `read_part_definitions().faces`
order, the kernel/Specify snapshot mesh's numbering; they are not STEP entity ids.
Names come from the face's `representation_item.name`, including names written
by OCCT's `write.stepcaf.subshapes.name`. A resolver accepts fingerprint support
keys supplied by the consuming snapshot/correspondence layer; it must not invent
an incompatible fingerprint algorithm. Unknown and ambiguous anchors are refused.
Edges retain zero-based indices. Geometry anchors reference supplemental items.

On STEP export, ids use `PmiRead.identifiers` (`ID_ATTRIBUTE`) where available;
otherwise the content-derived rule above applies. All ids in a document are unique,
nonempty strings. Input ids are preserved when writing back to STEP; re-reading
that output returns the same ids. Generated ids avoid collisions with file ids.
Validation first applies the shipped schema, then constructors and
`PartPmi::validate`, then resolves topology; STEP writing applies the existing
writer's feature/kind, EXPRESS and rule checks. Every refusal includes its reason.

## Type sources

The table covers every `$defs` semantic type. Names in the left column are the
Rust/model names used as schema definition names. Item reference wrappers are id strings,
not separate AP242 entities. “PMI” section numbers refer to CAx-IF PMI v4.1;
[Supplemental Geometry v1.3](https://www.mbx-if.org/home/cax/recpractices/)
[User Defined Attributes v1.8](https://www.mbx-if.org/home/wp-content/uploads/2024/05/rec_prac_user_def_attributes_v18.pdf) and
[Material Identification and Density R2.1](https://web.archive.org/web/20240419135511id_/https://www.mbx-if.org/documents/RecPrac_MaterialDensity_v21.pdf) supply the additional practices. Schema-level
wrappers, ids and unit ids are JSON transport conventions, explicitly
not AP242 entities.

| JSON types | AP242 entities/types represented | Practice |
| --- | --- | --- |
| `Decimal`, `Length`, `Angle`, `Quantity`, `Value`, `Ratio`, `Count`, `Bounds`, `LengthUnit`, `AngleUnit` | length_measure_with_unit, plane_angle_measure_with_unit, measure_representation_item, value_format_type_qualifier, plus_minus_tolerance, tolerance_value, si_unit, conversion_based_unit | PMI §§5.2–5.4 |
| `Anchor`, `Feature`, `GroupKind`, `DerivedKind`, `DerivedGeometry`, `DerivedGeometryKind` | shape_aspect, composite_group_shape_aspect, derived_shape_aspect and subtypes, shape_aspect_deriving_relationship, geometric_item_specific_usage | PMI §§3.8–3.9, 5.1.4, 6.4 |
| `Geometry`, `GeometryItem`, `GeometryValue` | geometric_representation_item and its typed geometry tree, constructive_geometry_representation | PMI §§3.9.1, 9.4.2; Supplemental Geometry §§5.1–5.4 |
| `DatumLabel`, `Datum` | datum, datum_feature, datum_feature_relationship | PMI §6.5 |
| `Placement`, `Direction`, `TargetShape`, `DatumTarget` | axis2_placement_3d, direction, datum_target, placed_datum_target_feature, target parameters, feature_for_datum_target_relationship | PMI §6.6, Tables 10–11 |
| `DatumModifier`, `SimpleDatumModifier`, `DatumModifierType`, `DatumReference`, `Compartment`, `DatumSystem` | datum_reference_modifier, simple_datum_reference_modifier, datum_reference_modifier_with_value, datum_reference_element, datum_reference_compartment, datum_system | PMI §§6.9.7–6.9.8, Table 15 |
| `Dimension`, `DimensionKind`, `SizeKind`, `LocationKind`, `AngleSelection` | dimensional_size, angular_size, dimensional_size_with_path, dimensional_location, angular_location, dimensional_location_with_path, directed_dimensional_location, angle_relator | PMI §5.1, Tables 2–4 |
| `DimTolerance`, `Principle`, `Qualifier`, `DimensionModifier` | shape_dimension_representation, plus_minus_tolerance, limits_and_fits, type_qualifier, descriptive_representation_item | PMI §§5.2–5.3, Tables 5–8 |
| `Iso286Class`, `FundamentalDeviation`, `FitFeature`, `DeviationLetter`, `ToleranceGrade` | limits_and_fits | PMI §5.2.3; ISO 286 |
| `GeometricTolerance`, `ToleranceKind`, `DatumRequirement`, `ToleranceTarget` | geometric_tolerance and the 15 characteristic subtypes, geometric_tolerance_with_datum_reference, toleranced_shape_aspect | PMI §§6.7–6.9, Table 12 (DatumRequirement is the model validation classification) |
| `Zone`, `AffectedPlaneKind`, `ProjectedZone`, `ZoneForm`, `ToleranceModifier`, `UnitBasis`, `UnitArea`, `AreaShape`, `AuxiliaryClassification` | tolerance_zone, tolerance_zone_form, projected_zone_definition, runout_zone_definition, non_uniform_zone_definition, geometric_tolerance_with_modifiers, geometric_tolerance_with_defined_unit, geometric_tolerance_with_maximum_tolerance, unequally_disposed_geometric_tolerance, geometric_tolerance_auxiliary_classification | PMI §§6.9.1–6.9.6, Tables 13–14 |
| `ToleranceRelation`, `RelationKind` | geometric_tolerance_relationship | PMI §6.9.9 |
| `Standard`, `StandardBody`, `GeneralTolerance`, `ToleranceCell`, `CellValue` | applied_document_reference, document, general_property, property_definition, representation, compound_representation_item | PMI §§4–4.1, Figures 1–3 |
| `Thread`, `ThreadSide`, `Hand` | thread, product_definition_shape, shape_aspect, shape_representation_with_parameters, property_definition, measure_representation_item | No thread section in PMI v4.1; AP242 thread WR1–WR16 (existing model extension) |
| `Knurl`, `KnurlPattern` | turned_knurl, product_definition_shape, shape_representation_with_parameters, property_definition, measure_representation_item | No knurl section in PMI v4.1; AP242 turned_knurl WR1–WR12 (existing model extension) |
| `Hole`, `RoundHole` | basic_round_hole, round_hole, shape_aspect, axis2_placement_3d | No hole-definition section in PMI v4.1; AP242 ed4 basic_round_hole / basic_round_hole_occurrence (reader extension) |
| `Material`, `Density` | property_definition, representation, descriptive_representation_item, measure_representation_item, positive_ratio_measure, derived_unit | Material Identification and Density R2.1 §§4.1–4.2 |
| `Note`, `Callout`, `NoteOwner` | property_definition, descriptive_representation_item, draughting_callout, draughting_model_item_association | PMI §§7.3–7.4; UDA |
| `SurfaceTexture`, `MaterialRemoval`, `SurfaceTextureParameter` | property_definition, general_property, surface_texture_representation, measure_representation_item | No surface-texture section in PMI v4.1; ISO 10303-1110 Surface_texture / Standard_surface_texture_parameter, with existing model decisions in step-ap242.md |
| `AttributeSet`, `GeneralProperty`, `AttributeValue` | property_definition, general_property, general_property_association, representation, descriptive_representation_item, measure_representation_item, compound_representation_item | PMI §7.4; UDA §§5–7 |
| `PartPmi`, `PartId`, `FaceIndex`, `EdgeIndex`, `FeatureId`, `DatumId`, `DatumTargetId`, `DimensionId`, `ToleranceId`, `GeometryId`, `DerivedGeometryId` | product_definition, product_definition_shape (part aggregate); references to the types above | PMI §3.8; JSON item-id and topology-index conventions |

## Presentation companion

`haecceity.presentation/1` contains `items`, a map from semantic item id to an
array of graphics, and `unlinked`, an array for graphics with no semantic link.
Each `Graphic` has `source: generated | file`, `name`, model-space `polylines`,
`triangles`, editable `text`, diagnostic `step_ids` and `unsupported` constructs.
Coordinates are millimetres; these display coordinates are JSON numbers rather
than semantic decimal statements. Unsupported graphics are retained as records
with their source ids and a reason, never silently removed.

| JSON type | AP242 entities | Practice |
| --- | --- | --- |
| Presentation document / items / unlinked | draughting_model, draughting_model_item_association | PMI §§7.1–7.3 |
| Graphic with source=file (As Drawn) | draughting_callout, annotation_occurrence, annotation_curve_occurrence, tessellated_annotation_occurrence, annotation_text_occurrence, annotation_plane, geometric_set, tessellated_geometric_set | PMI §§7.1–7.3; Polyline Presentation |
| Graphic with source=generated | same presentation entities, rendered from the semantic model by haecceity::presentation | PMI §§7.1–7.3 |
| polylines / triangles / text | polyline, tessellated_curve_set, triangulated_face, text_literal and literal text | PMI §7.1; Tessellated 3D Geometry |
| source / step_ids / unsupported | transport provenance and explicit diagnostic retention; no additional semantic entity | JSON convention |

### Optional editor placement and views (For #142)

The schema remains `haecceity.presentation/1`. All fields below are additive and
optional; the original required fields and their meanings are unchanged. Old
companions validate and deserialize with absent metadata defaulted to empty/None.
A version bump would unnecessarily reject those existing documents.

| Optional field | Meaning and source |
| --- | --- |
| `Graphic.id` | Document-local graphic id (`step:<root instance id>` for file graphics). Copies under several semantic keys share this id; render/count once per id. Generated ids contain the semantic collection and id, or `generated:general-notes` for the shared notes block. |
| `Graphic.plane_id` | Reference to `planes[].id`, from the containing `annotation_plane`. Omitted with a diagnostic if unresolved or if several planes contain the same graphic. |
| `planes[]` | `{id, name, origin, x_axis, normal}` from the annotation plane's `plane.position` or `planar_box.placement`. Origins are model-space millimetres; axes are unit vectors. The in-plane Y axis is `normal × x_axis`. |
| `Graphic.text_placements[]` | `{text_index?, origin, x_axis, normal, height?, line_spacing?, alignment?, stroked?}`. `text_index` is a zero-based index into the existing `text` array. The origin is the native alignment point, not an assumed lower-left corner. |
| `views[]` | `{id, name, direction?, up?, graphics}` from draughting/presentation models and their camera models (including `model_geometric_view` associations). `graphics` contains graphic ids, not semantic ids. A saved view includes only its own contained graphics, including mapped content. |

Text literals retain their `text_literal.placement`, including 2D placements in
mapped text representations. Mapping composes target placement with the inverse
representation-map origin; repositioned tessellated placement is also respected.
A text style's stated `BOX_HEIGHT` takes precedence over a literal's extent height.
Heights and origins use the representation length unit and are exported in mm.
For literal text, height describes the stated style/extent box; it is not an
inferred font cap height. Native alignment strings are retained. Unknown height
is omitted and diagnosed rather than estimated from the string.

Placeholders retain their actual placement(s), without manufacturing editable
strings from requirement names. They have no `text_index`; their declared
`line_spacing` is distinct from height. Source zero spacing is retained and
reported as nonconforming. A placeholder planar box may supply its extent height.
Generated notation uses the actual placed text runs, centred alignment points and
scaled capital heights. `stroked: true` means that run's glyphs (including explicit
geometric symbols) are already in this graphic's polylines: retain the string for
editing/accessibility, and skip a second text draw. The flag records actual stroke
emission, never a deduction from `source`. File literals have absent `stroked`
(or equivalently false), including literals in a generated-source graphic; draw
those with the client's font. Old documents may omit the flag. Empty-ink cases retain a diagnostic instead of non-finite
coordinates. Existing strokes, tessellated fills and text strings are unchanged.

Regeneration resolves linear dimensions from their semantic witnesses, orders
rungs shortest first, and retains common outer annotation planes. Compact ladders
reserve actual glyph ink rather than padded notation cells. Datum flags are placed
before labels; control frames attach near their first cell. Parallel annotation
planes pack together; unrelated camera planes do not force labels outward. This
uses the generated saved-view model. The STEP writer uses the same compact ink
spacing while retaining its seven-view clearance policy. Text origins and heights come from the actual stroke transform,
including the span clipping of short dimensions and the tip offset of surface symbols.

Camera direction points into the scene (negative camera Z), and up is camera Y.
A draughting model without a camera still supplies a membership group, omitting
both direction and up. File cameras are never guessed. Generated output supplies
one orthographic view per annotation plane: direction = -normal, up = normal ×
x_axis, with graphic ids deduplicated. The model has no camera position, scale or
frustum; clients fit the part and the listed graphics when opening a view. IDs are local to this companion;
use `semantic_ids` to verify the companion's semantic link set before use.
Validation checks plane, view and text-index references and consistent copies of
one graphic under multiple semantic keys.

Generated part settings and submitted part-level `notes` share one `General notes` graphic (`generated:general-notes`),
one `GENERAL NOTES` heading and one annotation plane. Each requirement's semantic
key aliases that same graphic, so per-item ids and questions remain available;
render the graphic only once by id, as for a read STEP callout. Material, which
has no independent semantic item id, joins the block (unlinked if material alone).
The STEP callout contains a separate occurrence per requirement with its own
DMIA to that requirement's property definition; the heading shares the first
occurrence. This retains the #137 typing and IIRU uniqueness rules.

Items are numbered from 1, including a single item, with two spaces after the
number and indented continuation lines, following Draftwright `1c830977`,
`src/notes.rs::rows` and `plan_with_rows`. Draftwright's `pmi_lowering.rs::sheet_requirements`
places tolerance and default finish in separate sheet defaults, so the combined
model-space block uses a defined order: general tolerance classes, default
finishes, edges, coating, heat treatment, other general notes, material. Each
category retains input order (structured settings, text attributes, then unstructured
notes); general notes use explicit positive note indices
first, then input order. Unstructured notes retain their own `notes` ids; recognized
finish parameters use the writer's explicit-unit parser, preserving stated precision
without guessing units. Generic attribute fields of unknown kind remain opaque;
they can carry unavailable CAD-font symbols rather than note prose. Explicitly
submitted unstructured part notes may use other kinds. No submitted settings means no block. Retained file
graphics and notes are unchanged.

Generated general notes use a front annotation plane (X right, Z up, Y at the
part bounds' centre). The first block's ink starts at `max X + 2h`, with its top
at `min Z - 2h`, where `h` is the bounds-derived capital height. Later blocks stack
down, separated by `2h`. Thus notes clear the part and one another in their own
plane and occupy one column beside the lower-right corner; feature leaders and
other annotation planes cannot recursively push the column away. The STEP writer
uses the same rule for its combined notes block. Clearance in all seven possible
projections is deliberately replaced by clearance in the notes plane; no nearby
3D annotation can be guaranteed readable from every camera.

Datum-feature symbol links include the owning datum id even when a direct feature
key is present. Ownership follows the native datum-feature-to-datum relationship,
with feature aliases retained; it is not inferred from equal face supports.

The shared mixed fixture is
`crates/haecceity/tests/fixtures/presentation/ctc01-text-filled.step.gz`:
**one authored literal and 2,754 triangles**. It is explicitly a derived CTC-01
fixture, not unmodified NIST data. None of the 28 STEP files in the inspected NIST
archive contains `TEXT_LITERAL`; original CTC-01 has zero literals and the same
2,754 triangles. The fixture README and deterministic builder record this limit.

Reading includes the file's own graphics. Presentation JSON writing is optional
and is not implemented in version 1; semantic JSON writing uses haecceity's
existing generated presentation. A semantic association may point to several
items, and each associated item keeps the graphic. Unassociated graphics remain
in `unlinked`.

ISO table helper types `Iso2768`, `LinearClass` (`f`, `m`, `c`, `v`) and
`GeometricClass` (`H`, `K`, `L`) map the `limits_and_fits`/general-tolerance
classification (PMI §4.1, ISO 2768). Supplemental `geometry`, computed
`derived_geometry` and `features` use identified `{id,value}` items, including
support projections of shared native carriers. `holes` remain details whose
`feature` field references the support's id. Settings and material are aggregate
part fields, not independently referenced items.

`SupportKey` maps the verified geometric namespace used by Specify #65/#77:
`{"fingerprint":{"schema":2,"faces":"<verified face hash>","support":[0]}}`.
It is a JSON support convention over AP242 shape-aspect topology (PMI §3.8), not
an EXPRESS entity. A fingerprint anchor can contain several support faces and expands to
that many face anchors in the model. Every face must resolve. `Anchors::bind_support` binds a consumer-verified namespace.
Opaque string support keys are also accepted through `Anchors.fingerprints`.
No new fingerprint algorithm or hash dependency is introduced.

Tolerance relationships use `applied_identification_assignment` with an
`identification_role` named `haecceity.pmi/1 item id`: AP242's
`id_attribute_select` excludes `geometric_tolerance_relationship`, while ed4's
`identification_item` includes it. JSON writes upgrade an ed1 file to the existing
ed4 writer target when this carrier is needed. Other independent items use
`ID_ATTRIBUTE`. The reader, including default-feature builds, recognizes this assignment role; its
assignment is consumed with the relationship. Ordinary writes keep their existing
schema and bytes. This is a transport convention over the standard assignment
entities, not an extension to the EXPRESS schema.

`LengthUnitDefinition` / `AngleUnitDefinition` / `DensityUnitDefinition` /
`OtherUnitDefinition` name the document-level unit definitions; their corresponding
`*Unit` definitions are id transport references. AP242 sources are `si_unit`,
`conversion_based_unit`, `derived_unit` / `derived_unit_element`, and the named
unit of a `measure_with_unit` (PMI §§5.2–5.4; Material practice §4.2; UDA §7).

## STEP writer boundary

The JSON mapping is lossless over `PartPmi`; it cannot recover facts the model
never held. STEP writing keeps the existing writer's refusals: for example a
missing nominal value (PMI §5.2.1), a projected zone without its end geometry,
an ambiguous named unit, an unstated group kind, and read-only hole definitions.
Repeated or empty source identifiers are retained in `identifier` metadata; affected
items receive unique content-derived reference ids, with native read findings describing
the nonconformance. Caller-authored JSON must still use unique, nonempty reference ids.
`json::write` verifies the emitted semantic model and source metadata by reading it back
before returning bytes. It never removes refused requirements.

`json::validate` and `pmi json validate` also check constructors, reference closure,
and the writer's geometry-independent authoring refusals. `json::validate_schema`
remains a structural schema check, suitable for read-only model interchange.
Import still checks face/edge existence, the geometry context and its units,
file-edition compatibility and native schema/rule validation against the actual STEP.
Stated REAL tokens must use Part 21 syntax (a decimal point and uppercase `E` for
exponents); integer-only and lowercase-exponent strings are refused for REAL fields,
consistently with dimensions. COUNT_MEASURE also permits canonical integers.
Placements, target sizes and movable directions retain every decimal occurrence,
including different spellings of equal numbers in the same vector.

The strict corpus STEP closure test is separately runnable with `--ignored` so
these pre-existing refusals remain visible. The local SFA sample test, also
explicitly invoked, records any supported-subset exclusions in adjacent audit
files; such a sample does not establish a complete source round trip.

Presentation associations to datum-feature support aspects include their owning
datum requirement even when a direct feature association is already available.
Feature keys remain aliases of the same graphic id. Other support-only graphics
remain unlinked and are retained.

## v1 identity and presentation policy

JSON writes preserve note kinds and text as CAx-IF §7.4 descriptive properties,
without migrating notes into attributes. All note kinds and arbitrary text
(including Unicode and empty kind strings) use the following form:

| JSON note | Native AP242 form read back as `Note` |
| --- | --- |
| `kind: "semantic text"` | `property_definition('manufacturing requirement','semantic text',owner)` |
| `kind: "surface texture"`, `"coating"`, `"edges"`, `"general note"`, or any caller string | Same property, with the exact kind in its description |
| `text` | One `descriptive_representation_item('note text',text)` in its representation |
| `on: null` | Part `product_definition_shape` |
| `on: {feature/datum_target: id}` | Referenced item's native shape aspect |
| `on: {dimension/tolerance: id}` | Referenced native dimension or tolerance, in an edition whose SELECT permits it |

For #142, notes and attribute sets owned by dimensions or tolerances trigger the
existing JSON-only ed1 → ed4 upgrade, with the schema change recorded in
`WriteReport.schema` and the complete upgraded file validated. AP242 ed1's
`characterized_definition` excludes `dimensional_size` and `geometric_tolerance`;
ed4 includes them through `characterized_item`. The direct property attachment is
allowed from ed2 onward by [CAx-IF PMI v4.1 §§7.4.2 and 10.1.2.1–2](https://www.mbx-if.org/home/wp-content/uploads/2024/06/rec_pracs_pmi_v41.pdf).
We choose that form to identify the exact requirement even when several dimensions
or tolerances share one shape aspect. All `NoteOwner` variants remain supported;
no owner is inferred from shared geometry. Ordinary non-JSON writes keep their
existing schema policy and bytes.

The reader already recognises this manufacturing-requirement form. A property
named `semantic text` associated with a general property remains an AttributeSet;
the two collections are intentionally distinct. Callout observations are retained
as transport bindings; presentation JSON does not create drawn geometry.

Feature and attribute order, optional descriptions (including null), and
general-property fields are retained. The following is a **haecceity producer
convention**, not a CAx-IF-defined requirement:
`PROPERTY_DEFINITION('haecceity.pmi/1 ordering',
'Transport ordering and reference aliases; native PMI remains authoritative',
<product_definition_shape>)` has a representation containing one
`DESCRIPTIVE_REPRESENTATION_ITEM('ordering', <compact JSON>)` when it fits.
Long text instead uses `COMPOUND_REPRESENTATION_ITEM('ordering',
LIST_REPRESENTATION_ITEM((#chunk1,#chunk2,...)))`. Each chunk is a
`DESCRIPTIVE_REPRESENTATION_ITEM('haecceity text chunk/1', <text>)`; concatenate
its decoded descriptions in **list order**, without separators. This also applies
to long native descriptive values (notes, attribute text, material descriptions,
and parameter text). The list, rather than STEP instance order or a SET, defines
the order. The complete encoded literal, including quotes, doubled apostrophes
and Unicode escapes, is at most 8192 bytes for generated values. Splits occur between Unicode scalars;
chunks contain independently valid escapes. Legacy single items remain readable,
regardless of length. JSON's schema and semantic values do not change.

This is a haecceity concatenation convention over ISO 10303-43 compound/list
representation items. [CAx-IF UDA 1.8 §7](https://www.mbx-if.org/home/wp-content/uploads/2024/05/rec_prac_user_def_attributes_v18.pdf)
allows multiple representation items when their interpretation is clear, and
§7.1 specifies descriptive strings; it gives **no** string length limit. The
8192-byte limit is an interoperability choice for Part 21 scanners, including
OpenCascade's. It leaves room below OCCT's default 16 KiB scanner buffer while
preserving ordinary existing source strings. Oversized foreign source literals
are retained with non-fatal `WriteReport.long_strings` warnings, rather than
blocking a write. Producer-owned legacy descriptive metadata is rechunked in
place even in Add mode; the parent ids and decoded values stay the same. Consumers of this producer property must support both forms.
SFA may display this ordinary property in its property worksheet; it contains no second copy of
the semantic values.

The compact producer convention has `v: 2` (omitted means the original version 1).
It contains `items`, grouped by collection, with each caller id, a checked
native content digest and only necessary reference/sequence/support bindings.
Empty collections/maps, null bindings, empty callouts and wire anchors are
omitted. A native singleton face/edge support does not repeat its anchor. Unit
identities/definitions/order are retained when present; empty tables are omitted.
Reference bindings use `[collection ordinal, item ordinal]` into this record,
avoiding repeated id strings. Collection ordinals follow: features, geometry,
derived_geometry, datum_targets, datums, dimensions, tolerances,
tolerance_relations, general, threads, knurls, notes, surface_textures, attributes.
`spellings` retains decimal tokens lost by REAL encoding or exact conversion-unit arithmetic.
Wire-name/fingerprint anchors, support order/aliases and nonempty callout
observations remain because native STEP does not recover those exact JSON forms.
The reader accepts the previous verbose field/default and reference-object forms.

The reader rebuilds collections from independently decoded native items and
verifies every resolved content key and spelling before restoration. Corrupt or
stale records are refused. Content digests and alias bindings remain necessary:
native SETs and shared carriers cannot recover caller order or distinguish equal
support projections. The carrier lookup cache is crate-private and excluded
from `PmiRead` equality.

`pmi::write::differences` retains the ordinary STEP writer's comparison of resolved
meaning as sets, accepting equivalent support aliases and collection order.
`differences_as_stated` additionally compares decimal text and units, accepting
only required REAL spelling changes (integer mantissas/exponent case) and exact
unit-factor arithmetic. Legal REAL precision remains significant.
The JSON-only `json::identity_differences` compares every `PartPmi` field exactly,
including collection membership, order, references, null descriptions and notes
becoming attributes. JSON write verification, CLI import and JSON identity tests
use this strict comparison; external ids, unit tables and wire anchors also have
separate identity checks.

Import preserves foreign graphics linked to surviving unchanged semantic items;
only presentation of removed/replaced requirements may be removed, and each
removal is reported. Unchanged file graphics and their dependency graphs are
reused with rebinding to the surviving IDs. Separate As Drawn linking models
preserve multiple source associations without violating IIRU UR1/UR2; the
original graphics, placements and view containers remain. JSON-generated callouts carry the association description
`haecceity.generated/1` (CAx-IF §7.3 permits this description); read-back labels
them `source: generated`. Unmarked graphics remain `source: file`.
Generated graphics use the same geometry-based placement as the STEP writer,
in model coordinates; producing placed graphics requires the part geometry.

Anonymous EXPRESS units in `OtherMeasure.unit` use a `step-unit:` content identity.
It encodes the unit graph, including named-unit kinds, SI names and prefixes,
conversion factors, dimensions, and derived-unit elements and exponents. Derived
SETs are sorted by content; instance ids are excluded. Import reuses an equal unit
in the destination or writes the encoded unit fresh, including when B-rep writing
creates a new STEP document. Duplicate equal units are interchangeable. Legacy
name-only units still require exactly one matching source unit.

Dimensions without a stated nominal value retain their features and graphical
callout links, with `nominal: null`; no value is invented and no nonconformance
is reported. A projected zone with `end: null` writes an empty shape aspect as
its mandatory projection-end carrier, retaining its length without inventing
origin geometry.

`pmi_json_roundtrip::nist_roundtrip` runs strict CTC/FTC acceptance in CI:
STEP → PartPmi → JSON → PartPmi → STEP, with empty identity differences, schema
and rule checks on additions, and unchanged solid validity. The larger run is
available with `QUIDDITY_CORPUS=/path/to/corpus cargo test -p haecceity --release
--features json-import --test pmi_json_roundtrip nist_and_corpus_roundtrip --
--ignored --exact`. `HAECCEITY_NIST_PMI` optionally adds external NIST copies.

Default-feature readers recover JSON identification-assignment item IDs.

Unit definitions use globally unique caller IDs, or `unit:<kind>:<content-digest>`
fallback IDs. Identical definitions must reuse one ID; duplicate definitions are
refused with a reason. The transport record retains unit IDs, unused unit
definitions, and table order, while quantities retain their native stated units.

Several nonempty `tolerance class` items under one identified property use
`<pdef-id>/tolerance-class/<UTF-8 text as lowercase hex>`. This parent-qualified
content id remains stable when distinct classes are reordered or inserted; only
identical class-text twins receive `~2`, `~3`, … . A dedicated single-class
property keeps its own `ID_ATTRIBUTE` unchanged, including JSON-written caller IDs.

Declared supplemental geometry is emitted in a native constructive geometry
representation even when no feature uses it. JSON-tagged reads recover those
items before applying verified order/alias bindings. Legacy reads/writes retain
their existing behaviour. Computed derived-geometry records still require their
deriving feature; an isolated cache record has no standalone native carrier.

Native verification happens **before** applying order bindings: every requirement
keeps its native values and resolved support, and references to independently
identified datums, dimensions, targets and tolerances must select the exact
native id (including equal-valued twins). Metadata cannot repair a wrong native
reference or hide an additional semantic item; only the redundant anonymous
precision carrier is excluded. JSON writes split datum features from their size
dimensions even without the presentation feature, so note-owner projections are
unambiguous. Ordinary writer output remains frozen.

Corrupt/stale ordering produces `ReadError::Transport`. `pmi::read_native` reads
the complete authoritative AP242 model without ordering/alias restoration, for
audit or recovery. A JSON import replaces stale metadata using those native
values for its removal plan; its new output still undergoes strict verification.

Name and fingerprint anchor spelling, including the grouping of multi-face
support keys, is retained through JSON → STEP → JSON. Transport bindings store
only those wire topology forms and their resolved native support; output refuses
a binding if the current native support no longer matches. The identity test
compares the xcaf producer's original whole JSON, without first replacing its
name anchors with indices. Callers editing a decoded `Semantic` model's topology
must clear or rebuild its `anchor_bindings` rather than reuse stale bindings.

## Source audit: xcaf-pmi (2026-11-01)

The native maximum-value form (§6.9.5) is one geometric tolerance with
`maximum_upper_tolerance`, exposed as `GeometricTolerance.maximum`. FTC-07's NIST
expected rows 24/25 draw MAX, but its STEP contains independent position controls
#14544/#14579 and #14781/#14816, with separate presentation associations and no maximum
subtype or relationship. They remain separate: their visual resemblance is insufficient
to replace the file's independent requirements. The existing NIST verdicts document this
source discrepancy and are unchanged. A synthetic native MAX regression checks one item
with both its magnitude and maximum.

`geometric_tolerance_auxiliary_classification(.ALL_OVER., ...)` leaves the tolerance's
own target intact. There is no `all_over_shape_aspect` in the AP242 schemas used here
(`all_around_shape_aspect` is a different construct). A literal
`product_definition_shape` target remains `WholePart`: PMI practice §6.3 explicitly
requires it for “applies to all surfaces”. NIST FTC-07/FTC-10 use that part target for
their all-over frames. The reader does not replace it with a guessed subset of faces.
An explicit all-over classification on a feature is tested separately and stays scoped
to that feature.

OCCT's fresh export of `cadgen_pmi_plate.xbf` has position tolerance #642, a cylindrical
zone form #643, and a two-attribute `runout_zone_definition` #645. Its orientation #646
is orphaned; the reader does not attach it by proximity. A missing orientation on a
non-runout tolerance is recovered only when the file states an orientation-independent
zone form and no runout boundaries. The model keeps that form and no runout angle, with
a `Nonconformance` finding naming the tolerance and definition. Real runout tolerances,
conical/unknown forms and boundaries requiring the missing orientation remain refused
with a specific reason. The downloaded `30-cadgen_pmi_plate.step` and
`31-xcaf_pmi_plate.step` already contain plain cylindrical zones and read all five
tolerances before this change.

Generated graphics also expose optional additive geometry metadata:
`polyline_roles` is parallel to `polylines`, with `text` (all glyphs represented in readable text cells), `frame`
(cell borders), `leader`, `extension` (measurement witnesses), `symbol` (explicit
geometric symbols, including scope rings), and `dimension` (dimension lines and
arrowheads). File graphics omit this field when their purposes are unknown.
To render `graphic.text[i]` in a host font, remove all `text` geometry. This
includes explicit ISO glyph geometry already encoded in the cell strings (such
as ⌖, ⌀ and Ⓜ); retain `symbol` geometry for standalone marks not encoded in those
strings (such as surface-finish legs and leader scope rings). This avoids drawing
symbols twice. `stroked` alone does not identify which lines are glyphs.

Each generated `text_placements` entry has `cell_box: [[xmin,ymin],[xmax,ymax]]`,
the frame cell or padded text box in millimetres **relative to that placement's
origin**, along `x_axis` and `normal × x_axis`. Its origin is the text's centring
point. Fit host-font text into this box at the stated capital height. File text
placements omit a box unless its extent is known. Viewed from `normal`, glyphs
advance along `+x_axis` and stand up along `normal × x_axis`.

These fields remain optional within `haecceity.presentation/1`; older documents
still deserialize. Consumers using `deny_unknown_fields` must update their
presentation schema/types before consuming documents containing these additions.

A projected zone whose end aspect has no geometry is preserved with `end: null`
and reported as a `fact` finding. Facts are observations, without a conformance
judgement. Value-less dimensions retain stated principles and type qualifiers;
an empty descriptive representation item carries them without a numeric value.

A projected zone whose end aspect has no geometry is preserved with `end: null`
and reported as a `fact` finding. Facts are observations, without a conformance
judgement. Value-less dimensions retain stated principles and type qualifiers;
an empty descriptive representation item carries them without a numeric value.
